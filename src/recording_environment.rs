use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::process::Command;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use objc2_app_kit::NSWorkspace;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};

use crate::app_settings::{self, RecordingAudioBehavior};
use crate::browser_media::{self, NativeBrowserAudio, PausedBrowser};

mod ducking;

const VIRTUAL_MAIN_VOLUME: u32 = u32::from_be_bytes(*b"vmvc");

/// A media player Hex may pause while recording and resume afterwards.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum MediaPlayer {
    Music,
    Spotify,
    Vlc,
}

impl MediaPlayer {
    const ALL: [Self; 3] = [Self::Music, Self::Spotify, Self::Vlc];

    fn name(self) -> &'static str {
        match self {
            Self::Music => "Music",
            Self::Spotify => "Spotify",
            Self::Vlc => "VLC",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|player| player.name() == name)
    }

    fn from_bundle_id(bundle_id: &str) -> Option<Self> {
        match bundle_id {
            "com.apple.Music" => Some(Self::Music),
            "com.spotify.client" => Some(Self::Spotify),
            "org.videolan.vlc" => Some(Self::Vlc),
            _ => None,
        }
    }

    /// The AppleScript condition that is true while the player is playing.
    fn playing_clause(self) -> &'static str {
        match self {
            Self::Music | Self::Spotify => "player state is playing",
            Self::Vlc => "playing",
        }
    }

    fn pause_fragment(self) -> String {
        let name = self.name();
        let playing = self.playing_clause();
        format!(
            "\ntry\n  if application \"{name}\" is running then\n    tell application \"{name}\"\n      if {playing} then\n        pause\n        set end of pausedPlayers to \"{name}\"\n      end if\n    end tell\n  end if\nend try\n"
        )
    }

    fn resume_fragment(self) -> String {
        let name = self.name();
        format!(
            "try\n  if application \"{name}\" is running then tell application \"{name}\" to play\nend try"
        )
    }
}

/// One thing the environment changes while a session wants it. Each is
/// held from the first session that wants it until the last one ends, so
/// overlapping sessions with different policies compose and none restores
/// what another still needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Sleep,
    Mute,
    Duck,
    Pause,
}

impl Effect {
    /// Restoration order: output is audible again before a session's
    /// callback, and resuming media (AppleScript) never delays it.
    const ALL: [Self; 4] = [Self::Sleep, Self::Mute, Self::Duck, Self::Pause];

    const fn index(self) -> usize {
        self as usize
    }
}

/// What one session asks for: dictation prevents idle sleep and applies
/// "While dictating"; a capture applies "While dictating" only.
#[derive(Clone, Copy, Debug)]
struct Hold {
    dictation: bool,
    audio: RecordingAudioBehavior,
}

impl Hold {
    fn wants(self, effect: Effect) -> bool {
        match effect {
            Effect::Sleep => self.dictation,
            Effect::Mute => self.audio == RecordingAudioBehavior::Mute,
            Effect::Duck => self.audio == RecordingAudioBehavior::Duck,
            Effect::Pause => self.audio == RecordingAudioBehavior::PauseMedia,
        }
    }
}

/// What the native environment holds for one effect until it is dropped.
enum Held {
    Sleep {
        _guard: Option<crate::idle_sleep::PreventSleep>,
    },
    Audio {
        _guard: AudioBehaviorGuard,
    },
}

fn hold_native(effect: Effect) -> Held {
    let audio = |behavior| Held::Audio {
        _guard: AudioBehaviorGuard::start(behavior),
    };
    match effect {
        Effect::Sleep => Held::Sleep {
            _guard: crate::idle_sleep::prevent_sleep(),
        },
        Effect::Mute => audio(RecordingAudioBehavior::Mute),
        Effect::Duck => audio(RecordingAudioBehavior::Duck),
        Effect::Pause => audio(RecordingAudioBehavior::PauseMedia),
    }
}

type Callback = Box<dyn FnOnce() + Send + Sync>;

enum EnvironmentCommand {
    Start(Hold),
    /// Stops, then runs the callback, if any, once a muted output is
    /// audible again.
    Stop(Hold, Option<Callback>),
    #[cfg(test)]
    Barrier(Sender<()>),
}

#[derive(Clone)]
pub struct RecordingEnvironmentController {
    commands: Sender<EnvironmentCommand>,
}

/// Environment threads that may still have output to unmute or media to
/// resume. Each ends once its controller and every session are dropped.
static RESTORERS: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// Waits, up to `deadline`, for every recording environment to restore
/// output and media after its listener stopped. Quitting calls this before
/// `_exit`, which would otherwise leave the output muted.
pub fn wait_until_restored(deadline: Duration) -> bool {
    let started = Instant::now();
    let mut restorers = RESTORERS.lock().unwrap_or_else(|error| error.into_inner());
    loop {
        restorers.retain(|restorer| !restorer.is_finished());
        if restorers.is_empty() {
            return true;
        }
        if started.elapsed() >= deadline {
            tracing::warn!("Hex quit before restoring output or media after recording");
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

impl RecordingEnvironmentController {
    pub fn start() -> Self {
        Self::restoring(hold_native)
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::with_environment(|_| ()).0
    }

    fn restoring<H: 'static>(hold: impl Fn(Effect) -> H + Send + 'static) -> Self {
        let (controller, restorer) = Self::with_environment(hold);
        let mut restorers = RESTORERS.lock().unwrap_or_else(|error| error.into_inner());
        restorers.retain(|restorer| !restorer.is_finished());
        restorers.push(restorer);
        controller
    }

    /// The thread that owns every held effect. `hold` changes the
    /// environment for one effect; dropping what it returns restores it.
    fn with_environment<H: 'static>(
        hold: impl Fn(Effect) -> H + Send + 'static,
    ) -> (Self, JoinHandle<()>) {
        let (commands, receiver) = mpsc::channel();
        let restorer = thread::spawn(move || {
            let mut sessions = [0_u32; Effect::ALL.len()];
            let mut held: [Option<H>; Effect::ALL.len()] = std::array::from_fn(|_| None);
            while let Ok(command) = receiver.recv() {
                match command {
                    EnvironmentCommand::Start(wanted) => {
                        for effect in Effect::ALL.into_iter().filter(|e| wanted.wants(*e)) {
                            let count = &mut sessions[effect.index()];
                            *count = count.saturating_add(1);
                            if *count == 1 {
                                held[effect.index()] = Some(hold(effect));
                            }
                        }
                    }
                    EnvironmentCommand::Stop(wanted, mut then) => {
                        for effect in Effect::ALL {
                            if effect == Effect::Pause
                                && let Some(then) = then.take()
                            {
                                then();
                            }
                            if !wanted.wants(effect) {
                                continue;
                            }
                            let count = &mut sessions[effect.index()];
                            *count = count.saturating_sub(1);
                            if *count == 0 {
                                held[effect.index()] = None;
                            }
                        }
                        if let Some(then) = then {
                            then();
                        }
                    }
                    #[cfg(test)]
                    EnvironmentCommand::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });
        (Self { commands }, restorer)
    }

    /// A dictation session: prevents idle sleep and applies "While
    /// dictating" until the session ends.
    pub fn begin(&self) -> RecordingEnvironmentSession {
        self.begin_as(Hold {
            dictation: true,
            audio: app_settings::recording_audio_behavior(),
        })
    }

    /// A capture session: applies `audio` ("While dictating") until the
    /// session ends. It is not a dictation, so it neither prevents sleep
    /// nor holds back meeting transcription.
    #[cfg(any(hex_capture, test))]
    pub fn begin_capture(&self, audio: RecordingAudioBehavior) -> RecordingEnvironmentSession {
        self.begin_as(Hold {
            dictation: false,
            audio,
        })
    }

    fn begin_as(&self, hold: Hold) -> RecordingEnvironmentSession {
        if hold.dictation {
            INTENTIONAL_RECORDINGS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        let _ = self.commands.send(EnvironmentCommand::Start(hold));
        RecordingEnvironmentSession {
            commands: self.commands.clone(),
            hold,
            then: None,
        }
    }
}

pub struct RecordingEnvironmentSession {
    commands: Sender<EnvironmentCommand>,
    hold: Hold,
    then: Option<Callback>,
}

impl RecordingEnvironmentSession {
    /// Ends the session and runs `then` once a muted output is audible
    /// again; while another session keeps it muted, or with paused media,
    /// it runs at once.
    pub fn end_then(mut self, then: impl FnOnce() + Send + Sync + 'static) {
        self.then = Some(Box::new(then));
    }
}

impl Drop for RecordingEnvironmentSession {
    fn drop(&mut self) {
        if self.hold.dictation {
            INTENTIONAL_RECORDINGS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
        let command = EnvironmentCommand::Stop(self.hold, self.then.take());
        if let Err(mpsc::SendError(EnvironmentCommand::Stop(_, Some(then)))) =
            self.commands.send(command)
        {
            then();
        }
    }
}

static INTENTIONAL_RECORDINGS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Whether an intentional dictation capture is in progress.
pub fn intentional_recording_active() -> bool {
    INTENTIONAL_RECORDINGS.load(std::sync::atomic::Ordering::Acquire) > 0
}

static CAPTURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Marks a dictation capture from its press until its clip is submitted, so
/// background work such as meeting transcription stops starting new jobs
/// before the capture can need the model.
pub struct CaptureMarker(());

impl CaptureMarker {
    pub fn new() -> Self {
        CAPTURES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self(())
    }
}

impl Drop for CaptureMarker {
    fn drop(&mut self) {
        CAPTURES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Whether a dictation capture has started, intentional or not, and its clip
/// is not yet submitted.
pub fn capture_active() -> bool {
    CAPTURES.load(std::sync::atomic::Ordering::Acquire) > 0
}

enum AudioBehaviorGuard {
    Ducked(crate::output_ducking::Guard),
    Muted {
        device: u32,
        previous: f32,
    },
    Paused {
        players: Vec<MediaPlayer>,
        browser: Option<PausedBrowser>,
    },
    None,
}

/// Only with the permission already granted: a tap without it opens the
/// macOS dialog, and a dialog must never open while you dictate.
fn browser_pause_allowed(
    enabled: bool,
    authorization: crate::meeting::SystemAudioAuthorization,
) -> bool {
    enabled
        && matches!(
            authorization,
            crate::meeting::SystemAudioAuthorization::Granted
        )
}

fn paused_guard(players: Vec<MediaPlayer>, browser: Option<PausedBrowser>) -> AudioBehaviorGuard {
    if players.is_empty() && browser.is_none() {
        AudioBehaviorGuard::None
    } else {
        AudioBehaviorGuard::Paused { players, browser }
    }
}

impl AudioBehaviorGuard {
    fn start(behavior: RecordingAudioBehavior) -> Self {
        match behavior {
            RecordingAudioBehavior::Mute => {
                mute_output().map_or(Self::None, |(device, previous)| {
                    tracing::info!(previous, "muted system output while recording");
                    Self::Muted { device, previous }
                })
            }
            RecordingAudioBehavior::PauseMedia => {
                let players = pause_media();
                if !players.is_empty() {
                    tracing::info!(?players, "paused media while recording");
                }
                let browser = browser_pause_allowed(
                    app_settings::pause_browser_media(),
                    crate::meeting::system_audio_authorization(),
                )
                .then(|| browser_media::pause(&mut NativeBrowserAudio::for_pause()))
                .flatten();
                if browser.is_some() {
                    tracing::info!("paused the browser while recording");
                }
                paused_guard(players, browser)
            }
            RecordingAudioBehavior::Duck => ducking::start(app_settings::recording_duck_volume())
                .map_or(Self::None, Self::Ducked),
            RecordingAudioBehavior::DoNothing => Self::None,
        }
    }
}

impl Drop for AudioBehaviorGuard {
    fn drop(&mut self) {
        match self {
            Self::Muted { device, previous } => {
                if output_volume(*device).is_some_and(|volume| volume <= 0.001)
                    && set_output_volume(*device, *previous)
                {
                    tracing::info!(volume = *previous, "restored system output after recording");
                }
            }
            Self::Ducked(guard) => guard.restore(),
            Self::Paused { players, browser } => {
                // The browser first: its play goes to whatever macOS treats as
                // playing, which a player resumed just before could become.
                if let Some(paused) = browser.take() {
                    browser_media::resume(&mut NativeBrowserAudio::for_resume(), paused);
                }
                resume_media(players);
            }
            Self::None => {}
        }
    }
}

fn mute_output() -> Option<(u32, f32)> {
    let device = default_output_device()?;
    let previous = output_volume(device)?;
    set_output_volume(device, 0.0).then_some((device, previous))
}

fn read_property<T: Copy + Default>(
    object: u32,
    mut address: AudioObjectPropertyAddress,
) -> Option<T> {
    let mut size = size_of::<T>() as u32;
    let mut value = T::default();
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    };
    (status == 0 && size as usize == size_of::<T>()).then_some(value)
}

fn default_output_device() -> Option<u32> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    read_property::<u32>(kAudioObjectSystemObject as u32, address).filter(|&device| device != 0)
}

fn output_volume(device: u32) -> Option<f32> {
    read_property(device, volume_address())
}

fn set_output_volume(device: u32, volume: f32) -> bool {
    let mut address = volume_address();
    let mut volume = volume;
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    unsafe {
        AudioObjectSetPropertyData(
            device,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            size_of::<f32>() as u32,
            NonNull::from(&mut volume).cast::<c_void>(),
        ) == 0
    }
}

fn volume_address() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: VIRTUAL_MAIN_VOLUME,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn pause_media() -> Vec<MediaPlayer> {
    let running = running_media_players();
    if running.is_empty() {
        return Vec::new();
    }
    let script = pause_media_script(&running);
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        tracing::warn!(
            error = %String::from_utf8_lossy(&output.stderr).trim(),
            "could not pause media while recording"
        );
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .split(',')
        .map(str::trim)
        .filter_map(MediaPlayer::from_name)
        .collect()
}

fn resume_media(players: &[MediaPlayer]) {
    let running = running_media_players();
    let script = players
        .iter()
        .filter(|player| running.contains(player))
        .map(|player| player.resume_fragment())
        .collect::<Vec<_>>()
        .join("\n");
    if script.is_empty() {
        return;
    }
    match Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
    {
        Ok(output) if output.status.success() => {
            tracing::info!(?players, "resumed media after recording")
        }
        Ok(output) => tracing::warn!(
            error = %String::from_utf8_lossy(&output.stderr).trim(),
            "could not resume media after recording"
        ),
        Err(error) => tracing::warn!(%error, "could not resume media after recording"),
    }
}

fn running_media_players() -> HashSet<MediaPlayer> {
    objc2::rc::autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter_map(|application| {
                MediaPlayer::from_bundle_id(&application.bundleIdentifier()?.to_string())
            })
            .collect()
    })
}

fn pause_media_script(players: &HashSet<MediaPlayer>) -> String {
    let mut script = String::from("set pausedPlayers to {}\n");
    for player in MediaPlayer::ALL {
        if players.contains(&player) {
            script.push_str(&player.pause_fragment());
        }
    }
    script.push_str("return pausedPlayers\n");
    script
}

/// A controller whose effects are only recorded, for tests here and in
/// the capture host. Nothing touches output, media, or sleep.
#[cfg(test)]
pub(crate) mod observed {
    use super::*;
    use std::sync::mpsc::Receiver;

    #[derive(Debug, PartialEq)]
    pub(crate) enum Event {
        Held(Effect),
        Restored(Effect),
        Then,
    }

    pub(crate) struct Observed(Effect, Sender<Event>);

    impl Drop for Observed {
        fn drop(&mut self) {
            let _ = self.1.send(Event::Restored(self.0));
        }
    }

    pub(crate) fn controller() -> (RecordingEnvironmentController, Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        (reporting(events), receiver)
    }

    /// A controller that reports to `events`.
    pub(crate) fn reporting(events: Sender<Event>) -> RecordingEnvironmentController {
        RecordingEnvironmentController::with_environment(move |effect| {
            let _ = events.send(Event::Held(effect));
            Observed(effect, events.clone())
        })
        .0
    }

    /// What the environment did since the last call, once every command
    /// sent before it was handled.
    pub(crate) fn settled(
        controller: &RecordingEnvironmentController,
        events: &Receiver<Event>,
    ) -> Vec<Event> {
        let (reply, response) = mpsc::channel();
        controller
            .commands
            .send(EnvironmentCommand::Barrier(reply))
            .unwrap();
        response.recv_timeout(Duration::from_secs(2)).unwrap();
        events.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::observed::{Event, controller as observed_controller, settled};
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use Effect::{Mute, Pause, Sleep};
    use Event::{Held, Restored, Then};
    use RecordingAudioBehavior::{DoNothing, Mute as MuteOutput, PauseMedia};

    #[test]
    fn the_browser_pauses_only_with_system_audio_granted() {
        use crate::meeting::SystemAudioAuthorization::{Denied, Granted, NotDetermined, Unknown};
        assert!(browser_pause_allowed(true, Granted));
        for authorization in [Denied, NotDetermined, Unknown] {
            assert!(!browser_pause_allowed(true, authorization));
        }
        assert!(!browser_pause_allowed(false, Granted));
    }

    #[test]
    fn a_pause_with_nothing_paused_holds_nothing() {
        assert!(matches!(
            paused_guard(Vec::new(), None),
            AudioBehaviorGuard::None
        ));
        let guard = paused_guard(Vec::new(), Some(PausedBrowser::for_test()));
        assert!(matches!(guard, AudioBehaviorGuard::Paused { .. }));
        // Dropping it would send a real play.
        std::mem::forget(guard);
    }

    fn dictation(
        controller: &RecordingEnvironmentController,
        audio: RecordingAudioBehavior,
    ) -> RecordingEnvironmentSession {
        controller.begin_as(Hold {
            dictation: true,
            audio,
        })
    }

    #[test]
    fn quitting_waits_for_the_last_session_to_restore() {
        struct SlowRestore(Sender<Effect>, Effect);
        impl Drop for SlowRestore {
            fn drop(&mut self) {
                thread::sleep(Duration::from_millis(100));
                let _ = self.0.send(self.1);
            }
        }
        let (events, restored) = mpsc::channel();
        let controller = RecordingEnvironmentController::restoring(move |effect| {
            SlowRestore(events.clone(), effect)
        });
        let session = controller.begin_capture(MuteOutput);
        // The listener stopping drops both.
        drop(controller);
        drop(session);
        assert!(wait_until_restored(Duration::from_secs(2)));
        assert_eq!(restored.try_recv(), Ok(Mute));
    }

    #[test]
    fn sessions_are_harmless_after_the_environment_worker_disconnects() {
        let (commands, receiver) = mpsc::channel();
        drop(receiver);
        let controller = RecordingEnvironmentController { commands };
        drop(controller.begin_capture(MuteOutput));
        let (ran, called) = mpsc::channel();
        dictation(&controller, MuteOutput).end_then(move || ran.send(()).unwrap());
        assert_eq!(called.try_recv(), Ok(()), "the callback still runs");
    }

    #[test]
    fn overlapping_sessions_restore_only_after_the_last_session() {
        let (controller, events) = observed_controller();
        assert_eq!(settled(&controller, &events), []);

        let first = dictation(&controller, MuteOutput);
        let second = dictation(&controller, MuteOutput);
        assert_eq!(settled(&controller, &events), [Held(Sleep), Held(Mute)]);

        drop(first);
        assert_eq!(settled(&controller, &events), []);

        let third = dictation(&controller, MuteOutput);
        drop(second);
        assert_eq!(settled(&controller, &events), []);

        drop(third);
        assert_eq!(
            settled(&controller, &events),
            [Restored(Sleep), Restored(Mute)]
        );
    }

    #[test]
    fn a_new_session_reacquires_the_environment_after_restoration() {
        let (controller, events) = observed_controller();
        for _ in 0..2 {
            let session = dictation(&controller, DoNothing);
            assert_eq!(settled(&controller, &events), [Held(Sleep)]);
            drop(session);
            assert_eq!(settled(&controller, &events), [Restored(Sleep)]);
        }
    }

    #[test]
    fn a_dictation_inside_a_capture_leaves_the_capture_quiet() {
        let (controller, events) = observed_controller();
        let capture = controller.begin_capture(MuteOutput);
        assert_eq!(settled(&controller, &events), [Held(Mute)]);

        let (then, ran) = mpsc::channel();
        let dictating = dictation(&controller, MuteOutput);
        assert_eq!(settled(&controller, &events), [Held(Sleep)]);
        // The stop tone waits for no unmute: the capture keeps output muted.
        dictating.end_then(move || then.send(Then).unwrap());
        assert_eq!(settled(&controller, &events), [Restored(Sleep)]);
        assert_eq!(ran.try_recv(), Ok(Then));

        drop(capture);
        assert_eq!(settled(&controller, &events), [Restored(Mute)]);
    }

    #[test]
    fn a_capture_inside_a_dictation_outlives_it() {
        let (controller, events) = observed_controller();
        let dictating = dictation(&controller, MuteOutput);
        assert_eq!(settled(&controller, &events), [Held(Sleep), Held(Mute)]);

        let capture = controller.begin_capture(MuteOutput);
        assert_eq!(settled(&controller, &events), []);

        drop(dictating);
        assert_eq!(settled(&controller, &events), [Restored(Sleep)]);

        drop(capture);
        assert_eq!(settled(&controller, &events), [Restored(Mute)]);
    }

    #[test]
    fn a_capture_that_ends_first_leaves_the_dictation_quiet() {
        let (controller, events) = observed_controller();
        let dictating = dictation(&controller, MuteOutput);
        let capture = controller.begin_capture(MuteOutput);
        assert_eq!(settled(&controller, &events), [Held(Sleep), Held(Mute)]);

        drop(capture);
        assert_eq!(settled(&controller, &events), []);

        drop(dictating);
        assert_eq!(
            settled(&controller, &events),
            [Restored(Sleep), Restored(Mute)]
        );
    }

    #[test]
    fn different_policies_compose_and_each_restores_its_own() {
        let (controller, events) = observed_controller();
        let capture = controller.begin_capture(PauseMedia);
        assert_eq!(settled(&controller, &events), [Held(Pause)]);

        let dictating = dictation(&controller, MuteOutput);
        assert_eq!(settled(&controller, &events), [Held(Sleep), Held(Mute)]);
        // The capture keeps media paused.
        drop(dictating);
        assert_eq!(
            settled(&controller, &events),
            [Restored(Sleep), Restored(Mute)]
        );

        drop(capture);
        assert_eq!(settled(&controller, &events), [Restored(Pause)]);
    }

    #[test]
    fn a_callback_runs_after_unmuting_and_before_resuming_media() {
        for (audio, expected) in [
            (MuteOutput, [Restored(Sleep), Restored(Mute), Then]),
            (PauseMedia, [Restored(Sleep), Then, Restored(Pause)]),
        ] {
            let (sender, events) = mpsc::channel();
            let controller = super::observed::reporting(sender.clone());
            let session = dictation(&controller, audio);
            settled(&controller, &events);
            session.end_then(move || sender.send(Then).unwrap());
            assert_eq!(settled(&controller, &events), expected);
        }
    }

    #[test]
    fn controller_moves_and_clones_do_not_restore_a_live_session() {
        let (controller, events) = observed_controller();
        let session = dictation(&controller, DoNothing);
        assert_eq!(settled(&controller, &events), [Held(Sleep)]);

        let cloned = controller.clone();
        let moved = controller;
        drop(moved);
        assert_eq!(settled(&cloned, &events), []);

        let overlapping = dictation(&cloned, DoNothing);
        drop(cloned);
        drop(session);
        let (reply, response) = mpsc::channel();
        overlapping
            .commands
            .send(EnvironmentCommand::Barrier(reply))
            .unwrap();
        response.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(events.try_iter().collect::<Vec<_>>(), []);

        drop(overlapping);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Restored(Sleep)
        );
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn pause_script_never_resolves_players_that_are_not_running() {
        let players = HashSet::from([MediaPlayer::Music, MediaPlayer::Spotify]);

        let script = pause_media_script(&players);

        assert!(script.contains("application \"Music\""));
        assert!(script.contains("application \"Spotify\""));
        assert!(!script.contains("application \"VLC\""));

        let script = pause_media_script(&HashSet::from([MediaPlayer::Vlc]));

        assert_eq!(
            script,
            r#"set pausedPlayers to {}

try
  if application "VLC" is running then
    tell application "VLC"
      if playing then
        pause
        set end of pausedPlayers to "VLC"
      end if
    end tell
  end if
end try
return pausedPlayers
"#
        );
        assert!(!script.contains("player state is playing"));
        assert!(
            pause_media_script(&HashSet::from([MediaPlayer::Music]))
                .contains("if player state is playing then")
        );
    }
}
