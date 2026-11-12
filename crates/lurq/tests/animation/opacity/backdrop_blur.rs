//! A node with a backdrop blur blurs what is painted behind its box, like
//! CSS `backdrop-filter: blur()`, before it paints itself
//! (`RenderList::backdrops`). The pixels are checked through both render
//! engines in `src/app/backdrop_blur_readback_tests.rs`.

use lurq::{
  animation::Transition,
  app::Tree,
  components::{Column, Rect, Stack},
  layout::opacity_layer::MAX_BLUR_RADIUS,
  node::color::Color,
};

use crate::support::{RenderSnapshot, render_pass};

const BACKGROUND: Color = Color::new(0x1c, 0x1c, 0x1c, 255);
const GLASS: Color = Color::new(0xff, 0xff, 0xff, 0x40);

fn render(root: impl Into<lurq::node::Element>, scale: f32) -> RenderSnapshot {
  let mut tree = Tree::new();
  tree.set_scale_factor(scale);
  tree.resize((300.0 * scale) as u32, (200.0 * scale) as u32);
  tree.set_root(root);
  render_pass(&mut tree)
}

/// `content`, `inset` logical pixels into a 300 x 200 window.
fn window(inset: f32, content: impl Into<lurq::node::Element>) -> Column {
  Column::new()
    .size(300.0, 200.0)
    .background(BACKGROUND)
    .padding(inset)
    .child(content)
}

#[test]
fn a_backdrop_blur_fills_the_box_before_the_node_paints() {
  let snapshot = render(
    window(
      40.0,
      Rect::new(100.0, 36.0)
        .background(GLASS)
        .rounded(8.0)
        .backdrop_blur(12.0),
    ),
    1.0,
  );

  assert_eq!(snapshot.backdrops.len(), 1, "{:?}", snapshot.backdrops);
  let backdrop = snapshot.backdrops[0];
  assert_eq!(
    (backdrop.x, backdrop.y, backdrop.width, backdrop.height),
    (40.0, 40.0, 100.0, 36.0)
  );
  assert_eq!(backdrop.radii, [8.0; 4]);
  // A radius of 12 is a deviation of 6.
  assert_eq!((backdrop.sigma, backdrop.opacity), (6.0, 1.0));
  // After the window's background, before the node's own fill; nothing else
  // has its order.
  let background = snapshot.rects.iter().find(|rect| rect.color == BACKGROUND).unwrap();
  let glass = snapshot.rects.iter().find(|rect| rect.color == GLASS).unwrap();
  assert!(background.order < backdrop.order && backdrop.order < glass.order);
  assert!(snapshot.rects.iter().all(|rect| rect.order != backdrop.order));
  // A backdrop blur alone needs no layer.
  assert!(snapshot.layers.is_empty(), "{:?}", snapshot.layers);
}

#[test]
fn the_backdrop_blur_is_in_logical_pixels_and_clamped() {
  let glass = |radius: f32| {
    Rect::new(100.0, 36.0)
      .background(GLASS)
      .rounded(8.0)
      .backdrop_blur(radius)
  };
  let snapshot = render(window(40.0, glass(12.0)), 2.0);
  let backdrop = snapshot.backdrops[0];
  assert_eq!((backdrop.x, backdrop.width, backdrop.sigma), (80.0, 200.0, 12.0));
  assert_eq!(backdrop.radii, [16.0; 4]);

  for radius in [0.0, -4.0, f32::NAN] {
    let snapshot = render(window(40.0, glass(radius)), 1.0);
    assert!(snapshot.backdrops.is_empty(), "backdrop_blur({radius})");
  }
  let snapshot = render(window(40.0, glass(1.0e9)), 1.0);
  assert_eq!(snapshot.backdrops[0].sigma, MAX_BLUR_RADIUS * 0.5);
}

#[test]
fn a_faded_node_fades_its_backdrop_blur_outside_its_own_layer() {
  // The fill and the border make the faded node a layer.
  let snapshot = render(
    window(
      40.0,
      Rect::new(100.0, 36.0)
        .background(GLASS)
        .border_inside(1.0, Color::new(255, 255, 255, 255))
        .opacity(0.5)
        .backdrop_blur(12.0),
    ),
    1.0,
  );
  assert_eq!(snapshot.layers.len(), 1, "{:?}", snapshot.layers);
  let (layer, backdrop) = (snapshot.layers[0], snapshot.backdrops[0]);
  assert_eq!((layer.opacity, backdrop.opacity), (0.5, 0.5));
  // The blur reads what is behind the node, so it runs in the target the
  // node's layer is composited into, right before that layer.
  assert_eq!(backdrop.order + 1, layer.start_order);
}

#[test]
fn a_backdrop_blur_inside_a_faded_parent_is_inside_its_layer() {
  let snapshot = render(
    window(
      40.0,
      Stack::new()
        .size(120.0, 60.0)
        .opacity(0.5)
        .child(Rect::new(120.0, 60.0).background(BACKGROUND))
        .child(Rect::new(100.0, 36.0).background(GLASS).backdrop_blur(12.0)),
    ),
    1.0,
  );
  assert_eq!(snapshot.layers.len(), 1, "{:?}", snapshot.layers);
  let (layer, backdrop) = (snapshot.layers[0], snapshot.backdrops[0]);
  assert!((layer.start_order..layer.end_order).contains(&backdrop.order));
  // It fades with the layer it is in, not on its own.
  assert_eq!(backdrop.opacity, 1.0);
}

#[test]
fn a_backdrop_blur_carries_the_clip_its_node_is_under() {
  let snapshot = render(
    window(
      40.0,
      Stack::new()
        .size(60.0, 30.0)
        .clip()
        .child(Rect::new(100.0, 36.0).background(GLASS).backdrop_blur(12.0)),
    ),
    1.0,
  );
  let backdrop = snapshot.backdrops[0];
  assert_eq!(backdrop.width, 100.0);
  assert!(backdrop.clip.active);
  assert_eq!(
    (
      backdrop.clip.x,
      backdrop.clip.y,
      backdrop.clip.width,
      backdrop.clip.height
    ),
    (40.0, 40.0, 60.0, 30.0)
  );
}

#[test]
fn a_backdrop_blur_can_be_a_transition() {
  let snapshot = render(
    window(
      40.0,
      Rect::new(100.0, 36.0)
        .background(GLASS)
        .backdrop_blur(4.0)
        .transition(Transition::backdrop_blur().duration_ms(200)),
    ),
    1.0,
  );
  assert_eq!(snapshot.backdrops[0].sigma, 2.0);
}
