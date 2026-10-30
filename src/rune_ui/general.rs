//! The General pane: one card of the basics (shortcut, speech model,
//! language, microphone, launch at login), each row led by an icon, and
//! every other setting behind the pane's Advanced fold. Also the pieces the
//! other panes' rows share: the fold, the quiet select, and the shortcut's
//! keys.

use rune::{El, col, row, text};
use rune_kit::{Placement, Select, SelectAppearance};

use super::controls::{
    self, ButtonKind, CardRow, DEVICE_SELECT_WIDTH, KeySize, METRICS, Paint, button, card,
    copy_row, folding, icon_button, icon_described_row, icon_row, icon_toggle_row, key_tray,
    keycap, note, page, pressable_row, row_switch, section, text_field,
};
use super::icons::*;
use super::lists::{PermissionControl, permission_control, slider};
use super::platform::PlatformRequest;
use super::settings::{self as s, Capture, Face};
use super::{Menu, Pane, Request, ShortcutTarget, Ui};
use crate::app_settings::{IndicatorPosition, RecordingAudioBehavior, UpdateChannel};
use crate::desktop_host::DesktopAction;
use crate::desktop_i18n::{tr, tr_fill, tr_named};
use crate::dictation::ShortcutMode;
use crate::login_item::LoginItemStatus;
use crate::onboarding::PermissionKind;
use crate::transcription_models;

/// The missing permissions, the listener (Windows and Linux), the basics,
/// names and terms where they exist, then Advanced.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    use super::platform_settings as os;
    let mut content = page("content")
        .children(os::permissions_section(ui, p))
        .children(os::error_section(ui, p));
    if ui.platform.caps.listener_control {
        content = content.child(s::listener_card(ui, p));
    }
    if !ui.records_shortcuts() {
        content = content.child(s::desktop_commands(p));
    }
    content = content
        .child(basics(ui, p, layer))
        .child(super::vocabulary::section_view(ui, p))
        .child(advanced_toggle(ui, p, Pane::General));
    if advanced_open(ui, Pane::General) {
        content = content
            .child(section(
                p,
                "more-dictation",
                tr("Dictation"),
                dictation(ui, p, layer),
            ))
            .child(section(p, "output", tr("Output"), output(ui, p, layer)))
            .child(section(
                p,
                "application",
                tr("Application"),
                application(ui, p, layer),
            ));
    }
    super::scroll_body("scroll", content)
}

// ---------------------------------------------------------------------------
// The basics

fn basics(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let model = transcription_models::definition(ui.settings.transcription.model);
    let mut rows: Vec<CardRow> = Vec::new();
    if ui.records_shortcuts() {
        rows.extend(shortcut_rows(
            ui,
            p,
            "dictation-shortcut",
            &KEYBOARD,
            tr("Shortcut"),
            ShortcutTarget::Dictation,
            None,
        ));
        rows.extend(configure_shortcuts_row(ui, p).map(CardRow::from));
    }
    rows.push(
        icon_described_row(
            p,
            "transcription",
            &WAVE,
            tr("Speech model"),
            s::transcription_status(ui, p),
            value_button(
                p,
                "transcription-control",
                model.name,
                &rune_theme_opencode::icons::CHEVRON_RIGHT,
                super::picker::open,
            ),
        )
        .into(),
    );
    rows.push(language_row(ui, p, layer).into());
    rows.extend(microphone_rows(ui, p, layer));
    rows.extend(launch_at_login_rows(ui, p));
    card(p, "basics", rows)
}

/// A shortcut's row with what a shortcuts portal granted, and the note when
/// the desktop kept it.
pub(super) fn shortcut_rows(
    ui: &Ui,
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    target: ShortcutTarget,
    purpose: Option<&str>,
) -> Vec<CardRow> {
    let mut control = row()
        .key(format!("{key}-control"))
        .gap(4.0)
        .center_y()
        .child(keys_button(ui, p, target));
    if target == ShortcutTarget::PasteLast && ui.settings.paste_last_hotkey.is_some() {
        control = control.child(icon_button(
            p,
            "clear-paste-last",
            &rune_theme_opencode::icons::TRASH,
            tr("Clear shortcut"),
            |ui: &mut Ui| {
                ui.update(|settings| settings.paste_last_hotkey = None);
            },
        ));
    }
    let mut rows: Vec<CardRow> = vec![
        icon_described_row(
            p,
            key,
            glyph,
            title,
            s::granted(ui, p, target)
                .or_else(|| purpose.map(|purpose| (purpose.to_string(), p.muted()))),
            control,
        )
        .into(),
    ];
    rows.extend(s::refused_note(ui, p, target).map(CardRow::from));
    rows
}

/// What other audio can do while dictating; Lower audio only where Hex can.
fn audio_behaviors(ui: &Ui) -> Vec<RecordingAudioBehavior> {
    RecordingAudioBehavior::ALL
        .into_iter()
        .filter(|behavior| *behavior != RecordingAudioBehavior::Duck || ui.platform.caps.duck_audio)
        .collect()
}

/// The desktop's own dialog for the shortcuts its portal bound, where the
/// user changes a trigger the desktop kept from an earlier binding.
fn configure_shortcuts_row(ui: &Ui, p: Paint) -> Option<El> {
    ui.view
        .as_ref()
        .is_some_and(|view| view.listener.shortcuts_configurable)
        .then(|| {
            icon_row(
                p,
                "configure-shortcuts",
                &KEYBOARD,
                tr("Desktop shortcuts"),
                Some(tr("Change the keys your desktop gave Hex")),
                button(
                    p,
                    "configure-shortcuts-control",
                    tr("Open"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.send(Request::Desktop(DesktopAction::ConfigureShortcuts)),
                ),
            )
        })
}

/// "Also pause the browser", under Pause media on macOS. Hex hears the
/// browser through System Audio Recording, so turning it on asks for that
/// here: never while you dictate.
fn pause_browser_rows(ui: &Ui, p: Paint) -> Vec<El> {
    let on = ui.settings.pause_browser_media;
    let mut rows = vec![icon_toggle_row(
        p,
        "pause-browser",
        &GLOBE,
        tr("Also pause the browser"),
        tr("YouTube and other tabs, resumed only if Hex paused them"),
        on,
        |ui: &mut Ui| {
            let on = !ui.settings.pause_browser_media;
            ui.update(|settings| settings.pause_browser_media = on);
            if on && !ui.platform.system_audio {
                ui.ask(PlatformRequest::SystemAudio);
            }
        },
    )];
    if on {
        let control = if ui.platform.system_audio {
            PermissionControl::Allowed
        } else if ui.platform.asked(PermissionKind::SystemAudio) {
            PermissionControl::Waiting
        } else {
            PermissionControl::Allow
        };
        rows.push(icon_row(
            p,
            "pause-browser-audio",
            &SPEAKER,
            tr("Screen & System Audio Recording"),
            Some(tr("Lets Hex hear whether a tab is playing")),
            permission_control(
                p,
                "pause-browser-allow",
                control,
                ButtonKind::Secondary,
                |ui: &mut Ui| ui.ask(PlatformRequest::SystemAudio),
            ),
        ));
    }
    rows
}

/// The model's languages as a quiet select, or its one language as text.
fn language_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let selection = &ui.settings.transcription;
    let model = transcription_models::definition(selection.model);
    let current = transcription_models::language_name(&selection.language);
    let codes = model.language_choices();
    let control = if codes.len() <= 1 {
        // The text lines up with the values above and below it.
        row()
            .key("transcription-language-control")
            .h(24.0)
            .pr(24.0)
            .center_y()
            .child(
                text(current.to_string())
                    .key("value")
                    .font(value_font(p))
                    .color(p.faint()),
            )
    } else {
        let labels: Vec<&'static str> = codes
            .iter()
            .map(|code| transcription_models::language_name(code))
            .collect();
        let selected = codes
            .iter()
            .position(|code| *code == selection.language)
            .unwrap_or(0);
        pick_select(
            ui,
            p,
            "transcription-language",
            labels,
            selected,
            |ui, index| {
                let model = transcription_models::definition(ui.settings.transcription.model);
                if let Some(code) = model.language_choices().get(index) {
                    super::picker::choose_language(ui, code);
                }
            },
            layer,
        )
    };
    icon_row(
        p,
        "transcription-language",
        &LANGUAGES,
        tr("Language"),
        super::picker::fallback_note(ui).as_deref(),
        control,
    )
}

/// The microphone with the input it resolves to, why another input stands
/// in, and the listing's error while its menu is open.
fn microphone_rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let settings = &ui.settings;
    let listed = ui.microphones.as_ref().ok();
    let default_name = listed.and_then(|devices| devices.system_default.as_deref());
    let missing = s::missing_microphone(ui);
    // A MacBook's built-in microphone is off while its lid is closed.
    let off = listed.and_then(|devices| devices.off_with_lid_closed.as_deref());
    let saved_off = settings
        .microphone
        .as_deref()
        .filter(|saved| off == Some(*saved));
    // The value always names the input in use, so a missing saved
    // microphone shows the system default standing in for it.
    let (microphone, detail) = match (&settings.microphone, missing.or(saved_off)) {
        (Some(saved), None) => (saved.clone(), None),
        _ => (
            tr("System default").to_owned(),
            default_name.map(str::to_owned),
        ),
    };
    let mac = ui.platform.caps.os == super::platform::Os::Macos;
    let lid_note = |built_in: &str| match default_name {
        Some(name) => tr_named(
            "{built_in} is off while the lid is closed, so Hex listens to {input}",
            &[("built_in", built_in), ("input", name)],
        ),
        None => tr_fill("{} is off while the lid is closed", built_in),
    };
    let microphone_note = listed.and_then(|devices| match &settings.microphone {
        Some(_) if saved_off.is_some() => saved_off.map(lid_note),
        Some(_) => missing.map(|saved| {
            if mac {
                tr_fill("{} isn't connected; using your Mac's input", saved)
            } else {
                tr_fill("{} isn't connected; using the system input", saved)
            }
        }),
        None if devices.default_is_hex_microphone => default_name.map(|name| {
            tr_fill(
                "Hex Microphone is the system default, so Hex listens to {}",
                name,
            )
        }),
        None if devices.default_is_off_with_lid_closed => off.map(lid_note),
        None => devices
            .bluetooth_default_passed_over
            .as_deref()
            .zip(default_name)
            .map(|(bluetooth, input)| {
                tr_named(
                    "{bluetooth} is Bluetooth, so Hex listens to {input}",
                    &[("bluetooth", bluetooth), ("input", input)],
                )
            }),
    });
    vec![
        icon_row(
            p,
            "microphone",
            &MIC,
            tr("Microphone"),
            microphone_note.as_deref(),
            mic_value(ui, p, &microphone, detail.as_deref(), layer),
        )
        .into(),
        folding(
            "devices-error-fold",
            match (ui.open, &ui.microphones) {
                (Some(Menu::Microphone), Err(error)) => {
                    Some(note("devices-error", error, p.danger()))
                }
                _ => None,
            },
        ),
    ]
}

/// The level and the input in use, drawn like the kit's borderless select,
/// opening every input.
fn mic_value(ui: &Ui, p: Paint, microphone: &str, detail: Option<&str>, layer: &mut Vec<El>) -> El {
    let c = &p.k.c;
    let label = match detail {
        Some(detail) => format!("{microphone}, {detail}"),
        None => microphone.to_owned(),
    };
    let trigger = row()
        .key("microphone-control")
        .role("button")
        .label(label)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .h(24.0)
        .pl(6.0)
        .pr(4.0)
        .gap(6.0)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(|ui: &mut Ui| s::toggle_menu(ui, Menu::Microphone))
        .child(super::onboarding::mic_bars(p, 0.0, p.faint()))
        .child(
            text(detail.unwrap_or(microphone).to_string())
                .key("value")
                .font(value_font(p))
                .color(c.text_base)
                .truncate(),
        )
        .child(
            rune::icon(p.k.glyph.chevron_down, 16.0)
                .key("chevron")
                .color(c.icon_muted),
        );
    let mut menu = rune_kit::Menu::new("microphone-menu")
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .min_width(DEVICE_SELECT_WIDTH)
        .open(ui.open == Some(Menu::Microphone))
        .on_close(|ui: &mut Ui| ui.open = None);
    for choice in s::microphone_choices(ui) {
        let mut item = rune_kit::MenuItem::new(choice.label).checked(choice.selected);
        if let Some(detail) = choice.detail {
            item = item.shortcut(detail);
        }
        menu = menu.item(item.on_select(move |ui: &mut Ui| (choice.choose)(ui)));
    }
    menu.build(p.k, layer)
}

/// Launch at login, driven by the OS's login item: macOS's `SMAppService`
/// (which may need approval) or Windows's `Run` key.
fn launch_at_login_rows(ui: &Ui, p: Paint) -> Vec<CardRow> {
    if !ui.platform.caps.launch_at_login {
        return Vec::new();
    }
    let status = ui.platform.login_item;
    let control = match status {
        None => text(if ui.platform.login_item_checking {
            tr("Checking…")
        } else {
            tr("Unavailable")
        })
        .key("login-item-status")
        .font(controls::DESCRIPTION)
        .color(p.muted()),
        Some(LoginItemStatus::RequiresApproval) => button(
            p,
            "launch-at-login-approval",
            tr("Open settings"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.ask(PlatformRequest::OpenLoginItemSettings),
        ),
        Some(_) => row_switch(p, ui.platform.login_item_on()),
    };
    let mut row = icon_row(
        p,
        "launch-at-login",
        &POWER,
        tr("Launch at login"),
        None,
        control,
    );
    if matches!(
        status,
        Some(LoginItemStatus::Enabled | LoginItemStatus::Disabled)
    ) {
        row = pressable_row(row).on_press(|ui: &mut Ui| {
            let enabled = !ui.platform.login_item_on();
            ui.ask(PlatformRequest::SetLaunchAtLogin(enabled));
        });
    }
    let mut rows: Vec<CardRow> = vec![row.into()];
    if let Some(error) = &ui.platform.login_item_error {
        rows.push(note("launch-at-login-error", error, p.danger()).into());
    }
    rows
}

// ---------------------------------------------------------------------------
// Advanced

/// How the shortcut triggers, the microphone's idle release, and the speech
/// model's hints.
fn dictation(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    use super::platform_settings::{SIDES, picked_side, side_binding};
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = Vec::new();
    if ui.records_shortcuts()
        && let Some(side) = picked_side(ui, ShortcutTarget::Dictation)
    {
        rows.push(side_row(
            ui,
            p,
            "dictation-side",
            side,
            |ui, index| {
                let target = ShortcutTarget::Dictation;
                if let Some(candidate) = side_binding(&ui.settings, target, SIDES[index]) {
                    ui.update(|settings| s::set_binding(settings, target, candidate));
                }
            },
            layer,
        ));
    }
    let toggle_mode = settings.shortcut_mode == ShortcutMode::Toggle;
    rows.push(
        copy_row(
            p,
            "shortcut-mode",
            Some(&HAND),
            tr("Shortcut mode"),
            Some(super::explainer::link(p)),
            pick_select(
                ui,
                p,
                "shortcut-mode",
                vec![tr("Hold"), tr("Toggle")],
                usize::from(toggle_mode),
                |ui, index| {
                    let mode = if index == 1 {
                        ShortcutMode::Toggle
                    } else {
                        ShortcutMode::Hold
                    };
                    if ui.settings.shortcut_mode != mode {
                        ui.update(|settings| settings.shortcut_mode = mode);
                    }
                },
                layer,
            ),
            true,
        )
        .into(),
    );
    rows.push(folding(
        "double-tap-fold",
        (!toggle_mode).then(|| {
            icon_toggle_row(
                p,
                "double-tap",
                &LOCK,
                tr("Double-tap to lock"),
                None,
                settings.double_tap_lock,
                super::explainer::flip_lock,
            )
        }),
    ));
    rows.push(folding(
        "double-tap-only-fold",
        s::double_tap_only_applies(settings).then(|| {
            icon_toggle_row(
                p,
                "double-tap-only",
                &TIMER,
                tr("Double-tap only"),
                tr("Wait for two complete shortcut taps before recording"),
                settings.double_tap_only,
                |ui: &mut Ui| {
                    let enabled = !ui.settings.double_tap_only;
                    ui.update(|settings| settings.double_tap_only = enabled);
                },
            )
        }),
    ));
    rows.push(
        icon_row(
            p,
            "microphone-mode",
            &ZAP,
            tr("Microphone mode"),
            settings
                .release_microphone_while_idle
                .then(|| tr("Opens only while holding the shortcut. Disables pre-roll.")),
            pick_select(
                ui,
                p,
                "microphone-mode",
                vec![tr("Keep ready (fast)"), tr("Release when idle")],
                usize::from(settings.release_microphone_while_idle),
                |ui, index| {
                    let release = index == 1;
                    if release && !ui.settings.release_microphone_while_idle {
                        ui.confirm_release = true;
                        return;
                    }
                    ui.confirm_release = false;
                    if ui.settings.release_microphone_while_idle != release {
                        ui.update(|settings| settings.release_microphone_while_idle = release);
                    }
                },
                layer,
            ),
        )
        .into(),
    );
    rows.push(folding(
        "release-confirmation-fold",
        ui.confirm_release.then(|| s::release_confirmation(ui, p)),
    ));
    if ui.platform.caps.keyboard_model_switching {
        rows.push(
            icon_toggle_row(
                p,
                "keyboard-model-switching",
                &KEYBOARD,
                tr("Switch model with keyboard language"),
                tr(
                    "Changing input source selects its mapped model and language; manual choices still work",
                ),
                settings.keyboard_model_switching.enabled,
                |ui: &mut Ui| {
                    ui.open = None;
                    ui.update(|settings| {
                        settings.keyboard_model_switching.enabled =
                            !settings.keyboard_model_switching.enabled;
                    });
                },
            )
            .into(),
        );
        rows.push(folding(
            "input-source-mappings",
            if settings.keyboard_model_switching.enabled {
                s::input_source_rows(ui, p, layer)
            } else {
                Vec::new()
            },
        ));
    }
    if ui.platform.caps.os == super::platform::Os::Macos {
        rows.push(
            icon_toggle_row(
                p,
                "avoid-bluetooth-microphone",
                &BLUETOOTH,
                tr("Use the Mac's microphone instead of Bluetooth"),
                tr("Keeps AirPods and other headsets out of call-quality audio"),
                settings.avoid_bluetooth_microphone,
                |ui: &mut Ui| {
                    let enabled = !ui.settings.avoid_bluetooth_microphone;
                    ui.update(|settings| settings.avoid_bluetooth_microphone = enabled);
                },
            )
            .into(),
        );
    }
    if ui.platform.caps.share_shortcut {
        rows.push(
            icon_toggle_row(
                p,
                "share-dictation-shortcut",
                &KEYBOARD,
                tr("Share dictation shortcut with other apps"),
                tr("Lets apps such as Discord detect the same key"),
                settings.share_dictation_shortcut,
                |ui: &mut Ui| {
                    let enabled = !ui.settings.share_dictation_shortcut;
                    ui.update(|settings| settings.share_dictation_shortcut = enabled);
                },
            )
            .into(),
        );
    }
    let model = transcription_models::definition(settings.transcription.model);
    if model.supports_recognition_hints {
        rows.push(
            icon_row(
                p,
                "hints",
                &HINTS,
                tr("Recognition hints"),
                Some(tr("Names and terms to prime the speech model")),
                text_field(
                    p,
                    "hints-field",
                    &ui.hints,
                    tr("Names and terms Whisper should expect, e.g. OpenCode, Effect…"),
                    |ui: &mut Ui| &mut ui.hints,
                    |ui: &mut Ui, _| {
                        let hints = ui.hints.text.clone();
                        ui.update(|settings| settings.transcription.recognition_hints = hints);
                    },
                ),
            )
            .into(),
        );
    }
    rows
}

/// Paste last, what happens to other audio while dictating, the HUD, and
/// terminal paste (Linux).
fn output(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = Vec::new();
    if ui.records_shortcuts() {
        rows.extend(shortcut_rows(
            ui,
            p,
            "paste-last",
            &PASTE,
            tr("Paste last dictation"),
            ShortcutTarget::PasteLast,
            None,
        ));
    }
    let behaviors = audio_behaviors(ui);
    rows.push(pick_row(
        ui,
        p,
        "while-dictating",
        &MUTE,
        tr("While dictating"),
        behaviors
            .iter()
            .map(|behavior| tr(behavior.label()))
            .collect(),
        behaviors
            .iter()
            .position(|behavior| *behavior == settings.recording_audio_behavior)
            .unwrap_or(2),
        |ui, index| {
            let behavior = audio_behaviors(ui)[index];
            if ui.settings.recording_audio_behavior != behavior {
                ui.update(|settings| settings.recording_audio_behavior = behavior);
            }
        },
        layer,
    ));
    if ui.platform.caps.duck_audio
        && settings.recording_audio_behavior == RecordingAudioBehavior::Duck
    {
        let volume = ui
            .duck_volume_drag
            .unwrap_or(settings.recording_duck_volume);
        rows.push(
            icon_row(
                p,
                "recording-duck-volume",
                &SPEAKER,
                tr("Lower to"),
                Some(tr("Percent of the current output volume")),
                slider(
                    p,
                    "recording-duck-slider",
                    tr("Volume while recording"),
                    volume,
                    format!("{:.0}%", volume * 100.0),
                    200.0,
                    |ui, value, done| {
                        let value = (value * 100.0).round() / 100.0;
                        ui.duck_volume_drag = if done { None } else { Some(value) };
                        if done {
                            ui.update(|settings| settings.recording_duck_volume = value);
                        }
                    },
                ),
            )
            .into(),
        );
    }
    if ui.platform.caps.os == super::platform::Os::Macos
        && settings.recording_audio_behavior == RecordingAudioBehavior::PauseMedia
    {
        rows.extend(pause_browser_rows(ui, p).into_iter().map(CardRow::from));
    }
    rows.push(pick_row(
        ui,
        p,
        "indicator",
        &PANEL,
        tr("Dictation HUD"),
        vec![tr("Top"), tr("Bottom"), tr("Off")],
        match settings.indicator_position {
            IndicatorPosition::Top => 0,
            IndicatorPosition::Bottom => 1,
            IndicatorPosition::Hidden => 2,
        },
        |ui, index| {
            let position = [
                IndicatorPosition::Top,
                IndicatorPosition::Bottom,
                IndicatorPosition::Hidden,
            ][index];
            if ui.settings.indicator_position != position {
                ui.update(|settings| settings.indicator_position = position);
            }
        },
        layer,
    ));
    if ui.platform.caps.terminal_paste {
        rows.push(
            icon_toggle_row(
                p,
                "terminal-paste",
                &TERMINAL,
                tr("Terminal paste"),
                tr("Paste with Ctrl+Shift+V so terminals accept it"),
                settings.paste_with_shift,
                |ui: &mut Ui| {
                    let enabled = !ui.settings.paste_with_shift;
                    ui.update(|settings| settings.paste_with_shift = enabled);
                },
            )
            .into(),
        );
    }
    rows
}

/// Enable on launch (Linux and Windows), the Dock icon, sounds, the
/// interface language, and the update channel.
fn application(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = Vec::new();
    if ui.platform.caps.listener_control {
        rows.push(
            icon_toggle_row(
                p,
                "enable-on-launch",
                &PLAY,
                tr("Enable on launch"),
                None,
                settings.enable_on_launch,
                |ui: &mut Ui| {
                    let enabled = !ui.settings.enable_on_launch;
                    ui.update(|settings| settings.enable_on_launch = enabled);
                },
            )
            .into(),
        );
    }
    if ui.platform.caps.dock_icon {
        rows.push(
            icon_toggle_row(
                p,
                "dock-icon",
                &DOCK,
                tr("Show Dock icon"),
                None,
                settings.show_dock_icon,
                |ui: &mut Ui| {
                    let show = !ui.settings.show_dock_icon;
                    ui.update(|settings| settings.show_dock_icon = show);
                },
            )
            .into(),
        );
    }
    rows.push(pick_row(
        ui,
        p,
        "sound-volume",
        &VOLUME,
        tr("Sound volume"),
        s::SOUND_VOLUME_STEPS
            .iter()
            .map(|(label, _)| tr(label))
            .collect(),
        s::sound_volume_index(settings),
        |ui, index| {
            let volume = s::SOUND_VOLUME_STEPS[index].1;
            let saved = ui.update(|settings| {
                settings.sound_effects = volume > 0.0;
                if volume > 0.0 {
                    settings.sound_effect_volume = volume;
                }
            });
            if saved && volume > 0.0 && ui.live() {
                crate::feedback::play(crate::feedback::Tone::DictationStart);
            }
        },
        layer,
    ));
    rows.push(
        icon_row(
            p,
            "sound-set",
            &MUSIC,
            tr("Sounds"),
            None,
            value_button(
                p,
                "sound-set-control",
                tr(settings.sound_theme.name()),
                &rune_theme_opencode::icons::CHEVRON_RIGHT,
                super::sound_sets::open,
            ),
        )
        .into(),
    );
    let languages = crate::desktop_i18n::LANGUAGE_CHOICES;
    rows.push(pick_row(
        ui,
        p,
        "interface-language",
        &GLOBE,
        tr("Interface language"),
        languages.iter().map(|(_, name)| tr(name)).collect(),
        languages
            .iter()
            .position(|(code, _)| *code == settings.ui_language.as_deref())
            .unwrap_or(0),
        |ui, index| {
            let code = crate::desktop_i18n::LANGUAGE_CHOICES[index]
                .0
                .map(str::to_string);
            if ui.update(|settings| settings.ui_language = code.clone()) {
                crate::desktop_i18n::apply(code.as_deref());
            }
        },
        layer,
    ));
    if s::updates_itself(ui) {
        let shown = settings
            .updates
            .channel
            .unwrap_or_else(crate::updater::build_channel);
        rows.push(pick_row(
            ui,
            p,
            "update-channel",
            &REFRESH,
            tr("Update channel"),
            UpdateChannel::ALL
                .iter()
                .map(|channel| tr(channel.label()))
                .collect(),
            UpdateChannel::ALL
                .iter()
                .position(|item| *item == shown)
                .unwrap_or(0),
            |ui, index| {
                // Choosing the channel already in effect saves nothing, so an
                // unset channel keeps following the build.
                let channel = UpdateChannel::ALL[index];
                let shown = ui
                    .settings
                    .updates
                    .channel
                    .unwrap_or_else(crate::updater::build_channel);
                if channel != shown
                    && ui.update(|settings| settings.updates.channel = Some(channel))
                {
                    ui.send(Request::Desktop(DesktopAction::CheckForUpdates));
                }
            },
            layer,
        ));
    }
    rows
}

/// Left, Either, or Right for a modifier-only shortcut (macOS); sides that
/// would collide with another shortcut are left out.
pub(super) fn side_row(
    ui: &Ui,
    p: Paint,
    key: &'static str,
    side: crate::app_settings::ModifierSide,
    pick: fn(&mut Ui, usize),
    layer: &mut Vec<El>,
) -> CardRow {
    use super::platform_settings::SIDES;
    pick_row(
        ui,
        p,
        key,
        &SIDE,
        tr("Shortcut side"),
        vec![tr("Left"), tr("Either"), tr("Right")],
        SIDES.iter().position(|s| *s == side).unwrap_or(1),
        pick,
        layer,
    )
}

// ---------------------------------------------------------------------------
// Shared pieces

/// Each pane's Advanced fold is closed until pressed.
pub(super) fn advanced_open(ui: &Ui, pane: Pane) -> bool {
    ui.advanced.contains(&pane)
}

/// The disclosure for a pane's settings most people never change, which
/// follow it.
pub(super) fn advanced_toggle(ui: &Ui, p: Paint, pane: Pane) -> El {
    let c = &p.k.c;
    let open = advanced_open(ui, pane);
    row()
        .key("advanced-toggle")
        .role("button")
        .label(tr("Advanced"))
        .h(18.0)
        .px(6.0)
        .ml(METRICS.row_px - 6.0)
        .gap(2.0)
        .center_y()
        .self_start()
        .radius(p.k.radius.md)
        .hover(c.hover)
        .press(c.pressed)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .on_press(move |ui: &mut Ui| {
            if let Some(index) = ui.advanced.iter().position(|open| *open == pane) {
                ui.advanced.remove(index);
                ui.confirm_release = false;
            } else {
                ui.advanced.push(pane);
            }
        })
        .child(
            text(tr("Advanced"))
                .key("title")
                .font(p.k.ty.ui.lh(16.0))
                .color(c.text_muted),
        )
        .child(
            rune::icon(
                if open {
                    p.k.glyph.triangle_down
                } else {
                    p.k.glyph.triangle_right
                },
                14.0,
            )
            .key("arrow")
            .color(c.icon_weaker),
        )
}

/// An icon row choosing one of `labels` from a quiet select.
#[allow(clippy::too_many_arguments)]
pub(super) fn pick_row(
    ui: &Ui,
    p: Paint,
    key: &'static str,
    glyph: &'static rune::Icon,
    title: &str,
    labels: Vec<&'static str>,
    selected: usize,
    pick: fn(&mut Ui, usize),
    layer: &mut Vec<El>,
) -> CardRow {
    let control = pick_select(ui, p, key, labels, selected, pick, layer);
    icon_row(p, key, glyph, title, None, control).into()
}

/// The kit's borderless select, keyed `{key}-control`: the value and a
/// chevron, no field.
pub(super) fn pick_select(
    ui: &Ui,
    p: Paint,
    key: &'static str,
    labels: Vec<&'static str>,
    selected: usize,
    pick: fn(&mut Ui, usize),
    layer: &mut Vec<El>,
) -> El {
    quiet_select(
        ui,
        p,
        Menu::Pick(key),
        format!("{key}-control"),
        labels,
        selected,
        pick,
        layer,
    )
}

/// [`pick_select`] for rows made at run time, opened as `menu`.
#[allow(clippy::too_many_arguments)]
pub(super) fn quiet_select(
    ui: &Ui,
    p: Paint,
    menu: Menu,
    key: String,
    labels: Vec<&'static str>,
    selected: usize,
    pick: impl Fn(&mut Ui, usize) + 'static,
    layer: &mut Vec<El>,
) -> El {
    let choices = labels.clone();
    let pick = std::rc::Rc::new(pick);
    Select::new(key, &labels)
        .appearance(SelectAppearance::Inline)
        .current(labels.get(selected).copied())
        .open(ui.open == Some(menu))
        .placement(Placement::BottomEnd)
        .on_toggle(move |ui: &mut Ui| s::toggle_menu(ui, menu))
        .on_close(|ui: &mut Ui| ui.open = None)
        .on_choose(move |ui: &mut Ui, value: &str, _: &mut rune::Tx| {
            ui.open = None;
            if let Some(index) = choices.iter().position(|label| *label == value) {
                pick(ui, index);
            }
        })
        .build(p.k, layer)
}

fn value_font(p: Paint) -> rune::TextStyle {
    p.k.ty.ui_medium.lh(16.0)
}

/// A value that opens something, drawn like the kit's borderless select.
pub(super) fn value_button(
    p: Paint,
    key: &str,
    label: &str,
    glyph: &'static rune::Icon,
    press: impl Fn(&mut Ui) + 'static,
) -> El {
    let c = &p.k.c;
    row()
        .key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .h(24.0)
        .pl(8.0)
        .pr(4.0)
        .gap(4.0)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(press)
        .child(
            text(label.to_string())
                .key("value")
                .font(value_font(p))
                .color(c.text_base)
                .truncate(),
        )
        .child(rune::icon(glyph, 16.0).key("chevron").color(c.icon_muted))
}

/// A shortcut's keys, pressed to record a new one; while recording, the
/// recorder's prompt in their place.
pub(super) fn keys_button(ui: &Ui, p: Paint, target: ShortcutTarget) -> El {
    let c = &p.k.c;
    let face = s::face(ui, target);
    let listening = matches!(
        ui.capture,
        Capture::Listening { target: active, .. } if active == target
    );
    let content = match face {
        Face::Listening(_) => s::recorder_face(p, face, true).key("face"),
        Face::Shortcut { saved } => {
            let keys = match s::binding(&ui.settings, target) {
                Some(binding) => {
                    let caps = s::shortcut_caps(binding, false);
                    key_tray(
                        p,
                        "keys",
                        KeySize::Small,
                        caps.iter()
                            .enumerate()
                            .map(|(index, cap)| keycap(p, index, cap, KeySize::Small, false)),
                    )
                }
                None => row().key("off").h(24.0).px(8.0).center().child(
                    text(tr("Off"))
                        .key("label")
                        .font(value_font(p))
                        .color(p.faint()),
                ),
            };
            row()
                .key("face")
                .gap(4.0)
                .center_y()
                .children(saved.then(|| {
                    rune::icon(&rune_theme_opencode::icons::CHECK_SMALL, 16.0)
                        .key("saved")
                        .color(p.success())
                }))
                .child(keys)
        }
    };
    let mut button = row()
        .key(s::hotkey_key(target))
        .role("button")
        .label(s::face_label(face))
        .center_y()
        .padding(2.0)
        .radius(p.k.radius.md)
        // While listening, keys reach the recorder rather than pressing it again.
        .focusable(!listening)
        .focus_ring(2.0, 0.0, c.border_focus)
        .on_press(move |ui: &mut Ui| s::begin_capture(ui, target))
        .child(col().key("content").child(content));
    if listening {
        button = button.fill(c.bg_button_neutral);
    } else {
        button = button.hover(c.hover).press(c.pressed);
    }
    button
}
