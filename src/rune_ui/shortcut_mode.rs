//! Shortcut mode: one choice of how the dictation shortcut triggers, in place
//! of the three saved settings behind it (`shortcut_mode`, `double_tap_lock`,
//! `double_tap_only`). Of their eight combinations only four behave
//! differently: Toggle ignores both double-tap settings, and double-tap only
//! needs the lock and a shortcut with a key. General's row names the choice
//! and opens the modal in `explainer` that makes it.

use rune::El;

use super::Ui;
use super::controls::{Paint, copy_row};
use super::explainer::{self, Script};
use super::general::value_button;
use super::icons::HAND;
use crate::app_settings::{AppSettings, HotkeyKey};
use crate::desktop::i18n::{tr, tr_fill};
use crate::dictation::ShortcutMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trigger {
    HoldOrDoubleTap,
    Hold,
    Tap,
    DoubleTap,
}

impl Trigger {
    /// In the modal's order, the default first.
    pub(super) const ALL: [Self; 4] = [
        Self::HoldOrDoubleTap,
        Self::Hold,
        Self::Tap,
        Self::DoubleTap,
    ];

    pub(super) fn of(settings: &AppSettings) -> Self {
        match settings.shortcut_mode {
            ShortcutMode::Toggle => Self::Tap,
            ShortcutMode::Hold if !settings.double_tap_lock => Self::Hold,
            ShortcutMode::Hold
                if settings.double_tap_only && settings.dictation_hotkey.key.is_some() =>
            {
                Self::DoubleTap
            }
            ShortcutMode::Hold => Self::HoldOrDoubleTap,
        }
    }

    pub(super) fn apply(self, settings: &mut AppSettings) {
        let (mode, lock, only) = match self {
            Self::HoldOrDoubleTap => (ShortcutMode::Hold, true, false),
            Self::Hold => (ShortcutMode::Hold, false, false),
            Self::Tap => (ShortcutMode::Toggle, true, false),
            Self::DoubleTap => (ShortcutMode::Hold, true, true),
        };
        settings.shortcut_mode = mode;
        settings.double_tap_lock = lock;
        settings.double_tap_only = only;
    }

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::HoldOrDoubleTap => "trigger-hold-or-double-tap",
            Self::Hold => "trigger-hold",
            Self::Tap => "trigger-tap",
            Self::DoubleTap => "trigger-double-tap",
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::HoldOrDoubleTap => tr("Hold or double-tap"),
            Self::Hold => tr("Hold"),
            Self::Tap => tr("Tap"),
            Self::DoubleTap => tr("Double-tap"),
        }
    }

    pub(super) fn caption(self) -> &'static str {
        match self {
            Self::HoldOrDoubleTap => tr("Hold to talk, or double-tap to talk hands-free"),
            Self::Hold => tr("Hold to talk, let go to paste"),
            Self::Tap => tr("Tap to talk hands-free, tap again to paste"),
            Self::DoubleTap => tr("Double-tap to talk hands-free, tap again to paste"),
        }
    }

    pub(super) fn script(self) -> &'static Script {
        match self {
            Self::HoldOrDoubleTap => &explainer::HOLD_OR_LOCK,
            Self::Hold => &explainer::HOLD,
            Self::Tap => &explainer::TAP,
            Self::DoubleTap => &explainer::DOUBLE_TAP,
        }
    }

    /// Why this shortcut can't trigger this way, if it can't: a lone
    /// modifier can't tell a double-tap from a chord.
    pub(super) fn unavailable(self, settings: &AppSettings) -> Option<String> {
        (self == Self::DoubleTap && settings.dictation_hotkey.key.is_none()).then(|| {
            let mut example = settings.dictation_hotkey.clone();
            example.key = Some(HotkeyKey::named("space"));
            tr_fill(
                "Needs a shortcut with a key, like {}",
                &example.keycaps().join("+"),
            )
        })
    }
}

/// Saves `trigger` unless it is already the choice.
pub(super) fn pick(ui: &mut Ui, trigger: Trigger) {
    if Trigger::of(&ui.settings) != trigger {
        ui.update(|settings| trigger.apply(settings));
    }
}

/// General's row, under the shortcut: the choice, opening the modal.
pub(super) fn row(ui: &Ui, p: Paint) -> El {
    copy_row(
        p,
        "shortcut-mode",
        Some(&HAND),
        tr("Shortcut mode"),
        None,
        value_button(
            p,
            "shortcut-mode-control",
            Trigger::of(&ui.settings).name(),
            &rune_theme_opencode::icons::CHEVRON_RIGHT,
            explainer::open,
        ),
        false,
    )
}

/// The modal's scenes.
enum Example {
    /// Option alone, which can't double-tap.
    Modifier,
    /// Option-Space, double-tapping.
    Key,
    /// A three-key chord: Control-Option-Space.
    Chord,
    /// Loops, for `--frames` clips.
    Motion,
}

pub(super) fn scene_modifier(ui: &mut Ui) {
    scene(ui, Example::Modifier);
}

pub(super) fn scene_key(ui: &mut Ui) {
    scene(ui, Example::Key);
}

pub(super) fn scene_chord(ui: &mut Ui) {
    scene(ui, Example::Chord);
}

pub(super) fn scene_motion(ui: &mut Ui) {
    scene(ui, Example::Motion);
}

fn scene(ui: &mut Ui, example: Example) {
    *ui = super::mac_fixture();
    let hotkey = &mut ui.settings.dictation_hotkey;
    match example {
        Example::Modifier | Example::Motion => hotkey.key = None,
        Example::Key => {
            hotkey.key = Some(HotkeyKey::named("space"));
            Trigger::DoubleTap.apply(&mut ui.settings);
        }
        Example::Chord => {
            hotkey.modifiers.control = hotkey.modifiers.option;
            hotkey.key = Some(HotkeyKey::named("space"));
        }
    }
    let held = !matches!(example, Example::Motion);
    explainer::fixture(ui, held.then_some(explainer::SNAPSHOT_MS));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(key: bool) -> AppSettings {
        let mut settings = AppSettings::default();
        settings.dictation_hotkey.key = key.then(|| HotkeyKey::named("space"));
        settings
    }

    #[test]
    fn each_trigger_reads_back_as_itself() {
        let mut settings = settings(true);
        for trigger in Trigger::ALL {
            trigger.apply(&mut settings);
            assert_eq!(Trigger::of(&settings), trigger);
        }
    }

    #[test]
    fn the_default_is_hold_or_double_tap() {
        assert_eq!(Trigger::of(&settings(false)), Trigger::HoldOrDoubleTap);
    }

    /// The eight saved combinations fold onto the four triggers.
    #[test]
    fn redundant_and_invalid_combinations_fold_away() {
        for key in [false, true] {
            for (lock, only) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut settings = settings(key);
                settings.double_tap_lock = lock;
                settings.double_tap_only = only;
                settings.shortcut_mode = ShortcutMode::Toggle;
                assert_eq!(Trigger::of(&settings), Trigger::Tap);
                settings.shortcut_mode = ShortcutMode::Hold;
                let expected = match (lock, only && key) {
                    (false, _) => Trigger::Hold,
                    (true, true) => Trigger::DoubleTap,
                    (true, false) => Trigger::HoldOrDoubleTap,
                };
                assert_eq!(
                    Trigger::of(&settings),
                    expected,
                    "lock {lock} only {only} key {key}"
                );
            }
        }
    }

    #[test]
    fn double_tap_needs_a_key() {
        assert!(Trigger::DoubleTap.unavailable(&settings(false)).is_some());
        assert!(Trigger::DoubleTap.unavailable(&settings(true)).is_none());
        for trigger in [Trigger::HoldOrDoubleTap, Trigger::Hold, Trigger::Tap] {
            assert!(trigger.unavailable(&settings(false)).is_none());
        }
    }
}
