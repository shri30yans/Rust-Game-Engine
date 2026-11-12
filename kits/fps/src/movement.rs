//! The mutually exclusive locomotion mode and its per-tick event log.
//!
//! Physics queries provide facts to [`next_state`]; the transition table itself is pure and is
//! shared by players, bots, prediction and tests.

use engine_core::StateHasher;
use engine_world::{NetId, Resource};
use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::MoveCommand;

/// One mutually exclusive locomotion mode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum MoveState {
    /// Ordinary ground movement.
    #[default]
    Walk,
    /// Faster forward ground movement.
    Sprint,
    /// Reduced-height ground movement.
    Crouch,
    /// A timed low-friction burst from a sprint.
    Slide {
        /// Remaining slide ticks, including this tick.
        ticks_left: u16,
    },
    /// Jumping or falling.
    Air,
    /// A short scripted traversal from `from` to `to`.
    Mantle {
        /// Starting capsule centre.
        from: Vec3,
        /// Standing capsule centre on the ledge.
        to: Vec3,
        /// Elapsed mantle ticks.
        tick: u16,
        /// Total mantle duration in ticks.
        ticks: u16,
    },
}

/// One mode change during a simulation tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementEvent {
    /// Actor whose mode changed.
    pub actor: NetId,
    /// Previous mode.
    pub from: MoveState,
    /// New mode.
    pub to: MoveState,
}

/// Per-tick movement mode changes. Its actor-sized buffer is allocated during setup.
pub struct MovementLog {
    events: Vec<MovementEvent>,
}

impl MovementLog {
    /// Creates a log with capacity for the match's actors.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: Vec::with_capacity(capacity),
        }
    }

    /// Mode changes recorded during this tick.
    #[must_use]
    pub fn events(&self) -> &[MovementEvent] {
        &self.events
    }

    /// Clears the previous tick without releasing its allocation.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Records a mode change.
    pub fn push(&mut self, event: MovementEvent) {
        self.events.push(event);
    }
}

impl Resource for MovementLog {
    fn hash_into(&self, hasher: &mut StateHasher) {
        for event in &self.events {
            hasher.write_u32(event.actor.index());
            hasher.write_u32(event.actor.generation());
            hash_state(hasher, event.from);
            hash_state(hasher, event.to);
        }
    }
}

/// Physics and tuning facts needed by the pure movement transition table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionFacts {
    /// Whether the character has walkable support.
    pub grounded: bool,
    /// Current horizontal speed in metres per second.
    pub horizontal_speed: f32,
    /// Whether the standing capsule fits at the current feet position.
    pub can_stand: bool,
    /// Validated mantle destination, when a ledge was detected.
    pub mantle_target: Option<Vec3>,
    /// Starting capsule centre for a newly accepted mantle.
    pub mantle_from: Vec3,
    /// Slide duration in ticks.
    pub slide_ticks: u16,
    /// Minimum speed for starting a slide.
    pub slide_min_speed: f32,
    /// Mantle duration in ticks.
    pub mantle_ticks: u16,
}

/// Chooses the next locomotion mode from the current mode, command, and physics facts.
#[must_use]
pub fn next_state(state: MoveState, command: MoveCommand, facts: TransitionFacts) -> MoveState {
    if let MoveState::Mantle {
        from,
        to,
        tick,
        ticks,
    } = state
    {
        return if tick.saturating_add(1) >= ticks {
            MoveState::Walk
        } else {
            MoveState::Mantle {
                from,
                to,
                tick: tick.saturating_add(1),
                ticks,
            }
        };
    }
    let is_crouched = matches!(state, MoveState::Crouch | MoveState::Slide { .. });
    if command.jump_pressed() {
        if let Some(to) = facts.mantle_target {
            return MoveState::Mantle {
                from: facts.mantle_from,
                to,
                tick: 0,
                ticks: facts.mantle_ticks.max(1),
            };
        }
        if is_crouched && !facts.can_stand {
            return state;
        }
    }
    if command.jump_pressed() || (!facts.grounded && (!is_crouched || facts.can_stand)) {
        return MoveState::Air;
    }
    match state {
        MoveState::Walk => {
            if command.crouch() {
                MoveState::Crouch
            } else if command.sprint()
                && command.move_axis().y > 0.5
                && !command.fire()
                && !command.reload()
            {
                MoveState::Sprint
            } else {
                MoveState::Walk
            }
        }
        MoveState::Sprint => {
            if command.crouch() {
                if facts.horizontal_speed >= facts.slide_min_speed {
                    MoveState::Slide {
                        ticks_left: facts.slide_ticks.max(1),
                    }
                } else {
                    MoveState::Crouch
                }
            } else if !command.sprint()
                || command.move_axis().y <= 0.5
                || command.fire()
                || command.reload()
            {
                MoveState::Walk
            } else {
                MoveState::Sprint
            }
        }
        MoveState::Crouch => {
            if !command.crouch() && facts.can_stand {
                MoveState::Walk
            } else {
                MoveState::Crouch
            }
        }
        MoveState::Slide { ticks_left } => {
            if ticks_left <= 1 {
                if command.crouch() || !facts.can_stand {
                    MoveState::Crouch
                } else {
                    MoveState::Walk
                }
            } else {
                MoveState::Slide {
                    ticks_left: ticks_left - 1,
                }
            }
        }
        MoveState::Air => MoveState::Walk,
        MoveState::Mantle { .. } => MoveState::Walk,
    }
}

fn hash_state(hasher: &mut StateHasher, state: MoveState) {
    match state {
        MoveState::Walk => hasher.write_u8(0),
        MoveState::Sprint => hasher.write_u8(1),
        MoveState::Crouch => hasher.write_u8(2),
        MoveState::Slide { ticks_left } => {
            hasher.write_u8(3);
            hasher.write_u32(u32::from(ticks_left));
        }
        MoveState::Air => hasher.write_u8(4),
        MoveState::Mantle {
            from,
            to,
            tick,
            ticks,
        } => {
            hasher.write_u8(5);
            for value in [from.x, from.y, from.z, to.x, to.y, to.z] {
                hasher.write_f32(value);
            }
            hasher.write_u32(u32::from(tick));
            hasher.write_u32(u32::from(ticks));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use app::ViewAngles;
    use glam::Vec2;

    use super::*;
    use crate::Trigger;

    /// Grounded, room to stand, no ledge, and typical tuning numbers.
    fn ground_facts() -> TransitionFacts {
        TransitionFacts {
            grounded: true,
            horizontal_speed: 0.0,
            can_stand: true,
            mantle_target: None,
            mantle_from: Vec3::ZERO,
            slide_ticks: 20,
            slide_min_speed: 5.0,
            mantle_ticks: 20,
        }
    }

    fn command(
        axis: Vec2,
        sprint: bool,
        crouch: bool,
        jump: bool,
        fire: bool,
        reload: bool,
    ) -> MoveCommand {
        MoveCommand::new(
            axis,
            ViewAngles::ZERO,
            jump,
            jump,
            if fire {
                Trigger::Held
            } else {
                Trigger::Released
            },
            reload,
        )
        .unwrap()
        .with_movement(sprint, crouch)
    }

    #[test]
    fn transition_table_covers_ground_modes_refusals_and_air() {
        let rows = [
            (
                MoveState::Walk,
                command(Vec2::Y, true, false, false, false, false),
                ground_facts(),
                MoveState::Sprint,
            ),
            (
                MoveState::Sprint,
                command(Vec2::Y, true, false, false, false, false),
                TransitionFacts {
                    horizontal_speed: 8.0,
                    ..ground_facts()
                },
                MoveState::Sprint,
            ),
            (
                MoveState::Sprint,
                command(Vec2::Y, false, false, false, false, false),
                ground_facts(),
                MoveState::Walk,
            ),
            (
                MoveState::Sprint,
                command(Vec2::Y, true, false, false, true, false),
                ground_facts(),
                MoveState::Walk,
            ),
            (
                MoveState::Sprint,
                command(Vec2::X, true, false, false, false, false),
                ground_facts(),
                MoveState::Walk,
            ),
            (
                MoveState::Walk,
                command(Vec2::ZERO, false, true, false, false, false),
                ground_facts(),
                MoveState::Crouch,
            ),
            (
                MoveState::Crouch,
                command(Vec2::ZERO, false, false, false, false, false),
                TransitionFacts {
                    can_stand: false,
                    ..ground_facts()
                },
                MoveState::Crouch,
            ),
            (
                MoveState::Crouch,
                command(Vec2::ZERO, false, false, false, false, false),
                ground_facts(),
                MoveState::Walk,
            ),
            (
                MoveState::Sprint,
                command(Vec2::Y, true, true, false, false, false),
                TransitionFacts {
                    horizontal_speed: 5.0,
                    ..ground_facts()
                },
                MoveState::Slide { ticks_left: 20 },
            ),
            (
                MoveState::Sprint,
                command(Vec2::Y, true, true, false, false, false),
                TransitionFacts {
                    horizontal_speed: 4.9,
                    ..ground_facts()
                },
                MoveState::Crouch,
            ),
            (
                MoveState::Slide { ticks_left: 2 },
                command(Vec2::Y, false, true, false, false, false),
                ground_facts(),
                MoveState::Slide { ticks_left: 1 },
            ),
            (
                MoveState::Slide { ticks_left: 1 },
                command(Vec2::Y, false, true, false, false, false),
                ground_facts(),
                MoveState::Crouch,
            ),
            (
                MoveState::Slide { ticks_left: 1 },
                command(Vec2::Y, false, false, false, false, false),
                TransitionFacts {
                    can_stand: false,
                    ..ground_facts()
                },
                MoveState::Crouch,
            ),
            (
                MoveState::Slide { ticks_left: 1 },
                command(Vec2::Y, false, false, false, false, false),
                ground_facts(),
                MoveState::Walk,
            ),
            (
                MoveState::Sprint,
                command(Vec2::Y, false, false, true, false, false),
                ground_facts(),
                MoveState::Air,
            ),
            (
                MoveState::Walk,
                command(Vec2::ZERO, false, false, false, false, false),
                TransitionFacts {
                    grounded: false,
                    ..ground_facts()
                },
                MoveState::Air,
            ),
        ];
        for (from, cmd, facts, expected) in rows {
            assert_eq!(next_state(from, cmd, facts), expected);
        }
    }

    #[test]
    fn mantle_only_starts_from_a_validated_target_and_completes_on_its_duration() {
        let cmd = command(Vec2::Y, false, false, true, false, false);
        let from = Vec3::new(0.0, 0.9, 0.0);
        let to = Vec3::new(0.0, 2.1, -1.0);
        let facts = TransitionFacts {
            mantle_target: Some(to),
            mantle_from: from,
            mantle_ticks: 3,
            ..ground_facts()
        };
        let first = next_state(MoveState::Air, cmd, facts);
        assert_eq!(
            first,
            MoveState::Mantle {
                from,
                to,
                tick: 0,
                ticks: 3
            }
        );
        let second = next_state(first, MoveCommand::IDLE, facts);
        assert_eq!(
            second,
            MoveState::Mantle {
                from,
                to,
                tick: 1,
                ticks: 3
            }
        );
        let third = next_state(
            MoveState::Mantle {
                from,
                to,
                tick: 2,
                ticks: 3,
            },
            MoveCommand::IDLE,
            facts,
        );
        assert_eq!(third, MoveState::Walk);
        assert_eq!(
            next_state(MoveState::Air, cmd, ground_facts()),
            MoveState::Air
        );
    }
}
