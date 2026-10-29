//! The desktop's GlobalShortcuts portal (`org.freedesktop.portal.GlobalShortcuts`).
//! It binds Hex's shortcuts in one session and plain Escape in a second
//! session only while something records, and reports their presses and
//! releases. A private bus connection owns both sessions: dropping the
//! portal closes them, and the desktop releases the keys. The connection
//! runs on its own one-thread tokio runtime, so the portal's signals queue
//! while the shortcut loop sleeps.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use futures_util::StreamExt;
use zbus::{
    Connection, DBusError, MatchRule, MessageStream, ObjectPath, OwnedObjectPath, OwnedVariant,
    Variant, message,
};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";
/// The portal names host apps by their desktop entry, which Hex installs as `hex.desktop`.
const APP_ID: &str = "hex";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Binding can wait for the user to approve a desktop dialog.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a pending request checks for cancellation.
const STOP_POLL: Duration = Duration::from_millis(50);
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

type Results = HashMap<String, OwnedVariant>;

/// A portal signal, by the session or request path it names.
enum Signal {
    Shortcut(String, Event),
    Response(String, u32, Results),
}

fn parse(message: &zbus::Message) -> Option<Signal> {
    let header = message.header();
    let path = header.path()?.to_string();
    let body = message.body();
    match (header.interface()?.as_str(), header.member()?.as_str()) {
        (SHORTCUTS, member @ ("Activated" | "Deactivated")) => {
            let (session, id, _, _): (OwnedObjectPath, String, u64, Results) =
                body.deserialize().ok()?;
            let event = if member == "Activated" {
                Event::Pressed(id)
            } else {
                Event::Released(id)
            };
            Some(Signal::Shortcut(session.to_string(), event))
        }
        (SESSION, "Closed") => Some(Signal::Shortcut(path, Event::Closed)),
        (REQUEST, "Response") => {
            let (code, results): (u32, Results) = body.deserialize().ok()?;
            Some(Signal::Response(path, code, results))
        }
        _ => None,
    }
}

/// An unknown method or interface: an older portal without that call.
fn unknown(error: &zbus::Error) -> bool {
    let name = match error {
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        zbus::Error::FDO(error) => error.name().to_string(),
        _ => return false,
    };
    name.starts_with("org.freedesktop.DBus.Error.Unknown")
}

pub(crate) struct Portal {
    runtime: tokio::runtime::Runtime,
    connection: Connection,
    signals: Receiver<Signal>,
    /// Shortcut signals that arrived while a request waited.
    pending: VecDeque<(String, Event)>,
    /// This connection's unique name as request paths spell it.
    sender: String,
    next_token: u32,
    session: Option<String>,
    escape: Option<String>,
}

impl Portal {
    /// `None` when the session has no shortcuts portal.
    pub(crate) fn connect(stop: &AtomicBool) -> Result<Option<Self>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("hex-portal")
            .enable_all()
            .build()
            .wrap_err("could not start the shortcuts portal runtime")?;
        let (signals, received) = channel();
        let connected = runtime.block_on(async {
            let connection = tokio::time::timeout(CALL_TIMEOUT, Connection::session())
                .await
                .ok()?
                .ok()?;
            // Subscribed before any request so a fast answer cannot be missed.
            let rule = MatchRule::builder()
                .msg_type(message::Type::Signal)
                .path_namespace(DESKTOP_PATH)
                .build()
                .ok()?;
            let mut stream = MessageStream::for_match_rule(rule, &connection, None)
                .await
                .ok()?;
            tokio::spawn(async move {
                while let Some(Ok(message)) = stream.next().await {
                    if let Some(signal) = parse(&message)
                        && signals.send(signal).is_err()
                    {
                        break;
                    }
                }
            });
            Some(connection)
        });
        let Some(connection) = connected else {
            return Ok(None);
        };
        let Some(sender) = connection
            .unique_name()
            .map(|name| name.trim_start_matches(':').replace('.', "_"))
        else {
            return Ok(None);
        };
        let portal = Self {
            runtime,
            connection,
            signals: received,
            pending: VecDeque::new(),
            sender,
            next_token: 0,
            session: None,
            escape: None,
        };
        let version = portal.call::<_, OwnedVariant>(
            DESKTOP_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            &(SHORTCUTS, "version"),
        );
        if let Err(error) = version {
            tracing::info!(%error, "the desktop has no GlobalShortcuts portal");
            return Ok(None);
        }
        // Host apps have no sandbox identity; older portals without the
        // registry derive one from the systemd unit instead.
        if let Err(error) = portal.call::<_, ()>(
            DESKTOP_PATH,
            "org.freedesktop.host.portal.Registry",
            "Register",
            &(APP_ID, HashMap::<&str, Variant<'_>>::new()),
        ) && !unknown(&error)
        {
            // The portal finds the app id through hex.desktop and the caller's
            // /proc entry, which setgid wrappers such as newgrp make unreadable.
            return Err(eyre!(
                "the shortcuts portal could not identify Hex ({error}); install Hex with its hex.desktop entry and run it without setuid or setgid wrappers such as newgrp"
            ));
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
        if let Some(session) = self.escape.take()
            && let Err(error) = self.call::<_, ()>(&session, SESSION, "Close", &())
        {
            tracing::warn!(%error, "could not close a shortcuts portal session");
        }
    }

    /// The presses and releases that arrived since the last call.
    pub(crate) fn events(&mut self) -> Vec<Event> {
        let mut queued = std::mem::take(&mut self.pending);
        queued.extend(self.signals.try_iter().filter_map(|signal| match signal {
            Signal::Shortcut(session, event) => Some((session, event)),
            Signal::Response(..) => None,
        }));
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

    fn call<B, R>(&self, path: &str, interface: &str, method: &str, body: &B) -> zbus::Result<R>
    where
        B: serde::Serialize + zbus::Type,
        R: for<'d> serde::Deserialize<'d> + zbus::Type,
    {
        self.runtime.block_on(async {
            let reply = tokio::time::timeout(
                CALL_TIMEOUT,
                self.connection
                    .call_method(Some(DESKTOP), path, Some(interface), method, body),
            )
            .await
            .map_err(|_| zbus::Error::Failure(format!("{method} timed out")))??;
            reply.body().deserialize()
        })
    }

    fn token(&mut self) -> String {
        self.next_token += 1;
        format!("hex{}_{}", std::process::id(), self.next_token)
    }

    /// Calls a portal method that answers through a Request's Response signal.
    fn request<B>(
        &mut self,
        method: &str,
        body: impl FnOnce(&str) -> B,
        stop: &AtomicBool,
    ) -> Result<Results>
    where
        B: serde::Serialize + zbus::Type,
    {
        let token = self.token();
        let path = format!("{DESKTOP_PATH}/request/{}/{token}", self.sender);
        self.call::<_, OwnedObjectPath>(DESKTOP_PATH, SHORTCUTS, method, &body(&token))
            .wrap_err_with(|| format!("the shortcuts portal refused {method}"))?;
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            match self.signals.recv_timeout(STOP_POLL) {
                Ok(Signal::Response(answered, code, results)) if answered == path => {
                    return match code {
                        0 => Ok(results),
                        1 => Err(eyre!("the shortcuts request was dismissed")),
                        _ => Err(eyre!("the shortcuts portal could not complete {method}")),
                    };
                }
                Ok(Signal::Shortcut(session, event)) => self.pending.push_back((session, event)),
                Ok(Signal::Response(..)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(eyre!("the shortcuts portal connection closed"));
                }
            }
            if stop.load(Ordering::Acquire) {
                return Err(eyre!("shortcut registration canceled"));
            }
            if Instant::now() >= deadline {
                return Err(eyre!("the shortcuts portal did not answer {method}"));
            }
        }
    }

    fn create_session(&mut self, stop: &AtomicBool) -> Result<String> {
        let session_token = self.token();
        let results = self.request(
            "CreateSession",
            |token| {
                (HashMap::from([
                    ("handle_token", Variant::from(token.to_owned())),
                    ("session_handle_token", Variant::from(session_token.clone())),
                ]),)
            },
            stop,
        )?;
        // Portals send the handle as a string, as the spec says, or as an object path.
        let handle = results
            .get("session_handle")
            .and_then(|handle| match <&str>::try_from(handle) {
                Ok(handle) => Some(handle.to_owned()),
                Err(_) => handle
                    .try_clone()
                    .ok()
                    .and_then(|handle| OwnedObjectPath::try_from(handle).ok())
                    .map(|path| path.to_string()),
            })
            .ok_or_else(|| eyre!("the shortcuts portal returned no session"))?;
        Ok(handle)
    }

    fn bind_session(
        &mut self,
        session: &str,
        shortcuts: &[Shortcut],
        stop: &AtomicBool,
    ) -> Result<HashMap<String, String>> {
        let handle = ObjectPath::try_from(session.to_owned())
            .map_err(|_| eyre!("the shortcuts portal returned an invalid session"))?;
        let requested = shortcuts
            .iter()
            .map(|shortcut| {
                (
                    shortcut.id,
                    HashMap::from([
                        ("description", Variant::from(shortcut.description)),
                        (
                            "preferred_trigger",
                            Variant::from(shortcut.trigger.as_str()),
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
                    HashMap::from([("handle_token", Variant::from(token.to_owned()))]),
                )
            },
            stop,
        )?;
        let bound = results
            .get("shortcuts")
            .and_then(|shortcuts| shortcuts.try_clone().ok())
            .and_then(|shortcuts| Vec::<(String, Results)>::try_from(shortcuts).ok())
            .unwrap_or_default();
        Ok(bound
            .into_iter()
            .map(|(id, options)| {
                let trigger = options
                    .get("trigger_description")
                    .and_then(|trigger| <&str>::try_from(trigger).ok())
                    .unwrap_or_default()
                    .to_string();
                (id, trigger)
            })
            .collect())
    }
}

impl Drop for Portal {
    fn drop(&mut self) {
        // The portal closes every session of a vanished connection.
        let connection = self.connection.clone();
        let _ = self
            .runtime
            .block_on(async { tokio::time::timeout(CALL_TIMEOUT, connection.close()).await });
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
