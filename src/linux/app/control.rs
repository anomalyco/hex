//! The Linux adapter `hex` control commands from the local API run through
//! ([`crate::control::host`]).

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use crate::app_settings::AppSettings;
use crate::control::{CaptureAction, ControlError, ControlReply, Pane};
use crate::desktop::host::DesktopAction;
use crate::linux::input::{ListenerCommand, ShortcutCommand};
use crate::linux::service::{self as linux_service, Control, Published, ShortcutRoute};

use super::{HostHandle, HostRequest};

pub(super) struct LinuxControl {
    pub(super) host: HostHandle,
    pub(super) published: std::sync::Weak<Published>,
    pub(super) window: mpsc::SyncSender<Control>,
}

impl LinuxControl {
    fn published(&self) -> std::result::Result<Arc<Published>, ControlError> {
        self.published
            .upgrade()
            .ok_or_else(|| ControlError::new("unavailable", "Hex is quitting"))
    }

    fn route(&self) -> std::result::Result<ShortcutRoute, ControlError> {
        self.published()?
            .shortcuts
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                ControlError::new(
                    "not-ready",
                    "Hex is not listening; enable it in Settings or run `hex start`",
                )
            })
    }

    fn shortcut(&self, command: ShortcutCommand) -> std::result::Result<(), ControlError> {
        linux_service::route_shortcut(Some(&self.route()?), command)
            .map_err(|error| ControlError::new("busy", format!("{error:#}")))
    }

    fn window(&self, control: Control) -> std::result::Result<(), ControlError> {
        self.window
            .try_send(control)
            .map_err(|_| ControlError::new("busy", "Hex is busy; try again"))
    }
}

impl crate::control::host::Host for LinuxControl {
    fn state(&self) -> std::result::Result<crate::control::host::HostState, ControlError> {
        let status = self
            .published()?
            .status
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ControlError::new("not-ready", "Hex is starting; try again"))?;
        Ok(crate::control::host::HostState {
            capture: crate::control::host::capture_from_state(status.desktop.activity.state),
            listener: status.desktop.listener,
            transcription: status.desktop.transcription,
            update: status.desktop.update_status,
            settings: status.settings,
            window_open: status.window_open,
        })
    }

    fn dictation(
        &self,
        action: CaptureAction,
        print: Option<crate::control::PrintReply>,
        raw: bool,
    ) -> std::result::Result<(), ControlError> {
        if let Some(print) = print {
            return self
                .route()?
                .commands
                .try_send(ListenerCommand::StopAndPrint(print))
                .map_err(|_| ControlError::new("busy", "Hex is busy; try again"));
        }
        self.shortcut(match (action, raw) {
            (CaptureAction::Start, false) => ShortcutCommand::DictationStart,
            (CaptureAction::Start, true) => ShortcutCommand::DictationStartRaw,
            (CaptureAction::Toggle, false) => ShortcutCommand::DictationToggle,
            (CaptureAction::Toggle, true) => ShortcutCommand::DictationToggleRaw,
            (CaptureAction::Stop, _) => ShortcutCommand::DictationStop,
            (CaptureAction::Cancel, _) => ShortcutCommand::DictationCancel,
        })
    }

    fn paste_last(&self) -> std::result::Result<(), ControlError> {
        self.shortcut(ShortcutCommand::PasteLast)
    }

    fn save(&self, settings: AppSettings) -> std::result::Result<(), ControlError> {
        self.host
            .save_settings(settings)
            .map_err(|error| ControlError::new("internal", format!("couldn't save: {error:#}")))
    }

    fn history(&self) -> Option<crate::history::History> {
        self.host.history()
    }

    fn show_window(&self, pane: Option<Pane>) -> std::result::Result<(), ControlError> {
        if let Some(why) = self.host.window_failure() {
            return Err(ControlError::new(
                "unavailable",
                super::window_failure_message(&why),
            ));
        }
        self.window(Control::Show(pane))
    }

    fn hide_window(&self) -> bool {
        self.host.window_open() && self.window(Control::Hide).is_ok()
    }

    fn onboarding_reset(&self) -> ControlReply {
        let (sender, receiver) = mpsc::sync_channel(1);
        if let Err(error) = self.window(Control::OnboardingReset(sender)) {
            return error.into();
        }
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => ControlReply::done("Opened onboarding"),
            Ok(Err(why)) => ControlReply::error("unsupported", why),
            Err(_) => ControlReply::error("unavailable", "the window didn't answer; try again"),
        }
    }

    fn choose_model(
        &self,
        selection: crate::transcription::models::TranscriptionSelection,
    ) -> std::result::Result<(), ControlError> {
        self.host
            .request(HostRequest::ChooseModel {
                model: selection.model,
                language: selection.language,
            })
            .map_err(|error| ControlError::new("failed", format!("{error:#}")))
    }

    fn cancel_model(&self) {
        let _ = self.host.request(HostRequest::CancelModel);
    }

    fn check_for_updates(&self) -> std::result::Result<(), ControlError> {
        self.host
            .request(HostRequest::Desktop(DesktopAction::CheckForUpdates))
            .map_err(|error| ControlError::new("unavailable", format!("{error:#}")))
    }

    fn launch_at_login(&self, enabled: Option<bool>) -> Option<std::result::Result<bool, String>> {
        let format = |error: color_eyre::Report| format!("{error:#}");
        Some(match enabled {
            None => crate::linux::login_item::is_enabled().map_err(format),
            Some(enabled) => crate::linux::login_item::set_enabled(enabled)
                .and_then(|()| crate::linux::login_item::is_enabled())
                .map_err(format),
        })
    }

    fn quit(&self, restart: bool) -> std::result::Result<(), ControlError> {
        // A unit restarts by itself; otherwise `hex restart` reopens Hex
        // once this one has drained.
        if restart && linux_service::supervised() {
            return linux_service::schedule_restart()
                .map_err(|error| ControlError::new("failed", format!("{error:#}")));
        }
        self.host.quit();
        Ok(())
    }

    fn shortcut_rules(&self) -> crate::control::settings::ShortcutRules {
        crate::rune_ui::shortcut_rules()
    }

    fn annotate(&self, command: crate::control::ControlCommand) -> ControlReply {
        match command {
            crate::control::ControlCommand::ScrawlCapture {
                action: crate::control::ScrawlCaptureAction::Status,
            } => linux_service::capture_status(),
            crate::control::ControlCommand::ScrawlCapture { action } => {
                match crate::capture_host::send_to_listener(
                    crate::capture_host::CaptureControl::Scrawl(action),
                ) {
                    Ok(()) => ControlReply::done("Sent to Hex"),
                    Err(error) => ControlReply::error("failed", format!("{error:#}")),
                }
            }
            _ => crate::control::host::unsupported(
                "run this `hex annotate` command directly; it reads the captures folder itself",
            ),
        }
    }
}
