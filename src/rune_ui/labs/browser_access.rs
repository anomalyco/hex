//! Labs › Browser access' card while on: the origins a web page may call the
//! local API from, typed as a comma- or new-line-separated list, with the
//! list Hex saved shown back normalized under the field.

use rune::{El, col, row, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use crate::desktop::i18n::tr;

/// How wide the origins field is inside its card.
const ORIGINS_WIDTH: f32 = 220.0;

pub(super) fn body(ui: &Ui, p: Paint) -> El {
    let saved = &ui.settings.labs.api_browser_origins;
    let field = TextInput::new(&ui.lab_store.origins)
        .placeholder("http://localhost:3000, https://app.example.com")
        .width(Some(ORIGINS_WIDTH))
        .on_edit(|ui: &mut Ui| &mut ui.lab_store.origins)
        .build(p.k)
        .key("browser-access-origins");
    let mut body = col()
        .key("browser-access")
        .gap(4.0)
        .child(
            row()
                .key("browser-access-origins-row")
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(
                    text(tr("Origins"))
                        .key("label")
                        .font(DESCRIPTION)
                        .color(p.muted()),
                )
                .child(field),
        )
        .child(
            text(tr(
                "Separate origins with commas or new lines; only http and https work",
            ))
            .key("browser-access-note")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap(),
        );
    if !saved.is_empty() {
        body = body.child(
            text(saved.join(", "))
                .key("browser-access-saved")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap(),
        );
    }
    body
}
