//! Labs' Live typing on Wayland: a dictation's provisional words sit in the
//! focused text field as an input-method preedit, underlined and replaced on
//! each update, and its final text is committed through the input method
//! instead of pasted.
//!
//! One owner thread holds a private Wayland connection. A capture's Begin
//! creates `zwp_input_method_v2` for the seat, and Finish or Cancel destroys
//! it, so Hex never holds the seat's input method while idle and another
//! input method keeps working. Everything is bounded: capture sends never
//! wait, preedits coalesce, and a Finish uses one shared claim so a caller
//! that gives up never pastes over a commit the owner already owns. Control
//! commands are capped at [`CONTROL_QUEUE_LIMIT`] outstanding; past that the
//! owner is treated as inert. Any failure is inert and silent apart from one
//! log line.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};

use self::protocol::zwp_input_method_manager_v2::{Event as ManagerEvent, ZwpInputMethodManagerV2};
use self::protocol::zwp_input_method_v2::{Event as InputMethodEvent, ZwpInputMethodV2};

mod protocol {
    //! `input-method-unstable-v2.xml`, vendored from wayland-protocols-misc
    //! (MIT, see the file's copyright header).

    #![allow(
        dead_code,
        non_camel_case_types,
        non_upper_case_globals,
        non_snake_case,
        unused_imports,
        unused_variables
    )]

    // The generated interfaces reach this through `super::wayland_client`.
    #[allow(clippy::single_component_path_imports)]
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::wp::text_input::zv3::client::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::wp::text_input::zv3::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("src/linux/protocols/input-method-unstable-v2.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("src/linux/protocols/input-method-unstable-v2.xml");
}

/// The largest string the protocol carries: a Wayland message's text is
/// limited to 4000 bytes.
const MAX_TEXT: usize = 4_000;
/// How long Finish waits for the owner's decision.
const FINISH_TIMEOUT: Duration = Duration::from_millis(350);
/// How long a caller waits once the owner has won the commit, so it never
/// pastes over it.
const COMMIT_WAIT: Duration = Duration::from_millis(750);
/// How long a caller that won the abandonment waits for the preedit to clear
/// before pasting.
const CLEAR_WAIT: Duration = Duration::from_millis(250);
/// Control commands allowed while the owner has not taken one. Past this the
/// owner is treated as inert rather than growing memory unboundedly.
const CONTROL_QUEUE_LIMIT: usize = 64;

/// A Finish's hand-off: whoever changes Pending first decides it.
const PENDING: u8 = 0;
const COMMITTING: u8 = 1;
const ABANDONED: u8 = 2;

/// One capture's live-typing session.
pub(crate) struct LiveTyping {
    /// Control commands only; preedits use `preedit` and a wake.
    commands: Sender<Command>,
    /// The newest preedit, replaced by each change.
    preedit: Arc<Mutex<Option<(u64, String)>>>,
    /// Whether a Wake is already queued for the preedit slot.
    wake: Arc<AtomicBool>,
    /// Whether the owner thread's channel is open. Only closing it stops a
    /// session that already began from being finished or cancelled.
    connected: Arc<AtomicBool>,
    /// Whether new sessions and preedits may start. An abandoned Finish or a
    /// full control queue turns this off without stranding begun sessions.
    accepting: Arc<AtomicBool>,
    /// Begin and Wake commands sent but not yet received by the owner.
    queued: Arc<AtomicUsize>,
}

enum Command {
    Begin(u64),
    Finish {
        id: u64,
        text: String,
        claim: Arc<AtomicU8>,
        reply: SyncSender<bool>,
    },
    Cancel(u64),
    Wake,
}

/// One capture's hold on the input method. It starts at the hold threshold
/// (or the Toggle press), and dropping it without `finish` clears the
/// preedit: every failed job and dropped queue entry ends here.
pub(crate) struct Session {
    live: Arc<LiveTyping>,
    id: u64,
    started: bool,
    committed: bool,
}

impl Session {
    pub(crate) fn new(live: Arc<LiveTyping>, id: u64) -> Self {
        Self {
            live,
            id,
            started: false,
            committed: false,
        }
    }

    /// Begins the session. Callers start it at the hold threshold or the
    /// Toggle press, so short taps and chords never create an input method.
    pub(crate) fn start(&mut self) {
        if !self.started {
            self.started = true;
            self.live.begin(self.id);
        }
    }

    pub(crate) fn preedit(&self, stable: &str, tentative: &str) {
        if self.started && !self.committed {
            self.live.preedit(self.id, stable, tentative);
        }
    }

    /// Commits `text` through the input method. The guard is consumed so
    /// its drop never cancels a finished session.
    pub(crate) fn finish(mut self, text: &str) -> bool {
        self.committed = true;
        self.started && self.live.finish(self.id, text)
    }

    /// Clears the preedit now, when the caller needs it gone before a paste.
    pub(crate) fn cancel(mut self) {
        self.committed = true;
        if self.started {
            self.live.cancel(self.id);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.committed && self.started {
            self.live.cancel(self.id);
        }
    }
}

impl LiveTyping {
    /// Starts the owner thread without connecting: the connection opens when
    /// the thread starts, and both flags turn false if it cannot.
    pub(crate) fn start() -> Arc<Self> {
        let (commands, receiver) = mpsc::channel();
        let preedit = Arc::new(Mutex::new(None));
        let wake = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(true));
        let accepting = Arc::new(AtomicBool::new(true));
        let queued = Arc::new(AtomicUsize::new(0));
        let thread_preedit = preedit.clone();
        let thread_wake = wake.clone();
        let thread_connected = connected.clone();
        let thread_accepting = accepting.clone();
        let thread_queued = queued.clone();
        if let Err(error) = thread::Builder::new()
            .name("hex-live-typing".into())
            .spawn(move || {
                run(
                    receiver,
                    thread_preedit,
                    thread_wake,
                    thread_connected,
                    thread_accepting,
                    thread_queued,
                )
            })
        {
            tracing::info!(%error, "live typing is unavailable; dictations paste normally");
            connected.store(false, Ordering::Release);
            accepting.store(false, Ordering::Release);
        }
        Arc::new(Self {
            commands,
            preedit,
            wake,
            connected,
            accepting,
            queued,
        })
    }

    /// Whether new captures may start Live typing. An existing session
    /// still finishes and cancels while only this is off.
    pub(crate) fn is_accepting(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub(crate) fn begin(&self, id: u64) {
        if self.enqueue_new(Command::Begin(id)) {
            *preedit_lock(&self.preedit) = None;
        }
    }

    pub(crate) fn preedit(&self, id: u64, stable: &str, tentative: &str) {
        if !self.is_accepting() {
            return;
        }
        let Some(text) = preedit_text(stable, tentative) else {
            return;
        };
        *preedit_lock(&self.preedit) = Some((id, text));
        if !self.wake.swap(true, Ordering::AcqRel) {
            self.enqueue_new(Command::Wake);
        }
    }

    pub(crate) fn cancel(&self, id: u64) {
        clear_slot(&self.preedit, id);
        self.send_control(Command::Cancel(id));
    }

    /// Commits `text` through the input method when its session is active,
    /// and says whether it did. False means the caller pastes as usual.
    pub(crate) fn finish(&self, id: u64, text: &str) -> bool {
        clear_slot(&self.preedit, id);
        let claim = Arc::new(AtomicU8::new(PENDING));
        let (reply, answer) = mpsc::sync_channel(1);
        if !self.send_control(Command::Finish {
            id,
            text: text.to_owned(),
            claim: claim.clone(),
            reply,
        }) {
            return false;
        }
        match answer.recv_timeout(FINISH_TIMEOUT) {
            Ok(inserted) => inserted,
            Err(RecvTimeoutError::Disconnected) => false,
            Err(RecvTimeoutError::Timeout) => {
                if caller_abandons(&claim) {
                    // The owner did not decide in time: stop starting new
                    // sessions, and wait briefly so the preedit is gone
                    // before this caller pastes. A session that already
                    // began still cancels and finishes below.
                    self.accepting.store(false, Ordering::Release);
                    answer.recv_timeout(CLEAR_WAIT).unwrap_or(false)
                } else {
                    // The owner owns the commit: never paste over it.
                    answer.recv_timeout(COMMIT_WAIT).unwrap_or(true)
                }
            }
        }
    }

    /// Sends a command for a session that already began. The channel's
    /// closure is the only gate, so an abandoned or full queue never strands
    /// a shown preedit.
    fn send_control(&self, command: Command) -> bool {
        if !self.is_connected() {
            return false;
        }
        if self.commands.send(command).is_err() {
            self.connected.store(false, Ordering::Release);
            self.accepting.store(false, Ordering::Release);
            return false;
        }
        true
    }

    /// Sends a Begin or Wake: new work, gated by `accepting` and the queue
    /// limit.
    fn enqueue_new(&self, command: Command) -> bool {
        if !self.is_accepting() || !self.is_connected() {
            return false;
        }
        if self.queued.fetch_add(1, Ordering::AcqRel) >= CONTROL_QUEUE_LIMIT {
            self.queued.fetch_sub(1, Ordering::AcqRel);
            self.accepting.store(false, Ordering::Release);
            return false;
        }
        if self.commands.send(command).is_err() {
            self.queued.fetch_sub(1, Ordering::AcqRel);
            self.connected.store(false, Ordering::Release);
            self.accepting.store(false, Ordering::Release);
            return false;
        }
        true
    }

    #[cfg(test)]
    fn for_test() -> (Arc<Self>, Receiver<Command>) {
        let (commands, receiver) = mpsc::channel();
        (
            Arc::new(Self {
                commands,
                preedit: Arc::new(Mutex::new(None)),
                wake: Arc::new(AtomicBool::new(false)),
                connected: Arc::new(AtomicBool::new(true)),
                accepting: Arc::new(AtomicBool::new(true)),
                queued: Arc::new(AtomicUsize::new(0)),
            }),
            receiver,
        )
    }
}

fn run(
    commands: Receiver<Command>,
    preedit: Arc<Mutex<Option<(u64, String)>>>,
    wake: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    accepting: Arc<AtomicBool>,
    queued: Arc<AtomicUsize>,
) {
    let _alive = Alive(connected, accepting);
    let mut owner = match Owner::connect() {
        Ok(owner) => owner,
        Err(error) => {
            tracing::info!(%error, "live typing is unavailable; dictations paste normally");
            return;
        }
    };
    while let Ok(command) = commands.recv() {
        match command {
            Command::Begin(id) => {
                queued.fetch_sub(1, Ordering::AcqRel);
                owner.begin(id);
            }
            Command::Finish {
                id,
                text,
                claim,
                reply,
            } => owner.finish(id, &text, &claim, &reply),
            Command::Cancel(id) => owner.cancel(id),
            Command::Wake => {
                queued.fetch_sub(1, Ordering::AcqRel);
                wake.store(false, Ordering::Release);
                if let Some((id, text)) = take_preedit(&preedit) {
                    owner.preedit(id, &text);
                }
            }
        }
        if owner.broken {
            // `Alive` reports false once the thread ends too.
            return;
        }
    }
    owner.shutdown();
}

/// Marks the owner unavailable however the thread ends, panics included.
struct Alive(Arc<AtomicBool>, Arc<AtomicBool>);

impl Drop for Alive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
        self.1.store(false, Ordering::Release);
    }
}

fn preedit_lock(slot: &Mutex<Option<(u64, String)>>) -> MutexGuard<'_, Option<(u64, String)>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

fn take_preedit(slot: &Mutex<Option<(u64, String)>>) -> Option<(u64, String)> {
    preedit_lock(slot).take()
}

fn clear_slot(slot: &Mutex<Option<(u64, String)>>, id: u64) {
    let mut slot = preedit_lock(slot);
    if slot.as_ref().is_some_and(|(slot_id, _)| *slot_id == id) {
        *slot = None;
    }
}

/// The owner wins a commit before queueing any request.
fn owner_claims(claim: &AtomicU8) -> bool {
    claim
        .compare_exchange(PENDING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// A waiting caller gives up; false when the owner already owns the commit.
fn caller_abandons(claim: &AtomicU8) -> bool {
    claim
        .compare_exchange(PENDING, ABANDONED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

/// The compositor's double-buffered state, applied on `done`.
#[derive(Default)]
struct Live {
    active: bool,
    pending_active: Option<bool>,
    /// The number of `done` events seen; the serial `commit` takes.
    done: u32,
    unavailable: bool,
}

impl Live {
    /// A done event applies the pending activate or deactivate and becomes
    /// the next commit serial.
    fn done(&mut self) {
        self.done = self.done.wrapping_add(1);
        if let Some(active) = self.pending_active.take() {
            self.active = active;
        }
    }
}

struct Bound {
    id: u64,
    method: ZwpInputMethodV2,
    /// Whether this capture set a preedit that is still on screen.
    shown: bool,
}

struct Owner {
    connection: Connection,
    queue: EventQueue<Live>,
    seat: wl_seat::WlSeat,
    manager: ZwpInputMethodManagerV2,
    live: Live,
    session: Option<Bound>,
    broken: bool,
}

impl Owner {
    fn connect() -> Result<Self> {
        let connection =
            Connection::connect_to_env().wrap_err("there is no Wayland display to reach")?;
        let (globals, queue) = registry_queue_init::<Live>(&connection)
            .wrap_err("the Wayland registry could not be read")?;
        let handle = queue.handle();
        let seat: wl_seat::WlSeat = globals
            .bind(&handle, 1..=1, ())
            .wrap_err("this Wayland seat is unavailable")?;
        let manager: ZwpInputMethodManagerV2 = globals
            .bind(&handle, 1..=1, ())
            .wrap_err("this compositor offers no zwp_input_method_manager_v2")?;
        Ok(Self {
            connection,
            queue,
            seat,
            manager,
            live: Live::default(),
            session: None,
            broken: false,
        })
    }

    fn begin(&mut self, id: u64) {
        if self.broken {
            return;
        }
        self.sync();
        self.clear_preedit();
        self.end();
        let method = self
            .manager
            .get_input_method(&self.seat, &self.queue.handle(), ());
        self.session = Some(Bound {
            id,
            method,
            shown: false,
        });
        // The compositor answers with activate or deactivate (or unavailable),
        // then a done that applies it.
        self.sync();
        if self.live.unavailable {
            tracing::debug!("another input method holds the seat; this dictation pastes");
        }
    }

    fn preedit(&mut self, id: u64, text: &str) {
        if self.broken
            || !self
                .session
                .as_ref()
                .is_some_and(|session| session.id == id)
        {
            return;
        }
        self.sync();
        if !self.live.active || self.live.unavailable {
            return;
        }
        let _ = self.queue.dispatch_pending(&mut self.live);
        let serial = self.live.done;
        let cursor = cursor_position(text);
        if let Some(session) = &mut self.session {
            session
                .method
                .set_preedit_string(text.to_owned(), cursor, cursor);
            session.method.commit(serial);
            session.shown = true;
        }
        let _ = self.connection.flush();
    }

    fn finish(&mut self, id: u64, text: &str, claim: &AtomicU8, reply: &SyncSender<bool>) {
        if self.broken
            || !self
                .session
                .as_ref()
                .is_some_and(|session| session.id == id)
        {
            let _ = reply.send(false);
            return;
        }
        self.sync();
        if self.broken || self.session.is_none() {
            let _ = reply.send(false);
            return;
        }
        let commit =
            self.live.active && !self.live.unavailable && commits_through_input_method(text);
        if commit && !owner_claims(claim) {
            // The caller gave up while this roundtrip ran: clear, never
            // commit after its paste.
            self.clear_preedit();
            let _ = reply.send(false);
            self.end();
            return;
        }
        if commit {
            let _ = self.queue.dispatch_pending(&mut self.live);
            let serial = self.live.done;
            if let Some(session) = &mut self.session {
                session.method.set_preedit_string(String::new(), 0, 0);
                session.method.commit_string(text.to_owned());
                session.method.commit(serial);
                session.shown = false;
            }
            // A failed flush leaves the requests unflushed while the
            // connection is marked broken, so the caller may paste safely.
            if self.connection.flush().is_err() {
                tracing::info!("live typing lost its compositor connection");
                self.broken = true;
                self.session = None;
                let _ = reply.send(false);
                return;
            }
            let _ = reply.send(true);
            self.end();
            return;
        }
        self.clear_preedit();
        let _ = reply.send(false);
        self.end();
    }

    fn cancel(&mut self, id: u64) {
        if self.broken
            || !self
                .session
                .as_ref()
                .is_some_and(|session| session.id == id)
        {
            return;
        }
        self.sync();
        self.clear_preedit();
        self.end();
    }

    fn clear_preedit(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !session.shown {
            return;
        }
        if self.live.unavailable {
            session.shown = false;
            return;
        }
        let _ = self.queue.dispatch_pending(&mut self.live);
        let serial = self.live.done;
        session.method.set_preedit_string(String::new(), 0, 0);
        session.method.commit(serial);
        session.shown = false;
        let _ = self.connection.flush();
    }

    /// Destroys the input method object, so the seat is free while idle.
    fn end(&mut self) {
        if let Some(session) = self.session.take() {
            session.method.destroy();
            let _ = self.connection.flush();
            self.sync();
        }
        self.live = Live::default();
    }

    /// Applies the compositor's events, and marks the connection broken when
    /// it has gone away.
    fn sync(&mut self) {
        if self.broken {
            return;
        }
        if let Err(error) = self.queue.roundtrip(&mut self.live) {
            tracing::info!(%error, "live typing lost its compositor connection");
            self.broken = true;
            self.session = None;
        }
    }

    fn shutdown(&mut self) {
        // A broken connection may still hold an abandoned commit in its
        // buffer; never flush it.
        if self.broken {
            return;
        }
        self.clear_preedit();
        self.end();
        self.manager.destroy();
        let _ = self.connection.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Live {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for Live {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpInputMethodManagerV2, ()> for Live {
    fn event(
        _: &mut Self,
        _: &ZwpInputMethodManagerV2,
        _: ManagerEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpInputMethodV2, ()> for Live {
    fn event(
        live: &mut Self,
        _: &ZwpInputMethodV2,
        event: InputMethodEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            InputMethodEvent::Activate => live.pending_active = Some(true),
            InputMethodEvent::Deactivate => live.pending_active = Some(false),
            InputMethodEvent::Done => live.done(),
            InputMethodEvent::Unavailable => {
                live.unavailable = true;
                live.active = false;
            }
            InputMethodEvent::SurroundingText { .. }
            | InputMethodEvent::TextChangeCause { .. }
            | InputMethodEvent::ContentType { .. } => {}
        }
    }
}

/// The single-line preedit for provisional words, or None when they are
/// blank. A protocol message carries at most [`MAX_TEXT`] bytes, so longer
/// text is cut at a character boundary.
fn preedit_text(stable: &str, tentative: &str) -> Option<String> {
    let stable = stable.trim();
    let tentative = tentative.trim();
    let mut text = String::with_capacity(stable.len() + 1 + tentative.len());
    text.push_str(stable);
    if !stable.is_empty() && !tentative.is_empty() {
        text.push(' ');
    }
    text.push_str(tentative);
    if text.is_empty() {
        return None;
    }
    let end = char_boundary(&text, MAX_TEXT);
    text.truncate(end);
    Some(text)
}

/// Whether the final text is committed through the input method rather than
/// pasted: non-empty, one line, and within the protocol's message limit.
fn commits_through_input_method(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= MAX_TEXT
        && !text
            .chars()
            .any(|character| character == '\n' || character == '\r')
}

/// A preedit's cursor at its end, in bytes.
fn cursor_position(text: &str) -> i32 {
    i32::try_from(text.len()).unwrap_or(i32::MAX)
}

/// The largest index at or below `limit` that is a character boundary.
fn char_boundary(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisional_words_become_one_preedit_line() {
        assert_eq!(
            preedit_text(" so the plan", " for today").as_deref(),
            Some("so the plan for today")
        );
        assert_eq!(preedit_text("", " heute").as_deref(), Some("heute"));
        assert_eq!(preedit_text("done", "").as_deref(), Some("done"));
        assert_eq!(preedit_text("  ", " "), None);
    }

    #[test]
    fn a_preedit_stops_at_a_character_boundary() {
        let long = "a".repeat(MAX_TEXT + 10);
        assert_eq!(preedit_text(&long, "").unwrap().len(), MAX_TEXT);
        let text = format!("{}é", "a".repeat(MAX_TEXT - 1));
        let fitted = preedit_text(&text, "").unwrap();
        assert_eq!(fitted.len(), MAX_TEXT - 1);
        assert!(fitted.ends_with('a'));
    }

    #[test]
    fn only_one_line_text_within_the_limit_commits_through_the_input_method() {
        assert!(commits_through_input_method("hello"));
        assert!(commits_through_input_method(" a "));
        assert!(!commits_through_input_method(""));
        assert!(!commits_through_input_method("   "));
        assert!(!commits_through_input_method("two\nlines"));
        assert!(!commits_through_input_method("two\rlines"));
        assert!(commits_through_input_method(&"a".repeat(MAX_TEXT)));
        assert!(!commits_through_input_method(&"a".repeat(MAX_TEXT + 1)));
    }

    #[test]
    fn the_cursor_sits_after_the_preedit() {
        assert_eq!(cursor_position("café"), 5);
        assert_eq!(cursor_position(""), 0);
    }

    #[test]
    fn done_events_number_the_commit_serial_and_apply_activation() {
        let mut live = Live::default();
        assert_eq!(live.done, 0);
        assert!(!live.active);
        live.pending_active = Some(true);
        live.done();
        assert_eq!(live.done, 1);
        assert!(live.active);
        live.pending_active = Some(false);
        live.done();
        assert_eq!(live.done, 2);
        assert!(!live.active);
    }

    #[test]
    fn the_owner_wins_the_commit_before_queueing_it() {
        let claim = AtomicU8::new(PENDING);
        assert!(owner_claims(&claim));
        assert!(!caller_abandons(&claim));
        assert_eq!(claim.load(Ordering::Acquire), COMMITTING);
    }

    #[test]
    fn a_waiting_caller_wins_abandonment_before_the_owner_claims() {
        let claim = AtomicU8::new(PENDING);
        assert!(caller_abandons(&claim));
        assert!(!owner_claims(&claim));
        assert_eq!(claim.load(Ordering::Acquire), ABANDONED);
    }

    #[test]
    fn a_started_session_cancels_when_dropped() {
        let (live, commands) = LiveTyping::for_test();
        let mut session = Session::new(live, 7);
        session.start();
        assert!(matches!(commands.try_recv(), Ok(Command::Begin(7))));
        drop(session);
        assert!(matches!(commands.try_recv(), Ok(Command::Cancel(7))));
        assert!(commands.try_recv().is_err(), "one cancel");
    }

    #[test]
    fn a_session_that_never_started_sends_nothing() {
        let (live, commands) = LiveTyping::for_test();
        let session = Session::new(live.clone(), 8);
        session.preedit("never", "shown");
        assert!(live.preedit.lock().unwrap().is_none());
        drop(session);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn finishing_a_session_never_sends_cancel() {
        let (live, commands) = LiveTyping::for_test();
        let mut session = Session::new(live, 9);
        session.start();
        assert!(matches!(commands.try_recv(), Ok(Command::Begin(9))));
        let responder = thread::spawn(move || {
            let Ok(Command::Finish { claim, reply, .. }) = commands.recv() else {
                panic!("a finish");
            };
            assert!(owner_claims(&claim));
            let _ = reply.send(true);
            commands
        });
        assert!(session.finish("hello"));
        let commands = responder.join().unwrap();
        assert!(commands.try_recv().is_err(), "finish replaced cancel");
    }

    #[test]
    fn a_full_control_queue_stops_new_sessions_but_not_old_ones() {
        let (live, commands) = LiveTyping::for_test();
        live.queued.store(CONTROL_QUEUE_LIMIT, Ordering::Release);
        live.begin(1);
        assert!(!live.is_accepting(), "an overfull queue stops new sessions");
        assert!(commands.try_recv().is_err(), "begin was not queued");
        live.cancel(2);
        assert!(
            matches!(commands.try_recv(), Ok(Command::Cancel(2))),
            "a begun session still cancels"
        );
    }

    #[test]
    fn a_stopped_owner_still_finishes_and_cancels_a_session_that_began() {
        let (live, commands) = LiveTyping::for_test();
        let mut cancel = Session::new(live.clone(), 7);
        cancel.start();
        let mut finish = Session::new(live.clone(), 8);
        finish.start();
        assert!(matches!(commands.try_recv(), Ok(Command::Begin(7))));
        assert!(matches!(commands.try_recv(), Ok(Command::Begin(8))));
        live.accepting.store(false, Ordering::Release);
        drop(cancel);
        assert!(matches!(commands.try_recv(), Ok(Command::Cancel(7))));
        let responder = thread::spawn(move || {
            let Ok(Command::Finish { claim, reply, .. }) = commands.recv() else {
                panic!("a finish");
            };
            assert!(owner_claims(&claim));
            let _ = reply.send(true);
        });
        assert!(finish.finish("hello"));
        responder.join().unwrap();
    }

    #[test]
    fn only_the_newest_preedit_waits_and_it_wakes_once() {
        let (live, commands) = LiveTyping::for_test();
        live.preedit(1, "so", "");
        live.preedit(1, "so the", " plan");
        assert!(matches!(commands.try_recv(), Ok(Command::Wake)));
        assert!(commands.try_recv().is_err(), "one wake");
        assert_eq!(take_preedit(&live.preedit), Some((1, "so the plan".into())));
        live.cancel(1);
        assert!(
            matches!(commands.try_recv(), Ok(Command::Cancel(1))),
            "a control command is never crowded out by preedits"
        );
    }
}
