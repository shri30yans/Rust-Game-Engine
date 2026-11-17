//! Default IMAGINE bindings and typed list row construction.

use std::collections::BTreeMap;

use crate::data::ModeDefinition;
use engine_ui::{ListBinding, ListRecord, Value, WidgetId};

use super::actions::SettingControl;
use super::runtime::GameMenus;

pub(super) fn display_number(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

pub(super) fn setting_control(
    known: Option<WidgetId>,
    event: WidgetId,
    control: SettingControl,
) -> Option<SettingControl> {
    (known == Some(event)).then_some(control)
}

impl GameMenus {
    pub(super) fn bind_defaults(&mut self, mode_data: &[ModeDefinition]) {
        self.bindings.set_text("build", "DEV");
        self.bindings.set_text("player", "PLAYER");
        self.bindings.set_num("mode_index", 0.0);
        let records = mode_data
            .iter()
            .map(|mode| {
                record(&[
                    ("name", Value::Text(mode.name.clone())),
                    ("players", Value::Text(mode.players_label())),
                    ("length", Value::Text(mode.length.clone())),
                    ("rule", Value::Text(mode.rule.clone())),
                    ("disabled", Value::Flag(!mode.available)),
                    (
                        "action",
                        Value::Text(
                            if mode.available {
                                "SELECT"
                            } else {
                                "COMING SOON"
                            }
                            .into(),
                        ),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        self.bindings.set_list("modes", list(&records));
        self.bindings.set_text("room_code", "------");
        self.bindings.set_text("mode", "PRACTICE");
        self.bindings.set_text("arena", "KILN YARD");
        self.bindings.set_num("seat_count", 1.0);
        self.bindings.set_num("seat_capacity", 0.0);
        self.bindings.set_flag("countdown", false);
        self.bindings.set_text("countdown_text", "");
        self.bindings.set_list("seats", list(&[]));
        self.bindings.set_text("ready_label", "READY");
        self.bindings.set_flag("cannot_start", true);
        self.bindings.set_flag("bot_capacity_reached", true);
        self.bindings.set_flag("no_bots_to_remove", true);
        self.bindings.set_text("toast", "");
        self.bindings.set_num("load_progress", 1.0);
        self.bindings
            .set_text("tip", "USE COVER TO BREAK LINE OF SIGHT.");
        self.bindings.set_list("scores", list(&[]));
        self.bindings.set_num("kills", 0.0);
        self.bindings.set_num("deaths", 0.0);
        self.bindings.set_num("score", 0.0);
        let tabs = ["GRAPHICS", "CONTROLS", "AUDIO", "ACCESSIBILITY"]
            .map(|label| record(&[("label", Value::Text(label.into()))]));
        self.bindings.set_list("tabs", list(&tabs));
        self.bindings.set_num("tab_index", 0.0);
        self.bindings.set_flag("graphics", true);
        self.bindings.set_flag("controls", false);
        self.bindings.set_flag("audio", false);
        self.bindings.set_flag("accessibility", false);
        self.bindings.set_text("quality", "MEDIUM");
        self.bindings.set_flag("quality_runtime_unavailable", true);
        let quality_options = ["LOW", "MEDIUM", "HIGH", "ULTRA"]
            .map(|label| record(&[("label", Value::Text(label.into()))]));
        self.bindings
            .set_list("quality_options", list(&quality_options));
        self.bindings.set_num("quality_index", 1.0);
        self.bindings.set_num("render_scale", 1.0);
        self.bindings.set_num("fov", 55.0);
        self.bindings.set_num("sens_x", 10.0);
        self.bindings.set_num("sens_y", 10.0);
        self.bindings.set_text("fire_mode", "MANUAL");
        self.bindings.set_num("volume", 80.0);
        self.bindings.set_num("music_volume", 80.0);
        self.bindings.set_num("sfx_volume", 80.0);
        self.bindings.set_num("voice_volume", 80.0);
        self.bindings.set_flag("reduce_motion", false);
        self.bindings.set_flag("aim_assist", true);
        self.bindings.set_flag("left_fire", false);
        self.bindings.set_text("title", "LEAVE MATCH?");
        self.bindings
            .set_text("body", "LEAVE THIS PRACTICE SESSION?");
    }
}

/// Creates a retained list binding with the supplied rows.
#[must_use]
pub fn list(records: &[ListRecord]) -> ListBinding {
    let mut value = ListBinding::with_capacity(records.len());
    value.replace(records);
    value
}

/// Builds one named list row for menu data.
#[must_use]
pub fn record(fields: &[(&str, Value)]) -> ListRecord {
    fields
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>()
}
