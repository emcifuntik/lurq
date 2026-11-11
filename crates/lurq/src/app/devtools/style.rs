use crate::{
  components::{Row, Text, TextInput},
  core::Signal,
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{CursorIcon, Element, color::Color, dimension::Dimension},
};

pub(crate) const BG: &str = "#0d0d0d";
pub(crate) const SURFACE: &str = "#161616";
pub(crate) const SURFACE_2: &str = "#1e1e1e";
pub(crate) const SELECTED: &str = "#1a1a2e";
pub(crate) const BORDER: &str = "#27272a";
pub(crate) const TEXT: &str = "#e4e4e7";
pub(crate) const MUTED: &str = "#71717a";
pub(crate) const PRIMARY: &str = "#a855f7";
pub(crate) const BLUE: &str = "#60a5fa";
pub(crate) const GREEN: &str = "#22c55e";
pub(crate) const SIGNAL_GREEN: &str = "#4ade80";
pub(crate) const ORANGE: &str = "#fb923c";
pub(crate) const YELLOW: &str = "#fbbf24";
pub(crate) const PINK: &str = "#f472b6";
pub(crate) const FILL: Dimension = Dimension::Pct(100.0);
pub(crate) const CONTROL_HEIGHT: f32 = 30.0;
pub(crate) const CONTROL_RADIUS: f32 = 4.0;

pub(crate) fn section_header(label: &str, count: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(text(label, 11.0, FontWeight::Bold, MUTED))
    .child(crate::components::Spacer::new().flex(1.0))
    .child(text(count, 10.0, FontWeight::Normal, MUTED))
    .width(FILL)
    .height(36.0)
    .padding_horizontal(12.0)
    .padding_vertical(0.0)
    .border_inside(1.0, Color::from_hex(BORDER))
    .into()
}

pub(crate) fn empty_state(message: &str) -> Element {
  Row::new()
    .child(text(message, 11.0, FontWeight::Normal, MUTED))
    .padding_horizontal(14.0)
    .padding_vertical(12.0)
    .into()
}

pub(crate) fn badge(label: &str, color: &str, fill: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(text(label, 10.0, FontWeight::Medium, color).nowrap())
    .height(18.0)
    .padding_horizontal(6.0)
    .padding_vertical(0.0)
    .background(fill)
    .rounded(3.0)
    .into()
}

pub(crate) fn toolbar_button(icon_name: &str, label: &str, color: &str, fill: &str, border: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(6.0)
    .child(icon(icon_name, 13.0, color))
    .child(text(label, 12.0, FontWeight::Medium, color))
    .height(CONTROL_HEIGHT)
    .padding_horizontal(10.0)
    .padding_vertical(0.0)
    .background(fill)
    .border_inside(1.0, Color::from_hex(border))
    .rounded(CONTROL_RADIUS)
    .cursor(CursorIcon::Pointer)
}

pub(crate) fn toolbar_input(placeholder: &str, shortcut: Option<&str>) -> Row {
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(icon("search", 13.0, MUTED))
    .child(text(placeholder, 12.0, FontWeight::Normal, MUTED));
  if let Some(shortcut) = shortcut {
    row = row.child(text(shortcut, 10.0, FontWeight::Normal, MUTED));
  }
  row
    .height(CONTROL_HEIGHT)
    .padding_horizontal(10.0)
    .padding_vertical(0.0)
    .background(SURFACE_2)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CONTROL_RADIUS)
}

pub(crate) fn toolbar_search_input(value: Signal<String>, placeholder: &str, shortcut: Option<&str>) -> Row {
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(icon("search", 13.0, MUTED))
    .child(
      TextInput::styled(
        value,
        TextStyle {
          font_size: 12.0,
          color: Color::from_hex(TEXT),
          ..Default::default()
        },
      )
      .placeholder(placeholder)
      .placeholder_style(TextStyle {
        font_size: 12.0,
        color: Color::from_hex(MUTED),
        ..Default::default()
      })
      .caret_color(TEXT)
      .single_line()
      .height(20.0)
      .width(170.0)
      .background("#00000000"),
    );
  if let Some(shortcut) = shortcut {
    row = row.child(text(shortcut, 10.0, FontWeight::Normal, MUTED));
  }
  row
    .height(CONTROL_HEIGHT)
    .padding_horizontal(10.0)
    .padding_vertical(0.0)
    .background(SURFACE_2)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(CONTROL_RADIUS)
}

pub(crate) fn text(content: &str, size: f32, weight: FontWeight, color: &str) -> Text {
  Text::styled(
    content,
    crate::layout::text_style::TextStyle {
      font_size: size,
      weight,
      color: Color::from_hex(color),
      ..Default::default()
    },
  )
}

pub(crate) fn icon(name: &str, size: f32, color: &str) -> Text {
  let ch = match name {
    "activity" => '\u{e038}',
    "arrow-right" => '\u{e04b}',
    "box" => '\u{e061}',
    "bug" => '\u{e20c}',
    "image" => '\u{e0f6}',
    "chevron-down" => '\u{e06d}',
    "chevron-right" => '\u{e06f}',
    "circle" => '\u{e083}',
    "component" => '\u{e2ad}',
    "external-link" => '\u{e0b9}',
    "git-branch" => '\u{e0e2}',
    "layers" => '\u{e529}',
    "refresh-cw" => '\u{e145}',
    "search" => '\u{e151}',
    "settings" => '\u{e154}',
    "share-2" => '\u{e156}',
    "circle-play" => '\u{e080}',
    "trash-2" => '\u{e18e}',
    "timer" => '\u{e188}',
    "zap" => '\u{e1b4}',
    _ => '\u{e061}',
  };
  let s: String = core::iter::once(ch).collect();
  Text::styled(
    &s,
    crate::layout::text_style::TextStyle {
      font_family: "lucide".into(),
      font_size: size,
      color: Color::from_hex(color),
      ..Default::default()
    },
  )
}

pub(crate) fn short_tag(tag: &str) -> &str {
  tag.rsplit("::").next().unwrap_or(tag)
}
