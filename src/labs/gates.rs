//! The runtime gate of each Labs feature: whether this process has its
//! switch on, this build ships it, this OS has it, and no managed policy
//! locks it. All four checks are folded into the switch bank in `super`.

use super::*;

/// Live text: provisional words beside the HUD (read as each dictation starts).
pub fn live_text() -> bool {
    feature_on(Feature::LiveText)
}

/// A shorter lookahead for live text, so its words appear sooner (Parakeet
/// buffered streaming; read as each live stream opens).
pub fn faster_live_text() -> bool {
    feature_on(Feature::FasterLiveText)
}

/// A failed dictation's explanation beside the HUD (read as each failure
/// reaches the HUD).
pub fn failure_notices() -> bool {
    feature_on(Feature::FailureNotices)
}

/// Live typing: provisional words in the focused text field as an
/// input-method preedit on Wayland, and the final text typed there instead of
/// pasted (Linux; read as each hotkey capture starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn live_typing() -> bool {
    feature_on(Feature::LiveTyping)
}

/// Live chapters while a meeting records.
pub fn live_chapters() -> bool {
    feature_on(Feature::LiveChapters)
}

/// Stop an offered, calendar, or by-hand call recording once its call app
/// leaves the microphone (read as each recording's start and every
/// controller pass while it runs).
pub fn stop_any_call_recording() -> bool {
    feature_on(Feature::StopAnyCallRecording)
}

/// A meeting recording can mute Hex's microphone without stopping: its
/// packets stay the same length and go silent, so the call's audio keeps
/// recording and no "You" line is produced (macOS; read as each microphone
/// packet reaches the track, so turning the lab off restores audio at once,
/// once per controller pass, which clears the switch, and where the pane and
/// menu bar offer the switch and the capsule shows its notice).
pub fn meeting_microphone_mute() -> bool {
    feature_on(Feature::MeetingMicrophoneMute)
}

/// Labs › Indicator on any display: a released meeting capsule lands in
/// the corner of the display under the pointer, and the capsules show on the
/// display they were dragged to instead of only the primary display (macOS
/// and Windows; read as each capsule draws and as each drag ends).
pub fn meeting_indicator_display() -> bool {
    feature_on(Feature::MeetingIndicatorDisplay)
}

/// The playground in the window.
pub fn playground() -> bool {
    feature_on(Feature::Playground)
}

/// Names and terms visible in the focused window join one dictation's
/// vocabulary, read once as the capture becomes real (macOS; read by the
/// listener at its hold threshold, its control start, or the released
/// microphone opening).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn screen_vocabulary() -> bool {
    feature_on(Feature::ScreenVocabulary)
}

/// Names a person fixes in text Hex just pasted join Vocabulary, read from
/// that field alone for a few minutes after the paste (macOS; read as each
/// paste commits).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn learn_from_corrections() -> bool {
    feature_on(Feature::LearnFromCorrections)
}

/// Mouse buttons may be recorded as shortcuts (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn mouse_shortcuts() -> bool {
    feature_on(Feature::MouseShortcuts)
}

/// The microphone (dictation) key in a laptop keyboard's top row may be a
/// shortcut, alone or with modifiers (macOS; read as the session tap attaches
/// and by the tap on every press).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn microphone_key() -> bool {
    feature_on(Feature::MicrophoneKeyAlone)
}

/// The mode's OpenCode rewrite is given the text around the cursor as
/// context, read once as the capture becomes real (macOS; read by the
/// listener at its hold threshold, its control start, or the released
/// microphone opening).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn cursor_context() -> bool {
    feature_on(Feature::CursorContext)
}

/// A finished Annotate capture copies to the clipboard as one light zip file
/// rather than its image and summary (read where a written capture is copied).
#[cfg_attr(not(hex_capture), allow(dead_code))]
pub fn copy_capture_zip() -> bool {
    feature_on(Feature::CopyCaptureZip)
}

/// While an Annotate capture records, the dictation shortcut dictates as it
/// always does: hold, toggle, or double-tap lock it, and the text pastes
/// into the app in front, History, and Paste Last. The capture keeps
/// recording, and its narration hears the words too. Read at each press the
/// capture would otherwise swallow, on every OS.
#[cfg_attr(not(hex_capture), allow(dead_code))]
pub fn dictate_while_annotating() -> bool {
    feature_on(Feature::DictateWhileAnnotating)
}

/// A kept dictation recording may be copied from History and the menu bar or
/// tray as a file on the clipboard (read as each Copy audio runs and each
/// menu or tray opens). It needs Labs › Keep recordings, which owns every
/// file; this lab stores none of its own.
pub fn copy_dictation_audio() -> bool {
    feature_on(Feature::CopyDictationAudio) && keep_recordings()
}

/// Whether `settings` enable Labs › Copy dictation audio (both it and Keep
/// recordings on, shipped, available on this OS, and unlocked by policy).
/// Used by UI views and actions that inspect an explicit `AppSettings`
/// snapshot rather than the process switch bank.
pub fn copy_dictation_audio_enabled(settings: &AppSettings) -> bool {
    copy_dictation_audio_enabled_under(settings, crate::policy::Policy::current())
}

pub(crate) fn copy_dictation_audio_enabled_under(
    settings: &AppSettings,
    policy: crate::policy::Policy,
) -> bool {
    Feature::CopyDictationAudio.enabled_under(settings, policy)
        && Feature::KeepRecordings.enabled_under(settings, policy)
}

/// A pasted dictation stays on the clipboard rather than the previous
/// clipboard being restored (read as each paste commits).
pub fn keep_on_clipboard() -> bool {
    feature_on(Feature::KeepOnClipboard)
}

/// A dictation goes to the clipboard and is never pasted (macOS and
/// Windows; read as each dictation's output commits).
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub fn copy_only() -> bool {
    feature_on(Feature::CopyOnly)
}

/// A dictation pastes into the app its capture started in, bringing it back
/// to the front first if another app came forward while the person talked
/// (macOS; read as each capture starts and again before its paste).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn return_to_app() -> bool {
    feature_on(Feature::ReturnToApp)
}

/// Spoken formatting phrases become line breaks and quotation marks (read
/// as each dictation's processing starts).
pub fn spoken_formatting() -> bool {
    feature_on(Feature::SpokenFormatting)
}

/// Spoken punctuation and symbol words become characters joined to their
/// neighbours (read as each dictation's processing starts).
pub fn spoken_symbols() -> bool {
    feature_on(Feature::SpokenSymbols)
}

/// Filler words such as "um" and "uh" leave dictations (read as each
/// dictation's processing starts).
pub fn filler_words() -> bool {
    feature_on(Feature::FillerWords)
}

/// Labs › Clean up with Apple Intelligence: one on-device pass over each
/// macOS dictation (read as each dictation's processing starts, and for the
/// HUD's processing stage).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn clean_up() -> bool {
    feature_on(Feature::CleanUpWithAppleIntelligence)
}

/// Labs › Neutral voice: a dictation in a mode that opts in is rewritten into
/// plain, neutral wording on this Mac, instead of the OpenCode rewrite and
/// the cleanup (read as each dictation's processing starts, and for the HUD's
/// processing stage).
pub fn neutral_voice() -> bool {
    feature_on(Feature::NeutralVoice)
}

/// A pasted dictation gets one leading space when the character before the
/// cursor asks for one (macOS; read as each paste commits).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn smart_spacing() -> bool {
    feature_on(Feature::SmartSpacing)
}

/// The shortcut recorder keeps each modifier's side in a modifier-only
/// chord of two or more keys (macOS; read as each recording starts).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn sided_modifier_chords() -> bool {
    feature_on(Feature::SidedChords)
}

/// In Toggle mode, a press held past the hold threshold finishes its capture
/// when it lifts instead of locking it (macOS reads it at each release;
/// Linux's listener reads the saved switch with its shortcut options).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn tap_or_hold() -> bool {
    feature_on(Feature::TapOrHold)
}

/// Double-tap shortcut mode accepts a shortcut without a key, such as
/// Option, Globe, or ⌃⌥ (macOS; read as each shortcut machine is
/// configured).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn double_tap_modifiers() -> bool {
    feature_on(Feature::DoubleTapModifiers)
}

/// A key or click joining a held modifier-only shortcut a second or more
/// after it went down is left to the app in front, so the release still
/// finishes the dictation (macOS and Windows; read by each OS's shortcut
/// machine as the key or click arrives).
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub fn hold_through_keys() -> bool {
    feature_on(Feature::HoldThroughKeys)
}

/// Parakeet Ultra joins the catalog (read wherever a model is listed,
/// chosen, or run).
pub fn parakeet_ultra() -> bool {
    feature_on(Feature::ParakeetUltra)
}

/// Labs › Dictation stats: History's summary of retained dictations, the
/// daily rollup that outlives retention, and `hex history stats` (read as
/// each entry is recorded, when the History pane reloads, and when the
/// command starts). Off, the rollup is never written or read.
pub fn history_stats() -> bool {
    feature_on(Feature::HistoryStats)
}

/// Labs › Stats on History: the Dictation stats dialog's six headline
/// numbers as one compact strip above History's list (read as the History
/// pane draws). It needs Labs › Dictation stats, which owns the numbers
/// and the rollup.
pub(crate) fn history_stats_strip_enabled(settings: &AppSettings) -> bool {
    history_stats_strip_enabled_under(settings, crate::policy::Policy::current())
}

pub(crate) fn history_stats_strip_enabled_under(
    settings: &AppSettings,
    policy: crate::policy::Policy,
) -> bool {
    Feature::HistoryStatsStrip.enabled_under(settings, policy)
        && Feature::HistoryStats.enabled_under(settings, policy)
}

/// The menu bar's Copy Recent Dictation submenu (macOS; read each time the
/// menu opens).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn recent_dictations() -> bool {
    feature_on(Feature::RecentDictations)
}

/// The menu bar's Vocabulary submenu: Add term…, the last saved terms, and
/// Open vocabulary… (macOS; read each time the status item refreshes, as the
/// menu opens, and by the menu's actions).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn vocabulary_menu() -> bool {
    feature_on(Feature::VocabularyMenu)
}

/// A whole-clip dictation transcription hears only the head and tail of
/// each long pause (Linux and Windows; read as each whole-clip run starts).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn skip_pauses() -> bool {
    feature_on(Feature::SkipPauses)
}

/// A dictation saved to History keeps its 16 kHz audio, and History offers
/// Transcribe again for it (read as each capture is handed to the engine;
/// the History pane reads the saved setting).
pub fn keep_recordings() -> bool {
    feature_on(Feature::KeepRecordings)
}

/// The menu-bar icon is hidden while nothing records (macOS; read each
/// time the status item refreshes, about twice a second).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn hide_menu_bar_icon() -> bool {
    feature_on(Feature::HideMenuBarIcon)
}

/// At either visible indicator position, the HUD grows out of the notch on
/// a screen that has one (macOS; read each time the HUD is placed).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn notch_indicator() -> bool {
    feature_on(Feature::NotchIndicator)
}

/// Labs › HUD height: with the HUD position Bottom, its plate keeps a chosen
/// gap above the Dock or the screen's edge (macOS; read each time the HUD is
/// placed).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn hud_height() -> bool {
    feature_on(Feature::HudHeight)
}

/// The plate's gap above the Dock or the screen's bottom edge at the Bottom
/// position: Labs › HUD height's saved points, or the fixed 24 pt while the
/// lab is off (macOS; read each time the HUD is placed).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn hud_bottom_gap() -> f64 {
    if hud_height() {
        HUD_BOTTOM_GAP.load(Ordering::Acquire) as f64
    } else {
        crate::rune_hud::hud_placement::BOTTOM_OFFSET
    }
}

/// Dictation's microphone opens through Apple's voice processing, so Control
/// Center's Mic Mode can offer Voice Isolation (macOS; read as each dictation
/// stream opens, and when an idle one may be replaced).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn voice_isolation() -> bool {
    feature_on(Feature::VoiceIsolation)
}

/// A paste on a Wayland desktop without data-control (GNOME) snapshots and
/// restores the clipboard through an already running XWayland (Linux; read
/// as each paste starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_clipboard_restore() -> bool {
    feature_on(Feature::GnomeClipboardRestore)
}

/// A shortcut runs the focused app's Output steps on the text already typed
/// there and replaces it (Linux; read as each shortcut press starts and by
/// the Output pane's row).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn typed_text() -> bool {
    feature_on(Feature::TypedText)
}

/// On a Wayland desktop without layer-shell (GNOME), the meeting capsules
/// float on an already running XWayland (Linux; read as each capsule shows).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_meeting_capsules() -> bool {
    feature_on(Feature::GnomeMeetingCapsules)
}

/// On a Wayland desktop without layer-shell (GNOME), Annotate's capsule and
/// ink float on an already running XWayland (Linux; read as each overlay
/// shows).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn gnome_annotate() -> bool {
    feature_on(Feature::GnomeAnnotate)
}

/// On Wayland, a screen capture asks the desktop for the monitor under the
/// pointer, and each monitor keeps its own ScreenCast choice (Linux; read as
/// each screen capture starts).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn annotate_pointer_screen() -> bool {
    feature_on(Feature::AnnotatePointerScreen)
}

/// The screenshot shortcut takes an area or a window (macOS; read by the
/// session tap on each key press, so nothing runs while it is off).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn screenshots() -> bool {
    feature_on(Feature::Screenshots)
}

/// Annotate's rolling 15 s display replay buffer (`SCClipBufferingOutput`,
/// macOS 27+; read where the replay worker starts and on each `hex capture
/// replay`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn annotate_replay() -> bool {
    feature_on(Feature::AnnotateReplay)
}

/// Audio files may be imported as meetings (macOS; read where the Meetings
/// pane offers Import audio and where an import starts).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_import() -> bool {
    feature_on(Feature::MeetingImport)
}

/// A saved meeting's transcript lines may be deleted (read where the pane
/// offers selection and where the controller starts an edit).
pub fn transcript_edits() -> bool {
    feature_on(Feature::TranscriptEdits)
}

/// "While dictating" applies only while the default output is speakers
/// (read as each dictation or screen capture starts: by the recording
/// environment on macOS, and by the audio-control worker on Linux and
/// Windows).
pub fn speakers_only() -> bool {
    feature_on(Feature::SpeakersOnly)
}

/// The menu bar offers Pause Hex, and every capture shortcut is inert while
/// it is paused (macOS; read each time the status item refreshes, as each
/// shortcut's suspension is read, and when settings change so turning the
/// lab off resumes).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn pause_menu() -> bool {
    feature_on(Feature::PauseMenu)
}

/// The menu bar lists the next calendar meetings, and choosing one with a
/// call link joins it (macOS; read each time the status item refreshes and
/// as the menu opens, and by the meeting controller before it reads its
/// calendar).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn calendar_menu() -> bool {
    feature_on(Feature::CalendarMenu)
}

/// A physical Caps Lock press starts and stops dictation, and its light
/// follows Hex's recording and processing (macOS; read by the listener on
/// every pass and by the event tap and HID tracker through their own flag).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn caps_lock_dictation() -> bool {
    feature_on(Feature::CapsLockDictation)
}

/// A spoken trigger phrase inside a dictation turns what follows it into a
/// one-off instruction for the mode's OpenCode model (read as each
/// dictation's processing starts). An organization's
/// DisableOpenCodeProcessing lock reads as off.
pub fn voice_prompt() -> bool {
    voice_prompt_under(crate::policy::Policy::current())
}

/// [`voice_prompt`] under an explicit policy, so its managed lock is read
/// where the gate is tested.
pub(crate) fn voice_prompt_under(policy: crate::policy::Policy) -> bool {
    feature_on(Feature::VoicePrompt) && !policy.locks(crate::policy::Lock::OpenCodeProcessing)
}

/// The words that start a voice prompt (read as each dictation's processing
/// starts, only while [`voice_prompt`] is on).
pub fn voice_prompt_phrase() -> String {
    ensure_loaded();
    VOICE_PROMPT_PHRASE
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Labs › Confirm long cancels: a dictation's cancel shortcut asks for a
/// second press once it has recorded for 10 seconds (read at each cancel
/// press, by each OS's shortcut machine).
pub fn confirm_long_cancels() -> bool {
    feature_on(Feature::ConfirmLongCancels)
}

/// Labs › Stop phrase: a spoken phrase at the end of a hands-free dictation
/// finishes it as if the shortcut were pressed again (read as each hands-free
/// capture starts). The partials it matches are produced while this or Live
/// text is on; the HUD's live-text line still needs [`live_text`].
pub fn stop_phrase() -> bool {
    feature_on(Feature::StopPhrase)
}

/// The phrase that finishes a hands-free dictation (read where each OS's
/// listener matches it, and once more when it fires so its job strips exactly
/// the words it heard).
pub fn stop_phrase_text() -> String {
    ensure_loaded();
    STOP_PHRASE_TEXT
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Labs › Signature: a normal dictation's final text carries the saved
/// signature on its own line at the end, before it is pasted, kept in
/// History, and remembered for Paste Last (every OS; read as each output
/// commits).
pub fn signature() -> bool {
    feature_on(Feature::Signature)
}

/// The signature line appended while [`signature`] is on (read as each
/// output commits, only while the gate is on). Empty appends nothing.
pub fn signature_text() -> String {
    ensure_loaded();
    SIGNATURE_TEXT
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Labs › Clipboard phrase: a spoken phrase inside a normal dictation becomes
/// the clipboard's text in double quotes before it is pasted, kept in
/// History, and remembered for Paste Last (every OS; read as each output
/// commits).
pub fn clipboard_phrase() -> bool {
    feature_on(Feature::ClipboardPhrase)
}

/// The words replaced with the clipboard while [`clipboard_phrase`] is on
/// (read as each output commits, only while the gate is on).
pub fn clipboard_phrase_text() -> String {
    ensure_loaded();
    CLIPBOARD_PHRASE_TEXT
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Labs › Choose meeting audio: the app or output a meeting's computer
/// audio records, or None while the lab is off (every OS; read by each
/// system capture source as it opens and on its route check).
pub fn meeting_audio_source() -> Option<crate::meeting::audio_source::SourceChoice> {
    feature_on(Feature::MeetingAudioSource).then(|| {
        MEETING_AUDIO_SOURCE_CHOICE
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    })
}

/// Labs › Dictation style: the style each normal dictation is written as,
/// or None while the lab is off (every OS; read once as each dictation's
/// processing starts).
pub fn dictation_style() -> Option<crate::app_settings::DictationStyle> {
    feature_on(Feature::DictationStyle).then(|| {
        *DICTATION_STYLE_CHOICE
            .read()
            .unwrap_or_else(|error| error.into_inner())
    })
}

/// Whether the Caps Lock light blinks slowly while recording instead of
/// staying solid (macOS; read by the light model for each pattern).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn caps_lock_slow_blink() -> bool {
    ensure_loaded();
    CAPS_LOCK_SLOW_BLINK.load(Ordering::Acquire)
}

/// A service running in the default WSL distribution is used ahead of a
/// Windows one when it answers; the Windows registration is the fallback when
/// WSL answers nothing (Windows; read as each service search starts).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn opencode_in_wsl() -> bool {
    feature_on(Feature::OpenCodeInWsl)
}

/// The local API's `POST /transcriptions` and `POST /streams/{id}/finish`
/// accept `process=global` (read where each request's route starts and by
/// the capabilities answer).
pub fn api_processing() -> bool {
    feature_on(Feature::ApiProcessing)
}

/// The local API's `POST /transcriptions` accepts `timestamps=segment|word`
/// and returns the model's own times (read where each request's route starts).
pub fn api_timestamps() -> bool {
    feature_on(Feature::ApiTimestamps)
}

/// Labs › Smaller model files: your own speech model imports may use Q6_K,
/// Q5_K, or Q4_K weights (macOS, where Metal has their kernels; read where an
/// import's header is checked and where a saved selection loads).
pub fn k_quant_models() -> bool {
    feature_on(Feature::KQuantModels)
}

/// The apps YOLO mode sends in (empty: everywhere), or None while off.
pub fn yolo_mode_apps() -> Option<Vec<String>> {
    ensure_loaded();
    YOLO_MODE_APPS
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Labs › Paste Last picker: holding Paste Last opens a picker of recent
/// dictations and annotations, and History lists them together (macOS; read
/// on every listener pass for the picker and the cancel-path words, and
/// where History's unified list starts, so it applies without a restart).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn paste_last_picker() -> bool {
    feature_on(Feature::PasteLastPicker)
}

/// Labs › Agent tools for captures: `hex annotate inspect`, `zoom`, `frames`,
/// and `skill` read a capture bundle in the CLI process (read by each of those
/// commands, in a process that never applied settings). An Annotate lock holds
/// it off even with its saved switch on.
pub fn annotate_agent_tools() -> bool {
    annotate_agent_tools_under(crate::policy::Policy::current())
}

/// [`annotate_agent_tools`] under an explicit policy, so its managed lock is
/// read where the gate is tested.
pub(crate) fn annotate_agent_tools_under(policy: crate::policy::Policy) -> bool {
    feature_on(Feature::AnnotateAgentTools)
        && policy.labs_lock(Feature::AnnotateAgentTools).is_none()
}

/// Labs › Transcribe dropped audio: a file with one of this build's
/// extensions, dropped on the window, is transcribed in this process and saved
/// as a text file beside it (read as each drop starts, and by the window's
/// drop target).
pub fn drop_transcribe() -> bool {
    feature_on(Feature::DropTranscribe)
}

/// How long the speech model may sit unused before the engine unloads it,
/// while Labs › Free memory when idle is on.
pub fn unload_idle() -> Option<std::time::Duration> {
    feature_on(Feature::UnloadIdle)
        .then(|| std::time::Duration::from_secs(UNLOAD_IDLE_MINUTES.load(Ordering::Acquire) * 60))
}

/// How long a capture's microphone keeps open after it ends, while Labs ›
/// Keep mic open after dictating is on: read as each capture ends and
/// on every pass that would release the microphone, so turning the lab off
/// releases it. `None` leaves Release microphone while idle exactly as it was.
pub fn microphone_linger() -> Option<std::time::Duration> {
    if !feature_on(Feature::KeepMicrophoneOpen) {
        return None;
    }
    match MICROPHONE_LINGER_SECONDS.load(Ordering::Acquire) {
        0 => None,
        seconds => Some(std::time::Duration::from_secs(seconds)),
    }
}
