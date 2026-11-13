//! Borders by hand: one box whose border is whatever the controls say, and the cases worth a look at the
//! same radius.

use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Grid, Row},
  core::Signal,
  layout::{Alignment, layout_kind::Justify},
  node::{
    Element,
    border::{Border, BorderPlacement},
  },
};

use crate::{
  engine_kit::{FILL, body, card, choice, chosen, controls, label, note, page, slider, toggle},
  style::{ACCENT, PRIMARY, WARNING},
};

/// The fill of the boxes, light so that a gap between a box and its border shows.
const BOX: &str = "#e2e8f0";
const PLACEMENTS: [(&str, BorderPlacement); 3] = [
  ("inside", BorderPlacement::Inside),
  ("center", BorderPlacement::Center),
  ("outside", BorderPlacement::Outside),
];

pub(crate) struct BordersDemo {
  radius: Signal<i32>,
  width: Signal<i32>,
  placement: Signal<usize>,
  dashed: Signal<bool>,
  dash: Signal<i32>,
  gap: Signal<i32>,
  /// Top, right, bottom, left.
  sides: [Signal<bool>; 4],
}

impl BordersDemo {
  fn border(&self, color: &str) -> Border {
    let border = Border::new(self.width.get() as f32, color, chosen(&PLACEMENTS, &self.placement));
    if self.dashed.get() {
      border.dash(self.dash.get() as f32, self.gap.get() as f32)
    } else {
      border
    }
  }

  /// A box of the radius the page is set to, as wide as the cell it is in.
  fn sample(&self, caption: &str) -> Column {
    Column::new()
      .height(76.0)
      .padding(10.0)
      .background(BOX)
      .rounded(self.radius.get() as f32)
      .child(lurq::components::Text::new(caption))
  }

  fn playground(&self) -> Element {
    let border = self.border(PRIMARY);
    let mut sample = Column::new()
      .size(300.0, 150.0)
      .background(BOX)
      .rounded(self.radius.get() as f32);
    let [top, right, bottom, left] = &self.sides;
    if top.get() {
      sample = sample.border_top(border.clone());
    }
    if right.get() {
      sample = sample.border_right(border.clone());
    }
    if bottom.get() {
      sample = sample.border_bottom(border.clone());
    }
    if left.get() {
      sample = sample.border_left(border);
    }
    card("The border you set")
      .child(controls([
        Element::from(label("Sides")),
        toggle("top", top).into(),
        toggle("right", right).into(),
        toggle("bottom", bottom).into(),
        toggle("left", left).into(),
      ]))
      .child(controls([
        Element::from(label("Placement")),
        choice(&PLACEMENTS, &self.placement).into(),
        toggle("dashed", &self.dashed).into(),
      ]))
      .child(slider("Radius", &self.radius, 0, 60))
      .child(slider("Width", &self.width, 1, 16))
      .child(slider("Dash", &self.dash, 1, 30))
      .child(slider("Gap", &self.gap, 1, 30))
      // Room around the box: a border outside it is painted past its edge, and a container clips there.
      .child(
        Row::new()
          .width(FILL)
          .height(210.0)
          .justify(Justify::Center)
          .align_items(Alignment::Center)
          .child(sample),
      )
      .child(note(
        "A side that has no border should show nothing at its edge, at any radius. A border on one side follows the \
         corners at its ends and thins out. Outside and centered, the border's inner edge keeps the box's corners.",
      ))
      .into()
  }

  fn cases(&self) -> Element {
    let line = |width: f32| Border::inside(width, "#475569");
    // Room between the boxes: a border outside one is painted past its edge.
    let cases = Grid::with_columns(4).gap(18.0).padding(8.0).width(FILL).with_children([
      self.sample("left only").border_left(Border::inside(5.0, PRIMARY)),
      self
        .sample("top 1, bottom 6")
        .border_top(line(1.0))
        .border_bottom(line(6.0)),
      self
        .sample("three sides")
        .border_top(line(2.0))
        .border_left(line(2.0))
        .border_right(line(2.0)),
      self
        .sample("two colors")
        .border_top(Border::inside(3.0, WARNING))
        .border_bottom(Border::inside(3.0, ACCENT)),
      self.sample("outside 4").border(Border::outside(4.0, PRIMARY)),
      self.sample("center 4").border(Border::center(4.0, PRIMARY)),
      self.sample("outside, left").border_left(Border::outside(5.0, PRIMARY)),
      // A border is painted over what is in the box: the caption starts below it.
      self
        .sample("wider than radius")
        .padding_top(30.0)
        .border_top(Border::inside(24.0, "#94a3b8")),
      self.sample("dashed").border(line(1.5).dashed()),
      self
        .sample("dash 2 4")
        .border(Border::inside(2.0, PRIMARY).dash(2.0, 4.0)),
      self
        .sample("dashed outside")
        .border(Border::outside(3.0, ACCENT).dashed()),
      self.sample("dashed bottom").border_bottom(line(2.0).dashed()),
    ]);
    card("Cases, at the radius set above")
      .child(cases)
      .child(body(
        "Dashes go around the corners and close on themselves: no half dash where the pattern ends.",
      ))
      .into()
  }
}

impl Component for BordersDemo {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      radius: ctx.signal(16),
      width: ctx.signal(4),
      placement: ctx.signal(0),
      dashed: ctx.signal(false),
      dash: ctx.signal(8),
      gap: ctx.signal(6),
      sides: [
        ctx.signal(false),
        ctx.signal(false),
        ctx.signal(false),
        ctx.signal(true),
      ],
    }
  }

  fn render(&self, _ctx: &mut Ctx) -> impl Into<Element> {
    page(
      "Borders",
      "Change the sides, the radius and the width and look at the corners. Try a fractional display scale too: an edge \
       with no border must stay clean.",
    )
    .child(self.playground())
    .child(self.cases())
  }
}
