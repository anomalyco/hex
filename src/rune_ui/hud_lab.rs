//! HUD Lab (macOS debug builds, and release builds while the `dot-scope`
//! experiment is on): tunes the dictation indicator live. Pick a
//! style, the state it shows and how many dictations wait behind it, and
//! drag the look's sliders; every change goes to the indicator as
//! `PlatformRequest::ConfigureHud`. Copy preset copies the tuning as one
//! line. Developer copy is English.
//!
//! With the `dot-scope` experiment on, the HUD draws Dot scope and HUD Lab
//! tunes it instead: its sliders, Save (`dot-scope.json`, loaded at launch),
//! Copy values (the same JSON), and Reset to the prototype's values. Only
//! that section shows then, so a release build never shows the capsule's
//! styles.

use rune::{El, Tx, col, row, text};

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, Paint, bare_page, button, segmented};
use super::lists::{group_label, slider};
use super::platform::{HudDemo, HudLab, HudTuning, PlatformRequest};
use crate::desktop_i18n::tr;
use crate::dot_scope::{KNOBS, Tuning};

/// `dictation_indicator`'s fastest animation speed.
const MAX_SPEED: f32 = 24.0;
const SLIDER_WIDTH: f32 = 360.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Control {
    LineCount,
    Curvature,
    Speed,
    Blur,
    Glow,
    Depth,
    LightAngle,
    Outline,
}

impl Control {
    pub(super) const ALL: [(Control, &'static str, &'static str); 8] = [
        (Control::LineCount, "hud-line-count", "Line count"),
        (Control::Curvature, "hud-curvature", "Curvature"),
        (Control::Speed, "hud-speed", "Speed"),
        (Control::Blur, "hud-blur", "Highlight blur"),
        (Control::Glow, "hud-glow", "Line glow"),
        (Control::Depth, "hud-depth", "Sphere depth"),
        (Control::LightAngle, "hud-light-angle", "Light angle"),
        (Control::Outline, "hud-outline", "Sphere outline"),
    ];

    pub(super) fn value(self, t: &HudTuning) -> f32 {
        match self {
            Control::LineCount => (t.line_count - 1.0) / 5.0,
            Control::Curvature => t.curvature,
            Control::Speed => t.speed / MAX_SPEED,
            Control::Blur => 1.0 - t.sharpness,
            Control::Glow => t.glow,
            Control::Depth => t.depth,
            Control::LightAngle => t.light_angle,
            Control::Outline => t.outline,
        }
        .clamp(0.0, 1.0)
    }

    pub(super) fn set(self, t: &mut HudTuning, value: f32) {
        let value = value.clamp(0.0, 1.0);
        match self {
            Control::LineCount => t.line_count = (1.0 + value * 5.0).round(),
            Control::Curvature => t.curvature = value,
            Control::Speed => t.speed = value * MAX_SPEED,
            Control::Blur => t.sharpness = 1.0 - value,
            Control::Glow => t.glow = value,
            Control::Depth => t.depth = value,
            Control::LightAngle => t.light_angle = value,
            Control::Outline => t.outline = value,
        }
    }

    fn display(self, t: &HudTuning) -> String {
        match self {
            Control::LineCount => format!("{:.0}", t.line_count),
            Control::Speed => format!("{:.2}x", t.speed),
            Control::LightAngle => format!("{:.0}°", t.light_angle * 360.0),
            _ => format!("{:.2}", self.value(t)),
        }
    }
}

/// The three styles' starting tunings.
pub(super) fn style(index: usize) -> HudTuning {
    match index {
        0 => HudTuning {
            style: 0.0,
            line_count: 1.0,
            curvature: 0.5,
            speed: 0.7,
            sharpness: 0.42,
            glow: 0.4,
            depth: 0.75,
            light_angle: 0.35,
            outline: 0.25,
        },
        1 => HudTuning::default(),
        _ => HudTuning {
            style: 2.0,
            line_count: 3.0,
            curvature: 0.7,
            speed: 0.9,
            sharpness: 0.64,
            glow: 0.28,
            depth: 0.74,
            light_angle: 0.35,
            outline: 0.3,
        },
    }
}

fn preset(t: &HudTuning) -> String {
    format!(
        "hud-v1 style={} lines={:.0} curvature={:.2} speed={:.2} blur={:.2} glow={:.2} depth={:.2} light={:.2} outline={:.2}",
        t.style as usize,
        t.line_count,
        t.curvature,
        t.speed,
        1.0 - t.sharpness,
        t.glow,
        t.depth,
        t.light_angle,
        t.outline,
    )
}

/// Applies a change and shows it on the indicator. A tuning-only change
/// applies live to whatever the HUD shows; anything else restarts the demo.
fn configure(ui: &mut Ui, change: impl FnOnce(&mut HudLab)) {
    let old = ui.platform.hud;
    let mut lab = old;
    change(&mut lab);
    let tuned = lab.tuning != old.tuning || lab.dot_scope != old.dot_scope;
    if tuned {
        ui.hud_copied = false;
        ui.hud_saved = false;
    }
    ui.platform.hud = lab;
    ui.ask(
        if tuned && lab.demo == old.demo && lab.queued == old.queued {
            PlatformRequest::TuneHud(lab)
        } else {
            PlatformRequest::ConfigureHud(lab)
        },
    );
}

/// Dot scope's sliders, grouped as HUD Lab shows them, by `KNOBS` key.
const DOT_SCOPE_GROUPS: [(&str, &[&str]); 5] = [
    (
        "Field",
        &[
            "dot-columns",
            "dot-rows",
            "dot-spacing",
            "dot-width",
            "dot-processing-width",
            "dot-height",
        ],
    ),
    ("Braid", &["dot-braid-speed", "dot-helix"]),
    (
        "Response",
        &[
            "dot-gain",
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
            |ui: &mut Ui, _: &mut Tx| configure(ui, |lab| lab.dot_scope = Some(Tuning::default())),
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
                if let Some(tuning) = ui.platform.hud.dot_scope {
                    tx.copy(tuning.json());
                    ui.hud_copied = true;
                }
            },
        ),
        button(
            p,
            "dot-scope-save",
            if ui.hud_saved { "Saved" } else { "Save" },
            ButtonKind::Secondary,
            |ui: &mut Ui, _: &mut Tx| {
                if let Some(tuning) = ui.platform.hud.dot_scope {
                    ui.ask(PlatformRequest::SaveDotScope(tuning));
                    ui.hud_saved = true;
                }
            },
        ),
    ]
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
                move |ui, value, _| {
                    configure(ui, |lab| {
                        if let Some(tuning) = &mut lab.dot_scope {
                            knob.set_fraction(tuning, value);
                        }
                    })
                },
            ));
        }
    }
    sliders
}

pub(super) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let lab = &ui.platform.hud;
    let actions = if lab.dot_scope.is_some() {
        dot_scope_actions(p, ui)
    } else {
        vec![button(
            p,
            "copy-hud-preset",
            if ui.hud_copied {
                tr("Copied")
            } else {
                "Copy preset"
            },
            ButtonKind::Secondary,
            |ui: &mut Ui, tx: &mut Tx| {
                tx.copy(preset(&ui.platform.hud.tuning));
                ui.hud_copied = true;
            },
        )]
    };
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
    let controls = if lab.dot_scope.is_some() {
        // Dot scope draws no queue.
        demo
    } else {
        demo.child(
            row()
                .key("queued")
                .gap(8.0)
                .center_y()
                .child(
                    text("Queued")
                        .key("label")
                        .font(DESCRIPTION)
                        .color(p.muted()),
                )
                .child(segmented(
                    p,
                    "hud-queued",
                    &["0", "1", "2", "3"],
                    lab.queued.min(3),
                    |ui, index| {
                        configure(ui, |lab| lab.queued = index);
                    },
                )),
        )
    };
    let c = &p.k.c;
    let styles = [
        (
            "A",
            "Moving light",
            "A line-free area light travels around the orb.",
        ),
        (
            "B",
            "Meridians",
            "Highlights wrap around the sphere instead of crossing it.",
        ),
        (
            "C",
            "Ribbons",
            "Curved bands retained for comparison and tuning.",
        ),
    ];
    let mut cards = row()
        .key("hud-styles")
        .role("radiogroup")
        .focus_group()
        .gap(12.0)
        .flex_wrap();
    for (index, (letter, name, description)) in styles.into_iter().enumerate() {
        let selected = lab.tuning.style as usize == index;
        cards = cards.child(
            col()
                .key(format!("hud-style{index}"))
                .role("radio")
                .label(name)
                .focusable(true)
                .focus_selected(selected)
                .focus_ring(2.0, 2.5, c.border_focus)
                .w(190.0)
                .padding(12.0)
                .gap(4.0)
                .radius(p.k.radius.lg)
                .border(
                    p.hair.max(0.5),
                    if selected {
                        c.info.border
                    } else {
                        c.border_weak
                    },
                )
                .fill(if selected { c.info.bg } else { c.bg_base })
                .hover(c.hover)
                .press(c.pressed)
                .on_press(move |ui: &mut Ui| configure(ui, |lab| lab.tuning = style(index)))
                .child(
                    text(letter)
                        .key("letter")
                        .font(p.k.ty.small_medium)
                        .color(p.accent()),
                )
                .child(
                    text(name)
                        .key("name")
                        .font(p.k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(
                    text(description)
                        .key("description")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                ),
        );
    }
    if let Some(tuning) = &lab.dot_scope {
        let note = text(
            "Recording follows a synthetic voice here. Save keeps these values for the next \
             launch.",
        )
        .key("dot-scope-note")
        .font(DESCRIPTION)
        .color(p.muted())
        .wrap();
        let content = bare_page("content")
            .child(controls)
            .child(note)
            .child(dot_scope_sliders(p, tuning));
        return (actions, super::scroll_body("hud-lab-scroll", content));
    }
    let mut sliders = col().key("hud-sliders").gap(16.0);
    for (control, key, label) in Control::ALL {
        sliders = sliders.child(slider(
            p,
            key,
            label,
            control.value(&lab.tuning),
            control.display(&lab.tuning),
            SLIDER_WIDTH,
            move |ui, value, _| configure(ui, |lab| control.set(&mut lab.tuning, value)),
        ));
    }
    let content = bare_page("content")
        .child(controls)
        .child(cards)
        .child(sliders)
        .child(
            text(preset(&lab.tuning))
                .key("hud-preset")
                .font(p.k.ty.mono)
                .color(p.muted())
                .wrap()
                .padding(12.0)
                .radius(p.k.radius.md)
                .border(p.hair.max(0.5), c.border_weak)
                .fill(c.layer_01),
        );
    (actions, super::scroll_body("hud-lab-scroll", content))
}
