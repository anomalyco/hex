//! The CoreAudio adapter for "Lower audio": the default output's channel
//! volumes, or its main volume, or its virtual main volume when channels
//! cannot be set separately.
use std::mem::{offset_of, size_of};
use std::ptr::NonNull;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectIsPropertySettable,
    AudioObjectPropertyAddress, AudioObjectSetPropertyData, kAudioObjectPropertyScopeOutput,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};

use super::VIRTUAL_MAIN_VOLUME;
use crate::audio::output_ducking::{Guard, Volume};

const VOLUME: [u8; 4] = *b"volm";

pub(super) fn start(factor: f32) -> Option<Guard> {
    Guard::start(factor, Output::current)
}

/// One property of an output, and the elements of it that ramp together.
struct Control {
    selector: [u8; 4],
    elements: Vec<u32>,
}

impl Control {
    /// The channels that actually have a settable volume scalar, then the main
    /// volume, then the virtual main volume, which macOS keeps balanced. A
    /// channel without a volume control is skipped, not fatal.
    fn select(channels: u32, mut usable: impl FnMut([u8; 4], u32) -> bool) -> Option<Self> {
        let elements: Vec<u32> = (1..=channels)
            .filter(|&element| usable(VOLUME, element))
            .collect();
        if !elements.is_empty() {
            return Some(Self {
                selector: VOLUME,
                elements,
            });
        }
        [VOLUME, VIRTUAL_MAIN_VOLUME]
            .into_iter()
            .find(|&selector| usable(selector, 0))
            .map(|selector| Self {
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

/// Whether the output can read this volume and set it.
fn usable(device: u32, address: AudioObjectPropertyAddress) -> bool {
    let mut property = address;
    let mut settable = 0;
    // SAFETY: Both output pointers refer to initialized, correctly typed values.
    if unsafe {
        AudioObjectIsPropertySettable(
            device,
            NonNull::from(&mut property),
            NonNull::from(&mut settable),
        )
    } != 0
        || settable == 0
    {
        return false;
    }
    super::read_property::<f32>(device, address)
        .is_some_and(|value| value.is_finite() && (0.0..=1.0).contains(&value))
}

impl Output {
    fn current() -> Option<Self> {
        let output = Self::resolve();
        if output.is_none() {
            tracing::warn!("found no output volume to lower while recording");
        }
        output
    }

    /// The default output and the control to ramp on it, when it has one.
    fn resolve() -> Option<Self> {
        let device = super::default_output_device()?;
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
            usable(device, address(selector, element))
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

    fn selected(
        channels: u32,
        usable: impl FnMut([u8; 4], u32) -> bool,
    ) -> Option<([u8; 4], Vec<u32>)> {
        Control::select(channels, usable).map(|control| (control.selector, control.elements))
    }

    #[test]
    fn channels_without_a_volume_scalar_are_skipped_not_fatal() {
        // A USB adapter reporting two buffers of two channels where only the
        // first pair has a per-channel volume scalar.
        assert_eq!(
            selected(4, |selector, element| selector == VOLUME
                && (1..=2).contains(&element)),
            Some((VOLUME, vec![1, 2]))
        );
        // One usable channel is enough to lower the output.
        assert_eq!(
            selected(2, |selector, element| selector == VOLUME && element == 2),
            Some((VOLUME, vec![2]))
        );
    }

    #[test]
    fn the_main_then_the_virtual_main_volume_are_the_fallbacks() {
        assert_eq!(
            selected(2, |selector, element| selector == VOLUME && element == 0),
            Some((VOLUME, vec![0]))
        );
        assert_eq!(
            selected(2, |selector, _| selector == VIRTUAL_MAIN_VOLUME),
            Some((VIRTUAL_MAIN_VOLUME, vec![0]))
        );
        assert_eq!(selected(2, |_, _| false), None);
    }
}
