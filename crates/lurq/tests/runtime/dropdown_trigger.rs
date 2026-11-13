//! Which presses switch dropdowns. The first interactive element under the
//! pointer decides: a `Select`, or a dropdown's anchor, is a trigger and gets
//! the press; anything else inside an anchor (a split button's other half, a
//! status bar's items) is ordinary content, and the press only closes the open
//! dropdown.

use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton},
  components::{Button, Column, OutsidePress, Placement, Popup, Rect, Row, ScrollVertical, Select, Spacer},
  core::{ElementRef, Signal},
  layout::{layout_kind::ScrollState, scrollbar::ScrollBarStyle},
  node::{Element, color::Color},
};

use crate::support::{pointer_click, render_pass};

#[derive(Default)]
struct Counts {
  send: AtomicUsize,
  row: AtomicUsize,
  bar_item: AtomicUsize,
  outer: AtomicUsize,
  other_presses: AtomicUsize,
  other_clicks: AtomicUsize,
  guarded: AtomicUsize,
}

fn count(counter: &AtomicUsize) -> usize {
  counter.load(Ordering::SeqCst)
}

#[derive(Clone)]
struct Signals {
  value: Signal<String>,
  inner_value: Signal<String>,
  split_menu: Signal<bool>,
  row_menu: Signal<bool>,
  bar_menu: Signal<bool>,
  outer: Signal<bool>,
  other: Signal<bool>,
  other_dismisses: Signal<bool>,
  pass: Signal<bool>,
  guarded: Signal<bool>,
}

#[derive(Clone)]
struct Refs {
  select: ElementRef,
  inner_select: ElementRef,
  split: ElementRef,
  row: ElementRef,
  bar: ElementRef,
  outer: ElementRef,
  other: ElementRef,
  pass: ElementRef,
  guarded: ElementRef,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct Props {
  #[devtools_ignore]
  signals: Signals,
  #[devtools_ignore]
  refs: Refs,
  #[devtools_ignore]
  counts: Arc<Counts>,
}

impl PartialEq for Props {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.counts, &other.counts)
  }
}

fn select(value: &Signal<String>, element: &ElementRef, prefix: &str) -> Select<String> {
  Select::new(value.clone())
    .options([
      ("one".to_owned(), format!("{prefix} One")),
      ("two".to_owned(), format!("{prefix} Two")),
    ])
    .width(120.0)
    .height(30.0)
    .ref_element(element.clone())
}

fn toggle(open: &Signal<bool>) -> impl Fn() + Send + Sync + 'static {
  let open = open.clone();
  move || open.update(|open| *open = !*open)
}

fn bump(counts: &Arc<Counts>, counter: fn(&Counts) -> &AtomicUsize) -> impl Fn() + Send + Sync + 'static {
  let counts = counts.clone();
  move || {
    counter(&counts).fetch_add(1, Ordering::SeqCst);
  }
}

fn trigger(id: &str, anchor: &ElementRef) -> Button {
  Button::new(id)
    .id(id)
    .width(80.0)
    .height(30.0)
    .ref_element(anchor.clone())
}

fn popup(anchor: &ElementRef, open: &Signal<bool>, inside: impl Into<Element>) -> Popup {
  Popup::new(anchor.clone(), inside)
    .open(open.clone())
    .placement(Placement::BottomStart)
}

struct Page;

impl Page {
  /// A split button [Send][v] anchoring its menu to the whole row.
  fn split(props: &Props) -> (Row, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let send = bump(&props.counts, |counts| &counts.send);
    let open = toggle(&signals.split_menu);
    let row = Row::new()
      .ref_element(refs.split.clone())
      .child(
        Button::new("Send")
          .id("send")
          .width(60.0)
          .height(30.0)
          .on_click(move |_| send()),
      )
      .child(
        Button::new("v")
          .id("chevron")
          .width(30.0)
          .height(30.0)
          .on_click(move |_| open()),
      );
    (
      row,
      popup(&refs.split, &signals.split_menu, Button::new("Queue").id("queue")),
    )
  }

  fn outer(props: &Props) -> (Button, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let clicked = bump(&props.counts, |counts| &counts.outer);
    let open = toggle(&signals.outer);
    let button = trigger("outer", &refs.outer).on_click(move |_| {
      clicked();
      open();
    });
    let inside = Column::new().child(select(&signals.inner_value, &refs.inner_select, "Inner"));
    (button, popup(&refs.outer, &signals.outer, inside))
  }

  fn other(props: &Props) -> (Button, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let pressed = bump(&props.counts, |counts| &counts.other_presses);
    let clicked = bump(&props.counts, |counts| &counts.other_clicks);
    let open = toggle(&signals.other);
    let button = trigger("other", &refs.other)
      .on_mouse_down(move |_| pressed())
      .on_click(move |_| {
        clicked();
        open();
      });
    let popup = popup(&refs.other, &signals.other, Button::new("O").id("inside-other"))
      .dismiss_on_outside_click(signals.other_dismisses.get());
    (button, popup)
  }

  fn pass(props: &Props) -> (Button, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let open = toggle(&signals.pass);
    let button = trigger("pass", &refs.pass).on_click(move |_| open());
    let popup =
      popup(&refs.pass, &signals.pass, Button::new("P").id("inside-pass")).outside_press(OutsidePress::PassThrough);
    (button, popup)
  }

  /// A trigger that prevents the default of its press.
  fn guarded(props: &Props) -> (Button, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let clicked = bump(&props.counts, |counts| &counts.guarded);
    let open = toggle(&signals.guarded);
    let button = trigger("guarded", &refs.guarded)
      .on_mouse_down(|event: lurq::app::events::MouseEvent| event.prevent_default())
      .on_click(move |_| {
        clicked();
        open();
      });
    (
      button,
      popup(&refs.guarded, &signals.guarded, Button::new("G").id("inside-guarded")),
    )
  }

  /// A list row anchoring a context menu that a right press would open.
  fn row(props: &Props) -> (Button, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let clicked = bump(&props.counts, |counts| &counts.row);
    let row = Button::new("Row")
      .id("row")
      .width(200.0)
      .height(30.0)
      .ref_element(refs.row.clone())
      .on_click(move |_| clicked());
    let menu = popup(&refs.row, &signals.row_menu, Button::new("Rename").id("rename")).switch_on_anchor_press(false);
    (row, menu)
  }

  /// A status bar anchoring a popover, with an item and empty space.
  fn bar(props: &Props) -> (Row, Popup) {
    let (signals, refs) = (&props.signals, &props.refs);
    let clicked = bump(&props.counts, |counts| &counts.bar_item);
    let bar = Row::new()
      .ref_element(refs.bar.clone())
      .child(
        Button::new("Item")
          .id("bar-item")
          .width(80.0)
          .height(30.0)
          .on_click(move |_| clicked()),
      )
      .child(Spacer::new().width(200.0).height(30.0));
    (
      bar,
      popup(&refs.bar, &signals.bar_menu, Button::new("B").id("inside-bar")),
    )
  }
}

impl Component for Page {
  type Props = Props;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Props>().clone();
    let (split, split_popup) = Self::split(&props);
    let (outer, outer_popup) = Self::outer(&props);
    let (other, other_popup) = Self::other(&props);
    let (pass, pass_popup) = Self::pass(&props);
    let (guarded, guarded_popup) = Self::guarded(&props);
    let (row, row_popup) = Self::row(&props);
    let (bar, bar_popup) = Self::bar(&props);
    Column::new()
      .child(
        Row::new()
          .spacing(20.0)
          .child(select(&props.signals.value, &props.refs.select, "A"))
          .child(split)
          .child(split_popup)
          .child(outer)
          .child(outer_popup)
          .child(other)
          .child(other_popup)
          .child(pass)
          .child(pass_popup)
          .child(guarded)
          .child(guarded_popup),
      )
      .child(Spacer::new().height(200.0))
      .child(row)
      .child(row_popup)
      .child(Spacer::new().height(60.0))
      .child(bar)
      .child(bar_popup)
  }
}

struct Fixture {
  app: App,
  tree: Tree,
  signals: Signals,
  refs: Refs,
  counts: Arc<Counts>,
}

impl Fixture {
  fn new() -> Self {
    let signals = Signals {
      value: Signal::new("one".to_owned()),
      inner_value: Signal::new("one".to_owned()),
      split_menu: Signal::new(false),
      row_menu: Signal::new(false),
      bar_menu: Signal::new(false),
      outer: Signal::new(false),
      other: Signal::new(false),
      other_dismisses: Signal::new(true),
      pass: Signal::new(false),
      guarded: Signal::new(false),
    };
    let refs = Refs {
      select: ElementRef::new(),
      inner_select: ElementRef::new(),
      split: ElementRef::new(),
      row: ElementRef::new(),
      bar: ElementRef::new(),
      outer: ElementRef::new(),
      other: ElementRef::new(),
      pass: ElementRef::new(),
      guarded: ElementRef::new(),
    };
    let counts = Arc::new(Counts::default());
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.mount_root::<Page>(
      &mut app,
      Props {
        signals: signals.clone(),
        refs: refs.clone(),
        counts: counts.clone(),
      },
    );
    let mut fixture = Self {
      app,
      tree,
      signals,
      refs,
      counts,
    };
    fixture.pass();
    fixture
  }

  fn pass(&mut self) {
    for _ in 0..2 {
      self.tree.request_redraw();
      self.tree.pass_headless(&mut self.app);
    }
  }

  fn click_at(&mut self, (x, y): (f32, f32)) {
    pointer_click(&mut self.tree, x, y, MouseButton::Left);
    self.pass();
  }

  fn click(&mut self, id: &str) {
    let point = self
      .tree
      .get_element_by_id_mut(id)
      .and_then(|element| element.bounds())
      .unwrap_or_else(|| panic!("#{id} should be laid out"))
      .center();
    self.click_at(point);
  }

  fn menu_open(&mut self, prefix: &str) -> bool {
    let label = format!("{prefix} Two");
    self
      .tree
      .find_element(|element| element.text_content() == Some(label.as_str()))
      .is_some()
  }

  fn open_select(&mut self) {
    self.click_at(self.refs.select.bounds().center());
    assert!(self.menu_open("A"), "select A opens");
  }

  fn open_inner_select(&mut self) {
    self.click("outer");
    assert!(self.signals.outer.get());
    self.click_at(self.refs.inner_select.bounds().center());
    assert!(self.menu_open("Inner"), "the select in the popover opens");
    assert!(self.signals.outer.get(), "the popover stays open");
  }
}

#[test]
fn the_other_half_of_a_split_button_anchor_is_not_a_trigger() {
  let mut fixture = Fixture::new();
  fixture.open_select();

  fixture.click("send");

  assert!(!fixture.menu_open("A"));
  assert_eq!(count(&fixture.counts.send), 0, "the closing press does not send");
  fixture.click("send");
  assert_eq!(count(&fixture.counts.send), 1, "the next press sends");
}

#[test]
fn a_row_anchoring_a_context_menu_is_not_a_trigger() {
  let mut fixture = Fixture::new();
  fixture.open_select();

  fixture.click("row");

  assert!(!fixture.menu_open("A"));
  assert_eq!(
    count(&fixture.counts.row),
    0,
    "the closing press does not click the row"
  );
  assert!(!fixture.signals.row_menu.get());
}

#[test]
fn presses_inside_a_container_anchor_only_close_the_dropdown() {
  let mut fixture = Fixture::new();
  fixture.open_select();
  fixture.click("bar-item");
  assert!(!fixture.menu_open("A"));
  assert_eq!(count(&fixture.counts.bar_item), 0, "the bar's item is not clicked");

  fixture.open_select();
  let bar = fixture.refs.bar.bounds();
  fixture.click_at((bar.x + bar.width - 20.0, bar.y + bar.height / 2.0));
  assert!(!fixture.menu_open("A"));
  assert!(
    fixture.refs.select.focused(),
    "the press on the bar's empty space is consumed, so the select keeps focus"
  );
}

#[test]
fn a_trigger_that_prevents_the_default_still_closes_the_open_dropdowns() {
  let mut fixture = Fixture::new();
  fixture.open_select();
  fixture.click("guarded");
  assert!(!fixture.menu_open("A"), "the select menu closes");
  assert!(fixture.signals.guarded.get(), "the trigger opens its popover");

  fixture.click("guarded");
  fixture.click("outer");
  fixture.click("guarded");
  assert!(!fixture.signals.outer.get(), "the popover closes");
  assert!(fixture.signals.guarded.get());
  assert_eq!(count(&fixture.counts.guarded), 3);
}

#[test]
fn the_open_popovers_own_trigger_closes_it_and_the_select_in_it() {
  let mut fixture = Fixture::new();
  fixture.open_inner_select();

  fixture.click("outer");

  assert!(!fixture.signals.outer.get(), "the popover closes");
  assert!(!fixture.menu_open("Inner"), "and so does the select menu in it");
  assert_eq!(
    count(&fixture.counts.outer),
    2,
    "the trigger got the press: open, then close"
  );
}

#[test]
fn a_page_trigger_closes_a_popover_and_the_select_in_it() {
  let mut fixture = Fixture::new();
  fixture.open_inner_select();

  fixture.click("other");

  assert!(!fixture.signals.outer.get());
  assert!(!fixture.menu_open("Inner"));
  assert!(fixture.signals.other.get(), "the pressed trigger opens its popover");
}

#[test]
fn an_anchor_stops_being_a_trigger_when_its_popup_stops_dismissing() {
  let mut fixture = Fixture::new();
  fixture.signals.other_dismisses.set(false);
  fixture.pass();
  fixture.open_select();
  fixture.click("other");
  assert!(!fixture.menu_open("A"));
  assert!(!fixture.signals.other.get(), "the press is consumed");
  assert_eq!(count(&fixture.counts.other_clicks), 0);

  fixture.signals.other_dismisses.set(true);
  fixture.pass();
  fixture.open_select();
  fixture.click("other");
  assert!(!fixture.menu_open("A"));
  assert!(fixture.signals.other.get(), "the anchor is a trigger again");
}

#[test]
fn a_pass_through_popover_switches_like_the_others() {
  let mut fixture = Fixture::new();
  fixture.click("pass");
  fixture.open_select();
  assert!(!fixture.signals.pass.get(), "the select's press closes it");
  assert!(fixture.menu_open("A"));

  fixture.click("pass");
  assert!(!fixture.menu_open("A"));
  assert!(fixture.signals.pass.get());

  fixture.click("other");
  assert!(!fixture.signals.pass.get());
  assert!(fixture.signals.other.get());
}

#[test]
fn a_switching_press_reaches_the_trigger_exactly_once() {
  let mut fixture = Fixture::new();
  fixture.open_select();

  fixture.click("other");

  assert_eq!(count(&fixture.counts.other_presses), 1);
  assert_eq!(count(&fixture.counts.other_clicks), 1);
  assert!(fixture.signals.other.get());

  fixture.click("outer");
  assert_eq!(
    count(&fixture.counts.other_clicks),
    1,
    "switching away does not click it"
  );
  assert_eq!(count(&fixture.counts.outer), 1);
}

/// The default style with a painted track, so the bar owns its lane.
fn tracked_bar() -> ScrollBarStyle {
  ScrollBarStyle {
    track_color: Color::from_hex("#e5e7eb"),
    ..ScrollBarStyle::default()
  }
}

#[test]
#[cfg_attr(
  target_os = "ios",
  ignore = "a desktop's scrollbar; a phone's default is an indicator that fades"
)]
fn a_press_on_a_scrollbar_over_a_trigger_is_consumed() {
  let state = ScrollState::new();
  let open_select = ElementRef::new();
  let mut rows = Column::new().spacing(0.0);
  for row in 0..30 {
    rows = rows.child(if row == 3 {
      select(&Signal::new("one".to_owned()), &ElementRef::new(), "Listed")
        .width(200.0)
        .height(24.0)
        .into()
    } else {
      Element::from(Rect::new(200.0, 24.0))
    });
  }
  let mut tree = Tree::new();
  tree.set_root(
    Row::new()
      .child(
        ScrollVertical::new(rows)
          .with_scroll_state(state.clone())
          .scrollbar(tracked_bar())
          .size(200.0, 100.0),
      )
      .child(Spacer::new().width(100.0))
      .child(select(&Signal::new("one".to_owned()), &open_select, "A")),
  );
  render_pass(&mut tree);
  let (x, y) = open_select.bounds().center();
  pointer_click(&mut tree, x, y, MouseButton::Left);
  render_pass(&mut tree);
  assert!(open_select.focused());

  // Row 3 (the listed select) spans y 72..96, under the track below the thumb.
  pointer_click(&mut tree, 194.0, 84.0, MouseButton::Left);
  render_pass(&mut tree);

  assert_eq!(state.scroll_y(), 0.0, "the closing press does not page the list");
  assert!(open_select.focused(), "the press is consumed");
}
