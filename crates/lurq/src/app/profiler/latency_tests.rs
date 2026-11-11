//! Input latency, presented-frame timing and pass reasons in the samples.
use std::time::{Duration, Instant};

use super::{model::PresentTracker, producer::WindowProfiler, *};
use crate::app::{PassReasons, PassReport};

const REFRESH: Duration = Duration::from_millis(16);

/// A time `ms` after `base`.
fn at(base: Instant, ms: u64) -> Instant {
  base + Duration::from_millis(ms)
}

/// A base far enough in the past that every time of a test is already over:
/// an input stamp from the future is not taken.
fn past() -> Instant {
  Instant::now()
    .checked_sub(Duration::from_secs(10))
    .expect("a monotonic clock past ten seconds")
}

/// A pass of `producer` that presents frame `frame_id` at `presented`.
fn present(producer: &mut WindowProfiler, frame_id: u64, presented: Instant, wants_next_frame: bool) {
  let (started, guard) = producer.begin_pass(frame_id);
  producer.presented(presented);
  producer.set_wants_next_frame(wants_next_frame);
  producer.finish_pass(
    started,
    &PassReport {
      required: true,
      rendered: true,
      ..Default::default()
    },
    "none",
    None,
  );
  drop(guard);
}

/// An input received at `received`, dispatched now.
fn input(producer: &mut WindowProfiler, received: Instant, wants_frame: bool) {
  producer.stamp_input(Some(received));
  let guard = producer.input(InputKind::Pointer);
  producer.end_input(guard, wants_frame);
}

fn presents(report: &ProfileReport) -> Vec<(u64, PresentTiming)> {
  report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::Pass(pass) => Some((pass.frame_id.unwrap(), pass.present.unwrap())),
      _ => None,
    })
    .collect()
}

fn inputs(report: &ProfileReport) -> Vec<InputDispatchSample> {
  let mut inputs: Vec<_> = report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::InputDispatch(input) => Some(input.clone()),
      _ => None,
    })
    .collect();
  inputs.sort_by_key(|input| input.interaction_id);
  inputs
}

#[test]
fn a_late_frame_misses_vsyncs_only_when_wanted_within_a_refresh() {
  let base = past();
  let mut tracker = PresentTracker::default();
  // The first frame has nothing to be late against.
  assert_eq!(
    tracker.present(at(base, 0), Some(at(base, 0)), Some(REFRESH)),
    (None, false, 0)
  );
  // Wanted at the last present: due a refresh after it.
  let on_time = tracker.present(at(base, 16), Some(at(base, 0)), Some(REFRESH));
  assert_eq!(on_time, (Some(REFRESH), true, 0));
  let one_late = tracker.present(at(base, 48), Some(at(base, 16)), Some(REFRESH));
  assert_eq!(one_late, (Some(Duration::from_millis(32)), true, 1));
  // Wanted after the last present: counted from when it was wanted.
  let wanted_late = tracker.present(at(base, 82), Some(at(base, 62)), Some(REFRESH));
  assert_eq!(wanted_late, (Some(Duration::from_millis(34)), true, 0));
  // Wanted over a refresh after the last present: an idle gap, never late.
  assert_eq!(
    tracker.present(at(base, 2000), Some(at(base, 1900)), Some(REFRESH)),
    (Some(Duration::from_millis(1918)), false, 0)
  );
  // Not wanted by anything known, or without a refresh: not a run either.
  assert!(!tracker.present(at(base, 2100), None, Some(REFRESH)).1);
  assert!(!tracker.present(at(base, 2200), Some(at(base, 2100)), None).1);
}

#[test]
fn missed_vsyncs_are_flagged_inside_runs_and_never_across_idle_gaps() {
  let base = past();
  let mut producer = WindowProfiler::new();
  producer.set_refresh_interval(Some(REFRESH));
  let handle = producer.handle();
  let id = handle.start_recording(Default::default()).unwrap().id;
  // An animation: each frame wants the next at once; frame 3 comes two
  // refreshes after frame 2.
  present(&mut producer, 1, at(base, 0), true);
  present(&mut producer, 2, at(base, 16), true);
  present(&mut producer, 3, at(base, 48), false);
  // The animation ended: the next frame, a second later, follows an idle gap.
  present(&mut producer, 4, at(base, 1000), false);
  // A drag: an input within a refresh of frame 4 asks for frame 5, which
  // comes 90 ms after it.
  input(&mut producer, at(base, 1010), true);
  present(&mut producer, 5, at(base, 1100), false);
  // A click long after: idle again, however long its frame took.
  input(&mut producer, at(base, 3000), true);
  present(&mut producer, 6, at(base, 3100), false);
  // A run the window broke off (it was hidden) resumes after a gap.
  present(&mut producer, 7, at(base, 4000), true);
  producer.break_run();
  present(&mut producer, 8, at(base, 5000), false);
  let report = handle.end(id).unwrap();

  let flagged: Vec<_> = presents(&report)
    .into_iter()
    .map(|(frame, present)| (frame, present.continuous, present.missed_vsyncs))
    .collect();
  assert_eq!(
    flagged,
    [
      (1, false, 0),
      (2, true, 0),
      (3, true, 1),
      (4, false, 0),
      (5, true, 5),
      (6, false, 0),
      (7, false, 0),
      (8, false, 0)
    ]
  );
  let (_, third) = presents(&report)[2];
  assert_eq!(third.interval, Some(Duration::from_millis(32)));
  assert_eq!(third.refresh_interval, Some(REFRESH));
  assert!((third.presented_ms - handle.millis(at(base, 48))).abs() < 1e-6);
}

#[test]
fn an_input_reports_its_queue_delay_dispatch_and_latency_to_the_frame_that_presented_it() {
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start_recording(Default::default()).unwrap().id;
  // After the session's start: an input received before it is left out.
  let base = Instant::now();
  present(&mut producer, 1, at(base, 0), false);
  // Two inputs wait for the next frame; one between them asks for none.
  input(&mut producer, at(base, 100), true);
  input(&mut producer, at(base, 104), false);
  input(&mut producer, at(base, 108), true);
  // A required pass that could not render presents nothing: they still wait.
  let (started, guard) = producer.begin_pass(2);
  producer.finish_pass(
    started,
    &PassReport {
      required: true,
      ..Default::default()
    },
    "none",
    None,
  );
  drop(guard);
  present(&mut producer, 3, at(base, 140), false);
  // An input the session ends before any frame presents.
  input(&mut producer, at(base, 200), true);
  let report = handle.end(id).unwrap();

  let inputs = inputs(&report);
  let shape: Vec<_> = inputs
    .iter()
    .map(|input| (input.interaction_id, input.presented_frame_id, input.input_to_present))
    .collect();
  assert_eq!(
    shape,
    [
      (1, Some(3), Some(Duration::from_millis(40))),
      (2, None, None),
      (3, Some(3), Some(Duration::from_millis(32))),
      (4, None, None),
    ]
  );
  for input in &inputs {
    assert_eq!(input.kind, InputKind::Pointer);
    assert!(input.frame_id.is_none());
  }
  // An input sample starts when the shell received it.
  let first = report
    .samples
    .iter()
    .find(|sample| matches!(&sample.data, SampleData::InputDispatch(input) if input.interaction_id == 1))
    .unwrap();
  assert!((first.started_ms - handle.millis(at(base, 100))).abs() < 1e-6);
}

#[test]
fn the_queue_delay_runs_from_the_shell_stamp_and_a_stamp_serves_one_input() {
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start_recording(Default::default()).unwrap().id;
  producer.stamp_input(Some(Instant::now()));
  std::thread::sleep(Duration::from_millis(2));
  let guard = producer.input(InputKind::Keyboard);
  producer.end_input(guard, false);
  // The stamp went with that input: the next one, synthetic, has none.
  let guard = producer.input(InputKind::Scroll);
  producer.end_input(guard, false);
  // A window that closes ends the inputs still waiting for its frame.
  let guard = producer.input(InputKind::Pointer);
  producer.end_input(guard, true);
  producer.set_open(false);
  let report = handle.end(id).unwrap();
  let inputs = inputs(&report);
  let kinds: Vec<_> = inputs.iter().map(|input| input.kind).collect();
  assert_eq!(kinds, [InputKind::Keyboard, InputKind::Scroll, InputKind::Pointer]);
  assert!(inputs[0].queue_delay >= Duration::from_millis(2));
  assert_eq!(inputs[1].queue_delay, Duration::ZERO);
  assert!(inputs.iter().all(|input| input.presented_frame_id.is_none()));
}

#[test]
fn each_pass_sample_carries_the_reasons_it_ran() {
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start(Default::default()).unwrap().id;
  let reasons = PassReasons {
    timer_run: true,
    layout_dirty: true,
    ..Default::default()
  };
  let (started, guard) = producer.begin_pass(1);
  producer.finish_pass(
    started,
    &PassReport {
      required: true,
      reasons,
      ..Default::default()
    },
    "none",
    None,
  );
  drop(guard);
  let report = handle.end(id).unwrap();
  let SampleData::Pass(pass) = &report.samples[0].data else {
    panic!("expected a pass");
  };
  assert_eq!(pass.reasons, reasons);
  assert_eq!(pass.reasons.names().collect::<Vec<_>>(), ["timer_run", "layout_dirty"]);
  // Not presented: no present timing.
  assert!(pass.present.is_none());
  #[cfg(any(feature = "serde", feature = "mcp"))]
  {
    let json = report.to_json();
    let data = &json["samples"][0]["data"];
    assert_eq!(data["reasons"], serde_json::json!(["timer_run", "layout_dirty"]));
    assert!(data["present"].is_null());
    assert_eq!(json["schema_version"], 1);
  }
}

#[cfg(any(feature = "serde", feature = "mcp"))]
#[test]
fn the_export_adds_latency_and_present_timing_without_content() {
  let mut producer = WindowProfiler::new();
  producer.set_refresh_interval(Some(REFRESH));
  let handle = producer.handle();
  let id = handle.start(Default::default()).unwrap().id;
  let base = Instant::now();
  present(&mut producer, 1, at(base, 0), true);
  input(&mut producer, at(base, 5), true);
  present(&mut producer, 2, at(base, 48), false);
  let json = handle.end(id).unwrap().to_json();
  let samples = json["samples"].as_array().unwrap();
  let pass = samples
    .iter()
    .find(|sample| sample["data"]["frame_id"] == 2 && sample["data"]["kind"] == "pass")
    .unwrap();
  let close = |value: &serde_json::Value, expected: f64| {
    assert!(
      (value.as_f64().unwrap() - expected).abs() < 1e-6,
      "{value} vs {expected}"
    );
  };
  let present = &pass["data"]["present"];
  assert_eq!(present["continuous"], true);
  assert_eq!(present["missed_vsync"], true);
  assert_eq!(present["missed_vsyncs"], 2);
  close(&present["interval_ms"], 48.0);
  close(&present["refresh_interval_ms"], 16.0);
  let input = samples
    .iter()
    .find(|sample| sample["data"]["kind"] == "input_dispatch")
    .unwrap();
  assert_eq!(input["data"]["interaction_id"], 1);
  assert_eq!(input["data"]["presented_frame_id"], 2);
  close(&input["data"]["latency_ms"]["input_to_present"], 43.0);
  assert!(input["data"]["latency_ms"]["queue_delay"].as_f64().unwrap() >= 0.0);
  assert!(input["data"]["latency_ms"]["dispatch"].as_f64().unwrap() >= 0.0);
  assert_eq!(json["schema_version"], 1);
}

/// A tree whose passes present: an input it dispatches is presented by the
/// next frame, and its sample holds no position, key or text.
#[cfg(feature = "screenshot")]
mod tree {
  use raw_window_handle::{DisplayHandle, WindowHandle};

  use super::*;
  use crate::{
    app::{App, Tree, capture_test_engine::TestSurface, render_engine::RenderEngine},
    components::{Button, Column},
    layout::render_list::RenderList,
  };

  struct Presenting;

  impl RenderEngine for Presenting {
    fn resize(&mut self, _: u32, _: u32) {}

    fn render(&mut self, _: &RenderList, _: WindowHandle<'_>, _: DisplayHandle<'_>) -> bool {
      true
    }
  }

  #[test]
  fn a_shell_stamped_key_is_presented_by_the_next_frame_without_its_content() {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.set_render_engine_factory(|| Box::new(Presenting));
    tree.resize(300, 120);
    tree.set_root(Column::new().child(Button::new("Save").size(120.0, 32.0).tab_index(0)));
    tree.mouse_move(287.375, 111.625);
    assert!(tree.pass(&mut app, &TestSurface).rendered);
    let handle = tree.profiling_handle();
    let id = handle.start_recording(Default::default()).unwrap().id;

    // The shell stamps the event, then dispatches it a little later.
    tree.stamp_profiled_input(Some(Instant::now()));
    std::thread::sleep(Duration::from_millis(2));
    tree.key_down("Tab".to_owned(), "Tab".to_owned(), false, false, false);
    assert!(tree.needs_redraw(), "the Tab moved focus");
    let report = tree.pass(&mut app, &TestSurface);
    assert!(report.rendered);
    // A move over nothing asks for no frame: presented by none.
    tree.stamp_profiled_input(Some(Instant::now()));
    tree.mouse_move(286.125, 112.875);
    let ended = handle.end(id).unwrap();

    let pass = ended
      .samples
      .iter()
      .find_map(|sample| match &sample.data {
        SampleData::Pass(pass) if pass.rendered => Some(pass),
        _ => None,
      })
      .unwrap();
    assert!(pass.reasons.any());
    assert_eq!(pass.reasons, report.reasons);
    let present = pass.present.unwrap();
    assert!(present.interval.is_some(), "timed against the frame before the session");
    let inputs = inputs(&ended);
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].kind, InputKind::Keyboard);
    assert_eq!(inputs[0].presented_frame_id, pass.frame_id);
    assert!(inputs[0].queue_delay >= Duration::from_millis(2));
    let latency = inputs[0].input_to_present.unwrap();
    assert!(latency >= inputs[0].queue_delay + inputs[0].total);
    assert_eq!(inputs[1].kind, InputKind::Pointer);
    assert_eq!(inputs[1].presented_frame_id, None);
    assert!(inputs[1].interaction_id > inputs[0].interaction_id);
    #[cfg(any(feature = "serde", feature = "mcp"))]
    {
      let exported = ended.to_json().to_string();
      for content in ["Tab", "287.375", "111.625", "286.125", "112.875", "Save"] {
        assert!(!exported.contains(content), "the export holds {content}");
      }
    }
  }
}
