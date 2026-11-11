use super::*;
impl Renderer {
  pub unsafe fn encode(&mut self, state: &mut Dx12State, canvases: &[CanvasHandle]) -> Result<()> {
    profile_if! { self.profile = Default::default(); }
    profile_if! {
      self.profile.asset_upload_details = CanvasAssetUploadProfile::capture(
        self.profile_context.as_ref().is_some_and(|context| context.capture_active()),
        self.assets.bytes(),
        self.assets.len(),
      );
    }
    let _process_start = profile_scope!();
    #[cfg(feature = "perf_profile")]
    let _phase = self
      .profile_context
      .as_ref()
      .map(|context| context.phase(crate::app::profiler::Phase::CanvasBackend));
    self.descriptor = 0;
    self.frames.begin_encode();
    let live: HashSet<_> = canvases
      .iter()
      .filter(|c| c.is_attached())
      .map(CanvasHandle::surface_id)
      .collect();
    let closed: Vec<_> = self
      .surfaces
      .keys()
      .chain(self.fronts.keys())
      .copied()
      .filter(|id| !live.contains(id))
      .collect();
    for id in closed {
      if let Some((_, front)) = self.frames.end_replacement(&mut self.fronts, id) {
        self.retire_back(state, id, front);
      }
      if let Some(back) = self.surfaces.remove(&id) {
        if let Some(owner) = back.owner.upgrade() {
          owner.set_gpu_bytes(0);
        }
        self.retire_back(state, id, back);
      }
      self.rejected.remove(&id);
    }
    self.reap(state, &live);
    state.command_list.SetDescriptorHeaps(&[
      Some(self.srvs[state.frame_index].heap.clone()),
      Some(self.samplers.heap.clone()),
    ]);
    state.command_list.SetGraphicsRootSignature(&self.root);
    state
      .command_list
      .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    for canvas in canvases {
      let Some(mut batch) = canvas.take_batch() else {
        continue;
      };
      self.frames.encodes(canvas);
      profile_if! { self.profile.batches += 1; }
      let mut commands: VecDeque<_> = batch.take_commands().into();
      self.batches.push(batch);
      if !self.surfaces.contains_key(&canvas.surface_id()) && !matches!(commands.front(), Some(Command::Resize { .. }))
      {
        let metrics = canvas.metrics();
        self.resize(state, canvas, metrics.pixel_width, metrics.pixel_height, false)?;
      }
      while let Some(command) = commands.pop_front() {
        if self.rejected.contains(&canvas.surface_id())
          && !matches!(
            command,
            Command::BeginPresentation(..)
              | Command::CommitPresentation(..)
              | Command::AbortPresentation(_)
              | Command::Readback(..)
              | Command::ForgetArtwork
          )
        {
          continue;
        }
        match command {
          Command::BeginPresentation(token, front_revision) => {
            if canvas.presentation_current(token) {
              self.begin_presentation(state, canvas, token, front_revision)?;
            } else {
              self.rejected.insert(canvas.surface_id());
            }
          }
          Command::CommitPresentation(token, revision) => self.end_presentation(state, canvas, token, true, revision),
          Command::AbortPresentation(token) => self.end_presentation(state, canvas, token, false, 0),
          Command::ForgetArtwork => self.forget_artwork(state, canvas),
          Command::CaptureArtwork => self.capture_artwork(state, canvas)?,
          Command::RetainedArtwork(matrix) => self.draw_artwork(state, canvas, matrix)?,
          Command::Resize {
            width,
            height,
            preserve,
          } => {
            self.abort_for_resize(state, canvas);
            if !self.surfaces.contains_key(&canvas.surface_id()) && commands.is_empty() {
              continue;
            }
            self.resize(state, canvas, width, height, preserve)?;
          }
          Command::Readback(done, metrics, revision, complete) => {
            let id = canvas.surface_id();
            if let Some(b) = self.fronts.get(&id).map(|(_, b)| b).or_else(|| self.surfaces.get(&id)) {
              let readback = readback(
                state,
                &b.texture,
                b.width,
                b.height,
                done,
                if complete || self.fronts.contains_key(&id) || self.rejected.contains(&id) {
                  b.revision
                } else {
                  revision
                },
              )?;
              self.readbacks.push(readback);
              // An explicit submission boundary keeps the readback copy ahead
              // of subsequent rendering/fast clears of the same texture.
              state.command_list.Close()?;
              let _submit_start = profile_scope!();
              state
                .command_queue
                .ExecuteCommandLists(&[Some(state.command_list.cast()?)]);
              profile_if! { self.profile.submit += profile_elapsed!(_submit_start); }
              self.flush_stats();
              state.command_list.Reset(
                &state.command_allocators[state.frame_index],
                None::<&ID3D12PipelineState>,
              )?;
              state.command_list.SetDescriptorHeaps(&[
                Some(self.srvs[state.frame_index].heap.clone()),
                Some(self.samplers.heap.clone()),
              ]);
              state.command_list.SetGraphicsRootSignature(&self.root);
              state
                .command_list
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            } else if metrics.pixel_width != 0 && metrics.pixel_height != 0 {
              done.finish(Err(canvas.status().error.unwrap_or(CanvasError::RendererLost)));
            } else {
              done.finish(Ok(CanvasSnapshot {
                width: metrics.pixel_width,
                height: metrics.pixel_height,
                rgba: Vec::new(),
                revision,
              }));
            }
          }
          first => {
            let mut group = vec![first];
            while commands.front().is_some_and(|c| {
              !matches!(
                c,
                Command::Resize { .. }
                  | Command::Readback(..)
                  | Command::BeginPresentation(..)
                  | Command::CommitPresentation(..)
                  | Command::AbortPresentation(_)
                  | Command::ForgetArtwork
                  | Command::CaptureArtwork
                  | Command::RetainedArtwork(_)
              )
            }) {
              group.push(commands.pop_front().unwrap());
            }
            let before = self.meshes.stats();
            let _tessellation_start = profile_scope!();
            #[cfg(feature = "perf_profile")]
            let _tessellation_phase = self
              .profile_context
              .as_ref()
              .and_then(|context| context.detail_phase(crate::app::profiler::Phase::CanvasTessellation));
            let prepared = Prepared::new(&group, &mut self.meshes);
            profile_if! {
              self.profile.tessellation += profile_elapsed!(_tessellation_start);
              self.profile.command_groups += 1;
              drop(_tessellation_phase);
            }
            canvas.record_mesh_cache(before, self.meshes.stats());
            match prepared {
              Ok(prepared) => self.draw(state, canvas.surface_id(), &prepared)?,
              Err(error) => canvas.set_gpu_error(error),
            }
          }
        }
      }
    }
    #[cfg(feature = "perf_profile")]
    let _eviction_start = CanvasAssetUploadProfile::start_timer(self.profile.asset_upload_details.as_ref());
    let frame_ends = self.frames.end_encode(&self.fronts);
    // Retired textures stay alive until the GPU has finished this frame.
    let retired = &mut state.canvas_retired[state.frame_index];
    let _frame = self.assets.finish_encode(frame_ends, |texture| retired.push(texture));
    profile_if! {
      self.profile.asset_cache_budget_bytes = self.assets.budget();
      self.profile.asset_cache_bytes = self.assets.bytes();
      self.profile.asset_cache_stretch_bytes = _frame.stretch_bytes;
      self.profile.asset_cache_uncached = _frame.uncached;
      self.profile.asset_cache_uncached_bytes = _frame.uncached_bytes;
      if let Some(detail) = self.profile.asset_upload_details.as_mut() {
        detail.cache_evictions += _frame.evictions;
        detail.add_stage(AssetUploadStage::CacheEviction, _eviction_start);
        detail.cache_state(self.assets.bytes(), self.assets.len());
      }
    }
    profile_if! { self.profile.total = profile_elapsed!(_process_start); }
    Ok(())
  }
  pub fn submitted(&mut self) {
    self.flush_stats();
    for batch in self.batches.drain(..) {
      batch.submit();
    }
  }
  fn flush_stats(&mut self) {
    for (owner, vertices, tiles, uploaded) in self.pending_stats.drain(..) {
      if let Some(canvas) = owner.upgrade() {
        canvas.record_gpu_update(vertices, tiles, uploaded);
      }
    }
  }
  pub fn finish_readbacks(&mut self, fence: &ID3D12Fence, value: u64) {
    for readback in self.readbacks.drain(..) {
      let fence = fence.clone();
      std::thread::spawn(move || unsafe {
        let result = (|| -> Result<CanvasSnapshot> {
          let event = CreateEventW(None, false, false, None)?;
          let wait = (|| -> Result<()> {
            fence.SetEventOnCompletion(value, event)?;
            if WaitForSingleObject(event, 30_000) != WAIT_OBJECT_0 {
              return Err(dx12_invalid_arg("canvas readback fence timeout".to_owned()));
            }
            Ok(())
          })();
          let _ = CloseHandle(event);
          wait?;
          let size = readback.pitch as usize * readback.height as usize;
          let mut mapped = ptr::null_mut();
          readback
            .buffer
            .Map(0, Some(&D3D12_RANGE { Begin: 0, End: size }), Some(&mut mapped))?;
          let source = std::slice::from_raw_parts(mapped.cast::<u8>(), size);
          let mut rgba = Vec::with_capacity(readback.width as usize * readback.height as usize * 4);
          for row in source.chunks_exact(readback.pitch as usize) {
            rgba.extend_from_slice(&row[..readback.width as usize * 4]);
          }
          readback.buffer.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }));
          Ok(CanvasSnapshot {
            width: readback.width,
            height: readback.height,
            rgba: unpremultiply(rgba),
            revision: readback.revision,
          })
        })();
        readback.done.finish(result.map_err(|_| CanvasError::RendererLost));
      });
    }
  }
}
