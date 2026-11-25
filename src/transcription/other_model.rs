//! Labs › Model switch shortcut: the plan to swap the dictation model and
//! language with the saved other one, and back.
//!
//! A plan only says what to switch to; the application-owned preparation in
//! each OS starts it installed-only and commits both selections together
//! once the new model has loaded. Nothing here writes settings or downloads,
//! and a switch that fails leaves the running model and the saved selection
//! exactly as they were.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::app_settings::{AppSettings, KeyboardModelChoice};
use crate::transcription::models::{self, TranscriptionSelection};

/// A shortcut press asks the host that owns preparation to swap. The
/// shortcut is handled on the listener thread and preparation lives with the
/// window/controller, so the request crosses them through this one flag.
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Asks the running app to swap the dictation model.
pub(crate) fn request() {
    REQUESTED.store(true, Ordering::Release);
}

/// Takes a pending request, if any.
pub(crate) fn take_request() -> bool {
    REQUESTED.swap(false, Ordering::AcqRel)
}

/// Why a swap cannot happen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SwitchError {
    /// Labs › Model switch shortcut is off.
    LabOff,
    /// No other model is saved yet.
    NoOther,
    /// The saved other choice isn't a model Hex can switch to.
    Invalid(String),
    /// The model that runs now can't run, so there is no way back.
    CurrentUnavailable(String),
    /// The saved other model isn't installed.
    NotInstalled(String),
    /// Another model preparation is still running.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Busy,
    /// The model that runs now can't be a saved other choice, so there would
    /// be no way back.
    CurrentNotSwappable(String),
}

impl SwitchError {
    /// The HUD's one-line notice, as the interface's English template and its
    /// detail, for the cases a person has to fix. A lab that's off or a
    /// choice not made yet stays in Settings and the log.
    pub(crate) fn notice(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::LabOff | Self::NoOther | Self::Busy => None,
            Self::Invalid(detail)
            | Self::CurrentUnavailable(detail)
            | Self::CurrentNotSwappable(detail) => {
                Some(("Couldn't switch the speech model: {}", detail))
            }
            Self::NotInstalled(name) => {
                Some(("Couldn't switch the speech model: {} isn't installed", name))
            }
        }
    }

    /// Why Settings' transcription row says nothing happened.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::LabOff => "Labs › Model switch shortcut is off".into(),
            Self::NoOther => "No other speech model is chosen".into(),
            Self::Busy => {
                "A model is being prepared. Try the switch again when it finishes.".into()
            }
            Self::Invalid(detail)
            | Self::CurrentUnavailable(detail)
            | Self::CurrentNotSwappable(detail) => {
                format!("Couldn't switch the speech model: {detail}")
            }
            Self::NotInstalled(name) => {
                format!("Couldn't switch the speech model: {name} isn't installed")
            }
        }
    }
}

impl std::fmt::Display for SwitchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

/// What one swap would do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Plan {
    /// The selection the next dictation uses.
    pub(crate) to: TranscriptionSelection,
    /// What the shortcut swaps back to: the selection that runs now.
    pub(crate) other: KeyboardModelChoice,
}

/// The swap `settings` describe, if pressing the shortcut would do one:
/// the lab on, a saved other choice Hex can switch to, and something to
/// switch back to. `Ok(None)` is the choice already running: no reload and
/// no save. Installation is the preparation's job: this only says what to
/// prepare.
pub(crate) fn planned(settings: &AppSettings) -> Result<Option<Plan>, SwitchError> {
    if !crate::labs::Feature::ModelSwitchShortcut.on(settings) {
        return Err(SwitchError::LabOff);
    }
    let Some(choice) = &settings.model_switch_other else {
        return Err(SwitchError::NoOther);
    };
    // The same rule the picker and the keyboard-language rows apply: no
    // Apple Speech, no language the model doesn't support.
    choice
        .validate()
        .map_err(|error| SwitchError::Invalid(format!("{error:#}")))?;
    let to = TranscriptionSelection {
        model: choice.model,
        language: choice.language.clone(),
    };
    if to == settings.transcription {
        return Ok(None);
    }
    // The way back must be the model that runs now, not a fallback that
    // would quietly change it: a missing import on the way back refuses the
    // switch instead.
    let current = crate::transcription::custom_models::runnable(&settings.transcription);
    if current != settings.transcription {
        return Err(SwitchError::CurrentUnavailable(
            models::definition(settings.transcription.model)
                .name
                .to_string(),
        ));
    }
    let other = KeyboardModelChoice {
        model: current.model,
        language: current.language,
    };
    // The model that runs now becomes the way back. Apple Speech can't be
    // saved as the other choice, so the next press would be one-way: refuse
    // before anything is prepared.
    other.validate().map_err(|_| {
        SwitchError::CurrentNotSwappable(models::definition(current.model).name.to_string())
    })?;
    Ok(Some(Plan { to, other }))
}

/// Whether `plan`'s model is installed, the fast check `hex model use` and
/// the hosts make before starting preparation. Preparation still verifies
/// and loads it before anything is saved.
pub(crate) fn installed(plan: &Plan) -> Result<(), SwitchError> {
    let definition =
        models::validate(&plan.to).map_err(|error| SwitchError::Invalid(format!("{error:#}")))?;
    if !crate::transcription::readiness::readiness(&definition, &plan.to.language).installed {
        return Err(SwitchError::NotInstalled(definition.name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::HotkeySlot;
    use crate::transcription::models::{CustomModelId, TranscriptionModelId};

    fn other(model: TranscriptionModelId, language: &str) -> KeyboardModelChoice {
        KeyboardModelChoice {
            model,
            language: language.into(),
        }
    }

    fn enabled(settings: &mut AppSettings, choice: KeyboardModelChoice) {
        settings.labs.model_switch_shortcut = true;
        settings.model_switch_other = Some(choice);
    }

    /// Pressing the shortcut moves to the saved other model and keeps the one
    /// that ran as the way back.
    #[test]
    fn a_plan_moves_to_the_other_model_and_back() {
        let mut settings = AppSettings::default();
        assert_eq!(planned(&settings), Err(SwitchError::LabOff));
        enabled(&mut settings, other(TranscriptionModelId::ParakeetV3, "es"));
        let plan = planned(&settings).unwrap().unwrap();
        assert_eq!(plan.to.model, TranscriptionModelId::ParakeetV3);
        assert_eq!(plan.to.language, "es");
        assert_eq!(plan.other.model, settings.transcription.model);
        assert_eq!(plan.other.language, settings.transcription.language);

        let mut candidate = settings.clone();
        candidate.model_switch_other = Some(plan.other.clone());
        candidate.remember_transcription(plan.to.clone());
        assert_eq!(candidate.transcription, plan.to);
        assert_eq!(candidate.model_switch_other, Some(plan.other.clone()));
        // The way back plans the reverse swap.
        let back = planned(&candidate).unwrap().unwrap();
        assert_eq!(back.to.model, plan.other.model);
        assert_eq!(back.other.model, TranscriptionModelId::ParakeetV3);
    }

    /// Choosing the model already running does nothing: no reload, no save.
    #[test]
    fn the_same_model_is_a_no_op() {
        let mut settings = AppSettings::default();
        let current = settings.transcription.clone();
        enabled(&mut settings, other(current.model, &current.language));
        assert_eq!(planned(&settings), Ok(None));
    }

    /// Apple Speech and a language the model lacks are refused before any
    /// preparation, and a lab that's off reports that instead.
    #[test]
    fn an_invalid_choice_and_a_lab_that_is_off_are_refused() {
        let mut settings = AppSettings::default();
        enabled(
            &mut settings,
            other(TranscriptionModelId::ParakeetUnifiedEnglish, "ja"),
        );
        assert!(matches!(planned(&settings), Err(SwitchError::Invalid(_))));
        settings.model_switch_other = Some(other(TranscriptionModelId::AppleSpeech, "en"));
        assert!(matches!(planned(&settings), Err(SwitchError::Invalid(_))));
        settings.labs.model_switch_shortcut = false;
        assert_eq!(planned(&settings), Err(SwitchError::LabOff));
        settings.labs.model_switch_shortcut = true;
        settings.model_switch_other = None;
        assert_eq!(planned(&settings), Err(SwitchError::NoOther));
    }

    /// Apple Speech can't be the saved other choice, so swapping away from it
    /// is refused rather than being one-way.
    #[test]
    fn a_current_model_that_could_not_be_the_other_choice_refuses_the_switch() {
        let mut settings = AppSettings {
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::AppleSpeech,
                language: "en".into(),
            },
            ..AppSettings::default()
        };
        enabled(&mut settings, other(TranscriptionModelId::ParakeetV3, "es"));
        assert_eq!(
            planned(&settings),
            Err(SwitchError::CurrentNotSwappable("Apple Speech".into()))
        );
    }

    /// A missing import that runs as a fallback can't be the way back: the
    /// switch is refused instead of quietly landing on another model.
    #[test]
    fn a_current_model_that_cannot_run_refuses_the_switch() {
        let mut settings = AppSettings::default();
        let missing = TranscriptionModelId::Custom(
            CustomModelId::parse("custom-0123456789abcdef").expect("16 hex digits"),
        );
        settings.transcription = TranscriptionSelection {
            model: missing,
            language: "en".into(),
        };
        enabled(&mut settings, other(TranscriptionModelId::ParakeetV3, "es"));
        assert!(matches!(
            planned(&settings),
            Err(SwitchError::CurrentUnavailable(_))
        ));
    }

    /// The binding is inactive while its lab is off or no other model is
    /// saved, and it is refused when another shortcut already holds it.
    #[test]
    fn the_shortcut_needs_its_lab_an_other_model_and_its_own_keys() {
        use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers};
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(crate::app_settings::ModifierSide::Either),
                shift: Some(crate::app_settings::ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: Some(HotkeyKey::named("f8")),
        };
        let mut settings = AppSettings {
            model_switch_other: Some(other(TranscriptionModelId::ParakeetV3, "es")),
            model_switch_hotkey: Some(binding.clone()),
            ..AppSettings::default()
        };
        assert!(!settings.labs.model_switch_shortcut);
        assert_eq!(settings.runtime_hotkeys().model_switch, None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &binding));

        settings.labs.model_switch_shortcut = true;
        assert_eq!(
            settings.runtime_hotkeys().model_switch,
            Some(binding.runtime())
        );
        assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &binding));

        // A shortcut without a key never runs: every OS matches a key press.
        let keyless = HotkeyBinding {
            modifiers: HotkeyModifiers {
                command: Some(crate::app_settings::ModifierSide::Either),
                ..HotkeyModifiers::default()
            },
            key: None,
        };
        settings.model_switch_hotkey = Some(keyless.clone());
        assert_eq!(settings.runtime_hotkeys().model_switch, None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &keyless));

        // Another active shortcut's keys are refused, and a saved overlap is
        // dropped rather than matched twice.
        let dictation = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::ModelSwitch, &dictation));
        settings.model_switch_hotkey = Some(dictation);
        assert_eq!(settings.runtime_hotkeys().model_switch, None);

        settings.model_switch_hotkey = Some(binding);
        settings.model_switch_other = None;
        assert_eq!(settings.runtime_hotkeys().model_switch, None);
    }
}
