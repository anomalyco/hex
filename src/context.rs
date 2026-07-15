use std::process::Command;
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use url::Url;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextSnapshot {
    pub application: Option<String>,
    pub browser_url: Option<Url>,
    pub window_title: Option<String>,
}

pub struct ContextMonitor {
    pub updates: Receiver<ContextSnapshot>,
}

impl ContextMonitor {
    pub fn start() -> Self {
        let (sender, updates) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut previous = None;
            let mut previous_error = None;
            loop {
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
        Self { updates }
    }
}

impl ContextSnapshot {
    pub fn capture() -> Result<Self> {
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", CAPTURE_SCRIPT])
            .output()
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
        })
    }

    pub fn browser_host_is(&self, host: &str) -> bool {
        // Brave is the first browser adapter. Command semantics intentionally
        // depend on browser host, not this application name.
        self.application.as_deref() == Some("Brave Browser")
            && self
                .browser_url
                .as_ref()
                .and_then(Url::host_str)
                .is_some_and(|current| current == host || current == format!("www.{host}"))
    }

    pub fn application_is(&self, application: &str) -> bool {
        self.application.as_deref() == Some(application)
    }

    pub fn label(&self) -> String {
        match (&self.application, &self.browser_url) {
            (Some(application), Some(url)) => {
                format!(
                    "{application} · {}",
                    url.host_str().unwrap_or("unknown host")
                )
            }
            (Some(application), None) => application.clone(),
            _ => "unknown context".into(),
        }
    }
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
