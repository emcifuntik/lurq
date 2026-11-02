use super::*;

#[test]
fn application_handles_and_parent_tokens_support_explicit_worker_handoff() {
  fn send_sync<T: Send + Sync>() {}
  send_sync::<ProfilingHandle>();
  send_sync::<ApplicationScopeParent>();
  #[cfg(not(feature = "perf_profile"))]
  {
    let handle = ProfilingHandle::new();
    let scope = handle.application_scope("unknown", "private invalid label", ApplicationLane::Worker);
    assert_eq!(scope.status(), ApplicationScopeStatus::FeatureDisabled);
    assert!(scope.parent().is_none());
    assert!(scope.id().is_none());
    assert_eq!(
      scope.child("child", ApplicationLane::Ui).status(),
      ApplicationScopeStatus::FeatureDisabled
    );
    assert!(matches!(
      handle.start(Default::default()),
      Err(ProfileError::FeatureDisabled)
    ));
  }
}

#[cfg(feature = "perf_profile")]
mod enabled {
  use std::sync::mpsc;

  use super::*;
  use crate::app::profiler::producer::WindowProfiler;

  #[test]
  fn overlapping_end_during_worker_stall_keeps_first_and_ui_phase_independent() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let first = handle.start(Default::default()).unwrap().id;
    let phase = producer.context.phase(Phase::InputDispatch);
    let parent = handle.application_scope("main", "local_save_apply", ApplicationLane::Ui);
    let token = parent.parent().unwrap();
    let parent_id = parent.id().unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker_handle = handle.clone();
    let worker = std::thread::spawn(move || {
      let scope = worker_handle.application_scope_child(&token, "store_save", ApplicationLane::Worker);
      assert_eq!(scope.status(), ApplicationScopeStatus::Active);
      entered_tx.send(scope.id().unwrap()).unwrap();
      resume_rx.recv().unwrap();
      // A real blocked synchronous callback, released only after end(second).
      scope.finish();
    });
    let worker_id = entered_rx.recv().unwrap();
    let second = handle.start(Default::default()).unwrap().id;
    let ended_second = handle.end(second).unwrap();
    assert_eq!(ended_second.in_flight[0].phase, Phase::InputDispatch);
    let second_app = ended_second.application_scopes.as_ref().unwrap();
    assert_eq!(second_app.in_flight.len(), 2);
    assert!(second_app.in_flight.iter().all(|scope| scope.started_before_session));
    let child = second_app
      .in_flight
      .iter()
      .find(|scope| scope.scope.id == worker_id)
      .unwrap();
    assert_eq!(child.scope.parent_id, Some(parent_id));
    assert_eq!(child.scope.depth, 1);
    assert_eq!(child.scope.lane, ApplicationLane::Worker);
    assert!(child.elapsed_ms >= 0.);
    #[cfg(any(feature = "serde", feature = "mcp"))]
    let frozen = ended_second.to_json();
    resume_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(handle.read(first).unwrap().in_flight[0].phase, Phase::InputDispatch);
    parent.finish();
    drop(phase);
    let ended_first = handle.end(first).unwrap();
    let first_app = ended_first.application_scopes.unwrap();
    assert_eq!(first_app.completed_scopes, 2);
    assert_eq!(first_app.started_scopes, 2);
    assert!(first_app.in_flight.is_empty());
    assert_eq!(first_app.samples[0].scope.id, worker_id);
    assert_eq!(first_app.samples[1].scope.id, parent_id);
    assert!(first_app.samples[1].wall >= first_app.samples[0].wall);
    assert_eq!(second_app.completed_scopes, 0);
    assert!(second_app.samples.is_empty());
    #[cfg(any(feature = "serde", feature = "mcp"))]
    assert_eq!(frozen, ended_second.to_json());
    assert!(matches!(handle.read(second), Err(ProfileError::AlreadyEnded)));
  }

  #[test]
  fn idle_scope_is_observed_on_mid_scope_start_and_whole_completion_is_excluded() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let idle = handle.application_scope("main", "edit_prepare", ApplicationLane::Ui);
    assert_eq!(idle.status(), ApplicationScopeStatus::Active);
    let id = handle.start(Default::default()).unwrap().id;
    let during = handle.read(id).unwrap();
    let app = during.application_scopes.as_ref().unwrap();
    assert_eq!(app.started_scopes, 0);
    assert!(app.samples.is_empty());
    assert!(app.in_flight[0].started_before_session);
    let child = idle.child("core_reconcile", ApplicationLane::Ui);
    child.finish();
    idle.finish();
    let report = handle.end(id).unwrap();
    let app = report.application_scopes.unwrap();
    assert_eq!(app.completed_scopes, 1);
    assert_eq!(app.boundary_excluded_scopes, 1);
    assert_eq!(app.samples[0].scope.label, "core_reconcile");
    assert_eq!(app.samples[0].scope.depth, 1);
    assert!(app.samples[0].scope.parent_id.is_some());
    assert!(app.in_flight.is_empty());
  }

  #[test]
  fn worker_child_may_finish_after_parent_without_invented_containment() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let id = handle.start(Default::default()).unwrap().id;
    let parent = handle.application_scope("main", "surface_prepare", ApplicationLane::Ui);
    let token = parent.parent().unwrap();
    let child = parent.child("scene_build", ApplicationLane::Worker);
    let child_id = child.id().unwrap();
    parent.finish();
    assert_eq!(
      handle
        .application_scope_child(&token, "late_child", ApplicationLane::Worker)
        .status(),
      ApplicationScopeStatus::InvalidParent
    );
    let current = handle.read(id).unwrap();
    let app = current.application_scopes.as_ref().unwrap();
    assert_eq!(app.samples.len(), 1);
    assert_eq!(app.in_flight.len(), 1);
    assert_eq!(app.in_flight[0].scope.id, child_id);
    child.finish();
    let app = handle.end(id).unwrap().application_scopes.unwrap();
    assert_eq!(app.completed_scopes, 2);
    assert!(app.samples[1].completed_ms >= app.samples[0].completed_ms);
    assert_eq!(app.samples[1].scope.parent_id, Some(app.samples[0].scope.id));
    assert_eq!(app.refused.invalid_parent, 1);
  }

  #[test]
  fn bounded_history_and_live_refusals_are_separate_from_ui_samples() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let bounded = handle
      .start(SessionOptions {
        max_samples: 2,
        ..Default::default()
      })
      .unwrap()
      .id;
    let other = handle.start(Default::default()).unwrap().id;
    for _ in 0..5 {
      handle.application_scope("main", "paint", ApplicationLane::Ui).finish();
    }
    let scopes: Vec<_> = (0..MAX_APPLICATION_LIVE_SCOPES)
      .map(|_| handle.application_scope("main", "busy", ApplicationLane::Worker))
      .collect();
    assert!(
      scopes
        .iter()
        .all(|scope| scope.status() == ApplicationScopeStatus::Active)
    );
    assert_eq!(
      handle
        .application_scope("main", "overflow", ApplicationLane::Worker)
        .status(),
      ApplicationScopeStatus::LiveLimit
    );
    let app = handle.read(bounded).unwrap().application_scopes.unwrap();
    assert_eq!((app.completed_scopes, app.dropped_scopes, app.samples.len()), (5, 3, 2));
    assert_eq!(app.in_flight.len(), 64);
    assert_eq!(app.refused.live_limit, 1);
    assert!(app.samples[0].scope.id.0 < app.samples[1].scope.id.0);
    let finalized = handle.end(bounded).unwrap();
    drop(scopes);
    let other_report = handle.end(other).unwrap();
    assert!(other_report.samples.is_empty());
    assert_eq!(other_report.completed_samples, 0);
    let app = other_report.application_scopes.unwrap();
    assert_eq!(app.completed_scopes, 69);
    assert_eq!(app.dropped_scopes, 0);
    assert_eq!(finalized.application_scopes.unwrap().in_flight.len(), 64);
    let large = handle
      .start(SessionOptions {
        max_samples: 240,
        ..Default::default()
      })
      .unwrap()
      .id;
    for _ in 0..130 {
      handle.application_scope("main", "paint", ApplicationLane::Ui).finish();
    }
    let app = handle.end(large).unwrap().application_scopes.unwrap();
    assert_eq!((app.max_samples, app.samples.len(), app.dropped_scopes), (128, 128, 2));
  }

  #[test]
  fn window_closure_replacement_and_shutdown_abandon_without_late_samples() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    handle.register("w1", false);
    let first = handle.start(Default::default()).unwrap().id;
    let old = handle.application_scope("w1", "scene_cache_wait", ApplicationLane::Worker);
    let token = old.parent().unwrap();
    let second = handle.start(Default::default()).unwrap().id;
    handle.set_open("w1", false);
    let app = handle.read(second).unwrap().application_scopes.unwrap();
    assert_eq!(app.abandoned_window_closed, 1);
    assert_eq!(app.boundary_excluded_scopes, 1);
    assert!(app.samples.is_empty());
    assert!(app.in_flight.is_empty());
    handle.register("w1", false);
    assert_eq!(
      handle
        .application_scope_child(&token, "stale", ApplicationLane::Worker)
        .status(),
      ApplicationScopeStatus::InvalidParent
    );
    let replacement = handle.application_scope("w1", "scene_build", ApplicationLane::Worker);
    assert_ne!(replacement.id(), old.id());
    old.finish();
    replacement.finish();
    let root = handle.application_scope("main", "paint", ApplicationLane::Ui);
    let worker = handle.application_scope("w1", "store_save", ApplicationLane::Worker);
    drop(producer);
    assert_eq!(
      handle.application_scope("main", "closed", ApplicationLane::Ui).status(),
      ApplicationScopeStatus::CollectorClosed
    );
    root.finish();
    worker.finish();
    let app = handle.end(first).unwrap().application_scopes.unwrap();
    assert_eq!(app.completed_scopes, 1);
    assert_eq!(app.abandoned_window_closed, 2); // w1 close and main root drop.
    assert_eq!(app.abandoned_producer_closed, 1); // remaining worker window.
    assert_eq!(app.refused.collector_closed, 1);
    assert!(app.in_flight.is_empty());
    assert_eq!(
      handle.end(second).unwrap().application_scopes.unwrap().completed_scopes,
      1
    );
  }

  #[test]
  fn invalid_labels_and_foreign_parent_collisions_cannot_hide_refusals() {
    let handle = ProfilingHandle::new();
    handle.register("devtools", true);
    let local = handle.application_scope("devtools", "local", ApplicationLane::Ui);
    let alien = ProfilingHandle::new();
    alien.register("main", false);
    let foreign = alien.application_scope("main", "alien", ApplicationLane::Ui);
    assert_eq!(local.id(), foreign.id());
    let id = handle.start(Default::default()).unwrap().id;
    assert_eq!(
      handle
        .application_scope_child(&foreign.parent().unwrap(), "foreign", ApplicationLane::Worker)
        .status(),
      ApplicationScopeStatus::InvalidParent
    );
    assert_eq!(
      handle
        .application_scope("missing", "good", ApplicationLane::Ui)
        .status(),
      ApplicationScopeStatus::WindowUnavailable
    );
    for label in [
      "",
      "two words",
      "path/file",
      "非ASCII",
      "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
      assert_eq!(
        handle.application_scope("missing", label, ApplicationLane::Ui).status(),
        ApplicationScopeStatus::InvalidLabel
      );
    }
    let app = handle.end(id).unwrap().application_scopes.unwrap();
    assert_eq!(app.refused.invalid_parent, 1);
    assert_eq!(app.refused.window_unavailable, 1);
    assert_eq!(app.refused.invalid_label, 5);
    assert!(app.in_flight.is_empty());
    assert!(app.samples.is_empty());
  }

  #[test]
  fn devtools_opt_in_and_unrelated_collectors_remain_independent() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    handle.register("devtools", true);
    let public = handle.start(Default::default()).unwrap().id;
    let all = handle
      .start(SessionOptions {
        include_devtools: true,
        ..Default::default()
      })
      .unwrap()
      .id;
    let devtools = handle.application_scope("devtools", "paint", ApplicationLane::Ui);
    assert!(
      handle
        .read(public)
        .unwrap()
        .application_scopes
        .unwrap()
        .in_flight
        .is_empty()
    );
    assert_eq!(handle.read(all).unwrap().application_scopes.unwrap().in_flight.len(), 1);
    devtools.finish();
    handle.application_scope("main", "paint", ApplicationLane::Ui).finish();
    let other = WindowProfiler::new();
    let other_id = other.handle().start(Default::default()).unwrap().id;
    assert!(
      other
        .handle()
        .end(other_id)
        .unwrap()
        .application_scopes
        .unwrap()
        .samples
        .is_empty()
    );
    assert_eq!(
      handle.end(public).unwrap().application_scopes.unwrap().completed_scopes,
      1
    );
    assert_eq!(handle.end(all).unwrap().application_scopes.unwrap().completed_scopes, 2);
  }

  #[cfg(any(feature = "serde", feature = "mcp"))]
  #[test]
  fn newest_completion_age_is_independent_of_worker_publication_order() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let id = handle.start(Default::default()).unwrap().id;
    handle
      .application_scope("main", "first", ApplicationLane::Worker)
      .finish();
    // Distinct real completion timestamps, without asserting a speed threshold.
    std::thread::sleep(std::time::Duration::from_millis(1));
    handle
      .application_scope("main", "second", ApplicationLane::Worker)
      .finish();
    let mut report = handle.end(id).unwrap();
    let app = report.application_scopes.as_mut().unwrap();
    let newest = app.samples[1].completed_ms;
    assert!(newest > app.samples[0].completed_ms);
    let expected_age = (report.observed_ms - newest).max(0.);
    // A worker can timestamp completion first yet acquire the lock last. Reorder
    // collected records to exercise exactly that permitted publication order.
    app.samples.reverse();
    let json = report.to_json();
    assert_eq!(json["application_scopes"]["sample_age_ms"], expected_age);
    assert_eq!(json["application_scopes"]["samples"][1]["scope"]["label"], "first");
  }

  #[cfg(any(feature = "serde", feature = "mcp"))]
  #[test]
  fn app_only_export_has_own_age_bounds_and_wall_units() {
    let producer = WindowProfiler::new();
    let handle = producer.handle();
    let id = handle.start(Default::default()).unwrap().id;
    let scope = handle.application_scope("main", "local_save_write", ApplicationLane::Worker);
    let live = handle.read(id).unwrap().to_json();
    assert_eq!(live["status"], "unfinished_work_observed");
    assert_eq!(live["completed_samples"], 0);
    assert_eq!(live["application_scopes"]["in_flight"][0]["unfinished"], true);
    scope.finish();
    let completed = handle.end(id).unwrap().to_json();
    assert_eq!(completed["status"], "completed_samples");
    assert!(completed["samples"].as_array().unwrap().is_empty());
    assert!(completed["sample_age_ms"].is_null());
    let app = &completed["application_scopes"];
    assert_eq!(app["completed_scopes"], 1);
    assert_eq!(app["truncated"], false);
    assert!(app["sample_age_ms"].as_f64().unwrap() >= 0.);
    assert!(app["samples"][0]["wall_timings_ms"]["total"].as_f64().unwrap() >= 0.);
    assert_eq!(completed["build"]["application_scopes"]["available"], true);
    assert_eq!(completed["build"]["application_scopes"]["max_live_scopes"], 64);
    assert_eq!(
      completed["build"]["application_scopes"]["thread_cpu_time_available"],
      false
    );
  }
}
