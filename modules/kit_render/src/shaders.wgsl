// Kit built-in shaders. Colors arrive in linear space; targets are sRGB.

struct Frame {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // xyz: direction the light travels
    light_dir: vec4<f32>,
    // rgb: color * intensity
    light_color: vec4<f32>,
    // x: lighting on, y: time, z: grid plane (0 = XZ, 1 = XY), w: grid fade distance
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Push {
    model: mat4x4<f32>,
    color: vec4<f32>,
    // sprites: flip u and flip v; meshes: negative two-sided flag and alpha cutoff
    // z: selected, w: unlit
    extra: vec4<f32>,
    // roughness, metallic, imported surface flag (sprites: x = soft round dot, for particles)
    surface: vec4<f32>,
    emission: vec4<f32>,
};
var<immediate> pc: Push;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

const ACCENT: vec3<f32> = vec3<f32>(0.533, 0.396, 1.0);

@vertex
fn vs_mesh(v: VIn) -> VOut {
    var o: VOut;
    let w = pc.model * vec4<f32>(v.pos, 1.0);
    o.clip = frame.view_proj * w;
    o.world = w.xyz;
    o.normal = (pc.model * vec4<f32>(v.normal, 0.0)).xyz;
    var uv = v.uv;
    if (pc.extra.x > 0.5) { uv.x = 1.0 - uv.x; }
    if (pc.extra.y > 0.5) { uv.y = 1.0 - uv.y; }
    o.uv = uv;
    return o;
}

fn shade(base: vec3<f32>, n_in: vec3<f32>, world: vec3<f32>) -> vec3<f32> {
    if (frame.params.x < 0.5 || pc.extra.w > 0.5) {
        return base;
    }
    let n = normalize(n_in);
    let l = normalize(-frame.light_dir.xyz);
    let v = normalize(frame.camera_pos.xyz - world);
    let h = normalize(l + v);
    let diff = max(dot(n, l), 0.0);
    let spec = pow(max(dot(n, h), 0.0), 64.0) * 0.4;
    // Studio hemisphere: soft violet sky, dark floor.
    let sky = vec3<f32>(0.20, 0.17, 0.30);
    let ground = vec3<f32>(0.035, 0.03, 0.05);
    let ambient = mix(ground, sky, n.y * 0.5 + 0.5);
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.35 * ACCENT;
    if (pc.surface.z > 0.5) {
        let roughness = clamp(pc.surface.x, 0.0, 1.0);
        let metallic = clamp(pc.surface.y, 0.0, 1.0);
        let highlight = pow(max(dot(n, h), 0.0), mix(128.0, 2.0, roughness));
        let reflectance = mix(vec3<f32>(0.04), base, metallic);
        return base * (ambient + (1.0 - metallic) * diff * frame.light_color.rgb)
            + highlight * reflectance * frame.light_color.rgb + pc.emission.rgb;
    }
    return base * (ambient + diff * frame.light_color.rgb) + spec * frame.light_color.rgb + rim;
}

@fragment
fn fs_mesh(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (!front && pc.extra.x >= 0.0) { discard; }
    let t = textureSample(tex, samp, i.uv) * pc.color;
    if (t.a < -pc.extra.y) { discard; }
    let normal = select(-i.normal, i.normal, front);
    var c = shade(t.rgb, normal, i.world);
    if (pc.extra.z > 0.5) {
        let v = normalize(frame.camera_pos.xyz - i.world);
        let f = pow(1.0 - abs(dot(normalize(i.normal), v)), 2.0);
        c = mix(c, ACCENT, 0.2 + 0.6 * f);
    }
    return vec4<f32>(c, t.a);
}

@fragment
fn fs_sprite(i: VOut) -> @location(0) vec4<f32> {
    var t = textureSample(tex, samp, i.uv) * pc.color;
    if (pc.surface.x > 0.5) {
        // Particles without a texture: a soft round dot.
        let d = length(i.uv - vec2<f32>(0.5, 0.5)) * 2.0;
        t.a = t.a * (1.0 - smoothstep(0.55, 1.0, d));
    }
    if (t.a < 0.004) {
        discard;
    }
    var c = t.rgb + pc.emission.rgb;
    if (pc.extra.z > 0.5) {
        c = mix(c, ACCENT, 0.35);
    }
    return vec4<f32>(c, t.a);
}

fn grid_line(p: vec2<f32>, size: f32) -> f32 {
    let c = p / size;
    let d = max(fwidth(c), vec2<f32>(1e-5, 1e-5));
    let g = abs(fract(c - 0.5) - 0.5) / d;
    return 1.0 - min(min(g.x, g.y), 1.0);
}

@fragment
fn fs_grid(i: VOut) -> @location(0) vec4<f32> {
    var p = i.world.xz;
    if (frame.params.z > 0.5) {
        p = i.world.xy;
    }
    let minor = grid_line(p, 1.0);
    let major = grid_line(p, 10.0);
    let dist = length(i.world - frame.camera_pos.xyz);
    var fade = 1.0 - smoothstep(frame.params.w * 0.25, frame.params.w, dist);
    if (frame.params.z > 0.5) {
        fade = 1.0;
    }
    // Axis lines are a little brighter.
    let axis_w = fwidth(p) * 1.5;
    let axis = max(1.0 - smoothstep(0.0, axis_w.x, abs(p.x)), 1.0 - smoothstep(0.0, axis_w.y, abs(p.y)));
    let a = max(max(minor * 0.16, major * 0.32), axis * 0.5) * fade;
    let col = mix(vec3<f32>(0.30, 0.22, 0.55), ACCENT, axis);
    return vec4<f32>(col, a);
}

struct FOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FOut {
    var o: FOut;
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    o.clip = vec4<f32>(uv * 2.0 - 1.0, 1.0, 1.0);
    o.uv = uv;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2, 2.2, 2.2));
}

// Dimension's stage: radial-gradient(ellipse at 50% 43%, #40345b, #201b30 50%, #151420 89%)
@fragment
fn fs_backdrop(i: FOut) -> @location(0) vec4<f32> {
    let center = vec2<f32>(0.5, 0.43);
    let radii = vec2<f32>(0.5, 0.57) * 1.41421;
    let t = length((i.uv - center) / radii);
    let c0 = vec3<f32>(64.0, 52.0, 91.0) / 255.0;
    let c1 = vec3<f32>(32.0, 27.0, 48.0) / 255.0;
    let c2 = vec3<f32>(21.0, 20.0, 32.0) / 255.0;
    var c = mix(c0, c1, smoothstep(0.0, 0.5, t));
    c = mix(c, c2, smoothstep(0.5, 0.89, t));
    return vec4<f32>(srgb_to_linear(c), 1.0);
}

// ---------------------------------------------------------------- terrain

struct TIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec3<f32>,
};

struct TOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};

@vertex
fn vs_terrain(v: TIn) -> TOut {
    var o: TOut;
    let w = pc.model * vec4<f32>(v.pos, 1.0);
    o.clip = frame.view_proj * w;
    o.world = w.xyz;
    o.normal = (pc.model * vec4<f32>(v.normal, 0.0)).xyz;
    o.color = v.color;
    return o;
}

@fragment
fn fs_terrain(i: TOut) -> @location(0) vec4<f32> {
    var c = shade(i.color * pc.color.rgb, i.normal, i.world);
    if (pc.extra.z > 0.5) {
        c = mix(c, ACCENT, 0.12);
    }
    return vec4<f32>(c, 1.0);
}
