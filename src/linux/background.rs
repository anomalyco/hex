//! The desktop's Background portal (`org.freedesktop.portal.Background`,
//! #812): Hex tells the desktop it runs in the background, and where the
//! portal exists it owns Launch at login. Asked to, the portal writes or
//! removes `~/.config/autostart/anomaly.hex.desktop` itself, the entry
//! [`crate::linux::login_item`] writes on desktops without it. Every request
//! also sets autostart, and turning it off removes that file, so Hex asks
//! only while the file is absent or its own.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use color_eyre::eyre::Result;
use zbus::Variant;

use crate::linux::portal::PortalBus;

const INTERFACE: &str = "org.freedesktop.portal.Background";

/// Asks the portal to let Hex run in the background, starting `launcher`
/// at login when `autostart`. Returns whether the portal now starts Hex at
/// login; `None` when there is no portal, or `launcher`'s path would not
/// survive the portal's unquoted `Exec=` line.
pub(crate) fn request(autostart: bool, launcher: &Path) -> Result<Option<bool>> {
    let Some(launcher) = launcher.to_str().filter(|path| plain(path)) else {
        return Ok(None);
    };
    let stop = AtomicBool::new(false);
    let Some(mut bus) = PortalBus::open(INTERFACE, &stop)? else {
        return Ok(None);
    };
    let commandline = vec![launcher.to_owned(), "start".to_owned()];
    let results = bus.request(
        "RequestBackground",
        |token| {
            (
                "",
                HashMap::from([
                    ("handle_token", Variant::from(token.to_owned())),
                    (
                        "reason",
                        Variant::from(
                            "Hex keeps listening for your dictation shortcut when its window is closed",
                        ),
                    ),
                    ("autostart", Variant::from(autostart)),
                    ("commandline", Variant::from(commandline.clone())),
                ]),
            )
        },
        &stop,
    )?;
    let flag = |key: &str| {
        results
            .get(key)
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(false)
    };
    tracing::info!(
        background = flag("background"),
        autostart = flag("autostart"),
        "the Background portal answered"
    );
    Ok(Some(flag("autostart")))
}

/// Whether `path` reads the same in an `Exec=` line without quoting.
fn plain(path: &str) -> bool {
    !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_paths_go_to_the_portal() {
        assert!(plain("/home/a/.local/share/hex/current/hex"));
        assert!(plain("/usr/bin/hex"));
        assert!(!plain("/home/a b/hex"));
        assert!(!plain("/tmp/100%/hex"));
    }
}
