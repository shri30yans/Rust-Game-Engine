//! Simulation tick counter. Wraps at `u32::MAX`; ordering uses the shortest
//! signed distance on the wrap circle, so comparisons stay correct across the
//! wrap as long as the two ticks are less than 2^31 apart.

use serde::{Deserialize, Serialize};

/// Half of the `u32` circle. Distances at or beyond this are ambiguous.
const HALF: u32 = 1 << 31;

/// A fixed-step simulation tick.
///
/// `Tick` deliberately does not implement `Ord`: on a wrapping counter the
/// ordering is only meaningful between nearby ticks, so use [`Tick::is_after`],
/// [`Tick::is_before`] and [`Tick::distance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tick(u32);

impl Tick {
    /// The first tick.
    pub const ZERO: Tick = Tick(0);

    /// Creates a tick from its raw counter value.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Tick(raw)
    }

    /// Returns the raw counter value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Returns the next tick, wrapping at `u32::MAX`.
    #[must_use]
    pub const fn next(self) -> Self {
        Tick(self.0.wrapping_add(1))
    }

    /// Returns the tick `n` steps later, wrapping at `u32::MAX`.
    #[must_use]
    pub const fn advance(self, n: u32) -> Self {
        Tick(self.0.wrapping_add(n))
    }

    /// True when `self` is strictly after `other` on the wrap circle.
    #[must_use]
    pub const fn is_after(self, other: Tick) -> bool {
        let forward = self.0.wrapping_sub(other.0);
        forward != 0 && forward < HALF
    }

    /// True when `self` is strictly before `other` on the wrap circle.
    #[must_use]
    pub const fn is_before(self, other: Tick) -> bool {
        other.is_after(self)
    }

    /// Shortest number of steps between the two ticks, in either direction.
    /// Always at most 2^31 and symmetric in its arguments.
    #[must_use]
    pub const fn distance(self, other: Tick) -> u32 {
        let forward = self.0.wrapping_sub(other.0);
        let backward = other.0.wrapping_sub(self.0);
        if forward < backward {
            forward
        } else {
            backward
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn next_wraps_at_max() {
        assert_eq!(Tick::new(u32::MAX).next(), Tick::ZERO);
        assert_eq!(Tick::ZERO.next().raw(), 1);
    }

    #[test]
    fn ordering_across_wrap() {
        let before_wrap = Tick::new(u32::MAX);
        let after_wrap = before_wrap.next();
        assert!(after_wrap.is_after(before_wrap));
        assert!(before_wrap.is_before(after_wrap));
        assert!(!before_wrap.is_after(after_wrap));
        assert!(!Tick::new(5).is_after(Tick::new(5)));
        assert!(!Tick::new(5).is_before(Tick::new(5)));
    }

    #[test]
    fn distance_is_shortest_and_symmetric() {
        assert_eq!(Tick::new(10).distance(Tick::new(4)), 6);
        assert_eq!(Tick::new(4).distance(Tick::new(10)), 6);
        assert_eq!(Tick::new(1).distance(Tick::new(u32::MAX)), 2);
        assert_eq!(Tick::new(0).distance(Tick::new(HALF)), HALF);
    }

    #[test]
    fn serde_round_trip_is_transparent() {
        let json = serde_json::to_string(&Tick::new(77)).unwrap();
        assert_eq!(json, "77");
        let back: Tick = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Tick::new(77));
    }

    proptest! {
        #[test]
        fn advance_is_ordered_within_half_circle(base in any::<u32>(), d in 1u32..HALF) {
            let a = Tick::new(base);
            let b = a.advance(d);
            prop_assert!(b.is_after(a));
            prop_assert!(a.is_before(b));
            prop_assert_eq!(a.distance(b), d);
            prop_assert_eq!(b.distance(a), d);
        }

        #[test]
        fn distance_never_exceeds_half_circle(a in any::<u32>(), b in any::<u32>()) {
            prop_assert!(Tick::new(a).distance(Tick::new(b)) <= HALF);
        }
    }
}
