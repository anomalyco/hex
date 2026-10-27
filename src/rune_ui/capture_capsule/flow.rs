//! What the capture capsule shows, decided from what happens: the
//! capture's stages from `capture_host`, the router's answer, and the keys
//! and clicks the capture input sends. Pure: no Rune, no clock of its own.
//! [`Flow::input`] and [`Flow::tick`] move it, [`Flow::pres`] says what the
//! capsule draws, and each returns the [`Action`]s the host carries out.

use scrawl_capture::capture::Stage;

use super::tuning;

/// A send, a rule's or a pick's, can be taken back with the Undo button for this
/// long (Scrawl's `SMART_SEND_UNDO`).
pub(crate) fn undo_s() -> f64 {
    tuning::get().undo_s
}

/// The chips stay at least this long even when work starts at once, so
/// they are seen (Scrawl's `REVEAL_MIN_MS`).
pub(crate) const CHIPS_MIN_S: f64 = 0.9;
/// The chips never fold under the pointer, and stay this much longer once
/// it leaves (Scrawl's `REVEAL_LINGER_MS`).
pub(crate) const CHIPS_LINGER_S: f64 = 1.2;
/// The recording's ✕ asks "Discard?" for this long, then the capsule goes
/// back to recording; never while the pointer is on it, and this much
/// longer once it leaves.
pub(crate) const DISCARD_ASK_S: f64 = 4.0;
pub(crate) const DISCARD_LINGER_S: f64 = 1.5;
/// How to send a kept capture later, which "Kept" says.
pub(crate) const SEND_LATER: &str = "hex capture send latest";
/// The most rows each list section shows.
pub(crate) const MAX_SUGGESTED: usize = 3;
pub(crate) const MAX_RECENT: usize = 5;
/// The suggestions a finished capture shows under it, picked with 1–3.
pub(crate) const SUGGEST_ROWS: usize = 3;
/// Session names in the capsule are cut to this many characters.
const ROUTE_NAME_MAX: usize = 18;

/// What a capture records, picked from the capsule's chips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Mode {
    Area,
    Window,
    Screen,
    /// A cabled iPhone, offered only while one is.
    Iphone,
}

impl Mode {
    /// The chips, left to right: iPhone last, and only with one cabled.
    pub(crate) fn chips(phone: bool) -> impl Iterator<Item = Mode> {
        [Mode::Area, Mode::Window, Mode::Screen]
            .into_iter()
            .chain(phone.then_some(Mode::Iphone))
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Mode::Area => "Area",
            Mode::Window => "Window",
            Mode::Screen => "Screen",
            Mode::Iphone => "iPhone",
        }
    }

    /// Area and Window start once something is picked on screen; Screen
    /// and iPhone need a Record press.
    pub(crate) fn shows_record(self) -> bool {
        matches!(self, Mode::Screen | Mode::Iphone)
    }
}

/// A session the list offers, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dest {
    pub(crate) name: String,
    pub(crate) why: String,
    /// The session id a send names, so two sessions with one title never
    /// collide.
    pub(crate) session: String,
}

impl Dest {
    /// A session known by its title alone, which the host resolves.
    pub(crate) fn new(name: impl Into<String>, why: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            session: name.clone(),
            name,
            why: why.into(),
        }
    }

    pub(crate) fn session(self, session: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            ..self
        }
    }
}

/// The router's answer for a finished capture (Scrawl's design f: only a
/// rule ever sends; a guess suggests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// A rule matched, so it was sent to `to`. `site` is what the rule
    /// matches, for an always-rule sending it elsewhere instead.
    Rule {
        to: Dest,
        suggested: Vec<Dest>,
        recent: Vec<Dest>,
        site: Option<String>,
    },
    /// Guesses, best first; nothing was sent. `site` is what a rule
    /// would match ("problems.kitlangton.com").
    Guess {
        suggested: Vec<Dest>,
        recent: Vec<Dest>,
        site: Option<String>,
    },
    /// Nothing fits: recent sessions only.
    Nothing { recent: Vec<Dest> },
    /// The router never answered.
    NoAnswer { recent: Vec<Dest> },
    /// OpenCode's sessions couldn't be read, so there's no list: `why` in
    /// a few words.
    Unreachable { why: String },
}

/// What the capture engine says became of the bundle
/// (`scrawl_capture::delivery::Delivery`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Delivered {
    Sent {
        to: String,
    },
    /// `why` it didn't go, in a few words ("OpenCode can't see this Mac's files").
    Queued {
        to: String,
        why: String,
    },
}

/// How a send the capsule asked to take back ended (Hex's router undo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Undone {
    /// Taken back before the session saw it, or reverted.
    TakenBack,
    /// Too late: the session already acted on it.
    TooLate,
}

/// What failed, for the finished capsule's headline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The capture never started recording.
    Start,
    /// It recorded, but wasn't written.
    Save,
    /// The router couldn't send it to this session.
    Send(String),
}

/// Everything that moves the capsule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Input {
    /// A capture's stage, from `capture_host`'s engine events.
    Stage(Stage),
    Delivered(Delivered),
    Routed(Route),
    /// Something failed, and why, when the host knows.
    Failed(Failure, Option<String>),
    /// An iPhone was cabled (true) or unplugged.
    Phone(bool),
    /// ⌥⇧S tapped while recording: the chips spring open.
    Chips,
    /// The recording got its first click, scroll, or stroke: the chips
    /// fold, once they have been seen.
    Worked,
    /// The capture shortcut while the picker is open: record what it's on.
    Record,
    /// The send-to list's shortcut (⌥⇧V): the list opens (or, when open,
    /// nothing), and a closed capsule whose capture waits for a pick comes
    /// back with it.
    OpenList,
    /// Typed into the open list's field.
    Type(String),
    /// Backspace in the open list's field.
    Backspace,
    /// Up and Down in the open list.
    Move(i32),
    /// Return (or ⇧Return, `always`) in the open list.
    Send {
        always: bool,
    },
    /// 1–9 in the open list (1–3 in the suggestions): send to that row.
    SendRow(usize),
    /// "Always send <site> here" in the suggestions, ticked or not.
    ToggleAlways,
    Escape,
    /// The Undo button: the only way to take a send back.
    Undo,
    Undone(Undone),
    /// The ✕, or anything that should just close it.
    Dismiss,
    /// The recording it shows left the snapshot with no outcome: a chip's
    /// discard (the picker is already up) or `hex capture discard`.
    Gone,
    /// A mouse button went down while the list was open: off the capsule,
    /// the list closes, so typing goes back to the app in front.
    ClickAway,
    /// The written capture has no words: the microphone heard no speech.
    NothingHeard,
    /// The router sent the capture, from the capsule or not
    /// (`hex capture send`): it no longer waits for a pick.
    Settled,
}

/// What the host should do about an input or a click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Start recording in this mode: the capsule's Record, or the capture
    /// shortcut while it picks. The picker turns it into [`Self::Start`]
    /// on what it shows; without one, it records the pointer's display.
    Record(Mode),
    /// Start recording what the picker picked on screen.
    Start(crate::capture_host::CaptureTarget),
    /// Switch the recording's mode.
    Switch(Mode),
    Finish,
    Cancel,
    /// Drop the recording without an outcome: a chip turned the capsule
    /// into the picker in its place.
    Discard,
    /// Take back the send to `to`.
    Undo {
        to: String,
    },
    /// Send the capture to the session `to`; `always` saves a rule for
    /// `site`.
    Send {
        to: String,
        always: Option<String>,
    },
    /// Paste the capture into the app in front.
    Paste,
    /// Open the capture to look it over.
    Inspect,
}

/// The glyph before the finished capsule's label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lead {
    Check,
    Warning,
}

/// A moment the capsule marks with a sound ([`Flow::take_cues`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cue {
    /// A capture started recording.
    Start,
    /// It stopped and is being written.
    Stop,
    /// A send went: a pick at once, a rule's once the router says so.
    Sent,
    /// A send was taken back, or a recording cancelled.
    TakenBack,
    Failed,
}

/// The second footer hint in the open list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Footer {
    /// Escape keeps it copied (the list is open, so it is Hex's).
    KeepCopied,
    /// Escape keeps it where the rule sent it.
    Keep(String),
    /// ⇧⏎ always sends this site to the selected session.
    Always(String),
    /// The suggestions: 1–3 send, the list shortcut searches every session.
    Suggesting,
}

/// The list under the open capsule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct List {
    pub(crate) suggested: Vec<Dest>,
    pub(crate) recent: Vec<Dest>,
    /// Across both sections, suggested first.
    pub(crate) selected: usize,
    pub(crate) footer: Footer,
    /// The suggestions' "Always send <site> here", and whether it's ticked.
    pub(crate) always: Option<(String, bool)>,
}

impl List {
    /// The suggestions a capture opens with, not a search.
    pub(crate) fn suggesting(&self) -> bool {
        self.footer == Footer::Suggesting
    }
}

impl List {
    pub(crate) fn rows(&self) -> impl Iterator<Item = &Dest> {
        self.suggested.iter().chain(&self.recent)
    }

    pub(crate) fn selected_dest(&self) -> Option<&Dest> {
        self.rows().nth(self.selected)
    }
}

/// The finished capsule: its lead, a quiet word on why, the Undo chip,
/// the route chips, the ⌥⇧V chip, and (open) the field with its list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Done {
    pub(crate) lead: Lead,
    pub(crate) text: String,
    pub(crate) why: Option<String>,
    /// It can still be taken back: the Undo chip shows, with no countdown.
    pub(crate) undo: bool,
    /// The route chips' labels, best first ("Send to SEER", "HEX", …).
    /// The flow names none now: a saved rule has no runner-up.
    pub(crate) routes: Vec<String>,
    /// The ⌥⇧V chip's label.
    pub(crate) ask: Option<String>,
    /// Its keycaps, the list's shortcut as saved when shown, so a change
    /// lays the chip out again.
    pub(crate) ask_keys: Vec<String>,
    /// Open: what's typed into the field, and the list under it.
    pub(crate) open: Option<(String, List)>,
    /// A written capture: Paste and Inspect, as Scrawl's capsule offers.
    pub(crate) hand_off: bool,
}

impl Done {
    pub(crate) fn plain(lead: Lead, text: impl Into<String>) -> Self {
        Self {
            lead,
            text: text.into(),
            why: None,
            undo: false,
            routes: Vec::new(),
            ask: None,
            ask_keys: Vec::new(),
            open: None,
            hand_off: false,
        }
    }
}

/// What the capsule draws.
#[expect(
    clippy::large_enum_variant,
    reason = "a few live at a time, and the finished capsule is most of what's drawn"
)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pres {
    Hidden,
    /// `phone`: an iPhone is cabled, so it has a chip.
    Selecting {
        mode: Mode,
        phone: bool,
    },
    /// `discarding`: its ✕ was pressed, and it asks before discarding.
    Recording {
        chips: bool,
        phone: bool,
        discarding: bool,
    },
    Processing,
    Done(Done),
}

impl Pres {
    pub(crate) fn phase(&self) -> &'static str {
        match self {
            Pres::Hidden => "hidden",
            Pres::Selecting { .. } => "selecting",
            Pres::Recording { .. } => "recording",
            Pres::Processing => "processing",
            Pres::Done(_) => "done",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Hidden,
    Selecting(Mode),
    /// `discard_until`: the ✕ asks "Discard?" until then (Escape never
    /// ends a recording, so discarding takes two clicks).
    Recording {
        chips_until: Option<f64>,
        discard_until: Option<f64>,
    },
    Processing,
    /// Written, with no word from the router (yet).
    Saved {
        at: f64,
    },
    Routed(Routed),
    /// Sent elsewhere from the list, or a rule saved.
    Sent(Sent),
    Queued {
        why: String,
        at: f64,
    },
    /// Taken back with nowhere else to offer.
    TakenBack {
        at: f64,
    },
    /// Dismissed after a take-back: the bundle stays, sent nowhere.
    Kept {
        at: f64,
    },
    TooLate {
        to: String,
        at: f64,
    },
    Failed {
        at: f64,
        what: Failure,
        why: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Routed {
    route: Route,
    /// When the rule sent it, for its Undo window.
    at: f64,
    open: Option<Open>,
    /// An Undo answered: it stays, offering the list again, until a pick
    /// or a dismissal, and never leaves by itself.
    taken_back: bool,
    /// The Undo came too late: this session already has it, and a pick
    /// sends a copy elsewhere.
    already_with: Option<String>,
    /// The session the send was taken back from. The list opens as the
    /// Undo is asked for, so a "too late" answer can still name it.
    taken_from: Option<Dest>,
    /// A rule's send went: the router said so, so it sounds Sent.
    delivered: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Open {
    query: String,
    selected: usize,
    /// Searching every session (the list shortcut): the field
    /// takes typing. Otherwise it's the suggestions a capture opens with.
    typing: bool,
    /// "Always send <site> here" is ticked: the pick saves a rule.
    always: bool,
}

impl Open {
    fn suggesting() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            typing: false,
            always: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Sent {
    to: String,
    /// The session id it went to, so taking it back moves that session,
    /// not one sharing its title, to the end of the list.
    session: String,
    rule_saved: bool,
    at: f64,
    /// The route it was picked from, offered again when the send is taken
    /// back.
    back: Option<Route>,
    /// The capsule's own pick, which the router can take back for
    /// [`undo_s`]. A queued send that went on a retry can't be.
    undoable: bool,
}

impl Sent {
    fn picked(dest: &Dest, rule_saved: bool, at: f64, back: Option<Route>) -> Self {
        Self {
            to: dest.name.clone(),
            session: dest.session.clone(),
            rule_saved,
            at,
            back,
            undoable: true,
        }
    }

    fn from(&self) -> Dest {
        Dest::new(self.to.clone(), "").session(self.session.clone())
    }
}

/// The capsule's state machine.
#[derive(Clone, Debug)]
pub(crate) struct Flow {
    phase: Phase,
    /// The pointer is on the capsule.
    hovered: bool,
    /// No finished capsule leaves before this: a fold or the pointer's
    /// leaving gives it a moment more.
    hold_floor: f64,
    /// An iPhone is cabled.
    phone: bool,
    /// The iPhone chip asked to switch, and its new recording hasn't shown:
    /// a failure now is that start's, not the discarded capture's save.
    switching: bool,
    /// Sounds due, in order, for the host to play.
    cues: Vec<Cue>,
    /// The capture shown has no words.
    silent: bool,
    /// The written capture shown was closed (dismissed, or its hold ran
    /// out); news of it leaves the capsule closed.
    closed: bool,
    /// The latest capture's list, closed before anything was picked: the
    /// send-to list shortcut brings it back until a pick, a paste, a send
    /// from elsewhere (`hex capture send`), or the next capture.
    waiting: Option<Routed>,
    /// The latest capture was pasted, inspected, or sent from elsewhere:
    /// closing its capsule leaves nothing waiting.
    settled: bool,
    /// An Undo was asked for and the router hasn't said how it ended: news
    /// of the send it takes back (it went, queued, or failed) changes
    /// nothing.
    undoing: bool,
    /// A pick came while that Undo was unanswered. The router takes the
    /// earlier send back before it sends the pick, so the next answer to an
    /// Undo is the earlier send's, and so is news of a send elsewhere: the
    /// pick shown stands.
    superseded: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Self::new()
    }
}

impl Flow {
    pub(crate) fn new() -> Self {
        Self {
            phase: Phase::Hidden,
            hovered: false,
            hold_floor: 0.0,
            phone: false,
            switching: false,
            cues: Vec::new(),
            silent: false,
            closed: false,
            waiting: None,
            settled: false,
            undoing: false,
            superseded: false,
        }
    }

    /// The sounds due since the last call.
    pub(crate) fn take_cues(&mut self) -> Vec<Cue> {
        std::mem::take(&mut self.cues)
    }

    /// The cue for moving from `before` to the current phase, if any.
    fn cue(&mut self, before: &Phase, actions: &[Action]) {
        let cue = match (before, &self.phase) {
            (Phase::Recording { .. }, Phase::Recording { .. }) => None,
            (_, Phase::Recording { .. }) => Some(Cue::Start),
            (Phase::Recording { .. }, Phase::Processing) => Some(Cue::Stop),
            (Phase::Failed { .. }, Phase::Failed { .. }) => None,
            (_, Phase::Failed { .. }) => Some(Cue::Failed),
            (Phase::TakenBack { .. }, Phase::TakenBack { .. }) => None,
            (_, Phase::TakenBack { .. }) => Some(Cue::TakenBack),
            (Phase::Sent(_), Phase::Sent(_)) => None,
            (_, Phase::Sent(_)) => Some(Cue::Sent),
            // The list shortcut bringing a taken-back capture back.
            (Phase::Hidden | Phase::Kept { .. }, Phase::Routed(_)) => None,
            (
                Phase::Routed(Routed {
                    taken_back: true, ..
                }),
                _,
            ) => None,
            (
                _,
                Phase::Routed(Routed {
                    taken_back: true,
                    already_with: None,
                    ..
                }),
            ) => Some(Cue::TakenBack),
            (Phase::Recording { .. } | Phase::Selecting(_), Phase::Hidden)
                if actions.contains(&Action::Cancel) =>
            {
                Some(Cue::TakenBack)
            }
            _ => None,
        };
        self.cues.extend(cue);
    }

    /// The pointer came onto the capsule or left it.
    pub(crate) fn hover(&mut self, on: bool, now: f64) {
        self.hovered = on;
        if !on {
            self.hold_floor = self.hold_floor.max(now + CHIPS_LINGER_S);
        }
        if let (
            false,
            Phase::Recording {
                chips_until,
                discard_until,
            },
        ) = (on, &mut self.phase)
        {
            if let Some(until) = chips_until {
                *until = until.max(now + CHIPS_LINGER_S);
            }
            if let Some(until) = discard_until {
                *until = until.max(now + DISCARD_LINGER_S);
            }
        }
    }

    pub(crate) fn input(&mut self, input: Input, now: f64) -> Vec<Action> {
        let before = self.phase.clone();
        let actions = self.step(input, now);
        self.cue(&before, &actions);
        self.closing(&before);
        self.left_waiting(before, &actions);
        actions
    }

    /// A list closed with nothing picked waits for the list shortcut.
    fn left_waiting(&mut self, before: Phase, actions: &[Action]) {
        // Taken back, it's sent nowhere again: it waits for a pick.
        if actions
            .iter()
            .any(|action| matches!(action, Action::Undo { .. }))
        {
            self.undoing = true;
            self.settled = false;
        }
        // A pick while an Undo is unanswered: the router answers the Undo
        // first, then sends the pick, so news of the pick counts.
        if self.undoing
            && actions
                .iter()
                .any(|action| matches!(action, Action::Send { .. }))
        {
            self.undoing = false;
            self.superseded = true;
        }
        if actions
            .iter()
            .any(|action| matches!(action, Action::Paste | Action::Inspect))
        {
            self.settled = true;
        }
        if self.settled
            || actions
                .iter()
                .any(|action| matches!(action, Action::Send { .. }))
        {
            self.waiting = None;
            return;
        }
        if let Phase::Routed(routed) = before
            && matches!(self.phase, Phase::Hidden | Phase::Kept { .. })
        {
            self.waiting = waits(routed);
        }
    }

    fn closing(&mut self, before: &Phase) {
        let written = !matches!(
            before,
            Phase::Hidden | Phase::Selecting(_) | Phase::Recording { .. } | Phase::Processing
        );
        if written && self.phase == Phase::Hidden {
            self.closed = true;
        }
    }

    fn step(&mut self, input: Input, now: f64) -> Vec<Action> {
        let mut actions = Vec::new();
        if input == Input::NothingHeard {
            self.silent = true;
            return actions;
        }
        if input == Input::Stage(Stage::Recording) {
            self.silent = false;
        }
        if matches!(input, Input::Stage(_)) {
            self.closed = false;
            self.waiting = None;
            self.settled = false;
            self.undoing = false;
            self.superseded = false;
        }
        if let Input::Undone(undone) = input {
            // The earlier send's, taken back before the pick went.
            if std::mem::take(&mut self.superseded) {
                return actions;
            }
            self.undoing = false;
            // Too late, it stays where it went.
            self.settled |= undone == Undone::TooLate;
        }
        // The send an Undo takes back, arriving late (it went, queued, or
        // failed), changes nothing: the list it reopened stays.
        if self.undoing
            && matches!(
                input,
                Input::Settled | Input::Delivered(_) | Input::Failed(Failure::Send(_), _)
            )
        {
            return actions;
        }
        // News of the earlier send under the pick shown says nothing of it.
        if self.superseded
            && let Phase::Sent(sent) = &self.phase
            && match &input {
                Input::Delivered(Delivered::Sent { to } | Delivered::Queued { to, .. })
                | Input::Failed(Failure::Send(to), _) => *to != sent.to,
                _ => false,
            }
        {
            return actions;
        }
        if matches!(input, Input::Settled | Input::Delivered(_)) {
            self.waiting = None;
            if input == Input::Settled {
                self.settled = true;
                // A rule's send went: it sounds Sent, once.
                if let Phase::Routed(routed) = &mut self.phase
                    && matches!(routed.route, Route::Rule { .. })
                    && !std::mem::replace(&mut routed.delivered, true)
                {
                    self.cues.push(Cue::Sent);
                }
                return actions;
            }
        }
        if let Input::Phone(phone) = input {
            self.phone = phone;
            if !phone && self.phase == Phase::Selecting(Mode::Iphone) {
                self.phase = Phase::Selecting(Mode::Screen);
            }
            return actions;
        }
        let switching = matches!(input, Input::Stage(_) | Input::Failed(..) | Input::Gone)
            && std::mem::take(&mut self.switching);
        let phase = std::mem::replace(&mut self.phase, Phase::Hidden);
        self.phase = match (phase, input) {
            (_, Input::Stage(stage)) => match stage {
                Stage::Recording => Phase::Recording {
                    chips_until: None,
                    discard_until: None,
                },
                Stage::Stopping | Stage::Transcribing | Stage::Rendering | Stage::Writing => {
                    Phase::Processing
                }
                Stage::Done => Phase::Saved { at: now },
                Stage::Cancelled => Phase::Hidden,
                Stage::Failed => Phase::Failed {
                    at: now,
                    what: Failure::Save,
                    why: None,
                },
            },
            (_, Input::Failed(what, why)) => Phase::Failed {
                at: now,
                what: match what {
                    Failure::Save if switching => Failure::Start,
                    what => what,
                },
                why: why.as_deref().map(short_reason),
            },
            (Phase::Recording { discard_until, .. }, Input::Chips) => Phase::Recording {
                chips_until: Some(now + tuning::get().chips_open_s),
                discard_until,
            },
            (
                Phase::Recording {
                    chips_until,
                    discard_until,
                },
                Input::Worked,
            ) => Phase::Recording {
                chips_until: chips_until.map(|until| {
                    until
                        .min(until - tuning::get().chips_open_s + CHIPS_MIN_S)
                        .max(now)
                }),
                discard_until,
            },
            (Phase::Recording { .. }, Input::Gone) => Phase::Hidden,
            (Phase::Selecting(mode), Input::Record) => {
                actions.push(Action::Record(mode));
                Phase::Selecting(mode)
            }
            // Closed before the router answered (dismissed, or the answer
            // came after the hold): it stays closed, and `hex capture list`
            // and `hex capture send` still offer the capture.
            // A guess still waits for the list shortcut.
            (Phase::Hidden, Input::Routed(route)) if self.closed => {
                if !self.settled {
                    self.waiting = waits(Routed {
                        route,
                        at: now,
                        open: None,
                        taken_back: false,
                        already_with: None,
                        taken_from: None,
                        delivered: false,
                    });
                }
                Phase::Hidden
            }
            (Phase::Hidden, Input::Delivered(_)) if self.closed => Phase::Hidden,
            // Closed with nothing picked: the list shortcut brings the list
            // back.
            (Phase::Hidden | Phase::Kept { .. }, Input::OpenList) if self.waiting.is_some() => {
                let routed = self.waiting.take().expect("checked");
                self.phase = Phase::Routed(Routed { at: now, ..routed });
                return self.step(Input::OpenList, now);
            }
            // A queued send went on a retry: Hex can't take it back.
            (_, Input::Delivered(delivered)) => match delivered {
                Delivered::Sent { to } => Phase::Sent(Sent {
                    undoable: false,
                    ..Sent::picked(&Dest::new(to, ""), false, now, None)
                }),
                Delivered::Queued { why, .. } => Phase::Queued { why, at: now },
            },
            // Only a rule Kit saved sends by itself; anything else opens on
            // its suggestions, and nothing goes until he picks.
            (_, Input::Routed(route)) => {
                let suggests = !matches!(route, Route::Rule { .. } | Route::Unreachable { .. })
                    && sections(&route) != (&[][..], &[][..]);
                Phase::Routed(Routed {
                    route,
                    at: now,
                    open: suggests.then(Open::suggesting),
                    taken_back: false,
                    already_with: None,
                    taken_from: None,
                    delivered: false,
                })
            }
            (Phase::Routed(routed), Input::OpenList)
                if matches!(routed.route, Route::Unreachable { .. }) =>
            {
                Phase::Routed(routed)
            }
            (Phase::Routed(mut routed), Input::OpenList) => {
                match &mut routed.open {
                    Some(open) if !open.typing => {
                        open.typing = true;
                        open.selected = 0;
                    }
                    Some(_) => {}
                    None => {
                        // A guess opens on itself; a rule on the next best place.
                        let selected = usize::from(matches!(routed.route, Route::Rule { .. }));
                        routed.open = Some(Open {
                            selected,
                            typing: true,
                            ..Open::suggesting()
                        });
                    }
                }
                clamp(&mut routed);
                Phase::Routed(routed)
            }
            (Phase::Routed(mut routed), input @ (Input::Type(_) | Input::Backspace)) => {
                if let Some(open) = routed.open.as_mut().filter(|open| open.typing) {
                    match input {
                        Input::Type(typed) => open.query.push_str(&typed),
                        _ => {
                            open.query.pop();
                        }
                    }
                    open.selected = 0;
                }
                Phase::Routed(routed)
            }
            (Phase::Routed(mut routed), Input::Move(by)) => {
                if let Some(open) = &mut routed.open {
                    open.selected = (open.selected as i64 + i64::from(by)).max(0) as usize;
                }
                clamp(&mut routed);
                Phase::Routed(routed)
            }
            (Phase::Routed(routed), Input::Send { always }) => {
                let selected = routed.open.as_ref().map(|open| open.selected);
                send(routed, selected, always, now, &mut actions)
            }
            (Phase::Routed(routed), Input::SendRow(row)) => {
                send(routed, Some(row), false, now, &mut actions)
            }
            (Phase::Routed(mut routed), Input::ToggleAlways) => {
                if let Some(open) = &mut routed.open {
                    open.always = !open.always;
                }
                Phase::Routed(routed)
            }
            // Taken back, folding it starts its hold: it leaves like any
            // finished capsule, and the list shortcut brings it back.
            (Phase::Routed(mut routed), Input::ClickAway) if !self.hovered => {
                if routed.taken_back {
                    routed.at = now;
                }
                if routed.open.take().is_some() {
                    self.fold_hold(now);
                }
                Phase::Routed(routed)
            }
            (phase, Input::ClickAway) => phase,
            // Taken back, ✕ (or Escape in its open list) is the dismissal:
            // the bundle is kept. Too late, it's where it went.
            (
                Phase::Routed(Routed {
                    taken_back: true,
                    already_with,
                    open,
                    ..
                }),
                input @ (Input::Escape | Input::Dismiss),
            ) if input == Input::Dismiss || open.as_ref().is_some_and(|open| open.typing) => {
                match already_with {
                    None => Phase::Kept { at: now },
                    Some(_) => Phase::Hidden,
                }
            }
            // Escape only reaches an open list; it folds it. It never takes
            // a send back: that is the Undo button's.
            (Phase::Routed(mut routed), Input::Escape)
                if routed.open.as_ref().is_some_and(|open| open.typing) =>
            {
                if routed.open.take().is_some() {
                    self.fold_hold(now);
                }
                Phase::Routed(routed)
            }
            (Phase::Routed(routed), Input::Undo) => {
                // The chip is only there while the window runs; a click that
                // lands as it ends still counts, and the router answers too
                // late if it was.
                match (rule_to(&routed.route).cloned(), !routed.taken_back) {
                    (Some(to), true) => {
                        actions.push(Action::Undo {
                            to: to.name.clone(),
                        });
                        taking_back(Some(routed.route), &to, now)
                    }
                    _ => Phase::Routed(routed),
                }
            }
            (Phase::Sent(sent), Input::Undo) if sent.undoable => {
                actions.push(Action::Undo {
                    to: sent.to.clone(),
                });
                let from = sent.from();
                taking_back(sent.back, &from, now)
            }
            // Nothing to list: dismissing says where the bundle is.
            (Phase::TakenBack { .. }, Input::Dismiss) => Phase::Kept { at: now },
            // Escape closes the picker, but never ends a recording: it may be
            // meant for the app being recorded.
            (Phase::Selecting(_), Input::Escape) => {
                actions.push(Action::Cancel);
                Phase::Hidden
            }
            // Pasted or dismissed: the take-back it asked for says nothing.
            (Phase::Hidden, Input::Undone(_)) => Phase::Hidden,
            // The list opened as the Undo was asked for. Taken back, it
            // stays as it is; too late, the session it went to has it.
            (Phase::Routed(mut routed), Input::Undone(undone)) if routed.taken_back => {
                if let (Undone::TooLate, None, Some(from)) =
                    (undone, &routed.already_with, routed.taken_from.clone())
                {
                    if let Route::Guess {
                        suggested, recent, ..
                    } = &mut routed.route
                    {
                        for dest in suggested.iter_mut().chain(recent.iter_mut()) {
                            if dest.session == from.session {
                                dest.why = "has it".into();
                            }
                        }
                    }
                    routed.already_with = Some(from.name);
                }
                Phase::Routed(routed)
            }
            (Phase::TakenBack { at }, Input::Undone(_)) => Phase::TakenBack { at },
            // An Undo never ends the capture: the list comes back with the
            // same sessions, the one it went to last, until a pick or a
            // dismissal. Too late to take back, a pick sends a copy.
            (phase, Input::Undone(undone)) => {
                let (route, from) = match phase {
                    Phase::Routed(Routed {
                        route: route @ Route::Rule { .. },
                        ..
                    }) => {
                        let from = rule_to(&route).cloned();
                        (Some(route), from)
                    }
                    Phase::Sent(sent) => {
                        let from = sent.from();
                        (sent.back, Some(from))
                    }
                    other if undone == Undone::TooLate => {
                        self.phase = other;
                        return actions;
                    }
                    _ => (None, None),
                };
                let too_late = undone == Undone::TooLate;
                match (route, &from) {
                    (Some(route), Some(from)) => offered_again(route, from, too_late, now),
                    _ => None,
                }
                .map_or_else(
                    || match (too_late, from) {
                        (true, Some(from)) => Phase::TooLate {
                            to: from.name,
                            at: now,
                        },
                        _ => Phase::TakenBack { at: now },
                    },
                    Phase::Routed,
                )
            }
            // Escape is the app's everywhere else: it is never claimed, so
            // one that arrives is ignored.
            (phase, Input::Escape | Input::Undo) => phase,
            (_, Input::Dismiss) => Phase::Hidden,
            (phase, _) => phase,
        };
        actions
    }

    /// What a click on `id` does: the action for the host, after moving
    /// the capsule as the click shows.
    pub(crate) fn click(&mut self, id: super::surface::Id, now: f64) -> Vec<Action> {
        let before = self.phase.clone();
        let cued = self.cues.len();
        let actions = self.press(id, now);
        // A click that became an input cued already.
        if self.cues.len() == cued {
            self.cue(&before, &actions);
        }
        self.closing(&before);
        self.left_waiting(before, &actions);
        actions
    }

    fn press(&mut self, id: super::surface::Id, now: f64) -> Vec<Action> {
        use super::surface::Id;
        match (&self.phase, id) {
            (Phase::Selecting(current), Id::Mode(mode)) => {
                if *current == mode && mode.shows_record() {
                    return vec![Action::Record(mode)];
                }
                self.phase = Phase::Selecting(mode);
                Vec::new()
            }
            (Phase::Selecting(mode), Id::Record) => vec![Action::Record(*mode)],
            // A chip restarts rather than retargets (Scrawl's
            // `switch_recording`): Screen is what's recording, so it only
            // folds the chips; iPhone starts the phone in its place; Window
            // and Area drop it and become the picker.
            (Phase::Recording { .. }, Id::Mode(mode)) => match mode {
                Mode::Screen => {
                    self.phase = Phase::Recording {
                        chips_until: None,
                        discard_until: None,
                    };
                    Vec::new()
                }
                Mode::Iphone => {
                    self.phase = Phase::Recording {
                        chips_until: None,
                        discard_until: None,
                    };
                    self.switching = true;
                    vec![Action::Switch(mode)]
                }
                Mode::Area | Mode::Window => {
                    self.phase = Phase::Selecting(mode);
                    vec![Action::Discard]
                }
            },
            (Phase::Recording { .. }, Id::Finish) => vec![Action::Finish],
            // Discarding a recording takes a second click: ✕ asks first.
            (Phase::Recording { .. }, Id::Cancel | Id::Close) => {
                self.phase = Phase::Recording {
                    chips_until: None,
                    discard_until: Some(now + DISCARD_ASK_S),
                };
                Vec::new()
            }
            (Phase::Recording { .. }, Id::Keep) => {
                self.phase = Phase::Recording {
                    chips_until: None,
                    discard_until: None,
                };
                Vec::new()
            }
            (
                Phase::Recording {
                    discard_until: Some(_),
                    ..
                },
                Id::Discard,
            )
            | (Phase::Selecting(_), Id::Cancel | Id::Close) => {
                self.phase = Phase::Hidden;
                vec![Action::Cancel]
            }
            // Pasting here takes back a send still waiting out its Undo, as
            // Scrawl's Paste does.
            (_, Id::Paste) => {
                let mut actions: Vec<Action> = self.undoable(now).into_iter().collect();
                actions.push(Action::Paste);
                self.phase = Phase::Hidden;
                actions
            }
            (_, Id::Inspect) => {
                self.phase = Phase::Hidden;
                vec![Action::Inspect]
            }
            (_, Id::Undo) => self.input(Input::Undo, now),
            (_, Id::Ask) => self.input(Input::OpenList, now),
            (_, Id::Row(row)) => self.input(Input::SendRow(row), now),
            (_, Id::Always) => self.input(Input::ToggleAlways, now),
            // ✕ beside "Sent to X" just closes, a rule's as a pick's.
            (_, Id::Close) => self.input(Input::Dismiss, now),
            _ => Vec::new(),
        }
    }

    /// The keys the capsule takes from Hex's tap: the picker's, an open
    /// list's (Escape only in a list Kit opened or the picker), or a routed
    /// capture's list shortcut; closed, the list shortcut while the capture
    /// waits for a pick. With no sessions to list, the list shortcut goes
    /// to the app in front.
    pub(crate) fn keys(&self) -> Option<crate::capture_host::CapsuleKeys> {
        use crate::capture_host::CapsuleKeys;
        match &self.phase {
            Phase::Hidden | Phase::Kept { .. } => self.waiting.as_ref().map(|_| CapsuleKeys::Route),
            Phase::Selecting(_) => Some(CapsuleKeys::Picker),
            // Nothing to list, so ⌥⇧V reaches the app; ✕ dismisses, or it
            // leaves by itself.
            Phase::TakenBack { .. } => None,
            Phase::Routed(Routed {
                open: Some(open),
                route,
                ..
            }) => Some(if open.typing {
                CapsuleKeys::List
            } else {
                let rows = list(route, open).rows().count();
                CapsuleKeys::Suggest {
                    rows: u8::try_from(rows).unwrap_or(u8::MAX),
                }
            }),
            Phase::Routed(Routed {
                route: Route::Unreachable { .. },
                ..
            }) => None,
            // Never Escape here: it goes to the app in front, and a send is
            // taken back only by clicking Undo.
            Phase::Routed(_) => Some(CapsuleKeys::Route),
            // A send from the list has no list to open: ⌥⇧V goes to the app.
            Phase::Sent(_) => None,
            _ => None,
        }
    }

    /// The take-back for a send still waiting out its Undo.
    fn undoable(&self, now: f64) -> Option<Action> {
        let (to, at) = match &self.phase {
            Phase::Routed(Routed {
                route: Route::Rule { to, .. },
                at,
                ..
            }) => (&to.name, *at),
            Phase::Sent(Sent {
                to,
                at,
                undoable: true,
                ..
            }) => (to, *at),
            _ => return None,
        };
        (now - at < undo_s()).then(|| Action::Undo { to: to.clone() })
    }

    /// Runs what time decides: the chips closing, the Undo window ending,
    /// the finished capsule leaving. True when one was due.
    pub(crate) fn tick(&mut self, now: f64) -> bool {
        match &mut self.phase {
            Phase::Recording {
                chips_until,
                discard_until,
            } => {
                let mut due = false;
                for until in [chips_until, discard_until] {
                    if !self.hovered && until.is_some_and(|t| now >= t) {
                        *until = None;
                        due = true;
                    }
                }
                due
            }
            phase => {
                let due = !self.hovered
                    && hold_until(phase).is_some_and(|at| now >= at.max(self.hold_floor));
                if due {
                    let before = std::mem::replace(&mut self.phase, Phase::Hidden);
                    self.closed = true;
                    self.left_waiting(before, &[]);
                }
                due
            }
        }
    }

    /// A folded list leaves the capsule up a brief moment more, not
    /// at once.
    fn fold_hold(&mut self, now: f64) {
        self.hold_floor = self.hold_floor.max(now + tuning::get().brief_hold_s);
    }

    /// When `tick` next has something to do.
    pub(crate) fn next_due(&self, now: f64) -> Option<f64> {
        // The Undo chip leaves when the window ends; it counts nothing down.
        let undo_at = |at: f64| {
            let end = at + undo_s();
            (end > now).then_some(end)
        };
        let due = match &self.phase {
            Phase::Recording {
                chips_until,
                discard_until,
            } => [*chips_until, *discard_until]
                .into_iter()
                .flatten()
                .min_by(f64::total_cmp)
                .filter(|_| !self.hovered),
            Phase::Routed(Routed {
                route: Route::Rule { .. },
                at,
                ..
            }) => undo_at(*at),
            Phase::Sent(sent) if sent.undoable => undo_at(sent.at),
            _ => None,
        };
        let hold = hold_until(&self.phase)
            .map(|at| at.max(self.hold_floor))
            .filter(|_| !self.hovered);
        [due, hold].into_iter().flatten().min_by(f64::total_cmp)
    }

    /// What the capsule draws at `now`.
    pub(crate) fn pres(&self, now: f64) -> Pres {
        let mut pres = self.pres_of(now);
        // Scrawl's "No speech": a capture with no words still copies.
        if let Pres::Done(done) = &mut pres
            && self.silent
            && done.text == "Copied"
            && done.why.is_none()
        {
            done.why = Some("no speech heard".into());
        }
        pres
    }

    fn pres_of(&self, now: f64) -> Pres {
        let undo_left = |at: f64| now - at < undo_s();
        match &self.phase {
            Phase::Hidden => Pres::Hidden,
            Phase::Selecting(mode) => Pres::Selecting {
                mode: *mode,
                phone: self.phone,
            },
            Phase::Recording {
                chips_until,
                discard_until,
            } => {
                let discarding = discard_until.is_some_and(|t| self.hovered || now < t);
                Pres::Recording {
                    chips: !discarding && chips_until.is_some_and(|t| self.hovered || now < t),
                    phone: self.phone,
                    discarding,
                }
            }
            Phase::Processing => Pres::Processing,
            Phase::Saved { .. } => Pres::Done(Done {
                hand_off: true,
                ..Done::plain(Lead::Check, "Copied")
            }),
            Phase::Failed { what, why, .. } => Pres::Done(Done {
                why: why.clone(),
                ..Done::plain(
                    Lead::Warning,
                    match what {
                        Failure::Start => "Couldn't start the capture".to_owned(),
                        Failure::Save => "Couldn't save the capture".to_owned(),
                        Failure::Send(to) => format!("Couldn't send to {}", short(to)),
                    },
                )
            }),
            Phase::TakenBack { .. } => Pres::Done(Done {
                why: Some(SEND_LATER.into()),
                ..Done::plain(Lead::Check, "Taken back")
            }),
            Phase::Kept { .. } => Pres::Done(Done {
                why: Some(SEND_LATER.into()),
                ..Done::plain(Lead::Check, "Kept")
            }),
            Phase::TooLate { to, .. } => Pres::Done(Done {
                why: Some("too late to take back".into()),
                hand_off: true,
                ..Done::plain(Lead::Check, sent_to(to))
            }),
            Phase::Queued { why, .. } => Pres::Done(Done {
                why: Some(why.clone()),
                hand_off: true,
                ..Done::plain(Lead::Check, "Copied")
            }),
            // A pick went at once: the check, and a plain Undo (no ring,
            // no drain) while it can still be taken back.
            Phase::Sent(sent) => Pres::Done(Done {
                why: sent.rule_saved.then(|| "rule saved".into()),
                undo: sent.undoable && undo_left(sent.at),
                hand_off: true,
                ..Done::plain(Lead::Check, sent_to(&sent.to))
            }),
            Phase::Routed(routed) => Pres::Done(routed_done(routed, undo_left(routed.at))),
        }
    }
}

/// A closed list that still waits for a pick: a guess, or one offered
/// again after an Undo; a rule's was sent, as was one too late to take
/// back, and an unreachable one lists nothing.
fn waits(routed: Routed) -> Option<Routed> {
    let (suggested, recent) = sections(&routed.route);
    (!matches!(routed.route, Route::Rule { .. })
        && routed.already_with.is_none()
        && !(suggested.is_empty() && recent.is_empty()))
    .then_some(Routed {
        open: None,
        ..routed
    })
}

fn hold_until(phase: &Phase) -> Option<f64> {
    match phase {
        Phase::Saved { at } => Some(at + tuning::get().done_hold_s),
        Phase::Queued { at, .. } | Phase::TooLate { at, .. } => {
            Some(at + tuning::get().brief_hold_s)
        }
        // A send leaves once its Undo window ends (a send with none, and
        // a queued one, after the brief hold).
        Phase::Sent(sent) if sent.undoable => Some(sent.at + undo_s()),
        Phase::Sent(sent) => Some(sent.at + tuning::get().brief_hold_s),
        // The suggestions leave like any finished capsule, and so does a
        // take-back once folded (from when it folded); a search, or a
        // take-back's open list, stays.
        Phase::Routed(Routed {
            route: Route::Rule { .. },
            at,
            open: None,
            taken_back: false,
            ..
        }) => Some(at + undo_s()),
        // Too late to take back, folded: it's where it went, so it leaves
        // as the other settled sends do.
        Phase::Routed(Routed {
            at,
            open: None,
            already_with: Some(_),
            ..
        }) => Some(at + tuning::get().brief_hold_s),
        Phase::Routed(Routed {
            at,
            open,
            taken_back,
            ..
        }) if open.as_ref().is_none_or(|open| !open.typing && !taken_back) => {
            Some(at + tuning::get().done_hold_s)
        }
        Phase::Kept { at } | Phase::TakenBack { at } | Phase::Failed { at, .. } => {
            Some(at + tuning::get().brief_hold_s)
        }
        _ => None,
    }
}

fn sections(route: &Route) -> (&[Dest], &[Dest]) {
    match route {
        Route::Rule {
            suggested, recent, ..
        }
        | Route::Guess {
            suggested, recent, ..
        } => (suggested, recent),
        Route::Nothing { recent } | Route::NoAnswer { recent } => (&[], recent),
        Route::Unreachable { .. } => (&[], &[]),
    }
}

/// The list for `route`, filtered by `query`.
fn list(route: &Route, open: &Open) -> List {
    let query = open.query.to_lowercase();
    let keep = |rows: &[Dest], max: usize| -> Vec<Dest> {
        rows.iter()
            .filter(|dest| dest.name.to_lowercase().contains(&query))
            .take(max)
            .cloned()
            .collect()
    };
    let (suggested, recent) = sections(route);
    if !open.typing {
        let site = match route {
            Route::Guess { site, .. } => site.clone(),
            _ => None,
        };
        return List {
            suggested: suggested
                .iter()
                .chain(recent)
                .take(SUGGEST_ROWS)
                .cloned()
                .collect(),
            recent: Vec::new(),
            selected: open.selected,
            footer: Footer::Suggesting,
            always: site.map(|site| (site, open.always)),
        };
    }
    let footer = match route {
        Route::Rule { to, .. } => Footer::Keep(to.name.clone()),
        Route::Guess {
            site: Some(site), ..
        } => Footer::Always(site.clone()),
        _ => Footer::KeepCopied,
    };
    List {
        suggested: keep(suggested, MAX_SUGGESTED),
        recent: keep(recent, MAX_RECENT),
        selected: open.selected,
        footer,
        always: None,
    }
}

fn clamp(routed: &mut Routed) {
    let Some(open) = &routed.open else { return };
    let count = list(&routed.route, open).rows().count();
    if let Some(open) = &mut routed.open {
        open.selected = open.selected.min(count.saturating_sub(1));
    }
}

fn send(
    routed: Routed,
    row: Option<usize>,
    always: bool,
    now: f64,
    actions: &mut Vec<Action>,
) -> Phase {
    let Some(open) = &routed.open else {
        return Phase::Routed(routed);
    };
    let list = list(&routed.route, open);
    let Some(dest) = row.and_then(|row| list.rows().nth(row)) else {
        return Phase::Routed(routed);
    };
    // The rule's own session already has it: the list just closes.
    if !(always || open.always)
        && rule_to(&routed.route).is_some_and(|to| to.session == dest.session)
    {
        return Phase::Routed(Routed {
            open: None,
            ..routed
        });
    }
    let site = match (&routed.route, always || open.always) {
        (Route::Guess { site, .. } | Route::Rule { site, .. }, true) => site.clone(),
        _ => None,
    };
    actions.push(Action::Send {
        to: dest.session.clone(),
        always: site.clone(),
    });
    Phase::Sent(Sent::picked(
        dest,
        site.is_some(),
        now,
        Some(routed.route.clone()),
    ))
}

/// Where a rule sent it.
fn rule_to(route: &Route) -> Option<&Dest> {
    match route {
        Route::Rule { to, .. } => Some(to),
        _ => None,
    }
}

/// The open list again after an Undo of the send to `from`: the same
/// sessions as a guess, `from` once, last of all. Taken back, nothing is
/// sent; too late, `from` keeps it. `None` when there's nothing to list.
fn offered_again(route: Route, from: &Dest, too_late: bool, now: f64) -> Option<Routed> {
    let (mut suggested, mut recent, site) = match route {
        Route::Rule {
            suggested,
            recent,
            site,
            ..
        }
        | Route::Guess {
            suggested,
            recent,
            site,
        } => (suggested, recent, site),
        Route::Nothing { recent } | Route::NoAnswer { recent } => (Vec::new(), recent, None),
        Route::Unreachable { .. } => return None,
    };
    // A rule's session leads its suggestions and may be among the recent
    // ones too: it is listed once, at the end of the whole list.
    let mut last = None;
    for rows in [&mut suggested, &mut recent] {
        rows.retain(|dest| {
            let taken = dest.session == from.session;
            if taken && last.is_none() {
                last = Some(dest.clone());
            }
            !taken
        });
    }
    if let Some(dest) = last {
        let rows = if recent.is_empty() {
            &mut suggested
        } else {
            &mut recent
        };
        rows.push(Dest {
            why: if too_late { "has it" } else { "taken back" }.into(),
            ..dest
        });
    }
    if suggested.is_empty() && recent.is_empty() {
        return None;
    }
    Some(Routed {
        route: Route::Guess {
            suggested,
            recent,
            site,
        },
        at: now,
        open: Some(Open::suggesting()),
        taken_back: true,
        already_with: too_late.then(|| from.name.clone()),
        taken_from: Some(from.clone()),
        delivered: false,
    })
}

/// What the capsule shows the moment an Undo of the send to `from` is
/// asked for: the list again (or "Taken back" with nothing to list), never
/// gone, whatever the router answers.
fn taking_back(route: Option<Route>, from: &Dest, now: f64) -> Phase {
    route
        .and_then(|route| offered_again(route, from, false, now))
        .map_or(Phase::TakenBack { at: now }, Phase::Routed)
}

fn sent_to(to: &str) -> String {
    format!("Sent to {}", short(to))
}

/// A session's name cut to [`ROUTE_NAME_MAX`] characters, so the
/// capsule's widest state stays inside its panel.
fn short(name: &str) -> String {
    if name.chars().count() <= ROUTE_NAME_MAX {
        return name.to_owned();
    }
    let cut: String = name.chars().take(ROUTE_NAME_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

fn routed_done(routed: &Routed, undo: bool) -> Done {
    let base = match &routed.route {
        // Sent nowhere, it waits for a pick: ⌥⇧V or a click reopens the list.
        _ if routed.taken_back => Done {
            // Too late, it's where it went: nothing to offer.
            ask: routed.already_with.is_none().then(|| "Send to…".into()),
            why: routed
                .already_with
                .as_ref()
                .map(|_| "too late to take back".into()),
            ..Done::plain(
                Lead::Check,
                match &routed.already_with {
                    Some(to) => sent_to(to),
                    None => "Taken back".into(),
                },
            )
        },
        // Sent at once, with a pick's plain Undo while it can be taken
        // back; ⌥⇧V opens the list throughout.
        Route::Rule { to, .. } => Done {
            why: Some("rule".into()),
            undo,
            ..Done::plain(Lead::Check, sent_to(&to.name))
        },
        // The suggestions are the ask (closed, the list shortcut opens them).
        Route::Guess { .. } => Done {
            ask: Some("Send to…".into()),
            ..Done::plain(Lead::Check, "Copied")
        },
        Route::Nothing { .. } => Done {
            ask: Some("Send to…".into()),
            ..Done::plain(Lead::Check, "Copied")
        },
        Route::NoAnswer { .. } => Done {
            why: Some("the router didn't answer".into()),
            ask: Some("Send to…".into()),
            ..Done::plain(Lead::Check, "Copied")
        },
        // Still copied, so Paste and Inspect stay; there's nothing to list.
        Route::Unreachable { why } => Done {
            why: Some(short_reason(why)),
            ..Done::plain(Lead::Warning, "Couldn't reach OpenCode")
        },
    };
    match &routed.open {
        // The suggestions hang under a finished capsule that keeps Paste
        // and Inspect, and its note.
        Some(open) if !open.typing => Done {
            routes: Vec::new(),
            ask: None,
            open: Some((String::new(), list(&routed.route, open))),
            hand_off: true,
            ..base
        },
        // Searching, the field takes the chips' place.
        Some(open) => Done {
            why: None,
            undo: false,
            routes: Vec::new(),
            ask: None,
            open: Some((open.query.clone(), list(&routed.route, open))),
            ..base
        },
        None => Done {
            hand_off: true,
            ask_keys: if base.ask.is_some() {
                super::surface::ask_keys()
            } else {
                Vec::new()
            },
            ..base
        },
    }
}

/// A host error (`{error:#}`, outer context first) as the capsule's reason:
/// the root cause on its first line, kept to what fits beside the message.
fn short_reason(error: &str) -> String {
    const MAX: usize = 32;
    let line = error.lines().next().unwrap_or_default().trim();
    let cause = line.rsplit(": ").next().unwrap_or(line).trim();
    if cause.chars().count() <= MAX {
        return cause.to_owned();
    }
    let cut: String = cause.chars().take(MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess() -> Route {
        Route::Guess {
            suggested: vec![
                Dest::new("SEER", "guess · problems.kitlangton.com"),
                Dest::new("HEX", "last sent"),
            ],
            recent: vec![Dest::new("HOME", "yesterday")],
            site: Some("problems.kitlangton.com".into()),
        }
    }

    /// Shaped like the router's answer: a saved rule names no runner-up,
    /// so it suggests only its own session, and the recent ones leave it
    /// out.
    fn rule() -> Route {
        let to = Dest::new("SEER", "sent · rule");
        Route::Rule {
            to: to.clone(),
            suggested: vec![to],
            recent: vec![Dest::new("HOME", "yesterday")],
            site: Some("problems.kitlangton.com".into()),
        }
    }

    fn done(flow: &Flow, now: f64) -> Done {
        match flow.pres(now) {
            Pres::Done(done) => done,
            other => panic!("not done: {other:?}"),
        }
    }

    /// A send OpenCode didn't take says why, not that the session was
    /// unreachable ("couldn't reach SEER" read as "cannot find session").
    #[test]
    fn a_queued_send_says_why_it_didnt_go() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        flow.input(
            Input::Delivered(Delivered::Queued {
                to: "SEER".into(),
                why: "OpenCode can't see this Mac's files".into(),
            }),
            0.5,
        );
        let shown = done(&flow, 0.5);
        assert_eq!(shown.text, "Copied");
        assert_eq!(
            shown.why.as_deref(),
            Some("OpenCode can't see this Mac's files")
        );
    }

    #[test]
    fn paste_takes_back_a_send_still_undoable_and_the_capsule_stays_gone() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        assert!(done(&flow, 1.0).hand_off);
        assert_eq!(
            flow.click(Id::Paste, 1.0),
            [Action::Undo { to: "SEER".into() }, Action::Paste]
        );
        assert_eq!(flow.pres(1.0), Pres::Hidden);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        assert_eq!(flow.pres(1.2), Pres::Hidden, "the take-back says nothing");

        // Once it's gone, Paste only pastes.
        flow.input(Input::Routed(rule()), 2.0);
        assert_eq!(flow.click(Id::Paste, 2.5 + undo_s()), [Action::Paste]);

        // Inspect never takes a send back.
        flow.input(Input::Routed(rule()), 10.0);
        assert_eq!(flow.click(Id::Inspect, 10.5), [Action::Inspect]);
        assert_eq!(flow.pres(10.5), Pres::Hidden);

        flow.input(Input::Stage(Stage::Done), 20.0);
        assert!(done(&flow, 20.0).hand_off);
        flow.input(Input::Failed(Failure::Save, None), 21.0);
        assert!(!done(&flow, 21.0).hand_off, "nothing was written");
    }

    #[test]
    fn a_list_shortcut_change_changes_the_ask_chip() {
        use crate::app_settings::{HotkeyBinding, HotkeyModifiers, publish_list_keycaps};
        let mut flow = Flow::new();
        flow.input(Input::Routed(Route::Nothing { recent: Vec::new() }), 0.0);
        let before = done(&flow, 0.5);
        assert_eq!(before.ask_keys, ["⌥", "⇧", "V"]);
        let rebound = HotkeyBinding {
            modifiers: HotkeyModifiers::option_command(),
            ..HotkeyBinding::capture_list_default()
        };
        publish_list_keycaps(Some(&rebound));
        let after = done(&flow, 0.5);
        assert_eq!(after.ask_keys, rebound.keycaps());
        assert_ne!(before, after, "so the surface lays the chip out again");
        publish_list_keycaps(None);
        assert!(done(&flow, 0.5).ask_keys.is_empty());
        // The keycaps are the process's: later tests and stills expect ⌥⇧V.
        publish_list_keycaps(Some(&HotkeyBinding::capture_list_default()));
    }

    #[test]
    fn the_capsule_takes_only_the_keys_its_state_needs() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        assert_eq!(flow.keys(), None);
        flow.input(Input::Chips, 0.5);
        flow.click(super::super::surface::Id::Mode(Mode::Window), 1.0);
        assert_eq!(flow.keys(), Some(CapsuleKeys::Picker));

        flow.input(Input::Routed(rule()), 2.0);
        assert_eq!(flow.keys(), Some(CapsuleKeys::Route));
        flow.input(Input::OpenList, 9.0);
        assert_eq!(flow.keys(), Some(CapsuleKeys::List));
        flow.input(Input::Type("ho".into()), 9.1);
        flow.input(Input::Type("x".into()), 9.2);
        flow.input(Input::Backspace, 9.3);
        assert_eq!(done(&flow, 9.3).open.unwrap().0, "ho");
        flow.input(Input::SendRow(0), 9.4);
        assert_eq!(flow.keys(), None, "a send takes no keys: Undo is a click");

        flow.input(Input::Stage(Stage::Done), 20.0);
        assert_eq!(flow.keys(), None);
    }

    #[test]
    fn a_recording_that_leaves_unseen_hides_the_capsule_but_not_the_picker() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Gone, 1.0);
        assert_eq!(flow.pres(1.0), Pres::Hidden);

        flow.input(Input::Stage(Stage::Recording), 2.0);
        flow.input(Input::Chips, 2.1);
        flow.click(super::super::surface::Id::Mode(Mode::Area), 2.5);
        flow.input(Input::Gone, 2.6);
        assert!(matches!(flow.pres(2.6), Pres::Selecting { .. }));
    }

    #[test]
    fn stages_walk_the_capsule_through_a_capture() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        assert_eq!(
            flow.pres(0.0),
            Pres::Recording {
                chips: false,
                phone: false,
                discarding: false,
            }
        );
        flow.input(Input::Chips, 1.0);
        assert_eq!(
            flow.pres(1.0),
            Pres::Recording {
                chips: true,
                phone: false,
                discarding: false,
            }
        );
        assert!(flow.tick(1.0 + tuning::get().chips_open_s));
        assert_eq!(
            flow.pres(5.0),
            Pres::Recording {
                chips: false,
                phone: false,
                discarding: false,
            }
        );
        for stage in [Stage::Stopping, Stage::Transcribing, Stage::Writing] {
            flow.input(Input::Stage(stage), 6.0);
            assert_eq!(flow.pres(6.0), Pres::Processing);
        }
        flow.input(Input::Stage(Stage::Done), 7.0);
        assert_eq!(done(&flow, 7.0).text, "Copied");
        assert!(flow.tick(7.0 + tuning::get().done_hold_s));
        assert_eq!(flow.pres(40.0), Pres::Hidden);
    }

    #[test]
    fn chips_fold_once_seen_when_work_starts_but_never_under_the_pointer() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Chips, 0.0);
        flow.input(Input::Worked, 0.2);
        assert!(!flow.tick(0.5), "seen for CHIPS_MIN_S first");
        assert!(flow.tick(CHIPS_MIN_S));
        assert_eq!(
            flow.pres(1.0),
            Pres::Recording {
                chips: false,
                phone: false,
                discarding: false,
            }
        );

        flow.input(Input::Chips, 2.0);
        flow.hover(true, 3.0);
        assert_eq!(flow.next_due(5.0), None);
        assert!(!flow.tick(2.0 + tuning::get().chips_open_s + 1.0));
        assert_eq!(
            flow.pres(7.0),
            Pres::Recording {
                chips: true,
                phone: false,
                discarding: false,
            }
        );
        flow.hover(false, 7.0);
        assert!(!flow.tick(7.0 + CHIPS_LINGER_S - 0.1));
        assert!(flow.tick(7.0 + CHIPS_LINGER_S));
    }

    #[test]
    fn the_shortcut_records_the_pickers_screen_and_a_chip_discards_the_recording() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Chips, 0.0);
        flow.click(Id::Mode(Mode::Window), 0.2);
        flow.click(Id::Mode(Mode::Screen), 0.3);
        assert_eq!(
            flow.input(Input::Record, 0.5),
            [Action::Record(Mode::Screen)]
        );
        flow.click(Id::Mode(Mode::Window), 0.6);
        assert_eq!(
            flow.input(Input::Record, 0.7),
            [Action::Record(Mode::Window)]
        );
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Chips, 0.0);
        assert_eq!(
            flow.click(super::super::surface::Id::Mode(Mode::Window), 0.5),
            [Action::Discard]
        );
        assert_eq!(
            flow.pres(0.5),
            Pres::Selecting {
                mode: Mode::Window,
                phone: false
            }
        );
    }

    #[test]
    fn a_rule_sends_at_once_with_a_plain_undo_and_leaves_when_its_window_ends() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        let at = |t| done(&flow, t).undo;
        assert!(at(10.0) && at(11.2) && at(14.5));
        assert!(!at(15.0));
        assert_eq!(done(&flow, 10.0).text, "Sent to SEER");
        assert_eq!(done(&flow, 10.0).lead, Lead::Check);
        assert_eq!(done(&flow, 10.0).ask, None, "Undo, as a pick has");
        assert_eq!(done(&flow, 15.0).ask, None, "nothing after the window");
        assert!(done(&flow, 10.0).routes.is_empty(), "no runner-up");
        assert_eq!(done(&flow, 10.0).why.as_deref(), Some("rule"));
        let actions = flow.input(Input::Undo, 11.0);
        assert_eq!(actions, vec![Action::Undo { to: "SEER".into() }]);
        flow.input(Input::Undone(Undone::TakenBack), 11.4);
        assert!(
            done(&flow, 11.4).open.is_some(),
            "taken back, the list opens"
        );
        // Escape never takes a send back or closes it, in the window or
        // after it: it was never Hex's.
        for at in [1.0, 5.5] {
            let mut late = Flow::new();
            late.input(Input::Routed(rule()), 0.0);
            assert!(late.input(Input::Escape, at).is_empty());
            assert!(matches!(late.pres(at), Pres::Done(_)));
        }
        assert_eq!(flow_keys_after_send(), None);
        // Time alone takes it away once the Undo window ends.
        let mut late = Flow::new();
        late.input(Input::Routed(rule()), 0.0);
        assert!(!late.tick(undo_s() - 0.1));
        assert_eq!(late.next_due(0.0), Some(undo_s()));
        assert!(late.tick(undo_s()));
        assert_eq!(late.pres(undo_s()), Pres::Hidden);
    }

    /// The pointer on a finished capsule holds it; leaving gives it a moment.
    /// A click on Undo at the window's edge still counts, and folding a list
    /// gives a brief hold rather than hiding it at once.
    #[test]
    fn hover_and_folding_hold_a_finished_capsule_and_a_late_undo_counts() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        flow.hover(true, 4.0);
        assert!(!flow.tick(undo_s() + 10.0));
        assert_eq!(flow.next_due(undo_s() + 10.0), None);
        flow.hover(false, 20.0);
        assert!(!flow.tick(20.0 + CHIPS_LINGER_S - 0.1));
        assert!(flow.tick(20.0 + CHIPS_LINGER_S));

        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        assert_eq!(
            flow.input(Input::Undo, undo_s() + 0.01),
            [Action::Undo { to: "SEER".into() }]
        );

        let brief = tuning::get().brief_hold_s;
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        flow.input(Input::OpenList, 4.0);
        flow.input(Input::Escape, 9.0);
        assert!(!flow.tick(9.0 + brief - 0.1));
        assert!(flow.tick(9.0 + brief));
    }

    fn flow_keys_after_send() -> Option<crate::capture_host::CapsuleKeys> {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::SendRow(0), 1.0);
        flow.keys()
    }

    /// A pick leaves on its own when its Undo window ends, and a send
    /// that queued or came too late leaves after the brief hold.
    #[test]
    fn a_sent_capsule_hides_by_itself() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::SendRow(0), 1.0);
        assert!(matches!(
            flow.pres(1.0),
            Pres::Done(Done { undo: true, .. })
        ));
        assert_eq!(flow.next_due(1.0), Some(1.0 + undo_s()));
        assert!(!flow.tick(1.0 + undo_s() - 0.1));
        assert!(flow.tick(1.0 + undo_s()));
        assert_eq!(flow.pres(1.0 + undo_s()), Pres::Hidden);

        let brief = tuning::get().brief_hold_s;
        let mut queued = Flow::new();
        queued.input(
            Input::Delivered(Delivered::Queued {
                to: "SEER".into(),
                why: "will retry".into(),
            }),
            0.0,
        );
        assert!(queued.tick(brief));
        assert_eq!(queued.pres(brief), Pres::Hidden);

        let mut late = Flow::new();
        late.input(Input::Routed(rule_of_three()), 0.0);
        late.input(Input::Undo, 1.0);
        late.input(Input::Undone(Undone::TooLate), 1.2);
        late.input(Input::ClickAway, 2.0);
        assert!(late.tick(2.0 + brief));
        assert_eq!(late.pres(2.0 + brief), Pres::Hidden);
    }

    fn rule_of_three() -> Route {
        let to = Dest::new("SEER", "rule").session("ses_seer");
        Route::Rule {
            to: to.clone(),
            suggested: vec![to],
            recent: vec![
                Dest::new("HEX", "last sent").session("ses_hex"),
                Dest::new("HOME", "recent").session("ses_home"),
            ],
            site: Some("problems.kitlangton.com".into()),
        }
    }

    fn rows(flow: &Flow, now: f64) -> Vec<String> {
        let (_, list) = done(flow, now).open.expect("the list is open");
        list.rows().map(|dest| dest.name.clone()).collect()
    }

    /// Kit's bug: an auto-send to the wrong session, taken back, must never
    /// lose the capture. However the Undo is asked for, the suggestions
    /// come back at once, before the router answers, and stay until a pick.
    #[test]
    fn rule_send_then_undo_shows_the_list_and_pick_2_sends_to_2() {
        use super::super::surface::Id;
        use crate::capture_host::CapsuleKeys;
        type Undo = fn(&mut Flow, f64) -> Vec<Action>;
        let ways: [(&str, Undo); 2] = [
            ("Undo input", |flow, now| flow.input(Input::Undo, now)),
            ("Undo chip", |flow, now| flow.click(Id::Undo, now)),
        ];
        for (how, undo) in ways {
            let mut flow = Flow::new();
            flow.input(Input::Routed(rule_of_three()), 0.0);
            assert_eq!(
                undo(&mut flow, 1.0),
                [Action::Undo { to: "SEER".into() }],
                "{how}"
            );
            // At once, the same suggestions, the wrong one last.
            assert_eq!(rows(&flow, 1.0), ["HEX", "HOME", "SEER"], "{how}");
            // 1-3 pick, the list shortcut searches; Escape stays the app's.
            assert_eq!(flow.keys(), Some(CapsuleKeys::Suggest { rows: 3 }), "{how}");
            // The router's answer changes nothing, and nothing times out.
            flow.input(Input::Undone(Undone::TakenBack), 1.3);
            assert_eq!(rows(&flow, 1.3), ["HEX", "HOME", "SEER"], "{how}");
            assert!(flow.next_due(1.3).is_none(), "{how}");
            assert!(!flow.tick(1_000.0), "{how}");
            // Pick 2: sent to #2.
            assert_eq!(
                flow.input(Input::SendRow(1), 1_000.0),
                [Action::Send {
                    to: "ses_home".into(),
                    always: None
                }],
                "{how}"
            );
            assert_eq!(done(&flow, 1_000.0).text, "Sent to HOME", "{how}");
        }

        // Even with nothing else to offer, it stays until dismissed, and
        // dismissing says where the bundle is.
        let mut flow = Flow::new();
        let lone = Route::Rule {
            to: Dest::new("SEER", "rule"),
            suggested: Vec::new(),
            recent: Vec::new(),
            site: None,
        };
        flow.input(Input::Routed(lone), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        assert_eq!(done(&flow, 1.2).text, "Taken back");
        // Nothing to list: no keys are Hex's, and it leaves by itself.
        assert_eq!(flow.keys(), None);
        let brief = tuning::get().brief_hold_s;
        assert!(!flow.tick(1.0 + brief - 0.1));
        flow.input(Input::Dismiss, 3.0);
        let kept = done(&flow, 3.0);
        assert_eq!(
            (kept.text.as_str(), kept.why.as_deref()),
            ("Kept", Some(SEND_LATER))
        );
    }

    #[test]
    fn undoing_a_rule_send_reopens_the_list_until_a_pick() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        assert_eq!(
            flow.input(Input::Undo, 1.0),
            [Action::Undo { to: "SEER".into() }]
        );
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        // The same sessions, the wrongly picked one last; nothing sent.
        assert_eq!(rows(&flow, 1.2), ["HEX", "HOME", "SEER"]);
        let (_, list) = done(&flow, 1.2).open.unwrap();
        assert_eq!(list.suggested[2].why, "taken back");
        assert_eq!(list.selected, 0);
        assert_eq!(
            flow.keys(),
            Some(crate::capture_host::CapsuleKeys::Suggest { rows: 3 })
        );
        // It never leaves by itself.
        assert!(flow.next_due(1.2).is_none());
        assert!(!flow.tick(1_000.0));
        assert!(done(&flow, 1_000.0).open.is_some());
        // Pick 2: sent to #2.
        assert_eq!(
            flow.input(Input::SendRow(1), 1_000.0),
            [Action::Send {
                to: "ses_home".into(),
                always: None
            }]
        );
        assert_eq!(done(&flow, 1_000.0).text, "Sent to HOME");

        // Clicking away folds the list and gives Escape back to the app; the
        // capsule leaves like any other, and the list shortcut still brings
        // it back. Escape (or ✕) is the dismissal, and the bundle is kept.
        let hold = tuning::get().done_hold_s;
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        flow.input(Input::ClickAway, 2.0);
        let closed = done(&flow, 2.0);
        assert_eq!(closed.text, "Taken back");
        assert_eq!(closed.ask.as_deref(), Some("Send to…"));
        let waiting = Some(crate::capture_host::CapsuleKeys::Route);
        assert_eq!(flow.keys(), waiting);
        assert_eq!(flow.next_due(2.0), Some(2.0 + hold));
        assert!(!flow.tick(2.0 + hold - 0.1));
        assert!(flow.tick(2.0 + hold));
        assert_eq!(flow.pres(1_000.0), Pres::Hidden);
        assert_eq!(flow.keys(), waiting);
        flow.input(Input::OpenList, 1_000.0);
        assert_eq!(rows(&flow, 1_000.0)[0], "HEX");
        assert!(flow.input(Input::Escape, 1_001.0).is_empty());
        let kept = done(&flow, 1_001.0);
        assert_eq!(
            (kept.text.as_str(), kept.why.as_deref()),
            ("Kept", Some(SEND_LATER))
        );
        assert!(flow.tick(1_001.0 + tuning::get().brief_hold_s));

        // Taken back, ✕ is the dismissal too.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        assert!(flow.click(Id::Close, 2.0).is_empty());
        assert_eq!(done(&flow, 2.0).text, "Kept");
        // Beside "Sent to X", a rule's or a pick's, ✕ just closes: the send
        // already went.
        for route in [rule_of_three(), guess()] {
            let mut flow = Flow::new();
            let routed = matches!(route, Route::Rule { .. });
            flow.input(Input::Routed(route), 0.0);
            if !routed {
                flow.input(Input::SendRow(0), 0.5);
            }
            assert!(flow.click(Id::Close, 1.0).is_empty());
            assert_eq!(flow.pres(1.0), Pres::Hidden);
        }
    }

    #[test]
    fn too_late_to_take_back_offers_the_list_for_a_copy() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TooLate), 1.2);
        let shown = done(&flow, 1.2);
        assert_eq!(shown.text, "Sent to SEER");
        assert_eq!(shown.why.as_deref(), Some("too late to take back"));
        assert_eq!(rows(&flow, 1.2), ["HEX", "HOME", "SEER"]);
        let (_, list) = shown.open.unwrap();
        assert_eq!(list.suggested[2].why, "has it");
        assert!(!flow.tick(1_000.0), "it waits for a pick");
        // Pick 2: a copy goes to #2.
        assert_eq!(
            flow.input(Input::SendRow(1), 1_000.0),
            [Action::Send {
                to: "ses_home".into(),
                always: None
            }]
        );
        assert_eq!(done(&flow, 1_000.0).text, "Sent to HOME");

        // Folded, it says where it is; dismissed, it just goes.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TooLate), 1.2);
        flow.input(Input::ClickAway, 2.0);
        let folded = done(&flow, 2.0);
        assert_eq!(folded.text, "Sent to SEER");
        assert_eq!(folded.why.as_deref(), Some("too late to take back"));
        assert_eq!(folded.ask, None, "it is where it went");
        flow.input(Input::Dismiss, 3.0);
        assert_eq!(flow.pres(3.0), Pres::Hidden);
        // It's where it went: once gone, nothing waits for a pick.
        assert_eq!(flow.keys(), None);

        // Folded, it leaves by itself, and the list shortcut stays the app's.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TooLate), 1.2);
        flow.input(Input::ClickAway, 2.0);
        assert!(flow.tick(2.0 + tuning::get().done_hold_s));
        assert_eq!(flow.keys(), None);
    }

    #[test]
    fn undoing_a_pick_reopens_the_list_it_came_from() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::OpenList, 0.5);
        flow.input(Input::SendRow(0), 1.0);
        assert_eq!(
            flow.input(Input::Undo, 2.0),
            [Action::Undo { to: "SEER".into() }]
        );
        flow.input(Input::Undone(Undone::TakenBack), 2.2);
        assert_eq!(rows(&flow, 2.2), ["HEX", "HOME", "SEER"], "last of all");
        let (_, list) = done(&flow, 2.2).open.unwrap();
        assert_eq!(list.always, Some(("problems.kitlangton.com".into(), false)));
    }

    /// The session taken back goes once to the end of the whole list,
    /// matched by id: a twin sharing its title keeps its place.
    #[test]
    fn a_take_back_moves_that_session_last_by_id() {
        let to = Dest::new("SEER", "rule").session("ses_seer");
        let route = Route::Rule {
            to: to.clone(),
            suggested: vec![to.clone()],
            recent: vec![
                Dest::new("SEER", "2 min ago").session("ses_twin"),
                to.clone(),
                Dest::new("HEX", "last sent").session("ses_hex"),
            ],
            site: None,
        };
        let mut flow = Flow::new();
        flow.input(Input::Routed(route), 0.0);
        flow.input(Input::Undo, 1.0);
        let Phase::Routed(Routed {
            route: Route::Guess {
                suggested, recent, ..
            },
            ..
        }) = &flow.phase
        else {
            panic!("the list again: {:?}", flow.phase);
        };
        let sessions: Vec<&str> = suggested
            .iter()
            .chain(recent)
            .map(|dest| dest.session.as_str())
            .collect();
        assert_eq!(sessions, ["ses_twin", "ses_hex", "ses_seer"]);
        assert_eq!(recent.last().unwrap().why, "taken back");
    }

    /// A pick taken back is sent nowhere again, even though the router
    /// said it went: dismissed, the list shortcut brings it back.
    #[test]
    fn a_pick_taken_back_waits_for_a_pick_again() {
        use crate::capture_host::CapsuleKeys;
        for dismiss in [Input::Dismiss, Input::ClickAway] {
            let mut flow = Flow::new();
            flow.input(Input::Routed(guess()), 0.0);
            flow.input(Input::SendRow(0), 1.0);
            flow.input(Input::Settled, 1.1);
            flow.input(Input::Undo, 2.0);
            // The send it took back, reported late, settles nothing.
            flow.input(Input::Settled, 2.1);
            flow.input(Input::Undone(Undone::TakenBack), 2.2);
            flow.input(dismiss.clone(), 3.0);
            flow.tick(3.0 + tuning::get().done_hold_s);
            assert_eq!(flow.pres(100.0), Pres::Hidden, "{dismiss:?}");
            assert_eq!(flow.keys(), Some(CapsuleKeys::Route), "{dismiss:?}");
            flow.input(Input::OpenList, 100.0);
            assert!(done(&flow, 100.0).open.is_some(), "{dismiss:?}");
        }
    }

    /// The list shortcut bringing a taken-back capture back is silent: the
    /// take-back sounded once.
    #[test]
    fn reopening_a_taken_back_list_sounds_nothing() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::SendRow(0), 1.0);
        flow.input(Input::Undo, 2.0);
        assert_eq!(flow.take_cues(), [Cue::Sent, Cue::TakenBack]);
        flow.input(Input::Undone(Undone::TakenBack), 2.2);
        // Kept, then back.
        flow.input(Input::Dismiss, 3.0);
        flow.input(Input::OpenList, 4.0);
        assert!(done(&flow, 4.0).open.is_some());
        // Hidden, then back.
        flow.input(Input::Dismiss, 5.0);
        flow.tick(5.0 + tuning::get().brief_hold_s);
        flow.input(Input::OpenList, 20.0);
        assert!(done(&flow, 20.0).open.is_some());
        assert!(flow.take_cues().is_empty());
    }

    /// A queued send that went on a retry can't be taken back by Hex: it
    /// says so plainly, with no Undo and no Escape.
    #[test]
    fn a_retried_send_has_no_undo() {
        let mut flow = Flow::new();
        flow.input(Input::Delivered(Delivered::Sent { to: "SEER".into() }), 0.0);
        let shown = done(&flow, 0.5);
        assert_eq!(shown.text, "Sent to SEER");
        assert!(!shown.undo);
        assert_eq!(flow.keys(), None);
        assert!(flow.input(Input::Escape, 0.5).is_empty());
        assert_eq!(
            flow.click(super::super::surface::Id::Paste, 0.6),
            [Action::Paste]
        );
    }

    /// The router sends a rule's answer at once: the capsule reads "Sent"
    /// as it shows it, and sounds Sent when the router says it went.
    #[test]
    fn a_rule_reads_sent_at_once_and_sounds_when_it_went() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        let sent = done(&flow, 10.0);
        assert_eq!(sent.text, "Sent to SEER");
        assert_eq!(sent.lead, Lead::Check);
        assert!(flow.take_cues().is_empty(), "not until the router says so");
        flow.input(Input::Settled, 10.2);
        assert_eq!(done(&flow, 10.2), sent, "it reads the same");
        assert_eq!(flow.take_cues(), [Cue::Sent]);
        flow.input(Input::Settled, 10.4);
        assert!(flow.take_cues().is_empty(), "once");
    }

    /// The Undo chip counts nothing down: the capsule reads the same
    /// through the window and wakes once, when it ends, so nothing
    /// interrupts a list folding back into "Sent to X".
    #[test]
    fn the_undo_window_wakes_the_capsule_once_at_its_end() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        assert_eq!(flow.pres(10.5), flow.pres(10.0 + undo_s() - 0.5));
        assert_eq!(flow.next_due(10.0), Some(10.0 + undo_s()));
        assert_eq!(flow.next_due(12.0), Some(10.0 + undo_s()));
        assert_ne!(flow.pres(10.0), flow.pres(10.0 + undo_s()));

        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::SendRow(0), 1.0);
        assert_eq!(flow.pres(1.5), flow.pres(1.0 + undo_s() - 0.5));
        assert_eq!(flow.next_due(1.5), Some(1.0 + undo_s()));
    }

    /// Each rule capture sounds Sent when its send goes, even one shown at
    /// the same moment as the last.
    #[test]
    fn every_rule_send_sounds_sent_once() {
        let mut flow = Flow::new();
        for _ in 0..2 {
            flow.input(Input::Stage(Stage::Recording), 0.0);
            flow.input(Input::Stage(Stage::Done), 1.0);
            flow.input(Input::Routed(rule()), 1.0);
            flow.take_cues();
            flow.input(Input::Settled, 1.2);
            assert_eq!(flow.take_cues(), [Cue::Sent]);
        }
    }

    /// After a rule's send, picking its own session in the list only
    /// closes the list: it's already there, and nothing restarts.
    #[test]
    fn picking_the_rules_own_session_just_closes_the_list() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        flow.input(Input::Settled, 0.1);
        flow.take_cues();
        flow.input(Input::OpenList, 1.0);
        assert_eq!(rows(&flow, 1.0)[0], "SEER");
        assert!(flow.input(Input::SendRow(0), 2.0).is_empty());
        let shown = done(&flow, 2.0);
        assert_eq!(shown.open, None);
        assert_eq!(shown.text, "Sent to SEER");
        assert!(flow.take_cues().is_empty(), "no second Sent");
        assert!(
            !done(&flow, undo_s() + 0.5).undo,
            "the window isn't restarted"
        );

        // Saving it as a rule still goes to the router.
        flow.input(Input::OpenList, 3.5);
        flow.input(Input::Move(-1), 3.5);
        assert_eq!(
            flow.input(Input::Send { always: true }, 4.0),
            [Action::Send {
                to: "SEER".into(),
                always: Some("problems.kitlangton.com".into())
            }]
        );
    }

    /// A pick made while the Undo of a rule's send is unanswered stands:
    /// the router takes the rule's send back first, and that answer (or
    /// its send's late news) never takes the pick back.
    #[test]
    fn a_pick_during_an_undo_stands_when_the_undo_answers() {
        let picked = |flow: &mut Flow| {
            flow.input(Input::Routed(rule_of_three()), 0.0);
            flow.input(Input::Undo, 1.0);
            assert_eq!(rows(flow, 1.0), ["HEX", "HOME", "SEER"]);
            flow.input(Input::SendRow(0), 1.1);
            assert_eq!(done(flow, 1.1).text, "Sent to HEX");
        };
        let mut flow = Flow::new();
        picked(&mut flow);
        flow.input(
            Input::Delivered(Delivered::Queued {
                to: "SEER".into(),
                why: "couldn't reach OpenCode".into(),
            }),
            1.2,
        );
        flow.input(Input::Undone(Undone::TakenBack), 1.3);
        flow.input(Input::Settled, 1.4);
        let shown = done(&flow, 1.4);
        assert_eq!(shown.text, "Sent to HEX");
        assert_eq!(shown.open, None);
        assert!(shown.undo, "the pick can still be taken back");
        assert_eq!(flow.take_cues(), [Cue::TakenBack, Cue::Sent]);

        // The take-back and the pick's sending, read as one change: the
        // pick's news still counts.
        let mut flow = Flow::new();
        picked(&mut flow);
        flow.input(Input::Settled, 1.4);
        assert!(flow.settled && !flow.undoing);
        assert_eq!(done(&flow, 1.4).text, "Sent to HEX");
    }

    /// An Undo asked for while the rule's send is still under way keeps
    /// the list it reopened, however that send ends: queued or failed.
    #[test]
    fn a_send_undone_while_under_way_keeps_the_list_however_it_ends() {
        for ended in [
            Input::Delivered(Delivered::Queued {
                to: "SEER".into(),
                why: "couldn't reach OpenCode".into(),
            }),
            Input::Failed(Failure::Send("SEER".into()), Some("500".into())),
        ] {
            let mut flow = Flow::new();
            flow.input(Input::Routed(rule_of_three()), 0.0);
            flow.input(Input::Undo, 0.5);
            flow.input(ended.clone(), 0.6);
            assert_eq!(rows(&flow, 0.6), ["HEX", "HOME", "SEER"], "{ended:?}");
            flow.input(Input::Undone(Undone::TakenBack), 0.8);
            assert_eq!(rows(&flow, 0.8), ["HEX", "HOME", "SEER"], "{ended:?}");
            assert_eq!(flow.take_cues(), [Cue::TakenBack], "once: {ended:?}");
        }
    }

    #[test]
    fn a_pick_sends_at_once_and_a_rule_has_no_runner_up() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        assert_eq!(
            flow.click(Id::Row(1), 1.0),
            [Action::Send {
                to: "HEX".into(),
                always: None
            }]
        );
        assert_eq!(done(&flow, 1.0).text, "Sent to HEX");
        assert!(done(&flow, 1.0).undo);

        // The router names no runner-up beside a saved rule.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        assert!(done(&flow, 0.0).routes.is_empty());
        assert!(flow.click(Id::Route(0), 1.0).is_empty());
        assert_eq!(done(&flow, 1.0).text, "Sent to SEER");
    }

    /// A switch that can't start settles the recording it discarded as
    /// failed: the capsule says the start failed, not the save.
    #[test]
    fn a_switch_that_cannot_start_says_so() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Phone(true), 0.0);
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Chips, 0.1);
        flow.click(Id::Mode(Mode::Iphone), 0.5);
        flow.input(Input::Failed(Failure::Save, Some("no phone".into())), 0.6);
        assert!(matches!(
            flow.phase,
            Phase::Failed {
                what: Failure::Start,
                ..
            }
        ));

        // Once the phone's recording shows, a later failure is a save's.
        let mut flow = Flow::new();
        flow.input(Input::Phone(true), 0.0);
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Chips, 0.1);
        flow.click(Id::Mode(Mode::Iphone), 0.5);
        flow.input(Input::Stage(Stage::Recording), 0.6);
        flow.input(Input::Failed(Failure::Save, None), 3.0);
        assert!(matches!(
            flow.phase,
            Phase::Failed {
                what: Failure::Save,
                ..
            }
        ));
    }

    #[test]
    fn a_click_away_closes_the_list_and_nothing_else() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::OpenList, 0.5);
        assert_eq!(flow.keys(), Some(CapsuleKeys::List));
        // A press on the capsule is the capsule's own click.
        flow.hover(true, 0.6);
        flow.input(Input::ClickAway, 0.6);
        assert_eq!(flow.keys(), Some(CapsuleKeys::List));
        flow.hover(false, 0.65);
        assert!(flow.input(Input::ClickAway, 0.7).is_empty());
        assert_eq!(flow.keys(), Some(CapsuleKeys::Route));
        assert!(matches!(flow.pres(0.8), Pres::Done(_)));
        flow.input(Input::ClickAway, 0.9);
        assert!(matches!(flow.pres(0.9), Pres::Done(_)), "the chips stay");
    }

    /// Two sessions may share a title: a send names the one picked by id.
    #[test]
    fn sends_name_the_session_by_id() {
        use super::super::surface::Id;
        let twins = || Route::Guess {
            suggested: vec![
                Dest::new("HEX", "guess").session("ses_a"),
                Dest::new("HEX", "last sent").session("ses_b"),
            ],
            recent: Vec::new(),
            site: None,
        };
        let mut flow = Flow::new();
        flow.input(Input::Routed(twins()), 0.0);
        assert_eq!(
            flow.click(Id::Row(1), 1.0),
            [Action::Send {
                to: "ses_b".into(),
                always: None
            }]
        );
        assert_eq!(done(&flow, 1.0).text, "Sent to HEX");

        let mut flow = Flow::new();
        flow.input(Input::Routed(twins()), 0.0);
        flow.input(Input::OpenList, 0.5);
        flow.input(Input::Move(1), 0.6);
        assert_eq!(
            flow.input(Input::Send { always: false }, 0.7),
            [Action::Send {
                to: "ses_b".into(),
                always: None
            }]
        );
    }

    #[test]
    fn suggestions_stop_at_three() {
        let mut flow = Flow::new();
        let suggested = ["SEER", "HEX", "HOME", "A VERY LONG SESSION TITLE", "FIFTH"]
            .map(|name| Dest::new(name, ""))
            .to_vec();
        flow.input(
            Input::Routed(Route::Guess {
                suggested,
                recent: Vec::new(),
                site: None,
            }),
            0.0,
        );
        let (_, list) = done(&flow, 0.0).open.unwrap();
        assert_eq!(list.rows().count(), SUGGEST_ROWS);
        assert_eq!(list.always, None, "no site, no rule to save");
    }

    #[test]
    fn the_screen_chip_folds_and_the_iphone_chip_restarts_on_the_phone() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Chips, 0.0);
        assert_eq!(
            flow.pres(0.1),
            Pres::Recording {
                chips: true,
                phone: false,
                discarding: false,
            }
        );
        assert_eq!(flow.click(Id::Mode(Mode::Screen), 0.5), []);
        assert_eq!(
            flow.pres(0.5),
            Pres::Recording {
                chips: false,
                phone: false,
                discarding: false,
            }
        );

        flow.input(Input::Phone(true), 1.0);
        flow.input(Input::Chips, 1.0);
        assert_eq!(
            flow.pres(1.1),
            Pres::Recording {
                chips: true,
                phone: true,
                discarding: false,
            }
        );
        assert_eq!(
            flow.click(Id::Mode(Mode::Iphone), 1.5),
            [Action::Switch(Mode::Iphone)]
        );
        assert_eq!(
            flow.pres(1.5),
            Pres::Recording {
                chips: false,
                phone: true,
                discarding: false,
            }
        );

        // Unplugged while the picker is on it: back to Screen.
        flow.input(Input::Stage(Stage::Recording), 1.8);
        flow.input(Input::Chips, 1.8);
        flow.click(Id::Mode(Mode::Window), 1.9);
        flow.click(Id::Mode(Mode::Iphone), 2.0);
        flow.input(Input::Phone(false), 2.5);
        assert_eq!(
            flow.pres(2.5),
            Pres::Selecting {
                mode: Mode::Screen,
                phone: false
            }
        );
    }

    #[test]
    fn a_guess_asks_and_the_list_sends_where_its_selected() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        let asked = done(&flow, 0.0);
        assert!(asked.routes.is_empty() && asked.ask.is_none());
        assert!(
            asked.hand_off,
            "Paste and Inspect stay beside the suggestions"
        );
        let (_, suggestions) = asked.open.expect("a guess opens on its suggestions");
        assert!(suggestions.suggesting());
        flow.input(Input::OpenList, 1.0);
        let open = done(&flow, 1.0);
        let (query, list) = open.open.expect("open");
        assert_eq!(query, "");
        assert_eq!(list.selected_dest().unwrap().name, "SEER");
        assert_eq!(
            list.footer,
            Footer::Always("problems.kitlangton.com".into())
        );
        assert!(
            open.ask.is_none() && open.routes.is_empty(),
            "the field takes the chips' place"
        );
        flow.input(Input::Type("ho".into()), 1.5);
        let (_, list) = done(&flow, 1.5).open.unwrap();
        assert_eq!(list.rows().count(), 1);
        let actions = flow.input(Input::Send { always: false }, 2.0);
        assert_eq!(
            actions,
            vec![Action::Send {
                to: "HOME".into(),
                always: None
            }]
        );
        let sent = done(&flow, 2.0);
        assert_eq!(sent.text, "Sent to HOME", "a pick goes at once");
        assert_eq!(sent.lead, Lead::Check);
        assert!(sent.undo, "and can still be taken back");
    }

    #[test]
    fn a_guess_suggests_three_and_a_digit_sends_nothing_before_it() {
        use super::super::surface::Id;
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        let (_, list) = done(&flow, 0.0).open.unwrap();
        let names: Vec<_> = list.rows().map(|dest| dest.name.as_str()).collect();
        assert_eq!(names, ["SEER", "HEX", "HOME"], "suggested, then recent");
        assert_eq!(list.selected, 0, "the best is marked");
        assert_eq!(list.footer, Footer::Suggesting);
        assert_eq!(list.always, Some(("problems.kitlangton.com".into(), false)));
        assert_eq!(flow.keys(), Some(CapsuleKeys::Suggest { rows: 3 }));
        assert!(flow.take_cues().is_empty(), "nothing sent, nothing heard");
        flow.tick(4.0);
        assert!(matches!(flow.pres(4.0), Pres::Done(_)), "no countdown runs");
        flow.click(Id::Always, 4.5);
        assert!(done(&flow, 4.5).open.unwrap().1.always.unwrap().1);
        assert_eq!(
            flow.input(Input::SendRow(1), 5.0),
            [Action::Send {
                to: "HEX".into(),
                always: Some("problems.kitlangton.com".into())
            }]
        );
        assert_eq!(flow.take_cues(), [Cue::Sent], "a pick sounds as it goes");
        assert_eq!(done(&flow, 5.0).why.as_deref(), Some("rule saved"));
    }

    #[test]
    fn a_click_away_folds_the_suggestions_and_they_leave_like_any_capsule() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        // Escape is never Hex's here: it changes nothing.
        assert!(flow.input(Input::Escape, 0.5).is_empty());
        assert!(done(&flow, 0.5).open.is_some());
        flow.input(Input::ClickAway, 1.0);
        let folded = done(&flow, 1.0);
        assert_eq!(folded.open, None);
        assert_eq!(folded.ask.as_deref(), Some("Send to…"));
        assert_eq!(flow.keys(), Some(CapsuleKeys::Route));
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.tick(tuning::get().done_hold_s);
        assert_eq!(flow.pres(tuning::get().done_hold_s), Pres::Hidden);
    }

    #[test]
    fn shift_return_saves_a_rule_for_the_site() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::OpenList, 0.5);
        let actions = flow.input(Input::Send { always: true }, 1.0);
        assert_eq!(
            actions,
            vec![Action::Send {
                to: "SEER".into(),
                always: Some("problems.kitlangton.com".into())
            }]
        );
        let shown = done(&flow, 1.0);
        assert_eq!(shown.why.as_deref(), Some("rule saved"));
    }

    #[test]
    fn an_answer_after_the_capsule_closed_leaves_it_closed() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        let closed = tuning::get().done_hold_s + 1.0;
        flow.tick(closed);
        assert_eq!(flow.pres(closed), Pres::Hidden);
        assert!(flow.input(Input::Routed(guess()), closed).is_empty());
        assert_eq!(flow.pres(closed), Pres::Hidden);
        flow.input(
            Input::Delivered(Delivered::Queued {
                to: "SEER".into(),
                why: "couldn't reach OpenCode".into(),
            }),
            closed,
        );
        assert_eq!(flow.pres(closed), Pres::Hidden);
        // The next capture shows as ever.
        flow.input(Input::Stage(Stage::Done), closed + 1.0);
        flow.input(Input::Routed(guess()), closed + 2.0);
        assert!(matches!(flow.pres(closed + 2.0), Pres::Done(_)));
    }

    /// Kit's ⌥⇧V after a guess: the list shortcut opens the list while the
    /// capsule shows, and brings it back after it closed with nothing
    /// picked, until something settles the capture.
    #[test]
    fn the_list_shortcut_brings_back_a_capture_waiting_for_a_pick() {
        use crate::capture_host::CapsuleKeys;
        let hold = tuning::get().done_hold_s;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        flow.input(Input::Routed(guess()), 0.0);
        assert_eq!(flow.keys(), Some(CapsuleKeys::Suggest { rows: 3 }));
        // Its hold runs out: hidden, but ⌥⇧V is still the capsule's.
        assert!(flow.tick(hold + 1.0));
        assert_eq!(flow.pres(hold + 1.0), Pres::Hidden);
        assert_eq!(flow.keys(), Some(CapsuleKeys::Route));
        assert!(flow.input(Input::OpenList, 30.0).is_empty());
        let (query, list) = done(&flow, 30.0).open.expect("the list is open");
        assert_eq!(query, "");
        assert_eq!(list.selected_dest().unwrap().name, "SEER");
        assert_eq!(flow.keys(), Some(CapsuleKeys::List));
        // Escape folds the list; it then leaves with its hold and waits again.
        flow.input(Input::Escape, 31.0);
        assert!(matches!(flow.pres(32.0), Pres::Done(_)));
        flow.tick(31.0 + hold + 1.0);
        assert_eq!(flow.pres(32.0 + hold), Pres::Hidden);
        flow.input(Input::OpenList, 40.0);
        assert!(done(&flow, 40.0).open.is_some());
        // A pick settles it: no key is Hex's, Escape and ⌥⇧V go to the app.
        assert_eq!(
            flow.input(Input::SendRow(1), 41.0),
            [Action::Send {
                to: "HEX".into(),
                always: None
            }]
        );
        assert_eq!(flow.keys(), None);
        flow.tick(41.0 + hold + 1.0);
        assert_eq!(flow.keys(), None);

        // `hex capture send` while it's closed settles it too.
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::Dismiss, 1.0);
        assert!(flow.keys().is_some());
        flow.input(Input::Settled, 2.0);
        assert_eq!(flow.keys(), None);
        flow.input(Input::OpenList, 3.0);
        assert_eq!(flow.pres(3.0), Pres::Hidden);
        // As it does while the capsule shows.
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::Settled, 1.0);
        flow.input(Input::Dismiss, 2.0);
        assert_eq!(flow.keys(), None);

        // So do a paste and the next capture; a rule's send never waits.
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.click(super::super::surface::Id::Paste, 1.0);
        assert_eq!(flow.keys(), None);
        flow.input(Input::Routed(guess()), 2.0);
        flow.input(Input::Dismiss, 3.0);
        flow.input(Input::Stage(Stage::Recording), 4.0);
        flow.input(Input::Stage(Stage::Done), 5.0);
        assert_eq!(flow.keys(), None);
        flow.input(Input::Routed(rule()), 5.0);
        flow.tick(5.0 + hold + 1.0);
        assert_eq!(flow.pres(5.0 + hold + 1.0), Pres::Hidden);
        assert_eq!(flow.keys(), None);

        // An answer that came after the capsule closed waits as well.
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        flow.tick(hold + 1.0);
        flow.input(Input::Routed(guess()), hold + 2.0);
        assert_eq!(flow.pres(hold + 2.0), Pres::Hidden);
        flow.input(Input::OpenList, hold + 3.0);
        assert!(done(&flow, hold + 3.0).open.is_some());
    }

    #[test]
    fn shift_return_in_a_rules_list_saves_a_rule_elsewhere() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        // A rule's list opens on the next best place.
        flow.input(Input::OpenList, 0.5);
        assert_eq!(
            flow.input(Input::Send { always: true }, 1.0),
            [Action::Send {
                to: "ses_hex".into(),
                always: Some("problems.kitlangton.com".into())
            }]
        );
        assert_eq!(done(&flow, 1.0).why.as_deref(), Some("rule saved"));

        // And in the list an Undo of the rule's send reopens.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        assert_eq!(
            flow.input(Input::Send { always: true }, 2.0),
            [Action::Send {
                to: "ses_hex".into(),
                always: Some("problems.kitlangton.com".into())
            }]
        );
    }

    #[test]
    fn cues_mark_start_stop_sent_and_taking_back() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        assert_eq!(flow.take_cues(), [Cue::Start]);
        flow.input(Input::Stage(Stage::Recording), 0.1);
        assert!(flow.take_cues().is_empty(), "a repeat stage is silent");
        flow.click(super::super::surface::Id::Finish, 1.0);
        flow.input(Input::Stage(Stage::Stopping), 1.0);
        assert_eq!(flow.take_cues(), [Cue::Stop]);
        flow.input(Input::Stage(Stage::Done), 2.0);
        flow.input(Input::Routed(rule()), 2.0);
        assert!(flow.take_cues().is_empty(), "the router hasn't sent it");
        flow.input(Input::Settled, 2.2);
        flow.tick(7.5);
        assert_eq!(flow.take_cues(), [Cue::Sent], "once, when it went");

        // Taken back: the undo sounds, and the send never does, even when
        // the router says it went before the take-back.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Settled, 1.1);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        flow.tick(9.0);
        assert_eq!(flow.take_cues(), [Cue::TakenBack]);

        // A discarded recording (Escape sounds nothing), and a failure.
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Undo, 0.5);
        flow.click(super::super::surface::Id::Cancel, 0.8);
        assert_eq!(flow.take_cues(), [Cue::Start]);
        flow.click(super::super::surface::Id::Discard, 1.0);
        assert_eq!(flow.take_cues(), [Cue::TakenBack]);
        flow.input(Input::Failed(Failure::Start, None), 2.0);
        assert_eq!(flow.take_cues(), [Cue::Failed]);
    }

    #[test]
    fn nothing_heard_says_so() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::NothingHeard, 1.0);
        flow.input(Input::Stage(Stage::Done), 1.0);
        flow.input(Input::Routed(Route::Nothing { recent: Vec::new() }), 1.0);
        let shown = done(&flow, 1.0);
        assert_eq!(shown.text, "Copied");
        assert_eq!(shown.why.as_deref(), Some("no speech heard"));
        // The next recording starts fresh.
        flow.input(Input::Stage(Stage::Recording), 2.0);
        flow.input(Input::Stage(Stage::Done), 3.0);
        flow.input(Input::Routed(Route::Nothing { recent: Vec::new() }), 3.0);
        assert_eq!(done(&flow, 3.0).why, None);
    }

    #[test]
    fn escape_in_the_list_keeps_it_where_it_was() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        flow.input(Input::OpenList, 6.0);
        let (_, list) = done(&flow, 6.0).open.unwrap();
        assert_eq!(
            list.selected_dest().unwrap().name,
            "HOME",
            "a rule opens on the next"
        );
        assert_eq!(list.footer, Footer::Keep("SEER".into()));
        assert!(flow.next_due(6.0).is_none(), "an open list stays");
        assert!(flow.input(Input::Escape, 7.0).is_empty());
        assert!(done(&flow, 7.0).open.is_none());
    }

    #[test]
    fn moving_stays_inside_the_list() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::OpenList, 0.0);
        flow.input(Input::Move(-4), 0.0);
        assert_eq!(done(&flow, 0.0).open.unwrap().1.selected, 0);
        flow.input(Input::Move(9), 0.0);
        assert_eq!(done(&flow, 0.0).open.unwrap().1.selected, 2);
    }

    #[test]
    fn the_router_not_answering_still_offers_the_list() {
        let mut flow = Flow::new();
        flow.input(
            Input::Routed(Route::NoAnswer {
                recent: vec![Dest::new("HOME", "yesterday")],
            }),
            0.0,
        );
        let shown = done(&flow, 0.0);
        assert_eq!(shown.why.as_deref(), Some("the router didn't answer"));
        let (_, list) = shown.open.expect("recent sessions are the suggestions");
        assert_eq!(list.rows().next().unwrap().name, "HOME");
    }

    #[test]
    fn opencode_out_of_reach_says_so_and_leaves_the_list_key_alone() {
        let mut flow = Flow::new();
        flow.input(
            Input::Routed(Route::Unreachable {
                why: "didn't answer in time".into(),
            }),
            0.0,
        );
        let shown = done(&flow, 0.0);
        assert_eq!(shown.lead, Lead::Warning);
        assert_eq!(shown.text, "Couldn't reach OpenCode");
        assert_eq!(shown.why.as_deref(), Some("didn't answer in time"));
        assert_eq!(shown.ask, None);
        assert!(shown.hand_off);
        // Nothing to list, so ⌥⇧V reaches the app, and a stray OpenList opens
        // nothing.
        assert_eq!(flow.keys(), None);
        flow.input(Input::OpenList, 0.5);
        assert_eq!(done(&flow, 0.5).open, None);
    }

    /// Kit: Escape never ends a screen recording (it may be meant for the
    /// app); ✕ asks, and Discard, the second click, ends it.
    #[test]
    fn discarding_a_recording_takes_two_clicks() {
        use super::super::surface::Id;
        let discarding = |flow: &Flow, now| {
            matches!(
                flow.pres(now),
                Pres::Recording {
                    discarding: true,
                    chips: false,
                    ..
                }
            )
        };
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        assert_eq!(flow.keys(), None, "Escape stays the app's");
        assert!(flow.input(Input::Escape, 0.5).is_empty());
        assert!(matches!(flow.pres(0.5), Pres::Recording { .. }));
        assert!(flow.click(Id::Discard, 0.6).is_empty(), "not asked yet");

        assert!(flow.click(Id::Cancel, 1.0).is_empty());
        assert!(discarding(&flow, 1.0));
        assert!(flow.input(Input::Escape, 1.5).is_empty());
        assert!(discarding(&flow, 1.5));
        assert_eq!(flow.keys(), None);
        // Nothing clicked: back to recording on its own.
        assert_eq!(flow.next_due(1.0), Some(1.0 + DISCARD_ASK_S));
        assert!(!flow.tick(1.0 + DISCARD_ASK_S - 0.1));
        assert!(flow.tick(1.0 + DISCARD_ASK_S));
        assert!(!discarding(&flow, 1.0 + DISCARD_ASK_S));
        assert!(flow.cues.iter().all(|cue| *cue != Cue::TakenBack));

        // Never under the pointer, and a moment after it leaves; the
        // chips' ✕ asks too.
        flow.input(Input::Chips, 9.0);
        assert!(flow.click(Id::Close, 10.0).is_empty());
        flow.hover(true, 10.0);
        assert!(!flow.tick(20.0));
        assert!(discarding(&flow, 20.0));
        flow.hover(false, 20.0);
        assert!(!flow.tick(20.0 + DISCARD_LINGER_S - 0.1));
        assert!(flow.tick(20.0 + DISCARD_LINGER_S));
        assert!(!discarding(&flow, 22.0));

        // Keep recording goes back at once.
        flow.click(Id::Cancel, 30.0);
        assert!(flow.click(Id::Keep, 31.0).is_empty());
        assert!(!discarding(&flow, 31.0));

        // Discard ends it, keeping nothing.
        flow.click(Id::Cancel, 40.0);
        assert_eq!(flow.click(Id::Discard, 41.0), [Action::Cancel]);
        assert_eq!(flow.pres(41.0), Pres::Hidden);

        // The picker still closes on Escape, and on ✕ at once.
        for close in [Input::Escape, Input::Dismiss] {
            let mut flow = Flow::new();
            flow.input(Input::Stage(Stage::Recording), 0.0);
            flow.input(Input::Chips, 0.5);
            flow.click(Id::Mode(Mode::Window), 1.0);
            assert_eq!(flow.keys(), Some(crate::capture_host::CapsuleKeys::Picker));
            if close == Input::Escape {
                assert_eq!(flow.input(close, 2.0), [Action::Cancel]);
            } else {
                assert_eq!(flow.click(Id::Close, 2.0), [Action::Cancel]);
            }
            assert_eq!(flow.pres(2.0), Pres::Hidden);
        }
    }

    #[test]
    fn cancel_and_failure() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.click(Id::Cancel, 0.5);
        assert_eq!(flow.click(Id::Discard, 1.0), vec![Action::Cancel]);
        assert_eq!(flow.pres(1.0), Pres::Hidden);
        flow.input(Input::Stage(Stage::Failed), 2.0);
        assert_eq!(done(&flow, 2.0).lead, Lead::Warning);
        flow.tick(2.0 + tuning::get().brief_hold_s);
        assert_eq!(flow.pres(9.0), Pres::Hidden);
        flow.input(
            Input::Failed(
                Failure::Start,
                Some("starting the capture: the window closed".into()),
            ),
            10.0,
        );
        assert_eq!(done(&flow, 10.0).text, "Couldn't start the capture");
        assert_eq!(done(&flow, 10.0).why.as_deref(), Some("the window closed"));
        flow.input(Input::Failed(Failure::Save, None), 11.0);
        assert_eq!(done(&flow, 11.0).text, "Couldn't save the capture");
        flow.input(
            Input::Failed(Failure::Send("SEER".into()), Some("offline".into())),
            12.0,
        );
        assert_eq!(done(&flow, 12.0).text, "Couldn't send to SEER");
        assert_eq!(done(&flow, 12.0).why.as_deref(), Some("offline"));
        assert_eq!(
            short_reason("Screen Recording permission is off for Hex"),
            "Screen Recording permission is…"
        );
    }
}
