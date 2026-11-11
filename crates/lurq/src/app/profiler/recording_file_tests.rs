use std::{sync::Arc, time::Duration};

use serde_json::Value;

use super::{firefox_export_tests::full_report, *};

/// `full_report` with every optional part of a report filled in.
fn every_field_report() -> ProfileReport {
  let mut report = full_report();
  report.windows = vec![
    WindowStatus {
      id: "main".into(),
      open: true,
      devtools: false,
      gpu_timestamps: Some(GpuTimestampAvailability {
        available: true,
        reason: None,
        adapter_backend: Some("vulkan"),
      }),
    },
    WindowStatus {
      id: "w1".into(),
      open: false,
      devtools: true,
      gpu_timestamps: Some(GpuTimestampAvailability::unavailable(
        GpuTimestampAvailability::NATIVE_DX12,
      )),
    },
  ];
  report.in_flight = vec![InFlightObservation {
    window: "main".into(),
    frame_id: Some(9),
    phase: Phase::RenderPresent,
    started_ms: 390.5,
    elapsed_ms: 9.5,
    phase_elapsed_ms: 1.25,
    started_before_session: false,
  }];
  report.recording = Some(RecordingStatus {
    max_bytes: DEFAULT_RECORDING_MAX_BYTES,
    size_bytes: 123_456,
    stopped_at_limit: true,
    stopped_ms: Some(399.0),
    screenshots: 1,
    screenshot_bytes: 200,
  });
  report.screenshots = vec![Arc::new(FrameScreenshot {
    window: "main".into(),
    frame_id: 4,
    captured_ms: 101.5,
    width: 2,
    height: 1,
    rgba: vec![0, 15, 16, 255, 128, 64, 32, 1],
  })];
  report.boundary_excluded_samples = 2;
  let first = Arc::make_mut(&mut report.samples[0]);
  let SampleData::Pass(pass) = &mut first.data else {
    unreachable!("the first sample is a pass")
  };
  pass.backend = "dx12";
  pass.dropped_spans = 3;
  pass.screenshot = true;
  pass.gpu = Some(GpuTiming {
    ui: Duration::from_nanos(1_500_001),
    canvas: Some(Duration::from_nanos(250)),
  });
  pass.canvas_text = Some(CanvasTextProfile {
    shape_calls: 4,
    produced_bitmap_bytes: 1 << 20,
    total: Duration::from_nanos(1_234_567),
    glyph_prepare: Duration::from_nanos(7),
    ..CanvasTextProfile::default()
  });
  pass.reasons = crate::app::PassReasons {
    timer_run: true,
    layout_dirty: true,
    ..Default::default()
  };
  pass.present = Some(PresentTiming {
    presented_ms: 109.75,
    interval: Some(Duration::from_micros(16_667)),
    refresh_interval: Some(Duration::from_micros(16_666)),
    continuous: true,
    missed_vsyncs: 2,
  });
  pass.counters = Some(PassCounters {
    nodes: 321,
    memory_bytes: 8192,
    caches: CacheSizes {
      glyph_atlas_bytes: 1 << 22,
      canvas_asset_cache_bytes: 77,
      ..CacheSizes::default()
    },
    allocations: Some(PassAllocations {
      allocations: 10,
      frees: 9,
      allocated_bytes: 1000,
      freed_bytes: 900,
      live_bytes: 1 << 30,
    }),
  });
  let frame = pass.frame.as_mut().unwrap();
  frame.quad_count = 17;
  frame.glyph_engine.rich_cosmic_shape = Duration::from_nanos(999);
  frame.render.present = Duration::from_micros(42);
  frame.render.draw_calls = 23;
  frame.render.canvas.asset_cache_bytes = 4321;
  frame.render.canvas.asset_upload_details = Some(CanvasAssetUploadProfile {
    pixel_packing: Duration::from_nanos(11),
    cache_entries_after: 5,
    ..CanvasAssetUploadProfile::default()
  });
  let scopes = report.application_scopes.as_mut().unwrap();
  scopes.dropped_scopes = 1;
  scopes.refused.live_limit = 2;
  scopes.in_flight = vec![ApplicationInFlight {
    scope: scopes.samples[0].scope.clone(),
    elapsed_ms: 3.5,
    started_before_session: true,
  }];
  report
}

fn pass_of(report: &ProfileReport, index: usize) -> &PassSample {
  match &report.samples[index].data {
    SampleData::Pass(pass) => pass,
    _ => panic!("sample {index} is not a pass"),
  }
}

#[test]
fn a_recording_file_reads_back_into_the_same_report() {
  let report = every_field_report();
  let file = report.to_recording_file();
  let loaded = ProfileReport::from_recording_file(&file).expect("the file it wrote");
  // Every field written is read back: the loaded report writes the same file.
  assert_eq!(loaded.to_recording_file(), file);

  assert_eq!(loaded.samples.len(), report.samples.len());
  assert_eq!(loaded.recording, report.recording);
  assert_eq!(loaded.in_flight[0].phase, Phase::RenderPresent);
  let pass = pass_of(&loaded, 0);
  assert_eq!(pass.backend, "dx12");
  assert_eq!(pass.spans, pass_of(&report, 0).spans);
  assert_eq!(pass.canvas_text.unwrap().total, Duration::from_nanos(1_234_567));
  assert_eq!(pass.reasons, pass_of(&report, 0).reasons);
  assert_eq!(pass.present, pass_of(&report, 0).present);
  assert_eq!(pass.counters, pass_of(&report, 0).counters);
  assert!(pass.screenshot);
  assert_eq!(pass.gpu, pass_of(&report, 0).gpu);
  assert_eq!(loaded.windows[0].gpu_timestamps, report.windows[0].gpu_timestamps);
  assert_eq!(loaded.windows[1].gpu_timestamps, report.windows[1].gpu_timestamps);
  let [screenshot] = &loaded.screenshots[..] else {
    panic!("{} screenshots", loaded.screenshots.len());
  };
  assert_eq!((screenshot.frame_id, screenshot.width, screenshot.height), (4, 2, 1));
  assert_eq!(screenshot.rgba, report.screenshots[0].rgba);
  let frame = pass.frame.as_ref().unwrap();
  assert_eq!(frame.memory.total_bytes, 4096);
  assert_eq!(frame.glyph_engine.rich_cosmic_shape, Duration::from_nanos(999));
  assert_eq!(frame.render.draw_calls, 23);
  assert_eq!(frame.render.canvas.asset_cache_bytes, 4321);
  match &loaded.samples[1].data {
    SampleData::InputDispatch(input) => {
      assert_eq!(input.interaction_id, 1);
      assert_eq!(input.queue_delay, Duration::from_micros(500));
      assert_eq!(input.presented_frame_id, Some(4));
      assert_eq!(input.input_to_present, Some(Duration::from_millis(90)));
    }
    _ => panic!("the second sample is an input dispatch"),
  }
  assert_eq!(
    frame.render.canvas.asset_upload_details,
    pass_of(&report, 0)
      .frame
      .as_ref()
      .unwrap()
      .render
      .canvas
      .asset_upload_details
  );
  let scopes = loaded.application_scopes.as_ref().unwrap();
  let labels: Vec<_> = scopes.samples.iter().map(|sample| sample.scope.label).collect();
  assert_eq!(labels, vec!["load_document", "parse", "decode_image", "decode_image"]);
  assert_eq!(scopes.samples[1].scope.parent_id, Some(ApplicationScopeId(1)));
  assert_eq!(scopes.samples[2].scope.lane, ApplicationLane::Worker);
  assert!(scopes.in_flight[0].started_before_session);
  match &loaded.samples[2].data {
    SampleData::UiUpdate(update) => {
      assert_eq!(update.kind, UiUpdateKind::SubtreeRefresh);
      assert_eq!(update.commit, Duration::from_micros(250));
    }
    _ => panic!("the third sample is a UI update"),
  }
}

#[test]
fn a_recording_file_keeps_every_time_exactly() {
  let mut report = full_report();
  // Values whose shortest decimal form a lax float parser can miss by a bit.
  let awkward = [
    0.1 + 0.2,
    1.0 / 3.0,
    2.0_f64.sqrt() * 1e3,
    123_456.789_012_345_67,
    5e-324,
  ];
  for (sample, value) in report.samples.iter_mut().zip(awkward) {
    Arc::make_mut(sample).started_ms = value;
  }
  report.observed_ms = 9_007_199_254.740_993;
  let loaded = ProfileReport::from_recording_file(&report.to_recording_file()).unwrap();
  for (sample, value) in loaded.samples.iter().zip(awkward) {
    assert_eq!(sample.started_ms.to_bits(), value.to_bits(), "{value}");
  }
  assert_eq!(loaded.observed_ms.to_bits(), report.observed_ms.to_bits());
}

#[test]
fn files_that_are_not_recordings_are_refused_with_the_reason() {
  let refused = |file: &str| {
    ProfileReport::from_recording_file(file)
      .err()
      .expect("refused")
      .to_string()
  };
  let file: Value = serde_json::from_str(&full_report().to_recording_file()).unwrap();
  let with = |edit: &dyn Fn(&mut Value)| {
    let mut file = file.clone();
    edit(&mut file);
    refused(&file.to_string())
  };
  let error = refused("{");
  assert!(error.starts_with("not a lurq recording"), "{error}");
  // A Firefox profile is for the Firefox Profiler, not for Load.
  let error = refused(&full_report().to_firefox_profile());
  assert!(error.contains("format is not lurq-recording"), "{error}");
  let error = with(&|file| file["format_version"] = Value::from(2));
  assert!(error.contains("format_version 2"), "{error}");
  // The path to a bad value is named.
  let error = with(&|file| {
    file["report"]["samples"][0]["data"]["pass"]
      .as_object_mut()
      .unwrap()
      .remove("total");
  });
  assert!(
    error.contains("report: samples: 0: data: pass: total: expected nanoseconds"),
    "{error}"
  );
  // Names are names, never text or paths.
  for text in ["Quarterly report.pdf", "C:/Users/me", "..\\secrets", ""] {
    let error = with(&|file| file["report"]["application_scopes"]["samples"][0]["scope"]["label"] = Value::from(text));
    assert!(error.contains("label: expected a name"), "{text:?}: {error}");
  }
  let error = with(&|file| file["report"]["samples"][0]["data"]["pass"]["spans"][1]["phase"] = Value::from("paint"));
  assert!(error.contains("unknown Phase \"paint\""), "{error}");
}

#[test]
fn a_custom_backend_name_saves_in_a_form_that_loads() {
  let long = "x".repeat(MAX_APPLICATION_LABEL_BYTES + 10).leak();
  for (backend, saved) in [
    ("my-engine", "my-engine"),
    ("vulkan.1_3", "vulkan.1_3"),
    ("my engine/v2", "my_engine_v2"),
    ("moteur-é", "moteur-_"),
    ("", "_"),
    (&*long, &long[..MAX_APPLICATION_LABEL_BYTES]),
  ] {
    let mut report = full_report();
    let SampleData::Pass(pass) = &mut Arc::make_mut(&mut report.samples[0]).data else {
      unreachable!("the first sample is a pass")
    };
    pass.backend = backend;
    let loaded = ProfileReport::from_recording_file(&report.to_recording_file())
      .unwrap_or_else(|error| panic!("{backend:?}: {error}"));
    assert_eq!(pass_of(&loaded, 0).backend, saved, "{backend:?}");
  }
}
