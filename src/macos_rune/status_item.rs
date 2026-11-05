use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use color_eyre::eyre::{Result, eyre};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSSize, NSString};

use crate::app_settings::{AppSettings, HotkeyBinding};
use crate::desktop::tray_menu::model_menu_entries;
use crate::transcription::models::{MODELS, TranscriptionModelId};
use crate::transcription::preparation::PreparationStatus;

const IDLE_ICON_PNG: &[u8] = include_bytes!("../../assets/icons/menubar-idle@2x.png");
const RECORDING_ICON_PNG: &[u8] = include_bytes!("../../assets/icons/menubar-recording@2x.png");
const STATUS_ICON_POINTS: f64 = 18.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusItemAction {
    OpenSettings,
    OpenModels,
    SelectModel(TranscriptionModelId),
    CancelModelPreparation,
    PasteLast,
    /// Copy History entry `id` (Labs › Copy recent dictations).
    CopyRecentDictation(u64),
    StartMeeting,
    StopMeeting,
    CopyLastMeetingTranscript,
    /// Copy chapter `n` of the recording meeting (Labs).
    CopyChapter(usize),
    /// The recording row: the Meetings pane.
    OpenMeetings,
    /// The update item: check, download, or restart into an update.
    Update,
    Quit,
}

#[derive(Debug)]
struct StatusItemTargetIvars {
    actions: SyncSender<StatusItemAction>,
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = StatusItemTargetIvars]
    struct StatusItemTarget;

    unsafe impl NSObjectProtocol for StatusItemTarget {}

    impl StatusItemTarget {
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::OpenSettings);
        }

        #[unsafe(method(openModels:))]
        fn open_models(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::OpenModels);
        }

        #[unsafe(method(selectModel:))]
        fn select_model(&self, sender: &NSMenuItem) {
            if let Some(model) = model_from_tag(sender.tag()) {
                let _ = self.ivars().actions.try_send(StatusItemAction::SelectModel(model));
            }
        }

        #[unsafe(method(cancelModelPreparation:))]
        fn cancel_model_preparation(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::CancelModelPreparation);
        }

        #[unsafe(method(pasteLast:))]
        fn paste_last(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::PasteLast);
        }

        #[unsafe(method(copyRecentDictation:))]
        fn copy_recent_dictation(&self, sender: &NSMenuItem) {
            if let Ok(id) = u64::try_from(sender.tag()) {
                let _ = self
                    .ivars()
                    .actions
                    .try_send(StatusItemAction::CopyRecentDictation(id));
            }
        }

        /// The status menu's delegate: it is about to open.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, _menu: &NSMenu) {
            refresh_recent_dictations();
        }

        #[unsafe(method(startMeeting:))]
        fn start_meeting(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::StartMeeting);
        }

        #[unsafe(method(stopMeeting:))]
        fn stop_meeting(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::StopMeeting);
        }

        #[unsafe(method(copyLastMeetingTranscript:))]
        fn copy_last_meeting_transcript(&self, _sender: &AnyObject) {
            let _ = self
                .ivars()
                .actions
                .try_send(StatusItemAction::CopyLastMeetingTranscript);
        }

        #[unsafe(method(copyChapter:))]
        fn copy_chapter(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                let _ = self.ivars().actions.try_send(StatusItemAction::CopyChapter(index));
            }
        }

        #[unsafe(method(openMeetings:))]
        fn open_meetings(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::OpenMeetings);
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: &AnyObject) {
            let _ = self
                .ivars()
                .actions
                .try_send(StatusItemAction::Update);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            let _ = self.ivars().actions.try_send(StatusItemAction::Quit);
        }
    }
);

impl StatusItemTarget {
    fn new(actions: SyncSender<StatusItemAction>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StatusItemTargetIvars { actions });
        unsafe { msg_send![super(this), init] }
    }
}

/// One row of the status menu, top to bottom: what's happening now (the
/// meeting group), then things to do, then the app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Row {
    MeetingStatus,
    StopMeeting,
    CopySection,
    RecordMeeting,
    MeetingSeparator,
    PasteLast,
    /// Labs › Copy recent dictations: hidden until the menu opens with it on.
    CopyRecentDictation,
    CopyLastMeetingTranscript,
    SpeechModel,
    Separator,
    Settings,
    Update,
    Quit,
}

const ROWS: [Row; 13] = [
    Row::MeetingStatus,
    Row::StopMeeting,
    Row::CopySection,
    Row::RecordMeeting,
    Row::MeetingSeparator,
    Row::PasteLast,
    Row::CopyRecentDictation,
    Row::CopyLastMeetingTranscript,
    Row::SpeechModel,
    Row::Separator,
    Row::Settings,
    Row::Update,
    Row::Quit,
];

impl Row {
    /// The title it is built with; `None` for a separator.
    fn title(self) -> Option<String> {
        let title = match self {
            Row::MeetingSeparator | Row::Separator => return None,
            Row::MeetingStatus => "Recording meeting",
            Row::StopMeeting => "Stop & Transcribe",
            Row::CopySection => "Copy Section",
            Row::RecordMeeting => "Record Meeting",
            Row::PasteLast => "Paste Last Dictation",
            Row::CopyRecentDictation => "Copy Recent Dictation",
            Row::CopyLastMeetingTranscript => "Copy Last Meeting Transcript",
            Row::SpeechModel => "Speech Model",
            Row::Settings => "Settings…",
            Row::Update => "Check for Updates…",
            Row::Quit => return Some(format!("Quit {}", crate::app_paths::app_name())),
        };
        Some(title.into())
    }

    fn action(self) -> Option<Sel> {
        Some(match self {
            Row::MeetingStatus => sel!(openMeetings:),
            Row::StopMeeting => sel!(stopMeeting:),
            Row::RecordMeeting => sel!(startMeeting:),
            Row::PasteLast => sel!(pasteLast:),
            Row::CopyLastMeetingTranscript => sel!(copyLastMeetingTranscript:),
            Row::Settings => sel!(openSettings:),
            Row::Update => sel!(checkForUpdates:),
            Row::Quit => sel!(quit:),
            Row::MeetingSeparator
            | Row::Separator
            | Row::CopySection
            | Row::CopyRecentDictation
            | Row::SpeechModel => return None,
        })
    }

    /// A fixed ⌘ key equivalent. Paste Last Dictation follows the saved
    /// binding instead ([`update_paste_last`]).
    fn command_key(self) -> Option<&'static str> {
        match self {
            Row::Settings => Some(","),
            Row::Quit => Some("q"),
            _ => None,
        }
    }

    /// Rows shown only by a meeting or a Lab, from the first refresh on.
    fn starts_hidden(self) -> bool {
        matches!(
            self,
            Row::MeetingStatus
                | Row::StopMeeting
                | Row::CopySection
                | Row::RecordMeeting
                | Row::MeetingSeparator
                | Row::CopyRecentDictation
                | Row::CopyLastMeetingTranscript
        )
    }
}

/// A menu item's key equivalent: the key and its modifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyEquivalent {
    key: String,
    modifiers: NSEventModifierFlags,
}

/// What Paste Last Dictation shows for the saved binding: nothing when it is
/// unbound, modifier-only, or a key a menu can't name (a mouse button).
fn paste_last_key(binding: Option<&HotkeyBinding>) -> Option<KeyEquivalent> {
    let binding = binding?;
    let key = menu_key(&binding.key.as_ref()?.label)?;
    let shortcut = binding.modifiers;
    let mut modifiers = NSEventModifierFlags::empty();
    for (held, flag) in [
        (shortcut.control.is_some(), NSEventModifierFlags::Control),
        (shortcut.option.is_some(), NSEventModifierFlags::Option),
        (shortcut.shift.is_some(), NSEventModifierFlags::Shift),
        (shortcut.command.is_some(), NSEventModifierFlags::Command),
        (shortcut.function, NSEventModifierFlags::Function),
    ] {
        if held {
            modifiers |= flag;
        }
    }
    Some(KeyEquivalent { key, modifiers })
}

/// The key equivalent string for a keycap label: one character in lowercase
/// (the modifier mask carries Shift), Space, or F1 to F20.
fn menu_key(label: &str) -> Option<String> {
    let mut characters = label.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        return Some(character.to_lowercase().collect());
    }
    if label == "Space" {
        return Some(" ".into());
    }
    let number = label.strip_prefix('F')?.parse::<u32>().ok()?;
    // NSF1FunctionKey and on.
    (1..=20)
        .contains(&number)
        .then(|| char::from_u32(0xF704 + number - 1))
        .flatten()
        .map(String::from)
}

struct StatusItemController {
    item: Retained<NSStatusItem>,
    idle_symbol: Retained<NSImage>,
    recording_symbol: Retained<NSImage>,
    meeting_status: Retained<NSMenuItem>,
    meeting_start: Retained<NSMenuItem>,
    meeting_stop: Retained<NSMenuItem>,
    meeting_copy_last: Retained<NSMenuItem>,
    meeting_sections: Retained<NSMenuItem>,
    meeting_sections_menu: Retained<NSMenu>,
    meeting_section_labels: RefCell<Vec<String>>,
    meeting_separator: Retained<NSMenuItem>,
    meeting_button: RefCell<(String, bool)>,
    paste_last: Retained<NSMenuItem>,
    paste_last_key: RefCell<Option<KeyEquivalent>>,
    recent: Retained<NSMenuItem>,
    recent_menu: Retained<NSMenu>,
    update: Option<Retained<NSMenuItem>>,
    update_title: RefCell<String>,
    _menu: Retained<NSMenu>,
    target: Retained<StatusItemTarget>,
    model_items: Vec<(TranscriptionModelId, Retained<NSMenuItem>)>,
    model_empty: Retained<NSMenuItem>,
    model_preparing: Retained<NSMenuItem>,
    model_cancel: Retained<NSMenuItem>,
    model_error: Retained<NSMenuItem>,
}

thread_local! {
    static STATUS_ITEM: RefCell<Option<StatusItemController>> = const { RefCell::new(None) };
}

pub fn install() -> Result<Receiver<StatusItemAction>> {
    let mtm =
        MainThreadMarker::new().ok_or_else(|| eyre!("status item requires the main thread"))?;
    let (actions, receiver) = sync_channel(8);
    let target = StatusItemTarget::new(actions, mtm);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Hex"));
    let meeting_sections_menu =
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Copy Section"));
    let recent_menu = NSMenu::initWithTitle(
        NSMenu::alloc(mtm),
        &NSString::from_str("Copy Recent Dictation"),
    );
    let models = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Speech Model"));
    models.setAutoenablesItems(false);
    let model_items = MODELS
        .iter()
        .enumerate()
        .filter(|(_, model)| model.available())
        .map(|(index, model)| {
            let item = add_item(&models, &target, model.name, sel!(selectModel:), mtm);
            item.setTag(index as isize);
            item.setHidden(true);
            (model.id, item)
        })
        .collect();
    let model_empty = add_item(
        &models,
        &target,
        "No downloaded models",
        sel!(openModels:),
        mtm,
    );
    model_empty.setEnabled(false);
    models.addItem(&NSMenuItem::separatorItem(mtm));
    let model_preparing = add_item(&models, &target, "Preparing model…", sel!(openModels:), mtm);
    model_preparing.setEnabled(false);
    model_preparing.setHidden(true);
    let model_cancel = add_item(
        &models,
        &target,
        "Cancel Model Switch",
        sel!(cancelModelPreparation:),
        mtm,
    );
    model_cancel.setHidden(true);
    let model_error = add_item(
        &models,
        &target,
        "Model switch failed — open Settings…",
        sel!(openModels:),
        mtm,
    );
    model_error.setHidden(true);
    add_item(&models, &target, "Manage Models…", sel!(openModels:), mtm);

    let update_managed = crate::updater::managed_by_policy();
    let mut rows = Vec::with_capacity(ROWS.len());
    for row in ROWS {
        let item = match (row, row.title()) {
            (_, None) => NSMenuItem::separatorItem(mtm),
            // An item without an action is shown disabled.
            (Row::Update, Some(_)) if update_managed => {
                plain_item(crate::updater::MANAGED_LABEL, mtm)
            }
            (Row::Update, Some(_)) => action_item(
                &target,
                &update_title(&crate::desktop::host::DesktopUpdateStatus::Checking),
                sel!(checkForUpdates:),
                mtm,
            ),
            (row, Some(title)) => match row.action() {
                Some(action) => action_item(&target, &title, action, mtm),
                None => {
                    let item = plain_item(&title, mtm);
                    match row {
                        Row::CopySection => item.setSubmenu(Some(&meeting_sections_menu)),
                        Row::CopyRecentDictation => item.setSubmenu(Some(&recent_menu)),
                        Row::SpeechModel => item.setSubmenu(Some(&models)),
                        _ => {}
                    }
                    item
                }
            },
        };
        if let Some(key) = row.command_key() {
            item.setKeyEquivalent(&NSString::from_str(key));
            item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
        }
        item.setHidden(row.starts_hidden());
        menu.addItem(&item);
        rows.push((row, item));
    }
    let row = |wanted: Row| {
        rows.iter()
            .find(|(row, _)| *row == wanted)
            .map(|(_, item)| item.clone())
            .expect("every row is built")
    };

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-2.0);
    let button = item
        .button(mtm)
        .ok_or_else(|| eyre!("status item button is unavailable"))?;
    let symbol = status_bar_icon(IDLE_ICON_PNG, "Hex")?;
    let recording_symbol = status_bar_icon(RECORDING_ICON_PNG, "Hex is recording a meeting")?;
    button.setImage(Some(&symbol));
    // SAFETY: the target answers `menuNeedsUpdate:` and outlives the menu's
    // weak delegate reference (both live in STATUS_ITEM for the app's life).
    unsafe {
        let _: () = msg_send![&*menu, setDelegate: &*target];
    }
    item.setMenu(Some(&menu));
    // Labs › Hide menu bar icon: the item stays installed, only hidden.
    item.setVisible(icon_shown(crate::labs::hide_menu_bar_icon(), false));

    STATUS_ITEM.with(|status_item| {
        *status_item.borrow_mut() = Some(StatusItemController {
            item,
            idle_symbol: symbol,
            recording_symbol,
            meeting_status: row(Row::MeetingStatus),
            meeting_start: row(Row::RecordMeeting),
            meeting_stop: row(Row::StopMeeting),
            meeting_copy_last: row(Row::CopyLastMeetingTranscript),
            meeting_sections: row(Row::CopySection),
            meeting_sections_menu,
            meeting_section_labels: RefCell::new(Vec::new()),
            meeting_separator: row(Row::MeetingSeparator),
            meeting_button: RefCell::new((String::new(), false)),
            paste_last: row(Row::PasteLast),
            paste_last_key: RefCell::new(None),
            recent: row(Row::CopyRecentDictation),
            recent_menu,
            update: (!update_managed).then(|| row(Row::Update)),
            update_title: RefCell::new("Check for Updates…".into()),
            _menu: menu,
            target,
            model_items,
            model_empty,
            model_preparing,
            model_cancel,
            model_error,
        });
    });
    Ok(receiver)
}

fn status_bar_icon(png: &[u8], accessibility_description: &str) -> Result<Retained<NSImage>> {
    let data = NSData::with_bytes(png);
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| eyre!("could not decode Hex status item icon"))?;
    image.setSize(NSSize::new(STATUS_ICON_POINTS, STATUS_ICON_POINTS));
    image.setTemplate(true);
    image.setAccessibilityDescription(Some(&NSString::from_str(accessibility_description)));
    Ok(image)
}

fn plain_item(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::new(),
        )
    }
}

fn action_item(
    target: &StatusItemTarget,
    title: &str,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::new(),
        )
    };
    unsafe { item.setTarget(Some(target)) };
    item
}

fn add_item(
    menu: &NSMenu,
    target: &StatusItemTarget,
    title: &str,
    action: Sel,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = action_item(target, title, action, mtm);
    menu.addItem(&item);
    item
}

fn model_from_tag(tag: isize) -> Option<TranscriptionModelId> {
    MODELS
        .get(usize::try_from(tag).ok()?)
        .filter(|model| model.available())
        .map(|model| model.id)
}

pub fn installed() -> bool {
    STATUS_ITEM.with(|controller| controller.borrow().is_some())
}

pub fn update_transcription(settings: &AppSettings, status: &PreparationStatus) {
    STATUS_ITEM.with(|controller| {
        let controller = controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        let entries = model_menu_entries(
            settings,
            status.model.is_some(),
            crate::transcription::models::is_installed,
        );
        for (model, item) in &controller.model_items {
            let entry = entries.iter().find(|entry| entry.model == *model);
            item.setHidden(entry.is_none());
            if let Some(entry) = entry {
                item.setTitle(&NSString::from_str(&entry.title));
                item.setState(if entry.selected { 1 } else { 0 });
                item.setEnabled(entry.enabled);
            }
        }
        controller.model_empty.setHidden(!entries.is_empty());
        controller.model_preparing.setHidden(status.model.is_none());
        controller.model_cancel.setHidden(status.model.is_none());
        if let Some(model) = status.model {
            controller
                .model_preparing
                .setTitle(&NSString::from_str(&format!(
                    "Preparing {}…",
                    crate::transcription::models::definition(model).name,
                )));
        }
        controller.model_error.setHidden(status.error.is_none());
    });
}

#[derive(Debug, Eq, PartialEq)]
struct MeetingMenu {
    status: Option<String>,
    button_title: String,
    recording: bool,
    can_start: bool,
    can_stop: bool,
    /// One Copy Section item per live chapter.
    sections: Vec<String>,
}

fn meeting_menu(state: &crate::meeting::watcher::MeetingMenuState, enabled: bool) -> MeetingMenu {
    use crate::meeting::watcher::MeetingMenuState;
    match state {
        MeetingMenuState::Idle => MeetingMenu {
            status: None,
            button_title: String::new(),
            recording: false,
            can_start: enabled,
            can_stop: false,
            sections: Vec::new(),
        },
        MeetingMenuState::Starting => MeetingMenu {
            status: Some("Starting meeting recording…".into()),
            button_title: String::new(),
            recording: true,
            can_start: false,
            can_stop: false,
            sections: Vec::new(),
        },
        MeetingMenuState::Recording {
            started,
            title,
            chapters,
            ..
        } => {
            let elapsed = crate::meeting::format_duration(started.elapsed().as_millis() as u64);
            MeetingMenu {
                status: Some(format!("Recording · {} · {elapsed}", menu_title(title))),
                button_title: format!(" {elapsed}"),
                recording: true,
                can_start: false,
                can_stop: true,
                sections: chapters
                    .iter()
                    .map(|chapter| {
                        format!(
                            "{} · {}",
                            crate::meeting::format_duration(chapter.start_ms),
                            menu_title(&chapter.title)
                        )
                    })
                    .collect(),
            }
        }
        MeetingMenuState::Transcribing => MeetingMenu {
            status: Some("Transcribing meeting…".into()),
            button_title: String::new(),
            recording: false,
            can_start: false,
            can_stop: false,
            sections: Vec::new(),
        },
    }
}

/// Whether the menu-bar icon shows: always, unless Labs › Hide menu bar
/// icon is on and no meeting is under way.
fn icon_shown(hide: bool, meeting_active: bool) -> bool {
    !hide || meeting_active
}

/// A meeting title short enough for one menu line.
fn menu_title(title: &str) -> String {
    const MAX_CHARS: usize = 48;
    let title = title.trim();
    if title.chars().count() <= MAX_CHARS {
        return title.into();
    }
    let cut = title.chars().take(MAX_CHARS - 1).collect::<String>();
    format!("{}…", cut.trim_end())
}

/// How many dictations Copy Recent Dictation lists.
const RECENT_DICTATIONS: usize = 5;

/// A dictation on one menu line: whitespace and line breaks collapsed, then
/// cut like a meeting title.
fn recent_title(text: &str) -> String {
    menu_title(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Rebuilds Copy Recent Dictation as the menu opens. History is read only
/// while Labs › Copy recent dictations is on; the item stays hidden while
/// it is off, History is off, or nothing is listed.
fn refresh_recent_dictations() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let entries = if crate::labs::recent_dictations() {
        crate::macos_rune::runtime()
            .and_then(|runtime| runtime.history())
            .map(|history| history.recent(RECENT_DICTATIONS))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    STATUS_ITEM.with(|controller| {
        let controller = controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        controller.recent_menu.removeAllItems();
        let mut listed = false;
        for entry in &entries {
            let Ok(tag) = isize::try_from(entry.id) else {
                continue;
            };
            let item = add_item(
                &controller.recent_menu,
                &controller.target,
                &recent_title(&entry.final_text),
                sel!(copyRecentDictation:),
                mtm,
            );
            item.setTag(tag);
            listed = true;
        }
        controller.recent.setHidden(!listed);
    });
}

fn update_title(status: &crate::desktop::host::DesktopUpdateStatus) -> String {
    use crate::desktop::host::DesktopUpdateStatus;
    match status {
        DesktopUpdateStatus::ReadyToRestart => "Restart to Update".into(),
        DesktopUpdateStatus::Available => "Download Update…".into(),
        DesktopUpdateStatus::SwitchAvailable { channel, version } => {
            format!("{}…", crate::updater::switch_label(channel, version))
        }
        _ => "Check for Updates…".into(),
    }
}

/// Shows the saved Paste Last binding beside Paste Last Dictation, or
/// nothing when it is unbound.
pub fn update_paste_last(settings: &AppSettings) {
    let key = paste_last_key(settings.paste_last_hotkey.as_ref());
    STATUS_ITEM.with(|controller| {
        let controller = controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        if *controller.paste_last_key.borrow() == key {
            return;
        }
        let (equivalent, modifiers) = key
            .as_ref()
            .map(|key| (key.key.as_str(), key.modifiers))
            .unwrap_or(("", NSEventModifierFlags::empty()));
        controller
            .paste_last
            .setKeyEquivalent(&NSString::from_str(equivalent));
        controller
            .paste_last
            .setKeyEquivalentModifierMask(modifiers);
        controller.paste_last_key.replace(key);
    });
}

/// Retitles the update item after the updater's status, so an update ready
/// or available stays reachable from the menu bar.
pub fn update_software(status: &crate::desktop::host::DesktopUpdateStatus) {
    let title = update_title(status);
    STATUS_ITEM.with(|controller| {
        let controller = controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        if let Some(item) = &controller.update
            && *controller.update_title.borrow() != title
        {
            item.setTitle(&NSString::from_str(&title));
            controller.update_title.replace(title);
        }
    });
}

/// Mirrors the meeting recorder into the menu bar so an active recording
/// stays visible and stoppable after the Settings window closes.
pub fn update_meeting(state: &crate::meeting::watcher::MeetingMenuState, enabled: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    STATUS_ITEM.with(|controller| {
        let controller = controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        let menu = meeting_menu(state, enabled);
        // A meeting starting, recording, or transcribing keeps the icon shown
        // so it stays visible and stoppable.
        let shown = icon_shown(crate::labs::hide_menu_bar_icon(), menu.status.is_some());
        if controller.item.isVisible() != shown {
            controller.item.setVisible(shown);
        }
        let visible = enabled || menu.status.is_some();
        controller.meeting_separator.setHidden(!visible);
        controller.meeting_copy_last.setHidden(!visible);
        controller.meeting_start.setHidden(!menu.can_start);
        controller.meeting_stop.setHidden(!menu.can_stop);
        controller.meeting_status.setHidden(menu.status.is_none());
        controller
            .meeting_sections
            .setHidden(menu.sections.is_empty());
        let mut labels = controller.meeting_section_labels.borrow_mut();
        if *labels != menu.sections {
            controller.meeting_sections_menu.removeAllItems();
            for (index, label) in menu.sections.iter().enumerate() {
                let item = add_item(
                    &controller.meeting_sections_menu,
                    &controller.target,
                    label,
                    sel!(copyChapter:),
                    mtm,
                );
                item.setTag(index as isize);
            }
            labels.clone_from(&menu.sections);
        }
        drop(labels);
        if let Some(status) = &menu.status {
            controller
                .meeting_status
                .setTitle(&NSString::from_str(status));
        }
        let mut button_state = controller.meeting_button.borrow_mut();
        if (*button_state != (menu.button_title.clone(), menu.recording))
            && let Some(button) = controller.item.button(mtm)
        {
            button.setTitle(&NSString::from_str(&menu.button_title));
            let symbol = if menu.recording {
                &controller.recording_symbol
            } else {
                &controller.idle_symbol
            };
            button.setImage(Some(symbol));
            *button_state = (menu.button_title, menu.recording);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meeting_menu_offers_one_action_per_state() {
        use crate::meeting::watcher::MeetingMenuState;
        let idle = meeting_menu(&MeetingMenuState::Idle, true);
        assert!(idle.can_start && !idle.can_stop && idle.status.is_none());
        assert!(!meeting_menu(&MeetingMenuState::Idle, false).can_start);
        let started = std::time::Instant::now() - std::time::Duration::from_secs(754);
        let recording_state = |title: &str, chapters| MeetingMenuState::Recording {
            started,
            id: "1".into(),
            title: title.into(),
            chapters,
        };
        let recording = meeting_menu(&recording_state("Zoom call — 2:30 PM", Vec::new()), false);
        assert_eq!(
            recording.status.as_deref(),
            Some("Recording · Zoom call — 2:30 PM · 12:34")
        );
        let long = meeting_menu(
            &recording_state(
                "A very long provisional title that keeps going past the limit",
                Vec::new(),
            ),
            false,
        );
        assert_eq!(
            long.status.as_deref(),
            Some("Recording · A very long provisional title that keeps going… · 12:34")
        );
        assert_eq!(recording.button_title, " 12:34");
        assert!(recording.can_stop && !recording.can_start);
        assert!(recording.sections.is_empty());
        let chapter = |title: &str, start_ms| crate::meeting::Chapter {
            title: title.into(),
            summary: String::new(),
            start_ms,
        };
        let chaptered = meeting_menu(
            &recording_state(
                "Team sync",
                vec![chapter("Image storage", 0), chapter("Motion lint", 190_000)],
            ),
            false,
        );
        assert_eq!(
            chaptered.sections,
            ["00:00 · Image storage", "03:10 · Motion lint"]
        );
        let transcribing = meeting_menu(&MeetingMenuState::Transcribing, true);
        assert!(!transcribing.can_start && !transcribing.can_stop);
        assert_eq!(
            transcribing.status.as_deref(),
            Some("Transcribing meeting…")
        );
    }

    #[test]
    fn the_menu_reads_meeting_then_actions_then_the_app() {
        let titles = ROWS
            .iter()
            .map(|row| row.title().unwrap_or_else(|| "—".into()))
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            [
                "Recording meeting".to_string(),
                "Stop & Transcribe".into(),
                "Copy Section".into(),
                "Record Meeting".into(),
                "—".into(),
                "Paste Last Dictation".into(),
                "Copy Recent Dictation".into(),
                "Copy Last Meeting Transcript".into(),
                "Speech Model".into(),
                "—".into(),
                "Settings…".into(),
                "Check for Updates…".into(),
                format!("Quit {}", crate::app_paths::app_name()),
            ]
        );
        assert_eq!(Row::Settings.command_key(), Some(","));
        assert_eq!(Row::Quit.command_key(), Some("q"));
        // Every row with neither a submenu nor a separator does something.
        for row in ROWS {
            let opens_submenu = matches!(
                row,
                Row::CopySection | Row::CopyRecentDictation | Row::SpeechModel
            );
            assert_eq!(
                row.action().is_some(),
                row.title().is_some() && !opens_submenu,
                "{row:?}"
            );
        }
    }

    #[test]
    fn paste_last_shows_the_saved_binding_or_nothing() {
        let default = paste_last_key(Some(&HotkeyBinding::paste_last_default())).unwrap();
        assert_eq!(default.key, "v");
        assert_eq!(
            default.modifiers,
            NSEventModifierFlags::Option | NSEventModifierFlags::Shift
        );
        assert_eq!(paste_last_key(None), None);
        let modifier_only = HotkeyBinding {
            key: None,
            ..HotkeyBinding::paste_last_default()
        };
        assert_eq!(paste_last_key(Some(&modifier_only)), None);
        assert_eq!(menu_key("F12").as_deref(), Some("\u{F70F}"));
        assert_eq!(menu_key("Space").as_deref(), Some(" "));
        assert_eq!(menu_key(","), Some(",".into()));
        assert_eq!(menu_key("Mouse 4"), None);
        assert_eq!(menu_key("F99"), None);
    }

    #[test]
    fn a_hidden_icon_comes_back_while_a_meeting_is_under_way() {
        assert!(icon_shown(false, false));
        assert!(icon_shown(false, true));
        assert!(!icon_shown(true, false));
        assert!(icon_shown(true, true));
    }

    #[test]
    fn recent_dictations_show_on_one_short_line() {
        assert_eq!(
            recent_title("  Hello\nthere,\n\n  world  "),
            "Hello there, world"
        );
        let long = recent_title(
            "A long dictation that goes on\nand on well past what fits on one menu line",
        );
        assert_eq!(long, "A long dictation that goes on and on well past…");
        assert_eq!(long.chars().count(), 47);
    }

    #[test]
    fn menu_tags_only_resolve_available_catalog_models() {
        assert_eq!(model_from_tag(-1), None);
        assert_eq!(model_from_tag(MODELS.len() as isize), None);
        for (index, model) in MODELS.iter().enumerate() {
            assert_eq!(
                model_from_tag(index as isize),
                model.available().then_some(model.id)
            );
        }
    }

    #[test]
    fn status_bar_icons_load_as_18pt_retina_templates() {
        for (png, description) in [
            (IDLE_ICON_PNG, "Hex"),
            (RECORDING_ICON_PNG, "Hex is recording a meeting"),
        ] {
            let icon = status_bar_icon(png, description).unwrap();
            assert!(icon.isTemplate());
            assert_eq!(
                icon.size(),
                NSSize::new(STATUS_ICON_POINTS, STATUS_ICON_POINTS)
            );
            assert_eq!(
                icon.accessibilityDescription().unwrap().to_string(),
                description
            );
            let rep = icon.representations().firstObject().unwrap();
            assert_eq!(rep.pixelsWide(), 36);
            assert_eq!(rep.pixelsHigh(), 36);
        }
    }
}
