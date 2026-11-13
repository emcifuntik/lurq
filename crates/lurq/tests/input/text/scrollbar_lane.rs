//! Presses on a scrollbar over selectable text: the scrollbar takes the
//! pointer, so it scrolls and no text selection starts. A plain overlay bar
//! takes the pointer on and just around its thumb; a bar with a painted track
//! or a reserved gutter takes its whole lane.

use lurq::{
  app::{Tree, events::MouseButton},
  components::{Column, ScrollHorizontal, ScrollVertical, Text},
  layout::{
    layout_kind::{ScrollAxis, ScrollState},
    scrollbar::{ScrollBarPlacement, ScrollBarStyle},
  },
  node::color::Color,
};

use crate::support::render_pass;

const LONG_LINE: &str = "The quick brown fox jumps over the lazy dog while the scrollbar lane keeps the pointer \
                         from selecting this line of text";

/// Paints the tree (the scrollbar's position and style are recorded when it is
/// painted) and counts the selection highlight rects.
fn selection_rect_count(runtime: &mut Tree) -> usize {
  render_pass(runtime)
    .rects
    .iter()
    .filter(|rect| rect.color == Color::from_hex("#bfdbfe") && rect.width > 1.0 && rect.height > 0.0)
    .count()
}

/// A 4 px overlay bar, 1 px from the edge, as in the reported repro.
fn thin_overlay_bar() -> ScrollBarStyle {
  ScrollBarStyle::thin().insets(1.0, 1.0)
}

/// [`thin_overlay_bar`] with a painted track, so it owns its lane.
fn thin_tracked_bar() -> ScrollBarStyle {
  ScrollBarStyle {
    track_color: Color::from_hex("#e5e7eb"),
    ..thin_overlay_bar()
  }
}

/// `(x, y, width, height)` of the thumb as last painted.
fn thumb(state: &ScrollState, axis: ScrollAxis) -> (f32, f32, f32, f32) {
  state
    .thumb_rect_for_axis(axis, &state.style())
    .expect("the content overflows, so the scrollbar is shown")
}

fn horizontal_text_scroller(style: ScrollBarStyle) -> (Tree, ScrollState) {
  let state = ScrollState::new();
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollHorizontal::new(Text::new(LONG_LINE).selectable(true).nowrap())
      .with_scroll_state(state.clone())
      .scrollbar(style)
      .size(200.0, 30.0),
  );
  render_pass(&mut runtime);
  assert_eq!(thumb(&state, ScrollAxis::Horizontal).3, 4.0, "the thin bar is painted");
  (runtime, state)
}

fn vertical_text_scroller(style: ScrollBarStyle) -> (Tree, ScrollState) {
  let state = ScrollState::new();
  let mut column = Column::new().spacing(0.0);
  for line in 0..30 {
    column = column.child(Text::new(&format!("Selectable line {line} of the log")).selectable(true));
  }
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollVertical::new(column)
      .with_scroll_state(state.clone())
      .scrollbar(style)
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);
  assert_eq!(thumb(&state, ScrollAxis::Vertical).2, 4.0, "the thin bar is painted");
  (runtime, state)
}

fn drag(runtime: &mut Tree, from: (f32, f32), to: (f32, f32)) {
  runtime.mouse_down(from.0, from.1, MouseButton::Left);
  runtime.mouse_move(to.0, to.1);
  runtime.mouse_up(to.0, to.1, MouseButton::Left);
  render_pass(runtime);
}

/// Selects the start of the first text by dragging over it.
fn select_text(runtime: &mut Tree, y: f32) {
  drag(runtime, (2.0, y), (60.0, y));
  assert!(selection_rect_count(runtime) > 0, "dragging over the text selects it");
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn horizontal_press_just_above_thumb_drags_instead_of_selecting() {
  for offset in [1.0, 2.0, 3.0] {
    let (mut runtime, state) = horizontal_text_scroller(thin_overlay_bar());
    let (x, y, width, _) = thumb(&state, ScrollAxis::Horizontal);
    let press = (x + width / 2.0, y - offset);

    drag(&mut runtime, press, (press.0 + 40.0, press.1));

    assert_eq!(selection_rect_count(&mut runtime), 0, "{offset} px above the thumb");
    assert!(state.scroll_x() > 0.0, "{offset} px above the thumb drags it");
  }
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn horizontal_press_between_thumb_and_edge_drags_instead_of_selecting() {
  let (mut runtime, state) = horizontal_text_scroller(thin_overlay_bar());
  let (x, y, width, height) = thumb(&state, ScrollAxis::Horizontal);
  let press = (x + width / 2.0, y + height + 0.5);

  drag(&mut runtime, press, (press.0 + 40.0, press.1));

  assert_eq!(selection_rect_count(&mut runtime), 0);
  assert!(state.scroll_x() > 0.0);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn horizontal_thumb_press_arms_no_selection() {
  let (mut runtime, state) = horizontal_text_scroller(thin_overlay_bar());
  select_text(&mut runtime, 8.0);
  let (x, y, width, height) = thumb(&state, ScrollAxis::Horizontal);
  let press = (x + width / 2.0, y + height / 2.0);

  runtime.mouse_down(press.0, press.1, MouseButton::Left);
  assert!(
    selection_rect_count(&mut runtime) > 0,
    "a thumb press starts no selection, so the existing one stays"
  );
  runtime.mouse_move(press.0 + 40.0, press.1);
  assert!(state.scroll_x() > 0.0, "the press drags the thumb");
  runtime.mouse_move(press.0, press.1);
  runtime.mouse_up(press.0, press.1, MouseButton::Left);

  assert_eq!(state.scroll_x(), 0.0);
  assert!(
    selection_rect_count(&mut runtime) > 0,
    "the release keeps the selection too"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn horizontal_track_press_pages_toward_the_press() {
  let (mut runtime, state) = horizontal_text_scroller(thin_tracked_bar());
  let max_scroll = state.content_width() - state.viewport_width();
  assert!(max_scroll > 400.0, "the line overflows by more than two pages");
  let (_, y, _, height) = thumb(&state, ScrollAxis::Horizontal);

  drag(&mut runtime, (190.0, y + height / 2.0), (150.0, y + height / 2.0));
  assert_eq!(
    state.scroll_x(),
    200.0,
    "a press after the thumb pages forward one viewport"
  );
  assert_eq!(selection_rect_count(&mut runtime), 0);

  let (x, ..) = thumb(&state, ScrollAxis::Horizontal);
  drag(&mut runtime, (x - 4.0, y - 2.0), (x - 4.0, y - 2.0));
  assert_eq!(
    state.scroll_x(),
    0.0,
    "a press before the thumb pages back one viewport"
  );
  assert_eq!(selection_rect_count(&mut runtime), 0);
}

#[test]
fn horizontal_press_on_text_still_selects() {
  let (mut runtime, state) = horizontal_text_scroller(thin_overlay_bar());

  drag(&mut runtime, (4.0, 8.0), (120.0, 8.0));

  assert!(selection_rect_count(&mut runtime) > 0);
  assert_eq!(state.scroll_x(), 0.0);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn hovering_a_plain_overlay_bar_hovers_only_around_its_thumb() {
  let (mut runtime, state) = horizontal_text_scroller(thin_overlay_bar());
  let (x, y, width, _) = thumb(&state, ScrollAxis::Horizontal);

  runtime.mouse_move(x + width / 2.0, y - 2.0);
  assert!(state.is_thumb_hovered(), "beside the thumb");
  runtime.mouse_move(190.0, y);
  assert!(!state.is_thumb_hovered(), "on the unpainted track");
  runtime.mouse_move(x + width / 2.0, y);
  runtime.mouse_move(x + width / 2.0, 260.0);
  assert!(!state.is_thumb_hovered(), "outside the scroll container");
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn hovering_the_lane_of_a_painted_track_hovers_the_scrollbar() {
  let (mut runtime, state) = horizontal_text_scroller(thin_tracked_bar());
  let (_, y, ..) = thumb(&state, ScrollAxis::Horizontal);

  runtime.mouse_move(190.0, y - 2.0);
  assert!(state.is_thumb_hovered(), "on the track");
  runtime.mouse_move(20.0, 8.0);
  assert!(!state.is_thumb_hovered(), "on the text");
}

#[test]
fn press_on_the_unpainted_track_of_a_plain_overlay_bar_does_not_page() {
  let (mut runtime, state) = horizontal_text_scroller(thin_overlay_bar());
  let (_, y, _, height) = thumb(&state, ScrollAxis::Horizontal);

  drag(&mut runtime, (190.0, y + height / 2.0), (190.0, y + height / 2.0));

  assert_eq!(state.scroll_x(), 0.0);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn vertical_press_just_left_of_thumb_drags_instead_of_selecting() {
  for offset in [1.0, 2.0, 3.0] {
    let (mut runtime, state) = vertical_text_scroller(thin_overlay_bar());
    let (x, y, _, height) = thumb(&state, ScrollAxis::Vertical);
    let press = (x - offset, y + height / 2.0);

    drag(&mut runtime, press, (press.0, press.1 + 30.0));

    assert_eq!(selection_rect_count(&mut runtime), 0, "{offset} px left of the thumb");
    assert!(state.scroll_y() > 0.0, "{offset} px left of the thumb drags it");
  }
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn vertical_thumb_press_arms_no_selection() {
  let (mut runtime, state) = vertical_text_scroller(thin_overlay_bar());
  select_text(&mut runtime, 8.0);
  let (x, y, width, height) = thumb(&state, ScrollAxis::Vertical);
  let press = (x + width / 2.0, y + height / 2.0);

  runtime.mouse_down(press.0, press.1, MouseButton::Left);
  assert!(
    selection_rect_count(&mut runtime) > 0,
    "a thumb press starts no selection, so the existing one stays"
  );
  runtime.mouse_move(press.0, press.1 + 10.0);
  assert!(state.scroll_y() > 0.0, "the press drags the thumb");
  runtime.mouse_move(press.0, press.1);
  runtime.mouse_up(press.0, press.1, MouseButton::Left);

  assert_eq!(state.scroll_y(), 0.0);
  assert!(
    selection_rect_count(&mut runtime) > 0,
    "the release keeps the selection too"
  );
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn vertical_track_press_pages_toward_the_press() {
  let (mut runtime, state) = vertical_text_scroller(thin_tracked_bar());
  let (x, _, width, _) = thumb(&state, ScrollAxis::Vertical);

  drag(&mut runtime, (x + width / 2.0, 95.0), (x + width / 2.0, 95.0));
  assert_eq!(
    state.scroll_y(),
    100.0,
    "a press below the thumb pages down one viewport"
  );
  assert_eq!(selection_rect_count(&mut runtime), 0);

  drag(&mut runtime, (x + width / 2.0, 95.0), (x + width / 2.0, 95.0));
  assert_eq!(state.scroll_y(), 200.0, "a quick second press pages again");

  drag(&mut runtime, (x - 2.0, 5.0), (x - 2.0, 5.0));
  assert_eq!(state.scroll_y(), 100.0, "a press above the thumb pages up one viewport");
  assert_eq!(selection_rect_count(&mut runtime), 0);
}

#[test]
fn vertical_press_on_text_still_selects() {
  let (mut runtime, state) = vertical_text_scroller(thin_overlay_bar());

  drag(&mut runtime, (2.0, 30.0), (60.0, 30.0));

  assert!(selection_rect_count(&mut runtime) > 0);
  assert_eq!(state.scroll_y(), 0.0);
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn press_anywhere_in_a_reserved_gutter_drags_instead_of_selecting() {
  let state = ScrollState::new();
  let mut column = Column::new().spacing(0.0);
  for line in 0..30 {
    column = column.child(
      Text::new(&format!(
        "Selectable line {line} of the log, long enough to reach the gutter"
      ))
      .selectable(true)
      .nowrap(),
    );
  }
  let reserved = ScrollBarStyle {
    placement: ScrollBarPlacement::Reserved,
    ..thin_overlay_bar()
  };
  let mut runtime = Tree::new();
  runtime.set_root(
    ScrollVertical::new(column)
      .with_scroll_state(state.clone())
      .scrollbar(reserved)
      .size(200.0, 100.0),
  );
  render_pass(&mut runtime);
  let (_, y, _, height) = thumb(&state, ScrollAxis::Vertical);
  // The gutter is the 4 px bar plus 1 px on each side: x 194..200.
  let press = (194.5, y + height / 2.0);

  drag(&mut runtime, press, (press.0, press.1 + 30.0));

  assert_eq!(selection_rect_count(&mut runtime), 0);
  assert!(state.scroll_y() > 0.0);
}
