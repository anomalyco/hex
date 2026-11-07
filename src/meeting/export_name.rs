//! Names of exported meeting Markdown files: `<YYYY-MM-DD HHMM> <title>.md`
//! in the user's meeting folder. Export and the Labs › Descriptive meeting
//! file names rename share the same sanitizer and `" (2)"` collision scheme,
//! so a rename lands on the name export would have chosen.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};

use super::MeetingManifest;
use super::transcript::LocalTime;

/// The title part of an exported file's name: path separators and the
/// characters Windows refuses become `-`, runs of space collapse, and it is
/// capped at 80 characters.
pub(super) fn file_title(title: &str) -> String {
    let cleaned = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect::<String>();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches(|character| character == '.' || character == '-');
    let truncated = cleaned.chars().take(80).collect::<String>();
    if truncated.trim().is_empty() {
        "Meeting".into()
    } else {
        truncated.trim().into()
    }
}

/// `<YYYY-MM-DD HHMM> <title>` with no extension, from the meeting's start
/// time as export names it.
pub(super) fn file_stem(manifest: &MeetingManifest, title: &str) -> String {
    format!(
        "{} {}",
        LocalTime::from_ms(manifest.created_at_ms).file_stamp(),
        file_title(title)
    )
}

/// The first free `<stem>.md`, `<stem> (2).md`, … path in `folder`.
/// `current`, the file's own path, never counts as a collision.
pub(super) fn path_without_collision(
    folder: &Path,
    stem: &str,
    current: Option<&Path>,
) -> Result<PathBuf> {
    (1..=100)
        .map(|attempt| {
            folder.join(if attempt == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem} ({attempt}).md")
            })
        })
        .find(|path| Some(path.as_path()) == current || !path.exists())
        .ok_or_else(|| eyre!("too many meeting files named {stem}"))
}

/// Renames the exported Markdown to the meeting's current title, keeping
/// the date and time prefix. Returns the new path when it renamed; a
/// missing file, an unchanged name, or a failed rename leaves it alone.
/// The bytes do not change, so `export_sha256` stays valid for retention.
/// `enabled` is the caller's Labs › Descriptive meeting file names switch.
pub(super) fn rename_to_title(
    manifest: &mut MeetingManifest,
    title: &str,
    enabled: bool,
) -> Option<PathBuf> {
    if !enabled {
        return None;
    }
    let current = manifest.export_path.clone().filter(|path| path.exists())?;
    let folder = current.parent()?;
    let stem = file_stem(manifest, title);
    let target = match path_without_collision(folder, &stem, Some(&current)) {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(%error, meeting = manifest.id, "could not name meeting Markdown");
            return None;
        }
    };
    if target == current {
        return None;
    }
    if let Err(error) = fs::rename(&current, &target) {
        tracing::warn!(
            %error,
            meeting = manifest.id,
            from = %current.display(),
            to = %target.display(),
            "could not rename meeting Markdown"
        );
        return None;
    }
    if let Err(error) = super::sync_directory(folder) {
        tracing::warn!(
            %error,
            meeting = manifest.id,
            folder = %folder.display(),
            "could not sync the meeting folder after a rename"
        );
    }
    tracing::info!(
        meeting = manifest.id,
        from = %current.display(),
        to = %target.display(),
        "renamed meeting Markdown"
    );
    manifest.export_path = Some(target.clone());
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTED: u64 = 1_760_000_000_000;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hex-meeting-file-name-{name}-{}-{}",
            std::process::id(),
            crate::desktop::events::now_ms()
        ));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn manifest(export: PathBuf) -> MeetingManifest {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": "1760000000000-1",
            "title": "Meeting — 2:18 PM",
            "status": "complete",
            "created_at_ms": STARTED,
            "ended_at_ms": STARTED + 60_000,
            "duration_ms": 60_000,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null,
            "export_path": export,
        }))
        .unwrap()
    }

    #[test]
    fn the_file_name_follows_the_title_and_keeps_the_date_and_time() {
        let folder = folder("rename");
        let stamp = LocalTime::from_ms(STARTED).file_stamp();
        let current = folder.join(format!("{stamp} Teams call — 2:00 PM.md"));
        fs::write(&current, "# Teams call — 2:00 PM\n").unwrap();
        let mut manifest = manifest(current.clone());

        let renamed = rename_to_title(&mut manifest, "Quarterly planning", true).expect("renamed");

        assert_eq!(
            renamed.file_name().unwrap().to_string_lossy(),
            format!("{stamp} Quarterly planning.md")
        );
        assert!(!current.exists());
        assert_eq!(
            fs::read_to_string(&renamed).unwrap(),
            "# Teams call — 2:00 PM\n"
        );
        assert_eq!(manifest.export_path, Some(renamed));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_taken_name_gets_the_next_number_without_overwriting() {
        let folder = folder("collision");
        let stamp = LocalTime::from_ms(STARTED).file_stamp();
        let current = folder.join(format!("{stamp} Old title.md"));
        fs::write(&current, "the meeting\n").unwrap();
        let taken = folder.join(format!("{stamp} Arcade planning.md"));
        fs::write(&taken, "another meeting\n").unwrap();
        let mut manifest = manifest(current.clone());

        let renamed =
            rename_to_title(&mut manifest, "Arcade planning", true).expect("renamed with a suffix");

        assert_eq!(
            renamed,
            folder.join(format!("{stamp} Arcade planning (2).md"))
        );
        assert_eq!(fs::read_to_string(&taken).unwrap(), "another meeting\n");
        assert!(!current.exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_missing_or_unchanged_name_renames_nothing() {
        let folder = folder("noop");
        let stamp = LocalTime::from_ms(STARTED).file_stamp();

        let missing = folder.join(format!("{stamp} Missing.md"));
        let mut missing_manifest = manifest(missing.clone());
        assert_eq!(
            rename_to_title(&mut missing_manifest, "New title", true),
            None
        );
        assert_eq!(missing_manifest.export_path, Some(missing));

        let current = folder.join(format!("{stamp} Arcade planning.md"));
        fs::write(&current, "the meeting\n").unwrap();
        let mut unchanged = manifest(current.clone());
        assert_eq!(
            rename_to_title(&mut unchanged, "Arcade planning", true),
            None
        );
        assert!(current.exists());
        assert_eq!(unchanged.export_path, Some(current));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_lab_gate_off_leaves_the_file_alone() {
        let folder = folder("gate-off");
        let stamp = LocalTime::from_ms(STARTED).file_stamp();
        let current = folder.join(format!("{stamp} Old title.md"));
        fs::write(&current, "the meeting\n").unwrap();
        let mut manifest = manifest(current.clone());

        assert_eq!(rename_to_title(&mut manifest, "New title", false), None);

        assert!(current.exists());
        assert_eq!(manifest.export_path, Some(current));
        fs::remove_dir_all(folder).unwrap();
    }
}
