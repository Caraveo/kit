// Kit post-processing. Shared by the Vulkan renderer (compiled by naga) and
// the WebGPU web player. Every pass is a fullscreen triangle with the same
// bindings; the pass list (which entry points run, in which order, into which
// targets) comes from kit_post::plan.
//
// Colors are linear (HDR) until fs_final, which writes linear display values;
// the output target's sRGB format encodes them once.

struct Params {
    // saturation, contrast, temperature, -
    color: vec4<f32>,
    // ev, tonemap (0 none / 1 filmic), -, -
    exposure: vec4<f32>,
    // threshold, intensity, radius, -
    glow: vec4<f32>,
    // distance, range, strength, -
    focus: vec4<f32>,
    // radius, strength, quality samples, -
    ambient: vec4<f32>,
    // fxaa, sharpen, -, -
    image: vec4<f32>,
};

struct Volume {
    to_local: mat4x4<f32>,
    // shape (0 box, 1 sphere), blend distance, weight, sphere radius
    shape: vec4<f32>,
    // box half extents
    half: vec4<f32>,
    // override per group (0 inherit, 1 on, 2 off): color, exposure, glow, focus
    modes_a: vec4<f32>,
    // ambient, image, -, -
    modes_b: vec4<f32>,
    p: Params,
};

struct Post {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    // xyz position, w = 1 for orthographic cameras
    camera_pos: vec4<f32>,
    camera_fwd: vec4<f32>,
    // width, height, 1/width, 1/height
    screen: vec4<f32>,
    // object volume count, AO samples, NDC y-up (1 on WebGPU), max blur radius in px
    info: vec4<f32>,
    base: Params,
    objects: array<Volume, 8>,
};

struct Pass {
    // destination width, height, 1/width, 1/height
    dst: vec4<f32>,
    // source texel size (xy), flags (z), extra (w)
    a: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var<uniform> pass_info: Pass;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var tex_a: texture_2d<f32>;
@group(0) @binding(4) var tex_b: texture_2d<f32>;
@group(0) @binding(5) var depth_tex: texture_depth_multisampled_2d;

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

@vertex
fn vs_post(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn uv_of(pos: vec4<f32>) -> vec2<f32> {
    return pos.xy * pass_info.dst.zw;
}

fn sample_a(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(tex_a, samp, uv, 0.0);
}

fn sample_b(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(tex_b, samp, uv, 0.0);
}

// Scene depth (0 near … 1 far) at a screen uv. MSAA depth: sample 0.
fn depth_at(uv: vec2<f32>) -> f32 {
    let size = post.screen.xy;
    let px = vec2<i32>(clamp(uv * size, vec2<f32>(0.0), size - vec2<f32>(1.0)));
    return textureLoad(depth_tex, px, 0);
}

fn is_sky(depth: f32) -> bool {
    return depth >= 0.999999;
}

fn ndc_of(uv: vec2<f32>) -> vec2<f32> {
    var ndc = uv * 2.0 - 1.0;
    if (post.info.z > 0.5) {
        ndc.y = -ndc.y;
    }
    return ndc;
}

fn uv_of_ndc(ndc: vec2<f32>) -> vec2<f32> {
    var n = ndc;
    if (post.info.z > 0.5) {
        n.y = -n.y;
    }
    return n * 0.5 + 0.5;
}

fn world_at(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let h = post.inv_view_proj * vec4<f32>(ndc_of(uv), depth, 1.0);
    return h.xyz / h.w;
}

// Distance in front of the camera along its view direction (perspective and
// orthographic alike).
fn view_depth(p: vec3<f32>) -> f32 {
    return dot(p - post.camera_pos.xyz, post.camera_fwd.xyz);
}

// ------------------------------------------------- "Objects inside" volumes

fn smooth01(x: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// Same rule as kit_post::resolve::influence.
fn influence(v: Volume, p: vec3<f32>) -> f32 {
    let q = (v.to_local * vec4<f32>(p, 1.0)).xyz;
    var d: f32;
    if (v.shape.x < 0.5) {
        d = length(max(abs(q) - v.half.xyz, vec3<f32>(0.0)));
    } else {
        d = max(length(q) - v.shape.w, 0.0);
    }
    let w = clamp(v.shape.z, 0.0, 1.0);
    if (d <= 0.0) {
        return w;
    }
    if (v.shape.y <= 0.0) {
        return 0.0;
    }
    return w * smooth01(1.0 - d / v.shape.y);
}

fn blend(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> {
    return a + (b - a) * t;
}

// Same rule as kit_post::resolve::Settings::apply.
fn apply(s_in: Params, v: Volume, t: f32) -> Params {
    var s = s_in;
    let half = t >= 0.5;
    // Color
    if (v.modes_a.x == 1.0) { s.color = vec4<f32>(blend(s.color, v.p.color, t).xyz, 0.0); }
    if (v.modes_a.x == 2.0) { s.color = vec4<f32>(blend(s.color, vec4<f32>(1.0, 1.0, 0.0, 0.0), t).xyz, 0.0); }
    // Exposure (tone mapping switches at half)
    if (v.modes_a.y == 1.0) {
        s.exposure.x = s.exposure.x + (v.p.exposure.x - s.exposure.x) * t;
        if (half) { s.exposure.y = v.p.exposure.y; }
    }
    if (v.modes_a.y == 2.0) {
        s.exposure.x = s.exposure.x * (1.0 - t);
        if (half) { s.exposure.y = 0.0; }
    }
    // Glow
    if (v.modes_a.z == 1.0) { s.glow = vec4<f32>(blend(s.glow, v.p.glow, t).xyz, 0.0); }
    if (v.modes_a.z == 2.0) { s.glow.y = s.glow.y * (1.0 - t); }
    // Focus
    if (v.modes_a.w == 1.0) { s.focus = vec4<f32>(blend(s.focus, v.p.focus, t).xyz, 0.0); }
    if (v.modes_a.w == 2.0) { s.focus.z = s.focus.z * (1.0 - t); }
    // Ambient (quality is per camera)
    if (v.modes_b.x == 1.0) {
        s.ambient.x = s.ambient.x + (v.p.ambient.x - s.ambient.x) * t;
        s.ambient.y = s.ambient.y + (v.p.ambient.y - s.ambient.y) * t;
    }
    if (v.modes_b.x == 2.0) { s.ambient.y = s.ambient.y * (1.0 - t); }
    // Image
    if (v.modes_b.y == 1.0) { s.image = vec4<f32>(blend(s.image, v.p.image, t).xy, 0.0, 0.0); }
    if (v.modes_b.y == 2.0) { s.image = vec4<f32>(s.image.xy * (1.0 - t), 0.0, 0.0); }
    return s;
}

// Settings at a pixel: the camera's, then "Objects inside" volumes around
// what that pixel shows (in priority order).
fn params_at(uv: vec2<f32>) -> Params {
    var s = post.base;
    let n = u32(post.info.x);
    if (n == 0u) {
        return s;
    }
    let d = depth_at(uv);
    if (is_sky(d)) {
        return s;
    }
    let p = world_at(uv, d);
    for (var i = 0u; i < n; i = i + 1u) {
        let t = influence(post.objects[i], p);
        if (t > 0.0) {
            s = apply(s, post.objects[i], t);
        }
    }
    return s;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// ------------------------------------------------------- Ambient occlusion

@fragment
fn fs_ao(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let d = depth_at(uv);
    if (is_sky(d)) {
        return vec4<f32>(1.0);
    }
    let s = params_at(uv);
    let radius = max(s.ambient.x, 0.01);
    if (s.ambient.y <= 0.001) {
        return vec4<f32>(1.0);
    }
    let p = world_at(uv, d);
    let tx = vec2<f32>(post.screen.z, 0.0);
    let ty = vec2<f32>(0.0, post.screen.w);
    let pr = world_at(uv + tx, depth_at(uv + tx));
    let pl = world_at(uv - tx, depth_at(uv - tx));
    let pd = world_at(uv + ty, depth_at(uv + ty));
    let pu = world_at(uv - ty, depth_at(uv - ty));
    // The smaller difference on each axis avoids normals across edges.
    var dx = pr - p;
    if (length(p - pl) < length(dx)) { dx = p - pl; }
    var dy = pd - p;
    if (length(p - pu) < length(dy)) { dy = p - pu; }
    var n = normalize(cross(dy, dx));
    var to_cam = post.camera_pos.xyz - p;
    if (post.camera_pos.w > 0.5) { to_cam = -post.camera_fwd.xyz; }
    if (dot(n, to_cam) < 0.0) { n = -n; }
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if (abs(n.y) > 0.9) { up = vec3<f32>(1.0, 0.0, 0.0); }
    let t1 = normalize(cross(up, n));
    let t2 = cross(n, t1);
    let count = max(u32(post.info.y), 1u);
    let spin = hash(pos.xy) * 6.2831853;
    let depth_here = view_depth(p);
    var occluded = 0.0;
    for (var i = 0u; i < count; i = i + 1u) {
        let f = (f32(i) + 0.5) / f32(count);
        // Points in the hemisphere around the normal, denser near the surface.
        let a = f32(i) * 2.3999632 + spin;
        let r = sqrt(f);
        let h = sqrt(1.0 - f);
        let dir = t1 * (cos(a) * r) + t2 * (sin(a) * r) + n * h;
        let scale = radius * mix(0.15, 1.0, f * f);
        let sp = p + dir * scale;
        let clip = post.view_proj * vec4<f32>(sp, 1.0);
        let suv = uv_of_ndc(clip.xy / clip.w);
        if (suv.x < 0.0 || suv.y < 0.0 || suv.x > 1.0 || suv.y > 1.0) {
            continue;
        }
        let sd = depth_at(suv);
        if (is_sky(sd)) {
            continue;
        }
        let scene = view_depth(world_at(suv, sd));
        if (scene < view_depth(sp) - 0.02 * radius) {
            occluded = occluded + smooth01(radius / max(abs(depth_here - scene), 1e-4));
        }
    }
    let ao = 1.0 - occluded / f32(count);
    return vec4<f32>(ao, ao, ao, 1.0);
}

@fragment
fn fs_ao_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let t = pass_info.a.xy;
    var sum = 0.0;
    for (var y = -1.5; y <= 1.5; y = y + 1.0) {
        for (var x = -1.5; x <= 1.5; x = x + 1.0) {
            sum = sum + sample_a(uv + vec2<f32>(x, y) * t).r;
        }
    }
    let ao = sum / 16.0;
    return vec4<f32>(ao, ao, ao, 1.0);
}

@fragment
fn fs_ao_apply(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let c = sample_a(uv);
    let ao = sample_b(uv).r;
    let s = params_at(uv);
    let k = pow(clamp(ao, 0.0, 1.0), max(s.ambient.y, 0.0));
    return vec4<f32>(c.rgb * k, c.a);
}

// ---------------------------------------------------------- Depth of field

fn coc_px(s: Params, dist: f32) -> f32 {
    let range = max(s.focus.y, 0.01);
    let out_of_focus = clamp((abs(dist - s.focus.x) - range * 0.5) / range, 0.0, 1.0);
    return clamp(s.focus.z, 0.0, 1.0) * out_of_focus * post.info.w;
}

@fragment
fn fs_dof(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let c = sample_a(uv);
    let s = params_at(uv);
    let d = depth_at(uv);
    var dist = 1.0e6;
    if (!is_sky(d)) {
        dist = view_depth(world_at(uv, d));
    }
    let radius = coc_px(s, dist);
    if (radius < 0.5) {
        return c;
    }
    var sum = c.rgb;
    var weight = 1.0;
    let taps = 24u;
    for (var i = 0u; i < taps; i = i + 1u) {
        let f = (f32(i) + 0.5) / f32(taps);
        let a = f32(i) * 2.3999632;
        let r = sqrt(f) * radius;
        let o = vec2<f32>(cos(a), sin(a)) * r * post.screen.zw;
        let tuv = uv + o;
        let td = depth_at(tuv);
        var tdist = 1.0e6;
        if (!is_sky(td)) {
            tdist = view_depth(world_at(tuv, td));
        }
        // Sharp things in front don't smear into the blurred background.
        let tap_radius = coc_px(s, tdist);
        let w = clamp(tap_radius - r + 1.0, 0.0, 1.0);
        sum = sum + sample_a(tuv).rgb * w;
        weight = weight + w;
    }
    return vec4<f32>(sum / weight, c.a);
}

// ------------------------------------------------------------------- Glow

@fragment
fn fs_bloom_prefilter(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let t = pass_info.a.xy;
    var c = (sample_a(uv + vec2<f32>(-t.x, -t.y)) + sample_a(uv + vec2<f32>(t.x, -t.y))
        + sample_a(uv + vec2<f32>(-t.x, t.y)) + sample_a(uv + vec2<f32>(t.x, t.y))).rgb * 0.25;
    c = min(c, vec3<f32>(64.0));
    let s = params_at(uv);
    let threshold = max(s.glow.x, 0.0);
    let knee = threshold * 0.5 + 1e-4;
    let lum = max(c.r, max(c.g, c.b));
    let soft = clamp(lum - threshold + knee, 0.0, 2.0 * knee);
    let contribution = max(soft * soft / (4.0 * knee), lum - threshold) / max(lum, 1e-4);
    return vec4<f32>(c * contribution * max(s.glow.y, 0.0), 1.0);
}

@fragment
fn fs_bloom_down(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let t = pass_info.a.xy;
    let c = sample_a(uv) * 4.0 + sample_a(uv + vec2<f32>(-t.x, -t.y)) + sample_a(uv + vec2<f32>(t.x, -t.y))
        + sample_a(uv + vec2<f32>(-t.x, t.y)) + sample_a(uv + vec2<f32>(t.x, t.y));
    return vec4<f32>(c.rgb / 8.0, 1.0);
}

@fragment
fn fs_bloom_up(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    // Tent filter of the smaller level, added to this level.
    let t = pass_info.a.xy;
    var up = sample_a(uv) * 4.0;
    up = up + (sample_a(uv + vec2<f32>(-t.x, 0.0)) + sample_a(uv + vec2<f32>(t.x, 0.0))
        + sample_a(uv + vec2<f32>(0.0, -t.y)) + sample_a(uv + vec2<f32>(0.0, t.y))) * 2.0;
    up = up + sample_a(uv + vec2<f32>(-t.x, -t.y)) + sample_a(uv + vec2<f32>(t.x, -t.y))
        + sample_a(uv + vec2<f32>(-t.x, t.y)) + sample_a(uv + vec2<f32>(t.x, t.y));
    let spread = mix(0.35, 1.0, clamp(post.base.glow.z, 0.0, 1.0));
    return vec4<f32>(sample_b(uv).rgb + up.rgb / 16.0 * spread, 1.0);
}

// --------------------------------------- Exposure, tone mapping, grading

fn filmic(x: vec3<f32>) -> vec3<f32> {
    // ACES approximation (Narkowicz).
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn grade(c_in: vec3<f32>, s: Params) -> vec3<f32> {
    var c = c_in;
    let temp = s.color.z;
    c = c * vec3<f32>(1.0 + 0.25 * temp, 1.0 + 0.02 * temp, 1.0 - 0.25 * temp);
    // Contrast around middle grey, in a perceptual (square-root) space.
    var g = sqrt(max(c, vec3<f32>(0.0)));
    g = (g - vec3<f32>(0.5)) * s.color.y + vec3<f32>(0.5);
    c = max(g, vec3<f32>(0.0));
    c = c * c;
    let l = dot(c, LUMA);
    c = mix(vec3<f32>(l), c, s.color.x);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_final(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    var c = sample_a(uv).rgb;
    if (pass_info.a.z > 0.5) {
        // Bloom (sum of all levels, normalized).
        c = c + sample_b(uv).rgb * pass_info.a.w;
    }
    let s = params_at(uv);
    c = c * exp2(s.exposure.x);
    if (s.exposure.y > 0.5) {
        c = filmic(c);
    } else {
        c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(grade(c, s), 1.0);
}

// ----------------------------------------------------- FXAA and sharpening

fn luma(c: vec3<f32>) -> f32 {
    return sqrt(dot(c, LUMA));
}

@fragment
fn fs_image(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let t = post.screen.zw;
    let s = params_at(uv);
    let m = sample_a(uv).rgb;
    let nw = sample_a(uv + vec2<f32>(-t.x, -t.y)).rgb;
    let ne = sample_a(uv + vec2<f32>(t.x, -t.y)).rgb;
    let sw = sample_a(uv + vec2<f32>(-t.x, t.y)).rgb;
    let se = sample_a(uv + vec2<f32>(t.x, t.y)).rgb;
    let l_nw = luma(nw);
    let l_ne = luma(ne);
    let l_sw = luma(sw);
    let l_se = luma(se);
    let l_m = luma(m);
    let l_min = min(l_m, min(min(l_nw, l_ne), min(l_sw, l_se)));
    let l_max = max(l_m, max(max(l_nw, l_ne), max(l_sw, l_se)));
    var dir = vec2<f32>(-((l_nw + l_ne) - (l_sw + l_se)), (l_nw + l_sw) - (l_ne + l_se));
    let reduce = max((l_nw + l_ne + l_sw + l_se) * 0.25 * 0.125, 1.0 / 128.0);
    let rcp = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * rcp, vec2<f32>(-8.0), vec2<f32>(8.0)) * t;
    let a = 0.5 * (sample_a(uv + dir * (1.0 / 3.0 - 0.5)).rgb + sample_a(uv + dir * (2.0 / 3.0 - 0.5)).rgb);
    let b = a * 0.5 + 0.25 * (sample_a(uv - dir * 0.5).rgb + sample_a(uv + dir * 0.5).rgb);
    let l_b = luma(b);
    var aa = b;
    if (l_b < l_min || l_b > l_max) {
        aa = a;
    }
    var c = mix(m, aa, clamp(s.image.x, 0.0, 1.0));
    // Unsharp mask from the four direct neighbors.
    let around = (sample_a(uv + vec2<f32>(t.x, 0.0)).rgb + sample_a(uv - vec2<f32>(t.x, 0.0)).rgb
        + sample_a(uv + vec2<f32>(0.0, t.y)).rgb + sample_a(uv - vec2<f32>(0.0, t.y)).rgb) * 0.25;
    c = c + (m - around) * clamp(s.image.y, 0.0, 1.0) * 0.8;
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

// ----------------------------------------------------------------- Debug

// The scene's view depth at each pixel, divided by `pass.a.w` (tests).
@fragment
fn fs_debug_depth(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = uv_of(pos);
    let d = depth_at(uv);
    if (is_sky(d)) {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }
    let v = view_depth(world_at(uv, d)) / pass_info.a.w;
    return vec4<f32>(v, v, v, 1.0);
}
