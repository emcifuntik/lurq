//! A multiline input whose scrollbar is placed `Reserved` keeps a lane for
//! the bar at its right edge, outside its padding: the text wraps short of
//! it, so no glyph sits under the bar, and the lane stays while the text fits. A bar placed
//! `Overlay` still runs over the text. The pointer over the bar shows the
//! default cursor, whatever cursor the input sets.

use lurq::{
  app::{App, Tree},
  components::{Column, TextInput},
  core::Signal,
  layout::{
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::TextStyle,
  },
  node::{CursorIcon, color::Color},
};

use crate::support::{GlyphSnapshot, RectSnapshot, RenderSnapshot, render_pass_with_app};

const ID: &str = "lane-input";
const THUMB: Color = Color::new(200, 30, 90, 255);
const THUMB_HOVERED: Color = Color::new(30, 200, 90, 255);
const PADDING: f32 = 6.0;

struct Editor {
  tree: Tree,
  app: App,
}

impl Editor {
  /// A 200 px wide input of up to 3 rows showing `value`, with a scrollbar
  /// placed `placement` that widens to `hovered_width` under the pointer.
  fn new(value: &str, placement: ScrollBarPlacement, hovered_width: f32) -> Self {
    let mut tree = Tree::new();
    tree.resize(400, 400);
    tree.set_root(
      Column::new().padding(20.0).child(
        TextInput::styled(
          Signal::new(value.to_owned()),
          TextStyle {
            font_size: 13.0,
            line_height: 1.4,
            ..TextStyle::default()
          },
        )
        .id(ID)
        .rows(1, 3)
        .width(200.0)
        .padding(PADDING)
        .cursor(CursorIcon::Text)
        .scrollbar(ScrollBarStyle {
          thumb_color: THUMB,
          placement,
          ..ScrollBarStyle::default()
        })
        .scrollbar_hovered(move |style| ScrollBarStyle {
          thumb_color: THUMB_HOVERED,
          width: hovered_width,
          ..style
        }),
      ),
    );
    let mut editor = Self { tree, app: App::new() };
    editor.render();
    editor
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  /// The glyphs painted inside the input's clip.
  fn glyphs(&mut self) -> Vec<GlyphSnapshot> {
    self
      .render()
      .glyphs
      .into_iter()
      .filter(|glyph| {
        glyph.clip.active && glyph.y + glyph.height > glyph.clip.y && glyph.y < glyph.clip.y + glyph.clip.height
      })
      .collect()
  }

  /// The right edge of the rightmost glyph the input shows.
  fn text_right(&mut self) -> f32 {
    self
      .glyphs()
      .iter()
      .map(|glyph| glyph.x + glyph.width)
      .fold(f32::MIN, f32::max)
  }

  /// How many glyphs the top row shows.
  fn first_row_len(&mut self) -> usize {
    let glyphs = self.glyphs();
    let top = glyphs.iter().map(|glyph| glyph.y).fold(f32::MAX, f32::min);
    glyphs.iter().filter(|glyph| (glyph.y - top).abs() < 2.0).count()
  }

  fn thumb(&mut self) -> Option<RectSnapshot> {
    self
      .render()
      .rects
      .into_iter()
      .find(|rect| rect.color == THUMB || rect.color == THUMB_HOVERED)
  }

  /// The right edge of the input's box.
  fn right(&mut self) -> f32 {
    let bounds = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
    bounds.x + bounds.width
  }
}

/// A word too long for a row breaks between glyphs, so every row of narrow
/// glyphs reaches to within a glyph of the right edge.
fn long_word(len: usize) -> String {
  "l".repeat(len)
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn text_input_reserved_scrollbar_keeps_the_text_out_of_its_lane() {
  let mut editor = Editor::new(&long_word(400), ScrollBarPlacement::Reserved, 8.0);
  let thumb = editor.thumb().expect("400 glyphs overflow three rows");
  let text_right = editor.text_right();
  assert!(
    text_right <= thumb.x,
    "the text ends at {text_right}, left of the bar at {}",
    thumb.x
  );
  assert!(
    text_right + PADDING <= thumb.x,
    "the padding stays between the text and the bar at {}",
    thumb.x
  );
  let bar_right = editor.right() - ScrollBarStyle::default().resolved_edge_inset();
  assert!(
    (thumb.x + thumb.width - bar_right).abs() < 0.01,
    "the bar runs its edge inset from the input's right edge, at {bar_right}: {thumb:?}"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn text_input_reserved_scrollbar_lane_stays_while_the_text_fits() {
  let mut fits = Editor::new(&long_word(100), ScrollBarPlacement::Reserved, 8.0);
  assert!(fits.thumb().is_none(), "100 glyphs fit in three rows");
  let mut overflows = Editor::new(&long_word(400), ScrollBarPlacement::Reserved, 8.0);
  assert!(overflows.thumb().is_some());
  assert_eq!(
    fits.first_row_len(),
    overflows.first_row_len(),
    "the text wraps at the same width with and without the bar"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn text_input_reserved_scrollbar_lane_fits_the_hovered_bar() {
  let mut editor = Editor::new(&long_word(400), ScrollBarPlacement::Reserved, 14.0);
  let rest_row = editor.first_row_len();
  let thumb = editor.thumb().expect("400 glyphs overflow three rows");
  editor
    .tree
    .mouse_move(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
  let hovered = editor.thumb().expect("the bar stays");
  assert_eq!(hovered.color, THUMB_HOVERED);
  assert!(hovered.width > thumb.width, "the hovered bar is wider");
  let text_right = editor.text_right();
  assert!(
    text_right <= hovered.x,
    "the text ends at {text_right}, left of the hovered bar at {}",
    hovered.x
  );
  assert_eq!(
    editor.first_row_len(),
    rest_row,
    "hovering the bar does not re-wrap the text"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn text_input_overlay_scrollbar_still_runs_over_the_text() {
  let mut editor = Editor::new(&long_word(400), ScrollBarPlacement::Overlay, 8.0);
  let thumb = editor.thumb().expect("400 glyphs overflow three rows");
  assert!(
    editor.text_right() > thumb.x,
    "an overlay bar keeps the full width for the text"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn text_input_scrollbar_shows_the_default_cursor() {
  let mut editor = Editor::new(&long_word(400), ScrollBarPlacement::Reserved, 8.0);
  let thumb = editor.thumb().expect("400 glyphs overflow three rows");
  let bounds = editor.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();

  editor.tree.mouse_move(bounds.x + 30.0, bounds.y + bounds.height / 2.0);
  assert_eq!(
    editor.tree.cursor(),
    CursorIcon::Text,
    "the text shows the input's cursor"
  );

  editor
    .tree
    .mouse_move(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
  assert_eq!(
    editor.tree.cursor(),
    CursorIcon::Default,
    "the bar shows the default cursor"
  );

  editor.tree.mouse_move(bounds.x + 30.0, bounds.y + bounds.height / 2.0);
  assert_eq!(editor.tree.cursor(), CursorIcon::Text, "and the text its own again");
}
