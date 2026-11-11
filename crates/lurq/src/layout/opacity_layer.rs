//! Flattened group opacity and layer blur.
//!
//! A node with `opacity < 1` fades its subtree as one flattened layer, like
//! CSS `opacity` and design tools: the subtree is painted into an offscreen
//! layer, which is then composited once at the node's opacity. Content inside
//! the group blends only with other content of the group, so a label over a
//! faded fill keeps its contrast against that fill, and a border drawn over
//! the fill's anti-aliased edge does not leave a lighter ring.
//!
//! A node with a blur is a layer too, like CSS `filter: blur()`: its subtree
//! is painted into the layer, and the layer is blurred as it is composited
//! ([`LayerBlur`]). A blurred layer is painted for more pixels than it shows,
//! because what lies just outside the visible part still bleeds into it.
//!
//! Layout records each group as a range of quads (`OpacityGroup`). A faded
//! group whose content paints a single primitive gives the same pixels either
//! way, so layout folds its opacity into that quad instead and no layer is
//! made. The runtime turns the remaining groups into [`LayerCmd`]s with pixel
//! bounds, and render engines walk a `LayerPlan` to paint each layer into
//! its own target before compositing it into its parent.

use std::ops::Range;

use crate::layout::{
  quad::ClipRect,
  render_list::{GlyphCmd, RectCmd},
};

/// Anti-aliasing reach of the quad pipelines past a primitive's box, in
/// physical pixels (their vertex stages grow each quad by 2 px).
const AA_REACH: f32 = 2.0;

/// Largest blur radius of a node, in logical pixels. A layer is painted for
/// its blur's reach past the window, so an unbounded radius is an unbounded
/// texture.
pub const MAX_BLUR_RADIUS: f32 = 256.0;

/// A blur below this many physical pixels of deviation changes no pixel.
const MIN_BLUR_SIGMA: f32 = 0.05;
/// The widest Gaussian one pass samples directly, in texels of its source.
/// Wider blurs run on a reduced copy of the layer.
const MAX_PASS_SIGMA: f32 = 8.0;
/// Most the layer is reduced by for a wide blur, per axis.
const MAX_BLUR_REDUCTION: u32 = 64;

/// The Gaussian's standard deviation for a blur radius: half of it, as for
/// box and text shadows and the canvas's `Filter::Blur`.
pub(crate) fn blur_sigma(radius: f32) -> f32 {
  radius * 0.5
}

/// How far a blur of `radius` moves colour, in the radius's units: three
/// standard deviations, past which a Gaussian has under 0.3 % of its weight.
pub(crate) fn blur_reach(radius: f32) -> f32 {
  (blur_sigma(radius) * 3.0).ceil()
}

/// A subtree painted as one layer: a node with `opacity < 1` that paints more
/// than one primitive, or a node with a blur. Its quads are `start..end`
/// (quad indices, which are also the render orders).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OpacityGroup {
  pub start: usize,
  pub end: usize,
  pub opacity: f32,
  /// The node's blur radius in logical pixels; 0 without a blur.
  pub blur: f32,
  /// With a blur: the clip the node itself is under, in logical pixels
  /// (x, y, width, height). The blurred layer shows only inside it; its
  /// content is clipped further out (see `LayoutEngine::collect_quads`).
  pub clip: Option<[f32; 4]>,
}

/// An offscreen layer: the draws whose `order` lies in `start_order..end_order`
/// are painted into a transparent layer covering `bounds`, which is then
/// composited over what was painted before `start_order` at `opacity`.
///
/// Layers nest: a layer inside another lies within its order range, and
/// [`RenderList::layers`](crate::layout::render_list::RenderList::layers) lists
/// them in paint order, an outer layer before the layers inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerCmd {
  pub start_order: usize,
  pub end_order: usize,
  /// The group's opacity, in `0.0..1.0`.
  pub opacity: f32,
  /// Where the layer shows, in whole physical pixels of the window. Without
  /// a blur this is also where its content can paint, and the layer's pixels
  /// map one to one onto the window's, so compositing never resamples.
  pub bounds: LayerBounds,
  /// Blurs the layer as it is composited. `None` composites it as painted.
  pub blur: Option<LayerBlur>,
}

impl LayerCmd {
  /// Whether compositing the layer changes any pixel. An invisible layer is
  /// skipped with everything inside it.
  pub fn is_visible(&self) -> bool {
    self.opacity > 0.0 && !self.bounds.is_empty() && self.blur.is_none_or(|blur| blur.width > 0 && blur.height > 0)
  }

  /// The window pixels the layer's content is painted for, as an origin and
  /// a size: `bounds`, or a blurred layer's larger rectangle.
  pub fn painted(&self) -> ([i32; 2], [u32; 2]) {
    match self.blur {
      Some(blur) => ([blur.x, blur.y], [blur.width, blur.height]),
      None => (
        [self.bounds.x as i32, self.bounds.y as i32],
        [self.bounds.width, self.bounds.height],
      ),
    }
  }
}

/// A Gaussian blur of a layer, like CSS `filter: blur()`.
///
/// The layer's content is painted for the rectangle here, which reaches past
/// [`LayerCmd::bounds`] by as far as the blur moves colour and may lie partly
/// outside the window: content there is not shown, but it bleeds into what
/// is. Its pixels are whole physical pixels of the window, as `bounds` are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerBlur {
  /// Standard deviation of the Gaussian, in physical pixels.
  pub sigma: f32,
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

/// How a render engine blurs a layer: wide blurs first reduce the layer by
/// `reduction` per axis (a box filter), then both run a horizontal Gaussian
/// pass into an intermediate target and a vertical one as the layer is
/// composited. The box filter and the resampling on the way back up widen
/// the result, so the Gaussian's `sigma` is narrowed to keep the total at the
/// blur asked for.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlurPlan {
  /// 1, or a power of two the layer is reduced by before the Gaussian.
  pub reduction: u32,
  /// The Gaussian's deviation in texels of the (reduced) layer.
  pub sigma: f32,
  /// Size of the reduced layer; the layer's own size when `reduction` is 1.
  pub size: [u32; 2],
}

#[cfg_attr(not(feature = "render"), allow(dead_code))]
impl BlurPlan {
  pub(crate) fn new(blur: &LayerBlur) -> Self {
    let reduction = blur_reduction(blur.sigma);
    if reduction == 1 {
      return Self {
        reduction,
        sigma: blur.sigma,
        size: [blur.width, blur.height],
      };
    }
    let r = reduction as f32;
    // Variances add: a box of `r` texels has (r^2 - 1) / 12, and the
    // bilinear resampling back up is a tent of half-width `r`, r^2 / 6.
    let widened = (r * r - 1.0) / 12.0 + r * r / 6.0;
    let sigma = (blur.sigma * blur.sigma - widened).max(0.25).sqrt() / r;
    Self {
      reduction,
      sigma: sigma.min(MAX_PASS_SIGMA * 1.25),
      size: [
        blur.width.div_ceil(reduction).max(1),
        blur.height.div_ceil(reduction).max(1),
      ],
    }
  }
}

fn blur_reduction(sigma: f32) -> u32 {
  let mut reduction = 1;
  while sigma / reduction as f32 > MAX_PASS_SIGMA && reduction < MAX_BLUR_REDUCTION {
    reduction *= 2;
  }
  reduction
}

/// A rectangle of whole physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerBounds {
  pub x: u32,
  pub y: u32,
  pub width: u32,
  pub height: u32,
}

impl LayerBounds {
  pub fn is_empty(&self) -> bool {
    self.width == 0 || self.height == 0
  }
}

/// A step of painting one target (the window or a layer): a run of ordered
/// draws, or compositing a finished inner layer.
#[cfg_attr(not(any(feature = "render", feature = "devtools")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LayerStep {
  /// Indices into the sorted draw list.
  Draws(Range<usize>),
  /// Composite the layer painted by `LayerPlan::targets()[index]`.
  Composite(usize),
}

/// One target to paint: a layer (`Some(index into RenderList::layers)`) or,
/// last, the window itself (`None`).
#[cfg_attr(not(any(feature = "render", feature = "devtools")), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerTarget {
  pub layer: Option<usize>,
  steps: Range<usize>,
}

/// The order in which a render engine paints a frame with layers: every
/// layer's target before the target it is composited into, the window last.
/// Without layers the plan is the window's draws in one step, so opacity 1
/// costs nothing.
#[cfg_attr(not(any(feature = "render", feature = "devtools")), allow(dead_code))]
#[derive(Default)]
pub(crate) struct LayerPlan {
  targets: Vec<LayerTarget>,
  steps: Vec<LayerStep>,
  pending: Vec<LayerStep>,
}

#[cfg_attr(not(any(feature = "render", feature = "devtools")), allow(dead_code))]
impl LayerPlan {
  /// Plans `draw_count` draws sorted by order (`order_of(index)`), grouped by
  /// `layers`. Invisible layers drop their draws and the layers inside them.
  pub(crate) fn build(&mut self, draw_count: usize, order_of: impl Fn(usize) -> usize, layers: &[LayerCmd]) {
    self.targets.clear();
    self.steps.clear();
    self.pending.clear();
    let mut cursor = 0;
    self.plan_target(None, 0..draw_count, usize::MAX, &order_of, layers, &mut cursor);
  }

  /// The targets in paint order; the window is last.
  pub(crate) fn targets(&self) -> &[LayerTarget] {
    &self.targets
  }

  pub(crate) fn steps(&self, target: &LayerTarget) -> &[LayerStep] {
    &self.steps[target.steps.clone()]
  }

  /// Whether the frame has any layer to paint.
  #[cfg_attr(not(feature = "render"), allow(dead_code))]
  pub(crate) fn has_layers(&self) -> bool {
    self.targets.len() > 1
  }

  fn plan_target(
    &mut self,
    layer: Option<usize>,
    draws: Range<usize>,
    end_order: usize,
    order_of: &impl Fn(usize) -> usize,
    layers: &[LayerCmd],
    cursor: &mut usize,
  ) -> usize {
    // Steps of this target wait on `pending` while inner targets are planned.
    let pending_start = self.pending.len();
    let mut next = draws.start;
    while *cursor < layers.len() && layers[*cursor].start_order < end_order {
      let index = *cursor;
      *cursor += 1;
      let inner = layers[index];
      let start = first_at_or_after(next..draws.end, inner.start_order, order_of);
      let end = first_at_or_after(start..draws.end, inner.end_order, order_of);
      if start > next {
        self.pending.push(LayerStep::Draws(next..start));
      }
      if inner.is_visible() && end > start {
        let target = self.plan_target(Some(index), start..end, inner.end_order, order_of, layers, cursor);
        self.pending.push(LayerStep::Composite(target));
      } else {
        while *cursor < layers.len() && layers[*cursor].start_order < inner.end_order {
          *cursor += 1;
        }
      }
      next = end;
    }
    if next < draws.end {
      self.pending.push(LayerStep::Draws(next..draws.end));
    }
    let steps_start = self.steps.len();
    self.steps.extend(self.pending.drain(pending_start..));
    self.targets.push(LayerTarget {
      layer,
      steps: steps_start..self.steps.len(),
    });
    self.targets.len() - 1
  }
}

/// The pixel space a render pass draws in: the window, or a layer texture
/// whose texel (0, 0) is window pixel `origin`. Draws keep window
/// coordinates; shaders subtract `origin` (the `zw` of their globals'
/// `viewport`), and clips are moved into the target with [`Self::clip`].
#[cfg(feature = "render")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TargetSpace {
  /// Size of the render target in pixels.
  pub width: f32,
  pub height: f32,
  pub origin: [f32; 2],
  /// The layer's own size within its (possibly larger, pooled) texture.
  layer_size: Option<[f32; 2]>,
}

#[cfg(feature = "render")]
impl TargetSpace {
  pub(crate) fn window(width: f32, height: f32) -> Self {
    Self {
      width,
      height,
      origin: [0.0; 2],
      layer_size: None,
    }
  }

  /// The pixels `layer` is painted for, in the top-left corner of a
  /// `texture_width` x `texture_height` texture.
  pub(crate) fn layer(layer: &LayerCmd, texture_width: u32, texture_height: u32) -> Self {
    let (origin, size) = layer.painted();
    Self {
      width: texture_width as f32,
      height: texture_height as f32,
      origin: [origin[0] as f32, origin[1] as f32],
      layer_size: Some([size[0] as f32, size[1] as f32]),
    }
  }

  /// An intermediate target of a blur: `size` texels in the top-left corner
  /// of a `texture_width` x `texture_height` texture.
  #[cfg_attr(not(feature = "dx12"), allow(dead_code))]
  pub(crate) fn blur_target(size: [u32; 2], texture_width: u32, texture_height: u32) -> Self {
    Self {
      width: texture_width as f32,
      height: texture_height as f32,
      origin: [0.0; 2],
      layer_size: Some([size[0] as f32, size[1] as f32]),
    }
  }

  #[cfg_attr(not(feature = "wgpu"), allow(dead_code))]
  pub(crate) fn is_window(&self) -> bool {
    self.layer_size.is_none()
  }

  /// The `viewport` of the shaders' globals: target size, then origin.
  pub(crate) fn viewport(&self) -> [f32; 4] {
    [self.width, self.height, self.origin[0], self.origin[1]]
  }

  /// `clip` (window pixels) in this target's pixels. In a layer, drawing
  /// without a clip is limited to the layer's bounds.
  pub(crate) fn clip(&self, clip: ClipRect) -> ClipRect {
    let Some([width, height]) = self.layer_size else {
      return clip;
    };
    if !clip.active {
      return ClipRect {
        x: 0.0,
        y: 0.0,
        width,
        height,
        active: true,
        border_radius: None,
      };
    }
    ClipRect {
      x: clip.x - self.origin[0],
      y: clip.y - self.origin[1],
      ..clip
    }
  }

  /// Where `layer` sits in this target, as a clip in this target's pixels.
  pub(crate) fn layer_rect(&self, layer: LayerBounds) -> ClipRect {
    ClipRect {
      x: layer.x as f32 - self.origin[0],
      y: layer.y as f32 - self.origin[1],
      width: layer.width as f32,
      height: layer.height as f32,
      active: true,
      border_radius: None,
    }
  }
}

/// Uniforms of the layer-composite shaders (`layer.wgsl`, `layer.hlsl`).
#[cfg(feature = "render")]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LayerComposite {
  /// The layer's rect in the parent target's pixels: x, y, width, height.
  pub rect: [f32; 4],
  /// The parent target's size, then the offset from a parent pixel to the
  /// layer texel it shows.
  pub target: [f32; 4],
  /// `x`: the layer's opacity.
  pub opacity: [f32; 4],
}

#[cfg(feature = "render")]
impl LayerComposite {
  pub(crate) fn new(layer: &LayerCmd, parent: &TargetSpace) -> Self {
    let rect = parent.layer_rect(layer.bounds);
    Self {
      rect: [rect.x, rect.y, rect.width, rect.height],
      target: [
        parent.width,
        parent.height,
        parent.origin[0] - layer.bounds.x as f32,
        parent.origin[1] - layer.bounds.y as f32,
      ],
      opacity: [layer.opacity, 0.0, 0.0, 0.0],
    }
  }
}

/// Uniforms of one pass of the layer-blur shaders (`layer_blur.wgsl`,
/// `layer_blur.hlsl`). A pass draws `rect` of its target; each pixel reads
/// the source texture around `(pixel + offset) * scale`, in source texels.
#[cfg(feature = "render")]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LayerBlurPass {
  /// The rect drawn, in the target's pixels: x, y, width, height.
  pub rect: [f32; 4],
  /// The target's size, then the offset added to a target pixel.
  pub target: [f32; 4],
  /// The scale from offset target pixels to source texels, then one over the
  /// source texture's size.
  pub source: [f32; 4],
  /// The Gaussian's axis in source texels, its deviation in source texels,
  /// and the opacity the result is multiplied by.
  pub kernel: [f32; 4],
  /// `x`: 0 for a Gaussian pass; otherwise the pass is a box reduction that
  /// averages this many bilinear taps per axis (half the reduction).
  pub mode: [f32; 4],
}

#[cfg(feature = "render")]
impl LayerBlurPass {
  /// Reduces the painted layer (in a `source` sized texture) into the
  /// top-left corner of a `target` sized one.
  pub(crate) fn reduce(plan: &BlurPlan, source: [u32; 2], target: [u32; 2]) -> Self {
    let r = plan.reduction as f32;
    Self {
      rect: [0.0, 0.0, plan.size[0] as f32, plan.size[1] as f32],
      target: [target[0] as f32, target[1] as f32, 0.0, 0.0],
      source: [r, r, 1.0 / source[0] as f32, 1.0 / source[1] as f32],
      kernel: [0.0, 0.0, 0.0, 1.0],
      mode: [(plan.reduction / 2) as f32, 0.0, 0.0, 0.0],
    }
  }

  /// The horizontal Gaussian, between two textures holding the (reduced)
  /// layer in their top-left corners.
  pub(crate) fn horizontal(plan: &BlurPlan, source: [u32; 2], target: [u32; 2]) -> Self {
    Self {
      rect: [0.0, 0.0, plan.size[0] as f32, plan.size[1] as f32],
      target: [target[0] as f32, target[1] as f32, 0.0, 0.0],
      source: [1.0, 1.0, 1.0 / source[0] as f32, 1.0 / source[1] as f32],
      kernel: [1.0, 0.0, plan.sigma, 1.0],
      mode: [0.0; 4],
    }
  }

  /// The vertical Gaussian, composited into `parent` over `layer`'s bounds
  /// at its opacity. `source` is the size of the texture the horizontal pass
  /// painted.
  pub(crate) fn composite(layer: &LayerCmd, plan: &BlurPlan, parent: &TargetSpace, source: [u32; 2]) -> Self {
    let rect = parent.layer_rect(layer.bounds);
    let (origin, _) = layer.painted();
    let scale = 1.0 / plan.reduction as f32;
    Self {
      rect: [rect.x, rect.y, rect.width, rect.height],
      target: [
        parent.width,
        parent.height,
        parent.origin[0] - origin[0] as f32,
        parent.origin[1] - origin[1] as f32,
      ],
      source: [scale, scale, 1.0 / source[0] as f32, 1.0 / source[1] as f32],
      kernel: [0.0, 1.0, plan.sigma, layer.opacity],
      mode: [0.0; 4],
    }
  }
}

/// The first index in `range` whose order is at least `order` (`range.end`
/// when there is none); draws are sorted by order.
fn first_at_or_after(range: Range<usize>, order: usize, order_of: &impl Fn(usize) -> usize) -> usize {
  let (mut low, mut high) = (range.start, range.end);
  while low < high {
    let mid = low + (high - low) / 2;
    if order_of(mid) < order {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  low
}

/// Resolves `groups` into `layers` (cleared first) from the draws a frame
/// built for its `orders` quads, over a `viewport` of physical pixels, each
/// logical pixel `scale` of them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_layers(
  groups: &[OpacityGroup],
  orders: usize,
  viewport: [f32; 2],
  scale: f32,
  rects: &[RectCmd],
  glyphs: &[GlyphCmd],
  #[cfg(feature = "raster")] images: &[crate::images::ImageCmd],
  #[cfg(feature = "svg")] svgs: &[crate::svg::SvgCmd],
  layers: &mut Vec<LayerCmd>,
) {
  layers.clear();
  if groups.is_empty() {
    return;
  }
  let mut bounds = LayerBoundsBuilder::new(orders);
  rects.iter().for_each(|rect| bounds.add_rect(rect));
  glyphs.iter().for_each(|glyph| bounds.add_glyph(glyph));
  #[cfg(feature = "raster")]
  images.iter().for_each(|image| bounds.add_image(image));
  #[cfg(feature = "svg")]
  svgs.iter().for_each(|svg| bounds.add_svg(svg));
  bounds.build(groups, viewport, scale, layers);
}

/// Axis-aligned bounds `[x0, y0, x1, y1]` in physical pixels.
type Extent = [f32; 4];

const EMPTY_EXTENT: Extent = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

fn union(a: Extent, b: Extent) -> Extent {
  [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// Collects the painted extent of every render order, then turns `groups`
/// into layers over a `viewport` of physical pixels.
struct LayerBoundsBuilder {
  extents: Vec<Extent>,
}

impl LayerBoundsBuilder {
  /// `orders` is the number of quad orders (orders past it are overlays that
  /// no group contains).
  fn new(orders: usize) -> Self {
    Self {
      extents: vec![EMPTY_EXTENT; orders],
    }
  }

  fn add(&mut self, order: usize, extent: Extent, clip: ClipRect) {
    let Some(slot) = self.extents.get_mut(order) else {
      return;
    };
    *slot = union(*slot, clipped(extent, clip));
  }

  fn add_rect(&mut self, rect: &RectCmd) {
    let reach = AA_REACH + rect.shadow.map_or(0.0, |shadow| shadow.outset());
    let extent = transformed_extent(
      rect.x,
      rect.y,
      rect.width,
      rect.height,
      rect.transform,
      rect.transform_origin,
      reach,
    );
    self.add(rect.order, extent, rect.clip);
  }

  fn add_glyph(&mut self, glyph: &GlyphCmd) {
    // The glyph pipeline pads a blurred shadow by `ceil(2 * sigma) + 1`.
    let reach = if glyph.shadow_sigma > 0.0 {
      (glyph.shadow_sigma * 2.0).ceil() + 1.0 + AA_REACH
    } else {
      AA_REACH
    };
    let extent = transformed_extent(
      glyph.x,
      glyph.y,
      glyph.width,
      glyph.height,
      glyph.transform,
      glyph.transform_origin,
      reach,
    );
    self.add(glyph.order, extent, glyph.clip);
  }

  #[cfg(feature = "raster")]
  fn add_image(&mut self, image: &crate::images::ImageCmd) {
    let extent = transformed_extent(
      image.x,
      image.y,
      image.width,
      image.height,
      image.transform,
      image.transform_origin,
      AA_REACH,
    );
    self.add(image.order, extent, image.clip);
  }

  #[cfg(feature = "svg")]
  fn add_svg(&mut self, svg: &crate::svg::SvgCmd) {
    let mesh = svg.mesh.vertices.iter().fold(EMPTY_EXTENT, |extent, vertex| {
      let [x, y] = vertex.position;
      union(extent, [svg.x + x, svg.y + y, svg.x + x, svg.y + y])
    });
    let extent = [
      mesh[0] - AA_REACH,
      mesh[1] - AA_REACH,
      mesh[2] + AA_REACH,
      mesh[3] + AA_REACH,
    ];
    self.add(svg.order, extent, svg.clip);
  }

  /// Layers for `groups`, outer layers first.
  fn build(&mut self, groups: &[OpacityGroup], viewport: [f32; 2], scale: f32, layers: &mut Vec<LayerCmd>) {
    // Groups close children first, so a group is resolved before the groups
    // around it.
    for group in groups {
      let end = group.end.min(self.extents.len());
      let start = group.start.min(end);
      let extent = self.extents[start..end].iter().copied().fold(EMPTY_EXTENT, union);
      let (bounds, blur) = blurred_bounds(group, extent, viewport, scale);
      // A blurred layer paints past its content, and a layer around it has
      // to cover that too.
      if blur.is_some() && start < end {
        let shown = [
          bounds.x as f32,
          bounds.y as f32,
          (bounds.x + bounds.width) as f32,
          (bounds.y + bounds.height) as f32,
        ];
        self.extents[start] = union(self.extents[start], shown);
      }
      layers.push(LayerCmd {
        start_order: group.start,
        end_order: group.end,
        opacity: group.opacity.clamp(0.0, 1.0),
        bounds,
        blur,
      });
    }
    // Groups close children first; paint order puts the outer layer first.
    layers.sort_by(|a, b| a.start_order.cmp(&b.start_order).then(b.end_order.cmp(&a.end_order)));
  }
}

/// The bounding box of a `width` x `height` box at (`x`, `y`) under the 2x2
/// `transform` applied around `origin` (relative to the box), grown by `reach`.
fn transformed_extent(
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  transform: [f32; 4],
  origin: [f32; 2],
  reach: f32,
) -> Extent {
  let [a, b, c, d] = transform;
  let corners = [
    (-reach, -reach),
    (width + reach, -reach),
    (-reach, height + reach),
    (width + reach, height + reach),
  ];
  corners.iter().fold(EMPTY_EXTENT, |extent, &(local_x, local_y)| {
    let (dx, dy) = (local_x - origin[0], local_y - origin[1]);
    let px = x + origin[0] + a * dx + c * dy;
    let py = y + origin[1] + b * dx + d * dy;
    union(extent, [px, py, px, py])
  })
}

fn clipped(extent: Extent, clip: ClipRect) -> Extent {
  if !clip.active {
    return extent;
  }
  [
    extent[0].max(clip.x.floor()),
    extent[1].max(clip.y.floor()),
    extent[2].min((clip.x + clip.width).ceil()),
    extent[3].min((clip.y + clip.height).ceil()),
  ]
}

fn grown(extent: Extent, by: f32) -> Extent {
  [extent[0] - by, extent[1] - by, extent[2] + by, extent[3] + by]
}

fn intersection(a: Extent, b: Extent) -> Extent {
  [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

/// Where a group painting `extent` shows, and its blur if it has one.
///
/// A blurred layer shows as far as its blur reaches past its content, inside
/// the node's clip and the window. It is painted for the visible part grown
/// by the blur's reach (and by what the passes' sampling adds): that holds
/// the content that can reach a visible pixel, and it is also where the
/// horizontal pass leaves its result, which spreads that far sideways. Past
/// the content the rectangle ends where no pass has anything left to read.
fn blurred_bounds(
  group: &OpacityGroup,
  extent: Extent,
  viewport: [f32; 2],
  scale: f32,
) -> (LayerBounds, Option<LayerBlur>) {
  let radius = if group.blur.is_finite() {
    group.blur.clamp(0.0, MAX_BLUR_RADIUS)
  } else {
    0.0
  };
  let sigma = blur_sigma(radius) * scale;
  if sigma < MIN_BLUR_SIGMA {
    return (pixel_bounds(extent, viewport), None);
  }
  let reach = (sigma * 3.0).ceil();
  let mut shown = grown(extent, reach);
  if let Some([x, y, width, height]) = group.clip {
    shown = intersection(
      shown,
      [
        (x * scale).floor(),
        (y * scale).floor(),
        ((x + width) * scale).ceil(),
        ((y + height) * scale).ceil(),
      ],
    );
  }
  let bounds = pixel_bounds(shown, viewport);
  if bounds.is_empty() {
    return (bounds, None);
  }
  let pad = reach + 2.0 * blur_reduction(sigma) as f32 + 1.0;
  let visible = [
    bounds.x as f32,
    bounds.y as f32,
    (bounds.x + bounds.width) as f32,
    (bounds.y + bounds.height) as f32,
  ];
  let painted = intersection(grown(extent, pad), grown(visible, pad));
  let (x0, y0, x1, y1) = (
    painted[0].floor(),
    painted[1].floor(),
    painted[2].ceil(),
    painted[3].ceil(),
  );
  if !(x1 > x0 && y1 > y0) {
    return (LayerBounds::default(), None);
  }
  (
    bounds,
    Some(LayerBlur {
      sigma,
      x: x0 as i32,
      y: y0 as i32,
      width: (x1 - x0) as u32,
      height: (y1 - y0) as u32,
    }),
  )
}

fn pixel_bounds(extent: Extent, viewport: [f32; 2]) -> LayerBounds {
  let x0 = extent[0].floor().max(0.0);
  let y0 = extent[1].floor().max(0.0);
  let x1 = extent[2].ceil().min(viewport[0].ceil());
  let y1 = extent[3].ceil().min(viewport[1].ceil());
  if !(x1 > x0 && y1 > y0) {
    return LayerBounds::default();
  }
  LayerBounds {
    x: x0 as u32,
    y: y0 as u32,
    width: (x1 - x0) as u32,
    height: (y1 - y0) as u32,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn layer(start_order: usize, end_order: usize, opacity: f32) -> LayerCmd {
    LayerCmd {
      start_order,
      end_order,
      opacity,
      bounds: LayerBounds {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
      },
      blur: None,
    }
  }

  fn plan(orders: &[usize], layers: &[LayerCmd]) -> Vec<(Option<usize>, Vec<LayerStep>)> {
    let mut plan = LayerPlan::default();
    plan.build(orders.len(), |index| orders[index], layers);
    plan
      .targets()
      .iter()
      .map(|target| (target.layer, plan.steps(target).to_vec()))
      .collect()
  }

  #[test]
  fn a_narrow_blur_runs_on_the_layer_and_a_wide_one_on_a_reduced_copy() {
    let blur = |sigma: f32| LayerBlur {
      sigma,
      x: -10,
      y: 0,
      width: 201,
      height: 100,
    };
    let narrow = BlurPlan::new(&blur(4.0));
    assert_eq!((narrow.reduction, narrow.sigma, narrow.size), (1, 4.0, [201, 100]));
    assert_eq!(BlurPlan::new(&blur(MAX_PASS_SIGMA)).reduction, 1);

    for sigma in [9.0, 20.0, 100.0, 384.0] {
      let plan = BlurPlan::new(&blur(sigma));
      let r = plan.reduction as f32;
      assert!(
        plan.reduction.is_power_of_two() && plan.reduction > 1,
        "sigma {sigma}: {plan:?}"
      );
      assert!(plan.sigma <= MAX_PASS_SIGMA, "sigma {sigma}: {plan:?}");
      assert_eq!(
        plan.size,
        [201_u32.div_ceil(plan.reduction), 100_u32.div_ceil(plan.reduction)]
      );
      // The box, the Gaussian and the resampling together are the blur asked for.
      let total = ((r * r - 1.0) / 12.0 + r * r / 6.0 + (plan.sigma * r).powi(2)).sqrt();
      assert!((total - sigma).abs() < sigma * 0.01, "sigma {sigma}: {total}");
    }
  }

  #[test]
  fn a_blurred_group_shows_inside_its_clip_and_is_painted_past_it() {
    let group = OpacityGroup {
      start: 0,
      end: 1,
      opacity: 1.0,
      blur: 8.0,
      clip: Some([10.0, 0.0, 50.0, 100.0]),
    };
    // Content from x 20 to 200, in a 100 x 100 window at scale 2.
    let (bounds, blur) = blurred_bounds(&group, [20.0, 30.0, 200.0, 60.0], [100.0, 100.0], 2.0);
    let blur = blur.expect("a blur");
    assert_eq!(blur.sigma, 8.0);
    // 24 pixels of reach: left to the clip at 20, right to the window's edge, up and down by the reach.
    assert_eq!(
      bounds,
      LayerBounds {
        x: 20,
        y: 6,
        width: 80,
        height: 78
      }
    );
    // Painted for the reach past what shows, and 3 pixels more for the passes' sampling: left of the clip and
    // right of the window. Above and below, what shows already ends where the content's spread does.
    assert_eq!((blur.x, blur.y), (20 - 27, 30 - 27));
    assert_eq!(
      (blur.x + blur.width as i32, blur.y + blur.height as i32),
      (100 + 27, 60 + 27)
    );

    let unblurred = OpacityGroup { blur: 0.0, ..group };
    let (bounds, blur) = blurred_bounds(&unblurred, [20.0, 30.0, 200.0, 60.0], [100.0, 100.0], 2.0);
    assert!(blur.is_none());
    assert_eq!((bounds.x, bounds.width), (20, 80));
  }

  #[test]
  fn without_layers_the_window_paints_every_draw_in_one_step() {
    assert_eq!(plan(&[0, 1, 2], &[]), vec![(None, vec![LayerStep::Draws(0..3)])]);
  }

  #[test]
  fn nested_layers_are_painted_before_the_targets_they_composite_into() {
    // Orders: 0 | 1 [2 (3 4) 5] | 6, with a draw per order and two at 3.
    let orders = [0, 1, 2, 3, 3, 4, 5, 6];
    let layers = [layer(1, 6, 0.5), layer(3, 5, 0.4)];
    assert_eq!(
      plan(&orders, &layers),
      vec![
        (Some(1), vec![LayerStep::Draws(3..6)]),
        (
          Some(0),
          vec![LayerStep::Draws(1..3), LayerStep::Composite(0), LayerStep::Draws(6..7)]
        ),
        (
          None,
          vec![LayerStep::Draws(0..1), LayerStep::Composite(1), LayerStep::Draws(7..8)]
        ),
      ]
    );
  }

  #[test]
  fn an_invisible_layer_drops_its_draws_and_inner_layers() {
    let orders = [0, 1, 2, 3];
    let layers = [layer(1, 3, 0.0), layer(2, 3, 0.5)];
    assert_eq!(
      plan(&orders, &layers),
      vec![(None, vec![LayerStep::Draws(0..1), LayerStep::Draws(3..4)])]
    );
  }

  #[test]
  fn a_layer_without_draws_is_skipped() {
    // Its quads were culled: no draw has an order in 2..4.
    let orders = [0, 1, 5];
    let layers = [layer(2, 4, 0.5)];
    assert_eq!(
      plan(&orders, &layers),
      vec![(None, vec![LayerStep::Draws(0..2), LayerStep::Draws(2..3)])]
    );
  }

  #[test]
  fn bounds_cover_transformed_content_in_whole_pixels_within_clip_and_viewport() {
    let identity = [1.0, 0.0, 0.0, 1.0];
    let extent = transformed_extent(10.25, 20.5, 30.0, 10.0, identity, [15.0, 5.0], 0.0);
    assert_eq!(extent, [10.25, 20.5, 40.25, 30.5]);
    assert_eq!(
      pixel_bounds(extent, [100.0, 100.0]),
      LayerBounds {
        x: 10,
        y: 20,
        width: 31,
        height: 11
      }
    );
    // A quarter turn around the box's centre swaps its extent.
    let turned = transformed_extent(0.0, 0.0, 40.0, 10.0, [0.0, 1.0, -1.0, 0.0], [20.0, 5.0], 0.0);
    assert!((turned[0] - 15.0).abs() < 1e-4 && (turned[2] - 25.0).abs() < 1e-4);
    assert!((turned[1] + 15.0).abs() < 1e-4 && (turned[3] - 25.0).abs() < 1e-4);
    let clip = ClipRect {
      x: 12.0,
      y: 0.0,
      width: 8.0,
      height: 200.0,
      active: true,
      border_radius: None,
    };
    assert_eq!(clipped(extent, clip), [12.0, 20.5, 20.0, 30.5]);
    assert_eq!(pixel_bounds([90.0, 90.0, 120.0, 95.0], [100.0, 100.0]).width, 10);
    assert!(pixel_bounds([120.0, 0.0, 130.0, 10.0], [100.0, 100.0]).is_empty());
  }
}
