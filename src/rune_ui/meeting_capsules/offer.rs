use super::*;

/// Starting acknowledges a particular offer. The controller's later withdrawal
/// retires the detection, not the worker it already started for that offer.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) enum Host {
    #[default]
    Panel,
    Offered(OfferId),
    Accepted(OfferId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Route {
    Ordinary,
    Keep,
    Close,
    Inherit,
}

impl Host {
    pub(super) fn on_pill(self) -> bool {
        self != Self::Panel
    }
    pub(super) fn route(&mut self, event: &ControllerEvent) -> Route {
        use ControllerEvent::*;
        match (*self, event) {
            (Self::Offered(shown) | Self::Accepted(shown), Starting(Some(id))) if shown == *id => {
                *self = Self::Accepted(shown);
                Route::Keep
            }
            (Self::Offered(shown), OfferWithdrawn(id)) if shown == *id => Route::Close,
            (
                Self::Accepted(_),
                RecordingStarted {
                    automatic: false, ..
                },
            ) => Route::Inherit,
            (Self::Accepted(_), Failed { .. }) => Route::Close,
            (
                Self::Offered(_) | Self::Accepted(_),
                Starting(_) | RecordingStarted { .. } | Transcribing,
            ) => Route::Close,
            _ => Route::Ordinary,
        }
    }
}

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

/// A website or app's name short enough for an answer's button, so a long
/// host doesn't widen the capsule.
pub(super) fn short_name(name: &str) -> String {
    const LONGEST: usize = 24;
    if name.chars().count() <= LONGEST {
        return name.to_owned();
    }
    let mut short: String = name.chars().take(LONGEST).collect();
    short.truncate(short.trim_end().len());
    short.push('…');
    short
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
        let face = match key {
            "offer-more" => IconButton::new(&crate::rune_ui::meetings::MORE_ICON)
                .variant(IconButtonVariant::Neutral)
                .control_size(ControlSize::Large)
                .help(tr("More"))
                .build(p.k)
                .w(34.0),
            "record" => Button::new(label)
                .variant(ButtonVariant::Submit)
                .build_push(p.k),
            _ => Button::new(label)
                .variant(if primary {
                    ButtonVariant::Contrast
                } else {
                    ButtonVariant::Neutral
                })
                .build_push(p.k),
        };
        face.key(key)
            .cursor(Cursor::Pointer)
            .on_press_tx(move |c: &mut Capsule, tx| {
                if !c.presentation.inherits_offer_window()
                    && c.more_open == choosing
                    && matches!(c.kind, Some(Kind::Offer(shown, _)) if shown == id)
                {
                    press(c);
                    // The surface answers in the frame of the press.
                    c.stir(tx);
                }
            })
    };
    if choosing {
        let label = candidate
            .provider()
            .map_or_else(String::new, |provider| provider.label);
        let name = short_name(&label);
        let mut actions = vec![
            action("offer-cancel", tr("Cancel"), false, |c| {
                c.set_more_open(false)
            }),
            action(
                "never-offer",
                &tr_fill("Don't offer for {}", &name),
                false,
                |c| {
                    c.choose(MeetingPolicy::NeverOffer);
                    c.send(ControllerCommand::Dismiss);
                    c.close();
                },
            ),
        ];
        if candidate.can_always_record() {
            actions.push(action(
                "always-record",
                &tr_fill("Always record {}", &name),
                true,
                |c| {
                    c.choose(MeetingPolicy::AlwaysRecord);
                    c.record_offer();
                },
            ));
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
        if self.presentation.inherits_offer_window() {
            return;
        }
        let Some(Kind::Offer(id, candidate)) = &self.kind else {
            return;
        };
        if self.send(ControllerCommand::Record(candidate.clone(), Some(*id))) {
            self.presentation.begin(*id, candidate.clone());
            self.rearm_offer = None;
        } else {
            tracing::warn!("could not queue the meeting recording request");
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
        if !open && std::mem::take(&mut self.expired) {
            self.arm_offer_timeout();
        }
    }

    pub(super) fn expire_offer(&mut self) {
        if self.presentation.inherits_offer_window() {
            return;
        }
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
        self.send(ControllerCommand::OfferTimedOut(*id));
        self.close();
    }
}
