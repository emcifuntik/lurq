use crate::{
  core::Guard,
  layout::{
    Alignment, StackAlignment,
    layout_kind::{FrameConstraints, Position, ScrollDirection, ScrollState},
  },
  node::{
    dimension::Dimension,
    node::Node,
    node_kind::{NodeKind, TextOverflow},
    padding::Padding,
    spacing_value::SpacingValue,
  },
};

const DEFAULT_SCROLL_TEXT_WRAP: bool = true;
const DEFAULT_SCROLL_OPACITY: f32 = 1.0;

fn make_scroll(child: Node, direction: ScrollDirection) -> Node {
  let tag_name = child.tag_name.clone();
  Node {
    node_id: crate::core::NodeId::UNASSIGNED,
    tag_name,
    component_slot_id: None,
    component_key: None,
    element_id: None,
    classes: Vec::new(),
    outer_decoration: None,
    overlay_declaration: None,
    dropdown_anchor: None,
    modal_declaration: None,
    layout_neutral: false,
    synthetic_role: None,
    #[cfg(feature = "devtools")]
    component_props_debug: None,
    #[cfg(feature = "devtools")]
    component_signals_debug: Vec::new(),
    #[cfg(feature = "devtools")]
    component_memos_debug: Vec::new(),
    #[cfg(feature = "devtools")]
    component_effects_debug: Vec::new(),
    #[cfg(feature = "devtools")]
    component_contexts_debug: Vec::new(),
    #[cfg(any(feature = "devtools", feature = "mcp"))]
    debug_attrs: Vec::new(),
    layout_kind: crate::layout::layout_kind::LayoutKind::ScrollModifier {
      state: ScrollState::new(),
      direction,
      culling: true,
    },
    frame: FrameConstraints::default(),
    padding: Padding::default(),
    position: Position::Static,
    offset: None,
    align_self: None,
    flex: None,
    shrink_rule: crate::layout::layout_kind::ShrinkRule::default(),
    node_kind: NodeKind::Empty,
    text_content: Guard::new(None),
    text_wrap: DEFAULT_SCROLL_TEXT_WRAP,
    text_overflow: TextOverflow::default(),
    overflow: crate::layout::layout_kind::Overflow::Hidden,
    intrinsic_size: None,
    color: Guard::new(None),
    border_radius: Guard::new(None),
    border: Guard::new(None),
    rare_style: Guard::new(crate::node::lazy_box::LazyBox::new()),
    cursor: None,
    hit_test: crate::node::HitTestBehavior::default(),
    #[cfg(feature = "raster")]
    background_image: Guard::new(None),
    #[cfg(feature = "raster")]
    background_size: crate::node::node::BackgroundSize::default(),
    #[cfg(all(feature = "image", feature = "resources"))]
    background_resource_image: None,
    scrollbar_style: Guard::new(None),
    scrollbar_hovered_style: None,
    element_ref: None,
    drag_payload: None,
    interaction: None,
    focusable: None,
    tab_index: None,
    button_kind: None,
    #[cfg(feature = "form")]
    form_name: None,
    style_state: crate::node::interaction_state::InteractionState::new(),
    state_styles: crate::node::lazy_box::LazyBox::new(),
    opacity: DEFAULT_SCROLL_OPACITY,
    transform: crate::node::transform::Transform2D::IDENTITY,
    animation_overrides: Vec::new(),
    transitions: Vec::new(),
    animation: None,
    layout_cache: Default::default(),
    children: vec![child],
    events: crate::node::lazy_box::LazyBox::new(),
  }
}

pub fn scroll_vertical(child: Node) -> Node {
  make_scroll(child, ScrollDirection::Vertical)
}

pub fn scroll_horizontal(child: Node) -> Node {
  make_scroll(child, ScrollDirection::Horizontal)
}

pub fn scroll_both(child: Node) -> Node {
  make_scroll(child, ScrollDirection::Both)
}

#[allow(dead_code)]
impl Node {
  pub fn child(mut self, child: Node) -> Self {
    self.push_child(child);
    self
  }

  pub fn with_children(mut self, children: impl IntoIterator<Item = Node>) -> Self {
    for child in children {
      self.push_child(child);
    }
    self
  }

  pub fn spacing(mut self, spacing: impl Into<SpacingValue>) -> Self {
    self.set_spacing(spacing);
    self
  }

  fn set_spacing(&mut self, spacing: impl Into<SpacingValue>) {
    let spacing = spacing.into();
    match &mut self.layout_kind {
      crate::layout::layout_kind::LayoutKind::Row { spacing: s, .. } => *s = spacing,
      crate::layout::layout_kind::LayoutKind::Column { spacing: s, .. } => *s = spacing,
      _ => {}
    }
  }

  pub fn align_items(mut self, align: Alignment) -> Self {
    self.set_align_items(align);
    self
  }

  fn set_align_items(&mut self, align: Alignment) {
    match &mut self.layout_kind {
      crate::layout::layout_kind::LayoutKind::Row { align: a, .. } => *a = align,
      crate::layout::layout_kind::LayoutKind::Column { align: a, .. } => *a = align,
      _ => {}
    }
  }

  pub fn justify(mut self, justify: crate::layout::layout_kind::Justify) -> Self {
    self.set_justify(justify);
    self
  }

  fn set_justify(&mut self, justify: crate::layout::layout_kind::Justify) {
    match &mut self.layout_kind {
      crate::layout::layout_kind::LayoutKind::Row { justify: j, .. } => *j = justify,
      crate::layout::layout_kind::LayoutKind::Column { justify: j, .. } => *j = justify,
      _ => {}
    }
  }

  pub fn wrap(mut self) -> Self {
    self.set_wrap();
    self
  }

  fn set_wrap(&mut self) {
    match &mut self.layout_kind {
      crate::layout::layout_kind::LayoutKind::Row { wrap: w, .. } => *w = crate::layout::layout_kind::FlexWrap::Wrap,
      crate::layout::layout_kind::LayoutKind::Column { wrap: w, .. } => *w = crate::layout::layout_kind::FlexWrap::Wrap,
      _ => {}
    }
  }

  pub fn stack_align(mut self, align: StackAlignment) -> Self {
    self.set_stack_align(align);
    self
  }

  fn set_stack_align(&mut self, align: StackAlignment) {
    if let crate::layout::layout_kind::LayoutKind::Stack { align: a } = &mut self.layout_kind {
      *a = align;
    }
  }

  fn push_child(&mut self, child: Node) {
    match self.layout_kind {
      crate::layout::layout_kind::LayoutKind::Row { .. }
      | crate::layout::layout_kind::LayoutKind::Column { .. }
      | crate::layout::layout_kind::LayoutKind::Stack { .. } => self.children.push(child),
      _ => self.children.push(child),
    }
  }

  pub fn size(self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      width: Some(width.into()),
      height: Some(height.into()),
      ..Default::default()
    })
  }

  pub fn width(self, width: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      width: Some(width.into()),
      ..Default::default()
    })
  }

  pub fn height(self, height: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      height: Some(height.into()),
      ..Default::default()
    })
  }

  pub fn min_width(self, width: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      min_width: Some(width.into()),
      ..Default::default()
    })
  }

  pub fn max_width(self, width: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      max_width: Some(width.into()),
      ..Default::default()
    })
  }

  pub fn min_height(self, height: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      min_height: Some(height.into()),
      ..Default::default()
    })
  }

  pub fn max_height(self, height: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      max_height: Some(height.into()),
      ..Default::default()
    })
  }

  pub fn min_size(self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      min_width: Some(width.into()),
      min_height: Some(height.into()),
      ..Default::default()
    })
  }

  pub fn max_size(self, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    self.frame(FrameConstraints {
      max_width: Some(width.into()),
      max_height: Some(height.into()),
      ..Default::default()
    })
  }

  pub fn relative(self, x: f32, y: f32) -> Self {
    self.offset(x, y)
  }

  pub fn absolute(self, x: f32, y: f32, width: impl Into<Dimension>, height: impl Into<Dimension>) -> Self {
    self.absolute_positioned(x, y, Some(width.into()), Some(height.into()))
  }

  pub fn absolute_position(self, x: f32, y: f32) -> Self {
    self.absolute_positioned(x, y, None, None)
  }

  pub fn padding_horizontal(self, val: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::horizontal(val.into()))
  }

  pub fn padding_vertical(self, val: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::vertical(val.into()))
  }

  pub fn padding_left(self, val: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::new().left(val.into()))
  }

  pub fn padding_right(self, val: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::new().right(val.into()))
  }

  pub fn padding_top(self, val: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::new().top(val.into()))
  }

  pub fn padding_bottom(self, val: impl Into<SpacingValue>) -> Self {
    self.padding(Padding::new().bottom(val.into()))
  }
}
