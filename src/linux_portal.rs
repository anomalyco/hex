//! The desktop's portals (`org.freedesktop.portal.*`) over a private bus
//! connection. `PortalBus` registers Hex with the portal and answers
//! requests; `Portal` is the GlobalShortcuts client and
//! `linux_remote_desktop` the RemoteDesktop one.
//!
//! `Portal` binds all of Hex's shortcuts in one session, once, and reports
//! their presses and releases. Desktops keep one shortcut set per app (KDE
//! replaces it with each BindShortcuts call), so Hex never binds a second
//! set or changes this one while running. Desktops also keep the trigger
//! they stored for an id over a later `preferred_trigger` (GNOME, KDE), so
//! `linux_input` names each shortcut by the trigger it asks for. Dropping a bus closes its
//! sessions, and the desktop releases the keys. Each bus runs on its own one-thread tokio runtime, so
//! the portal's signals queue while the caller sleeps.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use futures_util::StreamExt;
use zbus::{
    Connection, DBusError, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant,
    message,
};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";
/// The portal names host apps by their desktop entry, `hex.desktop`.
const APP_ID: &str = "hex";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// A request can wait for the user to approve a desktop dialog.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a pending request checks for cancellation.
const STOP_POLL: Duration = Duration::from_millis(50);
/// Cancel dictation, which the user assigns in the desktop. Not `cancel`:
/// KDE still holds Escape under that id from builds that bound it.
pub(crate) const CANCEL_ID: &str = "cancel-dictation";

/// One shortcut Hex asks the desktop for.
pub(crate) struct Shortcut {
    pub(crate) id: String,
    pub(crate) description: &'static str,
    /// XDG shortcuts format, such as `CTRL+ALT+space`; `None` leaves the
    /// shortcut for the user to assign.
    pub(crate) trigger: Option<String>,
}

impl Shortcut {
    /// Cancel dictation, with no suggested key: a portal shortcut is held
    /// for as long as Hex runs, so suggesting Escape would take it from
    /// every other application.
    pub(crate) fn cancel() -> Self {
        Self {
            id: CANCEL_ID.into(),
            description: "Cancel dictation",
            trigger: None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    Pressed(String),
    Released(String),
    /// The user changed Hex's shortcuts in the desktop: the trigger each
    /// one now has, by id, or `None` when it has none.
    Changed(HashMap<String, Option<String>>),
    /// The desktop closed the session.
    Closed,
}

pub(crate) type Results = HashMap<String, OwnedVariant>;

/// A portal signal, by the session or request path it names.
enum Signal {
    Session(String, Event),
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
            Some(Signal::Session(session.to_string(), event))
        }
        (SHORTCUTS, "ShortcutsChanged") => {
            let (session, shortcuts): (OwnedObjectPath, Vec<(String, Results)>) =
                body.deserialize().ok()?;
            Some(Signal::Session(
                session.to_string(),
                Event::Changed(triggers(shortcuts)),
            ))
        }
        (SESSION, "Closed") => Some(Signal::Session(path, Event::Closed)),
        (REQUEST, "Response") => {
            let (code, results): (u32, Results) = body.deserialize().ok()?;
            Some(Signal::Response(path, code, results))
        }
        _ => None,
    }
}

/// Each shortcut's trigger description. An empty one (KDE's for a shortcut
/// it keeps unbound) means no trigger.
fn triggers(shortcuts: Vec<(String, Results)>) -> HashMap<String, Option<String>> {
    shortcuts
        .into_iter()
        .map(|(id, options)| {
            let trigger = options
                .get("trigger_description")
                .and_then(|trigger| <&str>::try_from(trigger).ok())
                .map(str::trim)
                .filter(|trigger| !trigger.is_empty())
                .map(str::to_owned);
            (id, trigger)
        })
        .collect()
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

/// A private session-bus connection registered with the portal as Hex.
pub(crate) struct PortalBus {
    runtime: tokio::runtime::Runtime,
    connection: Connection,
    signals: Receiver<Signal>,
    /// Session signals that arrived while a request waited.
    pending: VecDeque<(String, Event)>,
    /// This connection's unique name as request paths spell it.
    sender: String,
    next_token: u32,
    /// The portal interface this bus serves and its version.
    interface: &'static str,
    pub(crate) version: u32,
}

impl PortalBus {
    /// `None` when the session has no portal offering `interface`.
    pub(crate) fn open(interface: &'static str, stop: &AtomicBool) -> Result<Option<Self>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("hex-portal")
            .enable_all()
            .build()
            .wrap_err("could not start the portal runtime")?;
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
        let mut bus = Self {
            runtime,
            connection,
            signals: received,
            pending: VecDeque::new(),
            sender,
            next_token: 0,
            interface,
            version: 0,
        };
        match bus.call::<_, OwnedVariant>(
            DESKTOP_PATH,
            "org.freedesktop.DBus.Properties",
            "Get",
            &(interface, "version"),
        ) {
            Ok(version) => bus.version = u32::try_from(&version).unwrap_or(1),
            Err(error) => {
                tracing::info!(%error, interface, "the desktop has no such portal");
                return Ok(None);
            }
        }
        let written = ensure_desktop_entry()?;
        // Host apps have no sandbox identity; older portals without the
        // registry derive one from the systemd unit instead.
        if let Err(error) = bus.call::<_, ()>(
            DESKTOP_PATH,
            "org.freedesktop.host.portal.Registry",
            "Register",
            &(APP_ID, HashMap::<&str, Variant<'_>>::new()),
        ) && !unknown(&error)
        {
            // The portal finds the app id through hex.desktop and the caller's
            // /proc entry, which setgid wrappers such as newgrp make unreadable.
            let fix = match written {
                Some(entry) => format!(
                    "Hex wrote {}; restart xdg-desktop-portal (systemctl --user restart xdg-desktop-portal) so it reads it",
                    entry.display()
                ),
                None => "run Hex without setuid or setgid wrappers such as newgrp".into(),
            };
            return Err(eyre!(
                "the desktop portal could not identify Hex ({error}); {fix}"
            ));
        }
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("portal registration canceled"));
        }
        Ok(Some(bus))
    }

    pub(crate) fn call<B, R>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> zbus::Result<R>
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

    /// Calls a method of this bus's interface on the portal object.
    pub(crate) fn call_portal<B>(&self, method: &str, body: &B) -> Result<()>
    where
        B: serde::Serialize + zbus::Type,
    {
        self.call::<_, ()>(DESKTOP_PATH, self.interface, method, body)
            .wrap_err_with(|| format!("the {} portal refused {method}", self.interface))
    }

    pub(crate) fn token(&mut self) -> String {
        self.next_token += 1;
        format!("hex{}_{}", std::process::id(), self.next_token)
    }

    /// Calls a portal method that answers through a Request's Response signal.
    pub(crate) fn request<B>(
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
        let interface = self.interface;
        self.call::<_, OwnedObjectPath>(DESKTOP_PATH, interface, method, &body(&token))
            .wrap_err_with(|| format!("the {interface} portal refused {method}"))?;
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            match self.signals.recv_timeout(STOP_POLL) {
                Ok(Signal::Response(answered, code, results)) if answered == path => {
                    return match code {
                        0 => Ok(results),
                        1 => Err(eyre!("the {interface} request {method} was dismissed")),
                        _ => Err(eyre!("the {interface} portal could not complete {method}")),
                    };
                }
                Ok(Signal::Session(session, event)) => self.pending.push_back((session, event)),
                Ok(Signal::Response(..)) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(eyre!("the portal connection closed"));
                }
            }
            if stop.load(Ordering::Acquire) {
                return Err(eyre!("the {interface} request {method} was canceled"));
            }
            if Instant::now() >= deadline {
                return Err(eyre!("the {interface} portal did not answer {method}"));
            }
        }
    }

    pub(crate) fn create_session(&mut self, stop: &AtomicBool) -> Result<OwnedObjectPath> {
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
        results
            .get("session_handle")
            .and_then(|handle| match <&str>::try_from(handle) {
                Ok(handle) => OwnedObjectPath::try_from(handle.to_owned()).ok(),
                Err(_) => handle
                    .try_clone()
                    .ok()
                    .and_then(|handle| OwnedObjectPath::try_from(handle).ok()),
            })
            .ok_or_else(|| eyre!("the {} portal returned no session", self.interface))
    }

    pub(crate) fn close_session(&self, session: &str) {
        if let Err(error) = self.call::<_, ()>(session, SESSION, "Close", &()) {
            tracing::warn!(%error, "could not close a portal session");
        }
    }

    /// The session signals that arrived since the last call.
    pub(crate) fn events(&mut self) -> Vec<(String, Event)> {
        let mut queued = std::mem::take(&mut self.pending);
        queued.extend(self.signals.try_iter().filter_map(|signal| match signal {
            Signal::Session(session, event) => Some((session, event)),
            Signal::Response(..) => None,
        }));
        queued.into()
    }
}

impl Drop for PortalBus {
    fn drop(&mut self) {
        // The portal closes every session of a vanished connection.
        let connection = self.connection.clone();
        let _ = self
            .runtime
            .block_on(async { tokio::time::timeout(CALL_TIMEOUT, connection.close()).await });
    }
}

/// The portal identifies a host app by its desktop entry. A loose binary
/// has none, so Hex writes a minimal `hex.desktop` that runs it, marked
/// managed so an install may replace it. Returns the entry it wrote.
fn ensure_desktop_entry() -> Result<Option<PathBuf>> {
    let Some(home) = dirs::data_dir() else {
        return Ok(None);
    };
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let installed = std::iter::once(home.clone())
        .chain(
            system
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        )
        .any(|dir| dir.join("applications/hex.desktop").exists());
    if installed {
        return Ok(None);
    }
    let executable = std::env::current_exe().wrap_err("could not find Hex's executable")?;
    let applications = home.join("applications");
    let entry = applications.join("hex.desktop");
    write_desktop_entry(&applications, &entry, &executable)
        .wrap_err_with(|| format!("could not write {}", entry.display()))?;
    tracing::info!(entry = %entry.display(), "wrote a desktop entry for the portal");
    Ok(Some(entry))
}

fn write_desktop_entry(applications: &Path, entry: &Path, executable: &Path) -> Result<()> {
    let executable = executable
        .to_str()
        .ok_or_else(|| eyre!("Hex's executable path is not UTF-8"))?;
    std::fs::create_dir_all(applications)?;
    let staged = applications.join(format!(".hex.desktop.{}", std::process::id()));
    let mut file = std::fs::File::create(&staged)?;
    file.write_all(desktop_entry(executable).as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&staged, entry)?;
    Ok(())
}

fn desktop_entry(executable: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Hex\nComment=Local voice dictation\nExec={}\nIcon=audio-input-microphone\nTerminal=false\nCategories=Utility;Accessibility;\nX-Hex-Managed=true\n",
        exec_argument(executable)
    )
}

/// One quoted Exec argument: quoting escapes, then string escapes, as the
/// Desktop Entry spec reads them, with `%` doubled for field codes.
fn exec_argument(argument: &str) -> String {
    let mut quoted = String::from("\"");
    for character in argument.chars() {
        match character {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(character);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// The GlobalShortcuts client.
pub(crate) struct Portal {
    bus: PortalBus,
    session: Option<OwnedObjectPath>,
}

impl Portal {
    /// `None` when the session has no shortcuts portal.
    pub(crate) fn connect(stop: &AtomicBool) -> Result<Option<Self>> {
        Ok(PortalBus::open(SHORTCUTS, stop)?.map(|bus| Self { bus, session: None }))
    }

    /// Whether the desktop can show its dialog for Hex's shortcuts
    /// (`ConfigureShortcuts`, version 2).
    pub(crate) fn configurable(&self) -> bool {
        self.bus.version >= 2
    }

    /// Opens the desktop's dialog for Hex's shortcuts; changes arrive as
    /// `Event::Changed`.
    pub(crate) fn configure(&self) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| eyre!("no shortcuts are bound"))?;
        self.bus.call_portal(
            "ConfigureShortcuts",
            &(session, "", HashMap::<&str, Variant<'_>>::new()),
        )
    }

    /// Binds Hex's shortcuts and returns the trigger the desktop describes
    /// for each one it bound, by id.
    pub(crate) fn bind(
        &mut self,
        shortcuts: &[Shortcut],
        stop: &AtomicBool,
    ) -> Result<HashMap<String, Option<String>>> {
        let session = self.bus.create_session(stop)?;
        self.session = Some(session.clone());
        self.bind_session(session, shortcuts, stop)
    }

    /// The presses, releases, and changes that arrived since the last call.
    pub(crate) fn events(&mut self) -> Vec<Event> {
        let shortcuts = self.session.as_ref().map(|session| session.as_str());
        self.bus
            .events()
            .into_iter()
            .filter_map(|(session, event)| (shortcuts == Some(session.as_str())).then_some(event))
            .collect()
    }

    fn bind_session(
        &mut self,
        session: OwnedObjectPath,
        shortcuts: &[Shortcut],
        stop: &AtomicBool,
    ) -> Result<HashMap<String, Option<String>>> {
        let requested = shortcuts
            .iter()
            .map(|shortcut| {
                let mut options =
                    HashMap::from([("description", Variant::from(shortcut.description))]);
                if let Some(trigger) = &shortcut.trigger {
                    options.insert("preferred_trigger", Variant::from(trigger.as_str()));
                }
                (shortcut.id.as_str(), options)
            })
            .collect::<Vec<_>>();
        let handle = session.into_inner();
        let results = self.bus.request(
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
        Ok(triggers(bound))
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
    fn a_loose_binary_gets_a_quoted_exec_line() {
        assert_eq!(
            exec_argument("/home/a/Downloads/hex"),
            "\"/home/a/Downloads/hex\""
        );
        assert_eq!(
            exec_argument("/tmp/my $dir/100%/a\"b\\c"),
            r#""/tmp/my \\$dir/100%%/a\\"b\\\\c""#
        );
        assert!(desktop_entry("/opt/hex").contains("\nExec=\"/opt/hex\"\n"));
    }

    #[test]
    fn an_empty_trigger_description_is_no_trigger() {
        let described = |trigger: &str| {
            HashMap::from([(
                "trigger_description".to_string(),
                OwnedVariant::try_from(Variant::from(trigger)).unwrap(),
            )])
        };
        let triggers = triggers(vec![
            ("dictation".into(), described("")),
            ("paste-last".into(), described("Ctrl+Alt+V")),
            ("capture".into(), Results::new()),
        ]);
        assert_eq!(triggers["dictation"], None);
        assert_eq!(triggers["paste-last"].as_deref(), Some("Ctrl+Alt+V"));
        assert_eq!(triggers["capture"], None);
    }

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
