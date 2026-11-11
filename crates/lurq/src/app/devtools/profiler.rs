use std::{sync::Arc, time::Duration};

use super::{
  profiler_analysis::AnalysisProps,
  profiler_capture::{
    self as capture, CaptureScreenshot, Counter, DetailSection, FRAME_BUDGET_MS, MAX_TIMELINE_PASSES, PhaseRow,
    ProfilerCapture, TimeRange, bucket_of, format_bytes, format_ms, format_span, ms, pass_of,
  },
  style::{
    BLUE, BORDER, FILL, MUTED, ORANGE, PINK, PRIMARY, SELECTED, SIGNAL_GREEN, SURFACE, SURFACE_2, TEXT, YELLOW,
    empty_state, text, toolbar_button,
  },
};
use crate::{
  app::{
    ctx::Interval,
    events::MouseEvent,
    profiler::{
      ProfileSample, ProfilingHandle, RecordingOptions, SampleData, ScreenshotOptions, SessionId, SessionStatus,
    },
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
const BLACK: &str = "#000000";
/// How often the status line of a running recording is reread.
pub(crate) const STATUS_INTERVAL: Duration = Duration::from_millis(250);
/// Equal parts of the whole recording in the overview strip.
const OVERVIEW_BUCKETS: usize = 120;
/// Equal parts of the selected range in the marker lanes.
const MARKER_BUCKETS: usize = 240;
/// Equal parts of the selected range in the filmstrip, each showing the
/// first screenshot placed in it.
const FILMSTRIP_CELLS: usize = 16;
const FILMSTRIP_HEIGHT: f32 = 48.0;
/// The most the sidebar's screenshot preview is high.
const PREVIEW_MAX_HEIGHT: f32 = 240.0;
const OVERVIEW_HEIGHT: f32 = 36.0;
const CHART_HEIGHT: f32 = 120.0;
/// Height of each counter track under the pass chart.
const COUNTER_HEIGHT: f32 = 24.0;
const BAR_WIDTH: f32 = 3.0;
/// Right of each timeline track, between it and its label.
const LANE_GAP: f32 = 8.0;
const LANE_LABEL_WIDTH: f32 = 90.0;
const SIDEBAR_WIDTH: f32 = 340.0;
/// Spans the sidebar lists for one pass; the rest are counted.
const MAX_PHASE_ROWS: usize = 256;

/// The Profiler tab: the recording Start opened on the inspected tree's
/// profiling handle, or the capture Stop finished, and what is selected in it.
#[derive(Clone, Default)]
pub(crate) struct ProfilerState {
  recording: Option<Recording>,
  /// Records DevTools' own windows too; read when a recording starts.
  include_devtools: bool,
  /// Takes screenshots of presented frames (`RecordingOptions::screenshots`);
  /// read when a recording starts. Off by default: each one reads a frame
  /// back from the GPU.
  screenshots: bool,
  capture: Option<Arc<ProfilerCapture>>,
  range: Option<TimeRange>,
  /// The overview bucket the range was pressed on; a shift-press extends from it.
  range_anchor: Option<usize>,
  /// True from a press on the overview until the button is released over it.
  range_dragging: bool,
  /// The selected pass's sample sequence.
  selected: Option<u64>,
  /// The filmstrip screenshot under the pointer, by its index in the
  /// capture's: the preview shows it instead of the selected pass's.
  hovered_screenshot: Option<usize>,
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
      screenshots: self.screenshots.then(ScreenshotOptions::default),
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
          screenshots: self.screenshots,
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
      screenshots: self.screenshots,
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
    let screenshots = state.screenshots;
    let toggle_signal = state_signal.clone();
    row = row.child(
      toolbar_button(
        "image",
        "Screenshots",
        if screenshots { PRIMARY } else { MUTED },
        if screenshots { SELECTED } else { SURFACE_2 },
        BORDER,
      )
      .on_click(move |_| toggle_signal.update(|state| state.screenshots = !state.screenshots)),
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
  if let Some(screenshots) = status
    .recording
    .map(|recording| recording.screenshots)
    .filter(|count| *count > 0)
  {
    line.push_str(&format!(" · {screenshots} screenshots"));
  }
  if status.recording.is_some_and(|recording| recording.stopped_at_limit) {
    line.push_str(" · size limit reached");
  }
  line
}

/// The props of the flame chart and tables under the timeline: the capture's
/// selected range and pass. None while recording or without a capture.
pub(crate) fn analysis_props(state: &ProfilerState, state_signal: Signal<ProfilerState>) -> Option<AnalysisProps> {
  if state.recording.is_some() {
    return None;
  }
  let capture = state.capture.clone()?;
  Some(AnalysisProps {
    range: state.range.unwrap_or_else(|| capture.default_range()),
    capture,
    selected: state.selected,
    on_select_pass: Some(Arc::new(move |sequence| {
      if state_signal.with_untracked(|state| state.selected != Some(sequence)) {
        state_signal.update(|state| state.selected = Some(sequence));
      }
    })),
  })
}

/// The tab's body; `analysis` is the mounted `ProfilerAnalysis` of
/// `analysis_props`, drawn under the timeline.
pub(crate) fn profiler_view(state: &ProfilerState, state_signal: Signal<ProfilerState>, analysis: Element) -> Element {
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
  // The screenshot under the pointer, else what the selected pass showed.
  let preview = state
    .hovered_screenshot
    .and_then(|index| capture.screenshots.get(index))
    .or_else(|| state.selected.and_then(|sequence| capture.screenshot_for(sequence)));

  Row::new()
    .child(
      Column::new()
        .child(overview(&capture, range, state_signal.clone()))
        .child(timeline(
          &capture,
          range,
          state.selected,
          state.hovered_screenshot,
          state_signal,
        ))
        .child(analysis)
        .height(FILL)
        .width(FILL)
        .flex(1.0),
    )
    .child(frame_sidebar(&capture, selected, preview))
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
  hovered_screenshot: Option<usize>,
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
  let missed_frames = passes.iter().filter(|sample| capture::missed_vsync(sample)).count();
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
    .child(mono_text(
      &format!("{missed_frames} missed vsync"),
      10.0,
      FontWeight::Normal,
      if missed_frames > 0 { RED } else { MUTED },
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
    timeline_tracks(capture, passes, range, selected, hovered_screenshot, state_signal)
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
    .padding_bottom(8.0)
    .width(FILL)
    .into()
}

/// The filmstrip of a recording with screenshots, the pass chart, the counter
/// tracks, the marker lanes and the axis, one row each. Each row's track takes
/// the width the label column leaves, and its time scale is that width less
/// `BAR_WIDTH` (so a bar at the range's end stays inside): a time is at the
/// same x in every row.
fn timeline_tracks(
  capture: &ProfilerCapture,
  passes: &[Arc<ProfileSample>],
  range: TimeRange,
  selected: Option<u64>,
  hovered_screenshot: Option<usize>,
  state_signal: Signal<ProfilerState>,
) -> Element {
  let mut tracks = Column::new().spacing(4.0);
  if !capture.screenshots.is_empty() {
    tracks = tracks.child(filmstrip(
      capture,
      range,
      selected,
      hovered_screenshot,
      state_signal.clone(),
    ));
  }
  // Missed vsyncs sit right under the bars they mark, then the frames that
  // paid for a screenshot; the counters follow.
  tracks = tracks
    .child(pass_chart(passes, range, selected, state_signal.clone()))
    .child(missed_vsync_lane(capture, range));
  if passes
    .iter()
    .any(|sample| pass_of(sample).is_some_and(|pass| pass.screenshot))
  {
    tracks = tracks.child(screenshot_lane(passes, range));
  }
  for counter in Counter::ALL {
    tracks = tracks.child(counter_track(counter, passes, range, selected, state_signal.clone()));
  }
  tracks
    .child(input_lane(capture, range))
    .child(marker_lane(capture, range, "ui update", PINK, |data| {
      matches!(data, SampleData::UiUpdate(_))
    }))
    .child(time_axis(capture, range))
    .width(FILL)
    .into()
}

/// The frames that missed a vsync inside a run of frames, under their bars.
fn missed_vsync_lane(capture: &ProfilerCapture, range: TimeRange) -> Element {
  let (missed, count) = capture.missed_vsync_buckets(range, MARKER_BUCKETS);
  let cells: Vec<_> = missed.into_iter().map(|missed| missed.then_some(RED)).collect();
  lane(
    &cells,
    Column::new().child(mono_text(
      &format!("missed vsync {count}"),
      9.0,
      FontWeight::Normal,
      if count > 0 { RED } else { MUTED },
    )),
  )
}

/// The element id of the filmstrip thumbnail of the capture's screenshot `index`.
pub(crate) fn filmstrip_id(index: usize) -> String {
  format!("profiler-screenshot-{index}")
}

/// The element id of the sidebar's screenshot preview.
pub(crate) const PREVIEW_ID: &str = "profiler-screenshot-preview";

/// The recording's screenshots over equal parts of the time scale, each part
/// showing the first one placed in it, at its frame's pass. Hovering one
/// previews it in the sidebar; a click selects its pass.
fn filmstrip(
  capture: &ProfilerCapture,
  range: TimeRange,
  selected: Option<u64>,
  hovered: Option<usize>,
  state_signal: Signal<ProfilerState>,
) -> Element {
  let first = capture.screenshots.partition_point(|shot| shot.at_ms < range.start_ms);
  let shown = capture.screenshots_in(range);
  let mut cells = [None; FILMSTRIP_CELLS];
  for (offset, shot) in shown.iter().enumerate() {
    cells[bucket_of(range, shot.at_ms, FILMSTRIP_CELLS)].get_or_insert(first + offset);
  }
  // The parts span the time scale, the track less `BAR_WIDTH`.
  let mut strip = Row::new()
    .height(FILMSTRIP_HEIGHT)
    .flex(1.0)
    .padding_right(BAR_WIDTH)
    .background(SURFACE_2);
  for cell in cells {
    let mut part = Row::new().height(FILL).flex(1.0).padding_right(2.0);
    if let Some(index) = cell {
      let shot = &capture.screenshots[index];
      let highlighted = hovered == Some(index) || (shot.sequence.is_some() && shot.sequence == selected);
      let sequence = shot.sequence;
      part = part.child(
        Row::new()
          .id(filmstrip_id(index))
          .child(screenshot_image(shot))
          .width(FILL)
          .height(FILL)
          .border_inside(1.0, Color::from_hex(if highlighted { PRIMARY } else { BORDER }))
          .cursor(CursorIcon::Pointer)
          .on_mouse_enter({
            let state_signal = state_signal.clone();
            move || {
              if state_signal.with_untracked(|state| state.hovered_screenshot != Some(index)) {
                state_signal.update(|state| state.hovered_screenshot = Some(index));
              }
            }
          })
          .on_mouse_leave({
            let state_signal = state_signal.clone();
            move || {
              if state_signal.with_untracked(|state| state.hovered_screenshot == Some(index)) {
                state_signal.update(|state| state.hovered_screenshot = None);
              }
            }
          })
          .on_click({
            let state_signal = state_signal.clone();
            move |_| {
              if let Some(sequence) = sequence {
                state_signal.update(|state| state.selected = Some(sequence));
              }
            }
          }),
      );
    }
    strip = strip.child(part);
  }
  track_row(
    strip,
    Column::new().child(mono_text(
      &format!("screenshots {}", shown.len()),
      9.0,
      FontWeight::Normal,
      ORANGE,
    )),
  )
}

/// The passes whose frame was copied for a screenshot, under their bars:
/// their render timings include that copy.
fn screenshot_lane(passes: &[Arc<ProfileSample>], range: TimeRange) -> Element {
  let mut cells = vec![None; MARKER_BUCKETS];
  let mut count = 0;
  for sample in passes {
    if pass_of(sample).is_some_and(|pass| pass.screenshot) {
      cells[bucket_of(range, sample.started_ms, MARKER_BUCKETS)] = Some(ORANGE);
      count += 1;
    }
  }
  lane(
    &cells,
    Column::new().child(mono_text(&format!("captured {count}"), 9.0, FontWeight::Normal, ORANGE)),
  )
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

/// Each input where the shell received it, coloured by its input-to-present
/// latency (the longest in its part); blue where no frame presented it.
fn input_lane(capture: &ProfilerCapture, range: TimeRange) -> Element {
  let buckets = capture.input_buckets(range, MARKER_BUCKETS);
  let count: usize = buckets.iter().map(|bucket| bucket.inputs).sum();
  let max_latency = buckets
    .iter()
    .filter_map(|bucket| bucket.max_latency_ms)
    .fold(None, |max: Option<f64>, latency| {
      Some(max.map_or(latency, |max| max.max(latency)))
    });
  let cells: Vec<_> = buckets
    .iter()
    .map(|bucket| (bucket.inputs > 0).then(|| bucket.max_latency_ms.map_or(BLUE, latency_color)))
    .collect();
  let mut label = Column::new().child(mono_text(&format!("input {count}"), 9.0, FontWeight::Normal, BLUE));
  if let Some(max) = max_latency {
    label = label.child(mono_text(
      &format!("max {}", format_ms(max)),
      9.0,
      FontWeight::Normal,
      latency_color(max),
    ));
  }
  lane(&cells, label)
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
  color: &'static str,
  kind: impl Fn(&SampleData) -> bool,
) -> Element {
  let mut marked = vec![None; MARKER_BUCKETS];
  let mut count = 0;
  for sample in capture.markers_in(range) {
    if kind(&sample.data) {
      marked[bucket_of(range, sample.started_ms, MARKER_BUCKETS)] = Some(color);
      count += 1;
    }
  }
  lane(
    &marked,
    Column::new().child(mono_text(&format!("{label} {count}"), 9.0, FontWeight::Normal, color)),
  )
}

/// A lane of equal parts of the time scale, each in its colour (None for
/// none), then its label.
fn lane(cells: &[Option<&'static str>], label: Column) -> Element {
  // The parts span the time scale, the track less `BAR_WIDTH`.
  let mut lane = Row::new()
    .height(6.0)
    .flex(1.0)
    .padding_right(BAR_WIDTH)
    .background(SURFACE_2);
  // One cell per run of equal parts keeps the lane small.
  let mut start = 0;
  while start < cells.len() {
    let value = cells[start];
    let end = cells[start..]
      .iter()
      .position(|cell| *cell != value)
      .map_or(cells.len(), |offset| start + offset);
    let mut cell = Row::new().height(FILL).flex((end - start) as f32);
    if let Some(color) = value {
      cell = cell.background(color);
    }
    lane = lane.child(cell);
    start = end;
  }
  track_row(lane, label)
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
      "faded: not presented · click a bar to inspect it · inputs coloured by input-to-present latency, blue: no frame",
      10.0,
      FontWeight::Normal,
      MUTED,
    ))
    .padding_horizontal(16.0)
    .width(FILL)
    .into()
}

/// The selected pass's details, under `preview`: the screenshot hovered in
/// the filmstrip, or what the selected pass's window showed.
fn frame_sidebar(
  capture: &ProfilerCapture,
  selected: Option<&ProfileSample>,
  preview: Option<&CaptureScreenshot>,
) -> Element {
  let mut body = Column::new().width(FILL);
  if let Some(shot) = preview {
    body = body.child(screenshot_preview(capture, shot));
  }
  match selected.and_then(|sample| pass_of(sample).map(|pass| (sample, pass))) {
    None => body = body.child(empty_state("Select a frame in the timeline")),
    Some((sample, pass)) => {
      let total_ms = ms(pass.total);
      let mut summary = vec![
        ("Start", format_offset(capture, sample.started_ms), TEXT),
        ("Duration", format_ms(total_ms), duration_color(total_ms)),
        ("Presented", if pass.rendered { "yes" } else { "no" }.to_owned(), TEXT),
        ("Reasons", capture::reasons_text(pass), TEXT),
        ("Window", sample.window.clone(), MUTED),
      ];
      if let Some(present) = pass.present.filter(|present| present.missed_vsync()) {
        summary.push(("Missed vsync", present.missed_vsyncs.to_string(), RED));
      }
      if pass.screenshot {
        summary.push(("Screenshot", "copied: render includes it".to_owned(), ORANGE));
      }
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
      let inputs = pass
        .frame_id
        .map(|frame_id| capture.inputs_presented_by(&sample.window, frame_id))
        .unwrap_or_default();
      for detail in capture::input_detail(&inputs)
        .into_iter()
        .chain(capture::pass_detail(sample, pass))
      {
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
    .width(SIDEBAR_WIDTH)
    .height(FILL)
    .background(SURFACE)
    .border_left(divider())
    .into()
}

/// `shot` at the sidebar's width, aspect kept, with its frame and time.
fn screenshot_preview(capture: &ProfilerCapture, shot: &CaptureScreenshot) -> Element {
  let screenshot = &shot.screenshot;
  let width = SIDEBAR_WIDTH - 32.0;
  let height = (width * screenshot.height as f32 / screenshot.width.max(1) as f32).min(PREVIEW_MAX_HEIGHT);
  Column::new()
    .id(PREVIEW_ID)
    .spacing(6.0)
    .child(
      Row::new()
        .align_items(Alignment::Center)
        .spacing(8.0)
        .child(text("SCREENSHOT", 11.0, FontWeight::Bold, MUTED))
        .child(Spacer::new().flex(1.0))
        .child(mono_text(
          &format!(
            "{} · frame {} · {}",
            screenshot.window,
            screenshot.frame_id,
            format_offset(capture, shot.at_ms)
          ),
          10.0,
          FontWeight::Normal,
          MUTED,
        ))
        .width(FILL),
    )
    .child(
      Row::new()
        .child(screenshot_image(shot))
        .width(width)
        .height(height)
        .background(BLACK),
    )
    .padding_custom(padding(12.0, 16.0, 12.0, 16.0))
    .width(FILL)
    .border_bottom(divider())
    .into()
}

/// `shot`'s pixels filling the box, aspect kept; an empty box in a build
/// without the `raster` feature, which draws no image.
fn screenshot_image(shot: &CaptureScreenshot) -> Element {
  #[cfg(feature = "raster")]
  {
    crate::components::Image::new(shot.image.clone())
      .contain()
      .width(FILL)
      .height(FILL)
      .into()
  }
  #[cfg(not(feature = "raster"))]
  {
    let _ = shot;
    Row::new().width(FILL).height(FILL).into()
  }
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

/// An input-to-present latency: green within three frame budgets (50 ms),
/// yellow within six, red above.
fn latency_color(ms: f64) -> &'static str {
  if ms > FRAME_BUDGET_MS * 6.0 {
    RED
  } else if ms > FRAME_BUDGET_MS * 3.0 {
    YELLOW
  } else {
    SIGNAL_GREEN
  }
}

/// `at` as a time since the recording started.
fn format_offset(capture: &ProfilerCapture, at: f64) -> String {
  format!("+{}", format_span(at - capture.started_ms))
}

pub(super) fn padding(top: f32, right: f32, bottom: f32, left: f32) -> crate::node::padding::Padding {
  crate::node::padding::Padding {
    top: Dimension::Px(top).into(),
    right: Dimension::Px(right).into(),
    bottom: Dimension::Px(bottom).into(),
    left: Dimension::Px(left).into(),
  }
}

pub(super) fn divider() -> Border {
  Border::inside(1.0, Color::from_hex(BORDER))
}

pub(super) fn mono_text(content: &str, size: f32, weight: FontWeight, color: &str) -> Text {
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
