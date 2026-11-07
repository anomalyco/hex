//! Labs' Live typing on Wayland: a dictation's provisional words sit in the
//! focused text field as an input-method preedit, underlined and replaced on
//! each update, and its final text is committed through the input method
//! instead of pasted.
//!
//! One owner thread holds a private Wayland connection. A capture's Begin
//! creates `zwp_input_method_v2` for the seat, and Finish or Cancel destroys
//! it, so Hex never holds the seat's input method while idle and another
//! input method keeps working. Everything is bounded: capture sends never
//! wait, and Finish waits briefly for the commit before falling back to the
//! clipboard paste. Any failure is inert and silent apart from one log line.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

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
/// A capture's commands may queue while the owner is busy; preedits beyond
/// this are dropped rather than waited on.
const BACKLOG: usize = 32;
/// How long Finish waits to hand its command over and for the reply.
const SEND_TIMEOUT: Duration = Duration::from_millis(100);
const FINISH_TIMEOUT: Duration = Duration::from_millis(350);

/// One capture's live-typing session.
pub(crate) struct LiveTyping {
    commands: SyncSender<Command>,
}

enum Command {
    Begin(u64),
    Preedit {
        id: u64,
        text: String,
    },
    Finish {
        id: u64,
        text: String,
        /// The caller stops waiting here, so a slow compositor never gets a
        /// commit after the caller has pasted.
        deadline: Instant,
        reply: SyncSender<bool>,
    },
    Cancel(u64),
}

impl LiveTyping {
    /// Starts the owner thread without connecting: the connection opens on
    /// the first Begin.
    pub(crate) fn start() -> Arc<Self> {
        let (commands, receiver) = mpsc::sync_channel(BACKLOG);
        if let Err(error) = thread::Builder::new()
            .name("hex-live-typing".into())
            .spawn(move || run(receiver))
        {
            tracing::info!(%error, "live typing is unavailable; dictations paste normally");
        }
        Arc::new(Self { commands })
    }

    pub(crate) fn begin(&self, id: u64) {
        self.send(Command::Begin(id));
    }

    pub(crate) fn preedit(&self, id: u64, stable: &str, tentative: &str) {
        let Some(text) = preedit_text(stable, tentative) else {
            return;
        };
        let _ = self.commands.try_send(Command::Preedit { id, text });
    }

    pub(crate) fn cancel(&self, id: u64) {
        self.send(Command::Cancel(id));
    }

    /// Commits `text` through the input method when its session is active,
    /// and says whether it did. False means the caller pastes as usual.
    pub(crate) fn finish(&self, id: u64, text: &str) -> bool {
        let (reply, answer) = mpsc::sync_channel(1);
        let commit_by = Instant::now() + FINISH_TIMEOUT;
        let mut command = Command::Finish {
            id,
            text: text.to_owned(),
            deadline: commit_by,
            reply,
        };
        let send_by = Instant::now() + SEND_TIMEOUT;
        loop {
            match self.commands.try_send(command) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) => {
                    if Instant::now() >= send_by {
                        return false;
                    }
                    command = returned;
                    thread::sleep(Duration::from_millis(2));
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
        answer.recv_timeout(FINISH_TIMEOUT).unwrap_or_default()
    }

    fn send(&self, command: Command) {
        if let Err(TrySendError::Disconnected(_)) = self.commands.try_send(command) {
            tracing::debug!("live typing's owner thread is gone");
        }
    }
}

fn run(commands: Receiver<Command>) {
    let mut owner = match Owner::connect() {
        Ok(owner) => owner,
        Err(error) => {
            tracing::info!(%error, "live typing is unavailable; dictations paste normally");
            return;
        }
    };
    while let Ok(command) = commands.recv() {
        match command {
            Command::Begin(id) => owner.begin(id),
            Command::Preedit { id, text } => owner.preedit(id, &text),
            Command::Finish {
                id,
                text,
                deadline,
                reply,
            } => owner.finish(id, &text, deadline, &reply),
            Command::Cancel(id) => owner.cancel(id),
        }
    }
    owner.shutdown();
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

struct Session {
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
    session: Option<Session>,
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
        self.session = Some(Session {
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
        let cursor = cursor_position(text);
        if let Some(session) = &mut self.session {
            session
                .method
                .set_preedit_string(text.to_owned(), cursor, cursor);
            session.method.commit(self.live.done);
            session.shown = true;
        }
        let _ = self.connection.flush();
    }

    fn finish(&mut self, id: u64, text: &str, deadline: Instant, reply: &SyncSender<bool>) {
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
        let inserted = self.live.active
            && !self.live.unavailable
            && commits_through_input_method(text)
            && Instant::now() <= deadline;
        if inserted {
            if let Some(session) = &mut self.session {
                session.method.set_preedit_string(String::new(), 0, 0);
                session.method.commit_string(text.to_owned());
                session.method.commit(self.live.done);
                session.shown = false;
            }
            // The caller stops waiting at `deadline`: a flush that failed
            // leaves nothing to insert, and the caller pastes. Never flush
            // the abandoned requests later.
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
        let done = self.live.done;
        let Some(session) = &mut self.session else {
            return;
        };
        if !session.shown {
            return;
        }
        session.method.set_preedit_string(String::new(), 0, 0);
        session.method.commit(done);
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
}
