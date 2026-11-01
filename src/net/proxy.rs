//! The proxy for a URL, as the computer would choose it: the standard
//! environment variables first on every OS, then the system's settings
//! (Windows: WinINet's per-user settings, a PAC file or WPAD, then WinHTTP's
//! machine proxy; macOS: the network's proxy settings). Linux has only the
//! environment variables. Answers are kept for a minute per scheme and host,
//! since WPAD can take seconds.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

const KEEP: Duration = Duration::from_secs(60);

static ANSWERS: LazyLock<Mutex<HashMap<String, (Instant, Option<String>)>>> =
    LazyLock::new(Mutex::default);

/// The proxy for `url`, or `None` to connect directly.
pub fn proxy_for(url: &str) -> Option<ureq::Proxy> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_owned();
    let key = format!("{}://{host}", parsed.scheme());
    let cached = ANSWERS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .filter(|(at, _)| at.elapsed() < KEEP)
        .map(|(_, proxy)| proxy.clone());
    let proxy = match cached {
        Some(proxy) => proxy,
        None => {
            let proxy = resolve(&parsed, &host);
            if let Some(proxy) = &proxy {
                tracing::info!(host, proxy = %redact(proxy), "using a proxy");
            }
            ANSWERS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(key, (Instant::now(), proxy.clone()));
            proxy
        }
    };
    let proxy = proxy?;
    match ureq::Proxy::new(&proxy) {
        Ok(proxy) => Some(proxy),
        Err(error) => {
            tracing::warn!(%error, proxy = %redact(&proxy), "ignoring a proxy Hex can't use");
            None
        }
    }
}

fn resolve(url: &url::Url, host: &str) -> Option<String> {
    let environment = |name: &str| {
        std::env::var(name)
            .ok()
            .or_else(|| std::env::var(name.to_lowercase()).ok())
            .filter(|value| !value.trim().is_empty())
    };
    let from_environment = if url.scheme() == "https" {
        environment("HTTPS_PROXY")
    } else {
        environment("HTTP_PROXY")
    }
    .or_else(|| environment("ALL_PROXY"));
    if let Some(proxy) = from_environment {
        let bypassed = environment("NO_PROXY").is_some_and(|list| no_proxy_matches(&list, host));
        return (!bypassed).then(|| with_scheme(&proxy));
    }
    system(url.as_str(), host).map(|proxy| with_scheme(&proxy))
}

#[cfg(windows)]
fn system(url: &str, host: &str) -> Option<String> {
    windows::proxy(url, host)
}

#[cfg(target_os = "macos")]
fn system(url: &str, _host: &str) -> Option<String> {
    macos::proxy(url)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system(_url: &str, _host: &str) -> Option<String> {
    None
}

/// `proxy` as a URL: a bare `host:port`, as system settings write it, is an
/// HTTP proxy.
fn with_scheme(proxy: &str) -> String {
    let proxy = proxy.trim();
    if proxy.contains("://") {
        proxy.to_owned()
    } else {
        format!("http://{proxy}")
    }
}

/// `proxy` without a user name or password, for logs.
fn redact(proxy: &str) -> String {
    match proxy.split_once('@') {
        Some((scheme_and_credentials, rest)) => {
            let scheme = scheme_and_credentials
                .split_once("://")
                .map_or("", |(scheme, _)| scheme);
            if scheme.is_empty() {
                rest.to_owned()
            } else {
                format!("{scheme}://{rest}")
            }
        }
        None => proxy.to_owned(),
    }
}

/// Whether `NO_PROXY` (comma or space separated) exempts `host`: `*`, an
/// exact host, or a domain suffix (`example.com` and `.example.com` both
/// cover `a.example.com`). A port after a name is ignored.
pub(crate) fn no_proxy_matches(list: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    list.split([',', ' '])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let entry = entry
                .rsplit_once(':')
                .filter(|(name, port)| !name.contains(':') && port.parse::<u16>().is_ok())
                .map_or(entry, |(name, _)| name);
            let entry = entry.trim_start_matches("*.").trim_start_matches('.');
            let entry = entry.to_ascii_lowercase();
            host == entry || host.ends_with(&format!(".{entry}"))
        })
}

/// Whether a Windows-style bypass list (`;`-separated, `*` wildcards,
/// `<local>` for names without a dot) exempts `host`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn bypass_matches(list: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    list.split([';', ',', ' '])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry.eq_ignore_ascii_case("<local>") {
                return !host.contains('.');
            }
            wildcard(&entry.to_ascii_lowercase(), &host)
        })
}

fn wildcard(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            text.starts_with(head)
                && (0..=text.len() - head.len())
                    .any(|skip| wildcard(rest, &text[head.len() + skip..]))
        }
    }
}

/// From a Windows proxy list (`host:port`, or `http=a:80;https=b:443`, `;` or
/// space separated), the proxy for `scheme`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn pick_from_list(list: &str, scheme: &str) -> Option<String> {
    let entries: Vec<&str> = list
        .split([';', ' '])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();
    let named = |name: &str| {
        entries.iter().find_map(|entry| {
            entry
                .split_once('=')
                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.to_owned())
        })
    };
    named(scheme)
        .or_else(|| {
            entries
                .iter()
                .find(|entry| !entry.contains('='))
                .map(|entry| (*entry).to_owned())
        })
        .or_else(|| named("http"))
        .filter(|entry| !entry.eq_ignore_ascii_case("DIRECT"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_proxy_covers_hosts_and_domains() {
        let list = "localhost, .corp.example,internal.net:8080 10.0.0.1";
        assert!(no_proxy_matches(list, "localhost"));
        assert!(no_proxy_matches(list, "a.corp.example"));
        assert!(no_proxy_matches(list, "corp.example"));
        assert!(no_proxy_matches(list, "x.internal.net"));
        assert!(no_proxy_matches(list, "10.0.0.1"));
        assert!(!no_proxy_matches(list, "huggingface.co"));
        assert!(no_proxy_matches("*", "huggingface.co"));
    }

    #[test]
    fn windows_bypass_lists_use_wildcards_and_local() {
        assert!(bypass_matches("<local>;*.corp.example", "intranet"));
        assert!(bypass_matches(
            "<local>;*.corp.example",
            "wiki.corp.example"
        ));
        assert!(!bypass_matches("<local>;*.corp.example", "huggingface.co"));
        assert!(bypass_matches("10.*", "10.1.2.3"));
    }

    #[test]
    fn windows_proxy_lists_pick_the_scheme() {
        assert_eq!(
            pick_from_list("proxy.corp:8080", "https").as_deref(),
            Some("proxy.corp:8080")
        );
        assert_eq!(
            pick_from_list("http=a:80;https=b:443", "https").as_deref(),
            Some("b:443")
        );
        assert_eq!(
            pick_from_list("http=a:80", "https").as_deref(),
            Some("a:80")
        );
        assert_eq!(
            pick_from_list("p1.corp:3128;p2.corp:3128", "https").as_deref(),
            Some("p1.corp:3128")
        );
        assert_eq!(pick_from_list("DIRECT", "https"), None);
    }

    #[test]
    fn proxies_get_a_scheme_and_logs_drop_credentials() {
        assert_eq!(with_scheme("proxy:8080"), "http://proxy:8080");
        assert_eq!(with_scheme("socks5://p:1080"), "socks5://p:1080");
        assert_eq!(redact("http://me:secret@proxy:8080"), "http://proxy:8080");
    }
}
