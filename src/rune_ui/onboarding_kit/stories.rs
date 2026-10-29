//! The onboarding options as playground stories (the `playground`
//! experiment, ⇧⌘D, group Onboarding). Each plays itself on open and on R,
//! from a fake host that reports what the real one would: download bytes,
//! the microphone's meter, and partial transcripts.

use std::cell::Cell;
use std::time::Duration;

use rune::{Cx, El, Play, Playground, Story, Tx, col, text};

use super::super::controls::Paint;
use super::fixture::{
    ANSWER_JAY, ANSWER_TRY, Download, EXAMPLE_REPLIES, Language, Speech, Take, downloaded,
};
use super::{flows, permissions, typing};

const DOWNLOAD_SECS: f64 = 9.0;
/// First run's download runs behind the slower typing demo (~8.6 s) and
/// finishes during Permissions (~25 s); a slow download is still going after
/// Permissions so `"Almost ready"` shows.
const FIRST_RUN_SECS: f64 = 26.0;
const SLOW_SECS: f64 = 34.0;
const DOWNLOAD_HOLD: f64 = 2.8;
const CLOSE_SECS: f64 = 0.65;
const WORD_SECS: f64 = 0.22;
const EXAMPLE_START: f64 = 0.6;
const EXAMPLE_GAP: f64 = 0.5;
const FRAME: Duration = Duration::from_millis(16);
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
    Permissions,
    Waiting,
    Try,
    TryHearing,
    TryHeard,
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
    download_from: Option<f64>,
    pub(crate) download: Option<Download>,
    pub(crate) typing_from: Option<f64>,
    pub(crate) take_from: Option<f64>,
    pub(crate) partial: &'static str,
    pub(crate) take: &'static Take,
    pub(crate) meter: (f32, f32),
    examples_from: Option<f64>,
    closed_from: Option<f64>,
    pub(crate) closed: f32,
    download_secs: f64,
    pub(crate) had_model: bool,
    pub(crate) granted: bool,
    pub(crate) permissions_from: Option<f64>,
}

const PRESET_SHORTCUTS: &[&[&str]] = &[&["⌥"], &["fn"], &["⌃", "Space"], &["R⌘"]];

impl Onboard {
    fn new(flow: Flow) -> Self {
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
            download_from: None,
            download: None,
            typing_from: None,
            take_from: None,
            partial: "",
            take: &ANSWER_JAY,
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
            permissions_from: None,
        }
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
    }

    /// Cycles the shortcut capsule on `"Your turn"` so clicking it previews
    /// changing the dictation key.
    pub(crate) fn cycle_shortcut(&mut self) {
        self.shortcut_index = (self.shortcut_index + 1) % PRESET_SHORTCUTS.len();
        self.caps = PRESET_SHORTCUTS[self.shortcut_index]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }

    pub(crate) fn downloaded(&self) -> bool {
        self.download.is_some_and(Download::done)
    }

    pub(crate) fn listening_peak(&self) -> Option<f32> {
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
    .trigger('d', "Finish the download", finish_download)
    .variant("Download done by the try", |_| {})
    .variant("Slow download", |s| s.download_secs = SLOW_SECS)
    .variant("Mac in French", |s| s.wants = Language::Other)
    .variant("Model already downloaded", |s| s.had_model = true)
    .variant("Permissions already granted", |s| s.granted = true)
    .side(side)
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
        .side(side)
}

fn view(s: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    s.reduced.set(cx.is_reduced_motion());
    flows::view(p, s)
}

fn play(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    s.now = play.now();
    s.examples_from = Some(play.now());
    if s.flow == Flow::FirstRun {
        start_first_run(s, play);
        return;
    }
    play.after(if s.flow == Flow::Versus { 1.4 } else { 2.6 }, advance);
    play.wake_after(Duration::ZERO);
}

fn start_first_run(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    s.pick(s.wants);
    if s.had_model {
        if let Some(download) = s.download.as_mut() {
            download.downloaded = download.total;
        }
        s.download_from = None;
        s.closed = 1.0;
        s.closed_from = None;
        s.step = Step::Parked;
        s.typing_from = Some(play.now() + 0.65);
        play.after(0.65 + typing::duration() + 1.0, advance);
    } else {
        play.after(DOWNLOAD_HOLD, advance);
    }
    play.wake_after(Duration::ZERO);
}

fn advance(s: &mut Onboard, play: &mut Play<'_, Onboard>) {
    const WAIT: f64 = 0.25;
    s.now = play.now();
    if s.flow == Flow::FirstRun {
        if let Some(secs) = first_run_next(s, play.now()) {
            play.after(secs, advance);
        }
        play.wake_after(Duration::ZERO);
        return;
    }
    let next = match s.step {
        Step::Pick => {
            s.pick(s.wants);
            Some(if s.flow == Flow::Shout { 0.6 } else { 0.9 })
        }
        Step::Started => {
            s.step = Step::Parked;
            if s.flow == Flow::Versus {
                s.typing_from = Some(play.now() + 0.55);
                Some(0.55 + typing::duration() + 1.0)
            } else {
                Some(0.5)
            }
        }
        Step::Parked if s.can_hear() => {
            s.step = Step::YourTurn;
            Some(1.1)
        }
        Step::Parked => Some(WAIT),
        Step::YourTurn => {
            s.step = Step::Hearing;
            s.take_from = Some(play.now());
            Some(ANSWER_JAY.release + 0.1)
        }
        Step::Hearing => {
            s.step = Step::Heard;
            Some(if s.flow == Flow::Versus { 1.6 } else { 1.2 })
        }
        Step::Heard if s.downloaded() => {
            s.step = Step::Done;
            Some(1.2)
        }
        Step::Heard => Some(WAIT),
        Step::Done => {
            s.step = Step::Ready;
            None
        }
        _ => None,
    };
    if let Some(secs) = next {
        play.after(secs, advance);
    }
    play.wake_after(Duration::ZERO);
}

fn first_run_next(s: &mut Onboard, now: f64) -> Option<f64> {
    const WAIT: f64 = 0.25;
    match s.step {
        Step::Pick | Step::Started => {
            s.step = Step::Parked;
            s.typing_from = Some(now + 0.6);
            Some(0.6 + typing::duration() + 1.0)
        }
        Step::Parked => {
            s.step = Step::YourTurn;
            Some(1.1)
        }
        Step::YourTurn => {
            s.step = Step::Hearing;
            s.take = &ANSWER_JAY;
            s.take_from = Some(now);
            Some(ANSWER_JAY.release + 0.1)
        }
        Step::Hearing => {
            s.step = Step::Heard;
            Some(1.5)
        }
        Step::Heard => {
            s.step = Step::Done;
            Some(1.3)
        }
        Step::Done if !s.granted => {
            s.step = Step::Permissions;
            s.permissions_from = Some(now);
            Some(permissions::PERMISSIONS_SECS)
        }
        Step::Done | Step::Permissions if !s.can_hear() => {
            s.step = Step::Waiting;
            Some(WAIT)
        }
        Step::Waiting if !s.can_hear() => Some(WAIT),
        Step::Waiting if s.closed < 1.0 => Some(0.9),
        Step::Done | Step::Permissions | Step::Waiting => {
            s.step = Step::Try;
            s.take_from = None;
            s.partial = "";
            Some(1.2)
        }
        Step::Try => {
            s.step = Step::TryHearing;
            s.take = &ANSWER_TRY;
            s.take_from = Some(now);
            Some(ANSWER_TRY.release + 0.1)
        }
        Step::TryHearing => {
            s.step = Step::TryHeard;
            Some(2.2)
        }
        Step::TryHeard => {
            s.step = Step::Ready;
            None
        }
        Step::Ready => None,
    }
}

fn poll(s: &mut Onboard, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    let mut moving = s.examples_from.is_some() && s.step < Step::Ready;
    if let (Some(from), Some(download)) = (s.download_from, s.download.as_mut()) {
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
    if moving {
        tx.wake_after(FRAME);
    }
    true
}

fn smooth(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

fn side(s: &Onboard, cx: &mut Cx<Onboard>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let line = |key: &str, copy: &str| {
        text(copy)
            .key(key)
            .font(p.k.ty.small)
            .color(p.muted())
            .wrap()
            .w(SIDE_W)
    };
    let words = match s.speech {
        Speech::Apple => "Words now: Apple Speech (SpeechAnalyzer) streams with no download.",
        Speech::AfterDownload => {
            "Words after the download: nothing can hear until the model lands, so this waits."
        }
    };
    col()
        .key("onboarding-side")
        .w(SIDE_W)
        .gap(8.0)
        .child(line(
            "progress",
            "Progress: transcription_preparation's downloaded bytes.",
        ))
        .child(line("meter", "Meter: the HUD's Meter { average, peak }."))
        .child(line("words", words))
        .child(line(
            "model",
            "English: Parakeet Unified English, 731 MB, streams. French: Parakeet v3, 740 MB, whole reply on release.",
        ))
}

#[cfg(test)]
mod tests {
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
        poll(&mut s, 1.5, &mut tx);
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
}
