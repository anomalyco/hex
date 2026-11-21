//! Labs › Learn from corrections: after a dictation is pasted, one watch
//! reads that field back while the person fixes it, and
//! [`learned_terms`](crate::dictation::correction_terms::learned_terms)
//! decides which words the fix taught.
//!
//! This module owns when the watch reads, when it stops, and how what it
//! learns reaches Vocabulary. It never logs a field's text or a learned
//! term: its one event says how many terms were learned. Nothing leaves the
//! Mac.

use crate::dictation::vocabulary::{self, VocabularyEntry};

/// How often the watched field is read.
#[cfg(target_os = "macos")]
const POLL: std::time::Duration = std::time::Duration::from_millis(100);
/// How long the app may take to show a paste Hex already posted: ⌘V is
/// handled after Hex's paste returns, so the first read is retried this long.
#[cfg(target_os = "macos")]
const LANDING: std::time::Duration = std::time::Duration::from_secs(1);
/// How long one watch follows its field. A correction usually lands within
/// seconds; five minutes keeps a forgotten field from being watched longer.
#[cfg(target_os = "macos")]
const WATCH: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Why a tick ended the watch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stop {
    /// Labs › Learn from corrections turned off: learn nothing.
    Off,
    /// A newer paste started, so only text read before it is compared.
    Superseded,
    /// The five minutes passed; the last text read is compared.
    Timeout,
    /// The focused window or field changed.
    Focus,
    /// The field is gone or blank.
    Empty,
}

impl Stop {
    /// Whether the field's final text still teaches its correction. Only the
    /// lab turning off discards it.
    fn learns(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Why a tick ends the watch before its field is read, checked in this
/// order: the lab (off learns nothing, so it wins every later reason), a
/// newer paste, then the deadline.
fn ending(current: bool, gate: bool, expired: bool) -> Option<Stop> {
    if !gate {
        return Some(Stop::Off);
    }
    if !current {
        return Some(Stop::Superseded);
    }
    expired.then_some(Stop::Timeout)
}

/// What one read of the watched field means. A paste generation that moved
/// during the read discards it: the field may already hold that paste's text,
/// so a superseded watch learns only from readings taken before the newer
/// paste. `None` or a blank field ends the watch.
fn reading(current: bool, text: Option<String>) -> Result<String, Stop> {
    if !current {
        return Err(Stop::Superseded);
    }
    match text {
        Some(text) if !text.trim().is_empty() => Ok(text),
        _ => Err(Stop::Empty),
    }
}

/// Whether a finished output leaves a field to watch: it pasted into one
/// (`pasted`), never copied instead, and never sent it away.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn watches(pasted: bool, copied: bool, sent: bool) -> bool {
    pasted && !copied && !sent
}

/// `entries` with `terms` added as plain terms, through
/// [`vocabulary::add`], so `hex vocabulary add`'s rules apply: a term
/// already listed in any case changes nothing. Returns the list and the terms
/// actually added.
fn with_terms(
    entries: &[VocabularyEntry],
    terms: &[String],
) -> (Vec<VocabularyEntry>, Vec<String>) {
    let mut list = entries.to_vec();
    let mut added = Vec::new();
    for term in terms {
        if matches!(vocabulary::add(&mut list, term, &[]), Ok(true)) {
            added.push(term.clone());
        }
    }
    (list, added)
}

/// Starts a watch when a finished output really pasted `inserted` into a
/// field: not a copy, not a YOLO send, and only while Labs › Learn from
/// corrections is on. Runs on the ordered output worker, so the field is the
/// one the paste just reached; the watch thread only reads it.
#[cfg(target_os = "macos")]
pub(crate) fn watch_paste(
    inserted: &str,
    target: crate::parakeet::TranscriptionTarget,
    mode: crate::paste::PasteMode,
    sent: bool,
) {
    let pasted = matches!(
        target,
        crate::parakeet::TranscriptionTarget::Paste
            | crate::parakeet::TranscriptionTarget::RawPaste
    );
    if !watches(
        pasted,
        matches!(mode, crate::paste::PasteMode::CopyOnly),
        sent,
    ) || !crate::labs::learn_from_corrections()
    {
        return;
    }
    // Before the field is captured: a newer paste landing between the two
    // must look newer to this watch, never become the paste it believes it
    // belongs to.
    let generation = crate::paste::paste_generation();
    let Some(focus) = crate::dictation::context::frontmost_pid()
        .and_then(crate::accessibility::PasteFocus::capture)
    else {
        return;
    };
    watch(focus, inserted.to_owned(), generation);
}

/// Starts the one watch that follows the field a dictation pasted `inserted`
/// into. Its focus is captured before this thread exists, and a later paste
/// supersedes it through the paste generation.
#[cfg(target_os = "macos")]
fn watch(focus: crate::accessibility::PasteFocus, inserted: String, generation: u64) {
    let inserted = inserted.trim().to_owned();
    if inserted.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("correction-watch".into())
        .spawn(move || follow(focus, inserted, generation));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the correction watch");
    }
}

/// Reads the field back every [`POLL`] until it ends the watch, then
/// compares the last text read with the baseline it started from.
#[cfg(target_os = "macos")]
fn follow(focus: crate::accessibility::PasteFocus, inserted: String, generation: u64) {
    let Some(baseline) = landed(&focus, &inserted, generation) else {
        return;
    };
    let started = std::time::Instant::now();
    let mut latest = baseline.clone();
    loop {
        std::thread::sleep(POLL);
        match tick(&focus, generation, started) {
            Ok(text) => latest = text,
            Err(stop) => {
                let learned = finish(stop, &baseline, &inserted, &latest);
                tracing::info!(terms = learned, "correction watch ended");
                return;
            }
        }
    }
}

/// The field's text once the app has taken the paste: the app handles ⌘V
/// after Hex's paste returns, so the first read is retried every [`POLL`]
/// for up to [`LANDING`] until the field holds what was inserted. `None`
/// when the lab turns off, a newer paste starts, the focus moves, the read
/// outlives its generation, or the paste never lands: a paste no watch can
/// see teaches nothing.
#[cfg(target_os = "macos")]
fn landed(
    focus: &crate::accessibility::PasteFocus,
    inserted: &str,
    generation: u64,
) -> Option<String> {
    let deadline = std::time::Instant::now() + LANDING;
    loop {
        if !crate::labs::learn_from_corrections() || crate::paste::paste_generation() != generation
        {
            return None;
        }
        if !focus.unchanged() {
            return None;
        }
        let text = focus.field_text();
        if crate::paste::paste_generation() != generation {
            return None;
        }
        if let Some(text) = text
            && text.contains(inserted)
        {
            return Some(text);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

/// One tick's reading of the watched field: the text to remember, or the
/// reason the watch ended. The cheap checks run first, so a tick that stops
/// for them never reads the field.
#[cfg(target_os = "macos")]
fn tick(
    focus: &crate::accessibility::PasteFocus,
    generation: u64,
    started: std::time::Instant,
) -> Result<String, Stop> {
    if let Some(stop) = ending(
        crate::paste::paste_generation() == generation,
        crate::labs::learn_from_corrections(),
        started.elapsed() >= WATCH,
    ) {
        return Err(stop);
    }
    if !focus.unchanged() {
        return Err(Stop::Focus);
    }
    let text = focus.field_text();
    reading(crate::paste::paste_generation() == generation, text)
}

/// Learns from the field's final text and saves the terms. Returns how many
/// terms were added.
#[cfg(target_os = "macos")]
fn finish(stop: Stop, baseline: &str, inserted: &str, latest: &str) -> usize {
    if !stop.learns() {
        return 0;
    }
    let terms = crate::dictation::correction_terms::learned_terms(
        baseline,
        inserted,
        latest,
        &vocabulary::entries(),
    );
    if terms.is_empty() {
        return 0;
    }
    save(&terms)
}

/// Saves `terms` the way `hex vocabulary add` does: read the saved
/// settings, add each new term through [`vocabulary::add`], and write the
/// list in one update, which also applies it to this process. The open
/// window adopts the save through the runtime's own settings reload. Returns
/// how many terms were added.
#[cfg(target_os = "macos")]
fn save(terms: &[String]) -> usize {
    let mut settings = match crate::app_settings::AppSettings::load() {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(%error, "could not read settings to learn vocabulary");
            return 0;
        }
    };
    let base = settings.clone();
    let (entries, added) = with_terms(&settings.vocabulary, terms);
    if added.is_empty() {
        return 0;
    }
    settings.vocabulary = entries;
    if let Err(error) = base.update(&settings) {
        tracing::warn!(%error, "could not save the learned vocabulary");
        return 0;
    }
    if let Some(runtime) = crate::macos_rune::runtime() {
        let _ = runtime.send(crate::macos_rune::Request::ReloadSettings);
    }
    added.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(term: &str, sounds_like: &[&str]) -> VocabularyEntry {
        VocabularyEntry {
            term: term.into(),
            sounds_like: sounds_like.iter().map(|&text| text.into()).collect(),
        }
    }

    #[test]
    fn a_tick_stops_in_the_order_the_watch_reads() {
        // A lab turned off learns nothing even on a superseded, expired tick.
        assert_eq!(ending(false, false, true), Some(Stop::Off));
        // A newer paste wins over the deadline.
        assert_eq!(ending(false, true, true), Some(Stop::Superseded));
        assert_eq!(ending(true, true, true), Some(Stop::Timeout));
        assert_eq!(ending(true, true, false), None);
    }

    #[test]
    fn a_reading_a_newer_paste_overtook_is_discarded() {
        // The generation is checked again after the field read: a paste that
        // landed during the read means the text may be its own, not ours.
        assert_eq!(
            reading(false, Some("hi Raunak".to_owned())),
            Err(Stop::Superseded)
        );
        assert_eq!(
            reading(true, Some("hi Raunak".to_owned())),
            Ok("hi Raunak".to_owned())
        );
        assert_eq!(reading(true, None), Err(Stop::Empty));
        assert_eq!(reading(true, Some(" \n\t".to_owned())), Err(Stop::Empty));
    }

    #[test]
    fn only_the_lab_turning_off_discards_a_correction() {
        assert!(!Stop::Off.learns());
        for stop in [Stop::Superseded, Stop::Timeout, Stop::Focus, Stop::Empty] {
            assert!(stop.learns());
        }
    }

    #[test]
    fn only_a_paste_the_person_can_still_fix_is_watched() {
        assert!(watches(true, false, false));
        assert!(
            !watches(false, false, false),
            "Service and Print never paste"
        );
        assert!(!watches(true, true, false), "a copy leaves no field");
        assert!(!watches(true, false, true), "a send took the field away");
    }

    #[test]
    fn learned_terms_join_the_list_the_way_hex_vocabulary_add_does() {
        let list = [entry("OpenCode", &["open code"])];
        let (list, added) = with_terms(
            &list,
            &[
                "Raunak".to_owned(),
                "opencode".to_owned(),
                "raunak".to_owned(),
                "Raunak".to_owned(),
            ],
        );
        assert_eq!(added, ["Raunak"]);
        assert_eq!(
            list,
            [entry("OpenCode", &["open code"]), entry("Raunak", &[])]
        );
        // `add` matches terms, as `hex vocabulary add` does: a spelling of
        // another term is a term of its own. `learned_terms` already drops
        // one, so a watch never offers it.
        let (extended, added) = with_terms(&list, &["open code".to_owned()]);
        assert_eq!(added, ["open code"]);
        assert_eq!(extended.len(), list.len() + 1);
    }

    #[test]
    fn a_full_list_keeps_its_terms_and_takes_no_more() {
        let full: Vec<_> = (0..vocabulary::MAX_ENTRIES)
            .map(|index| entry(&format!("Term {index}"), &[]))
            .collect();
        let (list, added) = with_terms(&full, &["Late term".to_owned()]);
        assert!(added.is_empty());
        assert_eq!(list, full);
    }
}
