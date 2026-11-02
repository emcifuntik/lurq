use lurq::{
  app::Tree,
  components::{Column, Rect},
  node::color::Color,
};

use crate::support::{render_pass, run_pass};

#[test]
fn style_mutation_through_handle_invalidates_and_renders() {
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Rect::new(30.0, 40.0).id("card").background("#22c55e")));
  run_pass(&mut tree);
  assert!(!tree.needs_redraw());

  tree.get_element_by_id_mut("card").unwrap().set_background("#ef4444");
  assert!(tree.needs_redraw());

  let snapshot = render_pass(&mut tree);
  let red = Color::from_hex("#ef4444");
  assert!(
    snapshot.rects.iter().any(|rect| rect.color == red),
    "mutated background should reach the render list"
  );
  assert_eq!(
    tree.get_element_by_id("card").unwrap().color(),
    Some(Color::from_hex("#ef4444"))
  );
}

#[test]
fn layout_affecting_mutation_relayouts() {
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Rect::new(30.0, 40.0).id("card").background("#22c55e")));
  run_pass(&mut tree);

  let bounds = tree.get_element_by_id_mut("card").unwrap().bounds().unwrap();
  assert_eq!((bounds.width, bounds.height), (30.0, 40.0));

  tree.get_element_by_id_mut("card").unwrap().set_size(50.0, 60.0);
  assert!(tree.needs_redraw());
  run_pass(&mut tree);

  let bounds = tree.get_element_by_id_mut("card").unwrap().bounds().unwrap();
  assert_eq!((bounds.width, bounds.height), (50.0, 60.0));
}

#[test]
fn bounds_are_none_before_first_layout_pass() {
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Rect::new(30.0, 40.0).id("card")));

  assert!(tree.get_element_by_id_mut("card").unwrap().bounds().is_none());

  run_pass(&mut tree);
  assert!(tree.get_element_by_id_mut("card").unwrap().bounds().is_some());
}
