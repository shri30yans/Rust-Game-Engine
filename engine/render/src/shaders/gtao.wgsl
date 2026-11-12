// View-space ambient occlusion: slice-based horizon integration.

struct GtaoUniforms {
    inv_view_proj: mat4x4<f32>,
    texel: vec4<f32>,  // 1/width, 1/height, world_radius, 0
    params: vec4<f32>, // slice_count, steps_per_slice, intensity, min_cos
    viewport: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: GtaoUniforms;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var depth_samp: sampler;
@group(0) @binding(3) var normal_tex: texture_2d<f32>;
@group(0) @binding(4) var linear_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> VsOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var o: VsOut;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

fn clip_to_view(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0, depth, 1.0);
    let w = u.inv_view_proj * ndc;
    return w.xyz / w.w;
}

fn load_n(uv: vec2<f32>) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(normal_tex));
    let uv_scaled = uv * u.viewport.xy;
    let safe_uv = clamp(uv_scaled, 0.5 / dims, u.viewport.xy - 0.5 / dims);
    return normalize(textureSample(normal_tex, linear_samp, safe_uv).xyz * 2.0 - 1.0);
}

@fragment
fn fs_gtao(i: VsOut) -> @location(0) vec4<f32> {
    let depth_dims = vec2<f32>(textureDimensions(depth_tex));
    let depth_uv = clamp(i.uv * u.viewport.xy, 0.5 / depth_dims, u.viewport.xy - 0.5 / depth_dims);
    let depth = textureSample(depth_tex, depth_samp, depth_uv);
    if (depth >= 0.9999) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    let origin = clip_to_view(i.uv * u.viewport.xy, depth);
    var n = load_n(i.uv);
    let v = normalize(-origin);
    let slices = i32(u.params.x + 0.5);
    let steps = i32(u.params.y + 0.5);
    let min_cos = u.params.w;

    var occ = 0.0;
    let px_step = u.texel.xy * u.texel.z;

    for (var s = 0; s < slices; s++) {
        let angle = (f32(s) + 0.5) * 6.2831853 / f32(slices);
        let dir = vec2<f32>(cos(angle), sin(angle));
        var h_cos = -1.0;

        for (var k = 1; k <= steps; k++) {
            let duv = dir * px_step * f32(k);
            let suv = clamp(i.uv + vec2<f32>(duv.x, -duv.y), vec2<f32>(0.0), vec2<f32>(1.0));
            let sample_uv = clamp(suv * u.viewport.xy, 0.5 / depth_dims, u.viewport.xy - 0.5 / depth_dims);
            let sd = textureSample(depth_tex, depth_samp, sample_uv);
            if (sd >= 0.9999) {
                continue;
            }
            let sp = clip_to_view(suv * u.viewport.xy, sd);
            let d = sp - origin;
            let len2 = dot(d, d);
            if (len2 < 1e-8) {
                continue;
            }
            let fwd = d * inverseSqrt(len2);
            let cos_v = dot(fwd, v);
            if (cos_v > min_cos) {
                h_cos = max(h_cos, cos_v);
            }
        }

        var slice_occ = 0.0;
        if (h_cos > -0.999) {
            let nv = max(dot(n, v), 1e-4);
            let bent = (h_cos - nv) / (1.0 - nv);
            slice_occ = clamp(1.0 - bent, 0.0, 1.0);
        }
        occ += slice_occ;
    }

    occ /= f32(slices);
    let out_occ = clamp(1.0 - occ * u.params.z, 0.0, 1.0);
    return vec4<f32>(out_occ, 0.0, 0.0, 1.0);
}
