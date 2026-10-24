# Architecture

Patina is a Cargo workspace. Crates added so far:

| Crate | Role | Depends on |
|---|---|---|
| `patina_core` | Project model: `Project`, `TextureSet`, `Channel`, `Image`. No GPU, no UI. | `image` |
| `patina_mesh` | `Mesh` (one vertex/index buffer, one part per material), OBJ/glTF import, primitives, normal and MikkTSpace tangent generation | core |
| `patina_render` | wgpu renderer: viewport, UV view, camera, environments, headless device and readback | core, mesh |
| `patina_app` | egui/eframe editor, CLI (`patina` binary) | all of the above |

The remaining crates from the roadmap (`patina_gpu`, `patina_bake`,
`patina_paint`, `patina_compositor`, …) are created when their milestone starts.

## Data flow

1. A mesh file is imported into a `patina_mesh::Mesh`. Each material becomes a
   `MeshPart` (a contiguous index range) and a `MaterialDesc`.
2. `Mesh::texture_sets()` seeds one `patina_core::TextureSet` per material.
3. `Renderer::create_scene` uploads the mesh and one `GpuMaterial` per texture set.
4. Each frame the editor calls `render_viewport` / `render_uv`, which draw
   into offscreen textures registered with egui.

## Editor structure

`patina_app` keeps all state in `PatinaApp`. UI code never mutates GPU
resources directly: it queues an `Action` (open mesh, load image into a channel,
factors changed…), and `apply_actions` runs them after the UI is drawn. This
keeps borrow scopes simple and is the seed of the undo/redo command system (M3).
