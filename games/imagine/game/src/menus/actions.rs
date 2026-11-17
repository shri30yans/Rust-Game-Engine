//! Game actions emitted by menu screens and settings controls.

/// A user action emitted by an IMAGINE document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
    /// Start mode selection.
    Play,
    /// Open settings.
    Settings,
    /// Open loadout.
    Loadout,
    /// Select a mode row.
    SelectMode(usize),
    /// Add a lobby bot.
    AddBot,
    /// Remove the last lobby bot.
    RemoveBot,
    /// Cycle a seat's team.
    ChangeTeam(usize),
    /// Kick the selected seat.
    Kick(usize),
    /// Toggle local readiness.
    Ready,
    /// Start the host countdown.
    Start,
    /// Resume active play.
    Resume,
    /// Open the match scoreboard.
    Scoreboard,
    /// Close the scoreboard.
    CloseScoreboard,
    /// Show the leave confirmation.
    LeaveMatch,
    /// Confirm a destructive choice.
    Confirm,
    /// Cancel a modal or close an auxiliary screen.
    Cancel,
    /// Back was pressed on a screen that uses the stack's default Back behavior.
    Back,
    /// Continue from the results screen.
    Continue,
    /// Toggle firing mode.
    FireMode,
    /// Confirm pending graphics settings.
    ConfirmGraphics,
    /// A settings range changed.
    SettingChanged(SettingControl, f32),
    /// A settings toggle changed.
    SettingToggled(SettingControl, bool),
    /// A settings tab changed.
    SettingsTab(u16),
    /// A graphics quality preset changed.
    QualityTier(u16),
}

/// A settings control with a bound value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingControl {
    /// Horizontal look speed.
    SensitivityX,
    /// Vertical look speed.
    SensitivityY,
    /// Master sound level.
    Volume,
    /// Music sound level.
    MusicVolume,
    /// Sound effects sound level.
    SfxVolume,
    /// Voice sound level.
    VoiceVolume,
    /// Render scale percentage.
    RenderScale,
    /// Vertical field of view.
    FieldOfView,
    /// Reduced motion.
    ReduceMotion,
    /// Touch aim slowdown.
    AimAssist,
    /// Optional left fire button.
    LeftFire,
}
