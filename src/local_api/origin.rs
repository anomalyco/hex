//! A browser `Origin` as Labs › Connected apps reads it: the one canonical
//! `scheme://host[:port]` form a browser sends, and the CORS answer a
//! consent route gives an origin it serves.

/// An origin a consent route answers CORS for, and whether the browser asked
/// for private-network access (a page on the public internet reaching this
/// loopback API).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CorsOrigin {
    pub(super) origin: String,
    pub(super) private_network: bool,
}

/// Whether `origin` is exactly an `http(s)` origin as a browser sends it:
/// lowercase scheme and host, no default port, and no path, query, fragment,
/// or user information. `*`, `null`, and every other shape are refused.
pub(super) fn canonical(origin: &str) -> bool {
    normalize(origin).as_deref() == Some(origin)
}

/// `origin` as `scheme://host[:port]`, or `None` when it is not an absolute
/// HTTP(S) origin; one trailing `/` names the root page and is dropped.
fn normalize(origin: &str) -> Option<String> {
    let origin = origin.trim();
    let (scheme, authority) = origin.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return None;
    }
    let (host, port) = split_authority(authority)?;
    if !valid_host(host) {
        return None;
    }
    let default = if scheme == "http" { 80 } else { 443 };
    let port = match port {
        Some(port) => {
            let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
            (port != default).then_some(port)
        }
        None => None,
    };
    let host = host.to_ascii_lowercase();
    Some(match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

/// `authority` split into its host (IPv6 brackets kept) and its port.
fn split_authority(authority: &str) -> Option<(&str, Option<&str>)> {
    if authority.starts_with('[') {
        let closing = authority.find(']')?;
        let host = &authority[..=closing];
        return match &authority[closing + 1..] {
            "" => Some((host, None)),
            after => Some((host, Some(after.strip_prefix(':')?))),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host, Some(port))),
        None => Some((authority, None)),
    }
}

/// A hostname, an IPv4 address, or a bracketed IPv6 address.
fn valid_host(host: &str) -> bool {
    if let Some(inner) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        return !inner.is_empty()
            && inner.contains(':')
            && inner
                .chars()
                .all(|character| character.is_ascii_hexdigit() || matches!(character, ':' | '.'));
    }
    !host.is_empty()
        && host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
}

#[cfg(test)]
mod tests {
    use super::canonical;

    #[test]
    fn only_an_origin_as_a_browser_sends_it_is_canonical() {
        for origin in [
            "https://notes.example",
            "http://localhost:5173",
            "http://[::1]:5173",
        ] {
            assert!(canonical(origin), "{origin}");
        }
        for origin in [
            "https://Notes.example",
            "https://notes.example/",
            "https://notes.example:443",
            "https://notes.example/path",
            "https://user@notes.example",
            "file:///tmp",
            "null",
            "*",
            "",
        ] {
            assert!(!canonical(origin), "{origin}");
        }
    }
}
