//! What the pages that exercise the style engine by hand are built from: a page, its cards, and the controls
//! that change what a card shows.

use lurq::{
  components::{Column, Row, Slider, Text},
  core::Signal,
  layout::{Alignment, text_style::FontWeight},
  node::{CursorIcon, Element, color::Color, dimension::Dimension},
};

use crate::style::{BG, BORDER, PRIMARY, SURFACE, SURFACE_DARK, TEXT, TEXT_MUTED, text};

pub(crate) const FILL: Dimension = Dimension::Pct(100.0);

/// A page: its title, what to try on it, and its cards.
pub(crate) fn page(title: &str, try_this: &str) -> Column {
  Column::new()
    .spacing(20.0)
    .padding(32.0)
    .width(FILL)
    .background(BG)
    .child(text(title, 28.0, FontWeight::Bold, TEXT).width(FILL))
    .child(note(try_this))
}

/// A card with a title, for one thing to look at.
pub(crate) fn card(title: &str) -> Column {
  Column::new()
    .spacing(14.0)
    .padding(20.0)
    .width(FILL)
    .background(SURFACE)
    .rounded(8.0)
    .border_inside(1.0, Color::from_hex(BORDER))
    .child(text(title, 16.0, FontWeight::Bold, TEXT))
}

pub(crate) fn note(content: &str) -> Text {
  text(content, 12.0, FontWeight::Normal, TEXT_MUTED).width(FILL)
}

pub(crate) fn label(content: &str) -> Text {
  text(content, 12.0, FontWeight::Medium, TEXT_MUTED)
}

pub(crate) fn body(content: &str) -> Text {
  text(content, 13.0, FontWeight::Normal, TEXT)
}

/// A pill to click: lit while `on`.
pub(crate) fn chip(caption: &str, on: bool, on_click: impl Fn() + Send + Sync + 'static) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .padding_horizontal(12.0)
    .height(28.0)
    .rounded(14.0)
    .background(if on { PRIMARY } else { SURFACE_DARK })
    .border_inside(1.0, Color::from_hex(if on { PRIMARY } else { BORDER }))
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| on_click())
    .child(text(
      caption,
      12.0,
      FontWeight::Medium,
      if on { "#ffffff" } else { TEXT },
    ))
}

/// A chip that turns `value` on and off.
pub(crate) fn toggle(caption: &str, value: &Signal<bool>) -> Row {
  let flipped = value.clone();
  chip(caption, value.get(), move || flipped.update(|on| *on = !*on))
}

/// One chip per option; the one whose index `value` holds is lit.
pub(crate) fn choice<T>(options: &[(&str, T)], value: &Signal<usize>) -> Row {
  let current = value.get();
  let mut row = Row::new().spacing(6.0);
  for (index, (caption, _)) in options.iter().enumerate() {
    let chosen = value.clone();
    row = row.child(chip(caption, index == current, move || chosen.set(index)));
  }
  row
}

/// The option whose index `value` holds.
pub(crate) fn chosen<T: Copy>(options: &[(&str, T)], value: &Signal<usize>) -> T {
  options[value.get().min(options.len() - 1)].1
}

/// A slider with its name and its value beside it.
pub(crate) fn slider(caption: &str, value: &Signal<i32>, min: i32, max: i32) -> Row {
  Row::new()
    .spacing(10.0)
    .align_items(Alignment::Center)
    .child(label(caption).width(96.0))
    .child(
      Slider::new(value.clone())
        .range(min, max)
        .width(180.0)
        .height(14.0)
        .background("#475569")
        .rounded(7.0)
        .cursor(CursorIcon::Pointer),
    )
    .child(body(&value.get().to_string()).width(40.0))
}

/// A row of controls.
pub(crate) fn controls(items: impl IntoIterator<Item = impl Into<Element>>) -> Row {
  Row::new()
    .spacing(14.0)
    .align_items(Alignment::Center)
    .width(FILL)
    .with_children(items)
}

/// A number the page counts, with what it counts.
pub(crate) fn counted(caption: &str, count: u32) -> Row {
  Row::new()
    .spacing(6.0)
    .align_items(Alignment::Center)
    .child(label(caption))
    .child(text(&count.to_string(), 14.0, FontWeight::Bold, TEXT))
}
