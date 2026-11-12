// Scene: PBR forward shading into an HDR target, with cascaded sun shadows,
// point lights, height fog, an analytic sky and procedural world-space materials.

struct Light { pos_r: vec4<f32>, col_i: vec4<f32> };

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    vm_proj: mat4x4<f32>,
    cam_world: mat4x4<f32>,
    cascade: array<mat4x4<f32>, 2>,
    cam_pos: vec4<f32>,      // w = time
    sun_dir: vec4<f32>,      // towards the sun, w = intensity
    sun_color: vec4<f32>,
    sky_zenith: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground: vec4<f32>,       // w = ambient intensity
    fog: vec4<f32>,          // density, height falloff, base height, in-scatter
    fog_color: vec4<f32>,
    shadow: vec4<f32>,       // texel size, split0, split1, pcf
    misc: vec4<f32>,         // light count, cloud amount, world texel c0, world texel c1
    tiles: vec4<u32>,        // tiles x, tiles y, tile size, 0
    lights: array<Light, 32>,
    unjit_view_proj: mat4x4<f32>, // view_proj without the TAA jitter (velocity, sky)
    prev_view_proj: mat4x4<f32>,  // last frame's unjittered view_proj
    taa: vec4<f32>,               // x = 1 when the TAA history is valid
};

@group(0) @binding(0) var<uniform> g: Globals;
struct MeshRecord { minimum: vec4<f32>, maximum: vec4<f32>, format: vec4<u32> };
@group(0) @binding(1) var<storage, read> instances: array<Instance>;
@group(0) @binding(2) var<storage, read> visible_indices: array<u32>;
@group(0) @binding(3) var<storage, read> previous_transforms: array<PreviousTransform>;
@group(0) @binding(4) var<storage, read> mesh_records: array<MeshRecord>;
@group(0) @binding(5) var<storage, read> tile_masks: array<u32>;
@group(0) @binding(6) var shadow_map: texture_depth_2d_array;
@group(0) @binding(7) var shadow_samp: sampler_comparison;
@group(0) @binding(8) var ao_tex: texture_2d<f32>;
@group(0) @binding(9) var ao_samp: sampler;

struct Instance {
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
    tint: u32,
    emissive: u32,
    material0: u32,
    material1: u32,
};

struct PreviousTransform { m0: vec4<f32>, m1: vec4<f32>, m2: vec4<f32> };
struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal_oct: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) mat: vec4<f32>,
};

struct CookedIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal_oct: vec2<f32>,
    @location(2) tangent: u32,
    @location(3) uv: vec2<u32>,
};

struct QuantizedIn {
    @location(0) pos: vec4<u32>,
    @location(1) normal_oct: vec2<f32>,
    @location(2) tangent: u32,
    @location(3) uv: vec2<u32>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    factors: vec4<f32>,
    alpha: vec4<f32>,
};
@group(1) @binding(0) var<uniform> material: MaterialUniform;
@group(1) @binding(1) var base_color_tex: texture_2d<f32>;
@group(1) @binding(2) var normal_tex: texture_2d<f32>;
@group(1) @binding(3) var orm_tex: texture_2d<f32>;
@group(1) @binding(4) var emissive_tex: texture_2d<f32>;
@group(1) @binding(5) var material_samp: sampler;

struct VOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) n: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) mat: vec4<f32>,
    @location(4) tint: vec4<f32>,
    @location(5) extra: vec4<f32>,
    @location(6) ppos: vec3<f32>,
    @location(7) cur: vec4<f32>,
    @location(8) prev: vec4<f32>,
    @location(9) tangent: vec4<f32>,
    @location(10) uv: vec2<f32>,
};

struct DepthOut {
    @invariant @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) cur: vec4<f32>,
    @location(2) prev: vec4<f32>,
};

fn unpack_bytes(v: u32) -> vec4<f32> { return unpack4x8unorm(v); }
fn transform_point(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, p: vec3<f32>) -> vec4<f32> {
    let q = vec4<f32>(p, 1.0);
    return vec4<f32>(dot(r0, q), dot(r1, q), dot(r2, q), 1.0);
}
fn decode_quantized_position(encoded: vec4<u32>, mesh: MeshRecord) -> vec3<f32> {
    let unit = vec3<f32>(encoded.xyz) / 65535.0;
    return mesh.minimum.xyz + unit * (mesh.maximum.xyz - mesh.minimum.xyz);
}
fn opaque_clip_position(instance_slot: u32, pos: vec3<f32>) -> vec4<f32> {
    let i = instances[instance_slot];
    let world = transform_point(i.m0, i.m1, i.m2, pos);
    return g.view_proj * world;
}
fn decode_normal(oct: vec2<f32>) -> vec3<f32> {
    var n = vec3<f32>(oct, 1.0 - abs(oct.x) - abs(oct.y));
    if (n.z < 0.0) {
        let old_x = n.x;
        n.x = (1.0 - abs(n.y)) * sign(old_x);
        n.y = (1.0 - abs(old_x)) * sign(n.y);
    }
    return normalize(n);
}
fn decode_tangent(packed: u32) -> vec4<f32> {
    let x = f32(packed & 0x7fffu) / 32767.0 * 2.0 - 1.0;
    let y = f32((packed >> 15u) & 0x7fffu) / 32767.0 * 2.0 - 1.0;
    var tangent = vec3<f32>(x, y, 1.0 - abs(x) - abs(y));
    if tangent.z < 0.0 {
        let old_x = tangent.x;
        tangent.x = (1.0 - abs(tangent.y)) * sign(old_x);
        tangent.y = (1.0 - abs(old_x)) * sign(tangent.y);
    }
    return vec4<f32>(normalize(tangent), select(1.0, -1.0, ((packed >> 30u) & 1u) != 0u));
}
fn unpack_uv(encoded: vec2<u32>) -> vec2<f32> {
    return unpack2x16float(encoded.x | (encoded.y << 16u));
}
fn normal_of(v: VIn, i: Instance) -> vec3<f32> {
    let c0 = vec3<f32>(i.m0.x, i.m1.x, i.m2.x) / max(dot(vec3<f32>(i.m0.x, i.m1.x, i.m2.x), vec3<f32>(i.m0.x, i.m1.x, i.m2.x)), 1e-8);
    let c1 = vec3<f32>(i.m0.y, i.m1.y, i.m2.y) / max(dot(vec3<f32>(i.m0.y, i.m1.y, i.m2.y), vec3<f32>(i.m0.y, i.m1.y, i.m2.y)), 1e-8);
    let c2 = vec3<f32>(i.m0.z, i.m1.z, i.m2.z) / max(dot(vec3<f32>(i.m0.z, i.m1.z, i.m2.z), vec3<f32>(i.m0.z, i.m1.z, i.m2.z)), 1e-8);
    return normalize(mat3x3<f32>(c0, c1, c2) * decode_normal(v.normal_oct));
}
fn pattern_pos(v: VIn, i: Instance, wpos: vec3<f32>) -> vec3<f32> {
    let flags = (i.material0 >> 24u) & 255u;
    if ((flags & 1u) != 0u) {
        return v.pos * vec3<f32>(length(vec3<f32>(i.m0.x, i.m1.x, i.m2.x)), length(vec3<f32>(i.m0.y, i.m1.y, i.m2.y)), length(vec3<f32>(i.m0.z, i.m1.z, i.m2.z)));
    }
    return wpos;
}

fn screen_uv(clip: vec4<f32>) -> vec2<f32> {
    let ndc = clip.xy / clip.w;
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

fn ssao_at(clip: vec4<f32>) -> f32 {
    let dims = vec2<f32>(textureDimensions(ao_tex));
    let half_texel = 0.5 / dims;
    let uv = screen_uv(clip) * g.taa.y;
    return textureSampleLevel(ao_tex, ao_samp, clamp(uv, half_texel, vec2<f32>(g.taa.y) - half_texel), 0.0).r;
}

fn fill(v: VIn, i: Instance, wpos: vec3<f32>, n: vec3<f32>) -> VOut {
    var o: VOut;
    let material0 = unpack_bytes(i.material0);
    o.wpos = wpos;
    o.n = n;
    o.color = v.color.rgb;
    o.mat = v.mat;
    o.tint = unpack_bytes(i.tint);
    o.extra = vec4<f32>(
        select(-1.0, round(material0.x * 255.0), material0.x < 1.0),
        select(-1.0, material0.y, material0.y < 1.0),
        select(-1.0, material0.z, material0.z < 1.0),
        material0.w * 255.0,
    );
    o.tint.a = unpack2x16float(i.emissive).x;
    o.ppos = pattern_pos(v, i, wpos);
    o.tangent = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    o.uv = vec2<f32>(0.0);
    return o;
}

@vertex
fn vs_main(v: VIn, @builtin(instance_index) instance_index: u32) -> VOut {
    let slot = visible_indices[instance_index];
    let i = instances[slot];
    let w = transform_point(i.m0, i.m1, i.m2, v.pos);
    var o = fill(v, i, w.xyz, normal_of(v, i));
    o.clip = opaque_clip_position(slot, v.pos);
    o.cur = g.unjit_view_proj * w;
    o.prev = g.prev_view_proj * w;
    return o;
}

fn cooked_vertex(pos: vec3<f32>, normal_oct: vec2<f32>, packed_tangent: u32, uv: vec2<u32>, instance_index: u32) -> VOut {
    let slot = visible_indices[instance_index];
    let i = instances[slot];
    let w = transform_point(i.m0, i.m1, i.m2, pos);
    var v: VIn;
    v.pos = pos;
    v.normal_oct = normal_oct;
    v.color = vec4<f32>(1.0);
    v.mat = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    var o = fill(v, i, w.xyz, normal_of(v, i));
    let decoded_tangent = decode_tangent(packed_tangent);
    let c0 = vec3<f32>(i.m0.x, i.m1.x, i.m2.x);
    let c1 = vec3<f32>(i.m0.y, i.m1.y, i.m2.y);
    let c2 = vec3<f32>(i.m0.z, i.m1.z, i.m2.z);
    let world_tangent = normalize(mat3x3<f32>(c0, c1, c2) * decoded_tangent.xyz);
    o.tangent = vec4<f32>(world_tangent, decoded_tangent.w);
    o.uv = unpack_uv(uv);
    o.clip = opaque_clip_position(slot, pos);
    o.cur = g.unjit_view_proj * w;
    o.prev = g.prev_view_proj * w;
    return o;
}

@vertex
fn vs_cooked(v: CookedIn, @builtin(instance_index) instance_index: u32) -> VOut {
    return cooked_vertex(v.pos, v.normal_oct, v.tangent, v.uv, instance_index);
}

@vertex
fn vs_cooked_quantized(v: QuantizedIn, @builtin(instance_index) instance_index: u32) -> VOut {
    let slot = visible_indices[instance_index];
    let mesh = mesh_records[instances[slot].material1];
    let pos = decode_quantized_position(v.pos, mesh);
    return cooked_vertex(pos, v.normal_oct, v.tangent, v.uv, instance_index);
}

fn cooked_vm_vertex(pos: vec3<f32>, normal_oct: vec2<f32>, packed_tangent: u32, uv: vec2<u32>, instance_index: u32) -> VOut {
    let i = instances[visible_indices[instance_index]];
    let vp = transform_point(i.m0, i.m1, i.m2, pos);
    let w = g.cam_world * vp;
    var input: VIn;
    input.pos = pos;
    input.normal_oct = normal_oct;
    input.color = vec4<f32>(1.0);
    input.mat = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    let n = (g.cam_world * vec4<f32>(normal_of(input, i), 0.0)).xyz;
    var o = fill(input, i, w.xyz, normalize(n));
    let decoded_tangent = decode_tangent(packed_tangent);
    let c0 = vec3<f32>(i.m0.x, i.m1.x, i.m2.x);
    let c1 = vec3<f32>(i.m0.y, i.m1.y, i.m2.y);
    let c2 = vec3<f32>(i.m0.z, i.m1.z, i.m2.z);
    let model_tangent = normalize(mat3x3<f32>(c0, c1, c2) * decoded_tangent.xyz);
    let camera_tangent = normalize((g.cam_world * vec4<f32>(model_tangent, 0.0)).xyz);
    o.tangent = vec4<f32>(camera_tangent, decoded_tangent.w);
    o.uv = unpack_uv(uv);
    var c = g.vm_proj * vp;
    c.z = c.z * 0.04;
    o.clip = c;
    o.cur = c;
    o.prev = c;
    return o;
}

@vertex
fn vs_vm_cooked(v: CookedIn, @builtin(instance_index) instance_index: u32) -> VOut {
    return cooked_vm_vertex(v.pos, v.normal_oct, v.tangent, v.uv, instance_index);
}

@vertex
fn vs_vm_cooked_quantized(v: QuantizedIn, @builtin(instance_index) instance_index: u32) -> VOut {
    let slot = visible_indices[instance_index];
    let mesh = mesh_records[instances[slot].material1];
    let unit = vec3<f32>(v.pos.xyz) / 65535.0;
    let pos = mesh.minimum.xyz + unit * (mesh.maximum.xyz - mesh.minimum.xyz);
    return cooked_vm_vertex(pos, v.normal_oct, v.tangent, v.uv, instance_index);
}

fn prepass_vertex(pos: vec3<f32>, instance_index: u32, taa: bool) -> DepthOut {
    let slot = visible_indices[instance_index];
    let i = instances[slot];
    let w = transform_point(i.m0, i.m1, i.m2, pos);
    var o: DepthOut;
    o.clip = opaque_clip_position(slot, pos);
    o.wpos = w.xyz;
    o.cur = g.unjit_view_proj * w;
    o.prev = o.cur;
    if taa {
        let previous = previous_transforms[slot];
        let pw = transform_point(previous.m0, previous.m1, previous.m2, pos);
        o.prev = g.prev_view_proj * pw;
    }
    return o;
}

@vertex
fn vs_prepass(@location(0) pos: vec3<f32>, @builtin(instance_index) instance_index: u32) -> DepthOut {
    return prepass_vertex(pos, instance_index, false);
}

@vertex
fn vs_prepass_taa(@location(0) pos: vec3<f32>, @builtin(instance_index) instance_index: u32) -> DepthOut {
    return prepass_vertex(pos, instance_index, true);
}

@vertex
fn vs_prepass_cooked(@location(0) pos: vec3<f32>, @builtin(instance_index) instance_index: u32) -> DepthOut {
    return prepass_vertex(pos, instance_index, false);
}

@vertex
fn vs_prepass_taa_cooked(@location(0) pos: vec3<f32>, @builtin(instance_index) instance_index: u32) -> DepthOut {
    return prepass_vertex(pos, instance_index, true);
}

fn prepass_quantized(encoded: vec4<u32>, instance_index: u32, taa: bool) -> DepthOut {
    let slot = visible_indices[instance_index];
    let mesh = mesh_records[instances[slot].material1];
    let pos = decode_quantized_position(encoded, mesh);
    return prepass_vertex(pos, instance_index, taa);
}

@vertex
fn vs_prepass_quantized(@location(0) encoded: vec4<u32>, @builtin(instance_index) instance_index: u32) -> DepthOut {
    return prepass_quantized(encoded, instance_index, false);
}

@vertex
fn vs_prepass_taa_quantized(@location(0) encoded: vec4<u32>, @builtin(instance_index) instance_index: u32) -> DepthOut {
    return prepass_quantized(encoded, instance_index, true);
}

// View model (weapon in hand): instances are in camera space, drawn with their own
// projection and squeezed toward the near plane so world geometry never clips them.
@vertex
fn vs_vm(v: VIn, @builtin(instance_index) instance_index: u32) -> VOut {
    let i = instances[visible_indices[instance_index]];
    let vp = transform_point(i.m0, i.m1, i.m2, v.pos);
    let w = g.cam_world * vp;
    let n = (g.cam_world * vec4<f32>(normal_of(v, i), 0.0)).xyz;
    var o = fill(v, i, w.xyz, normalize(n));
    var c = g.vm_proj * vp;
    c.z = c.z * 0.04;
    o.clip = c;
    o.cur = c;
    o.prev = c;
    return o;
}
