//! Resolving the configured microphone and opening it: `--device`, the saved
//! microphone, else the system default, with the stand-in rules that keep Hex
//! Microphone, a lid-closed built-in microphone, or a Bluetooth headset from
//! recording the wrong input.

use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::Device;
use cpal::traits::{DeviceTrait, HostTrait};

use super::call_input;
use super::fixture;
use super::routes::avoid_bluetooth_microphone;
use super::transport::{Transport, transport};
use super::{AudioInput, Opened, Selection};

pub(super) type InputOpenResult = (u64, Result<Opened, String>);

/// What an open resolves: `--device`, the saved microphone, and the open
/// device's name, which is kept rather than reopened when it resolves again.
pub(super) struct OpenRequest {
    pub(super) device_override: Option<String>,
    pub(super) selected_device: Option<String>,
    pub(super) fallback: SelectedFallback,
    pub(super) keep: Option<String>,
    /// Whether System default passes over a Bluetooth default when the
    /// setting asks. Dictation does; a meeting records the input the call
    /// is using, so the headset a call app holds is never swapped out.
    pub(super) avoids_bluetooth: bool,
    /// The UID of the input a meeting's call app records from, tried first.
    pub(super) call_input: Option<String>,
    /// Whether the open makes the device renegotiate its sample rate first: a
    /// silence recovery's device can stay on a stale clock (#1575).
    pub(super) renegotiate_rate: bool,
}

impl OpenRequest {
    /// The route this open resolves against.
    pub(super) fn route(&self, current: Route) -> Route {
        Route {
            avoid_bluetooth: current.avoid_bluetooth && self.avoids_bluetooth,
            ..current
        }
    }
}

/// An on-demand open retries the selected microphone briefly before falling
/// back, because a device that has idled for minutes can fail its first open.
const SELECTED_MICROPHONE_ATTEMPTS: usize = 3;
const SELECTED_MICROPHONE_RETRY_DELAY: Duration = Duration::from_millis(100);

/// How an open treats a saved microphone that is listed but cannot be
/// opened. Either way the system default stands in, and the saved
/// microphone is tried again with backoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectedFallback {
    /// Startup and meetings fall back at once.
    Immediate,
    /// Other opens retry briefly first.
    AfterRetries,
    /// Never: only the named input opens (a Screenshots recording's own).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Never,
}

/// Which input an open resolves to, before anything opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InputTarget {
    /// `--device` or the saved microphone, by its index in the list.
    Named(usize),
    /// The OS's default input.
    SystemDefault,
    /// A real input standing in for a default that is Hex Microphone, the
    /// built-in microphone with the lid closed, a passed-over Bluetooth
    /// headset, or missing: the built-in microphone while the lid is open,
    /// else the first usable one listed, never a virtual input or an
    /// iPhone's Continuity microphone.
    StandIn(usize),
}

/// The state of the machine an input resolves against.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Route {
    /// A MacBook's lid is closed, so its built-in microphone is off.
    pub(super) lid_closed: bool,
    /// System default passes over a Bluetooth default while another real
    /// input is listed.
    pub(super) avoid_bluetooth: bool,
}

pub(crate) const LID_CLOSED_ERROR: &str = "the built-in microphone is off while the lid is closed";
pub(crate) const NO_MICROPHONE_ERROR: &str = "no microphone is available";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InputPlan {
    pub(super) target: InputTarget,
    pub(super) selection: Selection,
}

/// Resolves the input Hex captures from: `--device` exactly as named, else
/// the saved microphone while it is listed, else the system default. Hex
/// never captures from its own virtual microphone, so a default that is Hex
/// Microphone (or none) falls back to a real input. With the lid closed, the
/// built-in microphone is switched off in hardware though still listed, so
/// it is passed over the same way. With `avoid_bluetooth`, a Bluetooth
/// default is passed over for the built-in microphone, else the first other
/// real input, and kept only when nothing else is usable. Saved microphones
/// and `--device` are never passed over for being Bluetooth. A virtual or
/// aggregate input, or an iPhone's Continuity microphone, never stands in,
/// though such a default, saved microphone, or `--device` is used as named.
pub(super) fn resolve_input(
    device_override: Option<&str>,
    selected: Option<&str>,
    names: &[String],
    default: Option<&str>,
    transport: impl Fn(usize) -> Transport,
    route: Route,
) -> std::result::Result<InputPlan, String> {
    let is_hex = crate::audio::hex_microphone::is_hex_virtual_device_name;
    let position = |name: &str| names.iter().position(|listed| listed == name);
    let is_built_in = |index: usize| transport(index) == Transport::BuiltIn;
    let avoided = |index: usize| route.avoid_bluetooth && transport(index) == Transport::Bluetooth;
    let off = |index: usize| route.lid_closed && is_built_in(index);
    if let Some(name) = device_override {
        if is_hex(name) {
            return Err(format!(
                "Hex cannot capture from its own virtual microphone: {name}"
            ));
        }
        return position(name)
            .map(|index| InputPlan {
                target: InputTarget::Named(index),
                selection: Selection::InUse,
            })
            .ok_or_else(|| format!("microphone is unavailable: {name}"));
    }
    let selection = match selected.filter(|name| !is_hex(name)) {
        Some(name) => match position(name) {
            Some(index) if off(index) => Selection::LidClosed,
            Some(index) => {
                return Ok(InputPlan {
                    target: InputTarget::Named(index),
                    selection: Selection::InUse,
                });
            }
            None => Selection::Unlisted,
        },
        None => Selection::InUse,
    };
    // ALSA names a default even with no devices listed.
    let default_usable = !names.is_empty()
        && default.is_some_and(|name| !is_hex(name) && !position(name).is_some_and(off));
    let default_avoided = default.and_then(position).is_some_and(avoided);
    if default_usable && !default_avoided {
        return Ok(InputPlan {
            target: InputTarget::SystemDefault,
            selection,
        });
    }
    // A virtual device or an iPhone's Continuity microphone is used only as
    // the default or by name, never as a stand-in: a virtual one can record
    // silence while its app is idle, and the iPhone prompts and can fail.
    let usable = || {
        (0..names.len()).filter(|&index| {
            !is_hex(&names[index])
                && !off(index)
                && !matches!(transport(index), Transport::Virtual | Transport::Continuity)
        })
    };
    usable()
        .find(|&index| is_built_in(index))
        .or_else(|| usable().find(|&index| !avoided(index)))
        .map(InputTarget::StandIn)
        // A Bluetooth default beats recording nothing.
        .or_else(|| default_usable.then_some(InputTarget::SystemDefault))
        .or_else(|| usable().next().map(InputTarget::StandIn))
        .map(|target| InputPlan { target, selection })
        .ok_or_else(|| {
            if (0..names.len()).any(off) {
                LID_CLOSED_ERROR.into()
            } else {
                NO_MICROPHONE_ERROR.into()
            }
        })
}

pub(super) fn current_route() -> Route {
    Route {
        lid_closed: lid_closed(),
        avoid_bluetooth: avoid_bluetooth_microphone(),
    }
}

/// Whether a MacBook's lid is closed, which switches its built-in microphone
/// off in hardware while CoreAudio still lists it, often as the default
/// input. A microphone in the headphone jack stays on. Read from
/// `AppleClamshellState` on IOKit's power root domain; Macs without a lid,
/// and other OSes, read open.
#[cfg(not(target_os = "macos"))]
fn lid_closed() -> bool {
    false
}

#[cfg(target_os = "macos")]
use super::lid::closed as lid_closed;

/// The input devices whose names can be read, with those names. A device
/// without a readable name, such as CoreAudio's placeholder when no input
/// exists, cannot be selected or opened, so it is left out.
pub(super) fn input_devices_of(host: &cpal::Host) -> Result<(Vec<Device>, Vec<String>)> {
    Ok(host
        .input_devices()
        .wrap_err("could not enumerate input devices")?
        .filter_map(|device| device_name(&device).map(|name| (device, name)))
        .unzip())
}

/// The system default input with its name, or `None` when there is none or
/// its name cannot be read.
pub(super) fn default_input_of(host: &cpal::Host) -> Option<(Device, String)> {
    let device = host.default_input_device()?;
    let name = device_name(&device)?;
    Some((device, name))
}

/// The device's name. cpal's `Display` for `Device` returns `fmt::Error`
/// when the name cannot be read, which makes `to_string` panic, so read the
/// description directly.
pub(super) fn device_name(device: &Device) -> Option<String> {
    description_name(device.description())
}

pub(super) fn description_name<E: std::fmt::Display>(
    description: std::result::Result<cpal::DeviceDescription, E>,
) -> Option<String> {
    match description {
        Ok(description) => Some(description.name().to_owned()),
        Err(error) => {
            tracing::debug!(%error, "skipping an input device whose name cannot be read");
            None
        }
    }
}

/// Resolves and opens the input, or keeps the open one when it resolves to
/// the same device. A listed saved microphone that will not open falls back
/// to the system default.
pub(super) fn open_configured_input(request: &OpenRequest) -> Result<Opened> {
    if let Some(path) = fixture::path() {
        if request.keep.as_deref() == Some(fixture::NAME) {
            return Ok(Opened::Unchanged {
                selection: Selection::InUse,
            });
        }
        return Ok(Opened::Input(Box::new(fixture::open(path)?)));
    }
    let host = cpal::default_host();
    let (devices, names) = input_devices_of(&host)?;
    let (default, default_name) = default_input_of(&host).unzip();
    let transports = devices.iter().map(transport).collect::<Vec<_>>();
    let route = request.route(current_route());
    let resolve = |selected: Option<&str>| {
        resolve_input(
            request.device_override.as_deref(),
            selected,
            &names,
            default_name.as_deref(),
            |index| transports[index],
            route,
        )
        .map_err(|error| eyre!(error))
    };
    // Keeping the open device returns before logging, so a periodic look
    // for a missing microphone logs only when the input changes.
    let open = |plan: InputPlan| -> Result<Opened> {
        let (device, name, follows_default) = match plan.target {
            InputTarget::Named(index) | InputTarget::StandIn(index) => {
                (devices[index].clone(), names[index].clone(), false)
            }
            InputTarget::SystemDefault => (
                default.clone().expect("resolved default"),
                default_name.clone().expect("resolved default"),
                true,
            ),
        };
        if request.keep.as_deref() == Some(name.as_str()) {
            return Ok(Opened::Unchanged {
                selection: plan.selection,
            });
        }
        if let InputTarget::StandIn(_) = plan.target {
            tracing::warn!(
                default = default_name.as_deref().unwrap_or("none"),
                using = %name,
                lid_closed = route.lid_closed,
                avoid_bluetooth = route.avoid_bluetooth,
                "the system default input is Hex Microphone, off with the lid closed, Bluetooth, or missing; using another microphone"
            );
        }
        match plan.selection {
            Selection::InUse => {}
            Selection::Unlisted => tracing::info!(
                selected = request.selected_device.as_deref(),
                "the saved microphone is not connected; using the system default"
            ),
            Selection::Unopenable => tracing::warn!(
                selected = request.selected_device.as_deref(),
                "the saved microphone would not open; using the system default"
            ),
            Selection::LidClosed => tracing::info!(
                selected = request.selected_device.as_deref(),
                "the saved microphone is off while the lid is closed; using the system default"
            ),
        }
        let mut input =
            AudioInput::open_device(device, name, follows_default, request.renegotiate_rate)?;
        input.selection = plan.selection;
        // The ALSA default cannot name another PipeWire source, so Linux
        // moves the opened stream off a Bluetooth default instead.
        #[cfg(target_os = "linux")]
        if follows_default && route.avoid_bluetooth {
            crate::linux::input::route::move_streams_off_bluetooth();
        }
        Ok(Opened::Input(Box::new(input)))
    };
    if let Some(index) =
        (request.call_input.as_deref()).and_then(|uid| call_input::listed_at(uid, &devices, &names))
    {
        let target = InputTarget::Named(index);
        match open(InputPlan {
            target,
            selection: Selection::InUse,
        }) {
            Ok(opened) => return Ok(opened),
            Err(error) => tracing::warn!(%error, "the input the call uses would not open"),
        }
    }
    let plan = resolve(request.selected_device.as_deref())?;
    if request.fallback == SelectedFallback::Never && !matches!(plan.target, InputTarget::Named(_))
    {
        return Err(eyre!("the microphone isn't connected"));
    }
    let (Some(selected), InputTarget::Named(_), None) = (
        request.selected_device.as_deref(),
        plan.target,
        request.device_override.as_deref(),
    ) else {
        return open(plan);
    };
    open_selected(
        selected,
        request.fallback,
        || open(plan),
        || {
            let mut fallback = resolve(None)?;
            fallback.selection = Selection::Unopenable;
            open(fallback)
        },
        std::thread::sleep,
    )
}

/// Opens `device` alone, or fails: never the system default or another
/// input in its place, so nothing else is opened on the way.
#[cfg(target_os = "macos")]
pub fn open_named_input(device: &str) -> Result<AudioInput> {
    let opened = open_configured_input(&OpenRequest {
        device_override: None,
        selected_device: Some(device.to_owned()),
        fallback: SelectedFallback::Never,
        keep: None,
        avoids_bluetooth: false,
        call_input: None,
        renegotiate_rate: false,
    })?;
    match opened {
        Opened::Input(input) if input.device_name == device => Ok(*input),
        Opened::Input(input) => Err(eyre!(
            "{device} wouldn't open ({} stood in)",
            input.device_name
        )),
        Opened::Unchanged { .. } => Err(eyre!("{device} resolved to no new input")),
    }
}

/// Opens the input dictation would open now: the saved microphone, else the
/// system default, passing over Hex Microphone and (as set) a Bluetooth
/// default. For a level meter while dictation's own is closed.
#[cfg(target_os = "macos")]
pub fn open_dictation_input(selected_device: Option<&str>) -> Result<AudioInput> {
    match open_configured_input(&OpenRequest {
        device_override: None,
        selected_device: selected_device.map(str::to_owned),
        fallback: SelectedFallback::Immediate,
        keep: None,
        avoids_bluetooth: true,
        call_input: None,
        renegotiate_rate: false,
    })? {
        Opened::Input(input) => Ok(*input),
        Opened::Unchanged { .. } => Err(eyre!("nothing was open to keep")),
    }
}

/// Opens the microphone a meeting records: the input its call app records
/// from (`call_input`, a device UID) when that opens, else one resolved like
/// dictation's but keeping a Bluetooth default a call app on a headset uses;
/// or keeps the open one named `keep` when it resolves to that again.
/// `renegotiate_rate` marks a silence recovery's open, like dictation's.
pub fn open_meeting_input(
    selected_device: Option<&str>,
    call_input: Option<&str>,
    keep: Option<&str>,
    renegotiate_rate: bool,
) -> Result<Opened> {
    open_configured_input(&OpenRequest {
        device_override: None,
        selected_device: selected_device.map(str::to_owned),
        fallback: SelectedFallback::Immediate,
        keep: keep.map(str::to_owned),
        avoids_bluetooth: false,
        call_input: call_input.map(str::to_owned),
        renegotiate_rate,
    })
}

pub(super) fn open_selected<T>(
    device: &str,
    fallback: SelectedFallback,
    mut open_selected: impl FnMut() -> Result<T>,
    open_default: impl FnOnce() -> Result<T>,
    mut sleep: impl FnMut(Duration),
) -> Result<T> {
    let attempts = if fallback == SelectedFallback::AfterRetries {
        SELECTED_MICROPHONE_ATTEMPTS
    } else {
        1
    };
    let mut attempt = 1;
    let error = loop {
        match open_selected() {
            Ok(input) => return Ok(input),
            Err(error) if attempt >= attempts => break error,
            Err(error) => {
                let error = format!("{error:#}");
                tracing::debug!(%error, device, attempt, "could not open the selected microphone; retrying");
                attempt += 1;
                sleep(SELECTED_MICROPHONE_RETRY_DELAY);
            }
        }
    };
    if fallback == SelectedFallback::Never {
        return Err(error);
    }
    let error = format!("{error:#}");
    tracing::debug!(%error, device, "the saved microphone would not open");
    open_default()
}

#[cfg(test)]
mod tests;
