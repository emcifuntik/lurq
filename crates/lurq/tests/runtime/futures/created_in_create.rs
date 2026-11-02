//! `ctx.future`, `ctx.stream` and `ctx.future_action` called in `create` get slots of their own, which no render
//! replaces or drops; an action a render dropped does nothing when run.

use std::sync::{
  Arc, Mutex,
  atomic::{AtomicUsize, Ordering},
};

use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::{Ctx, FutureAction, FutureHandle, FutureStatus, StreamHandle},
  },
  components::Text,
  node::Element,
};

type Action = FutureAction<String, String, String>;

/// Props that hand the component's action to the test. `round` changes force a render; `call` says whether the
/// render makes its own `ctx.future_action` call.
#[derive(Clone, Default)]
struct Probe {
  action: Arc<Mutex<Option<Action>>>,
  starts: Arc<AtomicUsize>,
  round: i32,
  call: bool,
}

#[cfg(feature = "devtools")]
impl lurq::app::component::DevtoolsInspectable for Probe {
  fn write_info(&self, _buffer: &mut Vec<lurq::app::component::ComponentInfo>) {}
}

impl PartialEq for Probe {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.action, &other.action) && self.round == other.round && self.call == other.call
  }
}

impl Probe {
  fn round(&self, round: i32) -> Self {
    Self { round, ..self.clone() }
  }

  fn action(&self) -> Action {
    self
      .action
      .lock()
      .unwrap()
      .clone()
      .expect("the component stored its action")
  }
}

fn label(status: FutureStatus, data: Option<String>) -> String {
  match status {
    FutureStatus::Idle => "idle".to_owned(),
    FutureStatus::Pending => "pending".to_owned(),
    FutureStatus::Fulfilled => data.unwrap_or_default(),
    FutureStatus::Rejected => "rejected".to_owned(),
  }
}

fn echo(ctx: &mut Ctx) -> Action {
  ctx.future_action(|value: String| async move { Ok::<_, String>(value) })
}

fn text(tree: &Tree) -> String {
  tree.root().unwrap().text_content().unwrap_or_default().to_owned()
}

/// Creates its action in `create`; each render first makes a `ctx.future` call, which used to take the action's slot.
struct ActionFromCreate {
  action: Action,
}

impl Component for ActionFromCreate {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let action = echo(ctx);
    *ctx.props::<Probe>().action.lock().unwrap() = Some(action.clone());
    Self { action }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let round = ctx.props::<Probe>().round;
    let loaded = ctx
      .future(round, |round| async move { Ok::<_, String>(format!("page{round}")) })
      .state()
      .get();
    let action = self.action.state().get();
    Text::new(&format!(
      "{} {}",
      label(loaded.status, loaded.data),
      label(action.status, action.data)
    ))
  }
}

#[test]
fn an_action_created_in_create_survives_the_futures_of_render() {
  let probe = Probe::default().round(1);
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<ActionFromCreate>(&mut app, probe.clone());
  tree.tick_futures();
  assert_eq!(text(&tree), "page1 idle");

  tree.update_root_props::<ActionFromCreate>(probe.round(2));
  tree.rebuild();
  tree.tick_futures();
  assert_eq!(text(&tree), "page2 idle");

  let action = probe.action();
  action.run("saved".to_owned());
  assert!(action.is_active());
  tree.tick_futures();
  assert_eq!(text(&tree), "page2 saved");
}

/// Creates its action in `create` and makes no future calls in render, which used to drop the action at the end
/// of every render.
struct OnlyAction {
  action: Action,
}

impl Component for OnlyAction {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let action = echo(ctx);
    *ctx.props::<Probe>().action.lock().unwrap() = Some(action.clone());
    Self { action }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let state = self.action.state().get();
    Text::new(&label(state.status, state.data))
  }
}

#[test]
fn an_action_created_in_create_runs_when_render_makes_no_future_calls() {
  let probe = Probe::default();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<OnlyAction>(&mut app, probe.clone());
  tree.update_root_props::<OnlyAction>(probe.round(1));
  tree.rebuild();

  probe.action().run("ran".to_owned());
  tree.tick_futures();
  assert_eq!(text(&tree), "ran");
}

/// Starts a future and a stream in `create`; renders run again with new props.
struct LoadFromCreate {
  load: FutureHandle<String, String>,
  feed: StreamHandle<String, String>,
}

impl Component for LoadFromCreate {
  type Props = Probe;

  fn create(ctx: &mut Ctx) -> Self {
    let starts = ctx.props::<Probe>().starts.clone();
    let load = ctx.future((), move |()| {
      starts.fetch_add(1, Ordering::SeqCst);
      async { Ok::<_, String>("loaded".to_owned()) }
    });
    let feed = ctx.stream((), |(), emitter| async move {
      emitter.emit("first".to_owned());
      emitter.emit("second".to_owned());
    });
    Self { load, feed }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let round = ctx.props::<Probe>().round;
    let load = self.load.state().get();
    let feed = self.feed.state().get();
    Text::new(&format!(
      "{round} {} {}",
      label(load.status, load.data),
      label(feed.status, feed.data)
    ))
  }
}

#[test]
fn a_future_and_a_stream_started_in_create_complete_and_start_once() {
  let probe = Probe::default().round(1);
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<LoadFromCreate>(&mut app, probe.clone());
  tree.tick_futures();
  assert_eq!(text(&tree), "1 loaded second");

  tree.update_root_props::<LoadFromCreate>(probe.round(2));
  tree.rebuild();
  tree.tick_futures();
  assert_eq!(text(&tree), "2 loaded second");
  assert_eq!(probe.starts.load(Ordering::SeqCst), 1);
}

/// Makes its `ctx.future_action` call in render only while its props say so.
struct ConditionalAction;

impl Component for ConditionalAction {
  type Props = Probe;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let probe = ctx.props::<Probe>().clone();
    if probe.call {
      *probe.action.lock().unwrap() = Some(echo(ctx));
    }
    Text::new("")
  }
}

#[test]
fn an_action_a_render_dropped_does_not_run() {
  let probe = Probe {
    call: true,
    ..Probe::default()
  };
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.mount_root::<ConditionalAction>(&mut app, probe.clone());
  let action = probe.action();

  tree.update_root_props::<ConditionalAction>(Probe {
    call: false,
    ..probe.clone()
  });
  tree.rebuild();

  action.run("late".to_owned());
  assert!(!action.is_active(), "a dropped action starts nothing");
  tree.tick_futures();
  assert!(action.state().get_untracked().is_idle());
}
