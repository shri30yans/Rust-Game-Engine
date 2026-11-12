//! Validated capsule and movement constants for the character controller.
//!
//! Invariant: `0 < skin_width < capsule_radius`, enforced by every constructor and setter that
//! sets either value.

use crate::error::{finite, non_negative, positive};
use crate::{ConvexShape, PhysicsError, QueryFilter};

/// The skin must be thinner than the capsule radius, so the clearance capsule (the capsule inset
/// by the skin) exists.
fn skin_fits(skin_width: f32, radius: f32) -> Result<(), PhysicsError> {
    if skin_width < radius {
        Ok(())
    } else {
        Err(PhysicsError::InvalidValue {
            field: "character skin width",
            requirement: "less than the capsule radius",
            value: skin_width,
        })
    }
}

/// Validated capsule and movement constants for a character controller.
///
/// Invariant: `0 < skin_width < capsule_radius`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterConfig {
    pub(super) capsule_half_height: f32,
    pub(super) capsule_radius: f32,
    pub(super) skin_width: f32,
    pub(super) step_height: f32,
    pub(super) step_min_width: f32,
    pub(super) slope_limit_radians: f32,
    pub(super) ground_snap_distance: f32,
    pub(super) jump_speed: f32,
    pub(super) gravity: f32,
    pub(super) max_fall_speed: f32,
    pub(super) query_filter: QueryFilter,
}

impl CharacterConfig {
    /// Creates a vertical capsule with conservative movement defaults.
    ///
    /// # Errors
    /// Returns an error for a negative half-height or non-positive radius.
    pub fn capsule(half_height: f32, radius: f32) -> Result<Self, PhysicsError> {
        let _validated = ConvexShape::capsule(half_height, radius)?;
        const DEFAULT_SKIN_WIDTH: f32 = 0.01;
        skin_fits(DEFAULT_SKIN_WIDTH, radius)?;
        Ok(Self {
            capsule_half_height: half_height,
            capsule_radius: radius,
            skin_width: DEFAULT_SKIN_WIDTH,
            step_height: 0.3,
            step_min_width: 0.2,
            slope_limit_radians: 50.0_f32.to_radians(),
            ground_snap_distance: 0.15,
            jump_speed: 5.0,
            gravity: 9.81,
            max_fall_speed: 50.0,
            query_filter: QueryFilter::default(),
        })
    }

    /// Sets the separation maintained between the capsule and geometry.
    ///
    /// # Errors
    /// Returns an error unless the width is finite and positive.
    pub fn skin_width(mut self, value: f32) -> Result<Self, PhysicsError> {
        positive(value, "character skin width")?;
        skin_fits(value, self.capsule_radius)?;
        self.skin_width = value;
        Ok(self)
    }

    /// Sets automatic step-up height and minimum obstacle width.
    ///
    /// # Errors
    /// Returns an error unless height is non-negative and width is positive.
    pub fn step_up(mut self, height: f32, min_width: f32) -> Result<Self, PhysicsError> {
        non_negative(height, "character step height")?;
        positive(min_width, "character step minimum width")?;
        self.step_height = height;
        self.step_min_width = min_width;
        Ok(self)
    }

    /// Sets the maximum climbable slope angle in radians.
    ///
    /// # Errors
    /// Returns an error unless the angle is finite and strictly between zero and π/2.
    pub fn slope_limit(mut self, radians: f32) -> Result<Self, PhysicsError> {
        finite(radians, "character slope limit")?;
        if radians <= 0.0 || radians >= core::f32::consts::FRAC_PI_2 {
            return Err(PhysicsError::InvalidValue {
                field: "character slope limit",
                requirement: "greater than zero and less than pi/2",
                value: radians,
            });
        }
        self.slope_limit_radians = radians;
        Ok(self)
    }

    /// Sets the distance used to snap a descending character to ground.
    ///
    /// # Errors
    /// Returns an error unless the distance is finite and non-negative.
    pub fn ground_snap_distance(mut self, value: f32) -> Result<Self, PhysicsError> {
        non_negative(value, "character ground snap distance")?;
        self.ground_snap_distance = value;
        Ok(self)
    }

    /// Sets jump speed, downward acceleration magnitude, and terminal fall speed.
    ///
    /// # Errors
    /// Returns an error unless every value is finite and positive.
    pub fn vertical_motion(
        mut self,
        jump_speed: f32,
        gravity: f32,
        max_fall_speed: f32,
    ) -> Result<Self, PhysicsError> {
        positive(jump_speed, "character jump speed")?;
        positive(gravity, "character gravity")?;
        positive(max_fall_speed, "character maximum fall speed")?;
        self.jump_speed = jump_speed;
        self.gravity = gravity;
        self.max_fall_speed = max_fall_speed;
        Ok(self)
    }

    /// Selects the collider layers used by movement, slope and ground queries.
    #[must_use]
    pub const fn query_filter(mut self, filter: QueryFilter) -> Self {
        self.query_filter = filter;
        self
    }

    /// Returns the validated character capsule.
    #[must_use]
    pub fn shape(self) -> ConvexShape {
        ConvexShape::valid_capsule(self.capsule_half_height, self.capsule_radius)
    }
}
