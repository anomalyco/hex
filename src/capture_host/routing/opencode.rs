//! OpenCode in WSL for Annotate's sends: with Labs › OpenCode in WSL on,
//! the router asks the host's service discovery, so a capture goes to a
//! service whose registration Windows can't read itself. A native Windows
//! service, or a discovery that found none, answers `None`, leaving the
//! router's own path exactly as it was.

#[cfg(target_os = "windows")]
use std::path::Path;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use scrawl_capture::env::{OpenCode, Reply};

#[cfg(target_os = "windows")]
use crate::desktop::opencode_service;

/// How long naming a capture folder may spend finding the service.
#[cfg(target_os = "windows")]
const DIRECTORY_DEADLINE: Duration = Duration::from_secs(3);

/// The default WSL distribution's OpenCode service, when Labs › OpenCode in
/// WSL is on.
#[cfg(target_os = "windows")]
pub(super) struct Wsl;

#[cfg(target_os = "windows")]
impl OpenCode for Wsl {
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Option<anyhow::Result<Reply>> {
        if !crate::labs::opencode_in_wsl() {
            return None;
        }
        let started = Instant::now();
        let cancelled = AtomicBool::new(false);
        let Ok(service) = opencode_service::find_cancellable(timeout, &cancelled) else {
            return None;
        };
        if !service.is_wsl() {
            return None;
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Some(Err(anyhow::anyhow!("OpenCode didn't answer in time")));
        }
        Some(
            service
                .request(method, path, body, None, remaining, &cancelled)
                .map(|response| Reply {
                    status: response.status,
                    body: response.body,
                })
                .map_err(|error| anyhow::anyhow!(request_error(&format!("{error:#}")))),
        )
    }

    fn directory(&self, host: &Path) -> Option<String> {
        if !crate::labs::opencode_in_wsl() {
            return None;
        }
        let service =
            opencode_service::find_cancellable(DIRECTORY_DEADLINE, &AtomicBool::new(false)).ok()?;
        service.wsl_directory(host)
    }
}

/// A failed `Service::request` from a confirmed WSL service, in the words
/// the capture capsule already classifies: a timeout, or a request dropped
/// after the connection was made, is the router's "didn't answer in time";
/// anything that couldn't be reached starts with "Could not reach server"
/// like the router's own request errors.
#[cfg(any(target_os = "windows", test))]
fn request_error(error: &str) -> String {
    let error = error.trim();
    if error.contains("didn't answer in time")
        || error.contains("exceeded")
        || error.contains("was cancelled")
        || error.contains("could not read OpenCode")
        || error.contains("could not send OpenCode")
    {
        "OpenCode didn't answer in time".to_owned()
    } else if error.contains("Could not reach server")
        || error.contains("answered more than")
        || error.contains("returned invalid HTTP")
    {
        error.to_owned()
    } else {
        format!("Could not reach server: {error}")
    }
}

#[cfg(test)]
mod tests {
    use crate::capture_host::routing::{send_why, unreachable_why};

    use super::request_error;

    #[test]
    fn a_wsl_request_failure_reads_as_the_capsule_expects() {
        // The send and read lines match opencode_service's `wrap_err_with`
        // formats; the reach line matches the request loop's own error.
        let path = "/api/session";
        for (raw, sent, unreachable) in [
            (
                format!("OpenCode {path} exceeded 5 seconds"),
                "OpenCode didn't answer",
                "didn't answer in time",
            ),
            (
                "OpenCode isn't running".to_owned(),
                "OpenCode isn't running",
                "it isn't running",
            ),
            (
                format!("could not send OpenCode {path}: Connection reset by peer"),
                "OpenCode didn't answer",
                "didn't answer in time",
            ),
            (
                format!("could not read OpenCode {path}: Connection reset by peer"),
                "OpenCode didn't answer",
                "didn't answer in time",
            ),
            (
                format!("could not reach OpenCode {path}: Connection refused (os error 10061)"),
                "OpenCode isn't running",
                "it isn't running",
            ),
        ] {
            let mapped = request_error(&raw);
            assert_eq!(send_why(&mapped, false), sent, "{raw}");
            assert_eq!(unreachable_why(&mapped), unreachable, "{raw}");
        }
        for answered in [
            "OpenCode /api/session returned invalid HTTP",
            "OpenCode /api/session answered more than 16 MiB",
        ] {
            assert_eq!(request_error(answered), answered, "{answered}");
        }
    }
}
