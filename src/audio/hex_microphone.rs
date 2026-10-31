use std::cell::UnsafeCell;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::voice_filter::{
    DictationSpan, HexMicEngine, ModeSpec, StageSpec, VoiceFilterMode,
};

const HEX_MICROPHONE_NAME: &str = "Hex Microphone";
const HEX_MICROPHONE_UID: &str = "anomaly.hex.microphone";
const HEX_MICROPHONE_SINK_NAME: &str = "Hex Microphone (Internal Sink)";
const HEX_MICROPHONE_SINK_UID: &str = "anomaly.hex.microphone.sink";
const HEX_MICROPHONE_SAMPLE_RATE: u32 = 48_000;

/// Capacity of the lock-free capture-to-sink ring buffer in 48 kHz mono samples (~341 ms).
const RING_CAPACITY: usize = 16_384;
const RING_MASK: usize = RING_CAPACITY - 1;
/// Queued frames, beyond one producer burst and one consumer pull, before the
/// consumer skips ahead to bound added latency (16 ms).
const MAX_RING_LATENCY_FRAMES: usize = 768;
/// Upper bound on the producer burst the latency cap makes room for (~85 ms).
const MAX_PRODUCER_BURST_FRAMES: usize = RING_CAPACITY / 4;
/// A smaller callback closes 1/N of the gap to a feed's recorded burst, so one
/// coalesced callback widens the latency cap for about a second, not for good.
const BURST_DECAY_CALLBACKS: usize = 32;

pub fn is_hex_virtual_device_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.eq_ignore_ascii_case(HEX_MICROPHONE_NAME)
        || trimmed.eq_ignore_ascii_case(HEX_MICROPHONE_SINK_NAME)
        || trimmed.to_ascii_lowercase().starts_with("hex microphone")
}

/// Lock-free single-producer single-consumer ring buffer for real-time audio handoff.
struct HexMicRingBuffer {
    buffer: [UnsafeCell<f32>; RING_CAPACITY],
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
    underflows: AtomicU64,
    overflows: AtomicU64,
    /// The active feed's recent largest capture callback, in frames.
    producer_burst: AtomicUsize,
}

unsafe impl Sync for HexMicRingBuffer {}

impl HexMicRingBuffer {
    const fn new() -> Self {
        Self {
            buffer: [const { UnsafeCell::new(0.0) }; RING_CAPACITY],
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
            underflows: AtomicU64::new(0),
            overflows: AtomicU64::new(0),
            producer_burst: AtomicUsize::new(0),
        }
    }

    fn clear(&self) {
        let w = self.write_pos.load(Ordering::Relaxed);
        self.read_pos.store(w, Ordering::Release);
        self.producer_burst.store(0, Ordering::Relaxed);
    }

    /// Sets the frames the latency cap leaves room for: one whole capture
    /// callback. A device delivering 20 ms at a time (a 24 kHz Bluetooth
    /// headset) would otherwise lose audio on every callback.
    fn set_producer_burst(&self, frames: usize) {
        self.producer_burst
            .store(frames.min(MAX_PRODUCER_BURST_FRAMES), Ordering::Relaxed);
    }

    fn counters(&self) -> (u64, u64) {
        (
            self.underflows.load(Ordering::Relaxed),
            self.overflows.load(Ordering::Relaxed),
        )
    }

    /// Pushes samples from the producer without blocking or allocating.
    fn push_slice(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let slice = if samples.len() > RING_CAPACITY {
            &samples[samples.len() - RING_CAPACITY..]
        } else {
            samples
        };
        let count = slice.len();
        let write = self.write_pos.load(Ordering::Relaxed);
        let read = self.read_pos.load(Ordering::Acquire);
        let available = write.wrapping_sub(read);

        if available + count > RING_CAPACITY {
            let new_read = write.wrapping_add(count).wrapping_sub(RING_CAPACITY);
            self.read_pos.store(new_read, Ordering::Release);
            self.overflows.fetch_add(1, Ordering::Relaxed);
        }

        for (i, &sample) in slice.iter().enumerate() {
            let idx = write.wrapping_add(i) & RING_MASK;
            // SAFETY: Bounded by RING_CAPACITY; producer is the sole writer.
            unsafe {
                *self.buffer[idx].get() = sample;
            }
        }

        self.write_pos
            .store(write.wrapping_add(count), Ordering::Release);
    }

    /// Pulls `dst.len()` samples for the real-time consumer without blocking.
    /// Zero-fills on underflow and skips ahead once more than
    /// `MAX_RING_LATENCY_FRAMES` beyond one producer burst are queued.
    fn pop_slice(&self, dst: &mut [f32]) -> usize {
        let needed = dst.len();
        if needed == 0 {
            return 0;
        }
        let write = self.write_pos.load(Ordering::Acquire);
        let mut read = self.read_pos.load(Ordering::Relaxed);
        let mut available = write.wrapping_sub(read);
        let burst = self.producer_burst.load(Ordering::Relaxed);
        let slack = MAX_RING_LATENCY_FRAMES / 2;
        let limit = MAX_RING_LATENCY_FRAMES.max(burst + slack) + needed;

        if available > RING_CAPACITY {
            read = write;
            available = 0;
        } else if available > limit {
            let keep = limit - slack;
            read = write.wrapping_sub(keep);
            available = keep;
            self.overflows.fetch_add(1, Ordering::Relaxed);
        }

        let to_copy = available.min(needed);
        for (i, out) in dst.iter_mut().take(to_copy).enumerate() {
            let idx = read.wrapping_add(i) & RING_MASK;
            // SAFETY: Consumer reads within [read, write).
            *out = unsafe { *self.buffer[idx].get() };
        }
        if to_copy < needed {
            dst[to_copy..].fill(0.0);
            self.underflows.fetch_add(1, Ordering::Relaxed);
        }

        self.read_pos
            .store(read.wrapping_add(to_copy), Ordering::Release);
        to_copy
    }

    #[cfg(test)]
    fn producer_burst(&self) -> usize {
        self.producer_burst.load(Ordering::Relaxed)
    }

    #[cfg(any(test, target_os = "linux"))]
    fn queued_frames(&self) -> usize {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read).min(RING_CAPACITY)
    }

    #[cfg(test)]
    fn underflows(&self) -> u64 {
        self.underflows.load(Ordering::Relaxed)
    }
}

static CAPTURE_RING: HexMicRingBuffer = HexMicRingBuffer::new();
static SINK_STREAMING: AtomicBool = AtomicBool::new(false);
static SUPERVISOR_STARTED: AtomicBool = AtomicBool::new(false);
static SERVICE_HOSTED: AtomicBool = AtomicBool::new(false);
static PENDING_ENGINE_SWAP: AtomicBool = AtomicBool::new(false);
static PENDING_ENGINE: OnceLock<Mutex<Option<HexMicEngine>>> = OnceLock::new();

// Tunable per-mode parameters for the live engine and Voice Filter pane sliders.
#[derive(Debug, Clone, PartialEq)]
pub struct TunedFilterConfig {
    pub mode: VoiceFilterMode,
    pub comfort_noise: bool,
    pub strength: f32,
    pub muffled_wall_hz: f64,
    pub muffled_room_mix: f64,
    pub muffled_level_db: f64,
    pub quiet_level_db: f64,
    pub muted_room_tone_db: f64,
    pub chipmunk_semitones: f64,
    pub robot_carrier_hz: f64,
    pub walkie_crunch: f64,
    pub underwater_depth_hz: f64,
    pub far_distance_mix: f64,
}

impl Default for TunedFilterConfig {
    fn default() -> Self {
        Self {
            mode: VoiceFilterMode::Muffled,
            comfort_noise: true,
            strength: 1.0,
            muffled_wall_hz: 480.0,
            muffled_room_mix: 0.3,
            muffled_level_db: -9.0,
            quiet_level_db: -24.0,
            muted_room_tone_db: -68.0,
            chipmunk_semitones: 7.0,
            robot_carrier_hz: 70.0,
            walkie_crunch: 4.0,
            underwater_depth_hz: 380.0,
            far_distance_mix: 0.75,
        }
    }
}

impl TunedFilterConfig {
    pub fn build_mode_spec(&self) -> ModeSpec {
        let mut spec = self.mode.spec_with_options(self.comfort_noise);
        match self.mode {
            VoiceFilterMode::Normal => {}
            VoiceFilterMode::Muffled => {
                if let Some(StageSpec::Lowpass { hz, .. }) = spec.stages.get_mut(0) {
                    *hz = self.muffled_wall_hz;
                }
                if let Some(StageSpec::Reverb { mix, .. }) = spec.stages.get_mut(2) {
                    *mix = self.muffled_room_mix;
                }
                if let Some(StageSpec::Gain { db }) = spec.stages.get_mut(3) {
                    *db = self.muffled_level_db;
                }
            }
            VoiceFilterMode::Quiet => {
                if let Some(StageSpec::Gain { db }) = spec.stages.get_mut(1) {
                    *db = self.quiet_level_db;
                }
            }
            VoiceFilterMode::Muted => {
                if let Some(StageSpec::Noise { db, .. }) = spec.stages.get_mut(1) {
                    *db = if self.comfort_noise {
                        self.muted_room_tone_db
                    } else {
                        -120.0
                    };
                }
            }
            VoiceFilterMode::Chipmunk => {
                if let Some(StageSpec::Pitch { semitones, .. }) = spec.stages.get_mut(0) {
                    *semitones = self.chipmunk_semitones;
                }
            }
            VoiceFilterMode::Robot => {
                if let Some(StageSpec::Ringmod { hz, .. }) = spec.stages.get_mut(0) {
                    *hz = self.robot_carrier_hz;
                }
            }
            VoiceFilterMode::Walkie => {
                if let Some(StageSpec::Drive { amount }) = spec.stages.get_mut(3) {
                    *amount = self.walkie_crunch;
                }
            }
            VoiceFilterMode::Underwater => {
                if let Some(StageSpec::Wobble { hz, .. }) = spec.stages.get_mut(0) {
                    *hz = self.underwater_depth_hz;
                }
            }
            VoiceFilterMode::Far => {
                if let Some(StageSpec::Reverb { mix, .. }) = spec.stages.get_mut(2) {
                    *mix = self.far_distance_mix;
                }
            }
        }
        spec
    }

    fn build_engine(&self) -> HexMicEngine {
        let spec = self.build_mode_spec();
        let mut engine = HexMicEngine::new(f64::from(HEX_MICROPHONE_SAMPLE_RATE), &spec);
        engine.set_strength(f64::from(self.strength));
        engine
    }
}

static TUNED_CONFIG: OnceLock<Mutex<TunedFilterConfig>> = OnceLock::new();

fn tuned_config_lock() -> &'static Mutex<TunedFilterConfig> {
    TUNED_CONFIG.get_or_init(|| Mutex::new(TunedFilterConfig::default()))
}

fn pending_engine_lock() -> &'static Mutex<Option<HexMicEngine>> {
    PENDING_ENGINE.get_or_init(|| Mutex::new(None))
}

pub fn current_tuned_config() -> TunedFilterConfig {
    tuned_config_lock()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

pub fn update_tuned_config(mutate: impl FnOnce(&mut TunedFilterConfig)) {
    let next_engine = {
        let Ok(mut guard) = tuned_config_lock().lock() else {
            return;
        };
        mutate(&mut guard);
        guard.build_engine()
    };
    if let Ok(mut slot) = pending_engine_lock().lock() {
        *slot = Some(next_engine);
        PENDING_ENGINE_SWAP.store(true, Ordering::Release);
    }
    if DEMO_BUFFERS.get().is_some() {
        schedule_demo_recompute();
    }
}

pub fn sync_from_settings(mode: VoiceFilterMode, comfort_noise: bool, strength: f32) {
    update_tuned_config(|cfg| {
        cfg.mode = mode;
        cfg.comfort_noise = comfort_noise;
        cfg.strength = strength.clamp(0.0, 1.0);
    });
    if crate::app_settings::hex_microphone_enabled() {
        ensure_service_started();
    }
}

/// Whether a dictation is past its hold threshold, so the filter applies.
#[cfg(target_os = "macos")]
fn dictating() -> bool {
    crate::audio::recording_environment::intentional_recording_active()
}

#[cfg(target_os = "linux")]
fn dictating() -> bool {
    DICTATIONS.load(Ordering::Acquire) > 0
}

#[cfg(target_os = "linux")]
static DICTATIONS: AtomicUsize = AtomicUsize::new(0);

/// An intentional dictation the Voice Filter applies to until dropped, where
/// no recording environment session marks one (Linux).
#[cfg(target_os = "linux")]
pub struct Dictation(());

#[cfg(target_os = "linux")]
impl Dictation {
    pub fn begin() -> Self {
        DICTATIONS.fetch_add(1, Ordering::AcqRel);
        Self(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for Dictation {
    fn drop(&mut self) {
        DICTATIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Fills `out` with the next Hex Microphone audio, from the sink's
/// real-time callback: adopts a retuned engine, follows the dictation, and
/// filters the microphone audio queued in the ring.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn render(engine: &mut HexMicEngine, out: &mut [f32]) {
    if PENDING_ENGINE_SWAP.load(Ordering::Acquire)
        && let Ok(mut guard) = pending_engine_lock().try_lock()
    {
        if let Some(mut next_engine) = guard.take() {
            next_engine.set_dictating(engine.is_dictating());
            *engine = next_engine;
        }
        PENDING_ENGINE_SWAP.store(false, Ordering::Release);
    }
    engine.set_dictating(dictating());
    CAPTURE_RING.pop_slice(out);
    engine.process_in_place(out);
}

/// One microphone stream's connection to Hex Microphone. Every capture stream
/// has one, but only the dictation listener's current stream switches it on
/// (`CaptureFeedSwitch`): the ring has a single producer, and a second stream
/// on the same microphone, such as a meeting recording's, would interleave a
/// duplicate of every callback into it.
pub struct CaptureFeed {
    enabled: Arc<AtomicBool>,
    resampler: Option<LinearResampler>,
    /// This feed's recent largest callback, decaying; zero while inactive,
    /// so a newly active feed never inherits another microphone's burst.
    burst: usize,
}

/// Turns one stream's `CaptureFeed` on or off from outside the audio callback.
#[derive(Clone)]
pub struct CaptureFeedSwitch(Arc<AtomicBool>);

impl CaptureFeedSwitch {
    pub fn set(&self, on: bool) {
        self.0.store(on, Ordering::Release);
    }

    #[cfg(test)]
    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// A switched-off feed for a capture stream delivering `sample_rate` audio.
pub fn capture_feed(sample_rate: u32) -> (CaptureFeed, CaptureFeedSwitch) {
    let enabled = Arc::new(AtomicBool::new(false));
    let feed = CaptureFeed {
        enabled: enabled.clone(),
        resampler: (sample_rate != HEX_MICROPHONE_SAMPLE_RATE && sample_rate > 0)
            .then(|| LinearResampler::new(sample_rate)),
        burst: 0,
    };
    (feed, CaptureFeedSwitch(enabled))
}

impl CaptureFeed {
    /// Called from the capture callback. Never blocks, never allocates.
    pub fn push(&mut self, samples: &[f32]) {
        let sink_active = SINK_STREAMING.load(Ordering::Relaxed);
        let demo_recording = DEMO_RECORDING_ACTIVE.load(Ordering::Relaxed);
        self.push_to(
            sink_active.then_some(&CAPTURE_RING),
            demo_recording,
            samples,
        );
    }

    fn push_to(&mut self, ring: Option<&HexMicRingBuffer>, demo: bool, samples: &[f32]) {
        if samples.is_empty() || !self.enabled.load(Ordering::Acquire) || (ring.is_none() && !demo)
        {
            if let Some(resampler) = &mut self.resampler {
                resampler.reset();
            }
            self.burst = 0;
            return;
        }
        let deliver = |chunk: &[f32]| {
            if let Some(ring) = ring {
                ring.push_slice(chunk);
            }
            if demo {
                append_demo_recording_samples(chunk);
            }
        };
        let frames = match &mut self.resampler {
            None => {
                deliver(samples);
                samples.len()
            }
            Some(resampler) => resampler.process(samples, deliver),
        };
        self.burst = if frames >= self.burst {
            frames
        } else {
            self.burst - (self.burst - frames).div_ceil(BURST_DECAY_CALLBACKS)
        };
        if let Some(ring) = ring {
            ring.set_producer_burst(self.burst);
        }
    }
}

/// Linear interpolation to 48 kHz that carries its phase and last sample
/// across callbacks, so block boundaries neither step nor gain or lose frames.
struct LinearResampler {
    /// Input frames per output frame.
    step: f64,
    /// Next output position in input frames, relative to the current block;
    /// -1 is the previous block's last sample.
    position: f64,
    previous: f32,
    primed: bool,
}

impl LinearResampler {
    fn new(sample_rate: u32) -> Self {
        Self {
            step: f64::from(sample_rate) / f64::from(HEX_MICROPHONE_SAMPLE_RATE),
            position: 0.0,
            previous: 0.0,
            primed: false,
        }
    }

    fn reset(&mut self) {
        self.primed = false;
    }

    /// Resamples `input` in stack chunks handed to `emit`; returns the frames emitted.
    fn process(&mut self, input: &[f32], mut emit: impl FnMut(&[f32])) -> usize {
        let Some(&last) = input.last() else {
            return 0;
        };
        if !self.primed {
            self.previous = input[0];
            self.position = -1.0;
            self.primed = true;
        }
        let end = (input.len() - 1) as f64;
        let mut chunk = [0.0_f32; 512];
        let mut filled = 0;
        let mut total = 0;
        while self.position < end {
            let floor = self.position.floor();
            let frac = (self.position - floor) as f32;
            let index = floor as isize;
            let a = if index < 0 {
                self.previous
            } else {
                input[index as usize]
            };
            let b = input[(index + 1) as usize];
            chunk[filled] = a + (b - a) * frac;
            filled += 1;
            if filled == chunk.len() {
                emit(&chunk);
                total += filled;
                filled = 0;
            }
            self.position += self.step;
        }
        if filled > 0 {
            emit(&chunk[..filled]);
            total += filled;
        }
        self.position -= input.len() as f64;
        self.previous = last;
        total
    }
}

// ── Status, Bundled Driver Discovery, and Privileged Install/Uninstall ──────

const INSTALLED_DRIVER_PATH: &str = "/Library/Audio/Plug-Ins/HAL/HexMicrophone.driver";
const EXPECTED_DRIVER_BUNDLE_ID: &str = "anomaly.hex.microphone.driver";

/// Fixed AppleScript for driver installation. No user input is ever interpolated
/// into this script; `osascript` receives the verified bundled `.driver` path
/// via `argv` and quotes it with `quoted form of (item 1 of argv)`.
#[cfg(target_os = "macos")]
const INSTALL_DRIVER_APPLESCRIPT: &str = r#"on run argv
    set srcPath to quoted form of (item 1 of argv)
    set dstDir to "'/Library/Audio/Plug-Ins/HAL'"
    set dstPath to "'/Library/Audio/Plug-Ins/HAL/HexMicrophone.driver'"
    set tmpPath to "'/Library/Audio/Plug-Ins/HAL/.HexMicrophone.driver.staging'"
    do shell script "/bin/mkdir -p " & dstDir & " && /bin/rm -rf " & tmpPath & " && /usr/bin/ditto " & srcPath & " " & tmpPath & " && /usr/sbin/chown -R root:wheel " & tmpPath & " && /bin/chmod -R a+rX " & tmpPath & " && /bin/rm -rf " & dstPath & " && /bin/mv " & tmpPath & " " & dstPath & " && (/bin/launchctl kickstart -k system/com.apple.audio.coreaudiod || /usr/bin/killall coreaudiod)" with administrator privileges
end run"#;

/// Fixed AppleScript for driver removal. Takes no arguments and touches only
/// `/Library/Audio/Plug-Ins/HAL/HexMicrophone.driver`.
#[cfg(target_os = "macos")]
const UNINSTALL_DRIVER_APPLESCRIPT: &str = r#"do shell script "/bin/rm -rf '/Library/Audio/Plug-Ins/HAL/HexMicrophone.driver' '/Library/Audio/Plug-Ins/HAL/.HexMicrophone.driver.staging' && (/bin/launchctl kickstart -k system/com.apple.audio.coreaudiod || /usr/bin/killall coreaudiod)" with administrator privileges"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverActionState {
    Idle,
    Installing,
    Uninstalling,
    DoneInstalled(String),
    DoneUninstalled,
    Cancelled,
    Failed(String),
}

static DRIVER_ACTION_STATE: OnceLock<Mutex<DriverActionState>> = OnceLock::new();

fn driver_action_lock() -> &'static Mutex<DriverActionState> {
    DRIVER_ACTION_STATE.get_or_init(|| Mutex::new(DriverActionState::Idle))
}

fn set_driver_action_state(state: DriverActionState) {
    let mut guard = driver_action_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *guard = state;
}

fn current_driver_action_state() -> DriverActionState {
    driver_action_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn driver_action_in_progress() -> bool {
    matches!(
        current_driver_action_state(),
        DriverActionState::Installing | DriverActionState::Uninstalling
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexMicrophoneStatus {
    pub driver_bundle_installed: bool,
    pub driver_loaded: bool,
    pub installed_version: Option<String>,
    pub bundled_version: String,
    pub sink_streaming: bool,
    pub mic_in_use: bool,
    pub active_apps: Vec<String>,
    pub action_state: DriverActionState,
}

impl HexMicrophoneStatus {
    pub fn needs_update(&self) -> bool {
        self.driver_bundle_installed
            && self.installed_version.as_deref() != Some(self.bundled_version.as_str())
    }

    #[cfg(test)]
    pub fn is_up_to_date(&self) -> bool {
        self.driver_loaded && !self.needs_update()
    }
}

pub fn driver_bundle_installed() -> bool {
    std::path::Path::new(INSTALLED_DRIVER_PATH).exists()
}

fn plist_string_value(plist_xml: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let after_key = &plist_xml[plist_xml.find(&needle)? + needle.len()..];
    let start = after_key.find("<string>")? + "<string>".len();
    let end = after_key[start..].find("</string>")?;
    let value = after_key[start..start + end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn read_driver_bundle_version(bundle_dir: &std::path::Path) -> Option<String> {
    let plist_path = bundle_dir.join("Contents/Info.plist");
    let xml = std::fs::read_to_string(plist_path).ok()?;
    if let Some(bundle_id) = plist_string_value(&xml, "CFBundleIdentifier")
        && bundle_id != EXPECTED_DRIVER_BUNDLE_ID
    {
        return None;
    }
    plist_string_value(&xml, "CFBundleShortVersionString")
}

fn validate_driver_bundle(bundle_dir: &std::path::Path) -> Result<String, String> {
    if bundle_dir.file_name().and_then(|s| s.to_str()) != Some("HexMicrophone.driver") {
        return Err("Invalid driver bundle name".into());
    }
    let binary = bundle_dir.join("Contents/MacOS/HexMicrophoneDriver");
    if !binary.is_file() {
        return Err("Driver executable is missing from bundle".into());
    }
    read_driver_bundle_version(bundle_dir)
        .ok_or_else(|| "Driver Info.plist is missing or invalid".into())
}

fn bundled_driver_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(contents_dir) = exe.parent().and_then(|p| p.parent())
    {
        candidates.push(contents_dir.join("Library/HexMicrophone.driver"));
        candidates.push(contents_dir.join("Resources/HexMicrophone.driver"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Library/Caches/hex-mic/HexMicrophone.driver"));
    }
    candidates.push(PathBuf::from(
        "/Applications/Hex.app/Contents/Library/HexMicrophone.driver",
    ));
    candidates
        .into_iter()
        .find(|candidate| validate_driver_bundle(candidate).is_ok())
}

fn bundled_driver_version() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            bundled_driver_path()
                .and_then(|path| read_driver_bundle_version(&path))
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
        })
        .clone()
}

fn installed_driver_version() -> Option<String> {
    read_driver_bundle_version(std::path::Path::new(INSTALLED_DRIVER_PATH))
}

#[cfg(target_os = "macos")]
fn classify_osascript_failure(stderr: &str, stdout: &str) -> DriverActionState {
    let combined = format!("{stderr}\n{stdout}");
    let lower = combined.to_ascii_lowercase();
    if lower.contains("user canceled")
        || lower.contains("user cancelled")
        || lower.contains("(-128)")
    {
        return DriverActionState::Cancelled;
    }
    let detail = combined
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Authorization or Core Audio restart failed");
    let cleaned = detail
        .rsplit_once("execution error: ")
        .map(|(_, tail)| tail)
        .unwrap_or(detail);
    DriverActionState::Failed(cleaned.chars().take(140).collect())
}

#[cfg(target_os = "macos")]
fn run_driver_osascript(
    script: &str,
    arg: Option<&std::path::Path>,
    settled: impl Fn(&HexMicrophoneStatus) -> bool,
    done: impl FnOnce() -> DriverActionState,
) -> DriverActionState {
    let mut command = std::process::Command::new("/usr/bin/osascript");
    command.arg("-e").arg(script);
    if let Some(arg) = arg {
        command.arg(arg);
    }
    match command.output() {
        Ok(out) if out.status.success() => {
            crate::audio::notify_audio_service_restarted();
            let deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < deadline {
                if settled(&status()) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            crate::audio::notify_audio_service_restarted();
            done()
        }
        Ok(out) => classify_osascript_failure(
            &String::from_utf8_lossy(&out.stderr),
            &String::from_utf8_lossy(&out.stdout),
        ),
        Err(error) => DriverActionState::Failed(error.to_string()),
    }
}

pub fn install_driver_async() {
    #[cfg(target_os = "macos")]
    {
        if driver_action_in_progress() {
            return;
        }
        set_driver_action_state(DriverActionState::Installing);
        let _ = thread::Builder::new()
            .name("hex-mic-install".into())
            .spawn(|| {
                let Some(src_bundle) = bundled_driver_path() else {
                    set_driver_action_state(DriverActionState::Failed(
                        "Bundled HexMicrophone.driver was not found in Hex.app".into(),
                    ));
                    return;
                };
                let version = match validate_driver_bundle(&src_bundle) {
                    Ok(v) => v,
                    Err(reason) => {
                        set_driver_action_state(DriverActionState::Failed(reason));
                        return;
                    }
                };
                let state = run_driver_osascript(
                    INSTALL_DRIVER_APPLESCRIPT,
                    Some(&src_bundle),
                    |st| st.driver_loaded,
                    || {
                        DriverActionState::DoneInstalled(
                            installed_driver_version().unwrap_or(version),
                        )
                    },
                );
                set_driver_action_state(state);
                // An installed driver turns the Voice Filter on by itself.
                ensure_service_started();
            });
    }
}

pub fn uninstall_driver_async() {
    #[cfg(target_os = "macos")]
    {
        if driver_action_in_progress() {
            return;
        }
        set_driver_action_state(DriverActionState::Uninstalling);
        let _ = thread::Builder::new()
            .name("hex-mic-uninstall".into())
            .spawn(|| {
                let state = run_driver_osascript(
                    UNINSTALL_DRIVER_APPLESCRIPT,
                    None,
                    |st| !st.driver_bundle_installed && !st.driver_loaded,
                    || DriverActionState::DoneUninstalled,
                );
                set_driver_action_state(state);
            });
    }
}

#[cfg(target_os = "macos")]
pub fn status() -> HexMicrophoneStatus {
    let mic_id = macos_hal::translate_uid_to_device(HEX_MICROPHONE_UID);
    let sink_id = macos_hal::translate_uid_to_device(HEX_MICROPHONE_SINK_UID);
    let driver_loaded = mic_id.is_some() && sink_id.is_some();
    let mic_in_use = mic_id.is_some_and(macos_hal::device_is_running_somewhere);
    let my_pid = std::process::id() as i32;
    let active_apps: Vec<String> = crate::microphone_activity::active_microphone_applications()
        .unwrap_or_default()
        .into_iter()
        .filter(|proc| proc.pid != my_pid && !proc.name.eq_ignore_ascii_case("hex"))
        .map(|proc| proc.name)
        .collect();

    HexMicrophoneStatus {
        driver_bundle_installed: driver_bundle_installed(),
        driver_loaded,
        installed_version: installed_driver_version(),
        bundled_version: bundled_driver_version(),
        sink_streaming: SINK_STREAMING.load(Ordering::Acquire),
        mic_in_use,
        active_apps,
        action_state: current_driver_action_state(),
    }
}

/// Linux has no driver: Hex Microphone exists while the Voice Filter runs
/// (`pipewire_source`).
#[cfg(target_os = "linux")]
pub fn status() -> HexMicrophoneStatus {
    let streaming = SINK_STREAMING.load(Ordering::Acquire);
    HexMicrophoneStatus {
        driver_bundle_installed: false,
        driver_loaded: streaming,
        installed_version: None,
        bundled_version: env!("CARGO_PKG_VERSION").to_string(),
        sink_streaming: streaming,
        mic_in_use: false,
        active_apps: Vec::new(),
        action_state: current_driver_action_state(),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn status() -> HexMicrophoneStatus {
    HexMicrophoneStatus {
        driver_bundle_installed: false,
        driver_loaded: false,
        installed_version: None,
        bundled_version: env!("CARGO_PKG_VERSION").to_string(),
        sink_streaming: false,
        mic_in_use: false,
        active_apps: Vec::new(),
        action_state: DriverActionState::Idle,
    }
}

/// Makes this process the Hex Microphone host, starting the sink whenever
/// the Voice Filter is on (`app_settings::hex_microphone_enabled`). Only the listener hosts it, so CLI commands that load
/// settings never start the pipeline.
pub fn host_service() {
    SERVICE_HOSTED.store(true, Ordering::Release);
    ensure_service_started();
}

fn ensure_service_started() {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        if !SERVICE_HOSTED.load(Ordering::Acquire) || !crate::app_settings::hex_microphone_enabled()
        {
            return;
        }
        if SUPERVISOR_STARTED.swap(true, Ordering::AcqRel) {
            return;
        }
        #[cfg(target_os = "macos")]
        let supervisor = macos_hal::run_sink_supervisor;
        #[cfg(target_os = "linux")]
        let supervisor = pipewire_source::run_supervisor;
        let _ = thread::Builder::new()
            .name("hex-mic-sink".to_string())
            .spawn(supervisor);
    }
}

// ── "Try it" Demo Recording & Playback (in-memory only) ──────────────────────

const DEMO_RECORD_MAX_SAMPLES: usize = (HEX_MICROPHONE_SAMPLE_RATE as usize) * 5; // 5.0 seconds
const WAVEFORM_BARS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoSourceKind {
    Example,
    Recording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoPlaybackTrack {
    CallHears,
    You,
}

#[derive(Debug, Clone)]
pub struct DemoSnapshot {
    pub source: DemoSourceKind,
    pub has_recording: bool,
    pub is_recording: bool,
    pub recording_progress: f32,
    pub playing: Option<DemoPlaybackTrack>,
    pub playback_progress: f32,
    pub duration_ms: u32,
    pub span: DictationSpan,
    pub peaks_you: Vec<f32>,
    pub peaks_call: Vec<f32>,
}

static DEMO_RECORDING_ACTIVE: AtomicBool = AtomicBool::new(false);
static DEMO_RECORDED_LEN: AtomicUsize = AtomicUsize::new(0);
static DEMO_PLAYBACK_GEN: AtomicU64 = AtomicU64::new(0);
static DEMO_PLAYING_TRACK: AtomicU32 = AtomicU32::new(0); // 0 = none, 1 = CallHears, 2 = You
static DEMO_PLAYBACK_PROGRESS_BITS: AtomicU32 = AtomicU32::new(0.0_f32.to_bits());
static DEMO_CLOCK_EPOCH: OnceLock<Instant> = OnceLock::new();
static DEMO_PLAYBACK_START_NS: AtomicU64 = AtomicU64::new(0);
static DEMO_PLAYBACK_DURATION_NS: AtomicU64 = AtomicU64::new(0);
static DEMO_RECORDING_START_NS: AtomicU64 = AtomicU64::new(0);
static DEMO_RECOMPUTE_RUNNING: AtomicBool = AtomicBool::new(false);
static DEMO_RECOMPUTE_DIRTY: AtomicBool = AtomicBool::new(false);

fn demo_now_ns() -> u64 {
    let epoch = DEMO_CLOCK_EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

pub fn demo_is_busy() -> bool {
    DEMO_RECORDING_ACTIVE.load(Ordering::Acquire) || DEMO_PLAYING_TRACK.load(Ordering::Acquire) != 0
}

pub fn demo_recompute_is_running() -> bool {
    DEMO_RECOMPUTE_RUNNING.load(Ordering::Acquire) || DEMO_RECOMPUTE_DIRTY.load(Ordering::Acquire)
}

fn schedule_demo_recompute() {
    DEMO_RECOMPUTE_DIRTY.store(true, Ordering::Release);
    if DEMO_RECOMPUTE_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = thread::Builder::new()
        .name("hex-mic-demo-dsp".into())
        .spawn(|| {
            while DEMO_RECOMPUTE_DIRTY.swap(false, Ordering::AcqRel) {
                let cfg = current_tuned_config();
                let Some((source, active_samples)) = ({
                    let Ok(guard) = demo_buffers_lock().lock() else {
                        break;
                    };
                    if guard.cached_mode.as_ref() == Some(&cfg)
                        && guard.cached_source == Some(guard.source)
                    {
                        None
                    } else {
                        Some((guard.source, guard.active_samples().to_vec()))
                    }
                }) else {
                    continue;
                };
                let span = span_for_source(source, active_samples.len());
                let spec = cfg.build_mode_spec();
                let processed_samples = crate::audio::voice_filter::render(
                    &active_samples,
                    f64::from(HEX_MICROPHONE_SAMPLE_RATE),
                    &spec,
                    &[span],
                    f64::from(cfg.strength),
                );
                let peaks_you = compute_peaks(&active_samples, WAVEFORM_BARS);
                let peaks_call = compute_peaks(&processed_samples, WAVEFORM_BARS);
                if let Ok(mut guard) = demo_buffers_lock().lock()
                    && guard.source == source
                {
                    guard.processed_samples = processed_samples;
                    guard.peaks_you = peaks_you;
                    guard.peaks_call = peaks_call;
                    guard.cached_mode = Some(cfg);
                    guard.cached_source = Some(source);
                }
            }
            DEMO_RECOMPUTE_RUNNING.store(false, Ordering::Release);
        });
}

struct DemoBuffers {
    source: DemoSourceKind,
    example_samples: Vec<f32>,
    recorded_samples: Vec<f32>,
    cached_mode: Option<TunedFilterConfig>,
    cached_source: Option<DemoSourceKind>,
    peaks_you: Vec<f32>,
    peaks_call: Vec<f32>,
    processed_samples: Vec<f32>,
}

impl DemoBuffers {
    fn active_samples(&self) -> &[f32] {
        if self.source == DemoSourceKind::Recording && !self.recorded_samples.is_empty() {
            &self.recorded_samples
        } else {
            &self.example_samples
        }
    }
}

static DEMO_BUFFERS: OnceLock<Mutex<DemoBuffers>> = OnceLock::new();

fn demo_buffers_lock() -> &'static Mutex<DemoBuffers> {
    DEMO_BUFFERS.get_or_init(|| {
        let example_samples = decode_bundled_example();
        let mut buffers = DemoBuffers {
            source: DemoSourceKind::Example,
            example_samples,
            recorded_samples: Vec::with_capacity(DEMO_RECORD_MAX_SAMPLES),
            cached_mode: None,
            cached_source: None,
            peaks_you: vec![0.0; WAVEFORM_BARS],
            peaks_call: vec![0.0; WAVEFORM_BARS],
            processed_samples: Vec::new(),
        };
        recompute_demo_cache(&mut buffers);
        Mutex::new(buffers)
    })
}

fn decode_bundled_example() -> Vec<f32> {
    use rodio::Source;
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/audio/funDemo.mp3"
    ));
    let Ok(decoder) = rodio::Decoder::new(Cursor::new(bytes.as_slice())) else {
        return vec![0.0; HEX_MICROPHONE_SAMPLE_RATE as usize * 5];
    };
    let channels = usize::from(decoder.channels().get()).max(1);
    let sr = decoder.sample_rate().get();
    let raw: Vec<f32> = decoder.collect();
    let mut mono = Vec::with_capacity(raw.len() / channels);
    for frame in raw.chunks_exact(channels) {
        let sum: f32 = frame.iter().copied().sum();
        mono.push(sum / (channels as f32));
    }
    if sr == HEX_MICROPHONE_SAMPLE_RATE || mono.is_empty() {
        return mono;
    }
    let ratio = f64::from(HEX_MICROPHONE_SAMPLE_RATE) / f64::from(sr);
    let out_len = ((mono.len() as f64) * ratio).round() as usize;
    (0..out_len)
        .map(|i| {
            let pos = (i as f64) / ratio;
            let idx0 = (pos.floor() as usize).min(mono.len() - 1);
            let idx1 = (idx0 + 1).min(mono.len() - 1);
            let frac = (pos - (idx0 as f64)) as f32;
            mono[idx0] + (mono[idx1] - mono[idx0]) * frac
        })
        .collect()
}

fn span_for_source(source: DemoSourceKind, sample_len: usize) -> DictationSpan {
    let duration_ms = (sample_len as f64) * 1000.0 / f64::from(HEX_MICROPHONE_SAMPLE_RATE);
    match source {
        DemoSourceKind::Example => DictationSpan {
            start_ms: 5921.0_f64.min(duration_ms),
            end_ms: 10231.0_f64.min(duration_ms),
        },
        DemoSourceKind::Recording => DictationSpan {
            start_ms: (duration_ms * 0.24).round(),
            end_ms: (duration_ms * 0.78).round(),
        },
    }
}

fn compute_peaks(samples: &[f32], bars: usize) -> Vec<f32> {
    if samples.is_empty() || bars == 0 {
        return vec![0.0; bars];
    }
    let step = (samples.len() as f64) / (bars as f64);
    (0..bars)
        .map(|i| {
            let a = ((i as f64) * step).floor() as usize;
            let b = (((i + 1) as f64) * step).ceil() as usize;
            let end = b.min(samples.len()).max(a + 1);
            samples[a.min(samples.len() - 1)..end]
                .iter()
                .fold(0.0_f32, |acc, &v| acc.max(v.abs()))
        })
        .collect()
}

fn recompute_demo_cache(buffers: &mut DemoBuffers) {
    let cfg = current_tuned_config();
    let active_samples = buffers.active_samples();
    let span = span_for_source(buffers.source, active_samples.len());
    let spec = cfg.build_mode_spec();
    let processed_samples = crate::audio::voice_filter::render(
        active_samples,
        f64::from(HEX_MICROPHONE_SAMPLE_RATE),
        &spec,
        &[span],
        f64::from(cfg.strength),
    );
    let peaks_you = compute_peaks(active_samples, WAVEFORM_BARS);
    let peaks_call = compute_peaks(&processed_samples, WAVEFORM_BARS);
    buffers.processed_samples = processed_samples;
    buffers.peaks_you = peaks_you;
    buffers.peaks_call = peaks_call;
    buffers.cached_mode = Some(cfg);
    buffers.cached_source = Some(buffers.source);
}

fn append_demo_recording_samples(samples: &[f32]) {
    let Ok(mut guard) = demo_buffers_lock().try_lock() else {
        return;
    };
    if !DEMO_RECORDING_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let remaining = DEMO_RECORD_MAX_SAMPLES.saturating_sub(guard.recorded_samples.len());
    let take = remaining.min(samples.len());
    guard.recorded_samples.extend_from_slice(&samples[..take]);
    DEMO_RECORDED_LEN.store(guard.recorded_samples.len(), Ordering::Relaxed);
    if guard.recorded_samples.len() >= DEMO_RECORD_MAX_SAMPLES {
        DEMO_RECORDING_ACTIVE.store(false, Ordering::Release);
        guard.source = DemoSourceKind::Recording;
        guard.cached_mode = None;
        recompute_demo_cache(&mut guard);
    }
}

pub fn start_demo_recording() {
    stop_demo_playback();
    if let Ok(mut guard) = demo_buffers_lock().lock() {
        guard.recorded_samples.clear();
        DEMO_RECORDED_LEN.store(0, Ordering::Relaxed);
        DEMO_RECORDING_START_NS.store(demo_now_ns(), Ordering::Release);
        DEMO_RECORDING_ACTIVE.store(true, Ordering::Release);
    }
}

pub fn stop_demo_recording() {
    if !DEMO_RECORDING_ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }
    if let Ok(mut guard) = demo_buffers_lock().lock()
        && guard.recorded_samples.len() >= (HEX_MICROPHONE_SAMPLE_RATE as usize / 4)
    {
        guard.source = DemoSourceKind::Recording;
        guard.cached_mode = None;
        recompute_demo_cache(&mut guard);
    }
}

pub fn select_demo_source(source: DemoSourceKind) {
    stop_demo_playback();
    if let Ok(mut guard) = demo_buffers_lock().lock() {
        if source == DemoSourceKind::Recording && guard.recorded_samples.is_empty() {
            return;
        }
        guard.source = source;
    }
    schedule_demo_recompute();
}

pub fn stop_demo_playback() {
    if DEMO_PLAYING_TRACK.load(Ordering::Acquire) != 0 {
        let start = DEMO_PLAYBACK_START_NS.load(Ordering::Acquire);
        let dur = DEMO_PLAYBACK_DURATION_NS.load(Ordering::Acquire);
        if dur > 0 {
            let elapsed = demo_now_ns().saturating_sub(start);
            let p = ((elapsed as f64) / (dur as f64)).clamp(0.0, 1.0) as f32;
            DEMO_PLAYBACK_PROGRESS_BITS.store(p.to_bits(), Ordering::Release);
        }
    }
    DEMO_PLAYBACK_GEN.fetch_add(1, Ordering::AcqRel);
    DEMO_PLAYING_TRACK.store(0, Ordering::Release);
}

pub fn play_demo_track(track: DemoPlaybackTrack) {
    stop_demo_recording();
    let samples = {
        let Ok(mut guard) = demo_buffers_lock().lock() else {
            return;
        };
        let cfg = current_tuned_config();
        if guard.cached_mode.as_ref() != Some(&cfg) || guard.cached_source != Some(guard.source) {
            recompute_demo_cache(&mut guard);
        }
        match track {
            DemoPlaybackTrack::CallHears => guard.processed_samples.clone(),
            DemoPlaybackTrack::You => guard.active_samples().to_vec(),
        }
    };
    if samples.is_empty() {
        return;
    }

    let generation = DEMO_PLAYBACK_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    let track_code = match track {
        DemoPlaybackTrack::CallHears => 1,
        DemoPlaybackTrack::You => 2,
    };
    let duration =
        Duration::from_secs_f64((samples.len() as f64) / f64::from(HEX_MICROPHONE_SAMPLE_RATE));
    DEMO_PLAYBACK_START_NS.store(demo_now_ns(), Ordering::Release);
    DEMO_PLAYBACK_DURATION_NS.store(duration.as_nanos() as u64, Ordering::Release);
    DEMO_PLAYBACK_PROGRESS_BITS.store(0.0_f32.to_bits(), Ordering::Release);
    DEMO_PLAYING_TRACK.store(track_code, Ordering::Release);

    thread::spawn(move || {
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            if DEMO_PLAYBACK_GEN.load(Ordering::Acquire) == generation {
                DEMO_PLAYING_TRACK.store(0, Ordering::Release);
            }
            return;
        };
        sink.log_on_drop(false);
        let Some(ch) = std::num::NonZeroU16::new(1) else {
            return;
        };
        let Some(sr) = std::num::NonZeroU32::new(HEX_MICROPHONE_SAMPLE_RATE) else {
            return;
        };
        let buffer = rodio::buffer::SamplesBuffer::new(ch, sr, samples);
        sink.mixer().add(buffer);
        DEMO_PLAYBACK_START_NS.store(demo_now_ns(), Ordering::Release);
        let started = Instant::now();
        while started.elapsed() < duration {
            if DEMO_PLAYBACK_GEN.load(Ordering::Acquire) != generation {
                return;
            }
            thread::sleep(Duration::from_millis(16));
        }
        if DEMO_PLAYBACK_GEN.load(Ordering::Acquire) == generation {
            DEMO_PLAYBACK_PROGRESS_BITS.store(1.0_f32.to_bits(), Ordering::Release);
            DEMO_PLAYING_TRACK.store(0, Ordering::Release);
        }
    });
}

pub fn demo_snapshot() -> DemoSnapshot {
    let cfg = current_tuned_config();
    let is_recording = DEMO_RECORDING_ACTIVE.load(Ordering::Acquire);
    let recording_progress = if is_recording {
        let start = DEMO_RECORDING_START_NS.load(Ordering::Acquire);
        let max_ns = (DEMO_RECORD_MAX_SAMPLES as u64) * 1_000_000_000
            / u64::from(HEX_MICROPHONE_SAMPLE_RATE);
        if max_ns > 0 && start > 0 {
            let elapsed = demo_now_ns().saturating_sub(start);
            ((elapsed as f64) / (max_ns as f64)).clamp(0.0, 1.0) as f32
        } else {
            let recorded_len = DEMO_RECORDED_LEN.load(Ordering::Relaxed);
            ((recorded_len as f32) / (DEMO_RECORD_MAX_SAMPLES as f32)).clamp(0.0, 1.0)
        }
    } else {
        1.0
    };
    let playing = match DEMO_PLAYING_TRACK.load(Ordering::Acquire) {
        1 => Some(DemoPlaybackTrack::CallHears),
        2 => Some(DemoPlaybackTrack::You),
        _ => None,
    };
    let playback_progress = if playing.is_some() {
        let start = DEMO_PLAYBACK_START_NS.load(Ordering::Acquire);
        let dur = DEMO_PLAYBACK_DURATION_NS.load(Ordering::Acquire);
        if dur > 0 {
            let elapsed = demo_now_ns().saturating_sub(start);
            let p = ((elapsed as f64) / (dur as f64)).clamp(0.0, 1.0) as f32;
            DEMO_PLAYBACK_PROGRESS_BITS.store(p.to_bits(), Ordering::Release);
            p
        } else {
            f32::from_bits(DEMO_PLAYBACK_PROGRESS_BITS.load(Ordering::Acquire))
        }
    } else {
        f32::from_bits(DEMO_PLAYBACK_PROGRESS_BITS.load(Ordering::Acquire))
    };

    let Ok(mut guard) = demo_buffers_lock().lock() else {
        return DemoSnapshot {
            source: DemoSourceKind::Example,
            has_recording: false,
            is_recording,
            recording_progress,
            playing,
            playback_progress,
            duration_ms: 14_237,
            span: DictationSpan {
                start_ms: 5921.0,
                end_ms: 10231.0,
            },
            peaks_you: vec![0.2; WAVEFORM_BARS],
            peaks_call: vec![0.1; WAVEFORM_BARS],
        };
    };

    if guard.cached_mode.is_none() {
        recompute_demo_cache(&mut guard);
    } else if guard.cached_mode.as_ref() != Some(&cfg) || guard.cached_source != Some(guard.source)
    {
        schedule_demo_recompute();
    }

    let sample_len = guard.active_samples().len();
    let duration_ms =
        ((sample_len as f64) * 1000.0 / f64::from(HEX_MICROPHONE_SAMPLE_RATE)).round() as u32;
    let span = span_for_source(guard.source, sample_len);

    DemoSnapshot {
        source: guard.source,
        has_recording: !guard.recorded_samples.is_empty(),
        is_recording,
        recording_progress,
        playing,
        playback_progress,
        duration_ms,
        span,
        peaks_you: guard.peaks_you.clone(),
        peaks_call: guard.peaks_call.clone(),
    }
}

// ── macOS CoreAudio HAL Sink Client ─────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos_hal;

#[cfg(target_os = "linux")]
mod pipewire_source;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_transfers_samples_and_handles_underflow_and_overflow() {
        let ring = HexMicRingBuffer::new();
        let input: Vec<f32> = (0..512).map(|i| (i as f32) / 512.0).collect();
        ring.push_slice(&input);
        assert_eq!(ring.queued_frames(), 512);

        let mut out = vec![0.0_f32; 512];
        let copied = ring.pop_slice(&mut out);
        assert_eq!(copied, 512);
        assert_eq!(out, input);
        assert_eq!(ring.queued_frames(), 0);

        // Underflow zero-fills cleanly without blocking
        let mut under = vec![1.0_f32; 128];
        let copied_under = ring.pop_slice(&mut under);
        assert_eq!(copied_under, 0);
        assert!(under.iter().all(|&s| s == 0.0));
        assert!(ring.underflows() >= 1);

        // Latency cap skips ahead when more than MAX_RING_LATENCY_FRAMES are queued
        let big = vec![0.25_f32; MAX_RING_LATENCY_FRAMES * 4];
        ring.push_slice(&big);
        let mut pull = vec![0.0_f32; 256];
        let pulled = ring.pop_slice(&mut pull);
        assert_eq!(pulled, 256);
        assert!(
            ring.queued_frames() <= MAX_RING_LATENCY_FRAMES,
            "expected ring buffer to bound latency to <= {MAX_RING_LATENCY_FRAMES}, got {}",
            ring.queued_frames()
        );
    }

    struct PassThrough {
        underflows: u64,
        overflows: u64,
        /// Output samples that do not continue the input ramp by one.
        discontinuities: usize,
    }

    /// Runs capture streams on one microphone (each callback delivering
    /// `capture_frames` of a 48 kHz ramp to every feed) against a sink pulling
    /// `sink_frames`, with small deterministic jitter, and checks the output
    /// after the first 100 ms continues the ramp.
    fn simulate_pass_through(
        feeds: &mut [CaptureFeed],
        capture_frames: usize,
        sink_frames: usize,
        seconds: f64,
    ) -> PassThrough {
        let ring = HexMicRingBuffer::new();
        let rate = f64::from(HEX_MICROPHONE_SAMPLE_RATE);
        let capture_period = capture_frames as f64 / rate;
        let sink_period = sink_frames as f64 / rate;
        let (mut capture_at, mut sink_at) = (0.003, 0.0);
        let mut seed = 1_u64;
        let mut jitter = || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 11) as f64) / ((1_u64 << 53) as f64) * 0.000_5
        };
        let mut next = 1.0_f32;
        let mut out = vec![0.0_f32; sink_frames];
        let mut last: Option<f32> = None;
        let mut warm = None;
        let mut discontinuities = 0;
        while sink_at < seconds {
            if capture_at + jitter() <= sink_at + jitter() {
                let block: Vec<f32> = (0..capture_frames).map(|i| next + i as f32).collect();
                next += capture_frames as f32;
                for feed in feeds.iter_mut() {
                    feed.push_to(Some(&ring), false, &block);
                }
                capture_at += capture_period;
            } else {
                ring.pop_slice(&mut out);
                sink_at += sink_period;
                if sink_at < 0.1 {
                    continue;
                }
                warm.get_or_insert(ring.counters());
                for &sample in &out {
                    if last.is_some_and(|last| sample != last + 1.0) {
                        discontinuities += 1;
                    }
                    last = Some(sample);
                }
            }
        }
        let (start, end) = (warm.unwrap(), ring.counters());
        PassThrough {
            underflows: end.0 - start.0,
            overflows: end.1 - start.1,
            discontinuities,
        }
    }

    fn switched_on_feed(sample_rate: u32) -> CaptureFeed {
        let (feed, switch) = capture_feed(sample_rate);
        switch.set(true);
        feed
    }

    #[test]
    fn a_meeting_stream_on_the_same_microphone_does_not_double_the_feed() {
        let listener = switched_on_feed(48_000);
        let (meeting, meeting_switch) = capture_feed(48_000);
        assert!(!meeting_switch.is_on(), "streams start switched off");

        let result = simulate_pass_through(&mut [listener, meeting], 512, 256, 10.0);

        assert_eq!(result.underflows, 0);
        assert_eq!(result.overflows, 0);
        assert_eq!(result.discontinuities, 0);

        // Both feeding, as every stream did before, repeats each callback and
        // skips ahead about 90 times a second.
        let doubled = simulate_pass_through(
            &mut [switched_on_feed(48_000), switched_on_feed(48_000)],
            512,
            256,
            10.0,
        );
        assert!(doubled.overflows > 500 && doubled.discontinuities > 500);
    }

    #[test]
    fn a_callback_larger_than_the_latency_cap_passes_through_whole() {
        // A 24 kHz Bluetooth headset delivers 480 frames, 960 at 48 kHz, per callback.
        let result = simulate_pass_through(&mut [switched_on_feed(48_000)], 960, 256, 10.0);

        assert_eq!(result.underflows, 0);
        assert_eq!(result.overflows, 0);
        assert_eq!(result.discontinuities, 0);
    }

    #[test]
    fn switching_to_a_smaller_callback_microphone_lowers_the_latency_cap() {
        let ring = HexMicRingBuffer::new();
        let (mut headset, headset_switch) = capture_feed(48_000);
        let (mut built_in, built_in_switch) = capture_feed(48_000);
        headset_switch.set(true);
        for _ in 0..5 {
            headset.push_to(Some(&ring), false, &[0.1; 960]);
        }
        assert_eq!(ring.producer_burst(), 960);

        headset_switch.set(false);
        built_in_switch.set(true);
        headset.push_to(Some(&ring), false, &[0.1; 960]);
        built_in.push_to(Some(&ring), false, &[0.1; 480]);
        assert_eq!(ring.producer_burst(), 480);

        // 1,200 queued frames now exceed the cap (1,120 for a 256-frame pull);
        // with the headset's burst it was 1,600.
        let mut out = [0.0_f32; 256];
        ring.pop_slice(&mut [0.0_f32; RING_CAPACITY / 2]);
        ring.push_slice(&[0.1; 1_200]);
        let (_, overflows) = ring.counters();
        ring.pop_slice(&mut out);
        assert_eq!(ring.counters().1, overflows + 1);
    }

    #[test]
    fn one_coalesced_callback_does_not_widen_the_cap_for_good() {
        let ring = HexMicRingBuffer::new();
        let mut feed = switched_on_feed(48_000);
        feed.push_to(Some(&ring), false, &[0.1; 480]);
        feed.push_to(Some(&ring), false, &[0.1; MAX_PRODUCER_BURST_FRAMES]);
        assert_eq!(ring.producer_burst(), MAX_PRODUCER_BURST_FRAMES);

        for _ in 0..BURST_DECAY_CALLBACKS {
            feed.push_to(Some(&ring), false, &[0.1; 480]);
        }
        assert!(ring.producer_burst() < MAX_PRODUCER_BURST_FRAMES / 2);
        for _ in 0..10 * BURST_DECAY_CALLBACKS {
            feed.push_to(Some(&ring), false, &[0.1; 480]);
        }
        assert_eq!(ring.producer_burst(), 480);
    }

    #[test]
    fn resampling_is_continuous_across_callbacks() {
        let tone: Vec<f32> = (0..24_000)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 24_000.0).sin())
            .collect();
        let resample = |block: usize| {
            let mut resampler = LinearResampler::new(24_000);
            let mut out = Vec::new();
            for chunk in tone.chunks(block) {
                resampler.process(chunk, |frames| out.extend_from_slice(frames));
            }
            out
        };
        let whole = resample(tone.len());
        let callbacks = resample(480);
        assert_eq!(callbacks.len(), whole.len());
        let worst = whole
            .iter()
            .zip(&callbacks)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1e-5,
            "block boundaries changed the signal by {worst}"
        );

        // 44.1 kHz blocks do not resample to a whole number of 48 kHz frames;
        // rounding each callback would drift the clock by hundreds of ppm.
        let mut resampler = LinearResampler::new(44_100);
        let block = vec![0.0_f32; 512];
        let frames: usize = (0..441 * 10)
            .map(|_| resampler.process(&block, |_| {}))
            .sum();
        let expected = 512 * 441 * 10 * 48_000 / 44_100;
        assert!(
            frames.abs_diff(expected) <= 1,
            "expected {expected} frames, got {frames}"
        );
    }

    #[test]
    fn virtual_device_names_are_excluded_from_self_capture() {
        assert!(is_hex_virtual_device_name("Hex Microphone"));
        assert!(is_hex_virtual_device_name("Hex Microphone (Internal Sink)"));
        assert!(is_hex_virtual_device_name("hex microphone"));
        assert!(!is_hex_virtual_device_name("MacBook Pro Microphone"));
        assert!(!is_hex_virtual_device_name("Shure MV7"));
    }

    #[test]
    fn demo_snapshot_computes_finite_peaks_for_bundled_example() {
        let snap = demo_snapshot();
        assert_eq!(snap.source, DemoSourceKind::Example);
        assert_eq!(snap.peaks_you.len(), WAVEFORM_BARS);
        assert_eq!(snap.peaks_call.len(), WAVEFORM_BARS);
        assert!(snap.peaks_you.iter().any(|&p| p > 0.05));
        assert!(snap.peaks_call.iter().any(|&p| p > 0.02));
    }

    #[test]
    fn driver_plist_version_and_status_mismatch_are_detected() {
        let plist = include_str!("../../macos/HexMicrophoneDriver/Info.plist");
        assert_eq!(
            plist_string_value(plist, "CFBundleIdentifier").as_deref(),
            Some(EXPECTED_DRIVER_BUNDLE_ID)
        );
        // The build script stamps the version, so the source plist's value
        // never ships; `driver_plugin_loopback_*` checks the built bundle.

        let mismatch_status = HexMicrophoneStatus {
            driver_bundle_installed: true,
            driver_loaded: true,
            installed_version: Some("1.0.0".into()),
            bundled_version: env!("CARGO_PKG_VERSION").into(),
            sink_streaming: true,
            mic_in_use: false,
            active_apps: Vec::new(),
            action_state: DriverActionState::Idle,
        };
        assert!(mismatch_status.needs_update());
        assert!(!mismatch_status.is_up_to_date());

        let current_status = HexMicrophoneStatus {
            installed_version: Some(env!("CARGO_PKG_VERSION").into()),
            ..mismatch_status
        };
        assert!(!current_status.needs_update());
        assert!(current_status.is_up_to_date());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn osascript_failure_classifies_user_cancel_and_errors() {
        assert_eq!(
            classify_osascript_failure("36:112: execution error: User canceled. (-128)", ""),
            DriverActionState::Cancelled
        );
        assert_eq!(
            classify_osascript_failure("0:42: execution error: Permission denied (1)", ""),
            DriverActionState::Failed("Permission denied (1)".into())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn driver_plugin_loopback_streams_at_48khz_under_20ms_latency() {
        use std::ffi::{CString, c_void};
        use std::process::Command;

        let out_dir =
            std::env::temp_dir().join(format!("hex-mic-driver-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/build-hex-microphone-driver.sh");
        let status = Command::new("/bin/sh")
            .arg(&script)
            .arg(&out_dir)
            .status()
            .expect("run build-hex-microphone-driver.sh");
        assert!(status.success(), "driver build script must succeed");

        let dylib_path = out_dir.join("HexMicrophone.driver/Contents/MacOS/HexMicrophoneDriver");
        assert!(dylib_path.exists(), "built driver binary must exist");
        assert_eq!(
            read_driver_bundle_version(&out_dir.join("HexMicrophone.driver")).as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
            "the build script stamps the crate version into the driver"
        );

        #[repr(C)]
        struct DriverInterfacePrefix {
            _reserved: *mut c_void,
            query_interface: *const c_void,
            add_ref: *const c_void,
            release: *const c_void,
            initialize: unsafe extern "C" fn(*mut *mut DriverInterfacePrefix, *const c_void) -> i32,
            create_device: *const c_void,
            destroy_device: *const c_void,
            add_device_client: *const c_void,
            remove_device_client: *const c_void,
            perform_device_configuration_change: *const c_void,
            abort_device_configuration_change: *const c_void,
            has_property: *const c_void,
            is_property_settable: *const c_void,
            get_property_data_size: *const c_void,
            get_property_data: *const c_void,
            set_property_data: *const c_void,
            start_io: unsafe extern "C" fn(*mut *mut DriverInterfacePrefix, u32, u32) -> i32,
            stop_io: unsafe extern "C" fn(*mut *mut DriverInterfacePrefix, u32, u32) -> i32,
            get_zero_time_stamp: *const c_void,
            will_do_io_operation: *const c_void,
            begin_io_operation: *const c_void,
            do_io_operation: unsafe extern "C" fn(
                *mut *mut DriverInterfacePrefix,
                u32,
                u32,
                u32,
                u32,
                u32,
                *const c_void,
                *mut c_void,
                *mut c_void,
            ) -> i32,
            end_io_operation: *const c_void,
        }

        let c_path = CString::new(dylib_path.to_str().unwrap()).unwrap();
        let handle = unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        assert!(!handle.is_null(), "dlopen HexMicrophone must succeed");

        let sym_name = CString::new("HexMicrophoneDriverCreate").unwrap();
        let sym = unsafe { libc::dlsym(handle, sym_name.as_ptr()) };
        assert!(
            !sym.is_null(),
            "HexMicrophoneDriverCreate symbol must exist"
        );

        // StartIO on Sink (4) and Mic (2), write 256 frames (5.33 ms at 48 kHz), read 256 frames.
        // Verify bit-exact transfer and < 20 ms latency.
        unsafe extern "C" {
            fn CFUUIDGetConstantUUIDWithBytes(
                alloc: *const c_void,
                b0: u8,
                b1: u8,
                b2: u8,
                b3: u8,
                b4: u8,
                b5: u8,
                b6: u8,
                b7: u8,
                b8: u8,
                b9: u8,
                b10: u8,
                b11: u8,
                b12: u8,
                b13: u8,
                b14: u8,
                b15: u8,
            ) -> *const c_void;
        }
        let create_fn: unsafe extern "C" fn(
            *const c_void,
            *const c_void,
        ) -> *mut *mut DriverInterfacePrefix = unsafe { std::mem::transmute(sym) };
        let type_uuid = unsafe {
            CFUUIDGetConstantUUIDWithBytes(
                std::ptr::null(),
                0x44,
                0x3A,
                0xBA,
                0xB8,
                0xE7,
                0xB3,
                0x49,
                0x1A,
                0xB9,
                0x85,
                0xBE,
                0xB9,
                0x18,
                0x70,
                0x30,
                0xDB,
            )
        };
        let driver_ref = unsafe { create_fn(std::ptr::null(), type_uuid) };
        assert!(!driver_ref.is_null());
        let vtable = unsafe { &**driver_ref };
        assert_eq!(
            unsafe { (vtable.initialize)(driver_ref, std::ptr::null()) },
            0
        );
        // kObjectID_MicDevice = 2 (stream 3), kObjectID_SinkDevice = 4 (stream 5)
        assert_eq!(unsafe { (vtable.start_io)(driver_ref, 4, 0) }, 0);
        assert_eq!(unsafe { (vtable.start_io)(driver_ref, 2, 0) }, 0);

        const IO_OP_WRITE_MIX: u32 = u32::from_be_bytes(*b"rite");
        const IO_OP_READ_INPUT: u32 = u32::from_be_bytes(*b"read");

        let frames = 256u32; // 5.33 ms at 48 kHz (< 20 ms requirement)
        let input: Vec<f32> = (0..frames)
            .map(|i| ((i as f32) * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin() * 0.5)
            .collect();
        let mut output = vec![0.0_f32; frames as usize];

        let write_rc = unsafe {
            (vtable.do_io_operation)(
                driver_ref,
                4,
                5,
                0,
                IO_OP_WRITE_MIX,
                frames,
                std::ptr::null(),
                input.as_ptr() as *mut c_void,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(write_rc, 0);

        let read_rc = unsafe {
            (vtable.do_io_operation)(
                driver_ref,
                2,
                3,
                0,
                IO_OP_READ_INPUT,
                frames,
                std::ptr::null(),
                output.as_mut_ptr().cast::<c_void>(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(read_rc, 0);
        assert_eq!(
            output, input,
            "driver loopback must transfer 256 frames bit-for-bit (5.33 ms latency)"
        );

        let _ = unsafe { (vtable.stop_io)(driver_ref, 2, 0) };
        let _ = unsafe { (vtable.stop_io)(driver_ref, 4, 0) };
        let _ = unsafe { libc::dlclose(handle) };
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
