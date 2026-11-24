//! Rule scoping for the post-send `Always…` configurator and the Auto-send
//! rules sheet: deciding what a capture can match (a website subpath or whole
//! site, a terminal's working folder, or a non-general app) without ever
//! hijacking shared hosts or general-purpose apps.

use std::path::Path;

use scrawl_capture::RuleKind;
use scrawl_capture::routing::{self, policy};

/// Why an automatic send is going out (shown beside "Sending to …").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AutoSendBy {
    /// A saved site, terminal, or app rule matched.
    #[default]
    Rule,
    /// Jev was confident enough to auto-send.
    Jev,
}

impl AutoSendBy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rule => "Rule",
            Self::Jev => "Jev",
        }
    }
}

/// What a rule saved from a capture can match ("When I annotate").
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuleMatch {
    /// A browser page: optional subpath (e.g. `demo.example.com/docs/` or
    /// `github.com/org/repo`) and whole-site host (`demo.example.com`).
    Site {
        /// Rule key and display text for the subpath when the URL has one.
        subpath: Option<String>,
        /// Rule key and display text for the whole site (host, plus port when
        /// non-default).
        host: String,
        /// Whether the subpath option is selected by default (true on shared
        /// hosts like `github.com` and whenever a subpath exists).
        default_subpath: bool,
    },
    /// A terminal working in `folder` (never home or `/`, and never the
    /// terminal app itself, since terminals are general-purpose apps).
    Terminal {
        /// Full path saved to `routing.terminals`.
        folder: String,
        /// Short display form (`~/code/acme-web`).
        display: String,
    },
    /// A non-general-purpose application (e.g. Hex, Xcode, Figma).
    App {
        /// Bundle ID saved to `routing.apps`.
        bundle_id: String,
        /// Human-readable app name.
        name: String,
    },
}

impl RuleMatch {
    /// The `(RuleKind, key)` saved when `use_subpath` chooses subpath vs whole
    /// site for websites.
    pub fn rule_key(&self, use_subpath: bool) -> (RuleKind, String) {
        match self {
            Self::Site {
                subpath: Some(subpath),
                ..
            } if use_subpath => (RuleKind::Site, subpath.clone()),
            Self::Site { host, .. } => (RuleKind::Site, host.clone()),
            Self::Terminal { folder, .. } => (RuleKind::Terminal, folder.clone()),
            Self::App { bundle_id, .. } => (RuleKind::App, bundle_id.clone()),
        }
    }
}

/// Shared code-hosting forges where the first two path segments identify one
/// repository (`github.com/org/repo`).
const TWO_SEGMENT_SHARED_HOSTS: &[&str] = &[
    "github.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    "sr.ht",
];

/// Other multi-tenant web apps where a bare host would hijack unrelated work.
const ONE_SEGMENT_SHARED_HOSTS: &[&str] = &[
    "linear.app",
    "notion.so",
    "notion.site",
    "figma.com",
    "docs.google.com",
    "drive.google.com",
    "app.slack.com",
    "discord.com",
    "vercel.com",
    "dash.cloudflare.com",
    "x.com",
    "twitter.com",
    "reddit.com",
];

/// Strips a leading `http://` or `https://` for display in rules UI.
pub fn display_site(prefix: &str) -> String {
    let trimmed = prefix.trim();
    trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed)
        .to_owned()
}

/// Whether a site rule key targets a subpath rather than a whole host.
pub fn site_is_subpath(prefix: &str) -> bool {
    let bare = display_site(prefix);
    let bare = bare.split(['?', '#']).next().unwrap_or(&bare);
    bare.split_once('/')
        .is_some_and(|(_, rest)| !rest.trim_matches('/').is_empty())
}

/// Derives what an `Always…` rule can match from a capture's observed URL,
/// terminal folder, and app identity.
pub fn rule_match(
    url: Option<&str>,
    terminal: Option<&Path>,
    bundle_id: Option<&str>,
    app_name: Option<&str>,
) -> Option<RuleMatch> {
    if let Some(url) = url
        && let Some(parsed) = routing::parse_url(url)
    {
        if parsed.is_loopback() {
            return None;
        }
        return Some(site_scope(url, &parsed));
    }
    if let Some(folder) = terminal.filter(|folder| policy::is_terminal_folder(folder)) {
        let full = folder.display().to_string();
        return Some(RuleMatch::Terminal {
            display: shorten_home(folder),
            folder: full,
        });
    }
    let id = bundle_id.map(str::trim).filter(|id| !id.is_empty())?;
    if policy::is_general_app(id) {
        return None;
    }
    let name = app_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(id)
        .to_owned();
    Some(RuleMatch::App {
        bundle_id: id.to_owned(),
        name,
    })
}

/// Computes the host and optional subpath rule keys for a non-loopback URL.
fn site_scope(url: &str, parsed: &routing::WebUrl) -> RuleMatch {
    let default_port = if parsed.scheme == "https" { 443 } else { 80 };
    let host_only = if parsed.host.contains(':') {
        format!("[{}]", parsed.host)
    } else {
        parsed.host.clone()
    };
    let host = if parsed.port == default_port {
        host_only
    } else {
        format!("{host_only}:{}", parsed.port)
    };
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let path = rest.find('/').map_or("", |at| &rest[at..]);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let host_lower = parsed.host.to_ascii_lowercase();
    let subpath = if segments.is_empty() {
        None
    } else if TWO_SEGMENT_SHARED_HOSTS.contains(&host_lower.as_str()) {
        if segments.len() >= 2 {
            Some(format!("{host}/{}/{}", segments[0], segments[1]))
        } else {
            Some(format!("{host}/{}/", segments[0]))
        }
    } else if ONE_SEGMENT_SHARED_HOSTS.contains(&host_lower.as_str()) {
        if segments.len() >= 2
            && matches!(
                segments[0],
                "design" | "file" | "board" | "document" | "d" | "channels"
            )
        {
            Some(format!("{host}/{}/{}", segments[0], segments[1]))
        } else {
            Some(format!("{host}/{}/", segments[0]))
        }
    } else {
        Some(format!("{host}/{}/", segments[0]))
    };
    let default_subpath = subpath.is_some();
    RuleMatch::Site {
        subpath,
        host,
        default_subpath,
    }
}

/// Replaces the user's home directory prefix with `~` for compact display.
pub fn shorten_home(path: &Path) -> String {
    if let Some(home) = crate::app_paths::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        if rest.as_os_str().is_empty() {
            return "~".to_owned();
        }
        return format!("~/{}", rest.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn shared_host_github_defaults_to_org_repo_subpath() {
        let m = rule_match(
            Some("https://github.com/acme/web-app/pull/42?tab=files"),
            None,
            Some("com.brave.Browser"),
            Some("Brave Browser"),
        );
        assert_eq!(
            m,
            Some(RuleMatch::Site {
                subpath: Some("github.com/acme/web-app".into()),
                host: "github.com".into(),
                default_subpath: true,
            })
        );
        let m = m.unwrap();
        assert_eq!(
            m.rule_key(true),
            (RuleKind::Site, "github.com/acme/web-app".into())
        );
        assert_eq!(m.rule_key(false), (RuleKind::Site, "github.com".into()));
    }

    #[test]
    fn regular_site_offers_subpath_and_whole_site() {
        let m = rule_match(
            Some("https://demo.example.com/docs/getting-started#top"),
            None,
            Some("com.apple.Safari"),
            Some("Safari"),
        );
        assert_eq!(
            m,
            Some(RuleMatch::Site {
                subpath: Some("demo.example.com/docs/".into()),
                host: "demo.example.com".into(),
                default_subpath: true,
            })
        );
        let root = rule_match(Some("https://status.example.com/"), None, None, None);
        assert_eq!(
            root,
            Some(RuleMatch::Site {
                subpath: None,
                host: "status.example.com".into(),
                default_subpath: false,
            })
        );
    }

    #[test]
    fn terminal_offers_working_folder_and_never_general_terminal_app() {
        let dir = PathBuf::from("/tmp/acme-web");
        let m = rule_match(
            None,
            Some(&dir),
            Some("com.mitchellh.ghostty"),
            Some("Ghostty"),
        );
        assert_eq!(
            m,
            Some(RuleMatch::Terminal {
                folder: "/tmp/acme-web".into(),
                display: "/tmp/acme-web".into(),
            })
        );
        // Ghostty at home or root yields no rule at all (never "any Ghostty window").
        let root_term = rule_match(
            None,
            Some(Path::new("/")),
            Some("com.mitchellh.ghostty"),
            Some("Ghostty"),
        );
        assert_eq!(root_term, None);
    }

    #[test]
    fn non_general_app_offers_app_while_general_app_without_context_offers_none() {
        let app = rule_match(None, None, Some("anomaly.hex"), Some("Hex"));
        assert_eq!(
            app,
            Some(RuleMatch::App {
                bundle_id: "anomaly.hex".into(),
                name: "Hex".into(),
            })
        );
        assert_eq!(
            rule_match(None, None, Some("com.apple.finder"), Some("Finder")),
            None
        );
        assert_eq!(
            rule_match(None, None, Some("com.tinyspeck.slackmacgap"), Some("Slack")),
            None
        );
    }

    #[test]
    fn site_display_and_subpath_detection_handle_schemes_and_paths() {
        assert_eq!(
            display_site("https://demo.example.com/docs/"),
            "demo.example.com/docs/"
        );
        assert!(site_is_subpath("https://demo.example.com/docs/"));
        assert!(site_is_subpath("github.com/acme/web"));
        assert!(!site_is_subpath("https://status.example.com"));
        assert!(!site_is_subpath("https://status.example.com/"));
    }
}
