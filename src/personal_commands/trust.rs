//! The trust prompt's data: what a loaded config can reach (`fetch`, or
//! programs through `node:child_process`), the hosts and programs its source
//! names where they are written literally, and the content hash the person's
//! approval is pinned to. Approval lives in Hex's own support folder, not the
//! config folder, and any change to the config's files asks again.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::io::Capabilities;

const MAX_LISTED: usize = 12;
const MAX_NAME_CHARS: usize = 64;
const MAX_TRUST_BYTES: u64 = 4096;

/// What a config can reach, as the Output pane asks about it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    /// SHA-256 of every file the config loaded.
    pub hash: String,
    pub fetch: bool,
    pub exec: bool,
    /// Hosts in `fetch("https://…")` calls.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Programs in `execFile("…")` and `exec("…")` calls.
    #[serde(default)]
    pub programs: Vec<String>,
    /// Some call builds its host or program at run time, so the lists may
    /// be incomplete.
    #[serde(default)]
    pub partial: bool,
}

impl Permissions {
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            fetch: self.fetch,
            exec: self.exec,
        }
    }
}

static FETCH_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bfetch\s*\(").unwrap());
static FETCH_LITERAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bfetch\s*\(\s*["'`]([a-zA-Z][a-zA-Z0-9+.-]*://[^/"'`\s?#$]+)"#).unwrap()
});
static EXEC_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bexec(?:File)?\b").unwrap());
static EXEC_LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bexec(?:File)?\s*\(\s*["'`]([^"'`$\r\n]+)["'`]"#).unwrap());
static CHILD_PROCESS_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"import\s*\{[^}]*\}\s*from\s*["'](?:node:)?child_process["']"#).unwrap()
});

/// Reads what `sources` (each loaded module's name and text) can reach.
/// `exec` is whether one imported `node:child_process`.
pub fn scan(sources: &[(String, String)], exec: bool) -> Permissions {
    let mut hasher = Sha256::new();
    let mut fetch = false;
    let mut hosts = Vec::new();
    let mut programs = Vec::new();
    let mut partial = false;
    for (name, source) in sources {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(source.as_bytes());
        hasher.update([0]);
        let calls = FETCH_CALL.find_iter(source).count();
        let literals = FETCH_LITERAL.captures_iter(source).collect::<Vec<_>>();
        fetch |= calls > 0;
        partial |= literals.len() < calls;
        hosts.extend(literals.iter().filter_map(|literal| {
            url::Url::parse(&format!("{}/", &literal[1]))
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
        }));
        if exec {
            let code = CHILD_PROCESS_IMPORT.replace_all(source, "");
            let names = EXEC_NAME.find_iter(&code).count();
            let literals = EXEC_LITERAL.captures_iter(&code).collect::<Vec<_>>();
            partial |= literals.len() < names;
            programs.extend(literals.iter().filter_map(|literal| {
                let program = literal[1].split_whitespace().next()?;
                Some(
                    Path::new(program)
                        .file_name()
                        .map_or(program.into(), |name| name.to_string_lossy().into_owned()),
                )
            }));
        }
    }
    Permissions {
        hash: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        fetch,
        exec,
        hosts: listed(hosts, &mut partial),
        programs: listed(programs, &mut partial),
        partial,
    }
}

fn listed(mut names: Vec<String>, partial: &mut bool) -> Vec<String> {
    names.retain(|name| !name.is_empty());
    for name in &mut names {
        if name.chars().count() > MAX_NAME_CHARS {
            *name = name.chars().take(MAX_NAME_CHARS).collect();
        }
    }
    names.sort();
    names.dedup();
    if names.len() > MAX_LISTED {
        names.truncate(MAX_LISTED);
        *partial = true;
    }
    names
}

#[derive(Default, Deserialize, Serialize)]
struct Approval {
    approved: Option<String>,
}

/// The config hash the person allowed, if any.
pub fn approved(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_TRUST_BYTES {
        return None;
    }
    serde_json::from_slice::<Approval>(&fs::read(path).ok()?)
        .ok()?
        .approved
}

/// Allows the config whose files hash to `hash`, replacing any earlier one.
pub fn approve(path: &Path, hash: &str) -> color_eyre::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::durable::write(
        path,
        &serde_json::to_vec(&Approval {
            approved: Some(hash.into()),
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(text: &str) -> Vec<(String, String)> {
        vec![("hex.config.ts".into(), text.into())]
    }

    #[test]
    fn literal_hosts_and_programs_are_listed() {
        let permissions = scan(
            &sources(
                r#"import { execFile } from "node:child_process"
const a = await fetch("http://localhost:11434/api/generate", { method: "POST" })
const b = await fetch(`https://api.example.com/v1?q=${text}`)
execFile("/opt/homebrew/bin/pandoc", ["-t", "plain"], () => {})
execFile('say', [text])
"#,
            ),
            true,
        );
        assert!(permissions.fetch && permissions.exec);
        assert_eq!(permissions.hosts, ["api.example.com", "localhost"]);
        assert_eq!(permissions.programs, ["pandoc", "say"]);
        assert!(!permissions.partial);
        assert_eq!(permissions.hash.len(), 64);
    }

    #[test]
    fn hosts_and_programs_built_at_run_time_mark_the_lists_partial() {
        let fetched = scan(&sources("const r = await fetch(url)\n"), false);
        assert!(fetched.fetch && fetched.partial && fetched.hosts.is_empty());

        let promised = scan(
            &sources(
                r#"import { execFile } from "node:child_process"
import { promisify } from "node:util"
const run = promisify(execFile)
await run("git", ["status"])
"#,
            ),
            true,
        );
        assert!(promised.exec && promised.partial);

        let pure = scan(&sources("export default { transformations: {} }\n"), false);
        assert!(!pure.capabilities().any());
    }

    #[test]
    fn any_change_to_any_file_changes_the_hash() {
        let one = scan(&sources("fetch(\"https://a.example\")"), false);
        let two = scan(&sources("fetch(\"https://a.example\") "), false);
        let mut more = sources("fetch(\"https://a.example\")");
        more.push(("lib.ts".into(), String::new()));
        assert_ne!(one.hash, two.hash);
        assert_ne!(one.hash, scan(&more, false).hash);
    }

    #[test]
    fn approval_pins_one_hash() {
        let path = std::env::temp_dir().join(format!(
            "hex-trust-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert_eq!(approved(&path), None);
        approve(&path, "abc").unwrap();
        assert_eq!(approved(&path).as_deref(), Some("abc"));
        approve(&path, "def").unwrap();
        assert_eq!(approved(&path).as_deref(), Some("def"));
        fs::remove_file(path).unwrap();
    }
}
