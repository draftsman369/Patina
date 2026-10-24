//! Built-in primitive meshes. All have non-overlapping UVs in [0, 1] so they
//! can be painted directly.

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use crate::{MaterialDesc, Mesh, MeshPart, Vertex, generate_tangents};

/// Primitive shapes available from the File > New menu and the CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    Sphere,
    Cube,
    Cylinder,
    Torus,
    Plane,
}

impl Primitive {
    pub const ALL: [Primitive; 5] = [
        Primitive::Sphere,
        Primitive::Cube,
        Primitive::Cylinder,
        Primitive::Torus,
        Primitive::Plane,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Primitive::Sphere => "Sphere",
            Primitive::Cube => "Cube",
            Primitive::Cylinder => "Cylinder",
            Primitive::Torus => "Torus",
            Primitive::Plane => "Plane",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(name))
    }

    pub fn build(self) -> Mesh {
        match self {
            Primitive::Sphere => sphere(1.0, 64, 32),
            Primitive::Cube => cube(1.6),
            Primitive::Cylinder => cylinder(0.8, 1.8, 64),
            Primitive::Torus => torus(1.0, 0.4, 96, 48),
            Primitive::Plane => plane(2.0),
        }
    }
}

fn vertex(position: Vec3, normal: Vec3, uv: [f32; 2]) -> Vertex {
    Vertex {
        position: position.to_array(),
        normal: normal.to_array(),
        uv,
        ..Default::default()
    }
}

fn finish(name: &str, mut vertices: Vec<Vertex>, mut indices: Vec<u32>) -> Mesh {
    generate_tangents(&mut vertices, &mut indices);
    let triangles = indices.len() / 3;
    Mesh {
        name: name.to_owned(),
        parts: vec![MeshPart {
            material: 0,
            first_index: 0,
            index_count: indices.len() as u32,
        }],
        vertices,
        indices,
        materials: vec![MaterialDesc::named(name)],
        has_uvs: true,
        objects: vec![name.to_owned()],
        triangle_objects: vec![0; triangles],
    }
}

/// UV sphere.
pub fn sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut vertices = Vec::new();
    for i in 0..=rings {
        let theta = PI * i as f32 / rings as f32;
        for j in 0..=segments {
            let phi = TAU * j as f32 / segments as f32;
            let n = Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                theta.sin() * phi.sin(),
            );
            vertices.push(vertex(
                n * radius,
                n,
                [j as f32 / segments as f32, i as f32 / rings as f32],
            ));
        }
    }
    let row = segments + 1;
    let mut indices = Vec::new();
    for i in 0..rings {
        for j in 0..segments {
            let a = i * row + j;
            let b = a + row;
            let c = b + 1;
            let d = a + 1;
            // Skip the zero-area triangles that touch the poles.
            if i != rings - 1 {
                indices.extend([a, c, b]);
            }
            if i != 0 {
                indices.extend([a, d, c]);
            }
        }
    }
    finish("Sphere", vertices, indices)
}

/// Cube with each face in its own cell of a 3x2 UV atlas.
pub fn cube(size: f32) -> Mesh {
    let h = size * 0.5;
    // (normal, u axis, v axis) with u × v = normal so corners wind CCW.
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    let pad = 0.01;
    let (cw, ch) = (1.0 / 3.0, 0.5);
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (f, (n, u, v)) in faces.into_iter().enumerate() {
        let (x0, y0) = ((f % 3) as f32 * cw, (f / 3) as f32 * ch);
        let base = vertices.len() as u32;
        for (s, t) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let p = n * h + u * (s * 2.0 - 1.0) * h + v * (t * 2.0 - 1.0) * h;
            let uv = [
                x0 + pad + s * (cw - 2.0 * pad),
                y0 + pad + (1.0 - t) * (ch - 2.0 * pad),
            ];
            vertices.push(vertex(p, n, uv));
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    finish("Cube", vertices, indices)
}

/// Capped cylinder along Y. The side uses the top half of UV space, the
/// caps sit side by side in the bottom half.
pub fn cylinder(radius: f32, height: f32, segments: u32) -> Mesh {
    let hh = height * 0.5;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    let angle = |j: u32| TAU * j as f32 / segments as f32;

    // Side: bottom row then top row per segment column.
    for j in 0..=segments {
        let phi = angle(j);
        let n = Vec3::new(phi.sin(), 0.0, phi.cos());
        let u = j as f32 / segments as f32;
        vertices.push(vertex(n * radius - Vec3::Y * hh, n, [u, 0.49]));
        vertices.push(vertex(n * radius + Vec3::Y * hh, n, [u, 0.0]));
    }
    for j in 0..segments {
        let a = j * 2;
        let (b, c, d) = (a + 2, a + 3, a + 1);
        indices.extend([a, b, c, a, c, d]);
    }

    // Caps.
    let cap_radius = 0.23;
    for (y, n, uv_center) in [
        (hh, Vec3::Y, [0.25, 0.75]),
        (-hh, Vec3::NEG_Y, [0.75, 0.75]),
    ] {
        let center = vertices.len() as u32;
        vertices.push(vertex(Vec3::Y * y, n, uv_center));
        for j in 0..=segments {
            let phi = angle(j);
            let (s, c) = phi.sin_cos();
            vertices.push(vertex(
                Vec3::new(s * radius, y, c * radius),
                n,
                [uv_center[0] + s * cap_radius, uv_center[1] + c * cap_radius],
            ));
        }
        for j in 0..segments {
            let (p0, p1) = (center + 1 + j, center + 2 + j);
            if y > 0.0 {
                indices.extend([center, p0, p1]);
            } else {
                indices.extend([center, p1, p0]);
            }
        }
    }
    finish("Cylinder", vertices, indices)
}

/// Torus around the Y axis.
pub fn torus(major_radius: f32, minor_radius: f32, rings: u32, sides: u32) -> Mesh {
    let mut vertices = Vec::new();
    for i in 0..=rings {
        let phi = TAU * i as f32 / rings as f32;
        let (sp, cp) = phi.sin_cos();
        for j in 0..=sides {
            let theta = TAU * j as f32 / sides as f32;
            let (st, ct) = theta.sin_cos();
            let n = Vec3::new(ct * cp, st, ct * sp);
            let center = Vec3::new(major_radius * cp, 0.0, major_radius * sp);
            vertices.push(vertex(
                center + n * minor_radius,
                n,
                [i as f32 / rings as f32, j as f32 / sides as f32],
            ));
        }
    }
    let row = sides + 1;
    let mut indices = Vec::new();
    for i in 0..rings {
        for j in 0..sides {
            let a = i * row + j;
            let b = a + row;
            let c = b + 1;
            let d = a + 1;
            indices.extend([a, c, b, a, d, c]);
        }
    }
    finish("Torus", vertices, indices)
}

/// Square plane facing +Y.
pub fn plane(size: f32) -> Mesh {
    let h = size * 0.5;
    let (n, u, v) = (Vec3::Y, Vec3::X, Vec3::NEG_Z);
    let mut vertices = Vec::new();
    for (s, t) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let p = u * (s * 2.0 - 1.0) * h + v * (t * 2.0 - 1.0) * h;
        vertices.push(vertex(p, n, [s, 1.0 - t]));
    }
    finish("Plane", vertices, vec![0, 1, 2, 0, 2, 3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_valid() {
        for p in Primitive::ALL {
            let mesh = p.build();
            assert!(!mesh.indices.is_empty(), "{p:?}");
            assert_eq!(mesh.indices.len() % 3, 0);
            assert!(
                mesh.indices
                    .iter()
                    .all(|&i| (i as usize) < mesh.vertices.len()),
                "{p:?} index out of range"
            );
            for v in &mesh.vertices {
                let n = Vec3::from(v.normal);
                assert!((n.length() - 1.0).abs() < 1e-4, "{p:?} normal not unit");
                assert!(
                    v.uv.iter().all(|c| (0.0..=1.0).contains(c)),
                    "{p:?} uv {:?} out of [0,1]",
                    v.uv
                );
                let t = Vec3::new(v.tangent[0], v.tangent[1], v.tangent[2]);
                assert!(t.is_finite() && t.length() > 0.5, "{p:?} bad tangent");
            }
        }
    }

    #[test]
    fn primitives_wind_counter_clockwise_outward() {
        for p in Primitive::ALL {
            let mesh = p.build();
            for tri in mesh.indices.as_chunks::<3>().0 {
                let v = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
                let [a, b, c] = v.map(|v| Vec3::from(v.position));
                let face = (b - a).cross(c - a);
                if face.length_squared() < 1e-12 {
                    continue;
                }
                let avg: Vec3 = v.iter().map(|v| Vec3::from(v.normal)).sum();
                assert!(face.dot(avg) > 0.0, "{p:?} has an inward-facing triangle");
            }
        }
    }

    #[test]
    fn from_name_is_case_insensitive() {
        assert_eq!(Primitive::from_name("torus"), Some(Primitive::Torus));
        assert_eq!(Primitive::from_name("CUBE"), Some(Primitive::Cube));
        assert_eq!(Primitive::from_name("teapot"), None);
    }
}
