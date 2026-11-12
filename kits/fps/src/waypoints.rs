//! Waypoints and their link graph: the places bots patrol and route between.
//!
//! [`Waypoints::build`] links two waypoints when a bot could walk between them: the segment is
//! clear at knee and head height across the bot's width, the vertical step is one a bot can walk
//! up (a jumpable ledge is not a link), and the distance is within the link range. It then
//! precomputes, for every pair of waypoints, the next waypoint on a shortest hop path, so a bot
//! routes with one table lookup and no allocation. A graph that is not connected is rejected.
//!
//! Invariants: every point is finite; there are at most `u16::MAX` waypoints; the graph is
//! undirected and connected once built; `next_hop(a, b)` is `Some` for every pair of waypoints,
//! is `b` when `a == b`, and otherwise is a linked neighbour of `a` one hop nearer to `b`.

use std::collections::VecDeque;

use engine_core::{SimRng, StateHasher};
use engine_world::Resource;
use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::bot::Senses;
use crate::error::KitError;
use crate::validate::{finite_vec3, non_negative, positive};

/// The number of hops a waypoint is unreachable at, while the table is being built.
const UNREACHED: u32 = u32::MAX;

/// Index of a waypoint in the list [`Waypoints`] was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WaypointId(u16);

impl WaypointId {
    /// The first waypoint of a list.
    pub const FIRST: Self = Self(0);

    /// The position of this waypoint in the list, as a `usize` for indexing.
    #[must_use]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// The plain numbers a game passes in to decide which waypoints link. [`LinkRules::new`]
/// validates them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinkParams {
    /// Half the width a bot needs, in metres: links are tested on rays this far to each side.
    pub clearance_radius: f32,
    /// Height of the lower sight line above a waypoint, in metres.
    pub knee_height: f32,
    /// Height of the upper sight line above a waypoint, in metres, above the knee height.
    pub head_height: f32,
    /// Highest vertical step a bot walks up or down, in metres. Taller steps are not links.
    pub max_step: f32,
    /// Longest link, in metres.
    pub max_link: f32,
}

/// Validated link rules. Built only from [`LinkParams`], including when decoded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LinkParams")]
pub struct LinkRules {
    clearance_radius: f32,
    knee_height: f32,
    head_height: f32,
    max_step: f32,
    max_link: f32,
}

impl TryFrom<LinkParams> for LinkRules {
    type Error = KitError;

    fn try_from(params: LinkParams) -> Result<Self, Self::Error> {
        let knee_height = positive(params.knee_height, "waypoint knee height")?;
        let head_height = positive(params.head_height, "waypoint head height")?;
        if head_height <= knee_height {
            return Err(KitError::OutOfRange {
                field: "waypoint head height",
                requirement: "above the knee height",
                value: head_height,
            });
        }
        Ok(Self {
            clearance_radius: positive(params.clearance_radius, "waypoint clearance radius")?,
            knee_height,
            head_height,
            max_step: non_negative(params.max_step, "waypoint max step")?,
            max_link: positive(params.max_link, "waypoint max link")?,
        })
    }
}

impl LinkRules {
    /// Validates the parameters into link rules.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] naming the first field that is not finite, not in range, or a head
    /// height that is not above the knee height.
    pub fn new(params: LinkParams) -> Result<Self, KitError> {
        Self::try_from(params)
    }

    /// Half the width a bot needs, in metres.
    #[must_use]
    pub const fn clearance_radius(&self) -> f32 {
        self.clearance_radius
    }

    /// Height of the lower sight line above a waypoint, in metres.
    #[must_use]
    pub const fn knee_height(&self) -> f32 {
        self.knee_height
    }

    /// Height of the upper sight line above a waypoint, in metres.
    #[must_use]
    pub const fn head_height(&self) -> f32 {
        self.head_height
    }

    /// Highest walkable vertical step, in metres.
    #[must_use]
    pub const fn max_step(&self) -> f32 {
        self.max_step
    }

    /// Longest link, in metres.
    #[must_use]
    pub const fn max_link(&self) -> f32 {
        self.max_link
    }
}

/// The waypoints a bot patrols and routes between, with their link graph. Owned by the world as
/// a resource.
#[derive(Debug, Clone, PartialEq)]
pub struct Waypoints {
    points: Vec<Vec3>,
    count: u16,
    rules: LinkRules,
    /// `next[to * count + from]` is the next waypoint from `from` towards `to`.
    next: Vec<WaypointId>,
}

impl Waypoints {
    /// Builds the waypoints and their link graph. The list may be empty; a world with bots must
    /// not be.
    ///
    /// A link needs a clear sight line at knee and head height, on the centre and on both sides
    /// of the bot's width; a vertical step within `max_step`; and a length within `max_link`.
    /// Building costs one clear-line test per link candidate, so it runs at setup, not per tick.
    ///
    /// # Errors
    ///
    /// Returns [`KitError::NonFiniteVector`] for a non-finite point, [`KitError::TooManyWaypoints`]
    /// for more than `u16::MAX` points, and [`KitError::Disconnected`] naming the first waypoint
    /// that cannot be reached from the first one.
    pub fn build<S: Senses>(
        points: Vec<Vec3>,
        senses: &S,
        rules: &LinkRules,
    ) -> Result<Self, KitError> {
        for point in &points {
            finite_vec3(*point, "waypoint")?;
        }
        let count = u16::try_from(points.len()).map_err(|_| KitError::TooManyWaypoints {
            count: points.len(),
            max: usize::from(u16::MAX),
        })?;
        let neighbours = link_neighbours(&points, senses, rules)?;
        let next = next_hops(&neighbours)?;
        Ok(Self {
            points,
            count,
            rules: *rules,
            next,
        })
    }

    /// The waypoints in list order.
    #[must_use]
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    /// The number of waypoints.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.count)
    }

    /// Whether there are no waypoints.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// A waypoint chosen uniformly by `rng`, or `None` when there are no waypoints.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when the RNG rejects the range, which cannot happen for a non-empty
    /// list.
    pub fn pick(&self, rng: &mut SimRng) -> Result<Option<WaypointId>, KitError> {
        if self.is_empty() {
            return Ok(None);
        }
        let drawn = rng.range_u32(0, u32::from(self.count))?;
        let index = u16::try_from(drawn).map_err(|_| KitError::TooManyWaypoints {
            count: self.len(),
            max: usize::from(u16::MAX),
        })?;
        Ok(Some(WaypointId(index)))
    }

    /// The rules the links were built with.
    #[must_use]
    pub const fn rules(&self) -> &LinkRules {
        &self.rules
    }

    /// The identifier of the waypoint at `index` in the list, if there is one.
    #[must_use]
    pub fn id(&self, index: usize) -> Option<WaypointId> {
        match u16::try_from(index) {
            Ok(value) if index < self.len() => Some(WaypointId(value)),
            _ => None,
        }
    }

    /// The position of a waypoint, if the identifier names one of these waypoints.
    #[must_use]
    pub fn point(&self, id: WaypointId) -> Option<Vec3> {
        self.points.get(id.index()).copied()
    }

    /// The waypoint after `id` in list order, looping back to the first after the last.
    #[must_use]
    pub fn following(&self, id: WaypointId) -> Option<WaypointId> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        self.id((id.index() + 1) % len)
    }

    /// The waypoint nearest `point` by horizontal distance; ties keep the lower index.
    #[must_use]
    pub fn nearest_to(&self, point: Vec3) -> Option<WaypointId> {
        let mut best: Option<(f32, WaypointId)> = None;
        for (index, waypoint) in self.points.iter().enumerate() {
            let distance = horizontal(*waypoint - point).length_squared();
            if best.is_some_and(|(closest, _)| distance >= closest) {
                continue;
            }
            if let Some(id) = self.id(index) {
                best = Some((distance, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// The next waypoint on a shortest hop path from `from` to `to`. It is `to` itself when the
    /// two are the same, and `None` only when either identifier names no waypoint.
    #[must_use]
    pub fn next_hop(&self, from: WaypointId, to: WaypointId) -> Option<WaypointId> {
        let len = self.len();
        if from.index() >= len || to.index() >= len {
            return None;
        }
        self.next.get(to.index() * len + from.index()).copied()
    }
}

impl Resource for Waypoints {
    fn hash_into(&self, hasher: &mut StateHasher) {
        hasher.write_u32(u32::from(self.count));
        for point in &self.points {
            hasher.write_f32(point.x);
            hasher.write_f32(point.y);
            hasher.write_f32(point.z);
        }
        for hop in &self.next {
            hasher.write_u64(u64::from(hop.0));
        }
    }
}

/// The horizontal part of a vector: its `x` and `z` components, with `y` zeroed.
pub(crate) fn horizontal(vector: Vec3) -> Vec3 {
    Vec3::new(vector.x, 0.0, vector.z)
}

/// The waypoint identifier for a list position, when it fits in a `u16`.
fn id_of(index: usize) -> Result<WaypointId, KitError> {
    u16::try_from(index)
        .map(WaypointId)
        .map_err(|_| KitError::TooManyWaypoints {
            count: index + 1,
            max: usize::from(u16::MAX),
        })
}

/// The linked neighbours of every waypoint, each list in ascending index order.
fn link_neighbours<S: Senses>(
    points: &[Vec3],
    senses: &S,
    rules: &LinkRules,
) -> Result<Vec<Vec<usize>>, KitError> {
    let count = points.len();
    let mut linked = vec![vec![false; count]; count];
    for (i, from) in points.iter().enumerate() {
        for (j, to) in points.iter().enumerate().skip(i + 1) {
            if link_clear(*from, *to, senses, rules)? {
                if let Some(row) = linked.get_mut(i)
                    && let Some(cell) = row.get_mut(j)
                {
                    *cell = true;
                }
                if let Some(row) = linked.get_mut(j)
                    && let Some(cell) = row.get_mut(i)
                {
                    *cell = true;
                }
            }
        }
    }
    Ok(linked
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .filter(|(_, is_linked)| **is_linked)
                .map(|(index, _)| index)
                .collect()
        })
        .collect())
}

/// Whether a bot can walk the segment from `from` to `to`: within the link length, the step
/// within `max_step`, and clear at knee and head height on the centre line and both sides.
fn link_clear<S: Senses>(
    from: Vec3,
    to: Vec3,
    senses: &S,
    rules: &LinkRules,
) -> Result<bool, KitError> {
    let offset = to - from;
    if offset.length() > rules.max_link() || (to.y - from.y).abs() > rules.max_step() {
        return Ok(false);
    }
    let side = Vec3::new(-offset.z, 0.0, offset.x).normalize_or_zero() * rules.clearance_radius();
    for height in [rules.knee_height(), rules.head_height()] {
        let lift = Vec3::Y * height;
        for across in [-1.0_f32, 0.0, 1.0] {
            let shift = side * across + lift;
            if !senses.clear(None, from + shift, to + shift)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The next-hop table, column by column: column `to` holds, for every `from`, the next waypoint
/// towards `to`. Breadth-first search from each target gives hop counts; ties pick the lowest
/// index. Fails when the graph is not connected, naming the first waypoint unreachable from 0.
fn next_hops(neighbours: &[Vec<usize>]) -> Result<Vec<WaypointId>, KitError> {
    let count = neighbours.len();
    let mut table = Vec::with_capacity(count * count);
    for target in 0..count {
        let hops = hop_counts(neighbours, target);
        if target == 0
            && let Some(unreached) = hops.iter().position(|hops| *hops == UNREACHED)
        {
            return Err(KitError::Disconnected {
                waypoint: unreached,
            });
        }
        for from in 0..count {
            let hop = if from == target {
                target
            } else {
                let from_hops = hops.get(from).copied().unwrap_or(UNREACHED);
                neighbours
                    .get(from)
                    .into_iter()
                    .flatten()
                    .copied()
                    .find(|&next| {
                        hops.get(next)
                            .is_some_and(|&hops| hops.checked_add(1) == Some(from_hops))
                    })
                    .ok_or(KitError::Disconnected { waypoint: from })?
            };
            table.push(id_of(hop)?);
        }
    }
    Ok(table)
}

/// Hop counts from `source` to every waypoint, [`UNREACHED`] where there is no path.
fn hop_counts(neighbours: &[Vec<usize>], source: usize) -> Vec<u32> {
    let mut hops = vec![UNREACHED; neighbours.len()];
    let mut queue = VecDeque::new();
    if let Some(start) = hops.get_mut(source) {
        *start = 0;
        queue.push_back(source);
    }
    while let Some(node) = queue.pop_front() {
        let here = hops.get(node).copied().unwrap_or(UNREACHED);
        for &next in neighbours.get(node).into_iter().flatten() {
            if let Some(slot) = hops.get_mut(next)
                && *slot == UNREACHED
            {
                *slot = here + 1;
                queue.push_back(next);
            }
        }
    }
    hops
}

#[cfg(test)]
pub(crate) mod test_links {
    //! A link table for tests: waypoints are given by position, and a sight line is clear exactly
    //! when the two waypoints nearest its ends are linked in the table.

    use super::*;
    use engine_physics::BodyId;

    /// Scripted links between waypoints placed on the x axis.
    pub(crate) struct LinkTable {
        pub(crate) points: Vec<Vec3>,
        pub(crate) linked: Vec<Vec<bool>>,
    }

    impl LinkTable {
        /// Waypoints at the given points, linked as the symmetric matrix says.
        pub(crate) fn new(points: Vec<Vec3>, linked: Vec<Vec<bool>>) -> Self {
            Self { points, linked }
        }

        /// The index of the waypoint nearest `point` by horizontal distance.
        fn nearest_index(&self, point: Vec3) -> Option<usize> {
            let mut best: Option<(f32, usize)> = None;
            for (index, waypoint) in self.points.iter().enumerate() {
                let distance = horizontal(*waypoint - point).length_squared();
                if best.is_none_or(|(closest, _)| distance < closest) {
                    best = Some((distance, index));
                }
            }
            best.map(|(_, index)| index)
        }
    }

    impl Senses for LinkTable {
        fn can_see(
            &self,
            _observer: BodyId,
            _eye: Vec3,
            _target: BodyId,
            _target_eye: Vec3,
        ) -> Result<bool, KitError> {
            Ok(false)
        }

        fn blocked(
            &self,
            _observer: BodyId,
            _origin: Vec3,
            _direction: Vec3,
            _range: f32,
        ) -> Result<bool, KitError> {
            Ok(false)
        }

        fn clear(&self, _observer: Option<BodyId>, from: Vec3, to: Vec3) -> Result<bool, KitError> {
            let (Some(a), Some(b)) = (self.nearest_index(from), self.nearest_index(to)) else {
                return Ok(false);
            };
            if a == b {
                return Ok(true);
            }
            Ok(self
                .linked
                .get(a)
                .and_then(|row| row.get(b))
                .copied()
                .unwrap_or(false))
        }
    }

    /// Link rules that accept any step and length, so only the scripted links decide.
    pub(crate) fn open_rules() -> Result<LinkRules, KitError> {
        LinkRules::new(LinkParams {
            clearance_radius: 0.3,
            knee_height: 0.5,
            head_height: 1.5,
            max_step: 0.4,
            max_link: 1000.0,
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::needless_range_loop,
    reason = "the reference search and the matrix generator index matrices they size themselves"
)]
mod tests {
    use super::test_links::{LinkTable, open_rules};
    use super::*;
    use proptest::prelude::*;

    /// Points on the x axis, ten metres apart.
    fn line(count: usize) -> Vec<Vec3> {
        (0..count)
            .map(|index| Vec3::new(index as f32 * 10.0, 0.0, 0.0))
            .collect()
    }

    #[test]
    fn link_rules_reject_a_head_below_the_knee() {
        let bad = LinkParams {
            clearance_radius: 0.3,
            knee_height: 1.0,
            head_height: 0.9,
            max_step: 0.4,
            max_link: 25.0,
        };
        assert!(LinkRules::new(bad).is_err());
    }

    #[test]
    fn a_chain_routes_hop_by_hop() -> Result<(), Box<dyn std::error::Error>> {
        // 0 - 1 - 2: no link between 0 and 2, so 0 reaches 2 through 1.
        let linked = vec![
            vec![false, true, false],
            vec![true, false, true],
            vec![false, true, false],
        ];
        let table = LinkTable::new(line(3), linked);
        let nav = Waypoints::build(line(3), &table, &open_rules()?)?;
        let (a, b, c) = (
            WaypointId::FIRST,
            nav.id(1).ok_or("waypoint 1")?,
            nav.id(2).ok_or("waypoint 2")?,
        );
        assert_eq!(nav.next_hop(a, c), Some(b));
        assert_eq!(nav.next_hop(c, a), Some(b));
        assert_eq!(nav.next_hop(a, a), Some(a));
        assert_eq!(nav.next_hop(a, b), Some(b));
        Ok(())
    }

    #[test]
    fn a_disconnected_graph_is_rejected_naming_the_unreachable_waypoint()
    -> Result<(), Box<dyn std::error::Error>> {
        // 0 - 1, and 2 alone.
        let linked = vec![
            vec![false, true, false],
            vec![true, false, false],
            vec![false, false, false],
        ];
        let table = LinkTable::new(line(3), linked);
        let result = Waypoints::build(line(3), &table, &open_rules()?);
        if let Err(KitError::Disconnected { waypoint }) = result {
            assert_eq!(waypoint, 2);
            Ok(())
        } else {
            Err("expected a disconnected error".into())
        }
    }

    #[test]
    fn too_many_waypoints_are_rejected_before_any_link_test() -> Result<(), KitError> {
        let table = LinkTable::new(Vec::new(), Vec::new());
        let points = vec![Vec3::ZERO; usize::from(u16::MAX) + 1];
        let result = Waypoints::build(points, &table, &open_rules()?);
        assert!(matches!(result, Err(KitError::TooManyWaypoints { .. })));
        Ok(())
    }

    #[test]
    fn non_finite_points_are_rejected() -> Result<(), KitError> {
        let table = LinkTable::new(Vec::new(), Vec::new());
        let points = vec![Vec3::new(f32::NAN, 0.0, 0.0)];
        assert!(Waypoints::build(points, &table, &open_rules()?).is_err());
        Ok(())
    }

    #[test]
    fn an_empty_list_builds_an_empty_graph() -> Result<(), KitError> {
        let table = LinkTable::new(Vec::new(), Vec::new());
        let nav = Waypoints::build(Vec::new(), &table, &open_rules()?)?;
        assert!(nav.is_empty());
        assert!(nav.nearest_to(Vec3::ZERO).is_none());
        Ok(())
    }

    #[test]
    fn following_wraps_back_to_the_first_waypoint() -> Result<(), Box<dyn std::error::Error>> {
        let table = LinkTable::new(line(2), vec![vec![false, true], vec![true, false]]);
        let nav = Waypoints::build(line(2), &table, &open_rules()?)?;
        let last = nav.id(1).ok_or("waypoint 1")?;
        assert_eq!(nav.following(last), Some(WaypointId::FIRST));
        assert_eq!(nav.following(WaypointId::FIRST), Some(last));
        Ok(())
    }

    /// Reference hop counts by Floyd-Warshall, independent of the breadth-first search the
    /// table uses.
    fn reference_hops(linked: &[Vec<bool>]) -> Vec<Vec<Option<u32>>> {
        let count = linked.len();
        let mut hops = vec![vec![None; count]; count];
        for (i, row) in linked.iter().enumerate() {
            for (j, is_linked) in row.iter().enumerate() {
                if i == j {
                    hops[i][j] = Some(0);
                } else if *is_linked {
                    hops[i][j] = Some(1);
                }
            }
        }
        for k in 0..count {
            for i in 0..count {
                for j in 0..count {
                    if let (Some(a), Some(b)) = (hops[i][k], hops[k][j]) {
                        let through = a + b;
                        if hops[i][j].is_none_or(|current| through < current) {
                            hops[i][j] = Some(through);
                        }
                    }
                }
            }
        }
        hops
    }

    /// Random symmetric link matrices over 1 to 8 waypoints.
    fn random_links() -> impl Strategy<Value = Vec<Vec<bool>>> {
        (1_usize..=8).prop_flat_map(|count| {
            proptest::collection::vec(any::<bool>(), count * count).prop_map(move |bits| {
                let mut linked = vec![vec![false; count]; count];
                for i in 0..count {
                    for j in (i + 1)..count {
                        let bit = bits.get(i * count + j).copied().unwrap_or(false);
                        linked[i][j] = bit;
                        linked[j][i] = bit;
                    }
                }
                linked
            })
        })
    }

    proptest! {
        #[test]
        fn next_hops_match_a_reference_search(linked in random_links()) {
            let count = linked.len();
            let points = line(count);
            let reference = reference_hops(&linked);
            let table = LinkTable::new(points.clone(), linked.clone());
            let connected = reference.iter().all(|row| row.iter().all(Option::is_some));
            let rules = open_rules().map_err(|error| TestCaseError::fail(error.to_string()))?;
            match Waypoints::build(points, &table, &rules) {
                Ok(nav) => {
                    prop_assert!(connected, "a disconnected graph was accepted");
                    for from in 0..count {
                        for to in 0..count {
                            let (Some(a), Some(b)) = (nav.id(from), nav.id(to)) else {
                                return Err(TestCaseError::fail("waypoint id out of range"));
                            };
                            let hop = nav.next_hop(a, b).map(WaypointId::index);
                            if from == to {
                                prop_assert_eq!(hop, Some(to));
                                continue;
                            }
                            let Some(hop) = hop else {
                                return Err(TestCaseError::fail("no next hop for a pair"));
                            };
                            prop_assert!(linked[from][hop], "hop {hop} is not linked to {from}");
                            let remaining = reference[hop][to];
                            let here = reference[from][to];
                            prop_assert_eq!(remaining.map(|r| r + 1), here);
                        }
                    }
                }
                Err(KitError::Disconnected { waypoint }) => {
                    prop_assert!(!connected, "a connected graph was rejected");
                    prop_assert!(reference[0][waypoint].is_none(), "waypoint {waypoint} is reachable");
                }
                Err(other) => return Err(TestCaseError::fail(format!("unexpected error {other}"))),
            }
        }
    }
}
