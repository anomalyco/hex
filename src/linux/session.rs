use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinuxSession {
    X11,
    Wayland,
}

impl LinuxSession {
    pub(crate) fn detect() -> Self {
        Self::from_wayland_display(std::env::var_os("WAYLAND_DISPLAY").as_deref())
    }

    fn from_wayland_display(display: Option<&OsStr>) -> Self {
        // Match winit's (Rune's) backend selection, including Wayland with XWayland present.
        if display.is_some_and(|display| !display.is_empty()) {
            Self::Wayland
        } else {
            Self::X11
        }
    }

    pub(crate) fn is_wayland(self) -> bool {
        self == Self::Wayland
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
        }
    }
}

/// Desktops whose window managers float every window, so Hex's window
/// opens at its own resizable size there.
const FLOATING_DESKTOPS: &[&str] = &[
    "KDE",
    "GNOME",
    "XFCE",
    "X-Cinnamon",
    "Cinnamon",
    "MATE",
    "LXQt",
    "LXDE",
    "Budgie",
    "Pantheon",
    "Unity",
    "Deepin",
    "Enlightenment",
];

/// Whether `XDG_CURRENT_DESKTOP` names a desktop that floats every window.
pub(crate) fn floats_windows() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| floating(&desktops))
}

fn floating(desktops: &str) -> bool {
    desktops.split(':').any(|desktop| {
        FLOATING_DESKTOPS
            .iter()
            .any(|floating| desktop.eq_ignore_ascii_case(floating))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_desktops_skip_the_fixed_size_map() {
        assert!(floating("KDE"));
        assert!(floating("ubuntu:GNOME"));
        assert!(floating("X-Cinnamon"));
        for tiling in ["niri", "sway", "Hyprland", "i3", ""] {
            assert!(!floating(tiling), "{tiling}");
        }
    }

    #[test]
    fn backend_selection_matches_winit_without_mutating_the_environment() {
        for display in [None, Some(OsStr::new(""))] {
            assert_eq!(
                LinuxSession::from_wayland_display(display),
                LinuxSession::X11
            );
        }
        for display in ["wayland-0", "/run/user/1000/wayland-1"] {
            assert_eq!(
                LinuxSession::from_wayland_display(Some(OsStr::new(display))),
                LinuxSession::Wayland
            );
        }
    }
}
