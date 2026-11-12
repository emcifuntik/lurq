pub mod background_color;
pub mod border;
pub mod border_size_value;
pub mod box_shadow;
pub mod checkbox_style;
pub mod color;
pub(crate) mod component_output;
pub mod cursor;
pub mod dimension;
pub(crate) mod dsl;
pub mod element;
mod font_features;
pub mod gradient;
#[cfg(feature = "markdown")]
pub(crate) mod inline_link;
pub mod interaction_state;
pub(crate) mod layout_cache;
mod layout_tracking;
pub(crate) mod lazy_box;
mod letter_spacing;
pub(crate) mod node;
pub(crate) mod node_kind;
pub(crate) mod outer_decoration;
pub mod outline;
pub mod padding;
pub(crate) mod presence;
pub mod radius_value;
pub mod select_icon;
pub(crate) mod select_state;
pub mod select_style;
mod sensitive_text;
pub mod slider_style;
pub mod spacing_value;
pub mod style;
pub mod text_color;
pub mod text_highlight;
pub(crate) mod text_selection;
pub mod transform;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextTransformMode {
  #[default]
  Bitmap,
  Rasterized,
}

pub use background_color::BackgroundColor;
pub use border_size_value::BorderSizeValue;
pub use box_shadow::{BoxShadow, BoxShadowValue};
pub use checkbox_style::CheckboxStyle;
pub use cursor::CursorIcon;
pub use element::{Element, ElementChildren, ElementRef};
pub use gradient::{Gradient, GradientKind, GradientStop};
#[cfg(feature = "form")]
pub use node::FormData;
pub use node::{
  BackgroundSize, ButtonKind, EventHandler, HitTestBehavior, IntoDragEventHandler, IntoDropEventHandler,
  IntoEdgePanEventHandler, IntoEventHandler, IntoFileDragEventHandler, IntoFileDropEventHandler,
  IntoHistorySwipeEventHandler, IntoKeyboardEventHandler, IntoLongPressEventHandler, IntoMouseEventHandler,
  IntoPanEventHandler, IntoPasteEventHandler, IntoPinchEventHandler, IntoRotateEventHandler, IntoScrollEventHandler,
  IntoSwipeEventHandler, IntoTapEventHandler, IntoTextInputEventHandler, IntoTouchEventHandler, IntoVoidEventHandler,
  VoidEventHandler,
};
pub(crate) use node::{Node, NodeUpdate, SyntheticNodeRole};
pub use outline::{Outline, ResolvedOutline};
pub use radius_value::RadiusValue;
pub use select_icon::{SelectCheckmarkPosition, SelectIcon};
pub use select_style::{SelectPartStyle, SelectStyle};
pub use slider_style::SliderPartStyle;
pub use spacing_value::SpacingValue;
pub use style::Style;
pub use text_color::TextColor;
