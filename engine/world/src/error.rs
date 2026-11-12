//! Typed errors for engine-world. Every fallible operation returns one of these;
//! nothing in this crate panics.

use engine_core::CoreError;
use thiserror::Error;

use crate::net_id::NetId;
use crate::registry::ComponentId;

/// Errors produced by engine-world.
#[derive(Debug, Error)]
pub enum WorldError {
    /// The id does not name a live entity (freed, never bound, or from another world).
    #[error("net id {0:?} does not name a live entity")]
    StaleNetId(NetId),
    /// The requested parent link would make an entity its own ancestor.
    #[error("parenting {child:?} under {parent:?} would create a cycle")]
    Cycle {
        /// Entity that would become a child.
        child: NetId,
        /// Entity that would become its parent.
        parent: NetId,
    },
    /// The entity exists but does not carry the requested component.
    #[error("entity {0:?} has no component `{1}`")]
    MissingComponent(NetId, &'static str),
    /// A component type was registered twice.
    #[error("component type `{0}` is already registered")]
    DuplicateComponent(&'static str),
    /// A component type was used without being registered first.
    #[error("component type `{0}` is not registered")]
    UnknownComponent(&'static str),
    /// A component id does not name a registered type.
    #[error("component id {0:?} is not registered")]
    UnknownComponentId(ComponentId),
    /// The component is built into the world and cannot be removed.
    #[error("component `{0}` is built in and cannot be removed")]
    BuiltinComponent(&'static str),
    /// A resource type was inserted twice into the same world.
    #[error("resource type `{0}` is already present")]
    DuplicateResource(&'static str),
    /// A resource type was read before it was inserted.
    #[error("resource type `{0}` is not present")]
    MissingResource(&'static str),
    /// Encoded component bytes had data left over after decoding.
    #[error("trailing bytes after component `{0}`")]
    TrailingBytes(&'static str),
    /// postcard failed to encode or decode a component.
    #[error("codec failure for component `{component}`: {source}")]
    Codec {
        /// Type name of the component.
        component: &'static str,
        /// Underlying postcard error.
        #[source]
        source: postcard::Error,
    },
    /// The 32-bit index space for net ids is full.
    #[error("net id index space is exhausted")]
    NetIdExhausted,
    /// More component types were registered than a 32-bit id can name.
    #[error("too many component types registered")]
    TooManyComponents,
    /// A system with this name is already scheduled.
    #[error("system `{0}` is already scheduled")]
    DuplicateSystem(&'static str),
    /// The fixed timestep must be finite and positive.
    #[error("fixed timestep must be finite and positive, got {0}")]
    InvalidDt(f32),
    /// A transform value failed validation.
    #[error("invalid transform: {0}")]
    InvalidTransform(&'static str),
    /// A system returned an error; the schedule stops at the first failure.
    #[error("system `{system}` failed: {source}")]
    System {
        /// Name the system was scheduled under.
        system: &'static str,
        /// The error the system returned.
        #[source]
        source: Box<WorldError>,
    },
    /// An engine-core helper rejected its input.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// A system failed for a reason in a crate above the world (a physics or kit error). The
    /// source is kept whole, so the schedule reports the original error rather than a flattened text.
    #[error("{source}")]
    Downstream {
        /// The error the system returned, boxed so the world does not depend on its crate.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// An internal invariant was violated. Reaching this is a bug in engine-world.
    #[error("internal invariant violated: {0}")]
    Invariant(&'static str),
}
