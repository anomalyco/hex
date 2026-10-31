//! HUD Lab (macOS dev builds, when `DEVELOPER_FEATURES_ENABLED` or the
//! `hud-lab` experiment is on): tunes Dot scope, the dictation HUD, live. Pick the
//! state it shows and drag its sliders; every change goes to the HUD as
//! `PlatformRequest::ConfigureHud` or `TuneHud`. Save writes
//! `dot-scope.json`, which the HUD loads at launch; Copy values copies the
//! same JSON; Reset restores the prototype's values. Developer copy is
//! English.

use rune::{El, Tx, col, row, text};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, Paint, bare_page, button, card, segmented, setting_row, toggle_row,
};
use super::lists::{group_label, slider};
use super::platform::{HudDemo, HudLab, PlatformRequest};
use crate::desktop::i18n::tr;
use crate::rune_hud::dot_scope::{self, KNOBS, Placement, Processing, Tuning};

const SLIDER_WIDTH: f32 = 360.0;

/// Applies a change and shows it on the indicator. A tuning-only change
/// applies live to whatever the HUD shows; a new demo state restarts the
/// demo.
fn configure(ui: &mut Ui, change: impl FnOnce(&mut HudLab)) {
    let old = ui.platform.hud;
    let mut lab = old;
    change(&mut lab);
    if lab == old {
        return;
    }
    ui.hud_copied = false;
    ui.hud_saved = false;
    ui.platform.hud = lab;
    dot_scope::set_lab(lab.tuning);
    ui.ask(if lab.demo == old.demo {
        PlatformRequest::TuneHud(lab)
    } else {
        PlatformRequest::ConfigureHud(lab)
    });
}

/// Dot scope's sliders, grouped by `KNOBS` key (shared with the HUD
/// stories in Playground).
pub(super) const DOT_SCOPE_GROUPS: [(&str, &[&str]); 5] = [
    (
        "Field",
        &[
            "dot-columns",
            "dot-rows",
            "dot-spacing",
            "dot-width",
            "dot-processing-width",
            "dot-height",
            "dot-island-spread",
        ],
    ),
    ("Braid", &["dot-braid-speed", "dot-helix"]),
    (
        "Response",
        &[
            "dot-gain",
            "dot-auto-release",
            "dot-noise-floor",
            "dot-response",
            "dot-release",
            "dot-history",
            "dot-phosphor",
        ],
    ),
    (
        "Light",
        &[
            "dot-glow",
            "dot-saturation",
            "dot-shimmer-pass",
            "dot-shimmer",
        ],
    ),
    ("Transition", &["dot-transition", "dot-bounce"]),
];

fn dot_scope_actions(p: Paint, ui: &Ui) -> Vec<El> {
    vec![
        button(
            p,
            "dot-scope-reset",
            "Reset",
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| configure(ui, |lab| lab.tuning = Tuning::default()),
        ),
        button(
            p,
            "dot-scope-copy",
            if ui.hud_copied {
                tr("Copied")
            } else {
                "Copy values"
            },
            ButtonKind::Secondary,
            |ui: &mut Ui, tx: &mut Tx| {
                tx.copy(ui.platform.hud.tuning.json());
                ui.hud_copied = true;
            },
        ),
        button(
            p,
            "dot-scope-save",
            if ui.hud_saved { "Saved" } else { "Save" },
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| {
                ui.ask(PlatformRequest::SaveDotScope(ui.platform.hud.tuning));
                ui.hud_saved = true;
            },
        ),
    ]
}

/// Changes Dot scope's tuning from HUD Lab.
fn tune(ui: &mut Ui, change: impl FnOnce(&mut Tuning)) {
    configure(ui, |lab| change(&mut lab.tuning));
}

/// Dot scope's choices: how processing looks, where the HUD sits, and how
/// the voice is levelled.
fn dot_scope_choices(p: Paint, tuning: &Tuning) -> El {
    let processing = Processing::ALL
        .iter()
        .position(|look| *look == tuning.processing)
        .unwrap_or(0);
    let placement = Placement::ALL
        .iter()
        .position(|place| *place == tuning.placement)
        .unwrap_or(0);
    let rows = vec![
        setting_row(
            p,
            "dot-processing",
            "Processing",
            Some("How the HUD shows it is working. Done always clears from the middle out"),
            segmented(
                p,
                "segments",
                &Processing::ALL.map(Processing::label),
                processing,
                |ui, index| tune(ui, |tuning| tuning.processing = Processing::ALL[index]),
            ),
        ),
        setting_row(
            p,
            "dot-placement",
            "Placement",
            Some(tuning.placement.label()),
            segmented(
                p,
                "segments",
                &["Below", "Menu bar", "Notch"],
                placement,
                |ui, index| tune(ui, |tuning| tuning.placement = Placement::ALL[index]),
            ),
        ),
        toggle_row(
            p,
            "dot-pretend-notch",
            "Pretend notch",
            "Draws From the notch's island on a screen without one",
            tuning.pretend_notch,
            |ui: &mut Ui| tune(ui, |tuning| tuning.pretend_notch = !tuning.pretend_notch),
        ),
        toggle_row(
            p,
            "dot-auto-level",
            "Auto level",
            "Follows the voice's recent peak, so a quiet voice fills the field too",
            tuning.auto_level,
            |ui: &mut Ui| tune(ui, |tuning| tuning.auto_level = !tuning.auto_level),
        ),
    ];
    card(p, "dot-scope-choices", rows)
}

fn dot_scope_sliders(p: Paint, tuning: &Tuning) -> El {
    let mut sliders = col().key("dot-scope-sliders").gap(16.0);
    for (index, (group, keys)) in DOT_SCOPE_GROUPS.into_iter().enumerate() {
        sliders = sliders.child(group_label(
            p,
            &format!("dot-scope-group{index}"),
            group,
            index == 0,
        ));
        for key in keys {
            let Some(knob) = KNOBS.iter().find(|knob| knob.key == *key).copied() else {
                continue;
            };
            sliders = sliders.child(slider(
                p,
                knob.key,
                knob.label,
                knob.fraction(tuning),
                knob.display(tuning),
                SLIDER_WIDTH,
                move |ui, value, _| tune(ui, |tuning| knob.set_fraction(tuning, value)),
            ));
        }
    }
    sliders
}

pub(super) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let lab = &ui.platform.hud;
    let demo_index = match lab.demo {
        HudDemo::Recording => 0,
        HudDemo::Transcribing => 1,
        HudDemo::Processing => 2,
    };
    let demo = row()
        .key("hud-state")
        .gap(16.0)
        .center_y()
        .flex_wrap()
        .child(segmented(
            p,
            "hud-demo",
            &[tr("Recording"), tr("Transcribing"), tr("Processing")],
            demo_index,
            |ui, index| {
                let demo = [
                    HudDemo::Recording,
                    HudDemo::Transcribing,
                    HudDemo::Processing,
                ][index];
                configure(ui, |lab| lab.demo = demo);
            },
        ));
    let note = text(
        "Recording follows a synthetic voice here. Save keeps these values for the next launch.",
    )
    .key("dot-scope-note")
    .font(DESCRIPTION)
    .color(p.muted())
    .wrap();
    let content = bare_page("content")
        .child(demo)
        .child(note)
        .child(dot_scope_choices(p, &lab.tuning))
        .child(dot_scope_sliders(p, &lab.tuning));
    (
        dot_scope_actions(p, ui),
        super::scroll_body("hud-lab-scroll", content),
    )
}
