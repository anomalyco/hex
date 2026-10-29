//! The Linux tray icon, a Rune tray (StatusNotifierItem). A click shows the
//! window; the menu offers Show Hex and Quit Hex. A desktop without a tray
//! host (GNOME without an extension, i3bar) shows nothing, and Hex runs on
//! as before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use crate::desktop_i18n::tr;
use crate::linux_service::{Activation, Control};

/// The white Hex mark the Windows tray uses, raw 32×32 RGBA.
const ICON_RGBA: &[u8] = include_bytes!("../resources/windows/tray-32.rgba");
const ICON_SIZE: u32 = 32;

/// Shows the tray icon until the returned tray drops.
pub(crate) fn start(
    control: SyncSender<Control>,
    shutdown: &'static AtomicBool,
) -> Option<rune::Tray> {
    let icon = rune::TrayIcon::rgba(ICON_SIZE, ICON_SIZE, ICON_RGBA.to_vec());
    let spec = rune::TraySpec::new("hex", "Hex", icon)
        .tooltip(tr("Hex Dictation"))
        .menu(vec![
            rune::TrayItem::action("show", tr("Show Hex")),
            rune::TrayItem::Separator,
            rune::TrayItem::action("quit", tr("Quit Hex")),
        ]);
    let show = move |token| {
        let activation = Activation {
            token,
            startup_id: None,
        };
        let _ = control.try_send(Control::Activate(activation));
    };
    let tray = rune::Tray::new(spec, move |event| match event {
        rune::TrayEvent::Activate { token } => show(token),
        rune::TrayEvent::Menu(id) if id == "show" => show(None),
        // As the window's Quit Hex: drains accepted work, then exits.
        rune::TrayEvent::Menu(id) if id == "quit" => shutdown.store(true, Ordering::Release),
        _ => {}
    });
    tray.inspect_err(|error| tracing::warn!(%error, "Hex has no tray icon"))
        .ok()
}
