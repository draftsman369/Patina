use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use patina_core::Channel;

use crate::renderer::{MSAA_SAMPLES, OUTPUT_FORMAT, Renderer, Scene};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct UvParams {
    transform: [f32; 4],
    channel: [u32; 4],
    wire_color: [f32; 4],
}

/// Pan/zoom state of the 2D UV view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvView {
    /// UV coordinate shown at the center of the view.
    pub center: Vec2,
    /// Pixels per UV unit.
    pub zoom: f32,
}

impl Default for UvView {
    fn default() -> Self {
        Self {
            center: Vec2::splat(0.5),
            zoom: 400.0,
        }
    }
}

impl UvView {
    /// Fits the 0–1 tile in a view of `size` pixels with a small margin.
    pub fn fit(&mut self, size: Vec2) {
        self.center = Vec2::splat(0.5);
        self.zoom = (size.x.min(size.y) * 0.9).max(1.0);
    }

    /// Moves the view by a drag of `delta` pixels.
    pub fn pan(&mut self, delta: Vec2) {
        self.center -= delta / self.zoom;
    }

    /// Zooms by `factor`, keeping the UV under `cursor` (pixels from the view's top-left) fixed.
    pub fn zoom_at(&mut self, factor: f32, cursor: Vec2, size: Vec2) {
        let before = self.screen_to_uv(cursor, size);
        self.zoom = (self.zoom * factor).clamp(10.0, 100_000.0);
        let after = self.screen_to_uv(cursor, size);
        self.center += before - after;
    }

    pub fn screen_to_uv(&self, p: Vec2, size: Vec2) -> Vec2 {
        self.center + (p - size * 0.5) / self.zoom
    }

    /// UV → NDC as (scale, offset) for the shader.
    fn transform(&self, size: Vec2) -> [f32; 4] {
        let sx = 2.0 * self.zoom / size.x.max(1.0);
        let sy = 2.0 * self.zoom / size.y.max(1.0);
        [sx, sy, -self.center.x * sx, self.center.y * sy]
    }
}

/// Offscreen targets for the 2D UV view.
pub struct UvTarget {
    width: u32,
    height: u32,
    msaa_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl UvTarget {
    pub fn new(renderer: &Renderer, width: u32, height: u32) -> Self {
        let device = &renderer.device;
        let (width, height) = (width.max(1), height.max(1));
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let msaa = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uv msaa"),
            size,
            mip_level_count: 1,
            sample_count: MSAA_SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uv output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uv params"),
            size: size_of::<UvParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uv view"),
            layout: &renderer.uv_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });
        Self {
            width,
            height,
            msaa_view: msaa.create_view(&Default::default()),
            output_view: output.create_view(&Default::default()),
            output,
            params,
            bind_group,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Recreates the targets if the size changed; returns true when it did.
    pub fn resize(&mut self, renderer: &Renderer, width: u32, height: u32) -> bool {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return false;
        }
        *self = Self::new(renderer, width, height);
        true
    }

    pub fn output(&self) -> &wgpu::Texture {
        &self.output
    }

    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }
}

impl Renderer {
    /// Renders the UV layout of mesh part `part` over the given channel.
    pub fn render_uv(
        &self,
        target: &UvTarget,
        scene: &Scene,
        part: usize,
        view: &UvView,
        channel: Channel,
    ) {
        let size = Vec2::new(target.width as f32, target.height as f32);
        let params = UvParams {
            transform: view.transform(size),
            channel: [channel.index() as u32, u32::from(scene.mesh.has_uvs), 0, 0],
            wire_color: [0.95, 0.95, 0.95, 0.55],
        };
        self.queue
            .write_buffer(&target.params, 0, bytemuck::bytes_of(&params));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("uv view"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("uv view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&target.output_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let gpu_part = scene.mesh.part(part);
            let material = gpu_part.and_then(|p| scene.materials.get(p.material));
            if let (Some(p), Some(material)) = (gpu_part, material) {
                pass.set_bind_group(1, material.bind_group(), &[]);
                pass.set_bind_group(2, &target.bind_group, &[]);
                pass.set_pipeline(&self.uv_quad_pipeline);
                pass.draw(0..3, 0..1);
                if scene.mesh.has_uvs {
                    pass.set_pipeline(&self.uv_wire_pipeline);
                    pass.set_vertex_buffer(0, scene.mesh.vertex_buffer().slice(..));
                    pass.set_index_buffer(
                        scene.mesh.edge_buffer().slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(p.first_edge..p.first_edge + p.edge_count, 0, 0..1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_cursor_fixed() {
        let size = Vec2::new(800.0, 600.0);
        let mut view = UvView::default();
        let cursor = Vec2::new(123.0, 456.0);
        let before = view.screen_to_uv(cursor, size);
        view.zoom_at(1.7, cursor, size);
        let after = view.screen_to_uv(cursor, size);
        assert!((before - after).length() < 1e-5);
    }

    #[test]
    fn transform_maps_center_to_origin() {
        let size = Vec2::new(640.0, 480.0);
        let view = UvView {
            center: Vec2::new(0.3, 0.7),
            zoom: 200.0,
        };
        let [sx, sy, ox, oy] = view.transform(size);
        let ndc = Vec2::new(0.3 * sx + ox, -0.7 * sy + oy);
        assert!(ndc.length() < 1e-5);
    }

    #[test]
    fn fit_shows_whole_tile() {
        let size = Vec2::new(500.0, 300.0);
        let mut view = UvView::default();
        view.fit(size);
        let tl = view.screen_to_uv(Vec2::ZERO, size);
        let br = view.screen_to_uv(size, size);
        assert!(tl.x < 0.0 && tl.y < 0.0 && br.x > 1.0 && br.y > 1.0);
    }
}
