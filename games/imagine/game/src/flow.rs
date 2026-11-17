//! The IMAGINE front-end and match state machine.
//!
//! Invariants: this module owns no input, documents or drawing; callers translate screen actions
//! into FlowAction and apply the returned stack operations to the menu runtime.

use engine_ui::Layer;

/// The major game phases, excluding transient menu overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Startup splash.
    Boot,
    /// Main menu.
    Home,
    /// Select a supported mode.
    ModeSelect,
    /// Local or online room.
    Lobby,
    /// Match world travel is in progress.
    Loading,
    /// Active match.
    Match,
    /// End-of-match summary.
    Results,
}

/// A document the menu presenter can activate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
    /// Startup splash.
    Boot,
    /// Main menu.
    Home,
    /// Mode list.
    Modes,
    /// Session lobby.
    Lobby,
    /// Travel screen.
    Loading,
    /// In-match menu.
    MatchMenu,
    /// Match score list.
    Scoreboard,
    /// Results card.
    Results,
    /// Settings form.
    Settings,
    /// Available loadout.
    Loadout,
    /// Leave-match confirmation.
    LeaveConfirm,
}

/// Input-neutral events that change the flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowAction {
    /// The splash timer expired or assets finished loading.
    BootComplete,
    /// Home's primary action.
    Play,
    /// A playable mode was selected.
    ModeChosen,
    /// The room countdown reached its end.
    MatchStarted,
    /// The match world finished loading.
    LoadingComplete,
    /// Open the in-match menu.
    Pause,
    /// Resume the match.
    Resume,
    /// Open the scoreboard overlay.
    Scoreboard,
    /// Close the scoreboard overlay.
    CloseScoreboard,
    /// Open the leave confirmation.
    LeaveMatch,
    /// Confirm leaving the match.
    ConfirmLeave,
    /// The match ended.
    MatchEnded,
    /// Continue from results.
    Continue,
    /// Open settings.
    Settings,
    /// Open the loadout screen.
    Loadout,
    /// Close an auxiliary screen or cancel a confirmation.
    Cancel,
    /// Invoke Back.
    Back,
}

/// A typed operation for the menu presenter, or a request to travel to the front end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowEffect {
    /// Push a screen at the supplied layer.
    Push(Screen, Layer),
    /// Replace a layer's current screen.
    Replace(Screen, Layer),
    /// Pop the top screen at a layer.
    Pop(Layer),
    /// Rebuild the front-end world using menu-backdrop settings.
    TravelHome,
    /// No stack change.
    None,
}

impl Flow {
    /// Applies an event and returns the stack operation the presenter should perform.
    pub fn transition(&mut self, action: FlowAction) -> FlowEffect {
        use FlowAction as Action;
        use FlowEffect as Effect;
        use Layer::{GameMenu, Menu, Modal};
        use Screen as Ui;

        match (*self, action) {
            (Self::Boot, Action::BootComplete) => {
                *self = Self::Home;
                Effect::Replace(Ui::Home, Menu)
            }
            (Self::Home, Action::Play) => {
                *self = Self::ModeSelect;
                Effect::Replace(Ui::Modes, Menu)
            }
            (Self::ModeSelect, Action::ModeChosen) => {
                *self = Self::Lobby;
                Effect::Replace(Ui::Lobby, Menu)
            }
            (Self::Lobby, Action::MatchStarted) => {
                *self = Self::Loading;
                Effect::Replace(Ui::Loading, Menu)
            }
            (Self::Loading, Action::LoadingComplete) => {
                *self = Self::Match;
                Effect::Pop(Menu)
            }
            (Self::Match, Action::Pause | Action::Back) => Effect::Push(Ui::MatchMenu, GameMenu),
            (Self::Match, Action::Scoreboard) => Effect::Push(Ui::Scoreboard, GameMenu),
            (Self::Match, Action::CloseScoreboard | Action::Resume) => Effect::Pop(GameMenu),
            (Self::Match, Action::LeaveMatch) => Effect::Push(Ui::LeaveConfirm, Modal),
            (Self::Match, Action::ConfirmLeave) => {
                *self = Self::Home;
                Effect::TravelHome
            }
            (Self::Match, Action::MatchEnded) => {
                *self = Self::Results;
                Effect::Replace(Ui::Results, Menu)
            }
            (Self::Results, Action::Continue | Action::Back) => {
                *self = Self::Home;
                Effect::TravelHome
            }
            (Self::Home, Action::Settings) => Effect::Push(Ui::Settings, Menu),
            (Self::Home, Action::Loadout) => Effect::Push(Ui::Loadout, Menu),
            (Self::ModeSelect, Action::Back) => {
                *self = Self::Home;
                Effect::Replace(Ui::Home, Menu)
            }
            (Self::Lobby, Action::Back) => {
                *self = Self::Home;
                Effect::TravelHome
            }
            (Self::Home, Action::Back) => Effect::None,
            (_, Action::Cancel) => Effect::Pop(Modal),
            (_, Action::Settings) => Effect::Push(Ui::Settings, Menu),
            _ => Effect::None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn primary_flow_actions_follow_the_screen_table() {
        let cases = [
            (
                Flow::Boot,
                FlowAction::BootComplete,
                Flow::Home,
                FlowEffect::Replace(Screen::Home, Layer::Menu),
            ),
            (
                Flow::Home,
                FlowAction::Play,
                Flow::ModeSelect,
                FlowEffect::Replace(Screen::Modes, Layer::Menu),
            ),
            (
                Flow::ModeSelect,
                FlowAction::ModeChosen,
                Flow::Lobby,
                FlowEffect::Replace(Screen::Lobby, Layer::Menu),
            ),
            (
                Flow::Lobby,
                FlowAction::MatchStarted,
                Flow::Loading,
                FlowEffect::Replace(Screen::Loading, Layer::Menu),
            ),
            (
                Flow::Loading,
                FlowAction::LoadingComplete,
                Flow::Match,
                FlowEffect::Pop(Layer::Menu),
            ),
            (
                Flow::Match,
                FlowAction::MatchEnded,
                Flow::Results,
                FlowEffect::Replace(Screen::Results, Layer::Menu),
            ),
        ];
        for (initial, action, expected_flow, expected_effect) in cases {
            let mut flow = initial;
            assert_eq!(flow.transition(action), expected_effect);
            assert_eq!(flow, expected_flow);
        }
    }

    #[test]
    fn home_back_is_inert_and_leaving_match_is_confirmed() {
        let mut home = Flow::Home;
        assert_eq!(home.transition(FlowAction::Back), FlowEffect::None);
        let mut match_flow = Flow::Match;
        assert_eq!(
            match_flow.transition(FlowAction::LeaveMatch),
            FlowEffect::Push(Screen::LeaveConfirm, Layer::Modal)
        );
        assert_eq!(
            match_flow.transition(FlowAction::ConfirmLeave),
            FlowEffect::TravelHome
        );
    }
}
