//! Keeps the computer from idle sleep while Hex records: an IOKit
//! no-idle-sleep assertion on macOS, a logind `idle:sleep` (else `idle`)
//! inhibitor on Linux, and a system-required power request on Windows. Each is held
//! until its guard drops. Starting one may wait on the OS, so callers start
//! it off the capture loop.

/// Prevents idle sleep until dropped, or `None` (logged) when the OS refuses.
pub fn prevent_sleep() -> Option<PreventSleep> {
    match PreventSleep::start() {
        Ok(prevention) => Some(prevention),
        Err(error) => {
            tracing::warn!(%error, "could not prevent idle system sleep");
            None
        }
    }
}

pub use platform::PreventSleep;

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    use objc2_core_foundation::CFString;

    const POWER_ASSERTION_LEVEL_ON: u32 = 255;

    pub struct PreventSleep {
        assertion_id: u32,
    }

    impl PreventSleep {
        pub(super) fn start() -> std::io::Result<Self> {
            let assertion_type = CFString::from_static_str("NoIdleSleepAssertion");
            let assertion_name = CFString::from_static_str("Hex intentional recording");
            let mut assertion_id = 0;
            // SAFETY: Both Core Foundation strings remain alive for the call and
            // assertion_id points to writable, correctly sized storage. IOKit
            // retains the assertion independently until IOPMAssertionRelease.
            let status = unsafe {
                IOPMAssertionCreateWithName(
                    (&*assertion_type as *const CFString).cast(),
                    POWER_ASSERTION_LEVEL_ON,
                    (&*assertion_name as *const CFString).cast(),
                    &mut assertion_id,
                )
            };
            if status != 0 {
                return Err(std::io::Error::other(format!(
                    "IOPMAssertionCreateWithName failed with IOReturn 0x{:08x}",
                    status as u32
                )));
            }
            Ok(Self { assertion_id })
        }
    }

    impl Drop for PreventSleep {
        fn drop(&mut self) {
            // SAFETY: assertion_id was returned by a successful create call and
            // this guard is its sole owner, so release occurs exactly once.
            let status = unsafe { IOPMAssertionRelease(self.assertion_id) };
            if status != 0 {
                tracing::warn!(
                    status = format_args!("0x{:08x}", status as u32),
                    "could not release idle-sleep assertion"
                );
            }
        }
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: *const c_void,
            assertion_level: u32,
            assertion_name: *const c_void,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::time::Duration;

    use zbus::OwnedFd;

    const TIMEOUT: Duration = Duration::from_millis(500);

    /// logind keeps the inhibitor while this descriptor is open.
    pub struct PreventSleep {
        _inhibitor: OwnedFd,
    }

    impl PreventSleep {
        /// Blocks idle and sleep, or only idle where polkit refuses to let
        /// a process outside the active seat session, such as Hex's user
        /// service, block sleep.
        pub(super) fn start() -> std::io::Result<Self> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let bus = tokio::time::timeout(TIMEOUT, zbus::Connection::system())
                    .await
                    .map_err(|_| std::io::Error::other("the system D-Bus did not answer"))?
                    .map_err(std::io::Error::other)?;
                match inhibit(&bus, "idle:sleep").await {
                    Ok(inhibitor) => Ok(inhibitor),
                    Err(error) => {
                        tracing::debug!(%error, "logind refused to block sleep; blocking idle only");
                        inhibit(&bus, "idle").await
                    }
                }
            })
        }
    }

    async fn inhibit(bus: &zbus::Connection, what: &str) -> std::io::Result<PreventSleep> {
        let call = bus.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &(what, "Hex", "Recording", "block"),
        );
        let reply = tokio::time::timeout(TIMEOUT, call)
            .await
            .map_err(|_| std::io::Error::other("logind did not answer"))?
            .map_err(std::io::Error::other)?;
        let inhibitor: OwnedFd = reply.body().deserialize().map_err(std::io::Error::other)?;
        Ok(PreventSleep {
            _inhibitor: inhibitor,
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
    };
    use windows_sys::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    const POWER_REQUEST_CONTEXT_VERSION: u32 = 0;

    /// A power request, unlike `SetThreadExecutionState`, is not tied to the
    /// thread that made it.
    pub struct PreventSleep {
        request: HANDLE,
    }

    // SAFETY: A power request handle may be used and closed from any thread.
    unsafe impl Send for PreventSleep {}

    impl PreventSleep {
        pub(super) fn start() -> std::io::Result<Self> {
            let mut reason: Vec<u16> = "Hex is recording".encode_utf16().chain([0]).collect();
            let context = REASON_CONTEXT {
                Version: POWER_REQUEST_CONTEXT_VERSION,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 {
                    SimpleReasonString: reason.as_mut_ptr(),
                },
            };
            // SAFETY: `context` and its string outlive the call, which copies them.
            let request = unsafe { PowerCreateRequest(&context) };
            if request.is_null() || request == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: `request` is the valid handle just created.
            if unsafe { PowerSetRequest(request, PowerRequestSystemRequired) } == 0 {
                let error = std::io::Error::last_os_error();
                // SAFETY: The handle is ours and closed once.
                unsafe { CloseHandle(request) };
                return Err(error);
            }
            Ok(Self { request })
        }
    }

    impl Drop for PreventSleep {
        fn drop(&mut self) {
            // SAFETY: The request was set in `start`, and this guard closes it once.
            unsafe {
                PowerClearRequest(self.request, PowerRequestSystemRequired);
                CloseHandle(self.request);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "exercises the OS's native idle-sleep prevention"]
    fn native_idle_sleep_prevention_acquires_and_releases() {
        drop(super::PreventSleep::start().unwrap());
    }
}
