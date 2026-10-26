//! HUD Lab (macOS debug builds): tunes the dictation indicator live. Pick a
//! style, the state it shows and how many dictations wait behind it, and
//! drag the look's sliders; every change goes to the indicator as
//! `PlatformRequest::ConfigureHud`. Copy preset copies the tuning as one
//! line. Developer copy is English.

use rune::{El, Tx, col, row, text};

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, InstantHover, Paint, button, segmented};
use super::lists::slider;
use super::platform::{HudDemo, HudLab, HudTuning, PlatformRequest};
use crate::desktop_i18n::tr;

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

/// Applies a change and shows it on the indicator.
fn configure(ui: &mut Ui, change: impl FnOnce(&mut HudLab)) {
    let mut lab = ui.platform.hud;
    change(&mut lab);
    if lab != ui.platform.hud {
        ui.hud_copied = false;
    }
    ui.platform.hud = lab;
    ui.ask(PlatformRequest::ConfigureHud(lab));
}

pub(super) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let lab = &ui.platform.hud;
    let actions = vec![button(
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
    )];
    let demo_index = match lab.demo {
        HudDemo::Recording => 0,
        HudDemo::Transcribing => 1,
        HudDemo::Processing => 2,
    };
    let controls = row()
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
        ))
        .child(
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
        );
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
                .instant_hover()
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
    let content = col()
        .key("content")
        .px(24.0)
        .pt(4.0)
        .gap(24.0)
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
