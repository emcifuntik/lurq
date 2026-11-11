//! What `perf_profile` costs a pass. Run it once without the feature and once
//! with it, on the same machine and session:
//!
//! - `cargo bench -p lurq --bench profiler_idle` reports `off`: the hooks are compiled out.
//! - `cargo bench -p lurq --bench profiler_idle --features perf_profile` reports `idle` (compiled in, no session, as
//!   every DevTools build runs) and `session` (one active session recording every pass).
//!
//! Scenes, each run through `Tree::pass_headless`:
//!
//! - `static`: a column of 40 rows that does not change; the pass finds the layout cached, so its fixed cost (and the
//!   profiler's share of it) shows.
//! - `signal_change`: the same rows as components, one of which changes its own signal before each pass, so the pass
//!   also refreshes a component and lays the tree out again.

use std::cell::RefCell;

use criterion::{Criterion, criterion_group, criterion_main};
use lurq::{
  app::{App, Tree, component::Component, ctx::Ctx},
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
    vec!["idle", "session"]
  } else {
    vec!["off"]
  }
}

fn start_session(tree: &Tree, configuration: &str) -> Option<lurq::app::profiler::SessionId> {
  (configuration == "session").then(|| tree.profiling_handle().start(Default::default()).unwrap().id)
}

/// Ends the session; it recorded every pass the bench ran (none when a filter skipped the bench).
fn end_session(tree: &Tree, session: Option<lurq::app::profiler::SessionId>) {
  if let Some(id) = session {
    tree.profiling_handle().end(id).unwrap();
  }
}

fn bench_profiler_idle(c: &mut Criterion) {
  let mut group = c.benchmark_group("profiler_pass");
  for configuration in configurations() {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(800, 1200);
    tree.set_root(static_scene());
    tree.pass_headless(&mut app);
    let session = start_session(&tree, configuration);
    group.bench_function(format!("static/{configuration}"), |b| {
      b.iter(|| tree.pass_headless(&mut app));
    });
    end_session(&tree, session);

    ROW_SIGNALS.with(|signals| signals.borrow_mut().clear());
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(800, 1200);
    tree.mount_root::<Root>(&mut app, ());
    tree.pass_headless(&mut app);
    let signal = ROW_SIGNALS.with(|signals| signals.borrow()[ROWS / 2].clone());
    let session = start_session(&tree, configuration);
    group.bench_function(format!("signal_change/{configuration}"), |b| {
      b.iter(|| {
        signal.update(|value| *value += 1);
        tree.pass_headless(&mut app)
      });
    });
    end_session(&tree, session);
  }
  group.finish();
}

criterion_group!(benches, bench_profiler_idle);
criterion_main!(benches);
