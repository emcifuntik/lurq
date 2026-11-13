use super::style::{BORDER, FILL, MUTED, ORANGE, SURFACE, SURFACE_2, TEXT, badge, icon, text};
use crate::{
  app::ctx::Ctx,
  components::{Column, Rect, Row, ScrollVertical, Spacer, Stack, Text, TextOverflow},
  core::Signal,
  layout::{
    Alignment,
    text_style::{FontWeight, TextStyle},
  },
  node::{Element, border::Border, color::Color, dimension::Dimension},
  persistent_storage::PersistentStorageSnapshotEntry,
};

const TOOLTIP_LOG_KEY: &str = "demo.profile";

pub(crate) fn persistent_storage_view(
  ctx: &mut Ctx,
  entries: &[PersistentStorageSnapshotEntry],
  active_type_tooltip: Signal<Option<String>>,
) -> Element {
  let mut body = Column::new().width(FILL);
  for (index, entry) in entries.iter().enumerate() {
    body = body.child(storage_row(ctx, entry, index, active_type_tooltip.clone()));
  }
  if entries.is_empty() {
    body = body.child(
      text("No persistent values", 11.0, FontWeight::Normal, MUTED)
        .padding_horizontal(16.0)
        .padding_vertical(10.0),
    );
  }

  Column::new()
    .child(storage_summary(entries))
    .child(storage_header())
    .child(ScrollVertical::new(body).height(FILL).width(FILL).flex(1.0))
    .width(FILL)
    .height(FILL)
    .background(SURFACE)
    .into()
}

pub(crate) fn persistent_storage_error_view(message: &str) -> Element {
  Column::new()
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(icon("box", 14.0, ORANGE))
        .child(text("Persistent Storage", 12.0, FontWeight::Bold, TEXT))
        .padding_horizontal(16.0)
        .padding_vertical(12.0)
        .width(FILL)
        .border_bottom(divider()),
    )
    .child(
      text(message, 11.0, FontWeight::Normal, ORANGE)
        .padding_horizontal(16.0)
        .padding_vertical(12.0),
    )
    .width(FILL)
    .height(FILL)
    .background(SURFACE)
    .into()
}

fn storage_summary(entries: &[PersistentStorageSnapshotEntry]) -> Element {
  let bytes = entries.iter().map(|entry| entry.byte_len).sum::<usize>();
  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(icon("box", 14.0, ORANGE))
    .child(text("Persistent Storage", 12.0, FontWeight::Bold, TEXT))
    .child(badge(&format!("{} keys", entries.len()), MUTED, SURFACE_2))
    .child(badge(&format!("{bytes} bytes"), MUTED, SURFACE_2))
    .child(Spacer::new().flex(1.0))
    .padding_horizontal(16.0)
    .padding_vertical(10.0)
    .width(FILL)
    .border_bottom(divider())
    .into()
}

fn storage_header() -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(header_cell("KEY", 1.5))
    .child(header_cell("TYPE", 0.8))
    .child(header_cell("VALUE", 2.9))
    .child(header_cell("BYTES", 0.5))
    .padding_custom(padding(8.0, 16.0, 8.0, 16.0))
    .width(FILL)
    .border_bottom(divider())
    .into()
}

fn storage_row(
  _ctx: &mut Ctx,
  entry: &PersistentStorageSnapshotEntry,
  index: usize,
  active_type_tooltip: Signal<Option<String>>,
) -> Element {
  let background = if index % 2 == 1 { SURFACE_2 } else { "#00000000" };
  Row::new()
    .align_items(Alignment::Center)
    .child(Rect::new(6.0, 6.0).background(ORANGE).rounded(3.0))
    .child(Spacer::new().width(8.0))
    .child(
      mono_text(&entry.key, 11.0, FontWeight::Medium, TEXT)
        .flex(1.5)
        .nowrap()
        .text_overflow(TextOverflow::Elipsis),
    )
    .child(type_cell(entry, active_type_tooltip))
    .child(mono_text(&entry.value, 11.0, FontWeight::Normal, TEXT).flex(2.9))
    .child(
      mono_text(&entry.byte_len.to_string(), 11.0, FontWeight::Normal, MUTED)
        .flex(0.5)
        .nowrap(),
    )
    .padding_custom(padding(7.0, 16.0, 7.0, 16.0))
    .width(FILL)
    .background(background)
    .into()
}

fn type_cell(entry: &PersistentStorageSnapshotEntry, active_type_tooltip: Signal<Option<String>>) -> Element {
  let show_tooltip = entry.full_type_name != entry.type_name;
  let should_log = entry.key == TOOLTIP_LOG_KEY;
  let full_type_name = entry.full_type_name.clone();
  let is_active = active_type_tooltip.get().as_deref() == Some(entry.full_type_name.as_str());
  if show_tooltip && should_log {
    log_tooltip("devtools", "render", &entry.type_name, &entry.full_type_name, is_active);
  }
  let mut label = Row::new()
    .align_items(Alignment::Center)
    .child(
      mono_text(&entry.type_name, 10.0, FontWeight::Normal, MUTED)
        .flex(1.0)
        .nowrap()
        .text_overflow(TextOverflow::Elipsis),
    )
    .width(FILL)
    .height(22.0)
    .hovered(|style| style.background("#252525"));

  if show_tooltip {
    label = label
      .on_mouse_enter({
        let active_type_tooltip = active_type_tooltip.clone();
        let full_type_name = full_type_name.clone();
        let type_name = entry.type_name.clone();
        move || {
          if should_log {
            log_tooltip("devtools", "enter", &type_name, &full_type_name, true);
          }
          active_type_tooltip.set(Some(full_type_name.clone()));
        }
      })
      .on_mouse_leave({
        let active_type_tooltip = active_type_tooltip.clone();
        let full_type_name = full_type_name.clone();
        let type_name = entry.type_name.clone();
        move || {
          if should_log {
            log_tooltip("devtools", "leave", &type_name, &full_type_name, false);
          }
          active_type_tooltip.set(None);
        }
      });
  }

  let mut cell = Stack::new().child(label).flex(0.8).min_height(22.0).overflow_visible();
  if show_tooltip && is_active {
    if should_log {
      log_tooltip(
        "devtools",
        "inline-tooltip",
        &entry.type_name,
        &entry.full_type_name,
        true,
      );
    }
    cell = cell.child(type_tooltip(&entry.full_type_name).absolute_position(0.0, -34.0));
  }

  cell.into()
}

fn log_tooltip(scope: &str, event: &str, type_name: &str, full_type_name: &str, open: bool) {
  tracing::info!(
    target: "lurq::persistent_storage_tooltip",
    scope,
    event,
    type_name,
    full_type_name,
    open,
    "persistent storage type tooltip"
  );
  eprintln!(
    "[persistent-storage-tooltip] scope={scope} event={event} type={type_name} full={full_type_name} open={open}"
  );
}

fn type_tooltip(full_type_name: &str) -> Row {
  Row::new()
    .key("persistent-storage-type-tooltip")
    .child(
      mono_text(full_type_name, 10.0, FontWeight::Medium, TEXT)
        .nowrap()
        .text_overflow(TextOverflow::Elipsis),
    )
    .max_width(520.0)
    .padding_horizontal(8.0)
    .padding_vertical(6.0)
    .background(SURFACE_2)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(4.0)
}

fn header_cell(label: &str, flex: f32) -> Element {
  text(label, 10.0, FontWeight::Bold, MUTED).flex(flex).into()
}

fn padding(top: f32, right: f32, bottom: f32, left: f32) -> crate::node::padding::Padding {
  crate::node::padding::Padding {
    top: Dimension::Px(top).into(),
    right: Dimension::Px(right).into(),
    bottom: Dimension::Px(bottom).into(),
    left: Dimension::Px(left).into(),
  }
}

fn divider() -> Border {
  Border::inside(1.0, Color::from_hex(BORDER))
}

fn mono_text(content: &str, size: f32, weight: FontWeight, color: &str) -> Text {
  Text::styled(
    content,
    TextStyle {
      font_family: "monospace".into(),
      font_size: size,
      weight,
      color: Color::from_hex(color),
      ..Default::default()
    },
  )
}
