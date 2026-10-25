# Patina

**Patina** is an open-source, local-first 3D texture painting application:
PBR painting on meshes, a layer stack with masks and effects, mesh-map baking,
smart materials and texture export for game engines. It is free software under
the **GNU GPL v3 or later**.

> Status: early development. Milestones **M1 (viewport & UI shell)** and
> **M2 (HDRI lighting & baking)** are done: you can open meshes, light them with
> HDRIs and bake mesh maps. Layers arrive in M3 and painting in M4. See
> [ROADMAP.md](ROADMAP.md) for the full plan.

## What works today

- Open **OBJ** (with MTL), **glTF / GLB** and **FBX** meshes, or start from
  built-in primitives (sphere, cube, cylinder, torus, plane).
- One **texture set per material**, seeded from the file's PBR factors and textures.
- Real-time **metal/rough PBR** viewport with HDR rendering, 4× MSAA and
  **ACES / Filmic / Linear** tonemapping.
- **Image-based lighting** from HDRIs (`.hdr`, `.exr`) or five built-in studio
  presets: GGX-prefiltered reflections, SH9 diffuse, rotation, exposure, background blur.
- **Channel views**: material, base color, metallic, roughness, normal,
  world normal, UV checker, emissive. Optional **wireframe** overlay.
- **2D UV view** with the UV layout over the current channel; F1/F2/F3 layouts.
- Load an image into any channel slot (Properties panel or drag-and-drop).
- **GPU mesh-map baking**: normal, world normal, ID, AO, curvature, position,
  thickness, height, opacity and bent normal, from the mesh itself or a
  high-poly (cage, match by name, supersampling, dilation). Results show in
  the viewport with **B**.
- Headless rendering and baking: `patina --snapshot out.png`,
  `patina low.fbx --bake out/ --high high.fbx`.

## Building

Requires a recent stable [Rust toolchain](https://rustup.rs) (1.88+) and a GPU
with Vulkan, Metal or DirectX 12.

```sh
cargo run --release -p patina_app            # start the editor
cargo run --release -p patina_app -- model.glb
cargo run --release -p patina_app -- --snapshot out.png --primitive torus
```

A C compiler is needed for the FBX importer (ufbx). On Linux you also need
`pkg-config`, `libxkbcommon-dev` and `libwayland-dev`.

## Viewport controls

| Action | Input |
|---|---|
| Orbit | Alt + left drag, or right drag |
| Pan | Middle drag |
| Zoom | Wheel, or Alt + right drag |
| Rotate environment | Shift + right drag |
| Frame mesh | F |
| Layouts 3D+2D / 3D / 2D | F1 / F2 / F3 |
| Cycle channel views / mesh maps / material view | C / B / M |

## Repository layout

```
crates/
  patina_core/     project model: texture sets, channels, images
  patina_mesh/     mesh data, OBJ/glTF/FBX import, primitives, tangents, BVH
  patina_render/   wgpu viewport renderer, image-based lighting, UV view, snapshots
  patina_bake/     GPU mesh-map bakers
  patina_app/      egui editor and the `patina` binary
docs/              user and developer documentation (mdBook)
tests/snapshots/   reference images for GPU render tests
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Patina reimplements workflows, never
proprietary code, assets or file formats; see the legal notes in the roadmap.

## License

Patina is licensed under the [GNU General Public License v3.0 or later](LICENSE).
