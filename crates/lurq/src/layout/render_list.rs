use crate::{
  app::theme::ThemePalette,
  layout::quad::ClipRect,
  node::{
    color::Color,
    gradient::{Gradient, GradientKind},
  },
};

pub struct RenderList {
  pub clear_color: Color,
  pub rects: Vec<RectCmd>,
  pub glyphs: Vec<GlyphCmd>,
  #[cfg(feature = "raster")]
  pub images: Vec<crate::images::ImageCmd>,
  #[cfg(feature = "svg")]
  pub svgs: Vec<crate::svg::SvgCmd>,
  /// Subtrees faded with `opacity < 1`, each painted into an offscreen layer
  /// and composited once at its opacity. Empty when nothing is faded.
  pub layers: Vec<crate::layout::opacity_layer::LayerCmd>,
  /// Blurs of what is painted behind a node (`.backdrop_blur(...)`), by
  /// ascending order. Empty when nothing blurs its backdrop.
  pub backdrops: Vec<crate::layout::backdrop::BackdropCmd>,
  pub atlas: GlyphAtlas,
}

#[derive(Clone)]
pub struct RectCmd {
  pub order: usize,
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub color: Color,
  pub radii: [f32; 4],
  pub stroke: [f32; 4],
  pub stroke_color: Color,
  /// How the stroke is drawn beyond its widths and color; `StrokeStyle::default()` is a solid line whose
  /// outer edge has the rect's `radii`.
  pub stroke_style: StrokeStyle,
  pub transform: [f32; 4],
  pub transform_origin: [f32; 2],
  pub clip: ClipRect,
  pub gradient: Option<RenderGradient>,
  /// Paints a box shadow of this rect instead of filling it; `color` is the
  /// shadow colour and `stroke`/`gradient` are unused. See [`RectShadow`].
  pub shadow: Option<RectShadow>,
}

/// How a [`RectCmd`]'s stroke is drawn beyond its widths and color. The shape is described in
/// [`crate::layout::stroke`], which holds the formulas the renderers paint with.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrokeStyle {
  /// The length of a dash and of the gap after it along the rect's outline, in physical pixels. A stroke
  /// with no dash or no gap is a solid line.
  pub dash: [f32; 2],
  /// Which edge of the stroke `RectCmd::radii` are the corner radii of: 0 for its outer edge, the rect's
  /// own (a border inside an element's box), 1 for the edge of its hole (a border outside the box, whose
  /// rect is the box grown by the widths), 0.5 for the middle of the stroke (a centered border).
  pub radii_at: f32,
}

/// A box shadow cast by a [`RectCmd`]'s box (x, y, width, height, `radii`).
/// Lengths are physical pixels.
///
/// An outer shadow's shape is the box grown by `spread` and moved by `offset`;
/// it paints only outside the box. An inset shadow's box is the element's
/// padding box; the shape is that box shrunk by `spread` and moved by
/// `offset`, and the shadow paints inside the box and outside the shape.
/// `shape_radii` are the shape's corner radii (CSS-adjusted for spread, see
/// [`spread_radius`](crate::layout::box_shadow::spread_radius)). The shape is
/// blurred by a Gaussian of standard deviation `sigma`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectShadow {
  pub inset: bool,
  pub offset: [f32; 2],
  pub spread: f32,
  pub sigma: f32,
  pub shape_radii: [f32; 4],
}

impl RectShadow {
  /// How far the shadow can paint outside its rect on any side.
  pub fn outset(&self) -> f32 {
    if self.inset {
      return 0.0;
    }
    let reach = self.spread + crate::layout::box_shadow::BLUR_EXTENT_SIGMAS * self.sigma;
    (reach + self.offset[0].abs().max(self.offset[1].abs())).max(0.0)
  }
}

/// A palette-resolved, encode-ready gradient attached to a rect fill.
///
/// `kind`: 0 = linear, 1 = radial, 2 = conic. `flags` bit0 marks a radial
/// ellipse (vs. circle). `dir` is the linear unit direction; `center` is the
/// radial/conic center normalized in `0..1`; `from_angle` is the conic start
/// angle in radians.
#[derive(Clone)]
pub struct RenderGradient {
  pub kind: u32,
  pub flags: u32,
  pub dir: [f32; 2],
  pub center: [f32; 2],
  pub from_angle: f32,
  pub stops: Vec<RenderGradientStop>,
}

#[derive(Clone, Copy)]
pub struct RenderGradientStop {
  pub color: [f32; 4],
  pub position: f32,
}

impl RenderGradient {
  pub fn resolve(gradient: &Gradient, palette: &ThemePalette) -> Option<Self> {
    if gradient.stops.is_empty() {
      return None;
    }
    let positions = normalize_positions(gradient);
    let stops = gradient
      .stops
      .iter()
      .zip(positions)
      .map(|(stop, position)| RenderGradientStop {
        color: stop
          .color
          .resolve(palette)
          .unwrap_or(Color::new(0, 0, 0, 0))
          .to_linear_f32_array(),
        position,
      })
      .collect();
    let (kind, flags) = match gradient.kind {
      GradientKind::Linear => (0, 0),
      GradientKind::Radial => (1, u32::from(gradient.radial_ellipse)),
      GradientKind::Conic => (2, 0),
    };
    let theta = gradient.angle_deg.to_radians();
    Some(Self {
      kind,
      flags,
      dir: [theta.sin(), -theta.cos()],
      center: [gradient.center.0, gradient.center.1],
      from_angle: theta,
      stops,
    })
  }
}

/// Resolve CSS stop positions: endpoints pin to 0/1 when omitted, defined
/// stops are clamped non-decreasing, and runs of omitted stops are spread
/// evenly between their defined neighbors.
fn normalize_positions(gradient: &Gradient) -> Vec<f32> {
  let n = gradient.stops.len();
  let mut pos: Vec<Option<f32>> = gradient.stops.iter().map(|s| s.position).collect();
  pos[0] = Some(pos[0].unwrap_or(0.0));
  pos[n - 1] = Some(pos[n - 1].unwrap_or(1.0));

  let mut last = 0.0_f32;
  for slot in pos.iter_mut() {
    if let Some(value) = slot {
      let clamped = value.clamp(0.0, 1.0).max(last);
      *slot = Some(clamped);
      last = clamped;
    }
  }

  let mut i = 0;
  while i < n {
    if pos[i].is_some() {
      i += 1;
      continue;
    }
    let start = i - 1;
    let start_val = pos[start].unwrap();
    let mut j = i;
    while j < n && pos[j].is_none() {
      j += 1;
    }
    let end_val = pos[j].unwrap();
    let segments = (j - start) as f32;
    for (offset, k) in (i..j).enumerate() {
      let frac = (offset + 1) as f32 / segments;
      pos[k] = Some(start_val + (end_val - start_val) * frac);
    }
    i = j;
  }

  pos.into_iter().map(|p| p.unwrap_or(0.0)).collect()
}

/// Append a gradient to a `vec4` storage buffer and return the `vec4` index of
/// its header. Layout (shared by both shader backends):
/// `[count, kind, flags, from_angle]`, `[dir.x, dir.y, center.x, center.y]`,
/// then per stop `[r, g, b, a]` and `[position, 0, 0, 0]`.
pub fn encode_gradient(buffer: &mut Vec<[f32; 4]>, gradient: &RenderGradient) -> f32 {
  let offset = buffer.len() as f32;
  buffer.push([
    gradient.stops.len() as f32,
    gradient.kind as f32,
    gradient.flags as f32,
    gradient.from_angle,
  ]);
  buffer.push([gradient.dir[0], gradient.dir[1], gradient.center[0], gradient.center[1]]);
  for stop in &gradient.stops {
    buffer.push(stop.color);
    buffer.push([stop.position, 0.0, 0.0, 0.0]);
  }
  offset
}

#[derive(Clone)]
pub struct GlyphCmd {
  pub order: usize,
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub color: [f32; 4],
  /// Glyph bounds in atlas pixels. Render backends normalize these against
  /// the final atlas size while building their existing instance buffers, so
  /// cached commands remain valid when the shared atlas grows.
  pub atlas_min: [f32; 2],
  pub atlas_max: [f32; 2],
  pub transform: [f32; 4],
  pub transform_origin: [f32; 2],
  pub sharpness: f32,
  pub color_glyph: bool,
  /// Gaussian sigma in physical pixels; `> 0` marks a text-shadow instance
  /// that the glyph shader blurs and the vertex stage expands.
  pub shadow_sigma: f32,
  pub clip: ClipRect,
}

#[derive(Clone)]
pub struct GlyphAtlas {
  pub data: std::sync::Arc<[u8]>,
  pub width: u32,
  pub height: u32,
  pub version: u64,
  /// Retained log of packed-glyph updates, each tagged with the atlas version
  /// its pack produced. The atlas is shared by every window, but each window's
  /// render engine owns a separate GPU texture, so the log must serve
  /// consumers at different versions: apply the rects newer than the version
  /// already uploaded. Entries older than `dirty_from_version` are pruned —
  /// a consumer behind that must re-upload the whole atlas.
  pub dirty_rects: std::sync::Arc<[GlyphAtlasDirtyRect]>,
  pub dirty_from_version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphAtlasDirtyRect {
  pub x: u32,
  pub y: u32,
  pub width: u32,
  pub height: u32,
  /// Atlas version produced by the pack that dirtied this rect (merged rects
  /// carry the newest version — re-applying the older part is harmless).
  pub version: u64,
}
