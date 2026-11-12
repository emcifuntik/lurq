//! Two fingers on one element: a pinch scales the square and a rotation turns it. Both at once is the usual
//! case, which the square follows.

use lurq::{
  app::{
    component::Component,
    ctx::Ctx,
    events::{GesturePhase, PanEvent, PinchEvent, RotateEvent, SwipeEvent, TapEvent},
    window::Haptic,
  },
  components::{Column, Rect, Row, Stack},
  core::Signal,
  layout::{Alignment, StackAlignment, layout_kind::Justify, text_style::FontWeight},
  node::{Element, color::Color, transform::Transform2D},
};

use crate::ui::{BG, BORDER, FILL, PRIMARY, SURFACE, TEXT, button, caption, card, event_log, push_log, text};

#[derive(Clone, Copy)]
struct Shape {
  scale: f32,
  /// Radians, clockwise.
  rotation: f32,
  pinching: bool,
  rotating: bool,
}

impl Default for Shape {
  fn default() -> Self {
    Self {
      scale: 1.0,
      rotation: 0.0,
      pinching: false,
      rotating: false,
    }
  }
}

pub(crate) struct GesturesScreen {
  shape: Signal<Shape>,
  log: Signal<Vec<String>>,
}

impl Component for GesturesScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      shape: ctx.signal(Shape::default()),
      log: ctx.signal(Vec::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let shape = self.shape.get();
    let reset = self.shape.clone();
    let window = ctx.window();
    Column::new()
      .spacing(12.0)
      .padding(16.0)
      .child(self.surface(shape, window))
      .child(card(
        Row::new()
          .align_items(Alignment::Center)
          .spacing(12.0)
          .child(
            Column::new()
              .spacing(2.0)
              .child(text(
                &format!("scale {:.2}   {:.0}°", shape.scale, shape.rotation.to_degrees()),
                17.0,
                FontWeight::Bold,
                TEXT,
              ))
              .child(caption(match (shape.pinching, shape.rotating) {
                (true, true) => "pinching and rotating",
                (true, false) => "pinching",
                (false, true) => "rotating",
                (false, false) => "at rest",
              }))
              .flex(1.0),
          )
          .child(button("Reset", move || reset.set(Shape::default())))
          .width(FILL),
      ))
      .child(event_log(&self.log))
      .size(FILL, FILL)
      .background(BG)
  }
}

impl GesturesScreen {
  /// Where the fingers go: anywhere on it, not only on the square.
  fn surface(&self, shape: Shape, window: lurq::app::window::WindowHandle) -> Element {
    let (pinched, pinch_log) = (self.shape.clone(), self.log.clone());
    let (rotated, rotate_log) = (self.shape.clone(), self.log.clone());
    let (tap_log, swipe_log, pan_log) = (self.log.clone(), self.log.clone(), self.log.clone());
    let (tap_window, swipe_window) = (window.clone(), window);
    Stack::new()
      .stack_align(StackAlignment::Center)
      .child(
        Column::new()
          .align_items(Alignment::Center)
          .justify(Justify::Center)
          .child(text("lurq", 22.0, FontWeight::Bold, "#ffffff"))
          .size(140.0, 140.0)
          .background(PRIMARY)
          .rounded(18.0)
          .transform(Transform2D::scale_uniform(shape.scale).then(&Transform2D::rotate(shape.rotation))),
      )
      .child(
        Rect::new(FILL, FILL)
          .on_tap(move |event: TapEvent| {
            // Two fingers and more: a tap of one is the click every screen has.
            if event.fingers > 1 {
              tap_window.haptic(Haptic::Light);
            }
            push_log(
              &tap_log,
              format!("tap of {} at {:.0}, {:.0}", event.fingers, event.x, event.y),
            );
          })
          .on_swipe(move |event: SwipeEvent| {
            swipe_window.haptic(Haptic::Selection);
            push_log(
              &swipe_log,
              format!(
                "swipe {:?} of {} at {:.0} pt/s",
                event.direction, event.fingers, event.velocity
              ),
            );
          })
          .on_pan(move |event: PanEvent| {
            if event.phase != GesturePhase::Change {
              push_log(
                &pan_log,
                format!(
                  "pan {:?} of {} by {:.0}, {:.0}",
                  event.phase, event.fingers, event.translation_x, event.translation_y
                ),
              );
            }
          })
          .on_pinch(move |event: PinchEvent| {
            pinched.update(|shape| {
              shape.scale = (shape.scale * event.delta).clamp(0.3, 4.0);
              shape.pinching = event.phase != GesturePhase::End;
            });
            if event.phase != GesturePhase::Change {
              push_log(
                &pinch_log,
                format!("pinch {:?} at scale {:.2}", event.phase, event.scale),
              );
            }
          })
          .on_rotate(move |event: RotateEvent| {
            rotated.update(|shape| {
              shape.rotation += event.delta;
              shape.rotating = event.phase != GesturePhase::End;
            });
            if event.phase != GesturePhase::Change {
              push_log(
                &rotate_log,
                format!("rotate {:?} at {:.0}°", event.phase, event.rotation.to_degrees()),
              );
            }
          }),
      )
      .flex(1.0)
      .min_height(220.0)
      .width(FILL)
      .background(SURFACE)
      .border_inside(1.0, Color::from_hex(BORDER))
      .rounded(10.0)
      .clip()
      .into()
  }
}
