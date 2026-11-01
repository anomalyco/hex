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
/// Hex's Linux app id: the portal names host apps by their desktop entry,
/// `anomaly.hex.desktop`, and Hex's windows report it as their Wayland
/// app_id and X11 WM_CLASS. GNOME's shortcuts provider before 51 rejects ids
/// that are not reverse-DNS (#579), so this matches the macOS bundle
/// identifier.
pub(crate) const APP_ID: &str = "anomaly.hex";
/// The app id and entry name before #579.
const LEGACY_APP_ID: &str = "hex";
const LEGACY_ENTRY: &str = "hex.desktop";
const MANAGED_MARK: &str = "X-Hex-Managed=true";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// A request can wait for the user to approve a desktop dialog.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// BindShortcuts waits as long as the desktop's approval dialog may
/// reasonably stay open. Giving up closes Hex's session, and KDE then keeps
/// every shortcut with no key and never asks again (#898).
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// How often a pending request checks for cancellation.
const STOP_POLL: Duration = Duration::from_millis(50);
/// The ids Cancel dictation had until it was removed (#786): `cancel`,
/// which asked for Escape before #492, then `cancel-dictation`, which the
/// user could put on any key. KDE keeps both, holding their keys.
const STALE_KDE_ACTIONS: [&str; 2] = ["cancel", "cancel-dictation"];
/// KDE's global shortcuts service, which keeps what portal apps bound.
const KGLOBALACCEL: &str = "org.kde.kglobalaccel";
const KGLOBALACCEL_PATH: &str = "/kglobalaccel";
const KGLOBALACCEL_INTERFACE: &str = "org.kde.KGlobalAccel";

/// One shortcut Hex asks the desktop for.
pub(crate) struct Shortcut {
    pub(crate) id: String,
    pub(crate) description: &'static str,
    /// XDG shortcuts format, such as `CTRL+ALT+space`; `None` leaves the
    /// shortcut for the user to assign.
    pub(crate) trigger: Option<String>,
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
    /// The portal knows Hex as `anomaly.hex` (xdg-desktop-portal 1.20 and
    /// newer register host apps); older ones may not know Hex's app id.
    pub(crate) registered: bool,
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
            registered: false,
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
        ) {
            if !unknown(&error) {
                // The portal finds the app id through anomaly.hex.desktop and the caller's
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
        } else {
            bus.registered = true;
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
        self.call_service(DESKTOP, path, interface, method, body)
    }

    /// Calls a method of another service on this bus.
    fn call_service<B, R>(
        &self,
        destination: &str,
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
                    .call_method(Some(destination), path, Some(interface), method, body),
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
        self.request_within(method, body, stop, RESPONSE_TIMEOUT)
    }

    /// [`Self::request`], waiting up to `timeout` for the answer.
    fn request_within<B>(
        &mut self,
        method: &str,
        body: impl FnOnce(&str) -> B,
        stop: &AtomicBool,
        timeout: Duration,
    ) -> Result<Results>
    where
        B: serde::Serialize + zbus::Type,
    {
        let token = self.token();
        let path = format!("{DESKTOP_PATH}/request/{}/{token}", self.sender);
        let interface = self.interface;
        self.call::<_, OwnedObjectPath>(DESKTOP_PATH, interface, method, &body(&token))
            .wrap_err_with(|| format!("the {interface} portal refused {method}"))?;
        let deadline = Instant::now() + timeout;
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

    /// Closes `session`. One the desktop already ended has no object left
    /// to close (GNOME 46 answers UnknownMethod, #739): that is closed too.
    pub(crate) fn close_session(&self, session: &str) {
        match self.call::<_, ()>(session, SESSION, "Close", &()) {
            Ok(()) => {}
            Err(error) if unknown(&error) => {
                tracing::debug!(%error, session, "the portal session was already closed");
            }
            Err(error) => tracing::warn!(%error, "could not close a portal session"),
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

/// Portal threads and startup check and write the launcher entry one at a
/// time, so none sees another's half-done write.
static ENTRY_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tidies Hex's launcher entry at startup: see [`DesktopEntries::tidy`].
pub(crate) fn tidy_desktop_entries() -> Result<()> {
    let _writing = ENTRY_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entries) = DesktopEntries::current() {
        entries.tidy()?;
        entries.repoint(&executable()?)?;
    }
    Ok(())
}

/// The portal identifies a host app by a desktop entry named for its id,
/// `anomaly.hex.desktop`, which installs ship: see [`DesktopEntries::ensure`].
/// What Hex's launcher entry starts: the same stable path as Launch at
/// login, so a version manager's upgrade doesn't strand it (#810).
fn executable() -> Result<String> {
    let executable = crate::linux::login_item::launcher()?;
    executable
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| eyre!("Hex's executable path is not UTF-8"))
}

fn ensure_desktop_entry() -> Result<Option<PathBuf>> {
    let _writing = ENTRY_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match DesktopEntries::current() {
        Some(entries) => entries.ensure(&executable()?),
        None => Ok(None),
    }
}

/// The system data directories from `XDG_DATA_DIRS`, which packages own.
pub(crate) fn system_data_dirs() -> Vec<PathBuf> {
    std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Hex's launcher entry: the user's `applications` directory, which Hex may
/// write, and the system ones from `XDG_DATA_DIRS`, which packages own.
struct DesktopEntries {
    applications: PathBuf,
    system: Vec<PathBuf>,
}

/// A desktop entry on disk. Hex reads and replaces only regular files.
enum Entry {
    Missing,
    File(String),
    /// A symlink or anything else the user put there.
    Other,
}

impl DesktopEntries {
    fn current() -> Option<Self> {
        let applications = dirs::data_dir()?.join("applications");
        let system = system_data_dirs()
            .into_iter()
            .map(|dir| dir.join("applications"))
            .collect();
        Some(Self::new(applications, system))
    }

    /// Leaves the user's own directory out of the system ones, as some
    /// profiles list it in `XDG_DATA_DIRS` too.
    fn new(applications: PathBuf, system: Vec<PathBuf>) -> Self {
        let canonical = |dir: &Path| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.into());
        let own = canonical(&applications);
        let system = system
            .into_iter()
            .filter(|dir| canonical(dir) != own)
            .collect();
        Self {
            applications,
            system,
        }
    }

    fn entry(&self) -> PathBuf {
        self.applications.join(format!("{APP_ID}.desktop"))
    }

    fn shipped(&self, name: &str) -> bool {
        self.system.iter().any(|dir| dir.join(name).exists())
    }

    /// Moves Hex's own managed `hex.desktop` (the name before #579) to
    /// `anomaly.hex.desktop`, keeping its Exec lines, unless the user put an
    /// entry of their own there; then removes a hidden fallback that would
    /// shadow a package's entry of the same id, or names Hex's icon in an
    /// entry Hex wrote with the generic one (#652). Returns the entry it wrote.
    fn tidy(&self) -> Result<Option<PathBuf>> {
        let entry = self.entry();
        let current = read_entry(&entry)?;
        let legacy = self.applications.join(LEGACY_ENTRY);
        if let Entry::File(old) = read_entry(&legacy)?
            && managed(&old)
        {
            let replace = match &current {
                Entry::Missing => true,
                Entry::File(current) => managed(current),
                Entry::Other => false,
            };
            if replace {
                write_entry(&self.applications, &entry, &moved_entry(&old))
                    .wrap_err_with(|| format!("could not write {}", entry.display()))?;
            }
            std::fs::remove_file(&legacy)
                .wrap_err_with(|| format!("could not remove {}", legacy.display()))?;
            tracing::info!(entry = %entry.display(), "moved Hex's desktop entry to its app id");
            return Ok(replace.then_some(entry));
        }
        if let Entry::File(current) = &current
            && managed(current)
            && no_display(current)
            && self.shipped(&format!("{APP_ID}.desktop"))
        {
            std::fs::remove_file(&entry)
                .wrap_err_with(|| format!("could not remove {}", entry.display()))?;
            tracing::info!(entry = %entry.display(), "removed Hex's fallback desktop entry");
        } else if let Entry::File(current) = &current
            && managed(current)
            && with_icon(current) != *current
        {
            write_entry(&self.applications, &entry, &with_icon(current))
                .wrap_err_with(|| format!("could not write {}", entry.display()))?;
            tracing::info!(entry = %entry.display(), "named Hex's icon in its desktop entry");
        }
        Ok(None)
    }

    /// Points Hex's own launcher entry at `executable` when the program it
    /// starts is gone, as after a version manager (mise, asdf) removes the
    /// version that wrote it. An entry whose program is still there is left
    /// as it is, and so is any entry Hex didn't write.
    fn repoint(&self, executable: &str) -> Result<()> {
        let entry = self.entry();
        let Entry::File(current) = read_entry(&entry)? else {
            return Ok(());
        };
        let Some(program) = crate::linux::login_item::program(&current) else {
            return Ok(());
        };
        if !managed(&current)
            || !crate::linux::login_item::superseded(&program, Path::new(executable))
        {
            return Ok(());
        }
        // A package now ships Hex's entry: the stranded fallback only hid it.
        if !program.is_file() && self.shipped(&format!("{APP_ID}.desktop")) {
            std::fs::remove_file(&entry)
                .wrap_err_with(|| format!("could not remove {}", entry.display()))?;
            tracing::info!(entry = %entry.display(), "removed Hex's stranded desktop entry");
            return Ok(());
        }
        let repointed = desktop_entry(executable, no_display(&current));
        if repointed == current {
            return Ok(());
        }
        write_entry(&self.applications, &entry, &repointed)
            .wrap_err_with(|| format!("could not write {}", entry.display()))?;
        tracing::info!(entry = %entry.display(), "pointed Hex's desktop entry at this Hex");
        Ok(())
    }

    /// After [`Self::tidy`], with no entry anywhere, writes a minimal one
    /// that runs `executable`, marked managed so an install may replace it,
    /// and hidden from launchers when an older package's or the user's own
    /// `hex.desktop` already shows Hex. Returns the entry it wrote.
    fn ensure(&self, executable: &str) -> Result<Option<PathBuf>> {
        let moved = self.tidy()?;
        self.repoint(executable)?;
        if moved.is_some() {
            return Ok(moved);
        }
        let entry = self.entry();
        if !matches!(read_entry(&entry)?, Entry::Missing)
            || self.shipped(&format!("{APP_ID}.desktop"))
        {
            return Ok(None);
        }
        let hidden = self.shipped(LEGACY_ENTRY)
            || !matches!(
                read_entry(&self.applications.join(LEGACY_ENTRY))?,
                Entry::Missing
            );
        write_entry(
            &self.applications,
            &entry,
            &desktop_entry(executable, hidden),
        )
        .wrap_err_with(|| format!("could not write {}", entry.display()))?;
        tracing::info!(entry = %entry.display(), "wrote a desktop entry for the portal");
        Ok(Some(entry))
    }
}

fn read_entry(path: &Path) -> Result<Entry> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => std::fs::read_to_string(path)
            .map(Entry::File)
            .wrap_err_with(|| format!("could not read {}", path.display())),
        Ok(_) => Ok(Entry::Other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Entry::Missing),
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
    }
}

fn managed(entry: &str) -> bool {
    entry.lines().any(|line| line.trim() == MANAGED_MARK)
}

fn no_display(entry: &str) -> bool {
    entry.lines().any(|line| line.trim() == "NoDisplay=true")
}

fn write_entry(applications: &Path, entry: &Path, contents: &str) -> Result<()> {
    std::fs::create_dir_all(applications)?;
    let staged = applications.join(format!(".{APP_ID}.desktop.{}", std::process::id()));
    let mut file = std::fs::File::create(&staged)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&staged, entry)?;
    Ok(())
}

/// `entry` with every `Icon=` naming Hex's icon (`anomaly.hex` in hicolor).
pub(crate) fn with_icon(entry: &str) -> String {
    let mut named = String::with_capacity(entry.len());
    for line in entry.lines() {
        if line.trim_start().starts_with("Icon=") {
            named.push_str(&format!("Icon={APP_ID}"));
        } else {
            named.push_str(line);
        }
        named.push('\n');
    }
    named
}

/// An old managed entry under the new name, keeping its Exec lines, naming
/// Hex's windows for X11 launchers, and showing Hex's icon.
fn moved_entry(old: &str) -> String {
    let mut entry = String::with_capacity(old.len() + 32);
    for line in old
        .lines()
        .filter(|line| !line.trim_start().starts_with("StartupWMClass="))
    {
        entry.push_str(line);
        entry.push('\n');
        if line.trim() == "[Desktop Entry]" {
            entry.push_str(&format!("StartupWMClass={APP_ID}\n"));
        }
    }
    with_icon(&entry)
}

/// `NoDisplay`, not `Hidden`: the portal refuses an app whose entry is Hidden.
fn desktop_entry(executable: &str, hidden: bool) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Hex\nComment=Local voice dictation\nExec={}\nIcon={APP_ID}\nTerminal=false\nCategories=Utility;Accessibility;\nStartupWMClass={APP_ID}\n{}{MANAGED_MARK}\n",
        exec_argument(executable),
        if hidden { "NoDisplay=true\n" } else { "" }
    )
}

/// One quoted Exec argument: quoting escapes, then string escapes, as the
/// Desktop Entry spec reads them, with `%` doubled for field codes.
pub(crate) fn exec_argument(argument: &str) -> String {
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
        let bound = self.bind_session(session, shortcuts, stop);
        if desktop_is("KDE") {
            match &bound {
                // Approved: whatever KDE keeps now, the person chose.
                Ok(granted) if granted.values().any(Option::is_some) => {
                    kde_unapproved::clear();
                }
                Ok(_) => {}
                // Dismissed, canceled, or abandoned: KDE keeps these ids
                // with no key and never asks for them again (#898).
                Err(_) => kde_unapproved::remember(shortcuts),
            }
        }
        bound
    }

    /// Clears the shortcuts GNOME keeps listed for Hex once its session
    /// closes (#575). GNOME Settings stores exactly the set an app binds and
    /// asks for approval only for shortcuts it has not stored, so binding none
    /// in a fresh session empties the list without a dialog; binding them again
    /// asks again. Only on GNOME: KDE would drop keys the person chose for Hex's
    /// shortcuts (#482).
    pub(crate) fn forget(mut self) -> Result<()> {
        if !gnome() {
            return Ok(());
        }
        if let Some(session) = self.session.take() {
            self.bus.close_session(session.as_str());
        }
        let stop = AtomicBool::new(false);
        let session = self.bus.create_session(&stop)?;
        self.bind_session(session, &[], &stop)?;
        tracing::info!("cleared Hex's shortcuts from GNOME Settings");
        Ok(())
    }

    /// Before #579 Hex registered with the portal as `hex`, and before #492
    /// its Cancel dictation id `cancel` asked for Escape. KDE keeps every
    /// shortcut a portal app bound under that app's id, so an upgraded Hex
    /// left `hex`'s set (Cancel dictation on Escape among it) grabbed with
    /// nothing able to rebind it, taking Escape from every app (#624).
    /// Removes that set, the [`STALE_KDE_ACTIONS`], and every other action
    /// under the current id that `keep` (the ids about to be bound) doesn't
    /// name from KDE's global shortcuts. A shortcut changed in Hex gets a new
    /// id, and KDE keeps the old one holding its key, so the new id could
    /// never take that key back (#898). Only on KDE, only Hex's entries.
    pub(crate) fn forget_legacy_kde_shortcuts(&self, keep: &[&str]) -> Result<()> {
        if !desktop_is("KDE") {
            return Ok(());
        }
        let list = |component: &str| -> Result<Vec<Vec<String>>> {
            self.bus
                .call_service(
                    KGLOBALACCEL,
                    KGLOBALACCEL_PATH,
                    KGLOBALACCEL_INTERFACE,
                    "allActionsForComponent",
                    &(vec![component],),
                )
                .wrap_err("could not list Hex's old KDE shortcuts")
        };
        let legacy = list(LEGACY_APP_ID)?;
        let current = list(APP_ID)?;
        for (component, action) in superseded_kde_actions(&legacy, &current, keep) {
            let removed: bool = self
                .bus
                .call_service(
                    KGLOBALACCEL,
                    KGLOBALACCEL_PATH,
                    KGLOBALACCEL_INTERFACE,
                    "unregister",
                    &(component, action),
                )
                .wrap_err_with(|| format!("could not remove Hex's old KDE shortcut {action}"))?;
            if removed {
                tracing::info!(component, action, "removed an old Hex shortcut from KDE");
            }
        }
        // Shortcuts a bind left unapproved, which KDE keeps with no key: gone,
        // KDE's dialog asks for them again. A key set since in System Settings
        // stays (#898).
        let unapproved = kde_unapproved::ids();
        for action in current
            .iter()
            .filter_map(|action| action.get(1))
            .filter(|action| unapproved.iter().any(|id| id == *action))
        {
            let keys: Vec<(Vec<i32>,)> = self
                .bus
                .call_service(
                    KGLOBALACCEL,
                    KGLOBALACCEL_PATH,
                    KGLOBALACCEL_INTERFACE,
                    "shortcutKeys",
                    &(vec![APP_ID, action.as_str()],),
                )
                .wrap_err_with(|| format!("could not read Hex's KDE shortcut {action}"))?;
            if keys.iter().any(|(keys,)| keys.iter().any(|key| *key != 0)) {
                continue;
            }
            let removed: bool = self
                .bus
                .call_service(
                    KGLOBALACCEL,
                    KGLOBALACCEL_PATH,
                    KGLOBALACCEL_INTERFACE,
                    "unregister",
                    &(APP_ID, action.as_str()),
                )
                .wrap_err_with(|| format!("could not remove Hex's KDE shortcut {action}"))?;
            if removed {
                tracing::info!(
                    action,
                    "removed a Hex shortcut KDE kept without a key after its dialog went unanswered; KDE asks again"
                );
            }
        }
        kde_unapproved::clear();
        Ok(())
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
        let results = self.bus.request_within(
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
            APPROVAL_TIMEOUT,
        )?;
        let bound = results
            .get("shortcuts")
            .and_then(|shortcuts| shortcuts.try_clone().ok())
            .and_then(|shortcuts| Vec::<(String, Results)>::try_from(shortcuts).ok())
            .unwrap_or_default();
        Ok(triggers(bound))
    }
}

/// The shortcut ids of a KDE bind whose approval dialog was dismissed,
/// canceled, or abandoned (Hex stopped while it was open), kept in
/// Application Support until the next bind on KDE (#898).
mod kde_unapproved {
    use std::path::PathBuf;

    fn path() -> Option<PathBuf> {
        crate::app_paths::support_dir()
            .ok()
            .map(|dir| dir.join("kde-unapproved-shortcuts"))
    }

    pub(super) fn remember(shortcuts: &[super::Shortcut]) {
        let Some(path) = path() else { return };
        let ids = shortcuts
            .iter()
            .map(|shortcut| format!("{}\n", shortcut.id))
            .collect::<String>();
        if let Err(error) = std::fs::write(&path, ids) {
            tracing::warn!(%error, "could not note Hex's unapproved KDE shortcuts");
        }
    }

    pub(super) fn ids() -> Vec<String> {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    pub(super) fn clear() {
        if let Some(path) = path() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The KDE actions to remove before binding: every action of the pre-#579
/// `hex` component, then every action under `anomaly.hex` that `keep` doesn't
/// name, the [`STALE_KDE_ACTIONS`] included. Each action is kglobalaccel's
/// `[component, action, …]` id.
fn superseded_kde_actions<'a>(
    legacy: &'a [Vec<String>],
    current: &'a [Vec<String>],
    keep: &[&str],
) -> Vec<(&'static str, &'a str)> {
    let mut actions = legacy
        .iter()
        .filter_map(|action| action.get(1))
        .map(|action| (LEGACY_APP_ID, action.as_str()))
        .collect::<Vec<_>>();
    let mut current = current
        .iter()
        .filter_map(|action| action.get(1))
        .map(String::as_str)
        .chain(STALE_KDE_ACTIONS)
        .filter(|action| !keep.contains(action))
        .collect::<Vec<_>>();
    current.sort_unstable();
    current.dedup();
    actions.extend(current.into_iter().map(|action| (APP_ID, action)));
    actions
}

/// What the log says when the session has no shortcuts portal at startup.
pub(crate) const MISSING_SHORTCUTS_PORTAL: &str = "no GlobalShortcuts portal; dictation follows the hex commands. \
If this desktop has one, xdg-desktop-portal may have started before the desktop session and picked the wrong \
backend: restart it (systemctl --user restart xdg-desktop-portal) and Hex binds its shortcuts at its next check";

/// Looks for the shortcuts portal again from time to time, after 15 s and
/// then doubling up to every 2 minutes. A portal started before the
/// desktop's session environment exists exposes none until it is restarted
/// (#875). Each check runs on its own thread, so a slow bus never holds back
/// the caller.
pub(crate) struct PortalProbe {
    stop: std::sync::Arc<AtomicBool>,
    retry: Duration,
    next: Instant,
    running: Option<std::thread::JoinHandle<Result<Option<Portal>>>>,
    warned: bool,
}

impl PortalProbe {
    const FIRST: Duration = Duration::from_secs(15);
    const MAX: Duration = Duration::from_secs(120);

    pub(crate) fn new(stop: std::sync::Arc<AtomicBool>) -> Self {
        Self {
            stop,
            retry: Self::FIRST,
            next: Instant::now() + Self::FIRST,
            running: None,
            warned: false,
        }
    }

    /// The portal, once a check found it.
    pub(crate) fn poll(&mut self) -> Result<Option<Portal>> {
        if self
            .running
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            match self.running.take().map(std::thread::JoinHandle::join) {
                Some(Ok(Ok(Some(portal)))) => {
                    tracing::info!("the GlobalShortcuts portal appeared; binding Hex's shortcuts");
                    return Ok(Some(portal));
                }
                Some(Ok(Err(error))) if !self.warned => {
                    self.warned = true;
                    tracing::warn!(
                        error = format!("{error:#}"),
                        "could not check for the GlobalShortcuts portal"
                    );
                }
                _ => {}
            }
            self.retry = (self.retry * 2).min(Self::MAX);
            self.next = Instant::now() + self.retry;
        } else if self.running.is_none() && Instant::now() >= self.next {
            let stop = self.stop.clone();
            self.running = Some(
                std::thread::Builder::new()
                    .name("hex-portal-probe".into())
                    .spawn(move || Portal::connect(&stop))
                    .wrap_err("could not start the shortcuts portal check")?,
            );
        }
        Ok(None)
    }
}

const GNOME_SHORTCUTS: &str = "org.gnome.settings-daemon.global-shortcuts";

/// Before #579 Hex registered with the portal as `hex`. GNOME Settings keeps
/// that set listed beside `anomaly.hex`, and nothing can bind or clear it
/// through the portal again (GNOME 49 and later refuse the id), so Hex removes
/// it from GNOME's own settings once (#575). Only on GNOME, only that entry.
pub(crate) fn forget_legacy_gnome_shortcuts() -> Result<()> {
    use crate::linux::gsettings::{gsettings, string_list, string_list_text};
    if !gnome() {
        return Ok(());
    }
    // No gsettings or no GNOME schema: nothing to clear.
    let Ok(listed) = gsettings(&["get", GNOME_SHORTCUTS, "applications"]) else {
        return Ok(());
    };
    let applications = string_list(&listed);
    if !applications.iter().any(|app| app == LEGACY_APP_ID) {
        return Ok(());
    }
    gsettings(&[
        "reset",
        &format!(
            "{GNOME_SHORTCUTS}.application:/org/gnome/settings-daemon/global-shortcuts/{LEGACY_APP_ID}/"
        ),
        "shortcuts",
    ])?;
    let kept = applications
        .iter()
        .filter(|app| *app != LEGACY_APP_ID)
        .map(String::as_str)
        .collect::<Vec<_>>();
    gsettings(&[
        "set",
        GNOME_SHORTCUTS,
        "applications",
        &string_list_text(&kept),
    ])?;
    tracing::info!("removed Hex's pre-#579 shortcuts from GNOME Settings");
    Ok(())
}

/// Whether the session's desktop is GNOME.
pub(crate) fn gnome() -> bool {
    desktop_is("GNOME")
}

/// Whether the desktop keeps Alt+Space for itself: GNOME's window menu
/// (#723) and KDE Plasma's KRunner (#898), which the KDE portal's approval
/// dialog flags as a conflict and then leaves Hex without.
pub(crate) fn desktop_takes_alt_space() -> bool {
    gnome() || desktop_is("KDE")
}

/// Whether the desktop assigns portal shortcuts their keys in its own
/// configuration and describes no trigger in the portal's answer. Hyprland's
/// portal returns an empty description for every shortcut, bound or not
/// (`bind = …, global, anomaly.hex:…` in its config), so an empty one there
/// is no refusal.
pub(crate) fn desktop_assigns_triggers() -> bool {
    desktop_is("Hyprland")
}

fn desktop_is(name: &str) -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| {
        desktops
            .split(':')
            .any(|desktop| desktop.eq_ignore_ascii_case(name))
    })
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

    /// A session the desktop already ended answers Close with UnknownMethod
    /// or UnknownObject (GNOME 46, #739); Hex counts it as closed.
    #[test]
    fn a_vanished_session_counts_as_closed() {
        let gone = |error: zbus::fdo::Error| unknown(&zbus::Error::FDO(Box::new(error)));
        assert!(gone(zbus::fdo::Error::UnknownMethod(
            "Object does not exist".into()
        )));
        assert!(gone(zbus::fdo::Error::UnknownObject("gone".into())));
        assert!(!gone(zbus::fdo::Error::AccessDenied("no".into())));
    }

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
        assert!(desktop_entry("/opt/hex", false).contains("\nExec=\"/opt/hex\"\n"));
        assert!(!desktop_entry("/opt/hex", false).contains("NoDisplay"));
        assert!(desktop_entry("/opt/hex", true).contains("\nNoDisplay=true\n"));
        assert!(desktop_entry("/opt/hex", false).contains("\nStartupWMClass=anomaly.hex\n"));
    }

    #[test]
    fn a_moved_entry_keeps_its_commands_and_names_the_window_class() {
        let old = "[Desktop Entry]\nName=Hex\nExec=\"/home/a/.local/bin/hex\"\nStartupWMClass=hex\nX-Hex-Managed=true\n\n[Desktop Action quit]\nExec=\"/home/a/.local/bin/hex\" quit\n";
        assert_eq!(
            moved_entry(old),
            "[Desktop Entry]\nStartupWMClass=anomaly.hex\nName=Hex\nExec=\"/home/a/.local/bin/hex\"\nX-Hex-Managed=true\n\n[Desktop Action quit]\nExec=\"/home/a/.local/bin/hex\" quit\n"
        );
    }

    fn entries(name: &str) -> (PathBuf, DesktopEntries) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("hex-desktop-{name}-{unique}"));
        let applications = root.join("home/applications");
        let system = root.join("system/applications");
        std::fs::create_dir_all(&applications).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        let entries = DesktopEntries::new(applications.clone(), vec![applications, system]);
        (root, entries)
    }

    const OLD: &str =
        "[Desktop Entry]\nName=Hex\nExec=\"/home/a/.local/bin/hex\"\nX-Hex-Managed=true\n";

    #[test]
    fn entries_hex_wrote_name_its_icon() {
        let (root, entries) = entries("icon");
        std::fs::write(
            entries.entry(),
            "[Desktop Entry]\nIcon=audio-input-microphone\nX-Hex-Managed=true\n",
        )
        .unwrap();
        entries.tidy().unwrap();
        let named = std::fs::read_to_string(entries.entry()).unwrap();
        assert!(named.contains("\nIcon=anomaly.hex\n"), "{named}");
        assert!(desktop_entry("/opt/hex", false).contains("\nIcon=anomaly.hex\n"));
        // Someone else's entry keeps its icon.
        std::fs::write(entries.entry(), "[Desktop Entry]\nIcon=mine\n").unwrap();
        entries.tidy().unwrap();
        assert!(
            std::fs::read_to_string(entries.entry())
                .unwrap()
                .contains("Icon=mine")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_old_managed_entry_moves_to_the_app_id() {
        let (root, entries) = entries("move");
        std::fs::write(entries.applications.join("hex.desktop"), OLD).unwrap();
        assert_eq!(entries.ensure("/opt/hex").unwrap(), Some(entries.entry()));
        assert!(!entries.applications.join("hex.desktop").exists());
        let moved = std::fs::read_to_string(entries.entry()).unwrap();
        assert!(moved.contains("Exec=\"/home/a/.local/bin/hex\"\n"));
        assert!(moved.contains("StartupWMClass=anomaly.hex\n"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn entries_hex_did_not_write_stay() {
        let (root, entries) = entries("unmanaged");
        let user = entries.applications.join("hex.desktop");
        std::fs::write(&user, "[Desktop Entry]\nName=Mine\n").unwrap();
        std::os::unix::fs::symlink(&user, entries.entry()).unwrap();
        entries.ensure("/opt/hex").unwrap();
        assert!(user.exists());
        assert!(entries.entry().is_symlink());
        // A symlinked old entry is the user's too, even with Hex's mark.
        std::fs::remove_file(entries.entry()).unwrap();
        let target = root.join("managed.desktop");
        std::fs::write(&target, OLD).unwrap();
        std::fs::remove_file(&user).unwrap();
        std::os::unix::fs::symlink(&target, &user).unwrap();
        entries.tidy().unwrap();
        assert!(user.is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Hex's own entry whose program is gone points at this Hex; one whose
    /// program is there, or the user's, stays (#810).
    #[test]
    fn a_stranded_fallback_follows_hex() {
        let (root, entries) = entries("repoint");
        let program = root.join("hex");
        std::fs::write(&program, "").unwrap();
        let program = program.to_str().unwrap();
        let target = root.join("new-hex");
        std::fs::write(&target, "").unwrap();
        let target = target.to_str().unwrap();
        entries.ensure(program).unwrap();
        let written = std::fs::read_to_string(entries.entry()).unwrap();
        entries.repoint(target).unwrap();
        assert_eq!(std::fs::read_to_string(entries.entry()).unwrap(), written);
        std::fs::remove_file(program).unwrap();
        entries.repoint(target).unwrap();
        assert_eq!(
            std::fs::read_to_string(entries.entry()).unwrap(),
            desktop_entry(target, false)
        );
        // Nothing to point at: left as it is.
        std::fs::remove_file(target).unwrap();
        std::fs::write(entries.entry(), desktop_entry(program, false)).unwrap();
        entries.repoint(target).unwrap();
        assert_eq!(
            std::fs::read_to_string(entries.entry()).unwrap(),
            desktop_entry(program, false)
        );
        // A package ships Hex's entry now: the stranded fallback goes.
        std::fs::write(
            entries.system[0].join("anomaly.hex.desktop"),
            "[Desktop Entry]\n",
        )
        .unwrap();
        std::fs::write(root.join("new-hex"), "").unwrap();
        entries.repoint(target).unwrap();
        assert!(!entries.entry().exists());
        let users = "[Desktop Entry]\nExec=/gone/hex\n";
        std::fs::write(entries.entry(), users).unwrap();
        entries.repoint(target).unwrap();
        assert_eq!(std::fs::read_to_string(entries.entry()).unwrap(), users);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A fallback already there is never written again (#810), and one that
    /// a data directory provides is never written at all.
    #[test]
    fn the_fallback_is_written_once_and_only_when_nothing_provides_it() {
        let (root, fallback) = entries("once");
        assert_eq!(fallback.ensure("/opt/hex").unwrap(), Some(fallback.entry()));
        let written = std::fs::metadata(fallback.entry())
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(fallback.ensure("/opt/hex").unwrap(), None);
        assert_eq!(fallback.tidy().unwrap(), None);
        assert_eq!(
            std::fs::metadata(fallback.entry())
                .unwrap()
                .modified()
                .unwrap(),
            written
        );
        std::fs::remove_dir_all(root).unwrap();
        let (root, provided) = entries("provided");
        std::fs::write(
            provided.system[0].join("anomaly.hex.desktop"),
            "[Desktop Entry]\n",
        )
        .unwrap();
        assert_eq!(provided.ensure("/opt/hex").unwrap(), None);
        assert!(!provided.entry().exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_fallback_hides_beside_the_users_old_entry_and_yields_to_a_package() {
        let (root, entries) = entries("fallback");
        // The user's own directory listed in XDG_DATA_DIRS is not a package.
        assert_eq!(entries.system.len(), 1);
        std::fs::write(
            entries.applications.join("hex.desktop"),
            "[Desktop Entry]\n",
        )
        .unwrap();
        assert_eq!(entries.ensure("/opt/hex").unwrap(), Some(entries.entry()));
        assert!(
            std::fs::read_to_string(entries.entry())
                .unwrap()
                .contains("NoDisplay=true")
        );
        assert_eq!(entries.ensure("/opt/hex").unwrap(), None);
        assert!(entries.entry().exists());
        std::fs::write(
            entries.system[0].join("anomaly.hex.desktop"),
            "[Desktop Entry]\n",
        )
        .unwrap();
        entries.tidy().unwrap();
        assert!(!entries.entry().exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// GNOME before 51 refuses a portal app id that is not reverse-DNS (#579).
    #[test]
    fn the_portal_app_id_is_reverse_dns() {
        let parts = APP_ID.split('.').collect::<Vec<_>>();
        assert!(parts.len() >= 2);
        assert!(parts.iter().all(|part| {
            part.chars()
                .next()
                .is_some_and(|first| !first.is_ascii_digit())
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }));
    }

    /// A shortcut changed in Hex gets a new id; on KDE the old id keeps its
    /// key until removed, so the new one could never take it (#898).
    #[test]
    fn kde_actions_hex_no_longer_binds_are_removed() {
        let id = |component: &str, action: &str| {
            vec![
                component.to_string(),
                action.to_string(),
                "Hex".to_string(),
                action.to_string(),
            ]
        };
        let legacy = vec![id("hex", "dictation")];
        let current = vec![
            id(APP_ID, "dictation-ALT-space"),
            id(APP_ID, "dictation-CTRL-ALT-space"),
            id(APP_ID, "paste-last-ALT-SHIFT-v"),
            id(APP_ID, "cancel"),
        ];
        let removed = superseded_kde_actions(
            &legacy,
            &current,
            &["dictation-CTRL-ALT-space", "paste-last-ALT-SHIFT-v"],
        );
        assert_eq!(
            removed,
            vec![
                ("hex", "dictation"),
                (APP_ID, "cancel"),
                (APP_ID, "cancel-dictation"),
                (APP_ID, "dictation-ALT-space"),
            ]
        );
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
