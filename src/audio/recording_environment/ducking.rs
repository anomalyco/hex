//! The CoreAudio adapter for "Lower audio": the default output's channel
//! volumes, or its main volume, or its virtual main volume when channels
//! cannot be set separately.
use std::mem::{offset_of, size_of};
use std::ptr::NonNull;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectHasProperty,
    AudioObjectIsPropertySettable, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    kAudioObjectPropertyScopeOutput,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};

use super::VIRTUAL_MAIN_VOLUME;
use crate::audio::output_ducking::{Guard, Volume};

const VOLUME: [u8; 4] = *b"volm";

pub(super) fn start(factor: f32) -> Option<Guard> {
    Guard::start(factor, Output::current)
}

/// What one channel of an output reports for its volume scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VolumeScalar {
    /// Present, readable, and settable: Lower audio can ramp it and put it back.
    Settable,
    /// Present but unusable, because it cannot be set or its value cannot be
    /// read, so its balance cannot be restored.
    ReadOnly,
    /// The property is not present at all: a stream channel without its own
    /// volume control.
    Absent,
}

/// One property of an output, and the elements of it that ramp together.
struct Control {
    selector: [u8; 4],
    elements: Vec<u32>,
}

impl Control {
    /// The settable channels when every channel that reports a volume can be
    /// set. A channel whose volume is present but unusable rules out both a
    /// per-channel subset and the plain main volume, so only the virtual main
    /// volume, which macOS keeps balanced, is left. Channels with no volume
    /// property at all are skipped; with none present, the main volume and then
    /// the virtual main volume are the fallbacks.
    fn select(channels: u32, mut scalar: impl FnMut([u8; 4], u32) -> VolumeScalar) -> Option<Self> {
        let mut elements = Vec::new();
        let mut read_only = false;
        for element in 1..=channels {
            match scalar(VOLUME, element) {
                VolumeScalar::Settable => elements.push(element),
                VolumeScalar::ReadOnly => read_only = true,
                VolumeScalar::Absent => {}
            }
        }
        if read_only {
            return Self::master(VIRTUAL_MAIN_VOLUME, &mut scalar);
        }
        if !elements.is_empty() {
            return Some(Self {
                selector: VOLUME,
                elements,
            });
        }
        Self::master(VOLUME, &mut scalar).or_else(|| Self::master(VIRTUAL_MAIN_VOLUME, &mut scalar))
    }

    /// The element-0 volume of `selector`, when it can be read and set.
    fn master(
        selector: [u8; 4],
        scalar: &mut impl FnMut([u8; 4], u32) -> VolumeScalar,
    ) -> Option<Self> {
        (scalar(selector, 0) == VolumeScalar::Settable).then_some(Self {
            selector,
            elements: vec![0],
        })
    }
}

struct Output {
    device: u32,
    control: Control,
}

fn address(selector: [u8; 4], element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: u32::from_be_bytes(selector),
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: element,
    }
}

/// What the output reports for this volume: absent, read-only, or settable.
fn scalar(device: u32, address: AudioObjectPropertyAddress) -> VolumeScalar {
    let mut property = address;
    // SAFETY: The property address refers to an initialized, correctly typed value.
    if !unsafe { AudioObjectHasProperty(device, NonNull::from(&mut property)) } {
        return VolumeScalar::Absent;
    }
    // A property that exists but cannot be read is still one we must not
    // disturb, so it counts as read-only rather than absent.
    let readable = super::read_property::<f32>(device, address)
        .is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value));
    if !readable {
        return VolumeScalar::ReadOnly;
    }
    let mut settable = 0;
    // SAFETY: Both output pointers refer to initialized, correctly typed values.
    let settable = unsafe {
        AudioObjectIsPropertySettable(
            device,
            NonNull::from(&mut property),
            NonNull::from(&mut settable),
        ) == 0
            && settable != 0
    };
    if settable {
        VolumeScalar::Settable
    } else {
        VolumeScalar::ReadOnly
    }
}

impl Output {
    fn current() -> Option<Self> {
        let device = super::default_output_device();
        let output = device.and_then(Self::resolve);
        if output.is_none() {
            match device {
                Some(device) => tracing::warn!(
                    device,
                    "lower audio: no controllable output volume on the default output, or its stream layout could not be read; leaving it unchanged"
                ),
                None => tracing::warn!(
                    "lower audio: no default output device to lower, leaving it unchanged"
                ),
            }
        }
        output
    }

    /// The default output and the control to ramp on it, when it has one.
    fn resolve(device: u32) -> Option<Self> {
        let mut property = address(*b"slay", 0); // stream configuration
        let mut bytes = 0;
        // SAFETY: The property address and size point to initialized stack values.
        if unsafe {
            AudioObjectGetPropertyDataSize(
                device,
                NonNull::from(&mut property),
                0,
                std::ptr::null(),
                NonNull::from(&mut bytes),
            )
        } != 0
            || !(size_of::<AudioBufferList>() as u32..=65536).contains(&bytes)
        {
            return None;
        }
        // u64 storage gives AudioBufferList and its pointer fields their required alignment.
        let mut storage = vec![0u64; (bytes as usize).div_ceil(8)];
        // SAFETY: Storage is aligned and has at least the size reported by CoreAudio.
        if unsafe {
            AudioObjectGetPropertyData(
                device,
                NonNull::from(&mut property),
                0,
                std::ptr::null(),
                NonNull::from(&mut bytes),
                NonNull::new(storage.as_mut_ptr()).unwrap().cast(),
            )
        } != 0
        {
            return None;
        }
        if bytes as usize > storage.len() * size_of::<u64>()
            || (bytes as usize) < size_of::<AudioBufferList>()
        {
            return None;
        }
        // SAFETY: The successful property read initialized the header within storage.
        let count =
            unsafe { (*(storage.as_ptr().cast::<AudioBufferList>())).mNumberBuffers } as usize;
        let offset = offset_of!(AudioBufferList, mBuffers);
        if offset + count.checked_mul(size_of::<AudioBuffer>())? > bytes as usize {
            return None;
        }
        // SAFETY: The bounds above cover every buffer, and offset preserves alignment.
        let buffers = unsafe {
            std::slice::from_raw_parts(
                storage
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<AudioBuffer>(),
                count,
            )
        };
        let channels = buffers
            .iter()
            .try_fold(0u32, |sum, b| sum.checked_add(b.mNumberChannels))?;
        if channels > 256 {
            return None;
        }
        let control = Control::select(channels, |selector, element| {
            scalar(device, address(selector, element))
        })?;
        Some(Self { device, control })
    }
}

impl Volume for Output {
    fn read(&self) -> Option<Vec<f32>> {
        self.control
            .elements
            .iter()
            .map(|&element| {
                super::read_property::<f32>(self.device, address(self.control.selector, element))
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            })
            .collect()
    }

    fn write(&mut self, values: &[f32]) -> bool {
        let mut success = true;
        for (&element, &value) in self.control.elements.iter().zip(values) {
            let mut property = address(self.control.selector, element);
            let mut value = value;
            // SAFETY: The scalar and property are initialized and correctly sized.
            success &= unsafe {
                AudioObjectSetPropertyData(
                    self.device,
                    NonNull::from(&mut property),
                    0,
                    std::ptr::null(),
                    size_of::<f32>() as u32,
                    NonNull::from(&mut value).cast(),
                ) == 0
            };
        }
        success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use VolumeScalar::{Absent, ReadOnly, Settable};

    fn selected(
        channels: u32,
        scalar: impl FnMut([u8; 4], u32) -> VolumeScalar,
    ) -> Option<([u8; 4], Vec<u32>)> {
        Control::select(channels, scalar).map(|control| (control.selector, control.elements))
    }

    #[test]
    fn channels_without_a_volume_scalar_are_skipped_not_fatal() {
        // The reported USB adapter: two buffers of two channels with scalars on
        // channels 1 and 2 only, and no element-0 volume of either kind.
        assert_eq!(
            selected(4, |selector, element| match (selector, element) {
                (VOLUME, 1 | 2) => Settable,
                _ => Absent,
            }),
            Some((VOLUME, vec![1, 2]))
        );
        // A channel with no control of its own does not stop the others.
        assert_eq!(
            selected(2, |selector, element| match (selector, element) {
                (VOLUME, 2) => Settable,
                _ => Absent,
            }),
            Some((VOLUME, vec![2]))
        );
    }

    #[test]
    fn a_read_only_channel_leaves_the_balance_to_the_virtual_main_volume() {
        // Channel 1 can be set while channel 2 can only be read: lowering
        // channel 1 alone would be lopsided, and a plain main write could
        // flatten channel 2, so the balanced virtual main volume is the only
        // choice.
        assert_eq!(
            selected(2, |selector, element| match (selector, element) {
                (VOLUME, 1) => Settable,
                (VOLUME, 2) => ReadOnly,
                (VIRTUAL_MAIN_VOLUME, 0) => Settable,
                _ => Absent,
            }),
            Some((VIRTUAL_MAIN_VOLUME, vec![0]))
        );
        // The same with nothing else to set: one read-only channel is enough
        // to send the whole output to the virtual main volume.
        assert_eq!(
            selected(1, |selector, element| match (selector, element) {
                (VOLUME, 1) => ReadOnly,
                (VIRTUAL_MAIN_VOLUME, 0) => Settable,
                _ => Absent,
            }),
            Some((VIRTUAL_MAIN_VOLUME, vec![0]))
        );
        // Without it the output is left alone, even with a settable main
        // volume.
        assert_eq!(
            selected(2, |selector, element| match (selector, element) {
                (VOLUME, 1) => Settable,
                (VOLUME, 2) => ReadOnly,
                (VOLUME, 0) => Settable,
                _ => Absent,
            }),
            None
        );
    }

    #[test]
    fn the_main_then_the_virtual_main_volume_are_the_fallbacks() {
        // No channel reports a volume at all: element 0's volume, then the
        // virtual main volume.
        assert_eq!(
            selected(2, |selector, element| match (selector, element) {
                (VOLUME, 0) => Settable,
                _ => Absent,
            }),
            Some((VOLUME, vec![0]))
        );
        assert_eq!(
            selected(2, |selector, element| match (selector, element) {
                (VIRTUAL_MAIN_VOLUME, 0) => Settable,
                _ => Absent,
            }),
            Some((VIRTUAL_MAIN_VOLUME, vec![0]))
        );
        assert_eq!(selected(2, |_, _| Absent), None);
    }
}
