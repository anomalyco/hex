//! Labs › Browser access: the switch and the origins it lists, beside the
//! state word in [`super`] (which is full), plus the one origin
//! normalization shared by settings, the window, and the local API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};

use crate::app_settings::LabsSettings;

/// Whether a web page from a listed origin may call the local API. The state
/// word is full (bit 63 is Keep mic open after dictating), so this lab has its
/// own atomic, written by [`super::state`] and [`super::apply`].
static ENABLED: AtomicBool = AtomicBool::new(false);

/// The origins it lists, normalized, set beside [`ENABLED`].
static ORIGINS: LazyLock<RwLock<Vec<String>>> = LazyLock::new(Default::default);

/// Records the saved switch and list; a lab the build or an organization
/// lock keeps off reads off.
pub(super) fn store(settings: &LabsSettings) {
    let feature = super::Feature::ApiBrowserAccess;
    ENABLED.store(
        settings.api_browser_access && feature.shipped() && feature.managed().is_none(),
        Ordering::Release,
    );
    let (origins, _) = normalize_all(settings.api_browser_origins.clone());
    *ORIGINS.write().unwrap_or_else(|error| error.into_inner()) = origins;
}

/// Whether browser access is on, loading the saved settings first in a
/// process that never applied them (`hex service`).
pub fn enabled() -> bool {
    let _ = super::state();
    ENABLED.load(Ordering::Acquire) && super::Feature::ApiBrowserAccess.on_this_os()
}

/// The origins a page may call from, normalized. Read per request, so a
/// change applies without a restart.
pub fn listed() -> Vec<String> {
    let _ = super::state();
    ORIGINS
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// One origin, normalized for listing and comparing: `http://` or `https://`,
/// a host, and an optional port, lowercase, with no path, query, fragment, or
/// userinfo. `*` and anything else are `None`.
pub fn normalize(raw: &str) -> Option<String> {
    let origin = raw.trim();
    if origin.is_empty() || origin.contains('*') {
        return None;
    }
    let (scheme, rest) = origin.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    // No path, query, fragment, userinfo, or whitespace.
    if rest.is_empty()
        || rest.contains(['@', '?', '#', '/'])
        || rest.chars().any(char::is_whitespace)
    {
        return None;
    }
    let (host, port) = split_host_port(rest)?;
    let mut normalized = format!("{scheme}://{}", host.to_ascii_lowercase());
    if let Some(port) = port {
        normalized.push(':');
        normalized.push_str(port);
    }
    Some(normalized)
}

/// `host[:port]`, keeping a bracketed IPv6 host whole. The port, when
/// present, is a number from 1 to 65535.
fn split_host_port(rest: &str) -> Option<(String, Option<&str>)> {
    let (host, port) = match rest.strip_prefix('[') {
        Some(after) => {
            let (host, tail) = after.split_once(']')?;
            if host.is_empty()
                || !host
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || matches!(c, ':' | '.'))
            {
                return None;
            }
            let port = if tail.is_empty() {
                None
            } else {
                Some(tail.strip_prefix(':')?)
            };
            (format!("[{host}]"), port)
        }
        None => {
            if rest.contains(['[', ']']) {
                return None;
            }
            match rest.split_once(':') {
                Some((host, port)) => (host.to_owned(), Some(port)),
                None => (rest.to_owned(), None),
            }
        }
    };
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
        return None;
    }
    if !host.starts_with('[')
        && !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return None;
    }
    if let Some(port) = port {
        if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if port.parse::<u16>().ok()? == 0 {
            return None;
        }
    }
    Some((host, port))
}

/// Every well-formed origin in `entries`, in order and without repeats, and
/// the entries it dropped.
pub fn normalize_all(entries: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for entry in entries {
        match normalize(&entry) {
            Some(origin) => {
                if !kept.contains(&origin) {
                    kept.push(origin);
                }
            }
            None => dropped.push(entry),
        }
    }
    (kept, dropped)
}

/// The origins in a typed list: entries separated by commas or new lines,
/// trimmed, with anything malformed left out.
pub fn parse_list(text: &str) -> Vec<String> {
    normalize_all(
        text.split([',', '\n'])
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect(),
    )
    .0
}

/// How a list reads in a field: one origin each, separated by commas.
pub fn display(origins: &[String]) -> String {
    origins.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_keeps_its_scheme_host_and_port_lowercased() {
        for (raw, normalized) in [
            ("http://localhost:3000", "http://localhost:3000"),
            ("  HTTP://LocalHost:3000  ", "http://localhost:3000"),
            ("https://App.Example.com", "https://app.example.com"),
            ("http://127.0.0.1:8787", "http://127.0.0.1:8787"),
            ("http://[::1]:3000", "http://[::1]:3000"),
            ("https://[::1]", "https://[::1]"),
            ("http://my_host:80", "http://my_host:80"),
        ] {
            assert_eq!(normalize(raw).as_deref(), Some(normalized), "{raw:?}");
        }
    }

    #[test]
    fn anything_but_scheme_host_and_port_is_not_an_origin() {
        for raw in [
            "",
            "   ",
            "*",
            "http://*",
            "https://*.example.com",
            "example.com",
            "//example.com",
            "ftp://example.com",
            "http://",
            "http://user@example.com",
            "http://user:pass@example.com",
            "http://example.com/",
            "http://example.com/path",
            "http://example.com?query",
            "http://example.com#fragment",
            "http://example.com:",
            "http://example.com:0",
            "http://example.com:70000",
            "http://example.com:80x",
            "http://exa mple.com",
            "http://",
            "http://[::1",
            "http://[zz]:80",
            "null",
        ] {
            assert_eq!(normalize(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_typed_list_takes_commas_and_new_lines_and_drops_the_rest() {
        let parsed = parse_list(
            "http://localhost:3000, https://app.example.com\n\
             nope\n\
             http://LOCALHOST:3000",
        );
        assert_eq!(parsed, ["http://localhost:3000", "https://app.example.com"]);
        assert_eq!(
            display(&parsed),
            "http://localhost:3000, https://app.example.com"
        );
        assert!(parse_list(" , \n ").is_empty());
    }

    #[test]
    fn normalize_all_reports_what_it_dropped() {
        let (kept, dropped) = normalize_all(vec![
            "https://Example.com".into(),
            "not an origin".into(),
            "https://example.com".into(),
            "http://example.com:1234".into(),
        ]);
        assert_eq!(kept, ["https://example.com", "http://example.com:1234"]);
        assert_eq!(dropped, ["not an origin"]);
    }
}
