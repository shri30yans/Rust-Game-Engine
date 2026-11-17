//! Platform input mapping, stack operations and typed event dispatch.

use app::AppError;
use engine_ui::{Document, Layer, UiEvent};
use glam::Vec2;

use crate::flow::Screen;

use super::actions::{MenuAction, SettingControl};
use super::bindings::setting_control;
use super::runtime::{ActiveScreen, GameMenus};

impl GameMenus {
    /// Advances menu transitions, routes current platform input and returns typed game actions.
    ///
    /// # Errors
    ///
    /// Returns a UI error for a binding that does not match a document widget.
    pub fn frame(
        &mut self,
        input: &platform::Input,
        screen: Vec2,
        elapsed_ms: u64,
    ) -> Result<&[MenuAction], AppError> {
        self.sync_active_documents()?;
        self.mapper.map(input, &mut self.input);
        self.now_ms = self.now_ms.saturating_add(elapsed_ms);
        self.last_screen = (screen.x, screen.y);
        let focused = self
            .active
            .iter()
            .rev()
            .find(|entry| entry.layer != Layer::Toast)
            .map(|entry| (entry.screen, entry.layer, entry.id));
        self.events.clear();
        let events =
            self.runtime
                .frame(&self.input, &self.bindings, self.last_screen, self.now_ms)?;
        self.raw_events.clear();
        self.raw_events.extend_from_slice(events);
        let raw_events = &self.raw_events;
        let mut popped_by_back = false;
        if let Some((screen, layer, screen_id)) = focused {
            for event in raw_events {
                if let UiEvent::Back(_) = event {
                    popped_by_back = true;
                }
                if let Some(action) = self.map_event(screen_id, screen, *event) {
                    self.events.push(action);
                }
            }
            if popped_by_back
                && let Some(index) = self
                    .active
                    .iter()
                    .rposition(|entry| entry.layer == layer && entry.id == screen_id)
            {
                self.active.remove(index);
            }
        }
        Ok(&self.events)
    }

    /// Applies a flow operation to the screen stack.
    ///
    /// # Errors
    ///
    /// Returns a UI error if the requested stack operation is invalid.
    pub fn apply(&mut self, effect: crate::flow::FlowEffect) -> Result<(), AppError> {
        use crate::flow::FlowEffect;
        match effect {
            FlowEffect::Push(screen, layer) => self.push(screen, layer)?,
            FlowEffect::Replace(screen, layer) => {
                if self.active.iter().any(|entry| entry.layer == layer) {
                    self.pop(layer)?;
                }
                self.push(screen, layer)?;
            }
            FlowEffect::Pop(layer) => self.pop(layer)?,
            FlowEffect::TravelHome => {
                for layer in [Layer::Modal, Layer::Menu, Layer::GameMenu] {
                    while self.active.iter().any(|entry| entry.layer == layer) {
                        self.pop(layer)?;
                    }
                }
                self.push(Screen::Home, Layer::Menu)?;
            }
            FlowEffect::None => {}
        }
        Ok(())
    }

    pub(super) fn push(&mut self, screen: Screen, layer: Layer) -> Result<(), AppError> {
        let document = Document::from_ron(self.document_source(screen))?;
        let id = self.runtime.push(layer, document)?;
        self.active.push(ActiveScreen {
            screen,
            layer,
            id,
            document_version: self.document_version(screen),
        });
        Ok(())
    }

    pub(super) fn pop(&mut self, layer: Layer) -> Result<(), AppError> {
        self.runtime.pop(layer)?;
        if let Some(index) = self.active.iter().rposition(|entry| entry.layer == layer) {
            self.active.remove(index);
        }
        Ok(())
    }

    fn map_event(
        &self,
        screen_id: engine_ui::ScreenId,
        screen: Screen,
        event: UiEvent,
    ) -> Option<MenuAction> {
        let (widget, row) = match event {
            UiEvent::Action(widget, row) => (widget, row.map(|index| usize::from(index.0))),
            UiEvent::Changed(widget, _, value) => {
                return setting_control(
                    self.runtime.widget_id(screen_id, "sens_x"),
                    widget,
                    SettingControl::SensitivityX,
                )
                .map(|control| MenuAction::SettingChanged(control, value))
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "sens_y"),
                        widget,
                        SettingControl::SensitivityY,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                })
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "volume"),
                        widget,
                        SettingControl::Volume,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                })
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "music_volume"),
                        widget,
                        SettingControl::MusicVolume,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                })
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "sfx_volume"),
                        widget,
                        SettingControl::SfxVolume,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                })
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "voice_volume"),
                        widget,
                        SettingControl::VoiceVolume,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                })
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "render_scale"),
                        widget,
                        SettingControl::RenderScale,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                })
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "fov"),
                        widget,
                        SettingControl::FieldOfView,
                    )
                    .map(|control| MenuAction::SettingChanged(control, value))
                });
            }
            UiEvent::Toggled(widget, _, value) => {
                return setting_control(
                    self.runtime.widget_id(screen_id, "reduce_motion"),
                    widget,
                    SettingControl::ReduceMotion,
                )
                .or_else(|| {
                    setting_control(
                        self.runtime.widget_id(screen_id, "aim_assist"),
                        widget,
                        SettingControl::AimAssist,
                    )
                    .or_else(|| {
                        setting_control(
                            self.runtime.widget_id(screen_id, "left_fire"),
                            widget,
                            SettingControl::LeftFire,
                        )
                    })
                })
                .map(|control| MenuAction::SettingToggled(control, value));
            }
            UiEvent::Selected(widget, _, value) => {
                return if self.runtime.widget_id(screen_id, "tabs") == Some(widget) {
                    Some(MenuAction::SettingsTab(value))
                } else if self.runtime.widget_id(screen_id, "quality") == Some(widget) {
                    Some(MenuAction::QualityTier(value))
                } else {
                    None
                };
            }
            UiEvent::Back(_) => return Some(MenuAction::Back),
        };
        let is = |name| self.runtime.widget_id(screen_id, name) == Some(widget);
        match screen {
            Screen::Home if is("play") => Some(MenuAction::Play),
            Screen::Home if is("settings") || is("settings_top") => Some(MenuAction::Settings),
            Screen::Home if is("loadout") => Some(MenuAction::Loadout),
            Screen::Modes if is("mode_select") => row.map(MenuAction::SelectMode),
            Screen::Lobby if is("add_bot") => Some(MenuAction::AddBot),
            Screen::Lobby if is("remove_bot") => Some(MenuAction::RemoveBot),
            Screen::Lobby if is("team") => row.map(MenuAction::ChangeTeam),
            Screen::Lobby if is("kick") => row.map(MenuAction::Kick),
            Screen::Lobby if is("ready") => Some(MenuAction::Ready),
            Screen::Lobby if is("start") => Some(MenuAction::Start),
            Screen::MatchMenu if is("resume") => Some(MenuAction::Resume),
            Screen::MatchMenu if is("settings") => Some(MenuAction::Settings),
            Screen::MatchMenu if is("scoreboard") => Some(MenuAction::Scoreboard),
            Screen::MatchMenu if is("leave") => Some(MenuAction::LeaveMatch),
            Screen::Scoreboard if is("close") => Some(MenuAction::CloseScoreboard),
            Screen::Results if is("continue") => Some(MenuAction::Continue),
            Screen::Settings if is("fire_mode") => Some(MenuAction::FireMode),
            Screen::Settings if is("confirm") => Some(MenuAction::ConfirmGraphics),
            Screen::Settings if is("close") => Some(MenuAction::Cancel),
            Screen::Loadout if is("close") => Some(MenuAction::Cancel),
            Screen::LeaveConfirm if is("cancel") => Some(MenuAction::Cancel),
            Screen::LeaveConfirm if is("confirm") => Some(MenuAction::Confirm),
            _ => None,
        }
    }
}
