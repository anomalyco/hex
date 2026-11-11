//! The onboarding's decoded cues, apart from the sets a person picks.

use super::*;

/// Decoded onboarding cues (`resources/audio/sounds/onboarding/`).
pub(super) struct OnboardingSounds {
    clack_keys: Vec<SamplesBuffer>,
    clack_space: SamplesBuffer,
    clack_backspace: SamplesBuffer,
    pub(super) felt_keys: [SamplesBuffer; 6],
    felt_space: SamplesBuffer,
    felt_backspace: SamplesBuffer,
    taptic_keys: [SamplesBuffer; 6],
    taptic_space: SamplesBuffer,
    soft_keys: [SamplesBuffer; 6],
    soft_space: SamplesBuffer,
    shortcut_down: SamplesBuffer,
    shortcut_up: SamplesBuffer,
    hexagon: SamplesBuffer,
    panel_escape: SamplesBuffer,
    permission_tick: SamplesBuffer,
    almost_ready: SamplesBuffer,
    build: [SamplesBuffer; 3],
    chat_incoming: SamplesBuffer,
    chat_sent: SamplesBuffer,
    chat_read: SamplesBuffer,
    chat_rewind: SamplesBuffer,
    chat_tapbacks: [SamplesBuffer; 6],
    heartbeat: SamplesBuffer,
    heartbeat_slow: SamplesBuffer,
    slow_down: SamplesBuffer,
    slam: SamplesBuffer,
    confetti: SamplesBuffer,
    fireworks: SamplesBuffer,
    triumphs: [SamplesBuffer; TRIUMPHS as usize],
    words: [SamplesBuffer; 3],
    burn: SamplesBuffer,
    mode_voice: SamplesBuffer,
}

impl OnboardingSounds {
    pub(super) fn load() -> Result<Self> {
        let tapback = decode_wav(bundled_sound!("onboarding", "chat_tapback"))?;
        let presses = [
            decode_wav(bundled_sound!("onboarding", "clack_1"))?,
            decode_wav(bundled_sound!("onboarding", "clack_2"))?,
            decode_wav(bundled_sound!("onboarding", "clack_3"))?,
            decode_wav(bundled_sound!("onboarding", "clack_4"))?,
            decode_wav(bundled_sound!("onboarding", "clack_5"))?,
            decode_wav(bundled_sound!("onboarding", "clack_6"))?,
            decode_wav(bundled_sound!("onboarding", "clack_7"))?,
            decode_wav(bundled_sound!("onboarding", "clack_8"))?,
        ];
        Ok(Self {
            clack_keys: CLACK_CENTS
                .iter()
                .flat_map(|&cents| presses.iter().map(move |press| detune(press, cents)))
                .collect(),
            clack_space: decode_wav(bundled_sound!("onboarding", "clack_space"))?,
            clack_backspace: decode_wav(bundled_sound!("onboarding", "clack_backspace"))?,
            felt_keys: [
                decode_wav(bundled_sound!("onboarding", "key_1"))?,
                decode_wav(bundled_sound!("onboarding", "key_2"))?,
                decode_wav(bundled_sound!("onboarding", "key_3"))?,
                decode_wav(bundled_sound!("onboarding", "key_4"))?,
                decode_wav(bundled_sound!("onboarding", "key_5"))?,
                decode_wav(bundled_sound!("onboarding", "key_6"))?,
            ],
            felt_space: decode_wav(bundled_sound!("onboarding", "key_space"))?,
            felt_backspace: decode_wav(bundled_sound!("onboarding", "key_backspace"))?,
            taptic_keys: [
                decode_wav(bundled_sound!("onboarding", "taptic_1"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_2"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_3"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_4"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_5"))?,
                decode_wav(bundled_sound!("onboarding", "taptic_6"))?,
            ],
            taptic_space: decode_wav(bundled_sound!("onboarding", "taptic_space"))?,
            soft_keys: [
                decode_wav(bundled_sound!("onboarding", "soft_key_1"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_2"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_3"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_4"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_5"))?,
                decode_wav(bundled_sound!("onboarding", "soft_key_6"))?,
            ],
            soft_space: decode_wav(bundled_sound!("onboarding", "soft_key_space"))?,
            shortcut_down: decode_wav(bundled_sound!("onboarding", "shortcut_down"))?,
            shortcut_up: decode_wav(bundled_sound!("onboarding", "shortcut_up"))?,
            hexagon: decode_wav(bundled_sound!("onboarding", "hexagon"))?,
            panel_escape: decode_wav(bundled_sound!("onboarding", "panel_escape"))?,
            permission_tick: decode_wav(bundled_sound!("onboarding", "permission_tick"))?,
            almost_ready: decode_wav(bundled_sound!("onboarding", "almost_ready"))?,
            build: [
                decode_wav(bundled_sound!("onboarding", "build_1"))?,
                decode_wav(bundled_sound!("onboarding", "build_2"))?,
                decode_wav(bundled_sound!("onboarding", "build_3"))?,
            ],
            chat_incoming: imessage::or_bundled(
                imessage::RECEIVED,
                &decode_wav(bundled_sound!("onboarding", "chat_incoming"))?,
            ),
            chat_sent: imessage::or_bundled(
                imessage::SENT,
                &decode_wav(bundled_sound!("onboarding", "chat_sent"))?,
            ),
            chat_read: decode_wav(bundled_sound!("onboarding", "chat_read"))?,
            chat_rewind: decode_wav(bundled_sound!("onboarding", "chat_rewind"))?,
            chat_tapbacks: Tapback::ALL
                .map(|kind| imessage::or_bundled(imessage::tapback(kind), &tapback)),
            heartbeat: decode_wav(bundled_sound!("onboarding", "heartbeat"))?,
            heartbeat_slow: decode_wav(bundled_sound!("onboarding", "heartbeat_slow"))?,
            slow_down: decode_wav(bundled_sound!("onboarding", "slow_down"))?,
            slam: decode_wav(bundled_sound!("onboarding", "slam"))?,
            confetti: decode_wav(bundled_sound!("onboarding", "confetti"))?,
            fireworks: decode_wav(bundled_sound!("onboarding", "fireworks"))?,
            triumphs: [
                decode_wav(bundled_sound!("onboarding", "triumph_1"))?,
                decode_wav(bundled_sound!("onboarding", "triumph_2"))?,
                decode_wav(bundled_sound!("onboarding", "triumph_3"))?,
            ],
            words: [
                decode_wav(bundled_sound!("onboarding", "word_1"))?,
                decode_wav(bundled_sound!("onboarding", "word_2"))?,
                decode_wav(bundled_sound!("onboarding", "word_3"))?,
            ],
            burn: decode_wav(bundled_sound!("onboarding", "card_burn"))?,
            mode_voice: decode_wav(bundled_sound!("onboarding", "mode_voice"))?,
        })
    }

    pub(super) fn get(&self, cue: OnboardingTone) -> Option<&SamplesBuffer> {
        Some(match cue {
            OnboardingTone::Key(KeySoundStyle::Silent, _)
            | OnboardingTone::KeySpace(KeySoundStyle::Silent)
            | OnboardingTone::KeyBackspace(KeySoundStyle::Silent) => return None,
            OnboardingTone::Key(KeySoundStyle::Clack, i) => {
                &self.clack_keys[usize::from(i) % self.clack_keys.len()]
            }
            OnboardingTone::KeySpace(KeySoundStyle::Clack) => &self.clack_space,
            OnboardingTone::KeyBackspace(KeySoundStyle::Clack) => &self.clack_backspace,
            OnboardingTone::Key(KeySoundStyle::Felt, i) => {
                &self.felt_keys[usize::from(i) % self.felt_keys.len()]
            }
            OnboardingTone::Key(KeySoundStyle::Taptic, i) => {
                &self.taptic_keys[usize::from(i) % self.taptic_keys.len()]
            }
            OnboardingTone::Key(KeySoundStyle::Soft, i) => {
                &self.soft_keys[usize::from(i) % self.soft_keys.len()]
            }
            OnboardingTone::KeySpace(KeySoundStyle::Felt) => &self.felt_space,
            OnboardingTone::KeySpace(KeySoundStyle::Taptic) => &self.taptic_space,
            OnboardingTone::KeySpace(KeySoundStyle::Soft) => &self.soft_space,
            OnboardingTone::KeyBackspace(KeySoundStyle::Felt) => &self.felt_backspace,
            OnboardingTone::KeyBackspace(KeySoundStyle::Taptic) => &self.taptic_keys[3],
            OnboardingTone::KeyBackspace(KeySoundStyle::Soft) => &self.soft_keys[3],
            OnboardingTone::ShortcutDown => &self.shortcut_down,
            OnboardingTone::ShortcutUp => &self.shortcut_up,
            OnboardingTone::Hexagon => &self.hexagon,
            OnboardingTone::PanelEscape => &self.panel_escape,
            OnboardingTone::PermissionTick => &self.permission_tick,
            OnboardingTone::AlmostReady => &self.almost_ready,
            OnboardingTone::Build(i) => &self.build[usize::from(i).min(2)],
            OnboardingTone::Incoming => &self.chat_incoming,
            OnboardingTone::Sent => &self.chat_sent,
            OnboardingTone::Read => &self.chat_read,
            OnboardingTone::Rewind => &self.chat_rewind,
            OnboardingTone::Tapback(kind) => &self.chat_tapbacks[kind as usize],
            OnboardingTone::Heartbeat => &self.heartbeat,
            OnboardingTone::HeartbeatSlow => &self.heartbeat_slow,
            OnboardingTone::SlowDown => &self.slow_down,
            OnboardingTone::Slam => &self.slam,
            OnboardingTone::Confetti => &self.confetti,
            OnboardingTone::Fireworks => &self.fireworks,
            OnboardingTone::Triumph(i) => &self.triumphs[usize::from(i) % self.triumphs.len()],
            OnboardingTone::Word(i) => &self.words[usize::from(i) % self.words.len()],
            OnboardingTone::Burn => &self.burn,
            OnboardingTone::ModeVoice => &self.mode_voice,
        })
    }
}
