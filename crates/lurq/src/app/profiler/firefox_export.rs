//! A recording in the Firefox Profiler's processed profile format, which
//! profiler.firefox.com opens with its call tree, flame graph, stack chart and
//! marker chart. Written to preprocessed profile version 55, field for field
//! as `fxprof-processed-profile` 0.8.1 serializes it. Plain data, without a
//! window or a file dialog; the lurq-native file is `recording_file`.
//!
//! The profile is a tracing profile (`weightType` "tracing-ms"): a sample is
//! taken at each point where the innermost span changes, weighted by the time
//! until the next one, so the call tree's totals are the spans' durations.
//! Threads: "UI" holds the passes' phase spans, the frame, input and UI update
//! markers and the memory counters; "UI scopes" and "Worker" hold the
//! application scopes of those lanes. Spans that overlap on one track (passes
//! of two windows, scopes of two workers) go to a further thread of it.
use std::collections::HashMap;

use serde_json::{Value, json};

use super::{
  ApplicationLane, PassSample, ProfileReport, SampleData, UiUpdateKind,
  aggregate::{SpanForest, SpanTrack, forest_of, ms_to_ns},
};

/// The `meta.preprocessedProfileVersion` this converter writes.
pub const FIREFOX_PROCESSED_PROFILE_VERSION: u64 = 55;

const PRODUCT: &str = "lurq";
const PID: &str = "0";

/// Indices into `meta.categories`; the first, grey one is the default.
const CATEGORY_OTHER: usize = 0;
const CATEGORY_FRAME: usize = 1;
const CATEGORY_APPLICATION: usize = 2;
const CATEGORY_INPUT: usize = 3;

impl ProfileReport {
  /// The report as a Firefox Profiler processed profile (JSON), to open at
  /// profiler.firefox.com. One way: DevTools loads the lurq-native file of
  /// [`ProfileReport::to_recording_file`] instead.
  pub fn to_firefox_profile(&self) -> String {
    serde_json::to_string(&firefox_profile(self)).expect("a JSON value serializes")
  }
}

pub(super) fn firefox_profile(report: &ProfileReport) -> Value {
  let scopes = report
    .application_scopes
    .as_ref()
    .map_or(&[][..], |scopes| scopes.samples.as_slice());
  let forest = forest_of(&report.samples, scopes);
  let mut children = vec![Vec::new(); forest.len()];
  for (index, span) in forest.spans().iter().enumerate() {
    if let Some(parent) = span.parent {
      children[parent as usize].push(index as u32);
    }
  }

  let mut threads = Vec::new();
  for (track, name, category) in [
    (SpanTrack::Frames, "UI", CATEGORY_FRAME),
    (
      SpanTrack::Application(ApplicationLane::Ui),
      "UI scopes",
      CATEGORY_APPLICATION,
    ),
    (
      SpanTrack::Application(ApplicationLane::Worker),
      "Worker",
      CATEGORY_APPLICATION,
    ),
  ] {
    let mut lanes = lanes_of(&forest, track);
    // The UI thread holds the markers and counters even without a pass.
    if lanes.is_empty() && track == SpanTrack::Frames {
      lanes.push(Vec::new());
    }
    for (lane, roots) in lanes.into_iter().enumerate() {
      let mut thread = Thread::default();
      for root in roots {
        thread.emit(&forest, &children, root, None, category);
        thread.sample(forest.spans()[root as usize].end_ns, None);
      }
      if threads.is_empty() {
        thread.add_markers(report);
      }
      let name = match lane {
        0 => name.to_owned(),
        lane => format!("{name} {}", lane + 1),
      };
      let index = threads.len();
      threads.push(thread.into_json(report, &name, index));
    }
  }

  json!({
    "meta": meta(),
    "libs": [],
    "threads": threads,
    "pages": [],
    "profilerOverhead": [],
    "counters": memory_counters(report),
  })
}

/// The roots of `track` in start order, split into lanes whose roots do not
/// overlap: each root goes to the first lane free at its start.
fn lanes_of(forest: &SpanForest, track: SpanTrack) -> Vec<Vec<u32>> {
  let spans = forest.spans();
  let mut roots: Vec<u32> = (0..spans.len() as u32)
    .filter(|index| {
      let span = &spans[*index as usize];
      span.track == track && span.parent.is_none() && span.end_ns > span.start_ns
    })
    .collect();
  roots.sort_by_key(|index| (spans[*index as usize].start_ns, *index));
  let mut lanes: Vec<(u64, Vec<u32>)> = Vec::new();
  for root in roots {
    let span = &spans[root as usize];
    match lanes.iter_mut().find(|(end_ns, _)| *end_ns <= span.start_ns) {
      Some((end_ns, lane)) => {
        *end_ns = span.end_ns;
        lane.push(root);
      }
      None => lanes.push((span.end_ns, vec![root])),
    }
  }
  lanes.into_iter().map(|(_, lane)| lane).collect()
}

/// The tables of one thread, each string, function, frame and stack once.
#[derive(Default)]
struct Thread {
  strings: Vec<String>,
  string_index: HashMap<String, usize>,
  /// Per function, its name.
  funcs: Vec<usize>,
  func_index: HashMap<usize, usize>,
  /// Per frame, its function and category.
  frames: Vec<(usize, usize)>,
  frame_index: HashMap<(usize, usize), usize>,
  /// Per stack, its prefix and frame.
  stacks: Vec<(Option<usize>, usize)>,
  stack_index: HashMap<(Option<usize>, usize), usize>,
  sample_ns: Vec<u64>,
  sample_stacks: Vec<Option<usize>>,
  markers: Vec<Marker>,
}

struct Marker {
  name: usize,
  category: usize,
  start_ms: f64,
  end_ms: f64,
  data: Value,
}

impl Thread {
  fn string(&mut self, value: &str) -> usize {
    if let Some(index) = self.string_index.get(value) {
      return *index;
    }
    let index = self.strings.len();
    self.strings.push(value.to_owned());
    self.string_index.insert(value.to_owned(), index);
    index
  }

  fn stack(&mut self, prefix: Option<usize>, name: &str, category: usize) -> usize {
    let name = self.string(name);
    let funcs = &mut self.funcs;
    let func = *self.func_index.entry(name).or_insert_with(|| {
      funcs.push(name);
      funcs.len() - 1
    });
    let frames = &mut self.frames;
    let frame = *self.frame_index.entry((func, category)).or_insert_with(|| {
      frames.push((func, category));
      frames.len() - 1
    });
    let stacks = &mut self.stacks;
    *self.stack_index.entry((prefix, frame)).or_insert_with(|| {
      stacks.push((prefix, frame));
      stacks.len() - 1
    })
  }

  /// From `at_ns` on, `stack` runs (None: nothing does), until the next sample.
  fn sample(&mut self, at_ns: u64, stack: Option<usize>) {
    if self.sample_ns.last() == Some(&at_ns) {
      *self.sample_stacks.last_mut().expect("a sample per time") = stack;
    } else {
      self.sample_ns.push(at_ns);
      self.sample_stacks.push(stack);
    }
  }

  /// Samples for span `index` and its children: one where the span or a gap
  /// between its children starts, and those of each child.
  fn emit(&mut self, forest: &SpanForest, children: &[Vec<u32>], index: u32, prefix: Option<usize>, category: usize) {
    let span = forest.spans()[index as usize];
    let stack = self.stack(prefix, span.name, category);
    let mut at_ns = span.start_ns;
    for &child in &children[index as usize] {
      let inner = forest.spans()[child as usize];
      if inner.end_ns == inner.start_ns {
        continue;
      }
      if inner.start_ns > at_ns {
        self.sample(at_ns, Some(stack));
      }
      self.emit(forest, children, child, Some(stack), category);
      at_ns = inner.end_ns;
    }
    if span.end_ns > at_ns {
      self.sample(at_ns, Some(stack));
    }
  }

  /// A marker per pass, input dispatch and UI update of the report.
  fn add_markers(&mut self, report: &ProfileReport) {
    for sample in &report.samples {
      let window = self.string(&sample.window);
      let (name, category, data) = match &sample.data {
        SampleData::Pass(pass) => (
          if pass.rendered { "Frame" } else { "Pass" },
          CATEGORY_FRAME,
          pass_data(sample.sequence, window, pass),
        ),
        SampleData::InputDispatch(input) => {
          let kind = self.string(input.kind.name());
          let mut data = json!({ "type": "LurqInput", "kind": kind, "window": window,
            "interactionId": input.interaction_id,
            "queueDelay": input.queue_delay.as_secs_f64() * 1000.0,
            "dispatch": input.total.as_secs_f64() * 1000.0 });
          if let (Some(frame_id), Some(latency)) = (input.presented_frame_id, input.input_to_present) {
            data["presentedFrameId"] = Value::from(frame_id);
            data["inputToPresent"] = Value::from(latency.as_secs_f64() * 1000.0);
          }
          ("Input", CATEGORY_INPUT, data)
        }
        SampleData::UiUpdate(update) => {
          let operation = self.string(match update.kind {
            UiUpdateKind::RootRebuild => "root_rebuild",
            UiUpdateKind::SubtreeRefresh => "subtree_refresh",
          });
          (
            "UI update",
            CATEGORY_FRAME,
            json!({ "type": "LurqUiUpdate", "operation": operation, "window": window,
              "commit": update.commit.as_secs_f64() * 1000.0 }),
          )
        }
      };
      let name = self.string(name);
      self.markers.push(Marker {
        name,
        category,
        start_ms: sample.started_ms,
        end_ms: sample.completed_ms.max(sample.started_ms),
        data,
      });
    }
  }

  fn into_json(self, report: &ProfileReport, name: &str, index: usize) -> Value {
    let frames = self.frames.len();
    let funcs = self.funcs.len();
    let samples = self.sample_ns.len();
    let mut previous_ns = 0;
    let time_deltas: Vec<f64> = self
      .sample_ns
      .iter()
      .map(|at_ns| {
        let delta = at_ns.saturating_sub(previous_ns) as f64 / 1e6;
        previous_ns = *at_ns;
        delta
      })
      .collect();
    let weights: Vec<f64> = (0..samples)
      .map(|sample| {
        self.sample_ns.get(sample + 1).map_or(0.0, |next_ns| {
          next_ns.saturating_sub(self.sample_ns[sample]) as f64 / 1e6
        })
      })
      .collect();
    let markers = &self.markers;
    json!({
      "frameTable": {
        "length": frames,
        "address": vec![-1; frames],
        "inlineDepth": vec![0; frames],
        "category": self.frames.iter().map(|(_, category)| *category).collect::<Vec<_>>(),
        "subcategory": vec![0; frames],
        "func": self.frames.iter().map(|(func, _)| *func).collect::<Vec<_>>(),
        "nativeSymbol": vec![Value::Null; frames],
        "innerWindowID": vec![0; frames],
        "line": vec![Value::Null; frames],
        "column": vec![Value::Null; frames],
      },
      "funcTable": {
        "length": funcs,
        "name": self.funcs,
        "isJS": vec![false; funcs],
        "relevantForJS": vec![false; funcs],
        "resource": vec![-1; funcs],
        "fileName": vec![Value::Null; funcs],
        "lineNumber": vec![Value::Null; funcs],
        "columnNumber": vec![Value::Null; funcs],
      },
      "markers": {
        "length": markers.len(),
        "category": markers.iter().map(|marker| marker.category).collect::<Vec<_>>(),
        "data": markers.iter().map(|marker| marker.data.clone()).collect::<Vec<_>>(),
        "endTime": markers.iter().map(|marker| marker.end_ms).collect::<Vec<_>>(),
        "name": markers.iter().map(|marker| marker.name).collect::<Vec<_>>(),
        // 1: an interval marker.
        "phase": vec![1; markers.len()],
        "startTime": markers.iter().map(|marker| marker.start_ms).collect::<Vec<_>>(),
      },
      "name": name,
      "isMainThread": index == 0,
      "nativeSymbols": { "length": 0, "address": [], "functionSize": [], "libIndex": [], "name": [] },
      "pausedRanges": [],
      "pid": PID,
      "processName": PRODUCT,
      "processShutdownTime": report.ended_ms,
      "processStartupTime": report.started_ms,
      "processType": "default",
      "registerTime": report.started_ms,
      "resourceTable": { "length": 0, "lib": [], "name": [], "host": [], "type": [] },
      "samples": {
        "length": samples,
        "weightType": "tracing-ms",
        "stack": self.sample_stacks,
        "timeDeltas": time_deltas,
        "weight": weights,
      },
      "stackTable": {
        "length": self.stacks.len(),
        "prefix": self.stacks.iter().map(|(prefix, _)| *prefix).collect::<Vec<_>>(),
        "frame": self.stacks.iter().map(|(_, frame)| *frame).collect::<Vec<_>>(),
      },
      "stringArray": self.strings,
      "tid": (index + 1).to_string(),
      "unregisterTime": Value::Null,
      "showMarkersInTimeline": index == 0,
    })
  }
}

fn pass_data(sequence: u64, window: usize, pass: &PassSample) -> Value {
  let mut data = json!({ "type": "LurqPass", "sequence": sequence, "window": window,
    "total": pass.total.as_secs_f64() * 1000.0 });
  if let Some(frame_id) = pass.frame_id {
    data["frameId"] = Value::from(frame_id);
  }
  data
}

/// The memory counters: the runtime's estimate after each pass that has one,
/// and the live heap after each pass a `CountingAllocator` counted.
fn memory_counters(report: &ProfileReport) -> Vec<Value> {
  let runtime = counter(
    report,
    "Runtime memory",
    "lurq's estimate of the runtime's memory at the end of each pass (without pass counters, its estimate cached once per second); not a heap allocation trace",
    |pass| {
      pass
        .counters
        .map(|counters| counters.memory_bytes)
        .or(pass.frame.as_ref().map(|frame| frame.memory.total_bytes))
        .map(|bytes| bytes as f64)
    },
  );
  let heap = counter(
    report,
    "Live heap",
    "bytes allocated and not yet freed by every thread at the end of each pass, counted by lurq's CountingAllocator",
    |pass| {
      pass
        .counters
        .and_then(|counters| counters.allocations)
        .map(|allocations| allocations.live_bytes as f64)
    },
  );
  runtime.into_iter().chain(heap).collect()
}

/// A counter of `value` at the end of each pass that has one; None without any.
fn counter(
  report: &ProfileReport,
  name: &str,
  description: &str,
  value: impl Fn(&PassSample) -> Option<f64>,
) -> Option<Value> {
  let mut points: Vec<(u64, f64)> = report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::Pass(pass) => value(pass).map(|value| (ms_to_ns(sample.completed_ms), value)),
      SampleData::UiUpdate(_) | SampleData::InputDispatch(_) => None,
    })
    .collect();
  if points.is_empty() {
    return None;
  }
  points.sort_by_key(|(at_ns, _)| *at_ns);
  // A counter's samples are changes from the previous one.
  let (mut previous_ns, mut previous_value) = (0, 0.0);
  let mut time_deltas = Vec::with_capacity(points.len());
  let mut counts = Vec::with_capacity(points.len());
  for (at_ns, value) in &points {
    time_deltas.push((at_ns - previous_ns) as f64 / 1e6);
    counts.push(value - previous_value);
    (previous_ns, previous_value) = (*at_ns, *value);
  }
  Some(json!({
    "category": "Memory",
    "name": name,
    "description": description,
    "mainThreadIndex": 0,
    "pid": PID,
    "samples": {
      "length": points.len(),
      "count": counts,
      "number": vec![0; points.len()],
      "timeDeltas": time_deltas,
    },
  }))
}

fn meta() -> Value {
  let category = |name: &str, color: &str| json!({ "name": name, "color": color, "subcategories": ["Other"] });
  let mut categories = vec![Value::Null; 4];
  categories[CATEGORY_OTHER] = category("Other", "grey");
  categories[CATEGORY_FRAME] = category("Frame", "green");
  categories[CATEGORY_APPLICATION] = category("Application", "blue");
  categories[CATEGORY_INPUT] = category("Input", "yellow");
  let field = |key: &str, label: &str, format: &str| json!({ "key": key, "label": label, "format": format });
  let display = ["marker-chart", "marker-table", "timeline-overview"];
  json!({
    "categories": categories,
    "debug": false,
    "extensions": { "length": 0, "baseURL": [], "id": [], "name": [] },
    // Not sampled: each sample is weighted by its own duration.
    "interval": 1.0,
    "preprocessedProfileVersion": FIREFOX_PROCESSED_PROFILE_VERSION,
    "processType": 0,
    "product": PRODUCT,
    // Report times count from the collector's monotonic epoch, not a wall-clock time.
    "startTime": 0.0,
    "symbolicated": true,
    "pausedRanges": [],
    // Ignored by the profiler, which reads preprocessedProfileVersion.
    "version": 24,
    "usesOnlyOneStackType": true,
    "sourceCodeIsNotOnSearchfox": true,
    "markerSchema": [
      {
        "name": "LurqInput",
        "display": display,
        "chartLabel": "{marker.data.kind}",
        "tableLabel": "{marker.name} {marker.data.kind}",
        "fields": [
          field("kind", "Kind", "unique-string"),
          field("window", "Window", "unique-string"),
          field("interactionId", "Interaction", "integer"),
          field("queueDelay", "Queue delay", "duration"),
          field("dispatch", "Dispatch", "duration"),
          field("presentedFrameId", "Presented in frame", "integer"),
          field("inputToPresent", "Input to present", "duration"),
        ],
      },
      {
        "name": "LurqPass",
        "display": display,
        "chartLabel": "{marker.name}",
        "tooltipLabel": "{marker.name} #{marker.data.sequence}",
        "tableLabel": "{marker.name} #{marker.data.sequence}",
        "fields": [
          field("sequence", "Sequence", "integer"),
          field("frameId", "Frame id", "integer"),
          field("window", "Window", "unique-string"),
          field("total", "Pass time", "duration"),
        ],
      },
      {
        "name": "LurqUiUpdate",
        "display": ["marker-chart", "marker-table"],
        "chartLabel": "{marker.data.operation}",
        "tableLabel": "{marker.name} {marker.data.operation}",
        "fields": [
          field("operation", "Operation", "unique-string"),
          field("window", "Window", "unique-string"),
          field("commit", "Commit", "duration"),
        ],
      },
    ],
  })
}
