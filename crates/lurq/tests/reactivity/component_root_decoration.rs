//! What a parent puts on the element a component's `mount` returns (`.id`,
//! `.class`) stays on that component's root when the component renders again
//! by itself, in a dirty refresh that does not render the parent.

use std::sync::{
  Arc, Mutex,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{Column, Text},
  core::{Signal, SignalValue},
  node::{Element, ElementRef},
};

use crate::support::run_pass;

/// Where the components hand their signals and render counts to the test.
#[derive(Clone, Default)]
struct Probe {
  /// Whether the outer component decorates the inner one's root.
  decorate: Arc<Mutex<Option<Signal<bool>>>>,
  /// Renders the component above the outer one again.
  top: Arc<Mutex<Option<Signal<i32>>>>,
  inner: Arc<Mutex<Option<Signal<i32>>>>,
  outer_renders: Arc<AtomicUsize>,
  inner_renders: Arc<AtomicUsize>,
}

impl PartialEq for Probe {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for Probe {}

impl Probe {
  fn signal<T: SignalValue>(slot: &Arc<Mutex<Option<Signal<T>>>>) -> Signal<T> {
    slot.lock().unwrap().clone().expect("the component made its signal")
  }

  fn inner(&self) -> Signal<i32> {
    Self::signal(&self.inner)
  }

  fn top(&self) -> Signal<i32> {
    Self::signal(&self.top)
  }

  fn decorate(&self) -> Signal<bool> {
    Self::signal(&self.decorate)
  }

  fn outer_renders(&self) -> usize {
    self.outer_renders.load(Ordering::Relaxed)
  }

  fn inner_renders(&self) -> usize {
    self.inner_renders.load(Ordering::Relaxed)
  }
}

/// Mounts `Inner` and decorates its root while its `decorate` signal is set.
struct Outer {
  decorate: Signal<bool>,
}

impl Component for Outer {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let decorate = ctx.signal(true);
    *ctx.props::<Self::Props>().decorate.lock().unwrap() = Some(decorate.clone());
    Self { decorate }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let probe = ctx.props::<Self::Props>().clone();
    probe.outer_renders.fetch_add(1, Ordering::Relaxed);
    let inner = ctx.mount::<Inner>(probe);
    let inner = if self.decorate.get() {
      inner.id("outer-id").class("outer-class")
    } else {
      inner
    };
    Column::new().child(inner)
  }
}

/// Shows its own count, sets an id and class of its own on its root, and
/// reads the breakpoint so that a resize across one renders it again.
struct Inner {
  count: Signal<i32>,
}

impl Component for Inner {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let count = ctx.signal(1);
    *ctx.props::<Self::Props>().inner.lock().unwrap() = Some(count.clone());
    Self { count }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    ctx.props::<Self::Props>().inner_renders.fetch_add(1, Ordering::Relaxed);
    let breakpoint = ctx.breakpoint().map(|breakpoint| breakpoint.as_str()).unwrap_or("base");
    Column::new()
      .id("inner-id")
      .class("inner-class")
      .child(Text::new(&format!("count-{} at {breakpoint}", self.count.get())))
  }
}

fn texts(element: ElementRef<'_>, out: &mut Vec<String>) {
  if let Some(text) = element.text_content() {
    out.push(text.to_owned());
  }
  for child in element.children() {
    texts(child, out);
  }
}

/// The text under the element with this id, or `None` when no element has it.
fn text_under_id(tree: &mut Tree, id: &str) -> Option<String> {
  let element = tree.get_element_by_id(id)?;
  let mut out = Vec::new();
  texts(element, &mut out);
  Some(out.join(" "))
}

fn classes_of(tree: &mut Tree, id: &str) -> Vec<String> {
  tree
    .get_element_by_id(id)
    .map(|element| element.classes().map(str::to_owned).collect())
    .unwrap_or_default()
}

fn mount(probe: &Probe) -> Tree {
  let mut tree = Tree::new();
  tree.mount_root::<Outer>(&mut App::new(), probe.clone());
  run_pass(&mut tree);
  tree
}

#[test]
fn an_outer_id_survives_the_inner_components_own_render() {
  let probe = Probe::default();
  let mut tree = mount(&probe);
  assert_eq!(text_under_id(&mut tree, "outer-id").as_deref(), Some("count-1 at md"));
  assert!(tree.get_element_by_id("inner-id").is_none(), "the outer id wins");

  probe.inner().set(2);
  run_pass(&mut tree);

  assert_eq!(probe.outer_renders(), 1, "only the inner component rendered again");
  assert_eq!(probe.inner_renders(), 2);
  assert_eq!(text_under_id(&mut tree, "outer-id").as_deref(), Some("count-2 at md"));
  assert!(tree.get_element_by_id("inner-id").is_none());
  assert_eq!(classes_of(&mut tree, "outer-id"), ["inner-class", "outer-class"]);
}

#[test]
fn an_outer_id_survives_a_breakpoint_change_that_renders_only_the_inner_component() {
  let probe = Probe::default();
  let mut tree = mount(&probe);

  tree.resize(1200, 600);
  run_pass(&mut tree);

  assert_eq!(
    probe.outer_renders(),
    1,
    "only the inner component reads the breakpoint"
  );
  assert_eq!(probe.inner_renders(), 2);
  assert_eq!(text_under_id(&mut tree, "outer-id").as_deref(), Some("count-1 at lg"));
  assert_eq!(classes_of(&mut tree, "outer-id"), ["inner-class", "outer-class"]);
}

#[test]
fn an_outer_id_survives_repeated_refreshes_and_a_lookup_between_passes() {
  let probe = Probe::default();
  let mut tree = mount(&probe);

  for count in 2..5 {
    probe.inner().set(count);
    assert_eq!(
      text_under_id(&mut tree, "outer-id"),
      Some(format!("count-{count} at md")),
      "the lookup refreshes the inner component and finds it by the outer id"
    );
    run_pass(&mut tree);
  }
  assert_eq!(probe.outer_renders(), 1);
  assert_eq!(classes_of(&mut tree, "outer-id"), ["inner-class", "outer-class"]);
}

#[test]
fn the_outer_component_dropping_its_decoration_drops_it_from_the_inner_root() {
  let probe = Probe::default();
  let mut tree = mount(&probe);

  probe.inner().set(2);
  run_pass(&mut tree);
  probe.decorate().set(false);
  run_pass(&mut tree);
  assert!(tree.get_element_by_id("outer-id").is_none());
  assert_eq!(text_under_id(&mut tree, "inner-id").as_deref(), Some("count-2 at md"));
  assert_eq!(classes_of(&mut tree, "inner-id"), ["inner-class"]);

  probe.inner().set(3);
  run_pass(&mut tree);
  assert!(
    tree.get_element_by_id("outer-id").is_none(),
    "a dropped decoration does not come back"
  );
  assert_eq!(text_under_id(&mut tree, "inner-id").as_deref(), Some("count-3 at md"));
  assert_eq!(classes_of(&mut tree, "inner-id"), ["inner-class"]);
}

/// Mounts an `Outer`, which decorates the `Inner` it mounts, and renders
/// again on its own signal while `Outer` keeps its cached tree.
struct Top {
  tick: Signal<i32>,
}

impl Component for Top {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let tick = ctx.signal(1);
    *ctx.props::<Self::Props>().top.lock().unwrap() = Some(tick.clone());
    Self { tick }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let probe = ctx.props::<Self::Props>().clone();
    let tick = self.tick.get();
    Column::new()
      .child(Text::new(&format!("top-{tick}")))
      .child(ctx.mount::<Outer>(probe))
  }
}

#[test]
fn an_outer_id_survives_in_the_cached_tree_of_a_clean_outer_component() {
  let probe = Probe::default();
  let mut tree = Tree::new();
  tree.mount_root::<Top>(&mut App::new(), probe.clone());
  run_pass(&mut tree);

  probe.inner().set(2);
  run_pass(&mut tree);
  assert_eq!(text_under_id(&mut tree, "outer-id").as_deref(), Some("count-2 at md"));

  // `Top` renders again; `Outer` is clean and hands back its kept output,
  // put together with the inner component's own render since.
  probe.top().set(2);
  run_pass(&mut tree);
  assert_eq!(probe.outer_renders(), 1, "the outer component kept its tree");
  assert!(
    tree
      .find_element(|element| element.text_content() == Some("top-2"))
      .is_some()
  );
  assert_eq!(text_under_id(&mut tree, "outer-id").as_deref(), Some("count-2 at md"));
  assert!(tree.get_element_by_id("inner-id").is_none());
  assert_eq!(classes_of(&mut tree, "outer-id"), ["inner-class", "outer-class"]);
}
