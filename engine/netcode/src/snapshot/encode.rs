//! Component codecs and bounded transform quantisation.

use engine_world::{Component, ComponentId, NetId, Transform, World};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::error::NetError;
use crate::snapshot::TRANSFORM_ROTATION_BITS;

/// Component-specific state codec. Implementations can replace postcard with
/// quantised wire forms; the packet remains a complete state for each entity.
pub trait NetEncode: Serialize + for<'de> Deserialize<'de> {
    /// Encodes this component using the default postcard representation.
    ///
    /// # Errors
    /// Returns a wire codec error if serialization fails.
    fn net_encode(&self, out: &mut Vec<u8>) -> Result<(), NetError> {
        let buffer = core::mem::take(out);
        *out = postcard::to_extend(self, buffer).map_err(NetError::Decode)?;
        Ok(())
    }

    /// Decodes a complete component value using the default postcard representation.
    ///
    /// # Errors
    /// Returns a wire codec error if bytes are malformed or trailing bytes remain.
    fn net_decode(bytes: &[u8]) -> Result<Self, NetError>
    where
        Self: Sized,
    {
        let (value, rest) = postcard::take_from_bytes(bytes).map_err(NetError::Decode)?;
        if !rest.is_empty() {
            return Err(NetError::InvalidSnapshot("component has trailing bytes"));
        }
        Ok(value)
    }
}

type EncodeComponentFn = fn(&World, NetId, &mut Vec<u8>) -> Result<bool, NetError>;
type DecodeComponentFn = fn(&mut World, NetId, &[u8]) -> Result<(), NetError>;

#[derive(Clone, Copy)]
struct NetCodec {
    id: ComponentId,
    encode: EncodeComponentFn,
    decode: DecodeComponentFn,
}

/// Per-world custom component codecs. Unregistered types use the world's postcard ops.
#[derive(Default)]
pub struct NetCodecRegistry {
    codecs: Vec<NetCodec>,
}

impl NetCodecRegistry {
    /// Registers `T`'s `NetEncode` implementation against its world component id.
    ///
    /// # Errors
    /// Returns an error if `T` is not registered with the world or already has an override.
    pub fn register<T: Component + NetEncode>(&mut self, world: &World) -> Result<(), NetError> {
        let id = world
            .registry()
            .id_of::<T>()
            .ok_or(NetError::InvalidSnapshot(
                "net codec component is not registered",
            ))?;
        if self.codecs.iter().any(|codec| codec.id == id) {
            return Err(NetError::InvalidSnapshot("net codec already registered"));
        }
        self.codecs.push(NetCodec {
            id,
            encode: encode_registered::<T>,
            decode: decode_registered::<T>,
        });
        Ok(())
    }

    pub(super) fn encode_into(
        &self,
        world: &World,
        id: NetId,
        component: ComponentId,
        out: &mut Vec<u8>,
    ) -> Result<bool, NetError> {
        out.clear();
        if let Some(codec) = self.codecs.iter().find(|codec| codec.id == component) {
            return (codec.encode)(world, id, out);
        }
        world
            .encode_component_into(id, component, out)
            .map_err(|error| NetError::World(error.to_string()))
    }

    pub(super) fn decode(
        &self,
        world: &mut World,
        id: NetId,
        component: ComponentId,
        bytes: &[u8],
    ) -> Result<(), NetError> {
        if let Some(codec) = self.codecs.iter().find(|codec| codec.id == component) {
            return (codec.decode)(world, id, bytes);
        }
        world
            .decode_component(id, component, bytes)
            .map_err(|error| NetError::World(error.to_string()))
    }
}

/// Registers the engine ownership component as server-only data.
///
/// # Errors
/// Propagates duplicate registration or world registry errors.
pub fn register_net_owner(world: &mut World) -> Result<ComponentId, NetError> {
    world
        .register::<NetOwner>(engine_world::Replication::ServerOnly)
        .map_err(|error| NetError::World(error.to_string()))
}

fn encode_registered<T: Component + NetEncode>(
    world: &World,
    id: NetId,
    out: &mut Vec<u8>,
) -> Result<bool, NetError> {
    match world.get::<T>(id) {
        Ok(value) => {
            value.net_encode(out)?;
            Ok(true)
        }
        Err(engine_world::WorldError::MissingComponent(_, _)) => Ok(false),
        Err(error) => Err(NetError::World(error.to_string())),
    }
}

fn decode_registered<T: Component + NetEncode>(
    world: &mut World,
    id: NetId,
    bytes: &[u8],
) -> Result<(), NetError> {
    let value = T::net_decode(bytes)?;
    world
        .insert(id, value)
        .map_err(|error| NetError::World(error.to_string()))
}

/// Per-game world bounds used by the built-in transform quantiser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformBounds {
    min: Vec3,
    max: Vec3,
    bits: [u32; 3],
}

/// A spatial axis used in a typed transform precision error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformAxis {
    /// X axis.
    X,
    /// Y axis.
    Y,
    /// Z axis.
    Z,
}

/// Default position precision for character movement, in metres.
pub const DEFAULT_CHARACTER_MOVEMENT_PRECISION_M: f32 = 0.001;

const MAX_TRANSFORM_POSITION_BITS: u32 = 24;

impl TransformBounds {
    /// Builds finite, increasing bounds that meet the requested position precision.
    ///
    /// # Errors
    /// Returns an error for invalid bounds, invalid precision, or an axis requiring more than
    /// 24 wire bits.
    pub fn new(min: Vec3, max: Vec3, precision_m: f32) -> Result<Self, NetError> {
        if !min.is_finite()
            || !max.is_finite()
            || min.x >= max.x
            || min.y >= max.y
            || min.z >= max.z
        {
            return Err(NetError::InvalidTransformBounds(
                "bounds must be finite and increasing",
            ));
        }
        if !precision_m.is_finite() || precision_m <= 0.0 {
            return Err(NetError::InvalidTransformBounds(
                "precision must be finite and positive",
            ));
        }
        let bits = [
            position_bits(min.x, max.x, precision_m, TransformAxis::X)?,
            position_bits(min.y, max.y, precision_m, TransformAxis::Y)?,
            position_bits(min.z, max.z, precision_m, TransformAxis::Z)?,
        ];
        Ok(Self { min, max, bits })
    }

    fn quantisers(self) -> Result<[engine_core::Quantiser; 3], NetError> {
        let [x_bits, y_bits, z_bits] = self.bits;
        let x = engine_core::Quantiser::new(self.min.x, self.max.x, x_bits)
            .map_err(|_| NetError::InvalidSnapshot("invalid x transform bounds"))?;
        let y = engine_core::Quantiser::new(self.min.y, self.max.y, y_bits)
            .map_err(|_| NetError::InvalidSnapshot("invalid y transform bounds"))?;
        let z = engine_core::Quantiser::new(self.min.z, self.max.z, z_bits)
            .map_err(|_| NetError::InvalidSnapshot("invalid z transform bounds"))?;
        Ok([x, y, z])
    }

    /// Largest per-axis position step represented by these bounds, in metres.
    #[must_use]
    pub fn position_step_m(self) -> f64 {
        let [x_bits, y_bits, z_bits] = self.bits;
        [
            axis_step(self.min.x, self.max.x, x_bits),
            axis_step(self.min.y, self.max.y, y_bits),
            axis_step(self.min.z, self.max.z, z_bits),
        ]
        .into_iter()
        .fold(0.0, f64::max)
    }
}

fn position_bits(
    min: f32,
    max: f32,
    precision_m: f32,
    axis: TransformAxis,
) -> Result<u32, NetError> {
    let precision = f64::from(precision_m);
    // The first bit width whose step meets precision is ceil(log2(span / precision + 1));
    // checking levels directly avoids rounding around exact powers of two.
    for bits in 1..=MAX_TRANSFORM_POSITION_BITS {
        if axis_step(min, max, bits) <= precision {
            return Ok(bits);
        }
    }
    Err(NetError::TransformPrecisionImpossible { axis })
}

fn axis_step(min: f32, max: f32, bits: u32) -> f64 {
    let levels = f64::from((1_u32 << bits) - 1);
    (f64::from(max) - f64::from(min)) / levels
}

#[derive(Serialize, Deserialize)]
struct TransformWire {
    position: [u32; 3],
    omitted_rotation: u8,
    rotation: [u16; 3],
    scale: Vec3,
}

/// Encodes a transform using game bounds and smallest-three rotation storage.
///
/// # Errors
/// Returns an error for invalid bounds, malformed data, or a postcard failure.
pub fn encode_transform(
    transform: &Transform,
    bounds: TransformBounds,
    out: &mut Vec<u8>,
) -> Result<(), NetError> {
    out.clear();
    let quantisers = bounds.quantisers()?;
    let p = transform.translation();
    if p.x < bounds.min.x
        || p.x > bounds.max.x
        || p.y < bounds.min.y
        || p.y > bounds.max.y
        || p.z < bounds.min.z
        || p.z > bounds.max.z
    {
        return Err(NetError::InvalidSnapshot(
            "transform position is outside configured world bounds",
        ));
    }
    let [qx, qy, qz] = quantisers;
    let position = [qx.encode(p.x), qy.encode(p.y), qz.encode(p.z)];
    let q = transform.rotation().to_array();
    let (omitted_rotation, values) = smallest_three(q)?;
    let rotation_quantiser = engine_core::Quantiser::new(
        -QUATERNION_COMPONENT_BOUND,
        QUATERNION_COMPONENT_BOUND,
        TRANSFORM_ROTATION_BITS,
    )
    .map_err(|_| NetError::InvalidSnapshot("invalid quaternion quantisation range"))?;
    let rotation = [
        u16::try_from(rotation_quantiser.encode(values.0))
            .map_err(|_| NetError::InvalidSnapshot("rotation quantisation overflow"))?,
        u16::try_from(rotation_quantiser.encode(values.1))
            .map_err(|_| NetError::InvalidSnapshot("rotation quantisation overflow"))?,
        u16::try_from(rotation_quantiser.encode(values.2))
            .map_err(|_| NetError::InvalidSnapshot("rotation quantisation overflow"))?,
    ];
    let wire = TransformWire {
        position,
        omitted_rotation,
        rotation,
        scale: transform.scale(),
    };
    let buffer = core::mem::take(out);
    *out = postcard::to_extend(&wire, buffer).map_err(NetError::Decode)?;
    Ok(())
}

/// Decodes a quantised transform produced by [`encode_transform`].
///
/// # Errors
/// Returns an error when the packet fields or transform values are invalid.
pub fn decode_transform(bytes: &[u8], bounds: TransformBounds) -> Result<Transform, NetError> {
    let (wire, rest): (TransformWire, _) =
        postcard::take_from_bytes(bytes).map_err(NetError::Decode)?;
    if !rest.is_empty() || wire.omitted_rotation > 3 {
        return Err(NetError::InvalidSnapshot("invalid quantised transform"));
    }
    let [qx, qy, qz] = bounds.quantisers()?;
    let [px, py, pz] = wire.position;
    let p = Vec3::new(qx.decode(px), qy.decode(py), qz.decode(pz));
    let qr = engine_core::Quantiser::new(
        -QUATERNION_COMPONENT_BOUND,
        QUATERNION_COMPONENT_BOUND,
        TRANSFORM_ROTATION_BITS,
    )
    .map_err(|_| NetError::InvalidSnapshot("invalid quaternion quantisation range"))?;
    let [rx, ry, rz] = wire.rotation;
    let small = [
        qr.decode(u32::from(rx)),
        qr.decode(u32::from(ry)),
        qr.decode(u32::from(rz)),
    ];
    let q = restore_smallest_three(wire.omitted_rotation, small)?;
    Transform::new(p, q, wire.scale).map_err(|error| NetError::World(error.to_string()))
}

fn smallest_three(q: [f32; 4]) -> Result<(u8, (f32, f32, f32)), NetError> {
    let [x, y, z, w] = q;
    let (index, _) = [(0_u8, x.abs()), (1, y.abs()), (2, z.abs()), (3, w.abs())]
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .ok_or(NetError::InvalidSnapshot("empty quaternion"))?;
    let omitted = match index {
        0 => x,
        1 => y,
        2 => z,
        _ => w,
    };
    let sign = if omitted < 0.0 { -1.0 } else { 1.0 };
    let mut values = [0.0; 3];
    let mut target = 0;
    for (source, value) in [x, y, z, w].into_iter().enumerate() {
        if u8::try_from(source).ok() != Some(index) {
            if let Some(slot) = values.get_mut(target) {
                *slot = value * sign;
            }
            target += 1;
        }
    }
    let (Some(a), Some(b), Some(c)) = (values.first(), values.get(1), values.get(2)) else {
        return Err(NetError::InvalidSnapshot("invalid quaternion components"));
    };
    Ok((index, (*a, *b, *c)))
}

fn restore_smallest_three(index: u8, small: [f32; 3]) -> Result<Quat, NetError> {
    let sum = small.iter().map(|value| value * value).sum::<f32>();
    if sum > 1.01 {
        return Err(NetError::InvalidSnapshot(
            "smallest-three quaternion is out of range",
        ));
    }
    let omitted = (1.0 - sum).max(0.0).sqrt();
    let mut values = [0.0; 4];
    let mut source = 0;
    for target in 0..4 {
        if u8::try_from(target).ok() == Some(index) {
            if let Some(slot) = values.get_mut(target) {
                *slot = omitted;
            }
        } else {
            let Some(component) = small.get(source) else {
                return Err(NetError::InvalidSnapshot(
                    "invalid quaternion component count",
                ));
            };
            if let Some(slot) = values.get_mut(target) {
                *slot = *component;
            }
            source += 1;
        }
    }
    let (Some(x), Some(y), Some(z), Some(w)) =
        (values.first(), values.get(1), values.get(2), values.get(3))
    else {
        return Err(NetError::InvalidSnapshot(
            "invalid quaternion component count",
        ));
    };
    Ok(Quat::from_xyzw(*x, *y, *z, *w).normalize())
}

/// Smallest-three quaternion components lie within this absolute range.
const QUATERNION_COMPONENT_BOUND: f32 = 0.707_106_77;

/// Engine-owned owner identity used to filter owner-only components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetOwner {
    /// Client slot that owns the entity.
    pub client: crate::protocol::ClientIndex,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct LegacyTransformWire {
        position: [u16; 3],
        omitted_rotation: u8,
        rotation: [u16; 3],
        scale: Vec3,
    }

    #[test]
    fn one_transform_keeps_its_wire_size_in_a_256_metre_arena() {
        let bounds = TransformBounds::new(
            Vec3::splat(-128.0),
            Vec3::splat(128.0),
            DEFAULT_CHARACTER_MOVEMENT_PRECISION_M,
        )
        .unwrap();
        let transform = Transform::IDENTITY;
        let mut current = Vec::new();
        encode_transform(&transform, bounds, &mut current).unwrap();
        let old_position_level = engine_core::Quantiser::new(-128.0, 128.0, 16)
            .unwrap()
            .encode(0.0);
        let rotation_quantiser = engine_core::Quantiser::new(
            -QUATERNION_COMPONENT_BOUND,
            QUATERNION_COMPONENT_BOUND,
            TRANSFORM_ROTATION_BITS,
        )
        .unwrap();
        let old_rotation_level = u16::try_from(rotation_quantiser.encode(0.0)).unwrap();
        let previous = postcard::to_allocvec(&LegacyTransformWire {
            position: [u16::try_from(old_position_level).unwrap(); 3],
            omitted_rotation: 3,
            rotation: [old_rotation_level; 3],
            scale: Vec3::ONE,
        })
        .unwrap();
        assert_eq!(current.len(), 28);
        assert_eq!(previous.len(), current.len());
    }
}
