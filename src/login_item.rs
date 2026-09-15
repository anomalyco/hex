use color_eyre::eyre::{Result, eyre};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoginItemStatus {
    Disabled,
    Enabled,
    RequiresApproval,
}

fn status() -> Result<LoginItemStatus> {
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

fn set_enabled(enabled: bool) -> Result<LoginItemStatus> {
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

fn open_settings() {
    // SAFETY: This asks macOS to open its Login Items settings pane.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

#[derive(Clone, Copy)]
pub enum LoginItemRequest {
    Status,
    SetEnabled(bool),
    OpenSettings,
}

pub struct LoginItemResponse {
    pub status: Option<LoginItemStatus>,
    pub error: Option<String>,
    pub clear_error: bool,
}

/// The caller retains the receiver until completion before admitting another request.
/// All ServiceManagement calls, including registration and opening Settings, can block.
pub fn request(request: LoginItemRequest) -> std::sync::mpsc::Receiver<LoginItemResponse> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let response = objc2::rc::autoreleasepool(|_| {
            let result = match request {
                LoginItemRequest::Status => status(),
                LoginItemRequest::SetEnabled(enabled) => set_enabled(enabled),
                LoginItemRequest::OpenSettings => {
                    open_settings();
                    status()
                }
            };
            match result {
                Ok(status) => LoginItemResponse {
                    status: Some(status),
                    error: None,
                    clear_error: matches!(request, LoginItemRequest::SetEnabled(_)),
                },
                Err(error) => LoginItemResponse {
                    // Registration may have changed macOS state even when it reports an error.
                    status: matches!(request, LoginItemRequest::SetEnabled(_))
                        .then(|| status().ok())
                        .flatten(),
                    error: Some(error.to_string()),
                    clear_error: false,
                },
            }
        });
        let _ = sender.send(response);
    });
    receiver
}
