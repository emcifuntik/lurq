#[cfg(feature = "perf_profile")]
use std::time::{Duration, Instant};
use std::{
  cell::{Cell, RefCell},
  sync::Arc,
};

use crate::{
  app::{
    ctx::{ModalSpec, OverlaySpec},
    glyph_engine::GlyphEngine,
    theme::{
      CaretMode, PaletteColor, Theme, ThemeBorderSizes, ThemeCaret, ThemePalette, ThemeRadii, ThemeShadows,
      ThemeSpacing, ThemeTypography,
    },
  },
  core::{ElementRect, ElementRef},
  layout::{
    Alignment, Constraints, Offset, Size, StackAlignment,
    layout_kind::{
      FlexWrap, FrameConstraints, Justify, LayoutKind, Overflow, Position, ScrollAxis, ScrollDirection, ScrollState,
      ShrinkLimit,
    },
    layout_result::{ChildLayout, LayoutResult, SubtreeHolds},
    quad::{ClipRect, Quad, QuadContent},
    scrollbar::{ScrollBarPlacement, ScrollBarStyle, ScrollBarVisibility},
    text_style::TextStyle,
  },
  node::{
    CheckboxStyle,
    border::{BorderPlacement, BorderRadius, ResolvedBorder, ResolvedBorders},
    color::Color,
    dimension::Dimension,
    node::Node,
    node_kind::{NodeKind, SelectState, SliderPartRect, TextInputOverflow, TextInputState, TextOverflow},
    padding::Padding,
    slider_style::SliderPartStyle,
    transform::Transform2D,
  },
};

mod box_shadow_quads;
mod control_fill;
mod flex_shrink;
#[cfg(feature = "markdown")]
pub(crate) mod inline_links;
mod input_layout;
pub(crate) mod input_token_layout;
mod input_token_quads;
mod opacity_groups;
mod select_quads;
mod selection_layout;
#[cfg(feature = "screenshot")]
mod sensitive_quads;
mod shrink_distribution;
mod shrink_floors;
mod shrink_sharing;
#[cfg(feature = "markdown")]
mod styled_input_layout;
mod text_highlights;
mod text_input_quads;
mod text_layout_output;

use flex_shrink::{FlexShrinkLine, FreeRoom, LineOverflow, main_size, occupies_line};
pub(crate) use text_layout_output::TextLayoutOutput;
use text_layout_output::{TextInputOutput, TextOutput};

const DEFAULT_CHECKBOX_WIDTH: f32 = 18.0;
const DEFAULT_CHECKBOX_HEIGHT: f32 = 18.0;
const DEFAULT_SLIDER_WIDTH: f32 = 120.0;
const DEFAULT_SLIDER_HEIGHT: f32 = 20.0;
const DEFAULT_SLIDER_THUMB_MIN_SIZE: f32 = 12.0;
const DEFAULT_TEXT_INPUT_WIDTH: f32 = 120.0;
const DEFAULT_SELECT_WIDTH: f32 = 160.0;
const DEFAULT_SELECT_HEIGHT: f32 = 32.0;
#[cfg(any(feature = "raster", all(feature = "svg", feature = "resources")))]
const DEFAULT_RESOURCE_WIDTH: f32 = 0.0;
#[cfg(any(feature = "raster", all(feature = "svg", feature = "resources")))]
const DEFAULT_RESOURCE_HEIGHT: f32 = 0.0;
const DEFAULT_QUAD_OPACITY: f32 = 1.0;
#[cfg(feature = "perf_profile")]
const SLOW_LAYOUT_NODE_THRESHOLD: Duration = Duration::from_millis(4);

#[cfg(feature = "perf_profile")]
fn layout_kind_profile_name(kind: &LayoutKind) -> &'static str {
  match kind {
    LayoutKind::Leaf => "leaf",
    LayoutKind::Row { .. } => "row",
    LayoutKind::Column { .. } => "column",
    LayoutKind::Stack { .. } => "stack",
    LayoutKind::LogicalModifier => "logical_modifier",
    LayoutKind::ScrollModifier { .. } => "scroll_modifier",
  }
}

const DEFAULT_TRANSPARENT_COLOR: Color = Color::new(0, 0, 0, 0);
const DEFAULT_TEXT_SELECTION_COLOR: Color = Color::new(191, 219, 254, 255);
/// Width of a text input's caret, in logical pixels.
const TEXT_INPUT_CARET_WIDTH: f32 = 1.0;
fn text_input_display_style<'a>(
  state: &crate::node::node_kind::TextInputState,
  style: &'a TextStyle,
  placeholder_style: Option<&'a TextStyle>,
) -> &'a TextStyle {
  if state.is_showing_placeholder() {
    placeholder_style.unwrap_or(style)
  } else {
    style
  }
}

fn text_input_vertical_offset(state: &TextInputState, height: f32) -> f32 {
  if state.overflow() == TextInputOverflow::Scroll {
    ((height - state.caret_height()).max(0.0)) * 0.5
  } else {
    0.0
  }
}

fn bounded_text_width(width: f32) -> bool {
  width.is_finite() && width < f32::MAX
}

fn constraints_are_tight(constraints: Constraints) -> bool {
  constraints.min_width == constraints.max_width && constraints.min_height == constraints.max_height
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ResolvedPadding {
  pub(crate) left: f32,
  pub(crate) top: f32,
  pub(crate) right: f32,
  pub(crate) bottom: f32,
}

#[derive(Clone)]
struct ChildLayoutOverride {
  constraints: Constraints,
  result: Arc<LayoutResult>,
}

#[derive(Default)]
pub(crate) struct OverlayLayoutIndex {
  pub(crate) elements: Vec<ElementLayoutRecord>,
  pub(crate) overlays: Vec<OverlayLayoutRecord>,
  /// The anchors of the dropdowns declared in the tree, open or closed.
  pub(crate) dropdown_anchors: Vec<ElementRef>,
}

#[derive(Clone)]
pub(crate) struct ElementLayoutRecord {
  pub(crate) element_ref: ElementRef,
  pub(crate) rect: ElementRect,
  /// Whether any of the element shows through the clips of its scroll
  /// containers and `clip()`ped ancestors. An element scrolled out of its
  /// container's viewport is laid out, but not visible.
  pub(crate) visible: bool,
}

pub(crate) enum OverlayLayoutRecord {
  SelectMenu {
    reuse_key: Option<Arc<str>>,
    state: SelectState,
    bounds: ElementRect,
    /// Whether the select shows through its ancestors' clips; see
    /// [`ElementLayoutRecord::visible`].
    visible: bool,
  },
  Overlay {
    reuse_key: Option<Arc<str>>,
    spec: OverlaySpec,
  },
  Modal {
    reuse_key: Option<Arc<str>>,
    spec: ModalSpec,
    parent: ElementRect,
  },
}

impl Clone for OverlayLayoutIndex {
  fn clone(&self) -> Self {
    Self {
      elements: self.elements.clone(),
      overlays: self.overlays.clone(),
      dropdown_anchors: self.dropdown_anchors.clone(),
    }
  }
}

impl Clone for OverlayLayoutRecord {
  fn clone(&self) -> Self {
    match self {
      Self::SelectMenu {
        reuse_key,
        state,
        bounds,
        visible,
      } => Self::SelectMenu {
        reuse_key: reuse_key.clone(),
        state: state.clone(),
        bounds: *bounds,
        visible: *visible,
      },
      Self::Overlay { reuse_key, spec } => Self::Overlay {
        reuse_key: reuse_key.clone(),
        spec: spec.clone_for_reuse(),
      },
      Self::Modal {
        reuse_key,
        spec,
        parent,
      } => Self::Modal {
        reuse_key: reuse_key.clone(),
        spec: spec.clone_for_reuse(),
        parent: *parent,
      },
    }
  }
}

pub(crate) struct LayoutEngine {
  last_recalculated: Cell<bool>,
  text_input_caret_visible: Cell<bool>,
  /// The theme the values below were copied from, and its version then
  /// (`set_theme`). `None` until a theme is set: the values are the defaults.
  theme_seen: RefCell<Option<(Theme, u64)>>,
  palette: RefCell<ThemePalette>,
  border_sizes: RefCell<ThemeBorderSizes>,
  spacing: RefCell<ThemeSpacing>,
  radii: RefCell<ThemeRadii>,
  caret: RefCell<ThemeCaret>,
  scrollbar: RefCell<ScrollBarStyle>,
  typography: RefCell<ThemeTypography>,
  shadows: RefCell<Arc<ThemeShadows>>,
  /// How the marks of styled text inputs look.
  #[cfg(feature = "markdown")]
  text_marks: RefCell<Arc<crate::app::theme::ThemeTextMarks>>,
  /// Open state of the `Select` whose trigger is being painted, if any.
  select_open: Cell<Option<bool>>,
  /// Caret of the focused text input recorded by the last quad resolution,
  /// where an input method places its candidate window.
  ime_cursor_area: Cell<Option<crate::app::events::ImeCursorArea>>,
  /// Opacity groups recorded by the last quad resolution.
  opacity_groups: RefCell<Vec<crate::layout::opacity_layer::OpacityGroup>>,
  /// Indices of the sensitive text quads of the last quad resolution, in
  /// ascending order (`sensitive_quads`).
  #[cfg(feature = "screenshot")]
  sensitive_quads: RefCell<Vec<usize>>,
  /// The lines being laid out collapsed by their parent's shrink step
  /// (`flex_shrink::is_collapsed`).
  collapsed_lines: RefCell<Vec<usize>>,
  /// What a flex child holds in its line's `child_layouts` until the line
  /// knows its share of the free room (`layout_flex`). Told apart by address;
  /// never left in a finished result.
  awaiting_room: Arc<LayoutResult>,
  /// Device pixels per logical pixel of the paint, for geometry that must
  /// match painted glyphs (rich-text links and underlines).
  paint_scale: Cell<f32>,
}

#[cfg(feature = "raster")]
struct BackgroundImagePlacement {
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  uv_min: [f32; 2],
  uv_max: [f32; 2],
}

#[cfg(feature = "raster")]
fn background_image_placement(
  size_mode: crate::node::BackgroundSize,
  box_width: f32,
  box_height: f32,
  image_width: f32,
  image_height: f32,
) -> BackgroundImagePlacement {
  let full = BackgroundImagePlacement {
    x: 0.0,
    y: 0.0,
    width: box_width,
    height: box_height,
    uv_min: [0.0, 0.0],
    uv_max: [1.0, 1.0],
  };

  if box_width <= 0.0 || box_height <= 0.0 || image_width <= 0.0 || image_height <= 0.0 {
    return full;
  }

  match size_mode {
    crate::node::BackgroundSize::Stretch => full,
    crate::node::BackgroundSize::Contain => {
      let scale = (box_width / image_width).min(box_height / image_height);
      let width = image_width * scale;
      let height = image_height * scale;
      BackgroundImagePlacement {
        x: (box_width - width) * 0.5,
        y: (box_height - height) * 0.5,
        width,
        height,
        uv_min: [0.0, 0.0],
        uv_max: [1.0, 1.0],
      }
    }
    crate::node::BackgroundSize::Cover => {
      let box_aspect = box_width / box_height;
      let image_aspect = image_width / image_height;
      let mut uv_min = [0.0, 0.0];
      let mut uv_max = [1.0, 1.0];

      if image_aspect > box_aspect {
        let visible_u = box_aspect / image_aspect;
        uv_min[0] = (1.0 - visible_u) * 0.5;
        uv_max[0] = 1.0 - uv_min[0];
      } else if image_aspect < box_aspect {
        let visible_v = image_aspect / box_aspect;
        uv_min[1] = (1.0 - visible_v) * 0.5;
        uv_max[1] = 1.0 - uv_min[1];
      }

      BackgroundImagePlacement { uv_min, uv_max, ..full }
    }
  }
}

fn push_slider_part_quads(
  quads: &mut Vec<Quad>,
  mut rect: SliderPartRect,
  style: &SliderPartStyle,
  color: Color,
  border_radius: Option<BorderRadius>,
  border: Option<ResolvedBorders>,
  opacity: f32,
  transform: Transform2D,
  clip: ClipRect,
) {
  if transform.is_identity() {
    rect.x = rect.x.round();
    rect.y = rect.y.round();
  }

  #[cfg(not(feature = "raster"))]
  let _ = style;
  #[cfg(feature = "raster")]
  let has_image = style.background_image.is_some();
  #[cfg(not(feature = "raster"))]
  let has_image = false;

  let (rect_x, rect_y, rect_transform, rect_transform_origin) = transformed_quad_frame(rect.x, rect.y, transform);
  quads.push(Quad {
    x: rect_x,
    y: rect_y,
    width: rect.width,
    height: rect.height,
    opacity,
    transform: rect_transform,
    transform_origin: rect_transform_origin,
    content: QuadContent::Rect { color, gradient: None },
    border_radius,
    border: if has_image { None } else { border },
    clip,
  });

  #[cfg(feature = "raster")]
  if let Some(ref bg_image) = style.background_image {
    let placement = background_image_placement(
      style.background_size,
      rect.width,
      rect.height,
      bg_image.width() as f32,
      bg_image.height() as f32,
    );
    let image_x = rect.x + placement.x;
    let image_y = rect.y + placement.y;
    let (image_x, image_y, image_transform, image_transform_origin) =
      transformed_quad_frame(image_x, image_y, transform);
    quads.push(Quad {
      x: image_x,
      y: image_y,
      width: placement.width,
      height: placement.height,
      opacity,
      transform: image_transform,
      transform_origin: image_transform_origin,
      content: QuadContent::Image {
        data: bg_image.clone(),
        uv_min: placement.uv_min,
        uv_max: placement.uv_max,
      },
      border_radius,
      border: None,
      clip,
    });
  }

  if has_image && border.is_some() {
    let (rect_x, rect_y, rect_transform, rect_transform_origin) = transformed_quad_frame(rect.x, rect.y, transform);
    quads.push(Quad {
      x: rect_x,
      y: rect_y,
      width: rect.width,
      height: rect.height,
      opacity,
      transform: rect_transform,
      transform_origin: rect_transform_origin,
      content: QuadContent::Rect {
        color: DEFAULT_TRANSPARENT_COLOR,
        gradient: None,
      },
      border_radius,
      border,
      clip,
    });
  }
}

fn push_checkbox_quads(
  quads: &mut Vec<Quad>,
  rect: SliderPartRect,
  style: &CheckboxStyle,
  color: Color,
  border_radius: Option<BorderRadius>,
  border: Option<ResolvedBorders>,
  checked: bool,
  opacity: f32,
  transform: Transform2D,
  clip: ClipRect,
) {
  #[cfg(not(feature = "raster"))]
  let _ = (style, checked);

  let (rect_x, rect_y, rect_transform, rect_transform_origin) = transformed_quad_frame(rect.x, rect.y, transform);
  quads.push(Quad {
    x: rect_x,
    y: rect_y,
    width: rect.width,
    height: rect.height,
    opacity,
    transform: rect_transform,
    transform_origin: rect_transform_origin,
    content: QuadContent::Rect { color, gradient: None },
    border_radius,
    border,
    clip,
  });

  #[cfg(feature = "raster")]
  if checked && let Some(ref indicator_image) = style.indicator_image {
    let indicator_width = style
      .indicator_width
      .unwrap_or(rect.width * 0.65)
      .min(rect.width)
      .max(0.0);
    let indicator_height = style
      .indicator_height
      .unwrap_or(rect.height * 0.65)
      .min(rect.height)
      .max(0.0);
    let indicator_x = rect.x + (rect.width - indicator_width) * 0.5;
    let indicator_y = rect.y + (rect.height - indicator_height) * 0.5;
    let placement = background_image_placement(
      style.indicator_size,
      indicator_width,
      indicator_height,
      indicator_image.width() as f32,
      indicator_image.height() as f32,
    );
    let image_x = indicator_x + placement.x;
    let image_y = indicator_y + placement.y;
    let (image_x, image_y, image_transform, image_transform_origin) =
      transformed_quad_frame(image_x, image_y, transform);
    quads.push(Quad {
      x: image_x,
      y: image_y,
      width: placement.width,
      height: placement.height,
      opacity,
      transform: image_transform,
      transform_origin: image_transform_origin,
      content: QuadContent::Image {
        data: indicator_image.clone(),
        uv_min: placement.uv_min,
        uv_max: placement.uv_max,
      },
      border_radius: None,
      border: None,
      clip,
    });
  }
}

fn quad_transform(transform: Transform2D) -> Transform2D {
  transform.linear_part()
}

fn transformed_quad_frame(x: f32, y: f32, transform: Transform2D) -> (f32, f32, Transform2D, Option<[f32; 2]>) {
  if transform.is_identity() {
    return (x, y, Transform2D::IDENTITY, None);
  }

  let (x, y) = transform.transform_point(x, y);
  let linear = quad_transform(transform);
  let origin = if linear.is_identity() { None } else { Some([0.0, 0.0]) };
  (x, y, linear, origin)
}

impl LayoutEngine {
  pub(crate) fn new() -> Self {
    Self {
      last_recalculated: Cell::new(false),
      text_input_caret_visible: Cell::new(true),
      theme_seen: RefCell::new(None),
      palette: RefCell::new(ThemePalette::default()),
      border_sizes: RefCell::new(ThemeBorderSizes::default()),
      spacing: RefCell::new(ThemeSpacing::default()),
      radii: RefCell::new(ThemeRadii::default()),
      caret: RefCell::new(ThemeCaret::default()),
      scrollbar: RefCell::new(ScrollBarStyle::default()),
      typography: RefCell::new(ThemeTypography::default()),
      shadows: RefCell::new(Arc::new(ThemeShadows::default())),
      #[cfg(feature = "markdown")]
      text_marks: RefCell::new(Arc::new(crate::app::theme::ThemeTextMarks::default())),
      select_open: Cell::new(None),
      ime_cursor_area: Cell::new(None),
      opacity_groups: RefCell::new(Vec::new()),
      #[cfg(feature = "screenshot")]
      sensitive_quads: RefCell::new(Vec::new()),
      collapsed_lines: RefCell::new(Vec::new()),
      awaiting_room: Arc::new(LayoutResult {
        size: Size::default(),
        children: Vec::new(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      }),
      paint_scale: Cell::new(1.0),
    }
  }

  /// Device pixels per logical pixel the paint draws at. Laid-out link and
  /// underline geometry depends on it; changing it needs a full relayout.
  pub(crate) fn set_paint_scale(&self, scale: f32) {
    self.paint_scale.set(scale);
  }

  /// Takes the theme values layout and paint read until the next call. They
  /// are copied only when `theme` is another theme, or the same one at
  /// another version, than the last call's: a pass on an unchanged theme
  /// copies nothing. Changing the values does not mark any node dirty; a
  /// caller that switches themes lays out with `force_dirty`.
  pub(crate) fn set_theme(&self, theme: &Theme) {
    let version = theme.version();
    if self
      .theme_seen
      .borrow()
      .as_ref()
      .is_some_and(|(seen, seen_version)| *seen_version == version && seen.same_theme(theme))
    {
      return;
    }
    *self.palette.borrow_mut() = theme.palette().clone();
    *self.border_sizes.borrow_mut() = theme.border_sizes().clone();
    *self.spacing.borrow_mut() = theme.spacing().clone();
    *self.radii.borrow_mut() = theme.radii().clone();
    *self.caret.borrow_mut() = *theme.caret();
    *self.scrollbar.borrow_mut() = theme.scrollbar().clone();
    *self.typography.borrow_mut() = theme.typography().clone();
    *self.shadows.borrow_mut() = theme.shared_shadows();
    #[cfg(feature = "markdown")]
    {
      *self.text_marks.borrow_mut() = theme.shared_text_marks();
    }
    // The version read before the copies: a change made meanwhile shows as
    // a newer version next call and is copied then.
    *self.theme_seen.borrow_mut() = Some((theme.clone(), version));
  }

  /// Lays `node` out under the theme of the last [`Self::set_theme`] (the
  /// defaults before any).
  pub(crate) fn compute(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    force_dirty: bool,
  ) -> Arc<LayoutResult> {
    self.compute_inner(glyph_engine, node, constraints, force_dirty)
  }

  pub(crate) fn compute_with_overlay_index(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    force_dirty: bool,
  ) -> (Arc<LayoutResult>, OverlayLayoutIndex) {
    let result = self.compute_inner(glyph_engine, node, constraints, force_dirty);
    let index = Self::collect_overlay_index(node, &result);
    (result, index)
  }

  fn compute_inner(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    force_dirty: bool,
  ) -> Arc<LayoutResult> {
    self.last_recalculated.set(false);
    Self::mark_layout_dirty(node, force_dirty);
    let result = self.layout_node(glyph_engine, node, constraints);
    node.clear_guards();
    result
  }

  fn collect_overlay_index(node: &Node, layout: &LayoutResult) -> OverlayLayoutIndex {
    let mut index = OverlayLayoutIndex::default();
    Self::collect_overlay_index_recursive(node, layout, 0.0, 0.0, 0.0, 0.0, ClipRect::default(), &mut index);
    index
  }

  fn collect_overlay_index_recursive(
    node: &Node,
    layout: &LayoutResult,
    abs_x: f32,
    abs_y: f32,
    parent_x: f32,
    parent_y: f32,
    clip: ClipRect,
    index: &mut OverlayLayoutIndex,
  ) {
    let rect = ElementRect {
      x: abs_x,
      y: abs_y,
      relative_x: abs_x - parent_x,
      relative_y: abs_y - parent_y,
      width: layout.size.width,
      height: layout.size.height,
    };
    let visible = !clip.active || rect_intersects_clip(rect.x, rect.y, rect.width, rect.height, clip);

    Self::index_node_layout(node, rect, visible, index);
    let child_clip = intersect_clip(clip, children_clip(node, rect));

    for (child_layout, child) in layout.children.iter().zip(node.children()) {
      if child.modal_declaration().is_some() {
        Self::index_child_modal(child, rect, index);
      }

      Self::collect_overlay_index_recursive(
        child,
        &child_layout.result,
        abs_x + child_layout.offset.x,
        abs_y + child_layout.offset.y,
        abs_x,
        abs_y,
        child_clip,
        index,
      );
    }
  }

  /// Records a node's element ref, dropdown anchor, open select menu and
  /// overlay. Split from the recursion in
  /// [`Self::collect_overlay_index_recursive`] so that the records, which
  /// carry whole nodes, are not held in every level's frame in an unoptimized
  /// build (lurq#25).
  #[inline(never)]
  fn index_node_layout(node: &Node, rect: ElementRect, visible: bool, index: &mut OverlayLayoutIndex) {
    index.dropdown_anchors.extend(node.dropdown_anchor().cloned());
    if let Some(element_ref) = node.element_ref.as_ref() {
      index.elements.push(ElementLayoutRecord {
        element_ref: element_ref.clone(),
        rect,
        visible,
      });
    }

    if let NodeKind::Select { state } = node.node_kind()
      && state.is_open()
    {
      index.overlays.push(OverlayLayoutRecord::SelectMenu {
        reuse_key: node
          .node_id()
          .is_assigned()
          .then(|| Arc::<str>::from(format!("select:{}", node.node_id().value()))),
        state: state.clone(),
        bounds: rect,
        visible,
      });
    }

    if let Some(spec) = node.overlay_declaration() {
      index.overlays.push(OverlayLayoutRecord::Overlay {
        reuse_key: spec
          .node
          .component_key()
          .map(|key| Arc::<str>::from(format!("overlay:{key}"))),
        spec: spec.clone_for_reuse(),
      });
    }
  }

  #[inline(never)]
  fn index_child_modal(child: &Node, parent: ElementRect, index: &mut OverlayLayoutIndex) {
    if let Some(spec) = child.modal_declaration() {
      index.overlays.push(OverlayLayoutRecord::Modal {
        reuse_key: spec
          .node
          .component_key()
          .map(|key| Arc::<str>::from(format!("modal:{key}"))),
        spec: spec.clone_for_reuse(),
        parent,
      });
    }
  }

  #[cfg_attr(not(feature = "perf_profile"), allow(dead_code))]
  pub(crate) fn last_recalculated(&self) -> bool {
    self.last_recalculated.get()
  }

  pub(crate) fn set_text_input_caret_visible(&self, visible: bool) {
    self.text_input_caret_visible.set(visible);
  }

  fn should_render_text_input_caret(&self, node: &Node) -> bool {
    match node.caret_mode_value().unwrap_or_else(|| self.caret.borrow().mode()) {
      CaretMode::Persistent => true,
      CaretMode::Blinking => self.text_input_caret_visible.get(),
    }
  }

  fn mark_layout_dirty(node: &Node, force_dirty: bool) -> bool {
    node.prepare_layout_tracking();
    if !force_dirty && !node.layout_cache.needs_dirty_scan() {
      return node.layout_cache.is_dirty();
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    // Overlay/modal declaration nodes are invisible to layout: flex/stack
    // walks skip them and their content is laid out inside the overlay host
    // instead. They never receive a cached result, so any dirt attributed to
    // them here could never be cleared by a store — it would wedge every
    // screen containing a popup (open or closed) into full relayout forever.
    if node.is_overlay_declaration() {
      return false;
    }
    // A node without a cached result has either never been laid out or had its
    // cache invalidated by a layout-affecting change (e.g. a reconciled subtree
    // patched in via a component slot replacement). It must be laid out, and its
    // ancestors must recompute to reposition it, so propagate dirtiness upward.
    let generation = node.layout_cache.watch().generation();
    let text_input_dirty = match node.node_kind() {
      // A multiline input scrolled by wheel or scrollbar lays out again to
      // keep its offset in its text, and stops following its caret.
      NodeKind::TextInput { state, .. } => state.take_layout_dirty() | state.take_user_scroll(),
      NodeKind::Select { state } => state.take_layout_dirty(),
      _ => false,
    };
    // Shared handles can be measured by a second tree first. Its walk may
    // consume the handle's flag, but cannot consume this node's notification.
    let local_changed = node.layout_cache.inspected_generation.get() != 0
      && node
        .layout_cache
        .watch()
        .local_changed_since(node.layout_cache.inspected_generation.get());
    let mut local_dirty = force_dirty
      || local_changed
      || node.text_content.is_changed()
      || text_input_dirty
      || !node.layout_cache.has_cached_result();

    // A cached result that contradicts the node's own fixed frame is stale no
    // matter how it got there (observed in production: retained-tree diffs can
    // transplant a cache across a frame change, leaving a clean-flagged node
    // laid at an old size — e.g. a virtualized list's spacer holding a
    // previous window's height, which blanks the viewport). Only sizes the
    // cached constraints would have permitted count as conflicts, so nodes
    // legitimately clamped by their parent are left alone.
    if !local_dirty && let Some((cached_constraints, cached_size)) = node.layout_cache.cached_entry() {
      let width_conflicts = matches!(
        node.frame.width,
        Some(Dimension::Px(px))
          if (cached_size.width - px).abs() > 0.5
            && px <= cached_constraints.max_width + 0.5
            && px >= cached_constraints.min_width - 0.5
      );
      let height_conflicts = matches!(
        node.frame.height,
        Some(Dimension::Px(px))
          if (cached_size.height - px).abs() > 0.5
            && px <= cached_constraints.max_height + 0.5
            && px >= cached_constraints.min_height - 0.5
      );
      if width_conflicts || height_conflicts {
        local_dirty = true;
      }
    }

    let scroll_offset_dirty = matches!(
      node.layout_kind(),
      LayoutKind::ScrollModifier { state, .. } if state.take_scroll_dirty()
    );

    if node
      .element_ref
      .as_ref()
      .is_some_and(|element_ref| element_ref.take_layout_dirty())
    {
      local_dirty = true;
    }

    if node.take_style_layout_dirty() {
      local_dirty = true;
    }

    // A base/overlay compute may already have consumed a child's marks.
    // Its revision still asks this parent to repair its own older snapshot.
    let mut child_dirty = node.layout_cache.inspected_generation.get() != 0 && node.layout_cache.needs_dirty_scan();
    for child in node.children() {
      child_dirty |= Self::mark_layout_dirty(child, force_dirty);
    }

    node
      .layout_cache
      .record_dirty_scan(generation, local_dirty, child_dirty || scroll_offset_dirty);

    node.layout_cache.is_dirty()
  }

  pub(crate) fn resolve_quads(&self, node: &Node, result: &LayoutResult) -> Vec<Quad> {
    self.resolve_quads_with_viewport(node, result, ClipRect::default())
  }

  pub(crate) fn resolve_quads_with_viewport(
    &self,
    node: &Node,
    result: &LayoutResult,
    viewport: ClipRect,
  ) -> Vec<Quad> {
    let mut quads = Vec::new();
    self.resolve_quads_with_viewport_into(node, result, viewport, &mut quads);
    quads
  }

  pub(crate) fn resolve_quads_with_viewport_into(
    &self,
    node: &Node,
    result: &LayoutResult,
    viewport: ClipRect,
    quads: &mut Vec<Quad>,
  ) {
    let root_offset = node.offset_position().unwrap_or_default();
    self.opacity_groups.borrow_mut().clear();
    #[cfg(feature = "screenshot")]
    self.sensitive_quads.borrow_mut().clear();
    self.ime_cursor_area.set(None);
    self.collect_quads(
      node,
      result,
      root_offset.x,
      root_offset.y,
      0.0,
      0.0,
      Transform2D::IDENTITY,
      viewport,
      viewport,
      true,
      quads,
    );
  }

  fn collect_quads(
    &self,
    node: &Node,
    result: &LayoutResult,
    abs_x: f32,
    abs_y: f32,
    parent_x: f32,
    parent_y: f32,
    inherited_transform: Transform2D,
    clip: ClipRect,
    cull_clip: ClipRect,
    culling_enabled: bool,
    quads: &mut Vec<Quad>,
  ) {
    if result.dropped || self.is_hidden_select_chevron(node) {
      return;
    }
    if node_is_plain_logical_wrapper(node) {
      if let Some(ref element_ref) = node.element_ref {
        element_ref.update(
          abs_x,
          abs_y,
          abs_x - parent_x,
          abs_y - parent_y,
          result.size.width,
          result.size.height,
        );
      }

      let (child_clip, child_cull_clip, child_culling_enabled) =
        if node.overflow == Overflow::Hidden && inherited_transform.is_axis_aligned() {
          let overflow_clip = intersect_clip(
            clip,
            ClipRect {
              x: abs_x,
              y: abs_y,
              width: result.size.width,
              height: result.size.height,
              active: true,
              border_radius: None,
            }
            .transformed(inherited_transform),
          );
          let child_cull_clip = if culling_enabled {
            intersect_clip(cull_clip, overflow_clip)
          } else {
            ClipRect::default()
          };
          (overflow_clip, child_cull_clip, culling_enabled)
        } else {
          (clip, cull_clip, culling_enabled)
        };

      for (child_layout, child_node) in result.children.iter().zip(node.children().iter()) {
        let child_abs_x = abs_x + child_layout.offset.x;
        let child_abs_y = abs_y + child_layout.offset.y;
        if child_culling_enabled
          && clipped_subtree_is_hidden(
            child_node,
            &child_layout.result,
            child_abs_x,
            child_abs_y,
            inherited_transform,
            child_cull_clip,
            self.box_shadow_outset(child_node),
          )
        {
          continue;
        }

        self.collect_quads(
          child_node,
          &child_layout.result,
          child_abs_x,
          child_abs_y,
          abs_x,
          abs_y,
          inherited_transform,
          child_clip,
          child_cull_clip,
          child_culling_enabled,
          quads,
        );
      }
      return;
    }

    // A plain wrapper never carries opacity (the fast path above requires
    // the default), so only here can a node open an opacity group.
    let group_start = quads.len();
    let frame = self.push_node_quads(
      node,
      result,
      abs_x,
      abs_y,
      parent_x,
      parent_y,
      inherited_transform,
      clip,
      cull_clip,
      culling_enabled,
      quads,
    );
    let NodeQuadFrame {
      transform,
      child_clip,
      child_cull_clip,
      child_culling_enabled,
      ..
    } = frame;

    let select_scope = self.enter_select_scope(node);
    for (child_layout, child_node) in result.children.iter().zip(node.children().iter()) {
      let child_abs_x = abs_x + child_layout.offset.x;
      let child_abs_y = abs_y + child_layout.offset.y;
      if child_culling_enabled
        && clipped_subtree_is_hidden(
          child_node,
          &child_layout.result,
          child_abs_x,
          child_abs_y,
          transform,
          child_cull_clip,
          self.box_shadow_outset(child_node),
        )
      {
        continue;
      }

      self.collect_quads(
        child_node,
        &child_layout.result,
        child_abs_x,
        child_abs_y,
        abs_x,
        abs_y,
        transform,
        child_clip,
        child_cull_clip,
        child_culling_enabled,
        quads,
      );
    }
    self.leave_select_scope(select_scope);

    self.push_node_overlay_quads(node, result, abs_x, abs_y, &frame, clip, quads);
    if node.opacity < DEFAULT_QUAD_OPACITY {
      self.close_opacity_group(node.opacity, group_start, quads);
    }
  }

  /// The quads a node paints under its children, and what its children
  /// inherit. Split from [`Self::collect_quads`], which recurses once per tree
  /// level: an unoptimized build gives every temporary of this body its own
  /// stack slot, and keeping them out of the recursive frame keeps deep trees
  /// within the main thread's stack (lurq#25).
  #[inline(never)]
  #[allow(clippy::too_many_arguments)]
  fn push_node_quads(
    &self,
    node: &Node,
    result: &LayoutResult,
    abs_x: f32,
    abs_y: f32,
    parent_x: f32,
    parent_y: f32,
    inherited_transform: Transform2D,
    clip: ClipRect,
    cull_clip: ClipRect,
    culling_enabled: bool,
    quads: &mut Vec<Quad>,
  ) -> NodeQuadFrame {
    if let Some(ref element_ref) = node.element_ref {
      element_ref.update(
        abs_x,
        abs_y,
        abs_x - parent_x,
        abs_y - parent_y,
        result.size.width,
        result.size.height,
      );
    }

    let background_color = node.resolved_color(&self.palette.borrow());
    let background_gradient = node
      .resolved_gradient()
      .and_then(|gradient| crate::layout::render_list::RenderGradient::resolve(&gradient, &self.palette.borrow()));
    let resolved_border = node.get_resolved_border(&self.palette.borrow(), &self.border_sizes.borrow());
    let has_visual = background_color.is_some() || background_gradient.is_some() || resolved_border.is_some();
    #[cfg(feature = "canvas")]
    let canvas_content = matches!(node.node_kind(), NodeKind::Canvas { .. });
    #[cfg(not(feature = "canvas"))]
    let canvas_content = false;
    // Inset shadows paint between the background and the border, so a border
    // that would share the background's quad moves after them.
    let has_inset_shadow = self.has_inset_box_shadow(node);
    let defer_border_to_overlay =
      resolved_border.is_some() && has_visual && (!node.children().is_empty() || canvas_content || has_inset_shadow);
    let content = match node.node_kind() {
      NodeKind::Text {
        state,
        style,
        transform_mode,
      } => {
        let resolved_style = style.resolve(&self.typography.borrow(), &self.palette.borrow());
        let vertical_align = resolved_style.vertical_align;
        QuadContent::Text {
          text: state.display_text().map_or_else(
            || node.text_content().unwrap_or_default().to_owned(),
            |display_text| display_text.to_string(),
          ),
          style: resolved_style,
          wrap: state.render_wrap(),
          vertical_align,
          transform_mode: *transform_mode,
        }
      }
      #[cfg(feature = "markdown")]
      NodeKind::RichText {
        spans, transform_mode, ..
      } => QuadContent::RichText {
        spans: if node.children().is_empty() {
          spans.clone()
        } else {
          Self::painted_rich_text_spans(node, spans)
        },
        wrap: node.text_wrap && node.text_overflow == TextOverflow::Clip,
        // Text that paints underlines or links is drawn from the top of its
        // laid-out lines, which `inline_text_frame` centers in its box.
        vertical_align: if Self::inline_text_frame(result).is_some() {
          crate::layout::text_style::VerticalAlign::LineBox
        } else {
          crate::layout::text_style::VerticalAlign::Center
        },
        transform_mode: *transform_mode,
      },
      NodeKind::TextInput {
        state,
        style,
        placeholder_style,
      } => self.input_content(state, style, placeholder_style.as_deref()),
      NodeKind::Checkbox { .. } => QuadContent::None,
      #[cfg(feature = "canvas")]
      NodeKind::Canvas { canvas } => canvas.image_data().map_or_else(
        || {
          if has_visual {
            QuadContent::Rect {
              color: background_color.unwrap_or(DEFAULT_TRANSPARENT_COLOR),
              gradient: background_gradient.clone(),
            }
          } else {
            QuadContent::None
          }
        },
        |data| QuadContent::Image {
          data,
          uv_min: [0.0, 0.0],
          uv_max: [1.0, 1.0],
        },
      ),
      #[cfg(feature = "raster")]
      NodeKind::Image { data, fit } => {
        let placement = background_image_placement(
          *fit,
          result.size.width,
          result.size.height,
          data.width() as f32,
          data.height() as f32,
        );
        QuadContent::Image {
          data: data.clone(),
          uv_min: placement.uv_min,
          uv_max: placement.uv_max,
        }
      }
      #[cfg(feature = "raster")]
      NodeKind::Video { data, fit } => {
        let placement = background_image_placement(
          *fit,
          result.size.width,
          result.size.height,
          data.width() as f32,
          data.height() as f32,
        );
        QuadContent::Video {
          data: data.clone(),
          uv_min: placement.uv_min,
          uv_max: placement.uv_max,
        }
      }
      #[cfg(feature = "image")]
      NodeKind::ResourceImage { .. } => QuadContent::None,
      #[cfg(feature = "svg")]
      NodeKind::Svg { data } => QuadContent::Svg { data: data.clone() },
      #[cfg(all(feature = "svg", feature = "resources"))]
      NodeKind::ResourceSvg { .. } => QuadContent::None,
      NodeKind::Slider { .. } => QuadContent::None,
      _ if has_visual => QuadContent::Rect {
        color: background_color.unwrap_or(DEFAULT_TRANSPARENT_COLOR),
        gradient: background_gradient.clone(),
      },
      _ => QuadContent::None,
    };

    // Quads paint at full opacity within their opacity group: a node with
    // `opacity < 1` fades its whole subtree as one layer, or folds its opacity
    // into its only quad (see `close_opacity_group`).
    let opacity = DEFAULT_QUAD_OPACITY;
    let local_transform = node.effective_transform();
    let local_transform_origin_abs = [abs_x + result.size.width * 0.5, abs_y + result.size.height * 0.5];
    let local_affine = if local_transform.is_identity() {
      Transform2D::IDENTITY
    } else {
      local_transform.around_origin(local_transform_origin_abs)
    };
    let transform = if inherited_transform.is_identity() {
      local_affine
    } else if local_affine.is_identity() {
      inherited_transform
    } else if local_transform.is_identity() {
      inherited_transform
    } else {
      inherited_transform.then(&local_affine)
    };

    let content_is_background = matches!(content, QuadContent::Rect { .. } | QuadContent::None);
    self.push_box_shadow_quads(node, result, abs_x, abs_y, false, opacity, transform, clip, quads);

    match &content {
      QuadContent::None => {}
      _ => {
        if (matches!(
          content,
          QuadContent::Text { .. } | QuadContent::RichText { .. } | QuadContent::DecoratedInput { .. }
        ) || (canvas_content && !matches!(content, QuadContent::Rect { .. })))
          && has_visual
        {
          let (visual_x, visual_y, visual_transform, visual_transform_origin) =
            transformed_quad_frame(abs_x, abs_y, transform);
          quads.push(Quad {
            x: visual_x,
            y: visual_y,
            width: result.size.width,
            height: result.size.height,
            opacity,
            transform: visual_transform,
            transform_origin: visual_transform_origin,
            content: QuadContent::Rect {
              color: background_color.unwrap_or(DEFAULT_TRANSPARENT_COLOR),
              gradient: background_gradient,
            },
            border_radius: node.get_border_radius(&self.radii.borrow()),
            border: if defer_border_to_overlay { None } else { resolved_border },
            clip,
          });
        }

        if has_inset_shadow && matches!(content, QuadContent::DecoratedInput { .. }) {
          self.push_box_shadow_quads(node, result, abs_x, abs_y, true, opacity, transform, clip, quads);
        }

        if let NodeKind::TextInput { state, .. } = node.node_kind() {
          self.push_input_tokens(node, state, result, (abs_x, abs_y), transform, clip, opacity, quads);
        }

        self.push_text_highlights(node, result, (abs_x, abs_y), transform, clip, opacity, quads);

        #[cfg(feature = "markdown")]
        let inline_paint = inline_links::InlinePaint {
          x: abs_x,
          y: abs_y,
          transform,
          clip,
          opacity,
        };
        #[cfg(feature = "markdown")]
        self.push_inline_link_fills(node, result, inline_paint, quads);

        if let NodeKind::Text { state, style, .. } = node.node_kind()
          && state.selectable()
        {
          let style = style.resolve(&self.typography.borrow(), &self.palette.borrow());
          let palette = self.palette.borrow();
          let selection_color = node
            .selection_color_value()
            .and_then(|color| color.resolve(&palette))
            .unwrap_or(DEFAULT_TEXT_SELECTION_COLOR);
          let selection_height = (style.font_size * style.line_height).min(result.size.height).max(1.0);
          let selection_clip = clip;
          for selection in state.selection_ranges(node.text_content().unwrap_or_default()) {
            let selection_x = abs_x + selection.x;
            let selection_y = abs_y + selection.y;
            let (selection_x, selection_y, selection_transform, selection_transform_origin) =
              transformed_quad_frame(selection_x, selection_y, transform);
            quads.push(Quad {
              x: selection_x,
              y: selection_y,
              width: selection.width,
              height: selection_height,
              opacity,
              transform: selection_transform,
              transform_origin: selection_transform_origin,
              content: QuadContent::Rect {
                color: selection_color,
                gradient: None,
              },
              border_radius: None,
              border: None,
              clip: selection_clip,
            });
          }
        }
        #[cfg(feature = "markdown")]
        if let NodeKind::RichText { state, spans, .. } = node.node_kind()
          && state.selectable()
        {
          let palette = self.palette.borrow();
          let selection_color = node
            .selection_color_value()
            .and_then(|color| color.resolve(&palette))
            .unwrap_or(DEFAULT_TEXT_SELECTION_COLOR);
          let style = spans.first().map(|span| &span.style);
          let selection_height = style
            .map(|style| style.font_size * style.line_height)
            .unwrap_or(16.0)
            .min(result.size.height)
            .max(1.0);
          let selection_clip = clip;
          let generated = Self::generated_span_rects(node, result);
          let pieces = state
            .selection_ranges(&node.rich_text_value().unwrap_or_default())
            .into_iter()
            .flat_map(|selection| {
              inline_links::without_rects(selection.x, selection.y, selection.width, selection_height, &generated)
                .into_iter()
                .map(move |(x, width)| (x, selection.y, width))
            });
          for (x, y, width) in pieces {
            let selection_x = abs_x + x;
            let selection_y = abs_y + y;
            let (selection_x, selection_y, selection_transform, selection_transform_origin) =
              transformed_quad_frame(selection_x, selection_y, transform);
            quads.push(Quad {
              x: selection_x,
              y: selection_y,
              width,
              height: selection_height,
              opacity,
              transform: selection_transform,
              transform_origin: selection_transform_origin,
              content: QuadContent::Rect {
                color: selection_color,
                gradient: None,
              },
              border_radius: None,
              border: None,
              clip: selection_clip,
            });
          }
        }

        if let NodeKind::TextInput { state, .. } = node.node_kind()
          && state.is_focused()
        {
          let palette = self.palette.borrow();
          let selection_color = node
            .selection_color_value()
            .and_then(|color| color.resolve(&palette))
            .unwrap_or(DEFAULT_TEXT_SELECTION_COLOR);
          let padding = self.resolved_padding_for_size(node, result.size);
          let content_width = (result.size.width - padding.left - padding.right).max(0.0);
          let content_height = (result.size.height - padding.top - padding.bottom).max(0.0);
          // Selections cover the glyphs' band in each line box (the font's
          // ascent and descent), not the leading around it.
          let (band_top, band_height) = state.text_band();
          let vertical_offset = padding.top + text_input_vertical_offset(state, content_height) + band_top;
          let selection_clip = intersect_clip(
            clip,
            ClipRect {
              x: abs_x + padding.left,
              y: abs_y + padding.top,
              width: content_width,
              height: content_height,
              active: true,
              border_radius: None,
            }
            .transformed(transform),
          );
          for selection in state.selection_ranges() {
            let (row_top, row_height) = state.selection_band_for_row(selection.y, (band_top, band_height));
            let selection_height = row_height.min(content_height).max(1.0);
            let selection_x = abs_x + padding.left + selection.x;
            let selection_y = abs_y + vertical_offset + selection.y + row_top - band_top;
            let (selection_x, selection_y, selection_transform, selection_transform_origin) =
              transformed_quad_frame(selection_x, selection_y, transform);
            quads.push(Quad {
              x: selection_x,
              y: selection_y,
              width: selection.width,
              height: selection_height,
              opacity,
              transform: selection_transform,
              transform_origin: selection_transform_origin,
              content: QuadContent::Rect {
                color: selection_color,
                gradient: None,
              },
              border_radius: None,
              border: None,
              clip: selection_clip,
            });
          }
        }

        let (content_x, content_y, content_width, content_height, content_clip) = match node.node_kind() {
          #[cfg(feature = "canvas")]
          NodeKind::Canvas { .. } => {
            let padding = self.resolved_padding_for_size(node, result.size);
            (
              abs_x + padding.left,
              abs_y + padding.top,
              (result.size.width - padding.left - padding.right).max(0.0),
              (result.size.height - padding.top - padding.bottom).max(0.0),
              clip,
            )
          }
          NodeKind::TextInput { state, .. } => {
            let padding = self.resolved_padding_for_size(node, result.size);
            let content_width = (result.size.width - padding.left - padding.right).max(0.0);
            let content_height = (result.size.height - padding.top - padding.bottom).max(0.0);
            let scroll_x = state.scroll_x();
            let scroll_y = state.scroll_y();
            // The glyph run is vertically aligned by the render path (via the
            // quad's `vertical_align`), so the content box starts at the top of
            // the padding box here — no metric centering offset is baked in.
            if matches!(
              state.overflow(),
              TextInputOverflow::Scroll | TextInputOverflow::Multiline
            ) {
              (
                abs_x + padding.left - scroll_x,
                abs_y + padding.top - scroll_y
                  + if state.painted_layout().is_some() {
                    text_input_vertical_offset(state, content_height)
                  } else {
                    0.0
                  },
                content_width + scroll_x,
                content_height + scroll_y,
                intersect_clip(
                  clip,
                  ClipRect {
                    x: abs_x + padding.left,
                    y: abs_y + padding.top,
                    width: content_width,
                    height: content_height,
                    active: true,
                    border_radius: None,
                  }
                  .transformed(transform),
                ),
              )
            } else {
              (
                abs_x + padding.left,
                abs_y + padding.top,
                content_width,
                content_height,
                clip,
              )
            }
          }
          #[cfg(feature = "raster")]
          NodeKind::Image { data, fit } | NodeKind::Video { data, fit } => {
            let placement = background_image_placement(
              *fit,
              result.size.width,
              result.size.height,
              data.width() as f32,
              data.height() as f32,
            );
            (
              abs_x + placement.x,
              abs_y + placement.y,
              placement.width,
              placement.height,
              clip,
            )
          }
          #[cfg(feature = "markdown")]
          NodeKind::RichText { .. } => match Self::inline_text_frame(result) {
            Some((top, text_height)) => (abs_x, abs_y + top, result.size.width, text_height, clip),
            None => (abs_x, abs_y, result.size.width, result.size.height, clip),
          },
          _ => (abs_x, abs_y, result.size.width, result.size.height, clip),
        };

        let content_uses_separate_visual_rect = (matches!(
          content,
          QuadContent::Text { .. } | QuadContent::RichText { .. } | QuadContent::DecoratedInput { .. }
        ) || (canvas_content && !matches!(content, QuadContent::Rect { .. })))
          && has_visual;
        let (content_x, content_y, content_transform, content_transform_origin) =
          transformed_quad_frame(content_x, content_y, transform);
        if has_inset_shadow && !content_is_background && !matches!(content, QuadContent::DecoratedInput { .. }) {
          self.push_box_shadow_quads(node, result, abs_x, abs_y, true, opacity, transform, clip, quads);
        }
        #[cfg(feature = "screenshot")]
        self.record_sensitive_quad(node, quads.len());
        #[cfg(feature = "markdown")]
        let underlines = match &content {
          QuadContent::RichText { spans, .. } => Self::inline_underlines(result, spans, inline_paint),
          _ => Vec::new(),
        };
        quads.push(Quad {
          x: content_x,
          y: content_y,
          width: content_width,
          height: content_height,
          opacity,
          transform: content_transform,
          transform_origin: content_transform_origin,
          content,
          border_radius: if content_uses_separate_visual_rect && !canvas_content {
            None
          } else {
            node.get_border_radius(&self.radii.borrow())
          },
          border: if content_uses_separate_visual_rect {
            None
          } else if defer_border_to_overlay {
            None
          } else {
            resolved_border
          },
          clip: content_clip,
        });
        #[cfg(feature = "markdown")]
        {
          quads.extend(underlines);
          self.push_inline_link_rings(node, result, inline_paint, quads);
        }
      }
    }

    #[cfg(feature = "raster")]
    if let Some(bg_image) = node.background_image.as_deref() {
      let placement = background_image_placement(
        node.background_size,
        result.size.width,
        result.size.height,
        bg_image.width() as f32,
        bg_image.height() as f32,
      );
      let image_x = abs_x + placement.x;
      let image_y = abs_y + placement.y;
      let (image_x, image_y, image_transform, image_transform_origin) =
        transformed_quad_frame(image_x, image_y, transform);
      quads.push(Quad {
        x: image_x,
        y: image_y,
        width: placement.width,
        height: placement.height,
        opacity,
        transform: image_transform,
        transform_origin: image_transform_origin,
        content: QuadContent::Image {
          data: bg_image.clone(),
          uv_min: placement.uv_min,
          uv_max: placement.uv_max,
        },
        border_radius: node.get_border_radius(&self.radii.borrow()),
        border: None,
        clip,
      });
    }

    if has_inset_shadow && content_is_background {
      self.push_box_shadow_quads(node, result, abs_x, abs_y, true, opacity, transform, clip, quads);
    }

    if let NodeKind::TextInput { state, style, .. } = node.node_kind()
      && state.is_focused()
    {
      self.push_focused_text_input_ime_quads(node, state, style, result, (abs_x, abs_y), transform, clip, quads);
    }

    match node.node_kind() {
      NodeKind::TextInput { state, style, .. } if state.is_focused() && self.should_render_text_input_caret(node) => {
        let padding = self.resolved_padding_for_size(node, result.size);
        let content_width = (result.size.width - padding.left - padding.right).max(0.0);
        let content_height = (result.size.height - padding.top - padding.bottom).max(0.0);
        let caret_line_height = state.caret_height();
        // Line height controls row pitch, but the insertion caret should track
        // the glyph/em height. Otherwise readable multiline leading produces
        // a visually oversized caret.
        let caret_height = state
          .caret_font_size(style.font_size)
          .min(caret_line_height)
          .min(content_height)
          .max(1.0);
        let caret_leading = ((caret_line_height - caret_height) * 0.5).max(0.0);
        let vertical_offset = padding.top + text_input_vertical_offset(state, content_height);
        // The caret starts at its insertion point, after the glyph before it.
        // At the start of a line nothing precedes it, so it ends there
        // instead: drawn from the line's left edge it covered the first
        // glyph (of the value or the placeholder), whose side bearing is
        // about zero. Where an ancestor clips right at that edge it stays at
        // the edge rather than disappear.
        let insertion_x = abs_x + padding.left + state.caret_x();
        let before_x = insertion_x - TEXT_INPUT_CARET_WIDTH;
        let room_before = transform.is_identity() && (!clip.active || before_x >= clip.x);
        let caret_x = if state.caret_at_line_start() && room_before {
          before_x
        } else {
          insertion_x
        };
        let caret_y = abs_y + vertical_offset + state.caret_y() + caret_leading;
        let palette = self.palette.borrow();
        let caret_color = node
          .caret_color_value()
          .and_then(|color| color.resolve(&palette))
          .or_else(|| style.caret_color.as_ref().and_then(|color| color.resolve(&palette)))
          .unwrap_or(style.color);
        let (caret_x, caret_y, caret_transform, caret_transform_origin) =
          transformed_quad_frame(caret_x, caret_y, transform);
        quads.push(Quad {
          x: caret_x,
          y: caret_y,
          width: TEXT_INPUT_CARET_WIDTH,
          height: caret_height,
          opacity,
          transform: caret_transform,
          transform_origin: caret_transform_origin,
          content: QuadContent::Rect {
            color: caret_color,
            gradient: None,
          },
          border_radius: None,
          border: None,
          clip: intersect_clip(
            clip,
            ClipRect {
              // Room for a caret at the start of a line.
              x: abs_x + padding.left - TEXT_INPUT_CARET_WIDTH,
              y: abs_y + padding.top,
              width: content_width + TEXT_INPUT_CARET_WIDTH,
              height: content_height,
              active: true,
              border_radius: None,
            }
            .transformed(transform),
          ),
        });
      }
      NodeKind::Checkbox { state } => {
        let checked = state.is_checked();
        let hovered = node.is_style_hovered();
        let style = state.style(checked, hovered, node.is_style_focus_visible());
        let width = style.width.unwrap_or(result.size.width).min(result.size.width).max(0.0);
        let height = style
          .height
          .unwrap_or(result.size.height)
          .min(result.size.height)
          .max(0.0);
        let rect = SliderPartRect {
          x: abs_x + (result.size.width - width) * 0.5,
          y: abs_y + (result.size.height - height) * 0.5,
          width,
          height,
        };
        let color = if checked {
          style
            .color
            .unwrap_or_else(|| self.default_control_fill(state.part_styled(true), PaletteColor::Accent))
        } else {
          style
            .color
            .or_else(|| node.resolved_color(&self.palette.borrow()))
            .unwrap_or_else(|| self.default_control_fill(state.part_styled(false), PaletteColor::SurfaceInput))
        };
        push_checkbox_quads(
          quads,
          rect,
          &style,
          color,
          style
            .border_radius
            .map(|radius| radius.resolve(&self.radii.borrow()))
            .or_else(|| node.get_border_radius(&self.radii.borrow())),
          style
            .border
            .as_ref()
            .and_then(|border| border.resolve_with_sizes(&self.palette.borrow(), &self.border_sizes.borrow()))
            .or_else(|| node.get_resolved_border(&self.palette.borrow(), &self.border_sizes.borrow())),
          checked,
          opacity,
          transform,
          clip,
        );
      }
      NodeKind::Slider { state } => {
        let hovered = node.is_style_hovered() || state.is_hovered() || state.is_dragging();
        let track_style = state.track_style(hovered);
        let fill_style = state.fill_style(hovered);
        let thumb_style = state.thumb_style(hovered, node.is_style_focus_visible());
        let (track_rect, thumb_rect) = state.part_rects(
          abs_x,
          abs_y,
          result.size.width,
          result.size.height,
          hovered,
          DEFAULT_SLIDER_THUMB_MIN_SIZE,
        );
        let track_color = track_style
          .color
          .or_else(|| node.resolved_color(&self.palette.borrow()))
          .unwrap_or_else(|| self.default_control_fill(state.track_styled(), PaletteColor::Border));
        let track_radius = track_style
          .border_radius
          .map(|radius| radius.resolve(&self.radii.borrow()))
          .or_else(|| node.get_border_radius(&self.radii.borrow()));
        let track_border = track_style
          .border
          .as_ref()
          .and_then(|border| border.resolve_with_sizes(&self.palette.borrow(), &self.border_sizes.borrow()))
          .or_else(|| node.get_resolved_border(&self.palette.borrow(), &self.border_sizes.borrow()));
        push_slider_part_quads(
          quads,
          track_rect,
          &track_style,
          track_color,
          track_radius,
          track_border,
          opacity,
          transform,
          clip,
        );
        if let Some(fill_style) = fill_style {
          let fill_height = fill_style.height.unwrap_or(track_rect.height).max(0.0);
          let fill_center_y = track_rect.y + track_rect.height * 0.5;
          let fill_rect = SliderPartRect {
            x: track_rect.x,
            y: fill_center_y - fill_height * 0.5,
            width: ((thumb_rect.x + thumb_rect.width * 0.5) - track_rect.x)
              .max(0.0)
              .min(track_rect.width),
            height: fill_height,
          };
          let fill_color = fill_style.color.unwrap_or(track_color);
          let fill_radius = fill_style
            .border_radius
            .map(|radius| radius.resolve(&self.radii.borrow()))
            .or(track_radius);
          let fill_border = fill_style
            .border
            .as_ref()
            .and_then(|border| border.resolve_with_sizes(&self.palette.borrow(), &self.border_sizes.borrow()));
          push_slider_part_quads(
            quads,
            fill_rect,
            &fill_style,
            fill_color,
            fill_radius,
            fill_border,
            opacity,
            transform,
            clip,
          );
        }
        push_slider_part_quads(
          quads,
          thumb_rect,
          &thumb_style,
          thumb_style
            .color
            .unwrap_or_else(|| self.default_control_fill(state.thumb_styled(), PaletteColor::Accent)),
          Some(
            thumb_style
              .border_radius
              .map(|radius| radius.resolve(&self.radii.borrow()))
              .unwrap_or_else(|| BorderRadius::all(thumb_rect.width.min(thumb_rect.height) * 0.5)),
          ),
          thumb_style
            .border
            .as_ref()
            .and_then(|border| border.resolve_with_sizes(&self.palette.borrow(), &self.border_sizes.borrow())),
          opacity,
          transform,
          clip,
        );
      }
      NodeKind::Select { state } => {
        self.push_select_trigger_quads(node, state, result, (abs_x, abs_y), opacity, transform, clip, quads);
      }
      _ => {}
    }

    let (child_clip, child_cull_clip, child_culling_enabled) =
      if let LayoutKind::ScrollModifier { state, culling, .. } = node.layout_kind() {
        let viewport = ClipRect {
          x: abs_x,
          y: abs_y,
          width: state.viewport_width(),
          height: state.viewport_height(),
          active: true,
          border_radius: node
            .get_border_radius(&self.radii.borrow())
            .map(|radius| radius.clamped_to_rect(state.viewport_width(), state.viewport_height())),
        };
        let child_clip = content_clip(clip, viewport, resolved_border, transform);
        let child_cull_clip = if *culling {
          content_clip(intersect_clip(cull_clip, clip), viewport, resolved_border, transform)
        } else {
          ClipRect::default()
        };
        (child_clip, child_cull_clip, *culling)
      } else if node.overflow == Overflow::Hidden && hidden_overflow_creates_clip(has_visual, transform) {
        let bounds = ClipRect {
          x: abs_x,
          y: abs_y,
          width: result.size.width,
          height: result.size.height,
          active: true,
          border_radius: node
            .get_border_radius(&self.radii.borrow())
            .map(|radius| radius.clamped_to_rect(result.size.width, result.size.height)),
        };
        let child_clip = content_clip(clip, bounds, resolved_border, transform);
        let child_cull_clip = if culling_enabled {
          content_clip(intersect_clip(cull_clip, clip), bounds, resolved_border, transform)
        } else {
          ClipRect::default()
        };
        (child_clip, child_cull_clip, culling_enabled)
      } else {
        (clip, cull_clip, culling_enabled)
      };

    NodeQuadFrame {
      transform,
      opacity,
      child_clip,
      child_cull_clip,
      child_culling_enabled,
      deferred_border: resolved_border.filter(|_| defer_border_to_overlay),
    }
  }

  /// The quads a node paints over its children: a border deferred past them
  /// and scrollbars. See [`Self::push_node_quads`].
  #[inline(never)]
  fn push_node_overlay_quads(
    &self,
    node: &Node,
    result: &LayoutResult,
    abs_x: f32,
    abs_y: f32,
    frame: &NodeQuadFrame,
    clip: ClipRect,
    quads: &mut Vec<Quad>,
  ) {
    let NodeQuadFrame {
      transform,
      opacity,
      deferred_border,
      ..
    } = *frame;
    if deferred_border.is_some() {
      let (border_x, border_y, border_transform, border_transform_origin) =
        transformed_quad_frame(abs_x, abs_y, transform);
      quads.push(Quad {
        x: border_x,
        y: border_y,
        width: result.size.width,
        height: result.size.height,
        opacity,
        transform: border_transform,
        transform_origin: border_transform_origin,
        content: QuadContent::Rect {
          color: DEFAULT_TRANSPARENT_COLOR,
          gradient: None,
        },
        border_radius: node.get_border_radius(&self.radii.borrow()),
        border: deferred_border,
        clip,
      });
    }

    if let Some((state, direction)) = node.scroll_container() {
      let (viewport_x, viewport_y) = match node.node_kind() {
        // A multiline text input scrolls the text inside its padding.
        NodeKind::TextInput { .. } => {
          let padding = self.resolved_padding_for_size(node, result.size);
          (abs_x + padding.left, abs_y + padding.top)
        }
        _ => (abs_x, abs_y),
      };
      state.set_viewport_position(viewport_x, viewport_y);
      let sb_style = node.scrollbar_style(self.scrollbar.borrow().clone());
      state.set_style(sb_style.clone());
      let thumb_color = sb_style.thumb_color;

      match direction {
        ScrollDirection::Vertical | ScrollDirection::Both => {
          if let Some(geo) = state.scrollbar_geometry_for_axis(ScrollAxis::Vertical, &sb_style) {
            if sb_style.track_color.a() > 0 {
              quads.push(Quad {
                x: geo.track_x,
                y: geo.track_y,
                width: geo.track_width,
                height: geo.track_height,
                opacity: DEFAULT_QUAD_OPACITY,
                transform: Transform2D::IDENTITY,
                transform_origin: None,
                content: QuadContent::Rect {
                  color: sb_style.track_color,
                  gradient: None,
                },
                border_radius: Some(crate::node::border::BorderRadius::all(sb_style.track_radius)),
                border: None,
                clip,
              });
            }
            quads.push(Quad {
              x: geo.thumb_x,
              y: geo.thumb_y,
              width: geo.thumb_width,
              height: geo.thumb_height,
              opacity: DEFAULT_QUAD_OPACITY,
              transform: Transform2D::IDENTITY,
              transform_origin: None,
              content: QuadContent::Rect {
                color: thumb_color,
                gradient: None,
              },
              border_radius: Some(crate::node::border::BorderRadius::all(sb_style.thumb_radius)),
              border: None,
              clip,
            });
          }
        }
        _ => {}
      }
      match direction {
        ScrollDirection::Horizontal | ScrollDirection::Both => {
          if let Some(geo) = state.scrollbar_geometry_for_axis(ScrollAxis::Horizontal, &sb_style) {
            if sb_style.track_color.a() > 0 {
              quads.push(Quad {
                x: geo.track_x,
                y: geo.track_y,
                width: geo.track_width,
                height: geo.track_height,
                opacity: DEFAULT_QUAD_OPACITY,
                transform: Transform2D::IDENTITY,
                transform_origin: None,
                content: QuadContent::Rect {
                  color: sb_style.track_color,
                  gradient: None,
                },
                border_radius: Some(crate::node::border::BorderRadius::all(sb_style.track_radius)),
                border: None,
                clip,
              });
            }
            quads.push(Quad {
              x: geo.thumb_x,
              y: geo.thumb_y,
              width: geo.thumb_width,
              height: geo.thumb_height,
              opacity: DEFAULT_QUAD_OPACITY,
              transform: Transform2D::IDENTITY,
              transform_origin: None,
              content: QuadContent::Rect {
                color: thumb_color,
                gradient: None,
              },
              border_radius: Some(crate::node::border::BorderRadius::all(sb_style.thumb_radius)),
              border: None,
              clip,
            });
          }
        }
        _ => {}
      }
    }
  }

  fn layout_node(&self, glyph_engine: &mut GlyphEngine, node: &Node, constraints: Constraints) -> Arc<LayoutResult> {
    // A collapsed line's cache holds layouts its parent did not collapse.
    if !self.is_collapsed(node)
      && let Some(cached) = self.layout_node_from_cache(glyph_engine, node, constraints)
    {
      return cached;
    }

    self.layout_node_uncached_with_child_overrides(glyph_engine, node, constraints, None)
  }

  fn layout_node_from_cache(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
  ) -> Option<Arc<LayoutResult>> {
    if !node.layout_cache.is_dirty() {
      return node.layout_cache.get_shared(constraints).and_then(|cached| {
        if !Self::cached_result_matches_node_tree(node, &cached) {
          return None;
        }
        let prepared = self.prepare_cached_result(node, cached);
        node.layout_cache.record_validation(constraints, prepared.generation);
        Some(prepared)
      });
    }

    if node.layout_cache.is_local_dirty() || !node.layout_cache.is_descendant_dirty() {
      return None;
    }
    // A paragraph places its links itself: a link laid out on its own has no
    // box, so a changed link lays its paragraph out again.
    #[cfg(feature = "markdown")]
    if matches!(node.node_kind(), NodeKind::RichText { .. }) {
      return None;
    }

    // `align-items: stretch` gives an intrinsically sized child a second,
    // cross-axis-tight layout pass while preserving its measured main-axis
    // size. That tight main-axis constraint is an output of the previous
    // parent layout, not an author constraint. Repairing changed descendants
    // under it can therefore clamp the child to its old height/width and hide
    // the intrinsic size change from this flex parent. Re-run the parent from
    // its original constraints so the child is measured naturally before it
    // is stretched again.
    if Self::has_dirty_stretched_intrinsic_child(node) || Self::has_dirty_shrunk_child(node) {
      return None;
    }

    // A descendant changed, but this node may still be able to keep its own
    // geometry. Patch dirty child results into the cached tree and only force
    // this parent to relayout if the child no longer fits the cached parent or
    // the parent layout kind needs to reposition siblings around the new size.
    let cached = node.layout_cache.get_repairable(constraints)?;
    if !Self::cached_result_matches_node_tree(node, &cached) {
      return None;
    }

    let mut cached = cached.as_ref().clone();
    let mut child_overrides = vec![None; cached.children.len()];
    for (index, child) in node.children().iter().enumerate() {
      if !child.layout_cache.is_dirty()
        && cached.children[index].result.generation == child.layout_cache.watch().generation()
      {
        continue;
      }

      let child_constraints = child.layout_cache.constraints()?;
      let repaired = self.layout_node(glyph_engine, child, child_constraints);
      let original_offset = cached.children[index].offset;
      child_overrides[index] = Some(ChildLayoutOverride {
        constraints: child_constraints,
        result: repaired.clone(),
      });
      if let Some(rect) = child.element_override_rect()
        && (rect.relative_x != original_offset.x || rect.relative_y != original_offset.y)
      {
        return Some(self.layout_node_uncached_with_child_overrides(
          glyph_engine,
          node,
          constraints,
          Some(&child_overrides),
        ));
      }
      // A cleared bounds override leaves the override baked into the cached
      // offset — only a full relayout recovers the child's natural position.
      if child.take_element_override_cleared() {
        return Some(self.layout_node_uncached_with_child_overrides(
          glyph_engine,
          node,
          constraints,
          Some(&child_overrides),
        ));
      }
      let size_changed = repaired.size != cached.children[index].result.size;
      if size_changed
        && (!Self::layout_kind_can_patch_child_size_change(node.layout_kind())
          || Self::scroll_sized_by_content(node, constraints)
          || !Self::child_fits_cached_parent(original_offset, repaired.size, cached.size))
      {
        return Some(self.layout_node_uncached_with_child_overrides(
          glyph_engine,
          node,
          constraints,
          Some(&child_overrides),
        ));
      }
      cached.children[index].result = repaired;
    }

    let prepared = self.prepare_cached_result(node, Arc::new(cached));
    node.layout_cache.store_shared(constraints, prepared.clone());
    Some(prepared)
  }

  fn cached_result_matches_node_tree(node: &Node, result: &Arc<LayoutResult>) -> bool {
    if result.generation == node.layout_cache.watch().generation() || node.layout_cache.matches_validated(result) {
      return true;
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    // A cached subtree whose geometry contradicts any node's fixed frame is
    // stale no matter how clean its flags are — observed in production as a
    // virtualized list's rows column serving a previous window's result
    // (old spacer heights inside), shifting every row off the viewport. The
    // per-node frame/cache check in `mark_layout_dirty` cannot catch this:
    // the stale geometry lives in the PARENT's cached tree, not in the
    // mismatched child's own cache.
    if let Some(Dimension::Px(px)) = node.frame.width
      && (result.size.width - px).abs() > 0.5
    {
      return false;
    }
    if let Some(Dimension::Px(px)) = node.frame.height
      && (result.size.height - px).abs() > 0.5
    {
      return false;
    }
    let matches = result.children.len() == node.children().len()
      && node
        .children()
        .iter()
        .zip(&result.children)
        .all(|(child, child_layout)| {
          // A cached offset that contradicts a live bounds override is stale
          // the same way: consumed dirty flags don't protect a parent cache
          // that predates the override (observed with the overlay-host double
          // compute — the second pass served the root's pre-drag snapshot,
          // pinning dragged elements to their declared position).
          if let Some(rect) = child.element_override_rect()
            && ((child_layout.offset.x - rect.relative_x).abs() > 0.01
              || (child_layout.offset.y - rect.relative_y).abs() > 0.01)
          {
            return false;
          }
          // Likewise a cached offset that contradicts the child's DECLARED
          // absolute position: a position-only change (e.g. an editor undo
          // moving a widget back) doesn't set any dirty flag, so a preserved
          // node cache keeps the parent serving the old offset until an
          // unrelated invalidation. Only absolutely-positioned children have
          // a self-declared offset to check against.
          if let Position::Absolute { x, y, .. } = child.position() {
            let expected = Self::apply_relative_position(child, Offset::new(x, y));
            if (child_layout.offset.x - expected.x).abs() > 0.01 || (child_layout.offset.y - expected.y).abs() > 0.01 {
              return false;
            }
          }
          Self::cached_result_matches_node_tree(child, &child_layout.result)
        });
    if matches {
      // Remember this exact snapshot too: a rebuilt parent can still hold
      // the old generation of an otherwise unchanged descendant.
      node.layout_cache.remember_validation(result);
    }
    matches
  }

  fn child_fits_cached_parent(offset: Offset, child_size: Size, parent_size: Size) -> bool {
    const EPSILON: f32 = 0.5;
    offset.x >= -EPSILON
      && offset.y >= -EPSILON
      && offset.x + child_size.width <= parent_size.width + EPSILON
      && offset.y + child_size.height <= parent_size.height + EPSILON
  }

  /// A scroll container under loose constraints takes its content's size, so
  /// a content size change must re-measure it rather than patch the child in.
  fn scroll_sized_by_content(node: &Node, constraints: Constraints) -> bool {
    matches!(node.layout_kind(), LayoutKind::ScrollModifier { .. }) && !constraints_are_tight(constraints)
  }

  fn layout_kind_can_patch_child_size_change(layout_kind: &LayoutKind) -> bool {
    matches!(
      layout_kind,
      LayoutKind::Stack { .. } | LayoutKind::LogicalModifier | LayoutKind::ScrollModifier { .. }
    )
  }

  fn has_dirty_stretched_intrinsic_child(node: &Node) -> bool {
    let vertical = match node.layout_kind() {
      LayoutKind::Column {
        align: Alignment::Stretch,
        ..
      } => true,
      LayoutKind::Row {
        align: Alignment::Stretch,
        ..
      } => false,
      _ => return false,
    };
    node.children().iter().any(|child| {
      if child.is_overlay_declaration() || !child.layout_cache.is_dirty() {
        return false;
      }

      let is_intrinsic_main_size = child
        .state_flex()
        .is_none_or(|params| params.grow == 0.0 && params.basis.is_none());
      let has_explicit_main_size = child.state_frame().is_some_and(|frame| {
        if vertical {
          frame.height.is_some()
        } else {
          frame.width.is_some()
        }
      });
      let cached_main_axis_is_tight = child.layout_cache.constraints().is_some_and(|constraints| {
        if vertical {
          constraints.min_height == constraints.max_height
        } else {
          constraints.min_width == constraints.max_width
        }
      });
      is_intrinsic_main_size && !has_explicit_main_size && cached_main_axis_is_tight
    })
  }

  /// Serving a cached result skips text layout, so the text output of every
  /// text leaf in it is written back as well: the node's text state otherwise
  /// keeps what the last real layout, possibly at other constraints, wrote.
  fn prepare_cached_result(&self, node: &Node, mut cached: Arc<LayoutResult>) -> Arc<LayoutResult> {
    if !cached.holds.scroll {
      if cached.holds.text {
        Self::restore_text_layout_tree(node, &cached);
      }
      if cached.generation != node.layout_cache.watch().generation() {
        Arc::make_mut(&mut cached).generation = node.layout_cache.watch().generation();
      }
      return cached;
    }
    self.prepare_layout_result_tree(node, Arc::make_mut(&mut cached), true);
    cached
  }

  /// Syncs the scroll containers in `result` with it and, for a served cache
  /// entry, restores its text outputs. Only the subtrees holding either are
  /// visited, and only those holding a scroll container are copied: the
  /// rest stay shared with the caches that hold them. Serving a cached tree
  /// would otherwise copy all of it, and preparing every freshly laid-out
  /// parent would walk and copy its whole subtree again.
  fn prepare_layout_result_tree(&self, node: &Node, result: &mut LayoutResult, restore_text_layout: bool) {
    result.generation = node.layout_cache.watch().generation();
    if restore_text_layout {
      Self::restore_text_layout_output(node, result);
    }
    for (child_node, child_layout) in node.children().iter().zip(result.children.iter_mut()) {
      if child_layout.result.holds.scroll {
        self.prepare_layout_result_tree(child_node, Arc::make_mut(&mut child_layout.result), restore_text_layout);
      } else if restore_text_layout && child_layout.result.holds.text {
        Self::restore_text_layout_tree(child_node, &child_layout.result);
      }
    }
    result.holds = result.children.iter().fold(
      SubtreeHolds {
        scroll: matches!(node.layout_kind(), LayoutKind::ScrollModifier { .. }),
        text: result.text_layout.is_some(),
      },
      |holds, child| SubtreeHolds {
        scroll: holds.scroll || child.result.holds.scroll,
        text: holds.text || child.result.holds.text,
      },
    );

    if let LayoutKind::ScrollModifier { state, direction, .. } = node.layout_kind() {
      let content_width = result.children.first().map(|c| c.result.size.width).unwrap_or(0.0);
      let content_height = result.children.first().map(|c| c.result.size.height).unwrap_or(0.0);
      let style = node.scrollbar_style(self.scrollbar.borrow().clone());
      let reserve_vertical = should_reserve_scrollbar(&style, *direction, ScrollAxis::Vertical);
      let reserve_horizontal = should_reserve_scrollbar(&style, *direction, ScrollAxis::Horizontal);
      let viewport = reserved_viewport(result.size, &style, reserve_vertical, reserve_horizontal);
      state.update_layout_with_container(
        content_width,
        content_height,
        viewport.width,
        viewport.height,
        result.size.width,
        result.size.height,
      );
      if let Some(child) = result.children.first_mut() {
        child.offset.x = -state.scroll_x();
        child.offset.y = -state.scroll_y();
      }
    }
  }

  fn layout_node_uncached_with_child_overrides(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> Arc<LayoutResult> {
    #[cfg(feature = "perf_profile")]
    let profile_start = Instant::now();
    #[cfg(feature = "perf_profile")]
    let profile_local_dirty = node.layout_cache.is_local_dirty();
    #[cfg(feature = "perf_profile")]
    let profile_descendant_dirty = node.layout_cache.is_descendant_dirty();
    #[cfg(feature = "perf_profile")]
    let profile_override_count = child_overrides
      .map(|overrides| {
        overrides
          .iter()
          .filter(|override_result| override_result.is_some())
          .count()
      })
      .unwrap_or(0);

    self.last_recalculated.set(true);
    #[cfg(test)]
    crate::core::layout_dirty::LAYOUTS.with(|layouts| layouts.set(layouts.get() + 1));
    let mut result = self.layout_node_box(glyph_engine, node, constraints, child_overrides);
    Self::apply_runtime_rect(node, &mut result);
    self.prepare_layout_result_tree(node, &mut result, false);
    let result = Arc::new(result);
    node.layout_cache.store_shared(constraints, result.clone());
    #[cfg(feature = "perf_profile")]
    {
      let elapsed = profile_start.elapsed();
      if elapsed >= SLOW_LAYOUT_NODE_THRESHOLD {
        tracing::trace!(
          target: "layout-profile",
          "[layout-profile] kind={} tag={} children={} overrides={} local_dirty={} descendant_dirty={} constraints={:.1}..{:.1}x{:.1}..{:.1} size={:.1}x{:.1} ms={:.3}",
          layout_kind_profile_name(node.layout_kind()),
          node.tag_name(),
          node.children().len(),
          profile_override_count,
          profile_local_dirty,
          profile_descendant_dirty,
          constraints.min_width,
          constraints.max_width,
          constraints.min_height,
          constraints.max_height,
          result.size.width,
          result.size.height,
          elapsed.as_secs_f64() * 1000.0,
        );
      }
    }
    result
  }

  fn layout_node_box(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let frame = node.effective_frame(FrameConstraints::default());
    let padding = node.effective_padding(&Padding::default());
    let frame_is_flat = frame != FrameConstraints::default();
    let padding_is_flat = padding != Padding::default() || self.text_input_scrollbar_lane(node) > 0.0;

    let mut result = if frame_is_flat || padding_is_flat {
      self.layout_flat_box(glyph_engine, node, constraints, &frame, &padding, child_overrides)
    } else {
      self.layout_node_content(glyph_engine, node, constraints, child_overrides)
    };
    self.lay_out_text_selection(glyph_engine, node, &mut result);
    #[cfg(feature = "markdown")]
    self.lay_out_inline_text(glyph_engine, node, &mut result);
    result
  }

  fn layout_node_content(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    match node.layout_kind() {
      LayoutKind::Leaf => self.layout_leaf(glyph_engine, node, constraints),
      LayoutKind::Row {
        spacing,
        align,
        justify,
        wrap,
      } => {
        let spacing = spacing.resolve(&self.spacing.borrow(), constraints.max_width);
        self.layout_flex(
          glyph_engine,
          node,
          constraints,
          spacing,
          *align,
          *justify,
          *wrap,
          false,
          child_overrides,
        )
      }
      LayoutKind::Column {
        spacing,
        align,
        justify,
        wrap,
      } => {
        let spacing = spacing.resolve(&self.spacing.borrow(), constraints.max_height);
        self.layout_flex(
          glyph_engine,
          node,
          constraints,
          spacing,
          *align,
          *justify,
          *wrap,
          true,
          child_overrides,
        )
      }
      LayoutKind::Stack { align } => self.layout_stack(glyph_engine, node, constraints, *align, child_overrides),
      LayoutKind::LogicalModifier => self.layout_passthrough(glyph_engine, node, constraints, child_overrides),
      LayoutKind::ScrollModifier { state, direction, .. } => {
        self.layout_scroll(glyph_engine, node, constraints, state, *direction, child_overrides)
      }
    }
  }

  fn apply_runtime_rect(node: &Node, result: &mut LayoutResult) {
    if let Some(rect) = node.element_override_rect() {
      result.size.width = rect.width;
      result.size.height = rect.height;
    }

    for (child_layout, child_node) in result.children.iter_mut().zip(node.children()) {
      if let Some(rect) = child_node.element_override_rect() {
        child_layout.offset.x = rect.relative_x;
        child_layout.offset.y = rect.relative_y;
      }
    }
  }

  // Leaf controls and scroll layout have large temporaries. Keep them out of
  // the dispatch frame that stays on the stack while containers recurse.
  #[inline(never)]
  fn layout_leaf(&self, glyph_engine: &mut GlyphEngine, node: &Node, constraints: Constraints) -> LayoutResult {
    match node.node_kind() {
      NodeKind::Text { state, style, .. } => {
        let source = node.text_content().unwrap_or_default();
        let style = style.resolve(&self.typography.borrow(), &self.palette.borrow());
        let (content, force_display_text) = (source, false);
        return self.layout_text_node(
          glyph_engine,
          content,
          state,
          &style,
          constraints,
          node.text_wrap,
          node.text_overflow,
          force_display_text,
        );
      }
      #[cfg(feature = "markdown")]
      NodeKind::RichText { state, spans, .. } => {
        return self.layout_rich_text_node(
          glyph_engine,
          spans,
          state,
          constraints,
          node.text_wrap,
          node.text_overflow,
        );
      }
      NodeKind::TextInput {
        state,
        style,
        placeholder_style,
      } => {
        let content = state.rendered_text_for_layout();
        return self.layout_text_input(
          glyph_engine,
          state,
          &content,
          style,
          placeholder_style.as_deref(),
          constraints,
          self.text_input_scrollbar_lane(node),
        );
      }
      NodeKind::Checkbox { .. } => {
        let preferred = if let NodeKind::Checkbox { state } = node.node_kind() {
          let (width, height) = state.preferred_size(DEFAULT_CHECKBOX_WIDTH, DEFAULT_CHECKBOX_HEIGHT);
          node.intrinsic_size.unwrap_or(Size::new(width, height))
        } else {
          node
            .intrinsic_size
            .unwrap_or(Size::new(DEFAULT_CHECKBOX_WIDTH, DEFAULT_CHECKBOX_HEIGHT))
        };
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      NodeKind::Slider { .. } => {
        let preferred = if let NodeKind::Slider { state } = node.node_kind() {
          let (width, height) = state.preferred_size(
            DEFAULT_SLIDER_WIDTH,
            DEFAULT_SLIDER_HEIGHT,
            DEFAULT_SLIDER_THUMB_MIN_SIZE,
          );
          node.intrinsic_size.unwrap_or(Size::new(width, height))
        } else {
          node
            .intrinsic_size
            .unwrap_or(Size::new(DEFAULT_SLIDER_WIDTH, DEFAULT_SLIDER_HEIGHT))
        };
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      NodeKind::Select { state } => {
        let trigger = state.style().resolved_trigger(false, false, false);
        let width = node
          .intrinsic_size
          .map(|size| size.width)
          .or(trigger.min_width)
          .unwrap_or(DEFAULT_SELECT_WIDTH);
        let height = node
          .intrinsic_size
          .map(|size| size.height)
          .or(trigger.min_height)
          .unwrap_or(DEFAULT_SELECT_HEIGHT);
        return LayoutResult {
          size: constraints.constrain(Size::new(width, height)),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      #[cfg(feature = "canvas")]
      NodeKind::Canvas { .. } => {
        return LayoutResult {
          size: constraints.constrain(node.intrinsic_size.unwrap_or(Size::new(300.0, 150.0))),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      #[cfg(feature = "raster")]
      NodeKind::Image { data, .. } => {
        let preferred = node
          .intrinsic_size
          .unwrap_or(Size::new(data.width() as f32, data.height() as f32));
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      #[cfg(feature = "raster")]
      NodeKind::Video { data, .. } => {
        let preferred = node
          .intrinsic_size
          .unwrap_or(Size::new(data.width() as f32, data.height() as f32));
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      #[cfg(feature = "image")]
      NodeKind::ResourceImage { .. } => {
        let preferred = node
          .intrinsic_size
          .unwrap_or(Size::new(DEFAULT_RESOURCE_WIDTH, DEFAULT_RESOURCE_HEIGHT));
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      #[cfg(feature = "svg")]
      NodeKind::Svg { data } => {
        let preferred = node
          .intrinsic_size
          .unwrap_or(Size::new(data.viewbox_width(), data.viewbox_height()));
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      #[cfg(all(feature = "svg", feature = "resources"))]
      NodeKind::ResourceSvg { .. } => {
        let preferred = node
          .intrinsic_size
          .unwrap_or(Size::new(DEFAULT_RESOURCE_WIDTH, DEFAULT_RESOURCE_HEIGHT));
        return LayoutResult {
          size: constraints.constrain(preferred),
          children: vec![].into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        };
      }
      // Its paragraph places it (`inline_links`); on its own it has no box.
      #[cfg(feature = "markdown")]
      NodeKind::InlineLink { .. } => {}
      // Laid out as a stack of its placeholder until resolved; a leaf only
      // when the runtime turned it into an `Image`.
      #[cfg(feature = "image")]
      NodeKind::LoadingImage { .. } => {}
      NodeKind::Empty => {}
    }

    let preferred = node.intrinsic_size.unwrap_or_default();
    LayoutResult {
      size: constraints.constrain(preferred),
      children: vec![].into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn layout_text(
    &self,
    glyph_engine: &mut GlyphEngine,
    text: &str,
    style: &TextStyle,
    constraints: Constraints,
    wrap: bool,
  ) -> LayoutResult {
    let max_width = if wrap && bounded_text_width(constraints.max_width) {
      constraints.max_width
    } else {
      f32::MAX
    };
    let measured = glyph_engine.measure_text(text, style, max_width);
    let size = if wrap {
      constraints.constrain(measured)
    } else {
      Size::new(
        measured.width.max(constraints.min_width),
        measured.height.max(constraints.min_height),
      )
    };
    LayoutResult {
      size,
      children: vec![].into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn layout_text_node(
    &self,
    glyph_engine: &mut GlyphEngine,
    text: &str,
    state: &crate::node::node_kind::TextState,
    style: &TextStyle,
    constraints: Constraints,
    wrap: bool,
    overflow: TextOverflow,
    force_display_text: bool,
  ) -> LayoutResult {
    let effective_wrap = wrap && overflow == TextOverflow::Clip;
    let overflow_display_text = match overflow {
      TextOverflow::Clip => None,
      TextOverflow::Elipsis => self.ellipsize_text(glyph_engine, text, style, constraints.max_width),
    };
    let layout_text = overflow_display_text.as_deref().unwrap_or(text);
    let display_text = (force_display_text || overflow_display_text.is_some()).then(|| Arc::from(layout_text));
    let render_wrap = effective_wrap && bounded_text_width(constraints.max_width);
    let size = if !state.selectable() && overflow_display_text.is_none() && constraints_are_tight(constraints) {
      Size::new(constraints.max_width, constraints.max_height)
    } else {
      self
        .layout_text(glyph_engine, layout_text, style, constraints, effective_wrap)
        .size
    };
    // Selectable text gets its carets from the painted layout once its box
    // is known (`lay_out_text_selection`).
    let output = TextOutput {
      display_text,
      render_wrap,
      carets: None,
      text_top: 0.0,
      #[cfg(feature = "markdown")]
      inline: None,
    };
    Self::text_result(state, size, output)
  }

  #[cfg(feature = "markdown")]
  fn layout_rich_text_node(
    &self,
    glyph_engine: &mut GlyphEngine,
    spans: &[crate::layout::quad::RichTextSpan],
    state: &crate::node::node_kind::TextState,
    constraints: Constraints,
    wrap: bool,
    overflow: TextOverflow,
  ) -> LayoutResult {
    let effective_wrap = wrap && overflow == TextOverflow::Clip;
    let render_wrap = effective_wrap && bounded_text_width(constraints.max_width);
    let max_width = if render_wrap { constraints.max_width } else { f32::MAX };
    let display_text = state
      .selectable()
      .then(|| Arc::from(spans.iter().map(|span| span.text.as_str()).collect::<String>()));
    let measured = glyph_engine.measure_rich_text(spans, 1.0, max_width);
    let size = if effective_wrap {
      constraints.constrain(measured)
    } else {
      Size::new(
        measured.width.max(constraints.min_width),
        measured.height.max(constraints.min_height),
      )
    };
    // Selectable text gets its carets from the painted layout once its box
    // is known (`lay_out_text_selection`).
    let output = TextOutput {
      display_text,
      render_wrap,
      carets: None,
      text_top: 0.0,
      #[cfg(feature = "markdown")]
      inline: None,
    };
    Self::text_result(state, size, output)
  }

  fn text_width(&self, glyph_engine: &mut GlyphEngine, text: &str, style: &TextStyle) -> f32 {
    glyph_engine.measure_text(text, style, f32::MAX).width
  }

  fn ellipsize_text(
    &self,
    glyph_engine: &mut GlyphEngine,
    text: &str,
    style: &TextStyle,
    max_width: f32,
  ) -> Option<String> {
    if text.is_empty() || !max_width.is_finite() {
      return None;
    }
    if max_width <= 0.0 {
      return Some(String::new());
    }
    if self.text_width(glyph_engine, text, style) <= max_width {
      return None;
    }

    const ELLIPSIS: &str = "…";
    let ellipsis_width = self.text_width(glyph_engine, ELLIPSIS, style);
    if ellipsis_width > max_width {
      return Some(String::new());
    }

    let boundaries: Vec<usize> = text
      .char_indices()
      .map(|(index, _)| index)
      .chain(std::iter::once(text.len()))
      .collect();
    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    while low < high {
      let mid = (low + high).div_ceil(2);
      let candidate = format!("{}{}", &text[..boundaries[mid]], ELLIPSIS);
      if self.text_width(glyph_engine, &candidate, style) <= max_width {
        low = mid;
      } else {
        high = mid - 1;
      }
    }

    Some(format!("{}{}", &text[..boundaries[low]], ELLIPSIS))
  }

  fn layout_flex(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    spacing: f32,
    align: Alignment,
    justify: Justify,
    wrap: FlexWrap,
    vertical: bool,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let children = node.children();
    if children.is_empty() {
      return LayoutResult {
        size: constraints.constrain(Size::default()),
        children: vec![].into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      };
    }

    if wrap == FlexWrap::Wrap {
      return self.layout_flex_wrap(
        glyph_engine,
        node,
        constraints,
        spacing,
        align,
        justify,
        vertical,
        child_overrides,
      );
    }

    let layout_child_count = children.iter().filter(|child| !child.is_overlay_declaration()).count();
    let total_spacing = spacing * (layout_child_count as f32 - 1.0).max(0.0);
    let max_main = if vertical {
      constraints.max_height
    } else {
      constraints.max_width
    };

    let mut grow_total = 0.0_f32;
    let mut shrink_total = 0.0_f32;
    // The one Vec of this line, kept as the result's children. The first pass
    // lays out the children sized on their own; a child that waits for its
    // share of the free room holds `self.awaiting_room` until the second.
    let mut child_layouts: Vec<ChildLayout> = Vec::with_capacity(children.len());

    for child in children {
      let result = if child.is_overlay_declaration() {
        Arc::new(LayoutResult {
          size: Size::default(),
          children: Vec::new().into(),
          text_layout: None,
          dropped: false,
          holds: SubtreeHolds::UNKNOWN,
          generation: 0,
        })
      } else {
        let flex_params = child.state_flex();
        if let Some(params) = flex_params {
          grow_total += params.grow;
          shrink_total += params.shrink;
        }
        if flex_params.is_some_and(|params| params.grow != 0.0 || params.basis.is_some()) {
          self.awaiting_room.clone()
        } else {
          let child_constraints = Self::non_flex_child_constraints(child, constraints, vertical);
          self.layout_child_node(
            glyph_engine,
            child_overrides,
            child_layouts.len(),
            child,
            child_constraints,
          )
        }
      };
      child_layouts.push(ChildLayout {
        offset: Offset::default(),
        result,
      });
    }

    let non_flex_main: f32 = child_layouts
      .iter()
      .filter(|layout| !Arc::ptr_eq(&layout.result, &self.awaiting_room))
      .map(|layout| main_size(&layout.result, vertical))
      .sum();

    let remaining = max_main - total_spacing - non_flex_main;

    let mut overflow = LineOverflow::new(FreeRoom { remaining, grow_total }, vertical);
    for (i, child) in children.iter().enumerate() {
      if !Arc::ptr_eq(&child_layouts[i].result, &self.awaiting_room) {
        continue;
      }
      // Only a flex child waits for free room, but its flex params are read
      // again here: a state style that changed since the first pass (its
      // interaction state is shared across clones) may have taken them away.
      // Such a child is laid out as the first pass would have laid it out
      // now; the change marks its layout dirty, so the next pass agrees.
      let Some(params) = child.state_flex() else {
        let child_constraints = Self::non_flex_child_constraints(child, constraints, vertical);
        child_layouts[i].result = self.layout_child_node(glyph_engine, child_overrides, i, child, child_constraints);
        continue;
      };
      // An unbounded main axis has no free space to grow into: a growing
      // child gets its basis, or its natural size without one. Growing by
      // an infinite remainder would lay it out at an infinite size.
      if !remaining.is_finite() && params.basis.is_none() {
        let child_constraints = Self::non_flex_child_constraints(child, constraints, vertical);
        child_layouts[i].result = self.layout_child_node(glyph_engine, child_overrides, i, child, child_constraints);
        continue;
      }
      let basis_size = params.basis.unwrap_or(0.0);
      let flex_size = if remaining.is_finite() && remaining > 0.0 && grow_total > 0.0 {
        basis_size + remaining * (params.grow / grow_total)
      } else {
        basis_size
      };
      let child_constraints = if vertical {
        Constraints {
          min_width: 0.0,
          max_width: constraints.max_width,
          min_height: flex_size,
          max_height: flex_size,
        }
      } else {
        Constraints {
          min_width: flex_size,
          max_width: flex_size,
          min_height: 0.0,
          max_height: constraints.max_height,
        }
      };
      let result = self.layout_child_node(glyph_engine, child_overrides, i, child, child_constraints);
      overflow.add_granted(&result, flex_size, params.basis);
      child_layouts[i].result = result;
    }

    let collapsed = self.shrink_flex_line(
      glyph_engine,
      &FlexShrinkLine {
        children,
        constraints,
        max_main,
        overflow: overflow.total(),
        spacing,
        shrink_total,
        vertical,
        collapsed: self.is_collapsed(node),
      },
      &mut child_layouts,
      child_overrides,
    );

    let max_cross: f32 = child_layouts
      .iter()
      .zip(children.iter())
      .filter(|(layout, child)| occupies_line(child, &layout.result))
      .map(|(layout, _)| {
        let r = &layout.result;
        if vertical { r.size.width } else { r.size.height }
      })
      .fold(0.0_f32, f32::max);

    // Dropped children take no spacing.
    let line_child_count = child_layouts
      .iter()
      .zip(children.iter())
      .filter(|(layout, child)| occupies_line(child, &layout.result))
      .count();
    let total_spacing = spacing * (line_child_count as f32 - 1.0).max(0.0);
    let total_main: f32 = child_layouts
      .iter()
      .zip(children.iter())
      .filter(|(layout, child)| occupies_line(child, &layout.result))
      .map(|(layout, _)| main_size(&layout.result, vertical))
      .sum::<f32>()
      + total_spacing;

    let size = if vertical {
      constraints.constrain(Size::new(max_cross, total_main))
    } else {
      constraints.constrain(Size::new(total_main, max_cross))
    };

    let container_cross = if vertical { size.width } else { size.height };

    if matches!(align, Alignment::Stretch) {
      for (i, child) in children.iter().enumerate() {
        if !occupies_line(child, &child_layouts[i].result) {
          continue;
        }
        let r = &child_layouts[i].result;
        let child_cross = if vertical { r.size.width } else { r.size.height };
        if child_cross < container_cross {
          let stretch_constraints = if vertical {
            Constraints {
              min_width: container_cross,
              max_width: container_cross,
              min_height: r.size.height,
              max_height: r.size.height,
            }
          } else {
            Constraints {
              min_width: r.size.width,
              max_width: r.size.width,
              min_height: container_cross,
              max_height: container_cross,
            }
          };
          child_layouts[i].result = if collapsed.get(i) == Some(&true) {
            self.layout_collapsed(glyph_engine, child, stretch_constraints)
          } else {
            self.layout_child_node(glyph_engine, child_overrides, i, child, stretch_constraints)
          };
        }
      }
    }

    self.position_flex_line(children, &mut child_layouts, &size, spacing, align, justify, vertical);

    LayoutResult {
      size,
      children: child_layouts.into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn non_flex_child_constraints(child: &Node, constraints: Constraints, vertical: bool) -> Constraints {
    let bounds_percent_main = Self::has_percent_main_frame(child, vertical);
    // A single-line ellipsized Text needs the row's finite width during its
    // first layout pass. Without it, the text measures at intrinsic width and
    // an enclosing flex/clip cell only clips the glyphs after layout, so the
    // requested ellipsis is never produced.
    // A shrinking text that drops out whole (`ShrinkLimit::Drop`) is
    // measured at its full width: measured inside the row it would ellipsize
    // instead of dropping. Without a shrink factor it never drops, so it
    // ellipsizes like any other text.
    let drops_whole =
      child.shrink_rule().limit == ShrinkLimit::Drop && child.state_flex().is_some_and(|params| params.shrink > 0.0);
    let bounds_ellipsized_text =
      !vertical && child.text_overflow == TextOverflow::Elipsis && constraints.max_width.is_finite() && !drops_whole;

    if vertical {
      Constraints {
        min_width: 0.0,
        max_width: constraints.max_width,
        min_height: 0.0,
        max_height: if bounds_percent_main {
          constraints.max_height
        } else {
          f32::INFINITY
        },
      }
    } else {
      Constraints {
        min_width: 0.0,
        max_width: if bounds_percent_main || bounds_ellipsized_text {
          constraints.max_width
        } else {
          f32::INFINITY
        },
        min_height: 0.0,
        max_height: constraints.max_height,
      }
    }
  }

  fn has_percent_main_frame(child: &Node, vertical: bool) -> bool {
    let has_own_percent_frame = child.state_frame().is_some_and(|frame| {
      let dimensions = if vertical {
        [frame.height, frame.min_height, frame.max_height]
      } else {
        [frame.width, frame.min_width, frame.max_width]
      };

      dimensions
        .into_iter()
        .flatten()
        .any(|dimension| matches!(dimension, Dimension::Pct(_)))
    });

    has_own_percent_frame
      || (matches!(child.layout_kind(), LayoutKind::LogicalModifier)
        && child
          .children()
          .first()
          .is_some_and(|child| Self::has_percent_main_frame(child, vertical)))
  }

  fn position_flex_line(
    &self,
    children: &[Node],
    child_layouts: &mut [ChildLayout],
    container_size: &Size,
    spacing: f32,
    align: Alignment,
    justify: Justify,
    vertical: bool,
  ) {
    let container_main = if vertical {
      container_size.height
    } else {
      container_size.width
    };
    let container_cross = if vertical {
      container_size.width
    } else {
      container_size.height
    };
    let children_main: f32 = child_layouts
      .iter()
      .zip(children.iter())
      .filter(|(layout, child)| occupies_line(child, &layout.result))
      .map(|(layout, _)| main_size(&layout.result, vertical))
      .sum();
    let free_space = (container_main - children_main).max(0.0);
    let layout_child_count = child_layouts
      .iter()
      .zip(children.iter())
      .filter(|(layout, child)| occupies_line(child, &layout.result))
      .count();
    let n = layout_child_count as f32;

    let (leading, gap) = match justify {
      Justify::Start => (0.0, spacing),
      Justify::End => (free_space - spacing * (n - 1.0), spacing),
      Justify::Center => ((free_space - spacing * (n - 1.0)) / 2.0, spacing),
      Justify::SpaceBetween => {
        if n > 1.0 {
          (0.0, free_space / (n - 1.0))
        } else {
          (0.0, 0.0)
        }
      }
      Justify::SpaceAround => {
        let g = if n > 0.0 { free_space / n } else { 0.0 };
        (g / 2.0, g)
      }
      Justify::SpaceEvenly => {
        let g = if n > 0.0 { free_space / (n + 1.0) } else { 0.0 };
        (g, g)
      }
    };

    let mut main_cursor = leading;
    let mut positioned_layout_children = 0usize;

    for (layout, child) in child_layouts.iter_mut().zip(children.iter()) {
      let result = &layout.result;
      if !occupies_line(child, result) {
        // A dropped child sits, at zero size, where the next child starts.
        layout.offset = match (result.dropped, vertical) {
          (false, _) => Offset::default(),
          (true, true) => Offset::new(0.0, main_cursor),
          (true, false) => Offset::new(main_cursor, 0.0),
        };
        continue;
      }

      let child_main = if vertical {
        result.size.height
      } else {
        result.size.width
      };
      let child_cross = if vertical {
        result.size.width
      } else {
        result.size.height
      };
      let cross_offset = align.cross_offset(container_cross, child_cross);

      let offset = if vertical {
        Offset::new(cross_offset, main_cursor)
      } else {
        Offset::new(main_cursor, cross_offset)
      };
      layout.offset = Self::apply_relative_position(child, offset);

      positioned_layout_children += 1;
      main_cursor += child_main
        + if positioned_layout_children < layout_child_count {
          gap
        } else {
          0.0
        };
    }
  }

  fn layout_flex_wrap(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    spacing: f32,
    align: Alignment,
    justify: Justify,
    vertical: bool,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let children = node.children();
    let max_main = if vertical {
      constraints.max_height
    } else {
      constraints.max_width
    };

    let mut child_layouts: Vec<ChildLayout> = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
      // Keep natural children intrinsic, but resolve percentage frames
      // against the finite content constraints, as in the non-wrap path.
      // The container's min cross-constraint must not stretch wrapped items.
      let c = Self::non_flex_child_constraints(child, constraints, vertical);
      child_layouts.push(ChildLayout {
        offset: Offset::default(),
        result: self.layout_child_node(glyph_engine, child_overrides, index, child, c),
      });
    }

    let mut cross_cursor = 0.0_f32;
    let mut max_main_used = 0.0_f32;
    let mut line_start = 0;

    while line_start < child_layouts.len() {
      let line_end = wrap_line_end(&child_layouts, line_start, spacing, max_main, vertical);
      let line = &mut child_layouts[line_start..line_end];

      let line_cross: f32 = line
        .iter()
        .map(|layout| {
          let r = &layout.result;
          if vertical { r.size.width } else { r.size.height }
        })
        .fold(0.0_f32, f32::max);

      let children_main: f32 = line
        .iter()
        .map(|layout| main_size(&layout.result, vertical))
        .sum::<f32>();
      let line_main_total = children_main + spacing * (line.len() as f32 - 1.0).max(0.0);
      max_main_used = max_main_used.max(line_main_total);

      let container_main = if vertical {
        max_main.min(constraints.max_height)
      } else {
        max_main.min(constraints.max_width)
      };
      let free_space = (container_main - children_main).max(0.0);
      let n = line.len() as f32;
      let (leading, gap) = match justify {
        Justify::Start => (0.0, spacing),
        Justify::End => (free_space - spacing * (n - 1.0), spacing),
        Justify::Center => ((free_space - spacing * (n - 1.0)) / 2.0, spacing),
        Justify::SpaceBetween => {
          if n > 1.0 {
            (0.0, free_space / (n - 1.0))
          } else {
            (0.0, 0.0)
          }
        }
        Justify::SpaceAround => {
          let g = free_space / n;
          (g / 2.0, g)
        }
        Justify::SpaceEvenly => {
          let g = free_space / (n + 1.0);
          (g, g)
        }
      };

      let mut main_cursor = leading;
      for (j, layout) in line.iter_mut().enumerate() {
        let child_main = main_size(&layout.result, vertical);
        let child_cross = if vertical {
          layout.result.size.width
        } else {
          layout.result.size.height
        };
        let cross_offset = align.cross_offset(line_cross, child_cross) + cross_cursor;
        let offset = if vertical {
          Offset::new(cross_offset, main_cursor)
        } else {
          Offset::new(main_cursor, cross_offset)
        };
        layout.offset = Self::apply_relative_position(&children[line_start + j], offset);
        main_cursor += child_main + if j < (n as usize - 1) { gap } else { 0.0 };
      }

      cross_cursor += line_cross + spacing;
      line_start = line_end;
    }

    let total_cross = (cross_cursor - spacing).max(0.0);
    let size = if vertical {
      constraints.constrain(Size::new(total_cross, max_main_used))
    } else {
      constraints.constrain(Size::new(max_main_used, total_cross))
    };

    LayoutResult {
      size,
      children: child_layouts.into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn layout_stack(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    align: StackAlignment,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let children = node.children();
    let child_constraints = constraints.loosen_width().loosen_height();
    let mut child_layouts: Vec<ChildLayout> = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
      let result = match child.position() {
        Position::Absolute { width, height, .. } => {
          let resolved_width = width.and_then(|size| Self::resolve_dimension(size, constraints.max_width));
          let resolved_height = height.and_then(|size| Self::resolve_dimension(size, constraints.max_height));
          let positioned_constraints = Constraints {
            min_width: resolved_width.unwrap_or(0.0),
            max_width: resolved_width.unwrap_or(child_constraints.max_width),
            min_height: resolved_height.unwrap_or(0.0),
            max_height: resolved_height.unwrap_or(child_constraints.max_height),
          };
          let mut result = self.layout_child_node(glyph_engine, child_overrides, index, child, positioned_constraints);
          if let Some(width) = resolved_width {
            Arc::make_mut(&mut result).size.width = width;
          }
          if let Some(height) = resolved_height {
            Arc::make_mut(&mut result).size.height = height;
          }
          result
        }
        _ => self.layout_child_node(glyph_engine, child_overrides, index, child, child_constraints),
      };
      child_layouts.push(ChildLayout {
        offset: Offset::default(),
        result,
      });
    }

    let (max_width, max_height) = children
      .iter()
      .zip(child_layouts.iter())
      .filter(|(child, _)| !matches!(child.position(), Position::Absolute { .. }))
      .fold((0.0_f32, 0.0_f32), |(width, height), (_, layout)| {
        (
          width.max(layout.result.size.width),
          height.max(layout.result.size.height),
        )
      });
    let size = constraints.constrain(Size::new(max_width, max_height));

    for (layout, child) in child_layouts.iter_mut().zip(children.iter()) {
      layout.offset = match child.position() {
        Position::Absolute { x, y, .. } => Self::apply_relative_position(child, Offset::new(x, y)),
        _ => {
          let child_align = child
            .align_self()
            .map(|align| align.to_stack_alignment())
            .unwrap_or(align);
          Self::apply_relative_position(child, child_align.resolve_offset(size, layout.result.size))
        }
      };
    }

    LayoutResult {
      size,
      children: child_layouts.into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn apply_relative_position(node: &Node, offset: Offset) -> Offset {
    node
      .offset_position()
      .map(|relative| Offset::new(offset.x + relative.x, offset.y + relative.y))
      .unwrap_or(offset)
  }

  /// The node's padding at `size`, with a multiline text input's scrollbar
  /// lane ([`Self::text_input_scrollbar_lane`]) counted in its right side as
  /// layout counts it: inside it is the box the content is laid out and
  /// painted in.
  pub(crate) fn resolved_padding_for_size(&self, node: &Node, size: Size) -> ResolvedPadding {
    let padding = node.effective_padding(&Padding::default());
    let scrollbar_lane = self.text_input_scrollbar_lane(node);
    if padding == Padding::default() && scrollbar_lane == 0.0 {
      return ResolvedPadding::default();
    }

    let spacing = self.spacing.borrow();
    ResolvedPadding {
      left: padding.get_left().resolve(&spacing, size.width),
      top: padding.get_top().resolve(&spacing, size.height),
      right: padding.get_right().resolve(&spacing, size.width) + scrollbar_lane,
      bottom: padding.get_bottom().resolve(&spacing, size.height),
    }
  }

  fn layout_flat_box(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    frame: &FrameConstraints,
    padding: &Padding,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let outer_constraints = self.frame_constraints_for_node(node, constraints, frame);
    let parent_w = outer_constraints.max_width;
    let parent_h = outer_constraints.max_height;
    let spacing = self.spacing.borrow();
    let left = padding.get_left().resolve(&spacing, parent_w);
    let right = padding.get_right().resolve(&spacing, parent_w) + self.text_input_scrollbar_lane(node);
    let top = padding.get_top().resolve(&spacing, parent_h);
    let bottom = padding.get_bottom().resolve(&spacing, parent_h);
    let h_pad = left + right;
    let v_pad = top + bottom;

    let inner_constraints = Constraints {
      min_width: (outer_constraints.min_width - h_pad).max(0.0),
      max_width: (outer_constraints.max_width - h_pad).max(0.0),
      min_height: (outer_constraints.min_height - v_pad).max(0.0),
      max_height: (outer_constraints.max_height - v_pad).max(0.0),
    };

    let mut result = self.layout_node_content(glyph_engine, node, inner_constraints, child_overrides);
    result.size = outer_constraints.constrain(Size::new(result.size.width + h_pad, result.size.height + v_pad));

    if left != 0.0 || top != 0.0 {
      for child in &mut result.children {
        child.offset.x += left;
        child.offset.y += top;
      }
    }

    result
  }

  fn frame_constraints_for_node(&self, node: &Node, constraints: Constraints, frame: &FrameConstraints) -> Constraints {
    let resolved_width = frame
      .width
      .and_then(|size| Self::resolve_dimension(size, constraints.max_width));
    let resolved_height = frame
      .height
      .and_then(|size| Self::resolve_dimension(size, constraints.max_height));
    let resolved_min_width = frame
      .min_width
      .and_then(|size| Self::resolve_dimension(size, constraints.max_width));
    let resolved_max_width = frame
      .max_width
      .and_then(|size| Self::resolve_dimension(size, constraints.max_width));
    let resolved_min_height = frame
      .min_height
      .and_then(|size| Self::resolve_dimension(size, constraints.max_height));
    let resolved_max_height = frame
      .max_height
      .and_then(|size| Self::resolve_dimension(size, constraints.max_height));

    let flex_child = node.state_flex().is_some();
    let tight_width = constraints.min_width == constraints.max_width;
    let tight_height = constraints.min_height == constraints.max_height;

    let mut c = constraints;
    if let Some(w) = resolved_width {
      let width = if flex_child && tight_width {
        constraints.max_width
      } else {
        Self::clamp_resolved_dimension(w, resolved_min_width, resolved_max_width)
      };
      c.min_width = width;
      c.max_width = width;
    }
    if let Some(h) = resolved_height {
      let height = if flex_child && tight_height {
        constraints.max_height
      } else {
        Self::clamp_resolved_dimension(h, resolved_min_height, resolved_max_height)
      };
      c.min_height = height;
      c.max_height = height;
    }

    #[cfg(feature = "raster")]
    if match node.node_kind() {
      NodeKind::Image { .. } | NodeKind::Video { .. } => true,
      #[cfg(feature = "image")]
      NodeKind::ResourceImage { .. } => true,
      _ => false,
    } {
      Self::apply_intrinsic_aspect_ratio(node, &mut c, resolved_width, resolved_height);
    }

    #[cfg(all(feature = "svg", feature = "resources"))]
    let is_svg_media = matches!(node.node_kind(), NodeKind::Svg { .. } | NodeKind::ResourceSvg { .. });
    #[cfg(all(feature = "svg", not(feature = "resources")))]
    let is_svg_media = matches!(node.node_kind(), NodeKind::Svg { .. });

    #[cfg(feature = "svg")]
    if is_svg_media {
      Self::apply_intrinsic_aspect_ratio(node, &mut c, resolved_width, resolved_height);
    }

    if resolved_width.is_none()
      && let Some(v) = resolved_min_width
    {
      c.min_width = c.min_width.max(v);
    }
    if resolved_width.is_none()
      && let Some(v) = resolved_max_width
    {
      c.max_width = c.max_width.min(v);
    }
    if resolved_height.is_none()
      && let Some(v) = resolved_min_height
    {
      c.min_height = c.min_height.max(v);
    }
    if resolved_height.is_none()
      && let Some(v) = resolved_max_height
    {
      c.max_height = c.max_height.min(v);
    }

    c.min_width = c.min_width.min(c.max_width);
    c.min_height = c.min_height.min(c.max_height);
    c
  }

  fn clamp_resolved_dimension(value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let mut value = value;
    if let Some(min) = min {
      value = value.max(min);
    }
    if let Some(max) = max {
      value = value.min(max);
    }
    value
  }

  #[cfg(any(feature = "raster", feature = "svg"))]
  fn apply_intrinsic_aspect_ratio(
    node: &Node,
    constraints: &mut Constraints,
    resolved_width: Option<f32>,
    resolved_height: Option<f32>,
  ) {
    if let Some(intrinsic) = node.intrinsic_size {
      if intrinsic.width > 0.0 && intrinsic.height > 0.0 {
        if let (Some(w), None) = (resolved_width, resolved_height) {
          let h = w * intrinsic.height / intrinsic.width;
          constraints.min_height = h;
          constraints.max_height = h;
        } else if let (None, Some(h)) = (resolved_width, resolved_height) {
          let w = h * intrinsic.width / intrinsic.height;
          constraints.min_width = w;
          constraints.max_width = w;
        }
      }
    }
  }

  fn resolve_dimension(dimension: Dimension, parent_size: f32) -> Option<f32> {
    match dimension {
      Dimension::Auto => None,
      Dimension::Px(value) => Some(value),
      Dimension::Pct(percent) if parent_size.is_finite() => Some(parent_size * percent / 100.0),
      Dimension::Pct(_) => None,
    }
  }

  #[inline(never)]
  fn layout_scroll(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    state: &ScrollState,
    direction: ScrollDirection,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let child = &node.children()[0];
    let style = node.scrollbar_style(self.scrollbar.borrow().clone());

    if style.placement != ScrollBarPlacement::Reserved {
      let child_result = self.layout_child_node(
        glyph_engine,
        child_overrides,
        0,
        child,
        scroll_child_constraints(direction, constraints, constraints.max_width, constraints.max_height),
      );
      let size = scroll_container_size(constraints, &child_result, &style, false, false);
      let viewport = reserved_viewport(size, &style, false, false);
      state.update_layout_with_container(
        child_result.size.width,
        child_result.size.height,
        viewport.width,
        viewport.height,
        size.width,
        size.height,
      );

      return LayoutResult {
        size,
        children: vec![ChildLayout {
          offset: Offset::new(-state.scroll_x(), -state.scroll_y()),
          result: child_result.into(),
        }]
        .into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      };
    }

    let reserve_vertical = should_reserve_scrollbar(&style, direction, ScrollAxis::Vertical);
    let reserve_horizontal = should_reserve_scrollbar(&style, direction, ScrollAxis::Horizontal);
    // First pass sizes the container from the child's natural extent (only
    // relevant when the constraints leave an axis content-sized); the second
    // lays the child inside the gutter-reduced viewport.
    let child_result = self.layout_child_node(
      glyph_engine,
      child_overrides,
      0,
      child,
      scroll_child_constraints(direction, constraints, constraints.max_width, constraints.max_height),
    );
    let size = scroll_container_size(constraints, &child_result, &style, reserve_vertical, reserve_horizontal);
    let viewport = reserved_viewport(size, &style, reserve_vertical, reserve_horizontal);
    let child_result = self.layout_child_node(
      glyph_engine,
      child_overrides,
      0,
      child,
      scroll_child_constraints(direction, constraints, viewport.width, viewport.height),
    );
    let size = scroll_container_size(constraints, &child_result, &style, reserve_vertical, reserve_horizontal);
    let viewport = reserved_viewport(size, &style, reserve_vertical, reserve_horizontal);

    state.update_layout_with_container(
      child_result.size.width,
      child_result.size.height,
      viewport.width,
      viewport.height,
      size.width,
      size.height,
    );

    LayoutResult {
      size,
      children: vec![ChildLayout {
        offset: Offset::new(-state.scroll_x(), -state.scroll_y()),
        result: child_result.into(),
      }]
      .into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn layout_passthrough(
    &self,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
  ) -> LayoutResult {
    let Some(child) = node.children().first() else {
      return LayoutResult {
        size: Size::new(0.0, 0.0),
        children: Vec::new().into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      };
    };
    let child_result = self.layout_child_node(glyph_engine, child_overrides, 0, child, constraints);
    let size = child_result.size;

    LayoutResult {
      size,
      children: vec![ChildLayout {
        offset: Offset::default(),
        result: child_result.into(),
      }]
      .into(),
      text_layout: None,
      dropped: false,
      holds: SubtreeHolds::UNKNOWN,
      generation: 0,
    }
  }

  fn layout_child_node(
    &self,
    glyph_engine: &mut GlyphEngine,
    child_overrides: Option<&[Option<ChildLayoutOverride>]>,
    index: usize,
    child: &Node,
    constraints: Constraints,
  ) -> Arc<LayoutResult> {
    if let Some(Some(child_override)) = child_overrides.and_then(|overrides| overrides.get(index)) {
      if child_override.constraints == constraints {
        // The override was laid out before this parent layout began, which
        // may have laid the child out again under other constraints since.
        Self::restore_text_layout_tree(child, &child_override.result);
        child.layout_cache.record_reuse(constraints, &child_override.result);
        return child_override.result.clone();
      }
    }

    self.layout_node(glyph_engine, child, constraints)
  }
}

/// Where the wrapped line starting at child `start` ends: a line takes at least
/// one child, and then every child that still fits on it.
fn wrap_line_end(child_layouts: &[ChildLayout], start: usize, spacing: f32, max_main: f32, vertical: bool) -> usize {
  let mut line_main = main_size(&child_layouts[start].result, vertical);
  for (index, layout) in child_layouts.iter().enumerate().skip(start + 1) {
    let needed = spacing + main_size(&layout.result, vertical);
    if line_main + needed > max_main {
      return index;
    }
    line_main += needed;
  }
  child_layouts.len()
}

fn multiline_preferred_height(
  state: &crate::node::node_kind::TextInputState,
  content_height: f32,
  line_height: f32,
) -> f32 {
  let (min_rows, max_rows) = state.row_limits();
  let min_height = min_rows
    .map(|rows| rows.max(1) as f32 * line_height)
    .unwrap_or(line_height);
  let preferred_height = content_height.max(min_height);
  let preferred_height = max_rows
    .map(|rows| preferred_height.min((rows.max(1) as f32 * line_height).max(min_height)))
    .unwrap_or(preferred_height);

  preferred_height
}

fn explicit_multiline_height(text: &str, line_height: f32) -> f32 {
  text.split('\n').count().max(1) as f32 * line_height
}

fn scroll_child_constraints(
  direction: ScrollDirection,
  constraints: Constraints,
  viewport_width: f32,
  viewport_height: f32,
) -> Constraints {
  match direction {
    ScrollDirection::Vertical => Constraints {
      min_width: constraints.min_width.min(viewport_width),
      max_width: viewport_width,
      min_height: 0.0,
      max_height: f32::INFINITY,
    },
    ScrollDirection::Horizontal => Constraints {
      min_width: 0.0,
      max_width: f32::INFINITY,
      min_height: constraints.min_height.min(viewport_height),
      max_height: viewport_height,
    },
    ScrollDirection::Both => Constraints {
      min_width: 0.0,
      max_width: f32::INFINITY,
      min_height: 0.0,
      max_height: f32::INFINITY,
    },
  }
}

fn scroll_container_size(
  constraints: Constraints,
  child_result: &LayoutResult,
  style: &ScrollBarStyle,
  reserve_vertical: bool,
  reserve_horizontal: bool,
) -> Size {
  let reserved_width = if reserve_vertical {
    reserved_scrollbar_size(style)
  } else {
    0.0
  };
  let reserved_height = if reserve_horizontal {
    reserved_scrollbar_size(style)
  } else {
    0.0
  };
  constraints.constrain(Size::new(
    child_result.size.width.max(constraints.min_width) + reserved_width,
    child_result.size.height.max(constraints.min_height) + reserved_height,
  ))
}

fn reserved_viewport(
  container_size: Size,
  style: &ScrollBarStyle,
  reserve_vertical: bool,
  reserve_horizontal: bool,
) -> Size {
  Size::new(
    (container_size.width
      - if reserve_vertical {
        reserved_scrollbar_size(style)
      } else {
        0.0
      })
    .max(0.0),
    (container_size.height
      - if reserve_horizontal {
        reserved_scrollbar_size(style)
      } else {
        0.0
      })
    .max(0.0),
  )
}

fn should_reserve_scrollbar(style: &ScrollBarStyle, direction: ScrollDirection, axis: ScrollAxis) -> bool {
  // Reserved placement keeps a stable gutter (CSS `scrollbar-gutter: stable`):
  // Auto visibility only decides whether the thumb is drawn, never whether the
  // gutter exists — otherwise content width toggles with overflow, oscillating
  // the child between two constraint sets across settle passes.
  style.placement == ScrollBarPlacement::Reserved
    && scroll_direction_has_axis(direction, axis)
    && style.visible != ScrollBarVisibility::Never
}

fn reserved_scrollbar_size(style: &ScrollBarStyle) -> f32 {
  style.width + style.resolved_edge_inset() * 2.0
}

fn scroll_direction_has_axis(direction: ScrollDirection, axis: ScrollAxis) -> bool {
  matches!(
    (direction, axis),
    (ScrollDirection::Horizontal, ScrollAxis::Horizontal)
      | (ScrollDirection::Vertical, ScrollAxis::Vertical)
      | (ScrollDirection::Both, _)
  )
}

fn intersect_clip(parent: ClipRect, child: ClipRect) -> ClipRect {
  if !parent.active {
    return child;
  }
  if !child.active {
    return parent;
  }

  let x1 = parent.x.max(child.x);
  let y1 = parent.y.max(child.y);
  let x2 = (parent.x + parent.width).min(child.x + child.width);
  let y2 = (parent.y + parent.height).min(child.y + child.height);

  ClipRect {
    x: x1,
    y: y1,
    width: (x2 - x1).max(0.0),
    height: (y2 - y1).max(0.0),
    active: true,
    border_radius: intersected_clip_radius(parent, child, x1, y1, x2, y2),
  }
}

fn intersected_clip_radius(
  parent: ClipRect,
  child: ClipRect,
  x1: f32,
  y1: f32,
  x2: f32,
  y2: f32,
) -> Option<crate::node::border::BorderRadius> {
  if same_clip_rect(parent, x1, y1, x2, y2) {
    return parent.border_radius;
  }
  if same_clip_rect(child, x1, y1, x2, y2) {
    return child.border_radius;
  }
  None
}

fn same_clip_rect(clip: ClipRect, x1: f32, y1: f32, x2: f32, y2: f32) -> bool {
  const EPSILON: f32 = 0.001;
  (clip.x - x1).abs() <= EPSILON
    && (clip.y - y1).abs() <= EPSILON
    && (clip.x + clip.width - x2).abs() <= EPSILON
    && (clip.y + clip.height - y2).abs() <= EPSILON
}

/// What [`LayoutEngine::push_node_quads`] hands back to the recursion in
/// `collect_quads`: the transform, opacity and clips a node's children inherit,
/// and a border to paint after them.
#[derive(Clone, Copy)]
struct NodeQuadFrame {
  transform: Transform2D,
  opacity: f32,
  child_clip: ClipRect,
  child_cull_clip: ClipRect,
  child_culling_enabled: bool,
  deferred_border: Option<ResolvedBorders>,
}

fn inset_clip_for_border(clip: ClipRect, border: Option<ResolvedBorders>) -> ClipRect {
  if !clip.active {
    return clip;
  }
  let Some(border) = border else {
    return clip;
  };

  let left = border_clip_inset(border.left);
  let top = border_clip_inset(border.top);
  let right = border_clip_inset(border.right);
  let bottom = border_clip_inset(border.bottom);
  if left == 0.0 && top == 0.0 && right == 0.0 && bottom == 0.0 {
    return clip;
  }

  ClipRect {
    x: clip.x + left,
    y: clip.y + top,
    width: (clip.width - left - right).max(0.0),
    height: (clip.height - top - bottom).max(0.0),
    active: true,
    border_radius: clip
      .border_radius
      .map(|radius| inset_border_radius(radius, left, top, right, bottom)),
  }
}

fn inset_border_radius(
  radius: crate::node::border::BorderRadius,
  left: f32,
  top: f32,
  right: f32,
  bottom: f32,
) -> crate::node::border::BorderRadius {
  crate::node::border::BorderRadius {
    top_left: (radius.top_left - left.max(top)).max(0.0),
    top_right: (radius.top_right - right.max(top)).max(0.0),
    bottom_right: (radius.bottom_right - right.max(bottom)).max(0.0),
    bottom_left: (radius.bottom_left - left.max(bottom)).max(0.0),
  }
}

fn border_clip_inset(border: Option<ResolvedBorder>) -> f32 {
  let Some(border) = border else {
    return 0.0;
  };
  match border.placement {
    BorderPlacement::Inside => border.width,
    BorderPlacement::Center => border.width * 0.5,
    BorderPlacement::Outside => 0.0,
  }
}

fn clipped_subtree_is_hidden(
  node: &Node,
  result: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  inherited_transform: Transform2D,
  clip: ClipRect,
  shadow_outset: f32,
) -> bool {
  if !clip.active
    || !inherited_transform.is_identity()
    || !node.effective_transform().is_identity()
    || node.overflow == Overflow::Visible
    || border_can_paint_outside(node)
  {
    return false;
  }

  !rect_intersects_clip(
    abs_x - shadow_outset,
    abs_y - shadow_outset,
    result.size.width + 2.0 * shadow_outset,
    result.size.height + 2.0 * shadow_outset,
    clip,
  )
}

fn node_is_plain_logical_wrapper(node: &Node) -> bool {
  matches!(node.layout_kind(), LayoutKind::LogicalModifier)
    && matches!(node.node_kind(), NodeKind::Empty)
    && node.color.as_ref().is_none()
    && node.rare_style.gradient.is_none()
    && node.border_radius.as_ref().is_none()
    && node.border.as_ref().is_none()
    && node.rare_style.box_shadow.is_none()
    && node.rare_style.caret_color.is_none()
    && node.rare_style.caret_mode.is_none()
    && node.scrollbar_style.as_ref().is_none()
    && node.state_styles.hovered.is_none()
    && node.state_styles.active.is_none()
    && node.state_styles.focused.is_none()
    && node.state_styles.focus_visible.is_none()
    && node.state_styles.drag_over.is_none()
    && node.opacity == DEFAULT_QUAD_OPACITY
    && node.animation_overrides.is_empty()
    && node.effective_transform().is_identity()
    && node.intrinsic_size.is_none()
    && {
      #[cfg(feature = "raster")]
      {
        node.background_image.as_ref().is_none()
      }
      #[cfg(not(feature = "raster"))]
      {
        true
      }
    }
}

/// Under a rotation or skew a clip can only be the bounding box of the
/// transformed rect, so a node without a visual of its own (a transformed text
/// node, for example) does not clip there.
fn hidden_overflow_creates_clip(has_visual: bool, transform: Transform2D) -> bool {
  transform.is_axis_aligned() || has_visual
}

/// The clip a node's `bounds` (in layout coordinates) give its children: inset
/// by the node's border, mapped into screen space through the node's
/// `transform`, and intersected with the inherited `parent` clip.
fn content_clip(
  parent: ClipRect,
  bounds: ClipRect,
  border: Option<ResolvedBorders>,
  transform: Transform2D,
) -> ClipRect {
  if transform.is_identity() {
    return inset_clip_for_border(intersect_clip(parent, bounds), border);
  }
  intersect_clip(parent, inset_clip_for_border(bounds, border).transformed(transform))
}

fn border_can_paint_outside(node: &Node) -> bool {
  let Some(borders) = node.get_border() else {
    return false;
  };

  [borders.top, borders.right, borders.bottom, borders.left]
    .into_iter()
    .flatten()
    .any(|border| {
      matches!(
        border.placement,
        crate::node::border::BorderPlacement::Outside | crate::node::border::BorderPlacement::Center
      )
    })
}

/// The clip `node`, laid out at `rect`, puts on its children for the overlay
/// index: a scroll container's viewport or a `clip()`ped node's bounds. It
/// leaves out what the paint clip adds (border insets, transforms), as the
/// index does.
fn children_clip(node: &Node, rect: ElementRect) -> ClipRect {
  let (width, height) = match node.layout_kind() {
    LayoutKind::ScrollModifier { state, .. } => (state.viewport_width(), state.viewport_height()),
    _ if node.overflow == Overflow::Hidden => (rect.width, rect.height),
    _ => return ClipRect::default(),
  };
  ClipRect {
    x: rect.x,
    y: rect.y,
    width,
    height,
    active: true,
    border_radius: None,
  }
}

fn rect_intersects_clip(x: f32, y: f32, width: f32, height: f32, clip: ClipRect) -> bool {
  width > 0.0
    && height > 0.0
    && x < clip.x + clip.width
    && x + width > clip.x
    && y < clip.y + clip.height
    && y + height > clip.y
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    app::glyph_engine::GlyphEngine,
    core::Signal,
    node::{Node, dsl::scroll_vertical},
  };

  #[test]
  fn layout_does_not_build_state_styles() {
    use crate::node::{Style, style::STYLE_OPERATIONS};

    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(200.0, 100.0));
    for node in [
      Node::column(0.0, Alignment::Start, vec![]).width(20.0).height(10.0),
      Node::column(0.0, Alignment::Start, vec![])
        .width(20.0)
        .height(10.0)
        .hovered_style(Style::new().width(60.0).padding(5.0).flex(2.0)),
    ] {
      for hovered in [false, true] {
        node.set_style_hovered(hovered);
        let operations = STYLE_OPERATIONS.with(|counts| counts.get());
        let result = engine.layout_node_uncached_with_child_overrides(&mut glyph_engine, &node, constraints, None);
        assert_eq!(STYLE_OPERATIONS.with(|counts| counts.get()), operations);
        assert_eq!(result.size.height, 10.0);
      }
    }
  }

  #[test]
  fn interaction_state_styles_relayout_frame_padding_and_flex() {
    use crate::node::Style;

    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let setters: [fn(&Node, bool) -> bool; 5] = [
      Node::set_style_focused,
      Node::set_style_focus_visible,
      Node::set_style_hovered,
      Node::set_style_active,
      Node::set_style_drag_over,
    ];
    for (index, set_state) in setters.into_iter().enumerate() {
      let styled = |node: Node, style: Style| match index {
        0 => node.focused_style(style),
        1 => node.focus_visible_style(style),
        2 => node.hovered_style(style),
        3 => node.active_style(style),
        _ => node.drag_over_style(style),
      };
      let node = Node::row(
        0.0,
        Alignment::Start,
        vec![
          styled(
            Node::column(0.0, Alignment::Start, vec![]).width(20.0).height(10.0),
            Style::new().width(40.0),
          ),
          styled(
            Node::column(
              0.0,
              Alignment::Start,
              vec![Node::column(0.0, Alignment::Start, vec![]).width(10.0).height(10.0)],
            )
            .padding(1.0),
            Style::new().padding(5.0),
          ),
          styled(
            Node::column(0.0, Alignment::Start, vec![]).height(10.0),
            Style::new().flex(1.0),
          ),
        ],
      );
      if index == 1 {
        for child in node.children() {
          child.set_style_focused(true);
        }
      }
      for enabled in [false, true, false] {
        for child in node.children() {
          set_state(child, enabled);
        }
        LayoutEngine::mark_layout_dirty(&node, false);
        let result = engine.layout_node(&mut glyph_engine, &node, Constraints::tight(Size::new(200.0, 100.0)));
        assert_eq!(result.children[0].result.size.width, if enabled { 40.0 } else { 20.0 });
        assert_eq!(result.children[1].result.size.width, if enabled { 20.0 } else { 12.0 });
        assert_eq!(
          result.children[1].result.children[0].offset.x,
          if enabled { 5.0 } else { 1.0 }
        );
        assert_eq!(result.children[2].result.size.width, if enabled { 140.0 } else { 0.0 });
      }
    }
  }

  fn dirty_child_with_cached_constraints(child: Node, constraints: Constraints) -> Node {
    child.layout_cache.store(
      constraints,
      LayoutResult {
        size: constraints.constrain(Size::new(10.0, 10.0)),
        children: Vec::new().into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      },
    );
    child.layout_cache.mark_local_dirty();
    child
  }

  #[test]
  fn stretch_relayout_guard_only_matches_intrinsic_main_axis_children() {
    let tight = Constraints::tight(Size::new(100.0, 24.0));
    let intrinsic = dirty_child_with_cached_constraints(Node::text("changed"), tight);
    let stretched = Node::column(0.0, Alignment::Stretch, vec![intrinsic]);
    assert!(LayoutEngine::has_dirty_stretched_intrinsic_child(&stretched));

    let non_stretched = Node::column(
      0.0,
      Alignment::Start,
      vec![dirty_child_with_cached_constraints(Node::text("changed"), tight)],
    );
    assert!(!LayoutEngine::has_dirty_stretched_intrinsic_child(&non_stretched));

    let flex_grow = Node::column(
      0.0,
      Alignment::Stretch,
      vec![dirty_child_with_cached_constraints(
        Node::text("changed").flex(1.0),
        tight,
      )],
    );
    assert!(!LayoutEngine::has_dirty_stretched_intrinsic_child(&flex_grow));

    let fixed_height = Node::column(
      0.0,
      Alignment::Stretch,
      vec![dirty_child_with_cached_constraints(
        Node::text("changed").height(24.0),
        tight,
      )],
    );
    assert!(!LayoutEngine::has_dirty_stretched_intrinsic_child(&fixed_height));

    let loose_main_axis = Node::column(
      0.0,
      Alignment::Stretch,
      vec![dirty_child_with_cached_constraints(
        Node::text("changed"),
        Constraints::loose(Size::new(100.0, 24.0)),
      )],
    );
    assert!(!LayoutEngine::has_dirty_stretched_intrinsic_child(&loose_main_axis));
  }

  #[test]
  fn unchanged_stretched_intrinsic_tree_reuses_cached_layout() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let node = Node::column(0.0, Alignment::Stretch, vec![Node::text("unchanged")]);
    let constraints = Constraints::loose(Size::new(320.0, 180.0));
    let compute = |glyph_engine: &mut GlyphEngine| {
      engine.compute(glyph_engine, &node, constraints, false);
    };

    compute(&mut glyph_engine);
    assert!(engine.last_recalculated());

    compute(&mut glyph_engine);
    assert!(!engine.last_recalculated());
  }

  /// `set_theme` copies the theme only when it changed. A change between two
  /// computes still lays out and paints with the new spacing, typography and
  /// palette; another theme at the same version is a change too; and a pass
  /// on the unchanged theme serves the cache, painting with the values it
  /// already holds.
  #[test]
  fn theme_change_between_computes_takes_effect_and_unchanged_theme_serves_cache() {
    use crate::{
      app::theme::SpacingSize,
      components::{Rect, Row, Text},
      node::Element,
    };

    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(400.0, 200.0));
    let node: Node = Element::from(
      Row::new()
        .spacing(SpacingSize::Md)
        .child(Rect::new(10.0, 10.0).background(PaletteColor::Accent))
        .child(Text::new("label")),
    )
    .into_node();
    let style = |font_size: f32| TextStyle {
      font_size,
      ..TextStyle::default()
    };
    let themed = |spacing: f32, font_size: f32, accent: &str| {
      let theme = Theme::new();
      theme.set_spacing_value(SpacingSize::Md, spacing);
      theme.set_default_text_style(style(font_size));
      theme.set_palette_color(PaletteColor::Accent, Color::from_hex(accent));
      theme
    };
    // A pass as the runtime runs it: a changed theme forces a relayout.
    let pass = |glyph_engine: &mut GlyphEngine, theme: &Theme, force_dirty: bool| {
      engine.set_theme(theme);
      let result = engine.compute(glyph_engine, &node, constraints, force_dirty);
      let quads = engine.resolve_quads(&node, &result);
      let accent = quads.iter().find_map(|quad| match quad.content {
        QuadContent::Rect { color, .. } => Some(color.to_hex()),
        _ => None,
      });
      let font_size = quads.iter().find_map(|quad| match &quad.content {
        QuadContent::Text { style, .. } => Some(style.font_size),
        _ => None,
      });
      (result.children[1].offset.x, result.size.height, accent, font_size)
    };

    let theme = themed(4.0, 12.0, "#112233");
    let (gap, small_height, accent, font_size) = pass(&mut glyph_engine, &theme, true);
    assert_eq!(gap, 14.0);
    assert_eq!(accent.as_deref(), Some("#112233"));
    assert_eq!(font_size, Some(12.0));

    theme.set_spacing_value(SpacingSize::Md, 9.0);
    theme.set_default_text_style(style(30.0));
    theme.set_palette_color(PaletteColor::Accent, Color::from_hex("#445566"));
    let (gap, height, accent, font_size) = pass(&mut glyph_engine, &theme, true);
    assert_eq!(gap, 19.0, "the new spacing lays out");
    assert!(height > small_height, "the new typography lays out");
    assert_eq!(accent.as_deref(), Some("#445566"), "the new palette paints");
    assert_eq!(font_size, Some(30.0), "the new typography paints");

    let (gap, cached_height, accent, font_size) = pass(&mut glyph_engine, &theme, false);
    assert!(!engine.last_recalculated(), "an unchanged theme serves the cache");
    assert_eq!((gap, cached_height), (19.0, height));
    assert_eq!(accent.as_deref(), Some("#445566"));
    assert_eq!(font_size, Some(30.0));

    // Versions count each theme's own changes, so a separate theme can
    // match this one's version with other values.
    let other = themed(2.0, 12.0, "#778899");
    for _ in 0..3 {
      other.set_spacing_value(SpacingSize::Md, 2.0);
    }
    assert_eq!(other.version(), theme.version());
    let (gap, _, accent, font_size) = pass(&mut glyph_engine, &other, true);
    assert_eq!(gap, 12.0);
    assert_eq!(accent.as_deref(), Some("#778899"));
    assert_eq!(font_size, Some(12.0));
  }

  #[test]
  fn clipped_subtree_culling_keeps_partially_visible_rects() {
    let clip = ClipRect {
      x: 0.0,
      y: 0.0,
      width: 100.0,
      height: 100.0,
      active: true,
      border_radius: None,
    };

    assert!(rect_intersects_clip(90.0, 90.0, 20.0, 20.0, clip));
    assert!(!rect_intersects_clip(120.0, 0.0, 20.0, 20.0, clip));
  }

  #[test]
  fn frame_conflicting_cached_result_is_recomputed() {
    // Production repro (PW-studio text preview): a retained-tree diff can
    // transplant a laid cache onto a node whose fixed frame has since changed,
    // leaving clean flags — a virtualized list's spacer then keeps serving a
    // previous window's height and the viewport goes blank. The layout walk
    // must treat a cache that contradicts the node's own fixed frame as dirty.
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(400.0, 10_000.0));
    let compute = |engine: &LayoutEngine, glyph_engine: &mut GlyphEngine, node: &Node| {
      engine.compute(glyph_engine, node, constraints, false)
    };

    let old: Node = crate::node::Element::from(crate::components::Spacer::new().height(2000.0)).into_node();
    let laid = compute(&engine, &mut glyph_engine, &old);
    assert_eq!(laid.size.height, 2000.0);

    let mut new: Node = crate::node::Element::from(crate::components::Spacer::new().height(3000.0)).into_node();
    // The transplant: previously laid cache adopted across a frame change,
    // flags cleared (what `preserve_from` does). Guards are cleared like the
    // retained diff does for unchanged content.
    new.layout_cache.preserve_from(&old.layout_cache);
    new.clear_guards();
    assert!(new.layout_cache.has_cached_result());
    assert!(!new.layout_cache.is_dirty());

    let result = compute(&engine, &mut glyph_engine, &new);
    assert_eq!(
      result.size.height, 3000.0,
      "stale transplanted cache must not override the node's fixed frame"
    );
  }

  /// A `Reserved` scroll container lays its content out at its full width,
  /// then at the width left beside the gutter. When a repaired content's
  /// size changes, the container lays out again and reuses the repaired
  /// result for the gutter width; the content's cache must then name the
  /// gutter width, not the full width it was measured at in between, or the
  /// next repair lays it out across the gutter.
  #[test]
  fn reused_child_override_becomes_the_childs_current_constraints() {
    fn scroll_content(node: &Node) -> &Node {
      if matches!(node.layout_kind(), LayoutKind::ScrollModifier { .. }) {
        return &node.children()[0];
      }
      scroll_content(&node.children()[0])
    }
    fn text_leaf(node: &mut Node) -> &mut Node {
      if node.children.is_empty() {
        return node;
      }
      text_leaf(&mut node.children[0])
    }

    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(300.0, 1000.0));
    let compute = |glyph_engine: &mut GlyphEngine, node: &Node| engine.compute(glyph_engine, node, constraints, false);
    let mut root: Node = crate::node::Element::from(
      crate::components::ScrollVertical::new(
        crate::components::Column::new()
          .width(crate::node::dimension::Dimension::Pct(100.0))
          .child(crate::components::Text::new("short")),
      )
      .scrollbar(ScrollBarStyle {
        visible: crate::layout::scrollbar::ScrollBarVisibility::Always,
        placement: crate::layout::scrollbar::ScrollBarPlacement::Reserved,
        width: 8.0,
        padding: 0.0,
        ..Default::default()
      }),
    )
    .into_node();
    compute(&mut glyph_engine, &root);
    let gutter_width = scroll_content(&root).layout_cache.constraints().unwrap().max_width;
    assert_eq!(gutter_width, 292.0, "the content is laid out beside the gutter");

    text_leaf(&mut root)
      .text_content
      .set(Some("a longer line that changes the content's size".repeat(8)));
    compute(&mut glyph_engine, &root);

    assert_eq!(
      scroll_content(&root).layout_cache.constraints().unwrap().max_width,
      gutter_width,
      "the reused override's constraints are the content's current ones"
    );
  }

  #[test]
  fn stale_parent_cache_with_conflicting_child_frame_is_recomputed() {
    // Production repro v2 (PW-studio text preview): the virtualized list's
    // rows COLUMN kept serving a clean-flagged cached result from a previous
    // window build — its spacer child's fixed frame said one height while the
    // column's cached tree still contained the old height, shifting every row
    // off the viewport. The child's own cache was fine, so the per-node
    // frame/cache check didn't fire; the parent's cached tree must be
    // validated against descendant frames.
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(400.0, 100_000.0));
    let compute = |engine: &LayoutEngine, glyph_engine: &mut GlyphEngine, node: &Node| {
      engine.compute(glyph_engine, node, constraints, false)
    };
    let column = |spacer_height: f32| -> Node {
      crate::node::Element::from(
        crate::components::Column::new()
          .spacing(0.0)
          .child(crate::components::Spacer::new().height(spacer_height))
          .child(crate::components::Spacer::new().height(50.0)),
      )
      .into_node()
    };

    let old = column(2000.0);
    let laid = compute(&engine, &mut glyph_engine, &old);
    assert_eq!(laid.size.height, 2050.0);

    let mut new = column(3000.0);
    // Lay the new tree once so every child's own cache is fresh and correct…
    compute(&engine, &mut glyph_engine, &new);
    // …then transplant the OLD column result onto the column node with clean
    // flags (what a retained-diff `preserve_from` chain does).
    new.layout_cache.preserve_from(&old.layout_cache);
    new.clear_guards();
    assert!(!new.layout_cache.is_dirty());

    let result = compute(&engine, &mut glyph_engine, &new);
    assert_eq!(
      result.size.height, 3050.0,
      "a stale parent cache conflicting with a child's fixed frame must be recomputed"
    );
    assert_eq!(result.children[0].result.size.height, 3000.0);
  }

  #[test]
  fn overlay_declaration_child_never_reports_pending_dirt() {
    // Production repro (PW-studio map screen): a popup — open or closed —
    // renders as a childless layout-neutral overlay-declaration node. Layout
    // skips such nodes entirely, so they never receive a cached result; the
    // dirty walk must not count that as dirt, or every frame after the first
    // recomputes the full tree forever (the layout fast path never engages).
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(400.0, 400.0));

    let mut root: Node =
      crate::node::Element::from(crate::components::Row::new().child(crate::components::Rect::new(40.0, 40.0)))
        .into_node();
    let mut anchor = Node::logical();
    anchor.set_layout_neutral(true);
    root.children.push(anchor);
    // Guards start "changed" on a fresh tree; the runtime clears them during
    // reconcile, which this engine-only test mimics.
    root.clear_guards();

    engine.compute(&mut glyph_engine, &root, constraints, false);

    assert!(
      !root.children[1].layout_cache.is_dirty(),
      "layout must not flag the never-laid-out overlay declaration as dirty"
    );
    assert!(
      !LayoutEngine::mark_layout_dirty(&root, false),
      "a laid-out tree with an overlay declaration must report clean"
    );
  }

  #[test]
  fn stale_parent_cache_with_conflicting_bounds_override_is_recomputed() {
    // Production repro (PW-studio move drag under an overlay-host root): the
    // overlay flow lays the base subtree first (consuming every dirty flag
    // and repairing the drag's bounds override), then computes the ROOT — the
    // second pass finds nothing dirty and serves the root's cached tree from
    // before the drag, pinning the dragged element to its declared position.
    // A cached offset contradicting a live override must invalidate the tree.
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::tight(Size::new(400.0, 300.0));
    let compute = |engine: &LayoutEngine, glyph_engine: &mut GlyphEngine, node: &Node| {
      engine.compute(glyph_engine, node, constraints, false)
    };

    let element_ref = crate::core::ElementRefMut::new();
    let root = crate::node::Element::from(
      crate::components::Row::new().child(
        crate::components::Stack::new().size(400.0, 300.0).child(
          crate::components::Rect::new(50.0, 50.0)
            .absolute_position(10.0, 20.0)
            .ref_element(element_ref.clone()),
        ),
      ),
    )
    .node;

    let laid = compute(&engine, &mut glyph_engine, &root);
    let widget_offset = laid.children[0].result.children[0].offset;
    assert_eq!((widget_offset.x, widget_offset.y), (10.0, 20.0));

    // The drag moves the element through its bounds override…
    element_ref.set_bounds(crate::core::ElementRect {
      x: 30.0,
      y: 40.0,
      relative_x: 30.0,
      relative_y: 40.0,
      width: 50.0,
      height: 50.0,
    });
    // …and the base compute (overlay flow's first pass) consumes the dirty
    // flags and repairs the stack subtree.
    let stack = &root.children()[0];
    let repaired = compute(&engine, &mut glyph_engine, stack);
    let repaired_offset = repaired.children[0].offset;
    assert_eq!((repaired_offset.x, repaired_offset.y), (30.0, 40.0));

    // The second, root-level compute must not serve the pre-drag snapshot.
    let result = compute(&engine, &mut glyph_engine, &root);
    let widget_offset = result.children[0].result.children[0].offset;
    assert_eq!(
      (widget_offset.x, widget_offset.y),
      (30.0, 40.0),
      "a cached tree contradicting a live bounds override must be recomputed"
    );
  }

  #[test]
  fn stale_parent_cache_with_conflicting_absolute_position_is_recomputed() {
    // Production repro (PW-studio editor undo): a widget's declared absolute
    // position changes (an edit reverts it) but no dirty flag is set — a
    // retained-diff that preserves the node's cache leaves the parent stack
    // serving the old child offset, so the widget stays put on screen until
    // an unrelated invalidation (a selection click). A cached offset that
    // contradicts the child's declared absolute position must be recomputed.
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::tight(Size::new(400.0, 300.0));
    let compute = |engine: &LayoutEngine, glyph_engine: &mut GlyphEngine, node: &Node| {
      engine.compute(glyph_engine, node, constraints, false)
    };
    let stack = |x: f32, y: f32| -> Node {
      crate::node::Element::from(
        crate::components::Stack::new()
          .size(400.0, 300.0)
          .child(crate::components::Rect::new(50.0, 50.0).absolute_position(x, y)),
      )
      .into_node()
    };

    let old = stack(10.0, 20.0);
    let laid = compute(&engine, &mut glyph_engine, &old);
    assert_eq!((laid.children[0].offset.x, laid.children[0].offset.y), (10.0, 20.0));

    let mut new = stack(30.0, 40.0);
    // Lay the new tree once so the child's own cache is fresh…
    compute(&engine, &mut glyph_engine, &new);
    // …then transplant the OLD stack cache (old child offset baked in) with
    // clean flags, as a retained-diff `preserve_from` chain would.
    new.layout_cache.preserve_from(&old.layout_cache);
    new.clear_guards();
    assert!(!new.layout_cache.is_dirty());

    let result = compute(&engine, &mut glyph_engine, &new);
    assert_eq!(
      (result.children[0].offset.x, result.children[0].offset.y),
      (30.0, 40.0),
      "a cached offset contradicting the child's declared absolute position must be recomputed"
    );
  }

  #[test]
  fn unchanged_text_input_reuses_cached_layout() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let node = Node::text_input(Signal::new("Hello".to_owned()));
    let constraints = Constraints::loose(Size::new(400.0, 400.0));

    engine.compute(&mut glyph_engine, &node, constraints, false);
    assert!(engine.last_recalculated());

    engine.compute(&mut glyph_engine, &node, constraints, false);
    assert!(!engine.last_recalculated());
  }

  #[test]
  fn moved_text_input_caret_invalidates_cached_layout() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let node = Node::text_input(Signal::new("Hello".to_owned()));
    let constraints = Constraints::loose(Size::new(400.0, 400.0));

    engine.compute(&mut glyph_engine, &node, constraints, false);

    let NodeKind::TextInput { state, .. } = node.node_kind() else {
      panic!("expected text input node");
    };
    state.move_left(false);

    engine.compute(&mut glyph_engine, &node, constraints, false);
    assert!(engine.last_recalculated());
  }

  #[test]
  fn unchanged_rebuilt_text_input_setters_reuse_cached_layout() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(400.0, 400.0));
    let value = Signal::new(String::new());
    let old = Node::text_input(value.clone())
      .placeholder("Display name")
      .text_input_overflow(TextInputOverflow::Scroll);

    engine.compute(&mut glyph_engine, &old, constraints, false);

    let mut new = Node::text_input(value)
      .placeholder("Display name")
      .text_input_overflow(TextInputOverflow::Scroll);
    new.preserve_runtime_state_from(&old);

    engine.compute(&mut glyph_engine, &new, constraints, false);

    assert!(!engine.last_recalculated());
  }

  #[test]
  fn dirty_child_that_still_fits_patches_cached_parent_layout() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::tight(Size::new(200.0, 40.0));
    let old = Node::stack(StackAlignment::TopStart, vec![Node::text("1")]);

    engine.compute(&mut glyph_engine, &old, constraints, false);

    let mut new = Node::stack(StackAlignment::TopStart, vec![Node::text("22")]);
    new.preserve_runtime_state_from(&old);

    let result = engine.compute(&mut glyph_engine, &new, constraints, false);

    assert!(new.layout_cache.has_cached_result());
    assert_eq!(result.size.width, 200.0);
    assert_eq!(result.size.height, 40.0);
    assert!(result.children[0].result.size.width > 0.0);
    assert!(result.children[0].result.size.width <= result.size.width);
  }

  #[test]
  fn dirty_child_that_no_longer_fits_relayouts_parent() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::tight(Size::new(20.0, 20.0));
    let old = Node::stack(StackAlignment::TopStart, vec![Node::text("1")]);

    engine.compute(&mut glyph_engine, &old, constraints, false);

    let mut new = Node::stack(StackAlignment::TopStart, vec![Node::text("this is too wide")]);
    new.preserve_runtime_state_from(&old);

    engine.compute(&mut glyph_engine, &new, constraints, false);

    assert!(engine.last_recalculated());
  }

  #[test]
  fn row_child_size_change_reflows_sibling_offsets() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::loose(Size::new(400.0, 40.0));
    let old = Node::row(8.0, Alignment::Start, vec![Node::text("1"), Node::text("tail")]);

    engine.compute(&mut glyph_engine, &old, constraints, false);

    let mut new = Node::row(
      8.0,
      Alignment::Start,
      vec![Node::text("longer label"), Node::text("tail")],
    );
    new.preserve_runtime_state_from(&old);

    let result = engine.compute(&mut glyph_engine, &new, constraints, false);

    let first = &result.children[0];
    let second = &result.children[1];
    assert!(second.offset.x >= first.offset.x + first.result.size.width + 8.0);
  }

  #[test]
  fn shared_handle_change_survives_another_tree_consuming_its_dirty_flag() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let reference = crate::core::ElementRefMut::new();
    let build = || {
      Node::stack(
        StackAlignment::TopStart,
        vec![fixed(50.0, 50.0).ref_element(reference.clone())],
      )
    };
    let first = build();
    let second = build();
    let constraints = Constraints::tight(Size::new(400.0, 300.0));
    compute_at(&engine, &mut glyph_engine, &first, constraints, false);
    compute_at(&engine, &mut glyph_engine, &second, constraints, false);
    reference.set_relative_bounds(30.0, 40.0, 80.0, 70.0);
    compute_at(&engine, &mut glyph_engine, &first, constraints, false);

    let result = compute_at(&engine, &mut glyph_engine, &second, constraints, false);

    assert_eq!(result.children[0].result.size, Size::new(80.0, 70.0));
    assert_eq!((result.children[0].offset.x, result.children[0].offset.y), (30.0, 40.0));
  }

  #[test]
  fn unchanged_large_tree_skips_layout_walks() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let root = Node::column(0.0, Alignment::Start, (0..10_000).map(|_| fixed(10.0, 1.0)).collect());
    let constraints = Constraints::tight(Size::new(400.0, 20_000.0));
    compute_at(&engine, &mut glyph_engine, &root, constraints, false);
    crate::core::layout_dirty::VISITS.with(|visits| visits.set(0));

    compute_at(&engine, &mut glyph_engine, &root, constraints, false);

    let visits = crate::core::layout_dirty::VISITS.with(|visits| visits.get());
    assert!(visits < 20, "unchanged layout visited {visits} nodes");
  }

  #[test]
  fn one_deep_leaf_change_only_visits_its_path_and_reflows_its_siblings() {
    fn branch(depth: usize) -> Node {
      if depth == 0 {
        return fixed(10.0, 1.0);
      }
      Node::column(0.0, Alignment::Start, (0..10).map(|_| branch(depth - 1)).collect())
    }
    fn same_geometry(actual: &LayoutResult, expected: &LayoutResult) {
      assert_eq!(actual.size, expected.size);
      assert_eq!(actual.children.len(), expected.children.len());
      for (actual, expected) in actual.children.iter().zip(&expected.children) {
        assert_eq!(
          (actual.offset.x, actual.offset.y),
          (expected.offset.x, expected.offset.y)
        );
        same_geometry(&actual.result, &expected.result);
      }
    }

    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let mut root = branch(4);
    let constraints = Constraints::loose(Size::new(400.0, 20_000.0));
    let old = compute_at(&engine, &mut glyph_engine, &root, constraints, false);
    // Start from carried caches as well: checking the rebuilt snapshot once
    // must not make later changes walk its unaffected branches again.
    let mut rebuilt = branch(4);
    rebuilt.preserve_runtime_state_from(&root);
    root = rebuilt;
    let before = compute_at(&engine, &mut glyph_engine, &root, constraints, false);
    same_geometry(&before, &old);
    let leaf = &mut root.children[0].children[0].children[0].children[0];
    crate::node::node::NodeUpdate::height(leaf, 2.0);
    crate::core::layout_dirty::VISITS.with(|visits| visits.set(0));
    crate::core::layout_dirty::LAYOUTS.with(|layouts| layouts.set(0));

    let repaired = compute_at(&engine, &mut glyph_engine, &root, constraints, false);

    let visits = crate::core::layout_dirty::VISITS.with(|visits| visits.get());
    let layouts = crate::core::layout_dirty::LAYOUTS.with(|layouts| layouts.get());
    assert!(visits < 100, "one changed leaf visited {visits} nodes");
    assert_eq!(layouts, 5, "only the leaf and its four ancestors need layout");
    assert_eq!(repaired.children[1].offset.y, before.children[1].offset.y + 1.0);
    assert!(Arc::ptr_eq(
      &repaired.children[1].result.children[0].result,
      &before.children[1].result.children[0].result,
    ));
    let fresh = compute_at(&engine, &mut glyph_engine, &root, constraints, true);
    same_geometry(&repaired, &fresh);
  }

  #[test]
  fn validated_rebuilt_tree_skips_later_layout_walks() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let build = || Node::column(0.0, Alignment::Start, (0..1000).map(|_| fixed(10.0, 1.0)).collect());
    let old = build();
    let constraints = Constraints::tight(Size::new(400.0, 2000.0));
    compute_at(&engine, &mut glyph_engine, &old, constraints, false);
    let mut new = build();
    new.preserve_runtime_state_from(&old);
    compute_at(&engine, &mut glyph_engine, &new, constraints, false);
    crate::core::layout_dirty::VISITS.with(|visits| visits.set(0));

    compute_at(&engine, &mut glyph_engine, &new, constraints, false);

    let visits = crate::core::layout_dirty::VISITS.with(|visits| visits.get());
    assert!(visits < 20, "validated rebuilt tree visited {visits} nodes");
  }

  /// A first layout attaches every node to its parent's watch, which marks
  /// the parent and its ancestors. One mark per parent covers all its
  /// children: a mark per child walked up the ancestors for every node, and
  /// made the first layout of a wide wrapping row cost more than a forced one.
  #[test]
  fn first_layout_notifies_each_parent_once_for_all_its_children() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let row = || Node::row(4.0, Alignment::Start, (0..1000).map(|_| fixed(50.0, 30.0)).collect()).wrap();
    let root = Node::column(0.0, Alignment::Start, vec![row(), row()]);
    let constraints = Constraints::tight(Size::new(1200.0, 800.0));
    crate::core::layout_dirty::NOTIFIED.with(|notified| notified.set(0));

    let result = compute_at(&engine, &mut glyph_engine, &root, constraints, false);

    let notified = crate::core::layout_dirty::NOTIFIED.with(|notified| notified.get());
    // The root's mark, and each row's, which reaches the root as well.
    assert_eq!(notified, 5, "a first layout of 2003 nodes notified {notified} watches");
    // 22 items of 50 with gaps of 4 fit in 1200.
    let line_start = &result.children[0].result.children[22];
    assert_eq!((line_start.offset.x, line_start.offset.y), (0.0, 34.0));
    assert_eq!(result.children[1].offset.y, result.children[0].result.size.height);
  }

  /// The setters and styles of the interaction states that can move layout
  /// on their own (focus-visible needs focus, which creates the state first).
  fn interaction_states() -> [(fn(&Node, bool) -> bool, fn(Node, crate::node::Style) -> Node); 4] {
    [
      (Node::set_style_hovered, Node::hovered_style),
      (Node::set_style_focused, Node::focused_style),
      (Node::set_style_active, Node::active_style),
      (Node::set_style_drag_over, Node::drag_over_style),
    ]
  }

  fn nested_in_columns(node: Node) -> Node {
    Node::column(
      0.0,
      Alignment::Start,
      vec![Node::column(0.0, Alignment::Start, vec![node])],
    )
  }

  /// Layout tracks a node before anything creates its interaction state, so
  /// the state binds the watch when it is created. A lost binding would serve
  /// the root's clean cached layout through the update fast path.
  #[test]
  fn interaction_state_created_after_tracking_marks_its_node_dirty() {
    use crate::node::Style;

    for (set_state, styled) in interaction_states() {
      let engine = LayoutEngine::new();
      let mut glyph_engine = GlyphEngine::new();
      let root = nested_in_columns(styled(fixed(20.0, 10.0), Style::new().width(40.0)));
      let constraints = Constraints::loose(Size::new(200.0, 100.0));
      let width = |result: &LayoutResult| result.children[0].result.children[0].result.size.width;
      let target = &root.children[0].children[0];
      assert_eq!(
        width(&compute_at(&engine, &mut glyph_engine, &root, constraints, false)),
        20.0
      );
      assert!(
        !target.style_state.has_shared_storage(),
        "layout left the state unallocated"
      );

      assert!(set_state(target, true));

      assert!(
        root.layout_cache.needs_dirty_scan(),
        "the change reached the root's watch"
      );
      assert_eq!(
        width(&compute_at(&engine, &mut glyph_engine, &root, constraints, false)),
        40.0
      );
      assert!(set_state(target, false));
      assert_eq!(
        width(&compute_at(&engine, &mut glyph_engine, &root, constraints, false)),
        20.0
      );
    }
  }

  /// A clone shares the state, so a change through it must still reach the
  /// original node even though cloning came after the original's tracking.
  #[test]
  fn interaction_state_cloned_after_tracking_marks_the_original_node_dirty() {
    use crate::node::Style;

    for (set_state, styled) in interaction_states() {
      let engine = LayoutEngine::new();
      let mut glyph_engine = GlyphEngine::new();
      let root = nested_in_columns(styled(fixed(20.0, 10.0), Style::new().width(40.0)));
      let constraints = Constraints::loose(Size::new(200.0, 100.0));
      compute_at(&engine, &mut glyph_engine, &root, constraints, false);
      let clone = root.children[0].children[0].clone_for_reuse();

      assert!(set_state(&clone, true));

      let result = compute_at(&engine, &mut glyph_engine, &root, constraints, false);
      assert_eq!(result.children[0].result.children[0].result.size.width, 40.0);
    }
  }

  #[test]
  fn revalidating_a_rebuilt_tree_leaves_interaction_states_unallocated() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let build = || Node::column(0.0, Alignment::Start, (0..10).map(|_| fixed(10.0, 1.0)).collect());
    let old = build();
    let constraints = Constraints::tight(Size::new(400.0, 200.0));
    compute_at(&engine, &mut glyph_engine, &old, constraints, false);
    let mut new = build();
    new.preserve_runtime_state_from(&old);

    compute_at(&engine, &mut glyph_engine, &new, constraints, false);

    assert!(!engine.last_recalculated());
    assert!(!new.style_state.has_shared_storage());
    assert!(
      new
        .children()
        .iter()
        .all(|child| !child.style_state.has_shared_storage())
    );
  }

  /// Each rebuild takes the layouts of the tree before it. The tree it
  /// replaced has none left, so nothing can be reading them: every rebuilt
  /// tree in the chain still lays out nothing.
  #[test]
  fn a_chain_of_rebuilds_serves_the_layouts_taken_from_the_old_trees() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let build = || Node::column(0.0, Alignment::Start, (0..10).map(|_| fixed(10.0, 1.0)).collect());
    let constraints = Constraints::tight(Size::new(400.0, 200.0));
    let mut old = build();
    let laid = compute_at(&engine, &mut glyph_engine, &old, constraints, false);

    for _ in 0..3 {
      let mut new = build();
      new.preserve_runtime_state_from(&old);
      assert!(!old.layout_cache.has_cached_result());
      assert!(
        old
          .children()
          .iter()
          .all(|child| !child.layout_cache.has_cached_result())
      );

      let result = compute_at(&engine, &mut glyph_engine, &new, constraints, false);

      assert!(!engine.last_recalculated(), "the rebuilt tree serves the old layout");
      assert_eq!(result.size, laid.size);
      assert_eq!(result.children.len(), laid.children.len());
      assert_eq!(result.children[9].offset.y, laid.children[9].offset.y);
      old = new;
    }
  }

  fn fixed(width: f32, height: f32) -> Node {
    Node::new().frame(FrameConstraints {
      width: Some(Dimension::Px(width)),
      height: Some(Dimension::Px(height)),
      ..FrameConstraints::default()
    })
  }

  fn compute_at(
    engine: &LayoutEngine,
    glyph_engine: &mut GlyphEngine,
    node: &Node,
    constraints: Constraints,
    force_dirty: bool,
  ) -> Arc<LayoutResult> {
    engine.compute(glyph_engine, node, constraints, force_dirty)
  }

  #[test]
  fn serving_an_unchanged_tree_shares_its_cached_subtrees() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::tight(Size::new(400.0, 300.0));
    let node = Node::column(
      4.0,
      Alignment::Start,
      (0..3)
        .map(|_| Node::row(2.0, Alignment::Start, vec![fixed(20.0, 10.0), fixed(30.0, 10.0)]))
        .collect(),
    );

    let first = compute_at(&engine, &mut glyph_engine, &node, constraints, false);
    let served = compute_at(&engine, &mut glyph_engine, &node, constraints, false);

    assert!(!engine.last_recalculated());
    assert!(Arc::ptr_eq(&first, &served));
    assert_eq!(
      first.holds,
      SubtreeHolds {
        scroll: false,
        text: false
      }
    );
    for (row, served_row) in first.children.iter().zip(&served.children) {
      assert!(Arc::ptr_eq(&row.result, &served_row.result));
    }
  }

  #[test]
  fn holds_record_the_scroll_containers_and_texts_below() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let node = Node::column(
      0.0,
      Alignment::Start,
      vec![
        fixed(10.0, 10.0),
        Node::text("label"),
        Node::column(0.0, Alignment::Start, vec![scroll_vertical(fixed(10.0, 500.0))]),
      ],
    );

    let result = compute_at(
      &engine,
      &mut glyph_engine,
      &node,
      Constraints::tight(Size::new(200.0, 100.0)),
      false,
    );

    assert_eq!(
      result.holds,
      SubtreeHolds {
        scroll: true,
        text: true
      }
    );
    let holds: Vec<_> = result.children.iter().map(|child| child.result.holds).collect();
    assert_eq!(
      holds,
      [
        SubtreeHolds {
          scroll: false,
          text: false
        },
        SubtreeHolds {
          scroll: false,
          text: true
        },
        SubtreeHolds {
          scroll: true,
          text: false
        },
      ]
    );
  }

  #[test]
  fn nested_scroll_container_follows_its_scroll_through_cached_and_fresh_layouts() {
    let engine = LayoutEngine::new();
    let mut glyph_engine = GlyphEngine::new();
    let constraints = Constraints::tight(Size::new(200.0, 300.0));
    let scroll = scroll_vertical(Node::column(
      0.0,
      Alignment::Start,
      (0..10).map(|_| fixed(50.0, 20.0)).collect(),
    ))
    .frame(FrameConstraints {
      height: Some(Dimension::Px(50.0)),
      ..FrameConstraints::default()
    });
    let node = Node::column(
      0.0,
      Alignment::Start,
      vec![
        fixed(10.0, 10.0),
        Node::column(
          0.0,
          Alignment::Start,
          vec![Node::row(0.0, Alignment::Start, vec![scroll])],
        ),
      ],
    );
    let content_offset = |result: &LayoutResult| {
      let scroll = &result.children[1].result.children[0].result.children[0].result;
      scroll.children[0].offset.y
    };
    let state = match node.children()[1].children()[0].children()[0].layout_kind() {
      LayoutKind::ScrollModifier { state, .. } => state.clone(),
      _ => unreachable!(),
    };

    compute_at(&engine, &mut glyph_engine, &node, constraints, false);
    state.set_scroll(0.0, 30.0);
    let scrolled = compute_at(&engine, &mut glyph_engine, &node, constraints, false);
    assert_eq!(content_offset(&scrolled), -30.0);

    // Served from the cache: nothing changed.
    let served = compute_at(&engine, &mut glyph_engine, &node, constraints, false);
    assert!(!engine.last_recalculated());
    assert_eq!(content_offset(&served), -30.0);

    // Every node laid out again.
    let fresh = compute_at(&engine, &mut glyph_engine, &node, constraints, true);
    assert_eq!(content_offset(&fresh), -30.0);

    state.set_scroll(0.0, 10.0);
    let rescrolled = compute_at(&engine, &mut glyph_engine, &node, constraints, false);
    assert_eq!(content_offset(&rescrolled), -10.0);
  }
}
