//! The list of screens.

use lurq::{
  app::ctx::Ctx,
  components::{Column, Link, Row, Spacer, StackOutlet},
  layout::{Alignment, text_style::FontWeight},
  node::{Element, color::Color},
};

use crate::ui::{BG, BORDER, FILL, MUTED, SURFACE, TEXT, caption, text};

/// A screen of the app: its route, its name and what it checks.
pub(crate) struct Screen {
  pub(crate) path: &'static str,
  pub(crate) title: &'static str,
  pub(crate) checks: &'static str,
}

pub(crate) const SCREENS: &[Screen] = &[
  Screen {
    path: "/touch",
    title: "Touch",
    checks: "Tap, double tap, long press, drag, a slider under the finger",
  },
  Screen {
    path: "/gestures",
    title: "Gestures",
    checks: "Pinch and rotate, taps of several fingers, swipes, pans. Back: a finger in from the left edge",
  },
  Screen {
    path: "/scroll",
    title: "Scroll",
    checks: "Momentum, nested and horizontal scrolling, taps while it moves",
  },
  Screen {
    path: "/virtual-list",
    title: "Virtual list",
    checks: "Ten thousand uneven rows mounted as they come into view: momentum, the ends, the way to the top",
  },
  Screen {
    path: "/inputs",
    title: "Inputs",
    checks: "The on-screen keyboard, what it types into a field, and a field it would cover",
  },
];

pub(crate) fn title_of(path: &str) -> &'static str {
  SCREENS
    .iter()
    .find(|screen| screen.path == path)
    .map_or("lurq on iOS", |screen| screen.title)
}

pub(crate) fn home(ctx: &mut Ctx) -> Element {
  let mut list = Column::new().spacing(10.0).padding(16.0).width(FILL);
  list = list.child(caption("Each screen checks one thing iOS does differently."));
  for screen in SCREENS {
    list = list.child(screen_row(ctx, screen));
  }
  // What the system said of the app: leave it and come back, or open `lurqdemo://anything` in a browser.
  let said = crate::app_events().get();
  list = list.child(caption(&if said.is_empty() {
    "App events: none yet".to_owned()
  } else {
    format!("App events, the latest first: {}", said.join(", "))
  }));
  StackOutlet::large_title_scroll(ctx, list)
    .flex(1.0)
    .background(BG)
    .into()
}

fn screen_row(ctx: &mut Ctx, screen: &Screen) -> Element {
  let row = Row::new()
    .align_items(Alignment::Center)
    .child(
      Column::new()
        .spacing(4.0)
        .child(text(screen.title, 17.0, FontWeight::Bold, TEXT))
        .child(caption(screen.checks).width(FILL))
        .flex(1.0),
    )
    .child(Spacer::new().width(12.0))
    .child(text("›", 24.0, FontWeight::Normal, MUTED))
    .padding(14.0)
    .width(FILL)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(10.0)
    .hovered(|style| style.background("#273449"));

  Link::build_empty(ctx, screen.path).child(row).width(FILL).into()
}
