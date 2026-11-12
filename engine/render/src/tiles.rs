//! Tiled light culling.
//!
//! A compute pass projects every light's bounding sphere onto the screen and records, per
//! 16x16-pixel tile, a bitmask of the lights that can touch it (at most 32, one `u32`). The scene
//! shader then loops only over the set bits. Pixels in the sky or in unlit corners pay for zero
//! lights; this is what keeps many-light scenes cheap on tile-based mobile GPUs.

#[cfg(any(feature = "hot-reload", test))]
use crate::shaders::create_module;
#[cfg(any(feature = "hot-reload", test))]
use crate::{RenderError, shaders::ShaderLibrary};

pub const TILE: u32 = 16;

pub struct Tiles {
    pipeline: wgpu::ComputePipeline,
    #[cfg(any(feature = "hot-reload", test))]
    pipeline_layout: wgpu::PipelineLayout,
    write_bgl: wgpu::BindGroupLayout,
    buf: wgpu::Buffer,
    write_bg: wgpu::BindGroup,
    dims: (u32, u32),
}

fn storage_entry(read_only: bool, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: vis,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl Tiles {
    pub(crate) fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tiles-write"),
            entries: &[storage_entry(false, wgpu::ShaderStages::COMPUTE)],
        })
    }

    pub fn new(
        device: &wgpu::Device,
        globals_bgl: &wgpu::BindGroupLayout,
        write_bgl: wgpu::BindGroupLayout,
        target: (u32, u32),
        tiles_sm: &wgpu::ShaderModule,
    ) -> Tiles {
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tiles"),
            bind_group_layouts: &[Some(globals_bgl), Some(&write_bgl)],
            immediate_size: 0,
        });
        let pipeline = Self::make_pipeline(device, &pipeline_layout, tiles_sm);
        let (buf, write_bg, dims) = Self::alloc(device, &write_bgl, target);
        Tiles {
            pipeline,
            #[cfg(any(feature = "hot-reload", test))]
            pipeline_layout,
            write_bgl,
            buf,
            write_bg,
            dims,
        }
    }

    fn make_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        module: &wgpu::ShaderModule,
    ) -> wgpu::ComputePipeline {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("tiles"),
            layout: Some(layout),
            module,
            entry_point: Some("cull"),
            compilation_options: Default::default(),
            cache: None,
        })
    }

    /// Rebuilds the compute pipeline after a hot reload of `tiles.wgsl`.
    #[cfg(any(feature = "hot-reload", test))]
    pub(crate) fn rebuild_shader(
        &mut self,
        device: &wgpu::Device,
        library: &ShaderLibrary,
    ) -> Result<wgpu::ShaderModule, RenderError> {
        let module = create_module(device, crate::shaders::ShaderId::Tiles, library)?;
        let new_pl = Self::make_pipeline(device, &self.pipeline_layout, &module);
        self.pipeline = new_pl;
        Ok(module)
    }

    fn alloc(
        device: &wgpu::Device,
        write_bgl: &wgpu::BindGroupLayout,
        target: (u32, u32),
    ) -> (wgpu::Buffer, wgpu::BindGroup, (u32, u32)) {
        let dims = (
            target.0.div_ceil(TILE).max(1),
            target.1.div_ceil(TILE).max(1),
        );
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile-masks"),
            size: u64::from(dims.0) * u64::from(dims.1) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let write_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tile-masks-write"),
            layout: write_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buf.as_entire_binding(),
            }],
        });
        (buf, write_bg, dims)
    }

    /// Reallocate for a new render-target size.
    pub fn resize(&mut self, device: &wgpu::Device, target: (u32, u32)) {
        let (buf, write_bg, dims) = Self::alloc(device, &self.write_bgl, target);
        self.buf = buf;
        self.write_bg = write_bg;
        self.dims = dims;
    }

    /// Changes the active tile grid inside the retained maximum-size mask buffer.
    pub fn set_viewport(&mut self, target: (u32, u32)) {
        self.dims = (
            target.0.div_ceil(TILE).max(1),
            target.1.div_ceil(TILE).max(1),
        );
    }

    /// Tile grid size, for the globals uniform.
    pub fn dims(&self) -> (u32, u32) {
        self.dims
    }

    pub(crate) fn read_buffer(&self) -> &wgpu::Buffer {
        &self.buf
    }

    pub fn cull(&self, enc: &mut wgpu::CommandEncoder, globals: &wgpu::BindGroup) {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("light-cull"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, &self.write_bg, &[]);
        pass.dispatch_workgroups(self.dims.0.div_ceil(8), self.dims.1.div_ceil(8), 1);
    }
}
