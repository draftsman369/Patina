//! FBX import (binary and ASCII, all versions) through the ufbx library.

use std::path::Path;

use glam::Vec3;
use patina_core::Image;

use crate::{MaterialDesc, Mesh, MeshBuilder, MeshError, Vertex, generate_normals};

pub fn load(path: &Path) -> Result<Mesh, MeshError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| MeshError::Fbx("path is not valid UTF-8".into()))?;
    let scene = ufbx::load_file(path_str, load_opts()).map_err(fbx_error)?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mesh".into());
    build(&scene, &name, path.parent())
}

#[cfg(test)]
fn load_memory(data: &[u8], name: &str) -> Result<Mesh, MeshError> {
    let scene = ufbx::load_memory(data, load_opts()).map_err(fbx_error)?;
    build(&scene, name, None)
}

fn load_opts() -> ufbx::LoadOpts<'static> {
    ufbx::LoadOpts {
        // Patina works in meters, right-handed, Y up (like glTF).
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        target_unit_meters: 1.0,
        space_conversion: ufbx::SpaceConversion::ModifyGeometry,
        generate_missing_normals: true,
        ignore_missing_external_files: true,
        ..Default::default()
    }
}

fn fbx_error(e: ufbx::Error) -> MeshError {
    MeshError::Fbx(e.description.to_string())
}

fn build(scene: &ufbx::Scene, name: &str, dir: Option<&Path>) -> Result<Mesh, MeshError> {
    let materials = scene
        .materials
        .iter()
        .map(|m| convert_material(m, dir))
        .collect();
    let mut builder = MeshBuilder::new(materials);
    let mut default_material = None;
    let mut has_uvs = true;

    for node in scene.nodes.iter() {
        let Some(mesh) = node.mesh.as_ref() else {
            continue;
        };
        let object = builder.add_object(if node.element.name.is_empty() {
            mesh.element.name.to_string()
        } else {
            node.element.name.to_string()
        });
        let to_world = &node.geometry_to_world;
        let normal_matrix = ufbx::matrix_for_normals(to_world);
        let flips = determinant(to_world) < 0.0;
        has_uvs &= mesh.vertex_uv.exists;

        // Triangles grouped by global material index.
        let mut per_material: Vec<(usize, Vec<Vertex>)> = Vec::new();
        let mut tri_indices = vec![0u32; mesh.max_face_triangles * 3];
        for (face_index, face) in mesh.faces.iter().enumerate() {
            let tris = mesh.triangulate_face(&mut tri_indices, *face) as usize;
            if tris == 0 {
                continue;
            }
            let material = mesh
                .face_material
                .get(face_index)
                .and_then(|&m| mesh.materials.get(m as usize))
                .map(|m| m.element.typed_id as usize);
            let material = match material {
                Some(m) => m,
                None => *default_material
                    .get_or_insert_with(|| builder.add_material(MaterialDesc::named("Default"))),
            };
            let slot = match per_material.iter().position(|(m, _)| *m == material) {
                Some(i) => i,
                None => {
                    per_material.push((material, Vec::new()));
                    per_material.len() - 1
                }
            };
            let out = &mut per_material[slot].1;
            for tri in tri_indices[..tris * 3].as_chunks::<3>().0 {
                let order = if flips { [0, 2, 1] } else { [0, 1, 2] };
                for k in order {
                    out.push(corner(mesh, tri[k] as usize, to_world, &normal_matrix));
                }
            }
        }

        for (material, mut vertices) in per_material {
            let indices: Vec<u32> = (0..vertices.len() as u32).collect();
            if !mesh.vertex_normal.exists {
                generate_normals(&mut vertices, &indices);
            }
            builder.add(material, object, &vertices, &indices);
        }
    }

    let mut mesh = builder.build(name.to_owned(), has_uvs)?;
    // Welds the per-corner vertices back into an indexed mesh.
    mesh.generate_tangents();
    Ok(mesh)
}

fn corner(
    mesh: &ufbx::Mesh,
    index: usize,
    to_world: &ufbx::Matrix,
    normal_matrix: &ufbx::Matrix,
) -> Vertex {
    let p = ufbx::transform_position(to_world, mesh.vertex_position[index]);
    let mut v = Vertex {
        position: [p.x as f32, p.y as f32, p.z as f32],
        ..Default::default()
    };
    if mesh.vertex_normal.exists {
        let n = ufbx::transform_direction(normal_matrix, mesh.vertex_normal[index]);
        v.normal = Vec3::new(n.x as f32, n.y as f32, n.z as f32)
            .normalize_or_zero()
            .to_array();
    }
    if mesh.vertex_uv.exists {
        let uv = mesh.vertex_uv[index];
        // FBX puts v = 0 at the bottom of the image; Patina at the top.
        v.uv = [uv.x as f32, 1.0 - uv.y as f32];
    }
    if mesh.vertex_color.exists {
        let c = mesh.vertex_color[index];
        v.color = [c.x, c.y, c.z, c.w].map(|x| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
    }
    v
}

fn determinant(m: &ufbx::Matrix) -> f64 {
    m.m00 * (m.m11 * m.m22 - m.m12 * m.m21) - m.m01 * (m.m10 * m.m22 - m.m12 * m.m20)
        + m.m02 * (m.m10 * m.m21 - m.m11 * m.m20)
}

fn convert_material(m: &ufbx::Material, dir: Option<&Path>) -> MaterialDesc {
    let mut desc = MaterialDesc::named(m.element.name.to_string());
    let pbr = &m.pbr;
    let load = |map: &ufbx::MaterialMap| -> Option<Image> {
        if !map.texture_enabled && map.texture.is_none() {
            return None;
        }
        let tex = map.texture.as_ref()?;
        if !tex.content.is_empty() {
            return Image::decode(&tex.content).ok();
        }
        let candidates = [
            tex.filename.to_string(),
            tex.absolute_filename.to_string(),
            dir.map(|d| {
                d.join(tex.relative_filename.as_ref())
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_default(),
        ];
        candidates
            .iter()
            .filter(|p| !p.is_empty())
            .find_map(|p| Image::load(p).ok())
            .or_else(|| {
                log::warn!("could not load FBX texture {}", tex.filename);
                None
            })
    };

    if pbr.base_color.has_value {
        let c = pbr.base_color.value_vec4;
        let f = if pbr.base_factor.has_value {
            pbr.base_factor.value_vec4.x
        } else {
            1.0
        };
        desc.base_color_factor = [(c.x * f) as f32, (c.y * f) as f32, (c.z * f) as f32, 1.0];
    }
    if pbr.roughness.has_value {
        desc.roughness_factor = pbr.roughness.value_vec4.x.clamp(0.0, 1.0) as f32;
    }
    if pbr.metalness.has_value {
        desc.metallic_factor = pbr.metalness.value_vec4.x.clamp(0.0, 1.0) as f32;
    }
    if pbr.emission_color.has_value {
        let c = pbr.emission_color.value_vec4;
        let f = if pbr.emission_factor.has_value {
            pbr.emission_factor.value_vec4.x
        } else {
            1.0
        };
        desc.emissive_factor = [(c.x * f) as f32, (c.y * f) as f32, (c.z * f) as f32];
    }
    desc.base_color_image = load(&pbr.base_color);
    if desc.base_color_image.is_some() {
        desc.base_color_factor = [1.0; 4];
    }
    desc.roughness_image = load(&pbr.roughness).map(|i| i.extract_channel(0));
    desc.metallic_image = load(&pbr.metalness).map(|i| i.extract_channel(0));
    desc.normal_image = load(&pbr.normal_map);
    desc.emissive_image = load(&pbr.emission_color);
    desc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit quad in an ASCII FBX 7.4 file, in centimeters with Y up.
    const QUAD_FBX: &str = r#"; FBX 7.4.0 project file
FBXHeaderExtension:  {
	FBXHeaderVersion: 1003
	FBXVersion: 7400
}
GlobalSettings:  {
	Version: 1000
	Properties70:  {
		P: "UpAxis", "int", "Integer", "",1
		P: "UpAxisSign", "int", "Integer", "",1
		P: "FrontAxis", "int", "Integer", "",2
		P: "FrontAxisSign", "int", "Integer", "",1
		P: "CoordAxis", "int", "Integer", "",0
		P: "CoordAxisSign", "int", "Integer", "",1
		P: "UnitScaleFactor", "double", "Number", "",1
	}
}
Objects:  {
	Geometry: 1000, "Geometry::Quad", "Mesh" {
		Vertices: *12 {
			a: 0,0,0,100,0,0,100,100,0,0,100,0
		}
		PolygonVertexIndex: *4 {
			a: 0,1,2,-4
		}
		GeometryVersion: 124
	}
	Model: 2000, "Model::QuadNode", "Mesh" {
		Version: 232
	}
}
Connections:  {
	C: "OO",1000,2000
	C: "OO",2000,0
}
"#;

    #[test]
    fn loads_ascii_quad() {
        let mesh = load_memory(QUAD_FBX.as_bytes(), "quad").unwrap();
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.objects, vec!["QuadNode".to_owned()]);
        assert!(!mesh.has_uvs);
        // Centimeters converted to meters.
        let b = mesh.bounds();
        assert!(
            (b.max.x - 1.0).abs() < 1e-4 && (b.max.y - 1.0).abs() < 1e-4,
            "{b:?}"
        );
        // Generated normals face +Z for this counter-clockwise quad.
        for v in &mesh.vertices {
            assert!((Vec3::from(v.normal) - Vec3::Z).length() < 1e-4);
        }
    }

    #[test]
    fn reports_garbage_as_error() {
        assert!(matches!(
            load_memory(b"not an fbx", "x"),
            Err(MeshError::Fbx(_))
        ));
    }
}
