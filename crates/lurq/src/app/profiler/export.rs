//! Versioned content-free JSON export, reusable outside the MCP transport.
use serde_json::{Value, json};

use super::{CanvasAssetUploadProfile, FrameProfile, model::*};

macro_rules! timings {
  ($value:expr; $($field:ident),* $(,)?) => {{
    let mut map = serde_json::Map::new();
    $(map.insert(stringify!($field).into(), Value::from($value.$field.as_secs_f64() * 1000.));)*
    Value::Object(map)
  }};
}
macro_rules! counts {
  ($value:expr; $($field:ident),* $(,)?) => {{
    let mut map = serde_json::Map::new();
    $(map.insert(stringify!($field).into(), Value::from($value.$field));)*
    Value::Object(map)
  }};
}

impl CanvasAssetUploadProfile {
  pub fn to_json(self) -> Value {
    json!({
      "cpu_timings_ms": timings!(self; texture_creation, pixel_packing, upload_staging_commands,
        descriptor_writes, cache_eviction),
      "counts": counts!(self; cache_hits, cache_misses, texture_creations, descriptor_pairs,
        padded_upload_bytes, arena_uploads, dedicated_uploads, cache_evictions,
        cache_charged_bytes_before, cache_charged_bytes_peak, cache_charged_bytes_after,
        cache_entries_before, cache_entries_after)
    })
  }
}

impl BuildAvailability {
  pub fn to_json(self) -> Value {
    json!({
      "lurq_version": env!("CARGO_PKG_VERSION"), "target_os": std::env::consts::OS,
      "target_arch": std::env::consts::ARCH, "debug_assertions": self.debug_assertions,
      "cargo_profile": "not_embedded", "optimization_level": "not_embedded",
      "features": { "perf_profile": self.perf_profile, "canvas": self.canvas, "wgpu": self.wgpu,
        "dx12": self.dx12, "devtools": self.devtools },
      "application_scopes": super::application_export::availability(self.perf_profile),
      "gpu_timestamps": { "available": false, "reason": "GPU timestamp queries are not instrumented" }
    })
  }
}

impl SessionStarted {
  pub fn to_json(&self) -> Value {
    // A recording's scopes are bounded only by the per-session scope limit.
    let scope_limit = if self.recording_max_bytes.is_some() {
      usize::MAX
    } else {
      self.max_samples
    };
    let mut value = json!({ "schema_version": 1, "status": "recording", "id": format!("profile_{}", self.id.0),
      "started_ms": self.started_ms, "max_samples": self.max_samples, "build": self.build.to_json(),
      "application_scope_max_samples": scope_limit.min(super::MAX_APPLICATION_SCOPES_PER_SESSION),
      "boundary_policy": "whole_completed_operations", "requests_redraw": false });
    if let Some(max_bytes) = self.recording_max_bytes {
      value["recording"] = json!({ "max_bytes": max_bytes });
    }
    value
  }
}

impl ProfileReport {
  /// Export is detached from live state; finalized results stay immutable.
  pub fn to_json(&self) -> Value {
    let app = self.application_scopes.as_ref();
    let status = if !self.in_flight.is_empty() || app.is_some_and(|app| !app.in_flight.is_empty()) {
      "unfinished_work_observed"
    } else if self.samples.is_empty() && app.is_none_or(|app| app.samples.is_empty()) {
      "no_completed_samples"
    } else {
      "completed_samples"
    };
    let newest = self.samples.last().map(|sample| sample.completed_ms);
    let mut value = json!({
      "schema_version": 1, "id": format!("profile_{}", self.id.0), "status": status,
      "finalized": self.finalized, "started_ms": self.started_ms, "ended_ms": self.ended_ms,
      "observed_ms": self.observed_ms, "time_origin": "collector_monotonic_epoch",
      "sample_age_ms": newest.map(|completed| (self.observed_ms - completed).max(0.)),
      "build": self.build.to_json(), "max_samples": self.max_samples,
      "completed_samples": self.completed_samples, "returned_samples": self.samples.len(),
      "dropped_samples": self.dropped_samples, "truncated": self.dropped_samples > 0,
      "boundary_excluded_samples": self.boundary_excluded_samples, "untracked_windows": self.untracked_windows,
      "boundary_policy": "whole_completed_operations; in-flight observations are unfinished and excluded from completed samples",
      "windows": self.windows.iter().map(|window| json!({
        "id": window.id, "open": window.open, "devtools": window.devtools
      })).collect::<Vec<_>>(),
      "in_flight": self.in_flight.iter().map(|current| json!({
        "window": current.window, "frame_id": current.frame_id, "phase": current.phase.name(),
        "started_ms": current.started_ms, "elapsed_so_far_ms": current.elapsed_ms,
        "phase_elapsed_so_far_ms": current.phase_elapsed_ms, "unfinished": true,
        "excluded_from_completed_samples": true, "started_before_session": current.started_before_session
      })).collect::<Vec<_>>(),
      "application_scopes": app.map(|app| app.to_json(self.observed_ms)),
      "samples": self.samples.iter().map(|sample| sample_json(sample)).collect::<Vec<_>>(),
      "scope_semantics": {
        "pass": "Tree::pass wall time; includes nested UI/layout/text/Canvas/render; excludes prior input dispatch",
        "ui_update": "inclusive rebuild/refresh with nested commit; may precede or nest in a pass",
        "layout_update": "inclusive rebuild/resources/layout/Canvas binding/component after-layout hooks",
        "layout_compute": "aggregate runtime-owned LayoutEngine calls including overlay measurements and UI text; nested in layout_update",
        "component_after_layout": "entire root/recursive hook sweep; includes application Canvas painting; nested in layout_update",
        "ui_text": "UI GlyphEngine only; excludes document CanvasTextEngine shaping",
        "canvas_text": "synchronous Canvas text calls on the pass thread; total includes cache work and nested CPU stages; excludes engine lock wait, calls outside a pass and GPU work; bytes count newly produced final RGBA data only",
        "render_encode": "inclusive backend encoding; DX12 includes Canvas and atlas uploads; WGPU includes buffer/image uploads",
        "canvas": "CPU processing; recording includes uploads and WGPU submission; tessellation includes mesh cache lookup",
        "canvas_asset_upload_details": "DX12 captured encodes only; four CPU subscopes nested in asset_upload; cache_eviction is outside asset_upload inside Canvas total; cache byte counters are policy charges, not GPU allocation/RSS; WGPU or uncaptured details are null",
        "submit_present": "CPU API wall time, not GPU execution",
        "fps": "on-demand rendering; sample count/interval is not throughput or FPS"
      }
    });
    // Additive: a bounded session's export has no such key.
    if let Some(recording) = self.recording {
      value["max_samples"] = Value::Null;
      value["recording"] = json!({
        "max_bytes": recording.max_bytes, "size_bytes": recording.size_bytes,
        "stopped_at_limit": recording.stopped_at_limit, "stopped_ms": recording.stopped_ms
      });
    }
    value
  }
}

fn sample_json(sample: &ProfileSample) -> Value {
  let data = match &sample.data {
    SampleData::Pass(pass) => json!({
      "kind": "pass", "frame_id": pass.frame_id, "rendered": pass.rendered,
      "cached_render_list": pass.cached_render_list, "backend": pass.backend,
      "layout_recalculated": pass.layout_recalculated,
      "cpu_timings_ms": timings!(pass; total, layout_update, layout_compute, component_after_layout, canvas_recording, canvas_preparation),
      "canvas_text": pass.canvas_text.map(|text| json!({
        "cpu_timings_ms": timings!(text; total, buffer_font_shape, glyph_prepare, bitmap_composition),
        "counts": counts!(text; measure_calls, fill_calls, shape_calls, shape_cache_hits,
          shape_cache_misses, shape_cache_evictions, shape_cache_uncached, shape_cache_stretch_bytes,
          produced_bitmap_bytes)
      })),
      "frame": pass.frame.as_ref().map(|frame| frame_json(frame, pass.backend)),
      "gpu_timing_ms": null,
      "spans": pass.spans.iter().map(|span| json!({
        "phase": span.phase.name(), "depth": span.depth,
        "start_ms": span.start_ns as f64 / 1e6, "end_ms": span.end_ns as f64 / 1e6
      })).collect::<Vec<_>>(),
      "dropped_spans": pass.dropped_spans,
      // Additive within schema_version 1; null for a pass without counters.
      "counters": pass.counters.map(counters_json)
    }),
    SampleData::UiUpdate(update) => json!({
      "kind": "ui_update", "frame_id": update.frame_id,
      "operation": match update.kind { UiUpdateKind::RootRebuild => "root_rebuild", UiUpdateKind::SubtreeRefresh => "subtree_refresh" },
      "cpu_timings_ms": timings!(update; total, commit), "inclusive_in_pass": update.frame_id.is_some()
    }),
    SampleData::InputDispatch(input) => json!({
      "kind": "input_dispatch", "frame_id": input.frame_id, "rendered": false,
      "category": input.kind.name(), "cpu_timings_ms": timings!(input; total),
      "inclusive_in_pass": input.frame_id.is_some()
    }),
  };
  json!({ "sequence": sample.sequence, "window": sample.window, "started_ms": sample.started_ms,
    "completed_ms": sample.completed_ms, "data": data })
}

fn counters_json(counters: PassCounters) -> Value {
  let allocations = counters.allocations;
  json!({
    "nodes": counters.nodes, "memory_bytes": counters.memory_bytes,
    "caches": counts!(counters.caches; glyph_atlas_bytes, text_cache_bytes, canvas_text_cache_bytes,
      canvas_asset_cache_bytes),
    "allocations": {
      "available": allocations.is_some(),
      "scope": "the pass thread from pass start until its counters are taken; live_bytes is process-wide",
      "allocations": allocations.map(|a| a.allocations), "frees": allocations.map(|a| a.frees),
      "allocated_bytes": allocations.map(|a| a.allocated_bytes), "freed_bytes": allocations.map(|a| a.freed_bytes),
      "live_bytes": allocations.map(|a| a.live_bytes)
    }
  })
}

fn frame_json(frame: &FrameProfile, backend: &str) -> Value {
  let render = frame.render;
  let text = frame.glyph_engine;
  let upload_details_available = cfg!(all(
    feature = "canvas",
    feature = "perf_profile",
    feature = "dx12",
    target_os = "windows"
  )) && frame.render_profile_available
    && backend == "dx12";
  json!({
    "cpu_timings_ms": timings!(frame; total, layout, quad_resolve, glyph_rasterize, gpu_submit),
    "legacy_gpu_submit_semantics": "CPU Canvas preparation + render call; not GPU time",
    "counts": counts!(frame; quad_count, rect_count, glyph_count, glyph_cache_hits, glyph_cache_misses,
      text_measure_cache_hits, text_measure_cache_misses),
    "render": {
      "profile_available": frame.render_profile_available,
      "coverage": {
        "canvas_cpu": cfg!(feature = "canvas") && frame.render_profile_available && matches!(backend, "wgpu" | "dx12"),
        "canvas_asset_upload_details": upload_details_available,
        "canvas_buffer_upload": "WGPU vertex/globals; DX12 vertex only (constants remain inside recording)",
        "ui_buffer_image_upload": "WGPU separate; DX12 included in encode without independent sub-timers",
        "gpu_execution": false
      },
      "cpu_timings_ms": frame.render_profile_available.then(|| timings!(render; total, init, acquire, globals_upload, atlas_upload,
        buffer_upload, image_upload, encode, submit, present)),
      // Render-list draws; Canvas draws are counted by canvas.counts.batches.
      "draw_calls": frame.render_profile_available.then_some(render.draw_calls),
      "glyph_atlas": counts!(render; glyph_atlas_upload_bytes, glyph_atlas_upload_rects,
        glyph_atlas_full_uploads, glyph_atlas_arena_uploads, glyph_atlas_dedicated_uploads),
      "canvas": {
        "cpu_timings_ms": timings!(render.canvas; total, tessellation, asset_upload, buffer_upload, recording, submit),
        "counts": counts!(render.canvas; batches, command_groups, vertices, tiles, uploaded_asset_bytes,
          asset_cache_budget_bytes, asset_cache_bytes, asset_cache_stretch_bytes, asset_cache_uncached,
          asset_cache_uncached_bytes),
        "asset_upload_details": render.canvas.asset_upload_details
          .filter(|_| upload_details_available)
          .map(CanvasAssetUploadProfile::to_json)
      }
    },
    "text": {
      "cpu_timings_ms": timings!(text; shape_text, shape_rich_text, caret_total, caret_buffer, caret_extract,
        vertical_extents, vertical_extents_shape, raster_acquire_buffer, raster_set_text, plain_buffer_build,
        plain_paragraph_shape, plain_paragraph_layout, plain_buffer_finalize, plain_buffer_retain,
        plain_reflow_analysis, rich_acquire_buffer, rich_set_text, rich_prepare_spans, rich_buffer_set_text,
        rich_align_lines, rich_cosmic_shape, rich_measure, rich_extract, pack_rich_shaped,
        swash_lookup, atlas_pack, append_cached),
      "counts": counts!(text; caret_requests, caret_hits, caret_misses, caret_returned_positions,
        caret_layout_reuses, caret_built_paragraphs, caret_reused_paragraphs, caret_built_positions,
        vertical_extents_runs, vertical_extents_glyphs, plain_buffer_hits, plain_buffer_misses,
        plain_reused_lines, plain_shaped_paragraphs, plain_laid_out_paragraphs, plain_reflow_reuses,
        plain_retained_paragraphs, plain_reindexed_paragraphs, plain_accounted_paragraphs,
        plain_cache_bytes, plain_cache_budget, plain_cache_compactions, plain_cache_evictions,
        plain_cache_bypasses, swash_requests, atlas_packs, rich_text_loads, rich_single_span_loads,
        rich_multi_span_loads, rich_loaded_spans, rich_loaded_bytes)
    },
    "memory_bytes": counts!(frame.memory; total_bytes, runtime_struct_bytes, root_tree_bytes,
      root_context_bytes, root_component_bytes, last_layout_bytes, glyph_engine_bytes, render_engine_bytes,
      hover_path_bytes, active_path_bytes, dragging_scroll_bytes),
    "memory_sampling": "cached estimate sampled at most once per second; not a heap allocation trace"
  })
}
