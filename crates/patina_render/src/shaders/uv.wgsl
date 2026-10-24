// 2D UV view: channel texture over [0,1]², tile grid and UV wireframe.
// Prefixed with common.wgsl (only the material bindings are used; group 0
// is replaced by the UV view's own uniforms below).

struct UvParams {
    // xy = scale (UV → NDC), zw = offset.
    transform: vec4<f32>,
    // x = channel to display (Channel::index), y = 1 if the mesh has UVs.
    channel: vec4<u32>,
    wire_color: vec4<f32>,
}

@group(2) @binding(0) var<uniform> uvp: UvParams;

fn uv_to_ndc(uv: vec2<f32>) -> vec2<f32> {
    // UV v grows downward, NDC y grows upward.
    return vec2(uv.x, -uv.y) * uvp.transform.xy + uvp.transform.zw;
}

struct QuadOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_quad(@builtin(vertex_index) i: u32) -> QuadOut {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: QuadOut;
    out.clip = vec4(p, 0.0, 1.0);
    out.ndc = p;
    return out;
}

fn channel_value(uv: vec2<f32>) -> vec3<f32> {
    switch uvp.channel.x {
        case 0u: { return (material.base_color * textureSample(t_base_color, s_material, uv)).rgb; }
        case 1u: { return display_data(vec3(material.params.x * textureSample(t_metallic, s_material, uv).r)); }
        case 2u: { return display_data(vec3(material.params.y * textureSample(t_roughness, s_material, uv).r)); }
        case 3u: { return display_data(textureSample(t_normal, s_material, uv).rgb); }
        default: { return material.emissive.rgb * textureSample(t_emissive, s_material, uv).rgb; }
    }
}

@fragment
fn fs_quad(in: QuadOut) -> @location(0) vec4<f32> {
    // Invert uv_to_ndc.
    let s = (in.ndc - uvp.transform.zw) / uvp.transform.xy;
    let uv = vec2(s.x, -s.y);

    // Derivatives must be taken in uniform control flow.
    let value = channel_value(fract(uv));
    let px = fwidth(uv);

    var color = vec3(0.025);
    let inside = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    if inside {
        color = value;
    } else {
        // Dim the repeated texture outside the 0–1 tile.
        color = mix(color, value, 0.12);
    }
    // Grid lines at UV tile boundaries.
    let grid = abs(uv - round(uv)) / px;
    if min(grid.x, grid.y) < 1.0 {
        color = vec3(0.35);
    }
    return vec4(color, 1.0);
}

@vertex
fn vs_wire(@location(2) uv: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(uv_to_ndc(uv), 0.0, 1.0);
}

@fragment
fn fs_wire() -> @location(0) vec4<f32> {
    return uvp.wire_color;
}
