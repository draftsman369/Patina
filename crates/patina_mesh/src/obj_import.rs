//! Wavefront OBJ import (with MTL materials and diffuse/normal textures).

use std::path::Path;

use patina_core::Image;

use crate::{MaterialDesc, Mesh, MeshBuilder, MeshError, Vertex, generate_normals};

pub fn load(path: &Path) -> Result<Mesh, MeshError> {
    let (models, materials) = tobj::load_obj(path, &tobj::GPU_LOAD_OPTIONS)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let materials = materials.unwrap_or_else(|e| {
        log::warn!("could not load OBJ materials: {e}");
        Vec::new()
    });
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mesh".into());
    build(&name, &models, &materials, Some(dir))
}

/// Shared by file loading and tests (which load OBJ text from memory).
pub(crate) fn build(
    name: &str,
    models: &[tobj::Model],
    materials: &[tobj::Material],
    texture_dir: Option<&Path>,
) -> Result<Mesh, MeshError> {
    let descs = materials
        .iter()
        .map(|m| convert_material(m, texture_dir))
        .collect();
    let mut builder = MeshBuilder::new(descs);
    let mut default_material = None;
    let mut has_uvs = true;

    for model in models {
        let m = &model.mesh;
        let count = m.positions.len() / 3;
        if count == 0 || m.indices.is_empty() {
            continue;
        }
        let has_normals = m.normals.len() == m.positions.len();
        let model_has_uvs = m.texcoords.len() / 2 == count;
        has_uvs &= model_has_uvs;

        let mut vertices: Vec<Vertex> = (0..count)
            .map(|i| Vertex {
                position: [
                    m.positions[i * 3],
                    m.positions[i * 3 + 1],
                    m.positions[i * 3 + 2],
                ],
                normal: if has_normals {
                    [m.normals[i * 3], m.normals[i * 3 + 1], m.normals[i * 3 + 2]]
                } else {
                    [0.0; 3]
                },
                // OBJ puts v = 0 at the bottom of the image; Patina at the top.
                uv: if model_has_uvs {
                    [m.texcoords[i * 2], 1.0 - m.texcoords[i * 2 + 1]]
                } else {
                    [0.0; 2]
                },
                color: if m.vertex_color.len() == m.positions.len() {
                    [0, 1, 2, 3].map(|c| {
                        let v = if c < 3 {
                            m.vertex_color[i * 3 + c]
                        } else {
                            1.0
                        };
                        (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                    })
                } else {
                    [255; 4]
                },
                ..Default::default()
            })
            .collect();
        if !has_normals {
            generate_normals(&mut vertices, &m.indices);
        }
        let object = builder.add_object(if model.name.is_empty() {
            name
        } else {
            &model.name
        });

        let material = match m.material_id.filter(|&id| id < materials.len()) {
            Some(id) => id,
            None => *default_material
                .get_or_insert_with(|| builder.add_material(MaterialDesc::named("Default"))),
        };
        builder.add(material, object, &vertices, &m.indices);
    }

    let mut mesh = builder.build(name.to_owned(), has_uvs)?;
    mesh.generate_tangents();
    Ok(mesh)
}

fn convert_material(m: &tobj::Material, texture_dir: Option<&Path>) -> MaterialDesc {
    let mut desc = MaterialDesc::named(if m.name.is_empty() {
        "Material"
    } else {
        &m.name
    });
    let load = |file: &Option<String>| -> Option<Image> {
        let file = file.as_ref()?;
        let path = texture_dir?.join(file);
        Image::load(&path)
            .inspect_err(|e| log::warn!("could not load texture {}: {e}", path.display()))
            .ok()
    };
    desc.base_color_image = load(&m.diffuse_texture);
    desc.normal_image = load(&m.normal_texture);

    if desc.base_color_image.is_none() {
        if let Some(kd) = m.diffuse {
            // MTL colors are display-referred; convert to linear.
            let lin = kd.map(|c| c.clamp(0.0, 1.0).powf(2.2));
            desc.base_color_factor = [lin[0], lin[1], lin[2], m.dissolve.unwrap_or(1.0)];
        }
    } else {
        desc.base_color_factor = [1.0; 4];
    }
    if let Some(ns) = m.shininess {
        // Blinn-Phong exponent to roughness.
        desc.roughness_factor = (2.0 / (ns.max(0.0) + 2.0)).sqrt().clamp(0.02, 1.0);
    }
    if let Some(ke) = m.emissive {
        desc.emissive_factor = ke.map(|c| c.max(0.0).powf(2.2));
    }
    desc
}

#[cfg(test)]
mod tests {
    use super::*;

    // The untextured triangle comes first: `usemtl` stays active for later objects.
    const CUBE_FACE: &str = "\
mtllib test.mtl
o tri
v 0 0 1
v 1 0 1
v 0 1 1
f 1 2 3
o quad
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
vt 0 0
vt 1 0
vt 1 1
vt 0 1
usemtl red
f 4/1 5/2 6/3 7/4
";

    fn red() -> tobj::Material {
        tobj::Material {
            name: "red".into(),
            diffuse: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        }
    }

    fn parse(src: &str) -> Mesh {
        let mut reader = std::io::BufReader::new(src.as_bytes());
        let (models, _) = tobj::load_obj_buf(&mut reader, &tobj::GPU_LOAD_OPTIONS, |_| {
            Ok((vec![red()], [("red".to_owned(), 0)].into_iter().collect()))
        })
        .unwrap();
        build("test", &models, &[red()], None).unwrap()
    }

    #[test]
    fn parses_quads_materials_and_flips_v() {
        let mesh = parse(CUBE_FACE);
        // Quad triangulated (2) + triangle (1).
        assert_eq!(mesh.triangle_count(), 3);
        // "red" from MTL + a default material for the untextured object.
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(mesh.materials[0].name, "red");
        assert_eq!(mesh.materials[0].base_color_factor[0], 1.0);
        assert_eq!(mesh.materials[1].name, "Default");
        assert_eq!(mesh.parts[0].index_count, 6);
        // One object lacks UVs, so the mesh is flagged.
        assert!(!mesh.has_uvs);
        // The quad corner at the origin had vt 0 0, flipped to (0, 1).
        let corner = mesh
            .vertices
            .iter()
            .find(|v| v.position == [0.0; 3])
            .unwrap();
        assert_eq!(corner.uv, [0.0, 1.0]);
    }

    #[test]
    fn generates_missing_normals() {
        let mesh = parse(CUBE_FACE);
        for v in &mesh.vertices {
            assert!((glam::Vec3::from(v.normal) - glam::Vec3::Z).length() < 1e-5);
        }
    }
}
