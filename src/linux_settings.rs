use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use x11rb::protocol::xproto::ModMask;
use x11rb::rust_connection::RustConnection;

use crate::linux_input::{Keymap, XK_ALT_L, XK_ALT_R};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LinuxSettings {
    pub schema_version: u32,
    pub platform: String,
    pub dictation_hotkey: LinuxHotkey,
    pub double_tap_lock: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LinuxHotkey {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

impl Default for LinuxSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            platform: "x11".into(),
            dictation_hotkey: LinuxHotkey::default(),
            double_tap_lock: true,
        }
    }
}

impl Default for LinuxHotkey {
    fn default() -> Self {
        Self {
            control: false,
            alt: true,
            shift: false,
            super_key: false,
            key: "space".into(),
        }
    }
}

impl LinuxSettings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let settings: Self = serde_json::from_slice(&fs::read(&path)?)
            .wrap_err_with(|| format!("could not parse {}", path.display()))?;
        if settings.platform != "x11" {
            return Err(eyre!(
                "unsupported Linux settings platform: {}",
                settings.platform
            ));
        }
        settings.dictation_hotkey.validate()?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        self.dictation_hotkey.validate()?;
        let path = settings_path()?;
        let directory = path.parent().unwrap();
        fs::create_dir_all(directory)?;
        let partial = path.with_extension("json.partial");
        let mut file = File::create(&partial)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(partial, &path)?;
        File::open(directory)?.sync_all()?;
        Ok(())
    }
}

impl LinuxHotkey {
    pub fn validate(&self) -> Result<()> {
        if self.key.trim().is_empty() {
            return Err(eyre!("Linux hotkeys require a non-modifier key"));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.control {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.super_key {
            parts.push("Super".to_string());
        }
        let key = if self.key == "space" {
            "Space".into()
        } else {
            self.key.to_ascii_uppercase()
        };
        parts.push(key);
        parts.join("+")
    }

    pub(crate) fn modifier_mask(
        &self,
        connection: &RustConnection,
        keymap: &Keymap,
    ) -> Result<ModMask> {
        let mut result = ModMask::default();
        for (enabled, symbols) in [
            (self.control, &[0xffe3, 0xffe4][..]),
            (self.alt, &[XK_ALT_L, XK_ALT_R][..]),
            (self.shift, &[0xffe1, 0xffe2][..]),
            (self.super_key, &[0xffeb, 0xffec][..]),
        ] {
            if enabled {
                result |= keymap.modifier_for(connection, symbols)?;
            }
        }
        Ok(result)
    }
}

fn settings_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("linux-settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_binding_is_alt_space() {
        assert_eq!(
            LinuxSettings::default().dictation_hotkey.label(),
            "Alt+Space"
        );
    }

    #[test]
    fn modifier_only_bindings_are_rejected() {
        let binding = LinuxHotkey {
            key: String::new(),
            ..LinuxHotkey::default()
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn standalone_function_keys_are_accepted() {
        let binding = LinuxHotkey {
            alt: false,
            key: "f12".into(),
            ..LinuxHotkey::default()
        };
        assert_eq!(binding.label(), "F12");
        assert!(binding.validate().is_ok());
    }
}
