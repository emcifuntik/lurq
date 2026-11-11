//! GPU time per frame from wgpu timestamp queries, measured only while a
//! profiling session is active. The UI encoder writes a timestamp at the start
//! of its first render pass and at the end of the window pass; each Canvas
//! command buffer is bracketed by two empty compute passes that write one
//! timestamp each. The frame's queries are resolved in the UI encoder and
//! copied to a pooled readback buffer, mapped asynchronously; a later frame
//! reads it and attaches the result to its pass by frame id.
use std::{
  sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
  },
  time::Duration,
};

use crate::app::profiler::{GpuTimestampAvailability, GpuTiming, ProfileContext};

/// Canvas command buffers measured per frame; a frame with more leaves its
/// Canvas time unmeasured.
const MAX_CANVAS_PAIRS: u32 = 255;
/// The UI pair, then one pair per Canvas command buffer.
const QUERY_COUNT: u32 = 2 + 2 * MAX_CANVAS_PAIRS;
/// Frames whose results may be awaited at once; a frame that finds none free
/// is not measured.
const READBACK_SLOTS: usize = 4;

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

pub(super) struct GpuTimer {
  /// The device has TIMESTAMP_QUERY.
  enabled: bool,
  /// The device can record the compute passes bracketing Canvas command buffers.
  #[cfg_attr(not(feature = "canvas"), allow(dead_code))]
  canvas_marks: bool,
  period_ns: f64,
  availability: GpuTimestampAvailability,
  /// Created by the first measured frame.
  resources: Option<Resources>,
  /// The frame being measured, from `begin_frame` to `submitted`.
  frame: Option<FrameQueries>,
  in_flight: Vec<Readback>,
  spare: Vec<wgpu::Buffer>,
  /// The context the availability was last reported to.
  reported: Option<ProfileContext>,
}

struct Resources {
  query_set: wgpu::QuerySet,
  resolve: wgpu::Buffer,
}

struct FrameQueries {
  frame_id: u64,
  readback: wgpu::Buffer,
  /// The next free query: 2 until a Canvas command buffer was measured.
  next_query: u32,
  canvas_complete: bool,
}

struct Readback {
  frame_id: u64,
  buffer: wgpu::Buffer,
  queries: u32,
  canvas_complete: bool,
  state: Arc<AtomicU8>,
}

/// Brackets the Canvas command buffers of one measured frame.
#[cfg(feature = "canvas")]
pub(super) struct CanvasMarks {
  query_set: wgpu::QuerySet,
  next_query: u32,
  open: bool,
  overflow: bool,
}

impl GpuTimer {
  pub(super) fn new() -> Self {
    Self {
      enabled: false,
      canvas_marks: false,
      period_ns: 0.0,
      availability: GpuTimestampAvailability::default(),
      resources: None,
      frame: None,
      in_flight: Vec::new(),
      spare: Vec::new(),
      reported: None,
    }
  }

  /// The features to request so that GPU time can be measured on `adapter`.
  pub(super) fn requested_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & wgpu::Features::TIMESTAMP_QUERY
  }

  /// Takes the new device's capabilities; reports them on the next frame.
  pub(super) fn configure(&mut self, adapter: &wgpu::Adapter, device: &wgpu::Device, queue: &wgpu::Queue) {
    self.release();
    self.enabled = device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    self.canvas_marks = self.enabled
      && adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS);
    self.period_ns = f64::from(queue.get_timestamp_period());
    let backend = adapter.get_info().backend.to_str();
    self.availability = GpuTimestampAvailability {
      available: self.enabled,
      reason: (!self.enabled).then_some(GpuTimestampAvailability::NO_TIMESTAMP_QUERY),
      adapter_backend: Some(backend),
    };
  }

  /// Forgets the device: its queries, buffers and awaited results.
  pub(super) fn release(&mut self) {
    self.enabled = false;
    self.canvas_marks = false;
    self.resources = None;
    self.frame = None;
    self.in_flight.clear();
    self.spare.clear();
    self.reported = None;
  }

  /// Starts a frame: reports availability once per context and, only while a
  /// session is active, reads results that arrived and prepares the frame's
  /// queries. With no session nothing is queried and awaited results are dropped.
  pub(super) fn begin_frame(&mut self, device: &wgpu::Device, context: Option<&ProfileContext>) {
    if let Some(frame) = self.frame.take() {
      // A frame that never submitted leaves its buffer unused.
      self.spare.push(frame.readback);
    }
    let Some(context) = context else {
      return;
    };
    if !self
      .reported
      .as_ref()
      .is_some_and(|reported| reported.same_window(context))
    {
      context.report_gpu_timestamps(self.availability);
      self.reported = Some(context.clone());
    }
    if !self.enabled || !context.capture_active() {
      self.in_flight.clear();
      return;
    }
    self.collect(device, context);
    let Some(frame_id) = context.frame_id() else {
      return;
    };
    let readback = match self.spare.pop() {
      Some(buffer) => buffer,
      None if self.in_flight.len() < READBACK_SLOTS => device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lurq_gpu_timing_readback"),
        size: u64::from(QUERY_COUNT * wgpu::QUERY_SIZE),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      }),
      None => return,
    };
    self.resources.get_or_insert_with(|| Resources {
      query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("lurq_gpu_timing"),
        ty: wgpu::QueryType::Timestamp,
        count: QUERY_COUNT,
      }),
      resolve: device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lurq_gpu_timing_resolve"),
        size: u64::from(QUERY_COUNT * wgpu::QUERY_SIZE),
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
      }),
    });
    self.frame = Some(FrameQueries {
      frame_id,
      readback,
      next_query: 2,
      canvas_complete: true,
    });
  }

  /// The marks for this frame's Canvas command buffers; None when it is not measured.
  #[cfg(feature = "canvas")]
  pub(super) fn canvas_marks(&self) -> Option<CanvasMarks> {
    let frame = self.frame.as_ref().filter(|_| self.canvas_marks)?;
    Some(CanvasMarks {
      query_set: self.resources.as_ref()?.query_set.clone(),
      next_query: frame.next_query,
      open: false,
      overflow: false,
    })
  }

  #[cfg(feature = "canvas")]
  pub(super) fn canvas_done(&mut self, marks: Option<CanvasMarks>) {
    if let (Some(frame), Some(marks)) = (&mut self.frame, marks) {
      frame.next_query = marks.next_query;
      frame.canvas_complete &= !marks.overflow;
    }
  }

  /// Timestamp writes for a UI render pass: the start of the first and the end
  /// of the last (the window pass). None when this frame is not measured.
  pub(super) fn ui_pass_writes(&self, first: bool, last: bool) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
    self.frame.as_ref()?;
    (first || last).then(|| wgpu::RenderPassTimestampWrites {
      query_set: &self.resources.as_ref().unwrap().query_set,
      beginning_of_pass_write_index: first.then_some(0),
      end_of_pass_write_index: last.then_some(1),
    })
  }

  /// Resolves this frame's queries into its readback buffer, after its passes.
  pub(super) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
    let (Some(frame), Some(resources)) = (&self.frame, &self.resources) else {
      return;
    };
    encoder.resolve_query_set(&resources.query_set, 0..frame.next_query, &resources.resolve, 0);
    encoder.copy_buffer_to_buffer(
      &resources.resolve,
      0,
      &frame.readback,
      0,
      u64::from(frame.next_query * wgpu::QUERY_SIZE),
    );
  }

  /// Maps this frame's results once the encoder that resolved them was submitted.
  pub(super) fn submitted(&mut self) {
    let Some(frame) = self.frame.take() else {
      return;
    };
    let state = Arc::new(AtomicU8::new(PENDING));
    let mapped = state.clone();
    let size = u64::from(frame.next_query * wgpu::QUERY_SIZE);
    frame
      .readback
      .slice(..size)
      .map_async(wgpu::MapMode::Read, move |result| {
        mapped.store(if result.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
      });
    self.in_flight.push(Readback {
      frame_id: frame.frame_id,
      buffer: frame.readback,
      queries: frame.next_query,
      canvas_complete: frame.canvas_complete,
      state,
    });
  }

  /// Attaches every result that arrived to its pass; a failed readback leaves
  /// its pass without GPU time.
  fn collect(&mut self, device: &wgpu::Device, context: &ProfileContext) {
    if self.in_flight.is_empty() {
      return;
    }
    let _ = device.poll(wgpu::PollType::Poll);
    let period_ns = self.period_ns;
    let spare = &mut self.spare;
    self
      .in_flight
      .retain(|readback| match readback.state.load(Ordering::Acquire) {
        PENDING => true,
        MAPPED => {
          let size = u64::from(readback.queries * wgpu::QUERY_SIZE);
          let ticks: Vec<u64> = readback
            .buffer
            .slice(..size)
            .get_mapped_range()
            .chunks_exact(8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
            .collect();
          readback.buffer.unmap();
          if let Some(timing) = gpu_timing(&ticks, readback.canvas_complete, period_ns) {
            context.record_gpu(readback.frame_id, timing);
          }
          spare.push(readback.buffer.clone());
          false
        }
        _ => false,
      });
  }
}

#[cfg(feature = "canvas")]
impl CanvasMarks {
  /// Writes the start timestamp of a Canvas command buffer, if a pair is left.
  pub(super) fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) {
    if self.next_query + 2 > QUERY_COUNT {
      self.overflow = true;
      return;
    }
    self.mark(encoder, Some(self.next_query), None);
    self.open = true;
  }

  /// Writes the end timestamp of the command buffer `begin` opened.
  pub(super) fn end(&mut self, encoder: &mut wgpu::CommandEncoder) {
    if !std::mem::take(&mut self.open) {
      return;
    }
    self.mark(encoder, None, Some(self.next_query + 1));
    self.next_query += 2;
  }

  fn mark(&self, encoder: &mut wgpu::CommandEncoder, beginning: Option<u32>, end: Option<u32>) {
    encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
      label: Some("lurq_gpu_timing_mark"),
      timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
        query_set: &self.query_set,
        beginning_of_pass_write_index: beginning,
        end_of_pass_write_index: end,
      }),
    });
  }
}

/// Opens a measured Canvas command buffer when this frame is measured.
#[cfg(feature = "canvas")]
pub(super) fn begin_canvas(marks: &mut Option<CanvasMarks>, encoder: &mut wgpu::CommandEncoder) {
  if let Some(marks) = marks {
    marks.begin(encoder);
  }
}

/// Closes the Canvas command buffer `begin_canvas` opened, before it is finished.
#[cfg(feature = "canvas")]
pub(super) fn end_canvas(marks: &mut Option<CanvasMarks>, encoder: &mut wgpu::CommandEncoder) {
  if let Some(marks) = marks {
    marks.end(encoder);
  }
}

/// A frame's GPU time from its resolved ticks: the UI pair, then one pair per
/// Canvas command buffer. None when the UI pair is invalid; Canvas None when
/// the frame had no Canvas command buffer, not all were measured or one is invalid.
pub(super) fn gpu_timing(ticks: &[u64], canvas_complete: bool, period_ns: f64) -> Option<GpuTiming> {
  let span = |pair: &[u64]| {
    (pair[0] != 0 && pair[1] >= pair[0]).then(|| Duration::from_nanos(((pair[1] - pair[0]) as f64 * period_ns) as u64))
  };
  let ui = span(ticks.get(..2)?)?;
  let canvas = &ticks[2..];
  Some(GpuTiming {
    ui,
    canvas: if canvas.is_empty() || !canvas_complete {
      None
    } else {
      canvas.chunks_exact(2).map(span).sum()
    },
  })
}

// The frames they render include a Canvas command buffer.
#[cfg(all(test, feature = "canvas"))]
mod tests;
