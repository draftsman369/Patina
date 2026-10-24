# Patina — Roadmap

**Patina** is an open-source, local-first 3D texture painting application. It covers the same feature set as Adobe Substance 3D Painter:

- PBR painting directly on meshes
- a layer stack with masks and effects
- mesh-map baking
- smart materials
- texture export for game engines and renderers

It is free software under **GPL-3.0-or-later**, in the spirit of Blender.

> Patina reimplements *features and workflows*. It must never reuse Adobe code, assets, icons, branding, shelf content or proprietary file formats it isn't legally allowed to read. Interoperability goes through open formats: glTF, OBJ, FBX, PNG, EXR, TIFF, OpenColorIO and MaterialX.

---

## 0. Ground rules

| Topic | Decision |
|---|---|
| Language | Rust (stable, edition 2024) |
| GPU | `wgpu` (Vulkan / Metal / DX12; no OpenGL requirement) |
| UI | `egui` via `eframe`, with a custom dark theme and dockable panels (`egui_dock`) |
| Math | `glam` |
| Mesh I/O | `gltf`, `tobj` (OBJ), an FBX reader (`fbxcel` or a custom one), `mikktspace` |
| Images | `image` (PNG/JPEG/TGA/BMP/TIFF/HDR), `exr` for OpenEXR, `psd` writer later |
| Color | OpenColorIO configs (sRGB, ACEScg); 8/16/32-bit pipelines |
| License | GPL-3.0-or-later; third-party crates must be GPL-compatible (MIT/Apache/BSD/zlib) |
| Platforms | Windows, Linux, macOS — CI builds all three on every PR |
| Repo | Cargo workspace (see §2), GitHub Actions CI, release binaries per tag |

Definition of done for every milestone:

- builds green on the 3 OS
- unit tests pass
- a headless *snapshot* render test passes, using lavapipe on CI
- the feature is documented in `docs/`
- the demo project is updated

---

## 1. Feature map (Substance Painter → Patina)

The checklist below is the full scope. Each item is tagged with the milestone that delivers it.

### 1.1 Project & files

- [ ] New project dialog: mesh, template, document resolution, normal map format (OpenGL/DirectX), tangent space per-fragment, UDIM vs texture-set-per-material — **M1/M2**
- [ ] Mesh import: OBJ, glTF/GLB — **M1**; FBX — **M2**; USD — **M12**
- [x] One texture set per material; texture set list panel — **M1**
- [ ] UDIM / UV tiles workflow (paint across tiles) — **M10**
- [ ] Mesh reimport with layer reprojection (UV or 3D projection) — **M10**
- [ ] Auto-unwrap for meshes without UVs (via `xatlas` bindings) — **M10**
- [ ] Project save/load (`.patina`): zip of JSON scene + layer bitmaps (EXR/PNG) + mesh, incremental autosave, crash recovery — **M3**
- [ ] Project templates (channel setup, shader, resolution) — **M3**
- [ ] Recent files, drag-and-drop import of meshes/images/materials — **M1/M3**

### 1.2 Viewport & display

- [x] 3D viewport: orbit/pan/zoom (Alt+LMB/MMB/RMB, wheel, MMB fallback), frame (F), camera presets, ortho toggle — **M1**
- [x] 2D UV view, split 3D/2D (F1/F2/F3), UV wireframe overlay — **M1**
- [x] Real-time PBR (metal/rough) with an analytic studio environment — **M1**
- [ ] HDRI environments (.hdr/.exr), prefiltered IBL (specular mip chain + irradiance SH), rotation (Shift+RMB), exposure, blur, background opacity — **M2**
- [ ] Spec/gloss shader, unlit shader, custom shader slots (WGSL) — **M7**
- [ ] Single-channel views (C/M/B shortcuts): base color, metallic, roughness, normal, height, AO, mesh maps — **M1/M2**
- [ ] Wireframe overlay — **M1**; shadows (shadow-mapped key light), SSAO — **M7**
- [ ] Tonemapping (ACES, Filmic, Linear), color profile via OCIO — **M1/M11**
- [ ] Post effects: DoF, bloom/glare, vignette, color correction, tone mapping LUTs — **M11**
- [ ] Camera manager (save/restore cameras, FOV, focal length) — **M7**
- [ ] Offline path-traced render mode (Iray equivalent) via a wgpu compute path tracer, with denoiser (OIDN bindings) — **M11**
- [ ] Viewer for histograms, texture set comparison (A/B) — **M11**

### 1.3 Baking (mesh maps)

All bakers run on the GPU in compute shaders.

- [ ] High-poly → low-poly with ray distance (front/rear), cage mesh, average normals, match by mesh name — **M2**
- [ ] Normal (tangent space, MikkTSpace exact), World space normal — **M2**
- [ ] Ambient occlusion (with ignore-backface, self-occlusion by name) — **M2**
- [ ] Curvature (from mesh and from normal), Position, Thickness, Height — **M2**
- [ ] ID map (from vertex color, material color, mesh ID, polygroup) — **M2**
- [ ] Opacity, Bent normals — **M2**
- [ ] Anti-aliasing (supersampling), dilation/padding, per-map resolution — **M2**
- [ ] Import external mesh maps into texture set slots — **M2**

### 1.4 Channels & texture sets

- [ ] Channels: Base Color, Metallic, Roughness, Normal, Height, Emissive, Opacity, AO, Specular, Glossiness, Displacement, Transmissive, Scattering, Anisotropy level/angle, Sheen, Coat… + user channels 0–7 — **M3/M7**
- [ ] Per-channel format: sRGB8, L8, RGB16, RGB16F, RGB32F — **M3**
- [ ] Height → normal conversion and combined normal (height + normal blending) — **M3**
- [ ] Texture set settings: resolution (128 → 8192), channel list, mesh maps slots — **M3**

### 1.5 Layer stack

This is the core of the tool.

- [ ] Paint layer, Fill layer, Folder (group), Instance layer (linked) — **M3**
- [ ] Per-channel blend modes (Normal, Multiply, Screen, Overlay, Linear Dodge/Burn, Darken, Lighten, Difference, Subtract, Divide, Color Dodge/Burn, Soft/Hard/Vivid/Linear/Pin Light, Hue, Saturation, Color, Value, Inverse Divide, Replace, Passthrough, Disable, Normal-map Combine / Detail / Inverse) + per-channel opacity — **M3**
- [ ] Layer opacity, visibility, lock, rename, duplicate, reorder by drag, copy/paste between texture sets — **M3**
- [ ] GPU compositing: each channel is a render target; recompose only dirty tiles — **M3**
- [ ] Undo/redo for every operation (command pattern + bitmap tile snapshots) — **M3**
- [ ] Material layering / layer instancing across texture sets — **M9**

### 1.6 Masks & effects

- [ ] Black/white masks; add mask with: white, black, bitmap, ID color selection — **M5**
- [ ] Effects inside layers and masks: Fill, Paint, Filter, Generator, Levels, Compare Mask, Color Selection, Anchor Point — **M5**
- [ ] Generators: Ambient Occlusion, Curvature, Position, Thickness, Light, Dirt, Edge wear, Mask Editor (multi-mesh-map mask builder), Metal edge wear, Dripping rust, Fiber glass… (Patina-original implementations) — **M5/M6**
- [ ] Filters: Blur, Directional blur, Sharpen, Warp, HSL, Levels, Contrast/Luminosity, Invert, Gradient map, Bake to layer, MatFX (normal/height manipulations) — **M5**
- [ ] Anchor points: reference a layer's channel downstream (e.g. height details in masks) — **M5**
- [ ] Smart masks: save a mask + its effects as a reusable asset — **M6**

### 1.7 Tools (left toolbar)

| # | Tool | Shortcut | Milestone |
|---|---|---|---|
| 1 | Paint brush | 1 | **M4** |
| 2 | Eraser | 2 | **M4** |
| 3 | Projection (stamp an image/material projected from screen) | 3 | **M4** |
| 4 | Polygon fill: triangle / quad / polygon / UV island / mesh, with ID | 4 | **M4** |
| 5 | Smudge | 5 | **M8** |
| 6 | Clone (from source point, 3D aware) | 6 | **M8** |
| 7 | Color picker (pick any channel value on the mesh) | P | **M4** |
| 8 | Path tool (spline strokes, editable after the fact) | — | **M8** |
| 9 | Physical paint (particle brushes: rain, dirt, fire, leaks…) | — | **M8** |
| 10 | Text tool (text resource as stamp) | — | **M8** |

Brush engine features:

- **M4** — size, flow, opacity, spacing, angle, follow-path, jitter (size/flow/angle/position), stroke smoothing (lazy mouse), pen pressure and tilt (via `winit`/tablet APIs), alpha shapes
- **M4** — material painting: one stroke paints every enabled channel
- **M4** — symmetry (mirror X/Y/Z, radial, in local or world space), paint through UV seams without artifacts (3D-space stamping into UV space with dilation)
- **M4** — stencils: image on screen, moved and scaled with S + mouse
- **M8** — dynamic strokes, sticky alpha (backface culling, angle fade), line strokes (Shift+click)
- **M8** — brush presets saved in the shelf

### 1.8 Fill & projection modes

- [ ] Fill projections: UV, Tri-planar (hardness, per-axis offset), Planar, Spherical, Cylindrical, Warp — **M3/M5**
- [ ] UV transforms: scale, rotation, offset, wrap modes — **M3**
- [ ] Triplanar on paint stamps (projection tool) — **M4**

### 1.9 Materials, smart materials & procedurals

Patina's own **procedural material graph** replaces `.sbsar`, which needs Adobe's proprietary engine and can't be supported:

- [ ] Node graph editor (egui node editor) — **M9**
- [ ] Nodes: noise (Perlin, Voronoi/cells, FBM, clouds), patterns (tiles, bricks, waves), blend, levels, warp, blur, slope blur, edge detect, height-to-normal, gradient map — **M9**
- [ ] Exposed parameters appear in the Properties panel like any fill material — **M9**
- [ ] Export/import graphs as `.patmat` (JSON); optional MaterialX export — **M9**
- [ ] Smart materials: save a folder of layers + masks + generators as one asset; re-adapts to any mesh through its mesh maps — **M6**
- [ ] Bitmap materials: drag a folder of PBR textures (auto-detect channel by file name suffix) — **M3**
- [ ] Starter library shipped with Patina (CC0 content only), e.g. ~30 materials, ~15 smart materials, ~20 smart masks, ~50 alphas, ~10 HDRIs (Poly Haven, CC0) — **M6/M12**

### 1.10 Shelf / asset browser

- [ ] Categories: Materials, Smart materials, Smart masks, Brushes, Alphas, Textures, Generators, Filters, Environments, Particles, Projects — **M6**
- [ ] Search, tags, thumbnails rendered by Patina (material ball), favorites — **M6**
- [ ] Multiple library folders (user, project, shared team folder) — **M6**
- [ ] Import resources dialog (as project / session / library) — **M6**

### 1.11 Export

- [ ] Export textures dialog: output folder, file format (PNG, TGA, TIFF, EXR, JPEG, PSD), bit depth, padding/dilation, resolution override, per texture set — **M7**
- [ ] Output templates (presets) with channel packing editor (e.g. ORM, MRA, R=Metal G=Rough B=AO): Unreal Engine (packed), Unity (HDRP mask map, URP/Built-in metallic smoothness), Godot, glTF PBR, Blender Principled, Arnold, V-Ray, Redshift, Document channels + mesh maps — **M7**
- [ ] Normal map Y-flip (DirectX/OpenGL) on export, roughness ↔ glossiness inversion — **M7**
- [ ] Export glTF/GLB with textures embedded — **M7**
- [ ] Export to PSD with layers (best-effort) — **M12**

### 1.12 Usability

- [ ] Fully dockable/undockable panels, saved workspace layouts — **M7**
- [ ] Customizable shortcuts (keymap editor, import/export) — **M7**
- [ ] Preferences: GPU selection, memory budget, autosave interval, paths, language — **M7**
- [ ] Performance: tiled sparse textures, GPU memory budget, async thumbnailing, background compositing — **M10**
- [ ] Localization (English, French first) — **M12**

### 1.13 Extensibility

- [ ] Python scripting API (via `pyo3`) — **M12**
  - scope: project/layers/export automation
  - plugin panels
  - events (project opened, export finished)
- [ ] Command-line batch mode: `patina --bake`, `--export`, headless — **M7/M12**
- [ ] Live link plugins (later, community): Blender, Godot, Unreal — **M12+**

---

## 2. Architecture

```
patina/                       (Cargo workspace)
├─ crates/
│  ├─ patina_core/            project model, layers, commands/undo, serialization (no GPU, no UI)
│  ├─ patina_mesh/            mesh data, import (obj/gltf/fbx), MikkTSpace, BVH, UV utils
│  ├─ patina_gpu/             wgpu context, texture pool, tiled textures, compute helpers
│  ├─ patina_render/          viewport renderer: PBR, IBL, shadows, SSAO, post FX, path tracer
│  ├─ patina_bake/            GPU bakers (ray casting on BVH in compute)
│  ├─ patina_paint/           brush engine, stroke → UV-space stamping, dilation
│  ├─ patina_compositor/      layer stack evaluation, blend modes, masks, effects
│  ├─ patina_procedural/      node graph + procedural nodes (generators, materials)
│  ├─ patina_export/          output templates, channel packing, file writers
│  ├─ patina_assets/          shelf/library, thumbnails, resource import
│  ├─ patina_script/          Python API (feature-gated)
│  └─ patina_app/             egui editor (panels, tools, keymap) → `patina` binary
├─ assets/                    built-in CC0 content, shaders, icons (original artwork)
├─ docs/                      user manual (mdBook) + developer docs
├─ tests/snapshots/           reference images for render regression tests
└─ .github/workflows/         ci.yml (fmt, clippy, test, snapshot), release.yml
```

Key technical decisions:

1. **Painting happens in UV space, driven from 3D.**
   - A brush stamp is a 3D footprint, a sphere/cylinder around the hit point oriented by the surface normal.
   - Each stroke step rasterizes the mesh in UV space (a vertex shader outputs UVs as positions) and tests texels against the footprint using the position map.
   - This handles seams and UV islands natively, which is the approach used by professional painters.
2. **Picking** uses a GPU ID + position buffer rendered every frame, so you get the exact hit position, normal, UV and triangle under the cursor.
3. **Layer storage.**
   - Each paint layer stores per-channel sparse tiles (e.g. 256×256) in GPU memory, with CPU mirrors for undo/save.
   - Fill layers are procedural: parameters only, evaluated in shaders.
4. **Compositing.** Per texture set and per channel, a compute/fragment pass walks the stack bottom-up into ping-pong render targets. Dirty-region tracking means only touched tiles get recomposited.
5. **Undo** uses a command pattern. Bitmap edits store before/after tile copies, compressed with lz4.
6. **Color.**
   - Stored linear internally, in 16F for color/height and 8-bit for masks.
   - Conversion happens on import/export through OCIO.
   - The viewport output is tonemapped.
7. **Determinism.** Generators and procedural nodes are seeded, so exports are reproducible across machines.

---

## 3. Milestones

Each milestone ends with a usable, tagged release.

### M1 — Foundations: viewport & UI shell
Goal: open a mesh and look at it with correct PBR shading in a Painter-like interface.

- [x] Workspace scaffold, GPL-3.0 `LICENSE`, `README`, `CONTRIBUTING`, CI (Win/Linux/macOS)
- [x] eframe + wgpu app with a dark theme
  - menu bar: File / Edit / Mode / Window / Viewport / Help
  - left tools bar
  - right panels: Texture Set List, Layers, Properties, Display Settings
  - bottom shelf
  - status bar
- [x] Offscreen renderer
  - HDR target (Rgba16F, MSAA 4×), depth
  - ACES tonemap pass into an Rgba8 texture displayed in egui
- [x] Orbit camera: Alt+mouse navigation, wheel zoom, F to frame, FOV, ortho
- [x] Analytic studio environments (5 presets), rotation, exposure, background blur
- [x] Metal/rough PBR shader with base color / metallic / roughness / normal / emissive textures + factors
- [x] OBJ + glTF import, material groups → texture sets
  - normals and tangents generated when missing
  - built-in primitives: sphere, cube, cylinder, torus, plane
- [x] Channel views (material, base color, metallic, roughness, normal, world normal, UV checker), wireframe overlay
- [x] 2D UV view with UV wireframe; F1/F2/F3 layouts
- [x] Load an image into a channel slot (test path for the texture pipeline)
- [x] `patina --snapshot out.png` headless render + CI snapshot test

**Exit criteria:** load any glTF from the Khronos sample models and inspect it at 60 fps on integrated GPUs.

### M2 — HDRI lighting & baking

- [ ] HDRI loading (.hdr/.exr)
  - equirect → cubemap
  - GGX prefiltered specular mips
  - irradiance (SH9)
  - BRDF LUT
- [ ] FBX import; exact MikkTSpace tangents
- [ ] BVH (CPU build, GPU traversal in compute)
- [ ] Bakers: normal, world normal, ID, AO, curvature, position, thickness, height, opacity, bent normal
  - high→low with cage
  - match by name
  - supersampling
  - dilation
- [ ] Baking dialog with per-map settings, progress, cancel
- [ ] Mesh map slots in texture set settings; mesh-map viewport views (B shortcut)

**Exit criteria:** baking matches reference bakes from other bakers within tolerance on test meshes.

### M3 — Layer stack & project files

- [ ] Project model in `patina_core`: texture sets → channels → layer stack (paint/fill/folder)
- [ ] GPU compositor with all blend modes, per-channel opacity/blending, height→normal combine
- [ ] Fill layer properties
  - per-channel color/value or bitmap
  - UV / tri-planar / planar projections
  - UV transforms
- [ ] Layers panel: drag-reorder, visibility, opacity, blend menu per channel, rename, duplicate, group into folder
- [ ] Undo/redo (Ctrl+Z / Ctrl+Y) for everything
- [ ] Save/load `.patina`, autosave, templates, recent files
- [ ] Bitmap material import (folder of textures → fill layer)

**Exit criteria:** build a material with 20 fill layers on a 4K texture set and keep the UI responsive.

### M4 — Painting

- [ ] GPU picking buffer (position/normal/UV/triangle ID)
- [ ] Brush engine
  - size/flow/opacity/spacing/hardness/angle
  - jitters
  - lazy mouse
  - pressure (tablet)
  - alpha shapes
- [ ] 3D-aware UV-space stamping, seam-free, with automatic texture padding
- [ ] Material painting (all channels per stroke), channel toggles in the tool's properties
- [ ] Tools: Paint, Eraser, Projection, Polygon fill (tri/quad/polygon/UV island/mesh), Color picker
- [ ] Symmetry (mirror axes, radial), stencils
- [ ] Brush/alpha presets (built-in CC0 alphas)

**Exit criteria:** paint across UV seams on a production asset without visible seams at 4K, with no lag at 120 Hz input.

### M5 — Masks, effects & generators

- [ ] Layer masks (paint + fill + effects inside), mask display (Alt+click)
- [ ] Effect types: fill, paint, filter, generator, levels, compare mask, color selection, anchor point
- [ ] Generators driven by mesh maps: curvature edge-wear, AO dirt, position gradient, thickness, light, mask builder with noise breakup
- [ ] Filters: blur, directional blur, sharpen, warp, HSL, levels, invert, gradient map, contrast
- [ ] Anchor points

**Exit criteria:** recreate a "worn painted metal" material procedurally that adapts to any baked mesh.

### M6 — Shelf, smart materials & smart masks

- [ ] Asset library with categories, search, tags and thumbnails (rendered material balls)
- [ ] Save layer/folder as smart material; save mask as smart mask; drag-and-drop onto layers/texture sets
- [ ] Library folders (user/project/shared), import resources dialog
- [ ] Ship the first CC0 starter library

### M7 — Export & professional usability

- [ ] Export dialog + output templates with a channel packing editor (Unreal, Unity URP/HDRP, Godot, glTF, Blender, Arnold…)
- [ ] PNG/TGA/TIFF/EXR/JPEG writers, 8/16/32-bit, padding, normal Y-flip, glTF export
- [ ] Shader variants: spec/gloss, unlit, user WGSL shaders with channel bindings
- [ ] Shadows + SSAO in viewport, camera manager
- [ ] Docking workspaces, keymap editor, preferences
- [ ] CLI: `patina export project.patina --template unreal --out ./tex`

**Exit criteria:** a full asset round-trip (import → bake → texture → export → engine) in Unreal, Unity and Godot, documented step by step.

### M8 — Advanced painting tools

- [ ] Smudge, Clone (3D source offsets), Path tool (editable splines), Text tool
- [ ] Physical paint: GPU particle simulation (gravity, collision with mesh, wind) emitting paint stamps; presets (rain, leaks, dust, fire burn)
- [ ] Dynamic strokes, sticky alphas, line strokes

### M9 — Procedural material graph & material layering

- [ ] Node graph editor + node library (noises, patterns, filters, blends, height→normal)
- [ ] Graph outputs usable as fill-layer materials, generators or filters; exposed parameters
- [ ] Layer instancing across texture sets; material layering
- [ ] MaterialX export of graphs (optional)

### M10 — Scale & production workflows

- [ ] UDIM / UV tiles: tiled painting across tiles, per-tile resolution
- [ ] Mesh reimport with reprojection of paint layers
- [ ] Auto-unwrap (xatlas) for UV-less meshes
- [ ] Sparse/virtual texturing, 8K+ texture sets, GPU memory budget and eviction
- [ ] Multi-GPU-friendly async compositing, profiling tools (puffin)

### M11 — Rendering & look-dev

- [ ] Path-traced render mode (compute), progressive, denoised
- [ ] Post FX: DoF, bloom/glare, vignette, color grading LUT
- [ ] OCIO color management (ACEScg working space option)
- [ ] Render dialog: resolution, samples, turntable animation export (image sequence / MP4 via ffmpeg if present)

### M12 — Ecosystem

- [ ] Python API + plugin system, script editor panel
- [ ] USD import, PSD layered export
- [ ] Localization, accessibility pass, installer packages (MSI, DMG, AppImage/Flatpak)
- [ ] Documentation site, tutorials, sample projects; 1.0 release

---

## 4. Testing & quality strategy

- **Unit tests**:
  - mesh import, tangent generation, camera math
  - blend modes (compared against reference formulas)
  - channel packing
  - serialization round-trips
- **GPU snapshot tests**:
  - render fixed scenes headless (lavapipe/WARP)
  - compare to reference PNGs with a perceptual tolerance
  - run on every PR
- **Bake regression**: bake known meshes and diff against stored maps.
- **Performance budgets**, tracked in CI:
  - stroke latency below 8 ms at 4K
  - compositing a 20-layer stack at 4K below 50 ms
  - viewport above 60 fps at 1080p on an integrated GPU
- **Fuzzing**: mesh/image importers (`cargo fuzz`).
- **Release checklist**: changelog, signed binaries, sample project opens without warnings.

## 5. Community & governance

- `CONTRIBUTING.md`:
  - code style (rustfmt, clippy pedantic on core crates)
  - commit conventions
  - DCO sign-off
- Issue templates (bug / feature / asset), and a `good first issue` label for UI and asset work.
- Design docs in `docs/rfcs/` for big features: compositor, brush engine, file format.
- Content policy: only CC0/CC-BY assets in the repo, with a license manifest per asset folder.
- File format versioning, with migrations for old `.patina` projects from 1.0 on.

## 6. Out of scope / legal notes

- No reading of `.spp` projects or `.sbsar` archives (proprietary). Interop goes through exported textures and open formats instead.
- No copying of Adobe UI artwork, icons, shelf content or documentation text. Workflows and feature names that are generic industry terms (fill layer, smart material, mesh maps) are fine.
- No Adobe trademarks in the product name, marketing or UI.
