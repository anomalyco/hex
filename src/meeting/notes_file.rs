//! The optional pair of Markdown files beside each other in the user's
//! meetings folder: `<stem>.transcript.md` and `<stem>.notes.md` while
//! Labs › Separate notes file is on. Export chooses a stem free for both, and
//! Labs › Descriptive meeting file names moves the pair together.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};

use super::export_name::{is_current_file, path_without_collision};
use super::notes::notes_footer;
use super::transcript::LocalTime;
use super::{MeetingManifest, display_title, format_duration};

/// The extra suffix a separated transcript carries before `.md`.
pub(super) const TRANSCRIPT_SUFFIX: &str = ".transcript";

/// The transcript's path: `<stem>.transcript.md` while the notes file lab is
/// on, else the plain `<stem>.md` export exactly as before. A separated
/// export takes the first stem whose transcript and notes file are both
/// free, so a later meeting never lands on half of a pair.
pub(super) fn export_path(folder: &Path, stem: &str, separate: bool) -> Result<PathBuf> {
    if separate {
        pair_without_collision(folder, stem, TRANSCRIPT_SUFFIX, None, None)
            .map(|(transcript, _notes)| transcript)
    } else {
        path_without_collision(folder, stem, None)
    }
}

/// The first `<stem>`/`<stem> (2)`/… for which `<stem><suffix>.md` and
/// `<stem>.notes.md` are both free. A `current` file is its own path, never a
/// collision, like [`path_without_collision`].
pub(super) fn pair_without_collision(
    folder: &Path,
    stem: &str,
    suffix: &str,
    transcript_current: Option<&Path>,
    notes_current: Option<&Path>,
) -> Result<(PathBuf, PathBuf)> {
    (1..=100)
        .map(|attempt| {
            let base = if attempt == 1 {
                stem.to_string()
            } else {
                format!("{stem} ({attempt})")
            };
            (
                folder.join(format!("{base}{suffix}.md")),
                folder.join(format!("{base}.notes.md")),
            )
        })
        .find(|(transcript, notes)| {
            is_free(transcript, transcript_current) && is_free(notes, notes_current)
        })
        .ok_or_else(|| eyre!("too many meeting files named {stem}"))
}

fn is_free(candidate: &Path, current: Option<&Path>) -> bool {
    current.is_some_and(|current| is_current_file(candidate, current)) || !candidate.exists()
}

/// `.transcript` while `path` is a separated transcript, else nothing.
pub(super) fn transcript_suffix(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.ends_with(&format!("{TRANSCRIPT_SUFFIX}.md")) {
        TRANSCRIPT_SUFFIX
    } else {
        ""
    }
}

/// A tracked notes file that still exists, for Trash and retention.
pub(super) fn tracked(manifest: &MeetingManifest) -> Option<PathBuf> {
    manifest
        .notes_export_path
        .clone()
        .filter(|path| path.exists())
}

/// Writes or refreshes the meeting's notes file after its notes are applied.
/// `create` is the caller's Labs › Separate notes file switch: only a first
/// notes file is gated, while an already tracked one follows later notes.
pub(super) fn write_after_notes(manifest: &mut MeetingManifest, create: bool) {
    let Some(transcript) = manifest.export_path.clone().filter(|path| path.exists()) else {
        return;
    };
    if let Err(error) = update(manifest, &transcript, create) {
        tracing::warn!(
            %error,
            meeting = manifest.id,
            "could not write the meeting's notes file"
        );
    }
}

/// Re-renders a tracked notes file whose bytes Hex still wrote, so its
/// heading and transcript link follow a rename or chapter retitle. Never
/// creates one: only [`write_after_notes`] does, and only with its lab on.
pub(super) fn refresh(manifest: &mut MeetingManifest) {
    let Some(transcript) = manifest.export_path.clone().filter(|path| path.exists()) else {
        return;
    };
    if manifest.notes_export_path.is_none() {
        return;
    }
    if let Err(error) = update(manifest, &transcript, false) {
        tracing::warn!(
            %error,
            meeting = manifest.id,
            "could not update the meeting's notes file"
        );
    }
}

fn update(manifest: &mut MeetingManifest, transcript: &Path, create: bool) -> Result<()> {
    let path = match manifest.notes_export_path.clone() {
        Some(path) => path,
        None if create => first_free_path(transcript)?,
        None => return Ok(()),
    };
    if path.exists() {
        let unchanged = manifest
            .notes_export_sha256
            .as_deref()
            .is_some_and(|written| super::file_sha256(&path).as_deref() == Some(written));
        if !unchanged {
            tracing::info!(
                meeting = manifest.id,
                path = %path.display(),
                "kept the meeting's notes file, which changed since Hex wrote it"
            );
            return Ok(());
        }
    }
    let markdown = render(manifest, transcript);
    write_atomically(&path, &markdown)?;
    manifest.notes_export_path = Some(path);
    manifest.notes_export_sha256 = Some(super::text_sha256(markdown.as_bytes()));
    Ok(())
}

/// `<base>.notes.md`, or the first free `<base> (2).notes.md`, … when a file
/// Hex doesn't track already has the plain name. The numbered choice is
/// logged; Hex never overwrites another file.
fn first_free_path(transcript: &Path) -> Result<PathBuf> {
    let folder = transcript
        .parent()
        .ok_or_else(|| eyre!("{} has no folder", transcript.display()))?;
    let base = base_name(transcript)
        .ok_or_else(|| eyre!("{} has no Markdown name", transcript.display()))?;
    let plain = folder.join(format!("{base}.notes.md"));
    (1..=100)
        .map(|attempt| {
            folder.join(if attempt == 1 {
                format!("{base}.notes.md")
            } else {
                format!("{base} ({attempt}).notes.md")
            })
        })
        .find(|path| !path.exists())
        .inspect(|path| {
            if path != &plain {
                tracing::info!(
                    path = %path.display(),
                    "named the meeting's notes file with a number, since its name was taken"
                );
            }
        })
        .ok_or_else(|| eyre!("too many meeting notes files named {base}"))
}

/// The transcript's name without `.md` and a trailing `.transcript`, so an
/// older plain `X.md` gets `X.notes.md` too.
fn base_name(transcript: &Path) -> Option<String> {
    let name = transcript.file_name()?.to_str()?.strip_suffix(".md")?;
    Some(
        name.strip_suffix(TRANSCRIPT_SUFFIX)
            .unwrap_or(name)
            .to_string(),
    )
}

/// The notes file's Markdown: metadata, a relative link to the transcript,
/// the summary, chapters, and the notes block's footer.
pub(super) fn render(manifest: &MeetingManifest, transcript: &Path) -> String {
    let started = LocalTime::from_ms(manifest.created_at_ms);
    let mut markdown = format!("# {}\n\n", display_title(manifest).trim());
    markdown.push_str(&format!(
        "- **Date:** {}, {}\n",
        started.date(),
        started.clock()
    ));
    if let Some(duration) = manifest.duration_ms {
        markdown.push_str(&format!("- **Duration:** {}\n", format_duration(duration)));
    }
    if let Some(app) = &manifest.app {
        markdown.push_str(&format!("- **App:** {app}\n"));
    }
    let transcript_name = transcript
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    markdown.push_str(&format!(
        "- **Transcript:** [{transcript_name}]({})\n",
        percent_encode(&transcript_name)
    ));
    markdown.push_str("\n## Summary\n\n");
    let summary = manifest
        .notes
        .as_ref()
        .map(|notes| notes.summary.trim())
        .unwrap_or_default();
    markdown.push_str(if summary.is_empty() {
        "_Nothing to summarize._"
    } else {
        summary
    });
    markdown.push('\n');
    if !manifest.chapters.is_empty() {
        markdown.push_str("\n## Chapters\n");
        for chapter in &manifest.chapters {
            markdown.push_str(&format!(
                "\n### {} · {}\n",
                format_duration(chapter.start_ms),
                chapter.title.trim()
            ));
            let summary = chapter.summary.trim();
            if !summary.is_empty() {
                markdown.push('\n');
                markdown.push_str(summary);
                markdown.push('\n');
            }
        }
    }
    let footer = notes_footer(
        manifest
            .notes
            .as_ref()
            .and_then(|notes| notes.written_by.as_ref()),
    );
    markdown.push_str(&format!("\n_{footer}_\n"));
    markdown
}

/// Percent-encodes a link target, at least the space, `%`, `(`, `)`, `<`,
/// `>`, `[`, `]`, `#`, and `?` that Markdown or a shell would otherwise read.
fn percent_encode(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for byte in name.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(*byte as char);
            }
            byte => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Replaces the file atomically: the complete new bytes are flushed under a
/// temporary name before the rename, so a crash leaves one whole file.
fn write_atomically(path: &Path, markdown: &str) -> Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| eyre!("{} has no folder", path.display()))?;
    let name = path
        .file_name()
        .ok_or_else(|| eyre!("{} has no name", path.display()))?
        .to_string_lossy();
    let temporary = folder.join(format!(".{name}.tmp"));
    fs::write(&temporary, markdown)?;
    if let Ok(metadata) = fs::metadata(path) {
        let _ = fs::set_permissions(&temporary, metadata.permissions());
    }
    crate::durable::sync_file(&temporary)?;
    fs::rename(&temporary, path)?;
    Ok(super::sync_directory(folder)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::export_name::rename_to_title;
    use crate::meeting::text_sha256;

    const STARTED: u64 = 1_760_000_000_000;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hex-meeting-notes-file-{name}-{}-{}",
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
            "title": "Old title",
            "status": "complete",
            "created_at_ms": STARTED,
            "ended_at_ms": STARTED + 60_000,
            "duration_ms": 60_000,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null,
            "app": "Zoom",
            "export_path": export,
            "notes": {
                "title": "Launch plan",
                "summary": "They agreed to ship on Friday.",
                "generated_at_ms": 0,
                "elapsed_ms": 0,
                "written_by": {"writer": "apple_intelligence"}
            },
            "chapters": [
                {"title": "Plan", "summary": "The plan.", "start_ms": 65_000},
                {"title": "Loose ends", "summary": "", "start_ms": 125_000}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn a_separated_export_names_the_transcript_and_its_notes_as_a_pair() {
        let folder = folder("names");
        let stem = "2026-10-15 1418 Design sync";

        assert_eq!(
            export_path(&folder, stem, false).unwrap(),
            folder.join("2026-10-15 1418 Design sync.md")
        );
        assert_eq!(
            export_path(&folder, stem, true).unwrap(),
            folder.join("2026-10-15 1418 Design sync.transcript.md")
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_taken_pair_gets_the_next_number_without_overwriting() {
        let folder = folder("pair");
        let stem = "2026-10-15 1418 Design sync";
        fs::write(folder.join(format!("{stem}.transcript.md")), "theirs\n").unwrap();
        fs::write(folder.join(format!("{stem}.notes.md")), "their notes\n").unwrap();

        assert_eq!(
            export_path(&folder, stem, true).unwrap(),
            folder.join("2026-10-15 1418 Design sync (2).transcript.md")
        );
        assert_eq!(
            fs::read_to_string(folder.join(format!("{stem}.transcript.md"))).unwrap(),
            "theirs\n"
        );
        assert_eq!(
            fs::read_to_string(folder.join(format!("{stem}.notes.md"))).unwrap(),
            "their notes\n"
        );

        // The numbered pair keeps its shared basename when the notes land.
        let numbered = folder.join(format!("{stem} (2).transcript.md"));
        fs::write(&numbered, "# Old title\n").unwrap();
        let mut manifest = manifest(numbered);
        write_after_notes(&mut manifest, true);
        assert_eq!(
            manifest.notes_export_path,
            Some(folder.join(format!("{stem} (2).notes.md")))
        );

        // A notes file alone still holds the pair's plain stem.
        let other = "2026-10-15 1500 Other call";
        fs::write(folder.join(format!("{other}.notes.md")), "their notes\n").unwrap();
        assert_eq!(
            export_path(&folder, other, true).unwrap(),
            folder.join("2026-10-15 1500 Other call (2).transcript.md")
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn notes_are_written_beside_the_transcript_with_an_encoded_link() {
        let folder = folder("content");
        let transcript = folder.join("2026-10-15 1418 Design sync.transcript.md");
        fs::write(&transcript, "# Old title\n").unwrap();
        let mut manifest = manifest(transcript.clone());

        write_after_notes(&mut manifest, true);

        let notes = manifest.notes_export_path.clone().expect("notes path");
        assert_eq!(notes, folder.join("2026-10-15 1418 Design sync.notes.md"));
        let written = fs::read_to_string(&notes).unwrap();
        assert!(written.starts_with("# Old title\n"));
        assert!(written.contains("- **Date:** "));
        assert!(written.contains("- **Duration:** 01:00\n"));
        assert!(written.contains("- **App:** Zoom\n"));
        assert!(written.contains(
            "- **Transcript:** [2026-10-15 1418 Design sync.transcript.md](\
             2026-10-15%201418%20Design%20sync.transcript.md)\n"
        ));
        assert!(written.contains("## Summary\n\nThey agreed to ship on Friday.\n"));
        assert!(written.contains("## Chapters\n"));
        assert!(written.contains("### 01:05 · Plan\n\nThe plan.\n"));
        assert!(written.contains("### 02:05 · Loose ends\n"));
        assert!(written.contains(
            "_Written on this Mac by Apple Intelligence; the transcript below has the details._\n"
        ));
        assert_eq!(
            manifest.notes_export_sha256,
            Some(text_sha256(written.as_bytes()))
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_taken_untracked_notes_name_gets_a_number_and_is_never_replaced() {
        let folder = folder("taken");
        let transcript = folder.join("2026-10-15 1418 Design sync.transcript.md");
        fs::write(&transcript, "# Old title\n").unwrap();
        let taken = folder.join("2026-10-15 1418 Design sync.notes.md");
        fs::write(&taken, "someone else's file\n").unwrap();
        let mut manifest = manifest(transcript);

        write_after_notes(&mut manifest, true);

        let notes = manifest.notes_export_path.clone().expect("notes path");
        assert_eq!(
            notes,
            folder.join("2026-10-15 1418 Design sync (2).notes.md")
        );
        assert_eq!(fs::read_to_string(&taken).unwrap(), "someone else's file\n");

        // A second write tracks the numbered file instead of adding another.
        write_after_notes(&mut manifest, true);
        assert_eq!(manifest.notes_export_path, Some(notes.clone()));
        let notes_files = fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".notes.md"))
            .count();
        assert_eq!(notes_files, 2);
        assert!(notes.exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_edited_notes_file_is_left_alone_even_with_new_notes() {
        let folder = folder("edited");
        let transcript = folder.join("2026-10-15 1418 Design sync.transcript.md");
        fs::write(&transcript, "# Old title\n").unwrap();
        let mut manifest = manifest(transcript);
        write_after_notes(&mut manifest, true);
        let notes = manifest.notes_export_path.clone().unwrap();
        fs::write(&notes, "# My own notes\n").unwrap();

        manifest.notes.as_mut().unwrap().summary = "A newer summary.".into();
        write_after_notes(&mut manifest, true);

        assert_eq!(fs::read_to_string(&notes).unwrap(), "# My own notes\n");
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_unedited_tracked_notes_file_follows_later_notes_without_the_lab() {
        let folder = folder("later");
        let transcript = folder.join("2026-10-15 1418 Design sync.transcript.md");
        fs::write(&transcript, "# Old title\n").unwrap();
        let mut manifest = manifest(transcript);
        write_after_notes(&mut manifest, true);
        let notes = manifest.notes_export_path.clone().unwrap();

        manifest.notes.as_mut().unwrap().summary = "A newer summary.".into();
        write_after_notes(&mut manifest, false);

        let written = fs::read_to_string(&notes).unwrap();
        assert!(written.contains("A newer summary."));
        assert_eq!(
            manifest.notes_export_sha256,
            Some(text_sha256(written.as_bytes()))
        );
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 2);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_descriptive_rename_moves_both_files_and_updates_the_link() {
        let folder = folder("rename");
        let transcript = folder.join("2026-10-15 1418 Old title.transcript.md");
        fs::write(&transcript, "# Old title\n").unwrap();
        let mut manifest = manifest(transcript.clone());
        write_after_notes(&mut manifest, true);
        let notes = manifest.notes_export_path.clone().unwrap();

        manifest.title = "Arcade planning".into();
        let renamed = rename_to_title(&mut manifest, "Arcade planning", true).expect("renamed");
        refresh(&mut manifest);

        assert_eq!(
            renamed,
            folder.join("2026-10-15 1418 Arcade planning.transcript.md")
        );
        let moved_notes = folder.join("2026-10-15 1418 Arcade planning.notes.md");
        assert_eq!(manifest.notes_export_path, Some(moved_notes.clone()));
        assert!(!transcript.exists());
        assert!(!notes.exists());
        let written = fs::read_to_string(&moved_notes).unwrap();
        assert!(written.starts_with("# Arcade planning\n"));
        assert!(written.contains(
            "[2026-10-15 1418 Arcade planning.transcript.md](\
             2026-10-15%201418%20Arcade%20planning.transcript.md)"
        ));
        assert_eq!(
            manifest.notes_export_sha256,
            Some(text_sha256(written.as_bytes()))
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_manifest_round_trips_the_notes_file_and_older_ones_still_load() {
        let folder = folder("manifest");
        let mut manifest = manifest(folder.join("2026-10-15 1418 Design sync.md"));
        manifest.notes_export_path = Some(folder.join("2026-10-15 1418 Design sync.notes.md"));
        manifest.notes_export_sha256 = Some("abc".into());

        let stored = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            stored["notes_export_path"],
            serde_json::json!(folder.join("2026-10-15 1418 Design sync.notes.md"))
        );
        assert_eq!(stored["notes_export_sha256"], "abc");
        let read: MeetingManifest = serde_json::from_value(stored).unwrap();
        assert_eq!(read.notes_export_path, manifest.notes_export_path);
        assert_eq!(read.notes_export_sha256, manifest.notes_export_sha256);

        let older: MeetingManifest = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": "1760000000000-1",
            "title": "Old meeting",
            "status": "complete",
            "created_at_ms": STARTED,
            "ended_at_ms": STARTED + 60_000,
            "duration_ms": 60_000,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null
        }))
        .unwrap();
        assert_eq!(older.notes_export_path, None);
        assert_eq!(older.notes_export_sha256, None);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_plain_export_gets_the_plain_notes_name() {
        let folder = folder("plain");
        let transcript = folder.join("2026-10-15 1418 Design sync.md");
        fs::write(&transcript, "# Old title\n").unwrap();
        let mut manifest = manifest(transcript);

        write_after_notes(&mut manifest, true);

        assert_eq!(
            manifest.notes_export_path,
            Some(folder.join("2026-10-15 1418 Design sync.notes.md"))
        );
        fs::remove_dir_all(folder).unwrap();
    }
}
