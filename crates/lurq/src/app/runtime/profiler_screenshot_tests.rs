//! A recording's screenshots of presented frames, taken through a tree's
//! passes: only with the option on, at most at their rate, each marking the
//! pass that paid for it, with sensitive text painted over.

use std::sync::{
  Arc, Mutex,
  atomic::{AtomicUsize, Ordering},
};

use raw_window_handle::{DisplayHandle, WindowHandle};

use super::Tree;
use crate::{
  app::{
    App,
    capture_redaction::REDACTION_FILL,
    capture_test_engine::{GLYPH_PAINT, GlyphPainter, ScreenFrame, TestSurface},
    profiler::{
      FrameScreenshot, PassSample, ProfileReport, ProfilingHandle, RecordingOptions, SampleData, ScreenshotOptions,
      SessionOptions,
    },
    render_engine::{RenderEngine, RenderFrameCapture},
  },
  components::{Column, Text},
  layout::render_list::RenderList,
};

const WIDTH: u32 = 300;
const HEIGHT: u32 = 120;

/// The capture engine of the redaction tests, counting the frames it was
/// asked to read back.
struct Counting {
  inner: Box<dyn RenderEngine>,
  captures: Arc<AtomicUsize>,
}

impl RenderEngine for Counting {
  fn resize(&mut self, width: u32, height: u32) {
    self.inner.resize(width, height);
  }

  fn render(&mut self, list: &RenderList, window: WindowHandle<'_>, display: DisplayHandle<'_>) -> bool {
    self.inner.render(list, window, display)
  }

  fn supports_frame_capture(&self) -> bool {
    true
  }

  fn render_with_capture(
    &mut self,
    list: &RenderList,
    window: WindowHandle<'_>,
    display: DisplayHandle<'_>,
    capture: Option<RenderFrameCapture>,
  ) -> bool {
    if capture.is_some() {
      self.captures.fetch_add(1, Ordering::SeqCst);
    }
    self.inner.render_with_capture(list, window, display, capture)
  }
}

struct Fixture {
  tree: Tree,
  app: App,
  captures: Arc<AtomicUsize>,
  screen: Arc<Mutex<ScreenFrame>>,
}

impl Fixture {
  /// A plain label over a sensitive code, drawn by the counting engine.
  fn new() -> Self {
    let (factory, screen) = GlyphPainter::factory();
    let captures = Arc::new(AtomicUsize::new(0));
    let counted = captures.clone();
    let mut tree = Tree::new();
    tree.set_render_engine_factory(move || {
      Box::new(Counting {
        inner: factory(),
        captures: counted.clone(),
      }) as Box<dyn RenderEngine>
    });
    tree.resize(WIDTH, HEIGHT);
    tree.set_root(
      Column::new()
        .spacing(24.0)
        .child(Text::new("Plain label"))
        .child(Text::new("493117").sensitive()),
    );
    let mut app = App::new();
    tree.pass(&mut app, &TestSurface);
    Self {
      tree,
      app,
      captures,
      screen,
    }
  }

  fn handle(&self) -> ProfilingHandle {
    self.tree.profiling_handle()
  }

  /// `count` presented frames.
  fn present(&mut self, count: usize) {
    for _ in 0..count {
      self.tree.request_redraw();
      assert!(self.tree.pass(&mut self.app, &TestSurface).rendered);
    }
  }
}

fn presented_passes(report: &ProfileReport) -> Vec<&PassSample> {
  report
    .samples
    .iter()
    .filter_map(|sample| match &sample.data {
      SampleData::Pass(pass) if pass.rendered => Some(pass),
      _ => None,
    })
    .collect()
}

fn recording_with(handle: &ProfilingHandle, screenshots: ScreenshotOptions) -> crate::app::profiler::SessionId {
  handle
    .start_recording(RecordingOptions {
      screenshots: Some(screenshots),
      ..RecordingOptions::default()
    })
    .unwrap()
    .id
}

#[test]
fn without_the_option_no_frame_is_read_back() {
  let mut fixture = Fixture::new();
  let handle = fixture.handle();
  let recording = handle.start_recording(RecordingOptions::default()).unwrap().id;
  let bounded = handle.start(SessionOptions::default()).unwrap().id;
  fixture.present(4);
  let report = handle.end(recording).unwrap();
  handle.end(bounded).unwrap();

  assert_eq!(fixture.captures.load(Ordering::SeqCst), 0);
  assert!(report.screenshots.is_empty());
  let passes = presented_passes(&report);
  assert_eq!(passes.len(), 4);
  assert!(passes.iter().all(|pass| !pass.screenshot));
  assert_eq!(report.recording.unwrap().screenshot_bytes, 0);
}

#[test]
fn a_screenshot_is_taken_at_most_at_its_rate_and_marks_its_frame() {
  let mut fixture = Fixture::new();
  let handle = fixture.handle();
  // One a second: the frames right after the first take none.
  let recording = recording_with(
    &handle,
    ScreenshotOptions {
      max_per_second: 1,
      max_width: WIDTH / 2,
      max_height: HEIGHT,
    },
  );
  fixture.present(4);
  let report = handle.end(recording).unwrap();

  assert_eq!(fixture.captures.load(Ordering::SeqCst), 1);
  let [screenshot] = &report.screenshots[..] else {
    panic!("{} screenshots", report.screenshots.len());
  };
  let marked: Vec<_> = presented_passes(&report)
    .into_iter()
    .filter(|pass| pass.screenshot)
    .map(|pass| pass.frame_id)
    .collect();
  // The frame that paid for it, the first, is marked; its screenshot names it.
  assert_eq!(marked, vec![Some(screenshot.frame_id)]);
  assert_eq!(presented_passes(&report)[0].frame_id, Some(screenshot.frame_id));
  assert_eq!(screenshot.window, "main");
  // Downscaled to fit, aspect kept.
  assert_eq!((screenshot.width, screenshot.height), (WIDTH / 2, HEIGHT / 2));
  assert_eq!(screenshot.rgba.len(), (WIDTH / 2 * HEIGHT / 2 * 4) as usize);
  let status = report.recording.unwrap();
  assert_eq!(status.screenshots, 1);
  assert!(status.screenshot_bytes >= screenshot.rgba.len() as u64);
  assert!(status.size_bytes > status.screenshot_bytes);
}

#[test]
fn a_screenshot_paints_over_sensitive_text_as_an_inspector_capture_does() {
  let mut fixture = Fixture::new();
  let handle = fixture.handle();
  let recording = recording_with(
    &handle,
    ScreenshotOptions {
      max_width: WIDTH,
      max_height: HEIGHT,
      ..ScreenshotOptions::default()
    },
  );
  fixture.present(1);
  let report = handle.end(recording).unwrap();

  let [screenshot] = &report.screenshots[..] else {
    panic!("{} screenshots", report.screenshots.len());
  };
  let screenshot: &FrameScreenshot = screenshot;
  assert_eq!((screenshot.width, screenshot.height), (WIDTH, HEIGHT));
  let pixel = |x: u32, y: u32| -> [u8; 4] {
    let index = (y * screenshot.width + x) as usize * 4;
    screenshot.rgba[index..index + 4].try_into().unwrap()
  };
  let [area] = fixture.tree.capture_redactions[..] else {
    panic!("one sensitive run: {:?}", fixture.tree.capture_redactions);
  };
  let screen = fixture.screen.lock().unwrap().clone();
  let mut shown = 0;
  for y in area.y0..area.y1.min(HEIGHT) {
    for x in area.x0..area.x1.min(WIDTH) {
      assert_eq!(pixel(x, y), REDACTION_FILL, "pixel {x},{y} of the code");
      shown += usize::from(screen.pixel(x, y) == GLYPH_PAINT);
    }
  }
  assert!(shown > 0, "the window shows the code");
  // The plain label above is captured as painted.
  let plain = (0..area.y0)
    .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
    .filter(|&(x, y)| pixel(x, y) == GLYPH_PAINT)
    .count();
  assert!(plain > 0);
}
