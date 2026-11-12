//! Game-supplied replication policy for one entity in one write: relevance, priority,
//! update rate and dormancy. The writer never decides these itself.

/// Per-client packet writer. Relevance weights are supplied by the game; the
/// accumulator carries unsent entities forward so a bounded packet cannot starve them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityReplication {
    /// Whether the client currently considers this entity relevant.
    pub relevant: bool,
    /// Game-authored scheduling priority; higher values get bandwidth sooner.
    pub priority: f32,
    /// Minimum ticks between updates for this entity.
    pub max_update_interval_ticks: u32,
    /// Dormant entities are omitted until a component value changes.
    pub dormant: bool,
}

impl EntityReplication {
    /// Always-relevant entity with the supplied priority and no extra rate limit.
    #[must_use]
    pub const fn always(priority: f32) -> Self {
        Self {
            relevant: true,
            priority,
            max_update_interval_ticks: 1,
            dormant: false,
        }
    }
}
