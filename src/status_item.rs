use std::cell::RefCell;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use color_eyre::eyre::{Result, eyre};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};

use crate::app_settings::AppSettings;
use crate::transcription_models::{MODELS, ModelDefinition, TranscriptionModelId, language_name};
use crate::transcription_preparation::PreparationStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusItemAction {
    OpenSettings,
    OpenModels,
    SelectModel(TranscriptionModelId),
    CancelModelPreparation,
    PasteLast,
    CheckForUpdates,
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

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: &AnyObject) {
            let _ = self
                .ivars()
                .actions
                .try_send(StatusItemAction::CheckForUpdates);
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
    _item: Retained<NSStatusItem>,
    _menu: Retained<NSMenu>,
    _target: Retained<StatusItemTarget>,
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
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("HEX"));

    add_item(
        &menu,
        &target,
        "Paste Last Dictation",
        sel!(pasteLast:),
        mtm,
    );
    add_item(&menu, &target, "Settings…", sel!(openSettings:), mtm);
    let models = NSMenu::initWithTitle(
        NSMenu::alloc(mtm),
        &NSString::from_str("Transcription Model"),
    );
    models.setAutoenablesItems(false);
    let heading = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Transcription Model"),
            None,
            &NSString::new(),
        )
    };
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
    add_item(
        &menu,
        &target,
        "Check for Updates…",
        sel!(checkForUpdates:),
        mtm,
    );
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add_item(&menu, &target, "Quit HEX", sel!(quit:), mtm);

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-2.0);
    let button = item
        .button(mtm)
        .ok_or_else(|| eyre!("status item button is unavailable"))?;
    let symbol = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("hexagon"),
        Some(&NSString::from_str("HEX")),
    )
    .ok_or_else(|| eyre!("HEX status symbol is unavailable"))?;
    symbol.setTemplate(true);
    button.setImage(Some(&symbol));
    item.setMenu(Some(&menu));

    STATUS_ITEM.with(|status_item| {
        *status_item.borrow_mut() = Some(StatusItemController {
            _item: item,
            _menu: menu,
            _target: target,
            model_items,
            model_empty,
            model_preparing,
            model_cancel,
            model_error,
        });
    });
    Ok(receiver)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription_models::TranscriptionSelection;

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
}
