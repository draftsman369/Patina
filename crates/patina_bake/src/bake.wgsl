// GPU mesh-map baking. Sections are separated by "//--- name" markers; the
// baker builds one module per pass from "math" plus the sections it needs.

//--- math
const PI: f32 = 3.14159265;

// Map slots, matching patina_core::MeshMap::index().
const MAP_NORMAL: u32 = 0u;
const MAP_WORLD_NORMAL: u32 = 1u;
const MAP_ID: u32 = 2u;
const MAP_AO: u32 = 3u;
const MAP_CURVATURE: u32 = 4u;
const MAP_POSITION: u32 = 5u;
const MAP_THICKNESS: u32 = 6u;
const MAP_HEIGHT: u32 = 7u;
const MAP_OPACITY: u32 = 8u;
const MAP_BENT_NORMAL: u32 = 9u;
const MAP_COUNT: u32 = 10u;

const FLAG_MATCH_BY_NAME: u32 = 1u;
const FLAG_AO_IGNORE_BACKFACES: u32 = 2u;
const FLAG_DIRECTX: u32 = 4u;
const FLAG_AO_SAME_NAME: u32 = 8u;
// ID source: 0 = vertex color, 1 = material, 2 = object (bits 8..15).

struct Params {
    // Tile origin and size, in supersampled texels.
    tile_origin: vec2<u32>,
    tile_size: vec2<u32>,
    // x = supersampled image size, y = supersampling factor, z = requested map mask, w = flags.
    config: vec4<u32>,
    // x = AO samples, y = first sample of this batch, z = samples in this batch, w = thickness samples.
    samples: vec4<u32>,
    // x = AO distance, y = thickness distance, z = height range, w = curvature scale.
    distances: vec4<f32>,
    bounds_min: vec4<f32>,
    // w = ray epsilon.
    bounds_max: vec4<f32>,
}

fn has_map(p: Params, slot: u32) -> bool {
    return (p.config.z & (1u << slot)) != 0u;
}

fn pcg(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand2(seed: u32, i: u32) -> vec2<f32> {
    let a = pcg(seed ^ pcg(i * 2u + 1u));
    let b = pcg(a ^ 0x9e3779b9u);
    return vec2(f32(a), f32(b)) * 2.3283064365386963e-10;
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let up = select(vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let t = normalize(cross(up, n));
    return mat3x3<f32>(t, cross(n, t), n);
}

// Cosine-weighted direction around `n`.
fn cosine_dir(n: vec3<f32>, r: vec2<f32>) -> vec3<f32> {
    let phi = 2.0 * PI * r.x;
    let s = sqrt(r.y);
    let local = vec3(cos(phi) * s, sin(phi) * s, sqrt(max(1.0 - r.y, 0.0)));
    return basis(n) * local;
}

// Tangent-space encoding of a world vector against a (normal, tangent) frame.
fn to_tangent(v: vec3<f32>, n: vec3<f32>, tangent: vec4<f32>, directx: bool) -> vec3<f32> {
    var t = tangent.xyz - n * dot(n, tangent.xyz);
    if dot(t, t) < 1e-12 {
        t = basis(n)[0];
    }
    t = normalize(t);
    let b = cross(n, t) * select(1.0, -1.0, tangent.w < 0.0);
    var ts = vec3(dot(v, t), dot(v, b), dot(v, n));
    if directx {
        ts.y = -ts.y;
    }
    return normalize(ts) * 0.5 + 0.5;
}

fn hue_color(id: u32) -> vec3<f32> {
    // Golden-ratio hues give well-separated colors for consecutive ids.
    let h = fract(f32(id) * 0.6180339887 + 0.13);
    let k = vec3(0.0, 2.0 / 3.0, 1.0 / 3.0);
    let rgb = clamp(abs(fract(vec3(h) + k) * 6.0 - 3.0) - 1.0, vec3(0.0), vec3(1.0));
    return mix(vec3(1.0), rgb, 0.85) * 0.9;
}

//--- bvh
struct Node {
    bmin: vec3<f32>,
    first: u32,
    bmax: vec3<f32>,
    count: u32,
}

@group(0) @binding(0) var<storage, read> nodes: array<Node>;
// xyz = position, w = curvature.
@group(0) @binding(1) var<storage, read> positions: array<vec4<f32>>;
// Triangle indices, reordered so BVH leaves address triangles directly.
@group(0) @binding(2) var<storage, read> indices: array<u32>;
// Per triangle (BVH order): x = match group, y = id (material or object).
@group(0) @binding(3) var<storage, read> tri_info: array<vec2<u32>>;

struct Hit {
    t: f32,
    tri: u32,
    uv: vec2<f32>,
    hit: bool,
}

fn safe_inv(d: vec3<f32>) -> vec3<f32> {
    let s = select(sign(d), vec3(1.0), d == vec3(0.0));
    return select(1.0 / d, s * 1e30, abs(d) < vec3(1e-20));
}

fn box_entry(n: Node, o: vec3<f32>, inv: vec3<f32>) -> f32 {
    let t0 = (n.bmin - o) * inv;
    let t1 = (n.bmax - o) * inv;
    let tmin = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), max(min(t0.z, t1.z), 0.0));
    let tmax = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z));
    return select(1e30, tmin, tmin <= tmax);
}

// Möller–Trumbore. Returns (t, u, v, det); t < 0 on a miss. det < 0 for back-face hits.
fn tri_hit(tri: u32, o: vec3<f32>, d: vec3<f32>) -> vec4<f32> {
    let a = positions[indices[tri * 3u]].xyz;
    let b = positions[indices[tri * 3u + 1u]].xyz;
    let c = positions[indices[tri * 3u + 2u]].xyz;
    let e1 = b - a;
    let e2 = c - a;
    let p = cross(d, e2);
    let det = dot(e1, p);
    if abs(det) < 1e-14 {
        return vec4(-1.0);
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = dot(s, p) * inv;
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if u < 0.0 || v < 0.0 || u + v > 1.0 {
        return vec4(-1.0);
    }
    return vec4(dot(e2, q) * inv, u, v, det);
}

// Closest (or any, when `any_hit`) hit in (t_min, t_max), optionally limited to one match group.
fn trace(o: vec3<f32>, d: vec3<f32>, t_min: f32, t_max: f32, group: u32, use_group: bool, any_hit: bool, skip_back: bool) -> Hit {
    var result: Hit;
    result.hit = false;
    result.t = t_max;
    let inv = safe_inv(d);
    var stack: array<u32, 48>;
    var sp = 1u;
    stack[0] = 0u;
    loop {
        if sp == 0u {
            break;
        }
        sp -= 1u;
        let node = nodes[stack[sp]];
        if box_entry(node, o, inv) > result.t {
            continue;
        }
        if node.count > 0u {
            for (var i = node.first; i < node.first + node.count; i++) {
                if use_group && tri_info[i].x != group {
                    continue;
                }
                let h = tri_hit(i, o, d);
                if h.x > t_min && h.x < result.t {
                    if skip_back && h.w < 0.0 {
                        continue;
                    }
                    result.t = h.x;
                    result.tri = i;
                    result.uv = h.yz;
                    result.hit = true;
                    if any_hit {
                        return result;
                    }
                }
            }
        } else if sp + 2u <= 48u {
            stack[sp] = node.first;
            stack[sp + 1u] = node.first + 1u;
            sp += 2u;
        }
    }
    return result;
}

fn bary(h: Hit) -> vec3<f32> {
    return vec3(1.0 - h.uv.x - h.uv.y, h.uv.x, h.uv.y);
}

//--- raster
// Rasterizes the low-poly texture set into UV space, one tile at a time.
struct Corner {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
    // xyz = ray direction normal (averaged or shading), w = match group.
    @location(4) ray: vec4<f32>,
    // xyz = cage position, w = 1 when a cage is used.
    @location(5) cage: vec4<f32>,
}

struct RasterOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) ray: vec4<f32>,
    @location(4) cage: vec4<f32>,
}

struct RasterParams {
    // xy = tile origin, zw = tile size (supersampled texels).
    tile: vec4<f32>,
    // x = image size, y = front distance, z = rear distance.
    config: vec4<f32>,
}
@group(0) @binding(0) var<uniform> rp: RasterParams;

@vertex
fn vs_raster(c: Corner) -> RasterOut {
    let px = c.uv * rp.config.x - rp.tile.xy;
    let ndc = vec2(px.x / rp.tile.z * 2.0 - 1.0, 1.0 - px.y / rp.tile.w * 2.0);
    var out: RasterOut;
    out.clip = vec4(ndc, 0.5, 1.0);
    out.position = c.position;
    out.normal = c.normal;
    out.tangent = c.tangent;
    out.ray = c.ray;
    out.cage = c.cage;
    return out;
}

// Two passes of two targets each keep within the 32 bytes-per-sample limit.
struct GeometryTargets {
    // xyz = position, w = match group + 1 (0 = empty texel).
    @location(0) position: vec4<f32>,
    // xyz = ray origin, w = maximum ray distance.
    @location(1) ray: vec4<f32>,
}

struct FrameTargets {
    @location(0) normal: vec4<f32>,
    @location(1) tangent: vec4<f32>,
}

@fragment
fn fs_frame(in: RasterOut) -> FrameTargets {
    var out: FrameTargets;
    out.normal = vec4(normalize(in.normal), 1.0);
    out.tangent = in.tangent;
    return out;
}

@fragment
fn fs_geometry(in: RasterOut) -> GeometryTargets {
    var out: GeometryTargets;
    out.position = vec4(in.position, in.ray.w + 1.0);
    let front = rp.config.y;
    let rear = rp.config.z;
    if in.cage.w > 0.5 {
        out.ray = vec4(in.cage.xyz, length(in.position - in.cage.xyz) + rear);
    } else {
        out.ray = vec4(in.position + normalize(in.ray.xyz) * front, front + rear);
    }
    return out;
}

//--- primary
// Casts the main ray per texel and writes the direct maps.
@group(0) @binding(4) var<storage, read> normals: array<vec4<f32>>;
@group(0) @binding(5) var rt_position: texture_2d<f32>;
@group(0) @binding(6) var rt_normal: texture_2d<f32>;
@group(0) @binding(7) var rt_tangent: texture_2d<f32>;
@group(0) @binding(8) var rt_ray: texture_2d<f32>;
@group(0) @binding(9) var<uniform> params: Params;
@group(0) @binding(10) var<storage, read_write> maps: array<vec4<f32>>;
// Two entries per texel: (secondary ray origin, valid), (secondary normal, match group).
@group(0) @binding(11) var<storage, read_write> surface: array<vec4<f32>>;

fn unpack_color(w: f32) -> vec3<f32> {
    return unpack4x8unorm(bitcast<u32>(w)).rgb;
}

@compute @workgroup_size(8, 8, 1)
fn primary(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params.tile_size;
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let texel = id.y * size.x + id.x;
    let n_texels = size.x * size.y;
    let p = textureLoad(rt_position, id.xy, 0);
    if p.w < 0.5 {
        for (var s = 0u; s < MAP_COUNT; s++) {
            maps[s * n_texels + texel] = vec4(0.0);
        }
        surface[texel * 2u] = vec4(0.0);
        surface[texel * 2u + 1u] = vec4(0.0);
        return;
    }
    let n = normalize(textureLoad(rt_normal, id.xy, 0).xyz);
    let tangent = textureLoad(rt_tangent, id.xy, 0);
    let ray = textureLoad(rt_ray, id.xy, 0);
    let group = u32(p.w + 0.5) - 1u;
    let flags = params.config.w;
    let eps = params.bounds_max.w;

    var dir = p.xyz - ray.xyz;
    let len = length(dir);
    dir = select(-n, dir / len, len > 1e-12);
    let h = trace(ray.xyz, dir, -eps, ray.w, group, (flags & FLAG_MATCH_BY_NAME) != 0u, false, false);

    var hp = p.xyz;
    var hn = n;
    var color = vec3(1.0);
    var curvature = 0.0;
    var id_value = 0u;
    if h.hit {
        let w = bary(h);
        let i0 = indices[h.tri * 3u];
        let i1 = indices[h.tri * 3u + 1u];
        let i2 = indices[h.tri * 3u + 2u];
        let p0 = positions[i0];
        let p1 = positions[i1];
        let p2 = positions[i2];
        hp = p0.xyz * w.x + p1.xyz * w.y + p2.xyz * w.z;
        curvature = p0.w * w.x + p1.w * w.y + p2.w * w.z;
        let n0 = normals[i0];
        let n1 = normals[i1];
        let n2 = normals[i2];
        hn = normalize(n0.xyz * w.x + n1.xyz * w.y + n2.xyz * w.z);
        // Vertex colors are flat-sampled from the closest corner, like ID masks expect.
        let corner = select(select(2u, 1u, w.y >= w.z), 0u, w.x >= w.y && w.x >= w.z);
        color = unpack_color(select(select(n2.w, n1.w, corner == 1u), n0.w, corner == 0u));
        id_value = tri_info[h.tri].y;
    }
    let directx = (flags & FLAG_DIRECTX) != 0u;
    let base = texel;
    let coverage = 1.0;
    if has_map(params, MAP_NORMAL) {
        maps[MAP_NORMAL * n_texels + base] = vec4(to_tangent(hn, n, tangent, directx), coverage);
    }
    if has_map(params, MAP_WORLD_NORMAL) {
        maps[MAP_WORLD_NORMAL * n_texels + base] = vec4(hn * 0.5 + 0.5, coverage);
    }
    if has_map(params, MAP_POSITION) {
        let extent = max(params.bounds_max.xyz - params.bounds_min.xyz, vec3(1e-9));
        maps[MAP_POSITION * n_texels + base] = vec4(clamp((hp - params.bounds_min.xyz) / extent, vec3(0.0), vec3(1.0)), coverage);
    }
    if has_map(params, MAP_HEIGHT) {
        let d = dot(hp - p.xyz, n) / max(params.distances.z, 1e-9);
        maps[MAP_HEIGHT * n_texels + base] = vec4(vec3(clamp(0.5 + 0.5 * d, 0.0, 1.0)), coverage);
    }
    if has_map(params, MAP_OPACITY) {
        maps[MAP_OPACITY * n_texels + base] = vec4(vec3(select(0.0, 1.0, h.hit)), coverage);
    }
    if has_map(params, MAP_ID) {
        let source = (flags >> 8u) & 0xffu;
        let c = select(hue_color(id_value), color, source == 0u);
        maps[MAP_ID * n_texels + base] = vec4(c, coverage);
    }
    if has_map(params, MAP_CURVATURE) {
        let c = 0.5 + 0.5 * tanh(curvature * params.distances.w);
        maps[MAP_CURVATURE * n_texels + base] = vec4(vec3(c), coverage);
    }
    surface[texel * 2u] = vec4(hp + hn * eps * 4.0, 1.0);
    surface[texel * 2u + 1u] = vec4(hn, f32(group));
}

//--- occlusion
// One batch of AO / bent normal / thickness samples per texel, accumulated.
@group(0) @binding(4) var<uniform> params: Params;
@group(0) @binding(5) var<storage, read> surface: array<vec4<f32>>;
// Two entries per texel: (unoccluded count, thickness sum, 0, 0), (bent normal sum, 0).
@group(0) @binding(6) var<storage, read_write> accum: array<vec4<f32>>;

@compute @workgroup_size(8, 8, 1)
fn occlusion(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params.tile_size;
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let texel = id.y * size.x + id.x;
    let s0 = surface[texel * 2u];
    var a0 = vec4(0.0);
    var a1 = vec4(0.0);
    if params.samples.y > 0u {
        a0 = accum[texel * 2u];
        a1 = accum[texel * 2u + 1u];
    }
    if s0.w < 0.5 {
        accum[texel * 2u] = a0;
        accum[texel * 2u + 1u] = a1;
        return;
    }
    let o = s0.xyz;
    let s1 = surface[texel * 2u + 1u];
    let n = s1.xyz;
    let group = u32(s1.w + 0.5);
    let same_name = (params.config.w & FLAG_AO_SAME_NAME) != 0u;
    let eps = params.bounds_max.w;
    let skip_back = (params.config.w & FLAG_AO_IGNORE_BACKFACES) != 0u;
    let global = (params.tile_origin.y + id.y) * params.config.x + params.tile_origin.x + id.x;
    let seed = pcg(global);
    let first = params.samples.y;
    let end = first + params.samples.z;

    let want_ao = has_map(params, MAP_AO) || has_map(params, MAP_BENT_NORMAL);
    let want_thickness = has_map(params, MAP_THICKNESS);
    for (var i = first; i < end; i++) {
        let r = rand2(seed, i);
        if want_ao && i < params.samples.x {
            let d = cosine_dir(n, r);
            let h = trace(o, d, eps, params.distances.x, group, same_name, true, skip_back);
            if !h.hit {
                a0.x += 1.0;
                a1 = vec4(a1.xyz + d, 0.0);
            }
        }
        if want_thickness && i < params.samples.w {
            let d = cosine_dir(-n, r);
            let origin = o - n * eps * 8.0;
            let h = trace(origin, d, eps, params.distances.y, 0u, false, false, false);
            a0.y += select(1.0, h.t / params.distances.y, h.hit);
        }
    }
    accum[texel * 2u] = a0;
    accum[texel * 2u + 1u] = a1;
}

//--- resolve
// Averages supersampled texels into output texels and finishes accumulated maps.
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> maps: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> accum: array<vec4<f32>>;
@group(0) @binding(3) var rt_position: texture_2d<f32>;
@group(0) @binding(4) var rt_normal: texture_2d<f32>;
@group(0) @binding(5) var rt_tangent: texture_2d<f32>;
// MAP_COUNT slots of (output tile texels), w = coverage (0-1).
@group(0) @binding(6) var<storage, read_write> out_maps: array<vec4<f32>>;

@compute @workgroup_size(8, 8, 1)
fn resolve(@builtin(global_invocation_id) id: vec3<u32>) {
    let ss = params.config.y;
    let out_size = params.tile_size / ss;
    if id.x >= out_size.x || id.y >= out_size.y {
        return;
    }
    let n_texels = params.tile_size.x * params.tile_size.y;
    let out_texel = id.y * out_size.x + id.x;
    let n_out = out_size.x * out_size.y;
    let directx = (params.config.w & FLAG_DIRECTX) != 0u;
    var sums: array<vec3<f32>, 10>;
    for (var s = 0u; s < MAP_COUNT; s++) {
        sums[s] = vec3(0.0);
    }
    var covered = 0.0;
    for (var sy = 0u; sy < ss; sy++) {
        for (var sx = 0u; sx < ss; sx++) {
            let t = id.xy * ss + vec2(sx, sy);
            if textureLoad(rt_position, t, 0).w < 0.5 {
                continue;
            }
            covered += 1.0;
            let texel = t.y * params.tile_size.x + t.x;
            for (var s = 0u; s < MAP_COUNT; s++) {
                if s == MAP_AO || s == MAP_THICKNESS || s == MAP_BENT_NORMAL {
                    continue;
                }
                if has_map(params, s) {
                    sums[s] += maps[s * n_texels + texel].rgb;
                }
            }
            let a0 = accum[texel * 2u];
            let a1 = accum[texel * 2u + 1u];
            if has_map(params, MAP_AO) {
                sums[MAP_AO] += vec3(a0.x / f32(max(params.samples.x, 1u)));
            }
            if has_map(params, MAP_THICKNESS) {
                sums[MAP_THICKNESS] += vec3(a0.y / f32(max(params.samples.w, 1u)));
            }
            if has_map(params, MAP_BENT_NORMAL) {
                let n = normalize(textureLoad(rt_normal, t, 0).xyz);
                let tangent = textureLoad(rt_tangent, t, 0);
                let bent = select(n, normalize(a1.xyz), dot(a1.xyz, a1.xyz) > 1e-12);
                sums[MAP_BENT_NORMAL] += to_tangent(bent, n, tangent, directx);
            }
        }
    }
    let coverage = covered / f32(ss * ss);
    for (var s = 0u; s < MAP_COUNT; s++) {
        var v = vec4(0.0);
        if covered > 0.0 {
            v = vec4(sums[s] / covered, coverage);
            if s == MAP_NORMAL || s == MAP_BENT_NORMAL {
                // Renormalize the averaged tangent-space vector.
                v = vec4(normalize(v.xyz * 2.0 - 1.0) * 0.5 + 0.5, coverage);
            }
        }
        out_maps[s * n_out + out_texel] = v;
    }
}
