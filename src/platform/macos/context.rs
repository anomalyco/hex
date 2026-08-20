use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use url::Url;

pub use crate::command_context::{ContextSelector, ContextSnapshot};

#[cfg(target_os = "macos")]
use objc2::rc::autoreleasepool;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSWorkspace;

pub struct ContextMonitor {
    pub updates: Receiver<ContextSnapshot>,
    stop: Arc<AtomicBool>,
}

impl ContextMonitor {
    pub fn start() -> Self {
        Self::start_with_capture(ContextSnapshot::capture, Duration::from_millis(500))
    }

    fn start_with_capture(
        mut capture: impl FnMut() -> Result<ContextSnapshot> + Send + 'static,
        poll_interval: Duration,
    ) -> Self {
        let (sender, updates) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        thread::spawn(move || {
            let mut previous = None;
            let mut previous_error = None;
            while !worker_stop.load(Ordering::Acquire) {
                match capture() {
                    Ok(context) => {
                        previous_error = None;
                        if !publish_context(&sender, &mut previous, context) {
                            break;
                        }
                    }
                    Err(error) => {
                        let error = error.to_string();
                        if previous_error.as_ref() != Some(&error) {
                            tracing::warn!(%error, "could not capture foreground context");
                            previous_error = Some(error);
                        }
                        if !publish_context(&sender, &mut previous, ContextSnapshot::default()) {
                            break;
                        }
                    }
                }
                thread::sleep(poll_interval);
            }
        });
        Self { updates, stop }
    }
}

fn publish_context(
    sender: &mpsc::SyncSender<ContextSnapshot>,
    previous: &mut Option<ContextSnapshot>,
    context: ContextSnapshot,
) -> bool {
    if previous.as_ref() == Some(&context) {
        return true;
    }
    match sender.try_send(context.clone()) {
        Ok(()) => *previous = Some(context),
        Err(TrySendError::Full(_)) => {}
        Err(TrySendError::Disconnected(_)) => return false,
    }
    true
}

impl Drop for ContextMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl ContextSnapshot {
    #[cfg(target_os = "macos")]
    pub fn capture() -> Result<Self> {
        capture_macos()
    }

    #[cfg(not(target_os = "macos"))]
    pub fn capture() -> Result<Self> {
        Ok(Self::default())
    }
}

#[cfg(target_os = "macos")]
fn capture_macos() -> Result<ContextSnapshot> {
    let (application, pid) = autoreleasepool(|_| {
        let application = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .ok_or_else(|| eyre!("macOS did not report a foreground application"))?;
        let name = application
            .localizedName()
            .map(|name| name.to_string())
            .ok_or_else(|| eyre!("the foreground application has no display name"))?;
        Ok::<_, color_eyre::Report>((name, application.processIdentifier()))
    })?;
    let browser_url = if application == "Brave Browser" {
        capture_brave_url()?
    } else {
        None
    };
    Ok(ContextSnapshot {
        application: Some(application),
        browser_url,
        window_title: crate::accessibility::focused_window_title(pid),
        selected_text: None,
        input_revision: None,
    })
}

#[cfg(target_os = "macos")]
fn capture_brave_url() -> Result<Option<Url>> {
    const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", BRAVE_URL_SCRIPT])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("could not inspect the active Brave tab")?;
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .wrap_err("could not inspect the active Brave tab")?
            .is_some()
        {
            break;
        }
        if started.elapsed() >= CAPTURE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(eyre!("active Brave tab inspection timed out"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child
        .wait_with_output()
        .wrap_err("could not inspect the active Brave tab")?;
    if !output.status.success() {
        return Err(eyre!(
            "active Brave tab inspection failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let url = String::from_utf8_lossy(&output.stdout);
    Ok(Url::parse(url.trim()).ok())
}

#[cfg(target_os = "macos")]
const BRAVE_URL_SCRIPT: &str = r#"
tell application "Brave Browser"
    if (count of windows) > 0 then return URL of active tab of front window
end tell
return ""
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn application_matching_ignores_process_name_capitalization() {
        let context = ContextSnapshot {
            application: Some("zed".into()),
            ..ContextSnapshot::default()
        };

        assert!(ContextSelector::application("Zed").matches(&context));
    }

    #[test]
    fn monitor_invalidates_stale_context_after_capture_failure() {
        let captured = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(Url::parse("https://x.com/home").unwrap()),
            ..ContextSnapshot::default()
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let capture_calls = calls.clone();
        let expected = captured.clone();
        let monitor = ContextMonitor::start_with_capture(
            move || {
                if capture_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(captured.clone())
                } else {
                    Err(eyre!("browser adapter failed"))
                }
            },
            Duration::from_millis(1),
        );

        assert_eq!(
            monitor
                .updates
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            expected
        );
        assert_eq!(
            monitor
                .updates
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            ContextSnapshot::default()
        );
    }
}
