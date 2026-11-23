//! Typed beat states for [`super::Onboard`]: per-beat clocks and flags live
//! inside the variant that owns them, so leaving a beat drops them
//! automatically and views never filter stale timestamps from earlier steps.

use super::Step;

/// A running `"Without Hex"` typing scene on the demo beat.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TypingRun {
    pub(crate) from: f64,
    /// The conversation came on screen open, Jay's messages landing in it,
    /// rather than growing out of the Launch Room's clicked row.
    pub(crate) in_place: bool,
}

/// Sub-phase of the demo beat (`Parked..=Done`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum DemoPhase {
    /// `"Without Hex"`, waiting for the Launch Room's row or playing it out.
    Parked,
    /// `"With Hex"` rewound and waiting for the shortcut.
    YourTurn { voice_from: f64, hud_from: f64 },
    /// `"With Hex"` shortcut down and voice take playing.
    Hearing {
        voice_from: f64,
        hud_from: f64,
        take_from: f64,
    },
    /// `"With Hex"` answer spoken, holding before score or permissions.
    Heard {
        voice_from: f64,
        hud_from: f64,
        take_from: f64,
    },
    /// Non-first-run score screen after `"With Hex"`.
    Done {
        voice_from: Option<f64>,
        hud_from: Option<f64>,
        take_from: Option<f64>,
    },
}

/// State owned by the demo beat (`Parked`, `YourTurn`, `Hearing`, `Heard`, `Done`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DemoBeat {
    pub(crate) parked_from: f64,
    pub(crate) typing: Option<TypingRun>,
    pub(crate) phase: DemoPhase,
    /// How many `"Without Hex"` bubbles have flown out of the field so far.
    pub(crate) sent_seen: usize,
    /// Whether `"With Hex"`'s reply has flown out of the field.
    pub(crate) hex_seen: bool,
}

impl DemoBeat {
    pub(crate) fn parked(now: f64) -> Self {
        Self {
            parked_from: now,
            typing: None,
            phase: DemoPhase::Parked,
            sent_seen: 0,
            hex_seen: false,
        }
    }

    pub(crate) fn step(&self) -> Step {
        match self.phase {
            DemoPhase::Parked => Step::Parked,
            DemoPhase::YourTurn { .. } => Step::YourTurn,
            DemoPhase::Hearing { .. } => Step::Hearing,
            DemoPhase::Heard { .. } => Step::Heard,
            DemoPhase::Done { .. } => Step::Done,
        }
    }

    pub(crate) fn typing_from(&self) -> Option<f64> {
        self.typing.map(|t| t.from)
    }

    pub(crate) fn in_place(&self) -> bool {
        self.typing.is_some_and(|t| t.in_place)
    }

    pub(crate) fn voice_from(&self) -> Option<f64> {
        match self.phase {
            DemoPhase::Parked => None,
            DemoPhase::YourTurn { voice_from, .. }
            | DemoPhase::Hearing { voice_from, .. }
            | DemoPhase::Heard { voice_from, .. } => Some(voice_from),
            DemoPhase::Done { voice_from, .. } => voice_from,
        }
    }

    pub(crate) fn hud_from(&self) -> Option<f64> {
        match self.phase {
            DemoPhase::Parked => None,
            DemoPhase::YourTurn { hud_from, .. }
            | DemoPhase::Hearing { hud_from, .. }
            | DemoPhase::Heard { hud_from, .. } => Some(hud_from),
            DemoPhase::Done { hud_from, .. } => hud_from,
        }
    }

    pub(crate) fn take_from(&self) -> Option<f64> {
        match self.phase {
            DemoPhase::Parked | DemoPhase::YourTurn { .. } => None,
            DemoPhase::Hearing { take_from, .. } | DemoPhase::Heard { take_from, .. } => {
                Some(take_from)
            }
            DemoPhase::Done { take_from, .. } => take_from,
        }
    }

    pub(crate) fn enter_your_turn(&mut self, now: f64) {
        self.phase = DemoPhase::YourTurn {
            voice_from: now,
            hud_from: now,
        };
    }

    pub(crate) fn enter_hearing(&mut self, now: f64) {
        let voice_from = self.voice_from().unwrap_or(now);
        let hud_from = self.hud_from().unwrap_or(now);
        self.phase = DemoPhase::Hearing {
            voice_from,
            hud_from,
            take_from: now,
        };
    }

    pub(crate) fn enter_heard(&mut self) {
        if let DemoPhase::Hearing {
            voice_from,
            hud_from,
            take_from,
        } = self.phase
        {
            self.phase = DemoPhase::Heard {
                voice_from,
                hud_from,
                take_from,
            };
        }
    }

    pub(crate) fn enter_done(&mut self) {
        self.phase = DemoPhase::Done {
            voice_from: self.voice_from(),
            hud_from: self.hud_from(),
            take_from: self.take_from(),
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermissionKind {
    Microphone,
    Accessibility,
}

/// Sub-phase of the practice beat (`Try`, `TryHearing`, `TryHeard`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum PracticePhase {
    /// Waiting for real dictation or the scripted take.
    Try,
    /// Scripted `"Your turn"` shortcut down and voice take playing.
    Hearing { take_from: f64 },
    /// Words landed (from real dictation with `take_from: None`, or scripted).
    Heard { take_from: Option<f64> },
}

/// State owned by the practice beat (`Try`, `TryHearing`, `TryHeard`).
pub(crate) struct PracticeBeat {
    pub(crate) hud_from: f64,
    pub(crate) phase: PracticePhase,
    pub(crate) live_hold_from: Option<f64>,
    /// A dictation's text landed since the plate last stepped.
    pub(crate) hud_landed: bool,
    /// The interactive try as the practice game (`game`).
    pub(crate) game: Option<super::super::game::Session>,
}

impl PracticeBeat {
    pub(crate) fn new(now: f64) -> Self {
        Self {
            hud_from: now,
            phase: PracticePhase::Try,
            live_hold_from: None,
            hud_landed: false,
            game: None,
        }
    }

    pub(crate) fn step(&self) -> Step {
        match self.phase {
            PracticePhase::Try => Step::Try,
            PracticePhase::Hearing { .. } => Step::TryHearing,
            PracticePhase::Heard { .. } => Step::TryHeard,
        }
    }

    pub(crate) fn take_from(&self) -> Option<f64> {
        match self.phase {
            PracticePhase::Try => None,
            PracticePhase::Hearing { take_from } => Some(take_from),
            PracticePhase::Heard { take_from } => take_from,
        }
    }
}

/// The active beat of an onboarding run. Replacing `beat` drops the previous
/// beat's clocks and flags automatically.
pub(crate) enum Beat {
    Pick,
    Started,
    Demo(DemoBeat),
    Permission { kind: PermissionKind, from: f64 },
    Waiting,
    Practice(Box<PracticeBeat>),
    Ready { from: f64 },
}

impl Beat {
    pub(crate) fn step(&self) -> Step {
        match self {
            Self::Pick => Step::Pick,
            Self::Started => Step::Started,
            Self::Demo(demo) => demo.step(),
            Self::Permission {
                kind: PermissionKind::Microphone,
                ..
            } => Step::Microphone,
            Self::Permission {
                kind: PermissionKind::Accessibility,
                ..
            } => Step::Accessibility,
            Self::Waiting => Step::Waiting,
            Self::Practice(practice) => practice.step(),
            Self::Ready { .. } => Step::Ready,
        }
    }

    pub(crate) fn demo(&self) -> Option<&DemoBeat> {
        match self {
            Self::Demo(demo) => Some(demo),
            _ => None,
        }
    }

    pub(crate) fn demo_mut(&mut self) -> Option<&mut DemoBeat> {
        match self {
            Self::Demo(demo) => Some(demo),
            _ => None,
        }
    }

    pub(crate) fn practice(&self) -> Option<&PracticeBeat> {
        match self {
            Self::Practice(practice) => Some(practice),
            _ => None,
        }
    }

    pub(crate) fn practice_mut(&mut self) -> Option<&mut PracticeBeat> {
        match self {
            Self::Practice(practice) => Some(practice),
            _ => None,
        }
    }

    pub(crate) fn hud_from(&self) -> Option<f64> {
        match self {
            Self::Demo(demo) => demo.hud_from(),
            Self::Practice(practice) => Some(practice.hud_from),
            _ => None,
        }
    }

    pub(crate) fn active_take_from(&self) -> Option<f64> {
        match self {
            Self::Demo(demo) => demo.take_from(),
            Self::Practice(practice) => practice.take_from(),
            _ => None,
        }
    }

    pub(crate) fn permissions_from(&self) -> Option<f64> {
        match self {
            Self::Permission { from, .. } => Some(*from),
            _ => None,
        }
    }

    pub(crate) fn ready_from(&self) -> Option<f64> {
        match self {
            Self::Ready { from } => Some(*from),
            _ => None,
        }
    }
}

impl super::Onboard {
    pub(crate) fn step(&self) -> Step {
        self.beat.step()
    }

    pub(crate) fn demo(&self) -> Option<&DemoBeat> {
        self.beat.demo()
    }

    pub(crate) fn hud_from(&self) -> Option<f64> {
        self.beat.hud_from()
    }

    pub(crate) fn practice_take_from(&self) -> Option<f64> {
        self.beat.practice().and_then(PracticeBeat::take_from)
    }

    pub(crate) fn permissions_from(&self) -> Option<f64> {
        self.beat.permissions_from()
    }

    pub(crate) fn live_hold_from(&self) -> Option<f64> {
        self.beat.practice().and_then(|p| p.live_hold_from)
    }

    pub(crate) fn set_live_hold_from(&mut self, from: Option<f64>) {
        if let Some(practice) = self.beat.practice_mut() {
            practice.live_hold_from = from;
        }
    }

    pub(crate) fn game(&self) -> Option<&super::super::game::Session> {
        self.beat.practice().and_then(|p| p.game.as_ref())
    }

    pub(crate) fn game_mut(&mut self) -> Option<&mut super::super::game::Session> {
        self.beat.practice_mut().and_then(|p| p.game.as_mut())
    }

    pub(crate) fn take_game(&mut self) -> Option<super::super::game::Session> {
        self.beat.practice_mut().and_then(|p| p.game.take())
    }

    pub(crate) fn set_game(&mut self, game: Option<super::super::game::Session>) {
        if let Some(practice) = self.beat.practice_mut() {
            practice.game = game;
        }
    }

    pub(crate) fn reset_to_pick(&mut self) {
        self.beat = Beat::Pick;
    }

    #[cfg(test)]
    pub(crate) fn enter_waiting(&mut self) {
        self.beat = Beat::Waiting;
    }

    #[cfg(test)]
    pub(crate) fn enter_try(&mut self, now: f64) {
        self.beat = Beat::Practice(Box::new(PracticeBeat::new(now)));
    }

    #[cfg(test)]
    pub(crate) fn parked_from(&self) -> Option<f64> {
        self.beat.demo().map(|d| d.parked_from)
    }

    #[cfg(test)]
    pub(crate) fn typing_from(&self) -> Option<f64> {
        self.beat.demo().and_then(DemoBeat::typing_from)
    }
}
