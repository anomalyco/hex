//! The dictation shortcut state machine: hold, double-tap lock, Toggle,
//! chord discards, stale-key recovery, and the chord breaker.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Discard(DiscardCause),
    /// Hex ended the recording: its input was lost or suspended.
    Cancel,
    /// Toggle's cancel shortcut discarded the recording.
    CancelShortcut,
    PasteLast,
    PasteMeeting,
}

impl HotkeyAction {
    pub fn discard_cause(self) -> Option<DiscardCause> {
        match self {
            Self::Discard(cause) => Some(cause),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum State {
    Idle {
        // The last Recording release, kept only while double-tap lock may
        // still pair the next press with it.
        last_release_at: Option<CaptureInstant>,
    },
    FirstTapPressed,
    AwaitingSecondTap {
        released_at: CaptureInstant,
    },
    SecondTapPressed {
        first_released_at: CaptureInstant,
    },
    Recording {
        started_at: CaptureInstant,
        previous_release: Option<CaptureInstant>,
    },
    Locked,
    Dirty,
}

impl State {
    const IDLE: Self = Self::Idle {
        last_release_at: None,
    };

    /// A double-tap-only gesture that has not yet locked.
    fn is_pending_gesture(&self) -> bool {
        matches!(
            self,
            Self::FirstTapPressed | Self::AwaitingSecondTap { .. } | Self::SecondTapPressed { .. }
        )
    }
}

/// Modifier changes seen since the keyboard last reported no modifiers.
#[derive(Clone, Copy)]
struct ModifierHistory {
    flags: u64,
    flags_at: CaptureInstant,
    since_neutral: u64,
    neutral_at: Option<CaptureInstant>,
}

impl ModifierHistory {
    /// No evidence, and none from events stamped before `at`.
    const fn reset(at: CaptureInstant) -> Self {
        Self {
            flags: 0,
            flags_at: at,
            since_neutral: 0,
            neutral_at: None,
        }
    }
}

/// Live keyboard state, read only by the stale-press check.
#[derive(Clone, Copy)]
struct PhysicalKeyboard {
    flags: fn() -> u64,
    key_down: fn(u16) -> bool,
}

impl PhysicalKeyboard {
    // Same source as `recover_stale_keys`. Unit tests never read the real
    // keyboard (see `trigger_is_physically_down`); their default reads as
    // everything up, so every test can take the fast path.
    const SESSION: Self = if cfg!(test) {
        Self {
            flags: || 0,
            key_down: |_| false,
        }
    } else {
        Self {
            // SAFETY: Read-only queries of a documented event-source state.
            flags: || unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) },
            key_down: input_is_held,
        }
    };
}

/// Labs › Tap or hold. Unit tests never read the saved Labs switches; they
/// turn it on per machine.
const TAP_OR_HOLD: fn() -> bool = if cfg!(test) {
    || false
} else {
    crate::labs::tap_or_hold
};

pub struct DictationHotkey {
    state: State,
    pressed_keys: HashSet<u16>,
    double_tap_enabled: bool,
    double_tap_only: bool,
    /// Press to start, press again to finish: releasing locks the capture.
    toggle: bool,
    /// Labs › Tap or hold, read at each Toggle release: a press held past
    /// the hold threshold finishes on release instead of locking.
    tap_or_hold: fn() -> bool,
    binding: RuntimeHotkey,
    /// Toggle's cancel shortcut; `None` in Hold mode.
    cancel: Option<RuntimeHotkey>,
    ignore_before: Option<CaptureInstant>,
    stale_keys_neutral_since: Option<CaptureInstant>,
    recovery_ignore_through: Option<CaptureInstant>,
    recovery_updated_keys: HashSet<u16>,
    // Fn evidence comes only from modifier events; None marks a blind period.
    // The timestamp prevents delayed events from restoring invalidated evidence.
    function_modifier: (CaptureInstant, Option<bool>),
    // Recent HID key presses, which another app's shortcut may have hidden.
    physical_key_downs: VecDeque<(CaptureInstant, u16, HidTiming)>,
    // When the held modifier-only press Hex owns is due its chord breaker.
    chord_break_at: Option<CaptureInstant>,
    modifier_history: ModifierHistory,
    // When each tracked key last went down.
    key_down_at: HashMap<u16, CaptureInstant>,
    // When each modifier key (by its device flag) went down, for chords
    // that hold both sides of one.
    side_down_at: HashMap<u64, CaptureInstant>,
    physical: PhysicalKeyboard,
}

impl DictationHotkey {
    pub fn new(now: CaptureInstant, double_tap_enabled: bool, binding: RuntimeHotkey) -> Self {
        Self::with_binding(
            trigger_is_physically_down(binding),
            now,
            double_tap_enabled,
            binding,
        )
    }

    fn with_binding(
        trigger_down: bool,
        now: CaptureInstant,
        double_tap_enabled: bool,
        binding: RuntimeHotkey,
    ) -> Self {
        Self {
            state: if trigger_down {
                State::Recording {
                    started_at: now,
                    previous_release: None,
                }
            } else {
                State::IDLE
            },
            pressed_keys: HashSet::new(),
            double_tap_enabled,
            double_tap_only: false,
            toggle: false,
            tap_or_hold: TAP_OR_HOLD,
            binding,
            cancel: None,
            ignore_before: None,
            stale_keys_neutral_since: None,
            recovery_ignore_through: None,
            recovery_updated_keys: HashSet::new(),
            function_modifier: (CaptureInstant::ZERO, None),
            physical_key_downs: VecDeque::new(),
            chord_break_at: None,
            modifier_history: ModifierHistory::reset(CaptureInstant::ZERO),
            key_down_at: HashMap::new(),
            side_down_at: HashMap::new(),
            physical: PhysicalKeyboard::SESSION,
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, State::Recording { .. } | State::Locked)
    }

    /// Recording hands-free: locked by a double tap, a Toggle release, or
    /// `hex dictation start`.
    #[cfg(test)]
    pub fn is_locked(&self) -> bool {
        matches!(self.state, State::Locked)
    }

    pub fn suppresses_recognition(&self) -> bool {
        !matches!(self.state, State::Idle { .. })
    }

    pub fn set_double_tap_enabled(&mut self, enabled: bool) {
        self.double_tap_enabled = enabled;
        if !enabled {
            match &mut self.state {
                State::Idle { last_release_at } => *last_release_at = None,
                State::Recording {
                    previous_release, ..
                } => *previous_release = None,
                _ => {}
            }
        }
    }

    pub fn set_double_tap_only(&mut self, enabled: bool) {
        self.double_tap_only =
            enabled && self.binding.key_code.is_some() && self.double_tap_enabled;
        // Active captures still need their release to reach the audio owner.
        if !self.double_tap_only && self.state.is_pending_gesture() {
            self.state = State::IDLE;
        }
    }

    /// Takes effect from the next press; an active capture keeps its gesture.
    pub fn set_toggle(&mut self, enabled: bool) {
        self.toggle = enabled;
    }

    /// Toggle's cancel shortcut, or `None` for no cancel.
    pub fn set_cancel(&mut self, cancel: Option<RuntimeHotkey>) {
        self.cancel = cancel;
    }

    pub fn set_binding(&mut self, binding: RuntimeHotkey) {
        if matches!(self.state, State::Idle { .. }) && self.binding != binding {
            self.binding = binding;
            self.state = State::IDLE;
        }
    }

    pub fn suspend(&mut self) -> Option<HotkeyAction> {
        let was_recording = self.is_recording();
        let now = CaptureInstant::now();
        self.function_modifier = (now, None);
        self.modifier_history = ModifierHistory::reset(now);
        self.state = State::IDLE;
        self.pressed_keys.clear();
        self.key_down_at.clear();
        self.side_down_at.clear();
        self.stale_keys_neutral_since = None;
        self.chord_break_at = None;
        was_recording.then_some(HotkeyAction::Cancel)
    }

    /// Locks an idle machine without a press, as a `hex dictation start`
    /// does: the next shortcut press finishes it, the same as a double-tap
    /// lock. Returns false while a press is in flight.
    pub fn lock(&mut self) -> bool {
        if !matches!(self.state, State::Idle { .. }) && !self.state.is_pending_gesture() {
            return false;
        }
        self.state = State::Locked;
        self.pressed_keys.clear();
        self.key_down_at.clear();
        self.stale_keys_neutral_since = None;
        self.chord_break_at = None;
        true
    }

    pub fn disarm_pending_gesture(&mut self) {
        if !self.is_recording() {
            if matches!(self.state, State::Idle { .. }) || self.state.is_pending_gesture() {
                self.state = State::IDLE;
            }
            self.stale_keys_neutral_since = None;
        }
    }

    #[cfg(test)]
    pub fn suppress_until_release(&mut self) {
        self.state = State::Dirty;
        self.stale_keys_neutral_since = None;
        self.chord_break_at = None;
    }

    // Call only after draining input. Polling repairs bookkeeping, never capture boundaries.
    pub fn recover_stale_keys(&mut self) {
        self.recover_stale_keys_with(
            CaptureInstant::now,
            // Match the annotated-session tap, including assistive keyboard input.
            || unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) },
            input_is_held,
        );
    }

    fn recover_stale_keys_with(
        &mut self,
        now: impl FnOnce() -> CaptureInstant,
        mut flags: impl FnMut() -> u64,
        mut key_down: impl FnMut(u16) -> bool,
    ) {
        // Arrow key metadata can leave SecondaryFn set after release. Only ignore
        // it when modifier-change events independently say Fn is not held.
        let modifiers_mask = if self.function_modifier.1 == Some(false) {
            HOTKEY_MODIFIERS_MASK & !crate::app_settings::FUNCTION_KEY_MASK
        } else {
            HOTKEY_MODIFIERS_MASK
        };
        // A missed modifier release would make the next real press of it
        // read as no change.
        let stale_modifiers = self.modifier_history.flags & modifiers_mask != 0;
        let recoverable = match self.state {
            State::Dirty => true,
            // A lock captures while the keyboard sits idle. A missed key-up would
            // otherwise block the fresh shortcut press that finishes it.
            State::Idle { .. } | State::Locked => !self.pressed_keys.is_empty() || stale_modifiers,
            _ => false,
        };
        if !recoverable {
            self.stale_keys_neutral_since = None;
            return;
        }
        // A missing modifier release can leave Dirty with no ordinary keys tracked.
        // Avoid a full scan when a tracked key is still physically held.
        let sampled_flags = flags();
        if sampled_flags & modifiers_mask != 0
            || (!self.pressed_keys.is_empty()
                && self.pressed_keys.iter().all(|&code| key_down(code)))
            || (0..128).any(&mut key_down)
            // Mouse triggers share pressed_keys; a stale keyboard entry must
            // not cause recovery while a tracked mouse button is still down.
            || self.pressed_keys.iter().any(|&code| {
                crate::app_settings::HotkeyKey::mouse_button(code).is_some() && key_down(code)
            })
            || flags() & modifiers_mask != 0
        {
            self.stale_keys_neutral_since = None;
            return;
        }
        let sampled_through = now();
        let Some(neutral_since) = self.stale_keys_neutral_since else {
            self.stale_keys_neutral_since = Some(sampled_through);
            return;
        };
        if sampled_through.duration_since(neutral_since) < STALE_KEY_NEUTRAL_DURATION {
            return;
        }
        let locked = matches!(self.state, State::Locked);
        // A system shortcut such as Command-Tab swallows its key's release.
        tracing::warn!(
            key_count = self.pressed_keys.len(),
            key_codes = ?self.pressed_keys,
            locked,
            "resynchronized stale input tracking after neutral keyboard"
        );
        self.pressed_keys.clear();
        self.key_down_at.clear();
        // The scan is not atomic, so it rebuilds the held modifiers but is no
        // neutral evidence for keys whose delayed edges may still arrive.
        self.modifier_history.flags = sampled_flags;
        self.modifier_history.flags_at = sampled_through;
        self.side_down_at
            .retain(|&mask, _| sampled_flags & mask != 0);
        // Repair bookkeeping only: a lock keeps capturing until a fresh press.
        if !locked {
            self.state = State::IDLE;
        }
        self.chord_break_at = None;
        self.stale_keys_neutral_since = None;
        self.recovery_ignore_through = Some(sampled_through);
        self.recovery_updated_keys.clear();
    }

    #[cfg(test)]
    fn wait_for_release_with_state(&mut self, now: CaptureInstant, flags: u64, key_down: bool) {
        self.suspend();
        self.ignore_before = Some(now);
        if key_down && let Some(code) = self.binding.key_code {
            self.pressed_keys.insert(code);
        }
        if flags & HOTKEY_MODIFIERS_MASK != 0 || !self.pressed_keys.is_empty() {
            self.state = State::Dirty;
        }
    }

    // Suspended shortcut matching must still observe releases of previously held keys.
    pub fn track_key_state(&mut self, event: InputEvent, at: CaptureInstant) -> Option<bool> {
        if let InputEvent::Key { code, .. } | InputEvent::PhysicalKeyDown { code, .. } = event
            && is_own_globe_key(code, self.binding)
        {
            return None;
        }
        self.stale_keys_neutral_since = None;
        let fenced = self.ignore_before.is_some_and(|changed_at| at < changed_at)
            || self
                .recovery_ignore_through
                .is_some_and(|fence| at <= fence);
        if let InputEvent::Flags(flags) = event {
            for mask in MODIFIER_SIDE_MASKS {
                if flags & mask == 0 {
                    self.side_down_at.remove(&mask);
                } else {
                    self.side_down_at.entry(mask).or_insert(at);
                }
            }
        } else if matches!(event, InputEvent::TapDisabled) {
            self.side_down_at.clear();
        }
        if let InputEvent::Flags(flags) = event
            && at >= self.function_modifier.0
        {
            self.function_modifier = (
                at,
                Some(flags & crate::app_settings::FUNCTION_KEY_MASK != 0),
            );
        } else if matches!(event, InputEvent::TapDisabled) {
            let now = CaptureInstant::now();
            self.function_modifier = (now, None);
            self.modifier_history = ModifierHistory::reset(now);
        }
        // Only modifier changes: key events can carry Fn as navigation metadata.
        // Fenced edges predate the evidence that replaced them.
        if let InputEvent::Flags(flags) = event
            && !fenced
            && at >= self.modifier_history.flags_at
        {
            let history = &mut self.modifier_history;
            history.flags = flags;
            history.flags_at = at;
            if flags & HOTKEY_MODIFIERS_MASK == 0 {
                history.since_neutral = 0;
                history.neutral_at = Some(at);
            } else {
                history.since_neutral |= flags & HOTKEY_MODIFIERS_MASK;
            }
        }
        if let InputEvent::Key { code, .. } = event
            && let Some(fence) = self.recovery_ignore_through
        {
            // A delayed pre-recovery release must not erase a newer press of that key.
            if at <= fence && self.recovery_updated_keys.contains(&code) {
                return None;
            }
            if at > fence {
                self.recovery_updated_keys.insert(code);
            }
        }
        Some(match event {
            InputEvent::Key { code, down, .. } => {
                if down {
                    let down_at = self.key_down_at.entry(code).or_insert(at);
                    *down_at = (*down_at).max(at);
                    self.pressed_keys.insert(code)
                } else {
                    self.pressed_keys.remove(&code);
                    self.key_down_at.remove(&code);
                    false
                }
            }
            InputEvent::Flags(_)
            | InputEvent::PhysicalKeyDown { .. }
            | InputEvent::MouseDown
            | InputEvent::TapDisabled => false,
        })
    }

    fn log_hid(
        &self,
        message: &'static str,
        code: u16,
        at: CaptureInstant,
        timing: HidTiming,
        press_at: CaptureInstant,
    ) {
        tracing::info!(
            shortcut = "dictation",
            cause = DiscardCause::HidKey.as_str(),
            key_code = code,
            after_press_ms = signed_ms(at, press_at),
            hid_raw = timing.raw,
            hid_clock = timing.clock.as_str(),
            hid_received_after_press_ms = signed_ms(timing.received_at, press_at),
            hid_arrival_delay_ms = signed_ms(timing.received_at, at),
            "{message}"
        );
    }

    pub fn process(&mut self, event: InputEvent, now: CaptureInstant) -> Option<HotkeyAction> {
        let was_locked = matches!(self.state, State::Locked);
        let action = self.process_event(event, now);
        self.track_chord_break(event, now, action, was_locked);
        action
    }

    /// A bare modifier passes through to other apps, so a hold reads to them
    /// as a tap on release (Raycast's Option-tap hotkey, for one). Once Hex
    /// owns the press (a hold past the threshold, a Toggle start past the
    /// chord grace, or the press that finishes a lock) it is owed one chord
    /// breaker while still held.
    fn track_chord_break(
        &mut self,
        event: InputEvent,
        now: CaptureInstant,
        action: Option<HotkeyAction>,
        was_locked: bool,
    ) {
        if self.binding.key_code.is_some() || self.binding.modifiers.function {
            self.chord_break_at = None;
            return;
        }
        let released = match event {
            InputEvent::Flags(flags) | InputEvent::Key { flags, .. } => {
                !self.binding.required_modifiers_down(flags)
            }
            InputEvent::TapDisabled => true,
            InputEvent::PhysicalKeyDown { .. } | InputEvent::MouseDown => false,
        };
        match action {
            Some(HotkeyAction::Start) if matches!(self.state, State::Recording { .. }) => {
                // Toggle's press is usually a short tap, so it can't wait out
                // the hold; the grace lets an extra modifier pressed with it
                // discard it first.
                self.chord_break_at = Some(if self.toggle {
                    now + crate::feedback::CHORD_GRACE
                } else {
                    now + MINIMUM_HOLD_DURATION
                });
            }
            Some(HotkeyAction::Finish) if was_locked && !released => {
                self.chord_break_at = Some(now);
            }
            Some(_) => self.chord_break_at = None,
            None if released => self.chord_break_at = None,
            None => {}
        }
    }

    /// Whether the held press is due its chord breaker at `now`; true at most
    /// once per press.
    pub fn take_chord_break(&mut self, now: CaptureInstant) -> bool {
        let due = self.chord_break_at.is_some_and(|due_at| now >= due_at);
        if due {
            self.chord_break_at = None;
        }
        due
    }

    /// Posts the chord breaker the held press is due, if exactly its modifiers
    /// are still physically down; any other modifier already breaks the tap.
    /// The posted key carries Hex's marker, so neither shortcut machine sees
    /// it, and the key tap drops it before the focused app.
    pub fn break_chord(&mut self, now: CaptureInstant) {
        if !self.take_chord_break(now) {
            return;
        }
        // SAFETY: A read-only query of the documented HID system state.
        let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
        if !self.binding.exact_modifiers(flags) {
            return;
        }
        if let Err(error) = crate::keyboard::post_chord_breaker(flags) {
            tracing::warn!(shortcut = "dictation", %error, "chord breaker failed");
        }
    }

    fn process_event(&mut self, event: InputEvent, now: CaptureInstant) -> Option<HotkeyAction> {
        self.stale_keys_neutral_since = None;
        if matches!(event, InputEvent::TapDisabled) {
            let was_recording = self.is_recording();
            let now = CaptureInstant::now();
            self.function_modifier = (now, None);
            self.modifier_history = ModifierHistory::reset(now);
            self.state = State::Dirty;
            return was_recording.then_some(HotkeyAction::Cancel);
        }
        // Queued edges from before an opt-in transition cannot start a new capture.
        if self
            .ignore_before
            .is_some_and(|changed_at| now < changed_at)
        {
            return None;
        }
        let prior = self.modifier_history;
        let fresh_key_down = self.track_key_state(event, now)?;
        let binding_press = self.binding_press(event, fresh_key_down, prior.flags);

        if self
            .recovery_ignore_through
            .is_some_and(|sampled_through| now <= sampled_through)
        {
            if binding_press {
                self.log_ignored_press("inside a stale-key recovery window");
            }
            // A key can go down during the non-atomic scan. Retain its edge but
            // require a later neutral event before accepting another gesture.
            // An old neutral release cannot invalidate the recovery's neutral sample.
            let neutral = matches!(
                event,
                InputEvent::Flags(flags) | InputEvent::Key { flags, .. }
                    if flags & HOTKEY_MODIFIERS_MASK == 0 && self.pressed_keys.is_empty()
            );
            if !neutral && matches!(self.state, State::Idle { .. } | State::Dirty) {
                self.state = State::Dirty;
            }
            return None;
        }

        if fresh_key_down
            && let Some(action) = paste_action(event, crate::app_settings::runtime_hotkeys())
        {
            self.state = State::Dirty;
            return Some(action);
        }
        // Toggle's cancel shortcut discards the recording; the tap kept the
        // key from the app in front. Hold mode has no cancel.
        if fresh_key_down
            && self.toggle
            && self.is_recording()
            && let InputEvent::Key {
                code,
                down: true,
                flags,
            } = event
            && self
                .cancel
                .is_some_and(|cancel| cancel.matches_key_press(code, flags))
        {
            self.state = State::Dirty;
            return Some(HotkeyAction::CancelShortcut);
        }

        let ignored_because = binding_press
            .then(|| self.resync_stale_press(now))
            .flatten();
        let trigger_pressed = self.trigger_pressed(event, fresh_key_down, prior.flags);
        if let InputEvent::PhysicalKeyDown { code, timing } = event
            && self.binding.key_code != Some(code)
        {
            if self.physical_key_downs.len() == PHYSICAL_KEY_DOWN_HISTORY {
                self.physical_key_downs.pop_front();
            }
            self.physical_key_downs.push_back((now, code, timing));
        }
        // HID evidence can overtake the session event of the press it follows.
        if trigger_pressed
            && matches!(
                self.state,
                State::Idle { .. } | State::AwaitingSecondTap { .. }
            )
            && let Some(&(at, code, timing)) = self.physical_key_downs.iter().find(|(at, ..)| {
                at.checked_duration_since(now)
                    .is_some_and(|after| after > Duration::ZERO && after < MINIMUM_HOLD_DURATION)
            })
        {
            self.log_hid(
                "dictation press dropped: HID key evidence overtook it",
                code,
                at,
                timing,
                now,
            );
            self.state = State::Dirty;
            return None;
        }
        let flags = match event {
            InputEvent::Flags(flags) | InputEvent::Key { flags, .. } => Some(flags),
            InputEvent::PhysicalKeyDown { .. }
            | InputEvent::MouseDown
            | InputEvent::TapDisabled => None,
        };
        let trigger_down = flags.is_some_and(|flags| {
            self.binding.required_modifiers_down(flags)
                && self
                    .binding
                    .key_code
                    .is_none_or(|code| self.pressed_keys.contains(&code))
        });
        let trigger_released = flags.is_some() && !trigger_down;
        let unrelated_key_down = matches!(
            event,
            InputEvent::Key {
                code,
                down: true,
                ..
            } | InputEvent::PhysicalKeyDown { code, .. } if self.binding.key_code != Some(code)
        );
        let extra_modifiers = flags.is_some_and(|flags| {
            self.binding.required_modifiers_down(flags) && !self.binding.exact_modifiers(flags)
        });
        // Escape ends no dictation, not even as a chord (#786).
        let chord_key_down = unrelated_key_down
            && !matches!(
                event,
                InputEvent::Key {
                    code: ESCAPE_KEY_CODE,
                    ..
                } | InputEvent::PhysicalKeyDown {
                    code: ESCAPE_KEY_CODE,
                    ..
                }
            );

        let action = match self.state {
            State::Idle { .. } if self.double_tap_only && !self.toggle && trigger_pressed => {
                self.state = State::FirstTapPressed;
                None
            }
            State::FirstTapPressed if trigger_released => {
                self.state = State::AwaitingSecondTap { released_at: now };
                None
            }
            State::AwaitingSecondTap { released_at }
                if trigger_pressed && now.duration_since(released_at) < DOUBLE_TAP_WINDOW =>
            {
                self.state = State::SecondTapPressed {
                    first_released_at: released_at,
                };
                None
            }
            State::SecondTapPressed { first_released_at }
                if trigger_released
                    && now.duration_since(first_released_at) < DOUBLE_TAP_WINDOW =>
            {
                self.state = State::Locked;
                Some(HotkeyAction::Start)
            }
            State::SecondTapPressed { .. } if trigger_released => {
                self.state = State::IDLE;
                None
            }
            State::AwaitingSecondTap { .. }
            | State::FirstTapPressed
            | State::SecondTapPressed { .. }
                if unrelated_key_down
                    || extra_modifiers
                    || matches!(event, InputEvent::MouseDown) =>
            {
                self.state = State::Dirty;
                None
            }
            State::AwaitingSecondTap { released_at }
                if now.duration_since(released_at) >= DOUBLE_TAP_WINDOW =>
            {
                self.state = if trigger_pressed {
                    State::FirstTapPressed
                } else {
                    State::IDLE
                };
                None
            }
            State::Locked if trigger_pressed => {
                self.state = State::Dirty;
                Some(HotkeyAction::Finish)
            }
            State::Idle { last_release_at } if trigger_pressed => {
                let previous_release = last_release_at.filter(|released| {
                    self.double_tap_enabled && now.duration_since(*released) < DOUBLE_TAP_WINDOW
                });
                self.state = State::Recording {
                    started_at: now,
                    previous_release,
                };
                Some(HotkeyAction::Start)
            }
            // With Labs › Tap or hold, a Toggle press held past the hold
            // threshold is push-to-talk: it falls through to finish below.
            State::Recording { started_at, .. }
                if trigger_released
                    && self.toggle
                    && (now.duration_since(started_at) < MINIMUM_HOLD_DURATION
                        || !(self.tap_or_hold)()) =>
            {
                self.state = State::Locked;
                None
            }
            State::Recording {
                previous_release: Some(released),
                ..
            } if trigger_released && now.duration_since(released) < DOUBLE_TAP_WINDOW => {
                self.state = State::Locked;
                None
            }
            State::Recording { .. } if trigger_released => {
                self.state = State::Idle {
                    last_release_at: self.double_tap_enabled.then_some(now),
                };
                Some(HotkeyAction::Finish)
            }
            State::Recording { started_at, .. }
                // Delayed HID evidence from before the press is not a chord.
                if !(matches!(event, InputEvent::PhysicalKeyDown { .. }) && now < started_at)
                    && ((now.duration_since(started_at) < MINIMUM_HOLD_DURATION
                        && (chord_key_down
                            || matches!(event, InputEvent::MouseDown)
                            || extra_modifiers))
                        // A held modifier-only shortcut plus any key is a chord at any time.
                        || (chord_key_down
                            && self.binding.key_code.is_none()
                            && !self.toggle)) =>
            {
                self.state = State::Dirty;
                let cause = match event {
                    InputEvent::PhysicalKeyDown { code, timing } => {
                        self.log_hid(
                            "dictation press discarded as a chord",
                            code,
                            now,
                            timing,
                            started_at,
                        );
                        DiscardCause::HidKey
                    }
                    InputEvent::Key { code, .. } if chord_key_down => {
                        tracing::info!(
                            shortcut = "dictation",
                            cause = DiscardCause::Key.as_str(),
                            key_code = code,
                            after_press_ms = signed_ms(now, started_at),
                            "dictation press discarded as a chord"
                        );
                        DiscardCause::Key
                    }
                    _ => {
                        let cause = if matches!(event, InputEvent::MouseDown) {
                            DiscardCause::Mouse
                        } else {
                            DiscardCause::ExtraModifier
                        };
                        tracing::info!(
                            shortcut = "dictation",
                            cause = cause.as_str(),
                            modifier_flags =
                                format_args!("{:#x}", flags.unwrap_or(0) & HOTKEY_MODIFIERS_MASK),
                            after_press_ms = signed_ms(now, started_at),
                            "dictation press discarded as a chord"
                        );
                        cause
                    }
                };
                Some(HotkeyAction::Discard(cause))
            }
            State::Dirty
                if flags.is_some_and(|flags| flags & HOTKEY_MODIFIERS_MASK == 0)
                    && self.pressed_keys.is_empty() =>
            {
                self.state = State::IDLE;
                None
            }
            _ => None,
        };
        if action.is_none()
            && let Some(reason) = ignored_because
        {
            self.log_ignored_press(reason);
        }
        action
    }

    /// A down edge of the binding, whether or not tracked keys block it.
    fn binding_press(&self, event: InputEvent, fresh_key_down: bool, prior_flags: u64) -> bool {
        match self.binding.key_code {
            Some(_) => self.trigger_pressed(event, fresh_key_down, prior_flags),
            None => self.modifiers_went_down(event, prior_flags),
        }
    }

    /// The modifier-only binding's modifiers went down, from up, to exactly
    /// themselves. Releasing Shift off a held Shift-Option is no Option press,
    /// nor is Caps Lock or the other Option changing while Option is held.
    fn modifiers_went_down(&self, event: InputEvent, prior_flags: u64) -> bool {
        matches!(event, InputEvent::Flags(flags) if self.binding.exact_modifiers(flags))
            && !self.binding.modifiers.is_empty()
            && !self.binding.required_modifiers_down(prior_flags)
            && self.both_sides_went_down_together()
    }

    /// A system shortcut such as Command-Tab swallows its key's release, and
    /// the stale key blocks the next press until `recover_stale_keys` has
    /// sampled 100 ms of neutral keyboard. A press of the binding itself
    /// resynchronizes at once, as a fresh press, when the keyboard reported no
    /// modifiers after every tracked key went down, only the binding's
    /// modifiers went down since, and both the tracked keys and every other
    /// modifier read up now. A lock stays locked, so the press finishes it.
    /// Returns why the press stays ignored.
    fn resync_stale_press(&mut self, now: CaptureInstant) -> Option<&'static str> {
        let binding_key = self.binding.key_code;
        let stale = |code: &u16| Some(*code) != binding_key;
        let has_stale = self.pressed_keys.iter().any(stale);
        match self.state {
            State::Dirty if has_stale => {}
            State::Dirty => return Some("waiting for a neutral keyboard"),
            // Tracked keys block only modifier-only presses outside Dirty.
            State::Idle { .. } | State::Locked if has_stale && binding_key.is_none() => {}
            _ => return None,
        }
        let history = self.modifier_history;
        // Typing after the switch is fine; only the stale keys' own presses count.
        let neutral_since_down = |code: &u16| {
            history
                .neutral_at
                .zip(self.key_down_at.get(code).copied())
                .is_some_and(|(neutral, down)| neutral > down)
        };
        if !self
            .pressed_keys
            .iter()
            .filter(|code| stale(code))
            .all(neutral_since_down)
        {
            return Some("no neutral keyboard since the tracked keys went down");
        }
        let binding_mask = modifier_mask(self.binding.modifiers);
        if history.since_neutral & !binding_mask != 0 {
            return Some("another modifier went down since the keyboard was neutral");
        }
        // Key repeats refresh `key_down_at`, so a silent keyboard key is latched.
        let silent = |code: &u16| {
            crate::app_settings::HotkeyKey::mouse_button(*code).is_none()
                && self.key_down_at.get(code).is_some_and(|down| {
                    now.checked_duration_since(*down)
                        .is_some_and(|age| age >= TRACKED_KEY_REPEAT_TIMEOUT)
                })
        };
        let held: Vec<u16> = self
            .pressed_keys
            .iter()
            .filter(|code| stale(code))
            .filter(|&&code| (self.physical.key_down)(code))
            .copied()
            .collect();
        if !held.iter().all(silent) {
            return Some("a tracked key is physically down");
        } else if !held.is_empty() {
            tracing::info!(key_codes = ?held, "tracked keys sent no key repeat; released");
        }
        // As in recovery, arrow keys can leave SecondaryFn set after release.
        let mut others = HOTKEY_MODIFIERS_MASK & !binding_mask;
        if self.function_modifier.1 == Some(false) {
            others &= !crate::app_settings::FUNCTION_KEY_MASK;
        }
        if (self.physical.flags)() & others != 0 {
            return Some("another modifier is physically down");
        }
        tracing::info!(
            shortcut = "dictation",
            key_codes = ?self.pressed_keys,
            "resynchronized stale input tracking on a shortcut press"
        );
        self.pressed_keys.retain(|code| !stale(code));
        let pressed = &self.pressed_keys;
        self.key_down_at.retain(|code, _| pressed.contains(code));
        // Command-Tab while hands-free: the press finishes the lock.
        if !matches!(self.state, State::Locked) {
            self.state = State::IDLE;
        }
        self.stale_keys_neutral_since = None;
        // Edges stamped before the press must not finish or restart it, and a
        // delayed older release must not untrack the binding's key. Edges at
        // the press itself still count, so a same-tick chord discards it.
        self.recovery_ignore_through = now.checked_sub(Duration::from_nanos(1));
        self.recovery_updated_keys = binding_key
            .filter(|code| self.pressed_keys.contains(code))
            .into_iter()
            .collect();
        None
    }

    fn log_ignored_press(&self, reason: &'static str) {
        tracing::info!(
            shortcut = "dictation",
            reason,
            key_codes = ?self.pressed_keys,
            "shortcut press ignored"
        );
    }

    fn trigger_pressed(&self, event: InputEvent, fresh_key_down: bool, prior_flags: u64) -> bool {
        match self.binding.key_code {
            Some(key_code) => {
                fresh_key_down
                    && matches!(
                        event,
                        InputEvent::Key {
                            code,
                            down: true,
                            flags,
                        } if code == key_code && self.binding.exact_modifiers(flags)
                    )
            }
            None => self.pressed_keys.is_empty() && self.modifiers_went_down(event, prior_flags),
        }
    }

    /// A modifier-only chord on both sides of a modifier (both Options)
    /// presses only when its two keys go down within `BOTH_SIDES_WINDOW`,
    /// in either order, as one gesture; one held and the other added later
    /// presses nothing. Other chords press as soon as their keys are down.
    fn both_sides_went_down_together(&self) -> bool {
        self.binding.both_sides_masks().all(|(left, right)| {
            match (self.side_down_at.get(&left), self.side_down_at.get(&right)) {
                (Some(&left), Some(&right)) => {
                    left.max(right).duration_since(left.min(right)) <= BOTH_SIDES_WINDOW
                }
                _ => false,
            }
        })
    }
}

/// Each modifier key's device flag, left and right.
const MODIFIER_SIDE_MASKS: [u64; 8] = {
    use crate::app_settings::{
        LEFT_COMMAND_MASK, LEFT_CONTROL_MASK, LEFT_OPTION_MASK, LEFT_SHIFT_MASK,
        RIGHT_COMMAND_MASK, RIGHT_CONTROL_MASK, RIGHT_OPTION_MASK, RIGHT_SHIFT_MASK,
    };
    [
        LEFT_CONTROL_MASK,
        RIGHT_CONTROL_MASK,
        LEFT_OPTION_MASK,
        RIGHT_OPTION_MASK,
        LEFT_SHIFT_MASK,
        RIGHT_SHIFT_MASK,
        LEFT_COMMAND_MASK,
        RIGHT_COMMAND_MASK,
    ]
};

/// The device-independent flag bits of `modifiers`, either side.
fn modifier_mask(modifiers: crate::app_settings::HotkeyModifiers) -> u64 {
    use crate::app_settings::{
        COMMAND_KEY_MASK, CONTROL_KEY_MASK, FUNCTION_KEY_MASK, OPTION_KEY_MASK, SHIFT_KEY_MASK,
    };
    [
        (modifiers.control.is_some(), CONTROL_KEY_MASK),
        (modifiers.option.is_some(), OPTION_KEY_MASK),
        (modifiers.shift.is_some(), SHIFT_KEY_MASK),
        (modifiers.command.is_some(), COMMAND_KEY_MASK),
        (modifiers.function, FUNCTION_KEY_MASK),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |mask, (_, bit)| mask | bit)
}

fn trigger_is_physically_down(binding: RuntimeHotkey) -> bool {
    // Unit tests never read the real keyboard: a key held while they run must
    // not change their outcome, and the first HID query can deadlock in a
    // process without a window-server session.
    if cfg!(test) {
        return false;
    }
    // HID state reflects the physical keyboard. Combined-session state can
    // remain latched after synthetic events or application switching.
    // SAFETY: These are pure system queries with a documented state id.
    unsafe {
        !binding.is_empty()
            && binding.exact_modifiers(CGEventSourceFlagsState(HID_SYSTEM_STATE))
            && binding
                .key_code
                .is_none_or(|code| input_is_down(HID_SYSTEM_STATE, code))
    }
}

// The suppression tests drive the hotkey's private state, so they live under it.
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
