//! Reading `settings.json` without letting one field stop a launch (#744).

use std::fs;

use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;

use super::{AppSettings, path};

/// Reads `settings.json`, one bad field at a time: a value this build can't
/// read (`null` where it wants a choice, a choice a newer build added, a
/// wrong type) keeps that setting's default, with a warning, instead of
/// failing the whole load. Only a file that isn't JSON at all fails. Returns
/// the settings and the paths that fell back.
pub(super) fn decode_leniently(data: &[u8]) -> Result<(AppSettings, Vec<String>)> {
    if let Ok(settings) = serde_json::from_slice(data) {
        return Ok((settings, Vec::new()));
    }
    let file: serde_json::Value =
        serde_json::from_slice(data).wrap_err("settings.json is not valid JSON")?;
    let mut accepted = serde_json::to_value(AppSettings::default())?;
    let mut dropped = Vec::new();
    merge_leniently(&mut accepted, &file, String::new(), &mut dropped)?;
    for path in &dropped {
        tracing::warn!(
            setting = path,
            "a setting could not be read; using its default"
        );
    }
    Ok((serde_json::from_value(accepted)?, dropped))
}

/// Copies `file` into `accepted` key by key, keeping each value only while
/// the whole still decodes. Objects on both sides merge field by field, so
/// one bad leaf costs only itself.
fn merge_leniently(
    accepted: &mut serde_json::Value,
    file: &serde_json::Value,
    path: String,
    dropped: &mut Vec<String>,
) -> Result<()> {
    let serde_json::Value::Object(fields) = file else {
        return Ok(());
    };
    for (key, value) in fields {
        let child = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        let target = object_at(accepted, &path);
        let previous = target.get(key).cloned();
        if value.is_object() && previous.as_ref().is_some_and(serde_json::Value::is_object) {
            merge_leniently(accepted, value, child, dropped)?;
            continue;
        }
        target.insert(key.clone(), value.clone());
        if serde_json::from_value::<AppSettings>(accepted.clone()).is_err() {
            let target = object_at(accepted, &path);
            match previous {
                Some(previous) => target.insert(key.clone(), previous),
                None => target.remove(key),
            };
            dropped.push(child);
        }
    }
    Ok(())
}

/// The object at dotted `path` inside `root`, which [`merge_leniently`]
/// only walks through objects.
fn object_at<'a>(
    root: &'a mut serde_json::Value,
    path: &str,
) -> &'a mut serde_json::Map<String, serde_json::Value> {
    let mut value = root;
    for key in path.split('.').filter(|key| !key.is_empty()) {
        value = value
            .get_mut(key)
            .expect("merge_leniently walks existing objects");
    }
    value
        .as_object_mut()
        .expect("merge_leniently walks objects")
}

/// Recognition hints saved before they merged into the vocabulary: the
/// selected model's, then the recent models' from newest. `load_from` folds
/// them into the vocabulary and the rewritten file drops them.
pub(super) fn legacy_recognition_hints(data: &[u8]) -> Vec<String> {
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

impl AppSettings {
    /// The settings an app launches with, never failing: a file that can't
    /// be read at all is moved aside, so no save overwrites it, and the app
    /// starts with defaults and the returned message to show (#744).
    pub fn load_for_launch() -> (Self, Option<String>) {
        let error = match Self::load() {
            Ok(settings) => return (settings, None),
            Err(error) => error,
        };
        tracing::error!(error = format!("{error:#}"), "could not load settings");
        let kept = path().ok().filter(|path| path.exists()).and_then(|path| {
            let aside = path.with_file_name(format!(
                "settings.unreadable-{}.json",
                crate::desktop::events::now_ms()
            ));
            fs::rename(&path, &aside).ok().map(|()| aside)
        });
        let message = match kept {
            Some(aside) => format!(
                "Hex couldn't read its settings and started with defaults. The old file is at {}. ({error:#})",
                aside.display()
            ),
            None => {
                format!("Hex couldn't read its settings and started with defaults. ({error:#})")
            }
        };
        (Self::default(), Some(message))
    }
}
