//! A Markdown code block whose lines are wider than its column scrolls
//! sideways inside its box: its lines stay unwrapped, the box, its label and
//! the document keep their width, a horizontal scrollbar shows while it
//! overflows, Shift+wheel and a trackpad's sideways swipe scroll it, and a
//! plain vertical wheel over it still scrolls the page around it. A block
//! whose lines fit has no scrollbar and the layout it had before.

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase, theme::ThemeMarkdown},
  components::{Column, Markdown, MarkdownProps, Rect, ScrollVertical},
  core::ElementRect,
  layout::{
    Alignment,
    quad::QuadContent,
    scrollbar::{ScrollBarPlacement, ScrollBarStyle, ScrollBarVisibility},
  },
  node::{Element, color::Color},
};

use super::links::{MarkdownRoot, bounds_of, pass, quads};
use crate::support::{TestSurface, selection::Page};

/// The column the document is laid out in.
const COLUMN: f32 = 400.0;
/// The end of the code block's last line, to find its text by.
const LINE_END: &str = "END-OF-THE-LAST-LINE";
const LABEL: &str = "markdown";
/// A wheel notch, as the winit shell maps a `LineDelta` of one line.
const LINE: f32 = 40.0;

/// A reply with a code block holding `first` and a short last line, between
/// two paragraphs.
fn source(first: &str) -> String {
  format!("A reply with code:\n\n```{LABEL}\n{first}\nshort {LINE_END}\n```\n\nAfter the code.")
}

/// A line some ten times wider than the column.
fn long_line() -> String {
  "- the long line that runs past the column ".repeat(8)
}

fn show(props: MarkdownProps) -> (App, Tree) {
  show_in(App::new(), props)
}

fn show_in(mut app: App, props: MarkdownProps) -> (App, Tree) {
  let mut tree = Tree::new();
  tree.mount_root::<MarkdownRoot>(&mut app, props.width(COLUMN));
  for _ in 0..3 {
    pass(&mut app, &mut tree);
  }
  (app, tree)
}

/// Renders the document again from `source`.
fn update(app: &mut App, tree: &mut Tree, source: String) {
  tree.update_root_props::<MarkdownRoot>(MarkdownProps::new(source).width(COLUMN));
  for _ in 0..3 {
    pass(app, tree);
  }
}

/// Where the code block's text is laid out, scroll included.
fn code_text(tree: &mut Tree) -> ElementRect {
  text_with(tree, LINE_END)
}

/// Where the code text holding `marker` is laid out, scroll included.
fn text_with(tree: &mut Tree, marker: &str) -> ElementRect {
  tree
    .find_element(|element| element.text_content().is_some_and(|text| text.contains(marker)))
    .unwrap_or_else(|| panic!("the code text with {marker:?} is laid out"))
    .bounds()
}

/// The document's width: the root's.
fn document_width(tree: &mut Tree) -> f32 {
  tree.find_element(|_| true).expect("a document").bounds().width
}

fn rects_of(tree: &Tree, color: Color) -> Vec<(f32, f32, f32, f32)> {
  quads(tree)
    .into_iter()
    .filter(|quad| matches!(&quad.content, QuadContent::Rect { color: drawn, .. } if *drawn == color))
    .map(|quad| (quad.x, quad.y, quad.width, quad.height))
    .collect()
}

/// The code block's box, found by the theme's background.
fn block_box(tree: &Tree) -> (f32, f32, f32, f32) {
  let background = ThemeMarkdown::default()
    .code_block_box
    .background
    .expect("the default theme fills the box");
  rects_of(tree, background)
    .into_iter()
    .next()
    .expect("the code block's box is drawn")
}

/// The scrollbar thumbs drawn in the default theme's code block bar color.
fn thumbs(tree: &Tree) -> Vec<(f32, f32, f32, f32)> {
  let bar = ThemeMarkdown::default()
    .code_block_scrollbar
    .expect("the default theme draws a bar");
  rects_of(tree, bar.thumb_color)
}

/// A point over the block's lines, in the middle of its box.
fn pointer(tree: &mut Tree) -> (f32, f32) {
  let block = block_box(tree);
  let text = code_text(tree);
  (block.0 + block.2 / 2.0, text.y + text.height / 2.0)
}

/// The wheel over the block's lines, as the winit shell delivers one notch
/// with Shift held: the line's delta on the x axis and none on the y axis.
fn shift_wheel(app: &mut App, tree: &mut Tree, notches: f32) {
  let (x, y) = pointer(tree);
  tree.scroll(x, y, -LINE * notches, 0.0, ScrollPhase::Scroll);
  pass(app, tree);
}

/// A trackpad's sideways swipe over the block's lines, `dx` pixels to the
/// right, as the shell delivers it: a start, moves, an end.
fn swipe(app: &mut App, tree: &mut Tree, dx: f32) {
  let (x, y) = pointer(tree);
  tree.scroll(x, y, 0.0, 0.0, ScrollPhase::Start);
  tree.scroll(x, y, -dx / 2.0, 0.0, ScrollPhase::Scroll);
  tree.scroll(x, y, -dx / 2.0, 0.0, ScrollPhase::Scroll);
  tree.scroll(x, y, 0.0, 0.0, ScrollPhase::End);
  pass(app, tree);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn a_long_line_is_not_wrapped_and_the_block_scrolls_by_shift_wheel() {
  let (mut app, mut tree) = show(MarkdownProps::new(source(&long_line())));
  let (_, mut short) = show(MarkdownProps::new(source("short")));
  let text = code_text(&mut tree);
  let block = block_box(&tree);
  assert_eq!(
    document_width(&mut tree),
    COLUMN,
    "the block never makes the document wider"
  );
  assert_eq!(block.2, COLUMN, "the box is as wide as the column");
  assert!(
    text.width > COLUMN * 5.0,
    "the line is laid out unwrapped: {}",
    text.width
  );
  assert_eq!(
    text.height,
    code_text(&mut short).height,
    "two lines, as in the short block"
  );
  assert_eq!(
    thumbs(&tree).len(),
    1,
    "a block that overflows shows its horizontal bar"
  );
  let label = bounds_of(&mut tree, LABEL);

  shift_wheel(&mut app, &mut tree, 1.0);
  let scrolled = code_text(&mut tree);
  assert_eq!(
    scrolled.x,
    text.x - LINE,
    "one notch scrolls the lines one line's delta"
  );
  assert_eq!(
    (scrolled.y, scrolled.width, scrolled.height),
    (text.y, text.width, text.height)
  );
  assert_eq!(block_box(&tree), block, "the box stays put");
  assert_eq!(bounds_of(&mut tree, LABEL), label, "the label stays put");
  assert_eq!(document_width(&mut tree), COLUMN);

  shift_wheel(&mut app, &mut tree, -3.0);
  assert_eq!(code_text(&mut tree).x, text.x, "scrolling back stops at the start");
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn a_sideways_swipe_scrolls_the_block_and_the_bar_follows() {
  let (mut app, mut tree) = show(MarkdownProps::new(source(&long_line())));
  let text = code_text(&mut tree);
  let thumb = thumbs(&tree)[0];
  let block = block_box(&tree);
  assert!(
    thumb.1 >= block.1 + block.3 - 14.0 && thumb.1 + thumb.3 <= block.1 + block.3,
    "the bar sits in the box's bottom padding: thumb {thumb:?}, box {block:?}"
  );

  swipe(&mut app, &mut tree, 200.0);
  assert_eq!(code_text(&mut tree).x, text.x - 200.0);
  let moved = thumbs(&tree)[0];
  assert!(
    moved.0 > thumb.0,
    "the thumb moves right with the scroll: {moved:?} after {thumb:?}"
  );
  assert_eq!((moved.1, moved.2, moved.3), (thumb.1, thumb.2, thumb.3));

  swipe(&mut app, &mut tree, 100_000.0);
  let end = code_text(&mut tree);
  assert_eq!(
    end.x + end.width + 14.0,
    block.0 + block.2,
    "the end of the line stops at the box's padding"
  );
}

#[test]
fn a_block_whose_lines_fit_has_no_bar_and_the_layout_it_had() {
  let (mut app, mut tree) = show(MarkdownProps::new(source("short")));
  let padding = 14.0;
  let spacing = ThemeMarkdown::default().code_block_spacing;
  let block = block_box(&tree);
  let label = bounds_of(&mut tree, LABEL);
  let text = code_text(&mut tree);
  assert_eq!(block.2, COLUMN);
  assert_eq!(
    (label.x, label.y),
    (block.0 + padding, block.1 + padding),
    "the label at the padding"
  );
  assert_eq!(
    label.width,
    COLUMN - 2.0 * padding,
    "the label spans the box, as before"
  );
  assert_eq!((text.x, text.y), (block.0 + padding, label.y + label.height + spacing));
  assert_eq!(
    block.3,
    padding + label.height + spacing + text.height + padding,
    "the box's height"
  );
  assert!(thumbs(&tree).is_empty(), "no bar without overflow");

  shift_wheel(&mut app, &mut tree, 1.0);
  swipe(&mut app, &mut tree, 200.0);
  assert_eq!(code_text(&mut tree), text, "nothing to scroll");
  assert!(thumbs(&tree).is_empty());
}

#[test]
fn a_block_without_a_label_keeps_its_lines_at_the_padding() {
  let (_, mut tree) = show(MarkdownProps::new(format!("```\nshort {LINE_END}\n```")));
  let block = block_box(&tree);
  let text = code_text(&mut tree);
  assert_eq!((text.x, text.y), (block.0 + 14.0, block.1 + 14.0));
  assert_eq!(block.3, 14.0 + text.height + 14.0);
}

/// A page: a header, and under it a scrolling conversation whose reply
/// holds the block, between earlier and later messages.
struct Conversation;

impl Component for Conversation {
  type Props = MarkdownProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let reply = Markdown::mount(ctx, ctx.props::<Self::Props>().clone());
    let messages = Column::new()
      .child(Rect::new(COLUMN, 60.0).background("#111827"))
      .child(reply)
      .child(Rect::new(COLUMN, 600.0).background("#111827"));
    Column::new()
      .child(Rect::new(COLUMN, 40.0).background("#1f2937"))
      .child(ScrollVertical::new(messages).size(COLUMN, 300.0))
  }
}

#[test]
fn a_vertical_wheel_over_the_block_scrolls_the_page_not_the_block() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<Conversation>(&mut app, MarkdownProps::new(source(&long_line())).width(COLUMN));
  for _ in 0..3 {
    pass(&mut app, &mut tree);
  }
  let text = code_text(&mut tree);
  let after = bounds_of(&mut tree, "After the code.");
  let pointer = (text.x + 40.0, text.y + text.height / 2.0);

  tree.scroll(pointer.0, pointer.1, 0.0, -LINE, ScrollPhase::Scroll);
  pass(&mut app, &mut tree);
  let scrolled = code_text(&mut tree);
  assert_eq!(scrolled.y, text.y - LINE, "the page scrolls under the still pointer");
  assert_eq!(
    scrolled.x, text.x,
    "the block does not scroll sideways for a vertical wheel"
  );
  assert_eq!(bounds_of(&mut tree, "After the code.").y, after.y - LINE);

  // The block moved up under the still pointer; Shift+wheel over where it is now.
  let pointer = (scrolled.x + 40.0, scrolled.y + scrolled.height / 2.0);
  tree.scroll(pointer.0, pointer.1, -LINE, 0.0, ScrollPhase::Scroll);
  pass(&mut app, &mut tree);
  let sideways = code_text(&mut tree);
  assert_eq!(sideways.x, text.x - LINE, "Shift+wheel scrolls the block");
  assert_eq!(sideways.y, scrolled.y, "and leaves the page where it was");
}

#[test]
fn a_block_nested_in_a_list_and_in_a_quote_scrolls_inside_its_item() {
  let theme = ThemeMarkdown::default();
  let long = long_line();
  let in_list = format!("- An item with code:\n\n  ```\n  {long}\n  short {LINE_END}\n  ```\n- The next item");
  let (mut app, mut tree) = show(MarkdownProps::new(in_list));
  let block = block_box(&tree);
  let indent = theme.unordered_list_marker_width + theme.list_marker_gap;
  assert_eq!(block.0, indent, "the box starts beside the marker");
  assert_eq!(block.2, COLUMN - indent, "and ends with the column");
  assert_eq!(document_width(&mut tree), COLUMN);
  let text = code_text(&mut tree);
  swipe(&mut app, &mut tree, 120.0);
  assert_eq!(code_text(&mut tree).x, text.x - 120.0);
  assert_eq!(block_box(&tree), block);
  assert_eq!(document_width(&mut tree), COLUMN);

  let in_quote = format!("> A quote with code:\n>\n> ```\n> {long}\n> short {LINE_END}\n> ```");
  let (mut app, mut tree) = show(MarkdownProps::new(in_quote));
  let block = block_box(&tree);
  assert!(block.0 + block.2 <= COLUMN, "the box ends inside the quote: {block:?}");
  assert_eq!(document_width(&mut tree), COLUMN);
  let text = code_text(&mut tree);
  swipe(&mut app, &mut tree, 120.0);
  assert_eq!(code_text(&mut tree).x, text.x - 120.0);
  assert_eq!(block_box(&tree), block);
}

/// A column `COLUMN` wide holding a document sized to its content, at its
/// start, as a chat puts a bubble.
struct FittedHost;

impl Component for FittedHost {
  type Props = MarkdownProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .width(COLUMN)
      .align_items(Alignment::Start)
      .child(Markdown::mount(
        ctx,
        ctx.props::<Self::Props>().clone().fit_content(true),
      ))
  }
}

fn show_fitted(source: String) -> (App, Tree) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<FittedHost>(&mut app, MarkdownProps::new(source));
  for _ in 0..3 {
    pass(&mut app, &mut tree);
  }
  (app, tree)
}

#[test]
fn a_fitted_document_keeps_a_short_block_hugging_its_line_and_scrolls_a_long_one() {
  let (_, mut tree) = show_fitted(format!("```\nshort {LINE_END}\n```"));
  let block = block_box(&tree);
  let text = code_text(&mut tree);
  assert_eq!(block.2, text.width + 28.0, "the box hugs its line");
  assert!(thumbs(&tree).is_empty());

  let (mut app, mut tree) = show_fitted(format!("```\n{}\nshort {LINE_END}\n```", long_line()));
  assert_eq!(block_box(&tree).2, COLUMN, "a long line fills the container, no wider");
  let text = code_text(&mut tree);
  swipe(&mut app, &mut tree, 120.0);
  assert_eq!(code_text(&mut tree).x, text.x - 120.0);
  assert_eq!(block_box(&tree).2, COLUMN);
}

/// How many of ten idle passes found the layout dirty.
fn dirty_passes(app: &mut App, tree: &mut Tree) -> usize {
  (0..10)
    .filter(|_| {
      tree.request_redraw();
      tree.pass(app, &TestSurface).reasons.layout_dirty
    })
    .count()
}

#[test]
fn a_scrolling_block_settles() {
  let (mut app, mut tree) = show(MarkdownProps::new(source(&long_line())));
  assert_eq!(
    dirty_passes(&mut app, &mut tree),
    0,
    "the block keeps the layout dirty when shown"
  );
  swipe(&mut app, &mut tree, 200.0);
  assert_eq!(
    dirty_passes(&mut app, &mut tree),
    0,
    "the block keeps the layout dirty after a scroll"
  );
  let text = code_text(&mut tree);
  tree.mouse_move(text.x + 40.0, text.y + 4.0);
  pass(&mut app, &mut tree);
  assert_eq!(
    dirty_passes(&mut app, &mut tree),
    0,
    "a pointer over the block keeps the layout dirty"
  );
}

/// A selectable document in a window wide enough for the column; its glyphs
/// are not read, since the lines scrolled out of the box are not drawn.
fn selectable(source: String) -> Page {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(COLUMN as u32 + 40, 1200);
  tree.mount_root::<MarkdownRoot>(&mut app, MarkdownProps::new(source).selectable(true).width(COLUMN));
  for _ in 0..3 {
    pass(&mut app, &mut tree);
  }
  Page::without_inks(app, tree, 1.0)
}

#[test]
fn selection_and_copy_reach_across_a_scrolled_block() {
  let mut page = selectable(source(&long_line()));
  let text = code_text(&mut page.tree);
  let (x, y) = pointer(&mut page.tree);
  page.tree.scroll(x, y, 0.0, 0.0, ScrollPhase::Start);
  page.tree.scroll(x, y, -150.0, 0.0, ScrollPhase::Scroll);
  page.tree.scroll(x, y, 0.0, 0.0, ScrollPhase::End);
  page.pass();
  assert_eq!(code_text(&mut page.tree).x, text.x - 150.0, "the block is scrolled");
  let block = block_box(&page.tree);
  let line_height = text.height / 2.0;
  let first_line = text.y + line_height / 2.0;
  let last_line = text.y + text.height - line_height / 2.0;

  // Inside the block: from the long line where it is scrolled to, to past
  // the end of the short last line, copies what lies between.
  page.drag((block.0 + 20.0, first_line), (block.0 + block.2 - 20.0, last_line));
  let inside = page.selected().expect("a selection inside the block");
  let (first, last) = inside
    .split_once('\n')
    .unwrap_or_else(|| panic!("two lines are selected: {inside:?}"));
  assert_eq!(last, format!("short {LINE_END}"));
  assert!(
    long_line().ends_with(first) && first.len() + 10 < long_line().len(),
    "the first line starts where the block is scrolled to: {first:?}"
  );
  let highlights = page.highlights();
  assert!(
    highlights
      .iter()
      .any(|rect| rect.x < block.0 + block.2 && rect.x + rect.width > block.0),
    "the selection is highlighted in the box: {highlights:?}"
  );

  // Across the block: from the paragraph before to the one after copies the
  // whole lines, the part scrolled out of view included, the label with them.
  let before = bounds_of(&mut page.tree, "A reply with code:");
  let after = bounds_of(&mut page.tree, "After the code.");
  page.drag(
    (before.x + 1.0, before.y + before.height / 2.0),
    (after.x + after.width - 1.0, after.y + after.height / 2.0),
  );
  let copied = page.selected().expect("a selection across the block");
  assert_eq!(
    copied,
    format!(
      "A reply with code:
{LABEL}
{}
short {LINE_END}
After the code.",
      long_line()
    ),
  );
}

/// A trackpad swipe of `times` moves, each `(dx, dy)`, over the block in
/// the conversation page.
fn swipe_in_page(app: &mut App, tree: &mut Tree, dx: f32, dy: f32, times: usize) {
  let text = code_text(tree);
  let (x, y) = (COLUMN / 2.0, text.y + text.height / 2.0);
  tree.scroll(x, y, 0.0, 0.0, ScrollPhase::Start);
  for _ in 0..times {
    tree.scroll(x, y, dx, dy, ScrollPhase::Scroll);
  }
  tree.scroll(x, y, 0.0, 0.0, ScrollPhase::End);
  pass(app, tree);
}

fn show_page() -> (App, Tree) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<Conversation>(&mut app, MarkdownProps::new(source(&long_line())).width(COLUMN));
  for _ in 0..3 {
    pass(&mut app, &mut tree);
  }
  (app, tree)
}

#[test]
fn a_mostly_vertical_swipe_over_the_block_scrolls_only_the_page() {
  let (mut app, mut tree) = show_page();
  let text = code_text(&mut tree);
  swipe_in_page(&mut app, &mut tree, -3.0, -6.0, 5);
  let after = code_text(&mut tree);
  assert_eq!(after.y, text.y - 30.0, "the page scrolls by the whole vertical delta");
  assert_eq!(after.x, text.x, "the slant does not nudge the block");
}

#[test]
fn a_mostly_sideways_swipe_over_the_block_scrolls_it() {
  let (mut app, mut tree) = show_page();
  let text = code_text(&mut tree);
  swipe_in_page(&mut app, &mut tree, -6.0, -3.0, 5);
  let after = code_text(&mut tree);
  assert_eq!(after.x, text.x - 30.0, "the block takes the sideways delta");
  assert_eq!(after.y, text.y - 15.0, "the page takes the vertical rest");
}

const END_A: &str = "END-OF-BLOCK-A";
const END_B: &str = "END-OF-BLOCK-B";

/// A fenced block with `language` on its fence whose long first line ends
/// in `marker`: blocks are told apart by their first line.
fn block(language: &str, marker: &str) -> String {
  format!("```{language}\n{}{marker}\nshort\n```", long_line())
}

/// Scrolls the block holding `marker` by `dx`.
fn scroll_block(app: &mut App, tree: &mut Tree, marker: &str, dx: f32) {
  let text = text_with(tree, marker);
  let (x, y) = (COLUMN / 2.0, text.y + text.height / 2.0);
  tree.scroll(x, y, -dx, 0.0, ScrollPhase::Scroll);
  pass(app, tree);
}

/// How far the block holding `marker` is scrolled: its text's distance
/// from where an unscrolled block's text sits, at the box's padding.
fn scrolled_by(tree: &mut Tree, marker: &str) -> f32 {
  14.0 - text_with(tree, marker).x
}

#[test]
fn scrolled_blocks_keep_their_scroll_when_paragraphs_come_and_go_around_them() {
  let two = format!("Two blocks:\n\n{}\n\n{}", block("rust", END_A), block("rust", END_B));
  let (mut app, mut tree) = show(MarkdownProps::new(two));
  scroll_block(&mut app, &mut tree, END_B, 100.0);
  scroll_block(&mut app, &mut tree, END_A, 40.0);
  assert_eq!(scrolled_by(&mut tree, END_B), 100.0);
  assert_eq!(scrolled_by(&mut tree, END_A), 40.0);

  let with_paragraphs = format!(
    "A paragraph inserted above.\n\nTwo blocks:\n\n{}\n\nAnd one between.\n\n{}\n\nAnd one after.",
    block("rust", END_A),
    block("rust", END_B)
  );
  update(&mut app, &mut tree, with_paragraphs);
  assert_eq!(scrolled_by(&mut tree, END_A), 40.0, "a paragraph inserted above");
  assert_eq!(scrolled_by(&mut tree, END_B), 100.0, "paragraphs around the block");

  let fewer = format!("{}\n\n{}", block("rust", END_A), block("rust", END_B));
  update(&mut app, &mut tree, fewer);
  assert_eq!(scrolled_by(&mut tree, END_A), 40.0, "the paragraphs removed again");
  assert_eq!(scrolled_by(&mut tree, END_B), 100.0);
}

#[test]
fn a_scrolled_block_keeps_its_scroll_as_a_reply_streams_on() {
  let (mut app, mut tree) = show(MarkdownProps::new(format!("A reply:\n\n{}", block("", END_A))));
  scroll_block(&mut app, &mut tree, END_A, 100.0);

  // More text after the block, then more lines in the block itself.
  update(
    &mut app,
    &mut tree,
    format!("A reply:\n\n{}\n\nAnd then some more words.", block("", END_A)),
  );
  assert_eq!(scrolled_by(&mut tree, END_A), 100.0, "text appended after the block");
  let longer = format!("```\n{}{END_A}\nshort\nanother line\n```", long_line());
  update(
    &mut app,
    &mut tree,
    format!("A reply:\n\n{longer}\n\nAnd then some more words."),
  );
  assert_eq!(scrolled_by(&mut tree, END_A), 100.0, "a line appended to the block");
}

#[test]
fn a_one_line_block_keeps_its_scroll_and_selection_as_its_line_streams() {
  let line = format!("{}{LINE_END}", long_line());
  let streaming = |tail: &str| format!("A reply:\n\n```\n{line}{tail}\n```");
  let mut page = selectable(streaming(""));
  let (x, y) = pointer(&mut page.tree);
  page.tree.scroll(x, y, -150.0, 0.0, ScrollPhase::Scroll);
  page.pass();
  assert_eq!(scrolled_by(&mut page.tree, LINE_END), 150.0);
  let block = block_box(&page.tree);
  page.drag((block.0 + 20.0, y), (block.0 + block.2 - 20.0, y));
  let selected = page.selected().expect("a selection in the line");
  assert!(line.contains(&selected) && selected.len() > 10, "{selected:?}");

  for token in [" one", " one two", " one two three"] {
    page
      .tree
      .update_root_props::<MarkdownRoot>(MarkdownProps::new(streaming(token)).selectable(true).width(COLUMN));
    page.pass();
    page.pass();
    assert_eq!(
      scrolled_by(&mut page.tree, LINE_END),
      150.0,
      "the scroll after {token:?}"
    );
    assert_eq!(
      page.selected().as_deref(),
      Some(selected.as_str()),
      "the selection after {token:?}"
    );
  }
}

#[test]
fn a_slanted_swipe_delivered_as_separate_events_scrolls_only_the_page() {
  let (mut app, mut tree) = show_page();
  let text = code_text(&mut tree);
  let (x, y) = (COLUMN / 2.0, text.y + text.height / 2.0);
  let start = std::time::Instant::now();
  for index in 0..5u32 {
    tree.set_animation_clock(Some(start + std::time::Duration::from_millis(10) * index));
    tree.scroll(x, y, 0.0, -6.0, ScrollPhase::Scroll);
    tree.scroll(x, y, -3.0, 0.0, ScrollPhase::Scroll);
  }
  pass(&mut app, &mut tree);
  let after = code_text(&mut tree);
  assert_eq!(after.y, text.y - 30.0, "the page scrolls by the vertical events");
  assert_eq!(
    after.x, text.x,
    "the horizontal events in between do not nudge the block"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn an_app_theme_bar_that_reserves_a_gutter_leaves_a_fitting_block_its_size() {
  let gutter_thumb = Color::from_hex("#8b5cf6");
  let app = App::new();
  app.theme().set_scrollbar(ScrollBarStyle {
    placement: ScrollBarPlacement::Reserved,
    visible: ScrollBarVisibility::Always,
    thumb_color: gutter_thumb,
    width: 12.0,
    ..ScrollBarStyle::default()
  });
  let theme = ThemeMarkdown {
    code_block_scrollbar: None,
    ..ThemeMarkdown::default()
  };
  let (_, mut short) = show_in(app, MarkdownProps::new(source("short")).theme(theme.clone()));
  let (_, mut plain) = show(MarkdownProps::new(source("short")));
  assert_eq!(
    block_box(&short),
    block_box(&plain),
    "the box keeps its size: no gutter"
  );
  assert_eq!(code_text(&mut short), code_text(&mut plain));
  assert_eq!(
    rects_of(&short, gutter_thumb).len(),
    1,
    "a bar shown always is drawn over the padding, not in a gutter"
  );

  let app = App::new();
  app.theme().set_scrollbar(ScrollBarStyle {
    placement: ScrollBarPlacement::Reserved,
    thumb_color: gutter_thumb,
    ..ScrollBarStyle::default()
  });
  let (_, mut long) = show_in(app, MarkdownProps::new(source(&long_line())).theme(theme));
  let (_, mut plain) = show(MarkdownProps::new(source(&long_line())));
  assert_eq!(
    block_box(&long),
    block_box(&plain),
    "an overflowing box keeps its size too"
  );
  assert_eq!(code_text(&mut long), code_text(&mut plain));
  assert_eq!(
    rects_of(&long, gutter_thumb).len(),
    1,
    "the app's bar is drawn over the padding"
  );
}
