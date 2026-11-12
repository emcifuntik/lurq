//! Offscreen targets for flattened group opacity and layer blur (see
//! [`crate::layout::opacity_layer`]) and the pipelines that composite them.
//!
//! A blurred layer takes one or two more textures from the same pool for the
//! passes between painting it and compositing it ([`BlurSlots`]). A backdrop
//! blur (see [`crate::layout::backdrop`]) takes those and one for the copy of
//! its target ([`BackdropSlots`]).
//!
//! Layer textures are pooled across frames: a frame takes the smallest free
//! texture that fits each layer, creating one (rounded up to
//! [`LAYER_SIZE_STEP`]) only when none does. Textures unused for
//! [`LAYER_IDLE_FRAMES`] frames are retired and released once the GPU is done
//! with the frame that last used them. Nothing is created until a frame has
//! a layer.

use std::{mem::ManuallyDrop, ptr};

use windows::{
  Win32::{
    Foundation::{FALSE, RECT, TRUE},
    Graphics::{
      Direct3D12::{
        D3D_ROOT_SIGNATURE_VERSION_1, D3D12_BLEND, D3D12_BLEND_DESC, D3D12_BLEND_INV_SRC_ALPHA, D3D12_BLEND_ONE,
        D3D12_BLEND_OP_ADD, D3D12_BLEND_ZERO, D3D12_BOX, D3D12_CLEAR_VALUE, D3D12_CLEAR_VALUE_0,
        D3D12_COLOR_WRITE_ENABLE_ALL, D3D12_COMPARISON_FUNC_ALWAYS, D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_CULL_MODE_NONE, D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        D3D12_DEPTH_STENCIL_DESC, D3D12_DEPTH_WRITE_MASK_ZERO, D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
        D3D12_DESCRIPTOR_HEAP_TYPE_RTV, D3D12_DESCRIPTOR_RANGE, D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
        D3D12_DESCRIPTOR_RANGE_TYPE_SRV, D3D12_FILL_MODE_SOLID, D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT,
        D3D12_GRAPHICS_PIPELINE_STATE_DESC, D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT,
        D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED, D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
        D3D12_INPUT_LAYOUT_DESC, D3D12_LOGIC_OP_NOOP, D3D12_MEMORY_POOL_UNKNOWN, D3D12_PIPELINE_STATE_FLAG_NONE,
        D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE, D3D12_RASTERIZER_DESC, D3D12_RENDER_TARGET_BLEND_DESC,
        D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_ROOT_CONSTANTS, D3D12_ROOT_DESCRIPTOR_TABLE, D3D12_ROOT_PARAMETER,
        D3D12_ROOT_PARAMETER_0, D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS, D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
        D3D12_ROOT_SIGNATURE_DESC, D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT, D3D12_SHADER_BYTECODE,
        D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SHADER_VISIBILITY_ALL,
        D3D12_SHADER_VISIBILITY_PIXEL, D3D12_SRV_DIMENSION_TEXTURE2D, D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
        D3D12_STATIC_SAMPLER_DESC, D3D12_TEX2D_SRV, D3D12_TEXTURE_ADDRESS_MODE_CLAMP, D3D12_TEXTURE_COPY_LOCATION,
        D3D12_TEXTURE_COPY_LOCATION_0, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, D3D12_TEXTURE_LAYOUT_UNKNOWN,
        D3D12_VIEWPORT, D3D12SerializeRootSignature, ID3D12Device, ID3D12PipelineState, ID3D12Resource,
        ID3D12RootSignature,
      },
      Dxgi::Common::{DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC},
    },
  },
  core::{Error, Result},
};

use super::{
  CpuDescriptorHeap, Dx12State, FRAME_COUNT, RENDER_TARGET_FORMAT, SRV_DESCRIPTOR_COUNT, Win32ErrorCompat, blob_bytes,
  blob_message, compile_shader, input_element, render_target_view_desc, shader_bytecode,
};
use crate::layout::{
  backdrop::BackdropRegion,
  opacity_layer::{BlurPlan, LayerBlur, LayerBlurPass, LayerCmd, LayerComposite, TargetSpace},
};

/// Layer textures grow in steps of this many pixels, so a layer that changes
/// size a little (an animation) keeps its texture.
const LAYER_SIZE_STEP: u32 = 64;
/// Frames a pooled layer texture may stay unused before it is released.
const LAYER_IDLE_FRAMES: u64 = 120;
/// Shader-visible descriptors each frame slot has for the layers it
/// composites; they follow the image descriptors in the SRV heap.
pub(super) const LAYER_SRVS_PER_FRAME: usize = 1024;
/// Most textures a single dimension may have (D3D12's 2D texture limit).
const MAX_TEXTURE_DIMENSION: u32 = 16384;

pub(super) struct LayerCompositor {
  root_signature: ID3D12RootSignature,
  pipeline_state: ID3D12PipelineState,
  blur_root_signature: ID3D12RootSignature,
  blur_pipeline_state: ID3D12PipelineState,
  /// The two draws that put a blurred backdrop back: remove what the box
  /// covers, then add the blurred pixels.
  backdrop_erase_pipeline_state: ID3D12PipelineState,
  backdrop_add_pipeline_state: ID3D12PipelineState,
  slots: Vec<LayerSlot>,
  /// The blur textures of each of the frame's targets, by target index.
  blur_slots: Vec<Option<BlurSlots>>,
  /// The textures of each of the frame's backdrop blurs, by its index in
  /// `RenderList::backdrops`.
  backdrop_slots: Vec<Option<BackdropSlots>>,
  /// Released slots, kept alive until their frame slot's fence passes.
  retired: [Vec<LayerSlot>; FRAME_COUNT],
  frame: u64,
  next_srv: usize,
}

/// The textures a blurred layer's passes paint, beside the one it is painted
/// into: the reduced copy of a wide blur, and the horizontal pass's result,
/// which the pass compositing the layer reads.
#[derive(Clone, Copy)]
struct BlurSlots {
  blur: LayerBlur,
  plan: BlurPlan,
  reduced: Option<usize>,
  blurred: usize,
}

/// The textures of a backdrop blur: the copy of its target's pixels, and the
/// textures of the blur run on it.
#[derive(Clone, Copy)]
struct BackdropSlots {
  region: BackdropRegion,
  copied: usize,
  blur: BlurSlots,
}

/// Which pipeline draws a pass of the blur shader.
#[derive(Clone, Copy)]
enum BlurDraw {
  /// A pass of a layer blur, or of a backdrop blur before its last.
  Pass,
  BackdropErase,
  BackdropAdd,
}

struct LayerSlot {
  resource: ID3D12Resource,
  rtv_heap: CpuDescriptorHeap,
  width: u32,
  height: u32,
  in_use: bool,
  last_used: u64,
}

impl LayerCompositor {
  unsafe fn new(device: &ID3D12Device) -> Result<Self> {
    let root_signature = create_layer_root_signature(device, std::mem::size_of::<LayerComposite>(), false)?;
    // Premultiplied source-over: a layer holds premultiplied colour.
    let over = (D3D12_BLEND_ONE, D3D12_BLEND_INV_SRC_ALPHA);
    let pipeline_state = create_layer_pipeline(
      device,
      &root_signature,
      include_bytes!("shaders/layer.hlsl"),
      b"ps_main\0",
      over,
    )?;
    let blur_root_signature = create_layer_root_signature(device, std::mem::size_of::<LayerBlurPass>(), true)?;
    let blur_shader = include_bytes!("shaders/layer_blur.hlsl");
    let blur_pipeline_state = create_layer_pipeline(device, &blur_root_signature, blur_shader, b"ps_main\0", over)?;
    let backdrop_erase_pipeline_state = create_layer_pipeline(
      device,
      &blur_root_signature,
      blur_shader,
      b"ps_backdrop_erase\0",
      (D3D12_BLEND_ZERO, D3D12_BLEND_INV_SRC_ALPHA),
    )?;
    let backdrop_add_pipeline_state = create_layer_pipeline(
      device,
      &blur_root_signature,
      blur_shader,
      b"ps_backdrop_add\0",
      (D3D12_BLEND_ONE, D3D12_BLEND_ONE),
    )?;
    Ok(Self {
      root_signature,
      pipeline_state,
      blur_root_signature,
      blur_pipeline_state,
      backdrop_erase_pipeline_state,
      backdrop_add_pipeline_state,
      slots: Vec::new(),
      blur_slots: Vec::new(),
      backdrop_slots: Vec::new(),
      retired: std::array::from_fn(|_| Vec::new()),
      frame: 0,
      next_srv: 0,
    })
  }

  /// The textures the passes of `blur` paint this frame.
  unsafe fn acquire_blur(&mut self, device: &ID3D12Device, blur: &LayerBlur) -> Result<BlurSlots> {
    let plan = BlurPlan::new(blur);
    let reduced = match plan.reduction > 1 {
      true => Some(self.acquire(device, plan.size[0], plan.size[1])?),
      false => None,
    };
    let blurred = self.acquire(device, plan.size[0], plan.size[1])?;
    Ok(BlurSlots {
      blur: *blur,
      plan,
      reduced,
      blurred,
    })
  }

  /// The textures the backdrop blur `region` uses this frame.
  unsafe fn acquire_backdrop(
    &mut self,
    device: &ID3D12Device,
    region: BackdropRegion,
  ) -> Result<Option<BackdropSlots>> {
    let Some(blur) = region.layer.blur else {
      return Ok(None);
    };
    let copied = self.acquire(device, blur.width, blur.height)?;
    Ok(Some(BackdropSlots {
      region,
      copied,
      blur: self.acquire_blur(device, &blur)?,
    }))
  }

  fn size(&self, slot: usize) -> [u32; 2] {
    [self.slots[slot].width, self.slots[slot].height]
  }

  /// A free texture of at least `width` x `height` for this frame.
  unsafe fn acquire(&mut self, device: &ID3D12Device, width: u32, height: u32) -> Result<usize> {
    let fitting = self
      .slots
      .iter()
      .enumerate()
      .filter(|(_, slot)| !slot.in_use && slot.width >= width && slot.height >= height)
      .min_by_key(|(_, slot)| u64::from(slot.width) * u64::from(slot.height))
      .map(|(index, _)| index);
    let index = match fitting {
      Some(index) => index,
      None => {
        let size = |value: u32| {
          value
            .div_ceil(LAYER_SIZE_STEP)
            .saturating_mul(LAYER_SIZE_STEP)
            .min(MAX_TEXTURE_DIMENSION)
        };
        self.slots.push(create_slot(device, size(width), size(height))?);
        self.slots.len() - 1
      }
    };
    let slot = &mut self.slots[index];
    slot.in_use = true;
    slot.last_used = self.frame;
    Ok(index)
  }
}

/// The pipeline of a layer shader: the unit quad drawn by `shader`'s
/// `vs_main` and its pixel shader `ps_entry`, blended into the render target
/// format as `src * blend.0 + dst * blend.1`, colour and alpha alike.
unsafe fn create_layer_pipeline(
  device: &ID3D12Device,
  root_signature: &ID3D12RootSignature,
  shader: &[u8],
  ps_entry: &'static [u8],
  (src_blend, dest_blend): (D3D12_BLEND, D3D12_BLEND),
) -> Result<ID3D12PipelineState> {
  {
    let vs = compile_shader(shader, b"vs_main\0", b"vs_5_0\0")?;
    let ps = compile_shader(shader, ps_entry, b"ps_5_0\0")?;
    let input_elements = [input_element(
      0,
      DXGI_FORMAT_R32G32_FLOAT,
      0,
      0,
      D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
      0,
    )];
    let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
    rtv_formats[0] = RENDER_TARGET_FORMAT;
    let blend = D3D12_RENDER_TARGET_BLEND_DESC {
      BlendEnable: TRUE,
      LogicOpEnable: FALSE,
      SrcBlend: src_blend,
      DestBlend: dest_blend,
      BlendOp: D3D12_BLEND_OP_ADD,
      SrcBlendAlpha: src_blend,
      DestBlendAlpha: dest_blend,
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
    let pipeline_state = device.CreateGraphicsPipelineState(&desc);
    ManuallyDrop::drop(&mut desc.pRootSignature);
    pipeline_state
  }
}

unsafe fn create_slot(device: &ID3D12Device, width: u32, height: u32) -> Result<LayerSlot> {
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
    Width: u64::from(width),
    Height: height,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: RENDER_TARGET_FORMAT,
    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
    Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
    Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
  };
  let clear = D3D12_CLEAR_VALUE {
    Format: RENDER_TARGET_FORMAT,
    Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.0; 4] },
  };
  let mut resource = None;
  device.CreateCommittedResource(
    &heap_properties,
    D3D12_HEAP_FLAG_NONE,
    &desc,
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    Some(&clear),
    &mut resource,
  )?;
  let resource: ID3D12Resource = resource.ok_or_else(Error::from_win32)?;
  let rtv_heap = CpuDescriptorHeap::new(
    device,
    D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
    1,
    D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
  )?;
  device.CreateRenderTargetView(&resource, Some(&render_target_view_desc()), rtv_heap.cpu_handle(0));
  Ok(LayerSlot {
    resource,
    rtv_heap,
    width,
    height,
    in_use: false,
    last_used: 0,
  })
}

/// The root signature of a layer shader: `constants` bytes of uniforms as
/// root constants (b0) and the layer texture (t0), with a bilinear sampler
/// (s0) when `sampler`.
unsafe fn create_layer_root_signature(
  device: &ID3D12Device,
  constants: usize,
  sampler: bool,
) -> Result<ID3D12RootSignature> {
  let srv_range = D3D12_DESCRIPTOR_RANGE {
    RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
    NumDescriptors: 1,
    BaseShaderRegister: 0,
    RegisterSpace: 0,
    OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
  };
  let root_parameters = [
    // b0: the shader's uniforms as root constants.
    D3D12_ROOT_PARAMETER {
      ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
      Anonymous: D3D12_ROOT_PARAMETER_0 {
        Constants: D3D12_ROOT_CONSTANTS {
          ShaderRegister: 0,
          RegisterSpace: 0,
          Num32BitValues: (constants / 4) as u32,
        },
      },
      ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
    },
    // t0: the layer texture.
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
  ];
  // Bilinear, so one tap reads two texels of a Gaussian or four of a box;
  // past the texture's edge it repeats the edge, which a pass never shows
  // (see `layout::opacity_layer`).
  let static_sampler = D3D12_STATIC_SAMPLER_DESC {
    Filter: D3D12_FILTER_MIN_MAG_LINEAR_MIP_POINT,
    AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
    MipLODBias: 0.0,
    MaxAnisotropy: 1,
    ComparisonFunc: D3D12_COMPARISON_FUNC_ALWAYS,
    BorderColor: D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
    MinLOD: 0.0,
    MaxLOD: f32::MAX,
    ShaderRegister: 0,
    RegisterSpace: 0,
    ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
  };
  let desc = D3D12_ROOT_SIGNATURE_DESC {
    NumParameters: root_parameters.len() as u32,
    pParameters: root_parameters.as_ptr(),
    NumStaticSamplers: u32::from(sampler),
    pStaticSamplers: if sampler { &static_sampler } else { ptr::null() },
    Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
  };
  let mut blob = None;
  let mut errors = None;
  D3D12SerializeRootSignature(&desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, Some(&mut errors)).map_err(|err| {
    Error::new(
      err.code(),
      format!(
        "failed to serialize dx12 layer root signature{}",
        blob_message(errors.as_ref())
      ),
    )
  })?;
  let blob = blob.ok_or_else(Error::from_win32)?;
  device.CreateRootSignature(0, blob_bytes(&blob))
}

#[cfg(all(test, feature = "screenshot"))]
impl LayerCompositor {
  pub(super) fn texture_count(&self) -> usize {
    self.slots.len()
  }
}

/// The first SRV descriptor of `frame_index`'s layer range.
fn layer_srv_start(frame_index: usize) -> usize {
  SRV_DESCRIPTOR_COUNT as usize + frame_index * LAYER_SRVS_PER_FRAME
}

impl Dx12State {
  /// Starts a frame whose plan has layers or backdrop blurs: takes a texture
  /// for each target that is a layer (`None` for the window), and the
  /// textures of each backdrop blur (`None` for one that changes nothing).
  /// Releases the textures of the frame that last used this frame slot's
  /// retired list.
  pub(super) unsafe fn begin_layer_frame(
    &mut self,
    layers: &[Option<&LayerCmd>],
    backdrops: &[Option<BackdropRegion>],
    slots: &mut Vec<Option<usize>>,
  ) -> Result<()> {
    slots.clear();
    if self.layer_compositor.is_none() {
      self.layer_compositor = Some(LayerCompositor::new(&self.device)?);
    }
    let Some(compositor) = self.layer_compositor.as_mut() else {
      return Ok(());
    };
    compositor.frame += 1;
    compositor.next_srv = 0;
    for slot in &mut compositor.slots {
      slot.in_use = false;
    }
    compositor.blur_slots.clear();
    for layer in layers {
      slots.push(match layer {
        Some(layer) => {
          let (_, [width, height]) = layer.painted();
          Some(compositor.acquire(&self.device, width, height)?)
        }
        None => None,
      });
      let blur_slots = match layer.and_then(|layer| layer.blur) {
        Some(blur) => Some(compositor.acquire_blur(&self.device, &blur)?),
        None => None,
      };
      compositor.blur_slots.push(blur_slots);
    }
    compositor.backdrop_slots.clear();
    for region in backdrops {
      let backdrop_slots = match region {
        Some(region) => compositor.acquire_backdrop(&self.device, *region)?,
        None => None,
      };
      compositor.backdrop_slots.push(backdrop_slots);
    }
    Ok(())
  }

  /// Retires layer textures unused for a while; they are released when this
  /// frame slot comes round again.
  pub(super) fn end_layer_frame(&mut self) {
    let frame_index = self.frame_index;
    let Some(compositor) = self.layer_compositor.as_mut() else {
      return;
    };
    let frame = compositor.frame;
    let (kept, idle): (Vec<_>, Vec<_>) = std::mem::take(&mut compositor.slots)
      .into_iter()
      .partition(|slot| slot.in_use || frame.saturating_sub(slot.last_used) <= LAYER_IDLE_FRAMES);
    compositor.slots = kept;
    compositor.retired[frame_index].extend(idle);
    #[cfg(feature = "perf_profile")]
    {
      self.layer_profile.textures = compositor.slots.len();
      // Four bytes a texel.
      self.layer_profile.texture_bytes = compositor
        .slots
        .iter()
        .map(|slot| slot.width as usize * slot.height as usize * 4)
        .sum();
    }
  }

  /// Drops the layer textures retired by the frame that last used this frame
  /// slot; its fence has been waited on.
  pub(super) fn release_retired_layers(&mut self) {
    let frame_index = self.frame_index;
    if let Some(compositor) = self.layer_compositor.as_mut() {
      compositor.retired[frame_index].clear();
    }
  }

  /// Makes the window's back buffer the render target.
  pub(super) unsafe fn begin_window_target(&mut self) {
    let rtv = self.current_rtv_handle();
    self.command_list.OMSetRenderTargets(1, Some(&rtv), false, None);
    self.set_target_viewport(TargetSpace::window(self.width as f32, self.height as f32));
  }

  /// Makes `slot`'s texture the render target, cleared to transparent, for
  /// painting `layer`.
  pub(super) unsafe fn begin_layer_target(&mut self, slot: usize, layer: &LayerCmd) {
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return;
    };
    let slot = &compositor.slots[slot];
    let (resource, rtv) = (slot.resource.clone(), slot.rtv_heap.cpu_handle(0));
    let space = TargetSpace::layer(layer, slot.width, slot.height);
    self.transition_resource(
      &resource,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    self.command_list.OMSetRenderTargets(1, Some(&rtv), false, None);
    self.command_list.ClearRenderTargetView(rtv, &[0.0; 4], None);
    self.set_target_viewport(space);
    #[cfg(feature = "perf_profile")]
    {
      self.layer_profile.render_passes += 1;
    }
  }

  /// Finishes painting `slot`'s layer so it can be composited.
  pub(super) unsafe fn end_layer_target(&mut self, slot: usize) {
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return;
    };
    let resource = compositor.slots[slot].resource.clone();
    self.transition_resource(
      &resource,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );
  }

  unsafe fn set_target_viewport(&mut self, space: TargetSpace) {
    let viewport = D3D12_VIEWPORT {
      TopLeftX: 0.0,
      TopLeftY: 0.0,
      Width: space.width,
      Height: space.height,
      MinDepth: 0.0,
      MaxDepth: 1.0,
    };
    self.command_list.RSSetViewports(std::slice::from_ref(&viewport));
    self.space = space;
  }

  /// Runs the passes of a blur that come before compositing, on the layer
  /// just painted into `painted` as the frame's target `target`: the
  /// reduction of a wide blur, then the horizontal Gaussian. Does nothing
  /// for a layer without a blur.
  pub(super) unsafe fn prepare_layer_blur(&mut self, target: usize, painted: usize) -> Result<()> {
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let Some(slots) = compositor.blur_slots.get(target).copied().flatten() else {
      return Ok(());
    };
    self.prepare_blur(painted, slots)
  }

  /// The passes of a blur before its last, on the pixels in `painted`.
  unsafe fn prepare_blur(&mut self, painted: usize, slots: BlurSlots) -> Result<()> {
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let mut source = painted;
    if let Some(reduced) = slots.reduced {
      let constants = LayerBlurPass::reduce(
        &slots.blur,
        &slots.plan,
        compositor.size(source),
        compositor.size(reduced),
      );
      self.run_blur_pass(source, reduced, &constants, slots.plan.size)?;
      source = reduced;
    }
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let constants = LayerBlurPass::horizontal(&slots.plan, compositor.size(source), compositor.size(slots.blurred));
    self.run_blur_pass(source, slots.blurred, &constants, slots.plan.size)
  }

  /// Runs the backdrop blur `index` of the frame on the current target, in
  /// the middle of painting it: copies the pixels the blur reads out of the
  /// target, blurs the copy, and puts the result back inside the node's box.
  /// `layer` is the layer the target paints and the texture it paints into;
  /// `None` for the window.
  pub(super) unsafe fn blur_backdrop(&mut self, index: usize, layer: Option<(&LayerCmd, usize)>) -> Result<()> {
    let parent = self.space;
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let Some(slots) = compositor.backdrop_slots.get(index).copied().flatten() else {
      return Ok(());
    };
    let Some(scissor) = super::scissor_rect(
      parent.layer_rect(slots.region.layer.bounds),
      parent.width,
      parent.height,
    ) else {
      return Ok(());
    };
    let (target, target_rtv, target_origin) = match layer {
      Some((layer, slot)) => {
        let slot = &compositor.slots[slot];
        (slot.resource.clone(), slot.rtv_heap.cpu_handle(0), layer.painted().0)
      }
      None => (self.current_render_target(), self.current_rtv_handle(), [0, 0]),
    };
    let copied = compositor.slots[slots.copied].resource.clone();
    let blur = slots.blur.blur;

    self.transition_resource(
      &target,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
      D3D12_RESOURCE_STATE_COPY_SOURCE,
    );
    self.transition_resource(
      &copied,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      D3D12_RESOURCE_STATE_COPY_DEST,
    );
    let mut src = D3D12_TEXTURE_COPY_LOCATION {
      pResource: ManuallyDrop::new(Some(target.clone())),
      Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
      Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    let mut dst = D3D12_TEXTURE_COPY_LOCATION {
      pResource: ManuallyDrop::new(Some(copied.clone())),
      Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
      Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    let left = (blur.x - target_origin[0]).max(0) as u32;
    let top = (blur.y - target_origin[1]).max(0) as u32;
    let source_box = D3D12_BOX {
      left,
      top,
      front: 0,
      right: left + blur.width,
      bottom: top + blur.height,
      back: 1,
    };
    self
      .command_list
      .CopyTextureRegion(&dst, 0, 0, 0, &src, Some(&source_box));
    ManuallyDrop::drop(&mut dst.pResource);
    ManuallyDrop::drop(&mut src.pResource);
    self.transition_resource(
      &copied,
      D3D12_RESOURCE_STATE_COPY_DEST,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );
    self.transition_resource(
      &target,
      D3D12_RESOURCE_STATE_COPY_SOURCE,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
    );

    let prepared = self.prepare_blur(slots.copied, slots.blur);
    // Back to the target, as it was: the blur's passes painted elsewhere.
    self.command_list.OMSetRenderTargets(1, Some(&target_rtv), false, None);
    self.set_target_viewport(parent);
    #[cfg(feature = "perf_profile")]
    {
      self.layer_profile.render_passes += 1;
      self.layer_profile.target_copies += 1;
      self.layer_profile.backdrops_blurred += 1;
    }
    prepared?;

    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let constants = LayerBlurPass::backdrop(
      &slots.region,
      &slots.blur.plan,
      &parent,
      compositor.size(slots.blur.blurred),
    );
    self.draw_layer_blur(slots.blur.blurred, &constants, scissor, BlurDraw::BackdropErase)?;
    self.draw_layer_blur(slots.blur.blurred, &constants, scissor, BlurDraw::BackdropAdd)
  }

  /// Clears `target`'s texture and draws one blur pass reading `source`
  /// into its top-left `size` texels.
  unsafe fn run_blur_pass(
    &mut self,
    source: usize,
    target: usize,
    constants: &LayerBlurPass,
    size: [u32; 2],
  ) -> Result<()> {
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let slot = &compositor.slots[target];
    let (resource, rtv) = (slot.resource.clone(), slot.rtv_heap.cpu_handle(0));
    let (width, height) = (slot.width, slot.height);
    self.transition_resource(
      &resource,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
    );
    self.command_list.OMSetRenderTargets(1, Some(&rtv), false, None);
    self.command_list.ClearRenderTargetView(rtv, &[0.0; 4], None);
    #[cfg(feature = "perf_profile")]
    {
      self.layer_profile.render_passes += 1;
    }
    self.set_target_viewport(TargetSpace::blur_target(size, width, height));
    let scissor = RECT {
      left: 0,
      top: 0,
      right: size[0].min(width) as i32,
      bottom: size[1].min(height) as i32,
    };
    let drawn = self.draw_layer_blur(source, constants, scissor, BlurDraw::Pass);
    self.transition_resource(
      &resource,
      D3D12_RESOURCE_STATE_RENDER_TARGET,
      D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    );
    drawn
  }

  /// Draws one blur pass reading `source`'s texture into the current target.
  unsafe fn draw_layer_blur(
    &mut self,
    source: usize,
    constants: &LayerBlurPass,
    scissor: RECT,
    draw: BlurDraw,
  ) -> Result<()> {
    let Some(descriptor) = self.layer_texture_descriptor(source) else {
      return Ok(());
    };
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let descriptor_heaps = [Some(self.srv_heap.heap.clone()), Some(self.sampler_heap.heap.clone())];
    self.command_list.SetDescriptorHeaps(&descriptor_heaps);
    self.command_list.SetPipelineState(match draw {
      BlurDraw::Pass => &compositor.blur_pipeline_state,
      BlurDraw::BackdropErase => &compositor.backdrop_erase_pipeline_state,
      BlurDraw::BackdropAdd => &compositor.backdrop_add_pipeline_state,
    });
    self
      .command_list
      .SetGraphicsRootSignature(&compositor.blur_root_signature);
    self.command_list.SetGraphicsRoot32BitConstants(
      0,
      (std::mem::size_of::<LayerBlurPass>() / 4) as u32,
      ptr::from_ref(constants).cast(),
      0,
    );
    self
      .command_list
      .SetGraphicsRootDescriptorTable(1, self.srv_heap.gpu_handle(descriptor));
    self.command_list.RSSetScissorRects(std::slice::from_ref(&scissor));
    self
      .command_list
      .IASetVertexBuffers(0, Some(std::slice::from_ref(&self.quad_buffers.vertex_view)));
    self
      .command_list
      .DrawIndexedInstanced(crate::render::gpu::QuadVertex::INDICES.len() as u32, 1, 0, 0, 0);
    #[cfg(feature = "perf_profile")]
    {
      self.draw_calls += 1;
      self.layer_profile.blur_draws += 1;
    }
    Ok(())
  }

  /// A shader-visible descriptor of `slot`'s texture, from this frame's
  /// layer range. `None` (and nothing drawn) once the range is used up.
  unsafe fn layer_texture_descriptor(&mut self, slot: usize) -> Option<usize> {
    let frame_index = self.frame_index;
    let compositor = self.layer_compositor.as_mut()?;
    if compositor.next_srv >= LAYER_SRVS_PER_FRAME {
      tracing::error!(
        "dx12: more than {LAYER_SRVS_PER_FRAME} layer textures read in one frame; the rest are not drawn"
      );
      return None;
    }
    let descriptor = layer_srv_start(frame_index) + compositor.next_srv;
    compositor.next_srv += 1;
    let resource = compositor.slots[slot].resource.clone();
    let srv_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: RENDER_TARGET_FORMAT,
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
      .CreateShaderResourceView(&resource, Some(&srv_desc), self.srv_heap.cpu_handle(descriptor));
    Some(descriptor)
  }

  /// Draws `layer` into the current target. It was painted into `slot` as
  /// the frame's target `target`; a blurred layer is drawn from what
  /// [`Self::prepare_layer_blur`] left of it.
  pub(super) unsafe fn composite_layer(&mut self, target: usize, slot: usize, layer: &LayerCmd) -> Result<()> {
    let parent = self.space;
    let Some(scissor) = super::scissor_rect(parent.layer_rect(layer.bounds), parent.width, parent.height) else {
      return Ok(());
    };
    let blur_slots = self
      .layer_compositor
      .as_ref()
      .and_then(|compositor| compositor.blur_slots.get(target).copied().flatten());
    if let (Some(slots), Some(compositor)) = (blur_slots, self.layer_compositor.as_ref()) {
      let constants = LayerBlurPass::composite(layer, &slots.plan, &parent, compositor.size(slots.blurred));
      return self.draw_layer_blur(slots.blurred, &constants, scissor, BlurDraw::Pass);
    }
    let Some(descriptor) = self.layer_texture_descriptor(slot) else {
      return Ok(());
    };

    let constants = LayerComposite::new(layer, &parent);
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let descriptor_heaps = [Some(self.srv_heap.heap.clone()), Some(self.sampler_heap.heap.clone())];
    self.command_list.SetDescriptorHeaps(&descriptor_heaps);
    self.command_list.SetPipelineState(&compositor.pipeline_state);
    self.command_list.SetGraphicsRootSignature(&compositor.root_signature);
    self.command_list.SetGraphicsRoot32BitConstants(
      0,
      (std::mem::size_of::<LayerComposite>() / 4) as u32,
      ptr::from_ref(&constants).cast(),
      0,
    );
    self
      .command_list
      .SetGraphicsRootDescriptorTable(1, self.srv_heap.gpu_handle(descriptor));
    self.command_list.RSSetScissorRects(std::slice::from_ref(&scissor));
    self
      .command_list
      .IASetVertexBuffers(0, Some(std::slice::from_ref(&self.quad_buffers.vertex_view)));
    self
      .command_list
      .DrawIndexedInstanced(crate::render::gpu::QuadVertex::INDICES.len() as u32, 1, 0, 0, 0);
    #[cfg(feature = "perf_profile")]
    {
      self.draw_calls += 1;
    }
    Ok(())
  }
}
