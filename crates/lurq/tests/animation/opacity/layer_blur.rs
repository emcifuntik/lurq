//! A node with a blur paints its subtree into an offscreen layer
//! (`RenderList::layers`) that is blurred as it is composited, like CSS
//! `filter: blur()`. The layer shows as far as the blur reaches past the
//! node's content, and is painted for more than it shows, so content just
//! outside a clip or the window bleeds in. The pixels are checked through
//! both render engines in `src/app/layer_blur_readback_tests.rs`.

use lurq::{
  animation::Transition,
  app::Tree,
  components::{Column, Rect, Stack},
  layout::opacity_layer::{LayerBounds, LayerCmd, MAX_BLUR_RADIUS, RoundedBox},
  node::{color::Color, transform::Transform2D},
};

use crate::support::{RenderSnapshot, render_pass};

const BACKGROUND: Color = Color::new(0x1c, 0x1c, 0x1c, 255);
const FILL: Color = Color::new(0xe4, 0xe4, 0xe4, 255);

fn render(root: impl Into<lurq::node::Element>, scale: f32) -> RenderSnapshot {
  let mut tree = Tree::new();
  tree.set_scale_factor(scale);
  tree.resize((300.0 * scale) as u32, (200.0 * scale) as u32);
  tree.set_root(root);
  render_pass(&mut tree)
}

/// A 100 x 36 box, `inset` logical pixels into a 300 x 200 window.
fn window(inset: f32, content: impl Into<lurq::node::Element>) -> Column {
  Column::new()
    .size(300.0, 200.0)
    .background(BACKGROUND)
    .padding(inset)
    .child(content)
}

fn right(bounds: LayerBounds) -> u32 {
  bounds.x + bounds.width
}

fn bottom(bounds: LayerBounds) -> u32 {
  bounds.y + bounds.height
}

/// The pixels the layer is painted for hold the pixels it shows.
fn assert_painted_covers_bounds(layer: &LayerCmd) {
  let ([x, y], [width, height]) = layer.painted();
  let bounds = layer.bounds;
  assert!(
    x <= bounds.x as i32
      && y <= bounds.y as i32
      && x + width as i32 >= right(bounds) as i32
      && y + height as i32 >= bottom(bounds) as i32,
    "{layer:?}"
  );
}

#[test]
fn a_blurred_box_is_a_layer_that_shows_past_the_box() {
  let snapshot = render(window(40.0, Rect::new(100.0, 36.0).background(FILL).blur(8.0)), 1.0);

  assert_eq!(snapshot.layers.len(), 1, "{:?}", snapshot.layers);
  let layer = snapshot.layers[0];
  assert_eq!(layer.opacity, 1.0);
  let blur = layer.blur.expect("a blurred layer");
  // A radius of 8 is a deviation of 4, which reaches 12 pixels; the box
  // itself has 2 pixels of anti-aliasing room.
  assert_eq!(blur.sigma, 4.0);
  assert_eq!(
    layer.bounds,
    LayerBounds {
      x: 40 - 14,
      y: 40 - 14,
      width: 100 + 28,
      height: 36 + 28
    }
  );
  assert_painted_covers_bounds(&layer);
  // The box paints unblurred, at full opacity, into the layer.
  let fill = snapshot.rects.iter().find(|rect| rect.color == FILL).unwrap();
  assert!((layer.start_order..layer.end_order).contains(&fill.order));
}

#[test]
fn the_blur_is_in_logical_pixels() {
  let snapshot = render(window(40.0, Rect::new(100.0, 36.0).background(FILL).blur(8.0)), 2.0);
  let layer = snapshot.layers[0];
  assert_eq!(layer.blur.unwrap().sigma, 8.0);
  assert_eq!((layer.bounds.x, layer.bounds.width), (80 - 26, 200 + 52));
}

#[test]
fn no_blur_is_no_layer_and_the_radius_is_clamped() {
  for radius in [0.0, -4.0, f32::NAN] {
    let snapshot = render(window(40.0, Rect::new(100.0, 36.0).background(FILL).blur(radius)), 1.0);
    assert!(snapshot.layers.is_empty(), "blur({radius}): {:?}", snapshot.layers);
  }
  let snapshot = render(window(40.0, Rect::new(100.0, 36.0).background(FILL).blur(1.0e9)), 1.0);
  assert_eq!(snapshot.layers[0].blur.unwrap().sigma, MAX_BLUR_RADIUS * 0.5);
}

#[test]
fn a_blur_and_an_opacity_share_one_layer() {
  // On its own the faded box would fold its opacity into its one quad.
  let snapshot = render(
    window(40.0, Rect::new(100.0, 36.0).background(FILL).opacity(0.5).blur(8.0)),
    1.0,
  );
  assert_eq!(snapshot.layers.len(), 1, "{:?}", snapshot.layers);
  assert_eq!(snapshot.layers[0].opacity, 0.5);
  assert!(snapshot.layers[0].blur.is_some());
  let fill = snapshot.rects.iter().find(|rect| rect.color.r() == FILL.r()).unwrap();
  assert_eq!(fill.color.a(), 255, "the box is unfaded inside its layer");
}

#[test]
fn a_blur_stays_inside_the_clip_its_node_is_under() {
  // The box runs past its clipping parent on the right and at the bottom.
  let snapshot = render(
    window(
      40.0,
      Stack::new()
        .size(60.0, 30.0)
        .clip()
        .child(Rect::new(100.0, 36.0).background(FILL).blur(8.0)),
    ),
    1.0,
  );
  let layer = snapshot.layers[0];
  // Left and top the blur would spread past the parent; it is cut there
  // too, as the box's own pixels would be.
  assert_eq!(
    layer.bounds,
    LayerBounds {
      x: 40,
      y: 40,
      width: 60,
      height: 30
    }
  );
  // The box is painted past the clip, by the blur's reach: what lies just
  // outside bleeds into what shows.
  let ([x, y], [width, height]) = layer.painted();
  assert!(x < 40 && y < 40, "{layer:?}");
  assert!(x + width as i32 >= 100 + 12 && y + height as i32 >= 70 + 6, "{layer:?}");
  let fill = snapshot.rects.iter().find(|rect| rect.color == FILL).unwrap();
  assert!(fill.clip.active);
  assert_eq!(
    (fill.clip.x, fill.clip.y, fill.clip.width, fill.clip.height),
    (40.0 - 12.0, 40.0 - 12.0, 60.0 + 24.0, 30.0 + 24.0)
  );
}

#[test]
fn a_rounded_clip_cuts_the_blur_with_its_corners() {
  let snapshot = render(
    window(
      40.0,
      Stack::new()
        .size(60.0, 30.0)
        .rounded(10.0)
        .clip()
        .child(Rect::new(100.0, 36.0).background(FILL).blur(8.0)),
    ),
    1.5,
  );
  let layer = snapshot.layers[0];
  assert_eq!((layer.bounds.x, layer.bounds.width), (60, 90));
  assert_eq!(
    layer.blur.unwrap().clip,
    Some(RoundedBox::rect(60.0, 60.0, 90.0, 45.0, [15.0; 4]))
  );
}

#[test]
fn the_blur_scales_with_the_transforms_its_node_is_under() {
  // The node's own transform, and an ancestor's: the radius is in the
  // node's units, so on screen the blur is as many times wider.
  let own = render(
    window(
      40.0,
      Rect::new(100.0, 36.0)
        .background(FILL)
        .blur(8.0)
        .transform(Transform2D::scale_uniform(2.0)),
    ),
    1.0,
  );
  assert_eq!(own.layers[0].blur.unwrap().sigma, 8.0);

  let inherited = render(
    window(
      40.0,
      Stack::new()
        .size(100.0, 36.0)
        .transform(Transform2D::scale(3.0, 0.75))
        .child(Rect::new(100.0, 36.0).background(FILL).blur(8.0)),
    ),
    1.0,
  );
  assert_eq!(
    inherited.layers[0].blur.unwrap().sigma,
    6.0,
    "sqrt(3 * 0.75) = 1.5 times"
  );

  // A turn changes no length.
  let turned = render(
    window(
      40.0,
      Rect::new(100.0, 36.0)
        .background(FILL)
        .blur(8.0)
        .transform(Transform2D::rotate_deg(30.0)),
    ),
    1.0,
  );
  assert!((turned.layers[0].blur.unwrap().sigma - 4.0).abs() < 1e-4);
}

#[test]
fn a_blurred_window_is_painted_past_its_edges() {
  // Content wider and taller than the window, blurred as a whole.
  let snapshot = render(
    Stack::new()
      .size(300.0, 200.0)
      .child(Rect::new(500.0, 400.0).background(FILL).blur(8.0)),
    1.0,
  );
  let layer = snapshot.layers[0];
  assert_eq!(
    layer.bounds,
    LayerBounds {
      x: 0,
      y: 0,
      width: 300,
      height: 200
    }
  );
  let ([x, y], [width, height]) = layer.painted();
  assert!(x <= -12 && y <= -12, "{layer:?}");
  assert!(x + width as i32 >= 312 && y + height as i32 >= 212, "{layer:?}");
}

#[test]
fn a_layer_around_a_blurred_one_covers_its_spread() {
  let snapshot = render(
    window(
      40.0,
      Stack::new()
        .size(100.0, 36.0)
        .opacity(0.5)
        .child(Rect::new(100.0, 36.0).background(FILL).blur(8.0))
        .child(Rect::new(10.0, 10.0).background(BACKGROUND)),
    ),
    1.0,
  );
  assert_eq!(snapshot.layers.len(), 2, "{:?}", snapshot.layers);
  // Paint order: the faded layer, then the blurred one inside it.
  let (outer, inner) = (snapshot.layers[0], snapshot.layers[1]);
  assert!(outer.blur.is_none() && inner.blur.is_some());
  assert!(
    outer.bounds.x <= inner.bounds.x
      && outer.bounds.y <= inner.bounds.y
      && right(outer.bounds) >= right(inner.bounds)
      && bottom(outer.bounds) >= bottom(inner.bounds),
    "{outer:?} does not cover {inner:?}"
  );
}

#[test]
fn a_blur_can_be_a_transition() {
  // The node lays out and paints as any blurred node with a transition set.
  let snapshot = render(
    window(
      40.0,
      Rect::new(100.0, 36.0)
        .background(FILL)
        .blur(4.0)
        .transition(Transition::blur().duration_ms(200)),
    ),
    1.0,
  );
  assert_eq!(snapshot.layers[0].blur.unwrap().sigma, 2.0);
}
