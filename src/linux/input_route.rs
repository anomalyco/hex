//! System default on Linux passes over a Bluetooth headset. Hex records
//! through ALSA's default device, which PipeWire (or PulseAudio) routes to
//! its default source and cannot name another one, so when that source is
//! Bluetooth Hex moves its own capture streams to the built-in microphone,
//! else the first other hardware input, passing over any whose active port
//! the sound server reports unplugged (an empty headset jack records only
//! silence). The streams are marked so
//! WirePlumber never remembers the move for the next launch.

use std::process::Command;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Properties every PipeWire ALSA stream Hex opens carries: no remembered
/// target, so a move lasts only for that stream, and Hex's PID to find it.
const STREAM_PROPERTIES: &str = "PIPEWIRE_ALSA";
const PID_PROPERTY: &str = "hex.process.id";
/// How long a new stream has to appear in the sound server.
const STREAM_DEADLINE: Duration = Duration::from_secs(2);
const STREAM_POLL: Duration = Duration::from_millis(50);

/// Marks the PipeWire ALSA streams this process opens. Called on the main
/// thread before Hex starts any other thread.
pub(crate) fn mark_streams() {
    if std::env::var_os(STREAM_PROPERTIES).is_some() {
        return;
    }
    let properties = format!(
        "{{ state.restore-target = false, {PID_PROPERTY} = {} }}",
        std::process::id()
    );
    // SAFETY: No other thread exists yet to read the environment.
    unsafe { std::env::set_var(STREAM_PROPERTIES, properties) };
}

#[derive(Deserialize)]
struct Source {
    index: u32,
    name: String,
    description: String,
    #[serde(default)]
    properties: std::collections::HashMap<String, String>,
    #[serde(default)]
    ports: Vec<Port>,
    #[serde(default)]
    active_port: Option<String>,
}

#[derive(Deserialize)]
struct Port {
    name: String,
    #[serde(default)]
    availability: String,
}

impl Source {
    fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }

    fn is_bluetooth(&self) -> bool {
        self.property("device.api") == Some("bluez5") || self.name.starts_with("bluez_")
    }

    /// A hardware microphone: not a monitor, loopback, filter or virtual
    /// device, which may record silence or Hex itself.
    fn is_hardware_input(&self) -> bool {
        self.property("device.api") == Some("alsa")
            && self.property("device.class") != Some("monitor")
            && !self.name.ends_with(".monitor")
    }

    /// The sound server reports the active port unplugged, such as an empty
    /// headset jack, which records only silence.
    fn is_unplugged(&self) -> bool {
        let Some(active) = self.active_port.as_deref() else {
            return false;
        };
        self.ports
            .iter()
            .any(|port| port.name == active && port.availability == "not available")
    }
}

#[derive(Deserialize)]
struct SourceOutput {
    index: u32,
    source: u32,
    #[serde(default)]
    properties: std::collections::HashMap<String, String>,
}

/// The Bluetooth default source and the input standing in for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StandIn {
    pub(crate) bluetooth: String,
    pub(crate) input: String,
    input_name: String,
    bluetooth_index: u32,
}

/// The stand-in when the default source is Bluetooth and another hardware
/// input exists; `None` otherwise, or without a sound server to ask.
pub(crate) fn bluetooth_stand_in() -> Option<StandIn> {
    let default = pactl(&["get-default-source"])?;
    let sources: Vec<Source> = serde_json::from_str(&pactl(&["-f", "json", "list", "sources"])?)
        .inspect_err(|error| tracing::debug!(%error, "could not read the sound server's sources"))
        .ok()?;
    stand_in_for(default.trim(), &sources)
}

fn stand_in_for(default: &str, sources: &[Source]) -> Option<StandIn> {
    let bluetooth = sources
        .iter()
        .find(|source| source.name == default)
        .filter(|source| source.is_bluetooth())?;
    let usable = || {
        sources.iter().filter(|source| {
            source.is_hardware_input() && !source.is_bluetooth() && !source.is_unplugged()
        })
    };
    let input = usable()
        .find(|source| source.property("device.bus") == Some("pci"))
        .or_else(|| usable().next())?;
    Some(StandIn {
        bluetooth: bluetooth.description.clone(),
        input: input.description.clone(),
        input_name: input.name.clone(),
        bluetooth_index: bluetooth.index,
    })
}

/// When the default source is Bluetooth, moves this process's capture
/// streams on it to the stand-in, in the background: a just-opened stream
/// takes a moment to appear, and capture never waits on the sound server.
pub(crate) fn move_streams_off_bluetooth() {
    let spawned = std::thread::Builder::new()
        .name("input-route".into())
        .spawn(move || {
            let Some(stand_in) = bluetooth_stand_in() else {
                return;
            };
            let started = Instant::now();
            while started.elapsed() < STREAM_DEADLINE {
                let moved = own_streams_on(stand_in.bluetooth_index)
                    .into_iter()
                    .filter(|index| {
                        pactl(&[
                            "move-source-output",
                            &index.to_string(),
                            &stand_in.input_name,
                        ])
                        .is_some()
                    })
                    .count();
                if moved > 0 {
                    tracing::info!(
                        bluetooth = %stand_in.bluetooth,
                        using = %stand_in.input,
                        "the system default input is Bluetooth; using another microphone"
                    );
                    return;
                }
                std::thread::sleep(STREAM_POLL);
            }
            tracing::debug!("no capture stream appeared on the Bluetooth default to move");
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start moving the microphone off Bluetooth");
    }
}

fn own_streams_on(source: u32) -> Vec<u32> {
    let pid = std::process::id().to_string();
    pactl(&["-f", "json", "list", "source-outputs"])
        .and_then(|json| serde_json::from_str::<Vec<SourceOutput>>(&json).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|output| {
            output.source == source
                && output.properties.get(PID_PROPERTY).map(String::as_str) == Some(pid.as_str())
        })
        .map(|output| output.index)
        .collect()
}

fn pactl(args: &[&str]) -> Option<String> {
    let output = Command::new("pactl").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Vec<Source> {
        serde_json::from_str(
            r#"[
              {"index": 1, "name": "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
               "description": "Monitor of Built-in Audio",
               "properties": {"device.api": "alsa", "device.class": "monitor", "device.bus": "pci"}},
              {"index": 2, "name": "alsa_input.usb-Blue_Yeti-00.analog-stereo",
               "description": "Yeti Stereo Microphone",
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "usb"}},
              {"index": 3, "name": "alsa_input.pci-0000_00_1f.3.analog-stereo",
               "description": "Built-in Audio Analog Stereo",
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "pci"}},
              {"index": 4, "name": "bluez_input.AA_BB_CC_DD_EE_FF.0",
               "description": "Kit Pods",
               "properties": {"device.api": "bluez5"}}
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn a_bluetooth_default_stands_in_with_the_built_in_microphone() {
        let stand_in = stand_in_for("bluez_input.AA_BB_CC_DD_EE_FF.0", &sources()).unwrap();
        assert_eq!(stand_in.bluetooth, "Kit Pods");
        assert_eq!(stand_in.input, "Built-in Audio Analog Stereo");
        assert_eq!(stand_in.bluetooth_index, 4);
    }

    #[test]
    fn a_wired_default_or_no_other_input_keeps_the_default() {
        assert_eq!(
            stand_in_for("alsa_input.usb-Blue_Yeti-00.analog-stereo", &sources()),
            None
        );
        let only_bluetooth = sources()
            .into_iter()
            .filter(|source| source.is_bluetooth() || !source.is_hardware_input())
            .collect::<Vec<_>>();
        assert_eq!(
            stand_in_for("bluez_input.AA_BB_CC_DD_EE_FF.0", &only_bluetooth),
            None
        );
    }

    #[test]
    fn without_a_built_in_microphone_the_first_wired_input_stands_in() {
        let no_built_in = sources()
            .into_iter()
            .filter(|source| source.index != 3)
            .collect::<Vec<_>>();
        let stand_in = stand_in_for("bluez_input.AA_BB_CC_DD_EE_FF.0", &no_built_in).unwrap();
        assert_eq!(stand_in.input, "Yeti Stereo Microphone");
    }

    #[test]
    fn an_empty_headset_jack_never_stands_in() {
        let sources: Vec<Source> = serde_json::from_str(
            r#"[
              {"index": 1, "name": "alsa_input.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Mic1__source",
               "description": "Stereo Microphone",
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "pci"},
               "ports": [{"name": "[In] Mic1", "availability": "not available"}],
               "active_port": "[In] Mic1"},
              {"index": 2, "name": "alsa_input.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Mic2__source",
               "description": "Digital Microphone",
               "properties": {"device.api": "alsa", "device.class": "sound", "device.bus": "pci"},
               "ports": [{"name": "[In] Mic2", "availability": "availability unknown"}],
               "active_port": "[In] Mic2"},
              {"index": 3, "name": "bluez_input.AA_BB_CC_DD_EE_FF.0",
               "description": "Kit Pods",
               "properties": {"device.api": "bluez5"}}
            ]"#,
        )
        .unwrap();
        let stand_in = stand_in_for("bluez_input.AA_BB_CC_DD_EE_FF.0", &sources).unwrap();
        assert_eq!(stand_in.input, "Digital Microphone");
        let only_jack = sources
            .into_iter()
            .filter(|source| source.index != 2)
            .collect::<Vec<_>>();
        assert_eq!(
            stand_in_for("bluez_input.AA_BB_CC_DD_EE_FF.0", &only_jack),
            None
        );
    }
}
