//! Release-awareness for the Windows alpha: a bounded background check of
//! the public GitHub releases feed. It only reports that a newer version
//! exists; installation stays a manual download from the release page.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const RELEASES_API: &str = "https://api.github.com/repos/anomalyco/hex/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/anomalyco/hex/releases/latest";
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateCheck {
    Checking,
    Current,
    Available { version: String, url: String },
    Failed,
}

#[derive(Clone)]
pub struct WindowsUpdater {
    state: Arc<Mutex<UpdateCheck>>,
    stop: Arc<AtomicBool>,
    check_now: Arc<AtomicBool>,
}

impl WindowsUpdater {
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(UpdateCheck::Checking));
        let stop = Arc::new(AtomicBool::new(false));
        let check_now = Arc::new(AtomicBool::new(false));
        if std::env::var_os("HEX_MOCK_UPDATE").is_some() {
            *state.lock().unwrap_or_else(|error| error.into_inner()) = UpdateCheck::Available {
                version: "9.9.9".into(),
                url: RELEASES_PAGE.into(),
            };
            return Self {
                state,
                stop,
                check_now,
            };
        }
        let worker_state = state.clone();
        let worker_stop = stop.clone();
        let worker_check_now = check_now.clone();
        let _ = thread::Builder::new()
            .name("windows-update-check".into())
            .spawn(move || {
                wait_for_next_check(&worker_stop, &worker_check_now, STARTUP_DELAY);
                while !worker_stop.load(Ordering::Relaxed) {
                    worker_check_now.store(false, Ordering::Relaxed);
                    let next = check_latest_release();
                    *worker_state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = next;
                    wait_for_next_check(&worker_stop, &worker_check_now, CHECK_INTERVAL);
                }
            });
        Self {
            state,
            stop,
            check_now,
        }
    }

    /// Run a check immediately instead of waiting out the interval.
    pub fn request_check(&self) {
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = UpdateCheck::Checking;
        self.check_now.store(true, Ordering::Relaxed);
    }

    pub fn state(&self) -> UpdateCheck {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn wait_for_next_check(stop: &AtomicBool, check_now: &AtomicBool, total: Duration) {
    let mut remaining = total;
    while !stop.load(Ordering::Relaxed)
        && !check_now.load(Ordering::Relaxed)
        && remaining > Duration::ZERO
    {
        let step = remaining.min(Duration::from_millis(500));
        thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
}

/// One bounded, unauthenticated query through the curl that ships with
/// Windows; any failure (offline, rate limit, private repository) reports
/// `Failed` and the next interval retries.
fn check_latest_release() -> UpdateCheck {
    let output = Command::new("curl.exe")
        .args([
            "--silent",
            "--fail",
            "--location",
            "--max-time",
            "20",
            "--header",
            "User-Agent: hex-windows-updater",
            "--header",
            "Accept: application/vnd.github+json",
            RELEASES_API,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let Ok(output) = output else {
        return UpdateCheck::Failed;
    };
    if !output.status.success() {
        return UpdateCheck::Failed;
    }
    let Ok(release) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return UpdateCheck::Failed;
    };
    let Some(tag) = release.get("tag_name").and_then(|tag| tag.as_str()) else {
        return UpdateCheck::Failed;
    };
    let latest = tag.trim_start_matches(['v', 'V']);
    let Some(latest_version) = parse_version(latest) else {
        return UpdateCheck::Failed;
    };
    let current =
        parse_version(env!("CARGO_PKG_VERSION")).expect("the crate version is well-formed");
    if latest_version > current {
        let url = release
            .get("html_url")
            .and_then(|url| url.as_str())
            .unwrap_or(RELEASES_PAGE)
            .to_string();
        UpdateCheck::Available {
            version: latest.to_string(),
            url,
        }
    } else {
        UpdateCheck::Current
    }
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn versions_compare_numerically() {
        assert!(parse_version("2.10.0") > parse_version("2.9.9"));
        assert!(parse_version("3.0") > parse_version("2.99.99"));
        assert_eq!(parse_version("2.1.0"), Some((2, 1, 0)));
    }

    #[test]
    fn tags_with_suffixes_still_parse() {
        assert_eq!(parse_version("2.2.0-beta"), Some((2, 2, 0)));
        assert_eq!(parse_version("nonsense"), None);
    }
}
