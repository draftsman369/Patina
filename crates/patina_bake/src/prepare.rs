//! CPU-side preparation of bake inputs: BVH-ordered high-poly buffers and
//! the low-poly corner list rasterized into UV space.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use patina_mesh::bvh::{Bvh, BvhNode};
use patina_mesh::{Mesh, curvature, generate_normals};

use crate::BakeError;
use crate::settings::{BakeSettings, IdSource, MatchMode, match_key};

// Group ids travel through f32 render targets: keep them below 2^24 so they stay exact.
/// Group id of high-poly triangles without a low-poly partner (never hit in ByName mode).
const UNMATCHED_HIGH: u32 = (1 << 23) + 1;
/// Group id of low-poly objects without a high-poly partner.
const UNMATCHED_LOW: u32 = 1 << 23;

/// High-poly data laid out for the GPU.
pub struct HighData {
    pub nodes: Vec<BvhNode>,
    /// xyz = position, w = curvature (1 / length units).
    pub positions: Vec<[f32; 4]>,
    /// xyz = normal, w = vertex color bits (RGBA8).
    pub normals: Vec<[f32; 4]>,
    /// Triangle indices in BVH leaf order.
    pub indices: Vec<u32>,
    /// Per triangle (BVH order): match group, id value.
    pub tri_info: Vec<[u32; 2]>,
    /// Mean edge length, used to make curvature scale-independent.
    pub mean_edge: f32,
}

/// One low-poly triangle corner (non-indexed), see `Corner` in bake.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Corner {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
    /// xyz = ray normal, w = match group (as float).
    pub ray: [f32; 4],
    /// xyz = cage position, w = 1 when a cage is used.
    pub cage: [f32; 4],
}

/// Match groups for low and high objects (by object index).
pub struct Groups {
    pub low: Vec<u32>,
    pub high: Vec<u32>,
}

pub fn match_groups(low: &Mesh, high: &Mesh, mode: MatchMode) -> Groups {
    if mode == MatchMode::Always {
        return Groups {
            low: vec![0; low.objects.len()],
            high: vec![0; high.objects.len()],
        };
    }
    let mut keys: HashMap<String, u32> = HashMap::new();
    let high_keys: Vec<String> = high.objects.iter().map(|n| match_key(n)).collect();
    let low_groups: Vec<u32> = low
        .objects
        .iter()
        .map(|n| {
            let k = match_key(n);
            if high_keys.contains(&k) {
                let next = keys.len() as u32;
                *keys.entry(k).or_insert(next)
            } else {
                UNMATCHED_LOW
            }
        })
        .collect();
    let high_groups = high_keys
        .iter()
        .map(|k| keys.get(k).copied().unwrap_or(UNMATCHED_HIGH))
        .collect();
    Groups {
        low: low_groups,
        high: high_groups,
    }
}

pub fn prepare_high(high: &Mesh, groups: &[u32], id_source: IdSource) -> HighData {
    let tri_count = high.triangle_count();
    let triangles: Vec<[Vec3; 3]> = (0..tri_count).map(|t| high.triangle(t)).collect();
    let bvh = Bvh::build(&triangles);
    let curv = curvature(&high.vertices, &high.indices);
    let materials = high.triangle_materials();

    let positions = high
        .vertices
        .iter()
        .zip(&curv)
        .map(|(v, c)| [v.position[0], v.position[1], v.position[2], *c])
        .collect();
    let normals = high
        .vertices
        .iter()
        .map(|v| {
            let bits = f32::from_bits(u32::from_le_bytes(v.color));
            [v.normal[0], v.normal[1], v.normal[2], bits]
        })
        .collect();

    let mut indices = Vec::with_capacity(high.indices.len());
    let mut tri_info = Vec::with_capacity(tri_count);
    for &t in &bvh.order {
        let t = t as usize;
        indices.extend_from_slice(&high.indices[t * 3..t * 3 + 3]);
        let object = high.triangle_objects.get(t).copied().unwrap_or(0);
        let group = groups.get(object as usize).copied().unwrap_or(0);
        let id = match id_source {
            IdSource::Material => materials[t],
            IdSource::Object | IdSource::VertexColor => object,
        };
        tri_info.push([group, id]);
    }

    let mut edge_sum = 0.0f64;
    for tri in &triangles {
        edge_sum += ((tri[1] - tri[0]).length()
            + (tri[2] - tri[1]).length()
            + (tri[0] - tri[2]).length()) as f64;
    }
    let mean_edge = if tri_count > 0 {
        (edge_sum / (tri_count as f64 * 3.0)) as f32
    } else {
        1.0
    };

    HighData {
        nodes: bvh.nodes,
        positions,
        normals,
        indices,
        tri_info,
        mean_edge,
    }
}

/// Corners of the low-poly triangles in `part`, ready for UV rasterization.
pub fn prepare_corners(
    low: &Mesh,
    part: usize,
    cage: Option<&Mesh>,
    groups: &[u32],
    settings: &BakeSettings,
) -> Result<Vec<Corner>, BakeError> {
    let part = low.parts.get(part).ok_or(BakeError::NoSuchPart(part))?;
    if !low.has_uvs {
        return Err(BakeError::NoUvs);
    }
    if let Some(cage) = cage
        && cage.triangle_count() != low.triangle_count()
    {
        return Err(BakeError::CageMismatch {
            low: low.triangle_count(),
            cage: cage.triangle_count(),
        });
    }
    // Smoothed normals (shared across seams and hard edges) for ray directions.
    let ray_normals: Vec<[f32; 3]> = if settings.average_normals {
        let mut copy = low.vertices.clone();
        generate_normals(&mut copy, &low.indices);
        copy.iter().map(|v| v.normal).collect()
    } else {
        low.vertices.iter().map(|v| v.normal).collect()
    };

    let first_tri = part.first_index as usize / 3;
    let tri_count = part.index_count as usize / 3;
    let mut corners = Vec::with_capacity(tri_count * 3);
    for t in first_tri..first_tri + tri_count {
        let object = low.triangle_objects.get(t).copied().unwrap_or(0);
        let group = groups.get(object as usize).copied().unwrap_or(0);
        // Wrap the triangle into the 0-1 UV tile (meshes may use other tiles
        // with repeat wrapping; UDIM support comes later).
        let centroid = (0..3).fold([0.0f32; 2], |acc, k| {
            let uv = low.vertices[low.indices[t * 3 + k] as usize].uv;
            [acc[0] + uv[0] / 3.0, acc[1] + uv[1] / 3.0]
        });
        let tile = [centroid[0].floor(), centroid[1].floor()];
        for k in 0..3 {
            let vi = low.indices[t * 3 + k] as usize;
            let v = &low.vertices[vi];
            let rn = ray_normals[vi];
            let cage_pos = cage.map(|c| c.vertices[c.indices[t * 3 + k] as usize].position);
            corners.push(Corner {
                position: v.position,
                normal: v.normal,
                uv: [v.uv[0] - tile[0], v.uv[1] - tile[1]],
                tangent: v.tangent,
                ray: [rn[0], rn[1], rn[2], group as f32],
                cage: match cage_pos {
                    Some(p) => [p[0], p[1], p[2], 1.0],
                    None => [0.0; 4],
                },
            });
        }
    }
    if corners.is_empty() {
        return Err(BakeError::NoSuchPart(0));
    }
    Ok(corners)
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_mesh::primitives::Primitive;

    #[test]
    fn groups_pair_by_name() {
        let mut low = Primitive::Cube.build();
        low.objects = vec!["Bolt_low".into(), "Lonely_low".into()];
        let mut high = Primitive::Cube.build();
        high.objects = vec!["bolt_high".into(), "Extra_high".into()];
        let g = match_groups(&low, &high, MatchMode::ByName);
        assert_eq!(g.low[0], g.high[0]);
        assert_eq!(g.low[1], UNMATCHED_LOW);
        assert_eq!(g.high[1], UNMATCHED_HIGH);
        let always = match_groups(&low, &high, MatchMode::Always);
        assert!(always.low.iter().chain(&always.high).all(|&g| g == 0));
    }

    #[test]
    fn high_data_is_consistent() {
        let mesh = Primitive::Torus.build();
        let data = prepare_high(&mesh, &[0], IdSource::Object);
        assert_eq!(data.indices.len(), mesh.indices.len());
        assert_eq!(data.tri_info.len(), mesh.triangle_count());
        assert!(data.mean_edge > 0.0);
        // Same triangles, different order.
        let mut a: Vec<[u32; 3]> = mesh.indices.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        let mut b: Vec<[u32; 3]> = data.indices.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn corners_wrap_into_the_first_tile() {
        let mut mesh = Primitive::Plane.build();
        for v in &mut mesh.vertices {
            v.uv[1] += 1.0;
        }
        let corners = prepare_corners(&mesh, 0, None, &[0], &BakeSettings::default()).unwrap();
        assert!(corners.iter().all(|c| (0.0..=1.0).contains(&c.uv[1])));
    }

    #[test]
    fn corners_cover_the_part() {
        let mesh = Primitive::Sphere.build();
        let corners = prepare_corners(&mesh, 0, None, &[0], &BakeSettings::default()).unwrap();
        assert_eq!(corners.len(), mesh.indices.len());
        let cage = Primitive::Cube.build();
        assert!(matches!(
            prepare_corners(&mesh, 0, Some(&cage), &[0], &BakeSettings::default()),
            Err(BakeError::CageMismatch { .. })
        ));
    }
}
