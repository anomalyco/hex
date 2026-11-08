//! The Linux tray icon, a Rune tray (StatusNotifierItem). A click shows the
//! window; the menu is the one `tray_menu` builds for Linux and Windows. A
//! desktop without a tray host (GNOME without an extension, i3bar) shows
//! nothing, and Hex runs on as before.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use zbus::{Connection, OwnedVariant, Variant};

use crate::desktop::host::DesktopAction;
use crate::desktop::tray_icon;
use crate::desktop::tray_menu::{TrayCommand, TrayView};
use crate::linux::app::{HostHandle, HostRequest, HostSnapshot};
use crate::linux::input::ShortcutCommand;
use crate::linux::service::{Activation, Control, Published};

/// How long the tray waits for the desktop's color scheme when it is created.
const COLOR_SCHEME_TIMEOUT: Duration = Duration::from_millis(250);
const PORTAL_DESKTOP: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME: &str = "color-scheme";

/// Shows the tray icon until the returned tray drops.
pub(crate) fn start(
    host: HostHandle,
    published: Arc<Published>,
    control: SyncSender<Control>,
    shutdown: &'static AtomicBool,
) -> Option<rune::Tray> {
    let view = TrayView::default();
    let icon = rune::TrayIcon::rgba(
        tray_icon::SIZE,
        tray_icon::SIZE,
        tray_icon::pixels(desktop_prefers_light()),
    );
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

/// Whether the desktop prefers a light color scheme, read once when the tray
/// is created: the tray has no set_icon, so a later switch applies at the
/// next launch. Only an explicit light preference gets the black mark:
/// GNOME reports no preference while its top bar stays dark.
fn desktop_prefers_light() -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    let light = runtime.block_on(async {
        tokio::time::timeout(COLOR_SCHEME_TIMEOUT, async {
            let bus = Connection::session().await.ok()?;
            let value = match color_scheme(&bus, "ReadOne").await {
                Some(value) => value,
                // Older portals have no ReadOne; Read wraps the same value
                // in one more variant.
                None => color_scheme(&bus, "Read").await?,
            };
            Some(prefers_light(&value))
        })
        .await
    });
    matches!(light, Ok(Some(true)))
}

/// The portal's `color-scheme` value, read with `method`.
async fn color_scheme(bus: &Connection, method: &str) -> Option<OwnedVariant> {
    let reply = bus
        .call_method(
            Some(PORTAL_DESKTOP),
            PORTAL_PATH,
            Some(PORTAL_SETTINGS),
            method,
            &(APPEARANCE, COLOR_SCHEME),
        )
        .await
        .ok()?;
    reply.body().deserialize().ok()
}

/// Whether a portal `color-scheme` value prefers light: 2 is light, and 0 (no
/// preference) and 1 (prefer dark) keep the white mark. The deprecated `Read`
/// wraps the same value in one more variant.
fn prefers_light(value: &Variant<'_>) -> bool {
    fn scheme(value: &Variant<'_>) -> Option<u32> {
        match value {
            Variant::U32(scheme) => Some(*scheme),
            Variant::Variant(inner) => scheme(inner),
            _ => None,
        }
    }
    scheme(value) == Some(2)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_explicit_light_color_scheme_gets_the_dark_mark() {
        assert!(prefers_light(&Variant::from(2u32)));
        assert!(!prefers_light(&Variant::from(0u32)));
        assert!(!prefers_light(&Variant::from(1u32)));
        assert!(!prefers_light(&Variant::from(3u32)));
        assert!(!prefers_light(&Variant::Bool(true)));
        // The deprecated Read wraps the same value in one more variant.
        assert!(prefers_light(&Variant::Variant(Box::new(Variant::from(
            2u32
        )))));
    }
}
