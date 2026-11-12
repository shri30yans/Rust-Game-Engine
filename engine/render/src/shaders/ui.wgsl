// UI: one shader for every 2D primitive. Shapes are signed distance fields in a
// local (unskewed, untilted) space; perspective tilt comes in via clip-space w.
struct U {
    gamma: vec4<f32>,
    grad_angle: f32,
    grad_count: f32,
    grad_pad: f32,
    grad_pad2: f32,
    grad_stop0: vec4<f32>,
    grad_stop1: vec4<f32>,
    grad_stop2: vec4<f32>,
    grad_stop3: vec4<f32>,
}
@group(0) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(1) var glyph_samp: sampler;
@group(0) @binding(2) var<uniform> u: U;

struct VIn {
    @location(0) pos: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) local: vec2<f32>,
    @location(3) half_size: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) p: vec4<f32>, // radius, border, mode, aux
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) p: vec4<f32>,
};

@vertex
fn vs_ui(v: VIn) -> VOut {
    var o: VOut;
    o.clip = v.pos;
    o.uv = v.uv;
    o.local = v.local;
    o.half_size = v.half_size;
    // Colours arrive as sRGB; blend in linear.
    o.color = vec4<f32>(pow(v.color.rgb, vec3<f32>(2.2)), v.color.a);
    o.p = v.p;
    return o;
}

fn sd_round_rect(p: vec2<f32>, h: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - h + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn grad_color(t: f32) -> vec3<f32> {
    let count = i32(u.grad_count + 0.5);
    if (count < 2) {
        return u.grad_stop0.rgb;
    }
    let stops = array<vec4<f32>, 4>(u.grad_stop0, u.grad_stop1, u.grad_stop2, u.grad_stop3);
    let t_clamped = clamp(t, stops[0].x, stops[count - 1].x);
    for (var i: i32 = 0; i < count - 1; i = i + 1) {
        let a = stops[i];
        let b = stops[i + 1];
        if (t_clamped >= a.x && t_clamped <= b.x) {
            let span = max(b.x - a.x, 1e-5);
            let f = (t_clamped - a.x) / span;
            return mix(vec3<f32>(a.y, a.z, a.w), vec3<f32>(b.y, b.z, b.w), f);
        }
    }
    return stops[count - 1].rgb;
}

@fragment
fn fs_ui(i: VOut) -> @location(0) vec4<f32> {
    let tex = textureSampleLevel(glyph_atlas, glyph_samp, i.uv, 0.0).r;
    let fw = max(length(fwidth(i.local)), 1e-4);
    let mode = u32(i.p.z + 0.5);
    var a = 1.0;
    var rgb = i.color.rgb;
    let d = sd_round_rect(i.local, i.half_size, i.p.x);
    if (mode == 0u) {          // filled rounded rect
        a = clamp(0.5 - d / fw, 0.0, 1.0);
    } else if (mode == 1u) {   // text glyph coverage
        a = tex;
    } else if (mode == 2u) {   // stroke
        let s = abs(d + i.p.y * 0.5) - i.p.y * 0.5;
        a = clamp(0.5 - s / fw, 0.0, 1.0);
    } else if (mode == 3u) {   // ring / arc: half_size.x = outer radius, p.y = thickness, p.w = fill 0..1
        let r = length(i.local);
        let s = abs(r - (i.half_size.x - i.p.y * 0.5)) - i.p.y * 0.5;
        a = clamp(0.5 - s / fw, 0.0, 1.0);
        let ang = fract(atan2(i.local.x, -i.local.y) / 6.2831853 + 1.0);
        a *= step(ang, i.p.w);
    } else if (mode == 4u) {   // soft glow / drop shadow, p.w = blur radius
        a = 1.0 - smoothstep(-i.p.w, i.p.w, d);
        a = a * a;
    } else if (mode == 5u) {   // linear gradient fill
        a = clamp(0.5 - d / fw, 0.0, 1.0);
        let ang = radians(u.grad_angle);
        let axis = vec2<f32>(sin(ang), -cos(ang));
        let t = dot(i.local, axis) / (dot(i.half_size, abs(axis)) + 1e-4);
        let tn = t * 0.5 + 0.5;
        rgb = grad_color(tn);
    }
    let al = a * i.color.a;
    var c = rgb;
    if (u.gamma.x > 0.5) { c = pow(c, vec3<f32>(1.0 / 2.2)); }
    return vec4<f32>(c * al, al);
}
