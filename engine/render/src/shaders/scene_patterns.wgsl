// ---------------------------------------------------------------- noise + patterns

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2<f32>) -> f32 {
    return vnoise(p) * 0.5 + vnoise(p * 2.03 + 7.1) * 0.3 + vnoise(p * 4.11 + 3.7) * 0.2;
}

struct Pat { albedo: vec3<f32>, rough: f32, metal: f32, height: f32, emissive: vec3<f32> };

// Planar projection along the dominant normal axis; walls use (along-wall, up).
fn plane_uv(p: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    let a = abs(n);
    if (a.y > a.x && a.y > a.z) { return p.xz; }
    if (a.x > a.z) { return vec2<f32>(p.z, p.y); }
    return vec2<f32>(p.x, p.y);
}

fn line_mask(x: f32, w: f32) -> f32 {
    let d = abs(fract(x) - 0.5);
    return smoothstep(0.5 - w, 0.5 - w * 0.4, d);
}

fn pattern(id: u32, p: vec3<f32>, n: vec3<f32>, base: vec3<f32>, rough0: f32, metal0: f32) -> Pat {
    var o: Pat;
    o.albedo = base;
    o.rough = rough0;
    o.metal = metal0;
    o.height = 0.0;
    o.emissive = vec3<f32>(0.0);
    let uv = plane_uv(p, n);
    switch id {
        case 1u: { // brick 0.42 x 0.18 m, running bond
            let b = uv / vec2<f32>(0.42, 0.18);
            let row = floor(b.y);
            let bx = b.x + row * 0.5;
            let cell = vec2<f32>(floor(bx), row);
            let mortar = max(line_mask(bx + 0.5, 0.06), line_mask(b.y + 0.5, 0.11));
            let v = hash2(cell);
            let brick = base * (0.75 + 0.45 * v) * (0.9 + 0.2 * vnoise(uv * 9.0));
            o.albedo = mix(brick, vec3<f32>(0.42, 0.4, 0.37), mortar);
            o.height = 1.0 - mortar;
            o.rough = mix(rough0, 0.95, mortar);
        }
        case 2u: { // concrete panels 2 m with tie holes
            let f = fbm(uv * 1.7);
            let seam = max(line_mask(uv.x * 0.5 + 0.5, 0.008), line_mask(uv.y * 0.5 + 0.5, 0.008));
            let hole_uv = fract(uv * 0.5 + vec2<f32>(0.25, 0.25)) - 0.5;
            let hole = 1.0 - smoothstep(0.015, 0.025, length(hole_uv));
            o.albedo = base * (0.82 + 0.3 * f) * (1.0 - 0.4 * seam - 0.5 * hole);
            o.height = f * 0.3 - seam - hole;
            o.rough = clamp(rough0 + (f - 0.5) * 0.2, 0.0, 1.0);
        }
        case 3u: { // paving tiles 0.6 m
            let t = uv / 0.6;
            let grout = max(line_mask(t.x + 0.5, 0.04), line_mask(t.y + 0.5, 0.04));
            let v = hash2(floor(t));
            o.albedo = mix(base * (0.85 + 0.25 * v) * (0.9 + 0.15 * vnoise(uv * 6.0)), base * 0.45, grout);
            o.height = 1.0 - grout;
            o.rough = mix(rough0, 1.0, grout);
        }
        case 4u: { // corrugated metal, ridges 12 cm along the wall
            let s = sin(uv.x * 52.36);
            let streak = vnoise(vec2<f32>(uv.x * 3.0, uv.y * 0.4));
            o.albedo = base * (0.8 + 0.2 * s * 0.5 + 0.2 * streak);
            o.height = s * 0.5;
            o.rough = clamp(rough0 + streak * 0.25 - 0.1, 0.0, 1.0);
        }
        case 5u: { // wood planks 0.2 m
            let pl = uv.x / 0.2;
            let v = hash2(vec2<f32>(floor(pl), 3.0));
            let grain = vnoise(vec2<f32>(pl * 6.0, uv.y * 0.7 + v * 10.0));
            let gap = line_mask(pl + 0.5, 0.03);
            o.albedo = base * (0.75 + 0.35 * v) * (0.8 + 0.3 * grain) * (1.0 - 0.6 * gap);
            o.height = 1.0 - gap + grain * 0.2;
        }
        case 6u: { // grass
            let f = fbm(uv * 0.9);
            let fine = vnoise(uv * 23.0);
            o.albedo = base * (0.7 + 0.5 * f) * (0.85 + 0.3 * fine);
            o.albedo = mix(o.albedo, o.albedo * vec3<f32>(1.25, 1.05, 0.6), smoothstep(0.55, 0.8, f));
            o.height = fine;
            o.rough = 0.95;
        }
        case 7u: { // facade with windows: 2.4 m bays, 3.2 m floors
            let c = uv / vec2<f32>(2.4, 3.2);
            let cell = floor(c);
            let f = fract(c);
            let win = step(0.18, f.x) * step(f.x, 0.82) * step(0.28, f.y) * step(f.y, 0.86);
            let frame = (step(0.15, f.x) * step(f.x, 0.85) * step(0.25, f.y) * step(f.y, 0.89)) - win;
            let v = hash2(cell + floor(p.xz * 0.01));
            let wall = base * (0.85 + 0.2 * vnoise(uv * 3.0));
            let lit = step(0.72, v);
            let glass = mix(vec3<f32>(0.04, 0.06, 0.08), vec3<f32>(0.9, 0.7, 0.45), lit * 0.15);
            o.albedo = mix(mix(wall, vec3<f32>(0.12, 0.12, 0.13), frame), glass, win);
            o.rough = mix(mix(rough0, 0.5, frame), 0.06, win);
            o.metal = mix(metal0, 0.0, win);
            o.emissive = vec3<f32>(1.0, 0.72, 0.42) * win * lit * (0.6 + 1.2 * v) * 1.6;
            o.height = -win - frame * 0.5;
        }
        case 8u: { // hazard stripes
            let s = fract((uv.x + uv.y) * 2.5);
            let m = step(0.5, s);
            o.albedo = mix(vec3<f32>(0.9, 0.62, 0.03), vec3<f32>(0.03, 0.03, 0.03), m);
            o.height = m * 0.2;
        }
        case 9u: { // asphalt with lane marks along x
            let f = vnoise(uv * 14.0);
            let crack = smoothstep(0.48, 0.5, abs(fbm(uv * 0.7) - 0.5) + 0.48);
            o.albedo = base * (0.75 + 0.5 * f) * (1.0 - 0.3 * crack);
            let lane = (1.0 - smoothstep(0.06, 0.08, abs(fract(uv.y / 8.0 + 0.5) - 0.5) * 8.0)) * step(0.5, fract(uv.x / 3.0));
            o.albedo = mix(o.albedo, vec3<f32>(0.75, 0.72, 0.6), lane * 0.9);
            o.rough = mix(0.85 + f * 0.1, 0.6, lane);
            o.height = f * 0.5;
        }
        case 10u: { // prototype grid
            let major = max(line_mask(uv.x + 0.5, 0.012), line_mask(uv.y + 0.5, 0.012));
            let minor = max(line_mask(uv.x * 4.0 + 0.5, 0.025), line_mask(uv.y * 4.0 + 0.5, 0.025));
            o.albedo = base * (1.0 - 0.18 * minor) * (1.0 - 0.45 * major);
            o.height = -major;
        }
        case 11u: { // quilted padding (object space, horizontal channels)
            let q = fract(p.y / 0.07);
            let s = sin(q * 3.14159);
            o.albedo = base * (0.7 + 0.4 * s);
            o.height = s;
        }
        case 12u: { // scales (object space)
            let s = vec2<f32>(p.x + p.z, p.y) / 0.12;
            let row = floor(s.y);
            let c = fract(vec2<f32>(s.x + row * 0.5, s.y)) - vec2<f32>(0.5, 0.0);
            let d = length(c);
            let e = smoothstep(0.35, 0.6, d);
            o.albedo = base * (1.05 - 0.45 * e) * (0.9 + 0.2 * hash2(vec2<f32>(floor(s.x + row * 0.5), row)));
            o.height = 1.0 - e;
        }
        default: {}
    }
    return o;
}
