# Texture sets and channels

Every material of the mesh becomes a **texture set**. Select one in the
Texture Set List; the Properties panel then shows:

- **Name** and **resolution** (128 to 8192; used by painting and export later).
- **Factors**: base color, metallic, roughness, emissive. With no bitmap, the
  channel is uniform; with a bitmap, the factor multiplies it.
- **Channel bitmaps**: load an image (PNG, JPEG, TGA, BMP, TIFF, HDR) into
  base color, metallic, roughness, normal or emissive, or clear it.
- **Normal map format**: OpenGL (Y+) or DirectX (Y−), for the whole project.

Dropping an image on the window loads it into the selected texture set, in
the channel shown by the current view mode (base color in material view).

## What gets imported

| Source | Imported |
|---|---|
| glTF / GLB | Base color, metallic-roughness (split into two channels), normal and emissive textures and factors; node transforms |
| OBJ + MTL | Diffuse color and texture (`map_Kd`), normal texture (`norm`), shininess → roughness, emissive color |

Missing normals are generated (smooth, area-weighted) and tangents are always
recomputed with MikkTSpace.

## Color handling

Base color and emissive bitmaps are treated as sRGB; metallic, roughness and
normal bitmaps as linear data. Rendering happens in linear HDR and is
tonemapped for display.
