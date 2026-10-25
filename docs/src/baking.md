# Baking mesh maps

Mesh maps capture the shape of a mesh in textures: surface detail from a
high-poly model, occlusion, curvature and so on. Later milestones use them to
drive generators and smart materials; today you can bake them, view them and
import maps baked elsewhere.

Open the baker with **File › Bake mesh maps…** or the **Bake…** button in the
Properties panel.

## Common settings

| Setting | Meaning |
|---|---|
| Output size | Map resolution, 128 to 8192 |
| Antialiasing | Supersampling: 2×2, 4×4 or 8×8 rays per pixel |
| Dilation width | Pixels of padding around UV islands |
| High poly mesh | Source of detail. Without one, the low poly bakes onto itself (useful for AO, curvature, thickness) |
| Cage | Optional mesh with exactly the same triangles as the low poly; rays start on the cage |
| Max frontal / rear distance | How far rays start in front of and end behind the low-poly surface, relative to the mesh's bounding radius |
| Average normals | Cast rays along smoothed normals so hard edges don't leave gaps |
| Match | *Always*, or *By mesh name*: objects named `Bolt_low` and `Bolt_high` only see each other |

Select which texture sets to bake, then which maps. Baking runs in the
background; the progress bar shows each texture set and **Cancel** stops it.

## Maps

| Map | Content | Options |
|---|---|---|
| Normal | High-poly normals in the low poly's tangent space (MikkTSpace, project normal format) | — |
| World Space Normal | High-poly normals in world space | — |
| ID | Flat colors per vertex color, material or mesh | Color source |
| Ambient Occlusion | How much of the hemisphere is open | Samples, distance, ignore backfaces, only same mesh name (with Match by name) |
| Curvature | Convex edges bright, cavities dark, from the high-poly mesh | Strength |
| Position | Position normalized to the high poly's bounding box | — |
| Thickness | Distance through the mesh; thin areas dark | Samples, distance |
| Height | Distance between high and low surfaces; mid-gray = no difference | — |
| Opacity | White where the high poly was found | — |
| Bent Normal | Average unoccluded direction, tangent space | Uses the AO options |

Results land in the texture set's mesh-map slots and the viewport switches to
the mesh-map view. Press **B** to cycle through maps; **M** returns to the material.

UV triangles outside the 0–1 tile are wrapped into it. Multi-tile (UDIM)
baking comes in M10.

## Importing maps

Each slot in **Properties › Mesh maps** has an **Import…** button. Dropping an
image on the window while a mesh map is displayed loads it into that slot.

## Command line

```sh
patina low.fbx --bake out/ --high high.fbx --maps normal,ao,curvature --bake-size 4096
```

See [Command line](cli.md) for all options. Maps are written as
`<texture set>_<map>.png`.

## How it works

The baker runs on the GPU. Each texture set is rasterized into UV space in
512×512 tiles (positions, normals, tangents and ray origins per texel). Compute
shaders then trace rays against a BVH of the high-poly mesh: one pass for the
direct maps, then batches of AO and thickness samples, then a resolve pass for
supersampling. Short GPU submissions keep the editor responsive and avoid
driver timeouts. Dilation runs on the CPU.
