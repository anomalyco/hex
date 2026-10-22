use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::NonNull;

use color_eyre::eyre::{Result, eyre};
use objc2_app_kit::NSRunningApplication;
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertySelector,
    kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyBundleID,
    kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID,
};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_foundation::NSString;

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
        canonical_bundle_id_for(&self.bundle_id, Some(&self.name))
    }

    fn dedup_key(&self) -> &str {
        self.canonical_bundle_id().unwrap_or(&self.bundle_id)
    }
}

fn canonical_bundle_id_for(bundle_id: &str, name: Option<&str>) -> Option<&'static str> {
    let bundle = bundle_id.to_ascii_lowercase();
    if bundle == "com.apple.webkit.gpu" {
        return name
            .is_some_and(|name| name.to_ascii_lowercase().starts_with("safari"))
            .then_some("com.apple.Safari");
    }
    [
        ("us.zoom.xos", "us.zoom.xos"),
        ("us.zoom.cpthost", "us.zoom.xos"),
        ("us.zoom.caphost", "us.zoom.xos"),
        ("us.zoom.aomhost", "us.zoom.xos"),
        ("us.zoom.zoomcefhelper", "us.zoom.xos"),
        ("us.zoom.zccimeetinghost", "us.zoom.xos"),
        ("us.zoom.zoomphone", "us.zoom.xos"),
        ("us.zoom.zoomhybridconf", "us.zoom.xos"),
        ("com.microsoft.teams2", "com.microsoft.teams2"),
        ("com.microsoft.teams", "com.microsoft.teams2"),
        ("com.tinyspeck.slackmacgap", "com.tinyspeck.slackmacgap"),
        ("com.apple.facetime", "com.apple.FaceTime"),
        ("com.google.chrome", "com.google.Chrome"),
        ("com.brave.browser", "com.brave.Browser"),
        ("com.apple.safari", "com.apple.Safari"),
        ("org.mozilla.firefox", "org.mozilla.firefox"),
        ("org.mozilla.plugincontainer", "org.mozilla.firefox"),
        ("com.microsoft.edgemac", "com.microsoft.edgemac"),
    ]
    .iter()
    .find_map(|(prefix, canonical)| {
        bundle
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
            .then_some(*canonical)
    })
}

pub(crate) fn canonical_application_name(canonical_bundle_id: &str) -> Option<&'static str> {
    match canonical_bundle_id {
        "us.zoom.xos" => Some("zoom.us"),
        "com.microsoft.teams2" => Some("Microsoft Teams"),
        "com.tinyspeck.slackmacgap" => Some("Slack"),
        "com.apple.FaceTime" => Some("FaceTime"),
        "com.google.Chrome" => Some("Google Chrome"),
        "com.brave.Browser" => Some("Brave Browser"),
        "com.apple.Safari" => Some("Safari"),
        "org.mozilla.firefox" => Some("Firefox"),
        "com.microsoft.edgemac" => Some("Microsoft Edge"),
        _ => None,
    }
}

fn is_helper_bundle_id(bundle_id: &str) -> bool {
    let lower = bundle_id.to_ascii_lowercase();
    lower.ends_with(".helper")
        || lower.contains(".helper.")
        || matches!(
            lower.as_str(),
            "com.apple.webkit.gpu"
                | "us.zoom.cpthost"
                | "us.zoom.caphost"
                | "us.zoom.aomhost"
                | "us.zoom.zoomcefhelper"
                | "us.zoom.zccimeetinghost"
                | "org.mozilla.plugincontainer"
        )
}

fn display_name(bundle_id: &str, localized_name: Option<String>) -> String {
    if !is_helper_bundle_id(bundle_id)
        && let Some(name) = localized_name.as_ref().filter(|name| !name.is_empty())
    {
        return name.clone();
    }
    if let Some(canonical) = canonical_bundle_id_for(bundle_id, localized_name.as_deref())
        && let Some(name) = canonical_application_name(canonical)
    {
        return name.to_string();
    }
    localized_name
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| bundle_id.to_string())
}

fn resolve_application_with(
    pid: i32,
    audio_bundle_id: Option<String>,
    mut lookup_running_app: impl FnMut(i32) -> Option<(String, Option<String>)>,
    mut lookup_parent_pid: impl FnMut(i32) -> Option<i32>,
    mut lookup_running_by_bundle: impl FnMut(&str) -> Option<(i32, String)>,
) -> Option<ActiveMicrophoneApplication> {
    let direct = lookup_running_app(pid);
    if let Some((ref bundle_id, ref name)) = direct
        && !is_helper_bundle_id(bundle_id)
    {
        return Some(ActiveMicrophoneApplication {
            pid,
            name: display_name(bundle_id, name.clone()),
            bundle_id: bundle_id.clone(),
        });
    }

    // Chromium and Electron run microphone input in a sandboxed utility
    // process (`audio.mojom.AudioService`) that does not register with
    // LaunchServices, so `NSRunningApplication(processIdentifier:)` returns
    // nil for the helper PID while its parent process is the host browser/app.
    let mut current_pid = pid;
    for _ in 0..4 {
        let Some(parent) = lookup_parent_pid(current_pid) else {
            break;
        };
        if let Some((parent_bundle, parent_name)) = lookup_running_app(parent)
            && !is_helper_bundle_id(&parent_bundle)
            && audio_bundle_id.as_deref().is_none_or(|audio_bundle| {
                is_helper_bundle_id(audio_bundle)
                    || audio_bundle.eq_ignore_ascii_case(&parent_bundle)
                    || (canonical_bundle_id_for(audio_bundle, None).is_some()
                        && canonical_bundle_id_for(audio_bundle, None)
                            == canonical_bundle_id_for(&parent_bundle, parent_name.as_deref()))
            })
        {
            return Some(ActiveMicrophoneApplication {
                pid: parent,
                name: display_name(&parent_bundle, parent_name),
                bundle_id: parent_bundle,
            });
        }
        current_pid = parent;
    }

    if audio_bundle_id.as_deref() == Some("com.apple.avconferenced") {
        let (facetime_pid, facetime_name) = lookup_running_by_bundle("com.apple.FaceTime")?;
        return Some(ActiveMicrophoneApplication {
            pid: facetime_pid,
            bundle_id: "com.apple.FaceTime".into(),
            name: facetime_name,
        });
    }

    let (bundle_id, localized_name) = match (direct, audio_bundle_id) {
        (Some((bundle_id, name)), _) => (bundle_id, name),
        (None, Some(bundle_id)) => (bundle_id, None),
        (None, None) => return None,
    };

    if is_helper_bundle_id(&bundle_id)
        && let Some(canonical) = canonical_bundle_id_for(&bundle_id, localized_name.as_deref())
        && let Some((host_pid, host_name)) = lookup_running_by_bundle(canonical)
    {
        return Some(ActiveMicrophoneApplication {
            pid: host_pid,
            bundle_id: canonical.into(),
            name: host_name,
        });
    }

    Some(ActiveMicrophoneApplication {
        pid,
        name: display_name(&bundle_id, localized_name),
        bundle_id,
    })
}

fn running_application_info(pid: i32) -> Option<(String, Option<String>)> {
    let application = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    let bundle_id = application.bundleIdentifier()?.to_string();
    if bundle_id.is_empty() {
        return None;
    }
    let name = application.localizedName().map(|name| name.to_string());
    Some((bundle_id, name))
}

fn running_application_by_bundle_id(bundle_id: &str) -> Option<(i32, String)> {
    let candidates = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(bundle_id),
    );
    let application = candidates.firstObject()?;
    let pid = application.processIdentifier();
    let name = application
        .localizedName()
        .map(|name| name.to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| canonical_application_name(bundle_id).map(str::to_string))
        .unwrap_or_else(|| bundle_id.to_string());
    Some((pid, name))
}

fn parent_pid(pid: i32) -> Option<i32> {
    if pid <= 1 {
        return None;
    }
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let ppid = i32::try_from(info.pbi_ppid).ok()?;
    (ppid > 1 && ppid != pid).then_some(ppid)
}

/// AppKit returns these lookups autoreleased. The meeting watcher polls from
/// a plain thread with no pool to drain them, so each poll would otherwise
/// keep every `NSRunningApplication` it touched for the life of the process.
pub fn active_microphone_applications() -> Result<Vec<ActiveMicrophoneApplication>> {
    objc2::rc::autoreleasepool(|_| inspect_microphone_applications())
}

fn inspect_microphone_applications() -> Result<Vec<ActiveMicrophoneApplication>> {
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
        let audio_bundle_id = property_cf_string(process_object, kAudioProcessPropertyBundleID);
        let Some(application) = resolve_application_with(
            pid,
            audio_bundle_id,
            running_application_info,
            parent_pid,
            running_application_by_bundle_id,
        ) else {
            continue;
        };
        applications.push(application);
    }
    applications.sort_by(|left, right| left.dedup_key().cmp(right.dedup_key()));
    applications.dedup_by(|left, right| left.dedup_key() == right.dedup_key());
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

fn property_cf_string(
    object: AudioObjectID,
    selector: AudioObjectPropertySelector,
) -> Option<String> {
    let mut raw: *const CFString = std::ptr::null();
    let mut size = size_of::<*const CFString>() as u32;
    let mut address = property_address(selector);
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut raw).cast::<c_void>(),
        )
    };
    if status != 0 || size as usize != size_of::<*const CFString>() {
        return None;
    }
    let ptr = NonNull::new(raw.cast_mut())?;
    let cf_string = unsafe { CFRetained::from_raw(ptr) };
    let value = cf_string.to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn physical_footprint() -> u64 {
        let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
        let status = unsafe {
            libc::proc_pid_rusage(
                std::process::id() as i32,
                libc::RUSAGE_INFO_V4,
                (&raw mut info).cast(),
            )
        };
        assert_eq!(status, 0);
        info.ri_phys_footprint
    }

    /// Polls like the meeting watcher, from a thread without an autorelease
    /// pool. Run alone: footprint is process-wide.
    #[test]
    #[ignore = "measures process footprint; needs another app using the microphone"]
    fn polling_microphone_activity_does_not_accumulate_running_applications() {
        std::thread::spawn(|| {
            assert!(
                !active_microphone_applications().unwrap().is_empty(),
                "an app must be using the microphone"
            );
            let before = physical_footprint();
            for _ in 0..20_000 {
                active_microphone_applications().unwrap();
            }
            let grown = physical_footprint().saturating_sub(before);
            println!("footprint grew {} KB over 20,000 polls", grown / 1024);
            assert!(grown < 2 * 1024 * 1024, "grew {grown} bytes");
        })
        .join()
        .unwrap();
    }

    #[test]
    fn resolves_unregistered_chromium_and_electron_audio_helpers_to_their_host_apps() {
        // Brave's sandboxed `audio.mojom.AudioService` helper is not registered in
        // `NSRunningApplication`, reports `com.brave.Browser.helper` to CoreAudio,
        // and is a child of the main `Brave Browser` process.
        let brave = resolve_application_with(
            2352,
            Some("com.brave.Browser.helper".into()),
            |pid| match pid {
                1592 => Some(("com.brave.Browser".into(), Some("Brave Browser".into()))),
                _ => None,
            },
            |pid| (pid == 2352).then_some(1592),
            |_| None,
        )
        .unwrap();
        assert_eq!(
            brave,
            ActiveMicrophoneApplication {
                pid: 1592,
                bundle_id: "com.brave.Browser".into(),
                name: "Brave Browser".into(),
            }
        );
        assert!(brave.is_supported_meeting_app());
        assert!(brave.is_browser());
        assert_eq!(brave.canonical_bundle_id(), Some("com.brave.Browser"));

        // Slack's audio utility process behaves the same way.
        let slack = resolve_application_with(
            31807,
            Some("com.tinyspeck.slackmacgap.helper".into()),
            |pid| match pid {
                31782 => Some(("com.tinyspeck.slackmacgap".into(), Some("Slack".into()))),
                _ => None,
            },
            |pid| (pid == 31807).then_some(31782),
            |_| None,
        )
        .unwrap();
        assert_eq!(
            slack,
            ActiveMicrophoneApplication {
                pid: 31782,
                bundle_id: "com.tinyspeck.slackmacgap".into(),
                name: "Slack".into(),
            }
        );
        assert!(slack.is_supported_meeting_app());
        assert!(!slack.is_browser());
        assert_eq!(
            slack.canonical_bundle_id(),
            Some("com.tinyspeck.slackmacgap")
        );
    }

    #[test]
    fn resolves_helper_bundle_ids_when_parent_lookup_is_unavailable() {
        let chrome = resolve_application_with(
            4100,
            Some("com.google.Chrome.helper".into()),
            |_| None,
            |_| None,
            |bundle| (bundle == "com.google.Chrome").then_some((4000, "Google Chrome".into())),
        )
        .unwrap();
        assert_eq!(
            chrome,
            ActiveMicrophoneApplication {
                pid: 4000,
                bundle_id: "com.google.Chrome".into(),
                name: "Google Chrome".into(),
            }
        );

        let zoom = resolve_application_with(
            9010,
            Some("us.zoom.CptHost".into()),
            |_| Some(("us.zoom.CptHost".into(), Some("CptHost".into()))),
            |_| None,
            |bundle| (bundle == "us.zoom.xos").then_some((9000, "zoom.us".into())),
        )
        .unwrap();
        assert_eq!(
            zoom,
            ActiveMicrophoneApplication {
                pid: 9000,
                bundle_id: "us.zoom.xos".into(),
                name: "zoom.us".into(),
            }
        );
        assert_eq!(zoom.canonical_bundle_id(), Some("us.zoom.xos"));
    }

    #[test]
    fn attributes_safari_webkit_gpu_and_facetime_avconferenced_only_when_active() {
        let safari = resolve_application_with(
            7100,
            Some("com.apple.WebKit.GPU".into()),
            |_| {
                Some((
                    "com.apple.WebKit.GPU".into(),
                    Some("Safari Graphics and Media".into()),
                ))
            },
            |_| None,
            |bundle| (bundle == "com.apple.Safari").then_some((7000, "Safari".into())),
        )
        .unwrap();
        assert_eq!(safari.canonical_bundle_id(), Some("com.apple.Safari"));
        assert!(safari.is_browser());

        let non_safari_webkit = resolve_application_with(
            7200,
            Some("com.apple.WebKit.GPU".into()),
            |_| {
                Some((
                    "com.apple.WebKit.GPU".into(),
                    Some("Mail Graphics and Media".into()),
                ))
            },
            |_| None,
            |_| Some((7000, "Safari".into())),
        )
        .unwrap();
        assert_eq!(non_safari_webkit.canonical_bundle_id(), None);
        assert!(!non_safari_webkit.is_supported_meeting_app());

        let facetime = resolve_application_with(
            8100,
            Some("com.apple.avconferenced".into()),
            |_| None,
            |_| None,
            |bundle| (bundle == "com.apple.FaceTime").then_some((8000, "FaceTime".into())),
        )
        .unwrap();
        assert_eq!(facetime.canonical_bundle_id(), Some("com.apple.FaceTime"));
        assert_eq!(facetime.name, "FaceTime");

        assert!(
            resolve_application_with(
                8100,
                Some("com.apple.avconferenced".into()),
                |_| None,
                |_| None,
                |_| None,
            )
            .is_none()
        );
    }

    #[test]
    fn canonical_bundle_matching_covers_supported_apps_and_variants() {
        for (bundle_id, name, expected, browser) in [
            ("us.zoom.xos", "zoom.us", Some("us.zoom.xos"), false),
            ("us.zoom.CptHost", "CptHost", Some("us.zoom.xos"), false),
            (
                "com.microsoft.teams2",
                "Microsoft Teams",
                Some("com.microsoft.teams2"),
                false,
            ),
            (
                "com.microsoft.teams2.helper",
                "Microsoft Teams Helper",
                Some("com.microsoft.teams2"),
                false,
            ),
            (
                "com.tinyspeck.slackmacgap.helper",
                "Slack Helper",
                Some("com.tinyspeck.slackmacgap"),
                false,
            ),
            (
                "com.brave.Browser.beta",
                "Brave Browser Beta",
                Some("com.brave.Browser"),
                true,
            ),
            (
                "com.google.Chrome.canary",
                "Google Chrome Canary",
                Some("com.google.Chrome"),
                true,
            ),
            (
                "org.mozilla.plugincontainer",
                "plugin-container",
                Some("org.mozilla.firefox"),
                true,
            ),
            (
                "com.microsoft.edgemac.helper",
                "Microsoft Edge Helper",
                Some("com.microsoft.edgemac"),
                true,
            ),
            ("com.kitlangton.hex2", "Hex", None, false),
        ] {
            let app = ActiveMicrophoneApplication {
                pid: 100,
                bundle_id: bundle_id.into(),
                name: name.into(),
            };
            assert_eq!(app.canonical_bundle_id(), expected, "bundle {bundle_id}");
            assert_eq!(app.is_browser(), browser, "browser {bundle_id}");
        }
    }
}
