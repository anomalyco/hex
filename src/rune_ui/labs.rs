//! Labs: one sidebar item that opens a store of every Labs feature
//! (`crate::labs`), each off by default. Discover shows what's new, then
//! every lab as a card under its group's collapsible header, and Turned on
//! what's on. Each card shows its description and switch, and while on any
//! settings the lab has of its own. A feature with its own pane (Meetings,
//! Voice Filter, Playground) joins the main sidebar while on and is hidden
//! while off.

mod group;
#[cfg(all(target_os = "macos", hex_capture))]
mod screenshots;
mod store;

use rune::{El, Icon, TextEdit, Tx, row, text};

pub(super) use self::store::{page, search_field, tabs};
use super::controls::{self, Paint};
use super::icons::{BOOK, CAMERA, GLOBE, KEYBOARD, MOUSE, SPARKLES, TERMINAL, TIMER, WAVE};
use super::sidebar::{NAV_ROW_GAP, NAV_ROW_HEIGHT, VOICE_FILTER_ICON};
use super::{Pane, Ui};
use crate::desktop::i18n::{tr, tr_fill};
use crate::labs::{Feature, Group};

pub(super) static LABS_ICON: Icon = Icon {
    name: "hex-labs",
    view: 16.0,
    body: r#"<path d="M6.25 2.5h3.5M6.75 2.5v4L3.2 12.3a.8.8 0 0 0 .7 1.2h8.2a.8.8 0 0 0 .7-1.2L9.25 6.5v-4M4.8 10h6.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// The main sidebar's panes, in order: General, Output, History, then the
/// features turned on (Meetings and Voice Filter from Labs, Annotate in dev
/// builds).
pub(super) fn main_panes(ui: &Ui) -> Vec<Pane> {
    let mut panes = vec![Pane::General, Pane::Output, Pane::History];
    #[cfg(hex_capture)]
    let optional = [Pane::Meetings, Pane::Annotate, Pane::VoiceFilter];
    #[cfg(not(hex_capture))]
    let optional = [Pane::Meetings, Pane::VoiceFilter];
    panes.extend(optional.into_iter().filter(|pane| pane.shown(ui)));
    panes
}

/// Every pane the sidebar can open, in its order, for Command-1 through 9.
pub(super) fn order(ui: &Ui) -> Vec<Pane> {
    main_panes(ui)
}

/// A sidebar item that isn't a pane: Labs, or the playground. Its slot
/// takes presses in the gaps around it too; assistive technology sees the
/// item alone, selected while `on`.
fn nav_row(
    p: Paint,
    key: &'static str,
    icon: &'static Icon,
    label: &'static str,
    on: bool,
    press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    let c = &p.k.c;
    let press = std::rc::Rc::new(press);
    let in_slot = press.clone();
    let mut item = row()
        .key(key)
        .role("listitem")
        .label(label)
        .focus_selected(on)
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| press(ui, tx))
        .child(
            rune::icon(icon, 16.0)
                .key("icon")
                .color(if on { c.icon_base } else { c.icon_muted })
                .a11y_hidden(),
        )
        .child(
            text(label)
                .key("label")
                .font(p.k.ty.ui)
                .color(if on { c.text_base } else { c.text_muted })
                .a11y_hidden(),
        );
    if on {
        item = item.fill(c.layer_02);
    }
    row()
        .key(format!("{key}-slot"))
        .role("none")
        .pt(NAV_ROW_GAP / 2.0)
        .pb(NAV_ROW_GAP / 2.0)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| in_slot(ui, tx))
        .child(item.grow(1.0))
}

/// The Playground item while Labs › Playground is on (dev builds only): it
/// opens it.
pub(super) fn playground_nav(ui: &Ui, p: Paint) -> Option<El> {
    (Feature::Playground.shipped() && ui.settings.labs.playground).then(|| {
        nav_row(
            p,
            "nav-playground",
            &SPARKLES,
            "Playground",
            false,
            |_: &mut Ui, tx: &mut Tx| tx.open_playground(),
        )
    })
}

/// The Labs item: opens the Labs page, or goes back to General.
pub(super) fn nav(ui: &Ui, p: Paint) -> El {
    let on = ui.labs_open;
    nav_row(
        p,
        "nav-labs",
        &LABS_ICON,
        tr("Labs"),
        on,
        move |ui: &mut Ui, tx: &mut Tx| {
            if on {
                ui.select_pane(Pane::General, tx);
            } else {
                open(ui, tx);
            }
        },
    )
}

/// One lab: a Labs feature, or Annotate in capture builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lab {
    Feature(Feature),
    #[cfg(hex_capture)]
    Annotate,
}

impl Lab {
    /// Its stable id, in `hex://labs/<id>`.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Feature(feature) => feature.name(),
            #[cfg(hex_capture)]
            Self::Annotate => "annotate",
        }
    }

    /// The day it joined Labs, `yyyymmdd`; 0 when it never counts as new.
    fn added(self) -> u32 {
        match self {
            Self::Feature(feature) => feature.added(),
            #[cfg(hex_capture)]
            Self::Annotate => 0,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Feature(feature) => Feature::ALL
                .iter()
                .position(|known| *known == feature)
                .unwrap_or(0),
            #[cfg(hex_capture)]
            Self::Annotate => 0,
        }
    }

    fn audience(self) -> crate::labs::Audience {
        match self {
            Self::Feature(feature) => feature.audience(),
            #[cfg(hex_capture)]
            Self::Annotate => crate::labs::Audience::Everyone,
        }
    }
}

/// Discover lists every lab; Turned on only those that are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Discover,
    On,
}

/// What the Labs store shows: its search, tab, and chip.
#[derive(Default)]
pub(super) struct Store {
    pub(super) search: TextEdit,
    pub(super) tab: Tab,
    /// The chip selected: only this group's labs show.
    pub(super) group: Option<Group>,
}

impl Store {
    fn query(&self) -> String {
        self.search.text.trim().to_lowercase()
    }
}

/// What a `hex://labs` link asks the store to show.
#[cfg_attr(
    not(any(target_os = "macos", all(test, target_os = "linux"))),
    allow(dead_code)
)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LabsLink {
    /// The lab to show, by its id: its group's chip is selected.
    pub(crate) lab: Option<String>,
    /// Turned on instead of Discover (`?tab=on`).
    pub(crate) turned_on: bool,
    /// The chip selected (`?category=<id>`).
    pub(crate) category: Option<String>,
    /// The search typed in (`?search=<words>`).
    pub(crate) search: Option<String>,
}

/// A deep link: the Labs store on its tab, chip, and search. A lab it names
/// selects that lab's group chip unless the link names a category. It never
/// turns a lab on. An unknown lab or category, or a lab this OS has no card
/// for, is logged and skipped.
#[cfg_attr(
    not(any(target_os = "macos", all(test, target_os = "linux"))),
    allow(dead_code)
)]
pub(super) fn reveal(ui: &mut Ui, link: &LabsLink, tx: &mut Tx) {
    super::show(ui, super::Destination::Settings, tx);
    open(ui, tx);
    let store = &mut ui.lab_store;
    store.tab = if link.turned_on {
        Tab::On
    } else {
        Tab::Discover
    };
    if let Some(id) = &link.category {
        match Group::from_id(id) {
            Some(group) => store.group = Some(group),
            None => tracing::warn!(category = %id, "a Labs link named no category"),
        }
    }
    if let Some(search) = &link.search {
        store.search = TextEdit::new(search.clone());
    }
    let Some(id) = &link.lab else { return };
    match shown(ui).into_iter().find(|row| row.lab.id() == id) {
        Some(row) if link.category.is_none() => ui.lab_store.group = Some(row.group),
        Some(_) => {}
        None => tracing::warn!(feature = %id, "a Labs link named no feature this Hex has"),
    }
}

fn open(ui: &mut Ui, tx: &mut Tx) {
    ui.select_pane(Pane::General, tx);
    ui.labs_open = true;
    ui.lab_store = Store::default();
    tx.settle();
}

/// ⌘F (Ctrl+F off macOS) on the Labs page puts the caret in its search.
pub(super) fn focus_search(ui: &mut Ui) {
    ui.lab_store.search.focused = true;
}

/// One lab's card.
struct Row {
    lab: Lab,
    icon: &'static Icon,
    title: &'static str,
    about: &'static str,
    on: bool,
    /// Can't be switched: on while an installed Hex Microphone driver or
    /// the dictation model needs it, off under an organization's lock.
    locked: bool,
    group: Group,
}

/// The labs this OS and build show, in the store's order.
fn shown(ui: &Ui) -> Vec<Row> {
    rows(ui)
        .into_iter()
        .filter(|row| row.lab.audience().reaches(crate::dev_build::is_dev_build()))
        .collect()
}

fn rows(ui: &Ui) -> Vec<Row> {
    let caps = ui.platform.caps;
    let settings = &ui.settings;
    let row = |feature: Feature, icon: Option<&'static Icon>, title, about| {
        let lab = Lab::Feature(feature);
        let group = feature.group();
        Row {
            lab,
            icon: icon.unwrap_or_else(|| group::icon(group)),
            title,
            about,
            on: feature.on(settings),
            locked: false,
            group,
        }
    };
    // An organization's lock shows the row off, disabled, and says why.
    let row = |feature: Feature, icon, title, about| match feature.managed() {
        Some(_) => Row {
            on: false,
            locked: true,
            about: super::managed_note(),
            ..row(feature, icon, title, about)
        },
        None => row(feature, icon, title, about),
    };
    let mut rows = Vec::new();
    if caps.meetings {
        rows.push(row(
            Feature::Meetings,
            Some(Pane::Meetings.icon()),
            tr("Meetings (beta)"),
            tr("Record calls and keep transcripts"),
        ));
    }
    if caps.fun && Feature::VoiceFilter.managed().is_some() {
        rows.push(row(
            Feature::VoiceFilter,
            Some(&VOICE_FILTER_ICON),
            tr("Voice Filter"),
            "",
        ));
    } else if caps.fun {
        let locked = super::fun::driver_installed(ui);
        rows.push(Row {
            on: super::fun::running(ui),
            locked,
            about: if locked {
                tr("On while the Hex Microphone driver is installed. Uninstall the driver in Voice Filter to turn it off.")
            } else {
                tr("Hex Microphone virtual input and real-time voice filters")
            },
            ..row(Feature::VoiceFilter, Some(&VOICE_FILTER_ICON), tr("Voice Filter"), "")
        });
    }
    rows.push(row(
        Feature::Playground,
        Some(&SPARKLES),
        "Playground",
        tr("Scene and story browser in the window"),
    ));
    rows.push(row(
        Feature::LiveText,
        Some(&WAVE),
        tr("Live text"),
        tr("Show provisional words beside the HUD while dictating"),
    ));
    if caps.meetings {
        rows.push(row(
            Feature::LiveChapters,
            None,
            tr("Live chapters"),
            tr("Break meeting transcripts into chapters while recording"),
        ));
        rows.push(row(
            Feature::SubtitleExport,
            None,
            tr("Subtitle export"),
            tr("Export meeting transcripts as SRT or VTT subtitles with hex meetings show"),
        ));
        // macOS's meeting controller reads the call app's audio output, so
        // Linux renders of macOS scenes leave it out.
        if cfg!(target_os = "macos") {
            rows.push(row(
                Feature::StopWhenCallEnds,
                Some(&TIMER),
                tr("Stop when the call ends"),
                tr("Stop a recording Hex started on its own 30 s after the call app leaves the microphone and goes quiet"),
            ));
            // Only macOS's capsules know the screen to snap a drag against.
            rows.push(row(
                Feature::MovableMeetingCapsules,
                None,
                tr("Movable meeting capsules"),
                tr("Drag meeting capsules out of the way; they come back in the corner you left them"),
            ));
        }
        rows.push(row(
            Feature::MeetingNotesVocabulary,
            Some(&BOOK),
            tr("Vocabulary in meeting notes"),
            tr("Meeting titles and summaries read your vocabulary, so names are spelled the way you wrote them"),
        ));
        rows.push(row(
            Feature::MeetingFinishedEvent,
            Some(&GLOBE),
            tr("Meeting finished event"),
            tr("Tell apps on this computer when a meeting's transcript is ready. They get only the meeting's ID, through Hex's local API"),
        ));
        rows.push(row(
            Feature::FollowLiveTranscript,
            None,
            tr("Follow live transcript"),
            tr("While a meeting records, the transcript scrolls to the newest line. Scroll up to read earlier text; scroll back to the bottom to follow again."),
        ));
        rows.push(row(
            Feature::MoreCallApps,
            Some(&GLOBE),
            tr("More call apps"),
            tr("Offer to record calls in Signal, Telegram, WeChat, LINE, Webex, Element and the Phone app, and name calls on more websites."),
        ));
    }
    rows.push(row(
        Feature::VocabularyBiasing,
        Some(&BOOK),
        tr("Vocabulary biasing"),
        tr("Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way"),
    ));
    if caps.keyboard_model_switching {
        rows.push(row(
            Feature::KeyboardModelSwitching,
            Some(&KEYBOARD),
            tr("Switch model with keyboard language"),
            tr("Changing input source selects its mapped model and language; manual choices still work"),
        ));
    }
    if caps.os == super::platform::Os::Macos {
        rows.push(row(
            Feature::MouseShortcuts,
            Some(&MOUSE),
            tr("Mouse shortcuts"),
            tr("Record mouse buttons as shortcuts"),
        ));
        rows.push(row(
            Feature::PauseBrowser,
            Some(&GLOBE),
            tr("Also pause the browser"),
            tr("While dictating with Pause media: YouTube and other tabs, resumed only if Hex paused them"),
        ));
        rows.push(row(
            Feature::KeepOnClipboard,
            None,
            tr("Keep dictation on clipboard"),
            tr("Leave the dictation on the clipboard after pasting instead of restoring what was there"),
        ));
        rows.push(row(
            Feature::CopyOnly,
            None,
            tr("Copy dictation only"),
            tr("Put the dictation on the clipboard instead of pasting it"),
        ));
        // macOS's listener only, so Linux renders of macOS scenes leave it out.
        if cfg!(target_os = "macos") {
            rows.push(row(
                Feature::SecondDictationShortcut,
                Some(&KEYBOARD),
                tr("Second dictation shortcut"),
                tr("Another shortcut that dictates like the main one, such as a key on an external keyboard"),
            ));
            // Reads the text before the cursor through macOS Accessibility.
            rows.push(row(
                Feature::SmartSpacing,
                None,
                tr("Smart spacing"),
                tr("Add a space before a dictation pasted right after a word"),
            ));
            // macOS's recorder reads each modifier key's side.
            rows.push(row(
                Feature::SidedChords,
                Some(&KEYBOARD),
                tr("Sided modifier chords"),
                tr("Record which side each modifier of a shortcut is on, such as Right ⌘ + Right ⌥"),
            ));
            // macOS's shortcut machine reads it at each Toggle release.
            rows.push(row(
                Feature::TapOrHold,
                Some(&KEYBOARD),
                tr("Tap or hold"),
                tr("In Toggle mode, hold the shortcut to dictate until you let go; a short tap still starts and stops"),
            ));
            // macOS's menu-bar menu lists them.
            rows.push(row(
                Feature::RecentDictations,
                None,
                tr("Copy recent dictations"),
                tr("Copy one of your last five dictations from the menu bar, under Paste Last Dictation"),
            ));
            // macOS's status item hides itself while nothing records.
            rows.push(row(
                Feature::HideMenuBarIcon,
                None,
                tr("Hide menu bar icon"),
                tr("Hide Hex's icon from the menu bar. Shortcuts keep working; open Hex again from Finder or Spotlight for Settings"),
            ));
            // macOS's HUD reads the screen's notch from NSScreen.
            rows.push(row(
                Feature::NotchIndicator,
                Some(&WAVE),
                tr("Notch indicator"),
                tr("On a screen with a notch, the recording indicator grows out of the notch instead of sitting below the menu bar"),
            ));
            // Hex's session tap takes the shortcut; ScreenCaptureKit shoots.
            #[cfg(hex_capture)]
            rows.push(row(
                Feature::Screenshots,
                Some(&CAMERA),
                tr("Screenshots"),
                tr("Press ⌘⇧4 to capture an area, or press Space for a window. The shot goes on the clipboard and into a folder"),
            ));
            // macOS 27's SCClipBufferingOutput keeps a rolling 15 s display ring in replayd.
            #[cfg(hex_capture)]
            if Feature::AnnotateReplay.on_this_os() {
                rows.push(row(
                    Feature::AnnotateReplay,
                    Some(Pane::Annotate.icon()),
                    tr("Annotate 15 s replay"),
                    tr("Save the last 15 seconds of your screen. macOS shows the screen-recording dot the whole time it's on"),
                ));
            }
        }
    }
    // Linux's X11 grabs and shortcuts portal take these keys alone.
    if caps.os == super::platform::Os::Linux {
        rows.push(row(
            Feature::StandaloneSpecialKeys,
            Some(&KEYBOARD),
            tr("Special keys alone"),
            tr("Use Menu, Pause, or Scroll Lock as a shortcut without a modifier, such as Caps Lock remapped to Menu"),
        ));
        rows.push(row(
            Feature::SectionKeyAlone,
            Some(&KEYBOARD),
            tr("Section key alone"),
            tr("Use the § key, left of 1 on ISO keyboards, as a shortcut without a modifier. Hex takes § from every app while it's bound"),
        ));
        // Linux's shortcut machine reads it with the listener's options.
        rows.push(row(
            Feature::TapOrHold,
            Some(&KEYBOARD),
            tr("Tap or hold"),
            tr("In Toggle mode, hold the shortcut to dictate until you let go; a short tap still starts and stops"),
        ));
        rows.push(row(
            Feature::GnomeClipboardRestore,
            None,
            tr("Restore clipboard on GNOME"),
            tr("Put back what you copied before a dictation, through XWayland. Works only while XWayland is already running"),
        ));
        rows.push(row(
            Feature::GnomeMeetingCapsules,
            None,
            tr("Floating meeting capsules on GNOME"),
            tr("Show the recording offer and recording indicator floating over your call, through XWayland. Works only while XWayland is already running"),
        ));
    }
    // Linux's and Windows' engines transcribe a whole clip on the CPU.
    if caps.os != super::platform::Os::Macos {
        rows.push(row(
            Feature::SkipPauses,
            Some(&TIMER),
            tr("Skip long pauses"),
            tr("Shorten long pauses before a whole dictation goes to the model, so slow computers paste sooner"),
        ));
    }
    rows.push(row(
        Feature::UnloadIdle,
        Some(&TIMER),
        tr("Unload model when idle"),
        tr("Frees the speech model's memory after a while unused; the next dictation reloads it"),
    ));
    rows.push(row(
        Feature::RestoreShortcutDefault,
        Some(&KEYBOARD),
        tr("Restore shortcut default"),
        tr("Offer Restore default next to Paste last dictation's shortcut"),
    ));
    rows.push(row(
        Feature::SpokenFormatting,
        None,
        tr("Spoken formatting"),
        tr("Say \"new line\", \"new paragraph\", \"open quote\", or \"close quote\" to insert line breaks and quotation marks"),
    ));
    rows.push(row(
        Feature::SpokenSymbols,
        None,
        tr("Spoken symbols"),
        tr("Say \"comma\", \"colon\", \"hashtag\", \"underscore\", or \"slash\" to insert the symbol joined to its words"),
    ));
    rows.push(row(
        Feature::FillerWords,
        None,
        tr("Remove filler words"),
        tr("Remove filler words such as um and uh from dictations, without an AI model"),
    ));
    rows.push(row(
        Feature::HistoryStats,
        None,
        tr("Dictation stats"),
        tr("Show a summary of your retained dictations at the top of History, with hex history stats"),
    ));
    rows.push(row(
        Feature::KeepRecordings,
        None,
        tr("Keep recordings"),
        tr("Keep the audio of dictations saved to History, so you can transcribe them again with another model"),
    ));
    rows.push(row(
        Feature::OutputFixes,
        Some(&BOOK),
        tr("Fix results in Output"),
        tr("Offer Vocabulary and Fix this beside Output's Try result, to correct a wrong result"),
    ));
    rows.push(row(
        Feature::PickerSortFilter,
        None,
        tr("Sort and filter models"),
        tr("Sort the Local transcription picker by size or name and show only models for the saved language"),
    ));
    rows.push(row(
        Feature::BenchmarkDetails,
        None,
        tr("Benchmark details"),
        tr("Show each model's raw error rate, benchmark, and source in the Local transcription picker"),
    ));
    rows.push(row(
        Feature::PickerActiveFirst,
        None,
        tr("Model in use first"),
        tr("The speech model picker lists the model you're using first"),
    ));
    rows.push(row(
        Feature::ModelInPlace,
        None,
        tr("Use model files where they are"),
        tr("Add a speech model file without copying it, so several accounts can share one file"),
    ));
    rows.push(row(
        Feature::PickerDownloaded,
        None,
        tr("Downloaded models only"),
        tr("Show only downloaded speech models in the picker, with the space they take"),
    ));
    let ultra_locked = Feature::ParakeetUltra.managed().is_none()
        && Feature::ParakeetUltra.off_refusal(&ui.settings).is_some();
    let ultra = row(Feature::ParakeetUltra, None, tr("Parakeet Ultra"), "");
    // A managed row already says so.
    rows.push(if ultra.locked {
        ultra
    } else {
        Row {
            locked: ultra_locked,
            about: if ultra_locked {
                tr("On while it is the dictation model. Choose another model to turn it off.")
            } else {
                tr("Offer Parakeet Ultra, Moondream's post-trained Parakeet v3, in the Local transcription picker")
            },
            ..ultra
        }
    });
    if caps.share_shortcut {
        rows.push(row(
            Feature::ShareShortcut,
            Some(&KEYBOARD),
            tr("Share dictation shortcut with other apps"),
            tr("Lets apps such as Discord detect the same key"),
        ));
    }
    if caps.os == super::platform::Os::Windows {
        rows.push(row(
            Feature::TerminalPaste,
            Some(&TERMINAL),
            tr("Terminal paste"),
            tr("Paste with Ctrl+Shift+V so terminals accept it"),
        ));
    }
    #[cfg(hex_capture)]
    if ui.platform.scrawl.is_some() {
        let locked = crate::policy::Policy::current().locks(crate::policy::Lock::Annotate);
        rows.push(Row {
            lab: Lab::Annotate,
            icon: Pane::Annotate.icon(),
            title: tr("Annotate"),
            about: if locked {
                super::managed_note()
            } else {
                tr("Record your screen, draw on it, and talk over it with a shortcut")
            },
            on: !locked && ui.settings.capture.enabled,
            locked,
            group: Group::Annotate,
        });
    }
    rows
}

fn row_key(lab: Lab) -> &'static str {
    let feature = match lab {
        Lab::Feature(feature) => feature,
        #[cfg(hex_capture)]
        Lab::Annotate => return "lab-annotate",
    };
    match feature {
        Feature::Meetings => "lab-meetings",
        Feature::VoiceFilter => "lab-voice-filter",
        Feature::Playground => "lab-playground",
        Feature::LiveText => "lab-live-text",
        Feature::LiveChapters => "lab-live-chapters",
        Feature::VocabularyBiasing => "lab-vocabulary-biasing",
        Feature::TerminalPaste => "lab-terminal-paste",
        Feature::KeyboardModelSwitching => "lab-keyboard-model-switching",
        Feature::MouseShortcuts => "lab-mouse-shortcuts",
        Feature::PauseBrowser => "lab-pause-browser",
        Feature::ShareShortcut => "lab-share-shortcut",
        Feature::UnloadIdle => "lab-unload-idle",
        Feature::RestoreShortcutDefault => "lab-restore-shortcut-default",
        Feature::KeepOnClipboard => "lab-keep-on-clipboard",
        Feature::CopyOnly => "lab-copy-only",
        Feature::SpokenFormatting => "lab-spoken-formatting",
        Feature::FillerWords => "lab-filler-words",
        Feature::SubtitleExport => "lab-subtitle-export",
        Feature::SecondDictationShortcut => "lab-second-dictation-shortcut",
        Feature::SmartSpacing => "lab-smart-spacing",
        Feature::SpokenSymbols => "lab-spoken-symbols",
        Feature::PickerSortFilter => "lab-picker-sort-filter",
        Feature::SidedChords => "lab-sided-modifier-chords",
        Feature::TapOrHold => "lab-tap-or-hold",
        Feature::BenchmarkDetails => "lab-benchmark-details",
        Feature::ParakeetUltra => "lab-parakeet-ultra",
        Feature::StandaloneSpecialKeys => "lab-special-keys",
        Feature::HistoryStats => "lab-history-stats",
        Feature::RecentDictations => "lab-recent-dictations",
        Feature::SkipPauses => "lab-skip-pauses",
        Feature::StopWhenCallEnds => "lab-stop-when-call-ends",
        Feature::OutputFixes => "lab-output-fixes",
        Feature::PickerActiveFirst => "lab-picker-active-first",
        Feature::ModelInPlace => "lab-model-in-place",
        Feature::PickerDownloaded => "lab-picker-downloaded",
        Feature::SectionKeyAlone => "lab-section-key",
        Feature::KeepRecordings => "lab-keep-recordings",
        Feature::HideMenuBarIcon => "lab-hide-menu-bar-icon",
        Feature::MovableMeetingCapsules => "lab-movable-meeting-capsules",
        Feature::GnomeClipboardRestore => "lab-gnome-clipboard-restore",
        Feature::MeetingNotesVocabulary => "lab-meeting-notes-vocabulary",
        Feature::NotchIndicator => "lab-notch-indicator",
        Feature::MeetingFinishedEvent => "lab-meeting-finished-event",
        Feature::GnomeMeetingCapsules => "lab-gnome-meeting-capsules",
        Feature::Screenshots => "lab-screenshots",
        Feature::FollowLiveTranscript => "lab-follow-live-transcript",
        Feature::MoreCallApps => "lab-more-call-apps",
        Feature::AnnotateReplay => "lab-annotate-replay",
    }
}

fn flip(ui: &mut Ui, lab: Lab) {
    let feature = match lab {
        Lab::Feature(feature) => feature,
        #[cfg(hex_capture)]
        Lab::Annotate => {
            if ui.settings.capture.enabled {
                group::keep_open(ui, Group::Annotate);
            }
            return super::scrawl::toggle(ui);
        }
    };
    if feature.managed().is_some() {
        return;
    }
    if feature == Feature::VoiceFilter && super::fun::driver_installed(ui) {
        return;
    }
    let on = !feature.on(&ui.settings);
    // Parakeet Ultra stays on while it dictates; its card says why.
    if !on && feature.off_refusal(&ui.settings).is_some() {
        return;
    }
    if !on {
        group::keep_open(ui, feature.group());
    }
    ui.update(|settings| feature.set(settings, on));
    match feature {
        Feature::KeyboardModelSwitching => ui.open = None,
        // Hex hears the browser through System Audio Recording.
        Feature::PauseBrowser if on && !ui.platform.system_audio => {
            ui.ask(super::platform::PlatformRequest::SystemAudio);
        }
        _ => {}
    }
}

/// A lab's own settings, on its card while it's on: how long the model sits
/// unused before it unloads, or the screenshot shortcut and folder.
fn extras(ui: &Ui, p: Paint, entry: &Row) -> Option<El> {
    if !entry.on {
        return None;
    }
    match entry.lab {
        Lab::Feature(Feature::UnloadIdle) => Some(
            row()
                .key("unload-after")
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(
                    text(tr("Unload after"))
                        .key("label")
                        .font(controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(unload_after(ui, p)),
        ),
        #[cfg(all(target_os = "macos", hex_capture))]
        Lab::Feature(Feature::Screenshots) => Some(screenshots::body(
            ui,
            p,
            crate::screenshots::system_takes_shortcut(),
        )),
        _ => None,
    }
}

/// How long the model sits unused before it unloads.
fn unload_after(ui: &Ui, p: Paint) -> El {
    use crate::app_settings::UNLOAD_IDLE_MINUTES;
    let labels: Vec<String> = UNLOAD_IDLE_MINUTES
        .iter()
        .map(|minutes| tr_fill("{} min", &minutes.to_string()))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let selected = UNLOAD_IDLE_MINUTES
        .iter()
        .position(|minutes| *minutes == ui.settings.labs.unload_idle_minutes)
        .unwrap_or(1);
    controls::segmented(p, "unload-idle-minutes", &labels, selected, |ui, index| {
        let minutes = UNLOAD_IDLE_MINUTES[index];
        ui.update(|settings| settings.labs.unload_idle_minutes = minutes);
    })
}
