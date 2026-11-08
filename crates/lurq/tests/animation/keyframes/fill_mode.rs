use lurq::{
  animation::{AnimatableProperty, AnimatableValue, Animation, AnimationFillMode, Keyframes, KeyframesId},
  app::Tree,
  layout::{Constraints, Size},
};

use crate::support::run_pass;

#[test]
fn fill_forwards_holds_final_value_after_completion() {
  let mut rt = Tree::new();

  rt.register_keyframes(
    Keyframes::new(KeyframesId::new(5))
      .frame(0.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(1.0));
      })
      .frame(1.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(0.2));
      }),
  );

  let node = lurq::components::Rect::new(100.0, 50.0)
    .background("#ff0000")
    .animation(
      Animation::new(KeyframesId::new(5))
        .duration_ms(1)
        .linear()
        .fill_mode(AnimationFillMode::Forwards),
    );
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));

  std::thread::sleep(std::time::Duration::from_millis(20));
  run_pass(&mut rt);
  std::thread::sleep(std::time::Duration::from_millis(20));
  run_pass(&mut rt);

  let result = rt.last_layout().unwrap();
  let quads = rt.resolve_quads(result);
  assert!(
    (quads[0].opacity - 0.2).abs() < 0.05,
    "fill forwards should hold at 0.2, got {}",
    quads[0].opacity
  );
}

#[test]
fn fill_forwards_keeps_holding_on_later_frames() {
  let mut rt = Tree::new();

  rt.register_keyframes(
    Keyframes::new(KeyframesId::new(6))
      .frame(0.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(1.0));
      })
      .frame(1.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(0.2));
      }),
  );

  let node = lurq::components::Rect::new(100.0, 50.0)
    .background("#ff0000")
    .animation(
      Animation::new(KeyframesId::new(6))
        .duration_ms(100)
        .linear()
        .fill_mode(AnimationFillMode::Forwards),
    );
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));

  // The run ends on the second frame; the frames after it draw nothing new but must keep its last pose.
  let start = std::time::Instant::now();
  for ms in [0, 200, 400, 600] {
    rt.set_animation_clock(Some(start + std::time::Duration::from_millis(ms)));
    run_pass(&mut rt);
  }

  let result = rt.last_layout().unwrap();
  let quads = rt.resolve_quads(result);
  assert!(
    (quads[0].opacity - 0.2).abs() < 0.05,
    "fill forwards should keep holding at 0.2, got {}",
    quads[0].opacity
  );
}

#[test]
fn fill_none_does_not_hold_final_value() {
  let mut rt = Tree::new();

  rt.register_keyframes(
    Keyframes::new(KeyframesId::new(5))
      .frame(0.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(1.0));
      })
      .frame(1.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(0.0));
      }),
  );

  let node = lurq::components::Rect::new(100.0, 50.0)
    .background("#ff0000")
    .animation(
      Animation::new(KeyframesId::new(5))
        .duration_ms(1)
        .linear()
        .fill_mode(AnimationFillMode::None),
    );
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));

  std::thread::sleep(std::time::Duration::from_millis(20));
  run_pass(&mut rt);
  std::thread::sleep(std::time::Duration::from_millis(20));
  run_pass(&mut rt);

  let result = rt.last_layout().unwrap();
  let quads = rt.resolve_quads(result);
  assert_eq!(quads[0].opacity, 1.0, "fill none should revert to node's base opacity");
}

#[test]
fn fill_backwards_applies_first_frame_during_delay() {
  let mut rt = Tree::new();

  rt.register_keyframes(
    Keyframes::new(KeyframesId::new(16))
      .frame(0.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(0.3));
      })
      .frame(1.0, |f| {
        f.set(AnimatableProperty::Opacity, AnimatableValue::Float(1.0));
      }),
  );

  let node = lurq::components::Rect::new(100.0, 50.0)
    .background("#ff0000")
    .animation(
      Animation::new(KeyframesId::new(16))
        .duration_ms(10000)
        .delay_ms(10000)
        .linear()
        .fill_mode(AnimationFillMode::Backwards),
    );
  rt.set_root(node);

  rt.set_layout_constraints_override(Some(Constraints::loose(Size::new(400.0, 400.0))));
  run_pass(&mut rt);

  let result = rt.last_layout().unwrap();
  let quads = rt.resolve_quads(result);
  assert!(
    (quads[0].opacity - 0.3).abs() < 0.05,
    "fill backwards should apply first frame during delay, got {}",
    quads[0].opacity
  );
}
