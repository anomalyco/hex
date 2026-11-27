use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

use super::*;
use crate::meeting::{MeetingNotes, MeetingStatus, file_sha256, notes, notes_file, text_sha256};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Window {
    Read,
    Publish,
}

type Hook = Box<dyn FnMut(&Path, Window)>;
thread_local! {
    static WINDOW: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub(super) fn publication_window(directory: &Path, window: Window) {
    WINDOW.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook(directory, window);
        }
    });
}

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let id = format!("write-{name}-{}", std::process::id());
    let base = std::env::temp_dir().join(format!("hex-{id}"));
    let directory = base.join(&id);
    fs::create_dir_all(&directory).unwrap();
    let export = base.join("2026-10-15 1418 Meeting.md");
    let markdown = "# Old title\n\n## Transcript\n\nA transcript.\n";
    fs::write(&export, markdown).unwrap();
    let manifest: MeetingManifest = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "id": id, "title": "Old title", "status": "complete",
        "created_at_ms": 1_760_000_000_000_u64, "duration_ms": 60_000,
        "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
        "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
        "export_path": export, "export_sha256": text_sha256(markdown.as_bytes())
    }))
    .unwrap();
    write_manifest(&directory, &manifest).unwrap();
    (base, directory)
}

fn summary() -> MeetingNotes {
    MeetingNotes {
        title: "Generated title".into(),
        summary: "Keep this summary.".into(),
        ..MeetingNotes::default()
    }
}

fn publish(directory: &Path) -> Result<MeetingManifest> {
    let id = directory.file_name().unwrap().to_str().unwrap();
    let claim = InProgress::claim(id).ok_or_else(|| eyre!("busy"))?;
    notes::publish_notes(directory, claim, summary(), true, true)
}

fn assert_published(directory: &Path) {
    let manifest = read_manifest(directory).unwrap();
    assert_eq!(manifest.title, "User title");
    assert_eq!(
        manifest.notes.as_ref().unwrap().summary,
        "Keep this summary."
    );
    assert_eq!(manifest.notes.as_ref().unwrap().title, "Generated title");
    assert!(manifest.notes_error.is_none());
    for (path, hash) in [
        (manifest.export_path, manifest.export_sha256),
        (manifest.notes_export_path, manifest.notes_export_sha256),
    ] {
        let path = path.unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("User title")
        );
        assert_eq!(file_sha256(&path), hash);
        let markdown = fs::read_to_string(path).unwrap();
        assert!(markdown.starts_with("# User title\n"));
        assert!(markdown.contains("Keep this summary."));
    }
}

/// Pause actual rename and actual notes publication after reading and after
/// export mutation, before manifest publication. The competing operation must
/// refuse without a write, then succeed on retry against the latest manifest.
#[test]
fn rename_and_notes_refuse_both_publication_windows_then_preserve_all_metadata() {
    for notes_first in [false, true] {
        for window in [Window::Read, Window::Publish] {
            let (base, directory) = fixture(&format!("{notes_first}-{window:?}"));
            let entered = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            let worker = {
                let directory = directory.clone();
                let entered = entered.clone();
                let resume = resume.clone();
                thread::spawn(move || {
                    WINDOW.set(Some(Box::new(move |_, at| {
                        if at == window {
                            entered.wait();
                            resume.wait();
                        }
                    })));
                    if notes_first {
                        publish(&directory)
                    } else {
                        crate::meeting::rename_meeting_with(&directory, "User title", true)
                    }
                })
            };
            entered.wait();
            let before = fs::read(directory.join("manifest.json")).unwrap();
            let refused = if notes_first {
                crate::meeting::rename_meeting_with(&directory, "User title", true)
            } else {
                publish(&directory)
            };
            let after = fs::read(directory.join("manifest.json")).unwrap();
            resume.wait();
            worker.join().unwrap().unwrap();
            assert!(refused.unwrap_err().to_string().contains("busy"));
            assert_eq!(before, after, "a refused writer must not publish");
            if notes_first {
                crate::meeting::rename_meeting_with(&directory, "User title", true).unwrap();
            } else {
                publish(&directory).unwrap();
            }
            assert_published(&directory);
            fs::remove_dir_all(base).unwrap();
        }
    }
}

#[test]
fn rename_claims_before_read_and_releases_after_read_failure() {
    let (base, directory) = fixture("claim-before-read");
    let id = directory.file_name().unwrap().to_str().unwrap();
    let claim = InProgress::claim(id).unwrap();
    fs::write(directory.join("manifest.json"), "invalid").unwrap();
    assert!(
        rename(&directory, "User title", true)
            .unwrap_err()
            .to_string()
            .contains("busy")
    );
    drop(claim);
    assert!(rename(&directory, "User title", true).is_err());
    assert!(InProgress::claim(id).is_some());
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn active_recording_rename_still_saves_the_user_title() {
    let (base, directory) = fixture("active");
    let mut manifest = read_manifest(&directory).unwrap();
    manifest.status = MeetingStatus::Recording;
    manifest.export_path = None;
    write_manifest(&directory, &manifest).unwrap();
    let renamed = rename(&directory, "User title", false).unwrap();
    assert_eq!(renamed.status, MeetingStatus::Recording);
    assert_eq!(read_manifest(&directory).unwrap().title, "User title");
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn notes_file_refresh_preserves_an_editor_save_during_temporary_write() {
    let (base, directory) = fixture("external-notes");
    let mut manifest = publish(&directory).unwrap();
    let path = manifest.notes_export_path.clone().unwrap();
    let hash = manifest.notes_export_sha256.clone();
    let external = path.clone();
    markdown::BEFORE_RECHECK.set(Some(Box::new(move || {
        fs::write(external, "# My external notes\n").unwrap();
    })));
    manifest.title = "Changed title".into();
    notes_file::refresh(&mut manifest);
    assert_eq!(fs::read_to_string(&path).unwrap(), "# My external notes\n");
    assert_eq!(manifest.notes_export_sha256, hash);
    assert_eq!(manifest.notes_export_path, Some(path));
    assert_no_temporaries(&base);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn merge_preserves_an_editor_save_during_temporary_write_and_its_ownership() {
    let (base, directory) = fixture("external-merge");
    let mut manifest = read_manifest(&directory).unwrap();
    let path = manifest.export_path.clone().unwrap();
    let hash = manifest.export_sha256.clone();
    let external = path.clone();
    markdown::BEFORE_RECHECK.set(Some(Box::new(move || {
        fs::write(external, "# My external transcript\n").unwrap();
    })));
    assert!(
        notes::retitle_markdown_file(
            &path,
            &mut manifest.export_sha256,
            "Old title",
            "User title",
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "# My external transcript\n"
    );
    assert_eq!(manifest.export_sha256, hash);
    assert_no_temporaries(&base);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn merge_keeps_user_sections_without_acquiring_their_ownership() {
    let (base, directory) = fixture("merge-policy");
    let manifest = read_manifest(&directory).unwrap();
    let path = manifest.export_path.unwrap();
    fs::write(
        &path,
        "# Old title\n\nMy additions.\n\n## Transcript\nText.\n",
    )
    .unwrap();
    let saved = publish(&directory).unwrap();
    let path = saved.export_path.unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("My additions."));
    assert!(text.contains("Keep this summary."));
    assert_eq!(saved.export_sha256, manifest.export_sha256);
    assert_ne!(saved.export_sha256, file_sha256(&path));
    fs::remove_dir_all(base).unwrap();
}

fn assert_no_temporaries(base: &Path) {
    assert!(!fs::read_dir(base).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn a_save_during_render_preserves_the_external_bytes_and_hash() {
    let (base, directory) = fixture("external-render");
    let manifest = read_manifest(&directory).unwrap();
    let path = manifest.export_path.unwrap();
    let mut written = manifest.export_sha256.clone();
    let result = markdown::rewrite(
        &path,
        &mut written,
        markdown::RewritePolicy::WholeFileIfOwned,
        |_| {
            fs::write(&path, "Saved during rendering.\n").unwrap();
            "Replacement.\n".into()
        },
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Saved during rendering.\n"
    );
    assert_eq!(written, manifest.export_sha256);
    assert_no_temporaries(&base);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_new_notes_file_does_not_replace_an_editor_save_at_the_selected_path() {
    let (base, directory) = fixture("new-notes-race");
    let mut manifest = read_manifest(&directory).unwrap();
    manifest.notes = Some(summary());
    let external = base.join("2026-10-15 1418 Meeting.notes.md");
    let saved = external.clone();
    markdown::BEFORE_RECHECK.set(Some(Box::new(move || {
        fs::write(saved, "Someone else's file.\n").unwrap();
    })));
    notes_file::write_after_notes(&mut manifest, true);
    assert_eq!(
        fs::read_to_string(external).unwrap(),
        "Someone else's file.\n"
    );
    assert!(manifest.notes_export_path.is_none());
    assert!(manifest.notes_export_sha256.is_none());
    assert_no_temporaries(&base);
    fs::remove_dir_all(base).unwrap();
}
