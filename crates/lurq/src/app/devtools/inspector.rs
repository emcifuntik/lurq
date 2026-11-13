use super::{
  snapshot::{DevToolsNode, DevToolsNodeKind, FrameProfileSnapshot},
  style::{
    BG, BLUE, BORDER, FILL, GREEN, MUTED, ORANGE, PINK, PRIMARY, SIGNAL_GREEN, SURFACE_2, TEXT, YELLOW, badge, icon,
    short_tag, text,
  },
};
use crate::{
  app::component::ComponentInfo,
  components::{Column, Rect, Row, ScrollVertical, Spacer, Text},
  core::Signal,
  layout::{
    Alignment,
    text_style::{FontStyle, FontWeight, TextStyle},
  },
  node::{CursorIcon, Element, border::Border, color::Color, dimension::Dimension, padding::Padding},
};

pub(crate) struct SectionState {
  collapsed: Vec<String>,
  signal: Signal<Vec<String>>,
}

impl SectionState {
  pub(crate) fn new(collapsed: Vec<String>, signal: Signal<Vec<String>>) -> Self {
    Self { collapsed, signal }
  }

  fn expanded(&self, title: &str) -> bool {
    !self.collapsed.iter().any(|t| t == title)
  }
}

fn toggle_section(signal: &Signal<Vec<String>>, title: &str) {
  let title = title.to_owned();
  signal.update(move |sections| {
    if let Some(index) = sections.iter().position(|t| *t == title) {
      sections.remove(index);
    } else {
      sections.push(title.clone());
    }
  });
}

pub(crate) fn inspector_panel(
  selected: Option<&DevToolsNode>,
  frame: FrameProfileSnapshot,
  collapsed_sections: Vec<String>,
  collapsed_sections_signal: Signal<Vec<String>>,
) -> Element {
  let state = SectionState::new(collapsed_sections, collapsed_sections_signal);
  let title = selected
    .map(|node| short_tag(&node.tag).to_owned())
    .unwrap_or_else(|| "Counter".to_owned());
  let node_id = selected
    .map(|node| format!("NodeId({})", node.id.value()))
    .unwrap_or_else(|| "NodeId(42)".to_owned());
  let child_summary = selected.map(child_summary).unwrap_or_else(|| "1 (Row)".to_owned());
  let kind = selected.map(|node| node.kind).unwrap_or(DevToolsNodeKind::Component);
  let mut details = Column::new().width(FILL);
  match kind {
    DevToolsNodeKind::Component => {
      details = details
        .child(props_section(selected, &state))
        .child(signals_section(selected, &state))
        .child(memos_section(selected, &state))
        .child(context_section(selected, &state))
        .child(node_shape_section(selected, &state));
    }
    DevToolsNodeKind::Element => {
      details = details.child(node_shape_section(selected, &state));
    }
  }
  details = details
    .child(layout_box_section(selected, &state))
    .child(attributes_section(selected, &state));

  Column::new()
    .child(inspector_title(&title, kind))
    .child(
      ScrollVertical::new(
        details
          .child(render_section(&title, &node_id, &child_summary, frame, &state))
          .child(effects_section(selected, &state)),
      )
      .width(FILL)
      .height(FILL)
      .flex(1.0),
    )
    .width(FILL)
    .height(FILL)
    .flex(1.0)
    .background(BG)
    .into()
}

fn inspector_title(title: &str, kind: DevToolsNodeKind) -> Element {
  let (kind_label, kind_color, kind_fill) = match kind {
    DevToolsNodeKind::Component => ("Component", PRIMARY, "#a855f720"),
    DevToolsNodeKind::Element => ("Element", BLUE, "#60a5fa20"),
  };

  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(icon("box", 16.0, BLUE))
    .child(mono_text(title, 14.0, FontWeight::Bold, BLUE))
    .child(badge(kind_label, kind_color, kind_fill))
    .child(Spacer::new().flex(1.0))
    .padding_horizontal(16.0)
    .padding_vertical(10.0)
    .width(FILL)
    .border_bottom(divider())
    .into()
}

fn props_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let props = selected.and_then(|node| node.props.as_ref());
  let count = props.map(|props| props.fields.len()).unwrap_or(0);
  let mut section = Column::new()
    .child(section_header("PROPS", None, ORANGE, &count.to_string(), state))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("PROPS") {
    if let Some(props) = props {
      if props.fields.is_empty() {
        section = section.child(props_empty_row());
      } else {
        section = section.child(prop_struct_row(&short_type_name(&props.type_name)));
        for field in &props.fields {
          section = section.child(prop_info_row(field, 0));
        }
      }
    } else {
      section = section.child(props_empty_row());
    }
  }

  section.into()
}

fn signals_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let signals = selected.map(|node| node.signals.as_slice()).unwrap_or(&[]);
  let mut section = Column::new()
    .child(section_header(
      "SIGNALS",
      Some("zap"),
      SIGNAL_GREEN,
      &signals.len().to_string(),
      state,
    ))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("SIGNALS") {
    if signals.is_empty() {
      section = section.child(signal_empty_row());
    } else {
      for signal in signals {
        section = section.child(signal_row(
          signal.id,
          &signal.type_name,
          signal.formatted_value().as_deref(),
        ));
      }
      section = section.child(Spacer::new().height(4.0));
    }
  }

  section.into()
}

fn render_section(
  title: &str,
  node_id: &str,
  child_summary: &str,
  frame: FrameProfileSnapshot,
  state: &SectionState,
) -> Element {
  let mut section = Column::new()
    .child(section_header("RENDER INFO", Some("refresh-cw"), PRIMARY, "", state))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("RENDER INFO") {
    section = section
      .child(render_info_row(
        "Rendered by",
        if title == "Counter" { "ReactivityDemo" } else { title },
        BLUE,
      ))
      .child(render_info_row("Render count", "7", TEXT))
      .child(render_info_row(
        "Last render",
        &format!("{:.1}ms", frame.total_ms.max(2.1)),
        SIGNAL_GREEN,
      ))
      .child(render_info_row("Dirty", "false", MUTED))
      .child(render_info_row("Children", child_summary, TEXT))
      .child(render_info_row("Node ID", node_id, MUTED))
      .child(Spacer::new().height(6.0));
  }

  section.into()
}

fn context_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let contexts = selected.map(|node| node.contexts.as_slice()).unwrap_or(&[]);
  let mut section = Column::new()
    .child(section_header(
      "CONTEXT",
      Some("share-2"),
      YELLOW,
      &contexts.len().to_string(),
      state,
    ))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("CONTEXT") {
    if contexts.is_empty() {
      section = section.child(context_empty_row());
    } else {
      for context in contexts {
        section = section.child(context_row(context.kind, &context.type_name));
      }
      section = section.child(Spacer::new().height(4.0));
    }
  }

  section.into()
}

fn effects_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let effects = selected.map(|node| node.effects.as_slice()).unwrap_or(&[]);
  let mut section = Column::new()
    .child(section_header(
      "EFFECTS",
      Some("circle-play"),
      PINK,
      &effects.len().to_string(),
      state,
    ))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("EFFECTS") {
    if effects.is_empty() {
      section = section.child(effects_empty_row());
    } else {
      for effect in effects {
        section = section.child(effect_row(effect.id));
      }
      section = section.child(Spacer::new().height(4.0));
    }
  }

  section.into()
}

fn node_shape_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let shape = selected.map(|node| node.shape.as_slice()).unwrap_or(&[]);
  let mut section = Column::new()
    .child(section_header(
      "NODE",
      Some("box"),
      BLUE,
      &shape.len().to_string(),
      state,
    ))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("NODE") {
    if shape.is_empty() {
      section = section.child(node_shape_empty_row());
    } else {
      for row in shape {
        section = section.child(node_shape_row(row, 0));
      }
      section = section.child(Spacer::new().height(4.0));
    }
  }

  section.into()
}

fn memos_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let memos = selected.map(|node| node.memos.as_slice()).unwrap_or(&[]);
  let mut section = Column::new()
    .child(section_header(
      "MEMOS",
      Some("activity"),
      GREEN,
      &memos.len().to_string(),
      state,
    ))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("MEMOS") {
    if memos.is_empty() {
      section = section.child(memo_empty_row());
    } else {
      for memo in memos {
        section = section.child(memo_row(memo.id, &memo.type_name, memo.formatted_value().as_deref()));
      }
      section = section.child(Spacer::new().height(4.0));
    }
  }

  section.into()
}

fn layout_box_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let layout_box = selected.and_then(|node| node.layout_box);
  let mut section = Column::new()
    .child(section_header("LAYOUT", Some("layers"), GREEN, "", state))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("LAYOUT") {
    match layout_box {
      Some(layout_box) => {
        section = section
          .child(box_metric_row("x", layout_box.bounds.x))
          .child(box_metric_row("y", layout_box.bounds.y))
          .child(box_metric_row("relative x", layout_box.relative_x))
          .child(box_metric_row("relative y", layout_box.relative_y))
          .child(box_metric_row("width", layout_box.bounds.width))
          .child(box_metric_row("height", layout_box.bounds.height));
        if let Some(content) = layout_box.content {
          section = section
            .child(box_metric_row("content x", content.x))
            .child(box_metric_row("content y", content.y))
            .child(box_metric_row("content width", content.width))
            .child(box_metric_row("content height", content.height));
        }
        section = section
          .child(box_flag_row("overflow x", layout_box.overflow_x))
          .child(box_flag_row("overflow y", layout_box.overflow_y));
      }
      None => {
        section = section.child(layout_empty_row());
      }
    }

    if let Some(node) = selected {
      section = section
        .child(box_flag_row("hovered", node.hovered))
        .child(box_flag_row("active", node.active))
        .child(box_flag_row("focused", node.focused));
    }
    section = section.child(Spacer::new().height(4.0));
  }

  section.into()
}

fn attributes_section(selected: Option<&DevToolsNode>, state: &SectionState) -> Element {
  let attrs = selected.map(|node| node.attrs.as_slice()).unwrap_or(&[]);
  let mut section = Column::new()
    .child(section_header(
      "ATTRIBUTES",
      Some("settings"),
      YELLOW,
      &attrs.len().to_string(),
      state,
    ))
    .width(FILL)
    .border_bottom(divider());

  if state.expanded("ATTRIBUTES") {
    if attrs.is_empty() {
      section = section.child(attributes_empty_row());
    } else {
      for (name, value) in attrs {
        section = section.child(attribute_row(name, value));
      }
      section = section.child(Spacer::new().height(4.0));
    }
  }

  section.into()
}

fn section_header(title: &str, leading_icon: Option<&str>, color: &str, count: &str, state: &SectionState) -> Element {
  let expanded = state.expanded(title);
  let signal = state.signal.clone();
  let title_owned = title.to_owned();
  let mut row = Row::new().align_items(Alignment::Center).spacing(6.0).child(icon(
    if expanded { "chevron-down" } else { "chevron-right" },
    12.0,
    MUTED,
  ));
  if let Some(icon_name) = leading_icon {
    row = row.child(icon(icon_name, 12.0, color));
  }
  row = row.child(text(title, 11.0, FontWeight::Bold, color));
  if !count.is_empty() {
    row = row.child(badge(count, color, SURFACE_2));
  }
  row
    .padding_horizontal(16.0)
    .padding_vertical(8.0)
    .width(FILL)
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| toggle_section(&signal, &title_owned))
    .into()
}

fn signal_row(id: usize, ty: &str, value: Option<&str>) -> Element {
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .child(Rect::new(6.0, 6.0).background(SIGNAL_GREEN).rounded(3.0))
    .child(Spacer::new().width(8.0))
    .child(mono_text(&format!("#{id}"), 12.0, FontWeight::Medium, TEXT))
    .child(Spacer::new().width(8.0))
    .child(mono_text(
      &format!("Signal<{}>", short_type_name(ty)),
      11.0,
      FontWeight::Normal,
      MUTED,
    ))
    .child(Spacer::new().width(6.0));

  if let Some(value) = value {
    row = row
      .child(mono_text("=", 11.0, FontWeight::Normal, MUTED))
      .child(Spacer::new().width(6.0))
      .child(mono_text(value, 11.0, FontWeight::Medium, SIGNAL_GREEN).nowrap());
  }

  row
    .child(Spacer::new().flex(1.0))
    .padding_custom(Padding {
      top: Dimension::Px(6.0).into(),
      right: Dimension::Px(16.0).into(),
      bottom: Dimension::Px(6.0).into(),
      left: Dimension::Px(40.0).into(),
    })
    .width(FILL)
    .into()
}

fn signal_empty_row() -> Element {
  Row::new()
    .child(italic_mono("No component signals", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn memo_row(id: usize, ty: &str, value: Option<&str>) -> Element {
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .child(Rect::new(6.0, 6.0).background(GREEN).rounded(3.0))
    .child(Spacer::new().width(8.0))
    .child(mono_text(&format!("#{id}"), 12.0, FontWeight::Medium, TEXT))
    .child(Spacer::new().width(8.0))
    .child(mono_text(
      &format!("Memo<{}>", short_type_name(ty)),
      11.0,
      FontWeight::Normal,
      MUTED,
    ))
    .child(Spacer::new().width(6.0));

  if let Some(value) = value {
    row = row
      .child(mono_text("=", 11.0, FontWeight::Normal, MUTED))
      .child(Spacer::new().width(6.0))
      .child(mono_text(value, 11.0, FontWeight::Medium, GREEN).nowrap());
  }

  row
    .child(Spacer::new().flex(1.0))
    .padding_custom(content_padding(6.0, 16.0, 6.0, 40.0))
    .width(FILL)
    .into()
}

fn memo_empty_row() -> Element {
  Row::new()
    .child(italic_mono("No component memos", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn box_metric_row(label: &str, value: f32) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(&format!("{label}:"), 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().width(12.0))
    .child(mono_text(&format_px(value), 11.0, FontWeight::Medium, TEXT).nowrap())
    .padding_custom(content_padding(5.0, 16.0, 5.0, 40.0))
    .width(FILL)
    .into()
}

fn box_flag_row(label: &str, value: bool) -> Element {
  let (text_value, color) = if value { ("true", GREEN) } else { ("false", MUTED) };
  Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(&format!("{label}:"), 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().width(12.0))
    .child(mono_text(text_value, 11.0, FontWeight::Medium, color).nowrap())
    .padding_custom(content_padding(5.0, 16.0, 5.0, 40.0))
    .width(FILL)
    .into()
}

fn layout_empty_row() -> Element {
  Row::new()
    .child(italic_mono("Not laid out yet", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn attribute_row(name: &str, value: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(name, 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().width(6.0))
    .child(mono_text("=", 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().width(6.0))
    .child(mono_text(value, 11.0, FontWeight::Medium, TEXT).nowrap())
    .child(Spacer::new().flex(1.0))
    .padding_custom(content_padding(5.0, 16.0, 5.0, 40.0))
    .width(FILL)
    .into()
}

fn attributes_empty_row() -> Element {
  Row::new()
    .child(italic_mono("No attributes", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn format_px(value: f32) -> String {
  if value.fract().abs() < f32::EPSILON {
    format!("{value:.0}px")
  } else {
    format!("{value:.2}px")
  }
}

fn context_row(kind: crate::app::ctx::ComponentContextKind, ty: &str) -> Element {
  let label = match kind {
    crate::app::ctx::ComponentContextKind::Provided => "provides",
    crate::app::ctx::ComponentContextKind::Consumed => "uses",
  };
  Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(label, 11.0, FontWeight::Medium, YELLOW))
    .child(Spacer::new().width(10.0))
    .child(mono_text(&short_type_name(ty), 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().flex(1.0))
    .padding_custom(content_padding(6.0, 16.0, 6.0, 40.0))
    .width(FILL)
    .into()
}

fn context_empty_row() -> Element {
  Row::new()
    .child(italic_mono("No component context", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn node_shape_empty_row() -> Element {
  Row::new()
    .child(italic_mono("No node shape", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn effects_empty_row() -> Element {
  Row::new()
    .child(italic_mono("No effects", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn effect_row(id: usize) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(Rect::new(6.0, 6.0).background(PINK).rounded(3.0))
    .child(Spacer::new().width(8.0))
    .child(mono_text(&format!("#{id}"), 12.0, FontWeight::Medium, TEXT))
    .child(Spacer::new().width(8.0))
    .child(mono_text("Effect", 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().flex(1.0))
    .padding_custom(content_padding(6.0, 16.0, 6.0, 40.0))
    .width(FILL)
    .into()
}

fn node_shape_row(row: &super::snapshot::DevToolsShapeRow, depth: usize) -> Element {
  let left = 40.0 + depth as f32 * 16.0;
  let mut header = Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(&format!("{}:", row.label), 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().width(12.0));

  if let Some(value) = &row.value {
    header = header.child(mono_text(value, 11.0, FontWeight::Medium, TEXT));
  }

  let bottom = if row.children.is_empty() { 5.0 } else { 2.0 };
  let mut column = Column::new()
    .child(
      header
        .padding_custom(content_padding(5.0, 16.0, bottom, left))
        .width(FILL),
    )
    .width(FILL);

  for child in &row.children {
    column = column.child(node_shape_row(child, depth + 1));
  }

  column.into()
}

fn render_info_row(label: &str, value: &str, value_color: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(text(label, 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().flex(1.0))
    .child(mono_text(value, 11.0, FontWeight::Medium, value_color).nowrap())
    .padding_custom(Padding {
      top: Dimension::Px(5.0).into(),
      right: Dimension::Px(16.0).into(),
      bottom: Dimension::Px(5.0).into(),
      left: Dimension::Px(40.0).into(),
    })
    .width(FILL)
    .into()
}

fn props_empty_row() -> Element {
  Row::new()
    .child(italic_mono("Props = ()  (unit type)", 11.0, MUTED))
    .padding_custom(content_padding(8.0, 16.0, 12.0, 40.0))
    .width(FILL)
    .into()
}

fn prop_struct_row(type_name: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(type_name, 11.0, FontWeight::Bold, ORANGE).nowrap())
    .padding_custom(content_padding(6.0, 16.0, 4.0, 40.0))
    .width(FILL)
    .into()
}

fn prop_info_row(info: &ComponentInfo, depth: usize) -> Element {
  let left = 64.0 + depth as f32 * 16.0;
  let mut column = Column::new()
    .child(prop_field_row(
      info.name(),
      &short_type_name(info.type_name()),
      info.formatted_value(),
      left,
    ))
    .width(FILL);

  for child in info.children() {
    column = column.child(prop_info_row(child, depth + 1));
  }

  column.into()
}

fn prop_field_row(name: &str, type_name: &str, value: Option<&str>, left: f32) -> Element {
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(name, 11.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().width(10.0))
    .child(mono_text(type_name, 11.0, FontWeight::Medium, TEXT).nowrap());

  if let Some(value) = value {
    row = row
      .child(Spacer::new().width(6.0))
      .child(mono_text("=", 11.0, FontWeight::Normal, MUTED))
      .child(Spacer::new().width(6.0))
      .child(mono_text(value, 11.0, FontWeight::Medium, ORANGE).nowrap());
  }

  row
    .padding_custom(content_padding(4.0, 16.0, 4.0, left))
    .width(FILL)
    .into()
}

fn short_type_name(type_name: &str) -> String {
  let mut out = String::new();
  let mut token = String::new();

  for ch in type_name.chars() {
    if matches!(ch, '<' | '>' | ',' | ' ' | '&' | '[' | ']' | '(' | ')') {
      push_short_type_token(&mut out, &mut token);
      out.push(ch);
    } else {
      token.push(ch);
    }
  }
  push_short_type_token(&mut out, &mut token);

  out
}

fn push_short_type_token(out: &mut String, token: &mut String) {
  if token.is_empty() {
    return;
  }
  out.push_str(token.rsplit("::").next().unwrap_or(token));
  token.clear();
}

fn content_padding(top: f32, right: f32, bottom: f32, left: f32) -> Padding {
  Padding {
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

fn italic_mono(content: &str, size: f32, color: &str) -> Text {
  Text::styled(
    content,
    TextStyle {
      font_family: "monospace".into(),
      font_size: size,
      style: FontStyle::Italic,
      color: Color::from_hex(color),
      ..Default::default()
    },
  )
}

fn child_summary(node: &DevToolsNode) -> String {
  match node.children.as_slice() {
    [] => "0".to_owned(),
    [child] => format!("1 ({})", short_tag(&child.tag)),
    children => format!("{}", children.len()),
  }
}

#[cfg(test)]
mod tests {
  use super::short_type_name;

  #[test]
  fn short_type_name_preserves_generic_delimiters() {
    assert_eq!(
      short_type_name("lurq::core::context::ReactiveContext<demo::context_demo::LocaleContext>"),
      "ReactiveContext<LocaleContext>"
    );
    assert_eq!(short_type_name("&str"), "&str");
  }
}
