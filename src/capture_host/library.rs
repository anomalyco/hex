//! The captures folder as `hex capture list|show|send|route` read it. Reads
//! and explanations run on the control thread, off the listener; a send goes
//! through the `capture-route` thread so it orders with the capsule's.

use std::path::{Path, PathBuf};

use scrawl_capture::bundle::resolve_bundle_dir_in;
use scrawl_capture::delivery;
use scrawl_capture::model::{CaptureMode, ScrawlBundle};

use crate::control::{
    ControlError, ControlReply, ScrawlCaptureDetail, ScrawlCaptureSession, ScrawlCaptureSummary,
};

/// How many captures `hex capture list` shows unless told.
const LIST: usize = 20;

pub fn list(limit: Option<usize>) -> Result<ControlReply, ControlError> {
    let captures = captures_dir();
    let entries = match std::fs::read_dir(&captures) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ControlReply::ScrawlCaptures {
                captures: Vec::new(),
            });
        }
        Err(error) => {
            return Err(ControlError::new(
                "internal",
                format!("couldn't read {}: {error}", captures.display()),
            ));
        }
    };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "scrawl") && path.is_dir())
        // One still being written has no scrawl.json yet.
        .filter_map(|path| load(&path).map(|bundle| summary(&path, &bundle)))
        .collect();
    found.sort_by_key(|summary| std::cmp::Reverse(summary.created_at_ms));
    found.truncate(limit.unwrap_or(LIST));
    Ok(ControlReply::ScrawlCaptures { captures: found })
}

pub fn show(capture: &str) -> Result<ControlReply, ControlError> {
    let dir = resolve(capture)?;
    let bundle = load(&dir).ok_or_else(|| unreadable(&dir))?;
    let file = |name: &Option<String>| name.as_ref().map(|name| dir.join(name));
    Ok(ControlReply::ScrawlCaptureDetail(Box::new(
        ScrawlCaptureDetail {
            summary: summary(&dir, &bundle),
            window_title: bundle.target.window_title.clone(),
            transcript: bundle.transcript.text.clone(),
            note: bundle.note.clone(),
            missing_transcript: bundle.transcript.missing.as_ref().map(|m| m.reason.clone()),
            marks: bundle.marks.len(),
            keyframes: bundle.keyframes.len(),
            video: file(&bundle.video),
            audio: file(&bundle.audio),
            contact_sheet: (!bundle.contact_sheet.is_empty())
                .then(|| dir.join(&bundle.contact_sheet)),
        },
    )))
}

pub fn send(capture: &str, to: Option<String>, always: bool) -> Result<ControlReply, ControlError> {
    let dir = resolve(capture)?;
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

fn captures_dir() -> PathBuf {
    crate::app_settings::capture_settings().captures_dir()
}

/// A bundle name, path, or `latest`, inside the captures folder or not.
fn resolve(capture: &str) -> Result<PathBuf, ControlError> {
    resolve_bundle_dir_in(Some(&captures_dir()), Path::new(capture))
        .map_err(|error| ControlError::new("not-found", format!("{error:#}")))
}

fn load(dir: &Path) -> Option<ScrawlBundle> {
    scrawl_capture::bundle::load_bundle_in(None, dir)
        .ok()
        .map(|(_, bundle)| bundle)
}

fn unreadable(dir: &Path) -> ControlError {
    ControlError::new(
        "not-found",
        format!("{} has no readable scrawl.json yet", dir.display()),
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
        words: bundle.transcript.words.len(),
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
}
