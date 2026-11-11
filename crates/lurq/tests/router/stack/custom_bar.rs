//! A bar of the app's own over a `StackOutlet`.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{NavigationBarBuilder, NavigationItem, Router, StackOutlet, StackOutletProps, Text},
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

struct Root {
  router: RouterHandle,
  bar: NavigationBarBuilder,
}

impl Component for Root {
  type Props = SharedRouter;

  fn create(ctx: &mut Ctx) -> Self {
    let bar: NavigationBarBuilder = Arc::new(|state| {
      let back = state.top.back_label.clone().unwrap_or_else(|| "nowhere".to_owned());
      Text::new(&format!(
        "bar: {} from {back}; under {:?}; large in view {}; searching {}",
        state.top.item.title, state.under_titles, state.top.large_title_in_view, state.top.searching
      ))
      .into()
    });
    let shell_bar = bar.clone();
    let router = ctx.router(Routes::new().layout(
      "/",
      move |ctx| StackOutlet::mount_with(ctx, StackOutletProps::new().bar_with(shell_bar.clone())),
      |routes| {
        routes
          .route("/first", |ctx| {
            ctx.navigation_item(NavigationItem::new("Alpha"));
            Text::new("first screen").into()
          })
          .route("/second", |ctx| {
            ctx.navigation_item(NavigationItem::new("Beta"));
            Text::new("second screen").into()
          })
          .route("/own", |ctx| {
            ctx.navigation_item(
              NavigationItem::new("Own")
                .large_title_view(Text::new("a title of the screen's own"))
                .search_view(Text::new("a field of the screen's own")),
            );
            // The screen's own field says when it is searched in.
            if let Some(searching) = StackOutlet::searching(ctx) {
              searching.set(true);
            }
            StackOutlet::large_title_scroll(ctx, Text::new("own screen")).into()
          })
      },
    ));
    *ctx.props::<Self::Props>().0.lock().unwrap() = Some(router.clone());
    router.replace("/first");
    Self { router, bar }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let _ = &self.bar;
    Router::mount(ctx, self.router.clone())
  }
}

#[test]
fn a_bar_of_the_apps_own_is_drawn_from_the_stacks_state() {
  let router_out = Arc::new(Mutex::new(None));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 800);
  tree.mount_root::<Root>(&mut app, SharedRouter(router_out.clone()));
  run_pass(&mut tree);
  let router = router_out.lock().unwrap().clone().unwrap();
  let shows = |tree: &mut Tree, text: &str| {
    tree
      .find_element(|element| element.text_content() == Some(text))
      .is_some()
  };
  assert!(shows(
    &mut tree,
    "bar: Alpha from nowhere; under []; large in view false; searching false"
  ));

  router.push("/second");
  run_pass(&mut tree);
  tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(1));
  run_pass(&mut tree);
  tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(2));
  run_pass(&mut tree);
  assert!(shows(
    &mut tree,
    "bar: Beta from Alpha; under [\"Alpha\"]; large in view false; searching false"
  ));
  assert!(shows(&mut tree, "second screen"));

  // A screen with a title and a search field of its own, which says it is searching.
  router.push("/own");
  for ahead in [3, 4, 5] {
    run_pass(&mut tree);
    tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(ahead));
  }
  run_pass(&mut tree);
  assert!(shows(&mut tree, "a title of the screen's own"));
  assert!(shows(&mut tree, "a field of the screen's own"));
  assert!(shows(
    &mut tree,
    "bar: Own from Beta; under [\"Beta\", \"Alpha\"]; large in view true; searching true"
  ));
}

struct Styled {
  router: RouterHandle,
}

impl Component for Styled {
  type Props = SharedRouter;

  fn create(ctx: &mut Ctx) -> Self {
    let router = ctx.router(Routes::new().layout(
      "/",
      |ctx| {
        let bar = lurq::components::NavigationBar {
          height: 60.0,
          ..lurq::components::NavigationBar::new().back("Назад").back_menu(false)
        };
        StackOutlet::mount_with(ctx, StackOutletProps::new().bar(bar))
      },
      |routes| {
        routes
          .route("/first", |_ctx| Text::new("first screen").into())
          .route("/second", |ctx| {
            ctx.navigation_item(NavigationItem::new("Beta"));
            Text::new("second screen").into()
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

#[test]
fn the_bars_sizes_and_its_word_for_going_back_are_the_apps() {
  let router_out = Arc::new(Mutex::new(None));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 800);
  tree.mount_root::<Styled>(&mut app, SharedRouter(router_out.clone()));
  run_pass(&mut tree);
  let router = router_out.lock().unwrap().clone().unwrap();
  let top_of = |tree: &mut Tree, text: &'static str| {
    tree
      .find_element(|element| element.text_content() == Some(text))
      .map(|element| element.rect().1)
  };
  // Under a bar 60 high, with its line.
  assert!(top_of(&mut tree, "first screen").is_some_and(|top| (60.0..62.0).contains(&top)));

  router.push("/second");
  run_pass(&mut tree);
  tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(1));
  run_pass(&mut tree);
  tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(2));
  run_pass(&mut tree);
  // The screen under gave no title: the way back says the word the app gave for it.
  assert!(top_of(&mut tree, "Назад").is_some());
  assert!(top_of(&mut tree, "Back").is_none());

  // With the list turned off, a long press on the way back opens nothing, and its lift is a tap.
  tree.touch_press(20.0, 30.0);
  tree.touch_long_press(20.0, 30.0);
  tree.touch_release(20.0, 30.0);
  run_pass(&mut tree);
  assert_eq!(router.path().get_untracked(), "/first");
}
