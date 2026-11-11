#![cfg_attr(not(feature = "perf_profile"), allow(dead_code))]

use std::time::Duration;

pub(crate) mod canvas_upload;
pub use canvas_upload::CanvasAssetUploadProfile;

#[derive(Clone, Default)]
pub struct FrameProfile {
  pub layout: Duration,
  pub layout_recalculated: bool,
  pub quad_resolve: Duration,
  pub glyph_rasterize: Duration,
  pub gpu_submit: Duration,
  pub render: RenderProfile,
  /// False for custom backends that do not implement CPU stage profiling.
  pub render_profile_available: bool,
  pub total: Duration,
  pub quad_count: usize,
  pub rect_count: usize,
  pub glyph_count: usize,
  pub glyph_cache_hits: usize,
  pub glyph_cache_misses: usize,
  pub text_measure_cache_hits: usize,
  pub text_measure_cache_misses: usize,
  pub glyph_engine: GlyphEngineProfile,
  pub memory: RuntimeMemoryProfile,
}

#[derive(Clone, Copy, Default)]
pub struct GlyphEngineProfile {
  pub shape_text: Duration,
  /// Complete caret request, including cache lookup, extraction, and result copies.
  pub caret_total: Duration,
  pub caret_buffer: Duration,
  pub caret_extract: Duration,
  pub caret_requests: usize,
  pub caret_hits: usize,
  pub caret_misses: usize,
  pub caret_returned_positions: usize,
  /// Caret cache misses served from an exact retained full-layout hit.
  pub caret_layout_reuses: usize,
  /// Paragraph-local caret geometry built/reused on caret cache misses.
  pub caret_built_paragraphs: usize,
  pub caret_reused_paragraphs: usize,
  pub caret_built_positions: usize,
  /// Cache-miss ink/optical bounds calculation; includes shaping, glyph visits, and raster misses.
  pub vertical_extents: Duration,
  /// Buffer preparation and shaping inside `vertical_extents` (inclusive sub-timer).
  pub vertical_extents_shape: Duration,
  pub vertical_extents_runs: usize,
  pub vertical_extents_glyphs: usize,
  pub raster_acquire_buffer: Duration,
  /// Text replacement and shaping for a plain-text raster cache miss.
  pub raster_set_text: Duration,
  pub plain_buffer_hits: usize,
  pub plain_buffer_misses: usize,
  pub plain_reused_lines: usize,
  /// Full plain-buffer construction, including the paragraph sub-timers below.
  pub plain_buffer_build: Duration,
  /// Cosmic paragraph shaping, including BiDi analysis and font matching/fallback.
  pub plain_paragraph_shape: Duration,
  /// Wrapping/positioning after paragraph shaping has completed.
  pub plain_paragraph_layout: Duration,
  pub plain_buffer_finalize: Duration,
  pub plain_buffer_retain: Duration,
  pub plain_shaped_paragraphs: usize,
  pub plain_laid_out_paragraphs: usize,
  /// Wrapped paragraph layouts preserved across a width change.
  pub plain_reflow_reuses: usize,
  /// Computing conservative width intervals after fresh paragraph layouts.
  pub plain_reflow_analysis: Duration,
  pub plain_retained_paragraphs: usize,
  /// Old paragraphs indexed for edits/reordering outside matching boundaries.
  pub plain_reindexed_paragraphs: usize,
  /// Paragraphs whose shape allocations were recounted during multiline construction.
  pub plain_accounted_paragraphs: usize,
  /// Retained public Cosmic storage, keys, and shared paragraph carets; excludes transient/private allocations.
  pub plain_cache_bytes: usize,
  pub plain_cache_budget: usize,
  /// Entries whose wrapped layouts were discarded this frame, preserving shaping.
  pub plain_cache_compactions: usize,
  pub plain_cache_evictions: usize,
  pub plain_cache_bypasses: usize,
  pub shape_rich_text: Duration,
  pub rich_acquire_buffer: Duration,
  pub rich_set_text: Duration,
  pub rich_prepare_spans: Duration,
  pub rich_buffer_set_text: Duration,
  pub rich_align_lines: Duration,
  pub rich_cosmic_shape: Duration,
  pub rich_measure: Duration,
  pub rich_extract: Duration,
  pub pack_rich_shaped: Duration,
  pub swash_lookup: Duration,
  pub atlas_pack: Duration,
  pub append_cached: Duration,
  pub swash_requests: usize,
  pub atlas_packs: usize,
  pub rich_text_loads: usize,
  pub rich_single_span_loads: usize,
  pub rich_multi_span_loads: usize,
  pub rich_loaded_spans: usize,
  pub rich_loaded_bytes: usize,
}

#[derive(Clone, Copy, Default)]
pub struct RenderProfile {
  /// Canvas CPU work. Subscopes are inclusive and can overlap encode/upload.
  pub canvas: CanvasProfile,
  pub init: Duration,
  pub acquire: Duration,
  pub globals_upload: Duration,
  pub atlas_upload: Duration,
  pub glyph_atlas_upload_bytes: usize,
  pub glyph_atlas_upload_rects: usize,
  pub glyph_atlas_full_uploads: usize,
  /// Glyph-atlas uploads staged in the existing frame arena.
  pub glyph_atlas_arena_uploads: usize,
  /// New dedicated upload resources allocated for glyph-atlas updates.
  pub glyph_atlas_dedicated_uploads: usize,
  /// Draw calls recorded for the window's render list (quads, glyph runs,
  /// images, SVGs and opacity-layer composites). Canvas draws are not
  /// included; `canvas.batches` counts those.
  pub draw_calls: usize,
  pub buffer_upload: Duration,
  pub image_upload: Duration,
  pub encode: Duration,
  pub submit: Duration,
  pub present: Duration,
  pub total: Duration,
}

#[derive(Clone, Copy, Default)]
pub struct CanvasProfile {
  /// Complete backend Canvas processing, including cache/resource work.
  pub total: Duration,
  /// Command preparation and mesh-cache lookup/miss tessellation.
  pub tessellation: Duration,
  /// Creating/staging new image and text textures (CPU, not GPU completion).
  pub asset_upload: Duration,
  /// DX12 detail only for an encode begun with a live profiling session.
  /// None for WGPU, feature-disabled or uncaptured work; not a GPU metric.
  pub asset_upload_details: Option<CanvasAssetUploadProfile>,
  /// Vertex and constant buffer preparation/staging.
  pub buffer_upload: Duration,
  /// Draw recording including uploads, tile planning and submission where present.
  pub recording: Duration,
  /// WGPU Canvas queue.submit / DX12 explicit readback boundary submission only.
  pub submit: Duration,
  pub batches: usize,
  pub command_groups: usize,
  pub vertices: usize,
  pub tiles: usize,
  pub uploaded_asset_bytes: usize,
  /// The renderer's Canvas asset budget (`CanvasAssetBudget`) in bytes.
  pub asset_cache_budget_bytes: usize,
  /// Charged bytes of the textures the asset cache keeps after the encode.
  pub asset_cache_bytes: usize,
  /// The most the asset cache was charged above its budget during the encode;
  /// above zero, the textures of the current and previous frame need more.
  pub asset_cache_stretch_bytes: usize,
  /// Textures uploaded and drawn but not kept past the encode, because the
  /// textures of the current and previous frame filled the cache's ceiling.
  pub asset_cache_uncached: usize,
  pub asset_cache_uncached_bytes: usize,
}

impl RenderProfile {
  pub fn upload_total(self) -> Duration {
    self.globals_upload + self.atlas_upload + self.buffer_upload + self.image_upload
  }

  pub fn active_total(self) -> Duration {
    self.total.saturating_sub(self.acquire)
  }
}

impl FrameProfile {
  pub fn glyph_cache_hit_rate(&self) -> f32 {
    let total = self.glyph_cache_hits + self.glyph_cache_misses;
    if total == 0 {
      1.0
    } else {
      self.glyph_cache_hits as f32 / total as f32
    }
  }

  pub fn text_measure_hit_rate(&self) -> f32 {
    let total = self.text_measure_cache_hits + self.text_measure_cache_misses;
    if total == 0 {
      1.0
    } else {
      self.text_measure_cache_hits as f32 / total as f32
    }
  }
}

impl std::fmt::Display for FrameProfile {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "total={:.2}ms layout={:.2}ms quads={:.2}ms glyphs={:.2}ms render_cpu={:.2}ms wait={:.2}ms upload={:.2}ms encode={:.2}ms submit={:.2}ms present={:.2}ms | atlas={}B {} rects {} full | {} rects {} glyphs {} quads | text shape={:.2}ms rich_shape={:.2}ms(acq={:.2} set={:.2}[prep={:.2} buffer={:.2} align={:.2}] cosmic={:.2} measure={:.2} extract={:.2} loads={}/{}+{} spans={} bytes={}) rich_pack={:.2}ms swash={:.2}ms/{} atlas_pack={:.2}ms/{} append={:.2}ms | measure hit={:.0}% glyph hit={:.0}% | {}",
      self.total.as_secs_f64() * 1000.0,
      self.layout.as_secs_f64() * 1000.0,
      self.quad_resolve.as_secs_f64() * 1000.0,
      self.glyph_rasterize.as_secs_f64() * 1000.0,
      self.render.active_total().as_secs_f64() * 1000.0,
      self.render.acquire.as_secs_f64() * 1000.0,
      self.render.upload_total().as_secs_f64() * 1000.0,
      self.render.encode.as_secs_f64() * 1000.0,
      self.render.submit.as_secs_f64() * 1000.0,
      self.render.present.as_secs_f64() * 1000.0,
      self.render.glyph_atlas_upload_bytes,
      self.render.glyph_atlas_upload_rects,
      self.render.glyph_atlas_full_uploads,
      self.rect_count,
      self.glyph_count,
      self.quad_count,
      self.glyph_engine.shape_text.as_secs_f64() * 1000.0,
      self.glyph_engine.shape_rich_text.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_acquire_buffer.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_set_text.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_prepare_spans.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_buffer_set_text.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_align_lines.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_cosmic_shape.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_measure.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_extract.as_secs_f64() * 1000.0,
      self.glyph_engine.rich_text_loads,
      self.glyph_engine.rich_single_span_loads,
      self.glyph_engine.rich_multi_span_loads,
      self.glyph_engine.rich_loaded_spans,
      self.glyph_engine.rich_loaded_bytes,
      self.glyph_engine.pack_rich_shaped.as_secs_f64() * 1000.0,
      self.glyph_engine.swash_lookup.as_secs_f64() * 1000.0,
      self.glyph_engine.swash_requests,
      self.glyph_engine.atlas_pack.as_secs_f64() * 1000.0,
      self.glyph_engine.atlas_packs,
      self.glyph_engine.append_cached.as_secs_f64() * 1000.0,
      self.text_measure_hit_rate() * 100.0,
      self.glyph_cache_hit_rate() * 100.0,
      self.memory,
    )?;
    write!(
      f,
      " | text_bounds={:.2}ms(shape={:.2}ms runs={} glyphs={}) raster_buffer={:.2}ms raster_text={:.2}ms buffers={}/{} hit/miss reused_lines={}",
      self.glyph_engine.vertical_extents.as_secs_f64() * 1000.0,
      self.glyph_engine.vertical_extents_shape.as_secs_f64() * 1000.0,
      self.glyph_engine.vertical_extents_runs,
      self.glyph_engine.vertical_extents_glyphs,
      self.glyph_engine.raster_acquire_buffer.as_secs_f64() * 1000.0,
      self.glyph_engine.raster_set_text.as_secs_f64() * 1000.0,
      self.glyph_engine.plain_buffer_hits,
      self.glyph_engine.plain_buffer_misses,
      self.glyph_engine.plain_reused_lines,
    )?;
    write!(
      f,
      " plain_build={:.2}ms(shape={:.2}ms/{} wrap={:.2}ms/{} finish={:.2}ms) retain={:.2}ms reused_paragraphs={} cache={}/{}B compact/evict/bypass={}/{}/{} reindexed/accounted={}/{} reflow_reuses={} reflow_analysis={:.2}ms",
      self.glyph_engine.plain_buffer_build.as_secs_f64() * 1000.0,
      self.glyph_engine.plain_paragraph_shape.as_secs_f64() * 1000.0,
      self.glyph_engine.plain_shaped_paragraphs,
      self.glyph_engine.plain_paragraph_layout.as_secs_f64() * 1000.0,
      self.glyph_engine.plain_laid_out_paragraphs,
      self.glyph_engine.plain_buffer_finalize.as_secs_f64() * 1000.0,
      self.glyph_engine.plain_buffer_retain.as_secs_f64() * 1000.0,
      self.glyph_engine.plain_retained_paragraphs,
      self.glyph_engine.plain_cache_bytes,
      self.glyph_engine.plain_cache_budget,
      self.glyph_engine.plain_cache_compactions,
      self.glyph_engine.plain_cache_evictions,
      self.glyph_engine.plain_cache_bypasses,
      self.glyph_engine.plain_reindexed_paragraphs,
      self.glyph_engine.plain_accounted_paragraphs,
      self.glyph_engine.plain_reflow_reuses,
      self.glyph_engine.plain_reflow_analysis.as_secs_f64() * 1000.0,
    )?;
    write!(
      f,
      " caret={:.2}ms(buffer={:.2}ms extract={:.2}ms) requests/hits/misses={}/{}/{} positions={} layout_reuses={} paragraphs_built/reused={}/{} positions_built={}",
      self.glyph_engine.caret_total.as_secs_f64() * 1000.0,
      self.glyph_engine.caret_buffer.as_secs_f64() * 1000.0,
      self.glyph_engine.caret_extract.as_secs_f64() * 1000.0,
      self.glyph_engine.caret_requests,
      self.glyph_engine.caret_hits,
      self.glyph_engine.caret_misses,
      self.glyph_engine.caret_returned_positions,
      self.glyph_engine.caret_layout_reuses,
      self.glyph_engine.caret_built_paragraphs,
      self.glyph_engine.caret_reused_paragraphs,
      self.glyph_engine.caret_built_positions,
    )
  }
}

#[derive(Clone, Copy, Default)]
pub struct RuntimeMemoryProfile {
  pub total_bytes: usize,
  pub runtime_struct_bytes: usize,
  pub root_tree_bytes: usize,
  pub root_context_bytes: usize,
  pub root_component_bytes: usize,
  pub last_layout_bytes: usize,
  pub glyph_engine_bytes: usize,
  pub render_engine_bytes: usize,
  pub hover_path_bytes: usize,
  pub active_path_bytes: usize,
  pub dragging_scroll_bytes: usize,
}

impl RuntimeMemoryProfile {
  pub fn total_kib(&self) -> f32 {
    self.total_bytes as f32 / 1024.0
  }
}

impl std::fmt::Display for RuntimeMemoryProfile {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "runtime_mem={:.1}KiB root={:.1}KiB ctx={:.1}KiB layout={:.1}KiB glyph_engine={:.1}KiB",
      self.total_kib(),
      self.root_tree_bytes as f32 / 1024.0,
      self.root_context_bytes as f32 / 1024.0,
      self.last_layout_bytes as f32 / 1024.0,
      self.glyph_engine_bytes as f32 / 1024.0,
    )
  }
}
