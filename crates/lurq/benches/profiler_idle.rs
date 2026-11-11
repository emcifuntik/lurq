//! What `perf_profile` costs a pass. Run it once without the feature and once
//! with it, on the same machine and session:
//!
//! - `cargo bench -p lurq --bench profiler_idle` reports `off`: the hooks are compiled out.
//! - `cargo bench -p lurq --bench profiler_idle --features perf_profile` reports `idle` (compiled in, no session, as
//!   every DevTools build runs), `session` (one active session recording every pass) and `detailed` (one detailed
//!   recording, with component spans, layout counts and render causes).
//!
//! Scenes, each run through `Tree::pass_headless`:
//!
//! - `static`: a column of 40 rows that does not change; the pass finds the layout cached, so its fixed cost (and the
//!   profiler's share of it) shows.
//! - `signal_change`: the same rows as components, one of which changes its own signal before each pass, so the pass
//!   also refreshes a component and lays the tree out again.

use std::{cell::RefCell, time::Instant};

use criterion::{Bencher, Criterion, criterion_group, criterion_main};
use lurq::{
  app::{
    App, Tree,
    component::Component,
    ctx::Ctx,
    profiler::{ProfilingHandle, RecordingOptions, SessionId},
  },
  components::{Column, Rect, Row, Text},
  core::Signal,
  node::Element,
};

const ROWS: usize = 40;

thread_local! {
  static ROW_SIGNALS: RefCell<Vec<Signal<u32>>> = const { RefCell::new(Vec::new()) };
}

fn row(index: usize, revision: u32) -> Row {
  Row::new()
    .spacing(4.0)
    .child(Rect::new(12.0, 12.0).background("#3b82f6"))
    .child(Text::new(&format!("Row {index} revision {revision}")))
}

fn static_scene() -> Element {
  let mut column = Column::new().spacing(2.0);
  for index in 0..ROWS {
    column = column.child(row(index, 0));
  }
  column.into()
}

struct Root;

impl Component for Root {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let mut column = Column::new().spacing(2.0);
    for index in 0..ROWS {
      column = column.child(ctx.mount_keyed::<RowComponent>(&format!("r{index}"), index));
    }
    column
  }
}

struct RowComponent {
  revision: Signal<u32>,
}

impl Component for RowComponent {
  type Props = usize;

  fn create(ctx: &mut Ctx) -> Self {
    let revision = ctx.signal(0);
    ROW_SIGNALS.with(|signals| signals.borrow_mut().push(revision.clone()));
    Self { revision }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    row(*ctx.props::<usize>(), self.revision.get())
  }
}

/// The configuration this build measures, with the session it keeps open.
fn configurations() -> Vec<&'static str> {
  if cfg!(feature = "perf_profile") {
    vec!["idle", "session", "detailed"]
  } else {
    vec!["off"]
  }
}

fn start_session(handle: &ProfilingHandle, configuration: &str) -> Option<SessionId> {
  match configuration {
    "session" => Some(handle.start(Default::default()).unwrap().id),
    "detailed" => Some(
      handle
        .start_recording(RecordingOptions {
          detailed: true,
          ..Default::default()
        })
        .unwrap()
        .id,
    ),
    _ => None,
  }
}

/// Ends the session; it recorded every pass it timed.
fn end_session(handle: &ProfilingHandle, session: Option<SessionId>) {
  if let Some(id) = session {
    handle.end(id).unwrap();
  }
}

/// Times `step` in `configuration`. A recording keeps every sample, so the
/// `detailed` one is started anew for each batch of iterations; a bounded
/// session stays open across the bench.
fn measure(b: &mut Bencher, handle: &ProfilingHandle, configuration: &str, mut step: impl FnMut()) {
  if configuration != "detailed" {
    let session = start_session(handle, configuration);
    b.iter(&mut step);
    end_session(handle, session);
    return;
  }
  b.iter_custom(|iterations| {
    let session = start_session(handle, configuration);
    let started = Instant::now();
    for _ in 0..iterations {
      step();
    }
    let elapsed = started.elapsed();
    end_session(handle, session);
    elapsed
  });
}

fn bench_profiler_idle(c: &mut Criterion) {
  let mut group = c.benchmark_group("profiler_pass");
  for configuration in configurations() {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(800, 1200);
    tree.set_root(static_scene());
    tree.pass_headless(&mut app);
    let handle = tree.profiling_handle();
    group.bench_function(format!("static/{configuration}"), |b| {
      measure(b, &handle, configuration, || {
        tree.pass_headless(&mut app);
      });
    });

    ROW_SIGNALS.with(|signals| signals.borrow_mut().clear());
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(800, 1200);
    tree.mount_root::<Root>(&mut app, ());
    tree.pass_headless(&mut app);
    let signal = ROW_SIGNALS.with(|signals| signals.borrow()[ROWS / 2].clone());
    let handle = tree.profiling_handle();
    group.bench_function(format!("signal_change/{configuration}"), |b| {
      measure(b, &handle, configuration, || {
        signal.update(|value| *value += 1);
        tree.pass_headless(&mut app);
      });
    });
  }
  group.finish();
}

criterion_group!(benches, bench_profiler_idle);
criterion_main!(benches);
