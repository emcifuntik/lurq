use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::MouseButton,
    theme::{MarkdownInlineStyle, ThemeMarkdown},
  },
  components::{Column, Markdown, MarkdownProps, Row, ScrollVertical, Text},
  layout::{
    Alignment, Constraints, Size,
    layout_result::LayoutResult,
    quad::QuadContent,
    text_style::{FontFeature, FontFeatures, FontStyle, FontWeight, TextStyle},
  },
  node::{ElementRef, color::Color, dimension::Dimension},
};

use crate::support::{TestSurface, pointer_click, render_pass_with_app};

struct MarkdownRoot;

impl Component for MarkdownRoot {
  type Props = MarkdownProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<lurq::node::Element> {
    Markdown::mount(ctx, ctx.props::<Self::Props>().clone())
  }
}

#[test]
fn markdown_component_renders_rich_inline_spans() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(
    &mut app,
    MarkdownProps::new("Hello **bold**, *soft*, and `code`")
      .selectable(true)
      .width(320.0),
  );
  tree.pass(&mut app, &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  let spans = quads
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::RichText { spans, .. } => Some(spans),
      _ => None,
    })
    .expect("markdown component should render inline content as rich text");

  assert_eq!(
    spans.iter().map(|span| span.text.as_str()).collect::<Vec<_>>(),
    vec!["Hello ", "bold", ", ", "soft", ", and ", "code"]
  );
  assert!(matches!(spans[1].style.weight, FontWeight::Bold));
  assert!(matches!(spans[3].style.style, FontStyle::Italic));
  assert_eq!(&*spans[5].style.font_family, "monospace");
}

#[test]
fn markdown_inline_code_font_features_shape_their_span() {
  // The ligature-probe face shapes "--" into one glyph unless liga is off.
  let render = |inline_code_features: Option<FontFeatures>| {
    let mut app = App::new();
    app.install_fonts(
      [include_bytes!("../assets/ligature_probe/LurqLigatureProbe-Regular.ttf").to_vec()],
      std::iter::empty::<(&str, &str)>(),
    );
    let mut markdown = ThemeMarkdown {
      inline_code: MarkdownInlineStyle::default(),
      ..ThemeMarkdown::default()
    };
    markdown.inline_code.text.font_features = inline_code_features;
    app.theme().set_markdown(markdown);
    let mut tree = Tree::new();
    tree.resize(400, 100);
    tree.mount_root::<MarkdownRoot>(
      &mut app,
      MarkdownProps::new("a `a --a`")
        .style(TextStyle {
          font_family: "Lurq Ligature Probe".into(),
          font_size: 10.0,
          ..TextStyle::default()
        })
        .width(320.0),
    );
    let glyphs = render_pass_with_app(&mut tree, &mut app).glyphs.len();
    let quads = tree.resolve_quads(tree.last_layout().expect("layout"));
    let features = quads
      .iter()
      .find_map(|quad| match &quad.content {
        QuadContent::RichText { spans, .. } => Some(
          spans
            .iter()
            .map(|span| span.style.font_features.clone())
            .collect::<Vec<_>>(),
        ),
        _ => None,
      })
      .expect("rich text");
    (glyphs, features)
  };

  let no_ligatures = FontFeatures::from([FontFeature::disable(*b"liga")]);
  let (glyphs, features) = render(Some(no_ligatures.clone()));
  assert_eq!(features, [FontFeatures::default(), no_ligatures]);
  assert_eq!(glyphs, 5, "a + a, -, -, a");
  let (glyphs, features) = render(None);
  assert!(features.iter().all(FontFeatures::is_empty), "{features:?}");
  assert_eq!(glyphs, 4, "a + a, ligature, a");
}

#[test]
fn markdown_reshapes_when_only_font_features_change() {
  let mut app = App::new();
  app.install_fonts(
    [include_bytes!("../assets/ligature_probe/LurqLigatureProbe-Regular.ttf").to_vec()],
    std::iter::empty::<(&str, &str)>(),
  );
  let mut tree = Tree::new();
  tree.resize(400, 100);
  let no_ligatures = FontFeatures::from([FontFeature::disable(*b"liga")]);
  for (font_features, glyphs) in [
    (FontFeatures::default(), 4),
    (no_ligatures.clone(), 5),
    (FontFeatures::default(), 4),
    (no_ligatures, 5),
  ] {
    let style = TextStyle {
      font_family: "Lurq Ligature Probe".into(),
      font_size: 10.0,
      font_features: font_features.clone(),
      ..TextStyle::default()
    };
    tree.mount_root::<MarkdownRoot>(&mut app, MarkdownProps::new("a *a --a*").style(style).width(320.0));
    let painted = render_pass_with_app(&mut tree, &mut app).glyphs.len();
    assert_eq!(painted, glyphs, "{font_features:?}");
  }
}

#[test]
fn text_without_markdown_renders_source_text() {
  let mut tree = Tree::new();
  tree.set_root(Text::new("Hello **bold**"));
  tree.pass(&mut App::new(), &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  assert!(quads.iter().any(|quad| {
    matches!(
      &quad.content,
      QuadContent::Text { text, .. } if text == "Hello **bold**"
    )
  }));
}

struct ChatMarkdownRoot;

impl Component for ChatMarkdownRoot {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<lurq::node::Element> {
    let messages = Column::new()
      .width(Dimension::Pct(100.0))
      .spacing(0.0)
      .child(Row::new().width(Dimension::Pct(100.0)).height(24.0))
      .child(chat_markdown_timeline_row(
        ctx,
        "Queued: Нурминский - Щемит в душе тоска",
      ))
      .child(chat_markdown_timeline_row(ctx, "Queued: Another short message"))
      .child(Row::new().width(Dimension::Pct(100.0)).height(6.0));

    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .child(
        ScrollVertical::new(messages)
          .width(Dimension::Pct(100.0))
          .height(Dimension::Pct(100.0))
          .flex(1.0),
      )
  }
}

struct ChatMarkdownUpdateRoot;

impl Component for ChatMarkdownUpdateRoot {
  type Props = String;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<lurq::node::Element> {
    let text = ctx.props::<Self::Props>().clone();
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(Dimension::Pct(100.0))
      .child(chat_markdown_timeline_row(ctx, &text))
  }
}

fn chat_markdown_timeline_row(ctx: &mut Ctx, text: &str) -> lurq::node::Element {
  Column::new()
    .width(Dimension::Pct(100.0))
    .padding_horizontal(24.0)
    .padding_bottom(18.0)
    .child(chat_markdown_message(ctx, text))
    .into()
}

fn chat_markdown_message(ctx: &mut Ctx, text: &str) -> lurq::node::Element {
  let style = TextStyle {
    font_size: 14.0,
    line_height: 1.5,
    color: Color::from_hex("#c7c2ba"),
    ..TextStyle::default()
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Start)
    .spacing(12.0)
    .child(Text::new("MU").width(36.0).height(36.0))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .min_width(0.0)
        .flex(1.0)
        .spacing(4.0)
        .child(Row::new().child(Text::new("Music Bot")).child(Text::new("22:28")))
        .child(
          Column::new().width(Dimension::Pct(100.0)).min_width(0.0).clip().child(
            ctx.mount::<Markdown>(
              MarkdownProps::styled(text, style)
                .width(Dimension::Pct(100.0))
                .selectable(true),
            ),
          ),
        ),
    )
    .into()
}

#[test]
fn markdown_plain_text_keeps_intrinsic_height_in_chat_like_layout() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<ChatMarkdownRoot>(&mut app, ());

  tree.set_layout_constraints_override(Some(Constraints::tight(Size::new(1000.0, 300.0))));
  tree.pass(&mut app, &TestSurface);
  let layout = tree.last_layout().cloned().expect("layout should be available");
  tree.set_layout_constraints_override(None);

  let root = tree.root().expect("root should be mounted");
  let layout_height = find_layout_height_for_text(root, &layout, "Queued: Нурминский - Щемит в душе тоска")
    .expect("markdown text layout should be present");

  assert!(
    layout_height <= 24.0,
    "single-line markdown text layout should stay near its 21px line height, got {layout_height}px"
  );

  let quads = tree.resolve_quads(&layout);
  let text = quads
    .iter()
    .find(|quad| {
      matches!(
        &quad.content,
        QuadContent::Text { text, .. } if text == "Queued: Нурминский - Щемит в душе тоска"
      )
    })
    .expect("markdown text should be rendered");

  assert!(
    text.height <= 24.0,
    "single-line markdown text should stay near its 21px line height, got {}px in root layout {}x{}",
    text.height,
    layout.size.width,
    layout.size.height
  );
}

#[test]
fn markdown_plain_text_layout_shrinks_after_source_update() {
  let mut app = App::new();

  let short = "Queued: Нурминский - Щемит в душе тоска";
  let long = [
    "Queued: very long first line that should wrap several times in this chat-like layout",
    "second line",
    "third line",
    "fourth line",
    "fifth line",
  ]
  .join("\n");

  let mut tree = Tree::new();
  tree.mount_root::<ChatMarkdownUpdateRoot>(&mut app, long);
  tree.set_layout_constraints_override(Some(Constraints::tight(Size::new(1000.0, 300.0))));
  tree.pass(&mut app, &TestSurface);

  tree.update_root_props::<ChatMarkdownUpdateRoot>(short.to_owned());
  tree.pass(&mut app, &TestSurface);
  let layout = tree.last_layout().cloned().expect("layout should be available");
  tree.set_layout_constraints_override(None);

  let root = tree.root().expect("root should be mounted");
  let layout_height =
    find_layout_height_for_text(root, &layout, short).expect("markdown text layout should be present");

  assert!(
    layout_height <= 24.0,
    "updated single-line markdown text layout should shrink to one line, got {layout_height}px"
  );
}

fn find_layout_height_for_text(element: ElementRef<'_>, layout: &LayoutResult, text: &str) -> Option<f32> {
  if element.text_content() == Some(text) {
    return Some(layout.size.height);
  }

  for (child, child_layout) in element.children().into_iter().zip(layout.children.iter()) {
    if let Some(height) = find_layout_height_for_text(child, &child_layout.result, text) {
      return Some(height);
    }
  }

  None
}

#[test]
fn markdown_component_renders_code_block_box() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(
    &mut app,
    MarkdownProps::styled(
      "```rust\nfn main() {}\n```",
      TextStyle {
        color: Color::from_hex("#f8fafc"),
        ..TextStyle::default()
      },
    )
    .width(320.0),
  );
  tree.pass(&mut app, &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  assert!(quads.iter().any(|quad| {
    matches!(
      &quad.content,
      QuadContent::Rect { color, .. } if *color == Color::from_hex("#0f172a")
    )
  }));
  assert!(quads.iter().any(|quad| {
    matches!(
      &quad.content,
      QuadContent::RichText { spans, .. } if spans.iter().map(|span| span.text.as_str()).collect::<String>() == "fn main() {}"
    )
  }));
  assert!(quads.iter().any(|quad| {
    matches!(
      &quad.content,
      QuadContent::RichText { spans, .. } if spans.iter().any(|span| span.text == "fn" && span.style.color == Color::from_hex("#93c5fd"))
    )
  }));
}

#[test]
fn markdown_component_uses_theme_markdown_overrides() {
  let mut markdown = ThemeMarkdown {
    strong: MarkdownInlineStyle::new(),
    ..ThemeMarkdown::default()
  };
  markdown.strong.text.color = Some(Color::from_hex("#dc2626"));

  let mut app = App::new();
  app.theme().set_markdown(markdown);

  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(&mut app, MarkdownProps::new("Hello **danger**").width(320.0));
  tree.pass(&mut app, &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  let spans = quads
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::RichText { spans, .. } => Some(spans),
      _ => None,
    })
    .expect("markdown component should render inline content as rich text");

  let strong = spans
    .iter()
    .find(|span| span.text == "danger")
    .expect("strong span should be present");
  assert_eq!(strong.style.color, Color::from_hex("#dc2626"));
}

#[test]
fn markdown_links_fire_click_callback() {
  let clicked = Arc::new(Mutex::new(None));
  let clicked_for_handler = clicked.clone();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(
    &mut app,
    MarkdownProps::new("Read [docs](/docs)")
      .on_link_click(move |link| *clicked_for_handler.lock().unwrap() = Some(link.destination().to_owned()))
      .width(320.0),
  );
  tree.pass(&mut app, &TestSurface);

  let link = tree
    .find_element(|element| element.text_content() == Some("docs"))
    .expect("link text should be rendered as its own clickable node");
  let bounds = link.bounds();
  pointer_click(
    &mut tree,
    bounds.x + bounds.width * 0.5,
    bounds.y + bounds.height * 0.5,
    MouseButton::Left,
  );

  assert_eq!(*clicked.lock().unwrap(), Some("/docs".to_owned()));
}

#[test]
#[cfg(not(all(feature = "image", feature = "resources")))]
fn markdown_image_falls_back_to_alt_text_without_resource_images() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(&mut app, MarkdownProps::new("![Alt **image**](demo.png)").width(320.0));
  tree.pass(&mut app, &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  assert!(quads.iter().any(|quad| {
    matches!(
      &quad.content,
      QuadContent::RichText { spans, .. } if spans.iter().map(|span| span.text.as_str()).collect::<Vec<_>>() == vec!["Alt ", "image"]
    )
  }));
}

#[test]
fn markdown_math_renders_as_code_styled_text() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(&mut app, MarkdownProps::new("Inline $x + y$").width(320.0));
  tree.pass(&mut app, &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  let spans = quads
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::RichText { spans, .. } => Some(spans),
      _ => None,
    })
    .expect("math should render in rich text");
  assert!(
    spans
      .iter()
      .any(|span| span.text == "$x + y$" && &*span.style.font_family == "monospace")
  );
}

#[test]
fn markdown_html_renders_as_readable_text() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(
    &mut app,
    MarkdownProps::new("Hello <strong>raw &amp; safe</strong>").width(320.0),
  );
  tree.pass(&mut app, &TestSurface);

  let quads = tree.resolve_quads(tree.last_layout().expect("layout should be available"));
  assert!(quads.iter().any(|quad| {
    matches!(
      &quad.content,
      QuadContent::RichText { spans, .. } if spans.iter().map(|span| span.text.as_str()).collect::<String>() == "Hello raw & safe"
    )
  }));
}
