//! Real component-hook boundary regression; no synthetic completed timers.
use std::{
  sync::{Arc, Mutex, mpsc},
  time::{Duration, Instant},
};

use super::*;
use crate::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::Column,
  node::Element,
};

#[derive(Clone)]
struct HookProps(Arc<HookChannels>);
struct HookChannels {
  entered: mpsc::Sender<Instant>,
  resume: Mutex<mpsc::Receiver<()>>,
}
impl PartialEq for HookProps {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}
impl crate::app::component::DevtoolsInspectable for HookProps {}

struct BlockingHook(HookProps);
impl Component for BlockingHook {
  type Props = HookProps;
  fn create(ctx: &mut Ctx) -> Self {
    Self(ctx.props::<HookProps>().clone())
  }
  fn render(&self, _: &mut Ctx) -> impl Into<Element> {
    Column::new()
  }
  fn after_layout(&self) {
    self.0.0.entered.send(Instant::now()).unwrap();
    self.0.0.resume.lock().unwrap().recv().unwrap();
  }
}

struct HookRoot(HookProps);
impl Component for HookRoot {
  type Props = HookProps;
  fn create(ctx: &mut Ctx) -> Self {
    Self(ctx.props::<HookProps>().clone())
  }
  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new().child(ctx.mount::<BlockingHook>(self.0.clone()))
  }
}

#[test]
fn real_recursive_after_layout_hook_is_unfinished_until_pass_completion() {
  let (entered_tx, entered_rx) = mpsc::channel();
  let (resume_tx, resume_rx) = mpsc::channel();
  let props = HookProps(Arc::new(HookChannels {
    entered: entered_tx,
    resume: Mutex::new(resume_rx),
  }));
  let (handle_tx, handle_rx) = mpsc::channel();
  let (begin_tx, begin_rx) = mpsc::channel();
  let worker = std::thread::spawn(move || {
    let mut tree = Tree::new();
    let mut app = App::new();
    tree.mount_root::<HookRoot>(&mut app, props);
    handle_tx.send(tree.profiling_handle()).unwrap();
    begin_rx.recv().unwrap();
    tree.pass_headless(&mut app);
  });
  let handle = handle_rx.recv_timeout(Duration::from_secs(5)).unwrap();
  let first = handle.start(Default::default()).unwrap().id;
  begin_tx.send(()).unwrap();
  let hook_started = entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
  let second = handle.start(Default::default()).unwrap().id;
  let ended_second = handle.end(second).unwrap();
  let resumed = Instant::now();
  resume_tx.send(()).unwrap();
  worker.join().unwrap();
  let ended_first = handle.end(first).unwrap();
  assert!(ended_second.samples.is_empty());
  assert_eq!(ended_second.in_flight[0].phase, Phase::ComponentAfterLayout);
  assert!(ended_second.in_flight[0].started_before_session);
  let (completed, pass) = ended_first
    .samples
    .iter()
    .find_map(|sample| match &sample.data {
      SampleData::Pass(pass) => Some((sample, pass)),
      _ => None,
    })
    .unwrap();
  assert!(completed.completed_ms >= handle.millis(resumed));
  assert!(pass.component_after_layout >= resumed.duration_since(hook_started));
  assert!(pass.layout_compute > Duration::ZERO);
  assert!(pass.component_after_layout <= pass.layout_update);
  assert!(pass.layout_compute + pass.component_after_layout <= pass.layout_update);
  assert!(!pass.rendered && pass.frame.is_none());
  assert!(ended_first.in_flight.is_empty());
  // Late completion never changes the finalized earlier result.
  assert!(ended_second.samples.is_empty());
  assert_eq!(ended_second.in_flight[0].phase, Phase::ComponentAfterLayout);
  #[cfg(any(feature = "serde", feature = "mcp"))]
  {
    let export = ended_first.to_json();
    let sample = export["samples"]
      .as_array()
      .unwrap()
      .iter()
      .find(|sample| sample["data"]["kind"] == "pass")
      .unwrap();
    assert!(sample["data"]["cpu_timings_ms"]["layout_compute"].as_f64().unwrap() > 0.);
    assert!(
      sample["data"]["cpu_timings_ms"]["component_after_layout"]
        .as_f64()
        .unwrap()
        > 0.
    );
    assert_eq!(
      ended_second.to_json()["in_flight"][0]["phase"],
      "component_after_layout"
    );
  }
}
