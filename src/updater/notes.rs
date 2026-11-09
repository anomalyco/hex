//! What a staged dev build changed: the dev channel's notes
//! (`dev-notes.json`, published by the Release workflow's dev-notes job) from
//! the offered build back to the running one, shown with Restart to update.
//! What's new opens the changelog at those builds (`?since=<running>`).
//! Notes are a nicety: any failure leaves the update without them.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::transport::ManifestCache;
use crate::desktop::host::UpdateNotes;

const URL: &str = "https://anoma.ly/hex/files/dev-notes.json";
const PAGE: &str = "https://anoma.ly/hex/dev";
/// A stable release's notes, as the release workflow links them (`plan.sh`).
const RELEASES: &str = "https://github.com/anomalyco/hex/releases/tag";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ITEMS: usize = 40;
const MAX_TITLE: usize = 200;

/// Fetches the notes for `offered`, the staged build, since `running`.
pub fn fetch(
    user_agent: &str,
    scratch: &Path,
    running: &str,
    offered: &str,
) -> Option<UpdateNotes> {
    std::fs::create_dir_all(scratch).ok()?;
    let text = ManifestCache::default()
        .fetch(URL, user_agent, scratch, TIMEOUT)
        .inspect_err(|error| tracing::info!(%error, "no dev notes for the update"))
        .ok()?;
    parse(&text, running, offered)
}

fn version(build: &Value) -> Option<&str> {
    build.get("version").and_then(Value::as_str)
}

fn is_dev_version(version: &str) -> bool {
    version.strip_prefix("0.0.0-dev-").is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// A plain `major.minor.patch`, with no suffix.
fn is_stable_version(version: &str) -> bool {
    fn digits(part: Option<&str>) -> bool {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }
    let mut parts = version.split('.');
    digits(parts.next()) && digits(parts.next()) && digits(parts.next()) && parts.next().is_none()
}

/// Where the release notes for a running version are: the dev changelog for a
/// dev version, GitHub's release for a stable one, and nothing for an odd one.
/// The sidebar asks only on a release build, so a local or test build with a
/// plain version still shows no link.
pub fn release_notes_url(running: &str) -> Option<String> {
    if is_dev_version(running) {
        Some(format!("{PAGE}#{running}"))
    } else if is_stable_version(running) {
        Some(format!("{RELEASES}/v{running}"))
    } else {
        None
    }
}

/// The changes from `offered` (newest first) back to, not including,
/// `running`; every build's when `running` is older than the notes go.
pub fn parse(text: &str, running: &str, offered: &str) -> Option<UpdateNotes> {
    if !is_dev_version(offered) {
        return None;
    }
    let value: Value = serde_json::from_str(text).ok()?;
    let builds = value.get("builds")?.as_array()?;
    let start = builds
        .iter()
        .position(|build| version(build) == Some(offered))?;
    let mut items: Vec<String> = Vec::new();
    for build in &builds[start..] {
        if version(build) == Some(running) {
            break;
        }
        let titles = build
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("title").and_then(Value::as_str));
        for title in titles {
            let title: String = title
                .chars()
                .filter(|character| !character.is_control())
                .take(MAX_TITLE)
                .collect();
            if !title.is_empty() && !items.contains(&title) && items.len() < MAX_ITEMS {
                items.push(title);
            }
        }
    }
    Some(UpdateNotes {
        items,
        url: if is_dev_version(running) {
            format!("{PAGE}?since={running}#{offered}")
        } else {
            format!("{PAGE}#{offered}")
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str = r#"{"schema": 1, "builds": [
      {"version": "0.0.0-dev-12", "commit": "c", "published_at": "t", "items": [{"title": "Newer", "url": "u"}]},
      {"version": "0.0.0-dev-11", "commit": "c", "published_at": "t", "items": [{"title": "Fix A", "url": "u"}, {"title": "Fix B", "url": "u"}]},
      {"version": "0.0.0-dev-10", "commit": "c", "published_at": "t", "items": []},
      {"version": "0.0.0-dev-9", "commit": "c", "published_at": "t", "items": [{"title": "Fix A", "url": "u"}, {"title": "Fix C\u0007", "url": "u"}]},
      {"version": "0.0.0-dev-8", "commit": "c", "published_at": "t", "items": [{"title": "Already running", "url": "u"}]}
    ]}"#;

    #[test]
    fn lists_the_changes_from_the_offered_build_back_to_the_running_one() {
        let notes = parse(NOTES, "0.0.0-dev-8", "0.0.0-dev-11").unwrap();
        assert_eq!(notes.items, ["Fix A", "Fix B", "Fix C"]);
        assert_eq!(
            notes.url,
            "https://anoma.ly/hex/dev?since=0.0.0-dev-8#0.0.0-dev-11"
        );
    }

    #[test]
    fn an_install_older_than_the_notes_gets_every_listed_change() {
        let notes = parse(NOTES, "0.0.0-dev-2", "0.0.0-dev-9").unwrap();
        assert_eq!(notes.items, ["Fix A", "Fix C", "Already running"]);
        let from_stable = parse(NOTES, "3.1.1", "0.0.0-dev-9").unwrap();
        assert_eq!(from_stable.url, "https://anoma.ly/hex/dev#0.0.0-dev-9");
    }

    #[test]
    fn no_notes_without_the_offered_build_or_for_stable() {
        assert!(parse(NOTES, "0.0.0-dev-8", "0.0.0-dev-99").is_none());
        assert!(parse(NOTES, "3.1.0", "3.1.1").is_none());
        assert!(parse("not json", "0.0.0-dev-8", "0.0.0-dev-11").is_none());
    }

    #[test]
    fn a_dev_build_opens_the_changelog_at_itself() {
        assert_eq!(
            release_notes_url("0.0.0-dev-12").as_deref(),
            Some("https://anoma.ly/hex/dev#0.0.0-dev-12")
        );
    }

    #[test]
    fn a_stable_build_opens_its_github_release() {
        assert_eq!(
            release_notes_url("3.3.0").as_deref(),
            Some("https://github.com/anomalyco/hex/releases/tag/v3.3.0")
        );
    }

    #[test]
    fn an_odd_version_has_no_release_notes() {
        for version in [
            "0.0.0-dev-local-abc1234",
            "0.0.0-local-abc1234-dirty",
            "3.3",
            "3.3.0.1",
            "3.3.0-beta.1",
            "3.3.0-dev-4",
            "v3.3.0",
            "",
        ] {
            assert_eq!(release_notes_url(version), None, "{version}");
        }
    }
}
