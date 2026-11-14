//! Labs: one sidebar item that opens a store of every Labs feature
//! (`crate::labs`), each off by default. Discover shows what's new, then
//! every lab as a card under its group's collapsible header, and Turned on
//! what's on. Each card shows its description and switch, and while on any
//! settings the lab has of its own. A feature with its own pane (Meetings,
//! Voice Filter, Playground) joins the main sidebar while on and is hidden
//! while off.

mod browser_access;
mod group;
#[cfg(all(target_os = "macos", hex_capture))]
mod screenshots;
mod store;
mod voice_prompt;
mod yolo_mode;

use rune::{El, Icon, TextEdit, Tx, row, text};

pub(super) use self::store::{page, search_field, tabs};
use super::controls::{self, Paint};
use super::icons::{
    BOOK, CAMERA, CLOCK, FOLDER, GLOBE, KEYBOARD, MOUSE, PANEL, SPARKLES, TERMINAL, TIMER, WAVE,
};
use super::list_detail::SEARCH_ICON;
use super::sidebar::{NAV_ROW_GAP, NAV_ROW_HEIGHT, VOICE_FILTER_ICON};
use super::{Pane, Ui};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
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
    let optional = [
        Pane::Captures,
        Pane::Meetings,
        Pane::Annotate,
        Pane::VoiceFilter,
    ];
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
/// item alone, selected while `on`. A focusable item joins the navigation
/// list's arrows.
fn nav_row(
    p: Paint,
    key: &'static str,
    icon: &'static Icon,
    label: &'static str,
    on: bool,
    focusable: bool,
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
    if focusable {
        item = item.focusable(true);
    }
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
            false,
            |_: &mut Ui, tx: &mut Tx| tx.open_playground(),
        )
    })
}

/// The Labs item: opens the Labs page.
pub(super) fn nav(ui: &Ui, p: Paint) -> El {
    let on = ui.labs_open;
    nav_row(
        p,
        "nav-labs",
        &LABS_ICON,
        tr("Labs"),
        on,
        true,
        |ui: &mut Ui, tx: &mut Tx| {
            if !ui.labs_open {
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
    /// Labs › Voice prompt's phrase while it's on: a draft, so invalid words
    /// stay in its field instead of replacing the saved phrase.
    pub(super) phrase: TextEdit,
    /// Whether `phrase` started from the saved phrase.
    pub(super) phrase_synced: bool,
    /// Labs › Browser access to the local API's origins while it's on: a
    /// draft, saved as the comma-separated field shows it.
    pub(super) origins: TextEdit,
    /// Whether `origins` started from the saved origins.
    pub(super) origins_synced: bool,
    /// The lab a `hex://labs/<feature>` link named and when, so its card
    /// shows a focus border for [`FLASH`].
    pub(super) flash: Option<(&'static str, std::time::Instant)>,
}

/// How long a linked lab's card keeps its focus border.
pub(super) const FLASH: std::time::Duration = std::time::Duration::from_millis(1500);

impl Store {
    fn query(&self) -> String {
        self.search.text.trim().to_lowercase()
    }

    /// Starts the phrase field at `saved` when Labs opens or its lab turns on.
    pub(super) fn sync_phrase(&mut self, saved: &str) {
        self.phrase = TextEdit::new(saved.to_owned());
        self.phrase_synced = true;
    }

    /// Adopts a phrase another writer saved, unless the field holds an
    /// unsaved draft.
    pub(super) fn rebase_phrase(&mut self, before: &str, after: &str) {
        if !self.phrase_synced || self.phrase.text.trim() == before {
            self.sync_phrase(after);
        }
    }

    /// The phrase `settings` should hold: the draft when it passes
    /// validation. An invalid draft never replaces the saved phrase.
    pub(super) fn project_phrase(&self, settings: &mut crate::app_settings::AppSettings) {
        let draft = self.phrase.text.trim();
        if crate::dictation::voice_prompt::phrase_problem(draft).is_none() {
            settings.labs.voice_prompt_phrase = draft.to_owned();
        }
    }

    /// Whether the phrase field holds typing the saved settings don't have.
    /// A lab that's off has no draft to keep.
    pub(super) fn phrase_dirty(&self, settings: &crate::app_settings::AppSettings) -> bool {
        if !settings.labs.voice_prompt {
            return false;
        }
        let draft = self.phrase.text.trim();
        !draft.is_empty() && draft != settings.labs.voice_prompt_phrase
    }

    /// Starts the origins field at the saved origins, normalized.
    pub(super) fn sync_origins(&mut self, saved: &[String]) {
        self.origins = TextEdit::new(origins_text(saved));
        self.origins_synced = true;
    }

    /// Adopts origins another writer saved, unless the field holds an unsaved
    /// draft. The field's own save passes the same list on both sides and
    /// leaves the text (and its caret) as typed.
    pub(super) fn rebase_origins(&mut self, before: &[String], after: &[String]) {
        let before = crate::app_settings::normalize_browser_origins(before);
        let after = crate::app_settings::normalize_browser_origins(after);
        if before == after {
            return;
        }
        let draft = crate::app_settings::parse_browser_origins(&self.origins.text);
        if !self.origins_synced || draft == before {
            self.sync_origins(&after);
        }
    }

    /// The origins `settings` should hold: the field's entries, ignoring
    /// anything that isn't an `http://` or `https://` origin. A lab that's
    /// off, or a field this window never opened, leaves its saved list alone.
    pub(super) fn project_origins(&self, settings: &mut crate::app_settings::AppSettings) {
        if !settings.labs.api_browser_access || !self.origins_synced {
            return;
        }
        settings.labs.api_browser_origins =
            crate::app_settings::parse_browser_origins(&self.origins.text);
    }

    /// Whether the origins field holds typing the saved origins don't have.
    /// A lab that's off, or a field this window never opened, has no draft.
    pub(super) fn origins_dirty(&self, settings: &crate::app_settings::AppSettings) -> bool {
        if !settings.labs.api_browser_access || !self.origins_synced {
            return false;
        }
        crate::app_settings::parse_browser_origins(&self.origins.text)
            != crate::app_settings::normalize_browser_origins(&settings.labs.api_browser_origins)
    }
}

/// The saved origins as the card's comma-separated field shows them.
fn origins_text(origins: &[String]) -> String {
    crate::app_settings::normalize_browser_origins(origins).join(", ")
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
        Some(row) => {
            if link.category.is_none() {
                ui.lab_store.group = Some(row.group);
            }
            let id = row.lab.id();
            ui.lab_store.flash = Some((id, std::time::Instant::now()));
            tx.scroll_into_view(format!("lab-card-{id}"));
            tx.wake_after(FLASH);
        }
        None => tracing::warn!(feature = %id, "a Labs link named no feature this Hex has"),
    }
}

fn open(ui: &mut Ui, tx: &mut Tx) {
    ui.select_pane(Pane::General, tx);
    ui.labs_open = true;
    ui.lab_store = Store::default();
    ui.lab_store
        .sync_phrase(&ui.settings.labs.voice_prompt_phrase);
    ui.lab_store
        .sync_origins(&ui.settings.labs.api_browser_origins);
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
    /// A line computed from this computer's state, shown in addition to
    /// `about` (the Live text card naming the models that stream, or the Live
    /// typing card explaining this desktop has no input method it can use).
    note: Option<String>,
    on: bool,
    /// Can't be switched: on while an installed Hex Microphone driver or
    /// the dictation model needs it, off under an organization's lock, or off
    /// because this desktop offers no input method Live typing uses.
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
    let row = |feature: Feature, icon: Option<&'static Icon>| {
        let lab = Lab::Feature(feature);
        let group = feature.group();
        Row {
            lab,
            icon: icon.unwrap_or_else(|| group::icon(group)),
            title: tr(feature.title()),
            about: tr(feature.summary()),
            note: None,
            on: feature.on(settings),
            locked: false,
            group,
        }
    };
    // An organization's lock shows the row off, disabled, and says why.
    let row = |feature: Feature, icon| match feature.managed() {
        Some(_) => Row {
            on: false,
            locked: true,
            about: super::managed_note(),
            ..row(feature, icon)
        },
        None => row(feature, icon),
    };
    let mut rows = Vec::new();
    if caps.meetings {
        rows.push(row(Feature::Meetings, Some(Pane::Meetings.icon())));
    }
    if caps.fun && Feature::VoiceFilter.managed().is_some() {
        rows.push(row(Feature::VoiceFilter, Some(&VOICE_FILTER_ICON)));
    } else if caps.fun {
        let locked = super::fun::driver_installed(ui);
        let voice_filter = row(Feature::VoiceFilter, Some(&VOICE_FILTER_ICON));
        rows.push(Row {
            on: super::fun::running(ui),
            locked,
            about: if locked {
                tr("On while the Hex Microphone driver is installed. Uninstall the driver in Voice Filter to turn it off.")
            } else {
                voice_filter.about
            },
            ..voice_filter
        });
    }
    rows.push(row(Feature::Playground, Some(&SPARKLES)));
    let live_text = row(Feature::LiveText, Some(&WAVE));
    rows.push(Row {
        note: live_text_note(ui),
        ..live_text
    });
    rows.push(row(Feature::FailureNotices, None));
    if caps.os == super::platform::Os::Linux && ui.platform.wayland {
        let live_typing = row(Feature::LiveTyping, Some(&KEYBOARD));
        // An organization's lock and its note already say why the switch is
        // off; the desktop's own limit only replaces an unlocked card.
        if Feature::LiveTyping.managed().is_some() {
            rows.push(live_typing);
        } else {
            let unsupported = ui.platform.live_typing_unsupported;
            let on = live_typing.on;
            rows.push(Row {
                locked: unsupported && !on,
                note: unsupported.then(|| {
                    tr("This desktop doesn't offer the input method Live typing needs (GNOME doesn't), so dictations paste normally")
                        .to_owned()
                }),
                ..live_typing
            });
        }
    }
    rows.push(row(Feature::SettingsSearch, Some(SEARCH_ICON)));
    rows.push(row(Feature::Appearance, Some(&PANEL)));
    rows.push(row(Feature::ReleaseNotesLink, None));
    if caps.meetings {
        rows.push(row(Feature::LiveChapters, None));
        rows.push(row(Feature::SubtitleExport, None));
        // macOS's meeting controller reads the call app's audio output, so
        // Linux renders of macOS scenes leave it out.
        if cfg!(target_os = "macos") {
            rows.push(row(Feature::StopWhenCallEnds, Some(&TIMER)));
            // Only macOS's capsules know the screen to snap a drag against.
            rows.push(row(Feature::MovableMeetingCapsules, None));
        }
        rows.push(row(Feature::MeetingNotesVocabulary, Some(&BOOK)));
        rows.push(row(Feature::MeetingDescriptiveFilenames, None));
        rows.push(row(Feature::MeetingNotesFile, None));
        rows.push(row(Feature::MeetingFinishedEvent, Some(&GLOBE)));
        rows.push(row(Feature::FollowLiveTranscript, None));
        rows.push(row(Feature::MoreCallApps, Some(&GLOBE)));
        if Feature::MeetingImport.on_this_os() {
            rows.push(row(Feature::MeetingImport, None));
        }
        rows.push(row(Feature::ExpireMeetingAudio, Some(&CLOCK)));
    }
    rows.push(row(Feature::VocabularyBiasing, Some(&BOOK)));
    // macOS reads the focused window through Accessibility as a capture
    // becomes real.
    if caps.os == super::platform::Os::Macos {
        rows.push(row(Feature::ScreenVocabulary, Some(&BOOK)));
    }
    if caps.keyboard_model_switching {
        rows.push(row(Feature::KeyboardModelSwitching, Some(&KEYBOARD)));
    }
    if caps.app_model_switching {
        rows.push(row(Feature::AppModelSwitching, Some(&KEYBOARD)));
    }
    // Every OS's paster can leave a dictation on the clipboard; only macOS's
    // and Windows' can skip the paste.
    rows.push(row(Feature::KeepOnClipboard, None));
    // Every OS's paster can press Return after its paste.
    rows.push(row(Feature::YoloMode, None));
    if matches!(
        caps.os,
        super::platform::Os::Macos | super::platform::Os::Windows
    ) {
        rows.push(row(Feature::CopyOnly, None));
    }
    // macOS's paster can bring the app a capture started in back to the front.
    if caps.os == super::platform::Os::Macos {
        rows.push(row(Feature::ReturnToApp, Some(&PANEL)));
        // macOS's listener answers a printing start when its dictation ends.
        rows.push(row(Feature::PrintStart, None));
    }
    // macOS's shortcut machine takes a shortcut without a key.
    if caps.os == super::platform::Os::Macos {
        rows.push(row(Feature::DoubleTapModifiers, Some(&KEYBOARD)));
    }
    if caps.os == super::platform::Os::Macos {
        rows.push(row(Feature::MouseShortcuts, Some(&MOUSE)));
        rows.push(row(Feature::CapsLockDictation, Some(&KEYBOARD)));
        rows.push(row(Feature::PauseBrowser, Some(&GLOBE)));
        // macOS's listener only, so Linux renders of macOS scenes leave it out.
        if cfg!(target_os = "macos") {
            // Up to four more, each with its own activation mode.
            rows.push(row(Feature::MoreDictationShortcuts, Some(&KEYBOARD)));
            // Reads the text before the cursor through macOS Accessibility.
            rows.push(row(Feature::SmartSpacing, None));
            // macOS's recorder reads each modifier key's side.
            rows.push(row(Feature::SidedChords, Some(&KEYBOARD)));
            // macOS's shortcut machine reads it at each Toggle release.
            rows.push(row(Feature::TapOrHold, Some(&KEYBOARD)));
            // macOS's menu-bar menu lists them.
            rows.push(row(Feature::RecentDictations, None));
            // macOS's status item hides itself while nothing records.
            rows.push(row(Feature::HideMenuBarIcon, None));
            // macOS's status item offers Pause Hex while its keyboard
            // listener stays up.
            rows.push(row(Feature::PauseMenu, None));
            // macOS's app menu Quit Hex closes the window instead.
            rows.push(row(Feature::QuitClosesWindow, None));
            // macOS's HUD reads the screen's notch from NSScreen.
            rows.push(row(Feature::NotchIndicator, Some(&WAVE)));
            // Hex's session tap takes the shortcut; ScreenCaptureKit shoots.
            #[cfg(hex_capture)]
            rows.push(row(Feature::Screenshots, Some(&CAMERA)));
            // macOS 27's SCClipBufferingOutput keeps a rolling 15 s display ring in replayd.
            #[cfg(hex_capture)]
            if Feature::AnnotateReplay.on_this_os() {
                rows.push(row(Feature::AnnotateReplay, Some(Pane::Annotate.icon())));
            }
        }
    }
    // The ISO section key, left of 1, may be a shortcut with no modifier
    // (Labs › Section key alone, Linux and macOS).
    if matches!(
        caps.os,
        super::platform::Os::Linux | super::platform::Os::Macos
    ) {
        rows.push(row(Feature::SectionKeyAlone, Some(&KEYBOARD)));
    }
    // Linux's X11 grabs and shortcuts portal take these keys alone.
    if caps.os == super::platform::Os::Linux {
        rows.push(row(Feature::StandaloneSpecialKeys, Some(&KEYBOARD)));
        // Linux's shortcut machine reads it with the listener's options.
        rows.push(row(Feature::TapOrHold, Some(&KEYBOARD)));
        rows.push(row(Feature::GnomeClipboardRestore, None));
        rows.push(row(Feature::TypedText, Some(&KEYBOARD)));
        rows.push(row(Feature::GnomeMeetingCapsules, None));
        rows.push(row(Feature::GnomeAnnotate, None));
    }
    // Linux's and Windows' engines transcribe a whole clip on the CPU.
    if caps.os != super::platform::Os::Macos {
        rows.push(row(Feature::SkipPauses, Some(&TIMER)));
    }
    rows.push(row(Feature::UnloadIdle, Some(&TIMER)));
    rows.push(row(Feature::SpokenFormatting, None));
    rows.push(row(Feature::SpokenSymbols, None));
    rows.push(row(Feature::FillerWords, None));
    // macOS reads the pass through Apple Intelligence: a managed lock keeps
    // the row it built (off, disabled, saying why), else the card says why.
    if caps.os == super::platform::Os::Macos {
        let clean_up = row(Feature::CleanUpWithAppleIntelligence, Some(&SPARKLES));
        rows.push(match Feature::CleanUpWithAppleIntelligence.managed() {
            Some(_) => clean_up,
            None => Row {
                locked: !ui.platform.notes.is_available() && !clean_up.on,
                note: super::platform_settings::clean_up_note(ui.platform.notes),
                ..clean_up
            },
        });
    }
    rows.push(row(Feature::HistoryStats, None));
    rows.push(row(Feature::KeepRecordings, None));
    rows.push(row(Feature::HistoryMultiSelect, None));
    rows.push(row(Feature::OutputFixes, Some(&BOOK)));
    rows.push(row(Feature::VoicePrompt, None));
    // macOS's Accessibility reads the text element's caret and nearby text.
    if caps.os == super::platform::Os::Macos {
        rows.push(Row {
            note: Some(
                tr("That text goes to the mode's OpenCode model, which may be a cloud provider. Password fields are never read.")
                    .to_owned(),
            ),
            ..row(Feature::CursorContext, Some(&BOOK))
        });
    }
    rows.push(row(Feature::PickerSortFilter, None));
    rows.push(row(Feature::BenchmarkDetails, None));
    rows.push(row(Feature::PickerActiveFirst, None));
    rows.push(row(Feature::ModelInPlace, None));
    // Metal has the K-quant kernels; the Vulkan builds embed shaders only for
    // the catalog's types.
    if caps.os == super::platform::Os::Macos {
        rows.push(row(Feature::KQuantModels, None));
    }
    rows.push(row(Feature::PickerDownloaded, None));
    rows.push(row(Feature::ShellCompletions, None));
    rows.push(row(Feature::ApiProcessing, None));
    rows.push(row(Feature::ApiTimestamps, None));
    rows.push(row(Feature::ApiBrowserAccess, Some(&GLOBE)));
    rows.push(row(Feature::SpeakersOnly, None));
    rows.push(row(Feature::KeepMicrophoneOpen, Some(&TIMER)));
    let ultra_locked = Feature::ParakeetUltra.managed().is_none()
        && Feature::ParakeetUltra.off_refusal(&ui.settings).is_some();
    let ultra = row(Feature::ParakeetUltra, None);
    // A managed row already says so.
    rows.push(if ultra.locked || !ultra_locked {
        ultra
    } else {
        Row {
            locked: true,
            about: tr("On while it is the dictation model. Choose another model to turn it off."),
            ..ultra
        }
    });
    if caps.share_shortcut {
        rows.push(row(Feature::ShareShortcut, Some(&KEYBOARD)));
    }
    if caps.os == super::platform::Os::Windows {
        rows.push(row(Feature::TerminalPaste, Some(&TERMINAL)));
        rows.push(row(Feature::OpenCodeInWsl, Some(&TERMINAL)));
    }
    // Another shortcut dictates exactly like the main one, on every OS (#627).
    rows.push(row(Feature::SecondDictationShortcut, Some(&KEYBOARD)));
    if caps.os != super::platform::Os::Macos {
        rows.push(row(Feature::TerminalPasteShortcut, Some(&TERMINAL)));
    }
    #[cfg(hex_capture)]
    if ui.platform.scrawl.is_some() {
        // A written capture's copy runs through the same engine and the same
        // Annotate lock.
        let annotate_locked = crate::policy::Policy::current().locks(crate::policy::Lock::Annotate);
        let locked = annotate_locked || Feature::CopyCaptureZip.managed().is_some();
        let copy_zip = row(Feature::CopyCaptureZip, Some(&FOLDER));
        rows.push(Row {
            locked,
            about: if locked {
                super::managed_note()
            } else {
                copy_zip.about
            },
            on: !locked && Feature::CopyCaptureZip.on(settings),
            ..copy_zip
        });
        let locked = annotate_locked;
        rows.push(Row {
            lab: Lab::Annotate,
            icon: Pane::Annotate.icon(),
            title: tr("Annotate"),
            about: if locked {
                super::managed_note()
            } else {
                tr("Record your screen, draw on it, and talk over it with a shortcut")
            },
            note: None,
            on: !locked && ui.settings.capture.enabled,
            locked,
            group: Group::Annotate,
        });
    }
    rows
}

/// The Live text card's line when it can't show words: the selected model's
/// own catalog flag, or a Linux speech engine explicitly set to the CPU
/// (which never streams). `None` when nothing known would stop it, and for an
/// imported model, whose streaming is only known once it loads.
fn live_text_note(ui: &Ui) -> Option<String> {
    let cpu_engine = ui.platform.caps.os == super::platform::Os::Linux
        && ui.settings.transcription_device == crate::app_settings::TranscriptionDevice::Cpu;
    live_text_note_for(
        ui.settings.transcription.model,
        &ui.settings.transcription.language,
        cpu_engine,
    )
}

/// [`live_text_note`] over the model, language, and engine choice, so its
/// copy is tested without a window.
fn live_text_note_for(
    model: crate::transcription::models::TranscriptionModelId,
    language: &str,
    cpu_engine: bool,
) -> Option<String> {
    use crate::transcription::models::{MODELS, TranscriptionModelId, definition};
    // An imported model's catalog entry says it can't stream, but its GGUF is
    // only checked when it loads.
    if matches!(model, TranscriptionModelId::Custom(_)) {
        return None;
    }
    if definition(model).streams {
        return cpu_engine
            .then(|| tr("Live words need the GPU, and Speech engine is set to CPU").to_owned());
    }
    let streaming: Vec<&'static str> = MODELS
        .iter()
        .filter(|model| model.streams && model.available() && model.supports_language(language))
        .map(|model| model.name)
        .collect();
    match *streaming.as_slice() {
        [first, second, ..] => Some(tr_named(
            "This speech model doesn't show live words; pick {first} or {second}",
            &[("first", first), ("second", second)],
        )),
        [only] => Some(tr_fill(
            "This speech model doesn't show live words; pick {}",
            only,
        )),
        [] => Some(tr("The selected speech model doesn't show live words").to_owned()),
    }
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
        Feature::FailureNotices => "lab-failure-notices",
        Feature::LiveTyping => "lab-live-typing",
        Feature::LiveChapters => "lab-live-chapters",
        Feature::VocabularyBiasing => "lab-vocabulary-biasing",
        Feature::ScreenVocabulary => "lab-screen-vocabulary",
        Feature::TerminalPaste => "lab-terminal-paste",
        Feature::KeyboardModelSwitching => "lab-keyboard-model-switching",
        Feature::AppModelSwitching => "lab-app-model-switching",
        Feature::MouseShortcuts => "lab-mouse-shortcuts",
        Feature::PauseBrowser => "lab-pause-browser",
        Feature::ShareShortcut => "lab-share-shortcut",
        Feature::UnloadIdle => "lab-unload-idle",
        Feature::KeepOnClipboard => "lab-keep-on-clipboard",
        Feature::CopyOnly => "lab-copy-only",
        Feature::ReturnToApp => "lab-return-to-app",
        Feature::SpokenFormatting => "lab-spoken-formatting",
        Feature::FillerWords => "lab-filler-words",
        Feature::CleanUpWithAppleIntelligence => "lab-clean-up",
        Feature::SubtitleExport => "lab-subtitle-export",
        Feature::SecondDictationShortcut => "lab-second-dictation-shortcut",
        Feature::MoreDictationShortcuts => "lab-more-dictation-shortcuts",
        Feature::TerminalPasteShortcut => "lab-terminal-paste-shortcut",
        Feature::SmartSpacing => "lab-smart-spacing",
        Feature::SpokenSymbols => "lab-spoken-symbols",
        Feature::PickerSortFilter => "lab-picker-sort-filter",
        Feature::SidedChords => "lab-sided-modifier-chords",
        Feature::TapOrHold => "lab-tap-or-hold",
        Feature::DoubleTapModifiers => "lab-double-tap-modifiers",
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
        Feature::KQuantModels => "lab-k-quant-models",
        Feature::PickerDownloaded => "lab-picker-downloaded",
        Feature::SectionKeyAlone => "lab-section-key",
        Feature::KeepRecordings => "lab-keep-recordings",
        Feature::HistoryMultiSelect => "lab-history-multi-select",
        Feature::SettingsSearch => "lab-settings-search",
        Feature::Appearance => "lab-appearance",
        Feature::ReleaseNotesLink => "lab-release-notes-link",
        Feature::HideMenuBarIcon => "lab-hide-menu-bar-icon",
        Feature::PauseMenu => "lab-pause-menu",
        Feature::QuitClosesWindow => "lab-quit-closes-window",
        Feature::CapsLockDictation => "lab-caps-lock-dictation",
        Feature::MovableMeetingCapsules => "lab-movable-meeting-capsules",
        Feature::GnomeClipboardRestore => "lab-gnome-clipboard-restore",
        Feature::TypedText => "lab-typed-text",
        Feature::MeetingNotesVocabulary => "lab-meeting-notes-vocabulary",
        Feature::NotchIndicator => "lab-notch-indicator",
        Feature::MeetingFinishedEvent => "lab-meeting-finished-event",
        Feature::GnomeMeetingCapsules => "lab-gnome-meeting-capsules",
        Feature::GnomeAnnotate => "lab-gnome-annotate",
        Feature::Screenshots => "lab-screenshots",
        Feature::FollowLiveTranscript => "lab-follow-live-transcript",
        Feature::MoreCallApps => "lab-more-call-apps",
        Feature::AnnotateReplay => "lab-annotate-replay",
        Feature::CopyCaptureZip => "lab-copy-capture-zip",
        Feature::MeetingImport => "lab-meeting-import",
        Feature::MeetingDescriptiveFilenames => "lab-meeting-descriptive-filenames",
        Feature::MeetingNotesFile => "lab-meeting-notes-file",
        Feature::ExpireMeetingAudio => "lab-expire-meeting-audio",
        Feature::OpenCodeInWsl => "lab-opencode-in-wsl",
        Feature::ShellCompletions => "lab-shell-completions",
        Feature::YoloMode => "lab-yolo-mode",
        Feature::ApiProcessing => "lab-api-processing",
        Feature::ApiTimestamps => "lab-api-timestamps",
        Feature::ApiBrowserAccess => "lab-api-browser-access",
        Feature::SpeakersOnly => "lab-speakers-only",
        Feature::KeepMicrophoneOpen => "lab-keep-microphone-open",
        Feature::PrintStart => "lab-print-start",
        Feature::VoicePrompt => "lab-voice-prompt",
        Feature::CursorContext => "lab-cursor-context",
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
        // Its selects live only in the Modes pane, and they close with it.
        Feature::AppModelSwitching => ui.open = None,
        // The card's duration menu closes when its lab does.
        Feature::KeepMicrophoneOpen => ui.open = None,
        // Its menu and application picker close with it.
        Feature::YoloMode => {
            ui.open = None;
            if ui.modes.picker_target == super::modes::PickerTarget::YoloMode {
                ui.modes.application_picker_open = false;
            }
        }
        // The phrase field starts at the saved phrase, and an invalid draft
        // leaves with its lab.
        Feature::VoicePrompt => {
            let saved = ui.settings.labs.voice_prompt_phrase.clone();
            ui.lab_store.sync_phrase(&saved);
        }
        // Its origins field starts at (and shows) the saved origins.
        Feature::ApiBrowserAccess => {
            let saved = ui.settings.labs.api_browser_origins.clone();
            ui.lab_store.sync_origins(&saved);
        }
        // A notice already on screen leaves with its lab.
        Feature::FailureNotices if !on => crate::rune_ui::live_text::dismiss_failure(),
        // Hex hears the browser through System Audio Recording.
        Feature::PauseBrowser if on && !ui.platform.system_audio => {
            ui.ask(super::platform::PlatformRequest::SystemAudio);
        }
        _ => {}
    }
}

/// A lab's own settings, on its card while it's on: how long the model sits
/// unused before it unloads, or the screenshot shortcut and folder.
fn extras(ui: &Ui, p: Paint, entry: &Row, layer: &mut Vec<El>) -> Option<El> {
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
                .child(unload_after(ui, p, layer)),
        ),
        Lab::Feature(Feature::KeepMicrophoneOpen) => Some(
            row()
                .key("keep-microphone-open-row")
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(
                    text(tr("Keep open for"))
                        .key("label")
                        .font(controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(keep_microphone_open_for(ui, p, layer)),
        ),
        Lab::Feature(Feature::YoloMode) => Some(yolo_mode::body(ui, p, layer)),
        Lab::Feature(Feature::CapsLockDictation) => Some(
            row()
                .key("caps-lock-light-row")
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(
                    text(tr("Recording light"))
                        .key("label")
                        .font(controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(caps_lock_light(ui, p)),
        ),
        Lab::Feature(Feature::Appearance) => Some(
            row()
                .key("appearance-theme-row")
                .gap(8.0)
                .center_y()
                .flex_wrap()
                .child(
                    text(tr("Theme"))
                        .key("label")
                        .font(controls::DESCRIPTION)
                        .color(p.muted()),
                )
                .child(appearance_theme(ui, p)),
        ),
        Lab::Feature(Feature::VoicePrompt) => Some(voice_prompt::body(ui, p)),
        Lab::Feature(Feature::ApiBrowserAccess) => Some(browser_access::body(ui, p)),
        #[cfg(all(target_os = "macos", hex_capture))]
        Lab::Feature(Feature::Screenshots) => Some(screenshots::body(
            ui,
            p,
            crate::screenshots::system_takes_shortcut(),
        )),
        _ => None,
    }
}

/// Whether the Caps Lock light stays solid or blinks slowly while recording.
fn caps_lock_light(ui: &Ui, p: Paint) -> El {
    use crate::app_settings::CapsLockLight;
    let labels: Vec<&str> = CapsLockLight::ALL
        .iter()
        .map(|light| tr(light.name()))
        .collect();
    let selected = CapsLockLight::ALL
        .iter()
        .position(|light| *light == ui.settings.labs.caps_lock_light)
        .unwrap_or(0);
    controls::segmented(p, "caps-lock-light", &labels, selected, |ui, index| {
        let light = CapsLockLight::ALL[index];
        ui.update(|settings| settings.labs.caps_lock_light = light);
    })
}

/// Which appearance the main window shows instead of the system's.
fn appearance_theme(ui: &Ui, p: Paint) -> El {
    use crate::app_settings::AppearanceTheme;
    let labels: Vec<&str> = AppearanceTheme::ALL
        .iter()
        .map(|theme| tr(theme.name()))
        .collect();
    let selected = AppearanceTheme::ALL
        .iter()
        .position(|theme| *theme == ui.settings.labs.appearance_theme)
        .unwrap_or(0);
    controls::segmented(p, "appearance-theme", &labels, selected, |ui, index| {
        let theme = AppearanceTheme::ALL[index];
        ui.update(|settings| settings.labs.appearance_theme = theme);
    })
}

/// How long the model sits unused before it unloads.
fn unload_after(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    use crate::app_settings::UNLOAD_IDLE_MINUTES;
    let labels: Vec<String> = UNLOAD_IDLE_MINUTES
        .iter()
        .map(|minutes| tr_fill("{} min", &minutes.to_string()))
        .collect();
    let selected = UNLOAD_IDLE_MINUTES
        .iter()
        .position(|minutes| *minutes == ui.settings.labs.unload_idle_minutes)
        .unwrap_or(1);
    super::general::quiet_select(
        ui,
        p,
        super::Menu::Pick("unload-idle-minutes"),
        "unload-idle-minutes".to_owned(),
        tr("Unload after"),
        labels,
        selected,
        |ui, index| {
            let minutes = UNLOAD_IDLE_MINUTES[index];
            ui.update(|settings| settings.labs.unload_idle_minutes = minutes);
        },
        layer,
    )
}

/// How long the microphone stays open after a dictation before it is
/// released. Five choices no longer fit a segmented control on a half-width
/// card, so the choice opens in a menu.
fn keep_microphone_open_for(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    use crate::app_settings::KEEP_MICROPHONE_OPEN_SECONDS;
    let labels: Vec<String> = KEEP_MICROPHONE_OPEN_SECONDS
        .iter()
        .map(|seconds| {
            if *seconds < 60 {
                tr_fill("{} s", &seconds.to_string())
            } else {
                tr_fill("{} min", &(seconds / 60).to_string())
            }
        })
        .collect();
    let selected = KEEP_MICROPHONE_OPEN_SECONDS
        .iter()
        .position(|seconds| *seconds == ui.settings.labs.keep_microphone_open_seconds)
        .unwrap_or(1);
    super::general::quiet_select(
        ui,
        p,
        super::Menu::Pick("keep-microphone-open"),
        "keep-microphone-open-seconds".to_owned(),
        tr("Keep open for"),
        labels,
        selected,
        |ui, index| {
            let seconds = KEEP_MICROPHONE_OPEN_SECONDS[index];
            ui.update(|settings| settings.labs.keep_microphone_open_seconds = seconds);
        },
        layer,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::models::{CustomModelId, TranscriptionModelId};

    fn custom_model() -> TranscriptionModelId {
        TranscriptionModelId::Custom(
            CustomModelId::parse("custom-0123456789abcdef").expect("a custom id"),
        )
    }

    /// The Live text card names the catalog's streaming models instead of
    /// hardcoding them, filtered to the selected language.
    #[test]
    fn live_text_says_which_models_show_live_words() {
        let note = live_text_note_for(TranscriptionModelId::ParakeetV3, "en", false).unwrap();
        let streaming: Vec<&str> = crate::transcription::models::MODELS
            .iter()
            .filter(|model| model.streams && model.available() && model.supports_language("en"))
            .map(|model| model.name)
            .collect();
        for name in streaming.iter().take(2) {
            assert!(note.contains(*name), "{note:?} names {name}");
        }
        assert_eq!(
            live_text_note_for(TranscriptionModelId::ParakeetUnifiedEnglish, "en", false),
            None
        );
    }

    /// With no streaming model for the saved language, the card says so
    /// without suggesting one that can't run it.
    #[test]
    fn live_text_suggests_no_model_for_another_language() {
        let note = live_text_note_for(TranscriptionModelId::ParakeetV3, "he", false).unwrap();
        assert_eq!(
            note,
            tr("The selected speech model doesn't show live words")
        );
    }

    /// An imported model's catalog entry says it can't stream, but its file is
    /// only checked when it loads, so its card claims nothing.
    #[test]
    fn live_text_never_judges_an_imported_model() {
        assert_eq!(live_text_note_for(custom_model(), "en", false), None);
        assert_eq!(live_text_note_for(custom_model(), "en", true), None);
    }

    /// A streaming model on a CPU engine still shows no live words, and the
    /// card says which setting to change.
    #[test]
    fn live_text_says_when_the_cpu_engine_stops_the_words() {
        let note = live_text_note_for(TranscriptionModelId::ParakeetUnifiedEnglish, "en", true)
            .expect("a note");
        assert!(note.contains(tr("Speech engine")), "{note:?}");
        assert!(note.to_lowercase().contains("gpu"), "{note:?}");
    }
}
