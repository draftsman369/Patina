//! glTF 2.0 / GLB import: meshes with node transforms applied, metal/rough
//! materials and their textures.

use std::path::Path;

use glam::{Mat3, Mat4, Vec3};
use gltf::image::Format;
use patina_core::Image;

use crate::{
    MaterialDesc, Mesh, MeshBuilder, MeshError, Vertex, generate_normals, generate_tangents,
};

pub fn load(path: &Path) -> Result<Mesh, MeshError> {
    let (doc, buffers, images) = gltf::import(path)?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mesh".into());

    let images: Vec<Option<Image>> = images.iter().map(convert_image).collect();
    let materials = doc
        .materials()
        .map(|m| convert_material(&m, &images))
        .collect();
    let mut builder = MeshBuilder::new(materials);
    let mut default_material = None;
    let mut has_uvs = true;

    let Some(scene) = doc.default_scene().or_else(|| doc.scenes().next()) else {
        return Err(MeshError::Empty);
    };
    let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        stack.extend(node.children().map(|c| (c, world)));
        let Some(mesh) = node.mesh() else { continue };

        let normal_matrix = Mat3::from_mat4(world).inverse().transpose();
        let flips_winding = world.determinant() < 0.0;

        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                log::warn!("skipping non-triangle primitive in mesh {:?}", mesh.name());
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<[f32; 3]> = positions.collect();
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let uvs: Option<Vec<[f32; 2]>> =
                reader.read_tex_coords(0).map(|t| t.into_f32().collect());
            has_uvs &= uvs.is_some();

            let mut indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if flips_winding {
                for tri in indices.as_chunks_mut::<3>().0 {
                    tri.swap(1, 2);
                }
            }

            let mut vertices: Vec<Vertex> = positions
                .iter()
                .enumerate()
                .map(|(i, p)| Vertex {
                    position: world.transform_point3(Vec3::from(*p)).to_array(),
                    normal: normals
                        .as_ref()
                        .map(|n| {
                            (normal_matrix * Vec3::from(n[i]))
                                .normalize_or_zero()
                                .to_array()
                        })
                        .unwrap_or_default(),
                    uv: uvs.as_ref().map(|t| t[i]).unwrap_or_default(),
                    tangent: [1.0, 0.0, 0.0, 1.0],
                })
                .collect();
            if normals.is_none() {
                generate_normals(&mut vertices, &indices);
            }

            let material = match prim.material().index() {
                Some(i) => i,
                None => *default_material
                    .get_or_insert_with(|| builder.add_material(MaterialDesc::named("Default"))),
            };
            builder.add(material, &vertices, &indices);
        }
    }

    let mut mesh = builder.build(name, has_uvs)?;
    generate_tangents(&mut mesh.vertices, &mesh.indices);
    Ok(mesh)
}

fn convert_material(m: &gltf::Material, images: &[Option<Image>]) -> MaterialDesc {
    let name = m
        .name()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Material {}", m.index().unwrap_or(0)));
    let mut desc = MaterialDesc::named(name);
    let pbr = m.pbr_metallic_roughness();
    desc.base_color_factor = pbr.base_color_factor();
    desc.metallic_factor = pbr.metallic_factor();
    desc.roughness_factor = pbr.roughness_factor();
    desc.emissive_factor = m.emissive_factor();

    let image = |tex: gltf::Texture, set: u32| -> Option<Image> {
        if set != 0 {
            log::warn!("texture uses TEXCOORD_{set}; only TEXCOORD_0 is supported");
        }
        images.get(tex.source().index()).cloned().flatten()
    };
    desc.base_color_image = pbr
        .base_color_texture()
        .and_then(|t| image(t.texture(), t.tex_coord()));
    if let Some(mr) = pbr
        .metallic_roughness_texture()
        .and_then(|t| image(t.texture(), t.tex_coord()))
    {
        // glTF packs roughness in G and metallic in B.
        desc.roughness_image = Some(mr.extract_channel(1));
        desc.metallic_image = Some(mr.extract_channel(2));
    }
    desc.normal_image = m
        .normal_texture()
        .and_then(|t| image(t.texture(), t.tex_coord()));
    desc.emissive_image = m
        .emissive_texture()
        .and_then(|t| image(t.texture(), t.tex_coord()));
    desc
}

/// Converts any glTF image format to RGBA8.
fn convert_image(img: &gltf::image::Data) -> Option<Image> {
    let px = &img.pixels;
    let n = img.width as usize * img.height as usize;
    // Reads component `c` of pixel `i` as 8-bit.
    let data: Vec<u8> = match img.format {
        Format::R8 => px.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        Format::R8G8 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], 0, 255])
            .collect(),
        Format::R8G8B8 => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        Format::R8G8B8A8 => px.clone(),
        Format::R16 | Format::R16G16 | Format::R16G16B16 | Format::R16G16B16A16 => {
            let comps = px.len() / 2 / n.max(1);
            // Keep the high byte of each little-endian u16.
            let high: Vec<u8> = px.as_chunks::<2>().0.iter().map(|c| c[1]).collect();
            expand(&high, comps)
        }
        Format::R32G32B32FLOAT | Format::R32G32B32A32FLOAT => {
            let comps = px.len() / 4 / n.max(1);
            let bytes: Vec<u8> = px
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| {
                    let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                })
                .collect();
            expand(&bytes, comps)
        }
    };
    Image::new(img.width, img.height, data)
        .inspect_err(|e| log::warn!("skipping glTF image: {e}"))
        .ok()
}

fn expand(values: &[u8], comps: usize) -> Vec<u8> {
    values
        .chunks_exact(comps.max(1))
        .flat_map(|p| match p.len() {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[1], 0, 255],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal embedded glTF: one triangle, no normals, translated by a node.
    const TRIANGLE_GLTF: &str = r#"{
      "asset": {"version": "2.0"},
      "scene": 0,
      "scenes": [{"nodes": [0]}],
      "nodes": [{"mesh": 0, "translation": [0, 0, 5]}],
      "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
      "materials": [{"name": "Steel", "pbrMetallicRoughness": {"metallicFactor": 1.0, "roughnessFactor": 0.25}}],
      "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                     "min": [0, 0, 0], "max": [1, 1, 0]}],
      "bufferViews": [{"buffer": 0, "byteLength": 36}],
      "buffers": [{"byteLength": 36,
        "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]
    }"#;

    #[test]
    fn loads_embedded_gltf_with_node_transform() {
        let dir = std::env::temp_dir().join(format!("patina_gltf_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tri.gltf");
        std::fs::write(&path, TRIANGLE_GLTF).unwrap();
        let mesh = load(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.materials[0].name, "Steel");
        assert_eq!(mesh.materials[0].metallic_factor, 1.0);
        assert_eq!(mesh.materials[0].roughness_factor, 0.25);
        assert!(!mesh.has_uvs);
        // Node translation applied.
        assert!(mesh.vertices.iter().all(|v| v.position[2] == 5.0));
        // Normals generated (+Z for this CCW triangle).
        assert!((Vec3::from(mesh.vertices[0].normal) - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn converts_rgb_to_rgba() {
        let data = gltf::image::Data {
            pixels: vec![1, 2, 3, 4, 5, 6],
            format: Format::R8G8B8,
            width: 2,
            height: 1,
        };
        let img = convert_image(&data).unwrap();
        assert_eq!(img.data, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }
}
