//! Labs › Words to cursor (macOS): a pasted dictation's words fly from the
//! HUD to where the text landed in the focused app.
//!
//! The paste never waits for it. The output worker hands the pasted text to
//! [`pasted`] after the paste returns, and one thread of its own reads the
//! app through Accessibility (each call bounded by 50 ms): it waits up to
//! `wait_ms` for the text before the cursor to read as the dictation, then
//! sends every word to its own rect. When the app never shows the text, the
//! words fly to the caret, or to the text field when only that is known,
//! and with neither there is no flight. The overlay that draws it is
//! `rune_ui::words_to_caret`.

use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::accessibility::{self, CaretTarget, ScreenRect};

/// The flight's timings: `~/.config/hex/words-to-caret.json`, read as each
/// flight launches, so an edit applies to the next dictation. Missing keys
/// keep these defaults.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Tuning {
    /// Each word's time in the air.
    pub(crate) duration_ms: f64,
    /// The gap between one word's launch and the next.
    pub(crate) stagger_ms: f64,
    /// How far the arc bows from the straight line, as a share of its length.
    pub(crate) bend: f32,
    /// The ease-out-back overshoot at the landing (0 for none).
    pub(crate) overshoot: f32,
    /// A word's size as it leaves the HUD, in points.
    pub(crate) launch_size: f32,
    /// The landing's spring: its push in points, frequency, and damping.
    pub(crate) land_push: f32,
    pub(crate) land_hz: f32,
    pub(crate) land_damping: f32,
    /// How long a landed word glows over the real text before it's gone.
    pub(crate) fade_ms: f64,
    /// How warm a word glows in flight and as it lands (0 for plain ink).
    pub(crate) glow: f32,
    /// At most this many words fly; the rest of the dictation just pastes.
    pub(crate) max_words: usize,
    /// How long to wait for the app to show the pasted text.
    pub(crate) wait_ms: u64,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            duration_ms: 300.0,
            stagger_ms: 28.0,
            bend: 0.16,
            overshoot: 0.9,
            launch_size: 15.0,
            land_push: 10.0,
            land_hz: 8.5,
            land_damping: 0.55,
            fade_ms: 260.0,
            glow: 0.6,
            max_words: 14,
            wait_ms: 250,
        }
    }
}

impl Tuning {
    pub(crate) fn load() -> Self {
        let Ok(path) = crate::app_paths::words_to_caret_tuning_file() else {
            return Self::default();
        };
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %path.display(), "words to cursor tuning ignored");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
}

/// One flight, in Rune's global space (points from the primary display's
/// top-left, y down).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Launch {
    /// The HUD plate's middle.
    pub(crate) from: [f32; 2],
    /// Each word and the rect it lands on, in order.
    pub(crate) words: Vec<(String, ScreenRect)>,
    pub(crate) tuning: Tuning,
}

enum Job {
    /// A dictation started in this app: Chromium needs a moment to wake its
    /// accessibility tree.
    Wake(i32),
    Pasted(String),
}

static LAUNCH: OnceLock<Box<dyn Fn(Launch) + Send + Sync>> = OnceLock::new();
static JOBS: OnceLock<SyncSender<Job>> = OnceLock::new();

/// Where flights go: the overlay, set once as it installs.
pub(crate) fn on_launch(launch: impl Fn(Launch) + Send + Sync + 'static) {
    let _ = LAUNCH.set(Box::new(launch));
}

/// A dictation is about to start in the frontmost app.
pub(crate) fn wake() {
    if crate::labs::words_to_caret()
        && let Some(pid) = crate::dictation::context::frontmost_pid()
    {
        send(Job::Wake(pid));
    }
}

/// `text` was just pasted. Never waits; a busy thread drops the flight.
pub(crate) fn pasted(text: &str) {
    if crate::labs::words_to_caret() && LAUNCH.get().is_some() && !text.trim().is_empty() {
        send(Job::Pasted(text.to_string()));
    }
}

fn send(job: Job) {
    let jobs = JOBS.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel(2);
        if let Err(error) = std::thread::Builder::new()
            .name("words-to-caret".into())
            .spawn(move || run(receiver))
        {
            tracing::warn!(%error, "could not start words to cursor");
        }
        sender
    });
    let _ = jobs.try_send(job);
}

fn run(jobs: Receiver<Job>) {
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Wake(pid) => accessibility::wake_text_markers(pid),
            Job::Pasted(text) => {
                let started = Instant::now();
                let launch = find(&text);
                tracing::debug!(
                    words = launch.as_ref().map_or(0, |launch| launch.words.len()),
                    find_ms = started.elapsed().as_millis(),
                    "words to cursor"
                );
                if let (Some(launch), Some(send)) = (launch, LAUNCH.get()) {
                    send(launch);
                }
            }
        }
    }
}

/// Where `text`'s words go, read from the frontmost app.
fn find(text: &str) -> Option<Launch> {
    if accessibility::secure_input() {
        return None;
    }
    let tuning = Tuning::load();
    let pid = crate::dictation::context::frontmost_pid()?;
    let from = crate::rune_hud::plate_center().unwrap_or_else(hud_spot);
    let words: Vec<&str> = text.split_whitespace().take(tuning.max_words).collect();
    let deadline = Instant::now() + Duration::from_millis(tuning.wait_ms);
    let rects = loop {
        if let Some(rects) = accessibility::pasted_words(pid, text, tuning.max_words) {
            break Some(rects);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(8));
    };
    let words = match rects {
        Some(rects) => land_on_words(&words, &rects),
        None => land_on_target(&words, accessibility::caret_target(pid)?),
    };
    (!words.is_empty()).then_some(Launch {
        from,
        words,
        tuning,
    })
}

/// Where the HUD's plate sits before its first show: under the primary
/// display's menu bar, in the middle.
fn hud_spot() -> [f32; 2] {
    #[repr(C)]
    struct CgRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CgRect;
    }
    // SAFETY: plain CoreGraphics queries, safe from any thread.
    let bounds = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    [
        (bounds.x + bounds.width / 2.0) as f32,
        (bounds.y + 44.0) as f32,
    ]
}

/// Each word onto its own rect; a word without a believable one stays put.
fn land_on_words(words: &[&str], rects: &[Option<ScreenRect>]) -> Vec<(String, ScreenRect)> {
    words
        .iter()
        .zip(rects)
        .filter_map(|(word, rect)| rect.map(|rect| (word.to_string(), rect)))
        .collect()
}

/// Every word into the caret, or near the text field's leading edge when
/// only the field is known.
fn land_on_target(words: &[&str], target: CaretTarget) -> Vec<(String, ScreenRect)> {
    let rect = match target {
        CaretTarget::Caret(rect) => rect,
        CaretTarget::Field(field) => {
            let height = field.height.min(20.0);
            ScreenRect {
                x: field.x + 16.0,
                y: field.y + field.height / 2.0 - height / 2.0,
                width: 0.0,
                height,
            }
        }
    };
    words.iter().map(|word| (word.to_string(), rect)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64) -> ScreenRect {
        ScreenRect {
            x,
            y: 400.0,
            width: 30.0,
            height: 16.0,
        }
    }

    #[test]
    fn a_word_without_a_believable_rect_does_not_fly() {
        let words = ["so", "the", "plan"];
        let landed = land_on_words(&words, &[Some(rect(10.0)), None, Some(rect(60.0))]);
        assert_eq!(
            landed,
            vec![("so".into(), rect(10.0)), ("plan".into(), rect(60.0))]
        );
    }

    #[test]
    fn a_field_alone_lands_words_near_its_leading_edge() {
        let field = ScreenRect {
            x: 100.0,
            y: 200.0,
            width: 400.0,
            height: 30.0,
        };
        let landed = land_on_target(&["hi"], CaretTarget::Field(field));
        assert_eq!(
            landed[0].1,
            ScreenRect {
                x: 116.0,
                y: 205.0,
                width: 0.0,
                height: 20.0,
            }
        );
    }

    #[test]
    fn missing_tuning_keys_keep_their_defaults() {
        let tuning: Tuning = serde_json::from_str(r#"{"duration_ms": 500}"#).unwrap();
        assert_eq!(tuning.duration_ms, 500.0);
        assert_eq!(tuning.stagger_ms, Tuning::default().stagger_ms);
    }
}
