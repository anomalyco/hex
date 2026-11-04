//! Labs › Floating meeting capsules on GNOME (#1274). A Wayland compositor
//! without layer-shell (GNOME) has no surface for an overlay that takes
//! clicks, so the recording offer and recording capsule can't float there and
//! Hex opens the Meetings pane instead (#1271). With this explicit opt-in, Rune
//! opens them on XWayland as override-redirect windows that take clicks only
//! on their buttons and never keyboard focus, and only while this user's
//! XWayland already runs: Hex never starts it.

/// Whether the meeting capsules may float on XWayland, read as each capsule
/// shows.
pub(crate) fn on_xwayland() -> bool {
    floats_on_xwayland(
        crate::labs::gnome_meeting_capsules(),
        || {
            crate::linux::session::LinuxSession::detect().is_wayland()
                && !crate::linux::paste::wayland_offers(crate::linux::rune_hud::LAYER_SHELL)
        },
        crate::linux::clipboard::xwayland_running,
    )
}

/// Only with the lab on, on a Wayland compositor without layer-shell, and
/// while XWayland already runs. Each check runs only when the ones before
/// pass, so with the lab off Hex never looks for layer-shell or XWayland.
fn floats_on_xwayland(
    opted_in: bool,
    without_layer_shell: impl FnOnce() -> bool,
    running: impl FnOnce() -> bool,
) -> bool {
    opted_in && without_layer_shell() && running()
}

#[cfg(test)]
mod tests {
    use super::floats_on_xwayland;

    #[test]
    fn floats_only_by_opt_in_without_layer_shell_while_xwayland_runs() {
        assert!(floats_on_xwayland(true, || true, || true));
        assert!(
            !floats_on_xwayland(false, || true, || true),
            "off by default"
        );
        assert!(
            !floats_on_xwayland(true, || false, || true),
            "layer-shell (or X11) keeps its own overlays"
        );
        assert!(
            !floats_on_xwayland(true, || true, || false),
            "never starts XWayland"
        );
    }

    #[test]
    fn looks_for_nothing_while_the_lab_is_off() {
        let looked = std::cell::Cell::new(false);
        let look = || {
            looked.set(true);
            true
        };
        assert!(!floats_on_xwayland(false, look, look));
        assert!(!looked.get());
    }
}
