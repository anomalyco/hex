//! The `capture` experiment (macOS and Linux): Scrawl's capture engine, the
//! `scrawl-capture` crate, hosted inside Hex. Step 3 of
//! `docs/plans/capture-in-hex.md`. Everything here is shared; what differs
//! by OS is the engine's own platform adapter (ScreenCaptureKit, or the
//! portal, PipeWire and VA-API), where the audio timeline comes from
//! (`DictationAudio` on macOS, the Linux listener's [`ListenerTimeline`]),
//! "While dictating" ([`Environment`]), the transcriber, and the clipboard.
//!
//! The engine records the screen and writes the bundle; Hex owns the
//! microphone and the model. A capture copies audio from the dictation
//! timeline through a bounded sink (no second stream) and hands it to
//! `SealedCapture::finish` ([`AudioIn::External`]), and its words come from
//! the dictation engine's loaded model as foreground work. The engine runs
//! with [`SharedResources::none`]: no ingest server, delivery retries, or
//! global shortcuts. [`CaptureHost::start`] does nothing unless the
//! experiment is on and the person turned captures on (`capture.enabled`).
//!
//! Step 4 adds the capture shortcut (`recognition`), and the pointer, Shift,
//! and key input the session tap reports while a capture records
//! ([`report_input`]), which the `capture-input` thread hands to the
//! capture's `InputSink` through the engine's pure input rules
//! (`scrawl_capture::hosted_input`).
//!
//! While a capture records it holds a recording-environment session with
//! "While dictating" (`recording_audio_behavior`, read at each start),
//! released when it stops, is cancelled, or Hex quits; dictation's own
//! session composes with it.
//!
//! Views never reach into the host: every change to a capture (started,
//! the screen recording, stopped, the engine's stages, settled) publishes
//! one [`CaptureSnapshot`], which [`snapshot`] reads and [`watch`] wakes on.

use std::fs;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use scrawl_capture::audio::{GapSpan, RecordedAudio};
use scrawl_capture::capture::{
    AudioIn, ConfigSource, Event, Host, InputSink, OwnWindows, SharedCaptureTelemetry,
    SharedResources, Stage, Start, Target,
};
use scrawl_capture::handoff::EngineConfig;
use scrawl_capture::hosted_input::{HostInput, HostedInput, Modifiers};
use scrawl_capture::model::{TranscriptData, WordTiming};
#[cfg(target_os = "macos")]
use scrawl_capture::transcribe::AppleTranscriber;
use scrawl_capture::transcribe::Transcriber;

use crate::audio::CaptureInstant;
use crate::control::{
    ControlReply, ScrawlCaptureAction, ScrawlCaptureOutcome, ScrawlCaptureReport,
    ScrawlCaptureState,
};
use crate::dictation::StreamingResampler;
#[cfg(target_os = "macos")]
use crate::dictation_audio::DictationAudio;
#[cfg(target_os = "macos")]
pub use crate::recording_environment::RecordingEnvironmentController as Environment;
#[cfg(target_os = "macos")]
use crate::recording_environment::RecordingEnvironmentSession as Quiet;
use crate::transcription_engine::TranscriptionEngine;
use crate::transcription_models::{ModelRuntime, TranscriptionModelId};

mod library;
mod routing;
mod sink;

pub use library::{
    explain as explain_capture, list as list_captures, send as send_capture, show as show_capture,
};
pub use routing::CaptureRoute;
pub use routing::{RouteAnswer, RouteDelivery, RouteDest};
#[cfg(target_os = "macos")]
pub use sink::CaptureSink;
#[cfg(target_os = "linux")]
pub use sink::ListenerTimeline;
pub use sink::{CaptureAudio, CaptureSinkClosed};

/// "While dictating" for a capture on Linux: its own audio suppressor, made
/// when the capture starts and restoring what it changed when dropped.
#[cfg(target_os = "linux")]
#[derive(Clone, Default)]
pub struct Environment;

#[cfg(target_os = "linux")]
type Quiet = crate::linux_audio_control::AudioSuppressor;

#[cfg(target_os = "linux")]
impl Environment {
    fn begin_capture(&self, behavior: crate::app_settings::RecordingAudioBehavior) -> Quiet {
        let quiet = Quiet::new(behavior);
        quiet.suppress();
        quiet
    }
}

/// The sink's capacity in microphone chunks (about 10 ms each): seconds of
/// slack before a stalled collector starts dropping audio as a gap.
const SINK_CAPACITY: usize = 512;
const EVENT_CAPACITY: usize = 64;
const SAMPLE_RATE: u32 = 16_000;
/// A capture keeps at most this much audio; the rest is recorded as a gap.
const MAX_CAPTURE: Duration = Duration::from_secs(20 * 60);
const GAP_TOLERANCE: Duration = Duration::from_millis(50);
/// How long shutdown waits for capture threads, inside the quit deadline.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
/// Captures being written at once: the one finishing plus those queued
/// behind it. A start beyond this is refused.
const MAX_FINISHING: usize = 2;
/// Tap events waiting for the `capture-input` thread; past this they drop.
const INPUT_CAPACITY: usize = 1_024;
/// A second press of the capture shortcut sooner than this after the start
/// is a bounce, not a stop.
pub const SHORTCUT_DEBOUNCE: Duration = Duration::from_millis(400);
/// Retention passes over the captures folder.
const RETENTION_FIRST: Duration = Duration::from_secs(90);
const RETENTION_EVERY: Duration = Duration::from_secs(60 * 60);
/// How long captures wait to try again after the host failed to start.
const START_RETRY: Duration = Duration::from_secs(60);

// ---------------------------------------------------------------------------
// The engine, as Hex runs it.

/// What [`CaptureHost`] needs from an engine: Scrawl's in the app, and in
/// tests one that records no screen.
trait CaptureEngine: Send + Sync + 'static {
    /// Starts recording the screen into `start.bundle_dir`. Blocking: it
    /// finds the display and starts ScreenCaptureKit.
    fn start(&self, start: Start) -> anyhow::Result<Box<dyn Recording>>;

    /// What the engine records for `target`. Blocking: a window or area
    /// reads ScreenCaptureKit's window list.
    fn target(&self, target: CaptureTarget) -> anyhow::Result<Target> {
        use scrawl_capture::capture::{discover_target_at, list_selectable_windows, region_target};
        Ok(match target {
            CaptureTarget::Pointer => Target::ActiveDisplay,
            CaptureTarget::Display { at } => Target::Screen(discover_target_at(None, Some(at))),
            // Wayland lists no windows: the portal's own picker chooses one.
            #[cfg(target_os = "linux")]
            CaptureTarget::Window { .. } => Target::Screen(scrawl_capture::model::TargetInfo {
                mode: scrawl_capture::model::CaptureMode::Window,
                ..discover_target_at(None, None)
            }),
            #[cfg(not(target_os = "linux"))]
            CaptureTarget::Window { id } => Target::Screen(
                list_selectable_windows()
                    .into_iter()
                    .find(|window| window.window_id == Some(id))
                    .ok_or_else(|| anyhow::anyhow!("the window closed"))?,
            ),
            CaptureTarget::Area {
                rect: [x, y, width, height],
            } => Target::Screen(region_target(
                x,
                y,
                width,
                height,
                &list_selectable_windows(),
            )),
            CaptureTarget::Phone => Target::Phone {
                device: None,
                name: None,
            },
        })
    }

    /// The names of the cabled iPhones. Blocking: it runs `scrawl-helper
    /// devices`, which reads the USB registry and asks for no permission.
    fn phones(&self) -> anyhow::Result<Vec<String>> {
        Ok(scrawl_capture::helper::devices()?
            .into_iter()
            .map(|phone| phone.name)
            .collect())
    }
}

/// What a capture records.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CaptureTarget {
    /// The display under the pointer, cropped to what changes: the
    /// shortcut's.
    Pointer,
    /// The whole display holding this point, in global top-left points.
    Display { at: [f64; 2] },
    /// A window, by its `CGWindowID`
    /// (`scrawl_capture::capture::list_selectable_windows`).
    Window { id: u32 },
    /// A rect in global top-left points, kept to the display holding its
    /// centre.
    Area { rect: [f64; 4] },
    /// The first cabled iPhone ([`CaptureSnapshot::phone`]), mirrored in a
    /// window while it records. The narration is Hex's microphone timeline,
    /// as for the screen; the mirror opens no microphone.
    Phone,
}

impl CaptureTarget {
    pub fn kind(self) -> TargetKind {
        match self {
            Self::Pointer => TargetKind::Pointer,
            Self::Display { .. } => TargetKind::Display,
            Self::Window { .. } => TargetKind::Window,
            Self::Area { .. } => TargetKind::Area,
            Self::Phone => TargetKind::Phone,
        }
    }
}

/// What a recording capture was started on: only the shortcut's
/// (`Pointer`) shows the capsule's switch chips.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetKind {
    Pointer,
    Display,
    Window,
    Area,
    Phone,
}

/// What the capture capsule asks of the host, through
/// `RecognitionControl::CaptureCommand`.
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureCommand {
    /// Starts a capture on the picker's pick; nothing while one records.
    Start(CaptureTarget),
    /// Looks for a cabled iPhone off the listener's thread;
    /// [`CaptureSnapshot::phone`] follows. Each start also looks.
    FindPhone,
    /// Drops the recording capture: its bundle is removed and it never
    /// settles, so the capsule can turn into the picker in its place.
    Discard,
    /// Scrawl's `switch_recording`: drops the recording capture as
    /// [`Self::Discard`] does and starts `target` in the same step, so no
    /// snapshot shows nothing recording in between. With nothing recording,
    /// only starts.
    Switch(CaptureTarget),
    /// Sends the newest written capture (`snapshot().last`) to `to` now: a
    /// session ID, or a name its route lists, suggestions first. A send
    /// already elsewhere is taken back first. `always` also saves a rule
    /// sending its site or app there from now on.
    Send { to: String, always: bool },
    /// Takes back the newest capture's send: one OpenCode took is
    /// withdrawn unless the session already acted on it.
    Undo,
    /// Pastes the newest written capture into the app in front, as
    /// Scrawl's Paste chip does: its contact sheet, then its summary.
    Paste,
    /// Opens the newest written capture's contact sheet: Hex has no review
    /// window, and opening the bundle could launch Scrawl.app, which takes
    /// captures over.
    Inspect,
}

/// A started capture.
trait Recording: Send {
    /// Stops recording and removes the bundle.
    fn cancel(self: Box<Self>);
    /// Stops recording the screen, its windows, and input now, keeping what
    /// was recorded for [`Sealed::finish`].
    fn stop(self: Box<Self>) -> Box<dyn Sealed>;
    /// Where the host's pointer and key input goes while it records.
    fn input(&self) -> Option<InputSink> {
        None
    }
}

/// A stopped capture, waiting to be written.
trait Sealed: Send {
    /// Removes the bundle.
    fn cancel(self: Box<Self>);
    /// Blocking: transcribes `audio`, picks keyframes, and writes the
    /// bundle.
    fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written>;
    /// The apps in front when it stopped, for routing; `None` when it isn't
    /// routed.
    fn route_context(&self) -> Option<scrawl_capture::capture::RouteContext> {
        None
    }
}

/// What a written capture reports.
struct Written {
    words: usize,
    /// Spans of its audio that went missing.
    gaps: usize,
    /// Why it has no transcript, if it has none.
    missing_transcript: Option<String>,
}

impl CaptureEngine for scrawl_capture::capture::Engine {
    fn start(&self, start: Start) -> anyhow::Result<Box<dyn Recording>> {
        Ok(Box::new(scrawl_capture::capture::Engine::start(
            self, start,
        )?))
    }
}

impl Recording for scrawl_capture::capture::Capture {
    fn cancel(self: Box<Self>) {
        scrawl_capture::capture::Capture::cancel(*self);
    }

    fn stop(self: Box<Self>) -> Box<dyn Sealed> {
        Box::new(scrawl_capture::capture::Capture::stop(*self))
    }

    fn input(&self) -> Option<InputSink> {
        Some(scrawl_capture::capture::Capture::input(self).clone())
    }
}

impl Sealed for scrawl_capture::capture::SealedCapture {
    fn cancel(self: Box<Self>) {
        scrawl_capture::capture::SealedCapture::cancel(*self);
    }

    fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written> {
        let finished = scrawl_capture::capture::SealedCapture::finish(*self, Some(audio))?;
        tracing::info!(
            bundle = %finished.dir.display(),
            duration_ms = finished.duration_ms,
            summary = %finished.summary,
            "capture written"
        );
        Ok(Written {
            words: finished.words,
            gaps: finished.gaps,
            missing_transcript: finished.missing_transcript,
        })
    }

    fn route_context(&self) -> Option<scrawl_capture::capture::RouteContext> {
        Some(scrawl_capture::capture::SealedCapture::route_context(self).clone())
    }
}

/// Scrawl's engine with nothing machine-wide: no ingest server (port
/// 47777), delivery retries, or global shortcuts. Hex never calls
/// `install_shared_resources`.
fn scrawl_engine(
    events: SyncSender<Event>,
    transcriber: Arc<dyn Transcriber>,
) -> scrawl_capture::capture::Engine {
    scrawl_capture::capture::Engine::new(Arc::new(HexHost { events }), Arc::new(HexConfig))
        .with_resources(SharedResources::none())
        .with_transcriber(transcriber)
}

/// What the capture asks for: `target`, on the clock the dictation timeline
/// shares, with Hex's audio and Hex's folder.
fn start_request(t0: CaptureInstant, bundle: PathBuf, target: Target) -> Start {
    Start {
        t0_boot_ns: t0.as_nanos(),
        target,
        bundle_dir: Some(bundle),
        audio: AudioIn::External,
        window_log: true,
        note: None,
    }
}

/// Forwards events to a bounded channel (the `capture-events` thread logs
/// them) and logs through tracing. Never blocks: a full channel drops the
/// event. `Engine::new` also makes it the crate's process-wide log sink, so
/// the engine's own log lines arrive here once, never in Scrawl's log file.
struct HexHost {
    events: SyncSender<Event>,
}

impl Host for HexHost {
    fn emit(&self, event: Event) {
        if let Err(TrySendError::Full(event)) = self.events.try_send(event) {
            tracing::warn!(?event, "capture event queue is full; dropping an event");
        }
    }

    fn log(&self, line: &str) {
        tracing::info!("{line}");
    }

    fn metric(&self, name: &str, ms: f64) {
        tracing::debug!(target: "hex::capture", name, ms, "capture metric");
    }

    fn own_windows(&self) -> OwnWindows {
        own_windows_of(
            &OWN_WINDOWS
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }
}

/// What a capture leaves out of its recording (every window Hex named) and
/// looks through for clicks: the ink overlay, and the capsule's panel, which
/// lets clicks through everywhere but its pill. Clicks on the pill never
/// reach the engine ([`CapsulePill`]).
fn own_windows_of(own: &[Option<u32>; 3]) -> OwnWindows {
    OwnWindows {
        excluded: own.iter().flatten().copied().collect(),
        passthrough: [OwnWindow::Ink, OwnWindow::Capsule]
            .into_iter()
            .filter_map(|role| own[role as usize])
            .collect(),
        recorded: Arc::clone(&OWN_WINDOWS_RECORDED),
    }
}

/// `capture.show_overlays_in_recordings`: while set, a capture records every
/// window of Hex's, its overlays included. A recording capture follows a
/// change at once.
static OWN_WINDOWS_RECORDED: LazyLock<Arc<AtomicBool>> = LazyLock::new(Arc::default);

/// Lets captures record Hex's own windows, or leaves them out again.
pub fn record_own_windows(recorded: bool) {
    OWN_WINDOWS_RECORDED.store(recorded, Ordering::Release);
}

/// Whether screen recordings may show Hex's overlays, as last saved.
pub fn own_windows_recorded() -> bool {
    OWN_WINDOWS_RECORDED.load(Ordering::Acquire)
}

/// The capsule's pill `[x, y, w, h]` in its panel's points while it shows.
static CAPSULE_PILL: Mutex<Option<[f32; 4]>> = Mutex::new(None);

/// Where the capsule's pill is in its panel while it shows, `None` when it
/// hides: the capsule's panel is far larger than the pill and lets clicks
/// through everywhere else.
pub fn set_capsule_pill(pill: Option<[f32; 4]>) {
    *CAPSULE_PILL
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = pill;
}

/// Whether a click lands on the capsule's pill, which takes it, rather than
/// the app beneath. The panel's place comes from the window list, reread at
/// most every 250 ms (scrolls come in bursts); it only moves between
/// captures.
#[derive(Default)]
struct CapsulePill {
    panel: Option<(Instant, u32, Option<[f64; 2]>)>,
}

impl CapsulePill {
    fn contains(&mut self, x: f64, y: f64) -> bool {
        let Some(pill) = *CAPSULE_PILL
            .lock()
            .unwrap_or_else(|error| error.into_inner())
        else {
            return false;
        };
        let Some(window) = OWN_WINDOWS
            .lock()
            .unwrap_or_else(|error| error.into_inner())[OwnWindow::Capsule as usize]
        else {
            return false;
        };
        let origin = match self.panel {
            Some((at, id, origin)) if id == window && at.elapsed() < Duration::from_millis(250) => {
                origin
            }
            _ => {
                let origin = panel_origin(&scrawl_capture::windows::raw_windows(), window);
                self.panel = Some((Instant::now(), window, origin));
                origin
            }
        };
        origin.is_some_and(|origin| on_pill(origin, pill, x, y))
    }
}

/// The top-left of `window` on screen, in global points.
fn panel_origin(windows: &[scrawl_capture::windows::RawWindow], window: u32) -> Option<[f64; 2]> {
    windows
        .iter()
        .find(|w| w.id == window)
        .map(|w| [w.bounds[0], w.bounds[1]])
}

/// Whether global point `(x, y)` is on `pill`, given in the points of a
/// panel whose top-left is `origin`.
fn on_pill(origin: [f64; 2], pill: [f32; 4], x: f64, y: f64) -> bool {
    let [px, py, pw, ph] = pill.map(f64::from);
    let (left, top) = (origin[0] + px, origin[1] + py);
    x >= left && x <= left + pw && y >= top && y <= top + ph
}

/// Hex's windows a capture leaves out of its recording, one per role. A
/// capture reads them when it starts.
static OWN_WINDOWS: Mutex<[Option<u32>; 3]> = Mutex::new([None, None, None]);

/// Which of Hex's windows a capture leaves out.
#[derive(Clone, Copy, Debug)]
pub enum OwnWindow {
    /// A panel far larger than the pill: click-through but for the pill
    /// ([`set_capsule_pill`]).
    Capsule,
    /// Click-through, so a click on it never counts as a click on Hex.
    Ink,
    /// The on-screen picker, hidden before the capture it starts records.
    Picker,
}

/// Leaves `window` out of every later capture's recording, replacing the
/// window that last held `role` (Rune may recreate an overlay's window).
pub fn exclude_window(role: OwnWindow, window: u32) {
    OWN_WINDOWS
        .lock()
        .unwrap_or_else(|error| error.into_inner())[role as usize] = Some(window);
}

/// The engine's settings, from Hex rather than `~/.config/scrawl`. The
/// captures folder, retention, and transcriber are Hex's `capture.*`
/// settings, applied by [`CaptureHost`]; the rest are Scrawl's defaults,
/// chosen here: auto-crop on (it applies to display captures), and the
/// `agentContext` summary format off.
struct HexConfig;

impl ConfigSource for HexConfig {
    fn engine_config(&self) -> anyhow::Result<EngineConfig> {
        Ok(hex_engine_config())
    }
}

fn hex_engine_config() -> EngineConfig {
    let mut config = EngineConfig::default();
    config.autocrop = Some(serde_json::Value::Bool(true));
    config
        .experiments
        .insert(scrawl_capture::handoff::AGENT_CONTEXT.into(), false);
    config
}

/// The listener's dictation engine, for the one capture engine's
/// transcriber; replaced when a listener starts captures.
static DICTATION_ENGINE: Mutex<Option<Weak<TranscriptionEngine>>> = Mutex::new(None);

/// Points captures at the listener's new engine after a model change.
#[cfg(target_os = "linux")]
pub fn use_engine(engine: &Arc<TranscriptionEngine>) {
    *DICTATION_ENGINE
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(Arc::downgrade(engine));
}

/// The process's one capture engine (it keeps process-global state), made
/// the first time captures turn on.
fn shared_engine(
    dictation: Option<&Arc<TranscriptionEngine>>,
) -> Result<Arc<scrawl_capture::capture::Engine>> {
    static ENGINE: Mutex<Option<Arc<scrawl_capture::capture::Engine>>> = Mutex::new(None);
    *DICTATION_ENGINE
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = dictation.map(Arc::downgrade);
    let mut engine = ENGINE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(engine) = &*engine {
        return Ok(engine.clone());
    }
    // The portal remembers the screen Hex was given; the token is Hex's.
    #[cfg(target_os = "linux")]
    if let Ok(support) = crate::app_paths::support_dir() {
        scrawl_capture::linux::set_token_path(support.join("screencast-token"));
    }
    let (events, received) = mpsc::sync_channel(EVENT_CAPACITY);
    thread::Builder::new()
        .name("capture-events".into())
        .spawn(move || log_events(received))?;
    let made = Arc::new(scrawl_engine(events, Arc::new(ChosenTranscriber)));
    *engine = Some(made.clone());
    Ok(made)
}

/// The transcriber `capture.transcriber` names when a capture is written.
struct ChosenTranscriber;

impl Transcriber for ChosenTranscriber {
    fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
        match crate::app_settings::capture_settings().transcriber {
            crate::app_settings::CaptureTranscriber::Parakeet => HexTranscriber {
                engine: DICTATION_ENGINE
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone(),
            }
            .transcribe(audio),
            #[cfg(target_os = "macos")]
            crate::app_settings::CaptureTranscriber::Apple => {
                AppleTranscriber::default().transcribe(audio)
            }
            // Apple's SpeechAnalyzer is macOS's; elsewhere it is Hex's model.
            #[cfg(not(target_os = "macos"))]
            crate::app_settings::CaptureTranscriber::Apple => HexTranscriber {
                engine: DICTATION_ENGINE
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone(),
            }
            .transcribe(audio),
        }
    }
}

/// Word-timed transcription on the dictation engine's loaded model. It holds
/// the engine weakly, so a capture still writing never keeps it alive.
struct HexTranscriber {
    engine: Option<Weak<TranscriptionEngine>>,
}

impl Transcriber for HexTranscriber {
    /// Word times are milliseconds from `audio.samples[0]`; the engine moves
    /// them onto the capture's clock and builds the segments.
    fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
        let Some(engine) = &self.engine else {
            return Ok(None);
        };
        let samples = if audio.sample_rate == SAMPLE_RATE {
            audio.samples.clone()
        } else {
            crate::dictation::try_resample_for_parakeet(&audio.samples, audio.sample_rate)
                .map_err(|error| anyhow::anyhow!(error))?
        };
        let (_, selection) = crate::app_settings::transcription_selection();
        let (text, words) = transcribe_words(engine, &selection, samples)
            .map_err(|error| anyhow::anyhow!("{error:#}"))?;
        Ok(Some(TranscriptData {
            source: "hex".into(),
            model: Some(selection.model.as_str().into()),
            text,
            words: corrected_words(words, word_lag(selection.model)),
            ..TranscriptData::default()
        }))
    }
}

/// One word and its times, in ms from the clip's start.
pub struct TimedWord {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// The clip's text and words on the dictation engine's warm model.
#[cfg(target_os = "macos")]
fn transcribe_words(
    engine: &Weak<TranscriptionEngine>,
    selection: &crate::transcription_models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<(String, Vec<TimedWord>)> {
    let transcript = crate::parakeet::transcribe_with_words(engine, selection, samples)?;
    let words = transcript
        .words
        .into_iter()
        .map(|word| TimedWord {
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            text: word.text,
        })
        .collect();
    Ok((transcript.text, words))
}

/// The clip's text and words, as one foreground request on the Linux
/// listener's engine: after the dictation already sent, never inside a live
/// stream. Its model is whatever the listener has loaded.
#[cfg(target_os = "linux")]
fn transcribe_words(
    engine: &Weak<TranscriptionEngine>,
    _selection: &crate::transcription_models::TranscriptionSelection,
    samples: Vec<f32>,
) -> Result<(String, Vec<TimedWord>)> {
    if samples.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    let (send, transcript) = mpsc::sync_channel(1);
    let offline: crate::transcription_engine::Offline = Box::new(move |model, samples| {
        let (text, words) = model
            .transcribe_words(&samples)
            .map_err(|error| format!("{error:#}"))?;
        let _ = send.send((text.clone(), words));
        Ok(text)
    });
    engine
        .upgrade()
        .ok_or_else(|| eyre!("the transcription engine stopped"))?
        .foreground(samples, offline)?
        .wait()
        .map_err(|error| eyre!(error))?;
    let (text, words) = transcript
        .try_recv()
        .map_err(|_| eyre!("the transcription returned no words"))?;
    let words = words
        .into_iter()
        .map(|(start_ms, end_ms, text)| TimedWord {
            start_ms,
            end_ms,
            text,
        })
        .collect();
    Ok((text, words))
}

/// How late a model's word times run, in ms: the median against Apple's
/// SpeechAnalyzer on 36 real captures (fitted on half, checked on the
/// other half; no drift, and the 30 s chunk seams cost nothing). Starts run
/// later after a pause.
#[derive(Debug, PartialEq)]
struct WordLag {
    start: i64,
    /// For a word whose gap since the previous word's end is at least
    /// `pause`, and for the first word.
    start_after_pause: i64,
    pause: i64,
    end: i64,
}

/// Each measured model with the pinned file it was measured on. A model or
/// file without an entry keeps its raw times: changing the pin drops the
/// correction until it is measured again (a test fails to say so).
const WORD_LAGS: &[(TranscriptionModelId, &str, WordLag)] = &[(
    TranscriptionModelId::ParakeetUnifiedEnglish,
    // parakeet-unified-en-0.6b-Q8_0.gguf, measured 2026-10-25.
    "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795",
    WordLag {
        start: 280,
        start_after_pause: 520,
        pause: 250,
        end: 195,
    },
)];

fn word_lag(model: TranscriptionModelId) -> Option<&'static WordLag> {
    let ModelRuntime::Gguf(artifact) = crate::transcription_models::definition(model).runtime
    else {
        return None;
    };
    WORD_LAGS
        .iter()
        .find(|(id, sha256, _)| *id == model && *sha256 == artifact.sha256)
        .map(|(_, _, lag)| lag)
}

/// The model's words in ms from the clip's start, moved earlier by `lag`.
/// Pauses are judged on the raw times. Times stay at 0 or later, each word
/// ends no earlier than it starts, and starts and ends never go backwards.
/// Neighbours may overlap a little, as Apple's times do: starts move further
/// than ends.
fn corrected_words(words: Vec<TimedWord>, lag: Option<&WordLag>) -> Vec<WordTiming> {
    let mut previous_end: Option<i64> = None;
    let (mut floor_t0, mut floor_t1) = (0, 0);
    words
        .into_iter()
        .map(|word| {
            let (mut t0, mut t1) = (word.start_ms, word.end_ms);
            if let Some(lag) = lag {
                let paused = previous_end.is_none_or(|end| t0 - end >= lag.pause);
                previous_end = Some(t1);
                t0 -= if paused {
                    lag.start_after_pause
                } else {
                    lag.start
                };
                t1 -= lag.end;
            }
            let t0 = t0.max(floor_t0);
            let t1 = t1.max(t0).max(floor_t1);
            (floor_t0, floor_t1) = (t0, t1);
            WordTiming {
                t0: t0 as u64,
                t1: t1 as u64,
                text: word.text,
            }
        })
        .collect()
}

/// Creates `dir` and missing parents owner-only, and makes `dir` itself
/// owner-only if it already existed.
fn private_dirs(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .wrap_err_with(|| format!("could not create {}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .wrap_err_with(|| format!("could not restrict {}", dir.display()))
}

/// A fresh owner-only `<captures>/<unix ms>.scrawl` folder; never adopts one
/// that exists. Stamps never repeat within the process: a discarded capture's
/// folder is removed on its collector's thread, so a switch in the same
/// millisecond could otherwise reuse its path while it is still unfinished.
fn new_bundle_dir(captures_dir: &Path) -> Result<PathBuf> {
    static LAST_STAMP: Mutex<u128> = Mutex::new(0);
    private_dirs(captures_dir)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let stamp = {
        let mut last = LAST_STAMP.lock().unwrap_or_else(|error| error.into_inner());
        *last = now.max(*last + 1);
        *last
    };
    for attempt in 0..100 {
        let name = match attempt {
            0 => format!("{stamp}.scrawl"),
            n => format!("{stamp}-{n}.scrawl"),
        };
        let dir = captures_dir.join(name);
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).wrap_err_with(|| format!("could not create {}", dir.display()));
            }
        }
    }
    Err(eyre!(
        "could not name a new capture in {}",
        captures_dir.display()
    ))
}

/// Makes everything in `dir` owner-only. Scrawl writes its manifest, audio,
/// and frames `0600`, but the movie, the window log, and folders still get
/// the process umask.
fn restrict_bundle(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let mode = if kind.is_dir() {
                pending.push(entry.path());
                0o700
            } else if kind.is_file() {
                0o600
            } else {
                continue;
            };
            if let Err(error) = fs::set_permissions(entry.path(), fs::Permissions::from_mode(mode))
            {
                tracing::warn!(%error, path = %entry.path().display(), "could not restrict a capture file");
            }
        }
    }
}

fn remove_bundle(dir: &Path) {
    match fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, bundle = %dir.display(), "could not remove a capture"),
    }
}

// ---------------------------------------------------------------------------
// The controller.

/// Where a capture's audio comes from: the dictation timeline.
pub trait AudioTimeline {
    fn open_sink(&self, sink: SyncSender<CaptureAudio>, since: CaptureInstant) -> Result<()>;
    fn close_sink(&self, through: CaptureInstant) -> Result<Option<CaptureSinkClosed>>;
}

#[cfg(target_os = "macos")]
impl AudioTimeline for DictationAudio {
    fn open_sink(&self, sink: SyncSender<CaptureAudio>, since: CaptureInstant) -> Result<()> {
        self.open_capture_sink(sink, since)
    }

    fn close_sink(&self, through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
        self.close_capture_sink(through)
    }
}

/// How the host ends the recording capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum End {
    /// Written in the background.
    Finish,
    /// Removed, settling as cancelled.
    Cancel,
    /// Removed without a trace, for another target ([`CaptureCommand::Discard`]).
    Discard,
}

/// How a capture's collector ends.
enum Seal {
    Finish {
        through: CaptureInstant,
        closed: Option<CaptureSinkClosed>,
    },
    Cancel,
}

struct Active {
    bundle: PathBuf,
    t0: CaptureInstant,
    target: TargetKind,
    seal: SyncSender<Seal>,
    collector: JoinHandle<()>,
    /// "While dictating", held until the capture stops.
    quiet: Option<Quiet>,
}

/// A stopped capture and its audio, waiting for the finisher.
struct FinishJob {
    bundle: PathBuf,
    capture: Box<dyn Sealed>,
    audio: RecordedAudio,
    /// What the router saw when it stopped, handed over once the job was
    /// queued; the sender is dropped without one when captures aren't sent.
    look: Receiver<routing::Look>,
}

// ---------------------------------------------------------------------------
// What views see: one snapshot of every capture, published on each change.

/// Every capture Hex knows about, as one value. A view (the capture
/// capsule, the menu bar, `hex capture`) reads it whole instead of reaching
/// into the host, and [`watch`] says when it changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaptureSnapshot {
    /// Bumped on every change, across hosts, for the life of the process.
    pub generation: u64,
    pub recording: Option<RecordingCapture>,
    /// Stopped captures still being written or cancelled, oldest first.
    pub finishing: Vec<FinishingCapture>,
    /// The most recent capture that finished, failed, or was cancelled.
    pub last: Option<SettledCapture>,
    /// The cabled iPhone's name, when one was found at the last look
    /// ([`CaptureCommand::FindPhone`]): the capsule's iPhone chip.
    pub phone: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingCapture {
    pub bundle: PathBuf,
    /// When it started, on the clock [`CaptureInstant::now`] reads.
    pub since: CaptureInstant,
    /// The engine started recording the screen; until then it is starting.
    pub screen: bool,
    pub target: TargetKind,
    /// The capture recorded a click, scroll, or ink stroke: the person got
    /// to work, so the capsule's switch chips fold.
    pub worked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinishingCapture {
    pub bundle: PathBuf,
    /// The engine's latest stage: `Stopping` until it reports another.
    pub stage: Stage,
    pub cancelling: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettledCapture {
    pub bundle: PathBuf,
    /// `Done`, `Failed`, or `Cancelled`.
    pub stage: Stage,
    /// A written capture's words, missing spans of audio, and why it has
    /// no transcript, if it has none.
    pub words: Option<usize>,
    pub gaps: Option<usize>,
    pub missing_transcript: Option<String>,
    /// Why a capture failed, when Hex knows.
    pub error: Option<String>,
    /// Where a written capture goes, while captures are sent to OpenCode
    /// (`capture.routing`'s `autoSend`).
    pub route: Option<CaptureRoute>,
}

impl SettledCapture {
    fn outcome(&self) -> ScrawlCaptureOutcome {
        ScrawlCaptureOutcome {
            bundle: self.bundle.clone(),
            stage: self.stage.label().into(),
            words: self.words,
            gaps: self.gaps,
            missing_transcript: self.missing_transcript.clone(),
        }
    }
}

/// The process's capture snapshot and those watching it.
struct Feed {
    snapshot: Mutex<CaptureSnapshot>,
    watchers: Mutex<Vec<SyncSender<()>>>,
}

impl Feed {
    const fn new() -> Self {
        Self {
            snapshot: Mutex::new(CaptureSnapshot {
                generation: 0,
                recording: None,
                finishing: Vec::new(),
                last: None,
                phone: None,
            }),
            watchers: Mutex::new(Vec::new()),
        }
    }

    fn publish(&self, mut next: CaptureSnapshot) {
        {
            let mut snapshot = self
                .snapshot
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            next.generation = snapshot.generation;
            if *snapshot == next {
                return;
            }
            next.generation += 1;
            *snapshot = next;
        }
        // A full wake-up is already pending; a gone watcher stops watching.
        self.watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|wake| !matches!(wake.try_send(()), Err(TrySendError::Disconnected(()))));
    }

    fn snapshot(&self) -> CaptureSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn watch(&'static self) -> CaptureWatch {
        let (wake, woken) = mpsc::sync_channel(1);
        let _ = wake.try_send(());
        self.watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(wake);
        CaptureWatch { feed: self, woken }
    }
}

static FEED: Feed = Feed::new();

/// The current capture snapshot.
#[expect(
    dead_code,
    reason = "the capture capsule subscribes next (scrawl-capsule)"
)]
pub fn snapshot() -> CaptureSnapshot {
    FEED.snapshot()
}

/// Watches the capture snapshot from any thread. Changes coalesce: a watcher
/// always gets the latest snapshot, never a stale one, but may skip the ones
/// between.
pub fn watch() -> CaptureWatch {
    FEED.watch()
}

pub struct CaptureWatch {
    feed: &'static Feed,
    woken: Receiver<()>,
}

impl CaptureWatch {
    /// The snapshot if it changed since the last call (the first call always
    /// returns it), without blocking.
    #[cfg_attr(not(test), expect(dead_code, reason = "the capsule waits instead"))]
    pub fn changed(&self) -> Option<CaptureSnapshot> {
        let mut woke = false;
        while self.woken.try_recv().is_ok() {
            woke = true;
        }
        woke.then(|| self.feed.snapshot())
    }

    /// Waits up to `timeout` for a change.
    pub fn wait(&self, timeout: Duration) -> Option<CaptureSnapshot> {
        self.woken.recv_timeout(timeout).ok()?;
        while self.woken.try_recv().is_ok() {}
        Some(self.feed.snapshot())
    }
}

/// The host's captures from start to outcome; every change publishes a
/// snapshot to the feed.
struct Outcomes {
    /// Every capture from its start until its outcome is settled: starting,
    /// recording, waiting to be written, writing, or cancelling. Each is
    /// settled exactly once, by its collector or by the finisher.
    unfinished: Vec<Unfinished>,
    last: Option<SettledCapture>,
    phone: Option<String>,
    /// The iPhone mirror ended on its own; the listener's next
    /// [`CaptureHost::reap`] finishes the capture.
    phone_ended: bool,
    feed: &'static Feed,
    /// A switch is between its discard and its start: views see neither
    /// half alone ([`CaptureCommand::Switch`]).
    held: bool,
}

struct Unfinished {
    bundle: PathBuf,
    since: CaptureInstant,
    /// `None` while it records; the stage once it was stopped or cancelled.
    stopped: Option<(Stage, bool)>,
    screen: bool,
    target: TargetKind,
    worked: bool,
    /// Dropped for another target: views never see it again, and it
    /// settles no outcome.
    discarded: bool,
}

impl Default for Outcomes {
    fn default() -> Self {
        Self::on(&FEED)
    }
}

impl Outcomes {
    fn on(feed: &'static Feed) -> Self {
        Self {
            unfinished: Vec::new(),
            last: None,
            phone: None,
            phone_ended: false,
            feed,
            held: false,
        }
    }

    fn idle(&self) -> bool {
        self.unfinished.is_empty()
    }

    /// Captures still being written or cancelled, which count against
    /// [`MAX_FINISHING`]; a discarded one is only being removed.
    fn writing(&self) -> usize {
        self.unfinished
            .iter()
            .filter(|unfinished| !unfinished.discarded)
            .count()
    }

    fn started(&mut self, bundle: PathBuf, since: CaptureInstant, target: TargetKind) {
        self.unfinished.push(Unfinished {
            bundle,
            since,
            stopped: None,
            screen: false,
            target,
            worked: false,
            discarded: false,
        });
        self.publish();
    }

    fn stopped(&mut self, bundle: &Path, end: End) {
        if let Some(unfinished) = self.find(bundle) {
            unfinished.stopped = Some((Stage::Stopping, end != End::Finish));
            unfinished.discarded = end == End::Discard;
            self.publish();
        }
    }

    fn worked(&mut self, bundle: &Path) {
        if let Some(unfinished) = self.find(bundle)
            && unfinished.stopped.is_none()
            && !unfinished.worked
        {
            unfinished.worked = true;
            self.publish();
        }
    }

    /// The engine's progress, which arrives late and out of band: it moves a
    /// capture on but never settles one (its collector or the finisher does).
    fn progress(&mut self, bundle: &Path, stage: Stage) {
        let Some(unfinished) = self.find(bundle) else {
            return;
        };
        match (stage, &mut unfinished.stopped) {
            (Stage::Recording, None) => unfinished.screen = true,
            (
                Stage::Stopping | Stage::Transcribing | Stage::Rendering | Stage::Writing,
                Some((current, false)),
            ) => *current = stage,
            _ => return,
        }
        self.publish();
    }

    fn settle(&mut self, bundle: PathBuf, stage: Stage, written: Option<Written>) {
        self.settle_with(bundle, stage, written, None, None);
    }

    fn fail(&mut self, bundle: PathBuf, error: String) {
        self.settle_with(bundle, Stage::Failed, None, Some(error), None);
    }

    /// A switch discarded `bundle` and then could not start: its outcome,
    /// since the discard settles none.
    fn fail_switch(&mut self, bundle: PathBuf, error: String) {
        self.last = Some(SettledCapture {
            bundle,
            stage: Stage::Failed,
            words: None,
            gaps: None,
            missing_transcript: None,
            error: Some(error),
            route: None,
        });
        self.publish();
    }

    fn phone(&mut self, phone: Option<String>) {
        if self.phone != phone {
            self.phone = phone;
            self.publish();
        }
    }

    /// A written capture's route changed; only the newest capture's shows.
    fn routed(&mut self, bundle: &Path, route: CaptureRoute) {
        if let Some(last) = self.last.as_mut()
            && last.bundle == bundle
            && last.route.as_ref() != Some(&route)
        {
            last.route = Some(route);
            self.publish();
        }
    }

    fn settle_with(
        &mut self,
        bundle: PathBuf,
        stage: Stage,
        written: Option<Written>,
        error: Option<String>,
        route: Option<CaptureRoute>,
    ) {
        let discarded = self
            .find(&bundle)
            .is_some_and(|unfinished| unfinished.discarded);
        self.unfinished
            .retain(|unfinished| unfinished.bundle != bundle);
        if !discarded {
            self.last = Some(SettledCapture {
                bundle,
                stage,
                words: written.as_ref().map(|written| written.words),
                gaps: written.as_ref().map(|written| written.gaps),
                missing_transcript: written.and_then(|written| written.missing_transcript),
                error,
                route,
            });
        }
        self.publish();
    }

    fn find(&mut self, bundle: &Path) -> Option<&mut Unfinished> {
        self.unfinished
            .iter_mut()
            .find(|unfinished| unfinished.bundle == bundle)
    }

    fn snapshot(&self) -> CaptureSnapshot {
        let mut snapshot = CaptureSnapshot {
            last: self.last.clone(),
            phone: self.phone.clone(),
            ..CaptureSnapshot::default()
        };
        for unfinished in self
            .unfinished
            .iter()
            .filter(|unfinished| !unfinished.discarded)
        {
            match unfinished.stopped {
                None => {
                    snapshot.recording = Some(RecordingCapture {
                        bundle: unfinished.bundle.clone(),
                        since: unfinished.since,
                        screen: unfinished.screen,
                        target: unfinished.target,
                        worked: unfinished.worked,
                    });
                }
                Some((stage, cancelling)) => snapshot.finishing.push(FinishingCapture {
                    bundle: unfinished.bundle.clone(),
                    stage,
                    cancelling,
                }),
            }
        }
        snapshot
    }

    fn publish(&self) {
        if !self.held {
            self.feed.publish(self.snapshot());
        }
    }

    /// Ends a hold, publishing what changed during it.
    fn release(&mut self) {
        self.held = false;
        self.publish();
    }
}

/// The host that last started, for engine progress, which arrives on the
/// process-wide `capture-events` thread.
static LIVE_OUTCOMES: Mutex<Weak<Mutex<Outcomes>>> = Mutex::new(Weak::new());

type SharedOutcomes = Arc<Mutex<Outcomes>>;

fn lock(outcomes: &Mutex<Outcomes>) -> std::sync::MutexGuard<'_, Outcomes> {
    outcomes.lock().unwrap_or_else(|error| error.into_inner())
}

/// The `hex capture` controller, owned by the listener beside its
/// `DictationAudio`. The recording capture gets a `capture-audio` thread
/// that starts the engine (off the listener's thread) and collects its
/// audio, and stops recording the screen the moment the capture is sealed;
/// one `capture-finish` thread then writes stopped captures one at a time.
/// One capture records at a time.
pub struct CaptureHost {
    engine: Arc<dyn CaptureEngine>,
    /// The folder for new captures; `None` reads `capture.folder` at each
    /// start.
    captures_dir: Option<PathBuf>,
    active: Option<Active>,
    /// The recording capture's input, once its engine started.
    input: LiveInput,
    retention: Option<(SyncSender<()>, JoinHandle<()>)>,
    outcomes: SharedOutcomes,
    finisher: Option<(SyncSender<FinishJob>, JoinHandle<()>)>,
    /// The `capture-route` thread.
    routes: Option<routing::Routes>,
    /// Collectors of ended captures, until they exit.
    ended: Vec<JoinHandle<()>>,
    /// A `capture-phone` thread is looking for an iPhone.
    looking: Arc<AtomicBool>,
    /// Mutes or pauses other audio while a capture records; `None` (a
    /// replay) leaves it alone.
    environment: Option<Environment>,
    /// Tests pin the policy; captures otherwise follow "While dictating".
    #[cfg(test)]
    audio_behavior: Option<crate::app_settings::RecordingAudioBehavior>,
}

impl CaptureHost {
    /// The host when captures are on (the `capture` experiment and
    /// `capture.enabled`); otherwise `None`, and no engine, thread, sink, or
    /// folder is created.
    pub fn start(
        engine: Option<Arc<TranscriptionEngine>>,
        environment: &Environment,
    ) -> Option<Self> {
        if !crate::app_settings::capture_on() {
            return None;
        }
        tracing::info!("capture experiment on: `hex capture` is available");
        let host = shared_engine(engine.as_ref()).and_then(|engine| {
            let mut host = Self::with(engine, None)?;
            host.environment = Some(environment.clone());
            host.start_retention()?;
            Ok(host)
        });
        host.inspect_err(|error| tracing::warn!(%error, "could not start the capture host"))
            .ok()
    }

    /// Starts or stops the host as captures are turned on and off. Turning
    /// them off cancels a recording capture and stops every capture thread.
    pub fn reconcile(
        host: &mut Option<Self>,
        engine: Option<&Arc<TranscriptionEngine>>,
        environment: &Environment,
        audio: &impl AudioTimeline,
    ) {
        static RETRY_AT: Mutex<Option<Instant>> = Mutex::new(None);
        watch_scrawl();
        match (host.is_some(), crate::app_settings::capture_on()) {
            (false, true) => {
                let mut retry_at = RETRY_AT.lock().unwrap_or_else(|error| error.into_inner());
                if retry_at.is_some_and(|at| Instant::now() < at) {
                    return;
                }
                *host = Self::start(engine.cloned(), environment);
                *retry_at = host.is_none().then(|| Instant::now() + START_RETRY);
            }
            (false, false) => {
                *RETRY_AT.lock().unwrap_or_else(|error| error.into_inner()) = None;
            }
            (true, false) => {
                if let Some(mut stopped) = host.take() {
                    tracing::info!("captures turned off");
                    stopped.shutdown(audio);
                }
            }
            (true, true) => {
                if let Some(host) = host {
                    host.reap(CaptureInstant::now(), audio);
                }
            }
        }
    }

    fn with(engine: Arc<dyn CaptureEngine>, captures_dir: Option<PathBuf>) -> Result<Self> {
        let outcomes = SharedOutcomes::default();
        *LIVE_OUTCOMES
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(&outcomes);
        let routes = routing::Routes::start(outcomes.clone())?;
        let (jobs, queued) = mpsc::sync_channel(MAX_FINISHING);
        let finisher = {
            let outcomes = outcomes.clone();
            let routes = routes.jobs();
            thread::Builder::new()
                .name("capture-finish".into())
                .spawn(move || finish_captures(queued, &outcomes, &routes))?
        };
        Ok(Self {
            engine,
            captures_dir,
            active: None,
            input: LiveInput::shared()?,
            retention: None,
            outcomes,
            finisher: Some((jobs, finisher)),
            routes: Some(routes),
            ended: Vec::new(),
            looking: Arc::default(),
            environment: None,
            #[cfg(test)]
            audio_behavior: None,
        })
    }

    /// Whether a capture is recording, and since when. A capture whose
    /// engine failed to start is ended first, so it never turns the next
    /// shortcut press into a stop.
    pub fn recording_since(&mut self, audio: &impl AudioTimeline) -> Option<CaptureInstant> {
        self.reap(CaptureInstant::now(), audio);
        self.active.as_ref().map(|active| active.t0)
    }

    /// What the host is doing for the capture shortcut pressed at `at`.
    pub fn shortcut_activity(
        &mut self,
        at: CaptureInstant,
        audio: &impl AudioTimeline,
    ) -> ShortcutActivity {
        if picking() {
            return ShortcutActivity::Picking;
        }
        match self.recording_since(audio) {
            Some(t0) => ShortcutActivity::Recording {
                elapsed: at.checked_duration_since(t0).unwrap_or_default(),
            },
            None => ShortcutActivity::Free,
        }
    }

    /// Carries out a capture shortcut action ([`CaptureShortcut`]), with the
    /// error tone when it does nothing.
    pub fn shortcut(&mut self, action: ShortcutAction, audio: &impl AudioTimeline) -> ControlReply {
        let action = match action {
            ShortcutAction::Record => ScrawlCaptureAction::Start,
            ShortcutAction::Finish => ScrawlCaptureAction::Stop,
            ShortcutAction::Pick => {
                capsule_key(CapsuleKey::Shortcut);
                return self.report(false, None);
            }
            ShortcutAction::None | ShortcutAction::Cancel => return self.report(false, None),
        };
        let reply = self.control(action, audio);
        if let ControlReply::Error { code, message, .. } = &reply {
            tracing::warn!(code, message, "the capture shortcut did nothing");
            #[cfg(target_os = "macos")]
            crate::feedback::play(crate::feedback::Tone::Error);
            #[cfg(not(target_os = "macos"))]
            crate::feedback::play(crate::feedback::Tone::CaptureFailed);
        }
        reply
    }

    /// The `capture-retention` thread: Scrawl's cleanup of the captures
    /// folder with `capture.*`'s policy, a while after the host starts and
    /// hourly, never while a capture records or is written, and never while
    /// Scrawl's own app runs (it cleans the folder itself).
    fn start_retention(&mut self) -> Result<()> {
        let (nudge, nudged) = mpsc::sync_channel(1);
        let outcomes = self.outcomes.clone();
        let thread = thread::Builder::new()
            .name("capture-retention".into())
            .spawn(move || {
                let mut wait = RETENTION_FIRST;
                loop {
                    match nudged.recv_timeout(wait) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                    if !lock(&outcomes).idle() || crate::app_settings::scrawl_app_running() {
                        wait = Duration::from_secs(60);
                        continue;
                    }
                    let settings = crate::app_settings::capture_settings();
                    let captures = settings.captures_dir();
                    if captures.is_dir() {
                        scrawl_capture::retention::run_pass(
                            &captures,
                            &settings.retention(),
                            false,
                            Duration::ZERO,
                            false,
                        );
                    }
                    wait = RETENTION_EVERY;
                }
            })?;
        self.retention = Some((nudge, thread));
        Ok(())
    }

    pub fn control(
        &mut self,
        action: ScrawlCaptureAction,
        audio: &impl AudioTimeline,
    ) -> ControlReply {
        let now = CaptureInstant::now();
        self.reap(now, audio);
        let result = match action {
            ScrawlCaptureAction::Start => self.begin(now, audio, CaptureTarget::Pointer, None),
            ScrawlCaptureAction::Stop => self.end(now, audio, End::Finish),
            ScrawlCaptureAction::Cancel => self.end(now, audio, End::Cancel),
            ScrawlCaptureAction::Status => Ok(self.report(false, None)),
        };
        result.unwrap_or_else(|error| ControlReply::error("failed", format!("{error:#}")))
    }

    /// Carries out the capture capsule's [`CaptureCommand`].
    pub fn command(&mut self, command: CaptureCommand, audio: &impl AudioTimeline) -> ControlReply {
        let now = CaptureInstant::now();
        self.reap(now, audio);
        let result = match command {
            CaptureCommand::Start(target) => self.begin(now, audio, target, None),
            CaptureCommand::FindPhone => {
                self.find_phone();
                Ok(self.report(false, None))
            }
            CaptureCommand::Discard => self.end(now, audio, End::Discard),
            CaptureCommand::Switch(target) => self.switch(now, audio, target),
            CaptureCommand::Send { to, always } => {
                Ok(self.route(routing::Job::Send { to, always }))
            }
            CaptureCommand::Undo => Ok(self.route(routing::Job::Undo)),
            CaptureCommand::Paste => self.hand_off(HandOff::Paste),
            CaptureCommand::Inspect => self.hand_off(HandOff::Inspect),
        };
        result.unwrap_or_else(|error| ControlReply::error("failed", format!("{error:#}")))
    }

    /// Cancels a recording capture, as when Hex quits, and waits a bounded
    /// time for its cleanup and for captures still writing. Quitting stops
    /// their transcription, so they keep their audio without words.
    pub fn shutdown(&mut self, audio: &impl AudioTimeline) {
        let deadline = Instant::now() + SHUTDOWN_WAIT;
        if self.active.is_some() {
            let _ = self.end(CaptureInstant::now(), audio, End::Cancel);
        }
        for collector in std::mem::take(&mut self.ended) {
            join_by(collector, deadline, "capture-audio");
        }
        // Dropping the queue's sender ends the finisher after its last job.
        if let Some((jobs, finisher)) = self.finisher.take() {
            drop(jobs);
            join_by(finisher, deadline, "capture-finish");
        }
        if let Some(routes) = self.routes.take() {
            routes.stop(deadline);
        }
        if let Some((nudge, retention)) = self.retention.take() {
            drop(nudge);
            join_by(retention, deadline, "capture-retention");
        }
    }

    /// Looks for a cabled iPhone on a `capture-phone` thread, one look at a
    /// time, and publishes what it finds as [`CaptureSnapshot::phone`].
    fn find_phone(&self) {
        if self.looking.swap(true, Ordering::AcqRel) {
            return;
        }
        let looking = self.looking.clone();
        let engine = self.engine.clone();
        let outcomes = self.outcomes.clone();
        let spawned = thread::Builder::new()
            .name("capture-phone".into())
            .spawn(move || {
                let phone = engine
                    .phones()
                    .inspect_err(|error| tracing::debug!(%error, "could not look for an iPhone"))
                    .ok()
                    .and_then(|phones| phones.into_iter().next());
                lock(&outcomes).phone(phone);
                looking.store(false, Ordering::Release);
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not look for an iPhone");
            self.looking.store(false, Ordering::Release);
        }
    }

    /// Ends a capture whose engine failed to start: its collector exited on
    /// its own and settled the failure. Finishes an iPhone capture whose
    /// mirror ended on its own (unplugged, never found, or no camera
    /// access): what it recorded is written, or why it recorded nothing
    /// fails it.
    fn reap(&mut self, now: CaptureInstant, audio: &impl AudioTimeline) {
        if std::mem::take(&mut lock(&self.outcomes).phone_ended)
            && self
                .active
                .as_ref()
                .is_some_and(|active| active.target == TargetKind::Phone)
        {
            tracing::info!("the iPhone mirror ended; finishing its capture");
            let _ = self.end(now, audio, End::Finish);
            return;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.collector.is_finished())
            && let Some(active) = self.active.take()
        {
            self.input.recording(None);
            let _ = audio.close_sink(now);
            self.ended.push(active.collector);
        }
    }

    /// Discards the recording capture and starts `target` in one published
    /// step. Refused before the discard when no capture could start, so the
    /// recording goes on; a start that fails after it settles `Failed`.
    fn switch(
        &mut self,
        now: CaptureInstant,
        audio: &impl AudioTimeline,
        target: CaptureTarget,
    ) -> Result<ControlReply> {
        if let Some(refused) = self.refuse_start() {
            return Ok(refused);
        }
        lock(&self.outcomes).held = true;
        let discarded = self.active.as_ref().map(|active| active.bundle.clone());
        // Output stays muted or media paused from one capture to the next.
        let quiet = self.active.as_mut().and_then(|active| active.quiet.take());
        let switched = self
            .end(now, audio, End::Discard)
            .and_then(|_| self.begin(now, audio, target, quiet));
        let mut outcomes = lock(&self.outcomes);
        if let (Some(bundle), true) = (discarded, self.active.is_none()) {
            let error = match &switched {
                Err(error) => format!("{error:#}"),
                Ok(ControlReply::Error { message, .. }) => message.clone(),
                Ok(_) => "the capture could not start".into(),
            };
            outcomes.fail_switch(bundle, error);
        }
        outcomes.release();
        switched
    }

    /// The session that applies "While dictating" to a capture starting
    /// now. Sending it never blocks: the environment's own thread mutes or
    /// pauses.
    fn quiet(&self) -> Option<Quiet> {
        let environment = self.environment.as_ref()?;
        #[cfg(test)]
        if let Some(behavior) = self.audio_behavior {
            return Some(environment.begin_capture(behavior));
        }
        let behavior = crate::app_settings::recording_audio_behavior();
        Some(environment.begin_capture(behavior))
    }

    /// Why no capture can start now, besides one already recording.
    fn refuse_start(&self) -> Option<ControlReply> {
        if self.finisher.is_none() {
            return Some(ControlReply::error("busy", "Hex is stopping"));
        }
        // Every unfinished capture but the recording one is stopped and
        // writing or about to be.
        let writing = lock(&self.outcomes)
            .writing()
            .saturating_sub(usize::from(self.active.is_some()));
        (writing >= MAX_FINISHING).then(|| {
            ControlReply::error(
                "busy",
                format!("{writing} captures are still being written; try again shortly"),
            )
        })
    }

    /// Starts a capture, holding "While dictating" with `quiet` when a
    /// switch carries the previous capture's session over.
    fn begin(
        &mut self,
        now: CaptureInstant,
        audio: &impl AudioTimeline,
        target: CaptureTarget,
        quiet: Option<Quiet>,
    ) -> Result<ControlReply> {
        if self.active.is_some() {
            return Ok(self.report(false, None));
        }
        if let Some(refused) = self.refuse_start() {
            return Ok(refused);
        }
        let Some((jobs, _)) = &self.finisher else {
            return Ok(ControlReply::error("busy", "Hex is stopping"));
        };
        self.ended.retain(|collector| !collector.is_finished());
        let captures_dir = self
            .captures_dir
            .clone()
            .unwrap_or_else(|| crate::app_settings::capture_settings().captures_dir());
        let bundle = new_bundle_dir(&captures_dir)?;
        let (sink, chunks) = mpsc::sync_channel(SINK_CAPACITY);
        if let Err(error) = audio.open_sink(sink, now) {
            remove_bundle(&bundle);
            return Err(error);
        }
        // Unfinished from here until its collector or the finisher settles
        // it, whatever stop or cancel does meanwhile.
        lock(&self.outcomes).started(bundle.clone(), now, target.kind());
        self.input.recording(Some(&bundle));
        let (seal, sealed) = mpsc::sync_channel(1);
        let collector = {
            let engine = self.engine.clone();
            let recording = bundle.clone();
            let jobs = jobs.clone();
            let outcomes = self.outcomes.clone();
            let input = self.input.clone();
            thread::Builder::new()
                .name("capture-audio".into())
                .spawn(move || {
                    let start = engine
                        .target(target)
                        .map(|target| start_request(now, recording.clone(), target));
                    collect(
                        &*engine, start, &recording, chunks, sealed, &jobs, &outcomes, &input,
                    );
                })
                .inspect_err(|_| {
                    let _ = audio.close_sink(now);
                    remove_bundle(&bundle);
                    lock(&self.outcomes).settle(bundle.clone(), Stage::Failed, None);
                })?
        };
        tracing::info!(bundle = %bundle.display(), ?target, "capture started");
        self.active = Some(Active {
            bundle,
            t0: now,
            target: target.kind(),
            seal,
            collector,
            quiet: quiet.or_else(|| self.quiet()),
        });
        self.find_phone();
        Ok(self.report(true, None))
    }

    fn end(
        &mut self,
        now: CaptureInstant,
        audio: &impl AudioTimeline,
        end: End,
    ) -> Result<ControlReply> {
        let Some(mut active) = self.active.take() else {
            return Ok(self.report(false, None));
        };
        // Input after the stop belongs to no capture.
        self.input.recording(None);
        drop(active.quiet.take());
        let closed = audio.close_sink(now).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not close the capture sink");
            None
        });
        let seal = match end {
            End::Finish => Seal::Finish {
                through: now,
                closed,
            },
            End::Cancel | End::Discard => Seal::Cancel,
        };
        lock(&self.outcomes).stopped(&active.bundle, end);
        let _ = active.seal.send(seal);
        self.ended.push(active.collector);
        tracing::info!(bundle = %active.bundle.display(), ?end, "capture ended");
        let ended = (end != End::Discard).then_some(active.bundle);
        Ok(self.report(true, ended))
    }

    /// Pastes or opens the newest written capture on a thread of its own,
    /// one at a time: pasting waits on the pasteboard and the app in front.
    fn hand_off(&self, how: HandOff) -> Result<ControlReply> {
        let bundle = lock(&self.outcomes)
            .last
            .as_ref()
            .filter(|last| last.stage == Stage::Done)
            .map(|last| last.bundle.clone())
            .ok_or_else(|| eyre!("no written capture"))?;
        if !cfg!(test) {
            static HANDING_OFF: AtomicBool = AtomicBool::new(false);
            if HANDING_OFF.swap(true, Ordering::AcqRel) {
                return Ok(self.report(false, None));
            }
            let spawned = std::thread::Builder::new()
                .name("capture-hand-off".into())
                .spawn(move || {
                    let done = hand_off(how, &bundle);
                    if let Err(error) = done {
                        tracing::warn!(
                            error = format!("{error:#}"),
                            ?how,
                            "capture hand-off failed"
                        );
                    }
                    HANDING_OFF.store(false, Ordering::Release);
                });
            if let Err(error) = spawned {
                HANDING_OFF.store(false, Ordering::Release);
                return Err(error).wrap_err("could not start the capture hand-off");
            }
        }
        Ok(self.report(true, None))
    }

    /// Hands the capsule's route command to the `capture-route` thread,
    /// which acts on the newest capture.
    fn route(&self, job: routing::Job) -> ControlReply {
        if !self.routes.as_ref().is_some_and(|routes| routes.send(job)) {
            return ControlReply::error("busy", "the capture router is not taking requests");
        }
        self.report(true, None)
    }

    fn report(&self, changed: bool, ended: Option<PathBuf>) -> ControlReply {
        let outcomes = lock(&self.outcomes);
        let (state, bundle, elapsed_ms) = match &self.active {
            Some(active) => (
                ScrawlCaptureState::Recording,
                Some(active.bundle.clone()),
                Some(CaptureInstant::now().duration_since(active.t0).as_millis() as u64),
            ),
            None => (ScrawlCaptureState::Idle, ended, None),
        };
        ControlReply::ScrawlCapture(Box::new(ScrawlCaptureReport {
            changed,
            state,
            bundle,
            elapsed_ms,
            finishing: outcomes
                .unfinished
                .iter()
                .filter(|unfinished| {
                    !unfinished.discarded
                        && self
                            .active
                            .as_ref()
                            .is_none_or(|active| active.bundle != unfinished.bundle)
                })
                .map(|unfinished| unfinished.bundle.clone())
                .collect(),
            last: outcomes.last.as_ref().map(SettledCapture::outcome),
        }))
    }
}

/// While captures are on, notes every couple of seconds whether Scrawl's own
/// app runs: it then owns the capture shortcut, and Hex doesn't match it.
fn watch_scrawl() {
    const EVERY: Duration = Duration::from_secs(2);
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if !crate::app_settings::capture_on() {
        crate::app_settings::set_scrawl_app_running(false);
        return;
    }
    let mut last = LAST.lock().unwrap_or_else(|error| error.into_inner());
    if last.is_some_and(|at| at.elapsed() < EVERY) {
        return;
    }
    *last = Some(Instant::now());
    let running = scrawl_running();
    if running != crate::app_settings::scrawl_app_running() {
        tracing::info!(
            running,
            "Scrawl's app owns the capture shortcut while it runs"
        );
    }
    crate::app_settings::set_scrawl_app_running(running);
}

/// Whether Scrawl's own app (`dev.kitlangton.scrawl`) runs. Safe off the
/// main thread.
pub fn scrawl_running() -> bool {
    #[cfg(target_os = "macos")]
    return objc2::rc::autoreleasepool(|_| scrawl_capture::capture::is_scrawl_app_running());
    // Scrawl's app is macOS's.
    #[cfg(not(target_os = "macos"))]
    false
}

/// Asks Scrawl's own app to quit, as "Use Hex for captures" does.
#[cfg(target_os = "macos")]
pub fn quit_scrawl() {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| {
        for app in NSRunningApplication::runningApplicationsWithBundleIdentifier(
            &NSString::from_str(scrawl_capture::capture::SCRAWL_BUNDLE_ID),
        ) {
            if !app.terminate() {
                tracing::warn!("Scrawl did not agree to quit");
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Input from Hex's event tap.

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// What the session tap reports while a capture records.
#[derive(Clone, Debug)]
pub enum Tapped {
    /// A pointer, scroll, or modifier event, in global top-left points.
    Pointer(HostInput),
    /// A fresh key press (never an autorepeat) and the modifiers held.
    Key { code: u16, flags: u64 },
}

/// The capture is recording and its engine took input: the tap reports.
static TAP_RECORDING: AtomicBool = AtomicBool::new(false);
static TAP_INPUT: OnceLock<SyncSender<(CaptureInstant, Tapped)>> = OnceLock::new();
static DROPPED_INPUT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
thread_local! {
    static TEST_TAP: std::cell::RefCell<Option<Vec<Tapped>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` on this thread as if a capture recorded, and returns what the
/// tap reported to it.
#[cfg(all(test, target_os = "macos"))]
pub fn tapped_while(f: impl FnOnce()) -> Vec<Tapped> {
    TEST_TAP.set(Some(Vec::new()));
    f();
    TEST_TAP.take().unwrap_or_default()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Whether the session tap should report pointer and key input: only while
/// a capture records.
pub fn wants_input() -> bool {
    #[cfg(test)]
    if TEST_TAP.with_borrow(Option::is_some) {
        return true;
    }
    TAP_RECORDING.load(Ordering::Acquire)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Called on the event tap's thread; never blocks. A full queue drops the
/// event (counted, logged once the capture stops).
pub fn report_input(at: CaptureInstant, input: Tapped) {
    #[cfg(test)]
    if TEST_TAP
        .with_borrow_mut(|tapped| tapped.as_mut().map(|tapped| tapped.push(input.clone())))
        .is_some()
    {
        return;
    }
    if !wants_input() {
        return;
    }
    if let Some(sender) = TAP_INPUT.get()
        && sender.try_send((at, input)).is_err()
    {
        DROPPED_INPUT.fetch_add(1, Ordering::Relaxed);
    }
}

/// The modifiers in a CGEvent's flags.
pub fn modifiers(flags: u64) -> Modifiers {
    const SHIFT: u64 = 0x0002_0000;
    const CONTROL: u64 = 0x0004_0000;
    const OPTION: u64 = 0x0008_0000;
    const COMMAND: u64 = 0x0010_0000;
    Modifiers {
        command: flags & COMMAND != 0,
        control: flags & CONTROL != 0,
        option: flags & OPTION != 0,
        shift: flags & SHIFT != 0,
    }
}

/// The recording capture's input target, shared by the host (which says
/// which capture records), its collector (which attaches the engine's sink
/// once the engine started), and the process's one `capture-input` thread.
#[derive(Clone)]
struct LiveInput(Arc<Mutex<InputState>>);

static SHARED_INPUT: Mutex<Option<LiveInput>> = Mutex::new(None);

/// The recording capture's ink, laser, and pointer, for the ink overlay to
/// draw: `None` until its engine started, and once it ended.
pub fn ink_source() -> Option<InkSource> {
    let shared = SHARED_INPUT
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let state = shared.as_ref()?.state();
    state.target.as_ref().map(|target| InkSource {
        telemetry: target.sink.telemetry().clone(),
        t0: target.t0,
    })
}

/// Where a recording capture keeps its strokes, and its clock.
#[derive(Clone)]
pub struct InkSource {
    pub telemetry: Arc<SharedCaptureTelemetry>,
    t0: CaptureInstant,
}

impl InkSource {
    /// Now on the strokes' clock: milliseconds since the capture started.
    pub fn now_ms(&self) -> u64 {
        CaptureInstant::now()
            .checked_duration_since(self.t0)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[derive(Default)]
struct InputState {
    recording: Option<PathBuf>,
    target: Option<InputTarget>,
}

struct InputTarget {
    bundle: PathBuf,
    sink: InputSink,
    t0: CaptureInstant,
    rules: HostedInput,
    /// Told once of the first click, scroll, or stroke
    /// ([`RecordingCapture::worked`]).
    outcomes: Option<SharedOutcomes>,
}

impl LiveInput {
    /// The process's one target and its `capture-input` thread.
    fn shared() -> Result<Self> {
        let mut shared = SHARED_INPUT
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(live) = &*shared {
            return Ok(live.clone());
        }
        let live = Self(Arc::default());
        let (sender, received) = mpsc::sync_channel(INPUT_CAPACITY);
        let forward = live.clone();
        thread::Builder::new()
            .name("capture-input".into())
            .spawn(move || forward.forward(received))?;
        let _ = TAP_INPUT.set(sender);
        *shared = Some(live.clone());
        Ok(live)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, InputState> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// The host started (`Some`) or ended a capture.
    fn recording(&self, bundle: Option<&Path>) {
        let mut state = self.state();
        state.recording = bundle.map(Path::to_path_buf);
        state.target = None;
        TAP_RECORDING.store(false, Ordering::Release);
        let dropped = DROPPED_INPUT.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            tracing::warn!(dropped, "capture input queue was full; dropped input");
        }
    }

    /// The engine for `bundle` started; input flows unless it already ended.
    /// `shift_held` is ⇧ now: left from the chord that started it (⌥⇧S) or
    /// held before it, so it inks only once it came up.
    fn attach(
        &self,
        bundle: &Path,
        sink: InputSink,
        t0: CaptureInstant,
        shift_held: bool,
        outcomes: &SharedOutcomes,
    ) {
        let mut state = self.state();
        if state.recording.as_deref() != Some(bundle) {
            return;
        }
        state.target = Some(InputTarget {
            bundle: bundle.to_path_buf(),
            sink,
            t0,
            rules: HostedInput::with_shift_held(shift_held),
            outcomes: Some(outcomes.clone()),
        });
        TAP_RECORDING.store(true, Ordering::Release);
    }

    fn detach(&self, bundle: &Path) {
        let mut state = self.state();
        if state
            .target
            .as_ref()
            .is_some_and(|target| target.bundle == bundle)
        {
            state.target = None;
            TAP_RECORDING.store(false, Ordering::Release);
        }
    }

    /// The `capture-input` thread: the engine's pure input rules turn each
    /// tap event into the event Scrawl's own taps would have sent, on the
    /// capture's clock; the engine then applies its ink, pin, and
    /// own-window rules (Hex's windows are this process's).
    fn forward(&self, received: Receiver<(CaptureInstant, Tapped)>) {
        let mut pill = CapsulePill::default();
        for (at, tapped) in received {
            let mut state = self.state();
            let Some(target) = state.target.as_mut() else {
                continue;
            };
            let t_ms = at
                .checked_duration_since(target.t0)
                .map_or(0, |since| since.as_millis() as u64);
            let input = match tapped {
                Tapped::Pointer(input) => input,
                #[cfg(target_os = "macos")]
                Tapped::Key { code, flags } => HostInput::Key {
                    name: scrawl_capture::hosted_input::key_name(
                        code,
                        crate::keyboard::command_character,
                    ),
                    z: crate::keyboard::key_code_for('z').is_ok_and(|z| z == code) || code == KEY_Z,
                    modifiers: modifiers(flags),
                },
                // Wayland reports no keys to Hex outside its own windows.
                #[cfg(not(target_os = "macos"))]
                Tapped::Key { .. } => continue,
            };
            let Some(event) = target.rules.translate(t_ms, input) else {
                continue;
            };
            // A press or scroll on the capsule's pill is Hex's; anywhere
            // else on its panel it's the app's beneath.
            if let scrawl_capture::capture::InputEvent::ButtonDown { x, y, .. }
            | scrawl_capture::capture::InputEvent::Scroll { x, y, .. } = event
                && pill.contains(x, y)
            {
                continue;
            }
            let worked = matches!(
                event,
                scrawl_capture::capture::InputEvent::ButtonDown { .. }
                    | scrawl_capture::capture::InputEvent::Scroll { .. }
            )
            .then(|| {
                target
                    .outcomes
                    .take()
                    .map(|outcomes| (outcomes, target.bundle.clone()))
            })
            .flatten();
            target.sink.send(event);
            drop(state);
            if let Some((outcomes, bundle)) = worked {
                lock(&outcomes).worked(&bundle);
            }
        }
    }
}

/// What the capture host is doing when the capture shortcut goes down.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutActivity {
    Free,
    /// The capsule's picker is open ([`CapsuleKeys::Picker`]).
    Picking,
    Recording {
        elapsed: Duration,
    },
}

/// A capture command from the capsule, for the listener that owns the host.
#[derive(Debug)]
pub enum CaptureControl {
    Scrawl(ScrawlCaptureAction),
    Command(CaptureCommand),
}

/// Hands `control` to the listener without waiting for it.
#[cfg(target_os = "macos")]
pub fn send_to_listener(control: CaptureControl) -> Result<()> {
    use crate::recognition::RecognitionControl;
    let runtime = crate::macos_rune::runtime().ok_or_else(|| eyre!("Hex is starting"))?;
    let (reply, _) = mpsc::sync_channel(1);
    runtime
        .recognition(match control {
            CaptureControl::Scrawl(action) => RecognitionControl::ScrawlCapture { action, reply },
            CaptureControl::Command(command) => {
                RecognitionControl::CaptureCommand { command, reply }
            }
        })
        .map_err(|error| eyre!(error))
}

/// The Linux listener's queue of capsule commands, while one listens.
#[cfg(target_os = "linux")]
static LISTENER: Mutex<Option<SyncSender<CaptureControl>>> = Mutex::new(None);

/// Hands `control` to the listener without waiting for it.
#[cfg(target_os = "linux")]
pub fn send_to_listener(control: CaptureControl) -> Result<()> {
    LISTENER
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .ok_or_else(|| eyre!("Hex is not listening"))?
        .try_send(control)
        .map_err(|_| eyre!("Hex is busy; try again"))
}

/// Makes the calling listener the one capsule commands reach.
#[cfg(target_os = "linux")]
pub fn listen_for_controls() -> Receiver<CaptureControl> {
    let (send, receive) = mpsc::sync_channel(16);
    *LISTENER.lock().unwrap_or_else(|error| error.into_inner()) = Some(send);
    receive
}

impl ShortcutActivity {
    /// What a press of a shortcut the desktop reports only as pressed (the
    /// Linux portal and keybindings) asks for: it records at once.
    #[cfg(target_os = "linux")]
    pub fn on_press(self) -> ShortcutAction {
        match self {
            Self::Free => ShortcutAction::Record,
            Self::Picking => ShortcutAction::Pick,
            Self::Recording { elapsed } if elapsed < SHORTCUT_DEBOUNCE => ShortcutAction::None,
            Self::Recording { .. } => ShortcutAction::Finish,
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// What a capture shortcut event asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutAction {
    None,
    /// The pressed shortcut's keys came up: record the display under the
    /// pointer.
    Record,
    /// Escape while the pressed shortcut's keys were still down: nothing
    /// records.
    Cancel,
    Finish,
    /// The picker is open: record what it is on ([`CapsuleKey::Shortcut`]).
    Pick,
}

/// The capture shortcut in the listener's input, as Scrawl has it: a fresh
/// press while nothing records arms it, and the capture starts once one of
/// its modifiers comes up, however long it was held, so nothing records
/// while ⌥⇧ is down (⇧ is ink). Escape before then records nothing, and
/// never ends a recording. A press while
/// recording finishes, except within [`SHORTCUT_DEBOUNCE`] of the start;
/// autorepeats never count.
#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct CaptureShortcut {
    held: Option<u16>,
    armed: Option<crate::app_settings::RuntimeHotkey>,
}

#[cfg(target_os = "macos")]
impl CaptureShortcut {
    pub fn input(
        &mut self,
        event: crate::suppression::InputEvent,
        hotkeys: crate::app_settings::RuntimeHotkeys,
        activity: ShortcutActivity,
    ) -> ShortcutAction {
        use crate::suppression::InputEvent;
        // Escape before anything records takes the press back; once a
        // capture records (started some other way), it only disarms: Escape
        // never ends a recording.
        if self.armed.is_some() && event.is_escape_down() {
            self.armed = None;
            return match activity {
                ShortcutActivity::Recording { .. } => ShortcutAction::None,
                _ => ShortcutAction::Cancel,
            };
        }
        match event {
            InputEvent::Key {
                code,
                down: true,
                flags,
            } => {
                if self.held == Some(code) {
                    return ShortcutAction::None;
                }
                let Some(binding) = hotkeys
                    .capture
                    .filter(|binding| binding.matches_key_press(code, flags))
                else {
                    return ShortcutAction::None;
                };
                self.held = Some(code);
                if self.armed.is_some() {
                    return ShortcutAction::None;
                }
                match activity {
                    ShortcutActivity::Recording { elapsed } if elapsed < SHORTCUT_DEBOUNCE => {
                        ShortcutAction::None
                    }
                    ShortcutActivity::Recording { .. } => ShortcutAction::Finish,
                    ShortcutActivity::Picking => ShortcutAction::Pick,
                    ShortcutActivity::Free => {
                        self.armed = Some(binding);
                        ShortcutAction::None
                    }
                }
            }
            InputEvent::Key {
                code,
                down: false,
                flags,
            } => {
                let key_up = self.held == Some(code);
                if key_up {
                    self.held = None;
                }
                match self.armed {
                    // A shortcut without modifiers records on its key's release.
                    Some(binding) if binding.modifiers.is_empty() && key_up => self.record(),
                    Some(binding)
                        if !binding.modifiers.is_empty()
                            && !binding.required_modifiers_down(flags) =>
                    {
                        self.record()
                    }
                    _ => ShortcutAction::None,
                }
            }
            InputEvent::Flags(flags) => match self.armed {
                Some(binding)
                    if !binding.modifiers.is_empty() && !binding.required_modifiers_down(flags) =>
                {
                    self.record()
                }
                _ => ShortcutAction::None,
            },
            InputEvent::TapDisabled => {
                self.reset();
                ShortcutAction::None
            }
            _ => ShortcutAction::None,
        }
    }

    fn record(&mut self) -> ShortcutAction {
        self.armed = None;
        ShortcutAction::Record
    }

    /// Pressed, waiting for its keys to come up: Escape is the shortcut's.
    pub fn armed(&self) -> bool {
        self.armed.is_some()
    }

    /// Whether Hex's tap keeps Escape from the app for captures: only for
    /// this press before it records, and for the capsule's picker. (An open
    /// list takes Escape at the session tap.) A recording capture never
    /// takes it, and a send's Undo is a click.
    pub fn takes_escape(&self) -> bool {
        self.armed() || wants_escape()
    }

    /// Forgets a held key whose release the listener will not see, as while
    /// shortcuts are suspended, so the next press counts; an armed press
    /// records nothing.
    pub fn reset(&mut self) {
        self.held = None;
        self.armed = None;
    }
}

/// The keys the capsule takes from Hex's tap while it is on screen: it never
/// takes focus, so these never reach the app in front.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapsuleKeys {
    /// The picker: the capture shortcut ([`CapsuleKey::Shortcut`]) and Escape.
    Picker,
    /// A routed capsule, or a hidden one whose capture still waits for a
    /// pick: the send-to list shortcut (`capture.list_hotkey`) opens its
    /// list ([`CapsuleKey::OpenList`]). Never Escape: it goes to the app in
    /// front, and a send is taken back only by clicking Undo.
    Route,
    /// The open list: every key press without ⌘ or ⌃ that isn't one of
    /// Hex's shortcuts.
    List,
    /// A capture's suggestions: the send-to list shortcut (search every
    /// session), 1 to `rows` with no modifier ([`CapsuleKey::Digit`]), and
    /// a mouse press ([`CapsuleKey::Press`]) that may fold them. `cycle`:
    /// Return and Tab too, for the toast that offers one guess at a time.
    Suggest { rows: u8, cycle: bool },
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// A key the capsule gets from Hex's tap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapsuleKey {
    /// The capture shortcut, while picking: record what the picker is on.
    Shortcut,
    Escape,
    /// The send-to list shortcut, while routed or waiting for a pick.
    OpenList,
    /// What a key types in the list, in the active layout.
    Text(String),
    Backspace,
    Up,
    Down,
    /// Return or Enter, ⇧ for always.
    Return {
        shift: bool,
    },
    /// Tab, ⇧ for back: the toast's next guess.
    Tab {
        shift: bool,
    },
    /// 1 to 9 typed in the list (otherwise text), with Shift too: a
    /// Shift pick also saves a rule.
    Digit(u8),
    /// Shift went down (true) or up while the list or suggestions show.
    Shift(bool),
    /// A mouse button went down anywhere, while the list is open. The click
    /// still reaches the app under it.
    Press,
}

type OnCapsuleKey = Arc<dyn Fn(CapsuleKey) + Send + Sync>;

struct KeyGrab {
    id: u64,
    keys: CapsuleKeys,
    on_key: OnCapsuleKey,
}

static GRAB: Mutex<Option<KeyGrab>> = Mutex::new(None);
static GRABS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// [`GRAB`]'s keys for the tap, without its lock: 0 none, then
/// [`keys_code`].
static GRAB_KEYS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn keys_code(keys: Option<CapsuleKeys>) -> u8 {
    match keys {
        None => 0,
        Some(CapsuleKeys::Picker) => 1,
        Some(CapsuleKeys::Route) => 2,
        Some(CapsuleKeys::List) => 4,
        Some(CapsuleKeys::Suggest { .. }) => 6,
    }
}

fn grab() -> std::sync::MutexGuard<'static, Option<KeyGrab>> {
    GRAB.lock().unwrap_or_else(|error| error.into_inner())
}

/// The capsule takes `keys` until the guard drops; [`CapsuleKeyGrab::set`]
/// changes them. Each key goes to `on_key` on the listener's thread or, for
/// the open list, the tap's: it must not block. A newer grab replaces an
/// older one.
pub fn take_keys(
    keys: CapsuleKeys,
    on_key: impl Fn(CapsuleKey) + Send + Sync + 'static,
) -> CapsuleKeyGrab {
    let id = GRABS.fetch_add(1, Ordering::Relaxed) + 1;
    *grab() = Some(KeyGrab {
        id,
        keys,
        on_key: Arc::new(on_key),
    });
    GRAB_KEYS.store(keys_code(Some(keys)), Ordering::Release);
    CapsuleKeyGrab(id)
}

/// The capsule's keys; see [`take_keys`].
#[must_use = "the capsule lets the keys go when this drops"]
pub struct CapsuleKeyGrab(u64);

impl CapsuleKeyGrab {
    pub fn set(&self, keys: CapsuleKeys) {
        let mut grab = grab();
        if let Some(current) = grab.as_mut().filter(|grab| grab.id == self.0) {
            current.keys = keys;
            GRAB_KEYS.store(keys_code(Some(keys)), Ordering::Release);
        }
    }
}

impl Drop for CapsuleKeyGrab {
    fn drop(&mut self) {
        let mut grab = grab();
        if grab.as_ref().is_some_and(|grab| grab.id == self.0) {
            *grab = None;
            GRAB_KEYS.store(0, Ordering::Release);
        }
    }
}

/// Whether the capsule's picker is open.
pub fn picking() -> bool {
    GRAB_KEYS.load(Ordering::Acquire) == keys_code(Some(CapsuleKeys::Picker))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Whether the capsule takes Escape from the listener: only the picker
/// (the open list takes it at the tap).
pub fn wants_escape() -> bool {
    GRAB_KEYS.load(Ordering::Acquire) == keys_code(Some(CapsuleKeys::Picker))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Whether the open list takes every key press at the tap.
pub fn list_open() -> bool {
    GRAB_KEYS.load(Ordering::Acquire) == keys_code(Some(CapsuleKeys::List))
}

/// Whether the capsule takes the send-to list shortcut: only then does Hex
/// match it (`app_settings::runtime_hotkeys`), so it reaches the app in
/// front the rest of the time.
pub fn list_key_wanted() -> bool {
    matches!(GRAB_KEYS.load(Ordering::Acquire), 2 | 4 | 6)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// Whether the capsule's suggestions take their digit keys at the tap.
pub fn suggesting() -> bool {
    GRAB_KEYS.load(Ordering::Acquire)
        == keys_code(Some(CapsuleKeys::Suggest {
            rows: 0,
            cycle: false,
        }))
}

/// Hands `key` to the capsule if its keys include it; false otherwise.
pub fn capsule_key(key: CapsuleKey) -> bool {
    let on_key = grab()
        .as_ref()
        .filter(|grab| takes(grab.keys, &key))
        .map(|grab| grab.on_key.clone());
    on_key.map(|on_key| on_key(key)).is_some()
}

fn takes(keys: CapsuleKeys, key: &CapsuleKey) -> bool {
    match (keys, key) {
        (CapsuleKeys::Picker, CapsuleKey::Shortcut | CapsuleKey::Escape) => true,
        (
            CapsuleKeys::Route | CapsuleKeys::List | CapsuleKeys::Suggest { .. },
            CapsuleKey::OpenList,
        ) => true,
        (CapsuleKeys::Suggest { rows, .. }, CapsuleKey::Digit(digit)) => (1..=rows).contains(digit),
        (CapsuleKeys::Suggest { .. }, CapsuleKey::Press | CapsuleKey::Shift(_)) => true,
        (
            CapsuleKeys::Suggest { cycle: true, .. },
            CapsuleKey::Return { shift: false } | CapsuleKey::Tab { .. },
        ) => true,
        (CapsuleKeys::List, key) => !matches!(key, CapsuleKey::Shortcut),
        _ => false,
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// What the tap does with a key press while the list is open.
#[derive(Debug, Eq, PartialEq)]
pub enum ListTap {
    /// Not the list's: ⌘ or ⌃ held, or one of Hex's shortcuts.
    Pass,
    /// Kept from the app; the list does nothing with it.
    Swallow,
    Key(CapsuleKey),
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The list's reading of a key press, `typed` being what it types in the
/// active layout.
pub fn list_key(code: u16, flags: u64, typed: &str) -> ListTap {
    const COMMAND: u64 = 1 << 20;
    const CONTROL: u64 = 1 << 18;
    const SHIFT: u64 = 1 << 17;
    const OPTION: u64 = 1 << 19;
    if flags & (COMMAND | CONTROL) != 0 {
        return ListTap::Pass;
    }
    let key = match code {
        53 => CapsuleKey::Escape,
        36 | 76 => CapsuleKey::Return {
            shift: flags & SHIFT != 0,
        },
        48 => CapsuleKey::Tab {
            shift: flags & SHIFT != 0,
        },
        126 => CapsuleKey::Up,
        125 => CapsuleKey::Down,
        51 => CapsuleKey::Backspace,
        // Shift's digits type symbols, so the digit row reads by position.
        _ if flags & (SHIFT | OPTION) == SHIFT && shifted_digit(code).is_some() => {
            CapsuleKey::Digit(shifted_digit(code).unwrap_or(1))
        }
        _ => match typed.chars().next() {
            Some(digit @ '1'..='9') if typed.len() == 1 && flags & OPTION == 0 => {
                CapsuleKey::Digit(digit as u8 - b'0')
            }
            Some(_) if !typed.chars().any(char::is_control) => CapsuleKey::Text(typed.into()),
            _ => return ListTap::Swallow,
        },
    };
    ListTap::Key(key)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// 1 to 9 for the ANSI digit row's key codes.
fn shifted_digit(code: u16) -> Option<u8> {
    [18, 19, 20, 21, 23, 22, 26, 28, 25]
        .iter()
        .position(|row| *row == code)
        .map(|i| i as u8 + 1)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The ANSI Z key, when the layout has none.
const KEY_Z: u16 = 6;

/// Waits for `thread` until `deadline`, then leaves it running.
fn join_by(thread: JoinHandle<()>, deadline: Instant, name: &str) {
    while !thread.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    if thread.is_finished() {
        let _ = thread.join();
    } else {
        tracing::warn!(
            thread = name,
            "capture thread did not stop in time; leaving it"
        );
    }
}

/// The `capture-events` thread: logs the engine's progress and hands it to
/// the host's outcomes, and so to the snapshot.
fn log_events(events: Receiver<Event>) {
    for event in events {
        if let Event::PhoneEnded = event {
            tracing::info!("the iPhone mirror ended on its own");
            let live = LIVE_OUTCOMES
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .upgrade();
            if let Some(outcomes) = live {
                lock(&outcomes).phone_ended = true;
            }
        }
        if let Event::Progress { bundle, stage } = event {
            tracing::info!(bundle = %bundle.display(), stage = stage.label(), "capture progress");
            let live = LIVE_OUTCOMES
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .upgrade();
            if let Some(outcomes) = live {
                lock(&outcomes).progress(&bundle, stage);
            }
        }
    }
}

/// The `capture-finish` thread: writes stopped captures in order, one at a
/// time, copies each to the clipboard, settles each outcome, and hands
/// written ones to the `capture-route` thread.
fn finish_captures(
    jobs: Receiver<FinishJob>,
    outcomes: &Mutex<Outcomes>,
    routes: &SyncSender<routing::Job>,
) {
    for FinishJob {
        bundle,
        capture,
        audio,
        look,
    } in jobs
    {
        let finished = capture.finish(audio);
        let look = look.recv().ok();
        restrict_bundle(&bundle);
        match finished {
            Ok(written) => {
                tracing::info!(
                    bundle = %bundle.display(),
                    words = written.words,
                    gaps = written.gaps,
                    missing_transcript = written.missing_transcript.as_deref(),
                    "capture finished"
                );
                copy_to_clipboard(&bundle);
                let route = look.as_ref().map(|_| CaptureRoute::looking());
                lock(outcomes).settle_with(bundle.clone(), Stage::Done, Some(written), None, route);
                if let Some(look) = look {
                    let job = routing::Job::Written {
                        bundle: bundle.clone(),
                        look,
                    };
                    if !routing::send(routes, job) {
                        lock(outcomes).routed(&bundle, routing::unrouted());
                    }
                }
            }
            Err(error) => {
                tracing::error!(error = format!("{error:#}"), "capture failed");
                lock(outcomes).settle(bundle, Stage::Failed, None);
            }
        }
    }
}

/// What the capsule's Paste and Inspect do with the newest written capture.
#[derive(Clone, Copy, Debug)]
enum HandOff {
    Paste,
    Inspect,
}

/// Pastes or opens a written capture: its contact sheet and summary.
#[cfg(target_os = "macos")]
fn hand_off(how: HandOff, bundle: &Path) -> anyhow::Result<()> {
    objc2::rc::autoreleasepool(|_| match how {
        // ⌘V from Hex's keyboard layout snapshot, marked as Hex's: the
        // crate's own reads TIS off the main thread.
        HandOff::Paste => scrawl_capture::handoff::paste_bundle_with(bundle, &|| {
            crate::keyboard::post_command('v').map_err(|error| anyhow::anyhow!("{error:#}"))
        }),
        HandOff::Inspect => inspect(bundle),
    })
}

/// Linux pastes the summary through Hex's own paste path (Wayland or X11).
#[cfg(target_os = "linux")]
fn hand_off(how: HandOff, bundle: &Path) -> anyhow::Result<()> {
    match how {
        HandOff::Paste => {
            let summary = summary(bundle)?;
            with_linux_paster(|paster| paster.paste(&summary))
        }
        HandOff::Inspect => inspect(bundle),
    }
}

fn inspect(bundle: &Path) -> anyhow::Result<()> {
    scrawl_capture::handoff::load_bundle(bundle).and_then(|(dir, loaded)| {
        scrawl_capture::handoff::open_path(&dir.join(loaded.contact_sheet))
    })
}

/// A written capture's summary, as the clipboard carries it.
#[cfg(target_os = "linux")]
fn summary(bundle: &Path) -> anyhow::Result<String> {
    let (dir, loaded) = scrawl_capture::handoff::load_bundle(bundle)?;
    Ok(scrawl_capture::sketch::summary_markdown(&loaded, &dir))
}

/// The process's one paster for captures. On X11 it serves the clipboard
/// for as long as it lives, so a copied capture stays pasteable.
#[cfg(target_os = "linux")]
fn with_linux_paster(
    use_it: impl FnOnce(&mut crate::linux_paste::LinuxPaster) -> Result<()>,
) -> anyhow::Result<()> {
    static PASTER: Mutex<Option<crate::linux_paste::LinuxPaster>> = Mutex::new(None);
    let mut paster = PASTER.lock().unwrap_or_else(|error| error.into_inner());
    if paster.is_none() {
        let shift = crate::app_settings::AppSettings::load()
            .map(|settings| settings.paste_with_shift)
            .unwrap_or(false);
        *paster = Some(
            crate::linux_paste::LinuxPaster::new(
                Arc::default(),
                Arc::new(AtomicBool::new(shift)),
                Default::default(),
            )
            .map_err(|error| anyhow::anyhow!("{error:#}"))?,
        );
    }
    use_it(paster.as_mut().expect("made above")).map_err(|error| anyhow::anyhow!("{error:#}"))
}

/// Copies a written capture to the clipboard, as Scrawl does: its summary,
/// ready to paste into an agent.
fn copy_to_clipboard(bundle: &Path) {
    if cfg!(test) {
        return;
    }
    #[cfg(target_os = "macos")]
    let copied =
        objc2::rc::autoreleasepool(|_| scrawl_capture::handoff::copy_bundle_to_clipboard(bundle));
    #[cfg(target_os = "linux")]
    let copied =
        summary(bundle).and_then(|summary| with_linux_paster(|paster| paster.copy(&summary)));
    if let Err(error) = copied {
        tracing::warn!(error = format!("{error:#}"), "could not copy the capture");
    }
}

/// One capture's audio thread: starts the engine, collects until the sink
/// closes, then, as sealed, stops the capture at once and queues it for the
/// finisher, or cancels it. If the engine cannot start, it removes the
/// bundle and settles the failure; dropping `chunks` closes the sink.
#[expect(clippy::too_many_arguments, reason = "one thread's whole context")]
fn collect(
    engine: &dyn CaptureEngine,
    start: anyhow::Result<Start>,
    bundle: &Path,
    chunks: Receiver<CaptureAudio>,
    sealed: Receiver<Seal>,
    jobs: &SyncSender<FinishJob>,
    outcomes: &SharedOutcomes,
    input: &LiveInput,
) {
    let bundle = bundle.to_path_buf();
    let capture = start.and_then(|start| {
        let t0 = CaptureInstant::from_nanos(start.t0_boot_ns);
        let capture = engine.start(start)?;
        if let Some(sink) = capture.input() {
            let shift = modifiers(scrawl_capture::capture::modifier_flags()).shift;
            input.attach(&bundle, sink, t0, shift, outcomes);
        }
        Ok((capture, t0))
    });
    let (capture, t0) = match capture {
        Ok(started) => started,
        Err(error) => {
            let error = format!("{error:#}");
            tracing::error!(error, "could not start the capture");
            drop(chunks);
            remove_bundle(&bundle);
            lock(outcomes).fail(bundle, error);
            return;
        }
    };
    let mut timeline = Timeline::new(t0);
    for chunk in chunks {
        match chunk {
            CaptureAudio::Samples {
                samples,
                sample_rate,
                captured_through,
            } => timeline.push(&samples, sample_rate, captured_through),
            CaptureAudio::Gap { from, through } => timeline.gap(from, through),
        }
    }
    input.detach(&bundle);
    match sealed.recv() {
        Ok(Seal::Finish { through, closed }) => {
            // The screen stops now, not when the finisher gets to it.
            let capture = capture.stop();
            // The app in front now; its tab and terminal are read while the
            // capture is written.
            let context = capture.route_context();
            let audio = timeline.finish(through, closed);
            let (give_look, look) = mpsc::sync_channel(1);
            // Starts are refused while the queue is full, so it has room
            // unless Hex is stopping.
            let job = FinishJob {
                bundle: bundle.clone(),
                capture,
                audio,
                look,
            };
            match jobs.try_send(job) {
                Ok(()) => {
                    // Only a queued capture is looked at, so a refused one
                    // leaves no look running.
                    if let Some(look) = context.and_then(routing::look) {
                        let _ = give_look.send(look);
                    }
                    return;
                }
                Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                    tracing::warn!("no room to write the capture; discarding it");
                    job.capture.cancel();
                }
            }
        }
        Ok(Seal::Cancel) | Err(_) => capture.cancel(),
    }
    lock(outcomes).settle(bundle, Stage::Cancelled, None);
}

/// A capture's audio at 16 kHz on its own clock: resampled as it arrives,
/// with missing spans padded with silence so positions stay on the clock.
struct Timeline {
    t0: CaptureInstant,
    /// Where `samples[0]` sits on the clock, once audio arrived.
    first: Option<CaptureInstant>,
    samples: Vec<f32>,
    resampler: Option<(u32, StreamingResampler)>,
    gaps: Vec<GapSpan>,
    /// The most samples kept (`MAX_CAPTURE`).
    limit: usize,
    /// Where audio past the limit started being dropped.
    truncated_at: Option<CaptureInstant>,
}

impl Timeline {
    fn new(t0: CaptureInstant) -> Self {
        Self::with_limit(t0, MAX_CAPTURE)
    }

    fn with_limit(t0: CaptureInstant, limit: Duration) -> Self {
        Self {
            t0,
            first: None,
            samples: Vec::new(),
            resampler: None,
            gaps: Vec::new(),
            limit: (limit.as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize,
            truncated_at: None,
        }
    }

    fn push(&mut self, samples: &[f32], sample_rate: u32, through: CaptureInstant) {
        if samples.is_empty() || sample_rate == 0 {
            return;
        }
        let from = through.sub_frames(samples.len(), sample_rate);
        self.first.get_or_insert(from.max(self.t0));
        if self
            .resampler
            .as_ref()
            .is_none_or(|(rate, _)| *rate != sample_rate)
        {
            self.flush();
            match StreamingResampler::new(sample_rate) {
                Ok(resampler) => self.resampler = Some((sample_rate, resampler)),
                Err(error) => {
                    tracing::warn!(%error, sample_rate, "capture audio cannot be resampled");
                    self.gap(from, through);
                    return;
                }
            }
        }
        if let Some((_, resampler)) = &mut self.resampler {
            let resampled = resampler.push(samples);
            self.append(&resampled, through);
        }
    }

    fn gap(&mut self, from: CaptureInstant, through: CaptureInstant) {
        if through.duration_since(from) <= GAP_TOLERANCE {
            return;
        }
        let Some(first) = self.first else {
            // Before any audio: the recording simply starts later.
            return;
        };
        if self.truncated_at.is_some() {
            // Past the limit: `finish` records everything after it as one gap.
            return;
        }
        self.flush();
        let limit_at = first + duration_for(self.limit);
        self.gaps.push(GapSpan {
            start_ms: self.clock_ms(from.max(first).min(limit_at)),
            end_ms: self.clock_ms(through.min(limit_at)),
        });
        // Padding stops one sample past the limit, which marks the truncation.
        let end = position(first, through).min(self.limit + 1);
        if end > self.samples.len() {
            let padding = vec![0.0; end - self.samples.len()];
            self.append(&padding, through);
        }
    }

    fn finish(
        mut self,
        through: CaptureInstant,
        closed: Option<CaptureSinkClosed>,
    ) -> RecordedAudio {
        if let Some(closed) = closed {
            if let Some((from, until)) = closed.pending_gap {
                self.gap(from, until);
            }
            if closed.accounted_through < through {
                self.gap(closed.accounted_through.max(self.t0), through);
            }
        }
        self.flush();
        if let Some(truncated_at) = self.truncated_at {
            self.gaps.push(GapSpan {
                start_ms: self.clock_ms(truncated_at),
                end_ms: self.clock_ms(through),
            });
        }
        let Some(first) = self.first else {
            return RecordedAudio::new(Vec::new(), SAMPLE_RATE, self.clock_ms(through), Vec::new());
        };
        // Audio delivered after the stop belongs to no capture.
        self.samples.truncate(position(first, through));
        let start_offset_ms = self.clock_ms(first);
        RecordedAudio::new(self.samples, SAMPLE_RATE, start_offset_ms, self.gaps)
    }

    fn flush(&mut self) {
        if let Some((_, resampler)) = self.resampler.take() {
            let rest = resampler.finish();
            let through = self
                .first
                .map(|first| first + duration_for(self.samples.len() + rest.len()))
                .unwrap_or(self.t0);
            self.append(&rest, through);
        }
    }

    fn append(&mut self, samples: &[f32], through: CaptureInstant) {
        let limit = self.limit;
        let room = limit.saturating_sub(self.samples.len());
        if samples.len() > room && self.truncated_at.is_none() {
            tracing::warn!("capture reached its length limit; dropping the rest of its audio");
            self.truncated_at = Some(
                self.first
                    .map_or(through, |first| first + duration_for(limit)),
            );
        }
        self.samples
            .extend_from_slice(&samples[..samples.len().min(room)]);
    }

    fn clock_ms(&self, at: CaptureInstant) -> u64 {
        at.duration_since(self.t0).as_millis() as u64
    }
}

fn position(first: CaptureInstant, at: CaptureInstant) -> usize {
    (at.duration_since(first).as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000) as usize
}

fn duration_for(samples: usize) -> Duration {
    crate::audio::duration_for_frames(samples, SAMPLE_RATE)
}

/// `hex dev capture-replay`: runs a capture through [`CaptureHost`] on a
/// fresh transcription engine with the selected model, feeding `wavs` in
/// place of the dictation timeline, and returns the finished bundle. Scrawl's
/// engine records the screen meanwhile, which needs Screen Recording.
#[cfg(all(debug_assertions, target_os = "macos"))]
pub fn replay(wavs: &[PathBuf]) -> Result<PathBuf> {
    let mut samples = Vec::new();
    let mut sample_rate = None;
    for wav in wavs {
        let (audio, rate) = crate::voice_filter::read_wav_mono_f32(wav)
            .map_err(|error| eyre!("{}: {error}", wav.display()))?;
        if *sample_rate.get_or_insert(rate) != rate {
            return Err(eyre!(
                "{} is not {} Hz",
                wav.display(),
                sample_rate.unwrap_or(rate)
            ));
        }
        if !samples.is_empty() {
            samples.extend(std::iter::repeat_n(0.0, rate as usize * 3 / 10));
        }
        samples.extend(audio);
    }
    let sample_rate = sample_rate.ok_or_else(|| eyre!("no WAV files"))?;
    let engine = Arc::new(TranscriptionEngine::start(Default::default())?);
    if !crate::app_settings::capture_available() {
        return Err(eyre!(
            "the `capture` experiment is off; run with HEX_EXPERIMENTS=capture"
        ));
    }
    let mut host = CaptureHost::with(shared_engine(Some(&engine))?, None)?;
    let timeline = WavTimeline {
        samples: Arc::new(samples),
        sample_rate,
        feeder: Mutex::new(None),
    };
    let ControlReply::ScrawlCapture(started) = host.control(ScrawlCaptureAction::Start, &timeline)
    else {
        return Err(eyre!("the capture did not start"));
    };
    let bundle = started
        .bundle
        .ok_or_else(|| eyre!("the capture has no bundle"))?;
    let ends = CaptureInstant::from_nanos(started_at(&host)?)
        + crate::audio::duration_for_frames(timeline.samples.len(), sample_rate);
    while CaptureInstant::now() < ends + Duration::from_millis(20) {
        thread::sleep(Duration::from_millis(20));
    }
    host.control(ScrawlCaptureAction::Stop, &timeline);
    for _ in 0..60_000 {
        let ControlReply::ScrawlCapture(status) =
            host.control(ScrawlCaptureAction::Status, &timeline)
        else {
            break;
        };
        if let Some(last) = status.last.filter(|last| last.bundle == bundle) {
            if last.stage != "done" {
                return Err(eyre!("the capture {}", last.stage));
            }
            return Ok(bundle);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(eyre!("the capture never finished"))
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn started_at(host: &CaptureHost) -> Result<u64> {
    host.active
        .as_ref()
        .map(|active| active.t0.as_nanos())
        .ok_or_else(|| eyre!("no capture is recording"))
}

/// Recorded audio standing in for the dictation timeline: the whole file is
/// delivered at once, stamped as if captured from the capture's start.
#[cfg(all(debug_assertions, target_os = "macos"))]
struct WavTimeline {
    samples: Arc<Vec<f32>>,
    sample_rate: u32,
    feeder: Mutex<Option<(thread::JoinHandle<()>, CaptureInstant)>>,
}

#[cfg(all(debug_assertions, target_os = "macos"))]
impl AudioTimeline for WavTimeline {
    fn open_sink(&self, sink: SyncSender<CaptureAudio>, since: CaptureInstant) -> Result<()> {
        let samples = self.samples.clone();
        let sample_rate = self.sample_rate;
        let chunk = (sample_rate as usize / 100).max(1);
        let feeder = thread::spawn(move || {
            for (index, part) in samples.chunks(chunk).enumerate() {
                let through = since
                    + crate::audio::duration_for_frames(index * chunk + part.len(), sample_rate);
                let audio = CaptureAudio::Samples {
                    samples: part.to_vec(),
                    sample_rate,
                    captured_through: through,
                };
                if sink.send(audio).is_err() {
                    return;
                }
            }
        });
        let ends = since + crate::audio::duration_for_frames(self.samples.len(), sample_rate);
        *self
            .feeder
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((feeder, ends));
        Ok(())
    }

    fn close_sink(&self, _through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
        let feeder = self
            .feeder
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        Ok(feeder.map(|(feeder, ends)| {
            let _ = feeder.join();
            CaptureSinkClosed {
                pending_gap: None,
                accounted_through: ends,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capsule is one 640×424 panel above everything, but only its pill
    /// takes clicks: a click elsewhere on the panel, or under the
    /// full-screen ink overlay, is a click on the app beneath, while one on
    /// the pill is Hex's and never reaches the engine.
    #[test]
    fn only_the_capsules_pill_and_not_its_panel_takes_clicks() {
        use scrawl_capture::windows::RawWindow;
        let own = std::process::id() as i32;
        let window = |id, pid, bounds, layer| RawWindow {
            id,
            pid,
            owner: String::new(),
            title: String::new(),
            bounds,
            layer,
            alpha: 1.0,
        };
        let (ink, capsule, app) = (11, 12, 13);
        let windows = [
            window(ink, own, [0.0, 0.0, 1728.0, 1117.0], 26),
            window(capsule, own, [544.0, 0.0, 640.0, 424.0], 27),
            window(app, own + 1, [0.0, 0.0, 1728.0, 1117.0], 0),
        ];
        let roles = [Some(capsule), Some(ink), None];
        let OwnWindows {
            excluded,
            passthrough,
            ..
        } = own_windows_of(&roles);
        assert_eq!(excluded, [capsule, ink]);
        let engine_says_hex = |x, y| {
            scrawl_capture::capture::on_own_window(
                &windows,
                (x, y),
                &passthrough,
                &excluded,
                0,
                |_| false,
            )
        };
        let pill = [160.0, 14.0, 320.0, 44.0];
        let origin = panel_origin(&windows, capsule).unwrap();
        assert_eq!(origin, [544.0, 0.0]);
        let hex_takes = |x, y| on_pill(origin, pill, x, y) || engine_says_hex(x, y);

        assert!(hex_takes(864.0, 36.0), "the pill");
        assert!(!hex_takes(600.0, 300.0), "the panel's empty part");
        assert!(!hex_takes(864.0, 100.0), "just under the pill");
        assert!(!hex_takes(1500.0, 900.0), "under the ink overlay only");
        assert_eq!(panel_origin(&windows, 99), None, "a panel not on screen");
    }

    fn t(ms: u64) -> CaptureInstant {
        CaptureInstant::from_nanos(90_000_000_000 + ms * 1_000_000)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("hex-capture-{name}-{unique}"))
    }

    #[test]
    fn the_timeline_resamples_pads_gaps_and_ends_at_the_stop() {
        let mut timeline = Timeline::new(t(0));
        // Audio starts 100 ms in (a cold microphone).
        let mut through = t(100);
        for _ in 0..50 {
            through = through + Duration::from_millis(10);
            timeline.push(&[0.5; 480], 48_000, through);
        }
        // 300 ms dropped, then audio again.
        timeline.gap(through, through + Duration::from_millis(300));
        through = through + Duration::from_millis(300);
        for _ in 0..50 {
            through = through + Duration::from_millis(10);
            timeline.push(&[0.5; 160], 16_000, through);
        }
        let audio = timeline.finish(
            through.checked_sub(Duration::from_millis(100)).unwrap(),
            None,
        );
        assert_eq!(audio.sample_rate, 16_000);
        assert_eq!(audio.start_offset_ms, 100);
        assert_eq!(
            audio.gaps,
            [GapSpan {
                start_ms: 600,
                end_ms: 900
            }]
        );
        // 500 + 300 + 500 ms of audio, cut 100 ms before the last chunk.
        assert_eq!(audio.duration_ms(), 100 + 1_200);
        let resampled_silence = &audio.samples[8_000 + 100..8_000 + 4_700];
        assert!(resampled_silence.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_pending_gap_and_a_stalled_tail_come_back_from_the_closed_sink() {
        let mut timeline = Timeline::new(t(0));
        timeline.push(&[0.5; 1_600], 16_000, t(100));
        let audio = timeline.finish(
            t(600),
            Some(CaptureSinkClosed {
                pending_gap: Some((t(100), t(300))),
                accounted_through: t(300),
            }),
        );
        assert_eq!(
            audio.gaps,
            [
                GapSpan {
                    start_ms: 100,
                    end_ms: 300
                },
                GapSpan {
                    start_ms: 300,
                    end_ms: 600
                }
            ]
        );
        assert_eq!(audio.samples.len(), 9_600);
    }

    #[derive(Default)]
    struct FakeTimeline {
        open: Mutex<Option<SyncSender<CaptureAudio>>>,
        /// The end of the audio fed so far.
        fed: Mutex<Option<CaptureInstant>>,
        /// Runs at the start of the next `close_sink`.
        on_close: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl AudioTimeline for FakeTimeline {
        fn open_sink(&self, sink: SyncSender<CaptureAudio>, _since: CaptureInstant) -> Result<()> {
            let mut open = self.open.lock().unwrap();
            assert!(open.is_none(), "one sink at a time");
            *open = Some(sink);
            Ok(())
        }

        fn close_sink(&self, _through: CaptureInstant) -> Result<Option<CaptureSinkClosed>> {
            let on_close = self.on_close.lock().unwrap().take();
            if let Some(on_close) = on_close {
                on_close();
            }
            let fed = self.fed.lock().unwrap().take();
            Ok(self.open.lock().unwrap().take().map(|_| CaptureSinkClosed {
                pending_gap: None,
                accounted_through: fed.unwrap_or(CaptureInstant::ZERO),
            }))
        }
    }

    impl FakeTimeline {
        fn feed(&self, samples: usize, through: CaptureInstant) {
            *self.fed.lock().unwrap() = Some(through);
            let open = self.open.lock().unwrap();
            open.as_ref()
                .expect("a sink is open")
                .send(CaptureAudio::Samples {
                    samples: vec![0.25; samples],
                    sample_rate: 16_000,
                    captured_through: through,
                })
                .unwrap();
        }
    }

    struct FakeTranscriber;

    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
            assert_eq!(audio.sample_rate, 16_000);
            Ok(Some(TranscriptData {
                source: "fake".into(),
                text: "hello there".into(),
                words: vec![
                    WordTiming {
                        t0: 0,
                        t1: 200,
                        text: "hello".into(),
                    },
                    WordTiming {
                        t0: 250,
                        t1: 500,
                        text: "there".into(),
                    },
                ],
                ..TranscriptData::default()
            }))
        }
    }

    /// Scrawl's engine minus the screen: it writes the bundle with Scrawl's
    /// own writer from the audio Hex hands it and no frames.
    struct FakeEngine {
        transcriber: Arc<dyn Transcriber>,
        starts: Mutex<Vec<Start>>,
        steps: Steps,
        refuse: bool,
        phones: Vec<String>,
    }

    /// What each capture did, in order: `stop`, then `finish` once its
    /// write begins.
    type Steps = Arc<Mutex<Vec<(&'static str, PathBuf)>>>;

    impl FakeEngine {
        fn new(transcriber: Arc<dyn Transcriber>) -> Arc<Self> {
            Arc::new(Self {
                transcriber,
                starts: Mutex::default(),
                steps: Steps::default(),
                refuse: false,
                phones: Vec::new(),
            })
        }
    }

    impl CaptureEngine for FakeEngine {
        fn start(&self, start: Start) -> anyhow::Result<Box<dyn Recording>> {
            self.starts.lock().unwrap().push(start.clone());
            if self.refuse {
                anyhow::bail!("no screen to record");
            }
            Ok(Box::new(FakeRecording {
                dir: start.bundle_dir.unwrap(),
                transcriber: self.transcriber.clone(),
                steps: self.steps.clone(),
            }))
        }

        /// No ScreenCaptureKit: window 0 has closed, and the rest record the
        /// display.
        fn target(&self, target: CaptureTarget) -> anyhow::Result<Target> {
            match target {
                CaptureTarget::Window { id: 0 } => anyhow::bail!("the window closed"),
                CaptureTarget::Phone => Ok(Target::Phone {
                    device: None,
                    name: None,
                }),
                _ => Ok(Target::ActiveDisplay),
            }
        }

        /// No `scrawl-helper`.
        fn phones(&self) -> anyhow::Result<Vec<String>> {
            Ok(self.phones.clone())
        }
    }

    struct FakeRecording {
        dir: PathBuf,
        transcriber: Arc<dyn Transcriber>,
        steps: Steps,
    }

    impl Recording for FakeRecording {
        fn cancel(self: Box<Self>) {
            fs::remove_dir_all(&self.dir).unwrap();
        }

        fn stop(self: Box<Self>) -> Box<dyn Sealed> {
            self.steps.lock().unwrap().push(("stop", self.dir.clone()));
            self
        }
    }

    impl Sealed for FakeRecording {
        fn cancel(self: Box<Self>) {
            fs::remove_dir_all(&self.dir).unwrap();
        }

        fn finish(self: Box<Self>, audio: RecordedAudio) -> anyhow::Result<Written> {
            use scrawl_capture::model::{CaptureMode, TargetFrame, TargetInfo};
            self.steps
                .lock()
                .unwrap()
                .push(("finish", self.dir.clone()));
            let duration_ms = audio.duration_ms();
            let gaps = audio.gaps.len();
            let target = TargetInfo {
                mode: CaptureMode::Window,
                app: None,
                bundle_id: None,
                window_title: None,
                window_id: None,
                display_id: 0,
                frame: TargetFrame {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                    scale: 2.0,
                },
            };
            let recording = scrawl_capture::capture::ScreenRecording {
                frames: Vec::new(),
                video: None,
                duration_ms,
            };
            let (_, bundle, _) = scrawl_capture::pipeline::finalize_capture_bundle_with_flags(
                &self.dir,
                target,
                recording,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(audio),
                None,
                Default::default(),
                Some(&*self.transcriber),
            )?;
            Ok(Written {
                words: bundle.transcript.words.len(),
                gaps,
                missing_transcript: bundle.transcript.missing.map(|missing| missing.reason),
            })
        }
    }

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn report(reply: ControlReply) -> ScrawlCaptureReport {
        match reply {
            ControlReply::ScrawlCapture(report) => *report,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    fn wait_for_last(
        host: &mut CaptureHost,
        audio: &FakeTimeline,
        bundle: &Path,
        idle: bool,
    ) -> ScrawlCaptureOutcome {
        for _ in 0..1_000 {
            let status = report(host.control(ScrawlCaptureAction::Status, audio));
            if (!idle || status.finishing.is_empty())
                && let Some(last) = status.last
                && last.bundle == bundle
            {
                return last;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the capture never finished");
    }

    #[test]
    fn controls_are_idempotent_and_stop_writes_a_scrawl_bundle() {
        let dir = temp_dir("control");
        let audio = FakeTimeline::default();
        let engine = FakeEngine::new(Arc::new(FakeTranscriber));
        let mut host = CaptureHost::with(engine.clone(), Some(dir.clone())).unwrap();

        let idle = report(host.control(ScrawlCaptureAction::Stop, &audio));
        assert!(!idle.changed && idle.bundle.is_none());
        // Paste and Inspect need a written capture.
        for command in [CaptureCommand::Paste, CaptureCommand::Inspect] {
            assert!(matches!(
                host.command(command, &audio),
                ControlReply::Error { .. }
            ));
        }
        assert!(!report(host.control(ScrawlCaptureAction::Cancel, &audio)).changed);

        let started = report(host.control(ScrawlCaptureAction::Start, &audio));
        assert!(started.changed);
        assert_eq!(started.state, ScrawlCaptureState::Recording);
        let bundle = started.bundle.unwrap();
        let again = report(host.control(ScrawlCaptureAction::Start, &audio));
        assert!(!again.changed);
        assert_eq!(again.bundle.as_ref(), Some(&bundle));
        let recording = lock(&host.outcomes).snapshot().recording.unwrap();
        assert_eq!(recording.bundle, bundle);

        // Half a second of audio arrives 300 ms after the start, as from a
        // cold microphone.
        let t0 = host.active.as_ref().unwrap().t0;
        audio.feed(8_000, t0 + Duration::from_millis(800));
        while CaptureInstant::now() < t0 + Duration::from_millis(820) {
            thread::sleep(Duration::from_millis(10));
        }
        let stopped = report(host.control(ScrawlCaptureAction::Stop, &audio));
        assert!(stopped.changed);
        assert_eq!(stopped.state, ScrawlCaptureState::Idle);
        assert_eq!(stopped.bundle.as_ref(), Some(&bundle));
        assert!(!report(host.control(ScrawlCaptureAction::Stop, &audio)).changed);

        let last = wait_for_last(&mut host, &audio, &bundle, true);
        assert_eq!(last.stage, "done");
        assert_eq!(last.words, Some(2));
        let snapshot = lock(&host.outcomes).snapshot();
        assert!(snapshot.recording.is_none() && snapshot.finishing.is_empty());
        assert_eq!(snapshot.last.unwrap().stage, Stage::Done);
        for command in [CaptureCommand::Paste, CaptureCommand::Inspect] {
            assert!(report(host.command(command, &audio)).changed);
        }
        // The fake audio ends before the stop, so the tail may count as a gap.
        assert!(last.gaps.is_some());
        assert_eq!(last.missing_transcript, None);

        // Hex's audio, on Hex's clock, into Hex's folder; nothing else.
        let start = engine.starts.lock().unwrap()[0].clone();
        assert_eq!(start.audio, AudioIn::External);
        assert!(matches!(start.target, Target::ActiveDisplay));
        assert_eq!(start.bundle_dir.as_ref(), Some(&bundle));
        assert_eq!(start.t0_boot_ns, t0.as_nanos());
        assert!(bundle.starts_with(&dir));

        // Scrawl's writer pads audio.wav to the capture's clock and moves
        // the words onto it.
        let wav = hound::WavReader::open(bundle.join("audio.wav")).unwrap();
        assert_eq!(wav.spec().sample_rate, 16_000);
        let leading = 300 * 16;
        let samples = wav
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(samples.len() >= leading + 8_000);
        assert!(samples[..leading - 16].iter().all(|sample| *sample == 0));
        assert!(
            samples[leading + 160..leading + 7_800]
                .iter()
                .all(|sample| *sample != 0)
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(bundle.join("scrawl.json")).unwrap()).unwrap();
        let words = &manifest["transcript"]["words"];
        assert_eq!(manifest["transcript"]["text"], "hello there");
        assert_eq!(words[1]["text"], "there");
        assert_eq!(words[1]["t0"], 550);
        assert_eq!(manifest["audio"], "audio.wav");
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&bundle), 0o700);
        for entry in fs::read_dir(&bundle).unwrap() {
            let entry = entry.unwrap();
            let expected = if entry.file_type().unwrap().is_dir() {
                0o700
            } else {
                0o600
            };
            assert_eq!(mode(&entry.path()), expected, "{}", entry.path().display());
        }

        // Cancel discards the bundle.
        let started = report(host.control(ScrawlCaptureAction::Start, &audio));
        let cancelled_bundle = started.bundle.unwrap();
        assert!(report(host.control(ScrawlCaptureAction::Cancel, &audio)).changed);
        assert!(!report(host.control(ScrawlCaptureAction::Cancel, &audio)).changed);
        let last = wait_for_last(&mut host, &audio, &cancelled_bundle, true);
        assert_eq!(last.stage, "cancelled");
        assert!(!cancelled_bundle.exists());
        assert!(bundle.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_engine_that_cannot_start_leaves_nothing_behind() {
        let dir = temp_dir("refused");
        let audio = FakeTimeline::default();
        let engine = Arc::new(FakeEngine {
            transcriber: Arc::new(FakeTranscriber),
            starts: Mutex::default(),
            steps: Steps::default(),
            refuse: true,
            phones: Vec::new(),
        });
        let mut host = CaptureHost::with(engine, Some(dir.clone())).unwrap();
        let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
            .bundle
            .unwrap();
        let last = wait_for_last(&mut host, &audio, &bundle, true);
        assert_eq!(last.stage, "failed");
        assert!(!bundle.exists());
        let status = report(host.control(ScrawlCaptureAction::Status, &audio));
        assert_eq!(status.state, ScrawlCaptureState::Idle);
        assert!(audio.open.lock().unwrap().is_none(), "the sink is closed");
        // The next start is a fresh capture.
        assert!(report(host.control(ScrawlCaptureAction::Start, &audio)).changed);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scrawls_engine_claims_nothing_machine_wide() {
        let (events, _received) = mpsc::sync_channel(1);
        let engine = scrawl_engine(events, Arc::new(FakeTranscriber));
        assert_eq!(engine.resources(), SharedResources::none());
    }

    #[test]
    fn the_engine_config_comes_from_hex() {
        let config = HexConfig.engine_config().unwrap();
        assert_eq!(
            scrawl_capture::autocrop::Settings::from_engine_config(&config),
            Some(Default::default()),
            "display captures crop with Scrawl's defaults"
        );
        assert!(!config.experiment(scrawl_capture::handoff::AGENT_CONTEXT));
        assert_eq!(config.transcriber, None);
    }

    #[test]
    fn padding_past_the_length_limit_stays_bounded() {
        let mut timeline = Timeline::with_limit(t(0), Duration::from_secs(1));
        timeline.push(&[0.5; 1_600], 16_000, t(100));
        // A gap far past the limit pads only up to it.
        timeline.gap(t(100), t(3 * 60 * 60 * 1_000));
        assert!(timeline.samples.len() <= 16_000);
        timeline.gap(t(3 * 60 * 60 * 1_000), t(6 * 60 * 60 * 1_000));
        timeline.push(&[0.5; 1_600], 16_000, t(6 * 60 * 60 * 1_000 + 100));
        let audio = timeline.finish(t(6 * 60 * 60 * 1_000 + 100), None);
        assert_eq!(audio.samples.len(), 16_000);
        assert_eq!(
            audio.gaps,
            [
                GapSpan {
                    start_ms: 100,
                    end_ms: 1_000
                },
                GapSpan {
                    start_ms: 1_000,
                    end_ms: 6 * 60 * 60 * 1_000 + 100
                }
            ]
        );
    }

    /// Holds every transcription until the test lets it through.
    struct GatedTranscriber(Mutex<Receiver<()>>);

    impl Transcriber for GatedTranscriber {
        fn transcribe(&self, _audio: &RecordedAudio) -> anyhow::Result<Option<TranscriptData>> {
            self.0.lock().unwrap().recv().ok();
            Ok(None)
        }
    }

    /// Waits until the one writer has taken `bundle`. Each capture's thread
    /// hands it over on its own schedule, so tests that need the first
    /// capture written first wait for it before starting the next.
    fn wait_until_writing(engine: &FakeEngine, bundle: &Path) {
        for _ in 0..1_000 {
            if engine
                .steps
                .lock()
                .unwrap()
                .iter()
                .any(|(step, dir)| *step == "finish" && dir == bundle)
            {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("{} was never written", bundle.display());
    }

    #[test]
    fn captures_finish_one_at_a_time_and_starts_wait_for_room() {
        let dir = temp_dir("bounded");
        let audio = FakeTimeline::default();
        let (gate, gated) = mpsc::channel();
        let engine = FakeEngine::new(Arc::new(GatedTranscriber(Mutex::new(gated))));
        let mut host = CaptureHost::with(engine.clone(), Some(dir.clone())).unwrap();
        let mut bundles = Vec::new();
        for _ in 0..MAX_FINISHING {
            let started = report(host.control(ScrawlCaptureAction::Start, &audio));
            let bundle = started.bundle.unwrap();
            // Some audio, so each capture has something to transcribe.
            audio.feed(
                160,
                host.active.as_ref().unwrap().t0 + Duration::from_millis(10),
            );
            thread::sleep(Duration::from_millis(20));
            host.control(ScrawlCaptureAction::Stop, &audio);
            if bundles.is_empty() {
                wait_until_writing(&engine, &bundle);
            }
            bundles.push(bundle);
        }
        assert_eq!(
            report(host.control(ScrawlCaptureAction::Status, &audio)).finishing,
            bundles
        );
        let ControlReply::Error { code, .. } = host.control(ScrawlCaptureAction::Start, &audio)
        else {
            panic!("a start past the bound must be refused");
        };
        assert_eq!(code, "busy");
        // One finishes at a time, in order; the outcome is recorded directly.
        gate.send(()).unwrap();
        let first = wait_for_last(&mut host, &audio, &bundles[0], false);
        assert_eq!(first.stage, "done");
        assert_eq!(
            report(host.control(ScrawlCaptureAction::Status, &audio)).finishing,
            bundles[1..]
        );
        gate.send(()).unwrap();
        assert_eq!(
            wait_for_last(&mut host, &audio, &bundles[1], true).stage,
            "done"
        );
        assert!(report(host.control(ScrawlCaptureAction::Start, &audio)).changed);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_capture_queued_behind_a_slow_finish_has_already_stopped() {
        let dir = temp_dir("stopped");
        let audio = FakeTimeline::default();
        let (gate, gated) = mpsc::channel();
        let engine = FakeEngine::new(Arc::new(GatedTranscriber(Mutex::new(gated))));
        let mut host = CaptureHost::with(engine.clone(), Some(dir.clone())).unwrap();
        let steps = |engine: &FakeEngine| engine.steps.lock().unwrap().clone();
        let mut bundles = Vec::new();
        for _ in 0..2 {
            let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
                .bundle
                .unwrap();
            audio.feed(
                160,
                host.active.as_ref().unwrap().t0 + Duration::from_millis(10),
            );
            host.control(ScrawlCaptureAction::Stop, &audio);
            if bundles.is_empty() {
                wait_until_writing(&engine, &bundle);
            }
            bundles.push(bundle);
        }
        // The first is stuck transcribing; the second waits behind it, its
        // screen already stopped.
        let (first, second) = (&bundles[0], &bundles[1]);
        let mut expected = vec![
            ("finish", first.clone()),
            ("stop", first.clone()),
            ("stop", second.clone()),
        ];
        expected.sort();
        let sorted = |mut steps: Vec<_>| {
            steps.sort();
            steps
        };
        for _ in 0..1_000 {
            if sorted(steps(&engine)) == expected {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(sorted(steps(&engine)), expected);
        assert_eq!(
            report(host.control(ScrawlCaptureAction::Status, &audio)).finishing,
            bundles
        );
        gate.send(()).unwrap();
        gate.send(()).unwrap();
        assert_eq!(wait_for_last(&mut host, &audio, second, true).stage, "done");
        assert_eq!(steps(&engine).last(), Some(&("finish", second.clone())));
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    /// Fails to start once released.
    struct StallingEngine(Mutex<Receiver<()>>);

    impl CaptureEngine for StallingEngine {
        fn start(&self, _start: Start) -> anyhow::Result<Box<dyn Recording>> {
            let _ = self.0.lock().unwrap().recv();
            anyhow::bail!("no screen to record")
        }
    }

    #[test]
    fn a_failed_start_ends_before_escape_or_the_shortcut_sees_it() {
        let dir = temp_dir("failed");
        let audio = FakeTimeline::default();
        let (release, released) = mpsc::channel();
        let mut host = CaptureHost::with(
            Arc::new(StallingEngine(Mutex::new(released))),
            Some(dir.clone()),
        )
        .unwrap();
        let first = report(host.shortcut(ShortcutAction::Record, &audio))
            .bundle
            .unwrap();
        release.send(()).unwrap();
        for _ in 0..1_000 {
            if lock(&host.outcomes).idle() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        // No control call came between: the next look ends it.
        assert_eq!(host.recording_since(&audio), None);
        assert!(!first.exists());
        // So the next press starts a capture instead of stopping one.
        let second = report(host.shortcut(ShortcutAction::Record, &audio));
        assert!(second.changed);
        assert_eq!(second.state, ScrawlCaptureState::Recording);
        assert_ne!(second.bundle.as_ref(), Some(&first));
        release.send(()).unwrap();
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_start_raced_by_stop_never_wedges_later_starts() {
        let dir = temp_dir("raced");
        let audio = FakeTimeline::default();
        let (release, released) = mpsc::channel();
        let mut host = CaptureHost::with(
            Arc::new(StallingEngine(Mutex::new(released))),
            Some(dir.clone()),
        )
        .unwrap();
        for _ in 0..3 {
            let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
                .bundle
                .unwrap();
            // The engine fails and its collector settles inside the stop,
            // after the stop saw it still starting.
            let (release, outcomes, failed) =
                (release.clone(), host.outcomes.clone(), bundle.clone());
            *audio.on_close.lock().unwrap() = Some(Box::new(move || {
                release.send(()).unwrap();
                while lock(&outcomes)
                    .last
                    .as_ref()
                    .is_none_or(|last| last.bundle != failed)
                {
                    thread::sleep(Duration::from_millis(5));
                }
            }));
            assert!(report(host.control(ScrawlCaptureAction::Stop, &audio)).changed);
            let status = report(host.control(ScrawlCaptureAction::Status, &audio));
            assert!(status.finishing.is_empty(), "{:?}", status.finishing);
            assert_eq!(status.last.unwrap().stage, "failed");
            assert!(!bundle.exists());
        }
        host.shutdown(&audio);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn shutdown_cancels_the_recording_capture_and_waits_for_its_cleanup() {
        let dir = temp_dir("shutdown");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let bundle = report(host.control(ScrawlCaptureAction::Start, &audio))
            .bundle
            .unwrap();
        assert!(bundle.exists());
        host.shutdown(&audio);
        assert!(!bundle.exists());
        let last = report(host.control(ScrawlCaptureAction::Status, &audio))
            .last
            .unwrap();
        assert_eq!((last.bundle, last.stage.as_str()), (bundle, "cancelled"));
        let ControlReply::Error { code, .. } = host.control(ScrawlCaptureAction::Start, &audio)
        else {
            panic!("a stopped host starts nothing");
        };
        assert_eq!(code, "busy");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_host_is_absent_while_captures_are_off() {
        // Captures are off by default, so no host, engine, thread, or sink
        // exists and the listener answers `feature-off`.
        assert!(!crate::app_settings::CaptureSettings::default().enabled);
        if !crate::app_settings::capture_on() {
            assert!(CaptureHost::start(None, &Environment::for_test()).is_none());
        }
    }

    #[test]
    fn the_shortcut_starts_and_a_later_press_stops() {
        let dir = temp_dir("shortcut");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let now = CaptureInstant::now();
        assert_eq!(host.shortcut_activity(now, &audio), ShortcutActivity::Free);
        let started = report(host.shortcut(ShortcutAction::Record, &audio));
        assert!(started.changed);
        let t0 = host.recording_since(&audio).unwrap();
        assert_eq!(
            host.shortcut_activity(t0 + Duration::from_millis(100), &audio),
            ShortcutActivity::Recording {
                elapsed: Duration::from_millis(100)
            }
        );
        let stopped = report(host.shortcut(ShortcutAction::Finish, &audio));
        assert!(stopped.changed);
        assert_eq!(stopped.state, ScrawlCaptureState::Idle);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    fn wait_until_idle(host: &CaptureHost) {
        for _ in 0..1_000 {
            if lock(&host.outcomes).idle() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the capture never settled");
    }

    #[test]
    fn a_discarded_capture_leaves_no_trace_and_a_pick_takes_its_place() {
        let dir = temp_dir("discard");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let first = report(host.shortcut(ShortcutAction::Record, &audio))
            .bundle
            .unwrap();
        let recording = lock(&host.outcomes).snapshot().recording.unwrap();
        assert_eq!(
            (recording.target, recording.worked),
            (TargetKind::Pointer, false)
        );

        let discarded = report(host.command(CaptureCommand::Discard, &audio));
        assert!(discarded.changed);
        assert_eq!(
            (discarded.state, discarded.bundle),
            (ScrawlCaptureState::Idle, None)
        );
        // Views see nothing at once: no recording, nothing finishing, no
        // outcome, so the capsule can turn into the picker.
        assert_eq!(lock(&host.outcomes).snapshot(), CaptureSnapshot::default());
        assert!(audio.open.lock().unwrap().is_none(), "the sink is closed");
        wait_until_idle(&host);
        assert!(!first.exists());
        assert_eq!(lock(&host.outcomes).snapshot(), CaptureSnapshot::default());
        assert!(
            report(host.control(ScrawlCaptureAction::Status, &audio))
                .last
                .is_none()
        );

        let pick = CaptureTarget::Area {
            rect: [10.0, 20.0, 300.0, 200.0],
        };
        let second = report(host.command(CaptureCommand::Start(pick), &audio))
            .bundle
            .unwrap();
        assert_ne!(first, second);
        let recording = lock(&host.outcomes).snapshot().recording.unwrap();
        assert_eq!(
            (recording.bundle, recording.target),
            (second, TargetKind::Area)
        );
        // A pick while one records does nothing.
        let again = host.command(CaptureCommand::Start(CaptureTarget::Pointer), &audio);
        assert!(!report(again).changed);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_iphone_capture_narrates_with_hex_audio_and_finishes_when_its_mirror_ends() {
        let dir = temp_dir("phone");
        let audio = FakeTimeline::default();
        let engine = Arc::new(FakeEngine {
            transcriber: Arc::new(FakeTranscriber),
            starts: Mutex::default(),
            steps: Steps::default(),
            refuse: false,
            phones: vec!["iPhone".into()],
        });
        let mut host = CaptureHost::with(engine.clone(), Some(dir.clone())).unwrap();
        host.command(CaptureCommand::FindPhone, &audio);
        wait_for(|| lock(&host.outcomes).snapshot().phone.as_deref() == Some("iPhone"));

        let bundle = report(host.command(CaptureCommand::Start(CaptureTarget::Phone), &audio))
            .bundle
            .unwrap();
        let recording = lock(&host.outcomes).snapshot().recording.unwrap();
        assert_eq!(recording.target, TargetKind::Phone);
        wait_for(|| !engine.starts.lock().unwrap().is_empty());
        let start = engine.starts.lock().unwrap()[0].clone();
        assert!(matches!(start.target, Target::Phone { .. }));
        assert_eq!(start.audio, AudioIn::External);

        // Unplugged: the next look at the host finishes what it recorded.
        lock(&host.outcomes).phone_ended = true;
        assert_eq!(host.recording_since(&audio), None);
        let last = wait_for_last(&mut host, &audio, &bundle, true);
        assert_eq!(last.stage, "done");
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(target_os = "macos")]
    /// Kit: Escape may be meant for the app being recorded, so it never
    /// ends a recording capture, and Hex's tap lets it through.
    #[test]
    fn escape_never_ends_a_recording_capture() {
        let dir = temp_dir("escape");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let mut shortcut = CaptureShortcut::default();
        report(host.shortcut(ShortcutAction::Record, &audio));
        let escape = crate::suppression::InputEvent::Key {
            code: 53,
            down: true,
            flags: 0,
        };
        let activity = host.shortcut_activity(CaptureInstant::now(), &audio);
        assert!(matches!(activity, ShortcutActivity::Recording { .. }));
        let action = shortcut.input(escape, Default::default(), activity);
        assert_eq!(action, ShortcutAction::None);
        host.shortcut(action, &audio);
        // The recording capsule takes no keys, so Escape reaches the app.
        assert!(!capsule_key(CapsuleKey::Escape));
        assert!(!shortcut.takes_escape());
        assert!(host.recording_since(&audio).is_some());
        // The picker before a recording still takes it.
        let grab = take_keys(CapsuleKeys::Picker, |_| {});
        assert!(shortcut.takes_escape());
        drop(grab);
        assert!(!shortcut.takes_escape());
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_mirror_ending_never_stops_a_screen_capture() {
        let dir = temp_dir("phone-ended");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        report(host.shortcut(ShortcutAction::Record, &audio));
        lock(&host.outcomes).phone_ended = true;
        assert!(host.recording_since(&audio).is_some());
        assert!(!lock(&host.outcomes).phone_ended);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    fn wait_for(done: impl Fn() -> bool) {
        for _ in 0..1_000 {
            if done() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out");
    }

    #[test]
    fn a_switch_never_shows_nothing_recording() {
        let feed: &'static Feed = Box::leak(Box::new(Feed::new()));
        let dir = temp_dir("switch");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        lock(&host.outcomes).feed = feed;
        let first = report(host.shortcut(ShortcutAction::Record, &audio))
            .bundle
            .unwrap();
        let watch = feed.watch();
        assert_eq!(watch.changed().unwrap().recording.unwrap().bundle, first);

        let pick = CaptureTarget::Display { at: [5.0, 5.0] };
        let switched = report(host.command(CaptureCommand::Switch(pick), &audio));
        let second = switched.bundle.unwrap();
        assert_ne!(first, second);
        assert_eq!(switched.state, ScrawlCaptureState::Recording);
        // One publish: the new recording, never the gap before it.
        let seen = watch.changed().unwrap();
        let recording = seen.recording.unwrap();
        assert_eq!(
            (recording.bundle, recording.target),
            (second, TargetKind::Display)
        );
        assert!(seen.finishing.is_empty() && seen.last.is_none());
        wait_until_idle_but_one(&host);
        assert!(!first.exists());
        assert!(lock(&host.outcomes).snapshot().last.is_none());
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_capture_stays_quiet_until_it_stops_is_cancelled_or_hex_quits() {
        use crate::recording_environment::Effect::Mute;
        use crate::recording_environment::observed::{
            self,
            Event::{Held, Restored},
        };
        let dir = temp_dir("quiet");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let (environment, events) = observed::controller();
        host.environment = Some(environment.clone());
        host.audio_behavior = Some(crate::app_settings::RecordingAudioBehavior::Mute);
        let settled = || observed::settled(&environment, &events);

        let start = |host: &mut CaptureHost| {
            report(host.control(ScrawlCaptureAction::Start, &audio))
                .bundle
                .unwrap()
        };
        start(&mut host);
        assert_eq!(settled(), [Held(Mute)]);
        // A switch keeps output muted from one capture to the next.
        let switch = CaptureCommand::Switch(CaptureTarget::Pointer);
        let bundle = report(host.command(switch, &audio)).bundle.unwrap();
        assert_eq!(settled(), []);
        report(host.control(ScrawlCaptureAction::Stop, &audio));
        assert_eq!(settled(), [Restored(Mute)]);
        wait_for_last(&mut host, &audio, &bundle, true);

        let bundle = start(&mut host);
        assert_eq!(settled(), [Held(Mute)]);
        report(host.control(ScrawlCaptureAction::Cancel, &audio));
        assert_eq!(settled(), [Restored(Mute)]);
        wait_for_last(&mut host, &audio, &bundle, true);

        start(&mut host);
        assert_eq!(settled(), [Held(Mute)]);
        host.shutdown(&audio);
        assert_eq!(settled(), [Restored(Mute)]);
        fs::remove_dir_all(dir).ok();
    }

    fn wait_until_idle_but_one(host: &CaptureHost) {
        for _ in 0..1_000 {
            if lock(&host.outcomes).unfinished.len() == 1 {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("the discarded capture never settled");
    }

    #[test]
    fn discarded_captures_never_count_against_the_writers() {
        let dir = temp_dir("discard-busy");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        // Discards still being removed are not captures being written.
        let mut outcomes = lock(&host.outcomes);
        for index in 0..MAX_FINISHING {
            let bundle = dir.join(format!("gone-{index}"));
            outcomes.started(bundle.clone(), t(0), TargetKind::Pointer);
            outcomes.stopped(&bundle, End::Discard);
        }
        assert_eq!(outcomes.writing(), 0);
        drop(outcomes);
        assert!(
            report(host.command(CaptureCommand::Start(CaptureTarget::Pointer), &audio)).changed
        );
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_switch_with_no_room_to_start_keeps_the_recording() {
        let dir = temp_dir("switch-busy");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let recording = report(host.control(ScrawlCaptureAction::Start, &audio))
            .bundle
            .unwrap();
        {
            let mut outcomes = lock(&host.outcomes);
            for index in 0..MAX_FINISHING {
                let bundle = dir.join(format!("writing-{index}"));
                outcomes.started(bundle.clone(), t(0), TargetKind::Pointer);
                outcomes.stopped(&bundle, End::Finish);
            }
        }
        let switch = CaptureCommand::Switch(CaptureTarget::Pointer);
        assert!(matches!(
            host.command(switch, &audio),
            ControlReply::Error { code, .. } if code == "busy"
        ));
        let snapshot = lock(&host.outcomes).snapshot();
        assert_eq!(snapshot.recording.unwrap().bundle, recording);
        assert_eq!(snapshot.last, None);
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_switch_that_cannot_start_after_its_discard_settles_failed() {
        let dir = temp_dir("switch-failed");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let first = report(host.control(ScrawlCaptureAction::Start, &audio))
            .bundle
            .unwrap();
        // No captures folder can be made: the new bundle fails.
        fs::remove_dir_all(&dir).unwrap();
        fs::write(&dir, "").unwrap();
        let switch = CaptureCommand::Switch(CaptureTarget::Pointer);
        assert!(matches!(
            host.command(switch, &audio),
            ControlReply::Error { code, .. } if code == "failed"
        ));
        let snapshot = lock(&host.outcomes).snapshot();
        assert_eq!(snapshot.recording, None);
        let last = snapshot.last.unwrap();
        assert_eq!((last.bundle, last.stage), (first, Stage::Failed));
        assert!(last.error.is_some());
        host.shutdown(&audio);
        fs::remove_file(dir).ok();
    }

    #[test]
    fn a_pick_that_cannot_start_says_why() {
        let dir = temp_dir("closed");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let closed = CaptureCommand::Start(CaptureTarget::Window { id: 0 });
        let bundle = report(host.command(closed, &audio)).bundle.unwrap();
        // The failed start can clear the recording before this reads it.
        if let Some(recording) = lock(&host.outcomes).snapshot().recording {
            assert_eq!(recording.target, TargetKind::Window);
        }
        let last = wait_for_last(&mut host, &audio, &bundle, true);
        assert_eq!(last.stage, "failed");
        assert!(!bundle.exists());
        let settled = lock(&host.outcomes).snapshot().last.unwrap();
        assert_eq!(settled.error.as_deref(), Some("the window closed"));
        assert!(matches!(
            host.command(CaptureCommand::Paste, &audio),
            ControlReply::Error { .. }
        ));
        host.shutdown(&audio);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn route_commands_wait_for_a_routed_capture() {
        let dir = temp_dir("route");
        let audio = FakeTimeline::default();
        let mut host = CaptureHost::with(
            FakeEngine::new(Arc::new(FakeTranscriber)),
            Some(dir.clone()),
        )
        .unwrap();
        let commands = [
            CaptureCommand::Send {
                to: "ses_a".into(),
                always: true,
            },
            CaptureCommand::Undo,
        ];
        for command in commands {
            assert!(report(host.command(command, &audio)).changed);
        }
        host.shutdown(&audio);
        let snapshot = lock(&host.outcomes).snapshot();
        assert_eq!(snapshot.last, None);
        assert!(matches!(
            host.command(CaptureCommand::Undo, &audio),
            ControlReply::Error { code, .. } if code == "busy"
        ));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_first_click_or_scroll_means_work_started() {
        let live = LiveInput(Arc::default());
        let outcomes = SharedOutcomes::default();
        let bundle = PathBuf::from("/work.scrawl");
        lock(&outcomes).started(bundle.clone(), t(0), TargetKind::Pointer);
        live.recording(Some(&bundle));
        live.attach(&bundle, InputSink::default(), t(0), false, &outcomes);
        let (sender, received) = mpsc::sync_channel(4);
        let pointer = |x| {
            Tapped::Pointer(HostInput::Moved {
                x,
                y: 0.0,
                modifiers: Modifiers::default(),
            })
        };
        sender.send((t(10), pointer(1.0))).unwrap();
        sender
            .send((
                t(20),
                Tapped::Pointer(HostInput::Button {
                    down: true,
                    right: false,
                    x: 1.0,
                    y: 0.0,
                    modifiers: Modifiers::default(),
                }),
            ))
            .unwrap();
        drop(sender);
        live.forward(received);
        assert!(lock(&outcomes).snapshot().recording.unwrap().worked);
    }

    #[test]
    fn input_reaches_only_the_capture_that_is_recording() {
        let live = LiveInput(Arc::default());
        let sink = InputSink::default();
        let (first, second) = (PathBuf::from("/a.scrawl"), PathBuf::from("/b.scrawl"));
        // A capture stopped before its engine started never takes input.
        live.recording(Some(&first));
        live.recording(None);
        let outcomes = SharedOutcomes::default();
        live.attach(&first, sink.clone(), t(0), false, &outcomes);
        assert!(live.state().target.is_none());
        // An old collector detaching never takes the new capture's input.
        live.recording(Some(&second));
        live.attach(&second, sink, t(0), false, &outcomes);
        live.detach(&first);
        assert!(live.state().target.is_some());
        live.recording(None);
        assert!(live.state().target.is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_shortcut_records_once_its_keys_come_up() {
        use crate::app_settings::{OPTION_KEY_MASK, SHIFT_KEY_MASK};
        use crate::suppression::InputEvent;
        use ShortcutAction::*;
        let hotkeys = crate::app_settings::RuntimeHotkeys {
            capture: Some(crate::app_settings::HotkeyBinding::capture_default().runtime()),
            ..Default::default()
        };
        let code = crate::app_settings::HotkeyBinding::capture_default()
            .key
            .unwrap()
            .code;
        let chord = OPTION_KEY_MASK | SHIFT_KEY_MASK;
        let key = |down, flags| InputEvent::Key { code, down, flags };
        let escape = InputEvent::Key {
            code: 53,
            down: true,
            flags: chord,
        };
        let free = ShortcutActivity::Free;
        let mut shortcut = CaptureShortcut::default();

        // Held as long as you like, autorepeats and all: nothing yet.
        assert_eq!(shortcut.input(key(true, chord), hotkeys, free), None);
        assert!(shortcut.armed());
        assert_eq!(shortcut.input(key(true, chord), hotkeys, free), None);
        assert_eq!(shortcut.input(key(false, chord), hotkeys, free), None);
        // One modifier coming up starts it.
        assert_eq!(
            shortcut.input(InputEvent::Flags(OPTION_KEY_MASK), hotkeys, free),
            Record
        );
        assert!(!shortcut.armed());
        assert_eq!(shortcut.input(InputEvent::Flags(0), hotkeys, free), None);

        // Escape before the keys come up cancels; the release then does nothing.
        assert_eq!(shortcut.input(key(true, chord), hotkeys, free), None);
        assert_eq!(shortcut.input(escape, hotkeys, free), Cancel);
        assert_eq!(shortcut.input(key(false, 0), hotkeys, free), None);
        assert_eq!(shortcut.input(InputEvent::Flags(0), hotkeys, free), None);

        // A capture started another way meanwhile (`hex capture start`):
        // Escape only disarms the press, and ends nothing.
        assert_eq!(shortcut.input(key(true, chord), hotkeys, free), None);
        let started = ShortcutActivity::Recording {
            elapsed: Duration::from_secs(2),
        };
        assert_eq!(shortcut.input(escape, hotkeys, started), None);
        assert!(!shortcut.armed());
        assert_eq!(shortcut.input(key(false, 0), hotkeys, started), None);
        assert_eq!(shortcut.input(InputEvent::Flags(0), hotkeys, started), None);
        // And a recording one never hears it.
        assert_eq!(shortcut.input(escape, hotkeys, started), None);

        // While recording a press finishes, except right after the start.
        let recording = |ms| ShortcutActivity::Recording {
            elapsed: Duration::from_millis(ms),
        };
        assert_eq!(
            shortcut.input(key(true, chord), hotkeys, recording(100)),
            None
        );
        assert_eq!(
            shortcut.input(key(false, chord), hotkeys, recording(150)),
            None
        );
        assert_eq!(
            shortcut.input(key(true, chord), hotkeys, recording(400)),
            Finish
        );
        assert_eq!(
            shortcut.input(key(true, chord), hotkeys, recording(500)),
            None
        );
        assert_eq!(shortcut.input(key(false, chord), hotkeys, free), None);
        // While picking, the picker records.
        let picking = ShortcutActivity::Picking;
        assert_eq!(shortcut.input(key(true, chord), hotkeys, picking), Pick);
        assert_eq!(shortcut.input(key(false, chord), hotkeys, picking), None);

        // The key without its modifiers is typing; with captures off, nothing.
        assert_eq!(shortcut.input(key(true, 0), hotkeys, free), None);
        assert_eq!(shortcut.input(key(false, 0), hotkeys, free), None);
        assert_eq!(
            shortcut.input(key(true, chord), Default::default(), free),
            None
        );
        assert!(!shortcut.armed());

        // A release lost while shortcuts were suspended never eats a press,
        // and an armed press records nothing.
        assert_eq!(shortcut.input(key(true, chord), hotkeys, free), None);
        shortcut.reset();
        assert_eq!(shortcut.input(InputEvent::Flags(0), hotkeys, free), None);
        assert_eq!(shortcut.input(key(true, chord), hotkeys, free), None);
        assert!(shortcut.armed());
    }

    /// One test, since the grab is the process's.
    #[test]
    fn the_newest_grab_takes_only_its_keys_until_it_drops() {
        let (sent, keys) = mpsc::channel();
        let first = {
            let sent = sent.clone();
            take_keys(CapsuleKeys::Picker, move |key| sent.send((1, key)).unwrap())
        };
        let second = take_keys(CapsuleKeys::Picker, move |key| sent.send((2, key)).unwrap());
        assert!(picking() && wants_escape() && !list_key_wanted());
        assert!(capsule_key(CapsuleKey::Shortcut));
        assert!(!capsule_key(CapsuleKey::OpenList));
        drop(first);
        assert!(capsule_key(CapsuleKey::Escape));

        // A routed capsule: the list shortcut opens the list; never Escape.
        second.set(CapsuleKeys::Route);
        assert!(!picking() && !wants_escape() && list_key_wanted());
        assert!(!capsule_key(CapsuleKey::Shortcut));
        assert!(!capsule_key(CapsuleKey::Escape));
        assert!(capsule_key(CapsuleKey::OpenList));
        assert!(
            !capsule_key(CapsuleKey::Press),
            "a press matters only to the list"
        );
        second.set(CapsuleKeys::Route);
        assert!(!wants_escape() && !capsule_key(CapsuleKey::Escape));
        // Suggestions: the list shortcut and only their own digits, never
        // Escape.
        second.set(CapsuleKeys::Suggest {
            rows: 2,
            cycle: false,
        });
        assert!(suggesting() && !wants_escape() && list_key_wanted() && !list_open());
        assert!(!capsule_key(CapsuleKey::Escape));
        assert!(capsule_key(CapsuleKey::Press), "a press may fold them");
        assert!(capsule_key(CapsuleKey::Shift(true)), "Shift shows the rule");
        assert!(capsule_key(CapsuleKey::Digit(2)));
        assert!(!capsule_key(CapsuleKey::Digit(3)), "no third row");
        assert!(!capsule_key(CapsuleKey::Text("h".into())));
        assert!(!capsule_key(CapsuleKey::Tab { shift: false }));
        assert!(!capsule_key(CapsuleKey::Return { shift: false }));
        // The toast's one guess at a time: Return sends it, Tab moves on.
        second.set(CapsuleKeys::Suggest {
            rows: 2,
            cycle: true,
        });
        assert!(suggesting());
        assert!(capsule_key(CapsuleKey::Tab { shift: false }));
        assert!(capsule_key(CapsuleKey::Return { shift: false }));
        assert!(!capsule_key(CapsuleKey::Return { shift: true }));
        second.set(CapsuleKeys::List);
        assert!(!suggesting());
        assert!(list_open() && !wants_escape() && list_key_wanted());
        assert!(capsule_key(CapsuleKey::Text("ho".into())));
        assert!(capsule_key(CapsuleKey::Press));
        drop(second);
        assert!(!picking() && !list_open() && !list_key_wanted());
        assert!(!capsule_key(CapsuleKey::Escape));
        assert_eq!(
            keys.try_iter().collect::<Vec<_>>(),
            [
                (2, CapsuleKey::Shortcut),
                (2, CapsuleKey::Escape),
                (2, CapsuleKey::OpenList),
                (2, CapsuleKey::Press),
                (2, CapsuleKey::Shift(true)),
                (2, CapsuleKey::Digit(2)),
                (2, CapsuleKey::Tab { shift: false }),
                (2, CapsuleKey::Return { shift: false }),
                (2, CapsuleKey::Text("ho".into())),
                (2, CapsuleKey::Press),
            ]
        );
    }

    #[test]
    fn the_open_list_reads_keys_and_leaves_command_chords_to_the_app() {
        let (shift, option, command, control) = (1 << 17, 1 << 19, 1 << 20, 1 << 18);
        let key = |key| ListTap::Key(key);
        assert_eq!(list_key(53, 0, "\u{1b}"), key(CapsuleKey::Escape));
        assert_eq!(
            list_key(36, 0, "\r"),
            key(CapsuleKey::Return { shift: false })
        );
        assert_eq!(
            list_key(76, shift, "\u{3}"),
            key(CapsuleKey::Return { shift: true })
        );
        assert_eq!(list_key(48, 0, "\t"), key(CapsuleKey::Tab { shift: false }));
        assert_eq!(list_key(126, 0, ""), key(CapsuleKey::Up));
        assert_eq!(list_key(125, 0, ""), key(CapsuleKey::Down));
        assert_eq!(list_key(51, 0, "\u{7f}"), key(CapsuleKey::Backspace));
        assert_eq!(list_key(18, 0, "1"), key(CapsuleKey::Digit(1)));
        // Shift's digits type symbols; the row reads by position.
        assert_eq!(list_key(19, shift, "@"), key(CapsuleKey::Digit(2)));
        assert_eq!(list_key(25, shift, "("), key(CapsuleKey::Digit(9)));
        assert_eq!(list_key(29, 0, "0"), key(CapsuleKey::Text("0".into())));
        assert_eq!(list_key(18, option, "¡"), key(CapsuleKey::Text("¡".into())));
        assert_eq!(list_key(4, shift, "H"), key(CapsuleKey::Text("H".into())));
        assert_eq!(list_key(123, 0, ""), ListTap::Swallow);
        assert_eq!(list_key(9, command, "v"), ListTap::Pass);
        assert_eq!(list_key(53, control, "\u{1b}"), ListTap::Pass);
    }

    #[test]
    fn tap_flags_become_modifiers() {
        let held = modifiers(0x0008_0000 | 0x0002_0000);
        assert!(held.option && held.shift && !held.command && !held.control);
        assert!(!held.shift_only());
        assert!(modifiers(0x0002_0000).shift_only());
    }

    #[test]
    fn the_snapshot_follows_a_capture_and_ignores_late_progress() {
        let feed: &'static Feed = Box::leak(Box::new(Feed::new()));
        let watch = feed.watch();
        assert_eq!(watch.changed(), Some(CaptureSnapshot::default()));
        assert_eq!(watch.changed(), None);

        let mut outcomes = Outcomes::on(feed);
        let (one, two) = (PathBuf::from("/c/one"), PathBuf::from("/c/two"));
        let since = CaptureInstant::from_nanos(5);
        outcomes.started(one.clone(), since, TargetKind::Pointer);
        let recording = watch.changed().unwrap();
        assert_eq!(
            recording.recording,
            Some(RecordingCapture {
                bundle: one.clone(),
                since,
                screen: false,
                target: TargetKind::Pointer,
                worked: false,
            })
        );
        outcomes.progress(&one, Stage::Recording);
        // Changes coalesce into the latest snapshot.
        outcomes.stopped(&one, End::Finish);
        outcomes.progress(&one, Stage::Transcribing);
        let finishing = watch.changed().unwrap();
        assert!(finishing.generation >= recording.generation + 3);
        assert_eq!(finishing.recording, None);
        assert_eq!(
            finishing.finishing,
            [FinishingCapture {
                bundle: one.clone(),
                stage: Stage::Transcribing,
                cancelling: false
            }]
        );

        // A second capture records while the first is written.
        outcomes.started(two.clone(), since, TargetKind::Window);
        outcomes.stopped(&two, End::Cancel);
        // A cancelled capture's stage stays put.
        outcomes.progress(&two, Stage::Writing);
        outcomes.settle(
            one.clone(),
            Stage::Done,
            Some(Written {
                words: 3,
                gaps: 0,
                missing_transcript: None,
            }),
        );
        let done = watch.changed().unwrap();
        assert_eq!(
            done.finishing,
            [FinishingCapture {
                bundle: two.clone(),
                stage: Stage::Stopping,
                cancelling: true
            }]
        );
        let last = done.last.clone().unwrap();
        assert_eq!(
            (last.bundle, last.stage, last.words, last.route),
            (one.clone(), Stage::Done, Some(3), None)
        );

        // Only the newest capture's route shows.
        outcomes.routed(&two, CaptureRoute::looking());
        assert_eq!(watch.changed(), None);
        outcomes.routed(&one, CaptureRoute::looking());
        let routed = watch.changed().unwrap();
        assert_eq!(
            routed.last.clone().unwrap().route,
            Some(CaptureRoute::looking())
        );
        outcomes.routed(&one, CaptureRoute::looking());
        assert_eq!(watch.changed(), None);
        let done = routed;

        // The engine's own Done and other late progress change nothing.
        outcomes.progress(&one, Stage::Done);
        outcomes.progress(&one, Stage::Writing);
        outcomes.stopped(&one, End::Finish);
        assert_eq!(watch.changed(), None);
        assert_eq!(feed.snapshot(), done);

        outcomes.settle(two.clone(), Stage::Cancelled, None);
        let idle = watch.wait(Duration::from_secs(1)).unwrap();
        assert!(idle.recording.is_none() && idle.finishing.is_empty());
        assert_eq!(idle.last.unwrap().stage, Stage::Cancelled);
        assert!(outcomes.idle());

        // A dropped watcher stops being woken.
        drop(watch);
        outcomes.started(one, since, TargetKind::Pointer);
        assert!(feed.watchers.lock().unwrap().is_empty());
    }

    fn words(times: &[(&str, i64, i64)]) -> Vec<TimedWord> {
        times
            .iter()
            .map(|&(text, start_ms, end_ms)| TimedWord {
                start_ms,
                end_ms,
                text: text.into(),
            })
            .collect()
    }

    fn times(words: &[WordTiming]) -> Vec<(u64, u64)> {
        words.iter().map(|word| (word.t0, word.t1)).collect()
    }

    #[test]
    fn word_times_move_earlier_by_the_measured_lag() {
        let lag = word_lag(TranscriptionModelId::ParakeetUnifiedEnglish);
        let corrected = corrected_words(
            words(&[
                // The first word counts as after a pause.
                ("one", 600, 900),
                ("two", 900, 1_200),
                // 400 ms since "two" ended: a pause.
                ("three", 1_600, 1_900),
                // 200 ms: not a pause.
                ("four", 2_100, 2_150),
            ]),
            lag,
        );
        assert_eq!(
            times(&corrected),
            [(80, 705), (620, 1_005), (1_080, 1_705), (1_820, 1_955)]
        );
        assert_eq!(corrected[2].text, "three");

        // Times stay at 0 or later and never go backwards.
        let clamped = corrected_words(
            words(&[("a", 100, 150), ("b", 1_000, 1_300), ("c", 900, 950)]),
            lag,
        );
        assert_eq!(times(&clamped), [(0, 0), (480, 1_105), (620, 1_105)]);
    }

    #[test]
    fn only_a_measured_model_and_file_is_corrected() {
        for (model, _, lag) in WORD_LAGS {
            assert_eq!(
                word_lag(*model),
                Some(lag),
                "{} has a new pinned file: measure its word lag again \
                 (docs/plans/capture-in-hex.md) before keeping the correction",
                model.as_str()
            );
        }
        assert_eq!(word_lag(TranscriptionModelId::ParakeetV3), None);
        assert_eq!(word_lag(TranscriptionModelId::AppleSpeech), None);
        let raw = corrected_words(words(&[("one", 600, 900), ("two", 900, 1_200)]), None);
        assert_eq!(times(&raw), [(600, 900), (900, 1_200)]);
    }
}
