// Resolves the HDR viewport into the 8-bit sRGB output.

struct Params {
    // x = exposure multiplier (2^EV), y unused.
    exposure: vec4<f32>,
    // x = tonemap operator (0 ACES, 1 Filmic, 2 Linear), y = 1 to bypass (channel views).
    mode: vec4<u32>,
}

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    return vec4(p, 0.0, 1.0);
}

// ACES fitted curve (Stephen Hill), including the sRGB → ACES AP1 transforms.
fn aces(color: vec3<f32>) -> vec3<f32> {
    let input = mat3x3<f32>(
        vec3(0.59719, 0.07600, 0.02840),
        vec3(0.35458, 0.90834, 0.13383),
        vec3(0.04823, 0.01566, 0.83777),
    );
    let output = mat3x3<f32>(
        vec3(1.60475, -0.10208, -0.00327),
        vec3(-0.53108, 1.10813, -0.07276),
        vec3(-0.07367, -0.00605, 1.07602),
    );
    let v = input * color;
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return clamp(output * (a / b), vec3(0.0), vec3(1.0));
}

// Hable's filmic curve.
fn hable_partial(x: vec3<f32>) -> vec3<f32> {
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    return ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f;
}

fn filmic(color: vec3<f32>) -> vec3<f32> {
    let white = hable_partial(vec3(11.2));
    return clamp(hable_partial(color * 2.0) / white, vec3(0.0), vec3(1.0));
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(hdr, vec2<i32>(pos.xy), 0).rgb;
    if params.mode.y == 1u {
        return vec4(clamp(c, vec3(0.0), vec3(1.0)), 1.0);
    }
    let x = c * params.exposure.x;
    var mapped: vec3<f32>;
    switch params.mode.x {
        case 0u: { mapped = aces(x); }
        case 1u: { mapped = filmic(x); }
        default: { mapped = clamp(x, vec3(0.0), vec3(1.0)); }
    }
    // The output target is sRGB: the hardware applies the transfer function.
    return vec4(mapped, 1.0);
}
