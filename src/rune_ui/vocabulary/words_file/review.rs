//! The review sheet a words file opens: which Hex the file says made it,
//! then, under a switch for each part, every term and replacement Add would
//! add, each clash with Keep mine or Use this one, what doesn't fit here, and
//! what the file itself left out. Nothing changes until Add. Each string from
//! the file is drawn verbatim in its own text run, so right-to-left text
//! can't reorder the labels around it, and shortened past 64 characters.

use rune::{El, TextStyle, col, row, text};

use super::{Review, add, cancel, reasons};
use crate::desktop::i18n::{tr, tr_fill};
use crate::dictation::hex_words::{Clash, Place};
use crate::rune_ui::controls::{
    ButtonKind, METRICS, MODAL, ModalWidth, Paint, button, button_enabled, card, modal_panel,
    modal_sheet, note, section, segmented, toggle_row,
};
use crate::rune_ui::{Ui, faded_body};

/// The tallest the parts and clashes grow before they scroll.
const REVIEW_HEIGHT: f32 = 440.0;
/// Space between the part cards and the clashes.
const LIST_GAP: f32 = 16.0;
/// A section label's line above its card.
const LABEL_LINE: f32 = 16.0;
/// Room above and below a part's list of items, inside its card.
const ITEMS_PY: f32 = 4.0;
/// The scrolling list's fades, which also keep the cards' shadows clear.
const FADES: (f32, f32) = (4.0, 8.0);
/// Room beside the cards inside the list's clip, for their shadows.
const SIDE_ROOM: f32 = 4.0;
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

/// One string from the file, in its own run; an empty one reads "(nothing)".
fn verbatim(p: Paint, key: &str, value: &str, font: TextStyle, on: bool) -> El {
    match shown(value) {
        Some(value) => text(value)
            .key(key)
            .font(font)
            .color(if on { p.text() } else { p.faint() }),
        None => text(tr("(nothing)")).key(key).font(font).color(p.faint()),
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

/// A term Add would add, or one it gives new spellings: the term, then
/// `label` and each spelling in its own run.
fn term_item(p: Paint, key: String, term: &str, label: &str, spellings: &[String], on: bool) -> El {
    let mut item = row()
        .key(key)
        .px(METRICS.row_px)
        .min_h(METRICS.row_h_compact)
        .gap(8.0)
        .center_y()
        .child(verbatim(p, "term", term, p.row_title(), on));
    if !spellings.is_empty() {
        let mut list = row()
            .key("spellings")
            .grow(1.0)
            .shrink()
            .gap(4.0)
            .flex_wrap()
            .child(
                text(label)
                    .key("label")
                    .font(p.description())
                    .color(p.muted()),
            );
        for (index, spelling) in spellings.iter().enumerate() {
            if index > 0 {
                list = list.child(
                    text("·")
                        .key(format!("dot{index}"))
                        .font(p.description())
                        .color(p.muted()),
                );
            }
            list = list.child(verbatim(
                p,
                &format!("spelling{index}"),
                spelling,
                p.description(),
                on,
            ));
        }
        item = item.child(list);
    }
    item
}

/// A replacement Add would add: the heard words, an arrow, what Hex writes.
fn replacement_item(p: Paint, index: usize, heard: &str, write: &str, on: bool) -> El {
    row()
        .key(format!("replacement{index}"))
        .px(METRICS.row_px)
        .min_h(METRICS.row_h_compact)
        .gap(8.0)
        .center_y()
        .child(verbatim(p, "heard", heard, p.row_title(), on))
        .child(text("→").key("arrow").font(p.row_title()).color(p.muted()))
        .child(verbatim(p, "write", write, p.row_title(), on))
}

/// The Vocabulary part: its switch, then each new term with its spellings
/// and each listed term with the spellings it gains.
fn vocabulary_part(p: Paint, review: &Review) -> El {
    let on = review.parts.vocabulary;
    let mut rows = vec![toggle_row(
        p,
        "vocabulary-part",
        tr("Vocabulary"),
        vocabulary_text(review).as_str(),
        on,
        |ui: &mut Ui| flip(ui, true),
    )];
    let terms = &review.file.vocabulary;
    let mut items: Vec<El> = Vec::new();
    for &index in &review.preview.new_terms {
        if let Some(term) = terms.get(index) {
            let label = tr("sounds like");
            items.push(term_item(
                p,
                format!("term{index}"),
                &term.term,
                label,
                &term.sounds_like,
                on,
            ));
        }
    }
    for (index, spellings) in &review.preview.new_spellings {
        if let Some(term) = terms.get(*index) {
            let label = tr("also sounds like");
            items.push(term_item(
                p,
                format!("term{index}"),
                &term.term,
                label,
                spellings,
                on,
            ));
        }
    }
    if !items.is_empty() {
        rows.push(col().key("new-terms").py(ITEMS_PY).children(items));
    }
    card(p, "vocabulary", rows)
}

/// The Replacements part: its switch, then each new replacement.
fn replacements_part(p: Paint, review: &Review) -> El {
    let on = review.parts.replacements;
    let mut rows = vec![toggle_row(
        p,
        "replacements-part",
        tr("Replacements"),
        replacements_text(review).as_str(),
        on,
        |ui: &mut Ui| flip(ui, false),
    )];
    let items: Vec<El> = review
        .preview
        .new_replacements
        .iter()
        .filter_map(|&index| {
            let rule = review.file.replacements.get(index)?;
            Some(replacement_item(p, index, &rule.heard, &rule.write, on))
        })
        .collect();
    if !items.is_empty() {
        rows.push(col().key("new-replacements").py(ITEMS_PY).children(items));
    }
    card(p, "replacements", rows)
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
            .child(verbatim(p, "value", value, p.description(), true))
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
                .child(verbatim(p, "heard", &clash.heard, p.strong(), true))
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

/// "Made by Hex for iPhone 1.0.3, as the file says": the file's own claim,
/// never trusted, its words in their own runs; and whether a newer Hex
/// wrote it.
fn heading(p: Paint, review: &Review) -> Option<El> {
    let made_by = &review.file.made_by;
    let mut heading = col()
        .key("heading")
        .px(METRICS.row_px)
        .mt(4.0 - MODAL.gap)
        .gap(4.0);
    let mut empty = true;
    let claim: Vec<&String> = [&made_by.app, &made_by.version]
        .into_iter()
        .flatten()
        .collect();
    if !claim.is_empty() {
        let template = tr("Made by {}, as the file says");
        let (before, after) = template.split_once("{}").unwrap_or((template, ""));
        let muted = |key: &str, words: &str| {
            text(words.trim())
                .key(key)
                .font(p.description())
                .color(p.muted())
        };
        let mut line = row().key("made-by").center_y();
        if !before.trim().is_empty() {
            line = line.child(muted("before", before));
        }
        for (index, value) in claim.into_iter().enumerate() {
            let run = verbatim(p, &format!("claim{index}"), value, p.description(), true);
            line = line.child(if index > 0 || !before.trim().is_empty() {
                run.ml(4.0)
            } else {
                run
            });
        }
        if !after.trim().is_empty() {
            let run = muted("after", after);
            line = line.child(if after.starts_with(char::is_whitespace) {
                run.ml(4.0)
            } else {
                run
            });
        }
        heading = heading.child(line);
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

/// About how tall the parts and clashes stand: a described row per part, a
/// compact row per item, a list row per clash, with the gaps and hairlines.
/// Past [`REVIEW_HEIGHT`] they scroll in that height instead.
fn natural_height(review: &Review) -> f32 {
    let preview = &review.preview;
    let part = |items: usize| {
        METRICS.row_h_described
            + if items > 0 {
                1.0 + 2.0 * ITEMS_PY + items as f32 * METRICS.row_h_compact
            } else {
                0.0
            }
    };
    let mut height = part(preview.new_terms.len() + preview.new_spellings.len())
        + LIST_GAP
        + part(preview.new_replacements.len());
    if !preview.clashes.is_empty() {
        height += LIST_GAP
            + LABEL_LINE
            + METRICS.label_gap
            + preview.clashes.len() as f32 * (METRICS.item_h + 1.0);
    }
    height
}

pub(in crate::rune_ui) fn modal(ui: &Ui, p: Paint) -> Vec<El> {
    let Some(review) = &ui.vocabulary.words.review else {
        return Vec::new();
    };
    let mut body: Vec<El> = heading(p, review).into_iter().collect();
    let mut list = col()
        .key("review")
        .gap(LIST_GAP)
        .child(vocabulary_part(p, review))
        .child(replacements_part(p, review));
    if !review.preview.clashes.is_empty() {
        let rows: Vec<El> = review
            .preview
            .clashes
            .iter()
            .map(|clash| clash_row(p, clash, review.theirs.contains(&clash.index)))
            .collect();
        list = list.child(section(p, "clashes", tr("Clashes"), rows));
    }
    body.push(if natural_height(review) <= REVIEW_HEIGHT {
        list
    } else {
        col()
            .key("review-column")
            .mx(-SIDE_ROOM)
            .h(REVIEW_HEIGHT)
            .child(faded_body(p.k, "review-scroll", list.px(SIDE_ROOM), FADES))
    });
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
    let (vocabulary, replacements) = (review.parts.vocabulary, review.parts.replacements);
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
