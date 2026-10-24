// Image-based lighting precomputation. Each pass is compiled as its own
// module: the renderer prepends IBL_COMMON and picks the section it needs
// (sections are separated by "//--- name" markers).

//--- common
const PI: f32 = 3.14159265;

// Direction through texel `uv` (0–1) of cube face `face`, following the
// D3D/Vulkan/Metal cubemap convention used by wgpu.
fn cube_dir(face: u32, uv: vec2<f32>) -> vec3<f32> {
    let p = uv * 2.0 - 1.0;
    switch face {
        case 0u: { return normalize(vec3(1.0, -p.y, -p.x)); }
        case 1u: { return normalize(vec3(-1.0, -p.y, p.x)); }
        case 2u: { return normalize(vec3(p.x, 1.0, p.y)); }
        case 3u: { return normalize(vec3(p.x, -1.0, -p.y)); }
        case 4u: { return normalize(vec3(p.x, -p.y, 1.0)); }
        default: { return normalize(vec3(-p.x, -p.y, -1.0)); }
    }
}

fn radical_inverse(bits_in: u32) -> f32 {
    var bits = bits_in;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2(f32(i) / f32(n), radical_inverse(i));
}

// GGX half vector around +Z for random numbers `xi`, alpha = roughness².
fn ggx_half_vector(xi: vec2<f32>, alpha: f32) -> vec3<f32> {
    let phi = 2.0 * PI * xi.x;
    let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (alpha * alpha - 1.0) * xi.y));
    let sin_theta = sqrt(1.0 - cos_theta * cos_theta);
    return vec3(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);
}

fn to_world(h: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let up = select(vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return t * h.x + b * h.y + n * h.z;
}

//--- equirect
// HDRI (equirectangular, rgba32float) → cubemap face texels.
@group(0) @binding(0) var equirect: texture_2d<f32>;
@group(0) @binding(1) var out_face: texture_storage_2d_array<rgba16float, write>;

fn load_wrapped(p: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    let x = ((p.x % size.x) + size.x) % size.x;
    let y = clamp(p.y, 0, size.y - 1);
    return textureLoad(equirect, vec2(x, y), 0);
}

@compute @workgroup_size(8, 8, 1)
fn equirect_to_cube(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(out_face).x;
    if id.x >= size || id.y >= size {
        return;
    }
    let src = vec2<i32>(textureDimensions(equirect));
    // 2x2 supersampling per texel smooths minification.
    var acc = vec3(0.0);
    for (var s = 0u; s < 4u; s++) {
        let offset = vec2(f32(s & 1u), f32(s >> 1u)) * 0.5 + 0.25;
        let d = cube_dir(id.z, (vec2<f32>(id.xy) + offset) / f32(size));
        let u = atan2(d.x, -d.z) / (2.0 * PI) + 0.5;
        let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
        let p = vec2(u, v) * vec2<f32>(src) - 0.5;
        let i = vec2<i32>(floor(p));
        let f = p - floor(p);
        let c00 = load_wrapped(i, src).rgb;
        let c10 = load_wrapped(i + vec2(1, 0), src).rgb;
        let c01 = load_wrapped(i + vec2(0, 1), src).rgb;
        let c11 = load_wrapped(i + vec2(1, 1), src).rgb;
        acc += mix(mix(c00, c10, f.x), mix(c01, c11, f.x), f.y);
    }
    // Clamp extreme values (e.g. the sun) so f16 stays finite.
    textureStore(out_face, id.xy, id.z, vec4(min(acc * 0.25, vec3(60000.0)), 1.0));
}

//--- analytic
// Analytic studio preset → cubemap: sky gradient plus spherical-Gaussian lights.
struct Light {
    dir: vec4<f32>,
    color: vec4<f32>,
}
struct Analytic {
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    sky_ground: vec4<f32>,
    lights: array<Light, 4>,
    // x = light count.
    count: vec4<u32>,
}
@group(0) @binding(0) var<uniform> env: Analytic;
@group(0) @binding(1) var out_face: texture_storage_2d_array<rgba16float, write>;

fn analytic_radiance(d: vec3<f32>) -> vec3<f32> {
    let up = smoothstep(-0.05, 1.0, d.y);
    let down = 1.0 - smoothstep(-0.6, 0.05, d.y);
    var c = mix(env.sky_horizon.rgb, env.sky_top.rgb, up);
    c = mix(c, env.sky_ground.rgb, down);
    for (var i = 0u; i < min(env.count.x, 4u); i++) {
        let l = env.lights[i];
        c += l.color.rgb * exp(l.dir.w * (dot(d, l.dir.xyz) - 1.0));
    }
    return c;
}

@compute @workgroup_size(8, 8, 1)
fn analytic_to_cube(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(out_face).x;
    if id.x >= size || id.y >= size {
        return;
    }
    var acc = vec3(0.0);
    for (var s = 0u; s < 4u; s++) {
        let offset = vec2(f32(s & 1u), f32(s >> 1u)) * 0.5 + 0.25;
        acc += analytic_radiance(cube_dir(id.z, (vec2<f32>(id.xy) + offset) / f32(size)));
    }
    textureStore(out_face, id.xy, id.z, vec4(acc * 0.25, 1.0));
}

//--- downsample
// Box-filters one cube mip into the next.
@group(0) @binding(0) var src_mip: texture_2d_array<f32>;
@group(0) @binding(1) var out_face: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(out_face).x;
    if id.x >= size || id.y >= size {
        return;
    }
    let p = vec2<i32>(id.xy) * 2;
    let f = i32(id.z);
    let c = textureLoad(src_mip, p, f, 0) + textureLoad(src_mip, p + vec2(1, 0), f, 0)
        + textureLoad(src_mip, p + vec2(0, 1), f, 0) + textureLoad(src_mip, p + vec2(1, 1), f, 0);
    textureStore(out_face, id.xy, id.z, c * 0.25);
}

//--- prefilter
// GGX-prefiltered specular cube (filtered importance sampling).
struct Prefilter {
    // x = roughness, y = source face size, z = sample count.
    params: vec4<f32>,
}
@group(0) @binding(0) var src_cube: texture_cube<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var out_face: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(3) var<uniform> pf: Prefilter;

@compute @workgroup_size(8, 8, 1)
fn prefilter(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(out_face).x;
    if id.x >= size || id.y >= size {
        return;
    }
    let n = cube_dir(id.z, (vec2<f32>(id.xy) + 0.5) / f32(size));
    let roughness = pf.params.x;
    if roughness <= 0.0 {
        textureStore(out_face, id.xy, id.z, textureSampleLevel(src_cube, src_sampler, n, 0.0));
        return;
    }
    let alpha = roughness * roughness;
    let count = u32(pf.params.z);
    let texel_solid_angle = 4.0 * PI / (6.0 * pf.params.y * pf.params.y);
    var acc = vec3(0.0);
    var weight = 0.0;
    for (var i = 0u; i < count; i++) {
        let h = to_world(ggx_half_vector(hammersley(i, count), alpha), n);
        let l = 2.0 * dot(n, h) * h - n;
        let n_dot_l = dot(n, l);
        if n_dot_l > 0.0 {
            // With V = N the GGX pdf simplifies to D / 4.
            let n_dot_h = max(dot(n, h), 0.0);
            let a2 = alpha * alpha;
            let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
            let d = a2 / (PI * denom * denom);
            let pdf = d * 0.25 + 1e-6;
            let sample_solid_angle = 1.0 / (f32(count) * pdf);
            let lod = max(0.5 * log2(sample_solid_angle / texel_solid_angle) + 1.0, 0.0);
            acc += textureSampleLevel(src_cube, src_sampler, l, lod).rgb * n_dot_l;
            weight += n_dot_l;
        }
    }
    textureStore(out_face, id.xy, id.z, vec4(acc / max(weight, 1e-6), 1.0));
}

//--- sh
// Projects the environment onto 9 spherical harmonics coefficients.
@group(0) @binding(0) var sh_src: texture_2d_array<f32>;
@group(0) @binding(1) var<storage, read_write> partials: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> sh_out: array<vec4<f32>, 9>;

fn sh_basis(d: vec3<f32>) -> array<f32, 9> {
    return array<f32, 9>(
        0.282095,
        0.488603 * d.y,
        0.488603 * d.z,
        0.488603 * d.x,
        1.092548 * d.x * d.y,
        1.092548 * d.y * d.z,
        0.315392 * (3.0 * d.z * d.z - 1.0),
        1.092548 * d.x * d.z,
        0.546274 * (d.x * d.x - d.y * d.y),
    );
}

// 256 threads, each accumulating a strided subset of all cube texels.
@compute @workgroup_size(256, 1, 1)
fn sh_partial(@builtin(local_invocation_index) t: u32) {
    let size = textureDimensions(sh_src).x;
    let total = size * size * 6u;
    var acc: array<vec3<f32>, 9>;
    for (var k = 0u; k < 9u; k++) {
        acc[k] = vec3(0.0);
    }
    for (var i = t; i < total; i += 256u) {
        let face = i / (size * size);
        let x = i % size;
        let y = (i / size) % size;
        let uv = (vec2(f32(x), f32(y)) + 0.5) / f32(size);
        let d = cube_dir(face, uv);
        let p = uv * 2.0 - 1.0;
        let texel = 2.0 / f32(size);
        let solid_angle = texel * texel / pow(1.0 + dot(p, p), 1.5);
        let c = textureLoad(sh_src, vec2(i32(x), i32(y)), i32(face), 0).rgb * solid_angle;
        let b = sh_basis(d);
        for (var k = 0u; k < 9u; k++) {
            acc[k] += c * b[k];
        }
    }
    for (var k = 0u; k < 9u; k++) {
        partials[t * 9u + k] = vec4(acc[k], 0.0);
    }
}

@compute @workgroup_size(9, 1, 1)
fn sh_sum(@builtin(local_invocation_index) k: u32) {
    var s = vec3(0.0);
    for (var t = 0u; t < 256u; t++) {
        s += partials[t * 9u + k].rgb;
    }
    sh_out[k] = vec4(s, 0.0);
}

//--- brdf
// Split-sum environment BRDF lookup table: x = N·V, y = roughness → (scale, bias).
@group(0) @binding(0) var lut: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn brdf_lut(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(lut);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let n_dot_v = max((f32(id.x) + 0.5) / f32(size.x), 1e-3);
    let roughness = (f32(id.y) + 0.5) / f32(size.y);
    let alpha = roughness * roughness;
    let v = vec3(sqrt(1.0 - n_dot_v * n_dot_v), 0.0, n_dot_v);
    let k = alpha * 0.5;
    var a = 0.0;
    var b = 0.0;
    let count = 512u;
    for (var i = 0u; i < count; i++) {
        let h = ggx_half_vector(hammersley(i, count), alpha);
        let l = 2.0 * dot(v, h) * h - v;
        let n_dot_l = max(l.z, 0.0);
        let n_dot_h = max(h.z, 0.0);
        let v_dot_h = max(dot(v, h), 0.0);
        if n_dot_l > 0.0 {
            let g_v = n_dot_v / (n_dot_v * (1.0 - k) + k);
            let g_l = n_dot_l / (n_dot_l * (1.0 - k) + k);
            let g_vis = g_v * g_l * v_dot_h / (n_dot_h * n_dot_v);
            let fc = pow(1.0 - v_dot_h, 5.0);
            a += (1.0 - fc) * g_vis;
            b += fc * g_vis;
        }
    }
    textureStore(lut, id.xy, vec4(a / f32(count), b / f32(count), 0.0, 1.0));
}
