use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::process::Command;
use std::ptr::NonNull;
use std::sync::mpsc::{self, Sender};
use std::thread;

use objc2_app_kit::NSWorkspace;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};
use objc2_core_foundation::CFString;

use crate::app_settings::{self, RecordingAudioBehavior};

const VIRTUAL_MAIN_VOLUME: u32 = u32::from_be_bytes(*b"vmvc");
const POWER_ASSERTION_LEVEL_ON: u32 = 255;

/// A media player HEX may pause for dictation and resume afterwards.
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

struct RecordingEnvironment {
    _sleep: Option<PreventSleep>,
    _audio: AudioBehaviorGuard,
}

impl RecordingEnvironment {
    pub fn start() -> Self {
        Self {
            _sleep: prevent_sleep(),
            _audio: AudioBehaviorGuard::start(app_settings::recording_audio_behavior()),
        }
    }
}

enum EnvironmentCommand {
    Start,
    Stop,
    #[cfg(test)]
    Barrier(Sender<()>),
}

#[derive(Clone)]
pub struct RecordingEnvironmentController {
    commands: Sender<EnvironmentCommand>,
}

impl RecordingEnvironmentController {
    pub fn start() -> Self {
        Self::with_environment(RecordingEnvironment::start)
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::with_environment(|| ())
    }

    fn with_environment<E>(start: impl Fn() -> E + Send + 'static) -> Self {
        let (commands, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut sessions = 0_u32;
            let mut _environment = None;
            while let Ok(command) = receiver.recv() {
                match command {
                    EnvironmentCommand::Start => {
                        sessions = sessions.saturating_add(1);
                        if sessions == 1 {
                            _environment = Some(start());
                        }
                    }
                    EnvironmentCommand::Stop => {
                        sessions = sessions.saturating_sub(1);
                        if sessions == 0 {
                            _environment = None;
                        }
                    }
                    #[cfg(test)]
                    EnvironmentCommand::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });
        Self { commands }
    }

    pub fn begin(&self) -> RecordingEnvironmentSession {
        let _ = self.commands.send(EnvironmentCommand::Start);
        RecordingEnvironmentSession {
            commands: self.commands.clone(),
        }
    }
}

pub struct RecordingEnvironmentSession {
    commands: Sender<EnvironmentCommand>,
}

impl Drop for RecordingEnvironmentSession {
    fn drop(&mut self) {
        let _ = self.commands.send(EnvironmentCommand::Stop);
    }
}

pub fn prevent_sleep() -> Option<PreventSleep> {
    match PreventSleep::start() {
        Ok(prevention) => Some(prevention),
        Err(error) => {
            tracing::warn!(%error, "could not prevent idle system sleep");
            None
        }
    }
}

pub struct PreventSleep {
    assertion_id: u32,
}

impl PreventSleep {
    fn start() -> std::io::Result<Self> {
        let assertion_type = CFString::from_static_str("NoIdleSleepAssertion");
        let assertion_name = CFString::from_static_str("HEX intentional recording");
        let mut assertion_id = 0;
        // SAFETY: Both Core Foundation strings remain alive for the call and
        // assertion_id points to writable, correctly sized storage. IOKit
        // retains the assertion independently until IOPMAssertionRelease.
        let status = unsafe {
            IOPMAssertionCreateWithName(
                (&*assertion_type as *const CFString).cast(),
                POWER_ASSERTION_LEVEL_ON,
                (&*assertion_name as *const CFString).cast(),
                &mut assertion_id,
            )
        };
        if status != 0 {
            return Err(std::io::Error::other(format!(
                "IOPMAssertionCreateWithName failed with IOReturn 0x{:08x}",
                status as u32
            )));
        }
        Ok(Self { assertion_id })
    }
}

impl Drop for PreventSleep {
    fn drop(&mut self) {
        // SAFETY: assertion_id was returned by a successful create call and
        // this guard is its sole owner, so release occurs exactly once.
        let status = unsafe { IOPMAssertionRelease(self.assertion_id) };
        if status != 0 {
            tracing::warn!(
                status = format_args!("0x{:08x}", status as u32),
                "could not release idle-sleep assertion"
            );
        }
    }
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        assertion_type: *const c_void,
        assertion_level: u32,
        assertion_name: *const c_void,
        assertion_id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(assertion_id: u32) -> i32;
}

enum AudioBehaviorGuard {
    Muted { device: u32, previous: f32 },
    Paused { players: Vec<MediaPlayer> },
    None,
}

impl AudioBehaviorGuard {
    fn start(behavior: RecordingAudioBehavior) -> Self {
        match behavior {
            RecordingAudioBehavior::Mute => {
                mute_output().map_or(Self::None, |(device, previous)| {
                    tracing::info!(previous, "muted system output for dictation");
                    Self::Muted { device, previous }
                })
            }
            RecordingAudioBehavior::PauseMedia => {
                let players = pause_media();
                if players.is_empty() {
                    Self::None
                } else {
                    tracing::info!(?players, "paused media for dictation");
                    Self::Paused { players }
                }
            }
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
                    tracing::info!(volume = *previous, "restored system output after dictation");
                }
            }
            Self::Paused { players } => resume_media(players),
            Self::None => {}
        }
    }
}

fn mute_output() -> Option<(u32, f32)> {
    let device = default_output_device()?;
    let previous = output_volume(device)?;
    set_output_volume(device, 0.0).then_some((device, previous))
}

fn default_output_device() -> Option<u32> {
    let mut address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size = size_of::<u32>() as u32;
    let mut device = 0_u32;
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as u32,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut device).cast::<c_void>(),
        )
    };
    (status == 0 && device != 0).then_some(device)
}

fn output_volume(device: u32) -> Option<f32> {
    let mut address = volume_address();
    let mut size = size_of::<f32>() as u32;
    let mut volume = 0.0_f32;
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut volume).cast::<c_void>(),
        )
    };
    (status == 0).then_some(volume)
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
            "could not pause media for dictation"
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
            tracing::info!(?players, "resumed media after dictation")
        }
        Ok(output) => tracing::warn!(
            error = %String::from_utf8_lossy(&output.stderr).trim(),
            "could not resume media after dictation"
        ),
        Err(error) => tracing::warn!(%error, "could not resume media after dictation"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::Duration;

    #[derive(Debug, PartialEq)]
    enum Event {
        Started,
        Restored,
    }

    struct ObservedEnvironment(Sender<Event>);

    impl Drop for ObservedEnvironment {
        fn drop(&mut self) {
            let _ = self.0.send(Event::Restored);
        }
    }

    fn observed_controller() -> (RecordingEnvironmentController, Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        let controller = RecordingEnvironmentController::with_environment(move || {
            let _ = events.send(Event::Started);
            ObservedEnvironment(events.clone())
        });
        (controller, receiver)
    }

    fn assert_events(
        commands: &Sender<EnvironmentCommand>,
        events: &Receiver<Event>,
        expected: &[Event],
    ) {
        let (reply, response) = mpsc::channel();
        commands.send(EnvironmentCommand::Barrier(reply)).unwrap();
        response.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(events.try_iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn sessions_are_harmless_after_the_environment_worker_disconnects() {
        let (commands, receiver) = mpsc::channel();
        drop(receiver);
        let controller = RecordingEnvironmentController { commands };
        drop(controller.begin());
    }

    #[test]
    #[ignore = "exercises the native macOS idle-sleep assertion"]
    fn native_idle_sleep_assertion_acquires_and_releases() {
        drop(PreventSleep::start().unwrap());
    }

    #[test]
    fn overlapping_sessions_restore_only_after_the_last_session() {
        let (controller, events) = observed_controller();
        assert_events(&controller.commands, &events, &[]);

        let first = controller.begin();
        let second = controller.begin();
        assert_events(&controller.commands, &events, &[Event::Started]);

        drop(first);
        assert_events(&controller.commands, &events, &[]);

        let third = controller.begin();
        drop(second);
        assert_events(&controller.commands, &events, &[]);

        drop(third);
        assert_events(&controller.commands, &events, &[Event::Restored]);
    }

    #[test]
    fn a_new_session_reacquires_the_environment_after_restoration() {
        let (controller, events) = observed_controller();
        for _ in 0..2 {
            let session = controller.begin();
            assert_events(&controller.commands, &events, &[Event::Started]);
            drop(session);
            assert_events(&controller.commands, &events, &[Event::Restored]);
        }
    }

    #[test]
    fn controller_moves_and_clones_do_not_restore_a_live_session() {
        let (controller, events) = observed_controller();
        let session = controller.begin();
        assert_events(&controller.commands, &events, &[Event::Started]);

        let cloned = controller.clone();
        let moved = controller;
        drop(moved);
        assert_events(&cloned.commands, &events, &[]);

        let overlapping = cloned.begin();
        drop(cloned);
        assert_events(&session.commands, &events, &[]);

        drop(session);
        assert_events(&overlapping.commands, &events, &[]);

        drop(overlapping);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Restored
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
