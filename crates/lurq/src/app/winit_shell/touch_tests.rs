//! What one finger comes to: a pointer where nothing scrolls, and over content that scrolls a tap, a scroll with
//! its momentum or a long press.

use std::time::{Duration, Instant};

use winit::event::TouchPhase;

use super::touch::{LONG_PRESS, PRESS_DELAY, TouchAction, TouchPointer};
use crate::app::{events::ScrollPhase, runtime::TouchMomentum};

const SCALE: f64 = 3.0;
const FINGER: u64 = 7;
const START: (f64, f64) = (100.0, 400.0);
const PRESS: TouchAction = TouchAction::Press { x: 100.0, y: 400.0 };
const RELEASE: TouchAction = TouchAction::Release { x: 100.0, y: 400.0 };
const TAP: TouchAction = TouchAction::Tap { x: 100.0, y: 400.0 };
const LONG: TouchAction = TouchAction::LongPress { x: 100.0, y: 400.0 };
/// The gesture a finger going up from [`START`] starts.
const TARGET_UP: TouchAction = TouchAction::ScrollTarget {
  x: 100.0,
  y: 400.0,
  dx: 0.0,
  dy: -40.0,
};

fn ms(millis: u64) -> Duration {
  Duration::from_millis(millis)
}

/// One event of the finger over content that scrolls and is at rest.
fn touch(pointer: &mut TouchPointer, phase: TouchPhase, position: (f64, f64), now: Instant) -> Vec<TouchAction> {
  pointer.touch(FINGER, phase, position, now, SCALE, false, |_| true)
}

fn scroll(dx: f64, dy: f64, phase: ScrollPhase) -> TouchAction {
  TouchAction::Scroll {
    x: START.0,
    y: START.1,
    dx,
    dy,
    phase,
  }
}

/// A finger that lands at [`START`] and moves up by `step` every 10 ms, `steps` times. Gives the time of the
/// last move and where it ended.
fn drag_up(pointer: &mut TouchPointer, start: Instant, step: f64, steps: u32) -> (Instant, (f64, f64)) {
  touch(pointer, TouchPhase::Started, START, start);
  let mut now = start;
  let mut position = START;
  for _ in 0..steps {
    now += ms(10);
    position.1 -= step;
    touch(pointer, TouchPhase::Moved, position, now);
  }
  (now, position)
}

fn total_scroll(actions: &[TouchAction]) -> f64 {
  actions
    .iter()
    .map(|action| match action {
      TouchAction::Scroll { dy, .. } => *dy,
      _ => 0.0,
    })
    .sum()
}

#[test]
fn where_nothing_scrolls_a_finger_is_the_pointer_with_its_button_down() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  let mut event = |phase, position, at| pointer.touch(FINGER, phase, position, at, SCALE, false, |_| false);

  let down = event(TouchPhase::Started, START, start);
  let moved = event(TouchPhase::Moved, (100.0, 200.0), start + ms(20));
  let up = event(TouchPhase::Ended, (100.0, 200.0), start + ms(900));

  assert_eq!(down, [PRESS]);
  assert_eq!(moved, [TouchAction::Move { x: 100.0, y: 200.0 }]);
  assert_eq!(up, [TouchAction::Release { x: 100.0, y: 200.0 }]);
  assert_eq!(pointer.tick(start + LONG_PRESS), []);
  assert_eq!(pointer.next_deadline(), None);
}

#[test]
fn a_finger_that_rests_where_nothing_scrolls_is_a_long_press_unless_it_travelled() {
  let start = Instant::now();
  let event = |pointer: &mut TouchPointer, phase, position, at| {
    pointer.touch(FINGER, phase, position, at, SCALE, false, |_| false)
  };

  let mut resting = TouchPointer::default();
  event(&mut resting, TouchPhase::Started, START, start);
  assert_eq!(resting.next_deadline(), Some(start + LONG_PRESS));
  // Within the slop: still resting.
  event(&mut resting, TouchPhase::Moved, (110.0, 410.0), start + ms(100));
  assert_eq!(resting.tick(start + LONG_PRESS - ms(1)), []);
  assert_eq!(resting.tick(start + LONG_PRESS), [LONG]);
  assert_eq!(resting.tick(start + LONG_PRESS + ms(8)), []);
  assert_eq!(resting.next_deadline(), None);

  let mut travelling = TouchPointer::default();
  event(&mut travelling, TouchPhase::Started, START, start);
  event(&mut travelling, TouchPhase::Moved, (100.0, 300.0), start + ms(100));
  assert_eq!(travelling.next_deadline(), None);
  assert_eq!(travelling.tick(start + LONG_PRESS), []);
}

#[test]
fn a_long_press_a_handler_took_clicks_nothing() {
  let start = Instant::now();
  let event = |pointer: &mut TouchPointer, phase, position, at| {
    pointer.touch(FINGER, phase, position, at, SCALE, false, |_| false)
  };

  let mut taken = TouchPointer::default();
  event(&mut taken, TouchPhase::Started, START, start);
  taken.tick(start + LONG_PRESS);
  taken.long_press_taken();
  let up = event(&mut taken, TouchPhase::Ended, START, start + ms(700));
  assert_eq!(up, [TouchAction::CancelPress { x: 100.0, y: 400.0 }]);

  let mut left = TouchPointer::default();
  event(&mut left, TouchPhase::Started, START, start);
  left.tick(start + LONG_PRESS);
  let up = event(&mut left, TouchPhase::Ended, START, start + ms(700));
  assert_eq!(up, [RELEASE]);
}

#[test]
fn a_finger_over_a_list_hovers_nothing_and_presses_nothing_at_first() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();

  let down = touch(&mut pointer, TouchPhase::Started, START, start);
  // Within the slop: 10 logical pixels are 30 physical ones at this scale.
  let moved = touch(&mut pointer, TouchPhase::Moved, (110.0, 420.0), start + ms(30));

  assert_eq!(down, []);
  assert_eq!(moved, []);
}

#[test]
fn a_quick_tap_is_a_tap_at_once_where_it_landed() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);

  let up = touch(&mut pointer, TouchPhase::Ended, (110.0, 420.0), start + ms(60));

  assert_eq!(up, [TAP]);
  assert_eq!(pointer.next_deadline(), None);
}

#[test]
fn two_quick_taps_are_two_taps() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);
  let first = touch(&mut pointer, TouchPhase::Ended, START, start + ms(40));

  let down = touch(&mut pointer, TouchPhase::Started, START, start + ms(70));
  let second = touch(&mut pointer, TouchPhase::Ended, START, start + ms(110));

  assert_eq!((first, down, second), (vec![TAP], vec![], vec![TAP]));
}

#[test]
fn a_finger_that_rests_presses_before_it_lifts() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);

  assert_eq!(pointer.next_deadline(), Some(start + PRESS_DELAY));
  assert_eq!(pointer.tick(start + PRESS_DELAY - ms(1)), []);
  let pressed = pointer.tick(start + PRESS_DELAY);
  let again = pointer.tick(start + PRESS_DELAY + ms(8));
  let up = touch(&mut pointer, TouchPhase::Ended, START, start + ms(300));

  assert_eq!(pressed, [PRESS]);
  assert_eq!(again, []);
  assert_eq!(up, [RELEASE]);
  assert_eq!(pointer.next_deadline(), None);
}

#[test]
fn a_finger_that_moves_past_the_slop_scrolls_from_where_the_content_is() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);

  let first = touch(&mut pointer, TouchPhase::Moved, (100.0, 360.0), start + ms(10));
  let second = touch(&mut pointer, TouchPhase::Moved, (104.0, 340.0), start + ms(20));

  // The 40 that told the scroll from a tap pick the gesture's way and move nothing.
  assert_eq!(first, [TARGET_UP, scroll(0.0, 0.0, ScrollPhase::Start)]);
  assert_eq!(second, [scroll(4.0, -20.0, ScrollPhase::Scroll)]);
}

#[test]
fn a_scroll_that_starts_after_the_press_ends_the_press_without_a_click() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);
  pointer.tick(start + PRESS_DELAY);

  let moved = touch(&mut pointer, TouchPhase::Moved, (100.0, 360.0), start + ms(200));

  assert_eq!(
    moved,
    [
      TouchAction::CancelPress { x: 100.0, y: 400.0 },
      TARGET_UP,
      scroll(0.0, 0.0, ScrollPhase::Start)
    ]
  );
}

#[test]
fn a_long_press_keeps_the_pointer_from_scrolling() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);
  pointer.tick(start + PRESS_DELAY);

  assert_eq!(pointer.next_deadline(), Some(start + LONG_PRESS));
  assert_eq!(pointer.tick(start + LONG_PRESS), [LONG]);
  assert_eq!(pointer.next_deadline(), None);
  let moved = touch(
    &mut pointer,
    TouchPhase::Moved,
    (100.0, 300.0),
    start + LONG_PRESS + ms(50),
  );
  let up = touch(
    &mut pointer,
    TouchPhase::Ended,
    (100.0, 300.0),
    start + LONG_PRESS + ms(80),
  );

  assert_eq!(moved, [TouchAction::Move { x: 100.0, y: 300.0 }]);
  assert_eq!(up, [TouchAction::Release { x: 100.0, y: 300.0 }]);
}

#[test]
fn a_long_press_no_turn_ran_during_still_presses() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);

  assert_eq!(pointer.tick(start + LONG_PRESS + ms(40)), [PRESS, LONG]);
  let moved = touch(
    &mut pointer,
    TouchPhase::Moved,
    (100.0, 300.0),
    start + LONG_PRESS + ms(50),
  );
  assert_eq!(moved, [TouchAction::Move { x: 100.0, y: 300.0 }]);
}

#[test]
fn a_finger_that_stops_before_it_lifts_leaves_no_momentum() {
  let mut pointer = TouchPointer::default();
  let (last_move, position) = drag_up(&mut pointer, Instant::now(), 40.0, 5);

  let up = touch(&mut pointer, TouchPhase::Ended, position, last_move + ms(300));

  assert_eq!(up, [scroll(0.0, 0.0, ScrollPhase::End)]);
  assert_eq!(pointer.next_deadline(), None);
}

#[test]
fn a_flick_keeps_scrolling_the_way_it_went_and_comes_to_rest() {
  let mut pointer = TouchPointer::default();
  let (last_move, position) = drag_up(&mut pointer, Instant::now(), 40.0, 5);

  let up = touch(&mut pointer, TouchPhase::Ended, position, last_move + ms(5));
  assert_eq!(
    up,
    [scroll(0.0, 0.0, ScrollPhase::End), scroll(0.0, 0.0, ScrollPhase::Start)]
  );

  let mut now = last_move + ms(5);
  let mut momentum = Vec::new();
  let mut steps = Vec::new();
  while let Some(deadline) = pointer.next_deadline() {
    assert!(deadline > now, "momentum asks for a later turn");
    // A 120 Hz display's frame.
    now += ms(8);
    let actions = pointer.tick(now);
    steps.push(total_scroll(&actions));
    momentum.extend(actions);
    assert!(now < last_move + ms(10_000), "momentum ends");
  }

  assert!(steps.iter().all(|step| *step < 0.0), "every step goes up: {steps:?}");
  assert!(
    steps.windows(2).all(|pair| pair[1].abs() < pair[0].abs()),
    "every step is shorter than the one before"
  );
  assert_eq!(momentum.last(), Some(&scroll(0.0, 0.0, ScrollPhase::End)));
  // The finger moved 160 px in 45 ms: momentum carries the content well beyond one more such stretch.
  assert!(total_scroll(&momentum) < -500.0, "{}", total_scroll(&momentum));
}

#[test]
fn momentum_that_moves_nothing_is_ended_with_the_velocity_it_had() {
  let mut pointer = TouchPointer::default();
  let (last_move, position) = drag_up(&mut pointer, Instant::now(), 40.0, 5);
  touch(&mut pointer, TouchPhase::Ended, position, last_move + ms(5));
  assert!(pointer.next_deadline().is_some());

  let (end, velocity) = pointer.end_fling().expect("momentum to end");
  assert_eq!(end, scroll(0.0, 0.0, ScrollPhase::End));
  assert!(velocity.0 == 0.0 && velocity.1 < 0.0, "it was going up: {velocity:?}");
  assert_eq!(pointer.end_fling(), None);
  assert_eq!(pointer.next_deadline(), None);
  // The next touch is a tap again, not one that catches momentum.
  let now = last_move + ms(50);
  touch(&mut pointer, TouchPhase::Started, START, now);
  assert_eq!(touch(&mut pointer, TouchPhase::Ended, START, now + ms(30)), [TAP]);
}

#[test]
fn a_touch_during_momentum_stops_it_and_presses_nothing() {
  let mut pointer = TouchPointer::default();
  let (last_move, position) = drag_up(&mut pointer, Instant::now(), 40.0, 5);
  let lifted = last_move + ms(5);
  touch(&mut pointer, TouchPhase::Ended, position, lifted);
  pointer.tick(lifted + ms(8));

  let down = touch(&mut pointer, TouchPhase::Started, (200.0, 300.0), lifted + ms(16));
  let rested = pointer.tick(lifted + ms(16) + LONG_PRESS);
  let up = touch(
    &mut pointer,
    TouchPhase::Ended,
    (200.0, 300.0),
    lifted + ms(40) + LONG_PRESS,
  );

  assert_eq!(down, [scroll(0.0, 0.0, ScrollPhase::End)]);
  assert_eq!(rested, []);
  assert_eq!(up, [TouchAction::Settle]);
  assert_eq!(pointer.next_deadline(), None);
}

#[test]
fn a_finger_that_catches_moving_content_scrolls_on_with_its_first_move() {
  let mut pointer = TouchPointer::default();
  let (last_move, position) = drag_up(&mut pointer, Instant::now(), 40.0, 5);
  let lifted = last_move + ms(5);
  touch(&mut pointer, TouchPhase::Ended, position, lifted);
  touch(&mut pointer, TouchPhase::Started, START, lifted + ms(16));

  // Three physical pixels: far within the slop of a touch on content at rest.
  let moved = touch(&mut pointer, TouchPhase::Moved, (100.0, 397.0), lifted + ms(24));

  assert_eq!(
    moved,
    [
      TouchAction::ScrollTarget {
        x: 100.0,
        y: 400.0,
        dx: 0.0,
        dy: -3.0
      },
      scroll(0.0, 0.0, ScrollPhase::Start)
    ]
  );
}

#[test]
fn a_finger_that_lands_on_content_springing_back_holds_it_and_lets_it_go() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();

  let down = pointer.touch(FINGER, TouchPhase::Started, START, start, SCALE, true, |_| true);
  let rested = pointer.tick(start + LONG_PRESS);
  let up = pointer.touch(FINGER, TouchPhase::Ended, START, start + ms(700), SCALE, false, |_| {
    true
  });

  assert_eq!(down, []);
  assert_eq!(rested, [], "it presses nothing");
  assert_eq!(up, [TouchAction::Settle]);
}

#[test]
fn a_cancelled_touch_clicks_nothing() {
  let start = Instant::now();

  let mut resting = TouchPointer::default();
  touch(&mut resting, TouchPhase::Started, START, start);
  let cancelled = touch(&mut resting, TouchPhase::Cancelled, START, start + ms(20));
  assert_eq!(cancelled, []);
  assert_eq!(resting.next_deadline(), None);

  let mut pressing = TouchPointer::default();
  touch(&mut pressing, TouchPhase::Started, START, start);
  pressing.tick(start + PRESS_DELAY);
  let cancelled = touch(&mut pressing, TouchPhase::Cancelled, (120.0, 400.0), start + ms(200));
  assert_eq!(cancelled, [TouchAction::CancelPress { x: 120.0, y: 400.0 }]);

  let mut scrolling = TouchPointer::default();
  let (last_move, position) = drag_up(&mut scrolling, start, 40.0, 5);
  let cancelled = touch(&mut scrolling, TouchPhase::Cancelled, position, last_move + ms(5));
  assert_eq!(cancelled, [scroll(0.0, 0.0, ScrollPhase::End)]);
  assert_eq!(scrolling.next_deadline(), None);
}

#[test]
fn a_second_finger_is_ignored_before_a_scroll_starts() {
  let mut pointer = TouchPointer::default();
  let start = Instant::now();
  touch(&mut pointer, TouchPhase::Started, START, start);

  let mut second = |phase, position, at| pointer.touch(8, phase, position, at, SCALE, false, |_| true);
  let down = second(TouchPhase::Started, (300.0, 300.0), start + ms(10));
  let moved = second(TouchPhase::Moved, (300.0, 100.0), start + ms(20));
  let up = second(TouchPhase::Ended, (300.0, 100.0), start + ms(30));
  let first_up = touch(&mut pointer, TouchPhase::Ended, START, start + ms(40));

  assert_eq!((down, moved, up), (vec![], vec![], vec![]));
  assert_eq!(first_up, [TAP]);
}

#[test]
fn a_second_finger_joins_a_scroll_and_carries_it_on_when_the_first_lifts() {
  let mut pointer = TouchPointer::default();
  let (last_move, first_at) = drag_up(&mut pointer, Instant::now(), 40.0, 3);
  let mut second = |phase, position, at| pointer.touch(8, phase, position, at, SCALE, false, |_| true);

  // It lands elsewhere: the content stays where it is.
  let joined = second(TouchPhase::Started, (300.0, 600.0), last_move + ms(10));
  assert_eq!(joined, []);
  // It moves 60 up while the first stays: the middle of the two moves 30.
  let moved = second(TouchPhase::Moved, (300.0, 540.0), last_move + ms(20));
  assert_eq!(moved, [scroll(0.0, -30.0, ScrollPhase::Scroll)]);

  // The first lifts: nothing ends, and the second scrolls alone, by all of its travel.
  let lifted = touch(&mut pointer, TouchPhase::Ended, first_at, last_move + ms(30));
  assert_eq!(lifted, []);
  let mut second = |phase, position, at| pointer.touch(8, phase, position, at, SCALE, false, |_| true);
  let alone = second(TouchPhase::Moved, (300.0, 500.0), last_move + ms(40));
  assert_eq!(alone, [scroll(0.0, -40.0, ScrollPhase::Scroll)]);

  // The last finger lifts after a rest: the scroll ends.
  let ended = second(TouchPhase::Ended, (300.0, 500.0), last_move + ms(400));
  assert_eq!(ended, [scroll(0.0, 0.0, ScrollPhase::End)]);
  assert_eq!(pointer.next_deadline(), None);
}

/// How far momentum carries the content after the same flick.
fn momentum_distance(momentum: TouchMomentum) -> f64 {
  let mut pointer = TouchPointer::default();
  pointer.set_momentum(momentum);
  let (last_move, position) = drag_up(&mut pointer, Instant::now(), 40.0, 5);
  touch(&mut pointer, TouchPhase::Ended, position, last_move + ms(5));
  let mut now = last_move + ms(5);
  let mut distance = 0.0;
  while pointer.next_deadline().is_some() {
    now += ms(8);
    distance += total_scroll(&pointer.tick(now));
    assert!(now < last_move + ms(20_000), "momentum ends");
  }
  -distance
}

#[test]
fn the_apps_momentum_decides_how_far_a_flick_goes() {
  let system = momentum_distance(TouchMomentum::default());
  let fast = momentum_distance(TouchMomentum {
    deceleration_rate: 0.99,
    ..TouchMomentum::default()
  });
  let capped = momentum_distance(TouchMomentum {
    max_speed: 300.0,
    ..TouchMomentum::default()
  });
  let none = momentum_distance(TouchMomentum {
    min_speed: 100_000.0,
    ..TouchMomentum::default()
  });

  // The fast rate keeps a hundredth less of its speed each millisecond: about a fifth of the distance.
  assert!(fast > system * 0.15 && fast < system * 0.25, "{system} then {fast}");
  assert!(capped < system * 0.5, "{system} then {capped}");
  assert_eq!(none, 0.0);
}

#[test]
fn a_pointer_given_up_to_a_gesture_ends_what_it_was_doing_without_a_click() {
  let start = Instant::now();

  let mut resting = TouchPointer::default();
  touch(&mut resting, TouchPhase::Started, START, start);
  assert_eq!(resting.abandon(), []);
  assert_eq!(
    touch(&mut resting, TouchPhase::Ended, START, start + ms(40)),
    [],
    "no tap follows"
  );

  let mut pressing = TouchPointer::default();
  touch(&mut pressing, TouchPhase::Started, START, start);
  pressing.tick(start + PRESS_DELAY);
  assert_eq!(pressing.abandon(), [TouchAction::CancelPress { x: 100.0, y: 400.0 }]);

  let mut scrolling = TouchPointer::default();
  let (last_move, position) = drag_up(&mut scrolling, start, 40.0, 5);
  assert_eq!(scrolling.abandon(), [scroll(0.0, 0.0, ScrollPhase::End)]);
  assert_eq!(
    touch(&mut scrolling, TouchPhase::Ended, position, last_move + ms(5)),
    []
  );
  assert_eq!(scrolling.next_deadline(), None, "and no momentum");
}
