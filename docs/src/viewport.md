# The viewport

## Navigation

| Action | 3D view | 2D view |
|---|---|---|
| Orbit | Alt + left drag, or right drag | — |
| Pan | Middle drag | Middle or right drag, Alt + left drag |
| Zoom | Wheel, or Alt + right drag | Wheel (zooms at the cursor) |
| Rotate environment | Shift + right drag | — |
| Frame | F | F |

**Viewport › Camera presets** snaps to front, back, left, right, top, bottom
or a three-quarter view. **Orthographic** and the field of view are in
Display Settings.

## Layouts

| Key | Layout |
|---|---|
| F1 | 3D and 2D side by side |
| F2 | 3D only |
| F3 | 2D only |

The 2D view shows the UV layout of the selected texture set over the channel
matching the current view mode. The 0–1 tile is drawn at full brightness,
repeats outside it are dimmed.

## View modes

**Material** shows the fully lit result. The other modes show one channel
unlit: base color, metallic, roughness, normal (the tangent-space map as
stored), world normal (the final shading normal), UV checker and emissive.
**Mesh Map** shows a baked or imported mesh map (see [Baking](baking.md)).

| Key | View |
|---|---|
| C | Cycle channel views |
| B | Show mesh maps; press again to cycle them |
| M | Back to the material view |

The **Viewport › Mesh map** menu picks a map directly.

## Lighting and display

Display Settings controls:

- **Environment**: one of five analytic studio setups (Studio Soft, Studio
  Contrast, Overcast, Sunset, Night Rim) or an **HDRI** (`.hdr` or `.exr`).
  Load HDRIs with **File › Load HDRI…**, the shelf's Environments tab, or by
  dropping the file on the window. Loaded HDRIs stay in the list for the session.
- **Rotation**, **exposure** (in stops), **background blur** and **background opacity**.
- **Tonemapping**: ACES (default), Filmic or Linear.
- **Wireframe** overlay.
