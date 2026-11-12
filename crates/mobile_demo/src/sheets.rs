//! Sheets: content that comes up from the bottom over the screen. One as high as what it says, one of two
//! heights with a list that scrolls in it, and one that only its own button puts away.

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, ScrollVertical, Sheet, SheetDetent, SheetProps, SheetStyle, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::ScrollState,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color},
};

use crate::ui::{BG, BORDER, FILL, PRIMARY, SURFACE, TAP_TARGET, TEXT, button, caption, card, label, text};

pub(crate) struct SheetsScreen {
  choice: Signal<bool>,
  list: Signal<bool>,
  held: Signal<bool>,
  /// Which of the list sheet's heights it is at.
  height: Signal<usize>,
  scroll: ScrollState,
  picked: Signal<String>,
  note: Signal<String>,
}

impl Component for SheetsScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      choice: ctx.signal(false),
      list: ctx.signal(false),
      held: ctx.signal(false),
      height: ctx.signal(0),
      scroll: ScrollState::new(),
      picked: ctx.signal(String::new()),
      note: ctx.signal(String::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let style = SheetStyle::dark().background(Color::from_hex(SURFACE)).grabber(true);
    let opens = |caption: &str, open: &Signal<bool>| {
      let open = open.clone();
      button(caption, move || open.set(true)).width(FILL)
    };
    let picked = self.picked.get();

    // As high as its content: a choice. A tap beside it, a drag down or its own rows put it away.
    let mut rows = Column::new().width(FILL).padding(16.0).padding_top(28.0).spacing(8.0);
    rows = rows.child(text("Pick one", 17.0, FontWeight::Bold, TEXT));
    for name in ["The first", "The second", "The third"] {
      let (open, picked) = (self.choice.clone(), self.picked.clone());
      rows = rows.child(sheet_row(name).on_click(move |_| {
        picked.set(name.to_owned());
        open.set(false);
      }));
    }
    let choice = Sheet::mount_keyed(ctx, "choice", SheetProps::new(self.choice.clone()).style(style), rows);

    // Half the screen or all of it, with a list: a finger on the list moves the sheet up first, then scrolls
    // the list, and takes the sheet down again once the list is at its top.
    let mut items = Column::new().width(FILL).padding(16.0).spacing(8.0);
    for index in 1..=40 {
      items = items.child(sheet_row(&format!("Item {index}")));
    }
    let height = self.height.get();
    let list = Sheet::mount_keyed(
      ctx,
      "list",
      SheetProps::new(self.list.clone())
        .detents([SheetDetent::Medium, SheetDetent::Large])
        .detent(self.height.clone())
        .scroll(self.scroll.clone())
        .style(style),
      Column::new()
        .size(FILL, FILL)
        .padding_top(24.0)
        .child(
          Row::new()
            .width(FILL)
            .padding_horizontal(16.0)
            .padding_bottom(8.0)
            .child(text(
              if height == 0 {
                "At half height"
              } else {
                "At full height"
              },
              17.0,
              FontWeight::Bold,
              TEXT,
            )),
        )
        .child(
          ScrollVertical::new(items)
            .with_scroll_state(self.scroll.clone())
            .width(FILL)
            .flex(1.0),
        ),
    );

    // Not dismissable: neither a tap beside it nor a drag puts it away, and it stays over the keyboard.
    let done = self.held.clone();
    let held = Sheet::mount_keyed(
      ctx,
      "held",
      SheetProps::new(self.held.clone()).dismissable(false).style(style),
      Column::new()
        .width(FILL)
        .padding(16.0)
        .padding_top(28.0)
        .spacing(12.0)
        .child(text("A note", 17.0, FontWeight::Bold, TEXT))
        .child(caption("Only the button puts this sheet away.").width(FILL))
        .child(
          TextInput::new(self.note.clone())
            .single_line()
            .placeholder("Type here")
            .text_style(TextStyle {
              font_size: 17.0,
              color: Color::from_hex(TEXT),
              ..TextStyle::default()
            })
            .caret_color(Color::from_hex(PRIMARY))
            .padding_horizontal(12.0)
            .padding_vertical(10.0)
            .min_height(TAP_TARGET)
            .width(FILL)
            .background(BG)
            .border_inside(1.0, Color::from_hex(BORDER))
            .rounded(10.0),
        )
        .child(button("Done", move || done.set(false)).width(FILL)),
    );

    Column::new()
      .size(FILL, FILL)
      .background(BG)
      .padding(16.0)
      .spacing(12.0)
      .child(caption("A sheet comes up over everything, the bar too, and the screen behind dims.").width(FILL))
      .child(opens("As high as its content", &self.choice))
      .child(opens("Two heights and a list", &self.list))
      .child(opens("One that stays", &self.held))
      .child(card(label(&if picked.is_empty() {
        "Nothing picked yet".to_owned()
      } else {
        format!("Picked: {picked}")
      })))
      .child(choice)
      .child(list)
      .child(held)
  }
}

fn sheet_row(name: &str) -> Row {
  Row::new()
    .width(FILL)
    .height(TAP_TARGET)
    .align_items(Alignment::Center)
    .padding_horizontal(14.0)
    .background(BG)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(10.0)
    .child(label(name))
}
