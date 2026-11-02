//! Tile/layer draw recording and asset binding for prepared Canvas commands.
use super::{
  resources::{asset_texture, upload_asset, viewport},
  *,
};

impl Renderer {
  pub(super) unsafe fn draw(&mut self, state: &mut Dx12State, id: CanvasId, prepared: &Prepared) -> Result<()> {
    let _record_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _record_phase = self
      .profile_context
      .as_ref()
      .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasCommands));
    let depth = layer_depth(prepared);
    self.reserve_layers(&state.device.clone(), depth)?;
    let Some(b) = self.surfaces.get(&id) else {
      return Ok(());
    };
    let (backing, width, height) = (b.texture.clone(), b.width, b.height);
    let seed_srv = self.srv(state, &backing)?;
    let mut assets = HashMap::new();
    let mut uploaded = 0;
    let _asset_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _asset_phase = self
      .profile_context
      .as_ref()
      .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasAssetUpload));
    for draw in prepared.draws() {
      if let Some(asset) = &draw.asset {
        let cached = self.assets.get(asset.id).cloned();
        let texture = if let Some(texture) = cached {
          profile_if! {
            if let Some(detail) = self.profile.asset_upload_details.as_mut() {
              detail.cache_hits += 1;
            }
          }
          texture
        } else {
          profile_if! {
            if let Some(detail) = self.profile.asset_upload_details.as_mut() {
              detail.cache_misses += 1;
            }
          }
          #[cfg(feature = "perf_profile")]
          let _texture_start = CanvasAssetUploadProfile::start_timer(self.profile.asset_upload_details.as_ref());
          let texture = asset_texture(&state.device, asset.width, asset.height)?;
          profile_if! {
            if let Some(detail) = self.profile.asset_upload_details.as_mut() {
              detail.add_stage(AssetUploadStage::TextureCreation, _texture_start);
              detail.texture_creations += 1;
            }
          }
          upload_asset(
            state,
            &texture,
            asset,
            #[cfg(feature = "perf_profile")]
            self.profile.asset_upload_details.as_mut(),
          )?;
          uploaded += asset.data.len();
          // Only textures no recent frame drew are evicted, so none this draw uses.
          let retired = &mut state.canvas_retired[state.frame_index];
          self
            .assets
            .insert(asset.id, texture.clone(), asset.data.len(), |old| retired.push(old));
          profile_if! {
            if let Some(detail) = self.profile.asset_upload_details.as_mut() {
              detail.cache_state(self.assets.bytes(), self.assets.len());
            }
          }
          texture
        };
        if let std::collections::hash_map::Entry::Vacant(entry) = assets.entry(asset.id) {
          #[cfg(feature = "perf_profile")]
          let _descriptor_start = CanvasAssetUploadProfile::start_timer(self.profile.asset_upload_details.as_ref());
          let srv = self.srv(state, &texture)?;
          profile_if! {
            if let Some(detail) = self.profile.asset_upload_details.as_mut() {
              detail.add_stage(AssetUploadStage::DescriptorWrites, _descriptor_start);
              detail.descriptor_pairs += 1;
            }
          }
          entry.insert(srv);
        }
      }
    }
    profile_if! {
      self.profile.asset_upload += profile_elapsed!(_asset_start);
      drop(_asset_phase);
    }
    if prepared.clear {
      let rtv = self.rtvs.cpu_handle(1);
      state.device.CreateRenderTargetView(&backing, None, rtv);
      state.transition_resource(
        &backing,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
      );
      state.command_list.ClearRenderTargetView(rtv, &[0.; 4], None);
      state.transition_resource(
        &backing,
        D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
      );
    }
    let tiles = prepared.tiles(width, height);
    profile_if! {
      self.profile.vertices += prepared.vertices.len();
      self.profile.tiles += tiles.len();
      self.profile.uploaded_asset_bytes += uploaded;
    }
    self.pending_stats.push((
      self.surfaces[&id].owner.clone(),
      prepared.vertices.len(),
      tiles.len(),
      uploaded,
    ));
    if prepared.vertices.is_empty() {
      profile_if! { self.profile.recording += profile_elapsed!(_record_start); }
      return Ok(());
    }
    let _vertices_start = profile_scope!();
    let vertices = state.upload_frame_pod_slice(&prepared.vertices, 16)?;
    profile_if! { self.profile.buffer_upload += profile_elapsed!(_vertices_start); }
    let rtv = self.rtvs.cpu_handle(0);
    let dsv = self.dsv.cpu_handle(0);
    for tile in tiles {
      let globals = state.upload_frame_constant(&[
        tile[0] as f32,
        tile[1] as f32,
        TILE as f32,
        TILE as f32,
        width as f32,
        height as f32,
        0.,
        0.,
      ])?;
      let mut step = 0usize;
      let mut level = 0usize;
      let mut stack: Vec<(f32, BlendMode)> = Vec::new();
      let mut seed = Some(backing.clone());
      let mut seed_is_surface = true;
      let mut composite: Option<(usize, f32, BlendMode)> = None;
      loop {
        state.command_list.OMSetRenderTargets(1, Some(&rtv), false, Some(&dsv));
        viewport(&state.command_list, TILE, TILE);
        state.command_list.ClearRenderTargetView(rtv, &[0.; 4], None);
        state.command_list.RSSetScissorRects(&[RECT {
          left: 0,
          top: 0,
          right: tile[2] as i32,
          bottom: tile[3] as i32,
        }]);
        state
          .command_list
          .SetGraphicsRootConstantBufferView(0, globals.gpu_address);
        state
          .command_list
          .SetGraphicsRootDescriptorTable(2, self.samplers.gpu_handle(0));
        if let Some(source) = seed.clone() {
          // The backing is surface-sized and sampled through this tile's
          // rectangle; a saved layer tile is already the size of the tile.
          let handle = if seed_is_surface {
            seed_srv
          } else {
            self.srv(state, &source)?
          };
          state.command_list.SetGraphicsRootDescriptorTable(1, handle);
          state
            .command_list
            .SetPipelineState(if seed_is_surface { &self.seed } else { &self.restore });
          state.command_list.DrawInstanced(3, 1, 0, 0);
        }
        if let Some((at, alpha, blend)) = composite.take() {
          let constants = state.upload_frame_constant(&[0., 0., 1., 1., 1., 1., blend.index() as f32, alpha])?;
          state
            .command_list
            .SetGraphicsRootConstantBufferView(0, constants.gpu_address);
          let layer = self.layer.clone().unwrap();
          let handle = if blend.is_normal() {
            self.srv(state, &layer)?
          } else {
            let saved = self.saved[at].clone();
            self.srv_pair(state, &layer, &saved)?
          };
          state.command_list.SetGraphicsRootDescriptorTable(1, handle);
          state
            .command_list
            .SetPipelineState(if blend.is_normal() { &self.compose } else { &self.blend });
          state.command_list.DrawInstanced(3, 1, 0, 0);
          state
            .command_list
            .SetGraphicsRootConstantBufferView(0, globals.gpu_address);
        }
        state
          .command_list
          .IASetVertexBuffers(0, Some(&[vertices.vertex_view::<Vertex>()]));
        let mut clips: Option<&Vec<std::ops::Range<u32>>> = None;
        while step < prepared.steps.len() {
          match &prepared.steps[step] {
            Step::Begin { bounds, end, .. } => {
              if touches(*bounds, tile) {
                break;
              }
              step = end + 1;
            }
            Step::End => break,
            Step::Draw(draw) => {
              step += 1;
              if !draw.intersects(tile) {
                continue;
              }
              if clips != Some(&draw.clips) {
                state
                  .command_list
                  .ClearDepthStencilView(dsv, D3D12_CLEAR_FLAG_STENCIL, 1., 0, None);
                state.command_list.SetPipelineState(&self.clip);
                for (stencil, range) in draw.clips.iter().enumerate() {
                  state.command_list.OMSetStencilRef(stencil as u32);
                  state
                    .command_list
                    .DrawInstanced(range.end - range.start, 1, range.start, 0);
                }
                clips = Some(&draw.clips);
              }
              state.command_list.OMSetStencilRef(draw.clips.len() as u32);
              state.command_list.SetPipelineState(match draw.kind {
                _ if draw.erase => &self.erase,
                DrawKind::Solid => &self.solid,
                DrawKind::Image => &self.image,
                DrawKind::Gradient(GradientKind::Linear) => &self.gradient_linear,
                DrawKind::Gradient(GradientKind::Radial) => &self.gradient_radial,
                DrawKind::Gradient(GradientKind::Angular) => &self.gradient_angular,
              });
              if let Some(asset) = &draw.asset {
                state.command_list.SetGraphicsRootDescriptorTable(1, assets[&asset.id]);
                state
                  .command_list
                  .SetGraphicsRootDescriptorTable(2, self.samplers.gpu_handle(usize::from(draw.smooth)));
              }
              state
                .command_list
                .DrawInstanced(draw.vertices.end - draw.vertices.start, 1, draw.vertices.start, 0);
            }
          }
        }
        self.resolve_tile(state);
        if step >= prepared.steps.len() {
          break;
        }
        match &prepared.steps[step] {
          Step::Begin { alpha, blend, .. } => {
            let saved = self.saved[level].clone();
            self.copy_resolved(state, &saved, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, (0, 0), tile);
            stack.push((*alpha, *blend));
            level += 1;
            seed = None;
            seed_is_surface = false;
            step += 1;
          }
          Step::End => {
            let layer = self.layer.clone().unwrap();
            self.copy_resolved(state, &layer, D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, (0, 0), tile);
            level -= 1;
            let (alpha, blend) = stack.pop().unwrap();
            seed = blend.is_normal().then(|| self.saved[level].clone());
            seed_is_surface = false;
            composite = Some((level, alpha, blend));
            step += 1;
          }
          Step::Draw(_) => unreachable!("a draw never ends a pass"),
        }
      }
      self.copy_resolved(
        state,
        &backing,
        D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE,
        (tile[0], tile[1]),
        tile,
      );
    }
    profile_if! { self.profile.recording += profile_elapsed!(_record_start); }
    Ok(())
  }
}
