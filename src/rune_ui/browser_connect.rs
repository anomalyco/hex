//! Consent for audio transcription or microphone recording, scoped to the named recipient.
//!
//! The dialog reads as the question a person has to answer, in order: what
//! Hex is being asked for, who is asking and how Hex knows, what they get,
//! then the three answers. Allow Once is the one accent; Always Allow sits
//! apart as a Secondary button, so lasting access is never the easiest press,
//! and Deny is the Return default (Escape and the scrim deny too).

use rune::{El, Icon, TextStyle, col, el, row, spacer, text};
use rune_theme_opencode::icons::{CIRCLE_CHECK, MONITOR, OUTLINE_TEXT, SHIELD};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, modal_scrim_transition, modal_transition,
};
use super::icons::{CLOCK, GLOBE, MIC, WAVE, tile};
use crate::desktop::i18n::{tr, tr_fill};
use crate::local_api::{Answer, Intent, Recipient};

/// The dialog's width and padding.
const WIDTH: f32 = 440.0;
const PADDING: f32 = 20.0;
/// The question: one step above the kit's body type, as a dialog's title.
const TITLE: TextStyle = TextStyle::new(17.0, 560.0).lh(22.0).tracking(-0.2);

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

/// What one kind of ask shows: its capability, question, and what the
/// recipient gets.
struct Ask {
    glyph: &'static Icon,
    capability: &'static str,
    title: &'static str,
    lead: &'static str,
    gets: Vec<(&'static Icon, String)>,
}

fn ask_copy(intent: Intent, recipient: &Recipient) -> Ask {
    let page = matches!(recipient, Recipient::Web { .. });
    if intent == Intent::Record {
        Ask {
            glyph: &MIC,
            capability: tr("Microphone"),
            title: tr("Allow microphone recording?"),
            lead: if page {
                tr(
                    "This page wants Hex to record from your microphone. Nothing records until it asks.",
                )
            } else {
                tr(
                    "This app wants Hex to record from your microphone. Nothing records until it asks.",
                )
            },
            gets: vec![
                (
                    &OUTLINE_TEXT,
                    tr("Only the transcript, never the audio").to_owned(),
                ),
                (
                    &CLOCK,
                    if page {
                        tr("Recording stops when the page ends it or stops responding, or anytime in Hex")
                    } else {
                        tr("Recording stops when the app ends it or stops responding, or anytime in Hex")
                    }
                    .to_owned(),
                ),
            ],
        }
    } else {
        Ask {
            glyph: &WAVE,
            capability: tr("Transcription"),
            title: if page {
                tr("Allow this page to transcribe audio?")
            } else {
                tr("Allow this app to transcribe audio?")
            },
            lead: if page {
                tr("This page wants to send Hex audio it already has and get the text back.")
            } else {
                tr("This app wants to send Hex audio it already has and get the text back.")
            },
            gets: vec![
                (
                    &OUTLINE_TEXT,
                    tr("The text of each audio file it sends").to_owned(),
                ),
                (&MIC, tr("Your microphone stays off").to_owned()),
            ],
        }
    }
}

/// Who is asking: the identity Hex checked, foremost, then how it knows,
/// then (for a page) the name the page gave itself, framed as a claim.
fn requester(p: Paint, recipient: &Recipient) -> El {
    let c = &p.k.c;
    let (glyph, kind, proof, proven): (&'static Icon, _, _, _) = match recipient {
        Recipient::Native { team, .. } => (
            &MONITOR,
            tr("App"),
            match team {
                Some(team) => tr_fill("Code signature verified · team {}", team),
                None => tr("Code signature verified · no team").to_owned(),
            },
            true,
        ),
        Recipient::Web { .. } => (
            &GLOBE,
            tr("Website"),
            tr("On your list in Labs › Browser access").to_owned(),
            false,
        ),
    };
    let identity = recipient.label();
    let mut said = format!("{kind}: {identity}. {proof}.");
    let mut copy = col()
        .key("requester-copy")
        .grow(1.0)
        .shrink()
        .gap(2.0)
        .child(
            text(kind)
                .key("requester-kind")
                .font(p.k.ty.caption())
                .color(p.muted())
                .a11y_hidden(),
        )
        .child(
            text(identity)
                .key("recipient-value")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap()
                .a11y_hidden(),
        )
        .child(
            row()
                .key("proof")
                .gap(5.0)
                .pt(2.0)
                .center_y()
                .child(
                    rune::icon(if proven { &CIRCLE_CHECK } else { &SHIELD }, 14.0)
                        .key("proof-icon")
                        .color(if proven { c.success.icon } else { c.icon_muted })
                        .a11y_hidden(),
                )
                .child(
                    text(proof)
                        .key("signing-note")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap()
                        .shrink()
                        .a11y_hidden(),
                ),
        );
    if let Some(name) = recipient.claimed_name() {
        let claim = tr_fill("Calls itself “{}” · not verified", name);
        said.push(' ');
        said.push_str(&claim);
        said.push('.');
        copy = copy.child(
            text(claim)
                .key("client-name")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap()
                .a11y_hidden(),
        );
    }
    row()
        .key("recipient")
        .role("group")
        .label(tr("Requested by"))
        .description(said)
        .items_start()
        .gap(12.0)
        .px(12.0)
        .py(12.0)
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .border(p.hair, c.border_weak)
        .child(tile(p, glyph, 32.0))
        .child(copy)
}

/// What the recipient gets if allowed, one line each.
fn gets(p: Paint, lines: Vec<(&'static Icon, String)>) -> El {
    let label = tr("If you allow");
    let mut list = col().key("gets").role("list").label(label).gap(8.0).child(
        text(label)
            .key("gets-label")
            .font(p.k.ty.small_medium)
            .color(p.muted())
            .a11y_hidden(),
    );
    for (index, (glyph, line)) in lines.into_iter().enumerate() {
        list = list.child(
            row()
                .key(format!("get{index}"))
                .role("listitem")
                .label(line.clone())
                .gap(10.0)
                .center_y()
                .child(
                    rune::icon(glyph, 16.0)
                        .key("icon")
                        .color(p.k.c.icon_muted)
                        .a11y_hidden(),
                )
                .child(
                    text(line)
                        .key("copy")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .wrap()
                        .shrink()
                        .a11y_hidden(),
                ),
        );
    }
    list
}

/// The ask's capability, as a mark and a name, under Hex's own label so
/// the dialog never passes for one of the system's.
fn capability(p: Paint, ask: &Ask) -> El {
    let c = &p.k.c;
    row()
        .key("capability")
        .gap(10.0)
        .center_y()
        .child(
            el().key("capability-mark")
                .size(36.0, 36.0)
                .center()
                .radius(p.k.radius.lg)
                .fill(c.bg_contrast)
                .a11y_hidden()
                .child(
                    rune::icon(ask.glyph, 18.0)
                        .key("glyph")
                        .color(c.icon_contrast),
                ),
        )
        .child(
            col()
                .key("capability-copy")
                .child(
                    text(tr("Hex permission request"))
                        .key("eyebrow")
                        .font(p.k.ty.caption())
                        .color(p.muted()),
                )
                .child(
                    text(ask.capability)
                        .key("capability-name")
                        .font(p.k.ty.ui_medium)
                        .color(p.text()),
                ),
        )
}

/// The three answers. Allow Once is the accent and Deny the Return default;
/// Always Allow stands apart on the leading edge, with what it means under it.
fn footer(p: Paint) -> El {
    col()
        .key("footer-area")
        .px(PADDING)
        .pt(14.0)
        .pb(14.0)
        .gap(10.0)
        .fill(p.k.c.layer_01)
        .child(
            row()
                .key("footer")
                .gap(8.0)
                .center_y()
                .child(button(
                    p,
                    "browser-connect-always",
                    tr("Always Allow"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| answer(ui, Answer::Always),
                ))
                .child(spacer())
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
                )),
        )
        .child(
            text(tr(
                "Always Allow lasts until you revoke it in Labs › Browser access.",
            ))
            .key("limits")
            .font(p.k.ty.caption())
            .color(p.faint())
            .wrap(),
        )
}

/// The dialog's scrim and panel, while a caller waits for an answer.
pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    let prompt = ui.consent.as_ref()?;
    let c = &p.k.c;
    let ask = ask_copy(prompt.intent, &prompt.recipient);
    let question = col()
        .key("question")
        .gap(4.0)
        .child(
            text(ask.title)
                .key("title")
                .role("heading")
                .level(1)
                .font(TITLE)
                .color(p.text())
                .wrap(),
        )
        .child(
            text(ask.lead)
                .key("body")
                .font(p.k.ty.ui)
                .color(p.muted())
                .wrap(),
        );
    let content = col()
        .key("content")
        .pt(PADDING)
        .px(PADDING)
        .pb(18.0)
        .gap(16.0)
        .child(capability(p, &ask))
        .child(question)
        .child(requester(p, &prompt.recipient))
        .child(gets(p, ask.gets));
    let mut panel = col()
        .key("browser-connect-dialog")
        .role("dialog")
        .label(ask.title)
        .description(ask.lead)
        .w(WIDTH)
        .radius(p.k.radius.xl)
        .clip_rounded(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(content)
        .child(el().key("rule").h(p.hair).fill(c.border_weak).a11y_hidden())
        .child(footer(p));
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

    /// The window with `prompt`'s dialog open, rendered dark.
    fn shown(prompt: crate::local_api::Prompt) -> rune::Runtime<Ui> {
        let mut ui = crate::rune_ui::scenes::mac_fixture();
        ui.consent = Some(prompt);
        let mut rt = crate::rune_ui::offscreen::runtime(
            ui,
            crate::rune_ui::view,
            (980.0, 760.0),
            rune::Env {
                system_dark: true,
                scale: 2.0,
            },
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        rt.settle_all();
        rt.frame(0.0);
        rt
    }

    fn copy(rt: &rune::Runtime<Ui>) -> Vec<String> {
        rt.elements().into_iter().filter_map(|e| e.text).collect()
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
        for prompt in [ask(), record_ask()] {
            for (key, label, expected) in [
                ("browser-connect-deny", "Deny", Answer::Deny),
                ("browser-connect-allow", "Allow Once", Answer::Once),
                ("browser-connect-always", "Always Allow", Answer::Always),
            ] {
                let mut rt = shown(prompt.clone());
                let copy = copy(&rt);
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

    /// Return presses the default, Deny, and Escape denies too: neither key
    /// can grant anything.
    #[test]
    fn return_and_escape_deny() {
        for key in [rune::Key::Enter, rune::Key::Escape] {
            let mut rt = shown(record_ask());
            rt.key(rune::KeyEvent {
                key,
                mods: rune::Mods::default(),
            });
            rt.frame(0.0);
            assert!(matches!(
                sent(&rt.state),
                [Request::AnswerConsent {
                    id: 9,
                    answer: Answer::Deny
                }]
            ));
        }
    }

    /// Allow Once is the dialog's one accent, and Always Allow stands apart
    /// from it, on the leading edge.
    #[test]
    fn lasting_access_is_not_the_promoted_answer() {
        let rt = shown(ask());
        let (always, _) = rt.find("browser-connect-always").unwrap();
        let (deny, _) = rt.find("browser-connect-deny").unwrap();
        let (once, _) = rt.find("browser-connect-allow").unwrap();
        assert!(always < deny && deny < once);
    }

    #[test]
    fn a_page_is_named_by_its_origin_and_its_own_name_is_only_a_claim() {
        let rt = shown(ask());
        let copy = copy(&rt);
        assert!(copy.iter().any(|s| s == "https://notes.example"));
        assert!(
            copy.iter()
                .any(|s| s == "Calls itself “Field Notes” · not verified")
        );
        assert!(
            !copy
                .iter()
                .any(|s| s.contains("verified") && s.contains("Code signature"))
        );
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
        assert!(prompt.recipient.claimed_name().is_none());
        let copy = copy(&shown(prompt.clone()));
        assert!(copy.iter().any(|s| s == "com.example.recorder"));
        assert!(
            copy.iter()
                .any(|s| s == "Code signature verified · team ABCDE12345")
        );
        assert!(copy.iter().any(|s| s == "This app wants Hex to record from your microphone. Nothing records until it asks."));
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
