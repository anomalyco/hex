//! The client rules of the Anomaly release protocol (`anomalyco/release`,
//! `docs/protocol.md`, schema 1) as pure functions: strict SemVer, lenient
//! manifest parsing, and the update decision. `tests/fixtures/release-protocol`
//! holds the service's shared test vectors, which these must reproduce.

use std::cmp::Ordering;

use serde_json::Value;

/// Numeric identifiers must fit a JavaScript safe integer so every client
/// compares them identically.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Identifier {
    Number(u64),
    Text(String),
}

/// Strict SemVer 2.0.0 without build metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Version {
    core: [u64; 3],
    prerelease: Vec<Identifier>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > 128 {
            return None;
        }
        let (core, prerelease) = match text.split_once('-') {
            Some((core, prerelease)) => (core, Some(prerelease)),
            None => (text, None),
        };
        let mut numbers = core.split('.');
        let mut parsed = [0; 3];
        for slot in &mut parsed {
            *slot = numeric(numbers.next()?)?;
        }
        if numbers.next().is_some() {
            return None;
        }
        let prerelease = match prerelease {
            None => Vec::new(),
            Some(text) => text
                .split('.')
                .map(|part| {
                    if !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) {
                        numeric(part).map(Identifier::Number)
                    } else if !part.is_empty()
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    {
                        Some(Identifier::Text(part.to_owned()))
                    } else {
                        None
                    }
                })
                .collect::<Option<_>>()?,
        };
        Some(Self {
            core: parsed,
            prerelease,
        })
    }
}

/// A decimal without leading zeros that fits a safe integer.
fn numeric(text: &str) -> Option<u64> {
    if text.is_empty()
        || !text.bytes().all(|byte| byte.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return None;
    }
    text.parse().ok().filter(|value| *value <= MAX_SAFE_INTEGER)
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (left, right) in self.prerelease.iter().zip(&other.prerelease) {
                        let order = match (left, right) {
                            (Identifier::Number(a), Identifier::Number(b)) => a.cmp(b),
                            (Identifier::Number(_), Identifier::Text(_)) => Ordering::Less,
                            (Identifier::Text(_), Identifier::Number(_)) => Ordering::Greater,
                            (Identifier::Text(a), Identifier::Text(b)) => a.cmp(b),
                        };
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                    self.prerelease.len().cmp(&other.prerelease.len())
                }
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Target {
    /// Relative to the manifest URL.
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Distribution {
    kind: String,
    version: Option<String>,
    command: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes_url: Option<String>,
    rollout_percent: u64,
    minimum_version: Option<String>,
    revoked_versions: Vec<String>,
    pub targets: Vec<(String, Target)>,
    distributions: Vec<Distribution>,
}

impl Release {
    pub fn target(&self, name: &str) -> Option<&Target> {
        self.targets
            .iter()
            .find(|(target, _)| target == name)
            .map(|(_, target)| target)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Manifest {
    app: String,
    artifact: String,
    channel: String,
    pub release: Release,
}

const MAX_TARGETS: usize = 64;
const MAX_REVOKED: usize = 256;

fn is_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn is_target_name(text: &str) -> bool {
    let mut parts = text.split('-');
    let first = parts.next().unwrap_or_default();
    let rest: Vec<&str> = parts.collect();
    !first.is_empty()
        && first
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && rest.len() <= 3
        && rest.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn text(value: &Value, max: usize) -> Option<&str> {
    value.as_str().filter(|text| text.chars().count() <= max)
}

fn version(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| Version::parse(text).is_some())
        .map(str::to_owned)
}

fn integer(value: &Value, min: u64, max: u64) -> Option<u64> {
    let number = value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.fract() == 0.0 && *number >= 0.0)
            .map(|number| number as u64)
    })?;
    (min..=max).contains(&number).then_some(number)
}

fn https_url(value: &Value) -> Option<String> {
    let url = text(value, 2048)?;
    let rest = url.strip_prefix("https://")?;
    (!rest.is_empty() && !url.chars().any(char::is_whitespace)).then(|| url.to_owned())
}

fn distribution(value: &Value) -> Option<Distribution> {
    let raw = value.as_object()?;
    let kind = text(raw.get("kind")?, 2048).filter(|kind| is_name(kind))?;
    if kind == "direct" {
        return Some(Distribution {
            kind: kind.into(),
            version: None,
            command: None,
        });
    }
    let version = match raw.get("version") {
        None => None,
        Some(value) => Some(self::version(value)?),
    };
    let command = match raw.get("command") {
        None => None,
        Some(value) => Some(
            text(value, 512)
                .filter(|command| !command.is_empty() && !command.chars().any(char::is_control))?
                .to_owned(),
        ),
    };
    for field in [
        "package",
        "formula",
        "cask",
        "id",
        "bucket",
        "attribute",
        "repository",
    ] {
        if let Some(value) = raw.get(field) {
            text(value, 256).filter(|value| {
                !value.is_empty()
                    && !value
                        .chars()
                        .any(|character| character.is_control() || character.is_whitespace())
            })?;
        }
    }
    Some(Distribution {
        kind: kind.into(),
        version,
        command,
    })
}

/// Parses leniently, as clients must: unknown fields are ignored, and any
/// invalid known field rejects the whole manifest.
pub fn parse_manifest(source: &str) -> Option<Manifest> {
    let raw: Value = serde_json::from_str(source).ok()?;
    let raw = raw.as_object()?;
    if raw.get("schema")? != &Value::from(1) {
        return None;
    }
    let release = raw.get("release")?.as_object()?;
    let mut targets = Vec::new();
    for (name, item) in release.get("targets")?.as_object()? {
        let item = item.as_object()?;
        targets.push((
            name.clone(),
            Target {
                url: text(item.get("url")?, 2048)?.to_owned(),
                size: integer(item.get("size")?, 0, MAX_SAFE_INTEGER)?,
                sha256: text(item.get("sha256")?, 2048)
                    .filter(|sha| is_sha256(sha))?
                    .to_owned(),
            },
        ));
    }
    let revoked = release.get("revoked_versions")?.as_array()?;
    if revoked.len() > MAX_REVOKED {
        return None;
    }
    let revoked_versions: Vec<String> = revoked.iter().map(version).collect::<Option<_>>()?;
    let parsed = Release {
        version: version(release.get("version")?)?,
        rollout_percent: integer(release.get("rollout_percent")?, 0, 100)?,
        revoked_versions,
        targets,
        distributions: release
            .get("distributions")?
            .as_array()?
            .iter()
            .map(distribution)
            .collect::<Option<_>>()?,
        notes_url: match release.get("notes_url") {
            None => None,
            Some(value) => Some(https_url(value)?),
        },
        minimum_version: match release.get("minimum_version") {
            None => None,
            Some(value) => Some(version(value)?),
        },
    };
    check_release(&parsed)?;
    let name = |key: &str| {
        raw.get(key)
            .and_then(|value| text(value, 2048))
            .filter(|name| is_name(name))
            .map(str::to_owned)
    };
    let manifest = Manifest {
        app: name("app")?,
        artifact: name("artifact")?,
        channel: name("channel")?,
        release: parsed,
    };
    integer(raw.get("sequence")?, 1, MAX_SAFE_INTEGER)?;
    text(raw.get("published_at")?, 2048)?;
    Some(manifest)
}

fn check_release(release: &Release) -> Option<()> {
    let current = Version::parse(&release.version)?;
    if let Some(minimum) = &release.minimum_version
        && Version::parse(minimum)? > current
    {
        return None;
    }
    if release.revoked_versions.contains(&release.version) {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    if !release
        .revoked_versions
        .iter()
        .all(|item| seen.insert(item))
    {
        return None;
    }
    let mut kinds = std::collections::HashSet::new();
    if !release
        .distributions
        .iter()
        .all(|item| kinds.insert(&item.kind))
    {
        return None;
    }
    (release.targets.len() <= MAX_TARGETS
        && release.targets.iter().all(|(name, _)| is_target_name(name)))
    .then_some(())
}

/// What this install is, as the protocol's client rules see it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Install {
    pub version: String,
    /// This build's target, e.g. `macos-arm64`.
    pub target: String,
    /// `direct` only with our receipt; otherwise the package manager or `unknown`.
    pub distribution: String,
    /// 0–99, drawn once per install and never sent anywhere.
    pub rollout_bucket: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Newer,
    Required,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    Reject {
        reason: &'static str,
    },
    None {
        reason: &'static str,
    },
    Install {
        reason: Reason,
        version: String,
        target: Target,
    },
    Notify {
        reason: Reason,
        version: String,
        distribution: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        notes_url: Option<String>,
    },
}

/// The protocol's client rules; the first that applies wins.
pub fn decide(
    app: &str,
    artifact: &str,
    channel: &str,
    manifest: &str,
    install: &Install,
) -> Decision {
    let running = Version::parse(&install.version).expect("the running version is valid SemVer");
    let Some(manifest) = parse_manifest(manifest) else {
        return Decision::Reject {
            reason: "malformed",
        };
    };
    if manifest.app != app {
        return Decision::Reject {
            reason: "wrong_app",
        };
    }
    if manifest.artifact != artifact {
        return Decision::Reject {
            reason: "wrong_artifact",
        };
    }
    if manifest.channel != channel {
        return Decision::Reject {
            reason: "wrong_channel",
        };
    }
    let release = manifest.release;
    let mut candidate = release.version.clone();
    let mut target = None;
    let mut distribution = None;
    if install.distribution == "direct" {
        let Some(found) = release.target(&install.target) else {
            return Decision::None {
                reason: "no_target",
            };
        };
        target = Some(found.clone());
    } else if install.distribution != "unknown" {
        let Some(found) = release
            .distributions
            .iter()
            .find(|item| item.kind == install.distribution)
        else {
            return Decision::None {
                reason: "distribution_unlisted",
            };
        };
        let Some(version) = &found.version else {
            return Decision::None {
                reason: "distribution_pending",
            };
        };
        candidate = version.clone();
        if release.revoked_versions.contains(&candidate) {
            return Decision::None {
                reason: "distribution_revoked",
            };
        }
        distribution = Some(found.clone());
    }
    let act = |reason| match &target {
        Some(target) => Decision::Install {
            reason,
            version: candidate.clone(),
            target: target.clone(),
        },
        None => Decision::Notify {
            reason,
            version: candidate.clone(),
            distribution: install.distribution.clone(),
            command: distribution.as_ref().and_then(|item| item.command.clone()),
            notes_url: release.notes_url.clone(),
        },
    };
    let candidate_version = Version::parse(&candidate).expect("parsed versions are valid");
    if release.revoked_versions.contains(&install.version) {
        return if candidate_version == running {
            Decision::None {
                reason: "up_to_date",
            }
        } else {
            act(Reason::Revoked)
        };
    }
    if candidate_version <= running {
        return Decision::None {
            reason: "up_to_date",
        };
    }
    if let Some(minimum) = &release.minimum_version
        && running < Version::parse(minimum).expect("parsed versions are valid")
    {
        return act(Reason::Required);
    }
    if install.rollout_bucket < release.rollout_percent {
        act(Reason::Newer)
    } else {
        Decision::None {
            reason: "rollout_deferred",
        }
    }
}

/// Resolves a target URL against the manifest URL (RFC 3986 for the
/// absolute-path and relative references the service writes); only HTTPS
/// results on a normalized path are accepted.
pub fn resolve_url(manifest_url: &str, reference: &str) -> Option<String> {
    if reference.contains("://") {
        return https_url(&Value::from(reference));
    }
    if reference.starts_with("//")
        || reference.contains(['?', '#', '\\'])
        || reference
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    let rest = manifest_url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (mut segments, relative): (Vec<&str>, &str) = match reference.strip_prefix('/') {
        Some(absolute) => (Vec::new(), absolute),
        None => {
            let mut base: Vec<&str> = path.split('/').collect();
            base.pop();
            (base, reference)
        }
    };
    for segment in relative.split('/') {
        match segment {
            "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    Some(format!("https://{host}/{}", segments.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECISIONS: &str = include_str!("../../tests/fixtures/release-protocol/decisions.json");
    const MANIFESTS: &str = include_str!("../../tests/fixtures/release-protocol/manifests.json");
    const SEMVER: &str = include_str!("../../tests/fixtures/release-protocol/semver.json");

    #[test]
    fn versions_follow_the_shared_vectors() {
        let vectors: Value = serde_json::from_str(SEMVER).unwrap();
        for case in vectors["compare"].as_array().unwrap() {
            let a = Version::parse(case[0].as_str().unwrap()).unwrap();
            let b = Version::parse(case[1].as_str().unwrap()).unwrap();
            let expected = match case[2].as_i64().unwrap() {
                -1 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            };
            assert_eq!(a.cmp(&b), expected, "{case}");
            assert_eq!(b.cmp(&a), expected.reverse(), "{case}");
        }
        for invalid in vectors["invalid"].as_array().unwrap() {
            assert_eq!(Version::parse(invalid.as_str().unwrap()), None, "{invalid}");
        }
        assert!(Version::parse(env!("CARGO_PKG_VERSION")).is_some());
    }

    #[test]
    fn decisions_follow_the_shared_vectors() {
        let decisions: Value = serde_json::from_str(DECISIONS).unwrap();
        let manifests: Value = serde_json::from_str(MANIFESTS).unwrap();
        let cases = decisions["cases"].as_array().unwrap();
        assert!(cases.len() >= 20);
        for case in cases {
            let input = &case["input"];
            let manifest = manifests["manifests"][input["manifest"].as_str().unwrap()]["manifest"]
                .as_str()
                .unwrap();
            let install = Install {
                version: input["install"]["version"].as_str().unwrap().into(),
                target: input["install"]["target"].as_str().unwrap().into(),
                distribution: input["install"]["distribution"].as_str().unwrap().into(),
                rollout_bucket: input["install"]["rollout_bucket"].as_u64().unwrap(),
            };
            let decision = decide(
                input["app"].as_str().unwrap(),
                input["artifact"].as_str().unwrap(),
                input["channel"].as_str().unwrap(),
                manifest,
                &install,
            );
            assert_eq!(
                serde_json::to_value(&decision).unwrap(),
                case["expect"],
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn identity_mismatches_are_rejected() {
        let manifests: Value = serde_json::from_str(MANIFESTS).unwrap();
        let manifest = manifests["manifests"]["base"]["manifest"].as_str().unwrap();
        let install = Install {
            version: "2.3.8".into(),
            target: "macos-arm64".into(),
            distribution: "direct".into(),
            rollout_bucket: 0,
        };
        for (app, artifact, reason) in [
            ("hex", "desktop", "wrong_app"),
            ("example", "cli", "wrong_artifact"),
        ] {
            assert_eq!(
                decide(app, artifact, "stable", manifest, &install),
                Decision::Reject { reason }
            );
        }
    }

    #[test]
    fn target_urls_resolve_against_the_manifest() {
        let manifest = "https://anoma.ly/release/hex/desktop/stable.json";
        assert_eq!(
            resolve_url(manifest, "/release/hex/files/abc/Hex.zip").as_deref(),
            Some("https://anoma.ly/release/hex/files/abc/Hex.zip")
        );
        assert_eq!(
            resolve_url(
                "https://dev.anoma.ly/release/hex/desktop/dev.json",
                "/release/hex/files/abc/Hex.zip"
            )
            .as_deref(),
            Some("https://dev.anoma.ly/release/hex/files/abc/Hex.zip")
        );
        assert_eq!(
            resolve_url(manifest, "../files/abc/Hex.zip").as_deref(),
            Some("https://anoma.ly/release/hex/files/abc/Hex.zip")
        );
        assert_eq!(
            resolve_url(manifest, "files/abc/Hex.zip").as_deref(),
            Some("https://anoma.ly/release/hex/desktop/files/abc/Hex.zip")
        );
        assert_eq!(
            resolve_url(manifest, "https://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        for refused in [
            "http://example.com/a",
            "//example.com/a",
            "/../x",
            "../../../../../x",
            "a b",
            "a?b",
        ] {
            assert_eq!(resolve_url(manifest, refused), None, "{refused}");
        }
    }
}
