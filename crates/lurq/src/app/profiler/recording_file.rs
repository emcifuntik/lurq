//! The lurq-native recording file: a whole `ProfileReport` as JSON that reads
//! back into the same report, for DevTools' Save and Load. Lossless: durations
//! are whole nanoseconds and every field of the report is kept. It holds what
//! the report holds and nothing more, so no text, prop value, input value or
//! path. Plain data, without a window or a file dialog.
use std::{
  collections::HashSet,
  sync::{Arc, Mutex, OnceLock},
  time::Duration,
};

use serde_json::{Map, Value};

use super::{
  ApplicationInFlight, ApplicationLane, ApplicationRefusals, ApplicationScopeId, ApplicationScopeInfo,
  ApplicationScopeReport, ApplicationScopeSample, CanvasAssetUploadProfile, CanvasProfile, FrameProfile,
  GlyphEngineProfile, MAX_APPLICATION_LABEL_BYTES, RenderProfile, RuntimeMemoryProfile, model::*, phase_slot::PHASES,
};
use crate::app::PassReasons;

/// The `format` of a recording file.
pub const RECORDING_FILE_FORMAT: &str = "lurq-recording";
/// The `format_version` this build writes and reads.
pub const RECORDING_FILE_VERSION: u64 = 1;
/// Distinct names (application scope labels, backends) loaded files may add
/// for the life of the process; a file with new ones past it is refused.
const MAX_LOADED_NAMES: usize = 4096;

/// Why a recording file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingFileError(String);

impl std::fmt::Display for RecordingFileError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "not a lurq recording: {}", self.0)
  }
}

impl std::error::Error for RecordingFileError {}

impl ProfileReport {
  /// The report as a lurq-native recording file, which
  /// [`ProfileReport::from_recording_file`] reads back unchanged.
  pub fn to_recording_file(&self) -> String {
    let file = serde_json::json!({
      "format": RECORDING_FILE_FORMAT,
      "format_version": RECORDING_FILE_VERSION,
      "lurq_version": env!("CARGO_PKG_VERSION"),
      "report": self.encode(),
    });
    serde_json::to_string(&file).expect("a JSON value serializes")
  }

  /// Reads a file [`ProfileReport::to_recording_file`] wrote.
  pub fn from_recording_file(text: &str) -> Result<Self, RecordingFileError> {
    let file: Value = serde_json::from_str(text).map_err(|error| RecordingFileError(error.to_string()))?;
    if file.get("format").and_then(Value::as_str) != Some(RECORDING_FILE_FORMAT) {
      return Err(RecordingFileError(format!("format is not {RECORDING_FILE_FORMAT}")));
    }
    match file.get("format_version").and_then(Value::as_u64) {
      Some(RECORDING_FILE_VERSION) => {}
      Some(version) => {
        return Err(RecordingFileError(format!(
          "format_version {version} is not {RECORDING_FILE_VERSION}; save it again with this lurq version"
        )));
      }
      None => return Err(RecordingFileError("no format_version".to_owned())),
    }
    let map = file.as_object().expect("format was read from an object");
    field(map, "report").map_err(RecordingFileError)
  }
}

/// A value of the file: written as JSON and read back exactly.
trait Field: Sized {
  fn encode(&self) -> Value;
  /// The error starts with the keys below `value` that lead to what failed.
  fn decode(value: &Value) -> Result<Self, String>;
}

/// `key` of `map`; a missing key reads as null.
fn field<T: Field>(map: &Map<String, Value>, key: &str) -> Result<T, String> {
  T::decode(map.get(key).unwrap_or(&Value::Null)).map_err(|error| format!("{key}: {error}"))
}

/// A struct as an object of its fields. The pattern and the literal name every
/// field, so a field added to the struct must be added here. `field as codec`
/// writes and reads that field with the module `codec` instead of `Field`.
macro_rules! record {
  ($ty:ident { $($field:ident $(as $codec:ident)?),* $(,)? }) => {
    impl Field for $ty {
      fn encode(&self) -> Value {
        let $ty { $($field),* } = self;
        let mut map = Map::new();
        $(map.insert(stringify!($field).into(), record!(@encode $field $($codec)?));)*
        Value::Object(map)
      }

      fn decode(value: &Value) -> Result<Self, String> {
        let map = value.as_object().ok_or("expected an object")?;
        Ok($ty { $($field: record!(@decode map $field $($codec)?)),* })
      }
    }
  };
  (@encode $field:ident) => {
    $field.encode()
  };
  (@encode $field:ident $codec:ident) => {
    $codec::encode($field)
  };
  (@decode $map:ident $field:ident) => {
    field($map, stringify!($field))?
  };
  (@decode $map:ident $field:ident $codec:ident) => {
    $codec::decode($map.get(stringify!($field)).unwrap_or(&Value::Null))
      .map_err(|error| format!("{}: {error}", stringify!($field)))?
  };
}

impl Field for bool {
  fn encode(&self) -> Value {
    Value::Bool(*self)
  }

  fn decode(value: &Value) -> Result<Self, String> {
    value.as_bool().ok_or_else(|| "expected a boolean".to_owned())
  }
}

macro_rules! integer {
  ($($ty:ty),*) => {$(
    impl Field for $ty {
      fn encode(&self) -> Value {
        Value::from(*self)
      }

      fn decode(value: &Value) -> Result<Self, String> {
        value
          .as_u64()
          .and_then(|value| <$ty>::try_from(value).ok())
          .ok_or_else(|| concat!("expected a ", stringify!($ty)).to_owned())
      }
    }
  )*};
}
integer!(u8, u32, u64, usize);

/// Exact with serde_json's `float_roundtrip`.
impl Field for f64 {
  fn encode(&self) -> Value {
    Value::from(*self)
  }

  fn decode(value: &Value) -> Result<Self, String> {
    value.as_f64().ok_or_else(|| "expected a number".to_owned())
  }
}

/// Whole nanoseconds.
impl Field for Duration {
  fn encode(&self) -> Value {
    Value::from(u64::try_from(self.as_nanos()).unwrap_or(u64::MAX))
  }

  fn decode(value: &Value) -> Result<Self, String> {
    u64::decode(value)
      .map(Duration::from_nanos)
      .map_err(|_| "expected nanoseconds".to_owned())
  }
}

impl Field for String {
  fn encode(&self) -> Value {
    Value::from(self.as_str())
  }

  fn decode(value: &Value) -> Result<Self, String> {
    value
      .as_str()
      .map(str::to_owned)
      .ok_or_else(|| "expected a string".to_owned())
  }
}

impl Field for Arc<str> {
  fn encode(&self) -> Value {
    Value::from(self.as_ref())
  }

  fn decode(value: &Value) -> Result<Self, String> {
    String::decode(value).map(Arc::from)
  }
}

/// Static identifiers of the toolkit or of the application's source, such as
/// a render backend or an application scope label.
impl Field for &'static str {
  /// As [`file_name`], which [`intern`] always reads back.
  fn encode(&self) -> Value {
    Value::from(file_name(self))
  }

  fn decode(value: &Value) -> Result<Self, String> {
    intern(value.as_str().ok_or("expected a string")?)
  }
}

/// A byte a name in the file may hold: an ASCII letter or digit, `_`, `-` or
/// `.`. No whitespace or path separator.
fn is_name_byte(byte: u8) -> bool {
  byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

/// `name` as the file keeps it. Scope labels and the toolkit's backends are
/// kept as they are; another name (a custom `RenderEngine::profile_backend`)
/// has each other character replaced by `_` and is cut to
/// `MAX_APPLICATION_LABEL_BYTES`, so that every saved file loads.
fn file_name(name: &str) -> std::borrow::Cow<'_, str> {
  if !name.is_empty() && name.len() <= MAX_APPLICATION_LABEL_BYTES && name.bytes().all(is_name_byte) {
    return name.into();
  }
  let mut kept: String = name
    .chars()
    .map(|char| match u8::try_from(char) {
      Ok(byte) if is_name_byte(byte) => char,
      _ => '_',
    })
    .take(MAX_APPLICATION_LABEL_BYTES)
    .collect();
  if kept.is_empty() {
    kept.push('_');
  }
  kept.into()
}

/// `name` as a `&'static str`, each distinct name leaked once. Only names
/// [`file_name`] writes are taken, so a file cannot carry text in them.
fn intern(name: &str) -> Result<&'static str, String> {
  let valid = !name.is_empty() && name.len() <= MAX_APPLICATION_LABEL_BYTES && name.bytes().all(is_name_byte);
  if !valid {
    return Err(format!(
      "expected a name of at most {MAX_APPLICATION_LABEL_BYTES} ASCII letters, digits, _, - or ."
    ));
  }
  leak_once(name)
}

/// Longest short type name a file may hold.
const MAX_TYPE_NAME_BYTES: usize = 256;

/// The short static type names of a detailed recording (components, the value
/// types of signals and memos) as `detail::short_type_name` makes them, so a
/// tuple or array type keeps its brackets, commas and spaces. Never a path
/// separator, quote or control character.
mod type_name {
  use super::*;

  pub(super) fn encode<T: TypeName>(name: &T) -> Value {
    name.encode_name()
  }

  pub(super) fn decode<T: TypeName>(value: &Value) -> Result<T, String> {
    T::decode_name(value)
  }

  pub(super) trait TypeName: Sized {
    fn encode_name(&self) -> Value;
    fn decode_name(value: &Value) -> Result<Self, String>;
  }

  impl TypeName for &'static str {
    fn encode_name(&self) -> Value {
      Value::from(*self)
    }

    fn decode_name(value: &Value) -> Result<Self, String> {
      let name = value.as_str().ok_or("expected a string")?;
      let valid = !name.is_empty()
        && name.len() <= MAX_TYPE_NAME_BYTES
        && name
          .bytes()
          .all(|byte| (byte.is_ascii_graphic() || byte == b' ') && !matches!(byte, b'/' | b'\\' | b'"'));
      if !valid {
        return Err(format!(
          "expected a type name of at most {MAX_TYPE_NAME_BYTES} ASCII characters, without / \\ or \""
        ));
      }
      leak_once(name)
    }
  }

  impl TypeName for Option<&'static str> {
    fn encode_name(&self) -> Value {
      self.map_or(Value::Null, |name| name.encode_name())
    }

    fn decode_name(value: &Value) -> Result<Self, String> {
      if value.is_null() {
        Ok(None)
      } else {
        <&'static str>::decode_name(value).map(Some)
      }
    }
  }
}

/// Why GPU time is not measured: one of `GpuTimestampAvailability::REASONS`,
/// by its text.
mod gpu_reason {
  use super::*;

  pub(super) fn encode(reason: &Option<&'static str>) -> Value {
    reason.map_or(Value::Null, Value::from)
  }

  pub(super) fn decode(value: &Value) -> Result<Option<&'static str>, String> {
    if value.is_null() {
      return Ok(None);
    }
    let reason = value.as_str().ok_or("expected a string")?;
    GpuTimestampAvailability::REASONS
      .into_iter()
      .find(|known| *known == reason)
      .map(Some)
      .ok_or_else(|| format!("unknown reason {reason:?}"))
  }
}

/// `name`, already checked, as a `&'static str` leaked once.
fn leak_once(name: &str) -> Result<&'static str, String> {
  static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
  let mut names = NAMES
    .get_or_init(Default::default)
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner());
  if let Some(known) = names.get(name) {
    return Ok(known);
  }
  if names.len() >= MAX_LOADED_NAMES {
    return Err(format!("more than {MAX_LOADED_NAMES} distinct names loaded"));
  }
  let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
  names.insert(leaked);
  Ok(leaked)
}

impl<T: Field> Field for Option<T> {
  fn encode(&self) -> Value {
    self.as_ref().map_or(Value::Null, Field::encode)
  }

  fn decode(value: &Value) -> Result<Self, String> {
    if value.is_null() {
      Ok(None)
    } else {
      T::decode(value).map(Some)
    }
  }
}

impl<T: Field> Field for Vec<T> {
  fn encode(&self) -> Value {
    Value::Array(self.iter().map(Field::encode).collect())
  }

  fn decode(value: &Value) -> Result<Self, String> {
    value
      .as_array()
      .ok_or("expected an array")?
      .iter()
      .enumerate()
      .map(|(index, item)| T::decode(item).map_err(|error| format!("{index}: {error}")))
      .collect()
  }
}

impl<T: Field> Field for Arc<T> {
  fn encode(&self) -> Value {
    self.as_ref().encode()
  }

  fn decode(value: &Value) -> Result<Self, String> {
    T::decode(value).map(Arc::new)
  }
}

/// An enum without data, by the name it has in reports.
macro_rules! named {
  ($ty:ty, $all:expr) => {
    impl Field for $ty {
      fn encode(&self) -> Value {
        Value::from(self.name())
      }

      fn decode(value: &Value) -> Result<Self, String> {
        let name = value.as_str().ok_or("expected a string")?;
        $all
          .into_iter()
          .find(|item: &$ty| item.name() == name)
          .ok_or_else(|| format!("unknown {} {name:?}", stringify!($ty)))
      }
    }
  };
}
named!(Phase, PHASES);
named!(InputKind, [InputKind::Pointer, InputKind::Keyboard, InputKind::Scroll]);
named!(ApplicationLane, [ApplicationLane::Ui, ApplicationLane::Worker]);
named!(
  RenderTrigger,
  [
    RenderTrigger::Signal,
    RenderTrigger::Memo,
    RenderTrigger::Store,
    RenderTrigger::Context,
    RenderTrigger::Form,
    RenderTrigger::Props,
    RenderTrigger::SlotChildren,
    RenderTrigger::Mount,
    RenderTrigger::ListItem,
    RenderTrigger::Unknown,
  ]
);

impl Field for UiUpdateKind {
  fn encode(&self) -> Value {
    Value::from(match self {
      Self::RootRebuild => "root_rebuild",
      Self::SubtreeRefresh => "subtree_refresh",
    })
  }

  fn decode(value: &Value) -> Result<Self, String> {
    match value.as_str() {
      Some("root_rebuild") => Ok(Self::RootRebuild),
      Some("subtree_refresh") => Ok(Self::SubtreeRefresh),
      _ => Err("expected root_rebuild or subtree_refresh".to_owned()),
    }
  }
}

impl Field for SessionId {
  fn encode(&self) -> Value {
    self.0.encode()
  }

  fn decode(value: &Value) -> Result<Self, String> {
    u64::decode(value).map(Self)
  }
}

impl Field for ApplicationScopeId {
  fn encode(&self) -> Value {
    self.0.encode()
  }

  fn decode(value: &Value) -> Result<Self, String> {
    u64::decode(value).map(Self)
  }
}

/// `{"pass": {...}}`, `{"ui_update": {...}}` or `{"input_dispatch": {...}}`.
impl Field for SampleData {
  fn encode(&self) -> Value {
    let (kind, data) = match self {
      Self::Pass(pass) => ("pass", pass.encode()),
      Self::UiUpdate(update) => ("ui_update", update.encode()),
      Self::InputDispatch(input) => ("input_dispatch", input.encode()),
    };
    Value::Object(Map::from_iter([(kind.to_owned(), data)]))
  }

  fn decode(value: &Value) -> Result<Self, String> {
    let map = value.as_object().ok_or("expected an object")?;
    match map.iter().next() {
      Some((kind, _)) if map.len() == 1 => match kind.as_str() {
        "pass" => field(map, "pass").map(Self::Pass),
        "ui_update" => field(map, "ui_update").map(Self::UiUpdate),
        "input_dispatch" => field(map, "input_dispatch").map(Self::InputDispatch),
        other => Err(format!("unknown sample kind {other:?}")),
      },
      _ => Err("expected one of pass, ui_update or input_dispatch".to_owned()),
    }
  }
}

/// The screenshots of a recording go into the file with it: they were charged
/// to the session's `max_bytes` like its samples, so a file stays as bounded
/// as the recording it holds. Pixels as lowercase hex, two characters a byte.
impl Field for FrameScreenshot {
  fn encode(&self) -> Value {
    let Self {
      window,
      frame_id,
      captured_ms,
      width,
      height,
      rgba,
    } = self;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut pixels = String::with_capacity(rgba.len() * 2);
    for byte in rgba {
      pixels.push(char::from(HEX[usize::from(byte >> 4)]));
      pixels.push(char::from(HEX[usize::from(byte & 15)]));
    }
    let mut map = Map::new();
    map.insert("window".into(), window.encode());
    map.insert("frame_id".into(), frame_id.encode());
    map.insert("captured_ms".into(), captured_ms.encode());
    map.insert("width".into(), width.encode());
    map.insert("height".into(), height.encode());
    map.insert("rgba".into(), Value::from(pixels));
    Value::Object(map)
  }

  fn decode(value: &Value) -> Result<Self, String> {
    let map = value.as_object().ok_or("expected an object")?;
    let width: u32 = field(map, "width")?;
    let height: u32 = field(map, "height")?;
    let pixels = map
      .get("rgba")
      .and_then(Value::as_str)
      .ok_or("rgba: expected a hex string")?;
    let expected = (width as usize)
      .checked_mul(height as usize)
      .and_then(|area| area.checked_mul(8));
    if expected != Some(pixels.len()) {
      return Err("rgba: expected width x height RGBA pixels".to_owned());
    }
    let nibble = |byte: u8| match byte {
      b'0'..=b'9' => Ok(byte - b'0'),
      b'a'..=b'f' => Ok(byte - b'a' + 10),
      _ => Err("rgba: expected lowercase hex".to_owned()),
    };
    let rgba = pixels
      .as_bytes()
      .chunks_exact(2)
      .map(|pair| Ok(nibble(pair[0])? << 4 | nibble(pair[1])?))
      .collect::<Result<Vec<u8>, String>>()?;
    Ok(Self {
      window: field(map, "window")?,
      frame_id: field(map, "frame_id")?,
      captured_ms: field(map, "captured_ms")?,
      width,
      height,
      rgba,
    })
  }
}

record!(ProfileReport {
  id,
  finalized,
  started_ms,
  ended_ms,
  observed_ms,
  build,
  max_samples,
  completed_samples,
  dropped_samples,
  boundary_excluded_samples,
  untracked_windows,
  windows,
  in_flight,
  samples,
  application_scopes,
  recording,
  screenshots,
});
record!(BuildAvailability {
  perf_profile,
  debug_assertions,
  canvas,
  wgpu,
  dx12,
  devtools,
  gpu_timestamps,
});
record!(GpuTimestampAvailability {
  available,
  reason as gpu_reason,
  adapter_backend,
});
record!(WindowStatus {
  id,
  open,
  devtools,
  gpu_timestamps,
});
record!(InFlightObservation {
  window,
  frame_id,
  phase,
  started_ms,
  elapsed_ms,
  phase_elapsed_ms,
  started_before_session,
});
record!(RecordingStatus {
  max_bytes,
  size_bytes,
  stopped_at_limit,
  stopped_ms,
  detailed,
  screenshots,
  screenshot_bytes,
});
record!(ProfileSample {
  sequence,
  window,
  started_ms,
  completed_ms,
  data,
});
record!(PassSample {
  frame_id,
  rendered,
  cached_render_list,
  total,
  layout_update,
  layout_compute,
  component_after_layout,
  layout_recalculated,
  canvas_recording,
  canvas_preparation,
  canvas_text,
  backend,
  frame,
  spans,
  dropped_spans,
  reasons,
  present,
  counters,
  gpu,
  component_layouts,
  screenshot,
});
record!(GpuTiming { ui, canvas });
record!(ComponentLayouts {
  component as type_name,
  reason,
  nodes,
});
record!(RenderCause {
  component as type_name,
  source,
  via,
});
record!(RenderSource {
  trigger,
  value_type as type_name,
});
record!(PassReasons {
  redraw_requested,
  scheduled_redraw,
  timer_run,
  timer_active,
  future_completed,
  future_active,
  timeline_active,
  continuous_redraw_image,
  perf_overlay,
  pending_click,
  input_interaction,
  text_input_caret,
  theme_changed,
  component_dirty,
  element_ref_dirty,
  layout_dirty,
});
record!(PresentTiming {
  presented_ms,
  interval,
  refresh_interval,
  continuous,
  missed_vsyncs,
});
record!(PassCounters {
  nodes,
  memory_bytes,
  caches,
  allocations,
});
record!(CacheSizes {
  glyph_atlas_bytes,
  text_cache_bytes,
  canvas_text_cache_bytes,
  canvas_asset_cache_bytes,
});
record!(PassAllocations {
  allocations,
  frees,
  allocated_bytes,
  freed_bytes,
  live_bytes,
});
record!(PhaseSpan {
  phase,
  depth,
  start_ns,
  end_ns,
  component as type_name,
});
record!(CanvasTextProfile {
  measure_calls,
  fill_calls,
  shape_calls,
  shape_cache_hits,
  shape_cache_misses,
  shape_cache_evictions,
  shape_cache_uncached,
  shape_cache_stretch_bytes,
  produced_bitmap_bytes,
  total,
  buffer_font_shape,
  glyph_prepare,
  bitmap_composition,
});
record!(UiUpdateSample {
  frame_id,
  kind,
  total,
  commit,
  causes,
});
record!(InputDispatchSample {
  frame_id,
  kind,
  total,
  interaction_id,
  queue_delay,
  presented_frame_id,
  input_to_present,
});
record!(FrameProfile {
  layout,
  layout_recalculated,
  quad_resolve,
  glyph_rasterize,
  gpu_submit,
  render,
  render_profile_available,
  total,
  quad_count,
  rect_count,
  glyph_count,
  glyph_cache_hits,
  glyph_cache_misses,
  text_measure_cache_hits,
  text_measure_cache_misses,
  glyph_engine,
  memory,
});
record!(GlyphEngineProfile {
  shape_text,
  caret_total,
  caret_buffer,
  caret_extract,
  caret_requests,
  caret_hits,
  caret_misses,
  caret_returned_positions,
  caret_layout_reuses,
  caret_built_paragraphs,
  caret_reused_paragraphs,
  caret_built_positions,
  vertical_extents,
  vertical_extents_shape,
  vertical_extents_runs,
  vertical_extents_glyphs,
  raster_acquire_buffer,
  raster_set_text,
  plain_buffer_hits,
  plain_buffer_misses,
  plain_reused_lines,
  plain_buffer_build,
  plain_paragraph_shape,
  plain_paragraph_layout,
  plain_buffer_finalize,
  plain_buffer_retain,
  plain_shaped_paragraphs,
  plain_laid_out_paragraphs,
  plain_reflow_reuses,
  plain_reflow_analysis,
  plain_retained_paragraphs,
  plain_reindexed_paragraphs,
  plain_accounted_paragraphs,
  plain_cache_bytes,
  plain_cache_budget,
  plain_cache_compactions,
  plain_cache_evictions,
  plain_cache_bypasses,
  shape_rich_text,
  rich_acquire_buffer,
  rich_set_text,
  rich_prepare_spans,
  rich_buffer_set_text,
  rich_align_lines,
  rich_cosmic_shape,
  rich_measure,
  rich_extract,
  pack_rich_shaped,
  swash_lookup,
  atlas_pack,
  append_cached,
  swash_requests,
  atlas_packs,
  rich_text_loads,
  rich_single_span_loads,
  rich_multi_span_loads,
  rich_loaded_spans,
  rich_loaded_bytes,
});
record!(RenderProfile {
  canvas,
  init,
  acquire,
  globals_upload,
  atlas_upload,
  glyph_atlas_upload_bytes,
  glyph_atlas_upload_rects,
  glyph_atlas_full_uploads,
  glyph_atlas_arena_uploads,
  glyph_atlas_dedicated_uploads,
  draw_calls,
  buffer_upload,
  image_upload,
  encode,
  submit,
  present,
  total,
});
record!(CanvasProfile {
  total,
  tessellation,
  asset_upload,
  asset_upload_details,
  buffer_upload,
  recording,
  submit,
  batches,
  command_groups,
  vertices,
  tiles,
  uploaded_asset_bytes,
  asset_cache_budget_bytes,
  asset_cache_bytes,
  asset_cache_stretch_bytes,
  asset_cache_uncached,
  asset_cache_uncached_bytes,
});
record!(CanvasAssetUploadProfile {
  texture_creation,
  pixel_packing,
  upload_staging_commands,
  descriptor_writes,
  cache_eviction,
  cache_hits,
  cache_misses,
  texture_creations,
  descriptor_pairs,
  padded_upload_bytes,
  arena_uploads,
  dedicated_uploads,
  cache_evictions,
  cache_charged_bytes_before,
  cache_charged_bytes_peak,
  cache_charged_bytes_after,
  cache_entries_before,
  cache_entries_after,
});
record!(RuntimeMemoryProfile {
  total_bytes,
  runtime_struct_bytes,
  root_tree_bytes,
  root_context_bytes,
  root_component_bytes,
  last_layout_bytes,
  glyph_engine_bytes,
  render_engine_bytes,
  hover_path_bytes,
  active_path_bytes,
  dragging_scroll_bytes,
});
record!(ApplicationScopeReport {
  available,
  max_live_scopes,
  max_samples,
  started_scopes,
  completed_scopes,
  dropped_scopes,
  boundary_excluded_scopes,
  abandoned_window_closed,
  abandoned_producer_closed,
  refused,
  in_flight,
  samples,
});
record!(ApplicationRefusals {
  invalid_label,
  window_unavailable,
  collector_closed,
  live_limit,
  invalid_parent,
});
record!(ApplicationInFlight {
  scope,
  elapsed_ms,
  started_before_session,
});
record!(ApplicationScopeSample {
  scope,
  completed_ms,
  wall,
});
record!(ApplicationScopeInfo {
  id,
  parent_id,
  depth,
  window,
  label,
  lane,
  started_ms,
});
