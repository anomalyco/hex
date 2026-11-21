//! The recording toolbar's microphone level: while the toolbar is open with a
//! microphone chosen, what that microphone hears, for its sound button. It
//! reads dictation's level while dictation's microphone is the one chosen
//! and delivering audio; otherwise it opens the chosen input for the meter
//! alone (macOS shows its microphone indicator meanwhile), keeps no samples,
//! and closes it when the toolbar closes, Record takes over, or another
//! microphone is chosen. Dictation's own stream is never touched.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rune::OverlayHandle;

use crate::app_settings::{AppSettings, RECORD_MICROPHONE_DEFAULT, RECORD_MICROPHONE_NONE};
use crate::audio::input_level::{self, Peak};
use crate::audio::{AudioInput, AudioInputEvent};
use crate::rune_ui::capture_capsule::toolbar::{LEVEL_STEPS, Toolbar, level_step};

/// The meter that may run: each start and stop moves it on, and a meter
/// that's behind ends.
static CURRENT: AtomicU64 = AtomicU64::new(0);

/// How often the meter reads the level, and so how long the toolbar takes
/// to show a change.
const TICK: Duration = Duration::from_millis(30);

/// Meters `microphone` (`none`, `default`, or a device's name) on the
/// toolbar, ending any meter already running.
pub(super) fn start(overlay: OverlayHandle<Toolbar>, microphone: String) {
    let meter = CURRENT.fetch_add(1, Ordering::AcqRel) + 1;
    show(&overlay, &microphone, None);
    if microphone == RECORD_MICROPHONE_NONE {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hex-record-level".into())
        .spawn(move || run(&overlay, &microphone, meter));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not meter the recording toolbar's microphone");
    }
}

/// Ends the running meter, closing any input it opened.
pub(super) fn stop() {
    CURRENT.fetch_add(1, Ordering::AcqRel);
}

fn current(meter: u64) -> bool {
    CURRENT.load(Ordering::Acquire) == meter && super::toolbar::is_open()
}

fn run(overlay: &OverlayHandle<Toolbar>, microphone: &str, meter: u64) {
    let saved = AppSettings::read()
        .ok()
        .and_then(|settings| settings.microphone);
    let dictation = microphone == RECORD_MICROPHONE_DEFAULT;
    // Dictation's level stands for the chosen microphone when it's the one
    // dictation opens.
    let shared = dictation || saved.as_deref() == Some(microphone);
    let mut own: Option<AudioInput> = None;
    let mut opened = false;
    let own_peak = Peak::new();
    let mut level = 0.0_f32;
    let mut shown = None;
    let mut last = Instant::now();
    while current(meter) {
        let from_dictation = shared.then(input_level::recent).flatten();
        if from_dictation.is_none() && !opened {
            opened = true;
            let input = if dictation {
                crate::audio::open_dictation_input(saved.as_deref())
            } else {
                crate::audio::open_named_input(microphone)
            };
            own = input
                .inspect_err(|error| {
                    tracing::info!(%error, "the recording toolbar can't hear its microphone");
                })
                .ok();
        }
        std::thread::sleep(TICK);
        if let Some(input) = &own
            && !drain(input, &own_peak)
        {
            tracing::info!("the recording toolbar's microphone stopped");
            own = None;
        }
        let peak =
            from_dictation.or_else(|| own.as_ref().map(|_| own_peak.recent().unwrap_or(0.0)));
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let next = peak.map(|peak| {
            level = (peak * 3.0).clamp(0.0, 1.0).max(level - dt * 3.0);
            level_step(level)
        });
        if next != shown {
            shown = next;
            show(
                overlay,
                microphone,
                next.map(|step| f32::from(step) / LEVEL_STEPS),
            );
        }
    }
}

/// Reads what `input` delivered since the last tick into `peak`, keeping
/// no samples. False once its stream failed.
fn drain(input: &AudioInput, peak: &Peak) -> bool {
    loop {
        match input.recv_timeout(Duration::ZERO) {
            AudioInputEvent::Chunk { samples, .. } => peak.record(&samples),
            AudioInputEvent::Timeout => return true,
            AudioInputEvent::StreamFailed(_) => return false,
        }
    }
}

/// Shows `level` on the toolbar while it still meters `microphone`.
fn show(overlay: &OverlayHandle<Toolbar>, microphone: &str, level: Option<f32>) {
    let microphone = microphone.to_owned();
    overlay.update_if(move |s, _| {
        if s.microphone != microphone || s.level == level {
            return false;
        }
        s.level = level;
        true
    });
}
