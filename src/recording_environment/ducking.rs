//! The CoreAudio adapter for "Lower audio": the default output's channel
//! volumes, or its master volume when channels cannot be set separately.
use std::mem::{offset_of, size_of};
use std::ptr::NonNull;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectIsPropertySettable,
    AudioObjectPropertyAddress, AudioObjectSetPropertyData, kAudioObjectPropertyScopeOutput,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};

use crate::output_ducking::{Guard, Volume};

pub(super) fn start(factor: f32) -> Option<Guard> {
    Guard::start(factor, Output::current)
}

struct Output {
    device: u32,
    elements: Vec<u32>,
}

fn address(selector: [u8; 4], element: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: u32::from_be_bytes(selector),
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: element,
    }
}

impl Output {
    fn current() -> Option<Self> {
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
        let separate = Self {
            device,
            elements: (1..=channels).collect(),
        };
        if channels > 0 && separate.writable() && separate.read().is_some() {
            return Some(separate);
        }
        // A master write must not destroy a readable balance we cannot restore individually.
        if separate.elements.iter().any(|&element| {
            super::read_property::<f32>(device, address(*b"volm", element)).is_some()
        }) {
            return None;
        }
        let master = Self {
            device,
            elements: vec![0],
        };
        (master.writable() && master.read().is_some()).then_some(master)
    }

    fn writable(&self) -> bool {
        self.elements.iter().all(|&element| {
            let mut property = address(*b"volm", element);
            let mut settable = 0;
            // SAFETY: Both output pointers refer to initialized, correctly typed values.
            unsafe {
                AudioObjectIsPropertySettable(
                    self.device,
                    NonNull::from(&mut property),
                    NonNull::from(&mut settable),
                ) == 0
                    && settable != 0
            }
        })
    }
}

impl Volume for Output {
    fn read(&self) -> Option<Vec<f32>> {
        self.elements
            .iter()
            .map(|&element| {
                super::read_property::<f32>(self.device, address(*b"volm", element))
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            })
            .collect()
    }

    fn write(&mut self, values: &[f32]) -> bool {
        let mut success = true;
        for (&element, &value) in self.elements.iter().zip(values) {
            let mut property = address(*b"volm", element);
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
