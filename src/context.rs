#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TrySendError};
use std::thread;
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

use color_eyre::eyre::Result;
#[cfg(target_os = "macos")]
use color_eyre::eyre::WrapErr;
#[cfg(any(target_os = "macos", test))]
use color_eyre::eyre::eyre;
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
    pub input_revision: Option<u64>,
}

pub struct ContextMonitor {
    pub updates: Receiver<ContextSnapshot>,
    stop: Arc<AtomicBool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextSelector {
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
            Self::BrowserHost(host) => context.browser_host_is(host),
            Self::Application(application) => context.application_is(application),
        }
    }

    pub fn is_browser(&self) -> bool {
        matches!(self, Self::BrowserHost(_))
    }
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

    #[cfg(target_os = "macos")]
    pub fn capture_for_meeting(canonical_bundle_id: &str) -> Result<Self> {
        if let Some(browser) = ScriptableBrowser::from_bundle_id(canonical_bundle_id)
            && let Some((browser_url, window_title)) = browser.meeting_tab()?
        {
            return Ok(Self {
                application: Some(browser.name.into()),
                browser_url: Some(browser_url),
                window_title,
                input_revision: None,
            });
        }
        capture_macos()
    }

    #[cfg(not(target_os = "macos"))]
    pub fn capture_for_meeting(_canonical_bundle_id: &str) -> Result<Self> {
        Self::capture()
    }

    pub fn browser_host(&self) -> Option<&str> {
        self.browser_url.as_ref().and_then(Url::host_str)
    }

    /// Command semantics depend on the browser host, never the browser.
    pub fn browser_host_is(&self, host: &str) -> bool {
        self.browser_host()
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
    let (application, bundle_id, pid) = autoreleasepool(|_| {
        let application = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .ok_or_else(|| eyre!("macOS did not report a foreground application"))?;
        let name = application
            .localizedName()
            .map(|name| name.to_string())
            .ok_or_else(|| eyre!("the foreground application has no display name"))?;
        let bundle_id = application.bundleIdentifier().map(|id| id.to_string());
        Ok::<_, color_eyre::Report>((name, bundle_id, application.processIdentifier()))
    })?;
    let scripted = bundle_id
        .as_deref()
        .and_then(ScriptableBrowser::from_bundle_id)
        .and_then(ScriptableBrowser::active_url);
    let browser_url = scripted.or_else(|| {
        crate::accessibility::focused_web_url(pid)
            .and_then(|url| Url::parse(&url).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
    });
    Ok(ContextSnapshot {
        application: Some(application),
        browser_url,
        window_title: crate::accessibility::focused_window_title(pid),
        input_revision: None,
    })
}

#[cfg(target_os = "macos")]
pub fn frontmost_bundle_path() -> Option<std::path::PathBuf> {
    autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()?
            .bundleURL()?
            .path()
            .map(|path| path.to_string().into())
    })
}

/// How a browser's AppleScript dictionary names the front window's tab.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabDialect {
    /// `active tab`, with `title`: Chrome and its forks.
    Chromium,
    /// `current tab`, with `name`: Safari and Orion.
    Safari,
}

/// A browser whose front tab is read through AppleScript. Browsers without
/// a dictionary (Firefox, Zen) use the Accessibility web area instead.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug)]
struct ScriptableBrowser {
    bundle_id: &'static str,
    name: &'static str,
    #[cfg(target_os = "macos")]
    dialect: TabDialect,
}

#[cfg(any(target_os = "macos", test))]
macro_rules! browsers {
    ($(($bundle_id:literal, $name:literal, $dialect:ident)),* $(,)?) => {
        &[$(ScriptableBrowser {
            bundle_id: $bundle_id,
            name: $name,
            #[cfg(target_os = "macos")]
            dialect: TabDialect::$dialect,
        }),*]
    };
}

#[cfg(any(target_os = "macos", test))]
const SCRIPTABLE_BROWSERS: &[ScriptableBrowser] = browsers![
    ("com.apple.Safari", "Safari", Safari),
    (
        "com.apple.SafariTechnologyPreview",
        "Safari Technology Preview",
        Safari
    ),
    ("com.brave.Browser", "Brave Browser", Chromium),
    ("com.brave.Browser.beta", "Brave Browser Beta", Chromium),
    (
        "com.brave.Browser.nightly",
        "Brave Browser Nightly",
        Chromium
    ),
    ("com.google.Chrome", "Google Chrome", Chromium),
    ("com.google.Chrome.beta", "Google Chrome Beta", Chromium),
    ("com.google.Chrome.canary", "Google Chrome Canary", Chromium),
    ("com.google.Chrome.dev", "Google Chrome Dev", Chromium),
    ("com.kagi.kagimacOS", "Orion", Safari),
    ("com.microsoft.edgemac", "Microsoft Edge", Chromium),
    ("com.operasoftware.Opera", "Opera", Chromium),
    ("com.vivaldi.Vivaldi", "Vivaldi", Chromium),
    ("company.thebrowser.Browser", "Arc", Chromium),
    ("net.imput.helium", "Helium", Chromium),
    ("org.chromium.Chromium", "Chromium", Chromium),
];

#[cfg(any(target_os = "macos", test))]
impl ScriptableBrowser {
    fn from_bundle_id(bundle_id: &str) -> Option<Self> {
        SCRIPTABLE_BROWSERS
            .iter()
            .find(|browser| browser.bundle_id.eq_ignore_ascii_case(bundle_id))
            .copied()
    }
}

#[cfg(target_os = "macos")]
impl ScriptableBrowser {
    fn tab(self) -> (&'static str, &'static str) {
        match self.dialect {
            TabDialect::Chromium => ("active tab", "title"),
            TabDialect::Safari => ("current tab", "name"),
        }
    }

    /// The front tab's address. A failure (Automation denied, a hung
    /// browser) is logged once per browser and leaves the application
    /// context intact.
    fn active_url(self) -> Option<Url> {
        let (tab, _) = self.tab();
        let script = format!(
            "tell application id \"{}\"\n    if (count of windows) > 0 then return URL of {tab} of front window\nend tell\nreturn \"\"",
            self.bundle_id
        );
        match self.run(&script) {
            Ok(url) => Url::parse(url.trim()).ok(),
            Err(error) => {
                static WARNED: std::sync::Mutex<Vec<&str>> = std::sync::Mutex::new(Vec::new());
                let mut warned = WARNED.lock().unwrap_or_else(|error| error.into_inner());
                if !warned.contains(&self.bundle_id) {
                    warned.push(self.bundle_id);
                    tracing::warn!(
                        error = format!("{error:#}"),
                        "could not read the browser tab"
                    );
                }
                None
            }
        }
    }

    /// The front window's tab, even when another app is in front.
    /// Background tabs are deliberately not searched: a call open in one
    /// says nothing about which tab is using the microphone.
    fn meeting_tab(self) -> Result<Option<(Url, Option<String>)>> {
        let (tab, title) = self.tab();
        let script = format!(
            "tell application id \"{}\"\n    if (count of windows) is 0 then return \"\"\n    set frontTab to {tab} of front window\n    return (URL of frontTab) & linefeed & ({title} of frontTab)\nend tell",
            self.bundle_id
        );
        Ok(parse_tab_output(&self.run(&script)?))
    }

    fn run(self, script: &str) -> Result<String> {
        run_osascript(script)
            .wrap_err_with(|| format!("could not inspect the active {} tab", self.name))
    }
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String> {
    const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("could not start osascript")?;
    let started = Instant::now();
    loop {
        if child.try_wait().wrap_err("osascript failed")?.is_some() {
            break;
        }
        if started.elapsed() >= CAPTURE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(eyre!("osascript timed out"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().wrap_err("osascript failed")?;
    if !output.status.success() {
        return Err(eyre!(
            "osascript failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(any(target_os = "macos", test))]
fn parse_tab_output(output: &str) -> Option<(Url, Option<String>)> {
    let mut lines = output.lines();
    let url = Url::parse(lines.next()?.trim()).ok()?;
    let title = lines.collect::<Vec<_>>().join(" ");
    let title = title.trim();
    Some((url, (!title.is_empty()).then(|| title.to_string())))
}

fn normalize_browser_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

fn application_names_equal(left: &str, right: &str) -> bool {
    strip_bundle_extension(left).eq_ignore_ascii_case(strip_bundle_extension(right))
}

/// Removes a trailing `.app` from an application name.
///
/// `NSFileManager.displayNameAtPath` honors Finder's "Show all filename
/// extensions" preference and then returns `Ghostty.app`, while the frontmost
/// application's `localizedName` is always `Ghostty`. Names are normalized
/// before they are stored and again when compared so settings written by
/// either form keep matching.
pub fn strip_bundle_extension(name: &str) -> &str {
    let bytes = name.as_bytes();
    match bytes.len().checked_sub(4) {
        Some(split) if split > 0 && bytes[split..].eq_ignore_ascii_case(b".app") => &name[..split],
        _ => name,
    }
}

pub(crate) fn browser_hosts_equal(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
}

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
    fn application_matching_ignores_finder_bundle_extensions() {
        let context = ContextSnapshot {
            application: Some("Ghostty".into()),
            ..ContextSnapshot::default()
        };

        assert!(ContextSelector::application("Ghostty.app").matches(&context));
        assert!(ContextSelector::application("ghostty.APP").matches(&context));
    }

    #[test]
    fn bundle_extension_stripping_only_removes_a_trailing_suffix() {
        assert_eq!(strip_bundle_extension("Ghostty.app"), "Ghostty");
        assert_eq!(
            strip_bundle_extension("T3 Code (Nightly).app"),
            "T3 Code (Nightly)"
        );
        assert_eq!(strip_bundle_extension("Ghostty"), "Ghostty");
        assert_eq!(strip_bundle_extension("app.application"), "app.application");
        assert_eq!(strip_bundle_extension(".app"), ".app");
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

    #[test]
    fn scriptable_browsers_are_found_by_bundle_id() {
        assert_eq!(
            ScriptableBrowser::from_bundle_id("com.google.Chrome").map(|browser| browser.name),
            Some("Google Chrome")
        );
        assert_eq!(
            ScriptableBrowser::from_bundle_id("com.brave.browser").map(|browser| browser.name),
            Some("Brave Browser")
        );
        assert!(ScriptableBrowser::from_bundle_id("org.mozilla.firefox").is_none());
        assert!(
            SCRIPTABLE_BROWSERS
                .windows(2)
                .all(|pair| pair[0].bundle_id < pair[1].bundle_id)
        );
    }

    #[test]
    fn browser_hosts_match_in_any_browser() {
        let context = ContextSnapshot {
            application: Some("Google Chrome".into()),
            browser_url: Some(Url::parse("https://X.com./home").unwrap()),
            ..ContextSnapshot::default()
        };
        assert!(ContextSelector::browser_host("x.com").matches(&context));
    }

    #[test]
    fn parses_tab_url_and_title_output() {
        assert_eq!(
            parse_tab_output("https://meet.google.com/abc-defg-hij\nDesign sync - Google Meet\n"),
            Some((
                Url::parse("https://meet.google.com/abc-defg-hij").unwrap(),
                Some("Design sync - Google Meet".into()),
            ))
        );
        assert_eq!(
            parse_tab_output("https://meet.google.com/abc-defg-hij\n"),
            Some((
                Url::parse("https://meet.google.com/abc-defg-hij").unwrap(),
                None,
            ))
        );
        assert_eq!(parse_tab_output(""), None);
    }
}
