//! Wayland paste through the desktop's RemoteDesktop portal
//! (`org.freedesktop.portal.RemoteDesktop`), for compositors without the
//! virtual keyboard protocol `wtype` needs, such as KWin and Mutter.
//!
//! One keyboard-only session lives as long as its `RemoteDesktop`. The
//! user approves it once: the portal's restore token, kept owner-only in
//! Application Support, restores later sessions without a dialog, so the
//! paster can close an idle session and reopen it at the next paste.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use zbus::{OwnedObjectPath, Variant};

use crate::linux::portal::{Event, PortalBus};

pub(crate) const INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const KEYBOARD: u32 = 1;
/// Persist the grant until the user revokes it.
const PERSIST_UNTIL_REVOKED: u32 = 2;
const XK_CONTROL_L: i32 = 0xffe3;
const XK_SHIFT_L: i32 = 0xffe1;
const XK_V: i32 = 0x76;
const PRESSED: u32 = 1;
const RELEASED: u32 = 0;
/// A restored grant starts in tens of milliseconds; a start that takes
/// longer waited for the person in the desktop's dialog.
const DIALOG_THRESHOLD: Duration = Duration::from_millis(500);

pub(crate) struct RemoteDesktop {
    bus: PortalBus,
    session: OwnedObjectPath,
    /// The portal handed out a restore token, so a later session restores
    /// this grant without a dialog.
    restorable: bool,
    /// The desktop showed its approval dialog for this session.
    prompted: bool,
    /// The desktop already ended the session, so there is nothing to close.
    ended: bool,
}

impl RemoteDesktop {
    /// Starts a keyboard session, which shows the desktop's approval
    /// dialog unless a saved grant restores it.
    pub(crate) fn start(stop: &AtomicBool) -> Result<Self> {
        let mut bus = PortalBus::open(INTERFACE, stop)?
            .ok_or_else(|| eyre!("the desktop has no RemoteDesktop portal ({INTERFACE})"))?;
        let began = Instant::now();
        let session = bus.create_session(stop)?;
        let persists = bus.version >= 2;
        let restore = persists.then(load_restore_token).flatten();
        bus.request(
            "SelectDevices",
            |token| {
                let mut options = HashMap::from([
                    ("handle_token", Variant::from(token.to_owned())),
                    ("types", Variant::from(KEYBOARD)),
                ]);
                if persists {
                    options.insert("persist_mode", Variant::from(PERSIST_UNTIL_REVOKED));
                }
                if let Some(restore) = restore.clone() {
                    options.insert("restore_token", Variant::from(restore));
                }
                (session.clone(), options)
            },
            stop,
        )?;
        let started = bus.request(
            "Start",
            |token| {
                (
                    session.clone(),
                    "",
                    HashMap::from([("handle_token", Variant::from(token.to_owned()))]),
                )
            },
            stop,
        )?;
        let devices = started
            .get("devices")
            .and_then(|devices| u32::try_from(devices).ok())
            .unwrap_or(0);
        if devices & KEYBOARD == 0 {
            bus.close_session(&session);
            return Err(eyre!("the desktop did not allow Hex to use the keyboard"));
        }
        let prompted = restore.is_none() || began.elapsed() >= DIALOG_THRESHOLD;
        // Restore tokens are single-use: each start hands out the next one.
        let mut restorable = false;
        if let Some(token) = started
            .get("restore_token")
            .and_then(|token| <&str>::try_from(token).ok())
        {
            match save_restore_token(token) {
                Ok(()) => restorable = true,
                Err(error) => tracing::warn!(
                    error = format!("{error:#}"),
                    "could not save the RemoteDesktop grant"
                ),
            }
        }
        Ok(Self {
            bus,
            session,
            restorable,
            prompted,
            ended: false,
        })
    }

    /// Asks the desktop for the keyboard ahead of the first paste, from
    /// onboarding or Settings. The saved grant lets later pastes start
    /// without a dialog.
    pub(crate) fn ask() -> Result<()> {
        Self::start(&AtomicBool::new(false)).map(drop)
    }

    /// Whether the desktop granted the keyboard before: a saved grant.
    pub(crate) fn granted() -> bool {
        load_restore_token().is_some()
    }

    /// Whether a later session restores this grant without a dialog.
    pub(crate) fn restorable(&self) -> bool {
        self.restorable
    }

    /// Whether the desktop showed its approval dialog for this session.
    pub(crate) fn prompted(&self) -> bool {
        self.prompted
    }

    /// Whether the desktop ended the session, as when the user revokes it.
    pub(crate) fn closed(&mut self) -> bool {
        if !self.ended {
            let session = self.session.as_str().to_owned();
            self.ended = self
                .bus
                .events()
                .into_iter()
                .any(|(closed, event)| closed == session && event == Event::Closed);
        }
        self.ended
    }

    /// Presses and releases Ctrl+V, or Ctrl+Shift+V for terminals.
    pub(crate) fn paste(&self, with_shift: bool) -> Result<()> {
        for (keysym, state) in paste_keys(with_shift) {
            self.bus.call_portal(
                "NotifyKeyboardKeysym",
                &(
                    &self.session,
                    HashMap::<&str, Variant<'_>>::new(),
                    keysym,
                    state,
                ),
            )?;
        }
        Ok(())
    }
}

impl Drop for RemoteDesktop {
    fn drop(&mut self) {
        // Closing a session the desktop already ended (the user stopped it
        // from GNOME's remote-control indicator) only fails (#718).
        if !self.closed() {
            self.bus.close_session(&self.session);
        }
    }
}

fn paste_keys(with_shift: bool) -> Vec<(i32, u32)> {
    let mut keys = vec![(XK_CONTROL_L, PRESSED)];
    if with_shift {
        keys.push((XK_SHIFT_L, PRESSED));
    }
    keys.extend([(XK_V, PRESSED), (XK_V, RELEASED)]);
    if with_shift {
        keys.push((XK_SHIFT_L, RELEASED));
    }
    keys.push((XK_CONTROL_L, RELEASED));
    keys
}

fn restore_token_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("remote-desktop-token"))
}

fn load_restore_token() -> Option<String> {
    let token = std::fs::read_to_string(restore_token_path().ok()?).ok()?;
    let token = token.trim();
    (!token.is_empty() && token.len() <= 1024).then(|| token.to_owned())
}

fn save_restore_token(token: &str) -> Result<()> {
    let path = restore_token_path()?;
    let directory = path
        .parent()
        .ok_or_else(|| eyre!("the grant has no folder"))?;
    std::fs::create_dir_all(directory)?;
    let staged = directory.join(format!(".remote-desktop-token.{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&staged)
        .wrap_err("could not stage the grant")?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&staged, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_keys_balance_their_modifiers() {
        assert_eq!(
            paste_keys(false),
            [
                (XK_CONTROL_L, PRESSED),
                (XK_V, PRESSED),
                (XK_V, RELEASED),
                (XK_CONTROL_L, RELEASED)
            ]
        );
        assert_eq!(
            paste_keys(true),
            [
                (XK_CONTROL_L, PRESSED),
                (XK_SHIFT_L, PRESSED),
                (XK_V, PRESSED),
                (XK_V, RELEASED),
                (XK_SHIFT_L, RELEASED),
                (XK_CONTROL_L, RELEASED)
            ]
        );
    }
}
