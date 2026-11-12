// Shadow depth pass consumes the position stream and retained instance indices only.
struct SU { m: mat4x4<f32> };
@group(0) @binding(0) var<uniform> su: SU;

struct Instance { m0: vec4<f32>, m1: vec4<f32>, m2: vec4<f32>, tint: u32, emissive: u32, material0: u32, material1: u32 };
@group(1) @binding(0) var<storage, read> instances: array<Instance>;
@group(1) @binding(1) var<storage, read> visible_indices: array<u32>;
struct MeshRecord { minimum: vec4<f32>, maximum: vec4<f32>, format: vec4<u32> };
@group(1) @binding(3) var<storage, read> mesh_records: array<MeshRecord>;

@vertex
fn vs_shadow(@location(0) pos: vec3<f32>, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    let i = instances[visible_indices[instance_index]];
    let p = vec4<f32>(pos, 1.0);
    let world = vec4<f32>(dot(i.m0, p), dot(i.m1, p), dot(i.m2, p), 1.0);
    return su.m * world;
}

@vertex
fn vs_shadow_cooked(@location(0) pos: vec3<f32>, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    let i = instances[visible_indices[instance_index]];
    let p = vec4<f32>(pos, 1.0);
    let world = vec4<f32>(dot(i.m0, p), dot(i.m1, p), dot(i.m2, p), 1.0);
    return su.m * world;
}

@vertex
fn vs_shadow_quantized(@location(0) encoded: vec4<u32>, @builtin(instance_index) instance_index: u32) -> @builtin(position) vec4<f32> {
    let slot = visible_indices[instance_index];
    let i = instances[slot];
    let mesh = mesh_records[i.material1];
    let pos = mesh.minimum.xyz + vec3<f32>(encoded.xyz) / 65535.0 * (mesh.maximum.xyz - mesh.minimum.xyz);
    let p = vec4<f32>(pos, 1.0);
    let world = vec4<f32>(dot(i.m0, p), dot(i.m1, p), dot(i.m2, p), 1.0);
    return su.m * world;
}
