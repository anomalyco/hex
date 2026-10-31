//! Dictation modes for the Windows and Linux listeners. The focused
//! application or website selects a mode, and its replacements,
//! post-processing, and transformations run in the shared
//! `dictation_processor` order. macOS selects modes through `config`.

use std::sync::atomic::AtomicBool;

use url::Url;

use crate::app_settings::{AppSettings, DictationMode};
use crate::dictation::processor::{PostProcessingSettings, Processed, Profile, Profiles};
use crate::dictation::text_replacements::ReplacementSet;

/// Live mode-processing snapshot. The output worker clones it before any
/// deadline-bounded OpenCode call, so settings writes never wait on
/// inference.
#[derive(Clone)]
pub struct ModeRuntime {
    replacements: ReplacementSet,
    default: PostProcessingSettings,
    default_transformations: Vec<String>,
    modes: Vec<DictationMode>,
}

impl ModeRuntime {
    pub fn from_settings(settings: &AppSettings) -> Self {
        let processing = &settings.dictation_processing;
        Self {
            replacements: ReplacementSet::new(&processing.default_mode.replacements),
            default: processing.default_mode.post_processing.clone(),
            default_transformations: processing.default_mode.transformations.clone(),
            modes: processing.modes.clone(),
        }
    }

    /// Global replacements, then the matching mode's corrections,
    /// post-processing, and transformations.
    pub fn process(
        &self,
        raw_text: &str,
        context: &crate::dictation::context::ContextSnapshot,
        transformations: &crate::personal_commands::TransformationClient,
    ) -> Processed {
        let text = self.replacements.replace(raw_text);
        let mode = mode_for_context(
            &self.modes,
            context.application.as_deref(),
            context.browser_host(),
        );
        let profile = mode.map_or_else(
            || {
                Profile::configured(
                    "Global",
                    ReplacementSet::default(),
                    self.default_transformations.clone(),
                    &self.default,
                )
            },
            Profile::from_mode,
        );
        let cancelled = AtomicBool::new(false);
        let mut processed = Profiles::new(profile).process_cancellable(&text, context, &cancelled);
        processed.apply_transformations(transformations, context, &cancelled);
        processed
    }
}

/// The history entry for one pasted dictation.
pub fn history_draft(
    raw_text: String,
    processed: &Processed,
    application: Option<String>,
    audio_ms: u64,
    inference_ms: u64,
    total_ms: u64,
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
    }
}

/// Whether a mode's applications match the focused application. Windows
/// and Linux name applications by executable, and a rule matches any
/// case-insensitive substring of that name ("code" matches "vscode").
fn matches_application(mode: &DictationMode, application: &str) -> bool {
    let application = application.to_lowercase();
    mode.applications.iter().any(|candidate| {
        let candidate = candidate.trim().to_lowercase();
        !candidate.is_empty() && application.contains(&candidate)
    })
}

/// Whether a mode's websites match the focused page's exact host.
fn matches_website(mode: &DictationMode, host: &str) -> bool {
    let host = normalize_host(host);
    mode.browser_hosts.iter().any(|candidate| {
        // Entries are stored as typed; pasted URLs, ports, and IDN text
        // reduce to their host here.
        canonical_host(candidate).is_some_and(|candidate| candidate == host)
    })
}

/// The first mode matching the focused context. A website rule is more
/// specific than an application rule, so browser-host matches win, like
/// the macOS selector ordering.
fn mode_for_context<'a>(
    modes: &'a [DictationMode],
    application: Option<&str>,
    browser_host: Option<&str>,
) -> Option<&'a DictationMode> {
    if let Some(host) = browser_host
        && let Some(mode) = modes.iter().find(|mode| matches_website(mode, host))
    {
        return Some(mode);
    }
    let application = application?;
    modes
        .iter()
        .find(|mode| matches_application(mode, application))
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

    fn matched<'a>(
        modes: &'a [DictationMode],
        application: Option<&str>,
        host: Option<&str>,
    ) -> Option<&'a str> {
        mode_for_context(modes, application, host).map(|mode| mode.name.as_str())
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
        assert_eq!(matched(&modes, Some("code"), None), Some("Code"));
        assert_eq!(matched(&modes, Some("slack"), None), Some("Chat"));
        assert_eq!(matched(&modes, Some("vscode"), None), Some("Code"));
        assert_eq!(matched(&modes, Some("notepad"), None), None);
        assert_eq!(matched(&modes, None, None), None);
    }

    #[test]
    fn website_rules_match_exact_hosts_and_beat_application_rules() {
        let modes = [
            mode("Browser", &["chrome"], &[]),
            mode("X", &[], &["X.com"]),
        ];
        assert_eq!(matched(&modes, Some("chrome"), Some("x.com.")), Some("X"));
        assert_eq!(
            matched(&modes, Some("chrome"), Some("news.x.com")),
            Some("Browser")
        );
        assert_eq!(matched(&modes, Some("chrome"), None), Some("Browser"));
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
            application: Some("Visual Studio Code".into()),
            ..crate::dictation::context::ContextSnapshot::default()
        };
        let transformations = crate::personal_commands::TransformationClient::default();

        let processed = runtime.process("Use open code in hex.", &context, &transformations);

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
            &crate::dictation::context::ContextSnapshot::default(),
            &transformations,
        );

        assert_eq!(processed.text, "keep opencode exact.");
        let observation = processed.observation.as_ref().expect("transformation ran");
        assert_eq!(observation.profile, "Custom transformations");
        assert_eq!(observation.fallback, None);
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
