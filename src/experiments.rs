//! Experiments: unfinished features that stay dormant on `dev` until a
//! person turns them on for their own Hex. They are unsupported and may
//! change or disappear without notice.
//!
//! Two sources are unioned: `HEX_EXPERIMENTS` (names separated by commas or
//! spaces) and the `experiments` file in Hex's per-user config folder (one
//! name per line, `#` comments). The process reads them once, on first use,
//! so changes apply at the next launch. Unknown names are logged and ignored.
//! Debug builds read the same sources; `DEVELOPER_FEATURES_ENABLED` turns
//! nothing on. An experiment behind a build option (`capture`) exists only
//! in builds with that option: others do not list it, cannot enable it, and
//! treat its name as unknown.
//!
//! Gate an experiment where it starts (the worker, thread, or capture that
//! does its work), not only where its UI shows: a hidden pane whose worker
//! still runs is still on.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use clap::Subcommand;
use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};
use serde::Serialize;

pub const ENV: &str = "HEX_EXPERIMENTS";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Experiment {
    Capture,
    LiveChapters,
    Modes,
    Playground,
    SmartTurn,
    Vocabulary,
    VoiceFilter,
}

impl Experiment {
    /// Every experiment, including those behind a build option;
    /// [`Self::available`] lists the ones this build offers.
    const ALL: [Self; 7] = [
        Self::Capture,
        Self::LiveChapters,
        Self::Modes,
        Self::Playground,
        Self::SmartTurn,
        Self::Vocabulary,
        Self::VoiceFilter,
    ];

    /// The experiments this build offers: one behind a build option only
    /// when built with it.
    pub fn available() -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(|experiment| experiment.offered())
    }

    /// Whether this build has the experiment's code: `capture` only with
    /// the `capture` build option on macOS.
    const fn offered(self) -> bool {
        !matches!(self, Self::Capture) || cfg!(hex_capture)
    }

    /// The stable name the env variable, the file, and the CLI use.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::LiveChapters => "live-chapters",
            Self::Modes => "modes",
            Self::Playground => "playground",
            Self::SmartTurn => "smart-turn",
            Self::Vocabulary => "vocabulary",
            Self::VoiceFilter => "voice-filter",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Capture => {
                "Scrawl's capture engine (`hex capture`; macOS, the `capture` build option)."
            }
            Self::LiveChapters => "Chapter a meeting's transcript live while it records.",
            Self::Modes => {
                "Modes: the pane, per-app corrections, OpenCode rewriting, and transformations."
            }
            Self::Playground => {
                "Rune's playground in the window (⇧⌘D, Ctrl+Shift+D off macOS): every fixture scene and story."
            }
            Self::SmartTurn => {
                "Stop hands-free dictation when Smart Turn hears you finish (macOS)."
            }
            Self::Vocabulary => "A shared vocabulary list that guides transcription.",
            Self::VoiceFilter => {
                "The Voice Filter pane: the Hex Microphone driver and its filters (macOS)."
            }
        }
    }

    /// The offered experiment called `name`; one this build lacks is
    /// unknown.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::available().find(|experiment| experiment.name() == name)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Whether `experiment` is on for this process, as read at launch.
pub fn enabled(experiment: Experiment) -> bool {
    snapshot().enabled(experiment)
}

/// The names of the experiments on for this process, for `hex status`.
pub fn active_names() -> Vec<String> {
    snapshot()
        .active()
        .map(|experiment| experiment.name().into())
        .collect()
}

/// Logs the launch snapshot to process.log; call once logging is up in a
/// long-lived Hex.
pub fn log_launch() {
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
    SNAPSHOT.get_or_init(Snapshot::load)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Sources {
    env: bool,
    file: bool,
}

impl Sources {
    fn any(self) -> bool {
        self.env || self.file
    }

    fn labels(self) -> Vec<&'static str> {
        [(self.env, "env"), (self.file, "file")]
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
        let env = std::env::var_os(ENV).map(|value| value.to_string_lossy().into_owned());
        let (file, file_error) =
            match crate::app_paths::experiments_file().and_then(|path| read_optional(&path)) {
                Ok(file) => (file, None),
                Err(error) => (None, Some(format!("{error:#}"))),
            };
        Self {
            file_error,
            ..Self::parse(env.as_deref(), file.as_deref())
        }
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
        for (name, source) in names {
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
        experiment.offered() && self.sources[experiment.index()].any()
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
            let expected = experiment.offered().then_some(experiment);
            assert_eq!(Experiment::from_name(experiment.name()), expected);
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
                 vocabulary   # shared list\n\
                 live-chapters\n\
                 vocabulary\n\
                 # disabled-thing\n\
                 nope\r\n",
            ),
        );
        assert!(snapshot.enabled(Experiment::LiveChapters));
        assert!(snapshot.enabled(Experiment::Vocabulary));
        assert_eq!(
            snapshot.sources[Experiment::LiveChapters.index()],
            Sources {
                env: true,
                file: true
            }
        );
        assert_eq!(
            snapshot.sources[Experiment::Vocabulary.index()].labels(),
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
    fn capture_exists_only_with_its_build_option() {
        let built = cfg!(hex_capture);
        let offered = Experiment::available().any(|experiment| experiment == Experiment::Capture);
        assert_eq!(offered, built);
        assert_eq!(Experiment::from_name("capture").is_some(), built);

        let snapshot = Snapshot::parse(Some("capture"), Some("capture\n"));
        assert_eq!(snapshot.enabled(Experiment::Capture), built);
        assert_eq!(
            snapshot.unknown.is_empty(),
            built,
            "builds without the option treat `capture` as unknown"
        );
        let listed = list_json(&snapshot, Path::new("/x"))
            .experiments
            .iter()
            .any(|experiment| experiment.name == "capture");
        assert_eq!(listed, built);
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
        let created = edit(None, "vocabulary", true).unwrap();
        assert!(created.starts_with("# Hex experiments"));
        assert!(created.ends_with("\nvocabulary\n"));
        assert_eq!(edit(Some(&created), "vocabulary", true), None);

        assert_eq!(
            edit(Some("# mine\nlive-chapters"), "vocabulary", true).unwrap(),
            "# mine\nlive-chapters\nvocabulary\n"
        );
        assert_eq!(edit(Some("vocabulary # yes\n"), "vocabulary", true), None);
    }

    #[test]
    fn disabling_removes_every_line_naming_it_and_keeps_the_rest() {
        assert_eq!(
            edit(
                Some("# mine\nvocabulary\nlive-chapters # keep\nvocabulary  # again\n"),
                "vocabulary",
                false
            )
            .unwrap(),
            "# mine\nlive-chapters # keep\n"
        );
        assert_eq!(edit(Some("# vocabulary\n"), "vocabulary", false), None);
        assert_eq!(edit(None, "vocabulary", false), None);
    }

    #[test]
    fn set_creates_edits_and_reads_back_the_file() {
        let directory = temp_dir("set");
        let path = directory.join("hex-test/experiments");

        assert!(!set(&path, Experiment::Vocabulary, false).unwrap());
        assert!(!path.exists());
        assert!(set(&path, Experiment::Vocabulary, true).unwrap());
        assert!(set(&path, Experiment::LiveChapters, true).unwrap());
        assert!(!set(&path, Experiment::LiveChapters, true).unwrap());
        let snapshot = Snapshot::parse(None, read_optional(&path).unwrap().as_deref());
        assert_eq!(
            snapshot.active().collect::<Vec<_>>(),
            [Experiment::LiveChapters, Experiment::Vocabulary]
        );

        assert!(set(&path, Experiment::Vocabulary, false).unwrap());
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

        assert!(set(&link, Experiment::Vocabulary, true).unwrap());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "# kept\nvocabulary\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn list_reports_each_source() {
        let snapshot = Snapshot::parse(Some("vocabulary"), Some("vocabulary\nmystery\n"));
        let path = Path::new("/home/kit/.config/hex/experiments");
        let mut json = serde_json::to_value(list_json(&snapshot, path)).unwrap();
        if cfg!(hex_capture) {
            let first = json["experiments"].as_array_mut().unwrap().remove(0);
            assert_eq!(first["name"], "capture");
            assert_eq!(first["enabled"], false);
        }
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
                        "name": "modes",
                        "description": Experiment::Modes.description(),
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
                        "name": "smart-turn",
                        "description": Experiment::SmartTurn.description(),
                        "enabled": false,
                        "sources": [],
                    },
                    {
                        "name": "vocabulary",
                        "description": Experiment::Vocabulary.description(),
                        "enabled": true,
                        "sources": ["env", "file"],
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
        assert!(text.contains("live-chapters  off  -"));
        assert!(text.contains("vocabulary     on   env+file"));
        assert!(text.contains("unknown experiment \"mystery\" in file"));
    }
}
