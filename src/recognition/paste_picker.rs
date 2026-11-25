//! Listener side of the existing Paste Last binding. A library scan never
//! blocks a delivered shortcut edge. One bounded reader serves every hold.
use crate::app_settings::RuntimeHotkey;
use crate::audio::CaptureInstant;
use crate::history::History;
use crate::parakeet::DictationWorker;
use crate::paste::{
    browse::{self, Choice, Nav},
    gesture::{Effect, Gesture},
    recent::{self, Request, Target},
};
use crate::suppression::InputEvent;
use crate::transcription::engine::TranscriptionEngine;
use std::sync::{
    Arc,
    mpsc::{self, SyncSender},
};
use std::time::{Duration, Instant};

/// Labs › Paste Last picker as the listener holds it: the picker and the
/// cancel path's word writer exist only while the lab is on. `sync` runs on
/// every listener pass, so the switch applies without a restart, and turning
/// it off drops both (closing an open picker), leaving Paste Last and Cancel
/// exactly as they are without the lab.
pub(super) struct Lab {
    history: Option<History>,
    engine: Option<Arc<TranscriptionEngine>>,
    picker: Option<Picker>,
    writer: Option<super::cancelled_words::Writer>,
}

impl Lab {
    pub fn new(history: Option<History>, engine: Option<Arc<TranscriptionEngine>>) -> Self {
        Self {
            history,
            engine,
            picker: None,
            writer: None,
        }
    }
    pub fn sync(&mut self, on: bool) {
        if on {
            if self.picker.is_none() {
                self.picker = Some(Picker::new(self.history.clone()));
            }
        } else {
            self.picker = None;
            self.writer = None;
        }
    }
    pub fn busy(&self) -> bool {
        self.picker.as_ref().is_some_and(Picker::busy)
    }
    pub fn picker(&mut self) -> Option<&mut Picker> {
        self.picker.as_mut()
    }
    /// Started on the first deliberate cancel after the lab turns on.
    pub fn writer(&mut self) -> Option<&super::cancelled_words::Writer> {
        self.picker.as_ref()?;
        if self.writer.is_none() {
            let history = self.history.clone()?;
            let engine = self.engine.clone()?;
            self.writer = super::cancelled_words::Writer::start(history, engine);
        }
        self.writer.as_ref()
    }
}

struct Invocation {
    epoch: u64,
    binding: RuntimeHotkey,
    target: Target,
    check: Instant,
    /// The rows are being read for this invocation.
    reading: bool,
}
pub(super) struct Picker {
    hold: Gesture,
    active: Option<Invocation>,
    history: Option<History>,
    reader: Option<SyncSender<u64>>,
}

impl Picker {
    pub fn new(history: Option<History>) -> Self {
        Self {
            hold: Default::default(),
            active: None,
            history,
            reader: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }
    pub fn press(
        &mut self,
        binding: RuntimeHotkey,
        at: CaptureInstant,
        worker: &DictationWorker,
    ) -> Result<(), &'static str> {
        if self.busy() {
            return Ok(());
        }
        let target = Target::capture().ok_or("could not confirm the paste target")?;
        let can_hold = browse::available()
            && !crate::suppression::secure_input::enabled()
            && binding.key_code.is_some();
        if !can_hold {
            return worker.paste_recent(Request { id: None, target });
        }
        let epoch = browse::begin();
        // Read the rows while the key is held, so the picker opens the
        // moment the hold passes; a tap leaves them unread.
        let reading = self.read(epoch).is_ok();
        self.active = Some(Invocation {
            epoch,
            binding,
            target,
            check: Instant::now(),
            reading,
        });
        self.hold.press(binding, at, true);
        Ok(())
    }
    pub fn edge(
        &mut self,
        event: InputEvent,
        at: CaptureInstant,
        worker: &DictationWorker,
    ) -> Result<(), &'static str> {
        if self.active.is_none() {
            return Ok(());
        }
        // Open, the picker outlives the keys: the session tap's keys, a
        // click on a row, or a click outside it end it.
        if browse::open() {
            if matches!(event, InputEvent::TapDisabled) {
                self.cancel();
            }
            return Ok(());
        }
        let effect = self.hold.edge(event, at);
        self.apply(effect, worker)
    }
    pub fn tick(&mut self, paused: bool, worker: &DictationWorker) -> Result<(), &'static str> {
        let Some(inv) = self.active.as_mut() else {
            return Ok(());
        };
        if paused
            || crate::suppression::secure_input::enabled()
            || crate::app_settings::runtime_hotkeys().paste_last != Some(inv.binding)
        {
            self.cancel();
            return Ok(());
        }
        if Instant::now() >= inv.check {
            inv.check = Instant::now() + Duration::from_millis(120);
            if !inv.target.unchanged() {
                self.cancel();
                return Ok(());
            }
        }
        if let Some(choice) = browse::take(inv.epoch) {
            let inv = self.active.take().unwrap();
            self.hold.cancel();
            return match choice {
                Choice::Cancel => Ok(()),
                Choice::Item(id) => worker.paste_recent(Request {
                    id: Some(id),
                    target: inv.target,
                }),
                Choice::Open(id) => {
                    if let Some(bundle) = recent::capture_bundle(&id) {
                        crate::rune_ui::paste_picker::open_in_editor(&bundle);
                    }
                    Ok(())
                }
            };
        }
        let effect = self.hold.tick(CaptureInstant::now());
        self.apply(effect, worker)
    }
    fn apply(&mut self, effect: Effect, worker: &DictationWorker) -> Result<(), &'static str> {
        match effect {
            Effect::Wait => Ok(()),
            Effect::Cancel => {
                self.cancel();
                Ok(())
            }
            Effect::PasteLatest => {
                let inv = self.active.take().unwrap();
                if !browse::tap(inv.epoch) {
                    return Ok(());
                }
                worker.paste_recent(Request {
                    id: None,
                    target: inv.target,
                })
            }
            Effect::Browse => {
                let inv = self.active.as_ref().unwrap();
                let (epoch, reading) = (inv.epoch, inv.reading);
                browse::reveal(epoch);
                if !reading && let Err(error) = self.read(epoch) {
                    self.cancel();
                    return Err(error);
                }
                Ok(())
            }
        }
    }
    /// Asks the one bounded reader for this invocation's rows.
    fn read(&mut self, epoch: u64) -> Result<(), &'static str> {
        if self.reader.is_none() {
            let (tx, rx) = mpsc::sync_channel(1);
            let history = self.history.clone();
            std::thread::Builder::new()
                .name("paste-choices".into())
                .spawn(move || {
                    while let Ok(epoch) = rx.recv() {
                        let rows = history
                            .as_ref()
                            .map(recent::current_rows)
                            .unwrap_or_default();
                        browse::show(epoch, rows);
                    }
                })
                .map_err(|_| "paste list is unavailable")?;
            self.reader = Some(tx);
        }
        self.reader
            .as_ref()
            .unwrap()
            .try_send(epoch)
            .map_err(|_| "paste list is busy")
    }
    pub fn cancel(&mut self) {
        if self.active.take().is_some() {
            browse::nav(Nav::Cancel);
        }
        self.hold.cancel();
    }
}
impl Drop for Picker {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paste::gesture::Effect;

    #[test]
    fn toggling_the_lab_after_start_changes_what_holding_paste_last_does() {
        let binding = RuntimeHotkey {
            modifiers: Default::default(),
            key_code: Some(9),
        };
        let at = |ms| CaptureInstant::from_nanos(ms * 1_000_000u64);
        // Off at listener start: no picker, so Paste Last repastes.
        let mut lab = Lab::new(None, None);
        lab.sync(false);
        assert!(lab.picker().is_none() && lab.writer().is_none());
        // Turned on later: the next pass builds the picker, and a hold browses.
        lab.sync(true);
        let picker = lab.picker().expect("picker once the lab is on");
        assert_eq!(picker.hold.press(binding, at(0), true), Effect::Wait);
        assert_eq!(picker.hold.tick(at(400)), Effect::Browse);
        // Off again mid-hold: the picker and its pending hold are gone.
        lab.sync(false);
        assert!(lab.picker().is_none() && !lab.busy());
        // On once more: a fresh gesture, nothing left of the old hold.
        lab.sync(true);
        assert_eq!(
            lab.picker().unwrap().hold.press(binding, at(600), false),
            Effect::PasteLatest
        );
    }
}
