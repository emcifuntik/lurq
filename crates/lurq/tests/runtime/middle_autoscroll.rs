//! Middle-button autoscroll (lurq#78): a scroll container that turns it on
//! scrolls toward the pointer while the middle button is held, faster the
//! farther the pointer is from where the button went down; the release, Esc
//! or leaving the window ends it. Off by default.

use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::{Duration, Instant},
};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton},
  components::{Column, Rect, Row, ScrollHorizontal, ScrollVertical, VirtualizedList},
  layout::layout_kind::ScrollState,
  node::{Element, color::Color, cursor::CursorIcon},
};

use crate::support::{RenderSnapshot, render_pass_with_app};

const FRAME: Duration = Duration::from_millis(16);
/// Where the button goes down: inside the 200x200 viewport, off the scrollbar.
const PRESS: (f32, f32) = (80.0, 100.0);

/// Fifty 20px rows: 1000px of content in a 200px viewport.
fn rows() -> Column {
  let mut column = Column::new().spacing(0.0);
  for _ in 0..50 {
    column = column.child(Rect::new(150.0, 20.0));
  }
  column
}

fn list(state: &ScrollState, autoscroll: bool) -> ScrollVertical {
  ScrollVertical::new(rows())
    .with_scroll_state(state.clone())
    .middle_button_autoscroll(autoscroll)
    .size(200.0, 200.0)
}

struct Page {
  tree: Tree,
  app: App,
  now: Instant,
}

impl Page {
  fn new(root: impl Into<Element>) -> Self {
    let mut page = Self {
      tree: Tree::new(),
      app: App::new(),
      now: Instant::now(),
    };
    page.tree.set_animation_clock(Some(page.now));
    page.tree.set_root(root);
    page.pass();
    page
  }

  fn pass(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  /// `frames` passes, a frame apart.
  fn frames(&mut self, frames: u32) -> RenderSnapshot {
    let mut snapshot = None;
    for _ in 0..frames {
      self.now += FRAME;
      self.tree.set_animation_clock(Some(self.now));
      snapshot = Some(self.pass());
    }
    snapshot.expect("at least one frame")
  }

  fn press(&mut self) {
    self.tree.mouse_down(PRESS.0, PRESS.1, MouseButton::Middle);
  }

  /// Moves the pointer `(dx, dy)` from where the button went down.
  fn hold_at(&mut self, dx: f32, dy: f32) {
    self.tree.mouse_move(PRESS.0 + dx, PRESS.1 + dy);
  }

  fn release(&mut self, dx: f32, dy: f32) {
    self.tree.mouse_up(PRESS.0 + dx, PRESS.1 + dy, MouseButton::Middle);
  }
}

#[test]
fn a_container_does_not_autoscroll_unless_it_turns_it_on() {
  let state = ScrollState::new();
  let mut page = Page::new(list(&state, false));
  page.press();
  page.hold_at(0.0, 80.0);
  page.frames(10);
  assert!(!page.tree.is_autoscrolling());
  assert_eq!(state.scroll_y(), 0.0);
}

#[test]
fn holding_below_the_press_scrolls_down_steadily_and_the_release_stops_it() {
  let state = ScrollState::new();
  let mut page = Page::new(list(&state, true));
  page.press();
  assert!(page.tree.is_autoscrolling());
  assert_eq!(
    page.tree.cursor(),
    CursorIcon::NsResize,
    "a vertical container's cursor"
  );
  page.hold_at(0.0, 60.0);
  page.frames(5);
  let after_five = state.scroll_y();
  assert!(after_five > 0.0, "the content scrolls down");
  page.frames(5);
  let after_ten = state.scroll_y();
  assert!(
    (after_ten - 2.0 * after_five).abs() < 1.0,
    "the speed holds while the pointer does: {after_five} then {after_ten}"
  );

  page.release(0.0, 60.0);
  assert!(!page.tree.is_autoscrolling());
  assert_eq!(page.tree.cursor(), CursorIcon::Default);
  page.frames(5);
  assert_eq!(state.scroll_y(), after_ten, "the release stops it");
}

#[test]
fn the_farther_the_pointer_the_faster_and_above_the_press_scrolls_up() {
  let speed = |dy: f32| {
    let state = ScrollState::new();
    let mut page = Page::new(list(&state, true));
    state.set_scroll(0.0, 400.0);
    page.pass();
    page.press();
    page.hold_at(0.0, dy);
    page.frames(4);
    state.scroll_y() - 400.0
  };
  let (near, far, above) = (speed(30.0), speed(90.0), speed(-90.0));
  assert!(near > 0.0 && far > 2.0 * near, "near {near}, far {far}");
  assert!(
    (above + far).abs() < 0.5,
    "above the press: {above}, as fast up as below is down"
  );
}

#[test]
fn the_pointer_near_the_press_scrolls_nothing() {
  let state = ScrollState::new();
  let mut page = Page::new(list(&state, true));
  page.press();
  page.hold_at(6.0, 8.0);
  page.frames(10);
  assert_eq!(state.scroll_y(), 0.0, "inside the dead zone");
  assert!(page.tree.is_autoscrolling());
  // Time spent there is not scrolled once the pointer leaves it.
  page.hold_at(0.0, 30.0);
  page.frames(1);
  assert!(state.scroll_y() < 5.0, "one frame's worth: {}", state.scroll_y());
}

#[test]
fn esc_and_leaving_the_window_end_it_and_its_release_clicks_nothing() {
  let clicks = Arc::new(AtomicUsize::new(0));
  let on_click = {
    let clicks = clicks.clone();
    move |_| {
      clicks.fetch_add(1, Ordering::SeqCst);
    }
  };
  for end in ["escape", "leave"] {
    let state = ScrollState::new();
    let mut page = Page::new(
      Column::new()
        .on_mouse_click(MouseButton::Middle, on_click.clone())
        .child(list(&state, true)),
    );
    page.press();
    page.hold_at(0.0, 60.0);
    page.frames(3);
    let scrolled = state.scroll_y();
    assert!(scrolled > 0.0);
    match end {
      "escape" => page
        .tree
        .key_down("Escape".into(), "Escape".into(), false, false, false),
      _ => page.tree.mouse_leave_window(),
    }
    assert!(!page.tree.is_autoscrolling(), "{end} ends it");
    page.frames(3);
    assert_eq!(state.scroll_y(), scrolled, "{end}: nothing scrolls after");
    page.release(0.0, 60.0);
  }
  assert_eq!(clicks.load(Ordering::SeqCst), 0, "no middle click");
}

#[test]
fn a_release_after_autoscroll_is_no_middle_click() {
  let clicks = Arc::new(AtomicUsize::new(0));
  let state = ScrollState::new();
  let mut page = Page::new(
    Column::new()
      .on_mouse_click(MouseButton::Middle, {
        let clicks = clicks.clone();
        move |_| {
          clicks.fetch_add(1, Ordering::SeqCst);
        }
      })
      .child(list(&state, true)),
  );
  page.press();
  page.release(0.0, 0.0);
  assert_eq!(clicks.load(Ordering::SeqCst), 0);
}

#[test]
fn a_horizontal_container_scrolls_sideways_only() {
  let state = ScrollState::new();
  let mut wide = Row::new().spacing(0.0);
  for _ in 0..50 {
    wide = wide.child(Rect::new(20.0, 150.0));
  }
  let mut page = Page::new(
    ScrollHorizontal::new(wide)
      .with_scroll_state(state.clone())
      .middle_button_autoscroll(true)
      .size(200.0, 200.0),
  );
  page.press();
  assert_eq!(page.tree.cursor(), CursorIcon::EwResize);
  page.hold_at(-60.0, 0.0);
  page.frames(3);
  assert_eq!(
    state.scroll_x(),
    0.0,
    "left of the press, at the start: nothing to scroll"
  );
  page.hold_at(60.0, 80.0);
  page.frames(3);
  assert!(state.scroll_x() > 0.0, "right of the press scrolls right");
  assert_eq!(state.scroll_y(), 0.0);
}

#[test]
fn an_element_that_takes_the_middle_button_keeps_its_press() {
  let clicks = Arc::new(AtomicUsize::new(0));
  let state = ScrollState::new();
  let mut column = Column::new()
    .spacing(0.0)
    .child(Rect::new(150.0, 200.0).on_mouse_click(MouseButton::Middle, {
      let clicks = clicks.clone();
      move |_| {
        clicks.fetch_add(1, Ordering::SeqCst);
      }
    }));
  for _ in 0..40 {
    column = column.child(Rect::new(150.0, 20.0));
  }
  let mut page = Page::new(
    ScrollVertical::new(column)
      .with_scroll_state(state.clone())
      .middle_button_autoscroll(true)
      .size(200.0, 200.0),
  );
  page.press();
  assert!(!page.tree.is_autoscrolling(), "the element's middle click");
  page.release(0.0, 0.0);
  assert_eq!(clicks.load(Ordering::SeqCst), 1);

  // A handler that prevents the press's default keeps it too.
  let state = ScrollState::new();
  let mut page = Page::new(
    ScrollVertical::new(rows().on_mouse_down(|event: lurq::app::events::MouseEvent| {
      if event.button == MouseButton::Middle {
        event.prevent_default();
      }
    }))
    .with_scroll_state(state.clone())
    .middle_button_autoscroll(true)
    .size(200.0, 200.0),
  );
  page.press();
  assert!(!page.tree.is_autoscrolling(), "prevented");
}

#[test]
fn the_innermost_container_that_can_scroll_takes_the_press() {
  let outer = ScrollState::new();
  let inner = ScrollState::new();
  // A short inner list that turns autoscroll on but does not overflow, in a
  // long outer one.
  let mut column = Column::new().spacing(0.0).child(
    ScrollVertical::new(Rect::new(150.0, 100.0))
      .with_scroll_state(inner.clone())
      .middle_button_autoscroll(true)
      .size(180.0, 180.0),
  );
  for _ in 0..40 {
    column = column.child(Rect::new(150.0, 20.0));
  }
  let mut page = Page::new(
    ScrollVertical::new(column)
      .with_scroll_state(outer.clone())
      .middle_button_autoscroll(true)
      .size(200.0, 200.0),
  );
  page.press();
  page.hold_at(0.0, 60.0);
  page.frames(3);
  assert!(outer.scroll_y() > 0.0, "the outer list scrolls");
  assert_eq!(inner.scroll_y(), 0.0);
}

#[test]
fn scroll_handlers_hear_the_autoscroll() {
  let scrolls = Arc::new(AtomicUsize::new(0));
  let ends = Arc::new(AtomicUsize::new(0));
  let state = ScrollState::new();
  let mut page = Page::new(
    list(&state, true)
      .on_scroll({
        let scrolls = scrolls.clone();
        move |_| {
          scrolls.fetch_add(1, Ordering::SeqCst);
        }
      })
      .on_scroll_end({
        let ends = ends.clone();
        move |_| {
          ends.fetch_add(1, Ordering::SeqCst);
        }
      }),
  );
  page.press();
  page.hold_at(0.0, 60.0);
  page.frames(4);
  page.release(0.0, 60.0);
  assert_eq!(scrolls.load(Ordering::SeqCst), 4, "one scroll a frame");
  assert_eq!(ends.load(Ordering::SeqCst), 1, "and an end");
}

#[test]
fn a_mark_is_drawn_where_the_button_went_down_while_it_runs() {
  let state = ScrollState::new();
  let mut page = Page::new(list(&state, true));
  let marks = |snapshot: &RenderSnapshot| {
    snapshot
      .rects
      .iter()
      .filter(|rect| {
        rect.color == Color::new(255, 255, 255, 230)
          && (rect.x + rect.width / 2.0 - PRESS.0).abs() < 0.5
          && (rect.y + rect.height / 2.0 - PRESS.1).abs() < 0.5
      })
      .count()
  };
  page.press();
  page.hold_at(0.0, 60.0);
  let snapshot = page.frames(1);
  assert_eq!(marks(&snapshot), 1, "the mark stays where the button went down");
  page.release(0.0, 60.0);
  assert_eq!(marks(&page.frames(1)), 0, "and goes with the release");
}

thread_local! {
  static LIST_STATE: ScrollState = ScrollState::new();
}

struct ListRow;

impl Component for ListRow {
  type Props = usize;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Rect::new(150.0, 20.0).id(format!("row-{}", ctx.props::<usize>()))
  }
}

struct LongList;

impl Component for LongList {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let state = LIST_STATE.with(Clone::clone);
    VirtualizedList::new(ctx, 0..2_000_usize)
      .with_scroll_state(state)
      .middle_button_autoscroll(true)
      .size(200.0, 200.0)
      .mount_keyed::<ListRow, _, _, _>(|item| *item, |item| *item)
  }
}

#[test]
fn a_virtualized_list_autoscrolls_through_its_rows() {
  let state = LIST_STATE.with(Clone::clone);
  let mut page = Page {
    tree: Tree::new(),
    app: App::new(),
    now: Instant::now(),
  };
  page.tree.set_animation_clock(Some(page.now));
  page.tree.mount_root::<LongList>(&mut page.app, ());
  page.frames(2);
  page.press();
  assert!(page.tree.is_autoscrolling());
  page.hold_at(0.0, 150.0);
  page.frames(60);
  page.release(0.0, 150.0);
  page.frames(2);
  let top = state.scroll_y();
  assert!(top > 1_500.0, "a second far from the press runs far down: {top}");
  let row = (top / 20.0) as usize + 2;
  assert!(
    page.tree.get_element_by_id(&format!("row-{row}")).is_some(),
    "the rows at the new place are mounted (row {row})"
  );
}
