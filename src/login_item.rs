use color_eyre::eyre::{Result, eyre};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoginItemStatus {
    Disabled,
    Enabled,
    RequiresApproval,
}

pub fn status() -> Result<LoginItemStatus> {
    // SAFETY: mainAppService and status do not retain caller-owned pointers.
    let status = unsafe { SMAppService::mainAppService().status() };
    match status {
        SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound => {
            Ok(LoginItemStatus::Disabled)
        }
        SMAppServiceStatus::Enabled => Ok(LoginItemStatus::Enabled),
        SMAppServiceStatus::RequiresApproval => Ok(LoginItemStatus::RequiresApproval),
        _ => Err(eyre!("macOS returned an unknown login item status")),
    }
}

pub fn set_enabled(enabled: bool) -> Result<LoginItemStatus> {
    // SAFETY: SMAppService owns the NSError produced by these synchronous APIs.
    let service = unsafe { SMAppService::mainAppService() };
    let current = status()?;
    if enabled && current == LoginItemStatus::Disabled {
        unsafe { service.registerAndReturnError() }.map_err(|error| eyre!(error.to_string()))?;
    } else if !enabled && current != LoginItemStatus::Disabled {
        unsafe { service.unregisterAndReturnError() }.map_err(|error| eyre!(error.to_string()))?;
    }
    status()
}

pub fn open_settings() {
    // SAFETY: This asks macOS to open its Login Items settings pane.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}
