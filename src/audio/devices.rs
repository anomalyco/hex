//! Listing the inputs Settings offers, and resolving what System default
//! stands in for.

use color_eyre::eyre::Result;

use super::open::{InputTarget, current_route, default_input_of, input_devices_of, resolve_input};
#[cfg(target_os = "macos")]
use super::routes::watch_input_devices;
use super::transport::{Transport, transport};

/// The inputs Settings offers, without Hex Microphone, and what System
/// default currently resolves to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputDevices {
    pub names: Vec<String>,
    /// The input System default captures from. Linux's ALSA default has no
    /// useful name, so it is `None` there.
    pub system_default: Option<String>,
    /// The OS default is Hex Microphone, so `system_default` stands in.
    pub default_is_hex_microphone: bool,
    /// The internal microphone, listed but off because the lid is closed. A
    /// headphone-jack microphone stays usable and is never named here.
    pub off_with_lid_closed: Option<String>,
    /// The OS default is `off_with_lid_closed`, so `system_default` stands in.
    pub default_is_off_with_lid_closed: bool,
    /// The OS default, a Bluetooth headset that `system_default` stands in
    /// for so the headset keeps its audio quality.
    pub bluetooth_default_passed_over: Option<String>,
}

pub fn input_devices() -> Result<InputDevices> {
    #[cfg(target_os = "macos")]
    watch_input_devices();
    let host = cpal::default_host();
    let (devices, listed) = input_devices_of(&host)?;
    let default = default_input_of(&host).map(|(_, name)| name);
    let default_is_hex_microphone = default
        .as_deref()
        .is_some_and(crate::audio::hex_microphone::is_hex_virtual_device_name);
    let transports = devices.iter().map(transport).collect::<Vec<_>>();
    let route = current_route();
    let off_with_lid_closed = (0..devices.len())
        .find(|&index| route.lid_closed && transports[index] == Transport::BuiltIn)
        .map(|index| listed[index].clone());
    let default_is_off_with_lid_closed =
        off_with_lid_closed.is_some() && default == off_with_lid_closed;
    let plan = resolve_input(
        None,
        None,
        &listed,
        default.as_deref(),
        |index| transports[index],
        route,
    )
    .ok();
    let bluetooth_default_passed_over = default
        .clone()
        .filter(|name| {
            listed
                .iter()
                .position(|listed| listed == name)
                .is_some_and(|index| transports[index] == Transport::Bluetooth)
        })
        .filter(|_| matches!(plan.map(|plan| plan.target), Some(InputTarget::StandIn(_))));
    let system_default = plan
        .and_then(|plan| match plan.target {
            InputTarget::SystemDefault => default.clone(),
            InputTarget::Named(index) | InputTarget::StandIn(index) => Some(listed[index].clone()),
        })
        .filter(|_| cfg!(not(target_os = "linux")));
    // Linux names the sound server's sources instead, and only when it
    // passes over a Bluetooth default.
    #[cfg(target_os = "linux")]
    let (system_default, bluetooth_default_passed_over) = match route
        .avoid_bluetooth
        .then(crate::linux::input::route::bluetooth_stand_in)
        .flatten()
    {
        Some(stand_in) => (Some(stand_in.input), Some(stand_in.bluetooth)),
        None => (system_default, bluetooth_default_passed_over),
    };
    let mut names = listed
        .into_iter()
        .filter(|name| !crate::audio::hex_microphone::is_hex_virtual_device_name(name))
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    Ok(InputDevices {
        names,
        system_default,
        default_is_hex_microphone,
        off_with_lid_closed,
        default_is_off_with_lid_closed,
        bluetooth_default_passed_over,
    })
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn input_device_names() -> Result<Vec<String>> {
    input_devices().map(|devices| devices.names)
}
