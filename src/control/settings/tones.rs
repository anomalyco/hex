//! Resolve the enable flag and remembered volume together, before a batch
//! enters the per-key retry loop. Zero turns tones off without erasing the
//! level that turning them back on restores.

use super::{AppSettings, ControlError, ShortcutRules, invalid, keys, parse};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ToneState {
    enabled: bool,
    restore_level: f32,
}

#[derive(Default)]
pub(super) struct TonePatch {
    enabled: Option<bool>,
    volume: Option<f32>,
}

impl TonePatch {
    pub(super) fn single(name: &str, value: &Value) -> Option<Self> {
        match name {
            "sound_effects" => Some(Self {
                enabled: value.as_bool(),
                volume: None,
            }),
            "sound_effect_volume" => Some(Self {
                enabled: None,
                volume: value.as_f64().map(|v| v as f32),
            }),
            _ => None,
        }
    }

    pub(super) fn parse(
        values: &Map<String, Value>,
        shortcuts: &ShortcutRules,
    ) -> Result<Self, ControlError> {
        let value = |name: &str| -> Result<Option<Value>, ControlError> {
            let Some(value) = values.get(name) else {
                return Ok(None);
            };
            if let Some(error) = super::unavailable(name) {
                return Err(error);
            }
            let key = keys()
                .find(|key| key.key == name)
                .expect("tone key is registered");
            let raw = match value {
                Value::String(text) => text.clone(),
                _ => value.to_string(),
            };
            parse(key, &raw, shortcuts).map(Some)
        };
        Ok(Self {
            enabled: value("sound_effects")?.and_then(|v| v.as_bool()),
            volume: value("sound_effect_volume")?
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
        })
    }

    pub(super) fn resolve(self, old: &AppSettings) -> Result<ToneState, ControlError> {
        if self.enabled == Some(true) && self.volume == Some(0.0) {
            return Err(invalid(
                "sound_effect_volume",
                "zero volume conflicts with sound_effects=true",
            ));
        }
        let mut state = ToneState {
            enabled: old.sound_effects,
            restore_level: old.sound_effect_volume,
        };
        if let Some(volume) = self.volume {
            state.enabled = volume > 0.0;
            if volume > 0.0 {
                state.restore_level = volume;
            }
        }
        // An explicit enable flag wins over a positive volume: off plus a
        // new level stores that level for the next enable.
        if let Some(enabled) = self.enabled {
            state.enabled = enabled;
            if enabled && state.restore_level <= 0.0 {
                state.restore_level = AppSettings::default().sound_effect_volume;
            }
        }
        Ok(state)
    }
}

impl ToneState {
    pub(super) fn apply(self, settings: &mut AppSettings) {
        settings.sound_effects = self.enabled;
        settings.sound_effect_volume = self.restore_level;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules() -> ShortcutRules {
        ShortcutRules {
            key_named: |_| panic!("tone batches contain no shortcut"),
            rule: |_, _| panic!("tone batches contain no shortcut"),
        }
    }

    #[test]
    fn batches_resolve_tones_once_and_keep_the_restore_level() {
        let default = AppSettings::default().sound_effect_volume;
        for (enabled, level, patch, expected_enabled, expected_level) in [
            (true, 0.7, json!({"sound_effects": false}), false, 0.7),
            (false, 0.7, json!({"sound_effects": true}), true, 0.7),
            (false, 0.0, json!({"sound_effects": true}), true, default),
            (true, 0.7, json!({"sound_effect_volume": 0}), false, 0.7),
            (false, 0.7, json!({"sound_effect_volume": 0}), false, 0.7),
            (false, 0.7, json!({"sound_effect_volume": 0.25}), true, 0.25),
            (
                true,
                0.7,
                json!({"sound_effects": false, "sound_effect_volume": 0}),
                false,
                0.7,
            ),
            (
                true,
                0.7,
                json!({"sound_effects": false, "sound_effect_volume": 0.25}),
                false,
                0.25,
            ),
            (
                false,
                0.7,
                json!({"sound_effects": true, "sound_effect_volume": 0.25}),
                true,
                0.25,
            ),
            (
                false,
                0.7,
                json!({"sound_effects": "on", "sound_effect_volume": "0.25"}),
                true,
                0.25,
            ),
        ] {
            let old = AppSettings {
                sound_effects: enabled,
                sound_effect_volume: level,
                ..Default::default()
            };
            let change =
                super::super::apply_all(&old, patch.as_object().unwrap(), &rules()).unwrap();
            assert_eq!(
                (
                    change.settings.sound_effects,
                    change.settings.sound_effect_volume
                ),
                (expected_enabled, expected_level),
                "{patch}"
            );
            assert!(change.effects.is_empty());
            if patch.as_object().unwrap().len() == 1 {
                let (key, value) = patch.as_object().unwrap().iter().next().unwrap();
                let single = super::super::apply(&old, key, &value.to_string(), &rules()).unwrap();
                assert_eq!(single.settings, change.settings);
            }
        }
    }

    #[test]
    fn contradictory_batch_returns_no_settings_or_effects_to_commit() {
        for enabled in [false, true] {
            let old = AppSettings {
                sound_effects: enabled,
                sound_effect_volume: 0.7,
                ..Default::default()
            };
            let before = serde_json::to_value(&old).unwrap();
            let patch = json!({"sound_effects": true, "sound_effect_volume": 0, "updates.channel": "stable"});
            let result = super::super::apply_all(&old, patch.as_object().unwrap(), &rules());
            let error = result.expect_err("a refusal must never expose a Change to commit");
            assert_eq!(error.code, "invalid-value");
            assert!(error.message.contains("conflicts"));
            assert_eq!(serde_json::to_value(&old).unwrap(), before);
        }
    }
}
