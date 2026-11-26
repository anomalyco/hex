//! Per-kind state owned by a meeting capsule, so closing moves it to
//! `State::Empty` and per-kind flags can never outlive the kind they belong to.
use super::*;

/// Whether the offer shows its Don't offer / Always record choices, and
/// whether its timeout fired while they were open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum OfferChoices {
    #[default]
    Closed,
    Open,
    /// The offer timeout fired while the choices were open; Cancel rearms it.
    OpenExpired,
}

impl OfferChoices {
    pub(super) fn is_open(self) -> bool {
        !matches!(self, Self::Closed)
    }
}

/// Hover and timeout state for a finished meeting's Ready panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ReadyHover {
    #[default]
    Idle,
    Hovered,
    /// The ready timeout fired while hovered; leaving closes the panel.
    ExpiredWhileHovered,
}

/// What a capsule currently shows, together with the state that only exists
/// for that kind.
#[derive(Default)]
pub(super) enum State {
    #[default]
    Empty,
    Reminder {
        meeting: CalendarMeeting,
        automatic: bool,
    },
    Offer {
        id: OfferId,
        candidate: MeetingCandidate,
        choices: OfferChoices,
        /// Arms another 20 s if Cancel returns to an offer whose timer already fired.
        rearm: Option<offer::After>,
    },
    CallEnded {
        app: String,
        stops: Option<(Instant, call_ended::Countdown)>,
        icon: Option<rune::Pixels>,
    },
    AutoStarted {
        title: String,
        app: String,
        icon: Option<rune::Pixels>,
    },
    Recording {
        recording: Recording,
        /// The pointer is over the recording pill (or holding it).
        hovered: bool,
        /// The command was accepted, but the worker has not reported capture stopped.
        stopping: bool,
        /// The ✕ was pressed: the panel asks "Discard recording?" until Keep,
        /// Discard, or the pointer leaving answers it.
        confirming: bool,
    },
    Finalizing {
        duration_ms: u64,
        /// Preserved while the pointer stays over the panel into Ready.
        hovered: bool,
    },
    Ready {
        manifest: Box<MeetingManifest>,
        hover: ReadyHover,
        /// What is left of its stay, drawn as the ring round ✕; held while
        /// the pointer is inside.
        stay: stay::Stay,
    },
}

impl State {
    pub(super) fn from_kind(kind: Kind, more_open: bool, rearm: Option<offer::After>) -> Self {
        match kind {
            Kind::Reminder { meeting, automatic } => Self::Reminder { meeting, automatic },
            Kind::Offer(id, candidate) => Self::Offer {
                id,
                candidate,
                choices: if more_open {
                    OfferChoices::Open
                } else {
                    OfferChoices::Closed
                },
                rearm,
            },
            Kind::CallEnded { app, stops, .. } => Self::CallEnded {
                app,
                stops,
                icon: None,
            },
            Kind::AutoStarted { title, app, .. } => Self::AutoStarted {
                title,
                app,
                icon: None,
            },
            #[cfg(test)]
            Kind::Recording(recording) => Self::Recording {
                recording,
                hovered: false,
                stopping: false,
                confirming: false,
            },
            Kind::Ready(manifest) => Self::Ready {
                manifest,
                hover: ReadyHover::Idle,
                stay: stay::Stay::new(READY_TIMEOUT),
            },
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub(super) fn more_open(&self) -> bool {
        matches!(self, Self::Offer { choices, .. } if choices.is_open())
    }

    pub(super) fn hovered(&self) -> bool {
        matches!(
            self,
            Self::Recording { hovered: true, .. }
                | Self::Finalizing { hovered: true, .. }
                | Self::Ready {
                    hover: ReadyHover::Hovered | ReadyHover::ExpiredWhileHovered,
                    ..
                }
        )
    }

    #[cfg(test)]
    pub(super) fn stopping(&self) -> bool {
        matches!(self, Self::Recording { stopping: true, .. })
    }

    #[cfg(test)]
    pub(super) fn recording_open(&self) -> bool {
        matches!(
            self,
            Self::Recording { hovered: true, .. } | Self::Recording { stopping: true, .. }
        )
    }
}
