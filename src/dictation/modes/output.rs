//! Platform-neutral finished-dictation output policy shared by macOS
//! (`parakeet`), Linux (`linux::dictation::output`), and Windows
//! (`windows::dictation::output`).
//!
//! Owns the single source of truth for:
//! - target classification (`OutputTarget`) and its exhaustive `HistoryKind` mapping,
//! - stop-phrase stripping and empty-transcript classification,
//! - Labs › Signature gating and ordering,
//! - Labs › Copy dictation only vs Labs › YOLO mode precedence (`InsertPlan`),
//! - Paste Last recovery on failure and update after insertion,
//! - dictation History recording.
//!
//! Each OS supplies only its thin insertion adapter (`OutputAdapter`).

use std::time::Instant;

use crate::dictation::paste_last::LastDictation;
use crate::history::{History, HistoryDraft, HistoryKind, HistoryProcessing, Recording};

/// Error reported when the speech model returned empty or whitespace-only text
/// without a stop phrase explaining it.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) const EMPTY_TRANSCRIPT_ERROR: &str = "transcription was empty";

/// Error reported when text replacements or other processing reduced a
/// non-empty transcript to empty text (without a voice prompt deliberately
/// asking for nothing).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) const EMPTY_PROCESSED_ERROR: &str = "text replacements produced empty output";

/// Error reported when Paste Last runs before any dictation has completed.
pub(crate) const NO_LAST_DICTATION_ERROR: &str = "there is no completed dictation to paste";

/// Where a finished dictation's text goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputTarget {
    /// Normal dictation: process, optionally sign, insert (or copy-only),
    /// update Paste Last, record History (`Dictation` or `Send`).
    Paste,
    /// A recording microphone interrupted a shortcut dictation: processed like
    /// a paste, copied to the clipboard instead of pasted, still available to
    /// Paste Last Dictation and kept in History. Never YOLO-sends.
    CopyOnlyPaste,
    /// The same interrupted recording from Dictate without processing: copied
    /// as heard, skipping the Output pane's steps.
    RawCopyOnlyPaste,
    /// Dictate without processing: pasted as heard, skipping the Output pane's
    /// steps.
    RawPaste,
    /// Programmatic `/dictations` API capture: no processing, no signature, no
    /// insertion, no Paste Last, no History.
    Service,
    /// `hex dictation stop --print`: processed as a paste, then handed to the
    /// waiting command instead of pasted. Paste Last skips it.
    Print,
    /// A printing stop of a raw dictation: the transcript as heard.
    RawPrint,
}

impl OutputTarget {
    /// Maps a listener's flavour and print flag to the canonical target.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn from_flavour(flavour: crate::dictation::DictationFlavour, printed: bool) -> Self {
        match (flavour.raw(), printed) {
            (true, true) => Self::RawPrint,
            (false, true) => Self::Print,
            (true, false) => Self::RawPaste,
            (false, false) => Self::Paste,
        }
    }

    /// Whether Output-pane mode processing (corrections, rewrite, transforms)
    /// runs on this target.
    pub const fn processes(self) -> bool {
        matches!(self, Self::Paste | Self::CopyOnlyPaste | Self::Print)
    }

    /// Whether Labs › Signature appends to this target.
    pub const fn signs(self) -> bool {
        matches!(self, Self::Paste | Self::CopyOnlyPaste)
    }

    /// Whether this target inserts or copies text for the user (and updates
    /// Paste Last).
    pub const fn inserts(self) -> bool {
        matches!(
            self,
            Self::Paste | Self::CopyOnlyPaste | Self::RawPaste | Self::RawCopyOnlyPaste
        )
    }

    /// Whether microphone loss forced this capture to copy-only.
    pub const fn is_rescued_copy_only(self) -> bool {
        matches!(self, Self::CopyOnlyPaste | Self::RawCopyOnlyPaste)
    }

    /// Whether this target belongs to `hex dictation stop --print`.
    pub const fn is_print(self) -> bool {
        matches!(self, Self::Print | Self::RawPrint)
    }

    /// Whether this target may press Return under Labs › YOLO mode.
    pub const fn may_yolo_send(self) -> bool {
        matches!(self, Self::Paste)
    }

    /// The History entry kind for this target, or `None` for API captures.
    pub const fn history_kind(self, sent: bool) -> Option<HistoryKind> {
        match self {
            Self::Paste if sent => Some(HistoryKind::Send),
            Self::Paste | Self::CopyOnlyPaste => Some(HistoryKind::Dictation),
            Self::RawPaste | Self::RawCopyOnlyPaste => Some(HistoryKind::Raw),
            Self::Print | Self::RawPrint => Some(HistoryKind::Printed),
            Self::Service => None,
        }
    }
}

/// Result of stripping any trailing stop phrase and classifying an empty
/// model transcript before processing runs.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PreparedTranscript {
    /// The capture contained only the spoken stop phrase: discard quietly with
    /// no error, no paste, and no History.
    StopPhraseOnly,
    /// The model returned empty or whitespace-only text.
    Empty,
    /// Transcript ready for optional processing (trimmed unless `Service`).
    Ready(String),
}

/// Strips any trailing stop phrase that ended the capture and classifies empty
/// or whitespace-only model output consistently across every OS.
pub(crate) fn prepare_transcript(
    raw_text: String,
    stop_phrase: Option<&str>,
    target: OutputTarget,
) -> PreparedTranscript {
    let stripped = crate::dictation::stop_phrase::strip_finished(raw_text, stop_phrase);
    if matches!(target, OutputTarget::Service) {
        return PreparedTranscript::Ready(stripped);
    }
    if stripped.trim().is_empty() {
        if stop_phrase.is_some() {
            return PreparedTranscript::StopPhraseOnly;
        }
        return PreparedTranscript::Empty;
    }
    PreparedTranscript::Ready(stripped.trim().to_string())
}

/// Appends the saved signature line when `target` allows signing and the lab
/// is active. Returns whether a signature was appended.
pub(crate) fn sign_for_target(
    target: OutputTarget,
    text: &mut String,
    signature_text: &str,
    signature_enabled: bool,
) -> bool {
    if !target.signs() {
        return false;
    }
    let Some(signed) = crate::dictation::signature::append(text, signature_text, signature_enabled)
    else {
        return false;
    };
    *text = signed;
    true
}

/// How the OS adapter should insert a non-empty finished dictation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InsertPlan {
    /// Leave the text on the clipboard without pasting: rescued captures, or
    /// Labs › Copy dictation only when YOLO mode does not send.
    CopyOnly,
    /// Paste (or live-type) the text, optionally pressing Return afterwards
    /// when `send` is true. `standalone` prevents joining the previous or next
    /// dictation (Labs › Signature).
    Paste { send: bool, standalone: bool },
}

impl InsertPlan {
    /// Applies the single cross-platform precedence for copy-only vs YOLO vs
    /// ordinary paste:
    /// - Non-inserting targets produce `None`.
    /// - Rescued captures always produce `CopyOnly` and never YOLO-send.
    /// - Otherwise, YOLO mode (`may_yolo_send && yolo_sends`) overrides
    ///   Labs › Copy dictation only, pasting and sending Return.
    /// - Otherwise, Labs › Copy dictation only produces `CopyOnly`.
    /// - Otherwise, paste (standalone when signed).
    pub(crate) fn decide(
        target: OutputTarget,
        yolo_sends: bool,
        copy_only_lab: bool,
        signed: bool,
    ) -> Option<Self> {
        if !target.inserts() {
            return None;
        }
        if target.is_rescued_copy_only() {
            return Some(Self::CopyOnly);
        }
        let send = target.may_yolo_send() && yolo_sends;
        if !send && copy_only_lab {
            return Some(Self::CopyOnly);
        }
        Some(Self::Paste {
            send,
            standalone: signed,
        })
    }
}

/// Outcome of one OS insertion attempt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct InsertResult {
    /// Whether YOLO mode actually pressed Return (false when Escape cancelled
    /// it on macOS, or when Windows fell back to copying for the shell).
    pub(crate) sent: bool,
    /// Whether the text was left on the clipboard without pasting.
    pub(crate) copied: bool,
}

/// Platform-specific hooks executed by [`deliver`].
pub(crate) trait OutputAdapter {
    /// Cancels any pending live preedit when delivery finishes without
    /// inserting (e.g. Linux live typing on empty/print/voice-prompt-nothing).
    fn cancel_pending(&mut self) {}

    /// Executes [`InsertPlan::CopyOnly`] or [`InsertPlan::Paste`].
    fn insert(&mut self, text: &str, plan: InsertPlan) -> Result<InsertResult, String>;

    /// Updates Paste Last after insertion succeeds or fails. `recorded_in_history`
    /// is true only when History persisted an entry for this dictation.
    fn remember_last(&mut self, last: LastDictation, recorded_in_history: bool);
}

/// Everything the shared delivery policy needs once transcription and optional
/// processing have completed.
pub(crate) struct DeliveryInput {
    pub(crate) target: OutputTarget,
    pub(crate) raw_text: String,
    pub(crate) text: String,
    pub(crate) nothing: bool,
    pub(crate) application: Option<String>,
    pub(crate) bundle_name: Option<String>,
    pub(crate) processing: Option<HistoryProcessing>,
    pub(crate) audio_ms: u64,
    pub(crate) inference_ms: u64,
    pub(crate) total_started: Instant,
    pub(crate) recording: Option<Recording>,
    /// Whether this capture was started by a shortcut and is therefore
    /// eligible for Labs › YOLO mode.
    pub(crate) allow_yolo: bool,
}

/// Outcome of running the shared delivery policy.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum DeliveryOutcome {
    /// A voice prompt deliberately asked for nothing: discard quietly without
    /// pasting, Paste Last, or History.
    VoicePromptNothing,
    /// Processed text was empty or whitespace-only (not a voice-prompt
    /// nothing): nothing was pasted, remembered for Paste Last, or recorded in
    /// History.
    EmptyOutput,
    /// Text was delivered according to `target`.
    Delivered {
        text: String,
        #[allow(dead_code)]
        sent: bool,
        #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
        copied: bool,
        #[allow(dead_code)]
        signed: bool,
    },
}

/// Runs the canonical finished-dictation delivery policy using live Labs
/// settings: voice-prompt-nothing check, empty-output check, signature,
/// `InsertPlan`, insertion (with Paste Last recovery on error), History
/// recording, and Paste Last update.
pub(crate) fn deliver(
    input: DeliveryInput,
    history: Option<&History>,
    adapter: &mut impl OutputAdapter,
) -> Result<DeliveryOutcome, String> {
    deliver_with_flags(
        input,
        history,
        adapter,
        &crate::labs::signature_text(),
        crate::labs::signature(),
        crate::labs::copy_only(),
        crate::dictation::yolo_mode::sends,
    )
}

/// Same as [`deliver`], with explicit feature flags for deterministic tests.
pub(crate) fn deliver_with_flags(
    mut input: DeliveryInput,
    history: Option<&History>,
    adapter: &mut impl OutputAdapter,
    signature_text: &str,
    signature_enabled: bool,
    copy_only_lab: bool,
    yolo_sends: impl FnOnce(Option<&str>, Option<&str>) -> bool,
) -> Result<DeliveryOutcome, String> {
    if input.nothing {
        adapter.cancel_pending();
        return Ok(DeliveryOutcome::VoicePromptNothing);
    }
    if !matches!(input.target, OutputTarget::Service) && input.text.trim().is_empty() {
        adapter.cancel_pending();
        return Ok(DeliveryOutcome::EmptyOutput);
    }

    let signed = sign_for_target(
        input.target,
        &mut input.text,
        signature_text,
        signature_enabled,
    );

    let sends = input.allow_yolo
        && input.target.may_yolo_send()
        && yolo_sends(input.application.as_deref(), input.bundle_name.as_deref());
    let plan = InsertPlan::decide(input.target, sends, copy_only_lab, signed);

    let inserted = match plan {
        None => {
            adapter.cancel_pending();
            InsertResult::default()
        }
        Some(plan) => match adapter.insert(&input.text, plan) {
            Ok(result) => result,
            Err(error) => {
                // A failed paste still keeps the text for Paste Last so the
                // dictation isn't lost; History keeps only pasted output.
                adapter.remember_last(LastDictation::new(input.text, signed), false);
                return Err(error);
            }
        },
    };

    let recorded = history
        .and_then(|history| record_history(history, input.target, inserted.sent, &mut input));

    if input.target.inserts() {
        adapter.remember_last(
            LastDictation::new(input.text.clone(), signed),
            recorded.is_some(),
        );
    }

    Ok(DeliveryOutcome::Delivered {
        text: input.text,
        sent: inserted.sent,
        copied: inserted.copied,
        signed,
    })
}

/// Records one successfully delivered dictation in History. History failures
/// are logged and never fail the delivery that already happened.
pub(crate) fn record_history(
    history: &History,
    target: OutputTarget,
    sent: bool,
    input: &mut DeliveryInput,
) -> Option<u64> {
    let kind = target.history_kind(sent)?;
    let draft = HistoryDraft {
        kind,
        raw_text: input.raw_text.clone(),
        final_text: input.text.clone(),
        application: input.application.clone(),
        processing: input.processing.clone(),
        audio_ms: input.audio_ms,
        inference_ms: input.inference_ms,
        total_ms: input.total_started.elapsed().as_millis() as u64,
        recording: input.recording.take(),
        model: crate::history::dictation_model(),
    };
    match history.record(draft) {
        Ok(id) => id,
        Err(error) => {
            tracing::warn!(%error, "could not record dictation history");
            None
        }
    }
}

/// Resolves the dictation Paste Last should reinsert, or the canonical error
/// message when none exists.
pub(crate) fn resolve_paste_last(
    last: Option<&LastDictation>,
    recovered: Option<LastDictation>,
) -> Result<LastDictation, &'static str> {
    last.cloned().or(recovered).ok_or(NO_LAST_DICTATION_ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::DictationFlavour;

    #[derive(Default)]
    struct MockAdapter {
        cancelled: bool,
        calls: Vec<(String, InsertPlan)>,
        remembered: Vec<(LastDictation, bool)>,
        fail_with: Option<&'static str>,
        result: InsertResult,
    }

    impl OutputAdapter for MockAdapter {
        fn cancel_pending(&mut self) {
            self.cancelled = true;
        }

        fn insert(&mut self, text: &str, plan: InsertPlan) -> Result<InsertResult, String> {
            self.calls.push((text.to_owned(), plan));
            if let Some(error) = self.fail_with {
                return Err(error.to_owned());
            }
            let sent = match plan {
                InsertPlan::Paste { send, .. } => self.result.sent || send,
                InsertPlan::CopyOnly => false,
            };
            let copied = matches!(plan, InsertPlan::CopyOnly) || self.result.copied;
            Ok(InsertResult { sent, copied })
        }

        fn remember_last(&mut self, last: LastDictation, recorded_in_history: bool) {
            self.remembered.push((last, recorded_in_history));
        }
    }

    fn input(target: OutputTarget, text: &str) -> DeliveryInput {
        DeliveryInput {
            target,
            raw_text: text.to_owned(),
            text: text.to_owned(),
            nothing: false,
            application: Some("Slack".into()),
            bundle_name: None,
            processing: None,
            audio_ms: 1_000,
            inference_ms: 50,
            total_started: Instant::now(),
            recording: None,
            allow_yolo: true,
        }
    }

    fn temp_history(tag: &str) -> (History, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("hex-output-policy-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let history = History::new(crate::history::HistoryStore::open(
            dir.join("history.json"),
            crate::history::HistoryRetention::Week,
            crate::history::now_ms(),
        ));
        (history, dir)
    }

    #[test]
    fn target_from_flavour_maps_all_four_combinations() {
        assert_eq!(
            OutputTarget::from_flavour(DictationFlavour::Normal, false),
            OutputTarget::Paste
        );
        assert_eq!(
            OutputTarget::from_flavour(DictationFlavour::Normal, true),
            OutputTarget::Print
        );
        assert_eq!(
            OutputTarget::from_flavour(DictationFlavour::Raw, false),
            OutputTarget::RawPaste
        );
        assert_eq!(
            OutputTarget::from_flavour(DictationFlavour::Raw, true),
            OutputTarget::RawPrint
        );
        assert_eq!(
            OutputTarget::from_flavour(DictationFlavour::TerminalPaste, false),
            OutputTarget::Paste
        );
    }

    #[test]
    fn history_kind_is_exhaustive_across_every_target() {
        assert_eq!(
            OutputTarget::Paste.history_kind(false),
            Some(HistoryKind::Dictation)
        );
        assert_eq!(
            OutputTarget::Paste.history_kind(true),
            Some(HistoryKind::Send)
        );
        assert_eq!(
            OutputTarget::CopyOnlyPaste.history_kind(false),
            Some(HistoryKind::Dictation)
        );
        assert_eq!(
            OutputTarget::CopyOnlyPaste.history_kind(true),
            Some(HistoryKind::Dictation)
        );
        assert_eq!(
            OutputTarget::RawPaste.history_kind(false),
            Some(HistoryKind::Raw)
        );
        assert_eq!(
            OutputTarget::RawCopyOnlyPaste.history_kind(false),
            Some(HistoryKind::Raw)
        );
        assert_eq!(
            OutputTarget::Print.history_kind(false),
            Some(HistoryKind::Printed)
        );
        assert_eq!(
            OutputTarget::RawPrint.history_kind(false),
            Some(HistoryKind::Printed)
        );
        assert_eq!(OutputTarget::Service.history_kind(false), None);
    }

    #[test]
    fn prepare_transcript_classifies_stop_phrase_and_whitespace_consistently() {
        assert_eq!(
            prepare_transcript(
                "stop dictation.".into(),
                Some("stop dictation"),
                OutputTarget::Paste
            ),
            PreparedTranscript::StopPhraseOnly
        );
        assert_eq!(
            prepare_transcript("   \n\t ".into(), None, OutputTarget::Paste),
            PreparedTranscript::Empty
        );
        assert_eq!(
            prepare_transcript("  hello world  ".into(), None, OutputTarget::Paste),
            PreparedTranscript::Ready("hello world".into())
        );
        assert_eq!(
            prepare_transcript(
                "hello world, stop dictation.".into(),
                Some("stop dictation"),
                OutputTarget::Paste
            ),
            PreparedTranscript::Ready("hello world".into())
        );
        // Service preserves raw whitespace without trimming.
        assert_eq!(
            prepare_transcript("  hi  ".into(), None, OutputTarget::Service),
            PreparedTranscript::Ready("  hi  ".into())
        );
    }

    #[test]
    fn yolo_mode_overrides_copy_only_while_rescued_captures_always_copy() {
        assert_eq!(
            InsertPlan::decide(OutputTarget::Paste, true, true, false),
            Some(InsertPlan::Paste {
                send: true,
                standalone: false
            }),
            "YOLO mode pastes and sends even while Copy dictation only is on"
        );
        assert_eq!(
            InsertPlan::decide(OutputTarget::Paste, false, true, false),
            Some(InsertPlan::CopyOnly)
        );
        assert_eq!(
            InsertPlan::decide(OutputTarget::CopyOnlyPaste, true, false, false),
            Some(InsertPlan::CopyOnly),
            "rescued captures always copy and never YOLO-send"
        );
        assert_eq!(
            InsertPlan::decide(OutputTarget::RawCopyOnlyPaste, true, false, false),
            Some(InsertPlan::CopyOnly)
        );
        assert_eq!(
            InsertPlan::decide(OutputTarget::RawPaste, true, true, false),
            Some(InsertPlan::CopyOnly),
            "raw dictations never YOLO-send, so Copy dictation only copies them"
        );
        assert_eq!(
            InsertPlan::decide(OutputTarget::Print, true, true, false),
            None
        );
    }

    #[test]
    fn empty_processed_output_cancels_pending_without_signing_or_inserting() {
        let mut adapter = MockAdapter::default();
        let outcome = deliver_with_flags(
            input(OutputTarget::Paste, "   \t "),
            None,
            &mut adapter,
            "Sent from Hex",
            true,
            false,
            |_, _| true,
        )
        .unwrap();

        assert_eq!(outcome, DeliveryOutcome::EmptyOutput);
        assert!(adapter.cancelled);
        assert!(adapter.calls.is_empty());
        assert!(adapter.remembered.is_empty());
    }

    #[test]
    fn voice_prompt_nothing_cancels_pending_and_skips_output_and_history() {
        let (history, dir) = temp_history("nothing");
        let mut req = input(OutputTarget::Paste, "");
        req.nothing = true;
        let mut adapter = MockAdapter::default();

        let outcome = deliver_with_flags(
            req,
            Some(&history),
            &mut adapter,
            "Sent from Hex",
            true,
            false,
            |_, _| true,
        )
        .unwrap();

        assert_eq!(outcome, DeliveryOutcome::VoicePromptNothing);
        assert!(adapter.cancelled);
        assert!(adapter.calls.is_empty());
        assert!(adapter.remembered.is_empty());
        assert!(history.recent(5).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signature_signs_normal_paste_once_and_skips_raw_and_print() {
        for (target, expected_text, expected_signed, expected_plan) in [
            (
                OutputTarget::Paste,
                "ship it\nSent from Hex",
                true,
                Some(InsertPlan::Paste {
                    send: false,
                    standalone: true,
                }),
            ),
            (
                OutputTarget::CopyOnlyPaste,
                "ship it\nSent from Hex",
                true,
                Some(InsertPlan::CopyOnly),
            ),
            (
                OutputTarget::RawPaste,
                "ship it",
                false,
                Some(InsertPlan::Paste {
                    send: false,
                    standalone: false,
                }),
            ),
            (OutputTarget::Print, "ship it", false, None),
        ] {
            let mut adapter = MockAdapter::default();
            let outcome = deliver_with_flags(
                input(target, "ship it"),
                None,
                &mut adapter,
                "Sent from Hex",
                true,
                false,
                |_, _| false,
            )
            .unwrap();
            assert_eq!(
                outcome,
                DeliveryOutcome::Delivered {
                    text: expected_text.to_owned(),
                    sent: false,
                    copied: matches!(expected_plan, Some(InsertPlan::CopyOnly)),
                    signed: expected_signed,
                }
            );
            match expected_plan {
                Some(plan) => {
                    assert_eq!(adapter.calls, vec![(expected_text.to_owned(), plan)]);
                    assert_eq!(
                        adapter.remembered,
                        vec![(
                            LastDictation::new(expected_text.to_owned(), expected_signed),
                            false
                        )]
                    );
                }
                None => {
                    assert!(adapter.calls.is_empty());
                    assert!(adapter.remembered.is_empty());
                }
            }
        }
    }

    #[test]
    fn failed_insert_remembers_signed_text_for_paste_last_and_skips_history() {
        let (history, dir) = temp_history("failed-insert");
        let mut adapter = MockAdapter {
            fail_with: Some("clipboard changed"),
            ..Default::default()
        };

        let error = deliver_with_flags(
            input(OutputTarget::Paste, "keep this"),
            Some(&history),
            &mut adapter,
            "Sent from Hex",
            true,
            false,
            |_, _| false,
        )
        .unwrap_err();

        assert_eq!(error, "clipboard changed");
        assert_eq!(
            adapter.remembered,
            vec![(
                LastDictation::new("keep this\nSent from Hex".into(), true),
                false
            )]
        );
        assert!(history.recent(5).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn printed_and_sent_dictations_record_expected_history_kinds() {
        let (history, dir) = temp_history("history-kinds");

        let mut adapter = MockAdapter::default();
        deliver_with_flags(
            input(OutputTarget::Paste, "sent"),
            Some(&history),
            &mut adapter,
            "",
            false,
            true, // CopyOnly enabled, but YOLO overrides it
            |_, _| true,
        )
        .unwrap();
        assert_eq!(
            adapter.calls[0].1,
            InsertPlan::Paste {
                send: true,
                standalone: false
            }
        );
        assert!(adapter.remembered[0].1);

        let mut print_adapter = MockAdapter::default();
        deliver_with_flags(
            input(OutputTarget::Print, "printed"),
            Some(&history),
            &mut print_adapter,
            "",
            false,
            false,
            |_, _| true,
        )
        .unwrap();
        assert!(print_adapter.calls.is_empty());
        assert!(print_adapter.remembered.is_empty());

        let entries = history.search("");
        assert_eq!(entries.len(), 2);
        let kinds: Vec<_> = entries.iter().map(|entry| entry.kind).collect();
        assert!(kinds.contains(&HistoryKind::Send));
        assert!(kinds.contains(&HistoryKind::Printed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_paste_last_prefers_memory_then_recovered_else_canonical_error() {
        let mem = LastDictation::new("in memory".into(), false);
        let rec = LastDictation::new("recovered".into(), true);
        assert_eq!(resolve_paste_last(Some(&mem), Some(rec.clone())), Ok(mem));
        assert_eq!(resolve_paste_last(None, Some(rec.clone())), Ok(rec));
        assert_eq!(resolve_paste_last(None, None), Err(NO_LAST_DICTATION_ERROR));
    }
}
