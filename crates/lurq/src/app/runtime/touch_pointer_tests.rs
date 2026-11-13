//! A finger's own events, and the pointer it stands for.

use std::{
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use super::{HeadlessSurface, TouchRoute, Tree, hit_test_tree};
use crate::{
  app::{
    App,
    events::{
      EdgePanEvent, GesturePhase, HistoryDirection, HistorySwipeEvent, LongPressEvent, PanEvent, PinchEvent,
      RotateEvent, ScrollPhase, SwipeEvent, TapEvent, TouchEvent, TouchPhase,
    },
  },
  components::{Column, Rect, ScrollVertical},
};

type Log = Arc<Mutex<Vec<String>>>;

fn logged(log: &Log) -> Vec<String> {
  std::mem::take(&mut *log.lock().unwrap())
}

/// At scale 2: a 100 by 100 button at the top left that logs what it is sent, in a column 300 high that logs
/// its touches too.
fn button() -> (Tree, App, Log) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_scale_factor(2.0);
  tree.resize(600, 600);
  let log = Log::default();
  let push = |log: &Log, line: String| log.lock().unwrap().push(line);
  let (start, moved, end, cancel, outer) = (log.clone(), log.clone(), log.clone(), log.clone(), log.clone());
  let (click, down, up, enter, long) = (log.clone(), log.clone(), log.clone(), log.clone(), log.clone());
  tree.set_root(
    Column::new()
      .child(
        Rect::new(100.0, 100.0)
          .on_touch_start(move |event: TouchEvent| {
            push(&start, format!("start {} at {},{}", event.id, event.x, event.y))
          })
          .on_touch_move(move |event: TouchEvent| push(&moved, format!("move {} at {},{}", event.id, event.x, event.y)))
          .on_touch_end(move |event: TouchEvent| push(&end, format!("end {}", event.id)))
          .on_touch_cancel(move |event: TouchEvent| push(&cancel, format!("cancel {}", event.id)))
          .on_long_press(move |event: LongPressEvent| push(&long, format!("long press at {},{}", event.x, event.y)))
          .on_mouse_enter(move || push(&enter, "enter".to_owned()))
          .on_mouse_down(move |_| push(&down, "down".to_owned()))
          .on_mouse_up(move |_| push(&up, "up".to_owned()))
          .on_click(move |_| push(&click, "click".to_owned())),
      )
      .on_touch_start(move |event: TouchEvent| push(&outer, format!("outer start {}", event.id)))
      .size(300.0, 300.0),
  );
  tree.pass(&mut app, &HeadlessSurface);
  (tree, app, log)
}

/// Whether the button shows as hovered and as pressed.
fn looks(tree: &Tree) -> (bool, bool) {
  let (root, layout) = (tree.root.as_ref().unwrap(), tree.last_layout.as_ref().unwrap());
  let mut hits = Vec::new();
  hit_test_tree(root, layout, 0.0, 0.0, 50.0, 50.0, &mut hits);
  let button = hits.first().unwrap().0;
  (button.style_state.is_hovered(), button.style_state.is_active())
}

#[test]
fn a_fingers_events_go_to_what_it_landed_on_and_bubble() {
  let (mut tree, _app, log) = button();

  assert_eq!(
    tree.touch_event(7, TouchPhase::Start, 100.0, 100.0, None),
    TouchRoute::Pointer
  );
  // It moves off the button and out of the column: the events are still the button's.
  tree.touch_event(7, TouchPhase::Move, 500.0, 500.0, None);
  tree.touch_event(7, TouchPhase::End, 500.0, 500.0, None);

  assert_eq!(
    logged(&log),
    ["start 7 at 50,50", "outer start 7", "move 7 at 250,250", "end 7"]
  );
}

#[test]
fn two_fingers_are_told_apart() {
  let (mut tree, _app, log) = button();

  tree.touch_event(1, TouchPhase::Start, 40.0, 40.0, None);
  // The second lands beside the button, on the column.
  tree.touch_event(2, TouchPhase::Start, 400.0, 400.0, None);
  tree.touch_event(1, TouchPhase::Cancel, 40.0, 40.0, None);
  tree.touch_event(2, TouchPhase::End, 400.0, 400.0, None);
  // A finger that is no longer down sends nothing.
  tree.touch_event(1, TouchPhase::Move, 40.0, 40.0, None);

  assert_eq!(
    logged(&log),
    ["start 1 at 20,20", "outer start 1", "outer start 2", "cancel 1"]
  );
}

#[test]
fn a_touch_start_that_prevents_the_default_takes_the_finger() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(300, 300);
  tree.set_root(Rect::new(100.0, 100.0).on_touch_start(|event: TouchEvent| event.prevent_default()));
  tree.pass(&mut app, &HeadlessSurface);

  assert_eq!(
    tree.touch_event(7, TouchPhase::Start, 50.0, 50.0, None),
    TouchRoute::Taken
  );
  assert_eq!(
    tree.touch_event(7, TouchPhase::Move, 60.0, 50.0, None),
    TouchRoute::Taken,
    "for the whole touch"
  );
  assert_eq!(
    tree.touch_event(7, TouchPhase::End, 60.0, 50.0, None),
    TouchRoute::Taken
  );
  assert_eq!(
    tree.touch_event(8, TouchPhase::Start, 200.0, 200.0, None),
    TouchRoute::Pointer,
    "beside it"
  );
}

#[test]
fn a_long_press_goes_to_what_is_under_the_finger_and_tells_whether_it_was_taken() {
  let (mut tree, _app, log) = button();

  assert!(tree.touch_long_press(100.0, 100.0));
  assert_eq!(logged(&log), ["long press at 50,50"]);
  assert!(!tree.touch_long_press(400.0, 400.0), "nothing there listens");
}

#[test]
fn a_tap_clicks_at_once_and_what_it_tapped_shows_as_pressed_for_a_moment() {
  let (mut tree, _app, log) = button();
  let highlight = Duration::from_millis(80);

  tree.touch_tap(100.0, 100.0, highlight);

  assert_eq!(logged(&log), ["enter", "down", "up", "click"]);
  assert_eq!(looks(&tree), (false, true), "pressed, and not hovered");
  assert!(tree.touch_animating());
  assert!(tree.touch_tick(Instant::now()));
  assert!(!tree.touch_tick(Instant::now() + highlight));
  assert_eq!(looks(&tree), (false, false));
  assert!(!tree.touch_animating());
}

#[test]
fn a_finger_hovers_nothing() {
  let (mut tree, mut app, _log) = button();

  tree.touch_press(100.0, 100.0);
  assert_eq!(looks(&tree), (false, true));
  tree.touch_pointer_move(120.0, 110.0);
  tree.pass(&mut app, &HeadlessSurface);
  assert_eq!(looks(&tree), (false, true));
  tree.touch_release(120.0, 110.0);
  assert_eq!(looks(&tree), (false, false));

  // A mouse does.
  tree.mouse_move(100.0, 100.0);
  assert_eq!(looks(&tree), (true, false));
}

#[test]
fn what_a_fingers_scroll_brings_to_where_it_pressed_is_not_hovered() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(100, 100);
  let log = Log::default();
  let entered = log.clone();
  tree.set_root(
    ScrollVertical::new(
      Column::new()
        .child(Rect::new(100.0, 60.0))
        .child(Rect::new(100.0, 60.0).on_mouse_enter(move || entered.lock().unwrap().push("enter".to_owned())))
        .child(Rect::new(100.0, 500.0)),
    )
    .size(100.0, 100.0),
  );
  tree.pass(&mut app, &HeadlessSurface);

  // The finger rests on the first row, then travels up with it: the press ends, and the list brings the second
  // row to where the finger landed.
  tree.touch_press(50.0, 30.0);
  tree.touch_cancel_press(50.0, 30.0);
  tree.touch_scroll_begin(50.0, 30.0, 0.0, -60.0);
  tree.touch_scroll(50.0, 30.0, 0.0, -60.0, ScrollPhase::Scroll, false);
  // What lays the window out anew looks for what a pointer that holds still is over.
  tree.resize(100, 120);
  tree.pass(&mut app, &HeadlessSurface);

  let (root, layout) = (tree.root.as_ref().unwrap(), tree.last_layout.as_ref().unwrap());
  let mut hits = Vec::new();
  hit_test_tree(root, layout, 0.0, 0.0, 50.0, 30.0, &mut hits);
  assert!(hits.iter().all(|(node, _)| !node.style_state.is_hovered()));
  assert!(
    logged(&log).is_empty(),
    "the finger is on the first row, wherever that went"
  );
}

#[test]
fn what_a_finger_holds_shows_as_pressed_only_while_the_finger_is_near() {
  let (mut tree, _app, log) = button();
  tree.touch_press(100.0, 100.0);
  logged(&log);

  // 60 logical pixels beside the button: within reach.
  tree.touch_pointer_move(320.0, 100.0);
  assert_eq!(looks(&tree).1, true);
  // 100 beside it: out of reach.
  tree.touch_pointer_move(400.0, 100.0);
  assert_eq!(looks(&tree).1, false);
  // Back within reach.
  tree.touch_pointer_move(320.0, 100.0);
  assert_eq!(looks(&tree).1, true);

  // Lifted there, beside the button, it clicks it: the release is the button's, which the pointer comes
  // back onto.
  tree.touch_release(320.0, 100.0);
  assert_eq!(logged(&log), ["up", "enter", "click"]);
}

#[test]
fn a_finger_lifted_out_of_reach_clicks_nothing() {
  let (mut tree, _app, log) = button();
  tree.touch_press(100.0, 100.0);
  tree.touch_pointer_move(400.0, 100.0);
  logged(&log);

  tree.touch_release(400.0, 100.0);

  assert!(!logged(&log).contains(&"click".to_owned()));
  assert_eq!(looks(&tree), (false, false));
}

#[test]
fn a_cancelled_press_clicks_nothing() {
  let (mut tree, _app, log) = button();
  tree.touch_press(100.0, 100.0);
  logged(&log);

  tree.touch_cancel_press(100.0, 100.0);

  assert!(!logged(&log).contains(&"click".to_owned()));
  assert_eq!(looks(&tree), (false, false));
}

/// A 300 by 300 square that logs its pinches and rotations, at scale 1.
fn canvas() -> (Tree, App, Log) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 400);
  let log = Log::default();
  let (pinches, rotations) = (log.clone(), log.clone());
  tree.set_root(
    Rect::new(300.0, 300.0)
      .on_pinch(move |event: PinchEvent| {
        let phase = match event.phase {
          GesturePhase::Start => "starts",
          GesturePhase::Change => "is",
          GesturePhase::End => "ends",
        };
        pinches.lock().unwrap().push(format!(
          "pinch {phase} {:.2} at {:.0},{:.0}",
          event.scale, event.x, event.y
        ));
      })
      .on_rotate(move |event: RotateEvent| {
        let phase = match event.phase {
          GesturePhase::Start => "starts",
          GesturePhase::Change => "is",
          GesturePhase::End => "ends",
        };
        rotations
          .lock()
          .unwrap()
          .push(format!("rotation {phase} {:.0} degrees", event.rotation.to_degrees()));
      }),
  );
  tree.pass(&mut app, &HeadlessSurface);
  (tree, app, log)
}

#[test]
fn two_fingers_moving_apart_pinch() {
  let (mut tree, _app, log) = canvas();

  // 100 apart, level with each other.
  assert_eq!(
    tree.touch_event(1, TouchPhase::Start, 100.0, 150.0, None),
    TouchRoute::Pointer
  );
  assert_eq!(
    tree.touch_event(2, TouchPhase::Start, 200.0, 150.0, None),
    TouchRoute::Pointer
  );
  // A few pixels are not a pinch yet.
  assert_eq!(
    tree.touch_event(2, TouchPhase::Move, 205.0, 150.0, None),
    TouchRoute::Pointer
  );
  assert_eq!(logged(&log), [] as [&str; 0]);

  // 120 apart: the pinch starts there, at scale 1, and the fingers are the gesture's.
  assert_eq!(
    tree.touch_event(2, TouchPhase::Move, 220.0, 150.0, None),
    TouchRoute::Gesture
  );
  assert_eq!(logged(&log), ["pinch starts 1.00 at 160,150"]);
  // 180 apart: half as far again.
  assert_eq!(
    tree.touch_event(1, TouchPhase::Move, 40.0, 150.0, None),
    TouchRoute::Gesture
  );
  assert_eq!(logged(&log), ["pinch is 1.50 at 130,150"]);

  // One lifts: the pinch ends, and the other finger does not become the pointer.
  assert_eq!(
    tree.touch_event(2, TouchPhase::End, 220.0, 150.0, None),
    TouchRoute::Gesture
  );
  assert_eq!(logged(&log), ["pinch ends 1.50 at 130,150"]);
  assert_eq!(
    tree.touch_event(1, TouchPhase::Move, 60.0, 150.0, None),
    TouchRoute::Gesture
  );
  assert_eq!(
    tree.touch_event(1, TouchPhase::End, 60.0, 150.0, None),
    TouchRoute::Gesture
  );
  assert_eq!(logged(&log), [] as [&str; 0]);
}

#[test]
fn two_fingers_turning_rotate_without_pinching() {
  let (mut tree, _app, log) = canvas();

  // 100 apart about (150, 150), level with each other.
  tree.touch_event(1, TouchPhase::Start, 100.0, 150.0, None);
  tree.touch_event(2, TouchPhase::Start, 200.0, 150.0, None);
  // The second goes a quarter of the way round, down the screen: clockwise.
  let around = |degrees: f32| {
    let angle = degrees.to_radians();
    (150.0 + 50.0 * angle.cos(), 150.0 + 50.0 * angle.sin())
  };
  let mut routes = Vec::new();
  for degrees in [3.0_f32, 10.0, 30.0, 45.0] {
    let (second, first) = (around(degrees), around(degrees + 180.0));
    tree.touch_event(1, TouchPhase::Move, first.0, first.1, None);
    routes.push(tree.touch_event(2, TouchPhase::Move, second.0, second.1, None));
  }
  tree.touch_event(1, TouchPhase::Cancel, 0.0, 0.0, None);

  assert_eq!(
    routes,
    [
      TouchRoute::Pointer,
      TouchRoute::Gesture,
      TouchRoute::Gesture,
      TouchRoute::Gesture
    ]
  );
  let log = logged(&log);
  assert!(
    log.iter().all(|line| line.starts_with("rotation")),
    "the fingers stayed as far apart: {log:?}"
  );
  assert_eq!(log.first().map(String::as_str), Some("rotation starts 0 degrees"));
  // It started once the fingers had turned past the slop, and is told from there.
  assert_eq!(log.last().map(String::as_str), Some("rotation ends 38 degrees"));
}

#[test]
fn two_fingers_on_what_listens_for_no_gesture_are_no_gesture() {
  let (mut tree, _app, _log) = button();

  tree.touch_event(1, TouchPhase::Start, 40.0, 40.0, None);
  tree.touch_event(2, TouchPhase::Start, 160.0, 40.0, None);

  assert_eq!(
    tree.touch_event(2, TouchPhase::Move, 400.0, 40.0, None),
    TouchRoute::Pointer
  );
}

/// A 400 by 400 window at scale 1, filled by a surface that logs taps, swipes, pans, edge pans and the way
/// back. With `takes_pans`, its pan handler takes the fingers.
fn surface(takes_pans: bool) -> (Tree, App, Log) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 400);
  let log = Log::default();
  let push = |log: &Log, line: String| log.lock().unwrap().push(line);
  let (taps, swipes, pans, edges, backs) = (log.clone(), log.clone(), log.clone(), log.clone(), log.clone());
  tree.set_root(
    Rect::new(400.0, 400.0)
      .on_tap(move |event: TapEvent| {
        push(
          &taps,
          format!("tap of {} at {:.0},{:.0}", event.fingers, event.x, event.y),
        )
      })
      .on_swipe(move |event: SwipeEvent| push(&swipes, format!("swipe {:?} of {}", event.direction, event.fingers)))
      .on_pan(move |event: PanEvent| {
        if takes_pans && event.phase == GesturePhase::Start {
          event.prevent_default();
        }
        push(
          &pans,
          format!(
            "pan {:?} of {} by {:.0},{:.0}",
            event.phase, event.fingers, event.translation_x, event.translation_y
          ),
        );
      })
      .on_edge_pan(move |event: EdgePanEvent| {
        push(
          &edges,
          format!("edge {:?} {:?} {:.0} in", event.edge, event.phase, event.translation),
        );
      })
      .on_history_swipe(move |event: HistorySwipeEvent| {
        push(
          &backs,
          match event.direction {
            HistoryDirection::Back => "back".to_owned(),
            HistoryDirection::Forward => "forward".to_owned(),
          },
        );
      }),
  );
  tree.pass(&mut app, &HeadlessSurface);
  (tree, app, log)
}

fn after(start: Instant, millis: u64) -> Instant {
  start + Duration::from_millis(millis)
}

#[test]
fn fingers_that_touch_and_lift_without_travelling_tap() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 100.0, 200.0, None, start);
  tree.touch_event_at(1, TouchPhase::End, 100.0, 200.0, None, after(start, 80));
  assert_eq!(logged(&log), ["tap of 1 at 100,200"]);

  // Two fingers, the second a little later, lifted one after the other.
  tree.touch_event_at(1, TouchPhase::Start, 100.0, 200.0, None, start);
  tree.touch_event_at(2, TouchPhase::Start, 200.0, 200.0, None, after(start, 30));
  tree.touch_event_at(1, TouchPhase::End, 100.0, 200.0, None, after(start, 90));
  assert_eq!(logged(&log), [] as [&str; 0], "not before the last one lifts");
  tree.touch_event_at(2, TouchPhase::End, 200.0, 200.0, None, after(start, 110));
  assert_eq!(logged(&log), ["tap of 2 at 150,200"]);

  // A finger that rests too long is no tap.
  tree.touch_event_at(1, TouchPhase::Start, 100.0, 200.0, None, start);
  tree.touch_event_at(1, TouchPhase::End, 100.0, 200.0, None, after(start, 900));
  assert_eq!(logged(&log), [] as [&str; 0]);
}

#[test]
fn fingers_that_travel_pan_and_a_flick_is_a_swipe() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 300.0, 200.0, None, start);
  // Within the slop: nothing yet.
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 295.0, 200.0, None, after(start, 10)),
    TouchRoute::Pointer
  );
  assert_eq!(logged(&log), [] as [&str; 0]);
  // The pan is told, and the finger stays the pointer's as well.
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 260.0, 202.0, None, after(start, 20)),
    TouchRoute::Pointer
  );
  tree.touch_event_at(1, TouchPhase::Move, 180.0, 204.0, None, after(start, 40));
  tree.touch_event_at(1, TouchPhase::End, 180.0, 204.0, None, after(start, 45));

  assert_eq!(
    logged(&log),
    [
      "pan Start of 1 by -40,2",
      "pan Change of 1 by -120,4",
      "pan End of 1 by -120,4",
      "swipe Left of 1"
    ]
  );
}

#[test]
fn a_finger_that_stops_before_it_lifts_pans_without_a_swipe() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 300.0, 200.0, None, start);
  tree.touch_event_at(1, TouchPhase::Move, 180.0, 200.0, None, after(start, 40));
  tree.touch_event_at(1, TouchPhase::End, 180.0, 200.0, None, after(start, 600));

  assert_eq!(logged(&log), ["pan Start of 1 by -120,0", "pan End of 1 by -120,0"]);
}

#[test]
fn a_pan_whose_start_prevents_the_default_takes_the_fingers() {
  let (mut tree, _app, _log) = surface(true);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 300.0, 200.0, None, start);
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 250.0, 200.0, None, after(start, 20)),
    TouchRoute::Gesture
  );
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::End, 250.0, 200.0, None, after(start, 40)),
    TouchRoute::Gesture
  );
}

#[test]
fn two_fingers_travelling_together_pan_as_two() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 100.0, 100.0, None, start);
  tree.touch_event_at(2, TouchPhase::Start, 200.0, 100.0, None, after(start, 10));
  tree.touch_event_at(1, TouchPhase::Move, 100.0, 160.0, None, after(start, 20));
  tree.touch_event_at(2, TouchPhase::Move, 200.0, 160.0, None, after(start, 20));

  // The middle of the two went 30 when one had moved, and 60 when both had.
  assert_eq!(logged(&log), ["pan Start of 2 by 0,30", "pan Change of 2 by 0,60"]);
}

#[test]
fn a_finger_from_the_left_edge_that_goes_far_enough_goes_back() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Start, 5.0, 200.0, None, start),
    TouchRoute::Pointer
  );
  // It comes in: the finger is the gesture's from here.
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 45.0, 204.0, None, after(start, 30)),
    TouchRoute::Gesture
  );
  tree.touch_event_at(1, TouchPhase::Move, 205.0, 210.0, None, after(start, 300));
  // Half the window across, and at rest when it lifts.
  tree.touch_event_at(1, TouchPhase::End, 205.0, 210.0, None, after(start, 900));

  assert_eq!(
    logged(&log),
    [
      "edge Left Start 40 in",
      "edge Left Change 200 in",
      "edge Left End 200 in",
      "back"
    ]
  );
}

#[test]
fn a_finger_from_the_left_edge_that_turns_back_does_not_go_back() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 5.0, 200.0, None, start);
  tree.touch_event_at(1, TouchPhase::Move, 65.0, 200.0, None, after(start, 100));
  tree.touch_event_at(1, TouchPhase::End, 65.0, 200.0, None, after(start, 900));

  assert_eq!(logged(&log), ["edge Left Start 60 in", "edge Left End 60 in"]);
}

#[test]
fn a_finger_that_lands_by_the_edge_and_goes_along_it_is_an_ordinary_touch() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 5.0, 200.0, None, start);
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 8.0, 120.0, None, after(start, 30)),
    TouchRoute::Pointer
  );
  // Having gone along the edge, it does not come in from it later.
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 120.0, 120.0, None, after(start, 60)),
    TouchRoute::Pointer
  );
  tree.touch_event_at(1, TouchPhase::Cancel, 120.0, 120.0, None, after(start, 90));

  assert!(logged(&log).iter().all(|line| line.starts_with("pan")));
}

#[test]
fn a_finger_drags_a_slider_from_where_its_thumb_is_and_a_mouse_brings_the_thumb_to_it() {
  use crate::{app::events::MouseButton, components::Slider, core::Signal};

  let slider = |value: &Signal<i32>| {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(400, 100);
    tree.set_root(Slider::new(value.clone()).range(0, 100).width(220.0).height(20.0));
    tree.pass(&mut app, &HeadlessSurface);
    tree
  };

  // The finger lands far from the thumb, which is at the start: the value stays.
  let touched = Signal::new(0);
  let mut tree = slider(&touched);
  tree.touch_press(150.0, 10.0);
  assert_eq!(touched.get(), 0);
  // And goes with the finger: a quarter of the travel is a quarter of the range.
  tree.touch_pointer_move(200.0, 10.0);
  let moved = touched.get();
  assert!((20..=30).contains(&moved), "about a quarter: {moved}");
  tree.touch_release(200.0, 10.0);
  assert_eq!(touched.get(), moved);

  // A mouse's press at the same point brings the thumb there.
  let clicked = Signal::new(0);
  let mut tree = slider(&clicked);
  tree.mouse_down(150.0, 10.0, MouseButton::Left);
  assert!(clicked.get() > 50, "{}", clicked.get());
}

#[test]
fn a_mouse_event_tells_a_finger_from_a_mouse() {
  use crate::app::events::{MouseButton, MouseEvent, PointerKind};

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(300, 300);
  let kinds: Arc<Mutex<Vec<PointerKind>>> = Arc::default();
  let (downs, clicks) = (kinds.clone(), kinds.clone());
  tree.set_root(
    Rect::new(100.0, 100.0)
      .on_mouse_down(move |event: MouseEvent| downs.lock().unwrap().push(event.pointer))
      .on_click(move |event: MouseEvent| clicks.lock().unwrap().push(event.pointer)),
  );
  tree.pass(&mut app, &HeadlessSurface);

  tree.touch_tap(50.0, 50.0, Duration::ZERO);
  tree.mouse_down(50.0, 50.0, MouseButton::Left);
  tree.mouse_up(50.0, 50.0, MouseButton::Left);

  assert_eq!(
    *kinds.lock().unwrap(),
    [
      PointerKind::Touch,
      PointerKind::Touch,
      PointerKind::Mouse,
      PointerKind::Mouse
    ]
  );
}

#[test]
fn a_finger_from_the_right_edge_that_goes_far_enough_goes_forward() {
  let (mut tree, _app, log) = surface(false);
  let start = Instant::now();

  tree.touch_event_at(1, TouchPhase::Start, 395.0, 200.0, None, start);
  assert_eq!(
    tree.touch_event_at(1, TouchPhase::Move, 355.0, 204.0, None, after(start, 30)),
    TouchRoute::Gesture
  );
  tree.touch_event_at(1, TouchPhase::Move, 195.0, 210.0, None, after(start, 300));
  tree.touch_event_at(1, TouchPhase::End, 195.0, 210.0, None, after(start, 900));

  assert_eq!(
    logged(&log),
    [
      "edge Right Start 40 in",
      "edge Right Change 200 in",
      "edge Right End 200 in",
      "forward"
    ]
  );
}
