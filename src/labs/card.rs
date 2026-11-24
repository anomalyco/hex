//! The English title and line each Labs card shows. The window translates
//! them (`desktop::i18n`), and Hex's website builds a page per lab from them
//! (`labs::web`, `site/src/labs.json`).

use super::Feature;

impl Feature {
    /// The card's title.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Meetings => "Meetings (beta)",
            Self::VoiceFilter => "Voice Filter",
            Self::Playground => "Playground",
            Self::LiveText => "Live text",
            Self::FasterLiveText => "Faster live text",
            Self::FailureNotices => "Failure notices",
            Self::LiveTyping => "Live typing",
            Self::LiveChapters => "Live chapters",
            Self::ScreenVocabulary => "Words on screen",
            Self::LearnFromCorrections => "Learn from corrections",
            Self::TerminalPaste => "Terminal paste",
            Self::KeyboardModelSwitching => "Switch model with keyboard language",
            Self::ModelSwitchShortcut => "Model switch shortcut",
            Self::MouseShortcuts => "Mouse shortcuts",
            Self::PauseBrowser => "Also pause the browser",
            Self::ShareShortcut => "Share dictation shortcut with other apps",
            Self::UnloadIdle => "Free memory when idle",
            Self::KeepMicrophoneOpen => "Keep mic open after dictating",
            Self::KeepOnClipboard => "Keep dictation on clipboard",
            Self::CopyOnly => "Copy dictation only",
            Self::ReturnToApp => "Return to app",
            Self::SpokenFormatting => "Spoken formatting",
            Self::FillerWords => "Remove filler words",
            Self::CleanUpWithAppleIntelligence => "Clean up with Apple Intelligence",
            Self::NeutralVoice => "Neutral voice",
            Self::SecondDictationShortcut => "Second dictation shortcut",
            Self::TerminalPasteShortcut => "Terminal paste shortcut",
            Self::SmartSpacing => "Smart spacing",
            Self::SpokenSymbols => "Spoken symbols",
            Self::PickerSortFilter => "Sort and filter models",
            Self::SidedChords => "Left and right keys",
            Self::TapOrHold => "Tap or hold",
            Self::DoubleTapModifiers => "Double-tap Option, Command, or Globe",
            Self::BenchmarkDetails => "Model test details",
            Self::ParakeetUltra => "Parakeet Ultra",
            Self::StandaloneSpecialKeys => "Special keys alone",
            Self::HistoryStats => "Dictation stats",
            Self::HistoryStatsStrip => "Stats on History",
            Self::RecentDictations => "Copy recent dictations",
            Self::SkipPauses => "Skip long pauses",
            Self::OutputFixes => "Fix results in Output",
            Self::ModelInPlace => "Use model files where they are",
            Self::KQuantModels => "Smaller model files",
            Self::PickerDownloaded => "Downloaded models only",
            Self::SectionKeyAlone => "§ key alone",
            Self::MicrophoneKeyAlone => "Microphone key alone",
            Self::KeepRecordings => "Keep recordings",
            Self::Appearance => "Appearance",
            Self::HideMenuBarIcon => "Hide menu bar icon",
            Self::GnomeClipboardRestore => "Restore clipboard on GNOME",
            Self::TypedText => "Transform typed text",
            Self::NotchIndicator => "Notch indicator",
            Self::GnomeMeetingCapsules => "Call recording controls on GNOME",
            Self::GnomeAnnotate => "Annotate on GNOME",
            Self::AnnotatePointerScreen => "Record the screen under the pointer",
            Self::Screenshots => "Screenshots",
            Self::AnnotateReplay => "Annotate last 15 seconds",
            Self::MeetingImport => "Import audio",
            Self::TranscriptEdits => "Delete transcript lines",
            Self::CapsLockDictation => "Caps Lock dictation",
            Self::OpenCodeInWsl => "OpenCode in WSL",
            Self::ApiProcessing => "Process API transcripts",
            Self::ApiTimestamps => "Timestamps in API transcripts",
            Self::SpeakersOnly => "Only on speakers",
            Self::MoreDictationShortcuts => "More dictation shortcuts",
            Self::PauseMenu => "Pause from menu bar",
            Self::CalendarMenu => "Upcoming meetings in menu bar",
            Self::CalendarSelectAll => "Select all calendars",
            Self::VoicePrompt => "Rewrite by voice",
            Self::CursorContext => "Rewrite reads nearby text",
            Self::CopyCaptureZip => "Copy a capture as one file",
            Self::YoloMode => "YOLO mode",
            Self::CopyDictationAudio => "Copy dictation audio",
            Self::StopPhrase => "Stop phrase",
            Self::HoldThroughKeys => "Hold through other keys",
            Self::ModesEditor => "Modes editor",
            Self::DropTranscribe => "Transcribe dropped audio",
            Self::PasteLastPicker => "Paste Last picker",
            Self::VocabularyFiles => "Vocabulary files",
            Self::VocabularyMenu => "Vocabulary menu",
            Self::AnnotateAgentTools => "Agent tools for captures",
            Self::AnnotateAgentInstall => "Install agent skill from Annotate",
            Self::Signature => "Signature",
            Self::DictationStyle => "Dictation style",
            Self::HudHeight => "HUD height",
            Self::VoiceIsolation => "Voice isolation",
            Self::DictateWhileAnnotating => "Dictate while annotating",
            Self::StopAnyCallRecording => "Stop every call recording when the call ends",
            Self::ClipboardPhrase => "Clipboard phrase",
            Self::MeetingMicrophoneMute => "Mute the microphone while recording",
            Self::MeetingIndicatorDisplay => "Indicator on any display",
        }
    }

    /// The card's one line, under its title.
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Meetings => "Record calls and keep a written copy",
            Self::VoiceFilter => "A Hex Microphone other apps can pick, with live voice filters",
            Self::Playground => "Browse scenes and stories in a window",
            Self::LiveText => {
                "Show your words next to the small recording indicator (Dictation HUD) as you speak, before the text is pasted"
            }
            Self::FasterLiveText => {
                "Show live words sooner, and paste text from the same shorter lookahead, at a little more GPU work"
            }
            Self::FailureNotices => {
                "When a dictation fails, show the reason next to the small recording indicator (Dictation HUD)"
            }
            Self::LiveTyping => {
                "On Wayland, show your words in the text box as you speak, then Hex types the text there"
            }
            Self::LiveChapters => "Split a meeting's text into chapters while it records",
            Self::ScreenVocabulary => {
                "Names in the window you're dictating into help Hex spell them. Never saved"
            }
            Self::LearnFromCorrections => {
                "When you fix a name in text Hex just pasted, Hex adds the corrected spelling to Vocabulary. It watches only that field, for up to 5 minutes, and never sends it anywhere."
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V so terminals accept it",
            Self::KeyboardModelSwitching => {
                "When you switch keyboard language, Hex switches to the speech model you picked for it"
            }
            Self::ModelSwitchShortcut => {
                "One shortcut swaps dictation to another model and language, and pressing it again swaps back"
            }
            Self::MouseShortcuts => "Use a mouse button, such as a side button, as a shortcut",
            Self::PauseBrowser => {
                "While dictating with Pause media: YouTube and other tabs, resumed only if Hex paused them"
            }
            Self::ShareShortcut => {
                "Other apps see your dictation key too, so it can also be push-to-talk in apps like Discord"
            }
            Self::UnloadIdle => {
                "Frees the speech model's memory after a while unused; the next dictation reloads it"
            }
            Self::KeepMicrophoneOpen => {
                "With Turn off the mic when not dictating on, the microphone stays open for a while after each dictation, so the next one starts at once"
            }
            Self::KeepOnClipboard => {
                "Leave the dictation on the clipboard after pasting instead of restoring what was there"
            }
            Self::CopyOnly => "Put the dictation on the clipboard instead of pasting it",
            Self::ReturnToApp => {
                "If another app comes to the front while you dictate, Hex switches back to the app you started in before pasting. If it can't, nothing is pasted and Paste last dictation has the text."
            }
            Self::SpokenFormatting => {
                "Say \"new line\", \"new paragraph\", \"open quote\", or \"close quote\" to insert line breaks and quotation marks"
            }
            Self::FillerWords => {
                "Remove filler words such as um and uh from dictations, without an AI model"
            }
            Self::CleanUpWithAppleIntelligence => {
                "Fix clearly misheard words and things you correct out loud in each dictation, using Apple Intelligence on this Mac"
            }
            Self::NeutralVoice => {
                "Rewrite dictations in chosen modes into plain, neutral wording on this Mac, so your writing style doesn't show"
            }
            Self::SecondDictationShortcut => {
                "Another shortcut that dictates like the main one, such as a key on an external keyboard"
            }
            Self::TerminalPasteShortcut => {
                "Another dictation shortcut that always pastes with Ctrl+Shift+V"
            }
            Self::SmartSpacing => "Add a space before a dictation pasted right after a word",
            Self::SpokenSymbols => {
                "Say \"comma\", \"colon\", \"hashtag\", \"underscore\", or \"slash\" to insert the symbol joined to its words"
            }
            Self::PickerSortFilter => {
                "Sort the speech model list by size or name and show only models for your language"
            }
            Self::SidedChords => {
                "Tell left and right Option, Command, Shift, and Control apart, so Right Command + Right Option doesn't trigger from the left keys"
            }
            Self::TapOrHold => {
                "In Toggle mode, hold the shortcut to dictate until you let go; a short tap still starts and stops"
            }
            Self::DoubleTapModifiers => {
                "In Double-tap mode, tap a shortcut made only of Option, Command, Shift, Control, or Globe, with no letter or number key, twice to dictate"
            }
            Self::BenchmarkDetails => {
                "Show each speech model's error rate, language, and how it was tested, in the model list"
            }
            Self::ParakeetUltra => {
                "Offer Parakeet Ultra, a tuned Parakeet v3 from Moondream, in the speech model list"
            }
            Self::StandaloneSpecialKeys if cfg!(target_os = "windows") => {
                "Use Insert, Home, End, Page Up, or Page Down as a shortcut by itself"
            }
            Self::StandaloneSpecialKeys => {
                "Use Menu, Pause, or Scroll Lock as a shortcut by itself, such as Caps Lock remapped to Menu"
            }
            Self::HistoryStats => {
                "Add a Stats button to History that summarizes the dictations you kept, with time saved against your typing speed, the hour you dictate most, a year-long heatmap, and all-time totals kept beyond History's retention (counts only, no text). The hex history stats command prints it too"
            }
            Self::HistoryStatsStrip => {
                "Show your dictation totals in a compact strip at the top of History. Needs Dictation stats."
            }
            Self::RecentDictations => {
                "Copy one of your last five dictations from the menu bar, under Paste Last Dictation"
            }
            Self::SkipPauses => {
                "Shorten long pauses before the recording is turned into text, so slow computers paste sooner"
            }
            Self::OutputFixes => {
                "Offer Vocabulary and Fix this beside Output's Try result, to correct a wrong result"
            }
            Self::ModelInPlace => {
                "Add a speech model file without copying it, so several accounts can share one file"
            }
            Self::KQuantModels => {
                "Add your own speech model files saved in smaller formats (Q6_K, Q5_K, or Q4_K). They take less space, but Hex hasn't checked them, so accuracy may vary."
            }
            Self::PickerDownloaded => {
                "Show only downloaded speech models in the list, with the space they take"
            }
            Self::SectionKeyAlone => {
                "Use the key left of 1 on ISO keyboards, § or a dead key such as ^, alone as a shortcut; Hex takes it from every app while it's bound"
            }
            Self::MicrophoneKeyAlone => {
                "Use the mic key in F5's spot on Mac laptops as a shortcut; once set, only Hex gets it"
            }
            Self::KeepRecordings => {
                "Keep the audio of your dictations in History, so you can turn them into text again with another speech model"
            }
            Self::Appearance => "Use light or dark for the main window instead of the system's",
            Self::HideMenuBarIcon => {
                "Hide Hex's icon from the menu bar. Shortcuts keep working; open Hex again from Finder or Spotlight for Settings"
            }
            Self::GnomeClipboardRestore => {
                "Put back what you copied before a dictation, through XWayland. Works only while XWayland is already running"
            }
            Self::TypedText => {
                "Add a shortcut that runs your Output steps on text you typed and replaces it."
            }
            Self::NotchIndicator => {
                "On a screen with a notch, the recording indicator grows out of the notch instead of sitting below the menu bar"
            }
            Self::GnomeMeetingCapsules => {
                "On GNOME, float the recording offer and the small recording indicator over your call. Works only while XWayland is already running"
            }
            Self::GnomeAnnotate => {
                "On GNOME, show Annotate's recording controls and drawing over your screen. Works only while XWayland is already running; Esc can't stop drawing there, so use the pen button or let go of the Annotate key"
            }
            Self::AnnotatePointerScreen => {
                "On Wayland, a capture records the monitor you point at, and each monitor remembers its own screen"
            }
            Self::Screenshots => {
                "Press ⌘⇧4 to capture an area, or press Space for a window. The shot goes on the clipboard and into a folder"
            }
            Self::AnnotateReplay => {
                "Save the last 15 seconds of your screen. macOS shows the screen-recording dot the whole time it's on"
            }
            Self::MeetingImport => {
                "Add an audio file to Meetings and turn it into text on this Mac"
            }
            Self::TranscriptEdits => {
                "Select lines in a saved meeting transcript and delete them. Summaries, copies, and the Markdown file use what's left"
            }
            Self::CapsLockDictation => {
                "Press Caps Lock to start dictating and again to finish. Hex keeps Caps Lock off and uses its light while it records and writes the text"
            }
            Self::OpenCodeInWsl => {
                "Use OpenCode running in your default WSL distribution for rewriting in Modes and the Modes model list. Looking for it can start WSL"
            }
            Self::ApiProcessing => {
                "Apps that use Hex's local API can ask for text with Global mode's replacements, corrections, rewriting, and transformations applied"
            }
            Self::ApiTimestamps => {
                "Apps that use Hex's local API can ask when each word or phrase starts and ends"
            }
            Self::SpeakersOnly => {
                "Apply \"While dictating\" only when sound plays through speakers. Headphones are left alone."
            }
            Self::MoreDictationShortcuts => {
                "Up to four more shortcuts that each dictate like the main one, each with its own mode"
            }
            Self::PauseMenu => {
                "Add Pause Hex to the menu bar. While paused, Hex ignores its shortcuts so other apps get the keys"
            }
            Self::CalendarMenu => {
                "Show your next calendar meetings in the menu bar menu. Choosing one with a call link opens it; that click doesn't start a recording."
            }
            Self::CalendarSelectAll => {
                "Adds a button to the Calendars row to include or leave out every calendar at once."
            }
            Self::VoicePrompt => {
                "Say \"rewrite this\" and then what you want; Hex redoes the text with the mode's AI model"
            }
            Self::CursorContext => {
                "Rewrite uses the text around the cursor to spell names and terms right and fit where it pastes."
            }
            Self::CopyCaptureZip => {
                "A finished Annotate capture copies as one zip file, without the video or audio"
            }
            Self::YoloMode => {
                "Press Return after a dictation is pasted, so the message sends itself"
            }
            Self::CopyDictationAudio => {
                "Copy a kept dictation's audio as a file from History, the menu bar, or the tray; needs Keep recordings"
            }
            Self::StopPhrase => {
                "Say a phrase to finish a hands-free dictation; the phrase isn't added to the text"
            }
            Self::HoldThroughKeys if cfg!(target_os = "windows") => {
                "If a shortcut is made only of Ctrl, Alt, Shift, or the Windows key, with no letter or number key, pressing another key or clicking after the first second no longer ends the dictation; let go to finish"
            }
            Self::HoldThroughKeys => {
                "If a shortcut is made only of Option, Command, Shift, Control, or Globe, with no letter or number key, pressing another key or clicking after the first second no longer ends the dictation; let go to finish"
            }
            Self::ModesEditor => {
                "Edit each mode's apps, corrections and OpenCode rewrite in a Modes pane"
            }
            Self::DropTranscribe => {
                "Drop a WAV, MP3, AAC, M4A, MP4, MOV, AIFF, CAF, FLAC, OGG, Opus, WebM, or MKV file on the Hex window to save its transcript as a text file next to it"
            }
            Self::PasteLastPicker => {
                "Tap Paste Last to paste the newest dictation or annotation; hold it to pick another"
            }
            Self::VocabularyFiles => {
                "Import terms from a CSV file in the Vocabulary sheet, or save the whole list as one"
            }
            Self::VocabularyMenu => {
                "Add terms to Vocabulary from the menu bar, or open the Vocabulary sheet"
            }
            Self::AnnotateAgentTools => {
                "Adds hex annotate zoom, inspect, frames, and skill so coding agents can read a capture closely"
            }
            Self::AnnotateAgentInstall => {
                "Adds an Agents section to the Annotate pane that writes the hex-annotate skill into Claude Code, Codex, OpenCode, or Pi, and on macOS links hex onto PATH"
            }
            Self::Signature => "Add a line to the end of each dictation, such as \"Sent from Hex\"",
            Self::DictationStyle => {
                "Format every dictation as Polished, Casual, Notes, or Verbatim, like the iPhone app, without an AI model"
            }
            Self::HudHeight => {
                "Choose how high the dictation HUD sits above the Dock when it's at the bottom"
            }
            Self::VoiceIsolation => {
                "Record dictation through Apple's voice processing so Mic Mode can turn on Voice Isolation and cut background noise."
            }
            Self::DictateWhileAnnotating => {
                "Dictate while a capture records: the text pastes as usual, and the capture keeps going"
            }
            Self::StopAnyCallRecording if !cfg!(target_os = "macos") => {
                "Also stop an offered or by-hand call recording 30 s after its call app leaves the microphone, even with Stop when the call ends off"
            }
            Self::StopAnyCallRecording => {
                "Also stop an offered, calendar, or by-hand call recording 30 s after its call app leaves the microphone, even with Stop when the call ends off"
            }
            Self::ClipboardPhrase => {
                "Say \"my clipboard\" while dictating and Hex pastes what you copied there, in quotes"
            }
            Self::MeetingMicrophoneMute => {
                "Adds a mute button to meeting recordings. Hex keeps recording the call's audio but leaves out your microphone until you unmute."
            }
            Self::MeetingIndicatorDisplay => {
                "Drag the meeting recording indicator to another display and it stays there"
            }
        }
    }
}
