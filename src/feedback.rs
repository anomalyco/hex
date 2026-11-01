use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use rodio::buffer::SamplesBuffer;
use rodio::cpal::BufferSize;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Source};
use serde::{Deserialize, Serialize};

mod imessage;

/// Auditionable key-sound styles for the onboarding typing demo.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KeySoundStyle {
    /// A real mechanical keyboard, presses cut from a CC0 recording of an IBM
    /// Model M, each at one of three pitches (default).
    #[default]
    Clack,
    /// Dry, crisp 13 ms Apple Taptic-style micro-pulse.
    Taptic,
    /// Whisper-quiet 28 ms felt-damped Magic Keyboard tap.
    Felt,
    /// Rounded 22 ms low-pitch warm droplet.
    Soft,
    /// Silent typing (only Hex's dictation tones play).
    Silent,
}

impl KeySoundStyle {
    pub const ALL: [Self; 5] = [
        Self::Clack,
        Self::Taptic,
        Self::Felt,
        Self::Soft,
        Self::Silent,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Clack => "Mechanical clack",
            Self::Taptic => "Taptic tick",
            Self::Felt => "Muted felt",
            Self::Soft => "Soft warm",
            Self::Silent => "Silent typing",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Clack => Self::Taptic,
            Self::Taptic => Self::Felt,
            Self::Felt => Self::Soft,
            Self::Soft => Self::Silent,
            Self::Silent => Self::Clack,
        }
    }
}

/// A Messages tapback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tapback {
    Heart,
    ThumbsUp,
    ThumbsDown,
    Haha,
    Exclamation,
    Question,
}

impl Tapback {
    pub const ALL: [Self; 6] = [
        Self::Heart,
        Self::ThumbsUp,
        Self::ThumbsDown,
        Self::Haha,
        Self::Exclamation,
        Self::Question,
    ];
}

/// Soft, varied tactile and motion cues for first-run onboarding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OnboardingTone {
    /// One of `style`'s distinct key taps: six for most styles, and for
    /// [`KeySoundStyle::Clack`] eight presses at three pitches
    /// ([`CLACK_KEYS`]` * pitch + press`).
    Key(KeySoundStyle, u8),
    /// Deeper stabilized space / return bar tap in `style`.
    KeySpace(KeySoundStyle),
    /// Soft delete / backspace tap in `style`.
    KeyBackspace(KeySoundStyle),
    /// Dictation shortcut keycap pressed down.
    ShortcutDown,
    /// Dictation shortcut keycap released.
    ShortcutUp,
    /// The single-hexagon mark arriving or settling.
    Hexagon,
    /// The Accessibility helper detaching and docking beside System Settings.
    PanelEscape,
    /// A permission row ticking to `Allowed`.
    PermissionTick,
    /// `"Almost ready"` appearing while finishing a slow model download.
    AlmostReady,
    /// One of the Hex logo's three hexagons landing on `"Hex is ready"`,
    /// outside in (`0..3`), rising to resolve on the last.
    Build(u8),
    /// A message arriving in the `"Without Hex"` conversation.
    Incoming,
    /// A reply leaving the field.
    Sent,
    /// The deadpan `"Read"` under a reply that should not have been sent.
    Read,
    /// The comedy winding back to before it went wrong.
    Rewind,
    /// A tapback landing on a message, each kind with its own sound.
    Tapback(Tapback),
    /// One low heartbeat under panicked typing.
    Heartbeat,
    /// The heartbeat stretched lower in slow motion.
    HeartbeatSlow,
    /// Time slowing hard: the clicks smear down.
    SlowDown,
    /// A comic title slamming onto the stage: a heavy bass hit.
    Slam,
    /// Confetti bursting over `"With Hex"`: a popper and falling paper.
    Confetti,
    /// Fireworks over `"With Hex"`: a whistle, a boom, and crackle.
    Fireworks,
    /// The triumphant sting as `"With Hex"` peaks: one of the candidates
    /// (`0..TRIUMPHS`) still being chosen between.
    Triumph(u8),
}

/// [`KeySoundStyle::Clack`]'s distinct presses, each also played a little
/// higher and lower.
pub const CLACK_KEYS: u8 = 8;
/// The pitches each clack plays at, in cents.
const CLACK_CENTS: [f64; 3] = [0.0, 45.0, -45.0];
/// The triumph candidates, `triumph_1.wav` onwards.
pub const TRIUMPHS: u8 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    Error,
    DictationStart,
    DictationStop,
    /// A dictation pasted at least [`DONE_MIN_AFTER_STOP`] after its stop
    /// sound, such as after a slow rewrite.
    DictationDone,
    Cancel,
    /// The first-run tour reached its last step.
    OnboardingSuccess,
    /// A first-run onboarding cue (typing clacks, shortcut keycap, motion landings).
    Onboarding(OnboardingTone),
    /// A Scrawl capture started recording.
    #[cfg(hex_capture)]
    CaptureStart,
    /// It stopped and is being written.
    #[cfg(hex_capture)]
    CaptureStop,
    /// Its auto-send went, once Undo ran out.
    #[cfg(hex_capture)]
    CaptureSent,
    /// A send was taken back, or a recording cancelled.
    #[cfg(hex_capture)]
    CaptureUndo,
    #[cfg(hex_capture)]
    CaptureFailed,
}

/// A request to the playback worker. The start tone of a shortcut press is
/// provisional: the press may still become a chord or short tap, which
/// retracts it before or while it sounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Request {
    /// A tone at a percentage of its level.
    Play(Tone, u8),
    Audition(SoundSet, Audition),
    StopAudition,
    ProvisionalStart,
    RetractStart,
    Prepare,
}

static DICTATION_PLAYER: OnceLock<SyncSender<Request>> = OnceLock::new();
static LOADER_STARTED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(0.5_f32.to_bits());

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_volume(volume: f32) {
    VOLUME.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

fn sounds_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) && volume() > 0.0
}

/// How long the output device stays open after the last tone finishes. Holding
/// the default output open keeps macOS from idle-sleeping, so an idle app must
/// release it. A tone implies recent user activity, so a grace shorter than any
/// idle-sleep timer costs nothing while sparing bursts of dictation the cold
/// device reopen.
///
/// On Linux an open output is a listed stream in the desktop's sound controls
/// (PipeWire, PulseAudio), so it closes about a second after the last tone
/// (#720); the start tone still opens the device during its chord grace.
#[cfg(not(target_os = "linux"))]
const IDLE_RELEASE_GRACE: Duration = Duration::from_secs(5 * 60);
#[cfg(target_os = "linux")]
const IDLE_RELEASE_GRACE: Duration = Duration::from_secs(1);
const OPEN_RETRY_BACKOFF: Duration = Duration::from_secs(2);

// Only the playback worker owns a device. It opens lazily for a tone, stays
// open through the idle grace, and drops within one observation interval once
// sounds turn off or the grace expires. Opening never blocks capture or UI.
struct FeedbackOutput<S> {
    sink: Option<S>,
    retry_at: Instant,
    playing_until: Option<Instant>,
    /// The output route generation the device was opened (or last tried)
    /// under. A newer one means the default output changed or disappeared.
    route: u64,
}

impl<S> FeedbackOutput<S> {
    fn new() -> Self {
        Self {
            sink: None,
            retry_at: Instant::now(),
            playing_until: None,
            route: 0,
        }
    }

    /// Opens the device for a tone unless a recent failure is still backing off.
    /// An output opened under an older `route` is dropped and the current
    /// default opens at once, without the failure backoff, so tones follow a
    /// default output that changed or went away instead of feeding a dead one.
    fn open_for_tone(
        &mut self,
        now: Instant,
        route: u64,
        open: impl FnOnce() -> Result<S>,
    ) -> Result<()> {
        if self.route != route {
            self.route = route;
            if self.sink.take().is_some() {
                tracing::debug!("default output changed; reopening feedback output");
            }
            self.retry_at = now;
        }
        if self.sink.is_none() && now >= self.retry_at {
            self.retry_at = now + OPEN_RETRY_BACKOFF;
            self.sink = Some(open()?);
        }
        Ok(())
    }

    fn mark_playing(&mut self, now: Instant, duration: Duration) {
        let until = now + duration;
        self.playing_until = Some(self.playing_until.unwrap_or(until).max(until));
    }

    /// Releases the device when sounds are off or nothing has played within the
    /// idle grace. A released device reopens immediately for the next tone.
    fn release_when_idle(&mut self, enabled: bool, now: Instant) {
        let idle = self
            .playing_until
            .is_none_or(|until| now >= until + IDLE_RELEASE_GRACE);
        if !enabled || idle {
            self.playing_until = None;
            if self.sink.take().is_some() {
                self.retry_at = now;
            }
        }
    }
}

/// Maps the admission wait outcome to an error. A timeout does not stop the
/// loader thread: the audio stack may simply be cold, and the loader keeps
/// running so tones recover once the default output finally opens.
fn admission_error(outcome: Result<Result<()>, mpsc::RecvTimeoutError>) -> Result<()> {
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(eyre!("timed out preloading feedback audio")),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(eyre!("feedback audio loader exited before initializing"))
        }
    }
}

fn publish_player(
    player: &OnceLock<SyncSender<Request>>,
    sender: SyncSender<Request>,
    ready: SyncSender<Result<()>>,
    outcome: Result<()>,
) {
    let _ = player.set(sender);
    let _ = ready.send(outcome);
}

pub fn preload() -> Result<()> {
    if DICTATION_PLAYER.get().is_some() {
        return Ok(());
    }
    if LOADER_STARTED.swap(true, Ordering::AcqRel) {
        // A loader is already running from an earlier admission; it registers
        // the player itself, so report the same slow-audio outcome.
        return Err(eyre!("feedback audio is still initializing"));
    }
    let (sender, receiver) = mpsc::sync_channel::<Request>(32);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let sounds = match Sounds::load() {
            Ok(sounds) => sounds,
            Err(error) => {
                let _ = ready_sender.send(Err(error));
                LOADER_STARTED.store(false, Ordering::Release);
                return;
            }
        };
        let mut output = FeedbackOutput::new();
        // The device opens for the first tone, not at startup, so an idle app
        // holds no output stream. Publish before any tone arrives.
        publish_player(&DICTATION_PLAYER, sender, ready_sender, Ok(()));
        let mut start_tone = ProvisionalStart::default();
        let mut audition = Auditioning::default();
        loop {
            let timeout = start_tone.wait(Instant::now(), OBSERVATION_INTERVAL);
            let request = match receiver.recv_timeout(timeout) {
                Ok(request) => Some(request),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let received_at = Instant::now();
            let timed_out = request.is_none();
            let enabled = sounds_enabled();
            output.release_when_idle(enabled, received_at);
            if !enabled {
                start_tone.retract();
                audition.stop();
                continue;
            }
            let tone = match request {
                Some(Request::Play(tone, percent)) => {
                    if tone == Tone::DictationStart {
                        start_tone.cancel_pending();
                    }
                    Some((tone, percent))
                }
                Some(Request::ProvisionalStart) => {
                    start_tone.request(received_at);
                    // Opening during the chord grace keeps a cold device off
                    // the path to the audible tone.
                    output.prepare(received_at);
                    None
                }
                Some(Request::RetractStart) => {
                    start_tone.retract();
                    None
                }
                Some(Request::Prepare) => {
                    output.prepare(received_at);
                    None
                }
                Some(Request::Audition(set, what)) => {
                    audition.stop();
                    let stopped = audition.begin();
                    for (sound, at, cue) in sounds.audition(set, what) {
                        let (sound, _) = Retractable::with_flag(sound.delay(at), stopped.clone());
                        output.play(sound, (set, cue));
                    }
                    #[cfg(target_os = "macos")]
                    if what == Audition::Cue(Cue::Error) && !set.has_own_error() {
                        audition.system = spawn_system_sound("Basso");
                    }
                    None
                }
                Some(Request::StopAudition) => {
                    audition.stop();
                    None
                }
                None => None,
            };
            if let Some((tone, percent)) = tone {
                if let Some(sound) = sounds.get(sound_set(), tone) {
                    output.play(sound.clone().amplify(f32::from(percent) / 100.0), tone);
                }
            } else if timed_out && start_tone.is_due(Instant::now()) {
                // Only an empty queue reaches here, so a retraction sent while
                // the device opened always wins over the tone.
                let start = sounds.set(sound_set()).start.clone();
                let (sound, retracted) = Retractable::new(start);
                if output.play(sound, Tone::DictationStart) {
                    start_tone.sounding(retracted);
                } else {
                    start_tone.retract();
                }
            }
        }
    });
    let outcome = ready_receiver.recv_timeout(Duration::from_secs(2));
    admission_error(outcome)?;
    Ok(())
}

/// The least time between the stop sound and a paste for the done sound to
/// play. A dictation that pastes at once is already over when stop sounds.
pub const DONE_MIN_AFTER_STOP: Duration = Duration::from_secs(2);

static LAST_STOP: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether a done sound asked for now follows a stop sound by
/// [`DONE_MIN_AFTER_STOP`]; without a stop sound there is nothing to follow.
fn done_is_late(stopped: Option<Instant>, now: Instant) -> bool {
    stopped.is_none_or(|at| now.saturating_duration_since(at) >= DONE_MIN_AFTER_STOP)
}

pub fn play(tone: Tone) {
    play_at(tone, 100);
}

/// [`play`] at `percent` of the tone's own level, such as a keystroke typed
/// softly.
pub fn play_at(tone: Tone, percent: u8) {
    if !sounds_enabled() {
        return;
    }
    match tone {
        Tone::DictationStop => {
            *LAST_STOP.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
        Tone::DictationDone => {
            let stopped = *LAST_STOP.lock().unwrap_or_else(|e| e.into_inner());
            if !done_is_late(stopped, Instant::now()) {
                return;
            }
        }
        _ => {}
    }
    match tone {
        #[cfg(target_os = "macos")]
        Tone::Error if !sound_set().has_own_error() => play_system_sound(tone),
        _ => enqueue(DICTATION_PLAYER.get(), Request::Play(tone, percent)),
    }
}

/// A set of feedback sounds, chosen in Settings. Every set has its own start,
/// stop and cancel, and all are level-matched, so a volume sounds the same in
/// each. `scripts/sounds/synth.py` makes their files.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum SoundSet {
    /// One kalimba-like voice playing one motif across a dictation.
    Signature = 0,
    /// The sounds Hex played before the signature set. It has no done tone,
    /// and errors play macOS Basso as they did.
    #[default]
    Classic = 1,
    Click = 2,
    Soft = 3,
    Wood = 4,
    Analog = 5,
    /// The team vote's winner (2026-10): one mechanical key's thock.
    Thock = 6,
}

impl SoundSet {
    /// In the order Settings lists them.
    pub const ALL: [Self; 7] = [
        Self::Signature,
        Self::Classic,
        Self::Click,
        Self::Soft,
        Self::Wood,
        Self::Analog,
        Self::Thock,
    ];

    /// The English name Settings shows, translated where it is shown.
    pub fn name(self) -> &'static str {
        match self {
            Self::Signature => "Hex signature",
            Self::Classic => "Classic",
            Self::Click => "Click",
            Self::Soft => "Soft tone",
            Self::Wood => "Wood",
            Self::Analog => "Warm synth",
            Self::Thock => "Thock",
        }
    }

    /// What the set sounds like, in a few words, as the sound picker lists it.
    pub fn character(self) -> &'static str {
        match self {
            Self::Signature => "A kalimba motif that rises and comes home",
            Self::Classic => "The tones Hex first shipped with",
            Self::Click => "Mechanical key clicks",
            Self::Soft => "Warm, rounded blips",
            Self::Wood => "Soft marimba and hollow wood blocks",
            Self::Analog => "A mellow analogue synth",
            Self::Thock => "A deep, creamy keyboard key",
        }
    }

    /// Whether the set plays a sound for `cue`. Classic and Thock have no done
    /// sound; Classic's error is macOS Basso, or Signature's elsewhere.
    pub fn has(self, cue: Cue) -> bool {
        cue != Cue::Done || BUNDLED_SETS[self as usize].done.is_some()
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn has_own_error(self) -> bool {
        BUNDLED_SETS[self as usize].error.is_some()
    }

    fn decode(value: u8) -> Self {
        Self::ALL
            .get(usize::from(value))
            .copied()
            .unwrap_or_default()
    }
}

/// A moment of dictation that has its own sound, as the sound picker plays
/// them one at a time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cue {
    Start,
    Stop,
    Done,
    Cancel,
    Error,
}

impl Cue {
    pub const ALL: [Self; 5] = [
        Self::Start,
        Self::Stop,
        Self::Done,
        Self::Cancel,
        Self::Error,
    ];

    /// The English label the sound picker shows, translated where shown.
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Done => "Done",
            Self::Cancel => "Cancel",
            Self::Error => "Error",
        }
    }
}

/// What the sound picker plays of a set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audition {
    /// A short dictation: start, a moment of speaking, stop, a moment of
    /// transcribing, then done.
    Dictation,
    Cue(Cue),
}

/// When a dictation audition plays each sound, from its start.
const DICTATION_CUES: [(Cue, Duration); 3] = [
    (Cue::Start, Duration::ZERO),
    (Cue::Stop, Duration::from_millis(1_400)),
    (Cue::Done, Duration::from_millis(2_100)),
];

impl Audition {
    /// Each cue it plays and when, from its start. A cue the set has no
    /// sound for keeps its moment and plays nothing.
    pub fn cues(self) -> &'static [(Cue, Duration)] {
        match self {
            Self::Dictation => &DICTATION_CUES,
            Self::Cue(Cue::Start) => &[(Cue::Start, Duration::ZERO)],
            Self::Cue(Cue::Stop) => &[(Cue::Stop, Duration::ZERO)],
            Self::Cue(Cue::Done) => &[(Cue::Done, Duration::ZERO)],
            Self::Cue(Cue::Cancel) => &[(Cue::Cancel, Duration::ZERO)],
            Self::Cue(Cue::Error) => &[(Cue::Error, Duration::ZERO)],
        }
    }

    /// How long its moments show, past its last sound.
    pub fn length(self) -> Duration {
        match self {
            Self::Dictation => Duration::from_millis(2_900),
            Self::Cue(_) => Duration::from_millis(900),
        }
    }
}

static SOUND_SET: AtomicU8 = AtomicU8::new(SoundSet::Classic as u8);

pub fn set_sound_set(set: SoundSet) {
    SOUND_SET.store(set as u8, Ordering::Relaxed);
}

fn sound_set() -> SoundSet {
    SoundSet::decode(SOUND_SET.load(Ordering::Relaxed))
}

/// Plays `what` of `set` at the current volume, so the sound picker can be
/// heard. It ends the audition still sounding, if any.
pub fn audition(set: SoundSet, what: Audition) {
    if sounds_enabled() {
        enqueue(DICTATION_PLAYER.get(), Request::Audition(set, what));
    }
}

/// Ends the audition still sounding, fading it out over [`RETRACT_FADE`].
pub fn stop_audition() {
    enqueue(DICTATION_PLAYER.get(), Request::StopAudition);
}

/// The worker's view of the audition sounding, if any.
#[derive(Default)]
struct Auditioning {
    stopped: Option<Arc<AtomicBool>>,
    /// Basso, for Classic's error.
    #[cfg(target_os = "macos")]
    system: Option<std::process::Child>,
}

impl Auditioning {
    /// The flag that stops the audition beginning now.
    fn begin(&mut self) -> Arc<AtomicBool> {
        self.stopped
            .insert(Arc::new(AtomicBool::new(false)))
            .clone()
    }

    fn stop(&mut self) {
        if let Some(stopped) = self.stopped.take() {
            stopped.store(true, Ordering::Release);
        }
        #[cfg(target_os = "macos")]
        if let Some(mut child) = self.system.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// One set's bundled files, in `resources/audio/sounds/<set>/`. A missing
/// done tone stays silent, a missing error (macOS only) plays Basso, and a missing
/// success plays the set's own stop sound.
struct BundledSet {
    start: &'static [u8],
    stop: &'static [u8],
    done: Option<&'static [u8]>,
    cancel: &'static [u8],
    error: Option<&'static [u8]>,
    success: Option<&'static [u8]>,
}

macro_rules! bundled_sound {
    ($set:literal, $name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/audio/sounds/",
            $set,
            "/",
            $name,
            ".wav"
        ))
        .as_slice()
    };
}

/// A set `synth.py` makes whole.
macro_rules! synthesized_set {
    ($set:literal) => {
        BundledSet {
            start: bundled_sound!($set, "start"),
            stop: bundled_sound!($set, "stop"),
            done: Some(bundled_sound!($set, "done")),
            cancel: bundled_sound!($set, "cancel"),
            error: Some(bundled_sound!($set, "error")),
            success: Some(bundled_sound!($set, "success")),
        }
    };
}

/// In [`SoundSet::ALL`]'s order.
const BUNDLED_SETS: [BundledSet; 7] = [
    synthesized_set!("signature"),
    BundledSet {
        start: bundled_sound!("classic", "start"),
        stop: bundled_sound!("classic", "stop"),
        done: None,
        cancel: bundled_sound!("classic", "cancel"),
        // macOS plays Basso; elsewhere Classic borrows Signature's error.
        error: if cfg!(target_os = "macos") {
            None
        } else {
            Some(bundled_sound!("signature", "error"))
        },
        success: None,
    },
    synthesized_set!("click"),
    synthesized_set!("soft"),
    synthesized_set!("wood"),
    synthesized_set!("analog"),
    // No done sound: the stop already says it (Kit, 2026-10-30).
    BundledSet {
        start: bundled_sound!("thock", "start"),
        stop: bundled_sound!("thock", "stop"),
        done: None,
        cancel: bundled_sound!("thock", "cancel"),
        error: Some(bundled_sound!("thock", "error")),
        success: Some(bundled_sound!("thock", "success")),
    },
];

/// One set, decoded.
struct SetSounds {
    start: SamplesBuffer,
    stop: SamplesBuffer,
    done: Option<SamplesBuffer>,
    cancel: SamplesBuffer,
    /// Played for failures; `None` plays macOS Basso.
    error: Option<SamplesBuffer>,
    success: Option<SamplesBuffer>,
}

impl SetSounds {
    fn load(bundled: &BundledSet) -> Result<Self> {
        let optional = |bytes: Option<&'static [u8]>| bytes.map(decode_wav).transpose();
        Ok(Self {
            start: decode_wav(bundled.start)?,
            stop: decode_wav(bundled.stop)?,
            done: optional(bundled.done)?,
            cancel: decode_wav(bundled.cancel)?,
            error: optional(bundled.error)?,
            success: optional(bundled.success)?,
        })
    }
}

/// Decoded onboarding cues (`resources/audio/sounds/onboarding/`).
struct OnboardingSounds {
    clack_keys: Vec<SamplesBuffer>,
    clack_space: SamplesBuffer,
    clack_backspace: SamplesBuffer,
    felt_keys: [SamplesBuffer; 6],
    felt_space: SamplesBuffer,
    felt_backspace: SamplesBuffer,
    taptic_keys: [SamplesBuffer; 6],
    taptic_space: SamplesBuffer,
    soft_keys: [SamplesBuffer; 6],
    soft_space: SamplesBuffer,
    shortcut_down: SamplesBuffer,
    shortcut_up: SamplesBuffer,
    hexagon: SamplesBuffer,
    panel_escape: SamplesBuffer,
    permission_tick: SamplesBuffer,
    almost_ready: SamplesBuffer,
    build: [SamplesBuffer; 3],
    chat_incoming: SamplesBuffer,
    chat_sent: SamplesBuffer,
    chat_read: SamplesBuffer,
    chat_rewind: SamplesBuffer,
    chat_tapbacks: [SamplesBuffer; 6],
    heartbeat: SamplesBuffer,
    heartbeat_slow: SamplesBuffer,
    slow_down: SamplesBuffer,
    slam: SamplesBuffer,
    confetti: SamplesBuffer,
    fireworks: SamplesBuffer,
    triumphs: [SamplesBuffer; TRIUMPHS as usize],
}

impl OnboardingSounds {
    fn load() -> Result<Self> {
        let tapback = decode_wav(bundled_sound!("onboarding", "chat_tapback"))?;
        let presses = [
            decode_wav(bundled_sound!("onboarding", "clack_1"))?,
            decode_wav(bundled_sound!("onboarding", "clack_2"))?,
            decode_wav(bundled_sound!("onboarding", "clack_3"))?,
            decode_wav(bundled_sound!("onboarding", "clack_4"))?,
            decode_wav(bundled_sound!("onboarding", "clack_5"))?,
            decode_wav(bundled_sound!("onboarding", "clack_6"))?,
            decode_wav(bundled_sound!("onboarding", "clack_7"))?,
            decode_wav(bundled_sound!("onboarding", "clack_8"))?,
        ];
        Ok(Self {
            clack_keys: CLACK_CENTS
                .iter()
                .flat_map(|&cents| presses.iter().map(move |press| detune(press, cents)))
                .collect(),
            clack_space: decode_wav(bundled_sound!("onboarding", "clack_space"))?,
            clack_backspace: decode_wav(bundled_sound!("onboarding", "clack_backspace"))?,
            felt_keys: [
                decode_wav(bundled_sound!("onboarding", "key_1"))?,
                decode_wav(bundled_sound!("onboarding", "key_2"))?,
                decode_wav(bundled_sound!("onboarding", "key_3"))?,
                decode_wav(bundled_sound!("onboarding", "key_4"))?,
                decode_wav(bundled_sound!("onboarding", "key_5"))?,
                decode_wav(bundled_sound!("onboarding", "key_6"))?,
            ],
            felt_space: decode_wav(bundled_sound!("onboarding", "key_space"))?,
            felt_backspace: decode_wav(bundled_sound!("onboarding", "key_backspace"))?,
            taptic_keys: [
                decode_wav(bundled_sound!("onboarding", "taptic_1"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_2"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_3"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_4"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_5"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_6"))?,
            ],
            taptic_space: decode_wav(bundled_sound!("onboarding", "taptic_space"))?,
            soft_keys: [
                decode_wav(bundled_sound!("onboarding", "soft_key_1"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_2"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_3"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_4"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_5"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_6"))?,
            ],
            soft_space: decode_wav(bundled_sound!("onboarding", "soft_key_space"))?,
            shortcut_down: decode_wav(bundled_sound!("onboarding", "shortcut_down"))?,
            shortcut_up: decode_wav(bundled_sound!("onboarding", "shortcut_up"))?,
            hexagon: decode_wav(bundled_sound!("onboarding", "hexagon"))?,
            panel_escape: decode_wav(bundled_sound!("onboarding", "panel_escape"))?,
            permission_tick: decode_wav(bundled_sound!("onboarding", "permission_tick"))?,
            almost_ready: decode_wav(bundled_sound!("onboarding", "almost_ready"))?,
            build: [
                decode_wav(bundled_sound!("onboarding", "build_1"))?,
                decode_wav(bundled_sound!("onboarding", "build_2"))?,
                decode_wav(bundled_sound!("onboarding", "build_3"))?,
            ],
            chat_incoming: imessage::or_bundled(
                imessage::RECEIVED,
                &decode_wav(bundled_sound!("onboarding", "chat_incoming"))?,
            ),
            chat_sent: imessage::or_bundled(
                imessage::SENT,
                &decode_wav(bundled_sound!("onboarding", "chat_sent"))?,
            ),
            chat_read: decode_wav(bundled_sound!("onboarding", "chat_read"))?,
            chat_rewind: decode_wav(bundled_sound!("onboarding", "chat_rewind"))?,
            chat_tapbacks: Tapback::ALL
                .map(|kind| imessage::or_bundled(imessage::tapback(kind), &tapback)),
            heartbeat: decode_wav(bundled_sound!("onboarding", "heartbeat"))?,
            heartbeat_slow: decode_wav(bundled_sound!("onboarding", "heartbeat_slow"))?,
            slow_down: decode_wav(bundled_sound!("onboarding", "slow_down"))?,
            slam: decode_wav(bundled_sound!("onboarding", "slam"))?,
            confetti: decode_wav(bundled_sound!("onboarding", "confetti"))?,
            fireworks: decode_wav(bundled_sound!("onboarding", "fireworks"))?,
            triumphs: [
                decode_wav(bundled_sound!("onboarding", "triumph_1"))?,
                decode_wav(bundled_sound!("onboarding", "triumph_2"))?,
                decode_wav(bundled_sound!("onboarding", "triumph_3"))?,
            ],
        })
    }

    fn get(&self, cue: OnboardingTone) -> Option<&SamplesBuffer> {
        Some(match cue {
            OnboardingTone::Key(KeySoundStyle::Silent, _)
            | OnboardingTone::KeySpace(KeySoundStyle::Silent)
            | OnboardingTone::KeyBackspace(KeySoundStyle::Silent) => return None,
            OnboardingTone::Key(KeySoundStyle::Clack, i) => {
                &self.clack_keys[usize::from(i) % self.clack_keys.len()]
            }
            OnboardingTone::KeySpace(KeySoundStyle::Clack) => &self.clack_space,
            OnboardingTone::KeyBackspace(KeySoundStyle::Clack) => &self.clack_backspace,
            OnboardingTone::Key(KeySoundStyle::Felt, i) => {
                &self.felt_keys[usize::from(i) % self.felt_keys.len()]
            }
            OnboardingTone::Key(KeySoundStyle::Taptic, i) => {
                &self.taptic_keys[usize::from(i) % self.taptic_keys.len()]
            }
            OnboardingTone::Key(KeySoundStyle::Soft, i) => {
                &self.soft_keys[usize::from(i) % self.soft_keys.len()]
            }
            OnboardingTone::KeySpace(KeySoundStyle::Felt) => &self.felt_space,
            OnboardingTone::KeySpace(KeySoundStyle::Taptic) => &self.taptic_space,
            OnboardingTone::KeySpace(KeySoundStyle::Soft) => &self.soft_space,
            OnboardingTone::KeyBackspace(KeySoundStyle::Felt) => &self.felt_backspace,
            OnboardingTone::KeyBackspace(KeySoundStyle::Taptic) => &self.taptic_keys[3],
            OnboardingTone::KeyBackspace(KeySoundStyle::Soft) => &self.soft_keys[3],
            OnboardingTone::ShortcutDown => &self.shortcut_down,
            OnboardingTone::ShortcutUp => &self.shortcut_up,
            OnboardingTone::Hexagon => &self.hexagon,
            OnboardingTone::PanelEscape => &self.panel_escape,
            OnboardingTone::PermissionTick => &self.permission_tick,
            OnboardingTone::AlmostReady => &self.almost_ready,
            OnboardingTone::Build(i) => &self.build[usize::from(i).min(2)],
            OnboardingTone::Incoming => &self.chat_incoming,
            OnboardingTone::Sent => &self.chat_sent,
            OnboardingTone::Read => &self.chat_read,
            OnboardingTone::Rewind => &self.chat_rewind,
            OnboardingTone::Tapback(kind) => &self.chat_tapbacks[kind as usize],
            OnboardingTone::Heartbeat => &self.heartbeat,
            OnboardingTone::HeartbeatSlow => &self.heartbeat_slow,
            OnboardingTone::SlowDown => &self.slow_down,
            OnboardingTone::Slam => &self.slam,
            OnboardingTone::Confetti => &self.confetti,
            OnboardingTone::Fireworks => &self.fireworks,
            OnboardingTone::Triumph(i) => &self.triumphs[usize::from(i) % self.triumphs.len()],
        })
    }
}

/// The decoded sounds the playback worker plays, every set at once so a new
/// choice plays at once.
struct Sounds {
    /// In [`SoundSet::ALL`]'s order.
    sets: Vec<SetSounds>,
    onboarding: OnboardingSounds,
    #[cfg(hex_capture)]
    capture: [SamplesBuffer; 5],
}

impl Sounds {
    fn load() -> Result<Self> {
        Ok(Self {
            sets: BUNDLED_SETS
                .iter()
                .map(SetSounds::load)
                .collect::<Result<_>>()?,
            onboarding: OnboardingSounds::load()?,
            #[cfg(hex_capture)]
            capture: capture_sounds()?,
        })
    }

    fn set(&self, set: SoundSet) -> &SetSounds {
        &self.sets[set as usize]
    }

    /// The sounds `what` plays of `set`, each with when it starts. Classic's
    /// error is macOS Basso, which the worker plays itself.
    fn audition(&self, set: SoundSet, what: Audition) -> Vec<(SamplesBuffer, Duration, Cue)> {
        let sounds = self.set(set);
        what.cues()
            .iter()
            .filter_map(|&(cue, at)| {
                let sound = match cue {
                    Cue::Start => Some(&sounds.start),
                    Cue::Stop => Some(&sounds.stop),
                    Cue::Done => sounds.done.as_ref(),
                    Cue::Cancel => Some(&sounds.cancel),
                    Cue::Error => sounds.error.as_ref(),
                };
                sound.map(|sound| (sound.clone(), at, cue))
            })
            .collect()
    }

    /// `tone` in `set`, or `None` when it plays nothing (or a system sound).
    fn get(&self, set: SoundSet, tone: Tone) -> Option<&SamplesBuffer> {
        let sounds = self.set(set);
        Some(match tone {
            Tone::DictationStart => &sounds.start,
            Tone::DictationStop => &sounds.stop,
            Tone::DictationDone => return sounds.done.as_ref(),
            Tone::Cancel => &sounds.cancel,
            Tone::OnboardingSuccess => {
                return sounds.success.as_ref().or(Some(&sounds.stop));
            }
            Tone::Onboarding(cue) => return self.onboarding.get(cue),
            Tone::Error => return sounds.error.as_ref(),
            #[cfg(hex_capture)]
            Tone::CaptureStart => &self.capture[0],
            #[cfg(hex_capture)]
            Tone::CaptureStop => &self.capture[1],
            #[cfg(hex_capture)]
            Tone::CaptureSent => &self.capture[2],
            #[cfg(hex_capture)]
            Tone::CaptureUndo => &self.capture[3],
            #[cfg(hex_capture)]
            Tone::CaptureFailed => &self.capture[4],
        })
    }
}

/// The capture tones, in [`Tone`]'s order: start, stop, sent, undo, failed.
#[cfg(hex_capture)]
fn capture_sounds() -> Result<[SamplesBuffer; 5]> {
    macro_rules! sound {
        ($name:literal) => {
            decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/",
                $name
            )))?
        };
    }
    Ok([
        sound!("captureStart.mp3"),
        sound!("captureStop.mp3"),
        sound!("captureSent.mp3"),
        sound!("captureUndo.mp3"),
        sound!("captureFailed.mp3"),
    ])
}

/// Plays the start tone for a shortcut press that may still become a chord or
/// short tap. It sounds after [`CHORD_GRACE`] unless [`retract_start`] arrives
/// first, so modifiers pressed together for another shortcut stay silent.
pub fn play_provisional_start() {
    if sounds_enabled() {
        enqueue(DICTATION_PLAYER.get(), Request::ProvisionalStart);
    }
}

/// Withdraws a provisional start tone: drops it before it sounds, or fades it
/// out over [`RETRACT_FADE`]. Has no effect on a tone that already finished.
pub fn retract_start() {
    enqueue(DICTATION_PLAYER.get(), Request::RetractStart);
}

/// Opens the output ahead of a tone expected soon, without playing anything.
pub fn prepare() {
    if sounds_enabled() {
        enqueue(DICTATION_PLAYER.get(), Request::Prepare);
    }
}

fn enqueue(player: Option<&SyncSender<Request>>, request: Request) {
    if let Some(player) = player {
        let _ = player.try_send(request);
    }
}

/// How long a provisional start tone waits before sounding. Modifiers pressed
/// together for another application's shortcut arrive within this window, so
/// those chords stay silent while a deliberate press still sounds at once.
pub(crate) const CHORD_GRACE: Duration = Duration::from_millis(40);
/// How quickly a retracted start tone fades out instead of clicking off.
const RETRACT_FADE: Duration = Duration::from_millis(20);
const OBSERVATION_INTERVAL: Duration = Duration::from_millis(250);
/// The output buffer, about 10 ms at 48 kHz. Rodio's default 50 ms buffer adds
/// that much again to every tone.
const OUTPUT_BUFFER_FRAMES: u32 = 512;

/// The worker's view of the provisional start tone: due while waiting out the
/// chord grace, then retractable while it sounds.
#[derive(Default)]
struct ProvisionalStart {
    due: Option<Instant>,
    sounding: Option<Arc<AtomicBool>>,
}

impl ProvisionalStart {
    fn request(&mut self, at: Instant) {
        self.retract();
        self.due = Some(at + CHORD_GRACE);
    }

    /// How long the worker may block before the pending tone is due.
    fn wait(&self, now: Instant, observation: Duration) -> Duration {
        self.due
            .map_or(observation, |due| due.saturating_duration_since(now))
            .min(observation)
    }

    fn is_due(&self, now: Instant) -> bool {
        self.due.is_some_and(|due| now >= due)
    }

    fn sounding(&mut self, retracted: Arc<AtomicBool>) {
        self.due = None;
        self.sounding = Some(retracted);
    }

    fn cancel_pending(&mut self) {
        self.due = None;
    }

    fn retract(&mut self) {
        self.due = None;
        if let Some(retracted) = self.sounding.take() {
            retracted.store(true, Ordering::Release);
        }
    }
}

/// A sound that fades out over [`RETRACT_FADE`] once its flag is set.
struct Retractable<S> {
    inner: S,
    retracted: Arc<AtomicBool>,
    gain: f32,
    step: f32,
    channel: u16,
}

impl<S: Source> Retractable<S> {
    fn new(inner: S) -> (Self, Arc<AtomicBool>) {
        Self::with_flag(inner, Arc::new(AtomicBool::new(false)))
    }

    /// Fades out once `retracted` is set, with every other sound sharing it.
    fn with_flag(inner: S, retracted: Arc<AtomicBool>) -> (Self, Arc<AtomicBool>) {
        let fade_frames = RETRACT_FADE.as_secs_f32() * inner.sample_rate().get() as f32;
        let sound = Self {
            inner,
            retracted: retracted.clone(),
            gain: 1.0,
            step: 1.0 / fade_frames.max(1.0),
            channel: 0,
        };
        (sound, retracted)
    }
}

impl<S: Source> Iterator for Retractable<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let channels = self.inner.channels().get();
        if self.channel == 0 && self.retracted.load(Ordering::Acquire) {
            if self.gain <= 0.0 {
                return None;
            }
            self.gain = (self.gain - self.step).max(0.0);
        }
        self.channel = (self.channel + 1) % channels;
        self.inner.next().map(|sample| sample * self.gain)
    }
}

impl<S: Source> Source for Retractable<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

fn open_output() -> Result<MixerDeviceSink> {
    let mut sink = DeviceSinkBuilder::from_default_device()
        .and_then(|builder| {
            builder
                .with_buffer_size(BufferSize::Fixed(OUTPUT_BUFFER_FRAMES))
                .open_stream()
        })
        .or_else(|_| DeviceSinkBuilder::open_default_sink())
        .wrap_err("could not open the audio output")?;
    sink.log_on_drop(false);
    Ok(sink)
}

impl FeedbackOutput<MixerDeviceSink> {
    /// Opens the output and starts its idle grace without playing anything.
    fn prepare(&mut self, now: Instant) {
        let opening = Instant::now();
        let cold = self.sink.is_none();
        let route = crate::audio::output_route_generation();
        if let Err(error) = self.open_for_tone(now, route, open_output) {
            tracing::warn!(%error, "recording audio output unavailable; retrying");
        }
        if cold && self.sink.is_some() {
            tracing::debug!(
                open_ms = opening.elapsed().as_millis(),
                "opened feedback output"
            );
        }
        self.mark_playing(Instant::now(), Duration::ZERO);
    }

    /// Plays a sound, opening the output first if needed. Returns whether it
    /// reached the mixer.
    fn play(&mut self, sound: impl Source + Send + 'static, tone: impl std::fmt::Debug) -> bool {
        self.prepare(Instant::now());
        // Opening may have been slow; recheck the latest preference before
        // retaining the output or playing the queued sound.
        if !sounds_enabled() {
            self.release_when_idle(false, Instant::now());
            return false;
        }
        let Some(sink) = self.sink.as_ref() else {
            return false;
        };
        let duration = sound.total_duration().unwrap_or(Duration::ZERO);
        sink.mixer().add(sound.amplify(volume()));
        tracing::debug!(?tone, "playing feedback tone");
        self.mark_playing(Instant::now(), duration);
        true
    }
}

#[cfg(target_os = "macos")]
fn play_system_sound(tone: Tone) {
    let sound = match tone {
        Tone::Error => "Basso",
        _ => return,
    };
    if let Some(mut child) = spawn_system_sound(sound) {
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Starts `/System/Library/Sounds/<sound>.aiff` at the current volume. The
/// caller waits for it.
#[cfg(target_os = "macos")]
fn spawn_system_sound(sound: &str) -> Option<std::process::Child> {
    let volume = volume().to_string();
    Command::new("/usr/bin/afplay")
        .args([
            "-v",
            &volume,
            &format!("/System/Library/Sounds/{sound}.aiff"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Mixes a timed `(at_secs, Tone, percent)` timeline offline into a 48 kHz 16-bit mono
/// WAV file at `path` using the exact bundled `Sounds` without opening any
/// audio output device. Used by headless `--video` renders and tests.
pub(crate) fn mix_offline_wav(
    path: &std::path::Path,
    cues: &[(f64, Tone, u8)],
    set: SoundSet,
    gain: f32,
    duration_secs: f64,
) -> Result<()> {
    const OUT_SR: u32 = 48_000;
    let total_samples = ((duration_secs.max(0.1) * f64::from(OUT_SR)).round() as usize).max(1);
    let mut mix = vec![0.0_f32; total_samples];
    let gain = gain.clamp(0.0, 1.0);
    if gain > 0.0 && !cues.is_empty() {
        let sounds = Sounds::load()?;
        for &(at, tone, percent) in cues {
            if at < 0.0 {
                continue;
            }
            let gain = gain * f32::from(percent) / 100.0;
            let Some(buf) = sounds.get(set, tone) else {
                continue;
            };
            let src_channels = usize::from(buf.channels().get());
            let src_rate = buf.sample_rate().get();
            let raw: Vec<f32> = buf.clone().collect();
            let frames = raw.len() / src_channels.max(1);
            let start_idx = (at * f64::from(OUT_SR)).round() as usize;
            if start_idx >= total_samples {
                continue;
            }
            if src_rate == OUT_SR {
                for f in 0..frames {
                    let dst = start_idx + f;
                    if dst >= total_samples {
                        break;
                    }
                    let mut mono = 0.0_f32;
                    for ch in 0..src_channels {
                        mono += raw[f * src_channels + ch];
                    }
                    mix[dst] += (mono / src_channels as f32) * gain;
                }
            } else {
                let out_frames = ((frames as f64 * f64::from(OUT_SR) / f64::from(src_rate)).round()
                    as usize)
                    .max(1);
                for of in 0..out_frames {
                    let dst = start_idx + of;
                    if dst >= total_samples {
                        break;
                    }
                    let sf = (of as f64 * f64::from(src_rate) / f64::from(OUT_SR))
                        .min((frames.saturating_sub(1)) as f64);
                    let i0 = sf.floor() as usize;
                    let i1 = (i0 + 1).min(frames.saturating_sub(1));
                    let frac = (sf - i0 as f64) as f32;
                    let sample_at = |idx: usize| -> f32 {
                        (0..src_channels)
                            .map(|ch| raw[idx * src_channels + ch])
                            .sum::<f32>()
                            / src_channels as f32
                    };
                    let val = sample_at(i0) * (1.0 - frac) + sample_at(i1) * frac;
                    mix[dst] += val * gain;
                }
            }
        }
    }

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: OUT_SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .wrap_err_with(|| format!("could not create {}", path.display()))?;
    for sample in mix {
        // Gentle tanh knee above 0.85 so overlapping tones never hard-clip.
        let a = sample.abs();
        let soft = if a <= 0.85 {
            sample
        } else {
            sample.signum() * (0.85 + 0.14 * ((a - 0.85) / 0.14).tanh())
        };
        let pcm = (soft.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        writer
            .write_sample(pcm)
            .wrap_err("could not write WAV sample")?;
    }
    writer.finalize().wrap_err("could not finalize WAV")?;
    Ok(())
}

/// Decodes one of Hex's bundled 16-bit PCM sounds.
/// `sound` played `cents` higher: the same samples at a faster rate.
fn detune(sound: &SamplesBuffer, cents: f64) -> SamplesBuffer {
    let rate = f64::from(sound.sample_rate().get()) * 2f64.powf(cents / 1200.0);
    let rate = std::num::NonZero::new(rate.round() as u32).unwrap_or(sound.sample_rate());
    SamplesBuffer::new(sound.channels(), rate, sound.clone().collect::<Vec<_>>())
}

fn decode_wav(bytes: &'static [u8]) -> Result<SamplesBuffer> {
    let reader =
        hound::WavReader::new(Cursor::new(bytes)).wrap_err("could not decode feedback audio")?;
    let spec = reader.spec();
    let channels = std::num::NonZero::new(spec.channels)
        .ok_or_else(|| eyre!("feedback audio has no channels"))?;
    let sample_rate = std::num::NonZero::new(spec.sample_rate)
        .ok_or_else(|| eyre!("feedback audio has no sample rate"))?;
    let samples = reader
        .into_samples::<i16>()
        .map(|sample| sample.map(|sample| f32::from(sample) / f32::from(i16::MAX)))
        .collect::<Result<Vec<_>, _>>()
        .wrap_err("could not decode feedback audio")?;
    Ok(SamplesBuffer::new(channels, sample_rate, samples))
}

#[cfg(hex_capture)]
fn decode(bytes: &'static [u8]) -> Result<SamplesBuffer> {
    let decoder =
        rodio::Decoder::new(Cursor::new(bytes)).wrap_err("could not decode feedback audio")?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    Ok(SamplesBuffer::new(
        channels,
        sample_rate,
        decoder.collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<S> FeedbackOutput<S> {
        fn is_open(&self) -> bool {
            self.sink.is_some()
        }
    }

    /// Every bundled sound of `set` with the longest it may run. The
    /// classic sounds keep their original lengths.
    fn bundled(set: SoundSet) -> Vec<(&'static str, &'static [u8], Duration)> {
        let bundled = &BUNDLED_SETS[set as usize];
        let classic = set == SoundSet::Classic;
        let budget = |synthesized: u64, classic_ms: u64| {
            Duration::from_millis(if classic { classic_ms } else { synthesized })
        };
        let mut sounds = vec![
            ("start", bundled.start, budget(130, 300)),
            ("stop", bundled.stop, budget(250, 450)),
            ("cancel", bundled.cancel, budget(250, 2_100)),
        ];
        let optional = [
            ("done", bundled.done, budget(350, 0)),
            ("error", bundled.error, budget(300, 300)),
            ("onboarding success", bundled.success, budget(1_200, 0)),
        ];
        for (name, bytes, budget) in optional {
            if let Some(bytes) = bytes {
                sounds.push((name, bytes, budget));
            }
        }
        sounds
    }

    fn leading_silence(sound: SamplesBuffer) -> Duration {
        let channels = usize::from(sound.channels().get());
        let rate = sound.sample_rate().get();
        let samples = sound.collect::<Vec<f32>>();
        let first = samples
            .iter()
            .position(|sample| sample.abs() > 0.01)
            .unwrap_or(samples.len());
        Duration::from_secs_f64((first / channels) as f64 / f64::from(rate))
    }

    /// Every start sounds at once. The classic stop and cancel keep their
    /// original soft onsets.
    #[test]
    fn bundled_sounds_are_short_and_sound_immediately() {
        for set in SoundSet::ALL {
            for (name, bytes, budget) in bundled(set) {
                let sound = decode_wav(bytes).unwrap();
                let length = sound.total_duration().unwrap();
                assert!(
                    length <= budget,
                    "{set:?} {name} runs {length:?}, over {budget:?}"
                );
                if set == SoundSet::Classic && name != "start" {
                    continue;
                }
                let silence = leading_silence(sound);
                assert!(
                    silence <= Duration::from_millis(1),
                    "{set:?} {name} starts after {silence:?} of silence"
                );
            }
        }
    }

    #[test]
    fn every_set_but_classic_has_every_sound() {
        for set in SoundSet::ALL {
            let bundled = &BUNDLED_SETS[set as usize];
            let own = bundled.error.is_some() && bundled.success.is_some();
            assert_eq!(own, set != SoundSet::Classic, "{set:?}");
            let done = ![SoundSet::Classic, SoundSet::Thock].contains(&set);
            assert_eq!(bundled.done.is_some(), done, "{set:?}");
        }
    }

    #[test]
    fn onboarding_cues_decode_stay_compact_and_respect_zero_volume_mix() {
        let sounds = Sounds::load().unwrap();
        for (style, keys, longest) in [
            (KeySoundStyle::Clack, CLACK_KEYS * 3, 100),
            (KeySoundStyle::Taptic, 6, 60),
            (KeySoundStyle::Felt, 6, 60),
            (KeySoundStyle::Soft, 6, 60),
        ] {
            for i in 0..keys {
                let buf = sounds
                    .get(
                        SoundSet::Signature,
                        Tone::Onboarding(OnboardingTone::Key(style, i)),
                    )
                    .expect("key cue present");
                assert!(buf.total_duration().unwrap() <= Duration::from_millis(longest));
            }
            assert!(
                sounds
                    .get(
                        SoundSet::Signature,
                        Tone::Onboarding(OnboardingTone::KeySpace(style))
                    )
                    .is_some()
            );
        }
        assert!(
            sounds
                .get(
                    SoundSet::Signature,
                    Tone::Onboarding(OnboardingTone::Key(KeySoundStyle::Silent, 0))
                )
                .is_none(),
            "Silent typing plays no key clacks"
        );
        for cue in [
            OnboardingTone::ShortcutDown,
            OnboardingTone::ShortcutUp,
            OnboardingTone::Hexagon,
            OnboardingTone::PanelEscape,
            OnboardingTone::PermissionTick,
            OnboardingTone::AlmostReady,
            OnboardingTone::Build(0),
            OnboardingTone::Build(1),
            OnboardingTone::Read,
            OnboardingTone::Rewind,
            OnboardingTone::Heartbeat,
        ] {
            let buf = sounds
                .get(SoundSet::Signature, Tone::Onboarding(cue))
                .expect("onboarding cue present");
            assert!(buf.total_duration().unwrap() <= Duration::from_millis(400));
        }
        // The slow-motion and comic-hit cues, and the Messages sounds, ring
        // longer, but not for long.
        let tapbacks = Tapback::ALL.map(OnboardingTone::Tapback);
        for cue in [
            OnboardingTone::Incoming,
            OnboardingTone::Sent,
            OnboardingTone::HeartbeatSlow,
            OnboardingTone::SlowDown,
            OnboardingTone::Slam,
        ]
        .iter()
        .chain(&tapbacks)
        {
            let buf = sounds
                .get(SoundSet::Signature, Tone::Onboarding(*cue))
                .expect("onboarding cue present");
            assert!(buf.total_duration().unwrap() <= Duration::from_millis(1200));
        }
        // The celebration rings longest of all.
        let triumphs = (0..TRIUMPHS).map(OnboardingTone::Triumph);
        for cue in [OnboardingTone::Confetti, OnboardingTone::Fireworks]
            .into_iter()
            .chain(triumphs)
        {
            let buf = sounds
                .get(SoundSet::Signature, Tone::Onboarding(cue))
                .expect("onboarding cue present");
            assert!(buf.total_duration().unwrap() <= Duration::from_millis(1600));
        }
        let land = sounds
            .get(
                SoundSet::Signature,
                Tone::Onboarding(OnboardingTone::Build(2)),
            )
            .expect("the logo's last note");
        assert!(land.total_duration().unwrap() <= Duration::from_millis(500));

        let k0: Vec<f32> = sounds.onboarding.felt_keys[0].clone().collect();
        let k1: Vec<f32> = sounds.onboarding.felt_keys[1].clone().collect();
        assert_ne!(k0, k1, "adjacent key clacks must differ");

        // Offline mix with gain 0.0 writes pure silence; gain 0.5 writes audible cues.
        let dir = std::env::temp_dir().join(format!(
            "hex-sfx-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let silent = dir.join("silent.wav");
        let audible = dir.join("audible.wav");
        let timeline = [
            (
                0.05,
                Tone::Onboarding(OnboardingTone::Key(KeySoundStyle::Taptic, 0)),
                100,
            ),
            (0.20, Tone::DictationStart, 100),
        ];
        mix_offline_wav(&silent, &timeline, SoundSet::Signature, 0.0, 0.5).unwrap();
        mix_offline_wav(&audible, &timeline, SoundSet::Signature, 0.5, 0.5).unwrap();
        let max_abs = |p: &std::path::Path| -> i16 {
            hound::WavReader::open(p)
                .unwrap()
                .into_samples::<i16>()
                .map(|s| s.unwrap().saturating_abs())
                .max()
                .unwrap_or(0)
        };
        assert_eq!(max_abs(&silent), 0, "volume 0 means silent");
        assert!(max_abs(&audible) > 400, "non-zero volume mixes cues");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sound_set_round_trips_by_name_and_index() {
        for set in SoundSet::ALL {
            assert_eq!(SoundSet::decode(set as u8), set);
            let json = serde_json::to_string(&set).unwrap();
            assert_eq!(serde_json::from_str::<SoundSet>(&json).unwrap(), set);
        }
        assert_eq!(SoundSet::decode(u8::MAX), SoundSet::Classic);
        assert_eq!(
            serde_json::to_string(&SoundSet::Analog).unwrap(),
            "\"analog\""
        );
    }

    #[test]
    fn provisional_start_sounds_after_the_chord_grace_unless_retracted() {
        let now = Instant::now();
        let observation = Duration::from_millis(250);
        let mut start = ProvisionalStart::default();
        assert_eq!(start.wait(now, observation), observation);
        assert!(!start.is_due(now + Duration::from_secs(1)));

        // A press waits out the grace, then is due.
        start.request(now);
        assert_eq!(start.wait(now, observation), CHORD_GRACE);
        assert!(!start.is_due(now + CHORD_GRACE - Duration::from_millis(1)));
        assert!(start.is_due(now + CHORD_GRACE));
        assert_eq!(
            start.wait(now + CHORD_GRACE * 2, observation),
            Duration::ZERO
        );

        // A chord inside the grace never sounds.
        start.retract();
        assert!(!start.is_due(now + CHORD_GRACE));
        assert_eq!(start.wait(now, observation), observation);

        // A chord after the tone began fades that tone, and only once.
        start.request(now);
        let (_, sounding) = Retractable::new(SamplesBuffer::new(
            std::num::NonZero::new(1).unwrap(),
            std::num::NonZero::new(48_000).unwrap(),
            vec![1.0; 10],
        ));
        start.sounding(sounding.clone());
        assert!(!start.is_due(now + CHORD_GRACE));
        start.retract();
        assert!(sounding.load(Ordering::Acquire));

        // A later press supersedes an earlier pending one; a definitive start
        // tone cancels the provisional one instead of doubling it.
        start.request(now);
        start.request(now + Duration::from_millis(10));
        assert!(!start.is_due(now + CHORD_GRACE));
        start.cancel_pending();
        assert!(!start.is_due(now + Duration::from_secs(1)));
    }

    #[test]
    fn retracted_tone_fades_out_without_a_click_or_channel_skew() {
        let rate = 48_000_u32;
        let frames = rate as usize;
        let buffer = || {
            SamplesBuffer::new(
                std::num::NonZero::new(2).unwrap(),
                std::num::NonZero::new(rate).unwrap(),
                vec![1.0; frames * 2],
            )
        };
        let (untouched, _) = Retractable::new(buffer());
        assert_eq!(untouched.count(), frames * 2);

        let (mut sound, retracted) = Retractable::new(buffer());
        for _ in 0..101 {
            assert_eq!(sound.next(), Some(1.0));
        }
        // Retracting mid-frame finishes that frame at full gain first.
        retracted.store(true, Ordering::Release);
        assert_eq!(sound.next(), Some(1.0));
        let rest = sound.collect::<Vec<_>>();
        let fade_frames = (RETRACT_FADE.as_secs_f32() * rate as f32) as usize;
        assert_eq!(rest.len() % 2, 0, "the fade ends on a frame boundary");
        assert!(rest.len().abs_diff(fade_frames * 2) <= 4, "{}", rest.len());
        for frame in rest.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
        assert!(rest.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(rest[0] < 1.0 && *rest.last().unwrap() < 0.01);
    }

    #[test]
    fn timed_out_admission_still_publishes_a_usable_player() {
        let player = OnceLock::new();
        let (sender, receiver) = mpsc::sync_channel(8);
        let (ready, waiting) = mpsc::sync_channel(1);
        assert!(admission_error(waiting.recv_timeout(Duration::ZERO)).is_err());
        drop(waiting);
        publish_player(&player, sender, ready, Ok(()));
        enqueue(player.get(), Request::Play(Tone::DictationStart, 100));
        assert_eq!(
            receiver.try_recv().unwrap(),
            Request::Play(Tone::DictationStart, 100)
        );
    }

    #[test]
    fn output_opens_for_a_tone_and_releases_after_the_idle_grace() {
        use std::cell::Cell;
        struct Sink<'a>(&'a Cell<usize>);
        impl Drop for Sink<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }
        let live = Cell::new(0);
        let opens = Cell::new(0);
        let open = || {
            live.set(live.get() + 1);
            opens.set(opens.get() + 1);
            Ok(Sink(&live))
        };
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        let tone = Duration::from_secs(1);

        // Idle observation without a tone never opens the device.
        output.release_when_idle(true, now);
        assert_eq!(opens.get(), 0);

        output.open_for_tone(now, 0, open).unwrap();
        output.mark_playing(now, tone);
        output.open_for_tone(now, 0, open).unwrap();
        assert_eq!((opens.get(), live.get()), (1, 1));

        // Retained while playing and through the grace, released afterwards.
        output.release_when_idle(true, now + tone);
        output.release_when_idle(true, now + tone + IDLE_RELEASE_GRACE / 2);
        assert!(output.is_open());
        output.release_when_idle(true, now + tone + IDLE_RELEASE_GRACE);
        assert!(!output.is_open());
        assert_eq!(live.get(), 0);

        // The next tone reopens immediately, without a failure backoff.
        let later = now + tone + IDLE_RELEASE_GRACE;
        output.open_for_tone(later, 0, open).unwrap();
        assert_eq!((opens.get(), live.get()), (2, 1));

        // Turning sounds off releases even while a tone is nominally playing.
        output.mark_playing(later, tone);
        output.release_when_idle(false, later);
        assert!(!output.is_open());
        drop(output);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn overlapping_tones_extend_playback_instead_of_truncating_it() {
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        output.open_for_tone(now, 0, || Ok(())).unwrap();
        output.mark_playing(now, Duration::from_secs(2));
        output.mark_playing(now + Duration::from_millis(300), Duration::from_secs(1));
        assert_eq!(output.playing_until, Some(now + Duration::from_secs(2)));

        output.release_when_idle(true, now + Duration::from_secs(1) + IDLE_RELEASE_GRACE);
        assert!(output.is_open());
        output.release_when_idle(true, now + Duration::from_secs(2) + IDLE_RELEASE_GRACE);
        assert!(!output.is_open());
    }

    #[test]
    fn failed_output_initialization_retries_with_a_bounded_backoff() {
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        assert!(
            output
                .open_for_tone(now, 0, || Err(eyre!("device unavailable")))
                .is_err()
        );
        output
            .open_for_tone(now + Duration::from_secs(1), 0, || -> Result<()> {
                panic!("must not spin on device failure")
            })
            .unwrap();
        assert!(!output.is_open());
        output
            .open_for_tone(now + OPEN_RETRY_BACKOFF, 0, || Ok(()))
            .unwrap();
        assert!(output.is_open());
    }

    /// Unplugging the default output leaves a warm stream feeding a dead
    /// device. A route change drops it and the next tone opens the current
    /// default at once, even inside a failure backoff.
    #[test]
    fn a_route_change_reopens_the_output_on_the_current_default() {
        use std::cell::Cell;
        let opens = Cell::new(0_u32);
        let open = || {
            opens.set(opens.get() + 1);
            Ok(opens.get())
        };
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        output.open_for_tone(now, 0, open).unwrap();
        output.open_for_tone(now, 0, open).unwrap();
        assert_eq!(output.sink, Some(1));

        // The default output went away: the old stream is dropped and the
        // new default opens for the next tone.
        output.open_for_tone(now, 1, open).unwrap();
        assert_eq!(output.sink, Some(2));

        // A failed reopen backs off, but a later route change retries at once.
        assert!(
            output
                .open_for_tone(now, 2, || Err(eyre!("device unavailable")))
                .is_err()
        );
        assert!(!output.is_open());
        output
            .open_for_tone(now + Duration::from_millis(10), 3, open)
            .unwrap();
        assert_eq!(output.sink, Some(3));
    }

    #[test]
    #[ignore = "opens the native default output and inspects pmset assertions"]
    #[cfg(target_os = "macos")]
    fn native_output_release_clears_the_idle_sleep_assertion() {
        fn assertion_for_this_process() -> bool {
            let output = Command::new("/usr/bin/pmset")
                .args(["-g", "assertions"])
                .output()
                .unwrap();
            let report = String::from_utf8_lossy(&output.stdout);
            let marker = format!("Created for PID: {}", std::process::id());
            report
                .split("PreventUserIdleSystemSleep")
                .skip(1)
                .any(|section| section.lines().take(3).any(|line| line.contains(&marker)))
        }

        let mut sink = DeviceSinkBuilder::open_default_sink().unwrap();
        sink.log_on_drop(false);
        thread::sleep(Duration::from_millis(500));
        assert!(
            assertion_for_this_process(),
            "an open output should hold the coreaudiod sleep assertion"
        );
        drop(sink);
        let released = (0..20).any(|_| {
            thread::sleep(Duration::from_millis(250));
            !assertion_for_this_process()
        });
        assert!(released, "releasing the output should clear the assertion");
    }

    /// Times the default output's tone path without recording it: from the
    /// request to the mixer's first pull of the tone, plus one output buffer
    /// until that sample is presented. `HEX_TONE_PATH=legacy` measures the
    /// previous path (rodio's default 50 ms buffer). Route the default output
    /// to a virtual device first so nothing sounds aloud.
    #[test]
    #[ignore = "opens the native default output; route it to a silent device or set HEX_TONE_MUTED"]
    fn native_start_tone_path_latency() {
        use std::sync::Mutex;

        struct Probe<S> {
            inner: S,
            pulled: Arc<Mutex<Option<Instant>>>,
        }
        impl<S: Source> Iterator for Probe<S> {
            type Item = rodio::Sample;
            fn next(&mut self) -> Option<Self::Item> {
                self.pulled.lock().unwrap().get_or_insert_with(Instant::now);
                self.inner.next()
            }
        }
        impl<S: Source> Source for Probe<S> {
            fn current_span_len(&self) -> Option<usize> {
                None
            }
            fn channels(&self) -> rodio::ChannelCount {
                self.inner.channels()
            }
            fn sample_rate(&self) -> rodio::SampleRate {
                self.inner.sample_rate()
            }
            fn total_duration(&self) -> Option<Duration> {
                self.inner.total_duration()
            }
        }

        let legacy = std::env::var("HEX_TONE_PATH").is_ok_and(|path| path == "legacy");
        // A muted probe times a real speaker path without sounding.
        let gain = if std::env::var_os("HEX_TONE_MUTED").is_some() {
            0.0
        } else {
            1.0
        };
        let device =
            rodio::cpal::traits::HostTrait::default_output_device(&rodio::cpal::default_host())
                .and_then(|device| rodio::DeviceTrait::description(&device).ok())
                .map(|description| description.name().to_owned());
        eprintln!("default output: {device:?}");
        let start = decode_wav(BUNDLED_SETS[SoundSet::Signature as usize].start).unwrap();
        let open = || {
            if legacy {
                let mut sink = DeviceSinkBuilder::open_default_sink().unwrap();
                sink.log_on_drop(false);
                sink
            } else {
                open_output().unwrap()
            }
        };
        let measure = |sink: &mut Option<MixerDeviceSink>, label: &str| {
            let requested = Instant::now();
            let sink = sink.get_or_insert_with(open);
            let opened = requested.elapsed();
            let config = sink.config();
            let buffer = match config.buffer_size() {
                BufferSize::Fixed(frames) => Duration::from_secs_f64(
                    f64::from(*frames) / f64::from(config.sample_rate().get()),
                ),
                BufferSize::Default => Duration::ZERO,
            };
            let pulled = Arc::new(Mutex::new(None));
            sink.mixer().add(
                Probe {
                    inner: start.clone(),
                    pulled: pulled.clone(),
                }
                .amplify(gain),
            );
            while pulled.lock().unwrap().is_none() {
                thread::sleep(Duration::from_micros(200));
            }
            let pulled = pulled.lock().unwrap().unwrap();
            let latency = pulled - requested + buffer;
            eprintln!(
                "{label}: open {} ms, buffer {} ms, presented ~{} ms after request",
                opened.as_millis(),
                buffer.as_millis(),
                latency.as_millis()
            );
            thread::sleep(Duration::from_millis(1_200));
            latency
        };
        let mut sink = None;
        let cold = measure(&mut sink, "cold");
        let warm: Vec<_> = (0..5)
            .map(|_| measure(&mut sink, "warm").as_millis())
            .collect();
        eprintln!(
            "{} path: cold {} ms, warm {warm:?} ms",
            if legacy { "legacy" } else { "current" },
            cold.as_millis()
        );
    }

    #[test]
    fn bundled_sounds_decode_without_an_audio_device() {
        let sounds = Sounds::load().unwrap();
        for set in SoundSet::ALL {
            for (name, bytes, _) in bundled(set) {
                let mut sound = decode_wav(bytes).unwrap();
                assert!(
                    sound.total_duration().unwrap() > Duration::ZERO,
                    "{set:?} {name}"
                );
                assert!(sound.any(|sample| sample.abs() > 0.0), "{set:?} {name}");
            }
            // Every set plays onboarding success; Classic's is its stop sound.
            assert!(
                sounds.get(set, Tone::OnboardingSuccess).is_some(),
                "{set:?}"
            );
        }
        assert!(
            sounds
                .get(SoundSet::Signature, Tone::DictationDone)
                .is_some()
        );
        assert!(sounds.get(SoundSet::Classic, Tone::DictationDone).is_none());
        assert!(std::ptr::eq(
            sounds
                .get(SoundSet::Classic, Tone::OnboardingSuccess)
                .unwrap(),
            sounds.get(SoundSet::Classic, Tone::DictationStop).unwrap()
        ));
    }

    #[test]
    fn done_plays_only_after_a_slow_paste() {
        let stop = Instant::now();
        assert!(!done_is_late(Some(stop), stop + Duration::from_millis(300)));
        assert!(!done_is_late(
            Some(stop),
            stop + DONE_MIN_AFTER_STOP - Duration::from_millis(1)
        ));
        assert!(done_is_late(Some(stop), stop + DONE_MIN_AFTER_STOP));
        assert!(done_is_late(None, stop));
    }

    /// A dictation audition plays start, stop and done at their moments,
    /// each finished before the next, and every cue plays alone at once.
    #[test]
    fn auditions_play_each_sound_at_its_moment() {
        let sounds = Sounds::load().unwrap();
        for set in SoundSet::ALL {
            let dictation = sounds.audition(set, Audition::Dictation);
            let cues: Vec<_> = dictation.iter().map(|&(_, at, cue)| (cue, at)).collect();
            let expected: Vec<_> = DICTATION_CUES
                .into_iter()
                .filter(|&(cue, _)| set.has(cue))
                .collect();
            assert_eq!(cues, expected, "{set:?}");
            for pair in dictation.windows(2) {
                let ends = pair[0].1 + pair[0].0.total_duration().unwrap();
                assert!(ends < pair[1].1, "{set:?} {:?} overlaps", pair[0].2);
            }
            assert!(Audition::Dictation.length() > DICTATION_CUES[2].1);

            for cue in Cue::ALL {
                let alone = sounds.audition(set, Audition::Cue(cue));
                let own = set.has(cue) && (cue != Cue::Error || set.has_own_error());
                assert_eq!(alone.len(), usize::from(own), "{set:?} {cue:?}");
                assert!(
                    alone
                        .iter()
                        .all(|&(_, at, played)| at.is_zero() && played == cue)
                );
            }
        }
        assert!(!SoundSet::Classic.has(Cue::Done));
        assert!(SoundSet::Classic.has(Cue::Error));
    }

    /// Stopping fades every sound of the audition, even one still waiting
    /// out its delay, and the next audition starts unstopped.
    #[test]
    fn stopping_an_audition_fades_all_of_it() {
        let mut audition = Auditioning::default();
        let stopped = audition.begin();
        let sound = || {
            SamplesBuffer::new(
                std::num::NonZero::new(1).unwrap(),
                std::num::NonZero::new(48_000).unwrap(),
                vec![1.0; 48_000],
            )
        };
        let (now, _) = Retractable::with_flag(sound(), stopped.clone());
        let (later, _) =
            Retractable::with_flag(sound().delay(Duration::from_secs(1)), stopped.clone());
        audition.stop();
        let fade = (RETRACT_FADE.as_secs_f32() * 48_000.0) as usize;
        assert!(now.count() <= fade + 2);
        assert!(later.count() <= fade + 2);
        assert!(!audition.begin().load(Ordering::Acquire));
    }

    #[test]
    fn feedback_admission_never_waits_for_playback() {
        let (sender, receiver) = mpsc::sync_channel(2);
        enqueue(Some(&sender), Request::ProvisionalStart);
        enqueue(Some(&sender), Request::RetractStart);
        enqueue(Some(&sender), Request::Play(Tone::Cancel, 100));
        assert_eq!(receiver.try_recv().unwrap(), Request::ProvisionalStart);
        assert_eq!(receiver.try_recv().unwrap(), Request::RetractStart);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        enqueue(Some(&sender), Request::Play(Tone::Cancel, 100));
        enqueue(None, Request::ProvisionalStart);
    }

    #[test]
    fn timeout_and_loader_exit_report_distinct_errors() {
        // A silent channel models a loader still warming the audio stack; a
        // dropped one models a loader that exited before loading.
        assert_eq!(
            admission_error(Err(mpsc::RecvTimeoutError::Timeout))
                .unwrap_err()
                .to_string(),
            "timed out preloading feedback audio"
        );
        assert_eq!(
            admission_error(Err(mpsc::RecvTimeoutError::Disconnected))
                .unwrap_err()
                .to_string(),
            "feedback audio loader exited before initializing"
        );

        // Real channels exercise the same admission wait the loader drives.
        let (slow_sender, slow_receiver) = mpsc::sync_channel::<Result<()>>(1);
        assert!(admission_error(slow_receiver.recv_timeout(Duration::ZERO)).is_err());
        drop(slow_sender);

        let (done_sender, done_receiver) = mpsc::sync_channel::<Result<()>>(1);
        done_sender.send(Ok(())).unwrap();
        assert!(admission_error(done_receiver.recv_timeout(Duration::ZERO)).is_ok());

        let (failed_sender, failed_receiver) = mpsc::sync_channel::<Result<()>>(1);
        failed_sender
            .send(Err(eyre!("could not open the audio output")))
            .unwrap();
        assert_eq!(
            admission_error(failed_receiver.recv_timeout(Duration::ZERO))
                .unwrap_err()
                .to_string(),
            "could not open the audio output"
        );
    }
}
