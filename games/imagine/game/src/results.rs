//! Stable match results copied from the finished world before it is replaced.
//!
//! Invariants: actor slot order is preserved; team score sums credited kills; tied top scores have
//! no winner.

use std::collections::BTreeMap;

use app::AppError;
use engine_world::World;
use kit_fps::Team;

use crate::rules::{Actors, PLAYER_SLOT, Score};

/// One actor's final score row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorResult {
    /// Stable display name derived from the actor slot.
    pub name: String,
    /// Team number from the finished world.
    pub team: u8,
    /// Kills credited by the rules.
    pub kills: u32,
    /// Deaths recorded by the rules.
    pub deaths: u32,
}

/// Compact result data retained by the presenter after a world travel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchSummary {
    /// Actor rows in world slot order.
    pub actors: Vec<ActorResult>,
    /// Team with the highest kill total, or `None` when the top total is tied.
    pub winner_team: Option<u8>,
}

impl MatchSummary {
    /// Copies the score and teams from a finished IMAGINE world.
    ///
    /// # Errors
    ///
    /// Returns a world error when score, actor, or team data is missing.
    pub fn from_world(world: &World) -> Result<Self, AppError> {
        let actors = world.resource::<Actors>()?;
        let score = world.resource::<Score>()?;
        let mut rows = Vec::with_capacity(actors.slots().len());
        let mut team_scores = BTreeMap::<u8, u32>::new();
        for (slot, actor) in actors.slots().iter().enumerate() {
            let team = world.get::<Team>(actor.id())?.number();
            let tally = score
                .tally(slot)
                .ok_or(engine_world::WorldError::Invariant("score row is missing"))?;
            rows.push(ActorResult {
                name: if slot == PLAYER_SLOT {
                    "PLAYER".to_owned()
                } else {
                    format!("BOT {slot:02}")
                },
                team,
                kills: tally.kills(),
                deaths: tally.deaths(),
            });
            let total = team_scores.entry(team).or_default();
            *total = total.saturating_add(tally.kills());
        }
        Ok(Self {
            actors: rows,
            winner_team: winning_team(&team_scores),
        })
    }

    /// Player's final kills, or zero when no player row exists.
    #[must_use]
    pub fn player_kills(&self) -> u32 {
        self.actors.get(PLAYER_SLOT).map_or(0, |row| row.kills)
    }

    /// Player's final deaths, or zero when no player row exists.
    #[must_use]
    pub fn player_deaths(&self) -> u32 {
        self.actors.get(PLAYER_SLOT).map_or(0, |row| row.deaths)
    }

    /// Team-level score for a team, summed from the rules' credited kills.
    #[must_use]
    pub fn team_score(&self, team: u8) -> u32 {
        self.actors
            .iter()
            .filter(|actor| actor.team == team)
            .fold(0, |total, actor| total.saturating_add(actor.kills))
    }
}

fn winning_team(scores: &BTreeMap<u8, u32>) -> Option<u8> {
    let top = scores.values().copied().max()?;
    let mut winners = scores
        .iter()
        .filter_map(|(&team, &score)| (score == top).then_some(team));
    let first = winners.next();
    if winners.next().is_none() {
        first
    } else {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn winner_comes_from_the_unique_highest_team_score() {
        let scores = BTreeMap::from([(0, 4), (1, 7)]);
        assert_eq!(winning_team(&scores), Some(1));
    }

    #[test]
    fn tied_team_scores_have_no_winner() {
        let scores = BTreeMap::from([(0, 4), (1, 4)]);
        assert_eq!(winning_team(&scores), None);
    }

    #[test]
    fn an_empty_match_has_no_winner() {
        assert_eq!(winning_team(&BTreeMap::new()), None);
    }
}
