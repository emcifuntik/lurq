//! What a finger comes to: where the pointer is, what was pressed and clicked, and whether a slider under the
//! finger is dragged instead of scrolling the screen.

use lurq::{
  app::{
    component::Component,
    ctx::Ctx,
    events::{LongPressEvent, MouseEvent, TouchEvent},
    window::{Haptic, WindowHandle},
  },
  components::{Column, Rect, Row, Slider, Spacer, Stack},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{Element, color::Color},
};

use crate::ui::{
  ACCENT, BG, BORDER, FILL, HEADER_HEIGHT, MUTED, PRIMARY, SURFACE, TEXT, button, caption, card, event_log, label,
  push_log, text,
};

/// The dot that follows the pointer.
const DOT: f32 = 56.0;

#[derive(Clone, Copy, Default)]
struct Pointer {
  x: f32,
  y: f32,
  inside: bool,
  pressed: bool,
}

#[derive(Clone, Copy, Default)]
struct Counts {
  downs: u32,
  ups: u32,
  clicks: u32,
  double_clicks: u32,
  moves: u32,
  long_presses: u32,
  /// The fingers on the pad now, and the most there were at once.
  fingers: u32,
  most_fingers: u32,
}

pub(crate) struct TouchScreen {
  pointer: Signal<Pointer>,
  counts: Signal<Counts>,
  log: Signal<Vec<String>>,
  slider: Signal<i32>,
  button_taps: Signal<u32>,
}

impl Component for TouchScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      pointer: ctx.signal(Pointer::default()),
      counts: ctx.signal(Counts::default()),
      log: ctx.signal(Vec::new()),
      slider: ctx.signal(40),
      button_taps: ctx.signal(0),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let pointer = self.pointer.get();
    let window = ctx.window();
    // Where this screen starts in the window: events carry the window's coordinates.
    let safe_area = ctx.window().safe_area;
    let (origin_x, origin_y) = (safe_area.left, safe_area.top + HEADER_HEIGHT);
    let mut screen = Stack::new().child(
      Column::new()
        .spacing(12.0)
        .padding(16.0)
        .child(self.pad(pointer, window.clone()))
        .child(self.counters())
        .child(self.controls(window))
        .child(event_log(&self.log))
        .size(FILL, FILL),
    );
    if pointer.inside {
      // Drawn where the tree says the pointer is. Under the finger, the window's coordinates are right.
      screen = screen.child(
        Rect::new(DOT, DOT)
          .background(if pointer.pressed { ACCENT } else { PRIMARY })
          .opacity(0.55)
          .rounded(DOT / 2.0)
          .pointer_events_none()
          .absolute_position(pointer.x - origin_x - DOT / 2.0, pointer.y - origin_y - DOT / 2.0),
      );
    }
    screen.size(FILL, FILL).background(BG)
  }
}

impl TouchScreen {
  fn pad(&self, pointer: Pointer, window: WindowHandle) -> Element {
    let status = if pointer.inside {
      format!(
        "x {:.0}  y {:.0}  {}",
        pointer.x,
        pointer.y,
        if pointer.pressed { "pressed" } else { "hovering" }
      )
    } else {
      "Touch here".to_owned()
    };
    let (pointer_down, counts_down, log_down) = (self.pointer.clone(), self.counts.clone(), self.log.clone());
    let (pointer_up, counts_up, log_up) = (self.pointer.clone(), self.counts.clone(), self.log.clone());
    let (pointer_move, counts_move) = (self.pointer.clone(), self.counts.clone());
    let (counts_click, log_click) = (self.counts.clone(), self.log.clone());
    let (counts_double, log_double) = (self.counts.clone(), self.log.clone());
    let (pointer_enter, log_enter) = (self.pointer.clone(), self.log.clone());
    let (pointer_leave, log_leave) = (self.pointer.clone(), self.log.clone());
    let (counts_long, log_long) = (self.counts.clone(), self.log.clone());
    let (counts_touch_start, log_touch_start) = (self.counts.clone(), self.log.clone());
    let (counts_touch_end, log_touch_end) = (self.counts.clone(), self.log.clone());
    let (counts_touch_cancel, log_touch_cancel) = (self.counts.clone(), self.log.clone());

    Column::new()
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .spacing(6.0)
      .child(text(&status, 17.0, FontWeight::Bold, TEXT).nowrap())
      .child(caption(
        "Tap, double tap, rest for a long press, put several fingers down.",
      ))
      .flex(1.0)
      .min_height(140.0)
      .width(FILL)
      .background(SURFACE)
      .border_inside(1.0, Color::from_hex(BORDER))
      .rounded(10.0)
      .active(|style| style.background("#273449"))
      .on_mouse_down(move |event: MouseEvent| {
        pointer_down.update(|pointer| {
          (pointer.x, pointer.y, pointer.pressed, pointer.inside) = (event.x, event.y, true, true);
        });
        counts_down.update(|counts| counts.downs += 1);
        push_log(&log_down, format!("down  {:.0}, {:.0}", event.x, event.y));
      })
      .on_mouse_up(move |event: MouseEvent| {
        pointer_up.update(|pointer| {
          (pointer.x, pointer.y, pointer.pressed) = (event.x, event.y, false);
        });
        counts_up.update(|counts| counts.ups += 1);
        push_log(&log_up, format!("up    {:.0}, {:.0}", event.x, event.y));
      })
      .on_mouse_move(move |event: MouseEvent| {
        pointer_move.update(|pointer| {
          (pointer.x, pointer.y, pointer.inside) = (event.x, event.y, true);
        });
        counts_move.update(|counts| counts.moves += 1);
      })
      .on_click(move |event: MouseEvent| {
        counts_click.update(|counts| counts.clicks += 1);
        push_log(&log_click, format!("click {:.0}, {:.0}", event.x, event.y));
      })
      .on_dblclick(move |event: MouseEvent| {
        counts_double.update(|counts| counts.double_clicks += 1);
        push_log(&log_double, format!("double click {:.0}, {:.0}", event.x, event.y));
      })
      .on_mouse_enter(move || {
        pointer_enter.update(|pointer| pointer.inside = true);
        push_log(&log_enter, "enter".to_owned());
      })
      .on_mouse_leave(move || {
        pointer_leave.update(|pointer| (pointer.inside, pointer.pressed) = (false, false));
        push_log(&log_leave, "leave".to_owned());
      })
      .on_long_press(move |event: LongPressEvent| {
        window.haptic(Haptic::Medium);
        counts_long.update(|counts| counts.long_presses += 1);
        push_log(&log_long, format!("long press {:.0}, {:.0}", event.x, event.y));
      })
      .on_touch_start(move |event: TouchEvent| {
        counts_touch_start.update(|counts| {
          counts.fingers += 1;
          counts.most_fingers = counts.most_fingers.max(counts.fingers);
        });
        push_log(
          &log_touch_start,
          format!("touch start #{:x} {:.0}, {:.0}", event.id & 0xfff, event.x, event.y),
        );
      })
      .on_touch_end(move |event: TouchEvent| {
        counts_touch_end.update(|counts| counts.fingers = counts.fingers.saturating_sub(1));
        push_log(&log_touch_end, format!("touch end #{:x}", event.id & 0xfff));
      })
      .on_touch_cancel(move |event: TouchEvent| {
        counts_touch_cancel.update(|counts| counts.fingers = counts.fingers.saturating_sub(1));
        push_log(&log_touch_cancel, format!("touch cancel #{:x}", event.id & 0xfff));
      })
      .into()
  }

  fn counters(&self) -> Element {
    let counts = self.counts.get();
    let cell = |name: &str, count: u32| {
      Column::new()
        .align_items(Alignment::Center)
        .spacing(2.0)
        .child(text(&count.to_string(), 17.0, FontWeight::Bold, TEXT))
        .child(text(name, 11.0, FontWeight::Normal, MUTED))
        .flex(1.0)
    };
    card(
      Row::new()
        .child(cell("down", counts.downs))
        .child(cell("up", counts.ups))
        .child(cell("click", counts.clicks))
        .child(cell("double", counts.double_clicks))
        .child(cell("long", counts.long_presses))
        .child(cell("fingers", counts.most_fingers))
        .width(FILL),
    )
    .into()
  }

  fn controls(&self, window: WindowHandle) -> Element {
    let taps = self.button_taps.clone();
    card(
      Column::new()
        .spacing(12.0)
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .child(label("Slider: drag it without a long press"))
            .child(Spacer::new().flex(1.0))
            .child(text(&self.slider.get().to_string(), 15.0, FontWeight::Bold, ACCENT)),
        )
        .child(
          Slider::new(self.slider.clone())
            .range(0, 100)
            .height(28.0)
            .width(FILL)
            .background("#475569")
            .rounded(14.0),
        )
        .child(
          Row::new()
            .align_items(Alignment::Center)
            .spacing(12.0)
            .child(button("Tap me", move || {
              window.haptic(Haptic::Light);
              taps.update(|taps| *taps += 1);
            }))
            .child(label(&format!("tapped {} times", self.button_taps.get()))),
        )
        .width(FILL),
    )
    .into()
  }
}
