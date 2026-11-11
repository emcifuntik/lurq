use std::sync::{
  Arc,
  atomic::{AtomicU64, Ordering},
};

#[cfg(feature = "devtools")]
use crate::app::ctx::{
  ComponentContextDebug, ComponentEffectDebug, ComponentMemoDebug, ComponentSignalDebug, DevtoolsInspectableDebug,
};
use crate::{
  animation::{Animation, Transition},
  app::{
    events::{
      DragEvent, DropEvent, EdgePanEvent, FileDragEvent, FileDropEvent, HistorySwipeEvent, KeyboardEvent,
      LongPressEvent, MouseButton, MouseButtonMask, MouseEvent, PanEvent, PasteEvent, PinchEvent, RotateEvent,
      ScrollEvent, SwipeEvent, TapEvent, TextInputEvent, TouchEvent,
    },
    theme::{CaretMode, TypographyStyle},
  },
  core::{ElementRef as CoreElementRef, Guard, IdGenerator, NodeId, Signal},
  layout::{
    Alignment, Offset, Size, StackAlignment,
    layout_kind::{FlexParams, FrameConstraints, LayoutKind, Overflow, Position, ShrinkLimit, ShrinkRule},
    scrollbar::ScrollBarStyle,
    text_style::{TextAlign, TextStyle},
  },
  node::{
    BackgroundColor, TextColor, TextTransformMode,
    border::{Border, BorderRadius, Borders, ThemedBorderRadius},
    border_size_value::BorderSizeValue,
    box_shadow::BoxShadowValue,
    checkbox_style::CheckboxStyle,
    color::Color,
    cursor::CursorIcon,
    dimension::Dimension,
    gradient::Gradient,
    interaction_state::InteractionState,
    lazy_box::{LazyBox, empty_default},
    node_kind::{
      CheckboxState, NodeKind, SelectChangeCallback, SelectState, SliderState, TextInputState, TextOverflow, TextState,
      TextStyleSource,
    },
    padding::Padding,
    radius_value::RadiusValue,
    slider_style::SliderPartStyle,
    spacing_value::SpacingValue,
    style::{StateStyles, Style},
    transform::Transform2D,
  },
};

static EVENT_HANDLER_IDS: AtomicU64 = AtomicU64::new(1);

pub struct EventHandler<T> {
  id: u64,
  callback: Arc<dyn Fn(*const T) + Send + Sync>,
}

impl<T> Clone for EventHandler<T> {
  fn clone(&self) -> Self {
    Self {
      id: self.id,
      callback: self.callback.clone(),
    }
  }
}

impl<T> EventHandler<T> {
  pub fn new(f: impl Fn(&T) + Send + Sync + 'static) -> Self {
    Self {
      id: EVENT_HANDLER_IDS.fetch_add(1, Ordering::Relaxed),
      callback: Arc::new(move |event| {
        // The pointer is produced from a live shared reference in `call` and
        // is only used for the duration of this synchronous callback.
        f(unsafe { &*event });
      }),
    }
  }

  pub fn from_event(f: impl Fn(T) + Send + Sync + 'static) -> Self
  where
    T: Clone + 'static,
  {
    Self::new(move |event| f(event.clone()))
  }

  pub(crate) fn call(&self, event: &T) {
    (self.callback)(event);
  }

  pub(crate) fn same_handler(&self, other: &Self) -> bool {
    self.id == other.id
  }
}

pub trait IntoEventHandler<T> {
  fn into_event_handler(self) -> EventHandler<T>;
}

impl<T> IntoEventHandler<T> for EventHandler<T> {
  fn into_event_handler(self) -> EventHandler<T> {
    self
  }
}

pub trait IntoMouseEventHandler {
  fn into_event_handler(self) -> EventHandler<MouseEvent>;
}

impl IntoMouseEventHandler for EventHandler<MouseEvent> {
  fn into_event_handler(self) -> EventHandler<MouseEvent> {
    self
  }
}

impl<F> IntoMouseEventHandler for F
where
  F: Fn(MouseEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<MouseEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoDragEventHandler {
  fn into_event_handler(self) -> EventHandler<DragEvent>;
}

impl IntoDragEventHandler for EventHandler<DragEvent> {
  fn into_event_handler(self) -> EventHandler<DragEvent> {
    self
  }
}

impl<F> IntoDragEventHandler for F
where
  F: Fn(DragEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<DragEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoDropEventHandler {
  fn into_event_handler(self) -> EventHandler<DropEvent>;
}

impl IntoDropEventHandler for EventHandler<DropEvent> {
  fn into_event_handler(self) -> EventHandler<DropEvent> {
    self
  }
}

impl<F> IntoDropEventHandler for F
where
  F: Fn(DropEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<DropEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoFileDragEventHandler {
  fn into_event_handler(self) -> EventHandler<FileDragEvent>;
}

impl IntoFileDragEventHandler for EventHandler<FileDragEvent> {
  fn into_event_handler(self) -> EventHandler<FileDragEvent> {
    self
  }
}

impl<F> IntoFileDragEventHandler for F
where
  F: Fn(FileDragEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<FileDragEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoFileDropEventHandler {
  fn into_event_handler(self) -> EventHandler<FileDropEvent>;
}

impl IntoFileDropEventHandler for EventHandler<FileDropEvent> {
  fn into_event_handler(self) -> EventHandler<FileDropEvent> {
    self
  }
}

impl<F> IntoFileDropEventHandler for F
where
  F: Fn(FileDropEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<FileDropEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoHistorySwipeEventHandler {
  fn into_event_handler(self) -> EventHandler<HistorySwipeEvent>;
}

impl IntoHistorySwipeEventHandler for EventHandler<HistorySwipeEvent> {
  fn into_event_handler(self) -> EventHandler<HistorySwipeEvent> {
    self
  }
}

impl<F> IntoHistorySwipeEventHandler for F
where
  F: Fn(HistorySwipeEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<HistorySwipeEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoTouchEventHandler {
  fn into_event_handler(self) -> EventHandler<TouchEvent>;
}

impl IntoTouchEventHandler for EventHandler<TouchEvent> {
  fn into_event_handler(self) -> EventHandler<TouchEvent> {
    self
  }
}

impl<F> IntoTouchEventHandler for F
where
  F: Fn(TouchEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<TouchEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoLongPressEventHandler {
  fn into_event_handler(self) -> EventHandler<LongPressEvent>;
}

impl IntoLongPressEventHandler for EventHandler<LongPressEvent> {
  fn into_event_handler(self) -> EventHandler<LongPressEvent> {
    self
  }
}

impl<F> IntoLongPressEventHandler for F
where
  F: Fn(LongPressEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<LongPressEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoPinchEventHandler {
  fn into_event_handler(self) -> EventHandler<PinchEvent>;
}

impl IntoPinchEventHandler for EventHandler<PinchEvent> {
  fn into_event_handler(self) -> EventHandler<PinchEvent> {
    self
  }
}

impl<F> IntoPinchEventHandler for F
where
  F: Fn(PinchEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<PinchEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoRotateEventHandler {
  fn into_event_handler(self) -> EventHandler<RotateEvent>;
}

impl IntoRotateEventHandler for EventHandler<RotateEvent> {
  fn into_event_handler(self) -> EventHandler<RotateEvent> {
    self
  }
}

impl<F> IntoRotateEventHandler for F
where
  F: Fn(RotateEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<RotateEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoTapEventHandler {
  fn into_event_handler(self) -> EventHandler<TapEvent>;
}

impl IntoTapEventHandler for EventHandler<TapEvent> {
  fn into_event_handler(self) -> EventHandler<TapEvent> {
    self
  }
}

impl<F> IntoTapEventHandler for F
where
  F: Fn(TapEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<TapEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoSwipeEventHandler {
  fn into_event_handler(self) -> EventHandler<SwipeEvent>;
}

impl IntoSwipeEventHandler for EventHandler<SwipeEvent> {
  fn into_event_handler(self) -> EventHandler<SwipeEvent> {
    self
  }
}

impl<F> IntoSwipeEventHandler for F
where
  F: Fn(SwipeEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<SwipeEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoPanEventHandler {
  fn into_event_handler(self) -> EventHandler<PanEvent>;
}

impl IntoPanEventHandler for EventHandler<PanEvent> {
  fn into_event_handler(self) -> EventHandler<PanEvent> {
    self
  }
}

impl<F> IntoPanEventHandler for F
where
  F: Fn(PanEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<PanEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoEdgePanEventHandler {
  fn into_event_handler(self) -> EventHandler<EdgePanEvent>;
}

impl IntoEdgePanEventHandler for EventHandler<EdgePanEvent> {
  fn into_event_handler(self) -> EventHandler<EdgePanEvent> {
    self
  }
}

impl<F> IntoEdgePanEventHandler for F
where
  F: Fn(EdgePanEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<EdgePanEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoPasteEventHandler {
  fn into_event_handler(self) -> EventHandler<PasteEvent>;
}

impl IntoPasteEventHandler for EventHandler<PasteEvent> {
  fn into_event_handler(self) -> EventHandler<PasteEvent> {
    self
  }
}

impl<F> IntoPasteEventHandler for F
where
  F: Fn(PasteEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<PasteEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoKeyboardEventHandler {
  fn into_event_handler(self) -> EventHandler<KeyboardEvent>;
}

impl IntoKeyboardEventHandler for EventHandler<KeyboardEvent> {
  fn into_event_handler(self) -> EventHandler<KeyboardEvent> {
    self
  }
}

impl<F> IntoKeyboardEventHandler for F
where
  F: Fn(KeyboardEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<KeyboardEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoScrollEventHandler {
  fn into_event_handler(self) -> EventHandler<ScrollEvent>;
}

impl IntoScrollEventHandler for EventHandler<ScrollEvent> {
  fn into_event_handler(self) -> EventHandler<ScrollEvent> {
    self
  }
}

impl<F> IntoScrollEventHandler for F
where
  F: Fn(ScrollEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<ScrollEvent> {
    EventHandler::from_event(self)
  }
}

pub trait IntoTextInputEventHandler {
  fn into_event_handler(self) -> EventHandler<TextInputEvent>;
}

impl IntoTextInputEventHandler for EventHandler<TextInputEvent> {
  fn into_event_handler(self) -> EventHandler<TextInputEvent> {
    self
  }
}

impl<F> IntoTextInputEventHandler for F
where
  F: Fn(TextInputEvent) + Send + Sync + 'static,
{
  fn into_event_handler(self) -> EventHandler<TextInputEvent> {
    EventHandler::from_event(self)
  }
}

#[derive(Clone)]
pub struct VoidEventHandler {
  id: u64,
  callback: Arc<dyn Fn() + Send + Sync>,
}

impl VoidEventHandler {
  pub fn new(f: impl Fn() + Send + Sync + 'static) -> Self {
    Self {
      id: EVENT_HANDLER_IDS.fetch_add(1, Ordering::Relaxed),
      callback: Arc::new(f),
    }
  }

  pub(crate) fn call(&self) {
    (self.callback)();
  }

  pub(crate) fn same_handler(&self, other: &Self) -> bool {
    self.id == other.id
  }
}

pub trait IntoVoidEventHandler {
  fn into_void_event_handler(self) -> VoidEventHandler;
}

impl IntoVoidEventHandler for VoidEventHandler {
  fn into_void_event_handler(self) -> VoidEventHandler {
    self
  }
}

impl<F> IntoVoidEventHandler for F
where
  F: Fn() + Send + Sync + 'static,
{
  fn into_void_event_handler(self) -> VoidEventHandler {
    VoidEventHandler::new(self)
  }
}

type Callback<T> = EventHandler<T>;
type VoidCallback = VoidEventHandler;
type ScrollbarStyleCallback = Arc<dyn Fn(ScrollBarStyle) -> ScrollBarStyle + Send + Sync>;

#[allow(private_bounds)]
pub(crate) trait NodeUpdate {
  /// Attach a semantic annotation surfaced to tooling (the devtools tree and
  /// the MCP `lurq_read_tree`). A no-op unless a tooling feature is enabled.
  fn describe(&mut self, name: Arc<str>, value: Arc<str>);
  fn child(&mut self, child: Node);
  fn id(&mut self, id: impl Into<Arc<str>>);
  fn class(&mut self, class: impl Into<Arc<str>>);
  fn classes(&mut self, classes: impl IntoIterator<Item: Into<Arc<str>>>);
  fn spacing(&mut self, spacing: impl Into<SpacingValue>);
  fn align_items(&mut self, align: Alignment);
  fn justify(&mut self, justify: crate::layout::layout_kind::Justify);
  fn wrap(&mut self);
  fn stack_align(&mut self, align: StackAlignment);
  fn size(&mut self, width: impl Into<Dimension>, height: impl Into<Dimension>);
  fn width(&mut self, width: impl Into<Dimension>);
  fn height(&mut self, height: impl Into<Dimension>);
  fn min_width(&mut self, width: impl Into<Dimension>);
  fn max_width(&mut self, width: impl Into<Dimension>);
  fn min_height(&mut self, height: impl Into<Dimension>);
  fn max_height(&mut self, height: impl Into<Dimension>);
  fn min_size(&mut self, width: impl Into<Dimension>, height: impl Into<Dimension>);
  fn max_size(&mut self, width: impl Into<Dimension>, height: impl Into<Dimension>);
  fn padding_left(&mut self, val: impl Into<SpacingValue>);
  fn padding_right(&mut self, val: impl Into<SpacingValue>);
  fn padding_top(&mut self, val: impl Into<SpacingValue>);
  fn padding_bottom(&mut self, val: impl Into<SpacingValue>);
  fn padding_horizontal(&mut self, val: impl Into<SpacingValue>);
  fn padding_vertical(&mut self, val: impl Into<SpacingValue>);
  fn padding(&mut self, padding: impl Into<Padding>);
  fn padding_custom(&mut self, padding: Padding);
  fn frame(&mut self, frame: FrameConstraints);
  fn offset(&mut self, x: f32, y: f32);
  fn relative(&mut self, x: f32, y: f32);
  fn absolute(&mut self, x: f32, y: f32, width: impl Into<Dimension>, height: impl Into<Dimension>);
  fn absolute_position(&mut self, x: f32, y: f32);
  fn align(&mut self, alignment: Alignment);
  fn flex(&mut self, factor: f32);
  fn flex_shrink(&mut self, factor: f32);
  fn flex_full(&mut self, grow: f32, shrink: f32, basis: Option<f32>);
  fn shrink_order(&mut self, order: i32);
  fn shrink_limit(&mut self, limit: ShrinkLimit);
  fn shrink_drop_below(&mut self, size: f32);
  fn background(&mut self, color: impl Into<BackgroundColor>);
  fn background_gradient(&mut self, gradient: impl Into<Gradient>);
  fn caret_color(&mut self, color: impl Into<TextColor>);
  fn selection_color(&mut self, color: impl Into<TextColor>);
  fn text_input_caret_mode(&mut self, mode: CaretMode);
  fn corner_radius(&mut self, radius: impl Into<RadiusValue>);
  fn corner_radius_custom(&mut self, radius: BorderRadius);
  fn corner_radius_top_left(&mut self, radius: impl Into<RadiusValue>);
  fn corner_radius_top_right(&mut self, radius: impl Into<RadiusValue>);
  fn corner_radius_bottom_right(&mut self, radius: impl Into<RadiusValue>);
  fn corner_radius_bottom_left(&mut self, radius: impl Into<RadiusValue>);
  fn rounded(&mut self, radius: impl Into<RadiusValue>);
  fn border_inside(&mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>);
  fn border_outside(&mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>);
  fn border_center(&mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>);
  fn border(&mut self, border: Border);
  fn border_custom(&mut self, border: Borders);
  fn border_top(&mut self, border: Border);
  fn border_right(&mut self, border: Border);
  fn border_bottom(&mut self, border: Border);
  fn border_left(&mut self, border: Border);
  fn box_shadow(&mut self, shadow: impl Into<BoxShadowValue>);
  fn cursor(&mut self, cursor: CursorIcon);
  #[cfg(feature = "raster")]
  fn background_image(&mut self, data: impl Into<crate::images::ImageKind>);
  #[cfg(feature = "raster")]
  fn background_size(&mut self, size: BackgroundSize);
  #[cfg(feature = "raster")]
  fn background_cover(&mut self);
  #[cfg(feature = "raster")]
  fn background_contain(&mut self);
  fn hovered_style(&mut self, style: Style);
  fn active_style(&mut self, style: Style);
  fn focused_style(&mut self, style: Style);
  fn focus_visible_style(&mut self, style: Style);
  fn drag_over_style(&mut self, style: Style);
  fn hovered(&mut self, f: impl FnOnce(Style) -> Style);
  fn active(&mut self, f: impl FnOnce(Style) -> Style);
  fn focused(&mut self, f: impl FnOnce(Style) -> Style);
  fn focus_visible(&mut self, f: impl FnOnce(Style) -> Style);
  fn drag_over(&mut self, f: impl FnOnce(Style) -> Style);
  fn on_click(&mut self, f: impl IntoMouseEventHandler);
  fn off_click(&mut self, f: impl IntoMouseEventHandler);
  fn on_mouse_click(&mut self, button: MouseButton, f: impl IntoMouseEventHandler);
  fn off_mouse_click(&mut self, button: MouseButton, f: impl IntoMouseEventHandler);
  fn on_dblclick(&mut self, f: impl IntoMouseEventHandler);
  fn off_dblclick(&mut self, f: impl IntoMouseEventHandler);
  fn on_mouse_down(&mut self, f: impl IntoMouseEventHandler);
  fn off_mouse_down(&mut self, f: impl IntoMouseEventHandler);
  fn on_mouse_up(&mut self, f: impl IntoMouseEventHandler);
  fn off_mouse_up(&mut self, f: impl IntoMouseEventHandler);
  fn on_mouse_move(&mut self, f: impl IntoMouseEventHandler);
  fn off_mouse_move(&mut self, f: impl IntoMouseEventHandler);
  fn start_drag_buttons(&mut self, buttons: MouseButtonMask);
  fn on_drag_start(&mut self, f: impl IntoDragEventHandler);
  fn off_drag_start(&mut self, f: impl IntoDragEventHandler);
  fn on_drag_move(&mut self, f: impl IntoDragEventHandler);
  fn off_drag_move(&mut self, f: impl IntoDragEventHandler);
  fn on_drag_end(&mut self, f: impl IntoDragEventHandler);
  fn off_drag_end(&mut self, f: impl IntoDragEventHandler);
  fn on_drop(&mut self, f: impl IntoDropEventHandler);
  fn off_drop(&mut self, f: impl IntoDropEventHandler);
  fn on_file_drag_enter(&mut self, f: impl IntoFileDragEventHandler);
  fn off_file_drag_enter(&mut self, f: impl IntoFileDragEventHandler);
  fn on_file_drag_over(&mut self, f: impl IntoFileDragEventHandler);
  fn off_file_drag_over(&mut self, f: impl IntoFileDragEventHandler);
  fn on_file_drag_leave(&mut self, f: impl IntoVoidEventHandler);
  fn off_file_drag_leave(&mut self, f: impl IntoVoidEventHandler);
  fn on_file_drop(&mut self, f: impl IntoFileDropEventHandler);
  fn off_file_drop(&mut self, f: impl IntoFileDropEventHandler);
  fn on_paste(&mut self, f: impl IntoPasteEventHandler);
  fn off_paste(&mut self, f: impl IntoPasteEventHandler);
  fn on_mouse_enter(&mut self, f: impl IntoVoidEventHandler);
  fn off_mouse_enter(&mut self, f: impl IntoVoidEventHandler);
  fn on_mouse_leave(&mut self, f: impl IntoVoidEventHandler);
  fn off_mouse_leave(&mut self, f: impl IntoVoidEventHandler);
  fn on_key_down(&mut self, f: impl IntoKeyboardEventHandler);
  fn off_key_down(&mut self, f: impl IntoKeyboardEventHandler);
  fn on_key_up(&mut self, f: impl IntoKeyboardEventHandler);
  fn off_key_up(&mut self, f: impl IntoKeyboardEventHandler);
  fn on_focus(&mut self, f: impl IntoVoidEventHandler);
  fn off_focus(&mut self, f: impl IntoVoidEventHandler);
  fn on_blur(&mut self, f: impl IntoVoidEventHandler);
  fn off_blur(&mut self, f: impl IntoVoidEventHandler);
  fn on_scroll(&mut self, f: impl IntoScrollEventHandler);
  fn off_scroll(&mut self, f: impl IntoScrollEventHandler);
  fn on_scroll_start(&mut self, f: impl IntoScrollEventHandler);
  fn off_scroll_start(&mut self, f: impl IntoScrollEventHandler);
  fn on_scroll_end(&mut self, f: impl IntoScrollEventHandler);
  fn off_scroll_end(&mut self, f: impl IntoScrollEventHandler);
  fn on_scroll_reach_top(&mut self, f: impl IntoScrollEventHandler);
  fn off_scroll_reach_top(&mut self, f: impl IntoScrollEventHandler);
  fn on_scroll_reach_bottom(&mut self, f: impl IntoScrollEventHandler);
  fn off_scroll_reach_bottom(&mut self, f: impl IntoScrollEventHandler);
  fn on_history_swipe(&mut self, f: impl IntoHistorySwipeEventHandler);
  fn off_history_swipe(&mut self, f: impl IntoHistorySwipeEventHandler);
  fn on_touch_start(&mut self, f: impl IntoTouchEventHandler);
  fn off_touch_start(&mut self, f: impl IntoTouchEventHandler);
  fn on_touch_move(&mut self, f: impl IntoTouchEventHandler);
  fn off_touch_move(&mut self, f: impl IntoTouchEventHandler);
  fn on_touch_end(&mut self, f: impl IntoTouchEventHandler);
  fn off_touch_end(&mut self, f: impl IntoTouchEventHandler);
  fn on_touch_cancel(&mut self, f: impl IntoTouchEventHandler);
  fn off_touch_cancel(&mut self, f: impl IntoTouchEventHandler);
  fn on_long_press(&mut self, f: impl IntoLongPressEventHandler);
  fn off_long_press(&mut self, f: impl IntoLongPressEventHandler);
  fn on_pinch(&mut self, f: impl IntoPinchEventHandler);
  fn off_pinch(&mut self, f: impl IntoPinchEventHandler);
  fn on_rotate(&mut self, f: impl IntoRotateEventHandler);
  fn off_rotate(&mut self, f: impl IntoRotateEventHandler);
  fn on_tap(&mut self, f: impl IntoTapEventHandler);
  fn off_tap(&mut self, f: impl IntoTapEventHandler);
  fn on_swipe(&mut self, f: impl IntoSwipeEventHandler);
  fn off_swipe(&mut self, f: impl IntoSwipeEventHandler);
  fn on_pan(&mut self, f: impl IntoPanEventHandler);
  fn off_pan(&mut self, f: impl IntoPanEventHandler);
  fn on_edge_pan(&mut self, f: impl IntoEdgePanEventHandler);
  fn off_edge_pan(&mut self, f: impl IntoEdgePanEventHandler);
  fn opacity(&mut self, value: f32);
  fn transform(&mut self, t: Transform2D);
  fn transition(&mut self, spec: Transition);
  fn animation(&mut self, spec: Animation);
  fn scrollbar(&mut self, style: ScrollBarStyle);
  fn scrollbar_hovered(&mut self, f: impl Fn(ScrollBarStyle) -> ScrollBarStyle + Send + Sync + 'static);
  fn culling(&mut self, enabled: bool);
  fn middle_button_autoscroll(&mut self, enabled: bool);
  fn hit_test(&mut self, behavior: HitTestBehavior);
  fn pointer_events_none(&mut self);
  fn ref_element(&mut self, element_ref: impl Into<CoreElementRef>);
  fn interactive(&mut self, state: InteractionState);
  fn focusable(&mut self, focusable: bool);
  fn tab_index(&mut self, tab_index: i32);
  fn button_kind(&mut self, kind: ButtonKind);
  #[cfg(feature = "form")]
  fn name(&mut self, name: impl Into<Arc<str>>);
  fn text_wrap(&mut self, wrap: bool);
  fn text_overflow(&mut self, overflow: TextOverflow);
  fn selectable(&mut self, selectable: bool);
  fn text_transform_mode(&mut self, mode: TextTransformMode);
  fn text_variant(&mut self, typography_style: impl Into<TypographyStyle>);
  fn text_color(&mut self, color: impl Into<TextColor>);
  fn text_shadow(&mut self, shadow: crate::layout::text_style::TextShadow);
  fn text_align(&mut self, align: impl Into<TextAlign>);
  fn on_input(&mut self, f: impl IntoTextInputEventHandler);
  fn off_input(&mut self, f: impl IntoTextInputEventHandler);
  fn placeholder(&mut self, placeholder: &str);
  fn text_input_overflow(&mut self, overflow: crate::node::node_kind::TextInputOverflow);
  fn text_input_unfocused_overflow_anchor(&mut self, anchor: crate::node::node_kind::TextInputOverflowAnchor);
  fn text_input_mask(&mut self);
  fn text_input_mask_char(&mut self, mask: char);
  fn text_input_unmask(&mut self);
  fn text_input_keyboard(&mut self, keyboard: crate::node::node_kind::KeyboardKind);
  fn text_input_return_key(&mut self, return_key: crate::node::node_kind::ReturnKey);
  fn text_input_content(&mut self, content: crate::node::node_kind::ContentKind);
  fn text_input_style(&mut self, text_style: TextStyle);
  fn text_input_placeholder_style(&mut self, text_style: TextStyle);
  fn text_input_align(&mut self, align: impl Into<TextAlign>);
  fn text_input_rows(&mut self, min_rows: usize, max_rows: usize);
  fn text_input_min_rows(&mut self, min_rows: usize);
  fn text_input_max_rows(&mut self, max_rows: usize);
  fn text_input_rows_exact(&mut self, rows: usize);
  fn range(&mut self, min: i32, max: i32);
  fn range_f32(&mut self, min: f32, max: f32);
  fn slider_step(&mut self, step: f32);
  fn slider_track_style(&mut self, style: SliderPartStyle);
  fn slider_track_hovered_style(&mut self, style: SliderPartStyle);
  fn slider_fill_style(&mut self, style: SliderPartStyle);
  fn slider_fill_hovered_style(&mut self, style: SliderPartStyle);
  fn slider_thumb_style(&mut self, style: SliderPartStyle);
  fn slider_thumb_hovered_style(&mut self, style: SliderPartStyle);
  fn slider_thumb_focused_style(&mut self, style: SliderPartStyle);
  fn checkbox_box_style(&mut self, style: CheckboxStyle);
  fn checkbox_checked_box_style(&mut self, style: CheckboxStyle);
  fn checkbox_box_hovered_style(&mut self, style: CheckboxStyle);
  fn checkbox_checked_box_hovered_style(&mut self, style: CheckboxStyle);
  fn checkbox_box_focused_style(&mut self, style: CheckboxStyle);
  fn clip(&mut self);
  fn overflow_visible(&mut self);
  fn intrinsic(&mut self, width: f32, height: f32);
  fn with_scroll_state(&mut self, existing: crate::layout::layout_kind::ScrollState);
}
#[cfg(feature = "form")]
type FormSubmitCallback = Arc<dyn Fn(FormData) + Send + Sync>;
const DEFAULT_TEXT_WRAP: bool = true;
const DEFAULT_OPACITY: f32 = 1.0;

#[cfg(feature = "form")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormData {
  fields: Vec<(String, String)>,
}

#[cfg(feature = "form")]
impl FormData {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
    self.fields.push((name.into(), value.into()));
  }

  pub fn get(&self, name: &str) -> Option<&str> {
    self
      .fields
      .iter()
      .find(|(field_name, _)| field_name == name)
      .map(|(_, value)| value.as_str())
  }

  pub fn get_all(&self, name: &str) -> Vec<&str> {
    self
      .fields
      .iter()
      .filter(|(field_name, _)| field_name == name)
      .map(|(_, value)| value.as_str())
      .collect()
  }

  pub fn entries(&self) -> &[(String, String)] {
    &self.fields
  }

  pub fn len(&self) -> usize {
    self.fields.len()
  }

  pub fn is_empty(&self) -> bool {
    self.fields.is_empty()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
  Button,
  #[cfg(feature = "form")]
  Submit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundSize {
  Stretch,
  Cover,
  Contain,
}

impl Default for BackgroundSize {
  fn default() -> Self {
    Self::Stretch
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HitTestBehavior {
  #[default]
  Auto,
  None,
  ContentOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyntheticNodeRole {
  OverlayHost,
  SelectMenu,
  /// An open dialog `Modal`'s container, a direct child of the overlay host.
  /// `WindowChrome`'s layer is built like a modal but never gets this role.
  Modal,
  /// The persistent title bar layer, which is not a popup above page content.
  WindowChromeLayer,
  /// A `Select` trigger's chevron for one open state. Painting skips it while
  /// the enclosing select's open state differs.
  SelectChevron {
    open: bool,
  },
  /// Stands for a child component's root in a component's cached output
  /// (`node/component_output.rs`); never in the live tree.
  ComponentSlot,
}

#[derive(Default, Clone)]
pub struct EventHandlers {
  pub on_click: Vec<Callback<MouseEvent>>,
  pub on_mouse_click: Vec<(MouseButton, Callback<MouseEvent>)>,
  pub on_dblclick: Vec<Callback<MouseEvent>>,
  pub on_mouse_down: Vec<Callback<MouseEvent>>,
  pub on_mouse_up: Vec<Callback<MouseEvent>>,
  pub on_mouse_move: Vec<Callback<MouseEvent>>,
  pub start_drag_buttons: MouseButtonMask,
  pub on_drag_start: Vec<Callback<DragEvent>>,
  pub on_drag_move: Vec<Callback<DragEvent>>,
  pub on_drag_end: Vec<Callback<DragEvent>>,
  pub on_drop: Vec<Callback<DropEvent>>,
  pub on_file_drag_enter: Vec<Callback<FileDragEvent>>,
  pub on_file_drag_over: Vec<Callback<FileDragEvent>>,
  pub on_file_drag_leave: Vec<VoidCallback>,
  pub on_file_drop: Vec<Callback<FileDropEvent>>,
  pub on_paste: Vec<Callback<PasteEvent>>,
  pub on_mouse_enter: Vec<VoidCallback>,
  pub on_mouse_leave: Vec<VoidCallback>,
  pub on_key_down: Vec<Callback<KeyboardEvent>>,
  pub on_key_up: Vec<Callback<KeyboardEvent>>,
  pub on_focus: Vec<VoidCallback>,
  pub on_blur: Vec<VoidCallback>,
  #[cfg(feature = "form")]
  pub on_submit: Option<FormSubmitCallback>,
  pub on_scroll: Vec<Callback<ScrollEvent>>,
  pub on_scroll_start: Vec<Callback<ScrollEvent>>,
  pub on_scroll_end: Vec<Callback<ScrollEvent>>,
  pub on_scroll_reach_top: Vec<Callback<ScrollEvent>>,
  pub on_scroll_reach_bottom: Vec<Callback<ScrollEvent>>,
  pub on_history_swipe: Vec<Callback<HistorySwipeEvent>>,
  pub on_touch_start: Vec<Callback<TouchEvent>>,
  pub on_touch_move: Vec<Callback<TouchEvent>>,
  pub on_touch_end: Vec<Callback<TouchEvent>>,
  pub on_touch_cancel: Vec<Callback<TouchEvent>>,
  pub on_long_press: Vec<Callback<LongPressEvent>>,
  pub on_pinch: Vec<Callback<PinchEvent>>,
  pub on_rotate: Vec<Callback<RotateEvent>>,
  pub on_tap: Vec<Callback<TapEvent>>,
  pub on_swipe: Vec<Callback<SwipeEvent>>,
  pub on_pan: Vec<Callback<PanEvent>>,
  pub on_edge_pan: Vec<Callback<EdgePanEvent>>,
}

/// The styles few nodes set, behind one guard: a guard holds a watch handle
/// that layout tracking takes and dropping a node gives back, so a guard per
/// field cost every node of a rebuilt tree those atomics, and these fields
/// cost it 176 bytes. Only whether any style changed is ever asked, so one
/// flag says what theirs said.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct RareStyle {
  pub(crate) gradient: Option<Gradient>,
  pub(crate) box_shadow: Option<BoxShadowValue>,
  pub(crate) caret_color: Option<TextColor>,
  pub(crate) selection_color: Option<TextColor>,
  pub(crate) caret_mode: Option<CaretMode>,
}

pub(crate) type AnimationOverrides = Vec<(crate::animation::AnimatableProperty, crate::animation::AnimatableValue)>;

empty_default!(EventHandlers);
empty_default!(StateStyles);
empty_default!(RareStyle);
empty_default!(AnimationOverrides);
empty_default!(Vec<Transition>);

pub(crate) struct Node {
  pub(crate) node_id: NodeId,
  pub(crate) tag_name: Arc<str>,
  pub(crate) component_slot_id: Option<u64>,
  pub(crate) component_key: Option<Arc<str>>,
  /// Author-supplied HTML-like `id` attribute for lookup and retained identity.
  pub(crate) element_id: Option<Arc<str>>,
  /// Author-supplied HTML-like class list. Used for lookup, not retained identity.
  pub(crate) classes: Vec<Arc<str>>,
  /// On a component's root: what the code that mounted it put on it, which
  /// the component's own re-render gets back. Boxed: few nodes have one.
  pub(crate) outer_decoration: Option<Box<crate::node::outer_decoration::OuterDecoration>>,
  pub(crate) overlay_declaration: Option<Box<crate::app::ctx::OverlaySpec>>,
  /// The anchor of a signal-driven overlay declared here that closes on an
  /// outside press and switches on an anchor press, open or closed: pressing
  /// it opens a dropdown, so another popup's closing press on it is delivered.
  pub(crate) dropdown_anchor: Option<CoreElementRef>,
  pub(crate) modal_declaration: Option<Box<crate::app::ctx::ModalSpec>>,
  pub(crate) layout_neutral: bool,
  pub(crate) synthetic_role: Option<SyntheticNodeRole>,
  #[cfg(feature = "devtools")]
  pub(crate) component_props_debug: Option<DevtoolsInspectableDebug>,
  #[cfg(feature = "devtools")]
  pub(crate) component_signals_debug: Vec<ComponentSignalDebug>,
  #[cfg(feature = "devtools")]
  pub(crate) component_memos_debug: Vec<ComponentMemoDebug>,
  #[cfg(feature = "devtools")]
  pub(crate) component_effects_debug: Vec<ComponentEffectDebug>,
  #[cfg(feature = "devtools")]
  pub(crate) component_contexts_debug: Vec<ComponentContextDebug>,
  #[cfg(any(feature = "devtools", feature = "mcp"))]
  pub(crate) debug_attrs: Vec<(Arc<str>, Arc<str>)>,
  pub(crate) layout_kind: LayoutKind,
  pub(crate) frame: FrameConstraints,
  pub(crate) padding: Padding,
  pub(crate) position: Position,
  pub(crate) offset: Option<Offset>,
  pub(crate) align_self: Option<Alignment>,
  pub(crate) flex: Option<FlexParams>,
  pub(crate) shrink_rule: ShrinkRule,
  pub(crate) node_kind: NodeKind,
  pub(crate) text_content: Guard<Option<String>>,
  pub(crate) text_wrap: bool,
  pub(crate) text_overflow: TextOverflow,
  pub(crate) overflow: Overflow,
  pub(crate) intrinsic_size: Option<Size>,
  pub(crate) color: Guard<Option<BackgroundColor>>,
  pub(crate) border_radius: Guard<Option<ThemedBorderRadius>>,
  /// Boxed to keep tracking metadata within the node's stack size budget.
  pub(crate) border: Guard<Option<Box<Borders>>>,
  pub(crate) rare_style: Guard<LazyBox<RareStyle>>,
  pub(crate) cursor: Option<CursorIcon>,
  pub(crate) hit_test: HitTestBehavior,
  #[cfg(feature = "raster")]
  /// Boxed: few nodes have one, and `Node` size is budgeted (lurq#25).
  pub(crate) background_image: Guard<Option<Box<crate::images::ImageData>>>,
  #[cfg(feature = "raster")]
  pub(crate) background_size: BackgroundSize,
  #[cfg(all(feature = "image", feature = "resources"))]
  pub(crate) background_resource_image: Option<Arc<str>>,
  /// Boxed: most nodes do not scroll, and `Node` size is budgeted (lurq#25).
  pub(crate) scrollbar_style: Guard<Option<Box<ScrollBarStyle>>>,
  pub(crate) scrollbar_hovered_style: Option<ScrollbarStyleCallback>,
  pub(crate) element_ref: Option<CoreElementRef>,
  pub(crate) drag_payload: Option<crate::app::events::DragPayload>,
  pub(crate) interaction: Option<InteractionState>,
  /// `None` follows the node kind (buttons and form controls are focusable)
  /// and `tab_index`; `Some` overrides both.
  pub(crate) focusable: Option<bool>,
  pub(crate) tab_index: Option<i32>,
  pub(crate) button_kind: Option<ButtonKind>,
  #[cfg(feature = "form")]
  pub(crate) form_name: Option<Arc<str>>,
  pub(crate) style_state: InteractionState,
  pub(crate) state_styles: LazyBox<StateStyles>,
  pub(crate) opacity: f32,
  pub(crate) transform: Transform2D,
  /// Boxed: few nodes animate, and `Node` size is budgeted (lurq#25).
  pub(crate) animation_overrides: LazyBox<AnimationOverrides>,
  /// Boxed: few nodes animate, and `Node` size is budgeted (lurq#25).
  pub(crate) transitions: LazyBox<Vec<Transition>>,
  /// Boxed: few nodes animate, and `Node` size is budgeted (lurq#25).
  pub(crate) animation: Option<Box<Animation>>,
  pub(crate) layout_cache: crate::node::layout_cache::LayoutCache,
  pub(crate) children: Vec<Node>,
  pub(crate) events: LazyBox<EventHandlers>,
}

impl Default for Node {
  fn default() -> Self {
    Self::new()
  }
}

impl NodeUpdate for Node {
  fn describe(&mut self, name: Arc<str>, value: Arc<str>) {
    #[cfg(any(feature = "devtools", feature = "mcp"))]
    self.debug_attrs.push((name, value));
    #[cfg(not(any(feature = "devtools", feature = "mcp")))]
    let _ = (name, value);
  }

  fn child(&mut self, child: Node) {
    self.children.push(child);
    self.layout_cache.invalidate();
  }

  fn id(&mut self, id: impl Into<Arc<str>>) {
    self.element_id = Some(id.into());
  }

  fn class(&mut self, class: impl Into<Arc<str>>) {
    self.push_class(class.into());
  }

  fn classes(&mut self, classes: impl IntoIterator<Item: Into<Arc<str>>>) {
    for class in classes {
      self.push_class(class.into());
    }
  }

  fn spacing(&mut self, spacing: impl Into<SpacingValue>) {
    let spacing = spacing.into();
    match &mut self.layout_kind {
      LayoutKind::Row { spacing: s, .. } | LayoutKind::Column { spacing: s, .. } => *s = spacing,
      _ => {}
    }
  }

  fn align_items(&mut self, align: Alignment) {
    match &mut self.layout_kind {
      LayoutKind::Row { align: a, .. } | LayoutKind::Column { align: a, .. } => *a = align,
      _ => {}
    }
  }

  fn justify(&mut self, justify: crate::layout::layout_kind::Justify) {
    match &mut self.layout_kind {
      LayoutKind::Row { justify: j, .. } | LayoutKind::Column { justify: j, .. } => *j = justify,
      _ => {}
    }
  }

  fn wrap(&mut self) {
    match &mut self.layout_kind {
      LayoutKind::Row { wrap, .. } | LayoutKind::Column { wrap, .. } => {
        *wrap = crate::layout::layout_kind::FlexWrap::Wrap
      }
      _ => {}
    }
  }

  fn stack_align(&mut self, align: StackAlignment) {
    if let LayoutKind::Stack { align: a } = &mut self.layout_kind {
      *a = align;
    }
  }

  fn size(&mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        width: Some(width.into()),
        height: Some(height.into()),
        ..Default::default()
      },
    );
  }

  fn width(&mut self, width: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        width: Some(width.into()),
        ..Default::default()
      },
    );
  }

  fn height(&mut self, height: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        height: Some(height.into()),
        ..Default::default()
      },
    );
  }

  fn min_width(&mut self, width: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        min_width: Some(width.into()),
        ..Default::default()
      },
    );
  }

  fn max_width(&mut self, width: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        max_width: Some(width.into()),
        ..Default::default()
      },
    );
  }

  fn min_height(&mut self, height: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        min_height: Some(height.into()),
        ..Default::default()
      },
    );
  }

  fn max_height(&mut self, height: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        max_height: Some(height.into()),
        ..Default::default()
      },
    );
  }

  fn min_size(&mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        min_width: Some(width.into()),
        min_height: Some(height.into()),
        ..Default::default()
      },
    );
  }

  fn max_size(&mut self, width: impl Into<Dimension>, height: impl Into<Dimension>) {
    NodeUpdate::frame(
      self,
      FrameConstraints {
        max_width: Some(width.into()),
        max_height: Some(height.into()),
        ..Default::default()
      },
    );
  }

  fn padding_left(&mut self, val: impl Into<SpacingValue>) {
    NodeUpdate::padding(self, Padding::new().left(val.into()));
  }

  fn padding_right(&mut self, val: impl Into<SpacingValue>) {
    NodeUpdate::padding(self, Padding::new().right(val.into()));
  }

  fn padding_top(&mut self, val: impl Into<SpacingValue>) {
    NodeUpdate::padding(self, Padding::new().top(val.into()));
  }

  fn padding_bottom(&mut self, val: impl Into<SpacingValue>) {
    NodeUpdate::padding(self, Padding::new().bottom(val.into()));
  }

  fn padding_horizontal(&mut self, val: impl Into<SpacingValue>) {
    NodeUpdate::padding(self, Padding::horizontal(val.into()));
  }

  fn padding_vertical(&mut self, val: impl Into<SpacingValue>) {
    NodeUpdate::padding(self, Padding::vertical(val.into()));
  }

  fn padding(&mut self, padding: impl Into<Padding>) {
    let padding = padding.into();
    self.padding.merge_from(&padding);
    self.layout_cache.invalidate();
  }

  fn padding_custom(&mut self, padding: Padding) {
    NodeUpdate::padding(self, padding);
  }

  fn frame(&mut self, frame: FrameConstraints) {
    self.frame = merge_frame(self.frame, frame);
    self.layout_cache.invalidate();
  }

  fn offset(&mut self, x: f32, y: f32) {
    self.offset = Some(Offset::new(x, y));
    self.layout_cache.invalidate();
  }

  fn relative(&mut self, x: f32, y: f32) {
    NodeUpdate::offset(self, x, y);
  }

  fn absolute(&mut self, x: f32, y: f32, width: impl Into<Dimension>, height: impl Into<Dimension>) {
    self.position = Position::Absolute {
      x,
      y,
      width: Some(width.into()),
      height: Some(height.into()),
    };
    self.layout_cache.invalidate();
  }

  fn absolute_position(&mut self, x: f32, y: f32) {
    self.position = Position::Absolute {
      x,
      y,
      width: None,
      height: None,
    };
    self.layout_cache.invalidate();
  }

  fn align(&mut self, alignment: Alignment) {
    self.align_self = Some(alignment);
    self.layout_cache.invalidate();
  }

  fn flex(&mut self, factor: f32) {
    self.flex = Some(FlexParams::grow(factor));
    self.layout_cache.invalidate();
  }

  fn flex_shrink(&mut self, factor: f32) {
    self.flex = Some(FlexParams {
      grow: 0.0,
      shrink: factor,
      basis: None,
    });
    self.layout_cache.invalidate();
  }

  fn flex_full(&mut self, grow: f32, shrink: f32, basis: Option<f32>) {
    self.flex = Some(FlexParams { grow, shrink, basis });
    self.layout_cache.invalidate();
  }

  fn shrink_order(&mut self, order: i32) {
    self.shrink_rule.order = order;
    self.layout_cache.invalidate();
  }

  fn shrink_limit(&mut self, limit: ShrinkLimit) {
    self.shrink_rule.limit = limit;
    self.layout_cache.invalidate();
  }

  fn shrink_drop_below(&mut self, size: f32) {
    // NaN would never compare equal and invalidate the layout cache on every
    // rebuild; a negative size means the same as 0.
    let size = if size.is_nan() { 0.0 } else { size.max(0.0) };
    self.shrink_rule.drop_below = Some(size);
    self.layout_cache.invalidate();
  }

  fn background(&mut self, color: impl Into<BackgroundColor>) {
    self.color.set(Some(color.into()));
  }

  fn background_gradient(&mut self, gradient: impl Into<Gradient>) {
    self.rare_style.gradient = Some(gradient.into());
  }

  fn caret_color(&mut self, color: impl Into<TextColor>) {
    self.set_caret_color(color.into());
  }

  fn selection_color(&mut self, color: impl Into<TextColor>) {
    self.set_selection_color(color.into());
  }

  fn text_input_caret_mode(&mut self, mode: CaretMode) {
    if matches!(self.node_kind, NodeKind::TextInput { .. }) {
      self.rare_style.caret_mode = Some(mode);
    }
  }

  fn corner_radius(&mut self, radius: impl Into<RadiusValue>) {
    self.border_radius.set(Some(ThemedBorderRadius::all(radius)));
  }

  fn corner_radius_custom(&mut self, radius: BorderRadius) {
    self.border_radius.set(Some(radius.into()));
  }

  fn corner_radius_top_left(&mut self, radius: impl Into<RadiusValue>) {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.top_left = radius.into();
    self.border_radius.set(Some(border_radius));
  }

  fn corner_radius_top_right(&mut self, radius: impl Into<RadiusValue>) {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.top_right = radius.into();
    self.border_radius.set(Some(border_radius));
  }

  fn corner_radius_bottom_right(&mut self, radius: impl Into<RadiusValue>) {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.bottom_right = radius.into();
    self.border_radius.set(Some(border_radius));
  }

  fn corner_radius_bottom_left(&mut self, radius: impl Into<RadiusValue>) {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.bottom_left = radius.into();
    self.border_radius.set(Some(border_radius));
  }

  fn rounded(&mut self, radius: impl Into<RadiusValue>) {
    NodeUpdate::corner_radius(self, radius);
  }

  fn border_inside(&mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) {
    self
      .border
      .set(Some(Box::new(Borders::all(Border::inside(width, color)))));
  }

  fn border_outside(&mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) {
    self
      .border
      .set(Some(Box::new(Borders::all(Border::outside(width, color)))));
  }

  fn border_center(&mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) {
    self
      .border
      .set(Some(Box::new(Borders::all(Border::center(width, color)))));
  }

  fn border(&mut self, border: Border) {
    self.border.set(Some(Box::new(Borders::all(border))));
  }

  fn border_custom(&mut self, border: Borders) {
    self.border.set(Some(Box::new(border)));
  }

  fn border_top(&mut self, border: Border) {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.top = Some(border);
    self.border.set(Some(Box::new(borders)));
  }

  fn border_right(&mut self, border: Border) {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.right = Some(border);
    self.border.set(Some(Box::new(borders)));
  }

  fn border_bottom(&mut self, border: Border) {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.bottom = Some(border);
    self.border.set(Some(Box::new(borders)));
  }

  fn border_left(&mut self, border: Border) {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.left = Some(border);
    self.border.set(Some(Box::new(borders)));
  }

  fn box_shadow(&mut self, shadow: impl Into<BoxShadowValue>) {
    self.rare_style.box_shadow = Some(shadow.into());
  }

  fn cursor(&mut self, cursor: CursorIcon) {
    self.cursor = Some(cursor);
  }

  #[cfg(feature = "raster")]
  fn background_image(&mut self, data: impl Into<crate::images::ImageKind>) {
    match data.into() {
      crate::images::ImageKind::Bytes(data) => self.background_image.set(Some(Box::new(data))),
      crate::images::ImageKind::Native(data) => self.background_image.set(Some(Box::new(data.image_data()))),
      #[cfg(all(feature = "image", feature = "resources"))]
      crate::images::ImageKind::Resource(path) => self.background_resource_image = Some(path),
    }
  }

  #[cfg(feature = "raster")]
  fn background_size(&mut self, size: BackgroundSize) {
    self.background_size = size;
  }

  #[cfg(feature = "raster")]
  fn background_cover(&mut self) {
    self.background_size = BackgroundSize::Cover;
  }

  #[cfg(feature = "raster")]
  fn background_contain(&mut self) {
    self.background_size = BackgroundSize::Contain;
  }

  fn hovered_style(&mut self, style: Style) {
    self.state_styles.hovered = Some(style);
    self.state_styles.invalidate_layout_style();
  }

  fn active_style(&mut self, style: Style) {
    self.state_styles.active = Some(style);
    self.state_styles.invalidate_layout_style();
  }

  fn focused_style(&mut self, style: Style) {
    self.state_styles.focused = Some(style);
    self.state_styles.invalidate_layout_style();
  }

  fn focus_visible_style(&mut self, style: Style) {
    self.state_styles.focus_visible = Some(style);
    self.state_styles.invalidate_layout_style();
  }

  fn drag_over_style(&mut self, style: Style) {
    self.state_styles.drag_over = Some(style);
    self.state_styles.invalidate_layout_style();
  }

  fn hovered(&mut self, f: impl FnOnce(Style) -> Style) {
    NodeUpdate::hovered_style(self, f(Style::new()));
  }

  fn drag_over(&mut self, f: impl FnOnce(Style) -> Style) {
    NodeUpdate::drag_over_style(self, f(Style::new()));
  }

  fn active(&mut self, f: impl FnOnce(Style) -> Style) {
    NodeUpdate::active_style(self, f(Style::new()));
  }

  fn focused(&mut self, f: impl FnOnce(Style) -> Style) {
    NodeUpdate::focused_style(self, f(Style::new()));
  }

  fn focus_visible(&mut self, f: impl FnOnce(Style) -> Style) {
    NodeUpdate::focus_visible_style(self, f(Style::new()));
  }

  fn on_click(&mut self, f: impl IntoMouseEventHandler) {
    self.events.on_click.push(f.into_event_handler());
  }

  fn off_click(&mut self, f: impl IntoMouseEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_click.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_mouse_click(&mut self, button: MouseButton, f: impl IntoMouseEventHandler) {
    self.events.on_mouse_click.push((button, f.into_event_handler()));
  }

  fn off_mouse_click(&mut self, button: MouseButton, f: impl IntoMouseEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_click
      .retain(|(handler_button, existing)| *handler_button != button || !existing.same_handler(&handler));
  }

  fn on_dblclick(&mut self, f: impl IntoMouseEventHandler) {
    self.events.on_dblclick.push(f.into_event_handler());
  }

  fn off_dblclick(&mut self, f: impl IntoMouseEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_dblclick
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_mouse_down(&mut self, f: impl IntoMouseEventHandler) {
    self.events.on_mouse_down.push(f.into_event_handler());
  }

  fn off_mouse_down(&mut self, f: impl IntoMouseEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_down
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_mouse_up(&mut self, f: impl IntoMouseEventHandler) {
    self.events.on_mouse_up.push(f.into_event_handler());
  }

  fn off_mouse_up(&mut self, f: impl IntoMouseEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_up
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_mouse_move(&mut self, f: impl IntoMouseEventHandler) {
    self.events.on_mouse_move.push(f.into_event_handler());
  }

  fn off_mouse_move(&mut self, f: impl IntoMouseEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_move
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn start_drag_buttons(&mut self, buttons: MouseButtonMask) {
    self.events.start_drag_buttons = buttons;
  }

  fn on_drag_start(&mut self, f: impl IntoDragEventHandler) {
    self.events.on_drag_start.push(f.into_event_handler());
  }

  fn off_drag_start(&mut self, f: impl IntoDragEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_drag_start
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_drag_move(&mut self, f: impl IntoDragEventHandler) {
    self.events.on_drag_move.push(f.into_event_handler());
  }

  fn off_drag_move(&mut self, f: impl IntoDragEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_drag_move
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_drag_end(&mut self, f: impl IntoDragEventHandler) {
    self.events.on_drag_end.push(f.into_event_handler());
  }

  fn off_drag_end(&mut self, f: impl IntoDragEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_drag_end
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_drop(&mut self, f: impl IntoDropEventHandler) {
    self.events.on_drop.push(f.into_event_handler());
  }

  fn off_drop(&mut self, f: impl IntoDropEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_drop.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_file_drag_enter(&mut self, f: impl IntoFileDragEventHandler) {
    self.events.on_file_drag_enter.push(f.into_event_handler());
  }

  fn off_file_drag_enter(&mut self, f: impl IntoFileDragEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_file_drag_enter
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_file_drag_over(&mut self, f: impl IntoFileDragEventHandler) {
    self.events.on_file_drag_over.push(f.into_event_handler());
  }

  fn off_file_drag_over(&mut self, f: impl IntoFileDragEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_file_drag_over
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_file_drag_leave(&mut self, f: impl IntoVoidEventHandler) {
    self.events.on_file_drag_leave.push(f.into_void_event_handler());
  }

  fn off_file_drag_leave(&mut self, f: impl IntoVoidEventHandler) {
    let handler = f.into_void_event_handler();
    self
      .events
      .on_file_drag_leave
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_file_drop(&mut self, f: impl IntoFileDropEventHandler) {
    self.events.on_file_drop.push(f.into_event_handler());
  }

  fn off_file_drop(&mut self, f: impl IntoFileDropEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_file_drop
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_paste(&mut self, f: impl IntoPasteEventHandler) {
    self.events.on_paste.push(f.into_event_handler());
  }

  fn off_paste(&mut self, f: impl IntoPasteEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_paste.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_mouse_enter(&mut self, f: impl IntoVoidEventHandler) {
    self.events.on_mouse_enter.push(f.into_void_event_handler());
  }

  fn off_mouse_enter(&mut self, f: impl IntoVoidEventHandler) {
    let handler = f.into_void_event_handler();
    self
      .events
      .on_mouse_enter
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_mouse_leave(&mut self, f: impl IntoVoidEventHandler) {
    self.events.on_mouse_leave.push(f.into_void_event_handler());
  }

  fn off_mouse_leave(&mut self, f: impl IntoVoidEventHandler) {
    let handler = f.into_void_event_handler();
    self
      .events
      .on_mouse_leave
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_key_down(&mut self, f: impl IntoKeyboardEventHandler) {
    self.events.on_key_down.push(f.into_event_handler());
  }

  fn off_key_down(&mut self, f: impl IntoKeyboardEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_key_down
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_key_up(&mut self, f: impl IntoKeyboardEventHandler) {
    self.events.on_key_up.push(f.into_event_handler());
  }

  fn off_key_up(&mut self, f: impl IntoKeyboardEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_key_up
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_focus(&mut self, f: impl IntoVoidEventHandler) {
    self.events.on_focus.push(f.into_void_event_handler());
  }

  fn off_focus(&mut self, f: impl IntoVoidEventHandler) {
    let handler = f.into_void_event_handler();
    self.events.on_focus.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_blur(&mut self, f: impl IntoVoidEventHandler) {
    self.events.on_blur.push(f.into_void_event_handler());
  }

  fn off_blur(&mut self, f: impl IntoVoidEventHandler) {
    let handler = f.into_void_event_handler();
    self.events.on_blur.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_scroll(&mut self, f: impl IntoScrollEventHandler) {
    self.events.on_scroll.push(f.into_event_handler());
  }

  fn off_scroll(&mut self, f: impl IntoScrollEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_scroll_start(&mut self, f: impl IntoScrollEventHandler) {
    self.events.on_scroll_start.push(f.into_event_handler());
  }

  fn off_scroll_start(&mut self, f: impl IntoScrollEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll_start
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_scroll_end(&mut self, f: impl IntoScrollEventHandler) {
    self.events.on_scroll_end.push(f.into_event_handler());
  }

  fn off_scroll_end(&mut self, f: impl IntoScrollEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll_end
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_scroll_reach_top(&mut self, f: impl IntoScrollEventHandler) {
    self.events.on_scroll_reach_top.push(f.into_event_handler());
  }

  fn off_scroll_reach_top(&mut self, f: impl IntoScrollEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll_reach_top
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_scroll_reach_bottom(&mut self, f: impl IntoScrollEventHandler) {
    self.events.on_scroll_reach_bottom.push(f.into_event_handler());
  }

  fn off_scroll_reach_bottom(&mut self, f: impl IntoScrollEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll_reach_bottom
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_history_swipe(&mut self, f: impl IntoHistorySwipeEventHandler) {
    self.events.on_history_swipe.push(f.into_event_handler());
  }

  fn off_history_swipe(&mut self, f: impl IntoHistorySwipeEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_history_swipe
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_touch_start(&mut self, f: impl IntoTouchEventHandler) {
    self.events.on_touch_start.push(f.into_event_handler());
  }

  fn off_touch_start(&mut self, f: impl IntoTouchEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_touch_start
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_touch_move(&mut self, f: impl IntoTouchEventHandler) {
    self.events.on_touch_move.push(f.into_event_handler());
  }

  fn off_touch_move(&mut self, f: impl IntoTouchEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_touch_move
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_touch_end(&mut self, f: impl IntoTouchEventHandler) {
    self.events.on_touch_end.push(f.into_event_handler());
  }

  fn off_touch_end(&mut self, f: impl IntoTouchEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_touch_end
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_touch_cancel(&mut self, f: impl IntoTouchEventHandler) {
    self.events.on_touch_cancel.push(f.into_event_handler());
  }

  fn off_touch_cancel(&mut self, f: impl IntoTouchEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_touch_cancel
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_long_press(&mut self, f: impl IntoLongPressEventHandler) {
    self.events.on_long_press.push(f.into_event_handler());
  }

  fn off_long_press(&mut self, f: impl IntoLongPressEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_long_press
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_pinch(&mut self, f: impl IntoPinchEventHandler) {
    self.events.on_pinch.push(f.into_event_handler());
  }

  fn off_pinch(&mut self, f: impl IntoPinchEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_pinch.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_rotate(&mut self, f: impl IntoRotateEventHandler) {
    self.events.on_rotate.push(f.into_event_handler());
  }

  fn off_rotate(&mut self, f: impl IntoRotateEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_rotate
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn on_tap(&mut self, f: impl IntoTapEventHandler) {
    self.events.on_tap.push(f.into_event_handler());
  }

  fn off_tap(&mut self, f: impl IntoTapEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_tap.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_swipe(&mut self, f: impl IntoSwipeEventHandler) {
    self.events.on_swipe.push(f.into_event_handler());
  }

  fn off_swipe(&mut self, f: impl IntoSwipeEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_swipe.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_pan(&mut self, f: impl IntoPanEventHandler) {
    self.events.on_pan.push(f.into_event_handler());
  }

  fn off_pan(&mut self, f: impl IntoPanEventHandler) {
    let handler = f.into_event_handler();
    self.events.on_pan.retain(|existing| !existing.same_handler(&handler));
  }

  fn on_edge_pan(&mut self, f: impl IntoEdgePanEventHandler) {
    self.events.on_edge_pan.push(f.into_event_handler());
  }

  fn off_edge_pan(&mut self, f: impl IntoEdgePanEventHandler) {
    let handler = f.into_event_handler();
    self
      .events
      .on_edge_pan
      .retain(|existing| !existing.same_handler(&handler));
  }

  fn opacity(&mut self, value: f32) {
    self.set_opacity(value);
  }

  fn transform(&mut self, t: Transform2D) {
    self.transform = t;
  }

  fn transition(&mut self, spec: Transition) {
    self.push_transition(spec);
  }

  fn animation(&mut self, spec: Animation) {
    self.set_animation(spec);
  }

  fn scrollbar(&mut self, style: ScrollBarStyle) {
    self.scrollbar_style.set(Some(Box::new(style)));
  }

  fn scrollbar_hovered(&mut self, f: impl Fn(ScrollBarStyle) -> ScrollBarStyle + Send + Sync + 'static) {
    self.scrollbar_hovered_style = Some(Arc::new(f));
  }

  fn culling(&mut self, enabled: bool) {
    if let LayoutKind::ScrollModifier { culling, .. } = &mut self.layout_kind {
      *culling = enabled;
    }
  }

  fn middle_button_autoscroll(&mut self, enabled: bool) {
    if let LayoutKind::ScrollModifier {
      middle_button_autoscroll,
      ..
    } = &mut self.layout_kind
    {
      *middle_button_autoscroll = enabled;
    }
  }

  fn hit_test(&mut self, behavior: HitTestBehavior) {
    self.hit_test = behavior;
  }

  fn pointer_events_none(&mut self) {
    self.hit_test = HitTestBehavior::None;
  }

  fn ref_element(&mut self, element_ref: impl Into<CoreElementRef>) {
    self.element_ref = Some(element_ref.into());
    self.layout_cache.mark_watch();
  }

  fn interactive(&mut self, state: InteractionState) {
    self.interaction = Some(state);
  }

  fn focusable(&mut self, focusable: bool) {
    self.focusable = Some(focusable);
  }

  fn tab_index(&mut self, tab_index: i32) {
    self.tab_index = Some(tab_index);
  }

  fn button_kind(&mut self, kind: ButtonKind) {
    self.button_kind = Some(kind);
  }

  #[cfg(feature = "form")]
  fn name(&mut self, name: impl Into<Arc<str>>) {
    self.form_name = Some(name.into());
  }

  fn text_wrap(&mut self, wrap: bool) {
    self.text_wrap = wrap;
    self.layout_cache.invalidate();
  }

  fn text_overflow(&mut self, overflow: TextOverflow) {
    self.text_overflow = overflow;
    self.layout_cache.invalidate();
  }

  fn selectable(&mut self, selectable: bool) {
    match &self.node_kind {
      NodeKind::Text { state, .. } => state.set_selectable(selectable),
      #[cfg(feature = "markdown")]
      NodeKind::RichText { state, .. } => state.set_selectable(selectable),
      _ => {}
    }
  }

  fn text_transform_mode(&mut self, mode: TextTransformMode) {
    if let NodeKind::Text { transform_mode, .. } = &mut self.node_kind {
      *transform_mode = mode;
    }
  }

  fn text_variant(&mut self, typography_style: impl Into<TypographyStyle>) {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_variant(typography_style);
      self.layout_cache.invalidate();
    }
  }

  fn text_color(&mut self, color: impl Into<TextColor>) {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_color(color);
    }
  }

  fn text_shadow(&mut self, shadow: crate::layout::text_style::TextShadow) {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_shadow(shadow);
    }
  }

  fn text_align(&mut self, align: impl Into<TextAlign>) {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_text_align(align);
      self.layout_cache.invalidate();
    }
  }

  fn on_input(&mut self, f: impl IntoTextInputEventHandler) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_on_input(f);
    }
  }

  fn off_input(&mut self, f: impl IntoTextInputEventHandler) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.clear_on_input(f);
    }
  }

  fn placeholder(&mut self, placeholder: &str) {
    self.set_placeholder(placeholder);
  }

  fn text_input_overflow(&mut self, overflow: crate::node::node_kind::TextInputOverflow) {
    self.set_text_input_overflow(overflow);
  }

  fn text_input_unfocused_overflow_anchor(&mut self, anchor: crate::node::node_kind::TextInputOverflowAnchor) {
    self.set_text_input_unfocused_overflow_anchor(anchor);
  }

  fn text_input_mask(&mut self) {
    NodeUpdate::text_input_mask_char(self, '\u{2022}');
  }

  fn text_input_keyboard(&mut self, keyboard: crate::node::node_kind::KeyboardKind) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_keyboard(keyboard);
    }
  }

  fn text_input_return_key(&mut self, return_key: crate::node::node_kind::ReturnKey) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_return_key(return_key);
    }
  }

  fn text_input_content(&mut self, content: crate::node::node_kind::ContentKind) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_content(content);
    }
  }

  fn text_input_mask_char(&mut self, mask: char) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_mask(Some(mask));
    }
  }

  fn text_input_unmask(&mut self) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_mask(None);
    }
  }

  fn text_input_style(&mut self, text_style: TextStyle) {
    self.set_text_input_style(text_style);
  }

  fn text_input_placeholder_style(&mut self, text_style: TextStyle) {
    self.set_text_input_placeholder_style(text_style);
  }

  fn text_input_align(&mut self, align: impl Into<TextAlign>) {
    self.set_text_input_align(align);
  }

  fn text_input_rows(&mut self, min_rows: usize, max_rows: usize) {
    self.set_text_input_rows(min_rows, max_rows);
  }

  fn text_input_min_rows(&mut self, min_rows: usize) {
    self.set_text_input_min_rows(min_rows);
  }

  fn text_input_max_rows(&mut self, max_rows: usize) {
    self.set_text_input_max_rows(max_rows);
  }

  fn text_input_rows_exact(&mut self, rows: usize) {
    self.set_text_input_rows_exact(rows);
  }

  fn range(&mut self, min: i32, max: i32) {
    if let Some(state) = self.slider_state() {
      state.set_range(min, max);
    }
  }

  fn range_f32(&mut self, min: f32, max: f32) {
    if let Some(state) = self.slider_state() {
      state.set_range_f32(min, max);
    }
  }

  fn slider_step(&mut self, step: f32) {
    if let Some(state) = self.slider_state() {
      state.set_step(step);
    }
  }

  fn slider_track_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_track_style(style);
    }
  }

  fn slider_track_hovered_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_track_hovered_style(style);
    }
  }

  fn slider_fill_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_fill_style(style);
    }
  }

  fn slider_fill_hovered_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_fill_hovered_style(style);
    }
  }

  fn slider_thumb_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_thumb_style(style);
    }
  }

  fn slider_thumb_hovered_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_thumb_hovered_style(style);
    }
  }

  fn slider_thumb_focused_style(&mut self, style: SliderPartStyle) {
    if let Some(state) = self.slider_state() {
      state.set_thumb_focused_style(style);
    }
  }

  fn checkbox_box_style(&mut self, style: CheckboxStyle) {
    if let Some(state) = self.checkbox_state() {
      state.set_style(style);
    }
  }

  fn checkbox_checked_box_style(&mut self, style: CheckboxStyle) {
    if let Some(state) = self.checkbox_state() {
      state.set_checked_style(style);
    }
  }

  fn checkbox_box_hovered_style(&mut self, style: CheckboxStyle) {
    if let Some(state) = self.checkbox_state() {
      state.set_hovered_style(style);
    }
  }

  fn checkbox_checked_box_hovered_style(&mut self, style: CheckboxStyle) {
    if let Some(state) = self.checkbox_state() {
      state.set_checked_hovered_style(style);
    }
  }

  fn checkbox_box_focused_style(&mut self, style: CheckboxStyle) {
    if let Some(state) = self.checkbox_state() {
      state.set_focused_style(style);
    }
  }

  fn clip(&mut self) {
    self.set_overflow_through_logical(Overflow::Hidden);
  }

  fn overflow_visible(&mut self) {
    self.set_overflow_through_logical(Overflow::Visible);
  }

  fn intrinsic(&mut self, width: f32, height: f32) {
    self.intrinsic_size = Some(Size::new(width, height));
  }

  fn with_scroll_state(&mut self, existing: crate::layout::layout_kind::ScrollState) {
    if let LayoutKind::ScrollModifier { state, .. } = &mut self.layout_kind {
      *state = existing;
    }
  }
}

#[allow(dead_code)]
impl Node {
  fn from_parts(layout_kind: LayoutKind, node_kind: NodeKind, children: Vec<Node>) -> Self {
    Self {
      layout_kind,
      frame: FrameConstraints::default(),
      padding: Padding::default(),
      position: Position::Static,
      offset: None,
      align_self: None,
      flex: None,
      shrink_rule: ShrinkRule::default(),
      node_kind,
      node_id: NodeId::UNASSIGNED,
      tag_name: {
        static TAG_NAME: std::sync::OnceLock<Arc<str>> = std::sync::OnceLock::new();
        TAG_NAME.get_or_init(|| Arc::from("Node")).clone()
      },
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
      text_content: Guard::new(None),
      text_wrap: DEFAULT_TEXT_WRAP,
      text_overflow: TextOverflow::default(),
      overflow: Overflow::Hidden,
      intrinsic_size: None,
      color: Guard::new(None),
      border_radius: Guard::new(None),
      border: Guard::new(None),
      rare_style: Guard::new(LazyBox::new()),
      cursor: None,
      hit_test: HitTestBehavior::default(),
      #[cfg(feature = "raster")]
      background_image: Guard::new(None),
      #[cfg(feature = "raster")]
      background_size: BackgroundSize::default(),
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
      style_state: InteractionState::new(),
      state_styles: LazyBox::new(),
      opacity: DEFAULT_OPACITY,
      transform: Transform2D::IDENTITY,
      animation_overrides: LazyBox::new(),
      transitions: LazyBox::new(),
      animation: None,
      layout_cache: Default::default(),
      children,
      events: LazyBox::new(),
    }
  }

  fn with_text_content(mut self, content: &str) -> Self {
    self.text_content.set(Some(content.to_owned()));
    self
  }

  pub fn text_wrap(mut self, wrap: bool) -> Self {
    self.text_wrap = wrap;
    self.layout_cache.invalidate();
    self
  }

  pub fn text_overflow(mut self, overflow: TextOverflow) -> Self {
    self.text_overflow = overflow;
    self.layout_cache.invalidate();
    self
  }

  pub fn key(mut self, key: impl Into<Arc<str>>) -> Self {
    self.component_key = Some(key.into());
    self
  }

  /// HTML-like `id` attribute for [`Tree::get_element_by_id`] lookup.
  /// Also participates in retained-node identity across sibling changes.
  /// Keep IDs stable; changing one can reset the node's runtime state.
  ///
  /// [`Tree::get_element_by_id`]: crate::app::Tree::get_element_by_id
  pub fn id(mut self, id: impl Into<Arc<str>>) -> Self {
    self.element_id = Some(id.into());
    self
  }

  /// Appends an HTML-like class for [`Tree::get_elements_by_class_name`]
  /// lookup. Classes do not affect reconciliation or styling.
  ///
  /// [`Tree::get_elements_by_class_name`]: crate::app::Tree::get_elements_by_class_name
  pub fn class(mut self, class: impl Into<Arc<str>>) -> Self {
    self.push_class(class.into());
    self
  }

  /// Appends several classes at once; see [`Node::class`].
  pub fn classes(mut self, classes: impl IntoIterator<Item: Into<Arc<str>>>) -> Self {
    for class in classes {
      self.push_class(class.into());
    }
    self
  }

  pub fn new() -> Self {
    Self::from_parts(LayoutKind::Leaf, NodeKind::Empty, vec![])
  }

  #[cfg_attr(not(feature = "form"), allow(dead_code))]
  pub fn logical() -> Self {
    Self::from_parts(LayoutKind::LogicalModifier, NodeKind::Empty, vec![])
  }

  pub fn text(content: &str) -> Self {
    let node = Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Text {
        state: TextState::new(),
        style: TextStyleSource::default_style(),
        transform_mode: TextTransformMode::default(),
      },
      vec![],
    );
    node.with_text_content(content)
  }

  pub fn text_styled(content: &str, style: TextStyle) -> Self {
    let node = Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Text {
        state: TextState::new(),
        style: TextStyleSource::explicit(style),
        transform_mode: TextTransformMode::default(),
      },
      vec![],
    );
    node.with_text_content(content)
  }

  #[cfg(feature = "markdown")]
  pub(crate) fn rich_text(spans: Vec<crate::layout::quad::RichTextSpan>) -> Self {
    Self::rich_text_with_links(spans, vec![], vec![])
  }

  /// Rich text with the links in it: [`Self::inline_link`] nodes, which the
  /// paragraph places on their spans. `generated` are the spans drawn as
  /// presentation (see `NodeKind::RichText::generated`).
  #[cfg(feature = "markdown")]
  pub(crate) fn rich_text_with_links(
    spans: Vec<crate::layout::quad::RichTextSpan>,
    links: Vec<Node>,
    generated: Vec<std::ops::Range<usize>>,
  ) -> Self {
    // The node's text is its content: without the spans it draws as
    // presentation. Selection and copying work on all its spans
    // (`rich_text_value`).
    let full = spans.iter().map(|span| span.text.as_str()).collect::<String>();
    let text = crate::node::inline_link::content_without_generated(&spans, &generated, &full, 0..full.len());
    Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::RichText {
        state: TextState::new(),
        spans,
        transform_mode: TextTransformMode::default(),
        generated: generated.into(),
      },
      links,
    )
    .with_text_content(&text)
  }

  /// A link of the `RichText` paragraph it is a child of, named `label`.
  #[cfg(feature = "markdown")]
  pub(crate) fn inline_link(link: crate::node::inline_link::InlineLink, label: &str) -> Self {
    Self::from_parts(LayoutKind::Leaf, NodeKind::InlineLink { link }, vec![])
      .with_tag_name("Link")
      .with_text_content(label)
  }

  pub fn text_input(value: Signal<String>) -> Self {
    Self::text_input_styled(value, TextStyle::default())
  }

  pub fn text_input_styled(value: Signal<String>, style: TextStyle) -> Self {
    let rendered = value.get_untracked();
    let node = Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::TextInput {
        state: TextInputState::new(value),
        style,
        placeholder_style: None,
      },
      vec![],
    );
    if rendered.is_empty() {
      node
    } else {
      node.with_text_content(&rendered)
    }
  }

  pub fn checkbox(value: Signal<bool>) -> Self {
    Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Checkbox {
        state: CheckboxState::new(value),
      },
      vec![],
    )
  }

  pub fn checkbox_box_style(self, style: CheckboxStyle) -> Self {
    if let Some(state) = self.checkbox_state() {
      state.set_style(style);
    }
    self
  }

  pub fn checkbox_checked_box_style(self, style: CheckboxStyle) -> Self {
    if let Some(state) = self.checkbox_state() {
      state.set_checked_style(style);
    }
    self
  }

  pub fn checkbox_box_hovered_style(self, style: CheckboxStyle) -> Self {
    if let Some(state) = self.checkbox_state() {
      state.set_hovered_style(style);
    }
    self
  }

  pub fn checkbox_checked_box_hovered_style(self, style: CheckboxStyle) -> Self {
    if let Some(state) = self.checkbox_state() {
      state.set_checked_hovered_style(style);
    }
    self
  }

  pub fn slider(value: Signal<i32>) -> Self {
    Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Slider {
        state: SliderState::new(value),
      },
      vec![],
    )
  }

  pub fn slider_f32(value: Signal<f32>) -> Self {
    Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Slider {
        state: SliderState::new_f32(value),
      },
      vec![],
    )
  }

  pub fn select() -> Self {
    let state = SelectState::new();
    let toggle = state.clone();
    let mut node = Self::row(0.0, Alignment::Center, vec![]);
    node.node_kind = NodeKind::Select { state };
    node
      .events
      .on_mouse_down
      .push(EventHandler::new(move |event: &MouseEvent| {
        if event.button == MouseButton::Left {
          toggle.toggle_open();
        }
      }));
    node
  }

  fn select_state(&self) -> Option<&SelectState> {
    match &self.node_kind {
      NodeKind::Select { state } => Some(state),
      _ => None,
    }
  }

  pub fn select_labels(self, labels: Vec<std::sync::Arc<str>>) -> Self {
    if let Some(state) = self.select_state() {
      state.set_labels(labels);
    }
    self
  }

  pub(crate) fn select_options_meta(self, details: Vec<Option<std::sync::Arc<str>>>, disabled: Vec<bool>) -> Self {
    if let Some(state) = self.select_state() {
      state.set_details(details);
      state.set_disabled(disabled);
    }
    self
  }

  pub fn select_selected(self, selected: Vec<usize>) -> Self {
    if let Some(state) = self.select_state() {
      state.set_selected(selected);
    }
    self
  }

  pub fn select_multiple(self, multiple: bool) -> Self {
    if let Some(state) = self.select_state() {
      state.set_multiple(multiple);
    }
    self
  }

  pub(crate) fn select_outside_press(self, outside_press: crate::app::ctx::OutsidePress) -> Self {
    if let Some(state) = self.select_state() {
      state.set_outside_press(outside_press);
    }
    self
  }

  pub fn select_placeholder(self, placeholder: Option<std::sync::Arc<str>>) -> Self {
    if let Some(state) = self.select_state() {
      state.set_placeholder(placeholder);
    }
    self
  }

  pub fn select_style(mut self, style: crate::node::SelectStyle) -> Self {
    let trigger = style.resolved_trigger(false, false, false);
    if let Some(padding) = trigger.padding.clone() {
      self = self.padding_custom(padding);
    }
    if let Some(min_width) = trigger.min_width {
      self = self.min_width(min_width);
    }
    if let Some(min_height) = trigger.min_height {
      self = self.min_height(min_height);
    }
    if let Some(state) = self.select_state() {
      state.set_style(style);
    }
    self
  }

  /// Binds the select to the signal it edits; that signal identifies the
  /// select across re-renders.
  pub(crate) fn select_binding(self, signal_id: usize) -> Self {
    if let Some(state) = self.select_state() {
      state.set_binding(signal_id);
    }
    self
  }

  pub fn select_on_change(self, on_change: SelectChangeCallback) -> Self {
    if let Some(state) = self.select_state() {
      state.set_on_change(on_change);
    }
    self
  }

  /// Apply a `SelectPartStyle`'s box fields (background/border/radius/padding/
  /// min size) to this node. Used by the runtime when building the popup menu.
  pub(crate) fn apply_select_part(mut self, part: &crate::node::select_style::SelectPartStyle) -> Self {
    if let Some(background) = &part.background {
      self.color.set(Some(background.clone()));
    }
    if let Some(border) = &part.border {
      self.border.set(Some(Box::new(border.clone())));
    }
    if let Some(radius) = part.border_radius {
      self.border_radius.set(Some(radius));
    }
    if let Some(shadow) = &part.box_shadow {
      self = self.box_shadow(shadow.clone());
    }
    if let Some(opacity) = part.opacity {
      self = self.opacity(opacity);
    }
    if let Some(padding) = &part.padding {
      self = self.padding_custom(padding.clone());
    }
    if let Some(min_width) = part.min_width {
      self = self.min_width(min_width);
    }
    if let Some(min_height) = part.min_height {
      self = self.min_height(min_height);
    }
    self
  }

  #[cfg(feature = "canvas")]
  pub fn canvas() -> Self {
    let mut node = Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Canvas {
        canvas: crate::canvas::CanvasHandle::new(),
      },
      vec![],
    );
    node.intrinsic_size = Some(Size::new(300.0, 150.0));
    node
  }

  #[cfg(feature = "canvas")]
  pub(crate) fn canvas_handle(&self) -> Option<crate::canvas::CanvasHandle> {
    match &self.node_kind {
      NodeKind::Canvas { canvas } => Some(canvas.clone()),
      _ => None,
    }
  }

  #[cfg(feature = "raster")]
  pub fn image(data: crate::images::ImageData) -> Self {
    let mut node = Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Image {
        data: data.clone(),
        fit: BackgroundSize::Stretch,
      },
      vec![],
    );
    node.intrinsic_size = Some(Size::new(data.width() as f32, data.height() as f32));
    node
  }

  /// An image the runtime decodes through the decoded-image cache; see
  /// [`Image::from_bytes`](crate::components::Image::from_bytes). Its
  /// children are the placeholder shown while it loads.
  #[cfg(feature = "image")]
  pub(crate) fn loading_image(request: crate::node::node_kind::LoadingImage) -> Self {
    crate::app::runtime::note_loading_image_built();
    Self::from_parts(
      LayoutKind::Stack {
        align: StackAlignment::Center,
      },
      NodeKind::LoadingImage {
        request: Box::new(request),
      },
      vec![],
    )
  }

  /// How an image's pixels fill its box: on an `Image` node, or on one that
  /// still loads.
  #[cfg(feature = "raster")]
  pub(crate) fn set_image_fit(&mut self, next_fit: BackgroundSize) {
    match &mut self.node_kind {
      NodeKind::Image { fit, .. } => *fit = next_fit,
      #[cfg(feature = "image")]
      NodeKind::LoadingImage { request } => request.fit = next_fit,
      _ => {}
    }
  }

  /// Changes a loading image's request: its key, size cap and error view.
  #[cfg(feature = "image")]
  pub(crate) fn update_loading_image(&mut self, f: impl FnOnce(&mut crate::node::node_kind::LoadingImage)) {
    if let NodeKind::LoadingImage { request } = &mut self.node_kind {
      f(request);
    }
  }

  #[cfg(feature = "raster")]
  pub fn video(data: crate::images::ImageData) -> Self {
    let mut node = Self::from_parts(
      LayoutKind::Leaf,
      NodeKind::Video {
        data: data.clone(),
        fit: BackgroundSize::Contain,
      },
      vec![],
    );
    node.intrinsic_size = Some(Size::new(data.width() as f32, data.height() as f32));
    node
  }

  #[cfg(feature = "raster")]
  pub(crate) fn set_video_fit(&mut self, next_fit: BackgroundSize) {
    if let NodeKind::Video { fit, .. } = &mut self.node_kind {
      *fit = next_fit;
    }
  }

  #[cfg(feature = "image")]
  pub fn resource_image(path: &str) -> Self {
    Self::from_parts(LayoutKind::Leaf, NodeKind::ResourceImage { path: path.into() }, vec![])
  }

  #[cfg(feature = "svg")]
  pub fn svg(data: crate::svg::SvgData) -> Self {
    let mut node = Self::from_parts(LayoutKind::Leaf, NodeKind::Svg { data: data.clone() }, vec![]);
    node.intrinsic_size = Some(Size::new(data.viewbox_width(), data.viewbox_height()));
    node
  }

  #[cfg(all(feature = "svg", feature = "resources"))]
  pub fn resource_svg(path: &str) -> Self {
    Self::from_parts(LayoutKind::Leaf, NodeKind::ResourceSvg { path: path.into() }, vec![])
  }

  pub fn row(spacing: impl Into<SpacingValue>, align: Alignment, children: Vec<Node>) -> Self {
    Self::from_parts(
      LayoutKind::Row {
        spacing: spacing.into(),
        align,
        justify: crate::layout::layout_kind::Justify::Start,
        wrap: crate::layout::layout_kind::FlexWrap::NoWrap,
      },
      NodeKind::Empty,
      children,
    )
  }

  pub fn column(spacing: impl Into<SpacingValue>, align: Alignment, children: Vec<Node>) -> Self {
    Self::from_parts(
      LayoutKind::Column {
        spacing: spacing.into(),
        align,
        justify: crate::layout::layout_kind::Justify::Start,
        wrap: crate::layout::layout_kind::FlexWrap::NoWrap,
      },
      NodeKind::Empty,
      children,
    )
  }

  pub fn stack(align: StackAlignment, children: Vec<Node>) -> Self {
    Self::from_parts(LayoutKind::Stack { align }, NodeKind::Empty, children)
  }

  pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
    let padding = padding.into();
    self.padding.merge_from(&padding);
    self.layout_cache.invalidate();
    self
  }

  pub fn padding_custom(self, padding: Padding) -> Self {
    self.padding(padding)
  }

  pub fn frame(mut self, frame: FrameConstraints) -> Self {
    self.frame = merge_frame(self.frame, frame);
    self.layout_cache.invalidate();
    self
  }

  pub fn offset(mut self, x: f32, y: f32) -> Self {
    self.offset = Some(Offset::new(x, y));
    self.layout_cache.invalidate();
    self
  }

  pub(crate) fn absolute_positioned(self, x: f32, y: f32, width: Option<Dimension>, height: Option<Dimension>) -> Self {
    let mut node = self;
    node.position = Position::Absolute { x, y, width, height };
    node.layout_cache.invalidate();
    node
  }

  pub fn align(mut self, alignment: Alignment) -> Self {
    self.align_self = Some(alignment);
    self.layout_cache.invalidate();
    self
  }

  pub fn flex(mut self, factor: f32) -> Self {
    self.flex = Some(crate::layout::layout_kind::FlexParams::grow(factor));
    self.layout_cache.invalidate();
    self
  }

  pub fn flex_shrink(mut self, factor: f32) -> Self {
    self.flex = Some(crate::layout::layout_kind::FlexParams {
      grow: 0.0,
      shrink: factor,
      basis: None,
    });
    self.layout_cache.invalidate();
    self
  }

  pub fn flex_full(mut self, grow: f32, shrink: f32, basis: Option<f32>) -> Self {
    self.flex = Some(crate::layout::layout_kind::FlexParams { grow, shrink, basis });
    self.layout_cache.invalidate();
    self
  }

  pub fn background(mut self, color: impl Into<BackgroundColor>) -> Self {
    self.color.set(Some(color.into()));
    self
  }

  pub fn background_gradient(mut self, gradient: impl Into<Gradient>) -> Self {
    self.rare_style.gradient = Some(gradient.into());
    self
  }

  pub(crate) fn caret_color(mut self, color: impl Into<TextColor>) -> Self {
    self.set_caret_color(color.into());
    self
  }

  fn set_caret_color(&mut self, color: TextColor) {
    self.rare_style.caret_color = Some(color);
  }

  pub(crate) fn selection_color(mut self, color: impl Into<TextColor>) -> Self {
    self.set_selection_color(color.into());
    self
  }

  fn set_selection_color(&mut self, color: TextColor) {
    self.rare_style.selection_color = Some(color);
  }

  pub(crate) fn text_input_caret_mode(mut self, mode: CaretMode) -> Self {
    if matches!(self.node_kind, NodeKind::TextInput { .. }) {
      self.rare_style.caret_mode = Some(mode);
    }
    self
  }

  pub fn corner_radius(mut self, radius: impl Into<RadiusValue>) -> Self {
    self.border_radius.set(Some(ThemedBorderRadius::all(radius)));
    self
  }

  pub fn corner_radius_custom(mut self, radius: BorderRadius) -> Self {
    self.border_radius.set(Some(radius.into()));
    self
  }

  pub fn corner_radius_top_left(mut self, radius: impl Into<RadiusValue>) -> Self {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.top_left = radius.into();
    self.border_radius.set(Some(border_radius));
    self
  }

  pub fn corner_radius_top_right(mut self, radius: impl Into<RadiusValue>) -> Self {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.top_right = radius.into();
    self.border_radius.set(Some(border_radius));
    self
  }

  pub fn corner_radius_bottom_right(mut self, radius: impl Into<RadiusValue>) -> Self {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.bottom_right = radius.into();
    self.border_radius.set(Some(border_radius));
    self
  }

  pub fn corner_radius_bottom_left(mut self, radius: impl Into<RadiusValue>) -> Self {
    let mut border_radius = (*self.border_radius).unwrap_or_default();
    border_radius.bottom_left = radius.into();
    self.border_radius.set(Some(border_radius));
    self
  }

  pub fn rounded(mut self, radius: impl Into<RadiusValue>) -> Self {
    self.border_radius.set(Some(ThemedBorderRadius::all(radius)));
    self
  }

  pub fn border_inside(mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    self
      .border
      .set(Some(Box::new(Borders::all(Border::inside(width, color)))));
    self
  }

  pub fn border_outside(mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    self
      .border
      .set(Some(Box::new(Borders::all(Border::outside(width, color)))));
    self
  }

  pub fn border_center(mut self, width: impl Into<BorderSizeValue>, color: impl Into<BackgroundColor>) -> Self {
    self
      .border
      .set(Some(Box::new(Borders::all(Border::center(width, color)))));
    self
  }

  pub fn border(mut self, border: Border) -> Self {
    self.border.set(Some(Box::new(Borders::all(border))));
    self
  }

  pub fn border_custom(mut self, border: Borders) -> Self {
    self.border.set(Some(Box::new(border)));
    self
  }

  pub fn border_top(mut self, border: Border) -> Self {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.top = Some(border);
    self.border.set(Some(Box::new(borders)));
    self
  }

  pub fn border_right(mut self, border: Border) -> Self {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.right = Some(border);
    self.border.set(Some(Box::new(borders)));
    self
  }

  pub fn border_bottom(mut self, border: Border) -> Self {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.bottom = Some(border);
    self.border.set(Some(Box::new(borders)));
    self
  }

  pub fn border_left(mut self, border: Border) -> Self {
    let mut borders = self.border.as_deref().cloned().unwrap_or_default();
    borders.left = Some(border);
    self.border.set(Some(Box::new(borders)));
    self
  }

  pub fn box_shadow(mut self, shadow: impl Into<BoxShadowValue>) -> Self {
    NodeUpdate::box_shadow(&mut self, shadow);
    self
  }

  pub fn cursor(mut self, cursor: CursorIcon) -> Self {
    self.cursor = Some(cursor);
    self
  }

  #[cfg(feature = "raster")]
  pub fn background_image(mut self, data: impl Into<crate::images::ImageKind>) -> Self {
    match data.into() {
      crate::images::ImageKind::Bytes(data) => {
        self.background_image.set(Some(Box::new(data)));
      }
      crate::images::ImageKind::Native(data) => {
        self.background_image.set(Some(Box::new(data.image_data())));
      }
      #[cfg(all(feature = "image", feature = "resources"))]
      crate::images::ImageKind::Resource(path) => {
        self.background_resource_image = Some(path);
      }
    }
    self
  }

  #[cfg(feature = "raster")]
  pub fn background_size(mut self, size: BackgroundSize) -> Self {
    self.background_size = size;
    self
  }

  #[cfg(feature = "raster")]
  pub fn background_cover(self) -> Self {
    self.background_size(BackgroundSize::Cover)
  }

  #[cfg(feature = "raster")]
  pub fn background_contain(self) -> Self {
    self.background_size(BackgroundSize::Contain)
  }

  pub fn cursor_icon(&self) -> Option<CursorIcon> {
    self.state_style().cursor.or(self.cursor)
  }

  pub fn hovered_style(mut self, style: Style) -> Self {
    self.state_styles.hovered = Some(style);
    self.state_styles.invalidate_layout_style();
    self
  }

  pub fn active_style(mut self, style: Style) -> Self {
    self.state_styles.active = Some(style);
    self.state_styles.invalidate_layout_style();
    self
  }

  /// Merged over the node's style while it has focus, however the focus got
  /// there (pointer, keyboard or a request).
  pub fn focused_style(mut self, style: Style) -> Self {
    self.state_styles.focused = Some(style);
    self.state_styles.invalidate_layout_style();
    self
  }

  /// Merged over the node's style (and over [`Node::focused_style`]) while it
  /// has keyboard focus, like CSS `:focus-visible`: for a focus ring that a
  /// pointer press does not show. See [`InteractionState::is_focus_visible`].
  pub fn focus_visible_style(mut self, style: Style) -> Self {
    self.state_styles.focus_visible = Some(style);
    self.state_styles.invalidate_layout_style();
    self
  }

  /// Merged over the node's style while files from the OS are dragged over
  /// it, so a drop target can highlight.
  pub fn drag_over_style(mut self, style: Style) -> Self {
    self.state_styles.drag_over = Some(style);
    self.state_styles.invalidate_layout_style();
    self
  }

  pub fn hovered(self, f: impl FnOnce(Style) -> Style) -> Self {
    self.hovered_style(f(Style::new()))
  }

  pub fn drag_over(self, f: impl FnOnce(Style) -> Style) -> Self {
    self.drag_over_style(f(Style::new()))
  }

  pub fn active(self, f: impl FnOnce(Style) -> Style) -> Self {
    self.active_style(f(Style::new()))
  }

  pub fn focused(self, f: impl FnOnce(Style) -> Style) -> Self {
    self.focused_style(f(Style::new()))
  }

  /// Builder form of [`Node::focus_visible_style`].
  pub fn focus_visible(self, f: impl FnOnce(Style) -> Style) -> Self {
    self.focus_visible_style(f(Style::new()))
  }

  // --- Event handlers ---

  pub fn on_click(mut self, f: impl IntoMouseEventHandler) -> Self {
    self.events.on_click.push(f.into_event_handler());
    self
  }

  pub fn off_click(mut self, f: impl IntoMouseEventHandler) -> Self {
    let handler = f.into_event_handler();
    self.events.on_click.retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_mouse_click(mut self, button: MouseButton, f: impl IntoMouseEventHandler) -> Self {
    self.events.on_mouse_click.push((button, f.into_event_handler()));
    self
  }

  pub fn off_mouse_click(mut self, button: MouseButton, f: impl IntoMouseEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_click
      .retain(|(handler_button, existing)| *handler_button != button || !existing.same_handler(&handler));
    self
  }

  pub fn on_dblclick(mut self, f: impl IntoMouseEventHandler) -> Self {
    self.events.on_dblclick.push(f.into_event_handler());
    self
  }

  pub fn off_dblclick(mut self, f: impl IntoMouseEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_dblclick
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_mouse_down(mut self, f: impl IntoMouseEventHandler) -> Self {
    self.events.on_mouse_down.push(f.into_event_handler());
    self
  }

  pub fn off_mouse_down(mut self, f: impl IntoMouseEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_down
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_mouse_up(mut self, f: impl IntoMouseEventHandler) -> Self {
    self.events.on_mouse_up.push(f.into_event_handler());
    self
  }

  pub fn off_mouse_up(mut self, f: impl IntoMouseEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_up
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_mouse_move(mut self, f: impl IntoMouseEventHandler) -> Self {
    self.events.on_mouse_move.push(f.into_event_handler());
    self
  }

  pub fn off_mouse_move(mut self, f: impl IntoMouseEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_mouse_move
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn start_drag_buttons(mut self, buttons: MouseButtonMask) -> Self {
    self.events.start_drag_buttons = buttons;
    self
  }

  pub fn on_drag_start(mut self, f: impl IntoDragEventHandler) -> Self {
    self.events.on_drag_start.push(f.into_event_handler());
    self
  }

  pub fn off_drag_start(mut self, f: impl IntoDragEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_drag_start
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_drag_move(mut self, f: impl IntoDragEventHandler) -> Self {
    self.events.on_drag_move.push(f.into_event_handler());
    self
  }

  pub fn off_drag_move(mut self, f: impl IntoDragEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_drag_move
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_drag_end(mut self, f: impl IntoDragEventHandler) -> Self {
    self.events.on_drag_end.push(f.into_event_handler());
    self
  }

  pub fn off_drag_end(mut self, f: impl IntoDragEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_drag_end
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_drop(mut self, f: impl IntoDropEventHandler) -> Self {
    self.events.on_drop.push(f.into_event_handler());
    self
  }

  pub fn off_drop(mut self, f: impl IntoDropEventHandler) -> Self {
    let handler = f.into_event_handler();
    self.events.on_drop.retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_file_drag_enter(mut self, f: impl IntoFileDragEventHandler) -> Self {
    NodeUpdate::on_file_drag_enter(&mut self, f);
    self
  }

  pub fn on_file_drag_over(mut self, f: impl IntoFileDragEventHandler) -> Self {
    NodeUpdate::on_file_drag_over(&mut self, f);
    self
  }

  pub fn on_file_drag_leave(mut self, f: impl IntoVoidEventHandler) -> Self {
    NodeUpdate::on_file_drag_leave(&mut self, f);
    self
  }

  pub fn on_file_drop(mut self, f: impl IntoFileDropEventHandler) -> Self {
    NodeUpdate::on_file_drop(&mut self, f);
    self
  }

  pub fn on_paste(mut self, f: impl IntoPasteEventHandler) -> Self {
    NodeUpdate::on_paste(&mut self, f);
    self
  }

  /// A trackpad swipe between pages ended over this element; see [`HistorySwipeEvent`].
  pub fn on_history_swipe(mut self, f: impl IntoHistorySwipeEventHandler) -> Self {
    NodeUpdate::on_history_swipe(&mut self, f);
    self
  }

  pub fn off_history_swipe(mut self, f: impl IntoHistorySwipeEventHandler) -> Self {
    NodeUpdate::off_history_swipe(&mut self, f);
    self
  }

  /// A finger landed on this element; see [`TouchEvent`]. A handler that calls `prevent_default` takes the finger
  /// for itself: no mouse events, click or scrolling follow for this touch.
  pub fn on_touch_start(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::on_touch_start(&mut self, f);
    self
  }

  pub fn off_touch_start(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::off_touch_start(&mut self, f);
    self
  }

  /// A finger that landed on this element moved, wherever it now is; see [`TouchEvent`].
  pub fn on_touch_move(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::on_touch_move(&mut self, f);
    self
  }

  pub fn off_touch_move(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::off_touch_move(&mut self, f);
    self
  }

  /// A finger that landed on this element lifted; see [`TouchEvent`].
  pub fn on_touch_end(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::on_touch_end(&mut self, f);
    self
  }

  pub fn off_touch_end(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::off_touch_end(&mut self, f);
    self
  }

  /// The system took away a finger that landed on this element; see [`TouchEvent`].
  pub fn on_touch_cancel(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::on_touch_cancel(&mut self, f);
    self
  }

  pub fn off_touch_cancel(mut self, f: impl IntoTouchEventHandler) -> Self {
    NodeUpdate::off_touch_cancel(&mut self, f);
    self
  }

  /// A finger rested on this element for half a second; see [`LongPressEvent`].
  pub fn on_long_press(mut self, f: impl IntoLongPressEventHandler) -> Self {
    NodeUpdate::on_long_press(&mut self, f);
    self
  }

  pub fn off_long_press(mut self, f: impl IntoLongPressEventHandler) -> Self {
    NodeUpdate::off_long_press(&mut self, f);
    self
  }

  /// Two fingers on this element moved apart or together; see [`PinchEvent`].
  pub fn on_pinch(mut self, f: impl IntoPinchEventHandler) -> Self {
    NodeUpdate::on_pinch(&mut self, f);
    self
  }

  pub fn off_pinch(mut self, f: impl IntoPinchEventHandler) -> Self {
    NodeUpdate::off_pinch(&mut self, f);
    self
  }

  /// Two fingers on this element turned about their middle; see [`RotateEvent`].
  pub fn on_rotate(mut self, f: impl IntoRotateEventHandler) -> Self {
    NodeUpdate::on_rotate(&mut self, f);
    self
  }

  pub fn off_rotate(mut self, f: impl IntoRotateEventHandler) -> Self {
    NodeUpdate::off_rotate(&mut self, f);
    self
  }

  /// One or more fingers tapped this element; see [`TapEvent`].
  pub fn on_tap(mut self, f: impl IntoTapEventHandler) -> Self {
    NodeUpdate::on_tap(&mut self, f);
    self
  }

  pub fn off_tap(mut self, f: impl IntoTapEventHandler) -> Self {
    NodeUpdate::off_tap(&mut self, f);
    self
  }

  /// Fingers flicked across this element; see [`SwipeEvent`].
  pub fn on_swipe(mut self, f: impl IntoSwipeEventHandler) -> Self {
    NodeUpdate::on_swipe(&mut self, f);
    self
  }

  pub fn off_swipe(mut self, f: impl IntoSwipeEventHandler) -> Self {
    NodeUpdate::off_swipe(&mut self, f);
    self
  }

  /// Fingers are travelling over this element; see [`PanEvent`].
  pub fn on_pan(mut self, f: impl IntoPanEventHandler) -> Self {
    NodeUpdate::on_pan(&mut self, f);
    self
  }

  pub fn off_pan(mut self, f: impl IntoPanEventHandler) -> Self {
    NodeUpdate::off_pan(&mut self, f);
    self
  }

  /// A finger came in from an edge of the window over this element; see [`EdgePanEvent`].
  pub fn on_edge_pan(mut self, f: impl IntoEdgePanEventHandler) -> Self {
    NodeUpdate::on_edge_pan(&mut self, f);
    self
  }

  pub fn off_edge_pan(mut self, f: impl IntoEdgePanEventHandler) -> Self {
    NodeUpdate::off_edge_pan(&mut self, f);
    self
  }

  pub fn on_mouse_enter(mut self, f: impl IntoVoidEventHandler) -> Self {
    self.events.on_mouse_enter.push(f.into_void_event_handler());
    self
  }

  pub fn off_mouse_enter(mut self, f: impl IntoVoidEventHandler) -> Self {
    let handler = f.into_void_event_handler();
    self
      .events
      .on_mouse_enter
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_mouse_leave(mut self, f: impl IntoVoidEventHandler) -> Self {
    self.events.on_mouse_leave.push(f.into_void_event_handler());
    self
  }

  pub fn off_mouse_leave(mut self, f: impl IntoVoidEventHandler) -> Self {
    let handler = f.into_void_event_handler();
    self
      .events
      .on_mouse_leave
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_key_down(mut self, f: impl IntoKeyboardEventHandler) -> Self {
    self.events.on_key_down.push(f.into_event_handler());
    self
  }

  pub fn off_key_down(mut self, f: impl IntoKeyboardEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_key_down
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_key_up(mut self, f: impl IntoKeyboardEventHandler) -> Self {
    self.events.on_key_up.push(f.into_event_handler());
    self
  }

  pub fn off_key_up(mut self, f: impl IntoKeyboardEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_key_up
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_focus(mut self, f: impl IntoVoidEventHandler) -> Self {
    self.events.on_focus.push(f.into_void_event_handler());
    self
  }

  pub fn off_focus(mut self, f: impl IntoVoidEventHandler) -> Self {
    let handler = f.into_void_event_handler();
    self.events.on_focus.retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_blur(mut self, f: impl IntoVoidEventHandler) -> Self {
    self.events.on_blur.push(f.into_void_event_handler());
    self
  }

  pub fn off_blur(mut self, f: impl IntoVoidEventHandler) -> Self {
    let handler = f.into_void_event_handler();
    self.events.on_blur.retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_scroll(mut self, f: impl IntoScrollEventHandler) -> Self {
    self.events.on_scroll.push(f.into_event_handler());
    self
  }

  pub fn off_scroll(mut self, f: impl IntoScrollEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_scroll_start(mut self, f: impl IntoScrollEventHandler) -> Self {
    self.events.on_scroll_start.push(f.into_event_handler());
    self
  }

  pub fn off_scroll_start(mut self, f: impl IntoScrollEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll_start
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn on_scroll_end(mut self, f: impl IntoScrollEventHandler) -> Self {
    self.events.on_scroll_end.push(f.into_event_handler());
    self
  }

  pub fn off_scroll_end(mut self, f: impl IntoScrollEventHandler) -> Self {
    let handler = f.into_event_handler();
    self
      .events
      .on_scroll_end
      .retain(|existing| !existing.same_handler(&handler));
    self
  }

  pub fn opacity(mut self, value: f32) -> Self {
    self.set_opacity(value);
    self
  }

  fn set_opacity(&mut self, value: f32) {
    self.opacity = value;
  }

  pub fn transform(mut self, t: Transform2D) -> Self {
    self.transform = t;
    self
  }

  pub fn transition(mut self, spec: Transition) -> Self {
    self.push_transition(spec);
    self
  }

  fn push_transition(&mut self, spec: Transition) {
    self.transitions.push(spec);
    self.layout_cache.invalidate();
  }

  pub fn animation(mut self, spec: Animation) -> Self {
    self.set_animation(spec);
    self
  }

  fn set_animation(&mut self, spec: Animation) {
    self.animation = Some(Box::new(spec));
    self.layout_cache.invalidate();
  }

  pub fn scrollbar(mut self, style: ScrollBarStyle) -> Self {
    self.scrollbar_style.set(Some(Box::new(style)));
    self
  }

  pub fn scrollbar_hovered(mut self, f: impl Fn(ScrollBarStyle) -> ScrollBarStyle + Send + Sync + 'static) -> Self {
    self.scrollbar_hovered_style = Some(Arc::new(f));
    self
  }

  pub fn culling(mut self, enabled: bool) -> Self {
    if let LayoutKind::ScrollModifier { culling, .. } = &mut self.layout_kind {
      *culling = enabled;
    }
    self
  }

  /// Middle-button autoscroll for a scroll container, off by default: with it
  /// on, pressing the middle button over the container and moving the pointer
  /// away from where it went down scrolls the content toward the pointer,
  /// faster the farther the pointer is (on the axes the container scrolls),
  /// until the button is released, Esc is pressed or the pointer leaves the
  /// window. A mark is drawn where the button went down. The innermost
  /// container under the pointer that has it on and can scroll takes the
  /// press; a press on an element that takes the middle button itself (a
  /// handler that calls `prevent_default` on the press, a middle-click or
  /// middle-drag handler) is left to that element. Does nothing on a node
  /// that does not scroll.
  pub fn middle_button_autoscroll(mut self, enabled: bool) -> Self {
    NodeUpdate::middle_button_autoscroll(&mut self, enabled);
    self
  }

  /// Whether a middle-button press may autoscroll this scroll container
  /// ([`Self::middle_button_autoscroll`]).
  pub(crate) fn has_middle_button_autoscroll(&self) -> bool {
    matches!(
      self.layout_kind,
      LayoutKind::ScrollModifier {
        middle_button_autoscroll: true,
        ..
      }
    )
  }

  pub fn hit_test(mut self, behavior: HitTestBehavior) -> Self {
    self.hit_test = behavior;
    self
  }

  pub fn pointer_events_none(mut self) -> Self {
    self.hit_test = HitTestBehavior::None;
    self
  }

  pub fn ref_element(mut self, element_ref: impl Into<CoreElementRef>) -> Self {
    self.element_ref = Some(element_ref.into());
    self.layout_cache.mark_watch();
    self
  }

  /// Attach data to drags starting on this node; the drop target's handler
  /// receives it through `DropEvent::payload`.
  pub fn drag_payload(mut self, payload: crate::app::events::DragPayload) -> Self {
    self.drag_payload = Some(payload);
    self
  }

  pub(crate) fn element_ref_handle(&mut self) -> CoreElementRef {
    let reference = self.element_ref.get_or_insert_with(CoreElementRef::new).clone();
    reference.track_layout(self.layout_cache.watch());
    reference
  }

  pub fn interactive(mut self, state: InteractionState) -> Self {
    self.interaction = Some(state);
    self
  }

  /// Whether the node can take focus. Buttons, text inputs, checkboxes,
  /// sliders, selects and nodes with a `tab_index` are focusable by default.
  /// `focusable(true)` makes any node focusable by click and
  /// [`Ctx::focus`](crate::app::ctx::Ctx::focus); `focusable(false)` keeps a
  /// node from ever taking focus, by click, Tab or request.
  pub fn focusable(mut self, focusable: bool) -> Self {
    self.focusable = Some(focusable);
    self
  }

  /// HTML `tabindex`. Inside a form, controls are in the Tab order without
  /// one; outside a form, only nodes with `tab_index(0)` or higher are.
  /// Positive values come first, in ascending order, then `0` and (in forms)
  /// unset in tree order. `-1` removes the node from the Tab order but keeps
  /// it focusable by click. Setting a tab index makes the node focusable
  /// unless it is `focusable(false)`.
  pub fn tab_index(mut self, tab_index: i32) -> Self {
    self.tab_index = Some(tab_index);
    self
  }

  pub fn button_kind(mut self, kind: ButtonKind) -> Self {
    self.button_kind = Some(kind);
    self
  }

  #[cfg(feature = "form")]
  pub fn form(mut self, on_submit: impl Fn(FormData) + Send + Sync + 'static) -> Self {
    self.events.on_submit = Some(Arc::new(on_submit));
    self
  }

  #[cfg(feature = "form")]
  pub fn name(mut self, name: impl Into<Arc<str>>) -> Self {
    self.form_name = Some(name.into());
    self
  }

  pub fn text_content(&self) -> Option<&str> {
    self.text_content.as_deref()
  }

  /// Text safe to expose through built-in inspectors. Layout/editing keep the
  /// underlying value; inspector boundaries follow the input's mask instead,
  /// and show sensitive text as `REDACTED`.
  #[cfg(any(feature = "mcp", feature = "devtools"))]
  pub(crate) fn inspection_text(&self) -> Option<String> {
    match self.node_kind() {
      NodeKind::TextInput { state, .. } if state.is_masked() => {
        state.rendered_text().map(|_| state.rendered_text_for_layout())
      }
      NodeKind::Text { state, .. } if state.is_sensitive() => {
        self.text_content().map(|_| crate::core::REDACTED.to_owned())
      }
      _ => self.text_content().map(str::to_owned),
    }
  }

  /// All of a rich text's spans, the presentation ones included: the text its
  /// caret positions and selection index. Its [`Self::text_content`] leaves
  /// the presentation out.
  /// Fills behind ranges of a `Text` or `RichText` leaf; nothing for any
  /// other node.
  pub(crate) fn set_text_highlights(
    &mut self,
    highlights: impl Into<std::sync::Arc<[crate::node::text_highlight::TextHighlight]>>,
  ) {
    match &self.node_kind {
      NodeKind::Text { state, .. } => state.set_highlights(highlights.into()),
      #[cfg(feature = "markdown")]
      NodeKind::RichText { state, .. } => state.set_highlights(highlights.into()),
      _ => {}
    }
  }

  #[cfg(feature = "markdown")]
  pub(crate) fn rich_text_value(&self) -> Option<String> {
    let NodeKind::RichText { spans, .. } = self.node_kind() else {
      return None;
    };
    Some(spans.iter().map(|span| span.text.as_str()).collect())
  }

  /// The bytes `range` of a rich text's `text`, without the spans it draws
  /// as presentation (a link's isolate marks and trailing runs); `text`
  /// itself for any other node.
  #[cfg(feature = "markdown")]
  pub(crate) fn rich_text_content(&self, text: &str, range: std::ops::Range<usize>) -> String {
    let NodeKind::RichText { spans, generated, .. } = self.node_kind() else {
      return text.get(range).unwrap_or_default().to_owned();
    };
    crate::node::inline_link::content_without_generated(spans, generated, text, range)
  }

  #[cfg(any(feature = "mcp", feature = "devtools"))]
  pub(crate) fn is_masked_input(&self) -> bool {
    matches!(self.node_kind(), NodeKind::TextInput { state, .. } if state.is_masked())
  }

  pub(crate) fn is_focusable(&self) -> bool {
    self.focusable.unwrap_or_else(|| {
      self.tab_index.is_some()
        || self.button_kind.is_some()
        || matches!(
          self.node_kind(),
          NodeKind::TextInput { .. } | NodeKind::Checkbox { .. } | NodeKind::Slider { .. } | NodeKind::Select { .. }
        )
    })
  }

  /// `focusable(false)`: the node never takes focus.
  pub(crate) fn is_focus_disabled(&self) -> bool {
    self.focusable == Some(false)
  }

  pub(crate) fn tab_index_value(&self) -> Option<i32> {
    self.tab_index
  }

  pub(crate) fn button_kind_value(&self) -> Option<ButtonKind> {
    self.button_kind
  }

  #[cfg(feature = "form")]
  pub(crate) fn submit_handler(&self) -> Option<FormSubmitCallback> {
    self.events.on_submit.clone()
  }

  #[cfg(feature = "form")]
  pub(crate) fn form_name_value(&self) -> Option<&str> {
    self.form_name.as_deref()
  }

  pub fn selectable(self, selectable: bool) -> Self {
    match &self.node_kind {
      NodeKind::Text { state, .. } => state.set_selectable(selectable),
      #[cfg(feature = "markdown")]
      NodeKind::RichText { state, .. } => state.set_selectable(selectable),
      _ => {}
    }
    self
  }

  #[cfg(feature = "markdown")]
  pub(crate) fn selectable_recursive(&mut self, selectable: bool) {
    match &self.node_kind {
      NodeKind::Text { state, .. } => state.set_selectable(selectable),
      NodeKind::RichText { state, .. } => state.set_selectable(selectable),
      _ => {}
    }
    for child in &mut self.children {
      child.selectable_recursive(selectable);
    }
  }

  pub fn text_transform_mode(mut self, mode: TextTransformMode) -> Self {
    if let NodeKind::Text { transform_mode, .. } = &mut self.node_kind {
      *transform_mode = mode;
    }
    self
  }

  pub fn text_variant(mut self, typography_style: impl Into<TypographyStyle>) -> Self {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_variant(typography_style);
      self.layout_cache.invalidate();
    }
    self
  }

  pub fn text_color(mut self, color: impl Into<TextColor>) -> Self {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_color(color);
    }
    self
  }

  pub fn text_align(mut self, align: impl Into<TextAlign>) -> Self {
    if let NodeKind::Text { style, .. } = &mut self.node_kind {
      style.set_text_align(align);
      self.layout_cache.invalidate();
    }
    self
  }

  pub fn placeholder(mut self, placeholder: &str) -> Self {
    self.set_placeholder(placeholder);
    self
  }

  fn set_placeholder(&mut self, placeholder: &str) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      let placeholder_changed = state.placeholder().as_deref() != Some(placeholder);
      state.set_placeholder(placeholder);
      if placeholder_changed && state.value().is_empty() {
        self.text_content.set(Some(placeholder.to_owned()));
      }
    }
  }

  pub fn text_input_overflow(mut self, overflow: crate::node::node_kind::TextInputOverflow) -> Self {
    self.set_text_input_overflow(overflow);
    self
  }

  fn set_text_input_overflow(&mut self, overflow: crate::node::node_kind::TextInputOverflow) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_overflow(overflow);
    }
  }

  fn set_text_input_unfocused_overflow_anchor(&mut self, anchor: crate::node::node_kind::TextInputOverflowAnchor) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_unfocused_overflow_anchor(anchor);
    }
  }

  /// Masks each value character with `•` (U+2022).
  pub fn text_input_mask(mut self) -> Self {
    NodeUpdate::text_input_mask(&mut self);
    self
  }

  pub fn text_input_mask_char(self, mask: char) -> Self {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_mask(Some(mask));
    }
    self
  }

  pub fn text_input_unmask(self) -> Self {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_mask(None);
    }
    self
  }

  pub fn text_input_style(mut self, text_style: TextStyle) -> Self {
    self.set_text_input_style(text_style);
    self
  }

  fn set_text_input_style(&mut self, text_style: TextStyle) {
    if let NodeKind::TextInput { style, .. } = &mut self.node_kind {
      if *style == text_style {
        return;
      }
      *style = text_style;
      self.layout_cache.invalidate();
    }
  }

  pub fn text_input_placeholder_style(mut self, text_style: TextStyle) -> Self {
    self.set_text_input_placeholder_style(text_style);
    self
  }

  fn set_text_input_placeholder_style(&mut self, mut text_style: TextStyle) {
    if let NodeKind::TextInput {
      style,
      placeholder_style,
      ..
    } = &mut self.node_kind
    {
      text_style.text_align = style.text_align;
      if placeholder_style.as_deref() == Some(&text_style) {
        return;
      }
      *placeholder_style = Some(Box::new(text_style));
      self.layout_cache.invalidate();
    }
  }

  pub fn text_input_align(mut self, align: impl Into<TextAlign>) -> Self {
    self.set_text_input_align(align);
    self
  }

  fn set_text_input_align(&mut self, align: impl Into<TextAlign>) {
    if let NodeKind::TextInput {
      style,
      placeholder_style,
      ..
    } = &mut self.node_kind
    {
      let align = align.into();
      if style.text_align == align
        && placeholder_style
          .as_deref()
          .is_none_or(|style| style.text_align == align)
      {
        return;
      }
      style.text_align = align;
      if let Some(placeholder_style) = placeholder_style {
        placeholder_style.text_align = align;
      }
      self.layout_cache.invalidate();
    }
  }

  pub fn text_input_rows(mut self, min_rows: usize, max_rows: usize) -> Self {
    self.set_text_input_rows(min_rows, max_rows);
    self
  }

  fn set_text_input_rows(&mut self, min_rows: usize, max_rows: usize) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_rows(min_rows, max_rows);
    }
  }

  pub fn text_input_min_rows(mut self, min_rows: usize) -> Self {
    self.set_text_input_min_rows(min_rows);
    self
  }

  fn set_text_input_min_rows(&mut self, min_rows: usize) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_min_rows(min_rows);
    }
  }

  pub fn text_input_max_rows(mut self, max_rows: usize) -> Self {
    self.set_text_input_max_rows(max_rows);
    self
  }

  fn set_text_input_max_rows(&mut self, max_rows: usize) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_max_rows(max_rows);
    }
  }

  pub fn text_input_rows_exact(mut self, rows: usize) -> Self {
    self.set_text_input_rows_exact(rows);
    self
  }

  fn set_text_input_rows_exact(&mut self, rows: usize) {
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.set_rows_exact(rows);
    }
  }

  pub fn range(self, min: i32, max: i32) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_range(min, max);
    }
    self
  }

  pub fn range_f32(self, min: f32, max: f32) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_range_f32(min, max);
    }
    self
  }

  pub fn slider_step(self, step: f32) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_step(step);
    }
    self
  }

  pub fn slider_track_style(self, style: SliderPartStyle) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_track_style(style);
    }
    self
  }

  pub fn slider_track_hovered_style(self, style: SliderPartStyle) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_track_hovered_style(style);
    }
    self
  }

  pub fn slider_fill_style(self, style: SliderPartStyle) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_fill_style(style);
    }
    self
  }

  pub fn slider_fill_hovered_style(self, style: SliderPartStyle) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_fill_hovered_style(style);
    }
    self
  }

  pub fn slider_thumb_style(self, style: SliderPartStyle) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_thumb_style(style);
    }
    self
  }

  pub fn slider_thumb_hovered_style(self, style: SliderPartStyle) -> Self {
    if let Some(state) = self.slider_state() {
      state.set_thumb_hovered_style(style);
    }
    self
  }

  pub(crate) fn checkbox_state(&self) -> Option<&CheckboxState> {
    if let NodeKind::Checkbox { state } = &self.node_kind {
      return Some(state);
    }
    if self.children.len() == 1 {
      return self.children[0].checkbox_state();
    }
    None
  }

  pub(crate) fn slider_state(&self) -> Option<&SliderState> {
    if let NodeKind::Slider { state } = &self.node_kind {
      return Some(state);
    }
    if self.children.len() == 1 {
      return self.children[0].slider_state();
    }
    None
  }

  pub fn clip(mut self) -> Self {
    self.set_overflow_through_logical(Overflow::Hidden);
    self
  }

  pub fn overflow_visible(mut self) -> Self {
    self.set_overflow_through_logical(Overflow::Visible);
    self
  }

  fn set_overflow_through_logical(&mut self, overflow: Overflow) {
    self.overflow = overflow;
    if matches!(&self.layout_kind, LayoutKind::LogicalModifier) && self.children.len() == 1 {
      self.children[0].set_overflow_through_logical(overflow);
    }
  }

  pub fn intrinsic(mut self, width: f32, height: f32) -> Self {
    self.intrinsic_size = Some(Size::new(width, height));
    self
  }

  pub fn assign_ids(&mut self, id_gen: &IdGenerator) {
    if !self.node_id.is_assigned() {
      self.node_id = id_gen.next();
    }
    for child in &mut self.children {
      child.assign_ids(id_gen);
    }
  }

  pub fn free_ids(&mut self, id_gen: &IdGenerator) {
    if self.node_id.is_assigned() {
      id_gen.free(self.node_id);
      self.node_id = NodeId::UNASSIGNED;
    }
    for child in &mut self.children {
      child.free_ids(id_gen);
    }
  }

  // --- Accessors ---

  pub fn node_id(&self) -> NodeId {
    self.node_id
  }

  pub fn tag_name(&self) -> &str {
    &self.tag_name
  }

  pub(crate) fn with_tag_name(mut self, tag_name: impl Into<Arc<str>>) -> Self {
    self.tag_name = tag_name.into();
    self
  }

  pub(crate) fn set_tag_name(&mut self, tag_name: impl Into<Arc<str>>) {
    self.tag_name = tag_name.into();
  }

  pub(crate) fn layout_kind(&self) -> &LayoutKind {
    &self.layout_kind
  }

  pub(crate) fn component_slot_id(&self) -> Option<u64> {
    self.component_slot_id
  }

  pub(crate) fn set_component_slot_id(&mut self, id: u64) {
    self.component_slot_id = Some(id);
  }

  #[allow(dead_code)]
  pub(crate) fn component_key(&self) -> Option<&str> {
    self.component_key.as_deref()
  }

  pub(crate) fn set_overlay_declaration(&mut self, spec: crate::app::ctx::OverlaySpec) {
    self.overlay_declaration = Some(Box::new(spec));
  }

  pub(crate) fn set_dropdown_anchor(&mut self, anchor: CoreElementRef) {
    self.dropdown_anchor = Some(anchor);
  }

  pub(crate) fn dropdown_anchor(&self) -> Option<&CoreElementRef> {
    self.dropdown_anchor.as_ref()
  }

  pub(crate) fn set_modal_declaration(&mut self, spec: crate::app::ctx::ModalSpec) {
    self.modal_declaration = Some(Box::new(spec));
  }

  pub(crate) fn set_layout_neutral(&mut self, layout_neutral: bool) {
    self.layout_neutral = layout_neutral;
    self.layout_cache.invalidate();
  }

  pub(crate) fn overlay_declaration(&self) -> Option<&crate::app::ctx::OverlaySpec> {
    self.overlay_declaration.as_deref()
  }

  pub(crate) fn modal_declaration(&self) -> Option<&crate::app::ctx::ModalSpec> {
    self.modal_declaration.as_deref()
  }

  pub(crate) fn is_overlay_declaration(&self) -> bool {
    self.layout_neutral || self.overlay_declaration.is_some() || self.modal_declaration.is_some()
  }

  pub(crate) fn set_synthetic_role(&mut self, role: SyntheticNodeRole) {
    self.synthetic_role = Some(role);
  }

  pub(crate) fn has_synthetic_role(&self, role: SyntheticNodeRole) -> bool {
    self.synthetic_role == Some(role)
  }

  /// The open state a `Select` chevron node is drawn for, if it is one.
  pub(crate) fn select_chevron_open(&self) -> Option<bool> {
    match self.synthetic_role {
      Some(SyntheticNodeRole::SelectChevron { open }) => Some(open),
      _ => None,
    }
  }

  pub(crate) fn set_component_key(&mut self, key: Option<&str>) {
    self.component_key = key.map(Arc::from);
  }

  pub fn element_id(&self) -> Option<&str> {
    self.element_id.as_deref()
  }

  pub(crate) fn class_list(&self) -> &[Arc<str>] {
    &self.classes
  }

  pub fn has_class(&self, class: &str) -> bool {
    self.classes.iter().any(|existing| existing.as_ref() == class)
  }

  /// Appends a class, deduplicating like the DOM's `classList.add`.
  pub(crate) fn push_class(&mut self, class: Arc<str>) {
    if !self.has_class(&class) {
      self.classes.push(class);
    }
  }

  pub(crate) fn remove_class(&mut self, class: &str) {
    self.classes.retain(|existing| existing.as_ref() != class);
  }

  // The components' `child` and `with_children` move each child's node out of
  // its box here, in a frame of its own, never inlined. Moved in the generic
  // builder instead, the node (over 1 KiB) sits in that builder's frame, and an
  // element tree built recursively (`Row::with(.., items.iter().map(build))`)
  // keeps that frame on the stack once per level while the children below are
  // built: when an inlining change outlined `Row::with` and `Column::with`, it
  // cost the deep_chain probe 55 of 346 levels on a 1 MiB stack (LUR-13).

  /// `NodeUpdate::child` for a built element.
  #[inline(never)]
  pub(crate) fn push_child_element(&mut self, child: crate::node::Element) {
    NodeUpdate::child(self, child.into_node());
  }

  /// The children of a `with_children`, grown as `Vec::extend` grows them, so
  /// a build allocates as before.
  pub(crate) fn extend_child_elements(&mut self, mut children: impl Iterator<Item = crate::node::Element>) {
    self.children.reserve(children.size_hint().0);
    while let Some(child) = children.next() {
      if self.children.len() == self.children.capacity() {
        self.children.reserve(children.size_hint().0.saturating_add(1));
      }
      self.push_child_node(child);
    }
    self.layout_cache.invalidate();
  }

  #[inline(never)]
  fn push_child_node(&mut self, child: crate::node::Element) {
    self.children.push(child.into_node());
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn set_component_props_debug(&mut self, props: Option<DevtoolsInspectableDebug>) {
    self.component_props_debug = props;
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn set_component_signals_debug(&mut self, signals: Vec<ComponentSignalDebug>) {
    self.component_signals_debug = signals;
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn set_component_memos_debug(&mut self, memos: Vec<ComponentMemoDebug>) {
    self.component_memos_debug = memos;
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn set_component_effects_debug(&mut self, effects: Vec<ComponentEffectDebug>) {
    self.component_effects_debug = effects;
  }

  #[cfg(feature = "devtools")]
  pub(crate) fn set_component_contexts_debug(&mut self, contexts: Vec<ComponentContextDebug>) {
    self.component_contexts_debug = contexts;
  }

  #[cfg(any(feature = "devtools", feature = "mcp"))]
  #[cfg_attr(not(feature = "router"), allow(dead_code))]
  pub(crate) fn debug_attr(mut self, name: impl Into<Arc<str>>, value: impl Into<Arc<str>>) -> Self {
    self.debug_attrs.push((name.into(), value.into()));
    self
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub fn component_props_debug(&self) -> Option<&DevtoolsInspectableDebug> {
    self.component_props_debug.as_ref()
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub fn component_signals_debug(&self) -> &[ComponentSignalDebug] {
    &self.component_signals_debug
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub fn component_memos_debug(&self) -> &[ComponentMemoDebug] {
    &self.component_memos_debug
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub fn component_effects_debug(&self) -> &[ComponentEffectDebug] {
    &self.component_effects_debug
  }

  #[cfg(feature = "devtools")]
  #[allow(dead_code)]
  pub fn component_contexts_debug(&self) -> &[ComponentContextDebug] {
    &self.component_contexts_debug
  }

  #[cfg(any(feature = "devtools", feature = "mcp"))]
  #[allow(dead_code)]
  pub fn debug_attrs(&self) -> &[(Arc<str>, Arc<str>)] {
    &self.debug_attrs
  }

  pub(crate) fn node_kind(&self) -> &NodeKind {
    &self.node_kind
  }

  pub fn with_scroll_state(mut self, existing: crate::layout::layout_kind::ScrollState) -> Self {
    if let LayoutKind::ScrollModifier { state, .. } = &mut self.layout_kind {
      *state = existing;
    }
    self
  }

  pub fn scrollbar_style(&self, default_style: ScrollBarStyle) -> ScrollBarStyle {
    let hovered = self
      .scroll_container()
      .is_some_and(|(state, _)| state.is_thumb_hovered());
    self.scrollbar_style_when(default_style, hovered)
  }

  /// The node's scrollbar style at rest or hovered, wherever the pointer is.
  pub(crate) fn scrollbar_style_when(&self, default_style: ScrollBarStyle, hovered: bool) -> ScrollBarStyle {
    let style = self.scrollbar_style.as_deref().cloned().unwrap_or(default_style);
    match &self.scrollbar_hovered_style {
      Some(hovered_style) if hovered => hovered_style(style),
      _ => style,
    }
  }

  /// The scroll of a node that scrolls what it shows, and along which axes:
  /// a scroll container, or a multiline text input, which scrolls its own
  /// text vertically. The wheel, the scrollbar and its drags move both alike.
  pub(crate) fn scroll_container(
    &self,
  ) -> Option<(
    crate::layout::layout_kind::ScrollState,
    crate::layout::layout_kind::ScrollDirection,
  )> {
    match (&self.layout_kind, &self.node_kind) {
      (LayoutKind::ScrollModifier { state, direction, .. }, _) => Some((state.clone(), *direction)),
      (_, NodeKind::TextInput { state, .. }) => state
        .scroll_state()
        .map(|scroll| (scroll, crate::layout::layout_kind::ScrollDirection::Vertical)),
      _ => None,
    }
  }

  pub fn color(&self) -> Option<Color> {
    self.background_color().and_then(BackgroundColor::as_color)
  }

  pub(crate) fn caret_color_value(&self) -> Option<TextColor> {
    self.rare_style.caret_color.clone()
  }

  pub(crate) fn selection_color_value(&self) -> Option<TextColor> {
    self.rare_style.selection_color.clone()
  }

  pub(crate) fn caret_mode_value(&self) -> Option<CaretMode> {
    self.rare_style.caret_mode
  }

  pub(crate) fn background_color(&self) -> Option<BackgroundColor> {
    if let Some(c) = self.animation_override_color() {
      return Some(BackgroundColor::Color(c));
    }
    self
      .state_style()
      .color
      .or_else(|| <Option<BackgroundColor> as Clone>::clone(&self.color))
  }

  pub(crate) fn resolved_color(&self, palette: &crate::app::theme::ThemePalette) -> Option<Color> {
    if let Some(c) = self.animation_override_color() {
      return Some(c);
    }
    self
      .state_style()
      .color
      .or_else(|| <Option<BackgroundColor> as Clone>::clone(&self.color))
      .and_then(|color| color.resolve(palette))
  }

  /// The box shadow in effect: the innermost matching state style (active,
  /// then hovered, then focus-visible, then focused) over the node's own.
  pub(crate) fn effective_box_shadow(&self) -> Option<&BoxShadowValue> {
    fn state_shadow(applies: bool, style: &Option<Style>) -> Option<&BoxShadowValue> {
      style.as_ref().filter(|_| applies)?.box_shadow.as_deref()
    }
    let states = &*self.state_styles;
    state_shadow(self.style_state.is_drag_over(), &states.drag_over)
      .or_else(|| state_shadow(self.style_state.is_active(), &states.active))
      .or_else(|| state_shadow(self.style_state.is_hovered(), &states.hovered))
      .or_else(|| state_shadow(self.style_state.is_focus_visible(), &states.focus_visible))
      .or_else(|| state_shadow(self.style_state.is_focused(), &states.focused))
      .or(self.rare_style.box_shadow.as_ref())
  }

  pub(crate) fn resolved_gradient(&self) -> Option<Gradient> {
    self.rare_style.gradient.clone()
  }

  pub fn get_border_radius(&self, radii: &crate::app::theme::ThemeRadii) -> Option<BorderRadius> {
    let mut r = self
      .state_style()
      .border_radius
      .or(*self.border_radius)
      .map(|radius| radius.resolve(radii));
    if let Some(mut br) = r {
      let overrides = &self.animation_overrides;
      for (prop, val) in overrides.iter() {
        if let crate::animation::AnimatableValue::Float(v) = val {
          match prop {
            crate::animation::AnimatableProperty::BorderRadiusTopLeft => br.top_left = *v,
            crate::animation::AnimatableProperty::BorderRadiusTopRight => br.top_right = *v,
            crate::animation::AnimatableProperty::BorderRadiusBottomRight => br.bottom_right = *v,
            crate::animation::AnimatableProperty::BorderRadiusBottomLeft => br.bottom_left = *v,
            _ => {}
          }
        }
      }
      r = Some(br);
    }
    r
  }

  pub fn get_border(&self) -> Option<Borders> {
    let mut b = self.state_style().border.or_else(|| self.border.as_deref().cloned());
    let overrides = &self.animation_overrides;
    if let Some(ref mut borders) = b {
      for (prop, val) in overrides.iter() {
        match (prop, val) {
          (crate::animation::AnimatableProperty::BorderColor, crate::animation::AnimatableValue::Color(c)) => {
            borders.set_color(*c);
          }
          (crate::animation::AnimatableProperty::BorderWidthTop, crate::animation::AnimatableValue::Float(v)) => {
            if let Some(border) = &mut borders.top {
              border.width = BorderSizeValue::Px(*v);
            }
          }
          (crate::animation::AnimatableProperty::BorderWidthRight, crate::animation::AnimatableValue::Float(v)) => {
            if let Some(border) = &mut borders.right {
              border.width = BorderSizeValue::Px(*v);
            }
          }
          (crate::animation::AnimatableProperty::BorderWidthBottom, crate::animation::AnimatableValue::Float(v)) => {
            if let Some(border) = &mut borders.bottom {
              border.width = BorderSizeValue::Px(*v);
            }
          }
          (crate::animation::AnimatableProperty::BorderWidthLeft, crate::animation::AnimatableValue::Float(v)) => {
            if let Some(border) = &mut borders.left {
              border.width = BorderSizeValue::Px(*v);
            }
          }
          _ => {}
        }
      }
    }
    b.filter(Borders::any)
  }

  pub(crate) fn get_resolved_border(
    &self,
    palette: &crate::app::theme::ThemePalette,
    border_sizes: &crate::app::theme::ThemeBorderSizes,
  ) -> Option<crate::node::border::ResolvedBorders> {
    self
      .get_border()
      .and_then(|borders| borders.resolve_with_sizes(palette, border_sizes))
  }

  /// The node's opacity as drawn: the opacity it was built with, or a running transition or animation's.
  pub(crate) fn effective_opacity(&self) -> f32 {
    self
      .animation_override_float(crate::animation::AnimatableProperty::Opacity)
      .unwrap_or(self.opacity)
  }

  /// The value a running transition or animation sets for a float property this tick, if any.
  fn animation_override_float(&self, prop: crate::animation::AnimatableProperty) -> Option<f32> {
    self.animation_overrides.iter().find_map(|(p, val)| match val {
      crate::animation::AnimatableValue::Float(v) if *p == prop => Some(*v),
      _ => None,
    })
  }

  pub(crate) fn effective_transform(&self) -> Transform2D {
    self
      .animation_overrides
      .iter()
      .find_map(|(prop, val)| match (prop, val) {
        (crate::animation::AnimatableProperty::Transform, crate::animation::AnimatableValue::Transform(d)) => {
          Some(d.to_matrix())
        }
        _ => None,
      })
      .unwrap_or(self.transform)
  }

  pub(crate) fn hit_test_behavior(&self) -> HitTestBehavior {
    self.hit_test
  }

  fn animation_override_color(&self) -> Option<Color> {
    self
      .animation_overrides
      .iter()
      .find_map(|(prop, val)| match (prop, val) {
        (crate::animation::AnimatableProperty::BackgroundColor, crate::animation::AnimatableValue::Color(c)) => {
          Some(*c)
        }
        _ => None,
      })
  }

  pub fn children(&self) -> &[Node] {
    &self.children
  }

  pub(crate) fn element_override_rect(&self) -> Option<crate::core::ElementRect> {
    self.element_ref.as_ref().and_then(CoreElementRef::override_rect)
  }

  pub(crate) fn take_element_override_cleared(&self) -> bool {
    self.element_ref.as_ref().is_some_and(|reference| {
      self
        .layout_cache
        .take_override_cleared(reference.override_clear_generation())
    })
  }

  pub(crate) fn state_styles_affect_layout(&self) -> bool {
    self.state_styles.affects_layout()
  }

  pub(crate) fn take_style_layout_dirty(&self) -> bool {
    self.style_state.take_layout_dirty()
  }

  pub(crate) fn has_style_layout_dirty(&self) -> bool {
    self.style_state.has_layout_dirty()
  }

  pub(crate) fn has_render_dirty(&self) -> bool {
    if self.layout_cache.guards_generation.get() == self.layout_cache.watch().generation() {
      return false;
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    self.text_content.is_changed()
      || self.color.is_changed()
      || self.border_radius.is_changed()
      || self.border.is_changed()
      || self.rare_style.is_changed()
      || self.scrollbar_style.is_changed()
      || {
        #[cfg(feature = "raster")]
        {
          self.background_image.is_changed()
        }
        #[cfg(not(feature = "raster"))]
        {
          false
        }
      }
      || self.children.iter().any(Node::has_render_dirty)
  }

  pub(crate) fn is_style_hovered(&self) -> bool {
    self.style_state.is_hovered()
  }

  pub(crate) fn is_style_focused(&self) -> bool {
    self.style_state.is_focused()
  }

  pub(crate) fn is_style_focus_visible(&self) -> bool {
    self.style_state.is_focus_visible()
  }

  /// The interaction state, for what may create it (a change, a clone).
  /// Layout tracking leaves an untouched state unallocated, so the state
  /// binds this node's watch here: tracking may never come again before the
  /// change, and a state without the watch would not mark the node dirty.
  fn bound_style_state(&self) -> &InteractionState {
    self.style_state.bind_layout(self.layout_cache.watch());
    &self.style_state
  }

  pub(crate) fn set_style_hovered(&self, hovered: bool) -> bool {
    let changed = self.style_state.is_hovered() != hovered;
    if changed {
      self.bound_style_state().set_hovered(hovered);
    }
    let layout_dirty = changed && self.state_styles_affect_layout();
    if layout_dirty {
      self.style_state.mark_layout_dirty();
    }
    layout_dirty
  }

  pub(crate) fn is_style_drag_over(&self) -> bool {
    self.style_state.is_drag_over()
  }

  pub(crate) fn set_style_drag_over(&self, drag_over: bool) -> bool {
    let changed = self.style_state.is_drag_over() != drag_over;
    if changed {
      self.bound_style_state().set_drag_over(drag_over);
    }
    let layout_dirty = changed && self.state_styles_affect_layout();
    if layout_dirty {
      self.style_state.mark_layout_dirty();
    }
    layout_dirty
  }

  pub(crate) fn set_style_active(&self, active: bool) -> bool {
    let changed = self.style_state.is_active() != active;
    if changed {
      self.bound_style_state().set_active(active);
    }
    let layout_dirty = changed && self.state_styles_affect_layout();
    if layout_dirty {
      self.style_state.mark_layout_dirty();
    }
    layout_dirty
  }

  pub(crate) fn set_style_focused(&self, focused: bool) -> bool {
    let changed = self.style_state.is_focused() != focused;
    if changed {
      self.bound_style_state().set_focused(focused);
    }
    let layout_dirty = changed && self.state_styles_affect_layout();
    if layout_dirty {
      self.style_state.mark_layout_dirty();
    }
    layout_dirty
  }

  pub(crate) fn set_style_focus_visible(&self, visible: bool) -> bool {
    let changed = self.style_state.is_focus_visible() != visible;
    if changed {
      self.bound_style_state().set_focus_visible(visible);
    }
    let layout_dirty = changed && self.state_styles_affect_layout();
    if layout_dirty {
      self.style_state.mark_layout_dirty();
    }
    layout_dirty
  }

  pub(crate) fn effective_frame(&self, base: FrameConstraints) -> FrameConstraints {
    let base = merge_frame(self.frame, base);
    let mut result = self
      .state_styles
      .layout_frame(&self.style_state)
      .map_or(base, |frame| merge_frame(base, frame));
    for (prop, val) in self.animation_overrides.iter() {
      if let crate::animation::AnimatableValue::Float(v) = val {
        match prop {
          crate::animation::AnimatableProperty::Width => {
            result.width = Some(Dimension::Px(*v));
          }
          crate::animation::AnimatableProperty::Height => {
            result.height = Some(Dimension::Px(*v));
          }
          _ => {}
        }
      }
    }
    result
  }

  pub(crate) fn state_frame(&self) -> Option<FrameConstraints> {
    let frame = self.effective_frame(FrameConstraints::default());
    (frame != FrameConstraints::default()).then_some(frame)
  }

  pub(crate) fn effective_padding(&self, base: &Padding) -> Padding {
    let mut padding = self.padding;
    padding.merge_from(base);
    self.state_styles.layout_padding(&self.style_state).unwrap_or(padding)
  }

  pub(crate) fn state_flex(&self) -> Option<FlexParams> {
    self
      .state_styles
      .layout_flex(&self.style_state)
      .or(self.flex)
      .or_else(|| match &self.layout_kind {
        LayoutKind::LogicalModifier => self.children.first().and_then(|child| child.state_flex()),
        _ => None,
      })
  }

  /// The give-way order and limit of this node as a shrinking flex child.
  /// A logical wrapper without its own rule passes its child's on, the way
  /// [`Self::state_flex`] passes the child's flex factors on.
  pub(crate) fn shrink_rule(&self) -> ShrinkRule {
    if self.shrink_rule != ShrinkRule::default() {
      return self.shrink_rule;
    }
    match &self.layout_kind {
      LayoutKind::LogicalModifier => self
        .children
        .first()
        .map_or_else(ShrinkRule::default, Node::shrink_rule),
      _ => self.shrink_rule,
    }
  }

  pub(crate) fn align_self(&self) -> Option<Alignment> {
    self.align_self
  }

  pub(crate) fn position(&self) -> Position {
    self.position
  }

  /// The node's relative offset as drawn: the offset it was built with, with a running transition or animation of
  /// either axis applied.
  pub(crate) fn offset_position(&self) -> Option<Offset> {
    let mut offset = self.offset;
    if let Some(x) = self.animation_override_float(crate::animation::AnimatableProperty::OffsetX) {
      offset.get_or_insert_default().x = x;
    }
    if let Some(y) = self.animation_override_float(crate::animation::AnimatableProperty::OffsetY) {
      offset.get_or_insert_default().y = y;
    }
    offset
  }

  pub(crate) fn min_main_size(&self, vertical: bool) -> f32 {
    if let Some(frame) = self.state_frame() {
      let size = if vertical { frame.min_height } else { frame.min_width };
      if let Some(size) = size {
        return size.to_px();
      }
    }

    match &self.layout_kind {
      LayoutKind::LogicalModifier => self.children.first().map(|c| c.min_main_size(vertical)).unwrap_or(0.0),
      _ => 0.0,
    }
  }

  pub(crate) fn clear_guards(&self) {
    let subtree = self.prepare_layout_tracking();
    if self.layout_cache.guards_generation.get() == self.layout_cache.watch().generation() && !subtree.scroll {
      return;
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    self.text_content.clear_changed();
    self.color.clear_changed();
    self.border_radius.clear_changed();
    self.border.clear_changed();
    self.rare_style.clear_changed();
    #[cfg(feature = "raster")]
    self.background_image.clear_changed();
    self.scrollbar_style.clear_changed();
    // Runs once after every layout computation: the last measurement of each
    // scroll container in that pass stands.
    if let LayoutKind::ScrollModifier { state, .. } = &self.layout_kind {
      state.finish_layout_pass();
    }
    for child in &self.children {
      child.clear_guards();
    }
    self
      .layout_cache
      .guards_generation
      .set(self.layout_cache.watch().generation());
  }

  pub(crate) fn sync_dynamic_content_recursive(&mut self) {
    if !self.prepare_layout_tracking().text_input {
      return;
    }
    #[cfg(test)]
    crate::core::layout_dirty::visit();
    if let NodeKind::TextInput { state, .. } = &self.node_kind {
      state.sync_external_value();
      let rendered = state.rendered_text();
      if self.text_content.as_ref() != rendered.as_ref() {
        self.text_content.set(rendered);
      }
    }
    for child in &mut self.children {
      child.sync_dynamic_content_recursive();
    }
  }

  pub(crate) fn preserve_runtime_state_from(&mut self, old: &Node) {
    self.preserve_runtime_state_and_layout_signature_from(old);
  }

  // Return the positional subtree signature while reconciling it, so each
  // ancestor does not compare the same descendants again on a deep tree.
  fn preserve_runtime_state_and_layout_signature_from(&mut self, old: &Node) -> bool {
    // A child component in a slot's cached output: the child's own render
    // carried its state over from its own slot, or it came unchanged from
    // there. Its layout is compared with the live tree's, never with a cache.
    if old.is_component_slot_placeholder() {
      return true;
    }
    self.layout_cache.adopt_watch_from(&old.layout_cache);
    self.layout_cache.mark_watch();
    #[cfg(feature = "canvas")]
    if self.can_preserve_runtime_state_from(old)
      && let (NodeKind::Canvas { canvas }, NodeKind::Canvas { canvas: old_canvas }) =
        (&mut self.node_kind, &old.node_kind)
    {
      *canvas = old_canvas.clone();
    }

    self.clear_unchanged_guard_flags_from(old);
    let own_layout_signature_matches = self.own_layout_signature_matches(old);

    match (&self.node_kind, &old.node_kind) {
      (NodeKind::Text { state, .. }, NodeKind::Text { state: old_state, .. }) => {
        state.copy_runtime_state_from(
          old_state,
          self.text_content().unwrap_or_default(),
          own_layout_signature_matches,
        );
      }
      #[cfg(feature = "markdown")]
      (NodeKind::RichText { state, .. }, NodeKind::RichText { state: old_state, .. }) => {
        state.copy_runtime_state_from(
          old_state,
          &self.rich_text_value().unwrap_or_default(),
          own_layout_signature_matches,
        );
      }
      (NodeKind::TextInput { state, .. }, NodeKind::TextInput { state: old_state, .. }) => {
        state.copy_runtime_state_from(old_state, own_layout_signature_matches);
      }
      (NodeKind::Select { state, .. }, NodeKind::Select { state: old_state, .. }) => {
        state.copy_runtime_state_from(old_state);
        if self.element_ref.is_none() {
          self.element_ref = old.element_ref.clone();
        }
      }
      (NodeKind::Slider { state }, NodeKind::Slider { state: old_state }) => {
        state.copy_runtime_state_from(old_state);
      }
      _ => {}
    }

    if let (
      LayoutKind::ScrollModifier { state, direction, .. },
      LayoutKind::ScrollModifier {
        state: old_state,
        direction: old_direction,
        ..
      },
    ) = (&mut self.layout_kind, &old.layout_kind)
    {
      if direction == old_direction {
        *state = old_state.clone();
      }
    }

    let positional = self
      .children
      .iter()
      .zip(&old.children)
      .all(|(child, old_child)| child.can_preserve_runtime_state_from(old_child));
    let mut child_layout_signatures_match = true;
    if positional {
      for (child, old_child) in self.children.iter_mut().zip(&old.children) {
        child_layout_signatures_match &= child.preserve_runtime_state_and_layout_signature_from(old_child);
      }
    } else {
      // Reordered children preserve state by identity, but cache signatures
      // still describe their positions. Compare those before copying state.
      child_layout_signatures_match = self.child_layout_signatures_match(old);
      let mut claimed = vec![false; old.children.len()];
      for index in 0..self.children.len() {
        let child = &self.children[index];
        let matched =
          old
            .children
            .get(index)
            .filter(|old_child| !claimed[index] && child.can_preserve_runtime_state_from(old_child))
            .map(|_| index)
            .or_else(|| {
              old.children.iter().enumerate().find_map(|(offset, old_child)| {
                (!claimed[offset] && child.identity_matches(old_child)).then_some(offset)
              })
            });
        if let Some(offset) = matched {
          claimed[offset] = true;
          self.children[index].preserve_runtime_state_from(&old.children[offset]);
        }
      }
    }

    let own_layout_matches = own_layout_signature_matches && self.children.len() == old.children.len();
    if own_layout_matches {
      self.layout_cache.preserve_from(&old.layout_cache);
      if !child_layout_signatures_match {
        self.layout_cache.mark_descendant_dirty();
      }
    }
    own_layout_matches && child_layout_signatures_match
  }

  fn can_preserve_runtime_state_from(&self, old: &Node) -> bool {
    if old.is_component_slot_placeholder() {
      return self.component_slot_id == old.component_slot_id;
    }
    // A canvas can keep its backing surface when its lookup id or ref binding
    // changes. Input editing state and an open select menu, however, belong
    // to the identified field.
    if matches!(self.node_kind, NodeKind::TextInput { .. } | NodeKind::Select { .. }) {
      return self.can_reuse_id_from(old);
    }
    std::mem::discriminant(&self.node_kind) == std::mem::discriminant(&old.node_kind)
      && std::mem::discriminant(&self.layout_kind) == std::mem::discriminant(&old.layout_kind)
      && self.component_slot_id == old.component_slot_id
      && self.component_key == old.component_key
  }

  fn clear_unchanged_guard_flags_from(&self, old: &Node) {
    if self.text_content.as_ref() == old.text_content.as_ref() {
      self.text_content.clear_changed();
    }
    if self.color.as_ref() == old.color.as_ref() {
      self.color.clear_changed();
    }
    if self.border_radius.as_ref() == old.border_radius.as_ref() {
      self.border_radius.clear_changed();
    }
    if self.border.as_ref() == old.border.as_ref() {
      self.border.clear_changed();
    }
    if **self.rare_style == **old.rare_style {
      self.rare_style.clear_changed();
    }
  }

  fn layout_signature_matches(&self, old: &Node) -> bool {
    if old.is_component_slot_placeholder() {
      return self.component_slot_id == old.component_slot_id;
    }
    self.own_layout_signature_matches(old)
      && self.children.len() == old.children.len()
      && self.child_layout_signatures_match(old)
  }

  fn child_layout_signatures_match(&self, old: &Node) -> bool {
    self
      .children
      .iter()
      .zip(old.children.iter())
      .all(|(child, old_child)| child.layout_signature_matches(old_child))
  }

  fn own_layout_signature_matches(&self, old: &Node) -> bool {
    #[cfg(test)]
    tests::LAYOUT_SIGNATURE_COMPARISONS.with(|count| count.set(count.get() + 1));
    self.component_slot_id == old.component_slot_id
      && self.component_key == old.component_key
      && self.layout_kind_matches_for_cache(old)
      && self.node_kind_matches_for_cache(old)
      && self.frame == old.frame
      && self.padding == old.padding
      && self.position == old.position
      && self.offset == old.offset
      && self.align_self == old.align_self
      && self.flex == old.flex
      && self.shrink_rule == old.shrink_rule
      && self.text_content.as_ref() == old.text_content.as_ref()
      && self.text_wrap == old.text_wrap
      && self.text_overflow == old.text_overflow
      && self.overflow == old.overflow
      && self.intrinsic_size == old.intrinsic_size
      && !self.holds_overrides_beyond_opacity()
      && !old.holds_overrides_beyond_opacity()
  }

  /// Whether a running or forwards-filled transition or animation sets anything but opacity. Opacity is drawn
  /// over the laid-out geometry and never shapes it, so a node fading in, or holding a faded pose, keeps its
  /// layout cache across rebuilds; any other override still makes it lay out again.
  fn holds_overrides_beyond_opacity(&self) -> bool {
    self
      .animation_overrides
      .iter()
      .any(|(prop, _)| *prop != crate::animation::AnimatableProperty::Opacity)
  }

  fn layout_kind_matches_for_cache(&self, old: &Node) -> bool {
    match (&self.layout_kind, &old.layout_kind) {
      (LayoutKind::Leaf, LayoutKind::Leaf) => true,
      (
        LayoutKind::Row {
          spacing,
          align,
          justify,
          wrap,
        },
        LayoutKind::Row {
          spacing: old_spacing,
          align: old_align,
          justify: old_justify,
          wrap: old_wrap,
        },
      )
      | (
        LayoutKind::Column {
          spacing,
          align,
          justify,
          wrap,
        },
        LayoutKind::Column {
          spacing: old_spacing,
          align: old_align,
          justify: old_justify,
          wrap: old_wrap,
        },
      ) => spacing == old_spacing && align == old_align && justify == old_justify && wrap == old_wrap,
      (LayoutKind::Stack { align }, LayoutKind::Stack { align: old_align }) => align == old_align,
      (LayoutKind::LogicalModifier, LayoutKind::LogicalModifier) => true,
      (
        LayoutKind::ScrollModifier { direction, .. },
        LayoutKind::ScrollModifier {
          direction: old_direction,
          ..
        },
      ) => direction == old_direction,
      _ => false,
    }
  }

  fn node_kind_matches_for_cache(&self, old: &Node) -> bool {
    match (&self.node_kind, &old.node_kind) {
      (NodeKind::Empty, NodeKind::Empty) => true,
      (
        NodeKind::Text {
          state,
          style,
          transform_mode,
        },
        NodeKind::Text {
          state: old_state,
          style: old_style,
          transform_mode: old_transform_mode,
        },
      ) => {
        style == old_style
          && state.selectable() == old_state.selectable()
          && state.measures_carets() == old_state.measures_carets()
          && state.is_sensitive() == old_state.is_sensitive()
          && transform_mode == old_transform_mode
      }
      #[cfg(feature = "markdown")]
      (
        NodeKind::RichText {
          state,
          spans,
          transform_mode,
          generated,
        },
        NodeKind::RichText {
          state: old_state,
          spans: old_spans,
          transform_mode: old_transform_mode,
          generated: old_generated,
        },
      ) => {
        spans == old_spans
          && state.selectable() == old_state.selectable()
          && state.measures_carets() == old_state.measures_carets()
          && transform_mode == old_transform_mode
          && generated == old_generated
      }
      #[cfg(feature = "markdown")]
      (NodeKind::InlineLink { link }, NodeKind::InlineLink { link: old_link }) => {
        link.label == old_link.label && link.target == old_link.target
      }
      (
        NodeKind::TextInput {
          state,
          style,
          placeholder_style,
        },
        NodeKind::TextInput {
          state: old_state,
          style: old_style,
          placeholder_style: old_placeholder_style,
        },
      ) => {
        style == old_style
          && placeholder_style == old_placeholder_style
          && state.layout_signature() == old_state.layout_signature()
      }
      (NodeKind::Checkbox { state }, NodeKind::Checkbox { state: old_state }) => {
        state.layout_signature() == old_state.layout_signature()
      }
      (NodeKind::Slider { state }, NodeKind::Slider { state: old_state }) => {
        state.layout_signature() == old_state.layout_signature()
      }
      #[cfg(feature = "raster")]
      (
        NodeKind::Image { data, fit },
        NodeKind::Image {
          data: old_data,
          fit: old_fit,
        },
      ) => data.id() == old_data.id() && fit == old_fit,
      #[cfg(feature = "image")]
      (NodeKind::LoadingImage { request }, NodeKind::LoadingImage { request: old }) => {
        request.key == old.key && request.cap == old.cap && request.fit == old.fit
      }
      #[cfg(feature = "raster")]
      (
        NodeKind::Video { data, fit },
        NodeKind::Video {
          data: old_data,
          fit: old_fit,
        },
      ) => data.id() == old_data.id() && fit == old_fit,
      #[cfg(feature = "image")]
      (NodeKind::ResourceImage { path }, NodeKind::ResourceImage { path: old_path }) => path == old_path,
      #[cfg(feature = "svg")]
      (NodeKind::Svg { data }, NodeKind::Svg { data: old_data }) => data.id() == old_data.id(),
      #[cfg(all(feature = "svg", feature = "resources"))]
      (NodeKind::ResourceSvg { path }, NodeKind::ResourceSvg { path: old_path }) => path == old_path,
      _ => false,
    }
  }

  pub(crate) fn preserve_ids_from(&mut self, old: &mut Node) {
    if self.can_reuse_id_from(old) && old.node_id.is_assigned() {
      self.node_id = old.node_id;
      old.node_id = NodeId::UNASSIGNED;
    }

    // Keyed/slotted children can move between renders (a windowed or reordered
    // list). Match them by identity so node ids follow the element. Unchanged
    // positions need no claimed-child buffer, even for identified children.
    let reorderable = self.children.iter().any(Node::has_stable_identity);
    let positional = !reorderable
      || self
        .children
        .iter()
        .zip(&old.children)
        .all(|(child, old_child)| child.can_reuse_id_from(old_child));
    if positional {
      for (child, old_child) in self.children.iter_mut().zip(old.children.iter_mut()) {
        child.preserve_ids_from(old_child);
      }
      return;
    }

    let mut claimed = vec![false; old.children.len()];
    for (index, child) in self.children.iter_mut().enumerate() {
      let matched = old
        .children
        .get(index)
        .filter(|old_child| !claimed[index] && child.can_reuse_id_from(old_child))
        .map(|_| index)
        .or_else(|| {
          old
            .children
            .iter()
            .enumerate()
            .find_map(|(offset, old_child)| (!claimed[offset] && child.identity_matches(old_child)).then_some(offset))
        });
      if let Some(offset) = matched {
        claimed[offset] = true;
        child.preserve_ids_from(&mut old.children[offset]);
      }
    }
  }

  fn can_reuse_id_from(&self, old: &Node) -> bool {
    self.component_slot_id == old.component_slot_id
      && self.component_key == old.component_key
      && self.element_id == old.element_id
      && std::mem::discriminant(&self.node_kind) == std::mem::discriminant(&old.node_kind)
      && std::mem::discriminant(&self.layout_kind) == std::mem::discriminant(&old.layout_kind)
      && (self.component_slot_id.is_some()
        || self.component_key.is_some()
        || self.element_id.is_some()
        || match (&self.node_kind, &old.node_kind) {
          (NodeKind::TextInput { state, .. }, NodeKind::TextInput { state: previous, .. }) => {
            state.same_value(previous)
          }
          (NodeKind::Select { state }, NodeKind::Select { state: previous })
            if state.has_binding() || previous.has_binding() =>
          {
            state.same_binding(previous) && refs_compatible(self.element_ref.as_ref(), old.element_ref.as_ref())
          }
          _ => refs_compatible(self.element_ref.as_ref(), old.element_ref.as_ref()),
        })
  }

  fn has_stable_identity(&self) -> bool {
    self.component_slot_id.is_some()
      || self.component_key.is_some()
      || self.element_id.is_some()
      || self.element_ref.is_some()
      || matches!(self.node_kind, NodeKind::TextInput { .. })
      || matches!(&self.node_kind, NodeKind::Select { state } if state.has_binding())
  }

  /// Match explicit keys/slots/IDs, retained refs, or input/select value signals across
  /// a reorder. Anonymous nodes still use positional pairing.
  fn identity_matches(&self, old: &Node) -> bool {
    self.has_stable_identity() && old.has_stable_identity() && self.can_reuse_id_from(old)
  }

  /// A plain loop rather than `iter().map(..).collect()`: in an unoptimized
  /// build every iterator adapter between the recursive calls would add a
  /// frame holding a `Node` for each tree level.
  fn clone_children_for_reuse(&self) -> Vec<Node> {
    let mut children = Vec::with_capacity(self.children.len());
    for child in &self.children {
      children.push(child.clone_for_reuse());
    }
    children
  }

  pub(crate) fn clone_for_reuse(&self) -> Self {
    self.clone_around(
      self
        .overlay_declaration
        .as_ref()
        .map(|spec| Box::new(spec.clone_for_reuse())),
      self
        .modal_declaration
        .as_ref()
        .map(|spec| Box::new(spec.clone_for_reuse())),
      self.clone_children_for_reuse(),
    )
  }

  /// [`Self::clone_for_reuse`] of this node alone, around declarations and
  /// children the caller cloned: a component's cached output clones them its
  /// own way (`node/component_output.rs`).
  pub(crate) fn clone_around(
    &self,
    overlay_declaration: Option<Box<crate::app::ctx::OverlaySpec>>,
    modal_declaration: Option<Box<crate::app::ctx::ModalSpec>>,
    children: Vec<Node>,
  ) -> Self {
    Self {
      node_id: NodeId::UNASSIGNED,
      tag_name: self.tag_name.clone(),
      component_slot_id: self.component_slot_id,
      component_key: self.component_key.clone(),
      element_id: self.element_id.clone(),
      classes: self.classes.clone(),
      outer_decoration: self.outer_decoration.clone(),
      overlay_declaration,
      dropdown_anchor: self.dropdown_anchor.clone(),
      modal_declaration,
      layout_neutral: self.layout_neutral,
      synthetic_role: self.synthetic_role,
      #[cfg(feature = "devtools")]
      component_props_debug: self.component_props_debug.clone(),
      #[cfg(feature = "devtools")]
      component_signals_debug: self.component_signals_debug.clone(),
      #[cfg(feature = "devtools")]
      component_memos_debug: self.component_memos_debug.clone(),
      #[cfg(feature = "devtools")]
      component_effects_debug: self.component_effects_debug.clone(),
      #[cfg(feature = "devtools")]
      component_contexts_debug: self.component_contexts_debug.clone(),
      #[cfg(any(feature = "devtools", feature = "mcp"))]
      debug_attrs: self.debug_attrs.clone(),
      layout_kind: self.layout_kind.clone(),
      frame: self.frame,
      padding: self.padding,
      position: self.position,
      offset: self.offset,
      align_self: self.align_self,
      flex: self.flex,
      shrink_rule: self.shrink_rule,
      node_kind: {
        #[cfg(feature = "canvas")]
        if let NodeKind::Canvas { canvas } = &self.node_kind {
          NodeKind::Canvas {
            canvas: canvas.clone_empty(),
          }
        } else {
          self.node_kind.clone()
        }
        #[cfg(not(feature = "canvas"))]
        {
          self.node_kind.clone()
        }
      },
      text_content: self.text_content.clone(),
      text_wrap: self.text_wrap,
      text_overflow: self.text_overflow,
      overflow: self.overflow,
      intrinsic_size: self.intrinsic_size,
      color: self.color.clone(),
      border_radius: self.border_radius.clone(),
      border: self.border.clone(),
      rare_style: self.rare_style.clone(),
      cursor: self.cursor,
      hit_test: self.hit_test,
      #[cfg(feature = "raster")]
      background_image: self.background_image.clone(),
      #[cfg(feature = "raster")]
      background_size: self.background_size,
      #[cfg(all(feature = "image", feature = "resources"))]
      background_resource_image: self.background_resource_image.clone(),
      scrollbar_style: self.scrollbar_style.clone(),
      scrollbar_hovered_style: self.scrollbar_hovered_style.clone(),
      element_ref: self.element_ref.clone(),
      drag_payload: self.drag_payload.clone(),
      interaction: self.interaction.clone(),
      focusable: self.focusable,
      tab_index: self.tab_index,
      button_kind: self.button_kind,
      #[cfg(feature = "form")]
      form_name: self.form_name.clone(),
      style_state: self.bound_style_state().clone(),
      state_styles: self.state_styles.clone(),
      opacity: self.opacity,
      transform: self.transform,
      animation_overrides: LazyBox::new(),
      transitions: self.transitions.clone(),
      animation: self.animation.clone(),
      layout_cache: Default::default(),
      children,
      events: self.events.clone(),
    }
  }

  /// [`Self::clone_for_reuse`] onto the heap. The clone is built in this
  /// frame, so callers on the component render path (which recurses once per
  /// component level) never hold a `Node` temporary in their own frames.
  pub(crate) fn clone_boxed(&self) -> Box<Node> {
    Box::new(self.clone_for_reuse())
  }

  /// This node and all its descendants. Iterative, so a deep tree cannot
  /// overflow the stack.
  #[cfg(feature = "perf_profile")]
  pub(crate) fn subtree_len(&self) -> usize {
    let mut count = 0;
    let mut pending = vec![self];
    while let Some(node) = pending.pop() {
      count += 1;
      pending.extend(node.children.iter());
    }
    count
  }

  pub(crate) fn estimated_memory_bytes(&self) -> usize {
    std::mem::size_of::<Self>()
      + self.tag_name.len()
      + self.estimated_id_class_heap_bytes()
      + self.estimated_debug_memory_bytes()
      + self.text_content.as_ref().map(|text| text.capacity()).unwrap_or(0)
      + self.children.capacity() * std::mem::size_of::<Node>()
      + self.layout_cache.estimated_memory_bytes()
      + self
        .children
        .iter()
        .map(Node::estimated_child_heap_bytes)
        .sum::<usize>()
  }

  #[cfg(feature = "devtools")]
  fn estimated_debug_memory_bytes(&self) -> usize {
    self
      .component_props_debug
      .as_ref()
      .map(|props| {
        props.type_name.len()
          + props.fields.capacity() * std::mem::size_of::<crate::app::component::ComponentInfo>()
          + props
            .fields
            .iter()
            .map(|field| field.estimated_memory_bytes())
            .sum::<usize>()
      })
      .unwrap_or(0)
      + self
        .component_signals_debug
        .iter()
        .map(|signal| signal.type_name.len() + signal.estimated_memory_bytes())
        .sum::<usize>()
      + self
        .component_memos_debug
        .iter()
        .map(|memo| memo.type_name.len() + memo.estimated_memory_bytes())
        .sum::<usize>()
      + self.component_effects_debug.capacity() * std::mem::size_of::<ComponentEffectDebug>()
      + self
        .component_contexts_debug
        .iter()
        .map(|context| context.type_name.len())
        .sum::<usize>()
      + self
        .debug_attrs
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum::<usize>()
      + self.debug_attrs.capacity() * std::mem::size_of::<(Arc<str>, Arc<str>)>()
  }

  #[cfg(not(feature = "devtools"))]
  fn estimated_debug_memory_bytes(&self) -> usize {
    0
  }

  fn estimated_id_class_heap_bytes(&self) -> usize {
    self.element_id.as_ref().map(|id| id.len()).unwrap_or(0)
      + self.classes.iter().map(|class| class.len()).sum::<usize>()
      + self.classes.capacity() * std::mem::size_of::<Arc<str>>()
  }

  fn estimated_child_heap_bytes(&self) -> usize {
    self.tag_name.len()
      + self.estimated_id_class_heap_bytes()
      + self.estimated_debug_memory_bytes()
      + self.text_content.as_ref().map(|text| text.capacity()).unwrap_or(0)
      + self.children.capacity() * std::mem::size_of::<Node>()
      + self.layout_cache.estimated_memory_bytes()
      + self
        .children
        .iter()
        .map(Node::estimated_child_heap_bytes)
        .sum::<usize>()
  }

  pub(crate) fn target_style(&self) -> Style {
    self.state_style()
  }

  pub(crate) fn state_style(&self) -> Style {
    let mut style = Style::new();
    if self.style_state.is_focused() {
      if let Some(focused) = &self.state_styles.focused {
        style.merge_from(focused);
      }
    }
    if self.style_state.is_focus_visible()
      && let Some(focus_visible) = &self.state_styles.focus_visible
    {
      style.merge_from(focus_visible);
    }
    if self.style_state.is_hovered() {
      if let Some(hovered) = &self.state_styles.hovered {
        style.merge_from(hovered);
      }
    }
    if self.style_state.is_active() {
      if let Some(active) = &self.state_styles.active {
        style.merge_from(active);
      }
    }
    if self.style_state.is_drag_over()
      && let Some(drag_over) = &self.state_styles.drag_over
    {
      style.merge_from(drag_over);
    }
    style
  }
}

pub(crate) fn merge_frame(mut base: FrameConstraints, overlay: FrameConstraints) -> FrameConstraints {
  if overlay.width.is_some() {
    base.width = overlay.width;
  }
  if overlay.height.is_some() {
    base.height = overlay.height;
  }
  if overlay.min_width.is_some() {
    base.min_width = overlay.min_width;
  }
  if overlay.max_width.is_some() {
    base.max_width = overlay.max_width;
  }
  if overlay.min_height.is_some() {
    base.min_height = overlay.min_height;
  }
  if overlay.max_height.is_some() {
    base.max_height = overlay.max_height;
  }
  base
}

/// Two retained refs name the same element only when they are one handle; a
/// missing ref on either side does not tell the elements apart.
fn refs_compatible(current: Option<&CoreElementRef>, previous: Option<&CoreElementRef>) -> bool {
  match (current, previous) {
    (Some(current), Some(previous)) => current.same_handle(previous),
    _ => true,
  }
}

#[cfg(test)]
mod tests {
  use super::{Node, NodeKind, NodeUpdate};
  use crate::{
    core::Signal,
    layout::{
      Alignment, Constraints, Offset, Size,
      layout_result::{ChildLayout, LayoutResult, SubtreeHolds},
    },
  };

  #[test]
  fn built_in_tag_names_share_storage() {
    use std::sync::Arc;

    use crate::components::{Column, Rect, Row};

    let a = Node::new();
    let b = Node::new();
    assert_eq!(a.tag_name(), "Node");
    assert!(Arc::ptr_eq(&a.tag_name, &b.tag_name));
    for (a, b, name) in [
      (Row::new().node, Row::new().node, "Row"),
      (Column::new().node, Column::new().node, "Column"),
      (Rect::new(1.0, 1.0).node, Rect::new(2.0, 2.0).node, "Rect"),
    ] {
      assert_eq!(a.tag_name(), name);
      assert!(Arc::ptr_eq(&a.tag_name, &b.tag_name));
    }
  }

  thread_local! {
    pub(super) static LAYOUT_SIGNATURE_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
  }

  #[test]
  fn deep_set_root_compares_each_layout_signature_once() {
    use crate::{
      app::Tree,
      components::{Column, Rect, Row},
      node::Element,
    };

    fn chain(depth: usize) -> Element {
      let mut element = Rect::new(10.0, 10.0).into();
      for index in 0..depth {
        element = if index % 2 == 0 {
          Row::with(0.0, Alignment::Start, [element]).into()
        } else {
          Column::with(0.0, Alignment::Start, [element]).into()
        };
      }
      element
    }

    for depth in [16, 64] {
      let mut tree = Tree::new();
      tree.set_root(chain(depth));
      LAYOUT_SIGNATURE_COMPARISONS.set(0);
      tree.set_root(chain(depth));
      assert_eq!(LAYOUT_SIGNATURE_COMPARISONS.get(), depth + 1);
    }
  }

  #[test]
  fn deep_reconcile_marks_every_ancestor_of_changed_leaf() {
    fn chain(leaf_width: f32) -> Node {
      let mut node = Node::new().size(leaf_width, 10.0);
      for _ in 0..16 {
        node = Node::column(0.0, Alignment::Start, vec![node]);
        node.layout_cache.store(
          Constraints::loose(Size::new(400.0, 400.0)),
          LayoutResult {
            size: Size::new(10.0, 10.0),
            children: Vec::new().into(),
            text_layout: None,
            dropped: false,
            holds: SubtreeHolds::UNKNOWN,
            generation: 0,
          },
        );
      }
      node
    }

    for width in [10.0, 20.0] {
      // Reconciling takes the old tree's entries, so each round needs its own.
      let old = chain(10.0);
      let mut new = chain(width);
      new.preserve_runtime_state_from(&old);
      let mut node = &new;
      while let Some(child) = node.children.first() {
        assert!(node.layout_cache.has_cached_result());
        assert_eq!(node.layout_cache.is_descendant_dirty(), width != 10.0);
        node = child;
      }
      assert_eq!(node.frame.width, Some(crate::node::dimension::Dimension::Px(width)));
    }
  }

  /// A built tree makes no watches. A rebuilt node takes over the watch of
  /// the node it carries its state over from, marked, and apart from the old
  /// parent until tracking attaches it to its new one, as a new watch would
  /// be: a rebuilt tree's first layout made a watch per node.
  #[test]
  fn rebuilt_nodes_take_over_the_old_nodes_watches() {
    use std::sync::Arc;

    use crate::{
      components::{Column, Rect},
      node::Element,
    };

    fn tree() -> Node {
      Element::from(Column::with(0.0, Alignment::Start, [Rect::new(10.0, 10.0)])).into_node()
    }

    let mut new = tree();
    assert!(!new.layout_cache.has_watch());
    assert!(!new.children[0].layout_cache.has_watch());

    let old = tree();
    old.prepare_layout_tracking();
    let old_root = old.layout_cache.watch().clone();
    let old_child = old.children[0].layout_cache.watch().clone();
    let tracked = old_root.generation();

    new.preserve_runtime_state_from(&old);
    assert!(Arc::ptr_eq(new.layout_cache.watch(), &old_root));
    assert!(Arc::ptr_eq(new.children[0].layout_cache.watch(), &old_child));
    assert!(old_root.generation() > tracked, "a watch taken over is marked");
    assert!(new.layout_cache.needs_dirty_scan());

    let detached = old_root.generation();
    old_child.mark();
    assert_eq!(old_root.generation(), detached, "the child left the old parent");
    new.prepare_layout_tracking();
    let attached = old_root.generation();
    assert!(attached > detached, "attaching the child notifies its new parent");
    old_child.mark();
    assert!(old_root.generation() > attached, "the child notifies its new parent");
  }

  #[test]
  fn layout_state_fields_follow_style_precedence_and_updates() {
    use crate::{
      layout::layout_kind::{FlexParams, FrameConstraints},
      node::{Style, dimension::Dimension, padding::Padding},
    };

    let mut node = Node::column(0.0, Alignment::Start, vec![])
      .width(10.0)
      .height(20.0)
      .padding(1.0)
      .flex(1.0)
      .focused_style(Style::new().width(30.0).padding(2.0).flex(2.0))
      .focus_visible_style(Style::new().height(40.0))
      .hovered_style(Style::new().padding_left(3.0))
      .active_style(Style::new().flex(4.0))
      .drag_over_style(Style::new().width(50.0).padding(5.0).flex(5.0));
    for bits in 0..32 {
      node.set_style_focused(bits & 1 != 0);
      node.set_style_focus_visible(bits & 2 != 0);
      node.set_style_hovered(bits & 4 != 0);
      node.set_style_active(bits & 8 != 0);
      node.set_style_drag_over(bits & 16 != 0);
      let style = node.state_style();
      let frame = style
        .frame
        .map_or(node.frame, |frame| super::merge_frame(node.frame, frame));
      for _ in 0..2 {
        assert!(node.effective_frame(FrameConstraints::default()) == frame);
        assert!(node.state_frame() == Some(frame));
        assert_eq!(
          node.effective_padding(&Padding::default()),
          style.padding.unwrap_or(node.padding)
        );
        assert!(node.state_flex() == style.flex.or(node.flex));
      }
    }

    // A shared interaction handle can change without going through this node.
    let cloned = node.clone_for_reuse();
    cloned.set_style_drag_over(false);
    assert!(node.state_flex() == Some(FlexParams::grow(4.0)));
    assert_eq!(node.state_frame().unwrap().width, Some(Dimension::Px(10.0)));

    // Replacing an applied style must refresh the cached field sources too.
    NodeUpdate::active_style(&mut node, Style::new().height(70.0));
    assert!(node.state_flex() == Some(FlexParams::grow(2.0)));
    assert_eq!(node.state_frame().unwrap().height, Some(Dimension::Px(70.0)));
    let node = node.active_style(Style::new().padding(8.0));
    assert_eq!(node.effective_padding(&Padding::default()), Padding::all(8.0));
    assert_eq!(node.state_frame().unwrap().height, Some(Dimension::Px(40.0)));
  }

  #[test]
  fn text_input_mask_builders_render_bullets_and_allow_overrides() {
    let value = Signal::new("abc".to_owned());
    let mut updated = Node::text_input(value.clone());
    NodeUpdate::text_input_mask(&mut updated);

    for node in [Node::text_input(value.clone()).text_input_mask(), updated] {
      let NodeKind::TextInput { state, .. } = &node.node_kind else {
        panic!("expected text input");
      };
      assert_eq!(state.rendered_text_for_layout(), "•••");

      let node = node.text_input_mask_char('*');
      let NodeKind::TextInput { state, .. } = &node.node_kind else {
        panic!("expected text input");
      };
      assert_eq!(state.rendered_text_for_layout(), "***");

      let node = node.text_input_unmask();
      let NodeKind::TextInput { state, .. } = &node.node_kind else {
        panic!("expected text input");
      };
      assert_eq!(state.rendered_text_for_layout(), "abc");
    }
    assert_eq!(value.get(), "abc");
  }

  #[test]
  fn changed_text_content_does_not_match_layout_cache_signature() {
    let old = Node::text("Hi");
    let new = Node::text("Vitayu vitayu vitayu");

    assert!(!new.layout_signature_matches(&old));
  }

  #[test]
  fn unchanged_text_content_matches_layout_cache_signature() {
    let old = Node::text("Hi");
    let new = Node::text("Hi");

    assert!(new.layout_signature_matches(&old));
  }

  #[test]
  fn changed_component_key_does_not_match_layout_cache_signature() {
    let mut old = Node::text("Hi");
    old.set_component_slot_id(1);
    old.set_component_key(Some("message-1"));
    let mut new = Node::text("Hi");
    new.set_component_slot_id(2);
    new.set_component_key(Some("message-2"));

    assert!(!new.layout_signature_matches(&old));
  }

  #[test]
  fn opacity_override_keeps_layout_cache_signature() {
    use crate::animation::{AnimatableProperty, AnimatableValue};

    let mut old = Node::text("Hi");
    old
      .animation_overrides
      .push((AnimatableProperty::Opacity, AnimatableValue::Float(0.4)));
    let new = Node::text("Hi");

    assert!(new.layout_signature_matches(&old));
  }

  #[test]
  fn offset_override_does_not_match_layout_cache_signature() {
    use crate::animation::{AnimatableProperty, AnimatableValue};

    let mut old = Node::text("Hi");
    old
      .animation_overrides
      .push((AnimatableProperty::OffsetX, AnimatableValue::Float(40.0)));
    let new = Node::text("Hi");

    assert!(!new.layout_signature_matches(&old));
  }

  #[test]
  fn changed_slider_value_matches_layout_cache_signature() {
    let old_value = Signal::new(10);
    let new_value = Signal::new(20);
    let old = Node::slider(old_value);
    let new = Node::slider(new_value);

    assert!(new.layout_signature_matches(&old));
  }

  #[test]
  fn parent_layout_cache_survives_changed_child_text() {
    let old = Node::row(
      0.0,
      Alignment::Start,
      vec![Node::text("Value: 1"), Node::text("Stable")],
    );
    old.layout_cache.store(
      Constraints::loose(Size::new(400.0, 400.0)),
      LayoutResult {
        size: Size::new(100.0, 20.0),
        children: vec![
          ChildLayout {
            offset: Offset::default(),
            result: LayoutResult {
              size: Size::new(50.0, 20.0),
              children: Vec::new().into(),
              text_layout: None,
              dropped: false,
              holds: SubtreeHolds::UNKNOWN,
              generation: 0,
            }
            .into(),
          },
          ChildLayout {
            offset: Offset::new(50.0, 0.0),
            result: LayoutResult {
              size: Size::new(50.0, 20.0),
              children: Vec::new().into(),
              text_layout: None,
              dropped: false,
              holds: SubtreeHolds::UNKNOWN,
              generation: 0,
            }
            .into(),
          },
        ]
        .into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      },
    );
    let mut new = Node::row(
      0.0,
      Alignment::Start,
      vec![Node::text("Value: 2"), Node::text("Stable")],
    );

    new.preserve_runtime_state_from(&old);

    assert!(new.layout_cache.has_cached_result());
    assert!(!new.children[0].layout_cache.has_cached_result());
  }

  #[test]
  fn preserved_parent_cache_is_dirty_when_child_signature_changes() {
    let old = Node::column(
      0.0,
      Alignment::Start,
      vec![Node::row(0.0, Alignment::Start, vec![Node::text("Old")])],
    );
    old.layout_cache.store(
      Constraints::loose(Size::new(400.0, 400.0)),
      LayoutResult {
        size: Size::new(100.0, 20.0),
        children: vec![ChildLayout {
          offset: Offset::default(),
          result: LayoutResult {
            size: Size::new(100.0, 20.0),
            children: vec![ChildLayout {
              offset: Offset::default(),
              result: LayoutResult {
                size: Size::new(30.0, 20.0),
                children: Vec::new().into(),
                text_layout: None,
                dropped: false,
                holds: SubtreeHolds::UNKNOWN,
                generation: 0,
              }
              .into(),
            }]
            .into(),
            text_layout: None,
            dropped: false,
            holds: SubtreeHolds::UNKNOWN,
            generation: 0,
          }
          .into(),
        }]
        .into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      },
    );

    let mut new = Node::column(
      0.0,
      Alignment::Start,
      vec![Node::row(0.0, Alignment::Start, vec![Node::text("Different")])],
    );

    new.preserve_runtime_state_from(&old);

    assert!(new.layout_cache.has_cached_result());
    assert!(new.layout_cache.is_descendant_dirty());
  }

  #[test]
  fn preserved_parent_cache_is_dirty_when_child_component_key_changes() {
    let mut old_child = Node::row(0.0, Alignment::Start, vec![Node::text("Same text")]);
    old_child.set_component_slot_id(1);
    old_child.set_component_key(Some("message-1"));
    let old = Node::column(0.0, Alignment::Start, vec![old_child]);
    old.layout_cache.store(
      Constraints::loose(Size::new(400.0, 400.0)),
      LayoutResult {
        size: Size::new(100.0, 20.0),
        children: vec![ChildLayout {
          offset: Offset::default(),
          result: LayoutResult {
            size: Size::new(100.0, 20.0),
            children: Vec::new().into(),
            text_layout: None,
            dropped: false,
            holds: SubtreeHolds::UNKNOWN,
            generation: 0,
          }
          .into(),
        }]
        .into(),
        text_layout: None,
        dropped: false,
        holds: SubtreeHolds::UNKNOWN,
        generation: 0,
      },
    );

    let mut new_child = Node::row(0.0, Alignment::Start, vec![Node::text("Same text")]);
    new_child.set_component_slot_id(2);
    new_child.set_component_key(Some("message-2"));
    let mut new = Node::column(0.0, Alignment::Start, vec![new_child]);

    new.preserve_runtime_state_from(&old);

    assert!(new.layout_cache.has_cached_result());
    assert!(new.layout_cache.is_descendant_dirty());
  }

  #[test]
  fn preserve_ids_follows_keyed_children_across_a_window_shift() {
    use crate::core::{IdGenerator, NodeId};

    fn keyed_row(key: &str) -> Node {
      let mut wrapper = Node::column(0.0, Alignment::Start, vec![Node::text(key)]);
      wrapper.set_component_key(Some(key));
      wrapper
    }
    fn id_for(parent: &Node, key: &str) -> NodeId {
      parent
        .children
        .iter()
        .find(|child| child.component_key() == Some(key))
        .expect("row present")
        .node_id()
    }
    fn collect_ids(node: &Node, out: &mut Vec<NodeId>) {
      out.push(node.node_id());
      for child in &node.children {
        collect_ids(child, out);
      }
    }

    let id_gen = IdGenerator::new();
    // Old window: rows a, b, c.
    let mut old = Node::column(
      0.0,
      Alignment::Start,
      vec![keyed_row("a"), keyed_row("b"), keyed_row("c")],
    );
    old.assign_ids(&id_gen);
    let old_b = id_for(&old, "b");
    let old_c = id_for(&old, "c");

    // New window scrolled by one: rows b, c, d — b and c each moved up a slot.
    let mut new = Node::column(
      0.0,
      Alignment::Start,
      vec![keyed_row("b"), keyed_row("c"), keyed_row("d")],
    );
    new.preserve_ids_from(&mut old);
    new.assign_ids(&id_gen);

    // b and c keep their node ids despite moving position, so hover/focus
    // tracked by id stays glued to the same rows instead of jumping.
    assert_eq!(id_for(&new, "b"), old_b);
    assert_eq!(id_for(&new, "c"), old_c);

    // No id collision between preserved and freshly assigned nodes.
    let mut ids = Vec::new();
    collect_ids(&new, &mut ids);
    let unique: std::collections::HashSet<u64> = ids.iter().map(|id| id.value()).collect();
    assert_eq!(unique.len(), ids.len());
  }
}
