//! GPU mesh-map baking.
//!
//! The low-poly texture set is rasterized into UV space tile by tile; compute
//! shaders then cast rays against a BVH of the high-poly mesh (or the low
//! poly itself) to produce normal, world normal, ID, AO, curvature, position,
//! thickness, height, opacity and bent normal maps. Work is split into short
//! GPU submissions so long bakes report progress, can be cancelled, and never
//! trigger driver timeouts.

mod dilate;
mod prepare;
mod settings;

use std::sync::atomic::{AtomicBool, Ordering};

use bytemuck::{Pod, Zeroable};
use patina_core::{Image, MeshMap, NormalFormat};
use patina_mesh::Mesh;
use thiserror::Error;
use wgpu::util::DeviceExt;

pub use dilate::dilate;
pub use settings::{BakeSettings, IdSource, MatchMode, match_key};

const BAKE_WGSL: &str = include_str!("bake.wgsl");
/// Tile edge in supersampled texels. Multiple of every supersampling factor.
const TILE: u32 = 512;
/// AO/thickness samples per dispatch.
const SAMPLE_BATCH: u32 = 16;
const RT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const MAP_SLOTS: u64 = MeshMap::COUNT as u64;

#[derive(Debug, Error)]
pub enum BakeError {
    #[error("the low-poly mesh has no UVs")]
    NoUvs,
    #[error("texture set {0} has no triangles")]
    NoSuchPart(usize),
    #[error("the cage has {cage} triangles but the low-poly mesh has {low}")]
    CageMismatch { low: usize, cage: usize },
    #[error("the high-poly mesh is too large for this GPU ({0} MB buffer)")]
    TooLarge(u64),
    #[error("bake cancelled")]
    Cancelled,
    #[error("GPU error: {0}")]
    Gpu(String),
}

/// What to bake: one texture set of `low`, optionally from a separate high-poly mesh.
pub struct BakeInput<'a> {
    pub low: &'a Mesh,
    /// High-poly source. `None` bakes the low-poly mesh onto itself.
    pub high: Option<&'a Mesh>,
    /// Optional cage with the same triangles as `low` (same order).
    pub cage: Option<&'a Mesh>,
    /// Index of the low-poly part (texture set) to bake.
    pub part: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    tile_origin: [u32; 2],
    tile_size: [u32; 2],
    config: [u32; 4],
    samples: [u32; 4],
    distances: [f32; 4],
    bounds_min: [f32; 4],
    bounds_max: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RasterParams {
    tile: [f32; 4],
    config: [f32; 4],
}

pub struct Baker {
    device: wgpu::Device,
    queue: wgpu::Queue,
    raster_geometry: wgpu::RenderPipeline,
    raster_frame: wgpu::RenderPipeline,
    primary: wgpu::ComputePipeline,
    occlusion: wgpu::ComputePipeline,
    resolve: wgpu::ComputePipeline,
}

fn section(name: &str) -> &'static str {
    let marker = format!("//--- {name}");
    let start = BAKE_WGSL
        .find(&marker)
        .expect("missing bake shader section");
    let rest = &BAKE_WGSL[start..];
    let end = rest[3..]
        .find("//--- ")
        .map(|e| e + 3)
        .unwrap_or(rest.len());
    &rest[..end]
}

fn module(device: &wgpu::Device, label: &str, sections: &[&str]) -> wgpu::ShaderModule {
    let source: String = sections
        .iter()
        .map(|s| section(s))
        .collect::<Vec<_>>()
        .join("\n");
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn compute(
    device: &wgpu::Device,
    label: &str,
    sections: &[&str],
    entry: &str,
) -> wgpu::ComputePipeline {
    let m = module(device, label, sections);
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &m,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

const CORNER_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x2,
    3 => Float32x4,
    4 => Float32x4,
    5 => Float32x4,
];

impl Baker {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let raster_module = module(device, "bake raster", &["math", "raster"]);
        let targets = [Some(RT_FORMAT.into()), Some(RT_FORMAT.into())];
        let raster_pipeline = |fragment: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bake raster"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &raster_module,
                    entry_point: Some("vs_raster"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<prepare::Corner>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &CORNER_ATTRIBUTES,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &raster_module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            device: device.clone(),
            queue: queue.clone(),
            raster_geometry: raster_pipeline("fs_geometry"),
            raster_frame: raster_pipeline("fs_frame"),
            primary: compute(
                device,
                "bake primary",
                &["math", "bvh", "primary"],
                "primary",
            ),
            occlusion: compute(
                device,
                "bake occlusion",
                &["math", "bvh", "occlusion"],
                "occlusion",
            ),
            resolve: compute(device, "bake resolve", &["math", "resolve"], "resolve"),
        }
    }

    fn wait(&self) -> Result<(), BakeError> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| BakeError::Gpu(e.to_string()))
    }

    fn storage(&self, label: &str, contents: &[u8]) -> wgpu::Buffer {
        // wgpu rejects zero-sized bindings.
        let padded;
        let contents = if contents.is_empty() {
            padded = [0u8; 16];
            &padded[..]
        } else {
            contents
        };
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
    }

    /// Bakes the requested maps for one texture set.
    ///
    /// `progress` receives values in 0..=1. Setting `cancel` stops the bake
    /// at the next GPU submission with [`BakeError::Cancelled`].
    pub fn bake(
        &self,
        input: &BakeInput<'_>,
        settings: &BakeSettings,
        progress: &mut dyn FnMut(f32),
        cancel: &AtomicBool,
    ) -> Result<Vec<(MeshMap, Image)>, BakeError> {
        let s = settings.sanitized();
        if s.maps.is_empty() {
            return Ok(Vec::new());
        }
        let device = &self.device;
        let low = input.low;
        let high = input.high.unwrap_or(low);

        // CPU preparation.
        let groups = prepare::match_groups(low, high, s.match_mode);
        let corners = prepare::prepare_corners(low, input.part, input.cage, &groups.low, &s)?;
        let high_data = prepare::prepare_high(high, &groups.high, s.id_source);
        let high_bounds = high.bounds();
        let all_bounds = high_bounds.union(&low.bounds());
        let radius = all_bounds.radius().max(1e-6);
        let front = s.max_frontal * radius;
        let rear = s.max_rear * radius;
        let eps = radius * 2e-5;

        let limit = device.limits().max_storage_buffer_binding_size;
        let biggest = [
            (high_data.nodes.len() * 32) as u64,
            (high_data.positions.len() * 16) as u64,
            (high_data.indices.len() * 4) as u64,
        ]
        .into_iter()
        .max()
        .unwrap_or(0);
        if biggest > limit {
            return Err(BakeError::TooLarge(biggest / (1024 * 1024)));
        }

        // GPU buffers.
        let nodes = self.storage("bvh nodes", bytemuck::cast_slice(&high_data.nodes));
        let positions = self.storage("high positions", bytemuck::cast_slice(&high_data.positions));
        let normals = self.storage("high normals", bytemuck::cast_slice(&high_data.normals));
        let indices = self.storage("high indices", bytemuck::cast_slice(&high_data.indices));
        let tri_info = self.storage(
            "high triangle info",
            bytemuck::cast_slice(&high_data.tri_info),
        );
        let corner_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bake corners"),
            contents: bytemuck::cast_slice(&corners),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let ss = s.supersampling;
        let full = s.resolution * ss;
        let tile = TILE.min(full);
        let tile_texels = (tile * tile) as u64;
        let out_tile = tile / ss;
        let out_texels = (out_tile * out_tile) as u64;

        let rt = |label: &str| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: tile,
                        height: tile,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: RT_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let rt_position = rt("bake position");
        let rt_normal = rt("bake normal");
        let rt_tangent = rt("bake tangent");
        let rt_ray = rt("bake ray");

        let buffer = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let maps = buffer(
            "bake maps",
            MAP_SLOTS * tile_texels * 16,
            wgpu::BufferUsages::STORAGE,
        );
        let surface = buffer(
            "bake surface",
            2 * tile_texels * 16,
            wgpu::BufferUsages::STORAGE,
        );
        let accum = buffer(
            "bake accumulation",
            2 * tile_texels * 16,
            wgpu::BufferUsages::STORAGE,
        );
        let out = buffer(
            "bake output",
            MAP_SLOTS * out_texels * 16,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let staging = buffer(
            "bake readback",
            s.maps.len() as u64 * out_texels * 16,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let params_buffer = buffer("bake params", size_of::<Params>() as u64, uniform);
        let raster_params = buffer(
            "bake raster params",
            size_of::<RasterParams>() as u64,
            uniform,
        );

        let tv = wgpu::BindingResource::TextureView;
        let raster_group = |pipeline: &wgpu::RenderPipeline| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bake raster"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[entry(0, raster_params.as_entire_binding())],
            })
        };
        let raster_groups = [
            raster_group(&self.raster_geometry),
            raster_group(&self.raster_frame),
        ];
        let primary_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bake primary"),
            layout: &self.primary.get_bind_group_layout(0),
            entries: &[
                entry(0, nodes.as_entire_binding()),
                entry(1, positions.as_entire_binding()),
                entry(2, indices.as_entire_binding()),
                entry(3, tri_info.as_entire_binding()),
                entry(4, normals.as_entire_binding()),
                entry(5, tv(&rt_position)),
                entry(6, tv(&rt_normal)),
                entry(7, tv(&rt_tangent)),
                entry(8, tv(&rt_ray)),
                entry(9, params_buffer.as_entire_binding()),
                entry(10, maps.as_entire_binding()),
                entry(11, surface.as_entire_binding()),
            ],
        });
        let wants_occlusion = s.maps.iter().any(|m| {
            matches!(
                m,
                MeshMap::AmbientOcclusion | MeshMap::BentNormal | MeshMap::Thickness
            )
        });
        let occlusion_group = wants_occlusion.then(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bake occlusion"),
                layout: &self.occlusion.get_bind_group_layout(0),
                entries: &[
                    entry(0, nodes.as_entire_binding()),
                    entry(1, positions.as_entire_binding()),
                    entry(2, indices.as_entire_binding()),
                    entry(3, tri_info.as_entire_binding()),
                    entry(4, params_buffer.as_entire_binding()),
                    entry(5, surface.as_entire_binding()),
                    entry(6, accum.as_entire_binding()),
                ],
            })
        });
        let resolve_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bake resolve"),
            layout: &self.resolve.get_bind_group_layout(0),
            entries: &[
                entry(0, params_buffer.as_entire_binding()),
                entry(1, maps.as_entire_binding()),
                entry(2, accum.as_entire_binding()),
                entry(3, tv(&rt_position)),
                entry(4, tv(&rt_normal)),
                entry(5, tv(&rt_tangent)),
                entry(6, out.as_entire_binding()),
            ],
        });

        let mask = s.maps.iter().fold(0u32, |m, map| m | 1 << map.index());
        let mut flags = 0u32;
        if s.match_mode == MatchMode::ByName {
            flags |= 1;
        }
        if s.ao_ignore_backfaces {
            flags |= 2;
        }
        if s.normal_format == NormalFormat::DirectX {
            flags |= 4;
        }
        if s.ao_only_same_name && s.match_mode == MatchMode::ByName {
            flags |= 8;
        }
        flags |= (match s.id_source {
            IdSource::VertexColor => 0,
            IdSource::Material => 1,
            IdSource::Object => 2,
        }) << 8;
        let total_samples = {
            let ao = if s
                .maps
                .iter()
                .any(|m| matches!(m, MeshMap::AmbientOcclusion | MeshMap::BentNormal))
            {
                s.ao_samples
            } else {
                0
            };
            let thick = if s.maps.contains(&MeshMap::Thickness) {
                s.thickness_samples
            } else {
                0
            };
            ao.max(thick)
        };
        let params_for = |origin: [u32; 2], size: [u32; 2], first: u32, count: u32| Params {
            tile_origin: origin,
            tile_size: size,
            config: [full, ss, mask, flags],
            samples: [s.ao_samples, first, count, s.thickness_samples],
            distances: [
                s.ao_distance * radius,
                s.thickness_distance * radius,
                front.max(rear).max(eps),
                s.curvature_strength * high_data.mean_edge,
            ],
            bounds_min: high_bounds.min.extend(0.0).to_array(),
            bounds_max: high_bounds.max.extend(eps).to_array(),
        };

        let res = s.resolution as usize;
        let mut images: Vec<Vec<[f32; 4]>> = vec![vec![[0.0; 4]; res * res]; s.maps.len()];
        let tiles_per_axis = full.div_ceil(tile);
        let total_tiles = tiles_per_axis * tiles_per_axis;
        let batches = total_samples.div_ceil(SAMPLE_BATCH).max(1);
        let steps_per_tile = 2 + if wants_occlusion { batches } else { 0 };
        let total_steps = (total_tiles * steps_per_tile) as f32;
        let mut step = 0u32;
        let mut advance = |progress: &mut dyn FnMut(f32)| -> Result<(), BakeError> {
            step += 1;
            progress(step as f32 / total_steps);
            if cancel.load(Ordering::Relaxed) {
                Err(BakeError::Cancelled)
            } else {
                Ok(())
            }
        };
        progress(0.0);

        for ty in 0..tiles_per_axis {
            for tx in 0..tiles_per_axis {
                let origin = [tx * tile, ty * tile];
                let size = [tile.min(full - origin[0]), tile.min(full - origin[1])];
                let groups_xy = [size[0].div_ceil(8), size[1].div_ceil(8)];

                // Rasterize + primary rays.
                self.queue.write_buffer(
                    &raster_params,
                    0,
                    bytemuck::bytes_of(&RasterParams {
                        tile: [
                            origin[0] as f32,
                            origin[1] as f32,
                            size[0] as f32,
                            size[1] as f32,
                        ],
                        config: [full as f32, front, rear, 0.0],
                    }),
                );
                self.queue.write_buffer(
                    &params_buffer,
                    0,
                    bytemuck::bytes_of(&params_for(origin, size, 0, 0)),
                );
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let attachment = |view| {
                        Some(wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })
                    };
                    let passes = [
                        (
                            &self.raster_geometry,
                            &raster_groups[0],
                            [&rt_position, &rt_ray],
                        ),
                        (
                            &self.raster_frame,
                            &raster_groups[1],
                            [&rt_normal, &rt_tangent],
                        ),
                    ];
                    for (pipeline, group, views) in passes {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("bake raster"),
                            color_attachments: &[attachment(views[0]), attachment(views[1])],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, group, &[]);
                        pass.set_vertex_buffer(0, corner_buffer.slice(..));
                        pass.draw(0..corners.len() as u32, 0..1);
                    }
                }
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.primary);
                    pass.set_bind_group(0, &primary_group, &[]);
                    pass.dispatch_workgroups(groups_xy[0], groups_xy[1], 1);
                }
                self.queue.submit([encoder.finish()]);
                self.wait()?;
                advance(progress)?;

                // Secondary rays, in batches.
                if let Some(group) = &occlusion_group {
                    for b in 0..batches {
                        let first = b * SAMPLE_BATCH;
                        let count = SAMPLE_BATCH.min(total_samples - first);
                        self.queue.write_buffer(
                            &params_buffer,
                            0,
                            bytemuck::bytes_of(&params_for(origin, size, first, count)),
                        );
                        let mut encoder = device.create_command_encoder(&Default::default());
                        {
                            let mut pass = encoder.begin_compute_pass(&Default::default());
                            pass.set_pipeline(&self.occlusion);
                            pass.set_bind_group(0, group, &[]);
                            pass.dispatch_workgroups(groups_xy[0], groups_xy[1], 1);
                        }
                        self.queue.submit([encoder.finish()]);
                        self.wait()?;
                        advance(progress)?;
                    }
                }

                // Resolve and read back.
                self.queue.write_buffer(
                    &params_buffer,
                    0,
                    bytemuck::bytes_of(&params_for(origin, size, 0, 0)),
                );
                let out_size = [size[0] / ss, size[1] / ss];
                let n_out = (out_size[0] * out_size[1]) as u64;
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&self.resolve);
                    pass.set_bind_group(0, &resolve_group, &[]);
                    pass.dispatch_workgroups(out_size[0].div_ceil(8), out_size[1].div_ceil(8), 1);
                }
                for (i, map) in s.maps.iter().enumerate() {
                    encoder.copy_buffer_to_buffer(
                        &out,
                        map.index() as u64 * n_out * 16,
                        &staging,
                        i as u64 * n_out * 16,
                        n_out * 16,
                    );
                }
                self.queue.submit([encoder.finish()]);
                let bytes = s.maps.len() as u64 * n_out * 16;
                let slice = staging.slice(..bytes);
                let (tx_done, rx_done) = std::sync::mpsc::channel();
                slice.map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx_done.send(r);
                });
                self.wait()?;
                rx_done
                    .recv()
                    .map_err(|e| BakeError::Gpu(e.to_string()))?
                    .map_err(|e| BakeError::Gpu(e.to_string()))?;
                {
                    let data = slice
                        .get_mapped_range()
                        .map_err(|e| BakeError::Gpu(e.to_string()))?;
                    let texels: &[[f32; 4]] = bytemuck::cast_slice(&data);
                    let (ox, oy) = ((origin[0] / ss) as usize, (origin[1] / ss) as usize);
                    for (i, image) in images.iter_mut().enumerate() {
                        let src = &texels[i * n_out as usize..(i + 1) * n_out as usize];
                        for y in 0..out_size[1] as usize {
                            let row =
                                &src[y * out_size[0] as usize..(y + 1) * out_size[0] as usize];
                            let dst = (oy + y) * res + ox;
                            image[dst..dst + row.len()].copy_from_slice(row);
                        }
                    }
                }
                staging.unmap();
                advance(progress)?;
            }
        }

        // Padding and quantization.
        let result = s
            .maps
            .iter()
            .zip(images)
            .map(|(&map, mut pixels)| {
                dilate(&mut pixels, res, res, s.dilation);
                let neutral = map.neutral();
                let data = pixels
                    .iter()
                    .flat_map(|p| {
                        if p[3] > 0.0 {
                            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                            [q(p[0]), q(p[1]), q(p[2]), 255]
                        } else {
                            neutral
                        }
                    })
                    .collect();
                (
                    map,
                    Image::new(s.resolution, s.resolution, data).expect("image size"),
                )
            })
            .collect();
        progress(1.0);
        Ok(result)
    }
}
