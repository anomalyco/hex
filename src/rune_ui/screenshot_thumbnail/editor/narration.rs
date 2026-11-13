//! A reply belongs to its original shot and dictation, not the latest editor.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::{EDITOR, Editor, Listen};

pub(super) static OPEN: Mutex<Option<(u64, PathBuf)>> = Mutex::new(None);
pub(super) static GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Narration {
    pub(super) generation: u64,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) file: PathBuf,
    pub(super) cancelled: AtomicBool,
}

impl Narration {
    fn claimable(&self, open: Option<u64>) -> bool {
        !self.cancelled.load(Ordering::Acquire) && open == Some(self.generation)
    }
}

impl Editor {
    fn heard_from(&mut self, request: &Arc<Narration>, words: Option<String>, tx: &mut rune::Tx) {
        if !self.on
            || self.generation != request.generation
            || request.cancelled.load(Ordering::Acquire)
        {
            return;
        }
        if self
            .narration
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, request))
        {
            self.heard(words, tx);
            self.narration = None;
        } else if let Some(words) = words.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            // An older accepted dictation can append to its own shot, but
            // cannot finish a newer dictation or release its waiting send.
            if !self.words.is_empty() {
                self.words.push(' ');
            }
            self.words.push_str(words);
        }
    }
}

/// Snapshot the shot when dictation starts. A reopened shot is a new editor.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn narration() -> Option<Arc<Narration>> {
    let open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    let (generation, file) = open.as_ref()?;
    Some(Arc::new(Narration {
        generation: *generation,
        file: file.clone(),
        cancelled: AtomicBool::new(false),
    }))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn can_claim(request: &Narration) -> bool {
    request.claimable(
        OPEN.lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(id, _)| *id),
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn listen(request: Arc<Narration>, listen: Listen) {
    if let Some(overlay) = EDITOR.get() {
        overlay.update(move |s, tx| {
            if s.generation != request.generation
                || !s.on
                || request.cancelled.load(Ordering::Acquire)
            {
                return;
            }
            if listen == Listen::Recording {
                s.narration = Some(request);
            } else if !s
                .narration
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &request))
            {
                return;
            }
            s.listen(listen, tx);
            tx.animated();
        });
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn heard(request: Arc<Narration>, words: Option<String>) {
    if let Some(overlay) = EDITOR.get() {
        overlay.update(move |s, tx| {
            if request.cancelled.load(Ordering::Acquire) {
                return;
            }
            // Accepted words remain attached to their original saved shot
            // even if the editor closed or changed after release.
            #[cfg(target_os = "macos")]
            if let Some(words) = &words {
                crate::screenshots::append_words(request.file.clone(), words.clone());
            }
            s.heard_from(&request, words, tx);
            tx.animated();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::Editor;
    use super::*;

    fn request(generation: u64) -> Arc<Narration> {
        Arc::new(Narration {
            generation,
            file: "shot.png".into(),
            cancelled: AtomicBool::new(false),
        })
    }

    #[test]
    fn closing_or_replacing_the_shot_before_release_returns_to_normal_paste() {
        let request = request(1);
        assert!(request.claimable(Some(1)));
        assert!(!request.claimable(None));
        assert!(!request.claimable(Some(2)));
    }

    #[test]
    fn an_old_reply_cannot_finish_new_narration_or_land_on_a_new_shot() {
        let old = request(1);
        let new = request(1);
        let mut editor = Editor {
            on: true,
            generation: 1,
            narration: Some(new.clone()),
            listen: Listen::Recording,
            ..Editor::default()
        };
        let mut tx = rune::Tx::default();
        editor.heard_from(&old, Some("First.".into()), &mut tx);
        assert_eq!(editor.words, "First.");
        assert_eq!(editor.listen, Listen::Recording);
        assert!(Arc::ptr_eq(editor.narration.as_ref().unwrap(), &new));
        editor.generation = 2;
        editor.words.clear();
        editor.heard_from(&old, Some("Stale.".into()), &mut tx);
        assert!(editor.words.is_empty());
    }

    #[test]
    fn cancelling_a_request_fences_its_late_reply_after_a_new_start() {
        let old = request(1);
        let mut editor = Editor {
            on: true,
            generation: 1,
            narration: Some(old.clone()),
            listen: Listen::Transcribing,
            ..Editor::default()
        };
        assert!(editor.cancel_listening());
        let new = request(1);
        editor.narration = Some(new);
        editor.listen = Listen::Transcribing;
        editor.heard_from(&old, Some("Cancelled.".into()), &mut rune::Tx::default());
        assert!(editor.words.is_empty());
        assert_eq!(editor.listen, Listen::Transcribing);
    }
}
