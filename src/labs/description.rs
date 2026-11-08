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
                "Show provisional words in the focused text field while dictating on Wayland, then type the final text there"
            }
            Self::LiveChapters => "Break meeting transcripts into chapters while recording",
            Self::VocabularyBiasing => {
                "Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way"
            }
            Self::TerminalPaste => "Paste with Ctrl+Shift+V, for apps that ignore Ctrl+V",
            Self::KeyboardModelSwitching => {
                "Changing the keyboard language switches the speech model and language"
            }
            Self::MouseShortcuts => "Record mouse buttons as shortcuts",
            Self::PauseBrowser => "Also pause the browser while dictating, with Pause media",
            Self::ShareShortcut => "Let apps such as Discord see the dictation shortcut too",
            Self::UnloadIdle => {
                "Free the speech model's memory after a while unused (labs.unload_idle_minutes); the next dictation reloads it"
            }
            Self::RestoreShortcutDefault => {
                "Offer Restore default next to Paste last dictation's shortcut"
            }
            Self::KeepOnClipboard => {
                "Leave the dictation on the clipboard after pasting instead of restoring what was there"
            }
            Self::CopyOnly => "Put the dictation on the clipboard instead of pasting it",
            Self::SpokenFormatting => {
                "Turn spoken \"new line\", \"new paragraph\", and \"open/close quote\" into line breaks and quotation marks"
            }
            Self::FillerWords => {
                "Remove filler words such as um and uh from dictations, without an AI model"
            }
            Self::SubtitleExport => {
                "Export meeting transcripts as SRT or VTT subtitles with hex meetings show"
            }
            Self::SecondDictationShortcut => {
                "Another shortcut that dictates like the main one (second_dictation_hotkey), such as a key on an external keyboard"
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
                "Record which side each modifier of a shortcut is on, such as Right Command + Right Option"
            }
            Self::TapOrHold => {
                "In Toggle mode, hold the shortcut to dictate until you let go; a short tap still starts and stops"
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
            Self::RecentDictations => {
                "Copy one of your last five dictations from the menu bar, under Paste Last Dictation"
            }
            Self::SkipPauses => {
                "Shorten long pauses before a whole dictation goes to the model, so slow computers paste sooner"
            }
            Self::StopWhenCallEnds => {
                "Stop a meeting recording Hex started on its own 30 s after the call app leaves the microphone and goes quiet"
            }
            Self::OutputFixes => {
                "Offer Vocabulary and Fix this beside Output's Try result, to correct a wrong result"
            }
            Self::PickerActiveFirst => "The speech model picker lists the model you're using first",
            Self::ModelInPlace => {
                "Add a speech model file and use it where it is, without copying it, so several accounts can share one file"
            }
            Self::PickerDownloaded => {
                "Show only downloaded speech models in the picker, with the space they take"
            }
            Self::SectionKeyAlone => {
                "Use the § key, left of 1 on ISO keyboards, alone as a shortcut; Hex takes § from every app while it's bound"
            }
            Self::KeepRecordings => {
                "Keep the audio of dictations saved to History, so you can transcribe them again with another model"
            }
            Self::SettingsSearch => "Find any setting in General with a search field",
            Self::HideMenuBarIcon => {
                "Hide Hex's menu bar icon; shortcuts keep working, and opening Hex again shows Settings"
            }
            Self::MovableMeetingCapsules => {
                "Drag meeting capsules such as Meeting transcript ready out of the way; they come back in the corner you left them"
            }
            Self::GnomeClipboardRestore => {
                "Put the previous clipboard back after a paste on GNOME Wayland, through the XWayland clipboard while XWayland is running"
            }
            Self::TypedText => {
                "Run the focused app's Output steps on the text already typed in the focused field and replace it (typed_text_hotkey)"
            }
            Self::MeetingNotesVocabulary => {
                "Meeting titles and summaries read your vocabulary, so names are spelled the way you wrote them"
            }
            Self::NotchIndicator => {
                "On a screen with a notch, the recording indicator grows out of the notch instead of sitting below the menu bar"
            }
            Self::MeetingFinishedEvent => {
                "Tell apps on this computer when a meeting's transcript is ready, through the local API's GET /meetings/events (the meeting's id only)"
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
            Self::FollowLiveTranscript => {
                "While a meeting records, the transcript scrolls to the newest line. Scroll up to read earlier text; scroll back to the bottom to follow again."
            }
            Self::MoreCallApps => {
                "Offer to record calls in Signal, Telegram, WeChat, LINE, Webex, Element and the Phone app, and name calls on more websites."
            }
            Self::AnnotateReplay => {
                "Save the last 15 seconds of your screen with hex capture replay or capture.replay_hotkey; macOS shows the screen-recording dot the whole time it's on (macOS 27+)"
            }
            Self::MeetingImport => {
                "Import an audio file into Meetings and transcribe it on this Mac"
            }
            Self::MeetingDescriptiveFilenames => {
                "Rename exported meeting Markdown to the meeting's title once notes or a rename provide one, keeping the date and time"
            }
            Self::QuitClosesWindow => {
                "⌘Q closes the Settings window instead of quitting Hex; dictation keeps running, and the menu bar icon still quits"
            }
            Self::ExpireMeetingAudio => {
                "Choose how long to keep meeting audio. Transcripts, notes, and summaries follow Keep meetings."
            }
            Self::CapsLockDictation => {
                "Press Caps Lock to start and stop dictation; its light shows when Hex records and transcribes"
            }
            Self::PersistentInk => "Ink stays on screen until you press Escape or stop recording",
        }
    }
}
