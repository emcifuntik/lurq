use std::{cell::Cell, sync::Arc};

use crate::{
  app::{
    component::Component,
    ctx::Ctx,
    theme::{MarkdownBlockStyle, ThemeMarkdown},
  },
  components::{Column, Rect, Row, ScrollHorizontal, Text},
  core::{Memo, Signal},
  layout::{
    Alignment,
    layout_kind::Justify,
    quad::RichTextSpan,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::{FontWeight, TextAlign, TextStyle},
  },
  markdown::{
    MarkdownBlock, MarkdownCodeBlockKind, MarkdownDocument, MarkdownHeadingLevel, MarkdownInline, MarkdownListItem,
    MarkdownTableAlignment, MarkdownTableRow, markdown_html_text, markdown_inline_rich_text, parse_markdown,
  },
  node::{Element, Node, color::Color, dimension::Dimension, text_highlight::TextHighlight},
};

mod bidi_paragraphs;
mod document_text;
mod link_block;
mod links;
mod list_view;
mod paragraph;
mod sizing;
mod tooltip;

use document_text::DocumentText;
pub use links::{MarkdownLink, MarkdownLinkPresentation, MarkdownRun};
pub use list_view::MarkdownList;
use list_view::MarkdownListView;
use paragraph::{FlowItem, LinkSession, MarkdownLinkActivator, MarkdownLinkCallback, MarkdownLinkPresenter, Paragraph};
use sizing::{BlockWidth, FILL_WIDTH};
use tooltip::{LinkTooltips, LinkTooltipsProps, TooltipHover};

#[derive(Clone)]
pub struct MarkdownProps {
  source: Arc<str>,
  style: TextStyle,
  theme: Option<ThemeMarkdown>,
  selectable: bool,
  width: Option<Dimension>,
  fit_content: bool,
  on_link_click: Option<MarkdownLinkCallback>,
  on_link_activate: Option<MarkdownLinkActivator>,
  link_presentation: Option<MarkdownLinkPresenter>,
  list_view: Option<MarkdownListView>,
  highlights: Arc<[TextHighlight]>,
}

impl PartialEq for MarkdownProps {
  fn eq(&self, other: &Self) -> bool {
    self.source == other.source
      && self.style == other.style
      && self.theme == other.theme
      && self.selectable == other.selectable
      && self.width == other.width
      && self.fit_content == other.fit_content
      && same_callback(&self.on_link_click, &other.on_link_click)
      && same_callback(&self.on_link_activate, &other.on_link_activate)
      && same_callback(&self.link_presentation, &other.link_presentation)
      && same_callback(&self.list_view, &other.list_view)
      && self.highlights == other.highlights
  }
}

impl MarkdownProps {
  pub fn new(source: impl Into<Arc<str>>) -> Self {
    Self {
      source: source.into(),
      style: TextStyle::default(),
      theme: None,
      selectable: false,
      width: None,
      fit_content: false,
      on_link_click: None,
      on_link_activate: None,
      link_presentation: None,
      list_view: None,
      highlights: Arc::from([]),
    }
  }

  pub fn styled(source: impl Into<Arc<str>>, style: TextStyle) -> Self {
    Self::new(source).style(style)
  }

  pub fn style(mut self, style: TextStyle) -> Self {
    self.style = style;
    self
  }

  pub fn theme(mut self, theme: ThemeMarkdown) -> Self {
    self.theme = Some(theme);
    self
  }

  /// Fills behind ranges of the document's words, such as the matches of a
  /// search: one per laid-out line a range covers, each in its own color,
  /// under the glyphs and under a selection. Ranges are in bytes of the
  /// document's rendered text ([`Self::rendered_text`]), not of its source;
  /// a range may run over several paragraphs. Links, code and lists render
  /// as without them.
  pub fn highlights(mut self, highlights: impl Into<Arc<[TextHighlight]>>) -> Self {
    self.highlights = highlights.into();
    self
  }

  /// The document's words as it renders them, the text
  /// [`Self::highlights`] ranges index: each paragraph, heading, table cell,
  /// code block and HTML block in document order, one line feed between
  /// two. List markers, footnote labels and a code block's language are not
  /// in it, nor link marks added for display; a link is its words. Search
  /// this text to find what to highlight. Built with these props' link
  /// presentation and list view; it renders the document to find it.
  pub fn rendered_text(&self) -> String {
    let document = parse_markdown(&self.source);
    let theme = self.theme.clone().unwrap_or_default();
    let links = LinkSession::new(
      self.link_presentation.clone(),
      None,
      None,
      #[cfg(feature = "router")]
      None,
      TooltipHover::new(crate::app::ctx::Timeout::detached),
    );
    let text = DocumentText::new(&[]);
    let render_ctx = MarkdownRenderContext {
      width: BlockWidth::new(self.fit_content),
      links: &links,
      list_view: self.list_view.as_ref(),
      code_block_scrollbar: ScrollBarStyle::default(),
      code_blocks: Cell::new(0),
      text: &text,
    };
    let _ = render_document(&document, &self.style, &theme, &render_ctx);
    text.into_text()
  }

  pub fn selectable(mut self, selectable: bool) -> Self {
    self.selectable = selectable;
    self
  }

  pub fn width(mut self, width: impl Into<Dimension>) -> Self {
    self.width = Some(width.into());
    self
  }

  /// Sizes the document to its content instead of filling its container,
  /// like CSS `width: fit-content; max-width: 100%`: the document is as wide
  /// as its widest block, and never wider than its container, where its
  /// text wraps. Off by default: the document and every block in it fill the
  /// container's width.
  ///
  /// A short message is as wide as its longest line, so a chat bubble around
  /// it hugs its text. Every block is then stretched to the document's
  /// width, as CSS blocks are: a code block's box and a rule span the widest
  /// block. A code block does not wrap, so it is as wide as its longest line
  /// up to the container's width, and a longer line scrolls sideways inside
  /// its box, as when filling. A table still fills the container's width, since its
  /// columns share that width and line up across its rows. An explicit
  /// [`width`](Self::width) sets the document's width as before.
  ///
  /// The container's width is the most a `Column` gives its children. A `Row`
  /// gives its children unbounded width, so a bubble in a row needs a
  /// `max_width` of `Dimension::Pct(100.0)` (or a `flex_shrink` factor) for
  /// its text to wrap at the row's width.
  pub fn fit_content(mut self, fit_content: bool) -> Self {
    self.fit_content = fit_content;
    self
  }

  /// Called when a link is followed: clicked with the left button, or Enter
  /// while it has keyboard focus. It runs inside the input's dispatch, so
  /// [`is_synthetic_input`](crate::app::events::is_synthetic_input) tells a
  /// person's press from synthetic input (an MCP client's). It is given the
  /// same [`MarkdownLink`] the presentation hook saw, with its
  /// [`index`](MarkdownLink::index). Without it and without
  /// [`on_link_activate`](Self::on_link_activate), a link to an in-app path
  /// (`/`, `#` or `?`) navigates the router, when there is one.
  pub fn on_link_click(mut self, f: impl Fn(&MarkdownLink) + Send + Sync + 'static) -> Self {
    self.on_link_click = Some(Arc::new(f));
    self
  }

  /// Called when a link is activated, with the input that did it: a left
  /// click, a middle click, or Enter while the link has keyboard focus. The
  /// [`MouseEvent`](crate::app::events::MouseEvent) says which: its `button`
  /// (`Left` or `Middle`; `Left` for Enter), its modifiers (`shift`, `ctrl`,
  /// `alt`, and `meta`: Cmd on macOS, the Windows key on Windows) and
  /// `from_keyboard` for Enter. An app follows browser rules with it: a
  /// middle click or Ctrl+click (Cmd+click on macOS) opens the link in the
  /// background, Ctrl+Shift+click opens it in front.
  ///
  /// A middle click is a press and a release of the middle button on the
  /// same link, like a left click. Its press starts no text selection or
  /// drag. A press released off the link activates nothing, and neither
  /// does a press on one link released on another more than a few pixels
  /// away; closer, the link under the release activates, as with a left
  /// click.
  /// The callback runs inside the input's dispatch, so
  /// [`is_synthetic_input`](crate::app::events::is_synthetic_input) tells a
  /// person from an MCP client here too.
  ///
  /// It runs before [`on_link_click`](Self::on_link_click), which still
  /// gets left clicks and Enter when both are set. With it, a link to an
  /// in-app path no longer navigates the router by itself.
  pub fn on_link_activate(
    mut self,
    f: impl Fn(&MarkdownLink, &crate::app::events::MouseEvent) + Send + Sync + 'static,
  ) -> Self {
    self.on_link_activate = Some(Arc::new(f));
    self
  }

  /// Decides how each link is shown: as a link with its own looks, trailing
  /// runs and tooltip, or as text ([`MarkdownLinkPresentation`]). It is
  /// called once per link and render with the exact link the document
  /// follows, so what a link shows always comes from the destination it
  /// opens. Without it, every link is a link in the theme's looks.
  ///
  /// A link stays inside its paragraph's text, which wraps and selects as
  /// one. A link is a Tab stop (in document order), Enter and a click follow
  /// it, and the pointer over it shows a pointer cursor. Its hovered and
  /// focused looks recolor its words, underline and trailing runs and can
  /// fill behind its words; keyboard focus also draws a ring around them. A
  /// link presented with [`MarkdownLinkPresentation::pointer_only`] is no
  /// Tab stop and takes no focus.
  ///
  /// Keep the props (or the closure's `Arc`) across renders where you can:
  /// props with a new closure compare unequal and render the document again.
  pub fn link_presentation(
    mut self,
    f: impl Fn(&MarkdownLink) -> MarkdownLinkPresentation + Send + Sync + 'static,
  ) -> Self {
    self.link_presentation = Some(Arc::new(f));
    self
  }

  /// Lets the app draw a list block itself: called for every list of the
  /// document, it returns the element to draw instead, or `None` to draw the
  /// list as usual. Decide from [`MarkdownList::items`] before drawing
  /// anything with the list's `inlines` or `blocks`, since drawing numbers
  /// the links it draws.
  pub fn list_view(mut self, f: impl Fn(&MarkdownList<'_>) -> Option<Element> + Send + Sync + 'static) -> Self {
    self.list_view = Some(Arc::new(f));
    self
  }
}

fn same_callback<T: ?Sized>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
  match (left, right) {
    (None, None) => true,
    (Some(left), Some(right)) => Arc::ptr_eq(left, right),
    _ => false,
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for MarkdownProps {
  fn write_info(&self, buffer: &mut Vec<crate::app::component::ComponentInfo>) {
    buffer.push(crate::app::component::ComponentInfo::with_value(
      "source",
      std::any::type_name::<Arc<str>>(),
      format!("len={}", self.source.len()),
    ));
  }
}

pub struct Markdown {
  source: Signal<Arc<str>>,
  document: Memo<MarkdownDocument>,
  hover: TooltipHover,
}

struct MarkdownRenderContext<'a> {
  width: BlockWidth,
  links: &'a LinkSession,
  list_view: Option<&'a MarkdownListView>,
  /// The bar of a code block that overflows sideways: the Markdown theme's,
  /// or else the app theme's, always drawn over the box's padding so a
  /// fitting block keeps its size whatever the app's bars reserve.
  code_block_scrollbar: ScrollBarStyle,
  /// How many code blocks this render has drawn: the next one's ordinal.
  code_blocks: Cell<usize>,
  /// The rendered text so far, and the highlights its leaves take.
  text: &'a DocumentText<'a>,
}

impl Markdown {
  pub fn mount(ctx: &mut Ctx, props: MarkdownProps) -> Element {
    ctx.mount::<Self>(props)
  }
}

impl Component for Markdown {
  type Props = MarkdownProps;

  fn create(ctx: &mut Ctx) -> Self {
    let source = ctx.signal(ctx.props::<Self::Props>().source.clone());
    let memo_source = source.clone();
    let document = ctx.memo(move || parse_markdown(&memo_source.get()));
    let hover = TooltipHover::new(|fire| ctx.create_timeout(std::time::Duration::ZERO, fire));
    Self {
      source,
      document,
      hover,
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    if self.source.get_untracked() != props.source {
      self.source.set(props.source.clone());
    }
    let theme = props.theme.clone().unwrap_or_else(|| ctx.theme().markdown().clone());
    self.hover.set_delay(theme.link_tooltip_delay);
    let document = self.document.get();
    let links = LinkSession::new(
      props.link_presentation.clone(),
      props.on_link_click.clone(),
      props.on_link_activate.clone(),
      #[cfg(feature = "router")]
      ctx.use_context::<crate::router::Navigator>(),
      self.hover.clone(),
    );
    let code_block_scrollbar = ScrollBarStyle {
      placement: ScrollBarPlacement::Overlay,
      ..theme
        .code_block_scrollbar
        .clone()
        .unwrap_or_else(|| ctx.theme().scrollbar().clone())
    };
    let text = DocumentText::new(&props.highlights);
    let render_ctx = MarkdownRenderContext {
      width: BlockWidth::new(props.fit_content),
      links: &links,
      list_view: props.list_view.as_ref(),
      code_block_scrollbar,
      code_blocks: Cell::new(0),
      text: &text,
    };
    // A style that keeps the default text color names none: the document is drawn in the theme's foreground.
    let mut base_style = props.style.clone();
    base_style.color = ctx.theme().palette().text_color(&base_style);
    let mut root = render_document(&document, &base_style, &theme, &render_ctx);
    let tooltips = links.take_tooltips();
    if !tooltips.is_empty() {
      root = root.child(ctx.mount::<LinkTooltips>(LinkTooltipsProps {
        tooltips: tooltips.into(),
        shown: self.hover.shown(),
        style: base_style,
        text: theme.link_tooltip.clone(),
        panel: theme.link_tooltip_box.clone(),
      }));
    }
    let mut element: Element = root.into();

    if let Some(width) = props.width {
      element = element.map_node(|node| node.width(width));
    }
    if props.selectable {
      element.node.selectable_recursive(true);
    }
    element
  }
}

/// The document's own containers (the document, flows, lists, quotes,
/// footnotes) let what their blocks draw past them show, as CSS blocks do: a
/// link's focus ring reaches outside its words. Code blocks and tables keep
/// clipping to their boxes.
fn render_document(
  document: &MarkdownDocument,
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Column {
  let mut root = render_ctx
    .width
    .blocks(Column::new().overflow_visible().spacing(theme.document_spacing));
  for block in &document.blocks {
    root = root.child(render_block(block, base_style, theme, render_ctx));
  }
  root
}

fn render_block(
  block: &MarkdownBlock,
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  match block {
    MarkdownBlock::Paragraph(inlines) => render_inline_block(inlines, base_style.clone(), theme, render_ctx),
    MarkdownBlock::Heading { level, children } => {
      render_inline_block(children, heading_style(theme, *level, base_style), theme, render_ctx)
    }
    MarkdownBlock::BlockQuote(blocks) => render_blockquote(blocks, base_style, theme, render_ctx),
    MarkdownBlock::List { ordered, start, items } => {
      render_list(*ordered, *start, items, base_style, theme, render_ctx)
    }
    MarkdownBlock::Table { alignments, rows } => render_table(alignments, rows, base_style, theme, render_ctx),
    MarkdownBlock::CodeBlock { kind, text } => render_code_block(kind, text, base_style, theme, render_ctx),
    MarkdownBlock::Math { text } => render_math_block(text, base_style, theme, render_ctx),
    MarkdownBlock::FootnoteDefinition { label, blocks } => {
      render_footnote_definition(label, blocks, base_style, theme, render_ctx)
    }
    MarkdownBlock::Html(text) => render_ctx.width.leaf(render_ctx.text.leaf(Text::styled(
      &markdown_html_text(text.trim_end_matches('\n')),
      base_style.clone(),
    ))),
    MarkdownBlock::ThematicBreak => render_ctx
      .width
      .leaf(Rect::new(Dimension::Auto, 1.0).background(theme.table_marker.apply(base_style).color)),
  }
}

/// A paragraph, a heading or a table cell: one rich-text node, links in it.
/// Images other than a lone one break it into a wrapping row of texts and
/// images.
fn render_inline_block(
  inlines: &[MarkdownInline],
  style: TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  if let Some(image) = single_image(inlines) {
    return render_markdown_image(image, &style, theme);
  }

  if let [MarkdownInline::Text(text)] = inlines {
    return render_ctx.width.leaf(render_ctx.text.leaf(Text::styled(text, style)));
  }

  if has_image(inlines) {
    return render_inline_flow(inlines, style, theme, render_ctx);
  }

  let mut paragraph = Paragraph::new(inlines, false, theme, render_ctx.links);
  paragraph.push_inlines(inlines, &style, &mut None);
  render_ctx
    .width
    .leaf(render_ctx.text.leaf(Element::from_node(paragraph.take_node())))
}

fn render_inline_flow(
  inlines: &[MarkdownInline],
  style: TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let mut hard_break_segments = inlines.split(|inline| matches!(inline, MarkdownInline::HardBreak));
  let Some(first_segment) = hard_break_segments.next() else {
    return render_inline_flow_row(inlines, &style, theme, render_ctx);
  };

  let remaining_segments = hard_break_segments.collect::<Vec<_>>();
  if remaining_segments.is_empty() {
    return render_inline_flow_row(first_segment, &style, theme, render_ctx);
  }

  let mut column = render_ctx
    .width
    .blocks(Column::new().overflow_visible().spacing(0.0).min_width(0.0));
  column = column.child(render_inline_flow_row(first_segment, &style, theme, render_ctx));
  for segment in remaining_segments {
    column = column.child(render_inline_flow_row(segment, &style, theme, render_ctx));
  }
  column.into()
}

/// A wrapping row of the texts between images, and the images.
fn render_inline_flow_row(
  inlines: &[MarkdownInline],
  style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let mut flow = Vec::new();
  let mut paragraph = Paragraph::new(inlines, true, theme, render_ctx.links);
  paragraph.push_inlines(inlines, style, &mut Some(&mut flow));
  if !paragraph.is_empty() {
    flow.push(FlowItem::Text(Box::new(paragraph.take_node())));
  }
  let mut row = render_ctx.width.row(
    Row::new()
      .overflow_visible()
      .wrap()
      .spacing(0.0)
      .align_items(Alignment::Center)
      .min_width(0.0),
  );
  for item in flow {
    row = row.child(match item {
      FlowItem::Text(mut node) => {
        render_ctx.text.append(&mut node);
        Element::from_node((*node).max_width(FILL_WIDTH).min_width(0.0))
      }
      FlowItem::Image(image, style) => render_markdown_image(image, &style, theme),
    });
  }
  row.into()
}

/// Whether an image outside any link is among `inlines`: an image in a link
/// is part of the link's words (its alt text).
fn has_image(inlines: &[MarkdownInline]) -> bool {
  inlines.iter().any(|inline| match inline {
    MarkdownInline::Image { .. } => true,
    MarkdownInline::Emphasis(children) | MarkdownInline::Strong(children) | MarkdownInline::Strikethrough(children) => {
      has_image(children)
    }
    _ => false,
  })
}

fn single_image(inlines: &[MarkdownInline]) -> Option<&MarkdownInline> {
  let mut image = None;
  for inline in inlines {
    match inline {
      MarkdownInline::Image { .. } if image.is_none() => image = Some(inline),
      MarkdownInline::Text(text) if text.trim().is_empty() => {}
      MarkdownInline::SoftBreak | MarkdownInline::HardBreak => {}
      _ => return None,
    }
  }
  image
}

fn render_markdown_image(inline: &MarkdownInline, base_style: &TextStyle, theme: &ThemeMarkdown) -> Element {
  let MarkdownInline::Image {
    destination,
    title: _,
    alt,
  } = inline
  else {
    return Element::from_node(Node::rich_text(markdown_inline_rich_text(
      std::slice::from_ref(inline),
      base_style,
      theme,
    )));
  };
  let _ = destination;
  let _ = alt;

  #[cfg(all(feature = "image", feature = "resources"))]
  {
    return crate::components::Image::from_resource(destination)
      .max_width(FILL_WIDTH)
      .into();
  }

  #[cfg(not(all(feature = "image", feature = "resources")))]
  {
    let mut style = theme.inline_code.apply(base_style);
    if theme.inline_code.text.color.is_none() {
      style.color = Color::from_hex("#64748b");
    }
    Element::from_node(Node::rich_text(markdown_inline_rich_text(alt, &style, theme)))
  }
}

fn render_blockquote(
  blocks: &[MarkdownBlock],
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let mut body = render_ctx.width.row_body(
    Column::new()
      .overflow_visible()
      .spacing(theme.nested_block_spacing)
      .min_width(0.0),
  );
  for block in blocks {
    body = body.child(render_block(block, base_style, theme, render_ctx));
  }

  let bar_color = theme
    .blockquote_box
    .border_color
    .unwrap_or_else(|| theme.blockquote_marker.apply(base_style).color);
  let bar_width = theme.blockquote_box.border_width.unwrap_or(3.0);

  let mut row = render_ctx.width.row(
    Row::new()
      .overflow_visible()
      .spacing(theme.blockquote_gap)
      .align_items(Alignment::Stretch)
      .child(Rect::new(bar_width, FILL_WIDTH).background(bar_color))
      .child(body),
  );
  row = apply_row_box(row, &theme.blockquote_box, false);
  row.into()
}

fn render_list(
  ordered: bool,
  start: Option<u64>,
  items: &[MarkdownListItem],
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  if let Some(list_view) = render_ctx.list_view {
    let list = MarkdownList {
      ordered,
      start,
      items,
      style: base_style,
      theme,
      render_ctx,
    };
    // A view that declines draws nothing that stays: forget the links it
    // drew and the code blocks it counted to decide, so the list's own are
    // numbered as if it had not.
    let checkpoint = render_ctx.links.checkpoint();
    let code_blocks = render_ctx.code_blocks.get();
    if let Some(element) = list_view(&list) {
      return element;
    }
    render_ctx.links.restore(checkpoint);
    render_ctx.code_blocks.set(code_blocks);
  }
  let mut list = render_ctx
    .width
    .blocks(Column::new().overflow_visible().spacing(theme.list_item_spacing));
  let start = start.unwrap_or(1);
  for (index, item) in items.iter().enumerate() {
    let marker = if ordered {
      format!("{}.", start + index as u64)
    } else {
      "-".to_owned()
    };
    let marker_style = theme.list_marker.apply(base_style);
    let marker_width = if ordered {
      theme.ordered_list_marker_width
    } else {
      theme.unordered_list_marker_width
    };
    let mut body = render_ctx.width.row_body(
      Column::new()
        .overflow_visible()
        .spacing(theme.nested_block_spacing)
        .min_width(0.0),
    );
    for block in &item.blocks {
      body = body.child(render_block(block, base_style, theme, render_ctx));
    }
    list = list.child(
      render_ctx.width.row(
        Row::new()
          .overflow_visible()
          .spacing(theme.list_marker_gap)
          .align_items(Alignment::Start)
          .child(Text::styled(&marker, marker_style).width(marker_width))
          .child(body),
      ),
    );
  }
  list.into()
}

fn render_code_block(
  kind: &MarkdownCodeBlockKind,
  text: &str,
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let width = render_ctx.width;
  let mut code_style = theme.code_block.apply(base_style);
  if theme.code_block.text.color.is_none() {
    code_style.color = Color::from_hex("#e2e8f0");
  }

  // The box's padding goes inside the lines' scroll container, so a line
  // wider than the box scrolls under the padding and is clipped at the
  // box's edge, like a CSS `pre` with `overflow-x: auto`, and the scrollbar
  // sits in the bottom padding. The label stays above, outside the scroll.
  let padding = theme.code_block_box.padding.unwrap_or_default();
  let mut lines_padding = padding;
  let ordinal = render_ctx.code_blocks.get();
  render_ctx.code_blocks.set(ordinal + 1);
  let key = code_block_key(ordinal, kind);
  let mut column = width.blocks(Column::new().spacing(0.0).key(key.clone()));
  if let MarkdownCodeBlockKind::Fenced {
    language: Some(language),
  } = kind
  {
    let mut label_style = theme.code_block_label.apply(base_style);
    if theme.code_block_label.text.color.is_none() {
      label_style.color = Color::from_hex("#94a3b8");
    }
    let label = width.blocks(
      Column::new()
        .overflow_visible()
        .padding(padding.bottom(0.0))
        .child(width.leaf(Text::styled(language, label_style))),
    );
    column = column.child(label);
    lines_padding = lines_padding.top(theme.code_block_spacing);
  }
  let lines = Column::new()
    .overflow_visible()
    .padding(lines_padding)
    .child(render_ctx.text.leaf(Element::from_node(
      Node::rich_text(highlight_code_spans(text.trim_end_matches('\n'), kind, &code_style)).text_wrap(false),
    )));
  let scroll = ScrollHorizontal::new(lines)
    .key(key)
    .scrollbar(render_ctx.code_block_scrollbar.clone());
  column = column.child(width.leaf(scroll));
  let box_style = MarkdownBlockStyle {
    padding: None,
    ..theme.code_block_box.clone()
  };
  column = apply_column_box(column, &box_style, true);
  column.into()
}

/// Keys a code block's box and its scroll node by the block's ordinal among
/// the document's code blocks and its language: the key holds while the
/// block's code streams in, token by token, and while paragraphs are added
/// or removed around it, so a scrolled block keeps its scroll and a
/// selection in it, and its node ids and MCP refs stay. A code block
/// inserted above it or removed from above it shifts the ordinals, and the
/// blocks after it take the scroll of the block that had their ordinal.
fn code_block_key(ordinal: usize, kind: &MarkdownCodeBlockKind) -> Arc<str> {
  match kind {
    MarkdownCodeBlockKind::Fenced {
      language: Some(language),
    } => format!("markdown-code:{ordinal}:{language}").into(),
    _ => format!("markdown-code:{ordinal}").into(),
  }
}

fn render_math_block(
  text: &str,
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let kind = MarkdownCodeBlockKind::Fenced {
    language: Some("math".to_owned()),
  };
  render_code_block(&kind, text, base_style, theme, render_ctx)
}

fn render_footnote_definition(
  label: &str,
  blocks: &[MarkdownBlock],
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let marker_style = theme.link.apply(base_style);
  let mut body = render_ctx
    .width
    .row_body(Column::new().overflow_visible().spacing(theme.nested_block_spacing));
  for block in blocks {
    body = body.child(render_block(block, base_style, theme, render_ctx));
  }

  render_ctx
    .width
    .row(
      Row::new()
        .overflow_visible()
        .spacing(theme.list_marker_gap)
        .align_items(Alignment::Start)
        .child(Text::styled(&format!("[^{label}]"), marker_style).width(theme.ordered_list_marker_width))
        .child(body),
    )
    .into()
}

fn highlight_code_spans(text: &str, kind: &MarkdownCodeBlockKind, base_style: &TextStyle) -> Vec<RichTextSpan> {
  if !is_rust_like_code(kind) {
    return vec![RichTextSpan {
      text: text.to_owned(),
      style: base_style.clone(),
    }];
  }

  let keyword_style = code_style(base_style, Color::from_hex("#93c5fd"));
  let string_style = code_style(base_style, Color::from_hex("#86efac"));
  let comment_style = code_style(base_style, Color::from_hex("#94a3b8"));
  let number_style = code_style(base_style, Color::from_hex("#fca5a5"));
  let mut spans = Vec::new();
  let mut rest = text;
  while !rest.is_empty() {
    if let Some(comment) = rest.strip_prefix("//") {
      let line_len = comment.find('\n').map(|index| index + 2).unwrap_or(rest.len());
      push_code_span(&mut spans, &rest[..line_len], comment_style.clone());
      rest = &rest[line_len..];
      continue;
    }
    let Some(ch) = rest.chars().next() else {
      break;
    };
    if ch == '"' {
      let len = quoted_len(rest);
      push_code_span(&mut spans, &rest[..len], string_style.clone());
      rest = &rest[len..];
      continue;
    }
    if ch.is_ascii_digit() {
      let len = rest
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit() && *ch != '.')
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
      push_code_span(&mut spans, &rest[..len], number_style.clone());
      rest = &rest[len..];
      continue;
    }
    if ch == '_' || ch.is_ascii_alphabetic() {
      let len = rest
        .char_indices()
        .find(|(_, ch)| *ch != '_' && !ch.is_ascii_alphanumeric())
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
      let token = &rest[..len];
      let style = if is_rust_keyword(token) {
        keyword_style.clone()
      } else {
        base_style.clone()
      };
      push_code_span(&mut spans, token, style);
      rest = &rest[len..];
      continue;
    }
    push_code_span(&mut spans, &rest[..ch.len_utf8()], base_style.clone());
    rest = &rest[ch.len_utf8()..];
  }
  spans
}

fn is_rust_like_code(kind: &MarkdownCodeBlockKind) -> bool {
  matches!(
    kind,
    MarkdownCodeBlockKind::Fenced {
      language: Some(language)
    } if matches!(language.as_str(), "rust" | "rs")
  )
}

fn quoted_len(text: &str) -> usize {
  let mut escaped = false;
  for (index, ch) in text.char_indices().skip(1) {
    if escaped {
      escaped = false;
      continue;
    }
    if ch == '\\' {
      escaped = true;
      continue;
    }
    if ch == '"' {
      return index + ch.len_utf8();
    }
  }
  text.len()
}

fn is_rust_keyword(token: &str) -> bool {
  matches!(
    token,
    "as"
      | "async"
      | "await"
      | "break"
      | "const"
      | "continue"
      | "crate"
      | "else"
      | "enum"
      | "false"
      | "fn"
      | "for"
      | "if"
      | "impl"
      | "in"
      | "let"
      | "loop"
      | "match"
      | "mod"
      | "move"
      | "mut"
      | "pub"
      | "ref"
      | "return"
      | "self"
      | "Self"
      | "static"
      | "struct"
      | "super"
      | "trait"
      | "true"
      | "type"
      | "unsafe"
      | "use"
      | "where"
      | "while"
  )
}

fn code_style(base_style: &TextStyle, color: Color) -> TextStyle {
  let mut style = base_style.clone();
  style.color = color;
  style
}

fn push_code_span(spans: &mut Vec<RichTextSpan>, text: &str, style: TextStyle) {
  if text.is_empty() {
    return;
  }
  if let Some(last) = spans.last_mut()
    && last.style == style
  {
    last.text.push_str(text);
    return;
  }
  spans.push(RichTextSpan {
    text: text.to_owned(),
    style,
  });
}

fn render_table(
  alignments: &[MarkdownTableAlignment],
  rows: &[MarkdownTableRow],
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let mut table = Column::new().width(FILL_WIDTH);
  let line_color = table_line_color(theme);
  for (row_index, row) in rows.iter().enumerate() {
    if row_index > 0
      && theme.table_row_spacing > 0.0
      && let Some(line_color) = line_color
    {
      table = table.child(Rect::new(FILL_WIDTH, theme.table_row_spacing).background(line_color));
    }
    let mut row_node = Row::new().align_items(Alignment::Stretch).width(FILL_WIDTH);
    for (cell_index, cell) in row.cells.iter().enumerate() {
      if cell_index > 0
        && theme.table_column_spacing > 0.0
        && let Some(line_color) = line_color
      {
        row_node = row_node.child(Rect::new(theme.table_column_spacing, FILL_WIDTH).background(line_color));
      }
      let alignment = alignments.get(cell_index).copied().unwrap_or_default();
      row_node = row_node.child(render_table_cell(
        row.header, cell, alignment, base_style, theme, render_ctx,
      ));
    }
    table = table.child(row_node);
  }
  table = apply_column_box(table, &theme.table_box, true);
  table.into()
}

fn table_line_color(theme: &ThemeMarkdown) -> Option<Color> {
  theme.table_box.border_color.or(theme.table_box.background)
}

fn render_table_cell(
  header: bool,
  cell: &[MarkdownInline],
  alignment: MarkdownTableAlignment,
  base_style: &TextStyle,
  theme: &ThemeMarkdown,
  render_ctx: &MarkdownRenderContext<'_>,
) -> Element {
  let mut style = if header {
    theme.table_header.apply(base_style)
  } else {
    theme.table_cell.apply(base_style)
  };
  if header {
    style.weight = FontWeight::Bold;
  }
  style.text_align = table_text_align(alignment);

  let box_style = if header {
    &theme.table_header_box
  } else {
    &theme.table_cell_box
  };
  if box_style.background.is_some() && style.color == base_style.color {
    style.color = Color::from_hex("#0f172a");
  }
  // The words are the cell's content, not the cell (see `apply_column_box`),
  // centered in its height as a stretched text leaf centers its words.
  let content = Column::new()
    .overflow_visible()
    .align_items(table_cell_alignment(alignment))
    .justify(Justify::Center)
    .child(render_inline_block(cell, style, theme, render_ctx));
  apply_column_box(content, box_style, true)
    .min_width(theme.table_cell_min_width)
    .flex(1.0)
    .into()
}

fn table_text_align(alignment: MarkdownTableAlignment) -> TextAlign {
  match alignment {
    MarkdownTableAlignment::None | MarkdownTableAlignment::Left => TextAlign::Left,
    MarkdownTableAlignment::Center => TextAlign::Center,
    MarkdownTableAlignment::Right => TextAlign::Right,
  }
}

/// Where a cell puts content narrower than the cell (an image, a fitting
/// text).
fn table_cell_alignment(alignment: MarkdownTableAlignment) -> Alignment {
  match alignment {
    MarkdownTableAlignment::None | MarkdownTableAlignment::Left => Alignment::Start,
    MarkdownTableAlignment::Center => Alignment::Center,
    MarkdownTableAlignment::Right => Alignment::End,
  }
}

/// Gives `column` the box `style`. A Markdown box is always a container
/// around its content, never the content itself: a `Text` or `RichText` leaf
/// with padding grows by it, but lurq paints the leaf's words over its whole
/// rect, padding included, so the words would start at the box's edge.
fn apply_column_box(mut column: Column, style: &MarkdownBlockStyle, include_border: bool) -> Column {
  if let Some(padding) = style.padding {
    column = column.padding(padding);
  }
  if let Some(background) = style.background {
    column = column.background(background);
  }
  if include_border && let (Some(width), Some(color)) = (style.border_width, style.border_color) {
    column = column.border_inside(width, color);
  }
  if let Some(radius) = style.radius {
    column = column.rounded(radius);
  }
  column
}

/// Gives `row` the box `style`; like [`apply_column_box`], never a text leaf.
fn apply_row_box(mut row: Row, style: &MarkdownBlockStyle, include_border: bool) -> Row {
  if let Some(padding) = style.padding {
    row = row.padding(padding);
  }
  if let Some(background) = style.background {
    row = row.background(background);
  }
  if include_border && let (Some(width), Some(color)) = (style.border_width, style.border_color) {
    row = row.border_inside(width, color);
  }
  if let Some(radius) = style.radius {
    row = row.rounded(radius);
  }
  row
}

fn heading_style(theme: &ThemeMarkdown, level: MarkdownHeadingLevel, base_style: &TextStyle) -> TextStyle {
  let shared = theme.heading.apply(base_style);
  match level {
    MarkdownHeadingLevel::H1 => theme.heading_1.apply(&shared),
    MarkdownHeadingLevel::H2 => theme.heading_2.apply(&shared),
    MarkdownHeadingLevel::H3 => theme.heading_3.apply(&shared),
    MarkdownHeadingLevel::H4 => theme.heading_4.apply(&shared),
    MarkdownHeadingLevel::H5 => theme.heading_5.apply(&shared),
    MarkdownHeadingLevel::H6 => theme.heading_6.apply(&shared),
  }
}
