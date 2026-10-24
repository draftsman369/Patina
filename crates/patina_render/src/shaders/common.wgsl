// Shared declarations for the viewport shaders.
// This file is concatenated in front of scene.wgsl and uv.wgsl by the renderer.

const PI: f32 = 3.14159265;
const MAX_LIGHTS: u32 = 4u;

// View modes. Keep in sync with `ViewMode` in settings.rs.
const VIEW_MATERIAL: u32 = 0u;
const VIEW_BASE_COLOR: u32 = 1u;
const VIEW_METALLIC: u32 = 2u;
const VIEW_ROUGHNESS: u32 = 3u;
const VIEW_NORMAL: u32 = 4u;
const VIEW_WORLD_NORMAL: u32 = 5u;
const VIEW_UV_CHECKER: u32 = 6u;
const VIEW_EMISSIVE: u32 = 7u;

struct Light {
    // xyz = direction the light comes from (unit), w = lobe sharpness.
    dir: vec4<f32>,
    // rgb = radiance, a unused.
    color: vec4<f32>,
}

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz = camera position, w = 1 for orthographic.
    camera_pos: vec4<f32>,
    // x = cos(rotation), y = sin(rotation), z = background sharpness, w = background opacity.
    env: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_ground: vec4<f32>,
    lights: array<Light, 4>,
    // x = view mode, y = light count.
    mode: vec4<u32>,
    // rgb = flat background color (HDR), a unused.
    background: vec4<f32>,
    wire_color: vec4<f32>,
}

struct Material {
    base_color: vec4<f32>,
    // rgb = emissive factor.
    emissive: vec4<f32>,
    // x = metallic, y = roughness, z = normal map green sign, w = 1 if the mesh has UVs.
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var t_base_color: texture_2d<f32>;
@group(1) @binding(2) var t_metallic: texture_2d<f32>;
@group(1) @binding(3) var t_roughness: texture_2d<f32>;
@group(1) @binding(4) var t_normal: texture_2d<f32>;
@group(1) @binding(5) var t_emissive: texture_2d<f32>;
@group(1) @binding(6) var s_material: sampler;

// Converts a data value so it reads back unchanged after the sRGB output encode.
fn display_data(v: vec3<f32>) -> vec3<f32> {
    return pow(clamp(v, vec3(0.0), vec3(1.0)), vec3(2.2));
}

// ---------------------------------------------------------------------------
// Analytic studio environment.
//
// A sky gradient plus a few spherical-Gaussian "softbox" lobes. Blurring by a
// kernel of sharpness `k` has a closed form for spherical Gaussians, which
// gives us roughness-dependent reflections and diffuse lighting with no
// prefiltering pass. HDRI image lighting replaces this in M2.
// ---------------------------------------------------------------------------

fn rotate_env(d: vec3<f32>) -> vec3<f32> {
    let c = frame.env.x;
    let s = frame.env.y;
    return vec3(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

fn sky(d: vec3<f32>, k: f32) -> vec3<f32> {
    // Angular blur width grows as the kernel gets softer.
    let w = clamp(2.0 / sqrt(k + 1e-4), 0.05, 4.0);
    let up = smoothstep(-0.1 * w, 1.0 + w, d.y);
    let down = 1.0 - smoothstep(-0.6 - w, 0.1 * w, d.y);
    var c = mix(frame.sky_horizon.rgb, frame.sky_top.rgb, up);
    c = mix(c, frame.sky_ground.rgb, down);
    let avg = (frame.sky_top.rgb + 2.0 * frame.sky_horizon.rgb + frame.sky_ground.rgb) * 0.25;
    let m = clamp(w / 4.0, 0.0, 1.0);
    return mix(c, avg, m * m);
}

// Environment radiance in world direction `dir`, blurred by kernel sharpness `k`.
fn env_radiance(dir: vec3<f32>, k: f32) -> vec3<f32> {
    let d = rotate_env(normalize(dir));
    var c = sky(d, k);
    for (var i = 0u; i < min(frame.mode.y, MAX_LIGHTS); i++) {
        let l = frame.lights[i];
        let lambda = l.dir.w;
        // Convolution of two spherical Gaussians, energy preserving.
        let lb = lambda * k / (lambda + k);
        let amp = lb / lambda;
        c += l.color.rgb * amp * exp(lb * (dot(d, l.dir.xyz) - 1.0));
    }
    return c;
}
