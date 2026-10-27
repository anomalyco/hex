//! The General pane: listener status, the Dictation, Output, and Application
//! cards with the shortcuts in their place, Advanced options behind a
//! disclosure, and recording shortcuts through the host.

use std::time::{Duration, Instant};

use rune::{Edge, El, Key, KeyEvent, Transition, Tx, col, row, spacer, text};

use super::controls::{
    ButtonKind, CardRow, Choice, DESCRIPTION, DEVICE_SELECT_WIDTH, KEY_GAP, KeySize, METRICS,
    Paint, button, described_row, folding, icon_button, key_tray, keycap, keycaps, link_row, note,
    page, pulsing_dot, section, section_with, segmented, select, select_sized, setting_row,
    sheet_button, status_dot, text_field, toggle_row, tray_roll,
};
use super::{Menu, Request, Service, ShortcutTarget, Ui};
use super::{explainer, host};
use crate::app_settings::{
    AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, IndicatorPosition, ModifierSide,
    RecordingAudioBehavior, UpdateChannel,
};
use crate::desktop_host::{
    DesktopAction, DesktopGrantedShortcut, DesktopShortcut, DesktopShortcuts, DesktopUpdateStatus,
};
use crate::desktop_i18n::{tr, tr_fill};
use crate::dictation::ShortcutMode;
use crate::transcription_models::{self, ModelPreparationStage};

static CHECK: &rune::Icon = &rune_theme_opencode::icons::CHECK_SMALL;

const SOUND_VOLUME_STEPS: [(&str, f32); 5] = [
    ("Off", 0.0),
    ("25%", 0.25),
    ("50%", 0.5),
    ("75%", 0.75),
    ("100%", 1.0),
];
/// How long a newly saved shortcut shows "Saved".
const SAVED_FOR: Duration = Duration::from_secs(1);
/// How long to wait for the service to start a requested shortcut edit.
const EDIT_START_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Capture {
    Idle,
    /// Waiting for the chord; `message` explains a rejected one, and `held`
    /// shows the modifiers releasing them would record.
    Listening {
        target: ShortcutTarget,
        message: Option<&'static str>,
        held: HotkeyModifiers,
    },
    /// Sent to the service, which validates, registers, and saves it.
    Saving {
        target: ShortcutTarget,
        since: Instant,
        seen_edit: bool,
    },
    /// `until` is on the poll clock, set by the first poll that sees it.
    Saved {
        target: ShortcutTarget,
        until: Option<f64>,
    },
}

/// A chord the host recorded outside the window's key events (Windows
/// polls the keyboard, since modifier-only chords never reach the window).
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(super) enum Chord {
    /// The modifiers held so far, shown while the chord is in progress.
    Held(HotkeyModifiers),
    Captured(Result<HotkeyBinding, &'static str>),
    Cancelled,
}

/// Follows the host's side of a shortcut edit.
pub(super) fn advance_capture(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    loop {
        let chord = match &mut ui.service {
            Service::Live(backend) => backend.take_chord(),
            _ => None,
        };
        let (Some(chord), Capture::Listening { target, .. }) = (chord, ui.capture) else {
            break;
        };
        match chord {
            Chord::Held(modifiers) => {
                hold(ui, modifiers);
                changed = true;
            }
            Chord::Captured(binding) => {
                record(ui, target, binding);
                return true;
            }
            Chord::Cancelled => {
                cancel_capture(ui);
                return true;
            }
        }
    }
    if changed {
        return true;
    }
    let editing = ui.view.as_ref().is_some_and(|view| view.editing);
    match ui.capture {
        Capture::Saving {
            target,
            since,
            seen_edit,
        } => {
            let finished = if seen_edit {
                !editing
            } else if editing {
                ui.capture = Capture::Saving {
                    target,
                    since,
                    seen_edit: true,
                };
                return false;
            } else {
                since.elapsed() >= EDIT_START_TIMEOUT
            };
            if finished {
                ui.capture = Capture::Saved {
                    target,
                    until: None,
                };
            }
            finished
        }
        Capture::Saved {
            target,
            until: None,
        } => {
            ui.capture = Capture::Saved {
                target,
                until: Some(clock + SAVED_FOR.as_secs_f64()),
            };
            tx.wake_after(SAVED_FOR);
            false
        }
        Capture::Saved {
            until: Some(until), ..
        } if clock >= until => {
            ui.capture = Capture::Idle;
            true
        }
        _ => false,
    }
}

/// Records a shortcut while capture listens; otherwise leaves keys alone.
/// Where the host records chords itself, the window only takes Escape and
/// keeps every other key out of the page.
pub(super) fn on_key(ui: &mut Ui, event: &KeyEvent, _: &mut Tx) -> bool {
    let Capture::Listening { target, .. } = ui.capture else {
        return false;
    };
    if event.key == Key::Escape {
        cancel_capture(ui);
        return true;
    }
    if host::RECORDS_KEY_EVENTS {
        record(ui, target, recorded_binding(event));
    }
    true
}

/// Saves a recorded chord, or keeps listening with why it was rejected.
fn record(ui: &mut Ui, target: ShortcutTarget, binding: Result<HotkeyBinding, &'static str>) {
    let message = match binding.and_then(|binding| {
        validate(&ui.settings, target, &binding)?;
        Ok(binding)
    }) {
        Ok(binding) if host::HOST_SAVES_SHORTCUTS => {
            ui.send(Request::SetShortcut {
                target,
                shortcut: desktop_shortcut(&binding),
            });
            if matches!(ui.service, Service::Fixture(_)) {
                set_binding(&mut ui.settings, target, binding);
            }
            ui.capture = Capture::Saving {
                target,
                since: Instant::now(),
                seen_edit: false,
            };
            return;
        }
        Ok(binding) => {
            // The window saves the binding; the host restarts its listener
            // with it.
            cancel_capture(ui);
            if ui.update(|settings| set_binding(settings, target, binding)) {
                ui.capture = Capture::Saved {
                    target,
                    until: None,
                };
            }
            return;
        }
        Err(message) => message,
    };
    ui.capture = Capture::Listening {
        target,
        message: Some(message),
        held: HotkeyModifiers::default(),
    };
}

/// Shows the modifiers held so far; pressing any clears a rejection.
pub(super) fn hold(ui: &mut Ui, modifiers: HotkeyModifiers) {
    if let Capture::Listening { message, held, .. } = &mut ui.capture {
        if !modifiers.is_empty() {
            *message = None;
        }
        *held = modifiers;
    }
}

fn begin_capture(ui: &mut Ui, target: ShortcutTarget) {
    if let Capture::Listening { target: active, .. } = ui.capture {
        cancel_capture(ui);
        if active == target {
            return;
        }
    }
    ui.send(Request::CaptureShortcutFor(target));
    ui.capture = Capture::Listening {
        target,
        message: None,
        held: HotkeyModifiers::default(),
    };
}

pub(super) fn cancel_capture(ui: &mut Ui) {
    if matches!(ui.capture, Capture::Listening { .. }) {
        ui.send(Request::CancelShortcut);
    }
    ui.capture = Capture::Idle;
}

/// The chord a key press records, in the key names Linux registers.
pub(super) fn recorded_binding(event: &KeyEvent) -> Result<HotkeyBinding, &'static str> {
    let name = match &event.key {
        Key::Text(text) if text == " " => "space".to_string(),
        Key::Text(text) if text.chars().count() == 1 => text.to_lowercase(),
        Key::Command(letter) => letter.to_string(),
        Key::Function(number) => format!("f{number}"),
        Key::Enter => "enter".into(),
        Key::Tab => "tab".into(),
        Key::Backspace => "backspace".into(),
        Key::Delete => "delete".into(),
        Key::Left => "left".into(),
        Key::Right => "right".into(),
        Key::Up => "up".into(),
        Key::Down => "down".into(),
        Key::Home => "home".into(),
        Key::End => "end".into(),
        _ => return Err(tr("Unsupported key")),
    };
    let held = |down: bool| down.then_some(ModifierSide::Either);
    let modifiers = HotkeyModifiers {
        control: held(event.mods.ctrl),
        option: held(event.mods.alt),
        shift: held(event.mods.shift),
        command: held(event.mods.logo),
        function: false,
    };
    if modifiers.is_empty() && !matches!(event.key, Key::Function(_)) {
        return Err(tr("Add a modifier"));
    }
    Ok(HotkeyBinding {
        modifiers,
        key: Some(HotkeyKey::named(&name)),
    })
}

pub(super) fn validate(
    settings: &AppSettings,
    target: ShortcutTarget,
    binding: &HotkeyBinding,
) -> Result<(), &'static str> {
    host::validate_hotkey(target, binding)?;
    if conflicts(settings, target, binding) {
        return Err(tr("Already in use"));
    }
    Ok(())
}

/// Whether `binding` overlaps a shortcut `target` must not share: the other
/// active shortcuts and, where meetings exist (macOS), meeting paste.
pub(super) fn conflicts(
    settings: &AppSettings,
    target: ShortcutTarget,
    binding: &HotkeyBinding,
) -> bool {
    use crate::app_settings::HotkeySlot;
    let slot = match target {
        ShortcutTarget::Dictation => HotkeySlot::Dictation,
        ShortcutTarget::VoiceAction => HotkeySlot::VoiceAction,
        ShortcutTarget::PasteLast => HotkeySlot::PasteLast,
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => HotkeySlot::Capture,
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => HotkeySlot::CaptureList,
    };
    settings.hotkey_in_use(slot, binding)
}

pub(super) fn desktop_shortcut(binding: &HotkeyBinding) -> DesktopShortcut {
    let modifiers = binding.modifiers;
    DesktopShortcut {
        alt: modifiers.option.is_some(),
        control: modifiers.control.is_some(),
        function: false,
        key: binding
            .key
            .as_ref()
            .map(HotkeyKey::name)
            .unwrap_or_default(),
        platform: modifiers.command.is_some(),
        shift: modifiers.shift.is_some(),
    }
}

pub(super) fn binding(settings: &AppSettings, target: ShortcutTarget) -> Option<&HotkeyBinding> {
    match target {
        ShortcutTarget::Dictation => Some(&settings.dictation_hotkey),
        ShortcutTarget::PasteLast => settings.paste_last_hotkey.as_ref(),
        ShortcutTarget::VoiceAction => Some(&settings.edit_hotkey),
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => Some(&settings.capture.hotkey),
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => Some(&settings.capture.list_hotkey),
    }
}

pub(super) fn set_binding(
    settings: &mut AppSettings,
    target: ShortcutTarget,
    binding: HotkeyBinding,
) {
    match target {
        ShortcutTarget::Dictation => settings.dictation_hotkey = binding,
        ShortcutTarget::PasteLast => settings.paste_last_hotkey = Some(binding),
        ShortcutTarget::VoiceAction => settings.edit_hotkey = binding,
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => settings.capture.hotkey = binding,
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => settings.capture.list_hotkey = binding,
    }
}

fn sound_volume_index(settings: &AppSettings) -> usize {
    if !settings.sound_effects {
        return 0;
    }
    SOUND_VOLUME_STEPS[1..]
        .iter()
        .enumerate()
        .min_by(|(_, (_, left)), (_, (_, right))| {
            (settings.sound_effect_volume - left)
                .abs()
                .total_cmp(&(settings.sound_effect_volume - right).abs())
        })
        .map_or(0, |(index, _)| index + 1)
}

pub(super) fn toggle_menu(ui: &mut Ui, menu: Menu) {
    ui.open = (ui.open != Some(menu)).then_some(menu);
    if ui.open == Some(Menu::Microphone) {
        refresh_microphones(ui);
    }
}

/// Lists the microphones again, and what System default resolves to, on a
/// worker thread; [`poll_microphones`] adopts the result. A change during a
/// listing in flight is listed again after it.
pub(super) fn refresh_microphones(ui: &mut Ui) {
    if matches!(ui.service, Service::Fixture(_)) || ui.microphones_listing.is_some() {
        return;
    }
    ui.microphones_route = Some(crate::audio::input_route_generation());
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("hex-microphones".into())
        .spawn(move || {
            let _ = sender.send(crate::audio::input_devices().map_err(|error| error.to_string()));
        });
    match spawned {
        Ok(_) => ui.microphones_listing = Some(receiver),
        Err(error) => ui.microphones = Err(error.to_string()),
    }
}

/// Adopts a finished listing. Returns whether the microphones changed.
pub(super) fn poll_microphones(ui: &mut Ui) -> bool {
    let Some(listing) = &ui.microphones_listing else {
        return false;
    };
    let listed = match listing.try_recv() {
        Ok(listed) => listed,
        Err(std::sync::mpsc::TryRecvError::Empty) => return false,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            Err("the microphone list stopped".to_owned())
        }
    };
    ui.microphones_listing = None;
    let changed = ui.microphones != listed;
    ui.microphones = listed;
    changed
}

pub(super) fn error_notes(p: Paint, error: &str) -> [El; 2] {
    [
        note(
            "error-title",
            tr("Hex could not apply this change."),
            p.danger(),
        ),
        note("error", error, p.muted()),
    ]
}

/// General: the missing permissions, the listener (Windows and Linux), then
/// the Dictation (with its Advanced disclosure), Output, and Application
/// cards. Features' own settings live in their panes; updates live under
/// the sidebar's version.
pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    use super::platform_settings as os;
    let mut content = page("content")
        .children(os::permissions_section(ui, p))
        .children(os::error_section(ui, p));
    if ui.platform.caps.listener_control {
        content = content.child(listener_card(ui, p));
    }
    if !ui.records_shortcuts() {
        content = content.child(desktop_commands(ui, p));
    }
    content = content
        .child(dictation_section(ui, p, layer))
        .children(
            ui.platform
                .vocabulary
                .then(|| super::vocabulary::section_view(ui, p)),
        )
        .child(output_section(ui, p))
        .child(application_section(ui, p, layer));
    super::scroll_body("scroll", content)
}

fn listener_card(ui: &Ui, p: Paint) -> El {
    let running = ui.running();
    let status = match &ui.view {
        Some(view) => tr(&view.listener.status).to_string(),
        None if matches!(ui.service, Service::Fixture(_)) || ui.connection_error.is_some() => {
            tr("Service disconnected").to_string()
        }
        None => tr("Connecting…").to_string(),
    };
    let shortcut = ui.settings.dictation_hotkey.keycaps().join("+");
    let hint = match (running, ui.settings.shortcut_mode) {
        _ if !ui.records_shortcuts() => {
            tr("Ready anywhere · Use your desktop's shortcuts to dictate").to_string()
        }
        (true, ShortcutMode::Toggle) => tr_fill(
            "Ready anywhere · Press {} to start and stop dictating",
            &shortcut,
        ),
        (true, ShortcutMode::Hold) => tr_fill("Ready anywhere · Hold {} to dictate", &shortcut),
        (false, _) => tr_fill("Enable Hex to dictate into any app with {}", &shortcut),
    };
    let failure = ui
        .view
        .as_ref()
        .and_then(|view| view.listener.warning.clone())
        .map(|failure| tr_fill("Last dictation failed: {}", &failure));
    let error = ui
        .error
        .clone()
        .or_else(|| ui.connection_error.clone())
        .or_else(|| {
            ui.view
                .as_ref()
                .and_then(|view| view.operation_error.clone())
        });
    let action = if running {
        button(
            p,
            "toggle-listening",
            tr("Disable"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.send(Request::Desktop(DesktopAction::StopListening)),
        )
    } else {
        button(
            p,
            "toggle-listening",
            tr("Enable"),
            ButtonKind::Primary,
            |ui: &mut Ui, _| ui.send(Request::Desktop(DesktopAction::StartListening)),
        )
    };
    let summary = row()
        .key("listener")
        .min_h(METRICS.row_h_described)
        .px(METRICS.row_px)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(status_dot(if running { p.success() } else { p.muted() }))
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(status)
                        .key("status")
                        .font(p.k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(
                    text(hint)
                        .key("hint")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                )
                .children(failure.map(|failure| {
                    text(failure)
                        .key("failure")
                        .font(DESCRIPTION)
                        .color(p.danger())
                        .wrap()
                })),
        )
        .child(action);
    let mut rows = vec![summary];
    if let Some(error) = error {
        rows.extend(error_notes(p, &error));
    }
    section(p, "listening", tr("Status"), rows)
}

/// What a shortcuts portal granted for `target`, since the desktop decides
/// which trigger Hex actually gets.
pub(super) fn granted(ui: &Ui, p: Paint, target: ShortcutTarget) -> Option<(String, rune::Color)> {
    Some(match portal_grant(ui, target)? {
        DesktopGrantedShortcut {
            trigger: Some(trigger),
            ..
        } => (tr_fill("Your desktop granted {}", trigger), p.muted()),
        DesktopGrantedShortcut { trigger: None, .. } => (
            tr("Your desktop did not grant this shortcut").to_string(),
            p.danger(),
        ),
    })
}

fn portal_grant(ui: &Ui, target: ShortcutTarget) -> Option<&DesktopGrantedShortcut> {
    let Some(DesktopShortcuts::Portal(granted)) = ui.shortcuts() else {
        return None;
    };
    let name = match target {
        ShortcutTarget::Dictation => "Dictation",
        ShortcutTarget::PasteLast => "Paste last dictation",
        ShortcutTarget::VoiceAction => "Voice Action",
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => "Capture",
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => "Send-to list",
    };
    granted.iter().find(|shortcut| shortcut.name == name)
}

/// The desktop kept `target`'s shortcut for itself: the note under its row.
pub(super) fn refused_note(ui: &Ui, p: Paint, target: ShortcutTarget) -> Option<El> {
    portal_grant(ui, target)
        .is_some_and(|shortcut| shortcut.trigger.is_none())
        .then(|| {
            note(
                "refused",
                tr("Your desktop kept a shortcut for itself. Record another one, or free it in your desktop's keyboard settings (on niri, remove its bind from config.kdl)."),
                p.danger(),
            )
        })
}

/// One recordable shortcut beside its recorder, with what a shortcuts
/// portal granted for it.
fn shortcut_row(ui: &Ui, p: Paint, target: ShortcutTarget) -> El {
    let (key, title) = match target {
        ShortcutTarget::Dictation => ("dictation-shortcut", tr("Shortcut")),
        ShortcutTarget::PasteLast => ("paste-last", tr("Paste last dictation")),
        ShortcutTarget::VoiceAction => ("voice-action-shortcut", tr("Voice Action")),
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => ("capture-shortcut", tr("Shortcut")),
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => ("capture-list-shortcut", tr("Send-to list")),
    };
    described_row(
        p,
        key,
        title,
        granted(ui, p, target),
        shortcut_control(ui, p, target),
    )
}

/// The side picker (macOS), the shortcut's keys in their tray, the
/// recorder, and Clear for Paste last. The recorder keeps one width through
/// every state, so the keys and the picker hold still; a new shortcut of
/// another width moves them together on one spring.
const CONTROL_GAP: f32 = 8.0;
/// The key tray keeps a little more room on each side than the controls
/// keep between them.
const TRAY_GAP: f32 = CONTROL_GAP + 4.0;

pub(super) fn shortcut_control(ui: &Ui, p: Paint, target: ShortcutTarget) -> El {
    let key = match target {
        ShortcutTarget::Dictation => "dictation-shortcut-control",
        ShortcutTarget::PasteLast => "paste-last-control",
        ShortcutTarget::VoiceAction => "voice-action-shortcut-control",
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => "capture-shortcut-control",
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => "capture-list-shortcut-control",
    };
    let picker = super::platform_settings::side_selector(ui, p, target);
    // The picker hangs off the keys' left edge in the room its margin
    // keeps, so both stay put while either changes; a leaving picker fades
    // where it stands.
    let room = picker.as_ref().map_or(0.0, |_| {
        super::platform_settings::SIDE_PICKER_WIDTH + TRAY_GAP
    });
    let keys = match binding(&ui.settings, target) {
        Some(binding) => keycaps(
            p,
            "keys",
            &shortcut_caps(
                binding,
                super::platform_settings::picked_side(ui, target).is_some(),
            ),
        ),
        None => key_tray(
            p,
            "keys",
            KeySize::Small,
            // Off trades places with the keys by rolling, never fading
            // over them.
            [row()
                .key("off")
                .h(KeySize::Small.height())
                .px(5.0)
                .center()
                .transition(tray_roll())
                .child(
                    text(tr("Off"))
                        .key("label")
                        .font(DESCRIPTION)
                        .color(p.faint()),
                )],
        ),
    };
    let mut control = row()
        .key(key)
        .gap(CONTROL_GAP)
        .center_y()
        .motion(p.k.motion.control)
        .child(
            row()
                .key("recorder-slot")
                .ml(room)
                .gap(TRAY_GAP)
                .center_y()
                .children(picker.map(|picker| picker.pin(Some(-room), Some(0.0), None, None)))
                .child(keys)
                .child(hotkey_control(ui, p, target)),
        );
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
    control
}

/// Without a shortcuts portal the desktop's keybindings own every shortcut:
/// the commands to bind there.
fn desktop_commands(ui: &Ui, p: Paint) -> El {
    let mut rows = vec![
        note(
            "no-portal",
            tr(
                "Your desktop has no shortcuts portal. Bind these commands in its keyboard settings.",
            ),
            p.muted(),
        ),
        command_row(
            p,
            "hold",
            tr("Hold to dictate"),
            tr("Bind start to the key press and stop to its release"),
            &["hex dictation start", "hex dictation stop"],
        ),
        command_row(
            p,
            "toggle",
            tr("Toggle dictation"),
            tr("Press to start dictating, press again to transcribe"),
            &["hex dictation toggle"],
        ),
        command_row(
            p,
            "cancel",
            tr("Cancel dictation"),
            tr("Discard the current recording"),
            &["hex dictation cancel"],
        ),
        command_row(
            p,
            "paste-last",
            tr("Paste last dictation"),
            tr("Re-paste the most recent dictation"),
            &["hex paste-last"],
        ),
    ];
    if ui.settings.voice_action.enabled {
        rows.push(command_row(
            p,
            "voice-action",
            tr("Voice Action"),
            tr("Hold to speak; selected text is included automatically"),
            &["hex voice-action start", "hex voice-action stop"],
        ));
    }
    section(p, "desktop-shortcuts", tr("Desktop shortcuts"), rows)
}

fn command_row(p: Paint, key: &str, title: &str, description: &str, commands: &[&str]) -> El {
    let lines = col()
        .key("commands")
        .gap(2.0)
        .children(commands.iter().enumerate().map(|(index, command)| {
            text(*command)
                .key(format!("command{index}"))
                .font(p.k.ty.mono)
                .color(p.muted())
                .align(rune::Align::End)
        }));
    setting_row(p, key, title, Some(description), lines)
}

/// The shortcut and how it triggers, the speech model, and the microphone.
fn dictation_section(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = Vec::new();
    if ui.records_shortcuts() {
        rows.push(shortcut_row(ui, p, ShortcutTarget::Dictation).into());
        rows.extend(refused_note(ui, p, ShortcutTarget::Dictation).map(CardRow::from));
    }
    let toggle_mode = settings.shortcut_mode == ShortcutMode::Toggle;
    rows.push(
        link_row(
            p,
            "shortcut-mode",
            tr("Shortcut mode"),
            explainer::link(p),
            segmented(
                p,
                "shortcut-mode-control",
                &[tr("Hold"), tr("Toggle")],
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
            ),
        )
        .into(),
    );
    rows.push(folding(
        "double-tap-fold",
        (!toggle_mode).then(|| {
            toggle_row(
                p,
                "double-tap",
                tr("Double-tap to lock"),
                None,
                settings.double_tap_lock,
                explainer::flip_lock,
            )
        }),
    ));
    let selection = &settings.transcription;
    let model = transcription_models::definition(selection.model);
    let label = format!(
        "{} · {}",
        transcription_models::language_name(&selection.language),
        model.name
    );
    let language = selection.language.clone();
    rows.push(
        described_row(
            p,
            "transcription",
            tr("Speech model"),
            transcription_status(ui, p),
            sheet_button(p, "transcription-control", &label, move |ui: &mut Ui| {
                super::picker::open(ui, language.clone())
            }),
        )
        .into(),
    );
    let listed = ui.microphones.as_ref().ok();
    let default_name = listed.and_then(|devices| devices.system_default.as_deref());
    // Only a list that names inputs can tell a saved microphone is missing.
    let missing = settings.microphone.as_deref().filter(|saved| {
        listed.is_some_and(|devices| {
            !devices.names.is_empty() && !devices.names.iter().any(|name| name == saved)
        })
    });
    // System default's choice, with the input it resolves to in faint text.
    let system_detail = default_name.map(str::to_owned);
    // The field always names the input in use, so a missing saved
    // microphone shows the system default standing in for it.
    let (microphone, microphone_detail) = match (&settings.microphone, missing) {
        (Some(saved), None) => (saved.clone(), None),
        _ => (tr("System default").to_owned(), system_detail.clone()),
    };
    let mac = ui.platform.caps.os == super::platform::Os::Macos;
    let microphone_note = listed.and_then(|devices| match &settings.microphone {
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
        None => None,
    });
    let mut devices = vec![Choice {
        label: tr("System default").to_owned(),
        detail: system_detail,
        selected: settings.microphone.is_none(),
        choose: Box::new(|ui: &mut Ui| {
            ui.open = None;
            ui.update(|settings| settings.microphone = None);
        }),
    }];
    if let Ok(listed) = &ui.microphones {
        devices.extend(listed.names.iter().map(|device| {
            let name = device.clone();
            Choice {
                label: device.clone(),
                detail: None,
                selected: settings.microphone.as_deref() == Some(device),
                choose: Box::new(move |ui: &mut Ui| {
                    ui.open = None;
                    let name = name.clone();
                    ui.update(|settings| settings.microphone = Some(name));
                }),
            }
        }));
    }
    if let Some(saved) = missing {
        devices.push(Choice {
            label: saved.to_owned(),
            detail: Some(tr("not connected").to_owned()),
            selected: true,
            choose: Box::new(|ui: &mut Ui| ui.open = None),
        });
    }
    rows.push(
        setting_row(
            p,
            "microphone",
            tr("Microphone"),
            microphone_note.as_deref(),
            select_sized(
                p,
                "microphone-control",
                &microphone,
                microphone_detail.as_deref(),
                ui.open == Some(Menu::Microphone),
                devices,
                |ui: &mut Ui| toggle_menu(ui, Menu::Microphone),
                layer,
                DEVICE_SELECT_WIDTH,
            ),
        )
        .into(),
    );
    rows.push(folding(
        "devices-error-fold",
        match (ui.open, &ui.microphones) {
            (Some(Menu::Microphone), Err(error)) => Some(note("devices-error", error, p.danger())),
            _ => None,
        },
    ));
    if model.supports_recognition_hints {
        rows.push(
            setting_row(
                p,
                "hints",
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
    let pinned = advanced_in_use(settings);
    rows.push(folding(
        "advanced-fold",
        if ui.advanced || pinned {
            advanced_rows(ui, p)
        } else {
            Vec::new()
        },
    ));
    // The toggle's word ends on the box's edge, its hover reaching past it.
    section_with(
        p,
        "dictation",
        tr("Dictation"),
        advanced_toggle(ui, p, pinned).mr(-6.0),
        rows,
    )
}

/// The speech model row's description: preparation progress or its error.
fn transcription_status(ui: &Ui, p: Paint) -> Option<(String, rune::Color)> {
    let transcription = &ui.view.as_ref()?.transcription;
    if let Some(error) = &transcription.error {
        return Some((error.clone(), p.danger()));
    }
    let model = transcription.preparing?;
    let name = transcription_models::definition(model).name;
    let stage = match transcription.preparation_stage {
        Some(ModelPreparationStage::Verifying) => tr("Verifying model").to_string(),
        Some(ModelPreparationStage::Loading) => tr("Loading model").to_string(),
        _ => format!(
            "{} {:.0} MB",
            tr("Downloading"),
            transcription.downloaded_bytes as f64 / 1_000_000.0
        ),
    };
    Some((format!("{name} · {stage}"), p.accent()))
}

/// What a recorder shows; each change rolls up into it from below.
#[derive(Clone, Copy, PartialEq)]
enum Face {
    /// What pressing does (Change shortcut), or that it just saved; the
    /// shortcut's keys sit in their tray beside the recorder.
    Shortcut {
        saved: bool,
    },
    Listening(Prompt),
}

#[derive(Clone, Copy, PartialEq)]
enum Prompt {
    Press,
    /// Why the last chord was rejected.
    Rejected(&'static str),
    /// The modifiers held so far.
    Held(HotkeyModifiers),
    Saving,
}

const RECORDER_HEIGHT: f32 = 28.0;
/// Between the recorder's edge and a keycap at that edge.
const RECORDER_CAP_INSET: f32 = 3.0;
/// How far held keys that overflow the recorder fade out, and the room
/// above and below them their shadows keep inside that clip.
const HELD_FADE: f32 = 8.0;
const HELD_SHADOW_ROOM: f32 = (RECORDER_HEIGHT - KeySize::Small.height()) / 2.0;

/// The shortcut's recorder: a button that keeps the width of its widest
/// face, so nothing beside it moves while it records, and swaps faces
/// inside its own rounded clip.
pub(super) fn hotkey_control(ui: &Ui, p: Paint, target: ShortcutTarget) -> El {
    let key = match target {
        ShortcutTarget::Dictation => "dictation-hotkey",
        ShortcutTarget::PasteLast => "paste-last-hotkey",
        ShortcutTarget::VoiceAction => "voice-action-hotkey",
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => "capture-hotkey",
        #[cfg(hex_capture)]
        ShortcutTarget::CaptureList => "capture-list-hotkey",
    };
    let face = match ui.capture {
        Capture::Listening {
            target: active,
            message,
            held,
        } if active == target => Face::Listening(match message {
            Some(message) => Prompt::Rejected(message),
            None if held.is_empty() => Prompt::Press,
            None => Prompt::Held(held),
        }),
        Capture::Saving { target: active, .. } if active == target => {
            Face::Listening(Prompt::Saving)
        }
        Capture::Saved { target: active, .. } if active == target => Face::Shortcut { saved: true },
        _ => Face::Shortcut { saved: false },
    };
    // The widths it reserves: every face but held modifiers, which clip.
    let mut sizes = vec![
        Face::Shortcut { saved: false },
        Face::Shortcut { saved: true },
        Face::Listening(Prompt::Press),
        Face::Listening(Prompt::Saving),
    ];
    sizes.extend(
        ["Unsupported key", "Add a modifier", "Already in use"]
            .map(|message| Face::Listening(Prompt::Rejected(tr(message)))),
    );
    if matches!(face, Face::Listening(Prompt::Rejected(_))) && !sizes.contains(&face) {
        sizes.push(face);
    }
    let c = &p.k.c;
    let listening =
        matches!(ui.capture, Capture::Listening { target: active, .. } if active == target);
    let mut frame = row()
        .key(key)
        .role("button")
        .label(face_label(face))
        .h(RECORDER_HEIGHT)
        .radius(p.k.radius.md)
        .fill(c.bg_button_neutral)
        // While listening, keys reach the recorder rather than pressing it again.
        .focusable(!listening)
        .focus_ring(2.0, 2.5, c.border_focus)
        .on_press(move |ui: &mut Ui| begin_capture(ui, target));
    for shadow in &p.k.elevation.button_neutral {
        frame = frame.box_shadow(*shadow);
    }
    if face == (Face::Shortcut { saved: false }) {
        frame = frame.hover(c.hover).press(c.pressed);
    }
    frame.child(
        col()
            .key("faces")
            .equal_columns(1)
            .clip_rounded(p.k.radius.md)
            .children(sizes.into_iter().enumerate().map(|(index, size)| {
                recorder_face(p, size, false)
                    .key(format!("size{index}"))
                    .role("none")
                    .grid_cell(0)
                    .opacity(0.0)
            }))
            .child(
                recorder_face(p, face, true)
                    .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                    .transition(roll()),
            ),
    )
}

/// In from below, out through the top, both inside the recorder's clip.
fn roll() -> Transition {
    Transition::push(Edge::Bottom)
}

fn face_label(face: Face) -> &'static str {
    match face {
        Face::Shortcut { saved: false } => tr("Change shortcut"),
        Face::Shortcut { saved: true } => tr("Saved"),
        Face::Listening(Prompt::Press | Prompt::Held(_)) => tr("Press shortcut"),
        Face::Listening(Prompt::Rejected(message)) => message,
        Face::Listening(Prompt::Saving) => tr("Saving…"),
    }
}

/// One face of the recorder at its full height. Only the shown face
/// (`live`) pulses.
fn recorder_face(p: Paint, face: Face, live: bool) -> El {
    let label_style = p.k.ty.ui_medium.lh(16.0);
    let label = |key: &str, copy: &str, color| text(copy).key(key).font(label_style).color(color);
    let face_row = |key: &str| row().key(key).h(RECORDER_HEIGHT).gap(8.0).center_y();
    match face {
        Face::Shortcut { saved } => {
            let what = if saved {
                row()
                    .key("saved")
                    .gap(2.0)
                    .center_y()
                    .child(rune::icon(CHECK, 16.0).key("check").color(p.success()))
                    .child(label("label", tr("Saved"), p.text()))
            } else {
                row()
                    .key("change")
                    .child(label("label", tr("Change shortcut"), p.text()))
            };
            face_row("shortcut")
                .px(11.0)
                .center()
                .child(what.transition(roll()))
        }
        Face::Listening(prompt) => {
            let dot = match prompt {
                Prompt::Saving => status_dot(p.muted()),
                _ if live => pulsing_dot(p.danger()),
                _ => status_dot(p.danger()),
            };
            let prompt_el =
                match prompt {
                    Prompt::Press => label("prompt", tr("Press shortcut"), p.text()),
                    Prompt::Rejected(message) => label("rejected", message, p.danger()),
                    Prompt::Held(held) => {
                        let held = HotkeyBinding {
                            modifiers: held.without_side_constraints(),
                            key: None,
                        };
                        // Held keys stand on the recorder, down. Too many
                        // to fit fade out at the right, so Esc stays.
                        row()
                            .key("held")
                            .gap(4.0)
                            .center_y()
                            .shrink()
                            .py(HELD_SHADOW_ROOM)
                            .pr(HELD_FADE)
                            .clip_fade(0.0, 0.0, HELD_FADE, 0.0)
                            .child(row().key("keys").gap(KEY_GAP).children(
                                held.keycaps().iter().enumerate().map(|(index, cap)| {
                                    keycap(p, index, cap, KeySize::Small, true)
                                }),
                            ))
                            .child(label("ellipsis", "…", p.muted()))
                    }
                    Prompt::Saving => label("saving", tr("Saving…"), p.muted()),
                };
            let face = face_row("listening")
                .pl(11.0)
                .pr(RECORDER_CAP_INSET)
                .child(dot)
                .child(prompt_el.transition(roll()));
            if prompt == Prompt::Saving {
                return face;
            }
            face.child(spacer()).child(
                keycap(p, 0, ESCAPE_KEYCAP, KeySize::Small, false)
                    .key("escape")
                    .label(tr("Esc to cancel")),
            )
        }
    }
}

/// The Escape key's legend, as the keyboard prints it.
const ESCAPE_KEYCAP: &str = if cfg!(target_os = "macos") {
    "esc"
} else {
    "Esc"
};

/// A shortcut's keycaps, naming the side a modifier must be pressed on
/// unless a side picker beside them already shows it.
pub(super) fn shortcut_caps(binding: &HotkeyBinding, side_picker: bool) -> Vec<String> {
    binding
        .sided_keycaps()
        .into_iter()
        .map(|(cap, side)| match side {
            ModifierSide::Left if !side_picker => format!("{} {cap}", tr("Left")),
            ModifierSide::Right if !side_picker => format!("{} {cap}", tr("Right")),
            _ => cap,
        })
        .collect()
}

/// Where dictation goes and what happens around it.
fn output_section(ui: &Ui, p: Paint) -> El {
    let settings = &ui.settings;
    let mut rows = Vec::new();
    if ui.records_shortcuts() {
        rows.push(shortcut_row(ui, p, ShortcutTarget::PasteLast));
        rows.extend(refused_note(ui, p, ShortcutTarget::PasteLast));
    }
    let audio_labels: Vec<&str> = RecordingAudioBehavior::ALL
        .iter()
        .map(|behavior| tr(behavior.label()))
        .collect();
    let audio_index = RecordingAudioBehavior::ALL
        .iter()
        .position(|behavior| *behavior == settings.recording_audio_behavior)
        .unwrap_or(2);
    rows.push(setting_row(
        p,
        "while-dictating",
        tr("While dictating"),
        None,
        segmented(
            p,
            "while-dictating-control",
            &audio_labels,
            audio_index,
            |ui, index| {
                let behavior = RecordingAudioBehavior::ALL[index];
                if ui.settings.recording_audio_behavior != behavior {
                    ui.update(|settings| settings.recording_audio_behavior = behavior);
                }
            },
        ),
    ));
    let indicator_index = match settings.indicator_position {
        IndicatorPosition::Top => 0,
        IndicatorPosition::Bottom => 1,
        IndicatorPosition::Hidden => 2,
    };
    rows.push(setting_row(
        p,
        "indicator",
        tr("Dictation HUD"),
        None,
        segmented(
            p,
            "indicator-control",
            &[tr("Top"), tr("Bottom"), tr("Off")],
            indicator_index,
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
        ),
    ));
    if ui.platform.caps.terminal_paste {
        rows.push(toggle_row(
            p,
            "terminal-paste",
            tr("Terminal paste"),
            tr("Paste with Ctrl+Shift+V so terminals accept it"),
            settings.paste_with_shift,
            |ui: &mut Ui| {
                let enabled = !ui.settings.paste_with_shift;
                ui.update(|settings| settings.paste_with_shift = enabled);
            },
        ));
    }
    section(p, "output", tr("Output"), rows)
}

/// An advanced setting is off its default, so Advanced stays open to show
/// it: the microphone released while idle, Double-tap only where its row
/// shows, or a chosen update channel.
fn advanced_in_use(settings: &AppSettings) -> bool {
    settings.release_microphone_while_idle
        || (double_tap_only_applies(settings) && settings.double_tap_only)
        || settings.updates.channel.is_some()
}

fn double_tap_only_applies(settings: &AppSettings) -> bool {
    settings.shortcut_mode == ShortcutMode::Hold
        && settings.double_tap_lock
        && settings.dictation_hotkey.key.is_some()
}

/// The Dictation header's disclosure for the options most people never
/// change, which show at the end of the card. It stays open while one is in
/// use. (A row above them at the card's end would Tab straight into them,
/// but costs the 37 px that keeps General from scrolling at 760.)
fn advanced_toggle(ui: &Ui, p: Paint, pinned: bool) -> El {
    let c = &p.k.c;
    let open = ui.advanced || pinned;
    let toggle = row()
        .key("advanced-toggle")
        .label(tr("Advanced"))
        .h(18.0)
        .px(6.0)
        .gap(2.0)
        .center_y()
        .radius(p.k.radius.md)
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
        );
    if pinned {
        return toggle;
    }
    toggle
        .role("button")
        .hover(c.hover)
        .press(c.pressed)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .on_press(|ui: &mut Ui| {
            ui.advanced = !ui.advanced;
            if !ui.advanced {
                ui.confirm_release = false;
            }
        })
}

/// Advanced dictation rows, shown at the end of the Dictation card: the
/// microphone's idle release and Double-tap only.
fn advanced_rows(ui: &Ui, p: Paint) -> Vec<CardRow> {
    let settings = &ui.settings;
    let mut rows: Vec<CardRow> = vec![
        setting_row(
            p,
            "microphone-mode",
            tr("Microphone mode"),
            Some(if settings.release_microphone_while_idle {
                tr("Opens only while holding the shortcut. Disables pre-roll and commands.")
            } else {
                tr("Keeps the microphone open for instant start and pre-roll")
            }),
            segmented(
                p,
                "microphone-mode-control",
                &[tr("Keep ready (fast)"), tr("Release when idle")],
                usize::from(settings.release_microphone_while_idle),
                |ui, index| {
                    ui.advanced = true;
                    let release = index == 1;
                    if release && !ui.settings.release_microphone_while_idle {
                        ui.confirm_release = true;
                        return;
                    }
                    ui.confirm_release = false;
                    if ui.settings.release_microphone_while_idle != release {
                        let mut candidate = ui.drafted();
                        match candidate.set_release_microphone_while_idle(release) {
                            Ok(()) => {
                                ui.commit(candidate);
                            }
                            Err(error) => ui.error = Some(format!("{error:#}")),
                        }
                    }
                },
            ),
        )
        .into(),
        folding(
            "release-confirmation-fold",
            ui.confirm_release.then(|| release_confirmation(ui, p)),
        ),
        folding(
            "double-tap-only-fold",
            double_tap_only_applies(settings).then(|| {
                toggle_row(
                    p,
                    "double-tap-only",
                    tr("Double-tap only"),
                    tr("Wait for two complete shortcut taps before recording"),
                    settings.double_tap_only,
                    |ui: &mut Ui| {
                        ui.advanced = true;
                        let enabled = !ui.settings.double_tap_only;
                        ui.update(|settings| settings.double_tap_only = enabled);
                    },
                )
            }),
        ),
    ];
    rows.extend(update_channel_row(ui, p).map(CardRow::from));
    rows
}

fn release_confirmation(ui: &Ui, p: Paint) -> El {
    let description = if ui.settings.commands_enabled {
        tr("Opens the microphone on shortcut press without pre-roll and turns Commands off.")
    } else {
        tr("Opens the microphone on shortcut press without pre-roll. Commands remain off.")
    };
    setting_row(
        p,
        "confirm-release",
        tr("Release the microphone when idle?"),
        Some(description),
        row()
            .key("actions")
            .gap(8.0)
            .child(button(
                p,
                "cancel-release",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    ui.confirm_release = false;
                },
            ))
            .child(
                button(
                    p,
                    "confirm-release",
                    tr("Release microphone"),
                    ButtonKind::Primary,
                    |ui: &mut Ui, _| {
                        if ui.update(AppSettings::disable_commands_and_release_microphone) {
                            ui.confirm_release = false;
                        }
                    },
                )
                .default_button(),
            ),
    )
    .fill(p.k.c.hover)
}

fn application_section(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let settings = &ui.settings;
    let mut rows = super::platform_settings::launch_at_login_rows(ui, p);
    if ui.platform.caps.listener_control {
        rows.push(toggle_row(
            p,
            "enable-on-launch",
            tr("Enable on launch"),
            tr("Start listening when Hex opens"),
            settings.enable_on_launch,
            |ui: &mut Ui| {
                let enabled = !ui.settings.enable_on_launch;
                ui.update(|settings| settings.enable_on_launch = enabled);
            },
        ));
    }
    rows.extend(super::platform_settings::dock_icon_row(ui, p));
    let volume_labels: Vec<&str> = SOUND_VOLUME_STEPS
        .iter()
        .map(|(label, _)| tr(label))
        .collect();
    rows.push(setting_row(
        p,
        "sound-volume",
        tr("Sound volume"),
        None,
        segmented(
            p,
            "sound-volume-control",
            &volume_labels,
            sound_volume_index(settings),
            |ui, index| {
                let volume = SOUND_VOLUME_STEPS[index].1;
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
        ),
    ));
    let language = tr(crate::desktop_i18n::choice_name(
        settings.ui_language.as_deref(),
    ));
    let languages = crate::desktop_i18n::LANGUAGE_CHOICES
        .iter()
        .map(|(code, name)| {
            let code = code.map(str::to_string);
            let selected = settings.ui_language == code;
            Choice {
                label: tr(name).to_string(),
                detail: None,
                selected,
                choose: Box::new(move |ui: &mut Ui| {
                    ui.open = None;
                    let code = code.clone();
                    if ui.update(|settings| settings.ui_language = code.clone()) {
                        crate::desktop_i18n::apply(code.as_deref());
                    }
                }),
            }
        })
        .collect();
    rows.push(setting_row(
        p,
        "interface-language",
        tr("Interface language"),
        None,
        select(
            p,
            "interface-language-control",
            language,
            ui.open == Some(Menu::Language),
            languages,
            |ui: &mut Ui| toggle_menu(ui, Menu::Language),
            layer,
        ),
    ));
    section(p, "application", tr("Application"), rows)
}

/// Stable or Dev, only while this install updates itself: test, source,
/// managed, and package-manager installs never check a channel.
fn update_channel_row(ui: &Ui, p: Paint) -> Option<El> {
    let updates = matches!(
        ui.view.as_ref()?.update_status,
        DesktopUpdateStatus::Checking
            | DesktopUpdateStatus::Current
            | DesktopUpdateStatus::Failed
            | DesktopUpdateStatus::Downloading
            | DesktopUpdateStatus::Available
            | DesktopUpdateStatus::ReadyToRestart
            | DesktopUpdateStatus::SwitchAvailable { .. }
    );
    if !updates {
        return None;
    }
    let shown = ui
        .settings
        .updates
        .channel
        .unwrap_or_else(crate::updater::build_channel);
    let labels: Vec<&str> = UpdateChannel::ALL
        .iter()
        .map(|channel| tr(channel.label()))
        .collect();
    Some(setting_row(
        p,
        "update-channel",
        tr("Update channel"),
        None,
        segmented(
            p,
            "update-channel-control",
            &labels,
            UpdateChannel::ALL
                .iter()
                .position(|item| *item == shown)
                .unwrap_or(0),
            move |ui, index| {
                // Choosing the channel already in effect saves nothing, so an
                // unset channel keeps following the build.
                let channel = UpdateChannel::ALL[index];
                ui.advanced = true;
                if channel != shown
                    && ui.update(|settings| settings.updates.channel = Some(channel))
                {
                    ui.send(Request::Desktop(DesktopAction::CheckForUpdates));
                }
            },
        ),
    ))
}
