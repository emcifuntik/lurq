//! The pieces every screen is built from.

use lurq::{
  components::{Column, Row, Text},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, color::Color, dimension::Dimension},
};

pub(crate) const BG: &str = "#0f172a";
pub(crate) const SURFACE: &str = "#1e293b";
pub(crate) const BORDER: &str = "#334155";
pub(crate) const TEXT: &str = "#f8fafc";
pub(crate) const MUTED: &str = "#94a3b8";
pub(crate) const PRIMARY: &str = "#3b82f6";
pub(crate) const PRIMARY_ACTIVE: &str = "#1d4ed8";
pub(crate) const ACCENT: &str = "#22c55e";

pub(crate) const FILL: Dimension = Dimension::Pct(100.0);
/// The header every screen sits under.
pub(crate) const HEADER_HEIGHT: f32 = 52.0;
/// The smallest side of something a finger is meant to hit.
pub(crate) const TAP_TARGET: f32 = 44.0;

/// How many lines an event log keeps.
const LOG_LINES: usize = 8;

pub(crate) fn text(content: &str, font_size: f32, weight: FontWeight, color: &str) -> Text {
  Text::styled(
    content,
    TextStyle {
      font_size,
      weight,
      color: Color::from_hex(color),
      ..TextStyle::default()
    },
  )
}

pub(crate) fn label(content: &str) -> Text {
  text(content, 15.0, FontWeight::Normal, TEXT)
}

pub(crate) fn caption(content: &str) -> Text {
  text(content, 13.0, FontWeight::Normal, MUTED)
}

pub(crate) fn card(content: impl Into<Element>) -> Column {
  Column::new()
    .child(content)
    .padding(14.0)
    .width(FILL)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(10.0)
}

pub(crate) fn button(caption: &str, on_click: impl Fn() + Send + Sync + 'static) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(text(caption, 15.0, FontWeight::Bold, "#ffffff"))
    .height(TAP_TARGET)
    .padding_horizontal(18.0)
    .background(PRIMARY)
    .rounded(8.0)
    .hovered(|style| style.background("#60a5fa"))
    .active(|style| style.background(PRIMARY_ACTIVE))
    .on_click(move |_| on_click())
}

/// The newest event first; the oldest lines fall off.
pub(crate) fn push_log(log: &Signal<Vec<String>>, line: String) {
  log.update(|lines| {
    lines.insert(0, line);
    lines.truncate(LOG_LINES);
  });
}

pub(crate) fn event_log(log: &Signal<Vec<String>>) -> Column {
  let lines = log.get();
  let mut column = Column::new().spacing(2.0);
  if lines.is_empty() {
    column = column.child(caption("No events yet"));
  }
  for line in lines {
    column = column.child(text(&line, 12.0, FontWeight::Normal, MUTED).nowrap());
  }
  card(column)
}
