use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct ElementRef {
  inner: Arc<RwLock<ElementRefInner>>,
}

#[derive(Clone, Default)]
pub struct ElementRefMut {
  inner: Arc<RwLock<ElementRefInner>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElementRect {
  pub x: f32,
  pub y: f32,
  pub relative_x: f32,
  pub relative_y: f32,
  pub width: f32,
  pub height: f32,
}

impl ElementRect {
  pub fn center(&self) -> (f32, f32) {
    (self.x + self.width / 2.0, self.y + self.height / 2.0)
  }
}

#[derive(Default)]
struct ElementRefInner {
  #[cfg(feature = "canvas")]
  canvas: Option<crate::canvas::CanvasWeak>,
  x: f32,
  y: f32,
  relative_x: f32,
  relative_y: f32,
  width: f32,
  height: f32,
  attached: bool,
  hovered: bool,
  active: bool,
  focused: bool,
  focus_signal: Option<crate::core::Signal<bool>>,
  focus_visible: bool,
  focus_visible_signal: Option<crate::core::Signal<bool>>,
  override_rect: Option<ElementRect>,
  override_cleared: bool,
  layout_dirty: bool,
  layout_subscribers: crate::core::layout_dirty::LayoutSubscribers,
  rect_observer: Option<Arc<dyn Fn(ElementRect) + Send + Sync>>,
}

impl ElementRef {
  pub(crate) fn track_layout(&self, watch: &Arc<crate::core::layout_dirty::LayoutWatch>) {
    self.inner.read().unwrap().layout_subscribers.bind(watch);
  }
  /// Typed access to the attached canvas; `None` before layout or after detachment.
  #[cfg(feature = "canvas")]
  pub fn as_canvas(&self) -> Option<crate::canvas::CanvasHandle> {
    let canvas = self.inner.read().unwrap().canvas.as_ref()?.upgrade()?;
    canvas.is_attached().then_some(canvas)
  }

  #[cfg(feature = "canvas")]
  pub(crate) fn bind_canvas(&self, canvas: &crate::canvas::CanvasHandle) {
    let mut inner = self.inner.write().unwrap();
    if let Some(old) = inner.canvas.as_ref().and_then(crate::canvas::CanvasWeak::upgrade)
      && old.is_attached()
      && old.surface_id() != canvas.surface_id()
    {
      tracing::warn!("an element ref cannot bind to two live canvases");
      return;
    }
    inner.canvas = Some(canvas.downgrade());
  }

  #[cfg(feature = "canvas")]
  pub(crate) fn detach_canvas(&self, id: crate::canvas::CanvasId) {
    let mut inner = self.inner.write().unwrap();
    if inner
      .canvas
      .as_ref()
      .and_then(crate::canvas::CanvasWeak::upgrade)
      .is_some_and(|canvas| canvas.surface_id() == id)
    {
      inner.canvas = None;
      inner.attached = false;
    }
  }

  pub fn new() -> Self {
    Self::default()
  }

  pub fn mutable(&self) -> ElementRefMut {
    ElementRefMut {
      inner: self.inner.clone(),
    }
  }

  pub fn x(&self) -> f32 {
    self.inner.read().unwrap().x
  }

  pub fn y(&self) -> f32 {
    self.inner.read().unwrap().y
  }

  pub fn width(&self) -> f32 {
    self.inner.read().unwrap().width
  }

  pub fn height(&self) -> f32 {
    self.inner.read().unwrap().height
  }

  pub fn rect(&self) -> (f32, f32, f32, f32) {
    let inner = self.inner.read().unwrap();
    (inner.x, inner.y, inner.width, inner.height)
  }

  pub fn bounds(&self) -> ElementRect {
    let inner = self.inner.read().unwrap();
    ElementRect {
      x: inner.x,
      y: inner.y,
      relative_x: inner.relative_x,
      relative_y: inner.relative_y,
      width: inner.width,
      height: inner.height,
    }
  }

  pub fn is_attached(&self) -> bool {
    self.inner.read().unwrap().attached
  }

  pub fn hovered(&self) -> bool {
    self.inner.read().unwrap().hovered
  }

  pub fn active(&self) -> bool {
    self.inner.read().unwrap().active
  }

  pub fn focused(&self) -> bool {
    self.focus_signal().get()
  }

  /// Reactive focus state. Reading `focused()` during render subscribes too.
  pub fn focus_signal(&self) -> crate::core::Signal<bool> {
    let mut inner = self.inner.write().unwrap();
    let focused = inner.focused;
    inner
      .focus_signal
      .get_or_insert_with(|| crate::core::Signal::new(focused))
      .clone()
  }

  /// Focused and showing a focus ring, like CSS `:focus-visible`: the focus
  /// came from the keyboard (or a key was pressed since), or the element is a
  /// text input. Reading it during render subscribes, like [`Self::focused`].
  pub fn focus_visible(&self) -> bool {
    self.focus_visible_signal().get()
  }

  /// Reactive focus-visible state. Reading `focus_visible()` during render
  /// subscribes too.
  pub fn focus_visible_signal(&self) -> crate::core::Signal<bool> {
    let mut inner = self.inner.write().unwrap();
    let visible = inner.focus_visible;
    inner
      .focus_visible_signal
      .get_or_insert_with(|| crate::core::Signal::new(visible))
      .clone()
  }

  pub(crate) fn update(&self, x: f32, y: f32, relative_x: f32, relative_y: f32, width: f32, height: f32) {
    let observer = {
      let mut inner = self.inner.write().unwrap();
      let changed = !inner.attached
        || inner.x != x
        || inner.y != y
        || inner.relative_x != relative_x
        || inner.relative_y != relative_y
        || inner.width != width
        || inner.height != height;
      inner.x = x;
      inner.y = y;
      inner.relative_x = relative_x;
      inner.relative_y = relative_y;
      inner.width = width;
      inner.height = height;
      inner.attached = true;
      if changed { inner.rect_observer.clone() } else { None }
    };
    // Outside the lock: the observer may read this ref (or write signals
    // whose subscribers do).
    if let Some(observer) = observer {
      observer(ElementRect {
        x,
        y,
        relative_x,
        relative_y,
        width,
        height,
      });
    }
  }

  /// Register a callback fired whenever the measured rect changes, including
  /// the first time the element is laid out. It runs during quad resolution
  /// (after layout, before present), so writing a [`crate::core::Signal`]
  /// from it marks the owning component dirty and the runtime schedules a
  /// follow-up pass — the measure-then-render handshake for components whose
  /// render depends on their own measured size. Registering again replaces
  /// the previous observer.
  pub fn observe_rect(&self, observer: impl Fn(ElementRect) + Send + Sync + 'static) {
    self.inner.write().unwrap().rect_observer = Some(Arc::new(observer));
  }

  pub(crate) fn same_handle(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.inner, &other.inner)
  }

  pub(crate) fn override_rect(&self) -> Option<ElementRect> {
    self.inner.read().unwrap().override_rect
  }

  pub(crate) fn take_override_cleared(&self) -> bool {
    let mut inner = self.inner.write().unwrap();
    let cleared = inner.override_cleared;
    inner.override_cleared = false;
    cleared
  }

  pub(crate) fn has_layout_dirty(&self) -> bool {
    self.inner.read().unwrap().layout_dirty
  }

  pub(crate) fn take_layout_dirty(&self) -> bool {
    let mut inner = self.inner.write().unwrap();
    let dirty = inner.layout_dirty;
    inner.layout_dirty = false;
    dirty
  }

  pub(crate) fn set_hovered(&self, hovered: bool) {
    self.inner.write().unwrap().hovered = hovered;
  }

  pub(crate) fn set_active(&self, active: bool) {
    self.inner.write().unwrap().active = active;
  }

  /// Losing focus also loses focus-visible.
  pub(crate) fn set_focused(&self, focused: bool) {
    if !focused {
      self.set_focus_visible(false);
    }
    let signal = {
      let mut inner = self.inner.write().unwrap();
      if inner.focused == focused {
        return;
      }
      inner.focused = focused;
      inner.focus_signal.clone()
    };
    if let Some(signal) = signal {
      signal.set(focused);
    }
  }

  pub(crate) fn set_focus_visible(&self, visible: bool) {
    let signal = {
      let mut inner = self.inner.write().unwrap();
      if inner.focus_visible == visible {
        return;
      }
      inner.focus_visible = visible;
      inner.focus_visible_signal.clone()
    };
    if let Some(signal) = signal {
      signal.set(visible);
    }
  }
}

impl ElementRefMut {
  #[cfg(feature = "canvas")]
  pub fn as_canvas(&self) -> Option<crate::canvas::CanvasHandle> {
    self.as_ref().as_canvas()
  }

  pub fn new() -> Self {
    Self::default()
  }

  pub fn as_ref(&self) -> ElementRef {
    ElementRef {
      inner: self.inner.clone(),
    }
  }

  pub fn x(&self) -> f32 {
    self.as_ref().x()
  }

  pub fn y(&self) -> f32 {
    self.as_ref().y()
  }

  pub fn width(&self) -> f32 {
    self.as_ref().width()
  }

  pub fn height(&self) -> f32 {
    self.as_ref().height()
  }

  pub fn rect(&self) -> (f32, f32, f32, f32) {
    self.as_ref().rect()
  }

  pub fn bounds(&self) -> ElementRect {
    self.as_ref().bounds()
  }

  /// See [`ElementRef::observe_rect`].
  pub fn observe_rect(&self, observer: impl Fn(ElementRect) + Send + Sync + 'static) {
    self.as_ref().observe_rect(observer);
  }

  pub fn set_bounds(&self, rect: ElementRect) {
    let mut inner = self.inner.write().unwrap();
    inner.x = rect.x;
    inner.y = rect.y;
    inner.relative_x = rect.relative_x;
    inner.relative_y = rect.relative_y;
    inner.width = rect.width;
    inner.height = rect.height;
    inner.override_rect = Some(rect);
    inner.layout_dirty = true;
    inner.layout_subscribers.mark();
  }

  pub fn set_relative_bounds(&self, relative_x: f32, relative_y: f32, width: f32, height: f32) {
    let current = self.bounds();
    let parent_x = current.x - current.relative_x;
    let parent_y = current.y - current.relative_y;
    self.set_bounds(ElementRect {
      x: parent_x + relative_x,
      y: parent_y + relative_y,
      relative_x,
      relative_y,
      width,
      height,
    });
  }

  pub fn clear_bounds_override(&self) {
    let mut inner = self.inner.write().unwrap();
    if inner.override_rect.take().is_some() {
      // Cached parent layouts have the override baked into their child
      // offsets — flag so the layout engine falls back to a full relayout
      // instead of serving the stale offset.
      inner.override_cleared = true;
    }
    inner.layout_dirty = true;
    inner.layout_subscribers.mark();
  }

  pub fn is_attached(&self) -> bool {
    self.as_ref().is_attached()
  }

  pub fn hovered(&self) -> bool {
    self.as_ref().hovered()
  }

  pub fn active(&self) -> bool {
    self.as_ref().active()
  }

  pub fn focused(&self) -> bool {
    self.as_ref().focused()
  }

  pub fn focus_signal(&self) -> crate::core::Signal<bool> {
    self.as_ref().focus_signal()
  }

  /// See [`ElementRef::focus_visible`].
  pub fn focus_visible(&self) -> bool {
    self.as_ref().focus_visible()
  }

  /// See [`ElementRef::focus_visible_signal`].
  pub fn focus_visible_signal(&self) -> crate::core::Signal<bool> {
    self.as_ref().focus_visible_signal()
  }
}

impl From<ElementRefMut> for ElementRef {
  fn from(value: ElementRefMut) -> Self {
    value.as_ref()
  }
}
