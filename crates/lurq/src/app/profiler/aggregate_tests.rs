use std::{sync::Arc, time::Duration};

use super::*;

const MS: u64 = 1_000_000;

fn span(phase: Phase, depth: u8, start_ms: u64, end_ms: u64) -> PhaseSpan {
  PhaseSpan {
    phase,
    depth,
    start_ns: start_ms * MS,
    end_ns: end_ms * MS,
  }
}

/// A pass taking `total_ms` from its first span, which is the pass itself.
fn pass(sequence: u64, total_ms: u64, spans: Vec<PhaseSpan>) -> Arc<ProfileSample> {
  let started_ms = spans.first().map_or(0.0, |span| (span.start_ns / MS) as f64);
  Arc::new(ProfileSample {
    sequence,
    window: "main".into(),
    started_ms,
    completed_ms: started_ms + total_ms as f64,
    data: SampleData::Pass(PassSample {
      frame_id: Some(sequence),
      rendered: true,
      cached_render_list: false,
      total: Duration::from_millis(total_ms),
      layout_update: Duration::ZERO,
      layout_compute: Duration::ZERO,
      component_after_layout: Duration::ZERO,
      layout_recalculated: false,
      canvas_recording: Duration::ZERO,
      canvas_preparation: Duration::ZERO,
      canvas_text: None,
      backend: "none",
      frame: None,
      spans,
      dropped_spans: 0,
    }),
  })
}

fn scope(
  id: u64,
  parent: Option<u64>,
  depth: u32,
  label: &'static str,
  lane: ApplicationLane,
  start_ms: f64,
  wall_ms: u64,
) -> Arc<ApplicationScopeSample> {
  Arc::new(ApplicationScopeSample {
    scope: ApplicationScopeInfo {
      id: ApplicationScopeId(id),
      parent_id: parent.map(ApplicationScopeId),
      depth,
      window: "main".into(),
      label,
      lane,
      started_ms: start_ms,
    },
    completed_ms: start_ms + wall_ms as f64,
    wall: Duration::from_millis(wall_ms),
  })
}

/// pass 0..10: layout_update 1..7 holds ui_rebuild 1..3 (holding ui_commit
/// 2..3) and layout_compute 4..6; render 7..9.
fn layered_pass(sequence: u64, at_ms: u64) -> Arc<ProfileSample> {
  pass(
    sequence,
    10,
    vec![
      span(Phase::PassSetup, 0, at_ms, at_ms + 10),
      span(Phase::LayoutUpdate, 1, at_ms + 1, at_ms + 7),
      span(Phase::Rebuild, 2, at_ms + 1, at_ms + 3),
      span(Phase::Commit, 3, at_ms + 2, at_ms + 3),
      span(Phase::LayoutCompute, 2, at_ms + 4, at_ms + 6),
      span(Phase::Render, 1, at_ms + 7, at_ms + 9),
    ],
  )
}

fn node<'a>(tree: &'a CallTree, path: &[&str]) -> &'a CallNode {
  let mut rows = &tree.roots;
  let mut found = None;
  for name in path {
    let index = rows
      .iter()
      .copied()
      .find(|index| tree.nodes[*index as usize].name == *name)
      .unwrap_or_else(|| panic!("no row {name} in {path:?}"));
    found = Some(&tree.nodes[index as usize]);
    rows = &tree.nodes[index as usize].children;
  }
  found.expect("a path")
}

fn times(node: &CallNode) -> (u64, u64, u64) {
  (node.total_ns / MS, node.self_ns / MS, node.count)
}

#[test]
fn nested_phases_take_their_time_from_the_parent_once() {
  let forest = forest_of(&[layered_pass(1, 100)], &[]);
  let tree = forest.call_tree(SpanTrack::Frames);
  assert_eq!(tree.total_ns, 10 * MS);
  assert_eq!(times(node(&tree, &["pass"])), (10, 2, 1));
  assert_eq!(times(node(&tree, &["pass", "layout_update"])), (6, 2, 1));
  assert_eq!(times(node(&tree, &["pass", "layout_update", "ui_rebuild"])), (2, 1, 1));
  assert_eq!(
    times(node(&tree, &["pass", "layout_update", "ui_rebuild", "ui_commit"])),
    (1, 1, 1)
  );
  assert_eq!(
    times(node(&tree, &["pass", "layout_update", "layout_compute"])),
    (2, 2, 1)
  );
  assert_eq!(times(node(&tree, &["pass", "render"])), (2, 2, 1));
  // Self times partition the pass: nested phases are not counted twice.
  let self_sum: u64 = tree.nodes.iter().map(|node| node.self_ns).sum();
  assert_eq!(self_sum, tree.total_ns);
  assert_eq!(tree.percent(node(&tree, &["pass", "layout_update"]).total_ns), 60.0);
}

#[test]
fn a_range_merges_the_same_path_of_every_pass_and_its_totals_are_the_pass_time() {
  let passes = [layered_pass(1, 0), layered_pass(2, 20), layered_pass(3, 40)];
  let forest = forest_of(&passes, &[]);
  let pass_time: Duration = passes
    .iter()
    .map(|sample| match &sample.data {
      SampleData::Pass(pass) => pass.total,
      _ => unreachable!(),
    })
    .sum();
  let tree = forest.call_tree(SpanTrack::Frames);
  assert_eq!(tree.total_ns as u128, pass_time.as_nanos());
  assert_eq!(forest.root_total_ns(SpanTrack::Frames) as u128, pass_time.as_nanos());
  assert_eq!(tree.roots.len(), 1);
  assert_eq!(times(node(&tree, &["pass"])), (30, 6, 3));
  assert_eq!(times(node(&tree, &["pass", "layout_update", "ui_rebuild"])), (6, 3, 3));

  let bottom_up = forest.bottom_up(SpanTrack::Frames);
  assert_eq!(bottom_up.total_ns, tree.total_ns);
  // Top rows: every name once, with its self time; together the pass time.
  let self_sum: u64 = bottom_up
    .roots
    .iter()
    .map(|root| bottom_up.nodes[*root as usize].self_ns)
    .sum();
  assert_eq!(self_sum, tree.total_ns);
  assert_eq!(times(node(&bottom_up, &["ui_commit"])), (3, 3, 3));
  assert_eq!(times(node(&bottom_up, &["layout_update"])), (18, 6, 3));
  // Under a name, its callers outward.
  assert_eq!(
    times(node(&bottom_up, &["ui_commit", "ui_rebuild", "layout_update", "pass"])),
    (3, 3, 3)
  );
  assert!(node(&bottom_up, &["pass"]).children.is_empty());
}

#[test]
fn a_phase_inside_one_of_its_name_counts_its_total_once_in_bottom_up() {
  // ui_rebuild 1..7 holds ui_commit 2..6, which holds ui_rebuild 3..5.
  let forest = forest_of(
    &[pass(
      1,
      10,
      vec![
        span(Phase::PassSetup, 0, 0, 10),
        span(Phase::Rebuild, 1, 1, 7),
        span(Phase::Commit, 2, 2, 6),
        span(Phase::Rebuild, 3, 3, 5),
      ],
    )],
    &[],
  );
  let bottom_up = forest.bottom_up(SpanTrack::Frames);
  // 6 ms of ui_rebuild on the stack, not 6 + 2; its self time is 2 + 2.
  assert_eq!(times(node(&bottom_up, &["ui_rebuild"])), (6, 4, 2));
  assert_eq!(times(node(&bottom_up, &["ui_rebuild", "pass"])), (6, 2, 1));
  assert_eq!(times(node(&bottom_up, &["ui_rebuild", "ui_commit"])), (0, 2, 1));
  // The call tree keeps each place apart.
  let tree = forest.call_tree(SpanTrack::Frames);
  assert_eq!(
    times(node(&tree, &["pass", "ui_rebuild", "ui_commit", "ui_rebuild"])),
    (2, 2, 1)
  );
  assert_eq!(times(node(&tree, &["pass", "ui_rebuild"])), (6, 2, 1));
}

#[test]
fn overlapping_and_overhanging_spans_are_trimmed_so_no_time_counts_twice() {
  // Two render-thread phases overlap at depth 1, the second runs past the pass.
  let forest = forest_of(
    &[pass(
      1,
      10,
      vec![
        span(Phase::PassSetup, 0, 0, 10),
        span(Phase::RenderEncode, 1, 2, 6),
        span(Phase::RenderSubmit, 1, 4, 12),
        // Deeper than one below the latest span: put under it.
        span(Phase::CanvasCommands, 3, 7, 8),
      ],
    )],
    &[],
  );
  let spans = forest.spans();
  let shape: Vec<_> = spans
    .iter()
    .map(|span| {
      (
        span.name,
        span.depth,
        span.start_ns / MS,
        span.end_ns / MS,
        span.self_ns / MS,
      )
    })
    .collect();
  assert_eq!(
    shape,
    [
      ("pass", 0, 0, 10, 2),
      ("render_encode", 1, 2, 6, 4),
      ("render_submit", 1, 6, 10, 3),
      ("canvas_commands", 2, 7, 8, 1),
    ]
  );
  let tree = forest.call_tree(SpanTrack::Frames);
  let self_sum: u64 = tree.nodes.iter().map(|node| node.self_ns).sum();
  assert_eq!(self_sum, 10 * MS);
}

#[test]
fn a_pass_without_spans_is_one_root_of_its_total() {
  let mut sample = (*pass(7, 5, Vec::new())).clone();
  sample.started_ms = 40.0;
  let forest = forest_of(&[Arc::new(sample)], &[]);
  assert_eq!(
    forest.spans(),
    &[ForestSpan {
      name: PASS_SPAN_NAME,
      track: SpanTrack::Frames,
      start_ns: 40 * MS,
      end_ns: 45 * MS,
      depth: 0,
      parent: None,
      self_ns: 5 * MS,
      sample: Some(7),
    }]
  );
}

#[test]
fn application_scopes_form_trees_per_lane_by_parent_id() {
  let scopes = [
    // Given out of order: the child before its parent.
    scope(2, Some(1), 1, "decode", ApplicationLane::Worker, 3.0, 4),
    scope(1, None, 0, "load", ApplicationLane::Worker, 1.0, 10),
    scope(3, None, 0, "click", ApplicationLane::Ui, 2.0, 1),
    // Its parent is not among them (dropped, or outside the range): a root.
    scope(4, Some(99), 1, "parse", ApplicationLane::Worker, 20.0, 2),
  ];
  let forest = forest_of(&[layered_pass(1, 0)], &scopes);
  let worker = SpanTrack::Application(ApplicationLane::Worker);
  let tree = forest.call_tree(worker);
  assert_eq!(times(node(&tree, &["load"])), (10, 6, 1));
  assert_eq!(times(node(&tree, &["load", "decode"])), (4, 4, 1));
  assert_eq!(times(node(&tree, &["parse"])), (2, 2, 1));
  assert_eq!(tree.total_ns, 12 * MS);
  let ui = forest.call_tree(SpanTrack::Application(ApplicationLane::Ui));
  assert_eq!(times(node(&ui, &["click"])), (1, 1, 1));
  // Each lane has its own rows; the frames track keeps only the pass.
  let rows = forest.rows(worker);
  let names = |row: &[u32]| {
    row
      .iter()
      .map(|index| forest.spans()[*index as usize].name)
      .collect::<Vec<_>>()
  };
  assert_eq!(names(&rows[0]), ["load", "parse"]);
  assert_eq!(names(&rows[1]), ["decode"]);
  assert_eq!(forest.call_tree(SpanTrack::Frames).total_ns, 10 * MS);
  assert!(
    forest
      .spans()
      .iter()
      .filter(|span| span.track != SpanTrack::Frames)
      .all(|span| span.sample.is_none())
  );
}

#[test]
fn sorting_orders_every_level_and_visible_rows_follow_expansion() {
  let mut tree = forest_of(&[layered_pass(1, 0)], &[]).call_tree(SpanTrack::Frames);
  tree.sort(CallSort::SelfTime, true);
  let pass = node(&tree, &["pass"]);
  let children: Vec<_> = pass
    .children
    .iter()
    .map(|index| tree.nodes[*index as usize].name)
    .collect();
  // layout_update and render both have 2 ms self: the name breaks the tie.
  assert_eq!(children, ["layout_update", "render"]);
  tree.sort(CallSort::Name, false);
  let names = |rows: Vec<u32>| {
    rows
      .into_iter()
      .map(|index| tree.nodes[index as usize].name)
      .collect::<Vec<_>>()
  };
  assert_eq!(
    names(tree.visible_rows(|_, node| node.depth < 1)),
    ["pass", "layout_update", "render"]
  );
  assert_eq!(
    names(tree.visible_rows(|_, _| true)),
    [
      "pass",
      "layout_update",
      "layout_compute",
      "ui_rebuild",
      "ui_commit",
      "render"
    ]
  );
}

#[test]
fn span_at_finds_the_covering_span_or_the_nearest_within_tolerance() {
  let forest = forest_of(&[layered_pass(1, 0), layered_pass(2, 20)], &[]);
  let rows = forest.rows(SpanTrack::Frames);
  let name = |found: Option<u32>| found.map(|index| forest.spans()[index as usize].name);
  assert_eq!(name(forest.span_at(&rows[1], 5 * MS, 0)), Some("layout_update"));
  assert_eq!(name(forest.span_at(&rows[1], 28 * MS, 0)), Some("render"));
  assert_eq!(
    forest.spans()[forest.span_at(&rows[1], 25 * MS, 0).unwrap() as usize].sample,
    Some(2)
  );
  // 9.5 ms is past render (7..9) in pass 1 and before pass 2.
  assert_eq!(forest.span_at(&rows[1], 9 * MS + MS / 2, 0), None);
  assert_eq!(name(forest.span_at(&rows[1], 9 * MS + MS / 2, MS)), Some("render"));
}
