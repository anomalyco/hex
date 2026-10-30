//! The `playground` experiment: Rune's playground in Hex's window. ⇧⌘D
//! (Ctrl+Shift+D off macOS) swaps the window for a folded list of
//! prototypes (the onboarding options, live meeting notes, and on macOS
//! the HUD and the capture capsule), then every fixture
//! scene in [`SCENES`] under a readable name, by pane. Each opens on a fresh
//! fixture state; the real window's state keeps running underneath,
//! untouched. Off, nothing is attached: no shortcut, menu item, or scene
//! list.

use std::sync::OnceLock;

use rune::{App, AppSpec, Playground};

use super::{DesktopShortcuts, SCENES, Scene, Ui, WINDOW_SIZE, fixture};
use crate::experiments::{self, Experiment};

/// The list's top-level groups, in order. Each starts folded.
const GROUPS: &[&str] = &[PROTOTYPES, WINDOW, LINUX_WINDOWS];
pub(super) const PROTOTYPES: &str = "Prototypes";
const WINDOW: &str = "Window";
const LINUX_WINDOWS: &str = "Linux & Windows";

/// Where each fixture scene lists: its playground path, by its `--scene`
/// name. The list follows this order. Every scene but `live` has one.
const PATHS: &[(&str, &str)] = &[
    ("mac-settings", "Window/General/Settings"),
    ("mac-settings-sides", "Window/General/Left Option shortcut"),
    ("mac-settings-advanced", "Window/General/Advanced"),
    ("mac-settings-mic-level", "Window/General/Microphone level"),
    (
        "mac-settings-mic-released",
        "Window/General/Microphone released while idle",
    ),
    (
        "mac-settings-ducking",
        "Window/General/Duck while dictating",
    ),
    (
        "mac-settings-pause-browser",
        "Window/General/Pause the browser too",
    ),
    (
        "mac-settings-permissions",
        "Window/General/Permissions missing",
    ),
    (
        "mac-microphone-missing",
        "Window/General/Microphone missing",
    ),
    ("mac-microphone-lid-closed", "Window/General/Lid closed"),
    (
        "mac-microphone-bluetooth",
        "Window/General/Bluetooth default passed over",
    ),
    ("mac-settings-vocabulary", "Window/General/Vocabulary"),
    (
        "mac-settings-vocabulary-empty",
        "Window/General/Vocabulary · empty",
    ),
    (
        "vocabulary-unused",
        "Window/General/Vocabulary · not used by the model",
    ),
    ("shortcut-row", "Window/General/Shortcut/Left Option"),
    ("shortcut-row-right", "Window/General/Shortcut/Right Option"),
    (
        "shortcut-row-sided-chord",
        "Window/General/Shortcut/Sided chord",
    ),
    (
        "shortcut-row-recording",
        "Window/General/Shortcut/Recording",
    ),
    (
        "mac-settings-recording",
        "Window/General/Shortcut/Recording · held",
    ),
    ("shortcut-row-held", "Window/General/Shortcut/Held"),
    ("shortcut-row-rejected", "Window/General/Shortcut/Rejected"),
    ("shortcut-row-saved", "Window/General/Shortcut/Saved"),
    ("mac-update-checking", "Window/General/Updates/Checking"),
    (
        "mac-update-downloading",
        "Window/General/Updates/Downloading",
    ),
    (
        "mac-update-ready",
        "Window/General/Updates/Ready to restart",
    ),
    (
        "mac-settings-dev-channel",
        "Window/General/Updates/Dev channel",
    ),
    ("mac-local-build", "Window/General/Updates/Local build"),
    ("mac-test-build", "Window/General/Updates/Test build"),
    ("picker", "Window/Transcription model/Picker"),
    (
        "picker-downloading",
        "Window/Transcription model/Downloading",
    ),
    (
        "picker-japanese",
        "Window/Transcription model/Japanese saved",
    ),
    ("language-menu", "Window/Transcription model/Language menu"),
    (
        "language-fallback",
        "Window/Transcription model/Language fallback",
    ),
    ("shortcut-explainer", "Window/Shortcut explainer/Hold"),
    (
        "shortcut-explainer-toggle",
        "Window/Shortcut explainer/Toggle",
    ),
    (
        "shortcut-explainer-chord",
        "Window/Shortcut explainer/Chord",
    ),
    (
        "shortcut-explainer-motion",
        "Window/Shortcut explainer/Looping",
    ),
    ("sound-sets", "Window/Sounds/Sheet"),
    ("sound-sets-playing", "Window/Sounds/Playing"),
    ("output", "Window/Output/Steps"),
    ("output-open", "Window/Output/App rule open"),
    ("output-empty", "Window/Output/Empty"),
    ("modes", "Window/Modes/Modes"),
    ("modes-global", "Window/Modes/Global mode"),
    ("modes-picker", "Window/Modes/App picker"),
    (
        "modes-transformation-menu",
        "Window/Modes/Transformation menu",
    ),
    ("modes-opencode-v1", "Window/Modes/OpenCode V1"),
    (
        "mac-input-source-models",
        "Window/General/Input source models",
    ),
    ("meetings", "Window/Meetings/Meetings"),
    ("meetings-recording", "Window/Meetings/Recording"),
    ("meetings-chapters", "Window/Meetings/Chapters"),
    ("meetings-actions", "Window/Meetings/Actions menu"),
    ("meetings-rename", "Window/Meetings/Rename"),
    ("meetings-delete", "Window/Meetings/Delete"),
    ("meetings-failed", "Window/Meetings/Failed"),
    ("meetings-empty", "Window/Meetings/Empty"),
    ("meetings-off", "Window/Meetings/Off"),
    ("meetings-settings", "Window/Meetings/Settings"),
    ("meetings-allow", "Window/Meetings/Allow system audio"),
    ("meetings-calendar", "Window/Meetings/Calendar"),
    (
        "meetings-calendar-denied",
        "Window/Meetings/Calendar denied",
    ),
    ("history", "Window/History/History"),
    ("history-empty", "Window/History/Empty"),
    #[cfg(hex_capture)]
    ("mac-scrawl", "Window/Annotate/On"),
    #[cfg(hex_capture)]
    ("mac-scrawl-off", "Window/Annotate/Off"),
    #[cfg(hex_capture)]
    ("mac-scrawl-running", "Window/Annotate/Scrawl.app running"),
    #[cfg(hex_capture)]
    (
        "mac-scrawl-permission",
        "Window/Annotate/Screen Recording missing",
    ),
    #[cfg(hex_capture)]
    (
        "mac-scrawl-restart",
        "Window/Annotate/Screen Recording needs restart",
    ),
    ("mac-voice-filter", "Window/Voice Filter/Voice Filter"),
    ("mac-voice-filter-muted", "Window/Voice Filter/Muted"),
    (
        "mac-voice-filter-driver-missing",
        "Window/Voice Filter/Driver missing",
    ),
    ("mac-hud-lab", "Window/HUD Lab/HUD Lab"),
    ("mac-labs", "Window/General/Labs & experiments"),
    ("mac-production", "Window/General/Production build"),
    ("mac-production-labs", "Window/General/Production Labs"),
    ("mac-activity", "Window/Activity/Activity"),
    ("mac-onboarding", "Window/Onboarding tour/Welcome"),
    ("onboarding-hero", "Window/Onboarding tour/Welcome · hero"),
    (
        "onboarding-hero-still",
        "Window/Onboarding tour/Welcome · still",
    ),
    ("mac-onboarding-setup", "Window/Onboarding tour/Setup"),
    (
        "mac-onboarding-progress",
        "Window/Onboarding tour/Setup · progress",
    ),
    (
        "mac-onboarding-denied",
        "Window/Onboarding tour/Setup · denied",
    ),
    (
        "mac-onboarding-restart",
        "Window/Onboarding tour/Setup · restart",
    ),
    (
        "mac-onboarding-download-failed",
        "Window/Onboarding tour/Setup · download failed",
    ),
    ("mac-onboarding-try", "Window/Onboarding tour/Try it"),
    (
        "mac-onboarding-listening",
        "Window/Onboarding tour/Try it · listening",
    ),
    (
        "mac-onboarding-landed",
        "Window/Onboarding tour/Try it · landed",
    ),
    (
        "mac-onboarding-reopen",
        "Window/Onboarding tour/Try it · reopen",
    ),
    ("mac-onboarding-mic", "Window/Onboarding tour/Mic · hearing"),
    (
        "mac-onboarding-mic-open",
        "Window/Onboarding tour/Mic · open",
    ),
    (
        "mac-onboarding-mic-silent",
        "Window/Onboarding tour/Mic · silent",
    ),
    (
        "mac-onboarding-mic-lid",
        "Window/Onboarding tour/Mic · lid closed",
    ),
    ("mac-onboarding-done", "Window/Onboarding tour/Done"),
    ("portal", "Linux & Windows/Settings · portal shortcuts"),
    ("commands", "Linux & Windows/Settings · hex commands"),
    ("portal-mouse", "Linux & Windows/Settings · mouse button"),
    ("windows-settings", "Linux & Windows/Settings · Windows"),
    ("disabled", "Linux & Windows/Disabled"),
    ("capturing", "Linux & Windows/Recording a shortcut"),
    ("menus", "Linux & Windows/Microphone menu"),
    ("chinese", "Linux & Windows/Chinese"),
    ("onboarding", "Linux & Windows/Onboarding"),
    ("onboarding-setup", "Linux & Windows/Onboarding · setup"),
];

pub(crate) fn enabled() -> bool {
    experiments::enabled(Experiment::Playground)
}

/// `spec` with the playground's scenes: every fixture scene but `live`, at
/// its path in [`PATHS`], each starting from a plain fixture. Only the
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
        .sizes(&[("Hex window", WINDOW_SIZE)])
        .group_order(GROUPS)
        .start_folded(true);
    let playground = super::onboarding_kit::stories(playground);
    let playground = super::live_notes::add(playground);
    #[cfg(target_os = "macos")]
    let playground = super::hud_stories::add(playground);
    #[cfg(all(hex_capture, target_os = "macos"))]
    let playground = super::capture_capsule::playground::stories(playground);
    app.playground(playground)
}

fn blank() -> Ui {
    fixture(DesktopShortcuts::Registered)
}

fn scenes() -> &'static [Scene] {
    static NAMED: OnceLock<Vec<Scene>> = OnceLock::new();
    NAMED.get_or_init(|| {
        // In `PATHS`' order; a scene without a path lists last, by name.
        let mut listed: Vec<Scene> = SCENES
            .iter()
            .filter(|(name, _)| *name != "live")
            .copied()
            .collect();
        listed.sort_by_key(|(name, _)| {
            PATHS
                .iter()
                .position(|(scene, _)| scene == name)
                .unwrap_or(PATHS.len())
        });
        listed
            .into_iter()
            .map(|(name, apply)| (path(name).unwrap_or(name), apply))
            .collect()
    })
}

fn path(scene: &str) -> Option<&'static str> {
    PATHS
        .iter()
        .find(|(name, _)| *name == scene)
        .map(|(_, path)| *path)
}

#[cfg(any(target_os = "macos", test))]
/// The groups a story or scene listed under before they nested, for links
/// made then.
const OLD_GROUPS: &[&str] = &[
    "General",
    "Transcription picker",
    "Shortcut explainer",
    "Modes",
    "Commands",
    "Meetings",
    "History",
    "Scrawl",
    "Voice Filter",
    "HUD Lab",
    "Activity",
    "Onboarding",
];

#[cfg(any(target_os = "macos", test))]
/// The groups of prototypes that listed at the top before they moved under
/// [`PROTOTYPES`].
const OLD_PROTOTYPES: &[&str] = &["Onboarding", "HUD", "Capture capsule"];

#[cfg(any(target_os = "macos", test))]
/// Today's name for a story link made before the list nested: a scene's
/// `--scene` name, its old `Group/scene` path or that path's slug, or a
/// prototype's path or slug without `Prototypes/`. `None` when `story`
/// needs no change.
pub(super) fn alias(story: &str) -> Option<String> {
    let scene = |name: &str| path(name).map(str::to_owned);
    if let Some(path) = scene(story) {
        return Some(path);
    }
    for group in OLD_GROUPS {
        for prefix in [
            format!("{group}/"),
            format!("{}-", rune::playground::slug(group)),
        ] {
            if let Some(name) = strip_prefix_ignore_case(story, &prefix)
                && let Some(path) = scene(name)
            {
                return Some(path);
            }
        }
    }
    for group in OLD_PROTOTYPES {
        if strip_prefix_ignore_case(story, &format!("{group}/")).is_some() {
            return Some(format!("{PROTOTYPES}/{story}"));
        }
        if strip_prefix_ignore_case(story, &format!("{}-", rune::playground::slug(group))).is_some()
        {
            return Some(format!("{}-{story}", rune::playground::slug(PROTOTYPES)));
        }
    }
    None
}

#[cfg(any(target_os = "macos", test))]
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rune_ui::Pane;

    #[test]
    fn every_scene_but_live_has_a_path_under_a_group() {
        for (name, _) in SCENES.iter().filter(|(name, _)| *name != "live") {
            let path = path(name).unwrap_or_else(|| panic!("{name} has no path in PATHS"));
            let top = path.split('/').next().unwrap_or_default();
            assert!(GROUPS.contains(&top), "{path} isn't under a group");
            assert!(path.matches('/').count() >= 1, "{path} names no group");
        }
        for (name, path) in PATHS {
            assert!(
                SCENES.iter().any(|(scene, _)| scene == name),
                "{path} names no scene {name}"
            );
            let paths = PATHS.iter().filter(|(_, other)| other == path).count();
            assert_eq!(paths, 1, "{path} lists twice");
        }
        let names: Vec<&str> = scenes().iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), SCENES.len() - 1);
        assert_eq!(names.first(), Some(&"Window/General/Settings"));
        assert!(
            names
                .last()
                .is_some_and(|name| name.starts_with("Linux & Windows/"))
        );
    }

    /// A scene hears the window's ⌘-digit pane shortcuts as soon as it opens
    /// from the list, without the stage taking the keyboard first, and the
    /// real window's state stays where it was.
    #[test]
    fn a_scene_opened_from_the_list_hears_command_digits() {
        let app = attach(App::new(spec(super::super::spec())));
        let mut rt = app.headless_runtime(WINDOW_SIZE, rune::Env::new(false, 1.0));
        rt.frame(0.0);
        rt.open_playground("Window/General/Settings", None);
        let stage = rt.playground_state::<Ui>().expect("the scene's state");
        assert_eq!(stage.pane, Pane::General);
        let second = *super::super::labs::order(stage)
            .get(1)
            .expect("a second pane");
        rt.key(rune::KeyEvent {
            key: rune::Key::Command('2'),
            mods: rune::Mods {
                cmd: true,
                ..rune::Mods::default()
            },
        });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.playground_state::<Ui>().map(|ui| ui.pane), Some(second));
        assert_eq!(rt.state.pane, Pane::General);
    }

    /// The playground's own keys still win while a story that takes keys
    /// is open: ↓ moves down the list and ⇧⌘D closes it.
    #[test]
    fn the_playgrounds_keys_work_with_a_story_open() {
        let app = attach(App::new(spec(super::super::spec())));
        let mut rt = app.headless_runtime(WINDOW_SIZE, rune::Env::new(false, 1.0));
        rt.frame(0.0);
        rt.open_playground("Window/General/Settings", None);
        let settings = rt
            .playground_state::<Ui>()
            .map(|ui| ui.settings.dictation_hotkey.clone());
        rt.key(rune::KeyEvent {
            key: rune::Key::Down,
            mods: rune::Mods::default(),
        });
        rt.frame(1.0 / 60.0);
        let next = rt
            .playground_state::<Ui>()
            .map(|ui| ui.settings.dictation_hotkey.clone());
        assert!(next.is_some());
        assert_ne!(next, settings, "↓ opened Left Option shortcut");
        rt.key(rune::KeyEvent {
            key: rune::Key::Command('d'),
            mods: rune::Mods {
                cmd: true,
                ctrl: cfg!(not(target_os = "macos")),
                shift: true,
                ..rune::Mods::default()
            },
        });
        rt.frame(2.0 / 60.0);
        assert!(rt.playground_state::<Ui>().is_none(), "⇧⌘D closed it");
    }

    /// Every HUD story, on every variant, opens from its `hex://story` link.
    #[cfg(target_os = "macos")]
    #[test]
    fn every_hud_story_and_variant_opens_from_its_link() {
        crate::dot_scope::set_lab(crate::dot_scope::Tuning::default());
        let app = attach(App::new(spec(super::super::spec())));
        let mut rt = app.headless_runtime(WINDOW_SIZE, rune::Env::new(false, 1.0));
        rt.frame(0.0);
        let looks = ["flow", "pulse", "sweep"];
        let endings = ["done", "cancel", "fail"];
        for story in ["notch", "pretend-notch", "menu-bar", "below-the-menu-bar"] {
            for look in looks {
                for ending in endings {
                    open_link(
                        &mut rt,
                        &format!("hex://story/prototypes/hud/{story}?variant={look}-{ending}"),
                    );
                }
            }
        }
        for story in ["processing", "your-voice"] {
            for variant in ["hud-lab", "notch", "pretend-notch", "menu-bar", "below"] {
                open_link(
                    &mut rt,
                    &format!("hex://story/prototypes/hud/{story}?variant={variant}"),
                );
            }
        }
        open_link(&mut rt, "hex://story/hud/processing?variant=menu-bar");
        for control in [
            "dot-scope-reset",
            "dot-scope-copy",
            "dot-scope-save",
            "dot-processing",
            "dot-placement",
            "dot-auto-level",
            "dot-spacing",
        ] {
            assert!(
                rt.find(control).is_some(),
                "HUD story side panel lacked {control}"
            );
        }
    }

    /// Every capture-capsule story, on every flow after a capture, opens
    /// from its `hex://story` link.
    #[cfg(all(hex_capture, target_os = "macos"))]
    #[test]
    fn every_capsule_story_and_flow_opens_from_its_link() {
        let app = attach(App::new(spec(super::super::spec())));
        let mut rt = app.headless_runtime(WINDOW_SIZE, rune::Env::new(false, 1.0));
        rt.frame(0.0);
        for story in [
            "whole-flow",
            "rule",
            "suggestions",
            "shift",
            "sent-rule",
            "search",
            "picked",
            "taken-back",
            "recording",
            "discard",
            "permission",
        ] {
            for flow in ["tuned", "list", "toast", "menu-bar", "cursor", "notch"] {
                open_link(
                    &mut rt,
                    &format!("hex://story/prototypes/capture-capsule/{story}?variant={flow}"),
                );
            }
        }
        for variant in ["Side%20by%20side", "1", "2", "3"] {
            open_link(
                &mut rt,
                &format!("hex://story/prototypes/capture-capsule/notch?variant={variant}"),
            );
        }
        for variant in ["braid", "bars", "line"] {
            open_link(
                &mut rt,
                &format!("hex://story/prototypes/capture-capsule/family?variant={variant}"),
            );
        }
    }

    /// Opens `link` as the URL handler does; panics when its story or
    /// variant isn't there.
    #[cfg(target_os = "macos")]
    fn open_link(rt: &mut rune::Runtime<Ui>, link: &str) {
        let link = super::super::story_link::parse(link, "hex").expect(link);
        rt.open_playground(&link.story, link.variant.as_deref());
        assert!(
            rt.playground_state::<Ui>().is_none(),
            "{} is a story",
            link.story
        );
    }

    #[test]
    fn old_links_find_their_story() {
        for (old, new) in [
            ("mac-settings", "Window/General/Settings"),
            ("General/mac-settings", "Window/General/Settings"),
            ("general-mac-settings", "Window/General/Settings"),
            ("Meetings/meetings-calendar", "Window/Meetings/Calendar"),
            (
                "onboarding-onboarding-setup",
                "Linux & Windows/Onboarding · setup",
            ),
            ("onboarding-hero", "Window/Onboarding tour/Welcome · hero"),
            ("HUD/Processing", "Prototypes/HUD/Processing"),
            (
                "hud-recording-your-voice",
                "prototypes-hud-recording-your-voice",
            ),
            (
                "Onboarding/Option · Pill",
                "Prototypes/Onboarding/Option · Pill",
            ),
            (
                "onboarding-option-pill",
                "prototypes-onboarding-option-pill",
            ),
            ("capture-capsule-notch", "prototypes-capture-capsule-notch"),
        ] {
            assert_eq!(alias(old).as_deref(), Some(new), "{old}");
        }
        for current in [
            "Prototypes/HUD/Processing",
            "prototypes-hud-processing",
            "Processing",
            "Option · Pill",
            "Window/General/Settings",
        ] {
            assert_eq!(alias(current), None, "{current}");
        }
    }
}
