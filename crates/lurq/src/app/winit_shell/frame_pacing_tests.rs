//! Paced animation frames, run through a model of the winit loop: each turn ticks timers, futures and scheduled
//! redraws, asks [`FramePacing`] for the window's next frame, renders when the tree needs a redraw and otherwise
//! sleeps as [`LoopWait`] says. The render engine models a 60 Hz FIFO swapchain with a frame latency of one: a
//! frame waits for the vblank after the previous present, as `get_current_texture` does, then takes its render
//! cost. These tests run on the wall clock, so their bounds leave room for a busy machine; the cadence rule itself
//! is also checked on a virtual clock, which no scheduling noise reaches, and so is how long input and timers wait
//! while frames are paced ([`VirtualShell`]). Nothing on the virtual clock may read the wall clock: a timer takes its
//! first tick from it when it starts, so those tests place the loop's clock from that tick ([`Clock::start_at`]).

use std::{
  collections::VecDeque,
  f32::consts::TAU,
  num::NonZeroIsize,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use raw_window_handle::{
  DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
use winit::{dpi::PhysicalPosition, event::WindowEvent};

use super::{
  frame_pacing::{FramePacing, PresentedFrame, frame_due},
  loop_wait::{LoopNeeds, LoopWait},
};
use crate::{
  animation::{AnimatableProperty, Animation, Easing, Keyframes, KeyframesId, StepPosition},
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, Interval},
    events::MouseButton,
    render_engine::RenderEngine,
  },
  components::{Rect, Row},
  layout::render_list::RenderList,
  node::{Element, transform::Decomposed},
};

const REFRESH: Duration = Duration::from_nanos(16_666_667);
const TURN: KeyframesId = KeyframesId::new(41);
const SPINNER: (f32, f32) = (20.0, 20.0);
/// How many vblanks of a 0.6 s wall-clock run may go without a frame because the machine held the loop back.
const MACHINE_SKIPS: u128 = 2;

struct TestSurface;

impl HasWindowHandle for TestSurface {
  fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
    let handle = Win32WindowHandle::new(NonZeroIsize::new(1).expect("1 is not zero"));
    // SAFETY: the handle is never dereferenced: the test render engine ignores it.
    Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
  }
}

impl HasDisplayHandle for TestSurface {
  fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
    // SAFETY: as above, the handle is never used.
    Ok(unsafe { DisplayHandle::borrow_raw(WindowsDisplayHandle::new().into()) })
  }
}

#[derive(Clone, Copy, Debug)]
struct Frame {
  /// When the pass that drew the frame started, which is the time the animation was sampled at.
  pass_at: Instant,
  /// The spinner's rotation, in radians.
  angle: f32,
}

struct Display {
  epoch: Instant,
  last_present: Option<Instant>,
  pass_at: Instant,
  /// The CPU time a frame takes once the swapchain hands it a buffer.
  render_cost: Duration,
  frames: Vec<Frame>,
  /// The vblank that shows each presented frame.
  vblanks: Vec<u128>,
}

impl Display {
  fn vblank_index(&self, at: Instant) -> u128 {
    vblank_index(self.epoch, REFRESH, at)
  }

  fn vblank_after(&self, at: Instant) -> Instant {
    let refreshes = self.vblank_index(at);
    self.epoch + REFRESH * u32::try_from(refreshes).expect("a test runs for less than two years")
  }
}

/// Which vblank shows a frame presented at `at`: the first after it, counted from the vblank at `epoch`.
fn vblank_index(epoch: Instant, refresh: Duration, at: Instant) -> u128 {
  (at - epoch).as_nanos() / refresh.as_nanos() + 1
}

/// How many vblanks between the first and the last present showed no new frame.
fn skipped_vblanks(vblanks: &[u128]) -> u128 {
  vblanks.windows(2).map(|pair| pair[1] - pair[0] - 1).sum()
}

struct VsyncEngine(Arc<Mutex<Display>>);

impl RenderEngine for VsyncEngine {
  fn resize(&mut self, _width: u32, _height: u32) {}

  fn render(&mut self, list: &RenderList, _window: WindowHandle<'_>, _display: DisplayHandle<'_>) -> bool {
    let mut display = self.0.lock().expect("the display is used on one thread");
    if let Some(previous) = display.last_present {
      sleep_until(display.vblank_after(previous));
    }
    sleep_until(Instant::now() + display.render_cost);
    let presented = Instant::now();
    display.last_present = Some(presented);
    let vblank = display.vblank_index(presented);
    display.vblanks.push(vblank);
    let [a, b, _, _] = list.rects.first().map_or([1.0, 0.0, 0.0, 1.0], |rect| rect.transform);
    let frame = Frame {
      pass_at: display.pass_at,
      angle: b.atan2(a).rem_euclid(TAU),
    };
    display.frames.push(frame);
    true
  }
}

/// Sleeps until `at` as an event loop waits for a deadline: on time. A plain `thread::sleep` is not: macOS lets a
/// sleep run over by up to half its length (12.5 ms asked, 18.75 ms slept), more than the quarter of a refresh a
/// loop's wake may be late by and still come back to a frame per vblank after it missed one. Sleeping half of what
/// is left, again and again, keeps each overrun inside the time left.
fn sleep_until(at: Instant) {
  loop {
    let now = Instant::now();
    if now >= at {
      return;
    }
    std::thread::sleep((at - now) / 2);
  }
}

/// The winit loop of one window, without winit.
struct Shell {
  app: App,
  tree: Tree,
  pacing: FramePacing,
  display: Arc<Mutex<Display>>,
  /// Clicks on the spinner the "OS" delivers at these times.
  clicks: VecDeque<Instant>,
  /// How late each due timer fired.
  timer_lateness: Vec<Duration>,
  /// Turns that ended with the loop asleep.
  sleeps: usize,
}

impl Shell {
  fn new(tree: Tree) -> Self {
    let display = Arc::new(Mutex::new(Display {
      epoch: Instant::now(),
      last_present: None,
      pass_at: Instant::now(),
      render_cost: Duration::ZERO,
      frames: Vec::new(),
      vblanks: Vec::new(),
    }));
    let mut shell = Self {
      app: App::new(),
      tree,
      pacing: FramePacing::default(),
      display: display.clone(),
      clicks: VecDeque::new(),
      timer_lateness: Vec::new(),
      sleeps: 0,
    };
    shell.tree.resize(200, 100);
    shell
      .tree
      .set_render_engine_factory(move || Box::new(VsyncEngine(display.clone())));
    shell.tree.request_redraw();
    shell
  }

  fn run_for(&mut self, duration: Duration) {
    let end = Instant::now() + duration;
    loop {
      let now = Instant::now();
      if now >= end {
        return;
      }
      self.turn(now, end);
    }
  }

  fn turn(&mut self, now: Instant, end: Instant) {
    while self.clicks.front().is_some_and(|at| *at <= now) {
      self.clicks.pop_front();
      self.tree.mouse_down(SPINNER.0, SPINNER.1, MouseButton::Left);
      self.tree.mouse_up(SPINNER.0, SPINNER.1, MouseButton::Left);
    }
    let timer_due = self.tree.next_timer_at().filter(|due| *due <= now);
    self.tree.tick_timers();
    if let Some(due) = timer_due {
      self.timer_lateness.push(Instant::now() - due);
    }
    self.tree.tick_futures();
    self.pacing.request_frame(&mut self.tree, || REFRESH, now);
    self.tree.tick_scheduled_redraw(now);
    if self.tree.needs_redraw() {
      let started = Instant::now();
      self.display.lock().expect("one thread").pass_at = started;
      if self.tree.pass(&mut self.app, &TestSurface).rendered {
        self.pacing.presented(PresentedFrame {
          started,
          presented: Instant::now(),
        });
      }
      return;
    }
    let needs = LoopNeeds {
      polling: self.tree.needs_polling(),
      next_redraw: self.tree.next_scheduled_redraw(),
      next_timer: self.tree.next_timer_at(),
      ..LoopNeeds::default()
    };
    let wake = match LoopWait::of(needs, now) {
      LoopWait::Poll => return,
      LoopWait::WaitUntil(at) => at.min(end),
      LoopWait::Wait => end,
    };
    let wake = self.clicks.front().map_or(wake, |click| wake.min(*click));
    self.sleeps += 1;
    sleep_until(wake);
  }

  fn frames(&self) -> Vec<Frame> {
    self.display.lock().expect("one thread").frames.clone()
  }

  fn set_render_cost(&mut self, cost: Duration) {
    self.display.lock().expect("one thread").render_cost = cost;
  }

  fn vblanks(&self) -> Vec<u128> {
    self.display.lock().expect("one thread").vblanks.clone()
  }
}

fn turn_keyframes() -> Keyframes {
  Keyframes::new(TURN)
    .frame(0.0, |f| {
      f.set(AnimatableProperty::Transform, Decomposed::IDENTITY.with_rotate(0.0));
    })
    .frame(1.0, |f| {
      f.set(AnimatableProperty::Transform, Decomposed::IDENTITY.with_rotate(TAU));
    })
}

fn steps(count: u32) -> Easing {
  Easing::Steps {
    count,
    position: StepPosition::JumpEnd,
  }
}

fn spinner(easing: Easing) -> Tree {
  let mut tree = Tree::new();
  tree.register_keyframes(turn_keyframes());
  tree.set_root(
    Row::new().child(
      Rect::new(40.0, 40.0)
        .background("#2563eb")
        .animation(Animation::new(TURN).duration_ms(1000).easing(easing).infinite()),
    ),
  );
  tree
}

/// Which of the `count` poses of a turn the angle is.
fn pose(angle: f32, count: u32) -> u32 {
  ((angle / (TAU / count as f32)).round() as u32) % count
}

#[test]
fn a_spinner_of_twelve_steps_a_second_presents_about_twelve_frames_a_second() {
  let mut shell = Shell::new(spinner(steps(12)));
  shell.run_for(Duration::from_secs(1));

  let frames = shell.frames();
  assert!(
    (11..=14).contains(&frames.len()),
    "a 12-step turn per second draws a frame per step, not one per refresh: {} frames in 1 s",
    frames.len()
  );
  for pair in frames.windows(2) {
    assert_ne!(
      pose(pair[0].angle, 12),
      pose(pair[1].angle, 12),
      "every frame shows the next step: {frames:?}"
    );
  }
}

#[test]
fn a_continuous_spinner_advances_every_refresh_through_paced_frames() {
  let mut shell = Shell::new(spinner(Easing::Linear));
  shell.run_for(Duration::from_millis(500));

  let frames = shell.frames();
  assert!(
    (24..=32).contains(&frames.len()),
    "a continuous animation runs at the display's rate: {} frames in 0.5 s",
    frames.len()
  );
  let advanced = frames.windows(2).filter(|pair| pair[0].angle != pair[1].angle).count();
  assert_eq!(advanced, frames.len() - 1, "every frame turns the spinner: {frames:?}");
  assert!(
    shell.sleeps >= frames.len() - 1,
    "the loop sleeps until each frame is due instead of waiting for the swapchain: {} sleeps for {} frames",
    shell.sleeps,
    frames.len()
  );
}

#[test]
fn the_loop_waits_for_an_event_once_an_animation_ends() {
  let mut tree = Tree::new();
  tree.register_keyframes(turn_keyframes());
  tree.set_root(
    Row::new().child(Rect::new(40.0, 40.0).animation(Animation::new(TURN).duration_ms(300).easing(steps(3)))),
  );
  let mut shell = Shell::new(tree);
  shell.run_for(Duration::from_millis(500));

  let frames = shell.frames().len();
  assert!((3..=5).contains(&frames), "three steps and the end: {frames} frames");
  assert!(!shell.tree.has_active_timeline());
  assert!(!shell.tree.needs_redraw());
  let needs = LoopNeeds {
    polling: shell.tree.needs_polling(),
    next_redraw: shell.tree.next_scheduled_redraw(),
    next_timer: shell.tree.next_timer_at(),
    ..LoopNeeds::default()
  };
  assert_eq!(
    LoopWait::of(needs, Instant::now()),
    LoopWait::Wait,
    "nothing is left to wake for"
  );
}

#[test]
fn a_minimized_window_draws_no_animation_frames_and_resumes_on_time_when_restored() {
  let mut shell = Shell::new(spinner(steps(12)));
  shell.run_for(Duration::from_millis(300));
  let started_at = shell.frames()[0].pass_at;

  shell.tree.window().set_minimized(true);
  let frames_before = shell.frames().len();
  shell.run_for(Duration::from_millis(500));
  assert_eq!(
    shell.frames().len(),
    frames_before,
    "a minimized window draws no animation frames"
  );

  shell.tree.window().set_minimized(false);
  let restored_at = Instant::now();
  shell.run_for(Duration::from_millis(400));

  let resumed = &shell.frames()[frames_before..];
  assert!(
    (4..=7).contains(&resumed.len()),
    "the spinner turns again at its pace after the restore: {} frames in 0.4 s",
    resumed.len()
  );
  assert!(
    resumed[0].pass_at - restored_at < REFRESH,
    "the first frame comes at once"
  );
  for frame in resumed {
    let elapsed_steps = ((frame.pass_at - started_at).as_secs_f64() * 12.0).floor() as u32 % 12;
    let drawn = pose(frame.angle, 12);
    let off = (drawn + 12 - elapsed_steps) % 12;
    assert!(
      off == 0 || off == 11,
      "the animation kept its time while minimized: step {drawn} drawn at step {elapsed_steps} of the clock"
    );
  }
}

/// The time of a test's loop: a virtual clock that only the loop moves.
#[derive(Clone)]
struct Clock(Arc<Mutex<Instant>>);

impl Clock {
  fn new() -> Self {
    Self(Arc::new(Mutex::new(Instant::now())))
  }

  fn now(&self) -> Instant {
    *self.0.lock().expect("one thread")
  }

  /// Moves the clock to `at`, never back.
  fn advance_to(&self, at: Instant) {
    let mut now = self.0.lock().expect("one thread");
    *now = (*now).max(at);
  }

  /// Sets the time the loop starts at, before it runs. A timer started on the wall clock has its first tick at a
  /// wall-clock time: a test starts its clock a chosen time before that tick, or the time the machine took to
  /// mount the component would decide when the timer fires between two vblanks.
  fn start_at(&self, at: Instant) {
    *self.0.lock().expect("one thread") = at;
  }
}

/// When each click reached the spinner's handler, on the loop's clock, and how the spinner turns.
#[derive(Clone)]
struct ClickLog {
  at: Arc<Mutex<Vec<Instant>>>,
  clock: Clock,
  easing: Easing,
}

impl PartialEq for ClickLog {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.at, &other.at)
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for ClickLog {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// A turning spinner that logs its clicks, beside a 25 ms interval.
struct ClickableSpinner {
  clicks: ClickLog,
  _interval: Interval,
}

impl Component for ClickableSpinner {
  type Props = ClickLog;

  fn create(ctx: &mut Ctx) -> Self {
    let interval = ctx.create_interval(Duration::from_millis(25), || {});
    interval.start();
    Self {
      clicks: ctx.props::<ClickLog>().clone(),
      _interval: interval,
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let clicks = self.clicks.clone();
    Row::new().child(
      Rect::new(40.0, 40.0)
        .background("#2563eb")
        .animation(Animation::new(TURN).duration_ms(1000).easing(clicks.easing).infinite())
        .on_click(move |_| clicks.at.lock().expect("one thread").push(clicks.clock.now())),
    )
  }
}

/// The 60 Hz FIFO swapchain of [`VsyncEngine`] on a virtual clock: a present waits for the vblank after the
/// previous one by moving the clock there, then takes its render cost the same way.
struct VirtualVsyncEngine {
  clock: Clock,
  epoch: Instant,
  render_cost: Duration,
  /// When each frame was presented.
  presents: Arc<Mutex<Vec<Instant>>>,
}

impl RenderEngine for VirtualVsyncEngine {
  fn resize(&mut self, _width: u32, _height: u32) {}

  fn render(&mut self, _list: &RenderList, _window: WindowHandle<'_>, _display: DisplayHandle<'_>) -> bool {
    let mut presents = self.presents.lock().expect("one thread");
    if let Some(previous) = presents.last() {
      let vblanks = vblank_index(self.epoch, REFRESH, *previous);
      self
        .clock
        .advance_to(self.epoch + REFRESH * u32::try_from(vblanks).expect("a test runs for less than two years"));
    }
    self.clock.advance_to(self.clock.now() + self.render_cost);
    presents.push(self.clock.now());
    true
  }
}

/// What asked for a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AskedBy {
  /// The pacing rule, for the timeline's next change.
  Pacing,
  /// A click or a timer, which draws at once.
  Event,
}

/// An event that waited while a present waited for its vblank.
#[derive(Clone, Copy, Debug)]
struct Held {
  waited: Duration,
  /// What asked for the frame it waited behind; `None` when no frame was presented in the turn before.
  behind: Option<AskedBy>,
}

/// The loop of [`Shell`] on a virtual clock, which moves only when the loop sleeps or a present waits for its
/// vblank: what it measures is the pacing rule's, whatever else the machine runs.
struct VirtualShell {
  app: App,
  tree: Tree,
  pacing: FramePacing,
  clock: Clock,
  presents: Arc<Mutex<Vec<Instant>>>,
  clicks: VecDeque<Instant>,
  timer_lateness: Vec<Duration>,
  /// When each due timer fired.
  timer_fired: Vec<Instant>,
  /// What asked for the frame the last turn presented.
  presented_for: Option<AskedBy>,
  /// The clicks and timers that did not run when they were due.
  held: Vec<Held>,
}

impl VirtualShell {
  fn new(tree: Tree, clock: Clock) -> Self {
    Self::with_render_cost(tree, clock, Duration::ZERO)
  }

  fn with_render_cost(mut tree: Tree, clock: Clock, render_cost: Duration) -> Self {
    let presents = Arc::new(Mutex::new(Vec::new()));
    let engine = (clock.clone(), clock.now(), presents.clone());
    tree.resize(200, 100);
    tree.set_render_engine_factory(move || {
      Box::new(VirtualVsyncEngine {
        clock: engine.0.clone(),
        epoch: engine.1,
        render_cost,
        presents: engine.2.clone(),
      })
    });
    tree.request_redraw();
    Self {
      app: App::new(),
      tree,
      pacing: FramePacing::default(),
      clock,
      presents,
      clicks: VecDeque::new(),
      timer_lateness: Vec::new(),
      timer_fired: Vec::new(),
      presented_for: None,
      held: Vec::new(),
    }
  }

  fn run_for(&mut self, duration: Duration) {
    let end = self.clock.now() + duration;
    // A loop that never sleeps would not move the clock: the turns are bounded instead.
    for _ in 0..100_000 {
      if self.clock.now() >= end {
        return;
      }
      self.turn(end);
    }
    panic!("the loop kept polling without sleeping");
  }

  fn turn(&mut self, end: Instant) {
    let now = self.clock.now();
    self.tree.set_animation_clock(Some(now));
    // The clock moved past a due event only while the last turn's present waited.
    let behind = self.presented_for.take();
    while let Some(sent) = self.clicks.front().copied().filter(|at| *at <= now) {
      self.clicks.pop_front();
      self.tree.mouse_down(SPINNER.0, SPINNER.1, MouseButton::Left);
      self.tree.mouse_up(SPINNER.0, SPINNER.1, MouseButton::Left);
      self.note_wait(now - sent, behind);
    }
    let timer_due = self.tree.next_timer_at().filter(|due| *due <= now);
    self.tree.tick_timers_at(now);
    if let Some(due) = timer_due {
      self.timer_lateness.push(now - due);
      self.timer_fired.push(now);
      self.note_wait(now - due, behind);
    }
    self.tree.tick_futures();
    let asked_by = if self.tree.needs_redraw() {
      AskedBy::Event
    } else {
      AskedBy::Pacing
    };
    self.pacing.request_frame(&mut self.tree, || REFRESH, now);
    self.tree.tick_scheduled_redraw(now);
    if self.tree.needs_redraw() {
      if self.tree.pass(&mut self.app, &TestSurface).rendered {
        self.presented_for = Some(asked_by);
        let presented = self.presents.lock().expect("one thread").last().copied().unwrap_or(now);
        self.pacing.presented(PresentedFrame {
          started: now,
          presented,
        });
      }
      return;
    }
    let needs = LoopNeeds {
      polling: self.tree.needs_polling(),
      next_redraw: self.tree.next_scheduled_redraw(),
      next_timer: self.tree.next_timer_at(),
      ..LoopNeeds::default()
    };
    let wake = match LoopWait::of(needs, now) {
      LoopWait::Poll => return,
      LoopWait::WaitUntil(at) => at.min(end),
      LoopWait::Wait => end,
    };
    let wake = self.clicks.front().map_or(wake, |click| wake.min(*click));
    self.clock.advance_to(wake);
  }

  fn note_wait(&mut self, waited: Duration, behind: Option<AskedBy>) {
    if !waited.is_zero() {
      self.held.push(Held { waited, behind });
    }
  }
}

/// How long each event waited while [`ClickableSpinner`] turned for 600 ms: its interval, and a click every 37 ms
/// when the run has clicks.
struct SpinnerRun {
  click_latency: Vec<Duration>,
  timer_lateness: Vec<Duration>,
  /// The clicks and ticks that waited, and behind which frame, from the loop's start.
  held: Vec<Held>,
  /// Frames presented while the clicks came.
  presents: usize,
}

/// Turns a spinner of twelve steps a second and clicks it.
fn click_a_turning_spinner(first_tick: Duration) -> SpinnerRun {
  run_a_spinner(steps(12), first_tick, 12)
}

/// Runs the loop with the interval's first tick `first_tick` after the loop's start. The spinner's first frame is
/// presented at the start, which is also a vblank, so `first_tick` is where the 25 ms ticks fall between the
/// vblanks, the spinner's frames and the clicks.
fn run_a_spinner(easing: Easing, first_tick: Duration, click_count: u32) -> SpinnerRun {
  let clock = Clock::new();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.register_keyframes(turn_keyframes());
  let log = ClickLog {
    at: Arc::default(),
    clock: clock.clone(),
    easing,
  };
  tree.mount_root::<ClickableSpinner>(&mut app, log.clone());
  clock.start_at(tree.next_timer_at().expect("the interval runs") - first_tick);
  let mut shell = VirtualShell::new(tree, clock.clone());
  shell.run_for(Duration::from_millis(100));

  let first_click = clock.now() + Duration::from_millis(7);
  let clicks: Vec<Instant> = (0..click_count)
    .map(|i| first_click + Duration::from_millis(37) * i)
    .collect();
  shell.clicks = clicks.iter().copied().collect();
  shell.timer_lateness.clear();
  let presents_before = shell.presents.lock().expect("one thread").len();
  shell.run_for(Duration::from_millis(600));

  let clicked_at = log.at.lock().expect("one thread").clone();
  assert_eq!(clicked_at.len(), clicks.len(), "every click is handled");
  let click_latency = clicks
    .iter()
    .zip(&clicked_at)
    .map(|(sent, handled)| *handled - *sent)
    .collect();
  let timer_lateness = shell.timer_lateness.clone();
  assert!(
    timer_lateness.len() >= 20,
    "the 25 ms interval keeps firing: {timer_lateness:?}"
  );
  SpinnerRun {
    click_latency,
    timer_lateness,
    held: shell.held.clone(),
    presents: shell.presents.lock().expect("one thread").len() - presents_before,
  }
}

/// Where the 25 ms ticks fall between the vblanks decides which of them meet a waiting present, so the tests of
/// how long an event waits run every phase: the first tick from 0.5 ms to 25 ms after the start.
fn first_ticks() -> impl Iterator<Item = Duration> {
  (500..=25_000).step_by(500).map(Duration::from_micros)
}

#[test]
fn a_timer_waits_half_a_refresh_at_most_behind_the_paced_frames_of_a_continuous_spinner() {
  // The loop sleeps between frames and wakes for a timer when it is due, so a timer waits only when it is due
  // while a present waits for its vblank. A paced frame is asked for half a refresh after the last present, so its
  // present waits half a refresh at most, and so does the timer. A loop that waited in the swapchain between
  // frames, asking for each frame right after the last present, would hold timers back for up to a whole refresh.
  let bound = REFRESH / 2 + Duration::from_micros(1);
  let mut longest = Duration::ZERO;
  for first_tick in first_ticks() {
    let SpinnerRun {
      timer_lateness, held, ..
    } = run_a_spinner(Easing::Linear, first_tick, 0);
    for tick in &held {
      assert!(
        tick.behind == Some(AskedBy::Pacing) && tick.waited <= bound,
        "with the first tick {first_tick:?} after the start, a tick waits only behind a paced frame, and half a \
         refresh at most: {tick:?}
  timer lateness {timer_lateness:?}"
      );
      longest = longest.max(tick.waited);
    }
  }
  assert!(
    longest > REFRESH / 4,
    "some phase has a tick come early in a present's wait, or the bound was not tested: the longest wait was \
     {longest:?}"
  );
}

#[test]
fn an_event_waits_a_whole_refresh_at_most_behind_the_frame_a_click_asked_for() {
  // A frame a click asks for is not paced: it is drawn at once. When the click comes right after a present, its
  // own present waits for the next vblank, up to a whole refresh away, and the events that come meanwhile wait
  // with it: the half refresh of a paced frame does not bound them. With the first tick 5.5 ms after the start, a
  // step of the spinner is presented at 250.1 ms, just after a vblank; the click at 255 ms draws a frame whose
  // present waits for the vblank at 266.7 ms, and the tick due at 255.5 ms fires 11.2 ms late.
  let paced_bound = REFRESH / 2 + Duration::from_micros(1);
  let click_bound = REFRESH + Duration::from_micros(1);
  let mut longest_behind_a_click = Duration::ZERO;
  for first_tick in first_ticks() {
    let SpinnerRun {
      click_latency,
      timer_lateness,
      held,
      ..
    } = click_a_turning_spinner(first_tick);
    for event in &held {
      let bound = match event.behind {
        Some(AskedBy::Pacing) => paced_bound,
        Some(AskedBy::Event) => click_bound,
        None => Duration::ZERO,
      };
      assert!(
        event.waited <= bound,
        "with the first tick {first_tick:?} after the start, an event waits a whole refresh at most behind a \
         click's frame, half of one behind a paced frame, and for nothing else: {event:?}
  click latency {click_latency:?}
  timer lateness {timer_lateness:?}"
      );
      if event.behind == Some(AskedBy::Event) {
        longest_behind_a_click = longest_behind_a_click.max(event.waited);
      }
    }
  }
  assert!(
    longest_behind_a_click > paced_bound,
    "some phase has a tick wait more than half a refresh behind a click's frame; if frames that input asks for \
     are now paced too, bound them by half a refresh here: the longest wait was {longest_behind_a_click:?}"
  );
}

#[test]
fn an_interval_that_changes_nothing_presents_no_frames_so_most_events_wait_for_none() {
  let SpinnerRun {
    click_latency,
    timer_lateness,
    presents,
    ..
  } = click_a_turning_spinner(Duration::from_millis(25));
  // In 600 ms the spinner draws about 7 steps and each click one frame; the interval's 24 ticks draw nothing.
  assert!(
    presents <= 7 + 1 + click_latency.len(),
    "only the spinner's steps and the clicks present frames: {presents} in 0.6 s"
  );
  // An event waits only when it comes while one of those presents waits for its vblank, which is at most half a
  // refresh after each of the spinner's 12 frames a second: about a tenth of the time.
  let events = click_latency.len() + timer_lateness.len();
  let at_once = click_latency
    .iter()
    .chain(&timer_lateness)
    .filter(|latency| latency.is_zero())
    .count();
  assert!(
    at_once * 4 >= events * 3,
    "most events are handled at once: {at_once} of {events}
  click latency {click_latency:?}
  timer lateness {timer_lateness:?}"
  );
}

/// A static rect beside a 25 ms interval whose callback bumps a counter that it shows when `shows` is true, and
/// bumps one nothing reads otherwise.
struct CountingInterval {
  count: crate::core::Signal<u32>,
  shows: bool,
  _interval: Interval,
}

impl Component for CountingInterval {
  type Props = bool;

  fn create(ctx: &mut Ctx) -> Self {
    let shows = *ctx.props::<bool>();
    let count = ctx.signal(0_u32);
    let unread = crate::core::Ref::new(0_u32);
    let interval = ctx.create_interval(Duration::from_millis(25), {
      let count = count.clone();
      move || {
        if shows {
          count.update(|count| *count += 1);
        } else {
          unread.update(|count| *count += 1);
        }
      }
    });
    interval.start();
    Self {
      count,
      shows,
      _interval: interval,
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let width = if self.shows {
      10.0 + (self.count.get() % 10) as f32
    } else {
      10.0
    };
    Row::new().child(Rect::new(width, 40.0).background("#2563eb"))
  }
}

/// When frames were presented and when the interval fired, in a 500 ms run of [`CountingInterval`] after its first
/// frame.
fn run_counting_interval(shows: bool) -> (Vec<Instant>, Vec<Instant>) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<CountingInterval>(&mut app, shows);
  // The interval starts on the wall clock: the loop's clock starts after it, so the first tick is not overdue.
  let clock = Clock::new();
  let mut shell = VirtualShell::new(tree, clock.clone());
  shell.run_for(Duration::from_millis(5));
  let started = clock.now();
  shell.run_for(Duration::from_millis(500));
  let presents = shell.presents.lock().expect("one thread").clone();
  let presents = presents.into_iter().filter(|at| *at > started).collect();
  assert!(
    shell.timer_fired.len() >= 19,
    "the interval keeps firing: {} ticks",
    shell.timer_fired.len()
  );
  assert!(
    shell.timer_lateness.iter().all(Duration::is_zero),
    "each tick fires when it is due: {:?}",
    shell.timer_lateness
  );
  (presents, shell.timer_fired)
}

#[test]
fn an_interval_that_changes_nothing_shown_presents_no_frames() {
  let (presents, _) = run_counting_interval(false);
  assert!(
    presents.is_empty(),
    "an interval that changes nothing on screen draws nothing: {presents:?}"
  );
}

#[test]
fn an_interval_that_changes_what_is_shown_presents_each_change_when_it_fires() {
  let (presents, fired) = run_counting_interval(true);
  assert_eq!(
    presents, fired,
    "each tick's frame is presented in the turn the tick fires"
  );
}

#[test]
fn a_continuous_spinner_holds_the_displays_rate_on_the_wall_clock_whatever_its_render_cost() {
  for cost_ms in 0..=3 {
    let mut shell = Shell::new(spinner(Easing::Linear));
    shell.set_render_cost(Duration::from_millis(cost_ms));
    shell.run_for(Duration::from_millis(600));

    // That every vblank shows a new frame is checked on a virtual clock, where it holds exactly. Here the machine
    // may hold the loop back, and a frame that starts more than half a refresh late misses its vblank: a few
    // vblanks may go without a frame, but not one in six, as when frames are paced a whole refresh after the last
    // present and 3 ms of work pushes every sixth past its vblank, and not every other one.
    // The first frame comes when the run starts, off the vblank grid; the cadence counts from the second.
    let vblanks = shell.vblanks();
    let skipped = skipped_vblanks(&vblanks[1..]);
    assert!(
      skipped <= MACHINE_SKIPS,
      "with {cost_ms} ms of render work, about every vblank shows a new frame: {} presents, {skipped} vblanks \
       without one",
      vblanks.len()
    );
    assert!(
      (30..=39).contains(&vblanks.len()),
      "60 presents a second: {} in 0.6 s with {cost_ms} ms of work",
      vblanks.len()
    );
  }
}

#[test]
fn on_a_virtual_clock_the_loop_presents_a_continuous_spinner_at_every_vblank_whatever_its_render_cost() {
  for cost_us in (0..=8000).step_by(500) {
    let cost = Duration::from_micros(cost_us);
    let clock = Clock::new();
    let epoch = clock.now();
    let mut shell = VirtualShell::with_render_cost(spinner(Easing::Linear), clock, cost);
    shell.run_for(Duration::from_millis(600));

    let vblanks: Vec<u128> = shell
      .presents
      .lock()
      .expect("one thread")
      .iter()
      .map(|present| vblank_index(epoch, REFRESH, *present))
      .collect();
    assert_eq!(
      skipped_vblanks(&vblanks),
      0,
      "with {cost:?} of render work, every vblank shows a new frame: {vblanks:?}"
    );
    assert!(
      (36..=37).contains(&vblanks.len()),
      "a frame per vblank for 0.6 s: {} presents with {cost:?} of work",
      vblanks.len()
    );
  }
}

#[test]
fn a_twelve_step_spinner_draws_only_its_steps_whatever_its_render_cost() {
  let mut shell = Shell::new(spinner(steps(12)));
  shell.set_render_cost(Duration::from_millis(3));
  shell.run_for(Duration::from_secs(1));

  let frames = shell.frames().len();
  assert!(
    (11..=14).contains(&frames),
    "12 steps a second draw 12 frames a second: {frames} in 1 s"
  );
}

#[test]
fn a_minimized_window_draws_no_perf_overlay_frames() {
  let mut tree = Tree::new();
  tree.set_root(Row::new().child(Rect::new(40.0, 40.0).background("#2563eb")));
  tree.draw_perf_overlay();
  let mut shell = Shell::new(tree);
  shell.run_for(Duration::from_millis(100));

  shell.tree.window().set_minimized(true);
  let frames_before = shell.frames().len();
  // Wake the loop every 50 ms, as events reach a minimized window (an MCP call, a wake from another thread),
  // across the overlay's 1 s sample.
  for _ in 0..24 {
    shell.run_for(Duration::from_millis(50));
  }
  assert_eq!(
    shell.frames().len(),
    frames_before,
    "the overlay's samples draw nothing while the window is minimized"
  );
}

#[test]
fn moving_the_window_reads_its_refresh_rate_again() {
  let mut app = App::new();
  let mut tree = spinner(Easing::Linear);
  tree.pass_headless(&mut app);
  let mut pacing = FramePacing::default();
  let mut reads = 0;
  let mut request = |pacing: &mut FramePacing, tree: &mut Tree| {
    pacing.request_frame(
      tree,
      || {
        reads += 1;
        REFRESH
      },
      Instant::now(),
    );
  };

  request(&mut pacing, &mut tree);
  request(&mut pacing, &mut tree);
  // Another display may refresh at another rate.
  pacing.handle_event(&WindowEvent::Moved(PhysicalPosition::new(1920, 0)));
  request(&mut pacing, &mut tree);
  assert_eq!(reads, 2, "the rate is read once, then again when the window moves");
}

/// A FIFO swapchain of frame latency one on a virtual clock that starts at `epoch` on a vblank: when each of
/// `count` frames is presented, for a pass that works `cost` before it asks the swapchain for a buffer and as long
/// again after, and a loop that wakes up to 1 ms late. `changes` gives the timeline's next change after a pass that
/// sampled it at a time.
fn virtual_presents(
  epoch: Instant,
  refresh: Duration,
  cost: Duration,
  count: usize,
  changes: impl Fn(Instant) -> Instant,
) -> Vec<Instant> {
  let vblank_after = |at: Instant| {
    let refreshes = vblank_index(epoch, refresh, at);
    epoch + refresh * u32::try_from(refreshes).expect("a short simulation")
  };
  let mut seed = 0x2545_f491_4f6c_dd1d_u64;
  let mut wake_latency = || {
    seed = seed
      .wrapping_mul(6_364_136_223_846_793_005)
      .wrapping_add(1_442_695_040_888_963_407);
    Duration::from_micros((seed >> 33) % 1000)
  };
  let mut now = epoch;
  let mut last: Option<PresentedFrame> = None;
  let mut presents: Vec<Instant> = Vec::with_capacity(count);
  while presents.len() < count {
    if let Some(frame) = last {
      now = now.max(frame_due(changes(frame.started), last, refresh)) + wake_latency();
    }
    let started = now;
    now += cost;
    if let Some(previous) = presents.last() {
      now = now.max(vblank_after(*previous));
    }
    now += cost;
    presents.push(now);
    last = Some(PresentedFrame {
      started,
      presented: now,
    });
  }
  presents
}

#[test]
fn on_a_virtual_clock_continuous_frames_take_every_vblank() {
  let cases = [(REFRESH, [0, 1000, 2000, 3000]), (REFRESH / 2, [0, 500, 1000, 1500])];
  for (refresh, costs_us) in cases {
    for cost in costs_us.map(Duration::from_micros) {
      // A continuous animation changes at once: its next change is the time its last pass sampled it.
      let epoch = Instant::now();
      let presents = virtual_presents(epoch, refresh, cost / 2, 600, |sampled| sampled);
      let vblanks: Vec<u128> = presents
        .iter()
        .map(|present| vblank_index(epoch, refresh, *present))
        .collect();
      assert_eq!(
        skipped_vblanks(&vblanks[1..]),
        0,
        "at {refresh:?} a refresh and {cost:?} of work per frame, every vblank shows a new frame"
      );
    }
  }
}

#[test]
fn on_a_virtual_clock_continuous_frames_take_every_vblank_at_144_hz() {
  let refresh = Duration::from_nanos(6_944_444);
  for cost_us in [2000, 2250, 2500] {
    let cost = Duration::from_micros(cost_us);
    let epoch = Instant::now();
    let presents = virtual_presents(epoch, refresh, cost / 2, 1200, |sampled| sampled);
    let vblanks: Vec<u128> = presents
      .iter()
      .map(|present| vblank_index(epoch, refresh, *present))
      .collect();
    assert_eq!(
      skipped_vblanks(&vblanks[1..]),
      0,
      "at 144 Hz and {cost:?} of work per frame, every vblank shows a new frame"
    );
  }
}

#[test]
fn on_a_virtual_clock_twelve_steps_draw_twelve_frames_a_second() {
  let step = Duration::from_secs(1) / 12;
  let epoch = Instant::now();
  let next_step = |sampled: Instant| {
    let steps = (sampled - epoch).as_nanos() / step.as_nanos() + 1;
    epoch + step * u32::try_from(steps).expect("a short simulation") + Duration::from_micros(100)
  };
  let presents = virtual_presents(epoch, REFRESH, Duration::from_millis(1), 40, next_step);
  let in_first_second = presents
    .iter()
    .filter(|present| **present - epoch < Duration::from_secs(1))
    .count();
  assert!(
    (12..=13).contains(&in_first_second),
    "a frame per step: {in_first_second} frames in the first second"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn a_middle_button_autoscroll_moves_the_content_at_every_refresh_and_then_lets_the_loop_wait() {
  use crate::{
    components::{Column, ScrollVertical},
    layout::layout_kind::ScrollState,
  };

  let state = ScrollState::new();
  let mut rows = Column::new().spacing(0.0);
  for _ in 0..200 {
    rows = rows.child(Rect::new(150.0, 20.0));
  }
  let mut tree = Tree::new();
  tree.set_root(
    ScrollVertical::new(rows)
      .with_scroll_state(state.clone())
      .middle_button_autoscroll(true)
      .size(200.0, 100.0),
  );
  let mut shell = Shell::new(tree);
  shell.run_for(Duration::from_millis(50));
  let before = shell.frames().len();
  shell.tree.mouse_down(80.0, 30.0, MouseButton::Middle);
  shell.tree.mouse_move(80.0, 90.0);
  shell.run_for(Duration::from_millis(300));
  let scrolled = state.scroll_y();
  let frames = shell.frames().len() - before;
  assert!(
    (14..=20).contains(&frames),
    "the autoscroll runs at the display's rate: {frames} frames in 0.3 s"
  );
  assert!(scrolled > 50.0, "and moves the content: {scrolled}");

  shell.tree.mouse_up(80.0, 90.0, MouseButton::Middle);
  shell.run_for(Duration::from_millis(100));
  assert_eq!(state.scroll_y(), scrolled, "the release stops it");
  assert!(!shell.tree.has_active_tick_sources());
  let needs = LoopNeeds {
    polling: shell.tree.needs_polling(),
    next_redraw: shell.tree.next_scheduled_redraw(),
    next_timer: shell.tree.next_timer_at(),
    ..LoopNeeds::default()
  };
  assert_eq!(LoopWait::of(needs, Instant::now()), LoopWait::Wait, "the loop waits");
}

const LEAVE: KeyframesId = KeyframesId::new(42);
const PANEL: &str = "#dc2626";

/// The open state of a popup and the keyframes its content leaves with.
#[derive(Clone)]
struct Leaver {
  open: crate::core::Signal<bool>,
  exit: KeyframesId,
}

impl PartialEq for Leaver {
  fn eq(&self, other: &Self) -> bool {
    self.exit == other.exit
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for Leaver {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

/// A square that closes the popup beside it when clicked; the popup's content has an exit animation of 150 ms.
struct LeavingPopup {
  anchor: crate::core::ElementRef,
}

impl Component for LeavingPopup {
  type Props = Leaver;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      anchor: crate::core::ElementRef::new(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Leaver>().clone();
    let close = props.open.clone();
    let content = Rect::new(60.0, 30.0)
      .background(PANEL)
      .exit_animation(Animation::new(props.exit).duration_ms(150).linear());
    Row::new()
      .child(Rect::new(40.0, 40.0).on_click(move |_| close.set(false)))
      .child(Rect::new(40.0, 40.0).ref_element(self.anchor.clone()))
      .child(
        crate::components::Popup::new(self.anchor.clone(), content)
          .open(props.open)
          .dismiss_on_outside_click(false),
      )
  }
}

fn panels_painted(tree: &Tree) -> usize {
  let panel = crate::node::color::Color::from_hex(PANEL);
  tree
    .resolve_quads(tree.last_layout().expect("a pass ran"))
    .iter()
    .filter(|quad| matches!(&quad.content, crate::layout::quad::QuadContent::Rect { color, .. } if *color == panel))
    .count()
}

/// Opens the popup, lets the loop settle, clicks it closed and runs the loop past the exit animation's end.
/// Returns the shell and how many frames the loop drew from the click on.
fn close_a_popup_that_leaves(exit: KeyframesId) -> (Shell, usize) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.register_keyframes(
    Keyframes::new(LEAVE)
      .frame(0.0, |f| {
        f.set(AnimatableProperty::Opacity, 1.0);
      })
      .frame(1.0, |f| {
        f.set(AnimatableProperty::Opacity, 0.0);
      }),
  );
  tree.mount_root::<LeavingPopup>(
    &mut app,
    Leaver {
      open: crate::core::Signal::new(true),
      exit,
    },
  );
  let mut shell = Shell::new(tree);
  shell.run_for(Duration::from_millis(100));
  assert_eq!(panels_painted(&shell.tree), 1, "the popup is open");
  assert!(!shell.tree.has_active_timeline());

  let before = shell.frames().len();
  shell.clicks.push_back(Instant::now() + Duration::from_millis(10));
  shell.run_for(Duration::from_millis(60));
  assert_eq!(panels_painted(&shell.tree), 1, "closed, it is still painted");
  shell.run_for(Duration::from_millis(340));
  let frames = shell.frames().len() - before;
  (shell, frames)
}

fn assert_the_loop_waits(shell: &Shell) {
  assert_eq!(panels_painted(&shell.tree), 0, "the layer is dropped");
  assert!(!shell.tree.has_active_timeline());
  assert!(!shell.tree.needs_redraw());
  let needs = LoopNeeds {
    polling: shell.tree.needs_polling(),
    next_redraw: shell.tree.next_scheduled_redraw(),
    next_timer: shell.tree.next_timer_at(),
    ..LoopNeeds::default()
  };
  assert_eq!(
    LoopWait::of(needs, Instant::now()),
    LoopWait::Wait,
    "nothing is left to wake for"
  );
}

#[test]
fn the_loop_draws_a_leaving_layer_out_and_then_waits() {
  let (shell, frames) = close_a_popup_that_leaves(LEAVE);

  assert!(
    (6..=14).contains(&frames),
    "0.15 s of a fade at the display's rate: {frames} frames"
  );
  assert_the_loop_waits(&shell);
}

#[test]
fn the_loop_wakes_to_drop_a_leaving_layer_whose_animation_draws_nothing() {
  // Keyframes nobody registered: no run paces a frame, and the layer's own end still wakes the loop.
  let (shell, frames) = close_a_popup_that_leaves(KeyframesId::new(43));

  assert!(frames <= 4, "the close and the drop: {frames} frames");
  assert_the_loop_waits(&shell);
}
