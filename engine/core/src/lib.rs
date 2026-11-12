//! Engine core: the lowest engine layer. Holds the simulation tick type, the
//! seeded deterministic RNG, the platform-stable state hasher, value
//! quantisation helpers, the shared error type and the tracing setup.
//!
//! Invariants: nothing here reads the wall clock, OS entropy or global
//! mutable state (tracing setup is the only process-wide item and is
//! idempotent). Every type is plain data and serialisable where it is
//! part of simulation state. The dev-content path convention performs no I/O;
//! the one file-system access is the dev-only [`watch`] module, which reads file
//! metadata and takes its clock as an argument.
#![forbid(unsafe_code)]

pub mod content;
pub mod error;
pub mod geometry;
pub mod hash;
pub mod quantise;
pub mod rng;
pub mod slots;
pub mod tick;
pub mod trace;
pub mod watch;

pub use content::{content_pack_filename, game_content_pack_path};
pub use error::CoreError;
pub use geometry::{Rect, ScreenRect, TouchRegion, TouchRegionId, TouchShape, Viewport};
pub use hash::StateHasher;
pub use quantise::Quantiser;
pub use rng::SimRng;
pub use slots::{SlotId, Slots};
pub use tick::Tick;
pub use trace::init_tracing;
