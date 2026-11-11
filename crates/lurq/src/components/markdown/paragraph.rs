//! The paragraph that draws a document's links.
//!
//! A paragraph is one rich-text node, links included, so it wraps and selects
//! as one text. Each link is a child node of the paragraph placed on its words
//! ([`crate::node::inline_link`]): a Tab stop that Enter and a click follow
//! (a middle click too, for an app that takes link activations), with a
//! pointer cursor, hover and focus looks and an optional tooltip. A link
//! presented as the pointer's only takes no focus: a click alone follows it.
//!
//! A link's words and trailing runs are laid out between invisible marks for
//! the direction of the bidi paragraph it is in ([`super::link_block`],
//! [`super::bidi_paragraphs`]), so its runs follow its words, left to right,
//! on every laid-out line at any width, and the paragraph's own text still
//! runs its way around the link. A paragraph with a link drops every
//! directional control and mark of its own text, before the link too:
//! Unicode bidi nests at most 125 levels deep, and past that it ignores an
//! isolate, so 63 overrides before a link would reverse the host after it
//! again; and a mark would set which way the paragraph and the links in it
//! run. Its other line and paragraph separators are line feeds, and a link's
//! words are one line of text: breaks in them are spaces. The link's marks
//! are then the only directional controls in the paragraph, three isolates
//! deep at most. A link whose words draw nothing (none, or only spaces,
//! invisible characters and combining marks with no letter) is its words as
//! text: no runs, nothing to press.
//! The marks, joiners and trailing runs are presentation: the paragraph's
//! copied and inspected text leaves them out.

use std::{
  cell::{Cell, RefCell},
  collections::HashMap,
  ops::Range,
  sync::Arc,
};

use super::{
  bidi_paragraphs::link_directions,
  link_block::LinkBlock,
  links::{MarkdownLink, MarkdownLinkPresentation},
  tooltip::{LinkTooltip, TooltipHover, TooltipKey},
};
use crate::{
  app::{
    events::{MouseButton, MouseEvent},
    theme::ThemeMarkdown,
  },
  layout::{quad::RichTextSpan, text_style::TextStyle},
  markdown::{
    Direction, FIRST_STRONG_ISOLATE, MarkdownInline, POP_DIRECTIONAL_ISOLATE, is_invisible, markdown_inline_rich_text,
    paragraph_breaks_as_line_feeds, paragraph_breaks_as_spaces, strip_bidi_controls,
  },
  node::{CursorIcon, EventHandler, Node, inline_link::InlineLink},
};

pub(super) type MarkdownLinkCallback = Arc<dyn Fn(&MarkdownLink) + Send + Sync>;
pub(super) type MarkdownLinkActivator = Arc<dyn Fn(&MarkdownLink, &MouseEvent) + Send + Sync>;
pub(super) type MarkdownLinkPresenter = Arc<dyn Fn(&MarkdownLink) -> MarkdownLinkPresentation + Send + Sync>;

/// What the links of one render of a document share: their numbering, the
/// app's hooks and the tooltips they register.
pub(super) struct LinkSession {
  next_index: Cell<usize>,
  /// How many links to each destination came so far, for tooltip keys.
  occurrences: RefCell<HashMap<Arc<str>, usize>>,
  presenter: Option<MarkdownLinkPresenter>,
  on_link_click: Option<MarkdownLinkCallback>,
  on_link_activate: Option<MarkdownLinkActivator>,
  #[cfg(feature = "router")]
  navigator: Option<crate::router::Navigator>,
  hover: TooltipHover,
  tooltips: RefCell<Vec<LinkTooltip>>,
}

/// Where a session was, to forget the links drawn after it.
pub(super) struct LinkCheckpoint {
  next_index: usize,
  occurrences: HashMap<Arc<str>, usize>,
  tooltips: usize,
}

impl LinkSession {
  pub(super) fn new(
    presenter: Option<MarkdownLinkPresenter>,
    on_link_click: Option<MarkdownLinkCallback>,
    on_link_activate: Option<MarkdownLinkActivator>,
    #[cfg(feature = "router")] navigator: Option<crate::router::Navigator>,
    hover: TooltipHover,
  ) -> Self {
    Self {
      next_index: Cell::new(0),
      occurrences: RefCell::new(HashMap::new()),
      presenter,
      on_link_click,
      on_link_activate,
      #[cfg(feature = "router")]
      navigator,
      hover,
      tooltips: RefCell::new(Vec::new()),
    }
  }

  /// The tooltips the drawn links registered, in order; anchors of links
  /// that are gone are forgotten.
  pub(super) fn take_tooltips(&self) -> Vec<LinkTooltip> {
    let tooltips = self.tooltips.take();
    self.hover.keep_anchors(&tooltips);
    tooltips
  }

  pub(super) fn checkpoint(&self) -> LinkCheckpoint {
    LinkCheckpoint {
      next_index: self.next_index.get(),
      occurrences: self.occurrences.borrow().clone(),
      tooltips: self.tooltips.borrow().len(),
    }
  }

  /// Forgets the links drawn since `checkpoint` (drawn and then discarded).
  pub(super) fn restore(&self, checkpoint: LinkCheckpoint) {
    self.next_index.set(checkpoint.next_index);
    *self.occurrences.borrow_mut() = checkpoint.occurrences;
    self.tooltips.borrow_mut().truncate(checkpoint.tooltips);
  }

  fn next_link(&self, destination: &str, title: &str, words: String) -> (MarkdownLink, TooltipKey) {
    let index = self.next_index.get();
    self.next_index.set(index + 1);
    let destination: Arc<str> = Arc::from(destination);
    let occurrence = {
      let mut occurrences = self.occurrences.borrow_mut();
      let count = occurrences.entry(destination.clone()).or_default();
      *count += 1;
      *count - 1
    };
    let link = MarkdownLink {
      destination: destination.clone(),
      title: Arc::from(title),
      text: Arc::from(words),
      index,
    };
    (link, TooltipKey::new(destination, occurrence))
  }

  fn present(&self, link: &MarkdownLink) -> MarkdownLinkPresentation {
    self
      .presenter
      .as_ref()
      .map_or_else(MarkdownLinkPresentation::link, |present| present(link))
  }

  /// What a click, a middle click or Enter on `link` does: the app's
  /// callbacks, or else, for a left click or Enter, a router navigation to
  /// an in-app path.
  fn activation_handler(&self, link: MarkdownLink) -> EventHandler<MouseEvent> {
    let on_link_activate = self.on_link_activate.clone();
    let on_link_click = self.on_link_click.clone();
    #[cfg(feature = "router")]
    let navigator = self.navigator.clone();
    EventHandler::new(move |event: &MouseEvent| {
      if let Some(on_link_activate) = &on_link_activate {
        on_link_activate(&link, event);
      }
      if event.button != MouseButton::Left {
        return;
      }
      if let Some(on_link_click) = &on_link_click {
        on_link_click(&link);
        return;
      }
      #[cfg(feature = "router")]
      if on_link_activate.is_none()
        && let Some(navigator) = &navigator
        && is_router_link(link.destination())
      {
        navigator.push(link.destination().to_owned());
      }
    })
  }

  /// `node`, followed as `link` on a left click and Enter, and on a middle
  /// click too when the app takes link activations.
  fn follows(&self, node: Node, link: MarkdownLink) -> Node {
    let activate = self.activation_handler(link);
    let node = node.on_click(activate.clone());
    if self.on_link_activate.is_none() {
      return node;
    }
    node
      .on_mouse_down(hold_middle_press)
      .on_mouse_click(MouseButton::Middle, activate)
  }

  /// `node` with the tooltip `text`: it registers an anchor that the
  /// document's tooltip shows at while the pointer rests on it.
  fn with_tooltip(&self, node: Node, key: TooltipKey, text: Option<Arc<str>>) -> Node {
    let Some(text) = text else {
      return node;
    };
    let anchor = self.hover.anchor(&key);
    self
      .tooltips
      .borrow_mut()
      .push(LinkTooltip::new(key.clone(), anchor.clone(), text));
    let (enter, leave) = (self.hover.clone(), self.hover.clone());
    let enter_key = key.clone();
    node
      .ref_element(anchor)
      .on_mouse_enter(move || enter.enter(&enter_key))
      .on_mouse_leave(move || leave.leave(&key))
  }
}

/// A middle press on a link starts its middle click, not a drag of what the
/// link is in. (A middle press starts no text selection or scrollbar drag.)
fn hold_middle_press(event: MouseEvent) {
  if event.button == MouseButton::Middle {
    event.prevent_default();
  }
}

#[cfg(feature = "router")]
fn is_router_link(destination: &str) -> bool {
  destination.starts_with('/') || destination.starts_with('#') || destination.starts_with('?')
}

/// What a row of texts and images holds, in order.
pub(super) enum FlowItem<'i> {
  Text(Box<Node>),
  /// An image, and the style of the text it sits in.
  Image(&'i MarkdownInline, TextStyle),
}

/// Builds one paragraph: its spans and the link nodes placed on them.
pub(super) struct Paragraph<'a> {
  spans: Vec<RichTextSpan>,
  /// Spans before this index are closed: nothing merges into them, so a
  /// link's spans stay its own.
  sealed: usize,
  /// Spans that are presentation, not the paragraph's text: marks, joiners
  /// and trailing runs, in span order.
  generated: Vec<Range<usize>>,
  /// The paragraph has a link: directional controls and marks are dropped
  /// from all of its text, and its separators are line feeds.
  strip_controls: bool,
  /// The direction of each next link's bidi paragraph.
  directions: std::vec::IntoIter<Direction>,
  links: Vec<Node>,
  theme: &'a ThemeMarkdown,
  session: &'a LinkSession,
}

impl<'a> Paragraph<'a> {
  /// A paragraph of `inlines`; with `flow`, an image outside a link ends the
  /// text before it ([`Self::push_inlines`]).
  pub(super) fn new(
    inlines: &[MarkdownInline],
    flow: bool,
    theme: &'a ThemeMarkdown,
    session: &'a LinkSession,
  ) -> Self {
    let strip_controls = has_link(inlines);
    let directions = if strip_controls {
      link_directions(inlines, flow, theme)
    } else {
      Vec::new()
    };
    Self {
      spans: Vec::new(),
      sealed: 0,
      generated: Vec::new(),
      strip_controls,
      directions: directions.into_iter(),
      links: Vec::new(),
      theme,
      session,
    }
  }

  pub(super) fn is_empty(&self) -> bool {
    self.spans.is_empty()
  }

  /// The paragraph so far as one rich-text node; the builder starts over.
  pub(super) fn take_node(&mut self) -> Node {
    self.sealed = 0;
    Node::rich_text_with_links(
      std::mem::take(&mut self.spans),
      std::mem::take(&mut self.links),
      std::mem::take(&mut self.generated),
    )
  }

  /// Adds `inlines` in `style`. With `flow`, an image outside a link ends
  /// the paragraph so far and goes into `flow` after it, for a row of texts
  /// and images; without it, an image is its alt text.
  pub(super) fn push_inlines<'i>(
    &mut self,
    inlines: &'i [MarkdownInline],
    style: &TextStyle,
    flow: &mut Option<&mut Vec<FlowItem<'i>>>,
  ) {
    for inline in inlines {
      match inline {
        MarkdownInline::Link {
          destination,
          title,
          children,
        } => self.push_link(destination, title, children, style),
        MarkdownInline::Image { .. } if flow.is_some() => {
          if let Some(flow) = flow.as_mut() {
            if !self.is_empty() {
              flow.push(FlowItem::Text(Box::new(self.take_node())));
            }
            flow.push(FlowItem::Image(inline, style.clone()));
          }
        }
        MarkdownInline::Emphasis(children) => {
          self.push_inlines(children, &self.theme.emphasis.apply(style), flow);
        }
        MarkdownInline::Strong(children) => {
          self.push_inlines(children, &self.theme.strong.apply(style), flow);
        }
        MarkdownInline::Strikethrough(children) => {
          self.push_inlines(children, &self.theme.strikethrough.apply(style), flow);
        }
        _ => {
          for span in markdown_inline_rich_text(std::slice::from_ref(inline), style, self.theme) {
            self.push_span(span);
          }
        }
      }
    }
  }

  fn push_link(&mut self, destination: &str, title: &str, children: &[MarkdownInline], style: &TextStyle) {
    let direction = self.directions.next().unwrap_or(Direction::LeftToRight);
    let (link, key) = self
      .session
      .next_link(destination, title, link_words(children, self.theme));
    if is_invisible(link.text()) {
      // Words that draw nothing would leave the runs where nothing marks
      // them as this link's: right after another link's words. Such a link
      // is its words, as text, and nothing to press.
      self.push_invisible_words(children, style);
      return;
    }
    let presentation = self.session.present(&link);
    let words_style = if presentation.as_text {
      style.clone()
    } else {
      presentation.style.as_ref().unwrap_or(&self.theme.link).apply(style)
    };
    let words = self.words(children, &words_style);
    let runs = presentation
      .trailing
      .iter()
      .map(|run| RichTextSpan {
        text: paragraph_breaks_as_spaces(&strip_bidi_controls(&run.text)).into_owned(),
        style: run.style.apply(style),
      })
      .collect();
    let block = self.push_block(LinkBlock::new(direction, words, runs, style));
    if presentation.as_text || block.label.is_empty() {
      return;
    }
    let inline_link = InlineLink {
      label: block.label,
      trailing: block.trailing,
      target: block.target,
      look: presentation.look(self.theme),
    };
    let node = Node::inline_link(inline_link, link.text()).cursor(CursorIcon::Pointer);
    let node = if presentation.pointer_only {
      node.focusable(false)
    } else {
      node.tab_index(0)
    };
    let node = self.session.follows(node, link);
    self
      .links
      .push(self.session.with_tooltip(node, key, presentation.tooltip));
  }

  /// A link's words in `style`, taken out of the paragraph to go into the
  /// link's block, on one line: breaks in them are spaces.
  fn words(&mut self, children: &[MarkdownInline], style: &TextStyle) -> Vec<RichTextSpan> {
    let start = self.seal();
    // The words of a link have no links in them (CommonMark), and an image
    // in them is its alt text, as in any link.
    self.push_inlines(children, style, &mut None);
    let mut words = self.spans.split_off(start);
    self.seal();
    for span in &mut words {
      span.text = paragraph_breaks_as_spaces(&span.text).into_owned();
    }
    words
  }

  /// The words of a link that draws nothing, in their own isolate: as with
  /// any link, bidi does not take the paragraph's direction from them.
  fn push_invisible_words(&mut self, children: &[MarkdownInline], style: &TextStyle) {
    let words = self.words(children, style);
    self.push_mark(FIRST_STRONG_ISOLATE, style);
    for span in words {
      self.push_span(span);
    }
    self.push_mark(POP_DIRECTIONAL_ISOLATE, style);
  }

  /// Adds a link's spans, sealed; returns where its words, runs and target
  /// are among the paragraph's spans.
  fn push_block(&mut self, block: LinkBlock) -> PlacedLink {
    let offset = self.seal();
    let shift = |range: Range<usize>| range.start + offset..range.end + offset;
    self.spans.extend(block.spans);
    self.generated.extend(block.generated.into_iter().map(shift));
    self.seal();
    PlacedLink {
      label: shift(block.label),
      trailing: shift(block.trailing),
      target: shift(block.target),
    }
  }

  /// An isolate mark: a span of its own, never merged, never copied.
  fn push_mark(&mut self, mark: char, style: &TextStyle) {
    let index = self.seal();
    self.spans.push(RichTextSpan {
      text: mark.to_string(),
      style: style.clone(),
    });
    self.generated.push(index..index + 1);
    self.seal();
  }

  fn push_span(&mut self, mut span: RichTextSpan) {
    if self.strip_controls {
      span.text = paragraph_breaks_as_line_feeds(&strip_bidi_controls(&span.text)).into_owned();
    }
    if span.text.is_empty() {
      return;
    }
    if self.spans.len() > self.sealed
      && let Some(last) = self.spans.last_mut()
      && last.style == span.style
    {
      last.text.push_str(&span.text);
      return;
    }
    self.spans.push(span);
  }

  /// Closes the spans so far; returns their count.
  fn seal(&mut self) -> usize {
    self.sealed = self.spans.len();
    self.sealed
  }
}

/// Whether a link is among `inlines`, at any depth.
fn has_link(inlines: &[MarkdownInline]) -> bool {
  inlines.iter().any(|inline| match inline {
    MarkdownInline::Link { .. } => true,
    MarkdownInline::Emphasis(children) | MarkdownInline::Strong(children) | MarkdownInline::Strikethrough(children) => {
      has_link(children)
    }
    _ => false,
  })
}

/// Where a link's words, runs and target landed among its paragraph's spans.
struct PlacedLink {
  label: Range<usize>,
  trailing: Range<usize>,
  target: Range<usize>,
}

/// A link's words as the paragraph draws them, as plain text: inline HTML as
/// its text, an image as its alt text, breaks and separators as spaces,
/// without directional controls.
fn link_words(children: &[MarkdownInline], theme: &ThemeMarkdown) -> String {
  let words = markdown_inline_rich_text(children, &TextStyle::default(), theme)
    .into_iter()
    .map(|span| span.text)
    .collect::<String>();
  paragraph_breaks_as_spaces(&strip_bidi_controls(&words)).into_owned()
}
