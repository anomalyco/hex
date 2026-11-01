use super::settings::{desktop_shortcut, recorded_binding, validate};
use super::*;
use crate::transcription::models::TranscriptionSelection;
use rune::render::Gpu;
use rune::text::TextSystem;
use rune::{Env, Key, KeyEvent, Mods, Runtime};

/// Tall enough to show every General section without scrolling.
const GOLDEN_SIZE: (f32, f32) = (980.0, 1720.0);
/// The window's own size, for the other panes.
const PANE_SIZE: (f32, f32) = (980.0, 760.0);
const GOLDEN_SCALE: f32 = 2.0;

fn runtime(scene: &str, dark: bool, size: (f32, f32)) -> Runtime<Ui> {
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    let (_, apply) = SCENES
        .iter()
        .find(|(name, _)| *name == scene)
        .unwrap_or_else(|| panic!("no scene {scene}"));
    apply(&mut state);
    runtime_for(state, dark, size)
}

fn runtime_for(state: Ui, dark: bool, size: (f32, f32)) -> Runtime<Ui> {
    let env = Env {
        system_dark: dark,
        scale: GOLDEN_SCALE,
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = crate::rune_ui::offscreen::runtime(state, view, size, env, text);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    // Fixtures show the state at rest, so sequences that play on mount
    // (a granted permission's Allowed) land before the first frame.
    rt.settle_all();
    rt.frame(0.0);
    rt
}

fn sent(rt: &Runtime<Ui>) -> &[Request] {
    match &rt.state.service {
        Service::Fixture(sent) => sent,
        _ => panic!("fixtures never reach a host"),
    }
}

fn saved(rt: &Runtime<Ui>) -> usize {
    rt.state.saved
}

/// The text shown, without the recorder's invisible sizing faces.
fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.path.contains("/faces/size"))
        .filter_map(|e| e.text)
        .collect()
}

/// Presses an element by key and lets every animation settle.
fn tap(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Chooses option `index` from the quiet select keyed `{key}-control`.
fn pick(rt: &mut Runtime<Ui>, key: &str, index: usize) {
    tap(rt, &format!("{key}-control-trigger"));
    tap_option(rt, index);
}

/// Scrolls an open select's list to option `index` and presses it.
fn tap_option(rt: &mut Runtime<Ui>, index: usize) {
    let key = format!("option{index}");
    for _ in 0..400 {
        let option = element(rt, &key).rect;
        let list = element(rt, "listbox").rect;
        let middle = option[1] + option[3] / 2.0;
        if middle > list[1] && middle < list[1] + list[3] {
            break;
        }
        let dy = if middle > list[1] + list[3] {
            20.0
        } else {
            -20.0
        };
        rt.wheel(list[0] + list[2] / 2.0, list[1] + list[3] / 2.0, dy);
        rt.frame(1.0 / 60.0);
    }
    tap(rt, &key);
}

/// A window tall enough for General with its Advanced fold open.
const TALL_SIZE: (f32, f32) = (980.0, 2800.0);

/// Opens a pane's Advanced fold, as its toggle does.
fn open_advanced(rt: &mut Runtime<Ui>, pane: Pane) {
    if !rt.state.advanced.contains(&pane) {
        rt.state.advanced.push(pane);
    }
    rt.invalidate();
    rt.settle_all();
    rt.frame(0.0);
    settle(rt);
}

fn key(rt: &mut Runtime<Ui>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    rt.frame(1.0 / 60.0);
}

#[test]
fn portal_desktops_show_what_they_granted_beside_each_recorder() {
    let mut rt = runtime("portal", true, TALL_SIZE);
    open_advanced(&mut rt, Pane::General);
    let shown = texts(&rt);
    for expected in [
        "Shortcut",
        "Your desktop granted F12",
        "Your desktop granted Ctrl+Alt+V",
    ] {
        assert!(
            shown.iter().any(|text| text == expected),
            "missing {expected:?} in {shown:?}"
        );
    }
    assert!(!shown.iter().any(|text| text == "Desktop shortcuts"));
    for recorder in ["dictation-hotkey", "paste-last-hotkey"] {
        tap(&mut rt, recorder);
        assert!(matches!(
            rt.state.capture,
            settings::Capture::Listening { .. }
        ));
        key(&mut rt, Key::Escape, Mods::default());
        assert_eq!(rt.state.capture, settings::Capture::Idle);
    }
}

#[test]
fn a_mouse_shortcut_on_wayland_explains_the_desktop_command_route() {
    let rt = runtime("portal-mouse", true, GOLDEN_SIZE);
    let shown = texts(&rt);
    assert!(
        shown
            .iter()
            .any(|text| text.contains("Wayland desktop using the hex commands"))
    );
    assert!(!shown.iter().any(|text| text.contains("config.kdl")));
}

#[test]
fn a_refused_portal_shortcut_says_how_to_free_it() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    if let Some(view) = &mut rt.state.view {
        view.listener.shortcuts = DesktopShortcuts::Portal(vec![DesktopGrantedShortcut {
            name: "Dictation".into(),
            trigger: None,
            unconfirmed: false,
        }]);
    }
    rt.invalidate();
    rt.frame(0.0);
    let shown = texts(&rt);
    assert!(
        shown
            .iter()
            .any(|text| text == "Your desktop did not grant this shortcut")
    );
    assert!(shown.iter().any(|text| text.contains("config.kdl")));
    assert!(rt.find("dictation-hotkey").is_some());
    assert!(rt.find("basics/refused").is_some());
    open_advanced(&mut rt, Pane::General);
    assert!(rt.find("output/card/refused").is_none());
}

/// #635: Hyprland's portal names no trigger, since its config binds the
/// keys, so the row says Hex can't confirm it and shows no refusal.
#[test]
fn an_unconfirmed_portal_shortcut_is_not_called_refused() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    if let Some(view) = &mut rt.state.view {
        view.listener.shortcuts = DesktopShortcuts::Portal(vec![DesktopGrantedShortcut {
            name: "Dictation".into(),
            trigger: None,
            unconfirmed: true,
        }]);
    }
    rt.invalidate();
    rt.frame(0.0);
    let shown = texts(&rt);
    assert!(
        shown
            .iter()
            .any(|text| text == "Your desktop assigns this key itself, so Hex can't confirm it")
    );
    assert!(
        !shown
            .iter()
            .any(|text| text == "Your desktop did not grant this shortcut")
    );
    assert!(rt.find("basics/refused").is_none());
}

#[test]
fn a_refused_note_sits_with_the_shortcut_the_desktop_kept() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    if let Some(view) = &mut rt.state.view {
        view.listener.shortcuts = DesktopShortcuts::Portal(vec![
            DesktopGrantedShortcut {
                name: "Dictation".into(),
                trigger: Some("F12".into()),
                unconfirmed: false,
            },
            DesktopGrantedShortcut {
                name: "Paste last dictation".into(),
                trigger: None,
                unconfirmed: false,
            },
        ]);
    }
    rt.invalidate();
    rt.frame(0.0);
    open_advanced(&mut rt, Pane::General);
    assert!(rt.find("output/card/refused").is_some());
    assert!(rt.find("basics/refused").is_none());
}

#[test]
fn each_panes_advanced_fold_starts_closed_and_stays_its_own() {
    let mut rt = runtime("portal", true, TALL_SIZE);
    assert!(rt.find("double-tap-only").is_none(), "closed at first");
    tap(&mut rt, "advanced-toggle");
    tap(&mut rt, "advanced-toggle");
    tap(&mut rt, "advanced-toggle");
    rt.state.settings.double_tap_lock = true;
    rt.state.settings.double_tap_only = true;
    rt.invalidate();
    settle(&mut rt);
    assert!(rt.find("double-tap-only").is_some());
    assert_eq!(rt.state.advanced, vec![Pane::General]);
}

#[test]
fn desktops_without_a_portal_list_commands_instead_of_recording_rows() {
    let rt = runtime("commands", true, GOLDEN_SIZE);
    assert!(texts(&rt).iter().any(|text| text == "hex dictation toggle"));
    assert!(rt.find("dictation-hotkey").is_none());
    assert!(rt.find("paste-last-hotkey").is_none());
}

#[test]
fn toggles_save_through_the_host() {
    let mut rt = runtime("portal", true, TALL_SIZE);
    assert!(rt.state.settings.paste_with_shift);
    open_advanced(&mut rt, Pane::General);
    tap(&mut rt, "terminal-paste");
    assert!(!rt.state.settings.paste_with_shift);
    assert_eq!(saved(&rt), 1);
    assert!(sent(&rt).is_empty());
}

#[test]
fn restore_default_brings_back_paste_last_only_with_its_lab_on() {
    use crate::app_settings::HotkeyBinding;
    let default = HotkeyBinding::paste_last_default();
    let mut rt = runtime("portal", true, TALL_SIZE);
    open_advanced(&mut rt, Pane::General);
    rt.state.settings.paste_last_hotkey = None;
    rt.invalidate();
    settle(&mut rt);
    assert!(
        rt.find("restore-paste-last").is_none(),
        "hidden while Labs › Restore shortcut default is off"
    );

    rt.state.settings.labs.restore_shortcut_default = true;
    rt.invalidate();
    settle(&mut rt);
    let note = format!("Default shortcut: {}", default.keycaps().join("+"));
    assert!(
        texts(&rt).contains(&note),
        "the empty row names its default"
    );
    tap(&mut rt, "restore-paste-last");
    assert_eq!(rt.state.settings.paste_last_hotkey, Some(default.clone()));
    assert_eq!(saved(&rt), 1);
    assert!(
        rt.find("restore-paste-last").is_none(),
        "hidden once the shortcut is the default"
    );

    // A default another shortcut already uses is refused, as recording it would be.
    rt.state.settings.paste_last_hotkey = None;
    rt.state.settings.raw_dictation_hotkey = Some(default);
    rt.invalidate();
    settle(&mut rt);
    tap(&mut rt, "restore-paste-last");
    assert_eq!(rt.state.settings.paste_last_hotkey, None);
    assert_eq!(saved(&rt), 1);
    assert!(matches!(
        rt.state.capture,
        settings::Capture::Listening {
            message: Some(_),
            ..
        }
    ));
}

#[test]
fn choosing_an_update_channel_saves_it_and_checks_now() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "advanced-toggle");
    assert_eq!(rt.state.settings.updates.channel, None);
    let (other, index) = other_update_channel();
    pick(&mut rt, "update-channel", index);
    assert_eq!(rt.state.settings.updates.channel, Some(other));
    assert_eq!(saved(&rt), 1);
    assert!(matches!(
        sent(&rt),
        [Request::Desktop(DesktopAction::CheckForUpdates)]
    ));
    pick(&mut rt, "update-channel", index);
    assert_eq!(
        saved(&rt),
        1,
        "choosing the same channel again saves nothing"
    );
}

/// The channel this build does not follow, and its option's index: the
/// build's own channel depends on its version (a local build follows dev).
fn other_update_channel() -> (crate::app_settings::UpdateChannel, usize) {
    let build = crate::updater::build_channel();
    let index = crate::app_settings::UpdateChannel::ALL
        .iter()
        .position(|channel| *channel != build)
        .unwrap();
    (crate::app_settings::UpdateChannel::ALL[index], index)
}

#[test]
fn choosing_the_channel_in_effect_keeps_following_the_build() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "advanced-toggle");
    assert_eq!(rt.state.settings.updates.channel, None);
    let build = crate::updater::build_channel();
    let index = crate::app_settings::UpdateChannel::ALL
        .iter()
        .position(|channel| *channel == build)
        .unwrap();
    pick(&mut rt, "update-channel", index);
    assert_eq!(rt.state.settings.updates.channel, None);
    assert_eq!(saved(&rt), 0);
    assert!(sent(&rt).is_empty());
}

#[test]
fn installs_that_never_update_hide_the_update_channel() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "advanced-toggle");
    assert!(rt.find("update-channel").is_some());
    if let Some(view) = &mut rt.state.view {
        view.update_status = DesktopUpdateStatus::PackageManager;
    }
    rt.invalidate();
    settle(&mut rt);
    assert!(rt.find("update-channel").is_none());
    if let Some(view) = &mut rt.state.view {
        view.update_status = DesktopUpdateStatus::SourceBuild;
    }
    rt.invalidate();
    settle(&mut rt);
    assert!(rt.find("update-channel").is_none());
}

#[test]
fn quit_hex_in_the_sidebar_asks_the_host_to_quit() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    tap(&mut rt, "quit");
    assert!(rt.state.quit_requested);
}

#[test]
fn ctrl_q_quits_hex() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    key(&mut rt, Key::Command('q'), Mods::default());
    assert!(rt.state.quit_requested);
}

#[test]
fn enabling_sends_the_listener_action() {
    let mut rt = runtime("disabled", true, GOLDEN_SIZE);
    tap(&mut rt, "toggle-listening");
    assert!(matches!(
        sent(&rt),
        [Request::Desktop(DesktopAction::StartListening)]
    ));
}

#[test]
fn the_microphone_switch_sits_beside_the_picker_and_needs_no_confirmation() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    assert!(
        rt.find("basics/microphone-mode").is_some(),
        "in General, not Advanced"
    );
    let ready = "The mic stays ready so dictation starts instantly; your system shows it in use";
    let released = "The mic opens only while you dictate; the first moment can be clipped";
    assert!(texts(&rt).iter().any(|text| text == ready));
    tap(&mut rt, "microphone-mode");
    assert!(rt.state.settings.release_microphone_while_idle);
    assert!(texts(&rt).iter().any(|text| text == released));
    tap(&mut rt, "microphone-mode");
    assert!(!rt.state.settings.release_microphone_while_idle);
}

#[test]
fn recording_a_shortcut_sends_the_chord_to_the_host() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "dictation-hotkey");
    assert!(matches!(
        sent(&rt),
        [Request::CaptureShortcutFor(ShortcutTarget::Dictation)]
    ));
    key(&mut rt, Key::Text("d".into()), Mods::default());
    assert!(matches!(
        rt.state.capture,
        Capture::Listening {
            message: Some("Add a modifier"),
            ..
        }
    ));
    let mods = Mods {
        ctrl: true,
        alt: true,
        cmd: true,
        ..Mods::default()
    };
    key(&mut rt, Key::Command('d'), mods);
    let Some(Request::SetShortcut { target, shortcut }) = sent(&rt).last() else {
        panic!("no shortcut sent: {:?}", sent(&rt));
    };
    assert_eq!(*target, ShortcutTarget::Dictation);
    assert!(shortcut.control && shortcut.alt && !shortcut.shift && !shortcut.platform);
    assert_eq!(shortcut.key, "d");
    assert!(matches!(rt.state.capture, Capture::Saving { .. }));
}

#[cfg(target_os = "linux")]
#[test]
fn a_modifier_alone_says_linux_needs_a_key_and_suggests_one() {
    use rune::Modifier;
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    // As the Linux app asks (`rune_ui::build`).
    rt.set_modifier_keys(true);
    tap(&mut rt, "dictation-hotkey");
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    let modifier = |modifier, pressed| Key::Modifier { modifier, pressed };
    key(&mut rt, modifier(Modifier::AltLeft, true), alt);
    assert!(matches!(
        rt.state.capture,
        Capture::Listening { message: None, held, .. } if held.option.is_some()
    ));
    key(&mut rt, modifier(Modifier::AltLeft, false), Mods::default());
    assert!(matches!(
        rt.state.capture,
        Capture::Listening {
            message: Some("Linux needs a key too. Hold these and press one, like Space"),
            ..
        }
    ));
    // Both Alts: Linux binds no side, so it says that instead.
    key(&mut rt, modifier(Modifier::AltRight, true), alt);
    key(&mut rt, modifier(Modifier::AltLeft, true), alt);
    key(&mut rt, modifier(Modifier::AltLeft, false), alt);
    assert!(matches!(
        rt.state.capture,
        Capture::Listening { message: None, .. }
    ));
    key(
        &mut rt,
        modifier(Modifier::AltRight, false),
        Mods::default(),
    );
    assert!(matches!(
        rt.state.capture,
        Capture::Listening {
            message: Some("Linux can't bind both sides. Use one with a key, like Space"),
            ..
        }
    ));
    // Modifiers spent on a key say nothing when released.
    key(&mut rt, modifier(Modifier::AltLeft, true), alt);
    key(&mut rt, Key::Text("d".into()), alt);
    assert!(matches!(rt.state.capture, Capture::Saving { .. }));
    assert!(sent(&rt).iter().any(|request| matches!(
        request,
        Request::SetShortcut { shortcut, .. } if shortcut.alt && shortcut.key == "d"
    )));
}

#[test]
fn escape_cancels_a_shortcut_recording() {
    let mut rt = runtime("capturing", true, GOLDEN_SIZE);
    key(&mut rt, Key::Escape, Mods::default());
    assert_eq!(rt.state.capture, Capture::Idle);
    assert!(matches!(sent(&rt), [Request::CancelShortcut]));
}

#[test]
fn a_recorder_shows_the_modifiers_held_so_far() {
    use crate::app_settings::{HotkeyModifiers, ModifierSide};
    let mut rt = runtime("capturing", true, GOLDEN_SIZE);
    let held = HotkeyModifiers {
        option: Some(ModifierSide::Either),
        command: Some(ModifierSide::Either),
        ..HotkeyModifiers::default()
    };
    settings::hold(&mut rt.state, held);
    rt.invalidate();
    settle(&mut rt);
    let shown = texts(&rt);
    let caps = HotkeyBinding {
        modifiers: held,
        key: None,
    }
    .keycaps();
    for expected in caps.iter().map(String::as_str).chain(["…"]) {
        assert!(
            shown.iter().any(|text| text == expected),
            "missing {expected:?} in {shown:?}"
        );
    }
    assert!(!shown.iter().any(|text| text == "Press shortcut"));
    settings::hold(&mut rt.state, HotkeyModifiers::default());
    rt.invalidate();
    settle(&mut rt);
    assert!(texts(&rt).iter().any(|text| text == "Press shortcut"));
}

#[test]
fn function_keys_and_super_record_as_linux_key_names() {
    let f12 = recorded_binding(&KeyEvent {
        key: Key::Function(12),
        mods: Mods::default(),
    })
    .unwrap();
    assert_eq!(desktop_shortcut(&f12).key, "f12");
    let space = recorded_binding(&KeyEvent {
        key: Key::Text(" ".into()),
        mods: Mods {
            logo: true,
            ..Mods::default()
        },
    })
    .unwrap();
    let shortcut = desktop_shortcut(&space);
    assert_eq!(shortcut.key, "space");
    assert!(shortcut.platform);
}

#[test]
fn shortcuts_that_collide_are_refused() {
    let ui = portal_fixture();
    let paste = ui.settings.paste_last_hotkey.clone().unwrap();
    assert_eq!(
        validate(&ui.settings, ShortcutTarget::Dictation, &paste),
        Err("Already in use")
    );
}

#[test]
fn service_settings_are_adopted_only_when_the_service_changes_them() {
    let mut ui = portal_fixture();
    let service = ui.settings.clone();
    ui.adopt(service.clone());
    ui.update(|settings| settings.paste_with_shift = false);
    // The service has not reloaded yet: its stale copy must not win.
    ui.adopt(service.clone());
    assert!(!ui.settings.paste_with_shift);
    let mut changed = service;
    changed.enable_on_launch = false;
    ui.adopt(changed);
    assert!(!ui.settings.enable_on_launch);
}

#[test]
fn a_draft_saves_over_what_another_writer_changed_meanwhile() {
    let mut ui = portal_fixture();
    let service = ui.settings.clone();
    ui.adopt(service.clone());
    ui.vocabulary
        .push(TextEdit::new("OpenCode"), TextEdit::default());
    let start = std::time::Instant::now();
    assert!(!ui.save_drafts(start), "waits for a pause");
    // `hex settings set` (or a capture's saved rule) saves while it waits.
    let mut elsewhere = service;
    elsewhere.enable_on_launch = !elsewhere.enable_on_launch;
    ui.adopt(elsewhere.clone());
    assert_eq!(
        ui.vocabulary.project()[0].term,
        "OpenCode",
        "the typing stays"
    );
    assert!(ui.save_drafts(start + DRAFT_DELAY));
    assert_eq!(ui.settings.enable_on_launch, elsewhere.enable_on_launch);
    assert_eq!(ui.settings.vocabulary[0].term, "OpenCode");
}

/// Typing in one editor keeps only that editor's text: the others take
/// what another writer saved, so the draft's save doesn't undo it.
#[test]
fn a_draft_keeps_only_its_own_editor() {
    let mut ui = portal_fixture();
    let service = ui.settings.clone();
    ui.adopt(service.clone());
    ui.vocabulary
        .push(TextEdit::new("OpenCode"), TextEdit::default());
    let start = std::time::Instant::now();
    assert!(!ui.save_drafts(start), "waits for a pause");
    let mut elsewhere = service;
    elsewhere
        .dictation_processing
        .default_mode
        .post_processing
        .prompt = "Keep it short.".into();
    ui.adopt(elsewhere);
    assert!(ui.save_drafts(start + DRAFT_DELAY));
    assert_eq!(ui.settings.vocabulary[0].term, "OpenCode");
    assert_eq!(
        ui.settings
            .dictation_processing
            .default_mode
            .post_processing
            .prompt,
        "Keep it short."
    );
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
}

/// Saves paused drafts now, as the next poll after [`DRAFT_DELAY`] would.
fn flush(rt: &mut Runtime<Ui>) {
    rt.state.flush_drafts();
    settle(rt);
}

/// A shown window keeps drawing while its runtime animates, has input queued
/// or has live values pending (Rune's `Host::redraw`). Five seconds after
/// mounting, with nothing changing, the only motion left in any scene is the
/// pulsing dot of something live, Output's turning spark, or the
/// shortcut explainer's looping demo, and under Reduce Motion, which stills
/// them all, no scene asks for another frame.
#[test]
fn every_scene_stops_asking_for_frames() {
    const FRAME: f64 = 1.0 / 60.0;
    let wants_frame =
        |rt: &mut Runtime<Ui>| rt.frame(FRAME) || rt.has_pending_input() || rt.live_pending();
    let mut moving = Vec::new();
    let mut drawing = Vec::new();
    for (scene, _) in SCENES.iter().filter(|(name, _)| *name != "live") {
        let mut rt = runtime(scene, true, PANE_SIZE);
        for _ in 0..150 {
            rt.frame(1.0 / 30.0);
        }
        if *scene != "shortcut-explainer-motion" {
            moving.extend(
                rt.elements()
                    .into_iter()
                    .filter(|e| e.moving && !matches!(e.key.as_deref(), Some("dot")) && !spark(e))
                    .map(|e| format!("{scene}: {}", e.path)),
            );
        }
        // Stilling a loop may land a spring or two first.
        rt.set_system_reduced_motion(true);
        for _ in 0..30 {
            rt.frame(1.0 / 30.0);
        }
        if (0..30).any(|_| wants_frame(&mut rt)) {
            drawing.push(*scene);
        }
    }
    assert!(moving.is_empty(), "still moving after 5 s: {moving:#?}");
    assert!(
        drawing.is_empty(),
        "still asking for frames under Reduce Motion: {drawing:?}"
    );
}

#[test]
fn the_sidebar_always_shows_output() {
    for scene in ["portal", "mac-settings", "mac-production"] {
        let rt = runtime(scene, true, PANE_SIZE);
        assert!(
            rt.find("nav-output").is_some(),
            "{scene} shows Output in the main sidebar"
        );
        assert!(Pane::Output.shown(&rt.state));
        assert!(rt.find("nav-modes").is_none(), "Output replaces Modes");
    }
}

#[test]
fn the_sidebar_switches_panes_and_keeps_settings() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    for (key, pane) in [
        ("nav-output", Pane::Output),
        ("nav-history", Pane::History),
        ("nav-general", Pane::General),
    ] {
        tap(&mut rt, key);
        assert_eq!(rt.state.pane, pane);
    }
    assert!(sent(&rt).is_empty());
    assert_eq!(saved(&rt), 0, "switching panes saves nothing");
}

#[test]
fn the_sidebar_switches_on_press_not_release() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    settle(&mut rt);
    let (x, y) = rt.find("nav-output").expect("the Output item");
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.pane, Pane::Output);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.pane, Pane::Output);
}

#[test]
fn switching_panes_is_instant() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    tap(&mut rt, "nav-labs");
    settle(&mut rt);
    let (x, y) = rt.find("nav-output").expect("the Output item");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    let panel: Vec<_> = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.contains("/panel/"))
        .collect();
    assert!(!panel.is_empty());
    assert!(
        !panel.iter().any(|e| e.key.as_deref() == Some("lede")),
        "Labs left at once"
    );
    // Output's spark turns on its own; it is not a pane transition.
    for element in panel.iter().filter(|e| !spark(e)) {
        assert!(
            element.opacity >= 0.999 && !element.moving,
            "{} is still animating",
            element.path
        );
    }
}

/// A frame of Output's turning spark or a word of its cycling examples,
/// which move on their own.
fn spark(e: &rune::ElementInfo) -> bool {
    e.key
        .as_deref()
        .is_some_and(|key| key.starts_with("spark") || key.starts_with("example"))
}

fn element(rt: &Runtime<Ui>, key: &str) -> rune::ElementInfo {
    let suffix = format!("/{key}");
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && (e.key.as_deref() == Some(key) || e.path.ends_with(&suffix)))
        .unwrap_or_else(|| panic!("no element {key}"))
}

/// The sidebar's items, top to bottom, with their drawn rects.
fn nav_items(rt: &Runtime<Ui>) -> Vec<(Pane, [f32; 4])> {
    let mut items: Vec<_> = Pane::ALL
        .iter()
        .copied()
        .filter_map(|pane| rt.presented(pane.key()).map(|(rect, _)| (pane, rect)))
        .collect();
    items.sort_by(|a, b| a.1[1].total_cmp(&b.1[1]));
    items
}

/// A sidebar row: its pane and rect.
type NavRow = (Pane, [f32; 4]);

/// Neighbouring sidebar items with nothing but a gap between them: the
/// Labs row sits below the panes.
fn nav_neighbours(rt: &Runtime<Ui>) -> Vec<(NavRow, NavRow)> {
    let labs = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("nav-labs"))
        .map(|e| e.rect);
    nav_items(rt)
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .filter(|((_, a), (_, b))| {
            labs.is_none_or(|labs| labs[1] + labs[3] <= a[1] + a[3] || labs[1] >= b[1])
        })
        .collect()
}

/// The items whose hover shows, after one short frame: hover is instant.
fn hovered_items(rt: &mut Runtime<Ui>, x: f32, y: f32) -> Vec<Pane> {
    rt.pointer_move(x, y);
    rt.frame(1.0 / 120.0);
    nav_items(rt)
        .into_iter()
        .filter(|(pane, _)| {
            let opacity = element(rt, &format!("{}/hover", pane.key())).opacity;
            assert!(
                opacity == 0.0 || opacity == 1.0,
                "{pane:?}'s hover is fading: {opacity}"
            );
            opacity == 1.0
        })
        .map(|(pane, _)| pane)
        .collect()
}

/// A sidebar with Labs features turned on, as a developer sees it.
fn grouped_sidebar() -> Runtime<Ui> {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    rt.state.platform.developer = true;
    rt.state.labs_open = true;
    rt.invalidate();
    settle(&mut rt);
    assert!(
        nav_items(&rt)
            .iter()
            .any(|(pane, _)| *pane == Pane::VoiceFilter)
    );
    rt
}

#[test]
fn the_nearest_sidebar_item_hovers_in_every_gap() {
    let mut rt = grouped_sidebar();
    let items = nav_items(&rt);
    let x = items[0].1[0] + 40.0;
    for ((above, a), (below, b)) in nav_neighbours(&rt) {
        let (gap_top, gap_bottom) = (a[1] + a[3], b[1]);
        let middle = (gap_top + gap_bottom) / 2.0;
        assert_eq!(hovered_items(&mut rt, x, a[1] + a[3] / 2.0), [above]);
        assert_eq!(hovered_items(&mut rt, x, gap_top + 0.5), [above]);
        assert_eq!(hovered_items(&mut rt, x, middle - 0.5), [above]);
        assert_eq!(hovered_items(&mut rt, x, middle + 0.5), [below]);
        assert_eq!(hovered_items(&mut rt, x, gap_bottom - 0.5), [below]);
    }
    let (_, last) = items[items.len() - 1];
    assert!(
        hovered_items(&mut rt, x, last[1] + last[3] + 200.0).is_empty(),
        "hover leaves at once"
    );
}

#[test]
fn a_press_in_a_sidebar_gap_selects_the_nearest_item() {
    let mut rt = grouped_sidebar();
    let items = nav_items(&rt);
    let x = items[0].1[0] + 40.0;
    for ((above, a), (below, b)) in nav_neighbours(&rt) {
        let middle = (a[1] + a[3] + b[1]) / 2.0;
        for (y, pane) in [(middle - 0.5, above), (middle + 0.5, below)] {
            // A fresh sidebar each time: choosing a main pane folds Labs.
            rt = grouped_sidebar();
            rt.pointer_move(x, y);
            rt.pointer_down(x, y);
            rt.frame(1.0 / 60.0);
            assert_eq!(rt.state.pane, pane, "a press at {y} selects {pane:?}");
            rt.pointer_up(x, y);
        }
    }
}

#[test]
fn a_settings_row_shows_no_hover_or_press_of_its_own() {
    let mut rt = runtime("portal", true, TALL_SIZE);
    open_advanced(&mut rt, Pane::General);
    let before = rt.state.settings.double_tap_lock;
    let (x, y) = rt.find("double-tap/lead/copy").expect("the row's title");
    rt.pointer_move(x, y);
    rt.frame(1.0 / 120.0);
    let row = element(&rt, "double-tap");
    assert_eq!((row.hover, row.fill), (0.0, rune::Color::TRANSPARENT));
    rt.pointer_down(x, y);
    rt.frame(1.0 / 120.0);
    assert_eq!(element(&rt, "double-tap").press, 0.0);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_ne!(
        rt.state.settings.double_tap_lock, before,
        "pressing the row still flips it"
    );
}

#[test]
fn hover_leaves_at_once() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    for key in ["update-status", "dictation-hotkey", "microphone-control"] {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_move(x, y);
        rt.frame(1.0 / 120.0);
        assert_eq!(element(&rt, key).hover, 1.0, "{key} hovers at once");
        rt.pointer_move(x, y + 400.0);
        rt.frame(1.0 / 120.0);
        assert_eq!(element(&rt, key).hover, 0.0, "{key} stops hovering at once");
    }
}

/// Every hover in the tree, kit controls' inner tracks included, is at rest
/// or fully on one short frame after the pointer moves.
fn assert_hover_is_binary(rt: &Runtime<Ui>, what: &str) {
    for e in rt.elements().into_iter().filter(|e| !e.exiting) {
        assert!(
            e.hover == 0.0 || e.hover == 1.0,
            "{what}: {} is fading: {}",
            e.path,
            e.hover
        );
    }
}

#[test]
fn segments_and_switch_tracks_hover_at_once() {
    let mut rt = runtime("portal", true, TALL_SIZE);
    open_advanced(&mut rt, Pane::General);
    let targets = [
        rt.find("shortcut-mode-control-trigger"),
        rt.find("double-tap/switch"),
    ];
    for (target, name) in targets.into_iter().zip(["select", "switch"]) {
        let (x, y) = target.unwrap_or_else(|| panic!("no {name}"));
        rt.pointer_move(x, y);
        rt.frame(1.0 / 120.0);
        assert_hover_is_binary(&rt, name);
        assert!(
            rt.elements().iter().any(|e| e.hover == 1.0),
            "{name} hovers at once"
        );
        // Above the page, where nothing hovers.
        rt.pointer_move(x, 2.0);
        rt.frame(1.0 / 120.0);
        assert_hover_is_binary(&rt, name);
        assert!(
            rt.elements().iter().all(|e| e.hover == 0.0),
            "{name} stops hovering at once"
        );
    }
}

#[test]
fn choosing_an_opencode_model_saves_it() {
    let mut rt = runtime("modes", true, PANE_SIZE);
    rt.state.models.open_picker();
    rt.invalidate();
    settle(&mut rt);
    tap(&mut rt, "model-picker-option-1");
    assert_eq!(
        rt.state.settings.dictation_processing.modes[0]
            .post_processing
            .model
            .as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
    assert!(!rt.state.models.picker.open);
    tap(&mut rt, "model-value");
    assert!(rt.state.models.picker.open && rt.state.models.picker.search.focused);
    tap(&mut rt, "model-picker-option-0");
    assert_eq!(
        rt.state.settings.dictation_processing.modes[0]
            .post_processing
            .model,
        None,
        "back to the OpenCode default"
    );
}

#[test]
fn a_long_model_list_fits_the_window_and_follows_the_highlight() {
    let mut rt = runtime("modes", true, PANE_SIZE);
    rt.state.models = models::Models::fixture_long();
    rt.state.models.open_picker();
    rt.invalidate();
    settle(&mut rt);
    let [_, top, _, height] = rt.presented("viewport").unwrap().0;
    assert!(
        top + height <= PANE_SIZE.1 - 8.0,
        "the list ends inside the window: {top} + {height}"
    );
    for _ in 0..40 {
        key(&mut rt, Key::Down, Mods::default());
    }
    settle(&mut rt);
    assert_eq!(rt.state.models.picker.active.as_deref(), Some("40"));
    let [_, row_top, _, row_height] = rt.presented("model-picker-option-40").unwrap().0;
    assert!(
        row_top >= top && row_top + row_height <= top + height,
        "the highlighted row scrolled into view: {row_top} in {top}..{}",
        top + height
    );
}

#[test]
fn arrow_keys_move_the_search_highlight_and_enter_picks_it() {
    let mut rt = runtime("modes", true, PANE_SIZE);
    rt.state.models.open_picker();
    rt.invalidate();
    settle(&mut rt);
    key(&mut rt, Key::Down, Mods::default());
    key(&mut rt, Key::Down, Mods::default());
    key(&mut rt, Key::Up, Mods::default());
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        rt.state.settings.dictation_processing.modes[0]
            .post_processing
            .model
            .as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
    assert!(!rt.state.models.picker.open);

    let mut rt = runtime("modes-picker", true, GOLDEN_SIZE);
    tap(&mut rt, "application-search");
    // Visual Studio Code, WezTerm (Terminal's), Zed.
    for _ in 0..3 {
        key(&mut rt, Key::Down, Mods::default());
    }
    key(&mut rt, Key::Enter, Mods::default());
    assert!(
        rt.state.settings.dictation_processing.modes[0]
            .applications
            .contains(&"Zed".to_string())
    );
}

#[test]
fn opencode_missing_offers_setup_instead_of_models() {
    let mut rt = runtime("modes", true, PANE_SIZE);
    rt.state.models = models::Models::fixture_not_running();
    rt.state.settings.dictation_processing.modes[0]
        .post_processing
        .enabled = false;
    rt.invalidate();
    settle(&mut rt);
    assert!(rt.find("opencode-unavailable").is_some());
    assert!(rt.find("open-opencode-setup").is_some());
    assert!(rt.find("opencode-model").is_none());
}

#[test]
fn unavailable_opencode_is_explained_beside_each_switch() {
    let beside = |rt: &Runtime<Ui>, row: &str| {
        rt.elements()
            .into_iter()
            .filter(|e| e.path.contains(row) && e.path.ends_with("/description"))
            .filter_map(|e| e.text)
            .collect::<Vec<_>>()
    };
    let rt = runtime("modes-opencode-not-running", true, PANE_SIZE);
    assert!(rt.find("processing-toggle").is_some());
    assert_eq!(
        beside(&rt, "processing-enabled"),
        ["OpenCode isn't running"]
    );
    assert!(
        texts(&rt)
            .iter()
            .any(|text| text == "Install or start OpenCode to transform dictated text.")
    );
}

#[test]
fn history_selects_deletes_and_clears_in_two_steps() {
    let mut rt = runtime("history", true, PANE_SIZE);
    tap(&mut rt, "history-entry3");
    assert_eq!(rt.state.history.selected, Some(3));
    tap(&mut rt, "history-delete");
    assert!(rt.state.history.entries.iter().all(|entry| entry.id != 3));
    tap(&mut rt, "history-clear");
    assert!(rt.state.history.clear_armed);
    assert_eq!(rt.state.history.entries.len(), 2);
    tap(&mut rt, "history-clear");
    assert!(rt.state.history.entries.is_empty());
}

/// What is still changing one short frame after an input: hover and press
/// overlays between their ends, fades, travel, and fills that differ from
/// where they settle.
fn still_changing(rt: &mut Runtime<Ui>, within: &str) -> Vec<String> {
    rt.frame(1.0 / 120.0);
    let now = rt.elements();
    settle(rt);
    let later = rt.elements();
    let mut changing = Vec::new();
    for e in now.iter().filter(|e| e.path.contains(within) && !e.exiting) {
        let rest = later.iter().find(|l| l.path == e.path && !l.exiting);
        if e.moving || (e.opacity > 0.001 && e.opacity < 0.999) {
            changing.push(format!("{} moving or fading ({})", e.path, e.opacity));
        }
        if let Some(rest) = rest
            && (e.fill != rest.fill
                || e.text_color != rest.text_color
                || (e.hover - rest.hover).abs() > 0.001
                || (e.press - rest.press).abs() > 0.001
                || (e.opacity - rest.opacity).abs() > 0.001)
        {
            changing.push(format!("{} differs from its rest", e.path));
        }
    }
    changing
}

/// List rows are as fast as they can be: hover is a fill that lands at once,
/// and a press selects the row and swaps the detail in the same frame.
#[test]
fn list_rows_select_and_hover_at_once() {
    for (scene, row, pane) in [
        ("history", "history-entry3", "history-pane"),
        ("modes", "mode-global", "/panel/"),
    ] {
        let mut rt = runtime(scene, true, PANE_SIZE);
        settle(&mut rt);
        let (x, y) = rt.find(row).expect("a row");
        rt.pointer_move(x, y);
        let hovered = still_changing(&mut rt, pane);
        assert!(hovered.is_empty(), "{scene}: hover animates: {hovered:#?}");
        assert!(element(&rt, row).hover > 0.999, "{scene}: no hover fill");
        rt.pointer_down(x, y);
        let pressed = still_changing(&mut rt, pane);
        assert!(
            pressed.is_empty(),
            "{scene}: selection animates: {pressed:#?}"
        );
        rt.pointer_up(x, y);
        let released = still_changing(&mut rt, pane);
        assert!(
            released.is_empty(),
            "{scene}: release animates: {released:#?}"
        );
    }
    let mut rt = runtime("history", true, PANE_SIZE);
    let (x, y) = rt.find("history-entry3").expect("a row");
    rt.pointer_down(x, y);
    rt.frame(1.0 / 120.0);
    assert_eq!(rt.state.history.selected, Some(3), "selects on the press");
    rt.pointer_up(x, y);
}

/// A row's Copy copies that entry without selecting it.
#[test]
fn a_rows_copy_leaves_the_selection() {
    let mut rt = runtime("history", true, PANE_SIZE);
    let (x, y) = rt.find("history-entry1").expect("a row");
    rt.pointer_move(x, y);
    settle(&mut rt);
    let copy = rt
        .elements()
        .into_iter()
        .find(|e| e.path.contains("history-entry1/") && e.key.as_deref() == Some("row-copy"))
        .expect("the row's Copy");
    let (x, y) = (
        copy.rect[0] + copy.rect[2] / 2.0,
        copy.rect[1] + copy.rect[3] / 2.0,
    );
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(rt.state.history.copied, Some(1));
    assert_eq!(rt.state.history.selected, Some(2));
}

#[test]
fn history_copy_writes_the_clipboard() {
    let mut rt = runtime("history", true, PANE_SIZE);
    tap(&mut rt, "history-copy");
    assert_eq!(
        rt.copy_request.as_deref(),
        Some("The deploy failed because the cache key changed.")
    );
    assert_eq!(rt.state.history.copied, Some(2));
}

#[test]
fn history_search_without_matches_says_so() {
    use crate::history::{History, HistoryDraft, HistoryKind, HistoryRetention, HistoryStore};
    let directory = std::env::temp_dir().join(format!("hex-rune-history-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let store = HistoryStore::open(
        directory.join("history.json"),
        HistoryRetention::Week,
        crate::history::now_ms(),
    );
    let history = History::new(store);
    history
        .record(HistoryDraft {
            kind: HistoryKind::Dictation,
            raw_text: "ship it".into(),
            final_text: "Ship it.".into(),
            application: None,
            processing: None,
            audio_ms: 900,
            inference_ms: 80,
            total_ms: 200,
        })
        .unwrap();
    let mut rt = runtime("history-empty", true, PANE_SIZE);
    rt.state.history = history::HistoryPane::shared(Some(history));
    rt.state.history.reload();
    rt.invalidate();
    settle(&mut rt);
    assert!(texts(&rt).iter().all(|text| text != "No dictations match."));
    tap(&mut rt, "history-search");
    rt.paste("zebra");
    settle(&mut rt);
    assert!(rt.state.history.entries.is_empty());
    let texts = texts(&rt);
    assert!(texts.iter().any(|text| text == "No dictations match."));
    assert!(
        texts
            .iter()
            .all(|text| text != "No dictations retained yet.")
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn modes_add_select_and_delete() {
    let mut rt = runtime("modes", true, PANE_SIZE);
    tap(&mut rt, "add-mode");
    assert_eq!(rt.state.settings.dictation_processing.modes.len(), 3);
    assert_eq!(rt.state.modes.selected, modes::Selection::Custom(2));
    assert_eq!(
        rt.state.settings.dictation_processing.modes[2].name,
        "Mode 3"
    );
    tap(&mut rt, "delete-mode");
    assert!(rt.state.modes.delete_armed);
    tap(&mut rt, "confirm-delete-mode");
    assert_eq!(rt.state.settings.dictation_processing.modes.len(), 2);
    assert_eq!(rt.state.modes.selected, modes::Selection::Default);
}

#[test]
fn mode_corrections_and_names_save_from_their_fields() {
    let mut rt = runtime("modes", true, PANE_SIZE);
    tap(&mut rt, "add-correction");
    assert_eq!(
        rt.state.settings.dictation_processing.modes[0]
            .replacements
            .len(),
        3
    );
    rt.paste("dax");
    settle(&mut rt);
    flush(&mut rt);
    assert_eq!(
        rt.state.settings.dictation_processing.modes[0].replacements[2].matched_phrase,
        "dax"
    );
    tap(&mut rt, "correction0/remove");
    let phrases: Vec<String> = rt.state.settings.dictation_processing.modes[0]
        .replacements
        .iter()
        .map(|r| r.matched_phrase.clone())
        .collect();
    assert_eq!(phrases, ["hex app", "dax"]);
}

#[test]
fn mode_transformations_add_reorder_and_remove() {
    let mut rt = runtime("modes", true, GOLDEN_SIZE);
    // The + opens a menu of only the transformations the mode lacks.
    assert!(rt.find("transformation-option0").is_none());
    tap(&mut rt, "add-transformation");
    assert!(rt.find("transformation-option0").is_some());
    assert!(rt.find("transformation-option1").is_none());
    tap(&mut rt, "transformation-option0");
    assert!(rt.find("transformation-option0").is_none());
    let ids = |rt: &Runtime<Ui>| {
        rt.state.settings.dictation_processing.modes[0]
            .transformations
            .clone()
    };
    assert_eq!(ids(&rt), ["smart-punctuation", "normalize-links"]);
    let saves = saved(&rt);
    // Drag the second row's handle up one row: the rows reorder while the
    // pointer moves and the order is saved once, on release.
    let (x, y) = rt.find("transformation1/handle").expect("a drag handle");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    for step in 1..=8 {
        rt.pointer_move(x, y - step as f32 * 5.0);
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(ids(&rt), ["smart-punctuation", "normalize-links"]);
    let shown: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.ends_with("/name") && e.path.contains("/transformation"))
        .filter_map(|e| e.text)
        .collect();
    assert_eq!(shown, ["Normalize links", "Smart punctuation"]);
    rt.pointer_up(x, y - 40.0);
    rt.frame(1.0 / 60.0);
    assert_eq!(ids(&rt), ["normalize-links", "smart-punctuation"]);
    assert_eq!(saved(&rt), saves + 1);
    tap(&mut rt, "transformation0/remove");
    assert_eq!(ids(&rt), ["smart-punctuation"]);
}

#[test]
fn the_transformation_menu_offers_what_the_mode_lacks_and_escape_closes_it() {
    let mut rt = runtime("modes-transformation-menu", true, GOLDEN_SIZE);
    let offered: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.contains("transformation-option") && e.path.ends_with("/name"))
        .filter_map(|e| e.text)
        .collect();
    assert_eq!(offered, ["Normalize links"]);
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(rt.state.open.is_none());
    assert!(rt.find("transformation-option0").is_none());
}

#[test]
fn escape_cancels_a_transformation_reorder_without_saving() {
    let mut rt = runtime("modes", true, GOLDEN_SIZE);
    tap(&mut rt, "add-transformation");
    tap(&mut rt, "transformation-option0");
    let order = |rt: &Runtime<Ui>| {
        rt.state.settings.dictation_processing.modes[0]
            .transformations
            .clone()
    };
    let before = order(&rt);
    let saves = saved(&rt);
    let (x, y) = rt.find("transformation1/handle").expect("a drag handle");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    for step in 1..=8 {
        rt.pointer_move(x, y - step as f32 * 5.0);
        rt.frame(1.0 / 60.0);
    }
    key(&mut rt, Key::Escape, Mods::default());
    rt.pointer_up(x, y - 40.0);
    rt.frame(1.0 / 60.0);
    assert_eq!(order(&rt), before);
    assert_eq!(saved(&rt), saves);
}

#[test]
fn escape_restores_a_slider_dragged_from_its_press() {
    let mut rt = runtime("mac-voice-filter", true, PANE_SIZE);
    let strength = |rt: &Runtime<Ui>| rt.state.platform.tuning.strength;
    let before = strength(&rt);
    let saves = saved(&rt);
    let (x, y) = rt.find("fun-slider-strength/track").expect("a slider");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_move(x + 60.0, y);
    rt.frame(1.0 / 60.0);
    assert_ne!(strength(&rt), before);
    key(&mut rt, Key::Escape, Mods::default());
    rt.frame(1.0 / 60.0);
    assert_eq!(strength(&rt), before);
    rt.pointer_up(x + 60.0, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(strength(&rt), before);
    assert_eq!(saved(&rt), saves);
}

#[test]
fn a_chosen_file_joins_the_catalog_and_the_mode() {
    let mut rt = runtime("modes-picker", true, GOLDEN_SIZE);
    let application = crate::application_catalog::InstalledApplication {
        name: "helix".into(),
        bundle_id: None,
        path: "/usr/bin/helix".into(),
    };
    modes::chosen_application(&mut rt.state, Ok(Some(application)));
    assert!(
        rt.state.settings.dictation_processing.modes[0]
            .applications
            .contains(&"helix".to_string())
    );
    modes::chosen_application(&mut rt.state, Ok(None));
    assert!(rt.state.modes.application_error.is_none());
    modes::chosen_application(&mut rt.state, Err("no file chooser portal".into()));
    assert_eq!(
        rt.state.modes.application_error.as_deref(),
        Some("no file chooser portal")
    );
}

#[test]
fn mode_applications_refuse_one_another_modes_use() {
    // In Chat, WezTerm is offered but belongs to Terminal.
    let mut rt = runtime("modes-picker", true, GOLDEN_SIZE);
    let chat = |rt: &Runtime<Ui>| {
        rt.state.settings.dictation_processing.modes[0]
            .applications
            .clone()
    };
    tap(&mut rt, "available1");
    assert!(rt.state.modes.application_error.is_some());
    assert!(!chat(&rt).contains(&"WezTerm".to_string()));
    tap(&mut rt, "available2");
    assert!(chat(&rt).contains(&"Zed".to_string()));
}

#[test]
fn the_transcription_picker_asks_the_service_for_a_model() {
    let mut rt = runtime("picker", true, PANE_SIZE);
    tap(&mut rt, "model2/top/actions/choose-model");
    assert!(matches!(
        sent(&rt),
        [Request::ChooseModel {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            ..
        }]
    ));
    assert_eq!(
        rt.state.picker.fallback, None,
        "Whisper keeps the saved English"
    );
    tap(&mut rt, "close-picker");
    assert!(!rt.state.picker.open);
}

/// The card index of `model` in the picker over `language`.
fn picker_card(language: &str, model: TranscriptionModelId) -> String {
    let index = crate::transcription::models::picker_models(language)
        .iter()
        .position(|(candidate, _)| candidate.id == model)
        .expect("every available model is listed");
    format!("model{index}")
}

#[test]
fn a_model_without_the_saved_language_falls_back_to_its_own_visibly() {
    let mut rt = runtime("picker", true, PANE_SIZE);
    rt.state.settings.transcription = TranscriptionSelection {
        model: TranscriptionModelId::WhisperLargeV3Turbo,
        language: "he".into(),
    };
    rt.state.picker.installed = Some(Vec::new());
    rt.invalidate();
    settle(&mut rt);
    let card = picker_card("he", TranscriptionModelId::ParakeetUnifiedEnglish);
    let shown = texts(&rt);
    assert!(
        shown.contains(&"Doesn't support Hebrew; switches the language to English".to_owned()),
        "{shown:?}"
    );
    tap(&mut rt, &format!("{card}/top/actions/choose-model"));
    assert_eq!(
        sent(&rt),
        [Request::ChooseModel {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        }]
    );
    // The service saves the pair; Settings names the change and fixes the
    // language, since the model has only English.
    rt.state.settings.transcription = TranscriptionSelection {
        model: TranscriptionModelId::ParakeetUnifiedEnglish,
        language: "en".into(),
    };
    rt.invalidate();
    tap(&mut rt, "close-picker");
    let shown = texts(&rt);
    assert!(
        shown.contains(
            &"Parakeet Unified English doesn't support Hebrew, so the language changed to English"
                .to_owned()
        ),
        "{shown:?}"
    );
    assert!(rt.find("transcription-language-control-item0").is_none());
    tap(&mut rt, "transcription-language-control");
    assert_eq!(rt.state.open, None, "a single language opens no menu");
}

#[test]
fn single_language_models_hide_the_language_row_without_a_fallback() {
    let single = runtime("mac-settings", true, PANE_SIZE);
    assert!(single.find("transcription-language").is_none());
    let multi = runtime("language-menu", true, PANE_SIZE);
    assert!(multi.find("transcription-language").is_some());
    let fallback = runtime("language-fallback", true, PANE_SIZE);
    assert!(fallback.find("transcription-language").is_some());
}

#[test]
fn browsing_the_picker_never_changes_the_language() {
    let mut rt = runtime("picker", true, PANE_SIZE);
    let before = rt.state.settings.transcription.clone();
    for _ in 0..12 {
        key(&mut rt, Key::Tab, Mods::default());
    }
    rt.pointer_move(400.0, 300.0);
    settle(&mut rt);
    assert_eq!(rt.state.settings.transcription, before);
    assert!(sent(&rt).is_empty());
}

#[test]
fn changing_only_the_language_prepares_the_same_model_again() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    rt.state.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
    rt.state.picker.fallback = Some(picker::LanguageFallback {
        model: TranscriptionModelId::WhisperLargeV3Turbo,
        from: "xx".into(),
    });
    rt.invalidate();
    settle(&mut rt);
    tap(&mut rt, "transcription-language-control-trigger");
    assert_eq!(rt.state.open, Some(Menu::Pick("transcription-language")));
    // Whisper lists every language in the catalog's order.
    let japanese =
        crate::transcription::models::definition(TranscriptionModelId::WhisperLargeV3Turbo)
            .language_choices()
            .iter()
            .position(|code| *code == "ja")
            .unwrap();
    tap_option(&mut rt, japanese);
    assert_eq!(rt.state.open, None);
    assert_eq!(
        sent(&rt),
        [Request::ChooseModel {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "ja".into(),
        }]
    );
    assert_eq!(
        rt.state.picker.fallback, None,
        "an explicit choice clears the note"
    );
}

/// General's Vocabulary row opens the sheet, and Escape closes it.
#[test]
fn the_vocabulary_row_opens_its_sheet() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "vocabulary-control");
    settle(&mut rt);
    assert!(rt.state.vocabulary.open);
    assert!(rt.find("add-term").is_some());
    assert!(rt.find("vocabulary-boost").is_none());
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(!rt.state.vocabulary.open);
}

/// Add term on a list longer than the sheet scrolls the new row into view.
#[test]
fn add_term_scrolls_the_new_row_into_view() {
    let mut rt = runtime("mac-settings-vocabulary", true, GOLDEN_SIZE);
    rt.state.settings.vocabulary = (0..14)
        .map(|index| crate::dictation::vocabulary::VocabularyEntry {
            term: format!("term {index}"),
            sounds_like: Vec::new(),
        })
        .collect();
    rt.state.vocabulary = super::vocabulary::VocabularyEditor::new(&rt.state.settings.vocabulary);
    rt.state.vocabulary.open = true;
    rt.invalidate();
    settle(&mut rt);
    tap(&mut rt, "add-term");
    settle(&mut rt);
    let id = rt.state.vocabulary.last_row().unwrap();
    let new_row = element(&rt, &format!("term{id}")).rect;
    let list = element(&rt, "term-scroll").rect;
    assert!(
        new_row[1] >= list[1] && new_row[1] + new_row[3] <= list[1] + list[3] + 0.5,
        "row {new_row:?} outside list {list:?}"
    );
}

/// A model whose transcription takes no vocabulary says so on the Vocabulary
/// row and in its sheet; Parakeet-based and prompt-reading models show none.
#[test]
fn the_vocabulary_row_says_when_the_model_ignores_the_list() {
    let note = "Cohere Transcribe doesn't use these terms; rewriting and search still do";
    let shows_note = |rt: &Runtime<Ui>| texts(rt).iter().any(|text| text == note);
    let sheet_note = |rt: &Runtime<Ui>| {
        rt.elements()
            .into_iter()
            .any(|e| !e.exiting && e.key.as_deref() == Some("unused"))
    };
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    assert!(!shows_note(&rt));
    rt.state.settings.transcription.model = TranscriptionModelId::CohereTranscribe;
    rt.invalidate();
    settle(&mut rt);
    assert!(shows_note(&rt), "{:?}", texts(&rt));
    tap(&mut rt, "vocabulary-control");
    assert!(rt.state.vocabulary.open);
    assert!(sheet_note(&rt));
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    for model in [
        TranscriptionModelId::UnderdogBark3Pro,
        TranscriptionModelId::WhisperLargeV3Turbo,
        TranscriptionModelId::QwenCleoAsr17B,
    ] {
        rt.state.settings.transcription.model = model;
        rt.invalidate();
        settle(&mut rt);
        assert!(
            !texts(&rt)
                .iter()
                .any(|text| text.contains("doesn't use these terms"))
        );
    }

    let rt = runtime("vocabulary-unused", true, PANE_SIZE);
    assert!(rt.state.vocabulary.open);
    assert!(sheet_note(&rt));
}

/// The Sounds row opens the sheet. A set's play button plays its dictation
/// without choosing it, and pressing it again stops it; a chip plays one
/// sound; picking a set saves it and plays it; Escape closes the sheet.
#[test]
fn the_sound_sets_sheet_plays_and_chooses_sets() {
    use crate::feedback::{Audition, Cue, SoundSet};
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    assert_eq!(rt.state.settings.sound_theme, SoundSet::Classic);
    open_advanced(&mut rt, Pane::General);
    tap(&mut rt, "sound-set-control");
    assert!(rt.state.sound_sets.open);
    let playing = |rt: &Runtime<Ui>| rt.state.sound_sets.playing();

    tap(&mut rt, "set-click/top/play");
    assert_eq!(playing(&rt), Some((SoundSet::Click, Audition::Dictation)));
    assert_eq!(rt.state.settings.sound_theme, SoundSet::Classic);
    tap(&mut rt, "set-click/top/play");
    assert_eq!(playing(&rt), None);

    // A single sound plays for under a second, so look before settling.
    let (x, y) = rt.find("stage/cues/Stop").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        playing(&rt),
        Some((SoundSet::Classic, Audition::Cue(Cue::Stop)))
    );

    tap(&mut rt, "set-wood");
    assert_eq!(rt.state.settings.sound_theme, SoundSet::Wood);
    assert_eq!(playing(&rt), Some((SoundSet::Wood, Audition::Dictation)));
    // The stage holds the HUD's window as the start sound plays.
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    let (window, _) = rt.presented("sound-sets-hud").expect("the HUD's window");
    assert!(window[2] > 0.0 && window[3] > 0.0);

    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(!rt.state.sound_sets.open);
    assert_eq!(playing(&rt), None);
    assert!(texts(&rt).iter().any(|text| text == "Wood"));
}

#[test]
fn a_download_in_progress_can_be_cancelled() {
    let mut rt = runtime("picker-downloading", true, PANE_SIZE);
    tap(&mut rt, "model1/top/actions/cancel-model");
    assert!(matches!(sent(&rt), [Request::CancelModel]));
}

fn focused(rt: &Runtime<Ui>) -> String {
    rt.elements()
        .into_iter()
        .find(|e| e.focused)
        .map(|e| e.path)
        .unwrap_or_default()
}

fn tab_to(rt: &mut Runtime<Ui>, target: &str) {
    let mut seen = Vec::new();
    for _ in 0..60 {
        if focused(rt).ends_with(target) {
            return;
        }
        seen.push(focused(rt));
        key(rt, Key::Tab, Mods::default());
    }
    panic!("Tab never reached {target}: {seen:?}");
}

#[test]
fn the_sidebar_moves_with_arrows_and_command_numbers() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    key(&mut rt, Key::Tab, Mods::default());
    assert!(
        focused(&rt).ends_with("nav-general"),
        "Tab enters at the current pane"
    );
    key(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.pane, Pane::Output, "selection follows the arrows");
    let command = Mods {
        cmd: true,
        ..Mods::default()
    };
    let shown = labs::order(&rt.state);
    key(&mut rt, Key::Command('3'), command);
    assert_eq!(rt.state.pane, shown[2]);
    assert!(
        focused(&rt).ends_with(shown[2].key()),
        "focus follows the shortcut"
    );
    key(&mut rt, Key::Command('1'), command);
    assert_eq!(rt.state.pane, Pane::General);
    key(&mut rt, Key::Command('9'), command);
    assert_eq!(rt.state.pane, Pane::General, "no ninth pane");
    key(&mut rt, Key::Up, Mods::default());
    assert_eq!(rt.state.pane, Pane::General, "Up stops at the first pane");
    let main = labs::main_panes(&rt.state);
    let last = *main.last().unwrap();
    key(
        &mut rt,
        Key::Command(char::from(b'0' + main.len() as u8)),
        command,
    );
    key(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.pane, last, "Down stops at the last pane");
}

#[test]
fn toggle_rows_take_focus_and_flip_on_space() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    assert!(rt.state.settings.paste_with_shift);
    open_advanced(&mut rt, Pane::General);
    tab_to(&mut rt, "terminal-paste");
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(!rt.state.settings.paste_with_shift);
}

#[test]
fn the_picker_keeps_focus_inside_and_escape_returns_it() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tab_to(&mut rt, "transcription-control");
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    settle(&mut rt);
    assert!(rt.state.picker.open);
    for _ in 0..12 {
        key(&mut rt, Key::Tab, Mods::default());
        assert!(
            focused(&rt).contains("transcription-picker"),
            "focus left the picker: {}",
            focused(&rt)
        );
    }
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(!rt.state.picker.open);
    assert!(focused(&rt).ends_with("transcription-control"));
}

#[test]
fn history_rows_select_with_arrows() {
    let mut rt = runtime("history", true, PANE_SIZE);
    tap(&mut rt, "history-entry3");
    key(&mut rt, Key::Down, Mods::default());
    assert_ne!(rt.state.history.selected, Some(3));
    key(&mut rt, Key::Up, Mods::default());
    assert_eq!(rt.state.history.selected, Some(3));
}

#[test]
fn settings_opens_the_picker_and_names_the_model_and_language_apart() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    let shown = texts(&rt);
    assert!(
        shown.contains(&"Parakeet Unified English".to_owned()),
        "{shown:?}"
    );
    assert!(
        !shown.iter().any(|text| text.contains(" · Parakeet")),
        "{shown:?}"
    );
    tap(&mut rt, "transcription-control");
    assert!(rt.state.picker.open);
}

#[test]
fn onboarding_downloads_the_model_through_the_service() {
    let mut rt = runtime("onboarding", true, PANE_SIZE);
    // Setup starts the download as soon as it opens.
    tap(&mut rt, "tour-continue");
    assert!(matches!(
        sent(&rt),
        [Request::ChooseModel {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            ..
        }]
    ));
    // The service finishes: the model is installed and setup moves on.
    rt.state.picker.installed = Some(vec![TranscriptionModelId::ParakeetUnifiedEnglish]);
    settle(&mut rt);
    std::thread::sleep(std::time::Duration::from_millis(750));
    settle(&mut rt);
    // Setup moves on to the practice dictation, which can be skipped.
    tap(&mut rt, "tour-skip");
    tap(&mut rt, "tour-continue");
    assert!(rt.state.tour.is_none(), "Start using Hex closes setup");
}

#[test]
fn with_no_model_found_onboarding_downloads_the_recommended_model_with_no_list() {
    for scene in ["onboarding", "mac-onboarding"] {
        let mut rt = runtime(scene, false, PANE_SIZE);
        assert!(rt.state.found.models.is_empty());
        // The download starts on its own, before any step or pick.
        assert!(
            matches!(
                sent(&rt),
                [Request::ChooseModel {
                    model: TranscriptionModelId::ParakeetUnifiedEnglish,
                    ..
                }]
            ),
            "{scene}: {:?}",
            sent(&rt)
        );
        tap(&mut rt, "tour-continue");
        let shown = texts(&rt);
        assert!(rt.find("model-list").is_none(), "{scene}: no model list");
        assert!(
            !shown.contains(&"Choose a speech model".to_owned()),
            "{scene}: {shown:?}"
        );
    }
}

#[test]
fn the_onboarding_model_list_starts_the_row_picked_or_the_recommended_one() {
    // Continue with no pick starts the selected, recommended model.
    let mut rt = runtime("onboarding-model-found", false, PANE_SIZE);
    assert!(sent(&rt).is_empty(), "nothing starts before a pick");
    tap(&mut rt, "tour-continue");
    assert!(matches!(
        sent(&rt),
        [Request::ChooseModel {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            ..
        }]
    ));
    // Picking a found catalog model uses it, with no download of ours.
    let mut rt = runtime("onboarding-model-found", false, PANE_SIZE);
    tap(&mut rt, "model1");
    assert!(matches!(
        sent(&rt),
        [Request::ChooseModel {
            model: TranscriptionModelId::ParakeetV3,
            ..
        }]
    ));
    assert_eq!(rt.state.found.picked, Some(1));
}

#[test]
fn the_microphone_field_names_the_input_in_use() {
    let field = |rt: &Runtime<Ui>| element(rt, "microphone-control").label;
    let rt = runtime("mac-settings", true, PANE_SIZE);
    assert_eq!(field(&rt).as_deref(), Some("System default, Kit Pods"));

    let rt = runtime("mac-microphone-missing", true, PANE_SIZE);
    assert_eq!(field(&rt).as_deref(), Some("System default, Kit Pods"));
    let texts = texts(&rt);
    assert!(texts.contains(&"not connected".to_owned()), "{texts:?}");
    assert!(texts.contains(&"Shure MV7 isn't connected; using your Mac's input".to_owned()));
}

#[test]
fn a_closed_lid_names_the_input_standing_in_for_the_built_in_microphone() {
    let note = "MacBook Pro Microphone is off while the lid is closed, so Hex listens to Studio Display Microphone";
    let field = |rt: &Runtime<Ui>| element(rt, "microphone-control").label;
    let mut rt = runtime("mac-microphone-lid-closed", true, PANE_SIZE);
    assert_eq!(
        field(&rt).as_deref(),
        Some("System default, Studio Display Microphone")
    );
    let shown = texts(&rt);
    assert!(shown.contains(&note.to_owned()), "{shown:?}");
    assert!(shown.contains(&"off while the lid is closed".to_owned()));

    // A saved built-in microphone shows the default standing in, too.
    rt.state.settings.microphone = Some("MacBook Pro Microphone".into());
    if let Ok(devices) = &mut rt.state.microphones {
        devices.default_is_off_with_lid_closed = false;
    }
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(
        field(&rt).as_deref(),
        Some("System default, Studio Display Microphone")
    );
    assert!(texts(&rt).contains(&note.to_owned()));
}

#[test]
fn a_passed_over_bluetooth_default_names_the_input_in_use() {
    let note = "Kit Pods is Bluetooth, so Hex listens to MacBook Pro Microphone";
    let mut rt = runtime("mac-microphone-bluetooth", true, TALL_SIZE);
    assert_eq!(
        element(&rt, "microphone-control").label.as_deref(),
        Some("System default, MacBook Pro Microphone")
    );
    assert!(texts(&rt).contains(&note.to_owned()), "{:?}", texts(&rt));

    tap(&mut rt, "avoid-bluetooth-microphone");
    assert!(!rt.state.settings.avoid_bluetooth_microphone);
    // A saved Bluetooth microphone is used as named, without the note.
    rt.state.settings.avoid_bluetooth_microphone = true;
    rt.state.settings.microphone = Some("Kit Pods".into());
    rt.invalidate();
    settle(&mut rt);
    assert!(!texts(&rt).contains(&note.to_owned()));
}

/// `HEX_ANNOTATE_SNAPS=<folder>` writes the Annotate pane, light and dark,
/// tall enough to show How it works.
#[cfg(hex_capture)]
#[test]
fn annotate_pane_snapshots() {
    let Some(folder) = std::env::var_os("HEX_ANNOTATE_SNAPS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    for dark in [false, true] {
        let Some((width, height, pixels)) = render("mac-scrawl", dark, (PANE_SIZE.0, 1500.0))
        else {
            eprintln!("skipping Annotate snapshots: no GPU adapter");
            return;
        };
        let theme = if dark { "dark" } else { "light" };
        let path = folder.join(format!("annotate-pane-{theme}.png"));
        rune::render::write_png(&path, width, height, &pixels).unwrap();
    }
}

// ---------------------------------------------------------------------------
// Goldens: offscreen GPU renders compared with checked-in PNGs. Set
// HEX_UPDATE_GOLDENS=1 to rewrite them. Without a GPU adapter they skip,
// unless HEX_REQUIRE_GPU is set (scripts/check-onboarding.sh).
// HEX_GOLDEN_RENDERS=<folder> also writes every render there.

/// One device for every golden, used one render at a time: creating Vulkan
/// devices from parallel test threads crashed Mesa's RADV on Linux.
fn gpu() -> Option<std::sync::MutexGuard<'static, Gpu>> {
    static GPU: std::sync::OnceLock<Option<std::sync::Mutex<Gpu>>> = std::sync::OnceLock::new();
    GPU.get_or_init(|| {
        Gpu::try_new(Gpu::instance(), None)
            .ok()
            .map(std::sync::Mutex::new)
    })
    .as_ref()
    .map(|gpu| {
        gpu.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    })
}

fn render(scene: &str, dark: bool, size: (f32, f32)) -> Option<(u32, u32, Vec<u8>)> {
    let gpu = gpu()?;
    let mut rt = runtime(scene, dark, size);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let (width, height) = (
        (size.0 * GOLDEN_SCALE) as u32,
        (size.1 * GOLDEN_SCALE) as u32,
    );
    let target = crate::rune_ui::offscreen::target(&gpu.device, width, height);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (width, height),
    );
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    // The window's own compositor: the UI, the welcome hero over its stage,
    // and the Sounds sheet's HUD.
    rune::compose::Compositor::render(
        &mut super::WindowCompositor::default(),
        &mut rune::compose::Frame {
            device: &gpu.device,
            queue: &gpu.queue,
            encoder: &mut encoder,
            target: &target.view,
            format: crate::rune_ui::offscreen::format(),
            size: (width, height),
            dt: 0.0,
            update_ms: 0.0,
            rt: &mut rt,
            ui: &mut renderer,
        },
    );
    target.copy(&mut encoder);
    renderer.submit(&gpu.queue, encoder);
    Some((width, height, target.read(&gpu.device)))
}

fn golden_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("rune-{name}.png"))
}

fn read_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

fn check_golden(scene: &str, dark: bool, size: (f32, f32)) {
    let name = format!("{scene}-{}", if dark { "dark" } else { "light" });
    let Some((width, height, pixels)) = render(scene, dark, size) else {
        assert!(
            std::env::var_os("HEX_REQUIRE_GPU").is_none(),
            "golden {name}: no GPU adapter, and HEX_REQUIRE_GPU is set"
        );
        eprintln!("skipping golden {name}: no GPU adapter");
        return;
    };
    if let Some(folder) = std::env::var_os("HEX_GOLDEN_RENDERS") {
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        rune::render::write_png(&folder.join(format!("{name}.png")), width, height, &pixels)
            .unwrap();
    }
    let path = golden_path(&name);
    if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        rune::render::write_png(&path, width, height, &pixels).unwrap();
        return;
    }
    if !path.exists() {
        let actual = std::env::temp_dir().join(format!("rune-settings-{name}-actual.png"));
        rune::render::write_png(&actual, width, height, &pixels).unwrap();
        let miss = format!(
            "{} is missing; run with HEX_UPDATE_GOLDENS=1 (render at {})",
            path.display(),
            actual.display()
        );
        if !GOLDEN_MISSES.with(|misses| {
            misses
                .borrow_mut()
                .as_mut()
                .map(|misses| misses.push(miss.clone()))
                .is_some()
        }) {
            panic!("{miss}");
        }
        return;
    }
    let (golden_width, golden_height, golden) = read_png(&path);
    assert_eq!(
        (golden_width, golden_height),
        (width, height),
        "{name} size changed"
    );
    // GPU drivers differ in the last bit of blending and glyph coverage;
    // a layout or colour change moves far more than this.
    let differing = pixels
        .chunks(4)
        .zip(golden.chunks(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 12))
        .count();
    let limit = (width * height) as usize / 500;
    if differing > limit {
        let actual = std::env::temp_dir().join(format!("rune-settings-{name}-actual.png"));
        rune::render::write_png(&actual, width, height, &pixels).unwrap();
        let miss = format!(
            "{name}: {differing} pixels differ (limit {limit}); actual render at {}",
            actual.display()
        );
        if !GOLDEN_MISSES.with(|misses| {
            misses
                .borrow_mut()
                .as_mut()
                .map(|misses| misses.push(miss.clone()))
                .is_some()
        }) {
            panic!("{miss}");
        }
    }
}

thread_local! {
    static GOLDEN_MISSES: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Collects every golden mismatch in a test and fails once at the end, so
/// one CI run uploads every render that moved rather than only the first.
struct Goldens;

impl Goldens {
    fn collect() -> Self {
        GOLDEN_MISSES.with(|misses| *misses.borrow_mut() = Some(Vec::new()));
        Self
    }
}

impl Drop for Goldens {
    fn drop(&mut self) {
        let misses = GOLDEN_MISSES
            .with(|misses| misses.borrow_mut().take())
            .unwrap_or_default();
        if !misses.is_empty() && !std::thread::panicking() {
            panic!("{}", misses.join("\n"));
        }
    }
}

/// Every pane in both appearances; General at its full height.
#[test]
fn goldens() {
    let _goldens = Goldens::collect();
    for dark in [true, false] {
        check_golden("portal", dark, GOLDEN_SIZE);
        for scene in [
            "history",
            "mac-labs-link",
            "output",
            "modes",
            "modes-picker",
            "picker-downloading",
        ] {
            check_golden(scene, dark, PANE_SIZE);
        }
    }
    check_golden("commands", true, GOLDEN_SIZE);
    check_golden("menus", false, GOLDEN_SIZE);
    // A list longer than the window fades at its scrolling edge (#840).
    check_golden("language-menu", false, GOLDEN_SIZE);
    check_golden("modes-opencode-not-running", false, PANE_SIZE);
    check_golden("picker-custom-models", false, PANE_SIZE);
    check_golden("picker-found", false, PANE_SIZE);
}

mod onboarding_check;
mod panes;

/// Rune hands focus back to a menu's trigger when it closes. A key the
/// window means for something else must not reach that trigger.
#[test]
fn the_recorder_still_gets_space_and_enter_after_a_menu_closes() {
    for scene in ["menus", "language-menu"] {
        for recorded in [Key::Text(" ".into()), Key::Enter] {
            let mut rt = runtime(scene, true, GOLDEN_SIZE);
            assert!(rt.state.open.is_some(), "{scene} opens a menu");
            key(&mut rt, Key::Escape, Mods::default());
            settle(&mut rt);
            assert!(rt.state.open.is_none(), "{scene}: Escape closes the menu");
            tap(&mut rt, "dictation-hotkey");
            assert!(matches!(
                rt.state.capture,
                settings::Capture::Listening { .. }
            ));
            key(&mut rt, recorded.clone(), Mods::default());
            settle(&mut rt);
            assert!(
                rt.state.open.is_none(),
                "{scene}: {recorded:?} reopened a menu while recording"
            );
        }
    }
}
