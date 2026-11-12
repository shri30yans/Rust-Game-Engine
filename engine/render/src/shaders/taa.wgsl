// TAA resolve. Blends the jittered HDR frame with reprojected history.
// History is clipped to the current 3x3 neighbourhood in YCoCg (variance clip) and dropped
// where a surface moved in a way the neighbourhood does not explain (disocclusion).
// Sky pixels reproject with the camera alone. Blend uses Karis-style luminance weighting
// so bright aliased edges do not dominate the average.
// Cost (H10): history is a 5-tap Catmull-Rom that lets the bilinear filter do the rest of the
// 16-tap kernel (Jimenez, "Filmic SMAA", 2016), and the closest-depth search reads the centre
// and the four diagonals (a cross pattern) instead of all nine pixels.

struct Light { pos_r: vec4<f32>, col_i: vec4<f32> };

// Must match the Rust `Globals` layout (the first fields are shared with scene.wgsl).
struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    vm_proj: mat4x4<f32>,
    cam_world: mat4x4<f32>,
    cascade: array<mat4x4<f32>, 2>,
    cam_pos: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground: vec4<f32>,
    fog: vec4<f32>,
    fog_color: vec4<f32>,
    shadow: vec4<f32>,
    misc: vec4<f32>,
    tiles: vec4<u32>,
    lights: array<Light, 32>,
    unjit_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    taa: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var hdr: texture_2d<f32>;
@group(1) @binding(1) var hist: texture_2d<f32>;
@group(1) @binding(2) var vel: texture_2d<f32>;
@group(1) @binding(3) var depth: texture_depth_2d;
@group(1) @binding(4) var lin: sampler;

// Blend weight of the current frame for fully valid history. Lower is smoother.
const BLEND: f32 = 0.1;
// Blend weight when history was clipped hard (keeps ghosts from lingering).
const CLIP_BLEND: f32 = 1.0;
// Disocclusion threshold: own-pixel motion vs closest-pixel motion, in pixels.
const REJECT_PX: f32 = 0.5;
// Variance clip box half-width in standard deviations.
const GAMMA: f32 = 1.0;

struct FsIn { @builtin(position) pos: vec4<f32> };

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FsIn {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var o: FsIn;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return o;
}

fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b,
    );
}

fn ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let t = c.x - c.z;
    return vec3<f32>(t + c.y, c.x + c.z, t - c.y);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn karis_in(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + luma(c));
}

fn karis_out(c: vec3<f32>) -> vec3<f32> {
    return c / max(1.0 - luma(c), 1e-4);
}

// One bilinear history tap, clamped half a texel inside the active region so the filter
// never reads pixels outside what the last frame rendered.
fn history_tap(st: vec2<f32>, inv_dims: vec2<f32>, active_dims: vec2<i32>) -> vec3<f32> {
    let clamped = clamp(st, vec2<f32>(0.5), vec2<f32>(active_dims) - vec2<f32>(0.5));
    return textureSampleLevel(hist, lin, clamped * inv_dims, 0.0).rgb;
}

// Catmull-Rom history sample at a uv in [0, 1] of the full texture, in five bilinear taps.
// The middle two weights of each axis are merged into one tap placed between their texels; the
// four corner taps carry almost no weight and are dropped, and the result is renormalised.
fn sample_history(uv: vec2<f32>, dims: vec2<i32>, active_dims: vec2<i32>) -> vec3<f32> {
    let size = vec2<f32>(dims);
    let inv = 1.0 / size;
    let st = uv * size;
    let t1 = floor(st - 0.5) + 0.5;
    let f = st - t1;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = t1 - 1.0;
    let t3 = t1 + 2.0;
    let t12 = t1 + w2 / w12;

    let a = w12.x * w0.y;
    let b = w0.x * w12.y;
    let c = w12.x * w12.y;
    let d = w3.x * w12.y;
    let e = w12.x * w3.y;
    let acc = a * history_tap(vec2<f32>(t12.x, t0.y), inv, active_dims)
        + b * history_tap(vec2<f32>(t0.x, t12.y), inv, active_dims)
        + c * history_tap(t12, inv, active_dims)
        + d * history_tap(vec2<f32>(t3.x, t12.y), inv, active_dims)
        + e * history_tap(vec2<f32>(t12.x, t3.y), inv, active_dims);
    return acc / (a + b + c + d + e);
}

@fragment
fn fs_taa(fi: FsIn) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(hdr));
    let active_dims = max(vec2<i32>(ceil(vec2<f32>(dims) * g.taa.y)), vec2<i32>(1));
    let size = vec2<f32>(active_dims);
    let p = clamp(vec2<i32>(fi.pos.xy), vec2<i32>(0), active_dims - vec2<i32>(1));
    let uv = (vec2<f32>(p) + 0.5) / size;
    let surface_uv = uv * g.taa.y;

    // 3x3 neighbourhood: colour statistics in YCoCg, and the closest depth pixel (cross pattern).
    var mn = vec3<f32>(1e30);
    var mx = vec3<f32>(-1e30);
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    var best_d = 2.0;
    var best_q = p;
    for (var j = -1; j <= 1; j = j + 1) {
        for (var i = -1; i <= 1; i = i + 1) {
            let q = clamp(p + vec2<i32>(i, j), vec2<i32>(0), dims - vec2<i32>(1));
            let c = rgb_to_ycocg(max(textureLoad(hdr, q, 0).rgb, vec3<f32>(0.0)));
            mn = min(mn, c);
            mx = max(mx, c);
            m1 = m1 + c;
            m2 = m2 + c * c;
            // Closest depth over the centre and the diagonals only.
            if (i * j != 0 || (i == 0 && j == 0)) {
                let d = textureLoad(depth, q, 0);
                if (d < best_d) {
                    best_d = d;
                    best_q = q;
                }
            }
        }
    }
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3<f32>(0.0))) * GAMMA;

    // Motion: the closest pixel decides where history came from (foreground beats background).
    let own_vel = textureLoad(vel, p, 0).xy;
    let closest_vel = textureLoad(vel, best_q, 0).xy;
    var prev_uv: vec2<f32>;
    if (best_d >= 1.0) {
        // Sky: no motion written, so reproject the view ray at a large distance.
        let ndc = vec2<f32>(surface_uv.x * 2.0 - 1.0, 1.0 - surface_uv.y * 2.0);
        let far = g.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
        let dir = normalize(far.xyz / far.w - g.cam_pos.xyz);
        let pc = g.prev_view_proj * vec4<f32>(g.cam_pos.xyz + dir * 1000.0, 1.0);
        prev_uv = vec2<f32>(pc.x / pc.w * 0.5 + 0.5, 0.5 - pc.y / pc.w * 0.5);
    } else {
        prev_uv = surface_uv - closest_vel;
    }
    // Disocclusion: this pixel moved differently from the surface in front of it.
    let rejected = best_d < 1.0 && length((own_vel - closest_vel) * size * g.taa.y) > REJECT_PX;
    let off_screen = any(prev_uv < vec2<f32>(0.0)) || any(prev_uv > vec2<f32>(1.0));

    let cur_rgb = max(textureLoad(hdr, p, 0).rgb, vec3<f32>(0.0));
    let cur_ycc = rgb_to_ycocg(cur_rgb);

    var blend = BLEND;
    var hist_ycc = cur_ycc;
    if (g.taa.x < 0.5 || off_screen || rejected) {
        blend = CLIP_BLEND;
    } else {
        let prev_scale = max(g.taa.z, 1e-5);
        let history_uv = prev_uv / prev_scale;
        let history_dims = max(vec2<i32>(ceil(vec2<f32>(dims) * prev_scale)), vec2<i32>(1));
        let hist_rgb = max(sample_history(history_uv, dims, history_dims), vec3<f32>(0.0));
        let raw = rgb_to_ycocg(hist_rgb);
        let lo = max(mn, mean - sigma);
        let hi = min(mx, mean + sigma);
        let clipped = clamp(raw, lo, hi);
        // The further history had to be pulled back, the less it is trusted.
        let pulled = length(raw - clipped) / max(length(sigma) + 1e-3, 1e-3);
        blend = mix(BLEND, CLIP_BLEND, clamp(pulled, 0.0, 1.0));
        hist_ycc = clipped;
    }

    let hist_c = karis_in(ycocg_to_rgb(hist_ycc));
    let cur_c = karis_in(cur_rgb);
    let out_rgb = karis_out(mix(hist_c, cur_c, blend));
    return vec4<f32>(max(out_rgb, vec3<f32>(0.0)), 1.0);
}
