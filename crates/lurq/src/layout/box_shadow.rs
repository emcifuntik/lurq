//! Box-shadow geometry shared by layout, the render backends and the software
//! screenshot renderer.
//!
//! A blurred shadow is the shadow shape (a rounded rect) convolved with a
//! Gaussian of standard deviation `sigma = blur / 2`, as CSS defines it. The
//! quad shaders (`quad.wgsl`, `quad.hlsl`) evaluate that analytically per
//! fragment: along x the convolution of a rounded-rect row with a Gaussian is
//! an `erf` difference, and along y it is integrated with
//! [`SHADOW_Y_SAMPLES`] midpoint samples over +/- 3 sigma. No offscreen target
//! or blur pass is involved, so a shadow is one instance in the existing quad
//! batch. [`blurred_rounded_rect_coverage`] is the same formula on the CPU,
//! kept in step with both shaders.

/// A Gaussian is treated as zero beyond this many standard deviations; it
/// bounds the quad a shadow draws and the y integration.
pub const BLUR_EXTENT_SIGMAS: f32 = 3.0;

/// Midpoint samples of the y integration in the shaders and on the CPU.
pub const SHADOW_Y_SAMPLES: u32 = 8;

/// Below this sigma (in physical pixels) a shadow is drawn as a hard,
/// anti-aliased rounded rect instead of a blur.
pub const MIN_BLUR_SIGMA: f32 = 0.1;

/// The corner radius of a shape grown by `delta` (shrunk when negative), as
/// CSS adjusts border radii for spread: a shrinking corner loses `delta` and
/// stops at 0; a growing one gains `delta`, except that a corner sharper than
/// `delta` grows by less (`1 + (r/delta - 1)^3` of it) so square corners stay
/// square and small radii do not balloon.
pub fn spread_radius(radius: f32, delta: f32) -> f32 {
  if radius <= 0.0 {
    return 0.0;
  }
  if delta <= 0.0 {
    return (radius + delta).max(0.0);
  }
  let ratio = radius / delta;
  if ratio >= 1.0 {
    radius + delta
  } else {
    let t = ratio - 1.0;
    radius + delta * (1.0 + t * t * t)
  }
}

/// Approximation of `erf` (Abramowitz and Stegun 7.1.27, max error 5e-4), the
/// one the shaders use.
pub fn erf(x: f32) -> f32 {
  let sign = x.signum();
  let a = x.abs();
  let r = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
  let r2 = r * r;
  sign - sign / (r2 * r2)
}

fn gaussian(x: f32, sigma: f32) -> f32 {
  (-(x * x) / (2.0 * sigma * sigma)).exp() / (2.506_628_3 * sigma)
}

/// The corner radius of the quadrant `(x, y)` lies in, in CSS order (top-left,
/// top-right, bottom-right, bottom-left), relative to the shape's centre.
fn pick_corner(x: f32, y: f32, radii: [f32; 4]) -> f32 {
  match (y < 0.0, x < 0.0) {
    (true, true) => radii[0],
    (true, false) => radii[1],
    (false, false) => radii[2],
    (false, true) => radii[3],
  }
}

fn blur_along_x(x: f32, y: f32, sigma: f32, corner: f32, half: [f32; 2]) -> f32 {
  let delta = (half[1] - corner - y.abs()).min(0.0);
  let curved = half[0] - corner + (corner * corner - delta * delta).max(0.0).sqrt();
  let scale = std::f32::consts::FRAC_1_SQRT_2 / sigma;
  let low = 0.5 + 0.5 * erf((x - curved) * scale);
  let high = 0.5 + 0.5 * erf((x + curved) * scale);
  high - low
}

/// Coverage (0 to 1) at `(x, y)`, relative to the centre of a rounded rect of
/// half extent `half` and corner `radii`, after a Gaussian blur of `sigma`.
pub fn blurred_rounded_rect_coverage(x: f32, y: f32, half: [f32; 2], radii: [f32; 4], sigma: f32) -> f32 {
  if half[0] <= 0.0 || half[1] <= 0.0 {
    return 0.0;
  }
  let corner = pick_corner(x, y, radii).min(half[0]).min(half[1]).max(0.0);
  let extent = BLUR_EXTENT_SIGMAS * sigma;
  let start = (-extent).clamp(y - half[1], y + half[1]);
  let end = extent.clamp(y - half[1], y + half[1]);
  let step = (end - start) / SHADOW_Y_SAMPLES as f32;
  let mut sample = start + step * 0.5;
  let mut coverage = 0.0;
  for _ in 0..SHADOW_Y_SAMPLES {
    coverage += blur_along_x(x, y - sample, sigma, corner, half) * gaussian(sample, sigma) * step;
    sample += step;
  }
  coverage.clamp(0.0, 1.0)
}

/// The render command for one shadow quad (see
/// [`QuadContent::BoxShadow`](crate::layout::quad::QuadContent::BoxShadow)),
/// in physical pixels at `scale`, with the quad's opacity applied.
pub(crate) fn shadow_rect_cmd(
  order: usize,
  quad: &crate::layout::quad::Quad,
  shadow: &crate::node::box_shadow::ResolvedBoxShadow,
  scale: f32,
  clip: crate::layout::quad::ClipRect,
) -> crate::layout::render_list::RectCmd {
  let (x, y, width, height) = (quad.x * scale, quad.y * scale, quad.width * scale, quad.height * scale);
  let max_radius = width.min(height) * 0.5;
  let radii = quad
    .border_radius
    .map(|radius| radius.to_array().map(|corner| (corner * scale).min(max_radius)))
    .unwrap_or([0.0; 4]);
  let spread = shadow.spread * scale;
  // An outer shape grows by the spread, an inset one shrinks by it.
  let delta = if shadow.inset { -spread } else { spread };
  let alpha = (f32::from(shadow.color.a()) * quad.opacity.clamp(0.0, 1.0)).round() as u8;
  crate::layout::render_list::RectCmd {
    order,
    x,
    y,
    width,
    height,
    color: shadow.color.with_alpha(alpha),
    radii,
    stroke: [0.0; 4],
    stroke_color: crate::node::color::Color::new(0, 0, 0, 0),
    stroke_style: crate::layout::render_list::StrokeStyle::default(),
    transform: quad.transform.matrix_2x2(),
    transform_origin: quad
      .transform_origin
      .map(|[x, y]| [x * scale, y * scale])
      .unwrap_or([width * 0.5, height * 0.5]),
    clip,
    gradient: None,
    shadow: Some(crate::layout::render_list::RectShadow {
      inset: shadow.inset,
      offset: [shadow.offset_x * scale, shadow.offset_y * scale],
      spread,
      sigma: shadow.blur * scale * 0.5,
      shape_radii: radii.map(|radius| spread_radius(radius, delta)),
    }),
  }
}

/// Signed distance from `(x, y)` (centre-relative) to a rounded rect with
/// circular corners; the quad shaders' `sd_rounded_box`.
fn rounded_rect_distance(x: f32, y: f32, half: [f32; 2], radii: [f32; 4]) -> f32 {
  let radius = pick_corner(x, y, radii).max(0.0);
  let qx = x.abs() - half[0] + radius;
  let qy = y.abs() - half[1] + radius;
  if qx > 0.0 && qy > 0.0 {
    return (qx * qx + qy * qy).sqrt() - radius;
  }
  (qx - radius).max(qy - radius)
}

/// Anti-aliased coverage of an untransformed rounded rect at a pixel centre:
/// four half-pixel subsamples with half-pixel ramps, as the quad shaders do.
pub(crate) fn rounded_rect_coverage(x: f32, y: f32, half: [f32; 2], radii: [f32; 4]) -> f32 {
  if half[0] <= 0.0 || half[1] <= 0.0 {
    return 0.0;
  }
  [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)]
    .into_iter()
    .map(|(dx, dy)| (0.5 - rounded_rect_distance(x + dx, y + dy, half, radii) / 0.5).clamp(0.0, 1.0))
    .sum::<f32>()
    * 0.25
}

/// The shadow alpha (0 to 1, before the colour's own alpha) that the quad
/// shaders paint at the pixel centre `(x, y)` for an untransformed shadow
/// rect. The software screenshot renderer paints with it and the backend
/// parity checks compare against it.
pub fn rect_shadow_coverage(
  rect: &crate::layout::render_list::RectCmd,
  shadow: &crate::layout::render_list::RectShadow,
  x: f32,
  y: f32,
) -> f32 {
  let half = [rect.width * 0.5, rect.height * 0.5];
  let local = [x - rect.x - half[0], y - rect.y - half[1]];
  let box_alpha = rounded_rect_coverage(local[0], local[1], half, rect.radii);
  let grow = if shadow.inset { -shadow.spread } else { shadow.spread };
  let shape_half = [(half[0] + grow).max(0.0), (half[1] + grow).max(0.0)];
  let (sx, sy) = (local[0] - shadow.offset[0], local[1] - shadow.offset[1]);
  let shape = if shadow.sigma < MIN_BLUR_SIGMA {
    rounded_rect_coverage(sx, sy, shape_half, shadow.shape_radii)
  } else {
    blurred_rounded_rect_coverage(sx, sy, shape_half, shadow.shape_radii, shadow.sigma)
  };
  if shadow.inset {
    box_alpha * (1.0 - shape)
  } else {
    shape * (1.0 - box_alpha)
  }
}
