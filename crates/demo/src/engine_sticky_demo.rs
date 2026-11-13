//! Sticky positioning by hand: section headers in a list, a footer, and a table whose first row and first
//! column stay.

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, ScrollBoth, ScrollVertical},
  core::Signal,
  layout::{Alignment, layout_kind::Sticky, text_style::FontWeight},
  node::{CursorIcon, Element, color::Color},
};

use crate::{
  engine_kit::{FILL, card, controls, counted, note, page, slider},
  style::{BORDER, PRIMARY, SURFACE_DARK, TEXT, TEXT_MUTED, text},
};

const HEADER: &str = "#3f4f66";
const LIST: &str = "#0f172a";

pub(crate) struct StickyDemo {
  inset: Signal<i32>,
  header_clicks: Signal<u32>,
  row_clicks: Signal<u32>,
}

fn list_frame(content: impl Into<Element>, height: f32) -> ScrollVertical {
  ScrollVertical::new(content)
    .size(320.0, height)
    .background(LIST)
    .rounded(8.0)
    .border_inside(1.0, Color::from_hex(BORDER))
}

impl StickyDemo {
  fn sections(&self) -> Element {
    let mut list = Column::new().width(FILL);
    for (letter, names) in [
      ("A", ["Ada", "Alan", "Alonzo", "Annie"]),
      ("B", ["Barbara", "Bjarne", "Brian", "Butler"]),
      ("C", ["Claude", "Clarence", "Cleve", "Corrado"]),
      ("D", ["Dana", "David", "Dennis", "Donald"]),
      ("E", ["Edsger", "Edgar", "Elizabeth", "Evelyn"]),
    ] {
      let header_clicks = self.header_clicks.clone();
      let mut section = Column::new().width(FILL).child(
        Row::new()
          .width(FILL)
          .padding_horizontal(12.0)
          .height(28.0)
          .align_items(Alignment::Center)
          .background(HEADER)
          .cursor(CursorIcon::Pointer)
          .sticky(Sticky::top(self.inset.get() as f32))
          .on_click(move |_| header_clicks.update(|count| *count += 1))
          .child(text(letter, 12.0, FontWeight::Bold, TEXT)),
      );
      for name in names {
        let row_clicks = self.row_clicks.clone();
        section = section.child(
          Row::new()
            .width(FILL)
            .padding_horizontal(12.0)
            .height(36.0)
            .align_items(Alignment::Center)
            .cursor(CursorIcon::Pointer)
            .hovered(|style| style.background("#1e293b"))
            .on_click(move |_| row_clicks.update(|count| *count += 1))
            .child(text(name, 13.0, FontWeight::Normal, TEXT)),
        );
      }
      list = list.child(section);
    }
    card("Section headers")
      .child(slider("Inset from top", &self.inset, 0, 40))
      .child(controls([
        counted("clicks on a header", self.header_clicks.get()),
        counted("clicks on a row", self.row_clicks.get()),
      ]))
      .child(list_frame(list, 230.0))
      .child(note(
        "Scroll the list: a header stays at the top until its section ends, and the next one pushes it out. Click a \
         stuck header where a row is under it: the header takes the click, the row does not.",
      ))
      .into()
  }

  fn footer(&self) -> Element {
    let mut section = Column::new().width(FILL);
    for index in 1..=14 {
      section = section.child(
        Row::new()
          .padding_horizontal(12.0)
          .height(32.0)
          .align_items(Alignment::Center)
          .child(text(&format!("Line {index}"), 13.0, FontWeight::Normal, TEXT)),
      );
    }
    let section = section.child(
      Row::new()
        .width(FILL)
        .padding_horizontal(12.0)
        .height(34.0)
        .align_items(Alignment::Center)
        .background(PRIMARY)
        .sticky(Sticky::bottom(0.0))
        .child(text("3 selected", 12.0, FontWeight::Bold, "#ffffff")),
    );
    let after =
      Column::new()
        .padding(12.0)
        .height(220.0)
        .child(text("After the section", 13.0, FontWeight::Normal, TEXT_MUTED));
    card("A footer")
      .child(list_frame(Column::new().width(FILL).child(section).child(after), 200.0))
      .child(note(
        "The bar is the last line of its section. It is held at the bottom of the view while its place is below, and \
         is in its place once that scrolls in.",
      ))
      .into()
  }

  fn table(&self) -> Element {
    const COLUMNS: usize = 10;
    let cell = |content: &str, fill: &str, weight: FontWeight| {
      Row::new()
        .size(110.0, 34.0)
        .padding_horizontal(10.0)
        .align_items(Alignment::Center)
        .background(fill)
        .border_bottom(lurq::node::border::Border::inside(1.0, BORDER))
        .child(text(content, 12.0, weight, TEXT))
    };
    let mut header = Row::new()
      .sticky(Sticky::top(0.0))
      .child(cell("Name", HEADER, FontWeight::Bold).sticky(Sticky::left(0.0)));
    for column in 1..COLUMNS {
      header = header.child(cell(&format!("Week {column}"), HEADER, FontWeight::Bold));
    }
    let mut table = Column::new().child(header);
    for (index, name) in [
      "Ada",
      "Alan",
      "Barbara",
      "Bjarne",
      "Claude",
      "Dana",
      "Dennis",
      "Edsger",
      "Elizabeth",
      "Evelyn",
      "Frances",
      "Grace",
    ]
    .into_iter()
    .enumerate()
    {
      let mut row = Row::new().child(cell(name, SURFACE_DARK, FontWeight::Medium).sticky(Sticky::left(0.0)));
      for column in 1..COLUMNS {
        row = row.child(cell(
          &format!("{}", (index * 7 + column * 3) % 40),
          LIST,
          FontWeight::Normal,
        ));
      }
      table = table.child(row);
    }
    card("A table: the first row and the first column stay")
      .child(
        ScrollBoth::new(table)
          .size(520.0, 240.0)
          .background(LIST)
          .rounded(8.0)
          .border_inside(1.0, Color::from_hex(BORDER)),
      )
      .child(note(
        "Scroll down and sideways. The names stay on the left and the weeks on top; the corner cell stays in the \
         corner, over both.",
      ))
      .into()
  }
}

impl Component for StickyDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      inset: ctx.signal(0),
      header_clicks: ctx.signal(0),
      row_clicks: ctx.signal(0),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    page(
      "Sticky",
      "Each list scrolls on its own. An element sticks in the scroll container it is in, not in this page's.",
    )
    .child(
      Row::new()
        .spacing(20.0)
        .align_items(Alignment::Start)
        .width(FILL)
        .child(Column::new().flex(1.0).child(self.sections()))
        .child(Column::new().flex(1.0).child(self.footer())),
    )
    .child(self.table())
  }
}
