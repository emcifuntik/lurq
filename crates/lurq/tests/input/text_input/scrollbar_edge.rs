//! A multiline input places its scrollbar from its own box, as a scroll
//! container does, however much padding it has: the bar runs `edge_inset`
//! from the right edge and its track ends `end_inset` from the top and bottom
//! edges. The padding is the text's inset and scrolls with the text: scrolled
//! down, the text runs through the top padding, and the bottom padding shows
//! under the last line at the end.

use lurq::{
  app::{
    App, Tree,
    events::{MouseButton, ScrollPhase},
  },
  components::{Column, TextInput},
  core::Signal,
  layout::{
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
    text_style::TextStyle,
  },
  node::color::Color,
};

use crate::support::{GlyphSnapshot, RectSnapshot, RenderSnapshot, render_pass_with_app};

const ID: &str = "edge-input";
const THUMB: Color = Color::new(200, 30, 90, 255);
const THUMB_HOVERED: Color = Color::new(30, 200, 90, 255);
const TRACK: Color = Color::new(30, 90, 200, 255);
const CARET: Color = Color::new(255, 0, 255, 255);
const PADDING: f32 = 16.0;
const BAR_WIDTH: f32 = 6.0;
const EDGE_INSET: f32 = 3.0;
const END_INSET: f32 = 5.0;
const LINE_HEIGHT: f32 = 13.0 * 1.4;
/// Line `n` (from 0) has `n + 1` letters, so the glyphs of a row tell which
/// line it is.
const LINES: [&str; 8] = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff", "ggggggg", "hhhhhhhh"];

struct Bounds {
  x: f32,
  y: f32,
  width: f32,
  height: f32,
}

impl Bounds {
  fn right(&self) -> f32 {
    self.x + self.width
  }

  fn bottom(&self) -> f32 {
    self.y + self.height
  }
}

struct Editor {
  tree: Tree,
  app: App,
  value: Signal<String>,
}

impl Editor {
  /// A 3-row input with 16 px of padding showing `LINES`, focused with the
  /// caret at their end, so it is scrolled to its end.
  fn new(placement: ScrollBarPlacement) -> Self {
    let value = Signal::new(LINES.join("\n"));
    let mut tree = Tree::new();
    tree.resize(400, 400);
    tree.set_root(
      Column::new().padding(20.0).child(
        TextInput::styled(
          value.clone(),
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
        .caret_color(CARET)
        .scrollbar(
          ScrollBarStyle {
            thumb_color: THUMB,
            track_color: TRACK,
            width: BAR_WIDTH,
            placement,
            ..ScrollBarStyle::default()
          }
          .insets(EDGE_INSET, END_INSET),
        )
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

  fn bounds(&mut self) -> Bounds {
    let bounds = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
    Bounds {
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
    }
  }

  fn wheel(&mut self, delta_y: f32) {
    let bounds = self.bounds();
    let (x, y) = (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0);
    self.tree.scroll(x, y, 0.0, delta_y, ScrollPhase::Scroll);
    self.render();
  }

  fn key(&mut self, key: &str) {
    self.tree.key_down(key.into(), key.into(), false, false, false);
    self.tree.key_up(key.into(), key.into(), false, false, false);
    self.render();
  }

  fn click(&mut self, x: f32, y: f32) {
    self.tree.mouse_move(x, y);
    self.tree.mouse_down(x, y, MouseButton::Left);
    self.tree.mouse_up(x, y, MouseButton::Left);
    self.render();
  }

  fn rect(&mut self, colors: &[Color]) -> Option<RectSnapshot> {
    self
      .render()
      .rects
      .into_iter()
      .find(|rect| colors.contains(&rect.color))
  }

  fn thumb(&mut self) -> RectSnapshot {
    self
      .rect(&[THUMB, THUMB_HOVERED])
      .expect("eight lines overflow three rows")
  }

  fn track(&mut self) -> RectSnapshot {
    self.rect(&[TRACK]).expect("the track is painted")
  }

  fn caret(&mut self) -> RectSnapshot {
    self.rect(&[CARET]).expect("the focused input paints its caret")
  }

  /// The glyphs the input shows at least partly inside its clip.
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

  /// The shown glyphs of line `line`, found by its letter count.
  fn line(&mut self, line: usize) -> Vec<GlyphSnapshot> {
    let glyphs = self.glyphs();
    glyphs
      .iter()
      .filter(|glyph| glyphs.iter().filter(|other| (other.y - glyph.y).abs() < 2.0).count() == line + 1)
      .copied()
      .collect()
  }

  /// The line whose row holds `y`, with the text scrolled to its end.
  fn row_center_at_end(&mut self, line: usize) -> f32 {
    let bounds = self.bounds();
    let rows_below = (LINES.len() - 1 - line) as f32;
    bounds.bottom() - PADDING - (rows_below + 0.5) * LINE_HEIGHT
  }
}

fn assert_close(actual: f32, expected: f32, what: &str) {
  assert!(
    (actual - expected).abs() < 0.01,
    "{what}: {actual}, expected {expected}"
  );
}

/// A glyph's clip is the text's box grown by the pixel of slop glyph rasterization gets.
fn assert_clip_edge(actual: f32, expected: f32, what: &str) {
  assert!(
    (actual - expected).abs() <= 1.0,
    "{what}: {actual}, expected {expected}"
  );
}

#[test]
fn text_input_scrollbar_runs_at_the_edge_of_a_padded_input() {
  for placement in [ScrollBarPlacement::Overlay, ScrollBarPlacement::Reserved] {
    let mut editor = Editor::new(placement);
    let bounds = editor.bounds();
    let track = editor.track();
    assert_close(
      track.x,
      bounds.right() - EDGE_INSET - BAR_WIDTH,
      "the track's left edge",
    );
    assert_close(track.width, BAR_WIDTH, "the track's width");
    assert_close(track.y, bounds.y + END_INSET, "the track's top");
    assert_close(
      track.y + track.height,
      bounds.bottom() - END_INSET,
      "the track's bottom",
    );

    let thumb = editor.thumb();
    assert_close(thumb.x, track.x, "the thumb's left edge");
    assert_close(
      thumb.y + thumb.height,
      track.y + track.height,
      "the thumb's bottom at the end",
    );
    assert!(thumb.y > track.y, "the thumb is shorter than its track: {thumb:?}");
  }
}

#[test]
fn text_input_reserved_scrollbar_keeps_the_padding_between_the_bar_and_the_text() {
  let mut reserved = Editor::new(ScrollBarPlacement::Reserved);
  let track = reserved.track();
  let text_right = reserved
    .glyphs()
    .iter()
    .map(|glyph| glyph.clip.x + glyph.clip.width)
    .fold(f32::MIN, f32::max);
  assert_clip_edge(
    text_right,
    track.x - EDGE_INSET - PADDING,
    "the text's box ends a padding left of the bar's lane",
  );

  let mut overlay = Editor::new(ScrollBarPlacement::Overlay);
  let bounds = overlay.bounds();
  let text_right = overlay
    .glyphs()
    .iter()
    .map(|glyph| glyph.clip.x + glyph.clip.width)
    .fold(f32::MIN, f32::max);
  assert_clip_edge(
    text_right,
    bounds.right() - PADDING,
    "an overlay bar takes no room from the text",
  );
}

#[test]
fn text_input_scrollbar_takes_the_pointer_where_it_is_drawn() {
  let mut editor = Editor::new(ScrollBarPlacement::Overlay);
  let thumb = editor.thumb();
  let (x, y) = (thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);

  // Where the bar was drawn when it was placed inside the padding.
  editor.tree.mouse_move(x - PADDING, y);
  assert_eq!(
    editor.thumb().color,
    THUMB,
    "the padding left of the bar is not the bar"
  );

  editor.tree.mouse_move(x, y);
  assert_eq!(
    editor.thumb().color,
    THUMB_HOVERED,
    "the pointer on the thumb hovers it"
  );

  editor.tree.mouse_down(x, y, MouseButton::Left);
  editor.tree.mouse_move(x, y - 200.0);
  editor.tree.mouse_up(x, y - 200.0, MouseButton::Left);
  let track = editor.track();
  assert_close(
    editor.thumb().y,
    track.y,
    "dragging the thumb up brings it to the track's top",
  );
  assert_eq!(editor.line(0).len(), 1, "and shows the first line");
  assert_eq!(editor.value.get(), LINES.join("\n"), "the drag edits nothing");
}

#[test]
fn text_input_scrollbar_track_press_pages_the_text() {
  let mut editor = Editor::new(ScrollBarPlacement::Overlay);
  let track = editor.track();
  let at_end = editor.thumb();

  editor.click(track.x + track.width / 2.0, track.y + 2.0);
  let paged = editor.thumb();
  assert!(
    paged.y < at_end.y,
    "a press on the track above the thumb pages up: {paged:?}"
  );
  assert_eq!(editor.value.get(), LINES.join("\n"), "the press edits nothing");
}

#[test]
fn text_input_padding_scrolls_with_the_text() {
  let mut editor = Editor::new(ScrollBarPlacement::Overlay);
  let bounds = editor.bounds();

  // At the end, the last line sits a padding above the bottom edge, and the
  // lines above the three rows run on through the top padding.
  let last = editor.line(LINES.len() - 1);
  assert_eq!(last.len(), LINES.len(), "the last line shows");
  let clip = last[0].clip;
  assert_clip_edge(clip.y, bounds.y, "the text is clipped at the top edge");
  assert_clip_edge(clip.y + clip.height, bounds.bottom(), "and at the bottom edge");
  for glyph in &last {
    assert!(
      glyph.y + glyph.height <= bounds.bottom() - PADDING,
      "the bottom padding shows under the last line: {glyph:?}"
    );
    assert!(glyph.y > bounds.bottom() - PADDING - LINE_HEIGHT);
  }
  let above = editor.line(LINES.len() - 4);
  assert!(
    above
      .iter()
      .any(|glyph| glyph.y < bounds.y + PADDING && glyph.y + glyph.height > bounds.y),
    "the line above the rows shows in the top padding: {above:?}"
  );

  // At the start, the first line sits a padding below the top edge, and the
  // lines below the three rows run on through the bottom padding.
  editor.wheel(1000.0);
  let first = editor.line(0);
  assert_eq!(first.len(), 1, "the first line shows");
  assert!(
    first[0].y >= bounds.y + PADDING && first[0].y < bounds.y + PADDING + LINE_HEIGHT,
    "the top padding shows over the first line: {first:?}"
  );
  let below = editor.line(3);
  assert!(
    below
      .iter()
      .any(|glyph| glyph.y < bounds.bottom() && glyph.y + glyph.height > bounds.bottom() - PADDING),
    "the line below the rows shows in the bottom padding: {below:?}"
  );

  // Scrolling less than the padding moves the first line into the top
  // padding, where it still shows.
  editor.wheel(-6.0);
  let moved = editor.line(0);
  assert_eq!(moved.len(), 1, "the first line still shows");
  assert!(
    moved[0].y < first[0].y && moved[0].y > bounds.y,
    "the top padding scrolls away with the text: {moved:?}"
  );
}

#[test]
fn text_input_caret_stays_inside_the_padding_as_it_moves() {
  let mut editor = Editor::new(ScrollBarPlacement::Overlay);
  let bounds = editor.bounds();
  let (top, bottom) = (bounds.y + PADDING, bounds.bottom() - PADDING);

  let caret = editor.caret();
  assert!(
    caret.y >= bottom - LINE_HEIGHT && caret.y + caret.height <= bottom,
    "the caret is on the last row, a padding above the bottom edge: {caret:?}"
  );

  for _ in 0..LINES.len() - 1 {
    editor.key("ArrowUp");
    let caret = editor.caret();
    assert!(
      caret.y >= top && caret.y + caret.height <= bottom,
      "the caret's row stays in view inside the padding: {caret:?}"
    );
  }
  let caret = editor.caret();
  assert!(
    caret.y < top + LINE_HEIGHT,
    "the caret is on the first row, a padding below the top edge: {caret:?}"
  );
  let track = editor.track();
  assert_close(editor.thumb().y, track.y, "the text is scrolled to its start");

  editor.wheel(-1000.0);
  editor.key("x");
  assert!(editor.value.get().starts_with("ax\n"), "typing edits at the caret");
  let caret = editor.caret();
  assert!(
    caret.y >= top && caret.y < top + LINE_HEIGHT,
    "typing brings the caret's row back into view: {caret:?}"
  );
}

#[test]
fn text_input_click_lands_on_the_row_under_the_pointer_after_scrolling() {
  // The caret goes to the end of the line a click right of its text lands on.
  let line_end = |line: usize| LINES[..=line].join("\n").len();

  let mut editor = Editor::new(ScrollBarPlacement::Overlay);
  let bounds = editor.bounds();
  let x = bounds.x + bounds.width / 2.0;

  let y = editor.row_center_at_end(6);
  editor.click(x, y);
  editor.key("X");
  let mut expected = LINES.join("\n");
  expected.insert(line_end(6), 'X');
  assert_eq!(editor.value.get(), expected, "a click on the middle row edits its line");

  // The line above the three rows shows in the top padding; a click there
  // lands on it.
  let mut editor = Editor::new(ScrollBarPlacement::Overlay);
  let y = editor.row_center_at_end(4);
  assert!(
    y > bounds.y && y < bounds.y + PADDING,
    "the row above the rows is in the top padding"
  );
  editor.click(x, y);
  editor.key("X");
  let mut expected = LINES.join("\n");
  expected.insert(line_end(4), 'X');
  assert_eq!(
    editor.value.get(),
    expected,
    "a click in the top padding edits the line shown there"
  );
}
