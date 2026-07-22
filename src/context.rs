use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use url::Url;

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
            (Self::BrowserHost(left), Self::BrowserHost(right)) => {
                normalize_browser_host(left) == normalize_browser_host(right)
            }
            (Self::Application(left), Self::Application(right)) => left == right,
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
    pub fn capture() -> Result<Self> {
        const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
        let mut child = Command::new("/usr/bin/osascript")
            .args(["-e", CAPTURE_SCRIPT])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .wrap_err("could not inspect the foreground application")?;
        let started = Instant::now();
        loop {
            if child
                .try_wait()
                .wrap_err("could not inspect the foreground application")?
                .is_some()
            {
                break;
            }
            if started.elapsed() >= CAPTURE_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(eyre!("foreground context inspection timed out"));
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = child
            .wait_with_output()
            .wrap_err("could not inspect the foreground application")?;
        if !output.status.success() {
            return Err(eyre!(
                "foreground context inspection failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines();
        let application = lines.next().filter(|line| !line.is_empty()).map(Into::into);
        let browser_url = lines
            .next()
            .filter(|line| !line.is_empty())
            .and_then(|line| Url::parse(line).ok());
        let window_title = lines.next().filter(|line| !line.is_empty()).map(Into::into);
        Ok(Self {
            application,
            browser_url,
            window_title,
            selected_text: None,
            input_revision: None,
        })
    }

    pub fn browser_host(&self) -> Option<&str> {
        self.browser_url.as_ref().and_then(Url::host_str)
    }

    pub fn browser_host_is(&self, host: &str) -> bool {
        // Brave is the first browser adapter. Command semantics intentionally
        // depend on browser host, not this application name.
        self.application.as_deref() == Some("Brave Browser")
            && self.browser_host().is_some_and(|current| {
                normalize_browser_host(current) == normalize_browser_host(host)
            })
    }

    pub fn application_is(&self, application: &str) -> bool {
        self.application.as_deref() == Some(application)
    }

    pub fn label(&self) -> String {
        match (&self.application, self.browser_host()) {
            (Some(application), Some(host)) => format!("{application} · {host}"),
            (Some(application), None) => application.clone(),
            _ => "unknown context".into(),
        }
    }
}

fn normalize_browser_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

const CAPTURE_SCRIPT: &str = r#"
tell application "System Events"
    set frontApp to name of first application process whose frontmost is true
end tell
set activeUrl to ""
set windowTitle to ""
tell application "System Events"
    try
        set windowTitle to name of front window of first application process whose frontmost is true
    end try
end tell
if frontApp is "Brave Browser" then
    tell application "Brave Browser"
        if (count of windows) > 0 then set activeUrl to URL of active tab of front window
    end tell
end if
return frontApp & linefeed & activeUrl & linefeed & windowTitle
"#;
