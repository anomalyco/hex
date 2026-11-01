//! The single-instance socket: one Hex process per user and data root.
//!
//! The running process binds `service.sock` next to its `service.lock`. A
//! later `hex` forwards `Activate` (with the launcher's activation token) and
//! exits; Annotate's recording commands and `Status` ride the same socket.
//! The `hex` control commands (`crate::control`) reach Hex through its local
//! API instead, answered by `linux_app`'s control host; dictation commands go
//! from there straight to the listener's command queue, never through the
//! window's thread. This module also owns the
//! per-user systemd lifecycle (`Type=notify` readiness, the session
//! environment, start, stop, and restart).
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::desktop::host::DesktopSnapshot;
use crate::linux::input::{ListenerCommand, ShortcutCommand};

/// Version 1 was the service/Settings-client protocol; its clients get a
/// version error and exit.
const VERSION: u32 = 2;
const MAX_FRAME: usize = 128 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENTS: usize = 8;
/// A send waits for the router's answer (it waits 8 s itself).
const SEND_WAIT: Duration = Duration::from_secs(10);
const UNIT: &str = "hex.service";
/// How long a launch waits for a started unit's socket. `Type=notify`
/// units are ready when `systemctl start` returns; units an older installer
/// wrote are `Type=simple` and need the wait.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Request {
    /// A relaunch: open or focus the window when `show`. A launcher's
    /// activation token rides along for the compositor.
    Activate {
        activation: Activation,
        show: bool,
        session: Session,
    },
    /// `hex annotate start|stop|cancel`.
    Capture(crate::control::ScrawlCaptureAction),
    /// `hex annotate send`: the listener's router sends it and says how.
    CaptureSend {
        capture: String,
        to: Option<String>,
        always: bool,
    },
    /// `hex annotate undo`: takes the newest send back.
    CaptureUndo,
    /// `hex annotate draw`: the pen on or off, as the capsule's pen.
    CaptureDraw,
    /// `hex annotate start --area`: records that area (X11).
    CaptureArea {
        rect: [f64; 4],
    },
    /// `hex annotate status`: what the listener's capture host reports.
    CaptureStatus,
    Status,
}

/// What a launcher handed the process that asked for the window.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Activation {
    /// Wayland `xdg_activation_v1` token (`XDG_ACTIVATION_TOKEN`).
    pub(crate) token: Option<String>,
    /// X11 startup notification id (`DESKTOP_STARTUP_ID`).
    pub(crate) startup_id: Option<String>,
}

impl Activation {
    /// Takes this process's activation environment so nothing it starts
    /// reuses a token meant for one window.
    pub(crate) fn take_from_environment() -> Self {
        let take = |key: &str| {
            let value = std::env::var(key).ok().filter(|value| !value.is_empty());
            // Called on the main thread before Hex starts any other thread.
            unsafe { std::env::remove_var(key) };
            value
        };
        Self {
            token: take("XDG_ACTIVATION_TOKEN"),
            startup_id: take("DESKTOP_STARTUP_ID"),
        }
    }
}

/// A saved `XAUTHORITY` outlives its file: Xwayland's cookie (GNOME's
/// `/run/user/…/.mutter-Xwaylandauth.*`) is new each login, so the path in an
/// older `session.env` breaks every X11 connection after a reboot. A missing
/// file takes the user manager's current value, else is dropped so Xlib falls
/// back to `~/.Xauthority`.
pub(crate) fn refresh_x_authority() {
    let Some(saved) = std::env::var_os("XAUTHORITY").filter(|path| !path.is_empty()) else {
        return;
    };
    if Path::new(&saved).exists() {
        return;
    }
    let live = manager_environment("XAUTHORITY").filter(|path| Path::new(path).exists());
    // Called on the main thread before Hex starts any other thread.
    unsafe {
        match live {
            Some(path) => std::env::set_var("XAUTHORITY", path),
            None => std::env::remove_var("XAUTHORITY"),
        }
    }
}

/// A service the user manager started before the desktop imported its
/// environment has no display: Hyprland, for one, imports nothing into
/// systemd unless its config runs `dbus-update-activation-environment`
/// (#756). Adopts the user manager's current `WAYLAND_DISPLAY`, else the
/// newest Wayland socket of this user's in `XDG_RUNTIME_DIR` that accepts a
/// connection. Returns the display it adopted. Called on the main thread
/// before Hex starts any other thread.
pub(crate) fn adopt_wayland_display() -> Option<String> {
    if graphical_session() {
        return None;
    }
    let uid = unsafe { libc::getuid() };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
    // A custom data root never asks the user's systemd manager (see `supervised`).
    let managed = if custom_root() {
        None
    } else {
        manager_environment("WAYLAND_DISPLAY")
    };
    let display = managed
        .filter(|display| wayland_socket(&runtime.join(display), uid))
        .or_else(|| newest_wayland_socket(&runtime, uid))?;
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &runtime);
        std::env::set_var("WAYLAND_DISPLAY", &display);
    }
    Some(display)
}

/// The newest `wayland-*` socket in `runtime` that `uid` owns and that
/// accepts a connection.
fn newest_wayland_socket(runtime: &Path, uid: u32) -> Option<String> {
    let mut sockets = fs::read_dir(runtime)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let metadata = entry.metadata().ok()?;
            (name.starts_with("wayland-")
                && !name.ends_with(".lock")
                && metadata.file_type().is_socket()
                && metadata.uid() == uid)
                .then(|| (metadata.modified().ok(), name))
        })
        .collect::<Vec<_>>();
    sockets.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    sockets
        .into_iter()
        .map(|(_, name)| name)
        .find(|name| wayland_socket(&runtime.join(name), uid))
}

fn wayland_socket(path: &Path, uid: u32) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == uid)
        && UnixStream::connect(path).is_ok()
}

/// A variable in the systemd user manager's environment, which the desktop
/// session imports at each login.
fn manager_environment(key: &str) -> Option<String> {
    let output = std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    environment_value(&String::from_utf8_lossy(&output.stdout), key)
}

fn environment_value(environment: &str, key: &str) -> Option<String> {
    environment.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix('=')?;
        (!value.is_empty()).then(|| value.to_owned())
    })
}

/// What `hex status` prints and every reply carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Status {
    pub(crate) desktop: DesktopSnapshot,
    pub(crate) settings: crate::app_settings::AppSettings,
    pub(crate) editing: bool,
    pub(crate) hud_limitation: Option<String>,
    pub(crate) window_open: bool,
    pub(crate) pid: u32,
    pub(crate) session: Session,
    /// The Labs features turned on (`crate::labs`).
    #[serde(default)]
    pub(crate) labs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Session {
    backend: String,
    display: String,
    runtime: String,
}

impl Session {
    pub(crate) fn current() -> Self {
        let session = crate::linux::session::LinuxSession::detect();
        Self {
            backend: session.as_str().into(),
            display: std::env::var(if session.is_wayland() {
                "WAYLAND_DISPLAY"
            } else {
                "DISPLAY"
            })
            .unwrap_or_default(),
            runtime: std::env::var("XDG_RUNTIME_DIR").unwrap_or_default(),
        }
    }
}

/// Where the socket hands a `hex` shortcut command: the running listener's
/// queue, once it reports its session.
#[derive(Clone)]
pub(crate) struct ShortcutRoute {
    pub(crate) commands: SyncSender<ListenerCommand>,
}

/// Routes one command, failing fast when nothing listens.
pub(crate) fn route_shortcut(
    route: Option<&ShortcutRoute>,
    command: ShortcutCommand,
) -> Result<()> {
    let route = route
        .ok_or_else(|| eyre!("Hex is not listening; enable it in Settings or run `hex start`"))?;
    route
        .commands
        .try_send(command.into())
        .map_err(|_| eyre!("Hex is busy; try again"))
}

/// What the socket reads without waiting on the host or the window: the
/// latest status and the listener's command queue, both published by the
/// host thread.
#[derive(Default)]
pub(crate) struct Published {
    pub(crate) status: Mutex<Option<Status>>,
    pub(crate) shortcuts: Mutex<Option<ShortcutRoute>>,
}

/// Requests for the main thread, which owns the window.
#[derive(Debug)]
pub(crate) enum Control {
    Activate(Activation),
    /// `hex window show`: opens the window at a pane.
    Show(Option<crate::control::Pane>),
    /// `hex window hide`.
    Hide,
    /// Replays the first run in the window, answering whether it opened.
    OnboardingReset(SyncSender<std::result::Result<(), String>>),
    Quit,
}

#[derive(Serialize, Deserialize)]
struct Message {
    version: u32,
    request: Request,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    version: u32,
    #[serde(default)]
    status: Option<Status>,
    error: Option<String>,
    /// What an Annotate command did, for the CLI to print.
    #[serde(default)]
    control: Option<crate::control::ControlReply>,
}

fn socket_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("service.sock"))
}

fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        bail!("Hex requires a user-owned, non-symlink data directory");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn check_peer(stream: &UnixStream) -> Result<()> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 || credentials.uid != unsafe { libc::geteuid() } {
        bail!("Hex only accepts local IPC from the same user");
    }
    Ok(())
}

fn read_exact(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| eyre!("Hex read timed out"))?;
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(bytes)?;
        if count == 0 {
            bail!("Hex disconnected");
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}

fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    read_frame_within(stream, IO_TIMEOUT)
}

fn read_frame_within<T: DeserializeOwned>(stream: &mut UnixStream, wait: Duration) -> Result<T> {
    let deadline = Instant::now() + wait;
    let mut header = [0; 4];
    read_exact(stream, &mut header, deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        bail!("Invalid Hex frame size");
    }
    let mut body = vec![0; length];
    read_exact(stream, &mut body, deadline)?;
    Ok(serde_json::from_slice(&body)?)
}

fn write_frame<T: Serialize>(stream: &mut UnixStream, message: &T) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    if body.len() > MAX_FRAME {
        bail!("Hex message exceeds the size limit");
    }
    let deadline = Instant::now() + IO_TIMEOUT;
    write_all(stream, &(body.len() as u32).to_be_bytes(), deadline)?;
    write_all(stream, &body, deadline)
}

fn write_all(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| eyre!("Hex write timed out"))?;
        stream.set_write_timeout(Some(remaining))?;
        let count = stream.write(bytes)?;
        if count == 0 {
            bail!("Hex disconnected");
        }
        bytes = &bytes[count..];
    }
    Ok(())
}

/// Why a request did not reach a current instance.
#[derive(Debug)]
pub(crate) enum RequestError {
    /// Nothing listens on the socket.
    NotRunning,
    /// An older Hex (the service/Settings protocol) owns the socket.
    Outdated,
    Failed(color_eyre::Report),
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => write!(f, "Hex is not running; run `hex start`"),
            Self::Outdated => write!(
                f,
                "An older Hex is running; quit it or run `hex restart`, then try again"
            ),
            Self::Failed(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for RequestError {}

fn connect() -> std::result::Result<UnixStream, RequestError> {
    let path = socket_path().map_err(RequestError::Failed)?;
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return Err(RequestError::NotRunning);
    };
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(RequestError::Failed(eyre!(
            "Refusing an insecure Hex socket"
        )));
    }
    // A crashed instance leaves its socket behind.
    let stream = UnixStream::connect(path).map_err(|_| RequestError::NotRunning)?;
    check_peer(&stream).map_err(RequestError::Failed)?;
    Ok(stream)
}

/// `hex capture`'s recording commands, handed to the running listener.
pub(crate) fn capture(action: crate::control::ScrawlCaptureAction) -> crate::control::ControlReply {
    use crate::control::{ControlReply, ScrawlCaptureAction};
    if action == ScrawlCaptureAction::Status {
        return match exchange(Request::CaptureStatus, SEND_WAIT) {
            Ok(reply) => reply
                .control
                .unwrap_or_else(|| ControlReply::error("failed", "Hex did not say how it went")),
            Err(RequestError::NotRunning) => crate::control::cli::not_running(),
            Err(error) => ControlReply::error("failed", format!("{error}")),
        };
    }
    match request(Request::Capture(action)) {
        Ok(_) => ControlReply::done("Sent to Hex"),
        Err(error) => ControlReply::error("failed", format!("{error}")),
    }
}

/// `hex annotate send`, carried out by the running Hex's router.
pub(crate) fn capture_send(
    capture: String,
    to: Option<String>,
    always: bool,
) -> crate::control::ControlReply {
    use crate::control::ControlReply;
    let sent = exchange(
        Request::CaptureSend {
            capture,
            to,
            always,
        },
        SEND_WAIT,
    );
    match sent {
        Ok(reply) => reply
            .control
            .unwrap_or_else(|| ControlReply::error("failed", "Hex did not say how it went")),
        Err(error) => ControlReply::error("failed", format!("{error}")),
    }
}

/// `hex annotate start --area`: the running Hex records that area.
pub(crate) fn capture_area(rect: [f64; 4]) -> crate::control::ControlReply {
    use crate::control::ControlReply;
    match request(Request::CaptureArea { rect }) {
        Ok(_) => ControlReply::done("Sent to Hex"),
        Err(error) => ControlReply::error("failed", format!("{error}")),
    }
}

/// `hex annotate draw`: drawing on or off in the running Hex.
pub(crate) fn capture_draw() -> crate::control::ControlReply {
    use crate::control::ControlReply;
    match request(Request::CaptureDraw) {
        Ok(_) => ControlReply::done("Drawing on or off; it starts a recording when none records"),
        Err(error) => ControlReply::error("failed", format!("{error}")),
    }
}

/// `hex annotate undo`: the running Hex takes its newest send back.
pub(crate) fn capture_undo() -> crate::control::ControlReply {
    use crate::control::ControlReply;
    match request(Request::CaptureUndo) {
        Ok(_) => ControlReply::done("Asked Hex to take the newest send back"),
        Err(error) => ControlReply::error("failed", format!("{error}")),
    }
}

/// Before `hex restart`: the unit restarts into this desktop's session
/// (without a desktop, over SSH, the one Hex last ran in).
pub(crate) fn prepare_restart() -> Result<()> {
    if supervised() && (graphical_session() || !session_environment_path()?.exists()) {
        write_session_environment()?;
    }
    Ok(())
}

/// `hex annotate status`, asked of the listener's capture host.
pub(crate) fn capture_status() -> crate::control::ControlReply {
    use crate::control::ControlReply;
    let (sender, receiver) = mpsc::sync_channel(1);
    if let Err(error) =
        crate::capture_host::send_to_listener(crate::capture_host::CaptureControl::Reply(
            crate::control::ScrawlCaptureAction::Status,
            sender,
        ))
    {
        return ControlReply::error("not-ready", format!("{error:#}"));
    }
    receiver
        .recv_timeout(SEND_WAIT / 2)
        .unwrap_or_else(|_| ControlReply::error("unavailable", "the listener didn't answer"))
}

/// Sends one request to the running instance. Never starts one.
pub(crate) fn request(request: Request) -> std::result::Result<Status, RequestError> {
    let reply = exchange(request, IO_TIMEOUT)?;
    reply
        .status
        .ok_or_else(|| RequestError::Failed(eyre!("Invalid Hex reply")))
}

/// One request and its reply, which may take up to `wait`.
fn exchange(request: Request, wait: Duration) -> std::result::Result<Reply, RequestError> {
    let mut stream = connect()?;
    write_frame(
        &mut stream,
        &Message {
            version: VERSION,
            request,
        },
    )
    .map_err(RequestError::Failed)?;
    let reply: Reply = read_frame_within(&mut stream, wait).map_err(RequestError::Failed)?;
    if reply.version != VERSION {
        return Err(RequestError::Outdated);
    }
    if let Some(error) = &reply.error {
        return Err(RequestError::Failed(eyre!(error.clone())));
    }
    Ok(reply)
}

pub(crate) struct Server {
    path: PathBuf,
    inode: u64,
    listener: Option<UnixListener>,
    acceptor: Option<JoinHandle<()>>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    _lock: crate::desktop::instance::InstanceLock,
}

impl Server {
    /// Claims the data root. Fails when another instance holds it.
    pub(crate) fn bind() -> Result<Self> {
        if unsafe { libc::geteuid() } == 0 {
            bail!("Run Hex as a desktop user, never root");
        }
        let path = socket_path()?;
        private_directory(path.parent().unwrap())?;
        let lock = crate::desktop::instance::acquire("service")?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                bail!("Refusing to replace an unmanaged Hex socket path");
            }
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let inode = fs::metadata(&path)?.ino();
        Ok(Self {
            path,
            inode,
            listener: Some(listener),
            acceptor: None,
            workers: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            _lock: lock,
        })
    }

    /// Starts answering once the host has published its first status.
    pub(crate) fn serve(&mut self, published: Arc<Published>, control: SyncSender<Control>) {
        let Some(listener) = self.listener.take() else {
            return;
        };
        let (connections, pending) = mpsc::sync_channel::<UnixStream>(CLIENTS);
        let pending = Arc::new(Mutex::new(pending));
        let session = Session::current();
        self.workers = (0..CLIENTS)
            .map(|_| {
                let pending = pending.clone();
                let published = published.clone();
                let control = control.clone();
                let stop = self.stop.clone();
                let session = session.clone();
                thread::spawn(move || {
                    loop {
                        let Ok(stream) = pending.lock().unwrap().recv() else {
                            break;
                        };
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        serve_client(stream, &published, &control, &session, &stop);
                    }
                })
            })
            .collect();
        let stop = self.stop.clone();
        self.acceptor = Some(thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                // A full pool drops the connection; its client fails fast.
                if let Ok(stream) = stream {
                    let _ = connections.try_send(stream);
                }
            }
        }));
    }
}

fn serve_client(
    mut stream: UnixStream,
    published: &Published,
    control: &SyncSender<Control>,
    session: &Session,
    stop: &AtomicBool,
) {
    if check_peer(&stream).is_err() {
        return;
    }
    while !stop.load(Ordering::Acquire) {
        // The version is read first: older clients send requests this
        // protocol no longer has.
        let Ok(frame) = read_frame::<serde_json::Value>(&mut stream) else {
            break;
        };
        let message = (frame.get("version").and_then(serde_json::Value::as_u64)
            == Some(VERSION.into()))
        .then(|| serde_json::from_value::<Message>(frame).ok())
        .flatten();
        let Some(message) = message else {
            let _ = write_frame(
                &mut stream,
                &Reply {
                    version: VERSION,
                    status: None,
                    error: Some("Unsupported Hex request; restart Hex".into()),
                    control: None,
                },
            );
            break;
        };
        let mut answer = None;
        let result = match message.request {
            Request::Activate {
                activation,
                show,
                session: caller,
            } => {
                // A caller from another desktop session decides from the reply.
                if show && caller == *session {
                    control
                        .try_send(Control::Activate(activation))
                        .map_err(|_| eyre!("Hex is busy; try again"))
                } else {
                    Ok(())
                }
            }
            Request::Capture(action) => crate::capture_host::send_to_listener(
                crate::capture_host::CaptureControl::Scrawl(action),
            ),
            Request::CaptureSend {
                capture,
                to,
                always,
            } => {
                answer = Some(
                    crate::capture_host::send_capture(&capture, to, always)
                        .unwrap_or_else(crate::control::ControlReply::from),
                );
                Ok(())
            }
            Request::CaptureDraw => {
                crate::capture_host::send_to_listener(crate::capture_host::CaptureControl::Draw)
            }
            Request::CaptureArea { rect } => {
                crate::capture_host::send_to_listener(crate::capture_host::CaptureControl::Command(
                    crate::capture_host::CaptureCommand::Start(
                        crate::capture_host::CaptureTarget::Area { rect },
                    ),
                ))
            }
            Request::CaptureUndo => {
                crate::capture_host::send_to_listener(crate::capture_host::CaptureControl::Command(
                    crate::capture_host::CaptureCommand::Undo,
                ))
            }
            Request::CaptureStatus => {
                answer = Some(capture_status());
                Ok(())
            }
            Request::Status => Ok(()),
        };
        let status = published.status.lock().unwrap().clone();
        let error = match (&result, &status) {
            (Err(error), _) => Some(format!("{error:#}")),
            (Ok(()), None) => Some("Hex is starting; try again".into()),
            (Ok(()), Some(_)) => None,
        };
        if write_frame(
            &mut stream,
            &Reply {
                version: VERSION,
                status,
                error,
                control: answer,
            },
        )
        .is_err()
        {
            break;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(acceptor) = self.acceptor.take() {
            // Wakes the blocking accept.
            let _ = UnixStream::connect(&self.path);
            let _ = acceptor.join();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Tells systemd (`Type=notify`) the socket answers. Without
/// `NOTIFY_SOCKET` (no unit, or an older `Type=simple` unit) it does nothing.
pub(crate) fn notify(state: &str) {
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    let result = (|| -> std::io::Result<()> {
        let socket = UnixDatagram::unbound()?;
        let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
        if let Some(name) = bytes.strip_prefix(b"@") {
            use std::os::linux::net::SocketAddrExt;
            let address = std::os::unix::net::SocketAddr::from_abstract_name(name)?;
            socket.send_to_addr(state.as_bytes(), &address)?;
        } else {
            socket.send_to(state.as_bytes(), &path)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!(%error, state, "could not notify systemd");
    }
}

fn unit() -> String {
    std::env::var("HEX_SYSTEMD_UNIT")
        .ok()
        .filter(|unit| !unit.is_empty())
        .unwrap_or_else(|| UNIT.into())
}

/// Whether launches start Hex through its systemd user unit: the unit is
/// installed (by the installer or Home Manager) and this process uses the
/// default data root. `HEX_SYSTEMD_UNIT` names another unit, for isolated
/// tests with a custom data root.
pub(crate) fn supervised() -> bool {
    if custom_root() {
        return false;
    }
    Command::new("systemctl")
        .args(["--user", "show", &unit(), "--property=LoadState", "--value"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "loaded"
        })
}

/// A custom data root without its own unit (isolated tests) never touches
/// the user's systemd manager.
fn custom_root() -> bool {
    std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some()
        && std::env::var_os("HEX_SYSTEMD_UNIT").is_none()
}

fn systemctl(arguments: &[&str]) -> Result<()> {
    let unit = unit();
    let mut child = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .arg(&unit)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("Could not run systemctl")?;
    // `start` waits for READY=1, which follows model-free startup.
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!(
                    "systemctl --user {} {unit} failed; inspect `systemctl --user status {unit}`",
                    arguments.join(" ")
                );
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Timed out controlling {unit}");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// Waits until the started unit answers.
fn wait_ready() -> Result<Status> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match request(Request::Status) {
            Ok(status) => return Ok(status),
            Err(RequestError::Outdated) => return Err(RequestError::Outdated.into()),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                return Err(eyre!(
                    "Hex did not become ready ({error}); inspect `journalctl --user -u {}`",
                    unit()
                ));
            }
        }
    }
}

/// What a launch found.
pub(crate) enum Launch {
    /// The running instance took the request.
    Forwarded,
    /// Nothing runs and no unit supervises Hex: this process becomes it.
    BecomeInstance,
}

/// After losing the data root to a concurrent launch, waits for that
/// instance to answer.
pub(crate) fn another_instance_answers() -> bool {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if request(Request::Status).is_ok() {
            return true;
        }
        if crate::desktop::instance::acquire("service").is_ok() {
            // Nobody holds the root: the failure was this process's own.
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// `hex`, `hex app`, and `hex start`: forward to the running instance, start
/// the systemd unit, or tell the caller to become the instance.
pub(crate) fn launch(activation: &Activation, show: bool) -> Result<Launch> {
    let activate = || Request::Activate {
        activation: activation.clone(),
        show,
        session: Session::current(),
    };
    match request(activate()) {
        Ok(status) if status.session == Session::current() => return Ok(Launch::Forwarded),
        Ok(_) if supervised() => {
            // Hand the instance over to this desktop session.
            restart()?;
        }
        Ok(_) => {
            bail!("Hex is running in another desktop session; run `hex quit`, then launch it again")
        }
        Err(RequestError::Outdated) if supervised() => restart()?,
        Err(RequestError::NotRunning) if supervised() => {
            write_session_environment()?;
            systemctl(&["start"])?;
        }
        Err(RequestError::NotRunning) => return Ok(Launch::BecomeInstance),
        Err(error) => return Err(error.into()),
    }
    let status = wait_ready()?;
    if status.session != Session::current() {
        bail!(
            "{} did not load this desktop's environment; check its EnvironmentFile",
            unit()
        );
    }
    request(activate())?;
    Ok(Launch::Forwarded)
}

pub(crate) fn restart() -> Result<()> {
    if !supervised() {
        bail!("Only Hex started by its systemd unit restarts; run `hex quit`, then launch it");
    }
    // Without a desktop (over SSH), keep the one Hex last ran in.
    if graphical_session() || !session_environment_path()?.exists() {
        write_session_environment()?;
    }
    systemctl(&["restart"])
}

pub(crate) fn schedule_restart() -> Result<()> {
    systemctl(&["--no-block", "restart"])
}

/// The display the process connects to. Absent values are written empty
/// so they override a stale user-manager environment from an earlier
/// desktop.
const SESSION_KEYS: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    // The window's cursor theme and input method.
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
    "XMODIFIERS",
    "GTK_IM_MODULE",
    "QT_IM_MODULE",
];

/// Written only when set: clearing them would break the user manager's own
/// locale and data directories.
const INHERITED_KEYS: &[&str] = &[
    "XDG_DATA_DIRS",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
];

fn environment_line(key: &str, value: &str) -> Result<String> {
    if value.contains(['\n', '\r', '\0']) {
        bail!("Invalid desktop environment value for {key}");
    }
    Ok(format!(
        "{key}=\"{}\"\n",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn session_environment(get: impl Fn(&str) -> Option<String>) -> Result<String> {
    let mut body = String::new();
    for key in SESSION_KEYS {
        body.push_str(&environment_line(key, &get(key).unwrap_or_default())?);
    }
    for key in INHERITED_KEYS {
        if let Some(value) = get(key).filter(|value| !value.is_empty()) {
            body.push_str(&environment_line(key, &value)?);
        }
    }
    Ok(body)
}

fn graphical_session() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|key| !std::env::var(key).unwrap_or_default().is_empty())
}

fn session_environment_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("session.env"))
}

fn write_session_environment() -> Result<()> {
    if !graphical_session() {
        bail!("Start Hex from your graphical login session");
    }
    let directory = crate::app_paths::support_dir()?;
    private_directory(&directory)?;
    let body = session_environment(|key| std::env::var(key).ok())?;
    let temporary = directory.join(format!(".session-{}.env", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, session_environment_path()?)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_reject_oversized_input() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        check_peer(&client).unwrap();
        write_frame(
            &mut client,
            &Message {
                version: VERSION,
                request: Request::Activate {
                    activation: Activation {
                        token: Some("token".into()),
                        startup_id: None,
                    },
                    show: true,
                    session: Session::current(),
                },
            },
        )
        .unwrap();
        let message: Message = read_frame(&mut server).unwrap();
        assert_eq!(message.version, VERSION);
        assert!(matches!(
            message.request,
            Request::Activate { activation, show: true, .. } if activation.token.as_deref() == Some("token")
        ));
        client
            .write_all(&((MAX_FRAME + 1) as u32).to_be_bytes())
            .unwrap();
        assert!(read_frame::<Message>(&mut server).is_err());
    }

    #[test]
    fn incomplete_frames_and_disconnections_are_bounded() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client.write_all(&100_u32.to_be_bytes()).unwrap();
        drop(client);
        assert!(read_frame::<Message>(&mut server).is_err());
        let (_client, mut server) = UnixStream::pair().unwrap();
        let started = Instant::now();
        assert!(
            read_exact(
                &mut server,
                &mut [0; 4],
                started + Duration::from_millis(25)
            )
            .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn version_one_replies_parse_as_outdated() {
        let reply: Reply = serde_json::from_str(
            r#"{"version":1,"state":null,"error":"Hex service version mismatch; restart the service"}"#,
        )
        .unwrap();
        assert_eq!(reply.version, 1);
        assert!(reply.status.is_none());
    }

    #[test]
    fn commands_reach_only_a_listening_route() {
        let error = route_shortcut(None, ShortcutCommand::DictationToggle).unwrap_err();
        assert!(error.to_string().contains("not listening"), "{error}");
        let (commands, received) = mpsc::sync_channel(2);
        let route = ShortcutRoute { commands };
        route_shortcut(Some(&route), ShortcutCommand::DictationStart).unwrap();
        route_shortcut(Some(&route), ShortcutCommand::DictationToggle).unwrap();
        let error = route_shortcut(Some(&route), ShortcutCommand::PasteLast).unwrap_err();
        assert!(error.to_string().contains("busy"), "{error}");
        assert_eq!(
            received.try_iter().collect::<Vec<_>>(),
            [
                ListenerCommand::from(ShortcutCommand::DictationStart),
                ListenerCommand::from(ShortcutCommand::DictationToggle)
            ]
        );
    }

    #[test]
    fn session_environment_clears_display_keys_and_keeps_locale_unset() {
        let body = session_environment(|key| match key {
            "DISPLAY" => Some(":0".into()),
            "XCURSOR_THEME" => Some("Adwaita".into()),
            "LANG" => Some("de_DE.UTF-8".into()),
            _ => None,
        })
        .unwrap();
        assert!(body.contains("DISPLAY=\":0\"\n"));
        assert!(body.contains("WAYLAND_DISPLAY=\"\"\n"));
        assert!(body.contains("XCURSOR_THEME=\"Adwaita\"\n"));
        assert!(body.contains("LANG=\"de_DE.UTF-8\"\n"));
        assert!(!body.contains("XDG_DATA_DIRS"));
        assert!(!body.contains("LC_ALL"));
        assert!(environment_line("DISPLAY", ":0\nEVIL=yes").is_err());
    }

    #[test]
    fn manager_environment_reads_one_exact_key() {
        let environment =
            "XAUTHORITY_OLD=/a\nXAUTHORITY=/run/user/1000/.mutter-Xwaylandauth.X1\nDISPLAY=:0\n";
        assert_eq!(
            environment_value(environment, "XAUTHORITY").as_deref(),
            Some("/run/user/1000/.mutter-Xwaylandauth.X1")
        );
        assert_eq!(environment_value("XAUTHORITY=\n", "XAUTHORITY"), None);
    }
}
