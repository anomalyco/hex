//! Which listed input a meeting's call app records from, found by the
//! device UID the OS reports for the app.

use cpal::Device;
use cpal::traits::DeviceTrait;

/// Where the input with UID `uid` is listed among `devices`, named `names`.
pub(super) fn listed_at(uid: &str, devices: &[Device], names: &[String]) -> Option<usize> {
    let ids = devices
        .iter()
        .map(|device| device.id().ok().map(|id| id.id().to_owned()))
        .collect::<Vec<_>>();
    position(uid, &ids, names)
}

/// Where the input with UID `uid` is, given each listed input's UID and
/// name; never Hex's own virtual microphone.
fn position(uid: &str, ids: &[Option<String>], names: &[String]) -> Option<usize> {
    ids.iter().zip(names).position(|(id, name)| {
        id.as_deref() == Some(uid) && !super::hex_microphone::is_hex_virtual_device_name(name)
    })
}

#[cfg(test)]
mod tests {
    use super::position;

    #[test]
    fn a_meeting_finds_the_input_its_call_uses_by_uid() {
        let names = ["MacBook Pro Microphone", "Hex Microphone", "Jabra Evolve"].map(String::from);
        let ids = ["BuiltIn", "HexMic", "Jabra"].map(|id| Some(id.to_owned()));
        assert_eq!(position("Jabra", &ids, &names), Some(2));
        assert_eq!(position("BuiltIn", &ids, &names), Some(0));
        // Never Hex's own microphone, and nothing for an unlisted input.
        assert_eq!(position("HexMic", &ids, &names), None);
        assert_eq!(position("Unplugged", &ids, &names), None);
        // An input whose UID cannot be read never matches.
        assert_eq!(position("Jabra", &[None, None, None], &names), None);
    }
}
