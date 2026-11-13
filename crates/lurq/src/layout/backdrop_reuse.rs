//! Keeping a backdrop blur from one frame to the next.
//!
//! A backdrop blur (see [`crate::layout::backdrop`]) copies the pixels its
//! target holds around the node's box and blurs the copy. A frame in which
//! those pixels are the ones of the frame before gets the same blur, so a
//! render engine keeps the blurred texture and puts it back the next frame
//! without copying or blurring anything, and without stopping the target's
//! pass.
//!
//! Whether the pixels are the same is told from the frame's commands, never
//! from the pixels. [`BackdropInputs`] writes down, for each blur of a frame,
//! everything that decides the pixels it reads (its input, see
//! [`BackdropInputs::describe`]), and [`BackdropCache`] keeps what was written
//! down with each kept texture: the texture is put back only while the two are
//! equal, byte for byte. What cannot be told from a command (an image whose
//! pixels its owner changes: a video frame, a canvas, an external texture)
//! makes the blur one that is never kept.
//!
//! Only the blurred pixels are kept. How they are put back (the node's box
//! and corner radii, its opacity, the clip it is under) is applied every
//! frame, so a glass that fades in over a still backdrop is blurred once.

use std::{cell::Cell, ops::Range};

use crate::layout::{
  backdrop::BackdropRegion,
  opacity_layer::{
    Extent, LayerBlur, LayerBounds, LayerCmd, LayerPlan, LayerStep, LayerTarget, RoundedBox, clipped, glyph_extent,
    glyph_reach, rect_extent,
  },
  quad::ClipRect,
  render_list::{GlyphCmd, RectCmd, RectShadow, RenderGradient, RenderGradientStop, RenderList, StrokeStyle},
};

/// The most bytes one blur's input may take; a blur with more behind it is
/// blurred every frame.
const MAX_INPUT_BYTES: usize = 1 << 20;
/// The most blurs a window keeps the result of.
const MAX_KEPT: usize = 64;
/// The bytes the kept textures of the smallest window may hold.
const MIN_BUDGET: usize = 1 << 20;

/// A draw of a frame, as a render engine orders them for painting.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PaintedDraw {
  Rect(usize),
  /// A run of glyphs of one order under one clip.
  Glyphs {
    start: usize,
    count: usize,
  },
  #[cfg(feature = "raster")]
  Image(usize),
  #[cfg(feature = "svg")]
  Svg(usize),
}

/// The regions of a frame's backdrop blurs (`regions`, by their index in
/// [`RenderList::backdrops`]): each in the target its step of `plan` is in,
/// the window of `window` pixels or a layer. `None` for a blur that changes
/// nothing there, and for one the plan left out.
pub(crate) fn backdrop_regions(
  list: &RenderList,
  plan: &LayerPlan,
  window: [u32; 2],
  regions: &mut Vec<Option<BackdropRegion>>,
) {
  regions.clear();
  regions.resize(list.backdrops.len(), None);
  for target in plan.targets() {
    let (origin, size) = match target.layer {
      Some(layer) => list.layers[layer].painted(),
      None => ([0, 0], window),
    };
    for step in plan.steps(target) {
      if let LayerStep::Backdrop(index) = step {
        regions[*index] = BackdropRegion::new(&list.backdrops[*index], origin, size);
      }
    }
  }
}

/// The inputs of a frame's backdrop blurs, by their index in
/// [`RenderList::backdrops`].
#[derive(Default)]
pub(crate) struct BackdropInputs {
  inputs: Vec<Input>,
  /// The pixels each draw of the frame can paint, by its index in paint
  /// order; found when a blur first asks, since several blurs ask for the
  /// same draws.
  extents: Vec<Cell<Option<Extent>>>,
}

#[derive(Default)]
struct Input {
  bytes: Vec<u8>,
  /// Whether `bytes` say everything about the pixels the blur reads. False
  /// for a blur the frame does not run, and for one that reads pixels no
  /// command decides.
  known: bool,
}

impl BackdropInputs {
  /// Writes down the input of every backdrop blur of a frame: `regions` are
  /// the blurs' regions (`None` for one that changes nothing), `plan` the
  /// frame's plan over the `draws` draws `draw` gives by their index in paint
  /// order, and `window` the window's size in pixels.
  ///
  /// The input of a blur is what decides the pixels it copies out of its
  /// target (the rectangle of its [`LayerBlur`]):
  ///
  /// - the target: the window with its size and clear colour, or the layer the blur is in with the rectangle it is
  ///   painted for;
  /// - the rectangle copied and the blur's deviation;
  /// - every draw painted into the target before the blur whose painted pixels can reach the rectangle, in paint order,
  ///   with all it is drawn from: its geometry, colours, gradient, shadow, transform and clip; a glyph's place in the
  ///   atlas, and the atlas's version and size; an image's id, frame and version; an SVG's mesh;
  /// - every layer composited into the target before the blur that shows inside the rectangle, with everything painted
  ///   into it (its draws, the layers inside it and their blurs, and its backdrop blurs);
  /// - every backdrop blur run on the target before this one that changes pixels inside the rectangle, with how it is
  ///   put back and its own input.
  ///
  /// A draw is left out when it cannot reach the rectangle, so a change next
  /// to the glass, or one painted after it, leaves the input as it was; so
  /// does a draw that is added or removed elsewhere, since the orders of the
  /// draws are not part of it, only their sequence.
  ///
  /// An image with pixels of its owner's (a video frame, a canvas, an
  /// external texture) can change without its command changing: a blur that
  /// can reach one has no known input. Nor has one whose input would take
  /// more than [`MAX_INPUT_BYTES`].
  pub(crate) fn describe(
    &mut self,
    list: &RenderList,
    plan: &LayerPlan,
    draws: usize,
    draw: impl Fn(usize) -> PaintedDraw,
    regions: &[Option<BackdropRegion>],
    window: [u32; 2],
  ) {
    self.inputs.resize_with(regions.len(), Input::default);
    for input in &mut self.inputs {
      input.bytes.clear();
      input.known = false;
    }
    if regions.iter().all(Option::is_none) {
      return;
    }
    self.extents.clear();
    self.extents.resize(draws, Cell::new(None));
    let frame = Frame {
      list,
      plan,
      draw: &draw,
      extents: &self.extents,
      regions,
      window,
    };
    for target in plan.targets() {
      let steps = plan.steps(target);
      for (at, step) in steps.iter().enumerate() {
        let LayerStep::Backdrop(index) = step else {
          continue;
        };
        let Some(blur) = regions
          .get(*index)
          .copied()
          .flatten()
          .and_then(|region| region.layer.blur)
        else {
          continue;
        };
        let mut notes = Notes {
          bytes: std::mem::take(&mut self.inputs[*index].bytes),
          atlas_noted: false,
        };
        let known = frame.note_input(&mut notes, &self.inputs, target, &steps[..at], &blur)
          && notes.bytes.len() <= MAX_INPUT_BYTES;
        if !known {
          notes.bytes.clear();
        }
        self.inputs[*index] = Input {
          bytes: notes.bytes,
          known,
        };
      }
    }
  }

  /// The input of the blur `index`; `None` when it is not known, and the
  /// blur has to run every frame.
  pub(crate) fn input(&self, index: usize) -> Option<&[u8]> {
    self
      .inputs
      .get(index)
      .filter(|input| input.known)
      .map(|input| input.bytes.as_slice())
  }
}

/// A frame being described.
struct Frame<'a, D> {
  list: &'a RenderList,
  plan: &'a LayerPlan,
  draw: &'a D,
  extents: &'a [Cell<Option<Extent>>],
  regions: &'a [Option<BackdropRegion>],
  window: [u32; 2],
}

impl<D: Fn(usize) -> PaintedDraw> Frame<'_, D> {
  /// Notes the input of a blur that copies `blur`'s rectangle out of
  /// `target` once `before`, the steps of the target before it, are painted.
  /// `inputs` holds the inputs of the blurs before it. Returns whether the
  /// notes say everything.
  fn note_input(
    &self,
    notes: &mut Notes,
    inputs: &[Input],
    target: &LayerTarget,
    before: &[LayerStep],
    blur: &LayerBlur,
  ) -> bool {
    match target.layer {
      Some(layer) => {
        // A layer starts transparent; its draws are cut to what it is painted for.
        let (origin, size) = self.list.layers[layer].painted();
        notes.tag(b'L');
        notes.i32s(&origin);
        notes.u32s(&size);
      }
      None => {
        let clear = self.list.clear_color;
        notes.tag(b'W');
        notes.u32s(&self.window);
        notes
          .bytes
          .extend_from_slice(&[clear.r(), clear.g(), clear.b(), clear.a()]);
      }
    }
    notes.f32(blur.sigma);
    notes.i32s(&[blur.x, blur.y]);
    notes.u32s(&[blur.width, blur.height]);
    let copied = [
      blur.x as f32,
      blur.y as f32,
      (blur.x + blur.width as i32) as f32,
      (blur.y + blur.height as i32) as f32,
    ];
    for step in before {
      let known = match step {
        LayerStep::Draws(range) => self.note_draws(notes, range.clone(), Some(copied)),
        LayerStep::Composite(child) => {
          let child = &self.plan.targets()[*child];
          match child.layer.map(|layer| &self.list.layers[layer]) {
            Some(layer) if touches(bounds_extent(layer), copied) => self.note_layer(notes, child, layer),
            _ => true,
          }
        }
        LayerStep::Backdrop(index) => match self.regions.get(*index).copied().flatten() {
          Some(region) if touches(bounds_extent(&region.layer), copied) => {
            // What it leaves in the rectangle follows from everything it
            // read, which may lie outside the rectangle.
            let Some(input) = inputs.get(*index).filter(|input| input.known) else {
              return false;
            };
            notes.tag(b'B');
            notes.region(&region);
            notes.u32(input.bytes.len() as u32);
            notes.bytes.extend_from_slice(&input.bytes);
            true
          }
          _ => true,
        },
      };
      if !known || notes.bytes.len() > MAX_INPUT_BYTES {
        return false;
      }
    }
    true
  }

  /// Notes everything painted into the layer `target` paints, as the layer
  /// is composited. Returns whether the notes say everything.
  fn note_layer(&self, notes: &mut Notes, target: &LayerTarget, layer: &LayerCmd) -> bool {
    notes.tag(b'C');
    notes.layer(layer);
    for step in self.plan.steps(target) {
      let known = match step {
        LayerStep::Draws(range) => self.note_draws(notes, range.clone(), None),
        LayerStep::Composite(child) => {
          let child = &self.plan.targets()[*child];
          match child.layer.map(|layer| &self.list.layers[layer]) {
            Some(layer) => self.note_layer(notes, child, layer),
            None => true,
          }
        }
        // It reads the layer alone, and everything in the layer is noted.
        LayerStep::Backdrop(index) => {
          if let Some(region) = self.regions.get(*index).copied().flatten() {
            notes.tag(b'B');
            notes.region(&region);
          }
          true
        }
      };
      if !known || notes.bytes.len() > MAX_INPUT_BYTES {
        return false;
      }
    }
    notes.tag(b'c');
    true
  }

  /// Notes the draws `range`: those that can reach `within`, or all of them.
  /// Returns false at a draw whose pixels no command decides.
  fn note_draws(&self, notes: &mut Notes, range: Range<usize>, within: Option<Extent>) -> bool {
    for index in range {
      let draw = (self.draw)(index);
      if within.is_some_and(|within| !self.reaches(index, draw, within)) {
        continue;
      }
      match draw {
        PaintedDraw::Rect(index) => notes.rect(&self.list.rects[index]),
        PaintedDraw::Glyphs { start, count } => {
          let reaches = |glyph: &&GlyphCmd| within.is_none_or(|within| touches(glyph_bounds(glyph), within));
          for glyph in self.list.glyphs[start..start + count].iter().filter(reaches) {
            if !std::mem::replace(&mut notes.atlas_noted, true) {
              // A glyph is the atlas's pixels at its place.
              notes.tag(b'A');
              notes.u64(self.list.atlas.version);
              notes.u32s(&[self.list.atlas.width, self.list.atlas.height]);
            }
            notes.glyph(glyph);
          }
        }
        #[cfg(feature = "raster")]
        PaintedDraw::Image(index) => {
          let image = &self.list.images[index];
          if image.native.is_some() {
            return false;
          }
          notes.image(image);
        }
        #[cfg(feature = "svg")]
        PaintedDraw::Svg(index) => notes.svg(&self.list.svgs[index]),
      }
    }
    true
  }

  /// Whether `draw`, the draw `index`, can paint a pixel of `within`.
  fn reaches(&self, index: usize, draw: PaintedDraw, within: Extent) -> bool {
    // Most draws are cut by a clip that ends before the rectangle, which is
    // told without finding what they paint.
    let clip = match draw {
      PaintedDraw::Rect(index) => self.list.rects[index].clip,
      PaintedDraw::Glyphs { start, .. } => self
        .list
        .glyphs
        .get(start)
        .map_or_else(ClipRect::default, |glyph| glyph.clip),
      #[cfg(feature = "raster")]
      PaintedDraw::Image(index) => self.list.images[index].clip,
      #[cfg(feature = "svg")]
      PaintedDraw::Svg(index) => self.list.svgs[index].clip,
    };
    // A pixel more than the clip on every side, which saves rounding it.
    let around = [
      clip.x - 1.0,
      clip.y - 1.0,
      clip.x + clip.width + 1.0,
      clip.y + clip.height + 1.0,
    ];
    if clip.active && !touches(around, within) {
      return false;
    }
    touches(self.extent(index, draw), within)
  }

  /// The pixels `draw`, the draw `index`, can paint: for a run of glyphs,
  /// those of all of them, inside the clip the run is under.
  fn extent(&self, index: usize, draw: PaintedDraw) -> Extent {
    let Some(known) = self.extents.get(index) else {
      return EVERYWHERE;
    };
    if let Some(extent) = known.get() {
      return extent;
    }
    let extent = match draw {
      PaintedDraw::Rect(index) => rect_extent(&self.list.rects[index]),
      PaintedDraw::Glyphs { start, count } => {
        let glyphs = &self.list.glyphs[start..start + count];
        let all = glyphs.iter().map(glyph_bounds).fold(NOWHERE, union);
        glyphs.first().map_or(NOWHERE, |glyph| clipped(all, glyph.clip))
      }
      #[cfg(feature = "raster")]
      PaintedDraw::Image(index) => crate::layout::opacity_layer::image_extent(&self.list.images[index]),
      #[cfg(feature = "svg")]
      PaintedDraw::Svg(index) => crate::layout::opacity_layer::svg_extent(&self.list.svgs[index]),
    };
    known.set(Some(extent));
    extent
  }
}

/// The extent of a draw that is not known, or of no clip: everything.
const EVERYWHERE: Extent = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::INFINITY, f32::INFINITY];
/// The extent of nothing.
const NOWHERE: Extent = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

/// The extent that holds both.
fn union(a: Extent, b: Extent) -> Extent {
  [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// The pixels a glyph can paint, without its clip: [`glyph_extent`] at
/// least, and without the transform's arithmetic for a glyph that has none.
fn glyph_bounds(glyph: &GlyphCmd) -> Extent {
  if glyph.transform != [1.0, 0.0, 0.0, 1.0] {
    return glyph_extent(glyph);
  }
  let reach = glyph_reach(glyph);
  [
    glyph.x - reach,
    glyph.y - reach,
    glyph.x + glyph.width + reach,
    glyph.y + glyph.height + reach,
  ]
}

/// Whether two extents share a pixel.
fn touches(a: Extent, b: Extent) -> bool {
  a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

/// Where a layer shows.
fn bounds_extent(layer: &LayerCmd) -> Extent {
  let bounds = layer.bounds;
  [
    bounds.x as f32,
    bounds.y as f32,
    (bounds.x + bounds.width) as f32,
    (bounds.y + bounds.height) as f32,
  ]
}

/// The bytes of an input. Every value is written whole, floats by their
/// bits, so equal bytes are equal commands.
struct Notes {
  bytes: Vec<u8>,
  /// The glyph atlas is noted with the first glyph.
  atlas_noted: bool,
}

impl Notes {
  fn tag(&mut self, tag: u8) {
    self.bytes.push(tag);
  }

  fn flag(&mut self, flag: bool) {
    self.bytes.push(u8::from(flag));
  }

  fn u32(&mut self, value: u32) {
    self.bytes.extend_from_slice(&value.to_ne_bytes());
  }

  fn u64(&mut self, value: u64) {
    self.bytes.extend_from_slice(&value.to_ne_bytes());
  }

  fn f32(&mut self, value: f32) {
    self.u32(value.to_bits());
  }

  fn u32s(&mut self, values: &[u32]) {
    self.bytes.extend_from_slice(bytemuck::cast_slice(values));
  }

  fn i32s(&mut self, values: &[i32]) {
    self.bytes.extend_from_slice(bytemuck::cast_slice(values));
  }

  fn f32s(&mut self, values: &[f32]) {
    self.bytes.extend_from_slice(bytemuck::cast_slice(values));
  }

  fn color(&mut self, color: crate::node::color::Color) {
    self
      .bytes
      .extend_from_slice(&[color.r(), color.g(), color.b(), color.a()]);
  }

  // The functions from here on name every field of what they write down, without `..`: a field added to a command
  // later does not compile here until it is written down or left out by name.

  fn clip(&mut self, clip: &ClipRect) {
    let &ClipRect {
      x,
      y,
      width,
      height,
      active,
      border_radius,
    } = clip;
    self.flag(active);
    if !active {
      return;
    }
    self.f32s(&[x, y, width, height]);
    self.flag(border_radius.is_some());
    if let Some(radius) = border_radius {
      let crate::node::border::BorderRadius {
        top_left,
        top_right,
        bottom_right,
        bottom_left,
      } = radius;
      self.f32s(&[top_left, top_right, bottom_right, bottom_left]);
    }
  }

  fn rounded(&mut self, rounded: Option<&RoundedBox>) {
    self.flag(rounded.is_some());
    if let Some(&RoundedBox {
      center,
      half,
      radii,
      inverse,
      scale,
    }) = rounded
    {
      self.f32s(&center);
      self.f32s(&half);
      self.f32s(&radii);
      self.f32s(&inverse);
      self.f32(scale);
    }
  }

  fn rect(&mut self, rect: &RectCmd) {
    let RectCmd {
      // An order places the draw among the others, which the sequence of the notes does.
      order: _,
      x,
      y,
      width,
      height,
      color,
      radii,
      stroke,
      stroke_color,
      stroke_style: StrokeStyle { dash, radii_at },
      transform,
      transform_origin,
      clip,
      gradient,
      shadow,
    } = rect;
    self.tag(b'r');
    self.f32s(&[*x, *y, *width, *height]);
    self.color(*color);
    self.f32s(radii);
    self.f32s(stroke);
    self.color(*stroke_color);
    self.f32s(dash);
    self.f32(*radii_at);
    self.f32s(transform);
    self.f32s(transform_origin);
    self.clip(clip);
    self.flag(gradient.is_some());
    if let Some(RenderGradient {
      kind,
      flags,
      dir,
      center,
      from_angle,
      stops,
    }) = gradient
    {
      self.u32s(&[*kind, *flags, stops.len() as u32]);
      self.f32s(dir);
      self.f32s(center);
      self.f32(*from_angle);
      for RenderGradientStop { color, position } in stops {
        self.f32s(color);
        self.f32(*position);
      }
    }
    self.flag(shadow.is_some());
    if let Some(RectShadow {
      inset,
      offset,
      spread,
      sigma,
      shape_radii,
    }) = shadow
    {
      self.flag(*inset);
      self.f32s(offset);
      self.f32s(&[*spread, *sigma]);
      self.f32s(shape_radii);
    }
  }

  fn glyph(&mut self, glyph: &GlyphCmd) {
    let &GlyphCmd {
      order: _,
      x,
      y,
      width,
      height,
      color,
      atlas_min,
      atlas_max,
      transform,
      transform_origin,
      sharpness,
      color_glyph,
      shadow_sigma,
      ref clip,
    } = glyph;
    self.tag(b'g');
    self.f32s(&[
      x,
      y,
      width,
      height,
      color[0],
      color[1],
      color[2],
      color[3],
      atlas_min[0],
      atlas_min[1],
      atlas_max[0],
      atlas_max[1],
      transform[0],
      transform[1],
      transform[2],
      transform[3],
      transform_origin[0],
      transform_origin[1],
      sharpness,
      shadow_sigma,
    ]);
    self.flag(color_glyph);
    self.clip(clip);
  }

  /// An image of pixels the engine uploads: it shows them again for the
  /// same id, frame and version.
  #[cfg(feature = "raster")]
  fn image(&mut self, image: &crate::images::ImageCmd) {
    let crate::images::ImageCmd {
      order: _,
      x,
      y,
      width,
      height,
      image_id,
      frame_index,
      version,
      // The pixels themselves, which the id, the frame and the version stand for.
      data: _,
      animation_frames,
      // A native texture's pixels change without the command changing: `Frame::note_draws` gives up on an input
      // that reaches one.
      native: _,
      image_width,
      image_height,
      image_format,
      uv_min,
      uv_max,
      radii,
      transform,
      transform_origin,
      clip,
      opacity,
    } = image;
    self.tag(b'i');
    self.f32s(&[*x, *y, *width, *height]);
    self.u64(*image_id);
    self.u64(*frame_index as u64);
    self.u64(*version);
    self.u32s(&[*image_width, *image_height]);
    self.tag(match image_format {
      crate::images::ImagePixelFormat::Rgba8 => 0,
      crate::images::ImagePixelFormat::Nv12 => 1,
    });
    self.u64(animation_frames.as_ref().map_or(u64::MAX, |frames| frames.len() as u64));
    self.f32s(uv_min);
    self.f32s(uv_max);
    self.f32s(radii);
    self.f32s(transform);
    self.f32s(transform_origin);
    self.clip(clip);
    self.f32(*opacity);
  }

  #[cfg(feature = "svg")]
  fn svg(&mut self, svg: &crate::svg::SvgCmd) {
    let crate::svg::SvgCmd {
      order: _,
      x,
      y,
      width,
      height,
      svg_id,
      mesh,
      clip,
    } = svg;
    let crate::svg::tessellate::TessellatedSvg { vertices, indices } = &**mesh;
    self.tag(b's');
    self.f32s(&[*x, *y, *width, *height]);
    self.u64(*svg_id);
    self.clip(clip);
    self.u32s(&[vertices.len() as u32, indices.len() as u32]);
    for crate::svg::tessellate::SvgVertex { position, color } in vertices {
      self.f32s(position);
      self.f32s(color);
    }
    self.u32s(indices);
  }

  /// A layer as it is composited.
  fn layer(&mut self, layer: &LayerCmd) {
    let &LayerCmd {
      // The orders say which draws are in the layer, and those are written down themselves.
      start_order: _,
      end_order: _,
      opacity,
      bounds,
      blur,
    } = layer;
    let LayerBounds { x, y, width, height } = bounds;
    self.f32(opacity);
    self.u32s(&[x, y, width, height]);
    self.flag(blur.is_some());
    if let Some(LayerBlur {
      sigma,
      x,
      y,
      width,
      height,
      clip,
    }) = blur
    {
      self.f32(sigma);
      self.i32s(&[x, y]);
      self.u32s(&[width, height]);
      self.rounded(clip.as_ref());
    }
  }

  /// A backdrop blur as it is run and put back.
  fn region(&mut self, region: &BackdropRegion) {
    let BackdropRegion { layer, mask, clip } = region;
    self.layer(layer);
    self.rounded(Some(mask));
    self.rounded(clip.as_ref());
  }
}

/// The blurs a window keeps the result of: for each, the texture (by the id
/// its render engine gave it) and the input it was blurred from.
///
/// A frame asks with [`Self::begin_frame`] what to do for each of its blurs,
/// has the textures of the blurs it starts keeping entered with
/// [`Self::keep`], and says with [`Self::commit`] that its commands were
/// handed to the GPU; until then nothing it blurred counts as kept.
#[derive(Default)]
pub(crate) struct BackdropCache {
  kept: Vec<Kept>,
  /// The bytes the kept textures may hold together.
  budget: usize,
}

struct Kept {
  /// The first bytes of the input: the target, the rectangle copied and the
  /// deviation. A blur with the same ones is the same glass at the same
  /// place, whose texture has the size this one needs.
  place: Vec<u8>,
  input: Vec<u8>,
  texture: u64,
  bytes: usize,
  /// Taken by a blur of the frame being built.
  taken: bool,
  /// Blurred by a frame that has not been committed: no blur may be put back
  /// from it.
  pending: bool,
}

/// What a frame does for one of its backdrop blurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackdropUse {
  /// Its input is the kept one: put the kept texture back.
  Reuse { texture: u64 },
  /// Blur again, into the texture kept for this glass.
  Refresh { texture: u64 },
  /// Blur into a texture of the frame's; [`BackdropCache::keep`] may then
  /// keep it.
  Blur { keep: bool },
}

impl BackdropCache {
  /// The most the kept textures of a `window` hold: twice the bytes of the
  /// window's own pixels, and no less than [`MIN_BUDGET`], which a small
  /// window's few blurs need for textures that come in steps of a size.
  pub(crate) fn budget_of(window: [u32; 2]) -> usize {
    (window[0] as usize * window[1] as usize * 4 * 2).max(MIN_BUDGET)
  }

  /// Decides what a frame does for each of its blurs (`uses`, by the blurs'
  /// index; `None` for one without a region) and forgets the kept results no
  /// blur of the frame takes, whose textures' ids go to `released`.
  ///
  /// A blur takes the kept result of its place; two blurs at one place take
  /// one each. It is put back when its input is the kept one, and blurred
  /// again otherwise, into the same texture. A blur without a known input
  /// keeps nothing.
  pub(crate) fn begin_frame(
    &mut self,
    regions: &[Option<BackdropRegion>],
    inputs: &BackdropInputs,
    budget: usize,
    uses: &mut Vec<Option<BackdropUse>>,
    released: &mut Vec<u64>,
  ) {
    self.budget = budget;
    uses.clear();
    released.clear();
    for kept in &mut self.kept {
      kept.taken = false;
      // The frame that blurred it never reached the GPU.
      if std::mem::take(&mut kept.pending) {
        kept.input.clear();
      }
    }
    // First the blurs whose input is a kept one, so that a blur that has to
    // run again does not take their result for its place.
    for (index, region) in regions.iter().enumerate() {
      uses.push(region.map(|_| match inputs.input(index) {
        None => BackdropUse::Blur { keep: false },
        Some(input) => match self.kept.iter_mut().find(|kept| !kept.taken && kept.input == input) {
          Some(kept) => {
            kept.taken = true;
            BackdropUse::Reuse { texture: kept.texture }
          }
          None => BackdropUse::Blur { keep: true },
        },
      }));
    }
    for (index, used) in uses.iter_mut().enumerate() {
      let (Some(BackdropUse::Blur { keep: true }), Some(input)) = (*used, inputs.input(index)) else {
        continue;
      };
      let at_place = self
        .kept
        .iter_mut()
        .find(|kept| !kept.taken && input.starts_with(&kept.place));
      if let Some(kept) = at_place {
        kept.taken = true;
        kept.input.clear();
        kept.input.extend_from_slice(input);
        kept.pending = true;
        *used = Some(BackdropUse::Refresh { texture: kept.texture });
      }
    }
    self.kept.retain(|kept| {
      if !kept.taken {
        released.push(kept.texture);
      }
      kept.taken
    });
  }

  /// Keeps `texture`, of `bytes` bytes, as the result of the frame's blur
  /// `index`, which [`Self::begin_frame`] answered with
  /// [`BackdropUse::Blur`]. Returns whether it is kept: not when that would
  /// take the kept textures past their budget or their number.
  pub(crate) fn keep(&mut self, inputs: &BackdropInputs, index: usize, texture: u64, bytes: usize) -> bool {
    let Some(input) = inputs.input(index) else {
      return false;
    };
    if self.kept.len() >= MAX_KEPT || self.bytes() + bytes > self.budget {
      return false;
    }
    self.kept.push(Kept {
      place: input[..place_len(input)].to_vec(),
      input: input.to_vec(),
      texture,
      bytes,
      taken: true,
      pending: true,
    });
    true
  }

  /// The frame's commands were handed to the GPU: what it blurred is kept.
  pub(crate) fn commit(&mut self) {
    for kept in &mut self.kept {
      kept.pending = false;
    }
  }

  /// Forgets every kept result; their textures' ids go to `released`.
  pub(crate) fn clear(&mut self, released: &mut Vec<u64>) {
    released.extend(self.kept.drain(..).map(|kept| kept.texture));
  }

  /// The number of kept textures.
  pub(crate) fn len(&self) -> usize {
    self.kept.len()
  }

  /// The bytes the kept textures hold.
  pub(crate) fn bytes(&self) -> usize {
    self.kept.iter().map(|kept| kept.bytes).sum()
  }
}

/// The length of an input's first part, which [`Frame::note_input`] writes
/// before anything painted: the target, the deviation and the rectangle.
fn place_len(input: &[u8]) -> usize {
  let target = match input.first() {
    // The window's size and its clear colour.
    Some(b'W') => 1 + 8 + 4,
    // The layer's origin and size.
    _ => 1 + 8 + 8,
  };
  (target + 4 + 8 + 8).min(input.len())
}

#[cfg(test)]
mod tests;
