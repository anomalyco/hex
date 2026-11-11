//! What the capture capsule shows, decided from what happens: the
//! capture's stages from `capture_host`, the router's answer, and the keys
//! and clicks the capture input sends. Pure: no Rune, no clock of its own.
//! [`Flow::input`] and [`Flow::tick`] move it, [`Flow::pres`] says what the
//! capsule draws, and each returns the [`Action`]s the host carries out.

use rune::TextEdit;
use scrawl_capture::capture::Stage;

use crate::capture_host::{AskedBy, CaptureNeed};
use crate::desktop::i18n::{tr, tr_fill};

use super::tuning::{self, CapsuleFlow};
mod done;
mod press;
mod routes;
mod step;
mod words;
use done::*;
use routes::{sections, sent_to, short};
pub(crate) use words::copied;
use words::{already_sent_to, not_running, plain_reason};

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
/// A countdown the pointer held goes on with at least this long left, so
/// it never ends the instant the pointer leaves.
const RESUME_LEFT_S: f64 = 1.0;
/// The recording's ✕ asks "Discard?" for this long, then the capsule goes
/// back to recording; never while the pointer is on it, and this much
/// longer once it leaves.
pub(crate) const DISCARD_ASK_S: f64 = 4.0;
pub(crate) const DISCARD_LINGER_S: f64 = 1.5;
/// The rows a finished capture's suggestions show under it, when the
/// router has that many, and the most a search shows.
pub(crate) const SUGGEST_ROWS: usize = 5;
/// The menu-bar flow's hint of the guesses stays this long, then the
/// capsule folds into Hex's menu-bar item, where the capture waits.
pub(crate) const MENU_HINT_S: f64 = 4.0;
/// The permission helper waits this long for a grant, then folds; the
/// capture shortcut brings it back.
pub(crate) const PERMISSION_HOLD_S: f64 = 120.0;
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
            // Wayland gives Hex no pointer to aim an area with yet.
            .filter(|mode| cfg!(not(target_os = "linux")) || *mode != Mode::Area)
            .chain(phone.then_some(Mode::Iphone))
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Mode::Area => tr("Area"),
            Mode::Window => tr("Window"),
            Mode::Screen => tr("Screen"),
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
    /// The name of the session's folder, when known: the search matches
    /// it, and a row shows it only when its title alone can't tell it apart.
    pub(crate) folder: Option<String>,
}

impl Dest {
    /// A session known by its title alone, which the host resolves.
    pub(crate) fn new(name: impl Into<String>, why: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            session: name.clone(),
            name,
            why: why.into(),
            folder: None,
        }
    }

    pub(crate) fn session(self, session: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            ..self
        }
    }

    pub(crate) fn folder(self, folder: Option<String>) -> Self {
        Self { folder, ..self }
    }
}

/// The router's answer for a finished capture (Scrawl's design f: only a
/// rule ever sends; a guess suggests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    ResolvingRule {
        name: String,
    },
    /// A rule matched and will send to `to`. `site` is what the rule
    /// matches, for an always-rule sending it elsewhere instead.
    Rule {
        to: Dest,
        suggested: Vec<Dest>,
        recent: Vec<Dest>,
        site: Option<String>,
    },
    /// Guesses, best first; nothing was sent. `site` is what a rule
    /// would match ("example.com").
    Guess {
        suggested: Vec<Dest>,
        recent: Vec<Dest>,
        site: Option<String>,
    },
    /// Nothing fits: recent sessions only.
    Nothing {
        recent: Vec<Dest>,
    },
    /// The router never answered.
    NoAnswer {
        recent: Vec<Dest>,
    },
    /// OpenCode's sessions couldn't be read, so there's no list: `why` in
    /// a few words.
    Unreachable {
        why: String,
    },
}

/// What the capture engine says became of the bundle
/// (`scrawl_capture::delivery::Delivery`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Delivered {
    Sent {
        to: String,
    },
    /// `why` it didn't go, in a few words ("OpenCode can't see files").
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

/// Where the permission helper's grant stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grant {
    Off,
    /// Allow was pressed: System Settings and its guide are open.
    Asked,
    /// Granted, but Hex must restart to use it.
    Restart,
    /// Granted: the next capture records.
    Ready,
}

/// Everything that moves the capsule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Input {
    /// A capture (or the Screenshots picker) was refused for a permission
    /// Hex lacks: the helper opens in its place, and its grant returns to
    /// what asked.
    NeedsPermission(CaptureNeed, AskedBy),
    /// How that permission stands now, while the helper shows.
    Permission(CaptureNeed, Grant),
    /// A capture's stage, from `capture_host`'s engine events.
    Stage(Stage),
    Delivered(Delivered),
    Routed(Route),
    Refined(Route),
    /// Something failed, and why, when the host knows.
    Failed(Failure, Option<String>),
    /// An iPhone was cabled (true) or unplugged.
    Phone(bool),
    /// The target chips spring open: only the playground's stories now;
    /// a recording's capsule shows its toolbar instead.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Chips,
    /// Annotate started (true) or stopped drawing: the pen lights.
    Drawing(bool),
    /// The recording got its first click, scroll, or stroke: the chips
    /// fold, once they have been seen.
    Worked,
    /// The capture shortcut while the picker is open: record what it's on.
    /// Over a capture's suggestions it opens the search.
    Record,
    /// Send to…: the list opens (or, when open,
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
    /// A row clicked: send to it.
    SendRow(usize),
    /// Tab in the toast (⇧ back): its next guess, round again after the
    /// last.
    Cycle(i32),
    /// Shift went down (true) or up while the list or suggestions show: a
    /// pick while it's down also saves a rule for the site or app.
    Shift(bool),
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
    /// The Screenshots picker, once its permission is granted: opens it
    /// again to shoot, or to record when `record` is set, instead of
    /// starting an Annotate capture.
    OpenScreenshots {
        record: bool,
    },
    /// The permission helper's Allow: macOS's prompt, or System Settings
    /// with the guide beside it.
    Allow(CaptureNeed),
    /// The permission helper's Restart Hex.
    Restart,
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
    /// Hold a rule's send while the pointer is on the capsule (`true`),
    /// or let it go on with the time it had left.
    Hold(bool),
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
    /// The pen: drawing on until it's clicked again.
    Pen,
}

/// The glyph before the finished capsule's label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lead {
    Check,
    Warning,
    /// The capture's first frame, in the check's place once it's decoded.
    Thumbnail,
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
    /// A send was taken back.
    TakenBack,
    /// A recording was discarded.
    Discarded,
    Failed,
}

/// The open list's footer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Footer {
    /// The search: ⏎ sends, the capture shortcut records anew, and
    /// `remember`: ⇧⏎ also saves a rule for the site or app.
    Search { remember: bool },
    /// The suggestions have none: a click sends, and the header says ⇧
    /// remembers.
    Suggesting,
}

/// The list under the open capsule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct List {
    /// The suggestions, then the newest sessions; searching, every
    /// session that matches.
    pub(crate) rows: Vec<Dest>,
    /// The header over the rows.
    pub(crate) title: &'static str,
    /// The rows the list keeps room for, so a search that matches fewer
    /// than the suggestions it opened from never shrinks it.
    pub(crate) min_rows: usize,
    pub(crate) selected: usize,
    /// The row a Shift pick would send to: the one under the pointer, else
    /// the search's highlighted row. Over the suggestions, a click picks,
    /// so with the pointer off the rows there is none.
    pub(crate) target: Option<usize>,
    pub(crate) footer: Footer,
    /// The site or app a Shift pick saves a rule for, and whether Shift is
    /// down: then the footer (or the suggestions' header) says so and the
    /// target row reads "always".
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
        self.rows.iter()
    }

    pub(crate) fn selected_dest(&self) -> Option<&Dest> {
        self.rows().nth(self.selected)
    }

    pub(crate) fn target_dest(&self) -> Option<&Dest> {
        self.target.and_then(|row| self.rows().nth(row))
    }
}

/// The finished capsule: its lead, a quiet word on why, the Undo chip,
/// the route chips, the Send to… chip, and (open) the field with its list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Done {
    pub(crate) lead: Lead,
    pub(crate) text: String,
    pub(crate) why: Option<String>,
    /// It can still be taken back: the Undo chip shows.
    pub(crate) undo: bool,
    /// When Undo goes, on the flow's clock: the chip's ring drains to it.
    /// `None` while Undo waits on the router with no window to count.
    pub(crate) countdown: Option<f64>,
    /// The pointer holds the countdown: how long it had left. The ring
    /// stands still until the pointer leaves.
    pub(crate) held: Option<f64>,
    /// The Send to… chip's label.
    pub(crate) ask: Option<String>,
    /// The capture shortcut opens the search from here: its keycaps sit
    /// on the chip ("Search ⌥⇧S").
    pub(crate) ask_keys: bool,
    /// Open: what's typed into the field, and the list under it.
    pub(crate) open: Option<(TextEdit, List)>,
    /// A written capture: Paste and Inspect, as Scrawl's capsule offers.
    pub(crate) hand_off: bool,
    /// The guesses in the capsule itself, picked with a click: chips
    /// in the menu-bar flow, rows in the cursor's card and the notch.
    /// Each is a name and its quiet reason.
    pub(crate) picks: Vec<(String, String)>,
    /// The toast's "Next ⇥": more than one guess to go through.
    pub(crate) cycle: bool,
    /// The label is the toast's one guess: a click on it sends there.
    pub(crate) sends: bool,
    /// About no capture (the permission helper): its glyph never gives
    /// way to an earlier capture's frame.
    pub(crate) no_frame: bool,
}

impl Done {
    pub(crate) fn plain(lead: Lead, text: impl Into<String>) -> Self {
        Self {
            lead,
            text: text.into(),
            why: None,
            undo: false,
            countdown: None,
            held: None,
            ask: None,
            ask_keys: false,
            open: None,
            hand_off: false,
            picks: Vec::new(),
            cycle: false,
            sends: false,
            no_frame: false,
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
    /// `pen`: the drawing pen beside the timer.
    Recording {
        chips: bool,
        phone: bool,
        discarding: bool,
        pen: Pen,
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

/// The recording capsule's pen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pen {
    /// The pen icon; a click turns drawing on.
    Off,
    /// Drawing: the pen lit; a click turns it off.
    On,
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
    /// The permission helper: a capture needs `need`, and its grant returns
    /// to `asked_by`.
    Permission {
        need: CaptureNeed,
        grant: Grant,
        at: f64,
        asked_by: AskedBy,
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
    query: Box<TextEdit>,
    selected: usize,
    /// Searching every session (Send to…): the field
    /// takes typing. Otherwise it's the suggestions a capture opens with.
    typing: bool,
    /// The search opened over the suggestions: Escape and a click away go
    /// back to them.
    back: bool,
    /// Shift is down: a pick also saves a rule.
    shift: bool,
    /// The row the pointer moved onto, until it leaves the row, a key
    /// moves the highlight, or the rows change under it.
    pointed: Option<usize>,
}

impl Open {
    fn suggesting() -> Self {
        Self {
            query: Box::default(),
            selected: 0,
            typing: false,
            back: false,
            shift: false,
            pointed: None,
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

/// An Undo countdown the pointer holds: since when, and the window's end
/// as it stood.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pause {
    since: f64,
    end: f64,
    /// A rule's: the router holds its send too.
    router: bool,
}

/// The capsule's state machine.
#[derive(Clone, Debug)]
pub(crate) struct Flow {
    phase: Phase,
    /// The pointer is on the capsule.
    hovered: bool,
    /// The pointer moved on the capsule since its suggestions last
    /// changed: typing over them searches. Being on it isn't enough, since
    /// a list that opens or grows under a resting pointer is hovered too,
    /// while Kit may still be typing in the app in front.
    aimed: bool,
    /// The pointer holds the Undo countdown: nothing sends or expires
    /// until it leaves.
    pause: Option<Pause>,
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
    /// The latest capture's list, closed before anything was picked: kept
    /// until a pick, a paste, a send from elsewhere (`hex capture send`), or
    /// the next capture.
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
    /// A guess opens its suggestions under the finished capsule (the Mac).
    /// On Linux the capsule shows Paste first and Send to… waits for a
    /// click, so Dax can paste again without a list in the way.
    suggest_on_route: bool,
    /// Annotate draws now.
    drawing: bool,
    /// A capture that just finished with no screen asked for the
    /// permission helper: it opens once that capture's capsule folds, and
    /// returns to what asked.
    helper_next: Option<(CaptureNeed, AskedBy)>,
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
            aimed: false,
            pause: None,
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
            suggest_on_route: true,
            drawing: false,
            helper_next: None,
        }
    }

    /// Whether a guess opens its suggestions by itself; the Linux capsule
    /// says no (see [`Flow::suggest_on_route`]).
    pub(crate) fn suggesting_on_route(mut self, suggest: bool) -> Self {
        self.suggest_on_route = suggest;
        self
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
            // Send to… bringing a taken-back capture back.
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
                Some(Cue::Discarded)
            }
            _ => None,
        };
        self.cues.extend(cue);
    }

    /// The pointer came onto the capsule or left it. On it, an Undo
    /// countdown holds (and the router holds a rule's send with it).
    pub(crate) fn hover(&mut self, on: bool, now: f64) -> Vec<Action> {
        self.hovered = on;
        if !on {
            self.aimed = false;
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
        self.hold_countdown(now)
    }

    /// The Undo window's clock: held, it stands where the pointer stopped it.
    fn clock(&self, now: f64) -> f64 {
        self.pause.map_or(now, |pause| pause.since.min(now))
    }

    /// Where Undo's window ends, as the capsule draws it now.
    fn countdown(&self, now: f64) -> Option<f64> {
        match self.pres_of(now) {
            Pres::Done(done) if done.undo => done.countdown,
            _ => None,
        }
    }

    /// Holds the Undo countdown while the pointer is on the capsule, one
    /// that starts under it too, and lets it go on with what it had left
    /// (at least [`RESUME_LEFT_S`]) once the pointer leaves. The router
    /// holds a rule's send alongside ([`Action::Hold`]); a pick went
    /// already, so only its Undo waits.
    fn hold_countdown(&mut self, now: f64) -> Vec<Action> {
        let end = self.countdown(now);
        match (self.hovered, end, self.pause) {
            (true, Some(end), Some(pause)) if pause.end == end => Vec::new(),
            (true, Some(end), old) => {
                let router = matches!(self.phase, Phase::Routed(_));
                self.pause = Some(Pause {
                    since: now,
                    end,
                    router,
                });
                match (old.is_some_and(|old| old.router), router) {
                    (_, true) => vec![Action::Hold(true)],
                    (true, false) => vec![Action::Hold(false)],
                    (false, false) => Vec::new(),
                }
            }
            (_, _, Some(pause)) => {
                self.pause = None;
                if end == Some(pause.end) {
                    let left = (pause.end - pause.since).clamp(RESUME_LEFT_S, undo_s());
                    let by = now + left - pause.end;
                    match &mut self.phase {
                        Phase::Routed(routed) => routed.at += by,
                        Phase::Sent(sent) => sent.at += by,
                        _ => {}
                    }
                }
                if pause.router {
                    vec![Action::Hold(false)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// The pointer moved on the capsule. True when that changes the keys
    /// it takes.
    pub(crate) fn pointer_moved(&mut self) -> bool {
        let before = self.keys();
        self.aimed = self.hovered;
        self.keys() != before
    }

    /// The pointer moved on the open list's `row` (`on`), or left it. The
    /// row it is on is where a Shift pick goes; in the search the highlight
    /// follows it too, as in a menu, and the arrow keys go on from there.
    /// True when that changes what the capsule draws.
    pub(crate) fn point(&mut self, row: usize, on: bool) -> bool {
        let Phase::Routed(Routed {
            route,
            open: Some(open),
            ..
        }) = &mut self.phase
        else {
            return false;
        };
        let before = (open.pointed, open.selected);
        if on {
            if row >= list(route, open).rows().count() {
                return false;
            }
            open.pointed = Some(row);
            if open.typing {
                open.selected = row;
            }
        } else if open.pointed == Some(row) {
            open.pointed = None;
        }
        (open.pointed, open.selected) != before
    }

    pub(crate) fn input(&mut self, input: Input, now: f64) -> Vec<Action> {
        // A new capture or a new answer may change the list under a resting
        // pointer: it has to move again before typing searches.
        if matches!(
            input,
            Input::Stage(_) | Input::Routed(_) | Input::Refined(_) | Input::Undone(_)
        ) {
            self.aimed = false;
        }
        let before = self.phase.clone();
        let pointed = self.pointed();
        let actions = self.step(input, now);
        // Another session under a resting pointer: it has to move to
        // point again.
        if self.pointed() != pointed
            && let Phase::Routed(Routed {
                open: Some(open), ..
            }) = &mut self.phase
        {
            open.pointed = None;
        }
        self.cue(&before, &actions);
        self.closing(&before);
        self.left_waiting(before, &actions);
        let mut actions = actions;
        actions.extend(self.hold_countdown(now));
        actions
    }

    /// The session of the open list's row under the pointer.
    fn pointed(&self) -> Option<String> {
        let Phase::Routed(Routed {
            route,
            open: Some(open),
            ..
        }) = &self.phase
        else {
            return None;
        };
        let row = open.pointed?;
        list(route, open)
            .rows()
            .nth(row)
            .map(|dest| dest.session.clone())
    }

    /// A list closed with nothing picked waits for Send to….
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

    /// The keys the capsule takes from Hex's tap: the picker's, an open
    /// list's (Escape only in a list Kit opened or the picker). Closed or
    /// routed with nothing open, it takes none.
    pub(crate) fn keys(&self) -> Option<crate::capture_host::CapsuleKeys> {
        use crate::capture_host::CapsuleKeys;
        match &self.phase {
            Phase::Hidden | Phase::Kept { .. } => None,
            Phase::Selecting(_) => Some(CapsuleKeys::Picker),
            // Nothing to list; ✕ dismisses, or it
            // leaves by itself.
            Phase::TakenBack { .. } => None,
            Phase::Routed(Routed {
                open: Some(open), ..
            }) => Some(if open.typing {
                CapsuleKeys::List
            } else {
                CapsuleKeys::Suggest {
                    cycle: tuning::get().flow == CapsuleFlow::Toast,
                    search: self.aimed,
                }
            }),
            Phase::Routed(Routed {
                route: Route::Unreachable { .. },
                ..
            }) => None,
            // Never Escape here: it goes to the app in front, and a send is
            // taken back only by clicking Undo.
            _ => None,
        }
    }

    /// The take-back for a send still waiting out its Undo.
    fn undoable(&self, now: f64) -> Option<Action> {
        routes::undoable(&self.phase, self.clock(now))
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
                    if let Some((need, asked_by)) = self.helper_next.take() {
                        self.phase = Phase::Permission {
                            need,
                            grant: Grant::Off,
                            at: now,
                            asked_by,
                        };
                    }
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
        // The Undo chip leaves when the window ends; its ring drains
        // without a tick.
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
            // A held countdown ends only once the pointer leaves.
            _ if self.pause.is_some() => None,
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
        if let (Pres::Done(done), Some(pause)) = (&mut pres, self.pause)
            && done.countdown == Some(pause.end)
        {
            done.held = Some(pause.end - pause.since);
        }
        // Scrawl's "No speech": a capture with no words still copies.
        if let Pres::Done(done) = &mut pres
            && self.silent
            && (done.text.is_empty() || done.text == copied())
            && done.why.is_none()
        {
            done.why = Some(tr("no speech").into());
        }
        pres
    }

    fn pres_of(&self, now: f64) -> Pres {
        let now = self.clock(now);
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
                let pen = if self.drawing { Pen::On } else { Pen::Off };
                Pres::Recording {
                    chips: !discarding && chips_until.is_some_and(|t| self.hovered || now < t),
                    phone: self.phone,
                    discarding,
                    pen,
                }
            }
            Phase::Processing => Pres::Processing,
            Phase::Saved { .. } => Pres::Done(Done {
                hand_off: true,
                ..Done::plain(Lead::Check, copied())
            }),
            Phase::Failed { what, why, .. } => Pres::Done(Done {
                why: why.clone(),
                ..Done::plain(
                    Lead::Warning,
                    match what {
                        Failure::Start => tr("Couldn't start").to_owned(),
                        Failure::Save => tr("Couldn't save").to_owned(),
                        Failure::Send(to) => tr_fill("Couldn't send to {}", &short(to)),
                    },
                )
            }),
            Phase::Permission { need, grant, .. } => Pres::Done(permission_done(*need, *grant)),
            Phase::TakenBack { .. } => Pres::Done(Done::plain(Lead::Check, tr("Taken back"))),
            Phase::Kept { .. } => Pres::Done(Done::plain(Lead::Check, tr("Kept"))),
            Phase::TooLate { to, .. } => Pres::Done(Done {
                hand_off: true,
                ..Done::plain(Lead::Check, already_sent_to(to))
            }),
            Phase::Queued { why, .. } => Pres::Done(Done {
                why: Some(why.clone()),
                hand_off: true,
                ..Done::plain(Lead::Check, "")
            }),
            // A pick went at once: the check, and Undo with its ring while
            // it can still be taken back.
            Phase::Sent(sent) => Pres::Done(Done {
                why: sent.rule_saved.then(|| tr("always").into()),
                undo: sent.undoable && undo_left(sent.at),
                countdown: (sent.undoable && undo_left(sent.at)).then(|| sent.at + undo_s()),
                hand_off: true,
                ..Done::plain(Lead::Check, sent_to(&sent.to))
            }),
            Phase::Routed(routed) => Pres::Done(routed_done(routed, now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess() -> Route {
        Route::Guess {
            suggested: vec![
                Dest::new("SEER", "same page"),
                Dest::new("HEX", "last sent"),
            ],
            recent: vec![Dest::new("HOME", "yesterday")],
            site: Some("example.com".into()),
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
            site: Some("example.com".into()),
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
                why: "OpenCode can't see files".into(),
            }),
            0.5,
        );
        let shown = done(&flow, 0.5);
        assert_eq!(shown.text, "");
        assert_eq!(shown.why.as_deref(), Some("OpenCode can't see files"));
    }

    /// The toast offers one guess at a time: ⇥ moves on, ⏎ (or a click
    /// on the line) sends the one it shows.
    #[test]
    fn the_toast_cycles_its_guesses_and_return_sends_the_one_shown() {
        use crate::capture_host::CapsuleKeys;
        tuning::with_flow(CapsuleFlow::Toast, || {
            let mut flow = Flow::new();
            flow.input(Input::Stage(Stage::Done), 0.0);
            flow.input(Input::Routed(guess()), 0.0);
            let shown = done(&flow, 0.1);
            assert_eq!(shown.text, "Send to SEER");
            assert_eq!(shown.why.as_deref(), Some("⏎"));
            assert!(shown.cycle && shown.sends && shown.open.is_none());
            assert_eq!(
                flow.keys(),
                Some(CapsuleKeys::Suggest {
                    cycle: true,
                    search: false
                })
            );
            flow.input(Input::Cycle(1), 0.5);
            assert_eq!(done(&flow, 0.5).text, "Send to HEX");
            flow.input(Input::Cycle(-1), 0.6);
            flow.input(Input::Cycle(-1), 0.7);
            assert_eq!(
                done(&flow, 0.7).text,
                "Send to HOME",
                "round past the first"
            );
            flow.input(Input::Cycle(1), 0.8);
            let sent = flow.input(Input::Send { always: false }, 1.0);
            assert_eq!(
                sent,
                [Action::Send {
                    to: "SEER".into(),
                    always: None
                }]
            );
        });
    }

    /// The menu-bar flow's guesses are chips for a moment, then the capsule
    /// goes.
    #[test]
    fn the_menu_bar_hint_is_brief_and_its_chips_send() {
        tuning::with_flow(CapsuleFlow::MenuBar, || {
            let mut flow = Flow::new();
            flow.input(Input::Stage(Stage::Done), 0.0);
            flow.input(Input::Routed(guess()), 0.0);
            let shown = done(&flow, 0.1);
            let names: Vec<&str> = shown.picks.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, ["SEER", "HEX", "HOME"]);
            assert!(shown.open.is_none());
            flow.tick(MENU_HINT_S + 0.1);
            assert_eq!(flow.pres(MENU_HINT_S + 0.1), Pres::Hidden);
            assert_eq!(flow.keys(), None, "no key is Hex's once it's gone");

            let mut flow = Flow::new();
            flow.input(Input::Stage(Stage::Done), 0.0);
            flow.input(Input::Routed(guess()), 0.0);
            let sent = flow.click(super::super::surface::Id::Pick(1), 1.0);
            assert_eq!(
                sent,
                [Action::Send {
                    to: "HEX".into(),
                    always: None
                }]
            );
        });
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
    fn the_capsule_takes_only_the_keys_its_state_needs() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        assert_eq!(flow.keys(), None);
        flow.input(Input::Chips, 0.5);
        flow.click(super::super::surface::Id::Mode(Mode::Window), 1.0);
        assert_eq!(flow.keys(), Some(CapsuleKeys::Picker));

        flow.input(Input::Routed(rule()), 2.0);
        assert_eq!(flow.keys(), None);
        flow.input(Input::OpenList, 9.0);
        assert_eq!(flow.keys(), Some(CapsuleKeys::List));
        flow.input(Input::Type("ho".into()), 9.1);
        flow.input(Input::Type("x".into()), 9.2);
        flow.input(Input::Backspace, 9.3);
        assert_eq!(done(&flow, 9.3).open.unwrap().0.text, "ho");
        flow.input(Input::SendRow(0), 9.4);
        assert_eq!(flow.keys(), None, "a send takes no keys: Undo is a click");

        flow.input(Input::Stage(Stage::Done), 20.0);
        assert_eq!(flow.keys(), None);
    }

    /// The search's field is a `TextEdit` fed by Hex's tap: typing inserts
    /// at the caret, Backspace takes one character (not one byte) back,
    /// and the caret follows the text.
    #[test]
    fn the_search_field_edits_at_its_caret() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        flow.input(Input::Routed(rule()), 2.0);
        flow.input(Input::OpenList, 9.0);
        let field = |flow: &Flow| done(flow, 9.5).open.unwrap().0;
        assert_eq!(field(&flow).text, "");

        flow.input(Input::Type("hé".into()), 9.1);
        flow.input(Input::Type("x".into()), 9.2);
        let typed = field(&flow);
        assert_eq!(typed.text, "héx");
        assert_eq!(typed.range(), (4, 4), "the caret sits after what's typed");

        flow.input(Input::Backspace, 9.3);
        flow.input(Input::Backspace, 9.3);
        let typed = field(&flow);
        assert_eq!(typed.text, "h");
        assert_eq!(typed.range(), (1, 1));

        flow.input(Input::Backspace, 9.4);
        flow.input(Input::Backspace, 9.4);
        let typed = field(&flow);
        assert_eq!(typed.text, "", "Backspace on an empty field does nothing");
        assert_eq!(typed.range(), (0, 0));
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
                pen: Pen::Off,
            }
        );
        flow.input(Input::Chips, 1.0);
        assert_eq!(
            flow.pres(1.0),
            Pres::Recording {
                chips: true,
                phone: false,
                discarding: false,
                pen: Pen::Off,
            }
        );
        assert!(flow.tick(1.0 + tuning::get().chips_open_s));
        assert_eq!(
            flow.pres(5.0),
            Pres::Recording {
                chips: false,
                phone: false,
                discarding: false,
                pen: Pen::Off,
            }
        );
        for stage in [Stage::Stopping, Stage::Transcribing, Stage::Writing] {
            flow.input(Input::Stage(stage), 6.0);
            assert_eq!(flow.pres(6.0), Pres::Processing);
        }
        flow.input(Input::Stage(Stage::Done), 7.0);
        assert_eq!(done(&flow, 7.0).text, copied());
        assert!(flow.tick(7.0 + tuning::get().done_hold_s));
        assert_eq!(flow.pres(40.0), Pres::Hidden);
    }

    /// The pen lights while drawing, and a click on it asks for the pen.
    #[test]
    fn the_pen_lights_while_drawing() {
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Recording), 0.0);
        let pen = |flow: &Flow, now| match flow.pres(now) {
            Pres::Recording { pen, .. } => pen,
            other => panic!("{other:?}"),
        };
        assert_eq!(pen(&flow, 1.0), Pen::Off);
        flow.input(Input::Drawing(true), 4.0);
        assert_eq!(pen(&flow, 4.0), Pen::On);
        flow.input(Input::Drawing(false), 5.0);
        assert_eq!(pen(&flow, 5.0), Pen::Off);
        assert_eq!(
            flow.click(super::super::surface::Id::Pen, 6.0),
            [Action::Pen]
        );
        // A new recording starts with the pen off.
        flow.input(Input::Drawing(true), 7.0);
        flow.input(Input::Stage(Stage::Done), 8.0);
        flow.input(Input::Stage(Stage::Recording), 9.0);
        assert_eq!(pen(&flow, 9.5), Pen::Off);
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
                pen: Pen::Off,
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
                pen: Pen::Off,
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
    fn a_rule_counts_down_and_stays_cancellable_until_delivered() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        let at = |t| done(&flow, t).undo;
        assert!(at(10.0) && at(11.2) && at(14.5));
        assert!(at(15.0), "a delayed send is still cancellable");
        assert_eq!(done(&flow, 10.0).text, "Sending to SEER");
        assert_eq!(done(&flow, 10.0).lead, Lead::Check);
        assert_eq!(done(&flow, 10.0).ask, None, "Undo, as a pick has");
        assert_eq!(done(&flow, 15.0).ask, None, "nothing after the window");
        // No words count it down: the Undo chip's ring drains to the
        // window's end, the same moment whenever it is asked.
        assert_eq!(done(&flow, 10.0).why, None);
        for t in [10.0, 11.0, 12.0] {
            assert_eq!(done(&flow, t).countdown, Some(10.0 + undo_s()));
        }
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
        late.input(Input::Settled, 3.0);
        assert!(!late.tick(undo_s() - 0.1));
        assert_eq!(late.next_due(3.0), Some(undo_s()));
        let leaves = undo_s().max(3.0 + tuning::get().brief_hold_s);
        assert!(late.tick(leaves));
        assert_eq!(late.pres(leaves), Pres::Hidden);
    }

    /// The pointer on the capsule holds Undo's countdown (the router holds
    /// a rule's send with it); leaving lets it go on with what it had left.
    #[test]
    fn hover_holds_the_undo_countdown_and_leaving_resumes_it() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        assert_eq!(flow.hover(true, 11.5), [Action::Hold(true)]);
        for t in [12.0, 15.0, 30.0] {
            let held = done(&flow, t);
            assert!(held.undo, "Undo never runs out under the pointer");
            assert_eq!(held.countdown, Some(10.0 + undo_s()));
            assert_eq!(held.held, Some(undo_s() - 1.5), "the ring stands still");
        }
        assert_eq!(flow.next_due(30.0), None);
        // Two seconds held, it ends two seconds later than it would have.
        assert_eq!(flow.hover(false, 13.5), [Action::Hold(false)]);
        let going = done(&flow, 13.5);
        assert_eq!(going.countdown, Some(10.0 + undo_s() + 2.0));
        assert_eq!(going.held, None);
        assert_eq!(flow.next_due(13.5), Some(10.0 + undo_s() + 2.0));

        // A pick's window too, the capsule's alone; one let go near its
        // end keeps a second.
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::SendRow(0), 1.0);
        assert!(flow.hover(true, 1.0 + undo_s() - 0.5).is_empty(), "it went");
        assert!(done(&flow, 20.0).undo);
        assert!(flow.hover(false, 20.0).is_empty());
        assert_eq!(done(&flow, 20.0).countdown, Some(20.0 + RESUME_LEFT_S));
        assert!(done(&flow, 20.0 + RESUME_LEFT_S - 0.01).undo);
        assert!(!done(&flow, 20.0 + RESUME_LEFT_S).undo);

        // One that starts under the pointer holds from the start, and an
        // Undo while it's held still counts and lets the router go.
        let mut flow = Flow::new();
        flow.hover(true, 0.0);
        assert!(
            flow.input(Input::Routed(rule()), 1.0)
                .contains(&Action::Hold(true))
        );
        assert_eq!(done(&flow, 9.0).held, Some(undo_s()));
        let actions = flow.input(Input::Undo, 9.0);
        assert_eq!(actions.first(), Some(&Action::Undo { to: "SEER".into() }));
        assert!(actions.contains(&Action::Hold(false)), "{actions:?}");
    }

    /// The pointer on a finished capsule holds it; leaving gives it a moment.
    /// A click on Undo at the window's edge still counts, and folding a list
    /// gives a brief hold rather than hiding it at once.
    #[test]
    fn hover_and_folding_hold_a_finished_capsule_and_a_late_undo_counts() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        flow.input(Input::Settled, 3.0);
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
        flow.input(Input::Settled, 3.0);
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
            site: Some("example.com".into()),
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
            // 1-3 pick; Escape stays the app's.
            assert_eq!(
                flow.keys(),
                Some(CapsuleKeys::Suggest {
                    cycle: false,
                    search: false
                }),
                "{how}"
            );
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

        // Even with nothing else to offer, the list comes back with the
        // session it was taken from, so Search can still send it.
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
        assert_eq!(rows(&flow, 1.2), ["SEER"]);
        assert!(done(&flow, 1.2).open.is_some());
        assert!(flow.keys().is_some());
        assert!(!flow.tick(1_000.0), "it waits for a pick");
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
        assert_eq!(list.rows[2].why, "taken back");
        assert_eq!(list.selected, 0);
        assert_eq!(
            flow.keys(),
            Some(crate::capture_host::CapsuleKeys::Suggest {
                cycle: false,
                search: false
            })
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
        // capsule leaves like any other, and Send to… still brings
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
        let waiting = None;
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
        assert_eq!((kept.text.as_str(), kept.why.as_deref()), ("Kept", None));
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

    /// A host that named a session by id (an old build, or a rule that only
    /// knew the id) still reads as a name in the pill, including "Already sent".
    #[test]
    fn a_session_named_by_its_id_reads_as_a_session() {
        let to = Dest::new("ses_e853b1bd3ffe68OoWKFGtjPy43", "rule")
            .session("ses_e853b1bd3ffe68OoWKFGtjPy43");
        let route = Route::Rule {
            to: to.clone(),
            suggested: vec![to],
            recent: Vec::new(),
            site: None,
        };
        assert_eq!(
            routes::sent_to("ses_e853b1bd3ffe68OoWKFGtjPy43"),
            "Sent to session"
        );
        let mut flow = Flow::new();
        flow.input(Input::Routed(route), 0.0);
        assert_eq!(
            done(&flow, 0.0).text,
            "Sending to session",
            "the rule's own send, before OpenCode takes it"
        );
        flow.input(Input::Settled, 0.5);
        assert_eq!(done(&flow, 0.5).text, "Sent to session");
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TooLate), 1.2);
        let shown = done(&flow, 1.2);
        assert_eq!(shown.text, "Already sent to session");
        assert_eq!(shown.why, None);
    }

    #[test]
    fn too_late_to_take_back_offers_the_list_for_a_copy() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TooLate), 1.2);
        let shown = done(&flow, 1.2);
        assert_eq!(shown.text, "Already sent to SEER");
        assert_eq!(shown.why, None);
        assert_eq!(rows(&flow, 1.2), ["HEX", "HOME", "SEER"]);
        let (_, list) = shown.open.unwrap();
        assert_eq!(list.rows[2].why, "already sent");
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
        assert_eq!(folded.text, "Already sent to SEER");
        assert_eq!(folded.why, None);
        assert_eq!(folded.ask, None, "it is where it went");
        flow.input(Input::Dismiss, 3.0);
        assert_eq!(flow.pres(3.0), Pres::Hidden);
        // It's where it went: once gone, nothing waits for a pick.
        assert_eq!(flow.keys(), None);

        // Folded, it leaves by itself, and Send to… stays the app's.
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
        assert_eq!(list.always, Some(("example.com".into(), false)));
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
    /// said it went: dismissed, Send to… brings it back.
    #[test]
    fn a_pick_taken_back_waits_for_a_pick_again() {
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
            assert_eq!(flow.keys(), None, "{dismiss:?}");
            flow.input(Input::OpenList, 100.0);
            assert!(done(&flow, 100.0).open.is_some(), "{dismiss:?}");
        }
    }

    /// Send to… bringing a taken-back capture back is silent: the
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

    /// A rule reads Sending until the router acknowledges its delivery.
    #[test]
    fn a_rule_reads_sending_and_sounds_only_when_it_went() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        let sent = done(&flow, 10.0);
        assert_eq!(sent.text, "Sending to SEER");
        assert_eq!(sent.lead, Lead::Check);
        assert!(flow.take_cues().is_empty(), "not until the router says so");
        flow.input(Input::Settled, 10.2);
        assert_eq!(done(&flow, 10.2).text, "Sent to SEER");
        assert_eq!(flow.take_cues(), [Cue::Sent]);
        flow.input(Input::Settled, 10.4);
        assert!(flow.take_cues().is_empty(), "once");
    }

    /// An Undo window never wakes the capsule to count: its ring drains on
    /// its own, so the capsule changes only when the window ends.
    #[test]
    fn an_undo_window_wakes_only_when_it_ends() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 10.0);
        flow.input(Input::Settled, 10.2);
        assert_eq!(flow.pres(10.5), flow.pres(10.0 + undo_s() - 0.5));
        assert_eq!(flow.next_due(10.5), Some(10.0 + undo_s()));
        assert_ne!(flow.pres(10.0), flow.pres(10.0 + undo_s()));

        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::SendRow(0), 1.0);
        assert_eq!(done(&flow, 1.5).countdown, Some(1.0 + undo_s()));
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
                always: Some("example.com".into())
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
                why: "OpenCode isn't running".into(),
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
                why: "OpenCode isn't running".into(),
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

        let mut flow = Flow::new();
        flow.input(Input::Routed(rule()), 0.0);
        assert_eq!(done(&flow, 1.0).text, "Sending to SEER");
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
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false
            }),
            "back to the suggestions it opened over"
        );
        assert!(flow.input(Input::ClickAway, 0.75).is_empty());
        assert_eq!(flow.keys(), None);
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
                Dest::new("HEX", "same page").session("ses_a"),
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
                pen: Pen::Off,
            }
        );
        assert_eq!(flow.click(Id::Mode(Mode::Screen), 0.5), []);
        assert_eq!(
            flow.pres(0.5),
            Pres::Recording {
                chips: false,
                phone: false,
                discarding: false,
                pen: Pen::Off,
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
                pen: Pen::Off,
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
                pen: Pen::Off,
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
        assert_eq!(asked.ask.as_deref(), Some("Search"));
        assert!(asked.ask_keys, "with the shortcut that opens it");
        assert!(!asked.hand_off, "trimmed to Search and ✕");
        let (_, suggestions) = asked.open.expect("a guess opens on its suggestions");
        assert!(suggestions.suggesting());
        flow.input(Input::OpenList, 1.0);
        let open = done(&flow, 1.0);
        let (query, list) = open.open.expect("open");
        assert_eq!(query.text, "");
        assert_eq!(list.selected_dest().unwrap().name, "SEER");
        assert_eq!(list.footer, Footer::Search { remember: true });
        assert_eq!(list.title, "All sessions");
        assert!(open.ask.is_none(), "the field takes the chips' place");
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
    fn a_guess_suggests_three_and_a_click_sends_nothing_before_it() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        let (_, list) = done(&flow, 0.0).open.unwrap();
        let names: Vec<_> = list.rows().map(|dest| dest.name.as_str()).collect();
        assert_eq!(names, ["SEER", "HEX", "HOME"], "suggested, then recent");
        assert_eq!(list.selected, 0, "the best is marked");
        assert_eq!(list.footer, Footer::Suggesting);
        assert_eq!(list.always, Some(("example.com".into(), false)));
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false
            })
        );
        assert!(flow.take_cues().is_empty(), "nothing sent, nothing heard");
        flow.tick(4.0);
        assert!(matches!(flow.pres(4.0), Pres::Done(_)), "no countdown runs");
        assert_eq!(
            flow.input(Input::SendRow(1), 5.0),
            [Action::Send {
                to: "HEX".into(),
                always: None
            }],
            "without Shift, a pick saves no rule"
        );
        assert_eq!(flow.take_cues(), [Cue::Sent], "a pick sounds as it goes");
        assert_eq!(done(&flow, 5.0).why, None);
    }

    #[test]
    fn the_capture_shortcut_searches_every_session_and_escape_comes_back() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        let (_, list) = done(&flow, 0.0).open.unwrap();
        assert_eq!(list.title, "Send to");
        let height = list.rows().count();
        assert!(
            flow.input(Input::Record, 1.0).is_empty(),
            "it records nothing"
        );
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::List),
            "the search takes typing"
        );
        flow.input(Input::Type("hom".into()), 1.5);
        let (query, list) = done(&flow, 1.5).open.unwrap();
        assert_eq!(query.text, "hom");
        assert_eq!(list.title, "All sessions");
        assert_eq!(
            list.rows().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["HOME"]
        );
        assert_eq!(list.min_rows, height, "fewer matches keep the height");
        assert!(flow.input(Input::Escape, 2.0).is_empty());
        let (query, list) = done(&flow, 2.0).open.unwrap();
        assert_eq!(query.text, "", "Escape goes back to the suggestions");
        assert!(list.suggesting());
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false
            })
        );
    }

    /// t1326: typing over the suggestions searched nothing; the letters went
    /// to the app in front.
    #[test]
    fn typing_over_the_suggestions_with_the_pointer_on_them_searches() {
        use crate::capture_host::CapsuleKeys;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.hover(true, 0.5);
        assert!(flow.pointer_moved(), "the pointer on them changes the keys");
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::Suggest {
                cycle: false,
                search: true
            })
        );
        for typed in ["h", "e", "x"] {
            flow.input(Input::Type(typed.into()), 1.0);
        }
        let (query, list) = done(&flow, 1.0).open.unwrap();
        assert_eq!(query.text, "hex");
        assert_eq!(query.range(), (3, 3), "the caret follows what was typed");
        assert_eq!(list.title, "All sessions");
        assert_eq!(
            list.rows().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["HEX"]
        );
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::List),
            "the search takes typing"
        );
        flow.input(Input::Escape, 1.5);
        let (query, list) = done(&flow, 1.5).open.unwrap();
        assert_eq!(query.text, "", "Escape goes back to the suggestions");
        assert!(list.suggesting());
    }

    /// Typing is the app's unless the pointer moved onto the suggestions
    /// since they last changed: a list that opens under a resting pointer
    /// takes nothing.
    #[test]
    fn typing_stays_the_apps_until_the_pointer_moves_onto_the_suggestions() {
        use crate::capture_host::CapsuleKeys;
        let quiet = Some(CapsuleKeys::Suggest {
            cycle: false,
            search: false,
        });
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        assert_eq!(flow.keys(), quiet, "the pointer elsewhere");
        assert!(
            !flow.pointer_moved(),
            "a move off the capsule changes nothing"
        );
        flow.hover(true, 0.5);
        assert_eq!(flow.keys(), quiet, "opened under a resting pointer");
        flow.pointer_moved();
        assert_ne!(flow.keys(), quiet);
        flow.hover(false, 1.0);
        assert_eq!(flow.keys(), quiet, "the pointer left");
        flow.hover(true, 1.5);
        flow.pointer_moved();
        flow.input(Input::Routed(guess()), 2.0);
        assert_eq!(flow.keys(), quiet, "a new answer under a resting pointer");
        flow.pointer_moved();
        assert_ne!(flow.keys(), quiet);
    }

    fn sessions(dests: &[(&str, &str, &str)]) -> Route {
        Route::Guess {
            suggested: dests
                .iter()
                .map(|&(name, folder, id)| {
                    Dest::new(name, "1 h ago")
                        .session(id)
                        .folder(Some(folder.into()))
                })
                .collect(),
            recent: Vec::new(),
            site: None,
        }
    }

    fn search(route: Route, query: &str) -> Vec<(String, String)> {
        let mut flow = Flow::new();
        flow.input(Input::Routed(route), 0.0);
        flow.input(Input::OpenList, 0.5);
        flow.input(Input::Type(query.into()), 1.0);
        let (_, list) = done(&flow, 1.0).open.unwrap();
        list.rows()
            .map(|dest| (dest.name.clone(), dest.why.clone()))
            .collect()
    }

    #[test]
    fn the_search_matches_every_word_in_the_title_folder_or_id() {
        let route = || {
            sessions(&[
                (
                    "Annotate type to search",
                    "annotate-type-to-search",
                    "ses_1",
                ),
                ("Fix the capture tap", "hex", "ses_2"),
                ("Hex release notes", "docs", "ses_3"),
            ])
        };
        let names = |query| -> Vec<String> {
            search(route(), query)
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };
        assert_eq!(
            names("search ANNOTATE"),
            ["Annotate type to search"],
            "any order, any case"
        );
        assert_eq!(
            search(route(), "hex"),
            [
                ("Hex release notes".into(), "1 h ago".into()),
                ("Fix the capture tap".into(), "hex · 1 h ago".into()),
            ],
            "titles first, and a folder match names its folder"
        );
        assert_eq!(
            names("capture hex"),
            ["Fix the capture tap"],
            "title and folder"
        );
        assert_eq!(names("ses_3"), ["Hex release notes"], "the id");
        assert!(names("hex search").is_empty(), "every word, in one session");
    }

    #[test]
    fn a_row_names_its_folder_only_beside_a_row_with_the_same_title() {
        let route = sessions(&[
            ("HOME", "home", "ses_1"),
            ("Hex", "hex", "ses_2"),
            ("home", "dotfiles", "ses_3"),
        ]);
        let mut flow = Flow::new();
        flow.input(Input::Routed(route), 0.0);
        let (_, list) = done(&flow, 0.0).open.unwrap();
        let notes: Vec<_> = list.rows().map(|dest| dest.why.as_str()).collect();
        assert_eq!(notes, ["home · 1 h ago", "1 h ago", "dotfiles · 1 h ago"]);
    }

    #[test]
    fn shift_shows_the_rule_and_a_shift_pick_saves_it() {
        use super::super::surface::Id;
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::Shift(true), 0.5);
        let (_, list) = done(&flow, 0.5).open.unwrap();
        assert_eq!(list.always, Some(("example.com".into(), true)));
        assert!(
            list.rows().all(|dest| dest.why != "always"),
            "over the suggestions a click picks: off the rows, nowhere yet"
        );
        assert!(flow.point(1, true));
        let (_, list) = done(&flow, 0.6).open.unwrap();
        let notes: Vec<_> = list.rows().map(|dest| dest.why.as_str()).collect();
        assert_eq!(
            notes[1], "always",
            "the row under the pointer is where it goes"
        );
        assert_ne!(notes[0], "always");
        assert_eq!(
            list.target_dest().map(|dest| dest.name.as_str()),
            Some("HEX")
        );
        flow.input(Input::Shift(false), 1.0);
        let (_, list) = done(&flow, 1.0).open.unwrap();
        assert_eq!(list.always, Some(("example.com".into(), false)));
        assert!(
            list.rows().all(|dest| dest.why != "always"),
            "released, as before"
        );
        flow.input(Input::Shift(true), 1.5);
        assert_eq!(
            flow.input(Input::SendRow(1), 2.0),
            [Action::Send {
                to: "HEX".into(),
                always: Some("example.com".into())
            }]
        );
        let sent = done(&flow, 2.0);
        assert_eq!(sent.text, "Sent to HEX");
        assert_eq!(sent.why.as_deref(), Some("always"));
        assert!(sent.undo);
        assert_eq!(
            flow.click(Id::Undo, 2.5),
            [Action::Undo { to: "HEX".into() }],
            "Undo takes back the send, and the router the rule with it"
        );
        // A Shift click saves it too.
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::Shift(true), 0.5);
        assert_eq!(
            flow.click(Id::Row(0), 1.0),
            [Action::Send {
                to: "SEER".into(),
                always: Some("example.com".into())
            }]
        );
    }

    #[test]
    fn shift_follows_the_pointer_and_the_arrows_go_on_from_it() {
        let names = |flow: &Flow| -> Vec<String> {
            let (_, list) = done(flow, 1.0).open.unwrap();
            list.rows()
                .filter(|dest| dest.why == "always")
                .map(|dest| dest.name.clone())
                .collect()
        };
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::Shift(true), 0.1);
        flow.point(0, true);
        assert_eq!(names(&flow), ["SEER"]);
        // Onto the next row: the pointer reaches it before it leaves the
        // last one.
        flow.point(2, true);
        assert!(!flow.point(0, false), "leaving a row it already left");
        assert_eq!(names(&flow), ["HOME"]);
        assert!(flow.point(2, false));
        assert!(names(&flow).is_empty(), "off the rows");
        // A Shift click on the pointed row sends there and saves it.
        flow.point(2, true);
        assert_eq!(
            flow.click(super::super::surface::Id::Row(2), 0.5),
            [Action::Send {
                to: "HOME".into(),
                always: Some("example.com".into())
            }]
        );
        // In the search the highlight follows the pointer, and the arrows
        // and ⇧⏎ go on from it.
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.point(1, true);
        flow.input(Input::OpenList, 0.1);
        flow.input(Input::Shift(true), 0.2);
        assert_eq!(
            names(&flow),
            ["HEX"],
            "the search starts on the pointed row"
        );
        flow.point(2, true);
        assert_eq!(names(&flow), ["HOME"]);
        flow.input(Input::Move(-1), 0.3);
        assert_eq!(names(&flow), ["HEX"], "the arrows take over");
        assert_eq!(
            flow.input(Input::Send { always: true }, 0.4),
            [Action::Send {
                to: "HEX".into(),
                always: Some("example.com".into())
            }]
        );
    }

    #[test]
    fn a_click_away_folds_the_suggestions_and_they_leave_like_any_capsule() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        // Escape is never Hex's here: it changes nothing.
        assert!(flow.input(Input::Escape, 0.5).is_empty());
        assert!(done(&flow, 0.5).open.is_some());
        flow.input(Input::ClickAway, 1.0);
        let folded = done(&flow, 1.0);
        assert_eq!(folded.open, None);
        assert_eq!(folded.ask.as_deref(), Some("Send to…"));
        assert_eq!(flow.keys(), None);
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
                always: Some("example.com".into())
            }]
        );
        let shown = done(&flow, 1.0);
        assert_eq!(shown.why.as_deref(), Some("always"));
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
                why: "OpenCode isn't running".into(),
            }),
            closed,
        );
        assert_eq!(flow.pres(closed), Pres::Hidden);
        // The next capture shows as ever.
        flow.input(Input::Stage(Stage::Done), closed + 1.0);
        flow.input(Input::Routed(guess()), closed + 2.0);
        assert!(matches!(flow.pres(closed + 2.0), Pres::Done(_)));
    }

    /// Send to… after a guess opens the list while the capsule shows, and
    /// brings it back after it closed with nothing picked, until something
    /// settles the capture. No key is Hex's while it's closed.
    #[test]
    fn send_to_brings_back_a_capture_waiting_for_a_pick() {
        use crate::capture_host::CapsuleKeys;
        let hold = tuning::get().done_hold_s;
        let mut flow = Flow::new();
        flow.input(Input::Stage(Stage::Done), 0.0);
        flow.input(Input::Routed(guess()), 0.0);
        assert_eq!(
            flow.keys(),
            Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false
            })
        );
        // Its hold runs out: hidden, and no key is the capsule's.
        assert!(flow.tick(hold + 1.0));
        assert_eq!(flow.pres(hold + 1.0), Pres::Hidden);
        assert_eq!(flow.keys(), None);
        assert!(flow.input(Input::OpenList, 30.0).is_empty());
        let (query, list) = done(&flow, 30.0).open.expect("the list is open");
        assert_eq!(query.text, "");
        assert_eq!(list.selected_dest().unwrap().name, "SEER");
        assert_eq!(flow.keys(), Some(CapsuleKeys::List));
        // Escape folds the list; it then leaves with its hold and waits again.
        flow.input(Input::Escape, 31.0);
        assert!(matches!(flow.pres(32.0), Pres::Done(_)));
        flow.tick(31.0 + hold + 1.0);
        assert_eq!(flow.pres(32.0 + hold), Pres::Hidden);
        flow.input(Input::OpenList, 40.0);
        assert!(done(&flow, 40.0).open.is_some());
        // A pick settles it: no key is Hex's, Escape goes to the app.
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
        assert_eq!(flow.keys(), None);
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
        flow.input(Input::Settled, 8.0);
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
                always: Some("example.com".into())
            }]
        );
        assert_eq!(done(&flow, 1.0).why.as_deref(), Some("always"));

        // And in the list an Undo of the rule's send reopens.
        let mut flow = Flow::new();
        flow.input(Input::Routed(rule_of_three()), 0.0);
        flow.input(Input::Undo, 1.0);
        flow.input(Input::Undone(Undone::TakenBack), 1.2);
        assert_eq!(
            flow.input(Input::Send { always: true }, 2.0),
            [Action::Send {
                to: "ses_hex".into(),
                always: Some("example.com".into())
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
        assert_eq!(flow.take_cues(), [Cue::Discarded]);
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
        assert_eq!(shown.text, "");
        assert_eq!(shown.why.as_deref(), Some("no speech"));
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
        assert!(matches!(list.footer, Footer::Search { .. }));
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
        assert_eq!(shown.why.as_deref(), Some("no suggestions"));
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
        assert_eq!(shown.why.as_deref(), Some("OpenCode didn't answer"));
        assert_eq!(shown.ask, None);
        assert!(shown.hand_off);
        // Nothing to list, and a stray OpenList opens
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
        assert!(flow.cues.iter().all(|cue| *cue != Cue::Discarded));

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
        assert_eq!(done(&flow, 10.0).text, "Couldn't start");
        assert_eq!(done(&flow, 10.0).why.as_deref(), Some("the window closed"));
        flow.input(Input::Failed(Failure::Save, None), 11.0);
        assert_eq!(done(&flow, 11.0).text, "Couldn't save");
        flow.input(
            Input::Failed(Failure::Send("SEER".into()), Some("offline".into())),
            12.0,
        );
        assert_eq!(done(&flow, 12.0).text, "Couldn't send to SEER");
        assert_eq!(done(&flow, 12.0).why.as_deref(), Some("offline"));
    }

    #[test]
    fn the_permission_helper_asks_follows_the_grant_and_records_once_ready() {
        use super::super::surface::Id;
        let screen = CaptureNeed::ScreenRecording;
        let mut flow = Flow::new();
        // News of a grant with no helper up shows nothing.
        flow.input(Input::Permission(screen, Grant::Ready), 0.5);
        assert_eq!(flow.pres(0.5), Pres::Hidden);

        flow.input(Input::NeedsPermission(screen, AskedBy::Capture), 1.0);
        let shown = done(&flow, 1.0);
        assert_eq!(shown.lead, Lead::Warning);
        assert_eq!(shown.text, "Hex needs Screen Recording");
        assert_eq!(shown.why.as_deref(), Some("to see your screen"));
        assert_eq!(shown.ask.as_deref(), Some("Allow"));
        assert_eq!(flow.keys(), None);

        assert_eq!(flow.click(Id::Ask, 2.0), vec![Action::Allow(screen)]);
        // Still off: it waits on System Settings, and the shortcut again
        // leaves it as it stands.
        flow.input(Input::Permission(screen, Grant::Off), 3.0);
        flow.input(Input::NeedsPermission(screen, AskedBy::Capture), 4.0);
        let waiting = done(&flow, 4.0);
        assert_eq!(
            waiting.why.as_deref(),
            Some("turn Hex on in System Settings")
        );
        // It waits a good while, and never folds under the pointer.
        flow.tick(60.0);
        assert_ne!(flow.pres(60.0), Pres::Hidden);

        flow.input(Input::Permission(screen, Grant::Restart), 61.0);
        let restart = done(&flow, 61.0);
        assert_eq!(restart.text, "Screen Recording is on");
        assert_eq!(restart.ask.as_deref(), Some("Restart Hex"));
        assert_eq!(flow.click(Id::Ask, 62.0), vec![Action::Restart]);

        flow.input(Input::Permission(screen, Grant::Ready), 63.0);
        let ready = done(&flow, 63.0);
        assert_eq!(ready.lead, Lead::Check);
        assert!(ready.ask_keys);
        assert_eq!(
            flow.click(Id::Ask, 64.0),
            vec![Action::Record(Mode::Screen)]
        );
        assert_eq!(flow.pres(64.0), Pres::Hidden);

        // Unanswered, it folds; ✕ closes it at once.
        flow.input(
            Input::NeedsPermission(CaptureNeed::Microphone, AskedBy::Capture),
            70.0,
        );
        assert_eq!(done(&flow, 70.0).text, "Hex needs Microphone");
        assert!(flow.tick(70.0 + PERMISSION_HOLD_S));
        assert_eq!(flow.pres(70.0 + PERMISSION_HOLD_S), Pres::Hidden);
        flow.input(Input::NeedsPermission(screen, AskedBy::Capture), 300.0);
        flow.click(Id::Close, 301.0);
        assert_eq!(flow.pres(301.0), Pres::Hidden);
        // A capture that starts takes its place.
        flow.input(Input::NeedsPermission(screen, AskedBy::Capture), 302.0);
        flow.input(Input::Stage(Stage::Recording), 303.0);
        assert!(matches!(flow.pres(303.0), Pres::Recording { .. }));
    }

    /// Labs › Screenshots' permission helper returns to its picker in the
    /// mode its shortcut asked for, never to Annotate's whole-screen
    /// recording. Annotate's own helper still records.
    #[test]
    fn a_granted_screenshots_permission_reopens_its_picker() {
        use super::super::surface::Id;
        let screen = CaptureNeed::ScreenRecording;
        for (record, action) in [
            (true, Action::OpenScreenshots { record: true }),
            (false, Action::OpenScreenshots { record: false }),
        ] {
            let mut flow = Flow::new();
            flow.input(
                Input::NeedsPermission(screen, AskedBy::Screenshots { record }),
                1.0,
            );
            assert_eq!(done(&flow, 1.0).text, "Hex needs Screen Recording");
            flow.input(Input::Permission(screen, Grant::Ready), 2.0);
            assert_eq!(flow.click(Id::Ask, 3.0), vec![action]);
            assert_eq!(flow.pres(3.0), Pres::Hidden, "the helper folds");
        }

        // The newest ask is what the grant returns to: Annotate asked
        // first, then the record shortcut.
        let mut flow = Flow::new();
        flow.input(Input::NeedsPermission(screen, AskedBy::Capture), 1.0);
        flow.input(
            Input::NeedsPermission(screen, AskedBy::Screenshots { record: true }),
            2.0,
        );
        flow.input(Input::Permission(screen, Grant::Ready), 3.0);
        assert_eq!(
            flow.click(Id::Ask, 4.0),
            vec![Action::OpenScreenshots { record: true }]
        );
        // The other way round, Annotate records its own capture.
        let mut flow = Flow::new();
        flow.input(
            Input::NeedsPermission(screen, AskedBy::Screenshots { record: false }),
            1.0,
        );
        flow.input(Input::NeedsPermission(screen, AskedBy::Capture), 2.0);
        flow.input(Input::Permission(screen, Grant::Ready), 3.0);
        assert_eq!(flow.click(Id::Ask, 4.0), vec![Action::Record(Mode::Screen)]);
    }
}
