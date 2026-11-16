use super::*;

pub(super) type After = Box<dyn Fn(Duration, fn(&mut Capsule)) + Send>;

/// Each wake belongs to one presentation, even when a different offer for
/// the same app has replaced it by the time Rune applies the update.
pub(super) fn timer(
    panel: OverlayHandle<Capsule>,
    current: Arc<AtomicU64>,
    generation: u64,
) -> After {
    Box::new(move |delay, fire| {
        let (panel, current) = (panel.clone(), current.clone());
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            panel.update(delivery(current, generation, fire));
        });
    })
}

pub(super) fn delivery(
    current: Arc<AtomicU64>,
    generation: u64,
    fire: fn(&mut Capsule),
) -> impl FnOnce(&mut Capsule, &mut Tx) {
    move |capsule, _| {
        if current.load(Ordering::SeqCst) == generation {
            fire(capsule);
        }
    }
}

/// Only the buttons of the shown offer and layout can answer it, even if
/// an input arrives between a replacement and the next frame.
pub(super) fn actions(
    p: Paint,
    id: OfferId,
    candidate: &MeetingCandidate,
    choosing: bool,
) -> (rune::Color, String, String, Vec<El>) {
    let action = |key: &str, label: &str, primary, press: fn(&mut Capsule)| {
        button(p, key, label, primary, move |c| {
            if c.more_open == choosing
                && matches!(c.kind, Some(Kind::Offer(shown, _)) if shown == id)
            {
                press(c);
            }
        })
    };
    if choosing {
        let label = candidate
            .provider()
            .map_or_else(String::new, |provider| provider.label);
        let mut actions = vec![
            action("offer-cancel", tr("Cancel"), false, |c| {
                c.set_more_open(false)
            }),
            action("never-offer", tr("Never offer"), false, |c| {
                c.choose(MeetingPolicy::NeverOffer);
                c.send(ControllerCommand::Dismiss);
                c.close();
            }),
        ];
        if candidate.can_always_record() {
            actions.push(action("always-record", tr("Always record"), true, |c| {
                c.choose(MeetingPolicy::AlwaysRecord);
                c.record_offer();
            }));
        }
        return (
            p.muted(),
            label,
            tr("Change anytime in Settings").to_owned(),
            actions,
        );
    }
    let mut actions = Vec::new();
    if candidate.provider().is_some() {
        actions.push(action("offer-more", "…", false, |c| c.set_more_open(true)).label(tr("More")));
    }
    actions.push(action("not-now", tr("Not now"), false, |c| {
        if let Some(Kind::Offer(_, candidate)) = &c.kind {
            tracing::info!(source = candidate.source.label(), title = %candidate.title, "meeting recording offer dismissed");
        }
        c.send(ControllerCommand::Dismiss);
        c.close();
    }));
    actions.push(action("record", tr("Record"), true, Capsule::record_offer));
    (p.muted(), candidate.offer_title(), String::new(), actions)
}

impl Capsule {
    fn record_offer(&mut self) {
        let Some(Kind::Offer(id, candidate)) = &self.kind else {
            return;
        };
        let command = if candidate.key == "preview" {
            ControllerCommand::Dismiss
        } else {
            ControllerCommand::Record(candidate.clone(), Some(*id))
        };
        if self.send(command) {
            self.close();
        }
    }
    pub(super) fn set_offer_timer(&mut self, timer: Option<After>) {
        self.rearm_offer = timer;
        self.arm_offer_timeout();
    }

    fn arm_offer_timeout(&self) {
        if let Some(after) = &self.rearm_offer {
            after(OFFER_TIMEOUT, Self::expire_offer);
        }
    }

    pub(super) fn set_more_open(&mut self, open: bool) {
        self.more_open = open;
        if let Some(resize) = &self.resize {
            resize(self.window_size());
        }
        if !open && std::mem::take(&mut self.expired) {
            self.arm_offer_timeout();
        }
    }

    pub(super) fn expire_offer(&mut self) {
        let Some(Kind::Offer(id, candidate)) = &self.kind else {
            return;
        };
        if self.more_open {
            self.expired = true;
            return;
        }
        tracing::info!(
            source = candidate.source.label(),
            title = %candidate.title,
            "meeting recording offer timed out"
        );
        if candidate.key != "preview" {
            self.send(ControllerCommand::OfferTimedOut(*id));
        }
        self.close();
    }
}
