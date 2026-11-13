//! The clipboard a Linux paste puts back afterwards: every format of the
//! CLIPBOARD selection, read within a size cap and deadline, then served
//! again from Hex until another client takes the clipboard. Wayland reads
//! and serves through ext-data-control or wlr-data-control. X11 uses its own selection owner,
//! because `x11-clipboard` serves a single target. A Wayland desktop with
//! neither protocol (GNOME's Mutter) restores nothing unless Labs › Restore
//! clipboard on GNOME is on and XWayland already runs: then the paste reads
//! and restores the CLIPBOARD selection through XWayland, which Mutter
//! bridges to Wayland apps both ways (#1111). Hex never starts XWayland.

use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};

/// The most clipboard data a paste preserves. Larger clipboards (big
/// images) are left replaced by the transcript.
pub(crate) const CAP: usize = 32 * 1024 * 1024;
/// The most plain text a typed-text job reads from the clipboard: a larger
/// selection is reported as unreadable rather than held on the output
/// worker.
const TEXT_CAP: usize = 1024 * 1024;
/// How long a paste waits for the clipboard's owner to hand over its formats.
const READ_DEADLINE: Duration = Duration::from_secs(1);
/// How long a restored clipboard waits for a slow reader.
const SERVE_DEADLINE: Duration = Duration::from_secs(2);

/// The clipboard MIME types and X11 targets that carry plain text, preferred
/// first.
const TEXT_TARGETS: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Snapshot {
    /// Nothing owned the clipboard; restoring clears it.
    Empty,
    Wayland(Vec<(String, Vec<u8>)>),
    X11(Vec<X11Format>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct X11Format {
    target: u32,
    type_: u32,
    format: u8,
    data: Vec<u8>,
}

impl AsRef<[u8]> for X11Format {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

/// The session's clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    Wayland,
    /// A Wayland session restoring through XWayland's selection, by the
    /// explicit Labs opt-in ([`Selection::current`]).
    XWayland,
    /// `transcript_owner` is the window `x11-clipboard` pastes from.
    X11 {
        transcript_owner: u32,
    },
}

impl Selection {
    /// The selection a paste uses now: a Wayland session switches to
    /// XWayland's only while [`uses_xwayland`] says so.
    pub(crate) fn current(self) -> Self {
        match self {
            Self::Wayland | Self::XWayland => {
                if uses_xwayland(
                    wayland::offers_data_control(),
                    crate::labs::gnome_clipboard_restore(),
                    xwayland_running,
                ) {
                    Self::XWayland
                } else {
                    Self::Wayland
                }
            }
            x11 @ Self::X11 { .. } => x11,
        }
    }

    pub(crate) fn snapshot(self) -> Result<Snapshot> {
        match self {
            Self::Wayland => wayland::snapshot(None),
            Self::XWayland => x11::snapshot().wrap_err("could not read the XWayland clipboard"),
            Self::X11 { .. } => x11::snapshot(),
        }
    }

    /// The clipboard's plain text, within [`TEXT_CAP`] and the read
    /// deadline; `None` when it holds no readable text.
    pub(crate) fn text(self) -> Result<Option<String>> {
        match self {
            Self::Wayland => wayland::text(),
            Self::XWayland => x11::text().wrap_err("could not read the XWayland clipboard"),
            Self::X11 { .. } => x11::text(),
        }
    }

    /// Whether the clipboard still holds what Hex pasted: the same bytes on
    /// Wayland, the paste's own selection owner on X11.
    pub(crate) fn holds(self, mime: &str, bytes: &[u8]) -> bool {
        match self {
            Self::Wayland => matches!(
                wayland::snapshot(Some(mime)),
                Ok(Snapshot::Wayland(formats)) if formats.first().is_some_and(|(_, data)| data == bytes)
            ),
            // XWayland names a Wayland client's MIME types as X11 targets,
            // and Mutter adds UTF8_STRING for text.
            Self::XWayland => {
                x11::holds(mime, bytes)
                    || (mime.starts_with("text/plain") && x11::holds("UTF8_STRING", bytes))
            }
            Self::X11 { transcript_owner } => {
                x11::owner().is_ok_and(|owner| owner == transcript_owner)
            }
        }
    }

    pub(crate) fn restore(self, snapshot: Snapshot) -> Result<()> {
        match snapshot {
            Snapshot::Empty => match self {
                Self::Wayland => wayland::serve(Vec::new()),
                Self::XWayland | Self::X11 { .. } => x11::serve(Vec::new()),
            },
            Snapshot::Wayland(formats) => wayland::serve(formats),
            Snapshot::X11(formats) => x11::serve(formats),
        }
    }
}

/// Whether this is a Wayland session whose compositor offers no
/// data-control protocol (GNOME), so pastes restore the clipboard only
/// through Labs › Restore clipboard on GNOME.
pub(crate) fn wayland_without_data_control() -> bool {
    crate::linux::session::LinuxSession::detect().is_wayland() && !wayland::offers_data_control()
}

/// Whether a Wayland paste restores through XWayland: only when the
/// compositor offers no data-control protocol, the person turned on Labs ›
/// Restore clipboard on GNOME, and XWayland already runs. `running` is
/// asked last, so other sessions never look for XWayland.
fn uses_xwayland(data_control: bool, opted_in: bool, running: impl FnOnce() -> bool) -> bool {
    !data_control && opted_in && running()
}

/// Whether this user's XWayland is up for `DISPLAY`. GNOME starts XWayland
/// when a client connects to its socket, so this looks for the process
/// rather than connecting.
pub(crate) fn xwayland_running() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())
        && std::fs::read_dir("/proc").is_ok_and(|entries| {
            let uid = unsafe { libc::getuid() };
            xwayland_listed(
                uid,
                entries.filter_map(|entry| {
                    use std::os::unix::fs::MetadataExt;
                    let entry = entry.ok()?;
                    let owner = entry.metadata().ok()?.uid();
                    let comm = std::fs::read_to_string(entry.path().join("comm")).ok()?;
                    Some((owner, comm))
                }),
            )
        })
}

/// Whether `processes` (owner and `/proc/<pid>/comm`) hold `uid`'s XWayland.
fn xwayland_listed(uid: u32, mut processes: impl Iterator<Item = (u32, String)>) -> bool {
    processes.any(|(owner, comm)| owner == uid && comm.trim_end() == "Xwayland")
}

fn too_large() -> color_eyre::eyre::Report {
    eyre!("the clipboard holds more than {} MiB", CAP / (1024 * 1024))
}

/// How soon a format is read: the text every application asks for, then the
/// common image one, then the offer's own order. The one-second deadline is
/// shared, so a slow exotic format must not spend it before the text is saved.
fn preferred(mime: &str) -> Option<usize> {
    TEXT_TARGETS
        .iter()
        .position(|target| *target == mime)
        .or_else(|| (mime == "image/png").then_some(TEXT_TARGETS.len()))
}

/// The first few skipped formats, so a long offer cannot flood one log line.
fn names(skipped: &[&str]) -> String {
    const SHOWN: usize = 8;
    let mut listed = skipped
        .iter()
        .take(SHOWN)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if skipped.len() > SHOWN {
        listed.push_str(&format!(", +{} more", skipped.len() - SHOWN));
    }
    listed
}

/// Reads every format of the clipboard, skipping the ones its owner fails to
/// hand over, and hands them back in the offer's own order so the restore
/// still offers exactly what was there. Formats are not even read once the
/// shared deadline has passed. An offer that held formats but yielded none is
/// an error, never an empty snapshot: an empty snapshot clears the clipboard,
/// and a failed read must not throw it away. `key` is what the caller's reader
/// needs to ask for the format, kept out of the label.
fn collect_formats<K: Copy, T: AsRef<[u8]>>(
    formats: Vec<(K, String)>,
    cap: usize,
    deadline: Instant,
    mut read: impl FnMut(K, &str, usize) -> Result<T>,
) -> Result<Vec<(String, T)>> {
    let offered = formats.len();
    let mut order: Vec<usize> = (0..offered).collect();
    order.sort_by_key(|index| preferred(&formats[*index].1).unwrap_or(usize::MAX));
    let mut read_formats: Vec<Option<(String, T)>> = (0..offered).map(|_| None).collect();
    let mut total = 0;
    let mut skipped: Vec<&str> = Vec::new();
    for index in order {
        let (key, mime) = &formats[index];
        if Instant::now() >= deadline {
            skipped.push(mime);
            continue;
        }
        let budget = cap.saturating_sub(total);
        match read(*key, mime, budget) {
            Ok(data) if data.as_ref().len() <= budget => {
                total += data.as_ref().len();
                read_formats[index] = Some((mime.clone(), data));
            }
            Ok(_) => {
                tracing::debug!(mime, "a clipboard format is over the remaining cap");
                skipped.push(mime);
            }
            Err(error) => {
                tracing::debug!(
                    mime,
                    error = format!("{error:#}"),
                    "could not read a clipboard format"
                );
                skipped.push(mime);
            }
        }
    }
    let read_formats: Vec<(String, T)> = read_formats.into_iter().flatten().collect();
    if read_formats.is_empty() && offered > 0 {
        return Err(eyre!(
            "none of the clipboard's {offered} formats could be read ({})",
            names(&skipped)
        ));
    }
    if !skipped.is_empty() {
        tracing::info!(
            skipped = %names(&skipped),
            "restoring {} of {offered} clipboard formats",
            read_formats.len()
        );
    }
    Ok(read_formats)
}

/// The clipboard's text through `wl-paste`, for a Wayland compositor without
/// data-control (GNOME). `wl-paste` comes with the `wl-copy` every Wayland
/// paste already needs. Its windowless client can briefly take focus on
/// GNOME, which is why Hex reads through data-control wherever it exists.
/// `--type text` keeps a binary clipboard from being read as text.
fn wl_paste_text() -> Result<Option<String>> {
    let mut child = Command::new("wl-paste")
        .args(["--no-newline", "--type", "text"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not start wl-paste; install wl-clipboard")?;
    let deadline = Instant::now() + READ_DEADLINE;
    let result = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("wl-paste has no output"))?;
        let data = read_bounded(OwnedFd::from(stdout), TEXT_CAP, deadline)?;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err(eyre!("wl-paste did not finish in time"));
            }
            thread::sleep(Duration::from_millis(5));
        };
        if !status.success() {
            return Ok(None);
        }
        Ok(String::from_utf8(data).ok())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn set_nonblocking(fd: &OwnedFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Waits until `fd` is ready for `events` or the deadline passes.
fn wait_for(fd: RawFd, events: libc::c_short, deadline: Instant) -> Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(eyre!("timed out"));
    }
    let mut poll = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut poll, 1, remaining.as_millis().max(1) as libc::c_int) };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
    Ok(())
}

/// Reads `fd` to its end, failing past `budget` bytes or the deadline.
fn read_bounded(fd: OwnedFd, budget: usize, deadline: Instant) -> Result<Vec<u8>> {
    set_nonblocking(&fd)?;
    let mut file = std::fs::File::from(fd);
    let mut data = Vec::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(data),
            Ok(read) => {
                if data.len() + read > budget {
                    return Err(too_large());
                }
                data.extend_from_slice(&buffer[..read]);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                wait_for(file.as_raw_fd(), libc::POLLIN, deadline)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Writes all of `data` to a reader's `fd`, giving up at the deadline.
fn write_bounded(fd: OwnedFd, data: &[u8], deadline: Instant) -> Result<()> {
    set_nonblocking(&fd)?;
    let mut file = std::fs::File::from(fd);
    let mut remaining = data;
    while !remaining.is_empty() {
        match file.write(remaining) {
            Ok(0) => return Err(eyre!("the reader closed the clipboard pipe")),
            Ok(written) => remaining = &remaining[written..],
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                wait_for(file.as_raw_fd(), libc::POLLOUT, deadline)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// The Wayland snapshot and restore over a data-control protocol whose
/// manager, device, offer, and source modules the caller imports as
/// `manager`, `device`, `offer`, and `source`: wlroots' and the standard
/// `ext` one share every request and event Hex uses.
macro_rules! data_control {
    ($name:literal) => {
        use std::collections::HashMap;
        use std::os::fd::AsFd;

        use wayland_client::backend::ObjectId;
        use wayland_client::globals::{GlobalListContents, registry_queue_init};
        use wayland_client::protocol::wl_registry::{self, WlRegistry};
        use wayland_client::protocol::wl_seat::{self, WlSeat};
        use wayland_client::{
            Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child,
        };

        #[derive(Default)]
        struct State {
            offers: HashMap<ObjectId, Vec<String>>,
            /// `None` until the device reports the selection.
            selection: Option<Option<Offer>>,
            served: Vec<(String, Vec<u8>)>,
            done: bool,
        }

        struct Session {
            connection: Connection,
            queue: EventQueue<State>,
            state: State,
            manager: Manager,
            device: Device,
        }

        fn connect() -> Result<Session> {
            let connection =
                Connection::connect_to_env().wrap_err("could not connect to the compositor")?;
            let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
            let handle = queue.handle();
            let manager: Manager = globals
                .bind(&handle, 1..=1, ())
                .wrap_err(concat!("the compositor offers no ", $name, " clipboard access"))?;
            let seat: WlSeat = globals
                .bind(&handle, 1..=1, ())
                .wrap_err("the compositor offers no seat")?;
            let device = manager.get_data_device(&seat, &handle, ());
            let mut state = State::default();
            // The device reports the current selection as soon as it exists.
            for _ in 0..2 {
                if state.selection.is_some() {
                    break;
                }
                queue.roundtrip(&mut state)?;
            }
            Ok(Session {
                connection,
                queue,
                state,
                manager,
                device,
            })
        }

        /// Every format of the selection, or only `only`.
        pub(super) fn snapshot(only: Option<&str>) -> Result<Snapshot> {
            let deadline = Instant::now() + READ_DEADLINE;
            let mut session = connect()?;
            let Some(Some(offer)) = session.state.selection.clone() else {
                return Ok(Snapshot::Empty);
            };
            let offer_id = offer.id();
            let mimes: Vec<(usize, String)> = session
                .state
                .offers
                .get(&offer.id())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|mime| only.is_none_or(|only| only == mime))
                .enumerate()
                .collect();
            let formats = collect_formats(mimes, CAP, deadline, |_, mime, budget| {
                let (read, write) = pipe()?;
                offer.receive(mime.to_owned(), write.as_fd());
                drop(write);
                session.connection.flush()?;
                read_bounded(read, budget, deadline)
                    .wrap_err_with(|| format!("could not read the clipboard's {mime}"))
            })?;
            // A selection that changed during the read handed over formats
            // from two different clipboards. Only a roundtrip can see a new
            // selection; `connect` already roundtrips without a bound, and
            // the paste's own still-holds-the-transcript check covers a later
            // change.
            session
                .queue
                .roundtrip(&mut session.state)
                .map_err(|error| eyre!("could not follow the clipboard's selection: {error}"))?;
            if !matches!(&session.state.selection, Some(Some(current)) if current.id() == offer_id)
            {
                return Err(eyre!("the clipboard changed while it was being read"));
            }
            Ok(if formats.is_empty() {
                Snapshot::Empty
            } else {
                Snapshot::Wayland(formats)
            })
        }

        /// The selection's plain text, within [`TEXT_CAP`] and the read
        /// deadline; `None` when it offers no text.
        pub(super) fn text() -> Result<Option<String>> {
            let deadline = Instant::now() + READ_DEADLINE;
            let session = connect()?;
            let Some(Some(offer)) = session.state.selection.clone() else {
                return Ok(None);
            };
            let mimes = session
                .state
                .offers
                .get(&offer.id())
                .cloned()
                .unwrap_or_default();
            let Some(mime) = TEXT_TARGETS
                .iter()
                .find(|wanted| mimes.iter().any(|mime| mime == *wanted))
            else {
                return Ok(None);
            };
            let (read, write) = pipe()?;
            offer.receive((*mime).to_owned(), write.as_fd());
            drop(write);
            session.connection.flush()?;
            let data = read_bounded(read, TEXT_CAP, deadline)
                .wrap_err("could not read the clipboard's text")?;
            Ok(String::from_utf8(data).ok())
        }

        /// Takes the selection with `formats`, or clears it when there are none,
        /// and serves it on a thread until another client replaces it.
        pub(super) fn serve(formats: Vec<(String, Vec<u8>)>) -> Result<()> {
            let mut session = connect()?;
            if formats.is_empty() {
                session.device.set_selection(None);
                session.queue.roundtrip(&mut session.state)?;
                session.device.destroy();
                return Ok(());
            }
            let source = session
                .manager
                .create_data_source(&session.queue.handle(), ());
            for (mime, _) in &formats {
                source.offer(mime.clone());
            }
            session.device.set_selection(Some(&source));
            session.state.served = formats;
            session.queue.roundtrip(&mut session.state)?;
            thread::Builder::new()
                .name("hex-clipboard-restore".into())
                .spawn(move || {
                    while !session.state.done {
                        if session.queue.blocking_dispatch(&mut session.state).is_err() {
                            break;
                        }
                    }
                    source.destroy();
                    session.device.destroy();
                    let _ = session.connection.flush();
                })?;
            Ok(())
        }

        impl Dispatch<WlRegistry, GlobalListContents> for State {
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

        impl Dispatch<WlSeat, ()> for State {
            fn event(
                _: &mut Self,
                _: &WlSeat,
                _: wl_seat::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<Manager, ()> for State {
            fn event(
                _: &mut Self,
                _: &Manager,
                _: manager::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }

        impl Dispatch<Device, ()> for State {
            fn event(
                state: &mut Self,
                _: &Device,
                event: device::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    device::Event::DataOffer { id } => {
                        state.offers.insert(id.id(), Vec::new());
                    }
                    device::Event::Selection { id } => {
                        if let Some(Some(previous)) = state.selection.take()
                            && Some(&previous) != id.as_ref()
                        {
                            state.offers.remove(&previous.id());
                            previous.destroy();
                        }
                        state.selection = Some(id);
                    }
                    device::Event::Finished => state.done = true,
                    _ => {}
                }
            }

            event_created_child!(State, Device, [
                device::EVT_DATA_OFFER_OPCODE => (Offer, ()),
            ]);
        }

        impl Dispatch<Offer, ()> for State {
            fn event(
                state: &mut Self,
                offer: &Offer,
                event: offer::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let offer::Event::Offer { mime_type } = event {
                    state.offers.entry(offer.id()).or_default().push(mime_type);
                }
            }
        }

        impl Dispatch<Source, ()> for State {
            fn event(
                state: &mut Self,
                _: &Source,
                event: source::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    source::Event::Send { mime_type, fd } => {
                        if let Some((_, data)) =
                            state.served.iter().find(|(mime, _)| *mime == mime_type)
                            && let Err(error) = write_bounded(fd, data, Instant::now() + SERVE_DEADLINE)
                        {
                            tracing::debug!(
                                error = format!("{error:#}"),
                                mime_type,
                                "could not serve the restored clipboard"
                            );
                        }
                    }
                    source::Event::Cancelled => state.done = true,
                    _ => {}
                }
            }
        }
    };
}

/// Wayland reads and serves the clipboard through `ext-data-control` (KWin,
/// #719) or wlroots' `wlr-data-control`, whichever the compositor offers.
mod wayland {
    use super::{Result, Snapshot, eyre};

    pub(super) fn snapshot(only: Option<&str>) -> Result<Snapshot> {
        if offers(ext::GLOBAL) {
            ext::snapshot(only)
        } else if offers(wlr::GLOBAL) {
            wlr::snapshot(only)
        } else {
            Err(missing())
        }
    }

    pub(super) fn text() -> Result<Option<String>> {
        if offers(ext::GLOBAL) {
            ext::text()
        } else if offers(wlr::GLOBAL) {
            wlr::text()
        } else {
            super::wl_paste_text()
        }
    }

    /// Whether the compositor offers either data-control protocol.
    pub(super) fn offers_data_control() -> bool {
        offers(ext::GLOBAL) || offers(wlr::GLOBAL)
    }

    pub(super) fn serve(formats: Vec<(String, Vec<u8>)>) -> Result<()> {
        if offers(ext::GLOBAL) {
            ext::serve(formats)
        } else if offers(wlr::GLOBAL) {
            wlr::serve(formats)
        } else {
            Err(missing())
        }
    }

    fn offers(global: &str) -> bool {
        crate::linux::paste::wayland_offers(global)
    }

    fn missing() -> color_eyre::eyre::Report {
        eyre!(
            "the compositor offers no ext-data-control or wlr-data-control clipboard access; Labs › Restore clipboard on GNOME restores it through XWayland while XWayland runs"
        )
    }

    mod ext {
        pub(super) const GLOBAL: &str = "ext_data_control_manager_v1";
        use device::ExtDataControlDeviceV1 as Device;
        use manager::ExtDataControlManagerV1 as Manager;
        use offer::ExtDataControlOfferV1 as Offer;
        use source::ExtDataControlSourceV1 as Source;
        use wayland_protocols::ext::data_control::v1::client::{
            ext_data_control_device_v1 as device, ext_data_control_manager_v1 as manager,
            ext_data_control_offer_v1 as offer, ext_data_control_source_v1 as source,
        };

        use super::super::*;

        data_control!("ext-data-control");
    }

    mod wlr {
        pub(super) const GLOBAL: &str = "zwlr_data_control_manager_v1";
        use device::ZwlrDataControlDeviceV1 as Device;
        use manager::ZwlrDataControlManagerV1 as Manager;
        use offer::ZwlrDataControlOfferV1 as Offer;
        use source::ZwlrDataControlSourceV1 as Source;
        use wayland_protocols_wlr::data_control::v1::client::{
            zwlr_data_control_device_v1 as device, zwlr_data_control_manager_v1 as manager,
            zwlr_data_control_offer_v1 as offer, zwlr_data_control_source_v1 as source,
        };

        use super::super::*;

        data_control!("wlr-data-control");
    }
}

mod x11 {
    use std::collections::HashMap;

    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{
        AtomEnum, ChangeWindowAttributesAux, ConnectionExt, CreateWindowAux, EventMask, PropMode,
        Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, Window, WindowClass,
    };
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

    use super::*;

    x11rb::atom_manager! {
        Atoms: AtomsCookie {
            CLIPBOARD,
            TARGETS,
            MULTIPLE,
            TIMESTAMP,
            SAVE_TARGETS,
            DELETE,
            INCR,
            HEX_CLIPBOARD,
        }
    }

    fn connect() -> Result<(RustConnection, usize, Atoms)> {
        let (connection, screen) =
            x11rb::connect(None).wrap_err("could not connect to the X server")?;
        let atoms = Atoms::new(&connection)?.reply()?;
        Ok((connection, screen, atoms))
    }

    fn window(connection: &RustConnection, screen: usize) -> Result<Window> {
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id()?;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?
            .check()?;
        Ok(window)
    }

    pub(super) fn owner() -> Result<u32> {
        let (connection, _, atoms) = connect()?;
        Ok(connection
            .get_selection_owner(atoms.CLIPBOARD)?
            .reply()?
            .owner)
    }

    /// Whether the clipboard's `target` (a MIME type or X11 target name)
    /// holds exactly `bytes`, reading no more than that.
    pub(super) fn holds(target: &str, bytes: &[u8]) -> bool {
        let read = || -> Result<bool> {
            let deadline = Instant::now() + READ_DEADLINE;
            let (connection, screen, atoms) = connect()?;
            let target = connection
                .intern_atom(false, target.as_bytes())?
                .reply()?
                .atom;
            let window = window(&connection, screen)?;
            let format = convert(&connection, &atoms, window, target, bytes.len(), deadline);
            let _ = connection.destroy_window(window);
            let _ = connection.flush();
            Ok(format?.is_some_and(|format| format.data == bytes))
        };
        read().unwrap_or(false)
    }

    fn next_event(connection: &RustConnection, deadline: Instant) -> Result<Event> {
        loop {
            if let Some(event) = connection.poll_for_event()? {
                return Ok(event);
            }
            if Instant::now() >= deadline {
                return Err(eyre!("the clipboard owner did not answer in time"));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    pub(super) fn snapshot() -> Result<Snapshot> {
        let deadline = Instant::now() + READ_DEADLINE;
        let (connection, screen, atoms) = connect()?;
        let owner = connection
            .get_selection_owner(atoms.CLIPBOARD)?
            .reply()?
            .owner;
        if owner == NONE {
            return Ok(Snapshot::Empty);
        }
        let mut requestor = window(&connection, screen)?;
        let result = (|| {
            let Some(targets) =
                convert(&connection, &atoms, requestor, atoms.TARGETS, CAP, deadline)?
            else {
                return Err(eyre!("the clipboard owner offers no readable targets"));
            };
            let skipped = [
                atoms.TARGETS,
                atoms.MULTIPLE,
                atoms.TIMESTAMP,
                atoms.SAVE_TARGETS,
                atoms.DELETE,
            ];
            let targets: Vec<u32> = targets
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_ne_bytes(*bytes))
                .filter(|target| !skipped.contains(target))
                .collect();
            // Ask for every name before reading any reply, so the whole list
            // costs one flush instead of one round trip per target.
            let cookies = targets
                .iter()
                .map(|target| connection.get_atom_name(*target))
                .collect::<Result<Vec<_>, _>>()?;
            let mut wanted = Vec::with_capacity(targets.len());
            for (target, cookie) in targets.iter().zip(cookies) {
                let name = cookie.reply()?.name;
                wanted.push((*target, String::from_utf8_lossy(&name).into_owned()));
            }
            // A conversion that failed partway through an INCR transfer
            // leaves the owner still writing to this window's property. A new
            // requestor window before the next target keeps the abandoned
            // chunks, and the MIME they belong to, out of the next read.
            let mut stale = false;
            let formats: Vec<X11Format> =
                collect_formats(wanted, CAP, deadline, |target, name, budget| {
                    if stale {
                        let _ = connection.destroy_window(requestor);
                        requestor = window(&connection, screen)?;
                        stale = false;
                    }
                    let read = convert(&connection, &atoms, requestor, target, budget, deadline)
                        .and_then(|format| {
                            format.ok_or_else(|| eyre!("the clipboard owner does not offer {name}"))
                        });
                    stale = read.is_err();
                    read
                })?
                .into_iter()
                .map(|(_, format)| format)
                .collect();
            // A selection that changed during the read handed over formats
            // from two different clipboards. This compares the selection's
            // owner window, so it does not see the same client replacing its
            // own contents mid-read.
            if connection
                .get_selection_owner(atoms.CLIPBOARD)?
                .reply()?
                .owner
                != owner
            {
                return Err(eyre!("the clipboard changed while it was being read"));
            }
            Ok(if formats.is_empty() {
                Snapshot::Empty
            } else {
                Snapshot::X11(formats)
            })
        })();
        let _ = connection.destroy_window(requestor);
        let _ = connection.flush();
        result
    }

    /// The clipboard's plain text, within [`TEXT_CAP`] and the read
    /// deadline; `None` when it holds no text.
    pub(super) fn text() -> Result<Option<String>> {
        let deadline = Instant::now() + READ_DEADLINE;
        let (connection, screen, atoms) = connect()?;
        if connection
            .get_selection_owner(atoms.CLIPBOARD)?
            .reply()?
            .owner
            == NONE
        {
            return Ok(None);
        }
        let window = window(&connection, screen)?;
        let result = (|| {
            for target in TEXT_TARGETS {
                let target = connection
                    .intern_atom(false, target.as_bytes())?
                    .reply()?
                    .atom;
                let Some(format) =
                    convert(&connection, &atoms, window, target, TEXT_CAP, deadline)?
                else {
                    continue;
                };
                let latin1 = target == connection.intern_atom(false, b"STRING")?.reply()?.atom;
                if let Some(text) = decode_text(format.data, latin1) {
                    return Ok(Some(text));
                }
            }
            Ok(None)
        })();
        let _ = connection.destroy_window(window);
        let _ = connection.flush();
        result
    }

    /// Clipboard bytes as text: UTF-8, or Latin-1 for the legacy `STRING`
    /// target.
    fn decode_text(data: Vec<u8>, latin1: bool) -> Option<String> {
        match String::from_utf8(data) {
            Ok(text) => Some(text),
            Err(error) if latin1 => Some(error.into_bytes().into_iter().map(char::from).collect()),
            Err(_) => None,
        }
    }

    /// One target of the clipboard, following INCR transfers.
    fn convert(
        connection: &RustConnection,
        atoms: &Atoms,
        requestor: Window,
        target: u32,
        budget: usize,
        deadline: Instant,
    ) -> Result<Option<X11Format>> {
        let property = atoms.HEX_CLIPBOARD;
        connection.delete_property(requestor, property)?;
        connection.convert_selection(requestor, atoms.CLIPBOARD, target, property, CURRENT_TIME)?;
        connection.flush()?;
        let notify = loop {
            // Another request's notify must not answer this one; a property
            // of NONE is the owner refusing this target.
            if let Event::SelectionNotify(notify) = next_event(connection, deadline)?
                && notify.requestor == requestor
                && notify.target == target
                && (notify.property == property || notify.property == NONE)
            {
                break notify;
            }
        };
        if notify.property == NONE {
            return Ok(None);
        }
        let reply = connection
            .get_property(true, requestor, property, AtomEnum::ANY, 0, u32::MAX)?
            .reply()?;
        if reply.type_ != atoms.INCR {
            if reply.value.len() > budget {
                return Err(too_large());
            }
            return Ok(Some(X11Format {
                target,
                type_: reply.type_,
                format: reply.format,
                data: reply.value,
            }));
        }
        connection.flush()?;
        let mut format = X11Format {
            target,
            type_: NONE,
            format: 8,
            data: Vec::new(),
        };
        loop {
            match next_event(connection, deadline)? {
                Event::PropertyNotify(event)
                    if event.window == requestor
                        && event.atom == property
                        && event.state == Property::NEW_VALUE =>
                {
                    let chunk = connection
                        .get_property(true, requestor, property, AtomEnum::ANY, 0, u32::MAX)?
                        .reply()?;
                    connection.flush()?;
                    if chunk.value.is_empty() {
                        return Ok(Some(format));
                    }
                    if format.data.len() + chunk.value.len() > budget {
                        return Err(too_large());
                    }
                    format.type_ = chunk.type_;
                    format.format = chunk.format;
                    format.data.extend_from_slice(&chunk.value);
                }
                _ => {}
            }
        }
    }

    /// Takes the clipboard with `formats`, or clears it when there are none,
    /// and serves it on a thread until another client takes it.
    pub(super) fn serve(formats: Vec<X11Format>) -> Result<()> {
        let (connection, screen, atoms) = connect()?;
        if formats.is_empty() {
            connection
                .set_selection_owner(NONE, atoms.CLIPBOARD, CURRENT_TIME)?
                .check()?;
            return Ok(());
        }
        let window = window(&connection, screen)?;
        connection
            .set_selection_owner(window, atoms.CLIPBOARD, CURRENT_TIME)?
            .check()?;
        if connection
            .get_selection_owner(atoms.CLIPBOARD)?
            .reply()?
            .owner
            != window
        {
            return Err(eyre!("could not own the X11 clipboard"));
        }
        thread::Builder::new()
            .name("hex-clipboard-restore".into())
            .spawn(move || serve_until_cleared(&connection, &atoms, window, &formats))?;
        Ok(())
    }

    fn serve_until_cleared(
        connection: &RustConnection,
        atoms: &Atoms,
        window: Window,
        formats: &[X11Format],
    ) {
        // Whole units of every format, well inside the request limit.
        let chunk = (connection.maximum_request_bytes() / 2).min(256 * 1024) & !3;
        // INCR transfers by requestor window and property: format and offset.
        let mut transfers = HashMap::<(Window, u32), (usize, usize)>::new();
        while let Ok(event) = connection.wait_for_event() {
            match event {
                Event::SelectionRequest(request) => {
                    // Obsolete clients name no property; use the target.
                    let property = if request.property == NONE {
                        request.target
                    } else {
                        request.property
                    };
                    let served = if request.target == atoms.TARGETS {
                        let targets: Vec<u32> = std::iter::once(atoms.TARGETS)
                            .chain(formats.iter().map(|format| format.target))
                            .collect();
                        connection
                            .change_property32(
                                PropMode::REPLACE,
                                request.requestor,
                                property,
                                AtomEnum::ATOM,
                                &targets,
                            )
                            .is_ok()
                    } else if let Some(index) = formats
                        .iter()
                        .position(|format| format.target == request.target)
                    {
                        let format = &formats[index];
                        if format.data.len() <= chunk {
                            put(
                                connection,
                                request.requestor,
                                property,
                                format,
                                &format.data,
                            )
                        } else {
                            let _ = connection.change_window_attributes(
                                request.requestor,
                                &ChangeWindowAttributesAux::new()
                                    .event_mask(EventMask::PROPERTY_CHANGE),
                            );
                            transfers.insert((request.requestor, property), (index, 0));
                            connection
                                .change_property32(
                                    PropMode::REPLACE,
                                    request.requestor,
                                    property,
                                    atoms.INCR,
                                    &[format.data.len() as u32],
                                )
                                .is_ok()
                        }
                    } else {
                        false
                    };
                    let _ = connection.send_event(
                        false,
                        request.requestor,
                        EventMask::default(),
                        SelectionNotifyEvent {
                            response_type: SELECTION_NOTIFY_EVENT,
                            sequence: 0,
                            time: request.time,
                            requestor: request.requestor,
                            selection: request.selection,
                            target: request.target,
                            property: if served { property } else { NONE },
                        },
                    );
                }
                Event::PropertyNotify(event) if event.state == Property::DELETE => {
                    let key = (event.window, event.atom);
                    if let Some((index, offset)) = transfers.get_mut(&key) {
                        let format = &formats[*index];
                        let end = (*offset + chunk).min(format.data.len());
                        put(
                            connection,
                            event.window,
                            event.atom,
                            format,
                            &format.data[*offset..end],
                        );
                        if *offset == end {
                            transfers.remove(&key);
                        } else {
                            *offset = end;
                        }
                    }
                }
                Event::SelectionClear(_) => break,
                _ => {}
            }
            let _ = connection.flush();
        }
        let _ = connection.destroy_window(window);
        let _ = connection.flush();
    }

    fn put(
        connection: &RustConnection,
        window: Window,
        property: u32,
        format: &X11Format,
        data: &[u8],
    ) -> bool {
        let unit = usize::from(format.format / 8).max(1);
        connection
            .change_property(
                PropMode::REPLACE,
                window,
                property,
                format.type_,
                format.format,
                (data.len() / unit) as u32,
                data,
            )
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_stops_at_the_cap() {
        let (read, write) = pipe().unwrap();
        let writer = thread::spawn(move || {
            let mut file = std::fs::File::from(write);
            let _ = file.write_all(&[7; 4096]);
        });
        let result = read_bounded(read, 1024, Instant::now() + Duration::from_secs(2));
        writer.join().unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn reading_returns_everything_within_the_cap() {
        let (read, write) = pipe().unwrap();
        let writer = thread::spawn(move || {
            std::fs::File::from(write).write_all(b"original").unwrap();
        });
        let data = read_bounded(read, 1024, Instant::now() + Duration::from_secs(2)).unwrap();
        writer.join().unwrap();
        assert_eq!(data, b"original");
    }

    fn offered(names: &[&str]) -> Vec<(usize, String)> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| (index, (*name).to_owned()))
            .collect()
    }

    fn names_of(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    fn kept(formats: Vec<(String, Vec<u8>)>) -> Vec<String> {
        formats.into_iter().map(|(mime, _)| mime).collect()
    }

    /// A format that fails is the only one lost, and what remains keeps the
    /// offer's order rather than the order it was read in.
    #[test]
    fn an_unreadable_format_is_skipped_and_the_rest_keep_the_offer_order() {
        let formats = collect_formats(
            offered(&[
                "image/jxl",
                "text/plain;charset=utf-8",
                "image/heic",
                "image/png",
            ]),
            CAP,
            deadline(),
            |_, mime, _| match mime {
                "image/jxl" | "image/heic" => Err(eyre!("timed out")),
                _ => Ok(mime.as_bytes().to_vec()),
            },
        )
        .unwrap();
        assert_eq!(
            kept(formats),
            names_of(&["text/plain;charset=utf-8", "image/png"])
        );
        let formats = collect_formats(
            offered(&[
                "image/jxl",
                "text/plain;charset=utf-8",
                "image/heic",
                "image/png",
            ]),
            CAP,
            deadline(),
            |_, mime, _| match mime {
                "text/plain;charset=utf-8" => Err(eyre!("timed out")),
                _ => Ok(mime.as_bytes().to_vec()),
            },
        )
        .unwrap();
        assert_eq!(
            kept(formats),
            names_of(&["image/jxl", "image/heic", "image/png"])
        );
    }

    /// Nothing usable must never look like an empty clipboard: that snapshot
    /// clears the clipboard instead of leaving the transcript.
    #[test]
    fn a_clipboard_with_no_readable_format_is_an_error() {
        let error = collect_formats(
            offered(&["image/jxl", "image/heic"]),
            CAP,
            deadline(),
            |_, _, _| Err::<Vec<u8>, _>(eyre!("timed out")),
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("could be read"), "{message}");
        assert!(message.contains("image/jxl"), "{message}");
        assert!(message.contains("image/heic"), "{message}");
    }

    #[test]
    fn an_offer_with_no_formats_is_empty() {
        let formats = collect_formats(
            Vec::new(),
            CAP,
            deadline(),
            |_: usize, _, _| -> Result<Vec<u8>> { panic!("an empty offer reads nothing") },
        )
        .unwrap();
        assert!(formats.is_empty());
    }

    /// Once the shared deadline has passed the rest are skipped, never read.
    #[test]
    fn formats_after_the_deadline_are_skipped_without_a_read() {
        let mut read = Vec::new();
        let error = collect_formats(
            offered(&["text/plain", "image/png"]),
            CAP,
            Instant::now(),
            |_, mime, _| {
                read.push(mime.to_owned());
                Ok(Vec::new())
            },
        )
        .unwrap_err();
        assert!(read.is_empty(), "read after the deadline: {read:?}");
        let message = format!("{error:#}");
        assert!(message.contains("could be read"), "{message}");
        assert!(message.contains("text/plain"), "{message}");
    }

    /// Text and PNG are read before a slow exotic format can spend the shared
    /// deadline; the result is still in offer order.
    #[test]
    fn text_and_png_are_read_first_and_offered_back_in_offer_order() {
        let mut read = Vec::new();
        let formats = collect_formats(
            offered(&[
                "image/jxl",
                "text/html",
                "image/heic",
                "image/png",
                "text/plain",
                "UTF8_STRING",
            ]),
            CAP,
            deadline(),
            |_, mime, _| {
                read.push(mime.to_owned());
                Ok(Vec::new())
            },
        )
        .unwrap();
        assert_eq!(
            read,
            names_of(&[
                "text/plain",
                "UTF8_STRING",
                "image/png",
                "image/jxl",
                "text/html",
                "image/heic",
            ])
        );
        assert_eq!(
            kept(formats),
            names_of(&[
                "image/jxl",
                "text/html",
                "image/heic",
                "image/png",
                "text/plain",
                "UTF8_STRING",
            ])
        );
    }

    /// A format over the remaining cap loses only itself, and an empty one is
    /// real data, not a failed read.
    #[test]
    fn a_format_over_the_remaining_cap_is_skipped_and_an_empty_one_is_kept() {
        let formats = collect_formats(
            offered(&["text/plain", "image/png", "image/heic"]),
            8,
            deadline(),
            |_, mime, budget| {
                Ok::<_, color_eyre::eyre::Report>(match mime {
                    "text/plain" => b"1234".to_vec(),
                    "image/png" => vec![0; budget + 1],
                    _ => Vec::new(),
                })
            },
        )
        .unwrap();
        assert_eq!(kept(formats), names_of(&["text/plain", "image/heic"]));
    }

    #[test]
    fn xwayland_restores_only_by_opt_in_without_data_control() {
        let running = || true;
        assert!(uses_xwayland(false, true, running));
        assert!(!uses_xwayland(true, true, running), "data-control wins");
        assert!(!uses_xwayland(false, false, running), "off by default");
        assert!(
            !uses_xwayland(false, true, || false),
            "never starts XWayland"
        );
    }

    #[test]
    fn xwayland_is_looked_for_only_when_it_could_be_used() {
        let looked = std::cell::Cell::new(false);
        let running = || {
            looked.set(true);
            true
        };
        assert!(!uses_xwayland(true, true, running));
        assert!(!uses_xwayland(false, false, running));
        assert!(!looked.get());
    }

    #[test]
    fn xwayland_counts_only_this_users_process() {
        let processes = || {
            [
                (1000, "gnome-shell\n".to_owned()),
                (1001, "Xwayland\n".to_owned()),
            ]
            .into_iter()
        };
        assert!(!xwayland_listed(1000, processes()));
        assert!(xwayland_listed(1001, processes()));
        assert!(!xwayland_listed(
            1000,
            std::iter::once((1000, "Xwayland-helper\n".to_owned()))
        ));
    }

    #[test]
    fn a_silent_owner_times_out() {
        let (read, _write) = pipe().unwrap();
        let start = Instant::now();
        assert!(read_bounded(read, 1024, Instant::now() + Duration::from_millis(100)).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
