use std::process::Command;

use block2::RcBlock;
use objc2::msg_send;
use objc2::runtime::{AnyClass, Bool};
use objc2_foundation::NSString;

use crate::transcription_models::TranscriptionSelection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionState {
    Ready,
    NeedsRequest,
    NeedsSettings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupStatus {
    pub microphone: PermissionState,
    pub input_monitoring: PermissionState,
    pub accessibility: PermissionState,
    pub command_model: bool,
    pub transcription_model: bool,
}

impl SetupStatus {
    pub fn ready(self) -> bool {
        self.microphone == PermissionState::Ready
            && self.input_monitoring == PermissionState::Ready
            && self.accessibility == PermissionState::Ready
            && self.command_model
            && self.transcription_model
    }
}

pub fn status(selection: &TranscriptionSelection) -> SetupStatus {
    let transcription_model = crate::transcription_models::validate(selection)
        .is_ok_and(|model| crate::transcription_models::is_installed(model, &selection.language));
    SetupStatus {
        microphone: microphone_state(),
        input_monitoring: settings(CGPreflightListenEventAccess()),
        accessibility: settings(CGPreflightPostEventAccess()),
        command_model: !crate::app_settings::commands_enabled()
            || crate::moonshine::model_installed(),
        transcription_model,
    }
}

pub fn request_microphone() {
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        tracing::error!("AVCaptureDevice is unavailable");
        return;
    };
    let handler = RcBlock::new(|granted: Bool| {
        tracing::info!(
            granted = granted.as_bool(),
            "microphone permission request finished"
        );
    });
    unsafe {
        let _: () = msg_send![
            class,
            requestAccessForMediaType: &*AVMediaTypeAudio,
            completionHandler: &*handler
        ];
    }
}

pub fn request_input_monitoring() {
    unsafe {
        CGRequestListenEventAccess();
    }
}

pub fn request_accessibility() {
    unsafe {
        CGRequestPostEventAccess();
    }
}

pub fn open_permission_settings(permission: &str) {
    let pane = match permission {
        "microphone" => "Privacy_Microphone",
        "input" => "Privacy_ListenEvent",
        "accessibility" => "Privacy_Accessibility",
        _ => "Privacy",
    };
    if let Err(error) = Command::new("/usr/bin/open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{pane}"
        ))
        .spawn()
    {
        tracing::error!(%error, "could not open Privacy & Security settings");
    }
}

fn microphone_state() -> PermissionState {
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        return PermissionState::NeedsRequest;
    };
    let status: isize =
        unsafe { msg_send![class, authorizationStatusForMediaType: &*AVMediaTypeAudio] };
    microphone_authorization_state(status)
}

const fn settings(value: bool) -> PermissionState {
    if value {
        PermissionState::Ready
    } else {
        PermissionState::NeedsSettings
    }
}

const fn microphone_authorization_state(status: isize) -> PermissionState {
    match status {
        0 => PermissionState::NeedsRequest,
        3 => PermissionState::Ready,
        _ => PermissionState::NeedsSettings,
    }
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: *const NSString;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    safe fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
    safe fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_requires_every_required_capability() {
        let mut status = SetupStatus {
            microphone: PermissionState::Ready,
            input_monitoring: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            command_model: true,
            transcription_model: true,
        };
        assert!(status.ready());
        status.command_model = false;
        assert!(!status.ready());
    }

    #[test]
    fn denied_microphone_permission_requires_system_settings() {
        assert_eq!(
            microphone_authorization_state(0),
            PermissionState::NeedsRequest
        );
        assert_eq!(
            microphone_authorization_state(2),
            PermissionState::NeedsSettings
        );
        assert_eq!(microphone_authorization_state(3), PermissionState::Ready);
    }
}
