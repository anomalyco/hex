//! Labs › Signature's card while on: the line every normal dictation gets at
//! its end, with the reason under it while a draft doesn't pass validation.

use rune::{El, col, row, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use crate::desktop::i18n::tr;

/// How wide the signature field is inside its card.
const SIGNATURE_WIDTH: f32 = 220.0;

pub(super) fn body(ui: &Ui, p: Paint) -> El {
    let problem = crate::dictation::signature::text_problem(ui.lab_store.signature.text.trim());
    let field = TextInput::new(&ui.lab_store.signature)
        .placeholder(crate::dictation::signature::DEFAULT_TEXT)
        .width(Some(SIGNATURE_WIDTH))
        .invalid(problem.is_some())
        .on_edit(|ui: &mut Ui| &mut ui.lab_store.signature)
        .build(p.k)
        .key("signature-text");
    let mut body = col().key("signature").gap(4.0).child(
        row()
            .key("signature-text-row")
            .gap(8.0)
            .center_y()
            .child(
                text(tr("Text"))
                    .key("label")
                    .font(DESCRIPTION)
                    .color(p.muted()),
            )
            .child(field),
    );
    if let Some(problem) = problem {
        body = body.child(
            text(tr(problem))
                .key("signature-text-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap(),
        );
    }
    body
}
