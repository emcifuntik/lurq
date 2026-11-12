//! A drawn frame holds no snapshot of the glyph atlas once it is drawn, so
//! the glyphs of the next frame are packed where the atlas's pixels are, not
//! into a copy of them.

use super::Tree;
use crate::{
  app::{
    App,
    capture_test_engine::{GlyphPainter, TestSurface},
  },
  components::Text,
};

fn drawn(text: &str) -> (Tree, App) {
  let (factory, _screen) = GlyphPainter::factory();
  let mut tree = Tree::new();
  tree.set_render_engine_factory(factory);
  tree.resize(300, 120);
  tree.set_root(Text::new(text));
  let mut app = App::new();
  assert!(tree.pass(&mut app, &TestSurface).rendered);
  (tree, app)
}

fn atlas_pixels(app: &App) -> *const u8 {
  app.shared.glyph_engine.lock().atlas().data.as_ptr()
}

#[test]
fn a_drawn_frame_holds_no_atlas_snapshot() {
  let (tree, app) = drawn("abc");

  assert!(tree.cached_render_list.is_some(), "the frame's list is kept");
  assert_eq!(app.shared.glyph_engine.lock().atlas_snapshots_held(), 0);
}

#[test]
fn new_glyphs_of_the_next_frame_are_packed_in_place() {
  let (mut tree, mut app) = drawn("abc");
  let pixels = atlas_pixels(&app);
  let version = app.shared.glyph_engine.lock().atlas().version;

  tree.set_root(Text::new("xyz"));
  assert!(tree.pass(&mut app, &TestSurface).rendered);

  assert!(app.shared.glyph_engine.lock().atlas().version > version, "new glyphs");
  assert_eq!(atlas_pixels(&app), pixels);
}

#[test]
fn a_frame_drawn_again_from_its_kept_list_takes_the_atlas_and_lets_go_of_it() {
  let (mut tree, mut app) = drawn("abc");

  // A scheduled redraw with nothing changed draws the kept list.
  tree.request_redraw_at(std::time::Instant::now());
  tree.tick_scheduled_redraw(std::time::Instant::now());
  let report = tree.pass(&mut app, &TestSurface);

  assert!(report.rendered);
  assert!(report.used_cached_render_list, "the kept list was drawn");
  assert_eq!(app.shared.glyph_engine.lock().atlas_snapshots_held(), 0);
}
