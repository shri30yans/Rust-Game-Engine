//! Spatial focus navigation for keyboard, gamepad and assistive tech.

use glam::Vec2;

/// Half-angle of the directional search cone (90° total).
const CONE_COS: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Row grouping tolerance for reading-order `Next` / `Prev` (logical px).
const ROW_TOLERANCE_PX: f32 = 4.0;

/// Focusable widget handle supplied by the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(pub u32);

/// Zero-based row of a repeated widget instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowIndex(pub u16);

/// Axis-aligned focus target in logical space (origin top-left, y grows downward).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusRect {
    /// Widget this rectangle belongs to.
    pub id: WidgetId,
    /// Minimum corner (top-left).
    pub min: Vec2,
    /// Maximum corner (bottom-right).
    pub max: Vec2,
}

impl FocusRect {
    fn centre(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
}

/// Navigation input independent of platform bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavInput {
    /// Move focus upward.
    Up,
    /// Move focus downward.
    Down,
    /// Move focus left.
    Left,
    /// Move focus right.
    Right,
    /// Next in reading order.
    Next,
    /// Previous in reading order.
    Prev,
}

/// Explicit focus edge from a screen RON file (MENUS_SPEC rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusOverride {
    /// Source widget.
    pub from: WidgetId,
    /// Input that triggers the override.
    pub input: NavInput,
    /// Destination widget.
    pub to: WidgetId,
}

/// Focus navigator with pre-sized scratch buffers for hot-path reuse.
#[derive(Debug, Default)]
pub struct FocusNav {
    scratch_candidates: Vec<WidgetId>,
    scratch_order: Vec<WidgetId>,
    scratch_row: Vec<WidgetId>,
    scratch_sort: Vec<(WidgetId, Vec2)>,
}

impl FocusNav {
    /// Creates a navigator with scratch space for the given widget count.
    #[must_use]
    pub fn with_capacity(rect_count: usize) -> Self {
        Self {
            scratch_candidates: Vec::with_capacity(rect_count),
            scratch_order: Vec::with_capacity(rect_count),
            scratch_row: Vec::with_capacity(rect_count),
            scratch_sort: Vec::with_capacity(rect_count),
        }
    }

    /// Chooses the next focus target; returns `None` when there is no move or no candidate.
    pub fn navigate(
        &mut self,
        current: WidgetId,
        input: NavInput,
        rects: &[FocusRect],
        overrides: &[FocusOverride],
    ) -> Option<WidgetId> {
        for ov in overrides {
            if ov.from == current && ov.input == input {
                if ov.to == current {
                    return None;
                }
                return Some(ov.to);
            }
        }

        let current_rect = rects.iter().find(|r| r.id == current)?;
        let current_centre = current_rect.centre();

        match input {
            NavInput::Next => self.navigate_reading(current, rects, true),
            NavInput::Prev => self.navigate_reading(current, rects, false),
            NavInput::Up | NavInput::Down | NavInput::Left | NavInput::Right => {
                let direction = direction_vec(input);
                self.scratch_candidates.clear();
                for rect in rects {
                    if rect.id == current {
                        continue;
                    }
                    let delta = rect.centre() - current_centre;
                    if !in_direction_cone(delta, direction) {
                        continue;
                    }
                    self.scratch_candidates.push(rect.id);
                }
                pick_best_spatial(current_centre, direction, rects, &self.scratch_candidates)
            }
        }
    }

    fn navigate_reading(
        &mut self,
        current: WidgetId,
        rects: &[FocusRect],
        forward: bool,
    ) -> Option<WidgetId> {
        self.reading_order(rects);
        let pos = self.scratch_order.iter().position(|&id| id == current)?;
        if forward {
            self.scratch_order.get(pos + 1).copied()
        } else {
            pos.checked_sub(1)
                .and_then(|i| self.scratch_order.get(i).copied())
        }
    }

    fn reading_order(&mut self, rects: &[FocusRect]) {
        self.scratch_order.clear();
        self.scratch_sort.clear();
        for rect in rects {
            self.scratch_sort.push((rect.id, rect.centre()));
        }
        self.scratch_sort.sort_unstable_by(|a, b| {
            a.1.y
                .total_cmp(&b.1.y)
                .then(a.1.x.total_cmp(&b.1.x))
                .then(a.0.cmp(&b.0))
        });

        if self.scratch_sort.is_empty() {
            return;
        }

        let mut row_start_y = self.scratch_sort.first().map_or(0.0, |entry| entry.1.y);
        self.scratch_row.clear();
        for (id, centre) in &self.scratch_sort {
            if (centre.y - row_start_y).abs() > ROW_TOLERANCE_PX && !self.scratch_row.is_empty() {
                sort_row_by_x(&mut self.scratch_row, rects);
                self.scratch_order.extend_from_slice(&self.scratch_row);
                self.scratch_row.clear();
                row_start_y = centre.y;
            }
            self.scratch_row.push(*id);
        }
        if !self.scratch_row.is_empty() {
            sort_row_by_x(&mut self.scratch_row, rects);
            self.scratch_order.extend_from_slice(&self.scratch_row);
        }
    }
}

fn sort_row_by_x(row: &mut [WidgetId], rects: &[FocusRect]) {
    row.sort_unstable_by(|a, b| {
        let ax = centre_x(*a, rects);
        let bx = centre_x(*b, rects);
        ax.total_cmp(&bx).then(a.cmp(b))
    });
}

fn centre_x(id: WidgetId, rects: &[FocusRect]) -> f32 {
    rects
        .iter()
        .find(|r| r.id == id)
        .map_or(0.0, |r| r.centre().x)
}

fn pick_best_spatial(
    origin: Vec2,
    direction: Vec2,
    rects: &[FocusRect],
    candidates: &[WidgetId],
) -> Option<WidgetId> {
    let mut best: Option<(WidgetId, f32)> = None;
    for &id in candidates {
        let Some(rect) = rects.iter().find(|r| r.id == id) else {
            continue;
        };
        let delta = rect.centre() - origin;
        let primary = delta.dot(direction);
        let orth = delta.perp_dot(direction).abs();
        let score = primary + 2.0 * orth;
        let replace = match best {
            None => true,
            Some((best_id, best_score)) => {
                score < best_score - f32::EPSILON
                    || ((score - best_score).abs() <= f32::EPSILON && id < best_id)
            }
        };
        if replace {
            best = Some((id, score));
        }
    }
    best.map(|(id, _)| id)
}

fn direction_vec(input: NavInput) -> Vec2 {
    match input {
        NavInput::Up => Vec2::new(0.0, -1.0),
        NavInput::Down => Vec2::new(0.0, 1.0),
        NavInput::Left => Vec2::new(-1.0, 0.0),
        NavInput::Right => Vec2::new(1.0, 0.0),
        NavInput::Next | NavInput::Prev => Vec2::ZERO,
    }
}

fn in_direction_cone(delta: Vec2, direction: Vec2) -> bool {
    let len_sq = delta.length_squared();
    if len_sq <= f32::EPSILON {
        return false;
    }
    let along = delta.dot(direction);
    if along <= 0.0 {
        return false;
    }
    let cos = along / len_sq.sqrt();
    cos >= CONE_COS
}
