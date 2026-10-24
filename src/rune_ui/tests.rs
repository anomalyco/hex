use super::settings::{desktop_shortcut, recorded_binding, validate};
use super::*;
use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
use rune::text::TextSystem;
use rune::{Env, Key, KeyEvent, Mods, Runtime};

/// Tall enough to show every Settings section without scrolling.
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
    let env = Env {
        system_dark: dark,
        scale: GOLDEN_SCALE,
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = Runtime::with_text(state, view, size, env, text);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
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

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements().into_iter().filter_map(|e| e.text).collect()
}

/// Presses an element by key and lets every animation settle.
fn tap(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn key(rt: &mut Runtime<Ui>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    rt.frame(1.0 / 60.0);
}

#[test]
fn portal_desktops_show_what_they_granted_beside_each_recorder() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    let shown = texts(&rt);
    for expected in [
        "Shortcuts",
        "Your desktop granted F12",
        "Your desktop granted Ctrl+Alt+V",
        "Change shortcut",
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
fn a_refused_portal_shortcut_says_how_to_free_it() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    if let Some(view) = &mut rt.state.view {
        view.listener.shortcuts = DesktopShortcuts::Portal(vec![DesktopGrantedShortcut {
            name: "Dictation".into(),
            trigger: None,
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
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    assert!(rt.state.settings.paste_with_shift);
    tap(&mut rt, "terminal-paste");
    assert!(!rt.state.settings.paste_with_shift);
    assert_eq!(saved(&rt), 1);
    assert!(sent(&rt).is_empty());
}

#[test]
fn quit_hex_in_the_sidebar_asks_the_host_to_quit() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    tap(&mut rt, "quit");
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
fn releasing_the_microphone_asks_first() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "microphone-mode-control/segment1");
    assert!(rt.state.confirm_release);
    assert!(!rt.state.settings.release_microphone_while_idle);
    tap(&mut rt, "confirm-release/actions/confirm-release");
    assert!(rt.state.settings.release_microphone_while_idle);
    assert!(!rt.state.confirm_release);
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

#[test]
fn the_sidebar_switches_panes_and_keeps_settings() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    for (key, pane) in [
        ("nav-modes", Pane::Modes),
        ("nav-voice-action", Pane::VoiceAction),
        ("nav-history", Pane::History),
        ("nav-settings", Pane::Settings),
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
    let (x, y) = rt.find("nav-modes").expect("the Modes item");
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.pane, Pane::Modes);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.pane, Pane::Modes);
}

#[test]
fn sidebar_rows_have_no_hover_or_press_tint() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    let tinted = |rt: &Runtime<Ui>| {
        rt.elements()
            .into_iter()
            .filter(|e| e.path.contains("nav-modes"))
            .any(|e| e.hover > 0.0 || e.press > 0.0)
    };
    let (x, y) = rt.find("nav-modes").expect("the Modes item");
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert!(!tinted(&rt), "hovering a sidebar row paints nothing");
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    assert!(!tinted(&rt), "pressing a sidebar row only selects it");
    rt.pointer_up(x, y);
}

#[test]
fn switching_panes_is_instant() {
    let mut rt = runtime("portal", true, PANE_SIZE);
    let (x, y) = rt.find("nav-modes").expect("the Modes item");
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
        !panel
            .iter()
            .any(|e| e.key.as_deref() == Some("terminal-paste")),
        "Settings left at once"
    );
    for element in panel {
        assert!(
            element.opacity >= 0.999 && !element.moving,
            "{} is still animating",
            element.path
        );
    }
}

#[test]
fn voice_action_saves_its_opt_in_and_refuses_a_taken_shortcut() {
    let mut rt = runtime("voice-action-off", true, PANE_SIZE);
    tap(&mut rt, "voice-action-enabled");
    assert!(rt.state.settings.voice_action.enabled);
    assert_eq!(saved(&rt), 1);
    assert!(rt.find("opencode-model").is_some(), "the model row appears");

    let mut rt = runtime("voice-action-off", true, PANE_SIZE);
    rt.state.settings.edit_hotkey = rt.state.settings.paste_last_hotkey.clone().unwrap();
    tap(&mut rt, "voice-action-enabled");
    assert!(!rt.state.settings.voice_action.enabled);
    assert!(rt.state.voice_action_error.is_some());
    assert_eq!(saved(&rt), 0);
}

#[test]
fn choosing_an_opencode_model_saves_it() {
    let mut rt = runtime("voice-action-models", true, PANE_SIZE);
    tap(&mut rt, "model1");
    assert_eq!(
        rt.state.settings.voice_action.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
    assert_eq!(rt.state.open, None);
    tap(&mut rt, "model-value");
    assert_eq!(rt.state.open, Some(Menu::Model));
    tap(&mut rt, "model0");
    assert_eq!(
        rt.state.settings.voice_action.model, None,
        "back to the OpenCode default"
    );
}

#[test]
fn arrow_keys_move_the_search_highlight_and_enter_picks_it() {
    let mut rt = runtime("voice-action-models", true, PANE_SIZE);
    tap(&mut rt, "model-search");
    key(&mut rt, Key::Down, Mods::default());
    key(&mut rt, Key::Down, Mods::default());
    key(&mut rt, Key::Up, Mods::default());
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        rt.state.settings.voice_action.model.as_deref(),
        Some("anthropic/claude-sonnet-4-5")
    );
    assert_eq!(rt.state.open, None);

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
    let rt = runtime("voice-action-unavailable", true, PANE_SIZE);
    assert!(rt.find("opencode-unavailable").is_some());
    assert!(rt.find("open-opencode-setup").is_some());
    assert!(rt.find("opencode-model").is_none());
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
    let mut rt = runtime("modes-picker", true, GOLDEN_SIZE);
    tap(&mut rt, "available-transformation0/add");
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
    tap(&mut rt, "language4");
    assert_eq!(rt.state.picker.language.as_deref(), Some("ja"));
    tap(&mut rt, "close-picker");
    assert_eq!(rt.state.picker.language, None);
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
        focused(&rt).ends_with("nav-settings"),
        "Tab enters at the current pane"
    );
    key(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.pane, Pane::Modes, "selection follows the arrows");
    let command = Mods {
        cmd: true,
        ..Mods::default()
    };
    let shown: Vec<_> = Pane::ALL
        .into_iter()
        .filter(|pane| pane.shown(&rt.state))
        .collect();
    key(&mut rt, Key::Command('3'), command);
    assert_eq!(rt.state.pane, shown[2]);
    assert!(
        focused(&rt).ends_with(shown[2].key()),
        "focus follows the shortcut"
    );
    key(&mut rt, Key::Command('1'), command);
    assert_eq!(rt.state.pane, Pane::Settings);
    key(&mut rt, Key::Command('9'), command);
    assert_eq!(rt.state.pane, Pane::Settings, "no ninth pane");
    key(&mut rt, Key::Up, Mods::default());
    assert_eq!(rt.state.pane, Pane::Settings, "Up stops at the first pane");
    let last = *shown.last().unwrap();
    key(
        &mut rt,
        Key::Command(char::from(b'0' + shown.len() as u8)),
        command,
    );
    key(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.pane, last, "Down stops at the last pane");
}

#[test]
fn toggle_rows_take_focus_and_flip_on_space() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    assert!(rt.state.settings.paste_with_shift);
    tab_to(&mut rt, "terminal-paste");
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(!rt.state.settings.paste_with_shift);
}

#[test]
fn the_picker_keeps_focus_inside_and_escape_returns_it() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tab_to(&mut rt, "transcription-control-field-trigger");
    key(&mut rt, Key::Enter, Mods::default());
    settle(&mut rt);
    assert_eq!(rt.state.picker.language.as_deref(), Some("en"));
    for _ in 0..12 {
        key(&mut rt, Key::Tab, Mods::default());
        assert!(
            focused(&rt).contains("transcription-picker"),
            "focus left the picker: {}",
            focused(&rt)
        );
    }
    tab_to(&mut rt, "language1");
    key(&mut rt, Key::Down, Mods::default());
    assert_ne!(rt.state.picker.language.as_deref(), Some("en"));
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert_eq!(rt.state.picker.language, None);
    assert!(focused(&rt).ends_with("transcription-control-field-trigger"));
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
fn settings_opens_the_picker_on_the_current_language() {
    let mut rt = runtime("portal", true, GOLDEN_SIZE);
    tap(&mut rt, "transcription-control-trigger");
    assert_eq!(rt.state.picker.language.as_deref(), Some("en"));
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
    tap(&mut rt, "tour-continue");
    assert!(rt.state.tour.is_none(), "Start using Hex closes setup");
}

#[test]
fn recognition_hints_save_when_typing_pauses() {
    let mut ui = portal_fixture();
    ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
    ui.hints = TextEdit::new("OpenCode, Effect");
    let start = std::time::Instant::now();
    assert!(!ui.save_drafts(start), "waits for a pause");
    assert!(ui.save_drafts(start + DRAFT_DELAY));
    assert_eq!(
        ui.settings.transcription.recognition_hints,
        "OpenCode, Effect"
    );
}

// ---------------------------------------------------------------------------
// Goldens: offscreen GPU renders compared with checked-in PNGs. Set
// HEX_UPDATE_GOLDENS=1 to rewrite them. Without a GPU adapter they skip.

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
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    let (width, height) = (
        (size.0 * GOLDEN_SCALE) as u32,
        (size.1 * GOLDEN_SCALE) as u32,
    );
    let target = Offscreen::new(&gpu.device, width, height);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (width, height),
    );
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, rt.clear_color());
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
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
        eprintln!("skipping golden {name}: no GPU adapter");
        return;
    };
    let path = golden_path(&name);
    if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        rune::render::write_png(&path, width, height, &pixels).unwrap();
        return;
    }
    assert!(
        path.exists(),
        "{} is missing; run with HEX_UPDATE_GOLDENS=1",
        path.display()
    );
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

/// Every pane in both appearances; Settings at its full height.
#[test]
fn goldens() {
    let _goldens = Goldens::collect();
    for dark in [true, false] {
        check_golden("portal", dark, GOLDEN_SIZE);
        for scene in [
            "voice-action",
            "history",
            "modes",
            "modes-picker",
            "picker-downloading",
            "onboarding-setup",
        ] {
            check_golden(scene, dark, PANE_SIZE);
        }
    }
    check_golden("commands", true, GOLDEN_SIZE);
    check_golden("menus", false, GOLDEN_SIZE);
    check_golden("voice-action-models", true, PANE_SIZE);
    check_golden("voice-action-unavailable", false, PANE_SIZE);
    check_golden("onboarding", true, PANE_SIZE);
}

mod panes;
