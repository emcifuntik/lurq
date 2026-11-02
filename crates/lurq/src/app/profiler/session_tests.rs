use super::*;

#[test]
fn profiling_availability_is_explicit() {
  assert_eq!(
    BuildAvailability::default().perf_profile,
    cfg!(feature = "perf_profile")
  );
  #[cfg(not(feature = "perf_profile"))]
  assert!(matches!(
    ProfilingHandle::new().start(SessionOptions::default()),
    Err(ProfileError::FeatureDisabled)
  ));
}

#[cfg(feature = "perf_profile")]
mod enabled {
  use std::{
    sync::mpsc,
    time::{Duration, Instant},
  };

  use super::*;
  use crate::app::profiler::{
    model::{MAX_ACTIVE_SESSIONS, MAX_SAMPLES_PER_SESSION},
    producer::WindowProfiler,
  };

  fn sample(handle: &ProfilingHandle, window: &str) {
    handle.record(
      window,
      Instant::now(),
      SampleData::UiUpdate(UiUpdateSample {
        frame_id: None,
        kind: UiUpdateKind::RootRebuild,
        total: Duration::from_millis(1),
        commit: Duration::ZERO,
      }),
    );
  }

  #[test]
  fn overlapping_sessions_end_out_of_order_without_reset_or_mixing() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let first = handle.start(SessionOptions::default()).unwrap().id;
    sample(&handle, "main");
    let second = handle.start(SessionOptions::default()).unwrap().id;
    sample(&handle, "main");
    let ended_second = handle.end(second).unwrap();
    assert_eq!(ended_second.completed_samples, 1);
    assert_eq!(ended_second.samples[0].sequence, 2);
    sample(&handle, "main");
    let ended_first = handle.end(first).unwrap();
    assert_eq!(
      ended_first
        .samples
        .iter()
        .map(|sample| sample.sequence)
        .collect::<Vec<_>>(),
      [1, 2, 3]
    );
    assert_eq!(ended_second.completed_samples, 1);
    assert_eq!(ended_second.samples.len(), 1);
    assert!(matches!(handle.end(second), Err(ProfileError::AlreadyEnded)));
    assert!(matches!(handle.read(SessionId(999)), Err(ProfileError::UnknownSession)));
  }

  #[test]
  fn end_during_stalled_pass_observes_phase_and_cannot_receive_late_completion() {
    let mut producer = WindowProfiler::new();
    let handle = producer.handle();
    let first = handle.start(SessionOptions::default()).unwrap().id;
    let (running_tx, running_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
      let (started, _pass) = producer.begin_pass(7);
      let _layout = producer.context.phase(Phase::LayoutUpdate);
      running_tx.send(()).unwrap();
      resume_rx.recv().unwrap();
      producer.finish_pass(
        started,
        &crate::app::PassReport {
          required: true,
          ..Default::default()
        },
        "none",
        None,
      );
    });
    running_rx.recv().unwrap();
    let second = handle.start(SessionOptions::default()).unwrap().id;
    let ended_second = handle.end(second).unwrap();
    assert_eq!(ended_second.completed_samples, 0);
    assert_eq!(ended_second.in_flight.len(), 1);
    assert_eq!(ended_second.in_flight[0].phase, Phase::LayoutUpdate);
    assert_eq!(ended_second.in_flight[0].frame_id, Some(7));
    assert!(ended_second.in_flight[0].started_before_session);
    assert!(ended_second.in_flight[0].elapsed_ms >= 0.);
    resume_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(ended_second.completed_samples, 0);
    assert_eq!(ended_second.in_flight[0].phase, Phase::LayoutUpdate);
    let ended_first = handle.end(first).unwrap();
    assert_eq!(ended_first.completed_samples, 1);
    assert!(ended_first.in_flight.is_empty());
    assert!(!ended_first.windows[0].open);
  }

  #[cfg(feature = "mcp")]
  #[test]
  fn pass_completion_includes_blocked_mcp_notification_tail() {
    use std::{
      future::Future,
      pin::Pin,
      sync::{Arc, Mutex},
      task::{Context, Wake, Waker},
    };

    use crate::{
      app::{App, Tree},
      components::Column,
      mcp::{McpWaitEntry, McpWaitMode},
    };

    struct BlockingWake {
      entered: mpsc::Sender<Instant>,
      resume: Mutex<mpsc::Receiver<()>>,
    }
    impl Wake for BlockingWake {
      fn wake(self: Arc<Self>) {
        self.wake_by_ref();
      }
      fn wake_by_ref(self: &Arc<Self>) {
        self.entered.send(Instant::now()).unwrap();
        self.resume.lock().unwrap().recv().unwrap();
      }
    }
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let waker = Waker::from(Arc::new(BlockingWake {
      entered: entered_tx,
      resume: Mutex::new(resume_rx),
    }));
    let (reply, mut receiver) = tokio::sync::oneshot::channel();
    assert!(
      Pin::new(&mut receiver)
        .poll(&mut Context::from_waker(&waker))
        .is_pending()
    );
    let (handle_tx, handle_rx) = mpsc::channel();
    let (begin_tx, begin_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
      let mut tree = Tree::new();
      let mut app = App::new();
      tree.set_root(Column::new());
      tree.mcp_wait_entries.push(McpWaitEntry {
        mode: McpWaitMode::Frames(0),
        reply: Some(reply),
      });
      handle_tx.send(tree.profiling_handle()).unwrap();
      begin_rx.recv().unwrap();
      tree.pass_headless(&mut app);
    });
    let handle = handle_rx.recv().unwrap();
    let first = handle.start(Default::default()).unwrap().id;
    begin_tx.send(()).unwrap();
    let tail_started = entered_rx.recv().unwrap();
    let during_tail = handle.read(first).unwrap();
    let second = handle.start(Default::default()).unwrap().id;
    let ended_second = handle.end(second).unwrap();
    let resumed = Instant::now();
    resume_tx.send(()).unwrap();
    worker.join().unwrap();
    let ended_first = handle.end(first).unwrap();
    assert!(
      during_tail
        .samples
        .iter()
        .all(|sample| !matches!(sample.data, SampleData::Pass(_)))
    );
    assert_eq!(ended_second.completed_samples, 0);
    assert_eq!(ended_second.in_flight[0].phase, Phase::PassNotifications);
    assert!(ended_second.in_flight[0].started_before_session);
    assert!(ended_second.in_flight[0].elapsed_ms >= ended_second.in_flight[0].phase_elapsed_ms);
    let completed = ended_first
      .samples
      .iter()
      .find(|sample| matches!(sample.data, SampleData::Pass(_)))
      .unwrap();
    assert!(completed.started_ms <= handle.millis(tail_started));
    assert!(completed.completed_ms >= handle.millis(resumed));
    assert!(ended_first.in_flight.is_empty());
    assert!(ended_second.samples.is_empty());
    assert_eq!(ended_second.in_flight[0].phase, Phase::PassNotifications);
    assert!(
      Pin::new(&mut receiver)
        .poll(&mut Context::from_waker(&waker))
        .is_ready()
    );
  }

  #[test]
  fn real_blocked_input_callback_is_observable_before_any_frame_pass() {
    use std::sync::{Arc, Mutex};

    use crate::{app::Tree, components::Column, node::Element};
    let (handle_tx, handle_rx) = mpsc::channel();
    let (begin_tx, begin_rx) = mpsc::channel();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Arc::new(Mutex::new(resume_rx));
    let worker = std::thread::spawn(move || {
      let mut tree = Tree::new();
      let root: Element = Column::new()
        .on_key_down(move |event: crate::app::events::KeyboardEvent| {
          event.prevent_default();
          entered_tx.send(()).unwrap();
          resume_rx.lock().unwrap().recv().unwrap();
        })
        .into();
      tree.set_root(root);
      handle_tx.send(tree.profiling_handle()).unwrap();
      begin_rx.recv().unwrap();
      tree.key_down("private input".into(), "private code".into(), false, false, false);
      assert_eq!(tree.frame_count(), 0);
    });
    let handle = handle_rx.recv().unwrap();
    let first = handle.start(Default::default()).unwrap().id;
    begin_tx.send(()).unwrap();
    entered_rx.recv().unwrap();
    let second = handle.start(Default::default()).unwrap().id;
    let ended_second = handle.end(second).unwrap();
    assert_eq!(ended_second.in_flight[0].phase, Phase::InputDispatch);
    assert!(ended_second.in_flight[0].frame_id.is_none());
    assert!(ended_second.samples.is_empty());
    resume_tx.send(()).unwrap();
    worker.join().unwrap();
    let ended_first = handle.end(first).unwrap();
    let input = ended_first
      .samples
      .iter()
      .find_map(|sample| match &sample.data {
        SampleData::InputDispatch(input) => Some(input),
        _ => None,
      })
      .unwrap();
    assert_eq!(input.kind, InputKind::Keyboard);
    assert!(input.frame_id.is_none());
    assert!(input.total > Duration::ZERO);
    #[cfg(any(feature = "serde", feature = "mcp"))]
    {
      let exported = ended_first.to_json().to_string();
      assert!(!exported.contains("private input"));
      assert!(!exported.contains("private code"));
    }
  }

  #[test]
  fn nested_pass_restores_an_unfinished_outer_input_activity() {
    let mut producer = WindowProfiler::new();
    let handle = producer.handle();
    let id = handle.start(Default::default()).unwrap().id;
    let input = producer.context.input(InputKind::Pointer);
    let (started, pass) = producer.begin_pass(1);
    producer.finish_pass(
      started,
      &crate::app::PassReport {
        required: true,
        ..Default::default()
      },
      "none",
      None,
    );
    drop(pass);
    let current = handle.read(id).unwrap();
    assert_eq!(current.in_flight[0].phase, Phase::InputDispatch);
    assert!(current.in_flight[0].frame_id.is_none());
    drop(input);
    assert!(handle.end(id).unwrap().in_flight.is_empty());
  }

  #[test]
  fn boundary_crossing_completed_operations_are_excluded() {
    let mut producer = WindowProfiler::new();
    let handle = producer.handle();
    let (started, pass) = producer.begin_pass(1);
    let id = handle.start(SessionOptions::default()).unwrap().id;
    producer.finish_pass(
      started,
      &crate::app::PassReport {
        required: true,
        ..Default::default()
      },
      "none",
      None,
    );
    drop(pass);
    let report = handle.end(id).unwrap();
    assert_eq!(report.completed_samples, 0);
    assert_eq!(report.boundary_excluded_samples, 1);
  }

  #[test]
  fn delayed_phase_guard_cannot_resurrect_a_closed_window() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let id = handle.start(Default::default()).unwrap().id;
    let outer = producer.context.phase(Phase::LayoutUpdate);
    let nested = producer.context.phase(Phase::Rebuild);
    producer.set_open(false);
    drop(nested);
    drop(outer);
    let report = handle.end(id).unwrap();
    assert!(report.in_flight.is_empty());
    assert!(!report.windows[0].open);
  }

  #[test]
  fn history_limits_drop_oldest_without_changing_other_sessions() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    assert!(matches!(
      handle.start(SessionOptions {
        max_samples: 0,
        ..Default::default()
      }),
      Err(ProfileError::InvalidSampleLimit)
    ));
    assert!(matches!(
      handle.start(SessionOptions {
        max_samples: MAX_SAMPLES_PER_SESSION + 1,
        ..Default::default()
      }),
      Err(ProfileError::InvalidSampleLimit)
    ));
    let bounded = handle
      .start(SessionOptions {
        max_samples: 2,
        ..Default::default()
      })
      .unwrap()
      .id;
    let other = handle.start(SessionOptions::default()).unwrap().id;
    for _ in 0..5 {
      sample(&handle, "main");
    }
    let report = handle.end(bounded).unwrap();
    assert_eq!(
      (report.completed_samples, report.dropped_samples, report.samples.len()),
      (5, 3, 2)
    );
    assert_eq!(
      report.samples.iter().map(|sample| sample.sequence).collect::<Vec<_>>(),
      [4, 5]
    );
    assert_eq!(handle.end(other).unwrap().samples.len(), 5);
    let ids: Vec<_> = (0..MAX_ACTIVE_SESSIONS)
      .map(|_| handle.start(Default::default()).unwrap().id)
      .collect();
    assert!(matches!(
      handle.start(Default::default()),
      Err(ProfileError::SessionLimit)
    ));
    handle.end(ids[3]).unwrap();
    assert!(handle.start(Default::default()).is_ok());
  }

  #[test]
  fn window_and_app_lifetimes_are_independent_and_devtools_is_opt_in() {
    let root = WindowProfiler::new();
    let handle = root.handle();
    let mut second = WindowProfiler::new();
    second.attach(&root, "w1".into(), false);
    let mut devtools = WindowProfiler::new();
    devtools.attach(&root, "w2".into(), true);
    let public = handle.start(Default::default()).unwrap().id;
    let all = handle
      .start(SessionOptions {
        include_devtools: true,
        ..Default::default()
      })
      .unwrap()
      .id;
    sample(&handle, "main");
    sample(&handle, "w1");
    sample(&handle, "w2");
    second.set_open(false);
    sample(&handle, "w1");
    let report = handle.end(public).unwrap();
    assert_eq!(report.samples.len(), 2);
    assert!(!report.windows.iter().find(|window| window.id == "w1").unwrap().open);
    assert!(!report.windows.iter().any(|window| window.devtools));
    assert_eq!(handle.end(all).unwrap().samples.len(), 3);
    let unrelated = WindowProfiler::new();
    let unrelated_id = unrelated.handle().start(Default::default()).unwrap().id;
    sample(&handle, "main");
    assert!(unrelated.handle().end(unrelated_id).unwrap().samples.is_empty());
    let survive_close = handle.start(Default::default()).unwrap().id;
    drop(root);
    assert!(matches!(handle.start(Default::default()), Err(ProfileError::Closed)));
    assert!(
      handle
        .end(survive_close)
        .unwrap()
        .windows
        .iter()
        .all(|window| !window.open)
    );
  }

  #[test]
  fn window_metadata_and_identifiers_have_fixed_bounds() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    for index in 1..super::super::model::MAX_TRACKED_WINDOWS {
      handle.register(&format!("w{index}"), false);
    }
    handle.register("overflow", false);
    handle.register(&"x".repeat(super::super::model::MAX_WINDOW_ID_BYTES + 1), false);
    let id = handle.start(Default::default()).unwrap().id;
    let report = handle.read(id).unwrap();
    assert_eq!(report.windows.len(), 64);
    assert_eq!(report.untracked_windows, 2);
    handle.set_open("w1", false);
    handle.register("replacement", false);
    assert_eq!(handle.end(id).unwrap().windows.len(), 64);
  }

  #[test]
  fn real_headless_pass_records_layout_without_manufacturing_render_or_gpu_timings() {
    use crate::{
      app::{App, Tree},
      components::{Column, Text},
    };
    let mut tree = Tree::new();
    let mut app = App::new();
    tree.set_root(Column::new().child(Text::new("Profile fixture")));
    let handle = tree.profiling_handle();
    let id = handle.start(Default::default()).unwrap().id;
    let frame_count = tree.frame_count();
    let before_redraw = tree.needs_redraw();
    handle.read(id).unwrap();
    assert_eq!(tree.needs_redraw(), before_redraw);
    assert_eq!(tree.frame_count(), frame_count);
    let pass = tree.pass_headless(&mut app);
    assert!(!pass.rendered);
    let report = handle.end(id).unwrap();
    let SampleData::Pass(sample) = &report.samples.last().unwrap().data else {
      panic!("expected a pass")
    };
    assert!(sample.total > Duration::ZERO);
    assert!(sample.layout_update > Duration::ZERO);
    assert!(sample.layout_compute > Duration::ZERO);
    assert_eq!(sample.component_after_layout, Duration::ZERO);
    assert!(sample.frame.is_none());
    assert!(sample.frame_id.is_none());
    assert_eq!(sample.backend, "none");
    #[cfg(any(feature = "serde", feature = "mcp"))]
    {
      let json = report.to_json();
      assert!(json["samples"][0]["data"]["gpu_timing_ms"].is_null());
      assert_eq!(json["build"]["gpu_timestamps"]["available"], false);
      assert!(!json.to_string().contains("Profile fixture"));
    }
  }
}
