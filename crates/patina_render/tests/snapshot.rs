//! GPU snapshot tests: render fixed scenes headless and compare them with the
//! reference PNGs in `tests/snapshots/` at the repository root.
//!
//! - `PATINA_UPDATE_SNAPSHOTS=1` rewrites the references.
//! - `PATINA_SOFTWARE_GPU=1` forces a software adapter (what CI uses).
//! - Without any GPU adapter the tests print a notice and pass, unless
//!   `PATINA_REQUIRE_GPU=1` is set (CI sets it so a broken driver can't hide failures).

use std::path::PathBuf;

use patina_core::{Channel, Image, Project};
use patina_mesh::primitives::Primitive;
use patina_render::{
    EnvSource, HeadlessGpu, OrbitCamera, Renderer, UvLayer, UvTarget, UvView, ViewMode,
    ViewportSettings, read_texture, render_snapshot,
};

const SIZE: u32 = 256;

fn gpu() -> Option<HeadlessGpu> {
    match HeadlessGpu::new() {
        Ok(gpu) => Some(gpu),
        Err(e) if std::env::var("PATINA_REQUIRE_GPU").is_ok_and(|v| v == "1") => {
            panic!("PATINA_REQUIRE_GPU=1 but no GPU is available: {e}")
        }
        Err(e) => {
            eprintln!("skipping GPU snapshot tests: {e}");
            None
        }
    }
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/snapshots")
}

/// Compares with the stored reference, tolerating small driver differences
/// (software vs hardware rasterizers, MSAA patterns).
fn check(name: &str, image: &Image) {
    check_with(name, image, 0.005);
}

/// Like [`check`], allowing `max_bad` of the pixels to differ. Images dominated by
/// 1-pixel lines need more slack: rasterizers disagree on lines that fall
/// exactly on pixel boundaries.
fn check_with(name: &str, image: &Image, max_bad: f64) {
    let dir = snapshot_dir();
    let path = dir.join(format!("{name}.png"));
    let update = std::env::var("PATINA_UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1");
    if update || !path.exists() {
        std::fs::create_dir_all(&dir).unwrap();
        image.save_png(&path).unwrap();
        assert!(
            update,
            "reference {} was missing; it has been created, re-run the tests",
            path.display()
        );
        return;
    }
    let reference = Image::load(&path).unwrap();
    assert_eq!(
        (reference.width, reference.height),
        (image.width, image.height),
        "{name}: size changed"
    );
    let (mean, bad_ratio, diff) = compare(image, &reference);
    if mean > 2.0 || bad_ratio > max_bad {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshot-failures");
        std::fs::create_dir_all(&dir).ok();
        let out = dir.join(format!("{name}.png"));
        image.save_png(&out).ok();
        diff.save_png(dir.join(format!("{name}_diff.png"))).ok();
        panic!(
            "{name}: snapshot mismatch (mean diff {mean:.2}, {:.2}% pixels off); actual saved to {}",
            bad_ratio * 100.0,
            out.display()
        );
    }
}

/// Returns (mean per-pixel difference, ratio of pixels off by more than 24/255, diff mask).
///
/// Each pixel is matched against the closest pixel in the 3x3 reference
/// neighborhood, so 1-pixel shifts of edges and thin lines between
/// rasterizers don't count as differences.
fn compare(image: &Image, reference: &Image) -> (f64, f64, Image) {
    let (w, h) = (image.width as i32, image.height as i32);
    // Diff mask: dimmed actual image with offending pixels in red.
    let mut diff = Image::new(
        image.width,
        image.height,
        image
            .data
            .iter()
            .enumerate()
            .map(|(i, v)| if i % 4 == 3 { 255 } else { v / 4 })
            .collect(),
    )
    .unwrap();
    let mut bad = 0usize;
    let mut total = 0u64;
    for y in 0..h {
        for x in 0..w {
            let a = image.pixel(x as u32, y as u32);
            let mut best = u8::MAX;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (rx, ry) = (x + dx, y + dy);
                    if rx < 0 || ry < 0 || rx >= w || ry >= h {
                        continue;
                    }
                    let b = reference.pixel(rx as u32, ry as u32);
                    let d = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap();
                    best = best.min(d);
                }
            }
            total += best as u64;
            if best > 24 {
                bad += 1;
                let i = ((y * w + x) * 4) as usize;
                diff.data[i..i + 3].copy_from_slice(&[255, 0, 0]);
            }
        }
    }
    let pixels = (w * h) as f64;
    (total as f64 / pixels, bad as f64 / pixels, diff)
}

/// 256x256 two-color checker.
fn checker_image(a: [u8; 4], b: [u8; 4]) -> Image {
    let mut data = Vec::with_capacity(256 * 256 * 4);
    for y in 0..256u32 {
        for x in 0..256u32 {
            data.extend_from_slice(if ((x / 32) + (y / 32)) % 2 == 0 {
                &a
            } else {
                &b
            });
        }
    }
    Image::new(256, 256, data).unwrap()
}

/// Horizontal gradient, used as a roughness map.
fn gradient_image() -> Image {
    let mut data = Vec::with_capacity(256 * 256 * 4);
    for _ in 0..256u32 {
        for x in 0..256u32 {
            let v = x as u8;
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    Image::new(256, 256, data).unwrap()
}

struct Fixture {
    renderer: Renderer,
}

impl Fixture {
    fn new(gpu: &HeadlessGpu) -> Self {
        Self {
            renderer: Renderer::new(&gpu.device, &gpu.queue),
        }
    }

    fn render(
        &self,
        primitive: Primitive,
        edit: impl FnOnce(&mut Project),
        settings: &ViewportSettings,
    ) -> Image {
        let mesh = primitive.build();
        let mut project = Project {
            texture_sets: mesh.texture_sets(),
            ..Default::default()
        };
        edit(&mut project);
        let scene = self.renderer.create_scene(&mesh, &project);
        let mut camera = OrbitCamera::default();
        camera.frame(&mesh.bounds());
        render_snapshot(&self.renderer, Some(&scene), &camera, settings, SIZE, SIZE).unwrap()
    }
}

fn textured_sphere(project: &mut Project) {
    let set = &mut project.texture_sets[0];
    set.base_color_factor = [1.0; 4];
    set.set_image(
        Channel::BaseColor,
        Some(checker_image([200, 80, 40, 255], [230, 225, 210, 255])),
    );
    set.set_image(Channel::Roughness, Some(gradient_image()));
    set.roughness_factor = 1.0;
}

#[test]
fn viewport_snapshots() {
    let Some(gpu) = gpu() else { return };
    eprintln!(
        "snapshot adapter: {} ({:?})",
        gpu.adapter_info.name, gpu.adapter_info.backend
    );
    let f = Fixture::new(&gpu);

    let lit = ViewportSettings::default();
    check(
        "sphere_material",
        &f.render(Primitive::Sphere, textured_sphere, &lit),
    );

    let base_color = ViewportSettings {
        view_mode: ViewMode::BaseColor,
        ..Default::default()
    };
    check(
        "sphere_base_color",
        &f.render(Primitive::Sphere, textured_sphere, &base_color),
    );

    let checker_wire = ViewportSettings {
        view_mode: ViewMode::UvChecker,
        wireframe: true,
        ..Default::default()
    };
    check_with(
        "torus_uv_checker",
        &f.render(Primitive::Torus, |_| {}, &checker_wire),
        0.03,
    );

    let sunset = ViewportSettings {
        environment: EnvSource::Preset(3),
        ..Default::default()
    };
    let gold = |p: &mut Project| {
        let set = &mut p.texture_sets[0];
        set.base_color_factor = [1.0, 0.77, 0.34, 1.0];
        set.metallic_factor = 1.0;
        set.roughness_factor = 0.3;
    };
    check(
        "cube_gold_sunset",
        &f.render(Primitive::Cube, gold, &sunset),
    );

    let normals = ViewportSettings {
        view_mode: ViewMode::WorldNormal,
        ..Default::default()
    };
    check(
        "cylinder_world_normal",
        &f.render(Primitive::Cylinder, |_| {}, &normals),
    );

    let mesh_map_view = ViewportSettings {
        view_mode: ViewMode::MeshMap,
        mesh_map: patina_core::MeshMap::Curvature,
        ..Default::default()
    };
    let with_map = |p: &mut Project| {
        p.texture_sets[0].set_mesh_map(patina_core::MeshMap::Curvature, Some(gradient_image()));
    };
    check(
        "sphere_mesh_map",
        &f.render(Primitive::Sphere, with_map, &mesh_map_view),
    );
}

/// Equirectangular test HDRI: blue sky, brown ground, a bright warm sun and
/// colored markers on the horizon so orientation errors are obvious.
fn synthetic_hdri() -> std::sync::Arc<patina_core::HdrImage> {
    let (w, h) = (256u32, 128u32);
    let mut data = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let v = (y as f32 + 0.5) / h as f32;
            let mut c = if v < 0.5 {
                [0.25, 0.45, 0.9, 1.0]
            } else {
                [0.3, 0.2, 0.1, 1.0]
            };
            // Horizon markers: red at u=0.25, green at u=0.5, blue-white at u=0.75.
            let near = |cu: f32| (u - cu).abs() < 0.03 && (v - 0.5).abs() < 0.06;
            if near(0.25) {
                c = [4.0, 0.2, 0.2, 1.0];
            } else if near(0.5) {
                c = [0.2, 4.0, 0.2, 1.0];
            } else if near(0.75) {
                c = [2.0, 2.0, 6.0, 1.0];
            }
            // Sun high in the sky.
            if (u - 0.6).abs() < 0.02 && (v - 0.2).abs() < 0.03 {
                c = [80.0, 70.0, 50.0, 1.0];
            }
            data.push(c);
        }
    }
    std::sync::Arc::new(patina_core::HdrImage::new(w, h, data).unwrap())
}

#[test]
fn reflection_snapshots() {
    let Some(gpu) = gpu() else { return };
    let f = Fixture::new(&gpu);
    let chrome = |p: &mut Project| {
        let set = &mut p.texture_sets[0];
        set.base_color_factor = [0.95, 0.95, 0.95, 1.0];
        set.metallic_factor = 1.0;
        set.roughness_factor = 0.0;
    };
    let studio = ViewportSettings {
        background_blur: 0.0,
        ..Default::default()
    };
    check(
        "sphere_chrome_studio",
        &f.render(Primitive::Sphere, chrome, &studio),
    );

    let hdri = ViewportSettings {
        environment: EnvSource::Hdri {
            image: synthetic_hdri(),
            name: "synthetic".into(),
        },
        background_blur: 0.0,
        ..Default::default()
    };
    check(
        "sphere_chrome_hdri",
        &f.render(Primitive::Sphere, chrome, &hdri),
    );
    let rough = |p: &mut Project| {
        chrome(p);
        p.texture_sets[0].roughness_factor = 0.5;
    };
    check(
        "sphere_rough_hdri",
        &f.render(Primitive::Sphere, rough, &hdri),
    );
}

#[test]
fn uv_view_snapshot() {
    let Some(gpu) = gpu() else { return };
    let renderer = Renderer::new(&gpu.device, &gpu.queue);
    let mesh = Primitive::Cube.build();
    let mut project = Project {
        texture_sets: mesh.texture_sets(),
        ..Default::default()
    };
    project.texture_sets[0].base_color_factor = [1.0; 4];
    project.texture_sets[0].set_image(
        Channel::BaseColor,
        Some(checker_image([60, 90, 160, 255], [200, 200, 200, 255])),
    );
    let scene = renderer.create_scene(&mesh, &project);
    let target = UvTarget::new(&renderer, SIZE, SIZE);
    let mut view = UvView::default();
    view.fit(glam::Vec2::splat(SIZE as f32));
    renderer.render_uv(
        &target,
        &scene,
        0,
        &view,
        UvLayer::Channel(Channel::BaseColor),
    );
    let image = read_texture(&gpu.device, &gpu.queue, target.output()).unwrap();
    check_with("cube_uv_view", &image, 0.03);
}

#[test]
fn empty_scene_renders_background() {
    let Some(gpu) = gpu() else { return };
    let renderer = Renderer::new(&gpu.device, &gpu.queue);
    let image = render_snapshot(
        &renderer,
        None,
        &OrbitCamera::default(),
        &ViewportSettings::default(),
        64,
        32,
    )
    .unwrap();
    assert_eq!((image.width, image.height), (64, 32));
    // The background is never pure black with the default studio environment.
    assert!(
        image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] > 10 && p[3] == 255)
    );
}
