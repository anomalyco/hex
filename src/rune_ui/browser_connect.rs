//! A web page's one-transcription ask: a modal naming the page's verified
//! origin, with Allow once and Deny.
//!
//! Nothing happens on the page's say-so: the local API only asks while the
//! dialog shows, and only a press answers it. Deny (or Escape, or the scrim)
//! cancels the upload; Allow issues one code the page may exchange for one
//! upload, scoped to the origin named here.

use rune::{El, col, el, row, text};

use super::Ui;
use super::controls::{ButtonKind, Paint, button, modal_scrim_transition, modal_transition};
use crate::desktop::i18n::tr;

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
pub(super) fn answer(ui: &mut Ui, allow: bool) {
    let Some(prompt) = ui.consent.take() else {
        return;
    };
    ui.consent_done = Some(prompt.id);
    ui.send(super::Request::AnswerConsent {
        id: prompt.id,
        allow,
    });
}

/// The dialog's scrim and panel, while a page waits for an answer.
pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    let prompt = ui.consent.as_ref()?;
    let c = &p.k.c;
    let mut panel = col()
        .key("browser-connect-dialog")
        .role("dialog")
        .label(tr("Allow one transcription?"))
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
            text(tr("Allow one transcription?"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(
            row()
                .key("origin")
                .gap(8.0)
                .center_y()
                .child(
                    text(tr("From"))
                        .key("from")
                        .font(super::controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(
                    text(prompt.origin.as_str())
                        .key("origin-value")
                        .font(super::controls::DESCRIPTION)
                        .color(p.text()),
                ),
        );
    // The page's own name, when it sent a short plain one, always framed as
    // its claim rather than Hex's description of it.
    if let Some(name) = &prompt.client_name {
        panel = panel.child(
            text(format!("{}{name}", tr("The page calls itself: ")))
                .key("client-name")
                .font(super::controls::DESCRIPTION)
                .color(p.muted()),
        );
    }
    let mut panel = panel
        .child(
            text(tr(
                "This page wants Hex to transcribe one audio recording. If you allow once, Hex sends it one transcript of that upload, and the permission ends.",
            ))
            .key("body")
            .font(super::controls::DESCRIPTION)
            .color(p.muted())
            .wrap(),
        )
        .child(
            text(tr(
                "Hex's microphone stays off. This page gets no recording, no dictation, no paste, and no other Hex control.",
            ))
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
                        |ui: &mut Ui, _| answer(ui, false),
                    )
                    .default_button(),
                )
                .child(button(
                    p,
                    "browser-connect-allow",
                    tr("Allow once"),
                    ButtonKind::Primary,
                    |ui: &mut Ui, _| answer(ui, true),
                )),
        );
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
            .on_press(|ui: &mut Ui| answer(ui, false))
            .transition(modal_scrim_transition(p)),
        layer("browser-connect-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape denies, and focus returns after.
            .child(panel.modal(true, |ui: &mut Ui| answer(ui, false))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rune_ui::{Request, Service};

    fn ask() -> crate::local_api::Prompt {
        crate::local_api::Prompt {
            id: 7,
            origin: "https://notes.example".into(),
            client_name: Some("Field Notes".into()),
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

        answer(&mut ui, true);
        assert!(ui.consent.is_none());
        assert!(matches!(
            sent(&ui),
            [Request::AnswerConsent { id: 7, allow: true }]
        ));
        // The host still publishes the same ask until the worker clears it;
        // it must not reopen the answered dialog.
        assert!(!poll(&mut ui));
        answer(&mut ui, false);
        assert_eq!(sent(&ui).len(), 1);
    }

    #[test]
    fn denying_sends_no_allow() {
        let mut ui = crate::rune_ui::scenes::mac_fixture();
        ui.consent = Some(ask());
        answer(&mut ui, false);
        assert!(matches!(
            sent(&ui),
            [Request::AnswerConsent {
                id: 7,
                allow: false
            }]
        ));
        assert_eq!(ui.consent_done, Some(7));
    }

    /// The host an origin names, with its port when it has one.
    fn origin_host(origin: &str) -> &str {
        origin
            .split_once("://")
            .map_or(origin, |(_, authority)| authority)
    }

    #[test]
    fn an_origin_names_its_host_and_port() {
        assert_eq!(origin_host("https://notes.example"), "notes.example");
        assert_eq!(origin_host("http://localhost:5173"), "localhost:5173");
    }
}
