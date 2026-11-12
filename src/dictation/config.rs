use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::dictation::processor::{Profile, Profiles};
use crate::dictation::text_replacements::ReplacementSet;

static DICTATION_PROFILES: OnceLock<RwLock<Profiles>> = OnceLock::new();
static READS_BROWSER: AtomicBool = AtomicBool::new(false);
static READS_WINDOW_TITLE: AtomicBool = AtomicBool::new(false);

/// Whether anything the user set up looks at the page: a mode chosen by
/// website, an OpenCode rewrite (its prompt names the site), a script
/// transformation (it receives the page address), or Labs › Voice prompt
/// (its request carries the browser host). Otherwise Hex never asks a
/// browser for its page. On macOS, any of these, the voice prompt's lab
/// included, can bring up the Automation prompt for the browser.
pub fn reads_browser() -> bool {
    READS_BROWSER.load(Ordering::Acquire)
}

/// Whether a script transformation is set up, the only reader of the front
/// window's title. Otherwise Hex never reads it.
pub fn reads_window_title() -> bool {
    READS_WINDOW_TITLE.load(Ordering::Acquire)
}

fn uses_scripts(settings: &DictationProcessingSettings) -> bool {
    std::iter::once(&settings.default_mode)
        .chain(&settings.modes)
        .flat_map(|mode| &mode.transformations)
        .any(|id| !crate::personal_commands::is_builtin_transformation(id))
}

fn uses_browser(settings: &DictationProcessingSettings, voice_prompt: bool) -> bool {
    // Labs › Voice prompt's request carries the browser host, so its lab
    // asks for the page too.
    crate::dictation::modes::uses_page(settings) || voice_prompt
}

pub fn dictation_profiles() -> Profiles {
    dictation_profiles_lock()
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The speech model and language the mode `settings` select for `context`,
/// through the same selectors processing uses: a browser host first, then an
/// exact application name, else Global. `None` means the mode never switches.
pub fn speech_for(
    settings: &DictationProcessingSettings,
    context: &crate::dictation::context::ContextSnapshot,
) -> Option<crate::app_settings::KeyboardModelChoice> {
    build_dictation_profiles(settings).speech_for(context)
}

pub fn update_dictation_profiles(settings: &DictationProcessingSettings, voice_prompt: bool) {
    let profiles = build_dictation_profiles(settings);
    READS_BROWSER.store(uses_browser(settings, voice_prompt), Ordering::Release);
    READS_WINDOW_TITLE.store(uses_scripts(settings), Ordering::Release);
    *dictation_profiles_lock()
        .write()
        .unwrap_or_else(|error| error.into_inner()) = profiles;
}

fn dictation_profiles_lock() -> &'static RwLock<Profiles> {
    DICTATION_PROFILES.get_or_init(|| {
        RwLock::new(build_dictation_profiles(
            &DictationProcessingSettings::default(),
        ))
    })
}

fn build_dictation_profiles(settings: &DictationProcessingSettings) -> Profiles {
    let global = &settings.default_mode;
    let mut profiles = Profiles::new(
        Profile::configured(
            "Global",
            ReplacementSet::default(),
            global.transformations.clone(),
            &global.post_processing,
        )
        .vocabulary(global.vocabulary.clone())
        .speech(global.speech.clone()),
    )
    .global_replacements(ReplacementSet::new(&global.replacements));
    for mode in &settings.modes {
        let profile = Profile::from_mode(mode);
        for application in &mode.applications {
            profiles = profiles.application(application.clone(), profile.clone());
        }
        for host in &mode.browser_hosts {
            profiles = profiles.browser_host(host.clone(), profile.clone());
        }
    }
    profiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::context::ContextSnapshot;

    #[test]
    fn configured_mode_profiles_match_application_context() {
        let settings = DictationProcessingSettings {
            modes: vec![crate::app_settings::DictationMode {
                applications: vec!["Slack".into()],
                transformations: vec!["lowercase".into()],
                ..crate::app_settings::DictationMode::default()
            }],
            ..DictationProcessingSettings::default()
        };
        let context = ContextSnapshot {
            application: Some("Slack".into()),
            ..ContextSnapshot::default()
        };

        assert!(build_dictation_profiles(&settings).processes(&context));
    }

    #[test]
    fn global_corrections_run_before_an_app_modes_own() {
        use crate::app_settings::DictationMode;
        use crate::dictation::text_replacements::TextReplacement;
        let pair = |phrase: &str, output: &str| TextReplacement {
            matched_phrase: phrase.into(),
            output: output.into(),
        };
        let mut settings = DictationProcessingSettings {
            modes: vec![DictationMode {
                applications: vec!["Slack".into()],
                replacements: vec![pair("OpenCode", "OpenCode :rocket:")],
                ..DictationMode::default()
            }],
            ..DictationProcessingSettings::default()
        };
        settings.default_mode.replacements = vec![pair("open code", "OpenCode")];
        let profiles = build_dictation_profiles(&settings);
        let process = |application: &str| {
            let context = ContextSnapshot {
                application: Some(application.into()),
                ..ContextSnapshot::default()
            };
            profiles
                .process_cancellable(
                    "I like open code",
                    "en",
                    &context,
                    &AtomicBool::new(false),
                    crate::dictation::processor::Source::Dictation,
                )
                .text
        };

        assert_eq!(process("Slack"), "I like OpenCode :rocket:");
        assert_eq!(process("Notes"), "I like OpenCode");
    }

    #[test]
    fn an_app_mode_has_its_own_vocabulary() {
        use crate::app_settings::DictationMode;
        use crate::dictation::vocabulary::VocabularyEntry;
        let term = |term: &str| VocabularyEntry {
            term: term.into(),
            sounds_like: Vec::new(),
        };
        let settings = DictationProcessingSettings {
            modes: vec![DictationMode {
                applications: vec!["Zed".into()],
                vocabulary: vec![term("useEffect")],
                ..DictationMode::default()
            }],
            ..DictationProcessingSettings::default()
        };
        let profiles = build_dictation_profiles(&settings);
        let in_app = |application: &str| {
            profiles.vocabulary_for(&ContextSnapshot {
                application: Some(application.into()),
                ..ContextSnapshot::default()
            })
        };

        assert_eq!(in_app("Zed"), [term("useEffect")]);
        assert!(in_app("Notes").is_empty());
    }

    #[test]
    fn a_modes_speech_choice_uses_the_same_selectors_as_its_processing() {
        use crate::app_settings::{DictationMode, KeyboardModelChoice};
        use crate::transcription::models::TranscriptionModelId;
        let choice = |language: &str| KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: language.into(),
        };
        let settings = DictationProcessingSettings {
            default_mode: DictationMode {
                speech: Some(choice("en")),
                ..DictationMode::default()
            },
            modes: vec![
                DictationMode {
                    applications: vec!["Slack".into()],
                    speech: Some(choice("de")),
                    ..DictationMode::default()
                },
                DictationMode {
                    browser_hosts: vec!["x.com".into()],
                    speech: Some(choice("es")),
                    ..DictationMode::default()
                },
            ],
        };
        let profiles = build_dictation_profiles(&settings);
        let speech = |application: &str, url: Option<&str>| {
            profiles.speech_for(&ContextSnapshot {
                application: Some(application.into()),
                browser_url: url.map(|url| url::Url::parse(url).unwrap()),
                ..ContextSnapshot::default()
            })
        };

        // An exact application name (a `.app` suffix never matters here).
        assert_eq!(speech("Slack", None).unwrap().language, "de");
        // A website is more specific than the application.
        assert_eq!(
            speech("Safari", Some("https://x.com/home"))
                .unwrap()
                .language,
            "es"
        );
        // Nothing matches, so Global's own choice applies.
        assert_eq!(speech("Notes", None).unwrap().language, "en");
        let bare = build_dictation_profiles(&DictationProcessingSettings::default());
        assert!(bare.speech_for(&ContextSnapshot::default()).is_none());
    }

    #[test]
    fn browsers_are_asked_only_when_something_uses_the_page() {
        use crate::app_settings::{DictationMode, DictationPostProcessing};
        let with = |mode: DictationMode| DictationProcessingSettings {
            modes: vec![mode],
            ..DictationProcessingSettings::default()
        };

        assert!(!uses_browser(
            &DictationProcessingSettings::default(),
            false
        ));
        assert!(uses_browser(&DictationProcessingSettings::default(), true));
        assert!(!uses_browser(
            &with(DictationMode {
                applications: vec!["Slack".into()],
                transformations: vec!["lowercase".into()],
                ..DictationMode::default()
            }),
            false
        ));
        assert!(uses_browser(
            &with(DictationMode {
                browser_hosts: vec!["x.com".into()],
                ..DictationMode::default()
            }),
            false
        ));
        let script = with(DictationMode {
            applications: vec!["Slack".into()],
            transformations: vec!["my-script".into()],
            ..DictationMode::default()
        });
        assert!(uses_browser(&script, false));
        assert!(uses_scripts(&script));
        let rewrite = with(DictationMode {
            applications: vec!["Slack".into()],
            post_processing: DictationPostProcessing {
                enabled: true,
                ..DictationPostProcessing::default()
            },
            ..DictationMode::default()
        });
        assert!(uses_browser(&rewrite, false));
        assert!(!uses_scripts(&rewrite));

        let global = |transformation: &str| DictationProcessingSettings {
            default_mode: DictationMode {
                transformations: vec![transformation.into()],
                ..DictationMode::default()
            },
            ..DictationProcessingSettings::default()
        };
        assert!(uses_scripts(&global("my-script")));
        assert!(!uses_scripts(&global("lowercase")));
    }
}
