//! Native offscreen canvas work on the UI renderer's command queue.
mod artwork;
mod presentation_pool;
use presentation_pool::{ArtworkLease, Spare};
mod encoding;
mod presentation;
use std::collections::VecDeque;

use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

mod draw;
mod pipeline;
mod resources;

use pipeline::pipeline;
pub(super) use resources::create_srv;
use resources::{copy_location, readback, texture, viewport};

use super::*;
#[cfg(feature = "perf_profile")]
use crate::app::profile_types::canvas_upload::{AssetUploadStage, CanvasAssetUploadProfile};
use crate::canvas::{
  AssetCache, BlendMode, CanvasAssetBudget, CanvasError, CanvasHandle, CanvasId, CanvasSnapshot, CanvasWeak,
  FrameBoundary, GradientKind, PresentationEvent, TargetCharge, gpu::*, replacement_admitted,
};

struct Artwork {
  bytes: usize,
  texture: ID3D12Resource,
  width: u32,
  height: u32,
}
struct Backing {
  bytes: usize,
  revision: u64,
  artwork: Option<std::sync::Arc<Artwork>>,
  owner: CanvasWeak,
  texture: ID3D12Resource,
  width: u32,
  height: u32,
}
struct Readback {
  done: Completion,
  buffer: ID3D12Resource,
  width: u32,
  height: u32,
  pitch: u32,
  revision: u64,
}
pub(super) struct Renderer {
  #[cfg(feature = "perf_profile")]
  pub(super) profile: crate::app::profile_types::CanvasProfile,
  #[cfg(feature = "perf_profile")]
  pub(super) profile_context: Option<crate::app::profiler::ProfileContext>,
  meshes: MeshCache,
  surfaces: HashMap<CanvasId, Backing>,
  fronts: HashMap<CanvasId, (u64, Backing)>,
  spares: Vec<Spare>,
  retired_artwork: Vec<ArtworkLease>,
  rejected: HashSet<CanvasId>,
  assets: AssetCache<ID3D12Resource>,
  frames: FrameBoundary,
  scratch: ID3D12Resource,
  resolve: ID3D12Resource,
  _stencil: ID3D12Resource,
  rtvs: CpuDescriptorHeap,
  dsv: CpuDescriptorHeap,
  srvs: [CpuDescriptorHeap; FRAME_COUNT],
  samplers: CpuDescriptorHeap,
  descriptor: usize,
  root: ID3D12RootSignature,
  seed: ID3D12PipelineState,
  resample: ID3D12PipelineState,
  clip: ID3D12PipelineState,
  solid: ID3D12PipelineState,
  image: ID3D12PipelineState,
  retained: ID3D12PipelineState,
  erase: ID3D12PipelineState,
  gradient_linear: ID3D12PipelineState,
  gradient_radial: ID3D12PipelineState,
  gradient_angular: ID3D12PipelineState,
  /// Composites a finished isolated layer onto what was under it, source-over.
  compose: ID3D12PipelineState,
  /// Puts a saved tile back as the base of the draw that composites over it.
  restore: ID3D12PipelineState,
  /// The same composite for the other seventeen blend modes, which read the
  /// backdrop instead of relying on fixed-function blending.
  blend: ID3D12PipelineState,
  /// Tile-sized copies for isolated layers, allocated on first use.
  saved: Vec<ID3D12Resource>,
  layer: Option<ID3D12Resource>,
  batches: Vec<Batch>,
  readbacks: Vec<Readback>,
  pending_stats: Vec<(CanvasWeak, usize, usize, usize)>,
}
impl Renderer {
  pub unsafe fn new(device: &ID3D12Device) -> Result<Self> {
    // Two shader resources: every pipeline binds a pair so that one root
    // signature serves the blend composite, which needs its backdrop, as well.
    let root = create_image_root_signature(device, 2)?;
    let seed = pipeline(device, &root, b"vs_seed\0", b"ps_premul\0", 4, 0)?;
    let resample = pipeline(device, &root, b"vs_seed\0", b"ps_premul\0", 1, 0)?;
    let clip = pipeline(device, &root, b"vs_main\0", b"ps_solid\0", 4, 2)?;
    let solid = pipeline(device, &root, b"vs_main\0", b"ps_solid\0", 4, 3)?;
    let retained = pipeline(device, &root, b"vs_main\0", b"ps_premul\0", 1, 6)?;
    let image = pipeline(device, &root, b"vs_main\0", b"ps_premul\0", 4, 3)?;
    let erase = pipeline(device, &root, b"vs_main\0", b"ps_solid\0", 4, 4)?;
    let gradient_linear = pipeline(device, &root, b"vs_main\0", b"ps_gradient_linear\0", 4, 3)?;
    let gradient_radial = pipeline(device, &root, b"vs_main\0", b"ps_gradient_radial\0", 4, 3)?;
    let gradient_angular = pipeline(device, &root, b"vs_main\0", b"ps_gradient_angular\0", 4, 3)?;
    let compose = pipeline(device, &root, b"vs_tile\0", b"ps_premul\0", 4, 5)?;
    let restore = pipeline(device, &root, b"vs_tile\0", b"ps_tile\0", 4, 6)?;
    let blend = pipeline(device, &root, b"vs_tile\0", b"ps_blend\0", 4, 6)?;
    let scratch = texture(device, TILE, TILE, 4, false, D3D12_RESOURCE_STATE_RENDER_TARGET)?;
    let resolve = texture(device, TILE, TILE, 1, false, D3D12_RESOURCE_STATE_COPY_SOURCE)?;
    let stencil = texture(device, TILE, TILE, 4, true, D3D12_RESOURCE_STATE_DEPTH_WRITE)?;
    let rtvs = CpuDescriptorHeap::new(
      device,
      D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
      2,
      D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
    )?;
    device.CreateRenderTargetView(&scratch, None, rtvs.cpu_handle(0));
    let dsv = CpuDescriptorHeap::new(
      device,
      D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
      1,
      D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
    )?;
    device.CreateDepthStencilView(&stencil, None, dsv.cpu_handle(0));
    let srvs = [
      CpuDescriptorHeap::new(
        device,
        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        16384,
        D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
      )?,
      CpuDescriptorHeap::new(
        device,
        D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        16384,
        D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
      )?,
    ];
    let samplers = CpuDescriptorHeap::new(
      device,
      D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
      2,
      D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
    )?;
    for (index, filter) in [D3D12_FILTER_MIN_MAG_MIP_POINT, D3D12_FILTER_MIN_MAG_MIP_LINEAR]
      .into_iter()
      .enumerate()
    {
      device.CreateSampler(
        &D3D12_SAMPLER_DESC {
          Filter: filter,
          AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
          AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
          AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
          ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
          MaxLOD: f32::MAX,
          MaxAnisotropy: 1,
          ..Default::default()
        },
        samplers.cpu_handle(index),
      );
    }
    Ok(Self {
      #[cfg(feature = "perf_profile")]
      profile: Default::default(),
      #[cfg(feature = "perf_profile")]
      profile_context: None,
      meshes: MeshCache::default(),
      surfaces: HashMap::new(),
      fronts: HashMap::new(),
      spares: Vec::new(),
      retired_artwork: Vec::new(),
      rejected: HashSet::new(),
      assets: AssetCache::new(CanvasAssetBudget::default()),
      frames: FrameBoundary::default(),
      scratch,
      resolve,
      _stencil: stencil,
      rtvs,
      dsv,
      srvs,
      samplers,
      descriptor: 0,
      root,
      seed,
      resample,
      clip,
      solid,
      image,
      retained,
      erase,
      gradient_linear,
      gradient_radial,
      gradient_angular,
      compose,
      restore,
      blend,
      saved: Vec::new(),
      layer: None,
      batches: Vec::new(),
      readbacks: Vec::new(),
      pending_stats: Vec::new(),
    })
  }
  pub fn with_asset_budget(mut self, budget: CanvasAssetBudget) -> Self {
    self.assets = AssetCache::new(budget);
    self
  }
  pub fn backing(&self, canvas: &CanvasHandle) -> Option<ID3D12Resource> {
    let id = canvas.surface_id();
    self
      .fronts
      .get(&id)
      .map(|(_, b)| b)
      .or_else(|| self.surfaces.get(&id))
      .map(|b| b.texture.clone())
  }

  unsafe fn srv(&mut self, state: &Dx12State, texture: &ID3D12Resource) -> Result<D3D12_GPU_DESCRIPTOR_HANDLE> {
    self.srv_pair(state, texture, texture)
  }
  /// Two adjacent descriptors, because the root signature's table is a pair.
  /// Only the blend composite reads the second one.
  unsafe fn srv_pair(
    &mut self,
    state: &Dx12State,
    texture: &ID3D12Resource,
    backdrop: &ID3D12Resource,
  ) -> Result<D3D12_GPU_DESCRIPTOR_HANDLE> {
    if self.descriptor + 2 > 16384 {
      return Err(dx12_invalid_arg("canvas descriptor budget exceeded".to_owned()));
    }
    let heap = &self.srvs[state.frame_index];
    let index = self.descriptor;
    self.descriptor += 2;
    create_srv(&state.device, texture, heap.cpu_handle(index));
    create_srv(&state.device, backdrop, heap.cpu_handle(index + 1));
    Ok(heap.gpu_handle(index))
  }
  unsafe fn resize(
    &mut self,
    state: &mut Dx12State,
    canvas: &CanvasHandle,
    width: u32,
    height: u32,
    preserve: bool,
  ) -> Result<()> {
    let id = canvas.surface_id();
    if width == 0 || height == 0 {
      if let Some(old) = self.surfaces.remove(&id) {
        self.retire_back(state, id, old);
      }
      canvas.set_gpu_bytes(0);
      return Ok(());
    }
    if !self.allocation_admitted(
      state,
      resources::target_allocation_bytes(&state.device, width, height, false),
    ) {
      canvas.set_gpu_error(CanvasError::PresentationBusy);
      return Ok(());
    }
    let next = match texture(
      &state.device,
      width,
      height,
      1,
      false,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
    ) {
      Ok(texture) => texture,
      Err(_) => {
        canvas.set_gpu_error(CanvasError::SurfaceTooLarge);
        return Ok(());
      }
    };
    let old = self.surfaces.remove(&id);
    let artwork = old.as_ref().and_then(|back| back.artwork.clone());
    let rtv = self.rtvs.cpu_handle(1);
    state.device.CreateRenderTargetView(&next, None, rtv);
    state.command_list.OMSetRenderTargets(1, Some(&rtv), false, None);
    state.command_list.ClearRenderTargetView(rtv, &[0.; 4], None);
    if let Some(old) = old {
      if preserve {
        let srv = self.srv(state, &old.texture)?;
        let globals = state.upload_frame_constant(&[
          0f32,
          0.,
          width as f32,
          height as f32,
          width as f32,
          height as f32,
          0.,
          0.,
        ])?;
        viewport(&state.command_list, width, height);
        state.command_list.SetPipelineState(&self.resample);
        state
          .command_list
          .SetGraphicsRootConstantBufferView(0, globals.gpu_address);
        state.command_list.SetGraphicsRootDescriptorTable(1, srv);
        state
          .command_list
          .SetGraphicsRootDescriptorTable(2, self.samplers.gpu_handle(1));
        state.command_list.DrawInstanced(3, 1, 0, 0);
      }
      self.retire_back(state, id, old);
    }
    state.transition_resource(
      &next,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );
    self.surfaces.insert(
      id,
      Backing {
        revision: 0,
        artwork,
        owner: canvas.downgrade(),
        bytes: resources::target_allocation_bytes(&state.device, width, height, false),
        texture: next,
        width,
        height,
      },
    );
    self.account_presentation(state, canvas);
    Ok(())
  }
  /// Tile-sized copies an isolated layer needs: `saved[d]` is what was under the
  /// layer opened at depth `d`, and one shared `layer` holds the finished layer
  /// being composited. Allocated on first use, so a canvas that never opens a
  /// layer pays nothing; each is TILE × TILE × 4 bytes.
  unsafe fn reserve_layers(&mut self, device: &ID3D12Device, depth: usize) -> Result<()> {
    while self.saved.len() < depth {
      self.saved.push(texture(
        device,
        TILE,
        TILE,
        1,
        false,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      )?);
    }
    if depth > 0 && self.layer.is_none() {
      self.layer = Some(texture(
        device,
        TILE,
        TILE,
        1,
        false,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      )?);
    }
    Ok(())
  }

  /// Resolves the multisampled tile into `self.resolve`, which the caller then
  /// copies wherever this segment belongs.
  unsafe fn resolve_tile(&self, state: &mut Dx12State) {
    state.transition_resource(
      &self.scratch,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
      D3D12_RESOURCE_STATE_RESOLVE_SOURCE,
    );
    state.transition_resource(
      &self.resolve,
      D3D12_RESOURCE_STATE_COPY_SOURCE,
      D3D12_RESOURCE_STATE_RESOLVE_DEST,
    );
    state
      .command_list
      .ResolveSubresource(&self.resolve, 0, &self.scratch, 0, DXGI_FORMAT_R8G8B8A8_UNORM);
    state.transition_resource(
      &self.scratch,
      D3D12_RESOURCE_STATE_RESOLVE_SOURCE,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    state.transition_resource(
      &self.resolve,
      D3D12_RESOURCE_STATE_RESOLVE_DEST,
      D3D12_RESOURCE_STATE_COPY_SOURCE,
    );
  }

  unsafe fn copy_resolved(
    &self,
    state: &mut Dx12State,
    target: &ID3D12Resource,
    from: D3D12_RESOURCE_STATES,
    at: (u32, u32),
    tile: [u32; 4],
  ) {
    state.transition_resource(target, from, D3D12_RESOURCE_STATE_COPY_DEST);
    let mut src = copy_location(&self.resolve);
    let mut dst = copy_location(target);
    state.command_list.CopyTextureRegion(
      &dst,
      at.0,
      at.1,
      0,
      &src,
      Some(&D3D12_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: tile[2],
        bottom: tile[3],
        back: 1,
      }),
    );
    ManuallyDrop::drop(&mut src.pResource);
    ManuallyDrop::drop(&mut dst.pResource);
    state.transition_resource(target, D3D12_RESOURCE_STATE_COPY_DEST, from);
  }
}

impl Drop for Renderer {
  fn drop(&mut self) {
    for backing in self.surfaces.values() {
      if let Some(c) = backing.owner.upgrade() {
        c.set_gpu_bytes(0);
        c.set_gpu_error(CanvasError::RendererLost);
      }
    }
  }
}
