//! The General pane: listener status, the Dictation, Output, and Application
//! cards with the shortcuts in their place, Advanced options behind a
//! disclosure, and recording shortcuts through the host.

use std::time::{Duration, Instant};

use rune::{Edge, El, Key, KeyEvent, Transition, Tx, col, row, spacer, text};

use super::controls::{
    ButtonKind, Choice, DESCRIPTION, KEY_GAP, KeySize, METRICS, Paint, button, fixed_select,
    keycap, note, pulsing_dot, section, select, setting_row, status_dot,
};
use super::host;
use super::{Menu, Request, Service, ShortcutTarget, Ui};
use crate::app_settings::{
    AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, KeyboardModelChoice, ModifierSide,
};
use crate::desktop_host::{
    DesktopAction, DesktopGrantedShortcut, DesktopShortcut, DesktopShortcuts, DesktopUpdateStatus,
};
use crate::desktop_i18n::{tr, tr_fill};
use crate::dictation::ShortcutMode;
use crate::keyboard_model_switching::InputSource;
use crate::transcription_models::{self, ModelPreparationStage};

static CHECK: &rune::Icon = &rune_theme_opencode::icons::CHECK_SMALL;

pub(super) const SOUND_VOLUME_STEPS: [(&str, f32); 5] = [
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
        match event.key {
            Key::Modifier { modifier, pressed } => {
                modifier_alone(ui, modifier, pressed, event.mods)
            }
            _ => record(ui, target, recorded_binding(event)),
        }
    }
    true
}

/// Where the window records key events (Linux), a modifier pressed on its
/// own shows among the held keys, and releasing them all without a key
/// says why Linux can't bind that and what it can: a key with them. The
/// GlobalShortcuts portal and X11 grabs take a key plus modifiers, never a
/// modifier alone or one side of it.
fn modifier_alone(ui: &mut Ui, modifier: rune::Modifier, pressed: bool, mods: rune::Mods) {
    let Capture::Listening { message, held, .. } = &mut ui.capture else {
        return;
    };
    if pressed {
        use rune::Modifier as M;
        let slot = match modifier {
            M::ControlLeft | M::ControlRight => &mut held.control,
            M::AltLeft | M::AltRight => &mut held.option,
            M::ShiftLeft | M::ShiftRight => &mut held.shift,
            M::SuperLeft | M::SuperRight => &mut held.command,
            _ => return,
        };
        let side = match modifier.side() {
            Some(rune::KeySide::Left) => ModifierSide::Left,
            Some(rune::KeySide::Right) => ModifierSide::Right,
            None => ModifierSide::Either,
        };
        *slot = Some(match (*slot, side) {
            (Some(ModifierSide::Left), ModifierSide::Right)
            | (Some(ModifierSide::Right), ModifierSide::Left) => ModifierSide::Both,
            (Some(current), _) => current,
            (None, side) => side,
        });
        *message = None;
        return;
    }
    if mods.ctrl || mods.alt || mods.shift || mods.logo {
        return;
    }
    let peak = std::mem::take(held);
    if !peak.is_empty() {
        *message = Some(modifier_only_hint(peak));
    }
}

/// The nearest chord Linux binds to modifiers held alone: the same
/// modifiers, one side or either, with a key.
pub(super) fn modifier_only_hint(peak: HotkeyModifiers) -> &'static str {
    let both =
        [peak.control, peak.option, peak.shift, peak.command].contains(&Some(ModifierSide::Both));
    if both {
        tr("Linux can't bind both sides. Use one with a key, like Space")
    } else {
        tr("Linux needs a key too. Hold these and press one, like Space")
    }
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

pub(super) fn begin_capture(ui: &mut Ui, target: ShortcutTarget) {
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
    // A bare left click would swallow every click, this window's included.
    if binding.is_bare_primary_click() {
        return Err(tr("Add a modifier"));
    }
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
        ShortcutTarget::PasteLast => HotkeySlot::PasteLast,
        ShortcutTarget::RawDictation => HotkeySlot::RawDictation,
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => HotkeySlot::Capture,
        #[cfg(target_os = "macos")]
        ShortcutTarget::SecondDictation => HotkeySlot::SecondDictation,
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
        ShortcutTarget::RawDictation => settings.raw_dictation_hotkey.as_ref(),
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => Some(&settings.capture.hotkey),
        #[cfg(target_os = "macos")]
        ShortcutTarget::SecondDictation => settings.second_dictation_hotkey.as_ref(),
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
        ShortcutTarget::RawDictation => settings.raw_dictation_hotkey = Some(binding),
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => settings.capture.hotkey = binding,
        #[cfg(target_os = "macos")]
        ShortcutTarget::SecondDictation => settings.second_dictation_hotkey = Some(binding),
    }
}

/// Labs › Restore shortcut default: puts `default` back as `target`'s
/// shortcut under the rules a recorded one meets. A default another
/// shortcut already uses changes nothing and opens the recorder with the
/// usual conflict, so another can be recorded.
pub(super) fn restore_default(ui: &mut Ui, target: ShortcutTarget, default: HotkeyBinding) {
    match validate(&ui.settings, target, &default) {
        Ok(()) => {
            cancel_capture(ui);
            ui.update(|settings| set_binding(settings, target, default.clone()));
        }
        Err(message) => {
            if !matches!(ui.capture, Capture::Listening { target: active, .. } if active == target)
            {
                begin_capture(ui, target);
            }
            if let Capture::Listening { message: shown, .. } = &mut ui.capture {
                *shown = Some(message);
            }
        }
    }
}

pub(super) fn sound_volume_index(settings: &AppSettings) -> usize {
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
    if matches!(ui.open, Some(Menu::Microphone | Menu::TourMicrophone)) {
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

/// A saved microphone the input list no longer names. Only a list that
/// names inputs can tell.
pub(super) fn missing_microphone(ui: &Ui) -> Option<&str> {
    let listed = ui.microphones.as_ref().ok();
    ui.settings.microphone.as_deref().filter(|saved| {
        listed.is_some_and(|devices| {
            !devices.names.is_empty() && !devices.names.iter().any(|name| name == saved)
        })
    })
}

/// The microphone menu's choices, shared by General and the tour: System
/// default with the input it resolves to, every listed input, and a saved
/// one that is missing. Choosing saves through `Ui::update`.
pub(super) fn microphone_choices(ui: &Ui) -> Vec<Choice> {
    let settings = &ui.settings;
    let listed = ui.microphones.as_ref().ok();
    let off = listed.and_then(|devices| devices.off_with_lid_closed.as_deref());
    let mut devices = vec![Choice {
        label: tr("System default").to_owned(),
        detail: listed.and_then(|devices| devices.system_default.clone()),
        selected: settings.microphone.is_none(),
        choose: Box::new(|ui: &mut Ui| {
            ui.open = None;
            ui.update(|settings| settings.microphone = None);
        }),
    }];
    if let Some(listed) = listed {
        devices.extend(listed.names.iter().map(|device| {
            let name = device.clone();
            Choice {
                label: device.clone(),
                detail: (off == Some(device.as_str()))
                    .then(|| tr("off while the lid is closed").to_owned()),
                selected: settings.microphone.as_deref() == Some(device),
                choose: Box::new(move |ui: &mut Ui| {
                    ui.open = None;
                    let name = name.clone();
                    ui.update(|settings| settings.microphone = Some(name));
                }),
            }
        }));
    }
    if let Some(saved) = missing_microphone(ui) {
        devices.push(Choice {
            label: saved.to_owned(),
            detail: Some(tr("not connected").to_owned()),
            selected: true,
            choose: Box::new(|ui: &mut Ui| ui.open = None),
        });
    }
    devices
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

pub(super) fn listener_card(ui: &Ui, p: Paint) -> El {
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
        DesktopGrantedShortcut {
            trigger: None,
            unconfirmed: true,
            ..
        } => (
            tr("Your desktop assigns this key itself, so Hex can't confirm it").to_string(),
            p.muted(),
        ),
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
        ShortcutTarget::RawDictation => "Dictate without processing",
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => "Capture",
        // macOS has no shortcuts portal.
        #[cfg(target_os = "macos")]
        ShortcutTarget::SecondDictation => return None,
    };
    granted.iter().find(|shortcut| shortcut.name == name)
}

/// The key a shortcuts portal's desktop gave Cancel dictation. Hex suggests
/// none, so a key here comes from the desktop (#624), and it takes that key
/// from every app.
pub(super) fn cancel_grant(ui: &Ui) -> Option<&str> {
    let Some(DesktopShortcuts::Portal(granted)) = ui.shortcuts() else {
        return None;
    };
    granted
        .iter()
        .find(|shortcut| shortcut.name == "Cancel dictation")
        .and_then(|shortcut| shortcut.trigger.as_deref())
}

/// The desktop kept `target`'s shortcut for itself: the note under its row.
pub(super) fn refused_note(ui: &Ui, p: Paint, target: ShortcutTarget) -> Option<El> {
    portal_grant(ui, target)
        .is_some_and(|shortcut| shortcut.trigger.is_none() && !shortcut.unconfirmed)
        .then(|| {
            note(
                "refused",
                if binding(&ui.settings, target).is_some_and(|binding| binding.key.as_ref().is_some_and(HotkeyKey::is_mouse)) {
                    tr("Configure mouse shortcuts in your Wayland desktop using the hex commands")
                } else {
                    tr("Your desktop kept a shortcut for itself. Record another one, or free it in your desktop's keyboard settings (on niri, remove its bind from config.kdl).")
                },
                p.danger(),
            )
        })
}

/// Without a shortcuts portal the desktop's keybindings own every shortcut:
/// the commands to bind there.
pub(super) fn desktop_commands(p: Paint) -> El {
    let rows = vec![
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
        command_row(
            p,
            "raw-dictation",
            tr("Dictate without processing"),
            tr("Paste your words as Hex heard them, skipping every step"),
            &["hex dictation toggle --raw"],
        ),
    ];
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

/// Lists keyboard input sources every few seconds while General shows the
/// switching rows. The window lists them itself on Windows and Linux.
#[cfg(not(target_os = "macos"))]
pub(super) fn poll_input_sources(ui: &mut Ui, clock: f64) -> bool {
    let switching =
        ui.platform.caps.keyboard_model_switching && ui.settings.keyboard_model_switching.enabled;
    if !switching {
        let changed = !ui.platform.input_sources.is_empty();
        ui.platform.input_sources.clear();
        return changed;
    }
    if ui.pane != super::Pane::General
        || !ui.live()
        || !ui.window.as_ref().is_some_and(rune::AppHandle::is_visible)
        || clock < ui.input_sources_listed_at + 2.0
    {
        return false;
    }
    ui.input_sources_listed_at = clock;
    let listed = crate::keyboard_input_sources::enabled();
    let changed = ui.platform.input_sources != listed;
    ui.platform.input_sources = listed;
    changed
}

/// One row per enabled input source: "Don't switch" or an installed model,
/// then a language that model supports, as the Language row offers it.
pub(super) fn input_source_rows(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Vec<El> {
    ui.platform
        .input_sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let mapped = ui
                .settings
                .keyboard_model_switching
                .mappings
                .get(&source.id)
                .cloned();
            let source_id = source.id.clone();
            let mut choices = vec![Choice {
                label: tr("Don't switch").into(),
                detail: None,
                selected: mapped.is_none(),
                choose: Box::new(move |ui| {
                    ui.open = None;
                    ui.update(|settings| {
                        settings
                            .keyboard_model_switching
                            .mappings
                            .remove(&source_id);
                    });
                }),
            }];
            choices.extend(transcription_models::MODELS.iter().filter_map(|model| {
                let fallback = ui.settings.transcription_for_model(model.id).language;
                if !model.available()
                    || !matches!(model.runtime, transcription_models::ModelRuntime::Gguf(_))
                    || !ui.picker.installed(model, &fallback)
                {
                    return None;
                }
                let language = match &mapped {
                    Some(choice) if model.supports_language(&choice.language) => {
                        choice.language.clone()
                    }
                    _ => source.language_for(model, &fallback),
                };
                let source_id = source.id.clone();
                let id = model.id;
                Some(Choice {
                    label: model.name.into(),
                    detail: None,
                    selected: mapped.as_ref().is_some_and(|choice| choice.model == id),
                    choose: Box::new(move |ui| {
                        ui.open = None;
                        ui.update(|settings| {
                            settings.keyboard_model_switching.mappings.insert(
                                source_id.clone(),
                                KeyboardModelChoice {
                                    model: id,
                                    language: language.clone(),
                                },
                            );
                        });
                    }),
                })
            }));
            let label = mapped.as_ref().map_or_else(
                || tr("Don't switch").to_string(),
                |choice| {
                    let model = transcription_models::definition(choice.model);
                    if ui.picker.installed(model, &choice.language) {
                        model.name.into()
                    } else {
                        format!("{} · {}", model.name, tr("Not installed"))
                    }
                },
            );
            let mut controls = row().gap(8.0).center_y();
            if let Some(choice) = &mapped {
                controls =
                    controls.child(input_source_language(ui, p, index, source, choice, layer));
            }
            setting_row(
                p,
                &format!("input-source{index}"),
                &source.name,
                None,
                controls.child(select(
                    p,
                    &format!("input-source-model{index}"),
                    &label,
                    ui.open == Some(Menu::InputSourceModel(index)),
                    choices,
                    move |ui| toggle_menu(ui, Menu::InputSourceModel(index)),
                    layer,
                )),
            )
        })
        .collect()
}

/// A mapped source's language: a select, or fixed for one-language models.
fn input_source_language(
    ui: &Ui,
    p: Paint,
    index: usize,
    source: &InputSource,
    choice: &KeyboardModelChoice,
    layer: &mut Vec<El>,
) -> El {
    let key = format!("input-source-language{index}");
    let current = transcription_models::language_name(&choice.language);
    let codes = transcription_models::definition(choice.model).language_choices();
    if codes.len() <= 1 {
        return fixed_select(p, &key, current);
    }
    let choices = codes
        .into_iter()
        .map(|code| {
            let source_id = source.id.clone();
            let model = choice.model;
            Choice {
                label: transcription_models::language_name(code).to_owned(),
                detail: None,
                selected: code == choice.language,
                choose: Box::new(move |ui: &mut Ui| {
                    ui.open = None;
                    ui.update(|settings| {
                        settings.keyboard_model_switching.mappings.insert(
                            source_id.clone(),
                            KeyboardModelChoice {
                                model,
                                language: code.to_owned(),
                            },
                        );
                    });
                }),
            }
        })
        .collect();
    select(
        p,
        &key,
        current,
        ui.open == Some(Menu::InputSourceLanguage(index)),
        choices,
        move |ui| toggle_menu(ui, Menu::InputSourceLanguage(index)),
        layer,
    )
}

/// The speech model row's description: preparation progress or its error.
pub(super) fn transcription_status(ui: &Ui, p: Paint) -> Option<(String, rune::Color)> {
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
pub(super) enum Face {
    /// What pressing does (Change shortcut), or that it just saved; the
    /// shortcut's keys sit in their tray beside the recorder.
    Shortcut {
        saved: bool,
    },
    Listening(Prompt),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Prompt {
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

/// The recorder's key for `target`.
pub(super) fn hotkey_key(target: ShortcutTarget) -> &'static str {
    match target {
        ShortcutTarget::Dictation => "dictation-hotkey",
        ShortcutTarget::PasteLast => "paste-last-hotkey",
        ShortcutTarget::RawDictation => "raw-dictation-hotkey",
        #[cfg(hex_capture)]
        ShortcutTarget::Capture => "capture-hotkey",
        #[cfg(target_os = "macos")]
        ShortcutTarget::SecondDictation => "second-dictation-hotkey",
    }
}

/// What `target`'s recorder shows now.
pub(super) fn face(ui: &Ui, target: ShortcutTarget) -> Face {
    match ui.capture {
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
    }
}

/// In from below, out through the top, both inside the recorder's clip.
fn roll() -> Transition {
    Transition::push(Edge::Bottom)
}

pub(super) fn face_label(face: Face) -> &'static str {
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
pub(super) fn recorder_face(p: Paint, face: Face, live: bool) -> El {
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
                        let held = held_caps(&HotkeyBinding {
                            modifiers: held,
                            key: None,
                        });
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
                                held.iter().enumerate().map(|(index, cap)| {
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

/// The keys held while recording, each with its side, so an extra key or
/// the second Option shows before it is recorded: compact on the Mac
/// (L⌥ R⌥), named elsewhere.
fn held_caps(held: &HotkeyBinding) -> Vec<String> {
    if cfg!(target_os = "macos") {
        held.keycaps()
    } else {
        shortcut_caps(held, false)
    }
}

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

pub(super) fn double_tap_only_applies(settings: &AppSettings) -> bool {
    settings.shortcut_mode == ShortcutMode::Hold
        && settings.double_tap_lock
        && settings.dictation_hotkey.key.is_some()
}

pub(super) fn release_confirmation(_ui: &Ui, p: Paint) -> El {
    setting_row(
        p,
        "confirm-release",
        tr("Release the microphone when idle?"),
        Some(tr(
            "Opens the microphone on shortcut press without pre-roll.",
        )),
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
                        if ui.update(|settings| settings.release_microphone_while_idle = true) {
                            ui.confirm_release = false;
                        }
                    },
                )
                .default_button(),
            ),
    )
    .fill(p.k.c.hover)
}

/// Stable or Dev shows only while this install updates itself: test,
/// source, managed, and package-manager installs never check a channel.
pub(super) fn updates_itself(ui: &Ui) -> bool {
    ui.view.as_ref().is_some_and(|view| {
        matches!(
            view.update_status,
            DesktopUpdateStatus::Checking
                | DesktopUpdateStatus::Current
                | DesktopUpdateStatus::Failed
                | DesktopUpdateStatus::Downloading
                | DesktopUpdateStatus::Available
                | DesktopUpdateStatus::ReadyToRestart
                | DesktopUpdateStatus::SwitchAvailable { .. }
        )
    })
}
