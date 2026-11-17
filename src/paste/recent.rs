//! One selection rule for an immediate paste and the open picker. Only the
//! stable ID crosses the selection boundary, never a payload or a row index.

use crate::capture_host::paste_source::{self as captures, Scope};
use crate::history::{History, paste_source as dictations};
use scrawl_capture::model::ScrawlBundle;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Id {
    Capture(captures::Id),
    Dictation(dictations::Id),
}

#[derive(Clone)]
pub(crate) struct Row {
    pub id: Id,
    pub ready_at: u64,
    pub preview: String,
}

pub(crate) fn snapshot(
    history: &History,
    captures: Option<&Scope<'_>>,
    limit: usize,
    now: u64,
) -> Vec<Row> {
    let mut rows: Vec<_> = history
        .paste_rows(limit, now)
        .into_iter()
        .map(|r| Row {
            id: Id::Dictation(r.id),
            ready_at: r.ready_at,
            preview: r.preview,
        })
        .collect();
    if let Some(scope) = captures {
        rows.extend(
            scope
                .rows(limit, now)
                .into_iter()
                .map(|(id, ready_at, preview)| Row {
                    id: Id::Capture(id),
                    ready_at,
                    preview,
                }),
        );
    }
    rows.sort_unstable_by(|a, b| b.ready_at.cmp(&a.ready_at).then_with(|| b.id.cmp(&a.id)));
    rows.truncate(limit);
    rows
}

pub(crate) fn latest(history: &History, captures: Option<&Scope<'_>>, now: u64) -> Option<Id> {
    snapshot(history, captures, 1, now).pop().map(|r| r.id)
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

/// Developer launch override, like HEX_EXPERIMENTAL_MIC. Never a new
/// preference, global binding, stable-build feature or managed-Labs bypass.
pub(crate) fn enabled() -> bool {
    static ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
        crate::dev_build::is_dev_build()
            && std::env::var_os("HEX_EXPERIMENTAL_PASTE_LAST").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            && !crate::policy::Policy::current().locks(crate::policy::Lock::Labs)
    });
    *ENABLED
}

pub(crate) fn current_rows(history: &History) -> Vec<Row> {
    let settings = crate::app_settings::capture_settings();
    let root = settings.enabled.then(|| settings.captures_dir());
    let scope = root.as_ref().map(|p| Scope::new(p, settings.bundle_days));
    snapshot(history, scope.as_ref(), 8, crate::history::now_ms())
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
        let root = settings.enabled.then(|| settings.captures_dir());
        let scope = root.as_ref().map(|p| Scope::new(p, settings.bundle_days));
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
}
