//! Labs › HUD height's card while on: the slider and nudge buttons for how
//! far above the Dock (or the screen's bottom edge) the dictation HUD sits
//! when its position is Bottom.

use rune::{El, row, text};

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use super::super::lists::{inline_slider, small_button};
use crate::app_settings::HUD_BOTTOM_GAP_MAX;
use crate::desktop::i18n::{tr, tr_fill};

/// How wide the gap's slider is inside its card.
const SLIDER_WIDTH: f32 = 150.0;

pub(super) fn body(ui: &Ui, p: Paint) -> El {
    let saved = ui.settings.labs.hud_bottom_gap.min(HUD_BOTTOM_GAP_MAX);
    let gap = ui.lab_store.hud_gap.unwrap_or(saved);
    let display = tr_fill("{} pt", &gap.to_string());
    let value = f32::from(gap) / f32::from(HUD_BOTTOM_GAP_MAX);
    row()
        .key("hud-height-row")
        .gap(8.0)
        .center_y()
        .flex_wrap()
        .child(
            text(tr("Height"))
                .key("label")
                .font(DESCRIPTION)
                .color(p.muted()),
        )
        .child(inline_slider(
            p,
            "hud-height-slider",
            tr("Height"),
            value,
            display,
            SLIDER_WIDTH,
            |ui: &mut Ui, position, done| set_gap(ui, position, done),
        ))
        .child(small_button(p, "hud-height-down", tr("−"), |ui, _| {
            nudge(ui, false)
        }))
        .child(small_button(p, "hud-height-up", tr("+"), |ui, _| {
            nudge(ui, true)
        }))
}

/// The point gap `position` names, rounded to whole points.
fn gap_at(position: f32) -> u16 {
    (f64::from(position.clamp(0.0, 1.0)) * f64::from(HUD_BOTTOM_GAP_MAX)).round() as u16
}

/// Follows the slider while it drags and saves where it lets go. A cancelled
/// drag puts the value from the press back.
fn set_gap(ui: &mut Ui, position: f32, done: bool) {
    let gap = gap_at(position);
    let saved = ui.settings.labs.hud_bottom_gap.min(HUD_BOTTOM_GAP_MAX);
    ui.lab_store.hud_gap = (!done && gap != saved).then_some(gap);
    if done && ui.settings.labs.hud_bottom_gap != gap {
        ui.update(|settings| settings.labs.hud_bottom_gap = gap);
    }
}

/// One point up or down, keeping the gap in the offered range.
fn nudge(ui: &mut Ui, up: bool) {
    let saved = ui.settings.labs.hud_bottom_gap.min(HUD_BOTTOM_GAP_MAX);
    let gap = if up {
        saved.saturating_add(1).min(HUD_BOTTOM_GAP_MAX)
    } else {
        saved.saturating_sub(1)
    };
    ui.lab_store.hud_gap = None;
    if ui.settings.labs.hud_bottom_gap != gap {
        ui.update(|settings| settings.labs.hud_bottom_gap = gap);
    }
}
