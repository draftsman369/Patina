//! Mesh data for Patina: a single indexed triangle mesh split into parts,
//! one part per material (and therefore one per texture set).

mod generate;
mod gltf_import;
mod obj_import;
pub mod primitives;

use std::path::Path;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use patina_core::{Channel, Image, TextureSet};
use thiserror::Error;

pub use generate::{generate_normals, generate_tangents};

/// GPU-ready vertex. The layout is shared with the renderer's vertex shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// xyz = tangent, w = bitangent sign (MikkTSpace convention).
    pub tangent: [f32; 4],
}

/// A contiguous range of the index buffer drawn with one material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshPart {
    pub material: usize,
    pub first_index: u32,
    pub index_count: u32,
}

/// Material description read from the source file. It seeds the texture set
/// created for the material.
#[derive(Clone, Debug)]
pub struct MaterialDesc {
    pub name: String,
    pub base_color_factor: [f32; 4],
    pub metallic_factor: f32,
    pub roughness_factor: f32,
    pub emissive_factor: [f32; 3],
    pub base_color_image: Option<Image>,
    pub metallic_image: Option<Image>,
    pub roughness_image: Option<Image>,
    pub normal_image: Option<Image>,
    pub emissive_image: Option<Image>,
}

impl MaterialDesc {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_color_factor: [0.8, 0.8, 0.8, 1.0],
            metallic_factor: 0.0,
            roughness_factor: 0.5,
            emissive_factor: [0.0; 3],
            base_color_image: None,
            metallic_image: None,
            roughness_image: None,
            normal_image: None,
            emissive_image: None,
        }
    }
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Radius of the bounding sphere around [`Aabb::center`].
    pub fn radius(&self) -> f32 {
        (self.max - self.min).length() * 0.5
    }
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// One part per material, in material order.
    pub parts: Vec<MeshPart>,
    pub materials: Vec<MaterialDesc>,
    /// False when the source had no texture coordinates. Painting needs UVs;
    /// auto-unwrap arrives in M10.
    pub has_uvs: bool,
}

#[derive(Debug, Error)]
pub enum MeshError {
    #[error("unsupported mesh format: {0}")]
    UnsupportedFormat(String),
    #[error("OBJ import failed: {0}")]
    Obj(#[from] tobj::LoadError),
    #[error("glTF import failed: {0}")]
    Gltf(#[from] gltf::Error),
    #[error("the file contains no triangles")]
    Empty,
}

/// Loads a mesh, choosing the importer from the file extension.
pub fn load(path: impl AsRef<Path>) -> Result<Mesh, MeshError> {
    let path = path.as_ref();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "obj" => obj_import::load(path),
        "gltf" | "glb" => gltf_import::load(path),
        other => Err(MeshError::UnsupportedFormat(other.to_owned())),
    }
}

/// Extensions accepted by [`load`].
pub const SUPPORTED_EXTENSIONS: &[&str] = &["obj", "gltf", "glb"];

impl Mesh {
    pub fn bounds(&self) -> Aabb {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in &self.vertices {
            let p = Vec3::from(v.position);
            min = min.min(p);
            max = max.max(p);
        }
        if self.vertices.is_empty() {
            min = Vec3::ZERO;
            max = Vec3::ZERO;
        }
        Aabb { min, max }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// One texture set per material, seeded with the material's factors and textures.
    pub fn texture_sets(&self) -> Vec<TextureSet> {
        self.materials
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let mut set = TextureSet::new(m.name.clone(), i);
                set.base_color_factor = m.base_color_factor;
                set.metallic_factor = m.metallic_factor;
                set.roughness_factor = m.roughness_factor;
                set.emissive_factor = m.emissive_factor;
                set.set_image(Channel::BaseColor, m.base_color_image.clone());
                set.set_image(Channel::Metallic, m.metallic_image.clone());
                set.set_image(Channel::Roughness, m.roughness_image.clone());
                set.set_image(Channel::Normal, m.normal_image.clone());
                set.set_image(Channel::Emissive, m.emissive_image.clone());
                // A glTF emissive texture with a zero factor would stay black.
                if m.emissive_image.is_some() && m.emissive_factor == [0.0; 3] {
                    set.emissive_factor = [1.0; 3];
                }
                set
            })
            .collect()
    }

    /// Unique edges of one part as a line-list index buffer (pairs of vertex indices).
    pub fn part_edges(&self, part: &MeshPart) -> Vec<u32> {
        let start = part.first_index as usize;
        let end = start + part.index_count as usize;
        let mut edges: Vec<(u32, u32)> = Vec::with_capacity(part.index_count as usize);
        for tri in self.indices[start..end].as_chunks::<3>().0 {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                edges.push((a.min(b), a.max(b)));
            }
        }
        edges.sort_unstable();
        edges.dedup();
        edges.into_iter().flat_map(|(a, b)| [a, b]).collect()
    }
}

/// Collects triangles per material and produces a [`Mesh`] with one
/// contiguous part per material.
pub(crate) struct MeshBuilder {
    vertices: Vec<Vertex>,
    /// Triangle indices per material.
    material_indices: Vec<Vec<u32>>,
    materials: Vec<MaterialDesc>,
}

impl MeshBuilder {
    pub fn new(materials: Vec<MaterialDesc>) -> Self {
        let n = materials.len();
        Self {
            vertices: Vec::new(),
            material_indices: vec![Vec::new(); n],
            materials,
        }
    }

    /// Adds a material and returns its index.
    pub fn add_material(&mut self, material: MaterialDesc) -> usize {
        self.materials.push(material);
        self.material_indices.push(Vec::new());
        self.materials.len() - 1
    }

    /// Appends vertices and triangles (indices local to `vertices`).
    pub fn add(&mut self, material: usize, vertices: &[Vertex], indices: &[u32]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(vertices);
        self.material_indices[material].extend(indices.iter().map(|i| i + base));
    }

    /// Finishes the mesh. Materials with no triangles are dropped.
    pub fn build(self, name: String, has_uvs: bool) -> Result<Mesh, MeshError> {
        let mut indices = Vec::new();
        let mut parts = Vec::new();
        let mut materials = Vec::new();
        for (material, tris) in self.materials.into_iter().zip(self.material_indices) {
            if tris.is_empty() {
                continue;
            }
            parts.push(MeshPart {
                material: materials.len(),
                first_index: indices.len() as u32,
                index_count: tris.len() as u32,
            });
            indices.extend(tris);
            materials.push(material);
        }
        if indices.is_empty() {
            return Err(MeshError::Empty);
        }
        Ok(Mesh {
            name,
            vertices: self.vertices,
            indices,
            parts,
            materials,
            has_uvs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_groups_by_material_and_drops_unused() {
        let v = [Vertex::default(); 3];
        let mut b = MeshBuilder::new(vec![
            MaterialDesc::named("a"),
            MaterialDesc::named("unused"),
            MaterialDesc::named("c"),
        ]);
        b.add(2, &v, &[0, 1, 2]);
        b.add(0, &v, &[0, 1, 2]);
        b.add(2, &v, &[0, 1, 2]);
        let mesh = b.build("t".into(), true).unwrap();
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(mesh.materials[0].name, "a");
        assert_eq!(mesh.materials[1].name, "c");
        assert_eq!(mesh.parts[0].index_count, 3);
        assert_eq!(mesh.parts[1].index_count, 6);
        assert_eq!(mesh.parts[1].first_index, 3);
        // Second triangle of material "a" was offset by 3 vertices.
        assert_eq!(&mesh.indices[0..3], &[3, 4, 5]);
    }

    #[test]
    fn empty_builder_is_an_error() {
        let b = MeshBuilder::new(vec![MaterialDesc::named("a")]);
        assert!(matches!(b.build("t".into(), true), Err(MeshError::Empty)));
    }

    #[test]
    fn part_edges_are_unique() {
        let quad = primitives::plane(1.0);
        let edges = quad.part_edges(&quad.parts[0]);
        // Two triangles sharing one diagonal: 5 unique edges.
        assert_eq!(edges.len(), 10);
    }

    #[test]
    fn unknown_extension_is_rejected() {
        assert!(matches!(
            load("model.xyz"),
            Err(MeshError::UnsupportedFormat(_))
        ));
    }
}
