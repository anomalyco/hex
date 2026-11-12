#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
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

/// How soon the worker retries a refreshed snapshot the one-slot channel held
/// back, instead of waiting the whole poll interval.
const REFRESH_RETRY: Duration = Duration::from_millis(10);

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
    /// Labs › Screen vocabulary: names and terms read from the focused window
    /// as this capture became real. Set only on the snapshot a capture
    /// finishes with, so they never reach another dictation or a meeting.
    pub screen_terms: Vec<String>,
}

/// The page refresh one capture owes. A shortcut's press starts it
/// provisionally: a chord discards it without reading the browser, and only
/// a capture that becomes real asks for one page read, whose finish then
/// waits on it. A `hex dictation start` has no hold to judge and asks at once.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct CaptureRefresh {
    pending: bool,
    /// The ticket this capture requested, once it was real.
    ticket: Option<u64>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl CaptureRefresh {
    /// A capture started. It asks for no page until it is real.
    pub(crate) fn begin(&mut self) {
        self.pending = true;
        self.ticket = None;
    }

    /// The capture became real (held past the hold threshold): its ticket,
    /// requesting one read through `request` on the first call only.
    pub(crate) fn intend(&mut self, request: impl FnOnce() -> u64) -> Option<u64> {
        if !self.pending {
            return None;
        }
        if self.ticket.is_none() {
            self.ticket = Some(request());
        }
        self.ticket
    }

    /// Whether a capture started and has not finished or been cancelled.
    pub(crate) fn pending(&self) -> bool {
        self.pending
    }

    /// The capture ended without transcribing: nothing waits for its page.
    pub(crate) fn cancel(&mut self) {
        self.pending = false;
        self.ticket = None;
    }

    /// The capture ended transcribing: the ticket its finish waits on, or
    /// `None` when no capture was pending. A capture that finished before it
    /// became real (a short tap) asks for its page now.
    pub(crate) fn finish(&mut self, request: impl FnOnce() -> u64) -> Option<u64> {
        let ticket = self.intend(request);
        self.pending = false;
        self.ticket = None;
        ticket
    }
}

pub struct ContextMonitor {
    pub updates: Receiver<ContextSnapshot>,
    /// Wakes the worker for a refresh; one pending request is enough.
    refreshes: SyncSender<()>,
    requested: Arc<AtomicU64>,
    completed: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

/// One cheap, in-process look at the frontmost application. Reading a
/// scriptable browser's page starts `osascript`, so the monitor keeps that
/// page between polls instead of reading it every time.
#[derive(Clone, Debug, Default, PartialEq)]
struct Frontmost {
    application: Option<String>,
    bundle_id: Option<String>,
    pid: i32,
    window_title: Option<String>,
    /// The page an in-process Accessibility read found.
    page: Option<Url>,
    /// A scriptable browser whose page the monitor reads out of process,
    /// caches per pid, and reads again only when the browser changes or a
    /// capture asks for a refresh. False once `reads_browser` turns off, so
    /// the cached page is dropped.
    scripted: bool,
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
        Self::start_with_capture(frontmost, scripted_page, Duration::from_millis(500))
    }

    /// The ticket of a requested page refresh. Never blocks: the pending
    /// request wakes the worker, and `refreshed_since` reports when that
    /// ticket's snapshot is queued for the loop.
    pub fn request_refresh(&self) -> u64 {
        let ticket = self.requested.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self.refreshes.try_send(());
        ticket
    }

    /// Whether the refresh `ticket` completed: its snapshot is queued or was
    /// already delivered. A refresh whose snapshot the one-slot channel
    /// held back isn't complete until it is delivered.
    pub fn refreshed_since(&self, ticket: u64) -> bool {
        self.completed.load(Ordering::Acquire) >= ticket
    }

    /// Starts the polling thread over an injected cheap frontmost read and
    /// page read, so tests can count out-of-process reads without macOS.
    fn start_with_capture(
        mut front: impl FnMut() -> Result<Frontmost> + Send + 'static,
        mut page: impl FnMut(&Frontmost) -> Option<Url> + Send + 'static,
        poll_interval: Duration,
    ) -> Self {
        let (sender, updates) = mpsc::sync_channel(1);
        let (refreshes, refresh_requests) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let requested = Arc::new(AtomicU64::new(0));
        let completed = Arc::new(AtomicU64::new(0));
        let worker_stop = stop.clone();
        let worker_requested = requested.clone();
        let worker_completed = completed.clone();
        let spawned = thread::Builder::new()
            .name("foreground-context".into())
            .spawn(move || {
                let mut previous = None;
                let mut previous_error = None;
                let mut cached: Option<(i32, u64, Option<Url>)> = None;
                while !worker_stop.load(Ordering::Acquire) {
                    let ticket = worker_requested.load(Ordering::Acquire);
                    let refresh = ticket > worker_completed.load(Ordering::Acquire);
                    let context = match front() {
                        Ok(front) => {
                            previous_error = None;
                            let page = if front.scripted {
                                // One out-of-process read per pid and per
                                // refresh ticket, even while its snapshot
                                // waits for the one-slot channel.
                                let cached_page = cached.as_ref().is_some_and(|(pid, read, _)| {
                                    *pid == front.pid && *read >= ticket
                                });
                                if cached_page {
                                    cached.as_ref().and_then(|(_, _, page)| page.clone())
                                } else {
                                    let page = page(&front);
                                    cached = Some((front.pid, ticket, page.clone()));
                                    page
                                }
                            } else {
                                // `reads_browser` is off, or the page is
                                // read in process: nothing to keep.
                                cached = None;
                                None
                            };
                            snapshot_of(&front, page)
                        }
                        Err(error) => {
                            let error = error.to_string();
                            if previous_error.as_ref() != Some(&error) {
                                tracing::warn!(%error, "could not capture foreground context");
                                previous_error = Some(error);
                            }
                            ContextSnapshot::default()
                        }
                    };
                    // A refreshed snapshot the full slot held back is
                    // captured again soon: a newer value supersedes it, and
                    // the ticket completes only once one is delivered.
                    let blocked_refresh = match publish_context(&sender, &mut previous, context) {
                        Published::Disconnected => break,
                        Published::Blocked => refresh,
                        Published::Sent | Published::Current => {
                            if refresh {
                                worker_completed.fetch_max(ticket, Ordering::AcqRel);
                            }
                            false
                        }
                    };
                    let wait = if blocked_refresh {
                        REFRESH_RETRY
                    } else {
                        poll_interval
                    };
                    match refresh_requests.recv_timeout(wait) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the foreground context monitor");
            // A monitor with no worker never holds a capture's finish.
            completed.store(u64::MAX, Ordering::Release);
        }
        Self {
            updates,
            refreshes,
            requested,
            completed,
            stop,
        }
    }
}

fn snapshot_of(front: &Frontmost, page: Option<Url>) -> ContextSnapshot {
    ContextSnapshot {
        application: front.application.clone(),
        browser_url: page.or_else(|| front.page.clone()),
        window_title: front.window_title.clone(),
        input_revision: None,
        screen_terms: Vec::new(),
    }
}

enum Published {
    Sent,
    /// The last sent snapshot is this one, so it already sits in the slot or
    /// was delivered.
    Current,
    /// The one-slot channel is full; a later poll tries again.
    Blocked,
    Disconnected,
}

fn publish_context(
    sender: &SyncSender<ContextSnapshot>,
    previous: &mut Option<ContextSnapshot>,
    context: ContextSnapshot,
) -> Published {
    if previous.as_ref() == Some(&context) {
        return Published::Current;
    }
    match sender.try_send(context.clone()) {
        Ok(()) => {
            *previous = Some(context);
            Published::Sent
        }
        Err(TrySendError::Full(_)) => Published::Blocked,
        Err(TrySendError::Disconnected(_)) => Published::Disconnected,
    }
}

impl Drop for ContextMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl ContextSnapshot {
    #[cfg(target_os = "macos")]
    pub fn capture_for_meeting(canonical_bundle_id: &str) -> Result<Self> {
        if let Some(browser) = ScriptableBrowser::from_bundle_id(canonical_bundle_id) {
            match browser.meeting_tab() {
                Ok(Some((browser_url, window_title))) => {
                    return Ok(Self {
                        application: Some(browser.name.into()),
                        browser_url: Some(browser_url),
                        window_title,
                        input_revision: None,
                        screen_terms: Vec::new(),
                    });
                }
                Ok(None) => {}
                Err(error) => browser.warn_once(&error),
            }
            // The dictionary didn't answer (Automation not allowed, a
            // browser whose script fails): read the page from the browser's
            // own web area through Accessibility, even behind another app.
            let pid = autoreleasepool(|_| {
                crate::microphone_activity::running_application_by_bundle_id(browser.bundle_id)
                    .map(|(pid, _)| pid)
            });
            // Chromium browsers behind another app expose no focused element,
            // and Arc's chrome sits between its windows and the page, so every
            // window's web areas and titles are read (`browser_page`), and a
            // Meet tab's title still names the call when no address does
            // (#827, #831). The browser's own window is reported rather than
            // the app in front.
            if let Some(pid) = pid {
                let pages = crate::accessibility::browser_pages(pid);
                let more_call_apps = crate::labs::more_call_apps();
                let (url, title) = crate::dictation::browser_page::choose(
                    &pages,
                    |url| {
                        web_url(url).is_some_and(|url| {
                            crate::meeting::detection::names_call(&url, more_call_apps)
                        })
                    },
                    crate::meeting::detection::title_names_call,
                );
                // A call's tab title outranks another tab's address.
                let title_names_call = title
                    .as_deref()
                    .is_some_and(crate::meeting::detection::title_names_call);
                let url = url.as_deref().and_then(web_url).filter(|url| {
                    crate::meeting::detection::names_call(url, more_call_apps) || !title_names_call
                });
                return Ok(Self {
                    application: Some(browser.name.into()),
                    browser_url: url,
                    window_title: title,
                    input_revision: None,
                    screen_terms: Vec::new(),
                });
            }
        }
        let front = capture_frontmost(Reads {
            page: true,
            window_title: true,
        })?;
        let page = if front.scripted {
            scripted_page(&front)
        } else {
            front.page.clone()
        };
        Ok(snapshot_of(&front, page))
    }

    #[cfg(not(target_os = "macos"))]
    pub fn capture_for_meeting(_canonical_bundle_id: &str) -> Result<Self> {
        Ok(Self::default())
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

/// What a frontmost read takes beyond the app itself. Asking a browser for
/// its page can show an Automation prompt, and a window title can hold an
/// email subject or a document name, so only callers that use them ask.
#[cfg(target_os = "macos")]
struct Reads {
    page: bool,
    window_title: bool,
}

#[cfg(target_os = "macos")]
fn frontmost() -> Result<Frontmost> {
    capture_frontmost(Reads {
        page: crate::dictation::config::reads_browser(),
        window_title: crate::dictation::config::reads_window_title(),
    })
}

#[cfg(not(target_os = "macos"))]
fn frontmost() -> Result<Frontmost> {
    Ok(Frontmost::default())
}

#[cfg(target_os = "macos")]
fn capture_frontmost(reads: Reads) -> Result<Frontmost> {
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
    // A scriptable browser's page is read out of process and cached, so its
    // Accessibility fallback runs with that read instead of every poll.
    let scripted = reads.page
        && bundle_id
            .as_deref()
            .and_then(ScriptableBrowser::from_bundle_id)
            .is_some();
    let page = (reads.page && !scripted)
        .then(|| accessibility_web_url(pid))
        .flatten();
    let window_title = reads
        .window_title
        .then(|| crate::accessibility::focused_window_title(pid))
        .flatten();
    Ok(Frontmost {
        application: Some(application),
        bundle_id,
        pid,
        window_title,
        page,
        scripted,
    })
}

/// A scriptable browser's page: its front tab over AppleScript, else the
/// focused web area through Accessibility. Runs only when the browser
/// changes or a capture asks for a refresh.
#[cfg(target_os = "macos")]
fn scripted_page(front: &Frontmost) -> Option<Url> {
    front
        .bundle_id
        .as_deref()
        .and_then(ScriptableBrowser::from_bundle_id)
        .and_then(ScriptableBrowser::active_url)
        .or_else(|| accessibility_web_url(front.pid))
}

#[cfg(not(target_os = "macos"))]
fn scripted_page(_front: &Frontmost) -> Option<Url> {
    None
}

/// The page of the web area holding `pid`'s focused element, through
/// Accessibility.
#[cfg(target_os = "macos")]
fn accessibility_web_url(pid: i32) -> Option<Url> {
    crate::accessibility::focused_web_url(pid).and_then(|url| web_url(&url))
}

/// An http(s) page address.
#[cfg(target_os = "macos")]
fn web_url(url: &str) -> Option<Url> {
    Url::parse(url)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
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

/// The frontmost application's display name, without reading a page or a
/// window title: a dictation mode's application name (Labs › Switch model
/// with app). `None` when Hex itself owns the frontmost application, since
/// its own window names no mode, or when macOS reports none.
#[cfg(target_os = "macos")]
pub fn frontmost_other_application() -> Option<String> {
    autoreleasepool(|_| {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        if application.processIdentifier() == std::process::id() as i32 {
            return None;
        }
        application.localizedName().map(|name| name.to_string())
    })
}

/// The frontmost application's process ID, for Accessibility reads.
#[cfg(target_os = "macos")]
pub fn frontmost_pid() -> Option<i32> {
    autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|application| application.processIdentifier())
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
                self.warn_once(&error);
                None
            }
        }
    }

    /// Logs a failed tab read once per browser.
    fn warn_once(self, error: &color_eyre::Report) {
        static WARNED: std::sync::Mutex<Vec<&str>> = std::sync::Mutex::new(Vec::new());
        let mut warned = WARNED.lock().unwrap_or_else(|error| error.into_inner());
        if !warned.contains(&self.bundle_id) {
            warned.push(self.bundle_id);
            tracing::warn!(
                error = format!("{error:#}"),
                "could not read the browser tab"
            );
        }
    }

    /// The front window's tab, even when another app is in front.
    /// Background tabs are deliberately not searched: a call open in one
    /// says nothing about which tab is using the microphone. The tab is
    /// named through the front window each time rather than kept in a
    /// variable: Arc can't resolve the stored tab-by-id reference (#1113).
    fn meeting_tab(self) -> Result<Option<(Url, Option<String>)>> {
        let (tab, title) = self.tab();
        let script = format!(
            "tell application id \"{}\"\n    if (count of windows) is 0 then return \"\"\n    return (URL of {tab} of front window) & linefeed & ({title} of {tab} of front window)\nend tell",
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
    use std::cell::Cell;
    use std::sync::atomic::{AtomicI32, AtomicUsize};
    use std::time::Instant;

    fn next(monitor: &ContextMonitor) -> ContextSnapshot {
        monitor
            .updates
            .recv_timeout(Duration::from_secs(1))
            .expect("the monitor published nothing")
    }

    #[test]
    fn a_press_discarded_as_a_chord_never_reads_the_page() {
        let mut capture = CaptureRefresh::default();
        let reads = Cell::new(0);
        let request = || {
            reads.set(reads.get() + 1);
            reads.get()
        };

        // A provisional press a chord discards ends before it is real.
        capture.begin();
        capture.cancel();

        assert_eq!(capture.intend(request), None);
        assert_eq!(capture.finish(request), None);
        assert_eq!(reads.get(), 0, "a discarded press read the browser page");
    }

    #[test]
    fn a_real_capture_reads_the_page_once() {
        let mut capture = CaptureRefresh::default();
        let reads = Cell::new(0);
        let request = || {
            reads.set(reads.get() + 1);
            reads.get()
        };

        capture.begin();
        // The hold threshold asks once; later calls only return the ticket.
        assert_eq!(capture.intend(request), Some(1));
        assert_eq!(capture.intend(request), Some(1));
        assert_eq!(capture.finish(request), Some(1));
        assert_eq!(reads.get(), 1);
        // The capture is over: later intent asks for nothing.
        assert_eq!(capture.intend(request), None);
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn a_finish_before_the_hold_threshold_reads_the_page_then() {
        let mut capture = CaptureRefresh::default();
        let reads = Cell::new(0);
        let request = || {
            reads.set(reads.get() + 1);
            reads.get()
        };

        // A short tap finishes before the loop ever sees it as intentional.
        capture.begin();
        assert_eq!(capture.finish(request), Some(1));
        // The next capture asks for its own page again.
        capture.begin();
        assert_eq!(capture.intend(request), Some(2));
        assert_eq!(capture.finish(request), Some(2));
        assert_eq!(reads.get(), 2);
    }

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
        let captured = Frontmost {
            application: Some("Brave Browser".into()),
            bundle_id: Some("com.brave.Browser".into()),
            pid: 10,
            page: Some(Url::parse("https://x.com/home").unwrap()),
            ..Frontmost::default()
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let capture_calls = calls.clone();
        let expected = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(Url::parse("https://x.com/home").unwrap()),
            ..ContextSnapshot::default()
        };
        let monitor = ContextMonitor::start_with_capture(
            move || {
                if capture_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(captured.clone())
                } else {
                    Err(eyre!("browser adapter failed"))
                }
            },
            |_| unreachable!("an in-process page is never read out of process"),
            Duration::from_millis(1),
        );

        assert_eq!(next(&monitor), expected);
        assert_eq!(next(&monitor), ContextSnapshot::default());
    }

    #[test]
    fn the_scripted_page_is_read_once_per_pid_and_on_refresh_only() {
        let pid = Arc::new(AtomicI32::new(10));
        let front_pid = pid.clone();
        let reads = Arc::new(AtomicUsize::new(0));
        let page_reads = reads.clone();
        let monitor = ContextMonitor::start_with_capture(
            move || {
                Ok(Frontmost {
                    application: Some("Brave Browser".into()),
                    bundle_id: Some("com.brave.Browser".into()),
                    pid: front_pid.load(Ordering::Relaxed),
                    scripted: true,
                    ..Frontmost::default()
                })
            },
            move |front| {
                let read = page_reads.fetch_add(1, Ordering::SeqCst) + 1;
                Some(Url::parse(&format!("https://x.com/{}/{}", front.pid, read)).unwrap())
            },
            Duration::from_millis(1),
        );

        let first = next(&monitor);
        assert_eq!(first.browser_url.as_ref().unwrap().path(), "/10/1");

        // Polling the same frontmost app never asks the browser again.
        thread::sleep(Duration::from_millis(100));
        assert_eq!(reads.load(Ordering::SeqCst), 1);

        // A new frontmost pid reads once.
        pid.store(11, Ordering::Relaxed);
        let changed = next(&monitor);
        assert_eq!(changed.browser_url.as_ref().unwrap().path(), "/11/2");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(reads.load(Ordering::SeqCst), 2);

        // An explicit refresh reads again and completes its ticket.
        let ticket = monitor.request_refresh();
        let refreshed = next(&monitor);
        assert_eq!(refreshed.browser_url.as_ref().unwrap().path(), "/11/3");
        let deadline = Instant::now() + Duration::from_secs(1);
        while !monitor.refreshed_since(ticket) {
            assert!(Instant::now() < deadline, "the refresh never completed");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_refresh_the_full_channel_held_back_completes_once_delivered() {
        let phase = Arc::new(AtomicI32::new(0));
        let front_phase = phase.clone();
        let observed = Arc::new(AtomicUsize::new(0));
        let front_observed = observed.clone();
        let reads = Arc::new(AtomicUsize::new(0));
        let page_reads = reads.clone();
        let monitor = ContextMonitor::start_with_capture(
            move || {
                let phase = front_phase.load(Ordering::Relaxed);
                if phase != 0 {
                    front_observed.fetch_add(1, Ordering::SeqCst);
                }
                let application = if phase == 0 {
                    "Brave Browser"
                } else {
                    "Brave Browser Beta"
                };
                Ok(Frontmost {
                    application: Some(application.into()),
                    pid: 10,
                    scripted: true,
                    ..Frontmost::default()
                })
            },
            move |_| {
                let read = page_reads.fetch_add(1, Ordering::SeqCst) + 1;
                Some(Url::parse(&format!("https://x.com/{read}")).unwrap())
            },
            Duration::from_millis(1),
        );

        let first = next(&monitor);
        assert_eq!(first.browser_url.as_ref().unwrap().path(), "/1");

        // Leave a later snapshot filling the one slot, then refresh into it:
        // wait until the worker's own read saw the changed app, so the stale
        // snapshot is published before the refresh is requested.
        phase.store(1, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(1);
        while observed.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "the worker never saw the change");
            thread::sleep(Duration::from_millis(1));
        }
        let ticket = monitor.request_refresh();
        let deadline = Instant::now() + Duration::from_secs(1);
        while reads.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline, "the refresh never read the page");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            !monitor.refreshed_since(ticket),
            "a refreshed snapshot still in the full slot is not delivered"
        );

        // The stale page first, then the refreshed one; only then is the
        // refresh complete.
        let stale = next(&monitor);
        assert_eq!(stale.application.as_deref(), Some("Brave Browser Beta"));
        assert_eq!(stale.browser_url.as_ref().unwrap().path(), "/1");
        let refreshed = next(&monitor);
        assert_eq!(refreshed.application.as_deref(), Some("Brave Browser Beta"));
        assert_eq!(refreshed.browser_url.as_ref().unwrap().path(), "/2");
        let deadline = Instant::now() + Duration::from_secs(1);
        while !monitor.refreshed_since(ticket) {
            assert!(Instant::now() < deadline, "the refresh never completed");
            thread::sleep(Duration::from_millis(5));
        }
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
