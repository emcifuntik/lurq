//! Offscreen targets for flattened group opacity and layer blur (see
//! [`crate::layout::opacity_layer`]) and the pipelines that composite them.
//!
//! A blurred layer takes two or three more textures from the same pool for the
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
//!
//! The finished blur of a backdrop is kept out of the pool for as long as
//! the pixels it was blurred from stay the same (see
//! [`crate::layout::backdrop_reuse`]): a frame that finds it kept puts it
//! back with two draws, and takes no other texture for that blur.

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
  backdrop_reuse::{BackdropCache, BackdropInputs, BackdropUse},
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
  /// The backdrop blurs whose finished texture is kept, by the textures'
  /// ids.
  backdrops: BackdropCache,
  /// The id of the texture created last.
  last_texture: u64,
  /// What the frame does for each of its backdrop blurs, and the textures
  /// the cache gave back: scratch of [`Self::acquire_backdrops`].
  backdrop_uses: Vec<Option<BackdropUse>>,
  released: Vec<u64>,
  /// Backdrop blurs the frame put back from a kept texture.
  backdrops_reused: usize,
}

/// The textures a blurred layer's passes paint, beside the one it is painted
/// into: the reduced copy of a wide blur, the horizontal pass's result, and
/// the vertical pass's, which is the finished blur the pass compositing the
/// layer reads.
#[derive(Clone, Copy)]
struct BlurSlots {
  blur: LayerBlur,
  plan: BlurPlan,
  reduced: Option<usize>,
  horizontal: usize,
  blurred: usize,
}

/// The textures of a backdrop blur: the finished blur, kept from an earlier
/// frame or painted by this one, and for a blur that runs the copy of its
/// target's pixels and the textures of the passes between.
#[derive(Clone, Copy)]
struct BackdropSlots {
  region: BackdropRegion,
  plan: BlurPlan,
  blurred: usize,
  /// `None` when the kept blur is put back as it is.
  work: Option<BackdropWork>,
}

/// The textures a backdrop blur paints on its way to the finished one.
#[derive(Clone, Copy)]
struct BackdropWork {
  blur: LayerBlur,
  copied: usize,
  reduced: Option<usize>,
  horizontal: usize,
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
  /// Names the texture to what outlives a frame: slots move as the pool
  /// retires others.
  id: u64,
  resource: ID3D12Resource,
  rtv_heap: CpuDescriptorHeap,
  width: u32,
  height: u32,
  in_use: bool,
  last_used: u64,
  /// Holds a kept backdrop blur: no frame takes it for anything else, and
  /// it is not retired for being idle.
  kept: bool,
}

impl LayerSlot {
  /// The bytes the texture holds, at four a texel.
  fn bytes(&self) -> usize {
    self.width as usize * self.height as usize * 4
  }
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
      backdrops: BackdropCache::default(),
      last_texture: 0,
      backdrop_uses: Vec::new(),
      released: Vec::new(),
      backdrops_reused: 0,
    })
  }

  /// The textures the passes of `blur` paint this frame.
  unsafe fn acquire_blur(&mut self, device: &ID3D12Device, blur: &LayerBlur) -> Result<BlurSlots> {
    let plan = BlurPlan::new(blur);
    let reduced = match plan.reduction > 1 {
      true => Some(self.acquire(device, plan.size[0], plan.size[1])?),
      false => None,
    };
    let horizontal = self.acquire(device, plan.size[0], plan.size[1])?;
    let blurred = self.acquire(device, plan.size[0], plan.size[1])?;
    Ok(BlurSlots {
      blur: *blur,
      plan,
      reduced,
      horizontal,
      blurred,
    })
  }

  /// Takes the textures of the frame's backdrop blurs (`backdrop_slots`, by
  /// the blurs' index; `None` for one without a region): `regions` are the
  /// blurs' regions and `inputs` what each of them reads, in a window of
  /// `window` pixels. A blur whose kept result is still the blur of what it
  /// reads takes that texture alone; any other takes the textures of its
  /// passes, and the texture kept for its place or a new one for the result,
  /// which is kept from then on if the cache has room for it.
  unsafe fn acquire_backdrops(
    &mut self,
    device: &ID3D12Device,
    regions: &[Option<BackdropRegion>],
    inputs: &BackdropInputs,
    window: [u32; 2],
  ) -> Result<()> {
    let mut uses = std::mem::take(&mut self.backdrop_uses);
    let mut released = std::mem::take(&mut self.released);
    self.backdrops.begin_frame(
      regions,
      inputs,
      BackdropCache::budget_of(window),
      &mut uses,
      &mut released,
    );
    // Given back to the pool, free for this frame.
    for slot in &mut self.slots {
      if slot.kept && released.contains(&slot.id) {
        slot.kept = false;
        slot.in_use = false;
      }
    }
    self.backdrop_slots.clear();
    self.backdrops_reused = 0;
    for (index, (region, used)) in regions.iter().zip(&uses).enumerate() {
      let (Some(region), Some(used)) = (region, used) else {
        self.backdrop_slots.push(None);
        continue;
      };
      let Some(blur) = region.layer.blur else {
        self.backdrop_slots.push(None);
        continue;
      };
      let plan = BlurPlan::new(&blur);
      let kept = match used {
        BackdropUse::Reuse { texture } | BackdropUse::Refresh { texture } => {
          self.slots.iter().position(|slot| slot.id == *texture)
        }
        BackdropUse::Blur { .. } => None,
      };
      let work = match (used, kept) {
        (BackdropUse::Reuse { .. }, Some(_)) => None,
        _ => Some(BackdropWork {
          blur,
          copied: self.acquire(device, blur.width, blur.height)?,
          reduced: match plan.reduction > 1 {
            true => Some(self.acquire(device, plan.size[0], plan.size[1])?),
            false => None,
          },
          horizontal: self.acquire(device, plan.size[0], plan.size[1])?,
        }),
      };
      let blurred = match kept {
        Some(slot) => slot,
        None => {
          let slot = self.acquire_to_keep(device, plan.size[0], plan.size[1])?;
          let (id, bytes) = (self.slots[slot].id, self.slots[slot].bytes());
          if matches!(used, BackdropUse::Blur { keep: true }) && self.backdrops.keep(inputs, index, id, bytes) {
            self.slots[slot].kept = true;
          }
          slot
        }
      };
      self.backdrops_reused += usize::from(work.is_none());
      self.backdrop_slots.push(Some(BackdropSlots {
        region: *region,
        plan,
        blurred,
        work,
      }));
    }
    self.backdrop_uses = uses;
    self.released = released;
    Ok(())
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
    self.take(device, fitting, width, height)
  }

  /// A free texture for a `width` x `height` blur that may be kept: one of
  /// the size a new one would have, so that a small blur does not keep a
  /// large texture out of the pool.
  unsafe fn acquire_to_keep(&mut self, device: &ID3D12Device, width: u32, height: u32) -> Result<usize> {
    let size = slot_size(width, height);
    let fitting = self
      .slots
      .iter()
      .position(|slot| !slot.in_use && [slot.width, slot.height] == size);
    self.take(device, fitting, width, height)
  }

  /// Takes the free texture `fitting` for this frame, or a new one for a
  /// `width` x `height` layer.
  unsafe fn take(&mut self, device: &ID3D12Device, fitting: Option<usize>, width: u32, height: u32) -> Result<usize> {
    let index = match fitting {
      Some(index) => index,
      None => {
        let [width, height] = slot_size(width, height);
        self.last_texture += 1;
        self.slots.push(create_slot(device, self.last_texture, width, height)?);
        self.slots.len() - 1
      }
    };
    let slot = &mut self.slots[index];
    slot.in_use = true;
    slot.last_used = self.frame;
    Ok(index)
  }
}

/// The size of a new texture for a `width` x `height` layer.
fn slot_size(width: u32, height: u32) -> [u32; 2] {
  [width, height].map(|value| {
    value
      .div_ceil(LAYER_SIZE_STEP)
      .saturating_mul(LAYER_SIZE_STEP)
      .min(MAX_TEXTURE_DIMENSION)
  })
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

unsafe fn create_slot(device: &ID3D12Device, id: u64, width: u32, height: u32) -> Result<LayerSlot> {
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
    id,
    resource,
    rtv_heap,
    width,
    height,
    in_use: false,
    last_used: 0,
    kept: false,
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

  /// Backdrop blurs the last frame put back from a kept texture, then the
  /// textures kept.
  pub(super) fn backdrop_reuse(&self) -> (usize, usize) {
    (self.backdrops_reused, self.backdrops.len())
  }
}

/// The first SRV descriptor of `frame_index`'s layer range.
fn layer_srv_start(frame_index: usize) -> usize {
  SRV_DESCRIPTOR_COUNT as usize + frame_index * LAYER_SRVS_PER_FRAME
}

impl Dx12State {
  /// Starts a frame whose plan has layers or backdrop blurs, or that follows
  /// one that kept a backdrop blur: takes a texture for each target that is
  /// a layer (`None` for the window), and the textures of each backdrop blur
  /// (`backdrops` are their regions, `None` for one that changes nothing, and
  /// `inputs` what each reads in a window of `window` pixels).
  pub(super) unsafe fn begin_layer_frame(
    &mut self,
    layers: &[Option<&LayerCmd>],
    backdrops: &[Option<BackdropRegion>],
    inputs: &BackdropInputs,
    window: [u32; 2],
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
      // A kept blur is not the frame's to paint over, and is in use for as
      // long as it is kept.
      slot.in_use = slot.kept;
      if slot.kept {
        slot.last_used = compositor.frame;
      }
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
    compositor.acquire_backdrops(&self.device, backdrops, inputs, window)
  }

  /// Whether a backdrop blur is kept: the next frame has to ask for it, or
  /// give it back.
  pub(super) fn keeps_backdrops(&self) -> bool {
    self
      .layer_compositor
      .as_ref()
      .is_some_and(|compositor| compositor.backdrops.len() > 0)
  }

  /// The frame's commands were handed to the GPU: the backdrop blurs it ran
  /// into kept textures can be put back from them.
  pub(super) fn layer_frame_submitted(&mut self) {
    if let Some(compositor) = self.layer_compositor.as_mut() {
      compositor.backdrops.commit();
    }
  }

  /// Keeps no backdrop blur any longer: their textures go back to the pool.
  pub(super) fn forget_backdrops(&mut self) {
    let Some(compositor) = self.layer_compositor.as_mut() else {
      return;
    };
    let mut released = std::mem::take(&mut compositor.released);
    released.clear();
    compositor.backdrops.clear(&mut released);
    for slot in &mut compositor.slots {
      if slot.kept && released.contains(&slot.id) {
        slot.kept = false;
      }
    }
    compositor.released = released;
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
      .partition(|slot| slot.in_use || slot.kept || frame.saturating_sub(slot.last_used) <= LAYER_IDLE_FRAMES);
    compositor.slots = kept;
    compositor.retired[frame_index].extend(idle);
    #[cfg(feature = "perf_profile")]
    {
      self.layer_profile.textures = compositor.slots.len();
      self.layer_profile.texture_bytes = compositor.slots.iter().map(LayerSlot::bytes).sum();
      self.layer_profile.backdrops_reused = compositor.backdrops_reused;
      self.layer_profile.kept_textures = compositor.backdrops.len();
      self.layer_profile.kept_texture_bytes = compositor.backdrops.bytes();
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
  /// reduction of a wide blur, then the horizontal Gaussian and the vertical
  /// one. Does nothing for a layer without a blur.
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
    let constants = LayerBlurPass::horizontal(&slots.plan, compositor.size(source), compositor.size(slots.horizontal));
    self.run_blur_pass(source, slots.horizontal, &constants, slots.plan.size)?;
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let constants = LayerBlurPass::vertical(
      &slots.plan,
      compositor.size(slots.horizontal),
      compositor.size(slots.blurred),
    );
    self.run_blur_pass(slots.horizontal, slots.blurred, &constants, slots.plan.size)
  }

  /// Blurs the backdrop `index` of the frame on the current target, in the
  /// middle of painting it, and puts the result back inside the node's box:
  /// the result an earlier frame kept, or, for a blur that runs, that of the
  /// pixels it reads out of the target now. `layer` is the layer the target
  /// paints and the texture it paints into; `None` for the window.
  pub(super) unsafe fn blur_backdrop(&mut self, index: usize, layer: Option<(&LayerCmd, usize)>) -> Result<()> {
    let parent = self.space;
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let Some(slots) = compositor.backdrop_slots.get(index).copied().flatten() else {
      return Ok(());
    };
    // A blur that runs fills the texture it is kept in, whether or not any
    // of it shows.
    if let Some(work) = slots.work {
      self.run_backdrop_blur(&slots, work, layer)?;
    }
    let Some(scissor) = super::scissor_rect(
      parent.layer_rect(slots.region.layer.bounds),
      parent.width,
      parent.height,
    ) else {
      return Ok(());
    };
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let constants = LayerBlurPass::backdrop(&slots.region, &slots.plan, &parent, compositor.size(slots.blurred));
    self.draw_layer_blur(slots.blurred, &constants, scissor, BlurDraw::BackdropErase)?;
    self.draw_layer_blur(slots.blurred, &constants, scissor, BlurDraw::BackdropAdd)
  }

  /// Copies the pixels a backdrop blur reads out of the current target and
  /// blurs the copy into `slots.blurred`, then makes the target current
  /// again.
  unsafe fn run_backdrop_blur(
    &mut self,
    slots: &BackdropSlots,
    work: BackdropWork,
    layer: Option<(&LayerCmd, usize)>,
  ) -> Result<()> {
    let parent = self.space;
    let Some(compositor) = self.layer_compositor.as_ref() else {
      return Ok(());
    };
    let (target, target_rtv, target_origin) = match layer {
      Some((layer, slot)) => {
        let slot = &compositor.slots[slot];
        (slot.resource.clone(), slot.rtv_heap.cpu_handle(0), layer.painted().0)
      }
      None => (self.current_render_target(), self.current_rtv_handle(), [0, 0]),
    };
    let copied = compositor.slots[work.copied].resource.clone();
    let blur = work.blur;

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

    let passes = BlurSlots {
      blur,
      plan: slots.plan,
      reduced: work.reduced,
      horizontal: work.horizontal,
      blurred: slots.blurred,
    };
    let prepared = self.prepare_blur(work.copied, passes);
    // Back to the target, as it was: the blur's passes painted elsewhere.
    self.command_list.OMSetRenderTargets(1, Some(&target_rtv), false, None);
    self.set_target_viewport(parent);
    #[cfg(feature = "perf_profile")]
    {
      self.layer_profile.render_passes += 1;
      self.layer_profile.target_copies += 1;
      self.layer_profile.backdrops_blurred += 1;
    }
    prepared
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
