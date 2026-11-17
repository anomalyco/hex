//! The live consented-recording card: who the recording is for, how long its
//! lease has left, and Stop or Cancel.
//!
//! It is not a modal: the person can keep using Hex while a client records.
//! Closing it is not a dismissal, because the card has no close; only Stop
//! (keep the transcript for the client) and Cancel (discard it) end the
//! recording, and the client can do either through its session token too.

use rune::{El, col, row, text};

use super::Ui;
use super::controls::{ButtonKind, Paint, button};
use crate::desktop::i18n::{tr, tr_fill};
use crate::local_api::RecordPhase;

const WIDTH: f32 = 380.0;

/// Brings the window forward when a client's recording starts (macOS), so
/// Stop and Cancel are reachable; returns whether anything changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    let record = ui
        .view
        .as_ref()
        .and_then(|view| view.record.as_ref())
        .map(|view| view.id);
    if record == ui.record_seen {
        return false;
    }
    ui.record_seen = record;
    if record.is_some() && ui.live() {
        #[cfg(target_os = "macos")]
        crate::macos_rune::bring_window_forward();
    }
    true
}

/// The card while a consented recording is live, if one is.
pub(super) fn panel(ui: &Ui, p: Paint) -> Option<El> {
    let view = ui.view.as_ref()?.record.as_ref()?;
    let recording = match view.phase {
        RecordPhase::Recording => true,
        RecordPhase::Stopped => false,
        _ => return None,
    };
    let recipient = view.recipient.label();
    let seconds = view.remaining_ms.div_ceil(1000);
    let clock = format!("{}:{:02}", seconds / 60, seconds % 60);
    let title = if recording {
        tr_fill("Recording for {}", &recipient)
    } else {
        tr_fill("Recording stopped for {}", &recipient)
    };
    let note = if recording {
        tr_fill("Stops itself in {} · raw audio stays in Hex", &clock)
    } else {
        tr_fill("Waiting {} s for the app to collect the transcript", &clock)
    };
    let mut footer = row().key("record-actions").gap(8.0).justify_end();
    if recording {
        footer = footer.child(button(
            p,
            "record-stop",
            tr("Stop recording"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| {
                if let Some(view) = ui.view.as_ref().and_then(|view| view.record.as_ref()) {
                    let id = view.id;
                    ui.send(super::Request::RecordControl { id, stop: true });
                }
            },
        ));
    }
    footer = footer.child(button(
        p,
        "record-cancel",
        tr("Cancel"),
        ButtonKind::Danger,
        |ui: &mut Ui, _| {
            if let Some(view) = ui.view.as_ref().and_then(|view| view.record.as_ref()) {
                let id = view.id;
                ui.send(super::Request::RecordControl { id, stop: false });
            }
        },
    ));
    let mut card = col()
        .key("record-panel")
        .role("status")
        .label(title.clone())
        .w(WIDTH)
        .px(14.0)
        .py(12.0)
        .gap(8.0)
        .radius(p.k.radius.lg)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_strong)
        // Presses on the card stay out of the page beneath it.
        .on_press(|_: &mut Ui| {})
        .child(
            text(title)
                .key("record-title")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap(),
        )
        .child(
            text(note)
                .key("record-note")
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .wrap(),
        )
        .child(footer);
    for shadow in &p.k.elevation.overlay {
        card = card.box_shadow(*shadow);
    }
    Some(
        col()
            .key("record-layer")
            .role("none")
            .absolute(0.0, 104.0)
            .w_full()
            .px(20.0)
            .items_end()
            .child(card),
    )
}
