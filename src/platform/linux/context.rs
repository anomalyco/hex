//! Foreground X11 context through the concrete EWMH contract supported by the
//! Linux beta.
//!
//! The active client comes from `_NET_ACTIVE_WINDOW`; its executable identity
//! comes from `_NET_WM_PID` and `/proc`, with `WM_CLASS` as a fallback. Window
//! title is observational only. Browser URLs deliberately remain absent until
//! a real browser adapter exists.

use std::fs;

use color_eyre::Result;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

const MAX_APPLICATION_CHARS: usize = 256;
const MAX_TITLE_CHARS: usize = 512;
const MAX_PROPERTY_LONGS: u32 = 1_024;

struct Atoms {
    active_window: Atom,
    utf8_string: Atom,
    window_name: Atom,
    window_pid: Atom,
}

/// A reusable EWMH reader owned by the Linux output worker. Keeping one X11
/// connection avoids reconnecting for every accepted dictation.
pub(crate) struct LinuxContext {
    connection: RustConnection,
    root: Window,
    atoms: Atoms,
}

impl LinuxContext {
    pub(crate) fn new() -> Result<Self> {
        let (connection, screen) = x11rb::connect(None)?;
        let root = connection.setup().roots[screen].root;
        let atoms = Atoms {
            active_window: intern(&connection, b"_NET_ACTIVE_WINDOW")?,
            utf8_string: intern(&connection, b"UTF8_STRING")?,
            window_name: intern(&connection, b"_NET_WM_NAME")?,
            window_pid: intern(&connection, b"_NET_WM_PID")?,
        };
        Ok(Self {
            connection,
            root,
            atoms,
        })
    }

    /// Capture the frontmost application and bounded title when the output
    /// worker begins a job. A disappearing client is a transient read failure;
    /// callers degrade that job to an empty context without affecting
    /// transcription or paste.
    pub(crate) fn capture(&self) -> Result<crate::command_context::ContextSnapshot> {
        let Some(window) =
            self.first_u32(self.root, self.atoms.active_window, AtomEnum::WINDOW.into())?
        else {
            return Ok(crate::command_context::ContextSnapshot::empty());
        };
        if window == x11rb::NONE {
            return Ok(crate::command_context::ContextSnapshot::empty());
        }

        let pid = self.first_u32(window, self.atoms.window_pid, AtomEnum::CARDINAL.into())?;
        let class =
            self.string_property(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into())?;
        let application = pid
            .and_then(application_from_pid)
            .or_else(|| class.as_deref().and_then(parse_wm_class))
            .map(|name| bounded(&name, MAX_APPLICATION_CHARS));
        let window_title = self
            .string_property(window, self.atoms.window_name, self.atoms.utf8_string)?
            .filter(|title| !title.trim().is_empty())
            .or_else(|| {
                self.string_property(window, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into())
                    .ok()
                    .flatten()
                    .filter(|title| !title.trim().is_empty())
            })
            .map(|title| bounded(title.trim_matches('\0').trim(), MAX_TITLE_CHARS));

        Ok(crate::command_context::ContextSnapshot {
            application,
            browser_url: None,
            window_title,
            selected_text: None,
            input_revision: None,
        })
    }

    fn first_u32(
        &self,
        window: Window,
        property: Atom,
        property_type: Atom,
    ) -> Result<Option<u32>> {
        let reply = self
            .connection
            .get_property(false, window, property, property_type, 0, 1)?
            .reply()?;
        Ok(reply.value32().and_then(|mut values| values.next()))
    }

    fn string_property(
        &self,
        window: Window,
        property: Atom,
        property_type: Atom,
    ) -> Result<Option<String>> {
        let reply = self
            .connection
            .get_property(
                false,
                window,
                property,
                property_type,
                0,
                MAX_PROPERTY_LONGS,
            )?
            .reply()?;
        if reply.value.is_empty() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&reply.value).into_owned()))
    }
}

fn intern(connection: &RustConnection, name: &[u8]) -> Result<Atom> {
    Ok(connection.intern_atom(false, name)?.reply()?.atom)
}

fn application_from_pid(pid: u32) -> Option<String> {
    let process = format!("/proc/{pid}");
    fs::read_link(format!("{process}/exe"))
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .map(|name| name.trim_end_matches(" (deleted)").to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            fs::read_to_string(format!("{process}/comm"))
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
}

fn parse_wm_class(value: &str) -> Option<String> {
    value
        .split('\0')
        .map(str::trim)
        .rfind(|part| !part.is_empty())
        .map(str::to_string)
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_prefers_the_application_class_over_the_instance() {
        assert_eq!(
            parse_wm_class("Navigator\0Firefox\0").as_deref(),
            Some("Firefox")
        );
        assert_eq!(parse_wm_class("code\0").as_deref(), Some("code"));
        assert_eq!(parse_wm_class("\0\0"), None);
    }

    #[test]
    fn context_strings_are_bounded_on_character_boundaries() {
        assert_eq!(bounded("żółw", 3), "żół");
    }
}
