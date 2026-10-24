//! Mesh data for Patina: a single indexed triangle mesh split into parts,
//! one part per material (and therefore one per texture set).

pub mod bvh;
mod fbx_import;
mod generate;
mod gltf_import;
mod obj_import;
pub mod primitives;

use std::path::Path;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use patina_core::{Channel, Image, TextureSet};
use thiserror::Error;

pub use generate::{curvature, generate_normals, generate_tangents};

/// GPU-ready vertex. The layout is shared with the renderer's vertex shader.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// xyz = tangent, w = bitangent sign (MikkTSpace convention).
    pub tangent: [f32; 4],
    /// Vertex color (RGBA8), white when the source has none. Used by ID bakes.
    pub color: [u8; 4],
}

impl Default for Vertex {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            normal: [0.0; 3],
            uv: [0.0; 2],
            tangent: [1.0, 0.0, 0.0, 1.0],
            color: [255; 4],
        }
    }
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
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Radius of the bounding sphere around [`Aabb::center`].
    pub fn radius(&self) -> f32 {
        (self.max - self.min).length() * 0.5
    }

    pub fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn surface_area(&self) -> f32 {
        let d = (self.max - self.min).max(Vec3::ZERO);
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
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
    /// Names of the source objects (glTF/FBX nodes, OBJ objects).
    pub objects: Vec<String>,
    /// Object index of every triangle, in index-buffer order.
    pub triangle_objects: Vec<u32>,
}

#[derive(Debug, Error)]
pub enum MeshError {
    #[error("unsupported mesh format: {0}")]
    UnsupportedFormat(String),
    #[error("OBJ import failed: {0}")]
    Obj(#[from] tobj::LoadError),
    #[error("glTF import failed: {0}")]
    Gltf(#[from] gltf::Error),
    #[error("FBX import failed: {0}")]
    Fbx(String),
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
        "fbx" => fbx_import::load(path),
        other => Err(MeshError::UnsupportedFormat(other.to_owned())),
    }
}

/// Extensions accepted by [`load`].
pub const SUPPORTED_EXTENSIONS: &[&str] = &["obj", "gltf", "glb", "fbx"];

impl Mesh {
    pub fn bounds(&self) -> Aabb {
        if self.vertices.is_empty() {
            return Aabb {
                min: Vec3::ZERO,
                max: Vec3::ZERO,
            };
        }
        let mut b = Aabb::EMPTY;
        for v in &self.vertices {
            b.grow(Vec3::from(v.position));
        }
        b
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Positions of triangle `t`.
    pub fn triangle(&self, t: usize) -> [Vec3; 3] {
        let i = &self.indices[t * 3..t * 3 + 3];
        [i[0], i[1], i[2]].map(|i| Vec3::from(self.vertices[i as usize].position))
    }

    /// Material index of every triangle, in index-buffer order.
    pub fn triangle_materials(&self) -> Vec<u32> {
        let mut out = vec![0; self.triangle_count()];
        for part in &self.parts {
            let start = part.first_index as usize / 3;
            let end = start + part.index_count as usize / 3;
            out[start..end].fill(part.material as u32);
        }
        out
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

    /// Recomputes exact MikkTSpace tangents, splitting vertices where needed.
    pub fn generate_tangents(&mut self) {
        generate_tangents(&mut self.vertices, &mut self.indices);
    }
}

/// Collects triangles per material and produces a [`Mesh`] with one
/// contiguous part per material.
pub(crate) struct MeshBuilder {
    vertices: Vec<Vertex>,
    /// Triangle indices per material.
    material_indices: Vec<Vec<u32>>,
    /// Object index of each triangle, per material.
    material_objects: Vec<Vec<u32>>,
    materials: Vec<MaterialDesc>,
    objects: Vec<String>,
}

impl MeshBuilder {
    pub fn new(materials: Vec<MaterialDesc>) -> Self {
        let n = materials.len();
        Self {
            vertices: Vec::new(),
            material_indices: vec![Vec::new(); n],
            material_objects: vec![Vec::new(); n],
            materials,
            objects: Vec::new(),
        }
    }

    /// Adds a material and returns its index.
    pub fn add_material(&mut self, material: MaterialDesc) -> usize {
        self.materials.push(material);
        self.material_indices.push(Vec::new());
        self.material_objects.push(Vec::new());
        self.materials.len() - 1
    }

    /// Adds an object name and returns its index.
    pub fn add_object(&mut self, name: impl Into<String>) -> u32 {
        self.objects.push(name.into());
        self.objects.len() as u32 - 1
    }

    /// Appends vertices and triangles (indices local to `vertices`).
    pub fn add(&mut self, material: usize, object: u32, vertices: &[Vertex], indices: &[u32]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(vertices);
        self.material_indices[material].extend(indices.iter().map(|i| i + base));
        self.material_objects[material].extend(std::iter::repeat_n(object, indices.len() / 3));
    }

    /// Finishes the mesh. Materials with no triangles are dropped.
    pub fn build(self, name: String, has_uvs: bool) -> Result<Mesh, MeshError> {
        let mut indices = Vec::new();
        let mut triangle_objects = Vec::new();
        let mut parts = Vec::new();
        let mut materials = Vec::new();
        for ((material, tris), objs) in self
            .materials
            .into_iter()
            .zip(self.material_indices)
            .zip(self.material_objects)
        {
            if tris.is_empty() {
                continue;
            }
            parts.push(MeshPart {
                material: materials.len(),
                first_index: indices.len() as u32,
                index_count: tris.len() as u32,
            });
            indices.extend(tris);
            triangle_objects.extend(objs);
            materials.push(material);
        }
        if indices.is_empty() {
            return Err(MeshError::Empty);
        }
        let mut objects = self.objects;
        if objects.is_empty() {
            objects.push(name.clone());
        }
        Ok(Mesh {
            name,
            vertices: self.vertices,
            indices,
            parts,
            materials,
            has_uvs,
            objects,
            triangle_objects,
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
        let o1 = b.add_object("one");
        let o2 = b.add_object("two");
        b.add(2, o1, &v, &[0, 1, 2]);
        b.add(0, o2, &v, &[0, 1, 2]);
        b.add(2, o2, &v, &[0, 1, 2]);
        let mesh = b.build("t".into(), true).unwrap();
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(mesh.materials[0].name, "a");
        assert_eq!(mesh.materials[1].name, "c");
        assert_eq!(mesh.parts[0].index_count, 3);
        assert_eq!(mesh.parts[1].index_count, 6);
        assert_eq!(mesh.parts[1].first_index, 3);
        // Second triangle of material "a" was offset by 3 vertices.
        assert_eq!(&mesh.indices[0..3], &[3, 4, 5]);
        // Object ids follow the triangle reordering.
        assert_eq!(mesh.triangle_objects, vec![o2, o1, o2]);
        assert_eq!(mesh.triangle_materials(), vec![0, 1, 1]);
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

    #[test]
    fn aabb_union_and_area() {
        let a = Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        let b = Aabb {
            min: Vec3::splat(2.0),
            max: Vec3::splat(3.0),
        };
        let u = a.union(&b);
        assert_eq!(u.max, Vec3::splat(3.0));
        assert_eq!(a.surface_area(), 6.0);
        assert_eq!(Aabb::EMPTY.surface_area(), 0.0);
    }
}
