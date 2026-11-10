//! Recording-time preparation and nonblocking publication. Background answers
//! belong to one bundle and cannot replace a choice or Undo made in the meantime,
//! or move a session row the capsule already lists.

use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub(super) struct Update {
    live: look::Live,
    judgement: smart::Judgement,
    evidence: smart::Evidence,
    model: Option<jev::Record>,
}

pub(super) struct Pending {
    updates: Receiver<Update>,
    cancelled: Arc<AtomicBool>,
}

/// Preparation a test feeds by hand, one judgement at a time.
#[cfg(test)]
pub(super) fn fed() -> (mpsc::SyncSender<Update>, Pending) {
    let (updates, receiver) = mpsc::sync_channel(2);
    let pending = Pending {
        updates: receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    (updates, pending)
}

#[cfg(test)]
impl Update {
    pub(super) fn judged(live: look::Live, judgement: smart::Judgement) -> Self {
        Self {
            live,
            judgement,
            evidence: smart::Evidence::default(),
            model: None,
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Rules are matched using only the captured identity and the locally read URL.
/// Folder resolution is separate: no session ID is invented for a repository.
/// A page that names its session (read by the look) comes first: a site rule
/// covering the whole host can't take it elsewhere.
fn rule_live(router: &Router, loaded: &ScrawlBundle, live: &look::Live) -> look::Live {
    let config = router.config();
    let mut clues: Vec<smart::Clue> = live
        .clues
        .iter()
        .filter(|clue| clue.source == smart::Source::Signed)
        .take(1)
        .cloned()
        .collect();
    if let Some((key, target)) = live
        .url
        .as_deref()
        .and_then(|url| routing::site_rule(url, &config.site_rules()))
    {
        let pointer = match target {
            routing::SiteTarget::Session(id) => smart::Pointer::Session {
                id,
                directory: None,
            },
            routing::SiteTarget::Folder(path) => smart::Pointer::Folder(smart::Folder {
                tree: true,
                ..look::folder_of(&path)
            }),
        };
        clues.push(smart::Clue::new(
            smart::Source::Rule,
            pointer,
            format!("routing.sites sends {key}"),
        ));
    } else if let Some(clue) = loaded
        .target
        .bundle_id
        .as_deref()
        .and_then(|id| look::app_rule_clue(id, &config.app_rules()))
    {
        clues.push(clue);
    }
    look::Live {
        clues,
        ..live.clone()
    }
}

fn evidence(loaded: &ScrawlBundle, bundle: &Path, live: look::Live) -> smart::Evidence {
    look::evidence_for(loaded, bundle, live)
}

impl Worker {
    pub(super) fn written(&mut self, bundle: PathBuf, look: Look) {
        let started = Instant::now();
        let pending = self.pending.take();
        if self.current.as_ref().is_some_and(|c| c.due.is_some())
            && let Some(mut current) = self.current.take()
        {
            // The capsule now shows the new capture: the older send goes on.
            current.resume(started);
            self.older.push((current, pending));
        }
        let Ok((_, loaded)) = scrawl_capture::handoff::load_bundle(&bundle) else {
            lock(&self.outcomes).routed(&bundle, unrouted());
            return;
        };
        let live = look.local.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let ruled = rule_live(&look.router, &loaded, &live);
        let app = loaded.target.app.clone();
        let rule = policy::rule_key(live.url.as_deref(), loaded.target.bundle_id.as_deref());
        let site = rule.as_ref().map(|(_, key)| key.clone());
        let early = look.router.run(|| {
            let evidence = smart::Evidence {
                clues: ruled.clues.clone(),
                candidates: ruled.candidates.clone(),
                address: None,
                now_ms: loaded.created_at_ms,
                url: live.url.clone(),
                sessions_error: None,
            };
            smart::decide(&evidence, smart::Policy::Weigh)
        });
        let mut answered = answer_for(
            Decided::Judged(early.decision),
            live.url.as_deref(),
            site,
            &live.candidates,
            now_ms(),
        );
        if answered.send.is_none() {
            answered.answer = match ruled.clues.first().map(|c| &c.pointer) {
                Some(smart::Pointer::Folder(folder)) => RouteAnswer::ResolvingRule {
                    name: folder
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                },
                _ => RouteAnswer::Looking,
            };
        }
        self.current = Some(Routed {
            bundle: bundle.clone(),
            router: look.router.clone(),
            url: live.url,
            app,
            rule,
            candidates: live.candidates,
            route: CaptureRoute::of(answered.answer),
            sent: None,
            offered: None,
            saved: None,
            due: (!ruled.clues.is_empty()).then(|| Instant::now() + SEND_HOLD),
            held: None,
            destination: answered.send,
        });
        self.publish();
        tracing::info!(
            answer = answer_kind(&self.current.as_ref().expect("just set").route.answer),
            elapsed_ms = started.elapsed().as_millis() as u64,
            phase = "local",
            "capture routed"
        );
        let (updates, receiver) = mpsc::sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let spawn = thread::Builder::new()
            .name("capture-judge".into())
            .spawn(move || {
                let live = look
                    .seen
                    .recv_timeout(look.until.saturating_duration_since(Instant::now()))
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| look::Live {
                        error: Some("OpenCode didn't answer in time".into()),
                        ..look.local.lock().unwrap_or_else(|e| e.into_inner()).clone()
                    });
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                look.router.run(|| {
                    let rules = rule_live(&look.router, &loaded, &live);
                    let ruled = !rules.clues.is_empty();
                    // A saved rule is explicit. Background suggestions cannot override it,
                    // including a newly recognized spoken name.
                    let evidence = if ruled {
                        smart::Evidence {
                            clues: rules.clues,
                            candidates: live.candidates.clone(),
                            address: None,
                            now_ms: loaded.created_at_ms,
                            url: live.url.clone(),
                            sessions_error: live.error.clone(),
                        }
                    } else {
                        evidence(&loaded, &bundle, live.clone())
                    };
                    let policy = if look.router.config().smart_routing {
                        smart::Policy::Weigh
                    } else {
                        smart::Policy::Ladder
                    };
                    let judgement = smart::decide(&evidence, policy);
                    let refine = !ruled
                        && look.router.config().jev_routing
                        && look.router.config().smart_routing
                        && !jev::settled(&judgement);
                    let _ = updates.send(Update {
                        live: live.clone(),
                        judgement: judgement.clone(),
                        evidence: evidence.clone(),
                        model: None,
                    });
                    if !refine || cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    // The deterministic answer is already usable. The model's whole
                    // request (recent messages included) has a hard three-second budget.
                    let started = Instant::now();
                    let hooks = jev::Hooks {
                        deadline: Some(started + Duration::from_secs(3)),
                        ..look.model
                    };
                    let (judgement, mut record) = jev::judge(
                        &evidence,
                        judgement,
                        &jev::capture_state(&loaded, live.url.as_deref()),
                        &jev::capture_asking(&loaded, &live.candidates),
                        None,
                        &hooks,
                        &jev::Thresholds::configured(),
                    );
                    tracing::info!(
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        model_ms = record.ms,
                        "capture model routing finished"
                    );
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let judgement = policy::suggest(
                        policy::AutoSend::configured(),
                        judgement,
                        &mut record,
                        &live.candidates,
                    );
                    let _ = updates.send(Update {
                        live,
                        judgement,
                        evidence,
                        model: Some(record),
                    });
                });
            });
        if spawn.is_ok() {
            self.pending = Some(Pending {
                updates: receiver,
                cancelled,
            });
        } else {
            tracing::warn!("could not prepare a capture's suggestions");
            self.preparation_failed();
        }
    }

    pub(super) fn prepared(&mut self) {
        self.prepared_current();
        let current = self.current.take();
        let pending = self.pending.take();
        let older = std::mem::take(&mut self.older);
        for (routed, preparing) in older {
            self.current = Some(routed);
            self.pending = preparing;
            self.prepared_current();
            if let (Some(current), pending) = (self.current.take(), self.pending.take())
                && current.due.is_some()
            {
                self.older.push((current, pending));
            }
        }
        self.current = current;
        self.pending = pending;
    }

    fn prepared_current(&mut self) {
        self.send_due();
        loop {
            let update = match self.pending.as_ref().map(|p| p.updates.try_recv()) {
                Some(Ok(update)) => update,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.pending = None;
                    self.preparation_failed();
                    return;
                }
                _ => return,
            };
            let Some(current) = &mut self.current else {
                return;
            };
            let site = policy::rule_key(update.live.url.as_deref(), None)
                .or_else(|| current.rule.clone())
                .map(|(_, key)| key);
            let answered = answer_for(
                judged(
                    update.judgement.decision.clone(),
                    sessions_unread(&update.live).as_deref(),
                ),
                update.live.url.as_deref(),
                site,
                &update.live.candidates,
                now_ms(),
            );
            current.candidates = update.live.candidates;
            current.url = update.live.url;
            if let Some(rule) = policy::rule_key(current.url.as_deref(), None) {
                current.rule = Some(rule);
            }
            // A rule already sent may gain its recent list, but never another send.
            let send = current
                .route
                .delivery
                .is_none()
                .then_some(answered.send)
                .flatten();
            // Once posted, an updated session list cannot change the destination shown.
            if current.route.delivery.is_none() {
                current.route.answer = keep_rows(&current.route.answer, answered.answer);
            } else if let (
                RouteAnswer::Rule { to, site, .. },
                RouteAnswer::Rule {
                    suggested, recent, ..
                },
            ) = (&current.route.answer, answered.answer)
            {
                let next = RouteAnswer::Rule {
                    to: to.clone(),
                    suggested,
                    recent,
                    site: site.clone(),
                };
                current.route.answer = keep_rows(&current.route.answer, next);
            }
            if let Err(error) = routing::write_record(
                &current.bundle,
                &update.evidence,
                &update.judgement,
                update.model.as_ref(),
            ) {
                tracing::warn!(%error, "could not record capture routing");
            }
            if let Some(shown) = answered.shown {
                let _ = routing::record_shown(&current.bundle, &shown);
            }
            self.publish();
            if let Some(dest) = send {
                if let Some(current) = &mut self.current {
                    current.destination = Some(dest);
                    current
                        .due
                        .get_or_insert_with(|| Instant::now() + SEND_HOLD);
                }
                self.send_due();
            } else if let Some(current) = &mut self.current
                && current.destination.is_none()
                && !matches!(current.route.answer, RouteAnswer::ResolvingRule { .. })
            {
                current.due = None;
            }
        }
    }

    fn send_due(&mut self) {
        let now = Instant::now();
        if let Some(current) = &mut self.current
            && !current.holding(now)
            && current.due.is_some_and(|due| now >= due)
            && let Some(destination) = current.destination.take()
        {
            current.due = None;
            self.send_current(destination, None);
        }
    }

    fn preparation_failed(&mut self) {
        if let Some(current) = &mut self.current
            && current.destination.is_none()
            && matches!(
                current.route.answer,
                RouteAnswer::Looking | RouteAnswer::ResolvingRule { .. }
            )
        {
            current.due = None;
            current.route.answer = RouteAnswer::Unreachable {
                why: "Couldn't finish routing".into(),
            };
            self.publish();
        }
    }
}

/// `next` with every row `shown` already listed kept where it was, since
/// the pointer may be on it: a row takes its new name and reason (its old
/// reason when the new one is blank), sessions new to the list come after
/// it, and no row leaves. A rule's own session still leads.
fn keep_rows(shown: &RouteAnswer, next: RouteAnswer) -> RouteAnswer {
    let before = listed(shown);
    if before.is_empty() {
        return next;
    }
    let fresh: Vec<RouteDest> = listed(&next).into_iter().cloned().collect();
    if fresh.is_empty() {
        return shown.clone();
    }
    let latest = |old: &RouteDest| match fresh.iter().find(|d| d.session == old.session) {
        Some(new) if new.why.is_empty() => RouteDest {
            why: old.why.clone(),
            ..new.clone()
        },
        Some(new) => new.clone(),
        None => old.clone(),
    };
    let (above, below): (Vec<&RouteDest>, Vec<&RouteDest>) = match shown {
        RouteAnswer::Rule {
            to,
            suggested,
            recent,
            ..
        } => (
            std::iter::once(to).chain(suggested).collect(),
            recent.iter().collect(),
        ),
        RouteAnswer::Guess {
            suggested, recent, ..
        } => (suggested.iter().collect(), recent.iter().collect()),
        _ => (Vec::new(), before.clone()),
    };
    let mut suggested: Vec<RouteDest> = above.into_iter().map(latest).collect();
    let mut recent: Vec<RouteDest> = below.into_iter().map(latest).collect();
    recent.extend(
        fresh
            .iter()
            .filter(|new| !before.iter().any(|old| old.session == new.session))
            .cloned(),
    );
    match next {
        RouteAnswer::Rule { to, site, .. } => {
            suggested.retain(|dest| dest.session != to.session);
            recent.retain(|dest| dest.session != to.session);
            RouteAnswer::Rule {
                to,
                suggested,
                recent,
                site,
            }
        }
        RouteAnswer::Guess { site, .. } => RouteAnswer::Guess {
            suggested,
            recent,
            site,
        },
        RouteAnswer::NoAnswer { .. } => RouteAnswer::NoAnswer {
            recent: suggested.into_iter().chain(recent).collect(),
        },
        _ => RouteAnswer::Nothing {
            recent: suggested.into_iter().chain(recent).collect(),
        },
    }
}
