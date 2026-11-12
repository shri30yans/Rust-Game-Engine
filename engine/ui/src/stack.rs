//! Screen layer stack: one stack of [`ScreenId`]s per [`Layer`].

use crate::error::UiError;

/// UI layer, low to high (see MENUS_SPEC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// In-world HUD and overlays that share the game view.
    Game,
    /// Pause / scoreboard while the match keeps running.
    GameMenu,
    /// Full-screen menus (home, settings, loadout).
    Menu,
    /// Blocking modal (confirm, matchmaking).
    Modal,
    /// Transient toasts; visible but not focusable.
    Toast,
}

impl Layer {
    /// Every layer in draw order (low to high), matching [`ScreenStack`] storage order.
    pub const ALL: [Self; 5] = [
        Self::Game,
        Self::GameMenu,
        Self::Menu,
        Self::Modal,
        Self::Toast,
    ];

    /// Whether keyboard / gamepad focus may sit on this layer.
    #[must_use]
    pub fn accepts_focus(self) -> bool {
        !matches!(self, Self::Toast)
    }
}

/// Opaque screen identifier supplied by the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenId(pub u32);

/// Stacks of screens per layer; hot paths do not allocate after the vectors are warm.
#[derive(Debug, Clone, Default)]
pub struct ScreenStack {
    stacks: [Vec<ScreenId>; Layer::ALL.len()],
}

impl ScreenStack {
    /// Creates an empty stack for every layer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stacks: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// Pushes a screen onto the given layer.
    pub fn push(&mut self, layer: Layer, screen: ScreenId) {
        for (slot_layer, stack) in Layer::ALL.iter().zip(&mut self.stacks) {
            if *slot_layer == layer {
                stack.push(screen);
                return;
            }
        }
    }

    /// Pops the top screen of the highest non-empty layer (Back).
    pub fn pop_top(&mut self) -> Result<(Layer, ScreenId), UiError> {
        for (layer, stack) in Layer::ALL.iter().rev().zip(self.stacks.iter_mut().rev()) {
            if let Some(screen) = stack.pop() {
                return Ok((*layer, screen));
            }
        }
        Err(UiError::EmptyStack)
    }

    /// Top screen of a layer.
    #[must_use]
    pub fn top(&self, layer: Layer) -> Option<ScreenId> {
        self.stacks
            .iter()
            .zip(Layer::ALL)
            .find(|(_, candidate)| *candidate == layer)
            .and_then(|(stack, _)| stack.last().copied())
    }

    /// Pops the top screen on exactly one layer.
    pub fn pop_layer(&mut self, layer: Layer) -> Result<ScreenId, UiError> {
        let Some((stack, _)) = self
            .stacks
            .iter_mut()
            .zip(Layer::ALL)
            .find(|(_, candidate)| *candidate == layer)
        else {
            return Err(UiError::EmptyStack);
        };
        stack.pop().ok_or(UiError::EmptyStack)
    }

    /// Top screen of the highest non-empty layer that accepts focus (`Toast` excluded).
    #[must_use]
    pub fn focused(&self) -> Option<(Layer, ScreenId)> {
        for (layer, stack) in Layer::ALL.iter().rev().zip(self.stacks.iter().rev()) {
            if !layer.accepts_focus() {
                continue;
            }
            if let Some(&screen) = stack.last() {
                return Some((*layer, screen));
            }
        }
        None
    }

    /// Removes every screen on the given layer.
    pub fn clear(&mut self, layer: Layer) {
        for (slot_layer, stack) in Layer::ALL.iter().zip(&mut self.stacks) {
            if *slot_layer == layer {
                stack.clear();
                return;
            }
        }
    }
}
