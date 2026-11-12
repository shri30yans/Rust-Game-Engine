//! Device-independent input state. Keyboard and touch both feed `move_axis`/`fire`,
//! so game code never knows which device it is running on.
use engine_core::{TouchRegion, TouchRegionId};
use glam::Vec2;

/// Game actions. Keyboard keys and touch controls both map onto these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// Turn or strafe left.
    Left,
    /// Turn or strafe right.
    Right,
    /// Pitch up or move forward.
    Up,
    /// Pitch down or move back.
    Down,
    /// Primary fire.
    Fire,
    /// Orbit camera toggle.
    Orbit,
    /// Stats overlay toggle.
    Stats,
    /// Quit the application.
    Quit,
    /// Throttle boost.
    Boost,
    /// Throttle brake.
    Brake,
    /// Roll left.
    RollLeft,
    /// Roll right.
    RollRight,
    /// Missile launch.
    Missile,
}
const KEY_COUNT: usize = 13;
const TOUCH_POINT_CAPACITY: usize = 16;

/// Raw keys for games that want their own bindings (FPS, menus, editor).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Code {
    /// Letter A.
    A,
    /// Letter B.
    B,
    /// Letter C.
    C,
    /// Letter D.
    D,
    /// Letter E.
    E,
    /// Letter F.
    F,
    /// Letter G.
    G,
    /// Letter H.
    H,
    /// Letter I.
    I,
    /// Letter J.
    J,
    /// Letter K.
    K,
    /// Letter L.
    L,
    /// Letter M.
    M,
    /// Letter N.
    N,
    /// Letter O.
    O,
    /// Letter P.
    P,
    /// Letter Q.
    Q,
    /// Letter R.
    R,
    /// Letter S.
    S,
    /// Letter T.
    T,
    /// Letter U.
    U,
    /// Letter V.
    V,
    /// Letter W.
    W,
    /// Letter X.
    X,
    /// Letter Y.
    Y,
    /// Letter Z.
    Z,
    /// Digit 0.
    D0,
    /// Digit 1.
    D1,
    /// Digit 2.
    D2,
    /// Digit 3.
    D3,
    /// Digit 4.
    D4,
    /// Digit 5.
    D5,
    /// Digit 6.
    D6,
    /// Digit 7.
    D7,
    /// Digit 8.
    D8,
    /// Digit 9.
    D9,
    /// Space bar.
    Space,
    /// Either Shift key.
    Shift,
    /// Either Control key.
    Ctrl,
    /// Either Alt key.
    Alt,
    /// Tab.
    Tab,
    /// Escape.
    Esc,
    /// Enter, including the keypad Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Function key F1.
    F1,
    /// Function key F2.
    F2,
    /// Function key F3.
    F3,
    /// Function key F4.
    F4,
    /// Function key F5.
    F5,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
}
/// Number of `Code` variants; sizes the per-code tables.
pub const CODE_COUNT: usize = Code::PageDown as usize + 1;

/// Mouse buttons the game can read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mouse {
    /// Primary button.
    Left,
    /// Secondary button.
    Right,
    /// Wheel button.
    Middle,
}

/// Maximum number of clipped touch regions retained from a rendered frame.
pub const MAX_TOUCH_REGIONS: usize = 8;

/// A raw touch change recorded during the current frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TouchPoint {
    /// Platform touch identifier.
    pub id: u64,
    /// Position in physical pixels.
    pub pos: Vec2,
    /// Change recorded this frame.
    pub phase: TouchPhase,
}

/// The kind of raw touch change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchPhase {
    /// A finger started touching the surface.
    Began,
    /// A finger moved on the surface.
    Moved,
    /// A finger stopped touching the surface.
    Ended,
}

/// The per-frame state of one drawn touch region, kept beside its layout entry.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct RegionState {
    /// The touch that holds the button down, if any.
    finger: Option<u64>,
    /// Set when the button went down this frame; cleared by `end_frame`.
    pressed: bool,
    /// Where the finger was last seen, for drag-look buttons.
    last: Vec2,
}

/// Per-frame input snapshot. The window runner writes it; the game reads it once per frame.
pub struct Input {
    held: [bool; KEY_COUNT],
    pressed: [bool; KEY_COUNT],
    stick: Option<(u64, Vec2, Vec2)>, // touch id, origin, current
    look_touch: Option<(u64, Vec2)>,  // touch id, last position
    /// Regions emitted by the last successful UI draw. Capacity is fixed at `MAX_TOUCH_REGIONS`.
    touch_regions: Vec<TouchRegion>,
    /// One entry per `touch_regions` entry, in the same order.
    region_state: Vec<RegionState>,
    touches: Vec<TouchPoint>,
    dropped_touches: usize,
    /// Pixels the right-half look touch moved this frame (device pixels). Zeroed by `end_frame`.
    pub touch_look: Vec2,
    /// Surface size in physical pixels, used to split the touch screen into stick and look halves.
    pub screen: Vec2,
    codes: [bool; CODE_COUNT],
    codes_pressed: [bool; CODE_COUNT],
    buttons: [bool; 3],
    buttons_pressed: [bool; 3],
    /// Raw mouse motion this frame (device pixels), independent of cursor position.
    pub mouse_delta: Vec2,
    /// Cursor position in window pixels.
    pub mouse_pos: Vec2,
    /// Wheel movement this frame, in lines.
    pub wheel: f32,
    /// Set by the game: lock and hide the cursor (FPS look). The window runner applies it.
    pub want_grab: bool,
    /// Whether the window currently holds the grab (written by the runner).
    pub grabbed: bool,
    /// Set by the game to close the app.
    pub quit: bool,
    /// When false, Escape is delivered to the game instead of quitting.
    pub esc_quits: bool,
}

/// Reads slot `i` of a per-key table. The tables are sized from the enum variant counts, so
/// a miss means they have drifted apart; it reads as "not down" rather than panicking.
fn slot(table: &[bool], i: usize) -> bool {
    table.get(i).copied().unwrap_or(false)
}

/// Writes slot `i` of a per-key table; an out-of-range index is ignored (see `slot`).
fn set_slot(table: &mut [bool], i: usize, v: bool) {
    if let Some(s) = table.get_mut(i) {
        *s = v;
    }
}

impl Input {
    /// Touch changes recorded since the previous [`Input::end_frame`].
    #[must_use]
    pub fn touches(&self) -> &[TouchPoint] {
        &self.touches
    }

    /// Number of touch changes dropped after the fixed per-frame buffer filled.
    #[must_use]
    pub const fn dropped_touches(&self) -> usize {
        self.dropped_touches
    }

    fn record_touch(&mut self, point: TouchPoint) {
        if self.touches.len() < TOUCH_POINT_CAPACITY {
            self.touches.push(point);
        } else {
            self.dropped_touches = self.dropped_touches.saturating_add(1);
        }
    }
    /// Records a key transition. Sets the once-per-press flag on a rising edge.
    pub fn set_key(&mut self, k: Key, down: bool) {
        let i = k as usize;
        if down && !slot(&self.held, i) {
            set_slot(&mut self.pressed, i, true);
        }
        set_slot(&mut self.held, i, down);
    }
    /// True while the action is held.
    pub fn held(&self, k: Key) -> bool {
        slot(&self.held, k as usize)
    }
    /// True once per press; cleared by `end_frame`.
    pub fn pressed(&self, k: Key) -> bool {
        slot(&self.pressed, k as usize)
    }
    /// Clears the per-frame edges and deltas. Called by the runner after each frame.
    pub fn end_frame(&mut self) {
        self.pressed = [false; KEY_COUNT];
        self.codes_pressed = [false; CODE_COUNT];
        self.buttons_pressed = [false; 3];
        self.mouse_delta = Vec2::ZERO;
        self.touch_look = Vec2::ZERO;
        self.wheel = 0.0;
        self.touches.clear();
        self.dropped_touches = 0;
        for state in &mut self.region_state {
            state.pressed = false;
        }
    }

    /// Installs regions from a successful UI draw. The previous regions remain intact if the
    /// incoming list is over capacity or has duplicate identifiers. A held identifier carries its
    /// finger across a moved layout, while the pressed edge remains frame-local.
    pub fn set_touch_regions(&mut self, regions: &[TouchRegion]) -> bool {
        if regions.len() > MAX_TOUCH_REGIONS
            || regions.iter().enumerate().any(|(index, region)| {
                regions
                    .iter()
                    .take(index)
                    .any(|other| other.id == region.id)
            })
        {
            return false;
        }
        let mut carried = [RegionState::default(); MAX_TOUCH_REGIONS];
        for (slot, region) in carried.iter_mut().zip(regions) {
            if let Some(old) = self
                .touch_regions
                .iter()
                .position(|old| old.id == region.id)
                && let Some(state) = self.region_state.get(old)
            {
                *slot = RegionState {
                    pressed: false,
                    ..*state
                };
            }
        }
        self.touch_regions.clear();
        self.touch_regions.extend_from_slice(regions);
        self.region_state.clear();
        self.region_state.extend(carried.iter().take(regions.len()));
        true
    }

    /// Regions from the most recent successful UI draw, in painter's order.
    #[must_use]
    pub fn touch_regions(&self) -> &[TouchRegion] {
        &self.touch_regions
    }

    /// True while a finger is down on region `id`.
    #[must_use]
    pub fn touch_region_held(&self, id: TouchRegionId) -> bool {
        self.region_state_of(id)
            .is_some_and(|state| state.finger.is_some())
    }

    /// True once per touch: region `id` went down this frame. Cleared by `end_frame`.
    #[must_use]
    pub fn touch_region_pressed(&self, id: TouchRegionId) -> bool {
        self.region_state_of(id).is_some_and(|state| state.pressed)
    }

    /// The state of region `id`, or `None` when it is absent.
    fn region_state_of(&self, id: TouchRegionId) -> Option<&RegionState> {
        let index = self
            .touch_regions
            .iter()
            .position(|region| region.id == id)?;
        self.region_state.get(index)
    }

    /// The topmost drawn region that contains pixel `p`.
    fn region_at(&self, p: Vec2) -> Option<usize> {
        self.touch_regions
            .iter()
            .rposition(|region| region.contains(p.x, p.y))
    }

    /// Records a raw key transition. Sets the once-per-press flag on a rising edge.
    pub fn set_code(&mut self, c: Code, down: bool) {
        let i = c as usize;
        if down && !slot(&self.codes, i) {
            set_slot(&mut self.codes_pressed, i, true);
        }
        set_slot(&mut self.codes, i, down);
    }
    /// True while the raw key is held.
    pub fn down(&self, c: Code) -> bool {
        slot(&self.codes, c as usize)
    }
    /// True once per press of the raw key; cleared by `end_frame`.
    pub fn hit(&self, c: Code) -> bool {
        slot(&self.codes_pressed, c as usize)
    }
    /// Records a mouse button transition. Sets the once-per-click flag on a rising edge.
    pub fn set_button(&mut self, b: Mouse, down: bool) {
        let i = b as usize;
        if down && !slot(&self.buttons, i) {
            set_slot(&mut self.buttons_pressed, i, true);
        }
        set_slot(&mut self.buttons, i, down);
    }
    /// True while the mouse button is held.
    pub fn button(&self, b: Mouse) -> bool {
        slot(&self.buttons, b as usize)
    }
    /// True once per click of the mouse button; cleared by `end_frame`.
    pub fn clicked(&self, b: Mouse) -> bool {
        slot(&self.buttons_pressed, b as usize)
    }
    /// Release every held key and button (focus lost, menu opened).
    pub fn release_all(&mut self) {
        self.held = [false; KEY_COUNT];
        self.codes = [false; CODE_COUNT];
        self.buttons = [false; 3];
    }

    /// x: turn right(+)/left(-), y: pitch up(+)/down(-), each in [-1, 1].
    pub fn move_axis(&self) -> Vec2 {
        let mut v = Vec2::ZERO;
        if self.held(Key::Right) {
            v.x += 1.0;
        }
        if self.held(Key::Left) {
            v.x -= 1.0;
        }
        if self.held(Key::Up) {
            v.y += 1.0;
        }
        if self.held(Key::Down) {
            v.y -= 1.0;
        }
        v += self.stick();
        v.clamp_length_max(1.0)
    }

    /// Deflection of the touch stick: x right(+)/left(-), y forward(+)/back(-), clamped to unit
    /// length. 80 px of drag is full deflection; zero when no stick touch is down.
    #[must_use]
    pub fn stick(&self) -> Vec2 {
        match self.stick {
            // Screen y grows downward, so the forward component is negated.
            Some((_, origin, current)) => {
                let d = (current - origin) / 80.0;
                Vec2::new(d.x, -d.y).clamp_length_max(1.0)
            }
            None => Vec2::ZERO,
        }
    }

    /// True while the keyboard fire key is held. Games read touch fire through their
    /// [`TouchRegionId`] with [`Input::touch_region_held`].
    pub fn fire(&self) -> bool {
        self.held(Key::Fire)
    }

    /// Throttle multiplier in [0.65, 1.25]: Shift boost, Ctrl brake.
    pub fn throttle(&self) -> f32 {
        let mut t = 1.0f32;
        if self.held(Key::Boost) {
            t += 0.25;
        }
        if self.held(Key::Brake) {
            t -= 0.35;
        }
        t.clamp(0.65, 1.25)
    }

    /// Manual roll in [-1, 1] from Q/E.
    pub fn roll_axis(&self) -> f32 {
        let mut r = 0.0f32;
        if self.held(Key::RollRight) {
            r += 1.0;
        }
        if self.held(Key::RollLeft) {
            r -= 1.0;
        }
        r.clamp(-1.0, 1.0)
    }

    /// True while the missile key is held.
    pub fn missile(&self) -> bool {
        self.held(Key::Missile)
    }

    /// Routes a new touch. A touch inside a drawn region holds that region and never
    /// becomes the stick or the look; a touch on a button that is already held is ignored. Any
    /// other touch on the left half is the floating stick (`move_axis`), and on the right half it
    /// is the look touch (`touch_look`) if no look touch is down.
    pub fn touch_start(&mut self, id: u64, p: Vec2) {
        self.record_touch(TouchPoint {
            id,
            pos: p,
            phase: TouchPhase::Began,
        });
        if let Some(index) = self.region_at(p) {
            if let Some(state) = self.region_state.get_mut(index)
                && state.finger.is_none()
            {
                state.finger = Some(id);
                state.pressed = true;
                state.last = p;
            }
            return;
        }
        if p.x < self.screen.x * 0.5 {
            if self.stick.is_none() {
                self.stick = Some((id, p, p));
            }
        } else if self.look_touch.is_none() {
            self.look_touch = Some((id, p));
        }
    }
    /// Moves the touch `id`: the stick follows it, and a look touch adds its motion to `touch_look`.
    /// A button stays held when its finger slides off it; only lifting releases it. A drag-look button also adds its finger's motion to `touch_look`.
    pub fn touch_move(&mut self, id: u64, p: Vec2) {
        self.record_touch(TouchPoint {
            id,
            pos: p,
            phase: TouchPhase::Moved,
        });
        if let Some((sid, _, c)) = self.stick.as_mut()
            && *sid == id
        {
            *c = p;
        }
        if let Some((lid, last)) = self.look_touch.as_mut()
            && *lid == id
        {
            self.touch_look += p - *last;
            *last = p;
        }
        for (region, state) in self.touch_regions.iter().zip(self.region_state.iter_mut()) {
            if region.drag_look && state.finger == Some(id) {
                self.touch_look += p - state.last;
                state.last = p;
            }
        }
    }
    /// Lifts the touch `id` from whichever control it drives.
    pub fn touch_end(&mut self, id: u64) {
        let pos = self
            .stick
            .filter(|(touch_id, _, _)| *touch_id == id)
            .map(|(_, _, current)| current)
            .or_else(|| {
                self.look_touch
                    .filter(|(touch_id, _)| *touch_id == id)
                    .map(|(_, last)| last)
            })
            .or_else(|| {
                self.touch_regions
                    .iter()
                    .zip(&self.region_state)
                    .find(|(_, state)| state.finger == Some(id))
                    .map(|(_, state)| state.last)
            })
            .unwrap_or(Vec2::ZERO);
        self.record_touch(TouchPoint {
            id,
            pos,
            phase: TouchPhase::Ended,
        });
        if matches!(self.stick, Some((sid, _, _)) if sid == id) {
            self.stick = None;
        }
        if matches!(self.look_touch, Some((lid, _)) if lid == id) {
            self.look_touch = None;
        }
        for state in &mut self.region_state {
            if state.finger == Some(id) {
                state.finger = None;
            }
        }
    }
}

impl Default for Input {
    fn default() -> Self {
        Input {
            held: [false; KEY_COUNT],
            pressed: [false; KEY_COUNT],
            stick: None,
            look_touch: None,
            touch_regions: Vec::with_capacity(MAX_TOUCH_REGIONS),
            region_state: Vec::with_capacity(MAX_TOUCH_REGIONS),
            touches: Vec::with_capacity(TOUCH_POINT_CAPACITY),
            dropped_touches: 0,
            touch_look: Vec2::ZERO,
            screen: Vec2::ZERO,
            codes: [false; CODE_COUNT],
            codes_pressed: [false; CODE_COUNT],
            buttons: [false; 3],
            buttons_pressed: [false; 3],
            mouse_delta: Vec2::ZERO,
            mouse_pos: Vec2::ZERO,
            wheel: 0.0,
            want_grab: false,
            grabbed: false,
            quit: false,
            esc_quits: true,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod region_tests {
    use super::*;
    use engine_core::{Rect, TouchRegion, TouchRegionId, TouchShape, Viewport};

    fn region(id: u8, rect: Rect) -> TouchRegion {
        TouchRegion {
            id: TouchRegionId(id),
            rect: Viewport::new(1000.0, 1000.0).unwrap().clip(rect).unwrap(),
            shape: TouchShape::Rect,
            drag_look: false,
        }
    }

    fn input() -> Input {
        Input {
            screen: Vec2::new(1000.0, 1000.0),
            ..Input::default()
        }
    }

    #[test]
    fn topmost_region_wins() {
        let mut input = input();
        assert!(input.set_touch_regions(&[
            region(
                1,
                Rect {
                    x: 400.0,
                    y: 400.0,
                    w: 200.0,
                    h: 200.0
                }
            ),
            region(
                2,
                Rect {
                    x: 450.0,
                    y: 450.0,
                    w: 200.0,
                    h: 200.0
                }
            ),
        ]));
        input.touch_start(1, Vec2::new(500.0, 500.0));
        assert!(!input.touch_region_held(TouchRegionId(1)));
        assert!(input.touch_region_held(TouchRegionId(2)));
    }

    #[test]
    fn held_state_survives_a_layout_move() {
        let mut input = input();
        assert!(input.set_touch_regions(&[region(
            1,
            Rect {
                x: 400.0,
                y: 400.0,
                w: 100.0,
                h: 100.0
            }
        )]));
        input.touch_start(7, Vec2::new(450.0, 450.0));
        assert!(input.set_touch_regions(&[region(
            1,
            Rect {
                x: 600.0,
                y: 600.0,
                w: 100.0,
                h: 100.0
            }
        )]));
        assert!(input.touch_region_held(TouchRegionId(1)));
        input.touch_end(7);
        assert!(!input.touch_region_held(TouchRegionId(1)));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod routing_tests {
    use super::*;
    use engine_core::{Rect, TouchShape, Viewport};

    fn screen() -> Input {
        Input {
            screen: Vec2::new(1000.0, 1000.0),
            ..Input::default()
        }
    }

    fn region(id: u8, rect: Rect, shape: TouchShape, drag_look: bool) -> TouchRegion {
        TouchRegion {
            id: TouchRegionId(id),
            rect: Viewport::new(1000.0, 1000.0)
                .and_then(|viewport| viewport.clip(rect))
                .expect("test rectangle is visible"),
            shape,
            drag_look,
        }
    }

    #[test]
    fn right_half_touch_looks_and_left_half_touch_moves_the_stick() {
        let mut input = screen();
        input.touch_start(1, Vec2::new(800.0, 500.0));
        input.touch_move(1, Vec2::new(810.0, 520.0));
        assert_eq!(input.touch_look, Vec2::new(10.0, 20.0));

        input.touch_start(2, Vec2::new(100.0, 500.0));
        input.touch_move(2, Vec2::new(140.0, 460.0));
        assert_eq!(input.stick(), Vec2::new(0.5, 0.5));
        assert_eq!(input.move_axis(), Vec2::new(0.5, 0.5));
    }

    #[test]
    fn regions_take_priority_over_stick_and_look_and_drag_regions_turn_the_view() {
        let mut input = screen();
        let fire = region(
            1,
            Rect {
                x: 400.0,
                y: 400.0,
                w: 200.0,
                h: 200.0,
            },
            TouchShape::Circle,
            true,
        );
        assert!(input.set_touch_regions(&[fire]));
        input.touch_start(7, Vec2::new(500.0, 500.0));
        input.touch_move(7, Vec2::new(530.0, 480.0));
        assert!(input.touch_region_held(TouchRegionId(1)));
        assert!(input.touch_region_pressed(TouchRegionId(1)));
        assert_eq!(input.stick(), Vec2::ZERO);
        assert_eq!(input.touch_look, Vec2::new(30.0, -20.0));
        input.touch_end(7);
        assert!(!input.touch_region_held(TouchRegionId(1)));
    }

    #[test]
    fn invalid_replacement_keeps_the_last_drawn_regions() {
        let mut input = screen();
        let old = region(
            1,
            Rect {
                x: 100.0,
                y: 100.0,
                w: 100.0,
                h: 100.0,
            },
            TouchShape::Rect,
            false,
        );
        assert!(input.set_touch_regions(&[old]));
        assert!(!input.set_touch_regions(&[old, old]));
        assert_eq!(input.touch_regions(), &[old]);

        let too_many = (0..=MAX_TOUCH_REGIONS)
            .map(|id| {
                region(
                    u8::try_from(id).expect("test id fits"),
                    Rect {
                        x: 100.0,
                        y: 100.0,
                        w: 100.0,
                        h: 100.0,
                    },
                    TouchShape::Rect,
                    false,
                )
            })
            .collect::<Vec<_>>();
        assert!(!input.set_touch_regions(&too_many));
        assert_eq!(input.touch_regions(), &[old]);
    }

    #[test]
    fn raw_touch_events_are_bounded_and_cleared_each_frame() {
        let mut input = screen();
        for id in 0..20 {
            input.touch_start(id, Vec2::new(600.0, 200.0));
        }
        assert_eq!(input.touches().len(), TOUCH_POINT_CAPACITY);
        assert_eq!(input.dropped_touches(), 4);
        input.end_frame();
        assert!(input.touches().is_empty());
        assert_eq!(input.dropped_touches(), 0);
    }
}
