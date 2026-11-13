//! Content two fingers pinch larger and smaller, in a list that scrolls it both ways: a photo, a map or a page
//! looked at on a phone.
//!
//! [`ZoomScroll`] goes around content of a size the app says. A pinch scales the content about the middle of
//! the two fingers, which stays under them, and the fingers carry the content with them as they move; a
//! double tap goes in at the point tapped, and back out. One finger scrolls what is shown, as in any list.
//!
//! The content is laid out once, at its own size, and drawn scaled: text in it is not laid out anew for the
//! size it is shown at.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::{
  app::{
    component::Component,
    ctx::Ctx,
    events::{GesturePhase, MouseEvent, PinchEvent},
  },
  components::{ScrollBoth, Stack},
  core::{ElementRef, Signal},
  layout::layout_kind::ScrollState,
  node::{Element, dimension::Dimension, transform::Transform2D},
};

const FILL: Dimension = Dimension::Pct(100.0);

#[derive(Clone)]
pub struct ZoomScrollProps {
  width: f32,
  height: f32,
  min: f32,
  max: f32,
  double_tap: Option<f32>,
  zoom: Option<Signal<f32>>,
  scroll: Option<ScrollState>,
}

impl PartialEq for ZoomScrollProps {
  fn eq(&self, _other: &Self) -> bool {
    // The signal and the scroll state are the app's: the content is made again with its parent.
    false
  }
}

#[cfg(feature = "devtools")]
impl crate::app::component::DevtoolsInspectable for ZoomScrollProps {
  fn write_info(&self, _buffer: &mut Vec<crate::app::component::ComponentInfo>) {}
}

impl ZoomScrollProps {
  /// For content `width` by `height` in logical pixels at a scale of 1: the size it is laid out at.
  pub fn new(width: f32, height: f32) -> Self {
    Self {
      width,
      height,
      min: 1.0,
      max: 4.0,
      double_tap: Some(2.0),
      zoom: None,
      scroll: None,
    }
  }

  /// The smallest and the largest scale a pinch goes to: 1 and 4 unless set. A photo larger than the screen
  /// has a smallest scale below 1, the one it fits at.
  pub fn range(mut self, min: f32, max: f32) -> Self {
    self.min = min.max(f32::EPSILON);
    self.max = max.max(self.min);
    self
  }

  /// The scale a double tap goes to from the smallest, as a share of the smallest: 2 unless set. `None`
  /// leaves a double tap to the content.
  pub fn double_tap(mut self, scale: Option<f32>) -> Self {
    self.double_tap = scale;
    self
  }

  /// The scale the content is shown at, to read and to set.
  pub fn zoom(mut self, zoom: Signal<f32>) -> Self {
    self.zoom = Some(zoom);
    self
  }

  /// The state of the list that scrolls the content, for the app to read where it is and to scroll it.
  pub fn scroll(mut self, scroll: ScrollState) -> Self {
    self.scroll = Some(scroll);
    self
  }
}

/// What two fingers took hold of.
struct Held {
  /// The scale the content was at.
  zoom: f32,
  /// The point of the content under the middle of the fingers, in the content's own pixels.
  point: (f32, f32),
}

/// Where content `size` large is drawn at a scale in a viewport: how large it is shown, and how far in from
/// the viewport's start it lies when it is smaller than the viewport, which it is in the middle of.
fn placed(size: (f32, f32), zoom: f32, viewport: (f32, f32)) -> ((f32, f32), (f32, f32)) {
  let shown = (size.0 * zoom, size.1 * zoom);
  let margin = (
    ((viewport.0 - shown.0) / 2.0).max(0.0),
    ((viewport.1 - shown.1) / 2.0).max(0.0),
  );
  (shown, margin)
}

/// Content that is pinched larger and smaller.
pub struct ZoomScroll {
  /// The scale, for content whose props name no signal.
  zoom: Signal<f32>,
  scroll: ScrollState,
  /// The list as it is laid out: where it is in the window.
  view: ElementRef,
  /// The viewport's size the content was last placed for.
  viewport: Signal<(f32, f32)>,
  held: Arc<Mutex<Option<Held>>>,
}

impl ZoomScroll {
  /// Around `content`, which is laid out at the size the props say. The list fills what it is put in.
  pub fn mount(ctx: &mut Ctx, props: ZoomScrollProps, content: impl Into<Element>) -> Element {
    ctx.mount_with::<Self>(props, vec![content.into()])
  }
}

impl Component for ZoomScroll {
  type Props = ZoomScrollProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self {
      zoom: Signal::new(1.0),
      scroll: ScrollState::new(),
      view: ElementRef::default(),
      viewport: Signal::new((0.0, 0.0)),
      held: Arc::default(),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<ZoomScrollProps>().clone();
    let zoom_signal = props.zoom.clone().unwrap_or_else(|| self.zoom.clone());
    let scroll = props.scroll.clone().unwrap_or_else(|| self.scroll.clone());
    let (min, max) = (props.min, props.max);
    let size = (props.width, props.height);
    let zoom = zoom_signal.get().clamp(min, max);
    let viewport = self.viewport.get();
    let (shown, margin) = placed(size, zoom, viewport);

    // A node is scaled about its middle: moved by half of what it grew, its corner stays where it is laid
    // out, and from there it is moved into the middle of a viewport larger than it.
    let mut content = Stack::new().size(size.0, size.1).transform(Transform2D {
      a: zoom,
      b: 0.0,
      c: 0.0,
      d: zoom,
      tx: size.0 / 2.0 * (zoom - 1.0) + margin.0,
      ty: size.1 / 2.0 * (zoom - 1.0) + margin.1,
    });
    for child in ctx.children() {
      content = content.child(child.clone());
    }
    // What the list scrolls is as large as the content is shown, and no smaller than the viewport.
    let area = Stack::new()
      .size(shown.0.max(viewport.0), shown.1.max(viewport.1))
      .child(content);

    // Brings the content to a scale with one of its points at a place in the viewport.
    let go = {
      let (zoom_signal, scroll, view) = (zoom_signal.clone(), scroll.clone(), self.view.clone());
      move |zoom: f32, point: (f32, f32), at: (f32, f32)| {
        let zoom = zoom.clamp(min, max);
        let (_, margin) = placed(size, zoom, (view.width(), view.height()));
        if zoom_signal.get_untracked() != zoom {
          zoom_signal.set(zoom);
        }
        // The list lays the content out at its new size before it takes the offset.
        scroll.set_scroll_pending(point.0 * zoom + margin.0 - at.0, point.1 * zoom + margin.1 - at.1);
      }
    };
    // The point of the content at a place in the window, and that place in the viewport.
    let point_at = {
      let (zoom_signal, scroll, view) = (zoom_signal.clone(), scroll.clone(), self.view.clone());
      move |x: f32, y: f32| {
        let zoom = zoom_signal.get_untracked().clamp(min, max);
        let (_, margin) = placed(size, zoom, (view.width(), view.height()));
        let at = (x - view.x(), y - view.y());
        let point = (
          (at.0 + scroll.scroll_x() - margin.0) / zoom,
          (at.1 + scroll.scroll_y() - margin.1) / zoom,
        );
        (zoom, point, at)
      }
    };

    let mut list = ScrollBoth::new(area)
      .with_scroll_state(scroll.clone())
      .ref_element(self.view.clone())
      .size(FILL, FILL)
      .on_pinch({
        let (held, go, point_at, view) = (self.held.clone(), go.clone(), point_at.clone(), self.view.clone());
        move |event: PinchEvent| {
          event.stop_propagation();
          match event.phase {
            GesturePhase::Start => {
              let (zoom, point, _) = point_at(event.x, event.y);
              *held.lock() = Some(Held { zoom, point });
            }
            GesturePhase::Change => {
              if let Some(held) = &*held.lock() {
                // The point the fingers took stays under their middle, wherever that goes.
                go(
                  held.zoom * event.scale,
                  held.point,
                  (event.x - view.x(), event.y - view.y()),
                );
              }
            }
            GesturePhase::End => {
              held.lock().take();
            }
          }
        }
      });
    if let Some(double_tap) = props.double_tap {
      list = list.on_dblclick(move |event: MouseEvent| {
        if event.from_keyboard {
          return;
        }
        event.stop_propagation();
        let (zoom, point, at) = point_at(event.x, event.y);
        // In at the point tapped, which stays under the finger; from anywhere further in, back out.
        go(if zoom > min * 1.01 { min } else { min * double_tap }, point, at);
      });
    }
    list
  }

  fn after_layout(&self) {
    if !self.view.is_attached() {
      return;
    }
    let viewport = (self.view.width(), self.view.height());
    let last = self.viewport.get_untracked();
    if (last.0 - viewport.0).abs() > 0.25 || (last.1 - viewport.1).abs() > 0.25 {
      self.viewport.set(viewport);
    }
  }
}
