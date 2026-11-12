#[cfg(feature = "image")]
use std::io::Cursor;
use std::{
  any::Any,
  sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use core_foundation_sys::base::{CFRelease, CFRetain};
#[cfg(target_os = "macos")]
use core_video_sys::pixel_buffer::CVPixelBufferRef;
#[cfg(feature = "image")]
use image::{AnimationDecoder, ImageFormat, codecs};
use parking_lot::{Mutex, RwLock};

pub enum ImageKind {
  Bytes(ImageData),
  Native(NativeImageData),
  #[cfg(all(feature = "image", feature = "resources"))]
  Resource(Arc<str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImagePixelFormat {
  Rgba8,
  Nv12,
}

#[cfg(all(feature = "image", feature = "resources"))]
impl From<&str> for ImageKind {
  fn from(value: &str) -> Self {
    Self::Resource(Arc::from(value))
  }
}

#[cfg(all(feature = "image", feature = "resources"))]
impl From<String> for ImageKind {
  fn from(value: String) -> Self {
    Self::Resource(Arc::from(value))
  }
}

#[cfg(all(feature = "image", feature = "resources"))]
impl From<Arc<str>> for ImageKind {
  fn from(value: Arc<str>) -> Self {
    Self::Resource(value)
  }
}

impl From<ImageData> for ImageKind {
  fn from(value: ImageData) -> Self {
    Self::Bytes(value)
  }
}

impl From<NativeImageData> for ImageKind {
  fn from(value: NativeImageData) -> Self {
    Self::Native(value)
  }
}

static NEXT_IMAGE_ID: AtomicU64 = AtomicU64::new(1);
static ANIMATION_EPOCH: OnceLock<Instant> = OnceLock::new();
#[cfg(feature = "image")]
const MIN_ANIMATION_FRAME_MS: u64 = 10;
const MAX_STREAMING_RECYCLED_BUFFERS: usize = 3;

fn animation_epoch() -> Instant {
  *ANIMATION_EPOCH.get_or_init(Instant::now)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeImageBackend {
  #[cfg(feature = "canvas")]
  Canvas,
  Dx12Nv12,
  #[cfg(feature = "wgpu")]
  WgpuExternalRgba,
  #[cfg(target_os = "macos")]
  MacosCvPixelBufferNv12,
}

#[derive(Clone)]
pub struct NativeImageData {
  id: u64,
  width: u32,
  height: u32,
  format: ImagePixelFormat,
  backend: NativeImageBackend,
  payload: Arc<dyn Any + Send + Sync>,
  version: Arc<AtomicU64>,
  continuous_redraw: bool,
}

#[cfg(all(feature = "dx12", target_os = "windows"))]
#[derive(Clone)]
pub struct Dx12Nv12Image {
  pub y_texture: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
  pub uv_texture: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
  pub y_plane_slice: u32,
  pub uv_plane_slice: u32,
  pub frame_number: Option<u32>,
}

#[cfg(all(feature = "dx12", target_os = "windows"))]
#[derive(Clone)]
pub struct Dx12Nv12ImageSlot {
  state: Arc<RwLock<Dx12Nv12ImageSlotState>>,
}

#[cfg(all(feature = "dx12", target_os = "windows"))]
struct Dx12Nv12ImageSlotState {
  image: Dx12Nv12Image,
  version: u64,
  pending_external_bump: bool,
}

#[cfg(target_os = "macos")]
pub struct MacosCvPixelBuffer {
  ptr: CVPixelBufferRef,
}

#[cfg(target_os = "macos")]
unsafe impl Send for MacosCvPixelBuffer {}

#[cfg(target_os = "macos")]
unsafe impl Sync for MacosCvPixelBuffer {}

#[cfg(target_os = "macos")]
impl MacosCvPixelBuffer {
  /// # Safety
  ///
  /// `ptr` must be a valid `CVPixelBufferRef`. This function retains it, and
  /// the wrapper releases it when dropped.
  pub unsafe fn retain(ptr: CVPixelBufferRef) -> Self {
    assert!(!ptr.is_null());
    unsafe {
      CFRetain(ptr.cast());
    }
    Self { ptr }
  }

  pub fn as_ptr(&self) -> CVPixelBufferRef {
    self.ptr
  }
}

#[cfg(target_os = "macos")]
impl Clone for MacosCvPixelBuffer {
  fn clone(&self) -> Self {
    unsafe { Self::retain(self.ptr) }
  }
}

#[cfg(target_os = "macos")]
impl Drop for MacosCvPixelBuffer {
  fn drop(&mut self) {
    unsafe {
      CFRelease(self.ptr.cast());
    }
  }
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct MacosCvPixelBufferNv12Image {
  pub pixel_buffer: MacosCvPixelBuffer,
}

#[derive(Clone)]
pub struct ImageData {
  id: u64,
  width: u32,
  height: u32,
  format: ImagePixelFormat,
  frames: Arc<Vec<ImageFrame>>,
  animation_frame_data: Option<Arc<Vec<Arc<Vec<u8>>>>>,
  total_duration_ms: u64,
  streaming: Option<Arc<StreamingImageInner>>,
  native: Option<NativeImageData>,
}

#[derive(Clone)]
struct ImageFrame {
  data: Arc<Vec<u8>>,
  duration_ms: u64,
}

struct StreamingImageInner {
  data: RwLock<Arc<Vec<u8>>>,
  recycled: Mutex<Vec<Arc<Vec<u8>>>>,
  version: AtomicU64,
  continuous_redraw: AtomicBool,
}

#[derive(Clone)]
pub struct StreamingImage {
  image: ImageData,
}

pub(crate) struct ImageFrameData {
  pub data: Arc<Vec<u8>>,
  pub animation_frames: Option<Arc<Vec<Arc<Vec<u8>>>>>,
  pub native: Option<NativeImageData>,
  pub width: u32,
  pub height: u32,
  pub format: ImagePixelFormat,
  pub frame_index: usize,
  pub version: u64,
  pub next_frame_at: Option<Instant>,
}

impl ImageData {
  pub fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(data.len(), (width * height * 4) as usize);
    Self {
      id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed),
      width,
      height,
      format: ImagePixelFormat::Rgba8,
      frames: Arc::new(vec![ImageFrame {
        data: Arc::new(data),
        duration_ms: 0,
      }]),
      animation_frame_data: None,
      total_duration_ms: 0,
      streaming: None,
      native: None,
    }
  }

  pub fn streaming_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(data.len(), (width * height * 4) as usize);
    Self::streaming(data, width, height, ImagePixelFormat::Rgba8, true)
  }

  pub fn streaming_nv12(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(data.len(), nv12_len(width, height));
    Self::streaming(data, width, height, ImagePixelFormat::Nv12, true)
  }

  fn streaming(data: Vec<u8>, width: u32, height: u32, format: ImagePixelFormat, continuous_redraw: bool) -> Self {
    let data = Arc::new(data);
    Self {
      id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed),
      width,
      height,
      format,
      frames: Arc::new(Vec::new()),
      animation_frame_data: None,
      total_duration_ms: 0,
      streaming: Some(Arc::new(StreamingImageInner {
        data: RwLock::new(data),
        recycled: Mutex::new(Vec::new()),
        version: AtomicU64::new(0),
        continuous_redraw: AtomicBool::new(continuous_redraw),
      })),
      native: None,
    }
  }

  #[cfg(feature = "image")]
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
    match image::guess_format(bytes)? {
      ImageFormat::Gif => Self::from_gif_bytes(bytes),
      ImageFormat::WebP => Self::from_webp_bytes(bytes),
      _ => Self::from_static_bytes(bytes),
    }
  }

  #[cfg(feature = "image")]
  fn from_static_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
    let img = image::load_from_memory(bytes)?.into_rgba8();
    let width = img.width();
    let height = img.height();
    Ok(Self::from_rgba(img.into_raw(), width, height))
  }

  #[cfg(feature = "image")]
  fn from_gif_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
    let decoder = codecs::gif::GifDecoder::new(Cursor::new(bytes))?;
    let frames = decoder.into_frames().collect_frames()?;
    Self::from_animation_frames(frames).or_else(|_| Self::from_static_bytes(bytes))
  }

  #[cfg(feature = "image")]
  fn from_webp_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
    let decoder = codecs::webp::WebPDecoder::new(Cursor::new(bytes))?;
    if !decoder.has_animation() {
      return Self::from_static_bytes(bytes);
    }
    let frames = decoder.into_frames().collect_frames()?;
    Self::from_animation_frames(frames).or_else(|_| Self::from_static_bytes(bytes))
  }

  #[cfg(feature = "image")]
  fn from_animation_frames(frames: Vec<image::Frame>) -> Result<Self, image::ImageError> {
    let Some(first) = frames.first() else {
      return Err(image::ImageError::Decoding(image::error::DecodingError::new(
        image::error::ImageFormatHint::Unknown,
        "animated image had no frames",
      )));
    };

    let width = first.buffer().width();
    let height = first.buffer().height();
    if frames.len() <= 1 {
      return Ok(Self::from_rgba(first.buffer().clone().into_raw(), width, height));
    }

    let mut total_duration_ms = 0_u64;
    let frames: Vec<_> = frames
      .into_iter()
      .map(|frame| {
        let duration_ms = delay_ms(frame.delay());
        total_duration_ms += duration_ms;
        ImageFrame {
          data: Arc::new(frame.into_buffer().into_raw()),
          duration_ms,
        }
      })
      .collect();
    let animation_frame_data = Arc::new(frames.iter().map(|frame| Arc::clone(&frame.data)).collect());

    Ok(Self {
      id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed),
      width,
      height,
      format: ImagePixelFormat::Rgba8,
      frames: Arc::new(frames),
      animation_frame_data: Some(animation_frame_data),
      total_duration_ms,
      streaming: None,
      native: None,
    })
  }

  #[cfg(feature = "image")]
  pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, image::ImageError> {
    let bytes = std::fs::read(path).map_err(image::ImageError::IoError)?;
    Self::from_bytes(&bytes)
  }

  #[cfg(feature = "canvas")]
  pub(crate) fn canvas_compatible(&self) -> bool {
    self.format == ImagePixelFormat::Rgba8
      && self.streaming.is_none()
      && self.native.is_none()
      && self.frames.len() == 1
      && self.width > 0
      && self.height > 0
  }

  pub fn id(&self) -> u64 {
    self.id
  }

  pub fn data(&self) -> &[u8] {
    assert!(
      self.streaming.is_none() && self.native.is_none(),
      "streaming images expose their latest pixels through data_arc()"
    );
    &self.frames[0].data
  }

  pub fn data_arc(&self) -> Arc<Vec<u8>> {
    if self.native.is_some() {
      return Arc::new(Vec::new());
    }
    if let Some(streaming) = &self.streaming {
      return Arc::clone(&streaming.data.read());
    }
    Arc::clone(&self.frames[0].data)
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  pub fn format(&self) -> ImagePixelFormat {
    self.format
  }

  pub fn is_animated(&self) -> bool {
    self.frames.len() > 1 && self.total_duration_ms > 0
  }

  pub fn is_streaming(&self) -> bool {
    self.streaming.is_some()
  }

  pub fn is_native(&self) -> bool {
    self.native.is_some()
  }

  pub fn requires_continuous_redraw(&self) -> bool {
    self.is_animated()
      || self
        .streaming
        .as_ref()
        .is_some_and(|streaming| streaming.continuous_redraw.load(Ordering::Acquire))
      || self
        .native
        .as_ref()
        .is_some_and(NativeImageData::requires_continuous_redraw)
  }

  pub fn version(&self) -> u64 {
    #[cfg(all(feature = "dx12", target_os = "windows"))]
    if let Some(slot) = self
      .native
      .as_ref()
      .and_then(|native| native.payload::<Dx12Nv12ImageSlot>())
    {
      return slot.version();
    }

    self.streaming.as_ref().map_or_else(
      || self.native.as_ref().map_or(0, NativeImageData::version),
      |streaming| streaming.version.load(Ordering::Acquire),
    )
  }

  pub fn set_streaming_rgba(&self, data: Vec<u8>) {
    assert_eq!(self.format, ImagePixelFormat::Rgba8);
    assert_eq!(data.len(), (self.width * self.height * 4) as usize);
    self.set_streaming_data(data);
  }

  pub fn set_streaming_nv12(&self, data: Vec<u8>) {
    assert_eq!(self.format, ImagePixelFormat::Nv12);
    assert_eq!(data.len(), nv12_len(self.width, self.height));
    self.set_streaming_data(data);
  }

  fn set_streaming_data(&self, data: Vec<u8>) {
    let Some(streaming) = &self.streaming else {
      panic!("set_streaming_data requires a streaming ImageData");
    };
    let old = std::mem::replace(&mut *streaming.data.write(), Arc::new(data));
    let mut recycled = streaming.recycled.lock();
    recycled.push(old);
    if recycled.len() > MAX_STREAMING_RECYCLED_BUFFERS {
      recycled.remove(0);
    }
    streaming.version.fetch_add(1, Ordering::Release);
  }

  pub fn take_streaming_rgba_buffer(&self) -> Option<Vec<u8>> {
    assert_eq!(self.format, ImagePixelFormat::Rgba8);
    self.take_streaming_buffer((self.width * self.height * 4) as usize)
  }

  pub fn take_streaming_nv12_buffer(&self) -> Option<Vec<u8>> {
    assert_eq!(self.format, ImagePixelFormat::Nv12);
    self.take_streaming_buffer(nv12_len(self.width, self.height))
  }

  fn take_streaming_buffer(&self, expected_len: usize) -> Option<Vec<u8>> {
    let Some(streaming) = &self.streaming else {
      panic!("take_streaming_buffer requires a streaming ImageData");
    };
    let mut recycled = streaming.recycled.lock();
    let mut pending = Vec::new();
    while let Some(candidate) = recycled.pop() {
      match Arc::try_unwrap(candidate) {
        Ok(mut data) if data.capacity() >= expected_len => {
          data.clear();
          recycled.extend(pending);
          return Some(data);
        }
        Ok(_) => {}
        Err(candidate) => {
          pending.push(candidate);
        }
      }
    }
    recycled.extend(pending);
    None
  }

  pub fn update_streaming_rgba(&self, update: impl FnOnce(&mut [u8])) {
    let Some(streaming) = &self.streaming else {
      panic!("update_streaming_rgba requires ImageData::streaming_rgba");
    };
    let expected_len = (self.width * self.height * 4) as usize;

    {
      let mut current = streaming.data.write();
      if let Some(data) = Arc::get_mut(&mut *current) {
        assert_eq!(data.len(), expected_len);
        update(data);
        streaming.version.fetch_add(1, Ordering::Release);
        return;
      }
    }

    let current = Arc::clone(&streaming.data.read());
    assert_eq!(current.len(), expected_len);
    let mut data = self
      .take_streaming_rgba_buffer()
      .unwrap_or_else(|| Vec::with_capacity(expected_len));
    data.clear();
    data.extend_from_slice(&current);
    update(&mut data);
    self.set_streaming_rgba(data);
  }

  pub(crate) fn frame_at(&self, now: Instant) -> ImageFrameData {
    if let Some(streaming) = &self.streaming {
      return ImageFrameData {
        data: Arc::clone(&streaming.data.read()),
        animation_frames: None,
        native: None,
        width: self.width,
        height: self.height,
        format: self.format,
        frame_index: 0,
        version: streaming.version.load(Ordering::Acquire),
        next_frame_at: streaming.continuous_redraw.load(Ordering::Acquire).then_some(now),
      };
    }

    if let Some(native) = &self.native {
      #[cfg(all(feature = "dx12", target_os = "windows"))]
      if let Some((native, version)) = native.dx12_nv12_slot_snapshot() {
        let next_frame_at = native.requires_continuous_redraw().then_some(now);
        return ImageFrameData {
          data: Arc::new(Vec::new()),
          animation_frames: None,
          native: Some(native),
          width: self.width,
          height: self.height,
          format: self.format,
          frame_index: 0,
          version,
          next_frame_at,
        };
      }

      return ImageFrameData {
        data: Arc::new(Vec::new()),
        animation_frames: None,
        native: Some(native.clone()),
        width: self.width,
        height: self.height,
        format: self.format,
        frame_index: 0,
        version: native.version(),
        next_frame_at: native.requires_continuous_redraw().then_some(now),
      };
    }

    let mut next_frame_at = None;
    let frame_index = if self.is_animated() {
      let elapsed_ms = now
        .saturating_duration_since(animation_epoch())
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
      let position_ms = elapsed_ms % self.total_duration_ms;
      let mut cursor_ms = 0_u64;
      let frame_index = self
        .frames
        .iter()
        .position(|frame| {
          cursor_ms += frame.duration_ms;
          position_ms < cursor_ms
        })
        .unwrap_or(0);
      let remaining_ms = cursor_ms.saturating_sub(position_ms);
      next_frame_at = now.checked_add(Duration::from_millis(remaining_ms));
      frame_index
    } else {
      0
    };
    let frame = &self.frames[frame_index];

    ImageFrameData {
      data: Arc::clone(&frame.data),
      animation_frames: self.animation_frame_data.clone(),
      native: None,
      width: self.width,
      height: self.height,
      format: self.format,
      frame_index,
      version: frame_index as u64,
      next_frame_at,
    }
  }
}

impl NativeImageData {
  pub fn new<T: Any + Send + Sync>(
    width: u32,
    height: u32,
    format: ImagePixelFormat,
    backend: NativeImageBackend,
    payload: T,
  ) -> Self {
    Self::new_with_continuous_redraw(width, height, format, backend, payload, false)
  }

  pub fn new_with_continuous_redraw<T: Any + Send + Sync>(
    width: u32,
    height: u32,
    format: ImagePixelFormat,
    backend: NativeImageBackend,
    payload: T,
    continuous_redraw: bool,
  ) -> Self {
    Self {
      id: NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed),
      width,
      height,
      format,
      backend,
      payload: Arc::new(payload),
      version: Arc::new(AtomicU64::new(0)),
      continuous_redraw,
    }
  }

  pub fn image_data(&self) -> ImageData {
    ImageData {
      id: self.id,
      width: self.width,
      height: self.height,
      format: self.format,
      frames: Arc::new(Vec::new()),
      animation_frame_data: None,
      total_duration_ms: 0,
      streaming: None,
      native: Some(self.clone()),
    }
  }

  pub fn id(&self) -> u64 {
    self.id
  }

  pub fn width(&self) -> u32 {
    self.width
  }

  pub fn height(&self) -> u32 {
    self.height
  }

  pub fn format(&self) -> ImagePixelFormat {
    self.format
  }

  pub fn backend(&self) -> NativeImageBackend {
    self.backend
  }

  pub fn payload<T: Any + Send + Sync>(&self) -> Option<&T> {
    self.payload.downcast_ref()
  }

  pub fn version(&self) -> u64 {
    #[cfg(feature = "wgpu")]
    if let Some(state) = self.payload::<super::WgpuExternalImageState>() {
      return state.version();
    }

    #[cfg(all(feature = "dx12", target_os = "windows"))]
    if let Some(slot) = self.payload::<Dx12Nv12ImageSlot>() {
      return slot.version();
    }

    self.version.load(Ordering::Acquire)
  }

  pub fn bump_version(&self) {
    #[cfg(feature = "wgpu")]
    if self.payload::<super::WgpuExternalImageState>().is_some() {
      return;
    }

    #[cfg(all(feature = "dx12", target_os = "windows"))]
    if let Some(slot) = self.payload::<Dx12Nv12ImageSlot>() {
      slot.bump_version();
      return;
    }

    self.version.fetch_add(1, Ordering::Release);
  }

  pub fn requires_continuous_redraw(&self) -> bool {
    self.continuous_redraw
  }

  #[cfg(all(feature = "dx12", target_os = "windows"))]
  fn dx12_nv12_slot_snapshot(&self) -> Option<(Self, u64)> {
    let slot = self.payload::<Dx12Nv12ImageSlot>()?;
    let (image, version) = slot.snapshot();
    Some((
      Self {
        id: self.id,
        width: self.width,
        height: self.height,
        format: self.format,
        backend: self.backend,
        payload: Arc::new(image),
        version: Arc::new(AtomicU64::new(version)),
        continuous_redraw: self.continuous_redraw,
      },
      version,
    ))
  }
}

#[cfg(all(feature = "dx12", target_os = "windows"))]
impl Dx12Nv12ImageSlot {
  pub fn new(image: Dx12Nv12Image) -> Self {
    Self {
      state: Arc::new(RwLock::new(Dx12Nv12ImageSlotState {
        image,
        version: 0,
        pending_external_bump: false,
      })),
    }
  }

  pub fn image(&self) -> Dx12Nv12Image {
    self.state.read().image.clone()
  }

  pub fn snapshot(&self) -> (Dx12Nv12Image, u64) {
    let state = self.state.read();
    (state.image.clone(), state.version)
  }

  pub fn set_image(&self, image: Dx12Nv12Image) -> u64 {
    let mut state = self.state.write();
    state.version = state.version.wrapping_add(1);
    state.image = image;
    state.pending_external_bump = true;
    state.version
  }

  pub fn version(&self) -> u64 {
    self.state.read().version
  }

  fn bump_version(&self) -> u64 {
    let mut state = self.state.write();
    if state.pending_external_bump {
      state.pending_external_bump = false;
      return state.version;
    }
    state.version = state.version.wrapping_add(1);
    state.version
  }
}

#[cfg(all(feature = "dx12", target_os = "windows"))]
impl NativeImageData {
  pub fn from_dx12_nv12(
    width: u32,
    height: u32,
    y_texture: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
    uv_texture: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
  ) -> Self {
    Self::new_with_continuous_redraw(
      width,
      height,
      ImagePixelFormat::Nv12,
      NativeImageBackend::Dx12Nv12,
      Dx12Nv12Image {
        y_texture,
        uv_texture,
        y_plane_slice: 0,
        uv_plane_slice: 0,
        frame_number: None,
      },
      true,
    )
  }

  pub fn from_dx12_nv12_texture(
    width: u32,
    height: u32,
    texture: windows::Win32::Graphics::Direct3D12::ID3D12Resource,
  ) -> Self {
    Self::new_with_continuous_redraw(
      width,
      height,
      ImagePixelFormat::Nv12,
      NativeImageBackend::Dx12Nv12,
      Dx12Nv12Image {
        y_texture: texture.clone(),
        uv_texture: texture,
        y_plane_slice: 0,
        uv_plane_slice: 1,
        frame_number: None,
      },
      true,
    )
  }

  pub fn from_dx12_nv12_slot(width: u32, height: u32, slot: Dx12Nv12ImageSlot) -> Self {
    Self::new_with_continuous_redraw(
      width,
      height,
      ImagePixelFormat::Nv12,
      NativeImageBackend::Dx12Nv12,
      slot,
      true,
    )
  }
}

#[cfg(target_os = "macos")]
impl NativeImageData {
  pub fn from_macos_cv_pixel_buffer_nv12(width: u32, height: u32, pixel_buffer: MacosCvPixelBuffer) -> Self {
    Self::new_with_continuous_redraw(
      width,
      height,
      ImagePixelFormat::Nv12,
      NativeImageBackend::MacosCvPixelBufferNv12,
      MacosCvPixelBufferNv12Image { pixel_buffer },
      true,
    )
  }
}

impl StreamingImage {
  pub fn new_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
    Self {
      image: ImageData::streaming_rgba(data, width, height),
    }
  }

  pub fn new_nv12(data: Vec<u8>, width: u32, height: u32) -> Self {
    Self {
      image: ImageData::streaming_nv12(data, width, height),
    }
  }

  pub fn new_rgba_manual_redraw(data: Vec<u8>, width: u32, height: u32) -> Self {
    Self {
      image: ImageData::streaming(data, width, height, ImagePixelFormat::Rgba8, false),
    }
  }

  pub fn new_nv12_manual_redraw(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(data.len(), nv12_len(width, height));
    Self {
      image: ImageData::streaming(data, width, height, ImagePixelFormat::Nv12, false),
    }
  }

  pub fn image_data(&self) -> ImageData {
    self.image.clone()
  }

  pub fn set_rgba(&self, data: Vec<u8>) {
    self.image.set_streaming_rgba(data);
  }

  pub fn set_nv12(&self, data: Vec<u8>) {
    self.image.set_streaming_nv12(data);
  }

  pub fn take_rgba_buffer(&self) -> Option<Vec<u8>> {
    self.image.take_streaming_rgba_buffer()
  }

  pub fn take_nv12_buffer(&self) -> Option<Vec<u8>> {
    self.image.take_streaming_nv12_buffer()
  }

  pub fn update_rgba(&self, update: impl FnOnce(&mut [u8])) {
    self.image.update_streaming_rgba(update);
  }

  pub fn version(&self) -> u64 {
    self.image.version()
  }
}

#[cfg(feature = "image")]
fn delay_ms(delay: image::Delay) -> u64 {
  let (numerator, denominator) = delay.numer_denom_ms();
  if denominator == 0 {
    return MIN_ANIMATION_FRAME_MS;
  }
  let ms = u64::from(numerator).div_ceil(u64::from(denominator));
  ms.max(MIN_ANIMATION_FRAME_MS)
}

fn nv12_len(width: u32, height: u32) -> usize {
  assert!(width > 0 && height > 0 && width.is_multiple_of(2) && height.is_multiple_of(2));
  (width * height + width * height / 2) as usize
}

#[cfg(all(test, feature = "image"))]
mod tests {
  use std::time::{Duration, Instant};

  use image::{Delay, Frame, Rgba, RgbaImage, codecs::gif::GifEncoder};

  use super::{ImageData, ImagePixelFormat, NativeImageBackend, NativeImageData, StreamingImage, animation_epoch};

  #[test]
  fn gif_decoding_preserves_animation_with_stable_image_id() {
    let mut bytes = Vec::new();
    {
      let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
      let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
      let frames = vec![
        Frame::from_parts(red, 0, 0, Delay::from_numer_denom_ms(20, 1)),
        Frame::from_parts(blue, 0, 0, Delay::from_numer_denom_ms(20, 1)),
      ];
      GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
    }

    let image = ImageData::from_bytes(&bytes).unwrap();
    let image_id = image.id();
    let epoch = animation_epoch();

    assert!(image.is_animated());
    assert_eq!(image.frame_at(epoch).frame_index, 0);
    assert_eq!(image.frame_at(epoch + Duration::from_millis(25)).frame_index, 1);
    assert_eq!(image.id(), image_id);
  }

  #[test]
  fn animated_frame_reports_next_frame_time() {
    let mut bytes = Vec::new();
    {
      let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
      let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
      let frames = vec![
        Frame::from_parts(red, 0, 0, Delay::from_numer_denom_ms(20, 1)),
        Frame::from_parts(blue, 0, 0, Delay::from_numer_denom_ms(20, 1)),
      ];
      GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
    }

    let image = ImageData::from_bytes(&bytes).unwrap();
    let epoch = animation_epoch();
    let frame = image.frame_at(epoch + Duration::from_millis(5));

    assert_eq!(frame.frame_index, 0);
    assert_eq!(frame.next_frame_at, Some(epoch + Duration::from_millis(20)));
  }

  #[test]
  fn separately_decoded_animated_images_share_frame_clock() {
    let mut bytes = Vec::new();
    {
      let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
      let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
      let frames = vec![
        Frame::from_parts(red, 0, 0, Delay::from_numer_denom_ms(100, 1)),
        Frame::from_parts(blue, 0, 0, Delay::from_numer_denom_ms(100, 1)),
      ];
      GifEncoder::new(&mut bytes).encode_frames(frames).unwrap();
    }

    let image = ImageData::from_bytes(&bytes).unwrap();
    let later_decoded_image = ImageData::from_bytes(&bytes).unwrap();

    let now = animation_epoch() + Duration::from_millis(125);
    let frame = image.frame_at(now);
    let later_decoded_frame = later_decoded_image.frame_at(now);

    assert_eq!(frame.frame_index, 1);
    assert_eq!(later_decoded_frame.frame_index, frame.frame_index);
    assert_eq!(later_decoded_frame.next_frame_at, frame.next_frame_at);
  }

  #[test]
  fn streaming_rgba_updates_shared_clones_with_stable_image_id() {
    let image = ImageData::streaming_rgba(vec![0, 0, 0, 255], 1, 1);
    let clone = image.clone();
    let image_id = image.id();
    let now = Instant::now();

    let initial = clone.frame_at(now);
    assert_eq!(&initial.data[..], &[0, 0, 0, 255]);
    assert_eq!(initial.version, 0);

    image.set_streaming_rgba(vec![255, 0, 0, 255]);
    let updated = clone.frame_at(now);

    assert_eq!(clone.id(), image_id);
    assert_eq!(&updated.data[..], &[255, 0, 0, 255]);
    assert_eq!(updated.version, 1);
  }

  #[test]
  fn streaming_rgba_update_mutates_pixels_and_bumps_version() {
    let image = ImageData::streaming_rgba(vec![0, 0, 0, 255], 1, 1);

    image.update_streaming_rgba(|pixels| {
      pixels[1] = 128;
    });

    let frame = image.frame_at(Instant::now());
    assert_eq!(&frame.data[..], &[0, 128, 0, 255]);
    assert_eq!(frame.version, 1);
  }

  #[test]
  fn streaming_rgba_update_reuses_unique_current_buffer() {
    let image = ImageData::streaming_rgba(vec![0, 0, 0, 255], 1, 1);
    image.set_streaming_rgba(vec![255, 0, 0, 255]);
    let current_ptr = {
      let frame = image.frame_at(Instant::now());
      frame.data.as_ptr()
    };

    image.update_streaming_rgba(|pixels| {
      pixels[1] = 128;
    });

    let frame = image.frame_at(Instant::now());
    assert_eq!(frame.data.as_ptr(), current_ptr);
    assert_eq!(&frame.data[..], &[255, 128, 0, 255]);
    assert_eq!(frame.version, 2);
  }

  #[test]
  fn streaming_rgba_recycles_released_buffers() {
    let image = ImageData::streaming_rgba(vec![0, 0, 0, 255], 1, 1);
    image.set_streaming_rgba(vec![255, 0, 0, 255]);
    image.set_streaming_rgba(vec![0, 255, 0, 255]);

    let recycled = image.take_streaming_rgba_buffer().unwrap();

    assert!(recycled.capacity() >= 4);
    assert!(recycled.is_empty());
  }

  #[test]
  fn streaming_rgba_keeps_buffers_until_renderer_releases_them() {
    let image = ImageData::streaming_rgba(vec![0, 0, 0, 255], 1, 1);
    image.set_streaming_rgba(vec![255, 0, 0, 255]);
    let held = image.frame_at(Instant::now()).data;
    image.set_streaming_rgba(vec![0, 255, 0, 255]);

    assert!(image.take_streaming_rgba_buffer().is_some());
    assert!(image.take_streaming_rgba_buffer().is_none());
    drop(held);

    assert!(image.take_streaming_rgba_buffer().is_some());
  }

  #[test]
  fn manual_redraw_streaming_rgba_does_not_request_continuous_redraw() {
    let image = StreamingImage::new_rgba_manual_redraw(vec![0, 0, 0, 255], 1, 1).image_data();

    assert!(image.is_streaming());
    assert!(!image.requires_continuous_redraw());
  }

  #[test]
  fn continuous_native_image_requests_immediate_redraw() {
    let native =
      NativeImageData::new_with_continuous_redraw(2, 2, ImagePixelFormat::Nv12, NativeImageBackend::Dx12Nv12, (), true);
    let image = native.image_data();
    let now = Instant::now();

    assert!(image.requires_continuous_redraw());
    assert_eq!(image.frame_at(now).next_frame_at, Some(now));
  }
}
