//! Detailed recordings on a real tree: component spans under rebuild and
//! after-layout, render causes from the notifications as they were sent
//! (memo recomputes included), per-component layout counts, and nothing of
//! it without `RecordingOptions::detailed`.
use std::cell::RefCell;

use super::*;
use crate::{
  app::{App, Tree, component::Component, ctx::Ctx},
  components::{Column, Text},
  core::{Memo, Signal},
  node::Element,
};

thread_local! {
  static COUNT: RefCell<Option<Signal<u32>>> = const { RefCell::new(None) };
  static BASE: RefCell<Option<Signal<u64>>> = const { RefCell::new(None) };
}

struct DetailRoot;

impl Component for DetailRoot {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    Column::new()
      .child(ctx.mount_keyed::<DetailCounter>("secret-row-key", ()))
      .child(ctx.mount::<DetailDoubled>(()))
  }
}

/// Renders its own signal's value.
struct DetailCounter {
  count: Signal<u32>,
}

impl Component for DetailCounter {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let count = ctx.signal(0_u32);
    COUNT.with(|slot| *slot.borrow_mut() = Some(count.clone()));
    Self { count }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Text::new(&format!("secret-text-{}", self.count.get()))
  }
}

/// Renders a memo of a signal it does not own.
struct DetailDoubled {
  doubled: Memo<u64>,
}

impl Component for DetailDoubled {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    let base = BASE.with(|slot| slot.borrow().clone().unwrap());
    Self {
      doubled: ctx.memo(move || base.get() * 2),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    Text::new(&format!("doubled {}", self.doubled.get()))
  }
}

fn mounted() -> (Tree, App, Signal<u32>, Signal<u64>) {
  let base = Signal::new(1_u64);
  BASE.with(|slot| *slot.borrow_mut() = Some(base.clone()));
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(400, 300);
  tree.mount_root::<DetailRoot>(&mut app, ());
  tree.pass_headless(&mut app);
  let count = COUNT.with(|slot| slot.borrow().clone().unwrap());
  (tree, app, count, base)
}

/// Writes the counter's signal, then the memo's source, a pass after each.
fn record(tree: &mut Tree, app: &mut App, count: &Signal<u32>, base: &Signal<u64>, detailed: bool) -> ProfileReport {
  let handle = tree.profiling_handle();
  let id = handle
    .start_recording(RecordingOptions {
      detailed,
      ..Default::default()
    })
    .unwrap()
    .id;
  count.set(1);
  tree.pass_headless(app);
  base.set(5);
  tree.pass_headless(app);
  handle.end(id).unwrap()
}

fn passes(report: &ProfileReport) -> impl Iterator<Item = &PassSample> {
  report.samples.iter().filter_map(|sample| match &sample.data {
    SampleData::Pass(pass) => Some(pass),
    _ => None,
  })
}

fn causes(report: &ProfileReport) -> Vec<RenderCause> {
  report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::UiUpdate(update) => Some(update.causes.clone()),
      _ => None,
    })
    .flatten()
    .collect()
}

#[test]
fn a_detailed_recording_names_components_under_their_phases() {
  let (mut tree, mut app, count, base) = mounted();
  let report = record(&mut tree, &mut app, &count, &base, true);
  assert!(report.recording.unwrap().detailed);

  // The counter's render is a span named by its type, inside the update's rebuild phase.
  let pass = passes(&report)
    .find(|pass| pass.spans.iter().any(|span| span.component == Some("DetailCounter")))
    .expect("a pass rendered the counter");
  let render = pass
    .spans
    .iter()
    .position(|span| span.component == Some("DetailCounter") && span.phase == Phase::Rebuild)
    .unwrap();
  let parent = pass.spans[..render]
    .iter()
    .rev()
    .find(|span| span.depth + 1 == pass.spans[render].depth)
    .unwrap();
  assert_eq!((parent.phase, parent.component), (Phase::Rebuild, None));
  // Every component's after_layout hook is a span below the hook sweep's phase.
  let hooks: Vec<_> = pass
    .spans
    .iter()
    .filter(|span| span.phase == Phase::ComponentAfterLayout)
    .map(|span| span.component)
    .collect();
  assert_eq!(hooks[0], None);
  for name in ["DetailRoot", "DetailCounter", "DetailDoubled"] {
    assert!(hooks.contains(&Some(name)), "{name} in {hooks:?}");
  }
  // The counter's text changed: its nodes were laid out again, counted under it.
  assert!(
    pass
      .component_layouts
      .iter()
      .any(|layout| layout.component == Some("DetailCounter") && layout.nodes > 0),
    "{:?}",
    pass.component_layouts
  );

  // The flame chart and the call tree show the components by name.
  let forest = forest_of(&report.samples, []);
  assert!(forest.spans().iter().any(|span| span.name == "DetailCounter"));
  let tree_rows = forest.call_tree(SpanTrack::Frames);
  assert!(tree_rows.nodes.iter().any(|node| node.name == "DetailDoubled"));
}

#[test]
fn a_detailed_recording_records_the_notifications_that_caused_each_render() {
  let (mut tree, mut app, count, base) = mounted();
  let report = record(&mut tree, &mut app, &count, &base, true);
  let causes = causes(&report);
  // The counter's own signal notified it.
  assert!(
    causes.contains(&RenderCause {
      component: "DetailCounter",
      source: RenderSource {
        trigger: RenderTrigger::Signal,
        value_type: Some("u32"),
      },
      via: None,
    }),
    "{causes:?}"
  );
  // The memo recomputed for the signal it reads, and its new value notified its reader.
  assert!(
    causes.contains(&RenderCause {
      component: "DetailDoubled",
      source: RenderSource {
        trigger: RenderTrigger::Memo,
        value_type: Some("u64"),
      },
      via: Some(RenderSource {
        trigger: RenderTrigger::Signal,
        value_type: Some("u64"),
      }),
    }),
    "{causes:?}"
  );
  // Neither write rendered the root. With `devtools`, `ctx.memo` reads the
  // memo for its debug value in `create`, during the root's render, so the
  // root reads it too and renders again for it, as its causes then say.
  #[cfg(not(feature = "devtools"))]
  assert!(causes.iter().all(|cause| cause.component != "DetailRoot"), "{causes:?}");
}

#[test]
fn without_detailed_nothing_is_attributed_to_components() {
  let (mut tree, mut app, count, base) = mounted();
  let report = record(&mut tree, &mut app, &count, &base, false);
  assert!(!report.recording.unwrap().detailed);
  assert!(passes(&report).count() >= 2);
  for pass in passes(&report) {
    assert!(pass.spans.iter().all(|span| span.component.is_none()));
    assert!(pass.component_layouts.is_empty());
  }
  assert!(causes(&report).is_empty());
}

/// Type names only: no text, key, prop value or id reaches the export, and a
/// bounded session running alongside a detailed recording exports what it
/// always did.
#[cfg(any(feature = "serde", feature = "mcp"))]
#[test]
fn the_export_holds_type_names_and_never_values() {
  let (mut tree, mut app, count, base) = mounted();
  let handle = tree.profiling_handle();
  let bounded = handle.start(SessionOptions::default()).unwrap().id;
  let report = record(&mut tree, &mut app, &count, &base, true);
  let bounded = handle.end(bounded).unwrap();

  let json = report.to_json();
  let text = json.to_string();
  assert!(text.contains("\"component\":\"DetailCounter\""), "{text}");
  assert!(text.contains("\"causes\""));
  assert!(text.contains("\"component_layouts\""));
  for value in ["secret-text", "secret-row-key", "doubled 10"] {
    assert!(!text.contains(value), "{value} in {text}");
  }

  let bounded_json = bounded.to_json().to_string();
  for key in ["\"component\"", "\"causes\"", "\"component_layouts\""] {
    assert!(!bounded_json.contains(key), "{key} in {bounded_json}");
  }
  assert_eq!(bounded.to_json()["schema_version"], 1);
}
