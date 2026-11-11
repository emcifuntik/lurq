use std::{sync::Arc, time::Duration};

use super::*;
use crate::app::{
  devtools::profiler_capture::{IDLE_GAP_MS, fps, pass_detail, phase_rows},
  profiler::{
    FrameProfile, PassAllocations, PassCounters, PassSample, Phase, PhaseSpan, ProfileSample, SampleData,
    producer::WindowProfiler,
  },
};

/// A pass of `producer` that a session records, presented with a frame profile.
fn presented_pass(producer: &mut WindowProfiler, frame_id: u64) {
  let (started, guard) = producer.begin_pass(frame_id);
  producer.finish_pass(
    started,
    &crate::app::PassReport {
      required: true,
      rendered: true,
      ..Default::default()
    },
    "none",
    Some(&FrameProfile::default()),
  );
  drop(guard);
}

fn capture_of(state: &ProfilerState) -> Arc<ProfilerCapture> {
  state.capture.clone().expect("Stop keeps the recording")
}

#[test]
fn start_and_stop_keep_every_pass_of_the_inspected_windows_and_not_devtools() {
  let mut main = WindowProfiler::new();
  let mut second = WindowProfiler::new();
  second.attach(&main, "w1".into(), false);
  second.set_open(true);
  let mut devtools = WindowProfiler::new();
  devtools.attach(&main, "w2".into(), true);
  devtools.set_open(true);
  let mut state = ProfilerState::default();

  // Before Start: not part of the recording.
  presented_pass(&mut main, 1);
  assert!(state.start(&main.handle()));
  assert!(state.recording());
  let id = state.recording.as_ref().unwrap().id;
  // Passes with the same timings and content stay apart.
  for frame in 2..=6 {
    presented_pass(&mut main, frame);
    presented_pass(&mut second, frame);
    presented_pass(&mut devtools, frame);
  }
  state.stop();
  // After Stop: not part of it either.
  presented_pass(&mut main, 7);

  assert!(!state.recording());
  let capture = capture_of(&state);
  let frames = |window: &str| {
    capture
      .passes
      .iter()
      .filter(|sample| sample.window == window)
      .map(|sample| pass_of(sample).unwrap().frame_id.unwrap())
      .collect::<Vec<_>>()
  };
  assert_eq!(frames("main"), vec![2, 3, 4, 5, 6]);
  assert_eq!(frames("w1"), vec![2, 3, 4, 5, 6]);
  assert!(frames("w2").is_empty(), "DevTools' own window was recorded");
  assert_eq!(capture.passes.len(), 10);
  assert!(capture.size_bytes > 0);
  // Every pass carries its frame profile and its spans.
  for sample in &capture.passes {
    let pass = pass_of(sample).unwrap();
    assert!(pass.frame.is_some());
    assert_eq!(phase_rows(pass)[0].name, "pass");
  }
  // The session is over: the handle has no recording left running.
  assert!(main.handle().status(id).is_err());
}

#[test]
fn include_devtools_records_devtools_windows_too() {
  let main = WindowProfiler::new();
  let mut devtools = WindowProfiler::new();
  devtools.attach(&main, "w1".into(), true);
  devtools.set_open(true);
  let mut state = ProfilerState {
    include_devtools: true,
    ..ProfilerState::default()
  };
  assert!(state.start(&main.handle()));
  presented_pass(&mut devtools, 1);
  state.stop();
  assert_eq!(capture_of(&state).passes.len(), 1);
  // The choice holds for the next recording, not the capture.
  assert!(state.include_devtools);
}

#[test]
fn a_recording_on_a_tree_keeps_each_headless_pass_with_its_phases() {
  use crate::{
    app::{App, Tree},
    components::{Column, Text},
  };
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Text::new("Profiler fixture")));
  let mut state = ProfilerState::default();
  assert!(state.start(&tree.profiling_handle()));
  let status = state.recording.as_ref().and_then(|recording| recording.status).unwrap();
  assert!(status.recording.is_some());
  for _ in 0..4 {
    tree.pass_headless(&mut app);
  }
  state.refresh_status();
  let status = state.recording.as_ref().and_then(|recording| recording.status).unwrap();
  assert_eq!(status.completed_samples, 4);
  assert!(status.recording.unwrap().size_bytes > 0);
  state.stop();

  let capture = capture_of(&state);
  assert_eq!(capture.passes.len(), 4);
  for sample in &capture.passes {
    let rows = phase_rows(pass_of(sample).unwrap());
    assert!(rows.iter().any(|row| row.name == "layout_update" && row.depth == 1));
  }
  // Headless passes present nothing, so there is no frame rate to show.
  assert_eq!(fps(capture.passes_in(capture.whole())), None);
}

/// What the tab draws of a capture: its passes, markers and scopes by sequence
/// and time, and the call tree of its whole range on every track.
fn drawn(capture: &ProfilerCapture) -> impl PartialEq + std::fmt::Debug + use<> {
  use crate::app::profiler::{SpanTrack, forest_of};
  let times = |samples: &[Arc<ProfileSample>]| {
    samples
      .iter()
      .map(|sample| {
        (
          sample.sequence,
          sample.window.clone(),
          sample.started_ms,
          sample.completed_ms,
        )
      })
      .collect::<Vec<_>>()
  };
  let whole = capture.whole();
  let forest = forest_of(capture.passes_in(whole), capture.scopes_in(whole));
  (
    (
      whole,
      capture.default_range(),
      capture.size_bytes,
      capture.boundary_excluded,
    ),
    times(&capture.passes),
    times(&capture.markers),
    capture
      .scopes
      .iter()
      .map(|scope| (scope.scope.label, scope.scope.started_ms, scope.wall))
      .collect::<Vec<_>>(),
    SpanTrack::ALL.map(|track| forest.call_tree(track)),
    capture
      .passes
      .iter()
      .map(|sample| phase_rows(pass_of(sample).unwrap()))
      .collect::<Vec<_>>(),
  )
}

#[test]
fn a_saved_recording_loads_back_into_the_same_timeline_and_call_tree() {
  use crate::{
    app::{App, Tree},
    components::{Column, Text},
  };
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.set_root(Column::new().child(Text::new("Profiler fixture")));
  let mut state = ProfilerState::default();
  assert!(state.start(&tree.profiling_handle()));
  for _ in 0..3 {
    tree.pass_headless(&mut app);
  }
  state.stop();
  let saved = state
    .report
    .as_ref()
    .expect("Stop keeps the report")
    .to_recording_file();

  let mut loaded = ProfilerState::default();
  loaded.load(ProfileReport::from_recording_file(&saved).map_err(|error| error.to_string()));
  assert!(loaded.error.is_none());
  assert_eq!(drawn(&capture_of(&loaded)), drawn(&capture_of(&state)));
  assert_eq!(capture_of(&loaded).passes.len(), 3);
  assert_eq!(loaded.range, state.range);
  assert_eq!(loaded.selected, state.selected);
  // Saved again, the loaded recording is the same file.
  assert_eq!(loaded.report.as_ref().unwrap().to_recording_file(), saved);
  // The tab draws it, with the file actions.
  tab_tree(Signal::new(loaded.clone()), 1440, 860);

  // A file that cannot be read leaves the capture shown and says why.
  loaded.load(Err("Load failed: not a lurq recording".to_owned()));
  assert_eq!(drawn(&capture_of(&loaded)), drawn(&capture_of(&state)));
  assert_eq!(loaded.error.as_deref(), Some("Load failed: not a lurq recording"));
  tab_tree(Signal::new(loaded.clone()), 1440, 860);
  // A file picked while a recording runs is not loaded, and the top bar says so.
  assert!(loaded.start(&tree.profiling_handle()));
  loaded.load(ProfileReport::from_recording_file(&saved).map_err(|error| error.to_string()));
  assert!(loaded.capture.is_none());
  assert!(loaded.recording());
  assert_eq!(loaded.error.as_deref(), Some(LOAD_WHILE_RECORDING));
  let (_app, recording_tab) = tab_tree(Signal::new(loaded.clone()), 1440, 860);
  assert!(has_text(&recording_tab, LOAD_WHILE_RECORDING));
  // Stop shows its own capture, without the message.
  loaded.stop();
  assert!(loaded.error.is_none());
  assert!(loaded.capture.is_some());
}

/// Some element of the tree shows `text`.
fn has_text(tree: &crate::app::Tree, text: &str) -> bool {
  fn find(element: crate::node::ElementRef<'_>, text: &str) -> bool {
    element.text_content() == Some(text) || element.children().iter().any(|child| find(child, text))
  }
  tree.root().is_some_and(|root| find(root, text))
}

#[test]
fn clear_ends_a_running_recording() {
  let main = WindowProfiler::new();
  let handle = main.handle();
  let mut state = ProfilerState::default();
  assert!(state.start(&handle));
  let id = state.recording.as_ref().unwrap().id;
  state.clear();
  assert!(!state.recording());
  assert!(state.capture.is_none());
  assert!(handle.status(id).is_err());
}

fn sample(sequence: u64, window: &str, started_ms: f64, total_ms: f64, rendered: bool) -> Arc<ProfileSample> {
  Arc::new(ProfileSample {
    sequence,
    window: window.into(),
    started_ms,
    completed_ms: started_ms + total_ms,
    data: SampleData::Pass(PassSample {
      frame_id: rendered.then_some(sequence),
      rendered,
      cached_render_list: false,
      total: Duration::from_secs_f64(total_ms / 1000.0),
      layout_update: Duration::ZERO,
      layout_compute: Duration::ZERO,
      component_after_layout: Duration::ZERO,
      layout_recalculated: false,
      canvas_recording: Duration::ZERO,
      canvas_preparation: Duration::ZERO,
      canvas_text: None,
      backend: "none",
      frame: rendered.then(FrameProfile::default),
      spans: Vec::new(),
      dropped_spans: 0,
      reasons: crate::app::PassReasons::default(),
      present: None,
      counters: None,
    }),
  })
}

fn capture(passes: Vec<Arc<ProfileSample>>, ended_ms: f64) -> ProfilerCapture {
  ProfilerCapture {
    started_ms: 0.0,
    ended_ms,
    passes,
    markers: Vec::new(),
    scopes: Vec::new(),
    dropped_scopes: 0,
    size_bytes: 0,
    stopped_at_limit: false,
    boundary_excluded: 0,
  }
}

#[test]
fn fps_comes_from_intervals_between_presented_frames() {
  // 2 ms passes every 20 ms: 50 FPS, where 1000 / total would say 500.
  let passes: Vec<_> = (0..10)
    .map(|index| sample(index, "main", index as f64 * 20.0, 2.0, true))
    .collect();
  let rate = fps(&passes).unwrap();
  assert!((rate - 50.0).abs() < 1e-6, "{rate}");

  // Passes that presented nothing do not count as frames.
  let mut with_unpresented = passes.clone();
  with_unpresented.push(sample(100, "main", 5.0, 1.0, false));
  assert!((fps(&with_unpresented).unwrap() - 50.0).abs() < 1e-6);

  // The window that presented most decides.
  let mut two_windows = passes;
  two_windows.push(sample(200, "w1", 0.0, 1.0, true));
  two_windows.push(sample(201, "w1", 10.0, 1.0, true));
  assert!((fps(&two_windows).unwrap() - 50.0).abs() < 1e-6);
}

#[test]
fn fps_is_blank_when_the_app_is_idle() {
  assert_eq!(fps(&[]), None);
  assert_eq!(fps(&[sample(1, "main", 0.0, 2.0, true)]), None);
  // Frames further apart than an idle gap are not a frame rate.
  let idle = [
    sample(1, "main", 0.0, 2.0, true),
    sample(2, "main", IDLE_GAP_MS * 2.0, 2.0, true),
  ];
  assert_eq!(fps(&idle), None);
  // Idle gaps between bursts are left out of the rate.
  let bursts = [
    sample(1, "main", 0.0, 2.0, true),
    sample(2, "main", 10.0, 2.0, true),
    sample(3, "main", 5000.0, 2.0, true),
    sample(4, "main", 5010.0, 2.0, true),
  ];
  assert!((fps(&bursts).unwrap() - 100.0).abs() < 1e-6);
}

#[test]
fn ranges_select_passes_by_start_and_keep_a_selection_inside() {
  // Pass n starts at 100 * (n - 1) + 30 ms and takes n ms.
  let passes: Vec<_> = (0..10)
    .map(|index| sample(index + 1, "main", index as f64 * 100.0 + 30.0, 1.0 + index as f64, true))
    .collect();
  let capture = capture(passes, 1000.0);
  assert_eq!(capture.default_range(), capture.whole());
  let first_half = TimeRange {
    start_ms: 0.0,
    end_ms: 500.0,
  };
  let sequences = |range| {
    capture
      .passes_in(range)
      .iter()
      .map(|sample| sample.sequence)
      .collect::<Vec<_>>()
  };
  assert_eq!(sequences(first_half), vec![1, 2, 3, 4, 5]);
  assert_eq!(capture.slowest_in(first_half), Some(5));
  assert_eq!(capture.slowest_in(capture.whole()), Some(10));

  let bucket_ms = 1000.0 / OVERVIEW_BUCKETS as f64;
  let bucket_of_pass_5 = (430.0 / bucket_ms) as usize;
  let mut state = ProfilerState {
    capture: Some(Arc::new(capture)),
    selected: Some(10),
    ..ProfilerState::default()
  };
  // A press on pass 5's bucket: the selected pass is outside its range, so
  // the slowest pass inside it is selected.
  state.press_bucket(bucket_of_pass_5, false);
  assert_eq!(state.selected, Some(5));
  // A move within the pressed bucket changes nothing, so renders nothing.
  assert!(!state.drag_changes_range(bucket_of_pass_5));
  assert!(state.drag_changes_range(0));
  // Dragging back to the first bucket widens the range from where it began.
  state.drag_bucket(0);
  let range = state.range.unwrap();
  assert_eq!(range.start_ms, 0.0);
  assert!((range.end_ms - bucket_ms * (bucket_of_pass_5 + 1) as f64).abs() < 1e-9);
  assert_eq!(sequences_of(&state, range), vec![1, 2, 3, 4, 5]);
  // A pass still in the range stays selected.
  assert_eq!(state.selected, Some(5));
  // Releasing ends the drag; a shift-press extends from the anchor.
  state.range_dragging = false;
  state.drag_bucket(OVERVIEW_BUCKETS - 1);
  assert_eq!(state.range, Some(range));
  state.press_bucket(OVERVIEW_BUCKETS - 1, true);
  let range = state.range.unwrap();
  assert!((range.start_ms - bucket_ms * bucket_of_pass_5 as f64).abs() < 1e-9);
  assert_eq!(range.end_ms, 1000.0);
  assert_eq!(sequences_of(&state, range), vec![5, 6, 7, 8, 9, 10]);
  assert_eq!(state.selected, Some(5));
}

fn sequences_of(state: &ProfilerState, range: TimeRange) -> Vec<u64> {
  capture_of(state)
    .passes_in(range)
    .iter()
    .map(|sample| sample.sequence)
    .collect()
}

#[test]
fn a_long_recording_opens_on_its_last_passes() {
  let count = MAX_TIMELINE_PASSES + 10;
  let passes: Vec<_> = (0..count)
    .map(|index| sample(index as u64 + 1, "main", index as f64 * 16.0, 1.0, true))
    .collect();
  let capture = capture(passes, count as f64 * 16.0);
  let range = capture.default_range();
  assert_eq!(capture.passes_in(range).len(), MAX_TIMELINE_PASSES);
  assert_eq!(range.end_ms, capture.ended_ms);
}

#[test]
fn every_stacked_pass_bar_takes_its_own_click() {
  use crate::{
    app::{App, Tree, events::MouseButton},
    components::Column,
  };
  const WIDTH: f32 = 600.0;
  // Three passes at a tenth, half and nine tenths of the range, each drawn in
  // its own full-size layer stacked over the earlier ones.
  let passes: Vec<_> = [100.0, 500.0, 900.0]
    .into_iter()
    .enumerate()
    .map(|(index, start)| sample(index as u64 + 1, "main", start, 30.0, true))
    .collect();
  let range = TimeRange {
    start_ms: 0.0,
    end_ms: 1000.0,
  };
  let state = Signal::new(ProfilerState::default());
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(WIDTH as u32, CHART_HEIGHT as u32);
  tree.set_root(
    Column::new()
      .child(pass_chart(&passes, range, None, state.clone()))
      .size(WIDTH, CHART_HEIGHT),
  );
  tree.pass_headless(&mut app);

  let click = |tree: &mut Tree, x: f32| {
    let y = CHART_HEIGHT - 2.0;
    tree.mouse_down(x, y, MouseButton::Left);
    tree.mouse_up(x, y, MouseButton::Left);
  };
  // Between bars, the layers above let the press through to nothing.
  click(&mut tree, WIDTH * 0.3);
  assert_eq!(state.get_untracked().selected, None);
  let scale = WIDTH - LANE_GAP - LANE_LABEL_WIDTH - BAR_WIDTH;
  for (sequence, fraction) in [(1, 0.1), (2, 0.5), (3, 0.9)] {
    click(&mut tree, scale * fraction + BAR_WIDTH / 2.0);
    assert_eq!(state.get_untracked().selected, Some(sequence));
  }
}

/// The node at `path`, child indices from the root.
fn node_at(tree: &crate::app::Tree, path: &[usize]) -> crate::core::NodeId {
  let mut element = tree.root().expect("a root");
  for &index in path {
    element = element.children().iter().nth(index).expect("a child at the path");
  }
  element.node_id()
}

fn bounds_at(tree: &mut crate::app::Tree, path: &[usize]) -> crate::core::ElementRect {
  let id = node_at(tree, path);
  tree
    .find_element(|element: crate::node::ElementRef<'_>| element.node_id() == id)
    .expect("laid out")
    .bounds()
}

#[test]
fn bars_markers_and_axis_ticks_share_one_time_scale() {
  use crate::{
    app::{App, Tree, profiler::InputDispatchSample},
    components::Column,
  };
  const WIDTH: f32 = 1068.0;
  // A pass and an input dispatch half way through the range, and a pass at
  // its end; the second pass has twice the nodes of the first.
  let passes = vec![
    with_nodes(sample(1, "main", 500.0, 30.0, true), 10),
    with_nodes(sample(2, "main", 1000.0, 30.0, true), 20),
  ];
  let mut capture = capture(passes.clone(), 1000.0);
  capture.markers = vec![Arc::new(ProfileSample {
    sequence: 3,
    window: "main".into(),
    started_ms: 500.0,
    completed_ms: 501.0,
    data: SampleData::InputDispatch(InputDispatchSample {
      frame_id: None,
      kind: crate::app::profiler::InputKind::Pointer,
      total: Duration::from_millis(1),
      interaction_id: 1,
      queue_delay: Duration::ZERO,
      presented_frame_id: None,
      input_to_present: None,
    }),
  })];
  let range = capture.whole();
  let mut app = App::new();
  let mut tree = Tree::new();
  tree.resize(WIDTH as u32, 500);
  tree.set_root(
    Column::new()
      .child(timeline_tracks(
        &capture,
        &passes,
        range,
        None,
        Signal::new(ProfilerState::default()),
      ))
      .size(WIDTH, 500.0),
  );
  tree.pass_headless(&mut app);

  // Rows: chart, missed vsync lane, the counter tracks (fps, nodes, draw
  // calls, memory, allocs), input lane, ui update lane, axis; each a track
  // then its label.
  let half_bar = bounds_at(&mut tree, &[0, 0, 0, 1, 1]);
  let end_bar = bounds_at(&mut tree, &[0, 0, 0, 2, 1]);
  let half_nodes = bounds_at(&mut tree, &[0, 3, 0, 0, 1]);
  let end_nodes = bounds_at(&mut tree, &[0, 3, 0, 1, 1]);
  let half_marker = bounds_at(&mut tree, &[0, 7, 0, 1]);
  let half_tick = bounds_at(&mut tree, &[0, 9, 0, 2]);
  let last_part = bounds_at(&mut tree, &[0, 9, 0, 3]);
  let close = |a: f32, b: f32, what: &str| assert!((a - b).abs() < 0.5, "{what}: {a} vs {b}");
  let scale = WIDTH - LANE_GAP - LANE_LABEL_WIDTH - BAR_WIDTH;
  close(half_bar.x, scale * 0.5, "bar at half the range");
  close(half_nodes.x, half_bar.x, "counter bar at the pass bar's time");
  close(end_nodes.x, end_bar.x, "last counter bar at the last pass bar");
  close(
    half_nodes.height * 2.0,
    end_nodes.height,
    "counter bars scaled by value",
  );
  close(end_nodes.height, COUNTER_HEIGHT, "the highest value fills the track");
  close(half_marker.x, half_bar.x, "marker at the bar's time");
  close(half_tick.x, half_bar.x, "tick at the bar's time");
  close(end_bar.x, scale, "bar at the range's end");
  close(last_part.x + last_part.width, end_bar.x, "axis end at the last bar");
}

fn with_nodes(sample: Arc<ProfileSample>, nodes: usize) -> Arc<ProfileSample> {
  let mut sample = (*sample).clone();
  if let SampleData::Pass(pass) = &mut sample.data {
    pass.counters = Some(PassCounters {
      nodes,
      ..PassCounters::default()
    });
  }
  Arc::new(sample)
}

#[test]
fn counter_tracks_take_values_per_pass_and_mark_missing_allocations_unavailable() {
  // Presented every 20 ms, then after an idle gap.
  let passes = vec![
    with_nodes(sample(1, "main", 0.0, 2.0, true), 5),
    with_nodes(sample(2, "main", 20.0, 2.0, true), 6),
    with_nodes(sample(3, "main", 40.0, 2.0, false), 7),
    with_nodes(sample(4, "main", 40.0 + IDLE_GAP_MS * 2.0, 2.0, true), 8),
  ];
  let values = |counter| {
    capture::counter_points(counter, &passes).map(|points| {
      points
        .iter()
        .map(|point| (point.sequence, point.value))
        .collect::<Vec<_>>()
    })
  };
  // FPS of the interval since the window's previous presented frame.
  assert_eq!(values(Counter::Fps), Some(vec![(2, 50.0)]));
  assert_eq!(
    values(Counter::Nodes),
    Some(vec![(1, 5.0), (2, 6.0), (3, 7.0), (4, 8.0)])
  );
  // Memory comes with the same counters; no pass has allocation counts.
  assert_eq!(values(Counter::Memory).map(|points| points.len()), Some(4));
  assert_eq!(values(Counter::Allocations), None);

  let mut counted = (*passes[0]).clone();
  if let SampleData::Pass(pass) = &mut counted.data {
    pass.counters.as_mut().unwrap().allocations = Some(PassAllocations {
      allocations: 12,
      ..PassAllocations::default()
    });
  }
  assert_eq!(
    capture::counter_points(Counter::Allocations, &[Arc::new(counted)]).map(|points| points[0].value),
    Some(12.0)
  );
  // Passes recorded without counters have none to show.
  assert_eq!(
    capture::counter_points(Counter::Nodes, &[sample(1, "main", 0.0, 2.0, true)]),
    None
  );
  // The sidebar lists a pass's counters, allocations marked unavailable.
  let detail = pass_detail(&passes[0], pass_of(&passes[0]).unwrap());
  let counters = detail.iter().find(|section| section.title == "COUNTERS").unwrap();
  assert!(counters.rows.contains(&("nodes", "5".to_owned())));
  assert!(counters.rows.contains(&("allocations", "unavailable".to_owned())));
}

/// The Profiler tab as DevTools draws it, the analysis mounted under the
/// timeline, on the state in its props.
struct TabHost {
  status_ticker: crate::app::ctx::Interval,
}

#[derive(Clone)]
struct TabHostProps {
  state: Signal<ProfilerState>,
}

impl PartialEq for TabHostProps {
  fn eq(&self, other: &Self) -> bool {
    self.state.id() == other.state.id()
  }
}

impl crate::app::component::DevtoolsInspectable for TabHostProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl crate::app::component::Component for TabHost {
  type Props = TabHostProps;

  fn create(ctx: &mut crate::app::ctx::Ctx) -> Self {
    Self {
      status_ticker: ctx.create_interval(STATUS_INTERVAL, || {}),
    }
  }

  fn render(&self, ctx: &mut crate::app::ctx::Ctx) -> impl Into<Element> {
    let signal = ctx.props::<TabHostProps>().state.clone();
    let state = signal.get();
    let analysis = analysis_props(&state, signal.clone())
      .map(|props| ctx.mount::<super::super::profiler_analysis::ProfilerAnalysis>(props))
      .unwrap_or_default();
    Column::new()
      .child(profiler_actions(
        &state,
        signal.clone(),
        None,
        self.status_ticker.clone(),
      ))
      .child(profiler_view(&state, signal, analysis))
      .width(FILL)
      .height(FILL)
  }
}

/// A headless tree of the tab at `width` by `height` on `state`, laid out.
fn tab_tree(state: Signal<ProfilerState>, width: u32, height: u32) -> (crate::app::App, crate::app::Tree) {
  let mut app = crate::app::App::new();
  let mut tree = crate::app::Tree::new();
  tree.resize(width, height);
  tree.mount_root::<TabHost>(&mut app, TabHostProps { state });
  // The second pass draws the chart at the width the first one measured.
  tree.pass_headless(&mut app);
  tree.pass_headless(&mut app);
  (app, tree)
}

#[test]
fn the_tab_lays_out_in_every_state() {
  let lay_out = |state: ProfilerState| {
    tab_tree(Signal::new(state), 1440, 860);
  };
  lay_out(ProfilerState::default());
  lay_out(ProfilerState {
    error: Some("profiling requires the perf_profile Cargo feature".to_owned()),
    ..ProfilerState::default()
  });
  let main = WindowProfiler::new();
  let mut recording = ProfilerState::default();
  assert!(recording.start(&main.handle()));
  lay_out(recording.clone());
  recording.clear();

  let mut presented = match &sample(2, "main", 400.0, 40.0, true).data {
    SampleData::Pass(pass) => pass.clone(),
    _ => unreachable!(),
  };
  presented.spans = vec![
    PhaseSpan {
      phase: Phase::PassSetup,
      depth: 0,
      start_ns: 400_000_000,
      end_ns: 440_000_000,
    },
    PhaseSpan {
      phase: Phase::LayoutUpdate,
      depth: 1,
      start_ns: 401_000_000,
      end_ns: 420_000_000,
    },
  ];
  let mut with_spans = (*sample(2, "main", 400.0, 40.0, true)).clone();
  with_spans.data = SampleData::Pass(presented);
  let capture = capture(
    vec![
      sample(1, "main", 100.0, 4.0, true),
      Arc::new(with_spans),
      sample(3, "main", 700.0, 20.0, false),
    ],
    1000.0,
  );
  let range = capture.whole();
  lay_out(ProfilerState {
    capture: Some(Arc::new(capture)),
    range: Some(range),
    selected: Some(2),
    ..ProfilerState::default()
  });
}

/// Elements in the tree.
fn element_count(tree: &crate::app::Tree) -> usize {
  fn count(element: crate::node::ElementRef<'_>) -> usize {
    1 + element.children().iter().map(count).sum::<usize>()
  }
  tree.root().map_or(0, count)
}

/// Milliseconds `run` took, each time.
fn timed(times: usize, mut run: impl FnMut(usize)) -> Vec<f64> {
  (0..times)
    .map(|index| {
      let started = std::time::Instant::now();
      run(index);
      started.elapsed().as_secs_f64() * 1000.0
    })
    .collect()
}

fn median_and_max(mut times: Vec<f64>) -> (f64, f64) {
  times.sort_by(f64::total_cmp);
  (times[times.len() / 2], times[times.len() - 1])
}

/// The tab on a 2000-frame recording with 64k spans, the analysis over all
/// of it: each hover, wheel zoom, pan step and table switch is one input and
/// one headless pass of the whole tab (layout and component work; no GPU).
/// Prints the times; `cargo test --release` for representative numbers.
#[test]
fn a_recording_of_2000_frames_stays_interactive() {
  use super::super::profiler_analysis::{
    CHART_ID, ROW_HEIGHT, TOOLTIP_ID, TRACK_HEADER_HEIGHT, tab_id, tests::synthetic_capture,
  };
  use crate::app::events::{MouseButton, ScrollPhase};
  let capture = Arc::new(synthetic_capture(2000, 20));
  let whole = capture.whole();
  let signal = Signal::new(ProfilerState {
    selected: capture.slowest_in(whole),
    range: Some(whole),
    capture: Some(capture.clone()),
    ..ProfilerState::default()
  });
  let started = std::time::Instant::now();
  let (mut app, mut tree) = tab_tree(signal.clone(), 1440, 860);
  let first_ms = started.elapsed().as_secs_f64() * 1000.0;
  let bounds = |tree: &mut crate::app::Tree, id: &str| {
    tree
      .find_element(|element: crate::node::ElementRef<'_>| element.id() == Some(id))
      .map(|element| element.bounds())
  };
  let chart = bounds(&mut tree, CHART_ID).expect("the flame chart");
  assert!(chart.width > 600.0, "{chart:?}");
  let elements = element_count(&tree);
  let chart_blocks = tree
    .get_element_by_id(CHART_ID)
    .map(|chart| chart.children().len())
    .unwrap();
  assert!(chart_blocks < 2000, "{chart_blocks} chart elements");

  let y = chart.y + TRACK_HEADER_HEIGHT + ROW_HEIGHT * 1.5;
  let hover = timed(120, |step| {
    tree.mouse_move(chart.x + chart.width * (step as f32 + 0.5) / 120.0, y);
    tree.pass_headless(&mut app);
  });
  assert!(bounds(&mut tree, TOOLTIP_ID).is_some(), "hovering shows the tooltip");
  let x = chart.x + chart.width * 0.5;
  let zoom = timed(15, |_| {
    tree.scroll(x, y, 0.0, 120.0, ScrollPhase::Scroll);
    tree.pass_headless(&mut app);
  });
  tree.mouse_down(x, y, MouseButton::Left);
  let pan = timed(20, |step| {
    tree.mouse_move(x - 10.0 * (step as f32 + 1.0), y);
    tree.pass_headless(&mut app);
  });
  tree.mouse_up(x - 200.0, y, MouseButton::Left);
  let mut switch = |label: &str| {
    let chip = bounds(&mut tree, &tab_id(label)).unwrap();
    let started = std::time::Instant::now();
    tree.mouse_down(chip.x + 4.0, chip.y + 4.0, MouseButton::Left);
    tree.mouse_up(chip.x + 4.0, chip.y + 4.0, MouseButton::Left);
    tree.pass_headless(&mut app);
    started.elapsed().as_secs_f64() * 1000.0
  };
  let call_tree_ms = switch("Call tree");
  let bottom_up_ms = switch("Bottom-up");
  let back_ms = switch("Call tree");
  assert!(
    tree
      .find_element(|element: crate::node::ElementRef<'_>| element.text_content() == Some("pass"))
      .is_some()
  );

  let (hover_median, hover_max) = median_and_max(hover);
  let (zoom_median, zoom_max) = median_and_max(zoom);
  let (pan_median, pan_max) = median_and_max(pan);
  eprintln!(
    "2000 frames, {} spans, {elements} elements ({chart_blocks} in the chart): first passes {first_ms:.1} ms; \
     hover median {hover_median:.2} ms max {hover_max:.2}; zoom median {zoom_median:.2} max {zoom_max:.2}; pan \
     median {pan_median:.2} max {pan_max:.2}; call tree {call_tree_ms:.1} ms, bottom-up {bottom_up_ms:.1} ms, call \
     tree again {back_ms:.1} ms",
    capture
      .passes
      .iter()
      .map(|sample| pass_of(sample).unwrap().spans.len())
      .sum::<usize>(),
  );
}

#[test]
fn phase_rows_place_spans_on_the_pass_time_axis() {
  let mut pass = match &sample(1, "main", 0.0, 4.0, true).data {
    SampleData::Pass(pass) => pass.clone(),
    _ => unreachable!(),
  };
  let span = |phase, depth, start_ns, end_ns| PhaseSpan {
    phase,
    depth,
    start_ns,
    end_ns,
  };
  pass.spans = vec![
    span(Phase::PassSetup, 0, 10_000_000, 14_000_000),
    span(Phase::LayoutUpdate, 1, 10_500_000, 12_000_000),
    span(Phase::LayoutCompute, 2, 11_000_000, 11_500_000),
  ];
  let rows = phase_rows(&pass);
  assert_eq!(
    rows.iter().map(|row| (row.name, row.depth)).collect::<Vec<_>>(),
    vec![("pass", 0), ("layout_update", 1), ("layout_compute", 2)]
  );
  assert_eq!(rows[0].offset_ms, 0.0);
  assert!((rows[0].duration_ms - 4.0).abs() < 1e-9);
  assert!((rows[1].offset_ms - 0.5).abs() < 1e-9);
  assert!((rows[2].duration_ms - 0.5).abs() < 1e-9);
}

#[test]
fn pass_detail_shows_the_whole_frame_profile() {
  let presented = sample(1, "main", 0.0, 4.0, true);
  let titles = |sample: &ProfileSample| {
    pass_detail(sample, pass_of(sample).unwrap())
      .into_iter()
      .map(|section| section.title)
      .collect::<Vec<_>>()
  };
  assert_eq!(
    titles(&presented),
    vec!["PASS", "FRAME", "COUNTS", "RENDER", "CANVAS", "TEXT", "MEMORY"]
  );
  assert_eq!(titles(&sample(2, "main", 0.0, 4.0, false)), vec!["PASS"]);
  let detail = pass_detail(&presented, pass_of(&presented).unwrap());
  let row = |title: &str, label: &str| {
    detail
      .iter()
      .find(|section| section.title == title)
      .and_then(|section| section.rows.iter().find(|(name, _)| *name == label))
      .map(|(_, value)| value.clone())
  };
  assert_eq!(row("PASS", "total").as_deref(), Some("4.00ms"));
  assert_eq!(row("COUNTS", "quad_count").as_deref(), Some("0"));
  assert_eq!(row("TEXT", "shape_text").as_deref(), Some("0.00ms"));
  assert_eq!(row("MEMORY", "total_bytes").as_deref(), Some("0"));
}

/// `sample` presented with `missed_vsyncs`, run by a redraw request.
fn with_present(sample: Arc<ProfileSample>, missed_vsyncs: u32) -> Arc<ProfileSample> {
  let mut sample = (*sample).clone();
  if let SampleData::Pass(pass) = &mut sample.data {
    pass.reasons.redraw_requested = true;
    pass.present = Some(crate::app::profiler::PresentTiming {
      presented_ms: sample.completed_ms,
      interval: Some(Duration::from_millis(16 * (u64::from(missed_vsyncs) + 1))),
      refresh_interval: Some(Duration::from_millis(16)),
      continuous: true,
      missed_vsyncs,
    });
  }
  Arc::new(sample)
}

/// An input received at `started_ms`, presented by `frame` `latency_ms` later.
fn input_marker(sequence: u64, started_ms: f64, frame: Option<(u64, f64)>) -> Arc<ProfileSample> {
  Arc::new(ProfileSample {
    sequence,
    window: "main".into(),
    started_ms,
    completed_ms: started_ms + 1.0,
    data: SampleData::InputDispatch(crate::app::profiler::InputDispatchSample {
      frame_id: None,
      kind: crate::app::profiler::InputKind::Pointer,
      total: Duration::from_micros(500),
      interaction_id: sequence,
      queue_delay: Duration::from_micros(250),
      presented_frame_id: frame.map(|(frame_id, _)| frame_id),
      input_to_present: frame.map(|(_, latency_ms)| Duration::from_secs_f64(latency_ms / 1000.0)),
    }),
  })
}

#[test]
fn missed_frames_and_input_latencies_fill_their_lanes_and_the_frame_details() {
  // Frame 2 missed two vsyncs; frame 1 made its refresh.
  let passes = vec![
    with_present(sample(1, "main", 100.0, 4.0, true), 0),
    with_present(sample(2, "main", 600.0, 40.0, true), 2),
  ];
  let mut capture = capture(passes, 1000.0);
  // Two inputs frame 2 presented, and one no frame presented.
  capture.markers = vec![
    input_marker(10, 550.0, Some((2, 90.0))),
    input_marker(11, 560.0, Some((2, 80.0))),
    input_marker(12, 900.0, None),
  ];
  let range = capture.whole();

  let (missed, count) = capture.missed_vsync_buckets(range, 10);
  assert_eq!(count, 1);
  let marked: Vec<_> = missed
    .iter()
    .enumerate()
    .filter_map(|(bucket, missed)| missed.then_some(bucket))
    .collect();
  assert_eq!(marked, vec![6], "only the bucket where frame 2 starts");
  let inputs = capture.input_buckets(range, 10);
  assert_eq!(inputs[5].inputs, 2);
  assert!((inputs[5].max_latency_ms.unwrap() - 90.0).abs() < 1e-6);
  assert_eq!(inputs[9].inputs, 1);
  assert_eq!(inputs[9].max_latency_ms, None);
  assert_eq!(inputs.iter().map(|bucket| bucket.inputs).sum::<usize>(), 3);

  let presented = capture.inputs_presented_by("main", 2);
  assert_eq!(
    presented.iter().map(|input| input.interaction_id).collect::<Vec<_>>(),
    vec![10, 11]
  );
  assert!(capture.inputs_presented_by("main", 1).is_empty());
  let detail = capture::input_detail(&presented).unwrap();
  assert_eq!(detail.title, "INPUT");
  assert_eq!(
    detail.rows[0],
    (
      "pointer",
      "#10 queue 0.25ms · dispatch 0.50ms · present 90.00ms".to_owned()
    )
  );
  assert!(capture::input_detail(&[]).is_none());

  let late = &capture.passes[1];
  let sections = pass_detail(late, pass_of(late).unwrap());
  assert_eq!(sections[1].title, "PRESENT");
  let row = |title: &str, label: &str| {
    sections
      .iter()
      .find(|section| section.title == title)
      .and_then(|section| section.rows.iter().find(|(name, _)| *name == label))
      .map(|(_, value)| value.clone())
  };
  assert_eq!(row("PASS", "reasons").as_deref(), Some("redraw_requested"));
  assert_eq!(row("PRESENT", "missed_vsyncs").as_deref(), Some("2"));
  assert_eq!(row("PRESENT", "continuous").as_deref(), Some("true"));
  assert_eq!(row("PRESENT", "interval").as_deref(), Some("48.00ms"));

  // The tab lays out with the late frame selected.
  let state = ProfilerState {
    capture: Some(Arc::new(capture)),
    range: Some(range),
    selected: Some(2),
    ..ProfilerState::default()
  };
  let mut app = crate::app::App::new();
  let mut tree = crate::app::Tree::new();
  tree.resize(1440, 860);
  tree.set_root(
    Column::new()
      .child(profiler_view(&state, Signal::new(state.clone()), Element::default()))
      .size(1440.0, 860.0),
  );
  tree.pass_headless(&mut app);
}
