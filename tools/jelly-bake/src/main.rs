//! jelly-bake —— 离线 headless wgpu 烘焙工具。
//! 移植 jelly-switch 圆角盒胶体 raymarch，离线烤横长厚胶按钮资产。
//! 当前：遍历 tone（primary/cyan/warning/neutral）各烤一张静止厚胶，
//! 直接写运行时资产 assets/jelly/button_{tone}_rest.png（primary 即 atlas 当前所用）。

use anyhow::{Result, anyhow};
use glam::{Mat4, Vec3};

// 输出横长比例匹配 GUI Standard 按钮（360×66 = 60:11），2x 取样保清晰。
const OUT_W: u32 = 720;
const OUT_H: u32 = 132;

// 各 tone 的厚胶本征色（饱和纯色，供 raymarch 透光/吸收），贴近 GUI tone 语义。
const TONES: [(&str, [f32; 3]); 4] = [
    ("primary", [0.08, 0.5, 1.0]),
    ("cyan", [0.06, 0.78, 0.86]),
    ("warning", [1.0, 0.62, 0.12]),
    ("neutral", [0.62, 0.7, 0.8]),
];

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
    resolution: [f32; 2],
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
    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

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

    // ---- 相机（固定机位，aspect 跟随横长输出）----
    let eye = Vec3::new(0.024, 2.7, 1.9);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let aspect = OUT_W as f32 / OUT_H as f32;
    let proj = Mat4::perspective_rh(17f32.to_radians(), aspect, 0.1, 10.0);
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
        resolution: [OUT_W as f32, OUT_H as f32],
    };

    // ---- 资源 ----
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
        size: wgpu::Extent3d {
            width: OUT_W,
            height: OUT_H,
            depth_or_array_layers: 1,
        },
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
    let unpadded = OUT_W * bpp;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * OUT_H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    std::fs::create_dir_all("assets/jelly")?;
    std::fs::create_dir_all("tmp")?;

    // 遍历 tone，各烤一张静止厚胶。
    for (name, color) in TONES {
        let u = JellyUniform {
            jelly_color: [color[0], color[1], color[2], 1.0],
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
                    rows_per_image: Some(OUT_H),
                },
            },
            wgpu::Extent3d {
                width: OUT_W,
                height: OUT_H,
                depth_or_array_layers: 1,
            },
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
        let mut pixels = Vec::with_capacity((OUT_W * OUT_H * bpp) as usize);
        for row in 0..OUT_H {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&data[start..start + unpadded as usize]);
        }
        drop(data);
        readback.unmap();

        // tmp 预览：深色 UI 合成，肉眼对比各 tone 厚胶质感。
        let bg = [28u8, 32, 42];
        let composed: Vec<u8> = pixels
            .chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as f32 / 255.0;
                [
                    (p[0] as f32 * a + bg[0] as f32 * (1.0 - a)) as u8,
                    (p[1] as f32 * a + bg[1] as f32 * (1.0 - a)) as u8,
                    (p[2] as f32 * a + bg[2] as f32 * (1.0 - a)) as u8,
                    255,
                ]
            })
            .collect();
        image::RgbaImage::from_raw(OUT_W, OUT_H, composed)
            .ok_or_else(|| anyhow!("compose failed"))?
            .save(format!("tmp/jelly-bake-{name}-on-ui.png"))?;

        // 运行时资产（透明背景厚胶），编译期被主 crate include_bytes! 嵌入。
        let asset = format!("assets/jelly/button_{name}_rest.png");
        image::RgbaImage::from_raw(OUT_W, OUT_H, pixels)
            .ok_or_else(|| anyhow!("raw buffer -> image failed"))?
            .save(&asset)?;
        println!("wrote {asset}");
    }

    Ok(())
}
