use super::*;
impl Renderer {
  pub(super) fn draw(&mut self, device: &Device, queue: &Queue, id: CanvasId, prepared: &Prepared) {
    let _record_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _record_phase = self
      .profile_context
      .as_ref()
      .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasCommands));
    let depth = layer_depth(prepared);
    self.reserve_layers(device, depth);
    let Some(backing) = self.surfaces.get(&id) else {
      return;
    };
    let (width, height) = (backing.image.texture.width(), backing.image.texture.height());
    let mut uploaded = 0;
    if prepared.draws().filter_map(|d| d.asset.as_ref()).any(|a| {
      a.width > device.limits().max_texture_dimension_2d || a.height > device.limits().max_texture_dimension_2d
    }) {
      if let Some(canvas) = backing.owner.upgrade() {
        canvas.set_gpu_error(CanvasError::SurfaceTooLarge);
      }
      return;
    }
    let _asset_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _asset_phase = self
      .profile_context
      .as_ref()
      .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasAssetUpload));
    for asset in prepared.draws().filter_map(|draw| draw.asset.as_ref()) {
      if self.assets.get(asset.id).is_none() {
        let image = Texture::new(
          device,
          &self.image_layout,
          &self.nearest,
          &self.linear,
          asset.width,
          asset.height,
        );
        let mut converted = Vec::new();
        let data = if asset.premultiplied {
          asset.data.as_slice()
        } else {
          converted.extend_from_slice(&asset.data);
          for p in converted.chunks_exact_mut(4) {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
              *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
          }
          &converted
        };
        queue.write_texture(
          image.texture.as_image_copy(),
          data,
          TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(asset.width * 4),
            rows_per_image: Some(asset.height),
          },
          Extent3d {
            width: asset.width,
            height: asset.height,
            depth_or_array_layers: 1,
          },
        );
        uploaded += asset.data.len();
        // Only textures no recent frame drew are evicted, so none this draw uses.
        self.assets.insert(asset.id, image, asset.data.len(), drop);
      }
    }
    let backing = &self.surfaces[&id];
    profile_if! {
      self.profile.asset_upload += profile_elapsed!(_asset_start);
      drop(_asset_phase);
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    #[cfg(feature = "perf_profile")]
    gpu_timing::begin_canvas(&mut self.gpu_marks, &mut encoder);
    if prepared.clear {
      let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("canvas clear"),
        color_attachments: &[Some(RenderPassColorAttachment {
          view: &backing.image.view,
          depth_slice: None,
          resolve_target: None,
          ops: Operations {
            load: LoadOp::Clear(Color::TRANSPARENT),
            store: StoreOp::Store,
          },
        })],
        ..Default::default()
      });
    }
    let tiles = prepared.tiles(width, height);
    if !tiles.is_empty() {
      // One constant slot per tile, then one per layer boundary carrying that
      // layer's own blend mode and alpha.
      let alignment = device.limits().min_uniform_buffer_offset_alignment as usize / 4;
      let mut slots: HashMap<usize, usize> = HashMap::new();
      let mut constants = vec![0f32; tiles.len() * alignment];
      for (index, tile) in tiles.iter().enumerate() {
        constants[index * alignment..index * alignment + 8].copy_from_slice(&[
          tile[0] as f32,
          tile[1] as f32,
          TILE as f32,
          TILE as f32,
          width as f32,
          height as f32,
          0.,
          0.,
        ]);
      }
      for (index, step) in prepared.steps.iter().enumerate() {
        if let Step::Begin { alpha, blend, .. } = step {
          slots.insert(index, constants.len() / alignment);
          constants.resize(constants.len() + alignment, 0.);
          let base = constants.len() - alignment;
          constants[base..base + 8].copy_from_slice(&[0., 0., 1., 1., 1., 1., blend.index() as f32, *alpha]);
        }
      }
      let _globals_start = profile_scope!();
      let globals = global_group(
        device,
        &self.globals_layout,
        self.globals.write(device, queue, &constants).unwrap(),
      );
      profile_if! { self.profile.buffer_upload += profile_elapsed!(_globals_start); }
      let blend_groups: Vec<BindGroup> = (0..depth)
        .map(|level| {
          blend_group(
            device,
            &self.blend_layout,
            &self.layer.as_ref().unwrap().view,
            &self.saved[level].view,
            &self.nearest,
          )
        })
        .collect();
      let _vertices_start = profile_scope!();
      let vertices = self.vertices.write(device, queue, &prepared.vertices).unwrap();
      profile_if! { self.profile.buffer_upload += profile_elapsed!(_vertices_start); }
      for (index, tile) in tiles.iter().copied().enumerate() {
        let tile_offset = (index * alignment * 4) as u32;
        let mut step = 0usize;
        let mut level = 0usize;
        let mut stack: Vec<(usize, BlendMode)> = Vec::new();
        let mut seed = Seed::Surface(&backing.image);
        let mut composite: Option<(usize, u32, BlendMode)> = None;
        loop {
          {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
              label: Some("canvas dirty tile"),
              color_attachments: &[Some(RenderPassColorAttachment {
                view: &self.scratch_view,
                depth_slice: None,
                resolve_target: Some(&self.resolve_view),
                ops: Operations {
                  load: LoadOp::Clear(Color::TRANSPARENT),
                  store: StoreOp::Discard,
                },
              })],
              depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &self.stencil_view,
                depth_ops: None,
                stencil_ops: Some(Operations {
                  load: LoadOp::Clear(0),
                  store: StoreOp::Discard,
                }),
              }),
              ..Default::default()
            });
            pass.set_scissor_rect(0, 0, tile[2], tile[3]);
            pass.set_bind_group(0, &globals, &[tile_offset]);
            match seed {
              // The backing is surface-sized, so it is sampled through the tile
              // rectangle; a saved layer tile is already the size of the tile.
              Seed::Surface(texture) => {
                pass.set_bind_group(1, &texture.nearest, &[]);
                pass.set_pipeline(&self.seed);
                pass.draw(0..3, 0..1);
              }
              Seed::Tile(texture) => {
                pass.set_bind_group(1, &texture.nearest, &[]);
                pass.set_pipeline(&self.restore);
                pass.draw(0..3, 0..1);
              }
              Seed::Nothing => {}
            }
            if let Some((at, slot, blend)) = composite.take() {
              pass.set_bind_group(0, &globals, &[slot]);
              if blend.is_normal() {
                pass.set_bind_group(1, &self.layer.as_ref().unwrap().nearest, &[]);
                pass.set_pipeline(&self.compose);
              } else {
                pass.set_bind_group(1, &blend_groups[at], &[]);
                pass.set_pipeline(&self.blend);
              }
              pass.draw(0..3, 0..1);
              pass.set_bind_group(0, &globals, &[tile_offset]);
            }
            pass.set_vertex_buffer(0, vertices.slice(..));
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
                    pass.set_bind_group(1, &self.white.nearest, &[]);
                    pass.set_pipeline(&self.reset_stencil);
                    pass.set_stencil_reference(0);
                    pass.draw(0..3, 0..1);
                    pass.set_pipeline(&self.clip);
                    for (stencil, range) in draw.clips.iter().enumerate() {
                      pass.set_stencil_reference(stencil as u32);
                      pass.draw(range.clone(), 0..1);
                    }
                    clips = Some(&draw.clips);
                  }
                  pass.set_stencil_reference(draw.clips.len() as u32);
                  pass.set_pipeline(match draw.kind {
                    _ if draw.erase => &self.erase,
                    DrawKind::Solid => &self.solid,
                    DrawKind::Image => &self.premul,
                    DrawKind::Gradient(GradientKind::Linear) => &self.gradient_linear,
                    DrawKind::Gradient(GradientKind::Radial) => &self.gradient_radial,
                    DrawKind::Gradient(GradientKind::Angular) => &self.gradient_angular,
                  });
                  let texture = draw
                    .asset
                    .as_ref()
                    .map(|a| self.assets.peek(a.id).expect("uploaded by this draw"))
                    .unwrap_or(&self.white);
                  pass.set_bind_group(1, if draw.smooth { &texture.linear } else { &texture.nearest }, &[]);
                  pass.draw(draw.vertices.clone(), 0..1);
                }
              }
            }
          }
          if step >= prepared.steps.len() {
            break;
          }
          match &prepared.steps[step] {
            Step::Begin { blend, .. } => {
              copy_tile(&mut encoder, &self.resolve, &self.saved[level].texture, tile);
              stack.push((slots[&step], *blend));
              level += 1;
              seed = Seed::Nothing;
              step += 1;
            }
            Step::End => {
              copy_tile(&mut encoder, &self.resolve, &self.layer.as_ref().unwrap().texture, tile);
              level -= 1;
              let (slot, blend) = stack.pop().unwrap();
              seed = if blend.is_normal() {
                Seed::Tile(&self.saved[level])
              } else {
                Seed::Nothing
              };
              composite = Some((level, (slot * alignment * 4) as u32, blend));
              step += 1;
            }
            Step::Draw(_) => unreachable!("a draw never ends a pass"),
          }
        }
        encoder.copy_texture_to_texture(
          self.resolve.as_image_copy(),
          TexelCopyTextureInfo {
            origin: Origin3d {
              x: tile[0],
              y: tile[1],
              z: 0,
            },
            ..backing.image.texture.as_image_copy()
          },
          Extent3d {
            width: tile[2],
            height: tile[3],
            depth_or_array_layers: 1,
          },
        );
      }
    }
    #[cfg(feature = "perf_profile")]
    gpu_timing::end_canvas(&mut self.gpu_marks, &mut encoder);
    let _submit_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _submit_phase = self
      .profile_context
      .as_ref()
      .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasSubmission));
    queue.submit([encoder.finish()]);
    profile_if! {
      self.profile.submit += profile_elapsed!(_submit_start);
      self.profile.recording += profile_elapsed!(_record_start);
      self.profile.vertices += prepared.vertices.len();
      self.profile.tiles += tiles.len();
      self.profile.uploaded_asset_bytes += uploaded;
    }
    if let Some(canvas) = backing.owner.upgrade() {
      canvas.record_gpu_update(prepared.vertices.len(), tiles.len(), uploaded);
    }
  }
}
/// What a tile pass starts from.
#[derive(Clone, Copy)]
enum Seed<'a> {
  /// The canvas backing, sampled through this tile's rectangle.
  Surface(&'a Texture),
  /// A saved tile-sized copy, sampled one to one.
  Tile(&'a Texture),
  /// Nothing: an isolated layer starts transparent, and a blended composite
  /// writes every channel of the tile itself.
  Nothing,
}
