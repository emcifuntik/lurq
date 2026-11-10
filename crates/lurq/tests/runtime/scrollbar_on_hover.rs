//! A scrollbar shown only on hover (`ScrollBarVisibility::OnHover`): hidden at rest, where it is not painted and
//! takes no pointer; shown while the pointer is over its scroll container, while the content scrolls and while
//! its thumb is dragged; then it stays for the style's delay and fades out. A bar that is never shown
//! (`ScrollBarVisibility::Never`) leaves the wheel scrolling and the content its clicks.

use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::{Duration, Instant},
};

use lurq::{
  animation::Easing,
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, ScrollPhase},
  },
  components::{Column, Rect, Row, ScrollVertical, VirtualizedList},
  layout::{
    layout_kind::ScrollState,
    scrollbar::{ScrollBarStyle, ScrollBarVisibility},
  },
  node::{Element, color::Color},
};
use lurq_macros::DevtoolsInspectable;

use crate::support::{RenderSnapshot, pointer_click, render_pass_if_needed, render_pass_with_app};

const THUMB: Color = Color::new(200, 30, 90, 255);
const DELAY: Duration = Duration::from_millis(600);
const FADE: Duration = Duration::from_millis(200);
/// Inside the list, left of where its bar runs.
const OVER: (f32, f32) = (60.0, 50.0);
/// Outside the 200 by 100 list.
const AWAY: (f32, f32) = (300.0, 300.0);

fn bar(visible: ScrollBarVisibility) -> ScrollBarStyle {
  ScrollBarStyle {
    thumb_color: THUMB,
    visible,
    fade_easing: Easing::Linear,
    ..ScrollBarStyle::default()
  }
  .fade(DELAY, FADE)
}

/// The alpha the thumb is painted with, or `None` when it is not painted.
fn thumb_alpha(snapshot: &RenderSnapshot) -> Option<u8> {
  snapshot
    .rects
    .iter()
    .find(|rect| (rect.color.r(), rect.color.g(), rect.color.b()) == (THUMB.r(), THUMB.g(), THUMB.b()))
    .map(|rect| rect.color.a())
}

struct List {
  tree: Tree,
  app: App,
  state: ScrollState,
  clicks: Arc<AtomicUsize>,
  start: Instant,
}

impl List {
  /// A 200 by 100 list of `rows` 24 px rows, each ending in a button under
  /// the bar's lane, on a clock the test moves.
  fn new(rows: usize, style: ScrollBarStyle) -> Self {
    let state = ScrollState::new();
    let clicks = Arc::new(AtomicUsize::new(0));
    let mut column = Column::new().spacing(0.0);
    for _ in 0..rows {
      let clicks = clicks.clone();
      let button = Rect::new(24.0, 24.0).on_click(move |_| {
        clicks.fetch_add(1, Ordering::SeqCst);
      });
      column = column.child(Row::new().spacing(0.0).child(Rect::new(176.0, 24.0)).child(button));
    }
    let mut tree = Tree::new();
    tree.resize(400, 400);
    tree.set_root(
      Column::new().child(
        ScrollVertical::new(column)
          .with_scroll_state(state.clone())
          .scrollbar(style)
          .size(200.0, 100.0),
      ),
    );
    let start = Instant::now();
    tree.set_animation_clock(Some(start));
    let mut list = Self {
      tree,
      app: App::new(),
      state,
      clicks,
      start,
    };
    list.render();
    list
  }

  fn render(&mut self) -> RenderSnapshot {
    render_pass_with_app(&mut self.tree, &mut self.app)
  }

  fn thumb(&mut self) -> Option<u8> {
    thumb_alpha(&self.render())
  }

  /// Moves the clock to `after` since the list was made.
  fn at(&mut self, after: Duration) {
    self.tree.set_animation_clock(Some(self.start + after));
  }
}

#[test]
fn an_on_hover_bar_is_hidden_until_the_pointer_is_over_its_container() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));
  assert_eq!(list.thumb(), None, "the bar is hidden at rest");

  list.tree.mouse_move(OVER.0, OVER.1);
  assert_eq!(
    list.thumb(),
    Some(255),
    "the pointer anywhere over the list shows the bar"
  );

  list.at(DELAY + FADE + Duration::from_secs(5));
  assert_eq!(list.thumb(), Some(255), "and holds it shown for as long as it stays");
}

#[test]
fn an_on_hover_bar_stays_for_its_delay_then_fades_out() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));
  list.tree.mouse_move(OVER.0, OVER.1);
  list.render();

  list.at(Duration::from_secs(1));
  list.tree.mouse_move(AWAY.0, AWAY.1);
  assert_eq!(list.thumb(), Some(255), "the bar stays when the pointer leaves");
  let left = Duration::from_secs(1);

  list.at(left + DELAY - Duration::from_millis(1));
  assert_eq!(list.thumb(), Some(255), "and through its delay");

  list.at(left + DELAY + FADE / 2);
  let half = list.thumb().expect("the bar is fading");
  assert!((120..=135).contains(&half), "half way through a linear fade: {half}");

  list.at(left + DELAY + FADE);
  assert_eq!(list.thumb(), None, "the bar is gone when the fade ends");
  list.at(left + DELAY + FADE + Duration::from_secs(5));
  assert_eq!(list.thumb(), None, "and stays hidden");
}

#[test]
fn an_on_hover_bar_fades_without_anyone_asking_for_a_frame() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));
  list.tree.mouse_move(OVER.0, OVER.1);
  list.render();
  list.tree.mouse_move(AWAY.0, AWAY.1);
  list.render();

  list.at(DELAY / 2);
  assert!(
    render_pass_if_needed(&mut list.tree, &mut list.app).is_none(),
    "nothing changes during the delay"
  );

  list.at(DELAY + FADE / 2);
  let fading = render_pass_if_needed(&mut list.tree, &mut list.app).expect("the end of the delay draws a frame");
  assert!(
    thumb_alpha(&fading).is_some_and(|alpha| alpha < 255),
    "of the fading bar"
  );

  list.at(DELAY + FADE);
  let gone = render_pass_if_needed(&mut list.tree, &mut list.app).expect("the fade draws its last frame");
  assert_eq!(thumb_alpha(&gone), None);

  list.at(DELAY + FADE + Duration::from_secs(5));
  assert!(
    render_pass_if_needed(&mut list.tree, &mut list.app).is_none(),
    "a hidden bar asks for no more frames"
  );
}

#[test]
fn an_on_hover_bar_goes_on_the_wall_clock_at_the_frame_its_change_schedules() {
  // On the wall clock the change comes as a scheduled redraw, which must paint the bars again instead of showing
  // the frame before from the cached render list.
  let delay = Duration::from_millis(30);
  let style = bar(ScrollBarVisibility::OnHover).fade(delay, Duration::ZERO);
  let mut list = List::new(30, style);
  list.tree.set_animation_clock(None);
  list.tree.mouse_move(OVER.0, OVER.1);
  list.render();
  list.tree.mouse_move(AWAY.0, AWAY.1);
  assert_eq!(
    list.thumb(),
    Some(255),
    "the delay starts at the frame that finds the pointer gone"
  );

  std::thread::sleep(delay * 2);
  let next = render_pass_if_needed(&mut list.tree, &mut list.app).expect("the bar's change draws a frame");
  assert_eq!(thumb_alpha(&next), None, "without the bar");
}

#[test]
fn an_on_hover_bar_with_no_fade_duration_goes_at_once() {
  let style = bar(ScrollBarVisibility::OnHover).fade(DELAY, Duration::ZERO);
  let mut list = List::new(30, style);
  list.tree.mouse_move(OVER.0, OVER.1);
  list.render();
  list.tree.mouse_move(AWAY.0, AWAY.1);
  list.render();

  list.at(DELAY - Duration::from_millis(1));
  assert_eq!(list.thumb(), Some(255));
  list.at(DELAY);
  assert_eq!(list.thumb(), None);
}

#[test]
fn an_on_hover_bar_shows_while_the_content_scrolls() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));

  list.state.set_scroll(0.0, 120.0);
  assert_eq!(
    list.thumb(),
    Some(255),
    "a scroll shows the bar with the pointer elsewhere"
  );

  list.at(DELAY / 2);
  list.state.set_scroll(0.0, 240.0);
  assert_eq!(list.thumb(), Some(255));
  list.at(DELAY / 2 + DELAY - Duration::from_millis(1));
  assert_eq!(list.thumb(), Some(255), "the delay runs from the last scroll");

  list.at(DELAY / 2 + DELAY + FADE);
  assert_eq!(list.thumb(), None, "then the bar fades out");
}

#[test]
fn an_on_hover_bar_shows_for_the_wheel_and_goes_when_the_pointer_leaves_the_window() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));

  list.tree.scroll(OVER.0, OVER.1, 0.0, -48.0, ScrollPhase::Scroll);
  assert_eq!(list.thumb(), Some(255), "the wheel shows the bar");
  assert!(list.state.scroll_y() > 0.0);

  list.tree.mouse_leave_window();
  assert_eq!(list.thumb(), Some(255));
  list.at(DELAY + FADE);
  assert_eq!(list.thumb(), None);
}

#[test]
fn an_on_hover_bar_stays_while_its_thumb_is_dragged() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));
  list.tree.mouse_move(OVER.0, OVER.1);
  let snapshot = list.render();
  let thumb = snapshot
    .rects
    .iter()
    .find(|rect| rect.color == THUMB)
    .copied()
    .expect("the bar is shown");
  let (x, y) = (thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);

  list.tree.mouse_move(x, y);
  list.tree.mouse_down(x, y, MouseButton::Left);
  list.tree.mouse_move(AWAY.0, y + 30.0);
  assert!(list.state.scroll_y() > 0.0, "the shown thumb drags");
  assert_eq!(list.clicks.load(Ordering::SeqCst), 0, "and its press reaches no button");

  list.at(DELAY + FADE + Duration::from_secs(5));
  assert_eq!(list.thumb(), Some(255), "a held thumb stays, wherever the pointer is");

  list.tree.mouse_up(AWAY.0, y + 30.0, MouseButton::Left);
  // The release ends the drag; the pointer's next move finds it off the list.
  list.tree.mouse_move(AWAY.0, AWAY.1);
  let released = DELAY + FADE + Duration::from_secs(5);
  assert_eq!(list.thumb(), Some(255));
  list.at(released + DELAY + FADE);
  assert_eq!(list.thumb(), None, "let go, it fades out");
}

#[test]
fn a_hidden_on_hover_bar_takes_no_pointer() {
  let mut list = List::new(30, bar(ScrollBarVisibility::OnHover));

  // No move came before the press: the bar was never shown where it lands.
  pointer_click(&mut list.tree, 194.0, 12.0, MouseButton::Left);
  assert_eq!(
    list.clicks.load(Ordering::SeqCst),
    1,
    "the press clicks the button under the bar's place"
  );
  assert_eq!(list.state.scroll_y(), 0.0, "and neither drags nor pages");
}

#[test]
fn an_on_hover_bar_over_content_that_fits_never_shows() {
  let mut list = List::new(3, bar(ScrollBarVisibility::OnHover));
  list.tree.mouse_move(OVER.0, OVER.1);
  assert_eq!(list.thumb(), None, "three rows fit");
  list.tree.mouse_move(AWAY.0, AWAY.1);
  list.render();
  list.at(DELAY / 2);
  assert!(
    render_pass_if_needed(&mut list.tree, &mut list.app).is_none(),
    "and no frame is asked for"
  );
}

#[test]
fn a_never_shown_bar_leaves_the_wheel_and_the_content_working() {
  let mut list = List::new(30, bar(ScrollBarVisibility::Never));
  list.tree.mouse_move(OVER.0, OVER.1);
  assert_eq!(list.thumb(), None, "no bar under the pointer");

  list.tree.mouse_move(194.0, 12.0);
  pointer_click(&mut list.tree, 194.0, 12.0, MouseButton::Left);
  assert_eq!(
    list.clicks.load(Ordering::SeqCst),
    1,
    "the button where a bar would run takes the click"
  );
  assert_eq!(list.state.scroll_y(), 0.0);

  list.tree.scroll(OVER.0, OVER.1, 0.0, -48.0, ScrollPhase::Scroll);
  assert!(list.state.scroll_y() > 0.0, "the wheel scrolls");
  assert_eq!(list.thumb(), None, "and still paints no bar");
}

#[derive(Clone, PartialEq, DevtoolsInspectable)]
struct RowData {
  id: usize,
}

struct Rows;

impl Component for Rows {
  type Props = Vec<RowData>;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let items = ctx.props::<Self::Props>().clone();
    VirtualizedList::new(ctx, items)
      .size(200.0, 100.0)
      .scrollbar(bar(ScrollBarVisibility::OnHover))
      .mount_keyed::<RowView, _, _, _>(|row| row.id, |row| (*row).clone())
  }
}

struct RowView;

impl Component for RowView {
  type Props = RowData;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Rect::new(200.0, 24.0)
  }
}

#[test]
fn a_virtualized_list_shows_its_on_hover_bar_under_the_pointer() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 400);
  let start = Instant::now();
  tree.set_animation_clock(Some(start));
  tree.mount_root::<Rows>(&mut app, (0..200).map(|id| RowData { id }).collect());
  render_pass_with_app(&mut tree, &mut app);
  assert_eq!(thumb_alpha(&render_pass_with_app(&mut tree, &mut app)), None);

  tree.mouse_move(OVER.0, OVER.1);
  assert_eq!(thumb_alpha(&render_pass_with_app(&mut tree, &mut app)), Some(255));

  tree.mouse_move(AWAY.0, AWAY.1);
  render_pass_with_app(&mut tree, &mut app);
  tree.set_animation_clock(Some(start + DELAY + FADE));
  assert_eq!(thumb_alpha(&render_pass_with_app(&mut tree, &mut app)), None);
}
