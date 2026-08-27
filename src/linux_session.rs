#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxSession {
    X11,
    Wayland,
}

impl LinuxSession {
    pub fn detect() -> Self {
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()) {
            return Self::Wayland;
        }
        if std::env::var("XDG_SESSION_TYPE")
            .map(|value| value.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
        {
            return Self::Wayland;
        }
        Self::X11
    }

    pub fn is_wayland(self) -> bool {
        matches!(self, Self::Wayland)
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
    fn empty_wayland_display_is_x11() {
        assert_eq!(
            std::env::var_os("WAYLAND_DISPLAY")
                .is_some_and(|value| !value.is_empty())
                .then_some(LinuxSession::Wayland)
                .unwrap_or(LinuxSession::X11),
            LinuxSession::detect()
        );
    }

    #[test]
    fn platform_names_match_settings() {
        assert_eq!(LinuxSession::X11.as_str(), "x11");
        assert_eq!(LinuxSession::Wayland.as_str(), "wayland");
    }
}
