mod button;
#[cfg(feature = "canvas")]
mod canvas;
mod checkbox;
mod column;
mod drag_container;
mod draggable;
mod drop_zone;
#[cfg(feature = "form")]
mod form;
#[cfg(all(feature = "raster", feature = "wgpu"))]
mod gpu_viewport;
#[cfg(feature = "raster")]
mod image;
#[cfg(feature = "router")]
mod link;
#[cfg(feature = "markdown")]
mod markdown;
#[cfg(feature = "router")]
mod outlet;
mod popup;
mod rect;
#[cfg(feature = "router")]
mod router_component;
mod row;
mod scroll;
mod select;
mod slider;
mod slot;
mod spacer;
mod stack;
#[cfg(feature = "router")]
pub(crate) mod stack_outlet;
#[cfg(feature = "markdown")]
pub(crate) mod styled_text;
#[cfg(feature = "svg")]
mod svg;
mod text;
mod text_input;
mod text_input_controller;
mod text_input_tokens;
#[cfg(feature = "raster")]
mod video;
mod virtualized_list;
mod window_chrome;
mod window_controls;

pub use button::Button;
#[cfg(feature = "canvas")]
pub use canvas::Canvas;
pub use checkbox::Checkbox;
pub use column::Column;
pub use drag_container::{DragBounds, DragContainer, DragContainerProps};
pub use draggable::{DragMovement, DragOverridePolicy, Draggable, DraggableProps, DropMissBehavior};
pub use drop_zone::{DropZone, DropZoneProps};
#[cfg(feature = "form")]
pub(crate) use form::FormContext;
#[cfg(feature = "form")]
pub use form::{
  Control, ControlOptions, ControlState, ErrorVisibility, Form, FormCheckboxInput, FormCheckboxInputProps,
  FormControlField, FormData, FormErrors, FormField, FormFieldProps, FormHandle, FormOptions, FormPrimaryButton,
  FormPrimaryButtonProps, FormProps, FormSecondaryButton, FormSecondaryButtonProps, FormSliderInput,
  FormSliderInputProps, FormTextInput, FormTextInputProps, FormValue, FormValues, ResolvedControl, ValidationResult,
  validators,
};
#[cfg(all(feature = "raster", feature = "wgpu"))]
pub use gpu_viewport::GpuViewport;
#[cfg(feature = "raster")]
pub use image::Image;
#[cfg(feature = "router")]
pub use link::Link;
#[cfg(feature = "markdown")]
pub use markdown::{Markdown, MarkdownLink, MarkdownLinkPresentation, MarkdownList, MarkdownProps, MarkdownRun};
#[cfg(feature = "router")]
pub use outlet::Outlet;
pub use popup::{Popover, Popup};
pub use rect::Rect;
#[cfg(feature = "router")]
pub use router_component::{Router, RouterProps};
pub use row::Row;
pub use scroll::{ScrollBoth, ScrollHorizontal, ScrollVertical};
pub use select::{Select, SelectOption};
pub use slider::Slider;
pub use slot::Slot;
pub use spacer::Spacer;
pub use stack::Stack;
#[cfg(feature = "router")]
pub use stack_outlet::{NavigationBar, NavigationItem, StackOutlet, StackOutletProps};
#[cfg(feature = "markdown")]
pub use styled_text::{
  LinkClick, MarkPresence, SelectionStyle, SpanStyle, StyledChange, StyledEdit, StyledSpan, StyledText, TextMark,
};
#[cfg(feature = "svg")]
pub use svg::Svg;
pub use text::{Text, TextOverflow};
pub use text_input::{ContentKind, KeyboardKind, ReturnKey, TextInput, TextInputOverflow, TextInputOverflowAnchor};
pub use text_input_controller::{InputEditError, InputSelection, TextInputController};
pub use text_input_tokens::{InputToken, InputTokenError, InputTokenStyle, InputTokens};
#[cfg(feature = "raster")]
pub use video::Video;
pub use virtualized_list::VirtualizedList;
pub use window_chrome::{
  ChromeBorderPolicy, ChromeTitleBar, ResizeHandlePlacement, ResizeHandlePolicy, WindowChrome, WindowChromeMetrics,
  WindowChromeMode, WindowChromeProps,
};
pub use window_controls::{
  TrafficLightColors, WindowControlColors, WindowControlContent, WindowControlKind, WindowControlStyle, WindowControls,
};

pub use crate::{
  app::ctx::{CollisionStrategy, Modal, ModalTarget, OpenState, OutsidePress, Overlay, Parent, Placement, Root},
  node::text_highlight::TextHighlight,
};

#[macro_export]
macro_rules! impl_into_node {
  ($struct_name:ident) => {
    pub struct $struct_name {
      pub(crate) node: Box<$crate::node::Node>,
    }

    impl $struct_name {
      pub(crate) fn from_node(node: $crate::node::Node) -> Self {
        Self {
          node: $crate::node::element::boxed_node(node.with_tag_name(Self::tag_name())),
        }
      }

      pub(crate) fn update_node(&mut self, f: impl FnOnce(&mut $crate::node::Node)) {
        f(&mut *self.node);
      }

      pub(crate) fn tag_name() -> std::sync::Arc<str> {
        static TAG_NAME: std::sync::OnceLock<std::sync::Arc<str>> = std::sync::OnceLock::new();
        TAG_NAME
          .get_or_init(|| std::sync::Arc::from(stringify!($struct_name)))
          .clone()
      }

      pub fn node_id(&self) -> $crate::core::NodeId {
        self.node.node_id()
      }

      pub fn key(mut self, key: impl Into<std::sync::Arc<str>>) -> Self {
        let key = key.into();
        self.update_node(|node| node.set_component_key(Some(&key)));
        self
      }

      /// Attach a semantic annotation surfaced to tooling — the devtools
      /// tree and the MCP `lurq_read_tree` — so agents can find and target
      /// this element, e.g. `.describe("role", "save-button")`. A no-op
      /// unless a tooling feature (`devtools`/`mcp`) is enabled.
      pub fn describe(mut self, name: impl Into<std::sync::Arc<str>>, value: impl Into<std::sync::Arc<str>>) -> Self {
        let name = name.into();
        let value = value.into();
        self.update_node(|node| $crate::node::NodeUpdate::describe(node, name, value));
        self
      }

      /// HTML-like `id` attribute for `Tree::get_element_by_id` lookup.
      /// Also participates in retained-node identity across sibling changes.
      /// Keep IDs stable; changing one can reset the node's runtime state.
      pub fn id(mut self, id: impl Into<std::sync::Arc<str>>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::id(node, id));
        self
      }

      /// Appends an HTML-like class for `Tree::get_elements_by_class_name`
      /// lookup. Classes do not affect reconciliation or styling.
      pub fn class(mut self, class: impl Into<std::sync::Arc<str>>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::class(node, class));
        self
      }

      /// Appends several classes at once; see `class`.
      pub fn classes<C: Into<std::sync::Arc<str>>>(mut self, classes: impl IntoIterator<Item = C>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::classes(node, classes));
        self
      }

      pub fn background(mut self, color: impl Into<$crate::node::BackgroundColor>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::background(node, color));
        self
      }

      pub fn background_gradient(mut self, gradient: impl Into<$crate::node::Gradient>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::background_gradient(node, gradient));
        self
      }

      /// CSS-like `box-shadow`: a [`ShadowStyle`](crate::app::theme::ShadowStyle)
      /// theme role, one [`BoxShadow`](crate::node::BoxShadow), or a list
      /// (the first on top). Paint only: it does not change layout or hit testing.
      pub fn box_shadow(mut self, shadow: impl Into<$crate::node::BoxShadowValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::box_shadow(node, shadow));
        self
      }

      pub fn size(
        mut self,
        width: impl Into<$crate::node::dimension::Dimension>,
        height: impl Into<$crate::node::dimension::Dimension>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::size(node, width, height));
        self
      }

      pub fn width(mut self, width: impl Into<$crate::node::dimension::Dimension>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::width(node, width));
        self
      }

      pub fn height(mut self, height: impl Into<$crate::node::dimension::Dimension>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::height(node, height));
        self
      }

      pub fn min_width(mut self, width: impl Into<$crate::node::dimension::Dimension>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::min_width(node, width));
        self
      }

      pub fn max_width(mut self, width: impl Into<$crate::node::dimension::Dimension>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::max_width(node, width));
        self
      }

      pub fn min_height(mut self, height: impl Into<$crate::node::dimension::Dimension>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::min_height(node, height));
        self
      }

      pub fn max_height(mut self, height: impl Into<$crate::node::dimension::Dimension>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::max_height(node, height));
        self
      }

      pub fn min_size(
        mut self,
        width: impl Into<$crate::node::dimension::Dimension>,
        height: impl Into<$crate::node::dimension::Dimension>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::min_size(node, width, height));
        self
      }

      pub fn max_size(
        mut self,
        width: impl Into<$crate::node::dimension::Dimension>,
        height: impl Into<$crate::node::dimension::Dimension>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::max_size(node, width, height));
        self
      }

      pub fn padding_left(mut self, val: impl Into<$crate::node::SpacingValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_left(node, val));
        self
      }

      pub fn padding_right(mut self, val: impl Into<$crate::node::SpacingValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_right(node, val));
        self
      }

      pub fn padding_top(mut self, val: impl Into<$crate::node::SpacingValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_top(node, val));
        self
      }

      pub fn padding_bottom(mut self, val: impl Into<$crate::node::SpacingValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_bottom(node, val));
        self
      }

      pub fn padding_horizontal(mut self, val: impl Into<$crate::node::SpacingValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_horizontal(node, val));
        self
      }

      pub fn padding_vertical(mut self, val: impl Into<$crate::node::SpacingValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_vertical(node, val));
        self
      }

      pub fn padding(mut self, padding: impl Into<$crate::node::padding::Padding>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding(node, padding));
        self
      }

      pub fn padding_custom(mut self, padding: $crate::node::padding::Padding) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::padding_custom(node, padding));
        self
      }

      pub fn frame(mut self, frame: $crate::layout::layout_kind::FrameConstraints) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::frame(node, frame));
        self
      }

      pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::offset(node, x, y));
        self
      }

      pub fn relative(mut self, x: f32, y: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::relative(node, x, y));
        self
      }

      pub fn absolute(
        mut self,
        x: f32,
        y: f32,
        width: impl Into<$crate::node::dimension::Dimension>,
        height: impl Into<$crate::node::dimension::Dimension>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::absolute(node, x, y, width, height));
        self
      }

      pub fn absolute_position(mut self, x: f32, y: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::absolute_position(node, x, y));
        self
      }

      pub fn align(mut self, alignment: $crate::layout::Alignment) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::align(node, alignment));
        self
      }

      pub fn flex(mut self, factor: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::flex(node, factor));
        self
      }

      pub fn flex_shrink(mut self, factor: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::flex_shrink(node, factor));
        self
      }

      pub fn flex_full(mut self, grow: f32, shrink: f32, basis: Option<f32>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::flex_full(node, grow, shrink, basis));
        self
      }

      /// The give-way order of a shrinking child (one with a `flex_shrink`
      /// factor) in a single-line `Row`/`Column`; 0 by default. When the line
      /// overflows, the children with the lowest order give way first, down
      /// to their [`ShrinkLimit`](crate::layout::layout_kind::ShrinkLimit),
      /// before any child of the next order shrinks at all. Children of one
      /// order share their part of the overflow by shrink factor, in whole
      /// pixels except for the child with the largest factor.
      pub fn shrink_order(mut self, order: i32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::shrink_order(node, order));
        self
      }

      /// How far a shrinking child (one with a `flex_shrink` factor) of a
      /// single-line `Row`/`Column` gives way: down to its minimum size (the
      /// default), down to its content, or not at all and then dropped. See
      /// [`ShrinkLimit`](crate::layout::layout_kind::ShrinkLimit).
      pub fn shrink_limit(mut self, limit: $crate::layout::layout_kind::ShrinkLimit) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::shrink_limit(node, limit));
        self
      }

      /// Lets a shrinking child (one with a `flex_shrink` factor) of a
      /// single-line `Row`/`Column` give way in two steps: it first shrinks
      /// like its [`ShrinkLimit`](crate::layout::layout_kind::ShrinkLimit),
      /// but no further than `size` on the main axis, and when its order
      /// still cannot absorb the overflow with every shrinking child at its
      /// floor, it drops like `ShrinkLimit::Drop` before the next order
      /// shrinks. A child narrower than `size` drops without shrinking. A
      /// negative or NaN `size` counts as 0.
      pub fn shrink_drop_below(mut self, size: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::shrink_drop_below(node, size));
        self
      }

      pub fn corner_radius(mut self, radius: impl Into<$crate::node::RadiusValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::corner_radius(node, radius));
        self
      }

      pub fn corner_radius_custom(mut self, radius: $crate::node::border::BorderRadius) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::corner_radius_custom(node, radius));
        self
      }

      pub fn corner_radius_top_left(mut self, radius: impl Into<$crate::node::RadiusValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::corner_radius_top_left(node, radius));
        self
      }

      pub fn corner_radius_top_right(mut self, radius: impl Into<$crate::node::RadiusValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::corner_radius_top_right(node, radius));
        self
      }

      pub fn corner_radius_bottom_right(mut self, radius: impl Into<$crate::node::RadiusValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::corner_radius_bottom_right(node, radius));
        self
      }

      pub fn corner_radius_bottom_left(mut self, radius: impl Into<$crate::node::RadiusValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::corner_radius_bottom_left(node, radius));
        self
      }

      pub fn rounded(mut self, radius: impl Into<$crate::node::RadiusValue>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::rounded(node, radius));
        self
      }

      pub fn border_inside(
        mut self,
        width: impl Into<$crate::node::BorderSizeValue>,
        color: impl Into<$crate::node::BackgroundColor>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_inside(node, width, color));
        self
      }

      pub fn border_outside(
        mut self,
        width: impl Into<$crate::node::BorderSizeValue>,
        color: impl Into<$crate::node::BackgroundColor>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_outside(node, width, color));
        self
      }

      pub fn border_center(
        mut self,
        width: impl Into<$crate::node::BorderSizeValue>,
        color: impl Into<$crate::node::BackgroundColor>,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_center(node, width, color));
        self
      }

      pub fn border(mut self, border: $crate::node::border::Border) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border(node, border));
        self
      }

      pub fn border_custom(mut self, border: $crate::node::border::Borders) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_custom(node, border));
        self
      }

      pub fn border_top(mut self, border: $crate::node::border::Border) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_top(node, border));
        self
      }

      pub fn border_right(mut self, border: $crate::node::border::Border) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_right(node, border));
        self
      }

      pub fn border_bottom(mut self, border: $crate::node::border::Border) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_bottom(node, border));
        self
      }

      pub fn border_left(mut self, border: $crate::node::border::Border) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::border_left(node, border));
        self
      }

      #[cfg(feature = "raster")]
      pub fn background_image(mut self, data: impl Into<$crate::images::ImageKind>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::background_image(node, data));
        self
      }

      #[cfg(feature = "raster")]
      pub fn background_size(mut self, size: $crate::node::BackgroundSize) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::background_size(node, size));
        self
      }

      #[cfg(feature = "raster")]
      pub fn background_cover(mut self) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::background_cover(node));
        self
      }

      #[cfg(feature = "raster")]
      pub fn background_contain(mut self) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::background_contain(node));
        self
      }

      pub fn cursor(mut self, cursor: $crate::node::CursorIcon) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::cursor(node, cursor));
        self
      }

      pub fn hovered_style(mut self, style: $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::hovered_style(node, style));
        self
      }

      pub fn active_style(mut self, style: $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::active_style(node, style));
        self
      }

      /// Merged over the element's style while it has focus, however the
      /// focus got there (pointer, keyboard or a request).
      pub fn focused_style(mut self, style: $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::focused_style(node, style));
        self
      }

      /// Merged over the element's style (and over `focused_style`) while it
      /// has keyboard focus, like CSS `:focus-visible`: a focus ring that a
      /// pointer press does not show. Text inputs show it for any focus.
      pub fn focus_visible_style(mut self, style: $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::focus_visible_style(node, style));
        self
      }

      /// Merged over the element's style while files from the OS are
      /// dragged over it (`on_file_drag_enter` fired, `on_file_drag_leave`
      /// has not), so a drop target can highlight.
      pub fn drag_over_style(mut self, style: $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::drag_over_style(node, style));
        self
      }

      pub fn hovered(mut self, f: impl FnOnce($crate::node::Style) -> $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::hovered(node, f));
        self
      }

      /// Builder form of `drag_over_style`.
      pub fn drag_over(mut self, f: impl FnOnce($crate::node::Style) -> $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::drag_over(node, f));
        self
      }

      pub fn active(mut self, f: impl FnOnce($crate::node::Style) -> $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::active(node, f));
        self
      }

      pub fn focused(mut self, f: impl FnOnce($crate::node::Style) -> $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::focused(node, f));
        self
      }

      /// Builder form of `focus_visible_style`.
      pub fn focus_visible(mut self, f: impl FnOnce($crate::node::Style) -> $crate::node::Style) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::focus_visible(node, f));
        self
      }

      pub fn on_click(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_click(node, f));
        self
      }

      pub fn off_click(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_click(node, f));
        self
      }

      pub fn on_mouse_click(
        mut self,
        button: $crate::app::events::MouseButton,
        f: impl $crate::node::IntoMouseEventHandler,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_mouse_click(node, button, f));
        self
      }

      pub fn off_mouse_click(
        mut self,
        button: $crate::app::events::MouseButton,
        f: impl $crate::node::IntoMouseEventHandler,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_mouse_click(node, button, f));
        self
      }

      pub fn on_dblclick(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_dblclick(node, f));
        self
      }

      pub fn off_dblclick(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_dblclick(node, f));
        self
      }

      pub fn on_mouse_down(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_mouse_down(node, f));
        self
      }

      pub fn off_mouse_down(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_mouse_down(node, f));
        self
      }

      pub fn on_mouse_up(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_mouse_up(node, f));
        self
      }

      pub fn off_mouse_up(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_mouse_up(node, f));
        self
      }

      pub fn on_mouse_move(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_mouse_move(node, f));
        self
      }

      pub fn off_mouse_move(mut self, f: impl $crate::node::IntoMouseEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_mouse_move(node, f));
        self
      }

      pub fn start_drag_buttons(mut self, buttons: $crate::app::events::MouseButtonMask) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::start_drag_buttons(node, buttons));
        self
      }

      pub fn on_drag_start(mut self, f: impl $crate::node::IntoDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_drag_start(node, f));
        self
      }

      pub fn off_drag_start(mut self, f: impl $crate::node::IntoDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_drag_start(node, f));
        self
      }

      pub fn on_drag_move(mut self, f: impl $crate::node::IntoDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_drag_move(node, f));
        self
      }

      pub fn off_drag_move(mut self, f: impl $crate::node::IntoDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_drag_move(node, f));
        self
      }

      pub fn on_drag_end(mut self, f: impl $crate::node::IntoDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_drag_end(node, f));
        self
      }

      pub fn off_drag_end(mut self, f: impl $crate::node::IntoDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_drag_end(node, f));
        self
      }

      pub fn on_drop(mut self, f: impl $crate::node::IntoDropEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_drop(node, f));
        self
      }

      pub fn off_drop(mut self, f: impl $crate::node::IntoDropEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_drop(node, f));
        self
      }

      /// Files from the OS (Explorer, Finder) dragged into the window reach
      /// the element under the pointer: this fires once when the drag
      /// enters the element, `on_file_drag_over` on every move over it,
      /// `on_file_drag_leave` when it leaves without a drop, and
      /// `on_file_drop` on the drop. The events bubble to ancestors; the
      /// paths are given once for the whole drag.
      pub fn on_file_drag_enter(mut self, f: impl $crate::node::IntoFileDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_file_drag_enter(node, f));
        self
      }

      pub fn off_file_drag_enter(mut self, f: impl $crate::node::IntoFileDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_file_drag_enter(node, f));
        self
      }

      pub fn on_file_drag_over(mut self, f: impl $crate::node::IntoFileDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_file_drag_over(node, f));
        self
      }

      pub fn off_file_drag_over(mut self, f: impl $crate::node::IntoFileDragEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_file_drag_over(node, f));
        self
      }

      pub fn on_file_drag_leave(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_file_drag_leave(node, f));
        self
      }

      pub fn off_file_drag_leave(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_file_drag_leave(node, f));
        self
      }

      pub fn on_file_drop(mut self, f: impl $crate::node::IntoFileDropEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_file_drop(node, f));
        self
      }

      pub fn off_file_drop(mut self, f: impl $crate::node::IntoFileDropEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_file_drop(node, f));
        self
      }

      /// A paste into the focused text input, before its text is inserted:
      /// on the input, or on an ancestor it bubbles to. The handler sees
      /// what the clipboard holds and may `prevent_default()` to take an
      /// image or files instead of inserting text.
      pub fn on_paste(mut self, f: impl $crate::node::IntoPasteEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_paste(node, f));
        self
      }

      pub fn off_paste(mut self, f: impl $crate::node::IntoPasteEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_paste(node, f));
        self
      }

      /// A trackpad swipe between pages ended over this element: two fingers moved sideways that nothing under the
      /// pointer scrolled with. Go back or forward by the event's `direction`, as a browser does.
      pub fn on_history_swipe(mut self, f: impl $crate::node::IntoHistorySwipeEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_history_swipe(node, f));
        self
      }

      pub fn off_history_swipe(mut self, f: impl $crate::node::IntoHistorySwipeEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_history_swipe(node, f));
        self
      }

      /// A finger landed on this element; see [`TouchEvent`]. A handler that calls `prevent_default` takes the finger
      /// for itself: no mouse events, click or scrolling follow for this touch.
      pub fn on_touch_start(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_touch_start(node, f));
        self
      }

      pub fn off_touch_start(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_touch_start(node, f));
        self
      }

      /// A finger that landed on this element moved, wherever it now is; see [`TouchEvent`].
      pub fn on_touch_move(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_touch_move(node, f));
        self
      }

      pub fn off_touch_move(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_touch_move(node, f));
        self
      }

      /// A finger that landed on this element lifted; see [`TouchEvent`].
      pub fn on_touch_end(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_touch_end(node, f));
        self
      }

      pub fn off_touch_end(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_touch_end(node, f));
        self
      }

      /// The system took away a finger that landed on this element; see [`TouchEvent`].
      pub fn on_touch_cancel(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_touch_cancel(node, f));
        self
      }

      pub fn off_touch_cancel(mut self, f: impl $crate::node::IntoTouchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_touch_cancel(node, f));
        self
      }

      /// A finger rested on this element for half a second; see [`LongPressEvent`].
      pub fn on_long_press(mut self, f: impl $crate::node::IntoLongPressEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_long_press(node, f));
        self
      }

      pub fn off_long_press(mut self, f: impl $crate::node::IntoLongPressEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_long_press(node, f));
        self
      }

      /// Two fingers on this element moved apart or together; see [`PinchEvent`].
      pub fn on_pinch(mut self, f: impl $crate::node::IntoPinchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_pinch(node, f));
        self
      }

      pub fn off_pinch(mut self, f: impl $crate::node::IntoPinchEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_pinch(node, f));
        self
      }

      /// Two fingers on this element turned about their middle; see [`RotateEvent`].
      pub fn on_rotate(mut self, f: impl $crate::node::IntoRotateEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_rotate(node, f));
        self
      }

      pub fn off_rotate(mut self, f: impl $crate::node::IntoRotateEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_rotate(node, f));
        self
      }

      /// One or more fingers tapped this element; see [`TapEvent`].
      pub fn on_tap(mut self, f: impl $crate::node::IntoTapEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_tap(node, f));
        self
      }

      pub fn off_tap(mut self, f: impl $crate::node::IntoTapEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_tap(node, f));
        self
      }

      /// Fingers flicked across this element; see [`SwipeEvent`].
      pub fn on_swipe(mut self, f: impl $crate::node::IntoSwipeEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_swipe(node, f));
        self
      }

      pub fn off_swipe(mut self, f: impl $crate::node::IntoSwipeEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_swipe(node, f));
        self
      }

      /// Fingers are travelling over this element; see [`PanEvent`].
      pub fn on_pan(mut self, f: impl $crate::node::IntoPanEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_pan(node, f));
        self
      }

      pub fn off_pan(mut self, f: impl $crate::node::IntoPanEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_pan(node, f));
        self
      }

      /// A finger came in from an edge of the window over this element; see [`EdgePanEvent`].
      pub fn on_edge_pan(mut self, f: impl $crate::node::IntoEdgePanEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_edge_pan(node, f));
        self
      }

      pub fn off_edge_pan(mut self, f: impl $crate::node::IntoEdgePanEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_edge_pan(node, f));
        self
      }

      pub fn on_mouse_enter(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_mouse_enter(node, f));
        self
      }

      pub fn off_mouse_enter(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_mouse_enter(node, f));
        self
      }

      pub fn on_mouse_leave(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_mouse_leave(node, f));
        self
      }

      pub fn off_mouse_leave(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_mouse_leave(node, f));
        self
      }

      pub fn on_key_down(mut self, f: impl $crate::node::IntoKeyboardEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_key_down(node, f));
        self
      }

      pub fn off_key_down(mut self, f: impl $crate::node::IntoKeyboardEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_key_down(node, f));
        self
      }

      pub fn on_key_up(mut self, f: impl $crate::node::IntoKeyboardEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_key_up(node, f));
        self
      }

      pub fn off_key_up(mut self, f: impl $crate::node::IntoKeyboardEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_key_up(node, f));
        self
      }

      pub fn on_focus(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_focus(node, f));
        self
      }

      pub fn off_focus(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_focus(node, f));
        self
      }

      pub fn on_blur(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_blur(node, f));
        self
      }

      pub fn off_blur(mut self, f: impl $crate::node::IntoVoidEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_blur(node, f));
        self
      }

      pub fn on_scroll(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_scroll(node, f));
        self
      }

      pub fn off_scroll(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_scroll(node, f));
        self
      }

      pub fn on_scroll_start(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_scroll_start(node, f));
        self
      }

      pub fn off_scroll_start(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_scroll_start(node, f));
        self
      }

      pub fn on_scroll_end(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_scroll_end(node, f));
        self
      }

      pub fn off_scroll_end(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_scroll_end(node, f));
        self
      }

      pub fn on_scroll_reach_top(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_scroll_reach_top(node, f));
        self
      }

      pub fn off_scroll_reach_top(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_scroll_reach_top(node, f));
        self
      }

      pub fn on_scroll_reach_bottom(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::on_scroll_reach_bottom(node, f));
        self
      }

      pub fn off_scroll_reach_bottom(mut self, f: impl $crate::node::IntoScrollEventHandler) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::off_scroll_reach_bottom(node, f));
        self
      }

      pub fn scrollbar(mut self, style: $crate::layout::scrollbar::ScrollBarStyle) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::scrollbar(node, style));
        self
      }

      pub fn scrollbar_hovered(
        mut self,
        f: impl Fn($crate::layout::scrollbar::ScrollBarStyle) -> $crate::layout::scrollbar::ScrollBarStyle
        + Send
        + Sync
        + 'static,
      ) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::scrollbar_hovered(node, f));
        self
      }

      pub fn hit_test(mut self, behavior: $crate::node::HitTestBehavior) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::hit_test(node, behavior));
        self
      }

      pub fn pointer_events_none(mut self) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::pointer_events_none(node));
        self
      }

      pub fn ref_element(mut self, element_ref: impl Into<$crate::core::ElementRef>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::ref_element(node, element_ref));
        self
      }

      pub fn interactive(mut self, state: $crate::node::interaction_state::InteractionState) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::interactive(node, state));
        self
      }

      /// Whether the element can take focus. Buttons, text inputs,
      /// checkboxes, sliders, selects and elements with a `tab_index` are
      /// focusable by default. `focusable(true)` makes any element focusable
      /// by click and `Ctx::focus`; `focusable(false)` keeps it from ever
      /// taking focus, by click, Tab or request.
      pub fn focusable(mut self, focusable: bool) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::focusable(node, focusable));
        self
      }

      /// HTML `tabindex`. Inside a form, controls are in the Tab order
      /// without one; outside a form, only elements with `tab_index(0)` or
      /// higher are. Positive values come first in ascending order, then `0`
      /// (and unset form controls) in tree order. `-1` removes the element
      /// from the Tab order but keeps it focusable by click. Setting a tab
      /// index makes the element focusable unless it is `focusable(false)`.
      pub fn tab_index(mut self, tab_index: i32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::tab_index(node, tab_index));
        self
      }

      #[cfg(feature = "form")]
      pub fn name(mut self, name: impl Into<std::sync::Arc<str>>) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::name(node, name));
        self
      }

      pub fn clip(mut self) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::clip(node));
        self
      }

      pub fn overflow_visible(mut self) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::overflow_visible(node));
        self
      }

      pub fn intrinsic(mut self, width: f32, height: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::intrinsic(node, width, height));
        self
      }

      pub fn opacity(mut self, value: f32) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::opacity(node, value));
        self
      }

      pub fn transition(mut self, spec: $crate::animation::Transition) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::transition(node, spec));
        self
      }

      pub fn animation(mut self, spec: $crate::animation::Animation) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::animation(node, spec));
        self
      }

      pub fn transform(mut self, t: $crate::node::transform::Transform2D) -> Self {
        self.update_node(|node| $crate::node::NodeUpdate::transform(node, t));
        self
      }
    }

    impl From<$struct_name> for $crate::node::Element {
      fn from(component: $struct_name) -> Self {
        $crate::node::Element::from_boxed_node(component.node)
      }
    }
  };
}
