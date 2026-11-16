//! Labs › Stop phrase: a hands-free capture that heard its phrase finishes as
//! its shortcut's finishing press would, and its job strips the words.
//!
//! The audio owner detects the phrase (it takes the live partials) and sends
//! the capture's generation with it, so a request that arrives after its
//! capture ended is dropped before it gets here. What is left is the
//! shortcut state machine: only a machine recording hands-free (a Toggle press
//! released, a completed double tap, or a command's lock) finishes, so a held
//! press reads the words as speech and records on.

use color_eyre::Result;

use super::{ContextWatch, DictationSink, Hotkeys, finish_hotkey_dictation};
use crate::desktop::events::EventLog;
use crate::dictation::audio::DictationAudio;
use crate::parakeet::DictationWorker;

/// The capture heard `phrase` and ends now, exactly as a press would:
/// its tone, HUD, context and release tail, and a job that removes the words.
/// Nothing happens when nothing records hands-free (the capture already ended,
/// its press is still held, or a client's dictation owns the microphone).
#[allow(clippy::too_many_arguments)]
pub(super) fn finish(
    phrase: String,
    hotkeys: &mut Hotkeys,
    input: &DictationAudio,
    worker: &DictationWorker,
    context: &mut ContextWatch,
    events: &mut EventLog,
    indicator: DictationSink<'_>,
    release_while_idle: bool,
) -> Result<()> {
    if !input.is_recording() {
        return Ok(());
    }
    let Some((shortcut, action)) = hotkeys.finish_hands_free() else {
        return Ok(());
    };
    // A `hex dictation start --print` capture answers its command, as its
    // press would.
    let print = hotkeys.print_for(shortcut, action);
    tracing::info!(
        phrase_chars = phrase.chars().count(),
        "the stop phrase finished a hands-free dictation"
    );
    let context = context.finish(events)?;
    finish_hotkey_dictation(
        input.captured_through(),
        input,
        worker,
        &context,
        &input.device_name(),
        events,
        indicator,
        release_while_idle,
        print,
        hotkeys.raw_capture,
        Some(phrase),
    )
    .map(drop)
}
