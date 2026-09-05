//! Per-user dictation service transport and lifecycle. No UI or audio work runs here.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::desktop_host::{DesktopAction, DesktopSnapshot};
use crate::linux_settings::LinuxSettings;
use crate::transcription_models::TranscriptionModelId;

const VERSION: u32 = 1;
const MAX_FRAME: usize = 128 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENTS: usize = 8;
pub const UNIT: &str = "hex.service";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Snapshot,
    Desktop(DesktopAction),
    CaptureShortcut,
    CancelShortcut,
    SetTerminalPaste(bool),
    SetVolume(f32),
    ChooseModel {
        model: TranscriptionModelId,
        language: String,
    },
    CancelModel,
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub desktop: DesktopSnapshot,
    pub settings: LinuxSettings,
    pub editing: bool,
    pub capturing: bool,
    pub hud_limitation: Option<String>,
    pub pid: u32,
    pub session: Session,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Session {
    backend: String,
    display: String,
    runtime: String,
}

impl Session {
    pub fn current() -> Self {
        let session = crate::linux_session::LinuxSession::detect();
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

#[derive(Serialize, Deserialize)]
struct Message {
    version: u32,
    request: Request,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    version: u32,
    state: Option<Snapshot>,
    error: Option<String>,
}

pub enum Event {
    Request {
        client: u64,
        request: Request,
        reply: SyncSender<(Snapshot, Option<String>)>,
    },
    Disconnected(u64),
}

fn socket_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("service.sock"))
}

fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        bail!("HEX requires a user-owned, non-symlink service directory");
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
        bail!("HEX only accepts local IPC from the same user");
    }
    Ok(())
}

fn read_exact(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| eyre!("HEX service read timed out"))?;
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(bytes)?;
        if count == 0 {
            bail!("HEX service disconnected");
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}

fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut header = [0; 4];
    read_exact(stream, &mut header, deadline)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        bail!("Invalid HEX service frame size");
    }
    let mut body = vec![0; length];
    read_exact(stream, &mut body, deadline)?;
    Ok(serde_json::from_slice(&body)?)
}

fn write_frame<T: Serialize>(stream: &mut UnixStream, message: &T) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    if body.len() > MAX_FRAME {
        bail!("HEX service message exceeds the size limit");
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend(body);
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut bytes = frame.as_slice();
    while !bytes.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| eyre!("HEX service write timed out"))?;
        stream.set_write_timeout(Some(remaining))?;
        let count = stream.write(bytes)?;
        if count == 0 {
            bail!("HEX service disconnected");
        }
        bytes = &bytes[count..];
    }
    Ok(())
}

fn connect() -> Result<UnixStream> {
    let path = socket_path()?;
    let metadata =
        fs::symlink_metadata(&path).wrap_err("HEX service is not running; run `hex start`")?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        bail!("Refusing an insecure HEX service socket");
    }
    let stream = UnixStream::connect(path)?;
    check_peer(&stream)?;
    Ok(stream)
}

fn exchange(stream: &mut UnixStream, request: Request) -> Result<(Snapshot, Option<String>)> {
    write_frame(
        stream,
        &Message {
            version: VERSION,
            request,
        },
    )?;
    let reply: Reply = read_frame(stream)?;
    if reply.version != VERSION {
        bail!("HEX service version mismatch; restart the service");
    }
    let state = reply.state.ok_or_else(|| {
        eyre!(
            "{}",
            reply.error.as_deref().unwrap_or("Invalid service reply")
        )
    })?;
    Ok((state, reply.error))
}

pub fn request(request: Request) -> Result<Snapshot> {
    let (state, error) = exchange(&mut connect()?, request)?;
    if let Some(error) = error {
        bail!(error);
    }
    Ok(state)
}

pub struct Server {
    path: PathBuf,
    inode: u64,
    listener: UnixListener,
    connections: Option<SyncSender<(u64, UnixStream)>>,
    next_client: u64,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pub events: Receiver<Event>,
    _lock: crate::instance::InstanceLock,
}

impl Server {
    pub fn bind() -> Result<Self> {
        if unsafe { libc::geteuid() } == 0 {
            bail!("Run HEX as a desktop user, never root");
        }
        let path = socket_path()?;
        private_directory(path.parent().unwrap())?;
        let lock = crate::instance::acquire("service")?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
                bail!("Refusing to replace an unmanaged HEX service path");
            }
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let inode = fs::metadata(&path)?.ino();
        let (connections, pending) = mpsc::sync_channel::<(u64, UnixStream)>(CLIENTS);
        let pending = Arc::new(Mutex::new(pending));
        let (sender, events) = mpsc::sync_channel(32);
        let stop = Arc::new(AtomicBool::new(false));
        let workers = (0..CLIENTS)
            .map(|_| {
                let pending = pending.clone();
                let sender = sender.clone();
                let stop = stop.clone();
                thread::spawn(move || {
                    loop {
                        let Ok((client, mut stream)) = pending.lock().unwrap().recv() else {
                            break;
                        };
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        if check_peer(&stream).is_err() {
                            continue;
                        }
                        while !stop.load(Ordering::Acquire) {
                            let Ok(message) = read_frame::<Message>(&mut stream) else {
                                break;
                            };
                            if message.version != VERSION {
                                let _ = write_frame(
                                    &mut stream,
                                    &Reply {
                                        version: VERSION,
                                        state: None,
                                        error: Some(
                                            "HEX service version mismatch; restart the service"
                                                .into(),
                                        ),
                                    },
                                );
                                break;
                            }
                            let (reply, receiver) = mpsc::sync_channel(1);
                            if sender
                                .try_send(Event::Request {
                                    client,
                                    request: message.request,
                                    reply,
                                })
                                .is_err()
                            {
                                break;
                            }
                            let Ok((state, error)) = receiver.recv_timeout(IO_TIMEOUT) else {
                                break;
                            };
                            if write_frame(
                                &mut stream,
                                &Reply {
                                    version: VERSION,
                                    state: Some(state),
                                    error,
                                },
                            )
                            .is_err()
                            {
                                break;
                            }
                        }
                        // A full queue is safe: the runtime also bounds shortcut capture by time.
                        let _ = sender.try_send(Event::Disconnected(client));
                    }
                })
            })
            .collect();
        Ok(Self {
            path,
            inode,
            listener,
            connections: Some(connections),
            next_client: 0,
            workers,
            stop,
            events,
            _lock: lock,
        })
    }

    pub fn accept(&mut self) -> Result<()> {
        for _ in 0..CLIENTS {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    self.next_client += 1;
                    let _ = self
                        .connections
                        .as_ref()
                        .unwrap()
                        .try_send((self.next_client, stream));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.connections.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub struct Client {
    pub state: Option<Snapshot>,
    pub error: Option<String>,
    sender: SyncSender<Request>,
    updates: Receiver<Result<(Snapshot, Option<String>), String>>,
    stop: Arc<AtomicBool>,
}

impl Client {
    pub fn new() -> Self {
        let (sender, requests) = mpsc::sync_channel(16);
        let (updates_sender, updates) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        thread::spawn(move || {
            let startup = ensure_started();
            if let Err(error) = startup {
                let _ = updates_sender.try_send(Err(format!("{error:#}")));
            }
            let mut stream = None;
            while !stopping.load(Ordering::Acquire) {
                let request = match requests.recv_timeout(Duration::from_millis(250)) {
                    Ok(request) => request,
                    Err(mpsc::RecvTimeoutError::Timeout) => Request::Snapshot,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let result = (|| {
                    if stream.is_none() {
                        if matches!(request, Request::Desktop(DesktopAction::StartListening)) {
                            ensure_started()?;
                        }
                        stream = Some(connect()?);
                    }
                    let result = exchange(stream.as_mut().unwrap(), request)?;
                    if result.0.session != Session::current() {
                        bail!(
                            "HEX belongs to a different desktop session; run `hex start` to hand it over"
                        );
                    }
                    Ok(result)
                })();
                if result.is_err() {
                    stream = None;
                }
                let _ = updates_sender.try_send(result.map_err(|e| format!("{e:#}")));
            }
        });
        Self {
            state: None,
            error: None,
            sender,
            updates,
            stop,
        }
    }

    pub fn send(&mut self, request: Request) -> Result<()> {
        self.sender
            .try_send(request)
            .map_err(|_| eyre!("HEX service command queue is busy"))?;
        Ok(())
    }

    pub fn refresh(&mut self) {
        while let Ok(update) = self.updates.try_recv() {
            match update {
                Ok((state, error)) => {
                    self.state = Some(state);
                    self.error = error;
                }
                Err(error) => {
                    self.state = None;
                    self.error = Some(error);
                }
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn systemctl(arguments: &[&str]) -> Result<()> {
    let mut child = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("Could not run systemctl; run `hex service` in a graphical session")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!(
                    "systemctl --user {} failed; inspect `systemctl --user status hex.service`",
                    arguments.join(" ")
                );
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Timed out controlling hex.service");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn ensure_started() -> Result<()> {
    if let Ok(state) = request(Request::Snapshot) {
        if state.session != Session::current() {
            bail!(
                "HEX belongs to a different desktop session; run `hex start` or `hex restart` to hand it over"
            );
        }
        return Ok(());
    }
    if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some() {
        bail!(
            "For a custom data directory, start `hex service` explicitly before opening Settings"
        );
    }
    write_session_environment()?;
    systemctl(&["start", UNIT])?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(state) = request(Request::Snapshot) {
            if state.session != Session::current() {
                bail!(
                    "hex.service did not load the current desktop environment; check its EnvironmentFile configuration"
                );
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("HEX service did not become ready; inspect `journalctl --user -u hex.service`");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn start() -> Result<()> {
    if request(Request::Snapshot).is_ok_and(|state| state.session != Session::current()) {
        restart()?;
    }
    ensure_started()?;
    request(Request::Desktop(DesktopAction::StartListening))?;
    Ok(())
}

pub fn stop() -> Result<()> {
    if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some() {
        request(Request::Shutdown)?;
        return Ok(());
    }
    // Also works for a foreground development service with no systemd unit.
    if let Err(error) = systemctl(&["stop", UNIT])
        && request(Request::Shutdown).is_err()
    {
        return Err(error);
    }
    Ok(())
}

pub fn restart() -> Result<()> {
    if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some() {
        bail!("Restart a custom-data service through its foreground process or supervisor");
    }
    write_session_environment()?;
    systemctl(&["restart", UNIT])
}

pub fn schedule_restart() -> Result<()> {
    systemctl(&["--no-block", "restart", UNIT])
}

const SESSION_KEYS: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
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

fn write_session_environment() -> Result<()> {
    if std::env::var("DISPLAY").unwrap_or_default().is_empty()
        && std::env::var("WAYLAND_DISPLAY")
            .unwrap_or_default()
            .is_empty()
    {
        bail!("Start HEX from your graphical login session");
    }
    let directory = crate::app_paths::support_dir()?;
    private_directory(&directory)?;
    let mut body = String::new();
    for key in SESSION_KEYS {
        // Empty values override stale user-manager environment from an earlier desktop.
        body.push_str(&environment_line(
            key,
            &std::env::var(key).unwrap_or_default(),
        )?);
    }
    let temporary = directory.join(format!(".session-{}.env", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, directory.join("session.env"))?;
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
                request: Request::SetVolume(0.25),
            },
        )
        .unwrap();
        let message: Message = read_frame(&mut server).unwrap();
        assert_eq!(message.version, VERSION);
        assert!(matches!(message.request, Request::SetVolume(0.25)));
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
    fn session_environment_clears_absent_values_and_rejects_injection() {
        assert_eq!(
            environment_line("WAYLAND_DISPLAY", "").unwrap(),
            "WAYLAND_DISPLAY=\"\"\n"
        );
        assert_eq!(
            environment_line("DISPLAY", ":0").unwrap(),
            "DISPLAY=\":0\"\n"
        );
        assert!(environment_line("DISPLAY", ":0\nEVIL=yes").is_err());
    }
}
