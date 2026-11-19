//! Ownership of the surface between the stop command and final publication.
use super::*;

impl Capsule {
    pub(super) fn request_stop(&mut self) {
        if !self.stopping
            && matches!(self.kind, Some(Kind::Recording(_)))
            && self.send(ControllerCommand::Stop)
        {
            self.mark_stopping();
        }
    }

    pub(super) fn mark_stopping(&mut self) {
        if matches!(self.kind, Some(Kind::Recording(_))) {
            self.stopping = true;
            #[cfg(target_os = "windows")]
            self.fit_recording_window();
        }
    }

    pub(super) fn capture_stopped(&mut self) {
        if let Some(Kind::Recording(r)) = &self.kind {
            self.kind = Some(Kind::Finalizing {
                duration_ms: r.started.elapsed().as_millis() as u64,
            });
            #[cfg(target_os = "windows")]
            if let Some(resize) = &self.resize {
                resize(panel::finalizing_window_size());
            }
        }
    }

    /// The controller accepted the recording: the offer's handoff ends and
    /// the recording takes the surface it left.
    pub(super) fn start_recording(&mut self, recording: Recording, tx: &mut Tx) {
        self.kind = Some(Kind::Recording(recording));
        self.waveform = Some(Box::new(waveform::Listening::new(Instant::now())));
        self.live = Live::default();
        self.hovered = false;
        self.stopping = false;
        self.expired = false;
        self.throw.stop();
        // Nothing here mounts with an entrance of its own: the surface
        // carries the change.
        tx.instant();
        self.stir(tx);
    }

    pub(super) fn published(&mut self, manifest: Box<MeetingManifest>) {
        self.live = Live::default();
        self.kind = Some(Kind::Ready(manifest));
    }
}

impl MeetingCapsules {
    pub(super) fn advance_handoff(&mut self, event: &ControllerEvent, inherit: bool) {
        // A new panel supersedes an old summary, and failure yields to the
        // host's failure UI. Notes never take ownership or show a window.
        if matches!(
            event,
            ControllerEvent::Starting(_)
                | ControllerEvent::RecordingStarted { .. }
                | ControllerEvent::Reminder { .. }
                | ControllerEvent::Offer(..)
                | ControllerEvent::Failed { .. }
        ) && self.finishing.take().is_some()
        {
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.pill.handle.update(|c, tx| c.dismiss(tx));
        }
        if let Some(id) = self.pill.apply(event, inherit) {
            self.finishing = Some(id);
            if matches!(event, ControllerEvent::Transcribing) {
                self.pill.handle.update(|c, tx| {
                    c.capture_stopped();
                    c.stir(tx);
                });
            }
        }
    }

    /// The capsule a finished meeting still owns: the recording's own
    /// window while it is finishing there, else the panel.
    pub(super) fn handle_for(&self, id: &str) -> &OverlayHandle<Capsule> {
        if self.finishing.as_deref() == Some(id) {
            &self.pill.handle
        } else {
            &self.panel
        }
    }

    pub(super) fn notes_progress(&self, id: &str) {
        let handle = self.handle_for(id);
        let id = id.to_owned();
        handle.update_if(move |c, _| {
            let Some(Kind::Ready(m)) = &c.kind else {
                return false;
            };
            #[cfg(target_os = "windows")]
            if m.id == id
                && let Some(resize) = &c.resize
            {
                resize(ready_window_size(m));
            }
            m.id == id
        });
    }

    pub(super) fn finish_meeting(&mut self, manifest: &MeetingManifest) -> OverlayHandle<Capsule> {
        if self.finishing.as_ref() != Some(&manifest.id) {
            self.show_panel(Kind::Ready(Box::new(manifest.clone())), Some(READY_TIMEOUT));
            return self.panel.clone();
        }
        let handle = self.pill.handle.clone();
        let finished = Box::new(manifest.clone());
        #[cfg(target_os = "windows")]
        handle.resize(ready_window_size(&finished));
        handle.update(move |c, tx| {
            c.published(finished);
            c.stir(tx);
        });
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.timeout(handle.clone(), generation, READY_TIMEOUT);
        handle
    }
}

impl Pill {
    fn apply(&self, event: &ControllerEvent, inherit: bool) -> Option<String> {
        // Relinquish the visible pill under the same lock as the ticker, so
        // its last reconciliation cannot hide or restore a finalizing surface.
        let mut model = self.lock();
        let retain = match event {
            ControllerEvent::Transcribing => model.shown.take(),
            ControllerEvent::Finished(m) if model.shown.as_ref() == Some(&m.id) => {
                model.shown.take()
            }
            _ => None,
        };
        model.apply(event, Instant::now());
        self.reconcile_locked(&mut model, inherit);
        drop(model);
        if let ControllerEvent::RecordingStarted { id, .. } = event {
            self.tick(id.clone());
        }
        retain
    }
}
