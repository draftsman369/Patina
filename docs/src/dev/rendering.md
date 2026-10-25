# Rendering

## Viewport pipeline

```
background (env) ─┐
mesh (PBR)        ├─► Rgba16F, 4× MSAA ─resolve─► Rgba16F ─tonemap─► Rgba8 sRGB ─► egui
wireframe (lines) ─┘          + Depth32F
```

- Shaders live in `crates/patina_render/src/shaders/`. `common.wgsl` (view
  modes, material bindings) is prepended to `scene.wgsl` and `uv.wgsl`.
- Channel and mesh-map views bypass tonemapping: they write display values
  directly so the numbers you see match the stored data.

Bind groups of the scene shader:

| Group | Content |
|---|---|
| 0 | Frame uniforms (camera, environment rotation, view mode) |
| 1 | Material: factors, 5 channel textures, sampler, displayed mesh map |
| 2 | Environment: source cube, prefiltered specular cube, BRDF LUT, sampler, SH9 |

## Image-based lighting

`environment.rs` and `shaders/ibl.wgsl` turn an `EnvSource` into GPU lighting
data with compute shaders:

1. **Source cube**: an HDRI is converted from equirectangular (2×2
   supersampled, bilinear); an analytic preset (sky gradient + spherical
   Gaussian lights) is evaluated directly. Size: 512 for presets, ~¼ of the
   HDRI width (up to 1024) for images.
2. **Mips** by 2×2 box filtering.
3. **Specular**: a 256² cube with 6 mips, mip *i* prefiltered with GGX
   importance sampling for roughness *i*/5 (filtered importance sampling reads
   lower source mips to avoid fireflies).
4. **Diffuse**: 9 spherical-harmonics coefficients projected from the 32²
   mip, convolved with a clamped cosine in the shader.
5. **BRDF LUT**: a 128² split-sum table, computed once per renderer.

The renderer rebuilds the environment only when `ViewportSettings::environment`
changes (presets compare by index, HDRIs by identity).

## Tangent space

Tangents are exact MikkTSpace, computed per corner and welded, in v-up UV
space so the bitangent points up the image: normal maps are green-up
("OpenGL"). The project's *DirectX* setting flips green when shading and baking.

## Textures

Bitmaps are uploaded as RGBA8 with a CPU-generated mip chain. sRGB channels
are averaged in linear space. Levels above the device limit (8192) are dropped.

## UV view

The UV view renders a fullscreen pass that inverts the view transform to look
up the channel texture (or mesh map), then draws the mesh's unique edges as
lines with the vertex shader placing each vertex at its UV.
