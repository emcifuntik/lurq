use lurq::{
  app::Tree,
  components::Rect,
  layout::{Constraints, Size, render_list::StrokeStyle},
  node::{BackgroundColor, border::Border, color::Color},
};

use super::PassLayoutExt;
use crate::support::render_pass;

#[test]
fn bottom_only_border_renders_as_single_bottom_rect() {
  let border_color = Color::from_hex("#ff0000");
  let mut tree = Tree::new();
  tree.set_root(Rect::new(100.0, 40.0).border_bottom(Border::inside(1.0, BackgroundColor::Color(border_color))));

  let snapshot = render_pass(&mut tree);
  let border_rects = snapshot
    .rects
    .iter()
    .filter(|rect| rect.color == border_color)
    .collect::<Vec<_>>();

  assert_eq!(border_rects.len(), 1, "bottom-only border should emit one filled rect");
  let rect = border_rects[0];
  assert_eq!(rect.x, 0.0);
  assert_eq!(rect.y, 39.0);
  assert_eq!(rect.width, 100.0);
  assert_eq!(rect.height, 1.0);
  assert_eq!(rect.stroke, [0.0; 4]);
}

/// The strokes painted in `color`: the rects a border that is not a filled band becomes.
fn strokes(tree: &mut Tree, color: Color) -> Vec<crate::support::RectSnapshot> {
  render_pass(tree)
    .rects
    .into_iter()
    .filter(|rect| rect.stroke_color == color && rect.stroke.iter().any(|width| *width > 0.0))
    .collect()
}

#[test]
fn a_border_on_one_side_of_a_rounded_box_is_a_stroke_that_follows_its_corners() {
  let border_color = Color::from_hex("#ff0000");
  let mut tree = Tree::new();
  tree.set_root(
    Rect::new(100.0, 40.0)
      .rounded(8.0)
      .border_bottom(Border::inside(1.0, BackgroundColor::Color(border_color))),
  );

  let strokes = strokes(&mut tree, border_color);
  assert_eq!(strokes.len(), 1, "one stroke over the whole box");
  let stroke = strokes[0];
  assert_eq!(
    (stroke.x, stroke.y, stroke.width, stroke.height),
    (0.0, 0.0, 100.0, 40.0)
  );
  assert_eq!(stroke.stroke, [0.0, 0.0, 1.0, 0.0]);
  assert_eq!(stroke.radii, [8.0; 4], "the box's corners, which the stroke follows");
  assert_eq!(stroke.stroke_style, StrokeStyle::default());
}

#[test]
fn a_border_on_a_side_between_square_corners_stays_a_filled_band() {
  let border_color = Color::from_hex("#ff0000");
  let mut tree = Tree::new();
  // Round at the top only: the bottom side ends in square corners.
  tree.set_root(
    Rect::new(100.0, 40.0)
      .corner_radius_top_left(8.0)
      .corner_radius_top_right(8.0)
      .border_bottom(Border::inside(1.0, BackgroundColor::Color(border_color))),
  );

  assert!(strokes(&mut tree, border_color).is_empty());
  let snapshot = render_pass(&mut tree);
  let band = snapshot
    .rects
    .iter()
    .find(|rect| rect.color == border_color)
    .expect("the band");
  assert_eq!((band.x, band.y, band.width, band.height), (0.0, 39.0, 100.0, 1.0));
}

#[test]
fn a_border_outside_a_rounded_box_keeps_the_boxs_corners_for_its_hole() {
  let border_color = Color::from_hex("#ff0000");
  let mut tree = Tree::new();
  tree.set_root(
    lurq::components::Column::new().padding(10.0).child(
      Rect::new(100.0, 40.0)
        .rounded(8.0)
        .border(Border::outside(2.0, BackgroundColor::Color(border_color))),
    ),
  );

  let stroke = strokes(&mut tree, border_color)[0];
  assert_eq!(
    (stroke.x, stroke.y, stroke.width, stroke.height),
    (8.0, 8.0, 104.0, 44.0)
  );
  assert_eq!(stroke.radii, [8.0; 4]);
  assert_eq!(
    stroke.stroke_style.radii_at, 1.0,
    "the radii are the hole's: the outer edge is rounder"
  );

  let mut centered = Tree::new();
  centered.set_root(
    lurq::components::Column::new().padding(10.0).child(
      Rect::new(100.0, 40.0)
        .rounded(8.0)
        .border(Border::center(2.0, BackgroundColor::Color(border_color))),
    ),
  );
  assert_eq!(strokes(&mut centered, border_color)[0].stroke_style.radii_at, 0.5);
}

#[test]
fn a_dashed_border_is_a_stroke_with_its_dashes() {
  let border_color = Color::from_hex("#ff0000");
  let dashes = |border: Border| {
    let mut tree = Tree::new();
    tree.set_root(Rect::new(100.0, 40.0).border(border));
    let strokes = strokes(&mut tree, border_color);
    assert_eq!(strokes.len(), 1);
    strokes[0].stroke_style.dash
  };
  let solid = Border::inside(2.0, BackgroundColor::Color(border_color));

  assert_eq!(dashes(solid.clone()), [0.0; 2]);
  assert_eq!(
    dashes(solid.clone().dashed()),
    [6.0; 2],
    "three widths long, with gaps as long"
  );
  assert_eq!(dashes(solid.clone().dash(5.0, 3.0)), [5.0, 3.0]);
  assert_eq!(dashes(solid.dash(5.0, 0.0)), [0.0; 2], "no gap: a solid line");
}

#[test]
fn a_dashed_border_on_one_side_is_a_stroke_on_a_square_box_too() {
  let border_color = Color::from_hex("#ff0000");
  let mut tree = Tree::new();
  tree
    .set_root(Rect::new(100.0, 40.0).border_bottom(Border::inside(1.0, BackgroundColor::Color(border_color)).dashed()));

  let strokes = strokes(&mut tree, border_color);
  assert_eq!(strokes.len(), 1);
  assert_eq!(strokes[0].stroke, [0.0, 0.0, 1.0, 0.0]);
  assert_eq!(strokes[0].stroke_style.dash, [3.0; 2]);
}

#[test]
fn sides_that_differ_only_in_their_dashes_are_painted_apart() {
  let border_color = Color::from_hex("#ff0000");
  let solid = Border::inside(1.0, BackgroundColor::Color(border_color));
  let mut tree = Tree::new();
  tree.set_root(
    Rect::new(100.0, 40.0)
      .rounded(6.0)
      .border_top(solid.clone())
      .border_bottom(solid.dashed()),
  );

  let strokes = strokes(&mut tree, border_color);
  assert_eq!(strokes.len(), 2);
  assert!(
    strokes
      .iter()
      .any(|stroke| stroke.stroke == [1.0, 0.0, 0.0, 0.0] && stroke.stroke_style.dash == [0.0; 2])
  );
  assert!(
    strokes
      .iter()
      .any(|stroke| stroke.stroke == [0.0, 0.0, 1.0, 0.0] && stroke.stroke_style.dash == [3.0; 2])
  );
}

/// A pill-shaped clipping container (the common `radius: 999` idiom) must
/// clamp its rounded-clip radii to the box, or the clip has no interior and
/// every child pixel is discarded by the shader-side rounded clip
/// (regression: chip/pill labels rendering invisible).
#[test]
fn pill_radius_clip_keeps_a_usable_interior_for_children() {
  let mut rt = Tree::new();
  let node = lurq::components::Row::new()
    .align_items(lurq::layout::Alignment::Center)
    .padding_horizontal(9.0)
    .padding_vertical(5.0)
    .rounded(999.0)
    .background("#16302A")
    .clip()
    .child(lurq::components::Text::new("auto_attack"));
  rt.set_root(node);
  let result = rt.pass_layout(Constraints::loose(Size::new(400.0, 300.0))).unwrap();
  let quads = rt.resolve_quads(&result);
  let text_clip = quads
    .iter()
    .find_map(|quad| match &quad.content {
      lurq::layout::quad::QuadContent::Text { text, .. } if text == "auto_attack" => Some(quad.clip),
      _ => None,
    })
    .expect("the label text quad should be emitted");

  assert!(text_clip.active, "the pill must clip its children");
  let radius = text_clip.border_radius.expect("the pill clip should stay rounded");
  let max_radius = (text_clip.width.min(text_clip.height)) / 2.0 + 0.001;
  assert!(
    radius.top_left <= max_radius && radius.bottom_right <= max_radius,
    "clip radii must be clamped to the box (radius {} in {}x{})",
    radius.top_left,
    text_clip.width,
    text_clip.height,
  );
  assert!(radius.top_left > 0.0, "clamping must keep the pill rounded");
}
