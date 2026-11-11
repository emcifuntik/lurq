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
      Text::new(&format!("bar: {} from {back}", state.top.item.title)).into()
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
  assert!(shows(&mut tree, "bar: Alpha from nowhere"));

  router.push("/second");
  run_pass(&mut tree);
  tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(1));
  run_pass(&mut tree);
  tree.tick_timers_at(std::time::Instant::now() + std::time::Duration::from_secs(2));
  run_pass(&mut tree);
  assert!(shows(&mut tree, "bar: Beta from Alpha"));
  assert!(shows(&mut tree, "second screen"));
}
