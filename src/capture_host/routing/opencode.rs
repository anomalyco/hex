//! OpenCode in WSL for Annotate's sends: with Labs › OpenCode in WSL on,
//! the router asks the host's service discovery, so a capture goes to a
//! service whose registration Windows can't read itself. A native Windows
//! service answers `None`, leaving the router's own path exactly as it was.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use scrawl_capture::env::{OpenCode, Reply};

use crate::desktop::opencode_service::{self, Missing};

/// How long naming a capture folder may spend finding the service.
const DIRECTORY_DEADLINE: Duration = Duration::from_secs(3);

/// The default WSL distribution's OpenCode service, when Labs › OpenCode in
/// WSL is on.
pub(super) struct Wsl;

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
        let service = match opencode_service::find_cancellable(timeout, &cancelled) {
            Ok(service) => service,
            Err(missing) => return Some(Err(missing_error(missing))),
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
                .map_err(|error| anyhow::anyhow!("{error:#}")),
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

/// Why no service answered, in the words the capture capsule already reads
/// as "OpenCode isn't running" and as the update notice.
fn missing_error(missing: Missing) -> anyhow::Error {
    match missing {
        Missing::NotRunning => anyhow::anyhow!("Could not reach server: {}", missing.message()),
        Missing::Outdated => anyhow::anyhow!(missing.message()),
    }
}
