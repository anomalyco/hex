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
            Self::FailureNotices => "Failure notices",
            Self::LiveTyping => "Live typing",
            Self::LiveChapters => "Live chapters",
            Self::VocabularyBiasing => "Vocabulary biasing",
            Self::ScreenVocabulary => "Screen vocabulary",
            Self::TerminalPaste => "Terminal paste",
            Self::KeyboardModelSwitching => "Switch model with keyboard language",
            Self::AppModelSwitching => "Switch model with app",
            Self::MouseShortcuts => "Mouse shortcuts",
            Self::PauseBrowser => "Also pause the browser",
            Self::ShareShortcut => "Share dictation shortcut with other apps",
            Self::UnloadIdle => "Unload model when idle",
            Self::KeepMicrophoneOpen => "Keep mic open after dictating",
            Self::KeepOnClipboard => "Keep dictation on clipboard",
            Self::CopyOnly => "Copy dictation only",
            Self::ReturnToApp => "Return to app",
            Self::SpokenFormatting => "Spoken formatting",
            Self::FillerWords => "Remove filler words",
            Self::CleanUpWithAppleIntelligence => "Clean up with Apple Intelligence",
            Self::SecondDictationShortcut => "Second dictation shortcut",
            Self::TerminalPasteShortcut => "Terminal paste shortcut",
            Self::SmartSpacing => "Smart spacing",
            Self::SpokenSymbols => "Spoken symbols",
            Self::PickerSortFilter => "Sort and filter models",
            Self::SidedChords => "Sided modifier chords",
            Self::TapOrHold => "Tap or hold",
            Self::DoubleTapModifiers => "Double-tap on a lone modifier",
            Self::BenchmarkDetails => "Benchmark details",
            Self::ParakeetUltra => "Parakeet Ultra",
            Self::StandaloneSpecialKeys => "Special keys alone",
            Self::HistoryStats => "Dictation stats",
            Self::HistoryMultiSelect => "Select multiple dictations",
            Self::RecentDictations => "Copy recent dictations",
            Self::SkipPauses => "Skip long pauses",
            Self::OutputFixes => "Fix results in Output",
            Self::PickerActiveFirst => "Model in use first",
            Self::ModelInPlace => "Use model files where they are",
            Self::KQuantModels => "Smaller model files",
            Self::PickerDownloaded => "Downloaded models only",
            Self::SectionKeyAlone => "Section key alone",
            Self::MicrophoneKeyAlone => "Microphone key alone",
            Self::KeepRecordings => "Keep recordings",
            Self::SettingsSearch => "Settings search",
            Self::Appearance => "Appearance",
            Self::ReleaseNotesLink => "Release notes link",
            Self::HideMenuBarIcon => "Hide menu bar icon",
            Self::GnomeClipboardRestore => "Restore clipboard on GNOME",
            Self::TypedText => "Transform typed text",
            Self::NotchIndicator => "Notch indicator",
            Self::GnomeMeetingCapsules => "Floating meeting capsules on GNOME",
            Self::GnomeAnnotate => "Annotate on GNOME",
            Self::Screenshots => "Screenshots",
            Self::AnnotateReplay => "Annotate 15 s replay",
            Self::MeetingImport => "Import audio",
            Self::QuitClosesWindow => "⌘Q closes Settings",
            Self::CapsLockDictation => "Caps Lock dictation",
            Self::OpenCodeInWsl => "OpenCode in WSL",
            Self::ApiProcessing => "Process API transcripts",
            Self::ApiTimestamps => "Timestamps in API transcripts",
            Self::ApiBrowserAccess => "Browser access to the local API",
            Self::SpeakersOnly => "Only on speakers",
            Self::MoreDictationShortcuts => "More dictation shortcuts",
            Self::PauseMenu => "Pause from menu bar",
            Self::CalendarMenu => "Upcoming meetings in menu bar",
            Self::PrintStart => "Print-only CLI dictation",
            Self::VoicePrompt => "Voice prompt",
            Self::CursorContext => "Rewrite reads nearby text",
            Self::CopyCaptureZip => "Copy captures as a zip",
            Self::ShellCompletions => "Shell completions",
            Self::YoloMode => "YOLO mode",
            Self::CopyDictationAudio => "Copy dictation audio",
            Self::StopPhrase => "Stop phrase",
            Self::HoldThroughKeys => "Hold through other keys",
        }
    }

    /// The card's one line, under its title.
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Meetings => "Record calls and keep transcripts",
            Self::VoiceFilter => "Hex Microphone virtual input and real-time voice filters",
            Self::Playground => "Scene and story browser in the window",
            Self::LiveText => {
                "Show your words beside the Dictation HUD as you speak, before the final text is pasted"
            }
            Self::FailureNotices => {
                "When a dictation fails, show the reason beside the Dictation HUD"
            }
            Self::LiveTyping => {
                "On Wayland, show your words in the text field as you speak and type the final text there"
            }
            Self::LiveChapters => "Break meeting transcripts into chapters while recording",
            Self::VocabularyBiasing => {
                "Help the Parakeet model hear your Vocabulary words. Rewriting and search use them anyway"
            }
            Self::ScreenVocabulary => {
                "Names in the window you're dictating into help Hex spell them. Never saved"
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V so terminals accept it",
            Self::KeyboardModelSwitching => {
                "When you switch keyboard language, Hex switches to the speech model you picked for it"
            }
            Self::AppModelSwitching => {
                "Each mode can pick a speech model; Hex switches to it when you change apps"
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
                "Fix clearly misheard words and spoken self-corrections in each dictation, on this Mac with Apple Intelligence"
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
                "Sort the Local transcription picker by size or name and show only models for the saved language"
            }
            Self::SidedChords => {
                "Tell left and right modifiers apart, so Right ⌘ + Right ⌥ doesn't fire on the left keys"
            }
            Self::TapOrHold => {
                "In Toggle mode, hold the shortcut to dictate until you let go; a short tap still starts and stops"
            }
            Self::DoubleTapModifiers => {
                "Let Double-tap mode use a modifier alone, like Option or Globe: tap twice to dictate"
            }
            Self::BenchmarkDetails => {
                "Show each model's raw error rate, benchmark, and source in the Local transcription picker"
            }
            Self::ParakeetUltra => {
                "Offer Parakeet Ultra, Moondream's post-trained Parakeet v3, in the Local transcription picker"
            }
            Self::StandaloneSpecialKeys if cfg!(target_os = "windows") => {
                "Use Insert, Home, End, Page Up, or Page Down as a shortcut without a modifier"
            }
            Self::StandaloneSpecialKeys => {
                "Use Menu, Pause, or Scroll Lock as a shortcut without a modifier, such as Caps Lock remapped to Menu"
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
                "Add a speech model file without copying it, so several accounts can share one file"
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
                "Use the mic key in F5's spot on Mac laptops as a shortcut; once set, only Hex gets it"
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
                "Hide Hex's icon from the menu bar. Shortcuts keep working; open Hex again from Finder or Spotlight for Settings"
            }
            Self::GnomeClipboardRestore => {
                "Put back what you copied before a dictation, through XWayland. Works only while XWayland is already running"
            }
            Self::TypedText => {
                "Adds a shortcut that runs your Output steps on text you typed and replaces it."
            }
            Self::NotchIndicator => {
                "On a screen with a notch, the recording indicator grows out of the notch instead of sitting below the menu bar"
            }
            Self::GnomeMeetingCapsules => {
                "Show the recording offer and recording indicator floating over your call, through XWayland. Works only while XWayland is already running"
            }
            Self::GnomeAnnotate => {
                "Show Annotate's recording capsule and ink over your screen, through XWayland. Works only while XWayland is already running; Esc can't stop drawing there, so use the capsule's pen button or let go of a held Annotate key"
            }
            Self::Screenshots => {
                "Press ⌘⇧4 to capture an area, or press Space for a window. The shot goes on the clipboard and into a folder"
            }
            Self::AnnotateReplay => {
                "Save the last 15 seconds of your screen. macOS shows the screen-recording dot the whole time it's on"
            }
            Self::MeetingImport => "Add an audio file to Meetings and transcribe it on this Mac",
            Self::QuitClosesWindow => {
                "⌘Q closes the Settings window instead of quitting Hex. Dictation keeps running; quit from the menu bar icon"
            }
            Self::CapsLockDictation => {
                "Press Caps Lock to start dictation and again to finish. Hex keeps Caps Lock off and uses its light while it records and transcribes"
            }
            Self::OpenCodeInWsl => {
                "Use an OpenCode service running in your default WSL distribution for Modes rewriting and the Modes model picker. Looking for it can start WSL."
            }
            Self::ApiProcessing => {
                "Apps can ask Hex's local API for transcripts with Global mode's processing applied."
            }
            Self::ApiTimestamps => {
                "Apps can ask Hex's local API for each segment's or word's start and end times."
            }
            Self::ApiBrowserAccess => {
                "Let web pages from these origins call Hex's local API. Requests still need the API token."
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
            Self::PrintStart => {
                "Let hex dictation start --print answer with the transcript when it ends"
            }
            Self::VoicePrompt => {
                "Say \"rewrite this\" in a dictation, then an instruction; Hex applies it with the mode's OpenCode model"
            }
            Self::CursorContext => {
                "Rewrite uses the text around the cursor to spell names and terms right and fit where it pastes."
            }
            Self::CopyCaptureZip => {
                "A finished capture copies as one zip file, without video or audio."
            }
            Self::ShellCompletions => {
                "Print a completion script for bash, zsh, fish, elvish, or PowerShell with hex completions <shell>"
            }
            Self::YoloMode => "Send dictations automatically",
            Self::CopyDictationAudio => {
                "Copy a kept WAV as a file from History and the menu bar or tray; needs Keep recordings"
            }
            Self::StopPhrase => "Say a phrase to finish a hands-free dictation",
            Self::HoldThroughKeys => {
                "Keys a second or more into a held modifier-only shortcut no longer end the dictation"
            }
        }
    }
}
