//! The window's navigation registry. Entries contain public UI copy and a
//! stable row key, never a field's current value, action, or user text.
use super::*;
use crate::app_settings::RecordingAudioBehavior;
use crate::labs::Feature;

pub(super) fn entries(ui: &Ui) -> Vec<Entry> {
    let caps = ui.platform.caps;
    let mut all = Vec::new();
    let mut add = |pane: Pane, key, advanced, title, about: &'static str| {
        all.push(Entry {
            link: Link::Setting(pane, key, advanced),
            title,
            about: about.into(),
            path: if advanced {
                format!("{} › {}", pane.title(), tr("Advanced"))
            } else {
                pane.title().into()
            },
        });
    };
    let g = Pane::General;
    if ui.records_shortcuts() {
        add(
            g,
            "dictation-shortcut",
            false,
            tr("Shortcut"),
            tr("The keys you press to dictate"),
        );
        add(
            g,
            "shortcut-mode",
            false,
            tr("Shortcut mode"),
            tr("How the dictation shortcut starts and stops recording"),
        );
        if ui.settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle
            && !matches!(
                ui.shortcuts(),
                Some(crate::desktop::host::DesktopShortcuts::Portal(_))
            )
        {
            add(
                g,
                "cancel-shortcut",
                false,
                tr("Cancel"),
                tr("Discards the recording"),
            );
        }
        if Feature::SecondDictationShortcut.on(&ui.settings) {
            add(
                g,
                "second-dictation-shortcut",
                false,
                tr("Second dictation shortcut"),
                tr("Dictates like the shortcut above"),
            );
        }
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        if Feature::TerminalPasteShortcut.on(&ui.settings) {
            add(
                g,
                "terminal-paste-shortcut",
                false,
                tr("Terminal paste shortcut"),
                "",
            );
        }
        if platform_settings::picked_side(ui, ShortcutTarget::Dictation).is_some() {
            add(g, "dictation-side", true, tr("Shortcut side"), "");
        }
        add(
            g,
            "paste-last",
            true,
            tr("Paste last dictation"),
            tr("Shortcut to paste your last dictation again"),
        );
        add(
            Pane::Output,
            "raw-dictation",
            false,
            tr("Dictate without processing"),
            tr("Pastes your words as Hex heard them"),
        );
    }
    add(g, "transcription", false, tr("Speech model"), "");
    if crate::transcription::models::definition(ui.settings.transcription.model)
        .language_choices()
        .len()
        > 1
    {
        add(g, "transcription-language", false, tr("Language"), "");
    }
    add(g, "vocabulary", false, tr("Vocabulary"), "");
    add(g, "microphone", false, tr("Microphone"), "");
    add(
        g,
        "microphone-mode",
        false,
        tr("Turn off the mic when not dictating"),
        "",
    );
    if caps.launch_at_login && ui.platform.login_item.is_some() {
        add(g, "launch-at-login", false, tr("Launch at login"), "");
    }
    add(
        g,
        "avoid-bluetooth-microphone",
        true,
        if caps.os == platform::Os::Macos {
            tr("Use the Mac's microphone instead of Bluetooth")
        } else {
            tr("Use another microphone instead of Bluetooth")
        },
        tr("Keeps AirPods and other headsets out of call-quality audio"),
    );
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    if caps.os != platform::Os::Macos {
        add(g, "speech-engine", true, tr("Speech engine"), "");
    }
    add(g, "while-dictating", true, tr("While dictating"), "");
    if ui.settings.recording_audio_behavior == RecordingAudioBehavior::Duck {
        add(
            g,
            "recording-duck-volume",
            true,
            tr("Lower to"),
            tr("Percent of the current output volume"),
        );
    }
    add(g, "indicator", true, tr("Dictation HUD"), "");
    if caps.terminal_paste {
        add(
            g,
            "terminal-paste",
            true,
            tr("Terminal paste"),
            tr("Paste with Ctrl+Shift+V so terminals accept it"),
        );
    }
    if caps.listener_control {
        add(g, "enable-on-launch", true, tr("Enable on launch"), "");
    }
    if caps.dock_icon {
        add(g, "dock-icon", true, tr("Show Dock icon"), "");
    }
    add(g, "sound-set", true, tr("Sound volume"), "");
    add(
        g,
        "sound-set",
        true,
        tr("Sounds"),
        tr("Preview the tones Hex plays when you dictate."),
    );
    add(g, "interface-language", true, tr("Interface language"), "");
    if settings::updates_itself(ui) {
        add(g, "update-channel", true, tr("Update channel"), "");
    }
    all.extend(
        labs::search_entries(ui).map(|(lab, title, about, group)| Entry {
            link: Link::Lab(lab),
            title,
            about,
            path: format!("{} › {}", tr("Labs"), group),
        }),
    );
    all
}
