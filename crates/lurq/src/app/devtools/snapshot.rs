#[cfg(feature = "perf_profile")]
use std::time::Duration;

#[cfg(feature = "perf_profile")]
use crate::app::profile_types::FrameProfile;
use crate::{
  app::{
    ctx::{
      ComponentContextDebug, ComponentEffectDebug, ComponentMemoDebug, ComponentSignalDebug, DevtoolsInspectableDebug,
    },
    runtime::Tree,
  },
  core::NodeId,
  layout::{
    Alignment, StackAlignment,
    layout_kind::{
      FlexParams, FlexWrap, FrameConstraints, Justify, LayoutKind, Position, ScrollDirection, ShrinkLimit,
    },
    layout_result::LayoutResult,
  },
  node::{
    ElementRef,
    border::{Border, BorderPlacement, Borders, ThemedBorderRadius},
    cursor::CursorIcon,
    dimension::Dimension,
    node_kind::NodeKind,
    padding::Padding,
    radius_value::RadiusValue,
    spacing_value::SpacingValue,
    style::Style,
  },
};

#[derive(Clone, Debug, PartialEq)]
pub struct DevToolsSnapshot {
  pub root: Option<DevToolsNode>,
  pub frame: FrameProfileSnapshot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DevToolsNode {
  pub id: NodeId,
  pub tag: String,
  pub kind: DevToolsNodeKind,
  pub key: Option<String>,
  pub attrs: Vec<(String, String)>,
  pub text: Option<String>,
  pub color: Option<String>,
  pub props: Option<DevtoolsInspectableDebug>,
  pub signals: Vec<ComponentSignalDebug>,
  pub memos: Vec<ComponentMemoDebug>,
  pub contexts: Vec<ComponentContextDebug>,
  pub shape: Vec<DevToolsShapeRow>,
  pub effects: Vec<ComponentEffectDebug>,
  pub layout_box: Option<DevToolsLayoutBox>,
  pub hovered: bool,
  pub active: bool,
  pub focused: bool,
  pub children: Vec<DevToolsNode>,
}

impl DevToolsNode {
  pub fn is_masked(&self) -> bool {
    self
      .attrs
      .iter()
      .any(|(name, value)| name == "masked" && value == "true")
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DevToolsRect {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DevToolsLayoutBox {
  pub bounds: DevToolsRect,
  pub relative_x: f32,
  pub relative_y: f32,
  pub content: Option<DevToolsRect>,
  pub overflow_x: bool,
  pub overflow_y: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevToolsNodeKind {
  Component,
  Element,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DevToolsShapeRow {
  pub label: String,
  pub value: Option<String>,
  pub children: Vec<DevToolsShapeRow>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameProfileSnapshot {
  pub fps: u32,
  pub total_ms: f32,
  pub layout_ms: f32,
  pub layout_recalculated: bool,
  pub quad_ms: f32,
  pub glyph_ms: f32,
  pub render_ms: f32,
  pub acquire_ms: f32,
  pub upload_ms: f32,
  pub encode_ms: f32,
  pub submit_ms: f32,
  pub present_ms: f32,
  pub quad_count: usize,
  pub rect_count: usize,
  pub glyph_count: usize,
  pub memory_kib: f32,
}

impl DevToolsSnapshot {
  pub fn from_tree(tree: &Tree) -> Self {
    Self {
      root: tree
        .root()
        .map(|root| snapshot_node(root, tree.last_layout(), 0.0, 0.0, 0.0, 0.0)),
      frame: {
        #[cfg(feature = "perf_profile")]
        {
          FrameProfileSnapshot::from_profile(tree.last_profile())
        }
        #[cfg(not(feature = "perf_profile"))]
        {
          FrameProfileSnapshot::default()
        }
      },
    }
  }

  pub(crate) fn from_tree_for_selection(tree: &Tree, selected_path: &[usize]) -> Self {
    Self {
      root: tree.root().map(|root| {
        snapshot_node_for_selection(
          root,
          tree.last_layout(),
          0.0,
          0.0,
          0.0,
          0.0,
          &mut Vec::new(),
          selected_path,
        )
      }),
      frame: {
        #[cfg(feature = "perf_profile")]
        {
          FrameProfileSnapshot::from_profile(tree.last_profile())
        }
        #[cfg(not(feature = "perf_profile"))]
        {
          FrameProfileSnapshot::default()
        }
      },
    }
  }

  pub fn empty() -> Self {
    Self {
      root: None,
      frame: FrameProfileSnapshot::default(),
    }
  }

  pub fn node_count(&self) -> usize {
    self.root.as_ref().map(count_nodes).unwrap_or(0)
  }

  pub(crate) fn selected_node<'a>(&'a self, path: &[usize]) -> Option<&'a DevToolsNode> {
    let mut node = self.root.as_ref()?;
    for index in path {
      node = node.children.get(*index)?;
    }
    Some(node)
  }
}

impl FrameProfileSnapshot {
  #[cfg(feature = "perf_profile")]
  pub fn from_profile(profile: &FrameProfile) -> Self {
    Self {
      fps: if profile.total.is_zero() {
        0
      } else {
        (1000.0 / ms(profile.total)).round() as u32
      },
      total_ms: ms(profile.total),
      layout_ms: ms(profile.layout),
      layout_recalculated: profile.layout_recalculated,
      quad_ms: ms(profile.quad_resolve),
      glyph_ms: ms(profile.glyph_rasterize),
      render_ms: ms(profile.render.active_total()),
      acquire_ms: ms(profile.render.acquire),
      upload_ms: ms(profile.render.upload_total()),
      encode_ms: ms(profile.render.encode),
      submit_ms: ms(profile.render.submit),
      present_ms: ms(profile.render.present),
      quad_count: profile.quad_count,
      rect_count: profile.rect_count,
      glyph_count: profile.glyph_count,
      memory_kib: profile.memory.total_kib(),
    }
  }
}

fn snapshot_node(
  element: ElementRef<'_>,
  layout: Option<&LayoutResult>,
  abs_x: f32,
  abs_y: f32,
  rel_x: f32,
  rel_y: f32,
) -> DevToolsNode {
  let props = element.component_props_debug().cloned();
  let signals = element.component_signals_debug().to_vec();
  let memos = element.component_memos_debug().to_vec();
  let effects = element.component_effects_debug().to_vec();
  let contexts = element.component_contexts_debug().to_vec();
  let kind = if props.is_some() || !signals.is_empty() || !memos.is_empty() || !contexts.is_empty() {
    DevToolsNodeKind::Component
  } else {
    DevToolsNodeKind::Element
  };

  DevToolsNode {
    id: element.node_id(),
    tag: element.tag_name().to_owned(),
    kind,
    key: element.component_key().map(str::to_owned),
    attrs: layout_attrs(element, layout),
    text: element.node.inspection_text(),
    color: element.color().map(|color| color.to_hex()),
    props,
    signals,
    memos,
    effects,
    contexts,
    shape: shape_rows(element),
    layout_box: layout.map(|layout| layout_box(element.node, layout, abs_x, abs_y, rel_x, rel_y)),
    hovered: element.node.style_state.is_hovered(),
    active: element.node.style_state.is_active(),
    focused: element.node.style_state.is_focused(),
    children: element
      .children()
      .into_iter()
      .enumerate()
      .map(|(index, child)| {
        let child_layout = layout.and_then(|layout| layout.children.get(index));
        let (result, x, y, rx, ry) = match child_layout {
          Some(child) => (
            Some(child.result.as_ref()),
            abs_x + child.offset.x,
            abs_y + child.offset.y,
            child.offset.x,
            child.offset.y,
          ),
          None => (None, abs_x, abs_y, 0.0, 0.0),
        };
        snapshot_node(child, result, x, y, rx, ry)
      })
      .collect(),
  }
}

/// [`element_attrs`], plus `dropped` for a child its Row/Column dropped to
/// make room (`ShrinkLimit::Drop`).
fn layout_attrs(element: ElementRef<'_>, layout: Option<&LayoutResult>) -> Vec<(String, String)> {
  let mut attrs = element_attrs(element);
  if layout.is_some_and(LayoutResult::is_dropped) {
    attrs.push(("dropped".to_owned(), "true".to_owned()));
  }
  attrs
}

/// Attribute rows for the inspector/tree: the author-supplied `id` and
/// `class` first (mirroring browser devtools), then the debug attrs.
fn element_attrs(element: ElementRef<'_>) -> Vec<(String, String)> {
  let mut attrs = Vec::new();
  if element.node.is_masked_input() {
    attrs.push(("masked".to_owned(), "true".to_owned()));
  }
  if element.node.is_sensitive_text() {
    attrs.push(("sensitive".to_owned(), "true".to_owned()));
  }
  if let Some(id) = element.id() {
    attrs.push(("id".to_owned(), id.to_owned()));
  }
  let classes = element.classes().collect::<Vec<_>>();
  if !classes.is_empty() {
    attrs.push(("class".to_owned(), classes.join(" ")));
  }
  attrs.extend(
    element
      .debug_attrs()
      .iter()
      .map(|(name, value)| (name.to_string(), value.to_string())),
  );
  attrs
}

fn snapshot_node_for_selection(
  element: ElementRef<'_>,
  layout: Option<&LayoutResult>,
  abs_x: f32,
  abs_y: f32,
  rel_x: f32,
  rel_y: f32,
  path: &mut Vec<usize>,
  selected_path: &[usize],
) -> DevToolsNode {
  let props_ref = element.component_props_debug();
  let signals_ref = element.component_signals_debug();
  let memos_ref = element.component_memos_debug();
  let effects_ref = element.component_effects_debug();
  let contexts_ref = element.component_contexts_debug();
  let kind = if props_ref.is_some() || !signals_ref.is_empty() || !memos_ref.is_empty() || !contexts_ref.is_empty() {
    DevToolsNodeKind::Component
  } else {
    DevToolsNodeKind::Element
  };
  let include_inspector_details = path.as_slice() == selected_path;

  DevToolsNode {
    id: element.node_id(),
    tag: element.tag_name().to_owned(),
    kind,
    key: element.component_key().map(str::to_owned),
    attrs: layout_attrs(element, layout),
    text: element.node.inspection_text(),
    color: element.color().map(|color| color.to_hex()),
    props: include_inspector_details.then(|| props_ref.cloned()).flatten(),
    signals: signals_ref.to_vec(),
    memos: memos_ref.to_vec(),
    effects: effects_ref.to_vec(),
    contexts: if include_inspector_details {
      contexts_ref.to_vec()
    } else {
      Vec::new()
    },
    shape: if include_inspector_details {
      shape_rows(element)
    } else {
      Vec::new()
    },
    layout_box: if include_inspector_details {
      layout.map(|layout| layout_box(element.node, layout, abs_x, abs_y, rel_x, rel_y))
    } else {
      None
    },
    hovered: element.node.style_state.is_hovered(),
    active: element.node.style_state.is_active(),
    focused: element.node.style_state.is_focused(),
    children: element
      .children()
      .into_iter()
      .enumerate()
      .map(|(index, child)| {
        let child_layout = layout.and_then(|layout| layout.children.get(index));
        let (result, x, y, rx, ry) = match child_layout {
          Some(child) => (
            Some(child.result.as_ref()),
            abs_x + child.offset.x,
            abs_y + child.offset.y,
            child.offset.x,
            child.offset.y,
          ),
          None => (None, abs_x, abs_y, 0.0, 0.0),
        };
        path.push(index);
        let node = snapshot_node_for_selection(child, result, x, y, rx, ry, path, selected_path);
        path.pop();
        node
      })
      .collect(),
  }
}

fn layout_box(
  node: &crate::node::Node,
  layout: &LayoutResult,
  abs_x: f32,
  abs_y: f32,
  rel_x: f32,
  rel_y: f32,
) -> DevToolsLayoutBox {
  let bounds = DevToolsRect {
    x: abs_x,
    y: abs_y,
    width: layout.size.width,
    height: layout.size.height,
  };
  let content = if node.padding != Padding::default() {
    layout.children.first().map(|child| DevToolsRect {
      x: abs_x + child.offset.x,
      y: abs_y + child.offset.y,
      width: child.result.size.width,
      height: child.result.size.height,
    })
  } else {
    None
  };
  let mut content_right = 0.0_f32;
  let mut content_bottom = 0.0_f32;
  for child in &layout.children {
    content_right = content_right.max(child.offset.x + child.result.size.width);
    content_bottom = content_bottom.max(child.offset.y + child.result.size.height);
  }
  DevToolsLayoutBox {
    bounds,
    relative_x: rel_x,
    relative_y: rel_y,
    content,
    overflow_x: content_right > layout.size.width + 0.5,
    overflow_y: content_bottom > layout.size.height + 0.5,
  }
}

fn shape_rows(element: ElementRef<'_>) -> Vec<DevToolsShapeRow> {
  let mut rows = Vec::new();
  push_shape_row(&mut rows, "layout", layout_name(element.node.layout_kind()));
  push_shape_row(&mut rows, "node", node_kind_name(element.node.node_kind()));
  push_layout_rows(&mut rows, element.node.layout_kind());
  push_flat_layout_rows(&mut rows, element.node);
  #[cfg(feature = "canvas")]
  if let Some(canvas) = element.node.canvas_handle() {
    let status = canvas.status();
    push_shape_row(&mut rows, "surface", format!("{:?}", canvas.surface_id()));
    push_shape_row(
      &mut rows,
      "canvas logical size",
      format!("{} x {}", status.metrics.size.width, status.metrics.size.height),
    );
    push_shape_row(
      &mut rows,
      "canvas pixels",
      format!("{} x {}", status.metrics.pixel_width, status.metrics.pixel_height),
    );
    push_shape_row(&mut rows, "canvas scale", format_number(status.metrics.scale_factor));
    push_shape_row(&mut rows, "canvas revision", status.content_revision.to_string());
    push_shape_row(
      &mut rows,
      "canvas renderer",
      if status.software { "software" } else { "GPU" },
    );
    push_shape_row(&mut rows, "canvas pending bytes", status.pending_bytes.to_string());
    push_shape_row(&mut rows, "canvas GPU backing bytes", status.gpu_bytes.to_string());
    push_shape_row(&mut rows, "canvas GPU tiles", status.gpu.tiles.to_string());
    push_shape_row(
      &mut rows,
      "canvas source upload bytes",
      status.gpu.uploaded_bytes.to_string(),
    );
    push_shape_row(
      &mut rows,
      "canvas color bytes",
      (u64::from(status.metrics.pixel_width) * u64::from(status.metrics.pixel_height) * 4).to_string(),
    );
    if let Some(error) = status.error {
      push_shape_row(&mut rows, "canvas error", error.to_string());
    }
  }

  if let Some(text) = element.node.inspection_text() {
    push_shape_row(&mut rows, "text", text);
  }
  if let Some(color) = element.color() {
    push_shape_row(&mut rows, "fill", color.to_hex());
  }
  if let Some(radius) = element.node.state_style().border_radius.or(*element.node.border_radius) {
    push_shape_group(&mut rows, "radius", themed_radius_rows(radius));
  }
  if let Some(border) = element.node.get_border() {
    push_shape_group(&mut rows, "border", border_rows(&border));
  }
  let opacity = element.node.effective_opacity();
  if (opacity - 1.0).abs() > f32::EPSILON {
    push_shape_row(&mut rows, "opacity", format_number(opacity));
  }
  push_state_style_rows(&mut rows, "hover style", element.node.state_styles.hovered.as_ref());
  push_state_style_rows(&mut rows, "active style", element.node.state_styles.active.as_ref());
  push_state_style_rows(&mut rows, "focused style", element.node.state_styles.focused.as_ref());
  push_state_style_rows(
    &mut rows,
    "focus-visible style",
    element.node.state_styles.focus_visible.as_ref(),
  );

  rows
}

fn push_state_style_rows(rows: &mut Vec<DevToolsShapeRow>, label: &str, style: Option<&Style>) {
  let Some(style) = style else {
    return;
  };
  let children = style_rows(style);
  if !children.is_empty() {
    push_shape_group(rows, label, children);
  }
}

fn push_layout_rows(rows: &mut Vec<DevToolsShapeRow>, layout: &LayoutKind) {
  match layout {
    LayoutKind::Leaf => {}
    LayoutKind::Row {
      spacing,
      align,
      justify,
      wrap,
    }
    | LayoutKind::Column {
      spacing,
      align,
      justify,
      wrap,
    } => {
      push_shape_row(rows, "spacing", format_spacing_value(spacing));
      push_shape_row(rows, "align", alignment_name(*align));
      push_shape_row(rows, "justify", justify_name(*justify));
      push_shape_row(rows, "wrap", flex_wrap_name(*wrap));
    }
    LayoutKind::Stack { align } => {
      push_shape_row(rows, "align", stack_alignment_name(*align));
    }
    LayoutKind::LogicalModifier => {}
    LayoutKind::ScrollModifier { state, direction, .. } => {
      push_shape_row(rows, "direction", scroll_direction_name(*direction));
      push_shape_group(
        rows,
        "scroll",
        vec![
          shape_leaf("x", format_px(state.scroll_x())),
          shape_leaf("y", format_px(state.scroll_y())),
        ],
      );
      push_shape_group(
        rows,
        "viewport",
        vec![
          shape_leaf("width", format_px(state.viewport_width())),
          shape_leaf("height", format_px(state.viewport_height())),
        ],
      );
      push_shape_group(
        rows,
        "content",
        vec![
          shape_leaf("width", format_px(state.content_width())),
          shape_leaf("height", format_px(state.content_height())),
        ],
      );
    }
  }
}

fn push_flat_layout_rows(rows: &mut Vec<DevToolsShapeRow>, node: &crate::node::Node) {
  let frame = node.effective_frame(FrameConstraints::default());
  if frame != FrameConstraints::default() {
    push_frame_rows(rows, &frame);
  }
  let padding = node.effective_padding(&crate::node::padding::Padding::default());
  if padding != crate::node::padding::Padding::default() {
    push_shape_group(rows, "padding", padding_rows(&padding));
  }
  if let Some(align) = node.align_self() {
    push_shape_row(rows, "align", alignment_name(align));
  }
  if let Some(flex) = node.state_flex() {
    push_flex_rows(rows, flex);
  }
  let shrink_rule = node.shrink_rule();
  if shrink_rule.order != 0 {
    push_shape_row(rows, "shrink order", shrink_rule.order.to_string());
  }
  if shrink_rule.limit != ShrinkLimit::default() {
    push_shape_row(rows, "shrink limit", format!("{:?}", shrink_rule.limit));
  }
  if let Some(size) = shrink_rule.drop_below {
    push_shape_row(rows, "shrink drop below", format_px(size));
  }
  match node.position() {
    Position::Static => {}
    Position::Absolute { x, y, width, height } => {
      let mut position = vec![shape_leaf("x", format_px(x)), shape_leaf("y", format_px(y))];
      if let Some(width) = width {
        position.push(shape_leaf("width", format_dimension(&width)));
      }
      if let Some(height) = height {
        position.push(shape_leaf("height", format_dimension(&height)));
      }
      push_shape_group(rows, "position", position);
    }
  }
  if let Some(offset) = node.offset_position() {
    push_shape_group(
      rows,
      "offset",
      vec![
        shape_leaf("x", format_px(offset.x)),
        shape_leaf("y", format_px(offset.y)),
      ],
    );
  }
}

fn push_frame_rows(rows: &mut Vec<DevToolsShapeRow>, frame: &FrameConstraints) {
  rows.extend(frame_rows(frame));
}

fn frame_rows(frame: &FrameConstraints) -> Vec<DevToolsShapeRow> {
  let mut rows = Vec::new();
  if let Some(value) = frame.width {
    push_shape_row(&mut rows, "width", format_dimension(&value));
  }
  if let Some(value) = frame.height {
    push_shape_row(&mut rows, "height", format_dimension(&value));
  }
  if let Some(value) = frame.min_width {
    push_shape_row(&mut rows, "min width", format_dimension(&value));
  }
  if let Some(value) = frame.max_width {
    push_shape_row(&mut rows, "max width", format_dimension(&value));
  }
  if let Some(value) = frame.min_height {
    push_shape_row(&mut rows, "min height", format_dimension(&value));
  }
  if let Some(value) = frame.max_height {
    push_shape_row(&mut rows, "max height", format_dimension(&value));
  }
  rows
}

fn push_flex_rows(rows: &mut Vec<DevToolsShapeRow>, flex: FlexParams) {
  rows.extend(flex_rows(flex));
}

fn flex_rows(flex: FlexParams) -> Vec<DevToolsShapeRow> {
  let mut rows = vec![
    shape_leaf("grow", format_number(flex.grow)),
    shape_leaf("shrink", format_number(flex.shrink)),
  ];
  if let Some(basis) = flex.basis {
    rows.push(shape_leaf("basis", format_px(basis)));
  }
  rows
}

fn push_shape_row(rows: &mut Vec<DevToolsShapeRow>, label: impl Into<String>, value: impl Into<String>) {
  rows.push(shape_leaf(label, value));
}

fn push_shape_group(rows: &mut Vec<DevToolsShapeRow>, label: impl Into<String>, children: Vec<DevToolsShapeRow>) {
  rows.push(shape_group(label, children));
}

fn shape_leaf(label: impl Into<String>, value: impl Into<String>) -> DevToolsShapeRow {
  DevToolsShapeRow {
    label: label.into(),
    value: Some(value.into()),
    children: Vec::new(),
  }
}

fn shape_group(label: impl Into<String>, children: Vec<DevToolsShapeRow>) -> DevToolsShapeRow {
  DevToolsShapeRow {
    label: label.into(),
    value: None,
    children,
  }
}

fn style_rows(style: &Style) -> Vec<DevToolsShapeRow> {
  let mut rows = Vec::new();
  if let Some(color) = &style.color {
    match color {
      crate::node::BackgroundColor::Color(color) => rows.push(shape_leaf("fill", color.to_hex())),
      crate::node::BackgroundColor::Palette(color) => {
        rows.push(shape_leaf("fill", format!("palette({})", color.as_str())))
      }
    }
  }
  if let Some(radius) = style.border_radius {
    rows.push(shape_group("radius", themed_radius_rows(radius)));
  }
  if let Some(border) = &style.border {
    rows.push(shape_group("border", border_rows(border)));
  }
  if let Some(cursor) = style.cursor {
    rows.push(shape_leaf("cursor", cursor_name(cursor)));
  }
  if let Some(frame) = &style.frame {
    rows.push(shape_group("frame", frame_rows(frame)));
  }
  if let Some(padding) = &style.padding {
    rows.push(shape_group("padding", padding_rows(padding)));
  }
  if let Some(flex) = style.flex {
    rows.push(shape_group("flex", flex_rows(flex)));
  }
  rows
}

fn layout_name(layout: &LayoutKind) -> &'static str {
  match layout {
    LayoutKind::Leaf => "Leaf",
    LayoutKind::Row { .. } => "Row",
    LayoutKind::Column { .. } => "Column",
    LayoutKind::Stack { .. } => "Stack",
    LayoutKind::LogicalModifier => "Logical",
    LayoutKind::ScrollModifier { .. } => "Scroll",
  }
}

fn node_kind_name(kind: &NodeKind) -> &'static str {
  match kind {
    NodeKind::Empty => "Empty",
    #[cfg(feature = "canvas")]
    NodeKind::Canvas { .. } => "Canvas",
    NodeKind::Text { .. } => "Text",
    #[cfg(feature = "markdown")]
    NodeKind::RichText { .. } => "RichText",
    #[cfg(feature = "markdown")]
    NodeKind::InlineLink { .. } => "InlineLink",
    NodeKind::TextInput { .. } => "TextInput",
    NodeKind::Checkbox { .. } => "Checkbox",
    NodeKind::Slider { .. } => "Slider",
    NodeKind::Select { .. } => "Select",
    #[cfg(feature = "raster")]
    NodeKind::Image { .. } => "Image",
    #[cfg(feature = "image")]
    NodeKind::LoadingImage { .. } => "LoadingImage",
    #[cfg(feature = "raster")]
    NodeKind::Video { .. } => "Video",
    #[cfg(feature = "image")]
    NodeKind::ResourceImage { .. } => "ResourceImage",
    #[cfg(feature = "svg")]
    NodeKind::Svg { .. } => "Svg",
    #[cfg(all(feature = "svg", feature = "resources"))]
    NodeKind::ResourceSvg { .. } => "ResourceSvg",
  }
}

fn alignment_name(align: Alignment) -> &'static str {
  match align {
    Alignment::Start => "Start",
    Alignment::Center => "Center",
    Alignment::End => "End",
    Alignment::Stretch => "Stretch",
  }
}

fn stack_alignment_name(align: StackAlignment) -> &'static str {
  match align {
    StackAlignment::TopStart => "TopStart",
    StackAlignment::TopCenter => "TopCenter",
    StackAlignment::TopEnd => "TopEnd",
    StackAlignment::CenterStart => "CenterStart",
    StackAlignment::Center => "Center",
    StackAlignment::CenterEnd => "CenterEnd",
    StackAlignment::BottomStart => "BottomStart",
    StackAlignment::BottomCenter => "BottomCenter",
    StackAlignment::BottomEnd => "BottomEnd",
  }
}

fn justify_name(justify: Justify) -> &'static str {
  match justify {
    Justify::Start => "Start",
    Justify::End => "End",
    Justify::Center => "Center",
    Justify::SpaceBetween => "SpaceBetween",
    Justify::SpaceAround => "SpaceAround",
    Justify::SpaceEvenly => "SpaceEvenly",
  }
}

fn flex_wrap_name(wrap: FlexWrap) -> &'static str {
  match wrap {
    FlexWrap::NoWrap => "NoWrap",
    FlexWrap::Wrap => "Wrap",
  }
}

fn scroll_direction_name(direction: ScrollDirection) -> &'static str {
  match direction {
    ScrollDirection::Horizontal => "Horizontal",
    ScrollDirection::Vertical => "Vertical",
    ScrollDirection::Both => "Both",
  }
}

fn padding_rows(padding: &Padding) -> Vec<DevToolsShapeRow> {
  vec![
    shape_leaf("left", format_spacing_value(&padding.left)),
    shape_leaf("right", format_spacing_value(&padding.right)),
    shape_leaf("top", format_spacing_value(&padding.top)),
    shape_leaf("bottom", format_spacing_value(&padding.bottom)),
  ]
}

fn themed_radius_rows(radius: ThemedBorderRadius) -> Vec<DevToolsShapeRow> {
  vec![
    shape_leaf("top left", format_radius_value(radius.top_left)),
    shape_leaf("top right", format_radius_value(radius.top_right)),
    shape_leaf("bottom right", format_radius_value(radius.bottom_right)),
    shape_leaf("bottom left", format_radius_value(radius.bottom_left)),
  ]
}

fn border_rows(borders: &Borders) -> Vec<DevToolsShapeRow> {
  [
    ("top", borders.top.as_ref()),
    ("right", borders.right.as_ref()),
    ("bottom", borders.bottom.as_ref()),
    ("left", borders.left.as_ref()),
  ]
  .into_iter()
  .filter_map(|(side, border)| border.map(|border| shape_group(side, single_border_rows(border))))
  .collect()
}

fn single_border_rows(border: &Border) -> Vec<DevToolsShapeRow> {
  vec![
    shape_leaf("width", format_border_size_value(border.width)),
    shape_leaf("color", format_background_color(&border.color)),
    shape_leaf("placement", border_placement_name(border.placement)),
  ]
}

fn format_background_color(color: &crate::node::BackgroundColor) -> String {
  match color {
    crate::node::BackgroundColor::Color(color) => color.to_hex(),
    crate::node::BackgroundColor::Palette(color) => format!("palette({})", color.as_str()),
  }
}

fn border_placement_name(placement: BorderPlacement) -> &'static str {
  match placement {
    BorderPlacement::Inside => "inside",
    BorderPlacement::Outside => "outside",
    BorderPlacement::Center => "center",
  }
}

fn cursor_name(cursor: CursorIcon) -> String {
  format!("{cursor:?}")
}

fn format_dimension(value: &Dimension) -> String {
  match value {
    Dimension::Auto => "auto".to_owned(),
    Dimension::Px(value) => format_px(*value),
    Dimension::Pct(value) => format!("{}%", format_number(*value)),
  }
}

fn format_spacing_value(value: &SpacingValue) -> String {
  match value {
    SpacingValue::Dimension(value) => format_dimension(value),
    SpacingValue::Theme(size) => format!("spacing({})", size.as_str()),
  }
}

fn format_radius_value(value: RadiusValue) -> String {
  match value {
    RadiusValue::Px(value) => format_px(value),
    RadiusValue::Theme(size) => format!("radius({})", size.as_str()),
  }
}

fn format_border_size_value(value: crate::node::BorderSizeValue) -> String {
  match value {
    crate::node::BorderSizeValue::Px(value) => format_px(value),
    crate::node::BorderSizeValue::Theme(size) => format!("border_size({})", size.as_str()),
  }
}

fn format_px(value: f32) -> String {
  format!("{}px", format_number(value))
}

fn format_number(value: f32) -> String {
  if (value.fract()).abs() < f32::EPSILON {
    format!("{value:.0}")
  } else {
    format!("{value:.2}")
  }
}

fn count_nodes(node: &DevToolsNode) -> usize {
  1 + node.children.iter().map(count_nodes).sum::<usize>()
}

#[cfg(feature = "perf_profile")]
fn ms(duration: Duration) -> f32 {
  duration.as_secs_f32() * 1000.0
}
