//! Ownership of the surface between the stop command and final publication.
use super::*;

impl Capsule {
    pub(super) fn request_stop(&mut self) {
        if matches!(
            self.state,
            State::Recording {
                stopping: false,
                ..
            }
        ) && self.send(ControllerCommand::Stop)
        {
            self.mark_stopping();
        }
    }

    pub(super) fn mark_stopping(&mut self) {
        if let State::Recording { stopping, .. } = &mut self.state {
            *stopping = true;
            #[cfg(target_os = "windows")]
            self.fit_recording_window();
        }
    }

    pub(super) fn capture_stopped(&mut self) {
        if let State::Recording {
            recording, hovered, ..
        } = &self.state
        {
            let duration_ms = recording.started.elapsed().as_millis() as u64;
            let hovered = *hovered;
            self.state = State::Finalizing {
                duration_ms,
                hovered,
            };
            #[cfg(target_os = "windows")]
            if let Some(resize) = &self.resize {
                resize(panel::finalizing_window_size());
            }
        }
    }

    /// The controller accepted the recording: the offer's handoff ends and
    /// the recording takes the surface it left.
    pub(super) fn start_recording(&mut self, recording: Recording, tx: &mut Tx) {
        self.state = State::Recording {
            recording,
            hovered: false,
            stopping: false,
        };
        self.waveform = Some(Box::new(waveform::Listening::new(Instant::now())));
        self.live = Live::default();
        self.throw.stop();
        // Nothing here mounts with an entrance of its own: the surface
        // carries the change.
        tx.instant();
        self.stir(tx);
    }

    pub(super) fn published(&mut self, manifest: Box<MeetingManifest>) {
        self.live = Live::default();
        let hover = if self.hovered() {
            ReadyHover::Hovered
        } else {
            ReadyHover::Idle
        };
        self.state = State::Ready { manifest, hover };
    }
}

impl MeetingCapsules {
    pub(super) fn clear_finishing(&mut self) {
        if matches!(self.host, offer::Host::Finishing(_)) {
            self.host = offer::Host::Panel;
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.pill.handle.update(|c, tx| c.dismiss(tx));
        }
    }

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
        ) {
            self.clear_finishing();
        }
        if let Some(id) = self.pill.apply(event, inherit) {
            self.host = offer::Host::Finishing(id);
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
        if self.host.finishing_id() == Some(id) {
            &self.pill.handle
        } else {
            &self.panel
        }
    }

    pub(super) fn notes_progress(&self, id: &str) {
        let handle = self.handle_for(id);
        let id = id.to_owned();
        handle.update_if(move |c, _| {
            let State::Ready { manifest: m, .. } = &c.state else {
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

    pub(super) fn finish_meeting(&mut self, manifest: &MeetingManifest) {
        if self.host.finishing_id() != Some(manifest.id.as_str()) {
            self.clear_finishing();
            self.show_panel(Kind::Ready(Box::new(manifest.clone())), Some(READY_TIMEOUT));
            return;
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
        self.timeout(handle, generation, READY_TIMEOUT);
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
