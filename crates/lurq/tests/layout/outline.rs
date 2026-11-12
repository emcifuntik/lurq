//! An outline is a ring around an element's box that takes no room in layout.

use lurq::{
  app::{
    App, Tree,
    events::MouseButton,
    theme::{BorderSize, PaletteColor},
  },
  components::{Button, Column, Rect},
  layout::{
    Constraints, Size,
    quad::{Quad, QuadContent},
  },
  node::{
    Element, Outline,
    border::{BorderPlacement, BorderRadius},
    color::Color,
  },
};

use crate::support::{TestSurface, pointer_click};

const RING: Color = Color::new(37, 99, 235, 255);
const OTHER: Color = Color::new(220, 38, 38, 255);

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

/// The quads that draw a line of `color` and nothing else: an outline's ring.
fn rings(tree: &Tree, color: Color) -> Vec<Quad> {
  quads(tree)
    .into_iter()
    .filter(|quad| {
      matches!(&quad.content, QuadContent::Rect { color: fill, gradient: None } if fill.a() == 0)
        && quad.border.and_then(|border| border.top).map(|side| side.color) == Some(color)
    })
    .collect()
}

fn bounds(tree: &mut Tree, id: &str) -> (f32, f32, f32, f32) {
  let rect = tree
    .get_element_by_id_mut(id)
    .and_then(|element| element.bounds())
    .expect("element should be laid out");
  (rect.x, rect.y, rect.width, rect.height)
}

fn tab(tree: &mut Tree) {
  tree.key_down("Tab".to_owned(), "Tab".to_owned(), false, false, false);
  tree.key_up("Tab".to_owned(), "Tab".to_owned(), false, false, false);
}

#[test]
fn an_outline_is_a_ring_past_the_box_by_its_offset_and_width() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new().padding(20.0).child(
      Rect::new(100.0, 40.0)
        .rounded(6.0)
        .outline(Outline::new(2.0, RING).offset(3.0)),
    ),
  );

  let rings = rings(&tree, RING);
  assert_eq!(rings.len(), 1);
  let ring = &rings[0];
  assert_eq!((ring.x, ring.y, ring.width, ring.height), (15.0, 15.0, 110.0, 50.0));
  let border = ring.border.unwrap();
  for side in [border.top, border.right, border.bottom, border.left] {
    let side = side.expect("the line runs on every side");
    assert_eq!(
      (side.width, side.color, side.placement),
      (2.0, RING, BorderPlacement::Inside)
    );
  }
  assert_eq!(ring.border_radius, Some(BorderRadius::all(11.0)));
}

#[test]
fn a_square_corner_stays_square() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new()
      .padding(20.0)
      .child(Rect::new(100.0, 40.0).outline(Outline::new(2.0, RING).offset(2.0))),
  );

  let rings = rings(&tree, RING);
  assert_eq!(rings.len(), 1);
  assert!(
    rings[0]
      .border_radius
      .is_none_or(|radius| radius == BorderRadius::all(0.0))
  );
}

#[test]
fn an_outline_takes_no_room_in_layout() {
  let page = |outlined: bool| {
    let first = Rect::new(100.0, 40.0).id("first");
    let first = if outlined {
      first.outline(Outline::new(4.0, RING).offset(4.0))
    } else {
      first
    };
    Column::new()
      .id("page")
      .padding(20.0)
      .child(first)
      .child(Rect::new(100.0, 40.0).id("second"))
  };
  let mut app = App::new();
  let mut plain = painted(&mut app, page(false));
  let mut outlined = painted(&mut app, page(true));

  for id in ["page", "first", "second"] {
    assert_eq!(bounds(&mut outlined, id), bounds(&mut plain, id), "{id}");
  }
}

#[test]
fn an_outline_is_painted_over_the_content_of_its_element() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new()
      .id("card")
      .outline(Outline::new(2.0, RING))
      .child(Rect::new(100.0, 40.0).background(OTHER)),
  );

  let quads = quads(&tree);
  let fill = quads
    .iter()
    .position(|quad| matches!(&quad.content, QuadContent::Rect { color, .. } if *color == OTHER))
    .expect("the child is painted");
  let ring = quads
    .iter()
    .position(|quad| quad.border.and_then(|border| border.top).map(|side| side.color) == Some(RING))
    .expect("the outline is painted");
  assert!(ring > fill);
}

#[test]
fn a_focus_visible_outline_shows_for_keyboard_focus_only() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new().padding(20.0).child(
      Button::new("Save")
        .id("save")
        .size(120.0, 32.0)
        .tab_index(0)
        .focus_visible(|style| style.outline(Outline::new(2.0, RING).offset(2.0))),
    ),
  );
  assert!(rings(&tree, RING).is_empty());

  pointer_click(&mut tree, 60.0, 36.0, MouseButton::Left);
  tree.pass(&mut app, &TestSurface);
  assert!(rings(&tree, RING).is_empty(), "a click focuses without a ring");

  tab(&mut tree);
  tab(&mut tree);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(rings(&tree, RING).len(), 1, "the keyboard shows the ring");
}

#[test]
fn a_state_style_replaces_or_removes_the_outline() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new()
      .padding(20.0)
      .child(
        Rect::new(100.0, 40.0)
          .outline(Outline::new(2.0, RING))
          .hovered(|style| style.outline(Outline::new(2.0, OTHER))),
      )
      .child(
        Rect::new(100.0, 40.0)
          .outline(Outline::new(2.0, RING))
          .hovered(|style| style.outline(Outline::none())),
      ),
  );
  assert_eq!(rings(&tree, RING).len(), 2);

  tree.mouse_move(60.0, 40.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!((rings(&tree, RING).len(), rings(&tree, OTHER).len()), (1, 1));

  tree.mouse_move(60.0, 80.0);
  tree.pass(&mut app, &TestSurface);
  assert_eq!((rings(&tree, RING).len(), rings(&tree, OTHER).len()), (1, 0));
}

#[test]
fn width_and_color_roles_follow_the_theme() {
  let mut app = App::new();
  app.theme().set_palette_color(PaletteColor::Accent, RING);
  app.theme().set_border_size_value(BorderSize::Md, 3.0);
  let tree = painted(
    &mut app,
    Column::new()
      .padding(20.0)
      .child(Rect::new(100.0, 40.0).outline(Outline::new(BorderSize::Md, PaletteColor::Accent))),
  );

  let rings = rings(&tree, RING);
  assert_eq!(rings.len(), 1);
  assert_eq!(rings[0].border.unwrap().top.unwrap().width, 3.0);
  assert_eq!((rings[0].x, rings[0].width), (17.0, 106.0));
}

#[test]
fn an_outline_without_width_paints_nothing() {
  let mut app = App::new();
  let tree = painted(
    &mut app,
    Column::new()
      .padding(20.0)
      .child(Rect::new(100.0, 40.0).outline(Outline::new(0.0, RING).offset(4.0))),
  );

  assert!(rings(&tree, RING).is_empty());
}

#[test]
fn an_outline_that_shows_keeps_an_element_whose_box_is_clipped_away() {
  let mut app = App::new();
  let mut tree = painted(
    &mut app,
    Column::new()
      .id("clip")
      .size(100.0, 100.0)
      .clip()
      .child(Rect::new(100.0, 100.0).flex_shrink(0.0))
      .child(
        Rect::new(100.0, 20.0)
          .id("below")
          .flex_shrink(0.0)
          .outline(Outline::new(2.0, RING).offset(2.0)),
      ),
  );

  assert_eq!(
    bounds(&mut tree, "below").1,
    100.0,
    "the element starts where the clip ends"
  );
  let rings = rings(&tree, RING);
  assert_eq!(rings.len(), 1, "its ring reaches back into the clip and is painted");
  assert!(rings[0].clip.active);
}
