// Bilateral blur for GTAO: preserves depth and normal edges.

struct DenoiseParams {
    texel: vec4<f32>, // 1/w, 1/h, depth_sigma, normal_sigma
    viewport: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: DenoiseParams;
@group(0) @binding(1) var ao_raw: texture_2d<f32>;
@group(0) @binding(2) var depth_tex: texture_depth_2d;
@group(0) @binding(3) var normal_tex: texture_2d<f32>;
@group(0) @binding(4) var samp: sampler;

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

@fragment
fn fs_denoise(i: VsOut) -> @location(0) vec4<f32> {
    let ao_dims = vec2<f32>(textureDimensions(ao_raw));
    let depth_dims = vec2<f32>(textureDimensions(depth_tex));
    let ao_uv = clamp(i.uv * u.viewport.xy, 0.5 / ao_dims, u.viewport.xy - 0.5 / ao_dims);
    let depth_uv = clamp(i.uv * u.viewport.xy, 0.5 / depth_dims, u.viewport.xy - 0.5 / depth_dims);
    let o = textureSample(ao_raw, samp, ao_uv).r;
    let d0 = textureSample(depth_tex, samp, depth_uv);
    let n0 = textureSample(normal_tex, samp, depth_uv).xyz;
    var sum = o;
    var wsum = 1.0;
    let t = u.texel.xy;
    let k = 0.5;

    for (var oy = -1; oy <= 1; oy++) {
        for (var ox = -1; ox <= 1; ox++) {
            if (ox == 0 && oy == 0) {
                continue;
            }
            let uv = i.uv + vec2<f32>(f32(ox), f32(oy)) * t * k;
            let auv = clamp(uv * u.viewport.xy, 0.5 / ao_dims, u.viewport.xy - 0.5 / ao_dims);
            let duv = clamp(uv * u.viewport.xy, 0.5 / depth_dims, u.viewport.xy - 0.5 / depth_dims);
            let d = textureSample(depth_tex, samp, duv);
            let n = textureSample(normal_tex, samp, duv).xyz;
            let a = textureSample(ao_raw, samp, auv).r;
            let dw = exp(-abs(d - d0) / max(u.texel.z, 1e-5));
            let nw = pow(max(dot(n, n0), 0.0), u.texel.w);
            let ww = dw * nw;
            sum += a * ww;
            wsum += ww;
        }
    }

    return vec4<f32>(sum / wsum, 0.0, 0.0, 1.0);
}
