//! `hex annotate inspect|zoom|frames`: reading a capture bundle for a coding
//! agent, in the CLI process rather than the running Hex. These only read
//! files and decode the bundle's video, so they work with Hex closed and on
//! every OS; Labs › Agent tools for captures gates them.

use scrawl_capture::model::parse_clock_ms;
use scrawl_capture::{agent_context, handoff, sketch, zoom};

use crate::control::ControlError;

use super::library;

/// `hex annotate inspect`: the agent summary, the marks, or the manifest.
pub(crate) fn inspect(capture: &str, marks: bool, json: bool) -> Result<String, ControlError> {
    let dir = library::resolve(capture)?;
    let bundle = library::load(&dir).ok_or_else(|| library::unreadable(&dir))?;
    if marks {
        let entries = agent_context::mark_entries(&bundle);
        if json {
            let list: Vec<_> = entries.iter().map(agent_context::MarkEntry::json).collect();
            return pretty(&serde_json::Value::Array(list));
        }
        if entries.is_empty() {
            return Ok("No marks\n".into());
        }
        let mut out = String::new();
        for entry in &entries {
            out.push_str(&entry.line());
            out.push('\n');
        }
        return Ok(out);
    }
    if json {
        // The manifest as written, not a re-serialization of what loads.
        let path = dir.join("scrawl.json");
        let text = std::fs::read_to_string(&path).map_err(|error| {
            ControlError::new(
                "not-found",
                format!("couldn't read {}: {error}", path.display()),
            )
        })?;
        let manifest: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
            ControlError::new(
                "not-found",
                format!("{} isn't valid JSON: {error}", path.display()),
            )
        })?;
        return pretty(&manifest);
    }
    let mut out = sketch::summary_markdown(&bundle, &dir);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// `hex annotate zoom`: a dense sheet of frames around a moment or a mark,
/// with the bundle's ink drawn in.
pub(crate) fn zoom(
    capture: &str,
    at: Option<&str>,
    mark: Option<&str>,
    keyframe: Option<usize>,
    span_seconds: f64,
    count: usize,
    full: bool,
) -> Result<String, ControlError> {
    if !span_seconds.is_finite() || span_seconds <= 0.0 {
        return Err(ControlError::new("invalid", "--span must be positive"));
    }
    let dir = library::resolve(capture)?;
    let bundle = library::load(&dir).ok_or_else(|| library::unreadable(&dir))?;
    let (focus, line) = match mark {
        Some(query) => {
            let entry = agent_context::find_mark(&bundle, query, keyframe)
                .map_err(|message| ControlError::new("not-found", message))?;
            let line = entry.line();
            (zoom::Focus::Mark(entry), Some(line))
        }
        None => (
            zoom::Focus::At(at.ok_or_else(|| ControlError::new("invalid", "pass --at or --mark"))?),
            None,
        ),
    };
    let span_ms = (span_seconds * 1_000.0) as u64;
    let sheet = zoom::zoom(&dir, &bundle, focus, span_ms, count, !full)
        .map_err(|error| ControlError::new("failed", format!("{error:#}")))?;
    Ok(zoom_text(line.as_deref(), &sheet))
}

/// What `hex annotate zoom` prints: the mark it zoomed into, the sheet's
/// path, each cell's time, and what was said over the window.
fn zoom_text(mark: Option<&str>, sheet: &zoom::ZoomSheet) -> String {
    let mut out = String::new();
    if let Some(line) = mark {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&sheet.path.display().to_string());
    out.push('\n');
    let cells: Vec<String> = sheet
        .cells
        .iter()
        .map(|(index, t)| format!("#{index} {}", zoom::clock(*t)))
        .collect();
    out.push_str(&format!(
        "{} → {}, {} frames: {}",
        zoom::clock(sheet.from_ms),
        zoom::clock(sheet.to_ms),
        sheet.cells.len(),
        cells.join(", ")
    ));
    out.push('\n');
    if !sheet.said.is_empty() {
        out.push_str(&format!("Said: “{}”\n", sheet.said));
    }
    out
}

/// `hex annotate frames`: one PNG per frame across a time range.
pub(crate) fn frames(
    capture: &str,
    from: &str,
    to: &str,
    fps: u32,
) -> Result<String, ControlError> {
    let from_ms = clock_arg("--from", from)?;
    let to_ms = clock_arg("--to", to)?;
    if to_ms < from_ms {
        return Err(ControlError::new(
            "invalid",
            "the range ends before it starts",
        ));
    }
    if !(1..=240).contains(&fps) {
        return Err(ControlError::new(
            "invalid",
            "FPS must be between 1 and 240",
        ));
    }
    let dir = library::resolve(capture)?;
    let paths = handoff::extract_frames_range(&dir, from_ms, to_ms, fps)
        .map_err(|error| ControlError::new("failed", format!("{error:#}")))?;
    let mut out = String::new();
    for path in paths {
        out.push_str(&path.display().to_string());
        out.push('\n');
    }
    Ok(out)
}

/// A `--from` or `--to` time: `M:SS`, seconds, or `SS.mmm`.
fn clock_arg(name: &str, value: &str) -> Result<u64, ControlError> {
    parse_clock_ms(value)
        .ok_or_else(|| ControlError::new("invalid", format!("invalid {name} time: {value}")))
}

fn pretty(value: &serde_json::Value) -> Result<String, ControlError> {
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|error| ControlError::new("internal", format!("couldn't print JSON: {error}")))?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A bundle on disk with two marks, a transcript, and one keyframe.
    fn bundle(root: &std::path::Path) -> PathBuf {
        let dir = root.join("1792933491953.scrawl");
        std::fs::create_dir_all(&dir).unwrap();
        let frame = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
        let manifest = format!(
            r#"{{"version":1,"id":"a","createdAtMs":1792933491953,"durationMs":4000,
            "target":{{"mode":"window","app":"Brave","displayId":1,"frame":{frame}}},
            "transcript":{{"source":"parakeet","text":"make it blue","segments":[],
              "words":[{{"text":"make","t0":0,"t1":200}},{{"text":"it","t0":300,"t1":500}},
                       {{"text":"blue","t0":600,"t1":900}}]}},
            "strokes":[],
            "marks":[{{"id":"m1","strokes":[],"t0":1000,"t1":1500,
              "boxNorm":[0.1,0.1,0.4,0.4],"shape":"circle","description":"marked"}},
             {{"id":"m2","strokes":[],"t0":2500,"t1":3000,
              "boxNorm":[0.5,0.5,0.8,0.8],"shape":"circle","description":"marked"}}],
            "events":[],
            "keyframes":[{{"index":1,"t":1200,"reason":"speech","file":"frames/frame-01.png",
              "event":"","markId":"m1"}}],
            "contactSheet":"contact-sheet.png"}}"#
        );
        std::fs::write(dir.join("scrawl.json"), manifest).unwrap();
        dir
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hex-annotate-read-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn marks_print_one_line_each_and_json_is_a_list() {
        let root = temp_root("marks");
        let dir = bundle(&root);
        let path = dir.to_str().unwrap();
        let lines = inspect(path, true, false).unwrap();
        let lines: Vec<&str> = lines.lines().collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("#1 ✎ 0:01"), "{}", lines[0]);
        assert!(lines[0].ends_with("· m1"), "{}", lines[0]);
        assert!(lines[1].ends_with("· m2"), "{}", lines[1]);
        let json: serde_json::Value =
            serde_json::from_str(&inspect(path, true, true).unwrap()).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 2);
        assert_eq!(json[1]["id"], "m2");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_bundle_without_marks_says_so_and_manifest_json_is_pretty() {
        let root = temp_root("no-marks");
        let plain = root.join("1792933491954.scrawl");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(
            plain.join("scrawl.json"),
            r#"{"version":1,"id":"b","createdAtMs":1,"durationMs":1000,
            "target":{"mode":"display","displayId":0,
              "frame":{"x":0,"y":0,"width":1,"height":1,"scale":1}},
            "transcript":{"source":"hex","text":"","segments":[],"words":[]},
            "strokes":[],"marks":[],"events":[],"keyframes":[],
            "contactSheet":"c.png","note":"hello"}"#,
        )
        .unwrap();
        let path = plain.to_str().unwrap();
        assert_eq!(inspect(path, true, false).unwrap(), "No marks\n");
        let manifest = inspect(path, false, true).unwrap();
        assert!(
            manifest.contains("\n  \"note\": \"hello\",\n"),
            "{manifest}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_summary_is_summary_markdown_and_unknown_captures_are_not_found() {
        let root = temp_root("summary");
        let dir = bundle(&root);
        let summary = inspect(dir.to_str().unwrap(), false, false).unwrap();
        assert!(summary.ends_with('\n'), "{summary:?}");
        assert!(summary.contains("Brave"), "{summary}");
        let error = inspect("no-such-capture", false, false).unwrap_err();
        assert_eq!(error.code, "not-found");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn zoom_needs_a_moment_and_a_positive_span() {
        let root = temp_root("zoom-args");
        let dir = bundle(&root);
        let path = dir.to_str().unwrap();
        let error = zoom(path, None, None, None, 2.0, 12, false).unwrap_err();
        assert_eq!(error.message, "pass --at or --mark");
        let error = zoom(path, Some("0:01"), None, None, 0.0, 12, false).unwrap_err();
        assert_eq!(error.message, "--span must be positive");
        let error = zoom(path, None, Some("9"), None, 2.0, 12, false).unwrap_err();
        assert_eq!(error.code, "not-found");
        // The bundle has no video to decode, so the sheet fails with why.
        let error = zoom(path, Some("0:01"), None, None, 2.0, 12, false).unwrap_err();
        assert_eq!(error.message, "The bundle has no video to zoom into");
        let _ = std::fs::remove_dir_all(root);
    }

    /// The lines an agent reads: a mark first when one was asked for, then the
    /// sheet's path, each cell's time, and what was said over the window.
    #[test]
    fn a_sheet_reads_as_its_mark_cells_and_what_was_said() {
        let sheet = zoom::ZoomSheet {
            path: PathBuf::from("/captures/1792.scrawl/zoom/zoom-9500-11500.png"),
            cells: vec![(1, 9_500), (2, 9_681), (3, 9_863)],
            from_ms: 9_500,
            to_ms: 11_500,
            said: "but the width changes".into(),
        };
        let text = zoom_text(Some("#2 ✎② 0:09 circled · m2"), &sheet);
        assert!(text.ends_with('\n'));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            vec![
                "#2 ✎② 0:09 circled · m2",
                &sheet.path.display().to_string(),
                "0:09.500 → 0:11.500, 3 frames: #1 0:09.500, #2 0:09.681, #3 0:09.863",
                "Said: “but the width changes”",
            ]
        );
        let quiet = zoom::ZoomSheet {
            said: String::new(),
            ..sheet
        };
        let text = zoom_text(None, &quiet);
        assert_eq!(text.lines().count(), 2, "{text:?}");
        assert!(!text.contains("Said:"), "{text:?}");
    }

    #[test]
    fn frames_reads_both_times_before_the_bundle() {
        let error = frames("latest", "0:05", "0:02", 4).unwrap_err();
        assert_eq!(error.message, "the range ends before it starts");
        let error = frames("latest", "0:01", "0:02", 0).unwrap_err();
        assert_eq!(error.message, "FPS must be between 1 and 240");
        let error = frames("latest", "soon", "0:02", 4).unwrap_err();
        assert_eq!(error.message, "invalid --from time: soon");
    }
}
