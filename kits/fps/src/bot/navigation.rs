//! Navigation: the steering goal for each phase, and the route through the waypoint graph when the
//! destination is out of sight.
//!
//! Invariants: a route step is the destination itself, the next waypoint on the route when the sight
//! line to it is clear, or the waypoint the bot already stands nearest to; it is never a waypoint
//! the bot cannot reach by a link the checks proved clear.

use glam::Vec3;

use super::brain::{BotBrain, BotInput, Phase};
use super::config::BotConfig;
use super::senses::Senses;
use crate::error::KitError;
use crate::waypoints::{WaypointId, Waypoints, horizontal};

impl BotBrain {
    /// Where the bot is steering this tick: the point it walks towards now (`step`, a waypoint on
    /// the route or the destination itself) and the destination it stops at.
    ///
    /// Patrol advances to the next waypoint once the current one is within `arrive_radius`. Chase
    /// seeks a visible enemy directly and otherwise routes to the last place it was seen.
    ///
    /// # Errors
    ///
    /// Returns a [`KitError`] when a sight query fails.
    pub(super) fn goal<S: Senses>(
        &mut self,
        cfg: &BotConfig,
        input: &BotInput,
        visible: bool,
        nav: &Waypoints,
        senses: &S,
    ) -> Result<Option<Goal>, KitError> {
        match self.phase {
            Phase::Idle => Ok(None),
            Phase::Patrol { waypoint } => {
                let Some(point) = nav.point(waypoint) else {
                    return Ok(None);
                };
                let destination = if horizontal(point - input.eye).length() <= cfg.arrive_radius {
                    let Some(next) = nav.following(waypoint) else {
                        return Ok(None);
                    };
                    self.phase = Phase::Patrol { waypoint: next };
                    nav.point(next)
                } else {
                    Some(point)
                };
                match destination {
                    Some(destination) => routed(input, nav, senses, destination),
                    None => Ok(None),
                }
            }
            Phase::Chase if visible => Ok(input.target.map(|target| Goal::direct(target.eye))),
            Phase::Chase => match self.last_seen {
                Some(seen) => routed(input, nav, senses, seen),
                None => Ok(None),
            },
            Phase::Attack => Ok(input.target.map(|target| Goal::direct(target.eye))),
        }
    }
}

/// Where a bot steers in one tick: the point it walks towards now, and the destination it stops at.
/// On a route the step is a waypoint and the destination is further on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Goal {
    pub(super) step: Vec3,
    pub(super) destination: Vec3,
}

impl Goal {
    /// A goal with no route: the bot walks straight at `point` and stops there.
    const fn direct(point: Vec3) -> Self {
        Self {
            step: point,
            destination: point,
        }
    }
}

/// The point to steer towards on the way to `destination`.
///
/// The destination's nearest waypoint is the goal. The bot starts from the nearest waypoint it can
/// see, or the nearest waypoint overall when none is in view. When the two are the same the bot
/// heads straight for the destination. Otherwise it heads for the next waypoint on the route when
/// the bot can see that waypoint, and for the waypoint it starts from when it cannot. The second
/// case keeps the bot on sight lines the link checks have already proved clear: a straight walk
/// from the bot to a hop it cannot see would cut the corner of the geometry.
///
/// # Errors
///
/// Returns a [`KitError`] when a sight query fails.
pub(super) fn route<S: Senses>(
    input: &BotInput,
    nav: &Waypoints,
    senses: &S,
    destination: Vec3,
) -> Result<Option<Vec3>, KitError> {
    let Some(goal) = nav.nearest_to(destination) else {
        return Ok(None);
    };
    let from = match nearest_visible(input, nav, senses)? {
        Some(id) => id,
        None => match nav.nearest_to(input.centre) {
            Some(id) => id,
            None => return Ok(None),
        },
    };
    if from == goal {
        return Ok(Some(destination));
    }
    let Some(hop) = nav.next_hop(from, goal) else {
        return Ok(None);
    };
    let (Some(hop_point), Some(from_point)) = (nav.point(hop), nav.point(from)) else {
        return Ok(None);
    };
    let knee = nav.rules().knee_height() * Vec3::Y;
    if senses.clear(Some(input.body), input.centre, hop_point + knee)? {
        return Ok(Some(hop_point));
    }
    Ok(Some(from_point))
}

/// The goal for a route to `destination`: the next step from [`route`], stopping at the
/// destination.
///
/// # Errors
///
/// Returns a [`KitError`] when a sight query fails.
fn routed<S: Senses>(
    input: &BotInput,
    nav: &Waypoints,
    senses: &S,
    destination: Vec3,
) -> Result<Option<Goal>, KitError> {
    Ok(route(input, nav, senses, destination)?.map(|step| Goal { step, destination }))
}

/// The waypoint nearest the bot's centre, horizontally, whose sight line from the bot's eye is
/// clear. Ties keep the lower index.
///
/// # Errors
///
/// Returns a [`KitError`] when a sight query fails.
fn nearest_visible<S: Senses>(
    input: &BotInput,
    nav: &Waypoints,
    senses: &S,
) -> Result<Option<WaypointId>, KitError> {
    let knee = nav.rules().knee_height() * Vec3::Y;
    let mut best: Option<(f32, WaypointId)> = None;
    for (index, point) in nav.points().iter().enumerate() {
        let distance = horizontal(*point - input.centre).length_squared();
        if best.is_some_and(|(closest, _)| distance >= closest) {
            continue;
        }
        let Some(id) = nav.id(index) else {
            continue;
        };
        if senses.clear(Some(input.body), input.eye, *point + knee)? {
            best = Some((distance, id));
        }
    }
    Ok(best.map(|(_, id)| id))
}
