//! Launch at login. macOS registers the app with `SMAppService` through a
//! worker thread, since ServiceManagement can block; Windows writes the
//! current user's `Run` registry value (`windows_login_item`). Linux starts
//! the `hex.service` unit from its install script or Nix module, so Hex does
//! not offer the setting there.

#[cfg(target_os = "macos")]
use color_eyre::eyre::{Result, eyre};
#[cfg(target_os = "macos")]
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoginItemStatus {
    Disabled,
    Enabled,
    RequiresApproval,
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

/// Reads, or turns on or off, the login item right away. ServiceManagement
/// can block: call it off the main thread.
#[cfg(target_os = "macos")]
pub fn set_now(enabled: Option<bool>) -> Result<LoginItemStatus> {
    objc2::rc::autoreleasepool(|_| match enabled {
        Some(enabled) => set_enabled(enabled),
        None => status(),
    })
}

#[cfg(target_os = "macos")]
fn open_settings() {
    // SAFETY: This asks macOS to open its Login Items settings pane.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginItemRequest {
    Status,
    SetEnabled(bool),
    OpenSettings,
}

#[derive(Debug)]
pub struct LoginItemFailure {
    pub status: Option<LoginItemStatus>,
    pub message: String,
}

/// A request's outcome.
pub type LoginItemResult = Result<LoginItemStatus, LoginItemFailure>;

/// One worker per Settings window. Polling never queues behind an active request;
/// user actions replace the single pending action and run before the next poll.
#[cfg(target_os = "macos")]
pub struct LoginItemWorker {
    requests: std::sync::mpsc::SyncSender<LoginItemRequest>,
    responses: std::sync::mpsc::Receiver<LoginItemResult>,
    active: Option<LoginItemRequest>,
    pending: Option<LoginItemRequest>,
}

#[cfg(target_os = "macos")]
impl LoginItemWorker {
    pub fn new() -> Result<Self, String> {
        Self::with_handler(|request| {
            let handle = || {
                let result = match request {
                    LoginItemRequest::Status => status(),
                    LoginItemRequest::SetEnabled(enabled) => set_enabled(enabled),
                    LoginItemRequest::OpenSettings => {
                        open_settings();
                        status()
                    }
                };
                result.map_err(|error| LoginItemFailure {
                    // Registration may change the OS state even when it reports an error.
                    status: matches!(request, LoginItemRequest::SetEnabled(_))
                        .then(|| status().ok())
                        .flatten(),
                    message: error.to_string(),
                })
            };
            objc2::rc::autoreleasepool(|_| handle())
        })
    }

    fn with_handler(
        mut handle: impl FnMut(LoginItemRequest) -> LoginItemResult + Send + 'static,
    ) -> Result<Self, String> {
        let (requests, receiver) = std::sync::mpsc::sync_channel(1);
        let (sender, responses) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("login-item".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    if sender.send(handle(request)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| format!("Could not start Launch at Login worker: {error}"))?;
        let mut worker = Self {
            requests,
            responses,
            active: None,
            pending: None,
        };
        worker.request(LoginItemRequest::Status)?;
        Ok(worker)
    }

    pub fn request(&mut self, request: LoginItemRequest) -> Result<(), String> {
        if self.active.is_some() {
            if request != LoginItemRequest::Status {
                self.pending = Some(request);
            }
            return Ok(());
        }
        self.requests
            .try_send(request)
            .map_err(|error| format!("Could not contact Launch at Login worker: {error}"))?;
        self.active = Some(request);
        Ok(())
    }

    /// No request is running: the last outcome answered every action asked.
    pub fn idle(&self) -> bool {
        self.active.is_none()
    }

    /// The finished request's outcome, once it has one.
    pub fn poll(&mut self) -> Result<Option<LoginItemResult>, String> {
        let result = match self.responses.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("Launch at Login worker stopped unexpectedly".into());
            }
        };
        self.active = None;
        if let Some(pending) = self.pending.take() {
            self.request(pending)?;
        }
        Ok(Some(result))
    }
}

// Dropping the channels lets the worker exit after its current native call.
// Never join here: ServiceManagement can block, including while Settings closes.

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::sync::mpsc::{RecvTimeoutError, channel};
    use std::time::{Duration, Instant};

    fn completed(worker: &mut LoginItemWorker) -> LoginItemResult {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(response) = worker.poll().unwrap() {
                return response;
            }
            assert!(Instant::now() < deadline, "worker did not complete");
            std::thread::yield_now();
        }
    }

    #[test]
    fn login_item_worker_reuses_thread_and_coalesces_actions_before_polling() {
        let (calls, observed) = channel();
        let (release, gate) = channel();
        let mut worker = LoginItemWorker::with_handler(move |request| {
            calls.send((request, std::thread::current().id())).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(LoginItemStatus::Disabled)
        })
        .unwrap();
        let (request, thread) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(request, LoginItemRequest::Status);
        assert_ne!(thread, std::thread::current().id());
        // A stalled native call neither blocks polling nor admits more status work.
        assert!(worker.poll().unwrap().is_none());
        worker.request(LoginItemRequest::SetEnabled(true)).unwrap();
        worker.request(LoginItemRequest::SetEnabled(false)).unwrap();
        for _ in 0..10 {
            worker.request(LoginItemRequest::Status).unwrap();
        }
        release.send(()).unwrap();
        assert!(completed(&mut worker).is_ok());
        assert!(!worker.idle(), "the last action runs next");
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            (LoginItemRequest::SetEnabled(false), thread)
        );
        release.send(()).unwrap();
        assert!(completed(&mut worker).is_ok());
        assert!(worker.idle());
        worker.request(LoginItemRequest::OpenSettings).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            (LoginItemRequest::OpenSettings, thread)
        );
        release.send(()).unwrap();
        assert!(completed(&mut worker).is_ok());
        drop(worker);
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn login_item_worker_closes_without_waiting_for_native_call() {
        let (calls, observed) = channel();
        let (release, gate) = channel();
        let mut worker = LoginItemWorker::with_handler(move |request| {
            calls.send(request).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(LoginItemStatus::Disabled)
        })
        .unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            LoginItemRequest::Status
        );
        worker.request(LoginItemRequest::SetEnabled(true)).unwrap();
        drop(worker);
        // The call is still blocked when the owner is dropped. Closing must not join it
        // or execute the user action that was still pending in the closed window.
        release.send(()).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)),
            Err(RecvTimeoutError::Disconnected)
        );
    }
}
