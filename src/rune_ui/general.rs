//! The General pane: one card of the basics (shortcut with its side,
//! shortcut mode, speech model, vocabulary, microphone, launch at login),
//! each row led by an icon, and every other setting behind one disclosure,
//! "Microphone, sound and window", in four groups: while you dictate,
//! microphone and model, pasting, and the app. Also the pieces the other
//! panes' rows share: the fold, the quiet select, and the shortcut's keys.

mod mic_level;

use rune::{El, col, row, text};
use rune_kit::{Placement, Select, SelectAppearance};

use super::controls::{
    self, ButtonKind, CardRow, DEVICE_SELECT_WIDTH, KeySize, METRICS, Paint, button, card, folding,
    icon_button, icon_described_row, icon_row, icon_toggle_row, key_tray, keycap, note, page,
    pressable_row, row_switch, section, value_button, value_font,
};
use super::icons::*;
use super::lists::{PermissionControl, permission_control, slider};
use super::platform::PlatformRequest;
use super::settings::{self as s, Capture, Face};
use super::{Menu, Pane, Request, ShortcutTarget, Ui};
use crate::app_settings::{IndicatorPosition, RecordingAudioBehavior, UpdateChannel};
use crate::desktop::host::DesktopAction;
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::desktop::login_item::LoginItemStatus;
use crate::desktop::onboarding::PermissionKind;
use crate::transcription::models as transcription_models;
pub(super) use mic_level::{MicLevel, poll_level};

/// The missing permissions, the listener (Windows and Linux), the basics,
/// names and terms where they exist, then the disclosure's groups.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    use super::platform_settings as os;
    let mut content = page("content")
        .children(os::permissions_section(ui, p))
        .children(os::error_section(ui, p));
    if ui.platform.caps.listener_control {
        content = content.child(s::listener_card(ui, p));
    }
    if !ui.records_shortcuts() {
        content = content.child(s::desktop_commands(ui, p));
    }
    content = content.child(basics(ui, p, layer)).child(disclosure(
        ui,
        p,
        Pane::General,
        tr("Microphone, sound and window"),
    ));
    if advanced_open(ui, Pane::General) {
        let groups = [
            (
                "while-you-dictate",
                tr("While you dictate"),
                while_you_dictate(ui, p, layer),
            ),
            (
                "microphone-and-model",
                tr("Microphone and model"),
                microphone_and_model(ui, p, layer),
            ),
            ("pasting", tr("Pasting"), pasting(ui, p)),
            ("app", tr("App"), app(ui, p, layer)),
        ];
        for (key, title, rows) in groups {
            if !rows.is_empty() {
                content = content.child(section(p, key, title, rows));
            }
        }
    }
    super::scroll_body(p.k, "scroll", content)
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
        rows.push(super::shortcut_mode::row(ui, p).into());
        // Toggle has a cancel shortcut; Hold has none.
        if ui.settings.shortcut_mode == crate::dictation::ShortcutMode::Toggle {
            rows.extend(cancel_rows(ui, p));
        }
        // Labs › Second dictation shortcut (macOS): another key that
        // dictates like the one above, such as on an external keyboard.
        #[cfg(target_os = "macos")]
        if crate::labs::Feature::SecondDictationShortcut.on(&ui.settings) {
            rows.extend(shortcut_rows(
                ui,
                p,
                "second-dictation-shortcut",
                &KEYBOARD,
                tr("Second dictation shortcut"),
                ShortcutTarget::SecondDictation,
                Some(tr("Dictates like the shortcut above")),
            ));
        }
        // Annotate's shortcut beside dictation's on Linux, where the desktop
        // lists both and a lone F9 is otherwise easy to miss (#723).
        #[cfg(all(target_os = "linux", hex_capture))]
        if ui.platform.scrawl.is_some() && ui.settings.capture.enabled {
            rows.extend(shortcut_rows(
                ui,
                p,
                "annotate-shortcut",
                &super::sidebar::SCRAWL_ICON,
                tr("Annotate"),
                ShortcutTarget::Capture,
                None,
            ));
        }
        rows.extend(configure_shortcuts_row(ui, p).map(CardRow::from));
    }
    let picker_button = value_button(
        p,
        "transcription-control",
        tr("Speech model"),
        model.name,
        &rune_theme_opencode::icons::CHEVRON_RIGHT,
        super::picker::open,
    );
    let (description, control) = match missing_model(ui) {
        // Hex can't listen without the selected model: say so, with the
        // download's progress, or Download and Add model file… when none
        // runs, as after Continue without a model (#894).
        Some(progress) => {
            let description = match (
                progress.downloading,
                super::model_import::download_failure(ui),
            ) {
                (Some(_), _) => (
                    tr("Hex can't listen until the speech model has downloaded").to_string(),
                    p.muted(),
                ),
                (None, Some(error)) => {
                    (tr_fill("No speech model installed: {}", &error), p.danger())
                }
                (None, None) => (
                    tr("No speech model installed. Download one or add a model file.").to_string(),
                    p.danger(),
                ),
            };
            let controls = row().key("transcription-controls").gap(8.0).center_y();
            let controls = match progress.downloading {
                Some(fraction) => controls.child(
                    rune_kit::Progress::new(Some(fraction))
                        .show_value()
                        .build(p.k)
                        .key("transcription-progress")
                        .w(120.0),
                ),
                None => controls
                    .child(button(
                        p,
                        "transcription-download",
                        tr("Download"),
                        ButtonKind::Secondary,
                        |ui: &mut Ui, _| {
                            let selection = ui.settings.transcription.clone();
                            ui.send(Request::ChooseModel {
                                model: selection.model,
                                language: selection.language,
                            });
                        },
                    ))
                    .child(super::model_import::use_file_button(ui, p)),
            };
            (Some(description), controls.child(picker_button))
        }
        None => (s::transcription_status(ui, p), picker_button),
    };
    rows.push(
        icon_described_row(
            p,
            "transcription",
            &WAVE,
            tr("Speech model"),
            description,
            control,
        )
        .into(),
    );
    rows.extend(language_row(ui, p, layer).map(CardRow::from));
    let vocabulary_note = super::vocabulary::general_note(ui);
    rows.push(
        icon_row(
            p,
            "vocabulary",
            &BOOK,
            tr("Vocabulary"),
            vocabulary_note.as_deref(),
            value_button(
                p,
                "vocabulary-control",
                tr("Vocabulary"),
                &super::vocabulary::summary(ui),
                &rune_theme_opencode::icons::CHEVRON_RIGHT,
                super::vocabulary::open,
            ),
        )
        .into(),
    );
    rows.extend(microphone_rows(ui, p, layer));
    rows.extend(launch_at_login_rows(ui, p));
    card(p, "basics", rows)
}

/// The selected speech model's download, while it is not installed, so
/// Hex can't listen yet. Fixtures that name no installed models say nothing,
/// so their rows never depend on this machine's models.
pub(super) fn missing_model(ui: &Ui) -> Option<crate::desktop::onboarding::ModelProgress> {
    if !ui.live() && ui.picker.installed.is_none() {
        return None;
    }
    // A saved import that is missing or changed: Hex listens with a
    // downloaded model in its place, and the row's notice says how to fix it.
    if ui.settings.transcription.model.is_labs_gated()
        && super::model_import::selection_notice(ui).is_some()
    {
        let fallback = crate::transcription::custom_models::fallback();
        let model = transcription_models::definition(fallback.model);
        if ui.picker.installed(model, &fallback.language) {
            return None;
        }
    }
    let (installed, progress) = super::onboarding::progress(ui);
    (!installed).then_some(progress)
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
        .children(side_picker(ui, p, target))
        .child(keys_button(ui, p, target));
    // The optional shortcuts can be turned off.
    #[cfg(target_os = "macos")]
    let second = target == ShortcutTarget::SecondDictation;
    #[cfg(not(target_os = "macos"))]
    let second = false;
    let optional = second
        || matches!(
            target,
            ShortcutTarget::PasteLast | ShortcutTarget::RawDictation
        );
    if optional && s::binding(&ui.settings, target).is_some() {
        control = control.child(icon_button(
            p,
            &format!("clear-{key}"),
            &rune_theme_opencode::icons::TRASH,
            tr("Clear shortcut"),
            move |ui: &mut Ui| {
                ui.update(|settings| match target {
                    ShortcutTarget::RawDictation => settings.raw_dictation_hotkey = None,
                    _ if second => settings.second_dictation_hotkey = None,
                    _ => settings.paste_last_hotkey = None,
                });
            },
        ));
    }
    // Labs › Restore shortcut default: Paste last dictation's default,
    // offered while the shortcut is something else or off. Cancel's
    // default, Escape, can't be recorded (Escape ends the recorder), so its
    // restore is always offered.
    let mut default_note = None;
    let default = match target {
        ShortcutTarget::PasteLast
            if crate::labs::Feature::RestoreShortcutDefault.on(&ui.settings) =>
        {
            Some(crate::app_settings::HotkeyBinding::paste_last_default())
        }
        ShortcutTarget::Cancel => Some(crate::app_settings::HotkeyBinding::cancel_default()),
        _ => None,
    };
    if let Some(default) = default {
        let current = s::binding(&ui.settings, target);
        if current.is_none() {
            default_note = Some((
                tr_fill("Default shortcut: {}", &default.keycaps().join("+")),
                p.muted(),
            ));
        }
        if current != Some(&default) {
            control = control.child(icon_button(
                p,
                &format!("restore-{key}"),
                &REFRESH,
                tr("Restore default shortcut"),
                move |ui: &mut Ui| s::restore_default(ui, target, default.clone()),
            ));
        }
    }
    let mut rows: Vec<CardRow> = vec![
        icon_described_row(
            p,
            key,
            glyph,
            title,
            default_note
                .or_else(|| s::granted(ui, p, target))
                .or_else(|| purpose.map(|purpose| (purpose.to_string(), p.muted()))),
            control,
        )
        .into(),
    ];
    rows.extend(s::refused_note(ui, p, target).map(CardRow::from));
    rows.extend(s::gnome_clash_note(ui, p, target).map(CardRow::from));
    rows
}

/// The side a modifier-only dictation shortcut answers to (macOS): Left,
/// Either, or Right, before its keys. A side that would collide with another
/// shortcut is refused.
fn side_picker(ui: &Ui, p: Paint, target: ShortcutTarget) -> Option<El> {
    use super::platform_settings::{SIDES, side_binding};
    let side = shown_side(ui, target)?;
    let labels = [tr("Left"), tr("Either"), tr("Right")];
    Some(
        controls::segmented(
            p,
            "dictation-side",
            &labels,
            SIDES.iter().position(|s| *s == side).unwrap_or(1),
            |ui: &mut Ui, index| {
                let target = ShortcutTarget::Dictation;
                if let Some(candidate) = side_binding(&ui.settings, target, SIDES[index]) {
                    ui.update(|settings| s::set_binding(settings, target, candidate));
                }
            },
        )
        .label(tr("Shortcut side"))
        .mr(8.0),
    )
}

/// The side [`side_picker`] shows for `target`: only dictation's
/// modifier-only shortcut has one, so its keys leave the side to it. While
/// the shortcut records or shows Saved, the recorder takes the picker's
/// place and its keys name the side, so the recorder's right edge and the
/// picker, when it shows, hold still.
fn shown_side(ui: &Ui, target: ShortcutTarget) -> Option<crate::app_settings::ModifierSide> {
    let recording = match ui.capture {
        Capture::Idle => false,
        Capture::Listening { target: active, .. }
        | Capture::Saving { target: active, .. }
        | Capture::Saved { target: active, .. } => active == target,
    };
    (target == ShortcutTarget::Dictation && !recording)
        .then(|| super::platform_settings::picked_side(ui, target))
        .flatten()
}

/// Toggle's cancel shortcut. A shortcuts portal would hold its key from
/// every app for as long as Hex runs, so on Wayland it says to bind
/// `hex dictation cancel` in the desktop instead.
fn cancel_rows(ui: &Ui, p: Paint) -> Vec<CardRow> {
    if matches!(
        ui.shortcuts(),
        Some(crate::desktop::host::DesktopShortcuts::Portal(_))
    ) {
        return vec![
            icon_row(
                p,
                "cancel-shortcut",
                &CIRCLE_X,
                tr("Cancel"),
                Some(tr(
                    "Bind hex dictation cancel to a key in your desktop's keyboard shortcuts",
                )),
                row().key("cancel-shortcut-control"),
            )
            .into(),
        ];
    }
    shortcut_rows(
        ui,
        p,
        "cancel-shortcut",
        &CIRCLE_X,
        tr("Cancel"),
        ShortcutTarget::Cancel,
        Some(tr("Discards the recording")),
    )
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

/// Whether the microphone closes between dictations: one switch, no
/// confirmation, and a line saying what happens either way (issues 61 and
/// #870). The default keeps it ready.
fn microphone_release_row(ui: &Ui, p: Paint) -> CardRow {
    let release = ui.settings.release_microphone_while_idle;
    icon_toggle_row(
        p,
        "microphone-mode",
        &ZAP,
        tr("Turn off the mic when not dictating"),
        if release {
            tr("On: the mic opens as you dictate; the first moment can be clipped")
        } else if ui.platform.caps.os == super::platform::Os::Macos {
            tr("Off: dictation starts instantly; macOS shows the mic in use")
        } else {
            tr("Off: dictation starts instantly; your system shows the mic in use")
        },
        release,
        |ui: &mut Ui| {
            let release = !ui.settings.release_microphone_while_idle;
            ui.update(|settings| settings.release_microphone_while_idle = release);
        },
    )
    .into()
}

/// Labs › Also pause the browser's permission, under Pause media on macOS:
/// Hex hears the browser through System Audio Recording, asked for here,
/// never while you dictate.
fn pause_browser_rows(ui: &Ui, p: Paint) -> Vec<El> {
    let on = ui.settings.pause_browser_media;
    let mut rows = Vec::new();
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

/// The model's languages as a quiet select, or the fallback note when
/// switching to a single-language model changed the saved language.
fn language_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Option<El> {
    let selection = &ui.settings.transcription;
    let model = transcription_models::definition(selection.model);
    let current = transcription_models::language_name(&selection.language);
    let codes = model.language_choices();
    let fallback = super::picker::fallback_note(ui);
    if codes.len() <= 1 && fallback.is_none() {
        return None;
    }
    let control = if codes.len() <= 1 {
        row()
            .key("transcription-language-control")
            .h(24.0)
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
            tr("Language"),
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
    Some(icon_row(
        p,
        "transcription-language",
        &LANGUAGES,
        tr("Language"),
        fallback.as_deref(),
        control,
    ))
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
    // Read as the microphone choice with the input in use as its value. The
    // level bars stay out of the tree: they change many times a second.
    let trigger = row()
        .key("microphone-control")
        .role("button")
        .label(tr("Microphone"))
        .a11y_value(label)
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
        .child(ui.mic_level.bars(p).a11y_hidden())
        .child(
            text(detail.unwrap_or(microphone).to_string())
                .key("value")
                .font(value_font(p))
                .color(c.text_base)
                .truncate()
                .a11y_hidden(),
        )
        .child(
            rune::icon(p.k.glyph.chevron_down, 16.0)
                .key("chevron")
                .color(c.icon_muted)
                .a11y_hidden(),
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
    menu.build(p.k, layer).mr(-9.0)
}

/// Launch at login, driven by the OS's login item: macOS's `SMAppService`
/// (which may need approval), Windows's `Run` key, or Linux's XDG autostart
/// entry.
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
        row = pressable_row(row, ui.platform.login_item_on()).on_press(|ui: &mut Ui| {
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
// Microphone, sound and window

/// What happens around a dictation: other audio, the sounds, and the HUD.
fn while_you_dictate(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = Vec::new();
    let behaviors = RecordingAudioBehavior::ALL;
    let behavior = settings.recording_audio_behavior;
    let volume = ui
        .duck_volume_drag
        .unwrap_or(settings.recording_duck_volume);
    let percent = format!("{:.0}", volume * 100.0);
    let other_audio = match behavior {
        RecordingAudioBehavior::Mute => Some(tr("Music and calls go silent while you talk").into()),
        RecordingAudioBehavior::PauseMedia => {
            Some(tr("Music pauses while you talk and plays on after").into())
        }
        RecordingAudioBehavior::Duck => Some(tr_fill(
            "Music and calls drop to {}% while you talk",
            &percent,
        )),
        RecordingAudioBehavior::DoNothing => None::<String>,
    };
    rows.push(
        icon_row(
            p,
            "while-dictating",
            &MUTE,
            tr("Other audio"),
            other_audio.as_deref(),
            pick_select(
                ui,
                p,
                "while-dictating",
                tr("Other audio"),
                behaviors
                    .iter()
                    .map(|behavior| tr(behavior.label()))
                    .collect(),
                behaviors
                    .iter()
                    .position(|item| *item == behavior)
                    .unwrap_or(0),
                |ui, index| {
                    let behavior = RecordingAudioBehavior::ALL[index];
                    if ui.settings.recording_audio_behavior != behavior {
                        ui.update(|settings| settings.recording_audio_behavior = behavior);
                    }
                },
                layer,
            ),
        )
        .into(),
    );
    if behavior == RecordingAudioBehavior::Duck {
        // Under Other audio, its title in line with the rows' titles.
        rows.push(
            controls::setting_row(
                p,
                "recording-duck-volume",
                tr("Lower to"),
                None,
                slider(
                    p,
                    "recording-duck-slider",
                    tr("Volume while recording"),
                    volume,
                    format!("{percent}%"),
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
            .pl(METRICS.row_px + controls::ICON_LEAD)
            .into(),
        );
    }
    if ui.platform.caps.os == super::platform::Os::Macos
        && behavior == RecordingAudioBehavior::PauseMedia
    {
        rows.extend(pause_browser_rows(ui, p).into_iter().map(CardRow::from));
    }
    let sounds = if settings.sound_effects {
        format!(
            "{} · {:.0}%",
            tr(settings.sound_theme.name()),
            settings.sound_effect_volume * 100.0
        )
    } else {
        tr("Off").to_string()
    };
    rows.push(
        icon_row(
            p,
            "sound-set",
            &BELL,
            tr("Sounds"),
            None,
            value_button(
                p,
                "sound-set-control",
                tr("Sounds"),
                &sounds,
                &rune_theme_opencode::icons::CHEVRON_RIGHT,
                super::sound_sets::open,
            ),
        )
        .into(),
    );
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
    rows
}

/// The microphone's idle release, headsets, the keyboard-language model
/// mappings (Labs), and where the model runs (Linux and Windows).
fn microphone_and_model(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
    let settings = &ui.settings;
    let mac = ui.platform.caps.os == super::platform::Os::Macos;
    let mut rows: Vec<CardRow> = vec![microphone_release_row(ui, p)];
    rows.push(
        icon_toggle_row(
            p,
            "avoid-bluetooth-microphone",
            &BLUETOOTH,
            if mac {
                tr("Use the Mac's microphone with headsets")
            } else {
                tr("Use another microphone with headsets")
            },
            if mac {
                tr("AirPods keep playing in full quality while you dictate")
            } else {
                tr("Headsets keep playing in full quality while you dictate")
            },
            settings.avoid_bluetooth_microphone,
            |ui: &mut Ui| {
                let enabled = !ui.settings.avoid_bluetooth_microphone;
                ui.update(|settings| settings.avoid_bluetooth_microphone = enabled);
            },
        )
        .into(),
    );
    // Labs › Switch model with keyboard language: its mappings, while on.
    if ui.platform.caps.keyboard_model_switching {
        rows.push(folding(
            "input-source-mappings",
            if settings.keyboard_model_switching.enabled {
                s::input_source_rows(ui, p, layer)
            } else {
                Vec::new()
            },
        ));
    }
    // macOS runs its models on Metal or Apple's speech engine.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if !mac {
        rows.push(speech_engine_row(ui, p, layer));
    }
    rows
}

/// Paste last, terminal paste (Linux and Windows), and the clipboard note.
fn pasting(ui: &Ui, p: Paint) -> Vec<CardRow> {
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
    if clipboard_note_shown(ui) {
        rows.push(
            note(
                "clipboard-restore-note",
                tr("This desktop doesn't let Hex put your clipboard back after a paste. Turn on Labs › Restore clipboard on GNOME to try it."),
                p.muted(),
            )
            .into(),
        );
    }
    rows
}

/// Pasting's note on a Wayland desktop without data-control (GNOME) while
/// Labs › Restore clipboard on GNOME is off.
fn clipboard_note_shown(ui: &Ui) -> bool {
    ui.platform.clipboard_unrestorable
        && !crate::labs::Feature::GnomeClipboardRestore.on(&ui.settings)
}

/// Enable on launch (Linux and Windows), the Dock icon, the interface
/// language, and the update channel.
fn app(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<CardRow> {
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
    let languages = crate::desktop::i18n::LANGUAGE_CHOICES;
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
            let code = crate::desktop::i18n::LANGUAGE_CHOICES[index]
                .0
                .map(str::to_string);
            if ui.update(|settings| settings.ui_language = code.clone()) {
                crate::desktop::i18n::apply(code.as_deref());
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

// ---------------------------------------------------------------------------
// Shared pieces

/// Each pane's Advanced fold is closed until pressed.
pub(super) fn advanced_open(ui: &Ui, pane: Pane) -> bool {
    ui.advanced.contains(&pane)
}

/// The disclosure for a pane's settings most people never change, which
/// follow it.
pub(super) fn advanced_toggle(ui: &Ui, p: Paint, pane: Pane) -> El {
    disclosure(ui, p, pane, tr("Advanced"))
}

/// [`advanced_toggle`] named `title`, for a pane whose folded settings have
/// a name of their own.
pub(super) fn disclosure(ui: &Ui, p: Paint, pane: Pane, title: &str) -> El {
    let c = &p.k.c;
    let open = advanced_open(ui, pane);
    row()
        .key("advanced-toggle")
        .role("button")
        .label(title)
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
            } else {
                ui.advanced.push(pane);
            }
        })
        .child(
            text(title)
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

/// Where the speech model runs (#856): Auto, GPU or CPU, and why Auto
/// stays on the CPU after a GPU failed.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn speech_engine_row(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> CardRow {
    use crate::app_settings::TranscriptionDevice;
    let device = ui.settings.transcription_device;
    let control = pick_select(
        ui,
        p,
        "speech-engine",
        tr("Speech engine"),
        vec![tr("Auto"), tr("GPU"), tr("CPU")],
        match device {
            TranscriptionDevice::Auto => 0,
            TranscriptionDevice::Gpu => 1,
            TranscriptionDevice::Cpu => 2,
        },
        |ui, index| {
            let device = [
                TranscriptionDevice::Auto,
                TranscriptionDevice::Gpu,
                TranscriptionDevice::Cpu,
            ][index];
            if ui.settings.transcription_device != device {
                ui.update(|settings| settings.transcription_device = device);
            }
        },
        layer,
    );
    // Fixtures never read this machine's GPU record.
    let failed = (ui.live() && device == TranscriptionDevice::Auto)
        .then(crate::transcription::gpu_health::current)
        .flatten();
    // Without a Vulkan 1.2 driver, every choice transcribes on the CPU (#1240).
    #[cfg(target_os = "windows")]
    let vulkan_missing = crate::windows::vulkan::missing();
    #[cfg(target_os = "linux")]
    let vulkan_missing = false;
    let vulkan_note = (ui.live() && vulkan_missing).then(|| {
        tr("Hex uses the CPU because the graphics driver's Vulkan is missing or too old. Update the GPU driver to use the GPU.")
            .to_owned()
    });
    // Launched on CPU, Hex loaded no GPU driver (#1253).
    let restart_note = (ui.live() && crate::transcription::local_transcriber::gpu_after_restart())
        .then(|| tr("Turns on when Hex restarts").to_owned());
    let note = vulkan_note.or(restart_note).or_else(|| failed.map(|failure| {
        tr_named(
            if failure.slow {
                "Auto uses the CPU because it runs speech faster than {device}. Choose GPU to use it anyway."
            } else {
                "Auto uses the CPU because {device} stopped responding. Choose GPU to try it again."
            },
            &[("device", failure.device.as_str())],
        )
    }));
    icon_row(
        p,
        "speech-engine",
        &WAVE,
        tr("Speech engine"),
        note.as_deref(),
        control,
    )
    .into()
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
    let control = pick_select(ui, p, key, title, labels, selected, pick, layer);
    icon_row(p, key, glyph, title, None, control).into()
}

/// The kit's borderless select, keyed `{key}-control`: the value and a
/// chevron, no field. Assistive technology reads it as `name`, with the
/// chosen label as its value.
#[allow(clippy::too_many_arguments)]
pub(super) fn pick_select(
    ui: &Ui,
    p: Paint,
    key: &'static str,
    name: &str,
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
        name,
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
    name: &str,
    labels: Vec<&'static str>,
    selected: usize,
    pick: impl Fn(&mut Ui, usize) + 'static,
    layer: &mut Vec<El>,
) -> El {
    let choices = labels.clone();
    let pick = std::rc::Rc::new(pick);
    let current = labels.get(selected).copied().unwrap_or_default();
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
        .label(name)
        .a11y_value(current)
        .mr(-8.0)
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
                    let caps = s::shortcut_caps(binding, shown_side(ui, target).is_some());
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
        .mr(-3.0)
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
