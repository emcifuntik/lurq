#![allow(unsafe_op_in_unsafe_fn)]
#[cfg(feature = "canvas")]
mod canvas;
mod layers;

#[cfg(feature = "raster")]
use std::collections::{HashMap, HashSet};
#[cfg(feature = "raster")]
use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::{
  ffi::c_void,
  mem::ManuallyDrop,
  ptr,
  time::{Duration, Instant},
};

use raw_window_handle::{DisplayHandle, RawWindowHandle, WindowHandle};
#[cfg(feature = "svg")]
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32_UINT;
#[cfg(feature = "raster")]
use windows::Win32::Graphics::{
  Direct3D12::{D3D12_HEAP_FLAG_SHARED, D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS, D3D12_RESOURCE_STATE_COMMON},
  Dxgi::{
    Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_R8G8_UNORM},
    IDXGIKeyedMutex,
  },
};
#[cfg(feature = "raster")]
use windows::core::PCWSTR;
use windows::{
  Win32::{
    Foundation::{CloseHandle, FALSE, HANDLE, HWND, RECT, TRUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Graphics::{
      Direct3D::{
        D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        Fxc::{D3DCOMPILE_DEBUG, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompile},
        ID3DBlob,
      },
      Direct3D12::{
        D3D_ROOT_SIGNATURE_VERSION_1, D3D12_BLEND_DESC, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_ONE, D3D12_BLEND_OP_ADD,
        D3D12_BLEND_SRC_ALPHA, D3D12_COLOR_WRITE_ENABLE_ALL, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC,
        D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12_COMPARISON_FUNC_ALWAYS, D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_CULL_MODE_NONE,
        D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING, D3D12_DEPTH_STENCIL_DESC, D3D12_DEPTH_WRITE_MASK_ZERO,
        D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_FLAG_NONE, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
        D3D12_DESCRIPTOR_HEAP_FLAGS, D3D12_DESCRIPTOR_HEAP_TYPE, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        D3D12_DESCRIPTOR_HEAP_TYPE_RTV, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, D3D12_DESCRIPTOR_RANGE,
        D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND, D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER, D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        D3D12_FENCE_FLAG_NONE, D3D12_FILL_MODE_SOLID, D3D12_FILTER_MIN_MAG_MIP_LINEAR, D3D12_GPU_DESCRIPTOR_HANDLE,
        D3D12_GRAPHICS_PIPELINE_STATE_DESC, D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT,
        D3D12_HEAP_TYPE_UPLOAD, D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED, D3D12_INDEX_BUFFER_VIEW,
        D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA, D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        D3D12_INPUT_ELEMENT_DESC, D3D12_INPUT_LAYOUT_DESC, D3D12_LOGIC_OP_NOOP, D3D12_MEMORY_POOL_UNKNOWN,
        D3D12_PIPELINE_STATE_FLAG_NONE, D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        D3D12_RANGE, D3D12_RASTERIZER_DESC, D3D12_RENDER_TARGET_BLEND_DESC, D3D12_RENDER_TARGET_VIEW_DESC,
        D3D12_RENDER_TARGET_VIEW_DESC_0, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0,
        D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
        D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_BUFFER,
        D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST,
        D3D12_RESOURCE_STATE_GENERIC_READ, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_PRESENT,
        D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATES, D3D12_RESOURCE_TRANSITION_BARRIER,
        D3D12_ROOT_DESCRIPTOR, D3D12_ROOT_DESCRIPTOR_TABLE, D3D12_ROOT_PARAMETER, D3D12_ROOT_PARAMETER_0,
        D3D12_ROOT_PARAMETER_TYPE_CBV, D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, D3D12_ROOT_PARAMETER_TYPE_SRV,
        D3D12_ROOT_SIGNATURE_DESC, D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        D3D12_RTV_DIMENSION_TEXTURE2D, D3D12_SAMPLER_DESC, D3D12_SHADER_BYTECODE, D3D12_SHADER_RESOURCE_VIEW_DESC,
        D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SHADER_VISIBILITY_ALL, D3D12_SRV_DIMENSION_TEXTURE2D,
        D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEX2D_RTV, D3D12_TEX2D_SRV, D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0, D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_TEXTURE_LAYOUT_UNKNOWN,
        D3D12_VERTEX_BUFFER_VIEW, D3D12_VIEWPORT, D3D12CreateDevice, D3D12SerializeRootSignature,
        ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue, ID3D12DescriptorHeap, ID3D12Device, ID3D12Fence,
        ID3D12GraphicsCommandList, ID3D12PipelineState, ID3D12Resource, ID3D12RootSignature,
      },
      Dxgi::{
        Common::{
          DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
          DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT,
          DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
        },
        CreateDXGIFactory2, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_CREATE_FACTORY_DEBUG, DXGI_CREATE_FACTORY_FLAGS,
        DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_DRIVER_INTERNAL_ERROR,
        DXGI_GPU_PREFERENCE_UNSPECIFIED, DXGI_MWA_NO_ALT_ENTER, DXGI_SCALING_NONE, DXGI_SWAP_CHAIN_DESC1,
        DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_DISCARD,
        DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter1, IDXGIFactory4, IDXGIFactory6, IDXGIOutput, IDXGISwapChain2,
        IDXGISwapChain3,
      },
    },
    System::Threading::{CreateEventW, WaitForSingleObject},
  },
  core::{Error, HRESULT, Interface, PCSTR, Result},
};

/// `windows` 0.62 renamed the last-thread Win32 error constructor.
trait Win32ErrorCompat {
  fn from_win32() -> Self;
}

impl Win32ErrorCompat for Error {
  fn from_win32() -> Self {
    Self::from_thread()
  }
}

#[cfg(feature = "screenshot")]
use windows::Win32::Graphics::Direct3D12::{
  D3D12_BOX, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT,
};

#[cfg(feature = "screenshot")]
use crate::app::{
  frame_capture::{align_capture_row_pitch, capture_rows_to_rgba, finish_capture},
  render_engine::RenderFrameCapture,
};
#[cfg(feature = "raster")]
use crate::render::gpu::ImageInstance;
#[cfg(feature = "svg")]
use crate::render::gpu::SvgVertexGpu;
use crate::{
  app::{
    profile_support::{profile_elapsed, profile_if, profile_scope},
    profile_types::RenderProfile,
    render_engine::RenderEngine,
  },
  layout::{
    opacity_layer::{LayerPlan, LayerStep, TargetSpace},
    quad::ClipRect,
    render_list::{GlyphCmd, RectCmd, RenderList},
  },
  render::gpu::{Globals, GlyphInstance, QuadInstance, QuadVertex},
};

const FRAME_COUNT: usize = 2;
const SRV_DESCRIPTOR_COUNT: u32 = 1024;
const GLYPH_ATLAS_SRV_INDEX: usize = 0;
// The incremental per-dirty-rect glyph-atlas upload drops occasional glyphs on
// D3D12 (a wide glyph uploads blank, leaving a gap mid-word) — a subtle issue in
// the per-rect `CopyTextureRegion` path that the wgpu backend's direct copy
// doesn't hit. Until that's root-caused, re-upload the whole atlas whenever it
// changes. The atlas only changes when new glyphs are rasterized, so this is
// infrequent; the full-upload path is the same one used on atlas resize.
const DX12_ALWAYS_FULL_ATLAS_UPLOAD: bool = true;
#[allow(dead_code)]
const IMAGE_SRV_FIRST_INDEX: usize = GLYPH_ATLAS_SRV_INDEX + 1;
const FRAME_UPLOAD_ARENA_BYTES: usize = 32 * 1024 * 1024;
const DX12_FRAME_WAIT_TIMEOUT_MS: u32 = 32;
/// Screenshots block the render thread on the copy's fence; they are
/// rare and user-invoked, so wait longer than the per-frame budget before
/// dropping the capture.
#[cfg(feature = "screenshot")]
const DX12_CAPTURE_WAIT_TIMEOUT_MS: u32 = 250;
const DX12_SLOW_FRAME_THRESHOLD: Duration = Duration::from_millis(12);
const DX12_SLOW_PHASE_THRESHOLD: Duration = Duration::from_millis(6);
const DX12_FRAME_NOT_READY: HRESULT = HRESULT(0x800705B4u32 as i32);
const SWAPCHAIN_FORMAT: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
/// Not sRGB: pixel shaders write sRGB-encoded colour, so alpha blending mixes
/// encoded values like CSS instead of linear light.
const RENDER_TARGET_FORMAT: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT = SWAPCHAIN_FORMAT;

#[cfg(feature = "raster")]
static DX12_NATIVE_IMAGE_DRAW_COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "raster")]
static DX12_IMAGE_MOD_NOT_FOUND_LOGGED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "raster")]
static DX12_IMAGE_DRAWS_DISABLED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "raster")]
fn dx12_native_image_log(message: impl std::fmt::Display) {
  crate::app::profile_support::video_log!(debug, "[dx12/native-image] {message}");
}

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

fn log_slow_dx12_render_frame(width: u32, height: u32, init: Duration, profile: &RenderProfile, total: Duration) {
  let upload = profile.upload_total();
  let render_work = total.saturating_sub(profile.acquire);
  if render_work < DX12_SLOW_FRAME_THRESHOLD
    && upload < DX12_SLOW_PHASE_THRESHOLD
    && profile.encode < DX12_SLOW_PHASE_THRESHOLD
    && profile.submit < DX12_SLOW_PHASE_THRESHOLD
    && profile.present < DX12_SLOW_PHASE_THRESHOLD
  {
    return;
  }

  crate::app::profile_support::video_log!(debug,
    target: "video::watch::lurq",
    "dx12 render slow frame size={}x{} total_ms={:.2} work_ms={:.2} init_ms={:.2} wait_ms={:.2} upload_ms={:.2} atlas_upload_ms={:.2} image_upload_ms={:.2} encode_ms={:.2} submit_ms={:.2} present_ms={:.2} atlas_bytes={} atlas_rects={} atlas_full={}",
    width,
    height,
    duration_ms(total),
    duration_ms(render_work),
    duration_ms(init),
    duration_ms(profile.acquire),
    duration_ms(upload),
    duration_ms(profile.atlas_upload),
    duration_ms(profile.image_upload),
    duration_ms(profile.encode),
    duration_ms(profile.submit),
    duration_ms(profile.present),
    profile.glyph_atlas_upload_bytes,
    profile.glyph_atlas_upload_rects,
    profile.glyph_atlas_full_uploads
  );
}

#[derive(Debug)]
struct Dx12RenderCadence {
  started_at: Instant,
  renders: u32,
  slow_renders: u32,
  last_render_at: Option<Instant>,
  max_render_delta: Duration,
  max_total: Duration,
  max_acquire: Duration,
  max_encode: Duration,
  max_present: Duration,
  last_rects: usize,
  last_glyphs: usize,
  last_images: usize,
}

impl Dx12RenderCadence {
  fn new(now: Instant) -> Self {
    Self {
      started_at: now,
      renders: 0,
      slow_renders: 0,
      last_render_at: None,
      max_render_delta: Duration::ZERO,
      max_total: Duration::ZERO,
      max_acquire: Duration::ZERO,
      max_encode: Duration::ZERO,
      max_present: Duration::ZERO,
      last_rects: 0,
      last_glyphs: 0,
      last_images: 0,
    }
  }
}

#[cfg(feature = "raster")]
fn acquire_native_nv12_mutex(
  mutex: Option<&IDXGIKeyedMutex>,
  image_id: u64,
  version: u64,
  plane: &'static str,
) -> bool {
  let Some(mutex) = mutex else {
    return false;
  };

  let started_at = Instant::now();
  let result = unsafe { mutex.AcquireSync(1, 5) };
  let elapsed = started_at.elapsed();
  let acquired = result.is_ok();
  if !acquired || elapsed >= Duration::from_millis(2) {
    crate::app::profile_support::video_log!(debug,
      target: "video::watch::lurq",
      "watched stream native texture mutex image_id={} version={} plane={} acquired={} wait_ms={:.2} error={:?}",
      image_id,
      version,
      plane,
      acquired,
      duration_ms(elapsed),
      result.err()
    );
  }
  acquired
}

pub struct Dx12RenderEngine {
  #[cfg(feature = "canvas")]
  canvases: Vec<crate::canvas::CanvasHandle>,
  #[cfg(feature = "canvas")]
  canvas_asset_budget: crate::canvas::CanvasAssetBudget,
  state: Option<Dx12State>,
  width: u32,
  height: u32,
  render_cadence: Dx12RenderCadence,
  #[cfg(feature = "screenshot")]
  pending_frame_capture: Option<RenderFrameCapture>,
  #[cfg(feature = "perf_profile")]
  last_profile: RenderProfile,
  #[cfg(feature = "perf_profile")]
  profile_context: Option<crate::app::profiler::ProfileContext>,
  #[cfg(feature = "raster")]
  video_surfaces: Option<Dx12VideoSurfaceAllocator>,
}

#[cfg(feature = "raster")]
#[derive(Clone, Default)]
pub struct Dx12VideoSurfaceAllocator {
  device: Arc<Mutex<Option<ID3D12Device>>>,
}

#[cfg(feature = "raster")]
pub struct Dx12Nv12Surface {
  native: crate::images::NativeImageData,
  _y_texture: ID3D12Resource,
  _uv_texture: ID3D12Resource,
  y_shared_handle: HANDLE,
  uv_shared_handle: Option<HANDLE>,
  y_allocation_size: u64,
  uv_allocation_size: u64,
  adapter_luid_low: u32,
  adapter_luid_high: i32,
  owns_shared_handles: bool,
  packed_nv12: bool,
}

#[cfg(feature = "raster")]
unsafe impl Send for Dx12Nv12Surface {}
#[cfg(feature = "raster")]
unsafe impl Sync for Dx12Nv12Surface {}

impl Default for Dx12RenderEngine {
  fn default() -> Self {
    Self::new()
  }
}

impl Dx12RenderEngine {
  pub fn new() -> Self {
    Self {
      state: None,
      #[cfg(feature = "canvas")]
      canvases: Vec::new(),
      #[cfg(feature = "canvas")]
      canvas_asset_budget: Default::default(),
      width: 800,
      height: 600,
      render_cadence: Dx12RenderCadence::new(Instant::now()),
      #[cfg(feature = "screenshot")]
      pending_frame_capture: None,
      #[cfg(feature = "perf_profile")]
      last_profile: RenderProfile::default(),
      #[cfg(feature = "perf_profile")]
      profile_context: None,
      #[cfg(feature = "raster")]
      video_surfaces: None,
    }
  }

  /// Opacity-layer textures the engine keeps pooled.
  #[cfg(all(test, feature = "screenshot"))]
  pub(crate) fn layer_texture_count(&self) -> usize {
    self
      .state
      .as_ref()
      .and_then(|state| state.layer_compositor.as_ref())
      .map_or(0, layers::LayerCompositor::texture_count)
  }

  /// How many charged bytes of uploaded Canvas images and text this renderer
  /// keeps between frames; 64 MiB by default. See [`CanvasAssetBudget`] for
  /// how a frame that needs more is handled.
  ///
  /// [`CanvasAssetBudget`]: crate::canvas::CanvasAssetBudget
  #[cfg(feature = "canvas")]
  pub fn with_canvas_asset_budget(mut self, budget: crate::canvas::CanvasAssetBudget) -> Self {
    self.canvas_asset_budget = budget;
    self
  }

  #[cfg(feature = "raster")]
  pub fn with_video_surface_allocator(video_surfaces: Dx12VideoSurfaceAllocator) -> Self {
    Self {
      video_surfaces: Some(video_surfaces),
      ..Self::new()
    }
  }

  fn ensure_initialized(&mut self, window: WindowHandle<'_>) -> Result<()> {
    if self.state.is_some() {
      return Ok(());
    }

    let hwnd = hwnd_from_window(window)?;
    #[cfg_attr(not(feature = "canvas"), allow(unused_mut))]
    let mut state = unsafe { Dx12State::new(hwnd, self.width.max(1), self.height.max(1))? };
    #[cfg(feature = "canvas")]
    {
      state.canvas_asset_budget = self.canvas_asset_budget;
    }
    #[cfg(feature = "raster")]
    if let Some(video_surfaces) = &self.video_surfaces {
      video_surfaces.set_device(Some(state.device.clone()));
    }
    self.state = Some(state);
    Ok(())
  }

  fn record_render_cadence(&mut self, list: &RenderList, total: Duration, profile: &RenderProfile) {
    let now = Instant::now();
    let stats = &mut self.render_cadence;
    stats.renders += 1;
    if let Some(last_render_at) = stats.last_render_at {
      stats.max_render_delta = stats.max_render_delta.max(now.duration_since(last_render_at));
    }
    stats.last_render_at = Some(now);
    if total >= DX12_SLOW_FRAME_THRESHOLD {
      stats.slow_renders += 1;
    }
    stats.max_total = stats.max_total.max(total);
    stats.max_acquire = stats.max_acquire.max(profile.acquire);
    stats.max_encode = stats.max_encode.max(profile.encode);
    stats.max_present = stats.max_present.max(profile.present);
    stats.last_rects = list.rects.len();
    stats.last_glyphs = list.glyphs.len();
    #[cfg(feature = "raster")]
    {
      stats.last_images = list.images.len();
    }
    #[cfg(not(feature = "raster"))]
    {
      stats.last_images = 0;
    }

    if now.duration_since(stats.started_at) < Duration::from_secs(1) {
      return;
    }

    crate::app::profile_support::video_log!(debug,
      target: "video::watch::lurq",
      "dx12 render cadence renders={} slow={} max_render_delta_ms={:.2} max_total_ms={:.2} max_wait_ms={:.2} max_encode_ms={:.2} max_present_ms={:.2} rects={} glyphs={} images={}",
      stats.renders,
      stats.slow_renders,
      duration_ms(stats.max_render_delta),
      duration_ms(stats.max_total),
      duration_ms(stats.max_acquire),
      duration_ms(stats.max_encode),
      duration_ms(stats.max_present),
      stats.last_rects,
      stats.last_glyphs,
      stats.last_images
    );

    let last_render_at = stats.last_render_at;
    *stats = Dx12RenderCadence::new(now);
    stats.last_render_at = last_render_at;
  }
}

#[cfg(feature = "raster")]
impl Dx12VideoSurfaceAllocator {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn create_nv12_surface(&self, width: u32, height: u32) -> Result<Option<Dx12Nv12Surface>> {
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
      return Err(Error::from_win32());
    }
    let device = self
      .device
      .lock()
      .expect("dx12 video surface allocator lock poisoned")
      .clone();
    let Some(device) = device else {
      return Ok(None);
    };

    unsafe {
      let (adapter_luid_low, adapter_luid_high) = device_adapter_luid_parts(&device);
      let (y_texture, y_shared_handle, y_allocation_size) =
        create_shared_texture(&device, width, height, DXGI_FORMAT_R8_UNORM)?;
      let (uv_texture, uv_shared_handle, uv_allocation_size) =
        create_shared_texture(&device, width / 2, height / 2, DXGI_FORMAT_R8G8_UNORM)?;
      let native = crate::images::NativeImageData::from_dx12_nv12(width, height, y_texture.clone(), uv_texture.clone());
      Ok(Some(Dx12Nv12Surface {
        native,
        _y_texture: y_texture,
        _uv_texture: uv_texture,
        y_shared_handle,
        uv_shared_handle: Some(uv_shared_handle),
        y_allocation_size,
        uv_allocation_size,
        adapter_luid_low,
        adapter_luid_high,
        owns_shared_handles: true,
        packed_nv12: false,
      }))
    }
  }

  pub fn open_shared_nv12_surface(
    &self,
    width: u32,
    height: u32,
    shared_handle_raw: isize,
  ) -> Result<Option<Dx12Nv12Surface>> {
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 || shared_handle_raw == 0 {
      return Err(Error::from_win32());
    }
    let device = self
      .device
      .lock()
      .expect("dx12 video surface allocator lock poisoned")
      .clone();
    let Some(device) = device else {
      return Ok(None);
    };

    unsafe {
      let (adapter_luid_low, adapter_luid_high) = device_adapter_luid_parts(&device);
      let mut texture = None;
      device.OpenSharedHandle(HANDLE(shared_handle_raw as *mut c_void), &mut texture)?;
      let texture: ID3D12Resource = texture.ok_or_else(Error::from_win32)?;
      let desc = texture.GetDesc();
      if desc.Format != DXGI_FORMAT_NV12 || desc.Width != u64::from(width) || desc.Height != height {
        return Err(Error::from_win32());
      }
      let allocation_size = device
        .GetResourceAllocationInfo(0, std::slice::from_ref(&desc))
        .SizeInBytes;
      let native = crate::images::NativeImageData::from_dx12_nv12_texture(width, height, texture.clone());
      Ok(Some(Dx12Nv12Surface {
        native,
        _y_texture: texture.clone(),
        _uv_texture: texture,
        y_shared_handle: HANDLE(shared_handle_raw as *mut c_void),
        uv_shared_handle: None,
        y_allocation_size: allocation_size,
        uv_allocation_size: allocation_size,
        adapter_luid_low,
        adapter_luid_high,
        owns_shared_handles: false,
        packed_nv12: true,
      }))
    }
  }

  pub fn open_shared_nv12_planes_surface(
    &self,
    width: u32,
    height: u32,
    y_shared_handle_raw: isize,
    uv_shared_handle_raw: isize,
  ) -> Result<Option<Dx12Nv12Surface>> {
    if width == 0
      || height == 0
      || width % 2 != 0
      || height % 2 != 0
      || y_shared_handle_raw == 0
      || uv_shared_handle_raw == 0
    {
      return Err(Error::from_win32());
    }
    let device = self
      .device
      .lock()
      .expect("dx12 video surface allocator lock poisoned")
      .clone();
    let Some(device) = device else {
      return Ok(None);
    };

    unsafe {
      let (adapter_luid_low, adapter_luid_high) = device_adapter_luid_parts(&device);
      let mut y_texture = None;
      device.OpenSharedHandle(HANDLE(y_shared_handle_raw as *mut c_void), &mut y_texture)?;
      let y_texture: ID3D12Resource = y_texture.ok_or_else(Error::from_win32)?;
      let y_desc = y_texture.GetDesc();
      if y_desc.Format != DXGI_FORMAT_R8_UNORM || y_desc.Width != u64::from(width) || y_desc.Height != height {
        return Err(Error::from_win32());
      }

      let mut uv_texture = None;
      device.OpenSharedHandle(HANDLE(uv_shared_handle_raw as *mut c_void), &mut uv_texture)?;
      let uv_texture: ID3D12Resource = uv_texture.ok_or_else(Error::from_win32)?;
      let uv_desc = uv_texture.GetDesc();
      if uv_desc.Format != DXGI_FORMAT_R8G8_UNORM
        || uv_desc.Width != u64::from(width / 2)
        || uv_desc.Height != height / 2
      {
        return Err(Error::from_win32());
      }

      let y_allocation_size = device
        .GetResourceAllocationInfo(0, std::slice::from_ref(&y_desc))
        .SizeInBytes;
      let uv_allocation_size = device
        .GetResourceAllocationInfo(0, std::slice::from_ref(&uv_desc))
        .SizeInBytes;
      let native = crate::images::NativeImageData::from_dx12_nv12(width, height, y_texture.clone(), uv_texture.clone());
      Ok(Some(Dx12Nv12Surface {
        native,
        _y_texture: y_texture,
        _uv_texture: uv_texture,
        y_shared_handle: HANDLE(y_shared_handle_raw as *mut c_void),
        uv_shared_handle: Some(HANDLE(uv_shared_handle_raw as *mut c_void)),
        y_allocation_size,
        uv_allocation_size,
        adapter_luid_low,
        adapter_luid_high,
        owns_shared_handles: false,
        packed_nv12: false,
      }))
    }
  }

  fn set_device(&self, device: Option<ID3D12Device>) {
    *self.device.lock().expect("dx12 video surface allocator lock poisoned") = device;
  }
}

#[cfg(feature = "raster")]
impl Dx12Nv12Surface {
  pub fn image_data(&self) -> crate::images::ImageData {
    self.native.image_data()
  }

  pub fn native_image_data(&self) -> crate::images::NativeImageData {
    self.native.clone()
  }

  pub fn dx12_nv12_image(&self) -> crate::images::Dx12Nv12Image {
    crate::images::Dx12Nv12Image {
      y_texture: self._y_texture.clone(),
      uv_texture: self._uv_texture.clone(),
      y_plane_slice: 0,
      uv_plane_slice: if self.packed_nv12 { 1 } else { 0 },
      frame_number: None,
    }
  }

  pub fn y_shared_handle_raw(&self) -> isize {
    self.y_shared_handle.0 as isize
  }

  pub fn uv_shared_handle_raw(&self) -> isize {
    self.uv_shared_handle.unwrap_or(self.y_shared_handle).0 as isize
  }

  pub fn y_allocation_size(&self) -> u64 {
    self.y_allocation_size
  }

  pub fn uv_allocation_size(&self) -> u64 {
    self.uv_allocation_size
  }

  pub fn adapter_luid_low(&self) -> u32 {
    self.adapter_luid_low
  }

  pub fn adapter_luid_high(&self) -> i32 {
    self.adapter_luid_high
  }

  pub fn is_packed_nv12(&self) -> bool {
    self.packed_nv12
  }
}

#[cfg(feature = "raster")]
fn native_dx12_nv12_image(native: &crate::images::NativeImageData) -> Option<crate::images::Dx12Nv12Image> {
  native.payload::<crate::images::Dx12Nv12Image>().cloned().or_else(|| {
    native
      .payload::<crate::images::Dx12Nv12ImageSlot>()
      .map(crate::images::Dx12Nv12ImageSlot::image)
  })
}

#[cfg(feature = "raster")]
impl Drop for Dx12Nv12Surface {
  fn drop(&mut self) {
    if self.owns_shared_handles {
      unsafe {
        let _ = CloseHandle(self.y_shared_handle);
        if let Some(uv_shared_handle) = self.uv_shared_handle {
          let _ = CloseHandle(uv_shared_handle);
        }
      }
    }
  }
}

impl RenderEngine for Dx12RenderEngine {
  fn profile_backend(&self) -> &'static str {
    "dx12"
  }
  #[cfg(feature = "perf_profile")]
  fn set_profile_context(&mut self, context: crate::app::profiler::ProfileContext) {
    // Once per window: this renderer has no GPU timestamp queries yet.
    if !self
      .profile_context
      .as_ref()
      .is_some_and(|current| current.same_window(&context))
    {
      use crate::app::profiler::GpuTimestampAvailability;
      context.report_gpu_timestamps(GpuTimestampAvailability::unavailable(
        GpuTimestampAvailability::NATIVE_DX12,
      ));
    }
    self.profile_context = Some(context);
  }
  #[cfg(feature = "canvas")]
  fn prepare_canvases(&mut self, canvases: &[crate::canvas::CanvasHandle]) {
    self.canvases.clear();
    self
      .canvases
      .extend(canvases.iter().filter(|c| !c.status().software).cloned());
  }
  fn resize(&mut self, width: u32, height: u32) {
    self.width = width.max(1);
    self.height = height.max(1);
  }

  fn render(&mut self, list: &RenderList, window: WindowHandle<'_>, _display: DisplayHandle<'_>) -> bool {
    let _total_start = profile_scope!();
    let _init_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _init_phase = self
      .profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::RenderInit));
    if let Err(err) = self.ensure_initialized(window) {
      tracing::error!("failed to initialize native dx12 renderer: {err:?}");
      return false;
    }
    let _init_dur = profile_elapsed!(_init_start);
    #[cfg(feature = "perf_profile")]
    drop(_init_phase);

    let _profile_context = {
      #[cfg(feature = "perf_profile")]
      {
        self.profile_context.clone()
      }
      #[cfg(not(feature = "perf_profile"))]
      {
        None
      }
    };
    let state = self.state.as_mut().unwrap();
    if state.width != self.width || state.height != self.height {
      crate::app::profile_support::video_log!(debug,
        target: "video::watch::lurq",
        "dx12 swapchain resize old={}x{} new={}x{}",
        state.width,
        state.height,
        self.width,
        self.height
      );
      if let Err(err) = unsafe { state.resize(self.width, self.height) } {
        if is_dxgi_device_lost(&err) {
          let reason = unsafe { state.device_removed_reason_code() };
          tracing::warn!(
            "native dx12 device lost while resizing swapchain; resetting renderer: error={err:?} removed_reason={}",
            dxgi_hresult_label(reason)
          );
          state.mark_device_lost();
        } else {
          tracing::error!(
            target: "video::watch::lurq",
            "failed to resize native dx12 swapchain old={}x{} new={}x{} error={err:?}",
            state.width,
            state.height,
            self.width,
            self.height
          );
        }
        self.state = None;
        return false;
      }
    }
    #[cfg(feature = "screenshot")]
    {
      state.pending_frame_capture = self.pending_frame_capture.take();
    }
    #[cfg(feature = "canvas")]
    {
      state.canvas_sources.clear();
      state.canvas_sources.extend_from_slice(&self.canvases);
    }
    let _render_profile = match unsafe { state.render(list, _profile_context) } {
      Ok(profile) => profile,
      Err(err) => {
        if is_dx12_frame_not_ready(&err) {
          tracing::trace!("native dx12 frame skipped while waiting for previous GPU work: {err:?}");
          return false;
        } else if is_dxgi_device_lost(&err) {
          let reason = unsafe { state.device_removed_reason_code() };
          tracing::warn!(
            "native dx12 device lost while rendering frame; resetting renderer: error={err:?} removed_reason={}",
            dxgi_hresult_label(reason)
          );
          state.mark_device_lost();
        } else if is_error_mod_not_found(&err) {
          tracing::error!("failed to render native dx12 frame: {err:?}");
          tracing::error!(
            "dx12 render failed with ERROR_MOD_NOT_FOUND; check the GPU driver, DirectX runtime, d3dcompiler_47.dll, and optional graphics debug tools"
          );
        } else {
          tracing::error!("failed to render native dx12 frame: {err:?}");
        }
        #[cfg(feature = "raster")]
        {
          if let Some(video_surfaces) = &self.video_surfaces {
            video_surfaces.set_device(None);
          }
        }
        self.state = None;
        return false;
      }
    };
    let total_dur = profile_elapsed!(_total_start);
    log_slow_dx12_render_frame(self.width, self.height, _init_dur, &_render_profile, total_dur);
    self.record_render_cadence(list, total_dur, &_render_profile);

    profile_if! {
      self.last_profile = RenderProfile {
        init: _init_dur,
        total: total_dur,
        .._render_profile
      };
    }
    true
  }

  #[cfg(feature = "screenshot")]
  fn supports_frame_capture(&self) -> bool {
    true
  }

  #[cfg(feature = "screenshot")]
  fn render_with_capture(
    &mut self,
    list: &RenderList,
    window: WindowHandle<'_>,
    display: DisplayHandle<'_>,
    capture: Option<RenderFrameCapture>,
  ) -> bool {
    if let Some(previous) = std::mem::replace(&mut self.pending_frame_capture, capture) {
      previous.target.fail("superseded before a frame was rendered");
    }
    let rendered = self.render(list, window, display);
    if let Some(capture) = self.pending_frame_capture.take() {
      capture.target.fail("the frame was not rendered");
    }
    rendered
  }

  fn release_window_surface(&mut self) {
    #[cfg(feature = "raster")]
    {
      if let Some(video_surfaces) = &self.video_surfaces {
        video_surfaces.set_device(None);
      }
    }
    self.state = None;
  }

  #[cfg(feature = "perf_profile")]
  fn last_profile(&self) -> Option<RenderProfile> {
    Some(self.last_profile)
  }
}

struct Dx12State {
  #[cfg(feature = "canvas")]
  canvas_sources: Vec<crate::canvas::CanvasHandle>,
  #[cfg(feature = "canvas")]
  canvas_renderer: Option<canvas::Renderer>,
  #[cfg(feature = "canvas")]
  canvas_asset_budget: crate::canvas::CanvasAssetBudget,
  #[cfg(feature = "canvas")]
  canvas_retired: [Vec<ID3D12Resource>; FRAME_COUNT],
  device: ID3D12Device,
  command_queue: ID3D12CommandQueue,
  swapchain: IDXGISwapChain3,
  frame_latency_waitable: HANDLE,
  rtv_heap: CpuDescriptorHeap,
  srv_heap: CpuDescriptorHeap,
  sampler_heap: CpuDescriptorHeap,
  render_targets: [Option<ID3D12Resource>; FRAME_COUNT],
  quad_buffers: StaticQuadBuffers,
  rect_pipeline: RectPipeline,
  glyph_pipeline: GlyphPipeline,
  #[cfg(feature = "raster")]
  image_pipeline: ImagePipeline,
  #[cfg(feature = "raster")]
  nv12_image_pipeline: ImagePipeline,
  #[cfg(feature = "svg")]
  svg_pipeline: SvgPipeline,
  glyph_atlas: Option<GlyphAtlasTexture>,
  #[cfg(feature = "raster")]
  image_textures: HashMap<u64, CachedImageTexture>,
  #[cfg(feature = "raster")]
  native_image_draw_cadence: HashMap<u64, NativeImageDrawCadence>,
  #[cfg(feature = "raster")]
  next_srv_index: usize,
  frame_arenas: [UploadArena; FRAME_COUNT],
  frame_uploads: [Vec<UploadBuffer>; FRAME_COUNT],
  command_allocators: [ID3D12CommandAllocator; FRAME_COUNT],
  command_list: ID3D12GraphicsCommandList,
  fence: ID3D12Fence,
  fence_event: HANDLE,
  fence_values: [u64; FRAME_COUNT],
  next_fence_value: u64,
  frame_index: usize,
  width: u32,
  height: u32,
  device_lost: bool,
  #[cfg(feature = "screenshot")]
  pending_frame_capture: Option<RenderFrameCapture>,
  /// Readback buffers whose fence wait failed; the GPU may still be writing
  /// them, so they are kept alive until their frame slot's fence is waited on
  /// again at the start of a later frame.
  #[cfg(feature = "screenshot")]
  retired_capture_readbacks: [Vec<ID3D12Resource>; FRAME_COUNT],
  /// The pixel space draws currently target: the window or an opacity layer.
  space: TargetSpace,
  layer_plan: LayerPlan,
  layer_slots: Vec<Option<usize>>,
  /// Offscreen targets of faded subtrees; created by the first frame that
  /// has one.
  layer_compositor: Option<layers::LayerCompositor>,
  /// Draw calls `encode_frame` recorded for the render list, layer
  /// composites included; Canvas draws are not counted.
  #[cfg(feature = "perf_profile")]
  draw_calls: usize,
}

/// A recorded-but-not-yet-read screenshot: the readback buffer the back
/// buffer was copied into, plus the layout needed to unpack it after the
/// frame's fence signals.
#[cfg(feature = "screenshot")]
struct FrameCaptureReadback {
  resource: ID3D12Resource,
  padded_bytes_per_row: u32,
  frame_index: usize,
}

#[cfg(feature = "raster")]
struct NativeImageDrawCadence {
  started_at: Instant,
  draws: u32,
  version_changes: u32,
  repeated_draws: u32,
  skipped_versions: u64,
  last_version: Option<u64>,
  stream_frame_changes: u32,
  repeated_stream_frames: u32,
  backward_stream_frames: u32,
  skipped_stream_frames: u64,
  max_stream_frame_delta: u32,
  last_stream_frame: Option<u32>,
  last_draw_at: Option<Instant>,
  max_draw_delta: Duration,
  max_version_delta: u64,
  last_width: u32,
  last_height: u32,
}

#[cfg(feature = "raster")]
fn stream_frame_after(frame_number: u32, previous_frame_number: u32) -> bool {
  let delta = frame_number.wrapping_sub(previous_frame_number);
  delta != 0 && delta < (u32::MAX / 2)
}

struct StaticQuadBuffers {
  _vertex_buffer: UploadBuffer,
  _index_buffer: UploadBuffer,
  vertex_view: D3D12_VERTEX_BUFFER_VIEW,
  index_view: D3D12_INDEX_BUFFER_VIEW,
}

struct RectPipeline {
  root_signature: ID3D12RootSignature,
  pipeline_state: ID3D12PipelineState,
}

struct GlyphPipeline {
  root_signature: ID3D12RootSignature,
  pipeline_state: ID3D12PipelineState,
}

#[cfg(feature = "raster")]
struct ImagePipeline {
  root_signature: ID3D12RootSignature,
  pipeline_state: ID3D12PipelineState,
}

#[cfg(feature = "svg")]
struct SvgPipeline {
  root_signature: ID3D12RootSignature,
  pipeline_state: ID3D12PipelineState,
}

struct GlyphAtlasTexture {
  texture: ID3D12Resource,
  width: u32,
  height: u32,
  version: u64,
  state: D3D12_RESOURCE_STATES,
}

#[derive(Clone, Copy, Default)]
struct GlyphAtlasUploadStats {
  bytes: usize,
  rects: usize,
  full_uploads: usize,
  arena_uploads: usize,
  dedicated_uploads: usize,
}

#[cfg(feature = "raster")]
enum CachedImageTexture {
  Rgba {
    _texture: ID3D12Resource,
    descriptor_index: Option<usize>,
    width: u32,
    height: u32,
    frame_index: usize,
    version: u64,
  },
  Nv12 {
    _y_texture: ID3D12Resource,
    _uv_texture: ID3D12Resource,
    descriptor_index: Option<usize>,
    width: u32,
    height: u32,
    frame_index: usize,
    version: u64,
  },
  NativeNv12 {
    _y_texture: ID3D12Resource,
    _uv_texture: ID3D12Resource,
    y_keyed_mutex: Option<IDXGIKeyedMutex>,
    uv_keyed_mutex: Option<IDXGIKeyedMutex>,
    descriptor_indices: [Option<usize>; FRAME_COUNT],
    descriptor_versions: [u64; FRAME_COUNT],
    descriptor_stream_frames: [Option<u32>; FRAME_COUNT],
    width: u32,
    height: u32,
    version: u64,
  },
}

#[cfg(feature = "raster")]
impl CachedImageTexture {
  fn clear_descriptor_index(&mut self) {
    match self {
      Self::Rgba { descriptor_index, .. } | Self::Nv12 { descriptor_index, .. } => {
        *descriptor_index = None;
      }
      Self::NativeNv12 { .. } => {}
    }
  }

  fn set_descriptor_index(&mut self, index: usize) {
    match self {
      Self::Rgba { descriptor_index, .. } | Self::Nv12 { descriptor_index, .. } => {
        *descriptor_index = Some(index);
      }
      Self::NativeNv12 { .. } => {}
    }
  }

  fn set_native_descriptor_index(&mut self, frame_index: usize, index: usize, version: u64, stream_frame: Option<u32>) {
    if let Self::NativeNv12 {
      descriptor_indices,
      descriptor_versions,
      descriptor_stream_frames,
      ..
    } = self
    {
      descriptor_indices[frame_index] = Some(index);
      descriptor_versions[frame_index] = version;
      descriptor_stream_frames[frame_index] = stream_frame;
    }
  }
}

struct CpuDescriptorHeap {
  heap: ID3D12DescriptorHeap,
  descriptor_size: u32,
}

impl CpuDescriptorHeap {
  unsafe fn new(
    device: &ID3D12Device,
    heap_type: D3D12_DESCRIPTOR_HEAP_TYPE,
    num_descriptors: u32,
    flags: D3D12_DESCRIPTOR_HEAP_FLAGS,
  ) -> Result<Self> {
    let desc = D3D12_DESCRIPTOR_HEAP_DESC {
      Type: heap_type,
      NumDescriptors: num_descriptors,
      Flags: flags,
      NodeMask: 0,
    };
    let heap = device.CreateDescriptorHeap(&desc)?;
    let descriptor_size = device.GetDescriptorHandleIncrementSize(heap_type);
    Ok(Self { heap, descriptor_size })
  }

  unsafe fn cpu_handle(&self, index: usize) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    offset_cpu_handle(
      self.heap.GetCPUDescriptorHandleForHeapStart(),
      self.descriptor_size,
      index,
    )
  }

  unsafe fn gpu_handle(&self, index: usize) -> D3D12_GPU_DESCRIPTOR_HANDLE {
    offset_gpu_handle(
      self.heap.GetGPUDescriptorHandleForHeapStart(),
      self.descriptor_size,
      index,
    )
  }
}

impl StaticQuadBuffers {
  unsafe fn new(device: &ID3D12Device) -> Result<Self> {
    let vertex_bytes = bytemuck::cast_slice(&QuadVertex::CORNERS);
    let index_bytes = bytemuck::cast_slice(&QuadVertex::INDICES);
    let vertex_buffer = UploadBuffer::from_bytes(device, vertex_bytes)?;
    let index_buffer = UploadBuffer::from_bytes(device, index_bytes)?;
    let vertex_view = D3D12_VERTEX_BUFFER_VIEW {
      BufferLocation: vertex_buffer.gpu_address,
      SizeInBytes: vertex_buffer.size_in_bytes,
      StrideInBytes: std::mem::size_of::<QuadVertex>() as u32,
    };
    let index_view = D3D12_INDEX_BUFFER_VIEW {
      BufferLocation: index_buffer.gpu_address,
      SizeInBytes: index_buffer.size_in_bytes,
      Format: DXGI_FORMAT_R16_UINT,
    };
    Ok(Self {
      _vertex_buffer: vertex_buffer,
      _index_buffer: index_buffer,
      vertex_view,
      index_view,
    })
  }
}

struct UploadBuffer {
  _resource: ID3D12Resource,
  gpu_address: u64,
  size_in_bytes: u32,
}

#[derive(Clone)]
#[allow(dead_code)]
struct UploadSlice {
  resource: ID3D12Resource,
  offset: u64,
  gpu_address: u64,
  size_in_bytes: u32,
}

struct UploadArena {
  _resource: ID3D12Resource,
  gpu_address: u64,
  mapped: *mut u8,
  capacity: usize,
  offset: usize,
}

impl UploadBuffer {
  unsafe fn from_bytes(device: &ID3D12Device, data: &[u8]) -> Result<Self> {
    Self::from_bytes_padded(device, data, data.len().max(1))
  }

  unsafe fn from_bytes_padded(device: &ID3D12Device, data: &[u8], padded_size: usize) -> Result<Self> {
    let padded_size = padded_size.max(data.len()).max(1);
    let heap_properties = D3D12_HEAP_PROPERTIES {
      Type: D3D12_HEAP_TYPE_UPLOAD,
      CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
      MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
      CreationNodeMask: 1,
      VisibleNodeMask: 1,
    };
    let desc = buffer_resource_desc(padded_size as u64);
    let mut resource = None;
    device.CreateCommittedResource(
      &heap_properties,
      D3D12_HEAP_FLAG_NONE,
      &desc,
      D3D12_RESOURCE_STATE_GENERIC_READ,
      None,
      &mut resource,
    )?;
    let resource: ID3D12Resource = resource.ok_or_else(Error::from_win32)?;

    let read_range = D3D12_RANGE { Begin: 0, End: 0 };
    let mut mapped: *mut c_void = ptr::null_mut();
    resource.Map(0, Some(&read_range), Some(&mut mapped))?;
    if !data.is_empty() {
      ptr::copy_nonoverlapping(data.as_ptr(), mapped.cast::<u8>(), data.len());
    }
    if padded_size > data.len() {
      ptr::write_bytes(mapped.cast::<u8>().add(data.len()), 0, padded_size - data.len());
    }
    let written_range = D3D12_RANGE {
      Begin: 0,
      End: padded_size,
    };
    resource.Unmap(0, Some(&written_range));

    Ok(Self {
      gpu_address: resource.GetGPUVirtualAddress(),
      size_in_bytes: padded_size as u32,
      _resource: resource,
    })
  }
}

impl UploadSlice {
  fn vertex_view<T>(&self) -> D3D12_VERTEX_BUFFER_VIEW {
    D3D12_VERTEX_BUFFER_VIEW {
      BufferLocation: self.gpu_address,
      SizeInBytes: self.size_in_bytes,
      StrideInBytes: std::mem::size_of::<T>() as u32,
    }
  }
}

impl UploadArena {
  unsafe fn new(device: &ID3D12Device, capacity: usize) -> Result<Self> {
    let heap_properties = D3D12_HEAP_PROPERTIES {
      Type: D3D12_HEAP_TYPE_UPLOAD,
      CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
      MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
      CreationNodeMask: 1,
      VisibleNodeMask: 1,
    };
    let desc = buffer_resource_desc(capacity.max(1) as u64);
    let mut resource = None;
    device.CreateCommittedResource(
      &heap_properties,
      D3D12_HEAP_FLAG_NONE,
      &desc,
      D3D12_RESOURCE_STATE_GENERIC_READ,
      None,
      &mut resource,
    )?;
    let resource: ID3D12Resource = resource.ok_or_else(Error::from_win32)?;
    let read_range = D3D12_RANGE { Begin: 0, End: 0 };
    let mut mapped: *mut c_void = ptr::null_mut();
    resource.Map(0, Some(&read_range), Some(&mut mapped))?;
    Ok(Self {
      gpu_address: resource.GetGPUVirtualAddress(),
      mapped: mapped.cast(),
      capacity: capacity.max(1),
      offset: 0,
      _resource: resource,
    })
  }

  fn reset(&mut self) {
    self.offset = 0;
  }

  unsafe fn upload_bytes(&mut self, data: &[u8], alignment: usize) -> Option<UploadSlice> {
    let alignment = alignment.max(1);
    let start = align_up(self.offset, alignment);
    let padded_size = align_up(data.len().max(1), alignment);
    let end = start.checked_add(padded_size)?;
    if end > self.capacity {
      return None;
    }

    ptr::copy_nonoverlapping(data.as_ptr(), self.mapped.add(start), data.len());
    self.offset = end;
    Some(UploadSlice {
      resource: self._resource.clone(),
      offset: start as u64,
      gpu_address: self.gpu_address + start as u64,
      size_in_bytes: padded_size as u32,
    })
  }

  #[allow(dead_code)]
  unsafe fn upload_rows(
    &mut self,
    data: &[u8],
    row_bytes: usize,
    row_pitch: usize,
    height: usize,
    alignment: usize,
  ) -> Option<UploadSlice> {
    let alignment = alignment.max(1);
    let start = align_up(self.offset, alignment);
    let upload_size = row_pitch.checked_mul(height)?;
    let padded_size = align_up(upload_size.max(1), alignment);
    let end = start.checked_add(padded_size)?;
    if end > self.capacity {
      return None;
    }

    copy_upload_rows(data, self.mapped.add(start), row_bytes, row_pitch, height);
    self.offset = end;
    Some(UploadSlice {
      resource: self._resource.clone(),
      offset: start as u64,
      gpu_address: self.gpu_address + start as u64,
      size_in_bytes: padded_size as u32,
    })
  }
}

// `destination` must hold row_pitch * height writable bytes, disjoint from data.
// Reused upload memory must not expose old texels when the source ends mid-row.
unsafe fn copy_upload_rows(data: &[u8], destination: *mut u8, row_bytes: usize, row_pitch: usize, height: usize) {
  assert!(row_pitch >= row_bytes);
  for row in 0..height {
    let src_start = row * row_bytes;
    let copied = data.len().saturating_sub(src_start).min(row_bytes);
    let dst = destination.add(row * row_pitch);
    if copied > 0 {
      ptr::copy_nonoverlapping(data.as_ptr().add(src_start), dst, copied);
    }
    ptr::write_bytes(dst.add(copied), 0, row_pitch - copied);
  }
}

impl Drop for UploadArena {
  fn drop(&mut self) {
    unsafe {
      let written_range = D3D12_RANGE {
        Begin: 0,
        End: self.offset,
      };
      self._resource.Unmap(0, Some(&written_range));
    }
  }
}

impl RectPipeline {
  unsafe fn new(device: &ID3D12Device, format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT) -> Result<Self> {
    let root_signature = create_rect_root_signature(device)?;
    let vs = compile_shader(include_bytes!("shaders/quad.hlsl"), b"vs_main\0", b"vs_5_0\0")?;
    let ps = compile_shader(include_bytes!("shaders/quad.hlsl"), b"ps_main\0", b"ps_5_0\0")?;
    let input_elements = rect_input_elements();
    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    rtv_formats[0] = format;

    let blend = D3D12_RENDER_TARGET_BLEND_DESC {
      BlendEnable: TRUE,
      LogicOpEnable: FALSE,
      SrcBlend: D3D12_BLEND_SRC_ALPHA,
      DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOp: D3D12_BLEND_OP_ADD,
      SrcBlendAlpha: D3D12_BLEND_ONE,
      DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOpAlpha: D3D12_BLEND_OP_ADD,
      LogicOp: D3D12_LOGIC_OP_NOOP,
      RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
      pRootSignature: ManuallyDrop::new(Some(root_signature.clone())),
      VS: shader_bytecode(&vs),
      PS: shader_bytecode(&ps),
      DS: D3D12_SHADER_BYTECODE::default(),
      HS: D3D12_SHADER_BYTECODE::default(),
      GS: D3D12_SHADER_BYTECODE::default(),
      StreamOutput: Default::default(),
      BlendState: D3D12_BLEND_DESC {
        AlphaToCoverageEnable: FALSE,
        IndependentBlendEnable: FALSE,
        RenderTarget: [blend; 8],
      },
      SampleMask: u32::MAX,
      RasterizerState: D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: D3D12_CULL_MODE_NONE,
        FrontCounterClockwise: FALSE,
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: TRUE,
        MultisampleEnable: FALSE,
        AntialiasedLineEnable: FALSE,
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
      },
      DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
        DepthEnable: FALSE,
        DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        StencilEnable: FALSE,
        StencilReadMask: 0,
        StencilWriteMask: 0,
        FrontFace: Default::default(),
        BackFace: Default::default(),
      },
      InputLayout: D3D12_INPUT_LAYOUT_DESC {
        pInputElementDescs: input_elements.as_ptr(),
        NumElements: input_elements.len() as u32,
      },
      IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
      PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
      NumRenderTargets: 1,
      RTVFormats: rtv_formats,
      DSVFormat: DXGI_FORMAT_UNKNOWN,
      SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
      NodeMask: 0,
      CachedPSO: Default::default(),
      Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
    };

    let pipeline_state = device.CreateGraphicsPipelineState(&desc)?;
    ManuallyDrop::drop(&mut desc.pRootSignature);

    Ok(Self {
      root_signature,
      pipeline_state,
    })
  }
}

impl GlyphPipeline {
  unsafe fn new(device: &ID3D12Device, format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT) -> Result<Self> {
    let root_signature = create_glyph_root_signature(device)?;
    let vs = compile_shader(include_bytes!("shaders/glyph.hlsl"), b"vs_main\0", b"vs_5_0\0")?;
    let ps = compile_shader(include_bytes!("shaders/glyph.hlsl"), b"ps_main\0", b"ps_5_0\0")?;
    let input_elements = glyph_input_elements();
    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    rtv_formats[0] = format;

    let blend = D3D12_RENDER_TARGET_BLEND_DESC {
      BlendEnable: TRUE,
      LogicOpEnable: FALSE,
      SrcBlend: D3D12_BLEND_SRC_ALPHA,
      DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOp: D3D12_BLEND_OP_ADD,
      SrcBlendAlpha: D3D12_BLEND_ONE,
      DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOpAlpha: D3D12_BLEND_OP_ADD,
      LogicOp: D3D12_LOGIC_OP_NOOP,
      RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
      pRootSignature: ManuallyDrop::new(Some(root_signature.clone())),
      VS: shader_bytecode(&vs),
      PS: shader_bytecode(&ps),
      DS: D3D12_SHADER_BYTECODE::default(),
      HS: D3D12_SHADER_BYTECODE::default(),
      GS: D3D12_SHADER_BYTECODE::default(),
      StreamOutput: Default::default(),
      BlendState: D3D12_BLEND_DESC {
        AlphaToCoverageEnable: FALSE,
        IndependentBlendEnable: FALSE,
        RenderTarget: [blend; 8],
      },
      SampleMask: u32::MAX,
      RasterizerState: D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: D3D12_CULL_MODE_NONE,
        FrontCounterClockwise: FALSE,
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: TRUE,
        MultisampleEnable: FALSE,
        AntialiasedLineEnable: FALSE,
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
      },
      DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
        DepthEnable: FALSE,
        DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        StencilEnable: FALSE,
        StencilReadMask: 0,
        StencilWriteMask: 0,
        FrontFace: Default::default(),
        BackFace: Default::default(),
      },
      InputLayout: D3D12_INPUT_LAYOUT_DESC {
        pInputElementDescs: input_elements.as_ptr(),
        NumElements: input_elements.len() as u32,
      },
      IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
      PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
      NumRenderTargets: 1,
      RTVFormats: rtv_formats,
      DSVFormat: DXGI_FORMAT_UNKNOWN,
      SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
      NodeMask: 0,
      CachedPSO: Default::default(),
      Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
    };

    let pipeline_state = device.CreateGraphicsPipelineState(&desc)?;
    ManuallyDrop::drop(&mut desc.pRootSignature);

    Ok(Self {
      root_signature,
      pipeline_state,
    })
  }
}

#[cfg(feature = "raster")]
impl ImagePipeline {
  unsafe fn new(device: &ID3D12Device, format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT) -> Result<Self> {
    Self::new_with_shader(device, format, include_bytes!("shaders/image.hlsl"), 1)
  }

  unsafe fn new_nv12(
    device: &ID3D12Device,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
  ) -> Result<Self> {
    Self::new_with_shader(device, format, include_bytes!("shaders/image_nv12.hlsl"), 2)
  }

  unsafe fn new_with_shader(
    device: &ID3D12Device,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    shader: &'static [u8],
    srv_descriptors: u32,
  ) -> Result<Self> {
    let root_signature = create_image_root_signature(device, srv_descriptors)?;
    let vs = compile_shader(shader, b"vs_main\0", b"vs_5_0\0")?;
    let ps = compile_shader(shader, b"ps_main\0", b"ps_5_0\0")?;
    let input_elements = image_input_elements();
    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    rtv_formats[0] = format;

    let blend = D3D12_RENDER_TARGET_BLEND_DESC {
      BlendEnable: TRUE,
      LogicOpEnable: FALSE,
      SrcBlend: D3D12_BLEND_SRC_ALPHA,
      DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOp: D3D12_BLEND_OP_ADD,
      SrcBlendAlpha: D3D12_BLEND_ONE,
      DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOpAlpha: D3D12_BLEND_OP_ADD,
      LogicOp: D3D12_LOGIC_OP_NOOP,
      RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
      pRootSignature: ManuallyDrop::new(Some(root_signature.clone())),
      VS: shader_bytecode(&vs),
      PS: shader_bytecode(&ps),
      DS: D3D12_SHADER_BYTECODE::default(),
      HS: D3D12_SHADER_BYTECODE::default(),
      GS: D3D12_SHADER_BYTECODE::default(),
      StreamOutput: Default::default(),
      BlendState: D3D12_BLEND_DESC {
        AlphaToCoverageEnable: FALSE,
        IndependentBlendEnable: FALSE,
        RenderTarget: [blend; 8],
      },
      SampleMask: u32::MAX,
      RasterizerState: D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: D3D12_CULL_MODE_NONE,
        FrontCounterClockwise: FALSE,
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: TRUE,
        MultisampleEnable: FALSE,
        AntialiasedLineEnable: FALSE,
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
      },
      DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
        DepthEnable: FALSE,
        DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        StencilEnable: FALSE,
        StencilReadMask: 0,
        StencilWriteMask: 0,
        FrontFace: Default::default(),
        BackFace: Default::default(),
      },
      InputLayout: D3D12_INPUT_LAYOUT_DESC {
        pInputElementDescs: input_elements.as_ptr(),
        NumElements: input_elements.len() as u32,
      },
      IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
      PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
      NumRenderTargets: 1,
      RTVFormats: rtv_formats,
      DSVFormat: DXGI_FORMAT_UNKNOWN,
      SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
      NodeMask: 0,
      CachedPSO: Default::default(),
      Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
    };

    let pipeline_state = device.CreateGraphicsPipelineState(&desc)?;
    ManuallyDrop::drop(&mut desc.pRootSignature);

    Ok(Self {
      root_signature,
      pipeline_state,
    })
  }
}

#[cfg(feature = "svg")]
impl SvgPipeline {
  unsafe fn new(device: &ID3D12Device, format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT) -> Result<Self> {
    let root_signature = create_rect_root_signature(device)?;
    let vs = compile_shader(include_bytes!("shaders/svg.hlsl"), b"vs_main\0", b"vs_5_0\0")?;
    let ps = compile_shader(include_bytes!("shaders/svg.hlsl"), b"ps_main\0", b"ps_5_0\0")?;
    let input_elements = svg_input_elements();
    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    rtv_formats[0] = format;

    let blend = D3D12_RENDER_TARGET_BLEND_DESC {
      BlendEnable: TRUE,
      LogicOpEnable: FALSE,
      SrcBlend: D3D12_BLEND_SRC_ALPHA,
      DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOp: D3D12_BLEND_OP_ADD,
      SrcBlendAlpha: D3D12_BLEND_ONE,
      DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
      BlendOpAlpha: D3D12_BLEND_OP_ADD,
      LogicOp: D3D12_LOGIC_OP_NOOP,
      RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
      pRootSignature: ManuallyDrop::new(Some(root_signature.clone())),
      VS: shader_bytecode(&vs),
      PS: shader_bytecode(&ps),
      DS: D3D12_SHADER_BYTECODE::default(),
      HS: D3D12_SHADER_BYTECODE::default(),
      GS: D3D12_SHADER_BYTECODE::default(),
      StreamOutput: Default::default(),
      BlendState: D3D12_BLEND_DESC {
        AlphaToCoverageEnable: FALSE,
        IndependentBlendEnable: FALSE,
        RenderTarget: [blend; 8],
      },
      SampleMask: u32::MAX,
      RasterizerState: D3D12_RASTERIZER_DESC {
        FillMode: D3D12_FILL_MODE_SOLID,
        CullMode: D3D12_CULL_MODE_NONE,
        FrontCounterClockwise: FALSE,
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: TRUE,
        MultisampleEnable: FALSE,
        AntialiasedLineEnable: FALSE,
        ForcedSampleCount: 0,
        ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
      },
      DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
        DepthEnable: FALSE,
        DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
        StencilEnable: FALSE,
        StencilReadMask: 0,
        StencilWriteMask: 0,
        FrontFace: Default::default(),
        BackFace: Default::default(),
      },
      InputLayout: D3D12_INPUT_LAYOUT_DESC {
        pInputElementDescs: input_elements.as_ptr(),
        NumElements: input_elements.len() as u32,
      },
      IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
      PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
      NumRenderTargets: 1,
      RTVFormats: rtv_formats,
      DSVFormat: DXGI_FORMAT_UNKNOWN,
      SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
      NodeMask: 0,
      CachedPSO: Default::default(),
      Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
    };

    let pipeline_state = device.CreateGraphicsPipelineState(&desc)?;
    ManuallyDrop::drop(&mut desc.pRootSignature);

    Ok(Self {
      root_signature,
      pipeline_state,
    })
  }
}

fn render_target_view_desc() -> D3D12_RENDER_TARGET_VIEW_DESC {
  D3D12_RENDER_TARGET_VIEW_DESC {
    Format: RENDER_TARGET_FORMAT,
    ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2D,
    Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
      Texture2D: D3D12_TEX2D_RTV {
        MipSlice: 0,
        PlaneSlice: 0,
      },
    },
  }
}

fn buffer_resource_desc(size: u64) -> D3D12_RESOURCE_DESC {
  D3D12_RESOURCE_DESC {
    Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
    Alignment: 0,
    Width: size,
    Height: 1,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: DXGI_FORMAT_UNKNOWN,
    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
    Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
    Flags: D3D12_RESOURCE_FLAG_NONE,
  }
}

unsafe fn create_rect_root_signature(device: &ID3D12Device) -> Result<ID3D12RootSignature> {
  let root_parameters = [
    // b0: per-frame globals.
    D3D12_ROOT_PARAMETER {
      ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
      Anonymous: D3D12_ROOT_PARAMETER_0 {
        Descriptor: D3D12_ROOT_DESCRIPTOR {
          ShaderRegister: 0,
          RegisterSpace: 0,
        },
      },
      ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
    // t0: gradient stop storage (StructuredBuffer<float4>) as a root SRV.
    D3D12_ROOT_PARAMETER {
      ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
      Anonymous: D3D12_ROOT_PARAMETER_0 {
        Descriptor: D3D12_ROOT_DESCRIPTOR {
          ShaderRegister: 0,
          RegisterSpace: 0,
        },
      },
      ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
  ];
  let desc = D3D12_ROOT_SIGNATURE_DESC {
    NumParameters: root_parameters.len() as u32,
    pParameters: root_parameters.as_ptr(),
    NumStaticSamplers: 0,
    pStaticSamplers: ptr::null(),
    Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
  };

  let mut blob = None;
  let mut errors = None;
  D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).map_err(|err| {
    Error::new(
      err.code(),
      format!(
        "failed to serialize dx12 rect root signature{}",
        blob_message(errors.as_ref())
      ),
    )
  })?;
  let blob = blob.ok_or_else(Error::from_win32)?;
  device.CreateRootSignature(0, blob_bytes(&blob))
}

unsafe fn create_glyph_root_signature(device: &ID3D12Device) -> Result<ID3D12RootSignature> {
  create_image_root_signature(device, 1)
}

unsafe fn create_image_root_signature(device: &ID3D12Device, srv_descriptors: u32) -> Result<ID3D12RootSignature> {
  let srv_range = D3D12_DESCRIPTOR_RANGE {
    RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
    NumDescriptors: srv_descriptors,
    BaseShaderRegister: 0,
    RegisterSpace: 0,
    OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
  };
  let sampler_range = D3D12_DESCRIPTOR_RANGE {
    RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,
    NumDescriptors: 1,
    BaseShaderRegister: 0,
    RegisterSpace: 0,
    OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
  };
  let root_parameters = [
    D3D12_ROOT_PARAMETER {
      ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
      Anonymous: D3D12_ROOT_PARAMETER_0 {
        Descriptor: D3D12_ROOT_DESCRIPTOR {
          ShaderRegister: 0,
          RegisterSpace: 0,
        },
      },
      ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
    D3D12_ROOT_PARAMETER {
      ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
      Anonymous: D3D12_ROOT_PARAMETER_0 {
        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
          NumDescriptorRanges: 1,
          pDescriptorRanges: &srv_range,
        },
      },
      ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
    D3D12_ROOT_PARAMETER {
      ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
      Anonymous: D3D12_ROOT_PARAMETER_0 {
        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
          NumDescriptorRanges: 1,
          pDescriptorRanges: &sampler_range,
        },
      },
      ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
  ];
  let desc = D3D12_ROOT_SIGNATURE_DESC {
    NumParameters: root_parameters.len() as u32,
    pParameters: root_parameters.as_ptr(),
    NumStaticSamplers: 0,
    pStaticSamplers: ptr::null(),
    Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
  };

  let mut blob = None;
  let mut errors = None;
  D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).map_err(|err| {
    Error::new(
      err.code(),
      format!(
        "failed to serialize dx12 image root signature{}",
        blob_message(errors.as_ref())
      ),
    )
  })?;
  let blob = blob.ok_or_else(Error::from_win32)?;
  device.CreateRootSignature(0, blob_bytes(&blob))
}

unsafe fn compile_shader(source: &[u8], entry: &'static [u8], target: &'static [u8]) -> Result<ID3DBlob> {
  let flags = if cfg!(debug_assertions) {
    D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION
  } else {
    D3DCOMPILE_ENABLE_STRICTNESS
  };
  let mut blob = None;
  let mut errors = None;
  D3DCompile(
    source.as_ptr().cast(),
    source.len(),
    PCSTR(b"quad.hlsl\0".as_ptr()),
    None,
    None,
    PCSTR(entry.as_ptr()),
    PCSTR(target.as_ptr()),
    flags,
    0,
    &mut blob,
    Some(&mut errors),
  )
  .map_err(|err| {
    Error::new(
      err.code(),
      format!("failed to compile dx12 rect shader{}", blob_message(errors.as_ref())),
    )
  })?;
  blob.ok_or_else(Error::from_win32)
}

fn blob_message(blob: Option<&ID3DBlob>) -> String {
  let Some(blob) = blob else {
    return String::new();
  };
  unsafe {
    let bytes = std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize());
    format!(": {}", String::from_utf8_lossy(bytes).trim_end_matches('\0'))
  }
}

unsafe fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
  std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
}

unsafe fn shader_bytecode(blob: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
  D3D12_SHADER_BYTECODE {
    pShaderBytecode: blob.GetBufferPointer(),
    BytecodeLength: blob.GetBufferSize(),
  }
}

fn rect_input_elements() -> [D3D12_INPUT_ELEMENT_DESC; 12] {
  [
    input_element(
      0,
      DXGI_FORMAT_R32G32_FLOAT,
      0,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
      0,
    ),
    input_element(
      1,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      2,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      8,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      3,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      16,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      4,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      32,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      5,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      48,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      6,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      64,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      7,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      80,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      8,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      96,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      9,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      112,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      10,
      DXGI_FORMAT_R32_FLOAT,
      1,
      120,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      11,
      DXGI_FORMAT_R32_FLOAT,
      1,
      124,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
  ]
}

fn glyph_input_elements() -> [D3D12_INPUT_ELEMENT_DESC; 11] {
  [
    input_element(
      0,
      DXGI_FORMAT_R32G32_FLOAT,
      0,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
      0,
    ),
    input_element(
      1,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      2,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      8,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      3,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      16,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      4,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      32,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      5,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      40,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      6,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      48,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      7,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      64,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      8,
      DXGI_FORMAT_R32_FLOAT,
      1,
      72,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      9,
      DXGI_FORMAT_R32_FLOAT,
      1,
      76,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      10,
      DXGI_FORMAT_R32_FLOAT,
      1,
      80,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
  ]
}

#[cfg(feature = "raster")]
fn image_input_elements() -> [D3D12_INPUT_ELEMENT_DESC; 9] {
  [
    input_element(
      0,
      DXGI_FORMAT_R32G32_FLOAT,
      0,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
      0,
    ),
    input_element(
      1,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      2,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      8,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      3,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      16,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      4,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      32,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      5,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      48,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      6,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      56,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      7,
      DXGI_FORMAT_R32G32_FLOAT,
      1,
      64,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
    input_element(
      8,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      1,
      72,
      D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
      1,
    ),
  ]
}

#[cfg(feature = "svg")]
fn svg_input_elements() -> [D3D12_INPUT_ELEMENT_DESC; 2] {
  [
    input_element(
      0,
      DXGI_FORMAT_R32G32_FLOAT,
      0,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
      0,
    ),
    input_element(
      1,
      DXGI_FORMAT_R32G32B32A32_FLOAT,
      0,
      8,
      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
      0,
    ),
  ]
}

fn input_element(
  semantic_index: u32,
  format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
  slot: u32,
  offset: u32,
  slot_class: windows::Win32::Graphics::Direct3D12::D3D12_INPUT_CLASSIFICATION,
  step_rate: u32,
) -> D3D12_INPUT_ELEMENT_DESC {
  D3D12_INPUT_ELEMENT_DESC {
    SemanticName: PCSTR(b"TEXCOORD\0".as_ptr()),
    SemanticIndex: semantic_index,
    Format: format,
    InputSlot: slot,
    AlignedByteOffset: offset,
    InputSlotClass: slot_class,
    InstanceDataStepRate: step_rate,
  }
}

fn glyph_instance(glyph: &GlyphCmd, atlas_inverse: [f32; 2]) -> GlyphInstance {
  GlyphInstance {
    pos: [glyph.x, glyph.y],
    size: [glyph.width, glyph.height],
    color: glyph.color,
    uv_min: [
      glyph.atlas_min[0] * atlas_inverse[0],
      glyph.atlas_min[1] * atlas_inverse[1],
    ],
    uv_max: [
      glyph.atlas_max[0] * atlas_inverse[0],
      glyph.atlas_max[1] * atlas_inverse[1],
    ],
    transform: glyph.transform,
    xf_origin: glyph.transform_origin,
    sharpness: glyph.sharpness,
    color_glyph: if glyph.color_glyph { 1.0 } else { 0.0 },
    shadow_sigma: glyph.shadow_sigma,
  }
}

unsafe fn create_linear_clamp_sampler(device: &ID3D12Device, handle: D3D12_CPU_DESCRIPTOR_HANDLE) {
  let desc = D3D12_SAMPLER_DESC {
    Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
    AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    MipLODBias: 0.0,
    MaxAnisotropy: 1,
    ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
    BorderColor: [0.0; 4],
    MinLOD: 0.0,
    MaxLOD: f32::MAX,
  };
  device.CreateSampler(&desc, handle);
}

#[allow(dead_code)]
unsafe fn create_r8_texture(device: &ID3D12Device, width: u32, height: u32) -> Result<ID3D12Resource> {
  let heap_properties = D3D12_HEAP_PROPERTIES {
    Type: D3D12_HEAP_TYPE_DEFAULT,
    CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
    CreationNodeMask: 1,
    VisibleNodeMask: 1,
  };
  let desc = D3D12_RESOURCE_DESC {
    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    Alignment: 0,
    Width: width as u64,
    Height: height,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: DXGI_FORMAT_R8_UNORM,
    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
    Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
    Flags: D3D12_RESOURCE_FLAG_NONE,
  };
  let mut resource = None;
  device.CreateCommittedResource(
    &heap_properties,
    D3D12_HEAP_FLAG_NONE,
    &desc,
    D3D12_RESOURCE_STATE_COPY_DEST,
    None,
    &mut resource,
  )?;
  resource.ok_or_else(Error::from_win32)
}

#[cfg(feature = "raster")]
unsafe fn create_shared_texture(
  device: &ID3D12Device,
  width: u32,
  height: u32,
  format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
) -> Result<(ID3D12Resource, HANDLE, u64)> {
  const GENERIC_ALL: u32 = 0x10000000;
  let heap_properties = D3D12_HEAP_PROPERTIES {
    Type: D3D12_HEAP_TYPE_DEFAULT,
    CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
    CreationNodeMask: 1,
    VisibleNodeMask: 1,
  };
  let desc = D3D12_RESOURCE_DESC {
    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    Alignment: 0,
    Width: width as u64,
    Height: height,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: format,
    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
    Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
    Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
  };
  let allocation_size = device
    .GetResourceAllocationInfo(0, std::slice::from_ref(&desc))
    .SizeInBytes;
  let mut resource = None;
  device.CreateCommittedResource(
    &heap_properties,
    D3D12_HEAP_FLAG_SHARED,
    &desc,
    D3D12_RESOURCE_STATE_COMMON,
    None,
    &mut resource,
  )?;
  let resource: ID3D12Resource = resource.ok_or_else(Error::from_win32)?;
  let shared_handle = device.CreateSharedHandle(&resource, None, GENERIC_ALL, PCWSTR::null())?;
  Ok((resource, shared_handle, allocation_size))
}

#[cfg(feature = "raster")]
unsafe fn device_adapter_luid_parts(device: &ID3D12Device) -> (u32, i32) {
  let luid = device.GetAdapterLuid();
  (luid.LowPart, luid.HighPart)
}

#[cfg(feature = "raster")]
unsafe fn create_r8g8_texture(device: &ID3D12Device, width: u32, height: u32) -> Result<ID3D12Resource> {
  let heap_properties = D3D12_HEAP_PROPERTIES {
    Type: D3D12_HEAP_TYPE_DEFAULT,
    CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
    CreationNodeMask: 1,
    VisibleNodeMask: 1,
  };
  let desc = D3D12_RESOURCE_DESC {
    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    Alignment: 0,
    Width: width as u64,
    Height: height,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: DXGI_FORMAT_R8G8_UNORM,
    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
    Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
    Flags: D3D12_RESOURCE_FLAG_NONE,
  };
  let mut resource = None;
  device.CreateCommittedResource(
    &heap_properties,
    D3D12_HEAP_FLAG_NONE,
    &desc,
    D3D12_RESOURCE_STATE_COPY_DEST,
    None,
    &mut resource,
  )?;
  resource.ok_or_else(Error::from_win32)
}

unsafe fn create_rgba_texture(device: &ID3D12Device, width: u32, height: u32) -> Result<ID3D12Resource> {
  let heap_properties = D3D12_HEAP_PROPERTIES {
    Type: D3D12_HEAP_TYPE_DEFAULT,
    CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
    CreationNodeMask: 1,
    VisibleNodeMask: 1,
  };
  let desc = D3D12_RESOURCE_DESC {
    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    Alignment: 0,
    Width: width as u64,
    Height: height,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
    Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
    Flags: D3D12_RESOURCE_FLAG_NONE,
  };
  let mut resource = None;
  device.CreateCommittedResource(
    &heap_properties,
    D3D12_HEAP_FLAG_NONE,
    &desc,
    D3D12_RESOURCE_STATE_COPY_DEST,
    None,
    &mut resource,
  )?;
  resource.ok_or_else(Error::from_win32)
}

/// A rect's instances: its fill and, when it has one, its stroke; or its box
/// shadow. Returned inline with a count so drawing a rect does not allocate.
fn rect_instances(rect: &RectCmd, gradient_offset: f32) -> ([QuadInstance; 2], usize) {
  if let Some(shadow) = &rect.shadow {
    let instance = QuadInstance::box_shadow(rect, shadow);
    return ([instance, instance], 1);
  }
  let fill = QuadInstance {
    pos: [rect.x, rect.y],
    size: [rect.width, rect.height],
    color: rect.color.to_linear_f32_array(),
    radii_h: rect.radii,
    radii_v: rect.radii,
    stroke: [0.0; 4],
    pattern: [0.0; 4],
    transform: rect.transform,
    xf_origin: rect.transform_origin,
    shadow_sigma: 0.0,
    gradient_offset,
  };
  if rect.stroke.iter().any(|width| *width > 0.0) {
    let stroke = QuadInstance {
      pos: [rect.x, rect.y],
      size: [rect.width, rect.height],
      color: rect.stroke_color.to_linear_f32_array(),
      radii_h: rect.radii,
      radii_v: rect.radii,
      stroke: rect.stroke,
      pattern: [0.0; 4],
      transform: rect.transform,
      xf_origin: rect.transform_origin,
      shadow_sigma: 0.0,
      gradient_offset: -1.0,
    };
    return ([fill, stroke], 2);
  }
  ([fill, fill], 1)
}

/// A draw of the render list, in paint order.
enum OrderedDraw {
  Rect(usize),
  Glyph {
    start: usize,
    count: usize,
  },
  #[cfg(feature = "raster")]
  Image(usize),
  #[cfg(feature = "svg")]
  Svg(usize),
}

fn same_clip(a: ClipRect, b: ClipRect) -> bool {
  a.active == b.active
    && a.x == b.x
    && a.y == b.y
    && a.width == b.width
    && a.height == b.height
    && a.border_radius == b.border_radius
}

/// Globals for a draw clipped by `clip`, already in `space`'s pixels.
fn globals_for_clip(clip: ClipRect, space: &TargetSpace) -> Globals {
  let radius = clip.border_radius.unwrap_or_default();
  let radii = radius.to_array();
  Globals {
    viewport: space.viewport(),
    clip_rect: if clip.active {
      [clip.x, clip.y, clip.width, clip.height]
    } else {
      [0.0, 0.0, space.width, space.height]
    },
    clip_radii_h: radii,
    clip_radii_v: radii,
    clip_active: if rounded_clip_needs_shader(clip) {
      [1.0, 0.0, 0.0, 0.0]
    } else {
      [0.0; 4]
    },
  }
}

fn rounded_clip_needs_shader(clip: ClipRect) -> bool {
  let Some(radius) = clip.border_radius else {
    return false;
  };
  clip.active
    && (radius.top_left > 0.0 || radius.top_right > 0.0 || radius.bottom_right > 0.0 || radius.bottom_left > 0.0)
}

fn scissor_rect(clip: ClipRect, vw: f32, vh: f32) -> Option<RECT> {
  let viewport_w = vw.ceil().max(1.0) as i32;
  let viewport_h = vh.ceil().max(1.0) as i32;
  if clip.active {
    let left_f = clip.x.floor().max(0.0);
    let top_f = clip.y.floor().max(0.0);
    let right_f = (clip.x + clip.width).ceil().clamp(0.0, viewport_w as f32);
    let bottom_f = (clip.y + clip.height).ceil().clamp(0.0, viewport_h as f32);
    if right_f <= left_f || bottom_f <= top_f {
      return None;
    }

    let left = left_f as i32;
    let top = top_f as i32;
    if left >= viewport_w || top >= viewport_h {
      return None;
    }
    let width = ((right_f - left_f).ceil() as i32).min(viewport_w.saturating_sub(left));
    let height = ((bottom_f - top_f).ceil() as i32).min(viewport_h.saturating_sub(top));
    if width <= 0 || height <= 0 {
      return None;
    }
    Some(RECT {
      left,
      top,
      right: left + width,
      bottom: top + height,
    })
  } else {
    Some(RECT {
      left: 0,
      top: 0,
      right: viewport_w,
      bottom: viewport_h,
    })
  }
}

fn align_up(value: usize, alignment: usize) -> usize {
  (value + alignment - 1) & !(alignment - 1)
}

fn dx12_context<T>(result: Result<T>, stage: impl std::fmt::Display) -> Result<T> {
  result.map_err(|err| Error::new(err.code(), format!("{stage}: {}", err.message())))
}

fn is_dxgi_device_lost(err: &Error) -> bool {
  is_dxgi_device_lost_code(err.code())
}

fn is_dxgi_device_lost_code(code: HRESULT) -> bool {
  code == DXGI_ERROR_DEVICE_REMOVED
    || code == DXGI_ERROR_DEVICE_HUNG
    || code == DXGI_ERROR_DEVICE_RESET
    || code == DXGI_ERROR_DRIVER_INTERNAL_ERROR
}

fn dxgi_hresult_label(code: HRESULT) -> String {
  let label = if code == DXGI_ERROR_DEVICE_REMOVED {
    "DXGI_ERROR_DEVICE_REMOVED"
  } else if code == DXGI_ERROR_DEVICE_HUNG {
    "DXGI_ERROR_DEVICE_HUNG"
  } else if code == DXGI_ERROR_DEVICE_RESET {
    "DXGI_ERROR_DEVICE_RESET"
  } else if code == DXGI_ERROR_DRIVER_INTERNAL_ERROR {
    "DXGI_ERROR_DRIVER_INTERNAL_ERROR"
  } else if code == HRESULT(0) {
    "S_OK"
  } else {
    "UNKNOWN"
  };
  format!("{label}({:#010x})", code.0 as u32)
}

fn dx12_gpu_wait_timeout_error(stage: &str, reason: HRESULT) -> Error {
  Error::new(
    reason,
    format!(
      "{stage}: timed out waiting for dx12 GPU fence; removed_reason={}",
      dxgi_hresult_label(reason)
    ),
  )
}

fn dx12_frame_not_ready_error(stage: &str, detail: impl std::fmt::Display) -> Error {
  Error::new(DX12_FRAME_NOT_READY, format!("{stage}: {detail}"))
}

fn is_dx12_frame_not_ready(err: &Error) -> bool {
  err.code() == DX12_FRAME_NOT_READY
}

fn is_error_mod_not_found(err: &Error) -> bool {
  (err.code().0 as u32) == 0x8007007E
}

#[allow(dead_code)]
fn dx12_invalid_arg(message: impl Into<String>) -> Error {
  Error::new(HRESULT(0x80070057u32 as i32), message.into())
}

#[cfg(feature = "raster")]
fn frame_image_srv_range(frame_index: usize) -> (usize, usize) {
  let image_descriptor_count = SRV_DESCRIPTOR_COUNT as usize - IMAGE_SRV_FIRST_INDEX;
  let descriptors_per_frame = image_descriptor_count / FRAME_COUNT;
  let start = IMAGE_SRV_FIRST_INDEX + frame_index * descriptors_per_frame;
  let end = if frame_index + 1 == FRAME_COUNT {
    SRV_DESCRIPTOR_COUNT as usize
  } else {
    start + descriptors_per_frame
  };
  (start, end)
}

impl Dx12State {
  unsafe fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
    let factory = create_factory()?;
    let adapter = choose_adapter(&factory)?;
    let device = create_device(&adapter)?;

    let queue_desc = D3D12_COMMAND_QUEUE_DESC {
      Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
      Priority: 0,
      Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
      NodeMask: 0,
    };
    let command_queue: ID3D12CommandQueue = device.CreateCommandQueue(&queue_desc)?;

    let swapchain_desc = DXGI_SWAP_CHAIN_DESC1 {
      Width: width.max(1),
      Height: height.max(1),
      Format: SWAPCHAIN_FORMAT,
      Stereo: FALSE,
      SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
      BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
      BufferCount: FRAME_COUNT as u32,
      Scaling: DXGI_SCALING_NONE,
      SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
      AlphaMode: DXGI_ALPHA_MODE_IGNORE,
      Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
    };
    let swapchain1 = factory.CreateSwapChainForHwnd(
      &command_queue.cast::<windows::core::IUnknown>()?,
      hwnd,
      &swapchain_desc,
      None,
      None::<&IDXGIOutput>,
    )?;
    let swapchain: IDXGISwapChain3 = swapchain1.cast()?;
    let swapchain2: IDXGISwapChain2 = swapchain.cast()?;
    swapchain2.SetMaximumFrameLatency(1)?;
    let frame_latency_waitable = swapchain2.GetFrameLatencyWaitableObject();
    factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)?;

    let rtv_heap = CpuDescriptorHeap::new(
      &device,
      D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
      FRAME_COUNT as u32,
      D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
    )?;
    // Image descriptors, then each frame slot's opacity-layer descriptors.
    let srv_heap = CpuDescriptorHeap::new(
      &device,
      D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
      SRV_DESCRIPTOR_COUNT + (layers::LAYER_SRVS_PER_FRAME * FRAME_COUNT) as u32,
      D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
    )?;
    let sampler_heap = CpuDescriptorHeap::new(
      &device,
      D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
      1,
      D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
    )?;
    create_linear_clamp_sampler(&device, sampler_heap.cpu_handle(0));

    let quad_buffers = StaticQuadBuffers::new(&device)?;
    let rect_pipeline = RectPipeline::new(&device, RENDER_TARGET_FORMAT)?;
    let glyph_pipeline = GlyphPipeline::new(&device, RENDER_TARGET_FORMAT)?;
    #[cfg(feature = "raster")]
    let image_pipeline = ImagePipeline::new(&device, RENDER_TARGET_FORMAT)?;
    #[cfg(feature = "raster")]
    let nv12_image_pipeline = ImagePipeline::new_nv12(&device, RENDER_TARGET_FORMAT)?;
    #[cfg(feature = "svg")]
    let svg_pipeline = SvgPipeline::new(&device, RENDER_TARGET_FORMAT)?;

    let command_allocators = [
      device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?,
      device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?,
    ];
    let command_list: ID3D12GraphicsCommandList = device.CreateCommandList(
      0,
      D3D12_COMMAND_LIST_TYPE_DIRECT,
      &command_allocators[0],
      None::<&ID3D12PipelineState>,
    )?;
    command_list.Close()?;

    let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
    let fence_event = CreateEventW(None, false, false, None)?;
    let frame_arenas = [
      UploadArena::new(&device, FRAME_UPLOAD_ARENA_BYTES)?,
      UploadArena::new(&device, FRAME_UPLOAD_ARENA_BYTES)?,
    ];

    let mut state = Self {
      device,
      command_queue,
      #[cfg(feature = "canvas")]
      canvas_sources: Vec::new(),
      #[cfg(feature = "canvas")]
      canvas_renderer: None,
      #[cfg(feature = "canvas")]
      canvas_asset_budget: Default::default(),
      #[cfg(feature = "canvas")]
      canvas_retired: std::array::from_fn(|_| Vec::new()),
      swapchain,
      frame_latency_waitable,
      rtv_heap,
      srv_heap,
      sampler_heap,
      render_targets: [None, None],
      quad_buffers,
      rect_pipeline,
      glyph_pipeline,
      #[cfg(feature = "raster")]
      image_pipeline,
      #[cfg(feature = "raster")]
      nv12_image_pipeline,
      #[cfg(feature = "svg")]
      svg_pipeline,
      glyph_atlas: None,
      #[cfg(feature = "raster")]
      image_textures: HashMap::new(),
      #[cfg(feature = "raster")]
      native_image_draw_cadence: HashMap::new(),
      #[cfg(feature = "raster")]
      next_srv_index: 1,
      frame_arenas,
      frame_uploads: std::array::from_fn(|_| Vec::new()),
      command_allocators,
      command_list,
      fence,
      fence_event,
      fence_values: [0, 0],
      next_fence_value: 1,
      frame_index: 0,
      width: width.max(1),
      height: height.max(1),
      device_lost: false,
      #[cfg(feature = "screenshot")]
      pending_frame_capture: None,
      #[cfg(feature = "screenshot")]
      retired_capture_readbacks: std::array::from_fn(|_| Vec::new()),
      space: TargetSpace::window(width.max(1) as f32, height.max(1) as f32),
      layer_plan: LayerPlan::default(),
      layer_slots: Vec::new(),
      layer_compositor: None,
      #[cfg(feature = "perf_profile")]
      draw_calls: 0,
    };
    state.create_render_targets()?;
    state.frame_index = state.swapchain.GetCurrentBackBufferIndex() as usize;
    Ok(state)
  }

  unsafe fn resize(&mut self, width: u32, height: u32) -> Result<()> {
    let width = width.max(1);
    let height = height.max(1);
    if self.width == width && self.height == height {
      return Ok(());
    }

    self.wait_for_gpu()?;
    self.render_targets = [None, None];
    for uploads in &mut self.frame_uploads {
      uploads.clear();
    }
    self.swapchain.ResizeBuffers(
      FRAME_COUNT as u32,
      width,
      height,
      SWAPCHAIN_FORMAT,
      DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    )?;
    self.width = width;
    self.height = height;
    self.create_render_targets()?;
    self.frame_index = self.swapchain.GetCurrentBackBufferIndex() as usize;
    Ok(())
  }

  unsafe fn render(
    &mut self,
    list: &RenderList,
    _profile_context: Option<crate::app::profiler::ProfileContext>,
  ) -> Result<RenderProfile> {
    let _total_start = profile_scope!();
    let _acquire_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _acquire_phase = _profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::RenderAcquire));
    self.wait_for_frame_latency()?;
    self.frame_index = self.swapchain.GetCurrentBackBufferIndex() as usize;
    dx12_context(self.wait_for_frame(self.frame_index), "wait for dx12 frame")?;
    self.frame_uploads[self.frame_index].clear();
    #[cfg(feature = "canvas")]
    self.canvas_retired[self.frame_index].clear();
    #[cfg(feature = "screenshot")]
    self.retired_capture_readbacks[self.frame_index].clear();
    self.release_retired_layers();
    self.frame_arenas[self.frame_index].reset();
    #[cfg(feature = "raster")]
    self.begin_frame_image_descriptors(list);
    self.reset_frame_command_allocator()?;
    let allocator = &self.command_allocators[self.frame_index];
    dx12_context(
      self.command_list.Reset(allocator, None::<&ID3D12PipelineState>),
      "reset dx12 command list",
    )?;
    let _acquire_dur = profile_elapsed!(_acquire_start);
    #[cfg(feature = "perf_profile")]
    drop(_acquire_phase);

    let _encode_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _encode_phase = _profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::RenderEncode));
    #[cfg(feature = "perf_profile")]
    let mut _canvas_profile = crate::app::profile_types::CanvasProfile::default();
    #[cfg(feature = "canvas")]
    if !self.canvas_sources.is_empty() || self.canvas_renderer.is_some() {
      let _canvas_start = profile_scope!();
      let mut renderer = match self.canvas_renderer.take() {
        Some(r) => r,
        None => canvas::Renderer::new(&self.device)?.with_asset_budget(self.canvas_asset_budget),
      };
      let sources = self.canvas_sources.clone();
      #[cfg(feature = "perf_profile")]
      {
        renderer.profile_context = _profile_context.clone();
      }
      let result = renderer.encode(self, &sources);
      #[cfg(feature = "perf_profile")]
      {
        _canvas_profile = renderer.profile;
        _canvas_profile.total = profile_elapsed!(_canvas_start);
      }
      self.canvas_renderer = Some(renderer);
      if let Err(error) = result {
        let _ = self.command_list.Close();
        return Err(error);
      }
    }
    let (atlas_stats, _atlas_dur) = match self.encode_frame(list) {
      Ok(result) => result,
      Err(err) => {
        let _ = self.command_list.Close();
        return Err(err);
      }
    };

    #[cfg(feature = "screenshot")]
    let frame_capture = self.encode_pending_frame_capture();

    dx12_context(self.command_list.Close(), "close dx12 command list")?;
    let _encode_dur = profile_elapsed!(_encode_start);
    #[cfg(feature = "perf_profile")]
    drop(_encode_phase);

    let _submit_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _submit_phase = _profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::RenderSubmit));
    let command_list: ID3D12CommandList = dx12_context(self.command_list.cast(), "cast dx12 command list")?;
    self.command_queue.ExecuteCommandLists(&[Some(command_list)]);
    #[cfg(feature = "canvas")]
    if let Some(renderer) = &mut self.canvas_renderer {
      renderer.submitted();
    }
    dx12_context(self.signal_current_frame(), "signal dx12 frame fence")?;
    #[cfg(feature = "canvas")]
    if let Some(renderer) = &mut self.canvas_renderer {
      renderer.finish_readbacks(&self.fence, self.fence_values[self.frame_index]);
    }
    #[cfg(feature = "screenshot")]
    let capture_fence_value = self.fence_values[self.frame_index];
    let _submit_dur = profile_elapsed!(_submit_start);
    #[cfg(feature = "perf_profile")]
    drop(_submit_phase);

    let _present_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _present_phase = _profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::RenderPresent));
    dx12_context(
      self.swapchain.Present(1, Default::default()).ok(),
      "present dx12 swapchain",
    )?;
    self.frame_index = self.swapchain.GetCurrentBackBufferIndex() as usize;
    let _present_dur = profile_elapsed!(_present_start);
    #[cfg(feature = "perf_profile")]
    drop(_present_phase);

    #[cfg(feature = "screenshot")]
    if let Some((capture, readback)) = frame_capture {
      self.finish_frame_capture(capture, readback, capture_fence_value);
    }

    Ok(RenderProfile {
      canvas: {
        #[cfg(feature = "perf_profile")]
        {
          _canvas_profile
        }
        #[cfg(not(feature = "perf_profile"))]
        {
          Default::default()
        }
      },
      acquire: _acquire_dur,
      atlas_upload: _atlas_dur,
      glyph_atlas_upload_bytes: atlas_stats.bytes,
      glyph_atlas_upload_rects: atlas_stats.rects,
      glyph_atlas_full_uploads: atlas_stats.full_uploads,
      glyph_atlas_arena_uploads: atlas_stats.arena_uploads,
      glyph_atlas_dedicated_uploads: atlas_stats.dedicated_uploads,
      #[cfg(feature = "perf_profile")]
      draw_calls: self.draw_calls,
      encode: _encode_dur,
      submit: _submit_dur,
      present: _present_dur,
      total: profile_elapsed!(_total_start),
      ..RenderProfile::default()
    })
  }

  unsafe fn encode_frame(&mut self, list: &RenderList) -> Result<(GlyphAtlasUploadStats, std::time::Duration)> {
    profile_if! { self.draw_calls = 0; }
    let target = self.current_render_target();
    self.transition_resource(
      &target,
      D3D12_RESOURCE_STATE_PRESENT,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    let rtv = self.current_rtv_handle();
    self.command_list.OMSetRenderTargets(1, Some(&rtv), false, None);
    self
      .command_list
      .ClearRenderTargetView(rtv, &list.clear_color.to_f32_array(), None);

    let _atlas_start = profile_scope!();
    let atlas_stats = dx12_context(self.update_glyph_atlas(list), "update dx12 glyph atlas")?;
    let atlas_dur = profile_elapsed!(_atlas_start);
    dx12_context(self.draw_ordered(list), "draw dx12 render list")?;

    self.transition_resource(
      &target,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
      D3D12_RESOURCE_STATE_PRESENT,
    );

    Ok((atlas_stats, atlas_dur))
  }

  #[cfg(feature = "screenshot")]
  unsafe fn encode_pending_frame_capture(&mut self) -> Option<(RenderFrameCapture, FrameCaptureReadback)> {
    let capture = self.pending_frame_capture.take()?;
    match self.encode_frame_capture(&capture) {
      Ok(readback) => Some((capture, readback)),
      Err(err) => {
        capture
          .target
          .fail(format!("failed to encode DX12 frame capture: {err:?}"));
        None
      }
    }
  }

  /// Record a copy of the capture rect from the back buffer into a fresh
  /// readback buffer. The back buffer bytes are already sRGB-encoded RGBA8
  /// (the pixel shaders encode before the UNORM render target blends), so the
  /// readback maps straight to PNG pixels.
  #[cfg(feature = "screenshot")]
  unsafe fn encode_frame_capture(&mut self, capture: &RenderFrameCapture) -> Result<FrameCaptureReadback> {
    if capture.width == 0
      || capture.height == 0
      || capture.x + capture.width > self.width
      || capture.y + capture.height > self.height
    {
      return Err(dx12_invalid_arg(format!(
        "screenshot capture rect {}x{} at ({}, {}) exceeds the {}x{} surface",
        capture.width, capture.height, capture.x, capture.y, self.width, self.height
      )));
    }

    let unpadded_bytes_per_row = capture.width * 4;
    let padded_bytes_per_row = align_capture_row_pitch(unpadded_bytes_per_row, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT);
    let heap_properties = D3D12_HEAP_PROPERTIES {
      Type: D3D12_HEAP_TYPE_READBACK,
      CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
      MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
      CreationNodeMask: 1,
      VisibleNodeMask: 1,
    };
    let desc = buffer_resource_desc(padded_bytes_per_row as u64 * capture.height as u64);
    let mut resource = None;
    self.device.CreateCommittedResource(
      &heap_properties,
      D3D12_HEAP_FLAG_NONE,
      &desc,
      D3D12_RESOURCE_STATE_COPY_DEST,
      None,
      &mut resource,
    )?;
    let resource: ID3D12Resource = resource.ok_or_else(Error::from_win32)?;

    let target = self.current_render_target();
    self.transition_resource(&target, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
    let mut src = D3D12_TEXTURE_COPY_LOCATION {
      pResource: ManuallyDrop::new(Some(target.clone())),
      Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
      Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    let mut dst = D3D12_TEXTURE_COPY_LOCATION {
      pResource: ManuallyDrop::new(Some(resource.clone())),
      Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
      Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
          Offset: 0,
          Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
            Format: SWAPCHAIN_FORMAT,
            Width: capture.width,
            Height: capture.height,
            Depth: 1,
            RowPitch: padded_bytes_per_row,
          },
        },
      },
    };
    let source_box = D3D12_BOX {
      left: capture.x,
      top: capture.y,
      front: 0,
      right: capture.x + capture.width,
      bottom: capture.y + capture.height,
      back: 1,
    };
    self
      .command_list
      .CopyTextureRegion(&dst, 0, 0, 0, &src, Some(&source_box));
    ManuallyDrop::drop(&mut dst.pResource);
    ManuallyDrop::drop(&mut src.pResource);
    self.transition_resource(&target, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);

    Ok(FrameCaptureReadback {
      resource,
      padded_bytes_per_row,
      frame_index: self.frame_index,
    })
  }

  /// Wait for the capture copy's fence, map the readback buffer, and hand the
  /// tight RGBA pixels to a worker thread for clipping and PNG encoding.
  #[cfg(feature = "screenshot")]
  unsafe fn finish_frame_capture(
    &mut self,
    capture: RenderFrameCapture,
    readback: FrameCaptureReadback,
    fence_value: u64,
  ) {
    if let Err(err) = self.wait_for_capture_fence(fence_value) {
      capture.target.fail(format!("capture fence wait failed: {err:?}"));
      // The GPU may still be writing the buffer; keep it alive until this
      // frame slot's fence is waited on again.
      self.retired_capture_readbacks[readback.frame_index].push(readback.resource);
      return;
    }

    let buffer_size = readback.padded_bytes_per_row as usize * capture.height as usize;
    let read_range = D3D12_RANGE {
      Begin: 0,
      End: buffer_size,
    };
    let mut mapped: *mut c_void = ptr::null_mut();
    if let Err(error) = readback.resource.Map(0, Some(&read_range), Some(&mut mapped)) {
      capture
        .target
        .fail(format!("failed to map DX12 frame capture readback buffer: {error:?}"));
      return;
    }
    let data = std::slice::from_raw_parts(mapped.cast::<u8>(), buffer_size);
    let pixels = capture_rows_to_rgba(
      data,
      readback.padded_bytes_per_row as usize,
      capture.width,
      capture.height,
      false,
    );
    let written_range = D3D12_RANGE { Begin: 0, End: 0 };
    readback.resource.Unmap(0, Some(&written_range));

    std::thread::spawn(move || finish_capture(pixels, &capture));
  }

  #[cfg(feature = "screenshot")]
  unsafe fn wait_for_capture_fence(&mut self, fence_value: u64) -> Result<()> {
    if self.device_lost {
      return Err(dx12_invalid_arg("dx12 device lost before frame capture readback"));
    }
    if self.fence.GetCompletedValue() >= fence_value {
      return Ok(());
    }
    if let Err(err) = self.fence.SetEventOnCompletion(fence_value, self.fence_event) {
      if is_dxgi_device_lost_code(err.code()) {
        self.device_lost = true;
      }
      return Err(err);
    }
    let wait = WaitForSingleObject(self.fence_event, DX12_CAPTURE_WAIT_TIMEOUT_MS);
    if wait == WAIT_TIMEOUT {
      return Err(dx12_frame_not_ready_error(
        "wait for dx12 frame capture readback",
        format_args!("GPU work was still busy after {DX12_CAPTURE_WAIT_TIMEOUT_MS}ms; fence={fence_value}"),
      ));
    }
    debug_assert_eq!(wait, WAIT_OBJECT_0);
    Ok(())
  }

  unsafe fn upload_frame_bytes(&mut self, data: &[u8], alignment: usize) -> Result<UploadSlice> {
    if let Some(slice) = self.frame_arenas[self.frame_index].upload_bytes(data, alignment) {
      return Ok(slice);
    }

    let padded_size = align_up(data.len().max(1), alignment.max(1));
    let upload = UploadBuffer::from_bytes_padded(&self.device, data, padded_size)?;
    let slice = UploadSlice {
      resource: upload._resource.clone(),
      offset: 0,
      gpu_address: upload.gpu_address,
      size_in_bytes: upload.size_in_bytes,
    };
    self.frame_uploads[self.frame_index].push(upload);
    Ok(slice)
  }

  #[allow(dead_code)]
  unsafe fn upload_frame_rows(
    &mut self,
    data: &[u8],
    row_bytes: usize,
    row_pitch: usize,
    height: usize,
    alignment: usize,
  ) -> Result<UploadSlice> {
    if let Some(slice) = self.frame_arenas[self.frame_index].upload_rows(data, row_bytes, row_pitch, height, alignment)
    {
      return Ok(slice);
    }

    let upload_size = row_pitch * height;
    let mut upload_bytes = vec![0u8; upload_size];
    copy_upload_rows(data, upload_bytes.as_mut_ptr(), row_bytes, row_pitch, height);
    self.upload_frame_bytes(&upload_bytes, alignment)
  }

  unsafe fn upload_frame_pod_slice<T: bytemuck::Pod>(&mut self, data: &[T], alignment: usize) -> Result<UploadSlice> {
    self.upload_frame_bytes(bytemuck::cast_slice(data), alignment)
  }

  unsafe fn upload_frame_constant<T: bytemuck::Pod>(&mut self, data: &T) -> Result<UploadSlice> {
    self.upload_frame_bytes(bytemuck::bytes_of(data), 256)
  }

  unsafe fn draw_ordered(&mut self, list: &RenderList) -> Result<()> {
    let has_draws = !list.rects.is_empty() || !list.glyphs.is_empty();
    #[cfg(feature = "raster")]
    let has_draws = has_draws || !list.images.is_empty();
    #[cfg(feature = "svg")]
    let has_draws = has_draws || !list.svgs.is_empty();
    if !has_draws {
      return Ok(());
    }

    let viewport = D3D12_VIEWPORT {
      TopLeftX: 0.0,
      TopLeftY: 0.0,
      Width: self.width as f32,
      Height: self.height as f32,
      MinDepth: 0.0,
      MaxDepth: 1.0,
    };
    self.command_list.RSSetViewports(std::slice::from_ref(&viewport));
    self.space = TargetSpace::window(self.width as f32, self.height as f32);

    self
      .command_list
      .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    self.command_list.IASetIndexBuffer(Some(&self.quad_buffers.index_view));

    let mut ordered_draws = Vec::new();
    for (index, rect) in list.rects.iter().enumerate() {
      ordered_draws.push((rect.order, OrderedDraw::Rect(index)));
    }
    let mut glyph_start = 0;
    while glyph_start < list.glyphs.len() {
      let order = list.glyphs[glyph_start].order;
      let clip = list.glyphs[glyph_start].clip;
      let mut glyph_end = glyph_start + 1;
      while glyph_end < list.glyphs.len()
        && list.glyphs[glyph_end].order == order
        && same_clip(list.glyphs[glyph_end].clip, clip)
      {
        glyph_end += 1;
      }
      ordered_draws.push((
        order,
        OrderedDraw::Glyph {
          start: glyph_start,
          count: glyph_end - glyph_start,
        },
      ));
      glyph_start = glyph_end;
    }
    #[cfg(feature = "raster")]
    for (index, image) in list.images.iter().enumerate() {
      ordered_draws.push((image.order, OrderedDraw::Image(index)));
    }
    #[cfg(feature = "svg")]
    for (index, svg) in list.svgs.iter().enumerate() {
      ordered_draws.push((svg.order, OrderedDraw::Svg(index)));
    }
    ordered_draws.sort_by_key(|(order, _)| *order);

    // Faded subtrees paint into offscreen layers first, each before the
    // target it is composited into; the window comes last. Without layers the
    // plan is the window's draws alone.
    let mut plan = std::mem::take(&mut self.layer_plan);
    let mut slots = std::mem::take(&mut self.layer_slots);
    plan.build(ordered_draws.len(), |index| ordered_draws[index].0, &list.layers);
    let result = self.draw_planned(list, &ordered_draws, &plan, &mut slots);
    self.layer_plan = plan;
    self.layer_slots = slots;
    result
  }

  unsafe fn draw_planned(
    &mut self,
    list: &RenderList,
    draws: &[(usize, OrderedDraw)],
    plan: &LayerPlan,
    slots: &mut Vec<Option<usize>>,
  ) -> Result<()> {
    slots.clear();
    if plan.has_layers() {
      let targets: Vec<_> = plan
        .targets()
        .iter()
        .map(|target| target.layer.map(|layer| &list.layers[layer]))
        .collect();
      dx12_context(self.begin_layer_frame(&targets, slots), "prepare dx12 opacity layers")?;
    }
    for (target_index, target) in plan.targets().iter().enumerate() {
      let layer_slot = slots.get(target_index).copied().flatten();
      match (target.layer, layer_slot) {
        (Some(layer), Some(slot)) => self.begin_layer_target(slot, &list.layers[layer]),
        _ if plan.has_layers() => self.begin_window_target(),
        _ => {}
      }
      for step in plan.steps(target) {
        match step {
          LayerStep::Draws(range) => {
            for (_, draw) in &draws[range.clone()] {
              self.draw_one(list, draw)?;
            }
          }
          LayerStep::Composite(child) => {
            let child_layer = plan.targets()[*child].layer;
            if let (Some(layer), Some(slot)) = (child_layer, slots.get(*child).copied().flatten()) {
              dx12_context(
                self.composite_layer(slot, &list.layers[layer]),
                "composite dx12 opacity layer",
              )?;
            }
          }
        }
      }
      if let (Some(_), Some(slot)) = (target.layer, layer_slot) {
        self.end_layer_target(slot);
      }
    }
    if plan.has_layers() {
      self.end_layer_frame();
    }
    Ok(())
  }

  unsafe fn draw_one(&mut self, list: &RenderList, draw: &OrderedDraw) -> Result<()> {
    match *draw {
      OrderedDraw::Rect(index) => dx12_context(
        self.draw_rect(&list.rects[index]),
        format_args!("draw dx12 rect #{index}"),
      ),
      OrderedDraw::Glyph { start, count } => dx12_context(
        self.draw_glyphs(&list.glyphs[start..start + count]),
        format_args!("draw dx12 glyph run start={start} count={count}"),
      ),
      #[cfg(feature = "raster")]
      OrderedDraw::Image(index) => {
        if DX12_IMAGE_DRAWS_DISABLED.load(Ordering::Relaxed) {
          return Ok(());
        }
        let image = &list.images[index];
        let result = dx12_context(
          self.draw_image(image),
          format_args!(
            "draw dx12 image #{} id={} format={:?} native={}",
            index,
            image.image_id,
            image.image_format,
            image.native.is_some()
          ),
        );
        if let Err(err) = result {
          if is_error_mod_not_found(&err) {
            DX12_IMAGE_DRAWS_DISABLED.store(true, Ordering::Relaxed);
            if !DX12_IMAGE_MOD_NOT_FOUND_LOGGED.swap(true, Ordering::Relaxed) {
              tracing::error!(
                "disabling dx12 image draws after ERROR_MOD_NOT_FOUND; continuing frame so non-image UI can render"
              );
            }
            return Ok(());
          }
          return Err(err);
        }
        Ok(())
      }
      #[cfg(feature = "svg")]
      OrderedDraw::Svg(index) => dx12_context(self.draw_svg(&list.svgs[index]), format_args!("draw dx12 svg #{index}")),
    }
  }

  unsafe fn draw_rect(&mut self, rect: &RectCmd) -> Result<()> {
    let Some(scissor) = scissor_rect(self.space.clip(rect.clip), self.space.width, self.space.height) else {
      return Ok(());
    };

    // Encode this rect's gradient (if any) into a per-draw storage buffer.
    // The structured buffer must always be bound, so a single zeroed vec4 is
    // uploaded when there is no gradient (offset stays -1 and is never read).
    let mut gradient_data: Vec<[f32; 4]> = Vec::new();
    let gradient_offset = match &rect.gradient {
      Some(gradient) => crate::layout::render_list::encode_gradient(&mut gradient_data, gradient),
      None => -1.0,
    };
    const NO_GRADIENT: [[f32; 4]; 1] = [[0.0; 4]];
    let gradient_upload = if gradient_data.is_empty() {
      self.upload_frame_pod_slice(&NO_GRADIENT, 16)?
    } else {
      self.upload_frame_pod_slice(&gradient_data, 16)?
    };
    let globals = globals_for_clip(self.space.clip(rect.clip), &self.space);
    let globals_upload = self.upload_frame_constant(&globals)?;

    self.command_list.SetPipelineState(&self.rect_pipeline.pipeline_state);
    self
      .command_list
      .SetGraphicsRootSignature(&self.rect_pipeline.root_signature);
    self
      .command_list
      .SetGraphicsRootConstantBufferView(0, globals_upload.gpu_address);
    self
      .command_list
      .SetGraphicsRootShaderResourceView(1, gradient_upload.gpu_address);

    let (instances, instance_count) = rect_instances(rect, gradient_offset);
    let instances = &instances[..instance_count];
    let instance_upload = self.upload_frame_pod_slice(instances, 16)?;
    let instance_view = instance_upload.vertex_view::<QuadInstance>();

    let vertex_views = [self.quad_buffers.vertex_view, instance_view];
    self.command_list.RSSetScissorRects(std::slice::from_ref(&scissor));
    self.command_list.IASetVertexBuffers(0, Some(&vertex_views));
    self
      .command_list
      .DrawIndexedInstanced(QuadVertex::INDICES.len() as u32, instances.len() as u32, 0, 0, 0);
    profile_if! { self.draw_calls += 1; }

    Ok(())
  }

  unsafe fn draw_glyphs(&mut self, glyphs: &[GlyphCmd]) -> Result<()> {
    if glyphs.is_empty() {
      return Ok(());
    }
    if self.glyph_atlas.is_none() {
      if crate::app::glyph_engine::glyph_debug_marker().is_some() {
        eprintln!(
          "[glyph-debug] dx12 SKIPPED batch of {} glyphs: no atlas texture (first at ({}, {}))",
          glyphs.len(),
          glyphs[0].x,
          glyphs[0].y,
        );
      }
      return Ok(());
    }
    let Some(scissor) = scissor_rect(self.space.clip(glyphs[0].clip), self.space.width, self.space.height) else {
      if crate::app::glyph_engine::glyph_debug_marker().is_some() {
        let clip = glyphs[0].clip;
        eprintln!(
          "[glyph-debug] dx12 SKIPPED batch of {} glyphs: degenerate scissor from clip ({}, {}, {}x{}, active={}) surface {}x{}",
          glyphs.len(),
          clip.x,
          clip.y,
          clip.width,
          clip.height,
          clip.active,
          self.width,
          self.height,
        );
      }
      return Ok(());
    };

    let atlas = self.glyph_atlas.as_ref().unwrap();
    let atlas_inverse = [1.0 / atlas.width.max(1) as f32, 1.0 / atlas.height.max(1) as f32];
    let instances: Vec<GlyphInstance> = glyphs
      .iter()
      .map(|glyph| glyph_instance(glyph, atlas_inverse))
      .collect();
    let instance_upload = self.upload_frame_pod_slice(&instances, 16)?;
    let instance_view = instance_upload.vertex_view::<GlyphInstance>();
    let globals = globals_for_clip(self.space.clip(glyphs[0].clip), &self.space);
    let globals_upload = self.upload_frame_constant(&globals)?;

    let descriptor_heaps = [Some(self.srv_heap.heap.clone()), Some(self.sampler_heap.heap.clone())];
    self.command_list.SetDescriptorHeaps(&descriptor_heaps);
    self.command_list.SetPipelineState(&self.glyph_pipeline.pipeline_state);
    self
      .command_list
      .SetGraphicsRootSignature(&self.glyph_pipeline.root_signature);
    self
      .command_list
      .SetGraphicsRootConstantBufferView(0, globals_upload.gpu_address);
    self
      .command_list
      .SetGraphicsRootDescriptorTable(1, self.srv_heap.gpu_handle(GLYPH_ATLAS_SRV_INDEX));
    self
      .command_list
      .SetGraphicsRootDescriptorTable(2, self.sampler_heap.gpu_handle(0));

    let vertex_views = [self.quad_buffers.vertex_view, instance_view];
    self.command_list.RSSetScissorRects(std::slice::from_ref(&scissor));
    self.command_list.IASetVertexBuffers(0, Some(&vertex_views));
    self
      .command_list
      .DrawIndexedInstanced(QuadVertex::INDICES.len() as u32, instances.len() as u32, 0, 0, 0);
    profile_if! { self.draw_calls += 1; }

    Ok(())
  }

  #[cfg(feature = "raster")]
  fn record_native_image_draw(&mut self, image: &crate::images::ImageCmd, stream_frame: Option<u32>) {
    let now = Instant::now();
    let stats = self
      .native_image_draw_cadence
      .entry(image.image_id)
      .or_insert_with(|| NativeImageDrawCadence {
        started_at: now,
        draws: 0,
        version_changes: 0,
        repeated_draws: 0,
        skipped_versions: 0,
        last_version: None,
        stream_frame_changes: 0,
        repeated_stream_frames: 0,
        backward_stream_frames: 0,
        skipped_stream_frames: 0,
        max_stream_frame_delta: 0,
        last_stream_frame: None,
        last_draw_at: None,
        max_draw_delta: Duration::ZERO,
        max_version_delta: 0,
        last_width: image.image_width,
        last_height: image.image_height,
      });

    stats.draws += 1;
    if let Some(last_draw_at) = stats.last_draw_at {
      stats.max_draw_delta = stats.max_draw_delta.max(now.duration_since(last_draw_at));
    }
    stats.last_draw_at = Some(now);
    stats.last_width = image.image_width;
    stats.last_height = image.image_height;
    match stats.last_version {
      Some(last_version) if last_version == image.version => {
        stats.repeated_draws += 1;
      }
      Some(last_version) => {
        let version_delta = image.version.saturating_sub(last_version);
        stats.max_version_delta = stats.max_version_delta.max(version_delta);
        stats.version_changes += 1;
        stats.skipped_versions += version_delta.saturating_sub(1);
        stats.last_version = Some(image.version);
      }
      None => {
        stats.version_changes += 1;
        stats.last_version = Some(image.version);
      }
    }
    if let Some(stream_frame) = stream_frame {
      match stats.last_stream_frame {
        Some(last_stream_frame) if last_stream_frame == stream_frame => {
          stats.repeated_stream_frames += 1;
        }
        Some(last_stream_frame) if stream_frame_after(stream_frame, last_stream_frame) => {
          let frame_delta = stream_frame.wrapping_sub(last_stream_frame);
          stats.max_stream_frame_delta = stats.max_stream_frame_delta.max(frame_delta);
          stats.skipped_stream_frames += u64::from(frame_delta.saturating_sub(1));
          stats.stream_frame_changes += 1;
          stats.last_stream_frame = Some(stream_frame);
        }
        Some(last_stream_frame) => {
          stats.backward_stream_frames += 1;
          crate::app::profile_support::video_log!(debug,
            target: "video::watch::lurq",
            "dx12 native image frame order image={} drawn_frame={} previous_drawn_frame={} backwards=true image_version={} size={}x{}",
            image.image_id,
            stream_frame,
            last_stream_frame,
            image.version,
            image.image_width,
            image.image_height
          );
          stats.last_stream_frame = Some(stream_frame);
        }
        None => {
          stats.stream_frame_changes += 1;
          stats.last_stream_frame = Some(stream_frame);
        }
      }
    }

    if now.duration_since(stats.started_at) < Duration::from_secs(1) {
      return;
    }

    crate::app::profile_support::video_log!(debug,
      target: "video::watch::lurq",
      "dx12 native image cadence image={} size={}x{} draws={} version_changes={} repeated_draws={} skipped_versions={} max_draw_delta_ms={:.2} max_version_delta={} last_version={:?} stream_frame_changes={} repeated_stream_frames={} backward_stream_frames={} skipped_stream_frames={} max_stream_frame_delta={} last_stream_frame={:?}",
      image.image_id,
      stats.last_width,
      stats.last_height,
      stats.draws,
      stats.version_changes,
      stats.repeated_draws,
      stats.skipped_versions,
      duration_ms(stats.max_draw_delta),
      stats.max_version_delta,
      stats.last_version,
      stats.stream_frame_changes,
      stats.repeated_stream_frames,
      stats.backward_stream_frames,
      stats.skipped_stream_frames,
      stats.max_stream_frame_delta,
      stats.last_stream_frame
    );

    *stats = NativeImageDrawCadence {
      started_at: now,
      draws: 0,
      version_changes: 0,
      repeated_draws: 0,
      skipped_versions: 0,
      last_version: stats.last_version,
      stream_frame_changes: 0,
      repeated_stream_frames: 0,
      backward_stream_frames: 0,
      skipped_stream_frames: 0,
      max_stream_frame_delta: 0,
      last_stream_frame: stats.last_stream_frame,
      last_draw_at: stats.last_draw_at,
      max_draw_delta: Duration::ZERO,
      max_version_delta: 0,
      last_width: image.image_width,
      last_height: image.image_height,
    };
  }

  #[cfg(feature = "raster")]
  unsafe fn draw_image(&mut self, image: &crate::images::ImageCmd) -> Result<()> {
    let refreshed_native_image = image.native.as_ref().and_then(|native| {
      let version = native.version();
      (version != image.version).then(|| crate::images::ImageCmd {
        version,
        native: Some(native.clone()),
        ..image.clone()
      })
    });
    let image = refreshed_native_image.as_ref().unwrap_or(image);

    if image.image_width == 0 || image.image_height == 0 {
      return Ok(());
    }
    let Some(scissor) = scissor_rect(self.space.clip(image.clip), self.space.width, self.space.height) else {
      return Ok(());
    };
    #[cfg(feature = "canvas")]
    let canvas_image = image
      .native
      .as_ref()
      .filter(|n| n.backend() == crate::images::NativeImageBackend::Canvas);
    #[cfg(not(feature = "canvas"))]
    let canvas_image: Option<&crate::images::NativeImageData> = None;
    #[cfg(feature = "canvas")]
    let canvas_descriptor = if let Some(native) = canvas_image {
      let backing = native
        .payload::<crate::canvas::CanvasWeak>()
        .and_then(|c| c.upgrade())
        .and_then(|c| self.canvas_renderer.as_ref()?.backing(&c));
      let Some(backing) = backing else {
        return Ok(());
      };
      let index = self.allocate_frame_image_descriptors(image.image_id, 1)?;
      canvas::create_srv(&self.device, &backing, self.srv_heap.cpu_handle(index));
      Some(index)
    } else {
      None
    };
    #[cfg(not(feature = "canvas"))]
    let canvas_descriptor: Option<usize> = None;
    let descriptor_index = if let Some(index) = canvas_descriptor {
      index
    } else {
      self.ensure_image_texture(image)?
    };
    let native_nv12_mutexes = match self.image_textures.get(&image.image_id) {
      Some(CachedImageTexture::NativeNv12 {
        y_keyed_mutex,
        uv_keyed_mutex,
        ..
      }) => Some((y_keyed_mutex.clone(), uv_keyed_mutex.clone())),
      _ => None,
    };
    let native_nv12_resources = match self.image_textures.get(&image.image_id) {
      Some(CachedImageTexture::NativeNv12 {
        _y_texture,
        _uv_texture,
        y_keyed_mutex,
        uv_keyed_mutex,
        ..
      }) => Some((
        _y_texture.clone(),
        if y_keyed_mutex.is_some() && uv_keyed_mutex.is_none() {
          None
        } else {
          Some(_uv_texture.clone())
        },
      )),
      _ => None,
    };
    let native_stream_frame = image
      .native
      .as_ref()
      .and_then(native_dx12_nv12_image)
      .and_then(|native| native.frame_number);
    let descriptor_stream_frame = match self.image_textures.get(&image.image_id) {
      Some(CachedImageTexture::NativeNv12 {
        descriptor_stream_frames,
        ..
      }) => descriptor_stream_frames[self.frame_index],
      _ => None,
    };
    if native_stream_frame != descriptor_stream_frame {
      crate::app::profile_support::video_log!(debug,
        target: "video::watch::lurq",
        "dx12 native image descriptor frame mismatch image={} live_frame={:?} descriptor_frame={:?} image_version={} frame_index={} descriptor={}",
        image.image_id,
        native_stream_frame,
        descriptor_stream_frame,
        image.version,
        self.frame_index,
        descriptor_index
      );
    }
    if native_nv12_resources.is_some() {
      self.record_native_image_draw(image, native_stream_frame);
    }
    let y_keyed_mutex_acquired = acquire_native_nv12_mutex(
      native_nv12_mutexes.as_ref().and_then(|(mutex, _)| mutex.as_ref()),
      image.image_id,
      image.version,
      "y",
    );
    let uv_keyed_mutex_acquired = acquire_native_nv12_mutex(
      native_nv12_mutexes.as_ref().and_then(|(_, mutex)| mutex.as_ref()),
      image.image_id,
      image.version,
      "uv",
    );
    if native_nv12_resources.is_some() {
      let draw_count = DX12_NATIVE_IMAGE_DRAW_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
      if draw_count == 1 || draw_count % 120 == 0 {
        dx12_native_image_log(format_args!(
          "draw #{} image={} version={} stream_frame={:?} y_keyed={} y_acquired={} uv_keyed={} uv_acquired={} descriptor={}",
          draw_count,
          image.image_id,
          image.version,
          native_stream_frame,
          native_nv12_mutexes
            .as_ref()
            .and_then(|(mutex, _)| mutex.as_ref())
            .is_some(),
          y_keyed_mutex_acquired,
          native_nv12_mutexes
            .as_ref()
            .and_then(|(_, mutex)| mutex.as_ref())
            .is_some(),
          uv_keyed_mutex_acquired,
          descriptor_index
        ));
      }
    }
    if let Some((y_texture, uv_texture)) = &native_nv12_resources {
      self.transition_resource(
        y_texture,
        D3D12_RESOURCE_STATE_COMMON,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      );
      if let Some(uv_texture) = uv_texture {
        self.transition_resource(
          uv_texture,
          D3D12_RESOURCE_STATE_COMMON,
          D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        );
      }
    }
    let (pipeline_state, root_signature) = match image.image_format {
      crate::images::ImagePixelFormat::Rgba8 => (
        self.image_pipeline.pipeline_state.clone(),
        self.image_pipeline.root_signature.clone(),
      ),
      crate::images::ImagePixelFormat::Nv12 => (
        self.nv12_image_pipeline.pipeline_state.clone(),
        self.nv12_image_pipeline.root_signature.clone(),
      ),
    };

    let instance = ImageInstance {
      pos: [image.x, image.y],
      size: [image.width, image.height],
      opacity: [image.opacity, if canvas_image.is_some() { 1. } else { 0. }, 0.0, 0.0],
      transform: image.transform,
      xf_origin: image.transform_origin,
      uv_min: image.uv_min,
      uv_max: image.uv_max,
      radii: image.radii,
    };
    let instance_upload = self.upload_frame_pod_slice(&[instance], 16)?;
    let instance_view = instance_upload.vertex_view::<ImageInstance>();
    let globals = globals_for_clip(self.space.clip(image.clip), &self.space);
    let globals_upload = self.upload_frame_constant(&globals)?;

    let descriptor_heaps = [Some(self.srv_heap.heap.clone()), Some(self.sampler_heap.heap.clone())];
    self.command_list.SetDescriptorHeaps(&descriptor_heaps);
    self.command_list.SetPipelineState(&pipeline_state);
    self.command_list.SetGraphicsRootSignature(&root_signature);
    self
      .command_list
      .SetGraphicsRootConstantBufferView(0, globals_upload.gpu_address);
    self
      .command_list
      .SetGraphicsRootDescriptorTable(1, self.srv_heap.gpu_handle(descriptor_index));
    self
      .command_list
      .SetGraphicsRootDescriptorTable(2, self.sampler_heap.gpu_handle(0));

    let vertex_views = [self.quad_buffers.vertex_view, instance_view];
    self.command_list.RSSetScissorRects(std::slice::from_ref(&scissor));
    self.command_list.IASetVertexBuffers(0, Some(&vertex_views));
    self
      .command_list
      .DrawIndexedInstanced(QuadVertex::INDICES.len() as u32, 1, 0, 0, 0);
    profile_if! { self.draw_calls += 1; }
    if uv_keyed_mutex_acquired && let Some((_, Some(mutex))) = native_nv12_mutexes.as_ref() {
      let _ = mutex.ReleaseSync(0);
    }
    if y_keyed_mutex_acquired && let Some((Some(mutex), _)) = native_nv12_mutexes.as_ref() {
      let _ = mutex.ReleaseSync(0);
    }
    if let Some((y_texture, uv_texture)) = &native_nv12_resources {
      self.transition_resource(
        y_texture,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_COMMON,
      );
      if let Some(uv_texture) = uv_texture {
        self.transition_resource(
          uv_texture,
          D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
          D3D12_RESOURCE_STATE_COMMON,
        );
      }
    }

    Ok(())
  }

  #[cfg(feature = "svg")]
  unsafe fn draw_svg(&mut self, svg: &crate::svg::SvgCmd) -> Result<()> {
    if svg.mesh.vertices.is_empty() || svg.mesh.indices.is_empty() {
      return Ok(());
    }
    let Some(scissor) = scissor_rect(self.space.clip(svg.clip), self.space.width, self.space.height) else {
      return Ok(());
    };

    let vertices: Vec<SvgVertexGpu> = svg
      .mesh
      .vertices
      .iter()
      .map(|vertex| SvgVertexGpu {
        position: [vertex.position[0] + svg.x, vertex.position[1] + svg.y],
        color: vertex.color,
      })
      .collect();
    let vertex_upload = self.upload_frame_pod_slice(&vertices, 16)?;
    let index_upload = self.upload_frame_pod_slice(&svg.mesh.indices, 4)?;
    let globals = globals_for_clip(self.space.clip(svg.clip), &self.space);
    let globals_upload = self.upload_frame_constant(&globals)?;
    let vertex_view = vertex_upload.vertex_view::<SvgVertexGpu>();
    let index_view = D3D12_INDEX_BUFFER_VIEW {
      BufferLocation: index_upload.gpu_address,
      SizeInBytes: index_upload.size_in_bytes,
      Format: DXGI_FORMAT_R32_UINT,
    };

    self.command_list.SetPipelineState(&self.svg_pipeline.pipeline_state);
    self
      .command_list
      .SetGraphicsRootSignature(&self.svg_pipeline.root_signature);
    self
      .command_list
      .SetGraphicsRootConstantBufferView(0, globals_upload.gpu_address);
    self.command_list.RSSetScissorRects(std::slice::from_ref(&scissor));
    self
      .command_list
      .IASetVertexBuffers(0, Some(std::slice::from_ref(&vertex_view)));
    self.command_list.IASetIndexBuffer(Some(&index_view));
    self
      .command_list
      .DrawIndexedInstanced(svg.mesh.indices.len() as u32, 1, 0, 0, 0);
    profile_if! { self.draw_calls += 1; }
    self.command_list.IASetIndexBuffer(Some(&self.quad_buffers.index_view));

    Ok(())
  }

  unsafe fn update_glyph_atlas(&mut self, list: &RenderList) -> Result<GlyphAtlasUploadStats> {
    let atlas = &list.atlas;
    if list.glyphs.is_empty() || atlas.width == 0 || atlas.height == 0 {
      return Ok(GlyphAtlasUploadStats::default());
    }

    let recreate = self.glyph_atlas.as_ref().map_or(true, |texture| {
      texture.width != atlas.width || texture.height != atlas.height
    });
    if recreate {
      let texture = create_rgba_texture(&self.device, atlas.width, atlas.height)?;
      let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
          Texture2D: D3D12_TEX2D_SRV {
            MostDetailedMip: 0,
            MipLevels: 1,
            PlaneSlice: 0,
            ResourceMinLODClamp: 0.0,
          },
        },
      };
      self.device.CreateShaderResourceView(
        &texture,
        Some(&srv_desc),
        self.srv_heap.cpu_handle(GLYPH_ATLAS_SRV_INDEX),
      );
      self.glyph_atlas = Some(GlyphAtlasTexture {
        texture,
        width: atlas.width,
        height: atlas.height,
        version: u64::MAX,
        state: D3D12_RESOURCE_STATE_COPY_DEST,
      });
    }

    let needs_upload = self
      .glyph_atlas
      .as_ref()
      .map_or(false, |texture| texture.version != atlas.version);
    if !needs_upload {
      return Ok(GlyphAtlasUploadStats::default());
    }

    let mut stats = GlyphAtlasUploadStats::default();
    let texture = self.glyph_atlas.as_ref().unwrap().texture.clone();
    let current_state = self.glyph_atlas.as_ref().unwrap().state;
    if current_state != D3D12_RESOURCE_STATE_COPY_DEST {
      self.transition_resource(&texture, current_state, D3D12_RESOURCE_STATE_COPY_DEST);
    }

    // The atlas is shared across windows; this texture may be several
    // versions behind, so apply the log's rects newer than *our* uploaded
    // version — or the full atlas when the log no longer covers the gap.
    let uploaded_version = self.glyph_atlas.as_ref().unwrap().version;
    let pending = if recreate {
      None
    } else {
      crate::app::glyph_engine::atlas_rects_to_apply(atlas, uploaded_version)
    };

    if pending.is_none() || DX12_ALWAYS_FULL_ATLAS_UPLOAD {
      let row_bytes = atlas.width as usize * 4;
      let row_pitch = align_up(row_bytes, 256);
      let upload_size = row_pitch * atlas.height as usize;
      let dedicated_before = self.frame_uploads[self.frame_index].len();
      // Frame arenas are reset only after their GPU fence completes. Normal
      // atlases have aligned rows, so copy directly into the mapped arena once.
      // Oversized uploads retain the existing dedicated-buffer fallback.
      let upload = if row_pitch == row_bytes && atlas.data.len() >= upload_size {
        self.upload_frame_bytes(&atlas.data[..upload_size], 512)?
      } else {
        self.upload_frame_rows(&atlas.data, row_bytes, row_pitch, atlas.height as usize, 512)?
      };
      let mut dst = D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(texture.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
      };
      let mut src = D3D12_TEXTURE_COPY_LOCATION {
        pResource: ManuallyDrop::new(Some(upload.resource.clone())),
        Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
          PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
            Offset: upload.offset,
            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
              Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
              Width: atlas.width,
              Height: atlas.height,
              Depth: 1,
              RowPitch: row_pitch as u32,
            },
          },
        },
      };
      self.command_list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
      ManuallyDrop::drop(&mut dst.pResource);
      ManuallyDrop::drop(&mut src.pResource);
      stats.bytes += upload_size;
      stats.rects += 1;
      stats.full_uploads += 1;
      let dedicated = self.frame_uploads[self.frame_index].len() - dedicated_before;
      stats.dedicated_uploads += dedicated;
      stats.arena_uploads += usize::from(dedicated == 0);
    } else {
      for rect in pending.unwrap_or_default() {
        if rect.width == 0 || rect.height == 0 || rect.x >= atlas.width || rect.y >= atlas.height {
          continue;
        }
        let width = rect.width.min(atlas.width - rect.x);
        let height = rect.height.min(atlas.height - rect.y);
        let row_bytes = width as usize * 4;
        let atlas_row_bytes = atlas.width as usize * 4;
        let rect_x_bytes = rect.x as usize * 4;
        let row_pitch = align_up(row_bytes, 256);
        let upload_size = row_pitch * height as usize;
        let mut upload_bytes = vec![0u8; upload_size];
        for row in 0..height as usize {
          let src_start = (rect.y as usize + row) * atlas_row_bytes + rect_x_bytes;
          let src_end = src_start + row_bytes;
          if src_end > atlas.data.len() {
            continue;
          }
          let dst_start = row * row_pitch;
          upload_bytes[dst_start..dst_start + row_bytes].copy_from_slice(&atlas.data[src_start..src_end]);
        }

        let upload = UploadBuffer::from_bytes(&self.device, &upload_bytes)?;
        let mut dst = D3D12_TEXTURE_COPY_LOCATION {
          pResource: ManuallyDrop::new(Some(texture.clone())),
          Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
          Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        let mut src = D3D12_TEXTURE_COPY_LOCATION {
          pResource: ManuallyDrop::new(Some(upload._resource.clone())),
          Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
          Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
            PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
              Offset: 0,
              Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                Width: width,
                Height: height,
                Depth: 1,
                RowPitch: row_pitch as u32,
              },
            },
          },
        };
        self.command_list.CopyTextureRegion(&dst, rect.x, rect.y, 0, &src, None);
        ManuallyDrop::drop(&mut dst.pResource);
        ManuallyDrop::drop(&mut src.pResource);
        stats.bytes += upload_size;
        stats.rects += 1;
        stats.dedicated_uploads += 1;
        self.frame_uploads[self.frame_index].push(upload);
      }
    }
    self.transition_resource(
      &texture,
      D3D12_RESOURCE_STATE_COPY_DEST,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );

    if let Some(glyph_atlas) = &mut self.glyph_atlas {
      glyph_atlas.version = atlas.version;
      glyph_atlas.state = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    }

    Ok(stats)
  }

  #[cfg(feature = "raster")]
  fn begin_frame_image_descriptors(&mut self, list: &RenderList) {
    let (start, _) = frame_image_srv_range(self.frame_index);
    self.next_srv_index = start;
    let live_image_ids: HashSet<u64> = list.images.iter().map(|image| image.image_id).collect();
    self
      .image_textures
      .retain(|image_id, _| live_image_ids.contains(image_id));
    for texture in self.image_textures.values_mut() {
      texture.clear_descriptor_index();
    }
  }

  #[cfg(feature = "raster")]
  fn allocate_frame_image_descriptors(&mut self, image_id: u64, descriptors_needed: usize) -> Result<usize> {
    let (_, frame_srv_end) = frame_image_srv_range(self.frame_index);
    if self.next_srv_index + descriptors_needed > frame_srv_end {
      return Err(dx12_invalid_arg(format!(
        "dx12 image descriptor heap exhausted: image id={} needs {} descriptors, next={}, frame_end={}, capacity={}",
        image_id, descriptors_needed, self.next_srv_index, frame_srv_end, SRV_DESCRIPTOR_COUNT
      )));
    }

    let descriptor_index = self.next_srv_index;
    self.next_srv_index += descriptors_needed;
    Ok(descriptor_index)
  }

  #[cfg(feature = "raster")]
  unsafe fn write_rgba_image_srv(&self, texture: &ID3D12Resource, descriptor_index: usize) {
    let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
      ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
      Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
      Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
        Texture2D: D3D12_TEX2D_SRV {
          MostDetailedMip: 0,
          MipLevels: 1,
          PlaneSlice: 0,
          ResourceMinLODClamp: 0.0,
        },
      },
    };
    self
      .device
      .CreateShaderResourceView(texture, Some(&srv_desc), self.srv_heap.cpu_handle(descriptor_index));
  }

  #[cfg(feature = "raster")]
  unsafe fn write_nv12_image_srvs(
    &self,
    y_texture: &ID3D12Resource,
    uv_texture: &ID3D12Resource,
    descriptor_index: usize,
  ) {
    let y_srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: DXGI_FORMAT_R8_UNORM,
      ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
      Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
      Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
        Texture2D: D3D12_TEX2D_SRV {
          MostDetailedMip: 0,
          MipLevels: 1,
          PlaneSlice: 0,
          ResourceMinLODClamp: 0.0,
        },
      },
    };
    let uv_srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: DXGI_FORMAT_R8G8_UNORM,
      ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
      Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
      Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
        Texture2D: D3D12_TEX2D_SRV {
          MostDetailedMip: 0,
          MipLevels: 1,
          PlaneSlice: 0,
          ResourceMinLODClamp: 0.0,
        },
      },
    };
    self
      .device
      .CreateShaderResourceView(y_texture, Some(&y_srv_desc), self.srv_heap.cpu_handle(descriptor_index));
    self.device.CreateShaderResourceView(
      uv_texture,
      Some(&uv_srv_desc),
      self.srv_heap.cpu_handle(descriptor_index + 1),
    );
  }

  #[cfg(feature = "raster")]
  unsafe fn write_native_nv12_image_srvs(&self, dx12: &crate::images::Dx12Nv12Image, descriptor_index: usize) {
    let y_srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: DXGI_FORMAT_R8_UNORM,
      ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
      Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
      Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
        Texture2D: D3D12_TEX2D_SRV {
          MostDetailedMip: 0,
          MipLevels: 1,
          PlaneSlice: dx12.y_plane_slice,
          ResourceMinLODClamp: 0.0,
        },
      },
    };
    let uv_srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: DXGI_FORMAT_R8G8_UNORM,
      ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
      Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
      Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
        Texture2D: D3D12_TEX2D_SRV {
          MostDetailedMip: 0,
          MipLevels: 1,
          PlaneSlice: dx12.uv_plane_slice,
          ResourceMinLODClamp: 0.0,
        },
      },
    };
    self.device.CreateShaderResourceView(
      &dx12.y_texture,
      Some(&y_srv_desc),
      self.srv_heap.cpu_handle(descriptor_index),
    );
    self.device.CreateShaderResourceView(
      &dx12.uv_texture,
      Some(&uv_srv_desc),
      self.srv_heap.cpu_handle(descriptor_index + 1),
    );
  }

  #[cfg(feature = "raster")]
  unsafe fn ensure_image_texture(&mut self, image: &crate::images::ImageCmd) -> Result<usize> {
    if image.native.is_some() {
      return self.ensure_native_image_texture(image);
    }

    if let Some(cached) = self.image_textures.get(&image.image_id) {
      match cached {
        CachedImageTexture::Rgba {
          _texture,
          descriptor_index,
          width,
          height,
          frame_index,
          version,
        } if image.image_format == crate::images::ImagePixelFormat::Rgba8
          && *width == image.image_width
          && *height == image.image_height =>
        {
          let cached_descriptor_index = *descriptor_index;
          let frame_index = *frame_index;
          let version = *version;
          let texture = _texture.clone();
          if frame_index != image.frame_index || version != image.version {
            dx12_context(
              self.upload_image_texture(&texture, image, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE),
              format_args!(
                "update cached rgba image texture id={} size={}x{}",
                image.image_id, image.image_width, image.image_height
              ),
            )?;
            if let Some(CachedImageTexture::Rgba {
              frame_index, version, ..
            }) = self.image_textures.get_mut(&image.image_id)
            {
              *frame_index = image.frame_index;
              *version = image.version;
            }
          }

          let descriptor_index = match cached_descriptor_index {
            Some(descriptor_index) => descriptor_index,
            None => {
              let descriptor_index = self.allocate_frame_image_descriptors(image.image_id, 1)?;
              self.write_rgba_image_srv(&texture, descriptor_index);
              if let Some(cached) = self.image_textures.get_mut(&image.image_id) {
                cached.set_descriptor_index(descriptor_index);
              }
              descriptor_index
            }
          };
          return Ok(descriptor_index);
        }
        CachedImageTexture::Nv12 {
          _y_texture,
          _uv_texture,
          descriptor_index,
          width,
          height,
          frame_index,
          version,
        } if image.image_format == crate::images::ImagePixelFormat::Nv12
          && *width == image.image_width
          && *height == image.image_height =>
        {
          let cached_descriptor_index = *descriptor_index;
          let frame_index = *frame_index;
          let version = *version;
          let y_texture = _y_texture.clone();
          let uv_texture = _uv_texture.clone();
          if frame_index != image.frame_index || version != image.version {
            dx12_context(
              self.upload_nv12_image_textures(
                &y_texture,
                &uv_texture,
                image,
                D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
              ),
              format_args!(
                "update cached nv12 image texture id={} size={}x{}",
                image.image_id, image.image_width, image.image_height
              ),
            )?;
            if let Some(CachedImageTexture::Nv12 {
              frame_index, version, ..
            }) = self.image_textures.get_mut(&image.image_id)
            {
              *frame_index = image.frame_index;
              *version = image.version;
            }
          }

          let descriptor_index = match cached_descriptor_index {
            Some(descriptor_index) => descriptor_index,
            None => {
              let descriptor_index = self.allocate_frame_image_descriptors(image.image_id, 2)?;
              self.write_nv12_image_srvs(&y_texture, &uv_texture, descriptor_index);
              if let Some(cached) = self.image_textures.get_mut(&image.image_id) {
                cached.set_descriptor_index(descriptor_index);
              }
              descriptor_index
            }
          };
          return Ok(descriptor_index);
        }
        _ => {
          self.image_textures.remove(&image.image_id);
        }
      }
    }

    let descriptor_index = match image.image_format {
      crate::images::ImagePixelFormat::Rgba8 => {
        let texture = dx12_context(
          create_rgba_texture(&self.device, image.image_width, image.image_height),
          format_args!(
            "create rgba image texture id={} size={}x{}",
            image.image_id, image.image_width, image.image_height
          ),
        )?;
        dx12_context(
          self.upload_image_texture(&texture, image, D3D12_RESOURCE_STATE_COPY_DEST),
          format_args!(
            "upload rgba image texture id={} size={}x{}",
            image.image_id, image.image_width, image.image_height
          ),
        )?;

        let descriptor_index = self.allocate_frame_image_descriptors(image.image_id, 1)?;
        self.write_rgba_image_srv(&texture, descriptor_index);
        self.image_textures.insert(
          image.image_id,
          CachedImageTexture::Rgba {
            _texture: texture,
            descriptor_index: Some(descriptor_index),
            width: image.image_width,
            height: image.image_height,
            frame_index: image.frame_index,
            version: image.version,
          },
        );
        descriptor_index
      }
      crate::images::ImagePixelFormat::Nv12 => {
        if image.image_width % 2 != 0 || image.image_height % 2 != 0 {
          return Err(dx12_invalid_arg(format!(
            "dx12 nv12 image dimensions must be even: image id={} size={}x{}",
            image.image_id, image.image_width, image.image_height
          )));
        }
        let y_texture = dx12_context(
          create_r8_texture(&self.device, image.image_width, image.image_height),
          format_args!(
            "create nv12 y image texture id={} size={}x{}",
            image.image_id, image.image_width, image.image_height
          ),
        )?;
        let uv_texture = dx12_context(
          create_r8g8_texture(&self.device, image.image_width / 2, image.image_height / 2),
          format_args!(
            "create nv12 uv image texture id={} size={}x{}",
            image.image_id,
            image.image_width / 2,
            image.image_height / 2
          ),
        )?;
        dx12_context(
          self.upload_nv12_image_textures(&y_texture, &uv_texture, image, D3D12_RESOURCE_STATE_COPY_DEST),
          format_args!(
            "upload nv12 image texture id={} size={}x{}",
            image.image_id, image.image_width, image.image_height
          ),
        )?;

        let descriptor_index = self.allocate_frame_image_descriptors(image.image_id, 2)?;
        self.write_nv12_image_srvs(&y_texture, &uv_texture, descriptor_index);
        self.image_textures.insert(
          image.image_id,
          CachedImageTexture::Nv12 {
            _y_texture: y_texture,
            _uv_texture: uv_texture,
            descriptor_index: Some(descriptor_index),
            width: image.image_width,
            height: image.image_height,
            frame_index: image.frame_index,
            version: image.version,
          },
        );
        descriptor_index
      }
    };
    Ok(descriptor_index)
  }

  #[cfg(feature = "raster")]
  unsafe fn ensure_native_image_texture(&mut self, image: &crate::images::ImageCmd) -> Result<usize> {
    let Some(native) = &image.native else {
      return Err(dx12_invalid_arg(format!(
        "dx12 native image command missing native payload: image id={}",
        image.image_id
      )));
    };
    match native.backend() {
      #[cfg(feature = "canvas")]
      crate::images::NativeImageBackend::Canvas => {
        return Err(dx12_invalid_arg("canvas backing unavailable".to_owned()));
      }
      crate::images::NativeImageBackend::Dx12Nv12 => {}
      #[cfg(feature = "wgpu")]
      crate::images::NativeImageBackend::WgpuExternalRgba => {
        return Err(dx12_invalid_arg(format!(
          "dx12 renderer cannot sample a wgpu external image: image id={}",
          image.image_id
        )));
      }
    }
    if image.image_format != crate::images::ImagePixelFormat::Nv12
      || image.image_width % 2 != 0
      || image.image_height % 2 != 0
    {
      return Err(dx12_invalid_arg(format!(
        "dx12 native image must be even-sized nv12: image id={} format={:?} size={}x{}",
        image.image_id, image.image_format, image.image_width, image.image_height
      )));
    }

    if let Some(cached) = self.image_textures.get(&image.image_id) {
      match cached {
        CachedImageTexture::NativeNv12 {
          descriptor_indices,
          descriptor_versions,
          descriptor_stream_frames: _,
          width,
          height,
          version,
          ..
        } if *width == image.image_width && *height == image.image_height => {
          let frame_index = self.frame_index;
          let cached_descriptor_index = descriptor_indices[frame_index];
          let cached_descriptor_version = descriptor_versions[frame_index];
          let version = *version;
          let Some(dx12) = native_dx12_nv12_image(native) else {
            return Err(dx12_invalid_arg(format!(
              "dx12 native image payload type mismatch: image id={}",
              image.image_id
            )));
          };

          if version != image.version {
            dx12_native_image_log(format_args!(
              "refresh SRV image={} version={} previous_version={} descriptor={:?} y_plane={} uv_plane={}",
              image.image_id, image.version, version, cached_descriptor_index, dx12.y_plane_slice, dx12.uv_plane_slice
            ));
            if let Some(CachedImageTexture::NativeNv12 {
              _y_texture,
              _uv_texture,
              y_keyed_mutex,
              uv_keyed_mutex,
              version,
              ..
            }) = self.image_textures.get_mut(&image.image_id)
            {
              *_y_texture = dx12.y_texture.clone();
              *_uv_texture = dx12.uv_texture.clone();
              let packed_nv12 = dx12.y_plane_slice == 0 && dx12.uv_plane_slice == 1;
              *y_keyed_mutex = if packed_nv12 {
                dx12.y_texture.cast::<IDXGIKeyedMutex>().ok()
              } else {
                None
              };
              *uv_keyed_mutex = None;
              *version = image.version;
            }
          }

          let descriptor_index = match cached_descriptor_index {
            Some(descriptor_index) => {
              self.next_srv_index = self.next_srv_index.max(descriptor_index + 2);
              descriptor_index
            }
            None => {
              let descriptor_index = self.allocate_frame_image_descriptors(image.image_id, 2)?;
              if let Some(cached) = self.image_textures.get_mut(&image.image_id) {
                cached.set_native_descriptor_index(frame_index, descriptor_index, 0, None);
              }
              descriptor_index
            }
          };
          if cached_descriptor_version != image.version {
            self.write_native_nv12_image_srvs(&dx12, descriptor_index);
            if let Some(cached) = self.image_textures.get_mut(&image.image_id) {
              cached.set_native_descriptor_index(frame_index, descriptor_index, image.version, dx12.frame_number);
            }
          }
          return Ok(descriptor_index);
        }
        _ => {
          self.image_textures.remove(&image.image_id);
        }
      }
    }

    let Some(dx12) = native_dx12_nv12_image(native) else {
      return Err(dx12_invalid_arg(format!(
        "dx12 native image payload type mismatch: image id={}",
        image.image_id
      )));
    };

    let descriptor_index = self.allocate_frame_image_descriptors(image.image_id, 2)?;
    dx12_native_image_log(format_args!(
      "create SRV image={} version={} descriptor={} size={}x{} y_plane={} uv_plane={}",
      image.image_id,
      image.version,
      descriptor_index,
      image.image_width,
      image.image_height,
      dx12.y_plane_slice,
      dx12.uv_plane_slice
    ));
    self.write_native_nv12_image_srvs(&dx12, descriptor_index);
    let packed_nv12 = dx12.y_plane_slice == 0 && dx12.uv_plane_slice == 1;
    let y_keyed_mutex = if packed_nv12 {
      dx12.y_texture.cast::<IDXGIKeyedMutex>().ok()
    } else {
      None
    };
    let uv_keyed_mutex = None;
    let mut descriptor_indices = [None; FRAME_COUNT];
    descriptor_indices[self.frame_index] = Some(descriptor_index);
    let mut descriptor_versions = [0; FRAME_COUNT];
    descriptor_versions[self.frame_index] = image.version;
    let mut descriptor_stream_frames = [None; FRAME_COUNT];
    descriptor_stream_frames[self.frame_index] = dx12.frame_number;
    self.image_textures.insert(
      image.image_id,
      CachedImageTexture::NativeNv12 {
        _y_texture: dx12.y_texture.clone(),
        _uv_texture: dx12.uv_texture.clone(),
        y_keyed_mutex,
        uv_keyed_mutex,
        descriptor_indices,
        descriptor_versions,
        descriptor_stream_frames,
        width: image.image_width,
        height: image.image_height,
        version: image.version,
      },
    );
    Ok(descriptor_index)
  }

  #[cfg(feature = "raster")]
  unsafe fn upload_image_texture(
    &mut self,
    texture: &ID3D12Resource,
    image: &crate::images::ImageCmd,
    before_state: D3D12_RESOURCE_STATES,
  ) -> Result<()> {
    self.upload_texture_rows(
      texture,
      image.data.as_slice(),
      image.image_width,
      image.image_height,
      image.image_width as usize * 4,
      DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
      before_state,
    )
  }

  #[cfg(feature = "raster")]
  unsafe fn upload_nv12_image_textures(
    &mut self,
    y_texture: &ID3D12Resource,
    uv_texture: &ID3D12Resource,
    image: &crate::images::ImageCmd,
    before_state: D3D12_RESOURCE_STATES,
  ) -> Result<()> {
    let y_len = image.image_width as usize * image.image_height as usize;
    let uv_len = y_len / 2;
    if image.data.len() < y_len + uv_len {
      return Err(Error::from_win32());
    }
    self.upload_texture_rows(
      y_texture,
      &image.data[..y_len],
      image.image_width,
      image.image_height,
      image.image_width as usize,
      DXGI_FORMAT_R8_UNORM,
      before_state,
    )?;
    self.upload_texture_rows(
      uv_texture,
      &image.data[y_len..y_len + uv_len],
      image.image_width / 2,
      image.image_height / 2,
      image.image_width as usize,
      DXGI_FORMAT_R8G8_UNORM,
      before_state,
    )
  }

  #[cfg(feature = "raster")]
  unsafe fn upload_texture_rows(
    &mut self,
    texture: &ID3D12Resource,
    data: &[u8],
    width: u32,
    height: u32,
    row_bytes: usize,
    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    before_state: D3D12_RESOURCE_STATES,
  ) -> Result<()> {
    if before_state != D3D12_RESOURCE_STATE_COPY_DEST {
      self.transition_resource(texture, before_state, D3D12_RESOURCE_STATE_COPY_DEST);
    }

    let row_pitch = align_up(row_bytes, 256);
    let upload = if row_pitch == row_bytes && data.len() >= row_bytes * height as usize {
      dx12_context(
        self.upload_frame_bytes(data, 512),
        format_args!(
          "stage image texture upload rows size={}x{} row_bytes={} row_pitch={}",
          width, height, row_bytes, row_pitch
        ),
      )?
    } else {
      dx12_context(
        self.upload_frame_rows(data, row_bytes, row_pitch, height as usize, 512),
        format_args!(
          "stage padded image texture upload rows size={}x{} row_bytes={} row_pitch={}",
          width, height, row_bytes, row_pitch
        ),
      )?
    };
    let mut dst = D3D12_TEXTURE_COPY_LOCATION {
      pResource: ManuallyDrop::new(Some(texture.to_owned())),
      Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
      Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    let mut src = D3D12_TEXTURE_COPY_LOCATION {
      pResource: ManuallyDrop::new(Some(upload.resource.clone())),
      Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
      Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
          Offset: upload.offset,
          Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
            Format: format,
            Width: width,
            Height: height,
            Depth: 1,
            RowPitch: row_pitch as u32,
          },
        },
      },
    };
    self.command_list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
    ManuallyDrop::drop(&mut dst.pResource);
    ManuallyDrop::drop(&mut src.pResource);
    self.transition_resource(
      texture,
      D3D12_RESOURCE_STATE_COPY_DEST,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );
    Ok(())
  }

  unsafe fn create_render_targets(&mut self) -> Result<()> {
    let rtv_desc = render_target_view_desc();
    for index in 0..FRAME_COUNT {
      let target: ID3D12Resource = self.swapchain.GetBuffer(index as u32)?;
      let handle = self.rtv_heap.cpu_handle(index);
      self.device.CreateRenderTargetView(&target, Some(&rtv_desc), handle);
      self.render_targets[index] = Some(target);
    }
    Ok(())
  }

  unsafe fn transition_resource(
    &self,
    resource: &ID3D12Resource,
    before: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
    after: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES,
  ) {
    let mut barrier = D3D12_RESOURCE_BARRIER {
      Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
      Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
      Anonymous: D3D12_RESOURCE_BARRIER_0 {
        Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
          pResource: ManuallyDrop::new(Some(resource.clone())),
          Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
          StateBefore: before,
          StateAfter: after,
        }),
      },
    };
    self.command_list.ResourceBarrier(std::slice::from_ref(&barrier));
    ManuallyDrop::drop(&mut (*barrier.Anonymous.Transition).pResource);
  }

  fn current_render_target(&self) -> ID3D12Resource {
    self.render_targets[self.frame_index]
      .as_ref()
      .expect("missing dx12 render target")
      .clone()
  }

  unsafe fn current_rtv_handle(&self) -> D3D12_CPU_DESCRIPTOR_HANDLE {
    self.rtv_heap.cpu_handle(self.frame_index)
  }

  unsafe fn signal_current_frame(&mut self) -> Result<()> {
    let fence_value = self.next_fence_value;
    self.next_fence_value += 1;
    self.command_queue.Signal(&self.fence, fence_value)?;
    self.fence_values[self.frame_index] = fence_value;
    Ok(())
  }

  unsafe fn reset_frame_command_allocator(&mut self) -> Result<()> {
    let allocator = self.command_allocators[self.frame_index].clone();
    if let Err(err) = allocator.Reset() {
      tracing::warn!("dx12 command allocator reset failed; waiting for gpu before retry: {err:?}");
      self.wait_for_gpu()?;
      dx12_context(allocator.Reset(), "reset dx12 command allocator after gpu wait")?;
    }
    Ok(())
  }

  unsafe fn wait_for_frame_latency(&mut self) -> Result<()> {
    if self.device_lost || self.frame_latency_waitable.is_invalid() {
      return Ok(());
    }

    let wait = WaitForSingleObject(self.frame_latency_waitable, DX12_FRAME_WAIT_TIMEOUT_MS);
    if wait == WAIT_TIMEOUT {
      return Err(dx12_frame_not_ready_error(
        "wait for dx12 frame latency",
        format_args!("swapchain was not ready after {DX12_FRAME_WAIT_TIMEOUT_MS}ms"),
      ));
    }
    debug_assert_eq!(wait, WAIT_OBJECT_0);
    Ok(())
  }

  unsafe fn wait_for_frame(&mut self, frame_index: usize) -> Result<()> {
    if self.device_lost {
      return Ok(());
    }

    let fence_value = self.fence_values[frame_index];
    let completed_value = self.fence.GetCompletedValue();
    if fence_value != 0 && completed_value < fence_value {
      if let Err(err) = self.fence.SetEventOnCompletion(fence_value, self.fence_event) {
        if is_dxgi_device_lost_code(err.code()) {
          self.device_lost = true;
        }
        return Err(err);
      }
      let wait = WaitForSingleObject(self.fence_event, DX12_FRAME_WAIT_TIMEOUT_MS);
      if wait == WAIT_TIMEOUT {
        return Err(dx12_frame_not_ready_error(
          "wait for dx12 frame",
          format_args!(
            "previous GPU work was still busy after {DX12_FRAME_WAIT_TIMEOUT_MS}ms; fence={fence_value} completed={completed_value}"
          ),
        ));
      }
      debug_assert_eq!(wait, WAIT_OBJECT_0);
    }
    Ok(())
  }

  unsafe fn wait_for_gpu(&mut self) -> Result<()> {
    if self.device_lost {
      return Ok(());
    }

    let fence_value = self.next_fence_value;
    self.next_fence_value += 1;
    if let Err(err) = self.command_queue.Signal(&self.fence, fence_value) {
      if is_dxgi_device_lost_code(err.code()) {
        self.device_lost = true;
      }
      return Err(err);
    }
    if let Err(err) = self.fence.SetEventOnCompletion(fence_value, self.fence_event) {
      if is_dxgi_device_lost_code(err.code()) {
        self.device_lost = true;
      }
      return Err(err);
    }
    let wait = WaitForSingleObject(self.fence_event, DX12_FRAME_WAIT_TIMEOUT_MS);
    if wait == WAIT_TIMEOUT {
      let reason = self.device_removed_reason_code();
      if is_dxgi_device_lost_code(reason) {
        self.device_lost = true;
        return Err(dx12_gpu_wait_timeout_error("wait for dx12 gpu idle", reason));
      }
      return Err(dx12_frame_not_ready_error(
        "wait for dx12 gpu idle",
        format_args!(
          "GPU queue was still busy after {DX12_FRAME_WAIT_TIMEOUT_MS}ms; fence={fence_value} removed_reason={}",
          dxgi_hresult_label(reason)
        ),
      ));
    }
    debug_assert_eq!(wait, WAIT_OBJECT_0);
    self.fence_values = [0; FRAME_COUNT];
    Ok(())
  }

  fn mark_device_lost(&mut self) {
    self.device_lost = true;
  }

  unsafe fn device_removed_reason_code(&self) -> HRESULT {
    match self.device.GetDeviceRemovedReason() {
      Ok(()) => HRESULT(0),
      Err(err) => err.code(),
    }
  }
}

impl Drop for Dx12State {
  fn drop(&mut self) {
    unsafe {
      if !self.device_lost {
        let _ = self.wait_for_gpu();
      }
      if !self.frame_latency_waitable.is_invalid() {
        let _ = CloseHandle(self.frame_latency_waitable);
      }
      let _ = CloseHandle(self.fence_event);
    }
  }
}

unsafe fn choose_adapter(factory: &IDXGIFactory4) -> Result<IDXGIAdapter1> {
  if let Some(adapter) = choose_preferred_adapter(factory) {
    return Ok(adapter);
  }

  let mut index = 0;
  loop {
    let adapter = match factory.EnumAdapters1(index) {
      Ok(adapter) => adapter,
      Err(err) => {
        if index == 0 {
          return Err(err);
        }
        break;
      }
    };
    let desc = adapter.GetDesc1()?;
    if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && create_device(&adapter).is_ok() {
      return Ok(adapter);
    }
    index += 1;
  }

  factory.EnumAdapters1(0)
}

unsafe fn choose_preferred_adapter(factory: &IDXGIFactory4) -> Option<IDXGIAdapter1> {
  let factory = factory.cast::<IDXGIFactory6>().ok()?;
  let mut index = 0;
  loop {
    let adapter = match factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(index, DXGI_GPU_PREFERENCE_UNSPECIFIED) {
      Ok(adapter) => adapter,
      Err(_) => return None,
    };
    let desc = adapter.GetDesc1().ok()?;
    if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && create_device(&adapter).is_ok() {
      return Some(adapter);
    }
    index += 1;
  }
}

unsafe fn create_device(adapter: &IDXGIAdapter1) -> Result<ID3D12Device> {
  let mut device = None;
  D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device)?;
  device.ok_or_else(Error::from_win32)
}

fn dxgi_factory_flags() -> DXGI_CREATE_FACTORY_FLAGS {
  if cfg!(debug_assertions) {
    DXGI_CREATE_FACTORY_DEBUG
  } else {
    DXGI_CREATE_FACTORY_FLAGS(0)
  }
}

unsafe fn create_factory() -> Result<IDXGIFactory4> {
  match CreateDXGIFactory2(dxgi_factory_flags()) {
    Ok(factory) => Ok(factory),
    Err(_) if cfg!(debug_assertions) => CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)),
    Err(err) => Err(err),
  }
}

fn hwnd_from_window(window: WindowHandle<'_>) -> Result<HWND> {
  match window.as_raw() {
    RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
    _ => Err(Error::from_win32()),
  }
}

fn offset_cpu_handle(
  handle: D3D12_CPU_DESCRIPTOR_HANDLE,
  descriptor_size: u32,
  index: usize,
) -> D3D12_CPU_DESCRIPTOR_HANDLE {
  D3D12_CPU_DESCRIPTOR_HANDLE {
    ptr: handle.ptr + descriptor_size as usize * index,
  }
}

fn offset_gpu_handle(
  handle: D3D12_GPU_DESCRIPTOR_HANDLE,
  descriptor_size: u32,
  index: usize,
) -> D3D12_GPU_DESCRIPTOR_HANDLE {
  D3D12_GPU_DESCRIPTOR_HANDLE {
    ptr: handle.ptr + descriptor_size as u64 * index as u64,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn upload_rows_clear_missing_texels_and_padding_without_overwriting_neighbors() {
    for (row_bytes, row_pitch, height) in [(12, 12, 3), (12, 256, 4), (1028, 1280, 3)] {
      for length in [
        0,
        1,
        row_bytes - 1,
        row_bytes + 3,
        row_bytes * height,
        row_bytes * height + 7,
      ] {
        let source = (0..length).map(|i| (i % 251 + 1) as u8).collect::<Vec<_>>();
        let mut memory = vec![0xa5; 17 + row_pitch * height + 19];
        unsafe {
          copy_upload_rows(&source, memory.as_mut_ptr().add(17), row_bytes, row_pitch, height);
        }
        let mut expected = Vec::new();
        for chunk in source.chunks(row_bytes).take(height) {
          let mut row = chunk.to_vec();
          row.resize(row_pitch, 0);
          expected.extend(row);
        }
        expected.resize(row_pitch * height, 0);
        assert_eq!(&memory[..17], &[0xa5; 17]);
        assert_eq!(&memory[17..17 + expected.len()], expected);
        assert_eq!(&memory[17 + expected.len()..], &[0xa5; 19]);
      }
    }
  }

  #[test]
  fn scissor_expands_fractional_clip_to_include_bottom_right_edge() {
    let rect = scissor_rect(
      ClipRect {
        x: 10.6,
        y: 20.2,
        width: 30.1,
        height: 40.6,
        active: true,
        border_radius: None,
      },
      100.0,
      100.0,
    )
    .unwrap();

    assert_eq!(rect.left, 10);
    assert_eq!(rect.top, 20);
    assert_eq!(rect.right, 41);
    assert_eq!(rect.bottom, 61);
  }

  #[cfg(feature = "screenshot")]
  #[test]
  fn dx12_engine_reports_frame_capture_support() {
    assert!(Dx12RenderEngine::new().supports_frame_capture());
  }

  #[test]
  fn dx12_hlsl_shaders_compile() {
    unsafe {
      compile_shader(include_bytes!("shaders/quad.hlsl"), b"vs_main\0", b"vs_5_0\0").unwrap();
      compile_shader(include_bytes!("shaders/quad.hlsl"), b"ps_main\0", b"ps_5_0\0").unwrap();
      compile_shader(include_bytes!("shaders/glyph.hlsl"), b"vs_main\0", b"vs_5_0\0").unwrap();
      compile_shader(include_bytes!("shaders/glyph.hlsl"), b"ps_main\0", b"ps_5_0\0").unwrap();
      compile_shader(include_bytes!("shaders/layer.hlsl"), b"vs_main\0", b"vs_5_0\0").unwrap();
      compile_shader(include_bytes!("shaders/layer.hlsl"), b"ps_main\0", b"ps_5_0\0").unwrap();
      #[cfg(feature = "raster")]
      {
        compile_shader(include_bytes!("shaders/image.hlsl"), b"vs_main\0", b"vs_5_0\0").unwrap();
        compile_shader(include_bytes!("shaders/image.hlsl"), b"ps_main\0", b"ps_5_0\0").unwrap();
        compile_shader(include_bytes!("shaders/image_nv12.hlsl"), b"vs_main\0", b"vs_5_0\0").unwrap();
        compile_shader(include_bytes!("shaders/image_nv12.hlsl"), b"ps_main\0", b"ps_5_0\0").unwrap();
      }
      #[cfg(feature = "svg")]
      {
        compile_shader(include_bytes!("shaders/svg.hlsl"), b"vs_main\0", b"vs_5_0\0").unwrap();
        compile_shader(include_bytes!("shaders/svg.hlsl"), b"ps_main\0", b"ps_5_0\0").unwrap();
      }
    }
  }
}
