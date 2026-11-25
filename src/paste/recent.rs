//! One selection rule for an immediate paste and the open picker. Only the
//! stable ID crosses the selection boundary, never a payload or a row index.

#[cfg(test)]
use crate::capture_host::paste_source as captures;
use crate::capture_host::paste_source::Scope;
use crate::history::History;
use scrawl_capture::model::ScrawlBundle;
use std::path::PathBuf;

pub(crate) use crate::history::timeline::{PasteId as Id, Row, latest, snapshot};

/// What the picker's preview shows beside a row: the whole text, where the
/// row keeps its first hundred characters, and what its owner knows of it.
/// Display only. A paste resolves the row's ID again, never this copy.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Detail {
    pub text: String,
    /// Dictated without processing.
    pub raw: bool,
    pub application: Option<String>,
    pub model: Option<String>,
    /// A capture's length; 0 for a dictation.
    pub duration_ms: u64,
    /// A capture's picture: its first keyframe, else its contact sheet.
    pub picture: Option<PathBuf>,
}

/// One picker row with its preview.
#[derive(Clone)]
pub(crate) struct Entry {
    pub row: Row,
    pub detail: Detail,
}

impl From<Row> for Entry {
    fn from(row: Row) -> Self {
        Self {
            detail: Detail {
                text: row.preview.clone(),
                ..Detail::default()
            },
            row,
        }
    }
}

fn detail(history: &History, captures: Option<&Scope<'_>>, row: &Row, now: u64) -> Detail {
    let read = match &row.id {
        Id::Dictation(id) => history.paste_words(*id, now).map(|words| Detail {
            text: words.text,
            raw: words.raw,
            application: words.application,
            model: words.model,
            ..Detail::default()
        }),
        Id::Capture(id) => {
            captures
                .and_then(|scope| scope.resolve(id, now))
                .map(|(dir, bundle)| Detail {
                    picture: Scope::picture(&dir, &bundle),
                    text: bundle.transcript.text.trim().to_owned(),
                    application: bundle.target.app,
                    duration_ms: bundle.duration_ms,
                    ..Detail::default()
                })
        }
    };
    // Gone since the rows were read: the row still says what it was.
    read.unwrap_or_else(|| Entry::from(row.clone()).detail)
}

pub(crate) enum Output {
    Text(String),
    Capture(PathBuf, Box<ScrawlBundle>),
}

pub(crate) fn resolve(
    history: &History,
    captures: Option<&Scope<'_>>,
    id: &Id,
    now: u64,
) -> Option<Output> {
    match id {
        Id::Dictation(id) => history.resolve_paste(*id, now).map(Output::Text),
        Id::Capture(id) => captures?
            .resolve(id, now)
            .map(|(dir, bundle)| Output::Capture(dir, Box::new(bundle))),
    }
}

/// Only constructed from the gesture/menu's frontmost application, never
/// from the caller of a control request. Runs on the ordered output worker.
pub(crate) struct Request {
    pub id: Option<Id>,
    pub target: Target,
}

pub(crate) struct Target {
    pid: i32,
    focus: crate::accessibility::PasteFocus,
}
impl Target {
    pub(crate) fn capture() -> Option<Self> {
        let pid = crate::dictation::context::frontmost_pid()?;
        Some(Self {
            pid,
            focus: crate::accessibility::PasteFocus::capture(pid)?,
        })
    }
    pub(crate) fn in_front(&self) -> bool {
        crate::dictation::context::frontmost_pid() == Some(self.pid)
    }
    pub(crate) fn unchanged(&self) -> bool {
        self.in_front() && self.focus.unchanged()
    }
}

/// Labs › Paste Last picker (macOS): the picker, unified History, and the
/// words a deliberate cancel keeps. Off, every one of those is exactly as it
/// was before the lab.
pub(crate) fn enabled() -> bool {
    crate::labs::paste_last_picker()
}

pub(crate) fn current_rows(history: &History) -> Vec<Entry> {
    let settings = crate::app_settings::capture_settings();
    let roots = settings.enabled.then(|| settings.annotation_roots());
    let scope = roots
        .as_deref()
        .map(|r| Scope::in_roots(r, settings.bundle_days));
    let now = crate::history::now_ms();
    snapshot(history, scope.as_ref(), 8, now)
        .into_iter()
        .map(|row| Entry {
            detail: detail(history, scope.as_ref(), &row, now),
            row,
        })
        .collect()
}

/// The capture `id` names, while it is still what the picker listed, for
/// the editor it opens in.
pub(crate) fn capture_bundle(id: &Id) -> Option<PathBuf> {
    let Id::Capture(id) = id else {
        return None;
    };
    let settings = crate::app_settings::capture_settings();
    let roots = settings.enabled.then(|| settings.annotation_roots())?;
    Scope::in_roots(&roots, settings.bundle_days)
        .resolve(id, crate::history::now_ms())
        .map(|(dir, _)| dir)
}

impl Request {
    pub(crate) fn insert(
        self,
        paster: &mut super::Paster,
        history: Option<&History>,
    ) -> color_eyre::Result<String> {
        use color_eyre::eyre::eyre;
        let history = history.ok_or_else(|| eyre!("paste history is unavailable"))?;
        let settings = crate::app_settings::capture_settings();
        let roots = settings.enabled.then(|| settings.annotation_roots());
        let scope = roots
            .as_deref()
            .map(|r| Scope::in_roots(r, settings.bundle_days));
        let now = crate::history::now_ms();
        let id = self
            .id
            .or_else(|| latest(history, scope.as_ref(), now))
            .ok_or_else(|| eyre!("nothing to paste"))?;
        let output = resolve(history, scope.as_ref(), &id, crate::history::now_ms())
            .ok_or_else(|| eyre!("that item is no longer available"))?;
        if !self.target.unchanged() {
            return Err(eyre!("paste target changed"));
        }
        match output {
            Output::Text(text) => {
                paster.insert_recent_text(&text, &self.target)?;
                Ok(text)
            }
            Output::Capture(dir, bundle) => {
                let payload = super::capture::Payload::prepare(
                    &dir,
                    &bundle,
                    scrawl_capture::capture::is_frontmost_terminal(),
                    scrawl_capture::handoff::experiment(scrawl_capture::handoff::AGENT_CONTEXT),
                )?;
                paster.paste_capture(&payload, &self.target)?;
                Ok(String::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryDraft, HistoryKind, HistoryRetention, HistoryStore};
    use std::fs;

    #[test]
    fn mixed_tap_and_picker_revalidate_the_same_ready_item_without_promoting_it() {
        let root = std::env::temp_dir().join(format!("hex-paste-model-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let mut history = HistoryStore::open(root.join("history.json"), HistoryRetention::Day, 10);
        history
            .record(
                HistoryDraft {
                    kind: HistoryKind::Raw,
                    raw_text: "first".into(),
                    final_text: "first".into(),
                    application: None,
                    processing: None,
                    audio_ms: 0,
                    inference_ms: 0,
                    total_ms: 0,
                    recording: None,
                    model: None,
                },
                11,
            )
            .unwrap();
        let history = History::new(history);
        let capture = root.join("12.scrawl");
        fs::create_dir(&capture).unwrap();
        let manifest = r#"{"version":1,"id":"12","createdAtMs":12,"durationMs":1800,"target":{"mode":"display","displayId":1,"frame":{"x":0,"y":0,"width":40,"height":30,"scale":1}},"transcript":{"source":"hex","text":"move it down","segments":[],"words":[]},"strokes":[],"marks":[],"events":[],"keyframes":[],"contactSheet":"contact-sheet.png"}"#;
        fs::write(capture.join("scrawl.json"), manifest).unwrap();
        let scope = Scope::new(&root, Some(7));
        // A readable manifest, even with Hex as its speech source, is not
        // proof it came from Hex Annotate (standalone Scrawl shares the root).
        assert_eq!(snapshot(&history, Some(&scope), 8, 20).len(), 1);
        captures::mark_ready(&capture, 13).unwrap();
        let failed = root.join("14.scrawl");
        fs::create_dir(&failed).unwrap();
        fs::write(failed.join("scrawl.json"), "unfinished").unwrap();
        let picker = snapshot(&history, Some(&scope), 8, 20);
        assert_eq!(picker.len(), 2);
        assert_eq!(picker[0].preview, "move it down");
        assert_eq!(
            latest(&history, Some(&scope), 20),
            Some(picker[0].id.clone())
        );
        assert!(
            matches!(resolve(&history, Some(&scope), &picker[1].id, 20), Some(Output::Text(text)) if text == "first")
        );
        assert_eq!(
            latest(&history, Some(&scope), 20),
            Some(picker[0].id.clone())
        );
        fs::remove_dir_all(&capture).unwrap();
        assert!(resolve(&history, Some(&scope), &picker[0].id, 20).is_none());
        assert_eq!(
            latest(&history, Some(&scope), 20),
            Some(picker[1].id.clone())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hex_annotation_and_legacy_receipts_coexist_across_roots_while_rejecting_copies() {
        let tmp = std::env::temp_dir().join(format!("hex-paste-roots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let hex_root = tmp.join("annotations");
        let legacy_root = tmp.join("legacy");
        fs::create_dir_all(&hex_root).unwrap();
        fs::create_dir_all(&legacy_root).unwrap();

        let mut store = HistoryStore::open(tmp.join("history.json"), HistoryRetention::Day, 10);
        store
            .record(
                HistoryDraft {
                    kind: HistoryKind::Dictation,
                    raw_text: "dictated".into(),
                    final_text: "dictated".into(),
                    application: None,
                    processing: None,
                    audio_ms: 0,
                    inference_ms: 0,
                    total_ms: 0,
                    recording: None,
                    model: None,
                },
                20,
            )
            .unwrap();
        let history = History::new(store);

        // 1. Pre-cutover two-field `.scrawl` receipt in legacy root, plus an unmarked newer `.scrawl`.
        let old_marked = legacy_root.join("old-1.scrawl");
        fs::create_dir_all(&old_marked).unwrap();
        let mut b_old: ScrawlBundle = serde_json::from_slice(include_bytes!(
            "../../tests/fixtures/annotations/marked-v1.json"
        ))
        .unwrap();
        b_old.id = "old-1".into();
        b_old.transcript.text = "older scrawl receipt".into();
        b_old.keyframes.clear();
        scrawl_capture::bundle::publish(&old_marked, &b_old).unwrap();
        fs::write(
            old_marked.join("summary.md"),
            scrawl_capture::sketch::summary_markdown(&b_old, &old_marked),
        )
        .unwrap();
        fs::write(
            old_marked.join(".hex-annotate-v1"),
            r#"{"ready_at":15,"generation":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]}"#,
        )
        .unwrap();

        // Copying that legacy `.scrawl` (receipt and summary included) to `hex_root` must not resolve.
        let copied_legacy = hex_root.join("old-1.scrawl");
        fs::create_dir_all(&copied_legacy).unwrap();
        for name in ["scrawl.json", "summary.md", ".hex-annotate-v1"] {
            fs::copy(old_marked.join(name), copied_legacy.join(name)).unwrap();
        }

        let unmarked_legacy = legacy_root.join("unmarked.scrawl");
        fs::create_dir_all(&unmarked_legacy).unwrap();
        let mut b_unmarked = b_old.clone();
        b_unmarked.id = "unmarked".into();
        b_unmarked.transcript.text = "unmarked standalone capture".into();
        scrawl_capture::bundle::publish(&unmarked_legacy, &b_unmarked).unwrap();

        // 2. Fresh `.hexannotation` receipt, destination update, and a full copy with `.hex-annotate-v1`.
        let fresh = hex_root.join("fresh.hexannotation");
        fs::create_dir_all(fresh.join("frames")).unwrap();
        rune::render::write_png(&fresh.join("contact-sheet.png"), 4, 4, &[255; 64]).unwrap();
        rune::render::write_png(&fresh.join("frames/frame-01.png"), 4, 4, &[255; 64]).unwrap();
        let mut b_fresh = b_old.clone();
        b_fresh.id = "fresh-1".into();
        b_fresh.transcript.text = "fresh hex annotation".into();
        scrawl_capture::bundle::publish(&fresh, &b_fresh).unwrap();
        captures::mark_ready(&fresh, 25).unwrap();
        // Routing records a destination after `mark_ready`: receipt must stay valid.
        scrawl_capture::routing::record_destination(
            &fresh,
            &mut b_fresh,
            scrawl_capture::model::Destination {
                session: "ses_dest".into(),
                title: "Dest".into(),
                via: "chosen".into(),
                url: None,
                directory: None,
            },
        )
        .unwrap();

        let copied_hex = hex_root.join("copied.hexannotation");
        fs::create_dir_all(&copied_hex).unwrap();
        for name in ["annotation.json", "contact-sheet.png", ".hex-annotate-v1"] {
            fs::copy(fresh.join(name), copied_hex.join(name)).unwrap();
        }

        let roots = [hex_root.clone(), legacy_root.clone()];
        let scope = Scope::in_roots(&roots, Some(7));
        let rows = snapshot(&history, Some(&scope), 8, 30);
        let previews: Vec<_> = rows.iter().map(|r| r.preview.as_str()).collect();
        assert_eq!(
            previews,
            ["fresh hex annotation", "dictated", "older scrawl receipt"]
        );
        assert!(matches!(
            resolve(&history, Some(&scope), &rows[0].id, 30),
            Some(Output::Capture(dir, b)) if dir == fresh && b.destination.as_ref().is_some_and(|d| d.session == "ses_dest")
        ));
        assert!(matches!(
            resolve(&history, Some(&scope), &rows[2].id, 30),
            Some(Output::Capture(dir, _)) if dir == old_marked
        ));
        let d = detail(&history, Some(&scope), &rows[0], 30);
        assert_eq!(d.picture, Some(fresh.join("contact-sheet.png")));
        fs::remove_dir_all(tmp).unwrap();
    }
}
