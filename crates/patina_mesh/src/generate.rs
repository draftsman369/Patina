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

struct MikkGeometry<'a> {
    vertices: &'a mut [Vertex],
    indices: &'a [u32],
}

impl MikkGeometry<'_> {
    fn vertex(&self, face: usize, vert: usize) -> &Vertex {
        &self.vertices[self.indices[face * 3 + vert] as usize]
    }
}

impl mikktspace::Geometry for MikkGeometry<'_> {
    fn num_faces(&self) -> usize {
        self.indices.len() / 3
    }

    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }

    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        self.vertex(face, vert).position
    }

    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        self.vertex(face, vert).normal
    }

    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        self.vertex(face, vert).uv
    }

    fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
        // Indexed meshes share vertices between faces; MikkTSpace may produce
        // slightly different tangents per face. Exact per-corner splitting
        // comes with the M2 baking work.
        let i = self.indices[face * 3 + vert] as usize;
        self.vertices[i].tangent = tangent;
    }
}

/// Generates MikkTSpace tangents. Requires normals and UVs.
///
/// Falls back to an arbitrary orthogonal tangent when the geometry is
/// degenerate (no UVs, zero-area UV triangles).
pub fn generate_tangents(vertices: &mut [Vertex], indices: &[u32]) {
    let ok = !indices.is_empty()
        && mikktspace::generate_tangents(&mut MikkGeometry { vertices, indices });
    if !ok {
        log::warn!("MikkTSpace failed; using fallback tangents");
    }
    for v in vertices.iter_mut() {
        let n = Vec3::from(v.normal);
        let t = Vec3::new(v.tangent[0], v.tangent[1], v.tangent[2]);
        if !ok || !t.is_finite() || t.length_squared() < 1e-12 {
            v.tangent = fallback_tangent(n).extend(1.0).to_array();
        }
    }
}

fn fallback_tangent(n: Vec3) -> Vec3 {
    let helper = if n.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    helper.cross(n).normalize_or(Vec3::X)
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
        let (mut v, i) = quad();
        generate_normals(&mut v, &i);
        generate_tangents(&mut v, &i);
        for vert in &v {
            let t = Vec3::new(vert.tangent[0], vert.tangent[1], vert.tangent[2]);
            assert!((t - Vec3::X).length() < 1e-4, "tangent {t:?}");
            assert_eq!(vert.tangent[3].abs(), 1.0);
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
