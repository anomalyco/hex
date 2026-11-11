use std::ffi::OsStr;
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11_clipboard::Clipboard;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use crate::linux::clipboard::{Selection, Snapshot};
use crate::linux::input::{
    KEY_PRESS, KEY_RELEASE, Keymap, ShortcutKeys, XK_CONTROL_L, XK_SHIFT_L, connect_x11,
};
use crate::linux::remote_desktop::RemoteDesktop;
use crate::linux::session::LinuxSession;

const XK_A: u32 = 0x61;
const XK_C: u32 = 0x63;
const XK_V: u32 = 0x76;
const HELPER_TIMEOUT: Duration = Duration::from_secs(3);
const PASTE_SETTLE: Duration = Duration::from_millis(100);
const PORTAL_FOCUS_SETTLE: Duration = Duration::from_millis(250);
/// How long an existing portal session waits after setting the clipboard
/// before its paste shortcut, so the target has the new selection (#578).
const PORTAL_CLIPBOARD_SETTLE: Duration = Duration::from_millis(150);
/// How long a RemoteDesktop session stays open after a paste. GNOME shows its
/// remote-control indicator while one is open (#718), and a restored grant
/// reopens it at the next paste without a dialog.
const PORTAL_IDLE: Duration = Duration::from_secs(15);
/// How long after the paste shortcut the previous clipboard comes back, as
/// on macOS. Nothing acknowledges that the target read the paste.
const RESTORE_DELAY: Duration = Duration::from_millis(500);
/// How long an X11 paste waits for modifiers no Hex shortcut holds.
const MODIFIER_WAIT_LIMIT: Duration = Duration::from_secs(2);

pub(crate) struct LinuxPaster {
    backend: Backend,
    stop: Arc<AtomicBool>,
    /// The terminal-paste preference, which the window changes live.
    paste_with_shift: Arc<AtomicBool>,
    /// When the shortcut that asked for the paste has let go of its keys.
    keys: ShortcutKeys,
    selection: Selection,
    restore: Arc<Mutex<Restore>>,
}

/// The clipboard a paste replaced, waiting to come back.
#[derive(Default)]
struct Restore {
    /// Advances with every clipboard write, so only the latest paste restores.
    generation: u64,
    original: Option<Snapshot>,
    /// What the latest paste put on the clipboard, until it restores.
    inserted: Option<(Mime, Vec<u8>)>,
}

impl Restore {
    /// Starts a job that shares the clipboard: cancels the pending restore,
    /// and hands back the clipboard that paste replaced when the clipboard
    /// still holds that paste (`pending_holds`), as a second paste would keep
    /// it. The job owns the original from here; a changed clipboard is read
    /// afresh instead.
    fn begin(&mut self, pending_holds: bool) -> Option<Snapshot> {
        self.generation = self.generation.wrapping_add(1);
        self.inserted = None;
        if pending_holds {
            self.original.take()
        } else {
            self.original = None;
            None
        }
    }

    /// A failed paste leaves the clipboard as it is. An ordinary paste keeps
    /// its transcript there for Ctrl+V, and a typed-text job restores the
    /// original it saved itself.
    fn failed(&mut self) {
        self.inserted = None;
        self.original = None;
    }

    /// Labs › Keep dictation on clipboard: an earlier paste's pending restore
    /// stands down and nothing is kept to put back, so a later restoring
    /// paste never brings an older clipboard back over this one.
    fn keep(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.original = None;
        self.inserted = None;
    }
}

enum Backend {
    X11(Box<X11Paster>),
    Wayland(WaylandKeys),
}

/// How Wayland sends the paste shortcut, chosen at the first paste.
enum WaylandKeys {
    Unchosen,
    /// `wtype`, when the compositor offers the virtual keyboard protocol.
    /// Its virtual keyboard sends its own modifiers, but some compositors
    /// (Hyprland) add every held physical modifier to them, so a still-held
    /// Alt turns its Ctrl+V into Ctrl+Alt+V. [`ShortcutKeys::settle`] waits
    /// for the shortcut to let go first.
    Wtype,
    /// The desktop's RemoteDesktop portal everywhere else (KWin, Mutter).
    Portal(Arc<Mutex<PortalSession>>),
}

/// The RemoteDesktop session: opened at a paste and, when its grant
/// restores without a dialog, closed [`PORTAL_IDLE`] after the last one.
/// A grant that cannot restore keeps its session for the paster's lifetime,
/// so pastes never ask again.
struct PortalSession {
    session: Option<Box<RemoteDesktop>>,
    last_paste: Instant,
}

/// Closes the session once it has been idle for [`PORTAL_IDLE`]; ends when the
/// session closes or the paster goes away.
fn close_when_idle(shared: Weak<Mutex<PortalSession>>) {
    loop {
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let wait = {
            let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
            if state.session.is_none() {
                return;
            }
            let idle = state.last_paste.elapsed();
            if idle >= PORTAL_IDLE {
                state.session = None;
                tracing::debug!("closed the idle RemoteDesktop session");
                return;
            }
            PORTAL_IDLE - idle
        };
        drop(shared);
        thread::sleep(wait);
    }
}

/// A clipboard chord a paste or typed-text job sends to the focused app.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Chord {
    SelectAll,
    Copy,
    Paste,
}

impl WaylandKeys {
    fn choose() -> Self {
        if wtype_pastes() {
            Self::Wtype
        } else {
            Self::Portal(Arc::new(Mutex::new(PortalSession {
                session: None,
                last_paste: Instant::now(),
            })))
        }
    }

    fn send(&mut self, stop: &AtomicBool, chord: Chord, paste_with_shift: bool) -> Result<()> {
        if matches!(self, Self::Unchosen) {
            *self = Self::choose();
        }
        match self {
            Self::Unchosen => unreachable!("chosen above"),
            Self::Wtype => {
                let mut keys = Command::new("wtype");
                keys.args(chord_keys(chord, paste_with_shift));
                run_helper(keys, &[], stop, HELPER_TIMEOUT)
                    .wrap_err("wtype could not send the clipboard shortcut")
            }
            Self::Portal(shared) => {
                let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                if state
                    .session
                    .as_mut()
                    .is_some_and(|session| session.closed())
                {
                    state.session = None;
                }
                // Without data-control, Mutter hands the new selection to the
                // focused app only after wl-copy returns; a Ctrl+V sent at once
                // pastes nothing or the previous clipboard (#578).
                let mut settle = if chord == Chord::Paste {
                    PORTAL_CLIPBOARD_SETTLE
                } else {
                    Duration::ZERO
                };
                if state.session.is_none() {
                    let began = Instant::now();
                    let started = RemoteDesktop::start(stop)?;
                    settle = if started.prompted() {
                        // The approval dialog hands focus back to the target.
                        PORTAL_FOCUS_SETTLE
                    } else if chord == Chord::Paste {
                        // Reopening already took part of the wait.
                        PORTAL_CLIPBOARD_SETTLE.saturating_sub(began.elapsed())
                    } else {
                        Duration::ZERO
                    };
                    if started.restorable() {
                        let idle = Arc::downgrade(shared);
                        thread::spawn(move || close_when_idle(idle));
                    }
                    state.session = Some(Box::new(started));
                }
                thread::sleep(settle);
                let Some(session) = state.session.as_ref() else {
                    unreachable!("started above")
                };
                let result = session.send(chord, paste_with_shift);
                state.last_paste = Instant::now();
                if result.is_err() {
                    state.session = None;
                }
                result
            }
        }
    }
}

/// Whether this session pastes through the desktop's RemoteDesktop portal,
/// whose dialog asks once before the first paste: Wayland without `wtype`'s
/// virtual keyboard (GNOME, KDE).
pub(crate) fn pastes_through_portal() -> bool {
    LinuxSession::detect().is_wayland() && !wtype_pastes()
}

fn wtype_pastes() -> bool {
    offers_virtual_keyboard() && on_path("wtype")
}

/// Whether the compositor advertises `zwp_virtual_keyboard_manager_v1`.
fn offers_virtual_keyboard() -> bool {
    wayland_offers("zwp_virtual_keyboard_manager_v1")
}

/// Whether the Wayland compositor advertises the global `interface`; false
/// without a Wayland connection.
pub(crate) fn wayland_offers(interface: &str) -> bool {
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::{Connection, Dispatch, QueueHandle};

    struct Globals;
    impl Dispatch<WlRegistry, GlobalListContents> for Globals {
        fn event(
            _: &mut Self,
            _: &WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let Ok(connection) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _queue)) = registry_queue_init::<Globals>(&connection) else {
        return false;
    };
    globals
        .contents()
        .with_list(|list| list.iter().any(|global| global.interface == interface))
}

fn on_path(program: &str) -> bool {
    on_path_in(std::env::var_os("PATH").as_deref(), program)
}

fn on_path_in(path: Option<&OsStr>, program: &str) -> bool {
    path.is_some_and(|path| {
        std::env::split_paths(path).any(|dir| {
            std::fs::metadata(dir.join(program))
                .is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
        })
    })
}

/// Every Wayland copy and paste runs `wl-copy`, from the `wl-clipboard`
/// package that direct installs do not bring.
const WL_COPY: &str = "wl-copy";

/// How a paste explains a Wayland session without `wl-copy`: the spawn error
/// alone ("No such file or directory") never named the package.
const WL_COPY_MISSING: &str =
    "wl-copy is not installed. Install the wl-clipboard package, then try again.";

/// What Settings and `hex status` warn with while this Wayland session can't
/// copy or paste at all.
pub(crate) fn missing_wayland_helper() -> Option<&'static str> {
    if !LinuxSession::detect().is_wayland() {
        return None;
    }
    missing_wl_copy(std::env::var_os("PATH").as_deref())
}

/// The notice Settings and `hex status` show while one runs.
pub(crate) fn listener_warning(running: bool) -> Option<String> {
    if !running {
        return None;
    }
    missing_wayland_helper().map(str::to_owned)
}

/// `Some` when `path` has no executable `wl-copy`; split out so a test needn't
/// change the process environment.
fn missing_wl_copy(path: Option<&OsStr>) -> Option<&'static str> {
    (!on_path_in(path, WL_COPY)).then_some(WL_COPY_MISSING)
}

/// Fails before spawning a helper that isn't there, naming what to install.
fn require_wl_copy() -> Result<()> {
    match missing_wl_copy(std::env::var_os("PATH").as_deref()) {
        Some(message) => Err(eyre!("{message}")),
        None => Ok(()),
    }
}

/// A Wayland paste that only reached the clipboard.
fn copied_only(
    error: color_eyre::eyre::Report,
    paste_with_shift: bool,
) -> color_eyre::eyre::Report {
    let shortcut = if paste_with_shift {
        "Ctrl+Shift+V"
    } else {
        "Ctrl+V"
    };
    eyre!(
        "Copied to the clipboard; press {shortcut} to paste. Automatic paste needs the desktop's RemoteDesktop portal ({}), or wtype with a compositor offering the virtual keyboard protocol: {error:#}",
        crate::linux::remote_desktop::INTERFACE
    )
}

impl LinuxPaster {
    pub(crate) fn new(
        stop: Arc<AtomicBool>,
        paste_with_shift: Arc<AtomicBool>,
        keys: ShortcutKeys,
    ) -> Result<Self> {
        let (backend, selection) = match LinuxSession::detect() {
            LinuxSession::X11 => {
                let paster = X11Paster::new()?;
                let transcript_owner = paster.clipboard.setter.window;
                (
                    Backend::X11(Box::new(paster)),
                    Selection::X11 { transcript_owner },
                )
            }
            LinuxSession::Wayland => (Backend::Wayland(WaylandKeys::Unchosen), Selection::Wayland),
        };
        Ok(Self {
            backend,
            stop,
            paste_with_shift,
            keys,
            selection,
            restore: Arc::default(),
        })
    }

    /// Puts `text` on the clipboard without pasting it. On X11 the clipboard
    /// is served for as long as this paster lives.
    pub(crate) fn copy(&mut self, text: &str) -> Result<()> {
        let mut restore = self.restore.lock().unwrap_or_else(PoisonError::into_inner);
        restore.keep();
        match &mut self.backend {
            Backend::X11(paster) => {
                let atoms = &paster.clipboard.getter.atoms;
                paster
                    .clipboard
                    .store(atoms.clipboard, atoms.utf8_string, text.as_bytes())
                    .wrap_err("could not own the X11 clipboard")
            }
            Backend::Wayland(_) => {
                require_wl_copy()?;
                let mut copy = Command::new("wl-copy");
                copy.args(["--type", "text/plain;charset=utf-8"]);
                run_helper(copy, text.as_bytes(), &self.stop, HELPER_TIMEOUT)
                    .wrap_err("could not own the Wayland clipboard; install wl-clipboard")
            }
        }
    }

    /// Selects the focused field's text (Ctrl+A).
    pub(crate) fn select_all(&mut self) -> Result<()> {
        self.send_chord(Chord::SelectAll)
    }

    /// Copies the focused field's selection (Ctrl+C).
    pub(crate) fn copy_selection(&mut self) -> Result<()> {
        self.send_chord(Chord::Copy)
    }

    /// Empties the clipboard, so a copy that the focused app ignores cannot
    /// leave older text looking like a fresh selection.
    pub(crate) fn clear_clipboard(&self) -> Result<()> {
        let selection = self.selection.current();
        // GNOME offers neither data-control protocol, so only wl-copy can
        // clear its clipboard; restoring an empty selection would fail.
        if selection == Selection::Wayland
            && crate::linux::clipboard::wayland_without_data_control()
        {
            require_wl_copy()?;
            let mut clear = Command::new(WL_COPY);
            clear.arg("--clear");
            return run_helper(clear, &[], &self.stop, HELPER_TIMEOUT)
                .wrap_err("could not clear the Wayland clipboard; install wl-clipboard");
        }
        selection.restore(Snapshot::Empty)
    }

    /// Starts a job that owns the clipboard until it restores: cancels any
    /// pending paste restore, adopts the clipboard that paste replaced when
    /// there is one, or snapshots the clipboard now. `None` when Hex cannot
    /// read it; the job still runs, and its result stays on the clipboard.
    pub(crate) fn begin_clipboard_job(&mut self) -> Result<Option<Snapshot>> {
        let selection = self.selection.current();
        let mut state = self.restore.lock().unwrap_or_else(PoisonError::into_inner);
        // The lock keeps a scheduled restore from putting its original back
        // between this cancel and the snapshot. Only a clipboard that still
        // holds the earlier paste keeps that paste's original.
        let pending_holds = state
            .inserted
            .as_ref()
            .is_some_and(|(mime, bytes)| selection.holds(mime.wayland(), bytes));
        if let Some(original) = state.begin(pending_holds) {
            return Ok(Some(original));
        }
        match selection.snapshot() {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(error) => {
                tracing::info!(
                    error = format!("{error:#}"),
                    "not restoring the clipboard after this typed-text job"
                );
                Ok(None)
            }
        }
    }

    /// The clipboard's plain text, bounded and within the read deadline.
    pub(crate) fn clipboard_text(&self) -> Result<Option<String>> {
        self.selection.current().text()
    }

    /// Whether Terminal paste is on. It turns every paste's Ctrl+V
    /// into Ctrl+Shift+V, so a typed-text replacement cannot use it.
    pub(crate) fn pastes_with_shift(&self) -> bool {
        self.paste_with_shift.load(Ordering::Relaxed)
    }

    /// Puts a [`Self::begin_clipboard_job`] snapshot back.
    pub(crate) fn restore(&self, snapshot: Snapshot) -> Result<()> {
        self.selection.current().restore(snapshot)
    }

    /// Sends Ctrl+A, Ctrl+C, or Ctrl+V through this session's key path.
    fn send_chord(&mut self, chord: Chord) -> Result<()> {
        match &mut self.backend {
            Backend::X11(paster) => paster.chord(chord, &self.stop, &self.keys),
            Backend::Wayland(wayland) => {
                self.keys.settle(&self.stop)?;
                wayland.send(&self.stop, chord, false)
            }
        }
    }

    pub(crate) fn paste(&mut self, text: &str, shift: bool) -> Result<()> {
        if text.trim().is_empty() {
            return Err(eyre!("refusing to paste an empty transcript"));
        }
        // Read as the paste commits, so the toggle applies at once. A kept
        // paste snapshots nothing and restores nothing, and an earlier
        // paste's pending restore stands down.
        let keep = crate::labs::keep_on_clipboard();
        self.paste_as(Mime::Text, text.as_bytes(), None, shift, keep)
    }

    /// Pastes over a typed-text selection, then puts `original` back: the
    /// clipboard from before the job copied the selection.
    pub(crate) fn paste_over(&mut self, text: &str, original: Snapshot) -> Result<()> {
        if text.trim().is_empty() {
            return Err(eyre!("refusing to paste an empty transformation"));
        }
        self.paste_as(Mime::Text, text.as_bytes(), Some(original), false, false)
    }

    /// Pastes a PNG (a capture's contact sheet) where the focus is, as the
    /// Mac's capture Paste does before its summary.
    pub(crate) fn paste_png(&mut self, png: &[u8]) -> Result<()> {
        self.paste_as(Mime::Png, png, None, false, false)
    }

    /// Pastes through the clipboard, then puts the previous clipboard back
    /// unless something newer replaced the paste first. `original` is the
    /// clipboard a typed-text job saved before it copied the selection.
    /// `shift` asks for Ctrl+Shift+V whatever the saved Terminal paste
    /// setting says. `keep` leaves the paste on the clipboard instead of
    /// restoring it (Labs › Keep dictation on clipboard).
    fn paste_as(
        &mut self,
        mime: Mime,
        bytes: &[u8],
        original: Option<Snapshot>,
        shift: bool,
        keep: bool,
    ) -> Result<()> {
        let restore = self.restore.clone();
        let selection = self.selection.current();
        let mut state = restore.lock().unwrap_or_else(PoisonError::into_inner);
        if keep {
            state.keep();
        } else {
            state.generation = state.generation.wrapping_add(1);
            // A paste before the previous one restored keeps that one's original.
            let previous_pending = state
                .inserted
                .take()
                .is_some_and(|(mime, bytes)| selection.holds(mime.wayland(), &bytes));
            if let Some(original) = original {
                state.original = Some(original);
            } else if !previous_pending {
                state.original = match selection.snapshot() {
                    Ok(snapshot) => Some(snapshot),
                    Err(error) => {
                        tracing::info!(
                            error = format!("{error:#}"),
                            "not restoring the clipboard after this paste"
                        );
                        None
                    }
                };
            }
        }
        let result = self.send_paste(mime, bytes, shift);
        if result.is_err() {
            // A paste that only reached the clipboard leaves the transcript
            // there for Ctrl+V; a typed-text job restores its own original.
            state.failed();
        } else if !keep {
            state.inserted = Some((mime, bytes.to_vec()));
            schedule_restore(restore.clone(), state.generation, selection);
        }
        drop(state);
        // Give the target time to request the selection before the next queued
        // transcript replaces it. This is a bounded settling policy, not an ACK.
        thread::sleep(PASTE_SETTLE);
        result
    }

    fn send_paste(&mut self, mime: Mime, bytes: &[u8], shift: bool) -> Result<()> {
        let paste_with_shift = shift || self.paste_with_shift.load(Ordering::Relaxed);
        match &mut self.backend {
            Backend::X11(paster) => {
                paster.paste(mime, bytes, &self.stop, &self.keys, paste_with_shift)?
            }
            Backend::Wayland(wayland) => {
                require_wl_copy()?;
                let mut copy = Command::new("wl-copy");
                copy.args(["--type", mime.wayland()]);
                run_helper(copy, bytes, &self.stop, HELPER_TIMEOUT)
                    .wrap_err("could not own the Wayland clipboard; install wl-clipboard and check compositor support")?;
                // wl-copy's parent exits only after the selection is installed.
                // Never read a pipe inherited by its clipboard-serving daemon.
                self.keys.settle(&self.stop)?;
                wayland
                    .send(&self.stop, Chord::Paste, paste_with_shift)
                    .map_err(|error| copied_only(error, paste_with_shift))?;
            }
        }
        Ok(())
    }
}

/// Puts the paste's original clipboard back once the target has had time to
/// read the paste, if no later paste, copy, or other client replaced it.
fn schedule_restore(restore: Arc<Mutex<Restore>>, generation: u64, selection: Selection) {
    thread::spawn(move || {
        thread::sleep(RESTORE_DELAY);
        let mut state = restore.lock().unwrap_or_else(PoisonError::into_inner);
        if state.generation != generation {
            return;
        }
        let (Some((mime, bytes)), Some(original)) = (state.inserted.take(), state.original.take())
        else {
            return;
        };
        if !selection.holds(mime.wayland(), &bytes) {
            tracing::debug!("the clipboard changed after the paste; leaving it");
            return;
        }
        if let Err(error) = selection.restore(original) {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not restore the clipboard after a paste"
            );
        }
    });
}

/// What a paste puts on the clipboard.
#[derive(Clone, Copy)]
enum Mime {
    Text,
    Png,
}

impl Mime {
    fn wayland(self) -> &'static str {
        match self {
            Self::Text => "text/plain;charset=utf-8",
            Self::Png => "image/png",
        }
    }
}

struct X11Paster {
    clipboard: Clipboard,
    connection: RustConnection,
    root: u32,
    control: u8,
    shift: u8,
    v: u8,
    /// Resolved only when Ctrl+A or Ctrl+C asks for them, so a keymap
    /// without A or C never breaks an ordinary Ctrl+V paste.
    keymap: Keymap,
}

impl X11Paster {
    fn new() -> Result<Self> {
        let clipboard = Clipboard::new().wrap_err("could not open the X11 clipboard")?;
        let (connection, root, keymap) = connect_x11()?;
        Ok(Self {
            clipboard,
            connection,
            root,
            control: keymap.keycode(XK_CONTROL_L)?,
            shift: keymap.keycode(XK_SHIFT_L)?,
            v: keymap.keycode(XK_V)?,
            keymap,
        })
    }

    /// Sends Ctrl+A, Ctrl+C, or Ctrl+V without touching the clipboard.
    fn chord(&mut self, chord: Chord, stop: &AtomicBool, keys: &ShortcutKeys) -> Result<()> {
        self.wait_for_modifier_release(stop, keys)?;
        let key = match chord {
            Chord::SelectAll => self.keymap.keycode(XK_A)?,
            Chord::Copy => self.keymap.keycode(XK_C)?,
            Chord::Paste => self.v,
        };
        self.send_keys(&[
            (KEY_PRESS, self.control),
            (KEY_PRESS, key),
            (KEY_RELEASE, key),
            (KEY_RELEASE, self.control),
        ])
    }

    fn paste(
        &mut self,
        mime: Mime,
        bytes: &[u8],
        stop: &AtomicBool,
        keys: &ShortcutKeys,
        paste_with_shift: bool,
    ) -> Result<()> {
        self.wait_for_modifier_release(stop, keys)?;
        let atoms = &self.clipboard.getter.atoms;
        let target = match mime {
            Mime::Text => atoms.utf8_string,
            Mime::Png => self
                .clipboard
                .getter
                .get_atom("image/png")
                .wrap_err("could not name the X11 image/png target")?,
        };
        self.clipboard
            .store(atoms.clipboard, target, bytes)
            .wrap_err("could not own the X11 clipboard")?;
        thread::sleep(Duration::from_millis(60));
        let mut sequence = vec![(KEY_PRESS, self.control)];
        if paste_with_shift {
            sequence.push((KEY_PRESS, self.shift));
        }
        sequence.extend([(KEY_PRESS, self.v), (KEY_RELEASE, self.v)]);
        if paste_with_shift {
            sequence.push((KEY_RELEASE, self.shift));
        }
        sequence.push((KEY_RELEASE, self.control));
        self.send_keys(&sequence)
    }

    /// Sends `sequence` in order. A call that fails may still reach the
    /// server, so every key still held is released before the error returns.
    fn send_keys(&mut self, sequence: &[(u8, u8)]) -> Result<()> {
        let mut pressed = Vec::new();
        let result = (|| -> Result<()> {
            for &(type_, key) in sequence {
                if type_ == KEY_PRESS {
                    // A failed call may still reach the server, so release it too.
                    pressed.push(key);
                }
                xtest::fake_input(&self.connection, type_, key, 0, self.root, 0, 0, 0)?.check()?;
                // A failed release stays recorded: the key may still be down,
                // and releasing one already up is harmless.
                if type_ == KEY_RELEASE
                    && let Some(index) = pressed.iter().rposition(|held| *held == key)
                {
                    pressed.remove(index);
                }
            }
            self.connection.flush()?;
            Ok(())
        })();
        if let Err(error) = result {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not send the X11 clipboard shortcut"
            );
            for key in pressed.into_iter().rev() {
                let released = (|| -> Result<()> {
                    xtest::fake_input(&self.connection, KEY_RELEASE, key, 0, self.root, 0, 0, 0)?
                        .check()?;
                    Ok(())
                })();
                if let Err(error) = released {
                    tracing::warn!(
                        key,
                        error = format!("{error:#}"),
                        "could not release an X11 clipboard key"
                    );
                }
            }
            let _ = self.connection.flush();
            return Err(error);
        }
        Ok(())
    }

    fn wait_for_modifier_release(&self, stop: &AtomicBool, keys: &ShortcutKeys) -> Result<()> {
        wait_for_modifiers(
            stop,
            MODIFIER_WAIT_LIMIT,
            || keys.any_held(),
            || {
                let mask = self.connection.query_pointer(self.root)?.reply()?.mask;
                let held = mask
                    & (KeyButMask::SHIFT
                        | KeyButMask::CONTROL
                        | KeyButMask::MOD1
                        | KeyButMask::MOD4);
                Ok(held != KeyButMask::default())
            },
        )
    }
}

/// The evdev codes of the keys the clipboard chords press (`KEY_A`, `KEY_C`,
/// `KEY_V`).
const EVDEV_KEY_A: usize = 30;
const EVDEV_KEY_C: usize = 46;
const EVDEV_KEY_V: usize = 47;

fn chord_keys(chord: Chord, with_shift: bool) -> Vec<String> {
    let (key, evdev, shift) = match chord {
        Chord::SelectAll => ("a", EVDEV_KEY_A, false),
        Chord::Copy => ("c", EVDEV_KEY_C, false),
        // Only a paste reads the terminal-paste preference.
        Chord::Paste => ("v", EVDEV_KEY_V, with_shift),
    };
    // wtype numbers its keymap in order of first use from evdev code 1, so a
    // bare `-k v` sends evdev 1 (Escape in the standard keymap). Hyprland
    // reads virtual-keyboard keys through its configured layout to match
    // binds, so it saw Ctrl+Shift+Escape and a user bind swallowed the paste
    // (#444). Releasing distinct, never-pressed placeholder keysyms first puts
    // each key on its evdev code, which any compositor's layout also reads as
    // that key.
    let mut keys: Vec<String> = (0..evdev - 1)
        .flat_map(|index| ["-p".to_owned(), format!("U{:04X}", 0x2460 + index)])
        .collect();
    keys.extend(["-M".to_owned(), "ctrl".to_owned()]);
    if shift {
        keys.extend(["-M".to_owned(), "shift".to_owned()]);
    }
    keys.extend(["-k".to_owned(), key.to_owned()]);
    if shift {
        keys.extend(["-m".to_owned(), "shift".to_owned()]);
    }
    keys.extend(["-m".to_owned(), "ctrl".to_owned()]);
    // GTK 3's reverse keymap lookup skips the final keycode. Keep the key
    // before a neutral VoidSymbol release so its shortcut remains discoverable
    // (#28).
    keys.extend(["-p".to_owned(), "VoidSymbol".to_owned()]);
    keys
}

/// Waits until the X server reports no held modifiers. While one of Hex's
/// shortcuts is down, a newer capture owns them, so its predecessor stays
/// queued for as long as it is held. Otherwise the wait ends after `limit`
/// and the paste goes ahead: XWayland's modifier state goes stale while a
/// Wayland window has focus and only a later key event clears it, which
/// held accepted output back for a minute (#733).
fn wait_for_modifiers(
    stop: &AtomicBool,
    limit: Duration,
    shortcut_held: impl Fn() -> bool,
    mut held: impl FnMut() -> Result<bool>,
) -> Result<()> {
    let began = Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("paste cancelled while stopping the listener"));
        }
        if !held()? {
            return Ok(());
        }
        if began.elapsed() >= limit && !shortcut_held() {
            tracing::warn!(
                waited_ms = began.elapsed().as_millis() as u64,
                "modifiers still read as held with no Hex shortcut down; pasting anyway"
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_helper(
    mut command: Command,
    input: &[u8],
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Err(eyre!("paste cancelled before starting a helper"));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .wrap_err("could not start the paste helper")?;
    let process_group = child.id();
    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre!("missing helper stdin"))?;
        let fd = stdin.as_raw_fd();
        // Nonblocking writes keep a stalled helper from bypassing cancellation
        // or the deadline when a long transcript fills its stdin pipe.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let deadline = Instant::now() + timeout;
        let mut remaining = input;
        while !remaining.is_empty() {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(eyre!("paste helper cancelled or timed out"));
            }
            match stdin.write(remaining) {
                Ok(0) => return Err(eyre!("paste helper stopped reading its input")),
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        drop(stdin);
        loop {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(eyre!("paste helper cancelled or timed out"));
            }
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(eyre!("paste helper failed with {status}"))
                };
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        // Terminate descendants too: a failed wl-copy must not claim the
        // clipboard later, after this request has been reported as cancelled.
        unsafe { libc::kill(-(process_group as i32), libc::SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    /// A typed-text job may start while a dictation paste still waits to
    /// restore: it must cancel that restore and keep its original, not
    /// snapshot the transcript the paste left behind.
    #[test]
    fn a_new_clipboard_job_adopts_the_pending_restore() {
        let original = Snapshot::Wayland(vec![(
            "text/plain;charset=utf-8".into(),
            b"before the dictation".to_vec(),
        )]);
        let mut restore = Restore {
            generation: 7,
            original: Some(original.clone()),
            inserted: Some((Mime::Text, b"the dictation".to_vec())),
        };
        assert_eq!(restore.begin(true), Some(original));
        assert_eq!(restore.generation, 8, "the pending restore is cancelled");
        assert!(restore.inserted.is_none());
        assert!(restore.original.is_none());
        assert_eq!(restore.begin(true), None);
    }

    /// A dictation paste that only reached the clipboard leaves its
    /// transcript there for Ctrl+V: nothing is put back over it.
    #[test]
    fn a_failed_dictation_paste_leaves_the_transcript_on_the_clipboard() {
        let mut restore = Restore {
            generation: 2,
            original: Some(Snapshot::Wayland(vec![(
                "text/plain;charset=utf-8".into(),
                b"before the dictation".to_vec(),
            )])),
            inserted: Some((Mime::Text, b"the dictation".to_vec())),
        };
        restore.failed();
        assert!(
            restore.original.is_none(),
            "nothing is restored over the transcript"
        );
        assert!(restore.inserted.is_none());
    }

    /// If something newer replaced the transcript, a new job snapshots that
    /// clipboard instead of putting the older paste's original back over it.
    #[test]
    fn a_changed_clipboard_is_not_replaced_by_a_pending_restore() {
        let stale = Snapshot::Wayland(vec![(
            "text/plain;charset=utf-8".into(),
            b"before the dictation".to_vec(),
        )]);
        let mut restore = Restore {
            generation: 3,
            original: Some(stale),
            inserted: Some((Mime::Text, b"the dictation".to_vec())),
        };
        assert_eq!(restore.begin(false), None);
        assert_eq!(restore.generation, 4);
        assert!(restore.original.is_none());
    }

    /// Labs › Keep dictation on clipboard: a kept paste cancels the restore
    /// an earlier paste left pending and keeps nothing to put back, so a
    /// later restoring paste cannot bring an older clipboard back.
    #[test]
    fn a_kept_paste_stands_down_the_pending_restore() {
        let stale = Snapshot::Wayland(vec![(
            "text/plain;charset=utf-8".into(),
            b"before the dictation".to_vec(),
        )]);
        let mut restore = Restore {
            generation: 5,
            original: Some(stale),
            inserted: Some((Mime::Text, b"the dictation".to_vec())),
        };
        restore.keep();
        assert_eq!(restore.generation, 6, "the pending restore is cancelled");
        assert!(restore.original.is_none(), "nothing is left to put back");
        assert!(restore.inserted.is_none());
    }

    #[test]
    fn a_missing_wl_copy_names_the_package_to_install() {
        let missing = missing_wl_copy(Some(OsStr::new("/hex-test-no-such-directory")));
        assert_eq!(missing, Some(WL_COPY_MISSING));

        // A `wl-copy` on PATH that is executable is enough.
        let directory = std::env::temp_dir().join(format!("hex-paste-path-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let helper = directory.join("wl-copy");
        std::fs::write(&helper, b"").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(missing_wl_copy(Some(directory.as_os_str())), None);

        // A file that cannot run is no better than no helper at all.
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            missing_wl_copy(Some(directory.as_os_str())),
            Some(WL_COPY_MISSING)
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn helper_failure_is_not_reported_as_a_successful_paste() {
        assert!(
            run_helper(
                shell("exit 7"),
                &[],
                &AtomicBool::new(false),
                HELPER_TIMEOUT
            )
            .is_err()
        );
    }

    #[test]
    fn standard_and_terminal_paste_balance_their_modifiers() {
        let padding = (EVDEV_KEY_V - 1) * 2;
        assert_eq!(
            chord_keys(Chord::Paste, false)[padding..],
            ["-M", "ctrl", "-k", "v", "-m", "ctrl", "-p", "VoidSymbol"]
        );
        assert_eq!(
            chord_keys(Chord::Paste, true)[padding..],
            [
                "-M",
                "ctrl",
                "-M",
                "shift",
                "-k",
                "v",
                "-m",
                "shift",
                "-m",
                "ctrl",
                "-p",
                "VoidSymbol"
            ]
        );
    }

    /// Ctrl+A and Ctrl+C land on their own evdev keys, and the terminal-paste
    /// preference never adds Shift to them.
    #[test]
    fn select_all_and_copy_send_their_keys_without_shift() {
        for (chord, key, evdev) in [
            (Chord::SelectAll, "a", EVDEV_KEY_A),
            (Chord::Copy, "c", EVDEV_KEY_C),
        ] {
            for with_shift in [false, true] {
                let args = chord_keys(chord, with_shift);
                assert_eq!(
                    args,
                    chord_keys(chord, false),
                    "the paste preference changed {key}"
                );
                assert_eq!(
                    args[(evdev - 1) * 2..],
                    ["-M", "ctrl", "-k", key, "-m", "ctrl", "-p", "VoidSymbol"]
                );
            }
        }
    }

    /// wtype gives each new keysym the next evdev code from 1, in argument
    /// order; V must land on `KEY_V` so Hyprland's binds read V (#444).
    #[test]
    fn wtype_sends_each_key_on_its_evdev_code() {
        for (chord, key, evdev) in [
            (Chord::Paste, "v", EVDEV_KEY_V),
            (Chord::SelectAll, "a", EVDEV_KEY_A),
            (Chord::Copy, "c", EVDEV_KEY_C),
        ] {
            for with_shift in [false, true] {
                let args = chord_keys(chord, with_shift);
                let mut keymap: Vec<&str> = Vec::new();
                for pair in args.windows(2) {
                    if matches!(pair[0].as_str(), "-k" | "-p" | "-P")
                        && !keymap.contains(&pair[1].as_str())
                    {
                        keymap.push(&pair[1]);
                    }
                }
                let position = keymap
                    .iter()
                    .position(|candidate| *candidate == key)
                    .unwrap()
                    + 1;
                assert_eq!(position, evdev, "{chord:?}");
                assert_eq!(keymap.last(), Some(&"VoidSymbol"));
            }
        }
    }

    #[test]
    fn helper_can_read_a_large_transcript_without_argv_or_files() {
        let text = b"--help\nplain text\n".repeat(32_768);
        let mut command = shell("test \"$(wc -c)\" -eq \"$1\"");
        command.args(["hex-paste-test", &text.len().to_string()]);
        assert!(run_helper(command, &text, &AtomicBool::new(false), HELPER_TIMEOUT).is_ok());
    }

    #[test]
    fn helper_timeout_includes_a_full_stdin_pipe() {
        let start = Instant::now();
        assert!(
            run_helper(
                shell("sleep 10"),
                &vec![b'x'; 1_048_576],
                &AtomicBool::new(false),
                Duration::from_millis(100),
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn stop_cancels_a_helper_that_never_finishes() {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel.store(true, Ordering::Release);
        });
        let start = Instant::now();
        let result = run_helper(shell("sleep 10"), &[], &stop, HELPER_TIMEOUT);
        worker.join().unwrap();
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_clipboard_daemon_inheriting_stderr_does_not_block_parent_completion() {
        let path = std::env::temp_dir().join(format!("hex-paste-daemon-{}", std::process::id()));
        let mut command = shell("cat >/dev/null; sleep 10 >&2 & echo $! >\"$1\"");
        command.arg("hex-paste-test").arg(&path);
        let result = run_helper(
            command,
            b"example",
            &AtomicBool::new(false),
            Duration::from_secs(2),
        );
        if let Ok(pid) = std::fs::read_to_string(&path) {
            if let Ok(pid) = pid.trim().parse::<i32>() {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            let _ = std::fs::remove_file(path);
        }
        assert!(result.is_ok());
    }

    #[test]
    fn accepted_output_waits_through_another_capture() {
        let start = Instant::now();
        let mut checks = 0;
        // The newer capture's shortcut stays down past the limit.
        wait_for_modifiers(
            &AtomicBool::new(false),
            Duration::from_millis(100),
            || true,
            || {
                checks += 1;
                Ok(start.elapsed() < Duration::from_millis(550))
            },
        )
        .unwrap();
        assert!(checks > 1);
        assert!(start.elapsed() >= Duration::from_millis(550));
    }

    /// XWayland can report a modifier held long after it came up (#733).
    #[test]
    fn stale_modifiers_without_a_held_shortcut_stop_holding_the_paste() {
        let start = Instant::now();
        wait_for_modifiers(
            &AtomicBool::new(false),
            Duration::from_millis(100),
            || false,
            || Ok(true),
        )
        .unwrap();
        let waited = start.elapsed();
        assert!(waited >= Duration::from_millis(100));
        assert!(waited < Duration::from_secs(2));
    }

    #[test]
    fn stopping_breaks_the_modifier_wait_without_checking_the_keyboard() {
        assert!(
            wait_for_modifiers(
                &AtomicBool::new(true),
                MODIFIER_WAIT_LIMIT,
                || false,
                || panic!("should not read devices")
            )
            .is_err()
        );
        assert!(run_helper(shell("exit 0"), &[], &AtomicBool::new(true), HELPER_TIMEOUT).is_err());
    }

    #[test]
    #[ignore = "run scripts/test-wayland-paste.sh with the compiled test executable"]
    fn native_wayland_clipboard_shortcut() {
        assert!(LinuxSession::detect().is_wayland());
        let path = std::env::var_os("HEX_WAYLAND_PASTE_OUTPUT")
            .expect("the isolated Wayland test runner must supply its output file");
        let expected = "Example dictation: --flags, punctuation, \u{6f22}\u{5b57}.\nSecond line.";
        let wait_for = |what: &str, done: &mut dyn FnMut() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !done() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                thread::sleep(Duration::from_millis(20));
            }
        };
        let wl_paste = |args: &[&str]| {
            Command::new("wl-paste")
                .args(args)
                .stderr(Stdio::null())
                .output()
                .unwrap()
        };
        let original = vec![
            ("text/html".to_owned(), b"<b>original</b>".to_vec()),
            ("text/plain;charset=utf-8".to_owned(), b"original".to_vec()),
        ];
        Selection::Wayland
            .restore(Snapshot::Wayland(original.clone()))
            .unwrap();
        let mut paster = LinuxPaster::new(
            Arc::new(AtomicBool::new(false)),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        paster.paste(expected, false).unwrap();
        wait_for("GTK to receive the transcript", &mut || {
            std::fs::read_to_string(&path).unwrap_or_default() == expected
        });
        // Every format of the previous clipboard comes back.
        wait_for("the original clipboard", &mut || {
            original.iter().all(|(mime, data)| {
                let output = wl_paste(&["--no-newline", "--type", mime]);
                output.status.success() && output.stdout == *data
            })
        });

        // An empty clipboard stays empty.
        assert!(
            Command::new("wl-copy")
                .arg("--clear")
                .status()
                .unwrap()
                .success()
        );
        wait_for("the clipboard to clear", &mut || {
            !wl_paste(&["--list-types"]).status.success()
        });
        paster.paste(expected, false).unwrap();
        let twice = expected.repeat(2);
        wait_for("GTK to receive the second transcript", &mut || {
            std::fs::read_to_string(&path).unwrap_or_default() == twice
        });
        thread::sleep(RESTORE_DELAY * 2);
        assert!(
            !wl_paste(&["--list-types"]).status.success(),
            "the transcript stayed on a clipboard that was empty"
        );

        // Labs › Keep dictation on clipboard: the dictation stays, and the
        // clipboard it replaced never comes back.
        Selection::Wayland
            .restore(Snapshot::Wayland(original.clone()))
            .unwrap();
        crate::labs::apply(&crate::app_settings::LabsSettings {
            keep_on_clipboard: true,
            ..crate::app_settings::LabsSettings::default()
        });
        paster.paste(expected, false).unwrap();
        let thrice = expected.repeat(3);
        wait_for("GTK to receive the kept transcript", &mut || {
            std::fs::read_to_string(&path).unwrap_or_default() == thrice
        });
        thread::sleep(RESTORE_DELAY * 2);
        let kept = wl_paste(&["--no-newline", "--type", "text/plain;charset=utf-8"]);
        assert!(
            kept.status.success() && kept.stdout == expected.as_bytes(),
            "a kept paste left the transcript on the clipboard"
        );
        crate::labs::apply(&crate::app_settings::LabsSettings::default());
    }
}
