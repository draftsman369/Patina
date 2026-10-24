//! `patina MESH --bake OUT_DIR`: headless mesh-map baking.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context as _, bail};
use patina_bake::{BakeInput, BakeSettings, Baker, MatchMode};
use patina_core::MeshMap;
use patina_render::HeadlessGpu;

use crate::Cli;

/// Parses a comma-separated map list ("normal,ao,curvature" or "all").
pub fn parse_maps(list: &str) -> anyhow::Result<Vec<MeshMap>> {
    if list.trim().eq_ignore_ascii_case("all") {
        return Ok(MeshMap::ALL.to_vec());
    }
    list.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| MeshMap::from_key(s).with_context(|| format!("unknown map {s:?}")))
        .collect()
}

/// File-name friendly version of a texture set name.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "set".into() } else { s }
}

pub fn run(cli: &Cli, out_dir: &Path) -> anyhow::Result<()> {
    let Some(low_path) = &cli.mesh else {
        bail!("--bake needs a low-poly mesh: patina low.fbx --bake out_dir");
    };
    let low = patina_mesh::load(low_path)
        .with_context(|| format!("could not open {}", low_path.display()))?;
    let high = cli
        .high
        .as_ref()
        .map(|p| patina_mesh::load(p).with_context(|| format!("could not open {}", p.display())))
        .transpose()?;
    let cage = cli
        .cage
        .as_ref()
        .map(|p| patina_mesh::load(p).with_context(|| format!("could not open {}", p.display())))
        .transpose()?;
    let settings = BakeSettings {
        resolution: cli.bake_size,
        supersampling: cli.supersampling,
        maps: parse_maps(&cli.maps)?,
        match_mode: if cli.match_by_name {
            MatchMode::ByName
        } else {
            MatchMode::Always
        },
        ..Default::default()
    };
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("could not create {}", out_dir.display()))?;

    let gpu = HeadlessGpu::new()?;
    log::info!(
        "baking on {} ({:?})",
        gpu.adapter_info.name,
        gpu.adapter_info.backend
    );
    let baker = Baker::new(&gpu.device, &gpu.queue);
    let cancel = AtomicBool::new(false);
    for (part, material) in low.materials.iter().enumerate() {
        let input = BakeInput {
            low: &low,
            high: high.as_ref(),
            cage: cage.as_ref(),
            part,
        };
        let mut last = -1;
        let maps = baker.bake(
            &input,
            &settings,
            &mut |p| {
                let pct = (p * 100.0) as i32;
                if pct / 10 != last / 10 {
                    last = pct;
                    eprintln!("{}: {pct}%", material.name);
                }
            },
            &cancel,
        )?;
        for (map, image) in maps {
            let path = out_dir.join(format!("{}_{}.png", sanitize(&material.name), map.key()));
            image
                .save_png(&path)
                .with_context(|| format!("could not write {}", path.display()))?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_map_lists() {
        assert_eq!(
            parse_maps("normal, ao").unwrap(),
            vec![MeshMap::Normal, MeshMap::AmbientOcclusion]
        );
        assert_eq!(parse_maps("all").unwrap().len(), MeshMap::COUNT);
        assert!(parse_maps("normal,bogus").is_err());
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize("Body Metal/01"), "Body_Metal_01");
        assert_eq!(sanitize(""), "set");
    }
}
