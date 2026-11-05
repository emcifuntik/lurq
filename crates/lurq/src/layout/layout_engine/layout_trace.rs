//! A trace of what each layout pass lays out from scratch (`perf_profile`
//! only), for finding where a pass loses layout caches. With
//! `LURQ_LAYOUT_TRACE` set to a file path, every pass appends a line for each
//! node it lays out without its cache, saying why, what the node is and the
//! component around it, then a summary line, and flushes: it works in a
//! windowed build without a console. Without `perf_profile` none of it is
//! compiled; without the variable it costs one branch per node laid out.
//! It writes tags, element ids and sizes, never text or values. The guide is
//! `docs/src/content/docs/profiling.md`, "Layout trace".
//!
//! Each line is tab separated, `key=value`:
//! - `layout` lines: `pass`, `reason`, `kind` (layout kind), `tag`, `id` (the element id, `-` for none), `component`
//!   (the tag of the nearest component root at or above the node), `children`, `constraints`.
//! - `pass` lines: `pass`, `root` (its tag), `laid_out` (nodes laid out from scratch), `hits` (nodes served whole from
//!   their cache), `repairs` (nodes whose cached result took repaired children without being laid out again), `ms`.
//!
//! The reasons: `local-dirty` (the node itself changed, or was never laid
//! out), `descendant-dirty` (a descendant changed and the cached result could
//! not be repaired), `no-cached-result` (its cache was not carried over),
//! `other-constraints` (cached under other constraints only),
//! `cached-tree-mismatch` (the cached result no longer fits the node's
//! children), `collapsed` (a line its parent collapses), `repair-*` (a repair
//! of a dirty child that had to lay the node out again), `direct` (laid out
//! without asking the cache).

use std::{
  cell::RefCell,
  fs::{File, OpenOptions},
  io::{BufWriter, Write},
  path::Path,
  sync::Arc,
  time::Instant,
};

use super::layout_kind_profile_name;
use crate::{layout::Constraints, node::node::Node};

const TRACE_ENV: &str = "LURQ_LAYOUT_TRACE";

pub(super) struct LayoutTrace {
  state: Option<RefCell<TraceState>>,
}

struct TraceState {
  out: BufWriter<File>,
  pass: u64,
  started: Instant,
  laid_out: usize,
  hits: usize,
  repairs: usize,
  /// The tags of the component roots around the node being laid out,
  /// innermost last.
  components: Vec<Arc<str>>,
  /// Why the next layout from scratch runs, set right before it.
  reason: Option<&'static str>,
}

impl LayoutTrace {
  /// The trace `LURQ_LAYOUT_TRACE` asks for, if it names a file that opens.
  pub(super) fn from_env() -> Self {
    match std::env::var_os(TRACE_ENV) {
      Some(path) => Self::to_file(Path::new(&path)),
      None => Self { state: None },
    }
  }

  /// A trace appended to the file at `path`; off when it does not open.
  pub(super) fn to_file(path: &Path) -> Self {
    let state = match OpenOptions::new().create(true).append(true).open(path) {
      Ok(file) => Some(RefCell::new(TraceState {
        out: BufWriter::new(file),
        pass: 0,
        started: Instant::now(),
        laid_out: 0,
        hits: 0,
        repairs: 0,
        components: Vec::new(),
        reason: None,
      })),
      Err(error) => {
        tracing::warn!(target: "layout-profile", "cannot open {TRACE_ENV} file {path:?}: {error}");
        None
      }
    };
    Self { state }
  }

  #[inline]
  pub(super) fn is_on(&self) -> bool {
    self.state.is_some()
  }

  pub(super) fn begin_pass(&self) {
    let Some(state) = &self.state else {
      return;
    };
    let mut state = state.borrow_mut();
    state.pass += 1;
    state.started = Instant::now();
    state.laid_out = 0;
    state.hits = 0;
    state.repairs = 0;
    state.components.clear();
    state.reason = None;
  }

  pub(super) fn end_pass(&self, root: &Node) {
    let Some(state) = &self.state else {
      return;
    };
    let mut state = state.borrow_mut();
    let state = &mut *state;
    let _ = writeln!(
      state.out,
      "pass\tpass={}\troot={}\tlaid_out={}\thits={}\trepairs={}\tms={:.3}",
      state.pass,
      root.tag_name(),
      state.laid_out,
      state.hits,
      state.repairs,
      state.started.elapsed().as_secs_f64() * 1000.0,
    );
    let _ = state.out.flush();
  }

  /// Enters `node`: a component root becomes the component around what is
  /// laid out below it. Returns whether [`Self::leave`] has one to drop.
  pub(super) fn enter(&self, node: &Node) -> bool {
    let Some(state) = &self.state else {
      return false;
    };
    if node.component_slot_id().is_none() {
      return false;
    }
    let tag: Arc<str> = Arc::from(node.tag_name());
    state.borrow_mut().components.push(tag);
    true
  }

  pub(super) fn leave(&self, entered: bool) {
    if entered && let Some(state) = &self.state {
      state.borrow_mut().components.pop();
    }
  }

  pub(super) fn set_reason(&self, reason: &'static str) {
    if let Some(state) = &self.state {
      state.borrow_mut().reason = Some(reason);
    }
  }

  pub(super) fn hit(&self) {
    if let Some(state) = &self.state {
      state.borrow_mut().hits += 1;
    }
  }

  pub(super) fn repaired(&self) {
    if let Some(state) = &self.state {
      state.borrow_mut().repairs += 1;
    }
  }

  /// Records that `node` is laid out from scratch under `constraints`, for
  /// the reason set last.
  pub(super) fn laid_out(&self, node: &Node, constraints: Constraints) {
    let Some(state) = &self.state else {
      return;
    };
    let mut state = state.borrow_mut();
    let state = &mut *state;
    state.laid_out += 1;
    let reason = state.reason.take().unwrap_or("direct");
    let component = state.components.last().map_or("-", |tag| &**tag);
    let _ = writeln!(
      state.out,
      "layout\tpass={}\treason={reason}\tkind={}\ttag={}\tid={}\tcomponent={component}\tchildren={}\tconstraints={:.1}..{:.1}x{:.1}..{:.1}",
      state.pass,
      layout_kind_profile_name(node.layout_kind()),
      node.tag_name(),
      node.element_id.as_deref().unwrap_or("-"),
      node.children().len(),
      constraints.min_width,
      constraints.max_width,
      constraints.min_height,
      constraints.max_height,
    );
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::LayoutTrace;
  use crate::{
    app::glyph_engine::GlyphEngine,
    components::{Column, Rect},
    layout::{Constraints, Size, layout_engine::LayoutEngine},
    node::Element,
  };

  /// A first pass lists every node it lays out, with why; a second pass on
  /// the unchanged tree lays out nothing and serves the root whole.
  #[test]
  fn a_traced_pass_lists_what_it_lays_out_and_sums_it_up() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/layout-trace");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("trace-test-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut engine = LayoutEngine::new();
    engine.trace = LayoutTrace::to_file(&path);
    let mut glyph_engine = GlyphEngine::new();
    let root = Element::from(
      Column::new()
        .id("traced")
        .child(Rect::new(10.0, 10.0))
        .child(Rect::new(20.0, 20.0)),
    )
    .into_node();
    let constraints = Constraints::loose(Size::new(400.0, 400.0));

    engine.compute(&mut glyph_engine, &root, constraints, false);
    engine.compute(&mut glyph_engine, &root, constraints, false);

    let log = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<&str> = log.lines().collect();
    let layouts: Vec<&&str> = lines.iter().filter(|line| line.starts_with("layout\t")).collect();
    assert_eq!(layouts.len(), 3, "{log}");
    assert!(
      layouts
        .iter()
        .all(|line| line.contains("\tpass=1\treason=local-dirty\t")),
      "{log}"
    );
    assert!(layouts.iter().any(|line| line.contains("\tid=traced\t")), "{log}");
    let passes: Vec<&&str> = lines.iter().filter(|line| line.starts_with("pass\t")).collect();
    assert_eq!(passes.len(), 2, "{log}");
    assert!(passes[0].contains("\tlaid_out=3\t"), "{log}");
    assert!(passes[1].contains("\tlaid_out=0\thits=1\t"), "{log}");
  }
}
