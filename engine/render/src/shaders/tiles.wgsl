// Tiled light culling: one thread per 16x16 tile builds a bitmask of lights touching it.

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
    unjit_view_proj: mat4x4<f32>,
    prev_view_proj: mat4x4<f32>,
    taa: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<storage, read_write> tile_masks: array<u32>;

@compute @workgroup_size(8, 8, 1)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= g.tiles.x || id.y >= g.tiles.y) { return; }
    let ts = f32(g.tiles.z);
    let dims = vec2<f32>(f32(g.tiles.x), f32(g.tiles.y)) * ts;
    let lo = vec2<f32>(id.xy) * ts;
    let hi = lo + vec2<f32>(ts);
    var mask = 0u;
    let nl = u32(g.misc.x);
    for (var k = 0u; k < nl; k = k + 1u) {
        let L = g.lights[k];
        let c = L.pos_r.xyz;
        let r = L.pos_r.w;
        var mn = vec2<f32>(1e9);
        var mx = vec2<f32>(-1e9);
        var behind = false;
        for (var corner = 0u; corner < 8u; corner = corner + 1u) {
            let o = vec3<f32>(
                select(-r, r, (corner & 1u) != 0u),
                select(-r, r, (corner & 2u) != 0u),
                select(-r, r, (corner & 4u) != 0u));
            let p = g.view_proj * vec4<f32>(c + o, 1.0);
            if (p.w <= 0.001) { behind = true; break; }
            let s = vec2<f32>(p.x / p.w * 0.5 + 0.5, 0.5 - p.y / p.w * 0.5) * dims;
            mn = min(mn, s);
            mx = max(mx, s);
        }
        // A box that crosses the near plane cannot be bounded: keep the light everywhere.
        if (behind || (mx.x >= lo.x && mn.x <= hi.x && mx.y >= lo.y && mn.y <= hi.y)) {
            mask |= 1u << k;
        }
    }
    tile_masks[id.y * g.tiles.x + id.x] = mask;
}
