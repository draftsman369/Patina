// Viewport scene: background, PBR mesh and wireframe overlay.
// Prefixed with common.wgsl.

struct Frame {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz = camera position (direction toward the camera when orthographic), w = 1 for orthographic.
    camera_pos: vec4<f32>,
    // x = cos(rotation), y = sin(rotation), z = background blur (0-1), w = background opacity.
    env: vec4<f32>,
    // x = highest specular mip, y = environment intensity.
    env_info: vec4<f32>,
    // x = view mode.
    mode: vec4<u32>,
    // rgb = flat background color (HDR).
    background: vec4<f32>,
    wire_color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;

@group(2) @binding(0) var env_cube: texture_cube<f32>;
@group(2) @binding(1) var env_specular: texture_cube<f32>;
@group(2) @binding(2) var brdf_lut: texture_2d<f32>;
@group(2) @binding(3) var env_sampler: sampler;
@group(2) @binding(4) var<uniform> sh: array<vec4<f32>, 9>;

// World direction → environment space (rotation around Y).
fn rotate_env(d: vec3<f32>) -> vec3<f32> {
    let c = frame.env.x;
    let s = frame.env.y;
    return vec3(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

// Diffuse irradiance divided by π, from SH9 convolved with a clamped cosine.
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    let d = rotate_env(n);
    var e = sh[0].rgb * 0.282095;
    e += (sh[1].rgb * d.y + sh[2].rgb * d.z + sh[3].rgb * d.x) * (0.488603 * 2.0 / 3.0);
    e += (sh[4].rgb * (1.092548 * d.x * d.y) + sh[5].rgb * (1.092548 * d.y * d.z)
        + sh[6].rgb * (0.315392 * (3.0 * d.z * d.z - 1.0)) + sh[7].rgb * (1.092548 * d.x * d.z)
        + sh[8].rgb * (0.546274 * (d.x * d.x - d.y * d.y))) * 0.25;
    return max(e, vec3(0.0)) * frame.env_info.y;
}

fn specular_env(r: vec3<f32>, roughness: f32) -> vec3<f32> {
    return textureSampleLevel(env_specular, env_sampler, rotate_env(r), roughness * frame.env_info.x).rgb
        * frame.env_info.y;
}

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
}

@vertex
fn vs_mesh(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = frame.view_proj * vec4(v.position, 1.0);
    out.world_pos = v.position;
    out.normal = v.normal;
    out.uv = v.uv;
    out.tangent = v.tangent;
    return out;
}

fn checker(uv: vec2<f32>) -> vec3<f32> {
    let cells = floor(uv * 8.0);
    let odd = (i32(cells.x) + i32(cells.y)) & 1;
    // Tint each 1x1 UV tile differently so overlapping/tiled UVs stand out.
    let tile = floor(uv);
    let tint = 0.75 + 0.25 * vec3(
        fract(sin(dot(tile, vec2(12.9898, 78.233))) * 43758.55),
        fract(sin(dot(tile, vec2(39.3468, 11.135))) * 24634.63),
        fract(sin(dot(tile, vec2(73.156, 52.235))) * 13758.37),
    );
    let base = select(vec3(0.85), vec3(0.25), odd == 1);
    return base * tint;
}

@fragment
fn fs_mesh(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let has_uvs = material.params.w > 0.5;
    let uv = in.uv;

    // Geometric normal, flipped for back faces so open meshes shade sensibly.
    var ng = normalize(in.normal);
    if !front {
        ng = -ng;
    }

    var base = material.base_color;
    var metallic = material.params.x;
    var roughness = material.params.y;
    var emissive = material.emissive.rgb;
    var tex_normal = vec3(0.5, 0.5, 1.0);
    var mesh_map = vec3(0.0);
    var n = ng;
    if has_uvs {
        base *= textureSample(t_base_color, s_material, uv);
        metallic *= textureSample(t_metallic, s_material, uv).r;
        roughness *= textureSample(t_roughness, s_material, uv).r;
        emissive *= textureSample(t_emissive, s_material, uv).rgb;
        tex_normal = textureSample(t_normal, s_material, uv).rgb;
        mesh_map = textureSample(t_mesh_map, s_material, uv).rgb;

        var ts = tex_normal * 2.0 - 1.0;
        ts.y *= material.params.z;
        var t = in.tangent.xyz;
        t = normalize(t - ng * dot(ng, t));
        let b = cross(ng, t) * in.tangent.w;
        n = normalize(t * ts.x + b * ts.y + ng * ts.z);
    }

    let mode = frame.mode.x;
    let v = normalize(select(
        frame.camera_pos.xyz - in.world_pos,
        frame.camera_pos.xyz,
        frame.camera_pos.w > 0.5,
    ));

    switch mode {
        case VIEW_BASE_COLOR: { return vec4(base.rgb, 1.0); }
        case VIEW_METALLIC: { return vec4(display_data(vec3(metallic)), 1.0); }
        case VIEW_ROUGHNESS: { return vec4(display_data(vec3(roughness)), 1.0); }
        case VIEW_NORMAL: { return vec4(display_data(tex_normal), 1.0); }
        case VIEW_WORLD_NORMAL: { return vec4(display_data(n * 0.5 + 0.5), 1.0); }
        case VIEW_EMISSIVE: { return vec4(emissive, 1.0); }
        case VIEW_MESH_MAP: { return vec4(display_data(mesh_map), 1.0); }
        case VIEW_UV_CHECKER: {
            let shade = 0.35 + 0.65 * max(dot(n, v), 0.0);
            return vec4(checker(uv) * shade, 1.0);
        }
        default: {}
    }

    // Metal/rough PBR with image-based lighting (split-sum specular, SH9 diffuse).
    roughness = clamp(roughness, 0.03, 1.0);
    metallic = clamp(metallic, 0.0, 1.0);
    let n_dot_v = clamp(dot(n, v), 1e-4, 1.0);
    let f0 = mix(vec3(0.04), base.rgb, metallic);

    let r = reflect(-v, n);
    // Fade reflections that point below the geometric surface.
    let horizon = clamp(1.0 + dot(r, ng), 0.0, 1.0);
    let ab = textureSampleLevel(brdf_lut, env_sampler, vec2(n_dot_v, roughness), 0.0).rg;
    let specular = specular_env(r, roughness) * (f0 * ab.x + ab.y) * horizon * horizon;

    // Roughness-aware Fresnel keeps diffuse + specular energy in check.
    let fresnel = f0 + (max(vec3(1.0 - roughness), f0) - f0) * pow(1.0 - n_dot_v, 5.0);
    let diffuse = base.rgb * (1.0 - metallic) * (1.0 - fresnel) * irradiance(n);

    return vec4(diffuse + specular + emissive, 1.0);
}

// --- Background --------------------------------------------------------------

struct BackgroundOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BackgroundOut {
    // Fullscreen triangle.
    let p = vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: BackgroundOut;
    out.clip = vec4(p, 0.0, 1.0);
    out.ndc = p;
    return out;
}

@fragment
fn fs_background(in: BackgroundOut) -> @location(0) vec4<f32> {
    if frame.mode.x != VIEW_MATERIAL {
        // Channel views are display-referred: flat neutral background.
        return vec4(vec3(0.03), 1.0);
    }
    let near = frame.inv_view_proj * vec4(in.ndc, 0.0, 1.0);
    let far = frame.inv_view_proj * vec4(in.ndc, 1.0, 1.0);
    let dir = rotate_env(normalize(far.xyz / far.w - near.xyz / near.w));
    let blur = frame.env.z;
    let sharp = textureSampleLevel(env_cube, env_sampler, dir, 0.0).rgb;
    let soft = textureSampleLevel(env_specular, env_sampler, dir, blur * frame.env_info.x).rgb;
    let env = mix(sharp, soft, clamp(blur * 6.0, 0.0, 1.0)) * frame.env_info.y;
    return vec4(mix(frame.background.rgb, env, frame.env.w), 1.0);
}

// --- Wireframe ---------------------------------------------------------------

@vertex
fn vs_wire(v: VertexIn) -> @builtin(position) vec4<f32> {
    var clip = frame.view_proj * vec4(v.position, 1.0);
    // Pull lines slightly toward the camera so they win the depth test.
    clip.z -= 0.0004 * clip.w;
    return clip;
}

@fragment
fn fs_wire() -> @location(0) vec4<f32> {
    return frame.wire_color;
}
