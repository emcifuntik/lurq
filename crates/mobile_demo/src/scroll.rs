//! Scrolling with a finger: a long list with momentum, a row that scrolls sideways and a list inside the list.
//! Every row counts its taps, so a tap that a scroll should have swallowed shows.

use lurq::{
  app::{
    component::Component,
    ctx::Ctx,
    events::{ScrollEvent, ScrollPhase},
  },
  components::{Column, Row, ScrollHorizontal, ScrollVertical},
  core::Signal,
  layout::{Alignment, layout_kind::Justify, text_style::FontWeight},
  node::{Element, color::Color},
};

use crate::ui::{ACCENT, BG, BORDER, FILL, PRIMARY, SURFACE, TAP_TARGET, TEXT, caption, label, text};

const ROWS: usize = 120;
/// The rows the sideways strip and the inner list come before.
const STRIP_BEFORE: usize = 3;
const INNER_LIST_BEFORE: usize = 8;

#[derive(Clone, Default)]
struct Status {
  taps: u32,
  last_tap: Option<String>,
  gestures: u32,
  scrolling: bool,
}

pub(crate) struct ScrollScreen {
  status: Signal<Status>,
}

impl Component for ScrollScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      status: ctx.signal(Status::default()),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    let mut list = Column::new().spacing(8.0).padding(16.0).width(FILL);
    for index in 0..ROWS {
      if index == STRIP_BEFORE {
        list = list.child(self.strip());
      }
      if index == INNER_LIST_BEFORE {
        list = list.child(self.inner_list());
      }
      list = list.child(self.row(format!("Row {}", index + 1)));
    }
    let (started, ended) = (self.status.clone(), self.status.clone());

    Column::new()
      .child(self.status_bar())
      .child(
        ScrollVertical::new(list)
          .flex(1.0)
          .on_scroll_start(move |_: ScrollEvent| {
            started.update(|status| {
              status.gestures += 1;
              status.scrolling = true;
            });
          })
          .on_scroll_end(move |event: ScrollEvent| {
            debug_assert!(matches!(event.phase, ScrollPhase::End));
            ended.update(|status| status.scrolling = false);
          }),
      )
      .size(FILL, FILL)
      .background(BG)
  }
}

impl ScrollScreen {
  fn status_bar(&self) -> Element {
    let status = self.status.get();
    let last = status.last_tap.as_deref().unwrap_or("nothing");
    Column::new()
      .spacing(2.0)
      .child(label(&format!("{} taps, last on {last}", status.taps)).nowrap())
      .child(
        caption(&format!(
          "{} scroll gestures, {}",
          status.gestures,
          // A flick counts twice: the finger's gesture and its momentum.
          if status.scrolling { "scrolling" } else { "at rest" }
        ))
        .nowrap(),
      )
      .padding_horizontal(16.0)
      .padding_vertical(10.0)
      .width(FILL)
      .background(SURFACE)
      .border_inside(1.0, Color::from_hex(BORDER))
      .into()
  }

  fn row(&self, name: String) -> Element {
    let status = self.status.clone();
    let tapped = name.clone();
    Row::new()
      .align_items(Alignment::Center)
      .child(label(&name))
      .height(TAP_TARGET)
      .padding_horizontal(14.0)
      .width(FILL)
      .background(SURFACE)
      .rounded(8.0)
      .active(|style| style.background(PRIMARY))
      .on_click(move |_| {
        status.update(|status| {
          status.taps += 1;
          status.last_tap = Some(tapped.clone());
        });
      })
      .into()
  }

  fn strip(&self) -> Element {
    let mut cards = Row::new().spacing(8.0);
    for index in 0..20 {
      cards = cards.child(self.tile(format!("Card {}", index + 1)));
    }
    Column::new()
      .spacing(6.0)
      .child(caption(
        "Sideways: a horizontal drag scrolls this, a vertical one the page",
      ))
      .child(ScrollHorizontal::new(cards).width(FILL))
      .width(FILL)
      .into()
  }

  fn tile(&self, name: String) -> Element {
    let status = self.status.clone();
    let tapped = name.clone();
    Column::new()
      .align_items(Alignment::Center)
      .justify(Justify::Center)
      .child(text(&name, 14.0, FontWeight::Bold, TEXT).nowrap())
      .size(110.0, 72.0)
      .background(PRIMARY)
      .rounded(10.0)
      .active(|style| style.background(ACCENT))
      .on_click(move |_| {
        status.update(|status| {
          status.taps += 1;
          status.last_tap = Some(tapped.clone());
        });
      })
      .into()
  }

  fn inner_list(&self) -> Element {
    let mut rows = Column::new().spacing(6.0).padding(8.0).width(FILL);
    for index in 0..30 {
      rows = rows.child(self.row(format!("Inner row {}", index + 1)));
    }
    Column::new()
      .spacing(6.0)
      .child(caption(
        "A list inside the list: it scrolls first, the page at its ends",
      ))
      .child(
        ScrollVertical::new(rows)
          .height(220.0)
          .width(FILL)
          .background("#111c30")
          .border_inside(1.0, Color::from_hex(BORDER))
          .rounded(10.0),
      )
      .width(FILL)
      .into()
  }
}
