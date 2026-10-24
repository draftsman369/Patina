# Architecture

Patina is a Cargo workspace. Crates added so far:

| Crate | Role | Depends on |
|---|---|---|
| `patina_core` | Project model: `Project`, `TextureSet`, `Channel`, `MeshMap`, `Image`, `HdrImage`. No GPU, no UI. | `image` |
| `patina_mesh` | `Mesh` (one vertex/index buffer, one part per material, object ids per triangle), OBJ/glTF/FBX import, primitives, normals, MikkTSpace tangents, curvature, BVH | core |
| `patina_render` | wgpu renderer: viewport, image-based lighting, UV view, camera, headless device and readback | core, mesh |
| `patina_bake` | GPU mesh-map bakers | core, mesh |
| `patina_app` | egui/eframe editor, CLI (`patina` binary) | all of the above |

The remaining crates from the roadmap (`patina_paint`, `patina_compositor`,
…) are created when their milestone starts. `patina_gpu` will be split out of
`patina_render` once painting and compositing share GPU helpers.

## Data flow

1. A mesh file is imported into a `patina_mesh::Mesh`. Each material becomes a
   `MeshPart` (a contiguous index range) and a `MaterialDesc`.
2. `Mesh::texture_sets()` seeds one `patina_core::TextureSet` per material.
3. `Renderer::create_scene` uploads the mesh and one `GpuMaterial` per texture set.
4. Each frame the editor calls `render_viewport` / `render_uv`, which draw
   into offscreen textures registered with egui.
5. Baking runs `patina_bake::Baker` on a worker thread with the editor's
   device; results are stored in `TextureSet::mesh_maps` and the material is
   re-uploaded.

## Editor structure

`patina_app` keeps all state in `PatinaApp`. UI code never mutates GPU
resources directly: it queues an `Action` (open mesh, load image into a channel,
load an HDRI, factors changed…), and `apply_actions` runs them after the UI is
drawn. This keeps borrow scopes simple and is the seed of the undo/redo
command system (M3).
