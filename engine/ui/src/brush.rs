//! Node backgrounds: a solid colour or a linear gradient drawn by the UI shader.
//!
//! Invariants: a gradient's angle is finite; it has two to four stops; each stop position is
//! finite, within `0..=1`, and not before the stop before it. [`check_gradient`] is the one rule,
//! shared by the document validator and the cooker.

use serde::{Deserialize, Serialize};

use crate::color::Color;

/// A node background: nothing, a solid colour, or a linear gradient.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub enum Background {
    /// No background.
    #[serde(rename = "None")]
    #[default]
    None,
    /// Flat fill (legacy `fill` field deserialises here).
    Solid(Color),
    /// Linear gradient across the node's box.
    LinearGradient {
        /// CSS angle in degrees: 0 points to the top, 90 to the right, clockwise.
        angle_deg: f32,
        /// Two to four stops in document order.
        stops: Vec<GradientStop>,
    },
}

/// One colour stop on a linear gradient, `0` at the start and `1` at the end.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    /// Position along the gradient axis, `0..=1`.
    pub pos: f32,
    /// Stop colour.
    pub color: Color,
}

/// Checks a gradient's angle and stops. Returns the reason when they are not usable.
///
/// # Errors
///
/// Returns a message when the angle is not finite, the stop count is not two to four, or a stop
/// position is not finite, not within `0..=1`, or before the previous stop.
pub fn check_gradient(angle_deg: f32, stops: &[GradientStop]) -> Result<(), &'static str> {
    if !angle_deg.is_finite() {
        return Err("gradient angle must be finite");
    }
    if !(2..=4).contains(&stops.len()) {
        return Err("gradient must have two to four stops");
    }
    let mut previous = 0.0_f32;
    for stop in stops {
        let pos = stop.pos;
        if !(pos.is_finite() && (0.0..=1.0).contains(&pos) && pos >= previous) {
            return Err("gradient stop positions must be within 0..=1 and in order");
        }
        previous = pos;
    }
    Ok(())
}
