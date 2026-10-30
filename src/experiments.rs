//! Experiments: unfinished features that stay dormant until a person turns
//! them on in a dev, local, or test build (`is_dev_build()`). Production
//! builds (`!is_dev_build()`) ignore `HEX_EXPERIMENTS` and
//! `~/.config/hex/experiments` completely so the public app runs strictly as
//! the dictation appliance.
//!
//! In dev builds, two sources are unioned at launch: `HEX_EXPERIMENTS` (names
//! separated by commas or spaces) and the `experiments` file in Hex's per-user
//! config folder (one name per line, `#` comments). Toggling an experiment in
//! the window's Labs page writes the file atomically and updates the live
//! runtime flags where supported. Unknown names are logged and ignored;
//! [`RETIRED`] names, experiments that shipped, are ignored silently.
//! Debug builds read the same sources; `DEVELOPER_FEATURES_ENABLED` turns
//! nothing on here.
//!
//! Gate an experiment where it starts (the worker, thread, or capture that
//! does its work), not only where its UI shows: a hidden pane whose worker
//! still runs is still on.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Subcommand;
use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};
use serde::Serialize;

pub const ENV: &str = "HEX_EXPERIMENTS";

/// Experiments that shipped or were retired: an old file or env variable may
/// still name them.
const RETIRED: [&str; 5] = ["capture", "dot-scope", "modes", "smart-turn", "vocabulary"];

/// Whether this is a developer, local, test, or dev-channel build.
/// Production builds (`!is_dev_build()`) ignore `HEX_EXPERIMENTS` and
/// `~/.config/hex/experiments` and hide experimental panes and features.
pub fn is_dev_build() -> bool {
    is_dev_build_for(
        cfg!(debug_assertions),
        cfg!(test),
        option_env!("HEX_LOCAL_BUILD"),
        option_env!("HEX_TEST_BUILD"),
        option_env!("HEX_BUILD_CHANNEL"),
        crate::VERSION,
    )
}

pub(crate) fn is_dev_build_for(
    debug: bool,
    test: bool,
    local_build: Option<&str>,
    test_build: Option<&str>,
    channel: Option<&str>,
    version: &str,
) -> bool {
    debug
        || test
        || local_build == Some("1")
        || test_build == Some("1")
        || matches!(channel, Some("dev"))
        || (channel.is_none() && version.contains("-dev"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Experiment {
    LiveChapters,
    LiveText,
    NewOnboarding,
    Playground,
    VoiceFilter,
}

impl Experiment {
    pub const ALL: [Self; 5] = [
        Self::LiveChapters,
        Self::LiveText,
        Self::NewOnboarding,
        Self::Playground,
        Self::VoiceFilter,
    ];

    pub fn available() -> impl Iterator<Item = Self> {
        Self::ALL.into_iter()
    }

    /// The stable name the env variable, the file, and the CLI use.
    pub const fn name(self) -> &'static str {
        match self {
            Self::LiveChapters => "live-chapters",
            Self::LiveText => "live-text",
            Self::NewOnboarding => "new-onboarding",
            Self::Playground => "playground",
            Self::VoiceFilter => "voice-filter",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::LiveChapters => "Chapter a meeting's transcript live while it records.",
            Self::LiveText => {
                "Live text: a streaming model publishes what you have said so far while you dictate."
            }
            Self::NewOnboarding => {
                "The new first-run onboarding: real download, permissions, and a live practice dictation (on in test builds)."
            }
            Self::Playground => {
                "Rune's playground in the window (⇧⌘D, Ctrl+Shift+D off macOS): HUD Lab, prototypes, and fixture scenes."
            }
            Self::VoiceFilter => {
                "The Voice Filter pane: the Hex Microphone driver and its filters (macOS)."
            }
        }
    }

    /// The experiment called `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::available().find(|experiment| experiment.name() == name)
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    /// On without being named: `Hex Test.app` always runs the new
    /// onboarding, so a fresh test install shows it.
    fn on_by_default(self) -> bool {
        self == Self::NewOnboarding && option_env!("HEX_TEST_BUILD") == Some("1")
    }
}

static LIVE: [AtomicBool; Experiment::ALL.len()] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

/// Whether `experiment` is on for this process. Always `false` in production
/// builds (`!is_dev_build()`).
pub fn enabled(experiment: Experiment) -> bool {
    if !is_dev_build() {
        return false;
    }
    let _ = snapshot();
    LIVE[experiment.index()].load(Ordering::Acquire)
}

/// Turns `experiment` on or off in `~/.config/hex/experiments` and updates the
/// live runtime flag for this process.
pub fn set_enabled(experiment: Experiment, on: bool) -> Result<()> {
    if !is_dev_build() {
        return Err(eyre!("experiments are only available in dev builds"));
    }
    let _ = snapshot();
    let path = crate::app_paths::experiments_file()?;
    set(&path, experiment, on)?;
    LIVE[experiment.index()].store(on, Ordering::Release);
    Ok(())
}

/// The names of the experiments on for this process, for `hex status`.
pub fn active_names() -> Vec<String> {
    if !is_dev_build() {
        return Vec::new();
    }
    Experiment::available()
        .filter(|experiment| enabled(*experiment))
        .map(|experiment| experiment.name().into())
        .collect()
}

/// Logs the launch snapshot to process.log; call once logging is up in a
/// long-lived Hex.
pub fn log_launch() {
    if !is_dev_build() {
        return;
    }
    let snapshot = snapshot();
    for unknown in &snapshot.unknown {
        tracing::warn!(
            name = %unknown.name,
            source = unknown.source.label(),
            "ignoring an unknown experiment"
        );
    }
    if let Some(error) = &snapshot.file_error {
        tracing::warn!(%error, "could not read the experiments file");
    }
    let active = snapshot.active().map(Experiment::name).collect::<Vec<_>>();
    if active.is_empty() {
        tracing::info!("no experiments enabled");
    } else {
        tracing::info!(experiments = %active.join(","), "experiments enabled");
    }
}

fn snapshot() -> &'static Snapshot {
    static SNAPSHOT: OnceLock<Snapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        let loaded = Snapshot::load();
        for experiment in Experiment::ALL {
            LIVE[experiment.index()].store(loaded.enabled(experiment), Ordering::Release);
        }
        loaded
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Sources {
    env: bool,
    file: bool,
    /// On by default in this build (`new-onboarding` in test builds).
    build: bool,
}

impl Sources {
    fn any(self) -> bool {
        self.env || self.file || self.build
    }

    fn labels(self) -> Vec<&'static str> {
        [
            (self.env, "env"),
            (self.file, "file"),
            (self.build, "build"),
        ]
        .into_iter()
        .filter_map(|(on, label)| on.then_some(label))
        .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Source {
    Env,
    File,
}

impl Source {
    const fn label(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::File => "file",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct Unknown {
    name: String,
    source: Source,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Snapshot {
    sources: [Sources; Experiment::ALL.len()],
    unknown: Vec<Unknown>,
    file_error: Option<String>,
}

impl Snapshot {
    fn load() -> Self {
        if !is_dev_build() {
            return Self::default();
        }
        let env = std::env::var_os(ENV).map(|value| value.to_string_lossy().into_owned());
        let (file, file_error) =
            match crate::app_paths::experiments_file().and_then(|path| read_optional(&path)) {
                Ok(file) => (file, None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
        let mut snapshot = Self {
            file_error,
            ..Self::parse(env.as_deref(), file.as_deref())
        };
        for experiment in Experiment::ALL {
            snapshot.sources[experiment.index()].build = experiment.on_by_default();
        }
        snapshot
    }

    fn parse(env: Option<&str>, file: Option<&str>) -> Self {
        let mut snapshot = Self::default();
        let names = env
            .into_iter()
            .flat_map(env_names)
            .map(|name| (name, Source::Env))
            .chain(
                file.into_iter()
                    .flat_map(|file| file.lines().filter_map(file_name))
                    .map(|name| (name, Source::File)),
            );
        for (name, source) in names.filter(|(name, _)| !RETIRED.contains(name)) {
            match Experiment::from_name(name) {
                Some(experiment) => {
                    let sources = &mut snapshot.sources[experiment.index()];
                    match source {
                        Source::Env => sources.env = true,
                        Source::File => sources.file = true,
                    }
                }
                None => {
                    let unknown = Unknown {
                        name: name.into(),
                        source,
                    };
                    if !snapshot.unknown.contains(&unknown) {
                        snapshot.unknown.push(unknown);
                    }
                }
            }
        }
        snapshot
    }

    fn enabled(&self, experiment: Experiment) -> bool {
        self.sources[experiment.index()].any()
    }

    fn active(&self) -> impl Iterator<Item = Experiment> + '_ {
        Experiment::available().filter(|experiment| self.enabled(*experiment))
    }
}

fn env_names(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|name| !name.is_empty())
}

/// The name on one file line, without its `#` comment.
fn file_name(line: &str) -> Option<&str> {
    let name = line.split('#').next().unwrap_or_default().trim();
    (!name.is_empty()).then_some(name)
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).wrap_err_with(|| format!("could not read {}", path.display())),
    }
}

const FILE_HEADER: &str = "\
# Hex experiments: unfinished, unsupported features, one name per line.
# `hex experiments list` shows them; changes apply when Hex next starts.
";

/// `contents` with `name` on or off, or `None` when it already was.
/// Turning off removes every line naming it and keeps everything else.
fn edit(contents: Option<&str>, name: &str, on: bool) -> Option<String> {
    let contents = contents.unwrap_or_default();
    let names_it = |line: &str| file_name(line) == Some(name);
    if on {
        if contents.lines().any(names_it) {
            return None;
        }
        let mut edited = if contents.is_empty() {
            FILE_HEADER.to_string()
        } else {
            contents.to_string()
        };
        if !edited.ends_with('\n') {
            edited.push('\n');
        }
        edited.push_str(name);
        edited.push('\n');
        Some(edited)
    } else {
        if !contents.lines().any(names_it) {
            return None;
        }
        Some(
            contents
                .lines()
                .filter(|line| !names_it(line))
                .flat_map(|line| [line, "\n"])
                .collect(),
        )
    }
}

/// Turns `experiment` on or off in the file at `path`, replacing it
/// atomically. Returns whether the file changed.
fn set(path: &Path, experiment: Experiment, on: bool) -> Result<bool> {
    // Edit through a symlinked file (a dotfiles checkout) instead of replacing it.
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(edited) = edit(read_optional(&path)?.as_deref(), experiment.name(), on) else {
        return Ok(false);
    };
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("{} has no parent folder", path.display()))?;
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("could not create {}", parent.display()))?;
    let temporary = path.with_extension(format!("{}.partial", std::process::id()));
    fs::write(&temporary, edited)
        .and_then(|()| fs::rename(&temporary, &path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
        .wrap_err_with(|| format!("could not write {}", path.display()))?;
    Ok(true)
}

#[derive(Debug, Subcommand)]
pub enum ExperimentsCommand {
    /// Show every experiment, whether it is on, and what turned it on.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Turn an experiment on in the experiments file (applies at the next launch).
    Enable { name: String },
    /// Turn an experiment off in the experiments file (applies at the next launch).
    Disable { name: String },
}

/// `hex experiments`: reads and edits only the sources, never the running Hex.
pub fn run(command: &ExperimentsCommand) -> Result<()> {
    if !is_dev_build() {
        return Err(eyre!("experiments are only available in dev builds"));
    }
    let path = crate::app_paths::experiments_file()?;
    match command {
        ExperimentsCommand::List { json } => {
            let env = std::env::var_os(ENV).map(|value| value.to_string_lossy().into_owned());
            let snapshot = Snapshot::parse(env.as_deref(), read_optional(&path)?.as_deref());
            let mut out = std::io::stdout().lock();
            if *json {
                serde_json::to_writer_pretty(&mut out, &list_json(&snapshot, &path))?;
                use std::io::Write;
                writeln!(out)?;
            } else {
                print_list(&mut out, &snapshot, &path)?;
            }
            Ok(())
        }
        ExperimentsCommand::Enable { name } | ExperimentsCommand::Disable { name } => {
            let on = matches!(command, ExperimentsCommand::Enable { .. });
            let experiment = Experiment::from_name(name).ok_or_else(|| {
                eyre!(
                    "unknown experiment {name:?}; known experiments: {}",
                    Experiment::available()
                        .map(Experiment::name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let state = if on { "on" } else { "off" };
            if set(&path, experiment, on)? {
                println!(
                    "Turned {} {state} in {}.",
                    experiment.name(),
                    path.display()
                );
                println!("{RESTART_HINT}");
            } else {
                println!(
                    "{} is already {state} in {}.",
                    experiment.name(),
                    path.display()
                );
            }
            let from_env = std::env::var_os(ENV).is_some_and(|value| {
                env_names(&value.to_string_lossy()).any(|other| other == experiment.name())
            });
            if !on && from_env {
                println!("{ENV} in this shell still turns it on for Hex started from here.");
            }
            if !on && experiment.on_by_default() {
                println!("This build turns it on regardless.");
            }
            Ok(())
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
const RESTART_HINT: &str = "Restart Hex to apply it: hex restart";
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const RESTART_HINT: &str = "Quit and reopen Hex to apply it.";

fn print_list(out: &mut impl std::io::Write, snapshot: &Snapshot, path: &Path) -> Result<()> {
    let width = Experiment::available()
        .map(|experiment| experiment.name().len())
        .max()
        .unwrap_or_default();
    for experiment in Experiment::available() {
        let sources = snapshot.sources[experiment.index()];
        let source = match sources.labels().join("+") {
            labels if labels.is_empty() => "-".to_string(),
            labels => labels,
        };
        writeln!(
            out,
            "{:<width$}  {:<3}  {:<8}  {}",
            experiment.name(),
            if sources.any() { "on" } else { "off" },
            source,
            experiment.description(),
        )?;
    }
    for unknown in &snapshot.unknown {
        writeln!(
            out,
            "unknown experiment {:?} in {} (ignored)",
            unknown.name,
            unknown.source.label()
        )?;
    }
    writeln!(out, "File: {}", path.display())?;
    writeln!(
        out,
        "Experiments are unsupported; changes apply when Hex next starts."
    )?;
    Ok(())
}

#[derive(Serialize)]
struct ListJson<'a> {
    version: u32,
    file: PathBuf,
    experiments: Vec<ExperimentJson>,
    unknown: &'a [Unknown],
}

#[derive(Serialize)]
struct ExperimentJson {
    name: &'static str,
    description: &'static str,
    enabled: bool,
    sources: Vec<&'static str>,
}

fn list_json<'a>(snapshot: &'a Snapshot, path: &Path) -> ListJson<'a> {
    ListJson {
        version: 1,
        file: path.to_path_buf(),
        experiments: Experiment::available()
            .map(|experiment| {
                let sources = snapshot.sources[experiment.index()];
                ExperimentJson {
                    name: experiment.name(),
                    description: experiment.description(),
                    enabled: sources.any(),
                    sources: sources.labels(),
                }
            })
            .collect(),
        unknown: &snapshot.unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("hex-experiments-{name}-{unique}"))
    }

    #[test]
    fn names_round_trip_and_stay_kebab_case() {
        for experiment in Experiment::ALL {
            assert_eq!(Experiment::from_name(experiment.name()), Some(experiment));
            assert!(
                experiment
                    .name()
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '-')
            );
            assert_eq!(Experiment::ALL[experiment.index()], experiment);
        }
        assert_eq!(Experiment::from_name("Live-Chapters"), None);
    }

    #[test]
    fn env_and_file_union_with_comments_unknowns_and_duplicates() {
        let snapshot = Snapshot::parse(
            Some(" live-chapters,,bogus  live-chapters "),
            Some(
                "# Hex experiments\n\
                 \n\
                 live-text   # provisional line\n\
                 live-chapters\n\
                 live-text\n\
                 # disabled-thing\n\
                 nope\r\n",
            ),
        );
        assert!(snapshot.enabled(Experiment::LiveChapters));
        assert!(snapshot.enabled(Experiment::LiveText));
        assert_eq!(
            snapshot.sources[Experiment::LiveChapters.index()],
            Sources {
                env: true,
                file: true,
                build: false,
            }
        );
        assert_eq!(
            snapshot.sources[Experiment::LiveText.index()].labels(),
            ["file"]
        );
        assert_eq!(
            snapshot.unknown,
            [
                Unknown {
                    name: "bogus".into(),
                    source: Source::Env
                },
                Unknown {
                    name: "nope".into(),
                    source: Source::File
                },
            ]
        );
    }

    #[test]
    fn retired_experiments_are_ignored_silently() {
        let snapshot = Snapshot::parse(
            Some("dot-scope,modes"),
            Some("dot-scope\nvocabulary\nsmart-turn\nlive-text\n"),
        );
        assert!(snapshot.unknown.is_empty());
        assert_eq!(
            snapshot.active().collect::<Vec<_>>(),
            [Experiment::LiveText]
        );
    }

    #[test]
    fn nothing_is_on_without_sources() {
        for snapshot in [
            Snapshot::parse(None, None),
            Snapshot::parse(Some(""), Some("# only comments\n\n")),
        ] {
            assert_eq!(snapshot.active().count(), 0);
            assert!(snapshot.unknown.is_empty());
        }
    }

    #[test]
    fn enabling_appends_once_under_a_header() {
        let created = edit(None, "live-text", true).unwrap();
        assert!(created.starts_with("# Hex experiments"));
        assert!(created.ends_with("\nlive-text\n"));
        assert_eq!(edit(Some(&created), "live-text", true), None);

        assert_eq!(
            edit(Some("# mine\nlive-chapters"), "live-text", true).unwrap(),
            "# mine\nlive-chapters\nlive-text\n"
        );
        assert_eq!(edit(Some("live-text # yes\n"), "live-text", true), None);
    }

    #[test]
    fn disabling_removes_every_line_naming_it_and_keeps_the_rest() {
        assert_eq!(
            edit(
                Some("# mine\nlive-text\nlive-chapters # keep\nlive-text  # again\n"),
                "live-text",
                false
            )
            .unwrap(),
            "# mine\nlive-chapters # keep\n"
        );
        assert_eq!(edit(Some("# live-text\n"), "live-text", false), None);
        assert_eq!(edit(None, "live-text", false), None);
    }

    #[test]
    fn set_creates_edits_and_reads_back_the_file() {
        let directory = temp_dir("set");
        let path = directory.join("hex-test/experiments");

        assert!(!set(&path, Experiment::LiveText, false).unwrap());
        assert!(!path.exists());
        assert!(set(&path, Experiment::LiveText, true).unwrap());
        assert!(set(&path, Experiment::LiveChapters, true).unwrap());
        assert!(!set(&path, Experiment::LiveChapters, true).unwrap());
        let snapshot = Snapshot::parse(None, read_optional(&path).unwrap().as_deref());
        assert_eq!(
            snapshot.active().collect::<Vec<_>>(),
            [Experiment::LiveChapters, Experiment::LiveText]
        );

        assert!(set(&path, Experiment::LiveText, false).unwrap());
        let snapshot = Snapshot::parse(None, read_optional(&path).unwrap().as_deref());
        assert_eq!(
            snapshot.active().collect::<Vec<_>>(),
            [Experiment::LiveChapters]
        );
        let leftovers = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name() != "experiments")
            .count();
        assert_eq!(leftovers, 0, "no temporary file is left behind");
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn set_edits_through_a_symlinked_file() {
        let directory = temp_dir("symlink");
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("dotfiles-experiments");
        fs::write(&target, "# kept\n").unwrap();
        let link = directory.join("experiments");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(set(&link, Experiment::LiveText, true).unwrap());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "# kept\nlive-text\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn list_reports_each_source() {
        let snapshot = Snapshot::parse(Some("live-text"), Some("live-text\nmystery\n"));
        let path = Path::new("/home/kit/.config/hex/experiments");
        let json = serde_json::to_value(list_json(&snapshot, path)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "version": 1,
                "file": "/home/kit/.config/hex/experiments",
                "experiments": [
                    {
                        "name": "live-chapters",
                        "description": Experiment::LiveChapters.description(),
                        "enabled": false,
                        "sources": [],
                    },
                    {
                        "name": "live-text",
                        "description": Experiment::LiveText.description(),
                        "enabled": true,
                        "sources": ["env", "file"],
                    },
                    {
                        "name": "new-onboarding",
                        "description": Experiment::NewOnboarding.description(),
                        "enabled": false,
                        "sources": [],
                    },
                    {
                        "name": "playground",
                        "description": Experiment::Playground.description(),
                        "enabled": false,
                        "sources": [],
                    },
                    {
                        "name": "voice-filter",
                        "description": Experiment::VoiceFilter.description(),
                        "enabled": false,
                        "sources": [],
                    },
                ],
                "unknown": [{ "name": "mystery", "source": "file" }],
            })
        );

        let mut text = Vec::new();
        print_list(&mut text, &snapshot, path).unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("live-chapters   off  -"));
        assert!(text.contains("live-text       on   env+file"));
        assert!(text.contains("unknown experiment \"mystery\" in file"));
    }

    #[test]
    fn list_reports_a_build_default() {
        let mut snapshot = Snapshot::parse(None, None);
        snapshot.sources[Experiment::NewOnboarding.index()].build = true;
        let mut text = Vec::new();
        print_list(&mut text, &snapshot, Path::new("/tmp/experiments")).unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("new-onboarding  on   build"), "{text}");
    }

    #[test]
    fn production_builds_are_not_dev_builds_and_dev_local_test_are() {
        // Stamped stable release (release.yml with channel=stable, or packaged tag):
        assert!(!is_dev_build_for(
            false,
            false,
            None,
            None,
            Some("stable"),
            "3.0.0"
        ));
        assert!(!is_dev_build_for(
            false,
            false,
            None,
            Some(""),
            Some("stable"),
            "3.0.0"
        ));
        assert!(!is_dev_build_for(false, false, None, None, None, "3.0.0"));

        // Dev, local, test, and debug builds:
        assert!(is_dev_build_for(
            true,
            false,
            None,
            None,
            Some("stable"),
            "3.0.0"
        ));
        assert!(is_dev_build_for(
            false,
            true,
            None,
            None,
            Some("stable"),
            "3.0.0"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            Some("1"),
            None,
            None,
            "3.0.0"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            Some("1"),
            None,
            "0.0.0-test-42"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            None,
            Some("dev"),
            "0.0.0-dev-42"
        ));
        assert!(is_dev_build_for(
            false,
            false,
            None,
            None,
            None,
            "0.0.0-dev-local-abc1234"
        ));
    }
}
