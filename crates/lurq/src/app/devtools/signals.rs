use std::time::{SystemTime, UNIX_EPOCH};

use super::{
  DevToolsNode, DevToolsSnapshot,
  style::{
    BLUE, BORDER, FILL, MUTED, PINK, SELECTED, SIGNAL_GREEN, SURFACE, SURFACE_2, TEXT, YELLOW, icon, short_tag, text,
  },
};
use crate::{
  components::{Column, Rect, Row, ScrollVertical, Spacer, Text},
  core::{Signal, Store},
  layout::{
    Alignment,
    layout_kind::Justify,
    text_style::{FontWeight, TextStyle},
  },
  node::{CursorIcon, Element, border::Border, color::Color, dimension::Dimension},
};

#[derive(Clone, Debug, Default, PartialEq, crate::DevtoolsInspectable)]
pub(crate) struct SignalsRecordingState {
  recording: bool,
  previous_values: Vec<RecordedSignalValue>,
  histories: Vec<RecordedSignalHistory>,
}

#[derive(Clone, Debug, PartialEq, crate::DevtoolsInspectable)]
struct RecordedSignalValue {
  key: String,
  value: String,
  trigger: String,
}

#[derive(Clone, Debug, PartialEq, crate::DevtoolsInspectable)]
struct RecordedSignalHistory {
  key: String,
  entries: Vec<SignalHistoryRow>,
}

#[derive(Clone, Debug, PartialEq)]
struct SignalRow {
  id: usize,
  kind: ReactiveKind,
  type_name: String,
  value: String,
  owner: String,
  history: Vec<SignalHistoryRow>,
  subscriber_count: usize,
}

#[derive(Clone, Debug, PartialEq, crate::DevtoolsInspectable)]
struct SignalHistoryRow {
  timestamp: String,
  from_value: String,
  to_value: String,
  trigger: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReactiveKind {
  Signal,
  Memo,
  Effect,
}

impl ReactiveKind {
  fn color(self) -> &'static str {
    match self {
      Self::Signal => SIGNAL_GREEN,
      Self::Memo => YELLOW,
      Self::Effect => PINK,
    }
  }

  fn label(self) -> &'static str {
    match self {
      Self::Signal => "Signal",
      Self::Memo => "Memo",
      Self::Effect => "Effect",
    }
  }

  fn rank(self) -> u8 {
    match self {
      Self::Signal => 0,
      Self::Memo => 1,
      Self::Effect => 2,
    }
  }
}

impl SignalRow {
  fn key(&self) -> String {
    format!("{}:{}", self.kind.label(), self.id)
  }

  fn type_display(&self) -> String {
    if self.kind == ReactiveKind::Effect {
      "Effect".to_owned()
    } else {
      format!("{}<{}>", self.kind.label(), self.type_name)
    }
  }
}

impl SignalsRecordingState {
  pub(crate) fn recording(&self) -> bool {
    self.recording
  }

  pub(crate) fn set_recording(&mut self, recording: bool) {
    if self.recording != recording {
      self.previous_values.clear();
    }
    self.recording = recording;
  }

  pub(crate) fn clear(&mut self) {
    self.previous_values.clear();
    self.histories.clear();
  }

  fn history_for(&self, key: &str) -> Vec<SignalHistoryRow> {
    self
      .histories
      .iter()
      .find(|history| history.key == key)
      .map(|history| history.entries.iter().rev().take(8).cloned().collect())
      .unwrap_or_default()
  }

  fn record_snapshot(&mut self, snapshot: &DevToolsSnapshot) {
    let current_values = collect_signal_rows(snapshot)
      .into_iter()
      .filter(|row| row.kind != ReactiveKind::Effect)
      .map(|row| RecordedSignalValue {
        key: row.key(),
        value: row.value,
        trigger: match row.kind {
          ReactiveKind::Signal => "change",
          ReactiveKind::Memo => "recompute",
          ReactiveKind::Effect => "effect",
        }
        .to_owned(),
      })
      .collect::<Vec<_>>();

    for current in &current_values {
      let previous = self.previous_values.iter().find(|previous| previous.key == current.key);
      let Some(previous) = previous else {
        continue;
      };
      if previous.value == current.value {
        continue;
      }

      let history = self.histories.iter_mut().find(|history| history.key == current.key);
      if let Some(history) = history {
        history.entries.push(SignalHistoryRow {
          timestamp: current_compact_timestamp(),
          from_value: previous.value.clone(),
          to_value: current.value.clone(),
          trigger: current.trigger.clone(),
        });
        trim_history(&mut history.entries);
      } else {
        self.histories.push(RecordedSignalHistory {
          key: current.key.clone(),
          entries: vec![SignalHistoryRow {
            timestamp: current_compact_timestamp(),
            from_value: previous.value.clone(),
            to_value: current.value.clone(),
            trigger: current.trigger.clone(),
          }],
        });
      }
    }

    self.previous_values = current_values;
  }
}

pub(crate) fn record_signal_changes(snapshot: &DevToolsSnapshot, recording_state: &Store<SignalsRecordingState>) {
  let next = recording_state.with(|state| {
    let mut next = state.clone();
    next.record_snapshot(snapshot);
    if next == *state { None } else { Some(next) }
  });
  if let Some(next) = next {
    recording_state.set(next);
  }
}

pub(crate) fn signals_view(
  snapshot: &DevToolsSnapshot,
  selected_signal_key: Option<String>,
  selected_signal_signal: Signal<Option<String>>,
  recording_state: &SignalsRecordingState,
) -> Element {
  let mut signals = collect_signal_rows(snapshot);
  for signal in &mut signals {
    signal.history = recording_state.history_for(&signal.key());
  }
  let selected_key = selected_signal_key.or_else(|| signals.first().map(SignalRow::key));
  let selected_signal = selected_key
    .as_deref()
    .and_then(|key| signals.iter().find(|signal| signal.key() == key));

  Row::new()
    .child(signals_list_panel(
      &signals,
      selected_key.as_deref(),
      selected_signal_signal,
    ))
    .child(signal_detail_panel(selected_signal))
    .height(FILL)
    .width(FILL)
    .flex(1.0)
    .into()
}

fn signals_list_panel(
  signals: &[SignalRow],
  selected_key: Option<&str>,
  selected_signal_signal: Signal<Option<String>>,
) -> Element {
  let mut body = Column::new().width(FILL);
  for (index, signal) in signals.iter().enumerate() {
    body = body.child(signal_row(
      signal,
      index,
      selected_key == Some(signal.key().as_str()),
      selected_signal_signal.clone(),
    ));
  }
  if signals.is_empty() {
    body = body.child(
      text("No signals captured", 11.0, FontWeight::Normal, MUTED)
        .padding_horizontal(16.0)
        .padding_vertical(10.0),
    );
  }

  Column::new()
    .child(signals_header())
    .child(ScrollVertical::new(body).height(FILL).width(FILL).flex(1.0))
    .width(560.0)
    .height(FILL)
    .background(SURFACE)
    .border_right(divider())
    .into()
}

fn signals_header() -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .child(header_cell("SIGNAL", 160.0))
    .child(header_cell("TYPE", 120.0))
    .child(header_cell("VALUE", 80.0))
    .child(header_cell("SUBS", 50.0))
    .child(header_cell("OWNER", 130.0))
    .padding_custom(padding(8.0, 16.0, 8.0, 16.0))
    .width(FILL)
    .border_bottom(divider())
    .into()
}

fn signal_row(
  signal: &SignalRow,
  index: usize,
  selected: bool,
  selected_signal_signal: Signal<Option<String>>,
) -> Element {
  let color = signal.kind.color();
  let background = if selected {
    SELECTED
  } else if index % 2 == 1 {
    SURFACE_2
  } else {
    "#00000000"
  };
  let signal_key = signal.key();
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .child(Rect::new(6.0, 6.0).background(color).rounded(3.0))
    .child(Spacer::new().width(6.0))
    .child(mono_text(&format!("#{}", signal.id), 11.0, FontWeight::Medium, TEXT).width(148.0))
    .child(mono_text(&signal.type_display(), 10.0, FontWeight::Normal, MUTED).width(120.0))
    .child(value_badge_with_color(&signal.value, color).width(80.0))
    .child(mono_text(&signal.subscriber_count.to_string(), 11.0, FontWeight::Normal, MUTED).width(50.0))
    .child(mono_text(&signal.owner, 10.0, FontWeight::Normal, BLUE).width(130.0))
    .padding_custom(padding(7.0, 16.0, 7.0, 16.0))
    .width(FILL)
    .background(background)
    .cursor(CursorIcon::Pointer)
    .on_click(move |_| selected_signal_signal.set(Some(signal_key.clone())));

  if selected {
    row = row.border_left(Border::inside(2.0, Color::from_hex(color)));
  }

  row.into()
}

fn signal_detail_panel(selected: Option<&SignalRow>) -> Element {
  Column::new()
    .child(dependency_header(selected))
    .child(dependency_graph(selected))
    .child(history_section(selected))
    .height(FILL)
    .width(FILL)
    .flex(1.0)
    .into()
}

fn dependency_header(selected: Option<&SignalRow>) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(icon("git-branch", 14.0, SIGNAL_GREEN))
    .child(text("Dependency Graph", 12.0, FontWeight::Bold, TEXT))
    .child(Spacer::new().flex(1.0))
    .child(mono_text(
      selected
        .map(|signal| format!("#{}", signal.id))
        .as_deref()
        .unwrap_or("-"),
      11.0,
      FontWeight::Normal,
      selected.map(|signal| signal.kind.color()).unwrap_or(SIGNAL_GREEN),
    ))
    .padding_custom(padding(10.0, 16.0, 10.0, 16.0))
    .width(FILL)
    .border_bottom(divider())
    .into()
}

fn dependency_graph(selected: Option<&SignalRow>) -> Element {
  let Some(signal) = selected else {
    return empty_graph("Select a signal");
  };

  Column::new()
    .child(Spacer::new().height(24.0))
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .justify(Justify::Center)
        .spacing(12.0)
        .child(graph_node(&signal.owner, "Component", BLUE, None))
        .child(graph_relation("owns"))
        .child(graph_node(
          &format!("#{}", signal.id),
          &signal.type_display(),
          signal.kind.color(),
          Some(&signal.value),
        ))
        .child(graph_relation("notifies"))
        .child(graph_node(
          "subscribers",
          "tracked reads",
          YELLOW,
          Some(&signal.subscriber_count.to_string()),
        ))
        .width(FILL),
    )
    .child(Spacer::new().flex(1.0))
    .height(220.0)
    .width(FILL)
    .background(SURFACE)
    .border_bottom(divider())
    .into()
}

fn empty_graph(message: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(text(message, 12.0, FontWeight::Normal, MUTED))
    .height(220.0)
    .width(FILL)
    .background(SURFACE)
    .border_bottom(divider())
    .into()
}

fn graph_node(label: &str, kind: &str, color: &str, value: Option<&str>) -> Column {
  let mut node = Column::new()
    .align_items(Alignment::Center)
    .spacing(4.0)
    .child(mono_text(label, 11.0, FontWeight::Bold, color))
    .child(mono_text(kind, 9.0, FontWeight::Normal, MUTED));
  if let Some(value) = value {
    node = node.child(value_badge_with_color(value, color));
  }
  node
    .padding_custom(padding(10.0, 16.0, 10.0, 16.0))
    .width(132.0)
    .height(72.0)
    .background(Color::from_hex(&with_alpha(color, "18")))
    .border_inside(1.0, Color::from_hex(color))
    .rounded(6.0)
}

fn graph_relation(label: &str) -> Column {
  Column::new()
    .align_items(Alignment::Center)
    .spacing(7.0)
    .child(graph_label(label))
    .child(Rect::new(56.0, 1.0).background(MUTED).opacity(0.35))
    .width(66.0)
}

fn graph_label(label: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(mono_text(label, 8.0, FontWeight::Normal, MUTED))
    .padding_horizontal(6.0)
    .padding_vertical(2.0)
    .background(SURFACE_2)
    .rounded(3.0)
}

fn history_section(selected: Option<&SignalRow>) -> Element {
  let history = selected.map(|signal| signal.history.as_slice()).unwrap_or(&[]);
  let mut body = Column::new()
    .padding_custom(padding(4.0, 0.0, 4.0, 0.0))
    .width(FILL)
    .height(FILL);

  if history.is_empty() {
    body = body.child(empty_history_placeholder());
  } else {
    for (index, row) in history.iter().enumerate() {
      body = body.child(history_row(row, index));
    }
  }

  Column::new()
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(icon("timer", 14.0, MUTED))
        .child(text("Change History", 12.0, FontWeight::Bold, TEXT))
        .child(Spacer::new().flex(1.0))
        .child(mono_text(
          &format!("last {} changes", history.len()),
          10.0,
          FontWeight::Normal,
          MUTED,
        ))
        .padding_custom(padding(10.0, 16.0, 10.0, 16.0))
        .width(FILL)
        .border_bottom(divider()),
    )
    .child(ScrollVertical::new(body).height(FILL).width(FILL))
    .height(FILL)
    .width(FILL)
    .into()
}

fn empty_history_placeholder() -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(10.0)
    .child(icon("timer", 14.0, MUTED))
    .child(
      Column::new()
        .spacing(2.0)
        .child(text("No changes recorded", 11.0, FontWeight::Bold, TEXT))
        .child(mono_text("Waiting for signal changes", 10.0, FontWeight::Normal, MUTED))
        .width(FILL),
    )
    .padding_custom(padding(10.0, 12.0, 10.0, 12.0))
    .width(FILL)
    .background(SURFACE_2)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(4.0)
    .into()
}

fn history_row(row: &SignalHistoryRow, index: usize) -> Element {
  Column::new()
    .spacing(5.0)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .child(mono_text(&row.timestamp, 10.0, FontWeight::Normal, MUTED).nowrap())
        .child(Spacer::new().flex(1.0))
        .child(history_badge(&row.trigger))
        .width(FILL),
    )
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .child(
          mono_text(&row.from_value, 11.0, FontWeight::Normal, "#ef4444")
            .nowrap()
            .width(150.0),
        )
        .child(icon("arrow-right", 12.0, MUTED))
        .child(Spacer::new().width(8.0))
        .child(
          mono_text(&row.to_value, 11.0, FontWeight::Bold, SIGNAL_GREEN)
            .nowrap()
            .width(FILL)
            .flex(1.0),
        )
        .width(FILL),
    )
    .padding_custom(padding(6.0, 16.0, 6.0, 16.0))
    .width(FILL)
    .background(if index % 2 == 1 { SURFACE_2 } else { "#00000000" })
    .into()
}

fn collect_signal_rows(snapshot: &DevToolsSnapshot) -> Vec<SignalRow> {
  let mut rows = Vec::new();
  if let Some(root) = &snapshot.root {
    collect_signal_rows_in(root, short_tag(&root.tag), &mut rows);
  }
  rows.sort_by_key(|row| (row.kind.rank(), row.id));
  rows
}

fn collect_signal_rows_in(node: &DevToolsNode, owner: &str, rows: &mut Vec<SignalRow>) {
  let current_owner = if node.signals.is_empty() && node.memos.is_empty() && node.effects.is_empty() {
    owner
  } else {
    short_tag(&node.tag)
  };
  for signal in &node.signals {
    rows.push(SignalRow {
      id: signal.id,
      kind: ReactiveKind::Signal,
      type_name: short_type_name(signal.type_name.as_ref()),
      value: signal.formatted_value().unwrap_or_else(|| "<unknown>".to_owned()),
      owner: current_owner.to_owned(),
      history: Vec::new(),
      subscriber_count: signal.subscriber_count(),
    });
  }
  for memo in &node.memos {
    rows.push(SignalRow {
      id: memo.id,
      kind: ReactiveKind::Memo,
      type_name: short_type_name(memo.type_name.as_ref()),
      value: memo.formatted_value().unwrap_or_else(|| "<unknown>".to_owned()),
      owner: current_owner.to_owned(),
      history: Vec::new(),
      subscriber_count: memo.subscriber_count(),
    });
  }
  for effect in &node.effects {
    rows.push(SignalRow {
      id: effect.id,
      kind: ReactiveKind::Effect,
      type_name: String::new(),
      value: "-".to_owned(),
      owner: current_owner.to_owned(),
      history: Vec::new(),
      subscriber_count: 0,
    });
  }
  for child in &node.children {
    collect_signal_rows_in(child, current_owner, rows);
  }
}

fn header_cell(label: &str, width: f32) -> Text {
  text(label, 10.0, FontWeight::Bold, MUTED).width(width)
}

fn value_badge_with_color(value: &str, color: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .child(mono_text(value, 11.0, FontWeight::Bold, color))
    .padding_horizontal(6.0)
    .padding_vertical(1.0)
    .background(Color::from_hex(&with_alpha(color, "12")))
    .rounded(3.0)
}

fn history_badge(label: &str) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .child(mono_text(label, 9.0, FontWeight::Normal, PINK))
    .padding_horizontal(6.0)
    .padding_vertical(1.0)
    .width(68.0)
    .background(SURFACE_2)
    .rounded(3.0)
}

fn short_type_name(type_name: &str) -> String {
  type_name.rsplit("::").next().unwrap_or(type_name).to_owned()
}

fn with_alpha(color: &str, alpha: &str) -> String {
  format!("{}{}", color.trim_end_matches(|ch| ch == '0' && color.len() > 7), alpha)
}

fn trim_history(history: &mut Vec<SignalHistoryRow>) {
  if history.len() > 64 {
    let overflow = history.len() - 64;
    history.drain(0..overflow);
  }
}

fn current_compact_timestamp() -> String {
  let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
  let millis = duration.subsec_millis();
  let seconds_of_day = duration.as_secs() % 86_400;
  let hour = seconds_of_day / 3_600;
  let minute = seconds_of_day % 3_600 / 60;
  let second = seconds_of_day % 60;
  format!("{hour:02}:{minute:02}:{second:02}.{millis:03}")
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    app::{ctx::Ctx, devtools::snapshot::DevToolsNodeKind},
    core::NodeId,
  };

  #[test]
  fn recording_state_records_signal_changes_after_baseline() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0_i32);
    let key = format!("Signal:{}", signal.id());
    let mut state = SignalsRecordingState::default();

    state.set_recording(true);
    state.record_snapshot(&snapshot_with_signals(ctx.signals_debug()));
    signal.set(1);
    state.record_snapshot(&snapshot_with_signals(ctx.signals_debug()));

    let history = state.history_for(&key);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from_value, "0");
    assert_eq!(history[0].to_value, "1");
    assert_eq!(history[0].trigger, "change");
  }

  #[test]
  fn recording_state_resets_baseline_when_recording_restarts() {
    let mut ctx = Ctx::new_root();
    let signal = ctx.signal(0_i32);
    let key = format!("Signal:{}", signal.id());
    let mut state = SignalsRecordingState::default();

    state.set_recording(true);
    state.record_snapshot(&snapshot_with_signals(ctx.signals_debug()));
    signal.set(1);
    state.record_snapshot(&snapshot_with_signals(ctx.signals_debug()));

    state.set_recording(false);
    signal.set(2);
    state.set_recording(true);
    state.record_snapshot(&snapshot_with_signals(ctx.signals_debug()));

    let history = state.history_for(&key);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from_value, "0");
    assert_eq!(history[0].to_value, "1");
  }

  fn snapshot_with_signals(signals: Vec<crate::app::ctx::ComponentSignalDebug>) -> DevToolsSnapshot {
    DevToolsSnapshot {
      root: Some(DevToolsNode {
        id: NodeId::UNASSIGNED,
        tag: "Root".to_owned(),
        kind: DevToolsNodeKind::Component,
        key: None,
        attrs: Vec::new(),
        text: None,
        color: None,
        props: None,
        signals,
        memos: Vec::new(),
        contexts: Vec::new(),
        shape: Vec::new(),
        effects: Vec::new(),
        layout_box: None,
        hovered: false,
        active: false,
        focused: false,
        children: Vec::new(),
      }),
      frame: Default::default(),
    }
  }
}
