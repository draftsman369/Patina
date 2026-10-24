use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use patina_core::{Channel, Image, NormalFormat, Project, TextureSet};
use patina_mesh::{Mesh, Vertex};
use wgpu::util::DeviceExt;

use crate::camera::OrbitCamera;
use crate::settings::{Tonemap, ViewMode, ViewportSettings};
use crate::texture;

pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Viewport output; egui samples it as linear and the readback is sRGB-encoded bytes.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub(crate) const MSAA_SAMPLES: u32 = 4;

const COMMON_WGSL: &str = include_str!("shaders/common.wgsl");
const SCENE_WGSL: &str = include_str!("shaders/scene.wgsl");
const TONEMAP_WGSL: &str = include_str!("shaders/tonemap.wgsl");
const UV_WGSL: &str = include_str!("shaders/uv.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniforms {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    env: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    sky_ground: [f32; 4],
    /// Four lights, each (direction + sharpness, color).
    lights: [[f32; 4]; 8],
    mode: [u32; 4],
    background: [f32; 4],
    wire_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniforms {
    base_color: [f32; 4],
    emissive: [f32; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TonemapUniforms {
    exposure: [f32; 4],
    mode: [u32; 4],
}

/// Mesh buffers on the GPU.
pub struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    /// Unique edges of every part, as a line list.
    edge_buffer: wgpu::Buffer,
    parts: Vec<GpuPart>,
    pub has_uvs: bool,
    pub vertex_count: usize,
    pub triangle_count: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct GpuPart {
    pub material: usize,
    pub first_index: u32,
    pub index_count: u32,
    pub first_edge: u32,
    pub edge_count: u32,
}

impl GpuMesh {
    pub(crate) fn vertex_buffer(&self) -> &wgpu::Buffer {
        &self.vertex_buffer
    }
    pub(crate) fn edge_buffer(&self) -> &wgpu::Buffer {
        &self.edge_buffer
    }
    pub(crate) fn part(&self, i: usize) -> Option<GpuPart> {
        self.parts.get(i).copied()
    }
}

/// Textures and factors of one texture set, bound for drawing.
pub struct GpuMaterial {
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// Kept alive for the bind group.
    _textures: Vec<wgpu::Texture>,
}

impl GpuMaterial {
    pub(crate) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// A mesh plus one material per mesh material slot.
pub struct Scene {
    pub mesh: GpuMesh,
    /// Indexed by mesh material index.
    pub materials: Vec<GpuMaterial>,
}

/// Offscreen targets for one 3D viewport.
pub struct ViewportTarget {
    width: u32,
    height: u32,
    msaa_view: wgpu::TextureView,
    resolve_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    tonemap_buffer: wgpu::Buffer,
    tonemap_bind_group: wgpu::BindGroup,
}

impl ViewportTarget {
    pub fn new(renderer: &Renderer, width: u32, height: u32) -> Self {
        let device = &renderer.device;
        let (width, height) = (width.max(1), height.max(1));
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |label, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let msaa = make("viewport msaa", HDR_FORMAT, MSAA_SAMPLES, attach);
        let resolve = make(
            "viewport hdr",
            HDR_FORMAT,
            1,
            attach | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = make("viewport depth", DEPTH_FORMAT, MSAA_SAMPLES, attach);
        let output = make(
            "viewport output",
            OUTPUT_FORMAT,
            1,
            attach | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let resolve_view = resolve.create_view(&Default::default());

        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniforms"),
            size: size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &renderer.frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let tonemap_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tonemap uniforms"),
            size: size_of::<TonemapUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tonemap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tonemap"),
            layout: &renderer.tonemap_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolve_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: tonemap_buffer.as_entire_binding(),
                },
            ],
        });

        Self {
            width,
            height,
            msaa_view: msaa.create_view(&Default::default()),
            resolve_view,
            depth_view: depth.create_view(&Default::default()),
            output_view: output.create_view(&Default::default()),
            output,
            frame_buffer,
            frame_bind_group,
            tonemap_buffer,
            tonemap_bind_group,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Recreates the targets if the size changed. Returns true when it did,
    /// so callers can re-register the output texture with their UI.
    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return false;
        }
        *self = Self::new(renderer, width, height);
        true
    }

    /// The final tonemapped image (sRGB, 8-bit).
    pub fn output(&self) -> &wgpu::Texture {
        &self.output
    }

    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }
}

pub struct Renderer {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    frame_layout: wgpu::BindGroupLayout,
    pub(crate) material_layout: wgpu::BindGroupLayout,
    tonemap_layout: wgpu::BindGroupLayout,
    pub(crate) uv_layout: wgpu::BindGroupLayout,
    mesh_pipeline: wgpu::RenderPipeline,
    background_pipeline: wgpu::RenderPipeline,
    wire_pipeline: wgpu::RenderPipeline,
    tonemap_pipeline: wgpu::RenderPipeline,
    pub(crate) uv_quad_pipeline: wgpu::RenderPipeline,
    pub(crate) uv_wire_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

pub(crate) const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x2,
    3 => Float32x4,
];

pub(crate) fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// Default bitmap for a channel with no image: neutral so factors apply as-is.
fn default_image(channel: Channel) -> Image {
    match channel {
        Channel::Normal => Image::solid(1, 1, [128, 128, 255, 255]),
        _ => Image::solid(1, 1, [255, 255, 255, 255]),
    }
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[uniform_entry(0, both)],
        });
        let mut material_entries = vec![uniform_entry(0, both)];
        material_entries.extend((1..=5).map(|b| texture_entry(b, true)));
        material_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &material_entries,
        });
        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap"),
            entries: &[
                texture_entry(0, false),
                uniform_entry(1, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let uv_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv view"),
            entries: &[uniform_entry(0, both)],
        });

        let scene_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene.wgsl"),
            source: wgpu::ShaderSource::Wgsl(format!("{COMMON_WGSL}\n{SCENE_WGSL}").into()),
        });
        let tonemap_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tonemap.wgsl"),
            source: wgpu::ShaderSource::Wgsl(TONEMAP_WGSL.into()),
        });
        let uv_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("uv.wgsl"),
            source: wgpu::ShaderSource::Wgsl(format!("{COMMON_WGSL}\n{UV_WGSL}").into()),
        });

        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&frame_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let frame_only_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frame only"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let tonemap_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tonemap"),
                bind_group_layouts: &[Some(&tonemap_layout)],
                immediate_size: 0,
            });
        let uv_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("uv view"),
            bind_group_layouts: &[None, Some(&material_layout), Some(&uv_layout)],
            immediate_size: 0,
        });

        let msaa = wgpu::MultisampleState {
            count: MSAA_SAMPLES,
            ..Default::default()
        };
        let depth = |write: bool, compare| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let hdr_target = |blend: Option<wgpu::BlendState>| {
            [Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&scene_layout),
            vertex: wgpu::VertexState {
                module: &scene_module,
                entry_point: Some("vs_mesh"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Both sides are drawn; the shader flips back-face normals.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_module,
                entry_point: Some("fs_mesh"),
                compilation_options: Default::default(),
                targets: &hdr_target(None),
            }),
            multiview_mask: None,
            cache: None,
        });

        let background_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background"),
            layout: Some(&frame_only_layout),
            vertex: wgpu::VertexState {
                module: &scene_module,
                entry_point: Some("vs_background"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::Always),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_module,
                entry_point: Some("fs_background"),
                compilation_options: Default::default(),
                targets: &hdr_target(None),
            }),
            multiview_mask: None,
            cache: None,
        });

        let wire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("wireframe"),
            layout: Some(&frame_only_layout),
            vertex: wgpu::VertexState {
                module: &scene_module,
                entry_point: Some("vs_wire"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &scene_module,
                entry_point: Some("fs_wire"),
                compilation_options: Default::default(),
                targets: &hdr_target(Some(wgpu::BlendState::ALPHA_BLENDING)),
            }),
            multiview_mask: None,
            cache: None,
        });

        let tonemap_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap"),
            layout: Some(&tonemap_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &tonemap_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &tonemap_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(OUTPUT_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let uv_target = |blend| {
            [Some(wgpu::ColorTargetState {
                format: OUTPUT_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let uv_quad_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("uv quad"),
            layout: Some(&uv_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &uv_module,
                entry_point: Some("vs_quad"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &uv_module,
                entry_point: Some("fs_quad"),
                compilation_options: Default::default(),
                targets: &uv_target(None),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uv_wire_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("uv wireframe"),
            layout: Some(&uv_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &uv_module,
                entry_point: Some("vs_wire"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: msaa,
            fragment: Some(wgpu::FragmentState {
                module: &uv_module,
                entry_point: Some("fs_wire"),
                compilation_options: Default::default(),
                targets: &uv_target(Some(wgpu::BlendState::ALPHA_BLENDING)),
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        Self {
            device: device.clone(),
            queue: queue.clone(),
            frame_layout,
            material_layout,
            tonemap_layout,
            uv_layout,
            mesh_pipeline,
            background_pipeline,
            wire_pipeline,
            tonemap_pipeline,
            uv_quad_pipeline,
            uv_wire_pipeline,
            sampler,
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn upload_mesh(&self, mesh: &Mesh) -> GpuMesh {
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        let mut edges = Vec::new();
        let mut parts = Vec::new();
        for part in &mesh.parts {
            let part_edges = mesh.part_edges(part);
            parts.push(GpuPart {
                material: part.material,
                first_index: part.first_index,
                index_count: part.index_count,
                first_edge: edges.len() as u32,
                edge_count: part_edges.len() as u32,
            });
            edges.extend(part_edges);
        }
        if edges.is_empty() {
            // wgpu rejects zero-sized buffers.
            edges.extend([0, 0]);
        }
        let edge_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh edges"),
                contents: bytemuck::cast_slice(&edges),
                usage: wgpu::BufferUsages::INDEX,
            });
        GpuMesh {
            vertex_buffer,
            index_buffer,
            edge_buffer,
            parts,
            has_uvs: mesh.has_uvs,
            vertex_count: mesh.vertices.len(),
            triangle_count: mesh.triangle_count(),
        }
    }

    fn material_uniforms(
        set: &TextureSet,
        has_uvs: bool,
        normal_format: NormalFormat,
    ) -> MaterialUniforms {
        let e = set.emissive_factor;
        MaterialUniforms {
            base_color: set.base_color_factor,
            emissive: [e[0], e[1], e[2], 0.0],
            params: [
                set.metallic_factor,
                set.roughness_factor,
                match normal_format {
                    NormalFormat::OpenGl => 1.0,
                    NormalFormat::DirectX => -1.0,
                },
                if has_uvs { 1.0 } else { 0.0 },
            ],
        }
    }

    /// Uploads a texture set's bitmaps and factors.
    pub fn create_material(
        &self,
        set: &TextureSet,
        has_uvs: bool,
        normal_format: NormalFormat,
    ) -> GpuMaterial {
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("material uniforms"),
                contents: bytemuck::bytes_of(&Self::material_uniforms(set, has_uvs, normal_format)),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let textures: Vec<wgpu::Texture> = Channel::ALL
            .iter()
            .map(|&c| {
                let fallback;
                let image = match set.image(c) {
                    Some(img) => img,
                    None => {
                        fallback = default_image(c);
                        &fallback
                    }
                };
                texture::upload(&self.device, &self.queue, image, c.is_srgb(), c.label())
            })
            .collect();
        let views: Vec<wgpu::TextureView> = textures
            .iter()
            .map(|t| t.create_view(&Default::default()))
            .collect();
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }];
        for (i, view) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + i as u32,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&set.name),
            layout: &self.material_layout,
            entries: &entries,
        });
        GpuMaterial {
            uniform,
            bind_group,
            _textures: textures,
        }
    }

    /// Updates factors only (cheap: no texture upload).
    pub fn update_material_factors(
        &self,
        material: &GpuMaterial,
        set: &TextureSet,
        has_uvs: bool,
        normal_format: NormalFormat,
    ) {
        self.queue.write_buffer(
            &material.uniform,
            0,
            bytemuck::bytes_of(&Self::material_uniforms(set, has_uvs, normal_format)),
        );
    }

    /// Uploads a mesh and builds a material for each mesh material from the
    /// matching project texture set (or a default one).
    pub fn create_scene(&self, mesh: &Mesh, project: &Project) -> Scene {
        let gpu_mesh = self.upload_mesh(mesh);
        let materials = (0..mesh.materials.len())
            .map(|i| {
                let fallback;
                let set = match project.texture_set_for_material(i) {
                    Some(s) => s,
                    None => {
                        fallback = TextureSet::new("default", i);
                        &fallback
                    }
                };
                self.create_material(set, mesh.has_uvs, project.normal_format)
            })
            .collect();
        Scene {
            mesh: gpu_mesh,
            materials,
        }
    }

    fn frame_uniforms(
        &self,
        target: &ViewportTarget,
        camera: &OrbitCamera,
        settings: &ViewportSettings,
    ) -> FrameUniforms {
        let aspect = target.width as f32 / target.height as f32;
        let view_proj = camera.view_proj(aspect);
        let camera_pos = if camera.orthographic {
            camera.direction().extend(1.0)
        } else {
            camera.eye().extend(0.0)
        };
        let preset = settings.preset();
        let mut lights = [[0.0f32; 4]; 8];
        for (i, l) in preset.lights.iter().take(4).enumerate() {
            let d = Vec3::from(l.direction).normalize_or(Vec3::Y);
            lights[i * 2] = [d.x, d.y, d.z, l.sharpness];
            lights[i * 2 + 1] = [l.color[0], l.color[1], l.color[2], 0.0];
        }
        let (s, c) = settings.env_rotation.sin_cos();
        let rgb = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
        FrameUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera_pos: camera_pos.to_array(),
            env: [
                c,
                s,
                settings.background_sharpness(),
                settings.background_opacity.clamp(0.0, 1.0),
            ],
            sky_top: rgb(preset.sky_top),
            sky_horizon: rgb(preset.sky_horizon),
            sky_ground: rgb(preset.sky_ground),
            lights,
            mode: [
                settings.view_mode.shader_id(),
                preset.lights.len().min(4) as u32,
                0,
                0,
            ],
            background: [0.18, 0.18, 0.18, 0.0],
            wire_color: [0.75, 0.75, 0.75, 0.45],
        }
    }

    /// Renders the 3D viewport into `target`.
    pub fn render_viewport(
        &self,
        target: &ViewportTarget,
        scene: Option<&Scene>,
        camera: &OrbitCamera,
        settings: &ViewportSettings,
    ) {
        self.queue.write_buffer(
            &target.frame_buffer,
            0,
            bytemuck::bytes_of(&self.frame_uniforms(target, camera, settings)),
        );
        let tonemap = TonemapUniforms {
            exposure: [2f32.powf(settings.exposure_ev), 0.0, 0.0, 0.0],
            mode: [
                match settings.tonemap {
                    Tonemap::Aces => 0,
                    Tonemap::Filmic => 1,
                    Tonemap::Linear => 2,
                },
                u32::from(settings.view_mode != ViewMode::Material),
                0,
                0,
            ],
        };
        self.queue
            .write_buffer(&target.tonemap_buffer, 0, bytemuck::bytes_of(&tonemap));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewport"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&target.resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &target.frame_bind_group, &[]);
            pass.set_pipeline(&self.background_pipeline);
            pass.draw(0..3, 0..1);

            if let Some(scene) = scene {
                let mesh = &scene.mesh;
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                for part in &mesh.parts {
                    let Some(material) = scene.materials.get(part.material) else {
                        continue;
                    };
                    pass.set_bind_group(1, &material.bind_group, &[]);
                    pass.draw_indexed(
                        part.first_index..part.first_index + part.index_count,
                        0,
                        0..1,
                    );
                }

                if settings.wireframe {
                    pass.set_pipeline(&self.wire_pipeline);
                    pass.set_index_buffer(mesh.edge_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    for part in &mesh.parts {
                        pass.draw_indexed(
                            part.first_edge..part.first_edge + part.edge_count,
                            0,
                            0..1,
                        );
                    }
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tonemap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.tonemap_pipeline);
            pass.set_bind_group(0, &target.tonemap_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }
}
