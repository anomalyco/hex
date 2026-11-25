//! The captures folder as `hex capture list|show|send|route` read it. Reads
//! and explanations run on the control thread, off the listener; a send goes
//! through the `capture-route` thread so it orders with the capsule's.

use std::path::{Path, PathBuf};

use scrawl_capture::bundle::{MANIFEST, manifest_name, read_manifest, resolve_bundle_dir_in};
use scrawl_capture::delivery;
use scrawl_capture::model::{CaptureMode, ScrawlBundle};

use crate::control::{
    ControlError, ControlReply, ScrawlCaptureDetail, ScrawlCaptureSession, ScrawlCaptureSummary,
};

/// How many captures `hex capture list` shows unless told.
const LIST: usize = 20;

pub fn list(limit: Option<usize>) -> Result<ControlReply, ControlError> {
    let mut found = listed(&crate::app_settings::capture_settings().annotation_roots())?;
    found.truncate(limit.unwrap_or(LIST));
    Ok(ControlReply::ScrawlCaptures { captures: found })
}

fn listed(roots: &[PathBuf]) -> Result<Vec<ScrawlCaptureSummary>, ControlError> {
    let mut found = Vec::new();
    for root in roots {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(ControlError::new(
                    "internal",
                    format!("couldn't read {}: {e}", root.display()),
                ));
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(b) = read_manifest(&path) {
                found.push((b.id.clone(), summary(&path, &b)));
            }
        }
    }
    found.sort_by(|(_, a), (_, b)| {
        (manifest_name(&a.bundle) != Some(MANIFEST), &a.bundle)
            .cmp(&(manifest_name(&b.bundle) != Some(MANIFEST), &b.bundle))
    });
    let mut ids = std::collections::HashSet::new();
    let mut rows: Vec<_> = found
        .into_iter()
        .filter(|(id, _)| ids.insert(id.clone()))
        .map(|(_, row)| row)
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.created_at_ms));
    Ok(rows)
}

pub fn show(capture: &str) -> Result<ControlReply, ControlError> {
    let dir = resolve(capture)?;
    let bundle = load(&dir).ok_or_else(|| unreadable(&dir))?;
    let file = |name: &Option<String>| {
        name.as_ref()
            .map(|name| dir.join(name))
            .filter(|p| p.is_file())
    };
    Ok(ControlReply::ScrawlCaptureDetail(Box::new(
        ScrawlCaptureDetail {
            summary: summary(&dir, &bundle),
            window_title: bundle.target.window_title.clone(),
            transcript: bundle.transcript.text.clone(),
            note: bundle.note.clone(),
            missing_transcript: bundle.transcript.missing.as_ref().map(|m| m.reason.clone()),
            audio_stopped_at_ms: bundle.audio_stopped_at_ms,
            marks: bundle.marks.len(),
            keyframes: bundle.keyframes.len(),
            video: file(&bundle.video),
            audio: file(&bundle.audio),
            contact_sheet: (!bundle.contact_sheet.is_empty())
                .then(|| dir.join(&bundle.contact_sheet))
                .filter(|p| p.is_file()),
        },
    )))
}

pub fn send(capture: &str, to: Option<String>, always: bool) -> Result<ControlReply, ControlError> {
    let dir = resolve(capture)?;
    if manifest_name(&dir) != Some(MANIFEST) {
        return Err(ControlError::new(
            "unsupported",
            "Legacy annotations open read-only; sending would change the original",
        ));
    }
    if load(&dir).is_none() {
        return Err(unreadable(&dir));
    }
    let message = super::routing::send_bundle(dir, to, always)
        .map_err(|message| ControlError::new("failed", message))?;
    Ok(ControlReply::Done {
        message,
        changed: true,
    })
}

pub fn explain(capture: &str, ask_jev: bool) -> Result<ControlReply, ControlError> {
    let dir = resolve(capture)?;
    super::routing::router()
        .run(|| scrawl_capture::routing::explain_bundle(&dir, None, ask_jev))
        .map(|explanation| ControlReply::ScrawlCaptureRoute { explanation })
        .map_err(|error| ControlError::new("failed", format!("{error:#}")))
}

/// A bundle name, path, or `latest`, inside the captures folder or not.
pub(super) fn resolve(capture: &str) -> Result<PathBuf, ControlError> {
    let roots = crate::app_settings::capture_settings().annotation_roots();
    if capture == "latest" {
        return listed(&roots)?
            .into_iter()
            .next()
            .map(|s| s.bundle)
            .ok_or_else(|| ControlError::new("not-found", "No completed annotations found"));
    }
    let path = Path::new(capture);
    if let Ok(dir) = resolve_bundle_dir_in(None, path) {
        return Ok(dir);
    }
    roots
        .iter()
        .find_map(|r| resolve_bundle_dir_in(Some(r), path).ok())
        .ok_or_else(|| ControlError::new("not-found", format!("Annotation not found: {capture}")))
}

pub(super) fn load(dir: &Path) -> Option<ScrawlBundle> {
    scrawl_capture::bundle::load_bundle_in(None, dir)
        .ok()
        .map(|(_, bundle)| bundle)
}

pub(super) fn unreadable(dir: &Path) -> ControlError {
    ControlError::new(
        "not-found",
        format!(
            "{} has no supported, complete annotation manifest",
            dir.display()
        ),
    )
}

fn summary(dir: &Path, bundle: &ScrawlBundle) -> ScrawlCaptureSummary {
    let mode = if bundle.source.as_deref() == Some("iphone") {
        "iphone"
    } else {
        match bundle.target.mode {
            CaptureMode::Display => "screen",
            CaptureMode::Window => "window",
            CaptureMode::Region => "area",
            CaptureMode::Device => "iphone",
        }
    };
    ScrawlCaptureSummary {
        id: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bundle: dir.to_path_buf(),
        created_at_ms: bundle.created_at_ms,
        duration_ms: bundle.duration_ms,
        mode: mode.into(),
        app: bundle.target.app.clone(),
        words: scrawl_capture::transcribe::word_count(&bundle.transcript),
        sent_to: bundle
            .destination
            .as_ref()
            .map(|dest| ScrawlCaptureSession {
                session: dest.session.clone(),
                title: dest.title.clone(),
            }),
        queued: dir.join(delivery::PENDING).exists(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_annotation_cli_resolves_both_packages_but_lists_a_copied_identity_once() {
        let root = std::env::temp_dir().join(format!("hex-library-{}", std::process::id()));
        let legacy = root.join("leftover.scrawl");
        let hex = root.join("chosen.hexannotation");
        for dir in [&legacy, &hex] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let bytes = include_bytes!("../../tests/fixtures/annotations/marked-v1.json");
        std::fs::write(legacy.join("scrawl.json"), bytes).unwrap();
        let b = read_manifest(&legacy).unwrap();
        scrawl_capture::bundle::publish(&hex, &b).unwrap();
        let found = listed(std::slice::from_ref(&root)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].bundle, hex);
        for dir in [&legacy, &hex] {
            let ControlReply::ScrawlCaptureDetail(detail) = show(dir.to_str().unwrap()).unwrap()
            else {
                panic!("wrong projection")
            };
            assert_eq!(detail.summary.created_at_ms, b.created_at_ms);
            assert_eq!(detail.transcript, "Move this");
            assert_eq!(detail.marks, 1);
            assert!(detail.video.is_none());
        }
        assert_eq!(std::fs::read(legacy.join("scrawl.json")).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_capture_on_disk_summarizes_its_mode_words_and_send() {
        let root = std::env::temp_dir().join(format!(
            "hex-capture-library-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = root.join("1792933491953.scrawl");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).is_none(), "nothing written yet");
        let frame = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
        let manifest = format!(
            r#"{{"version":1,"id":"a","createdAtMs":1792933491953,"durationMs":4000,
            "target":{{"mode":"window","app":"Brave","displayId":1,"frame":{frame}}},
            "transcript":{{"source":"parakeet","text":"make it blue","segments":[],
              "words":[{{"text":"make","t0":0,"t1":1}},{{"text":"it","t0":1,"t1":2}},
                       {{"text":"blue","t0":2,"t1":3}}]}},
            "strokes":[],"marks":[],"events":[],"keyframes":[],"contactSheet":"sheet.jpg",
            "destination":{{"session":"ses_1","title":"Hex","via":"chosen"}}}}"#
        );
        std::fs::write(dir.join("scrawl.json"), manifest).unwrap();
        std::fs::write(dir.join(delivery::PENDING), "{}").unwrap();

        let resolved = resolve(dir.to_str().unwrap()).unwrap();
        let bundle = load(&resolved).expect("the manifest reads");
        let summary = summary(&resolved, &bundle);
        assert_eq!(summary.id, "1792933491953.scrawl");
        assert_eq!(summary.mode, "window");
        assert_eq!(summary.app.as_deref(), Some("Brave"));
        assert_eq!(summary.words, 3);
        assert_eq!(
            summary.sent_to,
            Some(ScrawlCaptureSession {
                session: "ses_1".into(),
                title: "Hex".into()
            })
        );
        assert!(summary.queued);
        assert_eq!(
            resolve(root.join("missing.scrawl").to_str().unwrap())
                .unwrap_err()
                .code,
            "not-found"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// A model that gives no word times (GigaAM v3) said every word in the
    /// text, so the list counts them and the capture never reads as silence.
    #[test]
    fn a_capture_without_word_times_counts_the_words_it_says() {
        let root = std::env::temp_dir().join(format!(
            "hex-capture-untimed-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let frame = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
        let write = |name: &str, text: &str| {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let manifest = format!(
                r#"{{"version":1,"id":"a","createdAtMs":1792933491953,"durationMs":4000,
                "target":{{"mode":"display","displayId":1,"frame":{frame}}},
                "transcript":{{"source":"hex","text":"{text}","segments":[],"words":[]}},
                "strokes":[],"marks":[],"events":[],"keyframes":[],"contactSheet":"sheet.jpg"}}"#
            );
            std::fs::write(dir.join("scrawl.json"), manifest).unwrap();
            let bundle = load(&dir).expect("the manifest reads");
            summary(&dir, &bundle).words
        };
        assert_eq!(write("1792933491953.scrawl", " make it blue "), 3);
        assert_eq!(
            write("1792933491954.scrawl", "  "),
            0,
            "silence counts none"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
