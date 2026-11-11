use std::{collections::BTreeMap, sync::Arc, time::Duration};

use serde_json::Value;

use super::*;

const MS: u64 = 1_000_000;

fn span(phase: Phase, depth: u8, start_ms: u64, end_ms: u64) -> PhaseSpan {
  PhaseSpan {
    phase,
    depth,
    start_ns: start_ms * MS,
    end_ns: end_ms * MS,
    component: None,
  }
}

/// pass at..at+10: layout_update +1..+7 holds ui_rebuild +1..+3 (holding
/// ui_commit +2..+3) and layout_compute +4..+6; render +7..+9. Presented with
/// `memory_bytes` when given.
fn pass(sequence: u64, window: &str, at_ms: u64, memory_bytes: Option<usize>) -> Arc<ProfileSample> {
  Arc::new(ProfileSample {
    sequence,
    window: window.into(),
    started_ms: at_ms as f64,
    completed_ms: (at_ms + 10) as f64,
    data: SampleData::Pass(PassSample {
      frame_id: memory_bytes.map(|_| sequence),
      rendered: memory_bytes.is_some(),
      cached_render_list: false,
      total: Duration::from_millis(10),
      layout_update: Duration::from_millis(6),
      layout_compute: Duration::from_millis(2),
      component_after_layout: Duration::ZERO,
      layout_recalculated: true,
      canvas_recording: Duration::ZERO,
      canvas_preparation: Duration::ZERO,
      canvas_text: None,
      backend: "none",
      frame: memory_bytes.map(|total_bytes| FrameProfile {
        memory: RuntimeMemoryProfile {
          total_bytes,
          ..RuntimeMemoryProfile::default()
        },
        ..FrameProfile::default()
      }),
      spans: vec![
        span(Phase::PassSetup, 0, at_ms, at_ms + 10),
        span(Phase::LayoutUpdate, 1, at_ms + 1, at_ms + 7),
        span(Phase::Rebuild, 2, at_ms + 1, at_ms + 3),
        span(Phase::Commit, 3, at_ms + 2, at_ms + 3),
        span(Phase::LayoutCompute, 2, at_ms + 4, at_ms + 6),
        span(Phase::Render, 1, at_ms + 7, at_ms + 9),
      ],
      dropped_spans: 0,
      reasons: crate::app::PassReasons::default(),
      present: None,
      counters: None,
      gpu: None,
      component_layouts: Vec::new(),
      screenshot: false,
    }),
  })
}

fn marker_sample(sequence: u64, at_ms: f64, data: SampleData) -> Arc<ProfileSample> {
  Arc::new(ProfileSample {
    sequence,
    window: "main".into(),
    started_ms: at_ms,
    completed_ms: at_ms + 1.0,
    data,
  })
}

fn scope(
  id: u64,
  parent: Option<u64>,
  label: &'static str,
  lane: ApplicationLane,
  start_ms: u64,
  wall_ms: u64,
) -> Arc<ApplicationScopeSample> {
  Arc::new(ApplicationScopeSample {
    scope: ApplicationScopeInfo {
      id: ApplicationScopeId(id),
      parent_id: parent.map(ApplicationScopeId),
      depth: u32::from(parent.is_some()),
      window: "main".into(),
      label,
      lane,
      started_ms: start_ms as f64,
    },
    completed_ms: (start_ms + wall_ms) as f64,
    wall: Duration::from_millis(wall_ms),
  })
}

fn report(samples: Vec<Arc<ProfileSample>>, scopes: Vec<Arc<ApplicationScopeSample>>) -> ProfileReport {
  ProfileReport {
    id: SessionId(1),
    finalized: true,
    started_ms: 50.0,
    ended_ms: Some(400.0),
    observed_ms: 400.0,
    build: BuildAvailability::default(),
    max_samples: 0,
    completed_samples: samples.len() as u64,
    dropped_samples: 0,
    boundary_excluded_samples: 0,
    untracked_windows: 0,
    windows: Vec::new(),
    in_flight: Vec::new(),
    samples,
    application_scopes: Some(ApplicationScopeReport {
      available: true,
      max_live_scopes: MAX_APPLICATION_LIVE_SCOPES,
      max_samples: MAX_APPLICATION_SCOPES_PER_SESSION,
      started_scopes: scopes.len() as u64,
      completed_scopes: scopes.len() as u64,
      dropped_scopes: 0,
      boundary_excluded_scopes: 0,
      abandoned_window_closed: 0,
      abandoned_producer_closed: 0,
      refused: ApplicationRefusals::default(),
      in_flight: Vec::new(),
      samples: scopes,
    }),
    recording: None,
    screenshots: Vec::new(),
  }
}

/// Two presented passes, one that presented nothing, an input dispatch and a
/// UI update; a UI-lane scope with a child and two overlapping worker scopes.
pub(super) fn full_report() -> ProfileReport {
  report(
    vec![
      pass(1, "main", 100, Some(4096)),
      marker_sample(
        2,
        120.0,
        SampleData::InputDispatch(InputDispatchSample {
          frame_id: None,
          kind: InputKind::Pointer,
          total: Duration::from_millis(1),
          interaction_id: 1,
          queue_delay: Duration::from_micros(500),
          presented_frame_id: Some(4),
          input_to_present: Some(Duration::from_millis(90)),
        }),
      ),
      marker_sample(
        3,
        125.0,
        SampleData::UiUpdate(UiUpdateSample {
          frame_id: None,
          kind: UiUpdateKind::SubtreeRefresh,
          total: Duration::from_millis(1),
          commit: Duration::from_micros(250),
          causes: Vec::new(),
        }),
      ),
      pass(4, "main", 200, Some(6144)),
      pass(5, "main", 300, None),
    ],
    vec![
      scope(1, None, "load_document", ApplicationLane::Ui, 130, 20),
      scope(2, Some(1), "parse", ApplicationLane::Ui, 135, 5),
      scope(3, None, "decode_image", ApplicationLane::Worker, 140, 30),
      scope(4, None, "decode_image", ApplicationLane::Worker, 150, 30),
    ],
  )
}

fn profile(report: &ProfileReport) -> Value {
  serde_json::from_str(&report.to_firefox_profile()).expect("the converter writes JSON")
}

fn thread<'a>(profile: &'a Value, name: &str) -> &'a Value {
  profile["threads"]
    .as_array()
    .unwrap()
    .iter()
    .find(|thread| thread["name"] == name)
    .unwrap_or_else(|| panic!("no thread {name}"))
}

fn strings(thread: &Value) -> Vec<&str> {
  thread["stringArray"]
    .as_array()
    .unwrap()
    .iter()
    .map(|string| string.as_str().unwrap())
    .collect()
}

/// Per stack path, root first, the weight of the samples whose innermost
/// frame it is: what the profiler's call tree shows as self time.
fn self_ms_by_path(thread: &Value) -> BTreeMap<Vec<String>, f64> {
  let strings = strings(thread);
  let column = |table: &str, key: &str| thread[table][key].as_array().unwrap().clone();
  let (prefixes, frames) = (column("stackTable", "prefix"), column("stackTable", "frame"));
  let (frame_funcs, func_names) = (column("frameTable", "func"), column("funcTable", "name"));
  let samples = &thread["samples"];
  let mut by_path = BTreeMap::new();
  for (stack, weight) in samples["stack"]
    .as_array()
    .unwrap()
    .iter()
    .zip(samples["weight"].as_array().unwrap())
  {
    let Some(mut stack) = stack.as_u64() else {
      continue;
    };
    let mut path = Vec::new();
    loop {
      let func = frame_funcs[frames[stack as usize].as_u64().unwrap() as usize]
        .as_u64()
        .unwrap();
      path.push(strings[func_names[func as usize].as_u64().unwrap() as usize].to_owned());
      match prefixes[stack as usize].as_u64() {
        Some(prefix) => stack = prefix,
        None => break,
      }
    }
    path.reverse();
    *by_path.entry(path).or_insert(0.0) += weight.as_f64().unwrap();
  }
  by_path
}

fn path(names: &[&str]) -> Vec<String> {
  names.iter().map(|name| (*name).to_owned()).collect()
}

fn close(a: f64, b: f64) -> bool {
  (a - b).abs() < 1e-9
}

#[test]
fn the_profile_is_a_processed_profile_with_a_thread_per_lane() {
  let profile = profile(&full_report());
  let meta = &profile["meta"];
  assert_eq!(meta["preprocessedProfileVersion"], FIREFOX_PROCESSED_PROFILE_VERSION);
  assert_eq!(meta["product"], "lurq");
  assert_eq!(
    meta["categories"][0]["color"], "grey",
    "the default category comes first"
  );
  for key in ["libs", "pages", "profilerOverhead"] {
    assert_eq!(profile[key], Value::Array(Vec::new()), "{key}");
  }
  let names: Vec<_> = profile["threads"]
    .as_array()
    .unwrap()
    .iter()
    .map(|thread| thread["name"].as_str().unwrap())
    .collect();
  // The overlapping worker scopes go to a second worker thread.
  assert_eq!(names, vec!["UI", "UI scopes", "Worker", "Worker 2"]);
  for (index, thread) in profile["threads"].as_array().unwrap().iter().enumerate() {
    assert_eq!(thread["isMainThread"], index == 0);
    assert_eq!(thread["processStartupTime"], 50.0);
    assert_eq!(thread["processShutdownTime"], 400.0);
    // Every column of a table is as long as the table says.
    for table in [
      "frameTable",
      "funcTable",
      "stackTable",
      "samples",
      "markers",
      "nativeSymbols",
      "resourceTable",
    ] {
      let table_value = &thread[table];
      let length = table_value["length"].as_u64().unwrap();
      for (key, column) in table_value.as_object().unwrap() {
        if let Some(column) = column.as_array() {
          assert_eq!(column.len() as u64, length, "{} {table}.{key}", names[index]);
        }
      }
    }
    assert_eq!(thread["samples"]["weightType"], "tracing-ms");
    // Indices stay inside the tables they point into.
    let strings = strings(thread).len() as u64;
    let len = |table: &str| thread[table]["length"].as_u64().unwrap();
    let all_below = |table: &str, key: &str, bound: u64| {
      thread[table][key]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_u64)
        .all(|index| index < bound)
    };
    assert!(all_below("funcTable", "name", strings));
    assert!(all_below("frameTable", "func", len("funcTable")));
    assert!(all_below("frameTable", "category", 4));
    assert!(all_below("stackTable", "frame", len("frameTable")));
    assert!(all_below("stackTable", "prefix", len("stackTable")));
    assert!(all_below("samples", "stack", len("stackTable")));
    assert!(all_below("markers", "name", strings));
  }
}

#[test]
fn sample_weights_make_the_call_tree_of_the_spans() {
  let profile = profile(&full_report());
  let ui = self_ms_by_path(thread(&profile, "UI"));
  // Self time of each of the three passes.
  let expected = [
    (&["pass"][..], 2.0),
    (&["pass", "layout_update"], 2.0),
    (&["pass", "layout_update", "ui_rebuild"], 1.0),
    (&["pass", "layout_update", "ui_rebuild", "ui_commit"], 1.0),
    (&["pass", "layout_update", "layout_compute"], 2.0),
    (&["pass", "render"], 2.0),
  ];
  for (names, per_pass_ms) in expected {
    let self_ms = ui[&path(names)];
    assert!(close(self_ms, per_pass_ms * 3.0), "{names:?}: {self_ms}");
  }
  assert_eq!(ui.len(), expected.len());
  let total: f64 = ui.values().sum();
  assert!(close(total, 30.0), "the passes' time once: {total}");

  let scopes = self_ms_by_path(thread(&profile, "UI scopes"));
  assert!(close(scopes[&path(&["load_document"])], 15.0));
  assert!(close(scopes[&path(&["load_document", "parse"])], 5.0));
  for worker in ["Worker", "Worker 2"] {
    let lane = self_ms_by_path(thread(&profile, worker));
    assert!(close(lane[&path(&["decode_image"])], 30.0), "{worker}");
  }
}

#[test]
fn samples_follow_the_spans_on_the_time_axis() {
  let profile = profile(&full_report());
  let ui = thread(&profile, "UI");
  let samples = &ui["samples"];
  let mut at_ms = 0.0;
  let times: Vec<f64> = samples["timeDeltas"]
    .as_array()
    .unwrap()
    .iter()
    .map(|delta| {
      at_ms += delta.as_f64().unwrap();
      at_ms
    })
    .collect();
  let stacks = samples["stack"].as_array().unwrap();
  // The first pass: its own time from 100, then layout_update, ...
  assert!(close(times[0], 100.0));
  assert!(times.windows(2).all(|pair| pair[1] > pair[0]), "{times:?}");
  // Each pass ends in a sample of no stack until the next one starts.
  let idle: Vec<f64> = times
    .iter()
    .zip(stacks)
    .filter(|(_, stack)| stack.is_null())
    .map(|(at, _)| *at)
    .collect();
  assert_eq!(idle.len(), 3);
  for (at, end) in idle.iter().zip([110.0, 210.0, 310.0]) {
    assert!(close(*at, end), "{idle:?}");
  }
}

#[test]
fn frames_input_and_ui_updates_are_markers_on_the_ui_thread() {
  let profile = profile(&full_report());
  let ui = thread(&profile, "UI");
  let strings = strings(ui);
  let markers = &ui["markers"];
  let names: Vec<&str> = markers["name"]
    .as_array()
    .unwrap()
    .iter()
    .map(|name| strings[name.as_u64().unwrap() as usize])
    .collect();
  assert_eq!(names, vec!["Frame", "Input", "UI update", "Frame", "Pass"]);
  let types: Vec<&str> = markers["data"]
    .as_array()
    .unwrap()
    .iter()
    .map(|data| data["type"].as_str().unwrap())
    .collect();
  assert_eq!(
    types,
    vec!["LurqPass", "LurqInput", "LurqUiUpdate", "LurqPass", "LurqPass"]
  );
  let schema_names: Vec<&str> = profile["meta"]["markerSchema"]
    .as_array()
    .unwrap()
    .iter()
    .map(|schema| schema["name"].as_str().unwrap())
    .collect();
  for marker_type in &types {
    assert!(schema_names.contains(marker_type), "{marker_type} has no schema");
  }
  assert_eq!(markers["phase"][0], 1);
  assert_eq!(markers["startTime"][0], 100.0);
  assert_eq!(markers["endTime"][0], 110.0);
  let input = &markers["data"][1];
  assert_eq!(strings[input["kind"].as_u64().unwrap() as usize], "pointer");
  assert_eq!(strings[input["window"].as_u64().unwrap() as usize], "main");
  assert_eq!(input["interactionId"], 1);
  assert!(close(input["queueDelay"].as_f64().unwrap(), 0.5));
  assert!(close(input["dispatch"].as_f64().unwrap(), 1.0));
  assert_eq!(input["presentedFrameId"], 4);
  assert!(close(input["inputToPresent"].as_f64().unwrap(), 90.0));
  assert_eq!(markers["data"][0]["frameId"], 1);
  assert!(
    markers["data"][4].get("frameId").is_none(),
    "a pass that presented nothing has no frame id"
  );
  assert!(close(markers["data"][2]["commit"].as_f64().unwrap(), 0.25));
  // An update without causes (not a detailed recording) has no causes field.
  assert!(markers["data"][2].get("causes").is_none());
  // Only the UI thread holds markers.
  assert_eq!(thread(&profile, "Worker")["markers"]["length"], 0);
}

#[test]
fn a_detailed_recording_names_components_in_the_call_tree_and_causes_on_its_updates() {
  let mut report = full_report();
  let SampleData::Pass(pass) = &mut Arc::make_mut(&mut report.samples[0]).data else {
    unreachable!("the first sample is a pass")
  };
  // A component render inside ui_rebuild +1..+3, before its commit.
  let at_ms = pass.spans[0].start_ns / MS;
  pass.spans.insert(
    3,
    PhaseSpan {
      component: Some("Counter"),
      ..span(Phase::Rebuild, 3, at_ms + 1, at_ms + 2)
    },
  );
  let SampleData::UiUpdate(update) = &mut Arc::make_mut(&mut report.samples[2]).data else {
    unreachable!("the third sample is a UI update")
  };
  update.causes = vec![
    RenderCause {
      component: "Counter",
      source: RenderSource {
        trigger: RenderTrigger::Signal,
        value_type: Some("u32"),
      },
      via: None,
    },
    RenderCause {
      component: "Total",
      source: RenderSource {
        trigger: RenderTrigger::Memo,
        value_type: Some("u64"),
      },
      via: Some(RenderSource {
        trigger: RenderTrigger::Signal,
        value_type: Some("u32"),
      }),
    },
  ];
  let profile = profile(&report);
  let ui = thread(&profile, "UI");
  assert!(
    strings(ui).contains(&"Counter"),
    "the component is a function of the call tree"
  );
  assert_eq!(
    ui["markers"]["data"][2]["causes"],
    "Counter: signal u32; Total: memo u64 via signal u32"
  );
}

fn counts(counter: &Value) -> Vec<f64> {
  counter["samples"]["count"]
    .as_array()
    .unwrap()
    .iter()
    .map(|count| count.as_f64().unwrap())
    .collect()
}

#[test]
fn the_memory_counter_adds_up_to_each_presented_estimate() {
  let profile = profile(&full_report());
  let counters = profile["counters"].as_array().unwrap();
  assert_eq!(counters.len(), 1);
  let counter = &counters[0];
  assert_eq!(counter["category"], "Memory");
  assert_eq!(counter["name"], "Runtime memory");
  assert_eq!(counter["mainThreadIndex"], 0);
  let samples = &counter["samples"];
  assert_eq!(samples["length"], 2);
  assert_eq!(counts(counter), vec![4096.0, 2048.0]);
  assert_eq!(samples["timeDeltas"][0], 110.0);
  assert_eq!(samples["timeDeltas"][1], 100.0);

  // Without a frame profile or pass counters there is nothing to count.
  let unpresented = profile_of_samples(vec![pass(1, "main", 100, None)]);
  assert_eq!(unpresented["counters"], Value::Array(Vec::new()));
}

/// `pass` with counters taken at its end, and allocations when `live_bytes`.
fn counted_pass(
  sequence: u64,
  at_ms: u64,
  frame_memory: Option<usize>,
  memory_bytes: usize,
  live_bytes: Option<u64>,
) -> Arc<ProfileSample> {
  let mut sample = Arc::unwrap_or_clone(pass(sequence, "main", at_ms, frame_memory));
  let SampleData::Pass(pass) = &mut sample.data else {
    unreachable!()
  };
  pass.counters = Some(PassCounters {
    nodes: 12,
    memory_bytes,
    caches: CacheSizes::default(),
    allocations: live_bytes.map(|live_bytes| PassAllocations {
      live_bytes,
      ..PassAllocations::default()
    }),
  });
  Arc::new(sample)
}

#[test]
fn pass_counters_feed_the_memory_and_live_heap_counters() {
  let profile = profile_of_samples(vec![
    // The pass's own estimate wins over the frame's cached one.
    counted_pass(1, 100, Some(1), 1000, Some(5000)),
    // Counted without a presented frame, and without an allocator.
    counted_pass(2, 200, None, 1500, None),
    counted_pass(3, 300, None, 1200, Some(4000)),
  ]);
  let counters = profile["counters"].as_array().unwrap();
  let names: Vec<_> = counters
    .iter()
    .map(|counter| counter["name"].as_str().unwrap())
    .collect();
  assert_eq!(names, vec!["Runtime memory", "Live heap"]);
  assert_eq!(counts(&counters[0]), vec![1000.0, 500.0, -300.0]);
  assert_eq!(counts(&counters[1]), vec![5000.0, -1000.0]);
  assert_eq!(counters[1]["samples"]["timeDeltas"][1], 200.0);
}

fn profile_of_samples(samples: Vec<Arc<ProfileSample>>) -> Value {
  profile(&report(samples, Vec::new()))
}

#[test]
fn an_empty_recording_is_one_empty_ui_thread() {
  let mut empty = report(Vec::new(), Vec::new());
  empty.application_scopes = None;
  let profile = profile(&empty);
  let threads = profile["threads"].as_array().unwrap();
  assert_eq!(threads.len(), 1);
  assert_eq!(threads[0]["name"], "UI");
  assert_eq!(threads[0]["samples"]["length"], 0);
  assert_eq!(threads[0]["markers"]["length"], 0);
}

#[test]
fn passes_of_two_windows_that_overlap_get_a_ui_thread_each() {
  let profile = profile_of_samples(vec![pass(1, "main", 100, None), pass(2, "w1", 105, None)]);
  let names: Vec<_> = profile["threads"]
    .as_array()
    .unwrap()
    .iter()
    .map(|thread| thread["name"].as_str().unwrap().to_owned())
    .collect();
  assert_eq!(names, vec!["UI", "UI 2"]);
  for name in ["UI", "UI 2"] {
    let total: f64 = self_ms_by_path(thread(&profile, name)).values().sum();
    assert!(close(total, 10.0), "{name}: {total}");
  }
}

#[test]
fn the_profile_holds_only_names_of_the_toolkit_and_the_recording() {
  let profile = profile(&full_report());
  let mut all = Vec::new();
  for thread in profile["threads"].as_array().unwrap() {
    all.extend(strings(thread).into_iter().map(str::to_owned));
  }
  let phases: Vec<&str> = phase_slot::PHASES.iter().map(|phase| phase.name()).collect();
  for string in all {
    let known = phases.contains(&string.as_str())
      || [
        "pass",
        "main",
        "pointer",
        "subtree_refresh",
        "Frame",
        "Pass",
        "Input",
        "UI update",
        "load_document",
        "parse",
        "decode_image",
      ]
      .contains(&string.as_str());
    assert!(known, "unexpected string {string:?}");
  }
}
