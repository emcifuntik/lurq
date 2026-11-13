//! A text draws its underline and the line through it under and through the
//! words of every line it is laid out in.

use lurq::{
  app::{App, Tree},
  components::{Column, Text},
  layout::{
    Constraints, Size,
    quad::{Quad, QuadContent},
    text_style::{TextStrikethrough, TextStyle, TextUnderline},
  },
  node::{Element, color::Color},
};

use crate::support::TestSurface;

const RED: Color = Color::new(200, 30, 30, 255);
const BLACK: Color = Color::new(0, 0, 0, 255);

fn painted(app: &mut App, root: impl Into<Element>) -> Tree {
  let mut tree = Tree::new();
  tree.set_root(root);
  tree.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 300.0))));
  tree.pass(app, &TestSurface);
  tree
}

fn quads(tree: &Tree) -> Vec<Quad> {
  tree.resolve_quads(tree.last_layout().unwrap())
}

/// The lines drawn with the text: every filled rectangle of the paint.
fn lines(tree: &Tree) -> Vec<(f32, f32, f32, f32, Color)> {
  quads(tree)
    .iter()
    .filter_map(|quad| match &quad.content {
      QuadContent::Rect { color, .. } if color.a() > 0 => Some((quad.x, quad.y, quad.width, quad.height, *color)),
      _ => None,
    })
    .collect()
}

fn text_box(tree: &Tree) -> (f32, f32, f32, f32) {
  quads(tree)
    .iter()
    .find_map(|quad| match &quad.content {
      QuadContent::Text { .. } => Some((quad.x, quad.y, quad.width, quad.height)),
      _ => None,
    })
    .expect("the text is painted")
}

#[test]
fn a_text_without_a_decoration_draws_no_line() {
  let mut app = App::new();
  let tree = painted(&mut app, Text::new("Save changes"));

  assert!(lines(&tree).is_empty());
}

#[test]
fn an_underline_in_the_style_of_a_plain_text_is_drawn_under_its_words() {
  let mut app = App::new();
  let style = TextStyle {
    underline: Some(TextUnderline::new()),
    ..TextStyle::default()
  };
  let tree = painted(&mut app, Text::styled("Save changes", style));

  let lines = lines(&tree);
  assert_eq!(lines.len(), 1);
  let (x, y, width, height, color) = lines[0];
  let (text_x, text_y, text_width, text_height) = text_box(&tree);
  assert_eq!(color, BLACK, "the line is the text's color");
  assert!(height >= 1.0);
  assert!(
    (x - text_x).abs() < 1.0 && (width - text_width).abs() < 2.0,
    "it spans the words: {x} {width}"
  );
  assert!(
    y > text_y + text_height * 0.5 && y + height <= text_y + text_height + 1.0,
    "it lies in the lower half of the line: {y} in {text_y}+{text_height}"
  );
}

#[test]
fn the_underline_of_a_text_takes_its_color_thickness_and_offset() {
  let underlined =
    |offset: f32| Text::new("Save").underline(TextUnderline::new().color(RED).thickness(3.0).offset(offset));
  let mut app = App::new();
  let near = painted(&mut app, underlined(1.0));
  let far = painted(&mut app, underlined(5.0));

  let (_, near_y, _, height, color) = lines(&near)[0];
  let (_, far_y, ..) = lines(&far)[0];
  assert_eq!((height, color), (3.0, RED));
  assert_eq!(far_y - near_y, 4.0, "the offset moves the line down");
}

#[test]
fn a_line_through_a_text_lies_above_its_underline() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Text::new("Done")
      .underline(TextUnderline::new())
      .strikethrough(TextStrikethrough::new().color(RED)),
  );

  let lines = lines(&tree);
  assert_eq!(lines.len(), 2);
  let underline = lines.iter().find(|line| line.4 == BLACK).expect("the underline");
  let strike = lines.iter().find(|line| line.4 == RED).expect("the line through");
  let (_, text_y, _, text_height) = text_box(&tree);
  assert!(strike.1 < underline.1, "the strike is above the underline");
  assert!(
    strike.1 > text_y && strike.1 < text_y + text_height,
    "and inside the line"
  );
  assert!((strike.2 - underline.2).abs() < 0.5, "both span the words");
}

#[test]
fn a_wrapped_text_is_underlined_line_by_line() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new().width(90.0).child(
      Text::new("one two three four five six seven eight")
        .underline(TextUnderline::new())
        .id("text"),
    ),
  );

  let lines = lines(&tree);
  assert!(lines.len() >= 3, "one underline per laid-out line: {}", lines.len());
  for pair in lines.windows(2) {
    assert!(pair[1].1 > pair[0].1, "each lower than the one before");
  }
  for (x, _, width, ..) in &lines {
    assert!(*x >= 0.0 && x + width <= 91.0, "inside the text's box: {x} {width}");
  }
}

#[test]
fn a_decoration_is_drawn_in_the_foreground_the_text_takes() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new()
      .foreground(RED)
      .child(Text::new("Link").underline(TextUnderline::new())),
  );

  assert_eq!(lines(&tree)[0].4, RED);
}

#[test]
fn a_decoration_does_not_change_the_size_of_its_text() {
  let mut app = App::new();
  let plain = painted(&mut app, Text::new("Save changes"));
  let decorated = painted(
    &mut app,
    Text::new("Save changes")
      .underline(TextUnderline::new().thickness(4.0))
      .strikethrough(TextStrikethrough::new()),
  );

  assert_eq!(plain.last_layout().unwrap().size, decorated.last_layout().unwrap().size);
}
