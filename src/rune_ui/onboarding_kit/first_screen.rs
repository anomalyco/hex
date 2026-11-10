//! More on `"Setting up Hex"` than a title and a percentage: the details a
//! download page owes you, how much is down and that your voice stays on
//! this Mac.

use rune::{El, Icon, TextStyle, Transition, col, icon, row, text};
use rune_kit::glyphs;

use crate::desktop::i18n::{tr, tr_fill};

use super::super::controls::Paint;
use super::fixture::Download;
use super::stories::{Onboard, Step};

const WINDOW: [f32; 2] = [980.0, 760.0];
const NOTE_FROM_BOTTOM: f32 = 40.0;

/// How much of the model is down, beside its percentage: `"190 of 610 MB"`.
pub(super) fn amount(p: Paint, s: &Onboard, d: Download) -> Option<El> {
    (!d.done() && d.total > 0 && !failed(s)).then(|| {
        let mb = |bytes: u64| (bytes as f64 / 1_000_000.0).round() as u64;
        text(format!("{} of {} MB", mb(d.downloaded), mb(d.total)))
            .key("amount")
            .font(TextStyle::new(13.0, 420.0).lh(18.0).features("'tnum' 1"))
            .color(p.muted())
    })
}

/// One quiet line at the foot of the window: why there's a download, and
/// again when Hex asks for the microphone, where it matters most. From the
/// practice on, where macOS first shows the mic in use, why it stays on.
pub(super) fn note(p: Paint, s: &Onboard) -> Option<El> {
    if failed(s) {
        return None;
    }
    match s.step {
        Step::Pick | Step::Started | Step::Microphone => Some(foot(
            "first-note",
            marked(
                p,
                &glyphs::LOCK,
                "Hex hears you on this Mac. Your voice never leaves it.",
            ),
        )),
        Step::Try | Step::TryHearing | Step::TryHeard | Step::Ready
            if s.live.as_ref().is_some_and(|live| live.mic_stays_open()) =>
        {
            let why = tr(
                "macOS shows the mic as on because Hex keeps it warm, so dictation starts instantly.",
            );
            let only = tr_fill(
                if s.toggle {
                    "It only records after you tap {}."
                } else {
                    "It only records while you hold {}."
                },
                &shortcut(&s.caps),
            );
            Some(foot(
                "mic-note",
                col()
                    .items_center()
                    .child(marked(p, &glyphs::MIC, why))
                    .child(words(p, &only).key("only")),
            ))
        }
        _ => None,
    }
}

/// The dictation shortcut as text where the template's `{}` is: macOS's
/// glyphs run together as its menus write them (`⌥⇧S`), others join with
/// `+`. Keycaps here would make the second line taller than the first.
fn shortcut(caps: &[String]) -> String {
    caps.join(if cfg!(target_os = "macos") { "" } else { "+" })
}

/// Quiet notes at the foot of the window, their first line where the
/// last one's was.
fn foot(key: &str, lines: El) -> El {
    row()
        .key(key)
        .absolute(0.0, WINDOW[1] - NOTE_FROM_BOTTOM - 18.0)
        .w(WINDOW[0])
        .justify_center()
        .transition(Transition::opacity())
        .child(lines)
}

/// A line of the note led by its glyph.
fn marked(p: Paint, glyph: &'static Icon, copy: &str) -> El {
    row()
        .key("marked")
        .gap(6.0)
        .center_y()
        .child(icon(glyph, 13.0).key("glyph").color(p.muted()))
        .child(words(p, copy))
}

fn words(p: Paint, copy: &str) -> El {
    text(copy)
        .key("words")
        .font(TextStyle::new(13.0, 420.0).lh(18.0))
        .color(p.muted())
}

/// A failed download keeps its page to the error and what to do about it.
fn failed(s: &Onboard) -> bool {
    s.live.as_ref().is_some_and(|live| live.failed)
}
