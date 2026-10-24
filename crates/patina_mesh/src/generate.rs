//! Normal and tangent generation.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::Vertex;

/// Computes smooth, area-weighted vertex normals.
///
/// Vertices that share a position (e.g. split along UV seams) are smoothed
/// together so seams don't show up as shading creases.
pub fn generate_normals(vertices: &mut [Vertex], indices: &[u32]) {
    // Group vertices by exact position.
    let mut groups: HashMap<[u32; 3], usize> = HashMap::new();
    let mut group_of = Vec::with_capacity(vertices.len());
    for v in vertices.iter() {
        let key = v.position.map(f32::to_bits);
        let next = groups.len();
        group_of.push(*groups.entry(key).or_insert(next));
    }

    let mut accum = vec![Vec3::ZERO; groups.len()];
    for tri in indices.as_chunks::<3>().0 {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(vertices[i as usize].position));
        // Cross product length is twice the triangle area: area weighting for free.
        let n = (b - a).cross(c - a);
        for &i in tri {
            accum[group_of[i as usize]] += n;
        }
    }

    for (v, g) in vertices.iter_mut().zip(group_of) {
        v.normal = accum[g].normalize_or(Vec3::Y).to_array();
    }
}

/// MikkTSpace input with one vertex per face corner.
struct MikkCorners<'a> {
    corners: &'a mut [Vertex],
}

impl mikktspace::Geometry for MikkCorners<'_> {
    fn num_faces(&self) -> usize {
        self.corners.len() / 3
    }

    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }

    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        self.corners[face * 3 + vert].position
    }

    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        self.corners[face * 3 + vert].normal
    }

    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        // MikkTSpace runs in v-up space (as in Blender and glTF tools) so the
        // bitangent points up the image: green-up "OpenGL" normal maps.
        let uv = self.corners[face * 3 + vert].uv;
        [uv[0], 1.0 - uv[1]]
    }

    fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
        self.corners[face * 3 + vert].tangent = tangent;
    }
}

/// Generates exact MikkTSpace tangents. Requires normals and UVs.
///
/// Tangents are computed per face corner, then identical corners are welded
/// back together. Vertices whose corners disagree are split, so the result
/// matches what bakers and engines using MikkTSpace expect. Unused vertices
/// are dropped; the index count and triangle order are unchanged.
///
/// Falls back to an arbitrary orthogonal tangent when the geometry is
/// degenerate (no UVs, zero-area UV triangles).
pub fn generate_tangents(vertices: &mut Vec<Vertex>, indices: &mut [u32]) {
    let mut corners: Vec<Vertex> = indices.iter().map(|&i| vertices[i as usize]).collect();
    let ok = !corners.is_empty()
        && mikktspace::generate_tangents(&mut MikkCorners {
            corners: &mut corners,
        });
    if !ok {
        log::warn!("MikkTSpace failed; using fallback tangents");
    }
    for v in corners.iter_mut() {
        let n = Vec3::from(v.normal);
        let t = Vec3::new(v.tangent[0], v.tangent[1], v.tangent[2]);
        if !ok || !t.is_finite() || t.length_squared() < 1e-12 {
            v.tangent = fallback_tangent(n).extend(1.0).to_array();
        }
    }

    // Weld bit-identical corners.
    let mut map: HashMap<[u32; 13], u32> = HashMap::with_capacity(vertices.len());
    let mut welded = Vec::with_capacity(vertices.len());
    for (corner, index) in corners.iter().zip(indices.iter_mut()) {
        let key = vertex_key(corner);
        *index = *map.entry(key).or_insert_with(|| {
            welded.push(*corner);
            welded.len() as u32 - 1
        });
    }
    *vertices = welded;
}

fn vertex_key(v: &Vertex) -> [u32; 13] {
    let mut k = [0u32; 13];
    let floats = v
        .position
        .iter()
        .chain(&v.normal)
        .chain(&v.uv)
        .chain(&v.tangent);
    for (slot, f) in k.iter_mut().zip(floats) {
        *slot = f.to_bits();
    }
    k[12] = u32::from_le_bytes(v.color);
    k
}

fn fallback_tangent(n: Vec3) -> Vec3 {
    let helper = if n.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    helper.cross(n).normalize_or(Vec3::X)
}

/// Per-vertex mean curvature estimate (1 / length units).
///
/// Positive on convex areas, negative in cavities. Vertices sharing a
/// position share a value, so hard edges and UV seams don't break it.
pub fn curvature(vertices: &[Vertex], indices: &[u32]) -> Vec<f32> {
    let mut groups: HashMap<[u32; 3], usize> = HashMap::new();
    let mut group_of = Vec::with_capacity(vertices.len());
    for v in vertices {
        let next = groups.len();
        group_of.push(*groups.entry(v.position.map(f32::to_bits)).or_insert(next));
    }
    let n = groups.len();
    let mut pos = vec![Vec3::ZERO; n];
    let mut normal = vec![Vec3::ZERO; n];
    for (v, &g) in vertices.iter().zip(&group_of) {
        pos[g] = Vec3::from(v.position);
        normal[g] += Vec3::from(v.normal);
    }
    for nrm in &mut normal {
        *nrm = nrm.normalize_or_zero();
    }

    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(indices.len());
    for tri in indices.as_chunks::<3>().0 {
        let g = tri.map(|i| group_of[i as usize]);
        for (a, b) in [(g[0], g[1]), (g[1], g[2]), (g[2], g[0])] {
            if a != b {
                edges.push((a.min(b), a.max(b)));
            }
        }
    }
    edges.sort_unstable();
    edges.dedup();

    let mut sum = vec![0.0f32; n];
    let mut count = vec![0u32; n];
    for (a, b) in edges {
        let d = pos[b] - pos[a];
        let len2 = d.length_squared();
        if len2 < 1e-20 {
            continue;
        }
        // Neighbors below the tangent plane mean a convex surface.
        sum[a] -= 2.0 * normal[a].dot(d) / len2;
        sum[b] -= 2.0 * normal[b].dot(-d) / len2;
        count[a] += 1;
        count[b] += 1;
    }
    let per_group: Vec<f32> = sum
        .iter()
        .zip(&count)
        .map(|(s, &c)| if c > 0 { s / c as f32 } else { 0.0 })
        .collect();
    group_of.iter().map(|&g| per_group[g]).collect()
}

/// Returns true if every triangle in the mesh has a non-degenerate UV area.
/// Used by tests and import diagnostics.
#[allow(dead_code)]
pub(crate) fn uv_area(vertices: &[Vertex], indices: &[u32]) -> f32 {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec2::from(vertices[i as usize].uv));
            ((b - a).perp_dot(c - a) * 0.5).abs()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> (Vec<Vertex>, Vec<u32>) {
        let v = |p: [f32; 3], uv: [f32; 2]| Vertex {
            position: p,
            uv,
            ..Default::default()
        };
        (
            vec![
                v([0.0, 0.0, 0.0], [0.0, 0.0]),
                v([1.0, 0.0, 0.0], [1.0, 0.0]),
                v([1.0, 1.0, 0.0], [1.0, 1.0]),
                v([0.0, 1.0, 0.0], [0.0, 1.0]),
            ],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    #[test]
    fn flat_quad_normals_point_along_z() {
        let (mut v, i) = quad();
        generate_normals(&mut v, &i);
        for vert in &v {
            assert!((Vec3::from(vert.normal) - Vec3::Z).length() < 1e-6);
        }
    }

    #[test]
    fn seam_vertices_share_normal() {
        // Two triangles folded at 90°, with the shared edge duplicated (a UV seam).
        let v = |p: [f32; 3]| Vertex {
            position: p,
            ..Default::default()
        };
        let mut verts = vec![
            v([0.0, 0.0, 0.0]),
            v([1.0, 0.0, 0.0]),
            v([0.0, 1.0, 0.0]),
            v([0.0, 0.0, 0.0]),
            v([0.0, 1.0, 0.0]),
            v([0.0, 0.0, 1.0]),
        ];
        let idx = [0, 1, 2, 3, 4, 5];
        generate_normals(&mut verts, &idx);
        assert_eq!(verts[0].normal, verts[3].normal);
        assert_eq!(verts[2].normal, verts[4].normal);
    }

    #[test]
    fn tangents_follow_u_direction() {
        let (mut v, mut i) = quad();
        generate_normals(&mut v, &i);
        generate_tangents(&mut v, &mut i);
        for vert in &v {
            let t = Vec3::new(vert.tangent[0], vert.tangent[1], vert.tangent[2]);
            assert!((t - Vec3::X).length() < 1e-4, "tangent {t:?}");
            assert_eq!(vert.tangent[3].abs(), 1.0);
            // The bitangent points up the image (toward smaller v): here world -Y,
            // because v grows with +Y in this quad.
            let b = Vec3::from(vert.normal).cross(t) * vert.tangent[3];
            assert!((b - Vec3::NEG_Y).length() < 1e-4, "bitangent {b:?}");
        }
        assert!((uv_area(&v, &i) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fallback_tangent_is_orthogonal() {
        for n in [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(1.0, 2.0, 3.0).normalize(),
        ] {
            let t = fallback_tangent(n);
            assert!(t.dot(n).abs() < 1e-5);
            assert!((t.length() - 1.0).abs() < 1e-5);
        }
    }
}
