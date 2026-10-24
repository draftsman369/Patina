// Viewport scene: background, PBR mesh and wireframe overlay.
// Prefixed with common.wgsl.

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

// Karis' analytic approximation of the split-sum environment BRDF.
fn env_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
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
    var n = ng;
    if has_uvs {
        base *= textureSample(t_base_color, s_material, uv);
        metallic *= textureSample(t_metallic, s_material, uv).r;
        roughness *= textureSample(t_roughness, s_material, uv).r;
        emissive *= textureSample(t_emissive, s_material, uv).rgb;
        tex_normal = textureSample(t_normal, s_material, uv).rgb;

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
        // Orthographic: constant view direction (camera position holds the direction).
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
        case VIEW_UV_CHECKER: {
            let shade = 0.35 + 0.65 * max(dot(n, v), 0.0);
            return vec4(checker(uv) * shade, 1.0);
        }
        default: {}
    }

    // Metal/rough PBR lit by the analytic environment.
    roughness = clamp(roughness, 0.03, 1.0);
    metallic = clamp(metallic, 0.0, 1.0);
    let n_dot_v = clamp(dot(n, v), 1e-4, 1.0);
    let f0 = mix(vec3(0.04), base.rgb, metallic);
    let alpha = roughness * roughness;

    let r = reflect(-v, n);
    // GGX lobe approximated as a spherical Gaussian of sharpness 2 / alpha^2.
    let spec_k = 2.0 / (alpha * alpha);
    // Fade reflections that point below the geometric surface.
    let horizon = clamp(1.0 + dot(r, ng), 0.0, 1.0);
    let specular = env_radiance(r, spec_k) * env_brdf(f0, roughness, n_dot_v) * horizon * horizon;

    // Clamped-cosine lobe ~ spherical Gaussian of sharpness 2.
    let irradiance = env_radiance(n, 2.0);
    let diffuse = base.rgb * (1.0 - metallic) * irradiance * (1.0 - f0);

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
    let dir = normalize(far.xyz / far.w - near.xyz / near.w);
    let env = env_radiance(dir, frame.env.z);
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
