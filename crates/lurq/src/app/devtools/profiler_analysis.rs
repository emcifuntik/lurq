//! The Profiler tab's analysis of the selected range or frame: a flame chart
//! of its spans (toolkit phases on the frames track, application scopes on a
//! track per lane) with zoom, pan, hover and selection, and the call tree and
//! bottom-up tables of one track. The aggregation is `profiler::SpanForest`'s;
//! this module only draws it.
//!
//! The chart draws one element per block, not per span: spans narrower than
//! `CELL_PX` are snapped to cells of that width and runs of occupied cells
//! merge into one block, so a row never holds more than its width allows
//! however many spans the range has. A single handler on the chart hit-tests
//! the forest for hover, click, drag and wheel.
use std::{
  collections::{HashMap, HashSet},
  sync::{Arc, Mutex},
};

use super::{
  profiler::{divider, mono_text, padding},
  profiler_capture::{ProfilerCapture, TimeRange, format_ms, format_span, ms, pass_of},
  style::{BORDER, FILL, MUTED, PRIMARY, SELECTED, SURFACE, SURFACE_2, TEXT, empty_state, icon, text, toolbar_button},
};
use crate::{
  app::{
    component::Component,
    ctx::Ctx,
    events::{MouseEvent, ScrollEvent},
    profiler::{
      ApplicationLane, CallNode, CallSort, CallTree, ForestSpan, PASS_SPAN_NAME, SpanForest, SpanTrack, forest_of,
    },
  },
  components::{Column, Row, ScrollVertical, Spacer, Stack},
  core::{ElementRef, Signal},
  layout::{Alignment, text_style::FontWeight},
  node::{CursorIcon, Element, border::Border, color::Color},
};

/// Element ids of the chart, its tooltip, its view and the selected span's line.
pub(crate) const CHART_ID: &str = "profiler-flame-chart";
pub(crate) const TOOLTIP_ID: &str = "profiler-flame-tooltip";
pub(crate) const VIEW_ID: &str = "profiler-flame-view";
pub(crate) const SELECTION_ID: &str = "profiler-flame-selection";
pub(crate) const ROW_HEIGHT: f32 = 18.0;
pub(crate) const TRACK_HEADER_HEIGHT: f32 = 16.0;
/// Rows a track draws at most; deeper spans are left out and counted.
pub(crate) const MAX_TRACK_ROWS: usize = 24;
/// Spans narrower than this snap to cells of this width, merged when adjacent.
pub(crate) const CELL_PX: f32 = 3.0;
/// Blocks at least this wide carry their span's name.
const LABEL_MIN_PX: f32 = 48.0;
/// How far from a thin span the pointer still hits it.
const HIT_TOLERANCE_PX: f32 = 3.0;
/// A press that moves less than this before its release is a click, not a pan.
const DRAG_THRESHOLD_PX: f32 = 3.0;
/// The narrowest view the chart zooms to: a microsecond.
const MIN_VIEW_MS: f64 = 0.001;
/// Zoom factor per logical pixel of wheel delta.
const ZOOM_PER_PX: f64 = 0.004;
const TABLE_ROW_HEIGHT: f32 = 22.0;
/// Rows the tables list at most; the rest are counted.
const MAX_TABLE_ROWS: usize = 400;
/// Call tree rows open by default: the passes and their phases.
const CALL_TREE_OPEN_DEPTH: u32 = 2;
const TOOLTIP_WIDTH: f32 = 230.0;
/// About three lines of the tooltip with its padding.
const TOOLTIP_HEIGHT: f32 = 60.0;
const MERGED: &str = "#52525b";
const NAME_COLUMN_MIN: f32 = 220.0;
const VALUE_COLUMN: f32 = 84.0;

/// The block colours of span names, picked by a hash of the name.
const PALETTE: [&str; 8] = [
  "#3b82f6", "#8b5cf6", "#ec4899", "#f59e0b", "#10b981", "#06b6d4", "#f97316", "#84cc16",
];

#[derive(Clone)]
pub(crate) struct AnalysisProps {
  pub capture: Arc<ProfilerCapture>,
  pub range: TimeRange,
  /// The pass selected in the timeline; the Frame scope analyses it.
  pub selected: Option<u64>,
  /// Takes the pass of a clicked frame span, to select it in the timeline.
  pub on_select_pass: Option<Arc<dyn Fn(u64) + Send + Sync>>,
}

impl PartialEq for AnalysisProps {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.capture, &other.capture) && self.range == other.range && self.selected == other.selected
  }
}

impl crate::app::component::DevtoolsInspectable for AnalysisProps {
  fn write_info(&self, buffer: &mut Vec<crate::app::component::ComponentInfo>) {
    buffer.push(crate::app::component::ComponentInfo::with_value(
      "passes",
      std::any::type_name::<usize>(),
      self.capture.passes_in(self.range).len().to_string(),
    ));
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum AnalysisTab {
  #[default]
  Flame,
  CallTree,
  BottomUp,
}

/// What the analysis covers: the timeline's range, or its selected pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AnalysisScope {
  #[default]
  Range,
  Frame,
}

/// What an analysis was made from; spans are indices into its forest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AnalysisKey {
  capture: usize,
  scope: AnalysisScope,
  range: TimeRange,
  frame: Option<u64>,
}

/// The forest of one range or pass, with what the chart and tables need.
pub(crate) struct Analysis {
  key: AnalysisKey,
  pub forest: SpanForest,
  /// The tracks drawn, frames first, then the lanes that have scopes.
  pub tracks: Vec<TrackRows>,
  /// The time the chart shows unzoomed: the range, or the pass.
  pub basis: TimeRange,
  pub passes: usize,
  /// Sum of `PassSample::total` over the passes analysed.
  pub pass_time_ms: f64,
  /// The capture's start, the origin of the times shown.
  pub origin_ms: f64,
  /// Application scopes of the recording that the session no longer kept.
  pub dropped_scopes: u64,
  tables: Mutex<Tables>,
}

/// Tables made so far, by bottom-up, track, sort and direction.
type Tables = HashMap<(bool, SpanTrack, SortKey, bool), Arc<CallTree>>;

pub(crate) struct TrackRows {
  pub track: SpanTrack,
  pub rows: Vec<Vec<u32>>,
}

/// `CallSort` with `Hash` for the table cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SortKey(u8);

impl From<CallSort> for SortKey {
  fn from(sort: CallSort) -> Self {
    Self(match sort {
      CallSort::Total => 0,
      CallSort::SelfTime => 1,
      CallSort::Count => 2,
      CallSort::Name => 3,
    })
  }
}

impl Analysis {
  pub(crate) fn new(
    capture: &Arc<ProfilerCapture>,
    scope: AnalysisScope,
    range: TimeRange,
    frame: Option<u64>,
  ) -> Self {
    let key = AnalysisKey {
      capture: Arc::as_ptr(capture) as usize,
      scope,
      range,
      frame: if scope == AnalysisScope::Frame { frame } else { None },
    };
    let passes = match scope {
      AnalysisScope::Range => capture.passes_in(range),
      AnalysisScope::Frame => frame
        .and_then(|sequence| capture.passes.iter().position(|sample| sample.sequence == sequence))
        .map_or(&[][..], |index| &capture.passes[index..=index]),
    };
    let basis = match scope {
      // Passes that start in the range are drawn to their end.
      AnalysisScope::Range => TimeRange {
        end_ms: passes
          .iter()
          .map(|sample| sample.completed_ms)
          .fold(range.end_ms, f64::max),
        ..range
      },
      AnalysisScope::Frame => passes.first().map_or(range, |sample| TimeRange {
        start_ms: sample.started_ms,
        end_ms: sample.completed_ms.max(sample.started_ms + MIN_VIEW_MS),
      }),
    };
    let forest = forest_of(passes, capture.scopes_in(basis));
    let tracks = SpanTrack::ALL
      .into_iter()
      .map(|track| TrackRows {
        track,
        rows: forest.rows(track),
      })
      .filter(|track| track.track == SpanTrack::Frames || !track.rows.is_empty())
      .collect();
    Self {
      key,
      tracks,
      basis,
      passes: passes.len(),
      pass_time_ms: passes
        .iter()
        .filter_map(|sample| pass_of(sample))
        .map(|pass| ms(pass.total))
        .sum(),
      origin_ms: capture.started_ms,
      dropped_scopes: capture.dropped_scopes,
      forest,
      tables: Mutex::new(HashMap::new()),
    }
  }

  /// The call tree, or the bottom-up table, of `track`, sorted.
  pub(crate) fn table(&self, bottom_up: bool, track: SpanTrack, sort: CallSort, descending: bool) -> Arc<CallTree> {
    let mut tables = self.tables.lock().unwrap_or_else(|error| error.into_inner());
    tables
      .entry((bottom_up, track, sort.into(), descending))
      .or_insert_with(|| {
        let mut tree = if bottom_up {
          self.forest.bottom_up(track)
        } else {
          self.forest.call_tree(track)
        };
        tree.sort(sort, descending);
        Arc::new(tree)
      })
      .clone()
  }

  fn span(&self, index: u32) -> &ForestSpan {
    &self.forest.spans()[index as usize]
  }
}

/// The chart's rows from the top: each track's header, then its rows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChartGeometry {
  /// Per track of the analysis: the y of its first row and the rows drawn.
  pub tracks: Vec<(f32, usize)>,
  pub height: f32,
}

impl ChartGeometry {
  pub(crate) fn of(analysis: &Analysis) -> Self {
    let mut y = 0.0;
    let mut tracks = Vec::with_capacity(analysis.tracks.len());
    for track in &analysis.tracks {
      y += TRACK_HEADER_HEIGHT;
      let rows = track.rows.len().clamp(1, MAX_TRACK_ROWS);
      tracks.push((y, rows));
      y += rows as f32 * ROW_HEIGHT;
    }
    Self { tracks, height: y }
  }

  /// The track and row at `y`, if `y` is on a row.
  fn row_at(&self, y: f32) -> Option<(usize, usize)> {
    self.tracks.iter().enumerate().find_map(|(track, (top, rows))| {
      let row = ((y - top) / ROW_HEIGHT).floor();
      (y >= *top && row < *rows as f32).then_some((track, row as usize))
    })
  }
}

/// The span under the chart-local point (`x`, `y`) when `view` spans `width`.
pub(crate) fn hit(
  analysis: &Analysis,
  geometry: &ChartGeometry,
  view: TimeRange,
  width: f32,
  x: f32,
  y: f32,
) -> Option<u32> {
  let (track, row) = geometry.row_at(y)?;
  let row = analysis.tracks.get(track)?.rows.get(row)?;
  if width <= 0.0 || !(0.0..=width).contains(&x) {
    return None;
  }
  let ns_per_px = view.duration_ms() * 1e6 / f64::from(width);
  let at_ns = (view.start_ms * 1e6 + f64::from(x) * ns_per_px).max(0.0) as u64;
  analysis
    .forest
    .span_at(row, at_ns, (f64::from(HIT_TOLERANCE_PX) * ns_per_px) as u64)
}

/// One drawn block: a span, or adjacent narrow spans merged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Block {
  pub x: f32,
  pub width: f32,
  /// The span, when the block is one.
  pub span: Option<u32>,
}

/// A run of occupied cells not yet drawn: its first and last cell, and its
/// span while it holds only one.
struct Run {
  first: i64,
  last: i64,
  span: Option<u32>,
}

/// The blocks of one row in `view`, drawn `width` wide, left to right and
/// never overlapping.
pub(crate) fn row_blocks(forest: &SpanForest, row: &[u32], view: TimeRange, width: f32) -> Vec<Block> {
  let mut blocks: Vec<Block> = Vec::new();
  if width <= 0.0 || view.duration_ms() <= 0.0 {
    return blocks;
  }
  let spans = forest.spans();
  let start_ns = view.start_ms * 1e6;
  let end_ns = view.end_ms * 1e6;
  let px_per_ns = f64::from(width) / (end_ns - start_ns);
  let first = row.partition_point(|index| (spans[*index as usize].end_ns as f64) < start_ns);
  let mut run: Option<Run> = None;
  // Draws the run between the last block and `limit`.
  let flush = |run: &mut Option<Run>, blocks: &mut Vec<Block>, limit: f32| {
    let Some(run) = run.take() else {
      return;
    };
    let after = blocks.last().map_or(0.0, |block| block.x + block.width);
    let x = (run.first as f32 * CELL_PX).max(after);
    let end = ((run.last + 1) as f32 * CELL_PX).min(limit).min(width);
    if end > x {
      blocks.push(Block {
        x,
        width: end - x,
        span: run.span,
      });
    }
  };
  for index in &row[first..] {
    let span = &spans[*index as usize];
    if span.start_ns as f64 > end_ns {
      break;
    }
    let x0 = ((span.start_ns as f64 - start_ns) * px_per_ns).max(0.0) as f32;
    let x1 = ((span.end_ns as f64 - start_ns) * px_per_ns).min(f64::from(width)) as f32;
    if x1 - x0 >= CELL_PX {
      flush(&mut run, &mut blocks, x0);
      blocks.push(Block {
        x: x0,
        width: x1 - x0,
        span: Some(*index),
      });
      continue;
    }
    let (first, last) = ((x0 / CELL_PX).floor() as i64, (x1 / CELL_PX).floor() as i64);
    match &mut run {
      Some(current) if first <= current.last + 1 => {
        current.last = current.last.max(last);
        current.span = None;
      }
      _ => {
        flush(&mut run, &mut blocks, f32::INFINITY);
        run = Some(Run {
          first,
          last,
          span: Some(*index),
        });
      }
    }
  }
  flush(&mut run, &mut blocks, f32::INFINITY);
  blocks
}

#[derive(Clone, Default)]
pub(crate) struct AnalysisState {
  pub tab: AnalysisTab,
  pub scope: AnalysisScope,
  pub table_track: Option<SpanTrack>,
  pub sort: Option<(CallSort, bool)>,
  /// The analysis the rest belongs to; it is dropped when the analysis changes.
  key: Option<AnalysisKey>,
  /// The zoomed view; None shows the whole basis.
  pub view: Option<TimeRange>,
  pub hovered: Option<Hover>,
  pub selected_span: Option<u32>,
  drag: Option<Drag>,
  /// Table rows opened or closed against their default, by table and track.
  toggled: HashSet<(bool, SpanTrack, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hover {
  pub span: u32,
  pub x: f32,
  pub y: f32,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
  x: f32,
  view: TimeRange,
  moved: bool,
}

impl crate::app::component::DevtoolsInspectable for AnalysisState {
  fn write_info(&self, buffer: &mut Vec<crate::app::component::ComponentInfo>) {
    buffer.push(crate::app::component::ComponentInfo::with_value(
      "tab",
      std::any::type_name::<AnalysisTab>(),
      format!("{:?}", self.tab),
    ));
  }
}

impl AnalysisState {
  /// Forgets the view, hover, selection and open rows of another analysis.
  fn sync(&mut self, key: AnalysisKey) {
    if self.key != Some(key) {
      *self = Self {
        tab: self.tab,
        scope: self.scope,
        table_track: self.table_track,
        sort: self.sort,
        key: Some(key),
        ..Self::default()
      };
    }
  }

  /// The state as it applies to `analysis`.
  fn for_analysis(&self, analysis: &Analysis) -> Self {
    let mut state = self.clone();
    state.sync(analysis.key);
    state
  }

  pub(crate) fn view_of(&self, analysis: &Analysis) -> TimeRange {
    self
      .view
      .filter(|_| self.key == Some(analysis.key))
      .unwrap_or(analysis.basis)
  }

  fn sort(&self) -> (CallSort, bool) {
    self.sort.unwrap_or((CallSort::Total, true))
  }

  fn table_track(&self, analysis: &Analysis) -> SpanTrack {
    self
      .table_track
      .filter(|track| analysis.tracks.iter().any(|rows| rows.track == *track))
      .unwrap_or(SpanTrack::Frames)
  }

  fn expanded(&self, bottom_up: bool, track: SpanTrack, index: u32, node: &CallNode) -> bool {
    let open = !bottom_up && node.depth < CALL_TREE_OPEN_DEPTH;
    open != self.toggled.contains(&(bottom_up, track, index))
  }

  /// Zooms by `factor` (below 1 zooms in) around `at` (0..1 of the view),
  /// within the basis.
  pub(crate) fn zoom(&mut self, analysis: &Analysis, factor: f64, at: f64) {
    self.sync(analysis.key);
    let view = self.view_of(analysis);
    let basis = analysis.basis;
    let duration = (view.duration_ms() * factor).clamp(MIN_VIEW_MS.min(basis.duration_ms()), basis.duration_ms());
    let anchor = view.start_ms + view.duration_ms() * at.clamp(0.0, 1.0);
    let start = anchor - duration * at.clamp(0.0, 1.0);
    self.view = Some(clamp_view(start, duration, basis));
  }

  /// Moves the view by `delta_ms`, within the basis.
  pub(crate) fn pan(&mut self, analysis: &Analysis, from: TimeRange, delta_ms: f64) {
    self.sync(analysis.key);
    self.view = Some(clamp_view(from.start_ms + delta_ms, from.duration_ms(), analysis.basis));
  }
}

fn clamp_view(start_ms: f64, duration_ms: f64, basis: TimeRange) -> TimeRange {
  let start = start_ms.clamp(basis.start_ms, (basis.end_ms - duration_ms).max(basis.start_ms));
  TimeRange {
    start_ms: start,
    end_ms: start + duration_ms,
  }
}

pub(crate) struct ProfilerAnalysis {
  state: Signal<AnalysisState>,
  chart_width: Signal<f32>,
  chart: ElementRef,
  analysis: Mutex<Option<Arc<Analysis>>>,
}

impl ProfilerAnalysis {
  /// The analysis of `props` in `scope`, made again only when they changed.
  fn analysis(&self, props: &AnalysisProps, scope: AnalysisScope) -> Arc<Analysis> {
    let mut cached = self.analysis.lock().unwrap_or_else(|error| error.into_inner());
    let key = AnalysisKey {
      capture: Arc::as_ptr(&props.capture) as usize,
      scope,
      range: props.range,
      frame: if scope == AnalysisScope::Frame {
        props.selected
      } else {
        None
      },
    };
    match &*cached {
      Some(analysis) if analysis.key == key => analysis.clone(),
      _ => {
        let analysis = Arc::new(Analysis::new(&props.capture, scope, props.range, props.selected));
        *cached = Some(analysis.clone());
        analysis
      }
    }
  }
}

impl Component for ProfilerAnalysis {
  type Props = AnalysisProps;

  fn create(ctx: &mut Ctx) -> Self {
    let chart_width = ctx.signal(0.0_f32);
    let chart = ElementRef::new();
    chart.observe_rect({
      let chart_width = chart_width.clone();
      move |rect| {
        if chart_width.get_untracked() != rect.width {
          chart_width.set(rect.width);
        }
      }
    });
    Self {
      state: ctx.signal(AnalysisState::default()),
      chart_width,
      chart,
      analysis: Mutex::new(None),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<AnalysisProps>().clone();
    let mut stored = self.state.get();
    // Without a selected pass, the Frame scope shows the range.
    if props.selected.is_none() {
      stored.scope = AnalysisScope::Range;
    }
    let analysis = self.analysis(&props, stored.scope);
    let state = stored.for_analysis(&analysis);
    let body = match state.tab {
      AnalysisTab::Flame => flame_chart(
        &analysis,
        &state,
        self.state.clone(),
        self.chart_width.get(),
        self.chart.clone(),
        props.on_select_pass.clone(),
      ),
      AnalysisTab::CallTree => table_view(&analysis, &state, self.state.clone(), false),
      AnalysisTab::BottomUp => table_view(&analysis, &state, self.state.clone(), true),
    };
    Column::new()
      .child(analysis_bar(
        &analysis,
        &state,
        self.state.clone(),
        props.selected.is_some(),
      ))
      .child(body)
      .width(FILL)
      .height(FILL)
      .flex(1.0)
      .border_top(divider())
  }
}

/// The tabs, the scope and what the analysis covers.
fn analysis_bar(
  analysis: &Analysis,
  state: &AnalysisState,
  state_signal: Signal<AnalysisState>,
  has_selected_pass: bool,
) -> Element {
  let tab = |label: &str, value: AnalysisTab| {
    let on = state.tab == value;
    let state_signal = state_signal.clone();
    chip(label, on)
      .id(tab_id(label))
      .on_click(move |_| state_signal.update(|state| state.tab = value))
  };
  let scope = |label: &str, value: AnalysisScope| {
    let on = state.scope == value;
    let state_signal = state_signal.clone();
    chip(label, on)
      .id(tab_id(label))
      .on_click(move |_| state_signal.update(|state| state.scope = value))
  };
  let mut bar = Row::new()
    .align_items(Alignment::Center)
    .spacing(6.0)
    .child(tab("Flame chart", AnalysisTab::Flame))
    .child(tab("Call tree", AnalysisTab::CallTree))
    .child(tab("Bottom-up", AnalysisTab::BottomUp))
    .child(Spacer::new().width(12.0))
    .child(scope("Range", AnalysisScope::Range));
  if has_selected_pass {
    bar = bar.child(scope("Selected frame", AnalysisScope::Frame));
  }
  let mut summary = format!(
    "{} passes · pass time {} · {} spans",
    analysis.passes,
    format_ms(analysis.pass_time_ms),
    analysis.forest.len()
  );
  if analysis.dropped_scopes > 0 {
    summary.push_str(&format!(
      " · {} earlier application scopes not kept",
      analysis.dropped_scopes
    ));
  }
  bar
    .child(Spacer::new().width(12.0))
    .child(mono_text(&summary, 10.0, FontWeight::Normal, MUTED))
    .padding_custom(padding(8.0, 16.0, 8.0, 16.0))
    .width(FILL)
    .background(SURFACE)
    .border_bottom(divider())
    .into()
}

/// The element id of the analysis tab or scope chip labelled `label`:
/// `profiler-analysis-call-tree` for "Call tree".
pub(crate) fn tab_id(label: &str) -> String {
  format!("profiler-analysis-{}", label.to_ascii_lowercase().replace(' ', "-"))
}

fn chip(label: &str, on: bool) -> Row {
  Row::new()
    .align_items(Alignment::Center)
    .child(text(label, 11.0, FontWeight::Medium, if on { PRIMARY } else { MUTED }).nowrap())
    .height(22.0)
    .padding_horizontal(8.0)
    .background(if on { SELECTED } else { SURFACE_2 })
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(3.0)
    .cursor(CursorIcon::Pointer)
}

fn flame_chart(
  analysis: &Arc<Analysis>,
  state: &AnalysisState,
  state_signal: Signal<AnalysisState>,
  width: f32,
  chart_ref: ElementRef,
  on_select_pass: Option<Arc<dyn Fn(u64) + Send + Sync>>,
) -> Element {
  let view = state.view_of(analysis);
  let geometry = Arc::new(ChartGeometry::of(analysis));
  let mut chart = Stack::new()
    .width(FILL)
    .height(geometry.height)
    .background(SURFACE_2)
    .clip()
    .cursor(CursorIcon::Pointer)
    .ref_element(chart_ref.clone())
    .id(CHART_ID);
  for (track, (top, rows)) in analysis.tracks.iter().zip(&geometry.tracks) {
    chart = chart.child(
      Row::new()
        .align_items(Alignment::Center)
        .child(mono_text(&track_label(track), 9.0, FontWeight::Bold, MUTED).nowrap())
        .padding_horizontal(4.0)
        .absolute(0.0, top - TRACK_HEADER_HEIGHT, width.max(1.0), TRACK_HEADER_HEIGHT)
        .background(SURFACE)
        .pointer_events_none(),
    );
    for (depth, row) in track.rows.iter().take(*rows).enumerate() {
      let y = top + depth as f32 * ROW_HEIGHT;
      for block in row_blocks(&analysis.forest, row, view, width) {
        chart = chart.child(block_element(analysis, state, block, y));
      }
    }
  }
  if let Some(hover) = state.hovered {
    chart = chart.child(tooltip(analysis, hover, width, geometry.height));
  }
  let chart = chart
    .on_mouse_move({
      let (analysis, geometry, state_signal, chart_ref) = (
        analysis.clone(),
        geometry.clone(),
        state_signal.clone(),
        chart_ref.clone(),
      );
      move |event: MouseEvent| {
        let (x, y) = local(&chart_ref, &event);
        let width = chart_ref.width();
        let next = state_signal.with_untracked(|state| {
          let state = state.for_analysis(&analysis);
          match state.drag {
            Some(drag) => {
              let moved = drag.moved || (x - drag.x).abs() >= DRAG_THRESHOLD_PX;
              moved.then(|| {
                let delta_ms = -f64::from(x - drag.x) * drag.view.duration_ms() / f64::from(width.max(1.0));
                let mut next = state.clone();
                next.pan(&analysis, drag.view, delta_ms);
                next.drag = Some(Drag { moved, ..drag });
                next.hovered = None;
                next
              })
            }
            None => {
              let span = hit(&analysis, &geometry, state.view_of(&analysis), width, x, y);
              (span != state.hovered.map(|hover| hover.span)).then(|| AnalysisState {
                hovered: span.map(|span| Hover { span, x, y }),
                ..state.clone()
              })
            }
          }
        });
        if let Some(next) = next {
          state_signal.set(next);
        }
      }
    })
    .on_mouse_down({
      let (analysis, state_signal, chart_ref) = (analysis.clone(), state_signal.clone(), chart_ref.clone());
      move |event: MouseEvent| {
        let (x, _) = local(&chart_ref, &event);
        state_signal.update(|state| {
          state.sync(analysis.key);
          state.drag = Some(Drag {
            x,
            view: state.view_of(&analysis),
            moved: false,
          });
        });
      }
    })
    .on_mouse_up({
      let (analysis, geometry, state_signal, chart_ref) = (
        analysis.clone(),
        geometry.clone(),
        state_signal.clone(),
        chart_ref.clone(),
      );
      move |event: MouseEvent| {
        let (x, y) = local(&chart_ref, &event);
        let width = chart_ref.width();
        let mut picked_pass = None;
        state_signal.update(|state| {
          state.sync(analysis.key);
          let clicked = state.drag.take().is_none_or(|drag| !drag.moved);
          if clicked {
            state.selected_span = hit(&analysis, &geometry, state.view_of(&analysis), width, x, y);
            picked_pass = state.selected_span.and_then(|span| analysis.span(span).sample);
          }
        });
        if let (Some(sequence), Some(on_select_pass)) = (picked_pass, &on_select_pass) {
          on_select_pass(sequence);
        }
      }
    })
    .on_mouse_leave({
      let state_signal = state_signal.clone();
      move || {
        if state_signal.with_untracked(|state| state.hovered.is_some() || state.drag.is_some()) {
          state_signal.update(|state| {
            state.hovered = None;
            state.drag = None;
          });
        }
      }
    })
    .on_scroll({
      let (analysis, state_signal, chart_ref) = (analysis.clone(), state_signal.clone(), chart_ref.clone());
      move |event: ScrollEvent| {
        event.prevent_default();
        let width = chart_ref.width().max(1.0);
        let at = f64::from((event.x - chart_ref.x()) / width);
        state_signal.update(|state| {
          if event.delta_y != 0.0 {
            state.zoom(&analysis, (-f64::from(event.delta_y) * ZOOM_PER_PX).exp(), at);
          }
          if event.delta_x != 0.0 {
            let view = state.view_of(&analysis);
            state.pan(
              &analysis,
              view,
              -f64::from(event.delta_x) * view.duration_ms() / f64::from(width),
            );
          }
          state.hovered = None;
        });
      }
    });

  Column::new()
    .child(chart_toolbar(analysis, state, view, state_signal))
    .child(Row::new().child(chart).padding_horizontal(16.0).width(FILL))
    .child(selection_line(analysis, state))
    .width(FILL)
    .height(FILL)
    .flex(1.0)
    .clip()
    .into()
}

/// `event`'s position in the chart.
fn local(chart: &ElementRef, event: &MouseEvent) -> (f32, f32) {
  (event.x - chart.x(), event.y - chart.y())
}

fn track_label(track: &TrackRows) -> String {
  let hidden = track.rows.len().saturating_sub(MAX_TRACK_ROWS);
  let label = match track.track {
    SpanTrack::Frames => "UI · toolkit phases".to_owned(),
    SpanTrack::Application(lane) => format!("{} · application scopes", lane_name(lane)),
  };
  if hidden > 0 {
    format!("{label} · {hidden} deeper rows not shown")
  } else {
    label
  }
}

fn lane_name(lane: ApplicationLane) -> &'static str {
  match lane {
    ApplicationLane::Ui => "UI lane",
    ApplicationLane::Worker => "worker lane",
  }
}

fn block_element(analysis: &Analysis, state: &AnalysisState, block: Block, y: f32) -> Element {
  let mut element = Row::new()
    .align_items(Alignment::Center)
    .absolute(block.x, y + 1.0, block.width.max(1.0), ROW_HEIGHT - 2.0)
    .pointer_events_none();
  let Some(index) = block.span else {
    return element.background(MERGED).into();
  };
  let span = analysis.span(index);
  element = element.background(span_color(span.name));
  if state.selected_span == Some(index) {
    element = element.border_inside(2.0, Color::from_hex(TEXT));
  } else if state.hovered.is_some_and(|hover| hover.span == index) {
    element = element.border_inside(1.0, Color::from_hex(TEXT));
  }
  if block.width >= LABEL_MIN_PX {
    element = element
      .child(mono_text(span.name, 9.0, FontWeight::Medium, "#0a0a0a").nowrap())
      .padding_horizontal(3.0)
      .clip();
  }
  element.into()
}

pub(crate) fn span_color(name: &str) -> &'static str {
  if name == PASS_SPAN_NAME {
    return "#a1a1aa";
  }
  let hash = name.bytes().fold(2166136261_u32, |hash, byte| {
    (hash ^ u32::from(byte)).wrapping_mul(16777619)
  });
  PALETTE[hash as usize % PALETTE.len()]
}

/// Under and right of the pointer, or above it where the chart ends below.
fn tooltip(analysis: &Analysis, hover: Hover, width: f32, height: f32) -> Element {
  let span = analysis.span(hover.span);
  let x = (hover.x + 12.0).min(width - TOOLTIP_WIDTH).max(0.0);
  let y = if hover.y + 14.0 + TOOLTIP_HEIGHT <= height {
    hover.y + 14.0
  } else {
    (hover.y - 8.0 - TOOLTIP_HEIGHT).max(0.0)
  };
  Column::new()
    .spacing(2.0)
    .child(mono_text(span.name, 11.0, FontWeight::Bold, TEXT).nowrap())
    .child(mono_text(
      &format!(
        "total {} · self {}",
        format_ms(span.duration_ns() as f64 / 1e6),
        format_ms(span.self_ns as f64 / 1e6)
      ),
      10.0,
      FontWeight::Normal,
      TEXT,
    ))
    .child(mono_text(&span_origin(analysis, span), 10.0, FontWeight::Normal, MUTED))
    .padding(6.0)
    .width(TOOLTIP_WIDTH)
    .background(SURFACE)
    .border_inside(1.0, Color::from_hex(BORDER))
    .rounded(3.0)
    .absolute_position(x, y)
    .pointer_events_none()
    .id(TOOLTIP_ID)
    .into()
}

/// Where a span comes from: its pass, or its application lane, and its start.
fn span_origin(analysis: &Analysis, span: &ForestSpan) -> String {
  let at = format_span(span.start_ns as f64 / 1e6 - analysis.origin_ms);
  match (span.track, span.sample) {
    (SpanTrack::Frames, Some(sequence)) => format!("pass #{sequence} · +{at}"),
    (SpanTrack::Application(lane), _) => format!("{} · +{at}", lane_name(lane)),
    _ => format!("+{at}"),
  }
}

fn chart_toolbar(
  analysis: &Arc<Analysis>,
  state: &AnalysisState,
  view: TimeRange,
  state_signal: Signal<AnalysisState>,
) -> Element {
  let mut row = Row::new()
    .align_items(Alignment::Center)
    .spacing(8.0)
    .child(
      mono_text(
        &format!(
          "{} – {} ({})",
          format_offset_from(analysis.origin_ms, view.start_ms),
          format_offset_from(analysis.origin_ms, view.end_ms),
          format_ms(view.duration_ms())
        ),
        10.0,
        FontWeight::Normal,
        TEXT,
      )
      .id(VIEW_ID),
    )
    .child(Spacer::new().flex(1.0))
    .child(text(
      "Wheel to zoom · drag to pan · click a span to select it",
      10.0,
      FontWeight::Normal,
      MUTED,
    ));
  if let Some(index) = state.selected_span {
    let analysis = analysis.clone();
    let state_signal = state_signal.clone();
    row = row.child(
      toolbar_button("search", "Zoom to selection", MUTED, SURFACE_2, BORDER).on_click(move |_| {
        let span = *analysis.span(index);
        state_signal.update(|state| {
          state.sync(analysis.key);
          let duration = ((span.end_ns - span.start_ns) as f64 / 1e6).max(MIN_VIEW_MS);
          let margin = duration * 0.1;
          state.view = Some(clamp_view(
            span.start_ns as f64 / 1e6 - margin,
            (duration + 2.0 * margin).min(analysis.basis.duration_ms()),
            analysis.basis,
          ));
        });
      }),
    );
  }
  if state.view.is_some() {
    row = row.child(
      toolbar_button("refresh-cw", "Reset zoom", MUTED, SURFACE_2, BORDER)
        .on_click(move |_| state_signal.update(|state| state.view = None)),
    );
  }
  row.padding_custom(padding(8.0, 16.0, 6.0, 16.0)).width(FILL).into()
}

fn format_offset_from(origin_ms: f64, at: f64) -> String {
  format!("+{}", format_span(at - origin_ms))
}

/// The selected span in full, under the chart.
fn selection_line(analysis: &Analysis, state: &AnalysisState) -> Element {
  let Some(index) = state.selected_span else {
    return empty_state("Hover a span for its times; click one to select it.");
  };
  let span = analysis.span(index);
  let parent = span.parent.map(|parent| analysis.span(parent).name).unwrap_or("–");
  Row::new()
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(Row::new().size(8.0, 8.0).background(span_color(span.name)).rounded(1.0))
    .child(mono_text(span.name, 11.0, FontWeight::Bold, TEXT))
    .child(mono_text(
      &format!(
        "total {} · self {} · in {parent} · {}",
        format_ms(span.duration_ns() as f64 / 1e6),
        format_ms(span.self_ns as f64 / 1e6),
        span_origin(analysis, span)
      ),
      10.0,
      FontWeight::Normal,
      MUTED,
    ))
    .padding_custom(padding(8.0, 16.0, 8.0, 16.0))
    .width(FILL)
    .id(SELECTION_ID)
    .into()
}

/// The call tree or the bottom-up table of the chosen track.
fn table_view(
  analysis: &Arc<Analysis>,
  state: &AnalysisState,
  state_signal: Signal<AnalysisState>,
  bottom_up: bool,
) -> Element {
  let track = state.table_track(analysis);
  let (sort, descending) = state.sort();
  let tree = analysis.table(bottom_up, track, sort, descending);
  let rows = tree.visible_rows(|index, node| state.expanded(bottom_up, track, index, node));

  let mut tracks = Row::new().align_items(Alignment::Center).spacing(6.0);
  for rows in &analysis.tracks {
    let value = rows.track;
    let state_signal = state_signal.clone();
    tracks = tracks.child(chip(&track_title(value), value == track).on_click(move |_| {
      state_signal.update(|state| state.table_track = Some(value));
    }));
  }
  let total_line = match track {
    SpanTrack::Frames => format!(
      "total {} = pass time of {} passes",
      format_ms(tree.total_ns as f64 / 1e6),
      analysis.passes
    ),
    SpanTrack::Application(_) => format!("total {} of root scopes", format_ms(tree.total_ns as f64 / 1e6)),
  };
  let toolbar = Row::new()
    .align_items(Alignment::Center)
    .spacing(12.0)
    .child(tracks)
    .child(mono_text(&total_line, 10.0, FontWeight::Normal, MUTED))
    .padding_custom(padding(8.0, 16.0, 6.0, 16.0))
    .width(FILL);

  let mut body = Column::new().width(FILL);
  if rows.is_empty() {
    body = body.child(empty_state("No spans in this range."));
  }
  for index in rows.iter().take(MAX_TABLE_ROWS) {
    body = body.child(table_row(
      &tree,
      *index,
      state.expanded(bottom_up, track, *index, &tree.nodes[*index as usize]),
      bottom_up,
      track,
      state_signal.clone(),
    ));
  }
  if rows.len() > MAX_TABLE_ROWS {
    body = body.child(empty_state(&format!(
      "{} more rows not shown: close some rows to see the rest.",
      rows.len() - MAX_TABLE_ROWS
    )));
  }

  Column::new()
    .child(toolbar)
    .child(table_header(state, state_signal))
    .child(ScrollVertical::new(body).width(FILL).height(FILL).flex(1.0))
    .width(FILL)
    .height(FILL)
    .flex(1.0)
    .into()
}

fn track_title(track: SpanTrack) -> String {
  match track {
    SpanTrack::Frames => "Frames".to_owned(),
    SpanTrack::Application(lane) => format!("App {}", lane_name(lane)),
  }
}

/// The column titles; a click sorts by the column, again reverses it.
fn table_header(state: &AnalysisState, state_signal: Signal<AnalysisState>) -> Element {
  let (sort, descending) = state.sort();
  let column = |label: &str, key: CallSort, width: Option<f32>| -> Element {
    let on = sort == key;
    let arrow = match (on, descending) {
      (false, _) => "",
      (true, true) => " ▼",
      (true, false) => " ▲",
    };
    let state_signal = state_signal.clone();
    let mut cell = Row::new()
      .align_items(Alignment::Center)
      .child(
        text(
          &format!("{label}{arrow}"),
          10.0,
          FontWeight::Bold,
          if on { TEXT } else { MUTED },
        )
        .nowrap(),
      )
      .cursor(CursorIcon::Pointer)
      .on_click(move |_| {
        state_signal.update(|state| {
          let (current, descending) = state.sort();
          state.sort = Some(if current == key {
            (key, !descending)
          } else {
            (key, key != CallSort::Name)
          });
        })
      });
    cell = match width {
      Some(width) => cell.width(width),
      None => cell.flex(1.0).min_width(NAME_COLUMN_MIN),
    };
    cell.into()
  };
  Row::new()
    .align_items(Alignment::Center)
    .child(column("Name", CallSort::Name, None))
    .child(column("Total", CallSort::Total, Some(VALUE_COLUMN)))
    .child(plain_cell("Total %", MUTED, FontWeight::Bold))
    .child(column("Self", CallSort::SelfTime, Some(VALUE_COLUMN)))
    .child(plain_cell("Self %", MUTED, FontWeight::Bold))
    .child(column("Count", CallSort::Count, Some(VALUE_COLUMN)))
    .height(TABLE_ROW_HEIGHT)
    .padding_horizontal(16.0)
    .width(FILL)
    .background(SURFACE)
    .border_bottom(divider())
    .into()
}

fn plain_cell(value: &str, color: &str, weight: FontWeight) -> Element {
  Row::new()
    .child(mono_text(value, 10.0, weight, color).nowrap())
    .width(VALUE_COLUMN)
    .into()
}

fn table_row(
  tree: &CallTree,
  index: u32,
  expanded: bool,
  bottom_up: bool,
  track: SpanTrack,
  state_signal: Signal<AnalysisState>,
) -> Element {
  let node = &tree.nodes[index as usize];
  let has_children = !node.children.is_empty();
  let chevron: Element = if has_children {
    icon(if expanded { "chevron-down" } else { "chevron-right" }, 11.0, MUTED).into()
  } else {
    Spacer::new().width(11.0).into()
  };
  let mut name = Row::new()
    .align_items(Alignment::Center)
    .spacing(4.0)
    .child(Spacer::new().width(node.depth as f32 * 12.0))
    .child(chevron)
    .child(Row::new().size(8.0, 8.0).background(span_color(node.name)).rounded(1.0))
    .child(mono_text(node.name, 10.0, FontWeight::Medium, TEXT).nowrap())
    .flex(1.0)
    .min_width(NAME_COLUMN_MIN)
    .clip();
  if has_children {
    name = name.cursor(CursorIcon::Pointer).on_click(move |_| {
      state_signal.update(|state| {
        let key = (bottom_up, track, index);
        if !state.toggled.remove(&key) {
          state.toggled.insert(key);
        }
      })
    });
  }
  Row::new()
    .align_items(Alignment::Center)
    .child(name)
    .child(plain_cell(
      &format_ms(node.total_ns as f64 / 1e6),
      TEXT,
      FontWeight::Normal,
    ))
    .child(plain_cell(
      &format!("{:.1}%", tree.percent(node.total_ns)),
      MUTED,
      FontWeight::Normal,
    ))
    .child(plain_cell(
      &format_ms(node.self_ns as f64 / 1e6),
      TEXT,
      FontWeight::Normal,
    ))
    .child(plain_cell(
      &format!("{:.1}%", tree.percent(node.self_ns)),
      MUTED,
      FontWeight::Normal,
    ))
    .child(plain_cell(&node.count.to_string(), MUTED, FontWeight::Normal))
    .height(TABLE_ROW_HEIGHT)
    .padding_horizontal(16.0)
    .width(FILL)
    .border_bottom(Border::inside(1.0, Color::from_hex("#1f1f23")))
    .into()
}

#[cfg(test)]
pub(super) mod tests;
