//! Where a written capture goes among OpenCode sessions: Scrawl's router
//! (`crates/router`) run in Hex's process with `capture.routing` and
//! Scrawl's own router folders, so `scrawl router undo`, Scrawl's rule
//! offers, and its evals see Hex's sends as Scrawl's.
//!
//! The collector starts a [`Look`] at the tab, terminal, and sessions the
//! moment a capture stops, beside its writing. Once it is written, the
//! finisher hands it to the one `capture-route` thread ([`Routes`]), which
//! decides, sends a saved rule's answer at once, and carries out the
//! capsule's Send and Undo for the newest capture, in the order
//! they came. Every change lands in the snapshot's `last.route`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use scrawl_capture::capture::RouteContext;
use scrawl_capture::delivery::{self, Delivery};
use scrawl_capture::model::{Destination, ScrawlBundle};
use scrawl_capture::routing::{self, Shown, jev, look, policy, smart, undo};
use scrawl_capture::{Router, RouterConfig, RouterDirs, RuleKind, Undone};

use super::{SharedOutcomes, lock};

mod opencode;
mod preparation;
pub(crate) mod rule_match;
mod send;
mod session_name;

pub use rule_match::{AutoSendBy, RuleMatch};
use send::{Clock, DeliverOutcome, PickAction, SavedRule, SendState, Sent, UndoPlan};

#[cfg(test)]
use session_name::session_name_by;
use session_name::{name_answer, session_name};

/// How often sends OpenCode didn't take are retried.
const RETRY_EVERY: Duration = Duration::from_secs(15);
/// How long taking back a send may wait on OpenCode.
const UNDO_DEADLINE: Duration = Duration::from_secs(10);
/// How long a rule's send waits for Undo before it goes. The capsule's
/// Undo ring drains over this same time, from when the rule shows.
pub const SEND_HOLD: Duration = Duration::from_secs(3);
/// A held send resumes with at least this long left, so it never goes the
/// instant the pointer leaves the capsule.
const RESUME_LEFT: Duration = Duration::from_secs(1);
/// A hold the capsule never ends (it closed under the pointer) lapses.
const HOLD_MAX: Duration = Duration::from_secs(120);
const QUEUE: usize = 16;

// ---------------------------------------------------------------------------
// What views see.

/// A written capture's route: the router's answer and what became of the
/// send.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRoute {
    pub answer: RouteAnswer,
    pub delivery: Option<RouteDelivery>,
    /// The site or app a rule could now send straight to where this
    /// capture went, as two sends agreed: `Send { to: <that session>,
    /// always: true }` saves it.
    pub offer: Option<String>,
    /// Structured match scope for the post-send `Always…` configurator
    /// (site subpath vs whole host, terminal folder, or non-general app).
    pub rule_match: Option<RuleMatch>,
}

impl CaptureRoute {
    fn of(answer: RouteAnswer) -> Self {
        Self {
            answer,
            delivery: None,
            offer: None,
            rule_match: None,
        }
    }

    /// The written capture, while the router works out where it goes.
    pub(super) fn looking() -> Self {
        Self::of(RouteAnswer::Looking)
    }
}

/// The route of a capture the router never got to.
pub(super) fn unrouted() -> CaptureRoute {
    CaptureRoute::of(RouteAnswer::NoAnswer { recent: Vec::new() })
}

/// An OpenCode session a capture can go to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteDest {
    pub session: String,
    /// The session's title as the router labels it.
    pub name: String,
    /// A quiet word on why: `rule`, `dev server`, `guess`, `2 min ago`.
    pub why: String,
    /// The session's folder, which the capsule's search also matches.
    pub directory: Option<PathBuf>,
}

/// `recent` is every other session the look read, newest first as first
/// listed; a later answer keeps listed rows in place and adds new ones last.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteAnswer {
    /// Written; the router is still working.
    Looking,
    /// A saved folder rule matched locally; its session is still being resolved.
    ResolvingRule { name: String },
    /// A rule the person saved (`routing.sites`, `routing.terminals` or
    /// `routing.apps`, a Shift pick's among them) says where it goes: sent to `to`
    /// after the countdown, unless Undo cancels it. `site` is what the
    /// rule matches, for an always-rule sending it elsewhere instead.
    Rule {
        to: RouteDest,
        suggested: Vec<RouteDest>,
        recent: Vec<RouteDest>,
        site: Option<String>,
        by: AutoSendBy,
    },
    /// Guesses, best first: everything else the router found (Jev's pick,
    /// HOME when Jev couldn't tell, a dev server's port, a page or its
    /// publisher, a name that was said). Nothing is sent until one is
    /// picked; picking one with `always` saves a rule for `site`, which is
    /// what an always-rule would match (the site's host, the terminal's
    /// folder or the app's name), `None` when no rule can be made.
    Guess {
        suggested: Vec<RouteDest>,
        recent: Vec<RouteDest>,
        site: Option<String>,
    },
    /// Nothing fits: recent sessions only.
    Nothing { recent: Vec<RouteDest> },
    /// The router failed.
    NoAnswer { recent: Vec<RouteDest> },
    /// OpenCode's sessions couldn't be read, so there is nowhere to offer;
    /// `why` says what went wrong in a few words ("didn't answer in time").
    Unreachable { why: String },
}

/// What became of a capture's send; `to` is the session's name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteDelivery {
    Sending {
        to: String,
    },
    Sent {
        to: String,
    },
    /// OpenCode didn't take it; retried every 15 s. `why` says what went
    /// wrong in a few words ("OpenCode isn't running").
    Queued {
        to: String,
        why: String,
    },
    /// Never sent, withdrawn, reverted, or disowned.
    TakenBack,
    /// The session already acted on it.
    TooLate {
        to: String,
    },
    Failed {
        to: String,
        why: String,
    },
}

// ---------------------------------------------------------------------------
// The router.

/// The router with Hex's `capture.routing` and Scrawl's folders under
/// `~/Library/Application Support/scrawl`. Never reads
/// `~/.config/scrawl`. Its own log lines name sites and sessions, so they
/// go to tracing's debug level, except how sends went (`deliver: …`).
pub fn router() -> Router {
    let settings = crate::app_settings::capture_settings();
    let config = settings.routing.config().clone();
    let support = crate::app_paths::home_dir()
        .unwrap_or_default()
        .join("Library/Application Support/scrawl");
    let dirs = RouterDirs::under(
        &support.join("router"),
        Some(support.join("routing-decisions.jsonl")),
        Some(settings.captures_dir()),
    );
    let router = Router::new(config, dirs)
        .with_log(|line| {
            if line.starts_with("deliver:") || line.starts_with("routing time:") {
                tracing::info!(line, "capture router");
            } else {
                tracing::debug!(line, "capture router");
            }
        })
        .with_summary(scrawl_capture::sketch::summary_markdown);
    #[cfg(target_os = "windows")]
    let router = router.with_opencode(std::sync::Arc::new(opencode::Wsl));
    router
}

fn sends(config: &RouterConfig) -> bool {
    config.send_captures && std::env::var_os("SCRAWL_NO_OPENCODE").is_none() && !opencode_managed()
}

/// Whether an organization turned OpenCode processing off, which includes
/// sending captures to OpenCode sessions (`DisableOpenCodeProcessing`).
fn opencode_managed() -> bool {
    crate::policy::Policy::current().locks(crate::policy::Lock::OpenCodeProcessing)
}

/// Whether Jev has an active credential available (`$OPENCODE_API_KEY` or
/// OpenCode's credential) within `budget`. Blocks; call it off the UI thread.
pub fn jev_available(budget: Duration) -> bool {
    if opencode_managed() || std::env::var_os("SCRAWL_NO_OPENCODE").is_some() {
        return false;
    }
    router().run(|| jev::available(budget))
}

/// Open OpenCode sessions, newest first, as Send to… lists them, read
/// within `budget`. Blocks; call it off the UI thread.
pub fn recent_sessions(budget: Duration) -> anyhow::Result<Vec<RouteDest>> {
    anyhow::ensure!(!opencode_managed(), "OpenCode is turned off here");
    let read = router().run(|| {
        let mut get = routing::within(scrawl_capture::opencode::api_get_within, budget);
        look::read_candidates(&mut get, &[], &[], 1)
    });
    if read.candidates.is_empty()
        && let Some(error) = read.error
    {
        return Err(error);
    }
    Ok(recent(&read.candidates, &[], now_ms()))
}

/// Sends the PNG at `png` with `text` into `session`, as the router sends
/// captures. Blocks; call it off the UI thread.
#[cfg(target_os = "macos")]
pub fn send_image(session: &str, text: &str, png: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(!opencode_managed(), "OpenCode is turned off here");
    router().run(|| scrawl_capture::opencode::send_image_to_session(session, text, png))?;
    Ok(())
}

/// The tab, terminal, and sessions behind a capture that just stopped,
/// read while it is written.
pub(super) struct Look {
    router: Router,
    seen: Receiver<Option<look::Live>>,
    /// When routing stops waiting for it.
    until: Instant,
    local: std::sync::Arc<Mutex<look::Live>>,
    model: jev::Hooks,
}

/// How long past its deadline a look's last step may run before routing
/// goes on without it.
const LOOK_GRACE: Duration = Duration::from_secs(1);

/// Starts looking, or `None` when captures aren't sent (`autoSend` off).
/// The whole look has [`look::LOOK_DEADLINE`]: the steps not begun by then
/// are skipped, and routing waits at most [`LOOK_GRACE`] more for the one
/// under way (the tab's osascript, 6 s at worst), then goes on without it.
pub(super) fn look(context: RouteContext) -> Option<Look> {
    look_with(context, router(), look::Senses::default())
}

fn look_with(context: RouteContext, router: Router, senses: look::Senses) -> Option<Look> {
    if !sends(router.config()) {
        return None;
    }
    let deadline = Instant::now() + look::LOOK_DEADLINE;
    let (found, seen) = mpsc::sync_channel(1);
    let local = std::sync::Arc::new(Mutex::new(look::Live::default()));
    {
        let router = router.clone();
        let local = local.clone();
        thread::Builder::new()
            .name("capture-look".into())
            .spawn(move || {
                let live = router.run(|| {
                    let live = look::look_live_observed(
                        context.front_bundle_id.as_deref(),
                        context.captured_bundle_id.as_deref(),
                        context.app_pid,
                        deadline,
                        &senses,
                        |url, terminal| {
                            let mut local = local.lock().unwrap_or_else(|e| e.into_inner());
                            local.url = url.map(str::to_owned);
                            local.terminal = terminal.map(Path::to_path_buf);
                        },
                    );
                    *local.lock().unwrap_or_else(|e| e.into_inner()) = live.clone();
                    Some(live)
                });
                let _ = found.try_send(live);
            })
            .inspect_err(|error| tracing::warn!(%error, "could not look for a capture's route"))
            .ok()?;
    }
    Some(Look {
        router,
        seen,
        until: deadline + LOOK_GRACE,
        local,
        model: jev::Hooks::default(),
    })
}

// ---------------------------------------------------------------------------
// The capture-route thread.

pub(super) enum Job {
    Written {
        bundle: PathBuf,
        look: Look,
    },
    Send {
        to: String,
        always: bool,
    },
    SaveRule {
        kind: RuleKind,
        key: String,
        target: String,
    },
    Undo,
    /// The pointer is on the capsule (`true`) or left it: a rule's send
    /// waiting out its Undo window pauses there and resumes after.
    Hold(bool),
    /// `hex capture send`: any capture, to `to` or where its route or its
    /// last send says.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    SendBundle {
        bundle: PathBuf,
        to: Option<String>,
        always: bool,
        reply: SyncSender<Result<String, String>>,
    },
}

/// The running host's route queue, for `hex capture send`, with the
/// [`Routes`] it belongs to.
static JOBS: Mutex<Option<(u64, SyncSender<Job>)>> = Mutex::new(None);
static NEXT_ROUTES: AtomicU64 = AtomicU64::new(0);

/// How long `hex capture send` waits for the send, inside the control
/// deadline.
#[cfg_attr(target_os = "windows", allow(dead_code))]
const CLI_WAIT: Duration = Duration::from_secs(8);

/// Sends `bundle` through the `capture-route` thread and says what became
/// of it: as the capsule's pick when it is the newest capture, so its
/// route and Undo follow.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(super) fn send_bundle(
    bundle: PathBuf,
    to: Option<String>,
    always: bool,
) -> Result<String, String> {
    let jobs = JOBS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map(|(_, jobs)| jobs.clone())
        .ok_or("captures are off")?;
    let (reply, answer) = mpsc::sync_channel(1);
    let job = Job::SendBundle {
        bundle,
        to,
        always,
        reply,
    };
    if !send(&jobs, job) {
        return Err("the capture router is busy; try again shortly".into());
    }
    answer
        .recv_timeout(CLI_WAIT)
        .unwrap_or_else(|_| Ok("Still sending; `hex capture show` tells how it went".into()))
}

/// The `capture-route` thread and its queue.
pub(super) struct Routes {
    id: u64,
    jobs: SyncSender<Job>,
    thread: JoinHandle<()>,
}

impl Routes {
    pub(super) fn start(outcomes: SharedOutcomes) -> std::io::Result<Self> {
        let (jobs, queued) = mpsc::sync_channel(QUEUE);
        let thread = thread::Builder::new()
            .name("capture-route".into())
            .spawn(move || Worker::new(outcomes).run(&queued))?;
        let id = NEXT_ROUTES.fetch_add(1, Ordering::Relaxed);
        *JOBS.lock().unwrap_or_else(|error| error.into_inner()) = Some((id, jobs.clone()));
        Ok(Self { id, jobs, thread })
    }

    pub(super) fn jobs(&self) -> SyncSender<Job> {
        self.jobs.clone()
    }

    /// Queues `job` without waiting; a full queue drops it.
    pub(super) fn send(&self, job: Job) -> bool {
        send(&self.jobs, job)
    }

    /// Ends the thread once its queue is empty and every other sender is
    /// gone.
    pub(super) fn stop(self, deadline: Instant) {
        let mut global = JOBS.lock().unwrap_or_else(|error| error.into_inner());
        if global.as_ref().is_some_and(|(id, _)| *id == self.id) {
            *global = None;
        }
        drop(global);
        drop(self.jobs);
        super::join_by(self.thread, deadline, "capture-route");
    }
}

pub(super) fn send(jobs: &SyncSender<Job>, job: Job) -> bool {
    match jobs.try_send(job) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            tracing::warn!("the capture router is behind; dropping a request");
            false
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

/// The newest routed capture, which the capsule's commands act on.
struct Routed {
    bundle: PathBuf,
    router: Router,
    url: Option<String>,
    app: Option<String>,
    bundle_id: Option<String>,
    /// What an always-rule would match.
    rule: Option<(RuleKind, String)>,
    /// Structured match scope for the post-send `Always…` configurator.
    rule_match: Option<RuleMatch>,
    candidates: Vec<smart::Candidate>,
    answer: RouteAnswer,
    /// The send, hold, delivery, and undo lifecycle; [`CaptureRoute::delivery`]
    /// and [`CaptureRoute::offer`] are projected from this.
    send: SendState,
}

impl Routed {
    fn capture_route(&self) -> CaptureRoute {
        CaptureRoute {
            answer: self.answer.clone(),
            delivery: self.send.delivery(),
            offer: self.send.offer_site(),
            rule_match: self.rule_match.clone(),
        }
    }

    #[cfg(test)]
    fn sent_session(&self) -> Option<&str> {
        match &self.send {
            SendState::Delivered { session, .. }
            | SendState::TooLate {
                session,
                retry_undo: Some(_),
                ..
            } => Some(session.as_str()),
            _ => None,
        }
    }
}

/// Posts a capture to an OpenCode session.
type Forward = fn(&Path, &str) -> anyhow::Result<scrawl_capture::handoff::Posted>;

struct Worker {
    outcomes: SharedOutcomes,
    current: Option<Routed>,
    retry_at: Instant,
    forward: Forward,
    pending: Option<preparation::Pending>,
    older: Vec<(Routed, Option<preparation::Pending>)>,
}

impl Worker {
    fn new(outcomes: SharedOutcomes) -> Self {
        Self {
            outcomes,
            current: None,
            retry_at: Instant::now() + RETRY_EVERY,
            forward: scrawl_capture::handoff::send_bundle,
            pending: None,
            older: Vec::new(),
        }
    }

    fn run(mut self, jobs: &Receiver<Job>) {
        loop {
            let wait = self.retry_at.saturating_duration_since(Instant::now());
            let wait = if self.pending.is_some()
                || !self.older.is_empty()
                || self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.send.is_pending_rule())
            {
                wait.min(Duration::from_millis(10))
            } else {
                wait
            };
            match jobs.recv_timeout(wait) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.prepared();
            if Instant::now() >= self.retry_at {
                self.retry();
                self.retry_at = Instant::now() + RETRY_EVERY;
            }
        }
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Written { bundle, look } => self.written(bundle, look),
            Job::Send { to, always } => self.chosen(&to, always),
            Job::SaveRule { kind, key, target } => self.save_rule(kind, key, target),
            Job::Undo => self.undo(),
            Job::Hold(on) => self.hold(on),
            Job::SendBundle {
                bundle,
                to,
                always,
                reply,
            } => {
                let _ = reply.try_send(self.send_bundle(&bundle, to.as_deref(), always));
            }
        }
    }

    /// `hex capture send`: the newest capture as the capsule's pick, any
    /// other straight to `to` or where it last went. `always` (newest only)
    /// also saves a rule for its site or app, as the capsule's ⇧⏎ does.
    fn send_bundle(
        &mut self,
        bundle: &Path,
        to: Option<&str>,
        always: bool,
    ) -> Result<String, String> {
        // Named by the CLI, the routed capture is acted on even when a newer
        // one has taken the capsule.
        if self.is_current(bundle) {
            let current = self.current.as_ref().ok_or("no capture")?;
            let to = match to {
                Some(to) => to.to_owned(),
                None => match &current.answer {
                    RouteAnswer::Rule { to, .. } => to.session.clone(),
                    RouteAnswer::Guess { suggested, .. } if !suggested.is_empty() => {
                        suggested[0].session.clone()
                    }
                    _ => recorded_destination(bundle)
                        .map(|dest| dest.session)
                        .ok_or("its route names no session; say where with --to")?,
                },
            };
            let Some(dest) = self.listed_as(&to) else {
                return Err(format!("{to} isn't a session this capture lists"));
            };
            if always && current.rule.is_none() && current.send.offered().is_none() {
                return Err("no rule can be made for this capture's site or app".into());
            }
            self.choose(dest, always);
            let current = self.current.as_ref().ok_or("no capture")?;
            return Ok(current
                .send
                .delivery()
                .as_ref()
                .map_or_else(|| "Nothing sent".into(), describe));
        }
        if always {
            return Err("only the last routed capture can save a rule".into());
        }
        let dest = match to {
            Some(to) if to.starts_with("ses") => Destination {
                title: session_name(to, to, &[]),
                session: to.to_owned(),
                via: "chosen".into(),
                url: None,
                directory: None,
            },
            Some(_) => return Err("an older capture is sent to a session id (ses…)".into()),
            None => recorded_destination(bundle)
                .ok_or("it was never sent anywhere; say where with --to")?,
        };
        let loaded = scrawl_capture::handoff::load_bundle(bundle)
            .map_err(|error| format!("couldn't read the capture: {error:#}"))?;
        let delivery =
            deliver(&router(), bundle, &dest, loaded.1.target.app, self.forward).delivery();
        tracing::info!(
            bundle = %bundle_name(bundle),
            session = dest.session,
            delivery = delivery_kind(&delivery),
            "capture sent from the CLI"
        );
        match delivery {
            RouteDelivery::Failed { why, .. } => Err(why),
            delivery => Ok(describe(&delivery)),
        }
    }

    /// The capsule's pick: sends the newest capture to `to` now, after
    /// taking back a send elsewhere. `always` also saves a rule for its
    /// site or app.
    fn chosen(&mut self, to: &str, always: bool) {
        if let Some(dest) = self.resolve(to) {
            self.choose(dest, always);
        }
    }

    /// Saves a rule chosen from the post-send `Always…` configurator and
    /// attaches it to the current capture so Undo can also revert it.
    fn save_rule(&mut self, kind: RuleKind, key: String, target: String) {
        if let Some(saved) = persist_rule((kind, key, target))
            && let Some(current) = self.current.as_mut()
        {
            current.send.attach_saved_rule(saved);
        }
    }

    fn choose(&mut self, dest: Destination, always: bool) {
        self.pending = None;
        let rule_spec = if always {
            let spec = self.rule_for(&dest.session);
            if spec.is_none() {
                tracing::info!("no rule can be made for this capture");
            }
            spec
        } else {
            None
        };
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.send.cancel_pending_for_pick();
        match current.send.pick_action(&dest.session) {
            // Accepting the offered rule, or a second click.
            PickAction::AlreadyThere => {
                if let Some(spec) = rule_spec
                    && let Some(saved) = persist_rule(spec)
                    && let Some(current) = self.current.as_mut()
                {
                    current.send.attach_saved_rule(saved);
                }
            }
            PickAction::Redirect => self.redirect_to(dest, rule_spec),
            PickAction::SendFresh => {
                self.correct(jev::Action::Redirect, "chip", Some(&dest));
                self.send_current(dest, Some("chosen"), rule_spec);
            }
        }
    }

    fn redirect_to(&mut self, dest: Destination, rule_spec: Option<(RuleKind, String, String)>) {
        self.take_back();
        self.correct(jev::Action::Redirect, "reroute", Some(&dest));
        self.send_current(dest, Some("chosen"), rule_spec);
    }

    fn hold(&mut self, on: bool) {
        if let Some(current) = &mut self.current {
            current.send.hold(on, Instant::now());
        }
    }

    fn undo(&mut self) {
        if !self.acting_on_last() {
            return;
        }
        self.pending = None;
        if self
            .current
            .as_mut()
            .is_some_and(|current| current.send.cancel_scheduled())
        {
            self.publish();
            return;
        }
        if let Some(saved) = self
            .current
            .as_mut()
            .and_then(|current| current.send.take_saved_rule())
        {
            forget_rule(&saved);
        }
        if self.take_back() {
            self.publish();
        }
    }

    /// Takes back the current capture's send, whatever state it is in;
    /// `false` when there was none. One OpenCode wouldn't give back, or one
    /// delivered later by a background retry without an undo handle, is too
    /// late.
    fn take_back(&mut self) -> bool {
        let Some(current) = self.current.as_mut() else {
            return false;
        };
        match current.send.begin_undo() {
            UndoPlan::Handle { session, to, sent } => {
                let undone = current.router.undo(&sent.delivery_id, UNDO_DEADLINE);
                current.send = match undone {
                    Ok(report) if report.undone != Undone::Unfinished => {
                        tracing::info!(
                            bundle = %bundle_name(&current.bundle),
                            undone = report.undone.label(),
                            "capture send taken back"
                        );
                        match report.undone {
                            Undone::Withdrawn | Undone::Reverted | Undone::Disowned => {
                                SendState::TakenBack
                            }
                            Undone::Gone | Undone::TooOld | Undone::Unfinished => {
                                SendState::TooLate {
                                    session,
                                    to,
                                    retry_undo: None,
                                }
                            }
                        }
                    }
                    // Still where it went (or staged for removal, which
                    // another undo can finish): the capsule, already showing
                    // it taken back, hears that it's too late.
                    failed => {
                        let error = match failed {
                            Ok(report) => report.text(),
                            Err(error) => format!("{error:#}"),
                        };
                        tracing::warn!(error = %error, "could not take a capture's send back");
                        SendState::TooLate {
                            session,
                            to,
                            retry_undo: Some(sent),
                        }
                    }
                };
                true
            }
            UndoPlan::RetriedNoHandle => true,
            UndoPlan::Queued => {
                // Its retries stop with the marker.
                let _ = std::fs::remove_file(current.bundle.join(delivery::PENDING));
                self.correct(jev::Action::Cancel, "dismiss", None);
                true
            }
            UndoPlan::None => false,
        }
    }

    fn send_current(
        &mut self,
        mut dest: Destination,
        via: Option<&str>,
        rule_spec: Option<(RuleKind, String, String)>,
    ) {
        let started = Instant::now();
        if let Some(via) = via {
            via.clone_into(&mut dest.via);
        }
        if let Some(current) = self.current.as_mut() {
            dest.title = session_name(&dest.session, &dest.title, &current.candidates);
            current.send = SendState::Sending {
                to: dest.title.clone(),
            };
        }
        self.publish();
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let outcome = deliver(
            &current.router,
            &current.bundle,
            &dest,
            current.app.clone(),
            self.forward,
        );
        let delivery = outcome.delivery();
        tracing::info!(
            bundle = %bundle_name(&current.bundle),
            session = dest.session,
            delivery = delivery_kind(&delivery),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "capture sent"
        );
        let saved = if outcome.succeeded_or_queued() {
            rule_spec.and_then(persist_rule)
        } else {
            None
        };
        current.send = outcome.into_state(saved);
        self.publish();
    }

    /// The always-rule sending the current capture's site or app to
    /// `session` itself: its site or app, else the key of the rule offered
    /// after its send. Never `session`'s folder: a folder rule sends to the
    /// newest session in that repository, which is another session
    /// whenever two work in it (a worktree's beside its main checkout's).
    fn rule_for(&self, session: &str) -> Option<(RuleKind, String, String)> {
        let current = self.current.as_ref()?;
        let (kind, key) = current.rule.clone().or_else(|| {
            let offer = current.send.offered()?;
            Some((offer.kind, offer.key.clone()))
        })?;
        Some((kind, key, session.to_owned()))
    }

    /// Logs the person correcting or confirming the router (with Jev routing
    /// on); `detail` is how (`chip`, `reroute`, `dismiss`).
    fn correct(&self, action: jev::Action, detail: &str, to: Option<&Destination>) {
        if let Some(current) = &self.current {
            current.router.run(|| {
                jev::correct_capture(
                    &current.bundle,
                    action,
                    detail,
                    to.map(|to| (to.session.as_str(), to.title.as_str())),
                );
            });
        }
    }

    /// The session `to` names for the newest capture: a session ID, or a
    /// name from its route, suggestions first. `None` (logged) when there
    /// is no routed capture or nothing matches.
    fn resolve(&self, to: &str) -> Option<Destination> {
        if !self.acting_on_last() {
            return None;
        }
        self.listed_as(to)
    }

    /// The routed capture's session named `to`: listed by id or title, or
    /// any session id.
    fn listed_as(&self, to: &str) -> Option<Destination> {
        let current = self.current.as_ref()?;
        let to = to.trim();
        let listed = listed(&current.answer);
        let found = listed.iter().find(|dest| dest.session == to).or_else(|| {
            listed
                .iter()
                .find(|dest| dest.name.eq_ignore_ascii_case(to))
        });
        let (session, title) = match found {
            Some(dest) => (dest.session.clone(), dest.name.clone()),
            None if to.starts_with("ses") => {
                (to.to_owned(), session_name(to, to, &current.candidates))
            }
            None => {
                tracing::info!("the capsule named a session the route doesn't list");
                return None;
            }
        };
        let directory = current
            .candidates
            .iter()
            .find(|candidate| candidate.id == session)
            .and_then(|candidate| candidate.directory.as_ref())
            .map(|dir| dir.display().to_string());
        Some(Destination {
            session,
            title,
            via: "chosen".into(),
            url: current.url.clone(),
            directory,
        })
    }

    /// Commands act on the snapshot's newest capture only, and only when
    /// it is the one routed.
    fn acting_on_last(&self) -> bool {
        let last = lock(&self.outcomes)
            .last
            .as_ref()
            .map(|last| last.bundle.clone());
        self.current
            .as_ref()
            .is_some_and(|current| Some(&current.bundle) == last.as_ref())
    }

    fn is_current(&self, bundle: &Path) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.bundle == bundle)
    }

    fn publish(&self) {
        if let Some(current) = &self.current {
            lock(&self.outcomes).routed(&current.bundle, current.capture_route());
        }
    }

    /// Applies the bundles a background retry pass just delivered: moves a
    /// matching queued current capture into [`SendState::Delivered`] (with
    /// `undo: None`) while keeping its session ID and any saved rule.
    fn apply_retried(&mut self, sent_bundles: &[PathBuf]) {
        let matched = self
            .current
            .as_mut()
            .filter(|current| sent_bundles.contains(&current.bundle))
            .is_some_and(|current| current.send.mark_retried());
        if matched {
            self.publish();
        }
    }

    /// Retries sends OpenCode didn't take, unless another process owns
    /// retries: Scrawl's app, or anything serving Scrawl's ingest port.
    fn retry(&mut self) {
        if cfg!(test)
            || opencode_managed()
            || crate::app_settings::scrawl_app_running()
            || ingest_port_served()
        {
            return;
        }
        let router = router();
        let captures = crate::app_settings::capture_settings().captures_dir();
        let report = router.run(|| {
            delivery::retry_due(
                &captures,
                now_ms(),
                scrawl_capture::handoff::send_bundle_to_session,
            )
        });
        let outbox = router.retry_outbox();
        if !report.sent.is_empty() || !outbox.sent.is_empty() {
            tracing::info!(
                captures = report.sent.len(),
                prompts = outbox.sent.len(),
                "retried sends went"
            );
        }
        let sent_bundles: Vec<PathBuf> = report.sent.into_iter().map(|(dir, _)| dir).collect();
        self.apply_retried(&sent_bundles);
    }
}

/// What the router made of a capture, before it is shown.
enum Decided {
    Judged(smart::Decision),
    /// OpenCode's sessions couldn't be read: why, in a few words.
    Unreachable(String),
}

/// Why a look read no sessions at all, when its read failed.
fn sessions_unread(live: &look::Live) -> Option<String> {
    live.candidates
        .is_empty()
        .then(|| live.error.clone())
        .flatten()
}

/// A decision as shown: nowhere to go for want of the sessions reads
/// Unreachable; anything a clue named outright still routes.
fn judged(decision: smart::Decision, unread: Option<&str>) -> Decided {
    match (decision, unread) {
        (smart::Decision::Keep { .. }, Some(error)) => Decided::Unreachable(unreachable_why(error)),
        (decision, _) => Decided::Judged(decision),
    }
}

/// A failed session read (`couldn't list sessions: …`, outer context
/// first) in the few words the capsule shows under "Couldn't reach
/// OpenCode".
fn unreachable_why(error: &str) -> String {
    let error = ["couldn't list sessions: ", "couldn't list every session: "]
        .into_iter()
        .find_map(|prefix| error.strip_prefix(prefix))
        .unwrap_or(error);
    if auth_refused(error) {
        AUTH_REFUSED.into()
    } else if error.contains("didn't answer in time") || error.contains("took over") {
        "didn't answer in time".into()
    } else if error.contains("Could not reach server") {
        "it isn't running".into()
    } else {
        error.to_owned()
    }
}

/// A send to an OpenCode server on another Mac, with no agent host to copy
/// the bundle there first.
const FILES_UNSEEN: &str = "OpenCode can't see files";

/// OpenCode answered but refused Hex's credentials (HTTP 401 or 403): the
/// password in its registration was missing or didn't match, as when the
/// service runs in WSL and writes its registration inside WSL's home.
const AUTH_REFUSED: &str = "OpenCode needs a password Hex doesn't have";

/// What process.log says beside [`AUTH_REFUSED`]; it never names the
/// password itself.
const AUTH_HINT: &str = "OpenCode refused Hex's password. Hex reads it from OpenCode's \
                         service.json under this user's ~/.local/state/opencode (or \
                         OPENCODE_PASSWORD); on Windows, turn on Labs › OpenCode in WSL \
                         when OpenCode runs there. The capture is saved and copied either way";

/// Whether a failed request (`{err:#}`) was OpenCode refusing the
/// credentials.
fn auth_refused(error: &str) -> bool {
    ["HTTP 401", "HTTP 403", "401 Unauthorized", "403 Forbidden"]
        .iter()
        .any(|status| error.contains(status))
}

/// A failed send (`{err:#}`, outer context first) in the few words the
/// capsule shows under "Copied". What OpenCode said comes first: it was
/// reached. `agent_host` is whether bundles are copied to its Mac first.
fn send_why(error: &str, agent_host: bool) -> String {
    let fit = |line: &str| match line.char_indices().nth(60) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_owned(),
    };
    // The server reads the contact sheet from its own disk.
    if error.contains("Unable to read attachment") && !agent_host {
        return FILES_UNSEEN.into();
    }
    if auth_refused(error) {
        return AUTH_REFUSED.into();
    }
    if error.contains("HTTP 404") {
        return "session not found".into();
    }
    if let Some(said) = error.strip_prefix("OpenCode said: ") {
        let said = said.split(": OpenCode API failed").next().unwrap_or(said);
        return fit(said.lines().next().unwrap_or(said));
    }
    if error.starts_with("Could not reach server") {
        "OpenCode isn't running".into()
    } else if error.contains("didn't answer in time") {
        "OpenCode didn't answer".into()
    } else {
        fit(&delivery::short(error))
    }
}

/// `routing.sites`, `routing.apps` or `routing.terminals`, as `kind` says.
fn rules_of(
    config: &scrawl_capture::RouterConfig,
    kind: RuleKind,
) -> &std::collections::BTreeMap<String, String> {
    match kind {
        RuleKind::Site => &config.sites,
        RuleKind::App => &config.apps,
        RuleKind::Terminal => &config.terminals,
    }
}

/// What the capsule calls the thing a rule would match: a site's origin
/// or an app's bundle ID as they are, and a terminal by its folder's name
/// ("hex terminal").
fn site_of((kind, key): &(RuleKind, String)) -> String {
    match kind {
        RuleKind::Site | RuleKind::App => key.clone(),
        RuleKind::Terminal => {
            let name = Path::new(key)
                .file_name()
                .map_or_else(|| key.clone(), |n| n.to_string_lossy().into_owned());
            format!("{name} terminal")
        }
    }
}

/// Saves `settings.json`'s routing as `edit` makes it, then has the app
/// reload its settings.
fn edit_routing(
    edit: impl FnOnce(&scrawl_capture::RouterConfig) -> color_eyre::Result<scrawl_capture::RouterConfig>,
) -> color_eyre::Result<()> {
    let mut settings = crate::app_settings::AppSettings::load()?;
    let base = settings.clone();
    let next = edit(settings.capture.routing.config())?;
    settings.capture.routing = next.into();
    base.update(&settings)?;
    #[cfg(target_os = "macos")]
    if let Some(runtime) = crate::macos_rune::runtime() {
        let _ = runtime.send(crate::macos_rune::Request::ReloadSettings);
    }
    Ok(())
}

/// Saves an always-rule into `settings.json`, returning the undo record when
/// it wrote cleanly.
fn persist_rule((kind, key, target): (RuleKind, String, String)) -> Option<SavedRule> {
    let mut saved = None;
    let result = edit_routing(|config| {
        let key = key.trim();
        saved = Some(SavedRule {
            kind,
            key: key.to_owned(),
            before: rules_of(config, kind).get(key).cloned(),
            was_refused: config.refused_rules.iter().any(|k| k == key),
        });
        config
            .with_rule(kind, key, &target)
            .map_err(|error| color_eyre::eyre::eyre!("{error:#}"))
    });
    match result {
        Ok(()) => {
            tracing::info!("saved a capture routing rule");
            saved
        }
        Err(error) => {
            tracing::warn!(%error, "could not save a capture routing rule");
            None
        }
    }
}

/// Undo of a Shift pick: the rule it saved goes, and what its key held
/// before comes back.
fn forget_rule(saved: &SavedRule) {
    let result = edit_routing(|config| {
        let mut next = config.clone();
        let rules = match saved.kind {
            RuleKind::Site => &mut next.sites,
            RuleKind::App => &mut next.apps,
            RuleKind::Terminal => &mut next.terminals,
        };
        match &saved.before {
            Some(before) => rules.insert(saved.key.clone(), before.clone()),
            None => rules.remove(&saved.key),
        };
        if saved.was_refused && !next.refused_rules.contains(&saved.key) {
            next.refused_rules.push(saved.key.clone());
        }
        Ok(next)
    });
    match result {
        Ok(()) => tracing::info!("took back a capture routing rule"),
        Err(error) => tracing::warn!(%error, "could not take back a capture routing rule"),
    }
}

/// The answer to show, and the destination of a rule's send.
fn answer_for(
    decided: Decided,
    url: Option<&str>,
    site: Option<String>,
    candidates: &[smart::Candidate],
    now_ms: u64,
    jev_auto_send: bool,
) -> Answered {
    let scored = |scored: &smart::Scored| {
        let dest = smart::destination_of(scored, url);
        RouteDest {
            session: dest.session,
            name: dest.title,
            why: why(&dest.via).to_owned(),
            directory: scored.candidate.directory.clone(),
        }
    };
    let recent = |shown: &[&str]| recent(candidates, shown, now_ms);
    let guess = |suggested: Vec<RouteDest>, why: String| {
        let shown: Vec<&str> = suggested.iter().map(|d| d.session.as_str()).collect();
        let recent = recent(&shown);
        Answered {
            answer: RouteAnswer::Guess {
                suggested,
                recent,
                site: site.clone(),
            },
            send: None,
            shown: Some(Shown::guess(why)),
        }
    };
    let auto_by = |to: &smart::Scored| match to.lead().map(|r| r.basis) {
        Some(smart::Basis::Clue(
            smart::Source::Rule | smart::Source::AppRule | smart::Source::TerminalRule,
        )) => Some(AutoSendBy::Rule),
        Some(smart::Basis::Jev) if jev_auto_send => Some(AutoSendBy::Jev),
        _ => None,
    };
    let mut answered = match decided {
        // Saved site, terminal, or app rules always auto-send; confident Jev
        // picks auto-send only when `Auto-send with Jev` is enabled.
        Decided::Judged(smart::Decision::Send { to, instead }) if auto_by(&to).is_some() => {
            let by = auto_by(&to).expect("checked");
            let dest = smart::destination_of(&to, url);
            let shown = Shown::rule(lead_why(&to));
            let to = scored(&to);
            let suggested: Vec<_> = instead.iter().map(scored).collect();
            let mut listed: Vec<&str> = suggested.iter().map(|d| d.session.as_str()).collect();
            listed.push(&to.session);
            let recent = recent(&listed);
            Answered {
                answer: RouteAnswer::Rule {
                    to,
                    suggested,
                    recent,
                    site,
                    by,
                },
                send: Some(dest),
                shown: Some(shown),
            }
        }
        Decided::Judged(smart::Decision::Send { to, instead }) => {
            let why = lead_why(&to);
            let suggested = std::iter::once(&to).chain(&instead).map(scored).collect();
            guess(suggested, why)
        }
        Decided::Judged(smart::Decision::Offer { choices }) => {
            let why = choices.first().map(lead_why).unwrap_or_default();
            guess(choices.iter().map(scored).collect(), why)
        }
        Decided::Judged(smart::Decision::Keep { .. }) => Answered::of(RouteAnswer::Nothing {
            recent: recent(&[]),
        }),
        Decided::Unreachable(why) => Answered::of(RouteAnswer::Unreachable { why }),
    };
    // The early local read, a saved rule, or a list read without a title can
    // leave a `ses…` id where a name goes; the capsule and the bundle say the
    // title instead.
    name_answer(&mut answered.answer, answered.send.as_mut(), candidates);
    answered
}

/// What [`answer_for`] made of a decision.
struct Answered {
    answer: RouteAnswer,
    /// A saved rule's send, which goes at once.
    send: Option<Destination>,
    /// Rule or guess, and why, for the capture's `routing.json`.
    shown: Option<Shown>,
}

impl Answered {
    fn of(answer: RouteAnswer) -> Self {
        Self {
            answer,
            send: None,
            shown: None,
        }
    }
}

/// The reason that put `to` first, as the router wrote it ("Jev couldn't
/// tell, so HOME").
fn lead_why(to: &smart::Scored) -> String {
    to.lead()
        .or_else(|| to.reasons.first())
        .map(|reason| reason.why.clone())
        .unwrap_or_default()
}

/// Every session read and not already shown, newest first, with how long
/// ago each moved: the capsule lists the newest and searches them all.
fn recent(candidates: &[smart::Candidate], shown: &[&str], now_ms: u64) -> Vec<RouteDest> {
    let mut recent: Vec<_> = candidates
        .iter()
        .filter(|candidate| !shown.contains(&candidate.id.as_str()))
        .collect();
    recent.sort_by_key(|candidate| std::cmp::Reverse(candidate.updated_ms));
    recent
        .into_iter()
        .map(|candidate| RouteDest {
            session: candidate.id.clone(),
            name: candidate.label().to_owned(),
            why: crate::desktop::i18n::event_age_at(candidate.updated_ms, now_ms),
            directory: candidate.directory.clone(),
        })
        .collect()
}

/// A destination's `via` as a quiet word: a real reason in plain words,
/// or nothing (a pick with no reason behind it, such as Jev's).
fn why(via: &str) -> &str {
    match via {
        "rule" | "app-rule" | "terminal-rule" => "rule",
        "signed" => "page names it",
        "named" | "speech" => "mentioned",
        "sdk" | "page" => "same page",
        "viewed" => "you were just in it",
        "localhost" | "port" => "dev server",
        "recorded" | "chosen" => "sent before",
        "publisher" => "published it",
        "dev-build" => "dev build",
        "terminal-session" => "open in this terminal",
        "terminal" => "same terminal",
        "document" => "same file",
        "app" => "same app",
        "title" => "same window",
        "weighed" => "recent",
        _ => "",
    }
}

/// Every session the route lists, suggestions first.
fn listed(answer: &RouteAnswer) -> Vec<&RouteDest> {
    match answer {
        RouteAnswer::Looking
        | RouteAnswer::ResolvingRule { .. }
        | RouteAnswer::Unreachable { .. } => Vec::new(),
        RouteAnswer::Rule {
            to,
            suggested,
            recent,
            ..
        } => std::iter::once(to).chain(suggested).chain(recent).collect(),
        RouteAnswer::Guess {
            suggested, recent, ..
        } => suggested.iter().chain(recent).collect(),
        RouteAnswer::Nothing { recent } | RouteAnswer::NoAnswer { recent } => {
            recent.iter().collect()
        }
    }
}

/// Where a capture's bundle says it was sent.
fn recorded_destination(bundle: &Path) -> Option<Destination> {
    scrawl_capture::handoff::load_bundle(bundle)
        .ok()
        .and_then(|(_, loaded)| loaded.destination)
}

/// A delivery as a sentence for `hex capture send`.
fn describe(delivery: &RouteDelivery) -> String {
    match delivery {
        RouteDelivery::Sending { to } => format!("Sending to {to}"),
        RouteDelivery::Sent { to } => format!("Sent to {to}"),
        RouteDelivery::Queued { to, why } => format!("Queued for {to}: {why}"),
        RouteDelivery::TakenBack => "Taken back".into(),
        RouteDelivery::TooLate { to } => format!("Already read in {to}"),
        RouteDelivery::Failed { to, why } => format!("Not sent to {to}: {why}"),
    }
}

/// Records where the capture went in its bundle, sends it, and remembers
/// the send for an undo. A send OpenCode didn't take is queued for
/// [`Worker::retry`].
fn deliver(
    router: &Router,
    bundle: &Path,
    dest: &Destination,
    app: Option<String>,
    forward: Forward,
) -> DeliverOutcome {
    if opencode_managed() {
        return DeliverOutcome::Failed {
            to: dest.title.clone(),
            why: crate::policy::Lock::OpenCodeProcessing.refusal().into(),
        };
    }
    router.run(|| {
        // The bundle and its summary name the session too, so they say its
        // title rather than a `ses…` id when one reached this far.
        let titled = Destination {
            title: session_name(&dest.session, &dest.title, &[]),
            ..dest.clone()
        };
        let dest = &titled;
        let recorded = scrawl_capture::handoff::load_bundle(bundle).and_then(
            |(_, mut loaded): (PathBuf, ScrawlBundle)| {
                routing::record_destination(bundle, &mut loaded, dest.clone())
            },
        );
        if let Err(error) = recorded {
            tracing::warn!(error = %format!("{error:#}"), "could not record a capture's destination");
        }
        let (delivery, posted) = delivery::deliver_by(
            bundle,
            &dest.session,
            &dest.title,
            None,
            forward,
        );
        let undo = posted.map(|posted| {
            let sent = undo::remember(
                &dest.session,
                Some(&dest.title),
                &posted,
                undo::Origin {
                    kind: "capture".into(),
                    bundle: Some(bundle.to_path_buf()),
                    app,
                    ..undo::Origin::default()
                },
            );
            Sent {
                delivery_id: sent.delivery_id,
            }
        });
        let to = dest.title.clone();
        match delivery {
            Delivery::Sent { .. } => DeliverOutcome::Delivered {
                session: dest.session.clone(),
                to,
                undo,
                offer: routing::learn_capture(bundle, &dest.session),
            },
            Delivery::Queued { error, .. } => {
                let agent_host = router.config().agent_host().is_some();
                let why = send_why(&error, agent_host);
                if why == FILES_UNSEEN {
                    tracing::warn!(
                        error = delivery::short(&error),
                        "OpenCode can't read a capture from its own disk; if it runs on another \
                         Mac, set capture.routing.agentHost to that Mac's SSH host. Retrying"
                    );
                } else if why == AUTH_REFUSED {
                    tracing::warn!("{AUTH_HINT}. Retrying");
                } else {
                    tracing::warn!(
                        error = delivery::short(&error),
                        "a capture's send didn't go; retrying"
                    );
                }
                DeliverOutcome::Queued {
                    session: dest.session.clone(),
                    to,
                    why,
                }
            }
            Delivery::Saved => DeliverOutcome::Failed {
                to,
                why: "nothing was sent".into(),
            },
        }
    })
}

/// Whether something serves Scrawl's ingest port: that process retries
/// deliveries, and two must never race over a bundle.
fn ingest_port_served() -> bool {
    let port = scrawl_capture::phone::DEFAULT_INGEST_PORT;
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(100),
    )
    .is_ok()
}

fn bundle_name(bundle: &Path) -> String {
    bundle
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn answer_kind(answer: &RouteAnswer) -> &'static str {
    match answer {
        RouteAnswer::Looking => "looking",
        RouteAnswer::ResolvingRule { .. } => "resolving-rule",
        RouteAnswer::Rule { .. } => "rule",
        RouteAnswer::Guess { .. } => "guess",
        RouteAnswer::Nothing { .. } => "nothing",
        RouteAnswer::NoAnswer { .. } => "no-answer",
        RouteAnswer::Unreachable { .. } => "unreachable",
    }
}

fn delivery_kind(delivery: &RouteDelivery) -> &'static str {
    match delivery {
        RouteDelivery::Sending { .. } => "sending",
        RouteDelivery::Sent { .. } => "sent",
        RouteDelivery::Queued { .. } => "queued",
        RouteDelivery::TakenBack => "taken-back",
        RouteDelivery::TooLate { .. } => "too-late",
        RouteDelivery::Failed { .. } => "failed",
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, title: &str, updated_ms: u64) -> smart::Candidate {
        smart::Candidate {
            id: id.into(),
            title: title.into(),
            directory: Some(PathBuf::from(format!("/tmp/{id}"))),
            repo: None,
            updated_ms,
            viewed_ms: None,
        }
    }

    fn scored(id: &str, title: &str) -> smart::Scored {
        smart::Scored {
            candidate: candidate(id, title, 0),
            score: 60,
            reasons: Vec::new(),
        }
    }

    /// `id` scored on one reason, as the router writes it.
    fn scored_by(id: &str, title: &str, basis: smart::Basis, why: &str) -> smart::Scored {
        smart::Scored {
            reasons: vec![smart::Reason {
                basis,
                points: 60,
                why: why.into(),
                directory: None,
            }],
            ..scored(id, title)
        }
    }

    fn sent_by(basis: smart::Basis, why: &str) -> Answered {
        sent_by_with_jev(basis, why, false)
    }

    fn sent_by_with_jev(basis: smart::Basis, why: &str, jev_auto_send: bool) -> Answered {
        answer_for(
            Decided::Judged(smart::Decision::Send {
                to: scored_by("ses_a", "Alpha", basis, why),
                instead: vec![scored("ses_c", "Gamma")],
            }),
            Some("https://example.com/page"),
            Some("example.com".into()),
            &[
                candidate("ses_a", "Alpha", 0),
                candidate("ses_c", "Gamma", 0),
            ],
            0,
            jev_auto_send,
        )
    }

    /// Kit's Nov 4 capture: a saved app rule named a session by id before the
    /// session server answered, and the pill said `Sent to ses_…`. A name is
    /// a title, or the word "session".
    #[test]
    fn a_session_is_named_by_its_title_never_its_id() {
        let candidates = [candidate("ses_seer", "SEER", 0)];
        let no_lookup = |_: &str| panic!("a title needs no lookup");
        assert_eq!(
            session_name_by("ses_seer", "SEER", &candidates, no_lookup),
            "SEER"
        );
        // A rule that named only the id: the capture's list has the title.
        assert_eq!(
            session_name_by("ses_seer", "ses_seer", &candidates, |_| None),
            "SEER"
        );
        // Not in the list: OpenCode's title.
        assert_eq!(
            session_name_by("ses_x", "ses_x", &candidates, |_| Some("HOME".into())),
            "HOME"
        );
        // Nowhere at all: "session", never the id.
        assert_eq!(
            session_name_by("ses_x", "ses_x", &candidates, |_| None),
            "session"
        );
        assert_eq!(
            session_name_by("ses_x", "ses_x", &candidates, |_| Some(String::new())),
            "session"
        );
        assert_eq!(session_name("ses_x", "", &candidates), "session");
        assert_eq!(
            session_name_by("ses_seer", "SEER", &candidates, |_| Some("HOME".into())),
            "SEER",
            "a title already read is kept"
        );
    }

    /// The answer and its send carry a name even when the router handed over
    /// only a session id.
    #[test]
    fn a_rule_answer_and_send_are_named_by_title() {
        let rule = |title: &str| {
            answer_for(
                Decided::Judged(smart::Decision::Send {
                    to: scored_by(
                        "ses_seer",
                        title,
                        smart::Basis::Clue(smart::Source::AppRule),
                        "routing.apps sends com.kitlangton.seer to ses_seer",
                    ),
                    instead: Vec::new(),
                }),
                None,
                None,
                &[candidate("ses_seer", "SEER", 0)],
                0,
                false,
            )
        };
        for answered in [rule(""), rule("ses_seer")] {
            let RouteAnswer::Rule { to, .. } = &answered.answer else {
                panic!("not a rule: {:?}", answered.answer);
            };
            assert_eq!(to.name, "SEER", "the pill's name");
            assert_eq!(
                answered.send.map(|dest| dest.title),
                Some("SEER".into()),
                "the bundle and summary's name"
            );
        }
        // Nothing read and no server: words, never the id.
        let answered = answer_for(
            Decided::Judged(smart::Decision::Send {
                to: scored("ses_seer", ""),
                instead: Vec::new(),
            }),
            None,
            None,
            &[],
            0,
            false,
        );
        let RouteAnswer::Guess { suggested, .. } = &answered.answer else {
            panic!("not a guess: {:?}", answered.answer);
        };
        assert_eq!(suggested[0].name, "session");
    }

    #[test]
    fn only_a_saved_rule_sends_on_its_own() {
        use smart::{Basis, Source};
        for (basis, why, shown) in [
            (Basis::JevHome, "Jev couldn't tell, so HOME", ""),
            (Basis::Jev, "Jev's pick", ""),
            (
                Basis::Clue(Source::Signed),
                "page names a session",
                "page names it",
            ),
            (
                Basis::Clue(Source::Port),
                "port 5173 is Alpha's",
                "dev server",
            ),
            (
                Basis::Clue(Source::Page),
                "the page registered with data-session",
                "same page",
            ),
            (
                Basis::Clue(Source::Publisher),
                "Alpha published the file",
                "published it",
            ),
            (Basis::Clue(Source::Named), "you said alpha", "mentioned"),
        ] {
            let answered = sent_by(basis, why);
            assert!(answered.send.is_none(), "{why}: nothing sends until picked");
            let RouteAnswer::Guess {
                suggested, site, ..
            } = &answered.answer
            else {
                panic!("{why}: not a guess: {:?}", answered.answer);
            };
            assert_eq!(
                suggested[0].session, "ses_a",
                "{why}: the router's pick first"
            );
            assert_eq!(suggested[0].why, shown);
            assert_eq!(suggested[1].session, "ses_c");
            // So "Always send example.com here" can make it a rule.
            assert_eq!(site.as_deref(), Some("example.com"));
            assert_eq!(answered.shown, Some(Shown::guess(why)));
        }
        for (basis, why) in [
            (
                Basis::Clue(Source::Rule),
                "routing.sites sends https://example.com to /p/a",
            ),
            (
                Basis::Clue(Source::AppRule),
                "routing.apps sends com.example.app to /p/a",
            ),
            (
                Basis::Clue(Source::TerminalRule),
                "routing.terminals sends /p/a",
            ),
        ] {
            let answered = sent_by(basis, why);
            assert_eq!(
                answered.send.map(|dest| dest.session),
                Some("ses_a".into()),
                "{why}: sent at once"
            );
            assert!(matches!(
                answered.answer,
                RouteAnswer::Rule {
                    ref to,
                    by: AutoSendBy::Rule,
                    ..
                } if to.why == "rule"
            ));
            assert_eq!(answered.shown, Some(Shown::rule(why)));
        }
        // With Auto-send with Jev on, Jev's confident pick auto-sends with
        // Jev provenance; JevHome fallback and non-rule clues still suggest.
        let jev_send = sent_by_with_jev(Basis::Jev, "Jev gave it 0.96", true);
        assert!(jev_send.send.is_some());
        assert!(matches!(
            jev_send.answer,
            RouteAnswer::Rule {
                by: AutoSendBy::Jev,
                ..
            }
        ));
        let jev_home = sent_by_with_jev(Basis::JevHome, "Jev couldn't tell, so HOME", true);
        assert!(jev_home.send.is_none());
        assert!(matches!(jev_home.answer, RouteAnswer::Guess { .. }));
    }

    #[test]
    fn a_rule_sends_and_lists_the_rest() {
        let candidates = [
            candidate("ses_a", "Alpha", 1_000),
            candidate("ses_b", "Beta", 3_000),
            candidate("ses_c", "Gamma", 2_000),
        ];
        let Answered { answer, send, .. } = answer_for(
            Decided::Judged(smart::Decision::Send {
                to: scored_by(
                    "ses_a",
                    "Alpha",
                    smart::Basis::Clue(smart::Source::Rule),
                    "routing.sites sends https://example.com to /tmp/ses_a",
                ),
                instead: vec![scored("ses_c", "Gamma")],
            }),
            None,
            None,
            &candidates,
            63_000,
            false,
        );
        assert_eq!(send.map(|dest| dest.session), Some("ses_a".into()));
        let RouteAnswer::Rule {
            to,
            suggested,
            recent,
            ..
        } = answer
        else {
            panic!("not a rule: {answer:?}");
        };
        assert_eq!(to.name, "Alpha");
        assert_eq!(suggested.len(), 1);
        assert_eq!(suggested[0].session, "ses_c");
        assert_eq!(
            recent,
            vec![RouteDest {
                session: "ses_b".into(),
                name: "Beta".into(),
                why: "1m ago".into(),
                directory: Some("/tmp/ses_b".into()),
            }]
        );
    }

    #[test]
    fn a_guess_sends_nothing_and_a_failure_still_lists_recent_sessions() {
        let candidates = [candidate("ses_a", "Alpha", 0)];
        let Answered { answer, send, .. } = answer_for(
            Decided::Judged(smart::Decision::Offer {
                choices: vec![scored("ses_a", "Alpha")],
            }),
            None,
            Some("example.com".into()),
            &candidates,
            0,
            false,
        );
        assert!(send.is_none());
        assert!(matches!(
            answer,
            RouteAnswer::Guess { ref suggested, ref recent, site: Some(ref site) }
                if suggested.len() == 1 && recent.is_empty() && site == "example.com"
        ));
    }

    /// The Air's first sends: Studio's OpenCode couldn't read the contact
    /// sheet yet, and the capsule said only "couldn't reach SEER".
    #[test]
    fn a_failed_send_says_what_went_wrong() {
        let unread = "OpenCode said: Unable to read attachment: file:///Users/alex/Library/\
                      Application%20Support/scrawl/captures/1.scrawl/contact-sheet.png: \
                      OpenCode API failed: HTTP 400 Bad Request";
        assert_eq!(send_why(unread, false), FILES_UNSEEN);
        assert_eq!(
            send_why(unread, true),
            "Unable to read attachment: file:///Users/alex/Library/Applic…"
        );
        let cases = [
            (
                "OpenCode said: Skill not found: x: OpenCode API failed: HTTP 400 Bad Request",
                "Skill not found: x",
            ),
            (
                "Could not reach server at http://127.0.0.1:4096: Connection refused (os error 61)",
                "OpenCode isn't running",
            ),
            (
                "OpenCode at http://127.0.0.1:4096 didn't answer in time: Resource temporarily \
                 unavailable",
                "OpenCode didn't answer",
            ),
            (
                "OpenCode API failed: HTTP 404 Not Found",
                "session not found",
            ),
            (
                "OpenCode said: Session not found: ses_x: OpenCode API failed: HTTP 404 Not Found",
                "session not found",
            ),
            ("bundle has no contact sheet", "bundle has no contact sheet"),
            (
                "OpenCode said: Unauthorized: OpenCode API failed: HTTP 401",
                AUTH_REFUSED,
            ),
            ("OpenCode API failed: HTTP 403", AUTH_REFUSED),
        ];
        for (error, why) in cases {
            assert_eq!(send_why(error, false), why, "{error}");
        }
        let long = send_why(&"x".repeat(100), false);
        assert_eq!(long.chars().count(), 61);
        assert!(long.ends_with('…'));
        assert_eq!(
            describe(&RouteDelivery::Queued {
                to: "SEER".into(),
                why: FILES_UNSEEN.into()
            }),
            "Queued for SEER: OpenCode can't see files"
        );
    }

    #[test]
    fn a_look_that_read_no_sessions_answers_unreachable() {
        let failed = |error: &str| look::Live {
            error: Some(error.into()),
            ..look::Live::default()
        };
        let slow = failed(
            "couldn't list sessions: OpenCode at http://127.0.0.1:4096 didn't answer in time: \
             Resource temporarily unavailable (os error 35)",
        );
        let error = sessions_unread(&slow).expect("nothing was read");
        assert_eq!(unreachable_why(&error), "didn't answer in time");
        let spent = "couldn't list sessions: OpenCode took over 10000 ms to list sessions";
        assert_eq!(unreachable_why(spent), "didn't answer in time");
        let down = "couldn't list sessions: Could not reach server at http://127.0.0.1:4096";
        assert_eq!(unreachable_why(down), "it isn't running");
        // A service that wants a password Hex doesn't have (#1131: OpenCode
        // in WSL, Hex on Windows) says so rather than a bare status.
        assert_eq!(
            unreachable_why("couldn't list sessions: 401 Unauthorized"),
            AUTH_REFUSED
        );
        assert_eq!(
            unreachable_why(
                "couldn't list sessions: OpenCode said: Unauthorized: OpenCode API failed: HTTP 401"
            ),
            AUTH_REFUSED
        );
        assert_eq!(
            unreachable_why("couldn't list sessions: 500"),
            "500",
            "other refusals still say what OpenCode answered"
        );
        let Answered { answer, send, .. } = answer_for(
            Decided::Unreachable(unreachable_why(&error)),
            None,
            None,
            &[],
            0,
            false,
        );
        assert!(send.is_none());
        assert_eq!(
            answer,
            RouteAnswer::Unreachable {
                why: "didn't answer in time".into()
            }
        );

        // A clue that names a session outright still routes without the list;
        // only nowhere to go reads Unreachable.
        let page = smart::Decision::Send {
            to: smart::Scored {
                candidate: candidate("ses_page", "", 0),
                score: 40,
                reasons: Vec::new(),
            },
            instead: Vec::new(),
        };
        assert!(matches!(
            judged(page, Some(&error)),
            Decided::Judged(smart::Decision::Send { ref to, .. }) if to.candidate.id == "ses_page"
        ));
        let keep = || smart::Decision::Keep {
            reason: String::new(),
        };
        assert!(
            matches!(judged(keep(), Some(&error)), Decided::Unreachable(ref why) if why == "didn't answer in time")
        );
        assert!(matches!(
            judged(keep(), None),
            Decided::Judged(smart::Decision::Keep { .. })
        ));

        // Sessions read alongside a failure (a folder's page) still route.
        let mut partial = failed("couldn't list sessions: 500");
        partial.candidates = vec![candidate("ses_a", "A", 1)];
        assert_eq!(sessions_unread(&partial), None);
        assert_eq!(sessions_unread(&look::Live::default()), None);
    }

    /// Captures written to disk and looked at through `look_with`, with
    /// OpenCode's sessions, the tab, and the dev server swapped for these.
    mod looked {
        use super::*;
        use scrawl_capture::routing::Hooks;

        fn sessions(path: &str, _: Duration) -> anyhow::Result<Vec<u8>> {
            let session = |id: &str, title: &str, dir: &str, updated: u64| {
                serde_json::json!({
                    "id": id, "title": title, "parentID": null,
                    "time": {"updated": updated}, "location": {"directory": dir},
                })
            };
            let all = [
                session("ses_board", "BOARD", "/p/board", 20),
                session("ses_home", "HOME", "/p/home", 10),
            ];
            let body = match path.strip_prefix("/api/session/") {
                Some(id) => all
                    .iter()
                    .find(|s| s["id"] == id)
                    .map(|s| serde_json::json!({ "data": s }))
                    .ok_or_else(|| anyhow::anyhow!("404 Not Found"))?,
                None => {
                    let dir = path
                        .split("directory=")
                        .nth(1)
                        .map(|d| d.replace("%2F", "/"));
                    let page: Vec<_> = all
                        .iter()
                        .filter(|s| {
                            dir.as_ref()
                                .is_none_or(|d| s["location"]["directory"] == *d)
                        })
                        .collect();
                    serde_json::json!({ "data": page })
                }
            };
            Ok(serde_json::to_vec(&body)?)
        }
        fn no_sdk(_: &str) -> Option<routing::Registration> {
            None
        }
        fn dev_server(port: u16) -> Option<PathBuf> {
            (port == 4948).then(|| PathBuf::from("/p/board"))
        }
        fn no_terminal(_: &str, _: Option<i32>) -> Option<look::Pane> {
            None
        }
        fn board_tab(_: Option<&str>, _: Option<&str>) -> Option<String> {
            Some("https://board.example.com/cards".into())
        }
        fn dev_tab(_: Option<&str>, _: Option<&str>) -> Option<String> {
            Some("http://127.0.0.1:4948/".into())
        }
        fn no_tab(_: Option<&str>, _: Option<&str>) -> Option<String> {
            None
        }

        #[test]
        fn routing_latency_with_a_slow_session_server() {
            static READS: AtomicU64 = AtomicU64::new(0);
            fn slow(path: &str, budget: Duration) -> anyhow::Result<Vec<u8>> {
                READS.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(300));
                sessions(path, budget)
            }
            for (name, destination) in [("session_rule", "ses_board"), ("folder_rule", "/p/board")]
            {
                READS.store(0, Ordering::Relaxed);
                let router = router_with(|c| {
                    c.apps
                        .insert("com.example.board".into(), destination.into());
                });
                let mut senses = senses(no_tab);
                senses.hooks.api = slow;
                let started = Instant::now();
                let (answer, sent) = route(router, "com.example.board", senses);
                eprintln!(
                    "METRIC {name}_ms={} session_reads={}",
                    started.elapsed().as_millis(),
                    READS.load(Ordering::Relaxed)
                );
                assert!(matches!(answer, RouteAnswer::Rule { .. }));
                assert_eq!(sent.as_deref(), Some("ses_board"));
            }
        }

        fn senses(tab: fn(Option<&str>, Option<&str>) -> Option<String>) -> look::Senses {
            look::Senses {
                hooks: Hooks {
                    sdk: no_sdk,
                    listener_dir: dev_server,
                    api: sessions,
                },
                tab,
                own_port: |_| None,
                terminal: no_terminal,
                ..look::Senses::default()
            }
        }

        fn temp(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "hex-route-{name}-{}-{}",
                std::process::id(),
                NEXT_ROUTES.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn written(bundle_id: &str) -> PathBuf {
            let dir = temp("bundle").join("test.scrawl");
            std::fs::create_dir(&dir).unwrap();
            let bundle = serde_json::json!({
                "version": 1, "id": "test", "createdAtMs": 0, "durationMs": 1000,
                "target": {
                    "mode": "display", "app": "Board", "bundleId": bundle_id, "displayId": 1,
                    "frame": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 100.0, "scale": 2.0},
                },
                "transcript": {"source": "test", "text": "this card", "segments": []},
                "strokes": [], "marks": [], "events": [], "keyframes": [], "contactSheet": "",
            });
            std::fs::write(dir.join("scrawl.json"), bundle.to_string()).unwrap();
            scrawl_capture::handoff::load_bundle(&dir).expect("a bundle");
            dir
        }

        fn router_with(config: impl FnOnce(&mut RouterConfig)) -> Router {
            let mut routing = RouterConfig {
                send_captures: true,
                ..RouterConfig::default()
            };
            config(&mut routing);
            Router::new(routing, RouterDirs::under(&temp("router"), None, None)).with_live(false)
        }

        /// Stands in for OpenCode taking a capture.
        fn posted(_: &Path, _: &str) -> anyhow::Result<scrawl_capture::handoff::Posted> {
            Ok(scrawl_capture::handoff::Posted {
                message_id: Some("msg_test".into()),
                delivery: "queue".into(),
                was_idle: None,
            })
        }

        /// A worker whose sends never reach OpenCode.
        fn worker(outcomes: SharedOutcomes) -> Worker {
            Worker {
                forward: posted,
                ..Worker::new(outcomes)
            }
        }

        fn settle(worker: &mut Worker) {
            settle_with_clock(worker, true);
        }

        fn settle_with_clock(worker: &mut Worker, advance: bool) {
            let deadline = Instant::now() + Duration::from_secs(20);
            while worker.pending.is_some()
                || !worker.older.is_empty()
                || worker
                    .current
                    .as_ref()
                    .is_some_and(|c| c.send.is_pending_rule())
            {
                assert!(Instant::now() < deadline, "routing did not finish");
                if advance {
                    let now = Instant::now();
                    for current in worker
                        .current
                        .iter_mut()
                        .chain(worker.older.iter_mut().map(|(c, _)| c))
                    {
                        if let Some(clock) = current.send.clock_mut() {
                            clock.expire_at(now);
                        }
                    }
                }
                worker.prepared();
                std::thread::park_timeout(Duration::from_millis(1));
            }
        }

        fn held_look(router: Router, live: look::Live) -> (Look, SyncSender<Option<look::Live>>) {
            let (found, seen) = mpsc::sync_channel(1);
            (
                Look {
                    router,
                    seen,
                    local: std::sync::Arc::new(Mutex::new(live)),
                    until: Instant::now() + Duration::from_secs(5),
                    model: jev::Hooks::default(),
                },
                found,
            )
        }

        #[test]
        fn saved_rules_publish_before_the_session_server_answers() {
            for target in ["ses_board", "/p/board"] {
                let router = router_with(|c| {
                    c.apps.insert("com.example.board".into(), target.into());
                });
                let (look, found) = held_look(router, look::Live::default());
                let outcomes = SharedOutcomes::default();
                let mut worker = worker(outcomes.clone());
                let bundle = written("com.example.board");
                lock(&outcomes).fail(bundle.clone(), "test".into());
                let started = Instant::now();
                worker.written(bundle, look);
                assert!(started.elapsed() < Duration::from_millis(200));
                let current = worker.current.as_ref().unwrap();
                assert!(current.sent_session().is_none());
                assert!(current.send.clock().unwrap().due().unwrap() >= started + SEND_HOLD);
                assert!(matches!(
                    current.answer,
                    RouteAnswer::Rule { .. } | RouteAnswer::ResolvingRule { .. }
                ));
                worker.undo();
                assert_eq!(
                    worker.current.as_ref().unwrap().send.delivery(),
                    Some(RouteDelivery::TakenBack)
                );
                drop(found);
                settle(&mut worker);
                assert!(worker.current.as_ref().unwrap().sent_session().is_none());
            }
        }

        /// Both timings of the page read: already available at publication or
        /// arriving with the final session list. Only a saved rule can send.
        #[test]
        fn page_metadata_is_only_a_suggestion_and_never_overrides_a_saved_rule() {
            static POSTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
            fn post(path: &Path, id: &str) -> anyhow::Result<scrawl_capture::handoff::Posted> {
                POSTS.lock().unwrap().push(id.into());
                posted(path, id)
            }
            for saved in [false, true] {
                for early in [false, true] {
                    for source in [smart::Source::Signed, smart::Source::Page] {
                        let router = router_with(|c| {
                            c.jev_routing = false;
                            if saved {
                                c.sites
                                    .insert("https://gallery.example".into(), "ses_ruled".into());
                            }
                        });
                        let url = "https://gallery.example/2026-01-02-demo/";
                        let mut live = look::Live {
                            url: Some(url.into()),
                            clues: vec![smart::Clue::new(
                                source,
                                smart::Pointer::Session {
                                    id: "ses_signer".into(),
                                    directory: None,
                                },
                                "index.html names it in a meta tag",
                            )],
                            ..look::Live::default()
                        };
                        let (look, found) = held_look(
                            router,
                            if early {
                                live.clone()
                            } else {
                                look::Live::default()
                            },
                        );
                        let outcomes = SharedOutcomes::default();
                        let mut worker = worker(outcomes.clone());
                        worker.forward = post;
                        let bundle = written("com.example.browser");
                        lock(&outcomes).fail(bundle.clone(), "test".into());
                        worker.written(bundle.clone(), look);
                        assert!(worker.current.as_ref().unwrap().sent_session().is_none());
                        worker.hold(true);
                        // The local tab matched a rule. A later partial read
                        // can return the page clue without repeating the URL.
                        if early {
                            live.url = None;
                        }
                        found.send(Some(live)).unwrap();
                        drop(found);
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while worker.pending.is_some() {
                            assert!(Instant::now() < deadline);
                            worker.prepared();
                            std::thread::yield_now();
                        }
                        assert!(POSTS.lock().unwrap().is_empty(), "not during Undo");
                        worker.hold(false);
                        settle(&mut worker); // Advance any send deadline and drain finalization.
                        let current = worker.current.unwrap();
                        if saved {
                            assert_eq!(current.sent_session(), Some("ses_ruled"));
                            assert!(
                                matches!(current.answer, RouteAnswer::Rule { to, .. } if to.session == "ses_ruled")
                            );
                            assert_eq!(std::mem::take(&mut *POSTS.lock().unwrap()), ["ses_ruled"]);
                        } else {
                            assert!(POSTS.lock().unwrap().is_empty(), "no automatic post");
                            assert!(current.send.delivery().is_none());
                            assert!(
                                current.sent_session().is_none() && !current.send.is_pending_rule()
                            );
                            assert!(
                                matches!(&current.answer, RouteAnswer::Guess { suggested, .. } if suggested[0].session == "ses_signer"),
                                "{:?}",
                                current.answer
                            );
                        }
                        std::fs::remove_dir_all(bundle.parent().unwrap()).unwrap();
                    }
                }
            }
        }

        /// A saved app rule to a session id sends before the session server
        /// answers: the pill, the delivery and the bundle still say a name,
        /// never `ses_…`.
        #[test]
        fn a_rule_sent_before_the_sessions_read_names_a_session_not_its_id() {
            let router = router_with(|c| {
                c.apps
                    .insert("com.example.board".into(), "ses_board".into());
            });
            let (look, found) = held_look(router, look::Live::default());
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            let bundle = written("com.example.board");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            worker.written(bundle.clone(), look);
            // The local read saw nothing, so the rule's send is due at once.
            worker
                .current
                .as_mut()
                .unwrap()
                .send
                .clock_mut()
                .unwrap()
                .expire_at(Instant::now());
            worker.prepared();
            let current = worker.current.as_ref().unwrap();
            assert!(
                matches!(&current.answer, RouteAnswer::Rule { to, .. }
                    if to.session == "ses_board" && to.name == "session"),
                "{:?}",
                current.answer
            );
            assert_eq!(
                current.send.delivery(),
                Some(RouteDelivery::Sent {
                    to: "session".into()
                }),
                "the pill's name"
            );
            let (_, loaded) = scrawl_capture::handoff::load_bundle(&bundle).unwrap();
            assert_eq!(
                loaded.destination.map(|dest| dest.title),
                Some("session".into()),
                "the bundle and summary's name"
            );
            drop(found);
            worker.pending = None;
            std::fs::remove_dir_all(bundle.parent().unwrap()).ok();
        }

        /// The capsule starts a rule's Undo ring when it first shows the
        /// rule. A rule still being looked up when its capture was written
        /// holds its send from its answer, not from the write, so the send
        /// never goes before the ring runs out.
        #[test]
        fn a_rule_answered_late_holds_its_send_from_the_answer() {
            let router = router_with(|c| {
                c.apps
                    .insert("com.example.board".into(), "ses_board".into());
            });
            let (look, found) = held_look(router, look::Live::default());
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            let bundle = written("com.example.board");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            worker.written(bundle.clone(), look);
            // As if the local read found no rule yet, and its hold ran out
            // while the capsule still said nothing.
            let current = worker.current.as_mut().unwrap();
            current.answer = RouteAnswer::Looking;
            current.send = SendState::ResolvingRule {
                clock: Clock::Counting {
                    due: Instant::now(),
                },
            };
            drop(found);
            let deadline = Instant::now() + Duration::from_secs(20);
            while matches!(
                worker.current.as_ref().unwrap().answer,
                RouteAnswer::Looking
            ) {
                assert!(Instant::now() < deadline, "the rule never came");
                worker.prepared();
                std::thread::park_timeout(Duration::from_millis(1));
            }
            let answered = Instant::now();
            let current = worker.current.as_ref().unwrap();
            assert!(
                current.sent_session().is_none(),
                "not before its ring runs out"
            );
            assert!(
                current.send.clock().unwrap().due().unwrap() + Duration::from_millis(100)
                    >= answered + SEND_HOLD
            );
            settle(&mut worker);
            assert!(worker.current.as_ref().unwrap().sent_session().is_some());
            std::fs::remove_dir_all(bundle.parent().unwrap()).ok();
        }

        #[test]
        fn the_capsule_holds_a_rules_send_while_the_pointer_is_on_it() {
            static POSTS: AtomicU64 = AtomicU64::new(0);
            fn post(path: &Path, id: &str) -> anyhow::Result<scrawl_capture::handoff::Posted> {
                POSTS.fetch_add(1, Ordering::Relaxed);
                posted(path, id)
            }
            let router = router_with(|c| {
                c.apps
                    .insert("com.example.board".into(), "ses_board".into());
            });
            let (look, found) = held_look(router, look::Live::default());
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            worker.forward = post;
            let bundle = written("com.example.board");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            worker.written(bundle.clone(), look);
            worker.hold(true);
            // Its window ran out while the pointer was on the capsule.
            worker
                .current
                .as_mut()
                .unwrap()
                .send
                .clock_mut()
                .unwrap()
                .set_paused(Instant::now(), Duration::ZERO);
            worker.prepared();
            assert_eq!(POSTS.load(Ordering::Relaxed), 0, "held, it waits");
            // Held 10 s with 2 s left: it goes 2 s after the pointer leaves.
            let current = worker.current.as_mut().unwrap();
            let since = Instant::now() - Duration::from_secs(10);
            current
                .send
                .clock_mut()
                .unwrap()
                .set_paused(since, Duration::from_secs(2));
            worker.hold(false);
            let current = worker.current.as_ref().unwrap();
            let clock = current.send.clock().unwrap();
            assert!(!clock.is_paused());
            let left = clock
                .due()
                .unwrap()
                .saturating_duration_since(Instant::now());
            assert!(left > Duration::from_millis(1900) && left <= Duration::from_secs(2));
            worker.prepared();
            assert_eq!(POSTS.load(Ordering::Relaxed), 0, "not before its time");
            drop(found);
            settle(&mut worker);
            assert_eq!(POSTS.load(Ordering::Relaxed), 1);
            std::fs::remove_dir_all(bundle.parent().unwrap()).unwrap();
        }

        #[test]
        fn a_new_capture_does_not_cancel_an_older_folder_rules_send() {
            static POSTS: AtomicU64 = AtomicU64::new(0);
            fn post(path: &Path, id: &str) -> anyhow::Result<scrawl_capture::handoff::Posted> {
                assert_eq!(id, "ses_board");
                POSTS.fetch_add(1, Ordering::Relaxed);
                posted(path, id)
            }
            let router = router_with(|c| {
                c.apps.insert("com.example.board".into(), "/p/board".into());
            });
            let (look, found) = held_look(router, look::Live::default());
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            worker.forward = post;
            let first = written("com.example.board");
            lock(&outcomes).fail(first.clone(), "test".into());
            worker.written(first.clone(), look);
            let second = written("com.example.other");
            lock(&outcomes).fail(second.clone(), "test".into());
            let (look, later) = held_look(router_with(|_| {}), look::Live::default());
            worker.written(second.clone(), look);
            assert_eq!(worker.older.len(), 1);
            found
                .send(Some(look::Live {
                    candidates: vec![smart::Candidate {
                        directory: Some("/p/board".into()),
                        repo: Some("/p/board".into()),
                        ..candidate("ses_board", "Board", 0)
                    }],
                    ..Default::default()
                }))
                .unwrap();
            later.send(Some(look::Live::default())).unwrap();
            settle(&mut worker);
            assert_eq!(POSTS.load(Ordering::Relaxed), 1);
            assert_eq!(worker.current.as_ref().unwrap().bundle, second);
            assert!(worker.current.as_ref().unwrap().sent_session().is_none());
            std::fs::remove_dir_all(first.parent().unwrap()).unwrap();
            std::fs::remove_dir_all(second.parent().unwrap()).unwrap();
        }

        /// Kit's Nov 5 report: the list appeared, then a later judgement put
        /// its pick on top and re-sorted the recent sessions, so a click
        /// landed on another session. Rows already listed stay where they
        /// are; a session new to the list comes last.
        #[test]
        fn a_later_judgement_never_moves_a_listed_row() {
            let (look, found) = held_look(router_with(|_| {}), look::Live::default());
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            let bundle = written("com.example.board");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            worker.written(bundle.clone(), look);
            let (updates, pending) = preparation::fed();
            worker.pending = Some(pending);
            drop(found);
            let rows = |worker: &Worker| -> Vec<String> {
                listed(&worker.current.as_ref().unwrap().answer)
                    .iter()
                    .map(|dest| dest.session.clone())
                    .collect()
            };
            let live = |candidates| look::Live {
                candidates,
                ..Default::default()
            };
            updates
                .send(preparation::Update::judged(
                    live(vec![
                        candidate("ses_board", "BOARD", 30),
                        candidate("ses_home", "HOME", 20),
                        candidate("ses_hex", "HEX", 10),
                    ]),
                    smart::Judgement {
                        decision: smart::Decision::Keep {
                            reason: String::new(),
                        },
                        ranking: Vec::new(),
                    },
                ))
                .unwrap();
            worker.prepared();
            assert_eq!(rows(&worker), ["ses_board", "ses_home", "ses_hex"]);
            // The model picks the last row; meanwhile HEX moved and a new
            // session opened.
            let pick = scored_by(
                "ses_hex",
                "HEX",
                smart::Basis::Clue(smart::Source::Named),
                "mentioned",
            );
            updates
                .send(preparation::Update::judged(
                    live(vec![
                        candidate("ses_hex", "HEX", 40),
                        candidate("ses_board", "BOARD", 30),
                        candidate("ses_home", "HOME", 20),
                        candidate("ses_new", "NEW", 50),
                    ]),
                    smart::Judgement {
                        decision: smart::Decision::Offer {
                            choices: vec![pick],
                        },
                        ranking: Vec::new(),
                    },
                ))
                .unwrap();
            worker.prepared();
            assert_eq!(
                rows(&worker),
                ["ses_board", "ses_home", "ses_hex", "ses_new"]
            );
            let answer = &worker.current.as_ref().unwrap().answer;
            assert!(matches!(answer, RouteAnswer::Guess { .. }), "{answer:?}");
            worker.pending = None;
            std::fs::remove_dir_all(bundle.parent().unwrap()).ok();
        }

        #[test]
        fn a_pick_cancels_a_late_rule_resolution() {
            let router = router_with(|c| {
                c.apps.insert("com.example.board".into(), "/p/board".into());
            });
            let (look, found) = held_look(router, look::Live::default());
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            let bundle = written("com.example.board");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            worker.written(bundle.clone(), look);
            worker.chosen("ses_home", false);
            let _ = found.send(Some(look::Live::default()));
            settle(&mut worker);
            assert_eq!(
                worker.current.as_ref().unwrap().sent_session(),
                Some("ses_home")
            );
            assert!(!worker.current.as_ref().unwrap().send.is_pending_rule());
            std::fs::remove_dir_all(bundle.parent().unwrap()).unwrap();
        }

        /// What the worker answers for a capture of `bundle_id` looked at
        /// with `senses`, and the session it was sent to at once, if any.
        fn route(
            router: Router,
            bundle_id: &str,
            senses: look::Senses,
        ) -> (RouteAnswer, Option<String>) {
            let context = RouteContext {
                front_bundle_id: Some("com.brave.Browser".into()),
                captured_bundle_id: Some(bundle_id.into()),
                app_pid: None,
            };
            let look = look_with(context, router, senses).expect("captures are sent");
            let mut worker = worker(SharedOutcomes::default());
            worker.written(written(bundle_id), look);
            settle(&mut worker);
            let current = worker.current.unwrap();
            let sent = current.sent_session().map(str::to_owned);
            (current.answer, sent)
        }

        #[test]
        fn a_saved_site_or_app_rule_sends_whatever_the_routing_flags() {
            for smart in [false, true] {
                let sites = router_with(|c| {
                    c.smart_routing = smart;
                    c.sites
                        .insert("https://board.example.com".into(), "/p/board".into());
                });
                let (answer, sent) = route(sites, "com.brave.Browser", senses(board_tab));
                assert!(
                    matches!(answer, RouteAnswer::Rule { ref to, .. } if to.session == "ses_board"),
                    "site rule, smart {smart}: {answer:?}"
                );
                assert_eq!(sent.as_deref(), Some("ses_board"), "sent at once");

                let apps = router_with(|c| {
                    c.smart_routing = smart;
                    c.apps.insert("com.example.board".into(), "/p/board".into());
                });
                let (answer, sent) = route(apps, "com.example.board", senses(no_tab));
                assert!(
                    matches!(answer, RouteAnswer::Rule { ref to, .. } if to.session == "ses_board"),
                    "app rule, smart {smart}: {answer:?}"
                );
                assert_eq!(sent.as_deref(), Some("ses_board"), "sent at once");
            }
        }

        /// `path` as an absolute path on this OS: Windows needs a drive.
        fn absolute(path: &str) -> String {
            if cfg!(windows) {
                format!("C:{path}")
            } else {
                path.to_owned()
            }
        }

        /// SEER and RUNE DESKTOP working in one checkout, SEER the newer.
        fn one_checkout(path: &str, _: Duration) -> anyhow::Result<Vec<u8>> {
            let session = |id: &str, title: &str, updated: u64| {
                serde_json::json!({
                    "id": id, "title": title, "parentID": null,
                    "time": {"updated": updated}, "location": {"directory": absolute("/p/seer")},
                })
            };
            let all = [
                session("ses_seer", "SEER", 30),
                session("ses_rune", "RUNE DESKTOP", 20),
            ];
            let body = match path.strip_prefix("/api/session/") {
                Some(id) => all
                    .iter()
                    .find(|s| s["id"] == id)
                    .map(|s| serde_json::json!({ "data": s }))
                    .ok_or_else(|| anyhow::anyhow!("404 Not Found"))?,
                None => serde_json::json!({ "data": all }),
            };
            Ok(serde_json::to_vec(&body)?)
        }

        #[test]
        fn an_always_rule_keeps_sending_to_the_session_picked() {
            let senses = || look::Senses {
                hooks: Hooks {
                    sdk: no_sdk,
                    listener_dir: dev_server,
                    api: one_checkout,
                },
                ..senses(no_tab)
            };
            let context = || RouteContext {
                front_bundle_id: Some("com.example.rune".into()),
                captured_bundle_id: Some("com.example.rune".into()),
                app_pid: None,
            };
            let look = look_with(context(), router_with(|_| {}), senses()).unwrap();
            let mut worker = worker(SharedOutcomes::default());
            worker.written(written("com.example.rune"), look);
            settle(&mut worker);
            // Shift-picking RUNE DESKTOP saves the app's rule to that session.
            let (kind, key, target) = worker.rule_for("ses_rune").expect("an app rule");
            assert_eq!(
                (kind, key.as_str(), target.as_str()),
                (RuleKind::App, "com.example.rune", "ses_rune")
            );
            let saved = RouterConfig {
                send_captures: true,
                ..RouterConfig::default()
            }
            .with_rule(kind, &key, &target)
            .unwrap();
            let (answer, sent) = route(
                router_with(|c| *c = saved.clone()),
                "com.example.rune",
                senses(),
            );
            assert!(
                matches!(answer, RouteAnswer::Rule { ref to, .. } if to.session == "ses_rune"),
                "{answer:?}"
            );
            assert_eq!(sent.as_deref(), Some("ses_rune"));
            // What it saved before: the checkout, whose newest session is SEER.
            let (_, sent) = route(
                router_with(|c| {
                    c.apps
                        .insert("com.example.rune".into(), absolute("/p/seer"));
                }),
                "com.example.rune",
                senses(),
            );
            assert_eq!(sent.as_deref(), Some("ses_seer"));
        }

        /// A Shift pick on a Ghostty capture remembers the terminal's folder,
        /// not Ghostty: the next capture of a terminal there (or below) goes
        /// to the session picked, and one elsewhere is only guessed.
        #[test]
        fn an_always_rule_from_a_terminal_keys_on_its_folder() {
            const GHOSTTY: &str = "com.mitchellh.ghostty";
            fn shell_in(folder: &str) -> look::Pane {
                look::Pane {
                    cwd: Some(folder.into()),
                    title: "zsh".into(),
                    foreground: None,
                }
            }
            fn in_seer(_: &str, _: Option<i32>) -> Option<look::Pane> {
                Some(shell_in(&absolute("/p/seer")))
            }
            fn below(_: &str, _: Option<i32>) -> Option<look::Pane> {
                Some(shell_in(&absolute("/p/seer/site")))
            }
            fn elsewhere(_: &str, _: Option<i32>) -> Option<look::Pane> {
                Some(shell_in(&absolute("/p/other")))
            }
            let senses = |terminal: fn(&str, Option<i32>) -> Option<look::Pane>| look::Senses {
                hooks: Hooks {
                    sdk: no_sdk,
                    listener_dir: dev_server,
                    api: one_checkout,
                },
                terminal,
                ..senses(no_tab)
            };
            let context = || RouteContext {
                front_bundle_id: Some(GHOSTTY.into()),
                captured_bundle_id: Some(GHOSTTY.into()),
                app_pid: None,
            };
            let look = look_with(
                context(),
                router_with(|c| c.smart_routing = true),
                senses(in_seer),
            )
            .unwrap();
            let mut worker = worker(SharedOutcomes::default());
            worker.written(written(GHOSTTY), look);
            settle(&mut worker);
            let answer = &worker.current.as_ref().unwrap().answer;
            assert!(
                matches!(answer, RouteAnswer::Guess { site: Some(site), .. } if site == "seer terminal"),
                "{answer:?}"
            );
            let (kind, key, target) = worker.rule_for("ses_rune").expect("a terminal rule");
            assert_eq!(
                (kind, key.as_str(), target.as_str()),
                (RuleKind::Terminal, absolute("/p/seer").as_str(), "ses_rune")
            );
            let saved = RouterConfig {
                send_captures: true,
                smart_routing: true,
                ..RouterConfig::default()
            }
            .with_rule(kind, &key, &target)
            .unwrap();
            for terminal in [
                in_seer as fn(&str, Option<i32>) -> Option<look::Pane>,
                below,
            ] {
                let (answer, sent) = route(
                    router_with(|c| *c = saved.clone()),
                    GHOSTTY,
                    senses(terminal),
                );
                assert!(
                    matches!(answer, RouteAnswer::Rule { ref to, .. } if to.session == "ses_rune"),
                    "{answer:?}"
                );
                assert_eq!(sent.as_deref(), Some("ses_rune"));
            }
            let (answer, sent) = route(
                router_with(|c| *c = saved.clone()),
                GHOSTTY,
                senses(elsewhere),
            );
            assert!(!matches!(answer, RouteAnswer::Rule { .. }), "{answer:?}");
            assert_eq!(sent, None);
        }

        /// A real repository with SEER in its main checkout (the newer) and
        /// RUNE DESKTOP in a worktree of it.
        fn worktree_repo() -> (PathBuf, PathBuf) {
            let base = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("hex-rule-worktree-{}", std::process::id()));
            let (main, worktree) = (base.join("seer"), base.join("worktrees/rune"));
            let gitdir = main.join(".git/worktrees/rune");
            std::fs::create_dir_all(&gitdir).unwrap();
            std::fs::create_dir_all(&worktree).unwrap();
            std::fs::write(gitdir.join("commondir"), "../..").unwrap();
            std::fs::write(
                worktree.join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();
            (main, worktree)
        }

        fn main_and_worktree(path: &str, _: Duration) -> anyhow::Result<Vec<u8>> {
            let (main, worktree) = worktree_repo();
            let session = |id: &str, title: &str, updated: u64, dir: &Path| {
                serde_json::json!({
                    "id": id, "title": title, "parentID": null,
                    "time": {"updated": updated}, "location": {"directory": dir},
                })
            };
            let all = [
                session("ses_seer", "SEER", 30, &main),
                session("ses_rune", "RUNE DESKTOP", 20, &worktree),
            ];
            let body = match path.strip_prefix("/api/session/") {
                Some(id) => all
                    .iter()
                    .find(|s| s["id"] == id)
                    .map(|s| serde_json::json!({ "data": s }))
                    .ok_or_else(|| anyhow::anyhow!("404 Not Found"))?,
                None => serde_json::json!({ "data": all }),
            };
            Ok(serde_json::to_vec(&body)?)
        }

        #[test]
        fn an_always_rule_to_a_worktree_session_sends_there_not_to_the_main_checkout() {
            let (main, _) = worktree_repo();
            let senses = || look::Senses {
                hooks: Hooks {
                    sdk: no_sdk,
                    listener_dir: dev_server,
                    api: main_and_worktree,
                },
                ..senses(no_tab)
            };
            let context = || RouteContext {
                front_bundle_id: Some("com.example.rune".into()),
                captured_bundle_id: Some("com.example.rune".into()),
                app_pid: None,
            };
            let look = look_with(context(), router_with(|_| {}), senses()).unwrap();
            let mut worker = worker(SharedOutcomes::default());
            worker.written(written("com.example.rune"), look);
            settle(&mut worker);
            let (kind, key, target) = worker.rule_for("ses_rune").expect("an app rule");
            assert_eq!(
                target, "ses_rune",
                "the worktree's session, not the repository"
            );
            let saved = RouterConfig {
                send_captures: true,
                ..RouterConfig::default()
            }
            .with_rule(kind, &key, &target)
            .unwrap();
            for _ in 0..2 {
                let (answer, sent) = route(
                    router_with(|c| *c = saved.clone()),
                    "com.example.rune",
                    senses(),
                );
                assert!(
                    matches!(answer, RouteAnswer::Rule { ref to, .. } if to.session == "ses_rune"),
                    "{answer:?}"
                );
                assert_eq!(sent.as_deref(), Some("ses_rune"));
            }
            // A rule naming the repository goes to its main checkout's session.
            let (_, sent) = route(
                router_with(|c| {
                    c.apps
                        .insert("com.example.rune".into(), main.display().to_string());
                }),
                "com.example.rune",
                senses(),
            );
            assert_eq!(sent.as_deref(), Some("ses_seer"));
            let _ = std::fs::remove_dir_all(main.parent().unwrap());
        }

        #[test]
        fn an_always_rule_names_the_session_it_sends_to() {
            let router = router_with(|c| {
                c.sites
                    .insert("https://board.example.com".into(), "/p/board".into());
            });
            let context = RouteContext {
                front_bundle_id: Some("com.brave.Browser".into()),
                captured_bundle_id: Some("com.brave.Browser".into()),
                app_pid: None,
            };
            let look = look_with(context, router, senses(board_tab)).unwrap();
            let mut worker = worker(SharedOutcomes::default());
            worker.written(written("com.brave.Browser"), look);
            settle(&mut worker);
            let current = worker.current.as_mut().unwrap();
            current.send = SendState::Delivered {
                session: "ses_board".into(),
                to: "BOARD".into(),
                undo: Some(Sent {
                    delivery_id: "dlv_1".into(),
                }),
                offer: Some(scrawl_capture::OfferRule {
                    kind: RuleKind::Site,
                    key: "https://board.example.com".into(),
                    target: "ses_board".into(),
                }),
                saved: None,
            };
            let site = |session: &str| {
                Some((
                    RuleKind::Site,
                    "https://board.example.com".to_owned(),
                    session.to_owned(),
                ))
            };
            assert_eq!(worker.rule_for("ses_board"), site("ses_board"));
            assert_eq!(
                worker.rule_for("ses_home"),
                site("ses_home"),
                "--to HOME --always names HOME, not the offer's folder"
            );
        }

        #[test]
        fn a_capture_routed_after_a_newer_one_settled_is_still_sendable_by_name() {
            let context = RouteContext {
                front_bundle_id: Some("com.brave.Browser".into()),
                captured_bundle_id: Some("com.brave.Browser".into()),
                app_pid: None,
            };
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            let a = written("com.brave.Browser");
            let look = look_with(context, router_with(|_| {}), senses(dev_tab)).unwrap();
            // B, started while A was looked at, is written first.
            lock(&outcomes).fail(written("com.example.board"), "test".into());
            worker.written(a.clone(), look);
            settle(&mut worker);
            assert!(worker.resolve("BOARD").is_none(), "the capsule shows B");
            assert_eq!(
                worker.listed_as("BOARD").map(|dest| dest.session),
                Some("ses_board".into())
            );
            assert_eq!(
                worker.send_bundle(&a, Some("nobody"), false),
                Err("nobody isn't a session this capture lists".into()),
                "A is sent as routed, by title or id, not as an older capture"
            );
        }

        #[test]
        fn an_undo_opencode_refuses_is_published_too_late() {
            let context = RouteContext {
                front_bundle_id: Some("com.brave.Browser".into()),
                captured_bundle_id: Some("com.brave.Browser".into()),
                app_pid: None,
            };
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            let bundle = written("com.brave.Browser");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            let look = look_with(context, router_with(|_| {}), senses(dev_tab)).unwrap();
            worker.written(bundle, look);
            settle(&mut worker);
            let current = worker.current.as_mut().unwrap();
            // Not a delivery id, so the router's undo fails.
            current.send = SendState::Delivered {
                session: "ses_board".into(),
                to: "BOARD".into(),
                undo: Some(Sent {
                    delivery_id: "not a delivery".into(),
                }),
                offer: None,
                saved: None,
            };
            worker.undo();
            let published = lock(&outcomes)
                .last
                .as_ref()
                .and_then(|last| last.route.clone())
                .and_then(|route| route.delivery);
            assert_eq!(
                published,
                Some(RouteDelivery::TooLate { to: "BOARD".into() })
            );
            assert!(
                worker.current.unwrap().send.has_undo_handle(),
                "it's still sent"
            );
        }

        #[test]
        fn the_port_ladder_is_a_guess() {
            for smart in [false, true] {
                let router = router_with(|c| c.smart_routing = smart);
                let (answer, sent) = route(router, "com.brave.Browser", senses(dev_tab));
                assert!(
                    matches!(answer, RouteAnswer::Guess { ref suggested, .. }
                        if suggested.first().is_some_and(|d| d.session == "ses_board" && d.why == "dev server")),
                    "smart {smart}: {answer:?}"
                );
                assert_eq!(sent, None, "nothing is sent until it's picked");
            }
        }

        /// With smart routing, a capture of an app (Hex's own Settings among them) suggests the
        /// open session whose checkout is named like the app, and sends nothing until it's
        /// picked, however confident sends may be. The port ladder doesn't read the app.
        #[test]
        fn the_captured_apps_checkout_is_a_guess() {
            for auto_send in [policy::AutoSend::Rules, policy::AutoSend::Confident] {
                let router = router_with(|c| {
                    c.smart_routing = true;
                    c.auto_send = auto_send;
                });
                let (answer, sent) = route(router, "com.example.board", senses(no_tab));
                assert!(
                    matches!(answer, RouteAnswer::Guess { ref suggested, .. }
                        if suggested.first().is_some_and(|d| d.session == "ses_board")),
                    "{auto_send:?}: {answer:?}"
                );
                assert_eq!(sent, None, "nothing is sent until it's picked");
            }
        }

        /// The hosted Jev lookup runs only while `Auto-send with Jev` is on, so a
        /// settings file left with `jevRouting` on and `autoSend` back at
        /// `rules` never reaches Jev.
        #[test]
        fn only_an_on_auto_send_with_jev_asks_jev() {
            static ASKED: AtomicU64 = AtomicU64::new(0);
            fn ask(_: &[u8], _: Duration) -> anyhow::Result<Vec<u8>> {
                ASKED.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!("test Jev")
            }
            for (auto_send, asks) in [
                (policy::AutoSend::Rules, 0),
                (policy::AutoSend::Confident, 1),
            ] {
                ASKED.store(0, Ordering::Relaxed);
                let router = router_with(|c| {
                    c.smart_routing = true;
                    c.jev_routing = true;
                    c.auto_send = auto_send;
                });
                let live = look::Live {
                    candidates: vec![
                        candidate("ses_board", "BOARD", 30),
                        candidate("ses_home", "HOME", 20),
                    ],
                    ..look::Live::default()
                };
                let (mut look, found) = held_look(router, live.clone());
                look.model.ask = ask;
                let outcomes = SharedOutcomes::default();
                let mut worker = worker(outcomes.clone());
                let bundle = written("com.example.board");
                lock(&outcomes).fail(bundle.clone(), "test".into());
                worker.written(bundle.clone(), look);
                found.send(Some(live)).unwrap();
                settle(&mut worker);
                assert_eq!(ASKED.load(Ordering::Relaxed), asks, "{auto_send:?}");
                std::fs::remove_dir_all(bundle.parent().unwrap()).unwrap();
            }
        }

        #[test]
        fn a_look_past_its_deadline_is_routed_without() {
            let (_looking, seen) = mpsc::sync_channel(1);
            let look = Look {
                router: router_with(|_| {}),
                seen,
                until: Instant::now() + Duration::from_millis(50),
                local: Default::default(),
                model: jev::Hooks::default(),
            };
            let mut worker = worker(SharedOutcomes::default());
            let started = Instant::now();
            worker.written(written("com.example.board"), look);
            settle(&mut worker);
            assert!(started.elapsed() < Duration::from_secs(1));
            assert!(matches!(
                worker.current.unwrap().answer,
                RouteAnswer::Unreachable { .. }
            ));
        }

        #[test]
        fn retried_queued_send_reports_too_late_on_undo() {
            fn offline(_: &Path, _: &str) -> anyhow::Result<scrawl_capture::handoff::Posted> {
                anyhow::bail!("Could not reach server")
            }
            let context = RouteContext {
                front_bundle_id: Some("com.brave.Browser".into()),
                captured_bundle_id: Some("com.brave.Browser".into()),
                app_pid: None,
            };
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            worker.forward = offline;
            let bundle = written("com.brave.Browser");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            let look = look_with(context, router_with(|_| {}), senses(dev_tab)).unwrap();
            worker.written(bundle.clone(), look);
            settle(&mut worker);

            // Pick BOARD while OpenCode is down -> Queued.
            worker.chosen("ses_board", false);
            assert!(matches!(
                worker.current.as_ref().unwrap().send.delivery(),
                Some(RouteDelivery::Queued { ref to, .. }) if to == "BOARD"
            ));

            // Background retry delivers the bundle -> Sent with no undo handle.
            worker.apply_retried(std::slice::from_ref(&bundle));
            let published = || {
                lock(&outcomes)
                    .last
                    .as_ref()
                    .and_then(|last| last.route.clone())
                    .and_then(|route| route.delivery)
            };
            assert_eq!(
                published(),
                Some(RouteDelivery::Sent { to: "BOARD".into() })
            );

            // Undo must report TooLate instead of silently doing nothing.
            worker.undo();
            assert_eq!(
                published(),
                Some(RouteDelivery::TooLate { to: "BOARD".into() })
            );
            std::fs::remove_dir_all(bundle.parent().unwrap()).ok();
        }

        #[test]
        fn retried_queued_send_never_double_sends_on_repick_and_redirects_elsewhere() {
            static POSTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
            fn offline(_: &Path, _: &str) -> anyhow::Result<scrawl_capture::handoff::Posted> {
                anyhow::bail!("Could not reach server")
            }
            fn record_post(
                path: &Path,
                id: &str,
            ) -> anyhow::Result<scrawl_capture::handoff::Posted> {
                POSTS.lock().unwrap().push(id.into());
                posted(path, id)
            }
            POSTS.lock().unwrap().clear();

            let context = RouteContext {
                front_bundle_id: Some("com.brave.Browser".into()),
                captured_bundle_id: Some("com.brave.Browser".into()),
                app_pid: None,
            };
            let outcomes = SharedOutcomes::default();
            let mut worker = worker(outcomes.clone());
            worker.forward = offline;
            let bundle = written("com.brave.Browser");
            lock(&outcomes).fail(bundle.clone(), "test".into());
            let look = look_with(context, router_with(|_| {}), senses(dev_tab)).unwrap();
            worker.written(bundle.clone(), look);
            settle(&mut worker);

            // Initial pick queues; background retry delivers to ses_board.
            worker.chosen("ses_board", false);
            worker.apply_retried(std::slice::from_ref(&bundle));
            worker.forward = record_post;

            // Re-picking the same session must not send a second time.
            worker.chosen("ses_board", false);
            assert!(
                POSTS.lock().unwrap().is_empty(),
                "picking the already-retried session must not double-send"
            );

            // Picking a different session redirects and sends once there.
            worker.chosen("ses_home", false);
            assert_eq!(std::mem::take(&mut *POSTS.lock().unwrap()), ["ses_home"]);
            assert_eq!(
                worker.current.as_ref().unwrap().sent_session(),
                Some("ses_home")
            );
            std::fs::remove_dir_all(bundle.parent().unwrap()).ok();
        }
    }
}
