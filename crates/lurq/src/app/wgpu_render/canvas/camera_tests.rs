use std::time::Duration;

use super::*;
use crate::{
  canvas::{FillRule, Path2D},
  node::transform::Transform2D,
};

#[test]
#[ignore = "requires a GPU adapter; run separately from context-creation tests"]
fn gpu_canvas_camera_cache_pixels() {
  let instance = Instance::default();
  let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default())).unwrap();
  eprintln!("canvas camera GPU: {:?}", adapter.get_info());
  let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
  let mut renderer = Renderer::new(&device, &queue);
  let mut path = Path2D::new();
  path.round_rect(30., 25., 140., 100., 18.).unwrap();
  path.rect(60., 50., 45., 40.);
  let mut clip = Path2D::new();
  clip.round_rect(10., 10., 210., 160., 15.).unwrap();
  let mut curve = Path2D::new();
  curve.move_to(20., 100.);
  curve.bezier_curve_to(50., 20., 150., 170., 200., 60.);
  curve.quadratic_curve_to(160., 130., 20., 100.);
  curve.close_path();
  for scale in [1., 2.] {
    let gpu = CanvasHandle::test_surface((320. * scale) as u32, (240. * scale) as u32, scale, false);
    let cpu = CanvasHandle::test_surface((320. * scale) as u32, (240. * scale) as u32, scale, true);
    for matrix in [
      Transform2D::IDENTITY,
      Transform2D::translate(13.25, 7.5),
      Transform2D::translate(-7., 4.).then(&Transform2D::scale_uniform(1.4)),
      Transform2D::scale_uniform(1.8),
      Transform2D::scale_uniform(2.2),
      Transform2D::translate(220., 10.)
        .then(&Transform2D::rotate(0.2))
        .then(&Transform2D::scale(-0.8, 1.3)),
      Transform2D::skew(0.35, -0.1),
    ] {
      for canvas in [&gpu, &cpu] {
        let d = canvas.context_2d();
        d.reset();
        d.set_fill_style("#182434");
        d.fill_rect(0., 0., 320., 240.);
        d.set_transform(Transform2D::translate(5., 3.));
        d.clip_path(&clip, FillRule::NonZero);
        d.set_transform(matrix);
        d.set_fill_style("#ff660080");
        d.fill_path(&path, FillRule::EvenOdd);
        d.set_global_alpha(0.7);
        d.set_fill_style("#20d0ff");
        d.fill_path(&curve, FillRule::NonZero);
        d.set_stroke_style("#ffffff");
        d.set_line_width(2.5);
        d.set_line_dash(&[5., 3.]);
        d.stroke_path(&curve);
        d.clear_rect(45., 45., 15., 15.);
      }
      let ticket = gpu.snapshot();
      renderer.process(&device, &queue, std::slice::from_ref(&gpu));
      let actual = ticket.wait_timeout(Duration::from_secs(15)).unwrap().unwrap();
      let expected = cpu.snapshot().try_take().unwrap().unwrap();
      let different = actual
        .rgba
        .chunks_exact(4)
        .zip(expected.rgba.chunks_exact(4))
        .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 16))
        .count();
      assert!(
        different < actual.width as usize * actual.height as usize / 100,
        "scale={scale} matrix={matrix:?}: {different} pixels differ by >16"
      );
      assert!(gpu.status().error.is_none(), "{:?}", gpu.status());
    }
  }
}
