//! A frame for a hover is asked for when the hover shows, and only then. The hover moves whenever the pointer
//! comes onto another node, and most nodes look the same under it: such a move keeps the hover, the cursor and
//! what the app reads of them up to date and draws nothing. A node that is drawn differently under the pointer
//! (a `hovered` style, a checkbox, a slider, a select, a link, a scrollbar) or runs a handler for the pointer
//! coming or going still gets its frame.

use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::{Duration, Instant},
};

use lurq::{
  animation::{Easing, Transition},
  app::{
    App, SyntheticInput, Tree,
    component::Component,
    ctx::Ctx,
    events::{MouseButton, ScrollPhase},
    synthetic_input,
  },
  components::{Checkbox, Column, Rect, Row, ScrollVertical, Select, SelectOption, Slider, Text},
  core::{ElementRef, Signal},
  layout::scrollbar::{ScrollBarStyle, ScrollBarVisibility},
  node::{CursorIcon, Element, SelectPartStyle, SelectStyle, color::Color, interaction_state::InteractionState},
};

use crate::support::{RenderSnapshot, pointer_click, render_pass_if_needed, render_pass_with_app};

const REST: &str = "#334155";
const HOVER: &str = "#22c55e";
const OTHER: &str = "#0ea5e9";
const THUMB: Color = Color::new(200, 30, 90, 255);
const THUMB_HOVER: Color = Color::new(30, 200, 90, 255);

/// Draws the first frame and the ones it asks for, until the tree asks for none.
fn settle(tree: &mut Tree, app: &mut App) {
  render_pass_with_app(tree, app);
  for _ in 0..4 {
    if !tree.needs_redraw() {
      return;
    }
    render_pass_if_needed(tree, app);
  }
  assert!(!tree.needs_redraw(), "nothing is left to draw before the pointer moves");
}

/// A 400 by 300 window showing `root`, with nothing left to draw.
fn shown(root: impl Into<Element>) -> (Tree, App) {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.set_root(root);
  settle(&mut tree, &mut app);
  (tree, app)
}

/// The middle of the element with the id.
fn middle(tree: &mut Tree, id: &str) -> (f32, f32) {
  tree
    .get_element_by_id_mut(id)
    .and_then(|element| element.bounds())
    .unwrap_or_else(|| panic!("{id} is laid out"))
    .center()
}

fn move_onto(tree: &mut Tree, id: &str) {
  let (x, y) = middle(tree, id);
  tree.mouse_move(x, y);
}

/// The frame the tree asks for; `what` must have asked for one.
fn frame(tree: &mut Tree, app: &mut App, what: &str) -> RenderSnapshot {
  assert!(tree.needs_redraw(), "{what} asks for a frame");
  render_pass_if_needed(tree, app).unwrap_or_else(|| panic!("{what} draws a frame"))
}

/// `what` asked for no frame, and a pass draws none.
fn no_frame(tree: &mut Tree, app: &mut App, what: &str) {
  assert!(!tree.needs_redraw(), "{what} asks for no frame");
  assert!(render_pass_if_needed(tree, app).is_none(), "{what} draws no frame");
}

fn has_fill(snapshot: &RenderSnapshot, color: Color) -> bool {
  snapshot.rects.iter().any(|rect| rect.color == color)
}

fn fill(hex: &str) -> Color {
  Color::from_hex(hex)
}

#[test]
fn a_hover_that_shows_nothing_asks_for_no_frame() {
  let (first, second) = (ElementRef::new(), ElementRef::new());
  let state = InteractionState::new();
  let (mut tree, mut app) = shown(
    Column::new()
      .child(
        Row::new()
          .id("first")
          .size(200.0, 40.0)
          .background(REST)
          .cursor(CursorIcon::Pointer)
          .ref_element(first.clone())
          .child(Text::new("First").id("words")),
      )
      .child(
        Rect::new(200.0, 40.0)
          .id("second")
          .background(REST)
          .ref_element(second.clone())
          .interactive(state.clone()),
      ),
  );

  move_onto(&mut tree, "first");
  assert!(first.hovered(), "the element is hovered");
  assert_eq!(tree.cursor(), CursorIcon::Pointer, "and gives the pointer its cursor");
  no_frame(
    &mut tree,
    &mut app,
    "the pointer coming onto an element that shows no hover",
  );

  move_onto(&mut tree, "words");
  assert!(first.hovered());
  no_frame(&mut tree, &mut app, "the pointer coming onto the text inside it");

  move_onto(&mut tree, "second");
  assert!(!first.hovered() && second.hovered(), "the hover moved");
  assert!(state.is_hovered(), "and the app reads it without a frame");
  assert_eq!(tree.cursor(), CursorIcon::Default);
  no_frame(&mut tree, &mut app, "the pointer moving onto another such element");

  tree.mouse_leave_window();
  assert!(!second.hovered() && !state.is_hovered(), "nothing is hovered");
  no_frame(&mut tree, &mut app, "the pointer leaving the window");
}

#[test]
fn a_hovered_style_is_drawn_when_the_pointer_comes_and_when_it_goes() {
  let (mut tree, mut app) = shown(
    Column::new()
      .child(
        Rect::new(200.0, 40.0)
          .id("styled")
          .background(REST)
          .hovered(|style| style.background(HOVER)),
      )
      .child(Rect::new(200.0, 40.0).id("plain")),
  );

  move_onto(&mut tree, "styled");
  let hovered = frame(&mut tree, &mut app, "the pointer coming onto a hovered style");
  assert!(has_fill(&hovered, fill(HOVER)) && !has_fill(&hovered, fill(REST)));

  move_onto(&mut tree, "plain");
  let rest = frame(&mut tree, &mut app, "the pointer leaving a hovered style");
  assert!(has_fill(&rest, fill(REST)) && !has_fill(&rest, fill(HOVER)));

  move_onto(&mut tree, "styled");
  frame(&mut tree, &mut app, "the pointer coming back");
  tree.mouse_leave_window();
  let left = frame(
    &mut tree,
    &mut app,
    "the pointer leaving the window from a hovered style",
  );
  assert!(has_fill(&left, fill(REST)) && !has_fill(&left, fill(HOVER)));
}

#[test]
fn a_move_inside_a_hovered_element_asks_for_no_frame() {
  let (mut tree, mut app) = shown(
    Row::new()
      .id("row")
      .size(300.0, 40.0)
      .background(REST)
      .hovered(|style| style.background(HOVER))
      .child(Rect::new(40.0, 40.0).id("icon"))
      .child(Text::new("Label").id("label")),
  );

  move_onto(&mut tree, "icon");
  let hovered = frame(&mut tree, &mut app, "the pointer coming onto the row");
  assert!(has_fill(&hovered, fill(HOVER)));

  move_onto(&mut tree, "label");
  no_frame(
    &mut tree,
    &mut app,
    "the pointer moving from the row's icon to its label",
  );
}

#[test]
fn a_hovered_style_that_sizes_its_element_is_laid_out_and_drawn() {
  let (mut tree, mut app) = shown(
    Column::new()
      .child(
        Rect::new(100.0, 40.0)
          .id("grows")
          .background(REST)
          .hovered(|style| style.width(160.0)),
      )
      .child(Rect::new(100.0, 40.0).id("plain")),
  );
  let width = |tree: &mut Tree| {
    tree
      .get_element_by_id_mut("grows")
      .and_then(|element| element.bounds())
      .expect("laid out")
      .width
  };

  move_onto(&mut tree, "grows");
  frame(
    &mut tree,
    &mut app,
    "the pointer coming onto a style that sizes its element",
  );
  assert_eq!(width(&mut tree), 160.0);

  move_onto(&mut tree, "plain");
  frame(&mut tree, &mut app, "the pointer leaving it");
  assert_eq!(width(&mut tree), 100.0);
}

#[test]
fn a_hover_that_dirties_layout_asks_for_a_frame() {
  // A state style that sizes the element lays it out again when any of its states changes, the hover too.
  let (mut tree, _app) = shown(
    Rect::new(100.0, 40.0)
      .id("pressable")
      .active(|style| style.width(160.0)),
  );

  move_onto(&mut tree, "pressable");
  assert!(tree.needs_redraw(), "the layout it dirtied is drawn");
}

#[test]
fn a_hover_transition_starts_in_the_frame_the_hover_asks_for() {
  let (mut tree, mut app) = shown(
    Rect::new(100.0, 40.0)
      .id("fades")
      .background(REST)
      .transition(Transition::background_color().duration_ms(400).easing(Easing::Linear))
      .hovered(|style| style.background(HOVER)),
  );
  let start = Instant::now();
  tree.set_animation_clock(Some(start));

  move_onto(&mut tree, "fades");
  frame(
    &mut tree,
    &mut app,
    "the pointer coming onto a hovered style with a transition",
  );
  assert!(tree.has_active_timeline(), "the transition runs");

  tree.set_animation_clock(Some(start + Duration::from_millis(200)));
  let halfway = render_pass_with_app(&mut tree, &mut app);
  assert!(
    !has_fill(&halfway, fill(REST)) && !has_fill(&halfway, fill(HOVER)),
    "halfway, the fill is between its two colors"
  );
}

#[test]
fn a_hover_handler_that_runs_asks_for_a_frame() {
  // The handlers count their runs and change nothing that is shown.
  let (enters, leaves) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
  let (entered, left) = (enters.clone(), leaves.clone());
  let (mut tree, mut app) = shown(
    Column::new()
      .child(Rect::new(200.0, 40.0).id("enter").on_mouse_enter(move || {
        entered.fetch_add(1, Ordering::SeqCst);
      }))
      .child(Rect::new(200.0, 40.0).id("leave").on_mouse_leave(move || {
        left.fetch_add(1, Ordering::SeqCst);
      }))
      .child(Rect::new(200.0, 40.0).id("plain")),
  );

  move_onto(&mut tree, "enter");
  assert_eq!(enters.load(Ordering::SeqCst), 1);
  frame(&mut tree, &mut app, "an on_mouse_enter that ran");

  // No handler runs: the first element has none for a leave, the second none for an enter.
  move_onto(&mut tree, "leave");
  no_frame(&mut tree, &mut app, "a move that runs no handler");

  move_onto(&mut tree, "plain");
  assert_eq!(leaves.load(Ordering::SeqCst), 1);
  frame(&mut tree, &mut app, "an on_mouse_leave that ran");
}

/// A row that shows a tooltip under it while the pointer is over it.
struct WithTooltip {
  open: Signal<bool>,
}

impl Component for WithTooltip {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      open: ctx.signal(false),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let (enter, leave) = (self.open.clone(), self.open.clone());
    let row = Rect::new(200.0, 40.0)
      .id("anchor")
      .on_mouse_enter(move || enter.set(true))
      .on_mouse_leave(move || leave.set(false));
    let mut column = Column::new().child(row);
    if self.open.get() {
      column = column.child(Rect::new(120.0, 20.0).background(HOVER));
    }
    column.child(Rect::new(200.0, 40.0).id("plain"))
  }
}

#[test]
fn a_tooltip_a_hover_handler_opens_is_drawn() {
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.mount_root::<WithTooltip>(&mut app, ());
  settle(&mut tree, &mut app);

  move_onto(&mut tree, "anchor");
  let open = frame(&mut tree, &mut app, "the handler that opens the tooltip");
  assert!(has_fill(&open, fill(HOVER)), "the tooltip is drawn");

  move_onto(&mut tree, "plain");
  let closed = frame(&mut tree, &mut app, "the handler that closes it");
  assert!(!has_fill(&closed, fill(HOVER)), "the tooltip is gone");
}

#[test]
fn a_checkbox_draws_its_hovered_box() {
  let (mut tree, mut app) = shown(
    Column::new()
      .child(
        Checkbox::new(Signal::new(false))
          .id("check")
          .size(20.0, 20.0)
          .background(REST)
          .box_hovered(|style| style.background(HOVER)),
      )
      .child(Rect::new(200.0, 40.0).id("plain")),
  );

  move_onto(&mut tree, "check");
  let hovered = frame(&mut tree, &mut app, "the pointer coming onto a checkbox");
  assert!(has_fill(&hovered, fill(HOVER)));

  move_onto(&mut tree, "plain");
  let rest = frame(&mut tree, &mut app, "the pointer leaving a checkbox");
  assert!(has_fill(&rest, fill(REST)) && !has_fill(&rest, fill(HOVER)));
}

#[test]
fn a_slider_draws_its_hovered_parts() {
  let (mut tree, mut app) = shown(
    Column::new()
      .child(
        Slider::new(Signal::new(5))
          .id("slider")
          .range(0, 10)
          .width(100.0)
          .height(20.0)
          .thumb(|style| style.size(10.0, 10.0).background(REST))
          .thumb_hovered(|style| style.background(HOVER)),
      )
      .child(Rect::new(200.0, 40.0).id("plain")),
  );

  move_onto(&mut tree, "slider");
  let hovered = frame(&mut tree, &mut app, "the pointer coming onto a slider");
  assert!(has_fill(&hovered, fill(HOVER)));

  move_onto(&mut tree, "plain");
  let rest = frame(&mut tree, &mut app, "the pointer leaving a slider");
  assert!(has_fill(&rest, fill(REST)) && !has_fill(&rest, fill(HOVER)));
}

#[test]
fn a_select_draws_its_hovered_trigger_and_its_hovered_option() {
  let style = SelectStyle::new()
    .trigger_hovered(SelectPartStyle::new().background(fill(OTHER)))
    .option_hovered(SelectPartStyle::new().background(fill(HOVER)));
  let options: Vec<_> = [("sm", "Small"), ("md", "Medium"), ("lg", "Large")]
    .into_iter()
    .map(|(value, label)| SelectOption::new(value.to_owned(), label))
    .collect();
  let select = Select::new(Signal::new("sm".to_owned()))
    .options(options)
    .width(200.0)
    .height(40.0)
    .style(style)
    .trigger(|_| Text::new("Pick"));
  let (mut tree, mut app) = shown(Element::from(select).id("select"));
  let option = |tree: &mut Tree, label: &str| {
    tree
      .find_element(|element| element.text_content() == Some(label))
      .unwrap_or_else(|| panic!("{label} is laid out"))
      .bounds()
      .center()
  };

  move_onto(&mut tree, "select");
  let hovered = frame(&mut tree, &mut app, "the pointer coming onto a select");
  assert!(has_fill(&hovered, fill(OTHER)), "the trigger's hovered part is drawn");

  let (x, y) = middle(&mut tree, "select");
  pointer_click(&mut tree, x, y, MouseButton::Left);
  settle(&mut tree, &mut app);

  let (x, y) = option(&mut tree, "Medium");
  tree.mouse_move(x, y);
  let medium = frame(&mut tree, &mut app, "the pointer coming onto an option");
  assert!(has_fill(&medium, fill(HOVER)), "the option's hovered part is drawn");

  let (x, y) = option(&mut tree, "Large");
  tree.mouse_move(x, y);
  frame(&mut tree, &mut app, "the pointer moving onto the next option");
}

/// A 200 by 100 list of thirty 24 px rows with the scrollbar `style`.
fn list(style: ScrollBarStyle) -> ScrollVertical {
  let mut rows = Column::new().spacing(0.0);
  for index in 0..30 {
    rows = rows.child(Rect::new(180.0, 24.0).id(format!("row-{index}")));
  }
  ScrollVertical::new(rows).scrollbar(style).size(200.0, 100.0)
}

fn bar(visible: ScrollBarVisibility) -> ScrollBarStyle {
  ScrollBarStyle {
    thumb_color: THUMB,
    visible,
    ..ScrollBarStyle::default()
  }
}

/// The thumb as painted: its middle, and its color without its alpha.
fn thumb(snapshot: &RenderSnapshot, color: Color) -> Option<(f32, f32)> {
  snapshot
    .rects
    .iter()
    .find(|rect| (rect.color.r(), rect.color.g(), rect.color.b()) == (color.r(), color.g(), color.b()))
    .map(|rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
}

#[test]
fn a_scrollbar_shown_on_hover_is_drawn_when_the_pointer_enters_and_leaves_its_container() {
  let (mut tree, mut app) = shown(
    Column::new()
      .child(list(bar(ScrollBarVisibility::OnHover)))
      .child(Rect::new(200.0, 40.0).id("outside")),
  );
  tree.set_animation_clock(Some(Instant::now()));
  assert!(thumb(&render_pass_with_app(&mut tree, &mut app), THUMB).is_none());
  settle(&mut tree, &mut app);

  move_onto(&mut tree, "row-1");
  let entered = frame(&mut tree, &mut app, "the pointer entering a scroll container");
  assert!(thumb(&entered, THUMB).is_some(), "the bar shows");

  move_onto(&mut tree, "row-2");
  no_frame(&mut tree, &mut app, "the pointer moving onto another row of the list");

  move_onto(&mut tree, "outside");
  let left = frame(&mut tree, &mut app, "the pointer leaving the scroll container");
  assert!(thumb(&left, THUMB).is_some(), "the bar stays for its delay");
}

#[test]
fn a_scrollbar_loses_its_hovered_style_with_the_pointer() {
  let (mut tree, mut app) = shown(
    Row::new()
      .child(list(bar(ScrollBarVisibility::Auto)).scrollbar_hovered(|style| style.with_thumb_color(THUMB_HOVER)))
      .child(Rect::new(100.0, 100.0).id("outside")),
  );
  let (x, y) = thumb(&render_pass_with_app(&mut tree, &mut app), THUMB).expect("the bar is painted");
  settle(&mut tree, &mut app);

  tree.mouse_move(x, y);
  let on_thumb = frame(&mut tree, &mut app, "the pointer coming onto the thumb");
  assert!(
    thumb(&on_thumb, THUMB_HOVER).is_some(),
    "the thumb's hovered style is drawn"
  );

  // Straight from the thumb out of the container: no move tells the scrollbar that the pointer left it.
  move_onto(&mut tree, "outside");
  let off_thumb = frame(&mut tree, &mut app, "the pointer leaving the container from its thumb");
  assert!(thumb(&off_thumb, THUMB).is_some() && thumb(&off_thumb, THUMB_HOVER).is_none());
}

#[test]
fn rows_a_scroll_moves_under_a_still_pointer_are_drawn_once() {
  let (mut tree, mut app) = shown(list(bar(ScrollBarVisibility::Auto)));
  move_onto(&mut tree, "row-1");
  let (x, y) = middle(&mut tree, "row-1");
  settle(&mut tree, &mut app);

  tree.scroll(x, y, 0.0, -48.0, ScrollPhase::Scroll);
  frame(&mut tree, &mut app, "the scroll");
  no_frame(&mut tree, &mut app, "the row the scroll brought under the pointer");
}

#[test]
fn a_selection_drag_is_drawn_and_the_pointer_over_selectable_text_is_not() {
  let (mut tree, mut app) = shown(Text::new("Hello world").id("words").selectable(true));
  let bounds = tree
    .get_element_by_id_mut("words")
    .and_then(|element| element.bounds())
    .expect("laid out");
  let y = bounds.y + bounds.height / 2.0;

  tree.mouse_move(bounds.x + 2.0, y);
  no_frame(&mut tree, &mut app, "the pointer over selectable text");

  tree.mouse_down(bounds.x, y, MouseButton::Left);
  settle(&mut tree, &mut app);
  tree.mouse_move(bounds.x + bounds.width, y);
  frame(&mut tree, &mut app, "a move that extends the selection");
  assert_eq!(tree.selected_text().as_deref(), Some("Hello world"));
}

#[test]
fn a_drag_session_is_drawn_on_every_move() {
  let (mut tree, mut app) = shown(Rect::new(100.0, 40.0).id("handle").on_drag_move(|_| {}));
  let (x, y) = middle(&mut tree, "handle");
  tree.mouse_down(x, y, MouseButton::Left);
  settle(&mut tree, &mut app);

  for step in 1..=3 {
    tree.mouse_move(x + step as f32, y);
    frame(&mut tree, &mut app, "a move of the drag");
  }
}

#[test]
fn the_perf_overlay_keeps_its_frames_over_a_hover_that_shows_nothing() {
  let (mut tree, mut app) = shown(Rect::new(200.0, 40.0).id("plain"));
  move_onto(&mut tree, "plain");
  no_frame(&mut tree, &mut app, "the pointer coming onto a plain element");

  tree.draw_perf_overlay();
  settle(&mut tree, &mut app);
  tree.mouse_leave_window();
  assert!(
    render_pass_if_needed(&mut tree, &mut app).is_some(),
    "the overlay is drawn whatever the pointer does"
  );
}

#[test]
fn synthetic_pointer_moves_follow_the_same_rule() {
  let (mut tree, mut app) = shown(
    Column::new().child(Rect::new(200.0, 40.0).id("plain")).child(
      Rect::new(200.0, 40.0)
        .id("styled")
        .background(REST)
        .hovered(|style| style.background(HOVER)),
    ),
  );

  let (x, y) = middle(&mut tree, "plain");
  synthetic_input::apply(&mut tree, &SyntheticInput::mouse_move(x, y));
  no_frame(&mut tree, &mut app, "a synthetic move onto a plain element");

  let (x, y) = middle(&mut tree, "styled");
  synthetic_input::apply(&mut tree, &SyntheticInput::mouse_move(x, y));
  let hovered = frame(&mut tree, &mut app, "a synthetic move onto a hovered style");
  assert!(has_fill(&hovered, fill(HOVER)));
}

#[test]
fn a_root_replaced_under_the_pointer_is_drawn() {
  let (mut tree, mut app) = shown(Rect::new(200.0, 40.0).id("plain"));
  move_onto(&mut tree, "plain");
  no_frame(&mut tree, &mut app, "the pointer coming onto a plain element");

  tree.set_root(Rect::new(200.0, 40.0).background(HOVER));
  let replaced = frame(&mut tree, &mut app, "the root replaced under the pointer");
  assert!(has_fill(&replaced, fill(HOVER)));
}

#[cfg(feature = "markdown")]
mod markdown_links {
  use lurq::{
    app::theme::MarkdownLinkState,
    components::{Markdown, MarkdownLinkPresentation, MarkdownProps},
  };

  use super::*;

  struct Document;

  impl Component for Document {
    type Props = MarkdownProps;

    fn create(_ctx: &mut Ctx) -> Self {
      Self
    }

    fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
      Markdown::mount(ctx, ctx.props::<Self::Props>().clone())
    }
  }

  #[test]
  fn a_markdown_link_draws_its_hovered_look() {
    let hovered = MarkdownLinkState::new().background(fill(HOVER));
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(700, 300);
    tree.mount_root::<Document>(
      &mut app,
      MarkdownProps::new("Open [PR 42](https://example.com/pull/42) now.")
        .link_presentation(move |_| MarkdownLinkPresentation::link().hovered(hovered))
        .width(600.0),
    );
    settle(&mut tree, &mut app);
    let link = tree
      .find_element(|element| element.text_content() == Some("PR 42"))
      .expect("the link is laid out")
      .bounds();

    tree.mouse_move(link.x + link.width / 2.0, link.y + link.height / 2.0);
    let over = frame(&mut tree, &mut app, "the pointer coming onto a link");
    assert!(has_fill(&over, fill(HOVER)), "the link's hovered fill is drawn");

    tree.mouse_move(link.x + link.width + 300.0, link.y + link.height / 2.0);
    let off = frame(&mut tree, &mut app, "the pointer leaving a link");
    assert!(!has_fill(&off, fill(HOVER)));
  }
}
