use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::process::{Child, Command};
use std::ptr::NonNull;
use std::sync::mpsc::{self, Sender};
use std::thread;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};

use crate::app_settings::{self, RecordingAudioBehavior};

const VIRTUAL_MAIN_VOLUME: u32 = u32::from_be_bytes(*b"vmvc");
const PAUSE_MUSIC: &str = r#"
try
  if application "Music" is running then
    tell application "Music"
      if player state is playing then
        pause
        set end of pausedPlayers to "Music"
      end if
    end tell
  end if
end try
"#;
const PAUSE_SPOTIFY: &str = r#"
try
  if application "Spotify" is running then
    tell application "Spotify"
      if player state is playing then
        pause
        set end of pausedPlayers to "Spotify"
      end if
    end tell
  end if
end try
"#;
const PAUSE_VLC: &str = r#"
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
"#;

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
}

#[derive(Clone)]
pub struct RecordingEnvironmentController {
    commands: Sender<EnvironmentCommand>,
}

impl RecordingEnvironmentController {
    pub fn start() -> Self {
        let (commands, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut sessions = 0_u32;
            let mut _environment = None;
            while let Ok(command) = receiver.recv() {
                match command {
                    EnvironmentCommand::Start => {
                        sessions = sessions.saturating_add(1);
                        if sessions == 1 {
                            _environment = Some(RecordingEnvironment::start());
                        }
                    }
                    EnvironmentCommand::Stop => {
                        sessions = sessions.saturating_sub(1);
                        if sessions == 0 {
                            _environment = None;
                        }
                    }
                }
            }
        });
        Self { commands }
    }

    pub fn begin(&self) -> RecordingEnvironmentSession {
        let active = self.commands.send(EnvironmentCommand::Start).is_ok();
        RecordingEnvironmentSession {
            commands: self.commands.clone(),
            active,
        }
    }
}

pub struct RecordingEnvironmentSession {
    commands: Sender<EnvironmentCommand>,
    active: bool,
}

impl Drop for RecordingEnvironmentSession {
    fn drop(&mut self) {
        if self.active {
            let _ = self.commands.send(EnvironmentCommand::Stop);
        }
    }
}

pub fn prevent_sleep() -> Option<PreventSleep> {
    if !app_settings::prevent_system_sleep() {
        return None;
    }
    match PreventSleep::start() {
        Ok(prevention) => Some(prevention),
        Err(error) => {
            tracing::warn!(%error, "could not prevent idle system sleep");
            None
        }
    }
}

pub struct PreventSleep {
    process: Child,
}

impl PreventSleep {
    fn start() -> std::io::Result<Self> {
        Ok(Self {
            process: Command::new("/usr/bin/caffeinate")
                .args(["-i", "-w"])
                .arg(std::process::id().to_string())
                .spawn()?,
        })
    }
}

impl Drop for PreventSleep {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

enum AudioBehaviorGuard {
    Muted { device: u32, previous: f32 },
    Paused { players: Vec<String> },
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

fn pause_media() -> Vec<String> {
    let mut script = format!("set pausedPlayers to {{}}\n{PAUSE_MUSIC}");
    if Path::new("/Applications/Spotify.app").exists() {
        script.push_str(PAUSE_SPOTIFY);
    }
    if Path::new("/Applications/VLC.app").exists() {
        script.push_str(PAUSE_VLC);
    }
    script.push_str("return pausedPlayers\n");
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
        .filter(|player| matches!(*player, "Music" | "Spotify" | "VLC"))
        .map(str::to_string)
        .collect()
}

fn resume_media(players: &[String]) {
    let script = players
        .iter()
        .filter_map(|player| match player.as_str() {
            "Music" => {
                Some("if application \"Music\" is running then tell application \"Music\" to play")
            }
            "Spotify" => Some(
                "if application \"Spotify\" is running then tell application \"Spotify\" to play",
            ),
            "VLC" => {
                Some("if application \"VLC\" is running then tell application \"VLC\" to play")
            }
            _ => None,
        })
        .map(|command| format!("try\n  {command}\nend try"))
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
