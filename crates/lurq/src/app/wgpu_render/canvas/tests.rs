use std::time::{Duration, Instant};

use super::*;
use crate::{
  canvas::{ArcDirection, CanvasFont, FillRule, Path2D},
  images::ImageData,
  node::transform::Transform2D,
};

pub(super) fn device() -> (Device, Queue) {
  let instance = Instance::default();
  let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
    .expect("GPU adapter required for canvas tests");
  eprintln!("canvas GPU: {:?}", adapter.get_info());
  pollster::block_on(adapter.request_device(&DeviceDescriptor {
    label: Some("canvas tests"),
    ..Default::default()
  }))
  .unwrap()
}
fn take(canvas: &CanvasHandle, renderer: &mut Renderer, device: &Device, queue: &Queue) -> CanvasSnapshot {
  let readback = canvas.snapshot();
  renderer.process(device, queue, std::slice::from_ref(canvas));
  readback
    .wait_timeout(Duration::from_secs(15))
    .expect("readback completed")
    .unwrap()
}
fn pixel(snapshot: &CanvasSnapshot, x: u32, y: u32) -> [u8; 4] {
  snapshot.rgba[((y * snapshot.width + x) * 4) as usize..((y * snapshot.width + x) * 4 + 4) as usize]
    .try_into()
    .unwrap()
}
#[test]
#[ignore = "requires a GPU adapter; run explicitly and separately from context-creation tests"]
fn gpu_canvas_pixels_ordering_clips_resize_and_asset_reuse() {
  let (device, queue) = device();
  let mut renderer = Renderer::new(&device, &queue);
  let gpu = CanvasHandle::test_surface(1024, 600, 1., false);
  renderer.process(&device, &queue, std::slice::from_ref(&gpu));
  assert_eq!(gpu.status().gpu_bytes, 0, "blank canvases allocate lazily");
  let cpu = CanvasHandle::test_surface(1024, 600, 1., true);
  let image = ImageData::from_rgba(
    vec![255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 128, 255, 255, 0, 255],
    2,
    2,
  );
  for canvas in [&gpu, &cpu] {
    let d = canvas.context_2d();
    d.set_fill_style("#182434");
    d.fill_rect(0., 0., 1024., 600.);
    d.set_fill_style("#f0444480");
    d.fill_rect(450., 30., 180., 180.);
    d.clear_rect(490., 50., 40., 20.);
    d.save();
    d.begin_path();
    d.rect(410., 10., 250., 250.);
    d.clip();
    d.begin_path();
    d.arc(512., 150., 100., 0., std::f32::consts::TAU, ArcDirection::Clockwise)
      .unwrap();
    d.clip();
    d.set_fill_style("#34d399");
    d.fill_rect(400., 100., 260., 160.);
    d.restore();
    let mut path = Path2D::new();
    path.rect(20., 20., 120., 120.);
    path.rect(40., 40., 80., 80.);
    d.set_fill_style("#ffffff");
    d.fill_path(&path, FillRule::EvenOdd);
    d.set_stroke_style("#ffa500");
    d.set_line_width(3.);
    d.set_line_dash(&[9., 3.]);
    d.begin_path();
    d.move_to(380., 300.);
    d.bezier_curve_to(420., 200., 620., 500., 700., 300.);
    d.stroke();
    d.save();
    d.set_transform(Transform2D::translate(200., 350.).then(&Transform2D::rotate(0.2)));
    d.draw_image_scaled(&image, 0., 0., 180., 120.).unwrap();
    d.restore();
    d.set_fill_style("#fff");
    d.set_font(CanvasFont::new("sans-serif", 20.));
    d.fill_text("GPU canvas", 480., 500.).unwrap();
  }
  let actual = take(&gpu, &mut renderer, &device, &queue);
  let expected = cpu.snapshot().try_take().unwrap().unwrap();
  let mut different = 0;
  let mut error = 0u64;
  for (a, b) in actual.rgba.chunks_exact(4).zip(expected.rgba.chunks_exact(4)) {
    let max = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    if max > 16 {
      different += 1;
    }
    error += a.iter().zip(b).map(|(a, b)| u64::from(a.abs_diff(*b))).sum::<u64>();
  }
  assert!(
    different < 1024 * 600 / 100,
    "AA-tolerant pixel parity: {different} pixels differ, mean error {}",
    error as f64 / actual.rgba.len() as f64
  );
  assert_eq!(pixel(&actual, 500, 60), [0, 0, 0, 0]);
  assert_eq!(gpu.status().pending_bytes, 0);
  assert_eq!(gpu.status().gpu_bytes, 1024 * 600 * 4);
  let before = gpu.status().gpu;
  let d = gpu.context_2d();
  d.draw_image_scaled(&image, 0., 0., 20., 20.).unwrap();
  renderer.process(&device, &queue, std::slice::from_ref(&gpu));
  assert_eq!(gpu.status().gpu.uploaded_bytes, before.uploaded_bytes);
  // Snapshot barriers survive a later full clear. The result captures its own revision.
  d.clear();
  d.set_fill_style("#ff000080");
  d.fill_rect(0., 0., 1024., 600.);
  let red = gpu.snapshot();
  d.clear();
  d.set_fill_style("#0000ff");
  d.fill_rect(0., 0., 1024., 600.);
  let blue = gpu.snapshot();
  assert_eq!(gpu.snapshot().try_take().unwrap().unwrap_err(), CanvasError::QueueFull);
  renderer.process(&device, &queue, std::slice::from_ref(&gpu));
  assert_eq!(
    pixel(&red.wait_timeout(Duration::from_secs(15)).unwrap().unwrap(), 100, 100),
    [255, 0, 0, 128]
  );
  assert_eq!(
    pixel(&blue.wait_timeout(Duration::from_secs(15)).unwrap().unwrap(), 100, 100),
    [0, 0, 255, 255]
  );
  let before = gpu.status().gpu;
  d.set_fill_style("#fff");
  d.fill_rect(20., 20., 64., 64.);
  renderer.process(&device, &queue, std::slice::from_ref(&gpu));
  assert_eq!(gpu.status().gpu.tiles - before.tiles, 1);
  assert_eq!(gpu.status().gpu.uploaded_bytes, before.uploaded_bytes);
  gpu.test_resize(2048, 1200, true);
  let scaled = take(&gpu, &mut renderer, &device, &queue);
  assert_eq!(pixel(&scaled, 100, 100), [255; 4]);
  gpu.test_resize(256, 256, false);
  let resized = take(&gpu, &mut renderer, &device, &queue);
  assert!(resized.rgba.iter().all(|v| *v == 0));
  gpu.detach();
  d.fill_rect(0., 0., 10., 10.);
  assert_eq!(gpu.status().error, Some(CanvasError::Detached));
  renderer.process(&device, &queue, &[]);
  assert_eq!(gpu.status().gpu_bytes, 0);
}

/// Reproducible end-to-end offscreen work, including GPU completion. No swapchain
/// or vsync is included. Run in release with --ignored --nocapture.
#[test]
#[ignore = "release GPU performance probe"]
fn canvas_gpu_performance() {
  let (device, queue) = device();
  let mut renderer = Renderer::new(&device, &queue);
  for (w, h, scale) in [(800, 600, 1.), (1920, 1080, 1.), (3840, 2160, 2.)] {
    let canvas = CanvasHandle::test_surface(w, h, scale, false);
    let d = canvas.context_2d();
    d.set_fill_style("#182434");
    d.fill_rect(0., 0., w as f32 / scale, h as f32 / scale);
    renderer.process(&device, &queue, std::slice::from_ref(&canvas));
    device.poll(PollType::wait_indefinitely()).unwrap();
    d.set_fill_style("#60a5fa80");
    for full in [false, true] {
      let mut completed = Vec::new();
      let mut submitted = Vec::new();
      let before = canvas.status().gpu;
      for index in 0..44 {
        let start = Instant::now();
        if full {
          d.clear();
          d.fill_rect(0., 0., w as f32 / scale, h as f32 / scale);
        } else {
          d.fill_rect(20., 20., 64., 64.);
        }
        renderer.process(&device, &queue, std::slice::from_ref(&canvas));
        let cpu = start.elapsed();
        device.poll(PollType::wait_indefinitely()).unwrap();
        if index >= 4 {
          submitted.push(cpu.as_secs_f64() * 1000.);
          completed.push(start.elapsed().as_secs_f64() * 1000.);
        }
      }
      submitted.sort_by(f64::total_cmp);
      completed.sort_by(f64::total_cmp);
      let after = canvas.status().gpu;
      eprintln!(
        "backing={w}x{h} full={full} submit_median_ms={:.4} submit_p95_ms={:.4} complete_median_ms={:.4} complete_p95_ms={:.4} backing_bytes={} source_upload_bytes={} tiles_per_update={}",
        submitted[20],
        submitted[38],
        completed[20],
        completed[38],
        canvas.status().gpu_bytes,
        after.uploaded_bytes - before.uploaded_bytes,
        (after.tiles - before.tiles) / 44
      );
    }
  }
}

/// Gradients, shadows, layer blur, blend modes and isolated layers, compared
/// against the software backend that defines what each of them means.
#[test]
#[ignore = "requires a GPU adapter; run explicitly and separately from context-creation tests"]
fn gpu_canvas_effects_match_the_software_backend() {
  let (device, queue) = device();
  let mut renderer = Renderer::new(&device, &queue);
  let gpu = CanvasHandle::test_surface(512, 512, 1., false);
  let cpu = CanvasHandle::test_surface(512, 512, 1., true);
  for canvas in [&gpu, &cpu] {
    crate::canvas::effects_scene(&canvas.context_2d());
  }
  let actual = take(&gpu, &mut renderer, &device, &queue);
  let expected = cpu.snapshot().try_take().unwrap().unwrap();
  let mut different = 0;
  let mut worst = 0u8;
  for (index, (a, b)) in actual
    .rgba
    .chunks_exact(4)
    .zip(expected.rgba.chunks_exact(4))
    .enumerate()
  {
    let max = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    if max > 16 {
      different += 1;
      if max > worst {
        worst = max;
        eprintln!(
          "worst so far at {},{}: {a:?} vs {b:?}",
          index as u32 % 512,
          index as u32 / 512
        );
      }
    }
  }
  assert!(
    different < 512 * 512 / 100,
    "AA-tolerant parity over gradients, shadows, blur, blend modes and layers: {different} pixels differ, worst {worst}"
  );
  // The isolated group at the bottom right fades as one image: the overlap of
  // its two shapes has the same alpha as either shape alone.
  assert!(
    pixel(&actual, 300, 300)[3].abs_diff(pixel(&actual, 340, 340)[3]) <= 2,
    "{:?} vs {:?}",
    pixel(&actual, 300, 300),
    pixel(&actual, 340, 340)
  );
  assert_eq!(gpu.status().error, None);
}
