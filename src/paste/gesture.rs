//! Paste Last on the Mac session tap. No work on the initial press; the
//! edge timestamps, not listener scheduling latency, decide tap versus hold.

use crate::app_settings::RuntimeHotkey;
use crate::audio::CaptureInstant;
use crate::suppression::InputEvent;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Wait,
    PasteLatest,
    Browse,
    Cancel,
}

#[derive(Default)]
pub(crate) struct Gesture(Option<(RuntimeHotkey, CaptureInstant)>);

impl Gesture {
    pub fn press(
        &mut self,
        binding: RuntimeHotkey,
        at: CaptureInstant,
        has_release: bool,
    ) -> Effect {
        if self.0.is_some() {
            return Effect::Wait;
        }
        if !has_release {
            return Effect::PasteLatest;
        }
        self.0 = Some((binding, at));
        Effect::Wait
    }

    pub fn edge(&mut self, event: InputEvent, at: CaptureInstant) -> Effect {
        let Some((binding, start)) = self.0 else {
            return Effect::Wait;
        };
        if at < start {
            return Effect::Wait;
        }
        let released = match event {
            InputEvent::Key { code, down, .. } if binding.key_code == Some(code) => !down,
            InputEvent::Flags(flags) if !binding.exact_modifiers(flags) => {
                if binding.required_modifiers_down(flags) {
                    return self.cancel();
                }
                true
            }
            InputEvent::Key { down: true, .. }
            | InputEvent::MouseDown
            | InputEvent::TapDisabled => return self.cancel(),
            _ => false,
        };
        if !released {
            return Effect::Wait;
        }
        self.0 = None;
        if at.duration_since(start) >= Duration::from_millis(300) {
            Effect::Browse
        } else {
            Effect::PasteLatest
        }
    }

    pub fn tick(&mut self, at: CaptureInstant) -> Effect {
        if self
            .0
            .is_some_and(|(_, start)| at.duration_since(start) >= Duration::from_millis(300))
        {
            self.0 = None;
            Effect::Browse
        } else {
            Effect::Wait
        }
    }

    pub fn cancel(&mut self) -> Effect {
        self.0 = None;
        Effect::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_rebound_key_browses_once_at_threshold_and_never_prepastes() {
        let binding = RuntimeHotkey {
            modifiers: Default::default(),
            key_code: Some(100),
        };
        let at = |ms| CaptureInstant::from_nanos(ms * 1_000_000u64);
        let up = InputEvent::Key {
            code: 100,
            down: false,
            flags: 0,
        };
        let mut gesture = Gesture::default();
        assert_eq!(gesture.press(binding, at(1), true), Effect::Wait);
        assert_eq!(gesture.edge(up, at(300)), Effect::PasteLatest);
        assert_eq!(gesture.press(binding, at(400), true), Effect::Wait);
        assert_eq!(gesture.tick(at(700)), Effect::Browse);
        assert_eq!(gesture.edge(up, at(710)), Effect::Wait);
        assert_eq!(gesture.press(binding, at(800), false), Effect::PasteLatest);
        assert_eq!(gesture.tick(at(1200)), Effect::Wait);
    }
}
