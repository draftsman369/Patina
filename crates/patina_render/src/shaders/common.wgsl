// Shared declarations for the viewport shaders.
// This file is concatenated in front of scene.wgsl and uv.wgsl by the renderer.

const PI: f32 = 3.14159265;

// View modes. Keep in sync with `ViewMode` in settings.rs.
const VIEW_MATERIAL: u32 = 0u;
const VIEW_BASE_COLOR: u32 = 1u;
const VIEW_METALLIC: u32 = 2u;
const VIEW_ROUGHNESS: u32 = 3u;
const VIEW_NORMAL: u32 = 4u;
const VIEW_WORLD_NORMAL: u32 = 5u;
const VIEW_UV_CHECKER: u32 = 6u;
const VIEW_EMISSIVE: u32 = 7u;
const VIEW_MESH_MAP: u32 = 8u;

struct Material {
    base_color: vec4<f32>,
    // rgb = emissive factor.
    emissive: vec4<f32>,
    // x = metallic, y = roughness, z = normal map green sign, w = 1 if the mesh has UVs.
    params: vec4<f32>,
}

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var t_base_color: texture_2d<f32>;
@group(1) @binding(2) var t_metallic: texture_2d<f32>;
@group(1) @binding(3) var t_roughness: texture_2d<f32>;
@group(1) @binding(4) var t_normal: texture_2d<f32>;
@group(1) @binding(5) var t_emissive: texture_2d<f32>;
@group(1) @binding(6) var s_material: sampler;
// The mesh map selected for display (B shortcut), or a neutral 1x1 texture.
@group(1) @binding(7) var t_mesh_map: texture_2d<f32>;

// Converts a data value so it reads back unchanged after the sRGB output encode.
fn display_data(v: vec3<f32>) -> vec3<f32> {
    return pow(clamp(v, vec3(0.0), vec3(1.0)), vec3(2.2));
}
