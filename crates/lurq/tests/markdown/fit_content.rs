use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, theme::ThemeMarkdown},
  components::{Column, Markdown, MarkdownProps, Row, Text},
  core::ElementRect,
  layout::{Alignment, layout_kind::Justify, text_style::TextStyle},
  node::{Element, dimension::Dimension},
};

use crate::support::TestSurface;

const HOST_WIDTH: f32 = 400.0;
const EPSILON: f32 = 0.5;

const LONG_PARAGRAPH: &str = "This paragraph is much longer than the bubble can ever be, so it has to wrap onto \
                              several lines at the width the bubble is given instead of overflowing it.";

#[derive(Clone, Copy, PartialEq)]
enum Host {
  /// A column of messages that aligns its children with this alignment.
  Column(Alignment),
  /// A row that puts the bubble at its end; the bubble may take the row's width.
  RowEnd,
}

#[derive(Clone, PartialEq)]
struct BubbleProps {
  source: &'static str,
  fit_content: bool,
  host: Host,
  /// A plain text measured beside the bubble, for its natural width.
  probe: &'static str,
}

impl BubbleProps {
  fn fit(source: &'static str) -> Self {
    Self {
      source,
      fit_content: true,
      host: Host::Column(Alignment::Start),
      probe: "",
    }
  }

  fn host(mut self, host: Host) -> Self {
    self.host = host;
    self
  }

  fn probe(mut self, probe: &'static str) -> Self {
    self.probe = probe;
    self
  }
}

impl lurq::app::component::DevtoolsInspectable for BubbleProps {}

struct BubbleRoot;

impl Component for BubbleRoot {
  type Props = BubbleProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    let markdown = Markdown::mount(
      ctx,
      MarkdownProps::styled(props.source, text_style()).fit_content(props.fit_content),
    );
    let bubble = Column::new().id("bubble").child(markdown);
    let host: Element = match props.host {
      Host::Column(alignment) => Column::new()
        .id("host")
        .width(HOST_WIDTH)
        .align_items(alignment)
        .child(bubble)
        .into(),
      Host::RowEnd => Row::new()
        .id("host")
        .width(HOST_WIDTH)
        .justify(Justify::End)
        .child(bubble.max_width(Dimension::Pct(100.0)))
        .into(),
    };
    Column::new()
      .child(host)
      .child(Text::styled(props.probe, text_style()).id("probe"))
  }
}

fn text_style() -> TextStyle {
  TextStyle {
    font_size: 14.0,
    ..TextStyle::default()
  }
}

fn layout(props: BubbleProps) -> Tree {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(800, 600);
  tree.mount_root::<BubbleRoot>(&mut app, props);
  tree.pass(&mut app, &TestSurface);
  tree
}

fn bounds_by_id(tree: &mut Tree, id: &str) -> ElementRect {
  tree
    .find_element(|element| element.id() == Some(id))
    .unwrap_or_else(|| panic!("element #{id} should be laid out"))
    .bounds()
}

fn bounds_by_text(tree: &mut Tree, text: &str) -> ElementRect {
  tree
    .find_element(|element| element.text_content() == Some(text) && element.id().is_none())
    .unwrap_or_else(|| panic!("markdown text {text:?} should be laid out"))
    .bounds()
}

fn assert_near(actual: f32, expected: f32, what: &str) {
  assert!(
    (actual - expected).abs() <= EPSILON,
    "{what}: expected {expected}px, got {actual}px"
  );
}

#[test]
fn markdown_fit_content_short_text_hugs_its_line() {
  let mut tree = layout(BubbleProps::fit("Hello there").probe("Hello there"));

  let probe = bounds_by_id(&mut tree, "probe");
  let bubble = bounds_by_id(&mut tree, "bubble");
  let text = bounds_by_text(&mut tree, "Hello there");

  assert!(probe.width > 0.0 && probe.width < HOST_WIDTH / 2.0, "{probe:?}");
  assert_near(bubble.width, probe.width, "bubble width");
  assert_near(text.width, probe.width, "text width");
}

#[test]
fn markdown_fit_content_long_text_wraps_at_the_available_width() {
  let mut tree = layout(BubbleProps::fit(LONG_PARAGRAPH).probe("Hello there"));

  let line_height = bounds_by_id(&mut tree, "probe").height;
  let host = bounds_by_id(&mut tree, "host");
  let bubble = bounds_by_id(&mut tree, "bubble");
  let text = bounds_by_text(&mut tree, LONG_PARAGRAPH);

  assert_near(host.width, HOST_WIDTH, "host width");
  assert!(bubble.width <= HOST_WIDTH + EPSILON, "bubble overflows: {bubble:?}");
  assert!(
    bubble.width > HOST_WIDTH * 0.8,
    "a wrapped bubble takes about the full width: {bubble:?}"
  );
  assert!(text.x + text.width <= host.x + HOST_WIDTH + EPSILON, "{text:?}");
  assert!(
    text.height >= line_height * 2.0,
    "the paragraph wraps: {text:?}, line {line_height}"
  );
}

#[test]
fn markdown_fit_content_takes_the_widest_block() {
  const WIDEST: &str = "The widest paragraph of this message";
  let source = "# Hi\n\nShort\n\nThe widest paragraph of this message\n\n- one\n- two\n\n```\nlet x = 1;\n```\n\n---";
  let mut tree = layout(BubbleProps::fit(source).probe(WIDEST));

  let probe = bounds_by_id(&mut tree, "probe");
  let bubble = bounds_by_id(&mut tree, "bubble");
  assert!(probe.width < HOST_WIDTH * 0.75, "{probe:?}");
  assert_near(bubble.width, probe.width, "bubble width");

  // Every block is stretched to the widest one, as CSS blocks are.
  let short = bounds_by_text(&mut tree, "Short");
  assert_near(short.width, bubble.width, "short paragraph width");
  let code_background = ThemeMarkdown::default()
    .code_block_box
    .background
    .expect("the default theme gives code blocks a background");
  let code = tree
    .find_element(|element| element.color() == Some(code_background))
    .expect("the code block box should be laid out")
    .bounds();
  assert_near(code.width, bubble.width, "code block width");
}

#[test]
fn markdown_fit_content_list_item_can_be_the_widest_block() {
  const ITEM: &str = "a list item wider than the paragraph";
  let mut tree = layout(BubbleProps::fit("Short\n\n- a list item wider than the paragraph").probe(ITEM));

  let theme = ThemeMarkdown::default();
  let probe = bounds_by_id(&mut tree, "probe");
  let bubble = bounds_by_id(&mut tree, "bubble");
  let item = bounds_by_text(&mut tree, ITEM);

  let expected = theme.unordered_list_marker_width + theme.list_marker_gap + probe.width;
  assert!(expected < HOST_WIDTH, "{expected}");
  assert_near(bubble.width, expected, "bubble width");
  assert_near(item.width, probe.width, "list item text width");
}

#[test]
fn markdown_fit_content_long_list_item_wraps_beside_its_marker() {
  let source = "- This list item is much longer than the bubble can ever be, so it has to wrap beside its marker \
                instead of pushing the bubble past the width it is given.";
  let mut tree = layout(BubbleProps::fit(source).probe("Hello there"));

  let line_height = bounds_by_id(&mut tree, "probe").height;
  let host = bounds_by_id(&mut tree, "host");
  let bubble = bounds_by_id(&mut tree, "bubble");
  let item = bounds_by_text(
    &mut tree,
    "This list item is much longer than the bubble can ever be, so it has to wrap beside its marker instead of \
     pushing the bubble past the width it is given.",
  );

  assert!(bubble.width <= HOST_WIDTH + EPSILON, "bubble overflows: {bubble:?}");
  assert!(
    item.x + item.width <= host.x + HOST_WIDTH + EPSILON,
    "item overflows: {item:?}"
  );
  assert!(
    item.height >= line_height * 2.0,
    "the item wraps: {item:?}, line {line_height}"
  );
}

#[test]
fn markdown_without_fit_content_still_fills_its_container() {
  let props = BubbleProps {
    fit_content: false,
    ..BubbleProps::fit("Hello there")
  };
  let mut tree = layout(props);

  let bubble = bounds_by_id(&mut tree, "bubble");
  let text = bounds_by_text(&mut tree, "Hello there");
  assert_near(bubble.width, HOST_WIDTH, "bubble width");
  assert_near(text.width, HOST_WIDTH, "text width");
}

#[test]
fn markdown_fit_content_bubble_aligns_to_the_end_of_a_column() {
  let mut tree = layout(
    BubbleProps::fit("Hello there")
      .host(Host::Column(Alignment::End))
      .probe("Hello there"),
  );

  let host = bounds_by_id(&mut tree, "host");
  let probe = bounds_by_id(&mut tree, "probe");
  let bubble = bounds_by_id(&mut tree, "bubble");
  assert_near(bubble.width, probe.width, "bubble width");
  assert_near(bubble.x + bubble.width, host.x + HOST_WIDTH, "bubble right edge");
}

#[test]
fn markdown_fit_content_bubble_at_the_end_of_a_row_hugs_and_wraps() {
  let mut tree = layout(BubbleProps::fit("Hello there").host(Host::RowEnd).probe("Hello there"));
  let host = bounds_by_id(&mut tree, "host");
  let probe = bounds_by_id(&mut tree, "probe");
  let bubble = bounds_by_id(&mut tree, "bubble");
  assert_near(bubble.width, probe.width, "short bubble width");
  assert_near(bubble.x + bubble.width, host.x + HOST_WIDTH, "short bubble right edge");

  let mut tree = layout(BubbleProps::fit(LONG_PARAGRAPH).host(Host::RowEnd));
  let host = bounds_by_id(&mut tree, "host");
  let bubble = bounds_by_id(&mut tree, "bubble");
  assert!(
    bubble.width <= HOST_WIDTH + EPSILON,
    "bubble overflows the row: {bubble:?}"
  );
  assert!(bubble.x >= host.x - EPSILON, "bubble starts before the row: {bubble:?}");
  assert_near(bubble.x + bubble.width, host.x + HOST_WIDTH, "long bubble right edge");
}
