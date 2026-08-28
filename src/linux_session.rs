use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxSession {
    X11,
    Wayland,
}

impl LinuxSession {
    pub fn detect() -> Self {
        Self::from_wayland_display(std::env::var_os("WAYLAND_DISPLAY").as_deref())
    }

    fn from_wayland_display(display: Option<&OsStr>) -> Self {
        // Match GPUI's backend selection, including Wayland with XWayland present.
        if display.is_some_and(|display| !display.is_empty()) {
            Self::Wayland
        } else {
            Self::X11
        }
    }

    pub fn is_wayland(self) -> bool {
        self == Self::Wayland
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_selection_matches_gpui_without_mutating_the_environment() {
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
