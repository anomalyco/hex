//! Settings recovery is in memory. A save refuses a file that isn't a JSON
//! object, and a process that loaded one refuses to save stale defaults until
//! a fresh read sees a fix; the first write over invalid fields preserves
//! those bytes beside the original, including when a migration is first.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;
use serde_json::Value;
use serde_path_to_error::Segment;
use sha2::{Digest, Sha256};

use super::{AppSettings, path};
use crate::policy::Policy;

// A settings editor and a background migration may save at the same time.
static WRITER: Mutex<()> = Mutex::new(());

/// Paths whose last read found something other than a JSON object. While one
/// is here, a save refuses to write the defaults that read used, even after
/// the file is fixed, until a fresh read (a control command, the Linux poll,
/// or a restart) clears it. Keyed by path so tests in one process do not
/// interfere.
static UNPARSEABLE: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

/// What launch loaded: settings for the first render and the note to show.
pub(crate) struct LaunchLoad {
    pub(crate) settings: AppSettings,
    pub(crate) note: Option<LaunchNote>,
}

/// Why launch shows a note. Only a failed load makes the defaults unsafe to
/// record with; a file that isn't a JSON object still dictates with them.
pub(crate) enum LaunchNote {
    Unparseable(String),
    Failed(String),
}

impl LaunchNote {
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::Unparseable(message) | Self::Failed(message) => message,
        }
    }
}

impl AppSettings {
    /// Launch stays available even when the data folder itself cannot be
    /// resolved. Invalid settings are recovered by `load_from` in place.
    pub(crate) fn load_for_launch() -> LaunchLoad {
        Self::load_for_launch_at(path())
    }

    /// `load_for_launch` from `path`, so a test can use its own folder.
    fn load_for_launch_at(path: Result<PathBuf>) -> LaunchLoad {
        let launch = match path.and_then(|path| Self::load_checked_from(&path)) {
            Ok((settings, issue)) => {
                let note = match issue {
                    Some(LoadIssue::Unparseable(reason)) => {
                        Some(LaunchNote::Unparseable(reason.launch_note()))
                    }
                    Some(LoadIssue::Unreadable) | None => None,
                };
                LaunchLoad { settings, note }
            }
            Err(error) => {
                tracing::error!(error = format!("{error:#}"), "could not load settings");
                LaunchLoad {
                    settings: Self::default(),
                    note: Some(LaunchNote::Failed(format!(
                        "Hex couldn't read its settings and started with defaults. ({error:#})"
                    ))),
                }
            }
        };
        // Launch has no other load that publishes, so the saved runtime state (or the defaults) goes live here, before any listener or worker starts.
        launch.settings.apply_runtime();
        launch
    }

    pub(crate) fn load_from(path: &Path) -> Result<Self> {
        Ok(Self::load_checked_from(path)?.0)
    }

    /// Clears the refusal recorded for a file that wasn't a JSON object.
    /// Only the Linux poll calls this, after it adopts a valid or removed
    /// file; every other platform waits for a restart.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn forget_unparseable(path: &Path) {
        remember_unparseable(path, false);
    }

    /// `load_from` with the file problem it recovered from, from the same
    /// read, so a caller deciding whether to adopt cannot race an edit.
    pub(crate) fn load_checked_from(path: &Path) -> Result<(Self, Option<LoadIssue>)> {
        let (settings, migrated, issue) = Self::read_managed_checked_from(path, Policy::current())?;
        if migrated && let Err(error) = settings.write_to(path) {
            tracing::warn!(%error, "could not persist normalized settings");
        }
        Ok((settings, issue))
    }

    /// What `load_from` loads, without writing anything back: true when
    /// `load_from` would persist a migration. Switches a managed lock holds
    /// read off (`crate::policy`).
    pub(crate) fn read_from(path: &Path) -> Result<(Self, bool)> {
        Self::read_managed_from(path, Policy::current())
    }

    pub(super) fn read_managed_from(path: &Path, policy: Policy) -> Result<(Self, bool)> {
        let (settings, migrated, _) = Self::read_managed_checked_from(path, policy)?;
        Ok((settings, migrated))
    }

    fn read_managed_checked_from(
        path: &Path,
        policy: Policy,
    ) -> Result<(Self, bool, Option<LoadIssue>)> {
        let (mut settings, migrated, issue) = Self::read_saved_from(path)?;
        policy.enforce(&mut settings);
        Ok((settings, migrated, issue))
    }

    fn read_saved_from(path: &Path) -> Result<(Self, bool, Option<LoadIssue>)> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), false, None));
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not read settings; using defaults without changing the file");
                return Ok((Self::default(), false, Some(LoadIssue::Unreadable)));
            }
        };
        let issue = match settings_object(&data) {
            Ok(_) => None,
            Err(reason) => {
                remember_unparseable(path, true);
                Some(LoadIssue::Unparseable(reason))
            }
        };
        let (mut settings, mut repaired) = decode(&data);
        let loaded = serde_json::to_value(&settings)?;
        let hints = legacy_recognition_hints(&data);
        if let Err(error) = settings.validate_recording_duck_volume() {
            warn("recording_duck_volume", &error.to_string());
            settings.recording_duck_volume = Self::default().recording_duck_volume;
            repaired = true;
        }
        settings.dictation_processing.default_mode.name = "Global".into();
        settings.repair_bare_primary_clicks();
        #[cfg(target_os = "macos")]
        settings.repair_mouse_shortcuts();
        #[cfg(target_os = "linux")]
        settings.repair_special_keys();
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        settings.repair_section_key();
        settings.normalize_double_tap_settings();
        settings.repair_extra_dictation_shortcuts();
        // After every repair that can put Annotate back on its default
        // shortcut, which a saved record shortcut may now share: a mouse
        // button with Labs › Mouse shortcuts off, or a bare left click.
        #[cfg(hex_capture)]
        settings.clear_record_shortcut_sharing_capture();
        settings.migrate_legacy_replacements();
        settings.normalize_mode_application_names();
        settings.migrate_disabled_transcription_model();
        for hints in &hints {
            crate::dictation::vocabulary::fold_hints(&mut settings.vocabulary, hints);
        }
        settings.apply_one_time_migrations();
        settings.normalize_transcription_recents();
        settings
            .keyboard_model_switching
            .mappings
            .retain(|_, choice| choice.validate().is_ok());
        // A mode's speech choice that no longer validates is forgotten; the
        // mode itself and its applications stay.
        for mode in std::iter::once(&mut settings.dictation_processing.default_mode)
            .chain(settings.dictation_processing.modes.iter_mut())
        {
            if mode
                .speech
                .as_ref()
                .is_some_and(|choice| choice.validate().is_err())
            {
                mode.speech = None;
            }
        }
        // A saved import that can't run stays saved (`validate_saved`).
        if let Err(error) = crate::transcription::models::validate_saved(&settings.transcription) {
            warn("transcription", &error.to_string());
            settings.transcription = Self::default().transcription;
            repaired = true;
        }
        // A repair alone never rewrites the user's file. Ordinary migrations
        // keep their existing write-on-load behavior for healthy settings.
        let migrated =
            !repaired && (!hints.is_empty() || serde_json::to_value(&settings)? != loaded);
        Ok((settings, migrated, issue))
    }

    pub(super) fn write_to(&self, path: &Path) -> Result<()> {
        self.write_managed_to(path, Policy::current())
    }

    pub(super) fn write_managed_to(&self, path: &Path, policy: Policy) -> Result<()> {
        self.validate_recording_duck_volume()?;
        let _writer = WRITER.lock().unwrap_or_else(|error| error.into_inner());
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        let original = match fs::read(path) {
            Ok(original) => {
                if let Err(reason) = settings_object(&original) {
                    return Err(eyre!("{}", reason.save_error()));
                }
                Some(original)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            // Never overwrite something we could not read and preserve.
            Err(error) => return Err(error.into()),
        };
        // The in-memory settings came from a file that wasn't a JSON object.
        // They may be stale defaults, so a fixed file must be loaded instead.
        if load_found_unparseable(path) {
            return Err(eyre!(
                "settings.json couldn't be read when Hex loaded it. Restart Hex to load the fixed file. Nothing was saved."
            ));
        }
        match &original {
            Some(original) if needs_backup(original) => backup(path, original)?,
            None => {
                #[cfg(target_os = "macos")]
                crate::desktop::onboarding::record_pending_at(parent)?;
            }
            _ => {}
        }
        // Bytes first: a `Value` stores floats as f64, so building one from the
        // struct prints an f32 such as 0.37 as 0.3700000047683716. A `Value`
        // object is a `BTreeMap` (serde_json's `preserve_order` feature is
        // off), so writing one sorts every object's keys alphabetically and
        // leaves array order alone. A settings file kept in version control
        // then changes only where a setting does.
        let saved = self.as_saved(policy, original.as_deref());
        let compact = serde_json::to_vec(&*saved)?;
        let value: Value = serde_json::from_slice(&compact)?;
        let bytes = serde_json::to_vec_pretty(&value)?;
        crate::durable::write(path, &bytes)?;
        Ok(())
    }

    /// What a save writes: these settings, with the switches a managed lock
    /// holds off kept as the file (`saved`) has them.
    fn as_saved(&self, policy: Policy, saved: Option<&[u8]>) -> Cow<'_, Self> {
        if !policy.any() {
            return Cow::Borrowed(self);
        }
        let mut saved = saved.map_or_else(Self::default, |data| decode(data).0);
        // A file from before `meetings.summaries` keeps its choice under a lock.
        saved.migrate_meeting_summaries();
        let mut written = self.clone();
        policy.keep_saved(&mut written, &saved);
        Cow::Owned(written)
    }
}

fn warn(key: &str, reason: &str) {
    tracing::warn!(key, reason, "invalid setting; using its default in memory");
}

fn remember_unparseable(path: &Path, unparseable: bool) {
    let mut paths = UNPARSEABLE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if unparseable {
        paths.insert(path.to_path_buf());
    } else {
        paths.remove(path);
    }
}

fn load_found_unparseable(path: &Path) -> bool {
    UNPARSEABLE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .contains(path)
}

/// Why a load used defaults without a settings JSON object on disk, even
/// though it recovered in memory.
pub(crate) enum LoadIssue {
    Unparseable(Unparseable),
    Unreadable,
}

impl LoadIssue {
    /// What a running Hex says when a saved edit can't be adopted.
    #[cfg(target_os = "linux")]
    pub(crate) fn reload_error(&self) -> String {
        match self {
            Self::Unparseable(reason) => reason.reload_error(),
            Self::Unreadable => {
                "Hex couldn't read settings.json; keeping the settings it already has.".into()
            }
        }
    }
}

/// Why a settings file isn't a JSON object.
pub(crate) enum Unparseable {
    /// serde's message keeps its line and column.
    InvalidJson(String),
    NotObject,
}

impl Unparseable {
    /// "isn't valid JSON (<serde error>)" or "must be a JSON object".
    fn sentence(&self) -> String {
        match self {
            Self::InvalidJson(error) => format!("isn't valid JSON ({error})"),
            Self::NotObject => "must be a JSON object".into(),
        }
    }

    /// What the launch note puts in parentheses.
    fn reason(&self) -> &str {
        match self {
            Self::InvalidJson(error) => error,
            Self::NotObject => "must be a JSON object",
        }
    }

    fn launch_note(&self) -> String {
        format!(
            "Hex couldn't read settings.json and started with defaults. Your file wasn't changed; fix or remove it, then restart Hex. ({})",
            self.reason()
        )
    }

    /// The error a save returns; nothing is written.
    fn save_error(&self) -> String {
        format!(
            "settings.json {}. Fix or remove it, then restart Hex. Nothing was saved.",
            self.sentence()
        )
    }

    /// What a running Hex says when a saved edit isn't a JSON object.
    #[cfg(target_os = "linux")]
    fn reload_error(&self) -> String {
        format!(
            "settings.json {}. Fix or remove it, then restart Hex.",
            self.sentence()
        )
    }
}

/// `data` as a settings JSON object, or why it isn't one. A JSON syntax error
/// keeps serde's line and column.
fn settings_object(data: &[u8]) -> Result<Value, Unparseable> {
    match serde_json::from_slice(data) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(_) => Err(Unparseable::NotObject),
        Err(error) => Err(Unparseable::InvalidJson(error.to_string())),
    }
}

/// Recognition hints saved before they merged into the vocabulary: the
/// selected model's, then the recent models' from newest.
fn legacy_recognition_hints(data: &[u8]) -> Vec<String> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Selection {
        recognition_hints: String,
    }
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct File {
        transcription: Selection,
        transcription_recents: Vec<Selection>,
    }
    let Ok(file) = serde_json::from_slice::<File>(data) else {
        return Vec::new();
    };
    std::iter::once(file.transcription)
        .chain(file.transcription_recents.into_iter().rev())
        .map(|selection| selection.recognition_hints)
        .filter(|hints| !hints.trim().is_empty())
        .collect()
}

fn decode(data: &[u8]) -> (AppSettings, bool) {
    let mut value = match settings_object(data) {
        Ok(value) => value,
        Err(reason) => {
            warn("$", &format!("settings.json {}", reason.sentence()));
            return (AppSettings::default(), true);
        }
    };
    let mut repaired = false;
    loop {
        match serde_path_to_error::deserialize::<_, AppSettings>(&value) {
            Ok(settings) => return (settings, repaired),
            Err(error) => {
                let path: Vec<_> = error.path().iter().collect();
                warn(&error.path().to_string(), &error.inner().to_string());
                // Deleting a field invokes its actual serde default, including
                // section-specific defaults. A malformed list/map entry with
                // required fields is removed as a unit on the next attempt.
                if !remove(&mut value, &path) {
                    return (AppSettings::default(), true);
                }
                repaired = true;
            }
        }
    }
}

/// Remove the deepest existing part of an error path. Each failed attempt
/// strictly shrinks the input; missing required fields cannot loop forever.
fn remove(value: &mut Value, path: &[&Segment]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    match (value, first) {
        (Value::Object(object), Segment::Map { key }) => {
            if let Some(child) = object.get_mut(key)
                && remove(child, rest)
            {
                return true;
            }
            object.remove(key).is_some()
        }
        (Value::Array(array), Segment::Seq { index }) if *index < array.len() => {
            if !remove(&mut array[*index], rest) {
                array.remove(*index);
            }
            true
        }
        _ => false,
    }
}

fn needs_backup(data: &[u8]) -> bool {
    if settings_object(data).is_err() {
        return true;
    }
    let Ok(settings) = serde_json::from_slice::<AppSettings>(data) else {
        return true;
    };
    settings.validate_recording_duck_volume().is_err()
        || crate::transcription::models::validate_saved(&settings.transcription).is_err()
}

#[cfg(hex_capture)]
pub(super) fn healthy_file(path: &Path) -> bool {
    fs::read(path).is_ok_and(|data| !needs_backup(&data))
}

fn backup_path(path: &Path, data: &[u8]) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    // Content-addressing also deduplicates a retry after a failed save or a
    // process crash between the backup and the settings replacement.
    let digest: String = Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    name.push(format!(".unreadable-{digest}"));
    path.with_file_name(name)
}

fn backup(path: &Path, data: &[u8]) -> Result<()> {
    let backup = backup_path(path, data);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&backup) {
        Ok(mut file) => {
            file.write_all(data)?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // A partial or replaced backup is not permission to lose data.
            if fs::read(&backup)? != data {
                return Err(eyre!("settings backup is incomplete: {}", backup.display()));
            }
            crate::durable::sync_file(&backup)?;
        }
        Err(error) => return Err(error.into()),
    }
    crate::durable::sync_directory(path.parent().unwrap_or(Path::new(".")))?;
    tracing::warn!(path = %backup.display(), "preserved original settings before saving repaired settings");
    Ok(())
}

#[cfg(test)]
mod tests;
