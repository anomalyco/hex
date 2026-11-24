//! Shortcut and control transitions for dictation. Capture boundaries belong to the audio owner.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_hotkey_action(
    action: HotkeyAction,
    raw: bool,
    print: Option<PrintReply>,
    action_at: CaptureInstant,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    context: &mut ContextWatch,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    release: &mut Release,
) -> Result<bool> {
    match action {
        HotkeyAction::Start => {
            if !dictation.start(action_at)? {
                return Ok(false);
            }
            // This capture remembers the press it started from. The latch
            // clears once the stream is back, but this capture still owes
            // the tone its cold press deferred.
            release.begin();
            // Labs › Return to app: this paste goes back to the app in front
            // now, even if another one comes forward while the person talks.
            worker.remember_return_to_app();
            // Labs › YOLO mode: a shortcut started this capture
            // unless a `hex dictation` command says otherwise below.
            worker.set_started_by_shortcut(true);
            // The press started a capture, but it may still be provisional:
            // no page is read until the capture is held past its threshold.
            context.begin();
            // The tone confirms the press at once; a chord or short tap
            // retracts it. A released microphone confirms only once it opens.
            if release.press {
                feedback::prepare();
            } else {
                feedback::play_provisional_start();
            }
            worker.prepare_paste();
            #[cfg(hex_capture)]
            crate::screenshots::narration::started();
            events.dictation(DictationPhase::Started, "")?;
            indicator.send(DictationIndicatorEvent::Started);
            emit_state(events, true, device)
        }
        HotkeyAction::Finish => {
            // The capture's own page, or none rather than the last
            // dictation's.
            let context = context.finish(events)?;
            finish_hotkey_dictation(
                action_at,
                dictation,
                worker,
                &context,
                device,
                events,
                indicator,
                release.capture,
                print,
                raw,
                None,
            )
            .map(drop)
        }
        HotkeyAction::Discard(_) | HotkeyAction::Cancel | HotkeyAction::CancelShortcut => {
            context.cancel();
            feedback::retract_start();
            #[cfg(hex_capture)]
            crate::screenshots::narration::ended();
            if action == HotkeyAction::CancelShortcut {
                feedback::play(Tone::Cancel);
            }
            end_capture(
                if matches!(action, HotkeyAction::Cancel | HotkeyAction::CancelShortcut) {
                    CaptureEnd::Cancelled
                } else {
                    CaptureEnd::Discarded(action.discard_cause())
                },
                dictation,
                device,
                events,
                indicator,
            )
        }
        // Labs › Model switch shortcut. It records nothing, and the chord's
        // modifier press may have started a provisional capture, which is
        // withdrawn as Paste Last's is. A capture already under way keeps its
        // model: the shortcut is ignored until it ends.
        HotkeyAction::ModelSwitch => {
            if crate::audio::recording_environment::intentional_recording_active() {
                tracing::info!("the model switch shortcut was ignored while a capture records");
            } else {
                context.cancel();
                feedback::retract_start();
                dictation.cancel()?;
                // The runtime owns model preparation, so the press only asks
                // it for the swap.
                crate::transcription::other_model::request();
            }
            emit_engine_state(events, false, worker, device)
        }
        HotkeyAction::PasteLast => {
            // The paste chord's modifier press started a provisional capture.
            context.cancel();
            feedback::retract_start();
            dictation.cancel()?;
            if let Err(error) = worker.paste_last() {
                feedback::play(Tone::Error);
                events.dictation(DictationPhase::Failed(error.into()), "")?;
            }
            emit_engine_state(events, false, worker, device)
        }
    }?;
    Ok(true)
}

/// Ends a shortcut dictation at `action_at` and hands it to transcription.
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_hotkey_dictation(
    action_at: CaptureInstant,
    dictation: &DictationAudio,
    worker: &DictationWorker,
    context: &ContextSnapshot,
    device: &str,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    pressed_cold: bool,
    print: Option<PrintReply>,
    raw: bool,
    stop_phrase: Option<String>,
) -> Result<FinishResult> {
    if dictation.become_intentional(action_at)? && pressed_cold {
        feedback::play(Tone::DictationStart);
    }
    // A dictation over a screenshot lands its words there, never pasted; a
    // printing capture stops that shot's editor waiting instead.
    #[cfg(hex_capture)]
    let print = print
        .inspect(|_| crate::screenshots::narration::ended())
        .or_else(crate::screenshots::narration::claim);
    // A discarded or silent clip never reaches transcription, so its waiter
    // still hears from it.
    let unanswered = print.clone();
    let finished = match print {
        None => finish_dictation(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            worker,
            if raw {
                TranscriptionTarget::RawPaste
            } else {
                TranscriptionTarget::Paste
            },
            context,
            device,
            events,
            indicator,
            stop_phrase,
        )?,
        Some(print) => finish_dictation_with(
            dictation,
            action_at,
            POST_RELEASE_TAIL,
            // Narration lands on its screenshot either way, never pasted.
            |clip, _| worker.transcribe_printed(clip, print, raw, context.clone(), stop_phrase),
            device,
            events,
            indicator,
        )?,
    };
    if matches!(finished, FinishResult::Discarded) {
        feedback::retract_start();
    }
    // A discarded or silent clip submitted no job: say so instead of leaving
    // the printing command to its cap.
    if matches!(finished, FinishResult::Discarded | FinishResult::Silent)
        && let Some(print) = unanswered
    {
        print.send(crate::control::PrintedTranscript::NoSpeech);
    }
    emit_engine_state(events, false, worker, device)?;
    Ok(finished)
}

/// A control start has no press to judge, so its capture counts as
/// intentional from its first sample: the recording environment starts and
/// the live stream opens now. `tone_owed` plays the start tone for a capture
/// that's already open; one still opening the microphone plays it on
/// `ReadyIntentional`.
pub(super) fn make_control_capture_intentional(
    input: &DictationAudio,
    tone_owed: bool,
) -> Result<()> {
    if input.mark_intentional()? && tone_owed {
        feedback::play(Tone::DictationStart);
    }
    Ok(())
}

/// The reply to a control `stop`: only a submitted clip transcribes.
pub(super) fn finish_reply(finished: Option<FinishResult>, submitted: &str) -> ControlReply {
    match finished {
        Some(FinishResult::Submitted) => ControlReply::done(submitted),
        Some(FinishResult::Discarded) | None => ControlReply::error(
            "discarded",
            "too short, discarded: no audio was recorded yet (the microphone may still have \
             been opening)",
        ),
        Some(FinishResult::Silent) => ControlReply::error(
            "discarded",
            "the recording was silent, discarded; nothing to transcribe",
        ),
        Some(FinishResult::Rejected(error)) => {
            ControlReply::error("failed", format!("couldn't transcribe it: {error}"))
        }
    }
}

/// `hex dictation`: a start locks the shortcut machine as a double tap
/// does, so the next press or `stop` finishes it. A printing start keeps the
/// command's reply here until that recording ends, however it ends.
#[allow(clippy::too_many_arguments)]
pub(super) fn control_dictation(
    action: CaptureAction,
    print: Option<PrintReply>,
    raw: bool,
    other_recording: bool,
    suspended: bool,
    hotkeys: &mut Hotkeys,
    input: &DictationAudio,
    worker: &DictationWorker,
    context: &mut ContextWatch,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    release: &mut Release,
) -> Result<ControlReply> {
    let recording = hotkeys.any_recording();
    let device = input.device_name();
    let start = match action {
        CaptureAction::Start => true,
        CaptureAction::Stop => false,
        CaptureAction::Toggle => !recording,
        CaptureAction::Cancel if recording => {
            hotkeys.cancel_capture();
            hotkeys.suspend_recording();
            handle_hotkey_action(
                HotkeyAction::CancelShortcut,
                false,
                None,
                input.captured_through(),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release,
            )?;
            return Ok(ControlReply::done("Cancelled the recording"));
        }
        CaptureAction::Cancel => return Ok(ControlReply::unchanged("Dictation isn't recording")),
    };
    Ok(match start {
        true if recording && print.is_some() => ControlReply::error(
            "busy",
            "a dictation is already recording; finish it before starting a print-only one",
        ),
        true if recording => ControlReply::unchanged("Dictation is already recording"),
        true => {
            // A reply still held belongs to a recording that already ended;
            // it must never answer this one.
            hotkeys.fail_print("its recording already ended");
            if let Some(refusal) = capture_refusal(other_recording, suspended) {
                return Ok(refusal);
            }
            if !hotkeys.dictation.lock() {
                return Ok(ControlReply::error(
                    "busy",
                    "the dictation shortcut is held; release it and try again",
                ));
            }
            hotkeys.begin_capture(Shortcut::Dictation, raw);
            let accepted = handle_hotkey_action(
                HotkeyAction::Start,
                raw,
                None,
                capture_boundary(input, release.press),
                input,
                worker,
                context,
                &device,
                events,
                indicator,
                release,
            )?;
            if accepted {
                // Labs › YOLO mode never sends a command's capture.
                worker.set_started_by_shortcut(false);
                // A warm start already queued its provisional tone. A control
                // start has no hold threshold to wait for, so its capture is
                // real now and reads its page at once.
                context.intend();
                make_control_capture_intentional(input, release.press)?;
                if let Some(print) = print {
                    // However this recording ends, its text comes back here.
                    hotkeys.keep_print(print);
                    ControlReply::done("Recording; the transcript comes back here when it ends")
                } else {
                    ControlReply::done("Recording; `hex dictation stop` or the shortcut finishes")
                }
            } else {
                hotkeys.suspend(Shortcut::Dictation);
                ControlReply::error("busy", "the microphone is busy; try again")
            }
        }
        false if !recording && print.is_some() => ControlReply::error(
            "not-recording",
            "dictation isn't recording; nothing to print",
        ),
        false if !recording => ControlReply::unchanged("Dictation isn't recording"),
        false => {
            // Whether this command prints, read before a print-only
            // recording's start reply joins it: a plain stop answers as a
            // plain stop does.
            let printing = print.is_some();
            let active = hotkeys.take_capture();
            let raw = active.as_ref().is_some_and(|capture| capture.raw);
            let start_print = active.and_then(|capture| capture.print);
            let start_printed = start_print.is_some();
            // A print-only recording answers its start command too: both it
            // and a printing stop hear the one transcript.
            let print = match (start_print, print) {
                (Some(start), Some(stop)) => Some(start.join(stop)),
                (Some(start), None) => Some(start),
                (None, print) => print,
            };
            hotkeys.suspend_recording();
            // A control capture asked for its page when it started; this
            // waits for that read if it is still in flight.
            let context = context.finish(events)?;
            let finished = finish_hotkey_dictation(
                capture_boundary(input, release.capture),
                input,
                worker,
                &context,
                &device,
                events,
                indicator,
                release.capture,
                print,
                raw,
                None,
            )?;
            match finished {
                FinishResult::Discarded | FinishResult::Silent if printing => {
                    ControlReply::error("no-speech", "no speech was recorded; nothing to print")
                }
                finished if printing => finish_reply(
                    Some(finished),
                    "Finished recording; printing the transcript",
                ),
                finished if start_printed => finish_reply(
                    Some(finished),
                    "Finished recording; the transcript goes to the command that started it",
                ),
                finished => finish_reply(
                    Some(finished),
                    "Finished recording; Hex pastes the text when it's ready",
                ),
            }
        }
    })
}

/// Pastes the last dictation at the current focus.
pub(super) fn paste_last(
    worker: &DictationWorker,
    device: &str,
    events: &mut EventLog,
) -> Result<ControlReply> {
    let reply = match worker.paste_last() {
        #[cfg(hex_capture)]
        Ok(()) if crate::paste::recent::enabled() => ControlReply::done("Pasting the latest item"),
        Ok(()) => ControlReply::done("Pasting the last dictation"),
        Err(error) => {
            feedback::play(Tone::Error);
            events.dictation(DictationPhase::Failed(error.into()), "")?;
            ControlReply::error("failed", format!("couldn't paste: {error}"))
        }
    };
    emit_engine_state(events, false, worker, device)?;
    Ok(reply)
}
