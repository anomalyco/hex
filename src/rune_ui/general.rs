//! The General pane: one card of the basics (shortcut, speech model,
//! language, microphone, launch at login), each row led by an icon, and
//! every other setting behind the pane's Advanced fold. Also the pieces the
//! other panes' rows share: the fold, the quiet select, and the shortcut's
//! keys.

use std::path::Path;
use std::time::Duration;

use rune::{El, Tx, col, row, text};
use rune_kit::{Placement, Select, SelectAppearance};

use super::controls::{
    self, ButtonKind, CardRow, DEVICE_SELECT_WIDTH, KeySize, METRICS, Paint, button, card,
    copy_row, folding, icon_button, icon_described_row, icon_row, icon_toggle_row, key_tray,
    keycap, note, page, pressable_row, row_switch, section,
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
use crate::dictation::ShortcutMode;
use crate::transcription::models as transcription_models;

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
        content = content.child(s::desktop_commands(ui, p));
    }
    content = content
        .child(basics(ui, p, layer))
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
                &super::SCRAWL_ICON,
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
        model.name,
        &rune_theme_opencode::icons::CHEVRON_RIGHT,
        super::picker::open,
    );
    let (description, control) = match missing_model(ui) {
        // Hex can't listen without the selected model: say so, with the
        // download's progress, or a Download button when none runs.
        Some(progress) => {
            let error = ui
                .view
                .as_ref()
                .and_then(|view| view.transcription.error.clone())
                .filter(|_| progress.downloading.is_none());
            let description = match error {
                Some(error) => (error, p.danger()),
                None => (
                    tr("Hex can't listen until the speech model has downloaded").to_string(),
                    p.muted(),
                ),
            };
            let action = match progress.downloading {
                Some(fraction) => rune_kit::Progress::new(Some(fraction))
                    .show_value()
                    .build(p.k)
                    .key("transcription-progress")
                    .w(120.0),
                None => button(
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
                ),
            };
            (
                Some(description),
                row()
                    .key("transcription-controls")
                    .gap(8.0)
                    .center_y()
                    .child(action)
                    .child(picker_button),
            )
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
    let vocabulary_note = super::vocabulary::unused_note(ui);
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
                &super::vocabulary::summary(ui),
                &rune_theme_opencode::icons::CHEVRON_RIGHT,
                super::vocabulary::open,
            ),
        )
        .into(),
    );
    rows.extend(microphone_rows(ui, p, layer));
    rows.push(microphone_release_row(ui, p));
    rows.extend(launch_at_login_rows(ui, p));
    card(p, "basics", rows)
}

/// The selected speech model's download, while it is not installed, so
/// Hex can't listen yet. Fixtures that name no installed models say nothing,
/// so their rows never depend on this machine's models.
fn missing_model(ui: &Ui) -> Option<crate::desktop::onboarding::ModelProgress> {
    if !ui.live() && ui.picker.installed.is_none() {
        return None;
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
    // offered while the shortcut is something else or off.
    let mut default_note = None;
    if target == ShortcutTarget::PasteLast
        && crate::labs::Feature::RestoreShortcutDefault.on(&ui.settings)
    {
        let default = crate::app_settings::HotkeyBinding::paste_last_default();
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

/// Whether the microphone closes between dictations, beside the microphone
/// picker: one switch, no confirmation, and a line saying what it means
/// (issues 61 and #870's copy). The default keeps it ready.
fn microphone_release_row(ui: &Ui, p: Paint) -> CardRow {
    let release = ui.settings.release_microphone_while_idle;
    icon_toggle_row(
        p,
        "microphone-mode",
        &ZAP,
        tr("Turn off the mic when not dictating"),
        if release {
            tr("The mic opens only while you dictate; the first moment can be clipped")
        } else {
            tr("The mic stays ready so dictation starts instantly; your system shows it in use")
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
        .child(ui.mic_level.bars(p))
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
    menu.build(p.k, layer).mr(-9.0)
}

/// How often the microphone row follows the level while audio arrives.
const LEVEL_FRAME: Duration = Duration::from_millis(33);

/// What the microphone row's level shows: the audio Hex receives from the
/// microphone in use, or nothing while none arrives (released while idle,
/// failed, or not allowed), as flat, dimmed bars.
pub(super) struct MicLevel {
    /// 0 to 1.
    level: f32,
    pub(super) receiving: bool,
    /// The poll clock when the next level frame is due.
    frame_at: f64,
    /// Scenes: a recording played from virtual time zero instead of the
    /// microphone.
    recording: Option<(Vec<f32>, u32)>,
}

impl Default for MicLevel {
    /// A quiet room, as fixtures show it.
    fn default() -> Self {
        Self {
            level: 0.0,
            receiving: true,
            frame_at: 0.0,
            recording: None,
        }
    }
}

impl MicLevel {
    /// No audio arriving.
    pub(super) fn closed() -> Self {
        Self {
            receiving: false,
            ..Self::default()
        }
    }

    /// Plays the mono WAV at `path` from virtual time zero.
    pub(super) fn playing(path: &Path) -> Self {
        Self {
            recording: crate::audio::voice_filter::read_wav_mono_f32(path).ok(),
            ..Self::default()
        }
    }

    /// Takes the latest peak, `None` when no audio arrived lately; true
    /// when what shows changed.
    pub(super) fn hear(&mut self, peak: Option<f32>) -> bool {
        let before = self.shown();
        match peak {
            Some(peak) => {
                self.receiving = true;
                self.level = super::onboarding::follow_level(self.level, peak);
            }
            None => {
                self.receiving = false;
                self.level = 0.0;
            }
        }
        before != self.shown()
    }

    fn shown(&self) -> (bool, u8) {
        (self.receiving, super::onboarding::level_step(self.level))
    }

    /// Whether the bars show a level, so the next frames can move them.
    pub(super) fn moving(&self) -> bool {
        self.receiving && super::onboarding::level_step(self.level) > 0
    }

    /// The recording's peak over the frame before `clock`, or `None` once
    /// it has played.
    fn recording_peak(&self, clock: f64) -> Option<Option<f32>> {
        let (samples, rate) = self.recording.as_ref()?;
        let end = (clock * f64::from(*rate)) as usize;
        let start = end.saturating_sub((LEVEL_FRAME.as_secs_f64() * f64::from(*rate)) as usize);
        Some((start < samples.len()).then(|| {
            samples[start..end.min(samples.len())]
                .iter()
                .fold(0.0_f32, |peak, s| peak.max(s.abs()))
        }))
    }

    pub(super) fn bars(&self, p: Paint) -> El {
        let tint = if self.receiving {
            p.faint()
        } else {
            p.k.c.border_strong
        };
        super::onboarding::mic_bars(p, super::onboarding::shown_level(self.level), tint)
    }
}

/// Follows the microphone's level while General shows in a visible window
/// (a scene's recording also plays headless), waking at a UI rate only while
/// the bars show a level; the window's own tick notices audio rising again. It only reads the level the audio owner stores, so it
/// never opens a microphone, including one released while idle. True when
/// the row changed.
pub(super) fn poll_level(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    if ui.pane != Pane::General || ui.labs_open || ui.tour.is_some() {
        return false;
    }
    let visible = ui.window.as_ref().is_none_or(rune::AppHandle::is_visible);
    let (peak, playing) = match ui.mic_level.recording_peak(clock) {
        Some(peak) => (peak.or(Some(0.0)), peak.is_some()),
        None if ui.live() && visible => (crate::audio::input_level::recent(), true),
        // Other fixtures keep the level they set.
        _ => return false,
    };
    let level = &mut ui.mic_level;
    let changed = level.hear(peak);
    // Frames follow the bars only while they stand above rest (a level still
    // falling finishes, even once a recording has played) or a scene's
    // recording plays. A quiet room's noise shows nothing, so it waits for
    // the window's tick instead of waking the window 30 times a second.
    let recording = level.recording.is_some() && playing;
    if (level.moving() || recording) && clock >= level.frame_at {
        level.frame_at = clock + LEVEL_FRAME.as_secs_f64();
        tx.wake_after(LEVEL_FRAME);
    }
    changed
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

/// How the shortcut triggers and the microphone's idle release.
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
    rows.push(
        icon_toggle_row(
            p,
            "avoid-bluetooth-microphone",
            &BLUETOOTH,
            if ui.platform.caps.os == super::platform::Os::Macos {
                tr("Use the Mac's microphone instead of Bluetooth")
            } else {
                tr("Use another microphone instead of Bluetooth")
            },
            tr("Keeps AirPods and other headsets out of call-quality audio"),
            settings.avoid_bluetooth_microphone,
            |ui: &mut Ui| {
                let enabled = !ui.settings.avoid_bluetooth_microphone;
                ui.update(|settings| settings.avoid_bluetooth_microphone = enabled);
            },
        )
        .into(),
    );
    // macOS runs its models on Metal or Apple's speech engine.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if ui.platform.caps.os != super::platform::Os::Macos {
        rows.push(speech_engine_row(ui, p, layer));
    }
    rows
}

/// Paste last, what happens to other audio while dictating, the HUD, and
/// terminal paste (Linux and Windows).
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
    let behaviors = RecordingAudioBehavior::ALL;
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
            .unwrap_or(0),
        |ui, index| {
            let behavior = RecordingAudioBehavior::ALL[index];
            if ui.settings.recording_audio_behavior != behavior {
                ui.update(|settings| settings.recording_audio_behavior = behavior);
            }
        },
        layer,
    ));
    if settings.recording_audio_behavior == RecordingAudioBehavior::Duck {
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
    let note = failed.map(|failure| {
        tr_named(
            if failure.slow {
                "Auto uses the CPU because it runs speech faster than {device}. Choose GPU to use it anyway."
            } else {
                "Auto uses the CPU because {device} stopped responding. Choose GPU to try it again."
            },
            &[("device", failure.device.as_str())],
        )
    });
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
        .mr(-8.0)
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
        .mr(-9.0)
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
