//! Labs › Clipboard phrase's card while on: the words replaced with the
//! clipboard, with the reason under them while a draft doesn't pass
//! validation.

use rune::{El, col, row, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use crate::desktop::i18n::tr;

/// How wide the phrase field is inside its card.
const PHRASE_WIDTH: f32 = 150.0;

pub(super) fn body(ui: &Ui, p: Paint) -> El {
    let problem =
        crate::dictation::voice_prompt::phrase_problem(ui.lab_store.clipboard_phrase.text.trim());
    let field = TextInput::new(&ui.lab_store.clipboard_phrase)
        .placeholder(crate::dictation::clipboard_phrase::DEFAULT_PHRASE)
        .width(Some(PHRASE_WIDTH))
        .invalid(problem.is_some())
        .on_edit(|ui: &mut Ui| &mut ui.lab_store.clipboard_phrase)
        .build(p.k)
        .key("clipboard-phrase-text");
    let mut body = col().key("clipboard-phrase").gap(4.0).child(
        row()
            .key("clipboard-phrase-text-row")
            .gap(8.0)
            .center_y()
            .child(
                text(tr("Phrase"))
                    .key("label")
                    .font(DESCRIPTION)
                    .color(p.muted()),
            )
            .child(field),
    );
    if let Some(problem) = problem {
        body = body.child(
            text(tr(problem.message()))
                .key("clipboard-phrase-text-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap(),
        );
    }
    body
}
