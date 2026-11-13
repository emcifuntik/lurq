//! A multiline input that grew to its most rows scrolls its own text: the
//! wheel moves it within its text, its scrollbar shows while the text
//! overflows and drags like a scroll container's, and a scroll stays put
//! until the next edit brings the caret back.

use lurq::{
  app::{
    App, Tree,
    events::{MouseButton, ScrollPhase},
  },
  components::{Column, TextInput},
  core::Signal,
  layout::{scrollbar::ScrollBarStyle, text_style::TextStyle},
  node::color::Color,
};

use crate::support::{RectSnapshot, RenderSnapshot, render_pass_with_app};

const ID: &str = "scroll-input";
const THUMB: Color = Color::new(200, 30, 90, 255);
const THUMB_HOVERED: Color = Color::new(30, 200, 90, 255);
const PADDING: f32 = 6.0;
/// Line `n` (from 0) has `n + 1` letters, so the glyphs of the top row tell
/// which line it is.
const LINES: [&str; 6] = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff"];

struct Editor {
  tree: Tree,
  app: App,
  value: Signal<String>,
}

impl Editor {
  /// A 3-row input, focused with the caret at the end of `lines`.
  fn new(lines: &[&str]) -> Self {
    Self::with_rows(
      lines,
      3,
      TextStyle {
        font_size: 13.0,
        line_height: 1.4,
        ..TextStyle::default()
      },
    )
  }

  /// An input of up to `rows` rows in `style`, focused with the caret at the
  /// end of `lines`.
  fn with_rows(lines: &[&str], rows: usize, style: TextStyle) -> Self {
    let value = Signal::new(lines.join("\n"));
    let mut tree = Tree::new();
    tree.resize(400, 400);
    tree.set_root(
      Column::new().padding(20.0).child(
        TextInput::styled(value.clone(), style)
          .id(ID)
          .rows(1, rows)
          .width(200.0)
          .padding(PADDING)
          .scrollbar(ScrollBarStyle {
            thumb_color: THUMB,
            ..ScrollBarStyle::default()
          })
          .scrollbar_hovered(|style| ScrollBarStyle {
            thumb_color: THUMB_HOVERED,
            ..style
          }),
      ),
    );
    let mut editor = Self {
      tree,
      app: App::new(),
      value,
    };
    editor.render();
    editor.tree.get_element_by_id_mut(ID).unwrap().focus();
    editor.render();
    editor
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  fn center(&mut self) -> (f32, f32) {
    let bounds = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
    (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0)
  }

  fn wheel(&mut self, delta_y: f32) {
    let (x, y) = self.center();
    self.tree.scroll(x, y, 0.0, delta_y, ScrollPhase::Scroll);
    self.render();
  }

  fn key(&mut self, key: &str) {
    self.tree.key_down(key.into(), key.into(), false, false, false);
    self.tree.key_up(key.into(), key.into(), false, false, false);
    self.render();
  }

  /// The line at the top of the input's rows and how far below the top of
  /// the box inside the padding its glyphs are, from the glyphs painted
  /// there. The padding scrolls with the text, so a line above the rows can
  /// show in the top padding; it is not counted.
  fn top_line(&mut self) -> (usize, f32) {
    let rows_top = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap().y + PADDING;
    let snapshot = self.render();
    let mut glyphs: Vec<_> = snapshot
      .glyphs
      .iter()
      .filter(|glyph| glyph.clip.active && glyph.y + glyph.height > rows_top)
      .collect();
    glyphs.sort_by(|a, b| a.y.total_cmp(&b.y));
    let top = glyphs.first().expect("the input shows text");
    let letters = glyphs.iter().filter(|glyph| (glyph.y - top.y).abs() < 2.0).count();
    (letters - 1, top.y - rows_top)
  }

  fn thumb(&mut self) -> Option<RectSnapshot> {
    self
      .render()
      .rects
      .into_iter()
      .find(|rect| rect.color == THUMB || rect.color == THUMB_HOVERED)
  }
}

#[test]
fn text_input_wheel_scrolls_an_overflowing_input_and_stops_at_both_ends() {
  let mut editor = Editor::new(&LINES);
  let end = editor.top_line();
  assert_eq!(end.0, 3, "the caret's line shows at the bottom");

  editor.wheel(18.0);
  let moved = editor.top_line();
  assert!(
    moved != end && moved.0 < end.0,
    "the wheel moves the text down: {moved:?}"
  );

  editor.wheel(1000.0);
  let top = editor.top_line();
  assert_eq!(top.0, 0, "the wheel reaches the first line");
  editor.wheel(100.0);
  assert_eq!(editor.top_line(), top, "and stops there");

  editor.wheel(-1000.0);
  assert_eq!(editor.top_line(), end, "the wheel reaches the last lines");
  editor.wheel(-100.0);
  assert_eq!(editor.top_line(), end, "and stops there");
}

#[test]
fn text_input_wheel_scroll_stays_until_the_next_edit() {
  let mut editor = Editor::new(&LINES);
  editor.wheel(1000.0);
  assert_eq!(editor.top_line().0, 0);

  editor.tree.request_redraw();
  editor.render();
  editor.render();
  assert_eq!(
    editor.top_line().0,
    0,
    "laying out again does not jump back to the caret"
  );

  editor.key("g");
  assert_eq!(editor.top_line().0, 3, "an edit brings the caret's line back");
  assert!(editor.value.get().ends_with("ffffffg"));
}

#[test]
fn text_input_wheel_does_nothing_while_the_text_fits() {
  let mut editor = Editor::new(&LINES[..2]);
  let before = editor.top_line();
  assert_eq!(before.0, 0);

  editor.wheel(-100.0);
  assert_eq!(editor.top_line(), before);
  editor.wheel(100.0);
  assert_eq!(editor.top_line(), before);
}

#[test]
fn text_input_scrollbar_shows_only_while_the_text_overflows() {
  let mut editor = Editor::new(&LINES[..2]);
  assert!(editor.thumb().is_none(), "two lines fit in three rows");

  editor.key("Enter");
  editor.key("x");
  assert!(editor.thumb().is_none(), "three lines fit in three rows");

  editor.key("Enter");
  editor.key("y");
  let thumb = editor.thumb().expect("four lines overflow three rows");
  assert_eq!(thumb.color, THUMB);

  editor.key("Backspace");
  editor.key("Backspace");
  assert!(editor.thumb().is_none(), "back to three lines, nothing overflows");
  assert_eq!(editor.top_line().0, 0, "and every line shows from the first");
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn text_input_scrollbar_thumb_hovers_and_drags() {
  let mut editor = Editor::new(&LINES);
  let thumb = editor.thumb().expect("six lines overflow three rows");
  let (x, y) = (thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);

  editor.tree.mouse_move(x, y);
  assert_eq!(editor.thumb().map(|thumb| thumb.color), Some(THUMB_HOVERED));

  editor.tree.mouse_down(x, y, MouseButton::Left);
  editor.tree.mouse_move(x, y - 200.0);
  editor.tree.mouse_up(x, y - 200.0, MouseButton::Left);
  assert_eq!(editor.top_line().0, 0, "dragging the thumb up shows the first line");
  assert_eq!(editor.value.get(), LINES.join("\n"), "the drag edits nothing");
}

#[test]
fn text_input_scrolled_text_clamps_when_lines_are_deleted() {
  let mut editor = Editor::new(&LINES);
  editor.wheel(14.0);
  for _ in 0..("eeeee\nffffff".len() + 1) {
    editor.key("Backspace");
  }
  assert_eq!(editor.value.get(), LINES[..4].join("\n"));
  assert_eq!(editor.top_line().0, 1, "four lines in three rows show the last three");

  for _ in 0..("dddd".len() + 1) {
    editor.key("Backspace");
  }
  let unscrolled = Editor::new(&LINES[..3]).top_line();
  assert_eq!(editor.top_line(), unscrolled, "three lines show from the first");
  assert!(editor.thumb().is_none());
}

#[test]
fn text_input_scrollbar_ignores_rounding_in_rows_that_fit() {
  // Six typed 16.9 px lines add up to a hair over the height of six rows.
  let style = TextStyle {
    font_size: 13.0,
    line_height: 1.3,
    ..TextStyle::default()
  };
  let mut editor = Editor::with_rows(&[], 6, style);
  for (index, line) in LINES.iter().enumerate() {
    if index > 0 {
      editor.key("Enter");
    }
    for letter in line.chars() {
      editor.key(&letter.to_string());
    }
  }
  assert!(editor.thumb().is_none(), "six lines fit in six rows");
  let before = editor.top_line();
  editor.wheel(-100.0);
  assert_eq!(editor.top_line(), before, "and do not scroll");
}
