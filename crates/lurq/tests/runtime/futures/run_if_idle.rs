//! `FutureAction::run` replaces a run in flight; `run_if_idle` leaves it and reports that it started nothing.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, FutureAction},
  },
  components::Text,
  node::Element,
};

type Action = FutureAction<String, String, String>;

#[derive(Clone, Default)]
struct Probe(Arc<Mutex<Option<Action>>>);

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for Probe {
  fn write_info(&self, _buffer: &mut Vec<lurq::app::component::ComponentInfo>) {}
}

impl PartialEq for Probe {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

struct Save;

impl Component for Save {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let action = ctx.future_action(|value: String| async move { Ok::<_, String>(value) });
    *ctx.props::<Probe>().0.lock().unwrap() = Some(action);
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Text::new("")
  }
}

fn mount() -> (Tree, Action) {
  let probe = Probe::default();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<Save>(&mut app, probe.clone());
  let action = probe.0.lock().unwrap().clone().unwrap();
  (tree, action)
}

#[test]
fn run_if_idle_leaves_a_run_in_flight() {
  let (mut tree, action) = mount();

  assert!(action.run_if_idle("first".to_owned()));
  assert!(!action.run_if_idle("second".to_owned()), "a run is in flight");
  tree.tick_futures();
  assert_eq!(
    action.state().get_untracked().fulfilled_data().map(String::as_str),
    Some("first")
  );

  assert!(
    action.run_if_idle("third".to_owned()),
    "idle again once the run completed"
  );
  tree.tick_futures();
  assert_eq!(
    action.state().get_untracked().fulfilled_data().map(String::as_str),
    Some("third")
  );
}

#[test]
fn run_replaces_a_run_in_flight() {
  let (mut tree, action) = mount();

  action.run("first".to_owned());
  action.run("second".to_owned());
  tree.tick_futures();
  assert_eq!(
    action.state().get_untracked().fulfilled_data().map(String::as_str),
    Some("second")
  );
}

#[test]
fn run_if_idle_starts_nothing_once_the_component_unmounted() {
  let (tree, action) = mount();
  drop(tree);
  assert!(!action.run_if_idle("late".to_owned()));
  assert!(!action.is_active());
}
