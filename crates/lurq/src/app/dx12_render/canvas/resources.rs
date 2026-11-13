//! Canvas texture/descriptors and CPU upload/readback transfer recording.
use super::*;

#[cfg(test)]
mod texture_tests;

pub(in super::super) unsafe fn create_srv(
  device: &ID3D12Device,
  texture: &ID3D12Resource,
  handle: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
  device.CreateShaderResourceView(
    texture,
    Some(&D3D12_SHADER_RESOURCE_VIEW_DESC {
      Format: DXGI_FORMAT_R8G8B8A8_UNORM,
      ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
      Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
      Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
        Texture2D: D3D12_TEX2D_SRV {
          MostDetailedMip: 0,
          MipLevels: 1,
          PlaneSlice: 0,
          ResourceMinLODClamp: 0.,
        },
      },
    }),
    handle,
  );
}
pub(super) unsafe fn texture(
  device: &ID3D12Device,
  width: u32,
  height: u32,
  samples: u32,
  stencil: bool,
  initial: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource> {
  let usage = if stencil {
    TextureUsage::DepthStencil
  } else {
    TextureUsage::RenderTarget
  };
  allocate_texture(device, width, height, samples, usage, initial)
}

/// Driver-reported allocation size of the exact presentation resource description.
pub(super) unsafe fn target_allocation_bytes(device: &ID3D12Device, width: u32, height: u32, artwork: bool) -> usize {
  let (desc, _) = texture_definition(
    width,
    height,
    1,
    if artwork {
      TextureUsage::Sampled
    } else {
      TextureUsage::RenderTarget
    },
  );
  usize::try_from(
    device
      .GetResourceAllocationInfo(0, std::slice::from_ref(&desc))
      .SizeInBytes,
  )
  .unwrap_or(usize::MAX)
}
/// Same descriptors as the renderer's tile workspace, including maximum admitted layers.
pub(super) unsafe fn workspace_ceiling_bytes(device: &ID3D12Device) -> usize {
  let charge = |samples, stencil| {
    let (desc, _) = texture_definition(
      TILE,
      TILE,
      samples,
      if stencil {
        TextureUsage::DepthStencil
      } else {
        TextureUsage::RenderTarget
      },
    );
    usize::try_from(
      device
        .GetResourceAllocationInfo(0, std::slice::from_ref(&desc))
        .SizeInBytes,
    )
    .unwrap_or(usize::MAX)
  };
  charge(4, false)
    .saturating_add(charge(4, true))
    .saturating_add(charge(1, false).saturating_mul(MAX_LAYER_DEPTH + 2))
}
pub(super) unsafe fn resource_allocation_bytes(device: &ID3D12Device, resource: &ID3D12Resource) -> usize {
  usize::try_from(
    device
      .GetResourceAllocationInfo(0, std::slice::from_ref(&resource.GetDesc()))
      .SizeInBytes,
  )
  .unwrap_or(usize::MAX)
}
/// Assets are copied to and sampled, never rendered to or fast-cleared. Keep
/// their resource capability narrow without changing the existing cache charge.
pub(super) unsafe fn asset_texture(device: &ID3D12Device, width: u32, height: u32) -> Result<ID3D12Resource> {
  allocate_texture(
    device,
    width,
    height,
    1,
    TextureUsage::Sampled,
    D3D12_RESOURCE_STATE_COPY_DEST,
  )
}

#[derive(Clone, Copy)]
enum TextureUsage {
  Sampled,
  RenderTarget,
  DepthStencil,
}

fn texture_definition(
  width: u32,
  height: u32,
  samples: u32,
  usage: TextureUsage,
) -> (D3D12_RESOURCE_DESC, Option<D3D12_CLEAR_VALUE>) {
  let stencil = matches!(usage, TextureUsage::DepthStencil);
  let desc = D3D12_RESOURCE_DESC {
    Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    Width: width as u64,
    Height: height,
    DepthOrArraySize: 1,
    MipLevels: 1,
    Format: if stencil {
      DXGI_FORMAT_D24_UNORM_S8_UINT
    } else {
      DXGI_FORMAT_R8G8B8A8_UNORM
    },
    SampleDesc: DXGI_SAMPLE_DESC {
      Count: samples,
      Quality: 0,
    },
    Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
    Flags: match usage {
      TextureUsage::Sampled => D3D12_RESOURCE_FLAG_NONE,
      TextureUsage::RenderTarget => D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
      TextureUsage::DepthStencil => D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
    },
    ..Default::default()
  };
  let clear = (!matches!(usage, TextureUsage::Sampled)).then_some(D3D12_CLEAR_VALUE {
    Format: desc.Format,
    Anonymous: if stencil {
      D3D12_CLEAR_VALUE_0 {
        DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1., Stencil: 0 },
      }
    } else {
      D3D12_CLEAR_VALUE_0 { Color: [0.; 4] }
    },
  });
  (desc, clear)
}

unsafe fn allocate_texture(
  device: &ID3D12Device,
  width: u32,
  height: u32,
  samples: u32,
  usage: TextureUsage,
  initial: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource> {
  let (desc, clear) = texture_definition(width, height, samples, usage);
  let mut resource = None;
  device.CreateCommittedResource(
    &D3D12_HEAP_PROPERTIES {
      Type: D3D12_HEAP_TYPE_DEFAULT,
      CreationNodeMask: 1,
      VisibleNodeMask: 1,
      ..Default::default()
    },
    D3D12_HEAP_FLAG_NONE,
    &desc,
    initial,
    clear.as_ref().map(|value| value as *const _),
    &mut resource,
  )?;
  resource.ok_or_else(Error::from_win32)
}
pub(super) unsafe fn viewport(list: &ID3D12GraphicsCommandList, width: u32, height: u32) {
  list.RSSetViewports(&[D3D12_VIEWPORT {
    Width: width as f32,
    Height: height as f32,
    MaxDepth: 1.,
    ..Default::default()
  }]);
  list.RSSetScissorRects(&[RECT {
    left: 0,
    top: 0,
    right: width as i32,
    bottom: height as i32,
  }]);
}
pub(super) fn copy_location(resource: &ID3D12Resource) -> D3D12_TEXTURE_COPY_LOCATION {
  D3D12_TEXTURE_COPY_LOCATION {
    pResource: ManuallyDrop::new(Some(resource.clone())),
    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
  }
}
pub(super) unsafe fn upload_asset(
  state: &mut Dx12State,
  texture: &ID3D12Resource,
  asset: &Asset,
  #[cfg(feature = "perf_profile")] mut profile: Option<&mut CanvasAssetUploadProfile>,
) -> Result<()> {
  #[cfg(feature = "perf_profile")]
  let _packing_start = CanvasAssetUploadProfile::start_timer(profile.as_deref());
  let pitch = (asset.width * 4).div_ceil(256) * 256;
  let mut data = vec![0u8; pitch as usize * asset.height as usize];
  for (src, dst) in asset
    .data
    .chunks_exact(asset.width as usize * 4)
    .zip(data.chunks_exact_mut(pitch as usize))
  {
    dst[..src.len()].copy_from_slice(src);
    if !asset.premultiplied {
      for p in dst[..src.len()].chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        for c in &mut p[..3] {
          *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
      }
    }
  }
  profile_if! {
    if let Some(detail) = profile.as_deref_mut() {
      detail.add_stage(AssetUploadStage::PixelPacking, _packing_start);
    }
  }
  #[cfg(feature = "perf_profile")]
  let _staging_start = CanvasAssetUploadProfile::start_timer(profile.as_deref());
  #[cfg(feature = "perf_profile")]
  let _dedicated_before = profile.as_ref().map(|_| state.frame_uploads[state.frame_index].len());
  let upload = state.upload_frame_bytes(&data, 512)?;
  profile_if! {
    if let Some(detail) = profile.as_deref_mut() {
      let dedicated = state.frame_uploads[state.frame_index].len() > _dedicated_before.unwrap();
      detail.uploaded(data.len(), dedicated);
    }
  }
  let mut dst = copy_location(texture);
  let mut src = D3D12_TEXTURE_COPY_LOCATION {
    pResource: ManuallyDrop::new(Some(upload.resource)),
    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
      PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
        Offset: upload.offset,
        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
          Format: DXGI_FORMAT_R8G8B8A8_UNORM,
          Width: asset.width,
          Height: asset.height,
          Depth: 1,
          RowPitch: pitch,
        },
      },
    },
  };
  state.command_list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
  ManuallyDrop::drop(&mut src.pResource);
  ManuallyDrop::drop(&mut dst.pResource);
  state.transition_resource(
    texture,
    D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
  );
  profile_if! {
    if let Some(detail) = profile {
      detail.add_stage(AssetUploadStage::UploadStagingCommands, _staging_start);
    }
  }
  Ok(())
}
pub(super) unsafe fn readback(
  state: &mut Dx12State,
  texture: &ID3D12Resource,
  width: u32,
  height: u32,
  done: Completion,
  revision: u64,
) -> Result<Readback> {
  let pitch = (width * 4).div_ceil(256) * 256;
  let mut buffer = None;
  state.device.CreateCommittedResource(
    &D3D12_HEAP_PROPERTIES {
      Type: D3D12_HEAP_TYPE_READBACK,
      CreationNodeMask: 1,
      VisibleNodeMask: 1,
      ..Default::default()
    },
    D3D12_HEAP_FLAG_NONE,
    &buffer_resource_desc(u64::from(pitch) * u64::from(height)),
    D3D12_RESOURCE_STATE_COPY_DEST,
    None,
    &mut buffer,
  )?;
  let buffer: ID3D12Resource = buffer.ok_or_else(Error::from_win32)?;
  let mut src = copy_location(texture);
  let mut dst = D3D12_TEXTURE_COPY_LOCATION {
    pResource: ManuallyDrop::new(Some(buffer.clone())),
    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
      PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
        Offset: 0,
        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
          Format: DXGI_FORMAT_R8G8B8A8_UNORM,
          Width: width,
          Height: height,
          Depth: 1,
          RowPitch: pitch,
        },
      },
    },
  };
  state.transition_resource(
    texture,
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
    D3D12_RESOURCE_STATE_COPY_SOURCE,
  );
  state.command_list.CopyTextureRegion(&dst, 0, 0, 0, &src, None);
  ManuallyDrop::drop(&mut src.pResource);
  ManuallyDrop::drop(&mut dst.pResource);
  state.transition_resource(
    texture,
    D3D12_RESOURCE_STATE_COPY_SOURCE,
    D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
  );
  state.canvas_retired[state.frame_index].push(buffer.clone());
  Ok(Readback {
    done,
    buffer,
    width,
    height,
    pitch,
    revision,
  })
}
