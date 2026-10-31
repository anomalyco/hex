//! The Linux tray icon, a Rune tray (StatusNotifierItem). A click shows the
//! window; the menu is the one `tray_menu` builds for Linux and Windows. A
//! desktop without a tray host (GNOME without an extension, i3bar) shows
//! nothing, and Hex runs on as before.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use crate::desktop::host::DesktopAction;
use crate::desktop::tray_menu::{TrayCommand, TrayView};
use crate::linux::app::{HostHandle, HostRequest, HostSnapshot};
use crate::linux::input::ShortcutCommand;
use crate::linux::service::{Activation, Control, Published};

/// The white Hex mark the Windows tray uses, raw 32×32 RGBA.
const ICON_RGBA: &[u8] = include_bytes!("../../resources/windows/tray-32.rgba");
const ICON_SIZE: u32 = 32;

/// Shows the tray icon until the returned tray drops.
pub(crate) fn start(
    host: HostHandle,
    published: Arc<Published>,
    control: SyncSender<Control>,
    shutdown: &'static AtomicBool,
) -> Option<rune::Tray> {
    let view = TrayView::default();
    let icon = rune::TrayIcon::rgba(ICON_SIZE, ICON_SIZE, ICON_RGBA.to_vec());
    let spec = rune::TraySpec::new("hex", "Hex", icon)
        .tooltip(view.tooltip())
        .menu(view.menu());
    let show = move |token| {
        let activation = Activation {
            token,
            startup_id: None,
        };
        let _ = control.try_send(Control::Activate(activation));
    };
    let tray = rune::Tray::new(spec, move |event| match event {
        rune::TrayEvent::Activate { token } => show(token),
        rune::TrayEvent::Menu(id) => match TrayCommand::from_id(&id) {
            Some(TrayCommand::Show) => show(None),
            // As the window's Quit Hex: drains accepted work, then exits.
            Some(TrayCommand::Quit) => shutdown.store(true, Ordering::Release),
            Some(command) => perform(&host, &published, command),
            None => {}
        },
        _ => {}
    });
    tray.inspect_err(|error| tracing::warn!(%error, "Hex has no tray icon"))
        .ok()
}

fn perform(host: &HostHandle, published: &Published, command: TrayCommand) {
    let snapshot = host.snapshot();
    let result = match command {
        TrayCommand::PasteLast => {
            let route = published.shortcuts.lock().unwrap().clone();
            crate::linux::service::route_shortcut(route.as_ref(), ShortcutCommand::PasteLast)
        }
        TrayCommand::UseModel(model) => host.request(HostRequest::ChooseModel {
            model,
            language: snapshot.settings.transcription_for_model(model).language,
        }),
        TrayCommand::Update => {
            match crate::desktop::tray_menu::update_action(&snapshot.desktop.update_status) {
                Some(action) => host.request(HostRequest::Desktop(action)),
                None => Ok(()),
            }
        }
        TrayCommand::ToggleListening => {
            host.request(HostRequest::Desktop(if snapshot.desktop.listener.running {
                DesktopAction::StopListening
            } else {
                DesktopAction::StartListening
            }))
        }
        TrayCommand::RecordMeeting => {
            crate::meeting::runtime::request(crate::meeting::MeetingRequest::Start { title: None })
                .map_err(|error| color_eyre::eyre::eyre!(error))
        }
        TrayCommand::StopMeeting => {
            crate::meeting::runtime::request(crate::meeting::MeetingRequest::Stop)
                .map_err(|error| color_eyre::eyre::eyre!(error))
        }
        TrayCommand::CopyLastMeeting => crate::meeting::runtime::copy_last_transcript()
            .map_err(|error| color_eyre::eyre::eyre!(error)),
        TrayCommand::Show | TrayCommand::Quit => Ok(()),
    };
    if let Err(error) = result {
        tracing::warn!(
            error = format!("{error:#}"),
            ?command,
            "the tray action failed"
        );
    }
}

/// What the tray shows for `snapshot`.
pub(crate) fn view(snapshot: &HostSnapshot) -> TrayView {
    let desktop = &snapshot.desktop;
    TrayView {
        meetings: crate::meeting::runtime::tray_state(),
        ..TrayView::new(
            desktop.listener.running,
            desktop.activity.state,
            &desktop.update_status,
            crate::desktop::tray_menu::model_menu_entries(
                &snapshot.settings,
                desktop.transcription.preparing.is_some(),
                crate::transcription::models::is_installed,
            ),
        )
    }
}

/// Shows `view` in `tray`.
pub(crate) fn show(tray: &rune::Tray, view: &TrayView) {
    tray.set_tooltip(view.tooltip());
    tray.set_menu(view.menu());
}
