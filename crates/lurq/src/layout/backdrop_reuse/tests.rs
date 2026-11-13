use std::sync::Arc;

use super::*;
use crate::{
  layout::{
    backdrop::BackdropCmd,
    opacity_layer::LayerBounds,
    render_list::{GlyphAtlas, RenderGradient, RenderGradientStop},
  },
  node::color::Color,
};

const WINDOW: [u32; 2] = [400, 300];
/// The glass of most tests: x, y, width, height. At sigma 4 its blur copies
/// 15 pixels around it.
const GLASS: [f32; 4] = [100.0, 100.0, 80.0, 60.0];

fn rect(order: usize, [x, y, width, height]: [f32; 4]) -> RectCmd {
  RectCmd {
    order,
    x,
    y,
    width,
    height,
    color: Color::new(200, 40, 10, 255),
    radii: [0.0; 4],
    stroke: [0.0; 4],
    stroke_color: Color::new(0, 0, 0, 0),
    stroke_style: crate::layout::render_list::StrokeStyle::default(),
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

fn glyph(order: usize, x: f32, y: f32) -> GlyphCmd {
  GlyphCmd {
    order,
    x,
    y,
    width: 8.0,
    height: 12.0,
    color: [1.0; 4],
    atlas_min: [0.0, 0.0],
    atlas_max: [8.0, 12.0],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0; 2],
    sharpness: 1.0,
    color_glyph: false,
    shadow_sigma: 0.0,
    clip: ClipRect::default(),
  }
}

fn glass(order: usize, [x, y, width, height]: [f32; 4], sigma: f32) -> BackdropCmd {
  BackdropCmd {
    order,
    x,
    y,
    width,
    height,
    radii: [8.0; 4],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [width * 0.5, height * 0.5],
    clip: ClipRect::default(),
    sigma,
    opacity: 1.0,
  }
}

fn layer(start_order: usize, end_order: usize, [x, y, width, height]: [u32; 4]) -> LayerCmd {
  LayerCmd {
    start_order,
    end_order,
    opacity: 0.5,
    bounds: LayerBounds { x, y, width, height },
    blur: None,
  }
}

fn list(rects: Vec<RectCmd>, backdrops: Vec<BackdropCmd>) -> RenderList {
  RenderList {
    clear_color: Color::new(30, 30, 30, 255),
    rects,
    glyphs: Vec::new(),
    #[cfg(feature = "raster")]
    images: Vec::new(),
    #[cfg(feature = "svg")]
    svgs: Vec::new(),
    layers: Vec::new(),
    backdrops,
    atlas: GlyphAtlas {
      data: Arc::from([255_u8; 4 * 4 * 4].as_slice()),
      width: 4,
      height: 4,
      version: 1,
      dirty_rects: Arc::from(Vec::new()),
      dirty_from_version: 0,
    },
  }
}

/// A frame as a render engine sees it: its draws in paint order, its plan
/// and the regions of its backdrop blurs.
struct Scene {
  inputs: BackdropInputs,
  regions: Vec<Option<BackdropRegion>>,
}

impl Scene {
  fn new(list: &RenderList) -> Self {
    Self::in_window(list, WINDOW)
  }

  fn in_window(list: &RenderList, window: [u32; 2]) -> Self {
    let mut draws: Vec<(usize, PaintedDraw)> = Vec::new();
    for (index, rect) in list.rects.iter().enumerate() {
      draws.push((rect.order, PaintedDraw::Rect(index)));
    }
    // Glyphs of one order are a run, as the engines draw them.
    let mut start = 0;
    while start < list.glyphs.len() {
      let order = list.glyphs[start].order;
      let count = list.glyphs[start..]
        .iter()
        .take_while(|glyph| glyph.order == order)
        .count();
      draws.push((order, PaintedDraw::Glyphs { start, count }));
      start += count;
    }
    #[cfg(feature = "raster")]
    for (index, image) in list.images.iter().enumerate() {
      draws.push((image.order, PaintedDraw::Image(index)));
    }
    #[cfg(feature = "svg")]
    for (index, svg) in list.svgs.iter().enumerate() {
      draws.push((svg.order, PaintedDraw::Svg(index)));
    }
    draws.sort_by_key(|(order, _)| *order);
    let mut plan = LayerPlan::default();
    plan.build(draws.len(), |index| draws[index].0, &list.layers, &list.backdrops);
    let mut regions = Vec::new();
    backdrop_regions(list, &plan, window, &mut regions);
    let mut inputs = BackdropInputs::default();
    inputs.describe(list, &plan, draws.len(), |index| draws[index].1, &regions, window);
    Self { inputs, regions }
  }

  fn input(&self, index: usize) -> Option<Vec<u8>> {
    self.inputs.input(index).map(<[u8]>::to_vec)
  }
}

/// The input of the first backdrop blur of `list`.
fn input(list: &RenderList) -> Option<Vec<u8>> {
  Scene::new(list).input(0)
}

/// A backdrop, a rect under the glass, and the glass at order 2.
fn behind(under: RectCmd) -> RenderList {
  list(
    vec![rect(0, [0.0, 0.0, 400.0, 300.0]), under],
    vec![glass(2, GLASS, 4.0)],
  )
}

fn under() -> RectCmd {
  rect(1, [120.0, 110.0, 20.0, 20.0])
}

#[test]
fn the_same_frame_has_the_same_input() {
  let first = input(&behind(under())).expect("a known input");
  assert_eq!(input(&behind(under())), Some(first));
}

#[test]
fn a_draw_behind_the_glass_that_changes_changes_the_input() {
  type Change = (&'static str, fn(&mut RectCmd));
  let before = input(&behind(under()));
  let changes: [Change; 9] = [
    ("colour", |rect| rect.color = Color::new(200, 40, 11, 255)),
    ("place", |rect| rect.x += 0.25),
    ("size", |rect| rect.height += 1.0),
    ("corners", |rect| rect.radii = [2.0, 0.0, 0.0, 0.0]),
    ("stroke", |rect| {
      rect.stroke = [1.0; 4];
      rect.stroke_color = Color::new(0, 0, 0, 255);
    }),
    ("transform", |rect| rect.transform = [0.0, 1.0, -1.0, 0.0]),
    ("clip", |rect| {
      rect.clip = ClipRect {
        x: 0.0,
        y: 0.0,
        width: 130.0,
        height: 300.0,
        active: true,
        border_radius: None,
      };
    }),
    ("gradient", |rect| {
      rect.gradient = Some(RenderGradient {
        kind: 0,
        flags: 0,
        dir: [0.0, 1.0],
        center: [0.5, 0.5],
        from_angle: 0.0,
        stops: vec![
          RenderGradientStop {
            color: [1.0, 0.0, 0.0, 1.0],
            position: 0.0,
          },
          RenderGradientStop {
            color: [0.0, 0.0, 1.0, 1.0],
            position: 1.0,
          },
        ],
      });
    }),
    ("shadow", |rect| {
      rect.shadow = Some(crate::layout::render_list::RectShadow {
        inset: false,
        offset: [0.0, 2.0],
        spread: 0.0,
        sigma: 3.0,
        shape_radii: [0.0; 4],
      });
    }),
  ];
  for (what, change) in changes {
    let mut changed = under();
    change(&mut changed);
    assert_ne!(input(&behind(changed)), before, "{what}");
  }
  // A stop of a gradient that moves is another gradient.
  let gradient = |position: f32| {
    let mut rect = under();
    rect.gradient = Some(RenderGradient {
      kind: 0,
      flags: 0,
      dir: [0.0, 1.0],
      center: [0.5, 0.5],
      from_angle: 0.0,
      stops: vec![RenderGradientStop {
        color: [1.0, 0.0, 0.0, 1.0],
        position,
      }],
    });
    rect
  };
  assert_ne!(input(&behind(gradient(0.25))), input(&behind(gradient(0.5))));
  // Gone, it changes the input too.
  let without = list(vec![rect(0, [0.0, 0.0, 400.0, 300.0])], vec![glass(1, GLASS, 4.0)]);
  assert_ne!(input(&without), before);
}

#[test]
fn a_draw_beside_the_glass_or_after_it_leaves_the_input_as_it_was() {
  let before = input(&behind(under())).expect("a known input");
  // Far from the glass, whatever it looks like.
  let beside = |color: Color| {
    let mut far = rect(1, [300.0, 20.0, 40.0, 40.0]);
    far.color = color;
    list(
      vec![rect(0, [0.0, 0.0, 400.0, 300.0]), far, {
        let mut under = under();
        under.order = 2;
        under
      }],
      vec![glass(3, GLASS, 4.0)],
    )
  };
  // The draw beside it moved every order after it by one: the sequence
  // behind the glass is the same.
  assert_eq!(input(&beside(Color::new(1, 2, 3, 255))), Some(before.clone()));
  assert_eq!(input(&beside(Color::new(9, 9, 9, 255))), Some(before.clone()));

  // Painted after the glass, over it: the glass's own content.
  let mut over = behind(under());
  over.rects.push(rect(3, [110.0, 105.0, 30.0, 30.0]));
  assert_eq!(input(&over), Some(before.clone()));

  // Cut by its clip to pixels far from the glass.
  let mut clipped = rect(1, [0.0, 0.0, 400.0, 300.0]);
  clipped.clip = ClipRect {
    x: 300.0,
    y: 0.0,
    width: 100.0,
    height: 50.0,
    active: true,
    border_radius: None,
  };
  let with_clipped = list(
    vec![rect(0, [0.0, 0.0, 400.0, 300.0]), clipped, {
      let mut under = under();
      under.order = 2;
      under
    }],
    vec![glass(3, GLASS, 4.0)],
  );
  assert_eq!(input(&with_clipped), Some(before));
}

#[test]
fn a_draw_the_blur_can_reach_past_the_box_is_behind_the_glass() {
  // Sigma 4 copies 15 pixels around the box, which starts at x 100.
  let near = |x: f32, color: Color| {
    let mut near = rect(1, [x, 110.0, 10.0, 10.0]);
    near.color = color;
    behind(near)
  };
  let (red, blue) = (Color::new(255, 0, 0, 255), Color::new(0, 0, 255, 255));
  // Ends at 92: inside the copied pixels.
  assert_ne!(input(&near(82.0, red)), input(&near(82.0, blue)));
  // Ends at 80, with its anti-aliasing at 82: outside them.
  assert_eq!(input(&near(70.0, red)), input(&near(70.0, blue)));
  // A wider blur reaches it.
  let wide = |color: Color| {
    let mut list = near(70.0, color);
    list.backdrops[0].sigma = 12.0;
    list
  };
  assert_ne!(input(&wide(red)), input(&wide(blue)));
}

#[test]
fn glyphs_behind_the_glass_are_the_atlas_at_their_place() {
  let text = |x: f32, version: u64| {
    let mut list = behind(under());
    list.glyphs = vec![glyph(1, x, 120.0), glyph(1, x + 8.0, 120.0)];
    list.atlas.version = version;
    list
  };
  let before = input(&text(110.0, 1)).expect("a known input");
  assert_eq!(input(&text(110.0, 1)), Some(before.clone()));
  assert_ne!(input(&text(111.0, 1)), Some(before.clone()), "a glyph moved");
  let mut recoloured = text(110.0, 1);
  recoloured.glyphs[1].color = [1.0, 0.0, 0.0, 1.0];
  assert_ne!(input(&recoloured), Some(before.clone()), "a glyph's colour");
  let mut other_glyph = text(110.0, 1);
  other_glyph.glyphs[0].atlas_min = [16.0, 0.0];
  other_glyph.glyphs[0].atlas_max = [24.0, 12.0];
  assert_ne!(input(&other_glyph), Some(before.clone()), "another glyph");
  // The atlas changed: its pixels at the glyphs' places may have.
  assert_ne!(input(&text(110.0, 2)), Some(before), "the atlas's version");

  // Text beside the glass is not behind it, whatever the atlas does.
  let beside = input(&text(300.0, 1)).expect("a known input");
  assert_eq!(input(&text(300.0, 2)), Some(beside.clone()));
  assert_eq!(input(&text(310.0, 2)), Some(beside));

  // A run under a clip that ends before the glass is cut before it.
  let clipped = |x: f32| {
    let mut list = text(x, 1);
    for glyph in &mut list.glyphs {
      glyph.clip = ClipRect {
        x: 0.0,
        y: 0.0,
        width: 60.0,
        height: 300.0,
        active: true,
        border_radius: None,
      };
    }
    list
  };
  assert_eq!(input(&clipped(110.0)), input(&clipped(118.0)));
}

#[cfg(feature = "raster")]
fn image(order: usize, version: u64, native: Option<crate::images::NativeImageData>) -> crate::images::ImageCmd {
  crate::images::ImageCmd {
    order,
    x: 110.0,
    y: 110.0,
    width: 40.0,
    height: 30.0,
    image_id: 7,
    frame_index: 0,
    version,
    data: Arc::new(vec![255; 4 * 4 * 4]),
    animation_frames: None,
    native,
    image_width: 4,
    image_height: 4,
    image_format: crate::images::ImagePixelFormat::Rgba8,
    uv_min: [0.0, 0.0],
    uv_max: [1.0, 1.0],
    radii: [0.0; 4],
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [20.0, 15.0],
    clip: ClipRect::default(),
    opacity: 1.0,
  }
}

#[cfg(feature = "raster")]
#[test]
fn an_image_behind_the_glass_is_its_id_frame_and_version() {
  let with = |image: crate::images::ImageCmd| {
    let mut list = behind(under());
    list.images = vec![image];
    list
  };
  let before = input(&with(image(1, 3, None))).expect("a known input");
  // The same pixels in another allocation are the same image.
  assert_eq!(input(&with(image(1, 3, None))), Some(before.clone()));
  assert_ne!(input(&with(image(1, 4, None))), Some(before.clone()), "a new version");
  let mut next_frame = image(1, 3, None);
  next_frame.frame_index = 1;
  assert_ne!(input(&with(next_frame)), Some(before.clone()), "another frame");
  let mut other = image(1, 3, None);
  other.image_id = 8;
  assert_ne!(input(&with(other)), Some(before.clone()), "another image");
  let mut faded = image(1, 3, None);
  faded.opacity = 0.5;
  assert_ne!(input(&with(faded)), Some(before.clone()), "its opacity");

  // Beside the glass it is not part of the input.
  let mut beside = image(1, 3, None);
  beside.x = 300.0;
  let far = input(&with(beside.clone())).expect("a known input");
  beside.version = 9;
  assert_eq!(input(&with(beside)), Some(far));
}

#[cfg(feature = "raster")]
#[test]
fn an_image_of_its_owners_pixels_behind_the_glass_leaves_no_known_input() {
  // A video frame, a canvas or an external texture: its pixels change
  // without its command changing.
  let native = || {
    crate::images::NativeImageData::new(
      4,
      4,
      crate::images::ImagePixelFormat::Rgba8,
      crate::images::NativeImageBackend::Dx12Nv12,
      (),
    )
  };
  let with = |image: crate::images::ImageCmd| {
    let mut list = behind(under());
    list.images = vec![image];
    list
  };
  assert_eq!(input(&with(image(1, 3, Some(native())))), None);
  // Beside the glass it does not matter.
  let mut beside = image(1, 3, Some(native()));
  beside.x = 300.0;
  assert!(input(&with(beside)).is_some());
  // Nor painted over the glass.
  let mut over = image(3, 3, Some(native()));
  over.order = 3;
  assert!(input(&with(over)).is_some());
}

#[test]
fn a_layer_behind_the_glass_is_part_of_the_input_with_all_it_holds() {
  // Orders: 0 the backdrop, [1, 2] a faded layer, 3 the glass.
  let scene = |bounds: [u32; 4], inner: [f32; 4], opacity: f32| {
    let mut list = list(
      vec![
        rect(0, [0.0, 0.0, 400.0, 300.0]),
        rect(1, inner),
        rect(2, [bounds[0] as f32, bounds[1] as f32, 4.0, 4.0]),
      ],
      vec![glass(3, GLASS, 4.0)],
    );
    let mut layer = layer(1, 3, bounds);
    layer.opacity = opacity;
    list.layers = vec![layer];
    list
  };
  // The layer shows inside the copied pixels; its first rect lies outside
  // them. What it holds anywhere can show where it does (a blurred layer
  // moves colour), so all of it counts.
  let touching = [20, 90, 100, 60];
  let before = input(&scene(touching, [30.0, 95.0, 10.0, 10.0], 0.5)).expect("a known input");
  assert_eq!(
    input(&scene(touching, [30.0, 95.0, 10.0, 10.0], 0.5)),
    Some(before.clone())
  );
  assert_ne!(
    input(&scene(touching, [31.0, 95.0, 10.0, 10.0], 0.5)),
    Some(before.clone()),
    "a draw inside the layer"
  );
  assert_ne!(
    input(&scene(touching, [30.0, 95.0, 10.0, 10.0], 0.6)),
    Some(before),
    "the layer's opacity"
  );

  // A layer that shows far from the glass is not behind it.
  let far = [300, 10, 60, 40];
  assert_eq!(
    input(&scene(far, [310.0, 15.0, 10.0, 10.0], 0.5)),
    input(&scene(far, [320.0, 15.0, 10.0, 10.0], 0.9))
  );
}

#[test]
fn a_backdrop_blur_behind_another_passes_its_input_on() {
  // Orders: 0 the backdrop, 1 a rect, 2 a first glass, 3 the glass.
  let scene = |first: [f32; 4], x: f32| {
    list(
      vec![rect(0, [0.0, 0.0, 400.0, 300.0]), rect(1, [x, 110.0, 10.0, 10.0])],
      vec![glass(2, first, 4.0), glass(3, GLASS, 4.0)],
    )
  };
  // The first glass (x 20..90) changes pixels the second copies (from x 85
  // on); the rect at x 30 is behind the first only. The first blur's result
  // inside the second's pixels follows from it all the same.
  let under_both = [20.0, 100.0, 70.0, 60.0];
  let second = |list: &RenderList| Scene::new(list).input(1);
  let before = second(&scene(under_both, 30.0)).expect("a known input");
  assert_eq!(second(&scene(under_both, 30.0)), Some(before.clone()));
  assert_ne!(second(&scene(under_both, 31.0)), Some(before), "behind the first glass");

  // A first glass that changes no pixel the second copies leaves it alone.
  let apart = [20.0, 100.0, 40.0, 60.0];
  assert_eq!(second(&scene(apart, 30.0)), second(&scene(apart, 31.0)));

  // How the first is put back shows in the pixels the second copies.
  let mut faded = scene(under_both, 30.0);
  faded.backdrops[0].opacity = 0.5;
  assert_ne!(second(&faded), second(&scene(under_both, 30.0)));
}

#[cfg(feature = "raster")]
#[test]
fn a_backdrop_blur_without_a_known_input_behind_another_leaves_it_none() {
  let native = crate::images::NativeImageData::new(
    4,
    4,
    crate::images::ImagePixelFormat::Rgba8,
    crate::images::NativeImageBackend::Dx12Nv12,
    (),
  );
  let scene = |first: [f32; 4]| {
    let mut list = list(
      vec![rect(0, [0.0, 0.0, 400.0, 300.0])],
      vec![glass(2, first, 4.0), glass(3, GLASS, 4.0)],
    );
    let mut video = image(1, 0, Some(native.clone()));
    video.x = 25.0;
    video.width = 20.0;
    list.images = vec![video];
    list
  };
  // The video is behind the first glass only.
  let both = Scene::new(&scene([20.0, 100.0, 70.0, 60.0]));
  assert_eq!((both.input(0), both.input(1)), (None, None));
  let apart = Scene::new(&scene([20.0, 100.0, 40.0, 60.0]));
  assert!(apart.input(0).is_none() && apart.input(1).is_some());
}

#[test]
fn the_window_and_what_is_copied_are_part_of_the_input() {
  let before = input(&behind(under()));
  let mut cleared = behind(under());
  cleared.clear_color = Color::new(31, 30, 30, 255);
  assert_ne!(input(&cleared), before, "the clear colour");
  assert_ne!(
    Scene::in_window(&behind(under()), [401, 300]).input(0),
    before,
    "the window's size"
  );

  let mut moved = behind(under());
  moved.backdrops[0].x += 1.0;
  assert_ne!(input(&moved), before, "the box moved");
  let mut wider = behind(under());
  wider.backdrops[0].sigma = 5.0;
  assert_ne!(input(&wider), before, "the radius");
}

#[test]
fn how_the_glass_is_put_back_is_not_part_of_the_input() {
  let before = input(&behind(under()));
  let mut faded = behind(under());
  faded.backdrops[0].opacity = 0.3;
  assert_eq!(input(&faded), before, "its opacity");
  let mut rounder = behind(under());
  rounder.backdrops[0].radii = [20.0; 4];
  assert_eq!(input(&rounder), before, "its corners");
}

#[test]
fn a_blur_inside_a_layer_reads_the_layer_only() {
  // Orders: 0 the window's backdrop, [1 a rect, 2 the glass, 3 a rect] in a
  // faded layer.
  let scene = |window_color: Color, inner: f32| {
    let mut backdrop = rect(0, [0.0, 0.0, 400.0, 300.0]);
    backdrop.color = window_color;
    let mut list = list(
      vec![
        backdrop,
        rect(1, [inner, 110.0, 20.0, 20.0]),
        rect(3, [130.0, 120.0, 10.0, 10.0]),
      ],
      vec![glass(2, GLASS, 4.0)],
    );
    list.layers = vec![layer(1, 4, [60, 60, 200, 160])];
    list
  };
  let (grey, white) = (Color::new(90, 90, 90, 255), Color::new(255, 255, 255, 255));
  let before = input(&scene(grey, 120.0)).expect("a known input");
  assert_eq!(
    input(&scene(white, 120.0)),
    Some(before.clone()),
    "the window under the layer"
  );
  assert_ne!(input(&scene(grey, 121.0)), Some(before), "the layer's own content");
  // The layer it is painted for is part of it.
  let mut resized = scene(grey, 120.0);
  resized.layers[0].bounds.width += 1;
  assert_ne!(input(&resized), input(&scene(grey, 120.0)));
}

#[test]
fn an_input_past_its_limit_is_not_known() {
  let many = |count: usize| {
    let mut rects = vec![rect(0, [0.0, 0.0, 400.0, 300.0])];
    rects.extend((0..count).map(|index| rect(index + 1, [110.0, 110.0, 20.0, 20.0])));
    list(rects, vec![glass(count + 1, GLASS, 4.0)])
  };
  assert!(input(&many(100)).is_some());
  // A rect takes some eighty bytes.
  assert_eq!(input(&many(MAX_INPUT_BYTES / 60)), None);
}

/// What a cache answers for the blurs of `scene`, keeping (as textures 100,
/// 101, ... by blur) every blur it may; with the textures it released.
fn frame(cache: &mut BackdropCache, scene: &Scene, bytes: usize) -> (Vec<Option<BackdropUse>>, Vec<u64>) {
  let (mut uses, mut released) = (Vec::new(), Vec::new());
  cache.begin_frame(
    &scene.regions,
    &scene.inputs,
    BackdropCache::budget_of(WINDOW),
    &mut uses,
    &mut released,
  );
  for (index, used) in uses.iter_mut().enumerate() {
    if *used == Some(BackdropUse::Blur { keep: true }) && !cache.keep(&scene.inputs, index, 100 + index as u64, bytes) {
      *used = Some(BackdropUse::Blur { keep: false });
    }
  }
  (uses, released)
}

#[test]
fn a_kept_result_is_put_back_while_its_input_is_the_kept_one() {
  let mut cache = BackdropCache::default();
  let still = Scene::new(&behind(under()));
  assert_eq!(
    frame(&mut cache, &still, 1000).0,
    vec![Some(BackdropUse::Blur { keep: true })]
  );
  cache.commit();
  assert_eq!((cache.len(), cache.bytes()), (1, 1000));
  for _ in 0..3 {
    assert_eq!(
      frame(&mut cache, &still, 1000),
      (vec![Some(BackdropUse::Reuse { texture: 100 })], vec![])
    );
    cache.commit();
  }

  // Something behind it changed: blurred again, into the texture it has.
  let mut changed = under();
  changed.color = Color::new(0, 0, 0, 255);
  let changed = Scene::new(&behind(changed));
  assert_eq!(
    frame(&mut cache, &changed, 1000),
    (vec![Some(BackdropUse::Refresh { texture: 100 })], vec![])
  );
  cache.commit();
  assert_eq!(
    frame(&mut cache, &changed, 1000).0,
    vec![Some(BackdropUse::Reuse { texture: 100 })]
  );
  cache.commit();
  // And back: the first input is no longer kept.
  assert_eq!(
    frame(&mut cache, &still, 1000).0,
    vec![Some(BackdropUse::Refresh { texture: 100 })]
  );
  assert_eq!(cache.len(), 1);
}

#[test]
fn a_frame_that_never_reached_the_gpu_keeps_nothing() {
  let mut cache = BackdropCache::default();
  let still = Scene::new(&behind(under()));
  // Kept by a frame that was not committed: the texture holds nothing yet.
  assert_eq!(
    frame(&mut cache, &still, 1000).0,
    vec![Some(BackdropUse::Blur { keep: true })]
  );
  assert_eq!(
    frame(&mut cache, &still, 1000).0,
    vec![Some(BackdropUse::Refresh { texture: 100 })]
  );
  cache.commit();
  assert_eq!(
    frame(&mut cache, &still, 1000).0,
    vec![Some(BackdropUse::Reuse { texture: 100 })]
  );
  cache.commit();

  // Blurred again by a frame that was not committed: the texture may hold
  // either frame's pixels.
  let mut changed = under();
  changed.color = Color::new(0, 0, 0, 255);
  let changed = Scene::new(&behind(changed));
  assert_eq!(
    frame(&mut cache, &changed, 1000).0,
    vec![Some(BackdropUse::Refresh { texture: 100 })]
  );
  assert_eq!(
    frame(&mut cache, &still, 1000).0,
    vec![Some(BackdropUse::Refresh { texture: 100 })]
  );
  assert_eq!(
    frame(&mut cache, &changed, 1000).0,
    vec![Some(BackdropUse::Refresh { texture: 100 })]
  );
}

#[test]
fn a_glass_that_goes_or_moves_gives_its_texture_back() {
  let mut cache = BackdropCache::default();
  let still = Scene::new(&behind(under()));
  frame(&mut cache, &still, 1000);
  cache.commit();

  // Moved: another place, so another texture; the old one is released.
  let mut moved = behind(under());
  moved.backdrops[0].x += 10.0;
  let moved = Scene::new(&moved);
  assert_eq!(
    frame(&mut cache, &moved, 1000),
    (vec![Some(BackdropUse::Blur { keep: true })], vec![100])
  );
  cache.commit();
  assert_eq!(cache.len(), 1);

  // Gone: nothing is kept for it.
  let without = Scene::new(&list(vec![rect(0, [0.0, 0.0, 400.0, 300.0])], Vec::new()));
  assert_eq!(frame(&mut cache, &without, 1000), (vec![], vec![100]));
  assert_eq!((cache.len(), cache.bytes()), (0, 0));

  // A window of another size has other pixels to copy.
  frame(&mut cache, &still, 1000);
  cache.commit();
  let resized = Scene::in_window(&behind(under()), [500, 300]);
  assert_eq!(
    frame(&mut cache, &resized, 1000),
    (vec![Some(BackdropUse::Blur { keep: true })], vec![100])
  );

  let mut released = Vec::new();
  cache.clear(&mut released);
  assert_eq!((released, cache.len()), (vec![100], 0));
}

#[test]
fn kept_textures_stay_within_their_budget() {
  let mut cache = BackdropCache::default();
  let budget = BackdropCache::budget_of(WINDOW);
  // Twice the window's pixels, four bytes each; a small window's is 1 MiB.
  assert_eq!(BackdropCache::budget_of([1000, 500]), 1000 * 500 * 4 * 2);
  assert_eq!(budget, MIN_BUDGET);
  // Three blurs apart from each other, each with a texture of 0.4 budgets.
  let three = Scene::new(&list(
    vec![rect(0, [0.0, 0.0, 400.0, 300.0])],
    vec![
      glass(1, [10.0, 10.0, 40.0, 40.0], 4.0),
      glass(2, [150.0, 10.0, 40.0, 40.0], 4.0),
      glass(3, [300.0, 10.0, 40.0, 40.0], 4.0),
    ],
  ));
  let each = budget * 2 / 5;
  assert_eq!(
    frame(&mut cache, &three, each).0,
    vec![
      Some(BackdropUse::Blur { keep: true }),
      Some(BackdropUse::Blur { keep: true }),
      Some(BackdropUse::Blur { keep: false }),
    ]
  );
  cache.commit();
  assert_eq!((cache.len(), cache.bytes()), (2, each * 2));
  // The third is blurred every frame; the two kept ones are put back.
  assert_eq!(
    frame(&mut cache, &three, each).0,
    vec![
      Some(BackdropUse::Reuse { texture: 100 }),
      Some(BackdropUse::Reuse { texture: 101 }),
      Some(BackdropUse::Blur { keep: false }),
    ]
  );

  // No more than `MAX_KEPT` results, however small.
  let mut cache = BackdropCache::default();
  let many = Scene::new(&list(
    vec![rect(0, [0.0, 0.0, 400.0, 300.0])],
    (0..MAX_KEPT + 3)
      .map(|index| glass(index + 1, [index as f32 * 4.0, 10.0, 2.0, 2.0], 1.0))
      .collect(),
  ));
  let kept = frame(&mut cache, &many, 16)
    .0
    .iter()
    .filter(|used| **used == Some(BackdropUse::Blur { keep: true }))
    .count();
  assert_eq!((kept, cache.len()), (MAX_KEPT, MAX_KEPT));
}

#[test]
fn two_blurs_at_one_place_keep_a_result_each() {
  // Two glasses on the same box, a rect between them.
  let scene = |between: Color| {
    let mut middle = rect(2, [110.0, 110.0, 30.0, 30.0]);
    middle.color = between;
    Scene::new(&list(
      vec![rect(0, [0.0, 0.0, 400.0, 300.0]), middle],
      vec![glass(1, GLASS, 4.0), glass(3, GLASS, 4.0)],
    ))
  };
  let mut cache = BackdropCache::default();
  let first = scene(Color::new(255, 255, 255, 128));
  assert_eq!(
    frame(&mut cache, &first, 1000).0,
    vec![
      Some(BackdropUse::Blur { keep: true }),
      Some(BackdropUse::Blur { keep: true }),
    ]
  );
  cache.commit();
  assert_eq!(
    frame(&mut cache, &first, 1000).0,
    vec![
      Some(BackdropUse::Reuse { texture: 100 }),
      Some(BackdropUse::Reuse { texture: 101 }),
    ]
  );
  cache.commit();
  // The rect between them is behind the second only: the first keeps its
  // result, and the second does not take it.
  let second = scene(Color::new(0, 0, 0, 128));
  assert_eq!(
    frame(&mut cache, &second, 1000).0,
    vec![
      Some(BackdropUse::Reuse { texture: 100 }),
      Some(BackdropUse::Refresh { texture: 101 }),
    ]
  );
}

#[cfg(feature = "raster")]
#[test]
fn a_blur_without_a_known_input_keeps_nothing() {
  let native = crate::images::NativeImageData::new(
    4,
    4,
    crate::images::ImagePixelFormat::Rgba8,
    crate::images::NativeImageBackend::Dx12Nv12,
    (),
  );
  let video = |native: Option<crate::images::NativeImageData>| {
    let mut list = behind(under());
    list.images = vec![image(1, 0, native)];
    Scene::new(&list)
  };
  let mut cache = BackdropCache::default();
  for _ in 0..2 {
    assert_eq!(
      frame(&mut cache, &video(Some(native.clone())), 1000),
      (vec![Some(BackdropUse::Blur { keep: false })], vec![])
    );
    cache.commit();
    assert_eq!(cache.len(), 0);
  }
  // A kept result is given back once a video plays behind its glass.
  frame(&mut cache, &video(None), 1000);
  cache.commit();
  assert_eq!(cache.len(), 1);
  assert_eq!(
    frame(&mut cache, &video(Some(native.clone())), 1000),
    (vec![Some(BackdropUse::Blur { keep: false })], vec![100])
  );
}
