//! Prints statistics about a mesh file: `cargo run -p patina_mesh --example mesh_info -- file.glb`
fn main() {
    let path = std::env::args().nth(1).expect("usage: mesh_info <file>");
    let mesh = patina_mesh::load(&path).expect("load failed");
    let (mut umin, mut umax) = ([f32::MAX; 2], [f32::MIN; 2]);
    for v in &mesh.vertices {
        for c in 0..2 {
            umin[c] = umin[c].min(v.uv[c]);
            umax[c] = umax[c].max(v.uv[c]);
        }
    }
    let b = mesh.bounds();
    println!(
        "{}: {} vertices, {} triangles, has_uvs={}",
        mesh.name,
        mesh.vertices.len(),
        mesh.triangle_count(),
        mesh.has_uvs
    );
    println!("uv range: {umin:?} .. {umax:?}");
    println!("bounds: {:?} .. {:?}", b.min, b.max);
    println!("objects: {:?}", mesh.objects);
    for (p, m) in mesh.parts.iter().zip(&mesh.materials) {
        println!("part {}: {} triangles", m.name, p.index_count / 3);
    }
}
