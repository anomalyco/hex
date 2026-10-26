//! The Vocabulary card under Dictation (the `vocabulary` experiment): one
//! row per term with its "sounds like" spellings, edited in place and saved
//! when typing pauses, like a mode's corrections.

use rune::{El, TextEdit, edit_access, row};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, TextInput};

use super::Ui;
use super::controls::{InstantHover, Paint, note, section, setting_row};
use super::lists::{remove_button, small_button};
use crate::desktop_i18n::tr;
use crate::vocabulary::{self, VocabularyEntry};

static PLUS: &rune::Icon = &rune_theme_opencode::icons::PLUS;
const TERM_WIDTH: f32 = 200.0;

/// The term and "sounds like" fields of each row.
#[derive(Default)]
pub(super) struct VocabularyEditor {
    rows: Vec<(TextEdit, TextEdit)>,
}

impl VocabularyEditor {
    pub(super) fn new(entries: &[VocabularyEntry]) -> Self {
        Self {
            rows: entries
                .iter()
                .map(|entry| {
                    (
                        TextEdit::new(entry.term.clone()),
                        TextEdit::new(entry.sounds_like.join(", ")),
                    )
                })
                .collect(),
        }
    }

    /// The rows as saved entries: blank terms are left out and repeated
    /// terms merge, as `hex vocabulary add` does.
    pub(super) fn project(&self) -> Vec<VocabularyEntry> {
        self.projection().0
    }

    /// Whether saving cuts or leaves out text that doesn't fit the limits.
    fn clamped(&self) -> bool {
        self.projection().1
    }

    fn projection(&self) -> (Vec<VocabularyEntry>, bool) {
        let mut entries = Vec::new();
        let mut clamped = false;
        for (term, sounds_like) in &self.rows {
            clamped |= vocabulary::add_clamped(
                &mut entries,
                &term.text,
                &vocabulary::parse_sounds_like(&sounds_like.text),
            );
        }
        (entries, clamped)
    }
}

fn add_row(ui: &mut Ui) {
    let mut term = TextEdit::default();
    term.focused = true;
    ui.vocabulary.rows.push((term, TextEdit::default()));
}

fn remove_row(ui: &mut Ui, index: usize) {
    if index < ui.vocabulary.rows.len() {
        ui.vocabulary.rows.remove(index);
        ui.flush_drafts();
    }
}

fn add_suggested(ui: &mut Ui) {
    let mut entries = ui.vocabulary.project();
    if vocabulary::add_suggested(&mut entries) > 0
        && ui.update(|settings| settings.vocabulary = entries)
    {
        ui.vocabulary = VocabularyEditor::new(&ui.settings.vocabulary);
    }
}

pub(super) fn section_view(ui: &Ui, p: Paint) -> El {
    let entries = ui.vocabulary.project();
    let add = IconButton::new(PLUS)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Regular)
        .label(tr("Add term"))
        .on_press(add_row)
        .build(p.k)
        .instant_hover()
        .key("add-term");
    let actions = row()
        .key("actions")
        .gap(4.0)
        .center_y()
        .children((!vocabulary::has_every_suggestion(&entries)).then(|| {
            small_button(p, "add-suggested", tr("Add suggested"), |ui: &mut Ui, _| {
                add_suggested(ui)
            })
        }))
        .child(add);
    let mut rows = vec![setting_row(
        p,
        "terms",
        tr("Names and terms"),
        Some(tr(
            "Rewriting spells them right, and search finds them when misheard",
        )),
        actions,
    )];
    if ui.vocabulary.rows.is_empty() {
        rows.push(note("no-terms", tr("No terms yet."), p.muted()));
    }
    for (index, (term, sounds_like)) in ui.vocabulary.rows.iter().enumerate() {
        rows.push(
            row()
                .key(format!("term{index}"))
                .px(12.0)
                .py(6.0)
                .gap(8.0)
                .center_y()
                .child(
                    TextInput::new(term)
                        .placeholder(tr("Term (e.g. abattoir)"))
                        .width(Some(TERM_WIDTH))
                        .on_edit(edit_access(move |ui: &mut Ui| {
                            &mut ui.vocabulary.rows[index].0
                        }))
                        .build(p.k)
                        .instant_hover()
                        .key("term"),
                )
                .child(
                    TextInput::new(sounds_like)
                        .placeholder(tr("Sounds like (e.g. avatar, a bat war)"))
                        .width(None)
                        .on_edit(edit_access(move |ui: &mut Ui| {
                            &mut ui.vocabulary.rows[index].1
                        }))
                        .build(p.k)
                        .instant_hover()
                        .key("sounds-like")
                        .grow(1.0)
                        .shrink(),
                )
                .child(remove_button(
                    p,
                    ControlSize::Regular,
                    move |ui: &mut Ui| remove_row(ui, index),
                )),
        );
    }
    if ui.vocabulary.clamped() {
        rows.push(note(
            "limits",
            tr("Up to 16 spellings per term are saved, and 64 characters per term or spelling."),
            p.danger(),
        ));
    }
    section(p, "vocabulary", tr("Vocabulary"), rows)
}

/// A few terms, one with spellings, and the suggestions not yet added.
pub(super) fn fixture(ui: &mut Ui, filled: bool) {
    ui.platform.vocabulary = true;
    ui.settings.vocabulary = if filled {
        let mut entries = Vec::new();
        for (term, sounds_like) in [
            ("abattoir", "avatar, a bat war"),
            ("OpenCode", "open code"),
            ("Rune", "roon"),
            ("Parakeet", ""),
            ("Dax", "Dex, Docs"),
        ] {
            let _ = vocabulary::add(
                &mut entries,
                term,
                &vocabulary::parse_sounds_like(sounds_like),
            );
        }
        entries
    } else {
        Vec::new()
    };
    ui.vocabulary = VocabularyEditor::new(&ui.settings.vocabulary);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_project_to_cleaned_merged_entries() {
        let mut editor = VocabularyEditor::new(&[VocabularyEntry {
            term: "abattoir".into(),
            sounds_like: vec!["avatar".into(), "a bat war".into()],
        }]);
        assert_eq!(editor.rows[0].1.text, "avatar, a bat war");
        editor
            .rows
            .push((TextEdit::default(), TextEdit::new("ignored")));
        editor
            .rows
            .push((TextEdit::new(" Abattoir "), TextEdit::new("abba toy,")));
        editor
            .rows
            .push((TextEdit::new("Rune"), TextEdit::new(" roon ")));
        assert_eq!(
            editor.project(),
            [
                VocabularyEntry {
                    term: "abattoir".into(),
                    sounds_like: vec!["avatar".into(), "a bat war".into(), "abba toy".into()],
                },
                VocabularyEntry {
                    term: "Rune".into(),
                    sounds_like: vec!["roon".into()],
                },
            ]
        );
        assert!(!editor.clamped());
    }

    #[test]
    fn rows_past_the_limits_keep_their_term_and_what_fits() {
        let spellings: Vec<String> = (0..17).map(|index| format!("spelling {index}")).collect();
        let mut editor = VocabularyEditor::new(&[]);
        editor.rows.push((
            TextEdit::new("abattoir"),
            TextEdit::new(format!("{}, {}", spellings.join(", "), "y".repeat(65))),
        ));
        editor
            .rows
            .push((TextEdit::new("Rune"), TextEdit::new("roon")));
        let entries = editor.project();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].term, "abattoir");
        assert_eq!(entries[0].sounds_like, spellings[..16]);
        assert_eq!(entries[1].sounds_like, ["roon"]);
        assert!(editor.clamped());
    }
}
