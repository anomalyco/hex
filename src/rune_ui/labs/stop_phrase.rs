//! Labs › Stop phrase's card while on: the words that finish a hands-free
//! dictation, with the reason under them while a draft doesn't pass
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
        crate::dictation::voice_prompt::phrase_problem(ui.lab_store.stop_phrase.text.trim());
    let field = TextInput::new(&ui.lab_store.stop_phrase)
        .placeholder(crate::dictation::stop_phrase::DEFAULT_PHRASE)
        .width(Some(PHRASE_WIDTH))
        .invalid(problem.is_some())
        .on_edit(|ui: &mut Ui| &mut ui.lab_store.stop_phrase)
        .build(p.k)
        .key("stop-phrase-text");
    let mut body = col().key("stop-phrase").gap(4.0).child(
        row()
            .key("stop-phrase-text-row")
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
                .key("stop-phrase-text-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap(),
        );
    }
    body
}
