//! The `playground` experiment: Rune's playground in Hex's window. ⇧⌘D
//! (Ctrl+Shift+D off macOS) swaps the window for every fixture scene in
//! [`SCENES`], grouped by the pane it shows, the onboarding options'
//! stories, and, in builds with captures, the capture capsule's stories. Each opens on a fresh fixture state; the
//! real window's state keeps running underneath, untouched. Off, nothing is
//! attached: no shortcut, menu item, or scene list.

use std::sync::OnceLock;

use rune::{App, AppSpec, Playground};

use super::{DesktopShortcuts, Pane, SCENES, Scene, Ui, WINDOW_SIZE, fixture};
use crate::experiments::{self, Experiment};

pub(crate) fn enabled() -> bool {
    experiments::enabled(Experiment::Playground)
}

/// `spec` with the playground's scenes: every fixture scene but `live`,
/// named `Group/scene`, each starting from a plain fixture. Only the
/// window's app uses it: headless renders keep the plain scene names that
/// `--scene` takes, unless they ask for `--playground`.
pub(super) fn spec(spec: AppSpec<Ui>) -> AppSpec<Ui> {
    AppSpec {
        init: blank,
        scenes: scenes(),
        ..spec
    }
}

pub(super) fn attach(app: App<Ui>) -> App<Ui> {
    let playground = Playground::new()
        .enabled(true)
        .bar(false)
        .sizes(&[("Hex window", WINDOW_SIZE)]);
    let playground = super::onboarding_kit::stories(playground);
    #[cfg(hex_capture)]
    let playground = super::capture_capsule::playground::stories(playground);
    app.playground(playground)
}

fn blank() -> Ui {
    fixture(DesktopShortcuts::Registered)
}

fn scenes() -> &'static [Scene] {
    static GROUPED: OnceLock<Vec<Scene>> = OnceLock::new();
    GROUPED.get_or_init(|| {
        let mut grouped: Vec<(usize, Scene)> = SCENES
            .iter()
            .filter(|(name, _)| *name != "live")
            .map(|&(name, apply)| {
                let mut ui = blank();
                apply(&mut ui);
                let (rank, group) = group(&ui);
                let path: &'static str = Box::leak(format!("{group}/{name}").into_boxed_str());
                (rank, (path, apply))
            })
            .collect();
        // Stable: scenes keep their order within a group.
        grouped.sort_by_key(|(rank, _)| *rank);
        grouped.into_iter().map(|(_, scene)| scene).collect()
    })
}

/// Where a scene lists, and the group's place in the list: what covers the
/// window (the tour, the picker, the shortcut explainer), else its pane, in
/// the sidebar's order.
fn group(ui: &Ui) -> (usize, &'static str) {
    if ui.tour.is_some() {
        return (100, "Onboarding");
    }
    if ui.picker.language.is_some() {
        return (1, "Transcription picker");
    }
    if ui.explainer.open {
        return (2, "Shortcut explainer");
    }
    let rank = Pane::ALL
        .iter()
        .position(|pane| *pane == ui.pane)
        .unwrap_or(0);
    let rank = if rank == 0 { 0 } else { rank + 2 };
    let name = match ui.pane {
        Pane::General => "General",
        Pane::Modes => "Modes",
        Pane::VoiceAction => "Voice Action",
        Pane::Commands => "Commands",
        Pane::Meetings => "Meetings",
        Pane::History => "History",
        #[cfg(hex_capture)]
        Pane::Scrawl => "Scrawl",
        Pane::VoiceFilter => "Voice Filter",
        Pane::HudLab => "HUD Lab",
        Pane::Activity => "Activity",
    };
    (rank, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_but_live_lists_under_its_pane() {
        let names: Vec<&str> = scenes().iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), SCENES.len() - 1);
        for expected in [
            "General/mac-settings",
            "Transcription picker/picker",
            "Shortcut explainer/shortcut-explainer",
            "General/shortcut-row",
            "Modes/modes-global",
            "Meetings/meetings-calendar",
            "HUD Lab/mac-hud-lab",
            "Activity/mac-activity",
            "Onboarding/mac-onboarding-done",
            "Onboarding/onboarding",
        ] {
            assert!(
                names.contains(&expected),
                "{expected} missing from {names:?}"
            );
        }
        assert_eq!(names.first(), Some(&"General/portal"));
        assert!(
            names
                .last()
                .is_some_and(|name| name.starts_with("Onboarding/"))
        );
    }
}
