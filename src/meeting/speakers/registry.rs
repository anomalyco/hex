//! Which adapter reads which call app.
//!
//! A call is known the way meeting detection knows it: a desktop app by its
//! canonical bundle id, a call on a web page by the page's host. Reading
//! another app is one line in [`ADAPTERS`]. There are none yet: each app's
//! adapter is written against a live read of that app's own windows.

use super::adapter::CallAppAdapter;

/// A call app, by the key meeting detection gives its calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallApp<'a> {
    /// A desktop app, by its canonical bundle id (`us.zoom.xos`).
    Bundle(&'a str),
    /// A call on a web page, in whichever browser, by the page's host as a
    /// site key: lowercase, without `www.` (`meet.google.com`).
    Site(&'a str),
}

/// Every call app Hex can read. An app known by several keys, as a desktop
/// app and as a page, is listed under each.
const ADAPTERS: &[(CallApp<'static>, &dyn CallAppAdapter)] = &[];

/// The adapter that reads `app`, when Hex can read it at all.
pub fn adapter_for(app: CallApp<'_>) -> Option<&'static dyn CallAppAdapter> {
    registered(ADAPTERS, app)
}

fn registered<'a>(
    adapters: &[(CallApp<'_>, &'a dyn CallAppAdapter)],
    app: CallApp<'_>,
) -> Option<&'a dyn CallAppAdapter> {
    let (_, adapter) = adapters.iter().find(|(known, _)| *known == app)?;
    Some(*adapter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::tree::Snapshot;
    use crate::meeting::speakers::adapter::Observation;
    use crate::meeting::speakers::evidence::Untrusted;

    fn desktop(_: &Snapshot) -> Observation {
        Observation::NoCallWindow
    }

    fn page(_: &Snapshot) -> Observation {
        Observation::Untrusted(Untrusted::WindowHidden)
    }

    /// Two apps, as they would be listed: one on the desktop, one on two
    /// hosts.
    const TWO: &[(CallApp<'static>, &dyn CallAppAdapter)] = &[
        (CallApp::Bundle("com.example.calls"), &desktop),
        (CallApp::Site("calls.example.com"), &page),
        (CallApp::Site("calls.example.org"), &page),
    ];

    fn read_by(app: CallApp<'_>) -> Option<Observation> {
        registered(TWO, app).map(|adapter| adapter.observe(&Snapshot::default()))
    }

    #[test]
    fn an_app_is_read_by_the_adapter_listed_under_its_key() {
        assert_eq!(
            read_by(CallApp::Bundle("com.example.calls")),
            Some(desktop(&Snapshot::default()))
        );
        for host in ["calls.example.com", "calls.example.org"] {
            assert_eq!(
                read_by(CallApp::Site(host)),
                Some(page(&Snapshot::default())),
                "{host}"
            );
        }
    }

    #[test]
    fn an_app_nobody_listed_is_not_read() {
        for unlisted in [
            CallApp::Bundle("com.example.browser"),
            CallApp::Site("example.com"),
            // A bundle id is not a host, nor a host a bundle id, even when
            // the text is the same.
            CallApp::Site("com.example.calls"),
            CallApp::Bundle("calls.example.com"),
            // Keys are detection's own, compared as they are.
            CallApp::Bundle("com.example.Calls"),
            CallApp::Site("www.calls.example.com"),
        ] {
            assert_eq!(read_by(unlisted), None, "{unlisted:?}");
        }
        assert!(adapter_for(CallApp::Bundle("invalid.hex.no-such-call-app")).is_none());
        assert!(adapter_for(CallApp::Site("no-such-call-app.invalid")).is_none());
    }

    #[test]
    fn no_app_is_listed_twice() {
        for (index, (app, _)) in ADAPTERS.iter().enumerate() {
            let earlier = ADAPTERS[..index].iter().any(|(known, _)| known == app);
            assert!(!earlier, "{app:?} is listed twice");
        }
    }
}
