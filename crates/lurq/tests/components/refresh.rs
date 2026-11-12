//! `PullToRefresh`: a list pulled down past its top asks to be loaded anew, once for each pull.

use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx, events::ScrollPhase},
  components::{Column, PullToRefresh, PullToRefreshProps, ScrollVertical, Spacer},
  core::Signal,
  layout::layout_kind::ScrollState,
  node::{Element, dimension::Dimension},
};

use crate::support::run_pass;

#[derive(Clone)]
struct Setup {
  refreshing: Signal<bool>,
  asked: Arc<AtomicUsize>,
}

impl PartialEq for Setup {
  fn eq(&self, other: &Self) -> bool {
    self.refreshing.id() == other.refreshing.id()
  }
}

impl lurq::app::component::DevtoolsInspectable for Setup {
  fn write_info(&self, _buffer: &mut Vec<lurq::app::component::ComponentInfo>) {}
}

struct Page {
  scroll: ScrollState,
}

impl Component for Page {
  type Props = Setup;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      scroll: ScrollState::new(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let setup = ctx.props::<Setup>().clone();
    let fill = Dimension::Pct(100.0);
    let list = ScrollVertical::new(Column::new().child(Spacer::new().width(300.0).height(3000.0)))
      .with_scroll_state(self.scroll.clone())
      .size(fill, fill);
    let asked = setup.asked.clone();
    PullToRefresh::mount(
      ctx,
      PullToRefreshProps::new(setup.refreshing, self.scroll.clone()).on_refresh(move || {
        asked.fetch_add(1, Ordering::SeqCst);
      }),
      list,
    )
  }
}

fn stage() -> (Tree, Signal<bool>, Arc<AtomicUsize>) {
  let (refreshing, asked) = (Signal::new(false), Arc::new(AtomicUsize::new(0)));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 800);
  tree.mount_root::<Page>(
    &mut app,
    Setup {
      refreshing: refreshing.clone(),
      asked: asked.clone(),
    },
  );
  run_pass(&mut tree);
  (tree, refreshing, asked)
}

/// A finger that scrolls the list `by` down past its top, and stays there.
fn pull(tree: &mut Tree, by: f32) {
  let step = by / 20.0;
  tree.touch_scroll_begin(200.0, 100.0, 0.0, step);
  tree.touch_scroll(200.0, 100.0, 0.0, 0.0, ScrollPhase::Start, false);
  for _ in 0..20 {
    tree.touch_scroll(200.0, 100.0, 0.0, step, ScrollPhase::Scroll, false);
    run_pass(tree);
  }
  run_pass(tree);
}

#[test]
fn a_list_pulled_far_enough_asks_for_a_refresh_once() {
  let (mut tree, refreshing, asked) = stage();
  pull(&mut tree, 650.0);
  assert!(refreshing.get_untracked(), "pulled past where it asks");
  assert_eq!(asked.load(Ordering::SeqCst), 1);

  // Held there, it does not ask again; nor after the refresh is done, until it has been let go.
  run_pass(&mut tree);
  refreshing.set(false);
  run_pass(&mut tree);
  run_pass(&mut tree);
  assert_eq!(asked.load(Ordering::SeqCst), 1);
  assert!(!refreshing.get_untracked());
}

#[test]
fn a_list_pulled_a_little_asks_for_nothing() {
  let (mut tree, refreshing, asked) = stage();
  pull(&mut tree, 60.0);
  assert!(!refreshing.get_untracked());
  assert_eq!(asked.load(Ordering::SeqCst), 0);
}
