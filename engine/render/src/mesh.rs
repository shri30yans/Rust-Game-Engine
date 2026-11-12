//! Procedural meshes. Indexed, smooth where the shape is smooth, flat on box faces.
//! Positions and packed attributes are separate streams so depth and shadow draws
//! can bind only the 12-byte position stream.
use crate::to_u32;
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};

/// A mesh position, uploaded as the first 12-byte vertex stream.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vertex {
    /// Object-space position.
    pub pos: [f32; 3],
}

/// Packed shading values paired with a position in the 12-byte attribute stream.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq, Eq)]
pub struct VertexAttributes {
    /// Octahedral object-space normal.
    pub normal_oct: [i16; 2],
    /// Linear RGBA base colour in unorm8.
    pub color: [u8; 4],
    /// Roughness, metalness, pattern id and ambient occlusion in unorm8.
    pub material: [u8; 4],
}

#[derive(Clone, Copy)]
struct EncodedVertex {
    position: Vertex,
    attributes: VertexAttributes,
}

/// Surface description used while building. `color` is linear RGB.
#[derive(Clone, Copy, Debug)]
pub struct Mat {
    /// Linear RGB base colour.
    pub color: [f32; 3],
    /// Roughness, 0 (glossy) to 1 (matte).
    pub rough: f32,
    /// Metalness, 0 (dielectric) to 1 (metal).
    pub metal: f32,
    /// Procedural pattern evaluated in the fragment shader.
    pub pattern: Pattern,
}

/// Procedural surface patterns evaluated in the fragment shader (zero texture bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Pattern {
    /// Plain surface, no pattern.
    None = 0,
    /// Brick courses.
    Brick = 1,
    /// Poured concrete.
    Concrete = 2,
    /// Square floor tiles.
    Tiles = 3,
    /// Corrugated sheet metal.
    Corrugated = 4,
    /// Wood grain.
    Wood = 5,
    /// Grass.
    Grass = 6,
    /// Window grid.
    Windows = 7,
    /// Yellow and black hazard stripes.
    Hazard = 8,
    /// Asphalt.
    Asphalt = 9,
    /// Square grid lines.
    Grid = 10,
    /// Quilted fabric.
    Quilt = 11,
    /// Overlapping scales.
    Scales = 12,
}

impl Mat {
    /// Material with no pattern.
    pub const fn new(color: [f32; 3], rough: f32, metal: f32) -> Mat {
        Mat {
            color,
            rough,
            metal,
            pattern: Pattern::None,
        }
    }
    /// Returns this material with `p` as its pattern.
    pub const fn pat(mut self, p: Pattern) -> Mat {
        self.pattern = p;
        self
    }
    /// Material from an sRGB hex colour (0xRRGGBB).
    pub fn hex(hex: u32, rough: f32, metal: f32) -> Mat {
        Mat::new(crate::srgb(hex), rough, metal)
    }
}

/// Indexed triangle list being built. Vertices and indices are public so callers can
/// upload them directly.
#[derive(Default, Clone)]
pub struct MeshBuilder {
    /// Vertices in build order.
    pub verts: Vec<Vertex>,
    /// Packed attributes paired with `verts` by index.
    attrs: Vec<VertexAttributes>,
    /// Triangle indices into `verts`, three per triangle.
    pub idx: Vec<u32>,
}

fn quantize_unorm8(value: f32) -> u8 {
    let scaled = value.clamp(0.0, 1.0) * 255.0;
    let mut low = 0u8;
    let mut high = u8::MAX;
    while low < high {
        let middle = low + (high - low) / 2;
        if f32::from(middle) < scaled {
            low = middle.saturating_add(1);
        } else {
            high = middle;
        }
    }
    if low > 0 && f32::from(low) - scaled > scaled - f32::from(low - 1) {
        low - 1
    } else {
        low
    }
}

pub(crate) fn pack_unorm8(value: f32) -> u8 {
    quantize_unorm8(value)
}

fn quantize_snorm16(value: f32) -> i16 {
    let scaled = value.clamp(-1.0, 1.0).abs() * 32767.0;
    let mut low = 0u16;
    let mut high = 32767u16;
    while low < high {
        let middle = low + (high - low) / 2;
        if f32::from(middle) < scaled {
            low = low.saturating_add(1);
        } else {
            high = middle;
        }
    }
    let rounded = if low > 0 && f32::from(low) - scaled > scaled - f32::from(low - 1) {
        low - 1
    } else {
        low
    };
    let magnitude = i16::try_from(rounded).unwrap_or(i16::MAX);
    if value.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

fn encode_normal(normal: Vec3) -> [i16; 2] {
    let normal = normal.normalize_or_zero();
    let denominator = normal.x.abs() + normal.y.abs() + normal.z.abs();
    if denominator <= f32::EPSILON {
        return [0, 0];
    }
    let mut p = Vec3::new(
        normal.x / denominator,
        normal.y / denominator,
        normal.z / denominator,
    );
    if p.z < 0.0 {
        let x = (1.0 - p.y.abs()) * p.x.signum();
        let y = (1.0 - p.x.abs()) * p.y.signum();
        p.x = x;
        p.y = y;
    }
    [quantize_snorm16(p.x), quantize_snorm16(p.y)]
}

fn decode_normal(oct: [i16; 2]) -> Vec3 {
    let x = f32::from(oct[0]) / 32767.0;
    let y = f32::from(oct[1]) / 32767.0;
    let mut normal = Vec3::new(x, y, 1.0 - x.abs() - y.abs());
    if normal.z < 0.0 {
        let old_x = normal.x;
        normal.x = (1.0 - normal.y.abs()) * old_x.signum();
        normal.y = (1.0 - old_x.abs()) * normal.y.signum();
    }
    normal.normalize_or_zero()
}

fn vtx(p: Vec3, n: Vec3, m: &Mat, ao: f32) -> EncodedVertex {
    EncodedVertex {
        position: Vertex { pos: p.to_array() },
        attributes: VertexAttributes {
            normal_oct: encode_normal(n),
            color: [
                quantize_unorm8(m.color[0]),
                quantize_unorm8(m.color[1]),
                quantize_unorm8(m.color[2]),
                u8::MAX,
            ],
            material: [
                quantize_unorm8(m.rough),
                quantize_unorm8(m.metal),
                m.pattern as u8,
                quantize_unorm8(ao),
            ],
        },
    }
}

/// Unit axis for loop index `i`: X, Y, Z for 0, 1, 2 (wrapping modulo 3).
fn axis(i: usize) -> Vec3 {
    match i % 3 {
        0 => Vec3::X,
        1 => Vec3::Y,
        _ => Vec3::Z,
    }
}

/// Component `i` of `v` (x, y, z for 0, 1, 2; wrapping modulo 3).
fn comp(v: Vec3, i: usize) -> f32 {
    match i % 3 {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

impl MeshBuilder {
    /// Empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    fn push_vertex(&mut self, encoded: EncodedVertex) {
        self.verts.push(encoded.position);
        self.attrs.push(encoded.attributes);
    }

    /// Packed vertex attributes in the same order as [`MeshBuilder::verts`].
    #[must_use]
    pub fn attributes(&self) -> &[VertexAttributes] {
        &self.attrs
    }

    fn base(&self) -> u32 {
        to_u32(self.verts.len())
    }

    /// Axis-aligned box centred at `c` with half extents `h`. Flat faces.
    pub fn cube(&mut self, c: Vec3, h: Vec3, m: Mat) -> &mut Self {
        for i in 0..3 {
            for s in [-1.0f32, 1.0] {
                let n = axis(i) * s;
                let u = axis(i + 1);
                let v = axis(i + 2);
                let (u, v) = if s > 0.0 { (u, v) } else { (v, u) };
                let b = self.base();
                let fc = c + n * h;
                let hu = u * h;
                let hv = v * h;
                for (a, bb) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    // Darken bottom faces a touch: cheap baked contact occlusion.
                    let ao = if n.y < -0.5 { 0.7 } else { 1.0 };
                    self.push_vertex(vtx(fc + hu * a + hv * bb, n, &m, ao));
                }
                self.idx
                    .extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        self
    }

    /// Unit cube (half 0.5) at the origin: the workhorse for instanced level geometry.
    pub fn unit_cube(m: Mat) -> MeshBuilder {
        let mut b = MeshBuilder::new();
        b.cube(Vec3::ZERO, Vec3::splat(0.5), m);
        b
    }

    /// Box with rounded edges (smooth normals). `r` is the edge radius, `s` the
    /// number of segments per rounded edge.
    pub fn rounded_box(&mut self, c: Vec3, h: Vec3, r: f32, s: u32, m: Mat) -> &mut Self {
        let r = r.min(h.x).min(h.y).min(h.z).max(1e-4);
        let inner = h - Vec3::splat(r);
        // Profile of one axis: `ii` is the flat half-extent, `r` the edge radius.
        let coords = |ii: f32| -> Vec<f32> {
            let mut v = Vec::new();
            for k in 0..=s {
                let a = std::f32::consts::FRAC_PI_2 * (1.0 - k as f32 / s as f32);
                v.push(-ii - r * a.sin());
            }
            for k in 0..=s {
                let a = std::f32::consts::FRAC_PI_2 * (k as f32 / s as f32);
                v.push(ii + r * a.sin());
            }
            v
        };
        for i in 0..3 {
            for sgn in [-1.0f32, 1.0] {
                let ui = (i + 1) % 3;
                let vi = (i + 2) % 3;
                let cu = coords(comp(inner, ui));
                let cv = coords(comp(inner, vi));
                let b = self.base();
                let w = to_u32(cu.len());
                for &bv in cv.iter() {
                    for &au in cu.iter() {
                        let mut q = axis(i) * sgn * comp(h, i);
                        q += axis(ui) * au + axis(vi) * bv;
                        let cl = q.clamp(-inner, inner);
                        let n = (q - cl).normalize_or_zero();
                        let p = cl + n * r;
                        self.push_vertex(vtx(c + p, n, &m, if n.y < -0.6 { 0.75 } else { 1.0 }));
                    }
                }
                let hgt = to_u32(cv.len());
                for y in 0..hgt - 1 {
                    for x in 0..w - 1 {
                        let a = b + y * w + x;
                        let (b1, c1, d1) = (a + 1, a + w + 1, a + w);
                        if sgn > 0.0 {
                            self.idx.extend_from_slice(&[a, b1, c1, a, c1, d1]);
                        } else {
                            self.idx.extend_from_slice(&[a, c1, b1, a, d1, c1]);
                        }
                    }
                }
            }
        }
        self
    }

    /// UV sphere (ellipsoid when `scale` is not uniform).
    pub fn sphere(&mut self, c: Vec3, scale: Vec3, seg: u32, rings: u32, m: Mat) -> &mut Self {
        let b = self.base();
        for j in 0..=rings {
            let t = std::f32::consts::PI * j as f32 / rings as f32;
            for i in 0..=seg {
                let p = std::f32::consts::TAU * i as f32 / seg as f32;
                let d = Vec3::new(t.sin() * p.cos(), t.cos(), t.sin() * p.sin());
                let n = (d / scale).normalize();
                self.push_vertex(vtx(c + d * scale, n, &m, 0.8 + 0.2 * (d.y * 0.5 + 0.5)));
            }
        }
        let w = seg + 1;
        for j in 0..rings {
            for i in 0..seg {
                let a = b + j * w + i;
                self.idx
                    .extend_from_slice(&[a, a + 1, a + w + 1, a, a + w + 1, a + w]);
            }
        }
        self
    }

    /// Cylinder along Y from `c - h` to `c + h`, optional taper (`r1` at top).
    pub fn cylinder(&mut self, c: Vec3, r0: f32, r1: f32, h: f32, seg: u32, m: Mat) -> &mut Self {
        let b = self.base();
        let slope = (r0 - r1) / (2.0 * h);
        for i in 0..=seg {
            let a = std::f32::consts::TAU * i as f32 / seg as f32;
            let d = Vec3::new(a.cos(), 0.0, a.sin());
            let n = (d + Vec3::Y * slope).normalize();
            self.push_vertex(vtx(c + d * r0 - Vec3::Y * h, n, &m, 0.8));
            self.push_vertex(vtx(c + d * r1 + Vec3::Y * h, n, &m, 1.0));
        }
        for i in 0..seg {
            let a = b + i * 2;
            self.idx
                .extend_from_slice(&[a, a + 1, a + 3, a, a + 3, a + 2]);
        }
        for (y, r, n) in [(-h, r0, -Vec3::Y), (h, r1, Vec3::Y)] {
            let cb = self.base();
            self.push_vertex(vtx(c + Vec3::Y * y, n, &m, 1.0));
            for i in 0..=seg {
                let a = std::f32::consts::TAU * i as f32 / seg as f32;
                self.push_vertex(vtx(c + Vec3::new(a.cos() * r, y, a.sin() * r), n, &m, 1.0));
            }
            for i in 0..seg {
                if n.y > 0.0 {
                    self.idx.extend_from_slice(&[cb, cb + 2 + i, cb + 1 + i]);
                } else {
                    self.idx.extend_from_slice(&[cb, cb + 1 + i, cb + 2 + i]);
                }
            }
        }
        self
    }

    /// Capsule along Y: hemispheres of radius `r` joined by a cylinder of half height `h`.
    pub fn capsule(&mut self, c: Vec3, r: f32, h: f32, seg: u32, m: Mat) -> &mut Self {
        let b = self.base();
        let half_rings = (seg / 2).max(3);
        let rings = half_rings * 2 + 1;
        for j in 0..=rings {
            let (t, off) = if j <= half_rings {
                (
                    std::f32::consts::FRAC_PI_2 * j as f32 / half_rings as f32,
                    h,
                )
            } else {
                (
                    std::f32::consts::FRAC_PI_2
                        * (1.0 + (j - half_rings - 1) as f32 / half_rings as f32),
                    -h,
                )
            };
            for i in 0..=seg {
                let p = std::f32::consts::TAU * i as f32 / seg as f32;
                let d = Vec3::new(t.sin() * p.cos(), t.cos(), t.sin() * p.sin());
                self.push_vertex(vtx(
                    c + d * r + Vec3::Y * off,
                    d,
                    &m,
                    if d.y < -0.5 { 0.8 } else { 1.0 },
                ));
            }
        }
        let w = seg + 1;
        for j in 0..rings {
            for i in 0..seg {
                let a = b + j * w + i;
                self.idx
                    .extend_from_slice(&[a, a + 1, a + w + 1, a, a + w + 1, a + w]);
            }
        }
        self
    }

    /// Flat quad facing +Y at height `c.y`.
    pub fn plane(&mut self, c: Vec3, hx: f32, hz: f32, m: Mat) -> &mut Self {
        let b = self.base();
        for (x, z) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)] {
            self.push_vertex(vtx(c + Vec3::new(x * hx, 0.0, z * hz), Vec3::Y, &m, 1.0));
        }
        self.idx
            .extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        self
    }

    /// Triangular prism (wedge): a box whose top slopes from full height at -Z to 0 at +Z.
    pub fn wedge(&mut self, c: Vec3, h: Vec3, m: Mat) -> &mut Self {
        let p = |x: f32, y: f32, z: f32| c + Vec3::new(x * h.x, y * h.y, z * h.z);
        let quads: [[Vec3; 4]; 3] = [
            [
                p(-1., -1., -1.),
                p(1., -1., -1.),
                p(1., -1., 1.),
                p(-1., -1., 1.),
            ],
            [
                p(-1., -1., -1.),
                p(-1., 1., -1.),
                p(1., 1., -1.),
                p(1., -1., -1.),
            ],
            [
                p(-1., 1., -1.),
                p(-1., -1., 1.),
                p(1., -1., 1.),
                p(1., 1., -1.),
            ],
        ];
        for q in quads {
            let n = (q[1] - q[0]).cross(q[2] - q[0]).normalize();
            let inside = c + Vec3::new(0.0, -0.5 * h.y, -0.5 * h.z);
            let flip = n.dot((q[0] + q[2]) * 0.5 - inside) < 0.0;
            let n = if flip { -n } else { n };
            let b = self.base();
            for v in q {
                self.push_vertex(vtx(v, n, &m, 1.0));
            }
            if flip {
                self.idx
                    .extend_from_slice(&[b, b + 2, b + 1, b, b + 3, b + 2]);
            } else {
                self.idx
                    .extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            }
        }
        for x in [-1.0f32, 1.0] {
            let tri = [p(x, -1., -1.), p(x, 1., -1.), p(x, -1., 1.)];
            let n = Vec3::X * x;
            let b = self.base();
            for v in tri {
                self.push_vertex(vtx(v, n, &m, 1.0));
            }
            if x > 0.0 {
                self.idx.extend_from_slice(&[b, b + 1, b + 2]);
            } else {
                self.idx.extend_from_slice(&[b, b + 2, b + 1]);
            }
        }
        self
    }

    /// Append another mesh transformed by `t` (compound models stay one draw).
    pub fn append(&mut self, o: &MeshBuilder, t: Mat4) -> &mut Self {
        let b = self.base();
        let nm = Mat3::from_mat4(t).inverse().transpose();
        let flip = Mat3::from_mat4(t).determinant() < 0.0;
        for (v, attributes) in o.verts.iter().zip(o.attrs.iter()) {
            let mut nv = *v;
            nv.pos = t.transform_point3(Vec3::from(v.pos)).to_array();
            let mut na = *attributes;
            na.normal_oct =
                encode_normal((nm * decode_normal(attributes.normal_oct)).normalize_or_zero());
            self.verts.push(nv);
            self.attrs.push(na);
        }
        for &[i0, i1, i2] in o.idx.as_chunks::<3>().0 {
            if flip {
                self.idx.extend_from_slice(&[b + i0, b + i2, b + i1]);
            } else {
                self.idx.extend_from_slice(&[b + i0, b + i1, b + i2]);
            }
        }
        self
    }

    /// Bounding sphere radius around the origin (used for culling).
    pub fn radius(&self) -> f32 {
        self.verts
            .iter()
            .map(|v| Vec3::from(v.pos).length())
            .fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::{Vertex, VertexAttributes, decode_normal, encode_normal};
    use glam::Vec3;

    #[test]
    fn procedural_vertex_streams_have_the_compact_layout() {
        assert_eq!(std::mem::size_of::<Vertex>(), 12);
        assert_eq!(std::mem::size_of::<VertexAttributes>(), 12);
    }

    #[test]
    fn octahedral_normals_round_trip_within_half_a_degree() {
        let normals = [
            Vec3::X,
            Vec3::Y,
            Vec3::Z,
            Vec3::new(-0.7, 0.4, 0.59).normalize(),
            Vec3::new(0.31, -0.88, -0.36).normalize(),
        ];
        for normal in normals {
            let decoded = decode_normal(encode_normal(normal));
            let angle = normal.dot(decoded).clamp(-1.0, 1.0).acos().to_degrees();
            assert!(angle <= 0.5, "normal error {angle} degrees");
        }
    }
}
