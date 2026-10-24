# Rendering

## Viewport pipeline

```
background (env) ─┐
mesh (PBR)        ├─► Rgba16F, 4× MSAA ─resolve─► Rgba16F ─tonemap─► Rgba8 sRGB ─► egui
wireframe (lines) ─┘          + Depth32F
```

- Shaders live in `crates/patina_render/src/shaders/`. `common.wgsl` (uniform
  layouts, environment functions) is prepended to `scene.wgsl` and `uv.wgsl`.
- Channel views bypass tonemapping: they write display values directly so the
  numbers you see match the stored data.

## Analytic studio environments

Until HDRI lighting (M2), environments are a sky gradient plus up to four
spherical-Gaussian light lobes. Blurring a spherical Gaussian with another has
a closed form, so:

- specular reflections use the lobe convolved with a GGX-like kernel of
  sharpness `2 / α²`,
- diffuse lighting uses a kernel of sharpness 2 (a clamped-cosine fit),
- the background uses the blur slider.

Specular uses Karis' analytic split-sum BRDF approximation, so no lookup
texture or prefiltering pass is needed.

## Textures

Bitmaps are uploaded as RGBA8 with a CPU-generated mip chain. sRGB channels
are averaged in linear space. Levels above the device limit (8192) are dropped.

## UV view

The UV view renders a fullscreen pass that inverts the view transform to look
up the channel texture, then draws the mesh's unique edges as lines with the
vertex shader placing each vertex at its UV.
