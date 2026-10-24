//! `patina --snapshot out.png`: headless render of one viewport frame.

use std::path::Path;

use anyhow::{Context as _, bail};
use patina_mesh::primitives::Primitive;
use patina_render::{
    ENV_PRESETS, HeadlessGpu, OrbitCamera, Renderer, ViewMode, ViewportSettings, render_snapshot,
};

use crate::Cli;
use crate::document::Document;

pub fn parse_size(s: &str) -> anyhow::Result<(u32, u32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .with_context(|| format!("size must look like 1280x720, got {s:?}"))?;
    let (w, h): (u32, u32) = (w.trim().parse()?, h.trim().parse()?);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("size must be between 1x1 and 8192x8192");
    }
    Ok((w, h))
}

pub fn run(cli: &Cli, out: &Path) -> anyhow::Result<()> {
    let (width, height) = parse_size(&cli.size)?;
    let view_mode = ViewMode::from_name(&cli.view)
        .with_context(|| format!("unknown view mode {:?}", cli.view))?;
    if cli.env >= ENV_PRESETS.len() {
        bail!("environment index must be 0-{}", ENV_PRESETS.len() - 1);
    }
    let doc = match &cli.mesh {
        Some(path) => Document::open(path)?,
        None => {
            let p = Primitive::from_name(&cli.primitive)
                .with_context(|| format!("unknown primitive {:?}", cli.primitive))?;
            Document::primitive(p)
        }
    };

    let gpu = HeadlessGpu::new()?;
    log::info!(
        "rendering on {} ({:?})",
        gpu.adapter_info.name,
        gpu.adapter_info.backend
    );
    let renderer = Renderer::new(&gpu.device, &gpu.queue);
    let scene = renderer.create_scene(&doc.mesh, &doc.project);

    let mut camera = OrbitCamera::default();
    camera.frame(&doc.mesh.bounds());
    let settings = ViewportSettings {
        env_preset: cli.env,
        exposure_ev: cli.exposure,
        view_mode,
        wireframe: cli.wireframe,
        ..Default::default()
    };
    let image = render_snapshot(&renderer, Some(&scene), &camera, &settings, width, height)?;
    image
        .save_png(out)
        .with_context(|| format!("could not write {}", out.display()))?;
    println!("wrote {} ({width}x{height})", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("1280x720").unwrap(), (1280, 720));
        assert_eq!(parse_size("64X32").unwrap(), (64, 32));
        assert!(parse_size("0x10").is_err());
        assert!(parse_size("big").is_err());
        assert!(parse_size("9000x10").is_err());
    }
}
