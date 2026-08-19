//! Platform-neutral command context: which application or browser page
//! the user is focused on, and the selectors commands scope themselves
//! with. macOS keeps its richer capture pipeline; the port shells fill
//! this from their own foreground readers, and an empty snapshot matches
//! only `Always`-scoped commands.
//!
//! This is a shared vocabulary: only the shells with voice Commands wired
//! up (Linux today) construct selectors, so per-platform dead-code
//! analysis stays quiet here.
#![allow(dead_code)]

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextSnapshot {
    pub application: Option<String>,
    pub browser_host: Option<String>,
}

impl ContextSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn browser_host(&self) -> Option<&str> {
        self.browser_host.as_deref()
    }

    pub fn browser_host_is(&self, host: &str) -> bool {
        self.browser_host()
            .is_some_and(|current| browser_hosts_equal(current, host))
    }

    pub fn application_is(&self, application: &str) -> bool {
        self.application
            .as_deref()
            .is_some_and(|current| application_names_equal(current, application))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextSelector {
    Always,
    BrowserHost(String),
    Application(String),
}

impl ContextSelector {
    pub fn application(application: impl Into<String>) -> Self {
        Self::Application(application.into())
    }

    pub fn browser_host(host: impl Into<String>) -> Self {
        Self::BrowserHost(normalize_browser_host(&host.into()))
    }

    pub fn matches(&self, context: &ContextSnapshot) -> bool {
        match self {
            Self::Always => true,
            Self::BrowserHost(host) => context.browser_host_is(host),
            Self::Application(application) => context.application_is(application),
        }
    }

    pub fn specificity(&self) -> u8 {
        match self {
            Self::Always => 0,
            Self::Application(_) => 1,
            Self::BrowserHost(_) => 2,
        }
    }

    pub fn can_coexist_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Always, _) | (_, Self::Always) => true,
            (Self::BrowserHost(left), Self::BrowserHost(right)) => browser_hosts_equal(left, right),
            (Self::Application(left), Self::Application(right)) => {
                application_names_equal(left, right)
            }
            _ => true,
        }
    }
}

fn normalize_browser_host(host: &str) -> String {
    host.trim_end_matches('.').to_lowercase()
}

fn application_names_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn browser_hosts_equal(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_match_their_scope_and_nothing_narrower() {
        let context = ContextSnapshot {
            application: Some("firefox".into()),
            browser_host: Some("x.com".into()),
        };
        assert!(ContextSelector::Always.matches(&context));
        assert!(ContextSelector::application("Firefox").matches(&context));
        assert!(ContextSelector::browser_host("X.com.").matches(&context));
        assert!(!ContextSelector::application("code").matches(&context));
        assert!(!ContextSelector::browser_host("github.com").matches(&context));
        assert!(!ContextSelector::browser_host("x.com").matches(&ContextSnapshot::empty()));
        assert!(ContextSelector::Always.matches(&ContextSnapshot::empty()));
    }

    #[test]
    fn coexistence_matches_the_macos_selector_semantics() {
        // Same-scope selectors coexist (many commands share one scope);
        // browser selectors for different hosts do not pair.
        let x = ContextSelector::browser_host("x.com");
        let also_x = ContextSelector::browser_host("X.COM");
        let github = ContextSelector::browser_host("github.com");
        assert!(x.can_coexist_with(&also_x));
        assert!(!x.can_coexist_with(&github));
        assert!(x.can_coexist_with(&ContextSelector::Always));
    }
}
