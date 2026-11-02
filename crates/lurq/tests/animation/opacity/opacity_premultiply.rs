use lurq::{
  app::Tree,
  layout::{Constraints, Size},
  node::color::Color,
};

use crate::support::render_pass;

#[test]
fn half_opacity_halves_alpha_in_rendered_rect() {
  let mut rt = Tree::new();
  let node = lurq::components::Rect::new(100.0, 50.0)
    .background(Color::new(255, 0, 0, 200))
    .opacity(0.5);
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  let snap = render_pass(&mut rt);

  assert!(!snap.rects.is_empty());
  assert_eq!(snap.rects[0].color.r(), 255);
  assert_eq!(snap.rects[0].color.a(), 100);
}

#[test]
fn full_opacity_preserves_alpha() {
  let mut rt = Tree::new();
  let node = lurq::components::Rect::new(100.0, 50.0)
    .background(Color::new(255, 0, 0, 200))
    .opacity(1.0);
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  let snap = render_pass(&mut rt);

  assert_eq!(snap.rects[0].color.a(), 200);
}

#[test]
fn zero_opacity_zeros_alpha() {
  let mut rt = Tree::new();
  let node = lurq::components::Rect::new(100.0, 50.0)
    .background(Color::new(255, 0, 0, 200))
    .opacity(0.0);
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  let snap = render_pass(&mut rt);

  assert_eq!(snap.rects[0].color.a(), 0);
}

#[test]
fn faded_container_fades_its_text_glyphs() {
  // Rects premultiplied opacity into their color; glyphs did not, so text
  // stayed fully opaque inside a faded subtree.
  let mut rt = Tree::new();
  let node = lurq::components::Column::new()
    .opacity(0.5)
    .child(lurq::components::Text::new("Hi").color(Color::new(255, 255, 255, 255)));
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  let snap = render_pass(&mut rt);

  assert!(!snap.glyphs.is_empty());
  for glyph in &snap.glyphs {
    assert!(
      (glyph.color[3] - 0.5).abs() < 0.01,
      "glyph alpha should be halved, got {}",
      glyph.color[3]
    );
  }
}

#[cfg(feature = "image")]
#[test]
fn faded_container_fades_its_images() {
  // Images ignored quad opacity entirely — both backends' shaders multiply an
  // instance opacity, but the CPU side hardcoded it to 1.0.
  let mut rt = Tree::new();
  let img = lurq::images::ImageData::from_rgba(vec![255; 4 * 4 * 4], 4, 4);
  let node = lurq::components::Column::new()
    .opacity(0.5)
    .child(lurq::components::Image::new(img));
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  let snap = render_pass(&mut rt);

  assert_eq!(snap.image_opacities, vec![0.5]);
}

#[test]
fn opacity_does_not_change_rgb_channels() {
  let mut rt = Tree::new();
  let node = lurq::components::Rect::new(100.0, 50.0)
    .background(Color::new(100, 150, 200, 255))
    .opacity(0.5);
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  let snap = render_pass(&mut rt);

  assert_eq!(snap.rects[0].color.r(), 100);
  assert_eq!(snap.rects[0].color.g(), 150);
  assert_eq!(snap.rects[0].color.b(), 200);
  assert_eq!(snap.rects[0].color.a(), 128);
}
