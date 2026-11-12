//! Where a scrollbar takes the pointer and where the content under it keeps it.

use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{Tree, events::MouseButton},
  components::{Column, Rect, Row, ScrollBoth, ScrollHorizontal, ScrollVertical},
  core::ElementRef,
  layout::{
    layout_kind::{ScrollAxis, ScrollState},
    scrollbar::{ScrollBarStyle, ScrollBarVisibility},
  },
  node::color::Color,
};

use crate::support::{pointer_click, render_pass};

/// The default style with a painted track, so the bar owns its lane.
fn tracked_bar() -> ScrollBarStyle {
  ScrollBarStyle {
    track_color: Color::from_hex("#e5e7eb"),
    ..ScrollBarStyle::default()
  }
}

fn counter() -> (
  Arc<AtomicUsize>,
  impl Fn(lurq::app::events::MouseEvent) + Send + Sync + 'static,
) {
  let count = Arc::new(AtomicUsize::new(0));
  let handler = {
    let count = count.clone();
    move |_| {
      count.fetch_add(1, Ordering::SeqCst);
    }
  };
  (count, handler)
}

#[test]
fn trailing_row_buttons_under_a_plain_overlay_bar_keep_their_clicks_and_hover() {
  let state = ScrollState::new();
  let (clicks, on_click) = counter();
  let on_click = Arc::new(on_click);
  let button_ref = ElementRef::new();
  let mut rows = Column::new().spacing(0.0);
  for row in 0..30 {
    let on_click = on_click.clone();
    let mut button = Rect::new(24.0, 24.0).on_click(move |event| on_click(event));
    if row == 3 {
      button = button.ref_element(button_ref.clone());
    }
    rows = rows.child(Row::new().spacing(0.0).child(Rect::new(176.0, 24.0)).child(button));
  }
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollVertical::new(rows)
      .with_scroll_state(state.clone())
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);
  let (_, thumb_y, _, thumb_height) = state
    .thumb_rect_for_axis(ScrollAxis::Vertical, &state.style())
    .expect("the rows overflow");
  // Row 3 spans y 72..96, below the thumb and its slop.
  assert!(thumb_y + thumb_height + 4.0 < 72.0);

  for x in [186.5, 194.0, 199.0] {
    runtime.mouse_move(x, 84.0);
    assert!(button_ref.hovered(), "the button at x {x} is hovered");
    pointer_click(&mut runtime, x, 84.0, MouseButton::Left);
  }

  assert_eq!(clicks.load(Ordering::SeqCst), 3, "every press clicks the button");
  assert_eq!(state.scroll_y(), 0.0, "no press pages the list");
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn trailing_row_buttons_under_a_painted_track_belong_to_the_scrollbar() {
  let state = ScrollState::new();
  let (clicks, on_click) = counter();
  let on_click = Arc::new(on_click);
  let mut rows = Column::new().spacing(0.0);
  for _ in 0..30 {
    let on_click = on_click.clone();
    let button = Rect::new(24.0, 24.0).on_click(move |event| on_click(event));
    rows = rows.child(Row::new().spacing(0.0).child(Rect::new(176.0, 24.0)).child(button));
  }
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollVertical::new(rows)
      .with_scroll_state(state.clone())
      .scrollbar(tracked_bar())
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);

  pointer_click(&mut runtime, 194.0, 84.0, MouseButton::Left);

  assert_eq!(clicks.load(Ordering::SeqCst), 0);
  assert_eq!(state.scroll_y(), 100.0, "the track press pages the list");
}

#[test]
fn an_always_visible_bar_over_fitting_content_takes_no_pointer() {
  let state = ScrollState::new();
  let (clicks, on_click) = counter();
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollVertical::new(Rect::new(200.0, 60.0).on_click(on_click))
      .with_scroll_state(state.clone())
      .scrollbar(ScrollBarStyle {
        visible: ScrollBarVisibility::Always,
        ..tracked_bar()
      })
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);
  assert!(
    state
      .thumb_rect_for_axis(ScrollAxis::Vertical, &state.style())
      .is_some(),
    "the bar is shown"
  );

  runtime.mouse_down(194.0, 20.0, MouseButton::Left);
  assert!(!state.is_dragging(), "a thumb that cannot move is not dragged");
  runtime.mouse_up(194.0, 20.0, MouseButton::Left);

  assert_eq!(
    clicks.load(Ordering::SeqCst),
    1,
    "the content under the bar keeps the press"
  );
  assert!(!state.is_thumb_hovered());
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn where_nested_lanes_overlap_the_outer_bar_painted_on_top_wins() {
  let outer = ScrollState::new();
  let inner = ScrollState::new();
  let content = Column::new()
    .spacing(0.0)
    .child(
      ScrollHorizontal::new(Rect::new(800.0, 40.0))
        .with_scroll_state(inner.clone())
        .scrollbar(tracked_bar())
        .size(200.0, 40.0),
    )
    .child(Rect::new(200.0, 300.0));
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollVertical::new(content)
      .with_scroll_state(outer.clone())
      .scrollbar(tracked_bar())
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);
  // The inner horizontal track runs along y 30..38 under the outer vertical
  // track at x 190..198, below the outer thumb.
  let (_, thumb_y, _, thumb_height) = outer
    .thumb_rect_for_axis(ScrollAxis::Vertical, &outer.style())
    .expect("the outer content overflows");
  assert!(thumb_y + thumb_height < 34.0);

  pointer_click(&mut runtime, 194.0, 34.0, MouseButton::Left);

  assert_eq!(outer.scroll_y(), 100.0, "the outer bar pages");
  assert_eq!(inner.scroll_x(), 0.0, "the inner bar under it does not");
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn in_the_corner_of_a_two_axis_scroller_the_horizontal_bar_painted_on_top_wins() {
  let state = ScrollState::new();
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollBoth::new(Rect::new(800.0, 800.0))
      .with_scroll_state(state.clone())
      .scrollbar(tracked_bar())
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);

  pointer_click(&mut runtime, 194.0, 94.0, MouseButton::Left);

  assert_eq!(state.scroll_x(), 200.0, "the horizontal bar pages");
  assert_eq!(state.scroll_y(), 0.0, "the vertical bar under it does not");
}
