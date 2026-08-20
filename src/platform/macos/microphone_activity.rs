use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::NonNull;

use color_eyre::eyre::{Result, eyre};
use objc2_app_kit::NSRunningApplication;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertySelector,
    kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyIsRunningInput,
    kAudioProcessPropertyPID,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveMicrophoneApplication {
    pub pid: i32,
    pub bundle_id: String,
    pub name: String,
}

impl ActiveMicrophoneApplication {
    pub fn is_supported_meeting_app(&self) -> bool {
        self.canonical_bundle_id().is_some()
    }

    pub fn is_browser(&self) -> bool {
        matches!(
            self.canonical_bundle_id(),
            Some(
                "com.google.Chrome"
                    | "com.brave.Browser"
                    | "com.apple.Safari"
                    | "org.mozilla.firefox"
                    | "com.microsoft.edgemac"
            )
        )
    }

    pub fn detection_priority(&self) -> u8 {
        u8::from(self.is_browser())
    }

    pub fn canonical_bundle_id(&self) -> Option<&'static str> {
        let bundle = self.bundle_id.to_ascii_lowercase();
        [
            ("us.zoom.xos", "us.zoom.xos"),
            ("com.microsoft.teams2", "com.microsoft.teams2"),
            ("com.tinyspeck.slackmacgap", "com.tinyspeck.slackmacgap"),
            ("com.apple.facetime", "com.apple.FaceTime"),
            ("com.google.chrome", "com.google.Chrome"),
            ("com.brave.browser", "com.brave.Browser"),
            ("com.apple.safari", "com.apple.Safari"),
            ("org.mozilla.firefox", "org.mozilla.firefox"),
            ("com.microsoft.edgemac", "com.microsoft.edgemac"),
        ]
        .iter()
        .find_map(|(prefix, canonical)| {
            (bundle == *prefix || bundle.starts_with(&format!("{prefix}."))).then_some(*canonical)
        })
    }
}

pub fn active_microphone_applications() -> Result<Vec<ActiveMicrophoneApplication>> {
    let process_objects = property_vec::<AudioObjectID>(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyProcessObjectList,
    )?;
    let mut applications = Vec::new();
    for process_object in process_objects {
        let Ok(is_running_input) =
            property_value::<u32>(process_object, kAudioProcessPropertyIsRunningInput)
        else {
            continue;
        };
        if is_running_input == 0 {
            continue;
        }
        let Ok(pid) = property_value::<i32>(process_object, kAudioProcessPropertyPID) else {
            continue;
        };
        let Some(application) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        else {
            continue;
        };
        let Some(bundle_id) = application.bundleIdentifier() else {
            continue;
        };
        let bundle_id = bundle_id.to_string();
        let name = application
            .localizedName()
            .map(|name| name.to_string())
            .unwrap_or_else(|| bundle_id.clone());
        applications.push(ActiveMicrophoneApplication {
            pid,
            bundle_id,
            name,
        });
    }
    applications.sort_by(|left, right| left.bundle_id.cmp(&right.bundle_id));
    applications.dedup_by(|left, right| left.bundle_id == right.bundle_id);
    Ok(applications)
}

fn property_address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn property_value<T: Copy + Default>(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
) -> Result<T> {
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    let mut address = property_address(selector);
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
    if status == 0 && size as usize == size_of::<T>() {
        Ok(value)
    } else {
        Err(eyre!(
            "CoreAudio property {selector:#x} failed with status {status} and size {size}"
        ))
    }
}

fn property_vec<T: Copy + Default>(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
) -> Result<Vec<T>> {
    let mut address = property_address(selector);
    let mut size = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 {
        return Err(eyre!(
            "CoreAudio property size {selector:#x} failed with status {status}"
        ));
    }
    if !(size as usize).is_multiple_of(size_of::<T>()) {
        return Err(eyre!(
            "CoreAudio property {selector:#x} had invalid size {size}"
        ));
    }
    let mut values = vec![T::default(); size as usize / size_of::<T>()];
    if values.is_empty() {
        return Ok(values);
    }
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(values.as_mut_ptr().cast::<c_void>())
                .ok_or_else(|| eyre!("CoreAudio returned a null output buffer"))?,
        )
    };
    if status == 0 {
        values.truncate(size as usize / size_of::<T>());
        Ok(values)
    } else {
        Err(eyre!(
            "CoreAudio property {selector:#x} failed with status {status}"
        ))
    }
}
