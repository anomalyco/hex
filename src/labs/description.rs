//! The English one-line description of each Labs feature, split from
//! [`super::Feature`] to keep its catalog readable.

use super::Feature;

impl Feature {
    /// The English one-line description the CLI prints; the window shows
    /// its translated row.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Meetings => "Record calls and keep a written copy (its own pane)",
            Self::VoiceFilter => {
                "A Hex Microphone other apps can pick, with live voice filters (its own pane)"
            }
            Self::Playground => "Browse scenes and stories in a window",
            Self::LiveText => {
                "Show your words next to the small recording indicator (Dictation HUD) as you dictate, before the text is pasted"
            }
            Self::FasterLiveText => {
                "Show live words sooner, and paste the text from the same shorter lookahead, at a little more GPU work"
            }
            Self::FailureNotices => {
                "When a dictation fails, show the reason next to the small recording indicator (Dictation HUD)"
            }
            Self::LiveTyping => {
                "On Wayland, show your words in the text box as you speak, then Hex types the text there"
            }
            Self::LiveChapters => "Split a meeting's text into chapters while it records",
            Self::ScreenVocabulary => {
                "Names in the window you're dictating into help Hex spell them. Never saved. Parakeet needs Vocabulary help, and a dictation with terms runs after release instead of from the live stream."
            }
            Self::LearnFromCorrections => {
                "When you fix a name in text Hex just pasted, add the corrected spelling to Vocabulary; Hex watches only that field, for up to 5 minutes, and sends nothing anywhere"
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V, for apps that ignore Ctrl+V",
            Self::KeyboardModelSwitching => {
                "Changing the keyboard language switches to the speech model you picked for it"
            }
            Self::MouseShortcuts => "Use a mouse button, such as a side button, as a shortcut",
            Self::PauseBrowser => "Also pause the browser while dictating, with Pause media",
            Self::ShareShortcut => "Let apps such as Discord see the dictation shortcut too",
            Self::UnloadIdle => {
                "Free the speech model's memory after a while unused (labs.unload_idle_minutes); the next dictation reloads it"
            }
            Self::KeepMicrophoneOpen => {
                "With Turn off the mic when not dictating on, keep the microphone open for a while after each dictation (labs.keep_microphone_open_seconds), so the next one starts at once"
            }
            Self::KeepOnClipboard => {
                "Leave the dictation on the clipboard after pasting instead of restoring what was there"
            }
            Self::CopyOnly => "Put the dictation on the clipboard instead of pasting it",
            Self::ReturnToApp => {
                "If another app comes to the front while you dictate, switch back to the app you started in before pasting"
            }
            Self::SpokenFormatting => {
                "Turn spoken \"new line\", \"new paragraph\", and \"open/close quote\" into line breaks and quotation marks"
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
            Self::ModelSwitchShortcut => {
                "One shortcut that swaps dictation to another model and language (model_switch_other), and back"
            }
            Self::SecondDictationShortcut => {
                "Another shortcut that dictates like the main one (second_dictation_hotkey), such as a key on an external keyboard"
            }
            Self::MoreDictationShortcuts => {
                "Up to four more shortcuts that dictate like the main one, each with its own binding and activation mode (extra_dictation_hotkeys)"
            }
            Self::TerminalPasteShortcut => {
                "Another dictation shortcut that always pastes with Ctrl+Shift+V (terminal_paste_hotkey)"
            }
            Self::SmartSpacing => "Add a space before a dictation pasted right after a word",
            Self::SpokenSymbols => {
                "Turn spoken \"comma\", \"colon\", \"hashtag\", \"underscore\", and similar words into symbols joined to their words"
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
                "Double-tap mode also accepts a shortcut made only of Option, Command, Shift, Control, or Globe, with no letter or number key: tap it twice to dictate"
            }
            Self::BenchmarkDetails => {
                "Show each speech model's error rate, language, and how it was tested, in the speech model list"
            }
            Self::ParakeetUltra => {
                "Offer Parakeet Ultra, a tuned Parakeet v3 from Moondream, in the speech model list"
            }
            Self::StandaloneSpecialKeys if cfg!(target_os = "windows") => {
                "Use Caps Lock, Insert, Home, End, Page Up, or Page Down as a shortcut by itself"
            }
            Self::StandaloneSpecialKeys => {
                "Use Menu, Pause, or Scroll Lock as a shortcut by itself, such as Caps Lock remapped to Menu"
            }
            Self::HistoryStats => {
                "Add a Stats button to History that summarizes the dictations you kept, with time saved against a typing speed, the local hour you dictate most, a year-long heatmap, and all-time totals from a daily rollup in usage.json (counts only, no text or app names) that outlives History's retention; off, the rollup is never written or read; the hex history stats command prints it too"
            }
            Self::HistoryStatsStrip => {
                "Show the six headline numbers the Dictation stats dialog summarizes as one compact strip above History's list; it needs Dictation stats and reads its numbers"
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
                "Add a speech model file and use it where it is, without copying it, so several accounts can share one file"
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
                "Use the mic key in F5's spot on a Mac laptop keyboard as a shortcut; once set, Hex takes the key from other apps. If macOS's own Dictation still opens, change macOS's dictation shortcut in System Settings › Keyboard › Dictation."
            }
            Self::KeepRecordings => {
                "Keep the audio of your dictations in History, so you can turn them into text again with another speech model"
            }
            Self::Appearance => "Use light or dark for the main window instead of the system's",
            Self::HideMenuBarIcon => {
                "Hide Hex's menu bar icon; shortcuts keep working, and opening Hex again shows Settings"
            }
            Self::GnomeClipboardRestore => {
                "Put the previous clipboard back after a paste on GNOME Wayland, through the XWayland clipboard while XWayland is running"
            }
            Self::TypedText => {
                "Run the focused app's Output steps on the text already typed in the focused field and replace it (typed_text_hotkey)"
            }
            Self::NotchIndicator => {
                "On a screen with a notch, the recording indicator grows out of the notch instead of sitting below the menu bar"
            }
            Self::GnomeMeetingCapsules => {
                "On GNOME, float the recording offer and the small recording indicator over your call, through XWayland while XWayland is running"
            }
            Self::AnnotatePointerScreen => {
                "On Wayland, a screen capture records the monitor the pointer is on, and each monitor keeps its own ScreenCast choice (Linux)"
            }
            Self::GnomeAnnotate => {
                "On GNOME, show Annotate's recording controls and drawing over your screen, through XWayland while XWayland is running; Esc can't stop drawing there, so use the pen button or let go of a held Annotate key"
            }
            Self::Screenshots => {
                "Take a screenshot of an area or a window with a shortcut (Command-Shift-4 by default): copied, saved, and shown in the corner"
            }
            Self::CopyCaptureZip => {
                "A finished Annotate capture copies as one zip file, without the video or audio."
            }
            Self::AnnotateReplay => {
                "Save the last 15 seconds of your screen with hex capture replay or capture.replay_hotkey; macOS shows the screen-recording dot the whole time it's on (macOS 27+)"
            }
            Self::MeetingImport => {
                "Add an audio file to Meetings and turn it into text on this Mac"
            }
            Self::TranscriptEdits => {
                "Select lines in a saved meeting transcript and delete them; summaries, copies, and the Markdown file use what's left"
            }
            Self::CapsLockDictation => {
                "Press Caps Lock to start and stop dictation; its light shows when Hex records and writes the text"
            }
            Self::OpenCodeInWsl => {
                "Use an OpenCode service running in your default WSL distribution for Modes rewriting and the Modes model picker; looking for it can start WSL"
            }
            Self::DropTranscribe => {
                "Drop a WAV, MP3, AAC, M4A, MP4, MOV, AIFF, CAF, FLAC, OGG, Opus, WebM, or MKV file on the Hex window to save its transcript as a text file next to it"
            }
            Self::PasteLastPicker => {
                "Tap Paste Last for the newest dictation or annotation, hold it to pick another, and see dictations, annotations, screenshots, and recordings together in History (macOS)"
            }
            Self::ApiProcessing => {
                "Let apps using Hex's local API ask for text with Global mode's replacements, corrections, rewriting, and transformations applied"
            }
            Self::ApiTimestamps => {
                "Let apps using Hex's local API ask when each word or phrase starts and ends, as the speech model heard them"
            }
            Self::SpeakersOnly => {
                "Apply \"While dictating\" only when sound plays through speakers; headphones are left alone"
            }
            Self::PauseMenu => {
                "Add Pause Hex to the menu bar. While paused, Hex ignores its shortcuts so other apps get the keys"
            }
            Self::CalendarMenu => {
                "Show your next calendar meetings in the menu bar menu. Choosing one with a call link opens it; that click doesn't start a recording (macOS, with Meetings on and a calendar connected)"
            }
            Self::CalendarSelectAll => {
                "Add a Select all or Deselect all button beside the Calendars row in Meetings settings, so every calendar is included or left out at once (macOS)"
            }
            Self::VoicePrompt => {
                "Say \"rewrite this\" and then what you want; Hex redoes the text with the mode's AI model"
            }
            Self::CursorContext => {
                "The mode's OpenCode rewrite uses the text around the cursor to spell names and terms right and fit where it pastes. That text goes to the mode's model, which may be a cloud provider; password fields are never read"
            }
            Self::YoloMode => {
                "Press Return after a dictation is pasted, so the message sends itself"
            }
            Self::CopyDictationAudio => {
                "History entries with a kept recording gain Copy audio, and the menu bar or tray gains Copy last dictation audio: one file on the clipboard, ready to attach. Needs Keep recordings, and copies the kept audio without storing any itself"
            }
            Self::StopPhrase => {
                "Say a phrase at the end of a hands-free dictation to finish it; the phrase never reaches the pasted text"
            }
            Self::ConfirmLongCancels => {
                "Once a dictation has recorded for 10 seconds, the first press of its cancel shortcut keeps recording and asks, beside the HUD, for a second press; a second press within 3 seconds throws the dictation away. A shorter dictation still cancels at once"
            }
            Self::HoldThroughKeys if cfg!(target_os = "windows") => {
                "If a shortcut is made only of Ctrl, Alt, Shift, or the Windows key, with no letter or number key, pressing another key or clicking after the first second no longer ends the dictation; let go to finish. Before that first second a chord still ends it, and a shortcut with a key is unchanged"
            }
            Self::HoldThroughKeys => {
                "If a shortcut is made only of Option, Command, Shift, Control, or Globe, with no letter or number key, pressing another key or clicking after the first second no longer ends the dictation; let go to finish. Before that first second a chord still ends it, and a shortcut with a key is unchanged"
            }
            Self::ModesEditor => {
                "Edit each mode's apps and websites, corrections, OpenCode rewrite (on or off, and its model), and transformations in a Modes pane; Output stays the read-only view"
            }
            Self::VocabularyFiles => {
                "Import terms from a CSV file into the vocabulary sheet, merging them into the list, and export the list as a CSV file"
            }
            Self::VocabularyMenu => {
                "Add a Vocabulary submenu to the menu bar, right under Speech Model: add a term or open the Vocabulary sheet, with the last five saved terms listed (macOS)"
            }
            Self::AnnotateAgentTools => {
                "Adds hex annotate zoom, inspect, frames, and skill so coding agents can read a capture closely"
            }
            Self::AnnotateAgentInstall => {
                "Adds an Agents section to the Annotate pane: one checked row per coding agent writes the hex-annotate skill into its own skills folder, a row copies the npx skills add command, and on macOS the Command-line tool row links hex onto PATH (/opt/homebrew/bin, /usr/local/bin, else ~/.local/bin) without replacing a hex another program owns"
            }
            Self::Signature => {
                "Each normal dictation pastes with your signature on its own line at the end, and Paste Last and History keep the same text; not added to raw, printed, or API output"
            }
            Self::DictationStyle => {
                "Write each dictation as Polished, Casual, Notes, or Verbatim after the corrections and rewrite and before transformations; Dictate without processing, Voice Action, meetings, and the local API are left alone"
            }
            Self::HudHeight => {
                "Choose how high the dictation HUD sits above the Dock or the screen's bottom edge when its position is Bottom (labs.hud_bottom_gap, macOS)"
            }
            Self::IndicatorFollowsMouse => {
                "While a dictation records and processes, the recording indicator follows the mouse pointer: just below it, above it near the screen's bottom, kept inside the screen's visible area, with live text beside it; Hidden still hides it, and turning it off restores the Top or Bottom position (macOS)"
            }
            Self::VoiceIsolation => {
                "Open dictation's microphone through Apple's voice processing, so Control Center's Mic Mode can offer Voice Isolation and background noise is reduced (macOS; falls back to normal capture when voice processing won't start, and may lower other audio)"
            }
            Self::StopAnyCallRecording if !cfg!(target_os = "macos") => {
                "An offered or by-hand call recording stops on its own 30 s after its call app leaves the microphone, as Always record's Stop when the call ends does, which still governs Always record; a recording started by hand watches the one call app holding the microphone when it starts"
            }
            Self::StopAnyCallRecording => {
                "An offered, calendar, or by-hand call recording stops on its own 30 s after its call app leaves the microphone, as Always record's Stop when the call ends does, which still governs Always record; a recording started by hand watches the one call app holding the microphone when it starts"
            }
            Self::ClipboardPhrase => {
                "Each time you say the phrase in a normal dictation (\"my clipboard\" by default, at least two words), Hex puts the clipboard's text there in double quotes after processing and before it pastes, and Paste Last and History keep the same text. An empty clipboard leaves the words as spoken; raw and API output are left alone"
            }
            Self::MeetingMicrophoneMute => {
                "Mute Hex's microphone while a meeting records: the call's audio keeps recording and transcribing, and the microphone's packets stay in place as silence until you unmute (macOS)"
            }
            Self::MeetingIndicatorDisplay => {
                "Drag the meeting recording indicator to another display and it stays there, in the corner nearest where you let go. Off, it stays on the primary display (macOS and Windows)"
            }
            Self::MeetingAudioSource => {
                "Record one app or one output device in a meeting instead of all computer audio (labs.meeting_audio_source_choice); a chosen source that isn't playing records silence until it returns"
            }
            Self::MiddleMouseDictation => {
                "Hold the middle mouse button for half a second to start a hands-free dictation and long-press again to finish; the dictation shortcut can finish it too, and a long press finishes a hands-free dictation the shortcut started. Clicks still reach the app, and Escape cancels a hands-free dictation and stays out of the app (macOS)"
            }
        }
    }
}
