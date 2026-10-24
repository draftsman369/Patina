# Command line

```sh
patina [MESH]                       # open the editor, optionally with a mesh
patina [MESH] --snapshot out.png    # render one frame headless and exit
```

Snapshot options:

| Option | Default | Meaning |
|---|---|---|
| `--primitive NAME` | `sphere` | Primitive used when no mesh is given: sphere, cube, cylinder, torus, plane |
| `--view MODE` | `material` | material, base-color, metallic, roughness, normal, world-normal, uv-checker, emissive |
| `--env N` | `0` | Environment preset index (0–4) |
| `--exposure EV` | `0` | Exposure in stops |
| `--size WxH` | `1280x720` | Output size, up to 8192×8192 |
| `--wireframe` | off | Draw the wireframe overlay |

Environment variables:

- `PATINA_SOFTWARE_GPU=1` forces a software adapter (lavapipe, WARP).
- `WGPU_BACKEND` (`vulkan`, `dx12`, `metal`, `gl`) picks the graphics API.
- `RUST_LOG=info` prints more logging.
