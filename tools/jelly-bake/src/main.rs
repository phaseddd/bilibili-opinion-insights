//! jelly-bake —— 离线 headless wgpu 烘焙工具。
//! 移植 jelly-switch 圆角盒胶体 raymarch；按 pressure 烤一组"静止→按下压扁"形变帧。

use anyhow::{Result, anyhow};
use glam::{Mat4, Vec3};

const SIZE: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct JellyUniform {
    view_inv: [[f32; 4]; 4],
    proj_inv: [[f32; 4]; 4],
    light_dir: [f32; 4],
    jelly_color: [f32; 4],
    progress: f32,
    squash_x: f32,
    squash_y: f32,
    squash_z: f32,
    wiggle_x: f32,
    exposure: f32,
    _pad: [f32; 2],
}

fn main() -> Result<()> {
    pollster::block_on(run())
}

async fn run() -> Result<()> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| anyhow!("no headless wgpu adapter found"))?;
    println!("adapter: {:?} ({:?})", adapter.get_info().name, adapter.get_info().backend);

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("jelly-bake-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )
        .await?;

    // ---- 相机（固定机位）----
    let eye = Vec3::new(0.024, 2.7, 1.9);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(26f32.to_radians(), 1.0, 0.1, 10.0);
    let light = Vec3::new(0.19, -0.24, 0.75).normalize();

    let base = JellyUniform {
        view_inv: view.inverse().to_cols_array_2d(),
        proj_inv: proj.inverse().to_cols_array_2d(),
        light_dir: [light.x, light.y, light.z, 0.0],
        jelly_color: [0.08, 0.5, 1.0, 1.0],
        progress: 1.0,
        squash_x: 0.0,
        squash_y: 0.0,
        squash_z: 0.0,
        wiggle_x: 0.0,
        exposure: 1.5,
        _pad: [0.0; 2],
    };

    // ---- 资源（创建一次，多帧复用）----
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniform"),
        size: std::mem::size_of::<JellyUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bg"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pl"),
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view_tex = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("jelly"),
        source: wgpu::ShaderSource::Wgsl(include_str!("jelly.wgsl").into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    let bpp = 4u32;
    let unpadded = SIZE * bpp;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    std::fs::create_dir_all("tmp")?;

    // ---- pressure 形变序列：press 0→1，胶体压扁变宽变矮 ----
    let frames = [
        ("rest", 0.0f32),
        ("press-33", 0.33),
        ("press-66", 0.66),
        ("press-100", 1.0),
    ];
    for (name, press) in frames {
        let u = JellyUniform {
            squash_x: press * 0.22,
            squash_y: press * 0.34,
            squash_z: press * 0.18,
            ..base
        };
        queue.write_buffer(&uniform_buf, 0, bytemuck::bytes_of(&u));

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rp"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_tex,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&pipeline);
            rp.set_bind_group(0, &bind_group, &[]);
            rp.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv()??;

        let data = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((SIZE * SIZE * bpp) as usize);
        for row in 0..SIZE {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        drop(data);
        readback.unmap();

        let out = format!("tmp/jelly-bake-{name}.png");
        image::RgbaImage::from_raw(SIZE, SIZE, pixels)
            .ok_or_else(|| anyhow!("raw buffer -> image failed"))?
            .save(&out)?;
        println!("wrote {out}");
    }
    Ok(())
}
