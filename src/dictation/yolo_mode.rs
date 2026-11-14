//! Labs › YOLO mode: a shortcut dictation held for at
//! least the lab's chosen time (`labs.yolo_mode_seconds`, 2 s by
//! default) presses Return after its paste, so a long message sends itself.
//! With apps chosen (`labs.yolo_mode_apps`), only a dictation whose
//! frontmost app was one of them sends. Shorter ones paste as usual.

use std::time::Duration;

/// How long the paste gets to land before Return goes after it: apps that
/// read the clipboard asynchronously (Electron, browsers) could otherwise
/// see Return first and send an empty message.
pub const SETTLE: Duration = Duration::from_millis(100);

/// Whether a shortcut dictation held `held_ms`, captured with `application`
/// frontmost, presses Return after its paste: the lab is on, the hold
/// reached its chosen time, and the app is one it sends in. Callers pass
/// only hotkey dictations that paste processed text; Dictate without
/// processing, printing stops, and `hex dictation` or local API captures
/// never send.
pub fn sends(held_ms: u64, application: Option<&str>) -> bool {
    crate::labs::yolo_mode().is_some_and(|minimum| {
        long_enough(held_ms, minimum) && in_app(&crate::labs::yolo_mode_apps(), application)
    })
}

fn long_enough(held_ms: u64, minimum: Duration) -> bool {
    u128::from(held_ms) >= minimum.as_millis()
}

/// No apps chosen sends anywhere; otherwise the frontmost app must match
/// one as a Mode's application does, and an unknown app never sends.
fn in_app(apps: &[String], application: Option<&str>) -> bool {
    if apps.is_empty() {
        return true;
    }
    let context = crate::dictation::context::ContextSnapshot {
        application: application.map(str::to_owned),
        ..Default::default()
    };
    apps.iter().any(|app| context.application_is(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_reaching_the_chosen_time_is_long_enough() {
        let two = Duration::from_secs(2);
        assert!(!long_enough(0, two));
        assert!(!long_enough(1_999, two));
        assert!(long_enough(2_000, two));
        assert!(long_enough(60_000, two));
        assert!(!long_enough(4_999, Duration::from_secs(5)));
        assert!(long_enough(1_000, Duration::from_secs(1)));
    }

    #[test]
    fn chosen_apps_limit_where_it_sends() {
        assert!(in_app(&[], None));
        assert!(in_app(&[], Some("Notes")));
        let apps = ["Slack".to_owned(), "Messages.app".to_owned()];
        assert!(in_app(&apps, Some("Slack")));
        assert!(in_app(&apps, Some("messages")));
        assert!(!in_app(&apps, Some("Notes")));
        assert!(!in_app(&apps, None), "an unknown app never sends");
    }
}
