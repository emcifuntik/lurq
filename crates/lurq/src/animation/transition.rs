use std::{
  collections::{HashMap, HashSet},
  time::{Duration, Instant},
};

use super::{
  animation::step_time,
  easing::Easing,
  interpolate::{
    AnimatableProperty, AnimatableValue, clear_overrides, property_accessors, read_target, write_property,
  },
};
use crate::core::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransitionProperty {
  All,
  Single(AnimatableProperty),
}

#[derive(Clone, Debug)]
pub struct Transition {
  pub property: TransitionProperty,
  pub duration: Duration,
  pub delay: Duration,
  pub easing: Easing,
}

impl Transition {
  fn single(prop: AnimatableProperty) -> Self {
    Self {
      property: TransitionProperty::Single(prop),
      duration: Duration::from_millis(300),
      delay: Duration::ZERO,
      easing: Easing::EASE,
    }
  }

  pub fn all() -> Self {
    Self {
      property: TransitionProperty::All,
      duration: Duration::from_millis(300),
      delay: Duration::ZERO,
      easing: Easing::EASE,
    }
  }

  pub fn duration(mut self, d: Duration) -> Self {
    self.duration = d;
    self
  }

  pub fn duration_ms(mut self, ms: u64) -> Self {
    self.duration = Duration::from_millis(ms);
    self
  }

  pub fn delay(mut self, d: Duration) -> Self {
    self.delay = d;
    self
  }

  pub fn delay_ms(mut self, ms: u64) -> Self {
    self.delay = Duration::from_millis(ms);
    self
  }

  pub fn easing(mut self, e: Easing) -> Self {
    self.easing = e;
    self
  }

  pub fn linear(mut self) -> Self {
    self.easing = Easing::Linear;
    self
  }
}

property_accessors!(Transition {
  BackgroundColor => background_color,
  BorderColor => border_color,
  BorderWidthTop => border_width_top,
  BorderWidthRight => border_width_right,
  BorderWidthBottom => border_width_bottom,
  BorderWidthLeft => border_width_left,
  BorderRadiusTopLeft => border_radius_top_left,
  BorderRadiusTopRight => border_radius_top_right,
  BorderRadiusBottomRight => border_radius_bottom_right,
  BorderRadiusBottomLeft => border_radius_bottom_left,
  OffsetX => offset_x,
  OffsetY => offset_y,
  Width => width,
  Height => height,
  Opacity => opacity,
  Transform => transform,
});

const ALL_PROPERTIES: &[AnimatableProperty] = &[
  AnimatableProperty::BackgroundColor,
  AnimatableProperty::BorderColor,
  AnimatableProperty::BorderWidthTop,
  AnimatableProperty::BorderWidthRight,
  AnimatableProperty::BorderWidthBottom,
  AnimatableProperty::BorderWidthLeft,
  AnimatableProperty::BorderRadiusTopLeft,
  AnimatableProperty::BorderRadiusTopRight,
  AnimatableProperty::BorderRadiusBottomRight,
  AnimatableProperty::BorderRadiusBottomLeft,
  AnimatableProperty::OffsetX,
  AnimatableProperty::OffsetY,
  AnimatableProperty::Width,
  AnimatableProperty::Height,
  AnimatableProperty::Opacity,
  AnimatableProperty::Transform,
];

struct TransitionRun {
  from: AnimatableValue,
  to: AnimatableValue,
  start_time: Instant,
  duration: Duration,
  delay: Duration,
  easing: Easing,
}

impl TransitionRun {
  /// How far the run is `elapsed` after its delay, from 0 at its start to 1 at its end. A run of no duration is at
  /// its end at once.
  fn progress(&self, elapsed: Duration) -> f64 {
    if self.duration.is_zero() {
      return 1.0;
    }
    elapsed.as_secs_f64() / self.duration.as_secs_f64()
  }

  /// When the run next draws something other than what it draws at `now`, which is `raw` of the way through it:
  /// the next step of a step easing or the end. A continuous easing changes at once, so it returns `now`.
  fn next_change(&self, now: Instant, raw: f64) -> Option<Instant> {
    match self.easing.distance_to_next_step(raw, true) {
      Some(until) => step_time(now, self.duration, until),
      None => Some(now),
    }
  }
}

pub struct TransitionEngine {
  active: HashMap<(NodeId, AnimatableProperty), TransitionRun>,
  prev_values: HashMap<(NodeId, AnimatableProperty), AnimatableValue>,
  seen: HashSet<NodeId>,
  next_change: Option<Instant>,
  pub has_active: bool,
}

impl TransitionEngine {
  pub fn new() -> Self {
    Self {
      active: HashMap::new(),
      prev_values: HashMap::new(),
      seen: HashSet::new(),
      next_change: None,
      has_active: false,
    }
  }

  pub(crate) fn clear_state(&mut self) {
    self.active.clear();
    self.prev_values.clear();
    self.seen.clear();
    self.next_change = None;
    self.has_active = false;
  }

  /// Starts a frame: forgets which transitioning nodes last frame's walks
  /// visited. Mirrors `AnimationEngine::begin_frame` — a frame ticks the base
  /// tree and each overlay subtree separately, so stale runs are only
  /// detectable after all walks in [`Self::finish_frame`].
  pub(crate) fn begin_frame(&mut self) {
    self.seen.clear();
    self.next_change = None;
  }

  /// Ends a frame after every tick walk ran: drops runs and prev-value
  /// tracking for nodes no walk visited. A node unmounting mid-transition
  /// otherwise leaves its run in `active` forever (only completed runs are
  /// removed in `process_node`), wedging `has_active` and with it the layout
  /// fast path.
  pub(crate) fn finish_frame(&mut self) {
    let seen = &self.seen;
    self.active.retain(|(node_id, _), _| seen.contains(node_id));
    self.prev_values.retain(|(node_id, _), _| seen.contains(node_id));
    self.has_active = !self.active.is_empty();
  }

  /// The earliest time a run ticked this frame draws something else; see [`TransitionRun::next_change`].
  pub(crate) fn next_change(&self) -> Option<Instant> {
    self.next_change
  }

  pub(crate) fn tick(&mut self, root: &mut crate::node::Node, now: Instant) -> bool {
    clear_overrides(root);
    self.has_active = false;
    let needs_layout = self.tick_recursive(root, now);
    self.has_active = !self.active.is_empty();
    needs_layout
  }

  pub(crate) fn tick_preserving_active_state(&mut self, root: &mut crate::node::Node, now: Instant) -> bool {
    clear_overrides(root);
    let needs_layout = self.tick_recursive(root, now);
    self.has_active = !self.active.is_empty();
    needs_layout
  }

  fn tick_recursive(&mut self, node: &mut crate::node::Node, now: Instant) -> bool {
    if !node.prepare_layout_tracking().timeline {
      return false;
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    let node_id = node.node_id();
    let mut needs_layout = false;
    if node_id.is_assigned() && !node.transitions.is_empty() {
      self.seen.insert(node_id);
      needs_layout = self.process_node(node, now);
    }

    for child in &mut node.children {
      if self.tick_recursive(child, now) {
        needs_layout = true;
      }
    }

    if needs_layout {
      node.layout_cache.invalidate();
    }

    needs_layout
  }

  fn process_node(&mut self, node: &mut crate::node::Node, now: Instant) -> bool {
    let node_id = node.node_id();
    let mut layout_affected = false;

    let properties: Vec<(AnimatableProperty, Duration, Duration, Easing)> = node
      .transitions
      .iter()
      .flat_map(|spec| {
        let props: &[AnimatableProperty] = match spec.property {
          TransitionProperty::All => ALL_PROPERTIES,
          TransitionProperty::Single(ref p) => std::slice::from_ref(p),
        };
        props.iter().map(move |p| (*p, spec.duration, spec.delay, spec.easing))
      })
      .collect();

    for (prop, duration, delay, easing) in &properties {
      let current = match read_target(node, *prop) {
        Some(v) => v,
        None => continue,
      };

      let key = (node_id, *prop);

      if let Some(prev) = self.prev_values.get(&key) {
        if *prev != current {
          let from = if let Some(running) = self.active.get(&key) {
            let elapsed = now.duration_since(running.start_time);
            if elapsed >= running.delay {
              let raw = running.progress(elapsed - running.delay);
              let t = running.easing.evaluate(raw.clamp(0.0, 1.0)) as f32;
              running.from.lerp(&running.to, t)
            } else {
              running.from
            }
          } else {
            *prev
          };

          self.active.insert(
            key,
            TransitionRun {
              from,
              to: current,
              start_time: now,
              duration: *duration,
              delay: *delay,
              easing: *easing,
            },
          );
        }
      }

      self.prev_values.insert(key, current);
    }

    let mut completed = Vec::new();
    for (&(nid, prop), run) in &self.active {
      if nid != node_id {
        continue;
      }
      let elapsed = now.duration_since(run.start_time);
      if elapsed < run.delay {
        layout_affected |= write_property(node, prop, &run.from);
        let at = run.start_time + run.delay;
        self.next_change = Some(self.next_change.map_or(at, |next| next.min(at)));
        continue;
      }
      let raw = run.progress(elapsed - run.delay);
      if raw >= 1.0 {
        layout_affected |= write_property(node, prop, &run.to);
        completed.push((nid, prop));
      } else {
        let t = run.easing.evaluate(raw) as f32;
        let interpolated = run.from.lerp(&run.to, t);
        layout_affected |= write_property(node, prop, &interpolated);
        if let Some(at) = run.next_change(now, raw) {
          self.next_change = Some(self.next_change.map_or(at, |next| next.min(at)));
        }
      }
    }

    for key in completed {
      self.active.remove(&key);
    }

    layout_affected
  }
}
