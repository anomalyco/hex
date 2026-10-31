use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use color_eyre::eyre::{Result, eyre};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSSize, NSString};

use crate::app_settings::AppSettings;
use crate::transcription_models::{MODELS, ModelDefinition, TranscriptionModelId, language_name};
use crate::transcription_preparation::PreparationStatus;

const IDLE_ICON_PNG: &[u8] = include_bytes!("../assets/icons/menubar-idle@2x.png");
const RECORDING_ICON_PNG: &[u8] = include_bytes!("../assets/icons/menubar-recording@2x.png");
const STATUS_ICON_POINTS: f64 = 18.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusItemAction {
    OpenSettings,
    OpenModels,
    SelectModel(TranscriptionModelId),
    CancelModelPreparation,
    PasteLast,
    StartMeeting,
    StopMeeting,
    CopyLastMeetingTranscript,
    /// Copy chapter `n` of the recording meeting (Labs).
    CopyChapter(usize),
    OpenMeetings,
    /// Flips `capture.show_overlays_in_recordings`.
    #[cfg(hex_capture)]
    ToggleOverlaysInRecordings,
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

        #[unsafe(method(toggleOverlaysInRecordings:))]
        fn toggle_overlays_in_recordings(&self, _sender: &AnyObject) {
            #[cfg(hex_capture)]
            let _ = self
                .ivars()
                .actions
                .try_send(StatusItemAction::ToggleOverlaysInRecordings);
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
    meeting_open: Retained<NSMenuItem>,
    meeting_separator: Retained<NSMenuItem>,
    meeting_button: RefCell<(String, bool)>,
    update: Option<Retained<NSMenuItem>>,
    update_title: RefCell<String>,
    /// A checkmark for `capture.show_overlays_in_recordings`, in builds
    /// that can capture with the experiment on.
    #[cfg(hex_capture)]
    overlays_in_recordings: Retained<NSMenuItem>,
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
    let meeting_status = add_item(
        &menu,
        &target,
        "Recording meeting",
        sel!(openMeetings:),
        mtm,
    );
    let meeting_stop = add_item(&menu, &target, "Stop & Transcribe", sel!(stopMeeting:), mtm);
    let meeting_sections_menu =
        NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Copy Section"));
    let meeting_sections = plain_item("Copy Section", mtm);
    meeting_sections.setSubmenu(Some(&meeting_sections_menu));
    menu.addItem(&meeting_sections);
    let meeting_start = add_item(&menu, &target, "Record Meeting", sel!(startMeeting:), mtm);
    let meeting_copy_last = add_item(
        &menu,
        &target,
        "Copy Last Meeting Transcript",
        sel!(copyLastMeetingTranscript:),
        mtm,
    );
    let meeting_open = add_item(&menu, &target, "Meetings…", sel!(openMeetings:), mtm);
    let meeting_separator = NSMenuItem::separatorItem(mtm);
    menu.addItem(&meeting_separator);
    for item in [
        &meeting_status,
        &meeting_stop,
        &meeting_sections,
        &meeting_start,
        &meeting_copy_last,
        &meeting_open,
        &meeting_separator,
    ] {
        item.setHidden(true);
    }

    add_item(
        &menu,
        &target,
        "Paste Last Dictation",
        sel!(pasteLast:),
        mtm,
    );
    add_item(&menu, &target, "Settings…", sel!(openSettings:), mtm);
    #[cfg(hex_capture)]
    let overlays_in_recordings = {
        let item = add_item(
            &menu,
            &target,
            "Show Hex in Screen Recordings",
            sel!(toggleOverlaysInRecordings:),
            mtm,
        );
        item.setState(isize::from(crate::capture_host::own_windows_recorded()));
        item.setHidden(!crate::dev_build::is_dev_build());
        item
    };
    let models = NSMenu::initWithTitle(
        NSMenu::alloc(mtm),
        &NSString::from_str("Transcription Model"),
    );
    models.setAutoenablesItems(false);
    let heading = plain_item("Transcription Model", mtm);
    heading.setSubmenu(Some(&models));
    menu.addItem(&heading);
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
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let update = if crate::updater::managed_by_policy() {
        // An item without an action is shown disabled.
        menu.addItem(&plain_item(crate::updater::MANAGED_LABEL, mtm));
        None
    } else {
        Some(add_item(
            &menu,
            &target,
            &update_title(&crate::desktop_host::DesktopUpdateStatus::Checking),
            sel!(checkForUpdates:),
            mtm,
        ))
    };
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(
        &menu,
        &target,
        &format!("Quit {}", crate::app_paths::app_name()),
        sel!(quit:),
        mtm,
    );

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-2.0);
    let button = item
        .button(mtm)
        .ok_or_else(|| eyre!("status item button is unavailable"))?;
    let symbol = status_bar_icon(IDLE_ICON_PNG, "Hex")?;
    let recording_symbol = status_bar_icon(RECORDING_ICON_PNG, "Hex is recording a meeting")?;
    button.setImage(Some(&symbol));
    item.setMenu(Some(&menu));

    STATUS_ITEM.with(|status_item| {
        *status_item.borrow_mut() = Some(StatusItemController {
            item,
            idle_symbol: symbol,
            recording_symbol,
            meeting_status,
            meeting_start,
            meeting_stop,
            meeting_copy_last,
            meeting_sections,
            meeting_sections_menu,
            meeting_section_labels: RefCell::new(Vec::new()),
            meeting_open,
            meeting_separator,
            meeting_button: RefCell::new((String::new(), false)),
            update,
            update_title: RefCell::new("Check for Updates…".into()),
            #[cfg(hex_capture)]
            overlays_in_recordings,
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

fn add_item(
    menu: &NSMenu,
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
    menu.addItem(&item);
    item
}

#[derive(Debug, Eq, PartialEq)]
struct ModelMenuEntry {
    model: TranscriptionModelId,
    title: String,
    selected: bool,
    enabled: bool,
}

fn model_from_tag(tag: isize) -> Option<TranscriptionModelId> {
    MODELS
        .get(usize::try_from(tag).ok()?)
        .filter(|model| model.available())
        .map(|model| model.id)
}

fn model_menu_entries(
    settings: &AppSettings,
    preparing: bool,
    installed: impl Fn(&ModelDefinition, &str) -> bool,
) -> Vec<ModelMenuEntry> {
    MODELS
        .iter()
        .filter(|model| model.available())
        .filter_map(|model| {
            let selection = settings.transcription_for_model(model.id);
            installed(model, &selection.language).then(|| ModelMenuEntry {
                model: model.id,
                title: format!("{} · {}", model.name, language_name(&selection.language)),
                selected: model.id == settings.transcription.model,
                enabled: !preparing && model.id != settings.transcription.model,
            })
        })
        .collect()
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
            crate::transcription_models::is_installed,
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
                    crate::transcription_models::definition(model).name,
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

fn meeting_menu(state: &crate::meeting_watcher::MeetingMenuState, enabled: bool) -> MeetingMenu {
    use crate::meeting_watcher::MeetingMenuState;
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

fn update_title(status: &crate::desktop_host::DesktopUpdateStatus) -> String {
    use crate::desktop_host::DesktopUpdateStatus;
    match status {
        DesktopUpdateStatus::ReadyToRestart => "Restart to Update".into(),
        DesktopUpdateStatus::Available => "Download Update…".into(),
        DesktopUpdateStatus::SwitchAvailable { channel, version } => {
            format!("{}…", crate::updater::switch_label(channel, version))
        }
        _ => "Check for Updates…".into(),
    }
}

/// Checks or unchecks Show Hex in Screen Recordings.
#[cfg(hex_capture)]
pub fn update_overlays_in_recordings(shown: bool) {
    STATUS_ITEM.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            controller
                .overlays_in_recordings
                .setState(isize::from(shown));
        }
    });
}

/// Retitles the update item after the updater's status, so an update ready
/// or available stays reachable from the menu bar.
pub fn update_software(status: &crate::desktop_host::DesktopUpdateStatus) {
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
pub fn update_meeting(state: &crate::meeting_watcher::MeetingMenuState, enabled: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    STATUS_ITEM.with(|controller| {
        let controller = controller.borrow();
        let Some(controller) = controller.as_ref() else {
            return;
        };
        let menu = meeting_menu(state, enabled);
        let visible = enabled || menu.status.is_some();
        controller.meeting_separator.setHidden(!visible);
        controller.meeting_copy_last.setHidden(!visible);
        controller.meeting_open.setHidden(!visible);
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
    use crate::transcription_models::TranscriptionSelection;

    #[test]
    fn meeting_menu_offers_one_action_per_state() {
        use crate::meeting_watcher::MeetingMenuState;
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
    fn model_menu_only_lists_downloaded_models_with_their_remembered_language() {
        let mut settings = AppSettings::default();
        settings.remember_transcription(TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
            recognition_hints: String::new(),
        });
        settings.remember_transcription(TranscriptionSelection::default());
        let installed = |model: &ModelDefinition, _: &str| {
            matches!(
                model.id,
                TranscriptionModelId::ParakeetUnifiedEnglish
                    | TranscriptionModelId::WhisperLargeV3Turbo
            )
        };
        let entries = model_menu_entries(&settings, false, installed);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Parakeet Unified English · English");
        assert!(entries[0].selected);
        assert!(!entries[0].enabled);
        assert_eq!(entries[1].title, "Whisper large-v3-turbo · French");
        assert!(!entries[1].selected);
        assert!(entries[1].enabled);
        let preparing = model_menu_entries(&settings, true, installed);
        assert!(preparing.iter().all(|entry| !entry.enabled));
        assert!(
            preparing[0].selected,
            "preparing must not mark the candidate selected"
        );
        assert!(model_menu_entries(&settings, false, |_, _| false).is_empty());
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
