//! A `TabOutlet`: several routers side by side, one shown, the others kept as they were.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::MouseButton, window::SafeAreaInsets},
  components::{
    Column, NavigationItem, StackOutlet, TabBar, TabBarPlace, TabItem, TabOutlet, TabOutletProps, Tabs, Text,
  },
  node::Element,
  router::Routes,
};

use crate::support::{pointer_click, run_pass};

struct Shared(Arc<Mutex<Option<Tabs>>>, Bar);

impl lurq::app::component::DevtoolsInspectable for Shared {
  fn write_info(&self, _buffer: &mut Vec<lurq::app::component::ComponentInfo>) {}
}

#[derive(Clone, Copy, PartialEq)]
enum Bar {
  None,
  Bottom,
  Top,
  Own,
}

impl PartialEq for Shared {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

thread_local! {
  /// How many times the first screen of the first tab was made in this test.
  static MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Home;

impl Component for Home {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    MADE.with(|made| made.set(made.get() + 1));
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let bottom = ctx.safe_area().bottom;
    Column::new()
      .child(Text::new("home screen"))
      .child(Text::new(&format!("clear of {bottom}")))
  }
}

struct Root {
  tabs: Tabs,
  bar: Bar,
}

impl Component for Root {
  type Props = Shared;

  fn create(ctx: &mut Ctx) -> Self {
    let home = ctx.router(
      Routes::new()
        .route("/", |ctx| ctx.mount::<Home>(()))
        .route("/deeper", |_| Text::new("deeper screen").into()),
    );
    home.replace("/");
    let other = ctx.router(Routes::new().route("/", |_| Text::new("other screen").into()));
    other.replace("/");
    // A tab that is a stack of screens, one of which has the bar away.
    let stacked = ctx.router(Routes::new().layout("/", StackOutlet::mount, |routes| {
      routes
        .route("/", |_| Text::new("stack's first").into())
        .route("/whole", |ctx| {
          ctx.navigation_item(NavigationItem::new("Whole").hides_tab_bar(true));
          let bottom = ctx.safe_area().bottom;
          Text::new(&format!("whole, clear of {bottom}")).into()
        })
    }));
    stacked.replace("/");
    let tabs = Tabs::new();
    tabs
      .add("home", TabItem::new("Home"), home)
      .add("other", TabItem::new("Other").badge("3"), other);
    if ctx.props::<Shared>().1 == Bar::Bottom {
      tabs.add("stacked", TabItem::new("Stacked"), stacked);
    }
    let props = ctx.props::<Shared>();
    *props.0.lock().unwrap() = Some(tabs.clone());
    Self { tabs, bar: props.1 }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = TabOutletProps::new(self.tabs.clone());
    TabOutlet::mount(
      ctx,
      match self.bar {
        Bar::None => props,
        Bar::Bottom => props.bar(TabBar::new()),
        Bar::Top => props.bar(TabBar::new().place(TabBarPlace::Top)),
        Bar::Own => props.bar_with(TabBarPlace::Bottom, |state| {
          Text::new(&format!("{} tabs, {} shown", state.entries.len(), state.tabs.active())).into()
        }),
      },
    )
  }
}

fn setup(bar: Bar) -> (Tree, Tabs) {
  let out = Arc::new(Mutex::new(None));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 800);
  tree.window().set_safe_area(SafeAreaInsets {
    bottom: 34.0,
    ..SafeAreaInsets::default()
  });
  tree.mount_root::<Root>(&mut app, Shared(out.clone(), bar));
  run_pass(&mut tree);
  let tabs = out.lock().unwrap().clone().unwrap();
  (tree, tabs)
}

fn shows(tree: &mut Tree, text: &str) -> bool {
  tree
    .find_element(|element| element.text_content() == Some(text))
    .is_some()
}

fn top_of(tree: &mut Tree, text: &str) -> f32 {
  tree
    .find_element(|element| element.text_content() == Some(text))
    .unwrap_or_else(|| panic!("{text} is shown"))
    .rect()
    .1
}

#[test]
fn the_first_tab_is_shown_and_another_when_it_is_selected() {
  let (mut tree, tabs) = setup(Bar::None);
  assert!(shows(&mut tree, "home screen"));
  assert!(!shows(&mut tree, "other screen"));

  tabs.select("other");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "other screen"));
  assert!(!shows(&mut tree, "home screen"));
  assert_eq!(tabs.active(), "other");
}

#[test]
fn a_tab_that_is_not_shown_keeps_its_screens_and_its_history() {
  let (mut tree, tabs) = setup(Bar::None);
  let made = MADE.with(std::cell::Cell::get);

  tabs.select("other");
  run_pass(&mut tree);
  tabs.select("home");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "home screen"));
  assert_eq!(MADE.with(std::cell::Cell::get), made, "the screen was not made anew");

  tabs.router("home").unwrap().push("/deeper");
  run_pass(&mut tree);
  tabs.select("other");
  run_pass(&mut tree);
  tabs.select("home");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "deeper screen"), "where it was left");
  assert!(tabs.router("home").unwrap().back(), "with the way back it had");
}

#[test]
fn a_press_on_a_tabs_place_shows_it() {
  let (mut tree, tabs) = setup(Bar::Bottom);
  assert!(shows(&mut tree, "Home") && shows(&mut tree, "Other"));
  assert!(shows(&mut tree, "3"), "the badge");
  assert!(top_of(&mut tree, "Other") > 700.0, "the bar is at the bottom");

  // The second of three places, in the bar over the home indicator's room.
  pointer_click(&mut tree, 200.0, 740.0, MouseButton::Left);
  run_pass(&mut tree);
  assert_eq!(tabs.active(), "other");
  assert!(shows(&mut tree, "other screen"));
}

#[test]
fn a_press_on_the_active_tabs_place_goes_back_to_its_first_screen() {
  let (mut tree, tabs) = setup(Bar::Bottom);
  tabs.router("home").unwrap().push("/deeper");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "deeper screen"));

  pointer_click(&mut tree, 60.0, 740.0, MouseButton::Left);
  run_pass(&mut tree);
  assert!(shows(&mut tree, "home screen"));
  assert_eq!(tabs.active(), "home");
}

#[test]
fn under_a_bar_at_the_bottom_a_screen_keeps_clear_of_the_bar_too() {
  let (mut tree, _tabs) = setup(Bar::Bottom);
  run_pass(&mut tree);
  // The home indicator's room, the bar and the line over it.
  assert!(shows(&mut tree, "clear of 83.5"));

  let (mut tree, _tabs) = setup(Bar::None);
  assert!(shows(&mut tree, "clear of 34"));

  let (mut tree, _tabs) = setup(Bar::Top);
  assert!(shows(&mut tree, "clear of 34"));
  assert!(top_of(&mut tree, "Other") < 100.0, "the bar is at the top");
  assert!(top_of(&mut tree, "home screen") > 40.0, "the screen is under it");
}

#[test]
fn a_hidden_bar_gives_its_room_back() {
  let (mut tree, tabs) = setup(Bar::Bottom);
  run_pass(&mut tree);
  assert!(shows(&mut tree, "clear of 83.5"));
  tabs.set_bar_hidden(true);
  run_pass(&mut tree);
  assert!(shows(&mut tree, "clear of 34"));
}

#[test]
fn the_bar_goes_while_the_keyboard_is_up() {
  let (mut tree, _tabs) = setup(Bar::Bottom);
  tree.window().set_safe_area(SafeAreaInsets {
    bottom: 300.0,
    keyboard: 266.0,
    ..SafeAreaInsets::default()
  });
  run_pass(&mut tree);
  assert!(shows(&mut tree, "clear of 300"));
}

#[test]
fn a_tab_taken_out_gives_its_place_to_the_next() {
  let (mut tree, tabs) = setup(Bar::Bottom);
  tabs.remove("home");
  run_pass(&mut tree);
  assert_eq!(tabs.active(), "other");
  assert!(shows(&mut tree, "other screen"));
  assert!(!shows(&mut tree, "Home"));
}

#[test]
fn an_app_draws_a_bar_of_its_own() {
  let (mut tree, tabs) = setup(Bar::Own);
  assert!(shows(&mut tree, "2 tabs, home shown"));
  tabs.select("other");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "2 tabs, other shown"));
}

#[test]
fn a_screen_that_hides_the_bar_has_the_bars_room() {
  let (mut tree, tabs) = setup(Bar::Bottom);
  tabs.select("stacked");
  run_pass(&mut tree);
  tabs.router("stacked").unwrap().push("/whole");
  for _ in 0..3 {
    run_pass(&mut tree);
    tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(2));
  }
  run_pass(&mut tree);
  assert!(shows(&mut tree, "whole, clear of 34"), "no bar to keep clear of");

  // The bar is out of view, under the window: a press where it was is the screen's.
  pointer_click(&mut tree, 60.0, 740.0, MouseButton::Left);
  run_pass(&mut tree);
  assert_eq!(tabs.active(), "stacked");
}
