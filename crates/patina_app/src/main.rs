//! `patina`: the Patina editor and its command-line entry points.

mod app;
mod bake_cli;
mod bake_job;
mod document;
mod snapshot;
mod theme;
mod ui;

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "patina",
    version,
    about = "Patina: open-source 3D texture painting"
)]
pub struct Cli {
    /// Mesh to open (OBJ, glTF, GLB).
    pub mesh: Option<PathBuf>,

    /// Render one frame headless to this PNG and exit.
    #[arg(long, value_name = "OUT.png")]
    pub snapshot: Option<PathBuf>,

    /// Built-in primitive used when no mesh is given: sphere, cube, cylinder, torus, plane.
    #[arg(long, default_value = "sphere")]
    pub primitive: String,

    /// Snapshot view mode: material, base-color, metallic, roughness, normal,
    /// world-normal, uv-checker, emissive.
    #[arg(long, default_value = "material")]
    pub view: String,

    /// Snapshot environment preset index (0-4).
    #[arg(long, default_value_t = 0)]
    pub env: usize,

    /// Snapshot HDRI environment (.hdr or .exr), overrides --env.
    #[arg(long, value_name = "FILE")]
    pub hdri: Option<PathBuf>,

    /// Snapshot exposure in stops.
    #[arg(long, default_value_t = 0.0)]
    pub exposure: f32,

    /// Snapshot size, WIDTHxHEIGHT.
    #[arg(long, default_value = "1280x720")]
    pub size: String,

    /// Draw the wireframe overlay in the snapshot.
    #[arg(long)]
    pub wireframe: bool,

    /// Bake mesh maps of MESH into this folder and exit.
    #[arg(long, value_name = "OUT_DIR")]
    pub bake: Option<PathBuf>,

    /// High-poly mesh for --bake (default: bake MESH onto itself).
    #[arg(long, value_name = "FILE")]
    pub high: Option<PathBuf>,

    /// Cage mesh for --bake, with the same triangles as MESH.
    #[arg(long, value_name = "FILE")]
    pub cage: Option<PathBuf>,

    /// Maps to bake: comma-separated (normal, world_normal, id, ao, curvature,
    /// position, thickness, height, opacity, bent_normal) or "all".
    #[arg(
        long,
        default_value = "normal,ao,curvature,thickness,position,world_normal,id"
    )]
    pub maps: String,

    /// Bake output size.
    #[arg(long, default_value_t = 2048)]
    pub bake_size: u32,

    /// Bake supersampling factor (1, 2, 4, 8).
    #[arg(long, default_value_t = 2)]
    pub supersampling: u32,

    /// Pair low and high objects by name (Bolt_low / Bolt_high).
    #[arg(long)]
    pub match_by_name: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,patina=info"))
        .init();
    let cli = Cli::parse();

    if let Some(out) = &cli.snapshot {
        return snapshot::run(&cli, out);
    }
    if let Some(out) = &cli.bake {
        return bake_cli::run(&cli, out);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Patina")
            .with_inner_size([1600.0, 950.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Patina",
        options,
        Box::new(move |cc| Ok(Box::new(app::PatinaApp::new(cc, cli.mesh.clone())))),
    )
    .map_err(|e| anyhow::anyhow!("could not start the editor: {e}"))
}
