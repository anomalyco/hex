use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use url::Url;

#[cfg(target_os = "macos")]
use objc2::rc::autoreleasepool;
#[cfg(target_os = "macos")]
use objc2_app_kit::NSWorkspace;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextSnapshot {
    pub application: Option<String>,
    pub browser_url: Option<Url>,
    pub window_title: Option<String>,
    pub selected_text: Option<String>,
    pub input_revision: Option<u64>,
}

pub struct ContextMonitor {
    pub updates: Receiver<ContextSnapshot>,
    stop: Arc<AtomicBool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextSelector {
    Always,
    BrowserHost(String),
    Application(String),
}

impl ContextSelector {
    pub fn application(application: impl Into<String>) -> Self {
        Self::Application(application.into())
    }

    pub fn browser_host(host: impl Into<String>) -> Self {
        Self::BrowserHost(normalize_browser_host(&host.into()))
    }

    pub fn matches(&self, context: &ContextSnapshot) -> bool {
        match self {
            Self::Always => true,
            Self::BrowserHost(host) => context.browser_host_is(host),
            Self::Application(application) => context.application_is(application),
        }
    }

    pub fn is_browser(&self) -> bool {
        matches!(self, Self::BrowserHost(_))
    }

    pub(crate) fn specificity(&self) -> u8 {
        match self {
            Self::Always => 0,
            Self::Application(_) => 1,
            Self::BrowserHost(_) => 2,
        }
    }

    pub(crate) fn can_coexist_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Always, _) | (_, Self::Always) => true,
            (Self::BrowserHost(left), Self::BrowserHost(right)) => browser_hosts_equal(left, right),
            (Self::Application(left), Self::Application(right)) => {
                application_names_equal(left, right)
            }
            (Self::BrowserHost(_), Self::Application(_))
            | (Self::Application(_), Self::BrowserHost(_)) => true,
        }
    }
}

impl ContextMonitor {
    pub fn start() -> Self {
        let (sender, updates) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        thread::spawn(move || {
            let mut previous = None;
            let mut previous_error = None;
            while !worker_stop.load(Ordering::Acquire) {
                match ContextSnapshot::capture() {
                    Ok(context) => {
                        previous_error = None;
                        if previous.as_ref() != Some(&context) {
                            match sender.try_send(context.clone()) {
                                Ok(()) => previous = Some(context),
                                Err(TrySendError::Full(_)) => {}
                                Err(TrySendError::Disconnected(_)) => break,
                            }
                        }
                    }
                    Err(error) => {
                        let error = error.to_string();
                        if previous_error.as_ref() != Some(&error) {
                            tracing::warn!(%error, "could not capture foreground context");
                            previous_error = Some(error);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(500));
            }
        });
        Self { updates, stop }
    }
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

    pub fn browser_host(&self) -> Option<&str> {
        self.browser_url.as_ref().and_then(Url::host_str)
    }

    pub fn browser_host_is(&self, host: &str) -> bool {
        // Brave is the first browser adapter. Command semantics intentionally
        // depend on browser host, not this application name.
        self.application.as_deref() == Some("Brave Browser")
            && self
                .browser_host()
                .is_some_and(|current| browser_hosts_equal(current, host))
    }

    pub fn application_is(&self, application: &str) -> bool {
        self.application
            .as_deref()
            .is_some_and(|current| application_names_equal(current, application))
    }

    pub fn label(&self) -> String {
        match (&self.application, self.browser_host()) {
            (Some(application), Some(host)) => format!("{application} · {host}"),
            (Some(application), None) => application.clone(),
            _ => "unknown context".into(),
        }
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

fn normalize_browser_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

fn application_names_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn browser_hosts_equal(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
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

    #[test]
    fn application_matching_ignores_process_name_capitalization() {
        let context = ContextSnapshot {
            application: Some("zed".into()),
            ..ContextSnapshot::default()
        };

        assert!(ContextSelector::application("Zed").matches(&context));
    }
}
