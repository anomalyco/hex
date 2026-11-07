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

/// `<stamp> <title>` for a file that already exists at `current`: its own
/// leading date and time stamp when it has one, so a timezone change does not
/// shift a name export already wrote, else the meeting's computed stamp.
fn file_stem_from(current: &Path, manifest: &MeetingManifest, title: &str) -> String {
    let stamp = current
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(existing_stamp)
        .map_or_else(
            || LocalTime::from_ms(manifest.created_at_ms).file_stamp(),
            String::from,
        );
    format!("{stamp} {}", file_title(title))
}

/// The leading `YYYY-MM-DD HHMM` of an exported file's stem, if it has one.
fn existing_stamp(stem: &str) -> Option<&str> {
    let stamp = stem.get(..15)?;
    let bytes = stamp.as_bytes();
    let shaped = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b' '
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 13, 14]
            .iter()
            .all(|&index| bytes[index].is_ascii_digit());
    (shaped && stem.as_bytes().get(15) == Some(&b' ')).then_some(stamp)
}

/// The first free `<stem>.md`, `<stem> (2).md`, … path in `folder`.
/// `current`, the file's own path, never counts as a collision, including a
/// case-only spelling a case-insensitive volume resolves to that same file.
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
        .find(|path| {
            current.is_some_and(|current| is_current_file(path, current)) || !path.exists()
        })
        .ok_or_else(|| eyre!("too many meeting files named {stem}"))
}

/// Whether `candidate` is the file at `current` itself. `current` is exact
/// path bytes off Unix; on Unix the same device and inode count, so a case
/// change on a case-insensitive volume is not a collision.
fn is_current_file(candidate: &Path, current: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(candidate), Ok(current)) = (fs::metadata(candidate), fs::metadata(current)) {
            return candidate.dev() == current.dev() && candidate.ino() == current.ino();
        }
    }
    candidate == current
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
    let stem = file_stem_from(&current, manifest, title);
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
        let current = folder.join(format!("{stamp} Teams call 2.00 PM.md"));
        fs::write(&current, "# Teams call 2.00 PM\n").unwrap();
        let mut manifest = manifest(current.clone());

        let renamed = rename_to_title(&mut manifest, "Quarterly planning", true).expect("renamed");

        assert_eq!(
            renamed.file_name().unwrap().to_string_lossy(),
            format!("{stamp} Quarterly planning.md")
        );
        assert!(!current.exists());
        assert_eq!(
            fs::read_to_string(&renamed).unwrap(),
            "# Teams call 2.00 PM\n"
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
    fn a_rename_keeps_the_stamp_export_wrote() {
        let folder = folder("keep-stamp");
        let current = folder.join("2020-01-02 0930 Old title.md");
        fs::write(&current, "the meeting\n").unwrap();
        let mut manifest = manifest(current.clone());
        assert_ne!(LocalTime::from_ms(STARTED).file_stamp(), "2020-01-02 0930");

        let renamed = rename_to_title(&mut manifest, "New title", true).expect("renamed");

        assert_eq!(
            renamed.file_name().unwrap().to_string_lossy(),
            "2020-01-02 0930 New title.md"
        );
        assert!(!current.exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn only_a_leading_date_and_time_stamp_is_kept() {
        assert_eq!(
            existing_stamp("2026-10-15 1418 Title"),
            Some("2026-10-15 1418")
        );
        assert_eq!(existing_stamp("2026-10-15 1418"), None);
        assert_eq!(existing_stamp("2026-1-15 1418 Title"), None);
        assert_eq!(existing_stamp("2026/10/15 1418 Title"), None);
        assert_eq!(existing_stamp("Title"), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_same_file_under_another_spelling_is_not_a_collision() {
        let folder = folder("same-file");
        let current = folder.join("2026-10-15 1418 arcade planning.md");
        fs::write(&current, "the meeting\n").unwrap();
        let target = folder.join("2026-10-15 1418 Arcade planning.md");
        // APFS already resolves the case-only name to the same file; a
        // case-sensitive volume needs the hard link to make one.
        if let Err(error) = fs::hard_link(&current, &target) {
            assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        }

        assert_eq!(
            path_without_collision(&folder, "2026-10-15 1418 Arcade planning", Some(&current))
                .unwrap(),
            target
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_numbered_current_name_stays_when_the_plain_name_is_taken() {
        let folder = folder("numbered");
        let stamp = LocalTime::from_ms(STARTED).file_stamp();
        let taken = folder.join(format!("{stamp} Arcade planning.md"));
        fs::write(&taken, "another meeting\n").unwrap();
        let current = folder.join(format!("{stamp} Arcade planning (2).md"));
        fs::write(&current, "the meeting\n").unwrap();
        let mut manifest = manifest(current.clone());

        assert_eq!(
            rename_to_title(&mut manifest, "Arcade planning", true),
            None
        );

        assert!(current.exists());
        assert_eq!(fs::read_to_string(&current).unwrap(), "the meeting\n");
        assert_eq!(manifest.export_path, Some(current));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_title_with_no_name_left_falls_back_to_meeting() {
        let folder = folder("empty-title");
        let stamp = LocalTime::from_ms(STARTED).file_stamp();
        let current = folder.join(format!("{stamp} Old title.md"));
        fs::write(&current, "the meeting\n").unwrap();
        let mut manifest = manifest(current.clone());

        let renamed = rename_to_title(&mut manifest, "///", true).expect("renamed");

        assert_eq!(
            renamed.file_name().unwrap().to_string_lossy(),
            format!("{stamp} Meeting.md")
        );
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
