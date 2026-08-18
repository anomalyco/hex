//! Foreground context for dictation on Windows: the focused application's
//! executable stem, and for known browsers the page URL read through UI
//! Automation, so web-domain mode rules and Voice Action prompts see the
//! real host. Chromium-family browsers and Firefox expose the page URL on
//! the UIA Document element's value pattern, which is locale-independent.
//! The UIA read runs on a bounded helper thread; a slow or hung
//! accessibility provider degrades to application-only context.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use url::Url;

/// Executable stems whose foreground document exposes the page URL.
const BROWSER_STEMS: &[&str] = &[
    "arc",
    "brave",
    "chrome",
    "chromium",
    "firefox",
    "librewolf",
    "msedge",
    "opera",
    "opera_gx",
    "thorium",
    "vivaldi",
    "zen",
];

const URL_READ_DEADLINE: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, Default)]
pub struct WindowsContext {
    pub application: Option<String>,
    pub browser_host: Option<String>,
}

/// A context read in flight: the application stem is immediate, the UIA
/// URL read proceeds on a helper thread while the caller transcribes.
pub struct PendingContext {
    application: Option<String>,
    url: Option<mpsc::Receiver<Option<String>>>,
}

/// Begin reading the foreground context at the moment the dictation is
/// submitted; `finish` collects the browser host after inference, hiding
/// the UIA walk behind it.
pub fn begin_capture() -> PendingContext {
    let application = crate::windows_input::foreground_process_stem();
    let url = application
        .as_deref()
        .filter(|stem| BROWSER_STEMS.contains(stem))
        .and_then(|_| spawn_url_read());
    PendingContext { application, url }
}

impl PendingContext {
    pub fn finish(self) -> WindowsContext {
        let browser_host = self
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
            .and_then(host_of);
        WindowsContext {
            application: self.application,
            browser_host,
        }
    }
}

fn host_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(normalize_host))
}

/// Hosts compare case-insensitively with any trailing dot removed,
/// matching the macOS browser-host semantics.
pub fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_lowercase()
}

/// A website rule entry reduced to its comparable host: bare hosts,
/// pasted URLs, host:port forms, and IDN text all work, matching the
/// macOS Websites field.
pub fn canonical_host(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    let parsed = Url::parse(entry)
        .ok()
        .filter(|url| url.host_str().is_some())
        .or_else(|| Url::parse(&format!("https://{entry}")).ok())?;
    parsed.host_str().map(normalize_host)
}

/// At most one UIA read runs at a time: a hung accessibility provider
/// blocks its helper thread until it answers, and spawning more would
/// pile blocked threads one per dictation.
static URL_READ_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

fn spawn_url_read() -> Option<mpsc::Receiver<Option<String>>> {
    if URL_READ_IN_FLIGHT.swap(true, Ordering::AcqRel) {
        tracing::debug!("skipping the browser URL read; a previous read is still blocked");
        return None;
    }
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("windows-uia-context".into())
        .spawn(move || {
            let url = read_foreground_document_url();
            URL_READ_IN_FLIGHT.store(false, Ordering::Release);
            if url.is_none() {
                tracing::debug!("the foreground browser exposed no page URL");
            }
            let _ = sender.send(url);
        });
    match spawned {
        Ok(_) => Some(receiver),
        Err(_) => {
            URL_READ_IN_FLIGHT.store(false, Ordering::Release);
            None
        }
    }
}

fn read_foreground_document_url() -> Option<String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants,
        UIA_ControlTypePropertyId, UIA_DocumentControlTypeId, UIA_ValuePatternId,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let url = (|| -> windows::core::Result<Option<String>> {
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = automation.ElementFromHandle(HWND(window.cast()))?;
            // An i32 VARIANT holding the Document control type; VT_I4 owns
            // nothing, so the ManuallyDrop payload never needs dropping.
            let document_type = VARIANT {
                Anonymous: VARIANT_0 {
                    Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                        vt: VT_I4,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: VARIANT_0_0_0 {
                            lVal: UIA_DocumentControlTypeId.0,
                        },
                    }),
                },
            };
            let condition =
                automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &document_type)?;
            // A browser window can hold several documents (docked
            // DevTools, sidebars); the page is the first with a web URL.
            let documents = root.FindAll(TreeScope_Descendants, &condition)?;
            for index in 0..documents.Length()? {
                let Ok(document) = documents.GetElement(index) else {
                    continue;
                };
                let Ok(pattern) =
                    document.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                else {
                    continue;
                };
                let Ok(value) = pattern.CurrentValue() else {
                    continue;
                };
                let value = value.to_string();
                if value.starts_with("https://") || value.starts_with("http://") {
                    return Ok(Some(value));
                }
            }
            Ok(None)
        })();
        if com.is_ok() {
            CoUninitialize();
        }
        url.ok().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture() -> WindowsContext {
        begin_capture().finish()
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
            if context.browser_host.is_some() {
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
