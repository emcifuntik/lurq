//! Another window of the app: beside the first on a tablet, or on a display plugged into the device. A list
//! to scroll and a button to press show that a finger works in it as in the first.

use lurq::{
  app::{WindowOpener, component::Component, ctx::Ctx},
  components::{Column, ScrollVertical},
  core::Signal,
  layout::text_style::FontWeight,
  node::Element,
};

use crate::ui::{BG, FILL, TEXT, button, caption, card, label, text};

/// Opens another window. On a display plugged in it says so.
pub(crate) fn open(opener: &WindowOpener, external: bool) {
  let title = if external { "On the display" } else { "Another window" };
  opener.open(title, 480, 640, move |app, tree| {
    tree.mount_root::<OtherWindow>(app, external);
  });
}

pub(crate) struct OtherWindow {
  presses: Signal<u32>,
}

impl Component for OtherWindow {
  type Props = bool;

  fn create(ctx: &mut Ctx) -> Self {
    Self { presses: ctx.signal(0) }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let external = *ctx.props::<bool>();
    let window = ctx.window();
    let safe_area = window.safe_area;
    let (presses, close) = (self.presses.clone(), window.clone());
    let mut rows = Column::new().spacing(8.0).width(FILL);
    for row in 1..=40 {
      rows = rows.child(card(label(&format!("Row {row}"))));
    }
    let content = Column::new()
      .spacing(12.0)
      .padding(16.0)
      .width(FILL)
      .child(text(
        if external { "On the display" } else { "Another window" },
        28.0,
        FontWeight::Bold,
        TEXT,
      ))
      .child(
        caption(&format!(
          "This window is {:.0} by {:.0}; its safe area leaves {:.0} at the top and {:.0} at the bottom.",
          window.logical_width(),
          window.logical_height(),
          safe_area.top,
          safe_area.bottom
        ))
        .width(FILL),
      )
      .child(
        button(&format!("Pressed {} times", self.presses.get()), move || {
          presses.update(|count| *count += 1)
        })
        .width(FILL),
      )
      .child(button("Close this window", move || close.close()).width(FILL))
      .child(rows);
    ScrollVertical::new(content)
      .size(FILL, FILL)
      .padding_top(safe_area.top)
      .padding_bottom(safe_area.bottom)
      .background(BG)
  }
}
