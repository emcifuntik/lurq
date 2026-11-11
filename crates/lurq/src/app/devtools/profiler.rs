use std::{sync::Arc, time::Duration};

use super::{
  profiler_capture::{
    self as capture, Counter, DetailSection, FRAME_BUDGET_MS, MAX_TIMELINE_PASSES, PhaseRow, ProfilerCapture,
    TimeRange, bucket_of, format_bytes, format_ms, format_span, ms, pass_of,
  },
  style::{
    BLUE, BORDER, FILL, MUTED, PINK, PRIMARY, SELECTED, SIGNAL_GREEN, SURFACE, SURFACE_2, TEXT, YELLOW, empty_state,
    text, toolbar_button,
  },
};
use crate::{
  app::{
    ctx::Interval,
    events::MouseEvent,
    profiler::{ProfileSample, ProfilingHandle, RecordingOptions, SampleData, SessionId, SessionStatus},
  },
  components::{Column, Row, ScrollVertical, Spacer, Stack, Text},
  core::Signal,
  layout::{
    Alignment,
    text_style::{FontWeight, TextStyle},
  },
  node::{CursorIcon, Element, HitTestBehavior, border::Border, color::Color, dimension::Dimension},
};

const RED: &str = "#ef4444";
/// How often the status line of a running recording is reread.
pub(crate) const STATUS_INTERVAL: Duration = Duration::from_millis(250);
/// Equal parts of the whole recording in the overview strip.
const OVERVIEW_BUCKETS: usize = 120;
/// Equal parts of the selected range in the marker lanes.
const MARKER_BUCKETS: usize = 240;
const OVERVIEW_HEIGHT: f32 = 36.0;
const CHART_HEIGHT: f32 = 120.0;
/// Height of each counter track under the pass chart.
const COUNTER_HEIGHT: f32 = 24.0;
const BAR_WIDTH: f32 = 3.0;
/// Right of each timeline track, between it and its label.
const LANE_GAP: f32 = 8.0;
const LANE_LABEL_WIDTH: f32 = 90.0;
/// Spans the sidebar lists for one pass; the rest are counted.
const MAX_PHASE_ROWS: usize = 256;

/// The Profiler tab: the recording Start opened on the inspected tree's
/// profiling handle, or the capture Stop finished, and what is selected in it.
#[derive(Clone, Default)]
pub(crate) struct ProfilerState {
  recording: Option<Recording>,
  /// Records DevTools' own windows too; read when a recording starts.
  include_devtools: bool,
  capture: Option<Arc<ProfilerCapture>>,
  range: Option<TimeRange>,
  /// The overview bucket the range was pressed on; a shift-press extends from it.
  range_anchor: Option<usize>,
  /// True from a press on the overview until the button is released over it.
  range_dragging: bool,
  /// The selected pass's sample sequence.
  selected: Option<u64>,
  error: Option<String>,
}

#[derive(Clone)]
struct Recording {
  handle: ProfilingHandle,
  id: SessionId,
  status: Option<SessionStatus>,
}

impl crate::app::component::DevtoolsInspectable for ProfilerState {
  fn write_info(&self, buffer: &mut Vec<crate::app::component::ComponentInfo>) {
    buffer.push(crate::app::component::ComponentInfo::with_value(
      "recording",
      std::any::type_name::<bool>(),
      self.recording.is_some().to_string(),
    ));
    buffer.push(crate::app::component::ComponentInfo::with_value(
      "passes",
      std::any::type_name::<usize>(),
      self
        .capture
        .as_ref()
        .map_or(0, |capture| capture.passes.len())
        .to_string(),
    ));
  }
}

impl ProfilerState {
  pub(crate) fn recording(&self) -> bool {
    self.recording.is_some()
  }

  /// Starts an unbounded recording session on `handle`, the inspected tree's,
  /// and forgets the previous capture. False when the session did not start.
  pub(crate) fn start(&mut self, handle: &ProfilingHandle) -> bool {
    if self.recording.is_some() {
      return true;
    }
    let options = RecordingOptions {
      include_devtools: self.include_devtools,
      ..RecordingOptions::default()
    };
    match handle.start_recording(options) {
      Ok(started) => {
        *self = Self {
          recording: Some(Recording {
            handle: handle.clone(),
            id: started.id,
            status: handle.status(started.id).ok(),
          }),
          include_devtools: self.include_devtools,
          ..Self::default()
        };
        true
      }
      Err(error) => {
        self.error = Some(error.to_string());
        false
      }
    }
  }

  /// Ends the recording and keeps every sample it took as the capture.
  pub(crate) fn stop(&mut self) {
    let Some(recording) = self.recording.take() else {
      return;
    };
    match recording.handle.end(recording.id) {
      Ok(report) => {
        let capture = ProfilerCapture::from_report(&report);
        let range = capture.default_range();
        self.selected = capture.slowest_in(range);
        self.range = Some(range);
        self.capture = Some(Arc::new(capture));
      }
      Err(error) => self.error = Some(error.to_string()),
    }
  }

  /// Rereads the running recording's duration and size.
  pub(crate) fn refresh_status(&mut self) {
    if let Some(recording) = &mut self.recording {
      recording.status = recording.handle.status(recording.id).ok();
    }
  }

  /// Ends a running recording without keeping it, and forgets the capture.
  pub(crate) fn clear(&mut self) {
    if let Some(recording) = self.recording.take() {
      let _ = recording.handle.end(recording.id);
    }
    *self = Self {
      include_devtools: self.include_devtools,
      ..Self::default()
    };
  }

  fn press_bucket(&mut self, bucket: usize, extend: bool) {
    let anchor = match (extend, self.range_anchor) {
      (true, Some(anchor)) => anchor,
      _ => bucket,
    };
    self.range_anchor = Some(anchor);
    self.range_dragging = true;
    self.select_buckets(anchor, bucket);
  }

  fn drag_bucket(&mut self, bucket: usize) {
    if let (true, Some(anchor)) = (self.range_dragging, self.range_anchor) {
      self.select_buckets(anchor, bucket);
    }
  }

  /// True when dragging onto `bucket` would change the range, so a move
  /// within one bucket renders nothing again.
  fn drag_changes_range(&self, bucket: usize) -> bool {
    match (self.range_dragging, self.range_anchor, &self.capture) {
      (true, Some(anchor), Some(capture)) => self.range != Some(capture.bucket_range(anchor, bucket, OVERVIEW_BUCKETS)),
      _ => false,
    }
  }

  fn select_buckets(&mut self, first: usize, last: usize) {
    if let Some(capture) = &self.capture {
      let range = capture.bucket_range(first, last, OVERVIEW_BUCKETS);
      self.set_range(range);
    }
  }

  fn set_range(&mut self, range: TimeRange) {
    let Some(capture) = &self.capture else {
      return;
    };
    let selected_in_range = self.selected.is_some_and(|sequence| {
      capture
        .passes_in(range)
        .iter()
        .any(|sample| sample.sequence == sequence)
    });
    if !selected_in_range {
      self.selected = capture.slowest_in(range);
    }
    self.range = Some(range);
  }
}

/// Start/Stop, the live status of a running recording, the DevTools-window
/// toggle and Clear, for the top bar.
pub(crate) fn profiler_actions(
  state: &ProfilerState,
  state_signal: Signal<ProfilerState>,
  handle: Option<ProfilingHandle>,
  status_ticker: Interval,
) -> Element {
  let recording = state.recording();
  let color = if recording { RED } else { PRIMARY };
  let label = if recording { "Stop recording" } else { "Start recording" };
  let mut row = Row::new().align_items(Alignment::Center).spacing(8.0);
  if let Some(status) = state.recording.as_ref().map(recording_status_line) {
    row = row.child(mono_text(&status, 11.0, FontWeight::Medium, RED));
  }
  row = row.child(
    toolbar_button(
      "circle",
      label,
      color,
      if recording { "#ef444420" } else { SURFACE_2 },
      if recording { "#ef444460" } else { BORDER },
    )
    .on_click({
      let state_signal = state_signal.clone();
      let status_ticker = status_ticker.clone();
      move |_| {
        if recording {
          status_ticker.stop();
          state_signal.update(ProfilerState::stop);
          return;
        }
        let Some(handle) = &handle else {
          state_signal.update(|state| state.error = Some("DevTools has no profiling handle".to_owned()));
          return;
        };
        let mut started = false;
        state_signal.update(|state| started = state.start(handle));
        if started {
          status_ticker.start();
        }
      }
    }),
  );
  if !recording {
    let include = state.include_devtools;
    let toggle_signal = state_signal.clone();
    row = row.child(
      toolbar_button(
        "layers",
        "Include DevTools",
        if include { PRIMARY } else { MUTED },
        if include { SELECTED } else { SURFACE_2 },
        BORDER,
      )
      .on_click(move |_| toggle_signal.update(|state| state.include_devtools = !state.include_devtools)),
    );
  }
  row
    .child(
      toolbar_button("trash-2", "Clear", MUTED, SURFACE_2, BORDER).on_click(move |_| {
        status_ticker.stop();
        state_signal.update(ProfilerState::clear);
      }),
    )
    .into()
}

fn recording_status_line(recording: &Recording) -> String {
  let Some(status) = recording.status else {
    return "recording".to_owned();
  };
  let mut line = format!(
    "{} · {} · {} samples",
    format_span(status.observed_ms - status.started_ms),
    format_bytes(status.recording.map_or(0, |recording| recording.size_bytes)),
    status.completed_samples
  );
  if status.recording.is_some_and(|recording| recording.stopped_at_limit) {
    line.push_str(" · size limit reached");
  }
  line
}

pub(crate) fn profiler_view(state: &ProfilerState, state_signal: Signal<ProfilerState>) -> Element {
  if let Some(recording) = &state.recording {
    return message_view(
      &format!("Recording {}", recording_status_line(recording)),
      "Use the inspected app, then Stop recording to see every pass it made.",
      RED,
    );
  }
  let Some(capture) = state.capture.clone() else {
    return match &state.error {
      Some(error) => message_view("Profiling failed", error, RED),
      None => message_view(
        "No recording",
        "Start recording, use the inspected app, then Stop: the timeline shows every pass of its windows in between.",
        MUTED,
      ),
    };
  };
  let range = state.range.unwrap_or_else(|| capture.default_range());
  let selected = state.selected.and_then(|sequence| capture.pass(sequence));

  Row::new()
    .child(
      Column::new()
        .child(overview(&capture, range, state_signal.clone()))
        .child(timeline(&capture, range, state.selected, state_signal))
        .height(FILL)
        .width(FILL)
        .flex(1.0),
    )
    .child(frame_sidebar(&capture, selected))
    .height(FILL)
    .width(FILL)
    .flex(1.0)
    .into()
}

fn message_view(title: &str, detail: &str, color: &str) -> Element {
  Column::new()
    .spacing(6.0)
    .child(text(title, 13.0, FontWeight::Bold, color))
    .child(text(detail, 11.0, FontWeight::Normal, MUTED))
    .padding(16.0)
    .height(FILL)
    .width(FILL)
    .flex(1.0)
    .into()
}

/// The whole recording, one column per equal part with its longest pass;
/// press, drag or shift-press over it to select the timeline's range.
fn overview(capture: &ProfilerCapture, range: TimeRange, state_signal: Signal<ProfilerState>) -> Element {
  let whole = capture.whole();
  let longest = capture.overview(OVERVIEW_BUCKETS);
  let scale = longest.iter().copied().fold(FRAME_BUDGET_MS * 2.0, f64::max);
  let first = bucket_of(whole, range.start_ms, OVERVIEW_BUCKETS);
  let last = bucket_of(whole, range.end_ms, OVERVIEW_BUCKETS);
  let mut columns = Row::new()
    .align_items(Alignment::End)
    .height(OVERVIEW_HEIGHT)
    .width(FILL)
    .on_mouse_up({
      let state_signal = state_signal.clone();
      move |_| {
        if state_signal.with_untracked(|state| state.range_dragging) {
          state_signal.update(|state| state.range_dragging = false);
        }
      }
    });
  for (bucket, longest_ms) in longest.into_iter().enumerate() {
    let in_range = (first..=last).contains(&bucket);
    let mut column = Row::new()
      .align_items(Alignment::End)
      .height(FILL)
      .flex(1.0)
      .cursor(CursorIcon::Pointer)
      .on_mouse_down({
        let state_signal = state_signal.clone();
        move |event: MouseEvent| state_signal.update(|state| state.press_bucket(bucket, event.shift))
      })
      .on_mouse_move({
        let state_signal = state_signal.clone();
        move |_| {
          if state_signal.with_untracked(|state| state.drag_changes_range(bucket)) {
            state_signal.update(|state| state.drag_bucket(bucket));
          }
        }
      });
    if in_range {
      column = column.background(SELECTED);
    }
    if longest_ms > 0.0 {
      column = column.child(
        Row::new()
          .height(((longest_ms / scale) as f32 * OVERVIEW_HEIGHT).max(2.0))
          .width(FILL)
          .background(duration_color(longest_ms))
          .opacity(if in_range { 1.0 } else { 0.45 }),
      );
    }
    columns = columns.child(column);
  }

  Column::new()
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(text("RECORDING", 11.0, FontWeight::Bold, MUTED))
        .child(mono_text(
          &format!(
            "{} · {} passes · {}",
            format_span(whole.duration_ms()),
            capture.passes.len(),
            format_bytes(capture.size_bytes)
          ),
          10.0,
          FontWeight::Normal,
          MUTED,
        ))
        .child(if capture.stopped_at_limit {
          mono_text("stopped at its size limit", 10.0, FontWeight::Normal, YELLOW).into()
        } else {
          Element::new()
        })
        .child(if capture.boundary_excluded > 0 {
          mono_text(
            &format!("{} begun before Start, left out", capture.boundary_excluded),
            10.0,
            FontWeight::Normal,
            MUTED,
          )
          .into()
        } else {
          Element::new()
        })
        .child(Spacer::new().flex(1.0))
        .child(text(
          "Drag or shift-click to select a range",
          10.0,
          FontWeight::Normal,
          MUTED,
        ))
        .child(
          toolbar_button("refresh-cw", "Whole recording", MUTED, SURFACE_2, BORDER).on_click({
            let state_signal = state_signal.clone();
            move |_| {
              state_signal.update(|state| {
                if let Some(whole) = state.capture.as_ref().map(|capture| capture.whole()) {
                  state.range_anchor = None;
                  state.set_range(whole);
                }
              })
            }
          }),
        )
        .padding_custom(padding(8.0, 16.0, 6.0, 16.0))
        .width(FILL),
    )
    .child(
      Row::new()
        .child(columns)
        .padding_custom(padding(0.0, 16.0, 10.0, 16.0))
        .width(FILL),
    )
    .width(FILL)
    .background(SURFACE)
    .border_bottom(divider())
    .into()
}

/// The selected range on a real time axis: one bar per pass at its start,
/// its height the pass's duration, with the frame budget line, lanes of input
/// dispatch and UI update markers, and the axis.
fn timeline(
  capture: &ProfilerCapture,
  range: TimeRange,
  selected: Option<u64>,
  state_signal: Signal<ProfilerState>,
) -> Element {
  let passes = capture.passes_in(range);
  let fps = capture::fps(passes)
    .map(|fps| format!("{fps:.1}"))
    .unwrap_or_else(|| "–".to_owned());
  let long_frames = passes
    .iter()
    .filter_map(|sample| pass_of(sample))
    .filter(|pass| ms(pass.total) > FRAME_BUDGET_MS)
    .count();
  let header = Row::new()
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(text("FRAMES", 11.0, FontWeight::Bold, MUTED))
    .child(mono_text(
      &format!(
        "{} – {} ({})",
        format_offset(capture, range.start_ms),
        format_offset(capture, range.end_ms),
        format_span(range.duration_ms())
      ),
      10.0,
      FontWeight::Normal,
      TEXT,
    ))
    .child(mono_text(
      &format!("{} passes", passes.len()),
      10.0,
      FontWeight::Normal,
      MUTED,
    ))
    .child(mono_text(
      &format!("{long_frames} over budget"),
      10.0,
      FontWeight::Normal,
      if long_frames > 0 { YELLOW } else { MUTED },
    ))
    .child(Spacer::new().flex(1.0))
    .child(mono_text(&format!("FPS {fps}"), 11.0, FontWeight::Bold, TEXT))
    .padding_custom(padding(10.0, 16.0, 8.0, 16.0))
    .width(FILL);

  let body: Element = if passes.len() > MAX_TIMELINE_PASSES {
    empty_state(&format!(
      "{} passes in this range: select at most {MAX_TIMELINE_PASSES} in the recording above to draw each one.",
      passes.len()
    ))
  } else {
    timeline_tracks(capture, passes, range, selected, state_signal)
  };

  Column::new()
    .child(header)
    .child(
      Column::new()
        .child(body)
        .padding_custom(padding(0.0, 16.0, 12.0, 16.0))
        .width(FILL),
    )
    .child(legend())
    .width(FILL)
    .height(FILL)
    .flex(1.0)
    .into()
}

/// The pass chart, the counter tracks, the marker lanes and the axis, one row
/// each. Each row's track takes the width the label column leaves, and its time
/// scale is that width less `BAR_WIDTH` (so a bar at the range's end stays
/// inside): a time is at the same x in every row.
fn timeline_tracks(
  capture: &ProfilerCapture,
  passes: &[Arc<ProfileSample>],
  range: TimeRange,
  selected: Option<u64>,
  state_signal: Signal<ProfilerState>,
) -> Element {
  let mut tracks = Column::new()
    .spacing(4.0)
    .child(pass_chart(passes, range, selected, state_signal.clone()));
  for counter in Counter::ALL {
    tracks = tracks.child(counter_track(counter, passes, range, selected, state_signal.clone()));
  }
  tracks
    .child(marker_lane(capture, range, "input", BLUE, |data| {
      matches!(data, SampleData::InputDispatch(_))
    }))
    .child(marker_lane(capture, range, "ui update", PINK, |data| {
      matches!(data, SampleData::UiUpdate(_))
    }))
    .child(time_axis(capture, range))
    .width(FILL)
    .into()
}

/// One counter's value per pass, a bar at the pass's start as in the pass
/// chart; a click selects the pass. Its label shows the selected pass's value,
/// or the range's highest, or that the counter is unavailable.
fn counter_track(
  counter: Counter,
  passes: &[Arc<ProfileSample>],
  range: TimeRange,
  selected: Option<u64>,
  state_signal: Signal<ProfilerState>,
) -> Element {
  let mut track = Stack::new().flex(1.0).height(COUNTER_HEIGHT).background(SURFACE_2);
  let Some(points) = capture::counter_points(counter, passes) else {
    return track_row(
      track,
      Column::new().child(mono_text(
        &format!("{} unavailable", counter.label()),
        9.0,
        FontWeight::Normal,
        MUTED,
      )),
    );
  };
  let highest = points.iter().map(|point| point.value).fold(0.0, f64::max);
  for point in &points {
    let is_selected = selected == Some(point.sequence);
    let sequence = point.sequence;
    let height = if highest > 0.0 {
      (point.value / highest) as f32 * COUNTER_HEIGHT
    } else {
      0.0
    };
    let bar = Row::new()
      .width(BAR_WIDTH)
      .height(height.max(1.0))
      .background(if is_selected { PRIMARY } else { BLUE })
      .opacity(if is_selected { 1.0 } else { 0.7 })
      .cursor(CursorIcon::Pointer)
      .on_click({
        let state_signal = state_signal.clone();
        move |_| state_signal.update(|state| state.selected = Some(sequence))
      });
    track = track.child(placed(range.fraction(point.started_ms) as f32, bar, Alignment::End));
  }
  let shown = points
    .iter()
    .find(|point| selected == Some(point.sequence))
    .map(|point| point.value)
    .or_else(|| (!points.is_empty()).then_some(highest));
  let value = shown.map_or_else(|| "–".to_owned(), |value| counter.format(value));
  track_row(
    track,
    Column::new().child(mono_text(
      &format!("{} {value}", counter.label()),
      9.0,
      FontWeight::Normal,
      TEXT,
    )),
  )
}

/// One row of the timeline: `track`, which must flex, then its label column.
fn track_row(track: impl Into<Element>, label: Column) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(LANE_GAP)
    .child(track)
    .child(label.width(LANE_LABEL_WIDTH))
    .width(FILL)
    .into()
}

fn pass_chart(
  passes: &[Arc<ProfileSample>],
  range: TimeRange,
  selected: Option<u64>,
  state_signal: Signal<ProfilerState>,
) -> Element {
  let scale = passes
    .iter()
    .filter_map(|sample| pass_of(sample))
    .map(|pass| ms(pass.total))
    .fold(FRAME_BUDGET_MS * 2.0, f64::max);
  let budget_height = (FRAME_BUDGET_MS / scale) as f32 * CHART_HEIGHT;
  // Behind the bars and never hit, so every bar stays clickable.
  let budget_line = Column::new()
    .child(Spacer::new().flex(1.0))
    .child(Row::new().height(1.0).width(FILL).background(YELLOW))
    .child(Spacer::new().height(budget_height))
    .width(FILL)
    .height(FILL)
    .pointer_events_none();
  // Its label sits beside the line, in the label column.
  let budget_label = Column::new()
    .child(Spacer::new().flex(1.0))
    .child(mono_text(
      &format!("budget {}", format_ms(FRAME_BUDGET_MS)),
      9.0,
      FontWeight::Normal,
      YELLOW,
    ))
    .child(Spacer::new().height((budget_height - 6.0).max(0.0)))
    .height(CHART_HEIGHT);
  let mut chart = Stack::new()
    .child(budget_line)
    .flex(1.0)
    .height(CHART_HEIGHT)
    .background(SURFACE_2);
  for sample in passes {
    let Some(pass) = pass_of(sample) else {
      continue;
    };
    let total_ms = ms(pass.total);
    let is_selected = selected == Some(sample.sequence);
    let sequence = sample.sequence;
    let bar = Row::new()
      .width(BAR_WIDTH)
      .height(((total_ms / scale) as f32 * CHART_HEIGHT).max(2.0))
      .background(if is_selected { PRIMARY } else { duration_color(total_ms) })
      .opacity(if pass.rendered || is_selected { 1.0 } else { 0.5 })
      .cursor(CursorIcon::Pointer)
      .on_click({
        let state_signal = state_signal.clone();
        move |_| state_signal.update(|state| state.selected = Some(sequence))
      });
    chart = chart.child(placed(range.fraction(sample.started_ms) as f32, bar, Alignment::End));
  }
  track_row(chart, budget_label)
}

/// `content` at `fraction` of the width less its own, in a full-size layer
/// that only its content hits.
fn placed(fraction: f32, content: impl Into<Element>, align: Alignment) -> Element {
  let mut row = Row::new()
    .align_items(align)
    .width(FILL)
    .height(FILL)
    .hit_test(HitTestBehavior::ContentOnly);
  if fraction > 0.0 {
    row = row.child(Spacer::new().flex(fraction).pointer_events_none());
  }
  row = row.child(content);
  if fraction < 1.0 {
    row = row.child(Spacer::new().flex(1.0 - fraction).pointer_events_none());
  }
  row.into()
}

/// The samples of one kind that start in `range`, marked in equal parts of it.
fn marker_lane(
  capture: &ProfilerCapture,
  range: TimeRange,
  label: &str,
  color: &str,
  kind: impl Fn(&SampleData) -> bool,
) -> Element {
  let mut marked = vec![false; MARKER_BUCKETS];
  let mut count = 0;
  for sample in capture.markers_in(range) {
    if kind(&sample.data) {
      marked[bucket_of(range, sample.started_ms, MARKER_BUCKETS)] = true;
      count += 1;
    }
  }
  // The parts span the time scale, the track less `BAR_WIDTH`.
  let mut lane = Row::new()
    .height(6.0)
    .flex(1.0)
    .padding_right(BAR_WIDTH)
    .background(SURFACE_2);
  // One cell per run of equal parts keeps the lane small.
  let mut start = 0;
  while start < MARKER_BUCKETS {
    let value = marked[start];
    let end = marked[start..]
      .iter()
      .position(|marked| *marked != value)
      .map_or(MARKER_BUCKETS, |offset| start + offset);
    let mut cell = Row::new().height(FILL).flex((end - start) as f32);
    if value {
      cell = cell.background(color);
    }
    lane = lane.child(cell);
    start = end;
  }
  track_row(
    lane,
    Column::new().child(mono_text(&format!("{label} {count}"), 9.0, FontWeight::Normal, color)),
  )
}

/// Ticks at each quarter of the time scale, the last part closed by the
/// range's end.
fn time_axis(capture: &ProfilerCapture, range: TimeRange) -> Element {
  const TICKS: usize = 4;
  let tick_label = |at: f64| mono_text(&format_offset(capture, at), 9.0, FontWeight::Normal, MUTED);
  let tick_border = || Border::inside(1.0, Color::from_hex(BORDER));
  let mut axis = Row::new().flex(1.0).padding_right(BAR_WIDTH);
  for tick in 0..TICKS {
    let at = range.start_ms + range.duration_ms() * tick as f64 / TICKS as f64;
    let mut part = Row::new()
      .child(tick_label(at))
      .border_left(tick_border())
      .padding_horizontal(3.0)
      .flex(1.0);
    if tick + 1 == TICKS {
      part = part
        .child(Spacer::new().flex(1.0))
        .child(tick_label(range.end_ms))
        .border_right(tick_border());
    }
    axis = axis.child(part);
  }
  track_row(axis, Column::new())
}

fn legend() -> Element {
  let swatch = |color: &str, label: &str| -> Element {
    Row::new()
      .align_items(Alignment::Center)
      .spacing(4.0)
      .child(Row::new().size(8.0, 8.0).background(color).rounded(1.0))
      .child(text(label, 10.0, FontWeight::Normal, MUTED))
      .into()
  };
  Row::new()
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(swatch(SIGNAL_GREEN, "within budget"))
    .child(swatch(YELLOW, "over budget"))
    .child(swatch(RED, "over twice the budget"))
    .child(swatch(PRIMARY, "selected"))
    .child(text(
      "faded: not presented · click a bar to inspect it",
      10.0,
      FontWeight::Normal,
      MUTED,
    ))
    .padding_horizontal(16.0)
    .width(FILL)
    .into()
}

fn frame_sidebar(capture: &ProfilerCapture, selected: Option<&ProfileSample>) -> Element {
  let mut body = Column::new().width(FILL);
  match selected.and_then(|sample| pass_of(sample).map(|pass| (sample, pass))) {
    None => body = body.child(empty_state("Select a frame in the timeline")),
    Some((sample, pass)) => {
      let total_ms = ms(pass.total);
      let summary = [
        ("Start", format_offset(capture, sample.started_ms), TEXT),
        ("Duration", format_ms(total_ms), duration_color(total_ms)),
        ("Presented", if pass.rendered { "yes" } else { "no" }.to_owned(), TEXT),
        ("Window", sample.window.clone(), MUTED),
      ];
      let mut section = Column::new()
        .spacing(8.0)
        .padding_custom(padding(12.0, 16.0, 12.0, 16.0))
        .width(FILL)
        .border_bottom(divider());
      for (label, value, color) in summary {
        section = section.child(detail_row(label, &value, color));
      }
      body = body
        .child(section)
        .child(phase_section(&capture::phase_rows(pass), pass.dropped_spans));
      for detail in capture::pass_detail(sample, pass) {
        body = body.child(detail_section(&detail));
      }
    }
  }

  Column::new()
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .child(text("FRAME DETAILS", 11.0, FontWeight::Bold, MUTED))
        .padding_custom(padding(10.0, 16.0, 10.0, 16.0))
        .width(FILL)
        .border_bottom(divider()),
    )
    .child(ScrollVertical::new(body).height(FILL).width(FILL).flex(1.0))
    .width(340.0)
    .height(FILL)
    .background(SURFACE)
    .border_left(divider())
    .into()
}

/// The pass's spans, nested by depth, each placed on the pass's time axis.
fn phase_section(rows: &[PhaseRow], dropped_spans: u32) -> Element {
  let pass_ms = rows.first().map_or(0.0, |row| row.duration_ms).max(f64::EPSILON);
  let mut section = Column::new()
    .spacing(4.0)
    .child(text("PHASES", 11.0, FontWeight::Bold, MUTED))
    .padding_custom(padding(12.0, 16.0, 12.0, 16.0))
    .width(FILL)
    .border_bottom(divider());
  if rows.is_empty() {
    section = section.child(mono_text("No spans recorded", 10.0, FontWeight::Normal, MUTED));
  }
  for row in rows.iter().take(MAX_PHASE_ROWS) {
    let start = (row.offset_ms / pass_ms).clamp(0.0, 1.0) as f32;
    let width = (row.duration_ms / pass_ms).clamp(0.0, 1.0 - start as f64) as f32;
    let mut track = Row::new().height(6.0).width(80.0).background(SURFACE_2);
    if start > 0.0 {
      track = track.child(Spacer::new().flex(start));
    }
    track = track.child(
      Row::new()
        .height(FILL)
        .flex(width.max(0.001))
        .min_width(1.0)
        .background(duration_color(row.duration_ms)),
    );
    if start + width < 1.0 {
      track = track.child(Spacer::new().flex(1.0 - start - width));
    }
    section = section.child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(6.0)
        .child(Spacer::new().width(f32::from(row.depth) * 8.0))
        .child(mono_text(row.name, 10.0, FontWeight::Medium, TEXT).nowrap())
        .child(Spacer::new().flex(1.0))
        .child(track)
        .child(mono_text(&format_ms(row.duration_ms), 10.0, FontWeight::Normal, MUTED).width(56.0))
        .width(FILL),
    );
  }
  let hidden = rows.len().saturating_sub(MAX_PHASE_ROWS) as u64 + u64::from(dropped_spans);
  if hidden > 0 {
    section = section.child(mono_text(
      &format!("{hidden} more spans not shown"),
      10.0,
      FontWeight::Normal,
      MUTED,
    ));
  }
  section.into()
}

fn detail_section(detail: &DetailSection) -> Element {
  let mut section = Column::new()
    .spacing(6.0)
    .child(text(detail.title, 11.0, FontWeight::Bold, MUTED))
    .padding_custom(padding(12.0, 16.0, 12.0, 16.0))
    .width(FILL)
    .border_bottom(divider());
  for (label, value) in &detail.rows {
    section = section.child(detail_row(label, value, TEXT));
  }
  section.into()
}

fn detail_row(label: &str, value: &str, color: &str) -> Element {
  Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(mono_text(label, 10.0, FontWeight::Normal, MUTED))
    .child(Spacer::new().flex(1.0))
    .child(mono_text(value, 10.0, FontWeight::Medium, color))
    .width(FILL)
    .into()
}

/// Green within the frame budget, yellow over it, red over twice it.
fn duration_color(ms: f64) -> &'static str {
  if ms > FRAME_BUDGET_MS * 2.0 {
    RED
  } else if ms > FRAME_BUDGET_MS {
    YELLOW
  } else {
    SIGNAL_GREEN
  }
}

/// `at` as a time since the recording started.
fn format_offset(capture: &ProfilerCapture, at: f64) -> String {
  format!("+{}", format_span(at - capture.started_ms))
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
mod tests;
