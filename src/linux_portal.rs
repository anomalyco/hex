//! The desktop's GlobalShortcuts portal (`org.freedesktop.portal.GlobalShortcuts`).
//! It binds Hex's shortcuts in one session and plain Escape in a second
//! session only while something records, and reports their presses and
//! releases. A private bus connection owns both sessions: dropping the
//! portal closes them, and the desktop releases the keys.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use gtk::gio;
use gtk::glib::{self, ToVariant, Variant, variant::ObjectPath};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
/// The portal names host apps by their desktop entry, which Hex installs as `hex.desktop`.
const APP_ID: &str = "hex";
const CALL_TIMEOUT_MS: i32 = 5_000;
/// Binding can wait for the user to approve a desktop dialog.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
const ESCAPE_ID: &str = "cancel";
const HELD_ESCAPE_ID: &str = "cancel-held";

/// Whether a pressed id is one of the Escape session's.
pub(crate) fn is_escape(id: &str) -> bool {
    id == ESCAPE_ID || id == HELD_ESCAPE_ID
}

/// One shortcut Hex asks the desktop for.
pub(crate) struct Shortcut {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    /// XDG shortcuts format, such as `CTRL+ALT+space`.
    pub(crate) trigger: String,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    Pressed(String),
    Released(String),
    /// The desktop closed the shortcuts session.
    Closed,
}

type Queue = Rc<RefCell<Vec<(String, Event)>>>;
type Response = Rc<RefCell<Option<(u32, HashMap<String, Variant>)>>>;

pub(crate) struct Portal {
    connection: gio::DBusConnection,
    context: glib::MainContext,
    /// This connection's unique name as request paths spell it.
    sender: String,
    next_token: u32,
    queue: Queue,
    session: Option<String>,
    escape: Option<String>,
}

impl Portal {
    /// `None` when the session has no shortcuts portal.
    pub(crate) fn connect(stop: &AtomicBool) -> Result<Option<Self>> {
        let Ok(address) =
            gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)
        else {
            return Ok(None);
        };
        let Ok(connection) = gio::DBusConnection::for_address_sync(
            &address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None,
            gio::Cancellable::NONE,
        ) else {
            return Ok(None);
        };
        let version = connection.call_sync(
            Some(DESKTOP),
            DESKTOP_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(SHORTCUTS, "version").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        );
        if let Err(error) = version {
            tracing::info!(%error, "the desktop has no GlobalShortcuts portal");
            return Ok(None);
        }
        // Host apps have no sandbox identity; older portals without the
        // registry derive one from the systemd unit instead.
        if let Err(error) = connection.call_sync(
            Some(DESKTOP),
            DESKTOP_PATH,
            "org.freedesktop.host.portal.Registry",
            "Register",
            Some(&(APP_ID, HashMap::<String, Variant>::new()).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        ) && !error
            .message()
            .contains("org.freedesktop.DBus.Error.Unknown")
        {
            // The portal finds the app id through hex.desktop and the caller's
            // /proc entry, which setgid wrappers such as newgrp make unreadable.
            return Err(eyre!(
                "the shortcuts portal could not identify Hex ({error}); install Hex with its hex.desktop entry and run it without setuid or setgid wrappers such as newgrp"
            ));
        }
        let sender = connection
            .unique_name()
            .ok_or_else(|| eyre!("the session bus gave Hex no name"))?
            .trim_start_matches(':')
            .replace('.', "_");
        let portal = Self {
            connection,
            context: glib::MainContext::new(),
            sender,
            next_token: 0,
            queue: Queue::default(),
            session: None,
            escape: None,
        };
        for (member, pressed) in [("Activated", true), ("Deactivated", false)] {
            let queue = portal.queue.clone();
            portal.subscribe(SHORTCUTS, member, DESKTOP_PATH, move |parameters| {
                let (session, id) = (parameters.child_value(0), parameters.child_value(1));
                let (Some(session), Some(id)) = (session.str(), id.str()) else {
                    return;
                };
                let event = if pressed {
                    Event::Pressed(id.into())
                } else {
                    Event::Released(id.into())
                };
                queue.borrow_mut().push((session.into(), event));
            })?;
        }
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("shortcut registration canceled"));
        }
        Ok(Some(portal))
    }

    /// Binds Hex's shortcuts and returns the trigger the desktop describes
    /// for each one it granted, by id.
    pub(crate) fn bind(
        &mut self,
        shortcuts: &[Shortcut],
        stop: &AtomicBool,
    ) -> Result<HashMap<String, String>> {
        let session = self.create_session(stop)?;
        self.session = Some(session.clone());
        self.bind_session(&session, shortcuts, stop)
    }

    /// Binds Escape in its own session, and `held`, Escape with the
    /// recording shortcut's modifiers, for a press while that shortcut is
    /// still down. False when the desktop refused plain Escape.
    pub(crate) fn bind_escape(&mut self, held: Option<String>, stop: &AtomicBool) -> Result<bool> {
        let session = self.create_session(stop)?;
        self.escape = Some(session.clone());
        let mut shortcuts = vec![Shortcut {
            id: ESCAPE_ID,
            description: "Cancel dictation",
            trigger: "Escape".into(),
        }];
        shortcuts.extend(held.map(|trigger| Shortcut {
            id: HELD_ESCAPE_ID,
            description: "Cancel dictation while holding its shortcut",
            trigger,
        }));
        let granted = self.bind_session(&session, &shortcuts, stop)?;
        Ok(granted.contains_key(ESCAPE_ID))
    }

    pub(crate) fn release_escape(&mut self) {
        if let Some(session) = self.escape.take() {
            self.close_session(&session);
        }
    }

    /// The presses and releases that arrived since the last call.
    pub(crate) fn events(&mut self) -> Vec<Event> {
        while self.context.iteration(false) {}
        let queued = std::mem::take(&mut *self.queue.borrow_mut());
        queued
            .into_iter()
            .filter_map(|(session, event)| {
                if self.escape.as_deref() == Some(session.as_str()) {
                    return (event != Event::Closed).then_some(event);
                }
                (self.session.as_deref() == Some(session.as_str())).then_some(event)
            })
            .collect()
    }

    fn subscribe(
        &self,
        interface: &str,
        member: &str,
        path: &str,
        callback: impl Fn(&Variant) + 'static,
    ) -> Result<gio::SignalSubscriptionId> {
        // Callbacks run in the thread-default context at subscription,
        // which `events` iterates; never the GTK thread's default context.
        self.context
            .with_thread_default(|| {
                self.connection.signal_subscribe(
                    Some(DESKTOP),
                    Some(interface),
                    Some(member),
                    Some(path),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_, _, _, _, _, parameters| callback(parameters),
                )
            })
            .map_err(|error| eyre!("could not watch the shortcuts portal: {error}"))
    }

    fn token(&mut self) -> String {
        self.next_token += 1;
        format!("hex{}_{}", std::process::id(), self.next_token)
    }

    /// Calls a portal method that answers through a Request's Response signal.
    fn request(
        &mut self,
        method: &str,
        parameters: impl FnOnce(&str) -> Variant,
        stop: &AtomicBool,
    ) -> Result<HashMap<String, Variant>> {
        let token = self.token();
        let path = format!("{DESKTOP_PATH}/request/{}/{token}", self.sender);
        let response = Response::default();
        let received = response.clone();
        let subscription = self.subscribe(
            "org.freedesktop.portal.Request",
            "Response",
            &path,
            move |parameters| {
                *received.borrow_mut() = parameters.get();
            },
        )?;
        let result = (|| {
            self.connection
                .call_sync(
                    Some(DESKTOP),
                    DESKTOP_PATH,
                    SHORTCUTS,
                    method,
                    Some(&parameters(&token)),
                    None,
                    gio::DBusCallFlags::NONE,
                    CALL_TIMEOUT_MS,
                    gio::Cancellable::NONE,
                )
                .wrap_err_with(|| format!("the shortcuts portal refused {method}"))?;
            let deadline = Instant::now() + RESPONSE_TIMEOUT;
            loop {
                while self.context.iteration(false) {}
                if let Some((code, results)) = response.borrow_mut().take() {
                    return match code {
                        0 => Ok(results),
                        1 => Err(eyre!("the shortcuts request was dismissed")),
                        _ => Err(eyre!("the shortcuts portal could not complete {method}")),
                    };
                }
                if stop.load(Ordering::Acquire) {
                    return Err(eyre!("shortcut registration canceled"));
                }
                if Instant::now() >= deadline {
                    return Err(eyre!("the shortcuts portal did not answer {method}"));
                }
                thread::sleep(Duration::from_millis(2));
            }
        })();
        self.connection.signal_unsubscribe(subscription);
        result
    }

    fn create_session(&mut self, stop: &AtomicBool) -> Result<String> {
        let session_token = self.token();
        let results = self.request(
            "CreateSession",
            |token| {
                (HashMap::from([
                    ("handle_token".to_string(), token.to_variant()),
                    (
                        "session_handle_token".to_string(),
                        session_token.to_variant(),
                    ),
                ]),)
                    .to_variant()
            },
            stop,
        )?;
        let session = results
            .get("session_handle")
            .and_then(|handle| handle.str())
            .ok_or_else(|| eyre!("the shortcuts portal returned no session"))?
            .to_string();
        let queue = self.queue.clone();
        let closed = session.clone();
        self.subscribe(
            "org.freedesktop.portal.Session",
            "Closed",
            &session,
            move |_| queue.borrow_mut().push((closed.clone(), Event::Closed)),
        )?;
        Ok(session)
    }

    fn bind_session(
        &mut self,
        session: &str,
        shortcuts: &[Shortcut],
        stop: &AtomicBool,
    ) -> Result<HashMap<String, String>> {
        let handle = ObjectPath::try_from(session)
            .map_err(|_| eyre!("the shortcuts portal returned an invalid session"))?;
        let requested = shortcuts
            .iter()
            .map(|shortcut| {
                (
                    shortcut.id.to_string(),
                    HashMap::from([
                        ("description".to_string(), shortcut.description.to_variant()),
                        (
                            "preferred_trigger".to_string(),
                            shortcut.trigger.to_variant(),
                        ),
                    ]),
                )
            })
            .collect::<Vec<_>>();
        let results = self.request(
            "BindShortcuts",
            |token| {
                (
                    handle,
                    requested,
                    "",
                    HashMap::from([("handle_token".to_string(), token.to_variant())]),
                )
                    .to_variant()
            },
            stop,
        )?;
        let bound = results
            .get("shortcuts")
            .and_then(|shortcuts| shortcuts.get::<Vec<(String, HashMap<String, Variant>)>>())
            .unwrap_or_default();
        Ok(bound
            .into_iter()
            .map(|(id, options)| {
                let trigger = options
                    .get("trigger_description")
                    .and_then(|trigger| trigger.str())
                    .unwrap_or_default()
                    .to_string();
                (id, trigger)
            })
            .collect())
    }

    fn close_session(&self, session: &str) {
        if let Err(error) = self.connection.call_sync(
            Some(DESKTOP),
            session,
            "org.freedesktop.portal.Session",
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT_MS,
            gio::Cancellable::NONE,
        ) {
            tracing::warn!(%error, "could not close a shortcuts portal session");
        }
    }
}

impl Drop for Portal {
    fn drop(&mut self) {
        // The portal closes every session of a vanished connection.
        let _ = self.connection.close_sync(gio::Cancellable::NONE);
    }
}

/// A desktop's trigger description as Settings shows it. GNOME describes a
/// GTK accelerator (`Press <ctrl><alt>F9`); other desktops send readable text.
pub(crate) fn readable_trigger(description: &str) -> String {
    let description = description.trim();
    let accelerator = description.strip_prefix("Press ").unwrap_or(description);
    if accelerator.is_empty() || accelerator.contains([' ', '+']) {
        return description.to_string();
    }
    let mut parts = Vec::new();
    let mut rest = accelerator;
    while let Some(after) = rest.strip_prefix('<') {
        let Some((name, after)) = after.split_once('>') else {
            return description.to_string();
        };
        parts.push(match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "primary" => "Ctrl".to_string(),
            "alt" | "mod1" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "super" | "logo" | "meta" => "Super".to_string(),
            other => other.to_string(),
        });
        rest = after;
    }
    parts.push(match rest {
        "space" => "Space".to_string(),
        key if key.chars().count() == 1 => key.to_uppercase(),
        key => key.to_string(),
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtk_accelerators_read_like_the_shortcut_rows() {
        assert_eq!(readable_trigger("Press <ctrl><alt>F9"), "Ctrl+Alt+F9");
        assert_eq!(readable_trigger("Press <alt>space"), "Alt+Space");
        assert_eq!(readable_trigger("<shift><super>a"), "Shift+Super+A");
        assert_eq!(readable_trigger("Press F13"), "F13");
        assert_eq!(readable_trigger("Press <ctrl>minus"), "Ctrl+minus");
        assert_eq!(readable_trigger("Ctrl+Alt+F9"), "Ctrl+Alt+F9");
        assert_eq!(readable_trigger("Press <ctrl"), "Press <ctrl");
    }
}
