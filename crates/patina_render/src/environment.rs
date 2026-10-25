//! Lighting environments for image-based lighting.
//!
//! Analytic presets and HDRI images go through the same path:
//! source → cubemap (+ mips) → GGX-prefiltered specular cube, SH9 irradiance.
//! A split-sum BRDF lookup table is computed once per renderer.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use patina_core::HdrImage;

use crate::settings::{ENV_PRESETS, EnvPreset};

const IBL_WGSL: &str = include_str!("shaders/ibl.wgsl");
const ENV_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const SPEC_SIZE: u32 = 256;
const SPEC_LEVELS: u32 = 6;
const BRDF_LUT_SIZE: u32 = 128;
const PREFILTER_SAMPLES: f32 = 256.0;

/// Where the viewport lighting comes from.
#[derive(Clone)]
pub enum EnvSource {
    /// Index into [`ENV_PRESETS`].
    Preset(usize),
    /// An HDRI image (equirectangular) and a display name.
    Hdri { image: Arc<HdrImage>, name: String },
}

impl EnvSource {
    pub fn name(&self) -> &str {
        match self {
            EnvSource::Preset(i) => ENV_PRESETS[(*i).min(ENV_PRESETS.len() - 1)].name,
            EnvSource::Hdri { name, .. } => name,
        }
    }

    pub fn preset_index(&self) -> Option<usize> {
        match self {
            EnvSource::Preset(i) => Some(*i),
            EnvSource::Hdri { .. } => None,
        }
    }

    fn key(&self) -> EnvKey {
        match self {
            EnvSource::Preset(i) => EnvKey::Preset(*i),
            EnvSource::Hdri { image, .. } => EnvKey::Hdri(Arc::as_ptr(image) as usize),
        }
    }
}

impl Default for EnvSource {
    fn default() -> Self {
        EnvSource::Preset(0)
    }
}

impl PartialEq for EnvSource {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl std::fmt::Debug for EnvSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvSource::Preset(i) => write!(f, "Preset({i})"),
            EnvSource::Hdri { name, image } => write!(f, "Hdri({name}, {image:?})"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnvKey {
    Preset(usize),
    Hdri(usize),
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AnalyticUniforms {
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    sky_ground: [f32; 4],
    lights: [[f32; 4]; 8],
    count: [u32; 4],
}

impl AnalyticUniforms {
    fn new(p: &EnvPreset) -> Self {
        let rgb = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
        let mut lights = [[0.0; 4]; 8];
        for (i, l) in p.lights.iter().take(4).enumerate() {
            let d = Vec3::from(l.direction).normalize_or(Vec3::Y);
            lights[i * 2] = [d.x, d.y, d.z, l.sharpness];
            lights[i * 2 + 1] = rgb(l.color);
        }
        Self {
            sky_top: rgb(p.sky_top),
            sky_horizon: rgb(p.sky_horizon),
            sky_ground: rgb(p.sky_ground),
            lights,
            count: [p.lights.len().min(4) as u32, 0, 0, 0],
        }
    }
}

/// Compute pipelines and shared resources for building environments.
pub(crate) struct Ibl {
    equirect: wgpu::ComputePipeline,
    analytic: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    prefilter: wgpu::ComputePipeline,
    sh_partial: wgpu::ComputePipeline,
    sh_sum: wgpu::ComputePipeline,
    brdf_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    /// Layout of the environment bind group used by the scene shader.
    pub layout: wgpu::BindGroupLayout,
}

/// A built environment, ready to bind.
pub(crate) struct Environment {
    key: EnvKey,
    pub bind_group: wgpu::BindGroup,
    /// Highest mip of the specular cube (roughness 1).
    pub spec_max_lod: f32,
    _textures: Vec<wgpu::Texture>,
    _buffers: Vec<wgpu::Buffer>,
}

impl Environment {
    pub fn matches(&self, source: &EnvSource) -> bool {
        self.key == source.key()
    }
}

fn section(name: &str) -> String {
    let marker = |n: &str| format!("//--- {n}");
    let body = |n: &str| {
        let start = IBL_WGSL.find(&marker(n)).expect("missing shader section");
        let rest = &IBL_WGSL[start..];
        let end = rest[3..]
            .find("//--- ")
            .map(|e| e + 3)
            .unwrap_or(rest.len());
        rest[..end].to_owned()
    };
    format!("{}\n{}", body("common"), body(name))
}

fn compute(device: &wgpu::Device, section_name: &str, entry: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(section_name),
        source: wgpu::ShaderSource::Wgsl(section(section_name).into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: None,
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn groups(size: u32) -> u32 {
    size.div_ceil(8)
}

fn cube_texture(device: &wgpu::Device, label: &str, size: u32, levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ENV_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn mip_view(texture: &wgpu::Texture, mip: u32) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        base_mip_level: mip,
        mip_level_count: Some(1),
        ..Default::default()
    })
}

fn cube_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}

fn bind(
    device: &wgpu::Device,
    pipeline: &wgpu::ComputePipeline,
    entries: &[wgpu::BindGroupEntry],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries,
    })
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    group: &wgpu::BindGroup,
    x: u32,
    y: u32,
    z: u32,
) {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.dispatch_workgroups(x, y, z);
}

impl Ibl {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let brdf = compute(device, "brdf", "brdf_lut");
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("brdf lut"),
            size: wgpu::Extent3d {
                width: BRDF_LUT_SIZE,
                height: BRDF_LUT_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ENV_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let brdf_view = lut.create_view(&Default::default());
        let group = bind(
            device,
            &brdf,
            &[entry(0, wgpu::BindingResource::TextureView(&brdf_view))],
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        dispatch(
            &mut encoder,
            &brdf,
            &group,
            groups(BRDF_LUT_SIZE),
            groups(BRDF_LUT_SIZE),
            1,
        );
        queue.submit([encoder.finish()]);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let tex = |binding, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("environment"),
            entries: &[
                tex(0, wgpu::TextureViewDimension::Cube),
                tex(1, wgpu::TextureViewDimension::Cube),
                tex(2, wgpu::TextureViewDimension::D2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let _keep_lut_alive = lut;
        Self {
            equirect: compute(device, "equirect", "equirect_to_cube"),
            analytic: compute(device, "analytic", "analytic_to_cube"),
            downsample: compute(device, "downsample", "downsample"),
            prefilter: compute(device, "prefilter", "prefilter"),
            sh_partial: compute(device, "sh", "sh_partial"),
            sh_sum: compute(device, "sh", "sh_sum"),
            brdf_view,
            sampler,
            layout,
        }
    }

    /// Builds the GPU resources for `source`. The work is submitted to the
    /// queue and completes asynchronously before the next draw.
    pub fn build(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &EnvSource,
    ) -> Environment {
        let max_dim = device.limits().max_texture_dimension_2d;
        let size = match source {
            EnvSource::Preset(_) => 512,
            EnvSource::Hdri { image, .. } => (image.width / 4)
                .next_power_of_two()
                .clamp(64, 1024)
                .min(max_dim),
        };
        let src_levels = size.ilog2() + 1;
        let src = cube_texture(device, "environment", size, src_levels);
        let mut textures = Vec::new();
        let mut buffers = Vec::new();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("build environment"),
        });

        // 1. Source → cube mip 0.
        let mip0 = mip_view(&src, 0);
        match source {
            EnvSource::Preset(i) => {
                let preset = &ENV_PRESETS[(*i).min(ENV_PRESETS.len() - 1)];
                let uniforms = wgpu::util::DeviceExt::create_buffer_init(
                    device,
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("analytic env"),
                        contents: bytemuck::bytes_of(&AnalyticUniforms::new(preset)),
                        usage: wgpu::BufferUsages::UNIFORM,
                    },
                );
                let group = bind(
                    device,
                    &self.analytic,
                    &[
                        entry(0, uniforms.as_entire_binding()),
                        entry(1, wgpu::BindingResource::TextureView(&mip0)),
                    ],
                );
                dispatch(
                    &mut encoder,
                    &self.analytic,
                    &group,
                    groups(size),
                    groups(size),
                    6,
                );
                buffers.push(uniforms);
            }
            EnvSource::Hdri { image, .. } => {
                let mut img = image.as_ref().clone();
                while img.width > max_dim || img.height > max_dim {
                    img = img.downsample();
                }
                let equirect = wgpu::util::DeviceExt::create_texture_with_data(
                    device,
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some("hdri"),
                        size: wgpu::Extent3d {
                            width: img.width,
                            height: img.height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba32Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    bytemuck::cast_slice(&img.data),
                );
                let view = equirect.create_view(&Default::default());
                let group = bind(
                    device,
                    &self.equirect,
                    &[
                        entry(0, wgpu::BindingResource::TextureView(&view)),
                        entry(1, wgpu::BindingResource::TextureView(&mip0)),
                    ],
                );
                dispatch(
                    &mut encoder,
                    &self.equirect,
                    &group,
                    groups(size),
                    groups(size),
                    6,
                );
                textures.push(equirect);
            }
        }

        // 2. Mip chain.
        for mip in 1..src_levels {
            let from = mip_view(&src, mip - 1);
            let to = mip_view(&src, mip);
            let group = bind(
                device,
                &self.downsample,
                &[
                    entry(0, wgpu::BindingResource::TextureView(&from)),
                    entry(1, wgpu::BindingResource::TextureView(&to)),
                ],
            );
            let s = (size >> mip).max(1);
            dispatch(
                &mut encoder,
                &self.downsample,
                &group,
                groups(s),
                groups(s),
                6,
            );
        }

        // 3. SH9 irradiance from the 32x32 (or smaller) mip.
        let sh_mip = src_levels.saturating_sub(6).min(src_levels - 1);
        let sh_view = mip_view(&src, sh_mip);
        let partials = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sh partials"),
            size: 256 * 9 * 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let sh = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sh9"),
            size: 9 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let group = bind(
            device,
            &self.sh_partial,
            &[
                entry(0, wgpu::BindingResource::TextureView(&sh_view)),
                entry(1, partials.as_entire_binding()),
            ],
        );
        dispatch(&mut encoder, &self.sh_partial, &group, 1, 1, 1);
        let group = bind(
            device,
            &self.sh_sum,
            &[
                entry(1, partials.as_entire_binding()),
                entry(2, sh.as_entire_binding()),
            ],
        );
        dispatch(&mut encoder, &self.sh_sum, &group, 1, 1, 1);

        // 4. Prefiltered specular cube, one roughness per mip.
        let spec = cube_texture(device, "specular environment", SPEC_SIZE, SPEC_LEVELS);
        let src_cube = cube_view(&src);
        for mip in 0..SPEC_LEVELS {
            let roughness = mip as f32 / (SPEC_LEVELS - 1) as f32;
            let params = wgpu::util::DeviceExt::create_buffer_init(
                device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("prefilter params"),
                    contents: bytemuck::cast_slice(&[
                        roughness,
                        size as f32,
                        PREFILTER_SAMPLES,
                        0.0,
                    ]),
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let to = mip_view(&spec, mip);
            let group = bind(
                device,
                &self.prefilter,
                &[
                    entry(0, wgpu::BindingResource::TextureView(&src_cube)),
                    entry(1, wgpu::BindingResource::Sampler(&self.sampler)),
                    entry(2, wgpu::BindingResource::TextureView(&to)),
                    entry(3, params.as_entire_binding()),
                ],
            );
            let s = SPEC_SIZE >> mip;
            dispatch(
                &mut encoder,
                &self.prefilter,
                &group,
                groups(s),
                groups(s),
                6,
            );
            buffers.push(params);
        }
        queue.submit([encoder.finish()]);

        let spec_cube = cube_view(&spec);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment"),
            layout: &self.layout,
            entries: &[
                entry(0, wgpu::BindingResource::TextureView(&src_cube)),
                entry(1, wgpu::BindingResource::TextureView(&spec_cube)),
                entry(2, wgpu::BindingResource::TextureView(&self.brdf_view)),
                entry(3, wgpu::BindingResource::Sampler(&self.sampler)),
                entry(4, sh.as_entire_binding()),
            ],
        });
        textures.extend([src, spec]);
        buffers.extend([partials, sh]);
        Environment {
            key: source.key(),
            bind_group,
            spec_max_lod: (SPEC_LEVELS - 1) as f32,
            _textures: textures,
            _buffers: buffers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_the_common_header() {
        for name in [
            "equirect",
            "analytic",
            "downsample",
            "prefilter",
            "sh",
            "brdf",
        ] {
            let s = section(name);
            assert!(s.contains("fn cube_dir"), "{name}");
            assert!(s.contains(&format!("//--- {name}")), "{name}");
            // Sections must not leak into each other.
            assert_eq!(s.matches("//--- ").count(), 2, "{name}");
        }
    }

    #[test]
    fn sources_compare_by_identity() {
        let img = Arc::new(HdrImage::new(1, 1, vec![[1.0; 4]]).unwrap());
        let a = EnvSource::Hdri {
            image: img.clone(),
            name: "a".into(),
        };
        let b = EnvSource::Hdri {
            image: img,
            name: "renamed".into(),
        };
        assert_eq!(a, b);
        assert_ne!(a, EnvSource::Preset(0));
        assert_eq!(EnvSource::Preset(2), EnvSource::Preset(2));
        assert_eq!(EnvSource::Preset(3).name(), "Sunset");
    }
}
