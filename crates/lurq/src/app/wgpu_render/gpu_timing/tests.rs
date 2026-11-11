use std::time::{Duration, Instant};

use super::*;
use crate::app::profiler::{PassSample, SampleData, producer::WindowProfiler};

/// One device for every test in this module, on the engine's default backends
/// and with timestamp queries when the adapter offers them: creating devices on
/// parallel test threads crashes some drivers.
fn gpu() -> &'static (wgpu::Adapter, wgpu::Device, wgpu::Queue) {
  static GPU: std::sync::OnceLock<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> = std::sync::OnceLock::new();
  GPU.get_or_init(|| {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = super::super::default_backends();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
      .expect("GPU adapter required for GPU timing tests");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
      label: Some("gpu timing tests"),
      required_features: GpuTimer::requested_features(&adapter),
      ..Default::default()
    }))
    .unwrap();
    (adapter, device, queue)
  })
}

fn configured_timer() -> GpuTimer {
  let (adapter, device, queue) = gpu();
  let mut timer = GpuTimer::new();
  timer.configure(adapter, device, queue);
  timer
}

fn target(device: &wgpu::Device) -> wgpu::TextureView {
  device
    .create_texture(&wgpu::TextureDescriptor {
      label: Some("gpu timing target"),
      size: wgpu::Extent3d {
        width: 64,
        height: 64,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      format: wgpu::TextureFormat::Rgba8Unorm,
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      view_formats: &[],
    })
    .create_view(&Default::default())
}

fn clear_pass<'a>(
  encoder: &'a mut wgpu::CommandEncoder,
  view: &wgpu::TextureView,
  timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'a> {
  encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("gpu timing pass"),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
      view,
      depth_slice: None,
      resolve_target: None,
      ops: wgpu::Operations {
        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        store: wgpu::StoreOp::Store,
      },
    })],
    timestamp_writes,
    ..Default::default()
  })
}

/// One frame as the engine renders it: a Canvas command buffer, then the UI
/// encoder with its pass, the resolve and the submission.
fn render_frame(timer: &mut GpuTimer, context: &ProfileContext) {
  let (_, device, queue) = gpu();
  let view = target(device);
  timer.begin_frame(device, Some(context));
  let mut marks = timer.canvas_marks();
  let mut canvas = device.create_command_encoder(&Default::default());
  begin_canvas(&mut marks, &mut canvas);
  drop(clear_pass(&mut canvas, &view, None));
  end_canvas(&mut marks, &mut canvas);
  queue.submit([canvas.finish()]);
  timer.canvas_done(marks);
  let mut ui = device.create_command_encoder(&Default::default());
  drop(clear_pass(&mut ui, &view, timer.ui_pass_writes(true, true)));
  timer.resolve(&mut ui);
  queue.submit([ui.finish()]);
  timer.submitted();
}

fn presented(producer: &mut WindowProfiler, frame_id: u64, timer: &mut GpuTimer) {
  let (started, guard) = producer.begin_pass(frame_id);
  render_frame(timer, &producer.context.clone());
  producer.finish_pass(
    started,
    &crate::app::PassReport {
      required: true,
      rendered: true,
      ..Default::default()
    },
    "wgpu",
    None,
  );
  drop(guard);
}

fn pass_of(sample: &crate::app::profiler::ProfileSample) -> &PassSample {
  match &sample.data {
    SampleData::Pass(pass) => pass,
    _ => panic!("expected a pass"),
  }
}

#[test]
fn ticks_become_ui_and_canvas_time() {
  // 2 ns a tick: UI 100 ticks, Canvas 30 + 20 ticks.
  let timing = gpu_timing(&[1000, 1100, 2000, 2030, 3000, 3020], true, 2.0).unwrap();
  assert_eq!(timing.ui, Duration::from_nanos(200));
  assert_eq!(timing.canvas, Some(Duration::from_nanos(100)));
  assert_eq!(timing.total(), Duration::from_nanos(300));
  // No Canvas command buffer this frame.
  assert_eq!(gpu_timing(&[1000, 1100], true, 1.0).unwrap().canvas, None);
  // Not every Canvas command buffer was measured, or one pair is invalid.
  assert_eq!(gpu_timing(&[1000, 1100, 2000, 2030], false, 1.0).unwrap().canvas, None);
  assert_eq!(gpu_timing(&[1000, 1100, 2030, 2000], true, 1.0).unwrap().canvas, None);
  // A UI pair that was never written or runs backwards is no result at all.
  assert_eq!(gpu_timing(&[0, 1100], true, 1.0), None);
  assert_eq!(gpu_timing(&[1100, 1000], true, 1.0), None);
  assert_eq!(gpu_timing(&[1000], true, 1.0), None);
}

#[test]
#[ignore = "requires a GPU adapter; run explicitly"]
fn canvas_marks_stop_at_the_query_set_and_mark_the_canvas_incomplete() {
  let (_, device, _) = gpu();
  if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
    return;
  }
  let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
    label: None,
    ty: wgpu::QueryType::Timestamp,
    count: QUERY_COUNT,
  });
  let mut marks = CanvasMarks {
    query_set,
    next_query: QUERY_COUNT - 2,
    open: false,
    overflow: false,
  };
  let mut encoder = device.create_command_encoder(&Default::default());
  marks.begin(&mut encoder);
  marks.end(&mut encoder);
  assert_eq!((marks.next_query, marks.overflow), (QUERY_COUNT, false));
  // The set is full: nothing more is written, and the frame's Canvas time is unknown.
  marks.begin(&mut encoder);
  marks.end(&mut encoder);
  assert_eq!((marks.next_query, marks.overflow), (QUERY_COUNT, true));
  drop(encoder);
}

#[test]
#[ignore = "requires a GPU adapter; run explicitly"]
fn no_query_work_without_an_active_session() {
  let (adapter, device, _) = gpu();
  let mut timer = configured_timer();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let (_, guard) = producer.begin_pass(1);
  timer.begin_frame(device, Some(&producer.context.clone()));
  drop(guard);
  // Nothing is created, written, resolved or awaited.
  assert!(timer.frame.is_none());
  assert!(timer.resources.is_none());
  assert!(timer.ui_pass_writes(true, true).is_none());
  assert!(timer.canvas_marks().is_none());
  let mut encoder = device.create_command_encoder(&Default::default());
  timer.resolve(&mut encoder);
  timer.submitted();
  assert!(timer.in_flight.is_empty());
  // The adapter's availability is still reported, with its backend.
  let gpu_timestamps = handle.availability().gpu_timestamps;
  assert_eq!(
    gpu_timestamps.adapter_backend,
    Some(adapter.get_info().backend.to_str())
  );
  assert_eq!(
    gpu_timestamps.available,
    device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
  );
}

#[test]
#[ignore = "requires a GPU adapter; run explicitly"]
fn recorded_frames_get_their_gpu_time_by_frame_id() {
  let (_, device, _) = gpu();
  let mut timer = configured_timer();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let id = handle.start(Default::default()).unwrap().id;
  if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
    // The frame renders unmeasured and the export says why.
    presented(&mut producer, 1, &mut timer);
    let report = handle.end(id).unwrap();
    assert!(timer.resources.is_none());
    assert_eq!(pass_of(&report.samples[0]).gpu, None);
    assert!(!report.build.gpu_timestamps.available);
    assert_eq!(
      report.build.gpu_timestamps.reason,
      Some(GpuTimestampAvailability::NO_TIMESTAMP_QUERY)
    );
    return;
  }
  presented(&mut producer, 1, &mut timer);
  presented(&mut producer, 2, &mut timer);
  presented(&mut producer, 3, &mut timer);
  // Results are read by a later frame once the GPU is done; outside a pass
  // that frame measures nothing itself.
  let deadline = Instant::now() + Duration::from_secs(10);
  while !timer.in_flight.is_empty() && Instant::now() < deadline {
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    timer.begin_frame(device, Some(&producer.context.clone()));
  }
  // Frame 4's result is still awaited when the session ends.
  presented(&mut producer, 4, &mut timer);
  let report = handle.end(id).unwrap();
  let gpu_of = |frame_id: u64| {
    report
      .samples
      .iter()
      .map(|sample| pass_of(sample))
      .find(|pass| pass.frame_id == Some(frame_id))
      .unwrap()
      .gpu
  };
  for frame_id in 1..=3 {
    let gpu = gpu_of(frame_id).unwrap_or_else(|| panic!("frame {frame_id} has no GPU time"));
    assert!(gpu.canvas.is_some() || !timer.canvas_marks, "frame {frame_id}: {gpu:?}");
  }
  assert_eq!(gpu_of(4), None, "a result that never arrived stays null");
  assert!(report.build.gpu_timestamps.available);

  // With the session ended, nothing is queried and frame 4's result is dropped.
  let (_, guard) = producer.begin_pass(5);
  timer.begin_frame(device, Some(&producer.context.clone()));
  drop(guard);
  assert!(timer.frame.is_none());
  assert!(timer.in_flight.is_empty());
}

/// The engine itself, rendering into a hidden window as the runtime drives it:
/// a pass sets the profile context, then renders.
#[cfg(all(windows, feature = "screenshot"))]
#[test]
#[ignore = "requires a Windows desktop and a GPU adapter; creates a hidden window"]
fn the_engine_measures_its_frames_only_while_a_session_is_active() {
  use raw_window_handle::DisplayHandle;

  use crate::app::{
    readback_window::{HiddenWindow, empty_list},
    render_engine::RenderEngine,
  };
  let window = HiddenWindow::new(64, 64);
  let mut engine = super::super::WgpuRenderEngine::new();
  engine.resize(64, 64);
  let list = empty_list();
  let mut producer = WindowProfiler::new();
  let handle = producer.handle();
  let render = |producer: &mut WindowProfiler, engine: &mut super::super::WgpuRenderEngine, frame_id: u64| {
    let (started, guard) = producer.begin_pass(frame_id);
    engine.set_profile_context(producer.context.clone());
    let rendered = engine.render(&list, window.window_handle(), DisplayHandle::windows());
    producer.finish_pass(
      started,
      &crate::app::PassReport {
        required: true,
        rendered,
        ..Default::default()
      },
      "wgpu",
      None,
    );
    drop(guard);
    rendered
  };

  assert!(render(&mut producer, &mut engine, 1));
  // No session: the adapter is reported, nothing is queried.
  let timer = &engine.gpu_timer;
  assert!(timer.resources.is_none() && timer.frame.is_none() && timer.in_flight.is_empty());
  let availability = handle.availability().gpu_timestamps;
  assert!(availability.adapter_backend.is_some());
  if !availability.available {
    assert_eq!(availability.reason, Some(GpuTimestampAvailability::NO_TIMESTAMP_QUERY));
    return;
  }

  let id = handle.start(Default::default()).unwrap().id;
  let gpu_of_frame_2 = || {
    handle
      .read(id)
      .unwrap()
      .samples
      .iter()
      .map(|sample| pass_of(sample))
      .find(|pass| pass.frame_id == Some(2))
      .and_then(|pass| pass.gpu)
  };
  let mut frame_id = 2;
  while frame_id < 60 && (frame_id == 2 || gpu_of_frame_2().is_none()) {
    assert!(render(&mut producer, &mut engine, frame_id));
    frame_id += 1;
  }
  let gpu = gpu_of_frame_2().expect("frame 2's GPU time arrived within the next frames");
  // A frame that drew no Canvas has UI time only.
  assert_eq!(gpu.canvas, None);
  handle.end(id).unwrap();

  assert!(render(&mut producer, &mut engine, frame_id));
  let timer = &engine.gpu_timer;
  assert!(timer.frame.is_none() && timer.in_flight.is_empty());
}
