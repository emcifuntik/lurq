//! Trackpad swipes between pages: a two-finger gesture sideways that nothing under the pointer scrolled with goes back
//! (fingers to the right) or forward (to the left) when the fingers lift, as `on_history_swipe` on the elements under
//! the pointer. macOS sends the gesture as scroll events with phases; a mouse wheel's scrolls have none and never
//! swipe.

use std::{
  sync::{Arc, Mutex},
  thread,
  time::Duration,
};

use lurq::{
  app::{
    Tree,
    events::{HistoryDirection, HistorySwipeEvent, ScrollEvent, ScrollPhase, with_synthetic_input},
  },
  components::{Column, Rect, ScrollHorizontal},
  layout::layout_kind::ScrollState,
};

use crate::support::run_pass;

/// Over the page, below the strip.
const PAGE: (f32, f32) = (200.0, 150.0);
/// Over the strip that scrolls sideways.
const STRIP: (f32, f32) = (200.0, 25.0);

type Swipes = Arc<Mutex<Vec<HistoryDirection>>>;

/// A 400x200 page that takes swipes, with a 400x50 strip scrolling sideways at its top. Swipes are on.
fn page(strip_scroll: f32) -> (Tree, Swipes, ScrollState) {
  let swipes = Swipes::default();
  let strip = ScrollState::new();
  let seen = swipes.clone();
  let mut tree = Tree::new();
  tree.set_root(
    Column::new()
      .spacing(0.0)
      .size(400.0, 200.0)
      .on_history_swipe(move |event: HistorySwipeEvent| seen.lock().unwrap().push(event.direction))
      .child(
        ScrollHorizontal::new(Rect::new(2000.0, 50.0).background("#1f2937"))
          .with_scroll_state(strip.clone())
          .size(400.0, 50.0),
      )
      .child(Rect::new(400.0, 150.0).background("#111827")),
  );
  run_pass(&mut tree);
  strip.set_scroll(strip_scroll, 0.0);
  run_pass(&mut tree);
  tree.set_history_swipes(true);
  (tree, swipes, strip)
}

/// A two-finger gesture at `at`: touch, `times` moves of `(dx, dy)`, lift.
fn gesture(tree: &mut Tree, at: (f32, f32), (dx, dy): (f32, f32), times: usize) {
  tree.scroll(at.0, at.1, 0.0, 0.0, ScrollPhase::Start);
  for _ in 0..times {
    tree.scroll(at.0, at.1, dx, dy, ScrollPhase::Scroll);
  }
  tree.scroll(at.0, at.1, 0.0, 0.0, ScrollPhase::End);
}

fn seen(swipes: &Swipes) -> Vec<HistoryDirection> {
  swipes.lock().unwrap().clone()
}

#[test]
fn fingers_to_the_right_go_back_and_to_the_left_forward() {
  let (mut tree, swipes, _) = page(0.0);
  gesture(&mut tree, PAGE, (20.0, 1.0), 8);
  assert_eq!(seen(&swipes), [HistoryDirection::Back]);
  // The second gesture starts at once, as the first one's momentum would: it is not a swipe.
  gesture(&mut tree, PAGE, (-20.0, 1.0), 8);
  assert_eq!(
    seen(&swipes),
    [HistoryDirection::Back],
    "a gesture right after one is its momentum"
  );
  thread::sleep(Duration::from_millis(200));
  gesture(&mut tree, PAGE, (-20.0, 1.0), 8);
  assert_eq!(seen(&swipes), [HistoryDirection::Back, HistoryDirection::Forward]);
}

#[test]
fn a_short_or_mostly_vertical_gesture_is_no_swipe() {
  let (mut tree, swipes, _) = page(0.0);
  gesture(&mut tree, PAGE, (10.0, 0.0), 8);
  thread::sleep(Duration::from_millis(200));
  gesture(&mut tree, PAGE, (20.0, 15.0), 8);
  assert_eq!(
    seen(&swipes),
    [],
    "80 px sideways is too short; 160 px with 120 px down is a scroll"
  );
}

#[test]
fn a_gesture_a_strip_scrolls_with_is_no_swipe_but_one_past_its_edge_is() {
  let (mut tree, swipes, strip) = page(100.0);
  gesture(&mut tree, STRIP, (20.0, 0.0), 8);
  assert_eq!(strip.scroll_x(), 0.0, "the strip scrolled back to its start");
  assert_eq!(seen(&swipes), [], "the strip took the gesture");
  thread::sleep(Duration::from_millis(200));
  gesture(&mut tree, STRIP, (20.0, 0.0), 8);
  assert_eq!(
    seen(&swipes),
    [HistoryDirection::Back],
    "at its start the strip takes nothing"
  );
}

#[test]
fn a_handler_that_prevents_the_scroll_keeps_the_gesture() {
  let swipes = Swipes::default();
  let seen_by_page = swipes.clone();
  let mut tree = Tree::new();
  tree.set_root(
    Column::new()
      .size(400.0, 200.0)
      .on_history_swipe(move |event: HistorySwipeEvent| seen_by_page.lock().unwrap().push(event.direction))
      .child(
        Rect::new(400.0, 200.0)
          .background("#111827")
          .on_scroll(|event: ScrollEvent| event.prevent_default()),
      ),
  );
  run_pass(&mut tree);
  tree.set_history_swipes(true);
  gesture(&mut tree, PAGE, (20.0, 0.0), 8);
  assert_eq!(seen(&swipes), [], "a canvas that pans with the gesture keeps it");
}

#[test]
fn wheels_synthetic_gestures_and_a_platform_without_swipes_make_none() {
  let (mut tree, swipes, _) = page(0.0);
  for _ in 0..10 {
    tree.scroll(PAGE.0, PAGE.1, 40.0, 0.0, ScrollPhase::Scroll);
  }
  assert_eq!(seen(&swipes), [], "a wheel's scrolls have no phases");
  thread::sleep(Duration::from_millis(200));
  with_synthetic_input(|| gesture(&mut tree, PAGE, (20.0, 0.0), 8));
  assert_eq!(seen(&swipes), [], "an agent's wheel is not a person's swipe");
  thread::sleep(Duration::from_millis(200));
  tree.set_history_swipes(false);
  gesture(&mut tree, PAGE, (20.0, 0.0), 8);
  assert_eq!(seen(&swipes), [], "swipes between pages are off");
}

#[test]
fn the_innermost_handler_that_stops_the_swipe_takes_it_alone() {
  let (outer, inner) = (Swipes::default(), Swipes::default());
  let (seen_outer, seen_inner) = (outer.clone(), inner.clone());
  let mut tree = Tree::new();
  tree.set_root(
    Column::new()
      .size(400.0, 200.0)
      .on_history_swipe(move |event: HistorySwipeEvent| seen_outer.lock().unwrap().push(event.direction))
      .child(
        Column::new()
          .size(400.0, 200.0)
          .on_history_swipe(move |event: HistorySwipeEvent| {
            seen_inner.lock().unwrap().push(event.direction);
            event.stop_propagation();
          })
          .child(Rect::new(400.0, 200.0).background("#111827")),
      ),
  );
  run_pass(&mut tree);
  tree.set_history_swipes(true);
  gesture(&mut tree, PAGE, (-20.0, 0.0), 8);
  assert_eq!(seen(&inner), [HistoryDirection::Forward]);
  assert_eq!(seen(&outer), [], "the inner handler stopped the swipe");
}

#[test]
fn a_swipe_that_interrupts_the_last_ones_momentum_still_swipes() {
  let (mut tree, swipes, _) = page(0.0);
  gesture(&mut tree, PAGE, (20.0, 1.0), 8);
  // The momentum starts at once and coasts until the fingers touch again, which ends it.
  tree.scroll(PAGE.0, PAGE.1, 0.0, 0.0, ScrollPhase::Start);
  tree.scroll(PAGE.0, PAGE.1, 12.0, 0.0, ScrollPhase::Scroll);
  thread::sleep(Duration::from_millis(400));
  tree.scroll(PAGE.0, PAGE.1, 6.0, 0.0, ScrollPhase::Scroll);
  tree.scroll(PAGE.0, PAGE.1, 0.0, 0.0, ScrollPhase::End);
  gesture(&mut tree, PAGE, (20.0, 1.0), 8);
  assert_eq!(
    seen(&swipes),
    [HistoryDirection::Back, HistoryDirection::Back],
    "the second swipe touched down as the momentum ended"
  );
}
