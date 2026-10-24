//! Patina's viewport renderer.
//!
//! Everything renders offscreen into textures: the editor shows them through
//! egui, and the headless `--snapshot` mode reads them back to PNG.

mod camera;
mod headless;
mod renderer;
mod settings;
mod texture;
mod uv_view;

pub use camera::OrbitCamera;
pub use headless::{HeadlessGpu, read_texture, render_snapshot};
pub use renderer::{GpuMaterial, GpuMesh, Renderer, Scene, ViewportTarget};
pub use settings::{ENV_PRESETS, EnvLight, EnvPreset, Tonemap, ViewMode, ViewportSettings};
pub use uv_view::{UvTarget, UvView};

/// Re-exported so dependants use the exact wgpu version this crate was built with.
pub use wgpu;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter found: {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    #[error("could not create GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("GPU readback failed: {0}")]
    Readback(String),
}
