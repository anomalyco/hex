//! The focused window's process and web page through AT-SPI, the desktop
//! accessibility bus, on X11 and Wayland. The compositor or X server names
//! the focused window; this finds the accessible window with the same title
//! (and process, when known), then reads the address of its showing web
//! document. Firefox exposes pages whenever the accessibility bus is up;
//! Chromium browsers only with `--force-renderer-accessibility` or a
//! running screen reader. Every call is bounded, so a hung application
//! costs at most the deadline.

use std::collections::HashMap;
use std::time::Duration;

use url::Url;
use zbus::{Connection, OwnedObjectPath, OwnedVariant};

const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
const NULL: &str = "/org/a11y/atspi/null";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const STATE_ACTIVE: u32 = 1;
const STATE_SHOWING: u32 = 25;
const ROLE_DOCUMENT_WEB: u32 = 95;
const MATCH_ALL: i32 = 1;
const MATCH_ANY: i32 = 2;
const SORT_CANONICAL: u32 = 1;
const CALL_TIMEOUT: Duration = Duration::from_millis(150);
const DEADLINE: Duration = Duration::from_millis(400);

type Reference = (String, OwnedObjectPath);

/// An AT-SPI collection match rule: states, attributes, roles, and
/// interfaces, each with its match type, then whether to invert.
type MatchRule = (
    Vec<i32>,
    i32,
    HashMap<String, String>,
    i32,
    Vec<i32>,
    i32,
    Vec<String>,
    i32,
    bool,
);

/// What the display server reports about the focused window.
#[derive(Debug, Default)]
pub(crate) struct WindowHint {
    pub(crate) pid: Option<u32>,
    pub(crate) title: Option<String>,
}

/// The focused window as AT-SPI sees it.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct AccessibleWindow {
    pub(crate) pid: Option<u32>,
    pub(crate) url: Option<Url>,
}

/// The accessible window matching `hint`, or `None` without an
/// accessibility bus, a hint, or a match.
pub(crate) fn focused_window(hint: &WindowHint) -> Option<AccessibleWindow> {
    if hint.pid.is_none() && hint.title.is_none() {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        tokio::time::timeout(DEADLINE, read_window(hint))
            .await
            .ok()
            .flatten()
    })
}

async fn read_window(hint: &WindowHint) -> Option<AccessibleWindow> {
    let session = Connection::session().await.ok()?;
    let address: String = call(
        &session,
        "org.a11y.Bus",
        "/org/a11y/bus",
        "org.a11y.Bus",
        "GetAddress",
        &(),
    )
    .await?;
    let bus = zbus::connection::Builder::address(address.as_str())
        .build()
        .await
        .ok()?;
    let applications: Vec<Reference> =
        call(&bus, REGISTRY, ROOT, ACCESSIBLE, "GetChildren", &()).await?;
    let candidates = futures_util::future::join_all(
        applications
            .iter()
            .map(|(name, root)| matching_window(&bus, name, root, hint)),
    )
    .await;
    // An application can keep reporting a window active after focus moved
    // to a window it doesn't own, so activity only breaks ties.
    let window = candidates
        .iter()
        .flatten()
        .find(|window| window.active)
        .or_else(|| candidates.iter().flatten().next())?;
    let url = page_url(&bus, &window.reference).await;
    Some(AccessibleWindow {
        pid: window.pid,
        url,
    })
}

struct Candidate {
    reference: Reference,
    pid: Option<u32>,
    active: bool,
}

/// The application's top-level window matching `hint`, if any.
async fn matching_window(
    bus: &Connection,
    name: &str,
    root: &OwnedObjectPath,
    hint: &WindowHint,
) -> Option<Candidate> {
    let pid = process_id(bus, name).await;
    if hint.pid.is_some() && pid != hint.pid {
        return None;
    }
    let windows: Vec<Reference> =
        call(bus, name, root.as_str(), ACCESSIBLE, "GetChildren", &()).await?;
    let mut matched = None;
    for (window_name, window) in windows {
        if window.as_str() == NULL {
            continue;
        }
        if let Some(title) = &hint.title
            && accessible_name(bus, &window_name, &window).await.as_deref() != Some(title.as_str())
        {
            continue;
        }
        let states: Vec<u32> = call(
            bus,
            &window_name,
            window.as_str(),
            ACCESSIBLE,
            "GetState",
            &(),
        )
        .await
        .unwrap_or_default();
        let candidate = Candidate {
            reference: (window_name, window),
            pid,
            active: has_state(&states, STATE_ACTIVE),
        };
        if candidate.active {
            return Some(candidate);
        }
        matched.get_or_insert(candidate);
    }
    matched
}

async fn process_id(bus: &Connection, name: &str) -> Option<u32> {
    call(
        bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixProcessID",
        &(name,),
    )
    .await
}

async fn accessible_name(bus: &Connection, name: &str, path: &OwnedObjectPath) -> Option<String> {
    let value: OwnedVariant = call(
        bus,
        name,
        path.as_str(),
        "org.freedesktop.DBus.Properties",
        "Get",
        &(ACCESSIBLE, "Name"),
    )
    .await?;
    <&str>::try_from(&value).ok().map(str::to_owned)
}

/// The first showing web document's http(s) address.
async fn page_url(bus: &Connection, (name, window): &Reference) -> Option<Url> {
    let documents: Vec<Reference> = call(
        bus,
        name,
        window.as_str(),
        "org.a11y.atspi.Collection",
        "GetMatches",
        &(document_rule(), SORT_CANONICAL, 8_i32, true),
    )
    .await?;
    for (document_name, document) in documents {
        let attributes: HashMap<String, String> = call(
            bus,
            &document_name,
            document.as_str(),
            "org.a11y.atspi.Document",
            "GetAttributes",
            &(),
        )
        .await
        .unwrap_or_default();
        if let Some(url) = web_url(&attributes) {
            return Some(url);
        }
    }
    None
}

fn document_rule() -> MatchRule {
    (
        bitset::<2>(STATE_SHOWING),
        MATCH_ALL,
        HashMap::new(),
        MATCH_ALL,
        bitset::<4>(ROLE_DOCUMENT_WEB),
        MATCH_ANY,
        Vec::new(),
        MATCH_ALL,
        false,
    )
}

/// AT-SPI's bit-array encoding of one member across `WORDS` 32-bit words.
fn bitset<const WORDS: usize>(member: u32) -> Vec<i32> {
    let mut words = vec![0_i32; WORDS];
    words[(member / 32) as usize] = (1_u32 << (member % 32)).cast_signed();
    words
}

fn has_state(states: &[u32], state: u32) -> bool {
    states
        .get((state / 32) as usize)
        .is_some_and(|word| word & (1 << (state % 32)) != 0)
}

/// The page address from a document's `DocURL` (Firefox) or `URI`
/// (Chromium) attribute; browser-internal pages have none.
fn web_url(attributes: &HashMap<String, String>) -> Option<Url> {
    ["DocURL", "URI"]
        .iter()
        .filter_map(|key| attributes.get(*key))
        .filter_map(|value| Url::parse(value.trim()).ok())
        .find(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

async fn call<B, R>(
    bus: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> Option<R>
where
    B: serde::Serialize + zbus::Type,
    R: for<'d> serde::Deserialize<'d> + zbus::Type,
{
    let call = bus.call_method(Some(destination), path, Some(interface), method, body);
    let reply = tokio::time::timeout(CALL_TIMEOUT, call).await.ok()?.ok()?;
    reply.body().deserialize().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_encode_as_atspi_bit_arrays() {
        assert_eq!(bitset::<2>(STATE_SHOWING), vec![1 << 25, 0]);
        assert_eq!(bitset::<4>(ROLE_DOCUMENT_WEB), vec![0, 0, i32::MIN, 0]);
        assert!(has_state(&[1 << STATE_ACTIVE, 0], STATE_ACTIVE));
        assert!(!has_state(&[1 << STATE_SHOWING], STATE_ACTIVE));
        assert!(!has_state(&[], STATE_ACTIVE));
    }

    #[test]
    fn only_web_addresses_count() {
        let attributes = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect()
        };
        assert_eq!(
            web_url(&attributes(&[("URI", "https://example.com/a")])).map(String::from),
            Some("https://example.com/a".into())
        );
        assert_eq!(
            web_url(&attributes(&[
                ("DocURL", "https://x.com/home"),
                ("URI", "https://example.com/")
            ]))
            .map(String::from),
            Some("https://x.com/home".into())
        );
        assert_eq!(
            web_url(&attributes(&[(
                "URI",
                "chrome://omnibox-popup.top-chrome/"
            )])),
            None
        );
        assert_eq!(web_url(&attributes(&[("DocURL", "about:blank")])), None);
    }
}
