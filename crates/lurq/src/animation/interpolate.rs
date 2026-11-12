use crate::node::{
  color::Color,
  transform::{Decomposed, Transform2D, decompose},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimatableProperty {
  BackgroundColor,
  BorderColor,
  BorderWidthTop,
  BorderWidthRight,
  BorderWidthBottom,
  BorderWidthLeft,
  BorderRadiusTopLeft,
  BorderRadiusTopRight,
  BorderRadiusBottomRight,
  BorderRadiusBottomLeft,
  OffsetX,
  OffsetY,
  Width,
  Height,
  Opacity,
  /// The node's blur radius (`Node::blur`), in logical pixels.
  Blur,
  /// The blur radius of the node's backdrop (`Node::backdrop_blur`), in
  /// logical pixels.
  BackdropBlur,
  Transform,
}

macro_rules! property_accessors {
  ($target:ident { $($variant:ident => $method:ident),* $(,)? }) => {
    impl $target {
      $(
        pub fn $method() -> Self {
          Self::single(AnimatableProperty::$variant)
        }
      )*
    }
  };
}

pub(crate) use property_accessors;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimatableValue {
  Color(Color),
  Float(f32),
  Transform(Decomposed),
}

impl AnimatableValue {
  pub fn lerp(&self, to: &AnimatableValue, t: f32) -> AnimatableValue {
    match (self, to) {
      (AnimatableValue::Color(a), AnimatableValue::Color(b)) => AnimatableValue::Color(lerp_color(*a, *b, t)),
      (AnimatableValue::Float(a), AnimatableValue::Float(b)) => AnimatableValue::Float(a + (b - a) * t),
      (AnimatableValue::Transform(a), AnimatableValue::Transform(b)) => AnimatableValue::Transform(a.lerp(b, t)),
      _ => {
        if t >= 0.5 {
          *to
        } else {
          *self
        }
      }
    }
  }
}

impl From<Color> for AnimatableValue {
  fn from(c: Color) -> Self {
    Self::Color(c)
  }
}

impl From<f32> for AnimatableValue {
  fn from(v: f32) -> Self {
    Self::Float(v)
  }
}

impl From<Transform2D> for AnimatableValue {
  fn from(t: Transform2D) -> Self {
    Self::Transform(decompose(&t).unwrap_or(Decomposed {
      translate_x: 0.0,
      translate_y: 0.0,
      scale_x: 1.0,
      scale_y: 1.0,
      rotate: 0.0,
      skew_x: 0.0,
    }))
  }
}

impl From<Decomposed> for AnimatableValue {
  fn from(d: Decomposed) -> Self {
    Self::Transform(d)
  }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
  let ar = a.r() as f32;
  let ag = a.g() as f32;
  let ab = a.b() as f32;
  let aa = a.a() as f32;
  let br = b.r() as f32;
  let bg = b.g() as f32;
  let bb = b.b() as f32;
  let ba = b.a() as f32;
  Color::new(
    (ar + (br - ar) * t).round() as u8,
    (ag + (bg - ag) * t).round() as u8,
    (ab + (bb - ab) * t).round() as u8,
    (aa + (ba - aa) * t).round() as u8,
  )
}

pub(crate) fn read_target(node: &crate::node::Node, prop: AnimatableProperty) -> Option<AnimatableValue> {
  let style = node.target_style();
  match prop {
    AnimatableProperty::BackgroundColor => style
      .color
      .or_else(|| <Option<_> as Clone>::clone(&node.color))
      .and_then(|color| color.as_color())
      .map(AnimatableValue::Color),
    AnimatableProperty::BorderColor => style
      .border
      .or_else(|| node.border.as_deref().cloned())
      .and_then(|b| b.color())
      .and_then(|color| color.as_color())
      .map(AnimatableValue::Color),
    AnimatableProperty::BorderWidthTop => style
      .border
      .or_else(|| node.border.as_deref().cloned())
      .and_then(|b| b.top_width())
      .and_then(|width| width.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderWidthRight => style
      .border
      .or_else(|| node.border.as_deref().cloned())
      .and_then(|b| b.right_width())
      .and_then(|width| width.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderWidthBottom => style
      .border
      .or_else(|| node.border.as_deref().cloned())
      .and_then(|b| b.bottom_width())
      .and_then(|width| width.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderWidthLeft => style
      .border
      .or_else(|| node.border.as_deref().cloned())
      .and_then(|b| b.left_width())
      .and_then(|width| width.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderRadiusTopLeft => style
      .border_radius
      .or(*node.border_radius)
      .and_then(|r| r.top_left.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderRadiusTopRight => style
      .border_radius
      .or(*node.border_radius)
      .and_then(|r| r.top_right.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderRadiusBottomRight => style
      .border_radius
      .or(*node.border_radius)
      .and_then(|r| r.bottom_right.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::BorderRadiusBottomLeft => style
      .border_radius
      .or(*node.border_radius)
      .and_then(|r| r.bottom_left.as_px())
      .map(AnimatableValue::Float),
    AnimatableProperty::OffsetX => read_offset_x(node).map(AnimatableValue::Float),
    AnimatableProperty::OffsetY => read_offset_y(node).map(AnimatableValue::Float),
    AnimatableProperty::Width => read_target_frame_dim(node, &style, true).map(AnimatableValue::Float),
    AnimatableProperty::Height => read_target_frame_dim(node, &style, false).map(AnimatableValue::Float),
    AnimatableProperty::Opacity => Some(AnimatableValue::Float(node.opacity)),
    AnimatableProperty::Blur => Some(AnimatableValue::Float(node.blur)),
    AnimatableProperty::BackdropBlur => Some(AnimatableValue::Float(node.backdrop_blur)),
    AnimatableProperty::Transform => decompose(&node.transform).map(AnimatableValue::Transform),
  }
}

pub(crate) fn write_property(node: &mut crate::node::Node, prop: AnimatableProperty, value: &AnimatableValue) -> bool {
  let affects_layout = matches!(
    prop,
    AnimatableProperty::OffsetX | AnimatableProperty::OffsetY | AnimatableProperty::Width | AnimatableProperty::Height
  );
  // Every property goes to the overrides, which each tick clears, and never into the node's own fields: those
  // hold what the render set, which `read_target` must keep seeing.
  if let Some(pos) = node.animation_overrides.iter().position(|(p, _)| *p == prop) {
    node.animation_overrides[pos].1 = *value;
  } else {
    node.animation_overrides.push((prop, *value));
  }
  affects_layout
}

pub(crate) fn clear_overrides(node: &mut crate::node::Node) {
  if !node.prepare_layout_tracking().timeline {
    return;
  }
  #[cfg(test)]
  crate::core::layout_dirty::visit();
  // Writing through the lazy box would allocate it for a node holding none.
  if !node.animation_overrides.is_empty() {
    node.animation_overrides.clear();
  }
  for child in &mut node.children {
    clear_overrides(child);
  }
}

fn read_offset_x(node: &crate::node::Node) -> Option<f32> {
  node.offset.map(|offset| offset.x)
}

fn read_offset_y(node: &crate::node::Node) -> Option<f32> {
  node.offset.map(|offset| offset.y)
}

fn read_target_frame_dim(node: &crate::node::Node, style: &crate::node::style::Style, is_width: bool) -> Option<f32> {
  let base = node.frame;
  if base == crate::layout::layout_kind::FrameConstraints::default() && style.frame.is_none() {
    return None;
  }
  let effective = match style.frame {
    Some(overlay) => crate::node::node::merge_frame(base, overlay),
    None => base,
  };
  if is_width {
    effective.width.map(|d| d.to_px())
  } else {
    effective.height.map(|d| d.to_px())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lerp_color_midpoint() {
    let a = Color::new(0, 0, 0, 255);
    let b = Color::new(255, 255, 255, 255);
    let mid = lerp_color(a, b, 0.5);
    assert_eq!(mid.r(), 128);
    assert_eq!(mid.g(), 128);
    assert_eq!(mid.b(), 128);
  }

  #[test]
  fn lerp_float() {
    let a = AnimatableValue::Float(0.0);
    let b = AnimatableValue::Float(100.0);
    assert_eq!(a.lerp(&b, 0.75), AnimatableValue::Float(75.0));
  }

  #[test]
  fn lerp_mismatched_types_discrete() {
    let a = AnimatableValue::Float(10.0);
    let b = AnimatableValue::Color(Color::new(255, 0, 0, 255));
    assert_eq!(a.lerp(&b, 0.3), a);
    assert_eq!(a.lerp(&b, 0.7), b);
  }
}
