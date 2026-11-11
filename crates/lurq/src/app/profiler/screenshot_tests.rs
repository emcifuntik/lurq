//! When a recording is due a screenshot, and what keeping one costs it.
use std::time::{Duration, Instant};

use super::{
  ProfilingHandle, RecordingOptions, ScreenshotOptions, SessionOptions, collector::screenshot_bytes,
  model::FrameScreenshot, producer::WindowProfiler,
};

fn with_screenshots(max_bytes: u64) -> RecordingOptions {
  RecordingOptions {
    max_bytes,
    screenshots: Some(ScreenshotOptions::default()),
    ..RecordingOptions::default()
  }
}

fn pixels(width: u32, height: u32) -> (u32, u32, Vec<u8>) {
  (width, height, vec![9; (width * height * 4) as usize])
}

#[test]
fn screenshots_are_due_at_most_four_times_a_second_by_default() {
  let producer = WindowProfiler::new();
  let handle = producer.handle();
  let start = Instant::now();
  // No session, or one without screenshots: never due.
  assert_eq!(handle.screenshot_due("main", start), None);
  let bounded = handle.start(SessionOptions::default()).unwrap().id;
  let plain = handle.start_recording(RecordingOptions::default()).unwrap().id;
  assert_eq!(handle.screenshot_due("main", start), None);

  let id = handle.start_recording(with_screenshots(1 << 20)).unwrap().id;
  let at = |ms: u64| start + Duration::from_millis(ms);
  assert_eq!(handle.screenshot_due("main", at(0)), Some(ScreenshotOptions::default()));
  assert_eq!(handle.screenshot_due("main", at(100)), None);
  assert_eq!(handle.screenshot_due("main", at(249)), None);
  assert!(handle.screenshot_due("main", at(250)).is_some());
  assert!(handle.screenshot_due("main", at(400)).is_none());
  assert!(handle.screenshot_due("main", at(500)).is_some());
  // A window the handle does not track has none.
  assert_eq!(handle.screenshot_due("w9", at(1000)), None);

  // Once the recording ends, no frame is read back.
  handle.end(id).unwrap();
  assert_eq!(handle.screenshot_due("main", at(2000)), None);
  handle.end(plain).unwrap();
  handle.end(bounded).unwrap();
}

#[test]
fn devtools_windows_have_screenshots_only_when_recorded() {
  let main = WindowProfiler::new();
  let mut devtools = WindowProfiler::new();
  devtools.attach(&main, "w1".into(), true);
  devtools.set_open(true);
  let handle = main.handle();
  let start = Instant::now();
  let id = handle.start_recording(with_screenshots(1 << 20)).unwrap().id;
  assert_eq!(handle.screenshot_due("w1", start), None);
  handle.end(id).unwrap();
  let id = handle
    .start_recording(RecordingOptions {
      include_devtools: true,
      ..with_screenshots(1 << 20)
    })
    .unwrap()
    .id;
  assert!(handle.screenshot_due("w1", start).is_some());
  handle.end(id).unwrap();
}

#[test]
fn screenshot_bytes_count_toward_the_ceiling() {
  let producer = WindowProfiler::new();
  let handle: ProfilingHandle = producer.handle();
  let id = handle.start_recording(with_screenshots(1 << 20)).unwrap().id;
  let requested = Instant::now();
  handle.screenshot_taken("main", 3, requested, pixels(16, 10));
  let status = handle.status(id).unwrap().recording.unwrap();
  let expected = screenshot_bytes(&FrameScreenshot {
    window: "main".into(),
    frame_id: 3,
    captured_ms: 0.0,
    width: 16,
    height: 10,
    rgba: vec![9; 16 * 10 * 4],
  });
  assert!(expected > 16 * 10 * 4);
  assert_eq!((status.screenshots, status.screenshot_bytes), (1, expected));
  assert_eq!(status.size_bytes, expected);
  let report = handle.end(id).unwrap();
  assert_eq!(report.screenshots[0].frame_id, 3);
  assert_eq!(report.screenshots[0].rgba.len(), 16 * 10 * 4);

  // A screenshot that would pass the ceiling stops the recording instead.
  let id = handle.start_recording(with_screenshots(expected * 2)).unwrap().id;
  let requested = Instant::now();
  handle.screenshot_taken("main", 4, requested, pixels(16, 10));
  handle.screenshot_taken("main", 5, requested, pixels(16, 10));
  handle.screenshot_taken("main", 6, requested, pixels(16, 10));
  let report = handle.read(id).unwrap();
  let status = report.recording.unwrap();
  assert!(status.stopped_at_limit);
  assert_eq!(status.screenshots, 2);
  assert!(status.size_bytes <= expected * 2);
  assert_eq!(
    report
      .screenshots
      .iter()
      .map(|screenshot| screenshot.frame_id)
      .collect::<Vec<_>>(),
    vec![4, 5]
  );
  // Stopped, it asks for no more.
  assert_eq!(handle.screenshot_due("main", Instant::now()), None);
  handle.end(id).unwrap();

  // A frame asked for before the recording began is not part of it.
  let id = handle.start_recording(with_screenshots(1 << 20)).unwrap().id;
  handle.screenshot_taken("main", 7, requested - Duration::from_secs(1), pixels(4, 4));
  assert!(handle.end(id).unwrap().screenshots.is_empty());
}

#[cfg(any(feature = "serde", feature = "mcp"))]
#[test]
fn the_export_lists_screenshots_and_marked_passes_without_pixels() {
  let producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start_recording(with_screenshots(1 << 20)).unwrap().id;
  handle.screenshot_taken("main", 3, Instant::now(), pixels(16, 10));
  let json = handle.end(id).unwrap().to_json();
  assert_eq!(json["schema_version"], 1);
  let listed = &json["recording"]["screenshots"][0];
  assert_eq!(listed["frame_id"], 3);
  assert_eq!(
    (listed["width"].as_u64(), listed["height"].as_u64()),
    (Some(16), Some(10))
  );
  assert!(listed.get("rgba").is_none());
  assert!(json["recording"]["screenshot_bytes"].as_u64().unwrap() > 0);
}
