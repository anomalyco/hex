//! The Linux tray icon, a Rune tray (StatusNotifierItem). A click shows the
//! window; the menu is the one `tray_menu` builds for Linux and Windows. A
//! desktop without a tray host (GNOME without an extension, i3bar) shows
//! nothing, and Hex runs on as before.
//!
//! Beside the IconPixmap, the item names Hex's symbolic icon when hicolor has
//! it, so a host that follows the panel's text color (Plasma 6, GNOME's
//! AppIndicator extension, Waybar) draws a mark that stays visible on any
//! panel. Hosts that don't know the name keep using the pixmap.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::time::error::Elapsed;
use zbus::{Connection, OwnedVariant, Variant};

use crate::desktop::host::DesktopAction;
use crate::desktop::tray_icon;
use crate::desktop::tray_menu::{TrayCommand, TrayView};
use crate::linux::app::{HostHandle, HostRequest, HostSnapshot};
use crate::linux::input::ShortcutCommand;
use crate::linux::service::{Activation, Control, Published};

/// How long the tray waits to reach the session bus when it is created.
const SESSION_BUS_TIMEOUT: Duration = Duration::from_millis(250);
/// How long it then waits for the desktop's color scheme.
const COLOR_SCHEME_TIMEOUT: Duration = Duration::from_millis(250);
/// How long it waits for KIconLoader's reload broadcast to leave.
const RELOAD_TIMEOUT: Duration = Duration::from_millis(250);
const PORTAL_DESKTOP: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME: &str = "color-scheme";
const ICON_LOADER_PATH: &str = "/KIconLoader";
const ICON_LOADER_INTERFACE: &str = "org.kde.KIconLoader";

/// Shows the tray icon until the returned tray drops.
pub(crate) fn start(
    host: HostHandle,
    published: Arc<Published>,
    control: SyncSender<Control>,
    shutdown: &'static AtomicBool,
) -> Option<rune::Tray> {
    let view = TrayView::default();
    // One runtime for the whole read and reload: zbus's connection keeps its
    // tasks on the runtime that created it, so a second runtime could not
    // drive the first one's connection.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok();
    let (light, bus) = match &runtime {
        Some(runtime) => appearance(runtime),
        None => (false, None),
    };
    // Before the tray exists: Plasma reads the name when the item registers.
    let symbolic = crate::linux::icons::install_tray_symbolic(light)
        .inspect_err(|error| tracing::warn!(%error, "could not install Hex's symbolic tray icon"))
        .unwrap_or_default();
    if symbolic.wrote
        && let (Some(runtime), Some(bus)) = (runtime.as_ref(), bus)
    {
        reload_icons(runtime, &bus);
    }
    drop(runtime);
    let icon = rune::TrayIcon::rgba(tray_icon::SIZE, tray_icon::SIZE, tray_icon::pixels(light));
    let mut spec = rune::TraySpec::new("hex", "Hex", icon)
        .tooltip(view.tooltip())
        .menu(view.menu());
    if symbolic.present {
        spec = spec.icon_name(crate::linux::icons::TRAY_ICON);
    }
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

/// The desktop's color-scheme preference and the session bus it was read on,
/// read once when the tray is created: the tray has no set_icon, so a later
/// switch applies at the next launch. It picks both the pixmap's mark and the
/// default color of the symbolic icon, so a host that does not recolor shows
/// the same mark. Only an explicit light preference gets the dark mark: GNOME
/// reports no preference while its top bar stays dark. Keeps the connection
/// when only the portal does not answer, so a reload can still use it.
fn appearance(runtime: &Runtime) -> (bool, Option<Connection>) {
    runtime.block_on(async {
        let Ok(Ok(bus)) = tokio::time::timeout(SESSION_BUS_TIMEOUT, Connection::session()).await
        else {
            return (false, None);
        };
        let light = tokio::time::timeout(COLOR_SCHEME_TIMEOUT, scheme(&bus))
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
        (light, Some(bus))
    })
}

/// Whether the portal's `color-scheme` prefers light, or `None` when the
/// portal does not answer.
async fn scheme(bus: &Connection) -> Option<bool> {
    let value = match color_scheme(bus, "ReadOne").await {
        Some(value) => value,
        // Older portals have no ReadOne; Read wraps the same value in one
        // more variant.
        None => color_scheme(bus, "Read").await?,
    };
    Some(prefers_light(&value))
}

/// Tells a running Plasma to reload its icons. KIconLoader lists the hicolor
/// folders that exist when the session starts, so it never looks in the
/// scalable folder Hex just created; without the broadcast its IconName draws
/// nothing until the next login. Emits on the connection's own runtime;
/// best-effort, so a failure must not unset the name.
fn reload_icons(runtime: &Runtime, bus: &Connection) {
    let sent = with_timeout(runtime, RELOAD_TIMEOUT, async {
        bus.emit_signal(
            None::<&str>,
            ICON_LOADER_PATH,
            ICON_LOADER_INTERFACE,
            "iconChanged",
            &(0i32,),
        )
        .await
    });
    match sent {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "could not ask the desktop to reload icons"),
        Err(_) => tracing::warn!("the desktop did not take the icon reload in time"),
    }
}

/// Runs `future` on `runtime` under `timeout`. The timeout is built inside the
/// runtime: `tokio::time::timeout` creates a `Sleep` on the calling thread, so
/// one built before `block_on` sees no reactor and panics.
fn with_timeout<F>(runtime: &Runtime, timeout: Duration, future: F) -> Result<F::Output, Elapsed>
where
    F: Future,
{
    runtime.block_on(async { tokio::time::timeout(timeout, future).await })
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

    /// `start` runs on the host thread, which has no ambient tokio context, and
    /// the tray's own runtime has the time driver on. A timeout built before
    /// `block_on` panicked there with "there is no reactor running".
    #[test]
    fn the_reload_timeout_is_built_inside_its_runtime() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let sent = with_timeout(&runtime, RELOAD_TIMEOUT, async { Ok::<(), ()>(()) });
        assert!(matches!(sent, Ok(Ok(()))), "a ready future completes");
        let stalled = with_timeout(
            &runtime,
            Duration::from_millis(1),
            std::future::pending::<()>(),
        );
        assert!(stalled.is_err(), "the deadline needs the time driver");
    }
}
