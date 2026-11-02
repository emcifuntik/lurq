//! Explicit GPU tests: run separately; no adapter absence becomes a passing zero-test gate.
use std::{sync::atomic::Ordering, time::Duration};

use super::*;
use crate::node::transform::Transform2D;
fn take(canvas: &CanvasHandle, renderer: &mut Renderer, device: &Device, queue: &Queue) -> CanvasSnapshot {
  let read = canvas.snapshot();
  renderer.process(device, queue, &[canvas.clone()]);
  read.wait_timeout(Duration::from_secs(15)).unwrap().unwrap()
}
fn pixel(snapshot: &CanvasSnapshot, x: u32, y: u32) -> [u8; 4] {
  snapshot.rgba[((y * snapshot.width + x) * 4) as usize..((y * snapshot.width + x) * 4 + 4) as usize]
    .try_into()
    .unwrap()
}
#[test]
#[ignore = "requires a GPU adapter; execute this exact test explicitly"]
fn gpu_complete_artwork_atomic_front_revision_camera_and_target_reuse() {
  let (device, queue) = tests::device();
  let mut renderer = Renderer::new(&device, &queue);
  let canvas = CanvasHandle::test_surface(64, 64, 1., false);
  let draw = canvas.context_2d();
  let first = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#ff0000");
  draw.fill_rect(0., 0., 64., 64.);
  draw.capture_artwork().unwrap();
  draw.set_fill_style("#0000ff");
  draw.fill_rect(0., 0., 16., 64.);
  draw.commit_presentation(first).unwrap();
  let before = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!(pixel(&before, 4, 4), [0, 0, 255, 255]);
  let next = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#00ff00");
  draw.fill_rect(0., 0., 32., 64.);
  let partial = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!((partial.rgba, partial.revision), (before.rgba.clone(), before.revision));
  draw.abort_presentation(next).unwrap();
  take(&canvas, &mut renderer, &device, &queue);
  let stats = canvas.status().gpu;
  for shift in 2..10 {
    let token = draw.begin_presentation().unwrap();
    draw.reset();
    draw
      .draw_retained_artwork(Transform2D::translate(shift as f32, 0.))
      .unwrap();
    draw.commit_presentation(token).unwrap();
    let shown = take(&canvas, &mut renderer, &device, &queue);
    assert_eq!(
      pixel(&shown, shift + 2, 4),
      [255, 0, 0, 255],
      "stale selection overlay was excluded"
    );
    assert_eq!(pixel(&shown, 0, 4), [0, 0, 0, 0]);
    assert_eq!(canvas.status().gpu.presentation_token, token);
  }
  let after = canvas.status().gpu;
  assert_eq!(after.presentation_allocations, stats.presentation_allocations);
  assert_eq!(after.presentation_reuses - stats.presentation_reuses, 8);
  assert_eq!(after.fallback_quads - stats.fallback_quads, 8);
  assert_eq!(after.mesh_cache_misses, stats.mesh_cache_misses);
}
#[test]
#[ignore = "requires a GPU adapter; execute this exact test explicitly"]
fn gpu_pending_lease_pressure_retains_front_then_recovers_latest_matching_commit() {
  let (device, queue) = tests::device();
  let mut renderer = Renderer::new(&device, &queue);
  let canvas = CanvasHandle::test_surface(64, 64, 1., false);
  let draw = canvas.context_2d();
  for index in 0..3 {
    let token = draw.begin_presentation().unwrap();
    draw.reset();
    draw.set_fill_style("#ff0000");
    draw.fill_rect(0., 0., 64., 64.);
    draw.commit_presentation(token).unwrap();
    take(&canvas, &mut renderer, &device, &queue);
    // Delay completion authority after its real callback has run: no allocation may guess readiness.
    assert_eq!(renderer.spares.len(), index + 1);
    assert!(renderer.spares.iter().any(|spare| spare.ready.load(Ordering::Acquire)));
    for spare in &renderer.spares {
      spare.ready.store(false, Ordering::Release);
    }
  }
  let before = take(&canvas, &mut renderer, &device, &queue);
  let rejected = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#0000ff");
  draw.fill_rect(0., 0., 64., 64.);
  draw.commit_presentation(rejected).unwrap();
  let denied = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!((denied.rgba, denied.revision), (before.rgba.clone(), before.revision));
  assert_eq!(canvas.status().error, Some(CanvasError::PresentationBusy));
  assert_ne!(canvas.status().gpu.presentation_token, rejected);
  let refused_recording = canvas.status().content_revision;
  draw.set_fill_style("#0000ff");
  draw.fill_rect(0., 0., 64., 64.);
  draw.reset();
  assert_eq!(canvas.status().content_revision, refused_recording);
  let ordinary = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!((ordinary.rgba, ordinary.revision), (before.rgba, before.revision));
  assert_eq!(canvas.status().error, Some(CanvasError::PresentationBusy));
  renderer.spares[0].ready.store(true, Ordering::Release);
  let latest = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#00ff00");
  draw.fill_rect(0., 0., 64., 64.);
  draw.commit_presentation(latest).unwrap();
  let shown = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!(pixel(&shown, 4, 4), [0, 255, 0, 255]);
  assert_eq!(canvas.status().gpu.presentation_token, latest);
  assert_eq!(canvas.status().error, None);
}

#[test]
#[ignore = "requires a GPU adapter; execute this exact test explicitly"]
fn gpu_superseded_queued_commit_cannot_publish_or_change_visible_revision() {
  let (device, queue) = tests::device();
  let mut renderer = Renderer::new(&device, &queue);
  let canvas = CanvasHandle::test_surface(64, 64, 1., false);
  let draw = canvas.context_2d();
  let first = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#ff0000");
  draw.fill_rect(0., 0., 64., 64.);
  draw.commit_presentation(first).unwrap();
  let before = take(&canvas, &mut renderer, &device, &queue);
  let obsolete = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#0000ff");
  draw.fill_rect(0., 0., 64., 64.);
  draw.commit_presentation(obsolete).unwrap();
  let latest = draw.begin_presentation().unwrap();
  draw.reset();
  draw.set_fill_style("#00ff00");
  draw.fill_rect(0., 0., 64., 64.);
  let pending = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!((pending.rgba, pending.revision), (before.rgba, before.revision));
  assert_ne!(canvas.status().gpu.presentation_token, obsolete);
  draw.commit_presentation(latest).unwrap();
  let shown = take(&canvas, &mut renderer, &device, &queue);
  assert_eq!(pixel(&shown, 4, 4), [0, 255, 0, 255]);
  assert_eq!(canvas.status().gpu.presentation_token, latest);
}
