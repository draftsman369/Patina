# Command line

```sh
patina [MESH]                                  # open the editor, optionally with a mesh
patina [MESH] --snapshot out.png               # render one frame headless and exit
patina MESH --bake out_dir [--high HIGH]       # bake mesh maps headless and exit
```

## Snapshot options

| Option | Default | Meaning |
|---|---|---|
| `--primitive NAME` | `sphere` | Primitive used when no mesh is given: sphere, cube, cylinder, torus, plane |
| `--view MODE` | `material` | material, base-color, metallic, roughness, normal, world-normal, uv-checker, emissive |
| `--env N` | `0` | Environment preset index (0–4) |
| `--hdri FILE` | — | HDRI environment (`.hdr` / `.exr`), overrides `--env` |
| `--exposure EV` | `0` | Exposure in stops |
| `--size WxH` | `1280x720` | Output size, up to 8192×8192 |
| `--wireframe` | off | Draw the wireframe overlay |

## Bake options

| Option | Default | Meaning |
|---|---|---|
| `--high FILE` | — | High-poly mesh; without it MESH bakes onto itself |
| `--cage FILE` | — | Cage with the same triangles as MESH |
| `--maps LIST` | `normal,ao,curvature,thickness,position,world_normal,id` | Comma-separated: normal, world_normal, id, ao, curvature, position, thickness, height, opacity, bent_normal, or `all` |
| `--bake-size N` | `2048` | Output size |
| `--supersampling N` | `2` | 1, 2, 4 or 8 |
| `--match-by-name` | off | Pair `_low` / `_high` objects by name |

Each texture set (material) of MESH produces `<set>_<map>.png` in the output folder.

## Environment variables

- `PATINA_SOFTWARE_GPU=1` forces a software adapter (lavapipe, WARP).
- `WGPU_BACKEND` (`vulkan`, `dx12`, `metal`, `gl`) picks the graphics API.
- `RUST_LOG=info` prints more logging.
