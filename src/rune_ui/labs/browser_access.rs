//! Labs › Browser access to the local API's card while on: the origins whose
//! web pages may call Hex's local API from the browser, comma-separated and
//! saved in their normalized `scheme://host[:port]` form, and the clients
//! already paired to start recordings, each with a Revoke button.

use rune::{El, col, row, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{ButtonKind, DESCRIPTION, Paint, button};
use crate::desktop::i18n::tr;

/// How wide the origins field is inside its card.
const ORIGINS_WIDTH: f32 = 190.0;

pub(super) fn body(ui: &Ui, p: Paint) -> El {
    let field = TextInput::new(&ui.lab_store.origins)
        .placeholder("https://example.com, http://localhost:5173")
        .width(Some(ORIGINS_WIDTH))
        .on_edit(|ui: &mut Ui| &mut ui.lab_store.origins)
        .build(p.k)
        .key("api-browser-origins");
    let mut card = col().key("api-browser-access").gap(4.0).child(
        row()
            .key("api-browser-origins-row")
            .gap(8.0)
            .center_y()
            .child(
                text(tr("Origins"))
                    .key("label")
                    .font(DESCRIPTION)
                    .color(p.muted()),
            )
            .child(field),
    );
    let grants = ui
        .view
        .as_ref()
        .map(|view| view.grants.as_slice())
        .unwrap_or_default();
    if !grants.is_empty() {
        card = card.child(
            text(tr("Paired to start recordings"))
                .key("api-paired-label")
                .font(DESCRIPTION)
                .color(p.muted()),
        );
        for grant in grants {
            let id = grant.id;
            card = card.child(
                row()
                    .key(format!("api-paired-{id}"))
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(grant.recipient.label())
                            .key("name")
                            .font(DESCRIPTION)
                            .color(p.text()),
                    )
                    .child(button(
                        p,
                        &format!("api-revoke-{id}"),
                        tr("Revoke"),
                        ButtonKind::Secondary,
                        move |ui: &mut Ui, _| {
                            ui.send(super::super::Request::RevokeRecordGrant { id });
                        },
                    )),
            );
        }
        card = card.child(
            text(tr(
                "Revoking ends a recording it is running. It can pair again, with a new ask.",
            ))
            .key("api-paired-note")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap(),
        );
    }
    card
}
