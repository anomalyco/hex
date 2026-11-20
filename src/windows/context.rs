//! Foreground context for dictation on Windows: the focused application's
//! executable stem, and for known browsers the page URL read through UI
//! Automation, so web-domain mode rules see the
//! real host. Chromium-family browsers and Firefox expose the page URL on
//! the UIA Document element's value pattern, which is locale-independent.
//! Meeting detection reads the same pages from a microphone-holding
//! browser's own windows, found by executable because the Chromium utility
//! process that holds the microphone has none. The UIA reads run on bounded
//! helper threads; a slow or hung accessibility provider degrades to
//! application-only context.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use url::Url;
use windows::Win32::UI::Accessibility::{IUIAutomation, IUIAutomationElement};
use windows_sys::Win32::Foundation::{HWND, LPARAM};

use crate::dictation::browser_page::BrowserPages;
use crate::dictation::context::ContextSnapshot;
use crate::dictation::modes::normalize_host;

/// Executable stems whose foreground document exposes the page URL.
const BROWSER_STEMS: &[&str] = &[
    "arc",
    "brave",
    "chrome",
    "chromium",
    "firefox",
    "helium",
    "librewolf",
    "msedge",
    "opera",
    "opera_gx",
    "thorium",
    "vivaldi",
    "zen",
];

const URL_READ_DEADLINE: Duration = Duration::from_millis(300);

/// A context read in flight: the application stem is immediate, the UIA
/// URL read proceeds on a helper thread while the caller transcribes.
#[derive(Default)]
pub struct PendingContext {
    application: Option<String>,
    url: Option<mpsc::Receiver<Option<String>>>,
}

/// Begin reading the foreground context at the moment the dictation is
/// submitted; `finish` collects the browser host after inference, hiding
/// the UIA walk behind it. The page is read only when `read_page` says
/// processing uses it (`ModeRuntime::reads_page`): a UIA client turns on a
/// Chromium browser's accessibility tree, and `finish` waits for the walk.
pub fn begin_capture(read_page: bool) -> PendingContext {
    let window = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    let application = crate::windows::input::window_process_path(window).and_then(|path| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().to_lowercase())
    });
    let url = application
        .as_deref()
        .filter(|stem| read_page && BROWSER_STEMS.contains(stem))
        .and_then(|_| spawn_url_read(window as isize));
    PendingContext { application, url }
}

impl PendingContext {
    pub fn finish(self) -> ContextSnapshot {
        let browser_url = self
            .url
            .and_then(|receiver| match receiver.recv_timeout(URL_READ_DEADLINE) {
                Ok(url) => url,
                Err(_) => {
                    tracing::warn!(
                        "the browser URL read did not finish in time; using application-only context"
                    );
                    None
                }
            })
            .as_deref()
            .and_then(browser_url);
        ContextSnapshot {
            application: self.application,
            browser_url,
            window_title: None,
            input_revision: None,
            screen_terms: Vec::new(),
            caret_text: None,
            bundle_name: None,
        }
    }
}

/// How long a meeting page read waits before falling back to the window
/// titles. The watcher loop is already shielded: its `TabProbe` runs the
/// query on a thread of its own, so this bounds that thread's wait for the
/// accessibility provider, not the loop.
const MEETING_READ_DEADLINE: Duration = Duration::from_secs(2);

/// How many of the browser's windows one meeting probe reads. A browser with
/// more open windows than this rarely has its call in the rest, and each
/// window costs a UI Automation walk.
const MAX_MEETING_WINDOWS: usize = 6;

/// The page a browser holding the microphone shows, for meeting detection:
/// the addresses and titles of every visible window of its executable, since
/// Chromium-family browsers behind another app expose a page only through
/// their own windows' UI Automation Documents. A page or window title naming
/// a call (a Google Meet call, a Teams page) reports that call's site; a
/// browser with no readable call page keeps the "Call in <browser>" fallback.
pub(crate) fn capture_for_meeting(
    canonical_bundle_id: &str,
) -> color_eyre::Result<ContextSnapshot> {
    let Some(application) =
        crate::microphone_activity::canonical_application_name(canonical_bundle_id)
    else {
        return Ok(ContextSnapshot::default());
    };
    let windows = meeting_browser_windows(canonical_bundle_id);
    if windows.is_empty() {
        return Ok(ContextSnapshot::default());
    }
    let mut pages = BrowserPages::default();
    for &window in &windows {
        // A window title is a whole window's, not one tab's: its browser
        // name, Edge's profile and Edge's other-tab count are removed before
        // detection reads it.
        pages.push_title(
            window_title(window)
                .map(|title| crate::meeting::detection::normalize_window_title(&title)),
        );
    }
    if let Some(receiver) = spawn_meeting_read(windows) {
        match receiver.recv_timeout(MEETING_READ_DEADLINE) {
            Ok(urls) => {
                for url in urls {
                    pages.push_url(Some(url));
                }
            }
            Err(_) => tracing::warn!(
                "the browser page read did not finish in time; using the window titles"
            ),
        }
    }
    let (browser_url, window_title) = crate::meeting::detection::call_page(&pages);
    Ok(ContextSnapshot {
        application: Some(application.into()),
        browser_url,
        window_title,
        ..Default::default()
    })
}

/// The browser's visible top-level windows, non-minimized first and in
/// z-order. A window belongs to the browser when its executable's canonical
/// id is `canonical_bundle_id`: the process that holds a Chromium browser's
/// microphone is an audio utility process with no window, so windows are
/// matched by executable rather than by process.
fn meeting_browser_windows(canonical_bundle_id: &str) -> Vec<HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsIconic;

    let mut shown = Vec::new();
    let mut minimized = Vec::new();
    for window in taskbar_windows() {
        let Some(stem) = crate::windows::input::window_process_path(window).and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase())
        }) else {
            continue;
        };
        if crate::microphone_activity::canonical_bundle_id_for(&stem, None)
            != Some(canonical_bundle_id)
        {
            continue;
        }
        // SAFETY: the handle came from EnumWindows and is only queried.
        if unsafe { IsIconic(window) } != 0 {
            minimized.push(window);
        } else {
            shown.push(window);
        }
    }
    shown
        .into_iter()
        .chain(minimized)
        .take(MAX_MEETING_WINDOWS)
        .collect()
}

/// The top-level windows showing an application in the taskbar: visible,
/// titled, unowned, and not tool windows. The application catalog lists their
/// executables; meeting detection searches a browser's for its call.
pub(crate) fn taskbar_windows() -> Vec<HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
        IsWindowVisible, WS_EX_TOOLWINDOW,
    };

    unsafe extern "system" fn collect(window: HWND, windows: LPARAM) -> i32 {
        // SAFETY: `taskbar_windows` passes the address of its own vector.
        let windows = unsafe { &mut *(windows as *mut Vec<HWND>) };
        windows.push(window);
        1
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: `collect` only appends the handles EnumWindows yields, and
    // `windows` outlives the call.
    unsafe {
        EnumWindows(Some(collect), (&raw mut windows) as LPARAM);
    }
    windows
        .into_iter()
        .filter(|&window| {
            // SAFETY: the handle came from EnumWindows and is only queried.
            unsafe {
                IsWindowVisible(window) != 0
                    && GetWindowTextLengthW(window) > 0
                    && GetWindow(window, GW_OWNER).is_null()
                    && GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0
            }
        })
        .collect()
}

/// A window's title, as the taskbar shows it.
fn window_title(window: HWND) -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    // SAFETY: the handle came from EnumWindows; the buffer holds `length + 1`
    // UTF-16 units, the most GetWindowTextW writes.
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0_u16; length as usize + 1];
    let read = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
    if read <= 0 {
        return None;
    }
    let title = String::from_utf16_lossy(&buffer[..read as usize]);
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

fn browser_url(url: &str) -> Option<Url> {
    let mut url = Url::parse(url).ok()?;
    let host = normalize_host(url.host_str()?);
    url.set_host(Some(&host)).ok()?;
    Some(url)
}

#[cfg(test)]
fn host_of(url: &str) -> Option<String> {
    browser_url(url).and_then(|url| url.host_str().map(normalize_host))
}

/// At most one UIA read runs at a time per guard: a hung accessibility
/// provider blocks its helper thread until it answers, and spawning more
/// would pile blocked threads one per read. Dictation and meeting detection
/// keep their own guards so neither starves the other.
static URL_READ_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static MEETING_READ_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Runs `read` on a named helper thread, returning its answer's receiver.
/// `None` while a previous read under the same `in_flight` guard is still
/// blocked.
fn spawn_read<T: Send + 'static>(
    thread_name: &str,
    in_flight: &'static AtomicBool,
    read: impl FnOnce() -> T + Send + 'static,
) -> Option<mpsc::Receiver<T>> {
    if in_flight.swap(true, Ordering::AcqRel) {
        tracing::debug!(
            thread_name,
            "skipping a UI Automation read; a previous read is still blocked"
        );
        return None;
    }
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let answer = read();
            in_flight.store(false, Ordering::Release);
            let _ = sender.send(answer);
        });
    match spawned {
        Ok(_) => Some(receiver),
        Err(_) => {
            in_flight.store(false, Ordering::Release);
            None
        }
    }
}

fn spawn_url_read(window: isize) -> Option<mpsc::Receiver<Option<String>>> {
    spawn_read("windows-uia-context", &URL_READ_IN_FLIGHT, move || {
        let url = read_document_url(window);
        if url.is_none() {
            tracing::debug!("the foreground browser exposed no page URL");
        }
        url
    })
}

fn spawn_meeting_read(windows: Vec<HWND>) -> Option<mpsc::Receiver<Vec<String>>> {
    let windows: Vec<isize> = windows.into_iter().map(|window| window as isize).collect();
    spawn_read("windows-uia-meeting", &MEETING_READ_IN_FLIGHT, move || {
        read_pages(windows)
    })
}

/// Reads each window's page in order, stopping at the first address that
/// names a call. One UI Automation instance serves every window, so a
/// meeting probe pays for COM and the automation object once, not per
/// window.
fn read_pages(windows: Vec<isize>) -> Vec<String> {
    let urls = with_documents(|automation| {
        let mut urls = Vec::new();
        for window in windows {
            let Some(url) = document_url(automation, window) else {
                continue;
            };
            let names_call =
                browser_url(&url).is_some_and(|url| crate::meeting::detection::names_call(&url));
            urls.push(url);
            if names_call {
                break;
            }
        }
        urls
    });
    urls.unwrap_or_default()
}

/// The page URL of `window`'s web area, through UI Automation. The foreground
/// context read of a dictation uses this too, so COM is initialized (and
/// released) here.
fn read_document_url(window: isize) -> Option<String> {
    with_documents(|automation| document_url(automation, window))?
}

/// Runs `read` with one UI Automation instance on this helper thread,
/// releasing COM afterwards; `None` when COM or the instance is unavailable.
fn with_documents<T>(read: impl FnOnce(&IUIAutomation) -> T) -> Option<T> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::CUIAutomation;

    // SAFETY: COM belongs to the calling helper thread, and the instance is
    // created, used, and released before COM is uninitialized.
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: windows::core::Result<IUIAutomation> =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER);
        let answer = match automation {
            Ok(automation) => {
                let answer = read(&automation);
                drop(automation);
                Some(answer)
            }
            Err(_) => None,
        };
        if com.is_ok() {
            CoUninitialize();
        }
        answer
    }
}

/// How many elements one window's walk reads at most: the browser's own
/// chrome is a few dozen, and a page's content is never entered.
const MAX_WINDOW_ELEMENTS: usize = 200;

/// The web address of `window`'s first Document. The walk follows the
/// browser's own chrome and stops at every Document, so a page's content,
/// the costly part of a UI Automation read, is never walked; a docked
/// DevTools document is a sibling and still answers if it is not the page.
fn document_url(automation: &IUIAutomation, window: isize) -> Option<String> {
    use std::collections::VecDeque;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::UIA_DocumentControlTypeId;

    // SAFETY: read-only UI Automation queries; `window` came from
    // EnumWindows and every element belongs to this thread's instance.
    unsafe {
        if window == 0 {
            return None;
        }
        let root = automation.ElementFromHandle(HWND(window as *mut _)).ok()?;
        let walker = automation.ControlViewWalker().ok()?;
        let mut pending = VecDeque::from([root]);
        let mut read = 0;
        while let Some(element) = pending.pop_front() {
            read += 1;
            if read > MAX_WINDOW_ELEMENTS {
                break;
            }
            if element
                .CurrentControlType()
                .is_ok_and(|kind| kind == UIA_DocumentControlTypeId)
            {
                if let Some(url) = document_value(&element) {
                    return Some(url);
                }
                continue;
            }
            let mut child = walker.GetFirstChildElement(&element).ok();
            while let Some(element) = child {
                pending.push_back(element.clone());
                child = walker.GetNextSiblingElement(&element).ok();
            }
        }
        None
    }
}

/// A Document element's page address, when its value is a web address.
fn document_value(element: &IUIAutomationElement) -> Option<String> {
    use windows::Win32::UI::Accessibility::{IUIAutomationValuePattern, UIA_ValuePatternId};

    // SAFETY: read-only queries on an element from this thread's instance.
    let pattern = unsafe {
        element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()?
    };
    let value = unsafe { pattern.CurrentValue().ok()? }.to_string();
    (value.starts_with("https://") || value.starts_with("http://")).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::modes::canonical_host;

    #[test]
    fn queued_context_keeps_the_stopped_application_and_page() {
        let (sender, receiver) = mpsc::channel();
        let pending = PendingContext {
            application: Some("chrome".into()),
            url: Some(receiver),
        };
        sender
            .send(Some("https://example.com/draft".into()))
            .unwrap();
        let context = pending.finish();
        assert_eq!(context.application.as_deref(), Some("chrome"));
        assert_eq!(context.browser_host(), Some("example.com"));
    }

    fn capture() -> ContextSnapshot {
        begin_capture(true).finish()
    }

    #[test]
    fn hosts_normalize_case_and_trailing_dots() {
        assert_eq!(normalize_host("X.com."), "x.com");
        assert_eq!(normalize_host("GitHub.COM"), "github.com");
    }

    #[test]
    fn website_entries_accept_pasted_urls_ports_and_idn() {
        assert_eq!(canonical_host("x.com").as_deref(), Some("x.com"));
        assert_eq!(
            canonical_host("https://x.com/home").as_deref(),
            Some("x.com")
        );
        assert_eq!(
            canonical_host("github.com/hex/pull/1").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            canonical_host("localhost:3000").as_deref(),
            Some("localhost")
        );
        assert_eq!(
            canonical_host("bücher.de").as_deref(),
            Some("xn--bcher-kva.de")
        );
        assert_eq!(canonical_host("   "), None);
    }

    #[test]
    fn urls_reduce_to_their_normalized_host() {
        assert_eq!(
            host_of("https://GitHub.com./hex/pull/1").as_deref(),
            Some("github.com")
        );
        assert_eq!(host_of("about:blank"), None);
        assert_eq!(host_of("not a url"), None);
    }

    #[test]
    fn only_known_browsers_are_probed() {
        assert!(BROWSER_STEMS.contains(&"msedge"));
        assert!(BROWSER_STEMS.contains(&"helium"));
        assert!(BROWSER_STEMS.contains(&"zen"));
        assert!(!BROWSER_STEMS.contains(&"code"));
        assert!(BROWSER_STEMS.is_sorted());
    }

    /// Manual probe: run
    /// `cargo test uia_reads_the_focused_browser -- --ignored --nocapture`
    /// and focus a browser window within fifteen seconds.
    #[test]
    #[ignore]
    fn uia_reads_the_focused_browser() {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let context = capture();
            if context.browser_host().is_some() {
                println!("foreground context: {context:?}");
                return;
            }
            if std::time::Instant::now() >= deadline {
                println!("last context: {context:?}");
                panic!("no browser page became foreground within fifteen seconds");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}
