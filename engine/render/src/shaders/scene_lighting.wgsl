// ---------------------------------------------------------------- lighting

const PI: f32 = 3.14159265;

struct SurfaceInputs {
    albedo: vec3<f32>,
    normal: vec3<f32>,
    roughness: f32,
    metallic: f32,
    ao: f32,
    emissive: vec3<f32>,
};

fn sky_color(d: vec3<f32>, clouds: bool) -> vec3<f32> {
    let sun = g.sun_dir.xyz;
    let up = max(d.y, 0.0);
    var c = mix(g.sky_horizon.rgb, g.sky_zenith.rgb, pow(up, 0.45));
    let below = clamp(-d.y * 3.0, 0.0, 1.0);
    c = mix(c, g.ground.rgb * 0.6, below);
    let mu = max(dot(d, sun), 0.0);
    c += g.sun_color.rgb * (pow(mu, 8.0) * 0.25 + pow(mu, 64.0) * 0.6);
    if (clouds && d.y > 0.01) {
        let t = g.cam_pos.w;
        let cp = d.xz / (d.y + 0.08) * 1.6 + vec2<f32>(t * 0.01, t * 0.004);
        let f = fbm(cp) * 0.7 + fbm(cp * 3.1) * 0.3;
        let cov = smoothstep(0.58 - g.misc.y * 0.25, 0.9, f) * smoothstep(0.0, 0.25, d.y);
        let lit = mix(vec3<f32>(0.75, 0.78, 0.85), vec3<f32>(1.15, 1.05, 0.95), mu) * (g.sky_horizon.rgb + g.sun_color.rgb * 0.15);
        c = mix(c, lit, cov * 0.85);
    }
    return c;
}

fn shadow_at(wpos: vec3<f32>, n: vec3<f32>, dist: f32) -> f32 {
    if (dist > g.shadow.z) { return 1.0; }
    var ci = 0u;
    if (dist > g.shadow.y) { ci = 1u; }
    let wt = select(g.misc.z, g.misc.w, ci == 1u);
    let p = wpos + n * wt * 1.5;
    let lp = g.cascade[ci] * vec4<f32>(p, 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    let z = lp.z - 0.0002;
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { return 1.0; }
    var s = 0.0;
    if (g.shadow.w > 0.5) {
        let ts = g.shadow.x;
        for (var y = -1; y <= 1; y = y + 1) {
            for (var x = -1; x <= 1; x = x + 1) {
                s += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2<f32>(f32(x), f32(y)) * ts * 1.2, i32(ci), z);
            }
        }
        s = s / 9.0;
    } else {
        s = textureSampleCompareLevel(shadow_map, shadow_samp, uv, i32(ci), z);
    }
    let fade = smoothstep(g.shadow.z * 0.85, g.shadow.z, dist);
    return mix(s, 1.0, fade);
}

fn fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - c, 5.0);
}

fn brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, rough: f32, metal: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-4);
    let ndh = max(dot(n, h), 0.0);
    let a = rough * rough;
    let a2 = a * a;
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let D = a2 / (PI * dd * dd);
    let k = a * 0.5;
    let Vis = 0.25 / ((ndl * (1.0 - k) + k) * (ndv * (1.0 - k) + k));
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let F = fresnel(f0, max(dot(v, h), 0.0));
    let spec = D * Vis * F;
    let kd = (vec3<f32>(1.0) - F) * (1.0 - metal);
    return (kd * albedo / PI + spec) * ndl;
}

fn fog_apply(c: vec3<f32>, wpos: vec3<f32>) -> vec3<f32> {
    let ro = g.cam_pos.xyz;
    let rd_full = wpos - ro;
    let dist = length(rd_full);
    let rd = rd_full / max(dist, 1e-4);
    let fall = g.fog.y;
    let base = g.fog.x * exp(-fall * (ro.y - g.fog.z));
    var amount = base * dist;
    if (abs(rd.y) > 1e-3) {
        amount = base * (1.0 - exp(-fall * rd.y * dist)) / (fall * rd.y);
    }
    let f = 1.0 - exp(-max(amount, 0.0));
    let sun = pow(max(dot(rd, g.sun_dir.xyz), 0.0), 8.0) * g.fog.w;
    let fc = g.fog_color.rgb + g.sun_color.rgb * sun;
    return mix(c, fc, clamp(f, 0.0, 1.0));
}

struct PrepassOut {
    @location(0) normal: vec4<f32>,
    // Screen-space motion in uv units (current minus previous), read by TAA.
    @location(1) velocity: vec2<f32>,
};

// Depth prepass: view-space normal, and per-pixel motion for TAA. Depth is written separately.
@fragment
fn fs_prepass(i: DepthOut) -> PrepassOut {
    let geometric_normal = normalize(cross(dpdx(i.wpos), dpdy(i.wpos)));
    let vn = normalize((g.view * vec4<f32>(geometric_normal, 0.0)).xyz);
    var o: PrepassOut;
    o.normal = vec4<f32>(vn * 0.5 + 0.5, 1.0);
    o.velocity = screen_uv(i.cur) - screen_uv(i.prev);
    return o;
}

fn procedural_surface(i: VOut) -> SurfaceInputs {
    var nrm = normalize(i.n);
    // Material: vertex values, overridden per instance where extra >= 0.
    var pid = u32(i.mat.z + 0.5);
    if (i.extra.x >= 0.0) { pid = u32(i.extra.x + 0.5); }
    var r0 = i.mat.x;
    if (i.extra.y >= 0.0) { r0 = i.extra.y; }
    var m0 = i.mat.y;
    if (i.extra.z >= 0.0) { m0 = i.extra.z; }
    let base = i.color * i.tint.rgb;
    let pat = pattern(pid, i.ppos, nrm, base, r0, m0);

    // Bump from the pattern height via screen-space derivatives (no tangents needed).
    let dpx = dpdx(i.ppos);
    let dpy = dpdy(i.ppos);
    let dhx = dpdx(pat.height);
    let dhy = dpdy(pat.height);
    let r1 = cross(dpy, nrm);
    let r2 = cross(nrm, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dhx * r1 + dhy * r2);
    let bump = 0.006;
    let nb = normalize(abs(det) * nrm - grad * bump * select(1.0, 0.0, abs(det) < 1e-12));
    if (abs(det) > 1e-12) { nrm = nb; }

    // Specular anti-aliasing: widen roughness where the normal changes fast.
    let dndx = dpdx(nrm);
    let dndy = dpdy(nrm);
    let variance = 0.25 * (dot(dndx, dndx) + dot(dndy, dndy));
    let a = pat.rough * pat.rough;
    let a2 = clamp(a * a + min(2.0 * variance, 0.2), 0.0, 1.0);
    let rough = clamp(sqrt(sqrt(a2)), 0.045, 1.0);

    let flags = u32(i.extra.w + 0.5);
    return SurfaceInputs(
        select(pat.albedo, base, (flags & 2u) != 0u),
        nrm,
        rough,
        pat.metal,
        i.mat.w,
        pat.emissive + base * i.tint.a,
    );
}

fn shade_procedural(i: VOut) -> vec4<f32> {
    let surface = procedural_surface(i);
    let flags = u32(i.extra.w + 0.5);
    return shade_surface(i, surface, (flags & 2u) != 0u);
}

fn base_colour_procedural(i: VOut) -> vec4<f32> {
    return vec4<f32>(procedural_surface(i).albedo, 1.0);
}

fn lighting_only_procedural(i: VOut) -> vec4<f32> {
    var surface = procedural_surface(i);
    surface.albedo = vec3<f32>(1.0);
    surface.emissive = vec3<f32>(0.0);
    return shade_surface(i, surface, false);
}

fn shade_surface(i: VOut, surface: SurfaceInputs, unlit: bool) -> vec4<f32> {
    if unlit {
        return vec4<f32>(surface.albedo + surface.emissive, 1.0);
    }
    let nrm = normalize(surface.normal);
    let v = normalize(g.cam_pos.xyz - i.wpos);
    let dist = length(g.cam_pos.xyz - i.wpos);
    let albedo = surface.albedo;
    let rough = clamp(surface.roughness, 0.045, 1.0);
    let metal = surface.metallic;
    let ao = surface.ao * ssao_at(i.clip);

    let sh = shadow_at(i.wpos, normalize(i.n), dist);
    var c = brdf(nrm, v, g.sun_dir.xyz, albedo, rough, metal) * g.sun_color.rgb * g.sun_dir.w * sh;

    let tile = vec2<u32>(i.clip.xy) / g.tiles.z;
    var mask = tile_masks[tile.y * g.tiles.x + tile.x];
    while (mask != 0u) {
        let k = firstTrailingBit(mask);
        mask &= mask - 1u;
        let L = g.lights[k];
        let d = L.pos_r.xyz - i.wpos;
        let d2 = dot(d, d);
        let r = L.pos_r.w;
        if (d2 < r * r) {
            let x = d2 / (r * r);
            let win = (1.0 - x * x);
            let att = win * win / (d2 + 1.0);
            c += brdf(nrm, v, d * inverseSqrt(d2), albedo, rough, metal) * L.col_i.rgb * L.col_i.w * att;
        }
    }

    // Ambient: sky/ground hemisphere for diffuse, analytic sky for specular.
    let hemi = mix(g.ground.rgb, g.sky_zenith.rgb * 0.6 + g.sky_horizon.rgb * 0.4, nrm.y * 0.5 + 0.5);
    let ndv = max(dot(nrm, v), 1e-4);
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let fr = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - ndv, 5.0);
    let rdir = reflect(-v, nrm);
    let env = sky_color(normalize(mix(rdir, nrm, rough * rough)), false);
    let spec_occ = clamp(ao * (0.4 + 0.6 * sh) + (1.0 - rough) * 0.2, 0.0, 1.0);
    let env_spec = env * fr * spec_occ * mix(1.0, 0.25, rough);
    let amb = hemi * albedo * (1.0 - metal) * ao * g.ground.w;
    c += amb + env_spec * g.ground.w + surface.emissive;

    c = fog_apply(c, i.wpos);
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    return shade_procedural(i);
}

fn textured_surface(i: VOut) -> SurfaceInputs {
    let base_sample = textureSample(base_color_tex, material_samp, i.uv);
    let base = base_sample * material.base_color * vec4<f32>(i.tint.rgb, 1.0);
    if material.alpha.x > 0.5 && base.a < material.alpha.y {
        discard;
    }
    let orm = textureSample(orm_tex, material_samp, i.uv);
    let normal_sample = textureSample(normal_tex, material_samp, i.uv).xyz * 2.0 - 1.0;
    let tangent = normalize(i.tangent.xyz - i.n * dot(i.n, i.tangent.xyz));
    let bitangent = normalize(cross(i.n, tangent)) * i.tangent.w;
    let mapped = normalize(mat3x3<f32>(tangent, bitangent, normalize(i.n)) *
        vec3<f32>(normal_sample.xy * material.factors.w, normal_sample.z));
    let emissive = textureSample(emissive_tex, material_samp, i.uv).rgb * material.factors.z;
    return SurfaceInputs(
        base.rgb,
        mapped,
        orm.g * material.factors.y,
        orm.b * material.factors.x,
        orm.r,
        emissive,
    );
}

@fragment
fn fs_textured(i: VOut) -> @location(0) vec4<f32> {
    return shade_surface(i, textured_surface(i), false);
}

@fragment
fn fs_coverage_marker() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
}

@fragment
fn fs_base_colour(i: VOut) -> @location(0) vec4<f32> {
    return base_colour_procedural(i);
}

@fragment
fn fs_base_colour_textured(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textured_surface(i).albedo, 1.0);
}

@fragment
fn fs_lighting_only(i: VOut) -> @location(0) vec4<f32> {
    return lighting_only_procedural(i);
}

@fragment
fn fs_lighting_only_textured(i: VOut) -> @location(0) vec4<f32> {
    var surface = textured_surface(i);
    surface.albedo = vec3<f32>(1.0);
    surface.emissive = vec3<f32>(0.0);
    return shade_surface(i, surface, false);
}

// Translucent additive shells (zones, shields, storm walls): rim-lit, unshadowed.
fn shade_glow(i: VOut) -> vec4<f32> {
    let v = normalize(g.cam_pos.xyz - i.wpos);
    let n = normalize(i.n);
    let rim = 1.0 - abs(dot(n, v));
    let t = g.cam_pos.w;
    let bands = 0.75 + 0.25 * sin(i.wpos.y * 6.0 - t * 3.0);
    let a = (0.25 + 0.75 * pow(rim, 2.0)) * bands;
    return vec4<f32>(i.color * i.tint.rgb * i.tint.a * a, 0.0);
}

@fragment
fn fs_glow(i: VOut) -> @location(0) vec4<f32> {
    return shade_glow(i);
}

@fragment
fn fs_glow_textured(i: VOut) -> @location(0) vec4<f32> {
    return shade_glow(i);
}

// ---------------------------------------------------------------- sky

struct SkyOut { @builtin(position) clip: vec4<f32>, @location(0) ndc: vec2<f32> };

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    var o: SkyOut;
    o.clip = vec4<f32>(x, y, 1.0, 1.0);
    o.ndc = vec2<f32>(x, y);
    return o;
}

@fragment
fn fs_sky(i: SkyOut) -> @location(0) vec4<f32> {
    let p = g.inv_view_proj * vec4<f32>(i.ndc, 1.0, 1.0);
    let d = normalize(p.xyz / p.w - g.cam_pos.xyz);
    var c = sky_color(d, true);
    let mu = dot(d, g.sun_dir.xyz);
    c += g.sun_color.rgb * smoothstep(0.99955, 0.9998, mu) * 30.0;
    // Fog toward the horizon so geometry and sky meet without a seam.
    let h = 1.0 - smoothstep(0.0, 0.18, d.y + 0.02);
    c = mix(c, g.fog_color.rgb + g.sun_color.rgb * pow(max(mu, 0.0), 8.0) * g.fog.w, h * clamp(g.fog.x * 60.0, 0.0, 0.85));
    return vec4<f32>(c, 1.0);
}

// ---------------------------------------------------------------- particles

struct PIn {
    @location(0) pos_size: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) vel_kind: vec4<f32>,
};

struct POut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) kind: f32,
};

@vertex
fn vs_particle(@builtin(vertex_index) vi: u32, p: PIn) -> POut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let c = corners[vi];
    let right = vec3<f32>(g.view[0][0], g.view[1][0], g.view[2][0]);
    let up = vec3<f32>(g.view[0][1], g.view[1][1], g.view[2][1]);
    let size = p.pos_size.w;
    var w = p.pos_size.xyz + (right * c.x + up * c.y) * size;
    let vel = p.vel_kind.xyz;
    let sp = length(vel);
    if (sp > 0.01) {
        // Stretched along velocity (sparks, tracers).
        let axis = vel / sp;
        let to_cam = normalize(g.cam_pos.xyz - p.pos_size.xyz);
        let side = normalize(cross(axis, to_cam));
        w = p.pos_size.xyz + side * c.x * size + axis * c.y * (size + sp);
    }
    var o: POut;
    o.clip = g.view_proj * vec4<f32>(w, 1.0);
    o.uv = c;
    o.color = p.color;
    o.kind = p.vel_kind.w;
    return o;
}

@fragment
fn fs_particle(i: POut) -> @location(0) vec4<f32> {
    let r = length(i.uv);
    var a = 0.0;
    let k = u32(i.kind + 0.5);
    if (k == 0u) { a = pow(clamp(1.0 - r, 0.0, 1.0), 1.5); }          // soft glow
    else if (k == 1u) { a = pow(clamp(1.0 - abs(i.uv.x), 0.0, 1.0), 3.0) * smoothstep(1.0, 0.5, abs(i.uv.y)); } // spark
    else if (k == 2u) {                                                // smoke puff
        let n = vnoise(i.uv * 2.5 + i.color.rg * 13.0);
        a = smoothstep(1.0, 0.25, r + abs(n) * 0.3) * smoothstep(1.0, 0.75, r);
    } else if (k == 3u) { a = smoothstep(0.08, 0.0, abs(r - 0.85)); }  // ring
    else { a = step(r, 1.0); }                                         // hard disc
    let al = a * i.color.a;
    return vec4<f32>(i.color.rgb * al, al);
}
