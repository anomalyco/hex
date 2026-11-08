//! More on `"Setting up Hex"` than a title and a percentage: the details a
//! download page owes you, how much is down and that your voice stays on
//! this Mac.

use rune::{El, TextStyle, Transition, icon, row, text};
use rune_kit::glyphs;

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
/// again when Hex asks for the microphone, where it matters most.
pub(super) fn note(p: Paint, s: &Onboard) -> Option<El> {
    (matches!(s.step, Step::Pick | Step::Started | Step::Microphone) && !failed(s)).then(|| {
        let quiet = p.muted();
        row()
            .key("first-note")
            .absolute(0.0, WINDOW[1] - NOTE_FROM_BOTTOM - 18.0)
            .w(WINDOW[0])
            .gap(6.0)
            .center()
            .transition(Transition::opacity())
            .child(icon(&glyphs::LOCK, 13.0).key("lock").color(quiet))
            .child(
                text("Hex hears you on this Mac. Your voice never leaves it.")
                    .key("words")
                    .font(TextStyle::new(13.0, 420.0).lh(18.0))
                    .color(quiet),
            )
    })
}

/// A failed download keeps its page to the error and what to do about it.
fn failed(s: &Onboard) -> bool {
    s.live.as_ref().is_some_and(|live| live.failed)
}
