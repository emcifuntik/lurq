//! `Text::max_lines`: a wrapped text takes at most that many lines and ends
//! in an ellipsis when it had more to say.

use lurq::{
  app::{App, Tree},
  components::{Column, Text},
  layout::{Constraints, Size, quad::QuadContent},
  node::Element,
};

use crate::support::TestSurface;

const LONG: &str = "one two three four five six seven eight nine ten eleven twelve";

fn painted(root: impl Into<Element>) -> Tree {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(root);
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 300.0))));
  tree.pass(&mut app, &TestSurface);
  tree
}

/// The string the text paints and the height of its box.
fn shown(tree: &mut Tree) -> (String, f32) {
  let text = tree
    .resolve_quads(tree.last_layout().unwrap())
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::Text { text, .. } => Some(text.clone()),
      _ => None,
    })
    .expect("the text is painted");
  let height = tree
    .get_element_by_id_mut("text")
    .and_then(|element| element.bounds())
    .expect("the text is laid out")
    .height;
  (text, height)
}

fn narrow(text: Text) -> Column {
  Column::new().width(120.0).child(text.id("text"))
}

fn height_of_lines(lines: usize) -> f32 {
  let text = vec!["x"; lines].join("\n");
  shown(&mut painted(narrow(Text::new(&text)))).1
}

#[test]
fn a_text_with_more_lines_than_its_limit_ends_in_an_ellipsis_on_the_last() {
  let (unlimited, full_height) = shown(&mut painted(narrow(Text::new(LONG))));
  assert_eq!(unlimited, LONG);
  assert!(full_height > height_of_lines(2), "the text needs more than two lines");

  let (text, height) = shown(&mut painted(narrow(Text::new(LONG).max_lines(2))));

  let kept = text.strip_suffix('…').expect("it ends in an ellipsis");
  assert!(
    LONG.starts_with(kept) && !kept.is_empty(),
    "it keeps the start of the text: {kept:?}"
  );
  assert!(!kept.ends_with(' '), "with no space before the ellipsis");
  assert_eq!(height, height_of_lines(2), "and takes two lines");
}

#[test]
fn a_limit_of_one_line_keeps_what_fits_on_it() {
  let (text, height) = shown(&mut painted(narrow(Text::new(LONG).max_lines(1))));

  assert!(text.ends_with('…'));
  assert_eq!(height, height_of_lines(1));
  let (two, _) = shown(&mut painted(narrow(Text::new(LONG).max_lines(2))));
  assert!(two.len() > text.len(), "two lines keep more than one");
}

#[test]
fn a_text_that_fits_its_limit_is_left_as_it_is() {
  let (text, height) = shown(&mut painted(narrow(Text::new("one two").max_lines(3))));

  assert_eq!(text, "one two");
  assert_eq!(height, height_of_lines(1));
}

#[test]
fn lines_ended_by_a_line_break_count() {
  let (text, height) = shown(&mut painted(narrow(Text::new("first\nsecond\nthird").max_lines(2))));

  assert_eq!(text, "first\nsecond…");
  assert_eq!(height, height_of_lines(2));
}

#[test]
fn no_limit_and_a_text_that_does_not_wrap_are_not_cut() {
  let (unlimited, _) = shown(&mut painted(narrow(Text::new(LONG).max_lines(0))));
  assert_eq!(unlimited, LONG);

  let (single, height) = shown(&mut painted(narrow(Text::new(LONG).nowrap().max_lines(2))));
  assert_eq!(single, LONG);
  assert_eq!(height, height_of_lines(1));
}
