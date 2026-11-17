//! Consent for audio transcription or microphone recording, scoped to the named recipient.

use rune::{El, col, el, row, text};

use super::Ui;
use super::controls::{ButtonKind, Paint, button, modal_scrim_transition, modal_transition};
use crate::desktop::i18n::{tr, tr_fill};
use crate::local_api::Answer;

/// The dialog's width and padding.
const WIDTH: f32 = 440.0;
const PADDING: f32 = 18.0;

/// Adopts the page's ask from the host's published state, and brings the
/// window forward so the question is actually seen. True when what shows
/// changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    let shown = ui
        .view
        .as_ref()
        .and_then(|view| view.consent.clone())
        .filter(|prompt| Some(prompt.id) != ui.consent_done);
    if shown == ui.consent {
        return false;
    }
    let opened = ui.consent.is_none() && shown.is_some();
    ui.consent = shown;
    if opened && ui.live() {
        // A page may ask while the window is closed; the person answers in
        // the window, so it opens on the ask.
        #[cfg(target_os = "macos")]
        crate::macos_rune::bring_window_forward();
    }
    true
}

/// Answers the ask with `allow`, and hides the dialog at once. A late or
/// duplicate press sends nothing, so one dialog is one answer.
pub(super) fn answer(ui: &mut Ui, answer: Answer) {
    let Some(prompt) = ui.consent.take() else {
        return;
    };
    ui.consent_done = Some(prompt.id);
    ui.send(super::Request::AnswerConsent {
        id: prompt.id,
        answer,
    });
}

/// The dialog's scrim and panel, while a caller waits for an answer.
pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    let prompt = ui.consent.as_ref()?;
    let c = &p.k.c;
    let record = prompt.intent == crate::local_api::Intent::Record;
    let title = if record {
        tr("Allow microphone recording?")
    } else {
        tr("Allow this page to transcribe audio?")
    };
    let mut panel = col()
        .key("browser-connect-dialog")
        .role("dialog")
        .label(title)
        .w(WIDTH)
        .pt(PADDING)
        .px(PADDING)
        .pb(14.0)
        .gap(12.0)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(
            text(title)
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(
            row()
                .key("recipient")
                .gap(8.0)
                .center_y()
                .child(
                    text(tr("For"))
                        .key("for")
                        .font(super::controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(
                    text(prompt.recipient.label())
                        .key("recipient-value")
                        .font(super::controls::DESCRIPTION)
                        .color(p.text()),
                ),
        );
    if let Some(note) = prompt.recipient.signing_note() {
        panel = panel
            .child(
                text(note)
                    .key("signing-note")
                    .font(super::controls::DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
            )
            .child(
                text(tr(
                    "Hex read this from the process's code signature, not its name.",
                ))
                .key("signing-verified")
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap(),
            );
    }
    // The page's own name, when it sent a short plain one, always framed as
    // its claim rather than Hex's description of it.
    if let Some(name) = prompt.recipient.claimed_name() {
        panel = panel.child(
            text(format!("{}{name}", tr("The page calls itself: ")))
                .key("client-name")
                .font(super::controls::DESCRIPTION)
                .color(p.muted()),
        );
    }
    let (body, limits) = if record {
        let seconds = crate::local_api::record_max_recording().as_secs();
        let seconds = seconds.to_string();
        (
            tr(
                "Allow Once lets this client start one microphone recording. Always Allow lets it start future recordings without asking. Only the transcript goes back.",
            ),
            tr_fill(
                "Nothing records until the client asks. Recordings stop after {} seconds. Stop or cancel in Hex; revoke access in Labs › Browser access.",
                &seconds,
            ),
        )
    } else {
        (
            tr(
                "Allow Once transcribes one audio file and returns its text to this page. Always Allow lets this page send more files without asking.",
            ),
            tr("Your microphone stays off. You can revoke access in Labs › Browser access.")
                .to_owned(),
        )
    };
    let panel = panel
        .child(
            text(body)
                .key("body")
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
        .child(
            text(limits)
                .key("limits")
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
        .child(
            row()
                .key("footer")
                .gap(8.0)
                .justify_end()
                .child(
                    button(
                        p,
                        "browser-connect-deny",
                        tr("Deny"),
                        ButtonKind::Secondary,
                        |ui: &mut Ui, _| answer(ui, Answer::Deny),
                    )
                    .default_button(),
                )
                .child(button(
                    p,
                    "browser-connect-allow",
                    tr("Allow Once"),
                    ButtonKind::Primary,
                    |ui: &mut Ui, _| answer(ui, Answer::Once),
                ))
                .child(button(
                    p,
                    "browser-connect-always",
                    tr("Always Allow"),
                    ButtonKind::Primary,
                    |ui: &mut Ui, _| answer(ui, Answer::Always),
                )),
        );
    let mut panel = panel;
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let layer = |key: &str| {
        el().key(key)
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
    };
    Some([
        layer("browser-connect-scrim")
            .fill(c.scrim)
            .on_press(|ui: &mut Ui| answer(ui, Answer::Deny))
            .transition(modal_scrim_transition(p)),
        layer("browser-connect-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape denies, and focus returns after.
            .child(panel.modal(true, |ui: &mut Ui| answer(ui, Answer::Deny))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_api::{Intent, Recipient};
    use crate::rune_ui::{Request, Service};

    fn ask() -> crate::local_api::Prompt {
        crate::local_api::Prompt {
            id: 7,
            intent: Intent::Transcribe,
            recipient: Recipient::Web {
                origin: "https://notes.example".into(),
                client_name: Some("Field Notes".into()),
            },
        }
    }

    fn record_ask() -> crate::local_api::Prompt {
        crate::local_api::Prompt {
            id: 9,
            intent: Intent::Record,
            recipient: Recipient::Native {
                identifier: "com.example.recorder".into(),
                cdhash: "fixture-digest".into(),
                team: Some("ABCDE12345".into()),
            },
        }
    }

    fn sent(ui: &Ui) -> &[Request] {
        match &ui.service {
            Service::Fixture(sent) => sent,
            _ => panic!("fixtures record requests"),
        }
    }

    #[test]
    fn a_pending_ask_is_adopted_once_and_answered_once() {
        let mut ui = crate::rune_ui::scenes::mac_fixture();
        ui.view.as_mut().unwrap().consent = Some(ask());
        assert!(poll(&mut ui));
        assert_eq!(ui.consent.as_ref().map(|prompt| prompt.id), Some(7));

        answer(&mut ui, Answer::Once);
        assert!(ui.consent.is_none());
        assert!(matches!(
            sent(&ui),
            [Request::AnswerConsent {
                id: 7,
                answer: Answer::Once
            }]
        ));
        // The host still publishes the same ask until the worker clears it;
        // it must not reopen the answered dialog.
        assert!(!poll(&mut ui));
        answer(&mut ui, Answer::Deny);
        assert_eq!(sent(&ui).len(), 1);
    }

    #[test]
    fn both_dialogs_send_the_duration_of_the_button_pressed() {
        use crate::rune_ui::{offscreen, view};
        for prompt in [ask(), record_ask()] {
            for (key, label, expected) in [
                ("browser-connect-deny", "Deny", Answer::Deny),
                ("browser-connect-allow", "Allow Once", Answer::Once),
                ("browser-connect-always", "Always Allow", Answer::Always),
            ] {
                let mut ui = crate::rune_ui::scenes::mac_fixture();
                ui.consent = Some(prompt.clone());
                let mut rt = offscreen::runtime(
                    ui,
                    view,
                    (980.0, 760.0),
                    rune::Env {
                        system_dark: true,
                        scale: 2.0,
                    },
                    rune::text::TextSystem::new(
                        rune_theme_opencode::FONTS,
                        rune_theme_opencode::FAMILIES,
                    ),
                );
                rt.settle_all();
                rt.frame(0.0);
                let copy: Vec<_> = rt.elements().into_iter().filter_map(|e| e.text).collect();
                assert!(copy.iter().any(|s| s == label));
                assert!(!copy.iter().any(|s| s.to_lowercase().contains("upload")));
                let (x, y) = rt.find(key).unwrap();
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
                rt.frame(0.0);
                assert!(
                    matches!(sent(&rt.state), [Request::AnswerConsent {answer, ..}] if *answer == expected)
                );
            }
        }
    }

    #[test]
    fn denying_sends_no_allow() {
        let mut ui = crate::rune_ui::scenes::mac_fixture();
        ui.consent = Some(ask());
        answer(&mut ui, Answer::Deny);
        assert!(matches!(
            sent(&ui),
            [Request::AnswerConsent {
                id: 7,
                answer: Answer::Deny
            }]
        ));
        assert_eq!(ui.consent_done, Some(7));
    }

    #[test]
    fn a_recording_ask_names_the_verified_native_identity() {
        let mut ui = crate::rune_ui::scenes::mac_fixture();
        ui.view.as_mut().unwrap().consent = Some(record_ask());
        assert!(poll(&mut ui));
        let prompt = ui.consent.as_ref().unwrap();
        assert_eq!(prompt.intent, Intent::Record);
        assert_eq!(prompt.recipient.label(), "com.example.recorder");
        assert_eq!(
            prompt.recipient.signing_note().as_deref(),
            Some("Signed by team ABCDE12345")
        );
        assert!(prompt.recipient.claimed_name().is_none());
        answer(&mut ui, Answer::Once);
        assert!(matches!(
            sent(&ui),
            [Request::AnswerConsent {
                id: 9,
                answer: Answer::Once
            }]
        ));
    }
}
