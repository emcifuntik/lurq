use lurq::{
  app::{App, Tree},
  components::{Column, ScrollVertical, Text},
  node::dimension::Dimension,
};

use crate::support::run_pass;

#[test]
fn a_lists_own_padding_is_around_what_scrolls() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  let _ = &mut app;
  tree.set_root(
    ScrollVertical::new(Column::new().width(Dimension::Pct(100.0)).child(Text::new("inside")))
      .size(Dimension::Pct(100.0), Dimension::Pct(100.0))
      .padding_left(50.0)
      .padding_right(30.0),
  );
  run_pass(&mut tree);
  let rect = tree
    .find_element(|element| element.text_content() == Some("inside"))
    .unwrap()
    .rect();
  assert_eq!(rect.0, 50.0, "the content starts inside the list's padding: {rect:?}");
}
