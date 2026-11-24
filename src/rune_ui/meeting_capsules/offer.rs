use super::*;

/// Who currently owns the recording window (`pill.handle`) outside an active
/// recording driven by `PillModel`. Only one occupant can own it at a time:
/// an offer, an accepted offer waiting to hand off, or a meeting finalizing /
/// showing its ready summary there.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) enum Host {
    #[default]
    Panel,
    Offered(OfferId),
    Accepted(OfferId),
    Finishing(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Route {
    Ordinary,
    Keep,
    Close,
    Inherit,
}

impl Host {
    pub(super) fn offer_on_pill(&self) -> bool {
        matches!(self, Self::Offered(_) | Self::Accepted(_))
    }

    pub(super) fn finishing_id(&self) -> Option<&str> {
        match self {
            Self::Finishing(id) => Some(id.as_str()),
            _ => None,
        }
    }

    pub(super) fn route(&mut self, event: &ControllerEvent) -> Route {
        use ControllerEvent::*;
        match (&*self, event) {
            (Self::Offered(shown) | Self::Accepted(shown), Starting(Some(id))) if *shown == *id => {
                let shown = *shown;
                *self = Self::Accepted(shown);
                Route::Keep
            }
            (Self::Offered(shown), OfferWithdrawn(id)) if *shown == *id => Route::Close,
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

/// Only the buttons of the shown offer and layout can answer it, even if
/// an input arrives between a replacement and the next frame.
pub(super) fn actions(
    p: Paint,
    id: OfferId,
    candidate: &MeetingCandidate,
    choosing: bool,
) -> (String, String, Vec<El>) {
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
                    && matches!(
                        c.state,
                        State::Offer { id: shown, choices, .. }
                            if shown == id && choices.is_open() == choosing
                    )
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
        let mut actions = vec![
            action("offer-cancel", tr("Cancel"), false, |c| {
                c.set_more_open(false)
            }),
            action("never-offer", tr("Don't offer"), false, |c| {
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
        return (label, tr("Change anytime in Settings").to_owned(), actions);
    }
    let mut actions = Vec::new();
    if candidate.provider().is_some() {
        actions.push(action("offer-more", "…", false, |c| c.set_more_open(true)).label(tr("More")));
    }
    actions.push(action("not-now", tr("Not now"), false, |c| {
        if let State::Offer { candidate, .. } = &c.state {
            tracing::info!(source = candidate.source.label(), title = %candidate.title, "meeting recording offer dismissed");
        }
        c.send(ControllerCommand::Dismiss);
        c.close();
    }));
    actions.push(action("record", tr("Record"), true, Capsule::record_offer));
    (candidate.offer_title(), String::new(), actions)
}

impl Capsule {
    fn record_offer(&mut self) {
        if self.presentation.inherits_offer_window() {
            return;
        }
        let State::Offer {
            id,
            candidate,
            rearm,
            ..
        } = &mut self.state
        else {
            return;
        };
        let (id, candidate) = (*id, candidate.clone());
        if self
            .commands
            .try_send(ControllerCommand::Record(candidate.clone(), Some(id)))
            .is_ok()
        {
            self.presentation.begin(id, candidate);
            *rearm = None;
        } else {
            tracing::warn!("could not queue the meeting recording request");
        }
    }

    #[cfg(test)]
    pub(super) fn set_offer_timer(&mut self, timer: Option<After>) {
        if let State::Offer { rearm, .. } = &mut self.state {
            *rearm = timer;
            self.arm_offer_timeout();
        }
    }

    pub(super) fn arm_offer_timeout(&self) {
        if let State::Offer {
            rearm: Some(after), ..
        } = &self.state
        {
            after(OFFER_TIMEOUT, Self::expire_offer);
        }
    }

    pub(super) fn set_more_open(&mut self, open: bool) {
        let State::Offer { choices, .. } = &mut self.state else {
            return;
        };
        if open {
            if *choices == OfferChoices::Closed {
                *choices = OfferChoices::Open;
            }
        } else {
            let expired = *choices == OfferChoices::OpenExpired;
            *choices = OfferChoices::Closed;
            if expired {
                self.arm_offer_timeout();
            }
        }
    }

    pub(super) fn expire_offer(&mut self) {
        if self.presentation.inherits_offer_window() {
            return;
        }
        let State::Offer {
            id,
            candidate,
            choices,
            ..
        } = &mut self.state
        else {
            return;
        };
        if choices.is_open() {
            *choices = OfferChoices::OpenExpired;
            return;
        }
        let id = *id;
        tracing::info!(
            source = candidate.source.label(),
            title = %candidate.title,
            "meeting recording offer timed out"
        );
        self.send(ControllerCommand::OfferTimedOut(id));
        self.close();
    }
}
