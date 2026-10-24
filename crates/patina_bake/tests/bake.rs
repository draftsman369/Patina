//! GPU bake tests with numerical checks. Skipped without a GPU unless
//! `PATINA_REQUIRE_GPU=1`. Baked maps are written to `target/bake-tests/`
//! for inspection.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use patina_bake::{BakeError, BakeInput, BakeSettings, Baker, IdSource};
use patina_core::{Image, MeshMap};
use patina_mesh::primitives::{self, Primitive};
use patina_mesh::{MaterialDesc, Mesh, MeshPart};
use patina_render::HeadlessGpu;

fn gpu() -> Option<HeadlessGpu> {
    match HeadlessGpu::new() {
        Ok(g) => Some(g),
        Err(e) if std::env::var("PATINA_REQUIRE_GPU").is_ok_and(|v| v == "1") => {
            panic!("PATINA_REQUIRE_GPU=1 but no GPU is available: {e}")
        }
        Err(e) => {
            eprintln!("skipping GPU bake tests: {e}");
            None
        }
    }
}

fn save(name: &str, image: &Image) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/bake-tests");
    std::fs::create_dir_all(&dir).ok();
    image.save_png(dir.join(format!("{name}.png"))).ok();
}

fn settings(maps: &[MeshMap], resolution: u32) -> BakeSettings {
    BakeSettings {
        resolution,
        supersampling: 1,
        dilation: 4,
        maps: maps.to_vec(),
        ao_samples: 64,
        ..Default::default()
    }
}

fn bake(gpu: &HeadlessGpu, input: &BakeInput<'_>, s: &BakeSettings) -> Vec<(MeshMap, Image)> {
    let baker = Baker::new(&gpu.device, &gpu.queue);
    let mut last = 0.0;
    let maps = baker
        .bake(
            input,
            s,
            &mut |p| {
                assert!(p >= last, "progress went backwards");
                last = p;
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(last, 1.0);
    maps
}

fn get(maps: &[(MeshMap, Image)], m: MeshMap) -> &Image {
    &maps.iter().find(|(k, _)| *k == m).unwrap().1
}

/// Pixel at UV (u, v).
fn at(img: &Image, u: f32, v: f32) -> [u8; 4] {
    let x = ((u * img.width as f32) as u32).min(img.width - 1);
    let y = ((v * img.height as f32) as u32).min(img.height - 1);
    img.pixel(x, y)
}

/// Concatenates meshes, one part (material) and one object per input.
fn merge(meshes: &[(&str, Mesh)]) -> Mesh {
    let mut out = Mesh {
        name: "merged".into(),
        vertices: Vec::new(),
        indices: Vec::new(),
        parts: Vec::new(),
        materials: Vec::new(),
        has_uvs: true,
        objects: Vec::new(),
        triangle_objects: Vec::new(),
    };
    for (i, (name, m)) in meshes.iter().enumerate() {
        let base = out.vertices.len() as u32;
        out.parts.push(MeshPart {
            material: i,
            first_index: out.indices.len() as u32,
            index_count: m.indices.len() as u32,
        });
        out.vertices.extend_from_slice(&m.vertices);
        out.indices.extend(m.indices.iter().map(|x| x + base));
        out.materials.push(MaterialDesc::named(*name));
        out.objects.push((*name).to_owned());
        out.triangle_objects
            .extend(std::iter::repeat_n(i as u32, m.triangle_count()));
    }
    out
}

fn translated(mut m: Mesh, offset: Vec3) -> Mesh {
    for v in &mut m.vertices {
        v.position = (Vec3::from(v.position) + offset).to_array();
    }
    m
}

#[test]
fn self_bake_sphere() {
    let Some(gpu) = gpu() else { return };
    let sphere = Primitive::Sphere.build();
    let maps = bake(
        &gpu,
        &BakeInput {
            low: &sphere,
            high: None,
            cage: None,
            part: 0,
        },
        &settings(
            &[
                MeshMap::Normal,
                MeshMap::Opacity,
                MeshMap::Curvature,
                MeshMap::Position,
                MeshMap::WorldNormal,
            ],
            128,
        ),
    );
    let normal = get(&maps, MeshMap::Normal);
    save("sphere_normal", normal);
    // Baking a mesh onto itself gives a flat tangent-space normal map.
    for (u, v) in [(0.25, 0.5), (0.5, 0.3), (0.75, 0.6)] {
        let p = at(normal, u, v);
        assert!(
            p[0].abs_diff(128) <= 3 && p[1].abs_diff(128) <= 3 && p[2] >= 250,
            "{p:?} at {u},{v}"
        );
    }
    assert!(at(get(&maps, MeshMap::Opacity), 0.5, 0.5)[0] == 255);
    // A sphere is convex everywhere.
    let curvature = get(&maps, MeshMap::Curvature);
    save("sphere_curvature", curvature);
    assert!(
        at(curvature, 0.5, 0.5)[0] > 132,
        "{:?}",
        at(curvature, 0.5, 0.5)
    );
    // World normal at the equator facing +X (u = 0) is red-dominant.
    let wn = at(get(&maps, MeshMap::WorldNormal), 0.01, 0.5);
    assert!(wn[0] > 240 && wn[1].abs_diff(128) < 10, "{wn:?}");
    save("sphere_position", get(&maps, MeshMap::Position));
}

#[test]
fn high_to_low_cube_from_sphere() {
    let Some(gpu) = gpu() else { return };
    let low = Primitive::Cube.build();
    let high = primitives::sphere(1.0, 96, 48);
    let mut s = settings(&[MeshMap::Normal, MeshMap::Height], 128);
    s.max_frontal = 0.5;
    s.max_rear = 0.5;
    let maps = bake(
        &gpu,
        &BakeInput {
            low: &low,
            high: Some(&high),
            cage: None,
            part: 0,
        },
        &s,
    );
    let normal = get(&maps, MeshMap::Normal);
    save("cube_from_sphere_normal", normal);
    // Face centers of the cube atlas cells are flat...
    let center = at(normal, 1.0 / 6.0, 0.25);
    assert!(
        center[0].abs_diff(128) <= 4 && center[1].abs_diff(128) <= 4,
        "{center:?}"
    );
    // ...while the sphere's normals tilt strongly near the face corners.
    // Toward the top of the face (world +Y, image up) the sphere normal tilts
    // up: green above mid-gray in the OpenGL convention.
    let top = at(normal, 1.0 / 6.0, 0.03);
    assert!(top[1] > 160, "green-up convention broken: {top:?}");
    let corner = at(normal, 0.03, 0.03);
    assert!(
        corner[0].abs_diff(128) > 30 || corner[1].abs_diff(128) > 30,
        "{corner:?}"
    );
    // The sphere bulges out at face centers: height above mid-gray there.
    let h = get(&maps, MeshMap::Height);
    save("cube_from_sphere_height", h);
    assert!(
        at(h, 1.0 / 6.0, 0.25)[0] > 140,
        "{:?}",
        at(h, 1.0 / 6.0, 0.25)
    );
}

#[test]
fn ambient_occlusion_under_a_cube() {
    let Some(gpu) = gpu() else { return };
    let scene = merge(&[
        ("ground", primitives::plane(2.0)),
        (
            "box",
            translated(primitives::cube(0.6), Vec3::new(0.0, 0.32, 0.0)),
        ),
    ]);
    let mut s = settings(
        &[MeshMap::AmbientOcclusion, MeshMap::Id, MeshMap::Thickness],
        128,
    );
    s.ao_distance = 1.0;
    s.id_source = IdSource::Object;
    let maps = bake(
        &gpu,
        &BakeInput {
            low: &scene,
            high: None,
            cage: None,
            part: 0,
        },
        &s,
    );
    let ao = get(&maps, MeshMap::AmbientOcclusion);
    save("ground_ao", ao);
    // Under and right next to the box is darker than the open corner.
    let near = at(ao, 0.5, 0.5 + 0.17)[0];
    let far = at(ao, 0.03, 0.03)[0];
    assert!(far > 230, "open corner AO {far}");
    assert!(near + 40 < far, "contact AO {near} vs open {far}");
    assert!(
        at(ao, 0.5, 0.5)[0] < 40,
        "under the box AO {}",
        at(ao, 0.5, 0.5)[0]
    );

    let id_box = bake(
        &gpu,
        &BakeInput {
            low: &scene,
            high: None,
            cage: None,
            part: 1,
        },
        &s,
    );
    save("box_id", get(&id_box, MeshMap::Id));
    // Ground and box get different ID colors.
    assert_ne!(
        at(get(&maps, MeshMap::Id), 0.1, 0.1),
        at(get(&id_box, MeshMap::Id), 1.0 / 6.0, 0.25)
    );
    save("ground_thickness", get(&maps, MeshMap::Thickness));
}

#[test]
fn supersampling_and_dilation_cover_islands() {
    let Some(gpu) = gpu() else { return };
    let cube = Primitive::Cube.build();
    let mut s = settings(&[MeshMap::Opacity], 64);
    s.supersampling = 4;
    s.dilation = 8;
    let maps = bake(
        &gpu,
        &BakeInput {
            low: &cube,
            high: None,
            cage: None,
            part: 0,
        },
        &s,
    );
    let o = get(&maps, MeshMap::Opacity);
    save("cube_opacity_ss", o);
    // The 1% gutters between atlas cells are filled by dilation: no neutral gaps.
    assert!(
        o.data.chunks(4).all(|p| p[0] == 255),
        "uncovered texels remain"
    );
}

#[test]
fn cancel_stops_the_bake() {
    let Some(gpu) = gpu() else { return };
    let sphere = Primitive::Sphere.build();
    let baker = Baker::new(&gpu.device, &gpu.queue);
    let result = baker.bake(
        &BakeInput {
            low: &sphere,
            high: None,
            cage: None,
            part: 0,
        },
        &settings(&[MeshMap::AmbientOcclusion], 256),
        &mut |_| {},
        &AtomicBool::new(true),
    );
    assert!(matches!(result, Err(BakeError::Cancelled)));
}

#[test]
fn multi_tile_bake_covers_the_whole_map() {
    let Some(gpu) = gpu() else { return };
    let sphere = Primitive::Sphere.build();
    // 640 px with 2x supersampling spans 3x3 tiles of 512 texels, the last one partial.
    let mut s = settings(&[MeshMap::WorldNormal], 640);
    s.supersampling = 2;
    let maps = bake(
        &gpu,
        &BakeInput {
            low: &sphere,
            high: None,
            cage: None,
            part: 0,
        },
        &s,
    );
    let wn = get(&maps, MeshMap::WorldNormal);
    save("sphere_world_normal_tiled", wn);
    for (u, v) in [(0.1, 0.1), (0.9, 0.2), (0.5, 0.5), (0.2, 0.9), (0.95, 0.95)] {
        let p = at(wn, u, v);
        assert!(
            p[0] as u32 + p[1] as u32 + p[2] as u32 > 60,
            "uncovered at {u},{v}: {p:?}"
        );
    }
}

#[test]
fn ao_can_ignore_other_objects() {
    let Some(gpu) = gpu() else { return };
    let scene = merge(&[
        ("ground", primitives::plane(2.0)),
        (
            "box",
            translated(primitives::cube(0.6), Vec3::new(0.0, 0.32, 0.0)),
        ),
    ]);
    let mut s = settings(&[MeshMap::AmbientOcclusion], 128);
    s.ao_distance = 1.0;
    s.match_mode = patina_bake::MatchMode::ByName;
    s.ao_only_same_name = true;
    let maps = bake(
        &gpu,
        &BakeInput {
            low: &scene,
            high: None,
            cage: None,
            part: 0,
        },
        &s,
    );
    let ao = get(&maps, MeshMap::AmbientOcclusion);
    save("ground_ao_same_name", ao);
    // The box is a different object, so it no longer darkens the ground.
    assert!(at(ao, 0.5, 0.5 + 0.17)[0] > 230, "{:?}", at(ao, 0.5, 0.67));
}
