//! A disabled element and everything in it take no input. It still covers
//! what is behind it, and a press on it reaches nothing.

use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{
    App, Tree,
    events::{MouseButton, ScrollPhase},
  },
  components::{Button, Checkbox, Column, Rect, Row, ScrollVertical, Spacer, Stack, Text},
  core::Signal,
  layout::quad::QuadContent,
  node::{Element, color::Color, interaction_state::InteractionState},
};

use crate::support::{TestSurface, pointer_click};

const RED: Color = Color::new(200, 30, 30, 255);
const GREY: Color = Color::new(160, 160, 160, 255);
const FILL: Color = Color::new(240, 240, 240, 255);

fn mounted(app: &mut App, root: impl Into<Element>) -> Tree {
  let mut tree = Tree::new();
  tree.set_root(root);
  tree.pass(app, &TestSurface);
  tree
}

fn counter() -> (Arc<AtomicUsize>, impl Fn() + Clone + Send + Sync + 'static) {
  let count = Arc::new(AtomicUsize::new(0));
  let bump = {
    let count = count.clone();
    move || {
      count.fetch_add(1, Ordering::SeqCst);
    }
  };
  (count, bump)
}

fn clicks(count: &Arc<AtomicUsize>) -> usize {
  count.load(Ordering::SeqCst)
}

fn center(tree: &mut Tree, id: &str) -> (f32, f32) {
  tree
    .get_element_by_id_mut(id)
    .and_then(|element| element.bounds())
    .expect("element should be laid out")
    .center()
}

fn click(tree: &mut Tree, id: &str) {
  let (x, y) = center(tree, id);
  pointer_click(tree, x, y, MouseButton::Left);
}

fn tab(tree: &mut Tree) {
  tree.key_down("Tab".to_owned(), "Tab".to_owned(), false, false, false);
  tree.key_up("Tab".to_owned(), "Tab".to_owned(), false, false, false);
}

fn focused_id(tree: &Tree) -> Option<String> {
  tree
    .focused_element()
    .and_then(|element| element.id().map(str::to_owned))
}

fn button(id: &str, on_click: impl Fn() + Clone + Send + Sync + 'static) -> Button {
  Button::new(id)
    .id(id)
    .size(120.0, 32.0)
    .tab_index(0)
    .on_click(move |_| on_click())
}

#[test]
fn a_click_on_a_disabled_element_runs_no_handler() {
  let (enabled, bump_enabled) = counter();
  let (disabled, bump_disabled) = counter();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .child(button("on", bump_enabled))
      .child(button("off", bump_disabled).disabled(true)),
  );

  click(&mut tree, "on");
  click(&mut tree, "off");

  assert_eq!((clicks(&enabled), clicks(&disabled)), (1, 0));
}

#[test]
fn everything_in_a_disabled_element_is_disabled_with_it() {
  let (inner, bump) = counter();
  let checked = Signal::new(false);
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .disabled(true)
      .child(button("inner", bump))
      .child(Checkbox::new(checked.clone()).id("check").size(20.0, 20.0)),
  );

  click(&mut tree, "inner");
  click(&mut tree, "check");
  tree.pass(&mut app, &TestSurface);

  assert_eq!(clicks(&inner), 0);
  assert!(!checked.get(), "a checkbox in a disabled element does not toggle");
}

#[test]
fn a_click_on_a_disabled_element_does_not_reach_the_elements_it_is_in() {
  let (card, bump_card) = counter();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .id("card")
      .padding(20.0)
      .on_click(move |_| bump_card())
      .child(button("delete", || {}).disabled(true))
      .child(Rect::new(120.0, 32.0).id("body")),
  );

  click(&mut tree, "delete");
  assert_eq!(clicks(&card), 0, "the card is not clicked through its disabled button");

  click(&mut tree, "body");
  assert_eq!(clicks(&card), 1);
}

#[test]
fn a_disabled_element_covers_what_is_behind_it() {
  let (behind, bump) = counter();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Stack::new()
      .size(120.0, 32.0)
      .child(button("behind", bump))
      .child(Rect::new(120.0, 32.0).id("cover").background(FILL).disabled(true)),
  );

  click(&mut tree, "cover");

  assert_eq!(clicks(&behind), 0);
}

#[test]
fn a_disabled_element_is_not_hovered_or_pressed_but_the_elements_it_is_in_stay_hovered() {
  let row = InteractionState::new();
  let item = InteractionState::new();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Row::new().id("row").padding(10.0).interactive(row.clone()).child(
      Rect::new(120.0, 32.0)
        .id("item")
        .interactive(item.clone())
        .disabled(true),
    ),
  );
  let (x, y) = center(&mut tree, "item");

  tree.mouse_move(x, y);
  assert!(row.is_hovered(), "the row is hovered through its disabled item");
  assert!(!item.is_hovered());

  tree.mouse_down(x, y, MouseButton::Left);
  assert!(row.is_hovered(), "a press on the item leaves the row hovered");
  assert!(!row.is_active() && !item.is_active(), "and presses nothing");
  tree.mouse_up(x, y, MouseButton::Left);
  assert!(row.is_hovered());
}

#[test]
fn a_press_on_a_disabled_element_leaves_focus_where_it_is() {
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .child(button("first", || {}))
      .child(button("off", || {}).disabled(true)),
  );
  tab(&mut tree);
  assert_eq!(focused_id(&tree).as_deref(), Some("first"));

  click(&mut tree, "off");

  assert_eq!(focused_id(&tree).as_deref(), Some("first"));
}

#[test]
fn tab_passes_a_disabled_element_and_what_is_in_it() {
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .child(button("first", || {}))
      .child(button("off", || {}).disabled(true))
      .child(Row::new().disabled(true).child(button("inside", || {})))
      .child(button("last", || {})),
  );

  tab(&mut tree);
  assert_eq!(focused_id(&tree).as_deref(), Some("first"));
  tab(&mut tree);
  assert_eq!(focused_id(&tree).as_deref(), Some("last"));
  tab(&mut tree);
  assert_eq!(focused_id(&tree).as_deref(), Some("first"));
}

#[test]
fn a_disabled_element_takes_no_focus_and_no_programmatic_click() {
  let (count, bump) = counter();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new().child(Row::new().disabled(true).child(button("inside", bump))),
  );

  tree.get_element_by_id_mut("inside").unwrap().focus();
  tree.get_element_by_id_mut("inside").unwrap().click();
  tree.pass(&mut app, &TestSurface);

  assert_eq!(focused_id(&tree), None);
  assert_eq!(clicks(&count), 0);
}

#[test]
fn an_element_that_becomes_disabled_loses_its_focus() {
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .child(Row::new().id("group").child(button("inside", || {})))
      .child(button("other", || {})),
  );
  tab(&mut tree);
  assert_eq!(focused_id(&tree).as_deref(), Some("inside"));

  tree.get_element_by_id_mut("group").unwrap().set_disabled(true);
  tree.pass(&mut app, &TestSurface);
  assert_eq!(focused_id(&tree), None);

  tab(&mut tree);
  assert_eq!(focused_id(&tree).as_deref(), Some("other"));
}

#[test]
fn the_state_of_a_disabled_element_and_of_what_is_in_it_says_disabled() {
  let group = InteractionState::new();
  let inside = InteractionState::new();
  let outside = InteractionState::new();
  let mut app = App::new();
  // Nothing is painted here: the state is written when the tree is built, for every element, in view or not.
  let mut tree = mounted(
    &mut app,
    Column::new()
      .child(
        Row::new()
          .id("group")
          .interactive(group.clone())
          .disabled(true)
          .child(Row::new().child(Rect::new(40.0, 20.0).interactive(inside.clone()))),
      )
      .child(Rect::new(40.0, 20.0).interactive(outside.clone())),
  );
  assert_eq!(
    (group.is_disabled(), inside.is_disabled(), outside.is_disabled()),
    (true, true, false)
  );

  // Enabling a live element is known at once, before any pass.
  tree.get_element_by_id_mut("group").unwrap().set_disabled(false);
  assert!(!group.is_disabled() && !inside.is_disabled());

  tree.get_element_by_id_mut("group").unwrap().set_disabled(true);
  assert!(group.is_disabled() && inside.is_disabled());
  assert!(!outside.is_disabled());
}

#[test]
fn an_element_enabled_inside_a_disabled_one_stays_disabled() {
  let inner = InteractionState::new();
  let leaf = InteractionState::new();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new().id("outer").disabled(true).child(
      Row::new()
        .id("inner")
        .interactive(inner.clone())
        .disabled(true)
        .child(Rect::new(40.0, 20.0).interactive(leaf.clone())),
    ),
  );

  tree.get_element_by_id_mut("inner").unwrap().set_disabled(false);
  assert!(
    inner.is_disabled() && leaf.is_disabled(),
    "the outer element still disables them"
  );

  tree.get_element_by_id_mut("outer").unwrap().set_disabled(false);
  assert!(!inner.is_disabled() && !leaf.is_disabled());
}

struct DisabledInPopup {
  anchor: lurq::core::ElementRef,
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct PopupProps {
  #[devtools_ignore]
  inside: InteractionState,
}

// One popup, mounted once: its props never differ.
impl PartialEq for PopupProps {
  fn eq(&self, _other: &Self) -> bool {
    true
  }
}

impl lurq::app::component::Component for DisabledInPopup {
  type Props = PopupProps;

  fn create(_ctx: &mut lurq::app::ctx::Ctx) -> Self {
    Self {
      anchor: lurq::core::ElementRef::new(),
    }
  }

  fn render(&self, ctx: &mut lurq::app::ctx::Ctx) -> impl Into<Element> {
    let inside = ctx.props::<PopupProps>().inside.clone();
    Column::new()
      .child(Rect::new(40.0, 20.0).ref_element(self.anchor.clone()))
      .child(
        lurq::components::Popup::new(
          self.anchor.clone(),
          Row::new()
            .disabled(true)
            .child(Rect::new(40.0, 20.0).interactive(inside)),
        )
        .open_when(true),
      )
  }
}

#[test]
fn the_state_reaches_what_a_popup_holds() {
  let inside = InteractionState::new();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<DisabledInPopup>(&mut app, PopupProps { inside: inside.clone() });
  tree.pass(&mut app, &TestSurface);
  tree.pass(&mut app, &TestSurface);

  assert!(
    inside.is_disabled(),
    "a popup's content is built after the page, and gets its state too"
  );
}

fn painted_colors(tree: &Tree) -> (Vec<Color>, Vec<Color>) {
  let quads = tree.resolve_quads(tree.last_layout().unwrap());
  let fills = quads
    .iter()
    .filter_map(|quad| match &quad.content {
      QuadContent::Rect { color, .. } if color.a() > 0 => Some(*color),
      _ => None,
    })
    .collect();
  let texts = quads
    .iter()
    .filter_map(|quad| match &quad.content {
      QuadContent::Text { style, .. } => Some(style.color),
      _ => None,
    })
    .collect();
  (fills, texts)
}

#[test]
fn a_disabled_style_paints_the_element_and_the_elements_in_it() {
  let page = |disabled: bool| {
    Row::new()
      .id("group")
      .background(FILL)
      .disabled(disabled)
      .when_disabled(|style| style.background(GREY).foreground(GREY))
      .child(Text::new("Save"))
      .child(
        Rect::new(20.0, 20.0)
          .background(FILL)
          .when_disabled(|style| style.background(RED)),
      )
  };
  let mut app = App::new();

  let enabled = mounted(&mut app, page(false));
  assert_eq!(painted_colors(&enabled).0, vec![FILL, FILL]);

  let disabled = mounted(&mut app, page(true));
  let (fills, texts) = painted_colors(&disabled);
  assert_eq!(fills, vec![GREY, RED], "the group's own style, and its child's");
  assert_eq!(texts, vec![GREY], "the label takes the disabled foreground");
}

#[test]
fn a_disabled_style_wins_over_a_hovered_one_and_changes_no_layout() {
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new().child(
      Rect::new(120.0, 32.0)
        .id("item")
        .background(FILL)
        .disabled(true)
        .hovered(|style| style.background(RED))
        .when_disabled(|style| style.background(GREY).width(300.0)),
    ),
  );
  let (x, y) = center(&mut tree, "item");

  tree.mouse_move(x, y);
  tree.pass(&mut app, &TestSurface);

  assert_eq!(painted_colors(&tree).0, vec![GREY]);
  let bounds = tree.get_element_by_id_mut("item").unwrap().bounds().unwrap();
  assert_eq!(bounds.width, 120.0, "a width in a disabled style is ignored");
}

#[test]
fn a_wheel_over_a_disabled_element_scrolls_the_container_it_is_in() {
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    ScrollVertical::new(
      Column::new()
        .id("content")
        .disabled(true)
        .child(Spacer::new().height(500.0)),
    )
    .size(100.0, 100.0),
  );
  let before = center(&mut tree, "content").1;

  tree.scroll(50.0, 50.0, 0.0, -60.0, ScrollPhase::Scroll);
  tree.pass(&mut app, &TestSurface);

  assert!(center(&mut tree, "content").1 < before, "the content moved up");
}

#[test]
fn what_a_disabled_element_holds_still_scrolls() {
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new()
      .disabled(true)
      .child(ScrollVertical::new(Column::new().id("content").child(Spacer::new().height(500.0))).size(100.0, 100.0)),
  );
  let before = center(&mut tree, "content").1;

  tree.scroll(50.0, 50.0, 0.0, -60.0, ScrollPhase::Scroll);
  tree.pass(&mut app, &TestSurface);

  assert!(
    center(&mut tree, "content").1 < before,
    "a disabled panel can be read to its end"
  );
}

#[test]
fn a_wheel_does_not_reach_the_handlers_of_a_disabled_element() {
  let (wheeled, bump) = counter();
  let (outer, bump_outer) = counter();
  let mut app = App::new();
  let mut tree = mounted(
    &mut app,
    Column::new().on_scroll(move |_| bump_outer()).child(
      Rect::new(100.0, 100.0)
        .background(FILL)
        .disabled(true)
        .on_scroll(move |_| bump()),
    ),
  );

  tree.scroll(50.0, 50.0, 0.0, -60.0, ScrollPhase::Scroll);

  assert_eq!(
    clicks(&wheeled),
    0,
    "a control that reads the wheel is disabled with the rest"
  );
  assert_eq!(clicks(&outer), 1, "the element it is in still takes the wheel");
}

#[test]
fn the_scrollbar_of_a_container_in_a_disabled_element_can_be_dragged() {
  use lurq::layout::layout_kind::{ScrollAxis, ScrollState};

  let state = ScrollState::new();
  let mut tree = Tree::new();
  tree.set_root(
    Column::new().disabled(true).child(
      ScrollVertical::new(Spacer::new().height(600.0))
        .with_scroll_state(state.clone())
        .size(120.0, 120.0),
    ),
  );
  // The scrollbar's place is recorded when it is painted.
  crate::support::render_pass(&mut tree);
  let (x, y, width, height) = state
    .thumb_rect_for_axis(ScrollAxis::Vertical, &state.style())
    .expect("the content overflows, so the scrollbar is shown");
  let press = (x + width / 2.0, y + height / 2.0);

  tree.mouse_down(press.0, press.1, MouseButton::Left);
  tree.mouse_move(press.0, press.1 + 30.0);
  tree.mouse_up(press.0, press.1 + 30.0, MouseButton::Left);

  assert!(state.scroll_y() > 0.0, "the thumb follows the pointer");
}
