//! The English one-line description of each Labs feature, split from
//! [`super::Feature`] to keep its catalog readable.

use super::Feature;

impl Feature {
    /// The English one-line description the CLI prints; the window shows
    /// its translated row.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Meetings => "Record calls and keep transcripts (its own pane)",
            Self::VoiceFilter => {
                "Hex Microphone virtual input and real-time voice filters (its own pane)"
            }
            Self::Playground => "Scene and story browser in the window",
            Self::LiveText => "Show provisional words beside the HUD while dictating",
            Self::FailureNotices => "Show why a dictation failed beside the HUD",
            Self::LiveTyping => {
                "On Wayland, show your words in the focused text field as you speak and type the final text there"
            }
            Self::LiveChapters => "Break meeting transcripts into chapters while recording",
            Self::VocabularyBiasing => {
                "Help the Parakeet model hear your Vocabulary words; rewriting and search use them anyway"
            }
            Self::ScreenVocabulary => {
                "Names in the window you're dictating into help Hex spell them. Never saved. Parakeet needs Vocabulary biasing, and a dictation with terms runs after release instead of from the live stream."
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V, for apps that ignore Ctrl+V",
            Self::KeyboardModelSwitching => {
                "Changing the keyboard language switches to the speech model you picked for it"
            }
            Self::AppModelSwitching => {
                "Each dictation mode's speech model and language follow the app in front"
            }
            Self::MouseShortcuts => "Record mouse buttons as shortcuts",
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
                "Fix clearly misheard words and spoken self-corrections in each dictation, on this Mac with Apple Intelligence"
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
                "Sort the Local transcription picker by size or name and show only models for the saved language"
            }
            Self::SidedChords => {
                "Tell left and right modifiers apart, so Right Command + Right Option doesn't fire on the left keys"
            }
            Self::TapOrHold => {
                "In Toggle mode, hold the shortcut to dictate until you let go; a short tap still starts and stops"
            }
            Self::DoubleTapModifiers => {
                "Double-tap shortcut mode also accepts a shortcut with no key, such as Option, Globe, or ⌃⌥: tap twice to dictate"
            }
            Self::BenchmarkDetails => {
                "Show each model's raw error rate, benchmark, and source in the Local transcription picker"
            }
            Self::ParakeetUltra => {
                "Offer Parakeet Ultra, Moondream's post-trained Parakeet v3, in the Local transcription picker"
            }
            Self::StandaloneSpecialKeys if cfg!(target_os = "windows") => {
                "Use Insert, Home, End, Page Up, or Page Down alone as a shortcut"
            }
            Self::StandaloneSpecialKeys => {
                "Use Menu, Pause, or Scroll Lock alone as a shortcut, such as Caps Lock remapped to Menu"
            }
            Self::HistoryStats => {
                "Add a Stats button to History that shows a summary of your retained dictations, with hex history stats"
            }
            Self::HistoryMultiSelect => {
                "Shift-click, Cmd-click (Ctrl on Windows and Linux) or drag to select several dictations, then copy or delete them together"
            }
            Self::RecentDictations => {
                "Copy one of your last five dictations from the menu bar, under Paste Last Dictation"
            }
            Self::SkipPauses => {
                "Shorten long pauses before a whole dictation goes to the model, so slow computers paste sooner"
            }
            Self::OutputFixes => {
                "Offer Vocabulary and Fix this beside Output's Try result, to correct a wrong result"
            }
            Self::PickerActiveFirst => "The speech model picker lists the model you're using first",
            Self::ModelInPlace => {
                "Add a speech model file and use it where it is, without copying it, so several accounts can share one file"
            }
            Self::KQuantModels => {
                "Add your own speech model files that use Q6_K, Q5_K, or Q4_K weights. They are smaller than Q8_0 but Hex hasn't verified them, so accuracy may vary."
            }
            Self::PickerDownloaded => {
                "Show only downloaded speech models in the picker, with the space they take"
            }
            Self::SectionKeyAlone => {
                "Use the key left of 1 on ISO keyboards, § or a dead key such as ^, alone as a shortcut; Hex takes it from every app while it's bound"
            }
            Self::MicrophoneKeyAlone => {
                "Use the mic key in F5's spot on a Mac laptop keyboard as a shortcut; once set, Hex takes the key from other apps. If macOS's own Dictation still opens, change macOS's dictation shortcut in System Settings › Keyboard › Dictation."
            }
            Self::KeepRecordings => {
                "Keep the audio of dictations saved to History, so you can transcribe them again with another model"
            }
            Self::SettingsSearch => "Find any setting in General with a search field",
            Self::Appearance => "Use light or dark for the main window instead of the system's",
            Self::ReleaseNotesLink => {
                "Show a link to this build's release notes in the sidebar footer"
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
                "Show the meeting capsules (recording offer, recording indicator) floating over your call on GNOME Wayland, through XWayland while XWayland is running"
            }
            Self::GnomeAnnotate => {
                "Show Annotate's recording capsule and ink over your screen on GNOME Wayland, through XWayland while XWayland is running; Esc can't stop drawing there, so use the capsule's pen button or let go of a held Annotate key"
            }
            Self::Screenshots => {
                "Take a screenshot of an area or a window with a shortcut (Command-Shift-4 by default): copied, saved, and shown in the corner"
            }
            Self::CopyCaptureZip => {
                "A finished capture copies as one zip file, without video or audio."
            }
            Self::AnnotateReplay => {
                "Save the last 15 seconds of your screen with hex capture replay or capture.replay_hotkey; macOS shows the screen-recording dot the whole time it's on (macOS 27+)"
            }
            Self::MeetingImport => {
                "Import an audio file into Meetings and transcribe it on this Mac"
            }
            Self::QuitClosesWindow => {
                "⌘Q closes the Settings window instead of quitting Hex; dictation keeps running, and the menu bar icon still quits"
            }
            Self::CapsLockDictation => {
                "Press Caps Lock to start and stop dictation; its light shows when Hex records and transcribes"
            }
            Self::OpenCodeInWsl => {
                "Use an OpenCode service running in your default WSL distribution for Modes rewriting and the Modes model picker; looking for it can start WSL"
            }
            Self::ApiProcessing => {
                "Let apps using Hex's local API ask for transcripts with your Global mode's corrections, rewrite, and transformations applied"
            }
            Self::ApiTimestamps => {
                "Let apps using Hex's local API ask for each segment's or word's start and end times in the transcript, as the speech model heard them"
            }
            Self::ApiBrowserAccess => {
                "Let web pages from the origins you list call Hex's local API from the browser; every request still needs the API token"
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
            Self::PrintStart => {
                "Let hex dictation start --print hand the transcript to the command that started it, however the recording ends"
            }
            Self::VoicePrompt => {
                "Say \"rewrite this\" in a dictation, then an instruction; Hex applies it with the mode's OpenCode model"
            }
            Self::CursorContext => {
                "The mode's OpenCode rewrite uses the text around the cursor to spell names and terms right and fit where it pastes. That text goes to the mode's model, which may be a cloud provider; password fields are never read"
            }
            Self::ShellCompletions => {
                "Print a completion script for bash, zsh, fish, elvish, or PowerShell with hex completions <shell>"
            }
            Self::YoloMode => "Send dictations automatically",
            Self::CopyDictationAudio => {
                "History entries with a kept recording gain Copy audio, and the menu bar or tray gains Copy last dictation audio: one file on the clipboard, ready to attach. Needs Keep recordings, and copies the kept WAV without storing any audio itself"
            }
            Self::StopPhrase => {
                "Say a phrase at the end of a hands-free dictation to finish it; the phrase never reaches the pasted text"
            }
            Self::HoldThroughKeys => {
                "Keys and clicks a second or more into a held modifier-only shortcut reach the app in front and no longer end the dictation, which the release then finishes as usual. Before that first second a chord still ends it, and a shortcut with a key is unchanged"
            }
        }
    }
}
