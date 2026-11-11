use super::*;
impl Renderer {
  pub fn new(device: &Device, queue: &Queue) -> Self {
    let globals_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
      label: Some("canvas globals"),
      entries: &[BindGroupLayoutEntry {
        binding: 0,
        // The blend composite reads its mode from the same constants.
        visibility: ShaderStages::VERTEX_FRAGMENT,
        ty: BindingType::Buffer {
          ty: BufferBindingType::Uniform,
          has_dynamic_offset: true,
          min_binding_size: BufferSize::new(32),
        },
        count: None,
      }],
    });
    let image_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
      label: Some("canvas image"),
      entries: &[
        BindGroupLayoutEntry {
          binding: 0,
          visibility: ShaderStages::FRAGMENT,
          ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        BindGroupLayoutEntry {
          binding: 1,
          visibility: ShaderStages::FRAGMENT,
          ty: BindingType::Sampler(SamplerBindingType::Filtering),
          count: None,
        },
      ],
    });
    let blend_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
      label: Some("canvas blend"),
      entries: &[
        BindGroupLayoutEntry {
          binding: 0,
          visibility: ShaderStages::FRAGMENT,
          ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
        BindGroupLayoutEntry {
          binding: 1,
          visibility: ShaderStages::FRAGMENT,
          ty: BindingType::Sampler(SamplerBindingType::Filtering),
          count: None,
        },
        BindGroupLayoutEntry {
          binding: 2,
          visibility: ShaderStages::FRAGMENT,
          ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: true },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
          },
          count: None,
        },
      ],
    });
    let nearest = device.create_sampler(&SamplerDescriptor::default());
    let linear = device.create_sampler(&SamplerDescriptor {
      mag_filter: FilterMode::Linear,
      min_filter: FilterMode::Linear,
      ..Default::default()
    });
    let shader = device.create_shader_module(ShaderModuleDescriptor {
      label: Some("canvas"),
      source: ShaderSource::Wgsl(include_str!("../shaders/canvas.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
      label: Some("canvas"),
      bind_group_layouts: &[Some(&globals_layout), Some(&image_layout)],
      immediate_size: 0,
    });
    let blend_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
      label: Some("canvas blend"),
      bind_group_layouts: &[Some(&globals_layout), Some(&blend_layout)],
      immediate_size: 0,
    });
    let make = |vs, fs, samples, mode| pipeline(device, &layout, &shader, vs, fs, samples, mode);
    let seed = make("vs_seed", "fs_premul", 4, 0);
    let resample = make("vs_seed", "fs_premul", 1, 0);
    let reset_stencil = make("vs_seed", "fs_solid", 4, 1);
    let clip = make("vs_main", "fs_solid", 4, 2);
    let solid = make("vs_main", "fs_solid", 4, 3);

    let retained = make("vs_main", "fs_premul", 1, 6);
    let premul = make("vs_main", "fs_premul", 4, 3);
    let erase = make("vs_main", "fs_solid", 4, 4);
    let gradient_linear = make("vs_main", "fs_gradient_linear", 4, 3);
    let gradient_radial = make("vs_main", "fs_gradient_radial", 4, 3);
    let gradient_angular = make("vs_main", "fs_gradient_angular", 4, 3);
    let compose = make("vs_tile", "fs_premul", 4, 5);
    let restore = make("vs_tile", "fs_tile", 4, 6);
    let blend = pipeline(device, &blend_pipeline_layout, &shader, "vs_tile", "fs_blend", 4, 6);
    let texture = |format, samples| {
      device.create_texture(&TextureDescriptor {
        label: Some("canvas shared tile"),
        size: Extent3d {
          width: TILE,
          height: TILE,
          depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT
          | if samples == 1 {
            TextureUsages::COPY_SRC
          } else {
            TextureUsages::empty()
          },
        view_formats: &[],
      })
    };
    let scratch = texture(TextureFormat::Rgba8Unorm, 4);
    let scratch_view = scratch.create_view(&Default::default());
    let resolve = texture(TextureFormat::Rgba8Unorm, 1);
    let resolve_view = resolve.create_view(&Default::default());
    let stencil = texture(TextureFormat::Depth24PlusStencil8, 4);
    let stencil_view = stencil.create_view(&Default::default());
    let white = Texture::new(device, &image_layout, &nearest, &linear, 1, 1);
    queue.write_texture(
      white.texture.as_image_copy(),
      &[255; 4],
      TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(4),
        rows_per_image: Some(1),
      },
      Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
      },
    );
    Self {
      #[cfg(feature = "perf_profile")]
      profile: Default::default(),
      #[cfg(feature = "perf_profile")]
      profile_context: None,
      #[cfg(feature = "perf_profile")]
      gpu_marks: None,
      meshes: MeshCache::default(),
      surfaces: HashMap::new(),
      fronts: HashMap::new(),
      spares: Vec::new(),
      retired_artwork: Vec::new(),
      rejected: HashSet::new(),
      assets: AssetCache::new(CanvasAssetBudget::default()),
      frames: FrameBoundary::default(),
      generation: 0,
      globals_layout,
      image_layout,
      blend_layout,
      nearest,
      linear,
      _scratch: scratch,
      scratch_view,
      resolve,
      resolve_view,
      _stencil: stencil,
      stencil_view,
      white,
      seed,
      resample,
      reset_stencil,
      clip,
      solid,
      premul,
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
      vertices: DynamicBuffer::new("canvas vertices", BufferUsages::VERTEX),
      globals: DynamicBuffer::new("canvas globals", BufferUsages::UNIFORM),
    }
  }
  pub fn with_asset_budget(mut self, budget: CanvasAssetBudget) -> Self {
    self.assets = AssetCache::new(budget);
    self
  }
}
