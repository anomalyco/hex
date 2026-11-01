//! Settings recovery is in memory. The first write preserves unreadable bytes
//! beside the original, including when a migration is the first writer.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;
use serde_json::Value;
use serde_path_to_error::Segment;
use sha2::{Digest, Sha256};

use super::AppSettings;

// A settings editor and a background migration may save at the same time.
static WRITER: Mutex<()> = Mutex::new(());

impl AppSettings {
    /// Launch stays available even when the data folder itself cannot be
    /// resolved. Invalid settings are recovered by `load_from` in place.
    pub fn load_for_launch() -> (Self, Option<String>) {
        match Self::load() {
            Ok(settings) => (settings, None),
            Err(error) => {
                tracing::error!(error = format!("{error:#}"), "could not load settings");
                (
                    Self::default(),
                    Some(format!(
                        "Hex couldn't read its settings and started with defaults. ({error:#})"
                    )),
                )
            }
        }
    }

    pub(crate) fn load_from(path: &Path) -> Result<Self> {
        let (settings, migrated) = Self::read_from(path)?;
        if migrated && let Err(error) = settings.write_to(path) {
            tracing::warn!(%error, "could not persist normalized settings");
        }
        Ok(settings)
    }

    /// What `load_from` loads, without writing anything back: true when
    /// `load_from` would persist a migration.
    pub(crate) fn read_from(path: &Path) -> Result<(Self, bool)> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Self::default(), false));
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not read settings; using defaults without changing the file");
                return Ok((Self::default(), false));
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
        settings.normalize_double_tap_settings();
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
        Ok((settings, migrated))
    }

    pub(super) fn write_to(&self, path: &Path) -> Result<()> {
        self.validate_recording_duck_volume()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        let _writer = WRITER.lock().unwrap_or_else(|error| error.into_inner());
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("settings path has no parent"))?;
        fs::create_dir_all(parent)?;
        match fs::read(path) {
            Ok(original) if needs_backup(&original) => backup(path, &original)?,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                #[cfg(target_os = "macos")]
                crate::desktop::onboarding::record_pending_at(parent)?;
            }
            // Never overwrite something we could not read and preserve.
            Err(error) => return Err(error.into()),
        }
        crate::durable::write(path, &bytes)?;
        Ok(())
    }
}

fn warn(key: &str, reason: &str) {
    tracing::warn!(key, reason, "invalid setting; using its default in memory");
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
    let mut value: Value = match serde_json::from_slice(data) {
        Ok(Value::Object(object)) => Value::Object(object),
        Ok(_) => {
            warn("$", "settings must be a JSON object");
            return (AppSettings::default(), true);
        }
        Err(error) => {
            warn("$", &error.to_string());
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
    if !serde_json::from_slice::<Value>(data).is_ok_and(|value| value.is_object()) {
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
