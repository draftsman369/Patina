//! GPU access without a window: snapshots, tests and the CLI.

use patina_core::Image;

use crate::{OrbitCamera, RenderError, Renderer, Scene, ViewportSettings, ViewportTarget};

/// Renders one viewport frame offscreen and returns it as an sRGB RGBA8 image.
pub fn render_snapshot(
    renderer: &Renderer,
    scene: Option<&Scene>,
    camera: &OrbitCamera,
    settings: &ViewportSettings,
    width: u32,
    height: u32,
) -> Result<Image, RenderError> {
    let target = ViewportTarget::new(renderer, width, height);
    renderer.render_viewport(&target, scene, camera, settings);
    read_texture(renderer.device(), renderer.queue(), target.output())
}

/// A wgpu device created without a surface.
pub struct HeadlessGpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
}

impl HeadlessGpu {
    /// Creates a device on the best available adapter.
    ///
    /// Honors the standard `WGPU_BACKEND` / `WGPU_ADAPTER_NAME` environment
    /// variables, and `PATINA_SOFTWARE_GPU=1` to force a software adapter
    /// (lavapipe, WARP), which is what CI uses.
    pub fn new() -> Result<Self, RenderError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let force_fallback = std::env::var("PATINA_SOFTWARE_GPU").is_ok_and(|v| v == "1");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: force_fallback,
            compatible_surface: None,
            ..Default::default()
        }))?;
        let adapter_info = adapter.get_info();
        log::info!(
            "headless GPU: {} ({:?})",
            adapter_info.name,
            adapter_info.backend
        );
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("patina headless"),
                required_limits: wgpu::Limits {
                    max_texture_dimension_2d: 8192.min(adapter.limits().max_texture_dimension_2d),
                    ..wgpu::Limits::default()
                },
                ..Default::default()
            }))?;
        Ok(Self {
            device,
            queue,
            adapter_info,
        })
    }
}

/// Copies an RGBA8 texture back to the CPU.
pub fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Image, RenderError> {
    let (width, height) = (texture.width(), texture.height());
    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: padded as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    rx.recv()
        .map_err(|e| RenderError::Readback(e.to_string()))?
        .map_err(|e| RenderError::Readback(e.to_string()))?;

    let mapped = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| RenderError::Readback(e.to_string()))?;
    let mut data = Vec::with_capacity((unpadded * height) as usize);
    for row in mapped.chunks_exact(padded as usize) {
        data.extend_from_slice(&row[..unpadded as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Image::new(width, height, data).map_err(|e| RenderError::Readback(e.to_string()))
}
