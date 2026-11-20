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
            Self::FailureNotices => {
                "When a dictation fails, show the reason next to the small recording indicator (Dictation HUD)"
            }
            Self::LiveTyping => {
                "On Wayland, show your words in the text box as you speak, then Hex types the text there"
            }
            Self::LiveChapters => "Split a meeting's text into chapters while it records",
            Self::VocabularyBiasing => {
                "Help Parakeet-based speech models hear your Vocabulary words; rewriting and search use them anyway"
            }
            Self::ScreenVocabulary => {
                "Names in the window you're dictating into help Hex spell them. Never saved. Parakeet needs Vocabulary help, and a dictation with terms runs after release instead of from the live stream."
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V, for apps that ignore Ctrl+V",
            Self::KeyboardModelSwitching => {
                "Changing the keyboard language switches to the speech model you picked for it"
            }
            Self::AppModelSwitching => {
                "Each dictation mode's speech model and language follow the app in front"
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
                "Use Insert, Home, End, Page Up, or Page Down as a shortcut by itself"
            }
            Self::StandaloneSpecialKeys => {
                "Use Menu, Pause, or Scroll Lock as a shortcut by itself, such as Caps Lock remapped to Menu"
            }
            Self::HistoryStats => {
                "Add a Stats button to History that summarizes the dictations you kept, with time saved against a typing speed, the local hour you dictate most, a year-long heatmap, and all-time totals from a daily rollup in usage.json (counts only, no text or app names) that outlives History's retention; off, the rollup is never written or read; the hex history stats command prints it too"
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
            Self::PickerActiveFirst => "The speech model list shows the model you're using first",
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
            Self::ReleaseNotesLink => {
                "Show a link to what changed in this version at the bottom of the sidebar"
            }
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
            Self::QuitClosesWindow => {
                "⌘Q closes the Settings window instead of quitting Hex; dictation keeps running, and the menu bar icon still quits"
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
            Self::ApiBrowserAccess => {
                "Let web pages from the addresses you list call Hex's local API from the browser; every request still needs the API token"
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
            Self::PrintStart => {
                "Let hex dictation start --print hand the transcript to the command that started it, however the recording ends"
            }
            Self::VoicePrompt => {
                "Say \"rewrite this\" and then what you want; Hex redoes the text with the mode's AI model"
            }
            Self::CursorContext => {
                "The mode's OpenCode rewrite uses the text around the cursor to spell names and terms right and fit where it pastes. That text goes to the mode's model, which may be a cloud provider; password fields are never read"
            }
            Self::ShellCompletions => {
                "Print a script for bash, zsh, fish, elvish, or PowerShell that completes hex commands, with hex completions <shell>"
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
            Self::HoldThroughKeys if cfg!(target_os = "windows") => {
                "If a shortcut is made only of Ctrl, Alt, Shift, or the Windows key, with no letter or number key, pressing another key or clicking after the first second no longer ends the dictation; let go to finish. Before that first second a chord still ends it, and a shortcut with a key is unchanged"
            }
            Self::HoldThroughKeys => {
                "If a shortcut is made only of Option, Command, Shift, Control, or Globe, with no letter or number key, pressing another key or clicking after the first second no longer ends the dictation; let go to finish. Before that first second a chord still ends it, and a shortcut with a key is unchanged"
            }
            Self::ModesEditor => {
                "Edit each mode's apps and websites, corrections, OpenCode rewrite (on or off, and its model), and transformations in a Modes pane; Output stays the read-only view"
            }
        }
    }
}
