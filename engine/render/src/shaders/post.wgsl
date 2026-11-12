// Post: bloom (threshold + downsample chain + tent upsample) and the final composite
// (exposure, ACES, grade, vignette, grain, damage and flash overlays).
struct Post {
    a: vec4<f32>, // exposure, bloom strength, bloom threshold, vignette
    b: vec4<f32>, // grain, time, damage, flash
    c: vec4<f32>, // saturation, contrast, manual gamma (1 = encode), desaturate (death)
    shadows: vec4<f32>,    // split-tone colour for shadows (rgb), amount
    highlights: vec4<f32>, // split-tone colour for highlights (rgb), amount
    flash_color: vec4<f32>,
    d: vec4<f32>, // auto exposure on, adapt speed, middle gray key, 0
    e: vec4<f32>, // source viewport scale xy, bloom viewport scale zw
};
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Post;
@group(0) @binding(3) var bloom: texture_2d<f32>;
@group(0) @binding(4) var exposure_tex: texture_2d<f32>;
// Dedicated inputs for terminal buffer visualisation passes. Their bindings never overlap the
// ordinary composite bindings, so each mode selects a distinct pipeline rather than a uniform.
@group(0) @binding(5) var debug_color: texture_2d<f32>;
@group(0) @binding(6) var debug_depth: texture_depth_2d;
@group(0) @binding(7) var debug_shadow: texture_depth_2d_array;

struct FsIn { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FsIn {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var o: FsIn;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }

fn clamp_uv(uv: vec2<f32>, dims: vec2<f32>, scale: vec2<f32>) -> vec2<f32> {
    let half_texel = 0.5 / dims;
    return clamp(uv * scale, half_texel, scale - half_texel);
}

fn sample_src(uv: vec2<f32>) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(src));
    return textureSampleLevel(src, samp, clamp_uv(uv, dims, p.e.xy), 0.0).rgb;
}

fn debug_texel(uv: vec2<f32>, dimensions: vec2<u32>) -> vec2<i32> {
    let maximum = vec2<i32>(dimensions) - vec2<i32>(1);
    return clamp(vec2<i32>(uv * vec2<f32>(dimensions)), vec2<i32>(0), maximum);
}

// HDR includes the BaseColour and LightingOnly scene variants. It is independently tonemapped
// here instead of using the ordinary post-composite pipeline.
@fragment
fn fs_view_hdr(i: FsIn) -> @location(0) vec4<f32> {
    let dimensions = textureDimensions(debug_color);
    let color = textureLoad(debug_color, debug_texel(i.uv, dimensions), 0).rgb;
    return vec4<f32>(pow(aces(color), vec3<f32>(1.0 / 2.2)), 1.0);
}

@fragment
fn fs_view_ao(i: FsIn) -> @location(0) vec4<f32> {
    let dimensions = textureDimensions(debug_color);
    let value = textureLoad(debug_color, debug_texel(i.uv, dimensions), 0).r;
    return vec4<f32>(vec3<f32>(value), 1.0);
}

@fragment
fn fs_view_depth(i: FsIn) -> @location(0) vec4<f32> {
    let dimensions = textureDimensions(debug_depth);
    let value = textureLoad(debug_depth, debug_texel(i.uv, dimensions), 0);
    return vec4<f32>(vec3<f32>(1.0 - value), 1.0);
}

// Red is emitted only by passing opaque fragments; green samples the actual depth attachment.
@fragment
fn fs_view_opaque_coverage(i: FsIn) -> @location(0) vec4<f32> {
    let dimensions = vec2<u32>(textureDimensions(debug_color));
    let pixel = debug_texel(i.uv, dimensions);
    let marker = textureLoad(debug_color, pixel, 0).r;
    let depth = textureLoad(debug_depth, pixel, 0);
    return vec4<f32>(marker, select(0.0, 1.0, depth < 1.0), depth, 1.0);
}

// Both shadow cascades are shown side-by-side, preserving the unfiltered depth values written
// by the shadow pass and making a cascade boundary visible during diagnosis.
@fragment
fn fs_view_shadow(i: FsIn) -> @location(0) vec4<f32> {
    let dimensions = textureDimensions(debug_shadow);
    let layer: i32 = select(0, 1, i.uv.x >= 0.5);
    let uv = vec2<f32>(fract(i.uv.x * 2.0), i.uv.y);
    let value = textureLoad(debug_shadow, debug_texel(uv, dimensions), layer, 0);
    return vec4<f32>(vec3<f32>(1.0 - value), 1.0);
}

// 13-tap downsample, weighted to avoid fireflies.
fn down13(uv: vec2<f32>) -> vec3<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    let a = sample_src(uv + t * vec2<f32>(-2.0, -2.0));
    let b = sample_src(uv + t * vec2<f32>(0.0, -2.0));
    let c = sample_src(uv + t * vec2<f32>(2.0, -2.0));
    let d = sample_src(uv + t * vec2<f32>(-2.0, 0.0));
    let e = sample_src(uv);
    let f = sample_src(uv + t * vec2<f32>(2.0, 0.0));
    let g = sample_src(uv + t * vec2<f32>(-2.0, 2.0));
    let h = sample_src(uv + t * vec2<f32>(0.0, 2.0));
    let i = sample_src(uv + t * vec2<f32>(2.0, 2.0));
    let j = sample_src(uv + t * vec2<f32>(-1.0, -1.0));
    let k = sample_src(uv + t * vec2<f32>(1.0, -1.0));
    let l = sample_src(uv + t * vec2<f32>(-1.0, 1.0));
    let m = sample_src(uv + t * vec2<f32>(1.0, 1.0));
    var o = e * 0.125;
    o += (a + c + g + i) * 0.03125;
    o += (b + d + f + h) * 0.0625;
    o += (j + k + l + m) * 0.125;
    return o;
}

@fragment
fn fs_down_first(i: FsIn) -> @location(0) vec4<f32> {
    var c = down13(i.uv);
    c = min(c, vec3<f32>(64.0));
    let l = luma(c);
    let knee = p.a.z * 0.5;
    let soft = clamp(l - p.a.z + knee, 0.0, 2.0 * knee);
    let w = max(soft * soft / (4.0 * knee + 1e-4), l - p.a.z) / max(l, 1e-4);
    return vec4<f32>(c * w, 1.0);
}

@fragment
fn fs_down(i: FsIn) -> @location(0) vec4<f32> {
    return vec4<f32>(down13(i.uv), 1.0);
}

@fragment
fn fs_up(i: FsIn) -> @location(0) vec4<f32> {
    let t = 1.0 / vec2<f32>(textureDimensions(src));
    var c = sample_src(i.uv) * 4.0;
    c += sample_src(i.uv + t * vec2<f32>(-1.0, 0.0)) * 2.0;
    c += sample_src(i.uv + t * vec2<f32>(1.0, 0.0)) * 2.0;
    c += sample_src(i.uv + t * vec2<f32>(0.0, -1.0)) * 2.0;
    c += sample_src(i.uv + t * vec2<f32>(0.0, 1.0)) * 2.0;
    c += sample_src(i.uv + t * vec2<f32>(-1.0, -1.0));
    c += sample_src(i.uv + t * vec2<f32>(1.0, -1.0));
    c += sample_src(i.uv + t * vec2<f32>(-1.0, 1.0));
    c += sample_src(i.uv + t * vec2<f32>(1.0, 1.0));
    return vec4<f32>(c / 16.0, 1.0);
}

// ACES filmic fit, applied in the ACES working space.
fn aces(x: vec3<f32>) -> vec3<f32> {
    let m1 = mat3x3<f32>(
        vec3<f32>(0.59719, 0.07600, 0.02840),
        vec3<f32>(0.35458, 0.90834, 0.13383),
        vec3<f32>(0.04823, 0.01566, 0.83777));
    let m2 = mat3x3<f32>(
        vec3<f32>(1.60475, -0.10208, -0.00327),
        vec3<f32>(-0.53108, 1.10813, -0.07276),
        vec3<f32>(-0.07367, -0.00605, 1.07602));
    let v = m1 * x;
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return clamp(m2 * (a / b), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Accumulates log-average luminance into a 1×1 exposure buffer (read in composite).
@fragment
fn fs_exposure(i: FsIn) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(src));
    let steps = 16.0;
    var sum = 0.0;
    var n = 0.0;
    for (var y: i32 = 0; y < i32(steps); y++) {
        for (var x: i32 = 0; x < i32(steps); x++) {
            let uv = (vec2<f32>(f32(x), f32(y)) + 0.5) / steps;
            let c = sample_src(uv);
            sum += max(luma(c), 1e-4);
            n += 1.0;
        }
    }
    let avg = sum / n;
    let key = max(p.d.z, 0.01);
    let ev = key / max(avg, 1e-4);
    return vec4<f32>(ev, 0.0, 0.0, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(443.897, 441.423));
    let r = q + dot(q, q.yx + 19.19);
    return fract((r.x + r.y) * r.x);
}

@fragment
fn fs_composite(i: FsIn) -> @location(0) vec4<f32> {
    let src_dims = vec2<f32>(textureDimensions(src));
    let bloom_dims = vec2<f32>(textureDimensions(bloom));
    var c = textureSampleLevel(src, samp, clamp_uv(i.uv, src_dims, p.e.xy), 0.0).rgb;
    c += textureSampleLevel(bloom, samp, clamp_uv(i.uv, bloom_dims, p.e.zw), 0.0).rgb * p.a.y;
    var exp = p.a.x;
    if (p.d.x > 0.5) {
        exp *= textureSampleLevel(exposure_tex, samp, vec2<f32>(0.5, 0.5), 0.0).r;
    }
    c *= exp;
    c = aces(c);
    // Grade in display-ish space: saturation, contrast, split tone.
    let l = luma(c);
    c = mix(vec3<f32>(l), c, p.c.x * (1.0 - p.c.w));
    c = clamp((c - 0.5) * p.c.y + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
    let st = mix(p.shadows.rgb * p.shadows.w, p.highlights.rgb * p.highlights.w, smoothstep(0.1, 0.7, l));
    c = clamp(c + st * (1.0 - abs(l - 0.5) * 1.2), vec3<f32>(0.0), vec3<f32>(1.0));
    // Vignette + damage edge.
    let d = i.uv - 0.5;
    let r2 = dot(d, d);
    c *= 1.0 - p.a.w * smoothstep(0.1, 0.55, r2);
    let edge = smoothstep(0.05, 0.4, r2) * p.b.z;
    c = mix(c, vec3<f32>(0.55, 0.02, 0.02), clamp(edge, 0.0, 0.85));
    c = mix(c, p.flash_color.rgb, clamp(p.b.w, 0.0, 1.0));
    // Grain (in linear, scaled by luminance so blacks stay black).
    let n = hash(i.pos.xy + vec2<f32>(p.b.y * 61.0, p.b.y * 17.0)) - 0.5;
    c += n * p.b.x * (0.25 + l);
    c = max(c, vec3<f32>(0.0));
    if (p.c.z > 0.5) { c = pow(c, vec3<f32>(1.0 / 2.2)); }
    return vec4<f32>(c, 1.0);
}
