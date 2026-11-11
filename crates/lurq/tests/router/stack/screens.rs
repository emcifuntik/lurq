//! A `StackOutlet`: the history's entries as screens that stay mounted under the top one.

use std::{
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use lurq::{
  animation::Easing,
  app::{App, Tree, component::Component, ctx::Ctx, events::TouchPhase},
  components::{NavigationBar, NavigationItem, Router, StackOutlet, StackOutletProps, Text},
  node::Element,
  router::{RouterHandle, Routes},
};

use crate::support::run_pass;

#[derive(lurq::DevtoolsInspectable)]
struct SharedRouter(Arc<Mutex<Option<RouterHandle>>>);

impl Clone for SharedRouter {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

impl PartialEq for SharedRouter {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

thread_local! {
  /// How many times the screen at `/first` was created in this test: each test runs on a thread of its own.
  static FIRST_CREATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct First;

impl Component for First {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    FIRST_CREATED.with(|created| created.set(created.get() + 1));
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.navigation_item(NavigationItem::new("Alpha"));
    Text::new("first screen")
  }
}

struct Root {
  router: RouterHandle,
}

impl Component for Root {
  type Props = SharedRouter;

  fn create(ctx: &mut Ctx) -> Self {
    let router = ctx.router(Routes::new().layout(
      "/",
      |ctx| StackOutlet::mount_with(ctx, StackOutletProps::new().bar(NavigationBar::new())),
      |routes| {
        routes
          .route("/first", |ctx| ctx.mount::<First>(()))
          .route("/second", |ctx| {
            ctx.navigation_item(NavigationItem::new("Beta"));
            Text::new("second screen").into()
          })
          .route("/held", |ctx| {
            ctx.navigation_item(NavigationItem::new("Held").back_gesture(false));
            Text::new("held screen").into()
          })
          .route("/long", |ctx| {
            ctx.navigation_item(NavigationItem::new(
              "A title too long to stand beside another as the way back",
            ));
            Text::new("long screen").into()
          })
          .route("/bare", |ctx| {
            ctx.navigation_item(NavigationItem::new("Bare").bar_hidden(true));
            Text::new("bare screen").into()
          })
          .route("/pictured", |ctx| {
            ctx.navigation_item(NavigationItem::new("Pictured").title_view(Text::new("a view for a title")));
            Text::new("pictured screen").into()
          })
          .route("/led", |ctx| {
            ctx.navigation_item(NavigationItem::new("Led").leading(Text::new("a control of its own")));
            Text::new("led screen").into()
          })
          .route("/big", |ctx| {
            ctx.navigation_item(NavigationItem::new("Gamma").large_title(true));
            StackOutlet::large_title_scroll(ctx, lurq::components::Spacer::new().width(300.0).height(2000.0)).into()
          })
          .route("/user/:id", |ctx| {
            let id = ctx.route_params().get("id").unwrap_or("none").to_owned();
            Text::new(&format!("user {id}")).into()
          })
      },
    ));
    *ctx.props::<Self::Props>().0.lock().unwrap() = Some(router.clone());
    router.replace("/first");
    Self { router }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Router::mount(ctx, self.router.clone())
  }
}

fn setup() -> (Tree, RouterHandle) {
  let router_out = Arc::new(Mutex::new(None));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 800);
  tree.mount_root::<Root>(&mut app, SharedRouter(router_out.clone()));
  run_pass(&mut tree);
  let router = router_out.lock().unwrap().clone().unwrap();
  (tree, router)
}

/// Lets a move of the stack end: its timers fire and the screens are where they go.
fn settle(tree: &mut Tree) {
  run_pass(tree);
  tree.tick_timers_at(Instant::now() + Duration::from_secs(1));
  run_pass(tree);
  tree.tick_timers_at(Instant::now() + Duration::from_secs(2));
  run_pass(tree);
}

fn shows(tree: &mut Tree, text: &str) -> bool {
  tree
    .find_element(|element| element.text_content() == Some(text))
    .is_some()
}

fn has_way_back(tree: &mut Tree) -> bool {
  tree
    .find_element(|element| element.id() == Some("navigation-back"))
    .is_some()
}

#[test]
fn a_screen_stays_mounted_under_the_one_pushed_over_it() {
  let (mut tree, router) = setup();
  let created = FIRST_CREATED.with(std::cell::Cell::get);
  assert!(shows(&mut tree, "first screen"));

  router.push("/second");
  settle(&mut tree);
  assert!(shows(&mut tree, "second screen"));
  assert!(
    !shows(&mut tree, "first screen"),
    "the screen under the top one is not laid out"
  );

  assert!(router.back());
  settle(&mut tree);
  assert!(shows(&mut tree, "first screen"));
  assert!(!shows(&mut tree, "second screen"), "the screen that went back is gone");
  assert_eq!(
    FIRST_CREATED.with(std::cell::Cell::get),
    created,
    "the first screen was not made anew"
  );
}

#[test]
fn both_screens_are_shown_while_one_comes_in() {
  let (mut tree, router) = setup();

  router.push("/second");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "second screen"));
  assert!(shows(&mut tree, "first screen"));
}

#[test]
fn the_bar_shows_the_top_screens_title_and_the_way_back_to_the_one_under() {
  let (mut tree, router) = setup();
  assert!(shows(&mut tree, "Alpha"));
  assert!(!has_way_back(&mut tree), "nothing to go back to");

  router.push("/second");
  settle(&mut tree);
  assert!(shows(&mut tree, "Beta"));
  assert!(shows(&mut tree, "Alpha"), "the way back says where it goes");
  assert!(has_way_back(&mut tree));
}

#[test]
fn a_screen_reads_the_params_of_its_own_entry() {
  let (mut tree, router) = setup();

  router.push("/user/1");
  settle(&mut tree);
  router.push("/user/2");
  run_pass(&mut tree);
  assert!(shows(&mut tree, "user 2"));
  assert!(
    shows(&mut tree, "user 1"),
    "the screen under keeps its own params while the next comes in"
  );
}

/// A finger lands at the left edge and goes `to` logical pixels across before it lifts.
fn swipe_from_the_left_edge(tree: &mut Tree, to: f32) {
  tree.touch_event(1, TouchPhase::Start, 2.0, 300.0, None);
  for step in 1..=10 {
    tree.touch_event(1, TouchPhase::Move, 2.0 + to * step as f32 / 10.0, 300.0, None);
  }
  std::thread::sleep(Duration::from_millis(150));
  tree.touch_event(1, TouchPhase::Move, 2.0 + to, 300.0, None);
  tree.touch_event(1, TouchPhase::End, 2.0 + to, 300.0, None);
}

#[test]
fn a_finger_from_the_left_edge_takes_the_top_screen_back() {
  let (mut tree, router) = setup();
  router.push("/second");
  settle(&mut tree);

  swipe_from_the_left_edge(&mut tree, 300.0);
  settle(&mut tree);
  assert_eq!(router.path().get_untracked(), "/first");
  assert!(shows(&mut tree, "first screen"));
}

#[test]
fn a_finger_that_lifts_short_of_half_way_leaves_the_screen() {
  let (mut tree, router) = setup();
  router.push("/second");
  settle(&mut tree);

  swipe_from_the_left_edge(&mut tree, 80.0);
  settle(&mut tree);
  assert_eq!(router.path().get_untracked(), "/second");
  assert!(shows(&mut tree, "second screen"));
  assert!(!shows(&mut tree, "first screen"));
}

#[test]
fn a_screen_that_turned_the_gesture_off_is_not_taken_back_by_a_finger() {
  let (mut tree, router) = setup();
  router.push("/held");
  settle(&mut tree);

  swipe_from_the_left_edge(&mut tree, 300.0);
  settle(&mut tree);
  assert_eq!(router.path().get_untracked(), "/held");
}

#[test]
fn the_way_back_says_only_back_when_the_title_under_has_no_room() {
  let (mut tree, router) = setup();
  router.push("/long");
  settle(&mut tree);
  router.push("/second");
  settle(&mut tree);

  assert!(has_way_back(&mut tree));
  assert!(shows(&mut tree, "Back"), "the label that does not fit gives way");
}

#[test]
fn a_screen_can_take_the_bar_away() {
  let (mut tree, router) = setup();
  let bar_height = |tree: &mut Tree| {
    tree
      .find_element(|element| element.text_content() == Some("Alpha") || element.text_content() == Some("Bare"))
      .map(|title| title.rect().1)
  };
  assert!(bar_height(&mut tree).is_some());

  router.push("/bare");
  settle(&mut tree);
  let screen = tree
    .find_element(|element| element.text_content() == Some("bare screen"))
    .expect("the screen");
  assert!(
    screen.rect().1 < 1.0,
    "the screen starts at the top: {:?}",
    screen.rect()
  );
}

#[test]
fn a_screen_can_put_a_view_in_the_titles_place() {
  let (mut tree, router) = setup();
  router.push("/pictured");
  settle(&mut tree);

  assert!(shows(&mut tree, "a view for a title"));
  assert!(!shows(&mut tree, "Pictured"), "the view stands in for the title");
}

#[test]
fn a_screen_can_put_a_control_in_the_place_of_the_way_back() {
  let (mut tree, router) = setup();
  router.push("/led");
  settle(&mut tree);

  assert!(shows(&mut tree, "a control of its own"));
  assert!(!has_way_back(&mut tree));
}

#[test]
fn a_spring_goes_from_rest_to_its_end_without_passing_it() {
  for velocity in [0.0, 2.0, 10.0, 50.0] {
    let spring = Easing::Spring { velocity };
    assert_eq!(spring.evaluate(0.0), 0.0);
    assert_eq!(spring.evaluate(1.0), 1.0);
    let mut last = 0.0;
    for step in 1..=100 {
      let value = spring.evaluate(f64::from(step) / 100.0);
      assert!(
        value >= last && value <= 1.0 + 1e-9,
        "velocity {velocity}: {last} then {value}"
      );
      last = value;
    }
  }
  let (rest, moving) = (Easing::Spring { velocity: 0.0 }, Easing::Spring { velocity: 4.0 });
  assert!(
    moving.evaluate(0.1) > rest.evaluate(0.1),
    "a spring let go at speed is further on"
  );
}

/// How wide the large title is drawn.
fn large_title_width(tree: &mut Tree) -> f32 {
  tree
    .find_element(|element| element.id() == Some("navigation-large-title"))
    .map_or(0.0, |title| title.rect().2)
}

#[test]
fn a_large_title_grows_as_the_content_is_pulled_down_and_goes_back() {
  let (mut tree, router) = setup();
  router.push("/big");
  settle(&mut tree);
  let at_rest = large_title_width(&mut tree);
  assert!(at_rest > 0.0);

  // A finger on the content pulls it down past its top.
  tree.touch_scroll_begin(200.0, 400.0, 0.0, 10.0);
  for _ in 0..12 {
    tree.touch_scroll(200.0, 400.0, 0.0, 20.0, lurq::app::events::ScrollPhase::Scroll, false);
  }
  run_pass(&mut tree);
  run_pass(&mut tree);
  let pulled = large_title_width(&mut tree);
  assert!(
    pulled > at_rest * 1.02,
    "the title is larger while pulled: {at_rest} then {pulled}"
  );
}
