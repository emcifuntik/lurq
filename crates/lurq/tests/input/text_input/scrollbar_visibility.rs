//! A multiline input's scrollbar shown only on hover (`ScrollBarVisibility::OnHover`) or never
//! (`ScrollBarVisibility::Never`): the first shows while the pointer is over the input and while its text
//! scrolls, to keep the caret in view too, then fades out; the second is never painted and never takes the
//! pointer, and the wheel still scrolls the text.

use std::time::{Duration, Instant};

use lurq::{
  animation::Easing,
  app::{App, Tree, events::ScrollPhase},
  components::{Column, TextInput},
  core::Signal,
  layout::{
    scrollbar::{ScrollBarStyle, ScrollBarVisibility},
    text_style::TextStyle,
  },
  node::{CursorIcon, color::Color},
};

use crate::support::{RectSnapshot, RenderSnapshot, render_pass_with_app};

const ID: &str = "visibility-input";
const THUMB: Color = Color::new(200, 30, 90, 255);
const PADDING: f32 = 6.0;
const DELAY: Duration = Duration::from_millis(600);
const FADE: Duration = Duration::from_millis(200);
/// Outside the input.
const AWAY: (f32, f32) = (350.0, 350.0);
/// Line `n` (from 0) has `n + 1` letters, so the glyphs of the top row tell which line it is.
const LINES: [&str; 6] = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff"];

struct Editor {
  tree: Tree,
  app: App,
  start: Instant,
}

impl Editor {
  /// A 3-row input showing `LINES`, focused with the caret at their end, on a clock the test moves; the clock
  /// starts after the bar that the first scroll to the caret showed has gone.
  fn new(visible: ScrollBarVisibility) -> Self {
    let mut tree = Tree::new();
    tree.resize(400, 400);
    tree.set_root(
      Column::new().padding(20.0).child(
        TextInput::styled(
          Signal::new(LINES.join("\n")),
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
        .scrollbar(
          ScrollBarStyle {
            thumb_color: THUMB,
            visible,
            fade_easing: Easing::Linear,
            ..ScrollBarStyle::default()
          }
          .fade(DELAY, FADE),
        ),
      ),
    );
    let mounted = Instant::now();
    tree.set_animation_clock(Some(mounted));
    let mut editor = Self {
      tree,
      app: App::new(),
      start: mounted + DELAY + FADE + Duration::from_secs(1),
    };
    editor.render();
    editor.tree.get_element_by_id_mut(ID).unwrap().focus();
    editor.render();
    editor.at(Duration::ZERO);
    editor.render();
    editor
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  /// Moves the clock to `after` since the editor settled.
  fn at(&mut self, after: Duration) {
    self.tree.set_animation_clock(Some(self.start + after));
  }

  fn center(&mut self) -> (f32, f32) {
    let bounds = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
    (bounds.x + bounds.width / 2.0, bounds.y + bounds.height / 2.0)
  }

  /// Where the middle of the bar is, shown or not: at the input's right edge.
  fn bar_place(&mut self) -> (f32, f32) {
    let bounds = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap();
    let style = ScrollBarStyle::default();
    (
      bounds.x + bounds.width - style.resolved_edge_inset() - style.width / 2.0,
      bounds.y + bounds.height - 12.0,
    )
  }

  fn key(&mut self, key: &str) {
    self.tree.key_down(key.into(), key.into(), false, false, false);
    self.tree.key_up(key.into(), key.into(), false, false, false);
    self.render();
  }

  fn thumb(&mut self) -> Option<RectSnapshot> {
    self
      .render()
      .rects
      .into_iter()
      .find(|rect| (rect.color.r(), rect.color.g(), rect.color.b()) == (THUMB.r(), THUMB.g(), THUMB.b()))
  }

  /// The line at the top of the input's rows, from the glyphs painted inside the padding.
  fn top_line(&mut self) -> usize {
    let rows_top = self.tree.get_element_by_id_mut(ID).unwrap().bounds().unwrap().y + PADDING;
    let snapshot = self.render();
    let mut glyphs: Vec<_> = snapshot
      .glyphs
      .iter()
      .filter(|glyph| glyph.clip.active && glyph.y + glyph.height > rows_top)
      .collect();
    glyphs.sort_by(|a, b| a.y.total_cmp(&b.y));
    let top = glyphs.first().expect("the input shows text");
    glyphs.iter().filter(|glyph| (glyph.y - top.y).abs() < 2.0).count() - 1
  }
}

#[test]
fn text_input_on_hover_scrollbar_shows_under_the_pointer_and_fades_after_it_leaves() {
  let mut editor = Editor::new(ScrollBarVisibility::OnHover);
  assert!(editor.thumb().is_none(), "the bar is hidden at rest");

  let (x, y) = editor.center();
  editor.tree.mouse_move(x, y);
  let thumb = editor.thumb().expect("the pointer over the input shows the bar");
  assert_eq!(thumb.color.a(), 255);

  editor.tree.mouse_move(AWAY.0, AWAY.1);
  assert!(editor.thumb().is_some(), "the bar stays when the pointer leaves");
  editor.at(DELAY + FADE / 2);
  let fading = editor.thumb().expect("the bar is fading").color.a();
  assert!(fading > 0 && fading < 255, "part way through its fade: {fading}");
  editor.at(DELAY + FADE);
  assert!(editor.thumb().is_none(), "then it is gone");
}

#[test]
fn text_input_on_hover_scrollbar_shows_while_typing_scrolls_the_text() {
  let mut editor = Editor::new(ScrollBarVisibility::OnHover);
  editor.tree.mouse_move(AWAY.0, AWAY.1);
  assert!(editor.thumb().is_none());

  editor.key("Enter");
  assert!(
    editor.thumb().is_some(),
    "a new line scrolls the caret into view, which shows the bar"
  );

  editor.at(DELAY + FADE);
  assert!(editor.thumb().is_none(), "and it fades out once the text is still");
}

#[test]
fn text_input_hidden_on_hover_scrollbar_leaves_its_place_to_the_text() {
  let mut editor = Editor::new(ScrollBarVisibility::OnHover);
  editor.tree.mouse_move(AWAY.0, AWAY.1);
  editor.render();

  // The pointer comes straight onto the bar's place: the bar was hidden there, so the text has the pointer.
  let (x, y) = editor.bar_place();
  editor.tree.mouse_move(x, y);
  assert_eq!(editor.tree.cursor(), CursorIcon::Text);

  // That move showed the bar; from the next one it has the pointer.
  let thumb = editor.thumb().expect("the pointer over the input shows the bar");
  editor
    .tree
    .mouse_move(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
  assert_eq!(editor.tree.cursor(), CursorIcon::Default);
}

#[test]
fn text_input_never_shown_scrollbar_is_not_painted_and_the_wheel_still_scrolls() {
  let mut editor = Editor::new(ScrollBarVisibility::Never);
  assert_eq!(editor.top_line(), 3, "the caret's line shows at the bottom");

  let (x, y) = editor.center();
  editor.tree.mouse_move(x, y);
  assert!(editor.thumb().is_none(), "no bar under the pointer");

  let (bar_x, bar_y) = editor.bar_place();
  editor.tree.mouse_move(bar_x, bar_y);
  editor.render();
  editor.tree.mouse_move(bar_x, bar_y - 1.0);
  assert_eq!(
    editor.tree.cursor(),
    CursorIcon::Text,
    "where a bar would run, the text has the pointer"
  );

  editor.tree.scroll(x, y, 0.0, 1000.0, ScrollPhase::Scroll);
  assert_eq!(editor.top_line(), 0, "the wheel scrolls to the first line");
  assert!(editor.thumb().is_none(), "and still paints no bar");
}
