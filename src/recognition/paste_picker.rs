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
use std::sync::mpsc::{self, SyncSender};
use std::time::{Duration, Instant};

struct Invocation {
    epoch: u64,
    binding: RuntimeHotkey,
    target: Target,
    check: Instant,
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
        self.active = Some(Invocation {
            epoch: browse::begin(),
            binding,
            target,
            check: Instant::now(),
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
        if browse::open() {
            if let InputEvent::Flags(flags) = event
                && let Some(inv) = &self.active
                && !inv.binding.modifiers.is_empty()
                && !inv.binding.required_modifiers_down(flags)
            {
                browse::nav(Nav::Commit);
            }
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
                let epoch = self.active.as_ref().unwrap().epoch;
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
                if self.reader.as_ref().unwrap().try_send(epoch).is_err() {
                    self.cancel();
                    return Err("paste list is busy");
                }
                Ok(())
            }
        }
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
