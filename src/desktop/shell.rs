//! Product-level state shared by the desktop window roots while their native
//! lifecycle and remaining pane renderers are converging.

use gpui::{AnyElement, Context, IntoElement, prelude::*};

use crate::desktop_host::DesktopCapabilities;
use crate::desktop_ui::{NavigationIcon, navigation_item};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopPane {
    Settings,
    Modes,
    Commands,
    VoiceAction,
    History,
    HudLab,
    Meetings,
    Activity,
}

impl DesktopPane {
    const ALL: [Self; 8] = [
        Self::Settings,
        Self::Modes,
        Self::Commands,
        Self::VoiceAction,
        Self::History,
        Self::HudLab,
        Self::Meetings,
        Self::Activity,
    ];

    pub(crate) fn available(capabilities: DesktopCapabilities) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|pane| match pane {
                Self::Settings => true,
                Self::Modes => capabilities.modes,
                Self::Commands => capabilities.commands,
                Self::VoiceAction => capabilities.voice_action,
                Self::History => capabilities.history,
                Self::HudLab => capabilities.hud_lab,
                Self::Meetings => capabilities.meetings,
                Self::Activity => capabilities.activity,
            })
            .collect()
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Modes => "Modes",
            Self::Commands => "Commands",
            Self::VoiceAction => "Voice Action",
            Self::History => "History",
            Self::HudLab => "HUD Lab",
            Self::Meetings => "Meetings",
            Self::Activity => "Activity",
        }
    }

    pub(crate) const fn icon(self) -> NavigationIcon {
        match self {
            Self::Settings => NavigationIcon::Settings,
            Self::Modes => NavigationIcon::Modes,
            Self::Commands => NavigationIcon::Commands,
            Self::VoiceAction => NavigationIcon::VoiceAction,
            Self::History => NavigationIcon::History,
            Self::HudLab => NavigationIcon::HudLab,
            Self::Meetings => NavigationIcon::Meetings,
            Self::Activity => NavigationIcon::Activity,
        }
    }

    const fn navigation_id(self) -> &'static str {
        match self {
            Self::Settings => "desktop-nav-settings",
            Self::Modes => "desktop-nav-modes",
            Self::Commands => "desktop-nav-commands",
            Self::VoiceAction => "desktop-nav-voice-action",
            Self::History => "desktop-nav-history",
            Self::HudLab => "desktop-nav-hud-lab",
            Self::Meetings => "desktop-nav-meetings",
            Self::Activity => "desktop-nav-activity",
        }
    }
}

pub(crate) fn render_navigation_items<V, L>(
    selected: DesktopPane,
    capabilities: DesktopCapabilities,
    label: L,
    select: fn(&mut V, DesktopPane, &mut Context<V>),
    cx: &mut Context<V>,
) -> Vec<AnyElement>
where
    V: 'static,
    L: Fn(&'static str) -> String + Copy,
{
    DesktopPane::available(capabilities)
        .into_iter()
        .map(|pane| {
            navigation_item(pane.icon(), selected == pane)
                .id(pane.navigation_id())
                .child(label(pane.label()))
                .on_click(cx.listener(move |view, _, _, cx| select(view, pane, cx)))
                .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panes_follow_one_stable_product_order_and_capability_filter() {
        let capabilities = DesktopCapabilities {
            activity: true,
            commands: false,
            history: true,
            hud_lab: false,
            meetings: false,
            modes: true,
            replacements: true,
            listener_control: true,
            update_restart: true,
            voice_action: true,
        };

        assert_eq!(
            DesktopPane::available(capabilities),
            vec![
                DesktopPane::Settings,
                DesktopPane::Modes,
                DesktopPane::VoiceAction,
                DesktopPane::History,
                DesktopPane::Activity,
            ]
        );
    }

    #[test]
    fn linux_navigation_contains_only_panes_with_renderers() {
        let mut expected = vec![
            DesktopPane::Settings,
            DesktopPane::Modes,
            DesktopPane::History,
        ];
        if crate::DEVELOPER_FEATURES_ENABLED {
            expected.push(DesktopPane::HudLab);
        }
        expected.push(DesktopPane::Activity);

        assert_eq!(
            DesktopPane::available(DesktopCapabilities::linux_x11()),
            expected
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_navigation_is_the_capability_filtered_product_order() {
        let mut expected = vec![
            DesktopPane::Settings,
            DesktopPane::Modes,
            DesktopPane::VoiceAction,
            DesktopPane::History,
        ];
        if crate::DEVELOPER_FEATURES_ENABLED {
            expected.push(DesktopPane::HudLab);
        }
        expected.push(DesktopPane::Activity);

        assert_eq!(
            DesktopPane::available(DesktopCapabilities::windows()),
            expected
        );
    }
}
