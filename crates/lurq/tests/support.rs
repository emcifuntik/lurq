#![allow(dead_code)]

// Cargo also builds this file as a test target of its own, where a plain
// `mod` would look for `tests/selection.rs`.
#[path = "support/selection.rs"]
pub mod selection;

use std::{
  num::NonZeroIsize,
  sync::{Arc, Mutex},
};

use lurq::{
  app::{App, Tree, events::MouseButton, render_engine::RenderEngine},
  layout::render_list::{GlyphCmd, RectCmd, RenderList},
  node::color::Color,
};

pub fn default_app() -> App {
  App::new()
}
use raw_window_handle::{
  DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};

pub struct TestSurface;

impl HasWindowHandle for TestSurface {
  fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
    let handle = Win32WindowHandle::new(NonZeroIsize::new(1).unwrap());
    Ok(unsafe { WindowHandle::borrow_raw(handle.into()) })
  }
}

impl HasDisplayHandle for TestSurface {
  fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
    Ok(unsafe { DisplayHandle::borrow_raw(WindowsDisplayHandle::new().into()) })
  }
}

pub fn run_pass(tree: &mut Tree) {
  let mut app = App::new();
  tree.request_redraw();
  tree.pass(&mut app, &TestSurface);
}

pub fn pointer_click(tree: &mut Tree, x: f32, y: f32, button: MouseButton) {
  tree.mouse_down(x, y, button);
  tree.mouse_up(x, y, button);
}

#[derive(Clone, Debug)]
pub struct RenderSnapshot {
  pub clear_color: Color,
  pub rects: Vec<RectSnapshot>,
  pub glyphs: Vec<GlyphSnapshot>,
  pub glyph_count: usize,
  #[cfg(feature = "image")]
  pub image_orders: Vec<usize>,
  #[cfg(feature = "image")]
  pub image_opacities: Vec<f32>,
  #[cfg(feature = "image")]
  pub images: Vec<ImageSnapshot>,
  #[cfg(feature = "svg")]
  pub svg_orders: Vec<usize>,
  pub layers: Vec<lurq::layout::opacity_layer::LayerCmd>,
  pub backdrops: Vec<lurq::layout::backdrop::BackdropCmd>,
}

/// Where an image is drawn, and which part of its pixels.
#[cfg(feature = "image")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageSnapshot {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub image_width: u32,
  pub image_height: u32,
  pub uv_min: [f32; 2],
  pub uv_max: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphSnapshot {
  pub order: usize,
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub color: [f32; 4],
  pub transform: [f32; 4],
  pub transform_origin: [f32; 2],
  pub shadow_sigma: f32,
  pub clip: ClipSnapshot,
}

#[derive(Clone, Copy, Debug)]
pub struct RectSnapshot {
  pub order: usize,
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub color: Color,
  pub radii: [f32; 4],
  pub stroke: [f32; 4],
  pub stroke_color: Color,
  pub stroke_style: lurq::layout::render_list::StrokeStyle,
  pub transform: [f32; 4],
  pub transform_origin: [f32; 2],
  pub clip: ClipSnapshot,
  pub shadow: Option<lurq::layout::render_list::RectShadow>,
}

#[derive(Clone, Copy, Debug)]
pub struct ClipSnapshot {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub active: bool,
}

pub fn render_pass(tree: &mut Tree) -> RenderSnapshot {
  let mut app = App::new();
  render_pass_with_app(tree, &mut app)
}

/// Like [`render_pass`] but with a caller-provided persistent `App` — a fresh
/// `App` per pass changes the theme version, which force-dirties every layout
/// and silently bypasses the incremental relayout paths under test.
pub fn render_pass_with_app(tree: &mut Tree, app: &mut App) -> RenderSnapshot {
  let capture = Arc::new(Mutex::new(None));
  let render_capture = capture.clone();
  tree.set_render_engine_factory(move || {
    Box::new(CapturingRenderEngine {
      capture: render_capture.clone(),
    })
  });
  tree.request_redraw();
  tree.pass(app, &TestSurface);
  capture.lock().unwrap().clone().unwrap_or_else(empty_snapshot)
}

struct CapturingRenderEngine {
  capture: Arc<Mutex<Option<RenderSnapshot>>>,
}

impl RenderEngine for CapturingRenderEngine {
  fn resize(&mut self, _width: u32, _height: u32) {}

  fn render(&mut self, list: &RenderList, _window: WindowHandle<'_>, _display: DisplayHandle<'_>) -> bool {
    let rects = list.rects.iter().map(rect_snapshot).collect();
    let glyphs = list.glyphs.iter().map(glyph_snapshot).collect();
    *self.capture.lock().unwrap() = Some(RenderSnapshot {
      clear_color: list.clear_color,
      rects,
      glyphs,
      glyph_count: list.glyphs.len(),
      #[cfg(feature = "image")]
      image_orders: list.images.iter().map(|image| image.order).collect(),
      #[cfg(feature = "image")]
      image_opacities: list.images.iter().map(|image| image.opacity).collect(),
      #[cfg(feature = "image")]
      images: list
        .images
        .iter()
        .map(|image| ImageSnapshot {
          x: image.x,
          y: image.y,
          width: image.width,
          height: image.height,
          image_width: image.image_width,
          image_height: image.image_height,
          uv_min: image.uv_min,
          uv_max: image.uv_max,
        })
        .collect(),
      #[cfg(feature = "svg")]
      svg_orders: list.svgs.iter().map(|svg| svg.order).collect(),
      layers: list.layers.clone(),
      backdrops: list.backdrops.clone(),
    });
    true
  }
}

fn empty_snapshot() -> RenderSnapshot {
  RenderSnapshot {
    clear_color: Color::new(0, 0, 0, 0),
    rects: vec![],
    glyphs: vec![],
    glyph_count: 0,
    #[cfg(feature = "image")]
    image_orders: vec![],
    #[cfg(feature = "image")]
    image_opacities: vec![],
    #[cfg(feature = "image")]
    images: vec![],
    #[cfg(feature = "svg")]
    svg_orders: vec![],
    layers: vec![],
    backdrops: vec![],
  }
}

fn glyph_snapshot(glyph: &GlyphCmd) -> GlyphSnapshot {
  GlyphSnapshot {
    order: glyph.order,
    x: glyph.x,
    y: glyph.y,
    width: glyph.width,
    height: glyph.height,
    color: glyph.color,
    transform: glyph.transform,
    transform_origin: glyph.transform_origin,
    shadow_sigma: glyph.shadow_sigma,
    clip: clip_snapshot(glyph.clip),
  }
}

fn rect_snapshot(rect: &RectCmd) -> RectSnapshot {
  RectSnapshot {
    order: rect.order,
    x: rect.x,
    y: rect.y,
    width: rect.width,
    height: rect.height,
    color: rect.color,
    radii: rect.radii,
    stroke: rect.stroke,
    stroke_color: rect.stroke_color,
    stroke_style: rect.stroke_style,
    transform: rect.transform,
    transform_origin: rect.transform_origin,
    clip: clip_snapshot(rect.clip),
    shadow: rect.shadow,
  }
}

fn clip_snapshot(clip: lurq::layout::quad::ClipRect) -> ClipSnapshot {
  ClipSnapshot {
    x: clip.x,
    y: clip.y,
    width: clip.width,
    height: clip.height,
    active: clip.active,
  }
}

/// Like [`render_pass_with_app`] but without requesting a redraw first: the
/// pass only draws when the tree itself asked for one, so a missing
/// invalidation shows up as a stale (or absent) snapshot.
pub fn render_pass_if_needed(tree: &mut Tree, app: &mut App) -> Option<RenderSnapshot> {
  let capture = Arc::new(Mutex::new(None));
  let render_capture = capture.clone();
  tree.set_render_engine_factory(move || {
    Box::new(CapturingRenderEngine {
      capture: render_capture.clone(),
    })
  });
  tree.pass(app, &TestSurface);
  capture.lock().unwrap().clone()
}
