//! The listener's side of Labs › Paste picker: the pending hold on Paste last
//! dictation, the open list's commits, and their pastes.

use std::time::Duration;

use color_eyre::Result;

use super::emit_engine_state;
use crate::audio::CaptureInstant;
use crate::desktop::events::{DictationPhase, EventLog, now_ms};
use crate::dictation::audio::DictationAudio;
use crate::feedback::{self, Tone};
use crate::history::History;
use crate::parakeet::DictationWorker;
use crate::paste_picker::{HoldOutcome, PendingHold, PickerItem};
use crate::suppression::InputEvent;

pub(super) struct Picker {
    hold: PendingHold,
    history: Option<History>,
}

impl Picker {
    pub(super) fn new(history: Option<History>) -> Self {
        Self {
            hold: PendingHold::default(),
            history,
        }
    }

    /// Whether a Paste last press is still deciding between a tap and a hold.
    pub(super) fn holding(&self) -> bool {
        self.hold.is_active()
    }

    /// Drops a pending hold and closes the list while shortcuts are paused or
    /// the Labs switch is off.
    pub(super) fn sync(&mut self, paused: bool) {
        if paused || !crate::labs::paste_picker() {
            self.hold.cancel();
            self.dismiss();
        }
    }

    /// Closes the list without pasting, if it shows.
    pub(super) fn dismiss(&self) {
        if crate::paste_picker::is_open() {
            crate::paste_picker::close();
        }
    }

    /// Takes over a Paste last press when the picker is on and the binding
    /// has a key; true when it did, so the press pastes nothing yet.
    pub(super) fn on_paste_last(
        &mut self,
        at: CaptureInstant,
        worker: &DictationWorker,
        input: &DictationAudio,
        events: &mut EventLog,
    ) -> Result<bool> {
        if !crate::labs::paste_picker() {
            return Ok(false);
        }
        let Some(binding) = crate::app_settings::runtime_hotkeys()
            .paste_last
            .filter(|binding| binding.key_code.is_some())
        else {
            return Ok(false);
        };
        feedback::retract_start();
        input.cancel()?;
        worker.prepare_paste();
        self.hold.begin(binding, at);
        emit_engine_state(events, false, worker, &input.device_name())?;
        Ok(true)
    }

    /// Feeds one input event to a pending hold; true when it was Escape,
    /// which the hold keeps from cancelling anything else.
    pub(super) fn on_event(
        &mut self,
        event: InputEvent,
        at: CaptureInstant,
        worker: &DictationWorker,
        input: &DictationAudio,
        events: &mut EventLog,
    ) -> Result<bool> {
        if !self.hold.is_active() {
            return Ok(false);
        }
        let outcome = match event {
            InputEvent::Key { code, down, flags } => {
                self.hold
                    .on_key_or_flags(Some((code, down)), Some(flags), at)
            }
            InputEvent::Flags(flags) => self.hold.on_key_or_flags(None, Some(flags), at),
            InputEvent::PhysicalKeyDown { .. } => HoldOutcome::None,
            InputEvent::MouseDown | InputEvent::TapDisabled => {
                self.hold.cancel();
                HoldOutcome::None
            }
        };
        self.apply(outcome, worker, &input.device_name(), events)?;
        Ok(event.is_escape_down())
    }

    /// Opens the list once a hold passes the threshold, and pastes whatever
    /// the open list committed.
    pub(super) fn tick(
        &mut self,
        worker: &DictationWorker,
        input: &DictationAudio,
        events: &mut EventLog,
    ) -> Result<()> {
        let device = input.device_name();
        if self.hold.is_active() {
            let outcome = self.hold.poll(CaptureInstant::now());
            self.apply(outcome, worker, &device, events)?;
        }
        while let Some(texts) = crate::paste_picker::take_commit() {
            self.apply(HoldOutcome::Commit(texts), worker, &device, events)?;
        }
        Ok(())
    }

    /// How long the listener may wait for audio: short while a hold is
    /// pending, so a tap pastes as soon as its key comes up.
    pub(super) fn wait(&self) -> Duration {
        if self.hold.is_active() {
            Duration::from_millis(2)
        } else {
            Duration::from_millis(20)
        }
    }

    fn apply(
        &self,
        outcome: HoldOutcome,
        worker: &DictationWorker,
        device: &str,
        events: &mut EventLog,
    ) -> Result<()> {
        match outcome {
            HoldOutcome::None => {}
            HoldOutcome::TapPaste => {
                if let Err(error) = worker.paste_last() {
                    feedback::play(Tone::Error);
                    events.dictation(DictationPhase::Failed(error.into()), "")?;
                }
                emit_engine_state(events, false, worker, device)?;
            }
            HoldOutcome::OpenPicker { binding } => {
                worker.prepare_paste();
                let items = self
                    .history
                    .as_ref()
                    .map(|store| store.recent_pasted_dictations(crate::paste_picker::MAX_ITEMS))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|entry| PickerItem {
                        id: entry.id,
                        text: entry.final_text,
                        timestamp_ms: entry.timestamp_ms,
                    })
                    .collect();
                let anchor = crate::paste_picker::resolve_anchor();
                let v_key_code = crate::keyboard::key_code_for('v').ok();
                crate::paste_picker::open(binding, v_key_code, items, now_ms(), anchor);
            }
            HoldOutcome::Commit(texts) => {
                let combined = crate::paste::join_sequence(texts.iter().map(String::as_str));
                if !combined.is_empty() {
                    if let Err(error) = worker.paste_picked(combined) {
                        feedback::play(Tone::Error);
                        events.dictation(DictationPhase::Failed(error.into()), "")?;
                    }
                    emit_engine_state(events, false, worker, device)?;
                }
            }
        }
        Ok(())
    }
}
