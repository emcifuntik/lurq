//! Recorded spans as trees, and the tables made from them: a call tree
//! (top-down) and a bottom-up table, each with total time, self time and
//! count per span name. Plain data over the content-free samples of a report,
//! for DevTools, export and MCP alike; it records nothing itself.
//!
//! A pass's phase spans become one tree under the pass, an application
//! scope's parent chain another tree per root scope. Spans are trimmed to
//! their parent and to the end of an earlier sibling (phases entered on
//! another thread can overlap), so the self times of a tree add up to its
//! root's duration and no time counts twice.
use std::{collections::HashMap, sync::Arc};

use super::{ApplicationLane, ApplicationScopeSample, PassSample, ProfileSample, SampleData};

/// The name of the root span of a pass.
pub const PASS_SPAN_NAME: &str = "pass";

/// Where a span was recorded: toolkit phases on the UI frames track,
/// application scopes on the track of their lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpanTrack {
  Frames,
  Application(ApplicationLane),
}

impl SpanTrack {
  /// Every track, in the order they are drawn.
  pub const ALL: [Self; 3] = [
    Self::Frames,
    Self::Application(ApplicationLane::Ui),
    Self::Application(ApplicationLane::Worker),
  ];

  pub fn name(self) -> &'static str {
    match self {
      Self::Frames => "frames",
      Self::Application(ApplicationLane::Ui) => "app ui",
      Self::Application(ApplicationLane::Worker) => "app worker",
    }
  }
}

/// One span of a [`SpanForest`]; times in nanoseconds since the collector
/// epoch, after trimming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForestSpan {
  pub name: &'static str,
  pub track: SpanTrack,
  pub start_ns: u64,
  pub end_ns: u64,
  /// Nesting below its tree's root, which is 0.
  pub depth: u32,
  /// The index of its parent in the forest; always below its own.
  pub parent: Option<u32>,
  /// Its duration less its children's.
  pub self_ns: u64,
  /// The sequence of the pass sample it belongs to; None for an application scope.
  pub sample: Option<u64>,
}

impl ForestSpan {
  pub fn duration_ns(&self) -> u64 {
    self.end_ns - self.start_ns
  }
}

/// Span trees, each parent before its children and the children of a parent
/// in start order.
#[derive(Clone, Debug, Default)]
pub struct SpanForest {
  spans: Vec<ForestSpan>,
  /// Per span, the end of its last child so far: where a later child starts at the earliest.
  children_end: Vec<u64>,
}

impl SpanForest {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn spans(&self) -> &[ForestSpan] {
    &self.spans
  }

  pub fn len(&self) -> usize {
    self.spans.len()
  }

  pub fn is_empty(&self) -> bool {
    self.spans.is_empty()
  }

  /// Adds the pass of `sample` as one tree: its root is the pass, named
  /// [`PASS_SPAN_NAME`], lasting `PassSample::total` from the pass's first
  /// span (or from the sample's start without spans), so the roots of a set
  /// of passes add up to their pass time. Other samples add nothing.
  pub fn push_pass(&mut self, sample: &ProfileSample) {
    let SampleData::Pass(pass) = &sample.data else {
      return;
    };
    self.push_pass_spans(sample.sequence, sample.started_ms, pass);
  }

  fn push_pass_spans(&mut self, sequence: u64, started_ms: f64, pass: &PassSample) {
    let start_ns = pass
      .spans
      .first()
      .map_or_else(|| ms_to_ns(started_ms), |span| span.start_ns);
    let total_ns = u64::try_from(pass.total.as_nanos()).unwrap_or(u64::MAX);
    let root = self.push(
      PASS_SPAN_NAME,
      SpanTrack::Frames,
      None,
      start_ns,
      start_ns.saturating_add(total_ns),
      Some(sequence),
    );
    // stack[d] is the latest span at depth d, the root at 0.
    let mut stack = vec![root];
    for span in pass.spans.iter().skip(1) {
      let depth = usize::from(span.depth.max(1));
      // A span deeper than one below the latest is put under the deepest one.
      stack.truncate(depth.min(stack.len()));
      let parent = *stack.last().expect("the root stays");
      let index = self.push(
        span.phase.name(),
        SpanTrack::Frames,
        Some(parent),
        span.start_ns,
        span.end_ns,
        Some(sequence),
      );
      stack.push(index);
    }
  }

  /// Adds application scopes, each under its parent when that is among them
  /// on the same lane, else as a root of its own.
  pub fn push_application_scopes<'a>(&mut self, scopes: impl IntoIterator<Item = &'a ApplicationScopeSample>) {
    let mut scopes: Vec<_> = scopes.into_iter().collect();
    // Parents start no later than their children; a parent first on ties.
    scopes.sort_by(|a, b| {
      a.scope
        .started_ms
        .total_cmp(&b.scope.started_ms)
        .then(a.scope.depth.cmp(&b.scope.depth))
        .then(a.scope.id.0.cmp(&b.scope.id.0))
    });
    let mut indices: HashMap<u64, u32> = HashMap::with_capacity(scopes.len());
    for sample in scopes {
      let track = SpanTrack::Application(sample.scope.lane);
      let parent = sample
        .scope
        .parent_id
        .and_then(|parent| indices.get(&parent.0).copied())
        .filter(|parent| self.spans[*parent as usize].track == track);
      let start_ns = ms_to_ns(sample.scope.started_ms);
      let wall_ns = u64::try_from(sample.wall.as_nanos()).unwrap_or(u64::MAX);
      let index = self.push(
        sample.scope.label,
        track,
        parent,
        start_ns,
        start_ns.saturating_add(wall_ns),
        None,
      );
      indices.insert(sample.scope.id.0, index);
    }
  }

  /// Adds a span trimmed to its parent and after the parent's earlier
  /// children, and takes its time from the parent's self time.
  fn push(
    &mut self,
    name: &'static str,
    track: SpanTrack,
    parent: Option<u32>,
    start_ns: u64,
    end_ns: u64,
    sample: Option<u64>,
  ) -> u32 {
    let index = u32::try_from(self.spans.len()).expect("fewer than 2^32 spans");
    let (start_ns, end_ns, depth) = match parent {
      Some(parent) => {
        let parent_index = parent as usize;
        let outer = self.spans[parent_index];
        let start = start_ns.max(self.children_end[parent_index]).min(outer.end_ns);
        let end = end_ns.min(outer.end_ns).max(start);
        self.children_end[parent_index] = end;
        let duration = end - start;
        self.spans[parent_index].self_ns -= duration;
        (start, end, outer.depth + 1)
      }
      None => (start_ns, end_ns.max(start_ns), 0),
    };
    self.spans.push(ForestSpan {
      name,
      track,
      start_ns,
      end_ns,
      depth,
      parent,
      self_ns: end_ns - start_ns,
      sample,
    });
    self.children_end.push(start_ns);
    index
  }

  /// Total time of the roots on `track`: the pass time of the passes in the forest for `Frames`.
  pub fn root_total_ns(&self, track: SpanTrack) -> u64 {
    self
      .spans
      .iter()
      .filter(|span| span.track == track && span.parent.is_none())
      .map(ForestSpan::duration_ns)
      .sum()
  }

  /// The spans of `track` by depth, each depth in start order; spans of one
  /// depth never overlap within a tree.
  pub fn rows(&self, track: SpanTrack) -> Vec<Vec<u32>> {
    let mut rows: Vec<Vec<u32>> = Vec::new();
    for (index, span) in self.spans.iter().enumerate() {
      if span.track != track {
        continue;
      }
      let depth = span.depth as usize;
      if rows.len() <= depth {
        rows.resize_with(depth + 1, Vec::new);
      }
      rows[depth].push(index as u32);
    }
    for row in &mut rows {
      row.sort_by_key(|index| self.spans[*index as usize].start_ns);
    }
    rows
  }

  /// The call tree of `track`: the spans merged by their names' path from the root.
  pub fn call_tree(&self, track: SpanTrack) -> CallTree {
    let mut tree = CallTree::default();
    let mut keys: HashMap<(u32, &'static str), u32> = HashMap::new();
    // The call tree node of each span of the forest; u32::MAX for other tracks.
    let mut node_of = vec![u32::MAX; self.spans.len()];
    for (index, span) in self.spans.iter().enumerate() {
      if span.track != track {
        continue;
      }
      let parent = span.parent.map_or(u32::MAX, |parent| node_of[parent as usize]);
      let node = tree.node_for(&mut keys, parent, span.name);
      let entry = &mut tree.nodes[node as usize];
      entry.total_ns += span.duration_ns();
      entry.self_ns += span.self_ns;
      entry.count += 1;
      node_of[index] = node;
    }
    tree.total_ns = tree.roots.iter().map(|root| tree.nodes[*root as usize].total_ns).sum();
    tree
  }

  /// The bottom-up table of `track`: a row per span name with its self time
  /// and its total time, and under it the names that called it, outward.
  /// A span inside another of its name adds its self time and count but not
  /// its total, which the outer one already holds.
  pub fn bottom_up(&self, track: SpanTrack) -> CallTree {
    let mut tree = CallTree::default();
    let mut keys: HashMap<(u32, &'static str), u32> = HashMap::new();
    for span in self.spans.iter().filter(|span| span.track == track) {
      let outermost = !self.ancestors(span).any(|ancestor| ancestor.name == span.name);
      let total_ns = if outermost { span.duration_ns() } else { 0 };
      let mut node = tree.node_for(&mut keys, u32::MAX, span.name);
      tree.add(node, total_ns, span.self_ns);
      for caller in self.ancestors(span) {
        node = tree.node_for(&mut keys, node, caller.name);
        tree.add(node, total_ns, span.self_ns);
      }
    }
    tree.total_ns = self.root_total_ns(track);
    tree
  }

  fn ancestors<'a>(&'a self, span: &ForestSpan) -> impl Iterator<Item = &'a ForestSpan> + 'a {
    std::iter::successors(span.parent.map(|parent| &self.spans[parent as usize]), |ancestor| {
      ancestor.parent.map(|parent| &self.spans[parent as usize])
    })
  }

  /// The span of `row` (from [`Self::rows`]) that covers `at_ns`, else the
  /// nearest one within `tolerance_ns`.
  pub fn span_at(&self, row: &[u32], at_ns: u64, tolerance_ns: u64) -> Option<u32> {
    let first = row.partition_point(|index| self.spans[*index as usize].end_ns < at_ns.saturating_sub(tolerance_ns));
    row[first..]
      .iter()
      .take_while(|index| self.spans[**index as usize].start_ns <= at_ns.saturating_add(tolerance_ns))
      .min_by_key(|index| {
        let span = &self.spans[**index as usize];
        if at_ns < span.start_ns {
          span.start_ns - at_ns
        } else {
          at_ns.saturating_sub(span.end_ns)
        }
      })
      .copied()
  }
}

/// Milliseconds since the collector epoch, as a report's `*_ms`, in nanoseconds.
pub fn ms_to_ns(ms: f64) -> u64 {
  (ms.max(0.0) * 1e6).round() as u64
}

/// A call tree or bottom-up table: rows with their children, in the order
/// [`CallTree::sort`] put them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallTree {
  pub nodes: Vec<CallNode>,
  pub roots: Vec<u32>,
  /// What `percent` divides by: the roots' total time in the forest's track.
  pub total_ns: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallNode {
  pub name: &'static str,
  /// Rows from a root, which is 0.
  pub depth: u32,
  pub total_ns: u64,
  pub self_ns: u64,
  pub count: u64,
  pub children: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallSort {
  Total,
  SelfTime,
  Count,
  Name,
}

impl CallTree {
  fn node_for(&mut self, keys: &mut HashMap<(u32, &'static str), u32>, parent: u32, name: &'static str) -> u32 {
    if let Some(node) = keys.get(&(parent, name)) {
      return *node;
    }
    let node = u32::try_from(self.nodes.len()).expect("fewer than 2^32 rows");
    let depth = if parent == u32::MAX {
      self.roots.push(node);
      0
    } else {
      self.nodes[parent as usize].children.push(node);
      self.nodes[parent as usize].depth + 1
    };
    self.nodes.push(CallNode {
      name,
      depth,
      total_ns: 0,
      self_ns: 0,
      count: 0,
      children: Vec::new(),
    });
    keys.insert((parent, name), node);
    node
  }

  fn add(&mut self, node: u32, total_ns: u64, self_ns: u64) {
    let entry = &mut self.nodes[node as usize];
    entry.total_ns += total_ns;
    entry.self_ns += self_ns;
    entry.count += 1;
  }

  /// `part` as a percentage of the tree's total; 0 for an empty tree.
  pub fn percent(&self, part_ns: u64) -> f64 {
    if self.total_ns == 0 {
      0.0
    } else {
      part_ns as f64 * 100.0 / self.total_ns as f64
    }
  }

  /// Orders the roots and every row's children by `key`, largest first when
  /// `descending`; ties keep the name order.
  pub fn sort(&mut self, key: CallSort, descending: bool) {
    let nodes = &self.nodes;
    let order = |a: &u32, b: &u32| {
      let (a, b) = (&nodes[*a as usize], &nodes[*b as usize]);
      let ordering = match key {
        CallSort::Total => a.total_ns.cmp(&b.total_ns),
        CallSort::SelfTime => a.self_ns.cmp(&b.self_ns),
        CallSort::Count => a.count.cmp(&b.count),
        CallSort::Name => a.name.cmp(b.name),
      };
      let ordering = if descending { ordering.reverse() } else { ordering };
      ordering.then_with(|| a.name.cmp(b.name))
    };
    let mut roots = std::mem::take(&mut self.roots);
    roots.sort_by(order);
    let mut children: Vec<Vec<u32>> = self.nodes.iter().map(|node| node.children.clone()).collect();
    for list in &mut children {
      list.sort_by(order);
    }
    self.roots = roots;
    for (node, list) in self.nodes.iter_mut().zip(children) {
      node.children = list;
    }
  }

  /// The rows shown when the rows `expanded` says are open: depth first, in order.
  pub fn visible_rows(&self, expanded: impl Fn(u32, &CallNode) -> bool) -> Vec<u32> {
    let mut rows = Vec::new();
    let mut stack: Vec<u32> = self.roots.iter().rev().copied().collect();
    while let Some(node) = stack.pop() {
      rows.push(node);
      let entry = &self.nodes[node as usize];
      if expanded(node, entry) {
        stack.extend(entry.children.iter().rev().copied());
      }
    }
    rows
  }
}

/// A forest of the passes among `samples` and the application scopes among
/// `scopes`, for one time range or pass.
pub fn forest_of<'a>(
  samples: impl IntoIterator<Item = &'a Arc<ProfileSample>>,
  scopes: impl IntoIterator<Item = &'a Arc<ApplicationScopeSample>>,
) -> SpanForest {
  let mut forest = SpanForest::new();
  for sample in samples {
    forest.push_pass(sample);
  }
  forest.push_application_scopes(scopes.into_iter().map(Arc::as_ref));
  forest
}
