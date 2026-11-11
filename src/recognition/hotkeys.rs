//! The dictation shortcut machines: dictation, Labs › Second dictation
//! shortcut while it is on and bound, Dictate without processing while it is
//! bound, Labs › Caps Lock dictation's hands-free toggle, and Labs › More
//! dictation shortcuts' rows while that Lab is on.

use crate::app_settings::MAX_EXTRA_DICTATION_SHORTCUTS;
use crate::audio::CaptureInstant;
use crate::suppression::{DictationHotkey, HotkeyAction, InputEvent};

/// One of the dictation shortcut machines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Shortcut {
    Dictation,
    /// Labs › Second dictation shortcut: dictates as the dictation shortcut
    /// does.
    Second,
    /// Dictate without processing.
    Raw,
    /// Labs › Caps Lock dictation: a press toggles a hands-free capture.
    CapsLock,
    /// Labs › More dictation shortcuts, by row.
    Extra(usize),
}

impl Shortcut {
    /// Every machine in matching order: the fixed shortcuts first, then the
    /// extra dictation rows.
    fn all() -> impl Iterator<Item = Self> {
        const FIXED: [Shortcut; 4] = [
            Shortcut::Dictation,
            Shortcut::Second,
            Shortcut::Raw,
            Shortcut::CapsLock,
        ];
        FIXED
            .into_iter()
            .chain((0..MAX_EXTRA_DICTATION_SHORTCUTS).map(Shortcut::Extra))
    }
}

/// The dictation shortcut machines: dictation, Labs › Second dictation
/// shortcut while it is on and bound, Dictate without processing while it is
/// bound, Labs › More dictation shortcuts' rows, and Labs › Caps Lock
/// dictation's machine. One records at a time, and the one that started a
/// capture finishes or cancels it.
pub(super) struct Hotkeys {
    pub(super) dictation: DictationHotkey,
    pub(super) second: Option<DictationHotkey>,
    pub(super) raw: Option<DictationHotkey>,
    /// Labs › More dictation shortcuts: each bound row's own machine, with
    /// its own activation mode.
    pub(super) extra: [Option<DictationHotkey>; MAX_EXTRA_DICTATION_SHORTCUTS],
    /// Labs › Caps Lock dictation's hands-free toggle. Its binding is empty:
    /// presses arrive from the caps-lock worker, not the shortcut matcher.
    pub(super) caps_lock: DictationHotkey,
    /// The recording (or the last) skips processing: the raw shortcut or
    /// `hex dictation start --raw` started it.
    pub(super) raw_capture: bool,
    /// Whether the listener last saw shortcuts suspended, so that a capture
    /// a command starts during a pause is left alone.
    pub(super) suspended: bool,
}

impl Hotkeys {
    /// A listener's machines: the dictation shortcut and the Caps Lock
    /// machine, with no optional shortcut bound until `configure`.
    pub(super) fn new(dictation: DictationHotkey, caps_lock: DictationHotkey) -> Self {
        Self {
            dictation,
            second: None,
            raw: None,
            extra: std::array::from_fn(|_| None),
            caps_lock,
            raw_capture: false,
            suspended: false,
        }
    }

    /// The fixed shortcuts' machines, in matching order. The extra rows have
    /// their own trigger modes and are configured separately.
    fn fixed_machines(&mut self) -> impl Iterator<Item = &mut DictationHotkey> {
        std::iter::once(&mut self.dictation)
            .chain(self.second.as_mut())
            .chain(self.raw.as_mut())
            .chain(Some(&mut self.caps_lock))
    }

    fn machines(&mut self) -> impl Iterator<Item = &mut DictationHotkey> {
        std::iter::once(&mut self.dictation)
            .chain(self.second.as_mut())
            .chain(self.raw.as_mut())
            .chain(Some(&mut self.caps_lock))
            .chain(self.extra.iter_mut().flatten())
    }

    fn machine(&self, shortcut: Shortcut) -> Option<&DictationHotkey> {
        match shortcut {
            Shortcut::Dictation => Some(&self.dictation),
            Shortcut::Second => self.second.as_ref(),
            Shortcut::Raw => self.raw.as_ref(),
            Shortcut::CapsLock => Some(&self.caps_lock),
            Shortcut::Extra(index) => self.extra.get(index).and_then(|machine| machine.as_ref()),
        }
    }

    fn machine_mut(&mut self, shortcut: Shortcut) -> Option<&mut DictationHotkey> {
        match shortcut {
            Shortcut::Dictation => Some(&mut self.dictation),
            Shortcut::Second => self.second.as_mut(),
            Shortcut::Raw => self.raw.as_mut(),
            Shortcut::CapsLock => Some(&mut self.caps_lock),
            Shortcut::Extra(index) => self
                .extra
                .get_mut(index)
                .and_then(|machine| machine.as_mut()),
        }
    }

    /// Whether a machine other than `shortcut`'s records.
    fn recording_besides(&self, shortcut: Shortcut) -> bool {
        Shortcut::all()
            .filter(|other| *other != shortcut)
            .filter_map(|other| self.machine(other))
            .any(DictationHotkey::is_recording)
    }

    /// Whether a machine other than `shortcut`'s has a double-tap gesture
    /// waiting to become a recording.
    fn pending_gesture_besides(&self, shortcut: Shortcut) -> bool {
        Shortcut::all()
            .filter(|other| *other != shortcut)
            .filter_map(|other| self.machine(other))
            .any(DictationHotkey::has_pending_gesture)
    }

    pub(super) fn any_recording(&self) -> bool {
        Shortcut::all()
            .filter_map(|shortcut| self.machine(shortcut))
            .any(DictationHotkey::is_recording)
    }

    /// Whether one of the Labs › More dictation shortcuts records.
    pub(super) fn extra_recording(&self) -> bool {
        self.extra
            .iter()
            .flatten()
            .any(DictationHotkey::is_recording)
    }

    pub(super) fn suppresses_recognition(&self) -> bool {
        Shortcut::all()
            .filter_map(|shortcut| self.machine(shortcut))
            .any(DictationHotkey::suppresses_recognition)
    }

    /// Stands `shortcut`'s machine down, as when its start was refused.
    pub(super) fn suspend(&mut self, shortcut: Shortcut) {
        match self.machine_mut(shortcut) {
            Some(machine) => machine.suspend(),
            None => self.dictation.suspend(),
        };
    }

    /// Suspends every machine (never stopping at the first, as `any`
    /// would) and reports whether one was recording.
    pub(super) fn suspend_all(&mut self) -> bool {
        self.machines().filter_map(DictationHotkey::suspend).count() > 0
    }

    /// Suspends every machine when shortcuts become suspended, once per
    /// suspension: the capture under way is cancelled, while a capture a
    /// command starts during the same suspension keeps recording. Reports
    /// whether one was recording when the suspension began.
    pub(super) fn suspend_when(&mut self, suspended: bool) -> bool {
        if std::mem::replace(&mut self.suspended, suspended) == suspended {
            return false;
        }
        suspended && self.suspend_all()
    }

    pub(super) fn disarm_pending_gestures(&mut self) {
        self.machines()
            .for_each(DictationHotkey::disarm_pending_gesture);
    }

    pub(super) fn track_key_state(&mut self, event: InputEvent, at: CaptureInstant) {
        for machine in self.machines() {
            machine.track_key_state(event, at);
        }
    }

    pub(super) fn recover_stale_keys(&mut self) {
        self.machines()
            .for_each(DictationHotkey::recover_stale_keys);
    }

    pub(super) fn break_chords(&mut self, now: CaptureInstant) {
        for machine in self.machines() {
            machine.break_chord(now);
        }
    }

    /// Applies the saved shortcut options. Unbinding the raw or second
    /// shortcut (or turning the second's Labs toggle off) while it records
    /// reports true: the caller cancels that recording. The same goes for an
    /// extra shortcut whose row disappears.
    pub(super) fn configure(&mut self, now: CaptureInstant) -> bool {
        let hotkeys = crate::app_settings::runtime_hotkeys();
        let raw_cancelled = rebind(&mut self.raw, hotkeys.raw_dictation, now);
        let second_cancelled = rebind(&mut self.second, hotkeys.second_dictation, now);
        let extra_cancelled = self.rebind_extra(hotkeys.extra_dictation, now);
        self.dictation.set_binding(hotkeys.dictation);
        for machine in self.fixed_machines() {
            machine.set_double_tap_enabled(crate::app_settings::double_tap_lock());
            machine.set_double_tap_only(crate::app_settings::double_tap_only());
            machine.set_toggle(crate::app_settings::shortcut_toggle());
            machine.set_cancel(hotkeys.cancel);
        }
        // Caps Lock is hands-free by definition: its own machine always
        // treats a press as Toggle, and Escape cancels it in every mode.
        self.caps_lock
            .set_binding(crate::app_settings::RuntimeHotkey::default());
        self.caps_lock.set_double_tap_enabled(false);
        self.caps_lock.set_double_tap_only(false);
        self.caps_lock.set_toggle(true);
        self.caps_lock.set_cancel(Some(
            crate::app_settings::HotkeyBinding::cancel_default().runtime(),
        ));
        // Each extra row triggers its own way and keeps its own cancel key:
        // the configured one if Toggle mode arms it, else Escape.
        let extra_cancel = hotkeys
            .cancel
            .unwrap_or_else(|| crate::app_settings::HotkeyBinding::cancel_default().runtime());
        for (index, machine) in self.extra.iter_mut().enumerate() {
            let Some(machine) = machine else {
                continue;
            };
            machine.set_trigger(hotkeys.extra_dictation_modes[index]);
            machine.set_cancel(Some(extra_cancel));
            machine.set_cancel_in_any_mode(true);
        }
        raw_cancelled || second_cancelled || extra_cancelled
    }

    /// Rebinds the extra rows by binding identity, so clearing or adding one
    /// row keeps every other row's machine, including a capture in progress.
    /// Returns whether a dropped machine was recording.
    pub(super) fn rebind_extra(
        &mut self,
        bindings: [Option<crate::app_settings::RuntimeHotkey>; MAX_EXTRA_DICTATION_SHORTCUTS],
        now: CaptureInstant,
    ) -> bool {
        let mut existing = std::mem::take(&mut self.extra);
        let mut rebound: [Option<DictationHotkey>; MAX_EXTRA_DICTATION_SHORTCUTS] =
            std::array::from_fn(|_| None);
        for (index, binding) in bindings.into_iter().enumerate() {
            let Some(binding) = binding else {
                continue;
            };
            let matched = existing
                .iter()
                .position(|machine| machine.as_ref().is_some_and(|m| m.binding() == binding));
            rebound[index] = Some(match matched {
                Some(position) => existing[position].take().expect("matched a machine"),
                None => DictationHotkey::new(now, crate::app_settings::double_tap_lock(), binding),
            });
        }
        let cancelled = existing.iter().flatten().any(DictationHotkey::is_recording);
        self.extra = rebound;
        cancelled
    }

    /// A Caps Lock press, if the lab's machine should act on it. A hold
    /// capture, another shortcut recording, or a pending double-tap gesture
    /// is left undisturbed.
    pub(super) fn caps_lock_press(&mut self) -> Option<HotkeyAction> {
        if self.recording_besides(Shortcut::CapsLock)
            || self.pending_gesture_besides(Shortcut::CapsLock)
        {
            return None;
        }
        let action = self.caps_lock.caps_lock_toggle()?;
        if action == HotkeyAction::Start {
            self.raw_capture = false;
        }
        Some(action)
    }

    /// Stands a Caps Lock capture down when its lab turns off. Returns
    /// whether one was recording so the caller can cancel it.
    pub(super) fn stop_caps_lock(&mut self) -> bool {
        if self.caps_lock.is_recording() {
            self.caps_lock.suspend();
            true
        } else {
            false
        }
    }

    /// Feeds one input to every machine, and returns what each did with
    /// which machine it was. A machine that starts while another records
    /// stands down until its keys come up, and only the dictation machine
    /// pastes.
    pub(super) fn process(
        &mut self,
        event: InputEvent,
        at: CaptureInstant,
    ) -> Vec<(Shortcut, HotkeyAction)> {
        let mut actions = Vec::new();
        for shortcut in Shortcut::all() {
            let others_recording = self.recording_besides(shortcut);
            let Some(machine) = self.machine_mut(shortcut) else {
                continue;
            };
            match machine.process(event, at) {
                Some(HotkeyAction::Start) if others_recording => {
                    machine.suspend();
                }
                Some(HotkeyAction::PasteLast | HotkeyAction::PasteMeeting)
                    if shortcut != Shortcut::Dictation => {}
                Some(action) => actions.push((shortcut, action)),
                None => {}
            }
        }
        actions
    }

    /// Stops whichever machine records, as a `stop` or `cancel` command does.
    pub(super) fn suspend_recording(&mut self) {
        for machine in self.machines() {
            if machine.is_recording() {
                machine.suspend();
            }
        }
    }
}

/// Creates, rebinds, or removes an optional shortcut machine. Returns
/// whether a removed machine was recording.
pub(super) fn rebind(
    slot: &mut Option<DictationHotkey>,
    binding: Option<crate::app_settings::RuntimeHotkey>,
    now: CaptureInstant,
) -> bool {
    match binding {
        None => slot.take().is_some_and(|machine| machine.is_recording()),
        Some(binding) => {
            match slot.as_mut() {
                Some(machine) => machine.set_binding(binding),
                None => {
                    *slot = Some(DictationHotkey::new(
                        now,
                        crate::app_settings::double_tap_lock(),
                        binding,
                    ));
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::RuntimeHotkey;

    fn hotkeys() -> Hotkeys {
        let base = CaptureInstant::ZERO;
        Hotkeys::new(
            DictationHotkey::new(base, true, RuntimeHotkey::default()),
            DictationHotkey::new(base, false, RuntimeHotkey::default()),
        )
    }

    #[test]
    fn a_pause_cancels_once_and_leaves_a_command_capture_alone() {
        let mut hotkeys = hotkeys();
        assert!(!hotkeys.suspend_when(false));

        // A capture under way is cancelled as the pause begins.
        assert!(hotkeys.dictation.lock());
        assert!(hotkeys.any_recording());
        assert!(hotkeys.suspend_when(true));
        assert!(!hotkeys.any_recording());

        // While the same pause holds, a command's capture keeps recording.
        assert!(hotkeys.dictation.lock());
        assert!(!hotkeys.suspend_when(true));
        assert!(hotkeys.any_recording());

        // Resuming leaves it alone too; the next pause cancels it.
        assert!(!hotkeys.suspend_when(false));
        assert!(hotkeys.any_recording());
        assert!(hotkeys.suspend_when(true));
        assert!(!hotkeys.any_recording());
    }
}
