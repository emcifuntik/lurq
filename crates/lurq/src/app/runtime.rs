#[cfg(feature = "screenshot")]
use std::path::PathBuf;
#[cfg(feature = "devtools")]
use std::sync::Mutex;
use std::{
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use raw_window_handle::{DisplayHandle, HasDisplayHandle, HasWindowHandle, WindowHandle};

#[cfg(feature = "devtools")]
use crate::app::devtools::{
  DevTools, DevToolsBoolCallback, DevToolsDebugOverlayCallback, DevToolsPathCallback, DevToolsProps, DevToolsSnapshot,
};
#[cfg(feature = "perf_profile")]
use crate::app::profile_types::{FrameProfile, RuntimeMemoryProfile};
#[cfg(feature = "screenshot")]
use crate::app::render_engine::{RenderFrameCapture, RenderFrameCaptureWindowClip};
#[cfg(feature = "form")]
use crate::node::ButtonKind;
use crate::{
  animation::{AnimationEngine, Keyframes, TransitionEngine},
  app::{
    app_state::App,
    component::Component,
    ctx::{
      CollisionStrategy, Ctx, ModalLayer, ModalSpec, ModalTarget, OutsidePress, OverlaySpec, Placement,
      component_tag_name,
    },
    events::{
      DragEvent, DropEvent, DropResult, EventControl, InputSource, KeyboardEvent, MouseButton, MouseEvent,
      MouseEventKind, ScrollEvent, ScrollPhase, input_source, with_input_source,
    },
    hit_test::{HitRect, hit_test_tree, hit_test_tree_all},
    profile_support::{PerfMeterStats, profile_elapsed, profile_if, profile_scope, profile_value},
    render_engine::{RenderEngine, RenderEngineFactory},
    theme::CaretMode,
  },
  core::{
    ElementRect, ElementRef as OwnedElementRef, ElementRefMut as OwnedElementRefMut, IdGenerator, NodeId, Signal,
  },
  layout::{
    Constraints, Size,
    layout_engine::{ElementLayoutRecord, LayoutEngine, OverlayLayoutIndex, OverlayLayoutRecord},
    layout_kind::{LayoutKind, Position, ScrollAxis, ScrollDirection, ScrollState},
    layout_result::LayoutResult,
    quad::{ClipRect, Quad, QuadContent},
    render_list::{GlyphCmd, RectCmd, RenderGradient, RenderList},
    scrollbar::ScrollBarPart,
    text_style::{FontWeight, TextStyle},
  },
  node::{
    Element, ElementRef, EventHandler, HitTestBehavior, Node, NodeUpdate, SyntheticNodeRole, TextTransformMode,
    VoidEventHandler,
    border::{BorderPlacement, BorderRadius, ResolvedBorder, ResolvedBorders},
    color::Color,
    cursor::CursorIcon,
    dimension::Dimension,
    node_kind::{CheckboxState, NodeKind, SelectState, SliderState, TextInputOverflow, TextInputState, TextState},
    transform::Transform2D,
  },
};

#[cfg(all(test, feature = "screenshot"))]
mod capture_redaction_tests;
#[cfg(test)]
mod caret_blink_tests;
mod file_drag;
mod focus_visible;
mod hover;
mod ime;
#[cfg(all(test, feature = "markdown"))]
mod inline_link_copy_tests;
#[cfg(feature = "markdown")]
mod inline_link_keys;
mod input_token_paint;
#[cfg(feature = "image")]
mod loading_images;
#[cfg(feature = "image")]
pub(crate) use loading_images::note_loading_image_built;
mod outside_press;
mod overlay_layers;
mod paste;
pub use paste::ClipboardReader;
mod scroll_into_view;
mod select_menu;
mod selected_text_lines;
mod tab_navigation;
#[cfg(all(test, feature = "markdown"))]
mod table_copy_tests;

use overlay_layers::{HiddenDismiss, OverlayLayers};
use selected_text_lines::{SelectedTextFragment, join_selected_text};

/// Surface of [`Tree::pass_headless`]: it has no window, so passes stop
/// after layout.
struct HeadlessSurface;

impl HasWindowHandle for HeadlessSurface {
  fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
    Err(raw_window_handle::HandleError::Unavailable)
  }
}

impl HasDisplayHandle for HeadlessSurface {
  fn display_handle(&self) -> Result<DisplayHandle<'_>, raw_window_handle::HandleError> {
    Err(raw_window_handle::HandleError::Unavailable)
  }
}

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_DISTANCE: f32 = 4.0;
/// How long a vertical wheel delta tells the slant of the horizontal deltas
/// that follow it at the same pointer (see [`Tree::wheel_gesture_vertical`]).
const WHEEL_AXIS_LOCK_WINDOW: Duration = Duration::from_millis(150);
/// How far the pointer may move between them, in logical pixels.
const WHEEL_AXIS_LOCK_DISTANCE: f32 = 8.0;

/// The last wheel or swipe event with a vertical delta: when, where (logical
/// pixels) and how much.
#[derive(Clone, Copy)]
struct RecentVerticalWheel {
  at: Instant,
  x: f32,
  y: f32,
  delta: f32,
}
const SUPPRESSED_CLICK_INTERVAL: Duration = Duration::from_millis(250);
const SUPPRESSED_CLICK_DISTANCE: f32 = 4.0;
const TEXT_INPUT_CARET_BLINK_INTERVAL: Duration = Duration::from_millis(530);
const PERF_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const VIDEO_TIMELINE_SAMPLE_INTERVAL_MS: u64 = 1_000;
const SLOW_FRAME_PASS_TIMELINE_THRESHOLD: Duration = Duration::from_millis(16);
const PASS_BREAKDOWN_THRESHOLD: Duration = Duration::from_millis(45);
const TRANSPARENT_COLOR: Color = Color::new(0, 0, 0, 0);
const DEFAULT_CLEAR_COLOR: Color = Color::new(255, 255, 255, 255);

/// The frame clear colour: the app root's background, with palette roles
/// resolved through the theme. The synthetic overlay host that wraps the root
/// while modals or overlays are mounted is skipped. Only a root without a
/// resolvable background falls back to white. Root rect edges are anti-aliased
/// against this colour, so clearing a dark app to white could show a light seam
/// at the window edge.
fn root_clear_color(root: Option<&Node>, app: &App) -> Color {
  let base = match root {
    Some(host) if host.has_synthetic_role(SyntheticNodeRole::OverlayHost) => host.children.first(),
    root => root,
  };
  base
    .and_then(Node::background_color)
    .and_then(|color| color.resolve(&app.theme().palette()))
    .unwrap_or(DEFAULT_CLEAR_COLOR)
}
const DEFAULT_SLIDER_THUMB_MIN_SIZE: f32 = 12.0;
#[cfg(feature = "devtools")]
const DEVTOOLS_SYNC_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(feature = "devtools")]
const DEVTOOLS_INTERACTION_SYNC_DELAY: Duration = Duration::from_millis(250);

#[allow(dead_code)]
static IMAGE_BUILD_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);
#[allow(dead_code)]
static IMAGE_REFRESH_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);
static FRAME_PASS_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);
static LAYOUT_FAST_PATH_MISS_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);
static RENDER_LIST_CACHE_HIT_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);
static RENDER_LIST_CACHE_MISS_TIMELINE_LAST_INFO_MS: AtomicU64 = AtomicU64::new(0);

#[allow(dead_code)]
fn log_draw_image_timeline_sampled(
  last_info_ms: &AtomicU64,
  phase: &'static str,
  image_id: u64,
  image_version: u64,
  frame_index: usize,
) {
  if !crate::app::profile_support::video_diagnostics_enabled() {
    return;
  }
  if should_log_video_timeline_sample(last_info_ms) {
    crate::app::profile_support::video_log!(info,
      target: "video::timeline",
      "[video:timeline] draw_image phase={} image_id={} image_version={} frame_index={}",
      phase,
      image_id,
      image_version,
      frame_index
    );
  } else {
    crate::app::profile_support::video_log!(debug,
      target: "video::timeline",
      "[video:timeline] draw_image phase={} image_id={} image_version={} frame_index={}",
      phase,
      image_id,
      image_version,
      frame_index
    );
  }
}

fn should_log_video_timeline_sample(last_info_ms: &AtomicU64) -> bool {
  let now_ms = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis()
    .min(u128::from(u64::MAX)) as u64;
  let previous_ms = last_info_ms.load(Ordering::Relaxed);
  now_ms.saturating_sub(previous_ms) >= VIDEO_TIMELINE_SAMPLE_INTERVAL_MS
    && last_info_ms
      .compare_exchange(previous_ms, now_ms, Ordering::Relaxed, Ordering::Relaxed)
      .is_ok()
}

fn timeline_ms(duration: Duration) -> f32 {
  duration.as_secs_f32() * 1000.0
}

fn log_render_list_cache_miss_timeline(reason: &'static str, pass_reasons: PassReasons) {
  if !crate::app::profile_support::video_diagnostics_enabled() {
    return;
  }
  if should_log_video_timeline_sample(&RENDER_LIST_CACHE_MISS_TIMELINE_LAST_INFO_MS) {
    crate::app::profile_support::video_log!(info,
      target: "video::timeline",
      "[video:timeline] render_list_cache phase=miss reason={} pass_reasons={:?}",
      reason,
      pass_reasons
    );
  } else {
    crate::app::profile_support::video_log!(debug,
      target: "video::timeline",
      "[video:timeline] render_list_cache phase=miss reason={} pass_reasons={:?}",
      reason,
      pass_reasons
    );
  }
}

#[allow(clippy::too_many_arguments)]
fn log_layout_fast_path_miss_timeline(
  animation_layout_changed: bool,
  image_resources_changed: bool,
  svg_resources_changed: bool,
  theme_changed: bool,
  has_active_overlays: bool,
  has_dirty_element_ref: bool,
  has_pending_layout_dirty: bool,
  has_runtime_layout_state: bool,
  has_last_layout: bool,
  root_cache_contains: bool,
  root_render_dirty: bool,
  root_child_count: usize,
  component_dirty: bool,
) {
  if !crate::app::profile_support::video_diagnostics_enabled() {
    return;
  }
  if should_log_video_timeline_sample(&LAYOUT_FAST_PATH_MISS_TIMELINE_LAST_INFO_MS) {
    crate::app::profile_support::video_log!(info,
      target: "video::timeline",
      "[video:timeline] layout_fast_path phase=miss animation_layout_changed={} image_resources_changed={} svg_resources_changed={} theme_changed={} has_active_overlays={} has_dirty_element_ref={} has_pending_layout_dirty={} has_runtime_layout_state={} has_last_layout={} root_cache_contains={} root_render_dirty={} root_child_count={} component_dirty={}",
      animation_layout_changed,
      image_resources_changed,
      svg_resources_changed,
      theme_changed,
      has_active_overlays,
      has_dirty_element_ref,
      has_pending_layout_dirty,
      has_runtime_layout_state,
      has_last_layout,
      root_cache_contains,
      root_render_dirty,
      root_child_count,
      component_dirty
    );
  } else {
    crate::app::profile_support::video_log!(debug,
      target: "video::timeline",
      "[video:timeline] layout_fast_path phase=miss animation_layout_changed={} image_resources_changed={} svg_resources_changed={} theme_changed={} has_active_overlays={} has_dirty_element_ref={} has_pending_layout_dirty={} has_runtime_layout_state={} has_last_layout={} root_cache_contains={} root_render_dirty={} root_child_count={} component_dirty={}",
      animation_layout_changed,
      image_resources_changed,
      svg_resources_changed,
      theme_changed,
      has_active_overlays,
      has_dirty_element_ref,
      has_pending_layout_dirty,
      has_runtime_layout_state,
      has_last_layout,
      root_cache_contains,
      root_render_dirty,
      root_child_count,
      component_dirty
    );
  }
}

#[allow(clippy::too_many_arguments)]
fn log_frame_pass_timeline(
  path: &'static str,
  total: Duration,
  layout: Duration,
  quad: Duration,
  glyph: Duration,
  gpu: Duration,
  report: PassReport,
  quad_count: usize,
  rect_count: usize,
  glyph_count: usize,
  text_measure_cache: (usize, usize),
  glyph_cache: (usize, usize),
) {
  if !crate::app::profile_support::video_diagnostics_enabled() {
    return;
  }
  // Text measurement happens during layout — a slow layout with a high
  // measure-miss count means re-shaping, not flex arithmetic.
  let caches = format!(
    "text_measure_hit/miss={}/{} glyph_hit/miss={}/{}",
    text_measure_cache.0, text_measure_cache.1, glyph_cache.0, glyph_cache.1
  );
  let slow = total >= SLOW_FRAME_PASS_TIMELINE_THRESHOLD;
  if slow {
    crate::app::profile_support::video_log!(warn,
      target: "video::timeline",
      "[video:timeline] frame_pass path={} total_ms={:.1} layout_ms={:.1} quad_ms={:.1} glyph_ms={:.1} gpu_ms={:.1} rendered={} cache={} layout_updated={} layout_recalculated={} quad_count={} rect_count={} glyph_count={} {} reasons={:?}",
      path,
      timeline_ms(total),
      timeline_ms(layout),
      timeline_ms(quad),
      timeline_ms(glyph),
      timeline_ms(gpu),
      report.rendered,
      report.used_cached_render_list,
      report.layout_updated,
      report.layout_recalculated,
      quad_count,
      rect_count,
      glyph_count,
      caches,
      report.reasons
    );
  } else if should_log_video_timeline_sample(&FRAME_PASS_TIMELINE_LAST_INFO_MS) {
    crate::app::profile_support::video_log!(info,
      target: "video::timeline",
      "[video:timeline] frame_pass path={} total_ms={:.1} layout_ms={:.1} quad_ms={:.1} glyph_ms={:.1} gpu_ms={:.1} rendered={} cache={} layout_updated={} layout_recalculated={} quad_count={} rect_count={} glyph_count={} {} reasons={:?}",
      path,
      timeline_ms(total),
      timeline_ms(layout),
      timeline_ms(quad),
      timeline_ms(glyph),
      timeline_ms(gpu),
      report.rendered,
      report.used_cached_render_list,
      report.layout_updated,
      report.layout_recalculated,
      quad_count,
      rect_count,
      glyph_count,
      caches,
      report.reasons
    );
  } else {
    crate::app::profile_support::video_log!(debug,
      target: "video::timeline",
      "[video:timeline] frame_pass path={} total_ms={:.1} layout_ms={:.1} quad_ms={:.1} glyph_ms={:.1} gpu_ms={:.1} rendered={} cache={} layout_updated={} layout_recalculated={} quad_count={} rect_count={} glyph_count={} {} reasons={:?}",
      path,
      timeline_ms(total),
      timeline_ms(layout),
      timeline_ms(quad),
      timeline_ms(glyph),
      timeline_ms(gpu),
      report.rendered,
      report.used_cached_render_list,
      report.layout_updated,
      report.layout_recalculated,
      quad_count,
      rect_count,
      glyph_count,
      caches,
      report.reasons
    );
  }
}

fn log_render_list_cache_hit_timeline(
  pass_reasons: PassReasons,
  gpu: Duration,
  rect_count: usize,
  glyph_count: usize,
  image_count: usize,
) {
  if !crate::app::profile_support::video_diagnostics_enabled() {
    return;
  }
  if should_log_video_timeline_sample(&RENDER_LIST_CACHE_HIT_TIMELINE_LAST_INFO_MS) {
    crate::app::profile_support::video_log!(info,
      target: "video::timeline",
      "[video:timeline] render_list_cache phase=hit gpu_ms={:.1} rect_count={} glyph_count={} image_count={} pass_reasons={:?}",
      timeline_ms(gpu),
      rect_count,
      glyph_count,
      image_count,
      pass_reasons
    );
  } else {
    crate::app::profile_support::video_log!(debug,
      target: "video::timeline",
      "[video:timeline] render_list_cache phase=hit gpu_ms={:.1} rect_count={} glyph_count={} image_count={} pass_reasons={:?}",
      timeline_ms(gpu),
      rect_count,
      glyph_count,
      image_count,
      pass_reasons
    );
  }
}

#[cfg(feature = "clipboard")]
fn write_clipboard_text(text: &str) -> bool {
  crate::clipboard::copy_to_clipboard(text)
}

#[cfg(not(feature = "clipboard"))]
fn write_clipboard_text(_text: &str) -> bool {
  false
}

#[cfg(feature = "devtools")]
fn set_component_debug_metadata(node: &mut Node, ctx: &Ctx) {
  node.set_component_props_debug(ctx.props_debug());
  node.set_component_signals_debug(ctx.signals_debug());
  node.set_component_effects_debug(ctx.effects_debug());
  node.set_component_contexts_debug(ctx.contexts_debug());
}

trait AnyRootComponent: Send + Sync {
  fn render(&self, ctx: &mut Ctx) -> Element;
  fn after_layout(&self);
  fn on_mounted(&self);
  fn on_unmounted(&self);
  fn tag_name(&self) -> Arc<str>;
}

struct RootComponentWrapper<C: Component> {
  component: C,
}

impl<C: Component> AnyRootComponent for RootComponentWrapper<C> {
  fn render(&self, ctx: &mut Ctx) -> Element {
    self.component.render(ctx).into()
  }

  fn after_layout(&self) {
    self.component.after_layout();
  }

  fn on_mounted(&self) {
    self.component.on_mounted();
  }

  fn on_unmounted(&self) {
    self.component.on_unmounted();
  }

  fn tag_name(&self) -> Arc<str> {
    component_tag_name::<C>()
  }
}

/// A frame capture waiting for the next rendered frame.
#[cfg(feature = "screenshot")]
struct PendingScreenshot {
  target: crate::app::render_engine::RenderCaptureTarget,
  region: Option<crate::app::window::ScreenshotRegion>,
  /// Made for an inspector: sensitive text is painted over.
  redact_sensitive: bool,
}

struct CachedRenderList {
  list: RenderList,
  #[cfg(feature = "raster")]
  image_sources: Vec<Option<crate::images::ImageData>>,
  #[cfg(feature = "raster")]
  video_sources: Vec<Option<crate::images::ImageData>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassReasons {
  pub redraw_requested: bool,
  pub scheduled_redraw: bool,
  pub timer_run: bool,
  pub timer_active: bool,
  pub future_completed: bool,
  pub future_active: bool,
  pub timeline_active: bool,
  pub continuous_redraw_image: bool,
  pub perf_overlay: bool,
  pub pending_click: bool,
  pub input_interaction: bool,
  pub text_input_caret: bool,
  pub theme_changed: bool,
  pub component_dirty: bool,
  pub element_ref_dirty: bool,
  pub layout_dirty: bool,
}

impl PassReasons {
  pub fn any(self) -> bool {
    self.redraw_requested
      || self.scheduled_redraw
      || self.timer_run
      || self.timer_active
      || self.future_completed
      || self.future_active
      || self.timeline_active
      || self.continuous_redraw_image
      || self.perf_overlay
      || self.pending_click
      || self.input_interaction
      || self.text_input_caret
      || self.theme_changed
      || self.component_dirty
      || self.element_ref_dirty
      || self.layout_dirty
  }

  fn merge(&mut self, other: Self) {
    self.redraw_requested |= other.redraw_requested;
    self.scheduled_redraw |= other.scheduled_redraw;
    self.timer_run |= other.timer_run;
    self.timer_active |= other.timer_active;
    self.future_completed |= other.future_completed;
    self.future_active |= other.future_active;
    self.timeline_active |= other.timeline_active;
    self.continuous_redraw_image |= other.continuous_redraw_image;
    self.perf_overlay |= other.perf_overlay;
    self.pending_click |= other.pending_click;
    self.input_interaction |= other.input_interaction;
    self.text_input_caret |= other.text_input_caret;
    self.theme_changed |= other.theme_changed;
    self.component_dirty |= other.component_dirty;
    self.element_ref_dirty |= other.element_ref_dirty;
    self.layout_dirty |= other.layout_dirty;
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassReport {
  pub required: bool,
  pub rendered: bool,
  pub used_cached_render_list: bool,
  pub layout_updated: bool,
  pub layout_recalculated: bool,
  pub reasons: PassReasons,
}

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

fn log_pass_breakdown(
  path: &'static str,
  started_at: Instant,
  setup: Duration,
  initial_cache: Duration,
  layout: Duration,
  second_cache: Duration,
  quad: Duration,
  glyph: Duration,
  gpu: Duration,
  report: PassReport,
) {
  let total = started_at.elapsed();
  if total < PASS_BREAKDOWN_THRESHOLD {
    return;
  }

  crate::app::profile_support::video_log!(debug,
    target: "video::watch::lurq",
    "lurq pass breakdown path={} total_ms={:.2} setup_ms={:.2} initial_cache_ms={:.2} layout_ms={:.2} second_cache_ms={:.2} quad_ms={:.2} glyph_ms={:.2} gpu_ms={:.2} rendered={} cached={} required={} reasons={:?}",
    path,
    duration_ms(total),
    duration_ms(setup),
    duration_ms(initial_cache),
    duration_ms(layout),
    duration_ms(second_cache),
    duration_ms(quad),
    duration_ms(glyph),
    duration_ms(gpu),
    report.rendered,
    report.used_cached_render_list,
    report.required,
    report.reasons
  );
}

// Retain callbacks and external state so removal can blur the old control even
// after reconciliation has dropped its Node. Never recover focus by an old path.
#[derive(Default)]
struct FocusMemory {
  blur: Vec<VoidEventHandler>,
  refs: Vec<OwnedElementRef>,
  interactions: Vec<crate::node::interaction_state::InteractionState>,
  input: Option<TextInputState>,
}
impl FocusMemory {
  fn capture(root: &Node, input_id: NodeId, event_id: NodeId) -> Self {
    let mut memory = Self::default();
    for id in [input_id, event_id] {
      if let Some(node) = find_node_by_id(root, id) {
        if let Some(reference) = &node.element_ref {
          memory.refs.push(reference.clone());
        }
        if let Some(state) = &node.interaction {
          memory.interactions.push(state.clone());
        }
        if let NodeKind::TextInput { state, .. } = node.node_kind() {
          memory.input = Some(state.clone());
        }
      }
    }
    memory.blur = find_node_by_id(root, event_id)
      .map(|node| node.events.on_blur.clone())
      .unwrap_or_default();
    memory
  }
  fn clear(self) {
    for reference in self.refs {
      reference.set_focused(false);
    }
    for state in self.interactions {
      state.set_focused(false);
    }
    if let Some(input) = self.input {
      input.set_focused(false);
    }
    for handler in self.blur {
      handler.call();
    }
  }
}

pub struct Tree {
  profiling: crate::app::profiler::producer::WindowProfiler,
  id_gen: IdGenerator,
  layout_engine: LayoutEngine,
  render_engine: Option<Box<dyn RenderEngine>>,
  render_engine_factory: Option<RenderEngineFactory>,
  root: Option<Node>,
  root_component: Option<Box<dyn AnyRootComponent>>,
  root_ctx: Option<Ctx>,
  last_layout: Option<LayoutResult>,
  /// The nodes the last layout dropped from their Row/Column
  /// (`ShrinkLimit::Drop`). Kept while the layout is invalidated, so focus
  /// requested before the next layout cannot land inside them either.
  dropped_nodes: Vec<NodeId>,
  /// Nodes to scroll into view after the next layout (focus requests and
  /// `ElementHandle::scroll_into_view`); see [`scroll_into_view`].
  pending_scroll_into_view: Vec<NodeId>,
  layout_constraints_override: Option<Constraints>,
  viewport_physical: Size,
  scale_factor: f32,
  window: crate::app::window::Window,
  /// Where the pointer last was, and the source of the input that put it
  /// there; `None` once it left the window. The hover is hit-tested again
  /// there after a layout moves content.
  pointer: Option<hover::PointerAt>,
  hover_path: Vec<NodeId>,
  active_path: Vec<NodeId>,
  dragging_scroll: Option<ScrollDrag>,
  dragging_slider: Option<SliderDrag>,
  dragging_text_selection: Option<TextSelectionDrag>,
  /// Where the selection in the selectable texts started, which a Shift
  /// press extends from.
  selectable_text_anchor: Option<SelectableTextAnchor>,
  active_drag: Option<ActiveDrag>,
  /// Files from the OS dragged over the window; see `file_drag`.
  file_drag: Option<file_drag::FileDragSession>,
  /// Runtime images still decoding after the last pass; see `loading_images`.
  #[cfg(feature = "image")]
  pending_images: Vec<(crate::images::ImageKey, Option<crate::images::ImageSizeCap>)>,
  /// The waker the decoded-image cache calls for this tree's window, made once.
  #[cfg(feature = "image")]
  loading_image_waker: Option<crate::app::window::WindowWaker>,
  /// The loading-image nodes built when the tree last walked for them.
  #[cfg(feature = "image")]
  loading_images_built_seen: u64,
  /// What paste shortcuts read instead of the OS clipboard; see `paste`.
  clipboard_reader: Option<Arc<dyn paste::ClipboardReader>>,
  focus_memory: FocusMemory,
  focused_node: Option<NodeId>,
  focused_event_node: Option<NodeId>,
  focused_path: Option<Vec<usize>>,
  focused_event_path: Option<Vec<usize>>,
  /// The modality of the last focus-moving input; see [`focus_visible`].
  input_modality: focus_visible::InputModality,
  text_input_caret_blink_started_at: Instant,
  text_input_caret_visible: bool,
  cursor: CursorIcon,
  click_tracker: ClickTracker,
  text_click_tracker: TextClickTracker,
  click_press: Option<ClickPress>,
  /// A left press that only closed popups (`OutsidePress::Consume`); its
  /// release is swallowed too.
  swallowed_press: Option<MouseButton>,
  suppressed_click: Option<SuppressedClick>,
  needs_redraw: bool,
  tree_rebuilt_since_layout: bool,
  pending_pass_reasons: PassReasons,
  scheduled_redraw_at: Option<Instant>,
  scheduled_redraw_due: bool,
  recent_vertical_wheel: Option<RecentVerticalWheel>,
  perf_overlay_enabled: bool,
  perf_overlay_stats: PerfMeterStats,
  perf_overlay_last_sample: Instant,
  perf_overlay_last_seen_frame: u64,
  perf_overlay_frames_since_sample: u64,
  secondary_windows: Vec<SecondaryWindow>,
  #[cfg(feature = "screenshot")]
  pending_screenshot: Option<PendingScreenshot>,
  /// Where the sensitive text of the last full render list was painted, for
  /// captures made for inspection (`app::capture_redaction`).
  #[cfg(feature = "screenshot")]
  capture_redactions: Vec<crate::app::capture_redaction::RedactedArea>,
  #[cfg(feature = "screenshot")]
  sensitive_quads: Vec<usize>,
  #[cfg(feature = "devtools")]
  pub(crate) devtools: Option<DevToolsWindow>,
  #[cfg(feature = "devtools")]
  devtools_state: DevToolsState,
  #[cfg(feature = "devtools")]
  debug_overlay_node_path: Option<Vec<usize>>,
  frame_count: u64,
  #[cfg(feature = "perf_profile")]
  last_profile: FrameProfile,
  #[cfg(feature = "perf_profile")]
  last_memory_profile: RuntimeMemoryProfile,
  #[cfg(feature = "perf_profile")]
  last_memory_profile_sample: Option<Instant>,
  transition_engine: TransitionEngine,
  animation_engine: AnimationEngine,
  /// The time animations and transitions tick at instead of each pass's: see [`Tree::set_animation_clock`].
  animation_clock: Option<Instant>,
  last_theme_version: u64,
  quad_scratch: Vec<Quad>,
  render_rects: Vec<RectCmd>,
  render_glyphs: Vec<GlyphCmd>,
  #[cfg(feature = "raster")]
  render_images: Vec<crate::images::ImageCmd>,
  #[cfg(feature = "svg")]
  render_svgs: Vec<crate::svg::SvgCmd>,
  render_layers: Vec<crate::layout::opacity_layer::LayerCmd>,
  opacity_groups: Vec<crate::layout::opacity_layer::OpacityGroup>,
  cached_render_list: Option<CachedRenderList>,
  #[cfg(feature = "canvas")]
  canvas_registry: Vec<crate::canvas::CanvasHandle>,
  overlay_dismiss_entries: Vec<OverlayDismissEntry>,
  /// The anchor of each overlay host layer after the page, in host order;
  /// `None` for a select menu or a modal over the page.
  overlay_layer_anchors: Vec<Option<OwnedElementRef>>,
  /// The anchors of the dropdowns in the tree, open or closed, while the
  /// overlay host is up: a press on one switches dropdowns.
  dropdown_anchors: Vec<OwnedElementRef>,
  /// Key releases that belong to an input method composition.
  ime_keys: ime::ImeKeys,
  /// Monotonic id source for secondary windows; ids are never reused so a
  /// closed window can be reported as gone instead of resolving to whatever
  /// window later occupies its Vec slot.
  next_secondary_window_id: u64,
  /// MCP server state; present only on the root tree after `enable_mcp`.
  #[cfg(feature = "mcp")]
  pub(crate) mcp: Option<Box<crate::mcp::McpState>>,
  /// Parked `lurq_wait` replies, resolved as this tree's frames complete.
  #[cfg(feature = "mcp")]
  pub(crate) mcp_wait_entries: Vec<crate::mcp::McpWaitEntry>,
  /// Set when an MCP call delivered held input (a `press`, a `move` while
  /// something is held, ...); the shell presents right after the drain, as it
  /// does from inside a real mouse-move stream.
  #[cfg(feature = "mcp")]
  pub(crate) mcp_input_present: bool,
}

struct OverlayDismissEntry {
  anchor: OwnedElementRef,
  bounds: ElementRect,
  open: Signal<bool>,
  dismiss_on_outside_click: bool,
  outside_press: OutsidePress,
  dismiss_on_escape: bool,
  /// The overlay's index among the overlay host's children (the page is 0).
  /// A press on a layer above it, such as a select menu or a popup opened
  /// from inside it, is not outside it.
  layer: usize,
  /// Whether the overlay is drawn. One hidden with its anchor out of view
  /// has nothing a press could land on: every press is outside it.
  shown: bool,
}

#[cfg_attr(not(feature = "winit"), allow(dead_code))]
pub(crate) struct SecondaryWindow {
  /// Stable id minted by the root tree on push; never reused, unlike the
  /// window's Vec index.
  id: u64,
  /// Optional app-assigned machine name (titles aren't unique) so external
  /// tooling can target `window: "settings"`.
  name: Option<String>,
  title: String,
  width: u32,
  height: u32,
  /// Whether the OS window is created with native decorations.
  decorations: bool,
  tree: Tree,
  open: bool,
  metadata: SecondaryWindowMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SecondaryWindowMetadata {
  pub(crate) window_id: Option<String>,
  pub(crate) raw_window_handle: Option<String>,
  pub(crate) raw_display_handle: Option<String>,
  pub(crate) hwnd: Option<isize>,
}

#[cfg(feature = "devtools")]
#[cfg_attr(not(feature = "winit"), allow(dead_code))]
pub(crate) struct DevToolsWindow {
  pub(crate) secondary_index: usize,
  pub(crate) metadata: SecondaryWindowMetadata,
}

#[cfg_attr(not(feature = "winit"), allow(dead_code))]
impl SecondaryWindow {
  #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
  fn new(title: impl Into<String>, width: u32, height: u32, tree: Tree) -> Self {
    let title = title.into();
    tree.window().record_initial_title(&title);
    Self {
      id: 0,
      name: None,
      title,
      width,
      height,
      decorations: true,
      tree,
      open: true,
      metadata: SecondaryWindowMetadata::default(),
    }
  }

  fn with_name(mut self, name: Option<String>) -> Self {
    self.name = name;
    self
  }

  #[cfg_attr(not(feature = "mcp"), allow(dead_code))]
  pub(crate) fn id(&self) -> u64 {
    self.id
  }

  #[cfg_attr(not(feature = "mcp"), allow(dead_code))]
  pub(crate) fn name(&self) -> Option<&str> {
    self.name.as_deref()
  }

  fn with_decorations(mut self, decorations: bool) -> Self {
    self.decorations = decorations;
    self
  }

  #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
  fn new_closed(title: impl Into<String>, width: u32, height: u32, tree: Tree) -> Self {
    let mut window = Self::new(title, width, height, tree);
    window.open = false;
    window
  }

  #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
  fn open(&mut self) -> bool {
    if self.open {
      return false;
    }
    self.open = true;
    self.tree.profiling.set_open(true);
    true
  }

  fn close(&mut self) -> bool {
    if !self.open {
      return false;
    }
    self.open = false;
    self.tree.profiling.set_open(false);
    self.set_metadata(SecondaryWindowMetadata::default());
    true
  }

  pub(crate) fn title(&self) -> &str {
    &self.title
  }

  pub(crate) fn width(&self) -> u32 {
    self.width
  }

  pub(crate) fn height(&self) -> u32 {
    self.height
  }

  pub(crate) fn decorations(&self) -> bool {
    self.decorations
  }

  pub(crate) fn tree(&self) -> &Tree {
    &self.tree
  }

  pub(crate) fn tree_mut(&mut self) -> &mut Tree {
    &mut self.tree
  }

  pub(crate) fn set_metadata(&mut self, metadata: SecondaryWindowMetadata) {
    self.metadata = metadata;
  }
}

#[cfg(feature = "devtools")]
struct DevToolsState {
  debug_overlay_path: Arc<Mutex<Option<Vec<usize>>>>,
  overlay_enabled: Arc<Mutex<bool>>,
  pick_mode: Arc<Mutex<bool>>,
  selected_path: Arc<Mutex<Vec<usize>>>,
  selected_path_dirty: Arc<Mutex<bool>>,
  screenshot_request: Arc<Mutex<Option<DevToolsScreenshotRequest>>>,
  picked_path: Option<Vec<usize>>,
  picked_revision: u64,
  snapshot_revision: u64,
  last_sync: Instant,
  last_input_interaction: Instant,
}

#[cfg(feature = "devtools")]
struct DevToolsScreenshotRequest {
  node_path: Vec<usize>,
  output_path: PathBuf,
  attempts: u8,
}

#[cfg(feature = "devtools")]
impl Default for DevToolsState {
  fn default() -> Self {
    Self {
      debug_overlay_path: Arc::new(Mutex::new(None)),
      overlay_enabled: Arc::new(Mutex::new(true)),
      pick_mode: Arc::new(Mutex::new(false)),
      selected_path: Arc::new(Mutex::new(Vec::new())),
      selected_path_dirty: Arc::new(Mutex::new(false)),
      screenshot_request: Arc::new(Mutex::new(None)),
      picked_path: None,
      picked_revision: 0,
      snapshot_revision: 0,
      last_sync: Instant::now() - DEVTOOLS_SYNC_INTERVAL,
      last_input_interaction: Instant::now() - DEVTOOLS_INTERACTION_SYNC_DELAY,
    }
  }
}

#[cfg(feature = "devtools")]
fn devtools_debug_overlay_callback(debug_overlay_path: Arc<Mutex<Option<Vec<usize>>>>) -> DevToolsDebugOverlayCallback {
  Arc::new(move |path| {
    *debug_overlay_path.lock().unwrap() = path;
  })
}

#[cfg(feature = "devtools")]
fn devtools_bool_callback(value: Arc<Mutex<bool>>) -> DevToolsBoolCallback {
  Arc::new(move |enabled| {
    *value.lock().unwrap() = enabled;
  })
}

#[cfg(feature = "devtools")]
fn devtools_selected_path_callback(
  selected_path: Arc<Mutex<Vec<usize>>>,
  selected_path_dirty: Arc<Mutex<bool>>,
) -> DevToolsPathCallback {
  Arc::new(move |path| {
    *selected_path.lock().unwrap() = path;
    *selected_path_dirty.lock().unwrap() = true;
  })
}

#[cfg(feature = "devtools")]
fn devtools_save_node_screenshot_callback(
  screenshot_request: Arc<Mutex<Option<DevToolsScreenshotRequest>>>,
) -> DevToolsPathCallback {
  Arc::new(move |node_path| {
    let screenshot_request = screenshot_request.clone();
    std::thread::spawn(move || {
      let Some(mut output_path) = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name("lurq-node.png")
        .save_file()
      else {
        return;
      };

      match output_path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("png") => {}
        _ => {
          output_path.set_extension("png");
        }
      }

      *screenshot_request.lock().unwrap() = Some(DevToolsScreenshotRequest {
        node_path,
        output_path,
        attempts: 0,
      });
    });
  })
}

impl Default for Tree {
  fn default() -> Self {
    Self::new()
  }
}

impl Tree {
  pub fn new() -> Self {
    let tree = Self {
      profiling: crate::app::profiler::producer::WindowProfiler::new(),
      id_gen: IdGenerator::new(),
      layout_engine: LayoutEngine::new(),
      render_engine: None,
      render_engine_factory: None,
      root: None,
      root_component: None,
      root_ctx: None,
      last_layout: None,
      dropped_nodes: Vec::new(),
      pending_scroll_into_view: Vec::new(),
      layout_constraints_override: None,
      viewport_physical: Size::new(800.0, 600.0),
      scale_factor: 1.0,
      window: crate::app::window::Window::new(),
      pointer: None,
      hover_path: Vec::new(),
      active_path: Vec::new(),
      dragging_scroll: None,
      dragging_slider: None,
      dragging_text_selection: None,
      selectable_text_anchor: None,
      active_drag: None,
      file_drag: None,
      #[cfg(feature = "image")]
      pending_images: Vec::new(),
      #[cfg(feature = "image")]
      loading_image_waker: None,
      #[cfg(feature = "image")]
      loading_images_built_seen: 0,
      clipboard_reader: None,
      focus_memory: FocusMemory::default(),
      focused_node: None,
      focused_event_node: None,
      focused_path: None,
      focused_event_path: None,
      input_modality: focus_visible::InputModality::default(),
      text_input_caret_blink_started_at: Instant::now(),
      text_input_caret_visible: true,
      cursor: CursorIcon::Default,
      click_tracker: ClickTracker::default(),
      text_click_tracker: TextClickTracker::default(),
      click_press: None,
      swallowed_press: None,
      suppressed_click: None,
      needs_redraw: true,
      tree_rebuilt_since_layout: false,
      pending_pass_reasons: PassReasons::default(),
      scheduled_redraw_at: None,
      scheduled_redraw_due: false,
      recent_vertical_wheel: None,
      perf_overlay_enabled: false,
      perf_overlay_stats: PerfMeterStats::default(),
      perf_overlay_last_sample: Instant::now(),
      perf_overlay_last_seen_frame: 0,
      perf_overlay_frames_since_sample: 0,
      secondary_windows: Vec::new(),
      #[cfg(feature = "screenshot")]
      pending_screenshot: None,
      #[cfg(feature = "screenshot")]
      capture_redactions: Vec::new(),
      #[cfg(feature = "screenshot")]
      sensitive_quads: Vec::new(),
      #[cfg(feature = "devtools")]
      devtools: None,
      #[cfg(feature = "devtools")]
      devtools_state: DevToolsState::default(),
      #[cfg(feature = "devtools")]
      debug_overlay_node_path: None,
      frame_count: 0,
      #[cfg(feature = "perf_profile")]
      last_profile: FrameProfile::default(),
      #[cfg(feature = "perf_profile")]
      last_memory_profile: RuntimeMemoryProfile::default(),
      #[cfg(feature = "perf_profile")]
      last_memory_profile_sample: None,
      transition_engine: TransitionEngine::new(),
      animation_engine: AnimationEngine::new(),
      animation_clock: None,
      last_theme_version: u64::MAX,
      quad_scratch: Vec::new(),
      render_rects: Vec::new(),
      render_glyphs: Vec::new(),
      #[cfg(feature = "raster")]
      render_images: Vec::new(),
      #[cfg(feature = "svg")]
      render_svgs: Vec::new(),
      render_layers: Vec::new(),
      opacity_groups: Vec::new(),
      cached_render_list: None,
      #[cfg(feature = "canvas")]
      canvas_registry: Vec::new(),
      overlay_dismiss_entries: Vec::new(),
      overlay_layer_anchors: Vec::new(),
      dropdown_anchors: Vec::new(),
      ime_keys: ime::ImeKeys::default(),
      next_secondary_window_id: 1,
      #[cfg(feature = "mcp")]
      mcp: None,
      #[cfg(feature = "mcp")]
      mcp_wait_entries: Vec::new(),
      #[cfg(feature = "mcp")]
      mcp_input_present: false,
    };
    tree
      .window
      .set_resolved_size(tree.viewport_physical.width, tree.viewport_physical.height);
    tree.window.set_scale_factor(tree.scale_factor);
    tree
  }

  pub fn scale_factor(&self) -> f32 {
    self.scale_factor
  }

  pub fn set_scale_factor(&mut self, scale: f32) {
    if self.scale_factor == scale {
      self.window.set_scale_factor(scale);
      return;
    }

    self.scale_factor = scale;
    self.window.set_scale_factor(scale);
    self.invalidate_viewport_layout();
    // Selection carets and rich-text link and underline geometry are laid
    // out at the paint's scale, inside caches below the root.
    self.layout_engine.set_paint_scale(scale);
    if let Some(root) = self.root.as_ref() {
      invalidate_layout_cache_recursive(root);
    }
  }

  fn invalidate_viewport_layout(&mut self) {
    self.last_layout = None;
    self.cached_render_list = None;
    if let Some(root) = self.root.as_ref() {
      root.layout_cache.invalidate();
    }
    self.needs_redraw = true;
    self.pending_pass_reasons.layout_dirty = true;
  }

  /// The reactive window handle for this tree's window. The shell pushes
  /// position updates here; size and scale are kept in sync by `resize` and
  /// `set_scale_factor`.
  pub fn window(&self) -> &crate::app::window::Window {
    &self.window
  }

  pub fn set_window_position(&mut self, x: i32, y: i32) {
    self.window.set_position(x, y);
  }

  #[cfg(feature = "perf_profile")]
  pub(crate) fn memory_profile_with_glyph(&self, glyph_engine_bytes: usize) -> RuntimeMemoryProfile {
    let runtime_struct_bytes = std::mem::size_of::<Self>();
    let root_tree_bytes = self.root.as_ref().map(Node::estimated_memory_bytes).unwrap_or(0);
    let root_context_bytes = self.root_ctx.as_ref().map(Ctx::estimated_memory_bytes).unwrap_or(0);
    let root_component_bytes = self
      .root_component
      .as_ref()
      .map(|_| std::mem::size_of::<Box<dyn AnyRootComponent>>())
      .unwrap_or(0);
    let last_layout_bytes = self
      .last_layout
      .as_ref()
      .map(LayoutResult::estimated_memory_bytes)
      .unwrap_or(0);
    let render_engine_bytes = self
      .render_engine
      .as_ref()
      .map(|_| std::mem::size_of::<Box<dyn RenderEngine>>())
      .unwrap_or(0);
    let hover_path_bytes = self.hover_path.capacity() * std::mem::size_of::<NodeId>();
    let active_path_bytes = self.active_path.capacity() * std::mem::size_of::<NodeId>();
    let dragging_scroll_bytes = self
      .dragging_scroll
      .as_ref()
      .map(|_| std::mem::size_of::<ScrollDrag>())
      .unwrap_or(0);
    let total_bytes = runtime_struct_bytes
      + root_tree_bytes
      + root_context_bytes
      + root_component_bytes
      + last_layout_bytes
      + glyph_engine_bytes
      + render_engine_bytes
      + hover_path_bytes
      + active_path_bytes
      + dragging_scroll_bytes;

    RuntimeMemoryProfile {
      total_bytes,
      runtime_struct_bytes,
      root_tree_bytes,
      root_context_bytes,
      root_component_bytes,
      last_layout_bytes,
      glyph_engine_bytes,
      render_engine_bytes,
      hover_path_bytes,
      active_path_bytes,
      dragging_scroll_bytes,
    }
  }

  #[cfg(feature = "perf_profile")]
  fn cached_memory_profile(&mut self, app: &App) -> RuntimeMemoryProfile {
    let now = Instant::now();
    let should_sample = match self.last_memory_profile_sample {
      Some(last_sample) => now.duration_since(last_sample) >= PERF_SAMPLE_INTERVAL,
      None => true,
    };

    if should_sample {
      self.last_memory_profile =
        self.memory_profile_with_glyph(app.shared.glyph_engine.lock().estimated_memory_bytes());
      self.last_memory_profile_sample = Some(now);
    }

    self.last_memory_profile
  }

  #[cfg(feature = "perf_profile")]
  pub fn last_profile(&self) -> &FrameProfile {
    &self.last_profile
  }

  /// Shared bounded profiling API; session operations never mutate the UI.
  pub fn profiling_handle(&self) -> crate::app::profiler::ProfilingHandle {
    self.profiling.handle()
  }

  #[cfg(feature = "perf_profile")]
  pub fn profile(&self) -> &FrameProfile {
    &self.last_profile
  }

  pub fn draw_perf_overlay(&mut self) {
    if self.perf_overlay_enabled {
      return;
    }
    self.perf_overlay_enabled = true;
    self.perf_overlay_last_sample = Instant::now();
    self.perf_overlay_last_seen_frame = self.frame_count;
    self.perf_overlay_frames_since_sample = 0;
    self.needs_redraw = true;
    self.pending_pass_reasons.perf_overlay = true;
  }

  pub fn request_redraw(&mut self) {
    self.needs_redraw = true;
    self.pending_pass_reasons.redraw_requested = true;
  }

  #[cfg(feature = "screenshot")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn request_screenshot(
    &mut self,
    output_path: impl Into<PathBuf>,
    region: Option<crate::app::window::ScreenshotRegion>,
  ) {
    self.request_screenshot_capture(PendingScreenshot {
      target: crate::app::render_engine::RenderCaptureTarget::Path(output_path.into()),
      region,
      redact_sensitive: false,
    });
  }

  /// Queue a capture of the next rendered frame for an inspector, delivered
  /// to `target`, with sensitive text painted over
  /// (`app::capture_redaction`).
  #[cfg(feature = "mcp")]
  pub(crate) fn request_inspection_capture(
    &mut self,
    target: crate::app::render_engine::RenderCaptureTarget,
    region: Option<crate::app::window::ScreenshotRegion>,
  ) {
    self.request_screenshot_capture(PendingScreenshot {
      target,
      region,
      redact_sensitive: true,
    });
  }

  /// Queue a capture of the next rendered frame. Only one capture can be
  /// pending per tree; a superseded byte-target capture completes with an
  /// error rather than silently vanishing.
  #[cfg(feature = "screenshot")]
  fn request_screenshot_capture(&mut self, request: PendingScreenshot) {
    if let Some(previous) = self.pending_screenshot.replace(request) {
      previous
        .target
        .fail("superseded by a newer capture request before a frame was rendered");
    }
    self.request_redraw();
  }

  pub(crate) fn request_redraw_at(&mut self, at: Instant) {
    self.scheduled_redraw_at = Some(self.scheduled_redraw_at.map_or(at, |current| current.min(at)));
  }

  /// Forgets the redraws scheduled with [`Tree::request_redraw_at`]; the next pass schedules what it still needs.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn drop_scheduled_redraws(&mut self) {
    self.scheduled_redraw_at = None;
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn next_scheduled_redraw(&self) -> Option<Instant> {
    #[cfg(feature = "query")]
    {
      self
        .scheduled_redraw_at
        .into_iter()
        .chain(self.root_ctx.as_ref().and_then(Ctx::next_query_deadline))
        .min()
    }
    #[cfg(not(feature = "query"))]
    {
      self.scheduled_redraw_at
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn tick_scheduled_redraw(&mut self, now: Instant) {
    if self.scheduled_redraw_at.is_some_and(|at| now >= at) {
      self.scheduled_redraw_at = None;
      self.scheduled_redraw_due = true;
      self.needs_redraw = true;
      self.pending_pass_reasons.scheduled_redraw = true;
    }
  }

  pub fn tick_timers(&mut self) {
    self.tick_timers_at(Instant::now());
  }

  pub fn tick_timers_at(&mut self, now: Instant) {
    #[cfg(feature = "image")]
    self.notice_decoded_images();
    let fired = self.root_ctx.as_mut().is_some_and(|ctx| ctx.tick_timers(now));
    if fired {
      self.needs_redraw = true;
      self.pending_pass_reasons.timer_run = true;
      self.apply_reactive_updates_after_event();
    }
  }

  pub fn tick_futures(&mut self) {
    let dirty_before = self.root_ctx.as_ref().is_some_and(Ctx::any_dirty);
    let active_before = self.root_ctx.as_ref().is_some_and(Ctx::has_active_futures);
    let completed = self.root_ctx.as_mut().is_some_and(Ctx::tick_futures);
    let dirty_after_poll = self.root_ctx.as_ref().is_some_and(Ctx::any_dirty);
    if completed || (!dirty_before && dirty_after_poll) {
      tracing::info!(
        target: "lurq::reactivity",
        "[lurq:reactivity] future_poll completed={} active_before={} dirty_before={} dirty_after_poll={}",
        completed,
        active_before,
        dirty_before,
        dirty_after_poll
      );
    }
    if completed {
      self.needs_redraw = true;
      self.pending_pass_reasons.future_completed = true;
      self.apply_reactive_updates_after_event();
      tracing::info!(
        target: "lurq::reactivity",
        "[lurq:reactivity] future_completed applied dirty_after_apply={} needs_redraw={}",
        self.root_ctx.as_ref().is_some_and(Ctx::any_dirty),
        self.needs_redraw
      );
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn perf_overlay_enabled(&self) -> bool {
    self.perf_overlay_enabled
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn tick_perf_overlay(&mut self) {
    if self.perf_overlay_enabled && Instant::now().duration_since(self.perf_overlay_last_sample) >= PERF_SAMPLE_INTERVAL
    {
      self.needs_redraw = true;
      self.pending_pass_reasons.perf_overlay = true;
    }
  }

  /// Whether something advances with time rather than input, so passes are required. A held drag is not such a
  /// source: its state changes only on input events, which request their own redraw, so a pointer held still
  /// lets the loop wait. The shell waits for these sources' deadlines and wakes ([`Tree::needs_polling`],
  /// [`Tree::next_scheduled_redraw`], [`Tree::next_timer_at`]) instead of polling while one is active.
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn has_active_tick_sources(&self) -> bool {
    self.perf_overlay_enabled
      || self.has_active_timeline()
      || self.has_continuous_redraw_image()
      || self.click_tracker.has_pending()
      || self.has_focused_blinking_text_input(CaretMode::Blinking)
      || self
        .root_ctx
        .as_ref()
        .is_some_and(|ctx| ctx.has_active_timers() || ctx.has_active_futures())
  }

  /// Whether the shell must poll its event loop: there is work for its next turn that nothing wakes the loop for.
  /// Timers, futures, streams and scheduled redraws do not count: they wake the loop or give it a deadline
  /// ([`Tree::next_scheduled_redraw`], [`Tree::next_timer_at`]).
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn needs_polling(&self) -> bool {
    #[cfg(feature = "query")]
    if self.root_ctx.as_ref().is_some_and(Ctx::has_ready_queries) {
      return true;
    }
    self.click_tracker.has_pending()
  }

  /// When the earliest running timer fires: the shell wakes then without an event, as it does for
  /// [`Tree::next_scheduled_redraw`].
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn next_timer_at(&self) -> Option<Instant> {
    self.root_ctx.as_ref().and_then(Ctx::next_timer_fire)
  }

  /// Whether any animation or transition is currently running. While true the
  /// shell asks for a frame at each [`Tree::next_timeline_change`] and the
  /// runtime recomputes layout on each of them, so this staying `true` on an
  /// idle screen is a bug (stale runs for unmounted nodes are pruned at the end
  /// of each full pass).
  pub fn has_active_timeline(&self) -> bool {
    self.transition_engine.has_active || self.animation_engine.has_active
  }

  /// When the running animations and transitions next draw something else, as of the last pass: the end of a
  /// delay, the next step of a span eased by [`Easing::Steps`](crate::animation::Easing::Steps), the next
  /// iteration or the end. A span that changes continuously returns the time of the last pass: its next frame is
  /// due at once, at the display's pace. `None` when nothing runs or nothing will change.
  ///
  /// Animations do not set [`Tree::needs_redraw`]: a shell that drives the tree itself paces its next frame from
  /// this time as the winit shell does, at `max(changes_at, presented + T/2, started + 3T/4)` for a display that
  /// refreshes every `T`, where `started` and `presented` are when the last frame's pass started and when its
  /// present returned. With a FIFO swapchain the acquire then puts each frame on its own vblank; a pace of a whole
  /// refresh from the present drifts past vblanks.
  pub fn next_timeline_change(&self) -> Option<Instant> {
    if !self.has_active_timeline() {
      return None;
    }
    let transition = self.transition_engine.next_change();
    let animation = self.animation_engine.next_change();
    transition.into_iter().chain(animation).min()
  }

  pub(crate) fn has_continuous_redraw_image(&self) -> bool {
    #[cfg(feature = "raster")]
    {
      self.cached_render_list.as_ref().is_some_and(|cached| {
        cached
          .image_sources
          .iter()
          .flatten()
          .any(crate::images::ImageData::requires_continuous_redraw)
      })
    }

    #[cfg(not(feature = "raster"))]
    {
      false
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn has_continuous_redraw_video(&self) -> bool {
    #[cfg(feature = "raster")]
    {
      self.cached_render_list.as_ref().is_some_and(|cached| {
        cached
          .video_sources
          .iter()
          .flatten()
          .any(crate::images::ImageData::requires_continuous_redraw)
      })
    }

    #[cfg(not(feature = "raster"))]
    {
      false
    }
  }

  pub fn frame_count(&self) -> u64 {
    self.frame_count
  }

  fn viewport_logical(&self) -> Size {
    let s = self.scale_factor();
    Size::new(self.viewport_physical.width / s, self.viewport_physical.height / s)
  }

  pub fn set_render_engine_factory<F>(&mut self, factory: F)
  where
    F: Fn() -> Box<dyn RenderEngine> + 'static,
  {
    let factory: RenderEngineFactory = Arc::new(factory);
    self.render_engine = Some((factory)());
    self.render_engine_factory = Some(factory);
    self.apply_render_engine_factory_to_secondaries();
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn secondary_window_count(&self) -> usize {
    self.secondary_windows.len()
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn secondary_window(&self, index: usize) -> Option<&SecondaryWindow> {
    self.secondary_windows.get(index).filter(|window| window.open)
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn secondary_window_mut(&mut self, index: usize) -> Option<&mut SecondaryWindow> {
    self.secondary_windows.get_mut(index).filter(|window| window.open)
  }

  /// The secondary window with stable id `id`, open or closed, and whether it is open.
  #[cfg(feature = "mcp")]
  pub(crate) fn secondary_tree_by_id_mut(&mut self, id: u64) -> Option<(&mut Tree, bool)> {
    self
      .secondary_windows
      .iter_mut()
      .find(|window| window.id == id)
      .map(|window| (&mut window.tree, window.open))
  }

  #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
  fn push_secondary_window(&mut self, mut window: SecondaryWindow) -> usize {
    let index = self.secondary_windows.len();
    window.id = self.next_secondary_window_id;
    self.next_secondary_window_id += 1;
    window
      .tree
      .adopt_profiling(&self.profiling, self.profiling.child_id(window.id), false, window.open);
    self.apply_render_engine_factory_to_secondary(&mut window);
    self.secondary_windows.push(window);
    index
  }

  fn adopt_profiling(
    &mut self,
    parent: &crate::app::profiler::producer::WindowProfiler,
    id: String,
    devtools: bool,
    open: bool,
  ) {
    self.profiling.attach(parent, id, devtools);
    self.profiling.set_open(open);
    for (index, child) in self.secondary_windows.iter_mut().enumerate() {
      #[cfg(feature = "devtools")]
      let devtools = self
        .devtools
        .as_ref()
        .is_some_and(|tools| tools.secondary_index == index);
      #[cfg(not(feature = "devtools"))]
      let devtools = {
        let _ = index;
        false
      };
      child
        .tree
        .adopt_profiling(&self.profiling, self.profiling.child_id(child.id), devtools, child.open);
    }
  }

  #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
  fn apply_render_engine_factory_to_secondary(&self, window: &mut SecondaryWindow) {
    if window.open
      && window.tree.render_engine.is_none()
      && let Some(factory) = &self.render_engine_factory
    {
      window.tree.render_engine = Some((factory)());
    }
  }

  fn apply_render_engine_factory_to_secondaries(&mut self) {
    let Some(factory) = self.render_engine_factory.clone() else {
      return;
    };
    for window in &mut self.secondary_windows {
      if window.open {
        window.tree.render_engine = Some((factory)());
      }
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn ensure_secondary_window_render_engine(&mut self, index: usize) {
    let Some(factory) = self.render_engine_factory.clone() else {
      return;
    };
    let Some(window) = self.secondary_windows.get_mut(index) else {
      return;
    };
    if window.open && window.tree.render_engine.is_none() {
      window.tree.render_engine = Some((factory)());
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn set_secondary_window_metadata(&mut self, index: usize, metadata: SecondaryWindowMetadata) {
    let Some(window) = self.secondary_windows.get_mut(index) else {
      return;
    };
    window.set_metadata(metadata.clone());

    #[cfg(feature = "devtools")]
    if let Some(devtools) = &mut self.devtools
      && devtools.secondary_index == index
    {
      devtools.metadata = metadata;
    }
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn close_secondary_window(&mut self, index: usize) -> bool {
    let Some(window) = self.secondary_windows.get_mut(index) else {
      return false;
    };
    if !window.close() {
      return false;
    }
    window.tree.profiling.set_open(false);
    if let Some(engine) = &mut window.tree.render_engine {
      engine.release_window_surface();
    }

    #[cfg(feature = "devtools")]
    if self
      .devtools
      .as_ref()
      .is_some_and(|devtools| devtools.secondary_index == index)
    {
      if let Some(devtools) = &mut self.devtools {
        devtools.metadata = SecondaryWindowMetadata::default();
      }
      *self.devtools_state.debug_overlay_path.lock().unwrap() = None;
      *self.devtools_state.pick_mode.lock().unwrap() = false;
      return self.clear_debug_overlay();
    }

    false
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn apply_secondary_window_requests(&mut self, app: &mut App) -> bool {
    #[allow(unused_mut)]
    let mut changed = false;
    #[cfg(feature = "devtools")]
    {
      changed |= self.apply_devtools_requests();
    }

    // User-requested windows (see `WindowOpener`): build each tree with the
    // caller's mount closure and register it open — the shell creates the OS
    // window on the next sync.
    for request in app.shared.window_opener.take() {
      let mut tree = Tree::new();
      (request.build)(app, &mut tree);
      self.push_secondary_window(
        SecondaryWindow::new(request.title, request.width, request.height, tree)
          .with_decorations(request.decorations)
          .with_name(request.name),
      );
      changed = true;
    }
    changed
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn secondary_pick_mode(&self) -> bool {
    #[cfg(feature = "devtools")]
    {
      return self.devtools_pick_mode();
    }
    #[cfg(not(feature = "devtools"))]
    false
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn pick_secondary_node_at(&mut self, _x: f32, _y: f32) -> Option<Vec<usize>> {
    #[cfg(feature = "devtools")]
    {
      return self.pick_devtools_node_at(_x, _y);
    }
    #[cfg(not(feature = "devtools"))]
    None
  }

  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn cancel_secondary_pick(&mut self) {
    #[cfg(feature = "devtools")]
    self.cancel_devtools_pick();
  }

  #[cfg(feature = "devtools")]
  pub fn mount_devtools(&mut self, app: &mut App) {
    let mut devtools = Tree::new();
    let selected_path = self.devtools_state.selected_path.lock().unwrap().clone();
    devtools.mount_root::<DevTools>(
      app,
      self.devtools_props(
        DevToolsSnapshot::from_tree_for_selection(self, &selected_path),
        self.devtools_state.snapshot_revision,
      ),
    );
    let index = self.push_secondary_window(SecondaryWindow::new_closed("lurq DevTools", 1440, 900, devtools));
    self.secondary_windows[index].tree.profiling.mark_devtools();
    self.devtools = Some(DevToolsWindow {
      secondary_index: index,
      metadata: SecondaryWindowMetadata::default(),
    });
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn open_devtools(&mut self) -> bool {
    let Some(index) = self.devtools.as_ref().map(|devtools| devtools.secondary_index) else {
      return false;
    };
    let Some(window) = self.secondary_windows.get_mut(index) else {
      return false;
    };
    if !window.open() {
      return false;
    }
    self.sync_devtools_now();
    true
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn close_devtools(&mut self) -> bool {
    let Some(index) = self.devtools.as_ref().map(|devtools| devtools.secondary_index) else {
      return false;
    };
    if !self.secondary_windows.get(index).is_some_and(|window| window.open) {
      return false;
    }
    self.close_secondary_window(index);
    true
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn toggle_devtools(&mut self) -> bool {
    let Some(index) = self.devtools.as_ref().map(|devtools| devtools.secondary_index) else {
      return false;
    };
    if self.secondary_windows.get(index).is_some_and(|window| window.open) {
      self.close_devtools()
    } else {
      self.open_devtools()
    }
  }

  #[cfg(not(feature = "devtools"))]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn open_devtools(&mut self) -> bool {
    false
  }

  #[cfg(not(feature = "devtools"))]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn close_devtools(&mut self) -> bool {
    false
  }

  #[cfg(not(feature = "devtools"))]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn toggle_devtools(&mut self) -> bool {
    false
  }

  #[cfg(feature = "devtools")]
  fn devtools_tree_mut(&mut self) -> Option<&mut Tree> {
    let index = self.devtools.as_ref()?.secondary_index;
    self.secondary_window_mut(index).map(SecondaryWindow::tree_mut)
  }

  #[cfg(feature = "devtools")]
  fn devtools_is_open(&self) -> bool {
    let Some(index) = self.devtools.as_ref().map(|devtools| devtools.secondary_index) else {
      return false;
    };
    self.secondary_window(index).is_some()
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn devtools_pick_mode(&self) -> bool {
    *self.devtools_state.pick_mode.lock().unwrap()
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn pick_devtools_node_at(&mut self, x: f32, y: f32) -> Option<Vec<usize>> {
    let picked_path = self.debug_overlay_node_at(x, y);
    *self.devtools_state.pick_mode.lock().unwrap() = false;
    self.devtools_state.picked_path = picked_path.clone();
    self.devtools_state.picked_revision = self.devtools_state.picked_revision.saturating_add(1);

    let overlay_enabled = *self.devtools_state.overlay_enabled.lock().unwrap();
    *self.devtools_state.debug_overlay_path.lock().unwrap() = if overlay_enabled { picked_path.clone() } else { None };
    self.apply_devtools_requests();
    self.sync_devtools_now();
    picked_path
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn cancel_devtools_pick(&mut self) {
    *self.devtools_state.pick_mode.lock().unwrap() = false;
    self.devtools_state.picked_path = None;
    self.devtools_state.picked_revision = self.devtools_state.picked_revision.saturating_add(1);
    self.sync_devtools_now();
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn apply_devtools_requests(&mut self) -> bool {
    let debug_overlay_path = self.devtools_state.debug_overlay_path.lock().unwrap().clone();
    let overlay_changed = match debug_overlay_path {
      Some(path) => self.draw_debug_overlay_over_node(path),
      None => self.clear_debug_overlay(),
    };

    let selected_path_dirty = {
      let mut dirty = self.devtools_state.selected_path_dirty.lock().unwrap();
      let was_dirty = *dirty;
      *dirty = false;
      was_dirty
    };
    if selected_path_dirty {
      self.sync_devtools_now();
    }

    let screenshot_requested = self.devtools_state.screenshot_request.lock().unwrap().is_some();
    if screenshot_requested {
      self.needs_redraw = true;
    }

    overlay_changed || screenshot_requested
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn sync_devtools_now(&mut self) {
    self.sync_devtools_inner(true);
  }

  #[cfg(feature = "devtools")]
  fn sync_devtools(&mut self) {
    self.sync_devtools_inner(false);
  }

  #[cfg(feature = "devtools")]
  fn sync_devtools_inner(&mut self, force: bool) {
    if !self.devtools_is_open() {
      return;
    }

    let now = Instant::now();

    if !force && self.has_active_input_interaction() {
      self.devtools_state.last_input_interaction = now;
      return;
    }

    if !force && now.duration_since(self.devtools_state.last_input_interaction) < DEVTOOLS_INTERACTION_SYNC_DELAY {
      return;
    }

    if !force && now.duration_since(self.devtools_state.last_sync) < DEVTOOLS_SYNC_INTERVAL {
      return;
    }

    let snapshot_revision = self.devtools_state.snapshot_revision.saturating_add(1);
    let selected_path = self.devtools_state.selected_path.lock().unwrap().clone();
    let props = self.devtools_props(
      DevToolsSnapshot::from_tree_for_selection(self, &selected_path),
      snapshot_revision,
    );
    let Some(devtools) = self.devtools_tree_mut() else {
      return;
    };
    devtools.update_root_props::<DevTools>(props);
    self.devtools_state.snapshot_revision = snapshot_revision;
    self.devtools_state.last_sync = now;
  }

  /// Whether the pointer currently holds an interactive drag (scrollbar,
  /// slider, text selection, or an `on_drag_*` session). The winit shell
  /// presents directly from `about_to_wait` while this is true — on Windows
  /// a continuous WM_MOUSEMOVE stream starves WM_PAINT, so redraws requested
  /// from drag handlers would otherwise only land when the mouse pauses.
  pub fn has_active_input_interaction(&self) -> bool {
    self.dragging_scroll.is_some()
      || self.dragging_slider.is_some()
      || self.dragging_text_selection.is_some()
      || self.active_drag.is_some()
  }

  #[cfg(feature = "devtools")]
  fn devtools_props(&self, snapshot: DevToolsSnapshot, snapshot_revision: u64) -> DevToolsProps {
    DevToolsProps {
      snapshot,
      snapshot_revision,
      #[cfg(feature = "persistent_storage")]
      persistent_storage_revision: self.devtools_persistent_storage_revision(),
      picked_path: self.devtools_state.picked_path.clone(),
      picked_revision: self.devtools_state.picked_revision,
      on_debug_overlay_path: Some(devtools_debug_overlay_callback(
        self.devtools_state.debug_overlay_path.clone(),
      )),
      on_overlay_enabled: Some(devtools_bool_callback(self.devtools_state.overlay_enabled.clone())),
      on_pick_inspected: Some(devtools_bool_callback(self.devtools_state.pick_mode.clone())),
      on_selected_path: Some(devtools_selected_path_callback(
        self.devtools_state.selected_path.clone(),
        self.devtools_state.selected_path_dirty.clone(),
      )),
      on_save_node_screenshot: Some(devtools_save_node_screenshot_callback(
        self.devtools_state.screenshot_request.clone(),
      )),
    }
  }

  #[cfg(all(feature = "devtools", feature = "persistent_storage"))]
  fn devtools_persistent_storage_revision(&self) -> u64 {
    self
      .root_ctx
      .as_ref()
      .map(|ctx| ctx.app_ref().persistent_storage().revision())
      .unwrap_or_default()
  }

  fn clear_animation_runtime_state(&mut self) {
    self.transition_engine.clear_state();
    self.animation_engine.clear_state();
  }

  pub fn mount_root<C: Component>(&mut self, app: &mut App, props: C::Props) {
    self.clear_focus();
    self.clear_hover_path();
    if let Some(component) = self.root_component.take() {
      component.on_unmounted();
    }
    if let Some(old) = &mut self.root {
      #[cfg(feature = "canvas")]
      detach_canvas_recursive(old);
      reset_element_ref_flags_recursive(old);
      old.free_ids(&self.id_gen);
    }
    self.clear_animation_runtime_state();
    let mut ctx = Ctx::new_root()
      .with_theme(app.theme().clone())
      .with_window(self.window.clone())
      .with_breakpoint();
    #[cfg(feature = "i18n")]
    {
      ctx = ctx.with_i18n(app.i18n().clone());
    }
    ctx.set_app_ref(app);
    ctx.set_root_props(props);
    ctx.set_component_name(std::any::type_name::<C>());
    let component = C::create(&mut ctx);
    let wrapper = RootComponentWrapper { component };
    ctx.begin_render();
    let mut node = wrapper.render(&mut ctx).node;
    ctx.end_render();
    node.set_tag_name(wrapper.tag_name());
    #[cfg(feature = "devtools")]
    set_component_debug_metadata(&mut node, &ctx);
    wrapper.on_mounted();
    self.root = Some(*node);
    if let Some(root) = &mut self.root {
      root.assign_ids(&self.id_gen);
    }
    self.root_component = Some(Box::new(wrapper));
    self.root_ctx = Some(ctx);
    self.last_layout = None;
    self.cached_render_list = None;
    self.last_theme_version = u64::MAX;
    self.active_path.clear();
    self.clear_focus();
    self.apply_focus_request();
  }

  pub(crate) fn set_app_ref(&mut self, app: &mut App) {
    if let Some(ctx) = &mut self.root_ctx {
      ctx.set_app_ref(app);
    }
    for secondary in &mut self.secondary_windows {
      secondary.tree.set_app_ref(app);
    }
  }

  pub fn update_root_props<C: Component>(&mut self, props: C::Props) {
    let changed = self.root_ctx.as_mut().is_some_and(|ctx| ctx.update_root_props(props));
    if changed {
      self.needs_redraw = true;
    }
  }

  pub fn rebuild(&mut self) {
    if self.root_component.is_none() || self.root_ctx.is_none() {
      return;
    }
    #[cfg(feature = "perf_profile")]
    let mut _profile_update = self
      .profiling
      .context
      .update(crate::app::profiler::UiUpdateKind::RootRebuild);
    if let (Some(component), Some(ctx)) = (&self.root_component, &mut self.root_ctx) {
      #[cfg(feature = "canvas")]
      let old_canvases = self.root.as_ref().map(canvas_bindings).unwrap_or_default();
      let mut old_parts = self.root.take().map(|old| {
        reset_element_ref_flags_recursive(&old);
        overlay_host_parts(old)
      });
      ctx.begin_render();
      let mut node = component.render(ctx).node;
      ctx.end_render();
      #[cfg(feature = "perf_profile")]
      _profile_update.commit();
      node.set_tag_name(component.tag_name());
      #[cfg(feature = "devtools")]
      set_component_debug_metadata(&mut node, ctx);
      if let Some(parts) = old_parts.as_mut() {
        node.preserve_runtime_state_from(&mut parts.base);
        node.preserve_ids_from(&mut parts.base);
      }
      self.root = Some(match old_parts {
        Some(parts) => root_with_preserved_overlay_parts(*node, parts, &self.id_gen),
        None => *node,
      });
      if let Some(root) = &mut self.root {
        #[cfg(feature = "canvas")]
        detach_canvases_not_carried(old_canvases, Some(root));
        root.assign_ids(&self.id_gen);
      }
      self.tree_rebuilt_since_layout = true;
      self.cached_render_list = None;
      self.refresh_interaction_state();
    }
    self.apply_focus_request();
  }

  pub fn set_root(&mut self, element: impl Into<Element>) {
    self.clear_focus();
    self.clear_hover_path();
    if let Some(component) = self.root_component.take() {
      component.on_unmounted();
    }
    let mut old_root = self.root.take();
    #[cfg(feature = "canvas")]
    let old_canvases = old_root.as_ref().map(canvas_bindings).unwrap_or_default();
    if let Some(old) = &mut old_root {
      reset_element_ref_flags_recursive(old);
    }
    self.clear_animation_runtime_state();
    let mut node = element.into().into_node();
    let old_parts = old_root.map(overlay_host_parts);
    if let Some(mut parts) = old_parts {
      node.preserve_runtime_state_from(&mut parts.base);
      node.preserve_ids_from(&mut parts.base);
      node = root_with_preserved_overlay_parts(node, parts, &self.id_gen);
    }
    #[cfg(feature = "canvas")]
    detach_canvases_not_carried(old_canvases, Some(&node));
    node.assign_ids(&self.id_gen);
    self.root = Some(node);
    self.root_component = None;
    self.root_ctx = None;
    self.last_layout = None;
    self.cached_render_list = None;
    self.last_theme_version = u64::MAX;
    self.active_path.clear();
    self.active_drag = None;
    self.clear_focus();
  }

  pub fn root(&self) -> Option<ElementRef<'_>> {
    self.root.as_ref().map(ElementRef::new)
  }

  /// DOM `document.activeElement`: the element that has focus (the control
  /// itself, not the wrapper that owns its `on_focus` handler), or `None`.
  pub fn focused_element(&self) -> Option<ElementRef<'_>> {
    let root = self.root.as_ref()?;
    let node = match self.focused_path.as_deref() {
      Some(path) => find_node_by_path(root, path).filter(|node| Some(node.node_id()) == self.focused_node),
      None => None,
    };
    node
      .or_else(|| find_node_by_id(root, self.focused_node?))
      .map(ElementRef::new)
  }

  pub fn find_element(&mut self, predicate: impl for<'a> Fn(ElementRef<'a>) -> bool) -> Option<OwnedElementRef> {
    let root = self.root.as_mut()?;
    let layout = self.last_layout.as_ref()?;
    find_element_recursive(root, layout, 0.0, 0.0, 0.0, 0.0, &predicate)
  }

  pub fn find_element_mut(&mut self, predicate: impl for<'a> Fn(ElementRef<'a>) -> bool) -> Option<OwnedElementRefMut> {
    self.find_element(predicate).map(|element_ref| element_ref.mutable())
  }

  /// Browser-style lookup: the first node in tree order whose author-supplied
  /// `id` (set via the builder `.id("...")`) matches. Pending component
  /// re-renders are flushed first so the result reflects the latest state.
  ///
  /// Unlike [`Tree::find_element`], this walks the live tree directly and so
  /// works before the first layout pass; anything that needs geometry
  /// (`bounds`, `click`) does still require a pass.
  ///
  /// Duplicate ids resolve to the first match, like the DOM; debug builds log
  /// a warning when more than one node carries the id.
  pub fn get_element_by_id(&mut self, id: &str) -> Option<ElementRef<'_>> {
    self.refresh_dirty_subtrees_settled();
    let root = self.root.as_ref()?;
    let found = find_by_element_id(root, id)?;
    #[cfg(debug_assertions)]
    {
      let count = count_element_id_matches(root, id);
      if count > 1 {
        tracing::warn!(
          target: "lurq::lookup",
          "get_element_by_id({id:?}) matched {count} nodes; returning the first in tree order"
        );
      }
    }
    Some(ElementRef::new(found))
  }

  /// All nodes carrying the given class (set via the builder `.class("...")`),
  /// in tree order. Pending component re-renders are flushed first.
  pub fn get_elements_by_class_name(&mut self, class: &str) -> Vec<ElementRef<'_>> {
    self.refresh_dirty_subtrees_settled();
    let mut found = Vec::new();
    if let Some(root) = self.root.as_ref() {
      collect_by_class(root, class, &mut found);
    }
    found
  }

  /// Like [`Tree::get_element_by_id`], but returns a short-lived
  /// [`ElementHandle`] for mutation and typed interaction. Re-resolve per
  /// lookup — handles are not meant to be stored (re-renders replace nodes
  /// wholesale, and the borrow checker enforces the short lifetime).
  pub fn get_element_by_id_mut(&mut self, id: &str) -> Option<ElementHandle<'_>> {
    self.refresh_dirty_subtrees_settled();
    let node_id = find_by_element_id(self.root.as_ref()?, id)?.node_id();
    Some(ElementHandle { tree: self, node_id })
  }

  pub fn id_gen(&self) -> &IdGenerator {
    &self.id_gen
  }

  pub fn resize(&mut self, width: u32, height: u32) {
    let size = Size::new(width as f32, height as f32);
    if self.viewport_physical == size {
      return;
    }

    self.viewport_physical = size;
    self.window.set_resolved_size(size.width, size.height);
    if let Some(engine) = &mut self.render_engine {
      engine.resize(width, height);
    }
    self.invalidate_viewport_layout();
  }

  /// Runs a pass without a window: rebuilds dirty components, lays out the
  /// tree (overlays and modals included) and updates hit-testing, focus and
  /// scroll state, but draws nothing and never touches the render engine.
  /// For headless tests of input, focus and layout; call [`Tree::resize`]
  /// first to choose the viewport.
  pub fn pass_headless(&mut self, app: &mut App) -> PassReport {
    self.request_redraw();
    self.pass(app, &HeadlessSurface)
  }

  pub fn pass(&mut self, app: &mut App, surface: &(impl HasWindowHandle + HasDisplayHandle)) -> PassReport {
    #[cfg(feature = "perf_profile")]
    let (_profile_start, _profile_phase) = self.profiling.begin_pass(self.frame_count + 1);
    #[cfg(all(feature = "canvas", feature = "perf_profile"))]
    let _canvas_text = self.profiling.context.canvas_text_scope();
    #[cfg(feature = "perf_profile")]
    if let Some(engine) = &mut self.render_engine {
      engine.set_profile_context(self.profiling.context.clone());
    }
    let report = self.pass_inner(app, surface);
    #[cfg(feature = "mcp")]
    {
      #[cfg(feature = "perf_profile")]
      let _notify_phase = self
        .profiling
        .context
        .phase(crate::app::profiler::Phase::PassNotifications);
      self.mcp_notify_pass(&report);
    }
    // Publish completion only after observers and MCP reconciliation/replies.
    // A blocked notification must remain unfinished to diagnostic readers.
    #[cfg(feature = "perf_profile")]
    self.profiling.finish_pass(
      _profile_start,
      &report,
      self
        .render_engine
        .as_ref()
        .map(|engine| engine.profile_backend())
        .unwrap_or("none"),
      report.rendered.then_some(&self.last_profile),
    );
    report
  }

  fn pass_inner(&mut self, app: &mut App, surface: &(impl HasWindowHandle + HasDisplayHandle)) -> PassReport {
    self.set_app_ref(app);
    let pass_started_at = Instant::now();
    self.tick_scheduled_redraw(Instant::now());
    let theme_version = self
      .root_ctx
      .as_ref()
      .map(|ctx| ctx.theme().version())
      .unwrap_or_else(|| app.theme().version());
    let caret_mode = self
      .root_ctx
      .as_ref()
      .map(|ctx| ctx.theme().caret_mode())
      .unwrap_or_else(|| app.theme().caret_mode());
    let mut reasons = self.pending_pass_reasons;
    reasons.merge(self.collect_pass_reasons(theme_version, caret_mode));
    let needs_redraw = self.needs_redraw();
    let has_active_tick_sources = self.has_active_tick_sources();
    let theme_changed = self.last_theme_version != theme_version;
    let required = needs_redraw || has_active_tick_sources || theme_changed;
    let mut report = PassReport {
      required,
      reasons,
      ..PassReport::default()
    };
    let setup = pass_started_at.elapsed();
    if !required {
      self.pending_pass_reasons = PassReasons::default();
      log_pass_breakdown(
        "not_required",
        pass_started_at,
        setup,
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
        Duration::ZERO,
        report,
      );
      return report;
    }
    self.pending_pass_reasons = PassReasons::default();
    #[cfg(feature = "canvas")]
    for canvas in &self.canvas_registry {
      // Acknowledge every canvas, including writes racing the reasons snapshot.
      // Do not short-circuit after the first dirty surface.
      report.reasons.redraw_requested |= canvas.consume_paint();
    }
    self.needs_redraw = false;
    self.scheduled_redraw_at = None;

    let frame_wall_start = Instant::now();
    let _frame_start = profile_scope!();
    let scale = self.scale_factor();
    // Cheap counter reset; the cache hit/miss stats feed the frame timeline
    // log (and the perf overlay when profiling).
    app.shared.glyph_engine.lock().reset_stats();
    self.update_perf_overlay_stats();

    let now = Instant::now();
    self.flush_due_pending_click(now);

    let initial_cache_start = Instant::now();
    let initial_cache_result = if self.root.is_some()
      && self.render_engine.is_some()
      && let (Ok(window), Ok(display)) = (surface.window_handle(), surface.display_handle())
    {
      let clear_color = root_clear_color(self.root.as_ref(), app);
      self.try_render_cached_render_list(app, clear_color, window, display, report.reasons)
    } else {
      None
    };
    let initial_cache = initial_cache_start.elapsed();
    match initial_cache_result {
      Some(true) => {
        report.rendered = true;
        report.used_cached_render_list = true;
        log_pass_breakdown(
          "initial_cache_hit",
          pass_started_at,
          setup,
          initial_cache,
          Duration::ZERO,
          Duration::ZERO,
          Duration::ZERO,
          Duration::ZERO,
          Duration::ZERO,
          report,
        );
        return report;
      }
      Some(false) => {
        log_pass_breakdown(
          "initial_cache_skip",
          pass_started_at,
          setup,
          initial_cache,
          Duration::ZERO,
          Duration::ZERO,
          Duration::ZERO,
          Duration::ZERO,
          Duration::ZERO,
          report,
        );
        return report;
      }
      None => {}
    }

    let layout_wall_start = Instant::now();
    let _layout_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _layout_phase = self.profiling.context.phase(crate::app::profiler::Phase::LayoutUpdate);
    let mut layout_updated = self.update_layout(app);
    // A scroll-into-view request moved a container: lay out again so this
    // frame already shows the element where it was revealed.
    if self.resolve_pending_scroll_into_view() {
      layout_updated |= self.update_layout(app);
    }
    // The layout may have moved content under a pointer that held still: the
    // hover follows, and what its handlers change (a tooltip that goes) is
    // laid out again so this frame already shows it.
    if layout_updated && self.refresh_hover_at_pointer() {
      self.update_layout(app);
    }
    self.update_text_input_caret_blink(now, caret_mode);
    let layout_wall_dur = layout_wall_start.elapsed();
    let _layout_dur = profile_elapsed!(_layout_start);
    #[cfg(feature = "perf_profile")]
    {
      self.profiling.layout(_layout_dur);
      drop(_layout_phase);
    }
    let _layout_recalculated: bool = profile_value!(layout_updated && self.layout_engine.last_recalculated());
    report.layout_updated = layout_updated;
    report.layout_recalculated = layout_updated && self.layout_engine.last_recalculated();

    if self.root.is_none() {
      return report;
    }
    if self.render_engine.is_none() {
      return report;
    }
    let clear_color = root_clear_color(self.root.as_ref(), app);

    // A surface without handles (`pass_headless`) gets layout only.
    let (Ok(window), Ok(display)) = (surface.window_handle(), surface.display_handle()) else {
      return report;
    };

    let mut second_cache = Duration::ZERO;
    if report.layout_recalculated {
      log_render_list_cache_miss_timeline("layout_recalculated", report.reasons);
    } else {
      let second_cache_start = Instant::now();
      let second_cache_result = self.try_render_cached_render_list(app, clear_color, window, display, report.reasons);
      second_cache = second_cache_start.elapsed();
      match second_cache_result {
        Some(true) => {
          report.rendered = true;
          report.used_cached_render_list = true;
          log_pass_breakdown(
            "second_cache_hit",
            pass_started_at,
            setup,
            initial_cache,
            layout_wall_dur,
            second_cache,
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
            report,
          );
          return report;
        }
        Some(false) => {
          log_pass_breakdown(
            "second_cache_skip",
            pass_started_at,
            setup,
            initial_cache,
            layout_wall_dur,
            second_cache,
            Duration::ZERO,
            Duration::ZERO,
            Duration::ZERO,
            report,
          );
          return report;
        }
        None => {}
      }
    }

    let root = match &self.root {
      Some(r) => r,
      None => return report,
    };

    let result = match self.last_layout.take() {
      Some(result) => result,
      None => return report,
    };

    let quad_wall_start = Instant::now();
    let _quad_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _quad_phase = self.profiling.context.phase(crate::app::profiler::Phase::QuadResolve);
    let viewport_clip = ClipRect {
      x: 0.0,
      y: 0.0,
      width: self.viewport_physical.width / scale,
      height: self.viewport_physical.height / scale,
      active: true,
      border_radius: None,
    };
    let mut quads = std::mem::take(&mut self.quad_scratch);
    quads.clear();
    self
      .layout_engine
      .resolve_quads_with_viewport_into(root, &result, viewport_clip, &mut quads);
    let mut opacity_groups = std::mem::take(&mut self.opacity_groups);
    self.layout_engine.take_opacity_groups(&mut opacity_groups);
    #[cfg(feature = "screenshot")]
    let (mut sensitive_quads, mut capture_redactions) = {
      let mut sensitive_quads = std::mem::take(&mut self.sensitive_quads);
      self.layout_engine.take_sensitive_quads(&mut sensitive_quads);
      let mut capture_redactions = std::mem::take(&mut self.capture_redactions);
      capture_redactions.clear();
      (sensitive_quads, capture_redactions)
    };
    let quad_wall_dur = quad_wall_start.elapsed();
    let _quad_dur = profile_elapsed!(_quad_start);
    #[cfg(feature = "perf_profile")]
    drop(_quad_phase);
    let quad_count = quads.len();
    #[cfg(feature = "devtools")]
    let devtools_overlay = self.devtools_overlay_target(root, &result);

    self.last_layout = Some(result);

    let mut glyph_engine = app.shared.glyph_engine.lock();
    let glyph_wall_start = Instant::now();
    let _glyph_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _glyph_phase = self
      .profiling
      .context
      .phase(crate::app::profiler::Phase::GlyphRasterize);
    let mut rects = std::mem::take(&mut self.render_rects);
    rects.clear();
    rects.reserve(quad_count);
    let mut glyphs = std::mem::take(&mut self.render_glyphs);
    glyphs.clear();
    glyphs.reserve(quad_count * 4);
    #[cfg(feature = "raster")]
    let mut images = {
      let mut images = std::mem::take(&mut self.render_images);
      images.clear();
      images
    };
    #[cfg(feature = "raster")]
    let image_frame_time = std::time::Instant::now();
    #[cfg(feature = "raster")]
    let mut image_sources = Vec::new();
    #[cfg(feature = "raster")]
    let mut video_sources = Vec::new();
    #[cfg(all(feature = "svg", feature = "raster"))]
    let svgs = {
      let mut svgs = std::mem::take(&mut self.render_svgs);
      svgs.clear();
      svgs
    };
    #[cfg(all(feature = "svg", not(feature = "raster")))]
    let mut svgs = {
      let mut svgs = std::mem::take(&mut self.render_svgs);
      svgs.clear();
      svgs
    };

    // `LURQ_GLYPH_DEBUG` — collect the marked text runs' bboxes+orders so the
    // end of the build can report which rects paint over them.
    let glyph_debug_marker = crate::app::glyph_engine::glyph_debug_marker();
    let mut glyph_debug_regions: Vec<(String, f32, f32, f32, f32, usize)> = Vec::new();

    for (order, quad) in quads.iter().enumerate() {
      let scaled_clip = if quad.clip.active {
        ClipRect {
          x: quad.clip.x * scale,
          y: quad.clip.y * scale,
          width: quad.clip.width * scale,
          height: quad.clip.height * scale,
          active: true,
          border_radius: quad.clip.border_radius.map(|radius| crate::node::border::BorderRadius {
            top_left: radius.top_left * scale,
            top_right: radius.top_right * scale,
            bottom_right: radius.bottom_right * scale,
            bottom_left: radius.bottom_left * scale,
          }),
        }
      } else {
        ClipRect::default()
      };

      let scaled_x = quad.x * scale;
      let scaled_y = quad.y * scale;
      let scaled_width = quad.width * scale;
      let scaled_height = quad.height * scale;
      let cull_clip = if matches!(
        &quad.content,
        QuadContent::Text { .. } | QuadContent::RichText { .. } | QuadContent::DecoratedInput { .. }
      ) {
        expand_text_clip_for_culling(scaled_clip)
      } else {
        scaled_clip
      };
      // A shadow paints beyond its quad.
      let cull_outset = match &quad.content {
        QuadContent::BoxShadow(shadow) => shadow.outset() * scale,
        _ => 0.0,
      };
      if quad.transform.is_identity()
        && cull_clip.active
        && !rect_intersects_clip(
          scaled_x - cull_outset,
          scaled_y - cull_outset,
          scaled_width + 2.0 * cull_outset,
          scaled_height + 2.0 * cull_outset,
          cull_clip,
        )
      {
        continue;
      }

      match &quad.content {
        QuadContent::Rect { color, gradient } => {
          let (x, y, w, h) = (quad.x * scale, quad.y * scale, quad.width * scale, quad.height * scale);
          let radii = scaled_radii(quad.border_radius, scale, w, h);
          let final_color = apply_opacity(*color, quad.opacity);
          let gradient = gradient
            .as_ref()
            .map(|gradient| apply_opacity_gradient(gradient, quad.opacity));
          let xf = quad.transform.matrix_2x2();
          let xf_origin = quad
            .transform_origin
            .map(|[x, y]| [x * scale, y * scale])
            .unwrap_or([w * 0.5, h * 0.5]);

          rects.push(RectCmd {
            order,
            x,
            y,
            width: w,
            height: h,
            color: final_color,
            radii,
            stroke: [0.0; 4],
            stroke_color: TRANSPARENT_COLOR,
            transform: xf,
            transform_origin: xf_origin,
            clip: scaled_clip,
            gradient,
            shadow: None,
          });

          if let Some(borders) = quad.border {
            push_border_rects(
              &mut rects,
              order,
              x,
              y,
              w,
              h,
              scale,
              quad.border_radius,
              borders,
              quad.opacity,
              xf,
              xf_origin,
              scaled_clip,
            );
          }
        }
        QuadContent::Text {
          text,
          style,
          wrap,
          vertical_align,
          transform_mode,
        } => {
          let glyph_start = glyphs.len();
          let mut scaled_style = style.clone();
          scaled_style.scale_pixels(scale);
          // Layout shapes in logical pixels, while paint reshapes at the DPI-scaled
          // font size. Round outward so subpixel scaling cannot add a paint-only line.
          let max_width =
            if (*wrap || style.text_align != crate::layout::text_style::TextAlign::Left) && quad.width > 0.0 {
              (quad.width * scale).ceil()
            } else {
              f32::MAX
            };
          let glyph_xf = quad.transform.matrix_2x2();
          let glyph_origin = quad
            .transform_origin
            .map(|[x, y]| [(quad.x + x) * scale, (quad.y + y) * scale])
            .unwrap_or([
              quad.x * scale + quad.width * scale * 0.5,
              quad.y * scale + quad.height * scale * 0.5,
            ]);
          let glyph_clip = expand_text_clip_for_rasterization(scaled_clip);
          let text_y = scaled_y
            + glyph_engine.text_vertical_align_offset(
              text,
              &scaled_style,
              max_width,
              *wrap,
              *vertical_align,
              scaled_height,
            );
          if quad.transform.is_identity() {
            glyph_engine.rasterize_text_with_wrap_clipped_into(
              text,
              &scaled_style,
              max_width,
              *wrap,
              quad.x * scale,
              text_y,
              glyph_clip,
              &mut glyphs,
            );
          } else if *transform_mode == TextTransformMode::Rasterized {
            glyph_engine.rasterize_text_with_baked_transform_into(
              text,
              &scaled_style,
              max_width,
              *wrap,
              quad.x * scale,
              text_y,
              quad.transform,
              glyph_origin,
              &mut glyphs,
            );
          } else {
            let raster_scale = transformed_text_raster_scale(quad.transform);
            scaled_style.scale_pixels(raster_scale);
            let raster_max_width = if max_width.is_finite() {
              max_width * raster_scale
            } else {
              f32::MAX
            };
            let raster_x = quad.x * scale * raster_scale;
            let raster_y = text_y * raster_scale;
            let unsnapped_start = glyphs.len();
            glyph_engine.rasterize_text_unsnapped_with_wrap_into(
              text,
              &scaled_style,
              raster_max_width,
              *wrap,
              raster_x,
              raster_y,
              &mut glyphs,
            );
            for glyph in &mut glyphs[unsnapped_start..] {
              glyph.x /= raster_scale;
              glyph.y /= raster_scale;
              glyph.width /= raster_scale;
              glyph.height /= raster_scale;
            }
          }
          for g in &mut glyphs[glyph_start..] {
            g.order = order;
            g.clip = glyph_clip;
            if !quad.transform.is_identity() && *transform_mode == TextTransformMode::Bitmap {
              g.transform = glyph_xf;
              g.transform_origin = [glyph_origin[0] - g.x, glyph_origin[1] - g.y];
            }
          }
          if let Some(marker) = glyph_debug_marker
            .as_ref()
            .filter(|marker| text.contains(marker.as_str()))
          {
            let run = &glyphs[glyph_start..];
            let x0 = run.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
            let y0 = run.iter().map(|g| g.y).fold(f32::INFINITY, f32::min);
            let x1 = run.iter().map(|g| g.x + g.width).fold(f32::NEG_INFINITY, f32::max);
            let y1 = run.iter().map(|g| g.y + g.height).fold(f32::NEG_INFINITY, f32::max);
            eprintln!(
              "[glyph-debug:{marker}] ORDER text={text:?} order={order} glyphs={} bbox=({x0}, {y0})..({x1}, {y1}) color={:?} opacity={} sharpness={:?}",
              run.len(),
              run.first().map(|g| g.color),
              quad.opacity,
              run.first().map(|g| g.sharpness),
            );
            glyph_debug_regions.push((text.clone(), x0, y0, x1, y1, order));
            if let Some(force) = crate::app::glyph_engine::glyph_debug_force() {
              let noclip = force == "noclip" || force == "both";
              let lift = force == "order" || force == "both";
              for g in &mut glyphs[glyph_start..] {
                if noclip {
                  g.clip = ClipRect::default();
                }
                if lift {
                  g.order += 10_000;
                }
              }
              eprintln!("[glyph-debug:{marker}] FORCED {force} on {text:?}");
            }
          }
          if let Some(shadow) = &style.shadow {
            append_text_shadow_glyphs(&mut glyphs, glyph_start, shadow, scale);
          }
          // Rects premultiply quad opacity into their color above; glyphs
          // (including the shadow run just appended) must do the same or text
          // stays fully opaque inside a faded subtree.
          if quad.opacity < 1.0 {
            let factor = quad.opacity.clamp(0.0, 1.0);
            for g in &mut glyphs[glyph_start..] {
              g.color[3] *= factor;
            }
          }
          #[cfg(feature = "screenshot")]
          if sensitive_quads.binary_search(&order).is_ok() {
            capture_redactions.extend(crate::app::capture_redaction::RedactedArea::of_glyph_run(
              &glyphs[glyph_start..],
            ));
          }
        }
        QuadContent::DecoratedInput { layout } => {
          let _range = input_token_paint::resolve_input_quad(
            &mut glyph_engine,
            layout,
            quad,
            scale,
            order,
            expand_text_clip_for_rasterization(scaled_clip),
            &mut glyphs,
          );
          #[cfg(feature = "screenshot")]
          if sensitive_quads.binary_search(&order).is_ok() {
            capture_redactions.extend(crate::app::capture_redaction::RedactedArea::of_glyph_run(
              &glyphs[_range],
            ));
          }
        }
        QuadContent::RichText {
          spans,
          wrap,
          vertical_align,
          transform_mode,
        } => {
          let glyph_start = glyphs.len();
          let mut scaled_spans = spans.clone();
          for span in &mut scaled_spans {
            span.style.scale_pixels(scale);
          }
          let align = scaled_spans
            .first()
            .map(|span| span.style.text_align)
            .unwrap_or(crate::layout::text_style::TextAlign::Left);
          let max_width = if (*wrap || align != crate::layout::text_style::TextAlign::Left) && quad.width > 0.0 {
            (quad.width * scale).ceil()
          } else {
            f32::MAX
          };
          let glyph_xf = quad.transform.matrix_2x2();
          let glyph_origin = quad
            .transform_origin
            .map(|[x, y]| [(quad.x + x) * scale, (quad.y + y) * scale])
            .unwrap_or([
              quad.x * scale + quad.width * scale * 0.5,
              quad.y * scale + quad.height * scale * 0.5,
            ]);
          let glyph_clip = expand_text_clip_for_rasterization(scaled_clip);
          let text_y = scaled_y
            + glyph_engine.rich_text_vertical_align_offset(
              &scaled_spans,
              1.0,
              max_width,
              *wrap,
              *vertical_align,
              scaled_height,
            );
          if quad.transform.is_identity() {
            glyph_engine.rasterize_rich_text_with_wrap_clipped_into(
              &scaled_spans,
              max_width,
              *wrap,
              quad.x * scale,
              text_y,
              glyph_clip,
              &mut glyphs,
            );
          } else if *transform_mode == TextTransformMode::Rasterized {
            glyph_engine.rasterize_rich_text_with_baked_transform_into(
              &scaled_spans,
              max_width,
              *wrap,
              quad.x * scale,
              text_y,
              quad.transform,
              glyph_origin,
              &mut glyphs,
            );
          } else {
            let raster_scale = transformed_text_raster_scale(quad.transform);
            for span in &mut scaled_spans {
              span.style.scale_pixels(raster_scale);
            }
            let raster_max_width = if max_width.is_finite() {
              max_width * raster_scale
            } else {
              f32::MAX
            };
            let raster_x = quad.x * scale * raster_scale;
            let raster_y = text_y * raster_scale;
            let unsnapped_start = glyphs.len();
            glyph_engine.rasterize_rich_text_unsnapped_with_wrap_into(
              &scaled_spans,
              raster_max_width,
              *wrap,
              raster_x,
              raster_y,
              &mut glyphs,
            );
            for glyph in &mut glyphs[unsnapped_start..] {
              glyph.x /= raster_scale;
              glyph.y /= raster_scale;
              glyph.width /= raster_scale;
              glyph.height /= raster_scale;
            }
          }
          for g in &mut glyphs[glyph_start..] {
            g.order = order;
            g.clip = glyph_clip;
            if !quad.transform.is_identity() && *transform_mode == TextTransformMode::Bitmap {
              g.transform = glyph_xf;
              g.transform_origin = [glyph_origin[0] - g.x, glyph_origin[1] - g.y];
            }
          }
          if let Some(shadow) = spans.first().and_then(|span| span.style.shadow) {
            append_text_shadow_glyphs(&mut glyphs, glyph_start, &shadow, scale);
          }
          if quad.opacity < 1.0 {
            let factor = quad.opacity.clamp(0.0, 1.0);
            for g in &mut glyphs[glyph_start..] {
              g.color[3] *= factor;
            }
          }
        }
        #[cfg(feature = "raster")]
        QuadContent::Image { data, uv_min, uv_max } | QuadContent::Video { data, uv_min, uv_max } => {
          let is_video = matches!(&quad.content, QuadContent::Video { .. });
          let frame = data.frame_at(image_frame_time);
          if data.requires_continuous_redraw() {
            log_draw_image_timeline_sampled(
              &IMAGE_BUILD_TIMELINE_LAST_INFO_MS,
              "build",
              data.id(),
              frame.version,
              frame.frame_index,
            );
          }
          if let Some(next_frame_at) = frame.next_frame_at
            && !data.requires_continuous_redraw()
          {
            self.request_redraw_at(next_frame_at);
          }
          let image_transform = quad.transform.matrix_2x2();
          let image_transform_origin = quad
            .transform_origin
            .map(|[x, y]| [x * scale, y * scale])
            .unwrap_or([quad.width * scale * 0.5, quad.height * scale * 0.5]);
          let max_r = scaled_width.min(scaled_height) * 0.5;
          let radii = quad
            .border_radius
            .map(|r| {
              [
                (r.top_left * scale).min(max_r),
                (r.top_right * scale).min(max_r),
                (r.bottom_right * scale).min(max_r),
                (r.bottom_left * scale).min(max_r),
              ]
            })
            .unwrap_or([0.0; 4]);
          images.push(crate::images::ImageCmd {
            order,
            x: quad.x * scale,
            y: quad.y * scale,
            width: quad.width * scale,
            height: quad.height * scale,
            image_id: data.id(),
            frame_index: frame.frame_index,
            version: frame.version,
            data: frame.data,
            animation_frames: frame.animation_frames,
            native: frame.native,
            image_width: frame.width,
            image_height: frame.height,
            image_format: frame.format,
            uv_min: *uv_min,
            uv_max: *uv_max,
            radii,
            transform: image_transform,
            transform_origin: image_transform_origin,
            clip: scaled_clip,
            opacity: quad.opacity,
          });
          image_sources.push(Some(data.clone()));
          video_sources.push(is_video.then(|| data.clone()));
        }
        #[cfg(all(feature = "svg", feature = "raster"))]
        QuadContent::Svg { data } => {
          let w = quad.width * scale;
          let h = quad.height * scale;
          // Quad opacity rides the image command, not the raster: a faded
          // icon shares the cache entry with its opaque siblings.
          let raster = crate::svg::rasterize::rasterize(&data, w, h);
          let image_transform = quad.transform.matrix_2x2();
          let image_transform_origin = quad
            .transform_origin
            .map(|[x, y]| [x * scale, y * scale])
            .unwrap_or([w * 0.5, h * 0.5]);
          images.push(crate::images::ImageCmd {
            order,
            x: quad.x * scale,
            y: quad.y * scale,
            width: w,
            height: h,
            image_id: raster.image_id,
            frame_index: 0,
            version: 0,
            data: raster.data,
            animation_frames: None,
            native: None,
            image_width: raster.width,
            image_height: raster.height,
            image_format: crate::images::ImagePixelFormat::Rgba8,
            uv_min: [0.0, 0.0],
            uv_max: [1.0, 1.0],
            radii: [0.0; 4],
            transform: image_transform,
            transform_origin: image_transform_origin,
            clip: scaled_clip,
            opacity: quad.opacity,
          });
          image_sources.push(None);
          video_sources.push(None);
        }
        #[cfg(all(feature = "svg", not(feature = "raster")))]
        QuadContent::Svg { data } => {
          let w = quad.width * scale;
          let h = quad.height * scale;
          // SvgCmd carries no opacity, so the quad's fade folds into the
          // data's own override before tessellation (it participates in the
          // mesh identity, so faded and opaque copies never share vertices).
          let data = if quad.opacity < 1.0 {
            data
              .clone()
              .with_opacity(data.opacity_override().unwrap_or(1.0) * quad.opacity)
          } else {
            data.clone()
          };
          let mesh = crate::svg::tessellate::tessellate(&data, w, h);
          svgs.push(crate::svg::SvgCmd {
            order,
            x: quad.x * scale,
            y: quad.y * scale,
            width: w,
            height: h,
            svg_id: data.id(),
            mesh: std::sync::Arc::new(mesh),
            clip: scaled_clip,
          });
        }
        QuadContent::BoxShadow(shadow) => {
          rects.push(crate::layout::box_shadow::shadow_rect_cmd(
            order,
            quad,
            shadow,
            scale,
            scaled_clip,
          ));
        }
        QuadContent::None => {}
      }
    }

    quads.clear();
    self.quad_scratch = quads;

    let mut layers = std::mem::take(&mut self.render_layers);
    crate::layout::opacity_layer::resolve_layers(
      &opacity_groups,
      quad_count,
      [self.viewport_physical.width, self.viewport_physical.height],
      &rects,
      &glyphs,
      #[cfg(feature = "raster")]
      &images,
      #[cfg(feature = "svg")]
      &svgs,
      &mut layers,
    );
    self.opacity_groups = opacity_groups;
    #[cfg(feature = "screenshot")]
    {
      sensitive_quads.clear();
      self.sensitive_quads = sensitive_quads;
      self.capture_redactions = capture_redactions;
    }

    #[cfg(feature = "screenshot")]
    let frame_capture = if self
      .render_engine
      .as_ref()
      .is_some_and(|render_engine| render_engine.supports_frame_capture())
    {
      self.take_pending_frame_capture()
    } else {
      self.drop_unsupported_screenshot();
      #[cfg(feature = "devtools")]
      {
        #[cfg(feature = "raster")]
        self.save_pending_devtools_screenshot(clear_color, &rects, &glyphs, &images, &layers, &glyph_engine.atlas());
        #[cfg(not(feature = "raster"))]
        self.save_pending_devtools_screenshot(clear_color, &rects, &glyphs, &layers, &glyph_engine.atlas());
      }
      None
    };

    #[cfg(feature = "devtools")]
    if frame_capture.is_none() {
      push_devtools_overlay(
        &mut rects,
        &mut glyphs,
        &mut glyph_engine,
        devtools_overlay,
        quad_count,
        scale,
        self.viewport_physical,
      );
    }
    #[cfg(feature = "screenshot")]
    let perf_overlay = if frame_capture.is_some() {
      None
    } else {
      self.perf_overlay_enabled.then_some(self.perf_overlay_stats)
    };
    #[cfg(not(feature = "screenshot"))]
    let perf_overlay = self.perf_overlay_enabled.then_some(self.perf_overlay_stats);
    push_perf_meter(
      &mut rects,
      &mut glyphs,
      &mut glyph_engine,
      perf_overlay,
      quad_count + 20_000,
      scale,
      self.viewport_physical,
    );

    if let Some(marker) = &glyph_debug_marker {
      for (text, x0, y0, x1, y1, text_order) in &glyph_debug_regions {
        for rect in &rects {
          let covers = rect.order >= *text_order
            && rect.x < *x1
            && rect.x + rect.width > *x0
            && rect.y < *y1
            && rect.y + rect.height > *y0;
          if covers {
            eprintln!(
              "[glyph-debug:{marker}] COVERING RECT over {text:?} (text order {text_order}): order={} at ({}, {}) {}x{} radii={:?} rgba=({}, {}, {}, {})",
              rect.order,
              rect.x,
              rect.y,
              rect.width,
              rect.height,
              rect.radii,
              rect.color.r(),
              rect.color.g(),
              rect.color.b(),
              rect.color.a(),
            );
          }
        }
      }
    }

    let glyph_wall_dur = glyph_wall_start.elapsed();
    let _glyph_dur = profile_elapsed!(_glyph_start);
    #[cfg(feature = "perf_profile")]
    drop(_glyph_phase);
    let _rect_count: usize = profile_value!(rects.len());
    let _glyph_count: usize = profile_value!(glyphs.len());

    let list = RenderList {
      clear_color,
      rects,
      glyphs,
      #[cfg(feature = "raster")]
      images,
      #[cfg(feature = "svg")]
      svgs,
      layers,
      atlas: glyph_engine.atlas(),
    };

    drop(glyph_engine);

    let gpu_wall_start = Instant::now();
    let _gpu_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _render_phase = self.profiling.context.phase(crate::app::profiler::Phase::Render);
    let Some(render_engine) = &mut self.render_engine else {
      return report;
    };
    #[cfg(feature = "canvas")]
    {
      let _canvas_start = profile_scope!();
      #[cfg(feature = "perf_profile")]
      let _canvas_phase = self
        .profiling
        .context
        .phase(crate::app::profiler::Phase::CanvasPreparation);
      render_engine.prepare_canvases(&self.canvas_registry);
      #[cfg(feature = "perf_profile")]
      self.profiling.canvas_preparation(profile_elapsed!(_canvas_start));
    }
    let rendered = {
      #[cfg(feature = "screenshot")]
      {
        render_engine.render_with_capture(&list, window, display, frame_capture)
      }
      #[cfg(not(feature = "screenshot"))]
      {
        render_engine.render(&list, window, display)
      }
    };
    if !rendered {
      self.needs_redraw = true;
      return report;
    }
    report.rendered = true;
    if let Some(root) = self.root.as_ref() {
      root.clear_guards();
    }
    let gpu_wall_dur = gpu_wall_start.elapsed();
    let _gpu_dur = profile_elapsed!(_gpu_start);
    #[cfg(feature = "perf_profile")]
    drop(_render_phase);
    let renderer_wants_redraw = render_engine.wants_redraw();

    let (measure_stats, glyph_stats) = {
      let engine = app.shared.glyph_engine.lock();
      (
        (engine.measure_hits, engine.measure_misses),
        (engine.glyph_hits, engine.glyph_misses),
      )
    };
    profile_if! {
      let render_profile = render_engine.last_profile();
      let glyph_engine_profile = app.shared.glyph_engine.lock().profile();
      self.last_profile = FrameProfile {
        layout: _layout_dur,
        layout_recalculated: _layout_recalculated,
        quad_resolve: _quad_dur,
        glyph_rasterize: _glyph_dur,
        gpu_submit: _gpu_dur,
        render: render_profile.unwrap_or_default(),
        render_profile_available: render_profile.is_some(),
        total: profile_elapsed!(_frame_start),
        quad_count,
        rect_count: _rect_count,
        glyph_count: _glyph_count,
        glyph_cache_hits: glyph_stats.0,
        glyph_cache_misses: glyph_stats.1,
        text_measure_cache_hits: measure_stats.0,
        text_measure_cache_misses: measure_stats.1,
        glyph_engine: glyph_engine_profile,
        memory: self.cached_memory_profile(app),
      };
      crate::app::profiler::notify_frame_profile(&self.last_profile);
    }
    if renderer_wants_redraw {
      self.needs_redraw = true;
    }
    log_frame_pass_timeline(
      "full",
      frame_wall_start.elapsed(),
      layout_wall_dur,
      quad_wall_dur,
      glyph_wall_dur,
      gpu_wall_dur,
      report,
      quad_count,
      list.rects.len(),
      list.glyphs.len(),
      measure_stats,
      glyph_stats,
    );
    log_pass_breakdown(
      "full",
      pass_started_at,
      setup,
      initial_cache,
      layout_wall_dur,
      second_cache,
      quad_wall_dur,
      glyph_wall_dur,
      gpu_wall_dur,
      report,
    );

    #[cfg(feature = "raster")]
    let should_cache_render_list = self.should_store_cached_render_list();
    #[cfg(not(feature = "raster"))]
    let should_cache_render_list = false;

    if should_cache_render_list {
      self.cached_render_list = Some(CachedRenderList {
        list,
        #[cfg(feature = "raster")]
        image_sources,
        #[cfg(feature = "raster")]
        video_sources,
      });
      self.scheduled_redraw_due = false;
      self.frame_count += 1;
      #[cfg(feature = "devtools")]
      self.sync_devtools();
      return report;
    }

    self.cached_render_list = None;
    let RenderList {
      clear_color: _,
      mut rects,
      mut glyphs,
      #[cfg(feature = "raster")]
      mut images,
      #[cfg(feature = "svg")]
      mut svgs,
      mut layers,
      atlas: _,
    } = list;
    rects.clear();
    glyphs.clear();
    layers.clear();
    self.render_rects = rects;
    self.render_glyphs = glyphs;
    self.render_layers = layers;
    #[cfg(feature = "raster")]
    {
      images.clear();
      self.render_images = images;
    }
    #[cfg(feature = "svg")]
    {
      svgs.clear();
      self.render_svgs = svgs;
    }
    self.scheduled_redraw_due = false;
    self.frame_count += 1;
    #[cfg(feature = "devtools")]
    self.sync_devtools();
    report
  }

  pub fn mouse_move(&mut self, x: f32, y: f32) {
    self.mouse_move_with_meta(x, y, false, false, false, false);
  }

  pub fn mouse_move_with_modifiers(&mut self, x: f32, y: f32, shift: bool, ctrl: bool, alt: bool) {
    self.mouse_move_with_meta(x, y, shift, ctrl, alt, false);
  }

  /// [`mouse_move_with_modifiers`](Self::mouse_move_with_modifiers) with the
  /// Meta key (Cmd on macOS, the Windows key on Windows) too.
  pub fn mouse_move_with_meta(&mut self, x: f32, y: f32, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Pointer);
    self.track_pointer(x, y);
    let active_scrollbar_drag = self.dragging_scroll.is_some();
    self.dispatch_mouse(
      x,
      y,
      MouseButton::Left,
      MouseEventKind::Move,
      MouseModifiers { shift, ctrl, alt, meta },
    );
    if !active_scrollbar_drag {
      self.apply_reactive_updates_after_event();
    }
  }

  pub fn mouse_leave_window(&mut self) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Pointer);
    self.pointer = None;
    self.clear_hover_path();
    self.apply_reactive_updates_after_event();
  }

  pub fn mouse_down(&mut self, x: f32, y: f32, button: MouseButton) {
    self.mouse_down_with_meta(x, y, button, false, false, false, false);
  }

  pub fn mouse_down_with_modifiers(&mut self, x: f32, y: f32, button: MouseButton, shift: bool, ctrl: bool, alt: bool) {
    self.mouse_down_with_meta(x, y, button, shift, ctrl, alt, false);
  }

  /// [`mouse_down_with_modifiers`](Self::mouse_down_with_modifiers) with the
  /// Meta key (Cmd on macOS, the Windows key on Windows) too.
  pub fn mouse_down_with_meta(
    &mut self,
    x: f32,
    y: f32,
    button: MouseButton,
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
  ) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Pointer);
    self.track_pointer(x, y);
    self.set_input_modality(focus_visible::InputModality::Pointer);
    let modifiers = MouseModifiers { shift, ctrl, alt, meta };
    if self.swallowed_press == Some(button) {
      // Its release never arrived (the OS took it); this is a new press.
      self.swallowed_press = None;
    }
    let scale = self.scale_factor();
    if button == MouseButton::Left && self.consume_outside_press(x / scale, y / scale) {
      self.swallowed_press = Some(button);
      self.click_press = None;
      self.click_tracker.take_pending();
      self.apply_reactive_updates_after_event();
      return;
    }
    let position = (x, y);
    let target_ids = self.hit_target_ids_at(x, y);
    let transient_root = self.root.as_ref().and_then(|root| {
      target_ids.iter().copied().find(|target_id| {
        find_node_by_id(root, *target_id).is_some_and(|node| node.has_synthetic_role(SyntheticNodeRole::SelectMenu))
      })
    });

    if button == MouseButton::Left {
      let now = Instant::now();
      if let Some(click_target_id) = self
        .click_tracker
        .pending_match_for_targets(now, position, button, &target_ids)
      {
        self.click_press = None;
        self.click_tracker.take_pending();
        self.dispatch_mouse_with_click_target(
          x,
          y,
          button,
          MouseEventKind::DoubleClick,
          modifiers,
          Some(click_target_id),
        );
        self.apply_reactive_updates_after_event();
        return;
      }
    }

    self.click_press = Some(ClickPress {
      position,
      button,
      target_ids,
      transient_root,
    });
    self.dispatch_mouse(x, y, button, MouseEventKind::Down, modifiers);
    self.apply_reactive_updates_after_event();
  }

  pub fn mouse_up(&mut self, x: f32, y: f32, button: MouseButton) {
    self.mouse_up_with_meta(x, y, button, false, false, false, false);
  }

  pub fn mouse_up_with_modifiers(&mut self, x: f32, y: f32, button: MouseButton, shift: bool, ctrl: bool, alt: bool) {
    self.mouse_up_with_meta(x, y, button, shift, ctrl, alt, false);
  }

  /// [`mouse_up_with_modifiers`](Self::mouse_up_with_modifiers) with the
  /// Meta key (Cmd on macOS, the Windows key on Windows) too.
  pub fn mouse_up_with_meta(
    &mut self,
    x: f32,
    y: f32,
    button: MouseButton,
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
  ) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Pointer);
    self.track_pointer(x, y);
    let modifiers = MouseModifiers { shift, ctrl, alt, meta };
    if self.swallowed_press == Some(button) {
      self.swallowed_press = None;
      // The pointer now hovers whatever the closed popup exposed.
      self.dispatch_mouse(x, y, button, MouseEventKind::Move, modifiers);
      self.apply_reactive_updates_after_event();
      return;
    }
    let transient_root = self
      .click_press
      .as_ref()
      .filter(|press| press.button == button)
      .and_then(|press| press.transient_root);
    let transient_disappeared =
      transient_root.is_some_and(|target_id| !self.hit_target_ids_at(x, y).contains(&target_id));
    if transient_disappeared {
      // A transient overlay may commit and disappear on press. Keep the
      // remainder of that pointer sequence from falling through to content
      // newly exposed beneath it.
      self.clear_active_path();
      self.needs_redraw = true;
      self.synthesize_click(x, y, button, modifiers);
      // Pointer capture ends with the release. Re-evaluate hover immediately
      // because the stationary pointer is now over the content exposed by
      // the closed overlay.
      self.dispatch_mouse(x, y, button, MouseEventKind::Move, modifiers);
      self.apply_reactive_updates_after_event();
      return;
    }
    self.dispatch_mouse(x, y, button, MouseEventKind::Up, modifiers);
    self.synthesize_click(x, y, button, modifiers);
  }

  /// Ends a press the OS took over, for example to run a native window move or resize loop.
  ///
  /// Such a loop consumes the release, so the shell calls this once the loop owns the button. The
  /// release takes the normal mouse-up path, clearing active styles, scrollbar, slider, and text
  /// selection drags, but produces no click: the press moved the window, it did not press the element.
  /// An `on_drag_*` session ends as a miss without reaching a drop target. Does nothing unless
  /// `button` is held.
  pub fn mouse_press_taken_by_os(&mut self, x: f32, y: f32, button: MouseButton) {
    let pressed = self.click_press.as_ref().is_some_and(|press| press.button == button)
      || self.active_drag.as_ref().is_some_and(|drag| drag.button == button)
      || self.dragging_scroll.is_some()
      || self.dragging_slider.is_some()
      || self.dragging_text_selection.is_some();
    if !pressed {
      return;
    }

    self.click_press = None;
    if self.active_drag.as_ref().is_some_and(|drag| drag.button == button) {
      let drag = self.active_drag.take().unwrap();
      let scale = self.scale_factor();
      let event = drag.event(x / scale, y / scale, Some(DropResult::Missed));
      for handler in drag.on_end {
        handler.call(&event);
      }
    }
    self.dispatch_mouse(x, y, button, MouseEventKind::Up, MouseModifiers::default());
    self.clear_active_path();
    // Drags that end on release arm a suppression for the click that release would produce. There
    // is no such click here, so the suppression would swallow the next real one instead.
    self.suppressed_click = None;
    self.needs_redraw = true;
    self.apply_reactive_updates_after_event();
  }

  fn synthesize_click(&mut self, x: f32, y: f32, button: MouseButton, modifiers: MouseModifiers) {
    let now = Instant::now();
    let position = (x, y);

    // Drag handlers record suppression after dispatch converts pointer coordinates to logical units.
    let scale = self.scale_factor();
    let logical_position = (x / scale, y / scale);
    if self.should_suppress_click(now, logical_position, button) {
      self.click_press = None;
      self.click_tracker.take_pending();
      self.apply_reactive_updates_after_event();
      return;
    }

    let Some(click_target) = self.take_matching_click_press(position, button) else {
      self.apply_reactive_updates_after_event();
      return;
    };

    if button == MouseButton::Left
      && let ClickDispatchTarget::Node(click_target_id) = click_target
    {
      if self
        .click_tracker
        .pending_matches(now, position, button, click_target_id)
      {
        self.click_tracker.take_pending();
        self.dispatch_mouse_with_click_target(
          x,
          y,
          button,
          MouseEventKind::DoubleClick,
          modifiers,
          Some(click_target_id),
        );
        self.apply_reactive_updates_after_event();
        return;
      }
    }

    self.flush_pending_click();

    match click_target {
      ClickDispatchTarget::Node(click_target_id)
        if button == MouseButton::Left && self.click_target_has_dblclick_handler(click_target_id) =>
      {
        self
          .click_tracker
          .set_pending(now, position, button, modifiers, click_target_id);
        self.needs_redraw = true;
      }
      ClickDispatchTarget::Node(click_target_id) => {
        self.dispatch_mouse_with_click_target(x, y, button, MouseEventKind::Click, modifiers, Some(click_target_id));
        self.apply_reactive_updates_after_event();
      }
      ClickDispatchTarget::CurrentHit => {
        self.dispatch_mouse_with_click_target(x, y, button, MouseEventKind::Click, modifiers, None);
        self.apply_reactive_updates_after_event();
      }
    }
  }

  pub fn scroll(&mut self, x: f32, y: f32, delta_x: f32, delta_y: f32, phase: ScrollPhase) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Scroll);
    self.track_pointer(x, y);
    self.dispatch_scroll(x, y, delta_x, delta_y, phase);
    self.apply_reactive_updates_after_event();
  }

  pub fn key_down(&mut self, key: String, code: String, shift: bool, ctrl: bool, alt: bool) {
    self.key_down_with_meta(key, code, shift, ctrl, alt, false);
  }

  pub fn key_down_with_meta(&mut self, key: String, code: String, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Keyboard);
    self.rebuild_if_dirty();
    self.note_key_press_modality(&key, ctrl, alt, meta);
    // Keys never reach a node that was dropped since it took focus.
    self.blur_focus_in_dropped_child();
    // The input method takes the keys it reports as `Process` (on Windows,
    // the Enter that confirms a composition included): no handler or default
    // sees them. Other presses arrive as usual, marked while composing.
    if self.withhold_composition_key_down(&key, &code) {
      self.apply_reactive_updates_after_event();
      return;
    }
    let control = EventControl::new();
    let mut evt = KeyboardEvent {
      key: key.clone(),
      code: code.clone(),
      shift,
      ctrl,
      alt,
      meta,
      target_id: NodeId::UNASSIGNED,
      text_input_focused: self.text_input_focused(),
      composing: self.is_composing(),
      control,
    };
    if let Some(root) = &self.root {
      fire_keyboard_recursive(root, &mut evt);
    }
    // An Enter the input method passes on during a composition reaches
    // handlers marked `composing`, but no default acts on it (form submit,
    // button or select activation, newline, blur): it belongs to the
    // composition, as the web's `isComposing` Enter does.
    let composing_enter = evt.composing && matches!((key.as_str(), code.as_str()), ("Enter", _) | (_, "Enter"));
    if !evt.default_prevented() && !composing_enter {
      let click_modifiers = MouseModifiers { shift, ctrl, alt, meta };
      let select_key = select_menu::SelectKey {
        key: &key,
        code: &code,
        alt,
        ctrl,
        meta,
      };
      let handled = if self.dispatch_select_key(select_key) {
        true
      } else if matches!((key.as_str(), code.as_str()), ("Tab", _) | (_, "Tab")) {
        self.focus_tab(shift)
      } else if matches!(
        (key.as_str(), code.as_str()),
        ("Enter" | " ", _) | (_, "Enter" | "Space")
      ) && (self.activate_focused_button(click_modifiers)
        || matches!((key.as_str(), code.as_str()), ("Enter", _) | (_, "Enter"))
          && self.follows_focused_link(click_modifiers))
      {
        true
      } else if matches!((key.as_str(), code.as_str()), ("Enter", _) | (_, "Enter")) && {
        #[cfg(feature = "form")]
        {
          self.submit_focused_single_line_text_input()
        }
        #[cfg(not(feature = "form"))]
        {
          false
        }
      } {
        true
      } else {
        false
      };

      if handled {
        self.needs_redraw = true;
      } else if self.dismiss_top_overlay_on_escape(&key, &code) {
        self.needs_redraw = true;
      } else {
        let blurred_text_input = self.blur_focused_text_input_on_key(&key, &code);
        let cleared_text_selection = self.clear_selectable_text_selection_on_key(&key, &code);
        if blurred_text_input || cleared_text_selection {
          self.needs_redraw = true;
        } else if self.dispatch_text_input(&evt) {
          self.needs_redraw = true;
        } else {
          self.dispatch_selectable_text_clipboard(&key, &code, shift, ctrl, meta);
        }
      }
    }
    self.apply_reactive_updates_after_event();
  }

  pub fn key_up(&mut self, key: String, code: String, shift: bool, ctrl: bool, alt: bool) {
    self.key_up_with_meta(key, code, shift, ctrl, alt, false);
  }

  pub fn key_up_with_meta(&mut self, key: String, code: String, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    #[cfg(feature = "perf_profile")]
    let _input = self.profiling.context.input(crate::app::profiler::InputKind::Keyboard);
    self.rebuild_if_dirty();
    let composing = self.is_composition_key_up(&key, &code);
    let mut evt = KeyboardEvent {
      key,
      code,
      shift,
      ctrl,
      alt,
      meta,
      target_id: NodeId::UNASSIGNED,
      text_input_focused: self.text_input_focused(),
      composing,
      control: EventControl::new(),
    };
    let root = match &self.root {
      Some(r) => r,
      None => return,
    };
    fire_keyboard_up_recursive(root, &mut evt);
    self.apply_reactive_updates_after_event();
  }

  fn dismiss_top_overlay_on_escape(&mut self, key: &str, code: &str) -> bool {
    if !matches!(key, "Escape") && code != "Escape" {
      return false;
    }

    let Some(entry) = self
      .overlay_dismiss_entries
      .iter()
      .rev()
      .find(|entry| entry.dismiss_on_escape)
    else {
      return false;
    };

    entry.open.set(false);
    true
  }

  fn clear_selectable_text_selection_on_key(&mut self, key: &str, code: &str) -> bool {
    if !matches!(key, "Escape") && code != "Escape" {
      return false;
    }

    let Some(root) = &self.root else {
      return false;
    };
    self.selectable_text_anchor = None;
    clear_selectable_text_selections(root)
  }

  /// Tab / Shift+Tab: moves focus to the next stop in the current Tab scope
  /// (see [`tab_navigation`]) and scrolls it into view.
  fn focus_tab(&mut self, reverse: bool) -> bool {
    self.set_input_modality(focus_visible::InputModality::Keyboard);
    let Some(root) = &self.root else {
      return false;
    };
    let focused = self.focused_node.zip(self.focused_path.as_deref());
    match tab_navigation::tab_move(root, &self.dropped_nodes, focused, reverse) {
      tab_navigation::TabMove::Focus(target) => {
        self.focus_node(target);
        self.scroll_focused_into_view();
        true
      }
      tab_navigation::TabMove::Stay => true,
      tab_navigation::TabMove::Unhandled => false,
    }
  }

  /// Moves focus off a node its Row/Column dropped to make room
  /// (`ShrinkLimit::Drop`): a dropped node is not drawn, so it must not keep
  /// taking keys.
  fn blur_focus_in_dropped_child(&mut self) {
    let (Some(root), Some(path)) = (&self.root, &self.focused_path) else {
      return;
    };
    if tab_navigation::path_is_dropped(root, &self.dropped_nodes, path) {
      self.blur_focus();
    }
  }

  fn scroll_focused_into_view(&mut self) {
    let (Some(root), Some(layout), Some(focused)) = (&self.root, &self.last_layout, self.focused_node) else {
      return;
    };
    if scroll_into_view::scroll_into_view(root, layout, focused) {
      self.needs_redraw = true;
    }
  }

  #[cfg(feature = "form")]
  fn submit_focused_single_line_text_input(&mut self) -> bool {
    let Some(focused) = self.focused_node else {
      return false;
    };
    let is_single_line = {
      let Some(root) = &self.root else {
        return false;
      };
      let node = self
        .focused_path
        .as_deref()
        .and_then(|path| find_node_by_path(root, path))
        .or_else(|| find_node_by_id(root, focused));
      matches!(node.map(Node::node_kind), Some(NodeKind::TextInput { state, .. }) if state.overflow() != TextInputOverflow::Multiline)
    };
    is_single_line && self.submit_nearest_form_for_node_id(focused)
  }

  /// Enter on a focused paragraph link follows it.
  fn follows_focused_link(&mut self, modifiers: MouseModifiers) -> bool {
    #[cfg(feature = "markdown")]
    {
      self.activate_focused_link(modifiers)
    }
    #[cfg(not(feature = "markdown"))]
    {
      let _ = modifiers;
      false
    }
  }

  fn activate_focused_button(&mut self, modifiers: MouseModifiers) -> bool {
    let Some(focused) = self.focused_node else {
      return false;
    };
    let (_kind, click) = {
      let Some(root) = &self.root else {
        return false;
      };
      let node = self
        .focused_path
        .as_deref()
        .and_then(|path| find_node_by_path(root, path))
        .or_else(|| find_node_by_id(root, focused));
      let Some(node) = node else {
        return false;
      };
      let Some(kind) = node.button_kind_value() else {
        return false;
      };
      (kind, node.events.on_click.clone())
    };

    let event = modifiers.keyboard_click(focused);
    for handler in click {
      handler.call(&event);
    }

    #[cfg(feature = "form")]
    if _kind == ButtonKind::Submit {
      self.submit_nearest_form_for_node_id(focused);
    }
    true
  }

  #[cfg(feature = "form")]
  fn submit_nearest_form_for_node_id(&mut self, node_id: NodeId) -> bool {
    let submission = {
      let Some(root) = &self.root else {
        return false;
      };
      let Some(path) = find_path_by_id(root, node_id) else {
        return false;
      };
      nearest_form_submission(root, &path)
    };
    let Some((handler, data)) = submission else {
      return false;
    };
    handler(data);
    true
  }

  #[cfg(feature = "canvas")]
  pub fn set_canvas_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
    self.window.set_waker(std::sync::Arc::new(waker));
  }

  fn has_dirty_canvas(&self) -> bool {
    #[cfg(feature = "canvas")]
    {
      return self.canvas_registry.iter().any(crate::canvas::CanvasHandle::dirty);
    }
    #[cfg(not(feature = "canvas"))]
    {
      false
    }
  }

  pub fn needs_redraw(&self) -> bool {
    self.needs_redraw
      || !self.pending_scroll_into_view.is_empty()
      || self.has_dirty_canvas()
      || self.click_tracker.has_pending()
      || self
        .root_ctx
        .as_ref()
        .is_some_and(|ctx| ctx.any_dirty() || ctx.has_element_request())
      || self.root.as_ref().is_some_and(has_dirty_element_ref_recursive)
  }

  fn collect_pass_reasons(&self, theme_version: u64, caret_mode: CaretMode) -> PassReasons {
    let root = self.root.as_ref();
    let root_ctx = self.root_ctx.as_ref();

    PassReasons {
      redraw_requested: (self.needs_redraw && !self.scheduled_redraw_due) || self.has_dirty_canvas(),
      scheduled_redraw: self.scheduled_redraw_due,
      timer_active: root_ctx.is_some_and(Ctx::has_active_timers),
      future_active: root_ctx.is_some_and(Ctx::has_active_futures),
      timeline_active: self.has_active_timeline(),
      continuous_redraw_image: self.has_continuous_redraw_image(),
      perf_overlay: self.perf_overlay_enabled,
      pending_click: self.click_tracker.has_pending(),
      input_interaction: self.has_active_input_interaction(),
      text_input_caret: self.has_focused_blinking_text_input(caret_mode),
      theme_changed: self.last_theme_version != theme_version,
      component_dirty: root_ctx.is_some_and(|ctx| ctx.any_dirty() || ctx.has_element_request()),
      element_ref_dirty: root.is_some_and(has_dirty_element_ref_recursive),
      layout_dirty: root.is_some_and(has_pending_layout_dirty_recursive),
      ..PassReasons::default()
    }
  }

  pub fn cursor(&self) -> CursorIcon {
    self.cursor
  }

  pub fn clear_needs_redraw(&mut self) {
    self.needs_redraw = false;
    self.pending_pass_reasons = PassReasons::default();
  }

  fn flush_due_pending_click(&mut self, now: Instant) {
    if self.click_tracker.pending_is_due(now) {
      self.flush_pending_click();
    } else if self.click_tracker.has_pending() {
      self.needs_redraw = true;
    }
  }

  fn flush_pending_click(&mut self) {
    let Some(click) = self.click_tracker.take_pending() else {
      return;
    };

    // A pass or the next click runs it, outside the delivery of its input:
    // its handlers see the source of that input, so an MCP click stays
    // synthetic input.
    with_input_source(click.source, || {
      self.dispatch_mouse_with_click_target(
        click.position.0,
        click.position.1,
        click.button,
        MouseEventKind::Click,
        click.modifiers,
        Some(click.target_id),
      );
      // The click hovered what was under it where it was made; the hover
      // belongs where the pointer is now, if it is still in the window.
      if self.pointer.is_some() {
        self.refresh_hover_at_pointer();
      } else {
        self.clear_hover_path();
      }
      self.apply_reactive_updates_after_event();
    });
  }

  fn click_target_has_dblclick_handler(&mut self, target_id: NodeId) -> bool {
    self.rebuild_if_dirty();
    let Some(root) = self.root.as_ref() else {
      return false;
    };
    let Some(path) = find_path_by_id(root, target_id) else {
      return false;
    };

    (0..=path.len())
      .rev()
      .any(|depth| find_node_by_path(root, &path[..depth]).is_some_and(|node| !node.events.on_dblclick.is_empty()))
  }

  fn take_matching_click_press(&mut self, position: (f32, f32), button: MouseButton) -> Option<ClickDispatchTarget> {
    let Some(press) = self.click_press.take() else {
      return None;
    };

    if press.button != button {
      return None;
    }

    let release_target_ids = self.hit_target_ids_at(position.0, position.1);
    if press
      .transient_root
      .is_some_and(|target_id| !release_target_ids.contains(&target_id))
    {
      return None;
    }
    for (press_index, target_id) in press.target_ids.iter().copied().enumerate() {
      let Some(release_index) = release_target_ids
        .iter()
        .position(|release_id| *release_id == target_id)
      else {
        continue;
      };

      if press_index > 0
        && release_index > 0
        && distance_squared(press.position, position) <= DOUBLE_CLICK_DISTANCE * DOUBLE_CLICK_DISTANCE
      {
        return Some(ClickDispatchTarget::CurrentHit);
      }

      return Some(ClickDispatchTarget::Node(target_id));
    }

    if press.target_ids.is_empty()
      && release_target_ids.is_empty()
      && distance_squared(press.position, position) <= DOUBLE_CLICK_DISTANCE * DOUBLE_CLICK_DISTANCE
    {
      return Some(ClickDispatchTarget::CurrentHit);
    }

    None
  }

  fn hit_target_ids_at(&mut self, x: f32, y: f32) -> Vec<NodeId> {
    let scale = self.scale_factor();
    let lx = x / scale;
    let ly = y / scale;

    self.rebuild_if_dirty();

    let root = match &self.root {
      Some(r) => r,
      None => return Vec::new(),
    };
    let result = match &self.last_layout {
      Some(r) => r,
      None => return Vec::new(),
    };

    let mut hits = Vec::new();
    hit_test_tree(root, result, 0.0, 0.0, lx, ly, &mut hits);
    trim_hits_to_scrollbar(&mut hits, lx, ly);
    hits.into_iter().map(|(node, _)| node.node_id()).collect()
  }

  fn suppress_click(&mut self, position: (f32, f32), button: MouseButton) {
    self.suppressed_click = Some(SuppressedClick {
      time: Instant::now(),
      position,
      button,
    });
  }

  fn should_suppress_click(&mut self, now: Instant, position: (f32, f32), button: MouseButton) -> bool {
    let Some(suppressed) = self.suppressed_click else {
      return false;
    };

    self.suppressed_click = None;

    now.duration_since(suppressed.time) <= SUPPRESSED_CLICK_INTERVAL
      && suppressed.button == button
      && distance_squared(suppressed.position, position) <= SUPPRESSED_CLICK_DISTANCE * SUPPRESSED_CLICK_DISTANCE
  }

  fn dispatch_mouse(&mut self, x: f32, y: f32, button: MouseButton, kind: MouseEventKind, modifiers: MouseModifiers) {
    self.dispatch_mouse_with_click_target(x, y, button, kind, modifiers, None);
  }

  fn dispatch_mouse_with_click_target(
    &mut self,
    x: f32,
    y: f32,
    button: MouseButton,
    kind: MouseEventKind,
    modifiers: MouseModifiers,
    click_target: Option<NodeId>,
  ) {
    let mut evt = MouseEvent {
      x,
      y,
      button,
      kind,
      shift: modifiers.shift,
      ctrl: modifiers.ctrl,
      alt: modifiers.alt,
      meta: modifiers.meta,
      from_keyboard: false,
      target_id: NodeId::UNASSIGNED,
      control: EventControl::new(),
    };
    let scale = self.scale_factor();
    let lx = evt.x / scale;
    let ly = evt.y / scale;
    evt.x = lx;
    evt.y = ly;

    // Handle active scrollbar drag
    if let Some(ref drag) = self.dragging_scroll.clone() {
      match evt.kind {
        MouseEventKind::Move => {
          if drag.drags_thumb {
            self.scroll_with_scrollbar(drag, lx, ly, |state| {
              state.drag_to_axis(drag.axis, lx, ly, &state.style());
            });
          }
          return;
        }
        MouseEventKind::Up => {
          drag.state.end_drag();
          let target_id = self.rebound_scroll_drag_target(drag);
          self.dispatch_scroll_handlers_for_node(target_id, evt.x, evt.y, 0.0, 0.0, ScrollPhase::End);
          self.dragging_scroll = None;
          self.clear_active_path();
          self.suppress_click((evt.x, evt.y), button);
          self.needs_redraw = true;
          self.apply_reactive_updates_after_event();
          return;
        }
        _ => {}
      }
    }

    self.rebuild_if_dirty();

    if let Some(mut drag) = self.dragging_slider.clone() {
      if let Some(state) = self.current_slider_state(drag.target_id) {
        drag.state = state;
      } else if let Some(rebound) = self.current_slider_drag_by_binding(drag.binding_id) {
        drag = rebound;
      } else {
        self.dragging_slider = None;
        self.clear_active_path();
        self.needs_redraw = true;
        return;
      }
      self.dragging_slider = Some(drag.clone());
      match evt.kind {
        MouseEventKind::Move => {
          drag.update(lx);
          self.needs_redraw = true;
          self.apply_reactive_updates_after_event();
          return;
        }
        MouseEventKind::Up => {
          drag.update(lx);
          drag.finish();
          self.dragging_slider = None;
          self.clear_active_path();
          self.suppress_click((evt.x, evt.y), button);
          self.needs_redraw = true;
          self.apply_reactive_updates_after_event();
          return;
        }
        _ => {}
      }
    }

    if let Some(drag) = self.dragging_text_selection.clone() {
      match evt.kind {
        MouseEventKind::Move => {
          if let (Some(root), Some(result)) = (&self.root, &self.last_layout) {
            drag.update_with_tree(root, result, lx, ly);
          } else {
            drag.update(lx, ly);
          }
          self.needs_redraw = true;
          return;
        }
        MouseEventKind::Up => {
          if let (Some(root), Some(result)) = (&self.root, &self.last_layout) {
            drag.update_with_tree(root, result, lx, ly);
          } else {
            drag.update(lx, ly);
          }
          let has_selection = drag.has_selection(self.root.as_ref());
          self.dragging_text_selection = None;
          self.clear_active_path();
          if has_selection {
            self.suppress_click((evt.x, evt.y), button);
          }
          self.needs_redraw = true;
          return;
        }
        _ => {}
      }
    }

    if self.active_drag.is_some() {
      match evt.kind {
        MouseEventKind::Move => {
          let (event, handlers) = {
            let drag = self.active_drag.as_mut().unwrap();
            let event = drag.event(lx, ly, None);
            drag.last_x = lx;
            drag.last_y = ly;
            (event, drag.on_move.clone())
          };
          for handler in handlers {
            handler.call(&event);
          }
          self.needs_redraw = true;
          return;
        }
        MouseEventKind::Up => {
          let drag = self.active_drag.take().unwrap();
          if drag.button != button {
            self.active_drag = Some(drag);
            return;
          }
          let drop_target = self.drop_target_at(lx, ly);
          let drop_result = drop_target
            .as_ref()
            .map(|(target_id, _)| DropResult::Accepted { target_id: *target_id })
            .unwrap_or(DropResult::Missed);
          let drag_event = drag.event(lx, ly, Some(drop_result));
          if let Some((target_id, handlers)) = drop_target {
            let drop_event = DropEvent {
              x: lx,
              y: ly,
              start_x: drag.start_x,
              start_y: drag.start_y,
              total_delta_x: lx - drag.start_x,
              total_delta_y: ly - drag.start_y,
              button,
              source_id: drag.target_id,
              target_id,
              payload: drag.payload.clone(),
            };
            for handler in handlers {
              handler.call(&drop_event);
            }
          }
          let moved = distance_squared((drag.start_x, drag.start_y), (lx, ly))
            > SUPPRESSED_CLICK_DISTANCE * SUPPRESSED_CLICK_DISTANCE;
          for handler in drag.on_end {
            handler.call(&drag_event);
          }
          // A drag that actually moved is not a click — suppress the one the
          // release would otherwise synthesize on the drag source.
          if moved {
            self.suppress_click((evt.x, evt.y), button);
          }
          self.clear_active_path();
          self.needs_redraw = true;
          return;
        }
        _ => {}
      }
    }

    let overlay_dismiss_signals = if matches!(evt.kind, MouseEventKind::Down) && button == MouseButton::Left {
      self.overlay_dismiss_signals_at(lx, ly)
    } else {
      Vec::new()
    };

    let root = match &self.root {
      Some(r) => r,
      None => return,
    };
    let result = match &self.last_layout {
      Some(r) => r,
      None => {
        self.needs_redraw = true;
        return;
      }
    };

    let mut hits = Vec::new();
    hit_test_tree(root, result, 0.0, 0.0, lx, ly, &mut hits);
    trim_hits_to_scrollbar(&mut hits, lx, ly);
    // A scrollbar takes the pointer: nothing below it (text, inputs, sliders)
    // gets the built-in press or click behaviour.
    let on_scrollbar = hits
      .first()
      .is_some_and(|(node, _)| scrollbar_hit_at(node, lx, ly).is_some());
    if matches!(evt.kind, MouseEventKind::Click | MouseEventKind::DoubleClick)
      && let Some(click_target) = click_target
    {
      let Some(target_index) = hits.iter().position(|(node, _)| node.node_id() == click_target) else {
        return;
      };
      hits.drain(..target_index);
    }
    let mut pending_focus = None;
    let mut builtin_needs_redraw = false;
    let mut pending_slider_drag = None;
    let mut pending_text_selection_drag = None;
    let mut reset_text_input_caret_blink = false;
    let mut blur_on_press = false;
    let is_left_button = evt.button == MouseButton::Left;
    let is_left_click = matches!(evt.kind, MouseEventKind::Click) && is_left_button;
    let is_left_down = matches!(evt.kind, MouseEventKind::Down) && is_left_button;
    let click_outside_dispatch = if is_left_click {
      let event = MouseEvent {
        x: evt.x,
        y: evt.y,
        button: evt.button,
        kind: evt.kind,
        shift: evt.shift,
        ctrl: evt.ctrl,
        alt: evt.alt,
        meta: evt.meta,
        from_keyboard: false,
        target_id: hits
          .first()
          .map(|(node, _)| node.node_id())
          .unwrap_or(NodeId::UNASSIGNED),
        control: EventControl::new(),
      };
      let callbacks = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.click_outside_callbacks_at(lx, ly))
        .unwrap_or_default();
      Some((event, callbacks))
    } else {
      None
    };
    #[cfg(feature = "form")]
    let pending_submit = if is_left_click {
      hits
        .iter()
        .find(|(node, _)| node.button_kind_value() == Some(ButtonKind::Submit))
        .and_then(|(node, _)| find_path_by_id(root, node.node_id()))
        .and_then(|path| nearest_form_submission(root, &path))
    } else {
      None
    };

    'mouse_dispatch: for (node, _rect) in &hits {
      evt.target_id = node.node_id();
      match evt.kind {
        MouseEventKind::Click => {
          if evt.button == MouseButton::Left {
            for handler in &node.events.on_click {
              handler.call(&evt);
              if evt.immediate_propagation_stopped() {
                break 'mouse_dispatch;
              }
            }
          }
          for (button, handler) in &node.events.on_mouse_click {
            if *button == evt.button {
              handler.call(&evt);
              if evt.immediate_propagation_stopped() {
                break 'mouse_dispatch;
              }
            }
          }
        }
        MouseEventKind::DoubleClick => {
          if evt.button == MouseButton::Left {
            for handler in &node.events.on_dblclick {
              handler.call(&evt);
              if evt.immediate_propagation_stopped() {
                break 'mouse_dispatch;
              }
            }
          }
        }
        MouseEventKind::Down => {
          for handler in &node.events.on_mouse_down {
            handler.call(&evt);
            if evt.immediate_propagation_stopped() {
              break 'mouse_dispatch;
            }
          }
        }
        MouseEventKind::Up => {
          for handler in &node.events.on_mouse_up {
            handler.call(&evt);
            if evt.immediate_propagation_stopped() {
              break 'mouse_dispatch;
            }
          }
        }
        MouseEventKind::Move => {
          for handler in &node.events.on_mouse_move {
            handler.call(&evt);
            if evt.immediate_propagation_stopped() {
              break 'mouse_dispatch;
            }
          }
        }
      }
      if evt.propagation_stopped() {
        break;
      }
    }

    if !evt.default_prevented() && is_left_down {
      // Focus the select trigger on press so keyboard navigation works.
      if let Some((node, _)) = hits
        .iter()
        .find(|(node, _)| matches!(node.node_kind(), NodeKind::Select { .. }))
      {
        pending_focus = Some(FocusTarget {
          input_id: node.node_id(),
          event_id: node.node_id(),
        });
        builtin_needs_redraw = true;
      }

      // Dismiss open selects when the press lands outside their menu and
      // outside the open trigger (the trigger's own click toggles it).
      let on_menu = hits
        .iter()
        .any(|(node, _)| node.has_synthetic_role(SyntheticNodeRole::SelectMenu));
      if !on_menu
        && let Some((node, _)) = hits
          .iter()
          .find(|(node, _)| matches!(node.node_kind(), NodeKind::Select { .. }))
        && close_open_selects_except(root, Some(node.node_id()))
      {
        builtin_needs_redraw = true;
      }
      let on_open_trigger = hits
        .iter()
        .any(|(node, _)| matches!(node.node_kind(), NodeKind::Select { state } if state.is_open()));
      if !on_menu && !on_open_trigger && close_all_open_selects(root) {
        builtin_needs_redraw = true;
      }
      // Like HTML: a press where nothing can take focus blurs the focused
      // element. A press on a focusable element leaves focus to its own
      // focusing (on press or click); `focusable(false)` keeps focus.
      blur_on_press = !on_menu && press_blurs_focus(&hits);
    }

    if !evt.default_prevented() && (is_left_click || is_left_down) && !on_scrollbar {
      if is_left_down {
        if let Some((node, rect)) = hits
          .iter()
          .find(|(node, _)| matches!(node.node_kind(), NodeKind::TextInput { .. }))
        {
          if let NodeKind::TextInput { state, .. } = node.node_kind() {
            if !evt.shift && !evt.ctrl {
              self.selectable_text_anchor = None;
              clear_selectable_text_selections(root);
            }
            let padding = self
              .layout_engine
              .resolved_padding_for_size(node, Size::new(rect.width, rect.height));
            let content_height = (rect.height - padding.top - padding.bottom).max(0.0);
            let vertical_offset = padding.top + text_input_vertical_offset(state, content_height);
            state.begin_selection_at_point(
              rect.local_x - rect.x - padding.left,
              rect.local_y - rect.y - vertical_offset,
            );
            pending_text_selection_drag = Some(TextSelectionDrag {
              kind: TextSelectionDragKind::Input {
                target_id: node.node_id(),
                state: state.clone(),
              },
              x: rect.x + padding.left,
              y: rect.y + vertical_offset,
              transform: rect.transform,
            });
            pending_focus = Some(FocusTarget {
              input_id: node.node_id(),
              event_id: node.node_id(),
            });
            reset_text_input_caret_blink = true;
            builtin_needs_redraw = true;
          }
        }

        let pointer_on_slider = hits
          .iter()
          .any(|(node, _)| matches!(node.node_kind(), NodeKind::Slider { .. }));

        if pending_text_selection_drag.is_none()
          && !pointer_on_slider
          && let Some((node, rect)) = selectable_text_drag_start_endpoint(root, result, lx, ly)
        {
          if let Some((state, value)) = selectable_text_state_and_value(node) {
            let text_x = rect.local_x - rect.x;
            let text_y = rect.local_y - rect.y;
            let caret = state.caret_index_at_point(&value, text_x, text_y);
            let extended = evt
              .shift
              .then_some(self.selectable_text_anchor)
              .flatten()
              .and_then(|anchor| anchor.extend_to(root, node.node_id(), caret));
            let kind = match extended {
              Some(kind) => kind,
              None => {
                let preserve_existing = evt.ctrl;
                if !preserve_existing {
                  clear_selectable_text_selections_except(root, Some(node.node_id()));
                }
                state.set_selection_indices(&value, caret, caret);
                self.selectable_text_anchor = Some(SelectableTextAnchor {
                  node_id: node.node_id(),
                  index: caret,
                });
                TextSelectionDragKind::Text {
                  start_id: node.node_id(),
                  anchor: caret,
                  state,
                  value,
                  preserve_existing,
                }
              }
            };
            pending_text_selection_drag = Some(TextSelectionDrag {
              kind,
              x: rect.x,
              y: rect.y,
              transform: rect.transform,
            });
            builtin_needs_redraw = true;
          }
        }

        if pending_text_selection_drag.is_none()
          && !evt.shift
          && !evt.ctrl
          && !hits.iter().any(|(node, _)| is_selectable_text_node(node))
        {
          self.selectable_text_anchor = None;
          if clear_selectable_text_selections(root) {
            builtin_needs_redraw = true;
          }
        }

        if pending_text_selection_drag.is_none()
          && let Some((node, rect)) = hits
            .iter()
            .find(|(node, _)| matches!(node.node_kind(), NodeKind::Slider { .. }))
        {
          if let NodeKind::Slider { state } = node.node_kind() {
            let (track_rect, thumb_rect) = state.part_rects(
              rect.x,
              rect.y,
              rect.width,
              rect.height,
              true,
              DEFAULT_SLIDER_THUMB_MIN_SIZE,
            );
            let travel_width = track_rect.width - thumb_rect.width;
            let (drag_x, drag_width) = if travel_width > 0.0 {
              (track_rect.x + thumb_rect.width * 0.5, travel_width)
            } else {
              (track_rect.x, track_rect.width)
            };
            let drag = SliderDrag {
              target_id: node.node_id(),
              binding_id: state.value_id(),
              state: state.clone(),
              x: drag_x,
              width: drag_width,
              on_finish: node.events.on_blur.clone(),
            };
            drag.update(lx);
            pending_slider_drag = Some(drag);
            pending_focus = Some(FocusTarget {
              input_id: node.node_id(),
              event_id: node.node_id(),
            });
            builtin_needs_redraw = true;
          }
        }
      } else if let Some((node, rect)) = hits
        .iter()
        .find(|(node, _)| matches!(node.node_kind(), NodeKind::TextInput { .. }))
      {
        if let NodeKind::TextInput { state, .. } = node.node_kind() {
          let text_click_count = self
            .text_click_tracker
            .record(Instant::now(), (evt.x, evt.y), button, node.node_id());
          let padding = self
            .layout_engine
            .resolved_padding_for_size(node, Size::new(rect.width, rect.height));
          let content_height = (rect.height - padding.top - padding.bottom).max(0.0);
          let vertical_offset = padding.top + text_input_vertical_offset(state, content_height);
          let text_x = rect.local_x - rect.x - padding.left;
          let text_y = rect.local_y - rect.y - vertical_offset;
          match text_click_count {
            1 => state.set_caret_from_point(text_x, text_y),
            2 => state.select_word_at_point(text_x, text_y),
            _ => state.select_line_at_point(text_x, text_y),
          }
          reset_text_input_caret_blink = true;
          builtin_needs_redraw = true;
        }
      } else if let Some((node, rect)) = hits.iter().find(|(node, _)| is_selectable_text_node(node)) {
        if let Some((state, value)) = selectable_text_state_and_value(node) {
          let text_click_count = self
            .text_click_tracker
            .record(Instant::now(), (evt.x, evt.y), button, node.node_id());
          if !evt.shift && !evt.ctrl {
            clear_selectable_text_selections_except(root, Some(node.node_id()));
          }
          match text_click_count {
            1 => state.clear_selection_at_point(&value, rect.local_x - rect.x, rect.local_y - rect.y),
            2 => state.select_word_at_point(&value, rect.local_x - rect.x, rect.local_y - rect.y),
            _ => state.select_line_at_point(&value, rect.local_x - rect.x, rect.local_y - rect.y),
          }
          if let Some(index) = state.selection_anchor() {
            self.selectable_text_anchor = Some(SelectableTextAnchor {
              node_id: node.node_id(),
              index,
            });
          }
          builtin_needs_redraw = true;
        }
      }
      if let Some(target) = dispatch_builtin_pointer(&hits, lx, is_left_click) {
        pending_focus = Some(target);
        builtin_needs_redraw = true;
      }
      if hits.is_empty() && is_left_click {
        if let Some((node, rect)) = find_slider_by_y_recursive(root, result, 0.0, 0.0, ly) {
          if let NodeKind::Slider { state } = node.node_kind() {
            let (track_rect, thumb_rect) = state.part_rects(
              rect.x,
              rect.y,
              rect.width,
              rect.height,
              node.is_style_hovered(),
              DEFAULT_SLIDER_THUMB_MIN_SIZE,
            );
            let ratio = state.pointer_ratio(lx, track_rect, thumb_rect);
            state.set_from_ratio(ratio);
            state.clear_drag_ratio();
            pending_focus = Some(FocusTarget {
              input_id: node.node_id(),
              event_id: node.node_id(),
            });
            builtin_needs_redraw = true;
          }
        }
      }
    }

    // Scrollbars: hovering one shows its hover state, and a left press on one
    // takes the pointer.
    let mut scrollbar_press = None;
    if !evt.default_prevented() {
      for (node, _) in &hits {
        let Some((state, _)) = node.scroll_container() else {
          continue;
        };
        let hit = scrollbar_hit_at(node, lx, ly);
        if hit.is_some() != state.is_thumb_hovered() {
          state.set_thumb_hovered(hit.is_some());
          self.needs_redraw = true;
        }
        if is_left_down && let Some((axis, part)) = hit {
          let drag = ScrollDrag {
            target_id: node.node_id(),
            state: state.clone(),
            axis,
            drags_thumb: part == ScrollBarPart::Thumb,
          };
          scrollbar_press = Some((drag, part));
          break;
        }
      }
    }
    if let Some((drag, part)) = scrollbar_press {
      drop(hits);
      self.press_scrollbar(drag, part, lx, ly);
      return;
    }

    let pending_drag = if !evt.default_prevented()
      && matches!(evt.kind, MouseEventKind::Down)
      && pending_slider_drag.is_none()
      && pending_text_selection_drag.is_none()
    {
      hits
        .iter()
        .find(|(node, _)| {
          node.events.start_drag_buttons.contains_button(button)
            && (!node.events.on_drag_start.is_empty()
              || !node.events.on_drag_move.is_empty()
              || !node.events.on_drag_end.is_empty())
        })
        .map(|(node, _)| {
          let event = DragEvent {
            x: lx,
            y: ly,
            start_x: lx,
            start_y: ly,
            delta_x: 0.0,
            delta_y: 0.0,
            total_delta_x: 0.0,
            total_delta_y: 0.0,
            button,
            target_id: node.node_id(),
            drop_result: None,
          };
          (
            event,
            node.events.on_drag_start.clone(),
            ActiveDrag {
              target_id: node.node_id(),
              start_x: lx,
              start_y: ly,
              last_x: lx,
              last_y: ly,
              button,
              on_move: node.events.on_drag_move.clone(),
              on_end: node.events.on_drag_end.clone(),
              payload: node.drag_payload.clone(),
            },
          )
        })
    } else {
      None
    };

    #[cfg(feature = "form")]
    if !evt.default_prevented()
      && let Some((handler, data)) = pending_submit
    {
      handler(data);
      self.needs_redraw = true;
    }
    if !evt.default_prevented() {
      for open in overlay_dismiss_signals {
        open.set(false);
        self.needs_redraw = true;
      }
    }
    if !evt.default_prevented()
      && let Some((event, callbacks)) = click_outside_dispatch
    {
      for callback in callbacks {
        callback(&event);
      }
    }

    let current_ids: Vec<NodeId> = hits.iter().map(|(n, _)| n.node_id()).collect();

    if hover::move_hover(root, &self.hover_path, &hits) {
      self.cached_render_list = None;
      self.needs_redraw = true;
    }

    let clear_active_after_dispatch = matches!(evt.kind, MouseEventKind::Up | MouseEventKind::Click);

    for (node, _) in &hits {
      match evt.kind {
        MouseEventKind::Down => {
          set_node_active(node, true);
          self.cached_render_list = None;
          self.needs_redraw = true;
        }
        MouseEventKind::Up | MouseEventKind::Click => {
          set_node_active(node, false);
          self.cached_render_list = None;
          self.needs_redraw = true;
        }
        _ => {}
      }
    }

    self.hover_path = current_ids;
    self.cursor = hovered_cursor(hits.iter().map(|(node, _)| *node));
    if matches!(evt.kind, MouseEventKind::Down) {
      self.active_path = self.hover_path.clone();
    }
    if builtin_needs_redraw {
      self.needs_redraw = true;
    }
    drop(hits);
    if reset_text_input_caret_blink {
      self.reset_text_input_caret_blink();
    }
    if blur_on_press {
      self.blur_focus();
    }
    if clear_active_after_dispatch {
      self.clear_active_path();
    }
    if let Some(drag) = pending_slider_drag {
      self.dragging_slider = Some(drag);
    }
    if let Some(drag) = pending_text_selection_drag {
      self.dragging_text_selection = Some(drag);
    }
    if let Some((event, handler, drag)) = pending_drag {
      for handler in handler {
        handler.call(&event);
      }
      self.active_drag = Some(drag);
      self.needs_redraw = true;
    }
    if let Some(target) = pending_focus {
      self.focus_node(target);
    }
  }

  fn current_slider_state(&self, node_id: NodeId) -> Option<SliderState> {
    let node = find_node_by_id(self.root.as_ref()?, node_id)?;
    match node.node_kind() {
      NodeKind::Slider { state } => Some(state.clone()),
      _ => None,
    }
  }

  /// A left press on a scrollbar. On the thumb it starts a drag. On the track,
  /// which takes the pointer only where the bar owns its lane, it pages one
  /// viewport toward the press, the default on both Windows and macOS (without
  /// auto-repeat while the button is held). Either way the scrollbar holds the
  /// pointer until the release, which clicks nothing.
  fn press_scrollbar(&mut self, drag: ScrollDrag, part: ScrollBarPart, x: f32, y: f32) {
    self.dragging_scroll = Some(drag.clone());
    match part {
      ScrollBarPart::Thumb => drag.state.begin_drag_axis(drag.axis, x, y),
      ScrollBarPart::TrackBefore | ScrollBarPart::TrackAfter => {
        let forward = part == ScrollBarPart::TrackAfter;
        self.scroll_with_scrollbar(&drag, x, y, |state| state.page_axis(drag.axis, forward));
      }
    }
    self.needs_redraw = true;
  }

  /// Applies `scroll` to the scrollbar's state and, if the content moved,
  /// does what the wheel path does: dirties layout so the next pass re-lays
  /// (and `after_layout` hooks observe) the moved content, and runs the
  /// container's scroll and scroll-reach handlers.
  fn scroll_with_scrollbar(&mut self, drag: &ScrollDrag, x: f32, y: f32, scroll: impl FnOnce(&ScrollState)) {
    let previous_scroll_x = drag.state.scroll_x();
    let previous_scroll_y = drag.state.scroll_y();
    let previous_max_scroll_y = (drag.state.content_height() - drag.state.viewport_height()).max(0.0);
    scroll(&drag.state);
    let moved = drag.state.scroll_x() != previous_scroll_x || drag.state.scroll_y() != previous_scroll_y;
    if !moved {
      return;
    }
    let target_id = self.rebound_scroll_drag_target(drag);
    if let Some(root) = &self.root {
      mark_scroll_layout_dirty(root, target_id);
    }
    self.dispatch_scroll_handlers_for_node(target_id, x, y, 0.0, 0.0, ScrollPhase::Scroll);
    self.dispatch_scroll_reach_handlers_for_node(
      target_id,
      x,
      y,
      0.0,
      0.0,
      ScrollPhase::Scroll,
      previous_scroll_y,
      previous_max_scroll_y,
    );
    self.needs_redraw = true;
    self.apply_reactive_updates_after_event();
  }

  /// The scroll node id for an active scrollbar drag. The drag's own scroll
  /// handlers can rebuild the retained tree mid-drag (e.g. a virtualized list
  /// re-windowing), replacing the node captured on mouse-down — without
  /// rebinding, the remaining drag would still move the scroll state but no
  /// scroll handlers would fire, leaving the stale window on screen.
  fn rebound_scroll_drag_target(&mut self, drag: &ScrollDrag) -> NodeId {
    let Some(root) = &self.root else {
      return drag.target_id;
    };
    if find_node_by_id(root, drag.target_id).is_some() {
      return drag.target_id;
    }
    let Some(rebound) = find_scroll_node_id_by_state(root, &drag.state) else {
      tracing::warn!(
        target: "lurq::scroll",
        "scrollbar drag target lost and no node shares its scroll state; scroll handlers skipped"
      );
      return drag.target_id;
    };
    tracing::warn!(
      target: "lurq::scroll",
      "scrollbar drag target rebound after retained-tree rebuild ({:?} -> {:?})",
      drag.target_id,
      rebound
    );
    if let Some(active) = self.dragging_scroll.as_mut() {
      active.target_id = rebound;
    }
    rebound
  }

  fn current_slider_drag_by_binding(&self, binding_id: usize) -> Option<SliderDrag> {
    let root = self.root.as_ref()?;
    let result = self.last_layout.as_ref()?;
    let (node, rect) = find_slider_by_binding_recursive(root, result, 0.0, 0.0, binding_id)?;
    let NodeKind::Slider { state } = node.node_kind() else {
      return None;
    };
    let (track_rect, thumb_rect) = state.part_rects(
      rect.x,
      rect.y,
      rect.width,
      rect.height,
      true,
      DEFAULT_SLIDER_THUMB_MIN_SIZE,
    );
    let travel_width = track_rect.width - thumb_rect.width;
    let (drag_x, drag_width) = if travel_width > 0.0 {
      (track_rect.x + thumb_rect.width * 0.5, travel_width)
    } else {
      (track_rect.x, track_rect.width)
    };
    Some(SliderDrag {
      target_id: node.node_id(),
      binding_id: state.value_id(),
      state: state.clone(),
      x: drag_x,
      width: drag_width,
      on_finish: node.events.on_blur.clone(),
    })
  }

  fn blur_focused_text_input_on_key(&mut self, key: &str, code: &str) -> bool {
    let escape = matches!(key, "Escape") || code == "Escape";
    let enter = matches!(key, "Enter") || code == "Enter";
    if !escape && !enter {
      return false;
    }

    let Some(focused) = self.focused_node else {
      return false;
    };
    let Some(root) = self.root.as_ref() else {
      return false;
    };
    let focused_path = self.focused_path.clone();
    let node = focused_path
      .as_deref()
      .and_then(|path| find_node_by_path(root, path))
      .or_else(|| find_node_by_id(root, focused));
    let Some(NodeKind::TextInput { state, .. }) = node.map(Node::node_kind) else {
      return false;
    };

    if enter {
      if state.overflow() == TextInputOverflow::Multiline {
        return false;
      }
      #[cfg(feature = "form")]
      if focused_path
        .as_deref()
        .and_then(|path| nearest_form_path_for_path(root, path))
        .is_some()
      {
        return false;
      }
    }

    self.blur_focus();
    self.clear_active_path();
    true
  }

  fn dispatch_text_input(&mut self, evt: &KeyboardEvent) -> bool {
    let key = evt.key.as_str();
    let code = evt.code.as_str();
    let shift = evt.shift;
    let ctrl = evt.ctrl;
    let alt = evt.alt;
    let meta = evt.meta;
    let focused = match self.focused_node {
      Some(id) => id,
      None => return false,
    };
    let root = match &self.root {
      Some(root) => root,
      None => return false,
    };
    let node = self
      .focused_path
      .as_deref()
      .and_then(|path| find_node_by_path(root, path))
      .or_else(|| find_node_by_id(root, focused));
    let node = match node {
      Some(node) => node,
      None => return false,
    };

    let command = code;
    let logical = key;
    let shortcut = Self::platform_shortcut_modifier(ctrl, meta);
    let word_navigation = Self::platform_word_navigation_modifier(ctrl, alt);

    let node_kind = node.node_kind().clone();
    match node_kind {
      NodeKind::TextInput { state, .. } => {
        match (logical, command) {
          ("a" | "A", _) | (_, "KeyA") if shortcut => state.select_all(),
          ("c" | "C", _) | (_, "KeyC") if shortcut => {
            let Some(selected) = state.selected_text() else {
              return false;
            };
            return write_clipboard_text(&selected);
          }
          ("x" | "X", _) | (_, "KeyX") if shortcut => {
            let Some(selected) = state.selected_text() else {
              return false;
            };
            if !write_clipboard_text(&selected) {
              return false;
            }
            let _ = state.cut_selection(evt);
          }
          ("v" | "V", _) | (_, "KeyV") if shortcut => {
            if !self.paste_from_clipboard_shortcut(evt) {
              return false;
            }
          }
          ("Insert", _) | (_, "Insert") if ctrl => {
            let Some(selected) = state.selected_text() else {
              return false;
            };
            return write_clipboard_text(&selected);
          }
          ("Insert", _) | (_, "Insert") if shift => {
            if !self.paste_from_clipboard_shortcut(evt) {
              return false;
            }
          }
          ("Delete", _) | (_, "Delete") if shift => {
            let Some(selected) = state.selected_text() else {
              return false;
            };
            if !write_clipboard_text(&selected) {
              return false;
            }
            let _ = state.cut_selection(evt);
          }
          ("z" | "Z", _) | (_, "KeyZ") if shortcut && shift => {
            if !state.redo(evt) {
              return false;
            }
          }
          ("z" | "Z", _) | (_, "KeyZ") if shortcut => {
            if !state.undo(evt) {
              return false;
            }
          }
          ("y" | "Y", _) | (_, "KeyY") if shortcut => {
            if !state.redo(evt) {
              return false;
            }
          }
          ("Enter", _) | (_, "Enter") => {
            if !state.insert_newline(evt) {
              return false;
            }
          }
          ("Backspace", _) | (_, "Backspace") => {
            if !state.backspace(evt) {
              return false;
            }
          }
          ("Delete", _) | (_, "Delete") => {
            if !state.delete(evt) {
              return false;
            }
          }
          ("ArrowLeft", _) | (_, "ArrowLeft") if word_navigation => state.move_word_left(shift),
          ("ArrowRight", _) | (_, "ArrowRight") if word_navigation => state.move_word_right(shift),
          ("ArrowLeft", _) | (_, "ArrowLeft") => state.move_left(shift),
          ("ArrowRight", _) | (_, "ArrowRight") => state.move_right(shift),
          ("ArrowUp", _) | (_, "ArrowUp") => state.move_up(shift),
          ("ArrowDown", _) | (_, "ArrowDown") => state.move_down(shift),
          ("Home", _) | (_, "Home") => state.move_home(shift),
          ("End", _) | (_, "End") => state.move_end(shift),
          _ if !ctrl && !meta && key.chars().count() == 1 => {
            if !state.insert(key, evt) {
              return false;
            }
          }
          _ => return false,
        }
        self.reset_text_input_caret_blink();
      }
      NodeKind::Checkbox { state } => match (logical, command) {
        (" " | "Space", _) | (_, "Space") => state.toggle(),
        _ => return false,
      },
      NodeKind::Slider { state } => match (logical, command) {
        ("ArrowRight" | "ArrowUp", _) | (_, "ArrowRight" | "ArrowUp") => state.nudge(1),
        ("ArrowLeft" | "ArrowDown", _) | (_, "ArrowLeft" | "ArrowDown") => state.nudge(-1),
        _ => return false,
      },
      _ => return false,
    }

    true
  }

  fn dispatch_selectable_text_clipboard(&mut self, key: &str, code: &str, shift: bool, ctrl: bool, meta: bool) -> bool {
    let command = code;
    let logical = key;
    let shortcut = Self::platform_shortcut_modifier(ctrl, meta);
    if !matches!(
      (logical, command),
      ("c" | "C", _) | (_, "KeyC") if shortcut
    ) && !matches!(
      (logical, command),
      ("Insert", _) | (_, "Insert") if ctrl && !shift
    ) {
      return false;
    }

    let Some(root) = &self.root else {
      return false;
    };
    let Some(layout) = &self.last_layout else {
      return false;
    };
    let Some(selected) = selected_selectable_text(root, layout) else {
      return false;
    };
    write_clipboard_text(&selected)
  }

  fn platform_shortcut_modifier(ctrl: bool, meta: bool) -> bool {
    if cfg!(target_os = "macos") { meta } else { ctrl }
  }

  fn platform_word_navigation_modifier(ctrl: bool, alt: bool) -> bool {
    if cfg!(target_os = "macos") { alt } else { ctrl }
  }

  fn drop_target_at(&self, x: f32, y: f32) -> Option<(NodeId, Vec<DropCallback>)> {
    let root = self.root.as_ref()?;
    let result = self.last_layout.as_ref()?;
    let mut hits = Vec::new();
    hit_test_tree_all(root, result, 0.0, 0.0, x, y, &mut hits);
    hits
      .into_iter()
      .find(|(node, _)| !node.events.on_drop.is_empty())
      .map(|(node, _)| (node.node_id(), node.events.on_drop.clone()))
  }

  fn apply_focus_request(&mut self) {
    self.apply_focus_request_inner(false);
  }

  fn apply_focus_request_inner(&mut self, discard_missing: bool) {
    let Some(reference) = self.root_ctx.as_ref().and_then(Ctx::focus_request) else {
      return;
    };
    fn find(root: &Node, reference: &OwnedElementRef) -> Option<FocusTarget> {
      if root
        .element_ref
        .as_ref()
        .is_some_and(|current| current.same_handle(reference))
      {
        fn control(node: &Node) -> Option<NodeId> {
          if node.is_focusable() {
            Some(node.node_id())
          } else {
            node.children().iter().find_map(control)
          }
        }
        return control(root).map(|input_id| FocusTarget {
          input_id,
          event_id: root.node_id(),
        });
      }
      root.children().iter().find_map(|child| find(child, reference))
    }
    let target = self.root.as_ref().and_then(|root| find(root, &reference));
    if target.is_some() || discard_missing {
      self.root_ctx.as_ref().unwrap().take_focus_request();
    }
    if let Some(target) = target {
      self.focus_node(target);
      if self.focused_node == Some(target.input_id) {
        self.request_scroll_into_view(target.input_id);
      }
    }
  }

  fn focus_node(&mut self, target: FocusTarget) {
    let Some(root) = self.root.as_ref() else {
      return;
    };
    let Some(input_path) = find_path_by_id(root, target.input_id) else {
      return;
    };
    if find_node_by_path(root, &input_path).is_some_and(Node::is_focus_disabled) {
      return;
    }
    // A node its Row/Column dropped (`ShrinkLimit::Drop`) is not drawn and
    // takes no focus, like a disabled one.
    if tab_navigation::path_is_dropped(root, &self.dropped_nodes, &input_path) {
      return;
    }
    let event_path = find_path_by_id(root, target.event_id).unwrap_or_else(|| input_path.clone());
    if self.focused_node == Some(target.input_id) && self.focused_event_node == Some(target.event_id) {
      return;
    }
    let memory = FocusMemory::capture(root, target.input_id, target.event_id);
    let focus = find_node_by_path(root, &event_path)
      .map(|node| node.events.on_focus.clone())
      .unwrap_or_default();
    self.clear_focus();
    self.focused_node = Some(target.input_id);
    self.focused_event_node = Some(target.event_id);
    self.focused_path = Some(input_path);
    self.focused_event_path = Some(event_path);
    self.focus_memory = memory;
    self.refresh_focus_ids();
    for handler in focus {
      handler.call();
    }
    self.needs_redraw = true;
    self.reset_text_input_caret_blink();
  }

  fn dispatch_scroll(&mut self, x: f32, y: f32, delta_x: f32, delta_y: f32, phase: ScrollPhase) {
    let mut evt = ScrollEvent {
      x,
      y,
      delta_x,
      delta_y,
      phase,
      target_id: NodeId::UNASSIGNED,
      control: EventControl::new(),
    };
    if self.root.is_none() || self.last_layout.is_none() {
      return;
    }

    let scale = self.scale_factor();
    let lx = evt.x / scale;
    let ly = evt.y / scale;
    evt.x = lx;
    evt.y = ly;

    let gesture_dy = self.wheel_gesture_vertical(lx, ly, evt.delta_y);
    let root = match &self.root {
      Some(r) => r,
      None => return,
    };
    let result = match &self.last_layout {
      Some(r) => r,
      None => return,
    };
    let mut hits = Vec::new();
    hit_test_tree(root, result, 0.0, 0.0, lx, ly, &mut hits);
    let scroll_delta_can_be_consumed =
      scroll_delta_can_be_consumed_by_hits(&hits, evt.delta_x, evt.delta_y, gesture_dy);
    // When the delta targets a scroll container that cannot move (edge-clamped
    // or non-overflowing), suppress scroll events entirely; handlers on hits
    // without a matching scroll container still receive wheel events.
    if !scroll_delta_can_be_consumed
      && (scroll_delta_targets_scroll_container(&hits, evt.delta_x, evt.delta_y, gesture_dy)
        || !scroll_handlers_present_for_phase(&hits, evt.phase))
    {
      return;
    }

    // Fire user handlers before the default auto-scroll so prevent_default
    // can block native scroll behavior.
    for (node, _) in &hits {
      evt.target_id = node.node_id();
      match evt.phase {
        ScrollPhase::Start => {
          for handler in &node.events.on_scroll_start {
            handler.call(&evt);
            if evt.immediate_propagation_stopped() {
              break;
            }
          }
        }
        ScrollPhase::Scroll => {
          for handler in &node.events.on_scroll {
            handler.call(&evt);
            if evt.immediate_propagation_stopped() {
              break;
            }
          }
        }
        ScrollPhase::End => {
          for handler in &node.events.on_scroll_end {
            handler.call(&evt);
            if evt.immediate_propagation_stopped() {
              break;
            }
          }
        }
      }
      if evt.immediate_propagation_stopped() || evt.propagation_stopped() {
        break;
      }
    }

    if evt.default_prevented() || !scroll_delta_can_be_consumed {
      return;
    }

    // Auto-scroll from the innermost scroll container outward, preserving
    // any delta an edge-clamped child could not consume.
    let mut remaining_dx = -evt.delta_x;
    let mut remaining_dy = -evt.delta_y;
    let mut pending_scroll_reach = Vec::new();
    for (node, _) in &hits {
      if let Some((state, direction)) = node.scroll_container() {
        let (dx, dy) = scroll_container_deltas(direction, remaining_dx, remaining_dy, gesture_dy);
        if dx == 0.0 && dy == 0.0 {
          continue;
        }

        let previous_scroll_y = state.scroll_y();
        let previous_max_scroll_y = (state.content_height() - state.viewport_height()).max(0.0);
        let (overflow_dx, overflow_dy) = state.scroll_by_with_overflow(dx, dy);
        if overflow_dx != dx || overflow_dy != dy {
          node.layout_cache.mark_local_dirty();
          for (hit_node, _) in &hits {
            hit_node.layout_cache.mark_descendant_dirty();
          }
          self.needs_redraw = true;
        }
        pending_scroll_reach.push((
          node.node_id(),
          evt.x,
          evt.y,
          evt.delta_x,
          evt.delta_y,
          evt.phase,
          previous_scroll_y,
          previous_max_scroll_y,
        ));
        if dx != 0.0 {
          remaining_dx = overflow_dx;
        }
        if dy != 0.0 {
          remaining_dy = overflow_dy;
        }
        if remaining_dx == 0.0 && remaining_dy == 0.0 {
          break;
        }
      }
    }
    for (target_id, x, y, delta_x, delta_y, phase, previous_scroll_y, previous_max_scroll_y) in pending_scroll_reach {
      self.dispatch_scroll_reach_handlers_for_node(
        target_id,
        x,
        y,
        delta_x,
        delta_y,
        phase,
        previous_scroll_y,
        previous_max_scroll_y,
      );
    }
  }

  fn dispatch_scroll_handlers_for_node(
    &mut self,
    target_id: NodeId,
    x: f32,
    y: f32,
    delta_x: f32,
    delta_y: f32,
    phase: ScrollPhase,
  ) {
    let Some(root) = &self.root else {
      return;
    };
    let Some(node) = find_node_by_id(root, target_id) else {
      return;
    };
    let evt = ScrollEvent {
      x,
      y,
      delta_x,
      delta_y,
      phase,
      target_id,
      control: EventControl::new(),
    };

    match evt.phase {
      ScrollPhase::Start => {
        for handler in &node.events.on_scroll_start {
          handler.call(&evt);
          if evt.immediate_propagation_stopped() {
            break;
          }
        }
      }
      ScrollPhase::Scroll => {
        for handler in &node.events.on_scroll {
          handler.call(&evt);
          if evt.immediate_propagation_stopped() {
            break;
          }
        }
      }
      ScrollPhase::End => {
        for handler in &node.events.on_scroll_end {
          handler.call(&evt);
          if evt.immediate_propagation_stopped() {
            break;
          }
        }
      }
    }
  }

  fn dispatch_scroll_reach_handlers_for_node(
    &self,
    target_id: NodeId,
    x: f32,
    y: f32,
    delta_x: f32,
    delta_y: f32,
    phase: ScrollPhase,
    previous_scroll_y: f32,
    previous_max_scroll_y: f32,
  ) {
    let Some(root) = &self.root else {
      return;
    };
    let Some(node) = find_node_by_id(root, target_id) else {
      return;
    };
    let Some((state, _)) = node.scroll_container() else {
      return;
    };

    let next_scroll_y = state.scroll_y();
    let next_max_scroll_y = (state.content_height() - state.viewport_height()).max(0.0);
    let reached_top = previous_scroll_y > 2.0 && next_scroll_y <= 2.0;
    let reached_bottom = next_max_scroll_y > 0.0
      && previous_scroll_y < previous_max_scroll_y - 2.0
      && next_scroll_y >= next_max_scroll_y - 2.0;

    if !reached_top && !reached_bottom {
      return;
    }

    let evt = ScrollEvent {
      x,
      y,
      delta_x,
      delta_y,
      phase,
      target_id,
      control: EventControl::new(),
    };

    if reached_top {
      for handler in &node.events.on_scroll_reach_top {
        handler.call(&evt);
        if evt.immediate_propagation_stopped() {
          break;
        }
      }
    }

    if reached_bottom {
      for handler in &node.events.on_scroll_reach_bottom {
        handler.call(&evt);
        if evt.immediate_propagation_stopped() {
          break;
        }
      }
    }
  }

  pub fn resolve_quads(&self, result: &LayoutResult) -> Vec<Quad> {
    match &self.root {
      Some(root) => self.layout_engine.resolve_quads(root, result),
      None => vec![],
    }
  }

  pub fn resolve_quads_with_viewport(&self, result: &LayoutResult, viewport: ClipRect) -> Vec<Quad> {
    match &self.root {
      Some(root) => self.layout_engine.resolve_quads_with_viewport(root, result, viewport),
      None => vec![],
    }
  }

  /// What the last layout paints, in paint order: one [`Quad`] per fill,
  /// border, shadow, text run, image and caret, at absolute logical positions
  /// and clipped to the viewport the way a pass draws them. Empty before the
  /// first pass.
  ///
  /// It needs no window or render engine, so a test can run
  /// [`Tree::pass_headless`] and check what an element paints that the
  /// element tree does not show, such as a control's own fill:
  ///
  /// ```
  /// use lurq::{
  ///   app::{App, Tree},
  ///   components::Rect,
  ///   layout::quad::QuadContent,
  ///   node::color::Color,
  /// };
  ///
  /// let mut tree = Tree::new();
  /// tree.set_root(Rect::new(100.0, 50.0).background(Color::from_hex("#22c55e")));
  /// tree.pass_headless(&mut App::new());
  /// let fills: Vec<Color> = tree
  ///   .painted_quads()
  ///   .iter()
  ///   .filter_map(|quad| match quad.content {
  ///     QuadContent::Rect { color, .. } => Some(color),
  ///     _ => None,
  ///   })
  ///   .collect();
  /// assert_eq!(fills, [Color::from_hex("#22c55e")]);
  /// ```
  pub fn painted_quads(&self) -> Vec<Quad> {
    let Some(result) = &self.last_layout else {
      return Vec::new();
    };
    let scale = self.scale_factor();
    let viewport = ClipRect {
      x: 0.0,
      y: 0.0,
      width: self.viewport_physical.width / scale,
      height: self.viewport_physical.height / scale,
      active: true,
      border_radius: None,
    };
    self.resolve_quads_with_viewport(result, viewport)
  }

  #[doc(hidden)]
  pub fn set_layout_constraints_override(&mut self, constraints: Option<Constraints>) {
    self.layout_constraints_override = constraints;
    self.last_layout = None;
    self.cached_render_list = None;
  }

  #[doc(hidden)]
  pub fn last_layout(&self) -> Option<&LayoutResult> {
    self.last_layout.as_ref()
  }

  /// Benchmarks only: runs the layout engine over the root under the
  /// viewport's tight constraints, without the rest of a pass (overlays,
  /// hit-testing, focus, quads). `force_dirty` lays every node out again.
  #[doc(hidden)]
  pub fn bench_layout_only(&mut self, app: &mut App, force_dirty: bool) -> Option<LayoutResult> {
    let root = self.root.as_ref()?;
    let constraints = self
      .layout_constraints_override
      .unwrap_or_else(|| Constraints::tight(self.viewport_logical()));
    let theme = app.theme();
    self.layout_engine.set_shadows(theme.shared_shadows());
    Some(self.layout_engine.compute(
      &mut app.shared.glyph_engine.lock(),
      root,
      constraints,
      theme.palette().clone(),
      theme.border_sizes().clone(),
      theme.spacing().clone(),
      theme.radii().clone(),
      *theme.caret(),
      theme.scrollbar().clone(),
      theme.typography().clone(),
      force_dirty,
    ))
  }

  /// The text selected in the selectable texts (`Text::selectable`,
  /// `MarkdownProps::selectable`) as of the last pass, as Ctrl+C copies it:
  /// the texts drawn side by side on one line, in x order; texts stacked
  /// below each other one line each; what a Markdown paragraph draws as
  /// presentation (a link's isolate marks and trailing runs) left out. `None`
  /// when nothing is selected. A text input keeps its own selection
  /// (`TextInputHandle::selected_text`); this does not read it.
  pub fn selected_text(&self) -> Option<String> {
    selected_selectable_text(self.root.as_ref()?, self.last_layout.as_ref()?)
  }

  #[cfg(feature = "mcp")]
  pub(crate) fn layout_is_stale(&self) -> bool {
    self.tree_rebuilt_since_layout
  }

  fn can_reuse_cached_render_list(&self, reasons: PassReasons) -> bool {
    if self.root_ctx.as_ref().is_some_and(Ctx::any_dirty) {
      return false;
    }
    if !reasons.scheduled_redraw && !reasons.timeline_active && !reasons.continuous_redraw_image {
      return false;
    }
    if reasons.redraw_requested
      || reasons.future_completed
      || reasons.pending_click
      || reasons.input_interaction
      || reasons.text_input_caret
      || reasons.theme_changed
      || reasons.component_dirty
      || reasons.element_ref_dirty
      || reasons.layout_dirty
    {
      return false;
    }
    if self.perf_overlay_enabled {
      return false;
    }
    #[cfg(feature = "devtools")]
    if self.devtools_is_open() || self.debug_overlay_node_path.is_some() {
      return false;
    }
    self.root.as_ref().is_some_and(|root| !root.has_render_dirty())
  }

  #[cfg(feature = "raster")]
  fn should_store_cached_render_list(&self) -> bool {
    if self.perf_overlay_enabled {
      return false;
    }
    #[cfg(feature = "devtools")]
    if self.devtools_is_open() || self.debug_overlay_node_path.is_some() {
      return false;
    }
    true
  }

  fn try_render_cached_render_list(
    &mut self,
    app: &mut App,
    clear_color: Color,
    window: WindowHandle<'_>,
    display: DisplayHandle<'_>,
    reasons: PassReasons,
  ) -> Option<bool> {
    if self.root_ctx.as_ref().is_some_and(Ctx::has_element_request) || !self.pending_scroll_into_view.is_empty() {
      return None;
    }
    if !self.can_reuse_cached_render_list(reasons) {
      log_render_list_cache_miss_timeline("gate", reasons);
      return None;
    }
    let Some(mut cached) = self.cached_render_list.take() else {
      log_render_list_cache_miss_timeline("empty", reasons);
      return None;
    };

    self.needs_redraw = false;
    cached.list.clear_color = clear_color;
    #[cfg(feature = "raster")]
    self.refresh_cached_image_frames(&mut cached, Instant::now());
    cached.list.atlas = app.shared.glyph_engine.lock().atlas();
    let rect_count = cached.list.rects.len();
    let glyph_count = cached.list.glyphs.len();
    #[cfg(feature = "raster")]
    let image_count = cached.list.images.len();
    #[cfg(not(feature = "raster"))]
    let image_count = 0;

    let gpu_wall_start = Instant::now();
    let _gpu_start = profile_scope!();
    let Some(render_engine) = &mut self.render_engine else {
      self.cached_render_list = Some(cached);
      return Some(false);
    };
    #[cfg(feature = "perf_profile")]
    let _render_phase = self.profiling.context.phase(crate::app::profiler::Phase::Render);
    #[cfg(feature = "canvas")]
    {
      let _canvas_start = profile_scope!();
      #[cfg(feature = "perf_profile")]
      let _canvas_phase = self
        .profiling
        .context
        .phase(crate::app::profiler::Phase::CanvasPreparation);
      render_engine.prepare_canvases(&self.canvas_registry);
      #[cfg(feature = "perf_profile")]
      self.profiling.canvas_preparation(profile_elapsed!(_canvas_start));
    }
    // No capture is taken from a cached list: one an engine still holds from
    // a frame it could not render is failed here rather than taken from this
    // frame, whose content (and sensitive text) it was not prepared for.
    let rendered = {
      #[cfg(feature = "screenshot")]
      {
        render_engine.render_with_capture(&cached.list, window, display, None)
      }
      #[cfg(not(feature = "screenshot"))]
      {
        render_engine.render(&cached.list, window, display)
      }
    };
    if !rendered {
      self.cached_render_list = Some(cached);
      return Some(false);
    }
    let gpu_wall_dur = gpu_wall_start.elapsed();
    let _gpu_dur = profile_elapsed!(_gpu_start);
    #[cfg(feature = "perf_profile")]
    drop(_render_phase);
    let renderer_wants_redraw = render_engine.wants_redraw();

    profile_if! {
      let render_profile = render_engine.last_profile();
      self.last_profile = FrameProfile {
        gpu_submit: _gpu_dur,
        render: render_profile.unwrap_or_default(),
        render_profile_available: render_profile.is_some(),
        total: _gpu_dur,
        rect_count: cached.list.rects.len(),
        glyph_count: cached.list.glyphs.len(),
        memory: self.cached_memory_profile(app),
        ..FrameProfile::default()
      };
      crate::app::profiler::notify_frame_profile(&self.last_profile);
    }

    if renderer_wants_redraw {
      self.needs_redraw = true;
    }

    self.cached_render_list = Some(cached);
    self.scheduled_redraw_due = false;
    self.frame_count += 1;
    log_render_list_cache_hit_timeline(reasons, gpu_wall_dur, rect_count, glyph_count, image_count);
    #[cfg(feature = "devtools")]
    self.sync_devtools();
    Some(true)
  }

  #[cfg(feature = "raster")]
  fn refresh_cached_image_frames(&mut self, cached: &mut CachedRenderList, now: Instant) {
    for (image, source) in cached.list.images.iter_mut().zip(cached.image_sources.iter()) {
      let Some(source) = source else {
        continue;
      };
      let frame = source.frame_at(now);
      if source.requires_continuous_redraw() {
        log_draw_image_timeline_sampled(
          &IMAGE_REFRESH_TIMELINE_LAST_INFO_MS,
          "refresh",
          source.id(),
          frame.version,
          frame.frame_index,
        );
      }
      if let Some(next_frame_at) = frame.next_frame_at
        && !source.requires_continuous_redraw()
      {
        self.request_redraw_at(next_frame_at);
      }
      image.frame_index = frame.frame_index;
      image.version = frame.version;
      image.data = frame.data;
      image.animation_frames = frame.animation_frames;
      image.native = frame.native;
      image.image_width = frame.width;
      image.image_height = frame.height;
      image.image_format = frame.format;
    }
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn draw_debug_overlay_over_node(&mut self, path: Vec<usize>) -> bool {
    self.set_debug_overlay_node_path(Some(path))
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn clear_debug_overlay(&mut self) -> bool {
    self.set_debug_overlay_node_path(None)
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  pub(crate) fn debug_overlay_node_at(&mut self, x: f32, y: f32) -> Option<Vec<usize>> {
    self.rebuild_if_dirty();
    let root = self.root.as_ref()?;
    let result = self.last_layout.as_ref()?;
    let scale = self.scale_factor();
    let mut hits = Vec::new();
    hit_test_tree(root, result, 0.0, 0.0, x / scale, y / scale, &mut hits);
    let node = hits.first()?.0;
    find_path_by_id(root, node.node_id())
  }

  #[cfg(feature = "devtools")]
  #[cfg_attr(not(feature = "winit"), allow(dead_code))]
  fn set_debug_overlay_node_path(&mut self, path: Option<Vec<usize>>) -> bool {
    if self.debug_overlay_node_path == path {
      return false;
    }
    self.debug_overlay_node_path = path;
    self.needs_redraw = true;
    true
  }

  fn rebuild_if_dirty(&mut self) {
    const MAX_REACTIVE_FLUSH_PASSES: usize = 8;

    for pass in 0..MAX_REACTIVE_FLUSH_PASSES {
      if self.root_ctx.as_ref().is_some_and(Ctx::is_dirty) {
        self.rebuild();
      } else if self.root_ctx.as_ref().is_some_and(Ctx::any_dirty) {
        self.refresh_dirty_subtrees();
      } else if pass > 0 {
        tracing::debug!(
          target: "lurq::reactivity",
          "[lurq:reactivity] dirty flush settled passes={}",
          pass
        );
        return;
      } else {
        return;
      }
    }

    if self.root_ctx.as_ref().is_some_and(Ctx::any_dirty) {
      tracing::debug!(
        target: "lurq::reactivity",
        "[lurq:reactivity] dirty flush capped passes={} dirty_after_cap=true",
        MAX_REACTIVE_FLUSH_PASSES
      );
      self.needs_redraw = true;
    }
  }

  fn apply_reactive_updates_after_event(&mut self) {
    if self.root_ctx.as_ref().is_some_and(Ctx::any_dirty) {
      self.needs_redraw = true;
      self.rebuild_if_dirty();
    }
    self.apply_focus_request();
  }

  fn reset_text_input_caret_blink(&mut self) {
    self.text_input_caret_blink_started_at = Instant::now();
    self.set_text_input_caret_visible(true);
  }

  fn update_text_input_caret_blink(&mut self, now: Instant, theme_caret_mode: CaretMode) {
    if !self.has_focused_blinking_text_input(theme_caret_mode) {
      self.set_text_input_caret_visible(true);
      return;
    }

    let interval_ms = TEXT_INPUT_CARET_BLINK_INTERVAL.as_millis().max(1);
    let phase = now.duration_since(self.text_input_caret_blink_started_at).as_millis() / interval_ms;
    self.set_text_input_caret_visible(phase.is_multiple_of(2));
    // Nothing else redraws an idle window: without a redraw at the next
    // toggle the shell never repainted the caret, so it never blinked.
    let next_toggle = self.text_input_caret_blink_started_at + TEXT_INPUT_CARET_BLINK_INTERVAL * (phase as u32 + 1);
    self.request_redraw_at(next_toggle);
  }

  fn set_text_input_caret_visible(&mut self, visible: bool) {
    if self.text_input_caret_visible != visible {
      self.text_input_caret_visible = visible;
      self.needs_redraw = true;
    }
    self.layout_engine.set_text_input_caret_visible(visible);
  }

  /// Whether keyboard focus currently sits on a text input — stamped onto
  /// [`KeyboardEvent`]s so global-shortcut handlers can step aside while the
  /// user is typing.
  fn text_input_focused(&self) -> bool {
    let Some(root) = self.root.as_ref() else {
      return false;
    };
    self
      .focused_path
      .as_deref()
      .and_then(|path| find_node_by_path(root, path))
      .or_else(|| self.focused_node.and_then(|id| find_node_by_id(root, id)))
      .is_some_and(|node| matches!(node.node_kind(), NodeKind::TextInput { .. }))
  }

  fn has_focused_blinking_text_input(&self, theme_caret_mode: CaretMode) -> bool {
    let Some(focused) = self.focused_node else {
      return false;
    };
    let Some(root) = self.root.as_ref() else {
      return false;
    };
    self
      .focused_path
      .as_deref()
      .and_then(|path| find_node_by_path(root, path))
      .or_else(|| find_node_by_id(root, focused))
      .is_some_and(|node| {
        matches!(node.node_kind(), NodeKind::TextInput { .. })
          && node.caret_mode_value().unwrap_or(theme_caret_mode) == CaretMode::Blinking
      })
  }

  fn update_perf_overlay_stats(&mut self) {
    if !self.perf_overlay_enabled {
      return;
    }

    let frame_count = self.frame_count;
    if frame_count > self.perf_overlay_last_seen_frame {
      self.perf_overlay_frames_since_sample += frame_count - self.perf_overlay_last_seen_frame;
      self.perf_overlay_last_seen_frame = frame_count;
    }

    let now = Instant::now();
    let elapsed = now.duration_since(self.perf_overlay_last_sample);
    if elapsed >= PERF_SAMPLE_INTERVAL {
      self.perf_overlay_stats = PerfMeterStats {
        fps: (self.perf_overlay_frames_since_sample as f32 / elapsed.as_secs_f32()).round() as u32,
        #[cfg(feature = "perf_profile")]
        total_ms: ms(self.last_profile.total),
        #[cfg(feature = "perf_profile")]
        layout_ms: ms(self.last_profile.layout),
        #[cfg(feature = "perf_profile")]
        quad_resolve_ms: ms(self.last_profile.quad_resolve),
        #[cfg(feature = "perf_profile")]
        glyph_ms: ms(self.last_profile.glyph_rasterize),
        #[cfg(feature = "perf_profile")]
        render_cpu_ms: ms(self.last_profile.render.active_total()),
        #[cfg(feature = "perf_profile")]
        render_acquire_ms: ms(self.last_profile.render.acquire),
        #[cfg(feature = "perf_profile")]
        render_upload_ms: ms(self.last_profile.render.upload_total()),
        #[cfg(feature = "perf_profile")]
        render_encode_ms: ms(self.last_profile.render.encode),
        #[cfg(feature = "perf_profile")]
        render_submit_ms: ms(self.last_profile.render.submit),
        #[cfg(feature = "perf_profile")]
        render_present_ms: ms(self.last_profile.render.present),
        #[cfg(feature = "perf_profile")]
        quad_count: self.last_profile.quad_count,
        #[cfg(feature = "perf_profile")]
        glyph_count: self.last_profile.glyph_count,
        ..PerfMeterStats::default()
      };
      self.perf_overlay_frames_since_sample = 0;
      self.perf_overlay_last_sample = now;
    }
  }

  /// Refreshes dirty subtrees while a refresh replaces something, up to as
  /// many times as a pass flushes: a refreshed component may mark others (a
  /// provider's readers, through `propagate_contexts`), which a single
  /// refresh leaves for the next. A lookup between passes sees the settled
  /// subtrees; a dirty root itself waits for the pass, as it did.
  pub(crate) fn refresh_dirty_subtrees_settled(&mut self) {
    const MAX_REFRESH_PASSES: usize = 8;
    for _ in 0..MAX_REFRESH_PASSES {
      if !self.root_ctx.as_ref().is_some_and(Ctx::any_dirty) || !self.refresh_dirty_subtrees() {
        return;
      }
    }
  }

  /// Returns whether a subtree was replaced.
  pub(crate) fn refresh_dirty_subtrees(&mut self) -> bool {
    #[cfg(feature = "perf_profile")]
    let mut _profile_update = self
      .profiling
      .context
      .update(crate::app::profiler::UiUpdateKind::SubtreeRefresh);
    let (replacements, dirty_after_refresh) = match &mut self.root_ctx {
      Some(ctx) => {
        let replacements = ctx.refresh_dirty_subtrees();
        let dirty_after_refresh = ctx.any_dirty();
        (replacements, dirty_after_refresh)
      }
      None => return false,
    };

    if replacements.is_empty() {
      if dirty_after_refresh {
        tracing::debug!(
          target: "lurq::reactivity",
          "[lurq:reactivity] dirty refresh left pending work without replacements"
        );
        self.needs_redraw = true;
      }
      return false;
    }
    #[cfg(feature = "perf_profile")]
    _profile_update.commit();

    if dirty_after_refresh {
      tracing::debug!(
        target: "lurq::reactivity",
        "[lurq:reactivity] dirty refresh left pending work replacements={}",
        replacements.len()
      );
    }

    if let Some(root) = &mut self.root {
      for (slot_id, replacement) in replacements {
        replace_live_component_slot_everywhere(root, slot_id, &replacement, &self.id_gen);
      }
    }

    if let Some(root) = &mut self.root {
      root.assign_ids(&self.id_gen);
    }
    self.tree_rebuilt_since_layout = true;
    self.cached_render_list = None;
    self.needs_redraw = true;
    self.refresh_interaction_state();
    self.apply_focus_request();
    true
  }

  pub fn register_keyframes(&mut self, keyframes: Keyframes) {
    self.animation_engine.register_keyframes(keyframes);
    // A node may already play them: its run waits for its frames and asks for no frame of its own.
    self.needs_redraw = true;
  }

  /// Registers keyframes whose runs all play in phase, on the process's animation clock
  /// ([`crate::animation::clock_origin`]): every node playing them, in this window or another, shows the same
  /// pose however late it mounted, and a `Steps` animation shown many times draws one frame per step. See
  /// [`AnimationEngine::register_keyframes_in_phase`] for how direction and `delay` play in phase.
  pub fn register_keyframes_in_phase(&mut self, keyframes: Keyframes) {
    self.animation_engine.register_keyframes_in_phase(keyframes);
    self.needs_redraw = true;
  }

  /// Ticks animations and transitions at `now` in every pass instead of at the moment the pass runs, and reads
  /// the wheel's axis lock ([`WHEEL_AXIS_LOCK_WINDOW`]) at `now` too, until set back to `None`. For tests that
  /// drive time explicitly; a shell paces its frames on the wall clock.
  #[doc(hidden)]
  pub fn set_animation_clock(&mut self, now: Option<Instant>) {
    self.animation_clock = now;
  }

  /// The runtime's clock: the one [`Self::set_animation_clock`] set, or the wall clock.
  fn now(&self) -> Instant {
    self.animation_clock.unwrap_or_else(Instant::now)
  }

  /// The vertical delta a wheel or swipe event belongs to, for telling a
  /// mostly vertical gesture from a sideways one: the event's own, or, for an
  /// event without one, the vertical delta seen within
  /// [`WHEEL_AXIS_LOCK_WINDOW`] at the same pointer. Windows delivers a
  /// precision touchpad's slanted swipe as separate vertical and horizontal
  /// events (`WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL`), so a horizontal event alone
  /// says nothing about the gesture's slant; a mouse's Shift+wheel, with no
  /// vertical wheel just before it at that spot, is sideways. An event with a
  /// vertical delta is remembered until the window passes.
  fn wheel_gesture_vertical(&mut self, x: f32, y: f32, delta_y: f32) -> f32 {
    let now = self.now();
    if delta_y != 0.0 {
      self.recent_vertical_wheel = Some(RecentVerticalWheel {
        at: now,
        x,
        y,
        delta: delta_y,
      });
      return delta_y;
    }
    match self.recent_vertical_wheel {
      Some(recent)
        if now.duration_since(recent.at) <= WHEEL_AXIS_LOCK_WINDOW
          && (recent.x - x).powi(2) + (recent.y - y).powi(2) <= WHEEL_AXIS_LOCK_DISTANCE * WHEEL_AXIS_LOCK_DISTANCE =>
      {
        recent.delta
      }
      _ => 0.0,
    }
  }

  fn update_layout(&mut self, app: &mut App) -> bool {
    let component_dirty_before_rebuild = self.root_ctx.as_ref().is_some_and(Ctx::any_dirty);
    self.transition_engine.begin_frame();
    self.animation_engine.begin_frame();
    self.rebuild_if_dirty();
    self.apply_focus_request();
    self.rebuild_if_dirty();
    self.sync_dynamic_content();
    #[cfg(all(feature = "image", feature = "resources"))]
    let image_resources_changed = self.resolve_resource_images(app);
    #[cfg(not(all(feature = "image", feature = "resources")))]
    let image_resources_changed = false;
    #[cfg(feature = "image")]
    let image_resources_changed = self.resolve_loading_images() || image_resources_changed;
    #[cfg(all(feature = "svg", feature = "resources"))]
    let svg_resources_changed = self.resolve_resource_svgs(app);
    #[cfg(not(all(feature = "svg", feature = "resources")))]
    let svg_resources_changed = false;

    let now = self.animation_clock.unwrap_or_else(Instant::now);
    let had_overlay_host = self
      .root
      .as_ref()
      .is_some_and(|root| root.has_synthetic_role(SyntheticNodeRole::OverlayHost));
    let had_active_overlays = self
      .root
      .as_ref()
      .is_some_and(|root| root.has_synthetic_role(SyntheticNodeRole::OverlayHost) && root.children.len() > 1);
    let mut animation_layout_changed = false;
    if had_overlay_host && !had_active_overlays {
      if let Some(root) = self.root.as_mut()
        && let Some(base) = root.children.first_mut()
      {
        animation_layout_changed |= self.transition_engine.tick(base, now);
        animation_layout_changed |= self.animation_engine.tick(base, now);
      }
    } else if !had_overlay_host && let Some(root) = self.root.as_mut() {
      animation_layout_changed |= self.transition_engine.tick(root, now);
      animation_layout_changed |= self.animation_engine.tick(root, now);
    }

    if let Some(root) = self.root.as_ref() {
      let constraints = self
        .layout_constraints_override
        .unwrap_or_else(|| Constraints::tight(self.viewport_logical()));
      let theme_version = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().version())
        .unwrap_or_else(|| app.theme().version());
      let theme_changed = self.last_theme_version != theme_version;
      let has_active_overlays = root.has_synthetic_role(SyntheticNodeRole::OverlayHost) && root.children.len() > 1;
      root.prepare_layout_tracking();
      let has_dirty_element_ref = has_dirty_element_ref_recursive(root);
      let has_pending_layout_dirty = has_pending_layout_dirty_recursive(root);
      let has_runtime_layout_state = has_runtime_layout_state_recursive(root);
      let has_active_timeline = self.has_active_timeline();
      let tree_rebuilt_since_layout = self.tree_rebuilt_since_layout;
      let has_render_dirty_timeline_target = self.cached_render_list.is_none() && has_timeline_target_recursive(root);
      let has_last_layout = self.last_layout.is_some();
      let root_cache_contains = root.layout_cache.contains(constraints);
      let root_render_dirty = root.has_render_dirty();
      let component_dirty = component_dirty_before_rebuild || self.root_ctx.as_ref().is_some_and(Ctx::any_dirty);
      if !animation_layout_changed
        && !image_resources_changed
        && !svg_resources_changed
        && !theme_changed
        && !has_active_timeline
        && !tree_rebuilt_since_layout
        && !has_render_dirty_timeline_target
        && !component_dirty
        && !has_dirty_element_ref
        && !has_pending_layout_dirty
        && !has_runtime_layout_state
        && has_last_layout
        && root_cache_contains
      {
        self.last_theme_version = theme_version;
        #[cfg(feature = "canvas")]
        if let Some(layout) = self.last_layout.as_ref()
          && root.prepare_layout_tracking().canvas
        {
          let offset = root.offset_position().unwrap_or_default();
          update_canvas_placement_recursive(
            root,
            layout,
            &self.layout_engine,
            offset.x,
            offset.y,
            crate::node::transform::Transform2D::IDENTITY,
          );
        }
        self.apply_focus_request_inner(true);
        return false;
      }
      if self.has_active_timeline() || component_dirty || has_pending_layout_dirty {
        log_layout_fast_path_miss_timeline(
          animation_layout_changed,
          image_resources_changed,
          svg_resources_changed,
          theme_changed,
          has_active_overlays,
          has_dirty_element_ref,
          has_pending_layout_dirty,
          has_runtime_layout_state,
          has_last_layout,
          root_cache_contains,
          root_render_dirty,
          root.children.len(),
          component_dirty,
        );
      }
    }

    let overlay_parts = self.detach_overlay_host();

    if had_overlay_host && let Some(root) = self.root.as_mut() {
      self.transition_engine.tick(root, now);
      self.animation_engine.tick(root, now);
    }

    if let Some(root) = self.root.as_ref() {
      let constraints = self
        .layout_constraints_override
        .unwrap_or_else(|| Constraints::tight(self.viewport_logical()));
      let theme_version = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().version())
        .unwrap_or_else(|| app.theme().version());
      self.layout_engine.set_shadows(
        self
          .root_ctx
          .as_ref()
          .map(|ctx| ctx.theme().shared_shadows())
          .unwrap_or_else(|| app.theme().shared_shadows()),
      );
      let typography = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().typography().clone())
        .unwrap_or_else(|| app.theme().typography().clone());
      let palette = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().palette().clone())
        .unwrap_or_else(|| app.theme().palette().clone());
      let spacing = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().spacing().clone())
        .unwrap_or_else(|| app.theme().spacing().clone());
      let radii = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().radii().clone())
        .unwrap_or_else(|| app.theme().radii().clone());
      let caret = self
        .root_ctx
        .as_ref()
        .map(|ctx| *ctx.theme().caret())
        .unwrap_or_else(|| *app.theme().caret());
      let scrollbar = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().scrollbar().clone())
        .unwrap_or_else(|| app.theme().scrollbar().clone());
      let border_sizes = self
        .root_ctx
        .as_ref()
        .map(|ctx| ctx.theme().border_sizes().clone())
        .unwrap_or_else(|| app.theme().border_sizes().clone());
      let theme_changed = self.last_theme_version != theme_version;
      let (mut layout, base_overlay_index) = crate::app::profiler::producer::layout_compute(
        #[cfg(feature = "perf_profile")]
        &mut self.profiling,
        || {
          self.layout_engine.compute_with_overlay_index(
            &mut app.shared.glyph_engine.lock(),
            root,
            constraints,
            palette.clone(),
            border_sizes.clone(),
            spacing.clone(),
            radii.clone(),
            caret,
            scrollbar.clone(),
            typography.clone(),
            theme_changed,
          )
        },
      );
      let focus_return = self.sync_overlay_host_from_layout(
        overlay_parts,
        base_overlay_index,
        &mut app.shared.glyph_engine.lock(),
        constraints,
        palette.clone(),
        border_sizes.clone(),
        spacing.clone(),
        radii.clone(),
        caret,
        scrollbar.clone(),
        typography.clone(),
        theme_changed,
      );
      // Focus/blur callbacks may mutate App services; the glyph lock used to
      // construct the overlay host must be released before dispatching them.
      if had_overlay_host
        || self
          .root
          .as_ref()
          .is_some_and(|root| root.has_synthetic_role(SyntheticNodeRole::OverlayHost))
      {
        self.refresh_interaction_state();
      }
      if let Some(anchor) = focus_return {
        self.return_focus_to_anchor(&anchor);
      }
      self.apply_focus_request_inner(true);
      self.tick_overlay_subtrees(now);
      if let Some(root) = self.root.as_ref()
        && root.has_synthetic_role(SyntheticNodeRole::OverlayHost)
      {
        layout = crate::app::profiler::producer::layout_compute(
          #[cfg(feature = "perf_profile")]
          &mut self.profiling,
          || {
            self.layout_engine.compute(
              &mut app.shared.glyph_engine.lock(),
              root,
              constraints,
              palette.clone(),
              border_sizes.clone(),
              spacing.clone(),
              radii.clone(),
              caret,
              scrollbar.clone(),
              typography.clone(),
              theme_changed,
            )
          },
        );
      }
      self.last_theme_version = theme_version;
      #[cfg(feature = "canvas")]
      self.canvas_registry.clear();
      #[cfg(feature = "canvas")]
      if let Some(root) = self.root.as_ref() {
        let offset = root.offset_position().unwrap_or_default();
        let canvas_text = app.shared.glyph_engine.lock().canvas_text_engine();
        let _canvas_start = profile_scope!();
        #[cfg(feature = "perf_profile")]
        let _canvas_phase = self
          .profiling
          .context
          .phase(crate::app::profiler::Phase::CanvasRecording);
        bind_canvas_layout_recursive(
          root,
          &layout,
          &self.layout_engine,
          offset.x,
          offset.y,
          crate::node::transform::Transform2D::IDENTITY,
          self.scale_factor,
          &self.window,
          &crate::canvas::CanvasFont::from_style(typography.default_style()),
          &canvas_text,
          &mut self.canvas_registry,
        );
        #[cfg(feature = "perf_profile")]
        self.profiling.canvas_recording(profile_elapsed!(_canvas_start));
      }
      if let Some(root) = self.root.as_mut() {
        update_element_refs_recursive(root, &layout, 0.0, 0.0, 0.0, 0.0);
        verify_scroll_offsets(root, &layout);
      }
      if let (Some(component), Some(ctx)) = (&self.root_component, &self.root_ctx) {
        let _after_layout_start = profile_scope!();
        #[cfg(feature = "perf_profile")]
        let _after_layout_phase = self
          .profiling
          .context
          .phase(crate::app::profiler::Phase::ComponentAfterLayout);
        component.after_layout();
        ctx.after_layout_recursive();
        #[cfg(feature = "perf_profile")]
        self
          .profiling
          .component_after_layout(profile_elapsed!(_after_layout_start));
        if ctx.any_dirty() {
          self.needs_redraw = true;
        }
      }
      if let Some(root) = &self.root {
        self.dropped_nodes = tab_navigation::dropped_node_ids(root, &layout);
      }
      self.last_layout = Some(layout);
      self.blur_focus_in_dropped_child();
      self.tree_rebuilt_since_layout = false;
      // All tick walks for this frame ran (base tree + overlay subtrees), so
      // the engines can now drop runs whose nodes left the tree; otherwise an
      // unmounted spinner keeps the timeline "active" and forces this full
      // layout pass every frame forever.
      self.transition_engine.finish_frame();
      self.animation_engine.finish_frame();
      return true;
    }
    false
  }

  fn tick_overlay_subtrees(&mut self, now: Instant) {
    let Some(root) = self.root.as_mut() else {
      return;
    };
    if !root.has_synthetic_role(SyntheticNodeRole::OverlayHost) {
      return;
    }

    for child in root.children.iter_mut().skip(1) {
      self.transition_engine.tick_preserving_active_state(child, now);
      self.animation_engine.tick_preserving_active_state(child, now);
    }
  }

  fn detach_overlay_host(&mut self) -> OverlayHostReuse {
    let Some(root) = self.root.take() else {
      return OverlayHostReuse {
        old_host: None,
        old_overlays: Vec::new(),
        old_overlay_used: Vec::new(),
        old_overlay_layout_dirty: Vec::new(),
      };
    };
    let parts = overlay_host_parts(root);
    let old_overlay_layout_dirty = parts
      .old_overlays
      .iter()
      .map(has_pending_layout_dirty_recursive)
      .collect();
    self.root = Some(parts.base);
    let old_overlay_used = vec![false; parts.old_overlays.len()];
    OverlayHostReuse {
      old_host: parts.old_host,
      old_overlays: parts.old_overlays,
      old_overlay_used,
      old_overlay_layout_dirty,
    }
  }

  #[allow(clippy::too_many_arguments)]
  fn sync_overlay_host_from_layout(
    &mut self,
    mut old_parts: OverlayHostReuse,
    mut base_overlay_index: OverlayLayoutIndex,
    glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
    constraints: Constraints,
    palette: crate::app::theme::ThemePalette,
    border_sizes: crate::app::theme::ThemeBorderSizes,
    spacing: crate::app::theme::ThemeSpacing,
    radii: crate::app::theme::ThemeRadii,
    caret: crate::app::theme::ThemeCaret,
    scrollbar: crate::layout::scrollbar::ScrollBarStyle,
    typography: crate::app::theme::ThemeTypography,
    theme_changed: bool,
  ) -> Option<OwnedElementRef> {
    let Some(base) = self.root.take() else {
      #[cfg(feature = "canvas")]
      detach_canvases_not_carried(old_parts.unused_canvas_bindings(), None);
      old_parts.free_ids(&self.id_gen);
      self.overlay_dismiss_entries.clear();
      self.overlay_layer_anchors.clear();
      self.dropdown_anchors.clear();
      return None;
    };

    let viewport = self.viewport_logical();
    let mut layers = OverlayLayers::default();
    let mut overlays = build_overlays_from_layout_index(
      &base_overlay_index,
      viewport,
      glyph_engine,
      &self.layout_engine,
      constraints,
      palette.clone(),
      border_sizes.clone(),
      spacing.clone(),
      radii.clone(),
      caret,
      scrollbar.clone(),
      typography.clone(),
      theme_changed,
      &mut layers,
      #[cfg(feature = "perf_profile")]
      &mut self.profiling,
    );

    if overlays.is_empty() {
      let focus_return = self.focused_overlay_hidden(old_parts.old_host.is_some(), &layers);
      #[cfg(feature = "canvas")]
      detach_canvases_not_carried(old_parts.unused_canvas_bindings(), Some(&base));
      old_parts.free_ids(&self.id_gen);
      self.root = Some(base);
      // Overlays hidden with their anchor are still dismissed.
      self.overlay_dismiss_entries = layers.dismiss_entries;
      self.overlay_layer_anchors.clear();
      self.dropdown_anchors.clear();
      return focus_return;
    }

    let mut dropdown_anchors = std::mem::take(&mut base_overlay_index.dropdown_anchors);
    let mut old_overlay_index = 0;
    for overlay in &mut overlays {
      preserve_overlay_reuse(overlay, &mut old_parts, old_overlay_index);
      old_overlay_index += 1;
    }

    let mut overlay_index = 0;
    while overlay_index < overlays.len() {
      let nested_overlays = if overlays[overlay_index].has_synthetic_role(SyntheticNodeRole::SelectMenu) {
        Vec::new()
      } else {
        let overlay = &overlays[overlay_index];
        let (_overlay_layout, mut overlay_layout_index) = crate::app::profiler::producer::layout_compute(
          #[cfg(feature = "perf_profile")]
          &mut self.profiling,
          || {
            self.layout_engine.compute_with_overlay_index(
              glyph_engine,
              overlay,
              constraints,
              palette.clone(),
              border_sizes.clone(),
              spacing.clone(),
              radii.clone(),
              caret,
              scrollbar.clone(),
              typography.clone(),
              theme_changed,
            )
          },
        );
        let (origin_x, origin_y) = match overlay.position() {
          Position::Absolute { x, y, .. } => (x, y),
          Position::Static => (0.0, 0.0),
        };
        translate_overlay_layout_index(&mut overlay_layout_index, origin_x, origin_y);
        dropdown_anchors.append(&mut overlay_layout_index.dropdown_anchors);
        let nested_entries_start = layers.dismiss_entries.len();
        let nested = build_overlays_from_layout_index(
          &overlay_layout_index,
          viewport,
          glyph_engine,
          &self.layout_engine,
          constraints,
          palette.clone(),
          border_sizes.clone(),
          spacing.clone(),
          radii.clone(),
          caret,
          scrollbar.clone(),
          typography.clone(),
          theme_changed,
          &mut layers,
          #[cfg(feature = "perf_profile")]
          &mut self.profiling,
        );
        // Nested overlays are appended after the ones built so far.
        for entry in &mut layers.dismiss_entries[nested_entries_start..] {
          entry.layer += overlays.len();
        }
        nested
      };

      for mut nested_overlay in nested_overlays {
        preserve_overlay_reuse(&mut nested_overlay, &mut old_parts, old_overlay_index);
        old_overlay_index += 1;
        overlays.push(nested_overlay);
      }
      overlay_index += 1;
    }

    let old_parts_had_host = old_parts.old_host.is_some();
    let mut children = Vec::with_capacity(1 + overlays.len());
    children.push(base);
    children.extend(overlays);
    let mut host = Node::stack(crate::layout::StackAlignment::TopStart, children);
    host.set_tag_name("OverlayHost");
    host.set_synthetic_role(SyntheticNodeRole::OverlayHost);
    if let Some(old_host) = old_parts.old_host.as_mut() {
      #[cfg(feature = "canvas")]
      detach_canvas_recursive(old_host);
      reset_element_ref_flags_recursive(old_host);
      host.preserve_ids_from(old_host);
    }
    #[cfg(feature = "canvas")]
    detach_canvases_not_carried(old_parts.unused_canvas_bindings(), Some(&host));
    old_parts.free_ids(&self.id_gen);
    host.assign_ids(&self.id_gen);
    self.root = Some(host);
    let focus_return = self.focused_overlay_hidden(old_parts_had_host, &layers);
    self.overlay_dismiss_entries = layers.dismiss_entries;
    self.overlay_layer_anchors = layers.anchors;
    self.dropdown_anchors = dropdown_anchors;
    focus_return
  }

  fn sync_dynamic_content(&mut self) {
    if let Some(root) = &mut self.root {
      root.sync_dynamic_content_recursive();
    }
  }

  #[cfg(all(feature = "image", feature = "resources"))]
  fn resolve_resource_images(&mut self, app: &mut App) -> bool {
    if let Some(root) = &mut self.root {
      let changed = Self::resolve_resource_images_recursive(
        root,
        &app.shared.resource_loader.lock(),
        &mut app.shared.image_resource_cache.lock(),
      );
      if changed {
        self.needs_redraw = true;
      }
      changed
    } else {
      false
    }
  }

  #[cfg(all(feature = "image", feature = "resources"))]
  fn resolve_resource_images_recursive(
    node: &mut Node,
    loader: &crate::resources::ResourceLoader,
    image_cache: &mut std::collections::HashMap<Arc<str>, crate::images::ImageData>,
  ) -> bool {
    let mut layout_dirty = false;

    if let NodeKind::ResourceImage { path } = node.node_kind() {
      let key: std::sync::Arc<str> = path.clone();
      if let Some(img) = Self::resolve_image_resource(&key, loader, image_cache) {
        node.intrinsic_size = Some(Size::new(img.width() as f32, img.height() as f32));
        node.node_kind = NodeKind::Image {
          data: img,
          fit: crate::node::BackgroundSize::Stretch,
        };
        layout_dirty = true;
      }
    }

    if let Some(key) = node.background_resource_image.clone() {
      if let Some(img) = Self::resolve_image_resource(&key, loader, image_cache) {
        let current_id = node.background_image.as_ref().map(crate::images::ImageData::id);
        if current_id != Some(img.id()) {
          node.background_image.set(Some(img));
          layout_dirty = true;
        }
      }
    }

    match node.node_kind() {
      NodeKind::Checkbox { state } => {
        if state.resolve_resource_images(|key| Self::resolve_image_resource(key, loader, image_cache)) {
          layout_dirty = true;
        }
      }
      NodeKind::Slider { state } => {
        if state.resolve_resource_images(|key| Self::resolve_image_resource(key, loader, image_cache)) {
          layout_dirty = true;
        }
      }
      _ => {}
    }

    for child in &mut node.children {
      if Self::resolve_resource_images_recursive(child, loader, image_cache) {
        layout_dirty = true;
      }
    }

    if layout_dirty {
      node.layout_cache.invalidate();
    }

    layout_dirty
  }

  #[cfg(all(feature = "image", feature = "resources"))]
  fn resolve_image_resource(
    key: &Arc<str>,
    loader: &crate::resources::ResourceLoader,
    image_cache: &mut std::collections::HashMap<Arc<str>, crate::images::ImageData>,
  ) -> Option<crate::images::ImageData> {
    if let Some(img) = image_cache.get(key) {
      return Some(img.clone());
    }

    let crate::resources::LoadResourceResult::Loaded(bytes) = loader.load_resource(key, None) else {
      return None;
    };

    let img = crate::images::ImageData::from_bytes(&bytes).ok()?;
    image_cache.insert(key.clone(), img.clone());
    Some(img)
  }

  #[cfg(all(feature = "svg", feature = "resources"))]
  fn resolve_resource_svgs(&mut self, app: &mut App) -> bool {
    if let Some(root) = &mut self.root {
      let changed = Self::resolve_resource_svgs_recursive(
        root,
        &app.shared.resource_loader.lock(),
        &mut app.shared.svg_resource_cache.lock(),
      );
      if changed {
        self.needs_redraw = true;
      }
      changed
    } else {
      false
    }
  }

  #[cfg(all(feature = "svg", feature = "resources"))]
  fn resolve_resource_svgs_recursive(
    node: &mut Node,
    loader: &crate::resources::ResourceLoader,
    svg_cache: &mut std::collections::HashMap<Arc<str>, crate::svg::SvgData>,
  ) -> bool {
    let mut layout_dirty = false;

    if let NodeKind::ResourceSvg { path } = node.node_kind() {
      let key: Arc<str> = path.clone();
      if let Some(svg) = Self::resolve_svg_resource(&key, loader, svg_cache) {
        node.intrinsic_size = Some(Size::new(svg.viewbox_width(), svg.viewbox_height()));
        node.node_kind = NodeKind::Svg { data: svg };
        layout_dirty = true;
      }
    }

    for child in &mut node.children {
      if Self::resolve_resource_svgs_recursive(child, loader, svg_cache) {
        layout_dirty = true;
      }
    }

    if layout_dirty {
      node.layout_cache.invalidate();
    }

    layout_dirty
  }

  #[cfg(all(feature = "svg", feature = "resources"))]
  fn resolve_svg_resource(
    key: &Arc<str>,
    loader: &crate::resources::ResourceLoader,
    svg_cache: &mut std::collections::HashMap<Arc<str>, crate::svg::SvgData>,
  ) -> Option<crate::svg::SvgData> {
    if let Some(svg) = svg_cache.get(key) {
      return Some(svg.clone());
    }

    let crate::resources::LoadResourceResult::Loaded(bytes) = loader.load_resource(key, None) else {
      return None;
    };

    let svg = crate::svg::SvgData::from_bytes(&bytes);
    svg_cache.insert(key.clone(), svg.clone());
    Some(svg)
  }

  fn clear_active_path(&mut self) {
    let active_path = std::mem::take(&mut self.active_path);
    if let Some(root) = self.root.as_ref() {
      for node_id in active_path {
        if let Some(node) = find_node_by_id(root, node_id) {
          set_node_active(node, false);
          self.cached_render_list = None;
          self.needs_redraw = true;
        }
      }
    }
  }

  fn clear_hover_path(&mut self) {
    let hover_path = std::mem::take(&mut self.hover_path);
    if hover_path.is_empty() {
      self.cursor = CursorIcon::Default;
      return;
    }

    if let Some(root) = self.root.as_ref() {
      for node_id in hover_path {
        if let Some(node) = find_node_by_id(root, node_id) {
          set_node_hovered(node, false);
          self.cached_render_list = None;
          for handler in &node.events.on_mouse_leave {
            handler.call();
          }
        }
      }
    }

    self.cursor = CursorIcon::Default;
    self.needs_redraw = true;
  }

  fn refresh_interaction_state(&mut self) {
    let Some(root) = self.root.as_ref() else {
      self.hover_path.clear();
      self.active_path.clear();
      self.cursor = CursorIcon::Default;
      self.clear_focus();
      return;
    };

    reset_element_ref_flags_recursive(root);

    let mut hover_path = Vec::new();
    for node_id in &self.hover_path {
      if let Some(node) = find_node_by_id(root, *node_id) {
        set_node_hovered(node, true);
        self.cached_render_list = None;
        hover_path.push(*node_id);
      }
    }
    self.hover_path = hover_path;

    let mut active_path = Vec::new();
    for node_id in &self.active_path {
      if let Some(node) = find_node_by_id(root, *node_id) {
        set_node_active(node, true);
        self.cached_render_list = None;
        active_path.push(*node_id);
      }
    }
    self.active_path = active_path;

    self.cursor = hovered_cursor(
      self
        .hover_path
        .iter()
        .filter_map(|node_id| find_node_by_id(root, *node_id)),
    );
    self.refresh_focus_ids();
    self.refresh_file_drag_state();
  }

  fn clear_focus(&mut self) {
    let had_focus = self.focused_node.is_some() || self.focused_event_node.is_some();
    if let Some(root) = &self.root {
      for id in [self.focused_node, self.focused_event_node].into_iter().flatten() {
        if let Some(node) = find_node_by_id(root, id) {
          set_node_focused(node, false);
        }
      }
    }
    self.focused_node = None;
    self.focused_event_node = None;
    self.focused_path = None;
    self.focused_event_path = None;
    if had_focus {
      self.cached_render_list = None;
    }
    std::mem::take(&mut self.focus_memory).clear();
  }

  fn blur_focus(&mut self) -> bool {
    if self.focused_node.is_none() {
      return false;
    }
    self.clear_focus();
    self.needs_redraw = true;
    true
  }

  fn refresh_focus_ids(&mut self) {
    let Some(root) = self.root.as_ref() else {
      self.clear_focus();
      return;
    };

    let Some(focused_node) = self.focused_node else {
      self.clear_focus();
      return;
    };
    let Some(input_path) = find_path_by_id(root, focused_node) else {
      self.clear_focus();
      return;
    };
    let event_id = self.focused_event_node.unwrap_or(focused_node);
    let event_path = find_path_by_id(root, event_id).unwrap_or_else(|| input_path.clone());

    let memory = FocusMemory::capture(root, focused_node, event_id);
    for old in &self.focus_memory.refs {
      if !memory.refs.iter().any(|current| old.same_handle(current)) {
        old.set_focused(false);
      }
    }
    self.focus_memory = memory;
    self.focused_path = Some(input_path.clone());
    self.focused_event_path = Some(event_path.clone());

    let input = find_node_by_path(root, &input_path);
    let visible = input.is_some_and(|node| focus_visible::shows_focus_ring(self.input_modality, node));
    if let Some(node) = input {
      set_node_focused(node, true);
      focus_visible::set_node_focus_visible(node, visible);
      self.cached_render_list = None;
      if let NodeKind::TextInput { state, .. } = node.node_kind() {
        state.set_focused(true);
      }
    }
    if let Some(node) = find_node_by_path(root, &event_path) {
      set_node_focused(node, true);
      focus_visible::set_node_focus_visible(node, visible);
      self.cached_render_list = None;
    }
  }
}

struct OverlayHostParts {
  base: Node,
  old_host: Option<Node>,
  old_overlays: Vec<Node>,
}

struct OverlayHostReuse {
  old_host: Option<Node>,
  old_overlays: Vec<Node>,
  old_overlay_used: Vec<bool>,
  old_overlay_layout_dirty: Vec<bool>,
}

impl OverlayHostReuse {
  /// Canvases of the old overlays that no new overlay reused. Their nodes are
  /// dropped after the rebuild, so their surfaces must be detached.
  #[cfg(feature = "canvas")]
  fn unused_canvas_bindings(&self) -> CanvasBindings {
    self
      .old_overlays
      .iter()
      .enumerate()
      .filter(|(index, _)| !self.old_overlay_used.get(*index).copied().unwrap_or(false))
      .flat_map(|(_, overlay)| canvas_bindings(overlay))
      .collect()
  }

  fn free_ids(&mut self, id_gen: &IdGenerator) {
    if let Some(old_host) = &mut self.old_host {
      old_host.free_ids(id_gen);
    }
    for old_overlay in &mut self.old_overlays {
      old_overlay.free_ids(id_gen);
    }
  }
}

fn overlay_host_parts(mut root: Node) -> OverlayHostParts {
  if root.has_synthetic_role(SyntheticNodeRole::OverlayHost) && !root.children.is_empty() {
    let mut children = std::mem::take(&mut root.children);
    let base = children.remove(0);
    OverlayHostParts {
      base,
      old_host: Some(root),
      old_overlays: children,
    }
  } else {
    OverlayHostParts {
      base: root,
      old_host: None,
      old_overlays: Vec::new(),
    }
  }
}

fn root_with_preserved_overlay_parts(base: Node, mut parts: OverlayHostParts, id_gen: &IdGenerator) -> Node {
  parts.base.free_ids(id_gen);

  let Some(mut old_host) = parts.old_host else {
    return base;
  };

  let mut children = Vec::with_capacity(1 + parts.old_overlays.len());
  children.push(base);
  children.append(&mut parts.old_overlays);

  let mut host = Node::stack(crate::layout::StackAlignment::TopStart, children);
  host.set_tag_name("OverlayHost");
  host.set_synthetic_role(SyntheticNodeRole::OverlayHost);
  host.preserve_runtime_state_from(&mut old_host);
  host.preserve_ids_from(&mut old_host);
  old_host.free_ids(id_gen);
  host
}

fn translate_overlay_layout_index(index: &mut OverlayLayoutIndex, x: f32, y: f32) {
  for element in &mut index.elements {
    element.rect.x += x;
    element.rect.y += y;
  }
  for overlay in &mut index.overlays {
    match overlay {
      OverlayLayoutRecord::SelectMenu { bounds, .. } => {
        bounds.x += x;
        bounds.y += y;
      }
      OverlayLayoutRecord::Modal { parent, .. } => {
        parent.x += x;
        parent.y += y;
      }
      OverlayLayoutRecord::Overlay { .. } => {}
    }
  }
}

fn preserve_overlay_reuse(overlay: &mut Node, old_parts: &mut OverlayHostReuse, index: usize) {
  let key = overlay.component_key().map(str::to_owned);
  if let Some(key) = key
    && let Some(keyed_index) = old_parts
      .old_overlays
      .iter()
      .enumerate()
      .find(|(old_index, old_overlay)| {
        !old_parts.old_overlay_used.get(*old_index).copied().unwrap_or(false)
          && old_overlay.component_key() == Some(key.as_str())
      })
      .map(|(old_index, _)| old_index)
  {
    preserve_overlay_reuse_at(overlay, old_parts, keyed_index);
    return;
  }

  if index < old_parts.old_overlays.len() && !old_parts.old_overlay_used.get(index).copied().unwrap_or(false) {
    preserve_overlay_reuse_at(overlay, old_parts, index);
  }
}

fn preserve_overlay_reuse_at(overlay: &mut Node, old_parts: &mut OverlayHostReuse, index: usize) {
  let Some(old_overlay) = old_parts.old_overlays.get_mut(index) else {
    return;
  };
  let old_layout_dirty = old_parts.old_overlay_layout_dirty.get(index).copied().unwrap_or(false);
  if let Some(used) = old_parts.old_overlay_used.get_mut(index) {
    *used = true;
  }
  reset_element_ref_flags_recursive(old_overlay);
  overlay.preserve_runtime_state_from(old_overlay);
  #[cfg(feature = "canvas")]
  detach_canvases_not_carried(canvas_bindings(old_overlay), Some(overlay));
  overlay.preserve_ids_from(old_overlay);
  if old_layout_dirty {
    invalidate_layout_cache_recursive(overlay);
  }
}

#[allow(clippy::too_many_arguments)]
fn build_overlays_from_layout_index(
  index: &OverlayLayoutIndex,
  viewport: Size,
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  layout_engine: &LayoutEngine,
  constraints: Constraints,
  palette: crate::app::theme::ThemePalette,
  border_sizes: crate::app::theme::ThemeBorderSizes,
  spacing: crate::app::theme::ThemeSpacing,
  radii: crate::app::theme::ThemeRadii,
  caret: crate::app::theme::ThemeCaret,
  scrollbar: crate::layout::scrollbar::ScrollBarStyle,
  typography: crate::app::theme::ThemeTypography,
  theme_changed: bool,
  layers: &mut OverlayLayers,
  #[cfg(feature = "perf_profile")] profiling: &mut crate::app::profiler::producer::WindowProfiler,
) -> Vec<Node> {
  let mut overlays = Vec::new();

  for record in &index.overlays {
    match record.clone() {
      OverlayLayoutRecord::SelectMenu {
        reuse_key,
        state,
        bounds,
        visible,
      } => {
        // A select scrolled out of view does not draw its menu over what
        // covers it; the menu comes back with the select.
        if !visible {
          continue;
        }
        let mut measure = |menu: &Node| {
          let measure_constraints = Constraints::loose(Size::new(
            constraints.max_width.min(viewport.width).max(0.0),
            constraints.max_height.min(viewport.height).max(0.0),
          ));
          crate::app::profiler::producer::layout_compute(
            #[cfg(feature = "perf_profile")]
            profiling,
            || {
              layout_engine.compute(
                glyph_engine,
                &menu.clone_for_reuse(),
                measure_constraints,
                palette.clone(),
                border_sizes.clone(),
                spacing.clone(),
                radii.clone(),
                caret,
                scrollbar.clone(),
                typography.clone(),
                theme_changed,
              )
            },
          )
        };
        let mut menu = select_menu::build_select_menu(&state, bounds, viewport, &mut measure);
        set_overlay_reuse_key(&mut menu, reuse_key.as_deref());
        overlays.push(menu);
        layers.anchors.push(None);
      }
      OverlayLayoutRecord::Overlay { reuse_key, spec } => {
        let Some(anchor) = find_element_record(index, &spec.anchor) else {
          continue;
        };
        // An anchor scrolled or clipped out of view takes its overlay with
        // it: left in place, the overlay would float over whatever now covers
        // the anchor, such as the header above a scrolled list.
        if !anchor.visible {
          let dismiss = HiddenDismiss {
            open: spec.open_signal.clone(),
            on_outside_click: spec.dismiss_on_outside_click,
            on_escape: spec.dismiss_on_escape,
          };
          layers.hide(&spec.anchor, dismiss, overlays.len() + 1);
          continue;
        }
        let anchor = anchor.rect;
        if anchor.width <= 0.0 || anchor.height <= 0.0 {
          continue;
        }
        let dismiss_anchor = spec.anchor.clone();
        let layer_anchor = spec.anchor.clone();
        let dismiss_signal = spec.open_signal.clone();
        let dismiss_on_outside_click = spec.dismiss_on_outside_click;
        let outside_press = spec.outside_press;
        let dismiss_on_escape = spec.dismiss_on_escape;
        let (mut overlay, bounds) = build_overlay_node(
          spec,
          anchor,
          viewport,
          glyph_engine,
          layout_engine,
          constraints,
          palette.clone(),
          border_sizes.clone(),
          spacing.clone(),
          radii.clone(),
          caret,
          scrollbar.clone(),
          typography.clone(),
          theme_changed,
          #[cfg(feature = "perf_profile")]
          profiling,
        );
        set_overlay_reuse_key(&mut overlay, reuse_key.as_deref());
        if let Some(open) = dismiss_signal
          && (dismiss_on_outside_click || dismiss_on_escape)
        {
          layers.dismiss_entries.push(OverlayDismissEntry {
            anchor: dismiss_anchor,
            bounds,
            open,
            dismiss_on_outside_click,
            outside_press,
            dismiss_on_escape,
            layer: overlays.len() + 1,
            shown: true,
          });
        }
        overlays.push(overlay);
        layers.anchors.push(Some(layer_anchor));
      }
      OverlayLayoutRecord::Modal {
        reuse_key,
        spec,
        parent,
      } => {
        // A modal over an element scrolled out of view is hidden like an
        // overlay whose anchor is.
        if let ModalTarget::Element(target) = &spec.target
          && find_element_record(index, target).is_some_and(|record| !record.visible)
        {
          let dismiss = HiddenDismiss {
            open: spec.open_signal.clone(),
            on_outside_click: false,
            on_escape: spec.dismiss_on_escape,
          };
          layers.hide(target, dismiss, overlays.len() + 1);
          continue;
        }
        let owner = match &spec.target {
          ModalTarget::Element(target) => Some(target.clone()),
          _ => None,
        };
        let target = match modal_target_rect_from_index(&spec, parent, viewport, index) {
          Some(target) if target.width > 0.0 && target.height > 0.0 => target,
          _ => continue,
        };
        let dismiss_signal = spec.open_signal.clone();
        let dismiss_on_escape = spec.dismiss_on_escape;
        let mut modal = build_modal_node(spec, target);
        set_overlay_reuse_key(&mut modal, reuse_key.as_deref());
        if let Some(open) = dismiss_signal
          && dismiss_on_escape
        {
          layers.dismiss_entries.push(OverlayDismissEntry {
            anchor: OwnedElementRef::new(),
            bounds: target,
            open,
            dismiss_on_outside_click: false,
            outside_press: OutsidePress::default(),
            dismiss_on_escape,
            layer: overlays.len() + 1,
            shown: true,
          });
        }
        overlays.push(modal);
        layers.anchors.push(owner);
      }
    }
  }

  overlays
}

fn set_overlay_reuse_key(node: &mut Node, key: Option<&str>) {
  if let Some(key) = key {
    node.set_component_key(Some(key));
  }
}

fn find_element_record<'a>(index: &'a OverlayLayoutIndex, target: &OwnedElementRef) -> Option<&'a ElementLayoutRecord> {
  index
    .elements
    .iter()
    .find(|record| record.element_ref.same_handle(target))
}

fn modal_target_rect_from_index(
  spec: &ModalSpec,
  parent: ElementRect,
  viewport: Size,
  index: &OverlayLayoutIndex,
) -> Option<ElementRect> {
  match &spec.target {
    ModalTarget::Parent => Some(parent),
    ModalTarget::Root => Some(ElementRect {
      x: 0.0,
      y: 0.0,
      relative_x: 0.0,
      relative_y: 0.0,
      width: viewport.width,
      height: viewport.height,
    }),
    ModalTarget::Element(target) => find_element_record(index, target).map(|record| record.rect),
  }
}

fn build_modal_node(spec: ModalSpec, target: ElementRect) -> Node {
  let layer = spec.layer;
  let content = spec.node;
  let mut modal = Node::stack(crate::layout::StackAlignment::TopStart, vec![content])
    .hit_test(HitTestBehavior::ContentOnly)
    .absolute_positioned(
      target.x,
      target.y,
      Some(Dimension::Px(target.width)),
      Some(Dimension::Px(target.height)),
    );
  match layer {
    ModalLayer::Dialog => {
      modal.set_tag_name("Modal");
      modal.set_synthetic_role(SyntheticNodeRole::Modal);
    }
    ModalLayer::WindowChrome => {
      modal.set_tag_name("WindowChromeLayer");
      modal.set_synthetic_role(SyntheticNodeRole::WindowChromeLayer);
    }
  }
  modal
}

#[allow(clippy::too_many_arguments)]
fn build_overlay_node(
  spec: OverlaySpec,
  anchor: ElementRect,
  viewport: Size,
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  layout_engine: &LayoutEngine,
  constraints: Constraints,
  palette: crate::app::theme::ThemePalette,
  border_sizes: crate::app::theme::ThemeBorderSizes,
  spacing: crate::app::theme::ThemeSpacing,
  radii: crate::app::theme::ThemeRadii,
  caret: crate::app::theme::ThemeCaret,
  scrollbar: crate::layout::scrollbar::ScrollBarStyle,
  typography: crate::app::theme::ThemeTypography,
  theme_changed: bool,
  #[cfg(feature = "perf_profile")] profiling: &mut crate::app::profiler::producer::WindowProfiler,
) -> (Node, ElementRect) {
  let mut node = spec.node;
  if spec.match_anchor_width {
    node = node.width(Dimension::Px(anchor.width));
  }
  if spec.hit_test != HitTestBehavior::Auto {
    node = node.hit_test(spec.hit_test);
  }

  let measure_constraints = Constraints::loose(Size::new(
    constraints.max_width.min(viewport.width).max(0.0),
    constraints.max_height.min(viewport.height).max(0.0),
  ));
  let pending_runtime_layout_dirty = has_pending_layout_dirty_recursive(&node);
  let measure_node = node.clone_for_reuse();
  let measured = crate::app::profiler::producer::layout_compute(
    #[cfg(feature = "perf_profile")]
    profiling,
    || {
      layout_engine.compute(
        glyph_engine,
        &measure_node,
        measure_constraints,
        palette,
        border_sizes,
        spacing,
        radii,
        caret,
        scrollbar,
        typography,
        theme_changed,
      )
    },
  );
  if pending_runtime_layout_dirty {
    invalidate_layout_cache_recursive(&node);
  }
  let overlay_size = measured.size;
  let placement = resolve_overlay_collision(
    spec.placement,
    anchor,
    overlay_size,
    viewport,
    spec.offset_x,
    spec.offset_y,
    spec.collision,
  );
  let (x, y) = overlay_position(anchor, overlay_size, placement, spec.offset_x, spec.offset_y);
  let (x, y) = if matches!(
    spec.collision,
    CollisionStrategy::Clamp | CollisionStrategy::FlipThenClamp
  ) {
    clamp_overlay_position(x, y, overlay_size, viewport)
  } else {
    (x, y)
  };

  let bounds = ElementRect {
    x,
    y,
    relative_x: x,
    relative_y: y,
    width: overlay_size.width,
    height: overlay_size.height,
  };
  (
    node.absolute_positioned(
      x,
      y,
      Some(Dimension::Px(overlay_size.width)),
      Some(Dimension::Px(overlay_size.height)),
    ),
    bounds,
  )
}

fn invalidate_layout_cache_recursive(node: &Node) {
  node.layout_cache.invalidate();
  for child in node.children() {
    invalidate_layout_cache_recursive(child);
  }
  if let Some(spec) = node.modal_declaration() {
    invalidate_layout_cache_recursive(&spec.node);
  }
  if let Some(spec) = node.overlay_declaration() {
    invalidate_layout_cache_recursive(&spec.node);
  }
}

fn resolve_overlay_collision(
  placement: Placement,
  anchor: ElementRect,
  overlay: Size,
  viewport: Size,
  offset_x: f32,
  offset_y: f32,
  collision: CollisionStrategy,
) -> Placement {
  if !matches!(collision, CollisionStrategy::Flip | CollisionStrategy::FlipThenClamp) {
    return placement;
  }

  let (x, y) = overlay_position(anchor, overlay, placement, offset_x, offset_y);
  match placement {
    Placement::TopStart | Placement::Top | Placement::TopEnd if y < 0.0 => opposite_placement(placement),
    Placement::BottomStart | Placement::Bottom | Placement::BottomEnd if y + overlay.height > viewport.height => {
      opposite_placement(placement)
    }
    Placement::LeftStart | Placement::Left | Placement::LeftEnd if x < 0.0 => opposite_placement(placement),
    Placement::RightStart | Placement::Right | Placement::RightEnd if x + overlay.width > viewport.width => {
      opposite_placement(placement)
    }
    _ => placement,
  }
}

fn opposite_placement(placement: Placement) -> Placement {
  match placement {
    Placement::TopStart => Placement::BottomStart,
    Placement::Top => Placement::Bottom,
    Placement::TopEnd => Placement::BottomEnd,
    Placement::BottomStart => Placement::TopStart,
    Placement::Bottom => Placement::Top,
    Placement::BottomEnd => Placement::TopEnd,
    Placement::LeftStart => Placement::RightStart,
    Placement::Left => Placement::Right,
    Placement::LeftEnd => Placement::RightEnd,
    Placement::RightStart => Placement::LeftStart,
    Placement::Right => Placement::Left,
    Placement::RightEnd => Placement::LeftEnd,
  }
}

fn overlay_position(
  anchor: ElementRect,
  overlay: Size,
  placement: Placement,
  offset_x: f32,
  offset_y: f32,
) -> (f32, f32) {
  let left = anchor.x;
  let right = anchor.x + anchor.width;
  let top = anchor.y;
  let bottom = anchor.y + anchor.height;
  let center_x = anchor.x + anchor.width * 0.5;
  let center_y = anchor.y + anchor.height * 0.5;

  match placement {
    Placement::TopStart => (left + offset_x, top - overlay.height - offset_y),
    Placement::Top => (
      center_x - overlay.width * 0.5 + offset_x,
      top - overlay.height - offset_y,
    ),
    Placement::TopEnd => (right - overlay.width + offset_x, top - overlay.height - offset_y),
    Placement::BottomStart => (left + offset_x, bottom + offset_y),
    Placement::Bottom => (center_x - overlay.width * 0.5 + offset_x, bottom + offset_y),
    Placement::BottomEnd => (right - overlay.width + offset_x, bottom + offset_y),
    Placement::LeftStart => (left - overlay.width - offset_x, top + offset_y),
    Placement::Left => (
      left - overlay.width - offset_x,
      center_y - overlay.height * 0.5 + offset_y,
    ),
    Placement::LeftEnd => (left - overlay.width - offset_x, bottom - overlay.height + offset_y),
    Placement::RightStart => (right + offset_x, top + offset_y),
    Placement::Right => (right + offset_x, center_y - overlay.height * 0.5 + offset_y),
    Placement::RightEnd => (right + offset_x, bottom - overlay.height + offset_y),
  }
}

fn clamp_overlay_position(x: f32, y: f32, overlay: Size, viewport: Size) -> (f32, f32) {
  let max_x = (viewport.width - overlay.width).max(0.0);
  let max_y = (viewport.height - overlay.height).max(0.0);
  (x.clamp(0.0, max_x), y.clamp(0.0, max_y))
}

/// Close every open select; returns whether any were open.
fn close_all_open_selects(node: &Node) -> bool {
  close_open_selects_except(node, None)
}

fn close_open_selects_except(node: &Node, except: Option<NodeId>) -> bool {
  let mut changed = false;
  if let NodeKind::Select { state } = node.node_kind()
    && Some(node.node_id()) != except
    && state.is_open()
  {
    state.set_open(false);
    changed = true;
  }
  for child in node.children() {
    changed |= close_open_selects_except(child, except);
  }
  changed
}

fn scroll_axes(direction: ScrollDirection) -> &'static [ScrollAxis] {
  match direction {
    ScrollDirection::Horizontal => &[ScrollAxis::Horizontal],
    ScrollDirection::Vertical => &[ScrollAxis::Vertical],
    ScrollDirection::Both => &[ScrollAxis::Vertical, ScrollAxis::Horizontal],
  }
}

fn scroll_delta_can_be_consumed_by_hits(
  hits: &[(&Node, HitRect)],
  delta_x: f32,
  delta_y: f32,
  gesture_dy: f32,
) -> bool {
  let remaining_dx = -delta_x;
  let remaining_dy = -delta_y;

  if remaining_dx == 0.0 && remaining_dy == 0.0 {
    return false;
  }

  for (node, _) in hits {
    let Some((state, direction)) = node.scroll_container() else {
      continue;
    };
    let (dx, dy) = scroll_container_deltas(direction, remaining_dx, remaining_dy, gesture_dy);
    if scroll_axis_delta_can_be_consumed(&state, ScrollAxis::Horizontal, dx)
      || scroll_axis_delta_can_be_consumed(&state, ScrollAxis::Vertical, dy)
    {
      return true;
    }
  }

  false
}

fn scroll_delta_targets_scroll_container(
  hits: &[(&Node, HitRect)],
  delta_x: f32,
  delta_y: f32,
  gesture_dy: f32,
) -> bool {
  hits.iter().any(|(node, _)| {
    let Some((_, direction)) = node.scroll_container() else {
      return false;
    };
    scroll_container_deltas(direction, -delta_x, -delta_y, gesture_dy) != (0.0, 0.0)
  })
}

/// The part of a wheel or swipe delta a scroll container takes: the axes it
/// scrolls. A container that scrolls sideways only takes the x delta of a
/// mostly vertical gesture (`|dx| < |gesture_dy|`, the vertical delta of
/// this event or of the one just before it at the same pointer, see
/// [`Tree::wheel_gesture_vertical`]) not at all, leaving the whole delta to
/// the containers around it: a chat's slightly slanted trackpad swipe
/// scrolls the chat and nudges no code block under the pointer. A
/// deliberate sideways swipe and Shift+wheel (which the shell delivers with
/// `dy == 0`) still scroll it.
fn scroll_container_deltas(direction: ScrollDirection, delta_x: f32, delta_y: f32, gesture_dy: f32) -> (f32, f32) {
  let dx = match direction {
    ScrollDirection::Horizontal if delta_x.abs() < gesture_dy.abs() => 0.0,
    ScrollDirection::Horizontal | ScrollDirection::Both => delta_x,
    ScrollDirection::Vertical => 0.0,
  };
  let dy = match direction {
    ScrollDirection::Vertical | ScrollDirection::Both => delta_y,
    ScrollDirection::Horizontal => 0.0,
  };
  (dx, dy)
}

fn scroll_handlers_present_for_phase(hits: &[(&Node, HitRect)], phase: ScrollPhase) -> bool {
  hits.iter().any(|(node, _)| match phase {
    ScrollPhase::Start => !node.events.on_scroll_start.is_empty(),
    ScrollPhase::Scroll => !node.events.on_scroll.is_empty(),
    ScrollPhase::End => !node.events.on_scroll_end.is_empty(),
  })
}

fn scroll_axis_delta_can_be_consumed(state: &ScrollState, axis: ScrollAxis, delta: f32) -> bool {
  if delta == 0.0 {
    return false;
  }

  match axis {
    ScrollAxis::Horizontal => {
      let overflow = state.content_width() - state.viewport_width();
      let max_scroll_x = overflow.max(0.0);
      (overflow == 0.0 && delta > 0.0 && state.equal_overflow_scroll_probe_enabled())
        || (overflow > 0.0 && delta > 0.0 && state.scroll_x() < max_scroll_x)
        || (delta < 0.0 && state.scroll_x() > 0.0)
    }
    ScrollAxis::Vertical => {
      let overflow = state.content_height() - state.viewport_height();
      let max_scroll_y = overflow.max(0.0);
      (overflow == 0.0 && delta > 0.0 && state.equal_overflow_scroll_probe_enabled())
        || (overflow > 0.0 && delta > 0.0 && state.scroll_y() < max_scroll_y)
        || (delta < 0.0 && state.scroll_y() > 0.0)
    }
  }
}

#[derive(Clone)]
struct ScrollDrag {
  target_id: NodeId,
  state: ScrollState,
  axis: ScrollAxis,
  /// The press landed on the thumb, so moves drag it. A press on the track
  /// pages once and then only holds the pointer until the release.
  drags_thumb: bool,
}

#[derive(Clone)]
struct SliderDrag {
  target_id: NodeId,
  binding_id: usize,
  state: SliderState,
  x: f32,
  width: f32,
  on_finish: Vec<VoidEventHandler>,
}

impl SliderDrag {
  fn update(&self, x: f32) -> bool {
    let ratio = if self.width > 0.0 {
      (x - self.x) / self.width
    } else {
      0.0
    };
    self.state.set_drag_ratio(ratio);
    self.state.set_from_ratio(ratio)
  }

  fn finish(&self) {
    self.state.clear_drag_ratio();
    for on_finish in &self.on_finish {
      on_finish.call();
    }
  }
}

#[derive(Clone)]
struct TextSelectionDrag {
  kind: TextSelectionDragKind,
  x: f32,
  y: f32,
  transform: Transform2D,
}

/// Where a selection of the selectable texts started: the text pressed and
/// the caret in it. A Shift press extends the selection from here to the
/// pressed point, as in a browser, and a drag with Shift held goes on from
/// there.
#[derive(Clone, Copy)]
struct SelectableTextAnchor {
  node_id: NodeId,
  index: usize,
}

impl SelectableTextAnchor {
  /// The drag that extends the selection from this anchor to `caret` in the
  /// text `end_id`, selecting that range now; `None` when the anchor's text
  /// is gone or no longer selectable.
  fn extend_to(self, root: &Node, end_id: NodeId, caret: usize) -> Option<TextSelectionDragKind> {
    let (state, value) = selectable_text_state_and_value(find_node_by_id(root, self.node_id)?)?;
    set_selectable_text_range(root, self.node_id, self.index, end_id, caret);
    Some(TextSelectionDragKind::Text {
      start_id: self.node_id,
      anchor: self.index,
      state,
      value,
      preserve_existing: false,
    })
  }
}

#[derive(Clone)]
enum TextSelectionDragKind {
  Input {
    target_id: NodeId,
    state: TextInputState,
  },
  Text {
    start_id: NodeId,
    anchor: usize,
    state: TextState,
    value: String,
    preserve_existing: bool,
  },
}

impl TextSelectionDrag {
  fn update(&self, x: f32, y: f32) {
    let (local_x, local_y) = self.local_point(x, y);
    match &self.kind {
      TextSelectionDragKind::Input { state, .. } => state.update_selection_to_point(local_x - self.x, local_y - self.y),
      TextSelectionDragKind::Text { state, value, .. } => {
        state.update_selection_to_point(value, local_x - self.x, local_y - self.y)
      }
    }
  }

  fn update_with_tree(&self, root: &Node, layout: &LayoutResult, x: f32, y: f32) {
    match &self.kind {
      TextSelectionDragKind::Input { .. } => {
        let Some(state) = self.current_input_state(root) else {
          self.update(x, y);
          return;
        };
        let (local_x, local_y) = self.local_point(x, y);
        state.update_selection_to_point(local_x - self.x, local_y - self.y);
      }
      TextSelectionDragKind::Text {
        start_id,
        anchor,
        preserve_existing,
        ..
      } if !preserve_existing => {
        if let Some((node, rect)) = selectable_text_endpoint(root, layout, x, y)
          && let Some((state, value)) = selectable_text_state_and_value(node)
        {
          let caret = state.caret_index_at_point(&value, rect.local_x - rect.x, rect.local_y - rect.y);
          set_selectable_text_range(root, *start_id, *anchor, node.node_id(), caret);
        } else {
          self.update(x, y);
        }
      }
      TextSelectionDragKind::Text { .. } => self.update(x, y),
    }
  }

  fn has_selection(&self, root: Option<&Node>) -> bool {
    match &self.kind {
      TextSelectionDragKind::Input { state, .. } => root
        .and_then(|root| self.current_input_state(root))
        .unwrap_or(state)
        .has_selection(),
      TextSelectionDragKind::Text {
        state,
        value,
        preserve_existing,
        ..
      } => {
        if !preserve_existing && let Some(root) = root {
          has_selected_selectable_text(root)
        } else {
          state.has_selection(value)
        }
      }
    }
  }

  fn current_input_state<'a>(&self, root: &'a Node) -> Option<&'a TextInputState> {
    let TextSelectionDragKind::Input { target_id, .. } = &self.kind else {
      return None;
    };
    let node = find_node_by_id(root, *target_id)?;
    let NodeKind::TextInput { state, .. } = node.node_kind() else {
      return None;
    };
    Some(state)
  }

  fn local_point(&self, x: f32, y: f32) -> (f32, f32) {
    if self.transform.is_identity() {
      return (x, y);
    }

    let Some(inverse) = self.transform.inverse_affine() else {
      return (x, y);
    };
    inverse.transform_point(x, y)
  }
}

fn text_input_vertical_offset(state: &TextInputState, height: f32) -> f32 {
  if state.overflow() == TextInputOverflow::Scroll {
    ((height - state.caret_height()).max(0.0)) * 0.5
  } else {
    0.0
  }
}

type DragCallback = EventHandler<DragEvent>;
type DropCallback = EventHandler<DropEvent>;

struct ActiveDrag {
  target_id: NodeId,
  start_x: f32,
  start_y: f32,
  last_x: f32,
  last_y: f32,
  button: MouseButton,
  on_move: Vec<DragCallback>,
  on_end: Vec<DragCallback>,
  payload: Option<crate::app::events::DragPayload>,
}

impl ActiveDrag {
  fn event(&self, x: f32, y: f32, drop_result: Option<DropResult>) -> DragEvent {
    DragEvent {
      x,
      y,
      start_x: self.start_x,
      start_y: self.start_y,
      delta_x: x - self.last_x,
      delta_y: y - self.last_y,
      total_delta_x: x - self.start_x,
      total_delta_y: y - self.start_y,
      button: self.button,
      target_id: self.target_id,
      drop_result,
    }
  }
}

#[derive(Default)]
struct ClickTracker {
  pending_click: Option<PendingClick>,
}

impl ClickTracker {
  fn has_pending(&self) -> bool {
    self.pending_click.is_some()
  }

  fn pending_matches(&self, now: Instant, position: (f32, f32), button: MouseButton, target_id: NodeId) -> bool {
    self.pending_click.is_some_and(|pending| {
      pending.button == button
        && pending.target_id == target_id
        && now.duration_since(pending.time) <= DOUBLE_CLICK_INTERVAL
        && distance_squared(pending.position, position) <= DOUBLE_CLICK_DISTANCE * DOUBLE_CLICK_DISTANCE
    })
  }

  fn pending_match_for_targets(
    &self,
    now: Instant,
    position: (f32, f32),
    button: MouseButton,
    target_ids: &[NodeId],
  ) -> Option<NodeId> {
    let pending = self.pending_click?;
    if pending.button != button
      || now.duration_since(pending.time) > DOUBLE_CLICK_INTERVAL
      || distance_squared(pending.position, position) > DOUBLE_CLICK_DISTANCE * DOUBLE_CLICK_DISTANCE
    {
      return None;
    }

    target_ids
      .iter()
      .copied()
      .find(|target_id| *target_id == pending.target_id)
  }

  fn pending_is_due(&self, now: Instant) -> bool {
    self
      .pending_click
      .is_some_and(|pending| now.duration_since(pending.time) > DOUBLE_CLICK_INTERVAL)
  }

  /// Holds back a click of the input being delivered now, with its source.
  fn set_pending(
    &mut self,
    now: Instant,
    position: (f32, f32),
    button: MouseButton,
    modifiers: MouseModifiers,
    target_id: NodeId,
  ) {
    self.pending_click = Some(PendingClick {
      time: now,
      position,
      button,
      modifiers,
      target_id,
      source: input_source(),
    });
  }

  fn take_pending(&mut self) -> Option<PendingClick> {
    self.pending_click.take()
  }
}

#[derive(Clone, Copy)]
struct PendingClick {
  time: Instant,
  position: (f32, f32),
  button: MouseButton,
  modifiers: MouseModifiers,
  target_id: NodeId,
  /// Where the input that made the click came from.
  source: InputSource,
}

struct ClickPress {
  position: (f32, f32),
  button: MouseButton,
  target_ids: Vec<NodeId>,
  transient_root: Option<NodeId>,
}

#[derive(Clone, Copy)]
enum ClickDispatchTarget {
  Node(NodeId),
  CurrentHit,
}

#[derive(Clone, Copy, Default)]
struct MouseModifiers {
  shift: bool,
  ctrl: bool,
  alt: bool,
  meta: bool,
}

impl MouseModifiers {
  /// The click a key with these modifiers makes on `target_id`: Enter on a
  /// focused link, Enter or Space on a focused button.
  fn keyboard_click(self, target_id: NodeId) -> MouseEvent {
    MouseEvent {
      x: 0.0,
      y: 0.0,
      button: MouseButton::Left,
      kind: MouseEventKind::Click,
      shift: self.shift,
      ctrl: self.ctrl,
      alt: self.alt,
      meta: self.meta,
      from_keyboard: true,
      target_id,
      control: EventControl::new(),
    }
  }
}

#[derive(Default)]
struct TextClickTracker {
  previous: Option<TextClick>,
}

impl TextClickTracker {
  fn record(&mut self, now: Instant, position: (f32, f32), button: MouseButton, target_id: NodeId) -> u8 {
    let count = self
      .previous
      .filter(|previous| {
        previous.button == button
          && previous.target_id == target_id
          && now.duration_since(previous.time) <= DOUBLE_CLICK_INTERVAL
          && distance_squared(previous.position, position) <= DOUBLE_CLICK_DISTANCE * DOUBLE_CLICK_DISTANCE
      })
      .map(|previous| (previous.count + 1).min(3))
      .unwrap_or(1);

    self.previous = Some(TextClick {
      time: now,
      position,
      button,
      target_id,
      count,
    });

    count
  }
}

#[derive(Clone, Copy)]
struct TextClick {
  time: Instant,
  position: (f32, f32),
  button: MouseButton,
  target_id: NodeId,
  count: u8,
}

#[derive(Clone, Copy)]
struct SuppressedClick {
  time: Instant,
  position: (f32, f32),
  button: MouseButton,
}

fn distance_squared(a: (f32, f32), b: (f32, f32)) -> f32 {
  let dx = a.0 - b.0;
  let dy = a.1 - b.1;
  dx * dx + dy * dy
}

impl Drop for Tree {
  fn drop(&mut self) {
    self.clear_focus();
    #[cfg(feature = "canvas")]
    if let Some(root) = &self.root {
      detach_canvas_recursive(root);
    }
    if let Some(component) = self.root_component.take() {
      component.on_unmounted();
    }
  }
}

fn find_element_recursive(
  node: &mut Node,
  layout: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  parent_x: f32,
  parent_y: f32,
  predicate: &impl for<'b> Fn(ElementRef<'b>) -> bool,
) -> Option<OwnedElementRef> {
  if layout.dropped {
    return find_element_in_dropped(node, abs_x, abs_y, parent_x, parent_y, predicate);
  }
  let rect = ElementRect {
    x: abs_x,
    y: abs_y,
    relative_x: abs_x - parent_x,
    relative_y: abs_y - parent_y,
    width: layout.size.width,
    height: layout.size.height,
  };
  if predicate(ElementRef::new(node)) {
    return Some(found_element_ref(node, rect));
  }

  for (child_layout, child_node) in layout.children.iter().zip(node.children.iter_mut()) {
    if let Some(found) = find_element_recursive(
      child_node,
      &child_layout.result,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      abs_x,
      abs_y,
      predicate,
    ) {
      return Some(found);
    }
  }

  None
}

/// [`find_element_recursive`] inside a subtree its Row/Column dropped: every
/// element there has a zero-size rect at the dropped child's place, as
/// [`collapse_dropped_element_refs`] reports it.
fn find_element_in_dropped(
  node: &mut Node,
  abs_x: f32,
  abs_y: f32,
  parent_x: f32,
  parent_y: f32,
  predicate: &impl for<'b> Fn(ElementRef<'b>) -> bool,
) -> Option<OwnedElementRef> {
  if predicate(ElementRef::new(node)) {
    let rect = ElementRect {
      x: abs_x,
      y: abs_y,
      relative_x: abs_x - parent_x,
      relative_y: abs_y - parent_y,
      width: 0.0,
      height: 0.0,
    };
    return Some(found_element_ref(node, rect));
  }
  node
    .children
    .iter_mut()
    .find_map(|child| find_element_in_dropped(child, abs_x, abs_y, abs_x, abs_y, predicate))
}

fn found_element_ref(node: &mut Node, rect: ElementRect) -> OwnedElementRef {
  let element_ref = node.element_ref_handle();
  #[cfg(feature = "canvas")]
  if let Some(canvas) = node.canvas_handle() {
    element_ref.bind_canvas(&canvas);
  }
  element_ref.update(
    rect.x,
    rect.y,
    rect.relative_x,
    rect.relative_y,
    rect.width,
    rect.height,
  );
  element_ref
}

fn find_by_element_id<'a>(node: &'a Node, id: &str) -> Option<&'a Node> {
  if node.element_id() == Some(id) {
    return Some(node);
  }
  node.children().iter().find_map(|child| find_by_element_id(child, id))
}

#[cfg(debug_assertions)]
fn count_element_id_matches(node: &Node, id: &str) -> usize {
  (node.element_id() == Some(id)) as usize
    + node
      .children()
      .iter()
      .map(|child| count_element_id_matches(child, id))
      .sum::<usize>()
}

fn collect_by_class<'a>(node: &'a Node, class: &str, found: &mut Vec<ElementRef<'a>>) {
  if node.has_class(class) {
    found.push(ElementRef::new(node));
  }
  for child in node.children() {
    collect_by_class(child, class, found);
  }
}

fn find_node_by_id_mut(node: &mut Node, id: NodeId) -> Option<&mut Node> {
  if node.node_id() == id {
    return Some(node);
  }
  node
    .children
    .iter_mut()
    .find_map(|child| find_node_by_id_mut(child, id))
}

/// Short-lived mutation handle for a node found via
/// [`Tree::get_element_by_id_mut`].
///
/// Writes go directly into the live node (browser style): in trees with a
/// static root they are permanent, in component trees they last until the
/// owning component re-renders — durable state belongs in signals. Value
/// operations on the typed sub-handles ([`ElementHandle::as_text_input`]
/// etc.) write signal-backed widget state and therefore DO survive
/// re-renders.
pub struct ElementHandle<'t> {
  tree: &'t mut Tree,
  node_id: NodeId,
}

impl<'t> ElementHandle<'t> {
  fn node(&self) -> Option<&Node> {
    self
      .tree
      .root
      .as_ref()
      .and_then(|root| find_node_by_id(root, self.node_id))
  }

  fn node_mut(&mut self) -> Option<&mut Node> {
    self
      .tree
      .root
      .as_mut()
      .and_then(|root| find_node_by_id_mut(root, self.node_id))
  }

  fn invalidate_render(&mut self) {
    self.tree.needs_redraw = true;
    self.tree.cached_render_list = None;
  }

  pub fn node_id(&self) -> NodeId {
    self.node_id
  }

  pub fn id(&self) -> Option<&str> {
    self.node()?.element_id()
  }

  pub fn tag_name(&self) -> Option<&str> {
    self.node().map(Node::tag_name)
  }

  pub fn text_content(&self) -> Option<&str> {
    self.node()?.text_content()
  }

  pub fn has_class(&self, class: &str) -> bool {
    self.node().is_some_and(|node| node.has_class(class))
  }

  /// Appends a class, deduplicating like the DOM's `classList.add`.
  /// Classes only affect lookup — there is no selector-based styling.
  pub fn add_class(&mut self, class: impl Into<Arc<str>>) {
    if let Some(node) = self.node_mut() {
      node.push_class(class.into());
    }
  }

  pub fn remove_class(&mut self, class: &str) {
    if let Some(node) = self.node_mut() {
      node.remove_class(class);
    }
  }

  /// DOM `classList.toggle`: returns whether the class is present afterwards.
  pub fn toggle_class(&mut self, class: &str) -> bool {
    let Some(node) = self.node_mut() else {
      return false;
    };
    if node.has_class(class) {
      node.remove_class(class);
      false
    } else {
      node.push_class(Arc::from(class));
      true
    }
  }

  /// The node's absolute bounds from the most recent layout pass, or `None`
  /// before the first pass.
  pub fn bounds(&self) -> Option<ElementRect> {
    let root = self.tree.root.as_ref()?;
    let layout = self.tree.last_layout.as_ref()?;
    let path = find_path_by_id(root, self.node_id)?;
    let mut result: &LayoutResult = layout;
    let (mut abs_x, mut abs_y, mut rel_x, mut rel_y) = (0.0, 0.0, 0.0, 0.0);
    for (depth, &index) in path.iter().enumerate() {
      let child = result.children.get(index)?;
      rel_x = child.offset.x;
      rel_y = child.offset.y;
      abs_x += rel_x;
      abs_y += rel_y;
      result = child.result.as_ref();
      if result.dropped {
        // Inside a dropped subtree every element sits, at zero size, at the
        // dropped child's place in its line.
        if depth + 1 < path.len() {
          (rel_x, rel_y) = (0.0, 0.0);
        }
        break;
      }
    }
    Some(ElementRect {
      x: abs_x,
      y: abs_y,
      relative_x: rel_x,
      relative_y: rel_y,
      width: result.size.width,
      height: result.size.height,
    })
  }

  pub fn set_background(&mut self, color: impl Into<crate::node::BackgroundColor>) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::background(node, color);
    self.invalidate_render();
  }

  pub fn set_background_gradient(&mut self, gradient: impl Into<crate::node::Gradient>) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::background_gradient(node, gradient);
    self.invalidate_render();
  }

  pub fn set_corner_radius(&mut self, radius: impl Into<crate::node::RadiusValue>) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::corner_radius(node, radius);
    self.invalidate_render();
  }

  pub fn set_box_shadow(&mut self, shadow: impl Into<crate::node::BoxShadowValue>) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::box_shadow(node, shadow);
    self.invalidate_render();
  }

  pub fn set_border(&mut self, border: crate::node::border::Border) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::border(node, border);
    self.invalidate_render();
  }

  pub fn set_opacity(&mut self, opacity: f32) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::opacity(node, opacity);
    self.invalidate_render();
  }

  pub fn set_text_color(&mut self, color: impl Into<crate::node::TextColor>) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::text_color(node, color);
    self.invalidate_render();
  }

  /// Replaces the node's text content. On `TextInput` nodes use
  /// [`ElementHandle::as_text_input`] + `set_value` instead — the input's
  /// signal-backed value overwrites raw text content on the next pass.
  pub fn set_text_content(&mut self, text: impl Into<String>) {
    let Some(node) = self.node_mut() else { return };
    node.text_content.set(Some(text.into()));
    // The pass' fast path only scans persistent dirty flags, so a live
    // mutation must leave one behind (builder-time invalidation clears them).
    node.layout_cache.mark_local_dirty();
    self.invalidate_render();
  }

  pub fn set_size(
    &mut self,
    width: impl Into<crate::node::dimension::Dimension>,
    height: impl Into<crate::node::dimension::Dimension>,
  ) {
    let Some(node) = self.node_mut() else { return };
    NodeUpdate::size(node, width, height);
    // See `set_text_content` — the fast path needs a persistent dirty flag.
    node.layout_cache.mark_local_dirty();
    self.invalidate_render();
  }

  /// DOM `el.click()`: invokes this node's own `on_click` handlers with a
  /// synthesized left-button [`MouseEvent`] at the node's bounds center
  /// (zeros before the first layout pass). No hit-testing — this works even
  /// when the node is occluded. Focusable nodes are focused first and submit
  /// buttons submit their form, like a real browser click. For
  /// pointer-fidelity clicks use [`Tree::mouse_down`] / [`Tree::mouse_up`].
  pub fn click(&mut self) {
    let center = self.bounds().map(|bounds| bounds.center()).unwrap_or((0.0, 0.0));
    let Some(node) = self.node() else { return };
    let focusable = node.is_focusable();
    let button_kind = node.button_kind_value();
    let handlers = node.events.on_click.clone();
    self.tree.set_input_modality(focus_visible::InputModality::Pointer);
    if focusable {
      self.tree.focus_node(FocusTarget {
        input_id: self.node_id,
        event_id: self.node_id,
      });
    }
    let event = MouseEvent {
      x: center.0,
      y: center.1,
      button: MouseButton::Left,
      kind: MouseEventKind::Click,
      shift: false,
      ctrl: false,
      alt: false,
      meta: false,
      from_keyboard: false,
      target_id: self.node_id,
      control: EventControl::new(),
    };
    for handler in handlers {
      handler.call(&event);
    }
    #[cfg(feature = "form")]
    if button_kind == Some(ButtonKind::Submit) {
      self.tree.submit_nearest_form_for_node_id(self.node_id);
    }
    #[cfg(not(feature = "form"))]
    let _ = button_kind;
    self.invalidate_render();
  }

  /// Focuses this node through the tree's focus machinery (fires `on_focus`
  /// / `on_blur` handlers declared on the node itself), then scrolls it into
  /// view in the next pass like [`ElementHandle::scroll_into_view`].
  pub fn focus(&mut self) {
    self.tree.focus_node(FocusTarget {
      input_id: self.node_id,
      event_id: self.node_id,
    });
    if self.tree.focused_node == Some(self.node_id) {
      self.tree.request_scroll_into_view(self.node_id);
    }
    self.invalidate_render();
  }

  /// Scrolls this node into view in the next pass: every scroll container
  /// around it scrolls by the smallest amount that shows it, innermost
  /// first, on each axis the container scrolls (the web's
  /// `scrollIntoView({ block: "nearest", inline: "nearest" })`). The app-side
  /// counterpart is [`Ctx::scroll_into_view`].
  pub fn scroll_into_view(&mut self) {
    self.tree.request_scroll_into_view(self.node_id);
    self.invalidate_render();
  }

  /// Removes focus if this node currently holds it; no-op otherwise.
  pub fn blur(&mut self) {
    if self.tree.focused_node == Some(self.node_id) {
      self.tree.blur_focus();
    }
  }

  /// DOM-downcast style: `Some` when the node is a `TextInput`.
  pub fn as_text_input(self) -> Option<TextInputHandle<'t>> {
    matches!(self.node()?.node_kind(), NodeKind::TextInput { .. }).then_some(TextInputHandle { inner: self })
  }

  /// DOM-downcast style: `Some` when the node is a `Checkbox`.
  pub fn as_checkbox(self) -> Option<CheckboxHandle<'t>> {
    matches!(self.node()?.node_kind(), NodeKind::Checkbox { .. }).then_some(CheckboxHandle { inner: self })
  }

  /// DOM-downcast style: `Some` when the node is a `Slider`.
  pub fn as_slider(self) -> Option<SliderHandle<'t>> {
    matches!(self.node()?.node_kind(), NodeKind::Slider { .. }).then_some(SliderHandle { inner: self })
  }

  /// DOM-downcast style: `Some` when the node is a `Select`.
  pub fn as_select(self) -> Option<SelectHandle<'t>> {
    matches!(self.node()?.node_kind(), NodeKind::Select { .. }).then_some(SelectHandle { inner: self })
  }
}

/// Typed interaction handle for `TextInput` nodes; see
/// [`ElementHandle::as_text_input`]. Value writes are signal-backed and
/// survive re-renders.
pub struct TextInputHandle<'t> {
  inner: ElementHandle<'t>,
}

impl TextInputHandle<'_> {
  pub fn mask(&self) -> Option<char> {
    self.state().and_then(TextInputState::mask)
  }

  pub fn is_masked(&self) -> bool {
    self.state().is_some_and(TextInputState::is_masked)
  }

  fn state(&self) -> Option<&TextInputState> {
    match self.inner.node()?.node_kind() {
      NodeKind::TextInput { state, .. } => Some(state),
      _ => None,
    }
  }

  pub fn value(&self) -> String {
    self.state().map(TextInputState::value).unwrap_or_default()
  }

  /// The input method composition the input shows at its caret, if one is
  /// in progress. It is not part of [`value`](Self::value) until committed.
  pub fn composition(&self) -> Option<String> {
    self.state().and_then(TextInputState::composition_text)
  }

  /// DOM `el.value = x` semantics: writes the backing signal and clamps
  /// caret/selection, but does NOT fire `on_input` handlers.
  pub fn set_value(&mut self, value: impl Into<String>) {
    if let Some(state) = self.state() {
      state.set_value(value);
    }
    self.inner.invalidate_render();
  }

  pub fn select_all(&mut self) {
    if let Some(state) = self.state() {
      state.select_all();
    }
    self.inner.invalidate_render();
  }

  /// The selected part of the value, as Ctrl+C copies it; `None` when the
  /// selection is empty.
  pub fn selected_text(&self) -> Option<String> {
    self.state().and_then(TextInputState::selected_text)
  }

  pub fn placeholder(&self) -> Option<String> {
    self.state().and_then(TextInputState::placeholder)
  }

  pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
    if let Some(state) = self.state() {
      state.set_placeholder(placeholder);
    }
    self.inner.invalidate_render();
  }

  pub fn focus(&mut self) {
    self.inner.focus();
  }
}

/// Typed interaction handle for `Checkbox` nodes; see
/// [`ElementHandle::as_checkbox`].
pub struct CheckboxHandle<'t> {
  inner: ElementHandle<'t>,
}

impl CheckboxHandle<'_> {
  fn state(&self) -> Option<&CheckboxState> {
    match self.inner.node()?.node_kind() {
      NodeKind::Checkbox { state } => Some(state),
      _ => None,
    }
  }

  pub fn is_checked(&self) -> bool {
    self.state().is_some_and(CheckboxState::is_checked)
  }

  pub fn toggle(&mut self) {
    if let Some(state) = self.state() {
      state.toggle();
    }
    self.inner.invalidate_render();
  }

  pub fn set_checked(&mut self, checked: bool) {
    if let Some(state) = self.state() {
      state.set_checked(checked);
    }
    self.inner.invalidate_render();
  }
}

/// Typed interaction handle for `Slider` nodes; see
/// [`ElementHandle::as_slider`].
pub struct SliderHandle<'t> {
  inner: ElementHandle<'t>,
}

impl SliderHandle<'_> {
  fn state(&self) -> Option<&SliderState> {
    match self.inner.node()?.node_kind() {
      NodeKind::Slider { state } => Some(state),
      _ => None,
    }
  }

  pub fn value(&self) -> f32 {
    self.state().map(SliderState::value_f32).unwrap_or_default()
  }

  /// Sets the value from a 0..=1 ratio along the track, snapping to the
  /// slider's step.
  pub fn set_from_ratio(&mut self, ratio: f32) {
    if let Some(state) = self.state() {
      state.set_from_ratio(ratio);
    }
    self.inner.invalidate_render();
  }

  /// Steps the value by `delta` slider steps (keyboard-arrow semantics).
  pub fn nudge(&mut self, delta: i32) {
    if let Some(state) = self.state() {
      state.nudge(delta);
    }
    self.inner.invalidate_render();
  }

  pub fn set_range(&mut self, min: i32, max: i32) {
    if let Some(state) = self.state() {
      state.set_range(min, max);
    }
    self.inner.invalidate_render();
  }

  pub fn set_range_f32(&mut self, min: f32, max: f32) {
    if let Some(state) = self.state() {
      state.set_range_f32(min, max);
    }
    self.inner.invalidate_render();
  }

  pub fn set_step(&mut self, step: f32) {
    if let Some(state) = self.state() {
      state.set_step(step);
    }
    self.inner.invalidate_render();
  }
}

/// Typed interaction handle for `Select` nodes; see
/// [`ElementHandle::as_select`].
pub struct SelectHandle<'t> {
  inner: ElementHandle<'t>,
}

impl SelectHandle<'_> {
  fn state(&self) -> Option<&SelectState> {
    match self.inner.node()?.node_kind() {
      NodeKind::Select { state } => Some(state),
      _ => None,
    }
  }

  pub fn is_open(&self) -> bool {
    self.state().is_some_and(SelectState::is_open)
  }

  pub fn set_open(&mut self, open: bool) {
    if let Some(state) = self.state() {
      state.set_open(open);
    }
    self.inner.invalidate_render();
  }

  pub fn toggle_open(&mut self) {
    if let Some(state) = self.state() {
      state.toggle_open();
    }
    self.inner.invalidate_render();
  }

  /// Commits option `index`: fires the select's change callback and, for
  /// single-select, closes the menu.
  pub fn commit(&mut self, index: usize) {
    if let Some(state) = self.state() {
      state.commit(index);
    }
    self.inner.invalidate_render();
  }

  pub fn labels(&self) -> Vec<Arc<str>> {
    self.state().map(SelectState::labels).unwrap_or_default()
  }

  pub fn selected_labels(&self) -> Vec<Arc<str>> {
    self.state().map(SelectState::selected_labels).unwrap_or_default()
  }

  pub fn is_selected(&self, index: usize) -> bool {
    self.state().is_some_and(|state| state.is_selected(index))
  }
}

/// Post-layout invariant probe: every scroll container's child offset in the
/// final layout must equal `-scroll` from its own state. A mismatch means a
/// stale cached layout leaked through the incremental relayout paths.
fn verify_scroll_offsets(node: &Node, result: &crate::layout::layout_result::LayoutResult) {
  if let LayoutKind::ScrollModifier { state, .. } = node.layout_kind() {
    if let Some(child) = result.children.first() {
      let expected_y = -state.scroll_y();
      let expected_x = -state.scroll_x();
      if (child.offset.y - expected_y).abs() > 0.5 || (child.offset.x - expected_x).abs() > 0.5 {
        tracing::warn!(
          target: "lurq::layout",
          "stale scroll offset in final layout: offset=({:.1},{:.1}) expected=({expected_x:.1},{expected_y:.1}) content_h={:.1}",
          child.offset.x,
          child.offset.y,
          child.result.size.height,
        );
      }
    }
  }
  // A node with a large fixed pixel height (a virtualized list's spacer) must
  // be laid at exactly that height — a mismatch means the final layout served
  // a stale cached result for this subtree. Gated to giant spacers so
  // legitimately clamped fixed-size controls don't produce noise.
  if let Some(crate::node::dimension::Dimension::Px(requested)) = node.frame.height {
    if requested > 1000.0 && (result.size.height - requested).abs() > 0.5 {
      tracing::warn!(
        target: "lurq::layout",
        "stale child geometry in final layout: laid_h={:.1} requested_h={requested:.1} (tag={} local_dirty={} descendant_dirty={} has_cached={})",
        result.size.height,
        node.tag_name(),
        node.layout_cache.is_local_dirty(),
        node.layout_cache.is_descendant_dirty(),
        node.layout_cache.has_cached_result(),
      );
    }
  }
  for (child_layout, child) in result.children.iter().zip(node.children()) {
    verify_scroll_offsets(child, &child_layout.result);
  }
}

/// Find the scroll node sharing `state`'s underlying scroll state — the
/// stable identity of a scroll container across retained-tree rebuilds.
fn find_scroll_node_id_by_state(node: &Node, state: &ScrollState) -> Option<NodeId> {
  if let Some((node_state, _)) = node.scroll_container() {
    if node_state.ptr_eq(state) {
      return Some(node.node_id());
    }
  }
  for child in node.children() {
    if let Some(found) = find_scroll_node_id_by_state(child, state) {
      return Some(found);
    }
  }
  None
}

/// Dirty the layout of node `id` plus the descendant flags on its ancestor
/// chain, mirroring what the wheel path does through its hit list.
fn mark_scroll_layout_dirty(node: &Node, id: NodeId) -> bool {
  if node.node_id() == id {
    node.layout_cache.mark_local_dirty();
    node.layout_cache.mark_descendant_dirty();
    return true;
  }
  for child in node.children() {
    if mark_scroll_layout_dirty(child, id) {
      node.layout_cache.mark_descendant_dirty();
      return true;
    }
  }
  false
}

fn find_node_by_id(node: &Node, id: NodeId) -> Option<&Node> {
  if node.node_id() == id {
    return Some(node);
  }

  for child in node.children() {
    if let Some(found) = find_node_by_id(child, id) {
      return Some(found);
    }
  }

  None
}

fn find_node_by_path<'a>(node: &'a Node, path: &[usize]) -> Option<&'a Node> {
  let mut current = node;
  for &index in path {
    current = current.children().get(index)?;
  }
  Some(current)
}

#[cfg(feature = "screenshot")]
impl Tree {
  fn take_pending_frame_capture(&mut self) -> Option<RenderFrameCapture> {
    let capture = self.take_pending_screenshot();
    #[cfg(feature = "devtools")]
    {
      capture.or_else(|| self.take_pending_devtools_frame_capture())
    }
    #[cfg(not(feature = "devtools"))]
    {
      capture
    }
  }

  fn take_pending_screenshot(&mut self) -> Option<RenderFrameCapture> {
    let PendingScreenshot {
      target,
      region,
      redact_sensitive,
    } = self.pending_screenshot.take()?;
    let Some(region) = region else {
      return Some(RenderFrameCapture {
        x: 0,
        y: 0,
        width: self.viewport_physical.width.round().max(1.0) as u32,
        height: self.viewport_physical.height.round().max(1.0) as u32,
        target: self.inspection_target(target, redact_sensitive, (0, 0)),
        window_clip: self.screenshot_window_clip(),
      });
    };
    let Some((x, y, width, height)) = crate::app::frame_capture::physical_capture_region(
      region,
      self.scale_factor(),
      self.viewport_physical.width,
      self.viewport_physical.height,
    ) else {
      target.fail(format!(
        "screenshot region {region:?} has no visible area in the viewport"
      ));
      return None;
    };
    // A region crop never touches the window corners, so no corner clip.
    Some(RenderFrameCapture {
      x,
      y,
      width,
      height,
      target: self.inspection_target(target, redact_sensitive, (x, y)),
      window_clip: None,
    })
  }

  /// `target`, painting over the last render list's sensitive text when the
  /// capture is made for inspection. `origin` is the capture's top-left
  /// window pixel.
  fn inspection_target(
    &self,
    target: crate::app::render_engine::RenderCaptureTarget,
    redact_sensitive: bool,
    origin: (u32, u32),
  ) -> crate::app::render_engine::RenderCaptureTarget {
    if !redact_sensitive {
      return target;
    }
    crate::app::capture_redaction::redacting_target(target, origin, self.capture_redactions.clone())
  }

  fn drop_unsupported_screenshot(&mut self) {
    if let Some(pending) = self.pending_screenshot.take() {
      pending.target.fail("render engine does not support frame capture");
    }
  }

  fn screenshot_window_clip(&self) -> Option<RenderFrameCaptureWindowClip> {
    let info = self.window.info();
    if info.is_decorated || info.is_maximized || info.is_full_screen {
      return None;
    }

    use crate::app::WindowCornerRadius;
    let logical_radius = match self.window.corner_radius() {
      WindowCornerRadius::Rounded => 10.0,
      WindowCornerRadius::RoundedSmall => 4.0,
      WindowCornerRadius::Default | WindowCornerRadius::None => return None,
    };
    let radius = logical_radius * self.scale_factor();
    (radius > 0.0).then_some(RenderFrameCaptureWindowClip {
      width: self.viewport_physical.width,
      height: self.viewport_physical.height,
      radii: [radius; 4],
    })
  }
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy)]
struct DevtoolsOverlayRect {
  x: f32,
  y: f32,
  width: f32,
  height: f32,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy)]
struct DevtoolsOverlayTarget {
  outer: DevtoolsOverlayRect,
  inner: Option<DevtoolsOverlayRect>,
}

#[cfg(feature = "devtools")]
impl Tree {
  fn devtools_overlay_target(&self, root: &Node, layout: &LayoutResult) -> Option<DevtoolsOverlayTarget> {
    let path = self.debug_overlay_node_path.as_deref()?;
    devtools_overlay_target_at_path(root, layout, path, 0.0, 0.0)
  }

  fn take_pending_devtools_frame_capture(&mut self) -> Option<RenderFrameCapture> {
    let Some(mut request) = self.devtools_state.screenshot_request.lock().unwrap().take() else {
      return None;
    };

    let Some(bounds) = self.devtools_screenshot_bounds(&request.node_path) else {
      if request.attempts < 2 {
        request.attempts += 1;
        *self.devtools_state.screenshot_request.lock().unwrap() = Some(request);
        self.needs_redraw = true;
      } else {
        tracing::warn!(
          "could not resolve selected devtools node path for screenshot to {}",
          request.output_path.display()
        );
      }
      return None;
    };

    Some(RenderFrameCapture {
      x: bounds.x,
      y: bounds.y,
      width: bounds.width,
      height: bounds.height,
      target: self.inspection_target(
        crate::app::render_engine::RenderCaptureTarget::Path(request.output_path),
        true,
        (bounds.x, bounds.y),
      ),
      window_clip: bounds.window_clip.map(|clip| RenderFrameCaptureWindowClip {
        width: clip.width,
        height: clip.height,
        radii: clip.radii,
      }),
    })
  }

  #[cfg(feature = "raster")]
  fn save_pending_devtools_screenshot(
    &mut self,
    clear_color: Color,
    rects: &[RectCmd],
    glyphs: &[GlyphCmd],
    images: &[crate::images::ImageCmd],
    layers: &[crate::layout::opacity_layer::LayerCmd],
    atlas: &crate::layout::render_list::GlyphAtlas,
  ) {
    let Some(mut request) = self.devtools_state.screenshot_request.lock().unwrap().take() else {
      return;
    };

    let Some(bounds) = self.devtools_screenshot_bounds(&request.node_path) else {
      if request.attempts < 2 {
        request.attempts += 1;
        *self.devtools_state.screenshot_request.lock().unwrap() = Some(request);
        self.needs_redraw = true;
      } else {
        tracing::warn!(
          "could not resolve selected devtools node path for screenshot to {}",
          request.output_path.display()
        );
      }
      return;
    };

    let output_path = request.output_path;
    let rects = rects.to_vec();
    let glyphs = glyphs.to_vec();
    let images = images.to_vec();
    let layers = layers.to_vec();
    let atlas = atlas.clone();
    let redactions = self.capture_redactions.clone();
    std::thread::spawn(move || {
      if let Err(error) = save_devtools_screenshot(
        &output_path,
        bounds,
        clear_color,
        &rects,
        &glyphs,
        &images,
        &layers,
        &atlas,
        &redactions,
      ) {
        tracing::warn!(
          "failed to save devtools node screenshot to {}: {error}",
          output_path.display()
        );
      } else {
        tracing::info!("Saved screenshot here: {}", output_path.display());
      }
    });
  }

  #[cfg(not(feature = "raster"))]
  fn save_pending_devtools_screenshot(
    &mut self,
    clear_color: Color,
    rects: &[RectCmd],
    glyphs: &[GlyphCmd],
    layers: &[crate::layout::opacity_layer::LayerCmd],
    atlas: &crate::layout::render_list::GlyphAtlas,
  ) {
    let Some(mut request) = self.devtools_state.screenshot_request.lock().unwrap().take() else {
      return;
    };

    let Some(bounds) = self.devtools_screenshot_bounds(&request.node_path) else {
      if request.attempts < 2 {
        request.attempts += 1;
        *self.devtools_state.screenshot_request.lock().unwrap() = Some(request);
        self.needs_redraw = true;
      } else {
        tracing::warn!(
          "could not resolve selected devtools node path for screenshot to {}",
          request.output_path.display()
        );
      }
      return;
    };

    let output_path = request.output_path;
    let rects = rects.to_vec();
    let glyphs = glyphs.to_vec();
    let layers = layers.to_vec();
    let atlas = atlas.clone();
    let redactions = self.capture_redactions.clone();
    std::thread::spawn(move || {
      if let Err(error) = save_devtools_screenshot(
        &output_path,
        bounds,
        clear_color,
        &rects,
        &glyphs,
        &layers,
        &atlas,
        &redactions,
      ) {
        tracing::warn!(
          "failed to save devtools node screenshot to {}: {error}",
          output_path.display()
        );
      } else {
        tracing::info!("Saved screenshot here: {}", output_path.display());
      }
    });
  }

  fn devtools_screenshot_bounds(&self, path: &[usize]) -> Option<DevtoolsScreenshotBounds> {
    let root = self.root.as_ref()?;
    let layout = self.last_layout.as_ref()?;
    let target = devtools_overlay_target_at_path(root, layout, path, 0.0, 0.0)?.outer;
    let scale = self.scale_factor();
    let left = (target.x * scale).floor().max(0.0);
    let top = (target.y * scale).floor().max(0.0);
    let right = ((target.x + target.width) * scale)
      .ceil()
      .min(self.viewport_physical.width.max(0.0));
    let bottom = ((target.y + target.height) * scale)
      .ceil()
      .min(self.viewport_physical.height.max(0.0));
    if right <= left || bottom <= top {
      return None;
    }

    Some(DevtoolsScreenshotBounds {
      x: left as u32,
      y: top as u32,
      width: (right - left).max(1.0) as u32,
      height: (bottom - top).max(1.0) as u32,
      window_clip: self.devtools_screenshot_window_clip(),
    })
  }

  fn devtools_screenshot_window_clip(&self) -> Option<DevtoolsWindowClip> {
    self.screenshot_window_clip().map(|clip| DevtoolsWindowClip {
      width: clip.width,
      height: clip.height,
      radii: clip.radii,
    })
  }
}

#[cfg(feature = "devtools")]
fn devtools_overlay_target_at_path(
  node: &Node,
  layout: &LayoutResult,
  path: &[usize],
  abs_x: f32,
  abs_y: f32,
) -> Option<DevtoolsOverlayTarget> {
  if let Some((&index, rest)) = path.split_first() {
    let child_layout = layout.children.get(index)?;
    let child_node = node.children().get(index)?;
    return devtools_overlay_target_at_path(
      child_node,
      &child_layout.result,
      rest,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
    );
  }

  let outer = DevtoolsOverlayRect {
    x: abs_x,
    y: abs_y,
    width: layout.size.width,
    height: layout.size.height,
  };
  let inner = if node.padding != crate::node::padding::Padding::default() {
    layout.children.first().map(|child| DevtoolsOverlayRect {
      x: abs_x + child.offset.x,
      y: abs_y + child.offset.y,
      width: child.result.size.width,
      height: child.result.size.height,
    })
  } else {
    None
  };

  Some(DevtoolsOverlayTarget { outer, inner })
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy)]
struct DevtoolsScreenshotBounds {
  x: u32,
  y: u32,
  width: u32,
  height: u32,
  window_clip: Option<DevtoolsWindowClip>,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy)]
struct DevtoolsWindowClip {
  width: f32,
  height: f32,
  radii: [f32; 4],
}

#[cfg(feature = "devtools")]
enum DevtoolsScreenshotDraw {
  Rect(usize),
  Glyph(usize),
  #[cfg(feature = "raster")]
  Image(usize),
}

#[cfg(feature = "devtools")]
fn save_devtools_screenshot(
  output_path: &std::path::Path,
  bounds: DevtoolsScreenshotBounds,
  clear_color: Color,
  rects: &[RectCmd],
  glyphs: &[GlyphCmd],
  #[cfg(feature = "raster")] images: &[crate::images::ImageCmd],
  layers: &[crate::layout::opacity_layer::LayerCmd],
  atlas: &crate::layout::render_list::GlyphAtlas,
  redactions: &[crate::app::capture_redaction::RedactedArea],
) -> Result<(), image::ImageError> {
  if let Some(parent) = output_path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
    std::fs::create_dir_all(parent).map_err(image::ImageError::IoError)?;
  }

  let mut pixels = vec![0_u8; bounds.width as usize * bounds.height as usize * 4];
  for y in 0..bounds.height {
    for x in 0..bounds.width {
      let index = (y * bounds.width + x) as usize * 4;
      let world_x = bounds.x as f32 + x as f32 + 0.5;
      let world_y = bounds.y as f32 + y as f32 + 0.5;
      if screenshot_window_clip_contains(bounds, world_x, world_y) {
        pixels[index] = clear_color.r();
        pixels[index + 1] = clear_color.g();
        pixels[index + 2] = clear_color.b();
        pixels[index + 3] = clear_color.a();
      }
    }
  }

  let draw_capacity = rects.len() + glyphs.len() + {
    #[cfg(feature = "raster")]
    {
      images.len()
    }
    #[cfg(not(feature = "raster"))]
    {
      0
    }
  };
  let mut draws = Vec::with_capacity(draw_capacity);
  draws.extend(
    rects
      .iter()
      .enumerate()
      .map(|(index, rect)| (rect.order, DevtoolsScreenshotDraw::Rect(index))),
  );
  draws.extend(
    glyphs
      .iter()
      .enumerate()
      .map(|(index, glyph)| (glyph.order, DevtoolsScreenshotDraw::Glyph(index))),
  );
  #[cfg(feature = "raster")]
  draws.extend(
    images
      .iter()
      .enumerate()
      .map(|(index, image)| (image.order, DevtoolsScreenshotDraw::Image(index))),
  );
  draws.sort_by_key(|(order, _)| *order);

  // Faded subtrees paint into transparent layers of the capture's size,
  // composited once at their opacity (see `layout::opacity_layer`).
  let mut plan = crate::layout::opacity_layer::LayerPlan::default();
  plan.build(draws.len(), |index| draws[index].0, layers);
  let mut finished: Vec<Option<Vec<u8>>> = vec![None; plan.targets().len()];
  for (target_index, target) in plan.targets().iter().enumerate() {
    let mut target_pixels = match target.layer {
      Some(_) => vec![0_u8; pixels.len()],
      None => std::mem::take(&mut pixels),
    };
    for step in plan.steps(target) {
      match step {
        crate::layout::opacity_layer::LayerStep::Draws(range) => {
          for (_, draw) in &draws[range.clone()] {
            match *draw {
              DevtoolsScreenshotDraw::Rect(index) => draw_screenshot_rect(&mut target_pixels, bounds, &rects[index]),
              DevtoolsScreenshotDraw::Glyph(index) => {
                draw_screenshot_glyph(&mut target_pixels, bounds, &glyphs[index], atlas)
              }
              #[cfg(feature = "raster")]
              DevtoolsScreenshotDraw::Image(index) => draw_screenshot_image(&mut target_pixels, bounds, &images[index]),
            }
          }
        }
        crate::layout::opacity_layer::LayerStep::Composite(child) => {
          let child_target = &plan.targets()[*child];
          let (Some(layer), Some(layer_pixels)) = (child_target.layer, finished[*child].take()) else {
            continue;
          };
          composite_screenshot_layer(&mut target_pixels, &layer_pixels, bounds.width, layers[layer].opacity);
        }
      }
    }
    if target.layer.is_some() {
      finished[target_index] = Some(target_pixels);
    } else {
      pixels = target_pixels;
    }
  }
  // Inspection captures paint over sensitive text (`app::capture_redaction`).
  crate::app::capture_redaction::redact_pixels(
    &mut pixels,
    bounds.width,
    bounds.height,
    (bounds.x, bounds.y),
    redactions,
  );

  image::save_buffer_with_format(
    output_path,
    &pixels,
    bounds.width,
    bounds.height,
    image::ColorType::Rgba8,
    image::ImageFormat::Png,
  )
}

#[cfg(feature = "devtools")]
fn draw_screenshot_rect(pixels: &mut [u8], bounds: DevtoolsScreenshotBounds, rect: &RectCmd) {
  if let Some(shadow) = &rect.shadow {
    draw_screenshot_box_shadow(pixels, bounds, rect, shadow);
    return;
  }
  let Some(draw) = screenshot_draw_rect(bounds, rect.x, rect.y, rect.width, rect.height, rect.clip) else {
    return;
  };
  let radii = screenshot_effective_rect_radii(bounds, rect);
  let solid_fill = [rect.color.r(), rect.color.g(), rect.color.b(), rect.color.a()];
  if rect.gradient.is_some() || solid_fill[3] > 0 {
    for py in draw.y0..draw.y1 {
      for px in draw.x0..draw.x1 {
        let coverage = screenshot_sample_coverage(bounds, px, py, |world_x, world_y| {
          screenshot_window_clip_contains(bounds, world_x, world_y)
            && screenshot_rounded_rect_contains(world_x, world_y, rect.x, rect.y, rect.width, rect.height, radii)
            && screenshot_clip_contains(rect.clip, world_x, world_y)
        });
        if coverage > 0.0 {
          let world_x = bounds.x as f32 + px as f32 + 0.5;
          let world_y = bounds.y as f32 + py as f32 + 0.5;
          let fill = screenshot_rect_fill_color(rect, solid_fill, world_x, world_y);
          blend_screenshot_pixel(
            pixels,
            bounds.width,
            px,
            py,
            screenshot_color_with_coverage(fill, coverage),
          );
        }
      }
    }
  }

  let stroke = [
    rect.stroke_color.r(),
    rect.stroke_color.g(),
    rect.stroke_color.b(),
    rect.stroke_color.a(),
  ];
  if rect.stroke.iter().any(|width| *width > 0.0) && stroke[3] > 0 {
    draw_screenshot_stroke(pixels, bounds, rect, stroke);
  }
}

#[cfg(feature = "devtools")]
fn draw_screenshot_box_shadow(
  pixels: &mut [u8],
  bounds: DevtoolsScreenshotBounds,
  rect: &RectCmd,
  shadow: &crate::layout::render_list::RectShadow,
) {
  let outset = shadow.outset();
  let Some(draw) = screenshot_draw_rect(
    bounds,
    rect.x - outset,
    rect.y - outset,
    rect.width + 2.0 * outset,
    rect.height + 2.0 * outset,
    rect.clip,
  ) else {
    return;
  };
  let color = [rect.color.r(), rect.color.g(), rect.color.b(), rect.color.a()];
  for py in draw.y0..draw.y1 {
    for px in draw.x0..draw.x1 {
      let world_x = bounds.x as f32 + px as f32 + 0.5;
      let world_y = bounds.y as f32 + py as f32 + 0.5;
      if !screenshot_window_clip_contains(bounds, world_x, world_y)
        || !screenshot_clip_contains(rect.clip, world_x, world_y)
      {
        continue;
      }
      let coverage = crate::layout::box_shadow::rect_shadow_coverage(rect, shadow, world_x, world_y);
      if coverage > 0.0 {
        blend_screenshot_pixel(
          pixels,
          bounds.width,
          px,
          py,
          screenshot_color_with_coverage(color, coverage),
        );
      }
    }
  }
}

#[cfg(feature = "devtools")]
fn draw_screenshot_glyph(
  pixels: &mut [u8],
  bounds: DevtoolsScreenshotBounds,
  glyph: &GlyphCmd,
  atlas: &crate::layout::render_list::GlyphAtlas,
) {
  if atlas.width == 0 || atlas.height == 0 {
    return;
  }
  let Some(draw) = screenshot_draw_rect(bounds, glyph.x, glyph.y, glyph.width, glyph.height, glyph.clip) else {
    return;
  };

  let atlas_x0 = glyph.atlas_min[0].round() as i32;
  let atlas_y0 = glyph.atlas_min[1].round() as i32;
  let atlas_x1 = glyph.atlas_max[0].round() as i32;
  let atlas_y1 = glyph.atlas_max[1].round() as i32;
  let atlas_w = (atlas_x1 - atlas_x0).max(1);
  let atlas_h = (atlas_y1 - atlas_y0).max(1);
  // Glyph colors arrive in linear space (see glyph_engine's to_linear_f32_array);
  // the PNG stores sRGB, so encode like the sRGB surface does on write.
  let color = [
    screenshot_linear_to_srgb_u8(glyph.color[0]),
    screenshot_linear_to_srgb_u8(glyph.color[1]),
    screenshot_linear_to_srgb_u8(glyph.color[2]),
    (glyph.color[3].clamp(0.0, 1.0) * 255.0).round() as u8,
  ];

  for py in draw.y0..draw.y1 {
    for px in draw.x0..draw.x1 {
      let world_x = bounds.x as f32 + px as f32 + 0.5;
      let world_y = bounds.y as f32 + py as f32 + 0.5;
      if !screenshot_window_clip_contains(bounds, world_x, world_y)
        || !screenshot_clip_contains(glyph.clip, world_x, world_y)
      {
        continue;
      }
      let u = ((world_x - glyph.x) / glyph.width.max(1.0)).clamp(0.0, 1.0);
      let v = ((world_y - glyph.y) / glyph.height.max(1.0)).clamp(0.0, 1.0);
      let sx = (atlas_x0 + (u * atlas_w as f32).floor() as i32).clamp(0, atlas.width as i32 - 1);
      let sy = (atlas_y0 + (v * atlas_h as f32).floor() as i32).clamp(0, atlas.height as i32 - 1);
      let source_index = (sy as u32 * atlas.width + sx as u32) as usize * 4;
      if source_index + 3 >= atlas.data.len() {
        continue;
      }
      let source = &atlas.data[source_index..source_index + 4];
      let rgba = if glyph.color_glyph {
        [
          source[0],
          source[1],
          source[2],
          ((source[3] as u16 * color[3] as u16) / 255) as u8,
        ]
      } else {
        [
          color[0],
          color[1],
          color[2],
          ((source[3] as u16 * color[3] as u16) / 255) as u8,
        ]
      };
      blend_screenshot_pixel(pixels, bounds.width, px, py, rgba);
    }
  }
}

#[cfg(all(feature = "devtools", feature = "raster"))]
fn draw_screenshot_image(pixels: &mut [u8], bounds: DevtoolsScreenshotBounds, image: &crate::images::ImageCmd) {
  if image.image_format != crate::images::ImagePixelFormat::Rgba8 || image.image_width == 0 || image.image_height == 0 {
    return;
  }
  let Some(draw) = screenshot_draw_rect(bounds, image.x, image.y, image.width, image.height, image.clip) else {
    return;
  };

  for py in draw.y0..draw.y1 {
    for px in draw.x0..draw.x1 {
      let world_x = bounds.x as f32 + px as f32 + 0.5;
      let world_y = bounds.y as f32 + py as f32 + 0.5;
      if !screenshot_window_clip_contains(bounds, world_x, world_y)
        || !screenshot_clip_contains(image.clip, world_x, world_y)
        || !screenshot_rounded_rect_contains(
          world_x,
          world_y,
          image.x,
          image.y,
          image.width,
          image.height,
          image.radii,
        )
      {
        continue;
      }
      let local_u = ((world_x - image.x) / image.width.max(1.0)).clamp(0.0, 1.0);
      let local_v = ((world_y - image.y) / image.height.max(1.0)).clamp(0.0, 1.0);
      let u = image.uv_min[0] + (image.uv_max[0] - image.uv_min[0]) * local_u;
      let v = image.uv_min[1] + (image.uv_max[1] - image.uv_min[1]) * local_v;
      let sx = (u * image.image_width as f32)
        .floor()
        .clamp(0.0, image.image_width.saturating_sub(1) as f32) as u32;
      let sy = (v * image.image_height as f32)
        .floor()
        .clamp(0.0, image.image_height.saturating_sub(1) as f32) as u32;
      let source_index = (sy * image.image_width + sx) as usize * 4;
      if source_index + 3 >= image.data.len() {
        continue;
      }
      blend_screenshot_pixel(
        pixels,
        bounds.width,
        px,
        py,
        [
          image.data[source_index],
          image.data[source_index + 1],
          image.data[source_index + 2],
          image.data[source_index + 3],
        ],
      );
    }
  }
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy)]
struct ScreenshotDrawRect {
  x0: u32,
  y0: u32,
  x1: u32,
  y1: u32,
}

#[cfg(feature = "devtools")]
fn screenshot_draw_rect(
  bounds: DevtoolsScreenshotBounds,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  clip: ClipRect,
) -> Option<ScreenshotDrawRect> {
  let mut left = x.floor().max(bounds.x as f32);
  let mut top = y.floor().max(bounds.y as f32);
  let mut right = (x + width).ceil().min((bounds.x + bounds.width) as f32);
  let mut bottom = (y + height).ceil().min((bounds.y + bounds.height) as f32);
  if clip.active {
    left = left.max(clip.x);
    top = top.max(clip.y);
    right = right.min(clip.x + clip.width);
    bottom = bottom.min(clip.y + clip.height);
  }
  if right <= left || bottom <= top {
    return None;
  }
  Some(ScreenshotDrawRect {
    x0: (left as u32).saturating_sub(bounds.x),
    y0: (top as u32).saturating_sub(bounds.y),
    x1: (right as u32).saturating_sub(bounds.x).min(bounds.width),
    y1: (bottom as u32).saturating_sub(bounds.y).min(bounds.height),
  })
}

#[cfg(feature = "devtools")]
fn draw_screenshot_stroke(pixels: &mut [u8], bounds: DevtoolsScreenshotBounds, rect: &RectCmd, color: [u8; 4]) {
  let top = rect.stroke[0].max(0.0);
  let right = rect.stroke[1].max(0.0);
  let bottom = rect.stroke[2].max(0.0);
  let left = rect.stroke[3].max(0.0);
  let radii = screenshot_effective_rect_radii(bounds, rect);
  let Some(draw) = screenshot_draw_rect(bounds, rect.x, rect.y, rect.width, rect.height, rect.clip) else {
    return;
  };

  let inner_x = rect.x + left;
  let inner_y = rect.y + top;
  let inner_width = (rect.width - left - right).max(0.0);
  let inner_height = (rect.height - top - bottom).max(0.0);
  let inner_radii = [
    (radii[0] - left.max(top)).max(0.0),
    (radii[1] - right.max(top)).max(0.0),
    (radii[2] - right.max(bottom)).max(0.0),
    (radii[3] - left.max(bottom)).max(0.0),
  ];

  for py in draw.y0..draw.y1 {
    for px in draw.x0..draw.x1 {
      let coverage = screenshot_sample_coverage(bounds, px, py, |world_x, world_y| {
        if !screenshot_window_clip_contains(bounds, world_x, world_y)
          || !screenshot_clip_contains(rect.clip, world_x, world_y)
          || !screenshot_rounded_rect_contains(world_x, world_y, rect.x, rect.y, rect.width, rect.height, radii)
        {
          return false;
        }
        if inner_width > 0.0
          && inner_height > 0.0
          && screenshot_rounded_rect_contains(
            world_x,
            world_y,
            inner_x,
            inner_y,
            inner_width,
            inner_height,
            inner_radii,
          )
        {
          return false;
        }
        true
      });
      if coverage > 0.0 {
        blend_screenshot_pixel(
          pixels,
          bounds.width,
          px,
          py,
          screenshot_color_with_coverage(color, coverage),
        );
      }
    }
  }
}

#[cfg(feature = "devtools")]
const SCREENSHOT_SAMPLE_OFFSETS: [(f32, f32); 4] = [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)];

#[cfg(feature = "devtools")]
fn screenshot_sample_coverage(
  bounds: DevtoolsScreenshotBounds,
  px: u32,
  py: u32,
  mut contains: impl FnMut(f32, f32) -> bool,
) -> f32 {
  let mut hits = 0_u32;
  for (dx, dy) in SCREENSHOT_SAMPLE_OFFSETS {
    let world_x = bounds.x as f32 + px as f32 + 0.5 + dx;
    let world_y = bounds.y as f32 + py as f32 + 0.5 + dy;
    if contains(world_x, world_y) {
      hits += 1;
    }
  }
  hits as f32 / SCREENSHOT_SAMPLE_OFFSETS.len() as f32
}

#[cfg(feature = "devtools")]
fn screenshot_color_with_coverage(mut color: [u8; 4], coverage: f32) -> [u8; 4] {
  color[3] = (color[3] as f32 * coverage.clamp(0.0, 1.0)).round() as u8;
  color
}

#[cfg(feature = "devtools")]
fn screenshot_window_clip_contains(bounds: DevtoolsScreenshotBounds, x: f32, y: f32) -> bool {
  let Some(clip) = bounds.window_clip else {
    return true;
  };
  screenshot_rounded_rect_contains(x, y, 0.0, 0.0, clip.width, clip.height, clip.radii)
}

#[cfg(feature = "devtools")]
fn screenshot_effective_rect_radii(bounds: DevtoolsScreenshotBounds, rect: &RectCmd) -> [f32; 4] {
  let mut radii = rect.radii;
  let Some(clip) = bounds.window_clip else {
    return radii;
  };

  const WINDOW_RECT_TOLERANCE_PX: f32 = 1.0;
  let covers_window = rect.x <= WINDOW_RECT_TOLERANCE_PX
    && rect.y <= WINDOW_RECT_TOLERANCE_PX
    && rect.x + rect.width >= clip.width - WINDOW_RECT_TOLERANCE_PX
    && rect.y + rect.height >= clip.height - WINDOW_RECT_TOLERANCE_PX;
  if covers_window {
    for (radius, window_radius) in radii.iter_mut().zip(clip.radii) {
      *radius = radius.max(window_radius);
    }
  }
  radii
}

#[cfg(feature = "devtools")]
fn screenshot_rect_fill_color(rect: &RectCmd, fallback: [u8; 4], world_x: f32, world_y: f32) -> [u8; 4] {
  let Some(gradient) = &rect.gradient else {
    return fallback;
  };
  if gradient.stops.is_empty() {
    return fallback;
  }

  let local = [
    world_x - rect.x - rect.width * 0.5,
    world_y - rect.y - rect.height * 0.5,
  ];
  let half = [rect.width * 0.5, rect.height * 0.5];
  screenshot_sample_gradient(gradient, local, half)
}

#[cfg(feature = "devtools")]
fn screenshot_sample_gradient(gradient: &RenderGradient, local: [f32; 2], half: [f32; 2]) -> [u8; 4] {
  let mut t = match gradient.kind {
    0 => {
      let dir = gradient.dir;
      let hl = (half[0] * dir[0]).abs() + (half[1] * dir[1]).abs();
      ((local[0] * dir[0] + local[1] * dir[1]) + hl) / (2.0 * hl.max(1e-5))
    }
    1 => {
      let center_norm = [gradient.center[0] * 2.0 - 1.0, gradient.center[1] * 2.0 - 1.0];
      let center = [center_norm[0] * half[0], center_norm[1] * half[1]];
      if gradient.flags & 1 != 0 {
        let p = [
          (local[0] - center[0]) / half[0].max(1e-3),
          (local[1] - center[1]) / half[1].max(1e-3),
        ];
        let radius = screenshot_max_corner_distance(center_norm, [1.0, 1.0]);
        screenshot_len(p) / radius.max(1e-5)
      } else {
        let radius = screenshot_max_corner_distance(center, half);
        screenshot_len([local[0] - center[0], local[1] - center[1]]) / radius.max(1e-5)
      }
    }
    2 => {
      let center = [
        (gradient.center[0] * 2.0 - 1.0) * half[0],
        (gradient.center[1] * 2.0 - 1.0) * half[1],
      ];
      let d = [local[0] - center[0], local[1] - center[1]];
      let angle = (d[0].atan2(-d[1]) - gradient.from_angle) / (2.0 * std::f32::consts::PI);
      angle - angle.floor()
    }
    _ => 0.0,
  };

  if gradient.kind != 2 {
    t = t.clamp(0.0, 1.0);
  }

  let mut color = gradient.stops.last().map(|stop| stop.color).unwrap_or([0.0; 4]);
  for stops in gradient.stops.windows(2) {
    let [a, b] = stops else {
      continue;
    };
    if t <= b.position {
      let span = (b.position - a.position).max(1e-5);
      let mix = ((t - a.position) / span).clamp(0.0, 1.0);
      color = [
        a.color[0] + (b.color[0] - a.color[0]) * mix,
        a.color[1] + (b.color[1] - a.color[1]) * mix,
        a.color[2] + (b.color[2] - a.color[2]) * mix,
        a.color[3] + (b.color[3] - a.color[3]) * mix,
      ];
      break;
    }
  }

  [
    screenshot_linear_to_srgb_u8(color[0]),
    screenshot_linear_to_srgb_u8(color[1]),
    screenshot_linear_to_srgb_u8(color[2]),
    (color[3].clamp(0.0, 1.0) * 255.0).round() as u8,
  ]
}

#[cfg(feature = "devtools")]
fn screenshot_max_corner_distance(center: [f32; 2], half: [f32; 2]) -> f32 {
  [
    [-half[0], -half[1]],
    [half[0], -half[1]],
    [-half[0], half[1]],
    [half[0], half[1]],
  ]
  .into_iter()
  .map(|corner| screenshot_len([corner[0] - center[0], corner[1] - center[1]]))
  .fold(0.0, f32::max)
}

#[cfg(feature = "devtools")]
fn screenshot_len(v: [f32; 2]) -> f32 {
  (v[0] * v[0] + v[1] * v[1]).sqrt()
}

#[cfg(feature = "devtools")]
fn screenshot_linear_to_srgb_u8(channel: f32) -> u8 {
  let channel = channel.clamp(0.0, 1.0);
  let srgb = if channel <= 0.003_130_8 {
    channel * 12.92
  } else {
    1.055 * channel.powf(1.0 / 2.4) - 0.055
  };
  (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(feature = "devtools")]
fn screenshot_clip_contains(clip: ClipRect, x: f32, y: f32) -> bool {
  if !clip.active {
    return true;
  }
  if x < clip.x || y < clip.y || x >= clip.x + clip.width || y >= clip.y + clip.height {
    return false;
  }
  let radii = clip.border_radius.map(|radius| radius.to_array()).unwrap_or([0.0; 4]);
  screenshot_rounded_rect_contains(x, y, clip.x, clip.y, clip.width, clip.height, radii)
}

#[cfg(feature = "devtools")]
fn screenshot_rounded_rect_contains(
  x: f32,
  y: f32,
  rect_x: f32,
  rect_y: f32,
  width: f32,
  height: f32,
  radii: [f32; 4],
) -> bool {
  if x < rect_x || y < rect_y || x >= rect_x + width || y >= rect_y + height {
    return false;
  }
  let radii = screenshot_normalized_radii(radii, width, height);
  if radii.iter().all(|radius| *radius <= 0.0) {
    return true;
  }

  let right = rect_x + width;
  let bottom = rect_y + height;
  if radii[0] > 0.0 && x < rect_x + radii[0] && y < rect_y + radii[0] {
    return screenshot_point_in_corner(x, y, rect_x + radii[0], rect_y + radii[0], radii[0]);
  }
  if radii[1] > 0.0 && x >= right - radii[1] && y < rect_y + radii[1] {
    return screenshot_point_in_corner(x, y, right - radii[1], rect_y + radii[1], radii[1]);
  }
  if radii[2] > 0.0 && x >= right - radii[2] && y >= bottom - radii[2] {
    return screenshot_point_in_corner(x, y, right - radii[2], bottom - radii[2], radii[2]);
  }
  if radii[3] > 0.0 && x < rect_x + radii[3] && y >= bottom - radii[3] {
    return screenshot_point_in_corner(x, y, rect_x + radii[3], bottom - radii[3], radii[3]);
  }
  true
}

#[cfg(feature = "devtools")]
fn screenshot_normalized_radii(mut radii: [f32; 4], width: f32, height: f32) -> [f32; 4] {
  let max_radius = width.min(height).max(0.0) * 0.5;
  for radius in &mut radii {
    *radius = radius.max(0.0).min(max_radius);
  }
  radii
}

#[cfg(feature = "devtools")]
fn screenshot_point_in_corner(x: f32, y: f32, center_x: f32, center_y: f32, radius: f32) -> bool {
  let dx = x - center_x;
  let dy = y - center_y;
  dx * dx + dy * dy <= radius * radius
}

#[cfg(feature = "devtools")]
/// Composites a layer painted like `pixels` (straight alpha, same size) over
/// them at `opacity`.
#[cfg(feature = "devtools")]
fn composite_screenshot_layer(pixels: &mut [u8], layer: &[u8], width: u32, opacity: f32) {
  for (index, source) in layer.chunks_exact(4).enumerate() {
    let alpha = (f32::from(source[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    if alpha == 0 {
      continue;
    }
    let (x, y) = (index as u32 % width, index as u32 / width);
    blend_screenshot_pixel(pixels, width, x, y, [source[0], source[1], source[2], alpha]);
  }
}

fn blend_screenshot_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, source: [u8; 4]) {
  let source_alpha = source[3] as f32 / 255.0;
  if source_alpha <= 0.0 {
    return;
  }
  let index = (y * width + x) as usize * 4;
  if index + 3 >= pixels.len() {
    return;
  }
  let dest_alpha = pixels[index + 3] as f32 / 255.0;
  let out_alpha = source_alpha + dest_alpha * (1.0 - source_alpha);
  if out_alpha <= 0.0 {
    pixels[index..index + 4].copy_from_slice(&[0, 0, 0, 0]);
    return;
  }
  for channel in 0..3 {
    let source_channel = source[channel] as f32 / 255.0;
    let dest_channel = pixels[index + channel] as f32 / 255.0;
    let out = (source_channel * source_alpha + dest_channel * dest_alpha * (1.0 - source_alpha)) / out_alpha;
    pixels[index + channel] = (out.clamp(0.0, 1.0) * 255.0).round() as u8;
  }
  pixels[index + 3] = (out_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn find_path_by_id(node: &Node, id: NodeId) -> Option<Vec<usize>> {
  fn visit(node: &Node, id: NodeId, path: &mut Vec<usize>) -> bool {
    if node.node_id() == id {
      return true;
    }

    for (index, child) in node.children().iter().enumerate() {
      path.push(index);
      if visit(child, id, path) {
        return true;
      }
      path.pop();
    }

    false
  }

  let mut path = Vec::new();
  visit(node, id, &mut path).then_some(path)
}

#[derive(Clone, Copy)]
struct FocusTarget {
  input_id: NodeId,
  event_id: NodeId,
}

fn set_node_hovered(node: &Node, hovered: bool) {
  node.set_style_hovered(hovered);
  // Mouse dispatch hovers a scrollbar under the pointer; leaving the
  // container (or the window) leaves its scrollbar too.
  if !hovered && let Some((state, _)) = node.scroll_container() {
    state.set_thumb_hovered(false);
  }
  if let Some(state) = node.slider_state() {
    state.set_hovered(hovered);
  }
  if let Some(ref state) = node.interaction {
    state.set_hovered(hovered);
  }
  if let Some(ref element_ref) = node.element_ref {
    element_ref.set_hovered(hovered);
  }
}

/// The cursor of the innermost of the hovered `nodes` that sets one. A
/// scrollbar under the pointer shows the default cursor, whatever cursor its
/// node sets (a text input's text cursor, say).
fn hovered_cursor<'a>(mut nodes: impl Iterator<Item = &'a Node>) -> CursorIcon {
  nodes
    .find_map(|node| {
      if node
        .scroll_container()
        .is_some_and(|(state, _)| state.is_thumb_hovered())
      {
        Some(CursorIcon::Default)
      } else {
        node.cursor_icon()
      }
    })
    .unwrap_or(CursorIcon::Default)
}

/// Drops the hits above the scroll container whose scrollbar takes the
/// pointer at `(x, y)`, so the scrollbar, not the content under it, gets it.
///
/// Where the scrollbars of nested containers overlap, the bar painted on top
/// wins: a container paints its scrollbars over its content, so the outermost
/// container's bar covers those of the containers inside it. `hits` runs from
/// the innermost node outward, so that is the last match.
fn trim_hits_to_scrollbar(hits: &mut Vec<(&Node, crate::app::hit_test::HitRect)>, x: f32, y: f32) {
  let Some(index) = hits
    .iter()
    .rposition(|(node, _)| scrollbar_hit_at(node, x, y).is_some())
  else {
    return;
  };

  if index > 0 {
    hits.drain(..index);
  }
}

/// The axis and part of `node`'s scrollbar that takes the pointer at
/// `(x, y)`, when `node` is a scroll container showing a scrollbar there.
///
/// Like nested containers, the two bars of a container scrolling both ways
/// resolve their overlap by paint order: the horizontal bar is painted over the
/// vertical one, so it wins in the corner.
fn scrollbar_hit_at(node: &Node, x: f32, y: f32) -> Option<(ScrollAxis, ScrollBarPart)> {
  let (state, direction) = node.scroll_container()?;

  let style = state.style();
  scroll_axes(direction)
    .iter()
    .rev()
    .find_map(|&axis| Some((axis, state.scrollbar_part_at(axis, &style, x, y)?)))
}

fn set_node_active(node: &Node, active: bool) {
  node.set_style_active(active);
  if let Some(ref state) = node.interaction {
    state.set_active(active);
  }
  if let Some(ref element_ref) = node.element_ref {
    element_ref.set_active(active);
  }
}

fn set_node_focused(node: &Node, focused: bool) {
  node.set_style_focused(focused);
  if let Some(ref state) = node.interaction {
    state.set_focused(focused);
  }
  if let Some(ref element_ref) = node.element_ref {
    element_ref.set_focused(focused);
  }
}

fn reset_element_ref_flags_recursive(node: &Node) {
  node.set_style_hovered(false);
  node.set_style_active(false);
  node.set_style_focused(false);
  node.set_style_drag_over(false);
  if let Some(ref element_ref) = node.element_ref {
    element_ref.set_hovered(false);
    element_ref.set_active(false);
  }
  for child in node.children() {
    reset_element_ref_flags_recursive(child);
  }
}

fn replace_live_component_slot_everywhere(
  node: &mut Node,
  slot_id: u64,
  replacement: &Node,
  id_gen: &IdGenerator,
) -> bool {
  let mut replaced = false;
  if node.component_slot_id() == Some(slot_id) {
    let mut replacement = replacement.clone_for_reuse();
    replacement.adopt_outer_decoration(node);
    reset_element_ref_flags_recursive(node);
    replacement.preserve_runtime_state_from(node);
    #[cfg(feature = "canvas")]
    detach_canvases_not_carried(canvas_bindings(node), Some(&replacement));
    replacement.preserve_ids_from(node);
    node.free_ids(id_gen);
    replacement.assign_ids(id_gen);
    *node = replacement;
    return true;
  }

  for child in &mut node.children {
    replaced |= replace_live_component_slot_everywhere(child, slot_id, replacement, id_gen);
  }

  if let Some(spec) = node.modal_declaration.as_mut() {
    replaced |= replace_live_component_slot_everywhere(&mut spec.node, slot_id, replacement, id_gen);
  }

  if let Some(spec) = node.overlay_declaration.as_mut() {
    replaced |= replace_live_component_slot_everywhere(&mut spec.node, slot_id, replacement, id_gen);
  }

  if replaced {
    node.layout_cache.mark_descendant_dirty();
  }

  replaced
}

#[cfg(test)]
mod layout_tracking_tests {
  use super::*;

  #[test]
  fn shared_handle_notification_survives_another_headless_pass() {
    let mut app = App::new();
    let reference = crate::core::ElementRefMut::new();
    let build = || {
      let mut tree = Tree::new();
      tree.resize(400, 300);
      tree.set_root(
        crate::components::Stack::new()
          .child(crate::components::Rect::new(50.0, 50.0).ref_element(reference.clone())),
      );
      tree
    };
    let mut first = build();
    let mut second = build();
    first.pass_headless(&mut app);
    second.pass_headless(&mut app);
    reference.set_relative_bounds(30.0, 40.0, 80.0, 70.0);
    first.pass_headless(&mut app);

    let report = second.pass_headless(&mut app);

    assert!(report.layout_updated);
    assert_eq!(second.last_layout().unwrap().children[0].result.size, Size::new(80.0, 70.0));
  }

  #[test]
  fn popup_declarations_do_not_wedge_the_layout_fast_path() {
    for open in [false, true] {
      let mut app = App::new();
      let mut tree = Tree::new();
      tree.resize(400, 300);
      let anchor = crate::core::ElementRef::new();
      tree.set_root(
        crate::components::Column::new()
          .child(crate::components::Rect::new(40.0, 40.0).ref_element(anchor.clone()))
          .child(
            crate::components::Popup::new(anchor, crate::components::Rect::new(30.0, 30.0))
              .open(crate::core::Signal::new(open)),
          ),
      );
      tree.pass_headless(&mut app);
      assert!(!has_pending_layout_dirty_recursive(tree.root.as_ref().unwrap()));

      let report = tree.pass_headless(&mut app);

      assert!(!report.layout_updated, "popup open={open} prevented the fast path");
    }
  }

  #[test]
  fn unchanged_large_headless_pass_skips_layout_walks() {
    let mut app = App::new();
    let mut tree = Tree::new();
    tree.resize(400, 20_000);
    let mut column = crate::components::Column::new();
    for _ in 0..10_000 {
      column = column.child(crate::components::Rect::new(10.0, 1.0));
    }
    tree.set_root(column);
    tree.pass_headless(&mut app);
    crate::core::layout_dirty::VISITS.with(|visits| visits.set(0));

    let report = tree.pass_headless(&mut app);

    assert!(!report.layout_updated);
    let visits = crate::core::layout_dirty::VISITS.with(|visits| visits.get());
    assert!(visits < 40, "unchanged pass visited {visits} nodes");
  }
}

fn has_dirty_element_ref_recursive(node: &Node) -> bool {
  if !node.layout_cache.needs_dirty_scan() {
    return false;
  }
  #[cfg(test)]
  crate::core::layout_dirty::visit();
  node
    .element_ref
    .as_ref()
    .is_some_and(|element_ref| element_ref.has_layout_dirty())
    || node.children().iter().any(has_dirty_element_ref_recursive)
}

fn has_runtime_layout_state_recursive(node: &Node) -> bool {
  if !node.prepare_layout_tracking().select {
    return false;
  }
  #[cfg(test)]
  crate::core::layout_dirty::visit();
  let local = match node.node_kind() {
    NodeKind::Select { state } => state.is_open(),
    _ => false,
  };
  local || node.children().iter().any(has_runtime_layout_state_recursive)
}

fn has_timeline_target_recursive(node: &Node) -> bool {
  if !node.prepare_layout_tracking().timeline {
    return false;
  }
  #[cfg(test)]
  crate::core::layout_dirty::visit();
  !node.transitions.is_empty() || node.animation.is_some() || node.children().iter().any(has_timeline_target_recursive)
}

fn has_pending_layout_dirty_recursive(node: &Node) -> bool {
  if !node.layout_cache.needs_dirty_scan() && !node.layout_cache.is_dirty() {
    return false;
  }
  #[cfg(test)]
  crate::core::layout_dirty::visit();
  // Overlay/modal declaration nodes are invisible to layout (mirrors the
  // `mark_layout_dirty` skip): they are never laid out, so their flags could
  // never be cleared — counting them here would wedge the fast path. Their
  // content is laid out — and tracked — inside the overlay host subtree.
  if node.is_overlay_declaration() {
    return false;
  }
  let text_input_dirty = match node.node_kind() {
    NodeKind::TextInput { state, .. } => state.has_layout_dirty(),
    NodeKind::Select { state } => state.has_layout_dirty(),
    _ => false,
  };
  // Another compute can consume a shared handle's flag before this tree's
  // pass. Its pending revision still needs to reach the layout engine.
  let local = (node.layout_cache.inspected_generation.get() != 0 && node.layout_cache.needs_dirty_scan())
    || text_input_dirty
    || node.layout_cache.is_dirty()
    || node.has_style_layout_dirty()
    || node
      .scroll_container()
      .is_some_and(|(state, _)| state.has_scroll_dirty());
  local || node.children().iter().any(has_pending_layout_dirty_recursive)
}

#[cfg(feature = "canvas")]
fn detach_canvas_recursive(node: &Node) {
  #[cfg(feature = "canvas")]
  if let Some(canvas) = node.canvas_handle() {
    if let Some(reference) = &node.element_ref {
      reference.detach_canvas(canvas.surface_id());
    }
    canvas.detach();
  }

  for child in node.children() {
    detach_canvas_recursive(child);
  }
}

/// Canvas surfaces of a subtree about to be replaced, with the refs bound to them.
#[cfg(feature = "canvas")]
type CanvasBindings = Vec<(crate::canvas::CanvasHandle, Option<crate::core::ElementRef>)>;

#[cfg(feature = "canvas")]
fn canvas_bindings(node: &Node) -> CanvasBindings {
  fn collect(node: &Node, bindings: &mut CanvasBindings) {
    if let Some(canvas) = node.canvas_handle() {
      bindings.push((canvas, node.element_ref.clone()));
    }
    for child in node.children() {
      collect(child, bindings);
    }
  }
  let mut bindings = Vec::new();
  collect(node, &mut bindings);
  bindings
}

/// Unbind the replaced subtree's refs from their canvases and detach the
/// canvases its replacement (if any) did not carry over. A carried-over surface
/// stays attached with its queued drawing and state: detaching it would drop
/// drawing queued earlier in the same frame (for example by the handler whose
/// state change caused the re-render). The next layout binds it and its new
/// node's ref again.
#[cfg(feature = "canvas")]
fn detach_canvases_not_carried(old: CanvasBindings, replacement: Option<&Node>) {
  if old.is_empty() {
    return;
  }
  let kept: std::collections::HashSet<_> = replacement
    .map(canvas_bindings)
    .unwrap_or_default()
    .iter()
    .map(|(canvas, _)| canvas.surface_id())
    .collect();
  for (canvas, reference) in old {
    let id = canvas.surface_id();
    if let Some(reference) = reference {
      reference.detach_canvas(id);
    }
    if !kept.contains(&id) {
      canvas.detach();
    }
  }
}

#[cfg(feature = "canvas")]
fn update_canvas_placement_recursive(
  node: &Node,
  layout: &LayoutResult,
  engine: &crate::layout::layout_engine::LayoutEngine,
  x: f32,
  y: f32,
  inherited: crate::node::transform::Transform2D,
) {
  use crate::node::transform::Transform2D;
  let composed = inherited.then(
    &node
      .effective_transform()
      .around_origin([x + layout.size.width * 0.5, y + layout.size.height * 0.5]),
  );
  if let Some(canvas) = node.canvas_handle() {
    let padding = engine.resolved_padding_for_size(node, layout.size);
    canvas.update_placement(composed.then(&Transform2D::translate(x + padding.left, y + padding.top)));
  }
  for (child_layout, child) in layout.children.iter().zip(node.children()) {
    update_canvas_placement_recursive(
      child,
      &child_layout.result,
      engine,
      x + child_layout.offset.x,
      y + child_layout.offset.y,
      composed,
    );
  }
}

#[cfg(feature = "canvas")]
#[allow(clippy::too_many_arguments)]
fn bind_canvas_layout_recursive(
  node: &Node,
  layout: &LayoutResult,
  engine: &crate::layout::layout_engine::LayoutEngine,
  abs_x: f32,
  abs_y: f32,
  inherited: crate::node::transform::Transform2D,
  scale: f32,
  window: &crate::app::window::Window,
  font: &crate::canvas::CanvasFont,
  text: &Arc<parking_lot::Mutex<crate::canvas::CanvasTextEngine>>,
  registry: &mut Vec<crate::canvas::CanvasHandle>,
) {
  use crate::node::transform::Transform2D;
  let local = node
    .effective_transform()
    .around_origin([abs_x + layout.size.width * 0.5, abs_y + layout.size.height * 0.5]);
  let composed = inherited.then(&local);
  if let Some(canvas) = node.canvas_handle() {
    registry.push(canvas.clone());
    let padding = engine.resolved_padding_for_size(node, layout.size);
    let size = Size::new(
      (layout.size.width - padding.left - padding.right).max(0.0),
      (layout.size.height - padding.top - padding.bottom).max(0.0),
    );
    let to_window = composed.then(&Transform2D::translate(abs_x + padding.left, abs_y + padding.top));
    let notification = canvas.bind_layout(size, scale, to_window, window.clone(), font.clone(), text.clone());
    if let Some(reference) = &node.element_ref {
      reference.bind_canvas(&canvas);
    }
    if let Some((metrics, observers)) = notification {
      for observer in observers {
        observer(metrics);
      }
    }
  }
  for (child_layout, child) in layout.children.iter().zip(node.children()) {
    bind_canvas_layout_recursive(
      child,
      &child_layout.result,
      engine,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      composed,
      scale,
      window,
      font,
      text,
      registry,
    );
  }
}

fn update_element_refs_recursive(
  node: &mut Node,
  layout: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  parent_x: f32,
  parent_y: f32,
) {
  if layout.dropped {
    collapse_dropped_element_refs(node, abs_x, abs_y, parent_x, parent_y);
    return;
  }
  if let Some(element_ref) = &node.element_ref {
    element_ref.update(
      abs_x,
      abs_y,
      abs_x - parent_x,
      abs_y - parent_y,
      layout.size.width,
      layout.size.height,
    );
  }
  // Same space as the element refs above, so after-layout hooks can compare
  // a scroll viewport with the rows inside it.
  if let LayoutKind::ScrollModifier { state, .. } = node.layout_kind() {
    state.set_layout_viewport_y(abs_y);
  }

  for (child_layout, child_node) in layout.children.iter().zip(node.children.iter_mut()) {
    update_element_refs_recursive(
      child_node,
      &child_layout.result,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      abs_x,
      abs_y,
    );
  }
}

/// Every element ref in a subtree its Row/Column dropped reports a zero-size
/// rect at the dropped child's place in the line, whatever its own layout
/// inside the zero-size box says.
fn collapse_dropped_element_refs(node: &Node, abs_x: f32, abs_y: f32, parent_x: f32, parent_y: f32) {
  if let Some(element_ref) = &node.element_ref {
    element_ref.update(abs_x, abs_y, abs_x - parent_x, abs_y - parent_y, 0.0, 0.0);
  }
  for child in node.children() {
    collapse_dropped_element_refs(child, abs_x, abs_y, abs_x, abs_y);
  }
}

#[cfg(feature = "form")]
fn nearest_form_path_for_path(root: &Node, path: &[usize]) -> Option<Vec<usize>> {
  for len in (0..=path.len()).rev() {
    let candidate = &path[..len];
    if find_node_by_path(root, candidate).is_some_and(|node| node.events.on_submit.is_some()) {
      return Some(candidate.to_vec());
    }
  }
  None
}

#[cfg(feature = "form")]
fn nearest_form_submission(
  root: &Node,
  path: &[usize],
) -> Option<(Arc<dyn Fn(crate::node::FormData) + Send + Sync>, crate::node::FormData)> {
  let form_path = nearest_form_path_for_path(root, path)?;
  let form = find_node_by_path(root, &form_path)?;
  let handler = form.submit_handler()?;
  let mut data = crate::node::FormData::new();
  collect_form_data(form, &mut data);
  Some((handler, data))
}

#[cfg(feature = "form")]
fn collect_form_data(node: &Node, data: &mut crate::node::FormData) {
  if let Some(name) = node.form_name_value() {
    match node.node_kind() {
      NodeKind::TextInput { state, .. } => data.append(name, state.value()),
      NodeKind::Checkbox { state } => {
        if state.is_checked() {
          data.append(name, "on");
        }
      }
      NodeKind::Slider { state } => data.append(name, state.value_string()),
      NodeKind::Select { state } => {
        for label in state.selected_labels() {
          data.append(name, label.to_string());
        }
      }
      _ => {}
    }
  }

  for child in node.children() {
    collect_form_data(child, data);
  }
}

/// Whether a press on `hits` (innermost first) blurs the focused element:
/// only when neither a focusable nor a `focusable(false)` node is hit.
fn press_blurs_focus(hits: &[(&Node, crate::app::hit_test::HitRect)]) -> bool {
  !hits
    .iter()
    .any(|(node, _)| node.is_focusable() || node.is_focus_disabled())
}

fn dispatch_builtin_pointer(
  hits: &[(&Node, crate::app::hit_test::HitRect)],
  x: f32,
  click: bool,
) -> Option<FocusTarget> {
  if !click {
    return None;
  }

  let event_id = hits
    .iter()
    .find(|(node, _)| !node.events.on_focus.is_empty() || !node.events.on_blur.is_empty())
    .map(|(node, _)| node.node_id());

  for (node, rect) in hits {
    if node.button_kind_value().is_some() {
      return Some(FocusTarget {
        input_id: node.node_id(),
        event_id: event_id.unwrap_or_else(|| node.node_id()),
      });
    }
    match node.node_kind() {
      // Focused on press, before the click.
      NodeKind::TextInput { .. } | NodeKind::Select { .. } => return None,
      NodeKind::Checkbox { state } => {
        state.toggle();
        return Some(FocusTarget {
          input_id: node.node_id(),
          event_id: event_id.unwrap_or_else(|| node.node_id()),
        });
      }
      NodeKind::Slider { state } => {
        let (track_rect, thumb_rect) = state.part_rects(
          rect.x,
          rect.y,
          rect.width,
          rect.height,
          node.is_style_hovered(),
          DEFAULT_SLIDER_THUMB_MIN_SIZE,
        );
        let ratio = state.pointer_ratio(x, track_rect, thumb_rect);
        state.set_from_ratio(ratio);
        state.clear_drag_ratio();
        return Some(FocusTarget {
          input_id: node.node_id(),
          event_id: event_id.unwrap_or_else(|| node.node_id()),
        });
      }
      // `focusable(true)` or a tab index: a click focuses it, like a browser.
      _ if node.is_focusable() => {
        return Some(FocusTarget {
          input_id: node.node_id(),
          event_id: event_id.unwrap_or_else(|| node.node_id()),
        });
      }
      _ => {}
    }
  }

  None
}

fn selected_selectable_text(node: &Node, layout: &LayoutResult) -> Option<String> {
  let mut fragments = Vec::new();
  collect_selected_selectable_text(node, layout, 0.0, 0.0, &mut fragments);
  if fragments.is_empty() {
    return None;
  }
  Some(join_selected_text(fragments))
}

fn has_selected_selectable_text(node: &Node) -> bool {
  if let Some((state, value)) = selectable_text_state_and_value(node)
    && state.selected_text(&value).is_some()
  {
    return true;
  }

  for child in node.children() {
    if has_selected_selectable_text(child) {
      return true;
    }
  }

  false
}

fn collect_selected_selectable_text(
  node: &Node,
  layout: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  fragments: &mut Vec<SelectedTextFragment>,
) {
  if let Some((state, value)) = selectable_text_state_and_value(node)
    && let Some(text) = selected_content(node, &state, &value)
  {
    let ranges = state.selection_ranges(&value);
    let x = ranges
      .iter()
      .map(|range| abs_x + range.x)
      .reduce(f32::min)
      .unwrap_or(abs_x);
    let end_x = ranges
      .iter()
      .map(|range| abs_x + range.x + range.width)
      .reduce(f32::max)
      .unwrap_or(abs_x + layout.size.width);
    fragments.push(SelectedTextFragment {
      x,
      end_x,
      top: abs_y,
      bottom: abs_y + layout.size.height,
      text,
    });
  }

  for (child_layout, child_node) in layout.children.iter().zip(node.children()) {
    collect_selected_selectable_text(
      child_node,
      &child_layout.result,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      fragments,
    );
  }
}

fn selectable_text_endpoint<'a>(
  root: &'a Node,
  layout: &'a LayoutResult,
  x: f32,
  y: f32,
) -> Option<(&'a Node, HitRect)> {
  let mut hits = Vec::new();
  hit_test_tree(root, layout, 0.0, 0.0, x, y, &mut hits);
  if let Some(hit) = hits.into_iter().find(|(node, _)| is_selectable_text_node(node)) {
    return Some(hit);
  }

  nearest_selectable_text(root, layout, 0.0, 0.0, x, y).map(|(_, node, rect)| (node, rect))
}

/// The selectable text a press at `(x, y)` starts selecting: the text under
/// the pointer, or the nearest one when the press is just off it. A press
/// where a scrollbar takes the pointer starts none, even over text or within
/// reach of it.
fn selectable_text_drag_start_endpoint<'a>(
  root: &'a Node,
  layout: &'a LayoutResult,
  x: f32,
  y: f32,
) -> Option<(&'a Node, HitRect)> {
  let mut hits = Vec::new();
  hit_test_tree(root, layout, 0.0, 0.0, x, y, &mut hits);
  if hits.iter().any(|(node, _)| scrollbar_hit_at(node, x, y).is_some()) {
    return None;
  }
  if let Some(hit) = hits.into_iter().find(|(node, _)| is_selectable_text_node(node)) {
    return Some(hit);
  }

  let (_, node, rect) = nearest_selectable_text(root, layout, 0.0, 0.0, x, y)?;
  let vertical_slop = (rect.height * 0.75).max(8.0);
  let outside_y = if y < rect.y {
    rect.y - y
  } else if y > rect.y + rect.height {
    y - (rect.y + rect.height)
  } else {
    0.0
  };

  (outside_y <= vertical_slop).then_some((node, rect))
}

fn nearest_selectable_text<'a>(
  node: &'a Node,
  layout: &'a LayoutResult,
  abs_x: f32,
  abs_y: f32,
  x: f32,
  y: f32,
) -> Option<(f32, &'a Node, HitRect)> {
  if layout.dropped {
    return None;
  }
  let mut best: Option<(f32, &'a Node, HitRect)> = None;

  for (child_layout, child_node) in layout.children.iter().zip(node.children()) {
    if let Some(candidate) = nearest_selectable_text(
      child_node,
      &child_layout.result,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      x,
      y,
    ) {
      if best
        .as_ref()
        .is_none_or(|(best_distance, ..)| candidate.0 < *best_distance)
      {
        best = Some(candidate);
      }
    }
  }

  if is_selectable_text_node(node) {
    let dx = if x < abs_x {
      abs_x - x
    } else if x > abs_x + layout.size.width {
      x - (abs_x + layout.size.width)
    } else {
      0.0
    };
    let dy = if y < abs_y {
      abs_y - y
    } else if y > abs_y + layout.size.height {
      y - (abs_y + layout.size.height)
    } else {
      0.0
    };
    let distance = dy * dy * 1024.0 + dx * dx;
    let rect = HitRect {
      x: abs_x,
      y: abs_y,
      width: layout.size.width,
      height: layout.size.height,
      local_x: x,
      local_y: y,
      transform: Transform2D::IDENTITY,
    };
    if best
      .as_ref()
      .is_none_or(|(best_distance, ..)| distance < *best_distance)
    {
      best = Some((distance, node, rect));
    }
  }

  best
}

struct SelectableTextRangeNode {
  id: NodeId,
  state: TextState,
  value: String,
}

fn set_selectable_text_range(root: &Node, start_id: NodeId, anchor: usize, end_id: NodeId, caret: usize) {
  let mut nodes = Vec::new();
  collect_selectable_text_range_nodes(root, &mut nodes);

  let Some(start_index) = nodes.iter().position(|node| node.id == start_id) else {
    return;
  };
  let Some(end_index) = nodes.iter().position(|node| node.id == end_id) else {
    return;
  };

  let range_start = start_index.min(end_index);
  let range_end = start_index.max(end_index);
  let forward = start_index <= end_index;

  for (index, node) in nodes.iter().enumerate() {
    let len = node.value.len();
    if index < range_start || index > range_end {
      node.state.clear_selection();
    } else if start_index == end_index {
      node.state.set_selection_indices(&node.value, anchor, caret);
    } else if forward {
      if index == start_index {
        node.state.set_selection_indices(&node.value, anchor, len);
      } else if index == end_index {
        node.state.set_selection_indices(&node.value, 0, caret);
      } else {
        node.state.set_selection_indices(&node.value, 0, len);
      }
    } else if index == start_index {
      node.state.set_selection_indices(&node.value, anchor, 0);
    } else if index == end_index {
      node.state.set_selection_indices(&node.value, len, caret);
    } else {
      node.state.set_selection_indices(&node.value, 0, len);
    }
  }
}

fn collect_selectable_text_range_nodes(node: &Node, nodes: &mut Vec<SelectableTextRangeNode>) {
  if let Some((state, value)) = selectable_text_state_and_value(node) {
    nodes.push(SelectableTextRangeNode {
      id: node.node_id(),
      state,
      value,
    });
  }

  for child in node.children() {
    collect_selectable_text_range_nodes(child, nodes);
  }
}

fn clear_selectable_text_selections(node: &Node) -> bool {
  clear_selectable_text_selections_except(node, None)
}

fn clear_selectable_text_selections_except(node: &Node, except: Option<NodeId>) -> bool {
  let is_except = except.is_some_and(|except| except == node.node_id());
  let mut cleared = false;

  if !is_except
    && let Some((state, value)) = selectable_text_state_and_value(node)
    && state.has_selection(&value)
  {
    state.clear_selection();
    cleared = true;
  }

  for child in node.children() {
    cleared |= clear_selectable_text_selections_except(child, except);
  }

  cleared
}

fn selectable_text_state_and_value(node: &Node) -> Option<(TextState, String)> {
  match node.node_kind() {
    NodeKind::Text { state, .. } if state.selectable() => {
      Some((state.clone(), node.text_content().unwrap_or_default().to_owned()))
    }
    #[cfg(feature = "markdown")]
    NodeKind::RichText { state, .. } if state.selectable() => Some((state.clone(), node.rich_text_value()?)),
    _ => None,
  }
}

/// The selected text of a selectable text node, without what a rich text
/// draws as presentation (a link's isolate marks and trailing runs).
fn selected_content(node: &Node, state: &TextState, value: &str) -> Option<String> {
  #[cfg(feature = "markdown")]
  if let NodeKind::RichText { .. } = node.node_kind() {
    let range = state.selected_range(value)?;
    return Some(node.rich_text_content(value, range)).filter(|text| !text.is_empty());
  }
  #[cfg(not(feature = "markdown"))]
  let _ = node;
  state.selected_text(value)
}

fn is_selectable_text_node(node: &Node) -> bool {
  selectable_text_state_and_value(node).is_some()
}

fn find_slider_by_y_recursive<'a>(
  node: &'a Node,
  layout: &'a LayoutResult,
  abs_x: f32,
  abs_y: f32,
  y: f32,
) -> Option<(&'a Node, ElementRect)> {
  for (child_layout, child_node) in layout.children.iter().zip(node.children()) {
    if let Some(found) = find_slider_by_y_recursive(
      child_node,
      &child_layout.result,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      y,
    ) {
      return Some(found);
    }
  }

  let rect = ElementRect {
    x: abs_x,
    y: abs_y,
    relative_x: 0.0,
    relative_y: 0.0,
    width: layout.size.width,
    height: layout.size.height,
  };

  if matches!(node.node_kind(), NodeKind::Slider { .. }) && y >= rect.y && y <= rect.y + rect.height {
    return Some((node, rect));
  }

  None
}

fn find_slider_by_binding_recursive<'a>(
  node: &'a Node,
  layout: &'a LayoutResult,
  abs_x: f32,
  abs_y: f32,
  binding_id: usize,
) -> Option<(&'a Node, ElementRect)> {
  for (child_layout, child_node) in layout.children.iter().zip(node.children()) {
    if let Some(found) = find_slider_by_binding_recursive(
      child_node,
      &child_layout.result,
      abs_x + child_layout.offset.x,
      abs_y + child_layout.offset.y,
      binding_id,
    ) {
      return Some(found);
    }
  }

  let NodeKind::Slider { state } = node.node_kind() else {
    return None;
  };
  if state.value_id() != binding_id {
    return None;
  }

  Some((
    node,
    ElementRect {
      x: abs_x,
      y: abs_y,
      relative_x: 0.0,
      relative_y: 0.0,
      width: layout.size.width,
      height: layout.size.height,
    },
  ))
}

fn point_in_element_rect(x: f32, y: f32, rect: ElementRect) -> bool {
  x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn fire_keyboard_recursive(node: &Node, evt: &mut KeyboardEvent) -> bool {
  evt.target_id = node.node_id();
  for handler in &node.events.on_key_down {
    handler.call(evt);
    if evt.immediate_propagation_stopped() {
      return true;
    }
  }
  if evt.propagation_stopped() {
    return true;
  }
  for child in node.children() {
    if fire_keyboard_recursive(child, evt) {
      return true;
    }
  }
  false
}

fn fire_keyboard_up_recursive(node: &Node, evt: &mut KeyboardEvent) -> bool {
  evt.target_id = node.node_id();
  for handler in &node.events.on_key_up {
    handler.call(evt);
    if evt.immediate_propagation_stopped() {
      return true;
    }
  }
  if evt.propagation_stopped() {
    return true;
  }
  for child in node.children() {
    if fire_keyboard_up_recursive(child, evt) {
      return true;
    }
  }
  false
}

#[cfg(feature = "perf_profile")]
fn ms(duration: Duration) -> f32 {
  duration.as_secs_f32() * 1000.0
}

fn transformed_text_raster_scale(transform: Transform2D) -> f32 {
  let aa = transform.a * transform.a + transform.b * transform.b;
  let bb = transform.c * transform.c + transform.d * transform.d;
  let ab = transform.a * transform.c + transform.b * transform.d;
  let trace = aa + bb;
  let det = aa * bb - ab * ab;
  let discriminant = (trace * trace - 4.0 * det).max(0.0);
  let max_scale = ((trace + discriminant.sqrt()) * 0.5).max(1.0).sqrt();
  max_scale.max(2.0).min(4.0).ceil()
}

#[cfg(feature = "devtools")]
fn push_devtools_overlay(
  rects: &mut Vec<RectCmd>,
  glyphs: &mut Vec<GlyphCmd>,
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  target: Option<DevtoolsOverlayTarget>,
  base_order: usize,
  scale: f32,
  viewport: Size,
) {
  let Some(target) = target else {
    return;
  };

  let outer = scale_devtools_rect(target.outer, scale);
  let inner = target.inner.map(|rect| scale_devtools_rect(rect, scale));
  let order = base_order + 10_000;

  if let Some(inner) = inner {
    rects.push(devtools_overlay_rect_cmd(
      order,
      outer,
      Color::new(251, 146, 60, 48),
      Color::new(251, 146, 60, 235),
    ));
    rects.push(devtools_overlay_rect_cmd(
      order + 1,
      inner,
      Color::new(96, 165, 250, 48),
      Color::new(96, 165, 250, 245),
    ));
  } else {
    rects.push(devtools_overlay_rect_cmd(
      order,
      outer,
      Color::new(96, 165, 250, 42),
      Color::new(96, 165, 250, 245),
    ));
  }

  push_devtools_size_label(
    rects,
    glyphs,
    glyph_engine,
    target.outer,
    outer,
    order + 2,
    scale,
    viewport,
  );
}

#[cfg(feature = "devtools")]
fn scale_devtools_rect(rect: DevtoolsOverlayRect, scale: f32) -> DevtoolsOverlayRect {
  DevtoolsOverlayRect {
    x: rect.x * scale,
    y: rect.y * scale,
    width: rect.width * scale,
    height: rect.height * scale,
  }
}

#[cfg(feature = "devtools")]
fn devtools_overlay_rect_cmd(order: usize, rect: DevtoolsOverlayRect, color: Color, stroke_color: Color) -> RectCmd {
  RectCmd {
    order,
    x: rect.x,
    y: rect.y,
    width: rect.width.max(0.0),
    height: rect.height.max(0.0),
    color,
    radii: [0.0; 4],
    stroke: [1.0; 4],
    stroke_color,
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0, 0.0],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  }
}

#[cfg(feature = "devtools")]
fn push_devtools_size_label(
  rects: &mut Vec<RectCmd>,
  glyphs: &mut Vec<GlyphCmd>,
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  logical_rect: DevtoolsOverlayRect,
  physical_rect: DevtoolsOverlayRect,
  order: usize,
  scale: f32,
  viewport: Size,
) {
  let label = format!("{:.0} x {:.0}", logical_rect.width, logical_rect.height);
  let style = TextStyle {
    font_size: 11.0 * scale,
    weight: FontWeight::Medium,
    color: Color::new(229, 231, 235, 255),
    ..Default::default()
  };
  let text_size = glyph_engine.measure_text(&label, &style, f32::MAX);
  let padding_x = 6.0 * scale;
  let padding_y = 3.0 * scale;
  let label_w = text_size.width + padding_x * 2.0;
  let label_h = text_size.height + padding_y * 2.0;
  let gap = 4.0 * scale;
  let outside_x = physical_rect.x.max(0.0);
  let outside_y = physical_rect.y + physical_rect.height + gap;
  let outside_fits =
    outside_x + label_w <= viewport.width && outside_y >= 0.0 && outside_y + label_h <= viewport.height;
  let (label_x, label_y) = if outside_fits {
    (outside_x, outside_y)
  } else {
    let max_x = (viewport.width - label_w).max(0.0);
    let max_y = (viewport.height - label_h).max(0.0);
    (
      (physical_rect.x + gap).clamp(0.0, max_x),
      (physical_rect.y + gap).clamp(0.0, max_y),
    )
  };

  rects.push(RectCmd {
    order,
    x: label_x,
    y: label_y,
    width: label_w,
    height: label_h,
    color: Color::new(17, 24, 39, 235),
    radii: [3.0 * scale; 4],
    stroke: [1.0; 4],
    stroke_color: Color::new(96, 165, 250, 235),
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0, 0.0],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  });

  let mut label_glyphs =
    glyph_engine.rasterize_text(&label, &style, f32::MAX, label_x + padding_x, label_y + padding_y);
  for glyph in &mut label_glyphs {
    glyph.order = order + 1;
    glyph.clip = ClipRect::default();
  }
  glyphs.extend(label_glyphs);
}

fn push_perf_meter(
  rects: &mut Vec<RectCmd>,
  glyphs: &mut Vec<GlyphCmd>,
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  stats: Option<PerfMeterStats>,
  order: usize,
  scale: f32,
  viewport: Size,
) {
  let Some(stats) = stats else {
    return;
  };

  let panel_w = 160.0 * scale;
  let panel_h = 213.0 * scale;
  let margin_top = 12.0 * scale;
  let margin_right = 16.0 * scale;
  let padding_x = 8.0 * scale;
  let padding_y = 8.0 * scale;
  let row_h = 15.0 * scale;
  let x = (viewport.width - panel_w - margin_right).max(0.0);
  let y = margin_top.max(0.0);

  rects.push(RectCmd {
    order,
    x,
    y,
    width: panel_w,
    height: panel_h,
    color: Color::from_hex("#111827"),
    radii: [6.0 * scale; 4],
    stroke: [1.0; 4],
    stroke_color: Color::from_hex("#374151"),
    transform: [1.0, 0.0, 0.0, 1.0],
    transform_origin: [0.0, 0.0],
    clip: ClipRect::default(),
    gradient: None,
    shadow: None,
  });

  let rows = [
    ("FPS", stats.fps.to_string(), FontWeight::Bold),
    ("total", format!("{:.2} ms", stats.total_ms), FontWeight::Normal),
    ("layout", format!("{:.2} ms", stats.layout_ms), FontWeight::Normal),
    (
      "resolve",
      format!("{:.2} ms", stats.quad_resolve_ms),
      FontWeight::Normal,
    ),
    ("glyph", format!("{:.2} ms", stats.glyph_ms), FontWeight::Normal),
    (
      "render cpu",
      format!("{:.2} ms", stats.render_cpu_ms),
      FontWeight::Normal,
    ),
    ("wait", format!("{:.2} ms", stats.render_acquire_ms), FontWeight::Normal),
    (
      "upload",
      format!("{:.2} ms", stats.render_upload_ms),
      FontWeight::Normal,
    ),
    (
      "encode",
      format!("{:.2} ms", stats.render_encode_ms),
      FontWeight::Normal,
    ),
    (
      "submit",
      format!("{:.2} ms", stats.render_submit_ms),
      FontWeight::Normal,
    ),
    (
      "present",
      format!("{:.2} ms", stats.render_present_ms),
      FontWeight::Normal,
    ),
    ("quads", stats.quad_count.to_string(), FontWeight::Normal),
    ("glyphs", stats.glyph_count.to_string(), FontWeight::Normal),
  ];

  for (index, (label, value, value_weight)) in rows.iter().enumerate() {
    let row_y = y + padding_y + index as f32 * row_h;
    push_raw_text(
      glyphs,
      glyph_engine,
      label,
      x + padding_x,
      row_y,
      10.0 * scale,
      FontWeight::Normal,
      Color::from_hex("#e5e7eb"),
      order + 1,
    );
    let value_size = measure_raw_text(glyph_engine, value, 10.0 * scale, *value_weight);
    push_raw_text(
      glyphs,
      glyph_engine,
      value,
      x + panel_w - padding_x - value_size.width,
      row_y,
      10.0 * scale,
      *value_weight,
      Color::from_hex("#e5e7eb"),
      order + 1,
    );
  }
}

fn measure_raw_text(
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  text: &str,
  font_size: f32,
  weight: FontWeight,
) -> Size {
  let style = TextStyle {
    font_size,
    weight,
    color: Color::new(255, 255, 255, 255),
    ..Default::default()
  };
  glyph_engine.measure_text(text, &style, f32::MAX)
}

#[allow(clippy::too_many_arguments)]
fn push_raw_text(
  glyphs: &mut Vec<GlyphCmd>,
  glyph_engine: &mut crate::app::glyph_engine::GlyphEngine,
  text: &str,
  x: f32,
  y: f32,
  font_size: f32,
  weight: FontWeight,
  color: Color,
  order: usize,
) {
  let style = TextStyle {
    font_size,
    weight,
    color,
    ..Default::default()
  };
  let mut text_glyphs = glyph_engine.rasterize_text(text, &style, f32::MAX, x, y);
  for glyph in &mut text_glyphs {
    glyph.order = order;
    glyph.clip = ClipRect::default();
  }
  glyphs.extend(text_glyphs);
}

fn apply_opacity(color: Color, opacity: f32) -> Color {
  if opacity >= 1.0 {
    return color;
  }
  let a = (color.a() as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
  Color::new(color.r(), color.g(), color.b(), a)
}

fn apply_opacity_gradient(gradient: &RenderGradient, opacity: f32) -> RenderGradient {
  if opacity >= 1.0 {
    return gradient.clone();
  }
  let factor = opacity.clamp(0.0, 1.0);
  let mut gradient = gradient.clone();
  for stop in &mut gradient.stops {
    stop.color[3] *= factor;
  }
  gradient
}

fn scaled_radii(border_radius: Option<BorderRadius>, scale: f32, width: f32, height: f32) -> [f32; 4] {
  let max_r = width.min(height) * 0.5;
  border_radius
    .map(|r| {
      [
        (r.top_left * scale).min(max_r),
        (r.top_right * scale).min(max_r),
        (r.bottom_right * scale).min(max_r),
        (r.bottom_left * scale).min(max_r),
      ]
    })
    .unwrap_or([0.0; 4])
}

fn push_border_rects(
  rects: &mut Vec<RectCmd>,
  order: usize,
  base_x: f32,
  base_y: f32,
  base_w: f32,
  base_h: f32,
  scale: f32,
  border_radius: Option<BorderRadius>,
  borders: ResolvedBorders,
  opacity: f32,
  transform: [f32; 4],
  transform_origin: [f32; 2],
  clip: ClipRect,
) {
  let origin_abs = [base_x + transform_origin[0], base_y + transform_origin[1]];
  let sides = [borders.top, borders.right, borders.bottom, borders.left];
  let mut used = [false; 4];

  for side in 0..4 {
    let Some(border) = sides[side] else {
      continue;
    };
    if used[side] {
      continue;
    }

    let mut stroke = [0.0; 4];
    for other_side in side..4 {
      if sides[other_side] == Some(border) {
        used[other_side] = true;
        stroke[other_side] = border.width * scale;
      }
    }

    push_border_rect(
      rects,
      order,
      base_x,
      base_y,
      base_w,
      base_h,
      scale,
      border_radius,
      border,
      stroke,
      opacity,
      transform,
      origin_abs,
      clip,
    );
  }
}

#[allow(clippy::too_many_arguments)]
fn push_border_rect(
  rects: &mut Vec<RectCmd>,
  order: usize,
  base_x: f32,
  base_y: f32,
  base_w: f32,
  base_h: f32,
  scale: f32,
  border_radius: Option<BorderRadius>,
  border: ResolvedBorder,
  stroke: [f32; 4],
  opacity: f32,
  transform: [f32; 4],
  origin_abs: [f32; 2],
  clip: ClipRect,
) {
  if stroke.iter().all(|width| *width <= 0.0) {
    return;
  }

  if let Some(side) = single_stroke_side(stroke) {
    push_single_side_border_rect(
      rects,
      order,
      base_x,
      base_y,
      base_w,
      base_h,
      border,
      side,
      stroke[side],
      opacity,
      transform,
      origin_abs,
      clip,
    );
    return;
  }

  let (mut x, mut y, mut w, mut h) = (base_x, base_y, base_w, base_h);
  match border.placement {
    BorderPlacement::Outside => {
      x -= stroke[3];
      y -= stroke[0];
      w += stroke[1] + stroke[3];
      h += stroke[0] + stroke[2];
    }
    BorderPlacement::Center => {
      x -= stroke[3] * 0.5;
      y -= stroke[0] * 0.5;
      w += (stroke[1] + stroke[3]) * 0.5;
      h += (stroke[0] + stroke[2]) * 0.5;
    }
    _ => {}
  }

  rects.push(RectCmd {
    order,
    x,
    y,
    width: w,
    height: h,
    color: TRANSPARENT_COLOR,
    radii: scaled_radii(border_radius, scale, w, h),
    stroke,
    stroke_color: apply_opacity(border.color, opacity),
    transform,
    transform_origin: [origin_abs[0] - x, origin_abs[1] - y],
    clip,
    gradient: None,
    shadow: None,
  });
}

fn single_stroke_side(stroke: [f32; 4]) -> Option<usize> {
  let mut side = None;
  for (index, width) in stroke.iter().enumerate() {
    if *width <= 0.0 {
      continue;
    }
    if side.is_some() {
      return None;
    }
    side = Some(index);
  }
  side
}

#[allow(clippy::too_many_arguments)]
fn push_single_side_border_rect(
  rects: &mut Vec<RectCmd>,
  order: usize,
  base_x: f32,
  base_y: f32,
  base_w: f32,
  base_h: f32,
  border: ResolvedBorder,
  side: usize,
  width: f32,
  opacity: f32,
  transform: [f32; 4],
  origin_abs: [f32; 2],
  clip: ClipRect,
) {
  let (x, y, w, h) = match (side, border.placement) {
    (0, BorderPlacement::Outside) => (base_x, base_y - width, base_w, width),
    (0, BorderPlacement::Center) => (base_x, base_y - width * 0.5, base_w, width),
    (0, BorderPlacement::Inside) => (base_x, base_y, base_w, width),
    (1, BorderPlacement::Outside) => (base_x + base_w, base_y, width, base_h),
    (1, BorderPlacement::Center) => (base_x + base_w - width * 0.5, base_y, width, base_h),
    (1, BorderPlacement::Inside) => (base_x + base_w - width, base_y, width, base_h),
    (2, BorderPlacement::Outside) => (base_x, base_y + base_h, base_w, width),
    (2, BorderPlacement::Center) => (base_x, base_y + base_h - width * 0.5, base_w, width),
    (2, BorderPlacement::Inside) => (base_x, base_y + base_h - width, base_w, width),
    (3, BorderPlacement::Outside) => (base_x - width, base_y, width, base_h),
    (3, BorderPlacement::Center) => (base_x - width * 0.5, base_y, width, base_h),
    (3, BorderPlacement::Inside) => (base_x, base_y, width, base_h),
    _ => return,
  };

  rects.push(RectCmd {
    order,
    x,
    y,
    width: w,
    height: h,
    color: apply_opacity(border.color, opacity),
    radii: [0.0; 4],
    stroke: [0.0; 4],
    stroke_color: TRANSPARENT_COLOR,
    transform,
    transform_origin: [origin_abs[0] - x, origin_abs[1] - y],
    clip,
    gradient: None,
    shadow: None,
  });
}

fn rect_intersects_clip(x: f32, y: f32, width: f32, height: f32, clip: ClipRect) -> bool {
  x < clip.x + clip.width && x + width > clip.x && y < clip.y + clip.height && y + height > clip.y
}

/// Duplicate the glyphs appended since `glyph_start` as shadow instances and
/// splice them in front so they render beneath the text within the same order.
fn append_text_shadow_glyphs(
  glyphs: &mut Vec<GlyphCmd>,
  glyph_start: usize,
  shadow: &crate::layout::text_style::TextShadow,
  scale: f32,
) {
  // Bounds the per-fragment blur loop in the glyph shaders.
  const MAX_SHADOW_SIGMA: f32 = 16.0;
  if !shadow.is_visible() || glyphs.len() == glyph_start {
    return;
  }
  let dx = shadow.offset_x * scale;
  let dy = shadow.offset_y * scale;
  // CSS blur radius equals twice the Gaussian standard deviation.
  let sigma = (shadow.blur_radius * scale * 0.5).min(MAX_SHADOW_SIGMA);
  let color = shadow.color.to_linear_f32_array();
  let shadows: Vec<GlyphCmd> = glyphs[glyph_start..]
    .iter()
    .map(|glyph| {
      let mut cmd = glyph.clone();
      cmd.x += dx;
      cmd.y += dy;
      // Keep the rotation centre at the same absolute point despite the offset.
      cmd.transform_origin = [glyph.transform_origin[0] - dx, glyph.transform_origin[1] - dy];
      cmd.color = color;
      cmd.color_glyph = false;
      cmd.sharpness = 1.0;
      cmd.shadow_sigma = sigma;
      cmd
    })
    .collect();
  glyphs.splice(glyph_start..glyph_start, shadows);
}

fn expand_text_clip_for_rasterization(clip: ClipRect) -> ClipRect {
  if !clip.active {
    return clip;
  }

  const RASTERIZATION_SLOP_PX: f32 = 1.0;
  ClipRect {
    x: clip.x - RASTERIZATION_SLOP_PX,
    y: clip.y - RASTERIZATION_SLOP_PX,
    width: clip.width + RASTERIZATION_SLOP_PX * 2.0,
    height: clip.height + RASTERIZATION_SLOP_PX * 2.0,
    active: true,
    border_radius: clip.border_radius,
  }
}

fn expand_text_clip_for_culling(clip: ClipRect) -> ClipRect {
  if !clip.active {
    return clip;
  }

  const TEXT_CULL_OVERSCAN_PX: f32 = 96.0;
  ClipRect {
    x: clip.x - TEXT_CULL_OVERSCAN_PX,
    y: clip.y - TEXT_CULL_OVERSCAN_PX,
    width: clip.width + TEXT_CULL_OVERSCAN_PX * 2.0,
    height: clip.height + TEXT_CULL_OVERSCAN_PX * 2.0,
    active: true,
    border_radius: clip.border_radius,
  }
}

#[cfg(test)]
mod tests {
  use crate::{
    app::runtime::{expand_text_clip_for_culling, expand_text_clip_for_rasterization, transformed_text_raster_scale},
    layout::quad::ClipRect,
    node::transform::Transform2D,
  };

  #[test]
  fn text_clip_expands_by_one_physical_pixel_for_glyph_rasterization() {
    let clip = ClipRect {
      x: 10.0,
      y: 20.0,
      width: 30.0,
      height: 40.0,
      active: true,
      border_radius: None,
    };

    let expanded = expand_text_clip_for_rasterization(clip);

    assert_eq!(expanded.x, 9.0);
    assert_eq!(expanded.y, 19.0);
    assert_eq!(expanded.width, 32.0);
    assert_eq!(expanded.height, 42.0);
    assert!(expanded.active);
  }

  #[test]
  fn inactive_text_clip_stays_inactive() {
    let clip = ClipRect::default();

    let expanded = expand_text_clip_for_rasterization(clip);

    assert!(!expanded.active);
  }

  #[test]
  fn text_cull_clip_expands_for_scroll_prewarm() {
    let clip = ClipRect {
      x: 10.0,
      y: 20.0,
      width: 30.0,
      height: 40.0,
      active: true,
      border_radius: None,
    };

    let expanded = expand_text_clip_for_culling(clip);

    assert_eq!(expanded.x, -86.0);
    assert_eq!(expanded.y, -76.0);
    assert_eq!(expanded.width, 222.0);
    assert_eq!(expanded.height, 232.0);
    assert!(expanded.active);
  }

  #[test]
  fn transformed_text_bitmap_mode_uses_moderate_minimum_raster_scale() {
    let transform = Transform2D::rotate_deg(-2.0).then(&Transform2D::scale(1.02, 1.02));

    assert_eq!(transformed_text_raster_scale(transform), 2.0);
  }

  #[cfg(feature = "devtools")]
  #[test]
  fn screenshot_glyph_encodes_linear_color_back_to_srgb() {
    use crate::{
      app::runtime::{DevtoolsScreenshotBounds, draw_screenshot_glyph},
      layout::render_list::{GlyphAtlas, GlyphCmd},
      node::color::Color,
    };

    let source = Color::new(128, 128, 128, 255);
    let glyph = GlyphCmd {
      order: 0,
      x: 0.0,
      y: 0.0,
      width: 1.0,
      height: 1.0,
      color: source.to_linear_f32_array(),
      atlas_min: [0.0, 0.0],
      atlas_max: [1.0, 1.0],
      transform: [1.0, 0.0, 0.0, 1.0],
      transform_origin: [0.0, 0.0],
      sharpness: 1.0,
      color_glyph: false,
      shadow_sigma: 0.0,
      clip: ClipRect::default(),
    };
    let atlas = GlyphAtlas {
      data: std::sync::Arc::from([255_u8, 255, 255, 255].as_slice()),
      width: 1,
      height: 1,
      version: 0,
      dirty_rects: std::sync::Arc::from([].as_slice()),
      dirty_from_version: 0,
    };
    let bounds = DevtoolsScreenshotBounds {
      x: 0,
      y: 0,
      width: 1,
      height: 1,
      window_clip: None,
    };

    let mut pixels = vec![0_u8, 0, 0, 255];
    draw_screenshot_glyph(&mut pixels, bounds, &glyph, &atlas);

    assert_eq!(&pixels, &[source.r(), source.g(), source.b(), 255]);
  }
}
