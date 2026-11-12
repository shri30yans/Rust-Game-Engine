//! Validated primitive geometry and its visual and physics properties.
use engine_assets::format::AssetId;
use serde::{Deserialize, Serialize};

use super::InvalidReason;
use super::validation::{finite, non_empty, positive, unit};

/// The geometric kind of a shape. Extents are half sizes unless stated otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    /// A box with half extents along its own axes.
    Cuboid {
        /// Half extents along X, Y and Z, in metres.
        half: [f32; 3],
    },
    /// A ball.
    Sphere {
        /// Radius, in metres.
        radius: f32,
    },
    /// A capsule along Y: a cylinder of `half_height` capped by two hemispheres of `radius`.
    Capsule {
        /// Half the straight section's length, in metres.
        half_height: f32,
        /// Radius of the caps, in metres.
        radius: f32,
    },
    /// A box tilted about its own Z axis by `tilt_deg` after the shape's rotation.
    Ramp {
        /// Half extents of the slab, in metres.
        half: [f32; 3],
        /// Tilt about Z, in degrees.
        tilt_deg: f32,
    },
}

impl ShapeKind {
    /// Checks every number of the kind.
    fn validate(self) -> Result<(), InvalidReason> {
        match self {
            Self::Cuboid { half } => positive("half", &half),
            Self::Sphere { radius } => positive("radius", &[radius]),
            Self::Capsule {
                half_height,
                radius,
            } => {
                positive("half_height", &[half_height])?;
                positive("radius", &[radius])
            }
            Self::Ramp { half, tilt_deg } => {
                positive("half", &half)?;
                finite("tilt_deg", &[tilt_deg])
            }
        }
    }

    /// The full size of the shape along its own axes, in metres. This is the scale of the unit
    /// mesh that draws it.
    #[must_use]
    pub fn full_size(self) -> [f32; 3] {
        match self {
            Self::Cuboid { half } | Self::Ramp { half, .. } => half.map(|h| h * 2.0),
            Self::Sphere { radius } => [radius * 2.0; 3],
            Self::Capsule {
                half_height,
                radius,
            } => [radius * 2.0, (half_height + radius) * 2.0, radius * 2.0],
        }
    }

    /// The enclosed volume, in cubic metres. Used to turn a mass into a density.
    #[must_use]
    pub fn volume(self) -> f32 {
        use std::f32::consts::PI;
        match self {
            Self::Cuboid { half } | Self::Ramp { half, .. } => 8.0 * half[0] * half[1] * half[2],
            Self::Sphere { radius } => 4.0 / 3.0 * PI * radius * radius * radius,
            Self::Capsule {
                half_height,
                radius,
            } => {
                PI * radius * radius * (2.0 * half_height)
                    + 4.0 / 3.0 * PI * radius * radius * radius
            }
        }
    }
}

/// How a shape takes part in the simulation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BodyDef {
    /// Never moves.
    Static,
    /// Moved by forces. The mass fixes its density from its volume.
    Dynamic {
        /// Mass in kilograms, above zero.
        mass_kg: f32,
    },
    /// Moved by setting its pose.
    Kinematic,
}

/// What a surface is made of. The renderer maps each kind to a look; the scene only stores it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MaterialKind {
    /// Ground.
    Floor,
    /// A boundary or test wall.
    Wall,
    /// Poured concrete.
    Concrete,
    /// Fired clay bricks.
    Brick,
    /// Painted or bare metal.
    Metal,
    /// Timber.
    Wood,
    /// A packing crate.
    Crate,
    /// Transparent-looking glass.
    Glass,
    /// Self-lit surface.
    Emissive,
    /// An explicit colour and surface response.
    Custom {
        /// sRGB colour as `0xRRGGBB`.
        colour_hex: u32,
        /// Roughness in `[0, 1]`.
        roughness: f32,
        /// Metallic amount in `[0, 1]`.
        metallic: f32,
    },
}

/// How a shape looks: its material and an optional sRGB tint that replaces the material colour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    /// The material.
    pub material: MaterialKind,
    /// An sRGB colour `0xRRGGBB` that replaces the material's colour, when present.
    #[serde(default)]
    pub tint: Option<u32>,
}

impl Look {
    fn validate(self) -> Result<(), InvalidReason> {
        if let Some(hex) = self.tint {
            wide_colour(hex)?;
        }
        if let MaterialKind::Custom {
            colour_hex,
            roughness,
            metallic,
        } = self.material
        {
            wide_colour(colour_hex)?;
            unit("roughness", roughness)?;
            unit("metallic", metallic)?;
        }
        Ok(())
    }
}

fn wide_colour(hex: u32) -> Result<(), InvalidReason> {
    if hex <= 0xFF_FFFF {
        Ok(())
    } else {
        Err(InvalidReason::WideColour(hex))
    }
}

/// One validated shape: geometry, pose, simulation role and look.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShapeDef {
    id: AssetId,
    name: Option<String>,
    kind: ShapeKind,
    translation: [f32; 3],
    rotation_deg: [f32; 3],
    body: BodyDef,
    look: Look,
}

impl ShapeDef {
    /// Validates the parts of a shape.
    ///
    /// # Errors
    ///
    /// Returns the first rule the parts break.
    pub fn new(
        id: AssetId,
        name: Option<String>,
        kind: ShapeKind,
        translation: [f32; 3],
        rotation_deg: [f32; 3],
        body: BodyDef,
        look: Look,
    ) -> Result<Self, InvalidReason> {
        if let Some(text) = &name {
            non_empty(text)?;
        }
        kind.validate()?;
        finite("translation", &translation)?;
        finite("rotation_deg", &rotation_deg)?;
        if let BodyDef::Dynamic { mass_kg } = body {
            positive("mass_kg", &[mass_kg])?;
        }
        look.validate()?;
        Ok(Self {
            id,
            name,
            kind,
            translation,
            rotation_deg,
            body,
            look,
        })
    }

    /// Stable authored ID, unique across every level record.
    #[must_use]
    pub const fn id(&self) -> AssetId {
        self.id
    }

    /// The shape's name, when it has one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The geometric kind.
    #[must_use]
    pub const fn kind(&self) -> ShapeKind {
        self.kind
    }

    /// The world-space centre, in metres.
    #[must_use]
    pub const fn translation(&self) -> [f32; 3] {
        self.translation
    }

    /// Rotation in degrees, applied as yaw (Y), then pitch (X), then roll (Z).
    #[must_use]
    pub const fn rotation_deg(&self) -> [f32; 3] {
        self.rotation_deg
    }

    /// The simulation role.
    #[must_use]
    pub const fn body(&self) -> BodyDef {
        self.body
    }

    /// The look.
    #[must_use]
    pub const fn look(&self) -> Look {
        self.look
    }
}

/// A shape as serde reads it. Not usable until [`ShapeDef::try_from`] accepts it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawShape {
    /// Stable authored ID, unique across every level record.
    pub id: AssetId,
    /// Optional name, unique among the shapes.
    #[serde(default)]
    pub name: Option<String>,
    /// Geometric kind.
    pub kind: ShapeKind,
    /// World-space centre.
    pub translation: [f32; 3],
    /// Rotation in degrees (yaw, pitch, roll).
    #[serde(default)]
    pub rotation_deg: [f32; 3],
    /// Simulation role.
    pub body: BodyDef,
    /// Look.
    pub look: Look,
}

impl TryFrom<RawShape> for ShapeDef {
    type Error = InvalidReason;

    fn try_from(raw: RawShape) -> Result<Self, Self::Error> {
        Self::new(
            raw.id,
            raw.name,
            raw.kind,
            raw.translation,
            raw.rotation_deg,
            raw.body,
            raw.look,
        )
    }
}
