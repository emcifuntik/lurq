//! A grid by hand: items that fill it as the controls say, a form, named cells, and tracks of each kind.

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Grid, Rect, Row, TextInput},
  core::Signal,
  layout::{
    Alignment,
    grid::GridTrack,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color},
};

use crate::{
  engine_kit::{FILL, body, card, choice, chosen, controls, label, note, page, slider, toggle},
  style::{BORDER, PRIMARY, SURFACE_DARK, TEXT, text},
};

const ALIGNMENTS: [(&str, Alignment); 4] = [
  ("stretch", Alignment::Stretch),
  ("start", Alignment::Start),
  ("center", Alignment::Center),
  ("end", Alignment::End),
];
const TILES: [&str; 6] = ["#1d4ed8", "#0e7490", "#15803d", "#a16207", "#b91c1c", "#7e22ce"];

pub(crate) struct GridDemo {
  columns: Signal<i32>,
  items: Signal<i32>,
  gap: Signal<i32>,
  align: Signal<usize>,
  justify: Signal<usize>,
  spans: Signal<bool>,
  name: Signal<String>,
  email: Signal<String>,
}

/// A tile with no size of its own: it takes its cell's where the grid stretches it.
fn tile(caption: &str, index: usize) -> Column {
  Column::new()
    .padding(10.0)
    .background(TILES[index % TILES.len()])
    .rounded(6.0)
    .child(text(caption, 12.0, FontWeight::Medium, "#ffffff"))
}

fn frame(grid: Grid) -> Grid {
  grid
    .padding(10.0)
    .background(SURFACE_DARK)
    .rounded(8.0)
    .border_inside(1.0, Color::from_hex(BORDER))
}

impl GridDemo {
  fn flow(&self) -> Element {
    let mut grid = Grid::with_columns(self.columns.get().max(1) as usize)
      .gap(self.gap.get() as f32)
      .align_items(chosen(&ALIGNMENTS, &self.align))
      .justify_items(chosen(&ALIGNMENTS, &self.justify))
      .width(FILL);
    for index in 0..self.items.get().max(0) as usize {
      let mut item = tile(&format!("item {}", index + 1), index);
      if self.spans.get() {
        item = match index {
          0 => item.column_span(2),
          2 => item.row_span(2),
          _ => item,
        };
      }
      // Every third item is taller, so the rows differ and stretching shows.
      if index % 3 == 1 {
        item = item.child(text("a second line", 12.0, FontWeight::Normal, "#ffffff"));
      }
      grid = grid.child(item);
    }
    card("Items take the cells in their order")
      .child(slider("Columns", &self.columns, 1, 6))
      .child(slider("Items", &self.items, 0, 18))
      .child(slider("Gap", &self.gap, 0, 32))
      .child(controls([
        Element::from(label("Top to bottom")),
        choice(&ALIGNMENTS, &self.align).into(),
      ]))
      .child(controls([
        Element::from(label("Side to side")),
        choice(&ALIGNMENTS, &self.justify).into(),
      ]))
      .child(controls([toggle(
        "item 1 spans two columns, item 3 two rows",
        &self.spans,
      )]))
      .child(frame(grid))
      .child(note(
        "Rows are as tall as their tallest item and are added as the items need them. Resize the window: the columns \
         share the width.",
      ))
      .into()
  }

  fn form(&self) -> Element {
    let field = |value: &Signal<String>, placeholder: &str| {
      TextInput::new(value.clone())
        .placeholder(placeholder)
        .placeholder_style(TextStyle {
          color: Color::from_hex("#94a3b8"),
          ..TextStyle::default()
        })
        .padding_horizontal(10.0)
        .height(32.0)
        .background("#f8fafc")
        .rounded(6.0)
        .border_inside(1.0, Color::from_hex("#94a3b8"))
        .tab_index(0)
    };
    let grid = Grid::new()
      .columns([GridTrack::Auto, GridTrack::Fr(1.0)])
      .gap(10.0)
      .align_items(Alignment::Center)
      .width(FILL)
      .child(body("Name"))
      .child(field(&self.name, "Ada Lovelace"))
      .child(body("Email address"))
      .child(field(&self.email, "ada@example.com"))
      .child(note("The labels' column is as wide as the widest label; the fields take the rest.").column_span(2));
    card("A form: a column of content, a column of what is left")
      .child(frame(grid))
      .into()
  }

  fn tracks(&self) -> Element {
    let grid = Grid::new()
      .columns([
        GridTrack::Px(90.0),
        GridTrack::Fr(1.0),
        GridTrack::Fr(2.0),
        GridTrack::Auto,
      ])
      .gap(8.0)
      .width(FILL)
      .child(tile("90 px", 0))
      .child(tile("1 fr", 1))
      .child(tile("2 fr", 2))
      .child(tile("auto: as wide as this", 3));
    let rows = Grid::new()
      .columns([GridTrack::Fr(1.0)])
      .rows([GridTrack::Px(30.0), GridTrack::Fr(1.0), GridTrack::Fr(2.0)])
      .gap(8.0)
      .size(220.0, 220.0)
      .child(tile("30 px", 0))
      .child(tile("1 fr", 1))
      .child(tile("2 fr", 2));
    let named = Grid::with_columns(3)
      .rows([GridTrack::Px(44.0), GridTrack::Px(44.0), GridTrack::Px(44.0)])
      .gap(8.0)
      // Three rows of 44, two gaps of 8, and the frame's padding of 10 on each side.
      .size(240.0, 168.0)
      .child(tile("0, 0", 0).grid_column(0).grid_row(0))
      .child(tile("2, 0", 1).grid_column(2).grid_row(0))
      .child(tile("1, 1", 2).grid_column(1).grid_row(1))
      .child(tile("0, 2", 3).grid_column(0).grid_row(2))
      .child(tile("2, 2", 4).grid_column(2).grid_row(2));
    card("Tracks, and cells an item names")
      .child(frame(grid))
      .child(
        // The three wrap onto more lines in a narrow window.
        Row::new()
          .wrap()
          .width(FILL)
          .spacing(24.0)
          .align_items(Alignment::Start)
          .child(
            Column::new()
              .spacing(8.0)
              .child(label("Rows share a height that is given: 220"))
              .child(frame(rows)),
          )
          .child(
            Column::new()
              .spacing(8.0)
              .child(label("Each item names its column and row"))
              .child(frame(named)),
          )
          .child(
            Column::new()
              .spacing(8.0)
              .child(label("An item with a size of its own keeps it"))
              .child(frame(
                Grid::with_columns(2)
                  .gap(8.0)
                  .size(220.0, 110.0)
                  .child(Rect::new(40.0, 24.0).background(PRIMARY))
                  .child(tile("stretched", 2))
                  .child(tile("stretched", 3))
                  .child(Rect::new(60.0, 16.0).background(TEXT)),
              )),
          ),
      )
      .into()
  }
}

impl Component for GridDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      columns: ctx.signal(3),
      items: ctx.signal(7),
      gap: ctx.signal(10),
      align: ctx.signal(0),
      justify: ctx.signal(0),
      spans: ctx.signal(true),
      name: ctx.signal(String::new()),
      email: ctx.signal(String::new()),
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    page(
      "Grid",
      "Move the sliders and resize the window. Type in the form: a field that grows does not move its label's column.",
    )
    .child(self.flow())
    .child(self.form())
    .child(self.tracks())
  }
}
