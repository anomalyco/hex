//! The review sheet a words file opens: what it adds, what is already
//! there, each clash with Keep mine or Use this one, what doesn't fit here,
//! and what the file itself left out, with a switch for each part. Nothing
//! changes until Add. Each string from the file is drawn verbatim in its own
//! text run, so right-to-left text can't reorder the labels around it, and
//! shortened past 64 characters.

use rune::{El, col, row, text};

use super::{Review, add, cancel, reasons};
use crate::desktop::i18n::{tr, tr_fill};
use crate::dictation::hex_words::{Clash, Place};
use crate::rune_ui::controls::{
    ButtonKind, METRICS, MODAL, ModalWidth, Paint, button, button_enabled, card, modal_panel,
    modal_sheet, note, section, segmented, toggle_row,
};
use crate::rune_ui::{Ui, faded_body};

/// Clashes the sheet shows before the rest scroll.
const CLASHES_SHOWN: f32 = 3.5;
/// The clash list's fades, as the Vocabulary sheet's term rows have.
const FADES: (f32, f32) = (0.0, 6.0);
/// The most of a file's string the sheet draws before "…".
const SHOWN_CHARS: usize = 64;

/// A string from the file as the sheet draws it: its first 64 characters,
/// line breaks and tabs made visible. None when it is empty.
fn shown(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let mut shown: String = text
        .chars()
        .take(SHOWN_CHARS)
        .map(|character| match character {
            '\n' => '↵',
            '\t' => '⇥',
            other => other,
        })
        .collect();
    if text.chars().nth(SHOWN_CHARS).is_some() {
        shown.push('…');
    }
    Some(shown)
}

/// One string from the file, in its own run.
fn verbatim(p: Paint, key: &str, value: &str) -> El {
    match shown(value) {
        Some(value) => text(value).key(key).font(p.description()).color(p.text()),
        None => text(tr("(nothing)"))
            .key(key)
            .font(p.description())
            .color(p.faint()),
    }
}

fn choose(ui: &mut Ui, index: usize, theirs: bool) {
    if let Some(review) = ui.vocabulary.words.review.as_mut() {
        if theirs {
            review.theirs.insert(index);
        } else {
            review.theirs.remove(&index);
        }
    }
}

fn flip(ui: &mut Ui, vocabulary: bool) {
    if let Some(review) = ui.vocabulary.words.review.as_mut() {
        let part = if vocabulary {
            &mut review.parts.vocabulary
        } else {
            &mut review.parts.replacements
        };
        *part = !*part;
    }
}

fn over_limit(review: &Review, kind: fn(&Place) -> bool) -> usize {
    review
        .preview
        .over_limit
        .iter()
        .filter(|place| kind(place))
        .count()
}

/// "3 new · 1 with new spellings · 2 already there".
fn vocabulary_text(review: &Review) -> String {
    if review.file.vocabulary.is_empty() {
        return tr("None in this file").into();
    }
    let summary = review.preview.summary;
    let over = over_limit(review, |place| matches!(place, Place::Term(_)));
    let mut parts = Vec::new();
    if summary.added_terms > 0 {
        parts.push(tr_fill("{} new", &summary.added_terms.to_string()));
    }
    if summary.updated_terms > 0 {
        parts.push(tr_fill(
            "{} with new spellings",
            &summary.updated_terms.to_string(),
        ));
    }
    if summary.already_have_terms > 0 {
        parts.push(tr_fill(
            "{} already there",
            &summary.already_have_terms.to_string(),
        ));
    }
    if over > 0 {
        parts.push(tr_fill("{} didn't fit", &over.to_string()));
    }
    parts.join(" · ")
}

/// "2 new · 1 already there · 1 clash".
fn replacements_text(review: &Review) -> String {
    if review.file.replacements.is_empty() {
        return tr("None in this file").into();
    }
    let summary = review.preview.summary;
    let clashes = review.preview.clashes.len();
    let over = over_limit(review, |place| matches!(place, Place::Replacement(_)));
    let mut parts = Vec::new();
    if summary.added_replacements > 0 {
        parts.push(tr_fill("{} new", &summary.added_replacements.to_string()));
    }
    if summary.already_have_replacements > 0 {
        parts.push(tr_fill(
            "{} already there",
            &summary.already_have_replacements.to_string(),
        ));
    }
    match clashes {
        0 => {}
        1 => parts.push(tr("1 clash").into()),
        count => parts.push(tr_fill("{} clashes", &count.to_string())),
    }
    if over > 0 {
        parts.push(tr_fill("{} didn't fit", &over.to_string()));
    }
    parts.join(" · ")
}

/// One clash: the heard words, yours and the file's, and the choice.
fn clash_row(p: Paint, clash: &Clash, theirs: bool) -> El {
    let index = clash.index;
    let side = |key: &str, label: &str, value: &str| {
        row()
            .key(key)
            .gap(4.0)
            .child(
                text(label)
                    .key("label")
                    .font(p.description())
                    .color(p.muted()),
            )
            .child(verbatim(p, "value", value))
    };
    row()
        .key(format!("clash{index}"))
        .px(METRICS.row_px)
        .min_h(METRICS.item_h)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .key("words")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(shown(&clash.heard).unwrap_or_default())
                        .key("heard")
                        .font(p.strong())
                        .color(p.text()),
                )
                .child(
                    row()
                        .key("sides")
                        .gap(12.0)
                        .child(side("yours", tr("Yours"), &clash.yours))
                        .child(side("theirs", tr("This file"), &clash.theirs)),
                ),
        )
        .child(segmented(
            p,
            "choice",
            &[tr("Keep mine"), tr("Use this one")],
            usize::from(theirs),
            move |ui: &mut Ui, pick| choose(ui, index, pick == 1),
        ))
}

fn clashes(p: Paint, review: &Review) -> El {
    let rows: Vec<El> = review
        .preview
        .clashes
        .iter()
        .map(|clash| clash_row(p, clash, review.theirs.contains(&clash.index)))
        .collect();
    let count = rows.len() as f32;
    if count <= CLASHES_SHOWN {
        return section(p, "clashes", tr("Clashes"), rows);
    }
    col()
        .key("clashes")
        .gap(METRICS.label_gap)
        .child(p.section_label(tr("Clashes")).mx(METRICS.row_px))
        .child(
            col()
                .key("clash-column")
                .h(CLASHES_SHOWN * METRICS.item_h + FADES.1)
                .child(faded_body(
                    p.k,
                    "clash-scroll",
                    card(p, "card", rows),
                    FADES,
                )),
        )
}

/// "Made by Hex for iPhone 1.0.3": the file's own claim, never trusted, and
/// whether a newer Hex wrote it.
fn heading(p: Paint, review: &Review) -> Option<El> {
    let made_by = &review.file.made_by;
    let mut heading = col()
        .key("heading")
        .px(METRICS.row_px)
        .mt(4.0 - MODAL.gap)
        .gap(4.0);
    let mut empty = true;
    if made_by.app.is_some() || made_by.version.is_some() {
        let mut claim = row().key("made-by").gap(4.0).child(
            text(tr("Made by"))
                .key("label")
                .font(p.description())
                .color(p.muted()),
        );
        for (key, value) in [("app", &made_by.app), ("version", &made_by.version)] {
            if let Some(value) = value {
                claim = claim.child(verbatim(p, key, value));
            }
        }
        heading = heading.child(claim);
        empty = false;
    }
    if review.file.newer() {
        heading = heading.child(
            text(tr(
                "Made by a newer Hex: what this Hex doesn't know is left out",
            ))
            .key("newer")
            .font(p.description())
            .color(p.muted())
            .wrap(),
        );
        empty = false;
    }
    (!empty).then_some(heading)
}

pub(in crate::rune_ui) fn modal(ui: &Ui, p: Paint) -> Vec<El> {
    let Some(review) = &ui.vocabulary.words.review else {
        return Vec::new();
    };
    let mut body: Vec<El> = heading(p, review).into_iter().collect();
    let (vocabulary, replacements) = (review.parts.vocabulary, review.parts.replacements);
    body.push(section(
        p,
        "parts",
        tr("In this file"),
        vec![
            toggle_row(
                p,
                "vocabulary-part",
                tr("Vocabulary"),
                vocabulary_text(review).as_str(),
                vocabulary,
                |ui: &mut Ui| flip(ui, true),
            ),
            toggle_row(
                p,
                "replacements-part",
                tr("Replacements"),
                replacements_text(review).as_str(),
                replacements,
                |ui: &mut Ui| flip(ui, false),
            ),
        ],
    ));
    if !review.preview.clashes.is_empty() {
        body.push(clashes(p, review));
    }
    let limits = [
        (
            over_limit(review, |place| matches!(place, Place::Term(_))),
            tr("Vocabulary holds up to 500 terms: {} left out"),
        ),
        (
            over_limit(review, |place| matches!(place, Place::Spelling(..))),
            tr("A term holds up to 16 spellings: {} left out"),
        ),
        (
            over_limit(review, |place| matches!(place, Place::Replacement(_))),
            tr("Replacements hold up to 1,000 when you import: {} left out"),
        ),
    ];
    for (index, (count, line)) in limits.into_iter().enumerate() {
        if count > 0 {
            let line = line.replace("{}", &count.to_string());
            body.push(note(&format!("limit{index}"), &line, p.muted()));
        }
    }
    if !review.file.skipped.is_empty() {
        body.push(note(
            "left-out",
            &tr_fill("Left out of the file: {}", &reasons(&review.file.skipped)),
            p.muted(),
        ));
    }
    let can_add = (vocabulary && !review.file.vocabulary.is_empty())
        || (replacements && !review.file.replacements.is_empty());
    let footer = vec![
        button(
            p,
            "cancel-words",
            tr("Cancel"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| cancel(ui),
        ),
        button_enabled(
            p,
            "add-words",
            tr("Add"),
            ButtonKind::Primary,
            can_add,
            |ui: &mut Ui, _| add(ui),
        )
        .default_button(),
    ];
    let panel = modal_panel(
        p,
        "words-review",
        tr("Add words from a file"),
        ModalWidth::Wide,
        body,
        footer,
    );
    modal_sheet(p, "words-review", panel, cancel).into()
}
