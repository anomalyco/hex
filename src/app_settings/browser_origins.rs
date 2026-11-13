//! Browser origins for the local API (Labs › Browser access to the local
//! API): one canonical `scheme://host[:port]` form, so settings can allow
//! only an origin a browser can actually send in an `Origin` header.

/// `origin` as `scheme://host[:port]`, or `None` when it is not an absolute
/// HTTP(S) origin.
///
/// Only `http` and `https` are accepted, and user information, a path, a
/// query, and a fragment are refused; one trailing `/` names the root page,
/// which is the origin itself, and is dropped. The scheme and host are
/// lowercased, and `http`'s `:80` or `https`'s `:443` is dropped, matching
/// what a browser sends. `*`, `null`, an empty entry, and every other shape
/// are refused rather than widened.
pub fn normalize_browser_origin(origin: &str) -> Option<String> {
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
    let port = match port {
        Some(port) => {
            let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
            (port != default_port(&scheme)).then_some(port)
        }
        None => None,
    };
    let host = host.to_ascii_lowercase();
    Some(match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

/// The origins in `text`, comma-separated as the Labs card takes them, in
/// the order typed; an entry that isn't an origin is left out, and a repeat
/// keeps its first place.
pub fn parse_browser_origins(text: &str) -> Vec<String> {
    let mut origins: Vec<String> = Vec::new();
    for entry in text.split(',') {
        if let Some(origin) = normalize_browser_origin(entry)
            && !origins.contains(&origin)
        {
            origins.push(origin);
        }
    }
    origins
}

/// `origins` normalized as [`normalize_browser_origin`] does, leaving out
/// every entry that isn't an origin and every repeat. Saved settings read
/// through this, so a hand-edited entry can never widen what is allowed.
pub fn normalize_browser_origins(origins: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for origin in origins {
        if let Some(origin) = normalize_browser_origin(origin)
            && !normalized.contains(&origin)
        {
            normalized.push(origin);
        }
    }
    normalized
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

fn default_port(scheme: &str) -> u16 {
    if scheme == "http" { 80 } else { 443 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_its_scheme_host_and_port() {
        assert_eq!(
            normalize_browser_origin("https://example.com").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_browser_origin("http://localhost:5173").as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            normalize_browser_origin("http://127.0.0.1:8000").as_deref(),
            Some("http://127.0.0.1:8000")
        );
        assert_eq!(
            normalize_browser_origin("http://[::1]:5173").as_deref(),
            Some("http://[::1]:5173")
        );
    }

    #[test]
    fn the_scheme_and_host_are_lowercased_and_a_root_slash_is_dropped() {
        for (typed, expected) in [
            ("HTTPS://Example.COM", "https://example.com"),
            ("HtTp://LOCALHOST:3000/", "http://localhost:3000"),
            ("  https://example.com/  ", "https://example.com"),
        ] {
            assert_eq!(
                normalize_browser_origin(typed).as_deref(),
                Some(expected),
                "{typed}"
            );
        }
    }

    #[test]
    fn a_default_port_is_dropped_as_a_browser_does() {
        assert_eq!(
            normalize_browser_origin("https://example.com:443").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_browser_origin("http://example.com:80").as_deref(),
            Some("http://example.com")
        );
        assert_eq!(
            normalize_browser_origin("https://example.com:8443").as_deref(),
            Some("https://example.com:8443")
        );
    }

    #[test]
    fn a_path_query_fragment_user_or_wildcard_is_refused() {
        for typed in [
            "*",
            "null",
            "",
            "   ",
            "example.com",
            "//example.com",
            "ftp://example.com",
            "file:///tmp/index.html",
            "https://",
            "https:///path",
            "https://example.com/index.html",
            "https://example.com//",
            "https://example.com?q=1",
            "https://example.com#top",
            "https://user:pass@example.com",
            "https://user@example.com",
            "https://example.com:",
            "https://example.com:0",
            "https://example.com:65536",
            "https://example.com:https",
            "https://exa mple.com",
            "https://*.example.com",
            "https://[::1",
            "https://[]",
        ] {
            assert_eq!(normalize_browser_origin(typed), None, "{typed:?}");
        }
    }

    #[test]
    fn a_comma_separated_card_field_keeps_only_origins() {
        assert_eq!(
            parse_browser_origins(
                " https://Example.com/ , *, http://localhost:5173,,\nhttps://example.com "
            ),
            vec!["https://example.com", "http://localhost:5173"]
        );
    }

    #[test]
    fn saved_origins_are_normalized_and_deduplicated() {
        let saved = vec![
            "HTTPS://Example.com:443/".to_owned(),
            "*".to_owned(),
            "http://localhost:3000".to_owned(),
            "https://example.com".to_owned(),
        ];
        assert_eq!(
            normalize_browser_origins(&saved),
            vec!["https://example.com", "http://localhost:3000"]
        );
    }
}
