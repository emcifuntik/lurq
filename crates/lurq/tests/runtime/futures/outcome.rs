//! `FutureState::outcome` and `fulfilled_data` report the latest run, while `data` keeps the last success.

use std::sync::{Arc, Mutex};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, FutureAction, FutureState, FutureStatus},
  },
  components::Text,
  node::Element,
};

type Action = FutureAction<Result<String, String>, String, String>;

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

struct Settle;

impl Component for Settle {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let action = ctx.future_action(|result: Result<String, String>| async move { result });
    *ctx.props::<Probe>().0.lock().unwrap() = Some(action);
    Self
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Text::new("")
  }
}

#[test]
fn a_rejection_after_a_success_keeps_data_but_its_outcome_is_the_error() {
  let probe = Probe::default();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<Settle>(&mut app, probe.clone());
  let action = probe.0.lock().unwrap().clone().unwrap();
  let state = || action.state().get_untracked();

  assert_eq!(state().outcome(), None);

  action.run(Ok("first".to_owned()));
  assert_eq!(state().outcome(), None, "pending");
  tree.tick_futures();
  assert_eq!(state().outcome(), Some(Ok(&"first".to_owned())));
  assert_eq!(state().fulfilled_data().map(String::as_str), Some("first"));

  action.run(Err("boom".to_owned()));
  assert_eq!(state().data.as_deref(), Some("first"), "pending keeps the last success");
  assert_eq!(state().fulfilled_data(), None);
  tree.tick_futures();
  let rejected = state();
  assert_eq!(rejected.status, FutureStatus::Rejected);
  assert_eq!(rejected.data.as_deref(), Some("first"), "data is the last success");
  assert_eq!(rejected.outcome(), Some(Err(&"boom".to_owned())));
  assert_eq!(rejected.fulfilled_data(), None);
}

#[test]
fn outcome_follows_the_status() {
  let idle = FutureState::<i32, &str>::idle();
  assert_eq!(idle.outcome(), None);
  assert_eq!(FutureState::<i32, &str>::pending(Some(1)).outcome(), None);
  assert_eq!(FutureState::<i32, &str>::fulfilled(2).outcome(), Some(Ok(&2)));
  assert_eq!(
    FutureState::<i32, &str>::rejected("no", Some(2)).outcome(),
    Some(Err(&"no"))
  );
  assert_eq!(FutureState::<i32, &str>::rejected("no", Some(2)).fulfilled_data(), None);
}
