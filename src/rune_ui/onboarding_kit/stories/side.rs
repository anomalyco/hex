//! The playground's side panel for the onboarding stories: flow, shortcut,
//! motion, and the Without Hex scene's timing knobs.

use super::*;

fn side_chip(
    p: Paint,
    key: &str,
    label: &str,
    active: bool,
    on_press: impl Fn(&mut Onboard) + 'static,
) -> El {
    let k = p.k;
    let c = &k.c;
    row()
        .key(key.to_string())
        .role("button")
        .label(label)
        .focusable(true)
        .w_full()
        .h(26.0)
        .px(8.0)
        .gap(7.0)
        .center_y()
        .radius(6.0)
        .fill(if active {
            c.surface_raised
        } else {
            rune::Color::TRANSPARENT
        })
        .border(
            p.hair,
            if active {
                c.border_base
            } else {
                rune::Color::TRANSPARENT
            },
        )
        .hover(c.hover)
        .press(c.pressed)
        .on_press(on_press)
        .child(el().key("dot").size(5.0, 5.0).radius(2.5).fill(if active {
            p.text()
        } else {
            c.border_base
        }))
        .child(
            text(label)
                .key("lbl")
                .font(if active {
                    k.ty.small_medium
                } else {
                    k.ty.small
                })
                .color(if active { p.text() } else { p.muted() }),
        )
}

fn side_section(p: Paint, key: &str, heading: &str, items: Vec<El>) -> El {
    let k = p.k;
    col()
        .key(key.to_string())
        .w(SIDE_W)
        .gap(3.0)
        .child(
            text(heading)
                .key("hdr")
                .font(TextStyle::new(11.0, 560.0).lh(15.0))
                .color(p.faint()),
        )
        .child(
            col()
                .key("box")
                .w_full()
                .padding(3.0)
                .gap(1.0)
                .radius(k.radius.md)
                .fill(k.c.bg_base)
                .border(p.hair, k.c.border_weak)
                .children(items),
        )
}

pub(super) fn side(s: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let mut out = col().key("onboarding-side").w(SIDE_W).gap(12.0);

    if s.flow == Flow::FirstRun {
        let step_items = vec![
            side_chip(
                p,
                "step-demo",
                "1 · Without Hex",
                matches!(s.step, Step::Pick | Step::Started | Step::Parked),
                Onboard::jump_to_demo,
            ),
            side_chip(
                p,
                "step-mic",
                "2 · Microphone",
                s.step == Step::Microphone,
                Onboard::jump_to_microphone,
            ),
            side_chip(
                p,
                "step-waiting",
                "· Almost ready",
                s.step == Step::Waiting,
                Onboard::jump_to_download_wait,
            ),
            side_chip(
                p,
                "step-try",
                "3 · With Hex (game)",
                matches!(s.step, Step::Try | Step::TryHearing | Step::TryHeard),
                Onboard::jump_to_try,
            ),
            side_chip(
                p,
                "step-ax",
                "4 · Accessibility",
                s.step == Step::Accessibility,
                Onboard::jump_to_accessibility,
            ),
            side_chip(
                p,
                "step-ready",
                "5 · Hex is ready",
                s.step == Step::Ready,
                Onboard::jump_to_ready,
            ),
        ];
        out = out.child(side_section(p, "steps-sec", "Jump to step", step_items));
        out = out.child(motion_controls(p, s));
    }

    if matches!(s.flow, Flow::FirstRun | Flow::Versus) {
        out = out.child(scene_controls(p, s));
    }

    if matches!(s.flow, Flow::FirstRun | Flow::Versus) {
        let sound_items = KeySoundStyle::ALL
            .into_iter()
            .enumerate()
            .map(|(i, style)| {
                side_chip(
                    p,
                    &format!("snd-{i}"),
                    style.name(),
                    s.sfx.style == style,
                    move |s: &mut Onboard| s.choose_key_sound(style),
                )
            })
            .collect();
        out = out.child(side_section(
            p,
            "sounds-sec",
            "Typing key sound (click to hear)",
            sound_items,
        ));
    }

    let note = if s.interactive_try() {
        "On Your turn, hold ⌥ and speak into your Mac's mic — your running Hex pastes your real words into the card."
    } else {
        "English: Parakeet Unified English, 731 MB, streams. French: Parakeet v3, 740 MB."
    };
    out.child(
        text(note)
            .key("side-note")
            .font(p.k.ty.small)
            .color(p.muted())
            .wrap()
            .w(SIDE_W),
    )
}

fn motion_controls(p: Paint, s: &Onboard) -> El {
    use crate::rune_ui::lists::slider_in;
    let m = s.motion;
    col()
        .key("motion-controls")
        .gap(10.0)
        .child(slider_in(
            p,
            "motion-enter",
            "Rise",
            (m.enter - 0.2) / 0.4,
            format!("{:.2} s", m.enter),
            SIDE_W,
            |s: &mut Onboard, v, _| s.motion.enter = 0.2 + v * 0.4,
        ))
        .child(slider_in(
            p,
            "motion-exit",
            "Leave",
            (m.exit - 0.15) / 0.25,
            format!("{:.2} s", m.exit),
            SIDE_W,
            |s: &mut Onboard, v, _| s.motion.exit = 0.15 + v * 0.25,
        ))
        .child(slider_in(
            p,
            "motion-travel",
            "Travel",
            m.travel / 20.0,
            format!("{:.0} pt", m.travel),
            SIDE_W,
            |s: &mut Onboard, v, _| s.motion.travel = v * 20.0,
        ))
        .child(slider_in(
            p,
            "motion-stagger",
            "Stagger",
            m.stagger / 0.08,
            format!("{:.0} ms", m.stagger * 1000.0),
            SIDE_W,
            |s: &mut Onboard, v, _| s.motion.stagger = v * 0.08,
        ))
        .child(slider_in(
            p,
            "motion-keyboard",
            "Keyboard",
            (m.keyboard - 0.35) / 0.65,
            format!("{:.2} s", m.keyboard),
            SIDE_W,
            |s: &mut Onboard, v, _| s.motion.keyboard = 0.35 + v * 0.65,
        ))
}

/// One `"Without Hex"` timing: a field of [`typing::Timing`] and its range.
struct Knob {
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    field: fn(&mut typing::Timing) -> &mut f64,
}

const SCENE_KNOBS: &[Knob] = &[
    Knob {
        key: "t-arrive",
        label: "First message",
        min: 0.0,
        max: 1.5,
        field: |t| &mut t.arrive,
    },
    Knob {
        key: "t-read",
        label: "Read per word",
        min: 0.05,
        max: 0.45,
        field: |t| &mut t.read_word,
    },
    Knob {
        key: "t-dots",
        label: "Jay typing",
        min: 0.3,
        max: 2.5,
        field: |t| &mut t.jay_typing,
    },
    Knob {
        key: "t-pace",
        label: "Key pace",
        min: 0.08,
        max: 0.32,
        field: |t| &mut t.pace,
    },
    Knob {
        key: "t-panic",
        label: "Panic pace",
        min: 0.035,
        max: 0.16,
        field: |t| &mut t.panic_pace,
    },
    Knob {
        key: "t-freeze",
        label: "Freeze before Read",
        min: 0.3,
        max: 3.5,
        field: |t| &mut t.freeze,
    },
    Knob {
        key: "t-cheer",
        label: "Hex: cheers apart",
        min: 0.1,
        max: 1.0,
        field: |t| &mut t.cheer,
    },
    Knob {
        key: "t-fired",
        label: "Hold on removed",
        min: 0.8,
        max: 5.0,
        field: |t| &mut t.fired,
    },
    Knob {
        key: "t-send",
        label: "Hex: words to send",
        min: 0.3,
        max: 2.0,
        field: |t| &mut t.hex_send,
    },
    Knob {
        key: "t-reply",
        label: "Hex: send to reply",
        min: 0.5,
        max: 3.0,
        field: |t| &mut t.hex_reply,
    },
];

/// Live sliders for every pause and the key pace. A change applies from the
/// next play.
fn scene_controls(p: Paint, s: &Onboard) -> El {
    use crate::rune_ui::lists::slider_in;
    let mut out = col().key("scene-controls").gap(10.0);
    for (i, knob) in SCENE_KNOBS.iter().enumerate() {
        let mut timing = s.timing;
        let value = *(knob.field)(&mut timing);
        out = out.child(slider_in(
            p,
            knob.key,
            knob.label,
            ((value - knob.min) / (knob.max - knob.min)) as f32,
            format!("{value:.2} s"),
            SIDE_W,
            move |s: &mut Onboard, v, _| {
                let knob = &SCENE_KNOBS[i];
                *(knob.field)(&mut s.timing) = knob.min + f64::from(v) * (knob.max - knob.min);
                s.retime();
            },
        ));
    }
    side_section(p, "scene-sec", "Without Hex timing (seconds)", vec![out])
}
