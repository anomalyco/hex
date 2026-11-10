//! What a capture needs from macOS before it starts: Screen Recording (not
//! for an iPhone), the microphone its narration comes from, and
//! Accessibility for its shortcut and pointer input. A capture that lacks
//! one never starts; the capsule's permission helper says which, and
//! follows the grant while it's made.

use std::sync::OnceLock;

use crate::desktop::onboarding::{PermissionKind, PermissionState};

/// A permission a capture needs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureNeed {
    ScreenRecording,
    Microphone,
    Accessibility,
}

impl CaptureNeed {
    /// Its name in System Settings.
    pub fn name(self) -> &'static str {
        match self {
            Self::ScreenRecording => "Screen Recording",
            Self::Microphone => "Microphone",
            Self::Accessibility => "Accessibility",
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn kind(self) -> PermissionKind {
        match self {
            Self::ScreenRecording => PermissionKind::ScreenRecording,
            Self::Microphone => PermissionKind::Microphone,
            Self::Accessibility => PermissionKind::Accessibility,
        }
    }
}

/// How a missing permission stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedState {
    /// Not granted.
    Off,
    /// Granted, but this Hex can use it only once it restarts.
    Restart,
}

/// What asked for the permission helper: a capture starting, or Labs ›
/// Screenshots' picker. The helper's grant returns to what asked: a capture
/// records the screen, while the picker opens again to shoot or record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskedBy {
    Capture,
    /// Labs › Screenshots, which reopens to shoot, or to record when
    /// `record` is set.
    Screenshots {
        record: bool,
    },
}

impl AskedBy {
    /// What a capture refused for a permission asks: a Screenshots recording
    /// (`clip`) returns to its picker; any other capture is Annotate's.
    pub fn for_capture(clip: bool) -> Self {
        if clip {
            Self::Screenshots { record: true }
        } else {
            Self::Capture
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn need_state(state: PermissionState) -> Option<NeedState> {
    match state {
        PermissionState::Ready => None,
        PermissionState::NeedsRestart => Some(NeedState::Restart),
        PermissionState::NeedsRequest | PermissionState::NeedsSettings => Some(NeedState::Off),
    }
}

/// The first permission a capture lacks, Screen Recording first and only
/// when `screen` (an iPhone records no screen). Reads Screen Recording
/// afresh when the cheap check says no, so a grant made since launch
/// counts.
#[cfg(target_os = "macos")]
pub fn missing(screen: bool) -> Option<(CaptureNeed, NeedState)> {
    let order = [
        CaptureNeed::ScreenRecording,
        CaptureNeed::Microphone,
        CaptureNeed::Accessibility,
    ];
    order
        .into_iter()
        .filter(|need| screen || *need != CaptureNeed::ScreenRecording)
        .find_map(|need| state_of(need, true).map(|state| (need, state)))
}

/// How `need` stands now: `None` once a capture can use it. Never blocks
/// unless `fresh`: Screen Recording then reads through a new helper, else it
/// follows the watched cache a visible permission surface polls.
#[cfg(target_os = "macos")]
pub fn state_of(need: CaptureNeed, fresh: bool) -> Option<NeedState> {
    use super::ScreenAccess;
    match need {
        CaptureNeed::ScreenRecording => {
            if scrawl_capture::capture::screen_capture_usable(false) {
                return None;
            }
            let access = if fresh {
                super::screen_access_now()
            } else {
                super::screen_access_watched()
            };
            match access {
                ScreenAccess::Allowed => None,
                ScreenAccess::NeedsRestart => Some(NeedState::Restart),
                ScreenAccess::Off => Some(NeedState::Off),
            }
        }
        CaptureNeed::Microphone => {
            need_state(crate::desktop::onboarding::status_with_transcription_model(true).microphone)
        }
        CaptureNeed::Accessibility => need_state(
            crate::desktop::onboarding::status_with_transcription_model(true).accessibility,
        ),
    }
}

/// Linux asks nothing of the person before a capture: the portal asks.
#[cfg(not(target_os = "macos"))]
pub fn missing(_screen: bool) -> Option<(CaptureNeed, NeedState)> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn state_of(_need: CaptureNeed, _fresh: bool) -> Option<NeedState> {
    None
}

/// The helper's Allow: macOS's own prompt where it still has one, else
/// System Settings with the guide floating beside it. Main thread only.
#[cfg(target_os = "macos")]
pub fn allow(need: CaptureNeed) {
    use crate::desktop::onboarding::PermissionState;
    match need {
        CaptureNeed::Microphone
            if crate::desktop::onboarding::status_with_transcription_model(true).microphone
                == PermissionState::NeedsRequest =>
        {
            crate::desktop::onboarding::request_microphone();
        }
        // Granted since the helper opened: the next poll shows it.
        CaptureNeed::ScreenRecording if super::screen_access() != super::ScreenAccess::Off => {}
        need => crate::desktop::permission_guide::show(need.kind()),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn allow(_need: CaptureNeed) {}

/// The helper's Restart Hex. Main thread only.
#[cfg(target_os = "macos")]
pub fn restart() {
    if crate::desktop::onboarding::spawn_relaunch() {
        crate::macos_rune::quit();
    } else {
        tracing::warn!("could not restart Hex for a capture permission");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn restart() {}

type Helper = Box<dyn Fn(CaptureNeed, AskedBy) + Send + Sync>;

static HELPER: OnceLock<Helper> = OnceLock::new();

/// Shows the permission helper with `show` whenever a capture is refused
/// for a missing permission. The capsule sets it once; without it a refusal
/// only answers with its error.
pub fn on_missing(show: impl Fn(CaptureNeed, AskedBy) + Send + Sync + 'static) {
    let _ = HELPER.set(Box::new(show));
}

/// A capture was refused for `need`; `clip` is a Screenshots recording,
/// which returns to its picker rather than Annotate's whole-screen capture.
pub fn report_capture(need: CaptureNeed, clip: bool) {
    report_asked(need, AskedBy::for_capture(clip));
}

/// A screenshot or recording was refused for `need`: the helper returns to
/// the Screenshots picker, to shoot or, with `record`, to record.
#[cfg(target_os = "macos")]
pub fn report_screenshots(need: CaptureNeed, record: bool) {
    report_asked(need, AskedBy::Screenshots { record });
}

fn report_asked(need: CaptureNeed, asked_by: AskedBy) {
    #[cfg(target_os = "macos")]
    crate::desktop::permission_guide::clear_legacy_entries_async(need.kind());
    if let Some(show) = HELPER.get() {
        show(need, asked_by);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Screenshots recording returns to its picker once granted; any other
    /// capture returns to Annotate's own whole-screen capture.
    #[test]
    fn a_clip_capture_asks_for_the_screenshots_picker() {
        assert_eq!(
            AskedBy::for_capture(true),
            AskedBy::Screenshots { record: true }
        );
        assert_eq!(AskedBy::for_capture(false), AskedBy::Capture);
    }
}
