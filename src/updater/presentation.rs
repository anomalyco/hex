//! Single presentation model for [`DesktopUpdateStatus`] across the sidebar,
//! the Linux and Windows tray menus, and the macOS menu bar.

use crate::app_settings::UpdateChannel;
use crate::desktop::host::{DesktopAction, DesktopUpdateStatus};
#[cfg(any(target_os = "macos", test))]
use crate::desktop::i18n::menu_label;
use crate::desktop::i18n::{tr, tr_named};

/// Why Restart to update waits while a meeting is active: relaunching would
/// cut the recording or launch recovery short.
pub(crate) const MEETING_RESTART_HOLD: &str =
    "Hex is finishing a meeting; restart to update when it's done";

/// An actionable update offer derived once from [`DesktopUpdateStatus`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UpdateOffer {
    /// Sentence-case translated CTA for sidebar buttons and non-macOS trays
    /// (e.g. `"Restart to update"`, `"Download update"`,
    /// `"Switch to Stable 2.3.1"`).
    pub(crate) label: String,
    /// macOS menu-bar title (English Title Case and trailing `…` where the
    /// item opens a download or confirms a switch; translated otherwise).
    #[cfg(any(target_os = "macos", test))]
    pub(crate) menu_title: String,
    /// The desktop action carrying out this offer.
    pub(crate) action: DesktopAction,
    /// Whether this offer must wait while a meeting is recording.
    pub(crate) held_by_meeting: bool,
}

impl UpdateOffer {
    /// Whether a recording meeting currently blocks this offer.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) fn is_held(&self, meeting_recording: bool) -> bool {
        meeting_recording && self.held_by_meeting
    }
}

/// The control that confirms a channel switch, e.g. `"Switch to Stable 2.3.1"`.
fn switch_label(channel: &str, version: &str) -> String {
    let channel = match channel {
        "dev" => UpdateChannel::Dev.label(),
        "stable" => UpdateChannel::Stable.label(),
        other => other,
    };
    tr_named(
        "Switch to {channel} {version}",
        &[("channel", tr(channel)), ("version", version)],
    )
}

/// Whether `status`'s offer must wait while a meeting is recording.
/// Exhaustive over [`DesktopUpdateStatus`] so new variants cannot silently
/// drift across surfaces.
const fn held_by_meeting(status: &DesktopUpdateStatus) -> bool {
    match status {
        DesktopUpdateStatus::ReadyToRestart => true,
        DesktopUpdateStatus::Available
        | DesktopUpdateStatus::SwitchAvailable { .. }
        | DesktopUpdateStatus::Unavailable
        | DesktopUpdateStatus::Managed
        | DesktopUpdateStatus::Checking
        | DesktopUpdateStatus::Current
        | DesktopUpdateStatus::Failed
        | DesktopUpdateStatus::Downloading
        | DesktopUpdateStatus::SourceBuild
        | DesktopUpdateStatus::PackageManager
        | DesktopUpdateStatus::PackageUpdate { .. } => false,
    }
}

/// The actionable update offer for `status`, if one is waiting for the user.
/// Exhaustive over [`DesktopUpdateStatus`] so new variants cannot silently
/// drift across surfaces.
pub(crate) fn offer(status: &DesktopUpdateStatus) -> Option<UpdateOffer> {
    let held_by_meeting = held_by_meeting(status);
    match status {
        DesktopUpdateStatus::ReadyToRestart => Some(UpdateOffer {
            label: tr("Restart to update").to_owned(),
            #[cfg(any(target_os = "macos", test))]
            menu_title: menu_label("Restart to Update", tr("Restart to update")),
            action: DesktopAction::RestartIntoUpdate,
            held_by_meeting,
        }),
        DesktopUpdateStatus::Available => Some(UpdateOffer {
            label: tr("Download update").to_owned(),
            #[cfg(any(target_os = "macos", test))]
            menu_title: menu_label("Download Update…", tr("Download update")),
            action: DesktopAction::OpenUpdate,
            held_by_meeting,
        }),
        DesktopUpdateStatus::SwitchAvailable { channel, version } => {
            let label = switch_label(channel, version);
            #[cfg(any(target_os = "macos", test))]
            let menu_title = format!("{label}…");
            Some(UpdateOffer {
                label,
                #[cfg(any(target_os = "macos", test))]
                menu_title,
                action: DesktopAction::ConfirmUpdateSwitch,
                held_by_meeting,
            })
        }
        DesktopUpdateStatus::Unavailable
        | DesktopUpdateStatus::Managed
        | DesktopUpdateStatus::Checking
        | DesktopUpdateStatus::Current
        | DesktopUpdateStatus::Failed
        | DesktopUpdateStatus::Downloading
        | DesktopUpdateStatus::SourceBuild
        | DesktopUpdateStatus::PackageManager
        | DesktopUpdateStatus::PackageUpdate { .. } => None,
    }
}

/// Why `status` cannot run right now while `meeting_recording` is true.
pub(crate) fn offer_hold(
    status: &DesktopUpdateStatus,
    meeting_recording: bool,
) -> Option<&'static str> {
    (meeting_recording && held_by_meeting(status)).then_some(MEETING_RESTART_HOLD)
}

/// The macOS menu-bar title for `status`: the active offer's menu title, or
/// `"Check for Updates…"` when no offer is waiting.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn menu_title(status: &DesktopUpdateStatus) -> String {
    offer(status).map_or_else(
        || menu_label("Check for Updates…", tr("Check for updates")),
        |offer| offer.menu_title,
    )
}

/// The action of the single macOS menu-bar update item for `status`.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn menu_action(status: &DesktopUpdateStatus) -> DesktopAction {
    offer(status).map_or(DesktopAction::CheckForUpdates, |offer| offer.action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::i18n;

    #[test]
    fn offers_and_menu_actions_stay_consistent_across_every_status() {
        i18n::apply(Some("en"));
        let ready = offer(&DesktopUpdateStatus::ReadyToRestart).expect("ready offer");
        assert_eq!(ready.label, "Restart to update");
        assert_eq!(ready.menu_title, "Restart to Update");
        assert_eq!(ready.action, DesktopAction::RestartIntoUpdate);
        assert!(ready.held_by_meeting);
        assert_eq!(
            menu_title(&DesktopUpdateStatus::ReadyToRestart),
            "Restart to Update"
        );
        assert_eq!(
            menu_action(&DesktopUpdateStatus::ReadyToRestart),
            DesktopAction::RestartIntoUpdate
        );
        assert_eq!(
            offer_hold(&DesktopUpdateStatus::ReadyToRestart, true),
            Some(MEETING_RESTART_HOLD)
        );
        assert_eq!(
            offer_hold(&DesktopUpdateStatus::ReadyToRestart, false),
            None
        );

        let available = offer(&DesktopUpdateStatus::Available).expect("available offer");
        assert_eq!(available.label, "Download update");
        assert_eq!(available.menu_title, "Download Update…");
        assert_eq!(available.action, DesktopAction::OpenUpdate);
        assert!(!available.held_by_meeting);
        assert_eq!(
            menu_title(&DesktopUpdateStatus::Available),
            "Download Update…"
        );
        assert_eq!(
            menu_action(&DesktopUpdateStatus::Available),
            DesktopAction::OpenUpdate
        );
        assert_eq!(offer_hold(&DesktopUpdateStatus::Available, true), None);

        let switch = DesktopUpdateStatus::SwitchAvailable {
            channel: "stable".into(),
            version: "2.3.1".into(),
        };
        let switch_offer = offer(&switch).expect("switch offer");
        assert_eq!(switch_offer.label, "Switch to Stable 2.3.1");
        assert_eq!(switch_offer.menu_title, "Switch to Stable 2.3.1…");
        assert_eq!(switch_offer.action, DesktopAction::ConfirmUpdateSwitch);
        assert!(!switch_offer.held_by_meeting);
        assert_eq!(menu_title(&switch), "Switch to Stable 2.3.1…");
        assert_eq!(menu_action(&switch), DesktopAction::ConfirmUpdateSwitch);
        assert_eq!(offer_hold(&switch, true), None);

        let dev_switch = DesktopUpdateStatus::SwitchAvailable {
            channel: "dev".into(),
            version: "0.0.0-dev-42".into(),
        };
        assert_eq!(
            offer(&dev_switch).unwrap().label,
            "Switch to Dev 0.0.0-dev-42"
        );

        let non_offers = [
            DesktopUpdateStatus::Unavailable,
            DesktopUpdateStatus::Managed,
            DesktopUpdateStatus::Checking,
            DesktopUpdateStatus::Current,
            DesktopUpdateStatus::Failed,
            DesktopUpdateStatus::Downloading,
            DesktopUpdateStatus::SourceBuild,
            DesktopUpdateStatus::PackageManager,
            DesktopUpdateStatus::PackageUpdate {
                command: "pacman -Syu".into(),
            },
        ];
        for status in &non_offers {
            assert_eq!(offer(status), None, "{status:?} should have no CTA offer");
            assert_eq!(menu_title(status), "Check for Updates…", "{status:?}");
            assert_eq!(
                menu_action(status),
                DesktopAction::CheckForUpdates,
                "{status:?}"
            );
            assert_eq!(offer_hold(status, true), None, "{status:?}");
        }
    }

    #[test]
    fn presentation_follows_the_active_translation() {
        i18n::apply(Some("zh"));
        let ready = offer(&DesktopUpdateStatus::ReadyToRestart).unwrap();
        assert_eq!(ready.label, "重启以更新");
        assert_eq!(ready.menu_title, "重启以更新");
        assert_eq!(menu_title(&DesktopUpdateStatus::Current), "检查更新…");
        assert_eq!(menu_title(&DesktopUpdateStatus::Available), "下载更新…");
        let switch = DesktopUpdateStatus::SwitchAvailable {
            channel: "stable".into(),
            version: "2.3.1".into(),
        };
        assert_eq!(
            menu_title(&switch),
            format!("{}…", offer(&switch).unwrap().label)
        );
        i18n::apply(Some("en"));
    }
}
