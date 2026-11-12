use std::{
  collections::{HashMap, HashSet},
  sync::OnceLock,
  time::{Duration, Instant},
};

use super::{
  easing::Easing,
  interpolate::write_property,
  keyframes::{KeyframeEntry, Keyframes, KeyframesId},
};
use crate::core::NodeId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationDirection {
  Normal,
  Reverse,
  Alternate,
  AlternateReverse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationFillMode {
  None,
  Forwards,
  Backwards,
  Both,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationIterationCount {
  Count(f64),
  Infinite,
}

#[derive(Clone, Debug)]
pub struct Animation {
  pub keyframes: KeyframesId,
  pub duration: Duration,
  pub delay: Duration,
  pub easing: Easing,
  pub direction: AnimationDirection,
  pub fill_mode: AnimationFillMode,
  pub iteration_count: AnimationIterationCount,
}

impl Animation {
  pub fn new(keyframes: impl Into<KeyframesId>) -> Self {
    Self {
      keyframes: keyframes.into(),
      duration: Duration::from_millis(300),
      delay: Duration::ZERO,
      easing: Easing::EASE,
      direction: AnimationDirection::Normal,
      fill_mode: AnimationFillMode::None,
      iteration_count: AnimationIterationCount::Count(1.0),
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

  pub fn direction(mut self, d: AnimationDirection) -> Self {
    self.direction = d;
    self
  }

  pub fn fill_mode(mut self, f: AnimationFillMode) -> Self {
    self.fill_mode = f;
    self
  }

  pub fn iteration_count(mut self, c: AnimationIterationCount) -> Self {
    self.iteration_count = c;
    self
  }

  pub fn infinite(mut self) -> Self {
    self.iteration_count = AnimationIterationCount::Infinite;
    self
  }

  pub fn alternate(mut self) -> Self {
    self.direction = AnimationDirection::Alternate;
    self
  }

  /// How long the animation takes from its start to its end, its delay
  /// included: one iteration for one that repeats without end.
  pub(crate) fn total_time(&self) -> Duration {
    let iterations = match self.iteration_count {
      AnimationIterationCount::Count(count) if count.is_finite() && count > 0.0 => count,
      AnimationIterationCount::Count(_) => 0.0,
      AnimationIterationCount::Infinite => 1.0,
    };
    let active = Duration::try_from_secs_f64(self.duration.as_secs_f64() * iterations).unwrap_or(Duration::MAX);
    self.delay.saturating_add(active)
  }
}

/// A frame due at a step boundary is drawn this long after it, so that it samples the new step even where
/// floating-point time lands a hair before the boundary.
const STEP_MARGIN: Duration = Duration::from_micros(100);

/// When a step `fraction` of `duration` after `now` is drawn, [`STEP_MARGIN`] after it: `now` for a fraction that
/// is not a finite, non-negative number, and `None` for one past what an `Instant` holds.
pub(super) fn step_time(now: Instant, duration: Duration, fraction: f64) -> Option<Instant> {
  if !(fraction.is_finite() && fraction >= 0.0) {
    return Some(now);
  }
  let offset = Duration::try_from_secs_f64(duration.as_secs_f64() * fraction).ok()?;
  now.checked_add(offset)?.checked_add(STEP_MARGIN)
}

struct AnimationRun {
  keyframes: KeyframesId,
  duration: Duration,
  delay: Duration,
  easing: Easing,
  direction: AnimationDirection,
  fill_mode: AnimationFillMode,
  iteration_count: f64,
  start_time: Instant,
  finished: bool,
}

/// Where a run is on its timeline at some time.
enum Phase {
  Delay,
  Active { iteration: f64, progress: f64 },
  Done,
}

impl AnimationRun {
  fn phase(&self, now: Instant) -> Phase {
    let elapsed = now.duration_since(self.start_time);
    if elapsed < self.delay {
      return Phase::Delay;
    }
    let duration = self.duration.as_secs_f64();
    let active = (elapsed - self.delay).as_secs_f64();
    if duration <= 0.0 || active >= duration * self.iteration_count {
      return Phase::Done;
    }
    let iteration = (active / duration).floor();
    Phase::Active {
      iteration,
      progress: active / duration - iteration,
    }
  }

  /// The keyframe progress a done run holds: the pose it stopped on when it fills forwards, `None` when it holds
  /// nothing. A whole iteration count stops at the end of the last iteration, a fractional one part way into the
  /// iteration after the last whole one, and a zero count at the start of the first. A zero duration plays every
  /// iteration in no time and holds the same pose; an infinite count ends only then, and holds the end of the
  /// first iteration.
  fn final_progress(&self) -> Option<f64> {
    let fills = matches!(self.fill_mode, AnimationFillMode::Forwards | AnimationFillMode::Both);
    fills.then(|| {
      let count = self.iteration_count.max(0.0);
      let (iteration, progress) = match count.fract() {
        _ if count == 0.0 => (0.0, 0.0),
        _ if count.is_infinite() => (0.0, 1.0),
        0.0 => (count - 1.0, 1.0),
        fraction => (count.floor(), fraction),
      };
      directed_progress(progress, self.direction, iteration)
    })
  }

  /// When the run next draws something other than what it draws at `now`: the end of its delay, the next step of a
  /// step-eased span, or the end of the iteration or of the run. A span that changes continuously changes at once,
  /// so it returns `now`. `None` once the run is done.
  fn next_change(&self, frames: &[KeyframeEntry], now: Instant) -> Option<Instant> {
    let (iteration, progress) = match self.phase(now) {
      Phase::Delay => return Some(self.start_time + self.delay),
      Phase::Done => return None,
      Phase::Active { iteration, progress } => (iteration, progress),
    };
    let forward = plays_forward(self.direction, iteration);
    let directed = if forward { progress } else { 1.0 - progress };
    // A step never lies past the end of its span, so the end of the iteration bounds it too; a fractional
    // iteration count ends the run inside an iteration.
    let Some(until_step) = distance_to_next_step(frames, directed, forward, &self.easing) else {
      return Some(now);
    };
    let until_end = self.iteration_count - iteration - progress;
    step_time(now, self.duration, until_step.min(until_end))
  }
}

static CLOCK_ORIGIN: OnceLock<Instant> = OnceLock::new();

/// The origin of the process's animation clock, which every in-phase run plays on (see
/// [`AnimationEngine::register_keyframes_in_phase`]): such a run starts a whole number of its periods after this
/// moment. It is the same for every tree, and so for every window, of the process; the first call fixes it.
pub fn clock_origin() -> Instant {
  *CLOCK_ORIGIN.get_or_init(Instant::now)
}

/// How long an in-phase run takes to come back to the same pose: its duration, or two durations for a direction
/// that alternates, so that every run started on that period's grid plays the same iterations forward.
fn phase_period(direction: AnimationDirection, duration: Duration) -> u128 {
  match direction {
    AnimationDirection::Normal | AnimationDirection::Reverse => duration.as_nanos(),
    AnimationDirection::Alternate | AnimationDirection::AlternateReverse => 2 * duration.as_nanos(),
  }
}

/// When a run that first ticks at `now` starts: then, or, in phase, at the last whole number of its periods
/// since the [`clock_origin`].
fn start_time(in_phase: bool, direction: AnimationDirection, duration: Duration, now: Instant) -> Instant {
  if !in_phase || duration.is_zero() {
    return now;
  }
  let since_origin = now.saturating_duration_since(clock_origin());
  let late = since_origin.as_nanos() % phase_period(direction, duration);
  // `late <= now - origin`: the remainder is at most the time since the clock's origin, which a process does not
  // run long enough to take past u64 nanoseconds (584 years). The same bound keeps `now - late` at or after the
  // origin, an `Instant` that exists.
  let late = u64::try_from(late).expect("the time since the animation clock's origin fits in u64 nanoseconds");
  now - Duration::from_nanos(late)
}

pub struct AnimationEngine {
  active: HashMap<(NodeId, KeyframesId), AnimationRun>,
  keyframe_store: HashMap<KeyframesId, Vec<KeyframeEntry>>,
  seen: HashSet<NodeId>,
  next_change: Option<Instant>,
  /// Keyframes whose runs play on the process's animation clock: see [`Self::register_keyframes_in_phase`].
  in_phase: HashSet<KeyframesId>,
  pub has_active: bool,
}

impl AnimationEngine {
  pub fn new() -> Self {
    Self {
      active: HashMap::new(),
      keyframe_store: HashMap::new(),
      seen: HashSet::new(),
      next_change: None,
      in_phase: HashSet::new(),
      has_active: false,
    }
  }

  pub fn register_keyframes(&mut self, keyframes: Keyframes) {
    self.in_phase.remove(&keyframes.id);
    self.keyframe_store.insert(keyframes.id, keyframes.frames);
  }

  /// Registers keyframes whose runs all play in phase on the process's animation clock: a run starts at the last
  /// whole number of its periods since [`clock_origin`], not when its node first ticks. The period is the
  /// animation's duration, or twice it for [`AnimationDirection::Alternate`] and
  /// [`AnimationDirection::AlternateReverse`], so that every run plays the same iterations forward and the same
  /// ones backward. Nodes that mount at different moments, in one window or several, then show the same pose and
  /// change it at the same moments, so a `Steps` spinner shown many times still draws one frame per step.
  ///
  /// A run that mounts late starts part-way through an iteration, as a CSS animation with a negative delay does;
  /// a finite iteration count therefore ends on the clock, not a whole count after the node mounted, and a run
  /// that mounts after that end in its period does not play. The `delay` is counted from the run's start on the
  /// clock, not from the mount: every run plays from a whole number of periods after the origin plus the delay,
  /// so a node that mounts within the delay of its period waits only for the rest of it (showing nothing, or the
  /// first pose with a backwards fill) while nodes that mounted earlier already play, and one that mounts later
  /// plays at once. Runs of the same animation show the same pose whenever they all play.
  ///
  /// Registering the same keyframes with [`Self::register_keyframes`] turns this off for runs that start
  /// afterwards.
  pub fn register_keyframes_in_phase(&mut self, keyframes: Keyframes) {
    self.in_phase.insert(keyframes.id);
    self.keyframe_store.insert(keyframes.id, keyframes.frames);
  }

  /// Forgets the runs of `node`, so that the animation it plays next starts
  /// from its beginning, even one of the keyframes it already played.
  pub(crate) fn forget_node(&mut self, node: NodeId) {
    self.active.retain(|(id, _), _| *id != node);
  }

  pub(crate) fn clear_state(&mut self) {
    self.active.clear();
    self.seen.clear();
    self.next_change = None;
    self.has_active = false;
  }

  /// Starts a frame: forgets which animated nodes last frame's walks visited.
  /// A frame may tick several subtrees (base tree, then each overlay), so the
  /// engine can only tell which runs belong to unmounted nodes once all walks
  /// are done — see [`Self::finish_frame`].
  pub(crate) fn begin_frame(&mut self) {
    self.seen.clear();
    self.next_change = None;
  }

  /// Ends a frame after every tick walk ran: drops runs whose node was not
  /// visited by any walk. Runs are kept for the node's whole lifetime (a
  /// finished run guards against restarting the animation), so without this
  /// prune an unmounted spinner's infinite run would keep `has_active` — and
  /// with it full per-frame relayout — for the rest of the process.
  pub(crate) fn finish_frame(&mut self) {
    let seen = &self.seen;
    self.active.retain(|(node_id, _), _| seen.contains(node_id));
    self.has_active = self.active.values().any(|run| !run.finished);
  }

  /// The earliest time a run ticked this frame draws something else; see [`AnimationRun::next_change`].
  pub(crate) fn next_change(&self) -> Option<Instant> {
    self.next_change
  }

  pub(crate) fn tick(&mut self, root: &mut crate::node::Node, now: Instant) -> bool {
    self.has_active = false;
    let needs_layout = self.tick_recursive(root, now);
    self.has_active = self.active.values().any(|run| !run.finished);
    needs_layout
  }

  pub(crate) fn tick_preserving_active_state(&mut self, root: &mut crate::node::Node, now: Instant) -> bool {
    let needs_layout = self.tick_recursive(root, now);
    self.has_active = self.active.values().any(|run| !run.finished);
    needs_layout
  }

  fn tick_recursive(&mut self, node: &mut crate::node::Node, now: Instant) -> bool {
    if !node.prepare_layout_tracking().timeline {
      return false;
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    let mut needs_layout = false;
    let node_id = node.node_id();
    if node_id.is_assigned()
      && let Some(spec) = node.playing_animation()
    {
      self.seen.insert(node_id);
      let spec = Animation::clone(spec);
      needs_layout = self.process_node(node, &spec, now);
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

  fn process_node(&mut self, node: &mut crate::node::Node, spec: &Animation, now: Instant) -> bool {
    let node_id = node.node_id();
    let key = (node_id, spec.keyframes);
    let iteration_count = match spec.iteration_count {
      AnimationIterationCount::Count(c) => c,
      AnimationIterationCount::Infinite => f64::INFINITY,
    };

    let in_phase = self.in_phase.contains(&spec.keyframes);
    let run = self.active.entry(key).or_insert_with(|| AnimationRun {
      keyframes: spec.keyframes,
      duration: spec.duration,
      delay: spec.delay,
      easing: spec.easing,
      direction: spec.direction,
      fill_mode: spec.fill_mode,
      iteration_count,
      start_time: start_time(in_phase, spec.direction, spec.duration, now),
      finished: false,
    });

    let frames = match self.keyframe_store.get(&run.keyframes) {
      Some(f) => f,
      None => return false,
    };

    if run.finished {
      // Each tick clears the overrides, so a run that fills forwards writes its last pose again. It asks for no
      // layout: a node that kept its cache was laid out with this pose, and a rebuilt node never keeps the cache
      // of one holding a layout override (`Node::own_layout_signature_matches`), so it is laid out afresh.
      // Asking here would invalidate the layout on every tick for as long as the pose is held.
      if let Some(progress) = run.final_progress() {
        apply_keyframe_at(node, frames, progress as f32, &run.easing);
      }
      return false;
    }

    let layout_affected = match run.phase(now) {
      Phase::Delay => {
        matches!(run.fill_mode, AnimationFillMode::Backwards | AnimationFillMode::Both)
          && apply_keyframe_at(
            node,
            frames,
            directed_progress(0.0, run.direction, 0.0) as f32,
            &run.easing,
          )
      }
      Phase::Done => {
        run.finished = true;
        run
          .final_progress()
          .is_some_and(|progress| apply_keyframe_at(node, frames, progress as f32, &run.easing))
      }
      Phase::Active { iteration, progress } => {
        let progress = directed_progress(progress, run.direction, iteration);
        apply_keyframe_at(node, frames, progress as f32, &run.easing)
      }
    };
    if let Some(at) = run.next_change(frames, now) {
      self.next_change = Some(self.next_change.map_or(at, |next| next.min(at)));
    }
    layout_affected
  }
}

fn plays_forward(direction: AnimationDirection, iteration: f64) -> bool {
  let even = (iteration as u64).is_multiple_of(2);
  match direction {
    AnimationDirection::Normal => true,
    AnimationDirection::Reverse => false,
    AnimationDirection::Alternate => even,
    AnimationDirection::AlternateReverse => !even,
  }
}

fn directed_progress(progress: f64, direction: AnimationDirection, iteration: f64) -> f64 {
  if plays_forward(direction, iteration) {
    progress
  } else {
    1.0 - progress
  }
}

/// How far keyframe progress moves, in the direction it plays, before the frames draw something else: to the next
/// step of the span at `progress` when that span is step-eased. `None` when the span changes continuously.
fn distance_to_next_step(
  frames: &[KeyframeEntry],
  progress: f64,
  forward: bool,
  overall_easing: &Easing,
) -> Option<f64> {
  let (lo, hi) = find_surrounding_frames(frames, progress as f32);
  if lo == hi {
    return None;
  }
  let (lo_frame, hi_frame) = (&frames[lo], &frames[hi]);
  let span = f64::from(hi_frame.offset - lo_frame.offset);
  if span <= 0.0 {
    return None;
  }
  let local = (progress - f64::from(lo_frame.offset)) / span;
  let easing = lo_frame.easing.as_ref().unwrap_or(overall_easing);
  easing
    .distance_to_next_step(local, forward)
    .map(|distance| distance * span)
}

fn apply_keyframe_at(
  node: &mut crate::node::Node,
  frames: &[KeyframeEntry],
  progress: f32,
  overall_easing: &Easing,
) -> bool {
  if frames.is_empty() {
    return false;
  }

  let mut layout_affected = false;
  let (lo, hi) = find_surrounding_frames(frames, progress);
  let lo_frame = &frames[lo];
  let hi_frame = &frames[hi];

  if lo == hi {
    for (prop, value) in &lo_frame.values {
      layout_affected |= write_property(node, *prop, value);
    }
    return layout_affected;
  }

  let span = hi_frame.offset - lo_frame.offset;
  let local = if span > 0.0 {
    ((progress - lo_frame.offset) / span).clamp(0.0, 1.0)
  } else {
    1.0
  };

  let easing = lo_frame.easing.as_ref().unwrap_or(overall_easing);
  let t = easing.evaluate(local as f64) as f32;

  for (prop, lo_val) in &lo_frame.values {
    if let Some((_, hi_val)) = hi_frame.values.iter().find(|(p, _)| p == prop) {
      let interpolated = lo_val.lerp(hi_val, t);
      layout_affected |= write_property(node, *prop, &interpolated);
    } else {
      layout_affected |= write_property(node, *prop, lo_val);
    }
  }

  layout_affected
}

fn find_surrounding_frames(frames: &[KeyframeEntry], progress: f32) -> (usize, usize) {
  if frames.len() == 1 {
    return (0, 0);
  }

  for (i, frame) in frames.iter().enumerate().skip(1) {
    if frame.offset >= progress {
      return (i - 1, i);
    }
  }

  let last = frames.len() - 1;
  (last, last)
}
