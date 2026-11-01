//! The onboarding options as playground stories (the `playground`
//! experiment, ⇧⌘D, group Onboarding). Each plays itself on open and on R,
//! and in a live desktop window pauses on `"Your turn"` with a focused
//! [`rune::TextEdit`] so you can hold `⌥` and test real voice dictation live.

mod side;

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rune::{Cx, El, Play, Playground, Story, TextEdit, TextStyle, Tx, canvas, col, el, row, text};

use super::super::controls::Paint;
use super::cards::{self, Caption, Side, TAP_BOX, bubble};
use super::dictated::Dictated;
use super::fixture::{ANSWER_TRY, Download, EXAMPLE_REPLIES, Language, Speech, Take, downloaded};
use super::hex_meter::BUILD_SECS;
use super::hud_stage::{FlowHud, LiveHud};
use super::live::Live;
use super::scene_script::Script;
use super::sfx::SfxTrack;
use super::typing::Glyph;
use super::{flows, permissions, typing, versus};
use crate::desktop::onboarding::Practice;
use crate::feedback::KeySoundStyle;

static LIVE_WINDOW: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_live_window(live: bool) {
    LIVE_WINDOW.store(live, Ordering::Relaxed);
}

pub(crate) fn is_live_window() -> bool {
    LIVE_WINDOW.load(Ordering::Relaxed)
}

const DOWNLOAD_SECS: f64 = 9.0;
const FIRST_RUN_SECS: f64 = 23.0;
const SLOW_SECS: f64 = 31.0;
const DOWNLOAD_HOLD: f64 = 2.8;
const PERMISSIONS_WAIT: f64 = 0.6;
const CLOSE_SECS: f64 = 0.65;
const WORD_SECS: f64 = 0.22;
const EXAMPLE_START: f64 = 0.6;
const EXAMPLE_GAP: f64 = 0.5;
// Ask for the next display frame, including 120 Hz displays. Rune paces a
// hidden or unpaced window itself rather than spinning on a zero deadline.
pub(super) const FRAME: Duration = Duration::ZERO;
const SIDE_W: f32 = 240.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Pill,
    Fill,
    Shout,
    Versus,
    FirstRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Step {
    Pick,
    Started,
    Parked,
    YourTurn,
    Hearing,
    Heard,
    Done,
    Microphone,
    Waiting,
    Try,
    TryHearing,
    TryHeard,
    Accessibility,
    Ready,
}

pub(crate) struct Onboard {
    pub(crate) flow: Flow,
    pub(crate) speech: Speech,
    pub(crate) wants: Language,
    pub(crate) caps: Vec<String>,
    shortcut_index: usize,
    pub(crate) step: Step,
    pub(crate) language: Option<Language>,
    pub(crate) now: f64,
    pub(crate) reduced: Cell<bool>,
    pub(crate) motion: super::landing::Motion,
    download_from: Option<f64>,
    pub(crate) download: Option<Download>,
    pub(crate) parked_from: Option<f64>,
    pub(crate) typing_from: Option<f64>,
    pub(crate) voice_from: Option<f64>,
    /// When the resting HUD plate first showed on `"With Hex"` or
    /// `"Your turn"`: the HUD unfolds from here and keeps running through the
    /// take, so pressing the shortcut never restarts it.
    pub(crate) hud_from: Option<f64>,
    pub(crate) take_from: Option<f64>,
    /// How many `"Without Hex"` bubbles have flown out of the field so far.
    sent_seen: usize,
    /// Whether `"With Hex"`'s reply has flown out of the field.
    hex_seen: bool,
    /// Every pause and the key pace of `"Without Hex"`, tunable live.
    pub(crate) timing: typing::Timing,
    /// `"Without Hex"` as [`Self::timing`] times it.
    pub(crate) scene: typing::Scene,
    pub(crate) partial: &'static str,
    pub(crate) take: &'static Take,
    pub(crate) meter: (f32, f32),
    examples_from: Option<f64>,
    closed_from: Option<f64>,
    pub(crate) closed: f32,
    download_secs: f64,
    pub(crate) had_model: bool,
    /// Both permissions are on.
    pub(crate) granted: bool,
    /// The microphone is on.
    pub(crate) mic_granted: bool,
    /// The practice game has started this run.
    played: bool,
    pub(crate) scripted_try: bool,
    pub(crate) permissions_from: Option<f64>,
    ready_from: Option<f64>,
    manual_override: bool,
    auto_due_at: Option<f64>,
    pub(crate) practice: TextEdit,
    pub(crate) live_hold_from: Option<f64>,
    /// The `"Your turn"` plate, on the microphone's real level.
    pub(crate) hud: LiveHud,
    pub(crate) flow_hud: FlowHud,
    /// A dictation's text landed since the plate last stepped.
    pub(crate) hud_landed: bool,
    pub(crate) sfx: SfxTrack,
    /// Set when this is Hex's real first run (Labs):
    /// what the window's tour reports, in place of the fixtures.
    pub(crate) live: Option<Live>,
    /// The dictated words flying into the practice field.
    pub(crate) dictated: Dictated,
    /// A story's stand-in for a real dictation on the interactive try.
    pub(crate) rehearsal: Option<Rehearsal>,
    /// The interactive try as the practice game (`game`).
    pub(crate) game: Option<super::game::Session>,
    /// The interactive try as the plain practice field, not the game.
    pub(crate) practice_field: bool,
    /// Presses too short to dictate on the practice, for its coaching.
    pub(crate) taps: u8,
    /// The shortcut starts and stops dictation instead of being held.
    pub(crate) toggle: bool,
    /// A story that plays the whole first run by itself: it presses
    /// `"Watch what happens"`, and its scripted player ends the game.
    pub(crate) autoplay: bool,
}

/// A story's dictation of [`ANSWER_TRY`] on the interactive `"Your turn"`,
/// through the same live words and paste as Hex's real one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rehearsal {
    /// Whether the model sends live words while the shortcut is held.
    pub(crate) streams: bool,
    from: f64,
    pasted: bool,
}

impl Rehearsal {
    /// How long after `"Your turn"` shows the shortcut goes down.
    const LEAD: f64 = 1.0;
    /// How long the paste takes after the shortcut comes up.
    const TRANSCRIBE: f64 = 0.45;

    pub(crate) fn new(streams: bool) -> Self {
        Self {
            streams,
            from: f64::INFINITY,
            pasted: false,
        }
    }

    fn elapsed(&self, now: f64) -> f64 {
        now - self.from - Self::LEAD
    }

    fn held(&self, now: f64) -> bool {
        let t = self.elapsed(now);
        (0.0..ANSWER_TRY.release).contains(&t)
    }

    fn partial(&self, now: f64) -> Option<crate::transcription::engine::Partial> {
        (self.streams && self.held(now)).then(|| crate::transcription::engine::Partial {
            stable: ANSWER_TRY.partial(self.elapsed(now)).to_owned(),
            tentative: String::new(),
        })
    }

    fn paste_due(&mut self, now: f64) -> bool {
        let due = !self.pasted && self.elapsed(now) >= ANSWER_TRY.release + Self::TRANSCRIBE;
        self.pasted |= due;
        due
    }
}

impl Drop for Onboard {
    fn drop(&mut self) {
        self.sfx.flush_offline_if_requested(self.now.max(1.0));
    }
}

const PRESET_SHORTCUTS: &[&[&str]] = &[&["⌥"], &["fn"], &["⌃", "Space"], &["R⌘"]];

impl Onboard {
    pub(super) fn new(flow: Flow) -> Self {
        Self {
            flow,
            speech: Speech::Apple,
            wants: Language::English,
            caps: crate::app_settings::AppSettings::default()
                .dictation_hotkey
                .keycaps(),
            shortcut_index: 0,
            step: Step::Pick,
            language: None,
            now: 0.0,
            reduced: Cell::new(false),
            motion: super::landing::Motion::default(),
            download_from: None,
            download: None,
            parked_from: None,
            typing_from: None,
            voice_from: None,
            hud_from: None,
            take_from: None,
            sent_seen: 0,
            hex_seen: false,
            timing: typing::Timing::default(),
            scene: typing::Scene::default(),
            partial: "",
            take: Script::chosen().take(),
            meter: (0.0, 0.0),
            examples_from: None,
            closed_from: None,
            closed: 0.0,
            download_secs: if flow == Flow::FirstRun {
                FIRST_RUN_SECS
            } else {
                DOWNLOAD_SECS
            },
            had_model: false,
            granted: false,
            mic_granted: false,
            played: false,
            scripted_try: false,
            permissions_from: None,
            ready_from: None,
            manual_override: false,
            auto_due_at: None,
            practice: TextEdit::default(),
            live_hold_from: None,
            hud: LiveHud::default(),
            flow_hud: FlowHud::default(),
            hud_landed: false,
            sfx: SfxTrack::default(),
            live: None,
            dictated: Dictated::default(),
            rehearsal: None,
            game: None,
            practice_field: false,
            taps: 0,
            toggle: false,
            autoplay: false,
        }
    }

    /// True when `"Your turn"` should wait for real voice dictation / typing
    /// rather than auto-playing the scripted reply.
    pub(crate) fn interactive_try(&self) -> bool {
        self.flow == Flow::FirstRun
            && (self.live.is_some() || is_live_window() || self.rehearsal.is_some())
            && !self.scripted_try
    }

    pub(crate) fn pick(&mut self, language: Language) {
        if self.step != Step::Pick {
            return;
        }
        self.language = Some(language);
        self.step = Step::Started;
        self.download_from = Some(self.now);
        self.download = Some(Download {
            language,
            downloaded: 0,
            total: language.bytes(),
        });
    }

    fn ensure_download(&mut self, finished: bool) {
        let lang = self.language.unwrap_or(self.wants);
        self.language = Some(lang);
        let d = self.download.get_or_insert(Download {
            language: lang,
            downloaded: 0,
            total: lang.bytes(),
        });
        if finished {
            d.downloaded = d.total;
            self.download_from = None;
            self.closed = 1.0;
            self.closed_from = None;
        } else if self.download_from.is_none() {
            self.download_from = Some(self.now);
        }
    }

    /// Toggles the inline language capsule button (`English` ↔ `French`) on
    /// `"Setting up Hex"`.
    pub(crate) fn cycle_language(&mut self) {
        let next = match self.language.unwrap_or(self.wants) {
            Language::English => Language::Other,
            Language::Other => Language::English,
        };
        self.wants = next;
        self.language = Some(next);
        if let Some(d) = self.download.as_mut() {
            let ratio = d.fraction();
            d.language = next;
            d.total = next.bytes();
            d.downloaded = (d.total as f64 * f64::from(ratio)).round() as u64;
        }
        self.sfx.on_capsule_tap();
    }

    /// Cycles the shortcut capsule on `"Your turn"` so clicking it previews
    /// changing the dictation key.
    pub(crate) fn cycle_shortcut(&mut self) {
        self.shortcut_index = (self.shortcut_index + 1) % PRESET_SHORTCUTS.len();
        self.caps = PRESET_SHORTCUTS[self.shortcut_index]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        self.sfx.on_capsule_tap();
    }

    pub(crate) fn choose_key_sound(&mut self, style: KeySoundStyle) {
        self.sfx.set_style_and_preview(style, self.now);
    }

    /// Called when the interactive `"Your turn"` [`TextArea`] changes (e.g. a
    /// real dictation paste from Hex lands or the user types).
    pub(crate) fn on_practice_change(&mut self, _: &mut Tx) {
        if super::game::changed(self) {
            return;
        }
        self.dictated.changed(&self.practice.text, self.now);
        let has_words = !self.practice.text.trim().is_empty();
        self.hud_landed |= has_words;
        if has_words && self.step == Step::Try {
            self.step = Step::TryHeard;
            self.sfx.on_live_dictation_landed(self.now);
        } else if !has_words && self.step == Step::TryHeard {
            self.step = Step::Try;
        }
    }

    /// Clicking `Continue →` immediately advances to the next step.
    pub(crate) fn press_continue(&mut self) {
        self.manual_override = true;
        self.auto_due_at = None;
        self.sfx.on_capsule_tap();
        match self.step {
            // The permission steps move on as the story does, so the model
            // and the permissions still to come decide the next page.
            Step::Microphone | Step::Accessibility => {
                self.auto_due_at = Some(self.now);
            }
            Step::Done => self.jump_to_ready(),
            Step::TryHeard => self.after_game(),
            _ => {}
        }
    }

    /// Leaving the practice: Accessibility, if it's still to ask for, then
    /// `"Hex is ready"`.
    pub(crate) fn after_game(&mut self) {
        if self.granted {
            self.jump_to_ready();
        } else {
            self.jump_to_accessibility();
        }
    }

    /// Leaves the practice as Continue would, but keeps the soundtrack so
    /// far, for a story that plays itself.
    pub(super) fn leave_game_keeping_sound(&mut self, now: f64) {
        self.manual_override = true;
        self.auto_due_at = if self.granted {
            self.jump_to_ready();
            None
        } else {
            to_accessibility(self, now).map(|secs| now + secs)
        };
    }

    pub(crate) fn jump_to_download(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.practice.focused = false;
        self.live_hold_from = None;
        self.step = Step::Started;
        let lang = self.wants;
        self.language = Some(lang);
        self.download_from = Some(self.now);
        self.closed_from = None;
        self.closed = 0.0;
        self.download = Some(Download {
            language: lang,
            downloaded: 0,
            total: lang.bytes(),
        });
        self.sfx.on_download_start(self.now);
        self.auto_due_at = Some(self.now + DOWNLOAD_HOLD);
    }

    pub(crate) fn jump_to_demo(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.practice.focused = false;
        self.live_hold_from = None;
        self.ensure_download(false);
        self.voice_from = None;
        self.hud_from = None;
        self.take_from = None;
        self.partial = "";
        self.auto_due_at = None;
        park(self, self.now);
    }

    /// `"Watch what happens"`: plays `"Without Hex"` from the top, then the
    /// story moves on by itself.
    /// `Skip →` or Escape: leaves the comedy for whatever follows it. True
    /// when there was one to skip.
    pub(crate) fn skip_scene(&mut self) -> bool {
        if !versus::skippable(self) {
            return false;
        }
        self.manual_override = true;
        self.sfx.clear();
        self.sfx.on_capsule_tap();
        let now = self.now;
        self.auto_due_at = if self.flow == Flow::FirstRun {
            leave_comedy(self, now)
        } else {
            self.step = Step::Done;
            next_beat(self, now)
        }
        .map(|secs| now + secs);
        true
    }

    pub(crate) fn start_scene(&mut self) {
        if self.step != Step::Parked || self.typing_from.is_some() {
            return;
        }
        self.manual_override = true;
        self.typing_from = Some(self.now);
        self.sfx.on_without_hex(self.now, &self.scene);
        self.auto_due_at = Some(self.now + self.scene.duration());
    }

    /// Rebuilds [`Self::scene`] after a change to [`Self::timing`].
    fn retime(&mut self) {
        self.scene = typing::Scene::build(&self.timing);
        if self.step == Step::Parked && self.typing_from.is_some() {
            // A scene already playing starts over, waiting for the button.
            self.typing_from = None;
            self.auto_due_at = None;
            self.sfx.clear();
        }
    }

    pub(crate) fn jump_to_microphone(&mut self) {
        self.leave_for_permission();
        let now = self.now;
        self.auto_due_at = to_microphone(self, now).map(|secs| now + secs);
    }

    pub(crate) fn jump_to_accessibility(&mut self) {
        self.leave_for_permission();
        let now = self.now;
        self.auto_due_at = to_accessibility(self, now).map(|secs| now + secs);
    }

    fn leave_for_permission(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.practice.focused = false;
        self.live_hold_from = None;
        self.ensure_download(false);
    }

    /// A real first run resumed while macOS still has a permission to ask.
    pub(super) fn resume_setup(&mut self) {
        if self.mic_granted {
            self.jump_to_accessibility();
        } else {
            self.jump_to_microphone();
        }
    }

    pub(crate) fn jump_to_try(&mut self) {
        self.manual_override = true;
        self.auto_due_at = None;
        self.sfx.clear();
        self.ensure_download(true);
        self.take_from = None;
        self.hud_from = Some(self.now);
        self.partial = "";
        self.live_hold_from = None;
        self.practice = TextEdit::default();
        self.practice.focused = self.interactive_try();
        self.dictated.reset();
        self.game = None;
        self.played = true;
        if let Some(rehearsal) = self.rehearsal.as_mut() {
            *rehearsal = Rehearsal::new(rehearsal.streams);
            rehearsal.from = self.now;
        }
        self.step = Step::Try;
        if !self.interactive_try() {
            self.auto_due_at = Some(self.now + 1.2);
        }
    }

    /// The real first run sets these as the story would at its start.
    pub(super) fn begin_live(&mut self, now: f64) {
        self.manual_override = true;
        self.auto_due_at = begin_first_run(self, now).map(|secs| now + secs);
    }

    /// The real first run's `"Almost ready"`, for a run resumed past the
    /// permissions while the model still downloads.
    /// The story's `"Almost ready"`, with the download restarted so it
    /// shows a while before the game.
    pub(crate) fn jump_to_download_wait(&mut self) {
        self.jump_to_download();
        self.step = Step::Waiting;
        self.sfx.clear();
        self.sfx.on_almost_ready(self.now);
        self.auto_due_at = Some(self.now);
    }

    pub(super) fn jump_to_waiting(&mut self) {
        self.manual_override = true;
        self.sfx.clear();
        self.ensure_download(false);
        self.step = Step::Waiting;
        self.auto_due_at = Some(self.now);
    }

    pub(crate) fn jump_to_ready(&mut self) {
        self.manual_override = true;
        self.auto_due_at = None;
        self.practice.focused = false;
        self.live_hold_from = None;
        self.ensure_download(true);
        self.step = Step::Ready;
        self.ready_from = Some(self.now);
        self.sfx.on_ready(self.now, self.reduced.get());
    }

    /// Seconds since `"Hex is ready"` appeared; a scene opened on it shows
    /// it settled.
    pub(crate) fn ready_age(&self) -> f64 {
        self.ready_from
            .map_or(f64::INFINITY, |from| self.now - from)
    }

    pub(crate) fn downloaded(&self) -> bool {
        self.download.is_some_and(Download::done)
    }

    pub(crate) fn listening_peak(&self) -> Option<f32> {
        if self.live_hold_from.is_some() {
            return Some(crate::audio::input_level::recent().unwrap_or(0.0));
        }
        matches!(self.step, Step::Hearing | Step::TryHearing).then_some(self.meter.1)
    }

    pub(crate) fn demo_hears(&self) -> bool {
        self.flow == Flow::FirstRun || self.can_hear()
    }

    pub(crate) fn can_hear(&self) -> bool {
        self.speech == Speech::Apple || self.downloaded()
    }

    pub(crate) fn streams(&self) -> bool {
        self.speech == Speech::Apple || self.language.is_some_and(Language::streams)
    }

    pub(crate) fn example(&self, index: usize) -> (&'static str, bool) {
        let Some(from) = self.examples_from else {
            return ("", false);
        };
        let mut start = from + EXAMPLE_START;
        for reply in &EXAMPLE_REPLIES[..index] {
            start += reply.split_whitespace().count() as f64 * WORD_SECS + EXAMPLE_GAP;
        }
        let reply = EXAMPLE_REPLIES[index];
        let count = reply.split_whitespace().count();
        let shown = (((self.now - start) / WORD_SECS).floor().max(0.0) as usize).min(count);
        let end = reply
            .split_whitespace()
            .take(shown)
            .map(str::len)
            .sum::<usize>()
            + shown.saturating_sub(1);
        (
            &reply[..end],
            shown == count && self.now - start >= count as f64 * WORD_SECS + 0.3,
        )
    }
}

pub(crate) fn add<S: 'static>(playground: Playground<S>) -> Playground<S> {
    playground
        .story(story("Prototypes/Onboarding/Option · Pill", || {
            Onboard::new(Flow::Pill)
        }))
        .story(story("Prototypes/Onboarding/Option · Fill", || {
            Onboard::new(Flow::Fill)
        }))
        .story(story("Prototypes/Onboarding/Option · Shout", || {
            Onboard::new(Flow::Shout)
        }))
        .story(story(
            "Prototypes/Onboarding/Without Hex · With Hex",
            || Onboard::new(Flow::Versus),
        ))
        .story(first_run())
        .story(Story::new(
            "Prototypes/Onboarding/Tapbacks",
            || Onboard::new(Flow::Versus),
            tapback_sheet,
        ))
}

/// Every tapback glyph, other people's then the reader's own, over stacks
/// on both sides of a thread, to hold against Messages.
fn tapback_sheet(_: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let glyphs = [
        Glyph::Heart,
        Glyph::ThumbsUp,
        Glyph::ThumbsDown,
        Glyph::Haha,
        Glyph::Bang,
        Glyph::Question,
    ];
    let set = |key: &str, own: bool| {
        row()
            .key(key.to_string())
            .gap(8.0)
            .children(glyphs.iter().enumerate().map(|(i, &glyph)| {
                canvas(cards::tapback(p, glyph, Side::Them, own, true))
                    .key(format!("g{i}"))
                    .size(TAP_BOX, TAP_BOX)
            }))
    };
    let line = |key: &str, side: Side, copy: &str, taps: &[Glyph]| {
        bubble(
            p,
            key.to_string(),
            side,
            Caption::None,
            copy,
            taps,
            1.0,
            true,
        )
    };
    let (fill, rim) = cards::material(p);
    col()
        .key("tapback-sheet")
        .w(380.0)
        .padding(20.0)
        .gap(10.0)
        .radius(18.0)
        .continuous_corners()
        .fill(fill)
        .border(p.hair, rim)
        .child(set("theirs", false))
        .child(set("own", true))
        .child(line(
            "they",
            Side::Them,
            "they can see my screen",
            &[Glyph::Haha],
        ))
        .child(line(
            "sorry",
            Side::You,
            "sorry fart fingers",
            &[Glyph::Haha, Glyph::Haha],
        ))
        .child(line(
            "incontinence",
            Side::You,
            "So sorry for the incontinence",
            &[Glyph::Haha, Glyph::Haha, Glyph::Bang],
        ))
        .child(line(
            "desk",
            Side::Them,
            "Clear your desk.",
            &[Glyph::ThumbsUp, Glyph::ThumbsUp, Glyph::Heart],
        ))
}

fn first_run() -> Story<Onboard> {
    Story::new(
        "Prototypes/Onboarding/First run",
        || {
            let mut s = Onboard::new(Flow::FirstRun);
            s.speech = Speech::AfterDownload;
            s
        },
        view,
    )
    .poll(poll)
    .trigger('r', "Play", play)
    .trigger('t', "Jump to Your turn", |s, _| s.jump_to_try())
    .trigger('s', "Next typing sound", |s, _| {
        let next = s.sfx.style.next();
        s.choose_key_sound(next);
    })
    .trigger('d', "Finish the download", finish_download)
    .variant("Download done by the try", |_| {})
    .variant("Slow download", |s| s.download_secs = SLOW_SECS)
    .variant("Mac in French", |s| s.wants = Language::Other)
    .variant("Model already downloaded", |s| s.had_model = true)
    .variant("Permissions already granted", |s| s.granted = true)
    .variant("Auto-play video", |s| s.scripted_try = true)
    .variant("Your turn, model streams", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
    })
    .variant("Your turn, model doesn't stream", |s| {
        s.rehearsal = Some(Rehearsal::new(false));
    })
    .variant("Whole run, scripted game", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
        s.autoplay = true;
    })
    .variant("Your turn, keeps tapping", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
        s.taps = TAPS_BEFORE_TOGGLE;
    })
    .variant("Your turn, practice field", |s| {
        s.rehearsal = Some(Rehearsal::new(true));
        s.practice_field = true;
    })
    .variant("Quick motion", |s| {
        s.motion.enter = 0.28;
        s.motion.exit = 0.20;
        s.motion.keyboard = 0.5;
    })
    .variant("Gentle motion", |s| {
        s.motion.enter = 0.4;
        s.motion.exit = 0.3;
        s.motion.keyboard = 0.8;
    })
    .side(side::side)
}

fn finish_download(s: &mut Onboard, _: &mut Play<'_, Onboard>) {
    if let Some(from) = s.download_from.as_mut() {
        *from -= s.download_secs;
    }
}

fn story(name: &'static str, init: fn() -> Onboard) -> Story<Onboard> {
    Story::new(name, init, view)
        .poll(poll)
        .trigger('r', "Play", play)
        .trigger('d', "Finish the download", finish_download)
        .variant("macOS 26 and later", |s| s.speech = Speech::Apple)
        .variant("Windows, Linux, older macOS", |s| {
            s.speech = Speech::AfterDownload;
        })
        .variant("Other language, no Apple Speech", |s| {
            s.speech = Speech::AfterDownload;
            s.wants = Language::Other;
        })
        .side(side::side)
}

fn view(s: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    s.reduced.set(cx.is_reduced_motion());
    flows::view(p, s)
}

fn play(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    s.now = play.now();
    s.ready_from = None;
    s.manual_override = false;
    s.auto_due_at = None;
    s.practice.focused = false;
    s.live_hold_from = None;
    s.sfx.clear();
    s.examples_from = Some(play.now());
    if s.rehearsal.is_some() && !s.autoplay {
        s.had_model = true;
        s.granted = true;
        s.jump_to_try();
        play.wake_after(Duration::ZERO);
        return;
    }
    if s.flow == Flow::FirstRun {
        start_first_run(s, play);
        return;
    }
    play.after(if s.flow == Flow::Versus { 1.4 } else { 2.6 }, advance);
    play.wake_after(Duration::ZERO);
}

fn start_first_run(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    if let Some(delay) = begin_first_run(s, play.now()) {
        play.after(delay, advance);
    }
    play.wake_after(Duration::ZERO);
}

/// `"Without Hex"`, waiting for `"Watch what happens"`.
fn park(s: &mut Onboard, now: f64) {
    s.step = Step::Parked;
    s.parked_from = Some(now);
    s.typing_from = None;
    s.sfx.on_parked(now, s.had_model);
}

/// Starts the first run at `now`; the story's next beat is due after the
/// returned seconds, or waits for `"Watch what happens"`.
pub(super) fn begin_first_run(s: &mut Onboard, now: f64) -> Option<f64> {
    s.now = now;
    s.flow_hud = FlowHud::default();
    s.examples_from = Some(now);
    s.pick(s.wants);
    if s.had_model {
        if let Some(download) = s.download.as_mut() {
            download.downloaded = download.total;
        }
        s.download_from = None;
        s.closed = 1.0;
        s.closed_from = None;
        park(s, now);
        None
    } else {
        s.sfx.on_download_start(now);
        Some(DOWNLOAD_HOLD)
    }
}

fn advance(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    if s.manual_override {
        return;
    }
    s.now = play.now();
    if let Some(secs) = next_beat(s, play.now()) {
        play.after(secs, advance);
    }
    play.wake_after(Duration::ZERO);
}

/// Moves the story one beat on at `now`; the next is due after the returned
/// seconds, or waits for the user.
fn next_beat(s: &mut Onboard, now: f64) -> Option<f64> {
    const WAIT: f64 = 0.25;
    if s.flow == Flow::FirstRun {
        return first_run_next(s, now);
    }
    match s.step {
        Step::Pick => {
            s.pick(s.wants);
            s.sfx.on_download_start(now);
            Some(if s.flow == Flow::Shout { 0.6 } else { 0.9 })
        }
        Step::Started if s.flow == Flow::Versus => {
            park(s, now);
            None
        }
        Step::Started => {
            s.step = Step::Parked;
            s.parked_from = Some(now);
            Some(0.5)
        }
        Step::Parked if s.typing_from.is_none() => None,
        Step::Parked if s.can_hear() => {
            s.step = Step::YourTurn;
            s.voice_from = Some(now);
            s.hud_from = Some(now);
            s.sfx.on_rewind(now);
            Some(versus::BEFORE_KEY)
        }
        Step::Parked => Some(WAIT),
        Step::YourTurn => {
            s.step = Step::Hearing;
            hear(s, now);
            Some(Script::chosen().take().release + 0.1)
        }
        Step::Hearing if s.flow == Flow::Versus => {
            s.step = Step::Heard;
            Some(versus::reply_secs(s))
        }
        Step::Hearing => {
            s.step = Step::Heard;
            Some(1.2)
        }
        Step::Heard if s.downloaded() => {
            s.step = Step::Done;
            s.sfx.on_score(now);
            Some(SCORE_HOLD)
        }
        Step::Heard => Some(WAIT),
        Step::Done => {
            s.step = Step::Ready;
            s.ready_from = Some(now);
            s.sfx.on_ready(now, s.reduced.get());
            None
        }
        _ => None,
    }
}

/// How long the score holds before the story moves on, when nobody is
/// there to press `Continue →`.
const SCORE_HOLD: f64 = 4.5;

/// `"With Hex"`: the shortcut goes down and Jay's answer is spoken.
fn hear(s: &mut Onboard, now: f64) {
    s.take = Script::chosen().take();
    s.take_from = Some(now);
    s.sfx.on_voice_take(now, s.take);
    let beats = versus::hex_beats(s.streams(), &s.timing);
    s.sfx.on_hex_reply(now, &beats);
}

/// The first run: `"Without Hex"` and a scripted `"With Hex"` play while
/// the model downloads, then the microphone, the practice game as the
/// user's own `"With Hex"` once the model is in, Accessibility, and
/// `"Hex is ready"`.
pub(super) fn first_run_next(s: &mut Onboard, now: f64) -> Option<f64> {
    const WAIT: f64 = 0.25;
    match s.step {
        // The real permissions wait for macOS, then hold their checks a beat.
        Step::Microphone
            if s.live
                .as_ref()
                .is_some_and(|live| !live.mic_granted_for(now)) =>
        {
            Some(WAIT)
        }
        Step::Accessibility if s.live.as_ref().is_some_and(|live| !live.granted_for(now)) => {
            Some(WAIT)
        }
        Step::Pick | Step::Started => {
            park(s, now);
            s.autoplay.then_some(WATCH_AFTER)
        }
        Step::Parked if s.autoplay && s.typing_from.is_none() => {
            let at = s.parked_from.unwrap_or(now) + WATCH_AFTER;
            if at - now > 1e-3 {
                return Some(at - now);
            }
            s.start_scene();
            Some(s.scene.duration() + COMEDY_HOLD)
        }
        // `"Without Hex"` waits for `"Watch what happens"`, plays out, and
        // its last line holds a beat; then `"With Hex"` shows how it goes,
        // with nothing asked of the user yet.
        Step::Parked => {
            let from = s.typing_from?;
            let left = from + s.scene.duration() + COMEDY_HOLD - now;
            if left > 1e-3 {
                return Some(left);
            }
            s.step = Step::YourTurn;
            s.voice_from = Some(now);
            s.hud_from = Some(now);
            s.sfx.on_rewind(now);
            Some(versus::BEFORE_KEY)
        }
        Step::YourTurn => {
            s.step = Step::Hearing;
            hear(s, now);
            Some(Script::chosen().take().release + 0.1)
        }
        Step::Hearing => {
            s.step = Step::Heard;
            Some(versus::reply_secs(s))
        }
        Step::Heard => leave_comedy(s, now),
        Step::Microphone | Step::Waiting => to_practice(s, now),
        Step::Try => {
            if s.interactive_try() {
                return None;
            }
            s.step = Step::TryHearing;
            s.take = &ANSWER_TRY;
            s.take_from = Some(now);
            s.sfx.on_voice_take(now, &ANSWER_TRY);
            Some(ANSWER_TRY.release + 0.1)
        }
        Step::TryHearing => {
            s.step = Step::TryHeard;
            Some(2.2)
        }
        Step::TryHeard => {
            if s.interactive_try() {
                return None;
            }
            if s.granted {
                s.jump_to_ready();
                None
            } else {
                to_accessibility(s, now)
            }
        }
        // A real first run that had to ask before the practice plays it now.
        Step::Accessibility if !s.played => to_practice(s, now),
        Step::Accessibility => {
            s.jump_to_ready();
            None
        }
        // The first run has no score: `"With Hex"` goes on to the microphone.
        Step::Done | Step::Ready => None,
    }
}

/// Short presses on the practice before it offers tap to start instead.
pub(crate) const TAPS_BEFORE_TOGGLE: u8 = 2;

/// How long an autoplaying story waits on `"Watch what happens"`.
const WATCH_AFTER: f64 = 1.8;

/// How long `"Without Hex"`'s last line holds before the first run moves on.
const COMEDY_HOLD: f64 = 1.6;

/// Out of the comedy, by `"With Hex"`'s end or `Skip →`.
fn leave_comedy(s: &mut Onboard, now: f64) -> Option<f64> {
    if s.mic_granted || s.granted {
        to_practice(s, now)
    } else {
        to_microphone(s, now)
    }
}

fn to_microphone(s: &mut Onboard, now: f64) -> Option<f64> {
    s.step = Step::Microphone;
    s.permissions_from = Some(now);
    if s.live.is_some() {
        return Some(PERMISSIONS_WAIT);
    }
    s.sfx.on_microphone(now);
    Some(permissions::MIC_SECS)
}

fn to_accessibility(s: &mut Onboard, now: f64) -> Option<f64> {
    s.step = Step::Accessibility;
    s.permissions_from = Some(now);
    if s.live.is_some() {
        return Some(PERMISSIONS_WAIT);
    }
    s.sfx.on_accessibility(now);
    Some(permissions::AX_SECS)
}

/// On to the practice game as `"With Hex"`, once Hex can hear: until the
/// model is in, a calm `"Almost ready"`. Hex's shortcut works only with
/// Accessibility, so a real first run without it asks for it first.
fn to_practice(s: &mut Onboard, now: f64) -> Option<f64> {
    const WAIT: f64 = 0.25;
    if s.live.is_some() && !s.granted {
        return to_accessibility(s, now);
    }
    if !s.can_hear() {
        if s.step != Step::Waiting {
            s.step = Step::Waiting;
            s.sfx.on_almost_ready(now);
        }
        return Some(WAIT);
    }
    if s.step == Step::Waiting && s.closed < 1.0 {
        return Some(0.9);
    }
    s.step = Step::Try;
    s.played = true;
    s.take_from = None;
    s.hud_from = Some(now);
    s.partial = "";
    s.practice = TextEdit::default();
    s.dictated.reset();
    if let Some(rehearsal) = s.rehearsal.as_mut() {
        *rehearsal = Rehearsal::new(rehearsal.streams);
        rehearsal.from = now;
    }
    if s.interactive_try() {
        s.practice.focused = true;
        None
    } else {
        Some(1.2)
    }
}

#[cfg(target_os = "macos")]
pub(super) fn physical_shortcut_held(caps: &[String]) -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
    }
    const HID_SYSTEM_STATE: u32 = 1;
    if caps.is_empty() {
        return false;
    }
    let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
    caps.iter().all(|cap| {
        if cap.contains('⌥') || cap.contains("Alt") {
            (flags & 0x0008_0000) != 0
        } else if cap.contains('⌘') {
            (flags & 0x0010_0000) != 0
        } else if cap.contains('⌃') || cap.contains("Ctrl") {
            (flags & 0x0004_0000) != 0
        } else if cap.contains('⇧') || cap.contains("Shift") {
            (flags & 0x0002_0000) != 0
        } else if cap == "fn" {
            (flags & 0x0080_0000) != 0
        } else {
            false
        }
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn physical_shortcut_held(_: &[String]) -> bool {
    false
}

fn poll(s: &mut Onboard, now: f64, tx: &mut Tx) -> bool {
    if advance_to(s, now, tx) {
        tx.wake_after(FRAME);
    }
    true
}

/// Moves the story to `now`; true while anything on it still moves.
pub(super) fn advance_to(s: &mut Onboard, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    tx.trace_channel("onboarding.step", "phase", s.step as u32 as f32);
    tx.trace_channel("onboarding.mic", "level", s.listening_peak().unwrap_or(0.0));
    // A sent `"Without Hex"` reply lifts out of the field into its bubble.
    let sent = versus::sent_count(s);
    for i in s.sent_seen..sent {
        tx.launch("typed/line0", format!("sent-bubble-{i}"));
    }
    s.sent_seen = sent;
    // `"With Hex"`'s answer lifts out of the field from its first word.
    let hex_sent = versus::hex_sent(s);
    if hex_sent && !s.hex_seen {
        tx.launch("w0", "hex-bubble");
    }
    s.hex_seen = hex_sent;
    if s.manual_override
        && let Some(due) = s.auto_due_at
        && now >= due
    {
        s.auto_due_at = next_beat(s, now).map(|secs| now + secs);
    }

    let trying = s.interactive_try() && matches!(s.step, Step::Try | Step::TryHeard);
    if trying && super::game::step(s, now) {
        s.hud.clear();
    } else if trying {
        // The real first run follows Hex's own dictation, which sounds its
        // own tones; a rehearsal plays one without them.
        let real = match s.rehearsal {
            Some(rehearsal) => Some(rehearsal.held(now)),
            None => s.live.as_ref().map(|live| live.listening),
        };
        let held = real.unwrap_or_else(|| physical_shortcut_held(&s.caps));
        match (s.live_hold_from, held) {
            (None, true) => {
                s.live_hold_from = Some(now);
                if real.is_none() {
                    s.sfx.on_live_shortcut_down(now);
                }
            }
            (Some(_), false) => {
                s.live_hold_from = None;
                if real.is_none() {
                    s.sfx.on_live_shortcut_up(now);
                }
            }
            _ => {}
        }
        let partial = match s.rehearsal {
            Some(rehearsal) => rehearsal.partial(now),
            None => crate::dictation::live_text::current(),
        };
        s.dictated.hear(partial.as_ref(), now);
        if s.live.as_ref().is_some_and(|live| {
            matches!(
                live.practice,
                Practice::TooShort | Practice::NothingHeard | Practice::Failed
            )
        }) {
            s.dictated.forget();
        }
        if s.rehearsal.as_mut().is_some_and(|r| r.paste_due(now)) {
            s.practice.insert(ANSWER_TRY.text());
            s.on_practice_change(tx);
        }
        s.dictated.settle(now);
        let landed = std::mem::take(&mut s.hud_landed);
        s.hud.step(
            now,
            s.live_hold_from.is_some(),
            crate::audio::input_level::recent(),
            landed,
            s.reduced.get(),
        );
    } else {
        s.game = None;
        s.live_hold_from = None;
        s.hud.clear();
    }

    s.sfx.advance_to(now);
    if s.ready_from.is_some_and(|from| now - from >= BUILD_SECS) {
        s.sfx.flush_offline_if_requested(now + 1.5);
    }
    let mut moving = s.sfx.has_pending(now)
        || s.auto_due_at.is_some()
        || s.live_hold_from.is_some()
        || (s.interactive_try() && matches!(s.step, Step::Try | Step::TryHeard))
        || (s.examples_from.is_some() && (s.step < Step::Ready || !s.reduced.get()))
        || (s.step == Step::Ready && !s.reduced.get() && s.ready_age() < BUILD_SECS);
    moving |= s.dictated.moving();

    // The real first run's download comes from Hex (`live::poll`).
    let from = s.download_from.filter(|_| s.live.is_none());
    if let Some(download) = s.download.as_mut().filter(|_| s.live.is_some()) {
        if download.done() {
            s.closed_from.get_or_insert(now);
        } else {
            moving = true;
        }
    } else if let (Some(from), Some(download)) = (from, s.download.as_mut()) {
        download.downloaded = downloaded(download.total, now - from, s.download_secs);
        if download.done() {
            s.closed_from.get_or_insert(now);
        } else {
            moving = true;
        }
    }
    if let Some(from) = s.closed_from {
        s.closed = if s.reduced.get() {
            1.0
        } else {
            smooth(((now - from) / CLOSE_SECS) as f32)
        };
        moving |= s.closed < 1.0;
    }
    match s.take_from {
        Some(from) => {
            let elapsed = now - from;
            let done = s.take.finished(elapsed);
            s.partial = if done || s.streams() {
                s.take.partial(elapsed)
            } else {
                ""
            };
            s.meter = s.take.meter(elapsed);
            moving |= !done;
        }
        None => {
            let hum = ((now * 9.0).sin() * (now * 4.3).sin()).abs() as f32;
            s.meter = (0.03 + 0.04 * hum, 0.05 + 0.06 * hum);
        }
    }
    moving
}

fn smooth(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

#[cfg(test)]
mod tests {
    use super::super::fixture::ANSWER_JAY;
    use super::*;

    fn run(flow: Flow, speech: Speech) -> Onboard {
        let mut s = Onboard::new(flow);
        s.speech = speech;
        s.examples_from = Some(0.0);
        s
    }

    #[test]
    fn apple_speech_hears_before_the_download_finishes() {
        let mut s = run(Flow::Pill, Speech::Apple);
        s.pick(Language::English);
        assert!(s.can_hear());
        assert!(!s.downloaded());
    }

    #[test]
    fn without_apple_speech_nothing_hears_until_the_model_lands() {
        let mut s = run(Flow::Pill, Speech::AfterDownload);
        s.pick(Language::English);
        assert!(!s.can_hear());
        let mut tx = Tx::default();
        poll(&mut s, DOWNLOAD_SECS + 0.1, &mut tx);
        assert!(s.downloaded());
        assert!(s.can_hear());
        assert!(s.streams());
    }

    #[test]
    fn the_multilingual_model_shows_the_reply_on_release() {
        let mut s = run(Flow::Shout, Speech::AfterDownload);
        s.pick(Language::Other);
        assert!(!s.streams());
        s.take_from = Some(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 0.8, &mut tx);
        assert_eq!(s.partial, "");
        poll(&mut s, ANSWER_JAY.release + 0.1, &mut tx);
        assert_eq!(s.partial, ANSWER_JAY.text());
    }

    #[test]
    fn examples_type_one_after_the_other() {
        let mut s = run(Flow::Pill, Speech::Apple);
        s.now = EXAMPLE_START + WORD_SECS * 2.5;
        assert_eq!(s.example(0), ("Yes, call", false));
        assert_eq!(s.example(1).0, "");
        s.now = 20.0;
        assert_eq!(s.example(0), (EXAMPLE_REPLIES[0], true));
        assert_eq!(s.example(1), (EXAMPLE_REPLIES[1], true));
    }

    #[test]
    fn live_try_pauses_and_completes_when_real_words_paste_in() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.jump_to_try();
        assert_eq!(s.step, Step::Try);
        s.practice.text = "Testing live dictation in Hex.".into();
        let mut tx = Tx::default();
        s.on_practice_change(&mut tx);
        assert_eq!(s.step, Step::TryHeard);
        s.press_continue();
        assert_eq!(s.step, Step::Accessibility);
    }

    #[test]
    fn the_practice_goes_straight_to_ready_once_both_permissions_are_on() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.granted = true;
        s.jump_to_try();
        s.practice.text = "Testing live dictation in Hex.".into();
        s.on_practice_change(&mut Tx::default());
        s.press_continue();
        assert_eq!(s.step, Step::Ready);
    }

    #[test]
    fn without_hex_waits_for_watch_what_happens() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.had_model = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        assert_eq!(s.step, Step::Parked);
        assert!(versus::waiting_to_start(&s));
        s.start_scene();
        let end = 30.0 + s.scene.duration();
        poll(&mut s, end - 0.05, &mut tx);
        assert_eq!(s.step, Step::Parked);
        poll(&mut s, end + 0.01, &mut tx);
        assert_eq!(s.step, Step::Parked);
        poll(&mut s, end + COMEDY_HOLD + 0.01, &mut tx);
        assert_eq!(s.step, Step::YourTurn);
    }

    #[test]
    fn with_hex_plays_itself_after_without_hex_then_asks_for_the_microphone() {
        use crate::feedback::{OnboardingTone, Tone};
        let mut s = Onboard::new(Flow::FirstRun);
        s.had_model = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        s.start_scene();
        let mut now = 30.0;
        let mut shortcut = None;
        let mut steps = vec![s.step];
        while now < 120.0 && s.step != Step::Microphone {
            poll(&mut s, now, &mut tx);
            if s.step == Step::Hearing {
                shortcut.get_or_insert(now);
            }
            if steps.last() != Some(&s.step) {
                steps.push(s.step);
            }
            now += 0.05;
        }
        assert_eq!(
            steps,
            [
                Step::Parked,
                Step::YourTurn,
                Step::Hearing,
                Step::Heard,
                Step::Microphone
            ],
            "nothing asked of the user on the way"
        );
        // From `⌥` going down to the microphone, polled every 0.05 s.
        let with_hex = now - shortcut.unwrap();
        assert!(
            with_hex <= super::super::triumph::MAX_SECS + 0.1,
            "{with_hex:.1} s"
        );
        let fired = |tone: Tone| s.sfx.fired_cues().iter().any(|&(_, t, _)| t == tone);
        assert!(fired(Tone::Onboarding(OnboardingTone::ShortcutDown)));
        assert!(
            s.sfx
                .fired_cues()
                .iter()
                .any(|(_, t, _)| matches!(t, Tone::Onboarding(OnboardingTone::Triumph(_))))
        );
    }

    #[test]
    fn the_first_run_asks_for_the_microphone_then_plays_then_asks_for_accessibility() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.speech = Speech::AfterDownload;
        s.scripted_try = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        let mut steps = vec![s.step];
        let mut now = 0.0;
        while now < 120.0 && s.step != Step::Ready {
            if versus::waiting_to_start(&s) {
                s.start_scene();
            }
            poll(&mut s, now, &mut tx);
            if steps.last() != Some(&s.step) {
                steps.push(s.step);
            }
            now += 0.05;
        }
        let order: Vec<Step> = [
            Step::Parked,
            Step::Heard,
            Step::Microphone,
            Step::Try,
            Step::Accessibility,
            Step::Ready,
        ]
        .into_iter()
        .filter(|step| steps.contains(step))
        .collect();
        assert_eq!(order.len(), 6, "{steps:?}");
        let at = |step| steps.iter().position(|s| *s == step).unwrap();
        assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{steps:?}");
    }

    #[test]
    fn the_autoplaying_story_plays_the_game_and_reaches_ready_by_itself() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.speech = Speech::AfterDownload;
        s.rehearsal = Some(Rehearsal::new(true));
        s.autoplay = true;
        s.manual_override = true;
        s.auto_due_at = begin_first_run(&mut s, 0.0);
        let mut tx = Tx::default();
        let mut now = 0.0;
        let mut played = false;
        while now < 150.0 && s.step != Step::Ready {
            poll(&mut s, now, &mut tx);
            played |= s.game.is_some();
            now += 1.0 / 60.0;
        }
        assert!(played);
        assert_eq!(s.step, Step::Ready);
        assert!(now < 120.0, "{now}");
        let triumphs = s
            .sfx
            .fired_cues()
            .iter()
            .filter(|(_, t, _)| {
                matches!(
                    t,
                    crate::feedback::Tone::Onboarding(crate::feedback::OnboardingTone::Triumph(_))
                )
            })
            .count();
        assert_eq!(
            triumphs, 2,
            "`With Hex`'s payoff and the game's first score card"
        );
    }

    #[test]
    fn skip_shows_two_seconds_in_and_leaves_the_comedy() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.had_model = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        s.start_scene();
        poll(&mut s, 31.0, &mut tx);
        assert!(!versus::skippable(&s));
        assert!(!s.skip_scene());
        poll(&mut s, 32.1, &mut tx);
        assert!(versus::skippable(&s));
        assert!(s.skip_scene());
        assert!(matches!(
            s.step,
            Step::Microphone | Step::Accessibility | Step::Waiting | Step::Try
        ));
    }

    #[test]
    fn skip_leaves_with_hex_too() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.had_model = true;
        s.begin_live(0.0);
        let mut tx = Tx::default();
        poll(&mut s, 30.0, &mut tx);
        s.start_scene();
        let mut now = 30.0;
        while now < 120.0 && s.step != Step::Heard {
            poll(&mut s, now, &mut tx);
            now += 0.05;
        }
        assert!(versus::skippable(&s));
        assert!(s.skip_scene());
        assert_eq!(s.step, Step::Microphone);
    }

    #[test]
    fn a_retimed_scene_plays_at_its_new_length() {
        let mut s = Onboard::new(Flow::FirstRun);
        let before = s.scene.duration();
        s.timing.freeze += 1.0;
        s.retime();
        assert!((s.scene.duration() - before - 1.0).abs() < 1e-9);
    }

    #[test]
    fn reduced_motion_does_not_suppress_onboarding_sounds() {
        let simulate = |reduced: bool| -> Vec<(f64, crate::feedback::Tone, u8)> {
            let mut s = Onboard::new(Flow::FirstRun);
            s.speech = Speech::AfterDownload;
            s.scripted_try = true;
            s.reduced.set(reduced);
            s.begin_live(0.0);
            let mut tx = Tx::default();
            let mut now = 0.0;
            while now < 90.0 && s.step != Step::Ready {
                if versus::waiting_to_start(&s) {
                    s.start_scene();
                }
                poll(&mut s, now, &mut tx);
                now += 0.05;
            }
            poll(&mut s, now + 0.3, &mut tx);
            assert_eq!(s.step, Step::Ready);
            s.sfx.fired_cues().to_vec()
        };
        let full = simulate(false);
        let reduced = simulate(true);
        // The one exception: Reduced Motion shows the ready logo already
        // built, so only its last note sounds.
        let build = |t: &crate::feedback::Tone| {
            matches!(
                t,
                crate::feedback::Tone::Onboarding(crate::feedback::OnboardingTone::Build(_))
            )
        };
        let before = |cues: &[(f64, crate::feedback::Tone, u8)]| -> Vec<_> {
            cues.iter().filter(|(_, t, _)| !build(t)).copied().collect()
        };
        assert_eq!(before(&full), before(&reduced));
        let notes = |cues: &[(f64, crate::feedback::Tone, u8)]| -> Vec<_> {
            cues.iter()
                .filter(|(_, t, _)| build(t))
                .map(|&(_, t, _)| t)
                .collect()
        };
        assert_eq!(notes(&full).len(), 3);
        assert_eq!(
            notes(&reduced),
            [crate::feedback::Tone::Onboarding(
                crate::feedback::OnboardingTone::Build(2)
            )]
        );
        assert!(
            full.len() >= 35,
            "expected full sound track, got {}",
            full.len()
        );
    }
}
