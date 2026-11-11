//! A finished Profiler-tab recording, prepared for display: its passes and
//! markers in time order, time ranges over them, the frame rate of a range and
//! the detail of one pass. Holds only the content-free samples of the session.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use crate::app::profiler::{PassSample, ProfileReport, ProfileSample, SampleData};

/// One 60 Hz refresh: the timeline draws its budget line here and colours the
/// passes that take longer.
pub(crate) const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;
/// An interval between two presented frames longer than this is the app
/// idling, not its frame rate, and is left out of the FPS.
pub(crate) const IDLE_GAP_MS: f64 = 250.0;
/// The most passes the timeline draws a bar for; a longer range asks for a
/// narrower one.
pub(crate) const MAX_TIMELINE_PASSES: usize = 600;

/// Milliseconds since the collector epoch, as in every `*_ms` of a report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TimeRange {
  pub start_ms: f64,
  pub end_ms: f64,
}

impl TimeRange {
  pub(crate) fn duration_ms(self) -> f64 {
    (self.end_ms - self.start_ms).max(0.0)
  }

  /// Where `ms` falls in the range, from 0 at its start to 1 at its end.
  pub(crate) fn fraction(self, ms: f64) -> f64 {
    let duration = self.duration_ms();
    if duration <= 0.0 {
      0.0
    } else {
      ((ms - self.start_ms) / duration).clamp(0.0, 1.0)
    }
  }
}

pub(crate) struct ProfilerCapture {
  pub started_ms: f64,
  pub ended_ms: f64,
  /// Every pass sample of the recording, ordered by start.
  pub passes: Vec<Arc<ProfileSample>>,
  /// Input dispatch and UI update samples, ordered by start.
  pub markers: Vec<Arc<ProfileSample>>,
  pub size_bytes: u64,
  pub stopped_at_limit: bool,
  /// Operations that began before Start, left out of the recording.
  pub boundary_excluded: u64,
}

impl ProfilerCapture {
  pub(crate) fn from_report(report: &ProfileReport) -> Self {
    let (mut passes, mut markers): (Vec<_>, Vec<_>) = report
      .samples
      .iter()
      .cloned()
      .partition(|sample| matches!(sample.data, SampleData::Pass(_)));
    let by_start = |a: &Arc<ProfileSample>, b: &Arc<ProfileSample>| {
      a.started_ms.total_cmp(&b.started_ms).then(a.sequence.cmp(&b.sequence))
    };
    passes.sort_by(by_start);
    markers.sort_by(by_start);
    Self {
      started_ms: report.started_ms,
      ended_ms: report.ended_ms.unwrap_or(report.observed_ms).max(report.started_ms),
      passes,
      markers,
      size_bytes: report.recording.map_or(0, |recording| recording.size_bytes),
      stopped_at_limit: report.recording.is_some_and(|recording| recording.stopped_at_limit),
      boundary_excluded: report.boundary_excluded_samples,
    }
  }

  pub(crate) fn whole(&self) -> TimeRange {
    TimeRange {
      start_ms: self.started_ms,
      end_ms: self.ended_ms,
    }
  }

  /// The whole recording, or its last `MAX_TIMELINE_PASSES` passes when it
  /// holds more.
  pub(crate) fn default_range(&self) -> TimeRange {
    let whole = self.whole();
    match self.passes.len().checked_sub(MAX_TIMELINE_PASSES) {
      Some(first) if first > 0 => TimeRange {
        start_ms: self.passes[first].started_ms,
        ..whole
      },
      _ => whole,
    }
  }

  /// The passes that start in `range`.
  pub(crate) fn passes_in(&self, range: TimeRange) -> &[Arc<ProfileSample>] {
    starting_in(&self.passes, range)
  }

  /// The input dispatch and UI update samples that start in `range`.
  pub(crate) fn markers_in(&self, range: TimeRange) -> &[Arc<ProfileSample>] {
    starting_in(&self.markers, range)
  }

  pub(crate) fn pass(&self, sequence: u64) -> Option<&ProfileSample> {
    self
      .passes
      .iter()
      .find(|sample| sample.sequence == sequence)
      .map(Arc::as_ref)
  }

  /// The longest pass that starts in `range`.
  pub(crate) fn slowest_in(&self, range: TimeRange) -> Option<u64> {
    self
      .passes_in(range)
      .iter()
      .filter_map(|sample| pass_of(sample).map(|pass| (sample.sequence, pass.total)))
      .max_by_key(|(_, total)| *total)
      .map(|(sequence, _)| sequence)
  }

  /// `buckets` equal parts of the recording, each with its longest pass in
  /// milliseconds (0 for none).
  pub(crate) fn overview(&self, buckets: usize) -> Vec<f64> {
    let whole = self.whole();
    let mut longest = vec![0.0_f64; buckets];
    if buckets == 0 {
      return longest;
    }
    for sample in &self.passes {
      if let Some(pass) = pass_of(sample) {
        let bucket = bucket_of(whole, sample.started_ms, buckets);
        longest[bucket] = longest[bucket].max(ms(pass.total));
      }
    }
    longest
  }

  /// The time range of overview buckets `first..=last`, in either order.
  pub(crate) fn bucket_range(&self, first: usize, last: usize, buckets: usize) -> TimeRange {
    let whole = self.whole();
    let (first, last) = (first.min(last), first.max(last));
    let width = whole.duration_ms() / buckets.max(1) as f64;
    TimeRange {
      start_ms: whole.start_ms + width * first as f64,
      end_ms: (whole.start_ms + width * (last + 1) as f64).min(whole.end_ms),
    }
  }
}

/// The overview bucket `ms` falls in.
pub(crate) fn bucket_of(range: TimeRange, ms: f64, buckets: usize) -> usize {
  ((range.fraction(ms) * buckets as f64) as usize).min(buckets.saturating_sub(1))
}

fn starting_in(samples: &[Arc<ProfileSample>], range: TimeRange) -> &[Arc<ProfileSample>] {
  let start = samples.partition_point(|sample| sample.started_ms < range.start_ms);
  let end = samples.partition_point(|sample| sample.started_ms <= range.end_ms);
  &samples[start..end.max(start)]
}

pub(crate) fn pass_of(sample: &ProfileSample) -> Option<&PassSample> {
  match &sample.data {
    SampleData::Pass(pass) => Some(pass),
    SampleData::UiUpdate(_) | SampleData::InputDispatch(_) => None,
  }
}

pub(crate) fn ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

/// Frames per second while the app renders: the presented frames of the
/// window that presented most among `passes`, over the intervals between them
/// up to `IDLE_GAP_MS`. None when no two presented frames are that close, as
/// when the app is idle.
pub(crate) fn fps(passes: &[Arc<ProfileSample>]) -> Option<f64> {
  let mut presented: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
  for sample in passes {
    if pass_of(sample).is_some_and(|pass| pass.rendered) {
      presented
        .entry(sample.window.as_str())
        .or_default()
        .push(sample.completed_ms);
    }
  }
  presented
    .into_values()
    .filter_map(|mut times| {
      times.sort_by(f64::total_cmp);
      let (count, total_ms) = times
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|interval| *interval > 0.0 && *interval <= IDLE_GAP_MS)
        .fold((0_usize, 0.0_f64), |(count, total), interval| {
          (count + 1, total + interval)
        });
      (count > 0).then(|| (count, count as f64 * 1000.0 / total_ms))
    })
    .max_by_key(|(count, _)| *count)
    .map(|(_, fps)| fps)
}

/// One phase span of a pass, placed on the pass's own time axis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PhaseRow {
  pub name: &'static str,
  pub depth: u8,
  pub offset_ms: f64,
  pub duration_ms: f64,
}

/// The pass's spans as rows, offsets from the pass's start; the first span is
/// the pass itself.
pub(crate) fn phase_rows(pass: &PassSample) -> Vec<PhaseRow> {
  let Some(origin) = pass.spans.first().map(|span| span.start_ns) else {
    return Vec::new();
  };
  let ns = |value: u64| value as f64 / 1e6;
  pass
    .spans
    .iter()
    .map(|span| PhaseRow {
      name: if span.depth == 0 { "pass" } else { span.phase.name() },
      depth: span.depth,
      offset_ms: ns(span.start_ns.saturating_sub(origin)),
      duration_ms: ns(span.end_ns.saturating_sub(span.start_ns)),
    })
    .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DetailSection {
  pub title: &'static str,
  pub rows: Vec<(&'static str, String)>,
}

/// Rows labelled with the field names, for durations.
macro_rules! timing_rows {
  ($value:expr; $($field:ident),* $(,)?) => {
    [$((stringify!($field), format_duration($value.$field))),*]
  };
}

/// Rows labelled with the field names, for counts and flags.
macro_rules! count_rows {
  ($value:expr; $($field:ident),* $(,)?) => {
    [$((stringify!($field), $value.$field.to_string())),*]
  };
}

fn section(title: &'static str, rows: impl IntoIterator<Item = (&'static str, String)>) -> DetailSection {
  DetailSection {
    title,
    rows: rows.into_iter().collect(),
  }
}

/// Everything the sample holds about one pass, its frame profile included,
/// in labelled sections.
pub(crate) fn pass_detail(sample: &ProfileSample, pass: &PassSample) -> Vec<DetailSection> {
  let identity = [
    ("window", sample.window.clone()),
    ("sequence", format!("#{}", sample.sequence)),
    (
      "frame_id",
      pass
        .frame_id
        .map_or_else(|| "-".to_owned(), |frame_id| frame_id.to_string()),
    ),
    ("backend", pass.backend.to_owned()),
    ("spans", pass.spans.len().to_string()),
  ];
  let mut sections = vec![section(
    "PASS",
    identity
      .into_iter()
      .chain(count_rows!(pass; rendered, cached_render_list, layout_recalculated, dropped_spans))
      .chain(
        timing_rows!(pass; total, layout_update, layout_compute, component_after_layout,
        canvas_recording, canvas_preparation),
      ),
  )];

  if let Some(text) = &pass.canvas_text {
    sections.push(section(
      "CANVAS TEXT",
      timing_rows!(text; total, buffer_font_shape, glyph_prepare, bitmap_composition)
        .into_iter()
        .chain(
          count_rows!(text; measure_calls, fill_calls, shape_calls, shape_cache_hits, shape_cache_misses,
          shape_cache_evictions, shape_cache_uncached, shape_cache_stretch_bytes, produced_bitmap_bytes),
        ),
    ));
  }

  let Some(frame) = &pass.frame else {
    return sections;
  };
  sections.push(section(
    "FRAME",
    timing_rows!(frame; total, layout, quad_resolve, glyph_rasterize, gpu_submit)
      .into_iter()
      .chain(count_rows!(frame; layout_recalculated, render_profile_available)),
  ));
  sections.push(section(
    "COUNTS",
    count_rows!(frame; quad_count, rect_count, glyph_count, glyph_cache_hits, glyph_cache_misses,
      text_measure_cache_hits, text_measure_cache_misses),
  ));

  let render = frame.render;
  sections.push(section(
    "RENDER",
    timing_rows!(render; total, init, acquire, globals_upload, atlas_upload, buffer_upload, image_upload,
      encode, submit, present)
    .into_iter()
    .chain(
      count_rows!(render; glyph_atlas_upload_bytes, glyph_atlas_upload_rects, glyph_atlas_full_uploads,
      glyph_atlas_arena_uploads, glyph_atlas_dedicated_uploads),
    ),
  ));

  let canvas = render.canvas;
  sections.push(section(
    "CANVAS",
    timing_rows!(canvas; total, tessellation, asset_upload, buffer_upload, recording, submit)
      .into_iter()
      .chain(
        count_rows!(canvas; batches, command_groups, vertices, tiles, uploaded_asset_bytes,
        asset_cache_budget_bytes, asset_cache_stretch_bytes, asset_cache_uncached, asset_cache_uncached_bytes),
      ),
  ));

  if let Some(upload) = canvas.asset_upload_details {
    sections.push(section(
      "CANVAS ASSET UPLOAD",
      timing_rows!(upload; texture_creation, pixel_packing, upload_staging_commands, descriptor_writes,
        cache_eviction)
      .into_iter()
      .chain(
        count_rows!(upload; cache_hits, cache_misses, texture_creations, descriptor_pairs,
        padded_upload_bytes, arena_uploads, dedicated_uploads, cache_evictions, cache_charged_bytes_before,
        cache_charged_bytes_peak, cache_charged_bytes_after, cache_entries_before, cache_entries_after),
      ),
    ));
  }

  let text = frame.glyph_engine;
  sections.push(section(
    "TEXT",
    timing_rows!(text; shape_text, caret_total, caret_buffer, caret_extract, vertical_extents,
      vertical_extents_shape, raster_acquire_buffer, raster_set_text, plain_buffer_build, plain_paragraph_shape,
      plain_paragraph_layout, plain_buffer_finalize, plain_buffer_retain, plain_reflow_analysis, shape_rich_text,
      rich_acquire_buffer, rich_set_text, rich_prepare_spans, rich_buffer_set_text, rich_align_lines,
      rich_cosmic_shape, rich_measure, rich_extract, pack_rich_shaped, swash_lookup, atlas_pack, append_cached)
    .into_iter()
    .chain(
      count_rows!(text; caret_requests, caret_hits, caret_misses, caret_returned_positions,
      caret_layout_reuses, caret_built_paragraphs, caret_reused_paragraphs, caret_built_positions,
      vertical_extents_runs, vertical_extents_glyphs, plain_buffer_hits, plain_buffer_misses, plain_reused_lines,
      plain_shaped_paragraphs, plain_laid_out_paragraphs, plain_reflow_reuses, plain_retained_paragraphs,
      plain_reindexed_paragraphs, plain_accounted_paragraphs, plain_cache_bytes, plain_cache_budget,
      plain_cache_compactions, plain_cache_evictions, plain_cache_bypasses, swash_requests, atlas_packs,
      rich_text_loads, rich_single_span_loads, rich_multi_span_loads, rich_loaded_spans, rich_loaded_bytes),
    ),
  ));

  let memory = frame.memory;
  sections.push(section(
    "MEMORY",
    count_rows!(memory; total_bytes, runtime_struct_bytes, root_tree_bytes, root_context_bytes,
      root_component_bytes, last_layout_bytes, glyph_engine_bytes, render_engine_bytes, hover_path_bytes,
      active_path_bytes, dragging_scroll_bytes),
  ));
  sections
}

pub(crate) fn format_duration(duration: Duration) -> String {
  format_ms(ms(duration))
}

pub(crate) fn format_ms(value: f64) -> String {
  format!("{value:.2}ms")
}

/// A span of time for the status line and the axis: ms below a second.
pub(crate) fn format_span(value_ms: f64) -> String {
  if value_ms.abs() < 1000.0 {
    format!("{value_ms:.0}ms")
  } else {
    format!("{:.2}s", value_ms / 1000.0)
  }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
  const KIB: f64 = 1024.0;
  let value = bytes as f64;
  if value < KIB {
    format!("{bytes} B")
  } else if value < KIB * KIB {
    format!("{:.1} KiB", value / KIB)
  } else if value < KIB * KIB * KIB {
    format!("{:.1} MiB", value / (KIB * KIB))
  } else {
    format!("{:.2} GiB", value / (KIB * KIB * KIB))
  }
}
