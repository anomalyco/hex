//! The dictation shortcut machines: dictation, Labs › Second dictation
//! shortcut while it is on and bound, Dictate without processing while it is
//! bound, Labs › Caps Lock dictation's hands-free toggle, and Labs › More
//! dictation shortcuts' rows while that Lab is on.

use crate::app_settings::MAX_EXTRA_DICTATION_SHORTCUTS;
use crate::audio::CaptureInstant;
use crate::control::{PrintReply, PrintedTranscript};
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

/// Per-capture output metadata for whichever shortcut machine is recording:
/// whether Output processing is skipped (`raw`), and whether a `hex dictation
/// start --print` command owns its transcript (`print`). Lives on `Hotkeys`
/// only while a capture is active so neither fact can outlive a recording or
/// leak into a later dictation.
#[derive(Debug)]
pub(super) struct ActiveCapture {
    pub(super) shortcut: Shortcut,
    pub(super) raw: bool,
    pub(super) print: Option<PrintReply>,
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
    /// Output metadata for the recording currently in progress, if any.
    pub(super) active: Option<ActiveCapture>,
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
            active: None,
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

    /// Labs › Stop phrase: ends whichever machine's hands-free capture records
    /// now, as its own finishing press would, and reports which one it was. A
    /// held press (Hold mode, or Toggle's push-to-talk) records on.
    pub(super) fn finish_hands_free(&mut self) -> Option<(Shortcut, HotkeyAction)> {
        let shortcut = Shortcut::all().find(|shortcut| {
            self.machine(*shortcut)
                .is_some_and(DictationHotkey::is_locked)
        })?;
        let action = self.machine_mut(shortcut)?.finish_hands_free()?;
        Some((shortcut, action))
    }

    /// The cancel shortcut of the recording that takes one, for the tap to
    /// keep from the app in front.
    pub(super) fn recording_cancel(&self) -> Option<crate::app_settings::RuntimeHotkey> {
        Shortcut::all()
            .filter_map(|shortcut| self.machine(shortcut))
            .find_map(DictationHotkey::recording_cancel)
    }

    /// Whether a held capture takes Escape at `now`, for the tap to keep it
    /// from the app in front.
    pub(super) fn takes_escape(&self, now: CaptureInstant) -> bool {
        Shortcut::all()
            .filter_map(|shortcut| self.machine(shortcut))
            .any(|machine| machine.takes_escape(now))
    }

    pub(super) fn suppresses_recognition(&self) -> bool {
        Shortcut::all()
            .filter_map(|shortcut| self.machine(shortcut))
            .any(DictationHotkey::suppresses_recognition)
    }

    /// Stands `shortcut`'s machine down, as when its start was refused.
    pub(super) fn suspend(&mut self, shortcut: Shortcut) {
        if self.active.as_ref().is_some_and(|a| a.shortcut == shortcut) {
            self.fail_print("its recording already ended");
        }
        match self.machine_mut(shortcut) {
            Some(machine) => machine.suspend(),
            None => self.dictation.suspend(),
        };
    }

    /// Suspends every machine (never stopping at the first, as `any`
    /// would) and reports whether one was recording. A command still waiting
    /// for a print-only recording is answered: that recording is over.
    pub(super) fn suspend_all(&mut self) -> bool {
        self.fail_print("its recording already ended");
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
        if let Some(active) = &self.active
            && ((raw_cancelled && active.shortcut == Shortcut::Raw)
                || (second_cancelled && active.shortcut == Shortcut::Second)
                || (extra_cancelled && matches!(active.shortcut, Shortcut::Extra(_))))
        {
            self.fail_print("its recording already ended");
        }
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
            self.begin_capture(Shortcut::CapsLock, false);
        }
        Some(action)
    }

    /// Stands a Caps Lock capture down when its lab turns off. Returns
    /// whether one was recording so the caller can cancel it.
    pub(super) fn stop_caps_lock(&mut self) -> bool {
        if self.caps_lock.is_recording() {
            if self
                .active
                .as_ref()
                .is_some_and(|active| active.shortcut == Shortcut::CapsLock)
            {
                self.active = None;
            }
            self.caps_lock.suspend();
            true
        } else {
            false
        }
    }

    /// Records a newly started capture's owning shortcut and `raw` mode. Any
    /// leftover print reply from an older recording is failed immediately so
    /// it can never answer this capture.
    pub(super) fn begin_capture(&mut self, shortcut: Shortcut, raw: bool) {
        self.fail_print("its recording already ended");
        self.active = Some(ActiveCapture {
            shortcut,
            raw,
            print: None,
        });
    }

    /// Attaches a `hex dictation start --print`'s reply to the recording it
    /// just started, which answers it instead of pasting or copying (preserving
    /// `raw` if started with `--raw --print`).
    pub(super) fn keep_print(&mut self, print: PrintReply) {
        match &mut self.active {
            Some(active) if active.shortcut == Shortcut::Dictation => {
                if let Some(previous) = active.print.replace(print) {
                    previous.send(PrintedTranscript::Failed(
                        "its recording already ended".into(),
                    ));
                }
            }
            _ => {
                self.fail_print("its recording already ended");
                self.active = Some(ActiveCapture {
                    shortcut: Shortcut::Dictation,
                    raw: false,
                    print: Some(print),
                });
            }
        }
    }

    /// Takes the active capture's metadata, leaving `None` behind.
    pub(super) fn take_capture(&mut self) -> Option<ActiveCapture> {
        self.active.take()
    }

    /// Resolves `(raw, print)` for a shortcut's `action` and keeps `active`
    /// synchronized with it. A `Start` begins a fresh capture (failing any
    /// leftover print reply); an ending action from the owning shortcut takes
    /// `active` and answers its `print` reply for discards or cancels, while
    /// another shortcut's action leaves the active capture untouched.
    pub(super) fn resolve_action(
        &mut self,
        shortcut: Shortcut,
        action: HotkeyAction,
    ) -> (bool, Option<PrintReply>) {
        if action == HotkeyAction::Start {
            let raw = shortcut == Shortcut::Raw;
            self.begin_capture(shortcut, raw);
            return (raw, None);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.shortcut != shortcut)
        {
            return (false, None);
        }
        let Some(active) = self.active.take() else {
            return (false, None);
        };
        match action {
            HotkeyAction::Finish => (active.raw, active.print),
            HotkeyAction::Discard(_) => {
                if let Some(print) = active.print {
                    print.send(PrintedTranscript::NoSpeech);
                }
                (active.raw, None)
            }
            HotkeyAction::PasteLast | HotkeyAction::Cancel | HotkeyAction::CancelShortcut => {
                if let Some(print) = active.print {
                    print.send(PrintedTranscript::Cancelled);
                }
                (active.raw, None)
            }
            HotkeyAction::Start => unreachable!("handled above"),
        }
    }

    /// Clears the active capture and answers any print-only recording that Hex
    /// itself ends without a transcript (a microphone loss, a suspended
    /// shortcut, a closed listener), so its command never waits for the cap.
    pub(super) fn fail_print(&mut self, why: &str) {
        if let Some(print) = self.active.take().and_then(|active| active.print) {
            print.send(PrintedTranscript::Failed(why.into()));
        }
    }

    /// Clears the active capture and answers any print-only recording that
    /// `hex dictation cancel` discards.
    pub(super) fn cancel_capture(&mut self) {
        if let Some(print) = self.active.take().and_then(|active| active.print) {
            print.send(PrintedTranscript::Cancelled);
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
                Some(HotkeyAction::PasteLast) if shortcut != Shortcut::Dictation => {}
                Some(action) => actions.push((shortcut, action)),
                None => {}
            }
        }
        actions
    }

    /// Stops whichever machine records, as a `stop` or `cancel` command does.
    /// A reply still waiting belongs to a recording that already ended, so it
    /// is answered rather than left to a later capture.
    pub(super) fn suspend_recording(&mut self) {
        self.fail_print("its recording already ended");
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
    use std::time::Duration;

    use super::*;
    use crate::app_settings::RuntimeHotkey;

    fn hotkeys() -> Hotkeys {
        let base = CaptureInstant::ZERO;
        let option = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers::option(),
            key_code: None,
        };
        Hotkeys::new(
            DictationHotkey::new(base, true, option),
            DictationHotkey::new(base, false, RuntimeHotkey::default()),
        )
    }

    /// The tap keeps the recording's own cancel key: Toggle's while a toggled
    /// recording runs, none in Hold mode, and Escape for a Caps Lock capture
    /// whatever Toggle's cancel is.
    #[test]
    fn the_tap_cancels_with_the_recording_machines_own_key() {
        let escape = crate::app_settings::HotkeyBinding::cancel_default().runtime();
        let custom = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers::default(),
            key_code: Some(1),
        };
        let mut machines = hotkeys();
        machines.dictation.set_cancel(Some(custom));
        assert_eq!(machines.recording_cancel(), None, "nothing records");
        assert!(machines.dictation.lock());
        assert_eq!(machines.recording_cancel(), None, "Hold mode has no cancel");
        machines.dictation.set_toggle(true);
        assert_eq!(machines.recording_cancel(), Some(custom));

        let mut machines = hotkeys();
        machines.dictation.set_cancel(Some(custom));
        machines.caps_lock.set_toggle(true);
        machines.caps_lock.set_cancel(Some(escape));
        assert!(machines.caps_lock.lock());
        assert_eq!(machines.recording_cancel(), Some(escape));

        // An extra row cancels in its own mode, Hold included.
        let mut machines = hotkeys();
        let mut extra = DictationHotkey::new(CaptureInstant::ZERO, false, custom);
        extra.set_cancel(Some(escape));
        extra.set_cancel_in_any_mode(true);
        assert!(extra.lock());
        machines.extra[0] = Some(extra);
        assert_eq!(machines.recording_cancel(), Some(escape));
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

    /// A `hex dictation start --print` recording is finished by the shortcut
    /// itself, and its reply goes with the capture to transcription.
    #[test]
    fn a_printing_start_follows_the_shortcut_that_finishes_it() {
        let base = CaptureInstant::ZERO;
        let at = |ms: u64| base + Duration::from_millis(ms);
        let mut hotkeys = hotkeys();
        assert!(hotkeys.dictation.lock());
        let (start, printed) = PrintReply::channel();
        hotkeys.keep_print(start);

        // The next shortcut press finishes the locked recording.
        assert_eq!(
            hotkeys.process(
                InputEvent::Flags(crate::app_settings::OPTION_KEY_MASK),
                at(0)
            ),
            [(Shortcut::Dictation, HotkeyAction::Finish)]
        );
        // Only `resolve_action` takes the reply; the action alone leaves it.
        assert!(hotkeys.active.as_ref().is_some_and(|a| a.print.is_some()));
        let (raw, print) = hotkeys.resolve_action(Shortcut::Dictation, HotkeyAction::Finish);
        assert!(!raw);
        let print = print.expect("the transcript goes to the printing start");
        assert!(hotkeys.active.is_none());
        print.send(PrintedTranscript::Text("Ship it.".into()));
        assert_eq!(
            printed.recv_timeout(Duration::from_millis(50)),
            Ok(PrintedTranscript::Text("Ship it.".into()))
        );

        // A cancel is refused rather than waited out.
        let (start, printed) = PrintReply::channel();
        hotkeys.keep_print(start);
        assert!(
            hotkeys
                .resolve_action(Shortcut::Dictation, HotkeyAction::Cancel)
                .1
                .is_none()
        );
        assert!(hotkeys.active.is_none());
        assert!(matches!(
            printed.recv_timeout(Duration::from_millis(50)),
            Ok(PrintedTranscript::Cancelled)
        ));
    }

    /// A held reply belongs to the recording that started it: another
    /// shortcut leaves it alone, and a later start or a suspension answers it
    /// instead of letting it wait for a new dictation.
    #[test]
    fn a_held_print_reply_never_answers_a_later_dictation() {
        use crate::suppression::DiscardCause;

        let mut hotkeys = hotkeys();

        // The raw shortcut's finish isn't the dictation machine's, so the
        // reply stays held for its own recording.
        let (start, printed) = PrintReply::channel();
        hotkeys.keep_print(start);
        assert!(
            hotkeys
                .resolve_action(Shortcut::Raw, HotkeyAction::Finish)
                .1
                .is_none()
        );
        assert!(hotkeys.active.as_ref().is_some_and(|a| a.print.is_some()));
        assert!(matches!(
            printed.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        // A start means that recording already ended: fail it, never let it
        // wait for the new capture.
        assert!(
            hotkeys
                .resolve_action(Shortcut::Dictation, HotkeyAction::Start)
                .1
                .is_none()
        );
        assert!(hotkeys.active.as_ref().is_some_and(|a| a.print.is_none()));
        assert!(matches!(
            printed.recv_timeout(Duration::from_millis(50)),
            Ok(PrintedTranscript::Failed(why)) if why == "its recording already ended"
        ));

        // A short tap or chord never spoke.
        let (start, printed) = PrintReply::channel();
        hotkeys.keep_print(start);
        assert!(
            hotkeys
                .resolve_action(
                    Shortcut::Dictation,
                    HotkeyAction::Discard(DiscardCause::Key)
                )
                .1
                .is_none()
        );
        assert!(hotkeys.active.is_none());
        assert!(matches!(
            printed.recv_timeout(Duration::from_millis(50)),
            Ok(PrintedTranscript::NoSpeech)
        ));

        // Suspending every machine answers a reply that was still waiting.
        let (start, printed) = PrintReply::channel();
        hotkeys.keep_print(start);
        hotkeys.suspend_all();
        assert!(hotkeys.active.is_none());
        assert!(matches!(
            printed.recv_timeout(Duration::from_millis(50)),
            Ok(PrintedTranscript::Failed(_))
        ));
    }

    /// Raw and print compose cleanly (`hex dictation start --raw --print` or
    /// `hex dictation start --raw` + `stop --print`), and ending a raw capture
    /// clears `raw` so it never leaks to the next dictation.
    #[test]
    fn raw_and_print_compose_on_the_active_capture_and_clear_on_end() {
        let mut hotkeys = hotkeys();

        // Start raw + print on the dictation machine: finishing returns both.
        hotkeys.begin_capture(Shortcut::Dictation, true);
        let (start, _printed) = PrintReply::channel();
        hotkeys.keep_print(start);
        let (raw, print) = hotkeys.resolve_action(Shortcut::Dictation, HotkeyAction::Finish);
        assert!(raw, "raw --print preserves raw mode");
        assert!(print.is_some(), "raw --print preserves the print reply");
        assert!(hotkeys.active.is_none(), "active capture clears on finish");

        // A raw shortcut capture taken by control stop keeps raw=true and clears.
        let (raw, print) = hotkeys.resolve_action(Shortcut::Raw, HotkeyAction::Start);
        assert!(raw && print.is_none());
        let stopped = hotkeys.take_capture().expect("active raw capture");
        assert!(stopped.raw);
        assert!(stopped.print.is_none());
        assert!(hotkeys.active.is_none());

        // Nothing left latched when no capture is active.
        assert_eq!(
            hotkeys.resolve_action(Shortcut::Dictation, HotkeyAction::Finish),
            (false, None)
        );
    }
}
