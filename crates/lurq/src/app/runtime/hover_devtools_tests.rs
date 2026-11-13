//! While DevTools is open on a tree, every change of its hover asks for a frame: the element tree and the
//! inspector show which nodes are hovered, and are brought up to date after a frame of the tree.

use super::Tree;
use crate::{
  app::App,
  components::{Column, Rect},
};

#[test]
fn every_change_of_the_hover_asks_for_a_frame_while_devtools_is_open() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.set_root(
    Column::new()
      .child(Rect::new(200.0, 40.0))
      .child(Rect::new(200.0, 40.0)),
  );
  tree.mount_devtools(&mut app);
  tree.pass_headless(&mut app);

  tree.mouse_move(20.0, 20.0);
  assert!(
    !tree.needs_redraw(),
    "closed, a hover that shows nothing asks for no frame"
  );

  assert!(tree.open_devtools());
  tree.pass_headless(&mut app);
  tree.mouse_move(20.0, 60.0);
  assert!(tree.needs_redraw(), "open, DevTools shows the hover that moved");

  tree.pass_headless(&mut app);
  tree.mouse_leave_window();
  assert!(tree.needs_redraw(), "and the hover that went");

  assert!(tree.close_devtools());
  tree.pass_headless(&mut app);
  tree.mouse_move(20.0, 20.0);
  assert!(!tree.needs_redraw(), "closed again, the hover asks for none");
}
