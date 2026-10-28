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

/// How often sends OpenCode didn't take are retried.
const RETRY_EVERY: Duration = Duration::from_secs(15);
/// How long taking back a send may wait on OpenCode.
const UNDO_DEADLINE: Duration = Duration::from_secs(10);
/// Recent sessions listed beside the router's suggestions.
const RECENT: usize = 8;
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
}

impl CaptureRoute {
    fn of(answer: RouteAnswer) -> Self {
        Self {
            answer,
            delivery: None,
            offer: None,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteAnswer {
    /// Written; the router is still working.
    Looking,
    /// A rule the person saved (`routing.sites` or `routing.apps`, a
    /// Shift pick's among them) says where it goes: sent to `to`
    /// at once, and the capsule's Undo takes it back. `site` is what the
    /// rule matches, for an always-rule sending it elsewhere instead.
    Rule {
        to: RouteDest,
        suggested: Vec<RouteDest>,
        recent: Vec<RouteDest>,
        site: Option<String>,
    },
    /// Guesses, best first: everything else the router found (Jev's pick,
    /// HOME when Jev couldn't tell, a dev server's port, a page or its
    /// publisher, a name that was said). Nothing is sent until one is
    /// picked; picking one with `always` saves a rule for `site`, which is
    /// what an always-rule would match (the site's host or the app's name),
    /// `None` when no rule can be made.
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
    /// wrong in a few words ("opencode2 not found").
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
    let support = dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Application Support/scrawl");
    let dirs = RouterDirs::under(
        &support.join("router"),
        Some(support.join("routing-decisions.jsonl")),
        Some(settings.captures_dir()),
    );
    Router::new(config, dirs)
        .with_log(|line| {
            if line.starts_with("deliver:") {
                tracing::info!(line, "capture router");
            } else {
                tracing::debug!(line, "capture router");
            }
        })
        .with_summary(scrawl_capture::sketch::summary_markdown)
        .with_opencode(crate::dictation_processor::installed_opencode_executable)
}

fn sends(config: &RouterConfig) -> bool {
    config.send_captures && std::env::var_os("SCRAWL_NO_OPENCODE").is_none()
}

/// The tab, terminal, and sessions behind a capture that just stopped,
/// read while it is written.
pub(super) struct Look {
    router: Router,
    seen: Receiver<Option<look::Live>>,
    /// When routing stops waiting for it.
    until: Instant,
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
    {
        let router = router.clone();
        thread::Builder::new()
            .name("capture-look".into())
            .spawn(move || {
                let live = router.run(|| {
                    routing::look_capture(
                        context.front_bundle_id.as_deref(),
                        context.captured_bundle_id.as_deref(),
                        context.app_pid,
                        deadline,
                        &senses,
                    )
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
    Undo,
    /// `hex capture send`: any capture, to `to` or where its route or its
    /// last send says.
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
const CLI_WAIT: Duration = Duration::from_secs(8);

/// Sends `bundle` through the `capture-route` thread and says what became
/// of it: as the capsule's pick when it is the newest capture, so its
/// route and Undo follow.
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
    /// What an always-rule would match.
    rule: Option<(RuleKind, String)>,
    candidates: Vec<smart::Candidate>,
    route: CaptureRoute,
    /// How to take the send back, once OpenCode took it.
    sent: Option<Sent>,
    /// The rule offered after the send ([`CaptureRoute::offer`]).
    offered: Option<scrawl_capture::OfferRule>,
    /// The rule a Shift pick saved with this send, which Undo takes back.
    saved: Option<SavedRule>,
}

/// A rule saved with a send, and what its key held before.
struct SavedRule {
    kind: RuleKind,
    key: String,
    before: Option<String>,
    was_refused: bool,
}

struct Sent {
    delivery_id: String,
    session: String,
}

/// Posts a capture to an OpenCode session.
type Forward = fn(&Path, &str) -> anyhow::Result<scrawl_capture::handoff::Posted>;

struct Worker {
    outcomes: SharedOutcomes,
    current: Option<Routed>,
    retry_at: Instant,
    forward: Forward,
}

impl Worker {
    fn new(outcomes: SharedOutcomes) -> Self {
        Self {
            outcomes,
            current: None,
            retry_at: Instant::now() + RETRY_EVERY,
            forward: scrawl_capture::handoff::send_bundle,
        }
    }

    fn run(mut self, jobs: &Receiver<Job>) {
        loop {
            match jobs.recv_timeout(self.retry_at.saturating_duration_since(Instant::now())) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
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
            Job::Undo => self.undo(),
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
                None => match &current.route.answer {
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
            if always && current.rule.is_none() && current.offered.is_none() {
                return Err("no rule can be made for this capture's site or app".into());
            }
            self.choose(dest, always);
            let current = self.current.as_ref().ok_or("no capture")?;
            return Ok(current
                .route
                .delivery
                .as_ref()
                .map_or_else(|| "Nothing sent".into(), describe));
        }
        if always {
            return Err("only the last routed capture can save a rule".into());
        }
        let dest = match to {
            Some(to) if to.starts_with("ses") => Destination {
                session: to.to_owned(),
                title: to.to_owned(),
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
        let (delivery, _, _) = deliver(&router(), bundle, &dest, loaded.1.target.app, self.forward);
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

    fn written(&mut self, bundle: PathBuf, look: Look) {
        let Look {
            router,
            seen,
            until,
        } = look;
        let live = seen
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|error| {
                match error {
                    RecvTimeoutError::Timeout => {
                        tracing::warn!("looking for a capture's route took too long");
                    }
                    RecvTimeoutError::Disconnected => {
                        tracing::error!("looking for a capture's route panicked");
                    }
                }
                None
            });
        let loaded = scrawl_capture::handoff::load_bundle(&bundle)
            .map(|(_, loaded)| loaded)
            .inspect_err(|error| tracing::warn!(error = %format!("{error:#}"), "could not read a capture to route"))
            .ok();
        let candidates = live
            .as_ref()
            .map(|live| live.candidates.clone())
            .unwrap_or_default();
        if let Some(live) = &live
            && let Some(error) = &live.error
        {
            tracing::warn!(
                bundle = %bundle_name(&bundle),
                error,
                read = live.candidates.len(),
                "a capture's look couldn't read all of OpenCode's sessions"
            );
        }
        let unread = live.as_ref().and_then(sessions_unread);
        // Judged even without the sessions: a rule, page, or publisher names
        // one outright, and the record keeps what was seen for a replay.
        let (decision, url) = router.run(|| match (live, &loaded) {
            (Some(live), Some(loaded)) => {
                let (judgement, url) = routing::judge_capture(&bundle, loaded, live);
                (judged(judgement.decision, unread.as_deref()), url)
            }
            (Some(live), None) => (Decided::Failed, live.url),
            (None, _) => (Decided::Failed, None),
        });
        let bundle_id = loaded
            .as_ref()
            .and_then(|loaded| loaded.target.bundle_id.clone());
        let app = loaded.as_ref().and_then(|loaded| loaded.target.app.clone());
        let rule = policy::rule_key(url.as_deref(), bundle_id.as_deref());
        let site = rule.as_ref().map(|(kind, key)| match kind {
            RuleKind::Site => key
                .split_once("://")
                .map_or(key.as_str(), |(_, host)| host)
                .to_owned(),
            RuleKind::App => app.clone().unwrap_or_else(|| key.clone()),
        });
        let Answered {
            answer,
            send: rule_send,
            shown,
        } = answer_for(decision, url.as_deref(), site, &candidates, now_ms());
        tracing::info!(
            bundle = %bundle_name(&bundle),
            answer = answer_kind(&answer),
            why = shown.as_ref().map(|shown| shown.why.as_str()),
            "capture routed"
        );
        if let Some(shown) = &shown
            && let Err(error) = routing::record_shown(&bundle, shown)
        {
            tracing::warn!(error = %format!("{error:#}"), "could not record how a capture's route was shown");
        }
        self.current = Some(Routed {
            bundle,
            router,
            url,
            app,
            rule,
            candidates,
            route: CaptureRoute::of(answer),
            sent: None,
            offered: None,
            saved: None,
        });
        self.publish();
        // A saved rule sends at once; the capsule's Undo takes it back.
        if let Some(dest) = rule_send {
            self.send_current(dest, None);
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

    fn choose(&mut self, dest: Destination, always: bool) {
        let Some(current) = self.current.as_ref() else {
            return;
        };
        let already = match (&current.sent, &current.route.delivery) {
            (Some(sent), _) => Some(sent.session == dest.session),
            (None, Some(RouteDelivery::Queued { .. })) => Some(false),
            _ => None,
        };
        if always {
            self.save_rule(&dest.session);
        }
        match already {
            // Accepting the offered rule, or a second click.
            Some(true) => {}
            Some(false) => self.redirect_to(dest),
            None => {
                self.correct(jev::Action::Redirect, "chip", Some(&dest));
                self.send_current(dest, Some("chosen"));
            }
        }
    }

    fn redirect_to(&mut self, dest: Destination) {
        self.take_back();
        self.correct(jev::Action::Redirect, "reroute", Some(&dest));
        self.send_current(dest, Some("chosen"));
    }

    fn undo(&mut self) {
        if !self.acting_on_last() {
            return;
        }
        if let Some(saved) = self
            .current
            .as_mut()
            .and_then(|current| current.saved.take())
        {
            forget_rule(&saved);
        }
        if self.take_back() {
            self.publish();
        }
    }

    /// Takes back the current capture's send, whatever state it is in;
    /// `false` when there was none. One OpenCode wouldn't give back is too
    /// late.
    fn take_back(&mut self) -> bool {
        let Some(current) = self.current.as_mut() else {
            return false;
        };
        let to = delivered_to(current.route.delivery.as_ref());
        if let Some(sent) = current.sent.take() {
            let undone = current.router.undo(&sent.delivery_id, UNDO_DEADLINE);
            let delivery = match undone {
                Ok(report) if report.undone != Undone::Unfinished => {
                    tracing::info!(
                        bundle = %bundle_name(&current.bundle),
                        undone = report.undone.label(),
                        "capture send taken back"
                    );
                    match report.undone {
                        Undone::Withdrawn | Undone::Reverted | Undone::Disowned => {
                            RouteDelivery::TakenBack
                        }
                        Undone::Gone | Undone::TooOld | Undone::Unfinished => {
                            RouteDelivery::TooLate {
                                to: to.unwrap_or_default(),
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
                    current.sent = Some(sent);
                    current.route.delivery = Some(RouteDelivery::TooLate {
                        to: to.unwrap_or_default(),
                    });
                    return true;
                }
            };
            current.route.offer = None;
            current.offered = None;
            current.route.delivery = Some(delivery);
            return true;
        }
        if matches!(current.route.delivery, Some(RouteDelivery::Queued { .. })) {
            // Its retries stop with the marker.
            let _ = std::fs::remove_file(current.bundle.join(delivery::PENDING));
            self.correct(jev::Action::Cancel, "dismiss", None);
            self.set_delivery(RouteDelivery::TakenBack);
            return true;
        }
        false
    }

    fn send_current(&mut self, mut dest: Destination, via: Option<&str>) {
        if let Some(via) = via {
            via.clone_into(&mut dest.via);
        }
        self.set_delivery(RouteDelivery::Sending {
            to: dest.title.clone(),
        });
        self.publish();
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let (delivery, sent, offered) = deliver(
            &current.router,
            &current.bundle,
            &dest,
            current.app.clone(),
            self.forward,
        );
        tracing::info!(
            bundle = %bundle_name(&current.bundle),
            session = dest.session,
            delivery = delivery_kind(&delivery),
            "capture sent"
        );
        current.sent = sent;
        current.route.offer = offered.as_ref().map(|offer| offer.key.clone());
        current.offered = offered;
        current.route.delivery = Some(delivery);
        self.publish();
    }

    /// Saves an always-rule from the current capture's site or app to
    /// `session`'s project (or the session itself), on top of what
    /// `settings.json` holds now.
    fn save_rule(&mut self, session: &str) {
        let Some((kind, key, target)) = self.rule_for(session) else {
            tracing::info!("no rule can be made for this capture");
            return;
        };
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
                if let Some(current) = self.current.as_mut() {
                    current.saved = saved;
                }
            }
            Err(error) => tracing::warn!(%error, "could not save a capture routing rule"),
        }
    }

    /// The always-rule sending the current capture's site or app to
    /// `session`: the rule offered after its send when that send went to
    /// `session`, else `session`'s project (or the session itself).
    fn rule_for(&self, session: &str) -> Option<(RuleKind, String, String)> {
        let current = self.current.as_ref()?;
        let sent_here = current
            .sent
            .as_ref()
            .is_some_and(|sent| sent.session == session);
        if let Some(offer) = current.offered.as_ref().filter(|_| sent_here) {
            return Some((offer.kind, offer.key.clone(), offer.folder.clone()));
        }
        let (kind, key) = current.rule.clone().or_else(|| {
            let offer = current.offered.as_ref()?;
            Some((offer.kind, offer.key.clone()))
        })?;
        let target = current
            .candidates
            .iter()
            .find(|candidate| candidate.id == session)
            .and_then(policy::rule_folder)
            .map_or_else(|| session.to_owned(), |folder| folder.display().to_string());
        Some((kind, key, target))
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
        let listed = listed(&current.route.answer);
        let found = listed.iter().find(|dest| dest.session == to).or_else(|| {
            listed
                .iter()
                .find(|dest| dest.name.eq_ignore_ascii_case(to))
        });
        let (session, title) = match found {
            Some(dest) => (dest.session.clone(), dest.name.clone()),
            None if to.starts_with("ses") => (to.to_owned(), to.to_owned()),
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

    fn set_delivery(&mut self, delivery: RouteDelivery) {
        if let Some(current) = self.current.as_mut() {
            current.route.delivery = Some(delivery);
        }
    }

    fn publish(&self) {
        if let Some(current) = &self.current {
            lock(&self.outcomes).routed(&current.bundle, current.route.clone());
        }
    }

    /// Retries sends OpenCode didn't take, unless another process owns
    /// retries: Scrawl's app, or anything serving Scrawl's ingest port.
    fn retry(&mut self) {
        if cfg!(test) || crate::app_settings::scrawl_app_running() || ingest_port_served() {
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
        let sent_now = self
            .current
            .as_ref()
            .is_some_and(|current| report.sent.iter().any(|(dir, _)| *dir == current.bundle));
        if sent_now
            && let Some(current) = self.current.as_mut()
            && let Some(RouteDelivery::Queued { to, .. }) = current.route.delivery.take()
        {
            current.route.delivery = Some(RouteDelivery::Sent { to });
            self.publish();
        }
    }
}

/// What the router made of a capture, before it is shown.
enum Decided {
    Judged(smart::Decision),
    /// OpenCode's sessions couldn't be read: why, in a few words.
    Unreachable(String),
    Failed,
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
    if error.contains("didn't answer in time") || error.contains("took over") {
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
    if error.contains("HTTP 404") {
        return "session not found".into();
    }
    if let Some(said) = error.strip_prefix("OpenCode said: ") {
        let said = said.split(": OpenCode API failed").next().unwrap_or(said);
        return fit(said.lines().next().unwrap_or(said));
    }
    if error.starts_with("Could not reach server") {
        "couldn't reach OpenCode".into()
    } else if error.contains("didn't answer in time") {
        "OpenCode didn't answer".into()
    } else if error.contains("Could not find opencode2") || error.contains("OPENCODE_BIN") {
        "opencode2 not found".into()
    } else {
        fit(&delivery::short(error))
    }
}

/// `routing.sites` or `routing.apps`, as `kind` says.
fn rules_of(
    config: &scrawl_capture::RouterConfig,
    kind: RuleKind,
) -> &std::collections::BTreeMap<String, String> {
    match kind {
        RuleKind::Site => &config.sites,
        RuleKind::App => &config.apps,
    }
}

/// Saves `settings.json`'s routing as `edit` makes it, then has the app
/// reload its settings.
fn edit_routing(
    edit: impl FnOnce(&scrawl_capture::RouterConfig) -> color_eyre::Result<scrawl_capture::RouterConfig>,
) -> color_eyre::Result<()> {
    let mut settings = crate::app_settings::AppSettings::load()?;
    let next = edit(settings.capture.routing.config())?;
    settings.capture.routing = next.into();
    settings.save()?;
    #[cfg(target_os = "macos")]
    if let Some(runtime) = crate::macos_rune::runtime() {
        let _ = runtime.send(crate::macos_rune::Request::ReloadSettings);
    }
    Ok(())
}

/// Undo of a Shift pick: the rule it saved goes, and what its key held
/// before comes back.
fn forget_rule(saved: &SavedRule) {
    let result = edit_routing(|config| {
        let mut next = config.clone();
        let rules = match saved.kind {
            RuleKind::Site => &mut next.sites,
            RuleKind::App => &mut next.apps,
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
) -> Answered {
    let scored = |scored: &smart::Scored| {
        let dest = smart::destination_of(scored, url);
        RouteDest {
            session: dest.session,
            name: dest.title,
            why: why(&dest.via).to_owned(),
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
    match decided {
        // Only a rule the person saved sends on its own; anything else the
        // router would send is its first suggestion.
        Decided::Judged(smart::Decision::Send { to, instead }) if policy::saved_rule(&to) => {
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
        Decided::Failed => Answered::of(RouteAnswer::NoAnswer {
            recent: recent(&[]),
        }),
    }
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

/// The newest sessions not already shown, with how long ago each moved.
fn recent(candidates: &[smart::Candidate], shown: &[&str], now_ms: u64) -> Vec<RouteDest> {
    let mut recent: Vec<_> = candidates
        .iter()
        .filter(|candidate| !shown.contains(&candidate.id.as_str()))
        .collect();
    recent.sort_by_key(|candidate| std::cmp::Reverse(candidate.updated_ms));
    recent
        .into_iter()
        .take(RECENT)
        .map(|candidate| RouteDest {
            session: candidate.id.clone(),
            name: candidate.label().to_owned(),
            why: format!(
                "{} ago",
                smart::ago_text(now_ms.saturating_sub(candidate.updated_ms))
            ),
        })
        .collect()
}

/// A destination's `via` as a quiet word: a real reason in plain words,
/// or nothing (a pick with no reason behind it, such as Jev's).
fn why(via: &str) -> &str {
    match via {
        "rule" | "app-rule" => "rule",
        "named" | "speech" => "mentioned",
        "sdk" | "page" => "same page",
        "viewed" => "you were just in it",
        "localhost" | "port" => "dev server",
        "recorded" | "chosen" => "sent before",
        "publisher" => "published it",
        "dev-build" => "dev build",
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
        RouteAnswer::Looking | RouteAnswer::Unreachable { .. } => Vec::new(),
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

fn delivered_to(delivery: Option<&RouteDelivery>) -> Option<String> {
    match delivery? {
        RouteDelivery::Sending { to }
        | RouteDelivery::Sent { to }
        | RouteDelivery::Queued { to, .. }
        | RouteDelivery::TooLate { to }
        | RouteDelivery::Failed { to, .. } => Some(to.clone()),
        RouteDelivery::TakenBack => None,
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
) -> (
    RouteDelivery,
    Option<Sent>,
    Option<scrawl_capture::OfferRule>,
) {
    router.run(|| {
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
        let sent = posted.map(|posted| {
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
                session: dest.session.clone(),
            }
        });
        let to = dest.title.clone();
        match delivery {
            Delivery::Sent { .. } => (
                RouteDelivery::Sent { to },
                sent,
                routing::learn_capture(bundle, &dest.session),
            ),
            Delivery::Queued { error, .. } => {
                let agent_host = router.config().agent_host().is_some();
                let why = send_why(&error, agent_host);
                if why == FILES_UNSEEN {
                    tracing::warn!(
                        error = delivery::short(&error),
                        "OpenCode can't read a capture from its own disk; if it runs on another \
                         Mac, set capture.routing.agentHost to that Mac's SSH host. Retrying"
                    );
                } else {
                    tracing::warn!(
                        error = delivery::short(&error),
                        "a capture's send didn't go; retrying"
                    );
                }
                (RouteDelivery::Queued { to, why }, None, None)
            }
            Delivery::Saved => (
                RouteDelivery::Failed {
                    to,
                    why: "nothing was sent".into(),
                },
                None,
                None,
            ),
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
        )
    }

    #[test]
    fn only_a_saved_rule_sends_on_its_own() {
        use smart::{Basis, Source};
        for (basis, why, shown) in [
            (Basis::JevHome, "Jev couldn't tell, so HOME", ""),
            (Basis::Jev, "Jev's pick", ""),
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
        ] {
            let answered = sent_by(basis, why);
            assert_eq!(
                answered.send.map(|dest| dest.session),
                Some("ses_a".into()),
                "{why}: sent at once"
            );
            assert!(
                matches!(answered.answer, RouteAnswer::Rule { ref to, .. } if to.why == "rule")
            );
            assert_eq!(answered.shown, Some(Shown::rule(why)));
        }
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
                why: "1 min ago".into(),
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
        );
        assert!(send.is_none());
        assert!(matches!(
            answer,
            RouteAnswer::Guess { ref suggested, ref recent, site: Some(ref site) }
                if suggested.len() == 1 && recent.is_empty() && site == "example.com"
        ));
        let Answered { answer, send, .. } = answer_for(Decided::Failed, None, None, &candidates, 0);
        assert!(send.is_none());
        assert!(matches!(answer, RouteAnswer::NoAnswer { ref recent } if recent.len() == 1));
    }

    /// The Air's first sends: Studio's OpenCode couldn't read the contact
    /// sheet yet, and the capsule said only "couldn't reach SEER".
    #[test]
    fn a_failed_send_says_what_went_wrong() {
        let unread = "OpenCode said: Unable to read attachment: file:///Users/kit/Library/\
                      Application%20Support/scrawl/captures/1.scrawl/contact-sheet.png: \
                      OpenCode API failed: HTTP 400 Bad Request";
        assert_eq!(send_why(unread, false), FILES_UNSEEN);
        assert_eq!(
            send_why(unread, true),
            "Unable to read attachment: file:///Users/kit/Library/Applica…"
        );
        let cases = [
            (
                "OpenCode said: Skill not found: x: OpenCode API failed: HTTP 400 Bad Request",
                "Skill not found: x",
            ),
            (
                "Could not find opencode2; set OPENCODE_BIN or add it to PATH",
                "opencode2 not found",
            ),
            (
                "Could not reach server at http://127.0.0.1:4096: Connection refused (os error 61)",
                "couldn't reach OpenCode",
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
        assert_eq!(
            unreachable_why("couldn't list sessions: 401 Unauthorized"),
            "401 Unauthorized"
        );
        let Answered { answer, send, .. } = answer_for(
            Decided::Unreachable(unreachable_why(&error)),
            None,
            None,
            &[],
            0,
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
        fn no_terminal(_: &str) -> Option<(PathBuf, String)> {
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
            let current = worker.current.unwrap();
            let sent = current.sent.map(|sent| sent.session);
            (current.route.answer, sent)
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

        #[test]
        fn an_always_rule_names_the_folder_of_the_session_it_sends_to() {
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
            let current = worker.current.as_mut().unwrap();
            current.sent = Some(Sent {
                delivery_id: "dlv_1".into(),
                session: "ses_board".into(),
            });
            current.offered = Some(scrawl_capture::OfferRule {
                kind: RuleKind::Site,
                key: "https://board.example.com".into(),
                folder: "/p/board".into(),
            });
            let site = |folder: &str| {
                Some((
                    RuleKind::Site,
                    "https://board.example.com".to_owned(),
                    folder.to_owned(),
                ))
            };
            assert_eq!(worker.rule_for("ses_board"), site("/p/board"));
            assert_eq!(
                worker.rule_for("ses_home"),
                site("/p/home"),
                "--to HOME --always names HOME's folder, not the offer's"
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
            let current = worker.current.as_mut().unwrap();
            // Not a delivery id, so the router's undo fails.
            current.sent = Some(Sent {
                delivery_id: "not a delivery".into(),
                session: "ses_board".into(),
            });
            current.route.delivery = Some(RouteDelivery::Sent { to: "BOARD".into() });
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
            assert!(worker.current.unwrap().sent.is_some(), "it's still sent");
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

        #[test]
        fn a_look_past_its_deadline_is_routed_without() {
            let (_looking, seen) = mpsc::sync_channel(1);
            let look = Look {
                router: router_with(|_| {}),
                seen,
                until: Instant::now() + Duration::from_millis(50),
            };
            let mut worker = worker(SharedOutcomes::default());
            let started = Instant::now();
            worker.written(written("com.example.board"), look);
            assert!(started.elapsed() < Duration::from_secs(1));
            assert!(matches!(
                worker.current.unwrap().route.answer,
                RouteAnswer::NoAnswer { .. }
            ));
        }
    }
}
