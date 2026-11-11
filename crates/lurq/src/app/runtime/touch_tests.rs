//! Where a moving finger scrolls, and whether a scroll it sent moved anything.

use super::{HeadlessSurface, Tree};
use crate::{
  app::{App, events::ScrollPhase},
  components::{Column, ScrollVertical, Slider, Spacer},
  core::Signal,
};

/// At scale 2 in a 400 by 400 window: a slider over the top 20 logical pixels of a list 180 high with 1000 of
/// content, and beside the list nothing.
fn list_under_a_slider() -> (Tree, App) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_scale_factor(2.0);
  tree.resize(400, 400);
  tree.set_root(
    Column::new()
      .child(Slider::new(Signal::new(0)).range(0, 10).width(100.0).height(20.0))
      .child(
        ScrollVertical::new(Spacer::new().width(100.0).height(1000.0))
          .width(100.0)
          .height(180.0),
      ),
  );
  tree.pass(&mut app, &HeadlessSurface);
  (tree, app)
}

#[test]
fn a_finger_scrolls_over_a_list_with_more_content_and_nowhere_else() {
  let (mut tree, _app) = list_under_a_slider();

  assert!(tree.touch_scrolls_at(100.0, 200.0), "over the list");
  assert!(!tree.touch_scrolls_at(100.0, 20.0), "over the slider");
  assert!(!tree.touch_scrolls_at(300.0, 200.0), "beside the list");
}

#[test]
fn a_list_that_shows_all_its_content_does_not_scroll() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 400);
  tree.set_root(ScrollVertical::new(Spacer::new().width(100.0).height(50.0)).size(100.0, 200.0));
  tree.pass(&mut app, &HeadlessSurface);

  assert!(!tree.touch_scrolls_at(50.0, 100.0));
}

#[test]
fn the_content_moves_as_far_as_the_finger() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_scale_factor(3.0);
  tree.resize(600, 600);
  let state = crate::layout::layout_kind::ScrollState::new();
  tree.set_root(
    ScrollVertical::new(Spacer::new().width(100.0).height(1000.0))
      .with_scroll_state(state.clone())
      .size(100.0, 100.0),
  );
  tree.pass(&mut app, &HeadlessSurface);

  // The finger travels 90 physical pixels up: 30 logical ones.
  tree.touch_scroll_begin(150.0, 150.0, 0.0, -90.0);
  tree.touch_scroll(150.0, 150.0, 0.0, -90.0, ScrollPhase::Scroll, false);

  assert_eq!(state.scroll_y(), 30.0);
}

#[test]
fn momentum_tells_whether_the_list_moved() {
  let (mut tree, _app) = list_under_a_slider();

  // Going up brings later content: the list moves until its end.
  tree.touch_scroll_begin(100.0, 200.0, 0.0, -100.0);
  assert!(tree.touch_scroll(100.0, 200.0, 0.0, -100.0, ScrollPhase::Scroll, true));
  assert!(tree.touch_scroll(100.0, 200.0, 0.0, -100_000.0, ScrollPhase::Scroll, true));
  assert!(
    !tree.touch_scroll(100.0, 200.0, 0.0, -100.0, ScrollPhase::Scroll, true),
    "at its end"
  );
  tree.touch_scroll_begin(300.0, 200.0, 0.0, -100.0);
  assert!(
    !tree.touch_scroll(300.0, 200.0, 0.0, -100.0, ScrollPhase::Scroll, true),
    "beside it"
  );
}

/// The page holds rows and, further down, a list of its own. Scrolling the page brings that list under the
/// finger, which goes on scrolling the page.
#[test]
fn a_gesture_keeps_scrolling_what_it_started_on() {
  use crate::layout::layout_kind::ScrollState;

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(200, 200);
  let (page, inner) = (ScrollState::new(), ScrollState::new());
  tree.set_root(
    ScrollVertical::new(
      Column::new()
        .child(Spacer::new().width(200.0).height(300.0))
        .child(
          ScrollVertical::new(Spacer::new().width(200.0).height(2000.0))
            .with_scroll_state(inner.clone())
            .size(200.0, 300.0),
        )
        .child(Spacer::new().width(200.0).height(2000.0)),
    )
    .with_scroll_state(page.clone())
    .size(200.0, 200.0),
  );
  tree.pass(&mut app, &HeadlessSurface);

  // The finger lands on the first rows; after 250 the inner list is under that point.
  tree.touch_scroll_begin(100.0, 100.0, 0.0, -250.0);
  tree.touch_scroll(100.0, 100.0, 0.0, -250.0, ScrollPhase::Scroll, false);
  tree.pass(&mut app, &HeadlessSurface);
  tree.touch_scroll(100.0, 100.0, 0.0, -100.0, ScrollPhase::Scroll, false);

  assert_eq!((page.scroll_y(), inner.scroll_y()), (350.0, 0.0));

  // A gesture that starts on the inner list scrolls it.
  tree.pass(&mut app, &HeadlessSurface);
  tree.touch_scroll_begin(100.0, 100.0, 0.0, -40.0);
  tree.touch_scroll(100.0, 100.0, 0.0, -40.0, ScrollPhase::Scroll, false);

  assert_eq!((page.scroll_y(), inner.scroll_y()), (350.0, 40.0));

  // Past the inner list's end the gesture moves nothing more: the page does not take the rest of it.
  tree.touch_scroll(100.0, 100.0, 0.0, -5000.0, ScrollPhase::Scroll, false);

  assert_eq!((page.scroll_y(), inner.scroll_y()), (350.0, 1700.0));

  // The next gesture the same way, started on the inner list at its end, scrolls the page.
  tree.pass(&mut app, &HeadlessSurface);
  tree.touch_scroll_begin(100.0, 100.0, 0.0, -30.0);
  tree.touch_scroll(100.0, 100.0, 0.0, -30.0, ScrollPhase::Scroll, false);

  assert_eq!((page.scroll_y(), inner.scroll_y()), (380.0, 1700.0));

  // And one the other way scrolls the inner list back.
  tree.pass(&mut app, &HeadlessSurface);
  tree.touch_scroll_begin(100.0, 100.0, 0.0, 30.0);
  tree.touch_scroll(100.0, 100.0, 0.0, 30.0, ScrollPhase::Scroll, false);

  assert_eq!((page.scroll_y(), inner.scroll_y()), (380.0, 1670.0));
}

/// A strip that scrolls sideways inside a page that scrolls down.
#[test]
fn the_travels_main_axis_picks_between_a_strip_and_the_page() {
  use crate::{components::ScrollHorizontal, layout::layout_kind::ScrollState};

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(200, 200);
  let (page, strip) = (ScrollState::new(), ScrollState::new());
  tree.set_root(
    ScrollVertical::new(
      Column::new()
        .child(
          ScrollHorizontal::new(Spacer::new().width(2000.0).height(100.0))
            .with_scroll_state(strip.clone())
            .size(200.0, 100.0),
        )
        .child(Spacer::new().width(200.0).height(2000.0)),
    )
    .with_scroll_state(page.clone())
    .size(200.0, 200.0),
  );
  tree.pass(&mut app, &HeadlessSurface);

  // Mostly sideways: the strip takes it, and the page gets none of its slant.
  tree.touch_scroll_begin(100.0, 50.0, -40.0, -10.0);
  tree.touch_scroll(100.0, 50.0, -40.0, -10.0, ScrollPhase::Scroll, false);
  assert_eq!((strip.scroll_x(), page.scroll_y()), (40.0, 0.0));

  // Mostly down the page, from the strip: the page takes it.
  tree.pass(&mut app, &HeadlessSurface);
  tree.touch_scroll_begin(100.0, 50.0, -10.0, -40.0);
  tree.touch_scroll(100.0, 50.0, -10.0, -40.0, ScrollPhase::Scroll, false);
  assert_eq!((strip.scroll_x(), page.scroll_y()), (40.0, 40.0));
}

/// Two taps of a finger land a few points apart, never on the same pixel.
#[test]
fn two_taps_a_few_points_apart_are_a_double_click() {
  use std::sync::{Arc, Mutex};

  use crate::{app::events::MouseButton, components::Rect};

  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_scale_factor(3.0);
  tree.resize(600, 600);
  tree.set_double_click_distance(40.0 * 3.0);
  let log = Arc::new(Mutex::new(Vec::new()));
  let (downs, ups, clicks, doubles) = (log.clone(), log.clone(), log.clone(), log.clone());
  tree.set_root(
    Rect::new(200.0, 200.0)
      .on_mouse_down(move |_| downs.lock().unwrap().push("down"))
      .on_mouse_up(move |_| ups.lock().unwrap().push("up"))
      .on_click(move |_| clicks.lock().unwrap().push("click"))
      .on_dblclick(move |_| doubles.lock().unwrap().push("double")),
  );
  tree.pass(&mut app, &HeadlessSurface);

  // 10 logical pixels apart: 30 physical ones.
  for (x, y) in [(300.0, 300.0), (324.0, 318.0)] {
    tree.mouse_down(x, y, MouseButton::Left);
    tree.mouse_up(x, y, MouseButton::Left);
  }

  // The second tap is a press and a release as well, and clicks nothing: the two are the double click.
  assert_eq!(*log.lock().unwrap(), ["down", "up", "down", "double", "up"]);
}

/// A list 100 high with 1000 of content, at scale 1.
fn list() -> (Tree, App, crate::layout::layout_kind::ScrollState) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(200, 200);
  let state = crate::layout::layout_kind::ScrollState::new();
  tree.set_root(
    ScrollVertical::new(Spacer::new().width(100.0).height(1000.0))
      .with_scroll_state(state.clone())
      .size(100.0, 100.0),
  );
  tree.pass(&mut app, &HeadlessSurface);
  (tree, app, state)
}

#[test]
fn a_finger_pulls_a_list_past_its_start_less_than_it_travels_and_lets_it_spring_back() {
  use std::time::{Duration, Instant};

  let (mut tree, _app, state) = list();

  // Down from the top: there is nothing above to scroll to.
  tree.touch_scroll_begin(50.0, 50.0, 0.0, 40.0);
  assert!(tree.touch_scroll(50.0, 50.0, 0.0, 40.0, ScrollPhase::Scroll, false));
  let pulled = state.overscroll().1;
  assert!(
    (-40.0..-10.0).contains(&pulled),
    "drawn before the start, by less than 40: {pulled}"
  );
  assert_eq!(state.scroll_y(), 0.0);

  // Further, it gives less and less.
  tree.touch_scroll(50.0, 50.0, 0.0, 40.0, ScrollPhase::Scroll, false);
  let further = state.overscroll().1;
  assert!(
    further < pulled && pulled - further < -pulled,
    "{pulled} then {further}"
  );

  // Back up by half the travel: the pull lets go first, and the list does not scroll.
  tree.touch_scroll(50.0, 50.0, 0.0, -40.0, ScrollPhase::Scroll, false);
  assert_eq!((state.overscroll().1, state.scroll_y()), (pulled, 0.0));

  // The finger lifts: the list springs back and comes to rest at its start.
  tree.touch_scroll(50.0, 50.0, 0.0, 0.0, ScrollPhase::End, false);
  assert!(tree.touch_animating());
  // The momentum's first event follows the lift at once and leaves the list where the spring holds it.
  tree.touch_scroll(50.0, 50.0, 0.0, 0.0, ScrollPhase::Start, false);
  assert_eq!(state.overscroll().1, pulled);
  let lifted = Instant::now();
  assert!(tree.touch_tick(lifted + Duration::from_millis(50)));
  let halfway = state.overscroll().1;
  assert!(pulled < halfway && halfway < 0.0, "{pulled} then {halfway}");
  assert!(!tree.touch_tick(lifted + Duration::from_secs(2)));
  assert_eq!(state.overscroll(), (0.0, 0.0));
  assert!(!tree.touch_animating());
}

#[test]
fn travel_past_the_end_and_back_scrolls_only_once_the_pull_is_let_go() {
  let (mut tree, _app, state) = list();

  tree.touch_scroll_begin(50.0, 50.0, 0.0, 30.0);
  tree.touch_scroll(50.0, 50.0, 0.0, 30.0, ScrollPhase::Scroll, false);
  // 30 back lets the pull go, the other 20 scroll.
  tree.touch_scroll(50.0, 50.0, 0.0, -50.0, ScrollPhase::Scroll, false);

  assert_eq!((state.overscroll().1, state.scroll_y()), (0.0, 20.0));
}

#[test]
fn momentum_that_reaches_the_end_carries_the_list_past_it_and_back() {
  use std::time::{Duration, Instant};

  let (mut tree, _app, state) = list();
  tree.touch_scroll_begin(50.0, 50.0, 0.0, -40.0);
  tree.touch_scroll(50.0, 50.0, 0.0, -100_000.0, ScrollPhase::Scroll, true);
  assert_eq!(
    (state.scroll_y(), state.overscroll().1),
    (900.0, 0.0),
    "momentum itself pulls nothing"
  );
  assert!(!tree.touch_scroll(50.0, 50.0, 0.0, -10.0, ScrollPhase::Scroll, true));

  // The finger was going up at 1500 px/s.
  let reached = Instant::now();
  tree.touch_scroll_bounce(0.0, -1500.0, reached);
  assert!(tree.touch_tick(reached + Duration::from_millis(60)));
  let out = state.overscroll().1;
  assert!(out > 10.0, "carried beyond the end: {out}");
  assert!(!tree.touch_tick(reached + Duration::from_secs(2)));
  assert_eq!((state.scroll_y(), state.overscroll().1), (900.0, 0.0));
}

#[test]
fn momentum_that_stops_short_of_an_end_does_not_bounce() {
  let (mut tree, _app, state) = list();
  tree.touch_scroll_begin(50.0, 50.0, 0.0, -40.0);
  tree.touch_scroll(50.0, 50.0, 0.0, -40.0, ScrollPhase::Scroll, true);

  tree.touch_scroll_bounce(0.0, -1500.0, std::time::Instant::now());

  assert!(!tree.touch_animating());
  assert_eq!(state.overscroll(), (0.0, 0.0));
}

#[test]
fn a_list_that_shows_all_its_content_is_not_pulled() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(200, 200);
  let state = crate::layout::layout_kind::ScrollState::new();
  tree.set_root(
    ScrollVertical::new(Spacer::new().width(100.0).height(50.0))
      .with_scroll_state(state.clone())
      .size(100.0, 100.0),
  );
  tree.pass(&mut app, &HeadlessSurface);

  tree.touch_scroll_begin(50.0, 50.0, 0.0, 40.0);
  tree.touch_scroll(50.0, 50.0, 0.0, 40.0, ScrollPhase::Scroll, false);

  assert_eq!(state.overscroll(), (0.0, 0.0));
}

#[test]
fn overscrolled_content_is_laid_out_past_its_end() {
  let (mut tree, mut app, state) = list();
  tree.touch_scroll_begin(50.0, 50.0, 0.0, 40.0);
  tree.touch_scroll(50.0, 50.0, 0.0, 40.0, ScrollPhase::Scroll, false);
  assert!(tree.needs_redraw());
  tree.pass(&mut app, &HeadlessSurface);

  // A point just under the list's top is now above its content: the gap the pull opened.
  let gap = -state.overscroll().1;
  assert!(gap > 10.0);
  let layout = tree.last_layout.as_ref().expect("a layout");
  let content = layout.children.first().expect("the list's content");
  assert_eq!(content.offset.y, gap);
}

#[test]
fn a_finger_catches_a_list_that_is_springing_back_and_lets_it_go_again() {
  use std::time::{Duration, Instant};

  let (mut tree, _app, state) = list();
  tree.touch_scroll_begin(50.0, 50.0, 0.0, 60.0);
  tree.touch_scroll(50.0, 50.0, 0.0, 60.0, ScrollPhase::Scroll, false);
  tree.touch_scroll(50.0, 50.0, 0.0, 0.0, ScrollPhase::End, false);
  tree.touch_tick(Instant::now() + Duration::from_millis(40));
  let caught_at = state.overscroll().1;
  assert!(caught_at < 0.0);

  // The finger lands: the list stays where it is, however long the finger rests.
  assert!(tree.touch_scroll_catch());
  assert!(!tree.touch_animating());
  assert!(!tree.touch_tick(Instant::now() + Duration::from_secs(1)));
  assert_eq!(state.overscroll().1, caught_at);
  assert!(!tree.touch_scroll_catch(), "nothing is springing any more");

  // It lifts without scrolling: the list springs back from there.
  tree.touch_scroll_settle();
  assert!(tree.touch_animating());
  assert!(!tree.touch_tick(Instant::now() + Duration::from_secs(2)));
  assert_eq!(state.overscroll(), (0.0, 0.0));
}

#[test]
fn a_tap_on_the_status_bar_scrolls_the_page_to_its_top() {
  use std::time::{Duration, Instant};

  let (mut tree, _app, state) = list();
  let now = Instant::now();
  assert!(!tree.touch_scroll_to_top(now), "already at its top");

  state.set_scroll(0.0, 400.0);
  assert!(tree.touch_scroll_to_top(now));
  assert!(tree.touch_animating());
  assert!(tree.touch_tick(now + Duration::from_millis(100)));
  let on_its_way = state.scroll_y();
  assert!(
    0.0 < on_its_way && on_its_way < 200.0,
    "most of the way after a third of the time: {on_its_way}"
  );
  assert!(!tree.touch_tick(now + Duration::from_millis(400)));
  assert_eq!(state.scroll_y(), 0.0);
  assert!(!tree.touch_animating());
}

#[test]
fn a_finger_stops_a_page_on_its_way_to_its_top() {
  use std::time::{Duration, Instant};

  let (mut tree, _app, state) = list();
  let now = Instant::now();
  state.set_scroll(0.0, 400.0);
  tree.touch_scroll_to_top(now);
  tree.touch_tick(now + Duration::from_millis(100));
  let stopped_at = state.scroll_y();

  assert!(tree.touch_scroll_catch());
  assert!(!tree.touch_tick(now + Duration::from_millis(400)));
  assert_eq!(state.scroll_y(), stopped_at);
}

#[test]
fn the_scroll_indicator_shortens_against_the_end_the_list_is_pulled_past() {
  use crate::layout::{layout_kind::ScrollAxis, scrollbar::ScrollBarStyle};

  let (mut tree, mut app, state) = list();
  let style = ScrollBarStyle::default();
  let thumb = |state: &crate::layout::layout_kind::ScrollState| {
    let bar = state
      .scrollbar_geometry_for_axis(ScrollAxis::Vertical, &style)
      .expect("a bar");
    (bar.thumb_y, bar.thumb_height)
  };
  let at_rest = thumb(&state);

  // Pulled past its start: the thumb keeps its top and gives up as much length as the content moved.
  tree.touch_scroll_begin(50.0, 50.0, 0.0, 30.0);
  tree.touch_scroll(50.0, 50.0, 0.0, 30.0, ScrollPhase::Scroll, false);
  tree.pass(&mut app, &HeadlessSurface);
  let pulled = thumb(&state);
  assert_eq!(pulled.0, at_rest.0);
  assert!(
    (pulled.1 - (at_rest.1 + state.overscroll().1)).abs() < 0.01,
    "{at_rest:?} then {pulled:?}"
  );

  // Pulled past its end: it keeps its bottom.
  tree.touch_scroll(50.0, 50.0, 0.0, 0.0, ScrollPhase::End, false);
  tree.touch_tick(std::time::Instant::now() + std::time::Duration::from_secs(2));
  state.set_scroll(0.0, 900.0);
  tree.pass(&mut app, &HeadlessSurface);
  let at_end = thumb(&state);
  tree.touch_scroll_begin(50.0, 50.0, 0.0, -30.0);
  tree.touch_scroll(50.0, 50.0, 0.0, -30.0, ScrollPhase::Scroll, false);
  tree.pass(&mut app, &HeadlessSurface);
  let pulled = thumb(&state);
  assert!(pulled.1 < at_end.1);
  assert!(
    (pulled.0 + pulled.1 - (at_end.0 + at_end.1)).abs() < 0.01,
    "{at_end:?} then {pulled:?}"
  );
}

#[test]
fn a_list_let_go_while_the_finger_still_pulls_goes_on_outward_before_it_springs_back() {
  use std::time::{Duration, Instant};

  let (mut tree, _app, state) = list();
  tree.touch_scroll_begin(50.0, 50.0, 0.0, 40.0);
  tree.touch_scroll(50.0, 50.0, 0.0, 40.0, ScrollPhase::Scroll, false);
  tree.touch_scroll(50.0, 50.0, 0.0, 0.0, ScrollPhase::End, false);
  let let_go_at = state.overscroll().1;

  // The finger was going down at 1200 px/s when it lifted; the shell's momentum finds the list at its end.
  let now = Instant::now();
  tree.touch_scroll_bounce(0.0, 1200.0, now);
  tree.touch_tick(now + Duration::from_millis(30));
  let further = state.overscroll().1;
  assert!(
    further < let_go_at,
    "further out than where it was let go: {let_go_at} then {further}"
  );

  assert!(!tree.touch_tick(now + Duration::from_secs(3)));
  assert_eq!(state.overscroll(), (0.0, 0.0));
}

#[test]
fn a_list_that_does_not_scroll_to_top_leaves_the_status_bars_tap_alone() {
  let (mut tree, _app, state) = list();
  state.set_scroll(0.0, 400.0);
  state.set_scrolls_to_top(false);

  assert!(!tree.touch_scroll_to_top(std::time::Instant::now()));
  assert_eq!(state.scroll_y(), 400.0);
}

#[test]
fn a_finger_scrolls_from_a_scroll_indicator_and_drags_a_bar_that_is_always_there() {
  use crate::layout::scrollbar::ScrollBarStyle;

  let list_with = |style: ScrollBarStyle| {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(200, 200);
    tree.set_root(
      ScrollVertical::new(Spacer::new().width(100.0).height(1000.0))
        .scrollbar(style)
        .size(100.0, 100.0),
    );
    tree.pass(&mut app, &HeadlessSurface);
    // Scrolling shows an indicator; then a frame paints it.
    tree.touch_scroll_begin(50.0, 50.0, 0.0, -10.0);
    tree.touch_scroll(50.0, 50.0, 0.0, -10.0, ScrollPhase::Scroll, false);
    tree.pass(&mut app, &HeadlessSurface);
    tree
  };
  // On the bar, at the list's right edge.
  let mut bar = list_with(ScrollBarStyle {
    width: 12.0,
    padding: 0.0,
    ..ScrollBarStyle::thin()
  });
  assert!(!bar.touch_scrolls_at(96.0, 10.0), "a bar is dragged");

  let mut indicator = list_with(ScrollBarStyle {
    width: 12.0,
    padding: 0.0,
    end_inset: None,
    ..ScrollBarStyle::indicator()
  });
  assert!(
    indicator.touch_scrolls_at(96.0, 10.0),
    "an indicator is content to a moving finger"
  );
}
