//! A list of ten thousand rows of uneven heights, of which only those on screen are mounted: the finger, its
//! momentum and the pull past the ends go through rows that come and go, and a tap on the status bar has a
//! long way to the top.

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Row, VirtualizedList},
  core::Signal,
  layout::{Alignment, text_style::FontWeight},
  node::{Element, color::Color},
};

use crate::ui::{BG, BORDER, FILL, MUTED, PRIMARY, SURFACE, TAP_TARGET, TEXT, caption, label, text};

const ROWS: usize = 10_000;

#[derive(Clone, Default, lurq::DevtoolsInspectable)]
struct Status {
  taps: u32,
  last_tap: Option<usize>,
}

pub(crate) struct VirtualListScreen {
  status: Signal<Status>,
}

impl Component for VirtualListScreen {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      status: ctx.signal(Status::default()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let status = self.status.get();
    let last = status
      .last_tap
      .map_or("nothing".to_owned(), |row| format!("row {}", row + 1));
    let taps = self.status.clone();
    let list = VirtualizedList::new(ctx, 0..ROWS)
      .flex(1.0)
      .width(FILL)
      .mount_keyed::<ListRow, _, _, _>(
        |index| format!("row-{index}"),
        move |index| RowProps {
          index: *index,
          taps: taps.clone(),
        },
      );

    Column::new()
      .child(
        Column::new()
          .spacing(2.0)
          .child(label(&format!("{} taps, last on {last}", status.taps)).nowrap())
          .child(caption(&format!("{ROWS} rows, mounted as they come into view")).nowrap())
          .padding_horizontal(16.0)
          .padding_vertical(10.0)
          .width(FILL)
          .background(SURFACE)
          .border_inside(1.0, Color::from_hex(BORDER)),
      )
      .child(list)
      .size(FILL, FILL)
      .background(BG)
  }
}

#[derive(Clone, lurq::DevtoolsInspectable)]
struct RowProps {
  index: usize,
  taps: Signal<Status>,
}

impl PartialEq for RowProps {
  fn eq(&self, other: &Self) -> bool {
    self.index == other.index
  }
}

struct ListRow;

impl Component for ListRow {
  type Props = RowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let RowProps { index, taps } = ctx.props::<RowProps>().clone();
    // One row in five has a second line, and one in eleven a third: the list measures them as they come.
    let lines = 1 + usize::from(index % 5 == 0) + usize::from(index % 11 == 0);
    let mut body = Column::new()
      .spacing(3.0)
      .child(text(&format!("Row {}", index + 1), 15.0, FontWeight::Bold, TEXT));
    for line in 1..lines {
      body = body.child(text(
        ["", "A second line makes this row taller than most.", "And a third one."][line],
        13.0,
        FontWeight::Normal,
        MUTED,
      ));
    }
    Row::new()
      .align_items(Alignment::Center)
      .child(body.flex(1.0))
      .min_height(TAP_TARGET)
      .padding_horizontal(16.0)
      .padding_vertical(8.0)
      .width(FILL)
      .border_inside(1.0, Color::from_hex(BORDER))
      .active(|style| style.background(PRIMARY))
      .on_click(move |_| {
        taps.update(|status| {
          status.taps += 1;
          status.last_tap = Some(index);
        });
      })
  }
}
