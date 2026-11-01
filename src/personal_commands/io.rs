//! The asynchronous host operations behind a script's `fetch`,
//! `node:child_process`, and timers. JavaScript starts an operation with a
//! resolve and reject pair (`hex:io`'s `start`); the work runs on its own
//! thread, and [`Io::wait`] hands finished operations back to the script
//! thread, which settles them so QuickJS's job queue can go on. Everything
//! an operation started ends with the transformation call that started it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::{Array, ArrayBuffer, Ctx, Exception, Function, IntoJs, Object, Persistent, Value};
use serde::{Deserialize, Serialize};

use super::fetch::{self, FetchRequest, FetchResponse};
use super::process::{self, ExecRequest, ExecResult, ProcessTree};

/// Requests in flight at once, across every script; more wait their turn.
/// A request a script gave up on still counts until its thread ends.
const MAX_FETCHES: usize = 8;
/// Programs running at once, across every script.
const MAX_PROCESSES: usize = 4;
/// How long [`Io::wait`] sleeps before checking for a stop request.
const WAIT_SLICE: Duration = Duration::from_millis(50);

static FETCHES: AtomicUsize = AtomicUsize::new(0);
static PROCESSES: AtomicUsize = AtomicUsize::new(0);

/// The host APIs a config can use beyond plain JavaScript.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Capabilities {
    pub fetch: bool,
    pub exec: bool,
}

impl Capabilities {
    pub const NONE: Self = Self {
        fetch: false,
        exec: false,
    };

    pub fn any(self) -> bool {
        self.fetch || self.exec
    }
}

pub type Shared = Rc<RefCell<Io>>;

pub struct Io {
    next_id: u32,
    operations: BTreeMap<u32, Operation>,
    /// Every process tree the current call started, ended when it returns.
    trees: Vec<ProcessTree>,
    sender: Sender<Completion>,
    receiver: Receiver<Completion>,
    /// What the current call may use; nothing outside a call.
    allowed: Capabilities,
    deadline: Option<Instant>,
    workspace: PathBuf,
}

struct Operation {
    resolve: Persistent<Function<'static>>,
    reject: Persistent<Function<'static>>,
    state: State,
}

enum State {
    Timer(Instant),
    WaitingToFetch(FetchRequest),
    Fetching,
    WaitingToRun(ExecRequest),
    Running {
        tree: Option<ProcessTree>,
        timeout: Option<Instant>,
        timed_out: bool,
    },
}

struct Completion {
    id: u32,
    outcome: Outcome,
}

enum Outcome {
    Fetch(Result<FetchResponse, String>),
    Exec(ExecResult),
}

/// A settled operation, ready to hand to JavaScript.
pub struct Settled {
    function: Persistent<Function<'static>>,
    value: Payload,
}

enum Payload {
    Nothing,
    Message(String),
    Fetch(FetchResponse),
    Exec(String),
}

impl Settled {
    /// Calls the operation's resolve or reject function.
    pub fn deliver<'js>(self, ctx: &Ctx<'js>) -> rquickjs::Result<()> {
        let function = self.function.restore(ctx)?;
        let value: Value = match self.value {
            Payload::Nothing => Value::new_undefined(ctx.clone()),
            Payload::Message(message) | Payload::Exec(message) => message.into_js(ctx)?,
            Payload::Fetch(response) => response_object(ctx, response)?.into_value(),
        };
        function.call::<_, ()>((value,))
    }
}

fn response_object<'js>(ctx: &Ctx<'js>, response: FetchResponse) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("status", response.status)?;
    object.set("statusText", response.status_text)?;
    object.set("url", response.url)?;
    let headers = Array::new(ctx.clone())?;
    for (index, (name, value)) in response.headers.into_iter().enumerate() {
        headers.set(index, vec![name, value])?;
    }
    object.set("headers", headers)?;
    object.set("body", ArrayBuffer::new(ctx.clone(), response.body)?)?;
    Ok(object)
}

/// What [`Io::wait`] found.
pub enum Wake {
    Settled(Vec<Settled>),
    /// Nothing is pending, so the call's promise can never settle.
    Idle,
}

impl Io {
    pub fn new(workspace: PathBuf) -> Shared {
        let (sender, receiver) = mpsc::channel();
        Rc::new(RefCell::new(Self {
            next_id: 0,
            operations: BTreeMap::new(),
            trees: Vec::new(),
            sender,
            receiver,
            allowed: Capabilities::NONE,
            deadline: None,
            workspace,
        }))
    }

    /// Starts a call that may use `allowed` until `deadline`.
    pub fn begin(&mut self, deadline: Instant, allowed: Capabilities) {
        self.deadline = Some(deadline);
        self.allowed = allowed;
    }

    /// Ends the call: forgets its pending operations and ends every process
    /// tree it started. Requests already sent finish on their own threads
    /// within the call's deadline, unread.
    pub fn end(&mut self) {
        self.operations.clear();
        for tree in self.trees.drain(..) {
            tree.kill();
        }
        self.deadline = None;
        self.allowed = Capabilities::NONE;
        while self.receiver.try_recv().is_ok() {}
    }

    /// Waits until an operation settles, a timer is due, or `until`, in
    /// slices short enough to notice a stop request.
    pub fn wait(&mut self, until: Instant) -> Wake {
        self.pump();
        if self.operations.is_empty() {
            return Wake::Idle;
        }
        let now = Instant::now();
        let mut settled = self.expire(now);
        if settled.is_empty() {
            let wake = self
                .operations
                .values()
                .filter_map(|operation| match operation.state {
                    State::Timer(at) => Some(at),
                    State::Running { timeout, .. } => timeout,
                    _ => None,
                })
                .fold(until.min(now + WAIT_SLICE), Instant::min);
            match self
                .receiver
                .recv_timeout(wake.saturating_duration_since(now))
            {
                Ok(completion) => settled.extend(self.complete(completion)),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
            }
            settled.extend(self.expire(Instant::now()));
        }
        while let Ok(completion) = self.receiver.try_recv() {
            settled.extend(self.complete(completion));
        }
        self.pump();
        Wake::Settled(settled)
    }

    /// Fires due timers and sends SIGTERM to programs past their `timeout`.
    fn expire(&mut self, now: Instant) -> Vec<Settled> {
        let due = self
            .operations
            .iter()
            .filter(|(_, operation)| matches!(operation.state, State::Timer(at) if at <= now))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for operation in self.operations.values_mut() {
            if let State::Running {
                tree,
                timeout,
                timed_out,
            } = &mut operation.state
                && timeout.is_some_and(|timeout| timeout <= now)
            {
                *timeout = None;
                *timed_out = true;
                if let Some(tree) = tree {
                    tree.terminate();
                }
            }
        }
        due.into_iter()
            .filter_map(|id| self.operations.remove(&id))
            .map(|operation| Settled {
                function: operation.resolve,
                value: Payload::Nothing,
            })
            .collect()
    }

    fn complete(&mut self, completion: Completion) -> Option<Settled> {
        let operation = self.operations.remove(&completion.id)?;
        Some(match completion.outcome {
            Outcome::Fetch(Ok(response)) => Settled {
                function: operation.resolve,
                value: Payload::Fetch(response),
            },
            Outcome::Fetch(Err(message)) => Settled {
                function: operation.reject,
                value: Payload::Message(message),
            },
            Outcome::Exec(mut result) => {
                result.timed_out = matches!(
                    operation.state,
                    State::Running {
                        timed_out: true,
                        ..
                    }
                );
                Settled {
                    function: operation.resolve,
                    value: Payload::Exec(
                        serde_json::to_string(&result).expect("plain data serializes"),
                    ),
                }
            }
        })
    }

    /// Starts waiting requests and programs while there is room, oldest first.
    fn pump(&mut self) {
        let waiting = self
            .operations
            .iter()
            .filter(|(_, operation)| {
                matches!(
                    operation.state,
                    State::WaitingToFetch(_) | State::WaitingToRun(_)
                )
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in waiting {
            let Some(operation) = self.operations.get_mut(&id) else {
                continue;
            };
            match &operation.state {
                State::WaitingToFetch(_) if reserve(&FETCHES, MAX_FETCHES) => {
                    let State::WaitingToFetch(request) =
                        std::mem::replace(&mut operation.state, State::Fetching)
                    else {
                        unreachable!()
                    };
                    let timeout = self
                        .deadline
                        .map_or(Duration::ZERO, |deadline| {
                            deadline.saturating_duration_since(Instant::now())
                        })
                        .max(Duration::from_millis(1));
                    let sender = self.sender.clone();
                    let spawned = thread::Builder::new()
                        .name("hex-config-fetch".into())
                        .spawn(move || {
                            let outcome = fetch::send(request, timeout);
                            FETCHES.fetch_sub(1, Ordering::AcqRel);
                            let _ = sender.send(Completion {
                                id,
                                outcome: Outcome::Fetch(outcome),
                            });
                        });
                    if let Err(error) = spawned {
                        FETCHES.fetch_sub(1, Ordering::AcqRel);
                        let _ = self.sender.send(Completion {
                            id,
                            outcome: Outcome::Fetch(Err(error.to_string())),
                        });
                    }
                }
                State::WaitingToRun(_) if reserve(&PROCESSES, MAX_PROCESSES) => {
                    let State::WaitingToRun(request) = std::mem::replace(
                        &mut operation.state,
                        State::Running {
                            tree: None,
                            timeout: None,
                            timed_out: false,
                        },
                    ) else {
                        unreachable!()
                    };
                    let timeout = request
                        .timeout
                        .filter(|timeout| *timeout > 0.0)
                        .map(|timeout| Instant::now() + Duration::from_secs_f64(timeout / 1000.0));
                    let sender = self.sender.clone();
                    let tree = process::start(request, &self.workspace, move |result| {
                        PROCESSES.fetch_sub(1, Ordering::AcqRel);
                        let _ = sender.send(Completion {
                            id,
                            outcome: Outcome::Exec(result),
                        });
                    });
                    if let Some(tree) = &tree {
                        self.trees.push(tree.clone());
                    }
                    operation.state = State::Running {
                        tree,
                        timeout,
                        timed_out: false,
                    };
                }
                _ => {}
            }
        }
    }

    fn insert(&mut self, operation: Operation) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.operations.insert(self.next_id, operation);
        self.next_id
    }

    /// Forgets an operation the script gave up on, sending SIGTERM to its
    /// program.
    fn cancel(&mut self, id: u32) {
        if let Some(Operation {
            state: State::Running {
                tree: Some(tree), ..
            },
            ..
        }) = self.operations.remove(&id)
        {
            tree.terminate();
        }
    }
}

/// Takes one of `limit` slots in `counter`, if one is free.
fn reserve(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < limit).then_some(count + 1)
        })
        .is_ok()
}

/// The `hex:io` object: `start(kind, spec, resolve, reject)` returns an
/// operation id, `cancel(id)` gives one up, and `decode`/`encode` convert
/// UTF-8 bodies.
pub fn bindings<'js>(ctx: &Ctx<'js>, io: &Shared) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    let starting = io.clone();
    object.set(
        "start",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>,
                  kind: String,
                  spec: String,
                  resolve: Function<'js>,
                  reject: Function<'js>| {
                let state = parse(&ctx, &starting.borrow(), &kind, &spec)?;
                let operation = Operation {
                    resolve: Persistent::save(&ctx, resolve),
                    reject: Persistent::save(&ctx, reject),
                    state,
                };
                let mut io = starting.borrow_mut();
                let id = io.insert(operation);
                io.pump();
                Ok::<_, rquickjs::Error>(id)
            },
        )?,
    )?;
    let cancelling = io.clone();
    object.set(
        "cancel",
        Function::new(ctx.clone(), move |id: u32| {
            cancelling.borrow_mut().cancel(id);
        })?,
    )?;
    object.set(
        "decode",
        Function::new(ctx.clone(), |buffer: ArrayBuffer<'js>| {
            // SAFETY: the bytes are copied before any script runs again, so
            // nothing can detach or resize the buffer while they are read.
            String::from_utf8_lossy(unsafe { buffer.as_bytes() }.unwrap_or_default()).into_owned()
        })?,
    )?;
    object.set(
        "encode",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, text: String| {
            ArrayBuffer::new(ctx, text.into_bytes())
        })?,
    )?;
    Ok(object)
}

/// The operation `kind` and its JSON `spec` describe, if the call may start it.
fn parse(ctx: &Ctx<'_>, io: &Io, kind: &str, spec: &str) -> rquickjs::Result<State> {
    let invalid = |error: serde_json::Error| Exception::throw_type(ctx, &error.to_string());
    match kind {
        "timer" => {
            let delay: f64 = serde_json::from_str(spec).map_err(invalid)?;
            let delay = if delay.is_finite() {
                delay.max(0.0)
            } else {
                0.0
            };
            Ok(State::Timer(
                Instant::now() + Duration::from_secs_f64(delay.min(1e9) / 1000.0),
            ))
        }
        "fetch" => {
            refuse_unless(ctx, io.allowed.fetch, "fetch", "use fetch")?;
            let request: FetchRequest = serde_json::from_str(spec).map_err(invalid)?;
            request
                .host()
                .map_err(|message| Exception::throw_type(ctx, &message))?;
            Ok(State::WaitingToFetch(request))
        }
        "exec" => {
            refuse_unless(ctx, io.allowed.exec, "exec", "run programs")?;
            let request: ExecRequest = serde_json::from_str(spec).map_err(invalid)?;
            Ok(State::WaitingToRun(request))
        }
        _ => Err(Exception::throw_type(
            ctx,
            &format!("unknown operation {kind}"),
        )),
    }
}

/// The trust gate: a config uses `fetch` or runs programs only once the
/// person allowed this exact config.
fn refuse_unless(
    ctx: &Ctx<'_>,
    allowed: bool,
    capability: &str,
    action: &str,
) -> rquickjs::Result<()> {
    if allowed {
        return Ok(());
    }
    tracing::warn!(
        target: "hex_config",
        capability,
        "a transformation needs permission; allow it in Hex's Output pane"
    );
    Err(Exception::throw_message(
        ctx,
        &format!(
            "Hex has not allowed this config to {action}. Allow it in Hex's Output pane; Hex asks again whenever the config changes"
        ),
    ))
}
