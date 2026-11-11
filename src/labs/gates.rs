//! The runtime gate of each Labs feature: whether this process has its
//! switch on and this OS has the feature at all. Split from
//! [`super::Feature`] to keep the registry under the source cap.

use super::*;

/// Live text: provisional words beside the HUD (read as each dictation starts).
pub fn live_text() -> bool {
    on(LIVE_TEXT)
}

/// A failed dictation's explanation beside the HUD (read as each failure
/// reaches the HUD).
pub fn failure_notices() -> bool {
    on(FAILURE_NOTICES) && Feature::FailureNotices.on_this_os()
}

/// Live typing: provisional words in the focused text field as an
/// input-method preedit on Wayland, and the final text typed there instead of
/// pasted (Linux; read as each hotkey capture starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn live_typing() -> bool {
    on(LIVE_TYPING) && Feature::LiveTyping.on_this_os()
}

/// Live chapters while a meeting records.
pub fn live_chapters() -> bool {
    on(LIVE_CHAPTERS)
}

/// The playground in the window.
pub fn playground() -> bool {
    on(PLAYGROUND)
}

/// Transcription biased toward the vocabulary (Parakeet).
pub fn vocabulary_biasing() -> bool {
    on(VOCABULARY_BIASING)
}

/// Mouse buttons may be recorded as shortcuts (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn mouse_shortcuts() -> bool {
    on(MOUSE_SHORTCUTS)
}

/// A pasted dictation stays on the clipboard rather than the previous
/// clipboard being restored (macOS and Windows; read as each paste commits).
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub fn keep_on_clipboard() -> bool {
    on(KEEP_ON_CLIPBOARD)
}

/// A dictation goes to the clipboard and is never pasted (macOS and
/// Windows; read as each dictation's output commits).
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub fn copy_only() -> bool {
    on(COPY_ONLY)
}

/// A dictation pastes into the app its capture started in, bringing it back
/// to the front first if another app came forward while the person talked
/// (macOS; read as each capture starts and again before its paste).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn return_to_app() -> bool {
    on(RETURN_TO_APP) && Feature::ReturnToApp.on_this_os()
}

/// Spoken formatting phrases become line breaks and quotation marks (read
/// as each dictation's processing starts).
pub fn spoken_formatting() -> bool {
    on(SPOKEN_FORMATTING)
}

/// Spoken punctuation and symbol words become characters joined to their
/// neighbours (read as each dictation's processing starts).
pub fn spoken_symbols() -> bool {
    on(SPOKEN_SYMBOLS)
}

/// Filler words such as "um" and "uh" leave dictations (read as each
/// dictation's processing starts).
pub fn filler_words() -> bool {
    on(FILLER_WORDS)
}

/// `hex meetings show --srt|--vtt` may export subtitles (read when the
/// command starts).
pub fn subtitle_export() -> bool {
    on(SUBTITLE_EXPORT)
}

/// A pasted dictation gets one leading space when the character before the
/// cursor asks for one (macOS; read as each paste commits).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn smart_spacing() -> bool {
    on(SMART_SPACING)
}

/// The shortcut recorder keeps each modifier's side in a modifier-only
/// chord of two or more keys (macOS; read as each recording starts).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn sided_modifier_chords() -> bool {
    on(SIDED_CHORDS)
}

/// In Toggle mode, a press held past the hold threshold finishes its capture
/// when it lifts instead of locking it (macOS reads it at each release;
/// Linux's listener reads the saved switch with its shortcut options).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn tap_or_hold() -> bool {
    on(TAP_OR_HOLD)
}

/// Double-tap shortcut mode accepts a shortcut without a key, such as
/// Option, Globe, or ⌃⌥ (macOS; read as each shortcut machine is
/// configured).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn double_tap_modifiers() -> bool {
    on(DOUBLE_TAP_MODIFIERS) && Feature::DoubleTapModifiers.on_this_os()
}

/// Parakeet Ultra joins the catalog (read wherever a model is listed,
/// chosen, or run).
pub fn parakeet_ultra() -> bool {
    on(PARAKEET_ULTRA)
}

/// History's summary of retained dictations and `hex history stats` (read
/// when the History pane reloads and when the command starts).
pub fn history_stats() -> bool {
    on(HISTORY_STATS)
}

/// History selects several dictations at once, with Shift, Command (Control
/// on Windows and Linux), or a drag, and copies or deletes them together.
/// The window reads the same switch from `settings.labs`, as it reads Keep
/// recordings; this process's copy is logged at launch with the other labs.
pub fn history_multi_select() -> bool {
    on(HISTORY_MULTI_SELECT)
}

/// The menu bar's Copy Recent Dictation submenu (macOS; read each time the
/// menu opens).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn recent_dictations() -> bool {
    on(RECENT_DICTATIONS)
}

/// A whole-clip dictation transcription hears only the head and tail of
/// each long pause (Linux and Windows; read as each whole-clip run starts).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn skip_pauses() -> bool {
    on(SKIP_PAUSES) && Feature::SkipPauses.on_this_os()
}

/// A meeting recording Hex started on its own stops once its call app has
/// left the microphone and gone quiet for a grace period (macOS; read on
/// each pass of the meeting controller).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn stop_when_call_ends() -> bool {
    on(STOP_WHEN_CALL_ENDS) && Feature::StopWhenCallEnds.on_this_os()
}

/// Calls in more apps and on more websites are named and offered (read by
/// call detection and the window's Detected calls list).
pub fn more_call_apps() -> bool {
    on(MORE_CALL_APPS) && Feature::MoreCallApps.on_this_os()
}

/// A dictation saved to History keeps its 16 kHz audio, and History offers
/// Transcribe again for it (read as each capture is handed to the engine;
/// the History pane reads the saved setting).
pub fn keep_recordings() -> bool {
    on(KEEP_RECORDINGS)
}

/// The menu-bar icon is hidden while nothing records (macOS; read each
/// time the status item refreshes, about twice a second).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn hide_menu_bar_icon() -> bool {
    on(HIDE_MENU_BAR_ICON) && Feature::HideMenuBarIcon.on_this_os()
}

/// ⌘Q closes the Settings window instead of quitting Hex (macOS; read each
/// time the app menu's Quit Hex runs).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn quit_closes_window() -> bool {
    on(QUIT_CLOSES_WINDOW) && Feature::QuitClosesWindow.on_this_os()
}

/// The meeting panel capsules (Meeting transcript ready, Recording
/// automatically, offers, reminders) show in the saved capsule corner and
/// can be dragged to another (macOS; read each time one shows).
pub fn movable_meeting_capsules() -> bool {
    on(MOVABLE_MEETING_CAPSULES) && Feature::MovableMeetingCapsules.on_this_os()
}

/// Final meeting notes (title and summary) put the vocabulary ahead of the
/// transcript (every OS with meetings; read at each generation, so
/// Regenerate summary uses the current list).
pub fn meeting_notes_vocabulary() -> bool {
    on(MEETING_NOTES_VOCABULARY) && Feature::MeetingNotesVocabulary.on_this_os()
}

/// At either visible indicator position, the HUD grows out of the notch on
/// a screen that has one (macOS; read each time the HUD is placed).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn notch_indicator() -> bool {
    on(NOTCH_INDICATOR) && Feature::NotchIndicator.on_this_os()
}

/// A paste on a Wayland desktop without data-control (GNOME) snapshots and
/// restores the clipboard through an already running XWayland (Linux; read
/// as each paste starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_clipboard_restore() -> bool {
    on(GNOME_CLIPBOARD_RESTORE) && Feature::GnomeClipboardRestore.on_this_os()
}

/// A shortcut runs the focused app's Output steps on the text already typed
/// there and replaces it (Linux; read as each shortcut press starts and by
/// the Output pane's row).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn typed_text() -> bool {
    on(TYPED_TEXT) && Feature::TypedText.on_this_os()
}

/// On a Wayland desktop without layer-shell (GNOME), the meeting capsules
/// float on an already running XWayland (Linux; read as each capsule shows).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_meeting_capsules() -> bool {
    on(GNOME_MEETING_CAPSULES) && Feature::GnomeMeetingCapsules.on_this_os()
}

/// On a Wayland desktop without layer-shell (GNOME), Annotate's capsule and
/// ink float on an already running XWayland (Linux; read as each overlay
/// shows).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_annotate() -> bool {
    on(GNOME_ANNOTATE) && Feature::GnomeAnnotate.on_this_os()
}

/// The local API's `GET /meetings/events` serves, and a meeting's final
/// publication sends, its id (every OS with meetings; read where the route
/// starts, on each pass of its stream, and at each publication).
pub fn meeting_finished_event() -> bool {
    on(MEETING_FINISHED_EVENT) && Feature::MeetingFinishedEvent.on_this_os()
}

/// The screenshot shortcut takes an area or a window (macOS; read by the
/// session tap on each key press, so nothing runs while it is off).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn screenshots() -> bool {
    on(SCREENSHOTS) && Feature::Screenshots.on_this_os()
}

/// Annotate's rolling 15 s display replay buffer (`SCClipBufferingOutput`,
/// macOS 27+; read where the replay worker starts and on each `hex capture
/// replay`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn annotate_replay() -> bool {
    on(ANNOTATE_REPLAY) && Feature::AnnotateReplay.on_this_os()
}

/// Audio files may be imported as meetings (macOS; read where the Meetings
/// pane offers Import audio and where an import starts).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_import() -> bool {
    on(MEETING_IMPORT) && Feature::MeetingImport.on_this_os()
}

/// Notes or a user rename rename an exported meeting's Markdown to the
/// meeting's title, keeping its date and time (read when notes are applied
/// and when a meeting is renamed).
pub fn meeting_descriptive_filenames() -> bool {
    on(MEETING_DESCRIPTIVE_FILENAMES) && Feature::MeetingDescriptiveFilenames.on_this_os()
}

/// A meeting's transcript exports as `<stem>.transcript.md` and its notes
/// are written to `<stem>.notes.md` beside it, linking back to the transcript
/// (read at final export and when notes are written).
pub fn meeting_notes_file() -> bool {
    on(MEETING_NOTES_FILE) && Feature::MeetingNotesFile.on_this_os()
}

/// "While dictating" applies only while the default output is speakers
/// (read as each dictation or screen capture starts: by the recording
/// environment on macOS, and by the audio-control worker on Linux and
/// Windows).
pub fn speakers_only() -> bool {
    on(SPEAKERS_ONLY) && Feature::SpeakersOnly.on_this_os()
}

/// The menu bar offers Pause Hex, and every capture shortcut is inert while
/// it is paused (macOS; read each time the status item refreshes, as each
/// shortcut's suspension is read, and when settings change so turning the
/// lab off resumes).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn pause_menu() -> bool {
    on(PAUSE_MENU) && Feature::PauseMenu.on_this_os()
}

/// A physical Caps Lock press starts and stops dictation, and its light
/// follows Hex's recording and processing (macOS; read by the listener on
/// every pass and by the event tap and HID tracker through their own flag).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn caps_lock_dictation() -> bool {
    on(CAPS_LOCK_DICTATION) && Feature::CapsLockDictation.on_this_os()
}

/// Whether the Caps Lock light blinks slowly while recording instead of
/// staying solid (macOS; read by the light model for each pattern).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn caps_lock_slow_blink() -> bool {
    on(CAPS_LOCK_SLOW_BLINK) && Feature::CapsLockDictation.on_this_os()
}

/// An OpenCode service running in the default WSL distribution is used when
/// no Windows registration answers (Windows; read as each service search
/// starts).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn opencode_in_wsl() -> bool {
    on(OPENCODE_IN_WSL) && Feature::OpenCodeInWsl.on_this_os()
}

/// The local API's `POST /transcriptions` and `POST /streams/{id}/finish`
/// accept `process=global` (read where each request's route starts and by
/// the capabilities answer).
pub fn api_processing() -> bool {
    on(API_PROCESSING) && Feature::ApiProcessing.on_this_os()
}

/// The local API's `POST /transcriptions` accepts `timestamps=segment|word`
/// and returns the model's own times (read where each request's route starts).
pub fn api_timestamps() -> bool {
    on(API_TIMESTAMPS) && Feature::ApiTimestamps.on_this_os()
}

/// Finished meetings may drop their WAV tracks on `meetings.audio_retention`
/// (read by the meeting controller's expiry pass and the window's Settings
/// row).
pub fn expire_meeting_audio() -> bool {
    on(EXPIRE_MEETING_AUDIO) && Feature::ExpireMeetingAudio.on_this_os()
}

/// How long the speech model may sit unused before the engine unloads it,
/// while Labs › Unload model when idle is on.
pub fn unload_idle() -> Option<std::time::Duration> {
    let state = state();
    (state & UNLOAD_IDLE != 0)
        .then(|| std::time::Duration::from_secs(((state >> MINUTES_SHIFT) & MINUTES_MASK) * 60))
}
