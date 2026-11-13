//! Labs › Browser access to the local API's card while on: the origins whose
//! web pages may call Hex's local API from the browser, comma-separated and
//! saved in their normalized `scheme://host[:port]` form.

use rune::{El, col, row, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
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
    col().key("api-browser-access").gap(4.0).child(
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
    )
}
