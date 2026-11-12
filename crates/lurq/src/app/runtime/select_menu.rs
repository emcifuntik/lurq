//! The open `Select` menu: building its overlay each pass and the keyboard
//! defaults of a focused select.

use std::{sync::Arc, time::Instant};

use super::{Tree, clamp_overlay_position, find_node_by_id, overlay_position, resolve_overlay_collision};
use crate::{
  app::{
    ctx::{CollisionStrategy, Placement},
    events::{MouseButton, MouseEvent},
  },
  core::ElementRect,
  layout::{Alignment, Size, layout_kind::Justify, layout_result::LayoutResult},
  node::{
    EventHandler, Node, SelectAdornmentState, SelectCheckmarkPosition, SelectPartStyle, SelectStyle, SpacingValue,
    Style, SyntheticNodeRole,
    border::ThemedBorderRadius,
    dimension::Dimension,
    node_kind::{NodeKind, SelectState, TextOverflow},
    radius_value::RadiusValue,
    select_style::SelectOptionState,
  },
};

/// Minimum option row height when the option part sets none.
const SELECT_OPTION_ROW_HEIGHT: f32 = 34.0;

/// Builds the menu of an open select below (or, without room, above) the
/// trigger at `bounds`. `measure` lays the menu out so its real height places
/// it and the option the state asks to reveal is scrolled into view.
pub(super) fn build_select_menu(
  state: &SelectState,
  bounds: ElementRect,
  viewport: Size,
  measure: &mut dyn FnMut(&Node) -> LayoutResult,
) -> Node {
  let style = state.style();
  let labels = state.labels();
  let rows = labels
    .iter()
    .enumerate()
    .map(|(index, label)| option_row(state, &style, index, label, labels.len(), measure))
    .collect();

  // The menu's padding goes on the scrolled list, so it scrolls with the
  // options like CSS padding on a scroll container.
  let mut menu_part = style.menu.clone();
  let mut list = Node::column(0.0, Alignment::Start, rows).width(Dimension::Pct(100.0));
  if let Some(padding) = menu_part.padding.take() {
    list = list.padding_custom(padding);
  }
  let width = bounds.width.max(style.min_menu_width).min(viewport.width.max(0.0));
  let mut menu = crate::node::dsl::scroll_vertical(list)
    .with_scroll_state(state.menu_scroll())
    .apply_select_part(&menu_part)
    .width(Dimension::Px(width))
    .max_height(Dimension::Px(style.max_menu_height));
  if let Some(scrollbar) = &style.menu_scrollbar {
    menu = menu.scrollbar(scrollbar.clone());
  }
  menu.set_tag_name("SelectMenu");
  menu.set_synthetic_role(SyntheticNodeRole::SelectMenu);

  let measured = measure(&menu);
  if let Some(index) = state.take_reveal()
    && let Some((start, end)) = option_extent(&measured, index)
  {
    state.menu_scroll().reveal_y_pending(start, end);
  }

  let overlay_size = Size::new(width, measured.size.height);
  let placement = resolve_overlay_collision(
    Placement::BottomStart,
    bounds,
    overlay_size,
    viewport,
    0.0,
    style.menu_gap,
    CollisionStrategy::FlipThenClamp,
  );
  let (x, y) = overlay_position(bounds, overlay_size, placement, 0.0, style.menu_gap);
  let (x, y) = clamp_overlay_position(x, y, overlay_size, viewport);
  menu.absolute_positioned(x, y, Some(Dimension::Px(width)), None)
}

/// The vertical extent of option `index` in the menu's scroll content.
fn option_extent(menu: &LayoutResult, index: usize) -> Option<(f32, f32)> {
  let list = menu.children.first()?;
  let row = list.result.children.get(index)?;
  Some((row.offset.y, row.offset.y + row.result.size.height))
}

fn option_row(
  state: &SelectState,
  style: &SelectStyle,
  index: usize,
  label: &str,
  count: usize,
  measure: &mut dyn FnMut(&Node) -> LayoutResult,
) -> Node {
  let option = SelectOptionState {
    hovered: false,
    highlighted: state.highlighted() == Some(index),
    selected: state.is_selected(index),
    disabled: state.is_disabled(index),
  };
  let mut part = style.resolved_option(option);
  apply_select_menu_edge_radius(&mut part, &style.menu, index, count);

  let adornment = SelectAdornmentState {
    selected: option.selected,
    highlighted: option.highlighted && !option.disabled,
    disabled: option.disabled,
    in_field: false,
    text_color: part.text_color.clone(),
  };
  let (leading, trailing) = state.adornments(index);
  let slots = [leading, trailing].map(|build| build.map(|build| build(&adornment).into_node().flex_shrink(0.0)));
  let content = option_content(&part, style, label, state.detail(index), slots, measure);
  let multiple = state.multiple();
  let (gap, children) = if style.shows_checkmark(multiple) {
    let slot = checkmark_slot(style, &part, option.selected);
    let children = match style.checkmark_position {
      SelectCheckmarkPosition::Leading => vec![slot, content],
      SelectCheckmarkPosition::Trailing => vec![content, slot],
    };
    (style.checkmark_gap, children)
  } else {
    (SpacingValue::from(0.0), vec![content])
  };

  let mut row = Node::row(gap, Alignment::Center, children)
    .width(Dimension::Pct(100.0))
    .apply_select_part(&part);
  if part.min_height.is_none() {
    row = row.min_height(SELECT_OPTION_ROW_HEIGHT);
  }
  row.set_tag_name("SelectOption");
  if option.disabled {
    return row;
  }

  let commit_state = state.clone();
  row
    .events
    .on_mouse_down
    .push(EventHandler::new(move |event: &MouseEvent| {
      if event.button == MouseButton::Left {
        commit_state.commit(index);
      }
    }));
  let hovered = style.resolved_option(SelectOptionState {
    hovered: true,
    ..option
  });
  row.hovered_style(hover_style(&hovered))
}

/// An option's content: the label between its leading and trailing elements,
/// and the detail line under the label when the option has one.
///
/// The elements are centred on the label's line. Beside a detail line that
/// takes two rows: the elements with the label, then the detail between
/// spacers as wide as the elements, which `measure` sizes, so the detail
/// starts and ends where the label does.
fn option_content(
  part: &SelectPartStyle,
  style: &SelectStyle,
  label: &str,
  detail: Option<Arc<str>>,
  [leading, trailing]: [Option<Node>; 2],
  measure: &mut dyn FnMut(&Node) -> LayoutResult,
) -> Node {
  let line = |node: Node| {
    node
      .text_wrap(false)
      .text_overflow(TextOverflow::Elipsis)
      .min_width(0.0)
  };
  let label = line(part.text_node(label));
  let detail = detail.map(|detail| {
    let mut detail_part = part.clone();
    detail_part.merge_from(&style.option_detail);
    line(detail_part.text_node(&detail))
  });
  let adorned = leading.is_some() || trailing.is_some();
  let gap = style.option_adornment_gap;
  let around = |before: Option<Node>, text: Node, after: Option<Node>, align: Alignment| {
    let children = before.into_iter().chain([text.flex(1.0)]).chain(after).collect();
    Node::row(gap, align, children)
  };

  match (detail, adorned) {
    (None, false) => label.flex(1.0),
    (None, true) => around(leading, label, trailing, Alignment::Center)
      .min_width(0.0)
      .flex(1.0),
    (Some(detail), false) => Node::column(
      0.0,
      Alignment::Start,
      vec![label.width(Dimension::Pct(100.0)), detail.width(Dimension::Pct(100.0))],
    )
    .min_width(0.0)
    .flex(1.0),
    (Some(detail), true) => {
      let mut spacer = |slot: &Option<Node>| {
        slot.as_ref().map(|element| {
          Node::row(0.0, Alignment::Start, Vec::new())
            .width(Dimension::Px(measure(element).size.width))
            .flex_shrink(0.0)
        })
      };
      let (before, after) = (spacer(&leading), spacer(&trailing));
      let full = |row: Node| row.width(Dimension::Pct(100.0));
      Node::column(
        0.0,
        Alignment::Start,
        vec![
          full(around(leading, label, trailing, Alignment::Center)),
          full(around(before, detail, after, Alignment::Start)),
        ],
      )
      .min_width(0.0)
      .flex(1.0)
    }
  }
}

/// The checkmark slot every option reserves, so labels line up whether or not
/// the option is selected.
fn checkmark_slot(style: &SelectStyle, part: &SelectPartStyle, selected: bool) -> Node {
  let icon = selected.then(|| {
    style.checkmark.build(
      part.text_style(),
      style.checkmark_size,
      style.checkmark_color.as_ref().or(part.text_color.as_ref()),
    )
  });
  let justify = match style.checkmark_position {
    SelectCheckmarkPosition::Leading => Justify::Start,
    SelectCheckmarkPosition::Trailing => Justify::End,
  };
  Node::row(0.0, Alignment::Center, icon.into_iter().collect())
    .justify(justify)
    .width(Dimension::Px(style.checkmark_slot_width()))
    .flex_shrink(0.0)
}

/// The pointer-hover paint of a row: fill, border and box shadow.
fn hover_style(hovered: &SelectPartStyle) -> Style {
  let mut style = Style::new();
  if let Some(background) = &hovered.background {
    style = style.background(background.clone());
  }
  if let Some(border) = &hovered.border {
    style = style.border_custom(border.clone());
  }
  if let Some(shadow) = &hovered.box_shadow {
    style = style.box_shadow(shadow.clone());
  }
  style
}

fn apply_select_menu_edge_radius(part: &mut SelectPartStyle, menu: &SelectPartStyle, index: usize, count: usize) {
  if count == 0 || part.border_radius.is_some() {
    return;
  }
  let Some(menu_radius) = menu.border_radius else {
    return;
  };
  let zero = RadiusValue::Px(0.0);
  let first = index == 0;
  let last = index + 1 == count;
  part.border_radius = Some(ThemedBorderRadius::new(
    if first { menu_radius.top_left } else { zero },
    if first { menu_radius.top_right } else { zero },
    if last { menu_radius.bottom_right } else { zero },
    if last { menu_radius.bottom_left } else { zero },
  ));
}

/// A key press on the focused element, as the select defaults read it.
#[derive(Clone, Copy)]
pub(super) struct SelectKey<'a> {
  pub(super) key: &'a str,
  pub(super) code: &'a str,
  pub(super) alt: bool,
  pub(super) ctrl: bool,
  pub(super) meta: bool,
}

impl SelectKey<'_> {
  fn is(&self, name: &str) -> bool {
    self.key == name || self.code == name
  }

  /// The character a printable key types, for type-ahead.
  fn typed_char(&self) -> Option<char> {
    if self.ctrl || self.alt || self.meta {
      return None;
    }
    let mut chars = self.key.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && !ch.is_control()).then_some(ch)
  }
}

impl Tree {
  /// Keyboard defaults of a focused select. Closed: ArrowDown, ArrowUp
  /// (with or without Alt), Enter and Space open it with the selected option
  /// highlighted. Open: arrows move the highlight over enabled options,
  /// Home/End jump to the first/last one, typing jumps by label prefix, Enter
  /// or Space commits, and Escape or Tab close without a change. Focus stays
  /// on the select throughout.
  pub(super) fn dispatch_select_key(&mut self, key: SelectKey<'_>) -> bool {
    let Some(focused) = self.focused_node else {
      return false;
    };
    let Some(root) = &self.root else {
      return false;
    };
    let Some(node) = find_node_by_id(root, focused) else {
      return false;
    };
    let NodeKind::Select { state } = node.node_kind() else {
      return false;
    };
    if state.is_open() {
      handle_open_select_key(state, key)
    } else {
      handle_closed_select_key(state, key)
    }
  }
}

fn handle_closed_select_key(state: &SelectState, key: SelectKey<'_>) -> bool {
  let opens = key.is("ArrowDown") || key.is("ArrowUp") || key.is("Enter") || key.key == " " || key.code == "Space";
  if opens {
    state.open_with_highlight();
  }
  opens
}

fn handle_open_select_key(state: &SelectState, key: SelectKey<'_>) -> bool {
  let now = Instant::now();
  if key.is("Escape") || key.is("Tab") {
    state.set_open(false);
  } else if key.is("ArrowDown") {
    state.move_highlight(1);
  } else if key.is("ArrowUp") {
    state.move_highlight(-1);
  } else if key.is("Home") {
    state.highlight_edge(false);
  } else if key.is("End") {
    state.highlight_edge(true);
  } else if key.is("Enter") {
    state.activate();
  } else if key.key == " " || key.code == "Space" {
    if state.is_typing_ahead(now) {
      state.type_ahead(' ', now);
    } else {
      state.activate();
    }
  } else if let Some(ch) = key.typed_char() {
    state.type_ahead(ch, now);
  } else {
    return false;
  }
  true
}
