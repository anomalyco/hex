//! The Windows and Linux listeners' snapshot of the shared processing pipeline.

use std::sync::atomic::AtomicBool;

use url::Url;

use crate::app_settings::{AppSettings, DictationProcessingSettings};
use crate::dictation::processor::{Processed, Profiles};

/// Live mode-processing snapshot. The output worker clones it before any
/// deadline-bounded OpenCode call, so settings writes never wait on
/// inference.
#[derive(Clone)]
pub struct ModeRuntime {
    profiles: Profiles,
    reads_page: bool,
}

impl ModeRuntime {
    pub fn from_settings(settings: &AppSettings) -> Self {
        let processing = &settings.dictation_processing;
        Self {
            profiles: Profiles::from_settings(processing),
            // Labs › Voice prompt carries the browser host in its request,
            // so its mode runtime reads the page too.
            reads_page: uses_page(processing) || settings.labs.voice_prompt,
        }
    }

    /// Whether processing looks at the focused page: a mode chosen by
    /// website, an OpenCode rewrite, or a script transformation. Otherwise
    /// the listener never asks the browser for its address, as on macOS.
    pub fn reads_page(&self) -> bool {
        self.reads_page
    }

    /// Speech preparation, Global corrections, mode corrections, rewrite,
    /// and transformations, through the same profiles macOS uses.
    pub fn process(
        &self,
        raw_text: &str,
        language: &str,
        context: &crate::dictation::context::ContextSnapshot,
        transformations: &crate::personal_commands::TransformationClient,
        source: crate::dictation::processor::Source,
    ) -> Processed {
        self.process_cancellable(
            raw_text,
            language,
            context,
            transformations,
            &AtomicBool::new(false),
            source,
        )
    }

    /// [`Self::process`] with a cancellation flag the stages check, for work
    /// that ends early when Hex quits.
    pub fn process_cancellable(
        &self,
        raw_text: &str,
        language: &str,
        context: &crate::dictation::context::ContextSnapshot,
        transformations: &crate::personal_commands::TransformationClient,
        cancelled: &AtomicBool,
        source: crate::dictation::processor::Source,
    ) -> Processed {
        let mut processed = self
            .profiles
            .process_cancellable(raw_text, language, context, cancelled, source);
        processed.apply_transformations(transformations, context, cancelled, source);
        processed
    }
}

/// The live mode runtime and transformation client a listener shares with
/// the local API's `process=global` processor, so saved settings and the
/// personal-commands host apply to both.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Clone)]
pub struct ProcessingState {
    pub modes: std::sync::Arc<std::sync::RwLock<ModeRuntime>>,
    pub transformations: std::sync::Arc<crate::personal_commands::TransformationClient>,
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
impl ProcessingState {
    /// Reads `settings` once; a listener replaces `modes` as settings change.
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            modes: std::sync::Arc::new(std::sync::RwLock::new(ModeRuntime::from_settings(
                settings,
            ))),
            transformations: std::sync::Arc::new(
                crate::personal_commands::TransformationClient::default(),
            ),
        }
    }
}

/// Whether anything set up in `settings` looks at the focused page: a mode
/// chosen by website, an OpenCode rewrite (its prompt names the site), or a
/// script transformation (it receives the page address). Every OS asks the
/// browser for its page only for one of these or Labs › Voice prompt (macOS
/// through `config::reads_browser`).
pub(crate) fn uses_page(settings: &DictationProcessingSettings) -> bool {
    settings
        .modes
        .iter()
        .any(|mode| !mode.browser_hosts.is_empty())
        || std::iter::once(&settings.default_mode)
            .chain(&settings.modes)
            .any(|mode| {
                mode.post_processing.enabled
                    || mode
                        .transformations
                        .iter()
                        .any(|id| !crate::personal_commands::is_builtin_transformation(id))
            })
}

/// The history entry for one pasted dictation.
pub fn history_draft(
    raw_text: String,
    processed: &Processed,
    application: Option<String>,
    audio_ms: u64,
    inference_ms: u64,
    total_ms: u64,
    recording: Option<crate::history::Recording>,
) -> crate::history::HistoryDraft {
    crate::history::HistoryDraft {
        kind: crate::history::HistoryKind::Dictation,
        raw_text,
        final_text: processed.text.clone(),
        application,
        processing: processed.observation.as_ref().map(|processing| {
            crate::history::HistoryProcessing {
                profile: processing.profile.clone(),
                latency_ms: processing.latency_ms,
                fallback: processing.fallback.clone(),
            }
        }),
        audio_ms,
        inference_ms,
        total_ms,
        recording,
        model: crate::history::dictation_model(),
    }
}

/// The speech model and language the mode matching the focused context
/// carries, else Global's. `None` means the mode never switches.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn speech_for(
    settings: &DictationProcessingSettings,
    application: Option<&str>,
    browser_host: Option<&str>,
) -> Option<crate::app_settings::KeyboardModelChoice> {
    Profiles::from_settings(settings).speech_for(&crate::dictation::context::ContextSnapshot {
        application: application.map(str::to_owned),
        browser_url: browser_host.and_then(|host| Url::parse(&format!("https://{host}")).ok()),
        ..Default::default()
    })
}

/// Hosts compare case-insensitively with any trailing dot removed,
/// matching the macOS browser-host semantics.
pub fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_lowercase()
}

/// A website rule entry reduced to its comparable host: bare hosts,
/// pasted URLs, host:port forms, and IDN text all work, matching the
/// macOS Websites field.
pub fn canonical_host(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }
    let parsed = Url::parse(entry)
        .ok()
        .filter(|url| url.host_str().is_some())
        .or_else(|| Url::parse(&format!("https://{entry}")).ok())?;
    parsed.host_str().map(normalize_host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::DictationMode;

    fn mode(name: &str, applications: &[&str], browser_hosts: &[&str]) -> DictationMode {
        DictationMode {
            name: name.into(),
            applications: applications.iter().map(|value| value.to_string()).collect(),
            browser_hosts: browser_hosts
                .iter()
                .map(|value| value.to_string())
                .collect(),
            ..DictationMode::default()
        }
    }

    fn matched(
        modes: &[DictationMode],
        application: Option<&str>,
        host: Option<&str>,
    ) -> Option<String> {
        let settings = DictationProcessingSettings {
            modes: modes
                .iter()
                .cloned()
                .map(|mut mode| {
                    mode.speech = Some(crate::app_settings::KeyboardModelChoice {
                        language: mode.name.clone(),
                        model: crate::transcription::models::TranscriptionModelId::ParakeetV3,
                    });
                    mode
                })
                .collect(),
            ..Default::default()
        };
        speech_for(&settings, application, host).map(|choice| choice.language)
    }

    fn runtime(settings: impl FnOnce(&mut AppSettings)) -> ModeRuntime {
        let mut value = AppSettings::default();
        settings(&mut value);
        ModeRuntime::from_settings(&value)
    }

    #[test]
    fn modes_match_case_insensitive_application_substrings() {
        let modes = [
            mode("Code", &["Code", "rustrover"], &[]),
            mode("Chat", &["slack"], &[]),
        ];
        assert_eq!(matched(&modes, Some("code"), None).as_deref(), Some("Code"));
        assert_eq!(
            matched(&modes, Some("slack"), None).as_deref(),
            Some("Chat")
        );
        assert_eq!(
            matched(&modes, Some("vscode"), None).as_deref(),
            if cfg!(target_os = "macos") {
                None
            } else {
                Some("Code")
            }
        );
        assert_eq!(matched(&modes, Some("notepad"), None), None);
        assert_eq!(matched(&modes, None, None), None);
    }

    #[test]
    fn website_rules_match_exact_hosts_and_beat_application_rules() {
        let modes = [
            mode("Browser", &["chrome"], &[]),
            mode("X", &[], &["X.com"]),
        ];
        assert_eq!(
            matched(&modes, Some("chrome"), Some("x.com.")).as_deref(),
            Some("X")
        );
        assert_eq!(
            matched(&modes, Some("chrome"), Some("news.x.com")).as_deref(),
            Some("Browser")
        );
        assert_eq!(
            matched(&modes, Some("chrome"), None).as_deref(),
            Some("Browser")
        );
    }

    #[test]
    fn the_page_is_read_only_when_processing_uses_it() {
        assert!(!runtime(|_| {}).reads_page());
        assert!(
            !runtime(|settings| {
                settings.dictation_processing.modes = vec![DictationMode {
                    transformations: vec!["lowercase".into()],
                    ..mode("Code", &["code"], &[])
                }];
            })
            .reads_page()
        );
        assert!(
            runtime(|settings| {
                settings.dictation_processing.modes = vec![mode("X", &[], &["x.com"])];
            })
            .reads_page()
        );
        assert!(
            runtime(|settings| {
                settings
                    .dictation_processing
                    .default_mode
                    .post_processing
                    .enabled = true;
            })
            .reads_page()
        );
        // A voice prompt's request carries the browser host, so its lab
        // reads the page even with no rewrite or website rule.
        assert!(
            runtime(|settings| {
                settings.labs.voice_prompt = true;
            })
            .reads_page()
        );
    }

    #[test]
    fn blank_rules_never_match() {
        let modes = [mode("Broken", &["  ", ""], &[" "])];
        assert_eq!(matched(&modes, Some("anything"), Some("x.com")), None);
    }

    #[test]
    fn global_replacements_run_before_mode_corrections() {
        let runtime = runtime(|settings| {
            let processing = &mut settings.dictation_processing;
            processing.default_mode.replacements =
                vec![crate::dictation::text_replacements::TextReplacement {
                    matched_phrase: "hex".into(),
                    output: "Hex".into(),
                }];
            processing.modes = vec![DictationMode {
                replacements: vec![crate::dictation::text_replacements::TextReplacement {
                    matched_phrase: "open code".into(),
                    output: "OpenCode".into(),
                }],
                ..mode("Code", &["code"], &[])
            }];
        });
        let context = crate::dictation::context::ContextSnapshot {
            application: Some("code".into()),
            ..crate::dictation::context::ContextSnapshot::default()
        };
        let transformations = crate::personal_commands::TransformationClient::default();

        let processed = runtime.process(
            "Use open code in hex.",
            "en",
            &context,
            &transformations,
            crate::dictation::processor::Source::Dictation,
        );

        assert_eq!(processed.text, "Use OpenCode in Hex.");
        assert!(processed.observation.is_none());
    }

    #[test]
    fn modes_execute_selected_transformations_after_corrections() {
        let runtime = runtime(|settings| {
            settings.dictation_processing.default_mode.transformations = vec!["lowercase".into()];
        });
        let transformations = crate::personal_commands::TransformationClient::default();

        let processed = runtime.process(
            "Keep OpenCode Exact.",
            "en",
            &crate::dictation::context::ContextSnapshot::default(),
            &transformations,
            crate::dictation::processor::Source::Dictation,
        );

        assert_eq!(processed.text, "keep opencode exact.");
        let observation = processed.observation.as_ref().expect("transformation ran");
        assert_eq!(observation.profile, "Custom transformations");
        assert_eq!(observation.fallback, None);
    }

    #[test]
    fn saved_post_processing_enables_the_global_rewrite() {
        let path = std::env::temp_dir().join(format!("hex-970-{}.json", std::process::id()));
        std::fs::write(
            &path,
            br#"{"dictation_processing":{"default_mode":{"post_processing":{"enabled":true,"prompt":"Fix it.","model":"opencode/big-pickle","deadline_seconds":30}}}}"#,
        )
        .unwrap();
        let (settings, _) = AppSettings::read_from(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let runtime = ModeRuntime::from_settings(&settings);
        let context = crate::dictation::context::ContextSnapshot {
            application: Some("kitty".into()),
            ..crate::dictation::context::ContextSnapshot::default()
        };
        assert!(runtime.profiles.processes(&context));
    }

    #[test]
    fn a_modes_speech_choice_matches_its_application_or_website_else_global() {
        use crate::app_settings::KeyboardModelChoice;
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
                    applications: vec!["code".into()],
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
        let matched = |application: Option<&str>, host: Option<&str>| {
            speech_for(&settings, application, host).map(|choice| choice.language)
        };

        assert_eq!(matched(Some("code"), None).as_deref(), Some("de"));
        // A website is more specific than an application.
        assert_eq!(
            matched(Some("chrome"), Some("x.com")).as_deref(),
            Some("es")
        );
        // Nothing matches, so Global's own choice applies.
        assert_eq!(matched(Some("notepad"), None).as_deref(), Some("en"));
        assert_eq!(matched(None, None).as_deref(), Some("en"));
    }

    #[test]
    fn website_entries_reduce_to_their_host() {
        assert_eq!(normalize_host("X.com."), "x.com");
        assert_eq!(
            canonical_host("https://x.com/home").as_deref(),
            Some("x.com")
        );
        assert_eq!(
            canonical_host("github.com/hex/pull/1").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            canonical_host("localhost:3000").as_deref(),
            Some("localhost")
        );
        assert_eq!(canonical_host("   "), None);
    }
}
