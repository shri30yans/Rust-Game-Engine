//! Immediate-mode 2D drawing in pixels (origin top-left). Everything (panels,
//! strokes, rings, glows, text) is one vertex format and one draw call.
//! A *tilt* rotates the following shapes in 3D around a pivot and projects them back,
//! which gives the angled, "printed on glass" HUD panels.
use crate::font::{FontAtlas, FontId};
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Vec2, Vec3};

/// One UI vertex as uploaded to the GPU.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct UiVertex {
    /// Clip-space position (homogeneous, `w` carries the tilt perspective).
    pub pos: [f32; 4],
    /// Glyph atlas coordinates (text only).
    pub uv: [f32; 2],
    /// Position relative to the shape centre, in pixels.
    pub local: [f32; 2],
    /// Half size of the shape, in pixels.
    pub half: [f32; 2],
    /// Linear RGBA colour.
    pub color: [f32; 4],
    /// Shape parameters: radius, stroke width, mode, blur (mode-specific).
    pub p: [f32; 4],
}

/// Capacity of the UI vertex buffer; submissions past this are dropped.
pub const MAX_UI_VERTS: usize = 60_000;

/// Perspective tilt applied to following shapes.
#[derive(Clone, Copy, Debug)]
pub struct Tilt {
    /// Pivot point in pixels, the centre of rotation.
    pub pivot: Vec2,
    /// Rotation about the vertical axis, radians.
    pub yaw: f32,
    /// Rotation about the horizontal axis, radians.
    pub pitch: f32,
    /// Viewer distance in pixels: smaller = stronger perspective.
    pub dist: f32,
}

/// Horizontal text alignment relative to the x coordinate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// x is the left edge of the text.
    Left,
    /// x is the horizontal centre of the text.
    Center,
    /// x is the right edge of the text.
    Right,
}

/// sRGB hex + alpha to a UI colour.
pub fn rgba(hex: u32, a: f32) -> [f32; 4] {
    [
        ((hex >> 16) & 255) as f32 / 255.0,
        ((hex >> 8) & 255) as f32 / 255.0,
        (hex & 255) as f32 / 255.0,
        a,
    ]
}

/// GPU uniform payload for UI gradients (matches `ui.wgsl` `U`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UiUniform {
    /// `[gamma, pad, pad, pad]`.
    pub gamma: [f32; 4],
    /// Gradient line angle, degrees.
    pub grad_angle: f32,
    /// Active stop count.
    pub grad_count: f32,
    /// Reserved.
    pub grad_pad: f32,
    /// Reserved.
    pub grad_pad2: f32,
    /// Each stop: `(pos, r, g, b)` in linear 0..1.
    pub grad_stop0: [f32; 4],
    /// Stop 1.
    pub grad_stop1: [f32; 4],
    /// Stop 2.
    pub grad_stop2: [f32; 4],
    /// Stop 3.
    pub grad_stop3: [f32; 4],
}

/// Immediate-mode UI builder. Submit shapes and text each frame, then the renderer
/// uploads `verts` and the glyph atlas.
pub struct Ui {
    /// Vertices submitted this frame.
    pub verts: Vec<UiVertex>,
    /// Target size in pixels, used to map positions to clip space.
    pub size: Vec2,
    /// Font atlas shared by all text.
    pub atlas: FontAtlas,
    tilt: Option<(Tilt, Mat3)>,
    /// Horizontal squash applied to text (emulates a condensed cut of the face).
    pub squash: f32,
    /// Shear applied to shapes and text: x += shear * (y - top).
    pub shear: f32,
    /// Uniform written with the current UI draw.
    pub uniform: UiUniform,
}

impl Ui {
    /// Empty UI with no fonts and no tilt.
    pub fn new() -> Self {
        Ui {
            verts: Vec::with_capacity(MAX_UI_VERTS),
            size: Vec2::ONE,
            atlas: FontAtlas::new(),
            tilt: None,
            squash: 1.0,
            shear: 0.0,
            uniform: UiUniform::zeroed(),
        }
    }

    /// Sets gradient parameters for following mode-5 quads.
    pub fn set_linear_gradient_uniform(&mut self, angle_deg: f32, stops: &[(f32, [f32; 4])]) {
        let mut uniform = self.uniform;
        uniform.grad_angle = angle_deg;
        uniform.grad_count = stops.len() as f32;
        let mut pack = [[0.0; 4]; 4];
        for (slot, (pos, color)) in pack.iter_mut().zip(stops.iter()) {
            *slot = [*pos, color[0], color[1], color[2]];
        }
        uniform.grad_stop0 = pack[0];
        uniform.grad_stop1 = pack[1];
        uniform.grad_stop2 = pack[2];
        uniform.grad_stop3 = pack[3];
        self.uniform = uniform;
    }

    /// Filled rounded rectangle with a linear gradient (shader mode 5).
    pub fn linear_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color: [f32; 4],
    ) {
        self.shape(x, y, w, h, color, [radius, 0.0, 5.0, 0.0], 1.0);
    }

    /// Sets (or with `None`, clears) the tilt applied to following shapes.
    pub fn set_tilt(&mut self, t: Option<Tilt>) {
        self.tilt = t.map(|t| {
            (
                t,
                Mat3::from_rotation_y(t.yaw) * Mat3::from_rotation_x(t.pitch),
            )
        });
    }

    fn project(&self, p: Vec2) -> [f32; 4] {
        let (w, h) = (self.size.x, self.size.y);
        match self.tilt {
            None => [p.x / w * 2.0 - 1.0, 1.0 - p.y / h * 2.0, 0.0, 1.0],
            Some((t, m)) => {
                let q = m * Vec3::new(p.x - t.pivot.x, p.y - t.pivot.y, 0.0);
                let ww = 1.0 + q.z / t.dist;
                let sx = t.pivot.x * ww + q.x;
                let sy = t.pivot.y * ww + q.y;
                [sx / w * 2.0 - ww, ww - sy / h * 2.0, 0.0, ww]
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the vertex layout (corners, uv, half, colour, params, pad); a struct would only move the fields.
    fn quad(
        &mut self,
        corners: [Vec2; 4],
        uv: [f32; 4],
        half: Vec2,
        color: [f32; 4],
        p: [f32; 4],
        pad: f32,
    ) {
        if self.verts.len() + 6 > MAX_UI_VERTS {
            return;
        }
        let locals = [
            Vec2::new(-half.x - pad, -half.y - pad),
            Vec2::new(half.x + pad, -half.y - pad),
            Vec2::new(half.x + pad, half.y + pad),
            Vec2::new(-half.x - pad, half.y + pad),
        ];
        let uvs = [
            [uv[0], uv[1]],
            [uv[2], uv[1]],
            [uv[2], uv[3]],
            [uv[0], uv[3]],
        ];
        let mut v = [UiVertex::zeroed(); 4];
        for (((out, &corner), &local), &uvc) in v
            .iter_mut()
            .zip(corners.iter())
            .zip(locals.iter())
            .zip(uvs.iter())
        {
            *out = UiVertex {
                pos: self.project(corner),
                uv: uvc,
                local: local.to_array(),
                half: half.to_array(),
                color,
                p,
            };
        }
        self.verts
            .extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    }

    #[allow(clippy::too_many_arguments)] // Position, size, colour, shape parameters and pad are all needed; grouping them would add a type for one caller.
    fn shape(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], p: [f32; 4], pad: f32) {
        let half = Vec2::new(w * 0.5, h * 0.5);
        let (x0, y0, x1, y1) = (x - pad, y - pad, x + w + pad, y + h + pad);
        // Shear leans the top to the right (italic): offset grows toward the top.
        let sh = self.shear;
        let off = |yy: f32| sh * (y + h - yy);
        let c = [
            Vec2::new(x0 + off(y0), y0),
            Vec2::new(x1 + off(y0), y0),
            Vec2::new(x1 + off(y1), y1),
            Vec2::new(x0 + off(y1), y1),
        ];
        self.quad(c, [0.0; 4], half, color, p, pad);
    }

    /// Filled rounded rectangle.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: [f32; 4]) {
        self.shape(x, y, w, h, color, [radius, 0.0, 0.0, 0.0], 1.0);
    }

    /// Outline of a rounded rectangle, `width` px thick (inside the box).
    #[allow(clippy::too_many_arguments)] // The rectangle, radius, width and colour are the public call's parameters.
    pub fn stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        width: f32,
        color: [f32; 4],
    ) {
        self.shape(x, y, w, h, color, [radius, width, 2.0, 0.0], 1.0);
    }

    /// Soft glow / drop shadow around a rectangle.
    #[allow(clippy::too_many_arguments)] // The rectangle, radius, blur and colour are the public call's parameters.
    pub fn glow(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        blur: f32,
        color: [f32; 4],
    ) {
        self.shape(x, y, w, h, color, [radius, 0.0, 4.0, blur], blur + 1.0);
    }

    /// Ring or arc centred at (cx, cy). `frac` 0..1 fills clockwise from 12 o'clock.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, thick: f32, frac: f32, color: [f32; 4]) {
        let half = Vec2::splat(r);
        let c = [
            Vec2::new(cx - r - 1.0, cy - r - 1.0),
            Vec2::new(cx + r + 1.0, cy - r - 1.0),
            Vec2::new(cx + r + 1.0, cy + r + 1.0),
            Vec2::new(cx - r - 1.0, cy + r + 1.0),
        ];
        self.quad(c, [0.0; 4], half, color, [0.0, thick, 3.0, frac], 1.0);
    }

    /// Filled circle.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: [f32; 4]) {
        let s = self.shear;
        self.shear = 0.0;
        self.rect(cx - r, cy - r, r * 2.0, r * 2.0, r, color);
        self.shear = s;
    }

    /// Line segment as a thin rotated rect (not tilted-aware for width, fine for HUD marks).
    pub fn line(&mut self, a: Vec2, b: Vec2, width: f32, color: [f32; 4]) {
        let d = b - a;
        let len = d.length().max(1e-3);
        let n = Vec2::new(-d.y, d.x) / len * (width * 0.5 + 1.0);
        let c = [a - n, b - n, b + n, a + n];
        let half = Vec2::new(len * 0.5, width * 0.5);
        // local x runs along the segment: map corners accordingly.
        self.quad(c, [0.0; 4], half, color, [width * 0.5, 0.0, 0.0, 0.0], 1.0);
    }

    /// Draw text with its top-left at (x, y) (or aligned around x). Returns the width.
    #[allow(clippy::too_many_arguments)] // Font, position, size, colour, alignment and string are the call's parameters.
    pub fn text(
        &mut self,
        font: FontId,
        x: f32,
        y: f32,
        px: f32,
        color: [f32; 4],
        align: Align,
        s: &str,
    ) -> f32 {
        if !self.atlas.has_fonts() {
            return 0.0;
        }
        let sq = self.squash;
        let w = self.atlas.measure(font, px, s) * sq;
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - w * 0.5,
            Align::Right => x - w,
        };
        let base = y + self.atlas.ascent(font, px);
        let mut prev: Option<char> = None;
        let sh = self.shear;
        for c in s.chars() {
            if let Some(p) = prev {
                pen += self.atlas.kern(font, p, c, px) * sq;
            }
            let g = self.atlas.glyph(font, c, px);
            if g.size[0] > 0.0 {
                let gx = pen + g.off[0] * sq;
                let gy = base + g.off[1];
                let (gw, gh) = (g.size[0] * sq, g.size[1]);
                let sk = |yy: f32| sh * (base - yy);
                let cs = [
                    Vec2::new(gx + sk(gy), gy),
                    Vec2::new(gx + gw + sk(gy), gy),
                    Vec2::new(gx + gw + sk(gy + gh), gy + gh),
                    Vec2::new(gx + sk(gy + gh), gy + gh),
                ];
                self.quad(cs, g.uv, Vec2::ONE, color, [0.0, 0.0, 1.0, 0.0], 0.0);
            }
            pen += g.advance * sq;
            prev = Some(c);
        }
        w
    }

    /// Width of `s` in pixels at `px`, including the current squash. 0 without fonts.
    pub fn measure(&mut self, font: FontId, px: f32, s: &str) -> f32 {
        if !self.atlas.has_fonts() {
            return 0.0;
        }
        self.atlas.measure(font, px, s) * self.squash
    }

    /// Clears the vertices and resets tilt, shear and squash for the next frame.
    pub fn clear(&mut self) {
        self.verts.clear();
        self.tilt = None;
        self.shear = 0.0;
        self.squash = 1.0;
        self.uniform = UiUniform::zeroed();
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}
