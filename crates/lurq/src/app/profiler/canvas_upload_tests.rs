use super::*;

#[test]
fn profiling_canvas_upload_idle_and_disabled_capture_are_absent() {
  assert!(CanvasAssetUploadProfile::capture(false, 65536, 1).is_none());
  #[cfg(not(feature = "perf_profile"))]
  assert!(CanvasAssetUploadProfile::capture(true, 65536, 1).is_none());
  #[cfg(feature = "perf_profile")]
  assert!(CanvasAssetUploadProfile::start_timer(None).is_none());
}

#[cfg(feature = "perf_profile")]
mod enabled {
  use std::time::Duration;

  use super::{super::producer::WindowProfiler, *};
  use crate::app::{PassReport, profile_types::canvas_upload::AssetUploadStage};

  #[test]
  fn profiling_canvas_upload_groups_accumulate_and_eviction_preserves_peak_charge() {
    let mut details = CanvasAssetUploadProfile::capture(true, 65536, 1).unwrap();
    // Two groups: one reuses its asset, then a new upload crosses the cache budget.
    details.cache_hits += 2;
    details.descriptor_pairs += 1;
    details.add_duration(AssetUploadStage::DescriptorWrites, Duration::from_millis(2));
    details.cache_misses += 1;
    details.texture_creations += 1;
    details.descriptor_pairs += 1;
    details.add_duration(AssetUploadStage::TextureCreation, Duration::from_millis(3));
    details.add_duration(AssetUploadStage::PixelPacking, Duration::from_millis(4));
    details.add_duration(AssetUploadStage::UploadStagingCommands, Duration::from_millis(5));
    details.add_duration(AssetUploadStage::DescriptorWrites, Duration::from_millis(1));
    details.uploaded(256 * 3, false);
    details.uploaded(512 * 2, true);
    details.cache_state(65 * 1024 * 1024, 1025);
    details.cache_evictions += 2;
    details.add_duration(AssetUploadStage::CacheEviction, Duration::from_millis(7));
    details.cache_state(63 * 1024 * 1024, 1023);
    assert_eq!(details.descriptor_writes, Duration::from_millis(3));
    assert_eq!(details.padded_upload_bytes, 1792);
    assert_eq!((details.arena_uploads, details.dedicated_uploads), (1, 1));
    assert_eq!(details.cache_charged_bytes_before, 65536);
    assert_eq!(details.cache_charged_bytes_peak, 65 * 1024 * 1024);
    assert_eq!(details.cache_charged_bytes_after, 63 * 1024 * 1024);
    assert_eq!((details.cache_entries_before, details.cache_entries_after), (1, 1023));
    #[cfg(any(feature = "serde", feature = "mcp"))]
    {
      let json = details.to_json();
      assert_eq!(json["counts"]["cache_hits"], 2);
      assert_eq!(json["counts"]["cache_misses"], 1);
      assert_eq!(json["counts"]["descriptor_pairs"], 2);
      assert_eq!(json["cpu_timings_ms"]["cache_eviction"], 7.);
      assert_eq!(json["cpu_timings_ms"]["texture_creation"], 3.);
      assert_eq!(json["cpu_timings_ms"]["pixel_packing"], 4.);
      assert_eq!(json["cpu_timings_ms"]["upload_staging_commands"], 5.);
    }
  }

  fn finish(producer: &mut WindowProfiler, start: std::time::Instant, frame: &FrameProfile, backend: &'static str) {
    producer.finish_pass(
      start,
      &PassReport {
        required: true,
        rendered: true,
        ..Default::default()
      },
      backend,
      Some(frame),
    );
  }

  #[test]
  fn profiling_canvas_upload_nested_windows_and_ended_sessions_keep_detached_frame_details() {
    let mut root = WindowProfiler::new();
    let mut child = WindowProfiler::new();
    child.attach(&root, "w1".into(), false);
    let handle = root.handle();
    let first = handle.start(Default::default()).unwrap().id;
    let second = handle.start(Default::default()).unwrap().id;
    let mut frame = FrameProfile {
      render_profile_available: true,
      ..Default::default()
    };
    frame.render.canvas.asset_upload_details = CanvasAssetUploadProfile::capture(true, 65536, 1);
    frame.render.canvas.asset_upload_details.as_mut().unwrap().cache_hits = 9;
    frame.render.canvas.asset_cache_stretch_bytes = 5;
    frame.render.canvas.asset_cache_uncached = 2;
    let (root_start, root_phase) = root.begin_pass(1);
    let (child_start, child_phase) = child.begin_pass(2);
    finish(&mut child, child_start, &frame, "dx12");
    drop(child_phase);
    let ended_second = handle.end(second).unwrap();
    assert_eq!(ended_second.samples.len(), 1);
    assert_eq!(ended_second.samples[0].window, "w1");
    assert_eq!(ended_second.in_flight[0].window, "main");
    frame.render.canvas.asset_upload_details.as_mut().unwrap().cache_hits = 27;
    finish(&mut root, root_start, &frame, "dx12");
    drop(root_phase);
    let ended_first = handle.end(first).unwrap();
    let hits = |report: &ProfileReport| {
      report
        .samples
        .iter()
        .map(|sample| {
          let SampleData::Pass(pass) = &sample.data else {
            panic!("pass required")
          };
          pass
            .frame
            .as_ref()
            .unwrap()
            .render
            .canvas
            .asset_upload_details
            .unwrap()
            .cache_hits
        })
        .collect::<Vec<_>>()
    };
    assert_eq!(hits(&ended_first), [9, 27]);
    assert_eq!(hits(&ended_second), [9]);
    assert!(ended_first.in_flight.is_empty());
    #[cfg(any(feature = "serde", feature = "mcp"))]
    {
      let exported = ended_first.to_json();
      let canvas = &exported["samples"][0]["data"]["frame"]["render"]["canvas"];
      assert_eq!(canvas["counts"]["asset_cache_stretch_bytes"], 5);
      assert_eq!(canvas["counts"]["asset_cache_uncached"], 2);
      let details = &canvas["asset_upload_details"];
      if cfg!(all(feature = "canvas", feature = "dx12", target_os = "windows")) {
        assert_eq!(details["counts"]["cache_hits"], 9);
      } else {
        assert!(details.is_null());
      }
    }
  }
}

#[cfg(any(feature = "serde", feature = "mcp"))]
#[test]
fn profiling_canvas_upload_export_does_not_advertise_wgpu_or_custom_backend_support() {
  // Deliberately injected data must not turn unsupported backend coverage true.
  use std::{sync::Arc, time::Duration};
  let mut frame = FrameProfile {
    render_profile_available: true,
    ..Default::default()
  };
  frame.render.canvas.asset_upload_details = Some(CanvasAssetUploadProfile::default());
  for backend in ["wgpu", "custom", "dx12"] {
    let mut report = ProfileReport {
      application_scopes: None,
      id: SessionId(1),
      finalized: true,
      started_ms: 0.,
      ended_ms: Some(1.),
      observed_ms: 1.,
      build: Default::default(),
      max_samples: 1,
      completed_samples: 1,
      dropped_samples: 0,
      boundary_excluded_samples: 0,
      untracked_windows: 0,
      windows: vec![],
      in_flight: vec![],
      samples: vec![Arc::new(ProfileSample {
        sequence: 1,
        window: "main".into(),
        started_ms: 0.,
        completed_ms: 1.,
        data: SampleData::Pass(PassSample {
          frame_id: Some(1),
          rendered: true,
          cached_render_list: false,
          total: Duration::ZERO,
          layout_update: Duration::ZERO,
          layout_compute: Duration::ZERO,
          component_after_layout: Duration::ZERO,
          layout_recalculated: false,
          canvas_recording: Duration::ZERO,
          canvas_preparation: Duration::ZERO,
          canvas_text: None,
          backend,
          frame: Some(frame.clone()),
          spans: Vec::new(),
          dropped_spans: 0,
          counters: None,
          gpu: None,
        }),
      })],
      recording: None,
    };
    let supported = backend == "dx12"
      && cfg!(all(
        feature = "canvas",
        feature = "perf_profile",
        feature = "dx12",
        target_os = "windows"
      ));
    let render = &report.to_json()["samples"][0]["data"]["frame"]["render"];
    assert_eq!(render["coverage"]["canvas_asset_upload_details"], supported);
    assert_eq!(render["canvas"]["asset_upload_details"].is_null(), !supported);
    let SampleData::Pass(pass) = &mut Arc::make_mut(&mut report.samples[0]).data else {
      unreachable!()
    };
    pass.frame.as_mut().unwrap().render_profile_available = false;
    let unavailable = report.to_json();
    assert_eq!(
      unavailable["samples"][0]["data"]["frame"]["render"]["coverage"]["canvas_asset_upload_details"],
      false
    );
    assert!(unavailable["samples"][0]["data"]["frame"]["render"]["canvas"]["asset_upload_details"].is_null());
  }
}
