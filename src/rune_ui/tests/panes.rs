//! The macOS and Windows surfaces, driven headlessly on Linux from preview
//! data: Commands, Meetings, the OS-only General rows, and the full tour.

use super::*;
use crate::login_item::LoginItemStatus;
use crate::onboarding::{Input, Observation, PermissionAction, PermissionKind, Practice, Step};
use platform::{CalendarRequest, MeetingRequest, PlatformHost, PlatformRequest};

fn asked(rt: &Runtime<Ui>) -> &[PlatformRequest] {
    match &rt.state.host {
        PlatformHost::Fixture(sent) => sent,
        _ => panic!("scenes record platform requests"),
    }
}

/// Redraws after a test changes state directly, as the host would.
fn refresh(rt: &mut Runtime<Ui>) {
    rt.invalidate();
    settle(rt);
}

#[test]
fn each_os_lists_only_the_panes_it_can_run() {
    let linux = runtime("portal", true, PANE_SIZE);
    assert!(linux.find("nav-commands").is_none());
    assert!(linux.find("nav-meetings").is_none());
    let mut mac = runtime("mac-settings", true, PANE_SIZE);
    for (key, pane) in [
        ("nav-commands", Pane::Commands),
        ("nav-meetings", Pane::Meetings),
        ("nav-general", Pane::General),
    ] {
        tap(&mut mac, key);
        assert_eq!(mac.state.pane, pane);
    }
    let windows = runtime("windows-settings", true, PANE_SIZE);
    assert!(windows.find("nav-commands").is_none());
    assert!(windows.find("nav-meetings").is_none());
}

#[test]
fn settings_rows_follow_each_os() {
    let present = |rt: &Runtime<Ui>, key: &str| rt.find(key).is_some();
    let linux = runtime("portal", true, GOLDEN_SIZE);
    assert!(present(&linux, "terminal-paste"));
    assert!(present(&linux, "toggle-listening"));
    assert!(present(&linux, "enable-on-launch"));
    assert!(!present(&linux, "launch-at-login"));
    assert!(!present(&linux, "dock-icon"));
    assert!(!present(&linux, "meetings-enabled"));

    let mac = runtime("mac-settings", true, PANE_SIZE);
    for key in ["launch-at-login", "dock-icon", "update-status"] {
        assert!(present(&mac, key), "macOS shows {key}");
    }
    // Features own their settings: none of these are in General.
    for key in [
        "terminal-paste",
        "toggle-listening",
        "enable-on-launch",
        "meetings-enabled",
        "detected-calls",
        "hex-mic-driver",
        "command-model-notice",
    ] {
        assert!(!present(&mac, key), "macOS General has no {key}");
    }

    let windows = runtime("windows-settings", true, GOLDEN_SIZE);
    assert!(present(&windows, "launch-at-login"));
    assert!(present(&windows, "toggle-listening"));
    assert!(!present(&windows, "dock-icon"));
    assert!(!present(&windows, "terminal-paste"));
    assert!(!present(&windows, "meetings-enabled"));
}

#[test]
fn launch_at_login_asks_the_login_item_and_needs_approval_first() {
    let mut rt = runtime("mac-settings", true, (980.0, 2400.0));
    tap(&mut rt, "launch-at-login");
    assert_eq!(asked(&rt), [PlatformRequest::SetLaunchAtLogin(true)]);
    assert_eq!(rt.state.platform.login_item, Some(LoginItemStatus::Enabled));

    let mut rt = runtime("mac-settings-permissions", true, (980.0, 2400.0));
    tap(&mut rt, "launch-at-login-approval");
    assert_eq!(asked(&rt), [PlatformRequest::OpenLoginItemSettings]);
}

/// The macOS login item switch's state before macOS has answered a click.
fn login_item(status: LoginItemStatus) -> platform::PlatformState {
    let mut state = platform::PlatformState::new(platform::Capabilities::MACOS);
    state.login_item = Some(status);
    state
}

fn login_failed(status: Option<LoginItemStatus>) -> crate::login_item::LoginItemResult {
    Err(crate::login_item::LoginItemFailure {
        status,
        message: "Operation not permitted".into(),
    })
}

#[test]
fn login_item_switch_moves_on_click_and_stays_when_macos_agrees() {
    let mut state = login_item(LoginItemStatus::Disabled);
    state.request_login_item(true);
    assert!(
        state.login_item_on(),
        "the thumb moves before macOS answers"
    );
    assert_eq!(state.login_item, Some(LoginItemStatus::Disabled));
    state.login_item_answered(Ok(LoginItemStatus::Enabled), true);
    assert!(state.login_item_on());
    assert_eq!(state.login_item_requested, None);
    assert_eq!(state.login_item, Some(LoginItemStatus::Enabled));
    assert_eq!(state.login_item_error, None);
}

#[test]
fn login_item_switch_springs_back_beside_the_error_when_macos_refuses() {
    let mut state = login_item(LoginItemStatus::Disabled);
    state.request_login_item(true);
    state.login_item_answered(login_failed(Some(LoginItemStatus::Disabled)), true);
    assert!(!state.login_item_on());
    assert_eq!(
        state.login_item_error.as_deref(),
        Some("Operation not permitted")
    );
    // A failure that could not read the status keeps the last known one.
    state.request_login_item(true);
    assert_eq!(state.login_item_error, None, "a new click clears the error");
    state.login_item_answered(login_failed(None), true);
    assert!(!state.login_item_on());
    assert!(state.login_item_error.is_some());
    // macOS answering with the other state, without an error, also springs back.
    state.request_login_item(true);
    state.login_item_answered(Ok(LoginItemStatus::Disabled), true);
    assert!(!state.login_item_on());
}

#[test]
fn login_item_status_answer_never_flips_a_pending_click() {
    let mut state = login_item(LoginItemStatus::Disabled);
    // The status check from opening Settings was still running at the click.
    state.request_login_item(true);
    state.login_item_answered(Ok(LoginItemStatus::Disabled), false);
    assert!(state.login_item_on());
    // Even a failed check behind the click leaves it, with no stale error.
    state.login_item_answered(login_failed(Some(LoginItemStatus::Disabled)), false);
    assert!(state.login_item_on());
    assert_eq!(state.login_item_error, None);
    state.login_item_answered(Ok(LoginItemStatus::Enabled), true);
    assert!(state.login_item_on());
    // Without a click, a status answer is simply adopted.
    state.login_item_answered(Ok(LoginItemStatus::Disabled), false);
    assert!(!state.login_item_on());
}

#[test]
fn login_item_double_click_keeps_the_last_click() {
    let mut state = login_item(LoginItemStatus::Disabled);
    state.request_login_item(true);
    state.request_login_item(false);
    assert!(!state.login_item_on());
    // The first request's answer arrives while the second still runs.
    state.login_item_answered(Ok(LoginItemStatus::Enabled), false);
    assert!(!state.login_item_on(), "the last click wins");
    assert_eq!(state.login_item, Some(LoginItemStatus::Enabled));
    state.login_item_answered(Ok(LoginItemStatus::Disabled), true);
    assert!(!state.login_item_on());
    assert_eq!(state.login_item_requested, None);
}

/// A native host whose login item answers only when the test says so.
struct SlowLoginItem(std::rc::Rc<std::cell::RefCell<Vec<PlatformRequest>>>);

impl platform::NativeHost for SlowLoginItem {
    fn send(&mut self, request: PlatformRequest) -> Result<(), String> {
        self.0.borrow_mut().push(request);
        Ok(())
    }
    fn poll(&mut self, _: &mut platform::PlatformState) -> bool {
        false
    }
    fn meetings(&mut self) -> Option<Result<Vec<meetings::Meeting>, String>> {
        None
    }
    fn tour_inputs(&mut self, _: bool) -> Vec<Input> {
        Vec::new()
    }
}

#[test]
fn login_item_row_moves_its_switch_before_macos_answers() {
    let mut rt = runtime("mac-settings", true, (980.0, 2400.0));
    let sent = std::rc::Rc::default();
    rt.state.host = PlatformHost::Native(Box::new(SlowLoginItem(std::rc::Rc::clone(&sent))));
    tap(&mut rt, "launch-at-login");
    tap(&mut rt, "launch-at-login");
    tap(&mut rt, "launch-at-login");
    assert_eq!(
        *sent.borrow(),
        [true, false, true].map(PlatformRequest::SetLaunchAtLogin)
    );
    assert!(rt.state.platform.login_item_on());
    assert_eq!(
        rt.state.platform.login_item,
        Some(LoginItemStatus::Disabled)
    );
}

#[test]
fn missing_permissions_lead_settings_and_open_the_guide() {
    let mut rt = runtime("mac-settings-permissions", true, (980.0, 2400.0));
    let shown = texts(&rt);
    assert!(shown.iter().any(|text| text == "Permissions needed"));
    assert!(
        rt.find("permission-microphone-allow-control/mark/dot")
            .is_none()
    );
    tap(&mut rt, "permission-microphone-allow");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Permission(PermissionAction::ShowGuide(
            PermissionKind::Microphone
        ))]
    );
    assert!(rt.state.platform.asked(PermissionKind::Microphone));
    assert!(
        rt.find("permission-microphone-allow-control/mark/dot")
            .is_some(),
        "asked permissions wait with a dot"
    );
}

/// The drawn rect and effective opacity of the element whose path ends
/// with `path`.
fn drawn(rt: &Runtime<Ui>, path: &str) -> Option<([f32; 4], f32)> {
    rt.elements()
        .into_iter()
        .find(|e| e.path.ends_with(path) && !e.exiting)
        .map(|e| (e.rect, e.opacity))
}

#[test]
fn a_grant_gathers_then_releases_into_allowed() {
    let mut rt = runtime("meetings-allow", true, PANE_SIZE);
    let pill = "meeting-system-audio-allow";
    let control = format!("{pill}-control");
    tap(&mut rt, pill);
    assert_eq!(asked(&rt), [PlatformRequest::SystemAudio]);
    assert!(
        rt.find(&format!("{control}/mark/dot")).is_some(),
        "waits with a dot"
    );
    let at_rest = drawn(&rt, &control).unwrap().0;

    // The fixture grants on the second ask.
    let (x, y) = rt.find(pill).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    let mut smallest = f32::MAX;
    for _ in 0..120 {
        rt.frame(1.0 / 120.0);
        smallest = smallest.min(drawn(&rt, &control).unwrap().0[2]);
        let label = drawn(&rt, &format!("{pill}/label")).unwrap().0;
        let (suffix, _) = drawn(&rt, &format!("{pill}/suffix")).unwrap();
        assert!(
            suffix[0] >= label[0] + label[2] - 0.5,
            "\"ed\" follows \"Allow\" without overlapping it: {label:?} {suffix:?}"
        );
        let (dot, _) = drawn(&rt, &format!("{control}/mark/dot")).unwrap();
        let (_, ink) = drawn(&rt, &format!("{control}/mark/ink")).unwrap();
        assert!(
            ink < 0.05 || dot[2] < 1.0,
            "the check appears once the dot has gone: dot {dot:?}, check opacity {ink}"
        );
    }
    assert!(rt.state.platform.system_audio);
    assert!(
        smallest < at_rest[2] * 0.97,
        "the control gathers: {smallest} of {}",
        at_rest[2]
    );
    let allowed = drawn(&rt, &control).unwrap().0;
    assert!(
        (allowed[2] - at_rest[2]).abs() < 2.0,
        "and releases to its size: {at_rest:?} → {allowed:?}"
    );
    assert!(rt.find(&format!("{control}/mark/ink/check")).is_some());
    let shown = texts(&rt);
    assert!(shown.iter().any(|text| text == "Allow"));
    assert!(shown.iter().any(|text| text == "ed"));

    tap(&mut rt, pill);
    assert_eq!(asked(&rt).len(), 2, "Allowed is not a button");

    // Coming back to the pane shows Allowed at rest, without replaying.
    tap(&mut rt, "nav-general");
    let (x, y) = rt.find("nav-meetings").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    let moving: Vec<_> = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.contains(&control) && e.moving)
        .map(|e| e.path)
        .collect();
    assert!(moving.is_empty(), "replayed: {moving:?}");
}

#[test]
fn reduce_motion_swaps_allow_for_allowed_at_once() {
    let mut rt = runtime("meetings-allow", true, PANE_SIZE);
    let pill = "meeting-system-audio-allow";
    tap(&mut rt, pill);
    let mut settled = runtime("meetings-allow", true, PANE_SIZE);
    settled.state.platform.system_audio = true;
    refresh(&mut settled);
    let allowed = settled.presented(pill).unwrap().0;

    rt.set_system_reduced_motion(true);
    let (x, y) = rt.find(pill).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.presented(pill).unwrap().0, allowed, "no travel");
}

#[test]
fn the_browser_switch_shows_only_with_pause_media() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    assert!(rt.find("pause-browser").is_none());
    rt.state.settings.recording_audio_behavior =
        crate::app_settings::RecordingAudioBehavior::PauseMedia;
    refresh(&mut rt);
    assert!(rt.find("pause-browser").is_some());
    assert!(
        rt.find("pause-browser-audio").is_none(),
        "the permission row waits for the switch"
    );
}

#[test]
fn the_browser_switch_turns_on_and_asks_for_system_audio() {
    let mut rt = runtime("mac-settings-pause-browser", true, PANE_SIZE);
    rt.state.settings.pause_browser_media = false;
    refresh(&mut rt);
    tap(&mut rt, "pause-browser");
    assert!(rt.state.settings.pause_browser_media);
    assert_eq!(asked(&rt), [PlatformRequest::SystemAudio]);
    assert!(
        rt.find("pause-browser-allow-control/mark/dot").is_some(),
        "the permission row waits with a dot"
    );
}

#[test]
fn an_allowed_browser_switch_asks_nothing() {
    let mut rt = runtime("mac-settings-pause-browser", true, PANE_SIZE);
    rt.state.settings.pause_browser_media = false;
    rt.state.platform.system_audio = true;
    refresh(&mut rt);
    tap(&mut rt, "pause-browser");
    assert!(rt.state.settings.pause_browser_media);
    assert!(asked(&rt).is_empty());
    assert!(
        rt.find("pause-browser-allow-control/mark/ink/check")
            .is_some(),
        "shows Allowed"
    );
}

#[test]
fn general_hides_advanced_options_until_asked() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    assert!(rt.find("microphone-mode").is_none());
    tap(&mut rt, "advanced-toggle");
    assert!(rt.find("microphone-mode").is_some());
    assert_eq!(
        rt.find("double-tap-only").is_some(),
        rt.state.settings.dictation_hotkey.key.is_some(),
        "double-tap only needs a key chord"
    );
    tap(&mut rt, "advanced-toggle");
    assert!(rt.find("microphone-mode").is_none());
}

#[test]
fn double_tap_to_lock_shows_only_for_hold() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    assert!(rt.find("double-tap").is_some());
    tap(&mut rt, "shortcut-mode-control/segment1");
    assert_eq!(
        rt.state.settings.shortcut_mode,
        crate::dictation::ShortcutMode::Toggle
    );
    assert!(rt.find("double-tap").is_none());
}

#[test]
fn how_it_works_opens_the_shortcut_explainer_and_keeps_focus_inside() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    tab_to(&mut rt, "shortcut-help");
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    settle(&mut rt);
    assert!(rt.state.explainer.open);
    tab_to(&mut rt, "mode-hold");
    let stops = ["mode-toggle", "double-tap", "close-explainer", "mode-hold"];
    for stop in stops {
        key(&mut rt, Key::Tab, Mods::default());
        assert!(
            focused(&rt).ends_with(stop),
            "Tab reached {} instead of {stop}",
            focused(&rt)
        );
    }
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(!rt.state.explainer.open);
    assert!(
        focused(&rt).ends_with("shortcut-help"),
        "focus went to {}",
        focused(&rt)
    );

    // Return presses Close, the default button, even from a focused card.
    let mode = rt.state.settings.shortcut_mode;
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    settle(&mut rt);
    tab_to(&mut rt, "mode-toggle");
    key(&mut rt, Key::Enter, Mods::default());
    settle(&mut rt);
    assert!(!rt.state.explainer.open);
    assert_eq!(rt.state.settings.shortcut_mode, mode);
}

#[test]
fn explainer_cards_pick_the_shortcut_mode() {
    let mut rt = runtime("shortcut-explainer", true, PANE_SIZE);
    let mode = |rt: &Runtime<Ui>| rt.state.settings.shortcut_mode;
    assert_eq!(mode(&rt), crate::dictation::ShortcutMode::Hold);
    assert!(
        rt.find("shortcut-explainer/explainer-body/lock/double-tap")
            .is_some()
    );
    tab_to(&mut rt, "mode-toggle");
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    settle(&mut rt);
    assert_eq!(mode(&rt), crate::dictation::ShortcutMode::Toggle);
    assert_eq!(saved(&rt), 1);
    assert!(
        rt.find("shortcut-explainer/explainer-body/lock/double-tap")
            .is_none(),
        "Double-tap to lock is Hold's"
    );
    tap(&mut rt, "mode-hold");
    assert_eq!(mode(&rt), crate::dictation::ShortcutMode::Hold);
    tap(&mut rt, "shortcut-explainer/explainer-body/lock/double-tap");
    assert!(!rt.state.settings.double_tap_lock);
    tap(&mut rt, "close-explainer");
    assert!(!rt.state.explainer.open);
}

#[test]
fn the_explainer_loops_on_the_poll_clock() {
    let mut rt = runtime("shortcut-explainer-motion", true, PANE_SIZE);
    let pressed = |rt: &Runtime<Ui>| {
        rt.elements()
            .into_iter()
            .find(|e| e.path.ends_with("mode-hold/visual/keys/keycap0/cap"))
            .map(|e| e.target[1])
            .unwrap()
    };
    let up = pressed(&rt);
    // Hold presses from 0.4 s to 2.4 s into the loop.
    rt.frame(1.0);
    assert!(pressed(&rt) > up, "the keycap goes down while held");
    rt.frame(2.0);
    assert_eq!(pressed(&rt), up, "and comes back up");
    let held = runtime("shortcut-explainer", true, PANE_SIZE);
    assert_eq!(held.state.explainer.time(), explainer::SNAPSHOT_MS);
}

#[test]
fn reduce_motion_shows_the_finished_explainer_without_waking() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    rt.set_system_reduced_motion(true);
    settle(&mut rt);
    assert_eq!(rt.wake_deadline(), None);
    // Opened from outside a handler, so the next poll runs before any view.
    explainer::open(&mut rt.state);
    let mut tx = rune::Tx::default();
    assert!(
        !explainer::poll(&mut rt.state, 1.0, true, &mut tx),
        "the first poll already knows"
    );
    settle(&mut rt);
    assert!(rt.state.explainer.open);
    assert_eq!(rt.state.explainer.time(), explainer::DONE);
    assert_eq!(rt.wake_deadline(), None, "the loop never ticks");
}

/// The side picker and the recorder's right edge never move, whatever the
/// recorder shows, and with Reduce Motion its faces change in place.
#[test]
fn the_shortcut_row_holds_still_through_every_face_and_side() {
    use crate::app_settings::HotkeyModifiers;
    use settings::Capture;
    fn rect(rt: &Runtime<Ui>, key: &str) -> [f32; 4] {
        let found = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key));
        found.unwrap_or_else(|| panic!("no {key}")).rect
    }
    fn place(rt: &Runtime<Ui>) -> ([f32; 4], f32) {
        let recorder = rect(rt, "dictation-hotkey");
        (rect(rt, "dictation-side"), recorder[0] + recorder[2])
    }
    fn watch(rt: &mut Runtime<Ui>, at_rest: ([f32; 4], f32), reduced: bool) {
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
            assert_eq!(place(rt), at_rest, "the row moved");
            if !reduced {
                continue;
            }
            let elements = rt.elements();
            let faces = elements
                .iter()
                .find(|e| e.path.ends_with("dictation-hotkey/faces"))
                .unwrap()
                .rect;
            for face in elements {
                let shown = face.path.contains("dictation-hotkey/faces/")
                    && !face.path.contains("/faces/size")
                    && matches!(face.key.as_deref(), Some("shortcut" | "listening"));
                if shown {
                    assert_eq!(face.rect[1], faces[1], "{} travelled", face.path);
                }
            }
        }
    }
    for reduced in [false, true] {
        let mut rt = runtime("shortcut-row", true, PANE_SIZE);
        rt.set_system_reduced_motion(reduced);
        settle(&mut rt);
        let at_rest = place(&rt);
        for segment in ["segment1", "segment2", "segment0"] {
            let (x, y) = rt.find(&format!("dictation-side/{segment}")).unwrap();
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            watch(&mut rt, at_rest, reduced);
        }
        let (x, y) = rt.find("dictation-hotkey").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        watch(&mut rt, at_rest, reduced);
        assert!(matches!(rt.state.capture, Capture::Listening { .. }));
        settings::hold(&mut rt.state, HotkeyModifiers::option_command());
        rt.invalidate();
        watch(&mut rt, at_rest, reduced);
        settings::hold(&mut rt.state, HotkeyModifiers::default());
        key(&mut rt, Key::Text("d".into()), Mods::default());
        watch(&mut rt, at_rest, reduced);
        rt.state.capture = Capture::Saved {
            target: ShortcutTarget::Dictation,
            until: None,
        };
        rt.invalidate();
        watch(&mut rt, at_rest, reduced);
        rt.state.capture = Capture::Idle;
        rt.invalidate();
        watch(&mut rt, at_rest, reduced);
        // A chord has no side: the picker leaves beside the keys, never
        // under them, and the recorder keeps its right edge.
        let (x, y) = rt.find("dictation-hotkey").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(&mut rt);
        let ctrl_alt = Mods {
            ctrl: true,
            alt: true,
            ..Mods::default()
        };
        key(&mut rt, Key::Text("d".into()), ctrl_alt);
        while rt.find("dictation-side").is_some() {
            let recorder = rect(&rt, "dictation-hotkey");
            let keys = rect(&rt, "keys");
            let side = rect(&rt, "dictation-side");
            assert_eq!(recorder[0] + recorder[2], at_rest.1, "the recorder moved");
            assert!(
                side[0] + side[2] <= keys[0],
                "the picker slid under the keys"
            );
            rt.frame(1.0 / 60.0);
        }
    }
}

#[test]
fn esc_stays_on_the_recorder_however_many_keys_are_held() {
    use crate::app_settings::{HotkeyModifiers, ModifierSide};
    let mut rt = runtime("shortcut-row", true, PANE_SIZE);
    tap(&mut rt, "dictation-hotkey");
    let any = Some(ModifierSide::Either);
    settings::hold(
        &mut rt.state,
        HotkeyModifiers {
            control: any,
            option: any,
            shift: any,
            command: any,
            function: false,
        },
    );
    refresh(&mut rt);
    let recorder = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("dictation-hotkey"))
        .unwrap()
        .rect;
    let (escape, opacity) = drawn(&rt, "dictation-hotkey/faces/listening/escape").unwrap();
    assert_eq!(opacity, 1.0);
    assert!(
        escape[0] + escape[2] <= recorder[0] + recorder[2],
        "esc is pushed out of the recorder"
    );
    let held = drawn(&rt, "dictation-hotkey/faces/listening/held")
        .unwrap()
        .0;
    assert!(
        held[0] + held[2] <= escape[0],
        "the held keys run under esc"
    );
}

#[test]
fn a_new_shortcut_s_keys_stay_inside_their_tray() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    let binding = rt.state.settings.paste_last_hotkey.clone().unwrap();
    // Every key and Off stay inside the well while it springs to the new
    // width, entering and leaving alike; returns whether any was clipped.
    let watch = |rt: &mut Runtime<Ui>| {
        let mut clipped = false;
        for _ in 0..45 {
            rt.frame(1.0 / 60.0);
            let elements = rt.elements();
            let tray = elements
                .iter()
                .find(|e| e.path.ends_with("paste-last-control/recorder-slot/keys") && !e.exiting)
                .expect("the tray stays")
                .rect;
            for e in elements.iter().filter(|e| {
                e.path.contains("paste-last-control/recorder-slot/keys/")
                    && e.key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("keycap") || k == "off")
            }) {
                let [x, y, w, h] = e.visible;
                let hidden = w <= 0.0 || h <= 0.0;
                let inside = hidden
                    || x >= tray[0] - 0.01
                        && y >= tray[1] - 0.01
                        && x + w <= tray[0] + tray[2] + 0.01
                        && y + h <= tray[1] + tray[3] + 0.01;
                assert!(
                    inside,
                    "{} pokes out of its tray {tray:?}: visible {:?}, rect {:?}, exiting {}, opacity {}",
                    e.path, e.visible, e.rect, e.exiting, e.opacity
                );
                clipped |= w < e.rect[2] - 0.5 || h < e.rect[3] - 0.5;
            }
        }
        clipped
    };
    rt.state.settings.paste_last_hotkey = None;
    rt.invalidate();
    assert!(watch(&mut rt), "the keys leave inside the well's clip");
    rt.state.settings.paste_last_hotkey = Some(binding);
    rt.invalidate();
    assert!(watch(&mut rt), "the keys arrive inside the well's clip");
}

#[test]
fn the_practice_keys_stay_and_spring_back_up_on_release() {
    let mut rt = runtime("mac-onboarding-try", true, PANE_SIZE);
    let cap = |rt: &Runtime<Ui>| {
        drawn(rt, "practice-hint/keys/keycap0/cap")
            .expect("the keys stay")
            .0
    };
    let hint = |rt: &Runtime<Ui>| drawn(rt, "practice-hint").unwrap().0;
    let (rest, line) = (cap(&rt), hint(&rt));
    onboarding::input(&mut rt.state, Input::Dictation(Observation::Started));
    refresh(&mut rt);
    assert!(cap(&rt)[1] > rest[1] + 0.5, "held down while it records");
    assert_eq!(hint(&rt), line, "the page holds still");
    onboarding::input(&mut rt.state, Input::Dictation(Observation::Transcribing));
    rt.invalidate();
    let mut last = cap(&rt)[1];
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
        let now = cap(&rt)[1];
        assert!(now <= last + 0.01, "the keys spring up, never back down");
        assert_eq!(hint(&rt), line, "the page holds still");
        last = now;
    }
    assert!((last - rest[1]).abs() < 0.05, "back at rest");
    // A retry presses the keys standing up, from any outcome.
    for outcome in [Observation::Discarded, Observation::Failed] {
        onboarding::input(&mut rt.state, Input::Dictation(outcome));
        refresh(&mut rt);
        assert!((cap(&rt)[1] - rest[1]).abs() < 0.05, "up after {outcome:?}");
        onboarding::input(&mut rt.state, Input::Dictation(Observation::Started));
        refresh(&mut rt);
        assert!(
            cap(&rt)[1] > rest[1] + 0.5,
            "pressed again after {outcome:?}"
        );
    }
}

#[test]
fn the_explainer_draws_a_keycap_per_key_and_lines_up_the_lanes() {
    use crate::app_settings::{HotkeyBinding, HotkeyModifiers};
    let mut rt = runtime("shortcut-explainer-chord", true, PANE_SIZE);
    let default = rt.state.settings.dictation_hotkey.clone();
    let one = HotkeyBinding {
        modifiers: HotkeyModifiers {
            option: default.modifiers.option,
            ..HotkeyModifiers::default()
        },
        key: None,
    };
    let three = rt.state.settings.dictation_hotkey.clone();
    for binding in [one, default, three] {
        rt.state.settings.dictation_hotkey = binding.clone();
        refresh(&mut rt);
        let elements = rt.elements();
        let rect = |suffix: &str| {
            elements
                .iter()
                .find(|e| e.path.ends_with(suffix))
                .map(|e| e.target)
                .unwrap_or_else(|| panic!("no {suffix}"))
        };
        let labels = binding.keycaps();
        for owner in ["mode-hold", "mode-toggle", "double-tap"] {
            let caps: Vec<_> = (0..labels.len())
                .map(|index| rect(&format!("{owner}/visual/keys/keycap{index}")))
                .collect();
            assert!(
                elements.iter().all(|e| !e
                    .path
                    .ends_with(&format!("{owner}/visual/keys/keycap{}", labels.len()))),
                "{owner} draws one keycap per key of {labels:?}"
            );
            for pair in caps.windows(2) {
                assert!(
                    pair[1][0] > pair[0][0] + pair[0][2],
                    "{owner}'s keycaps sit side by side"
                );
                assert_eq!(pair[0][1], pair[1][1], "{owner}'s keycaps share a baseline");
            }
        }
        let hold = rect("mode-hold/visual/lanes");
        let toggle = rect("mode-toggle/visual/lanes");
        let lock = rect("double-tap/visual/lanes");
        assert!(
            (hold[0] - lock[0]).abs() < 0.5,
            "{labels:?}: lanes start together"
        );
        assert!(
            (hold[2] - toggle[2]).abs() < 0.5,
            "{labels:?}: both cards' lanes are as wide"
        );
        assert!(hold[2] > 30.0, "{labels:?}: lanes keep room to draw");
    }
}

/// Opened in either mode, the panel centres at its Hold height, so Hold and
/// Toggle move only its bottom edge; resizing the window centres it again.
#[test]
fn the_explainer_centres_at_its_hold_height_in_either_mode() {
    fn panel(rt: &Runtime<Ui>) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("shortcut-explainer"))
            .map(|e| e.target)
            .expect("the panel")
    }
    for (size, scene) in [
        (PANE_SIZE, "shortcut-explainer-toggle"),
        (PANE_SIZE, "shortcut-explainer"),
        ((900.0, 620.0), "shortcut-explainer-toggle"),
    ] {
        let mut rt = runtime(scene, true, size);
        settle(&mut rt);
        let opened = panel(&rt);
        tap(&mut rt, "mode-hold");
        settle(&mut rt);
        let hold = panel(&rt);
        assert!(
            (hold[1] - opened[1]).abs() < 0.5,
            "{scene}: the top moved from {} to {}",
            opened[1],
            hold[1]
        );
        assert!(
            (hold[1] + hold[3] / 2.0 - size.1 / 2.0).abs() < 0.5,
            "{scene}: Hold is centred in {size:?}: {hold:?}"
        );
        tap(&mut rt, "mode-toggle");
        settle(&mut rt);
        let toggle = panel(&rt);
        assert!(
            (toggle[1] - opened[1]).abs() < 0.5,
            "{scene}: Toggle {opened:?} {hold:?} {toggle:?}"
        );
        rt.set_viewport((size.0, size.1 + 100.0), 1.0);
        settle(&mut rt);
        assert!(
            (panel(&rt)[1] - opened[1] - 50.0).abs() < 0.5,
            "{scene}: a taller window centres it again"
        );
    }
}

/// A five-key chord widens the panel so every card's lanes keep room, and
/// the keycaps stay inside their cards.
#[test]
fn a_long_chord_widens_the_explainer() {
    use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide};
    let mut rt = runtime("shortcut-explainer-chord", true, PANE_SIZE);
    let side = Some(ModifierSide::Either);
    rt.state.settings.dictation_hotkey = HotkeyBinding {
        modifiers: HotkeyModifiers {
            control: side,
            option: side,
            shift: side,
            command: side,
            ..HotkeyModifiers::default()
        },
        key: Some(HotkeyKey::named("space")),
    };
    refresh(&mut rt);
    let elements = rt.elements();
    let rect = |suffix: &str| {
        elements
            .iter()
            .find(|e| e.path.ends_with(suffix))
            .map(|e| e.target)
            .unwrap_or_else(|| panic!("no {suffix}"))
    };
    let panel = rect("shortcut-explainer");
    assert!(panel[0] >= 0.0 && panel[0] + panel[2] <= PANE_SIZE.0);
    for owner in ["modes/mode-hold", "modes/mode-toggle", "lock/double-tap"] {
        let (card, keys, lanes) = (
            rect(&format!("explainer-body/{owner}")),
            rect(&format!("{owner}/visual/keys")),
            rect(&format!("{owner}/visual/lanes")),
        );
        assert!(lanes[2] >= 63.5, "{owner}'s lanes keep room: {lanes:?}");
        assert!(
            keys[0] >= card[0] && lanes[0] >= keys[0] + keys[2],
            "{owner}: {card:?} {keys:?} {lanes:?} {panel:?}"
        );
        assert!(
            lanes[0] + lanes[2] <= card[0] + card[2],
            "{owner}'s lanes stay in the card"
        );
    }
}

/// Reduce Motion is read again when the window gains focus, after that
/// frame's poll; the tape starts again on the next.
#[test]
fn the_explainer_restarts_when_reduce_motion_turns_off() {
    let mut rt = runtime("shortcut-explainer-motion", true, PANE_SIZE);
    rt.set_system_reduced_motion(true);
    settle(&mut rt);
    assert_eq!(rt.state.explainer.time(), explainer::DONE);
    assert_eq!(rt.wake_deadline(), None);
    rt.window_focus(false);
    settle(&mut rt);
    rt.set_system_reduced_motion(false);
    rt.window_focus(true);
    // A host draws no further frame unless one is asked for.
    rt.frame(1.0 / 60.0);
    assert!(
        rt.wake_deadline().is_some(),
        "the focus frame asks for another"
    );
    settle(&mut rt);
    let before = rt.state.explainer.time();
    rt.frame(0.25);
    assert_ne!(rt.state.explainer.time(), before, "and moves");
}

#[test]
fn the_explainer_picker_and_shortcut_recording_never_share_the_window() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    tap(&mut rt, "dictation-hotkey");
    assert!(matches!(
        rt.state.capture,
        settings::Capture::Listening { .. }
    ));
    explainer::open(&mut rt.state);
    assert!(matches!(rt.state.capture, settings::Capture::Idle));
    assert!(sent(&rt).contains(&Request::CancelShortcut));

    picker::open(&mut rt.state);
    assert!(!rt.state.explainer.open, "the picker closes the explainer");
    explainer::open(&mut rt.state);
    assert!(!rt.state.picker.open, "and the explainer the picker");

    rt.state
        .select_pane(Pane::History, &mut rune::Tx::default());
    assert!(!rt.state.explainer.open);
    picker::open(&mut rt.state);
    rt.state
        .select_pane(Pane::General, &mut rune::Tx::default());
    assert!(!rt.state.picker.open);
}

#[test]
fn a_failed_command_model_offers_retry_and_turning_commands_off() {
    let mut rt = runtime("mac-commands-unavailable", true, PANE_SIZE);
    assert!(
        runtime("mac-settings-permissions", true, (980.0, 2400.0))
            .find("command-model-notice")
            .is_none(),
        "the notice lives in Commands"
    );
    tap(&mut rt, "retry-commands");
    assert_eq!(asked(&rt), [PlatformRequest::RetryCommandModel]);
    tap(&mut rt, "disable-commands");
    assert!(!rt.state.settings.commands_enabled);
    assert!(rt.find("command-model-notice").is_none());
}

#[test]
fn meetings_switch_between_recordings_and_settings() {
    let mut rt = runtime("meetings", true, PANE_SIZE);
    assert!(rt.find("meeting-preview-design").is_some());
    assert!(rt.find("meetings-enabled").is_none());
    tap(&mut rt, "meetings-view/segment1");
    assert_eq!(rt.state.meetings.view, meetings::View::Settings);
    assert!(rt.find("meetings-enabled").is_some());
    assert!(rt.find("meeting-preview-design").is_none());
    tap(&mut rt, "meetings-view/segment0");
    assert!(rt.find("meeting-preview-design").is_some());
}

#[test]
fn the_meetings_switch_stays_put_and_a_swap_drops_pending_edits() {
    let mut rt = runtime("meetings", true, PANE_SIZE);
    let switch = |rt: &Runtime<Ui>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("meetings-view"))
            .map(|e| e.rect)
            .unwrap()
    };
    let recordings = switch(&rt);
    tap(&mut rt, "meeting-rename");
    assert!(rt.state.meetings.renaming.is_some());
    rt.state.meetings.delete_armed = Some("preview-notes".into());
    tap(&mut rt, "meetings-view/segment1");
    assert_eq!(switch(&rt), recordings, "the search field never moves it");
    assert!(rt.state.meetings.renaming.is_none());
    assert!(rt.state.meetings.delete_armed.is_none());

    tap(&mut rt, "meetings-enabled");
    assert!(!rt.state.settings.meetings.enabled);
    assert_eq!(rt.state.meetings.view, meetings::View::Recordings);
}

#[test]
fn an_update_is_a_secondary_button_with_a_dot_and_its_own_label() {
    let mut rt = runtime("mac-update-ready", true, PANE_SIZE);
    assert!(rt.find("sidebar-update-control/dot").is_some());
    assert!(texts(&rt).iter().any(|text| text == "Restart to update"));
    tap(&mut rt, "sidebar-update");
    assert!(matches!(
        sent(&rt),
        [Request::Desktop(DesktopAction::RestartIntoUpdate)]
    ));
    if let Some(view) = &mut rt.state.view {
        view.update_status = DesktopUpdateStatus::Available;
    }
    refresh(&mut rt);
    assert!(texts(&rt).iter().any(|text| text == "Download update"));
}

#[test]
fn meetings_off_offers_the_opt_in_and_keeps_the_pane() {
    let mut rt = runtime("meetings-off", true, PANE_SIZE);
    assert!(rt.find("nav-meetings").is_some(), "always in the sidebar");
    assert!(rt.find("meetings-view").is_none());
    assert!(rt.find("start-meeting").is_none());
    tap(&mut rt, "meetings-enabled");
    assert!(rt.state.settings.meetings.enabled);
    assert!(rt.find("meetings-view").is_some());
}

#[test]
fn meetings_settings_save_and_ask_for_system_audio() {
    let mut rt = runtime("meetings-settings", true, (980.0, 2400.0));
    rt.state.platform.system_audio = false;
    refresh(&mut rt);
    tap(&mut rt, "meeting-system-audio-allow");
    assert_eq!(asked(&rt), [PlatformRequest::SystemAudio]);
    use crate::app_settings::MeetingPolicy;
    let policy = |rt: &Runtime<Ui>, key: &str| {
        rt.state
            .settings
            .meetings
            .providers
            .iter()
            .find(|choice| choice.key == key)
            .map(|choice| choice.policy)
    };
    // Known services first (Zoom is second), then the sites and apps chosen for.
    tap(&mut rt, "call1/policy/segment1");
    assert_eq!(
        policy(&rt, "us.zoom.xos"),
        Some(MeetingPolicy::AlwaysRecord)
    );
    tap(&mut rt, "call5/policy/segment0");
    assert_eq!(
        policy(&rt, "problems.kitlangton.com"),
        Some(MeetingPolicy::Ask)
    );
    assert!(rt.find("call5").is_some());
    tap(&mut rt, "meetings-enabled");
    assert!(!rt.state.settings.meetings.enabled);
    assert!(rt.find("meeting-folder").is_none());
    assert!(rt.find("meetings-enabled").is_some(), "the opt-in remains");
}

#[test]
fn the_calendar_asks_for_access_only_when_connect_is_pressed() {
    let mut rt = runtime("meetings-settings", true, (980.0, 2400.0));
    assert!(rt.find("calendar-connect").is_some());
    assert!(rt.find("calendar-remind").is_none());
    assert!(asked(&rt).is_empty());
    tap(&mut rt, "calendar-connect-button");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Calendar(CalendarRequest::Connect)]
    );

    // Meetings off: no calendar card, so nothing can ask for access.
    tap(&mut rt, "meetings-enabled");
    assert!(rt.find("calendar").is_none());
    for scene in ["portal", "windows-settings"] {
        let rt = runtime(scene, true, GOLDEN_SIZE);
        assert!(rt.find("calendar").is_none(), "{scene} has no calendar");
    }
}

#[test]
fn a_connected_calendar_saves_reminders_and_lists_meetings() {
    let mut rt = runtime("meetings-calendar", true, (980.0, 2600.0));
    let shown = texts(&rt);
    for text in [
        "Design sync",
        "Today · 2:30 PM",
        "Google Meet",
        "2 of 3 calendars",
    ] {
        assert!(shown.iter().any(|shown| shown == text), "shows {text}");
    }
    assert!(rt.find("upcoming2").is_some());
    assert!(
        rt.find("upcoming2/provider/icon").is_none(),
        "no link, no icon"
    );
    tap(&mut rt, "calendar-lead/lead/segment3");
    assert_eq!(rt.state.settings.meetings.calendar.lead_minutes, 5);
    tap(&mut rt, "calendar-remind");
    assert!(!rt.state.settings.meetings.calendar.remind);
    assert!(rt.find("calendar-lead").is_none());
    tap(&mut rt, "calendar-calendars-control-trigger");
    assert_eq!(rt.state.open, Some(Menu::Calendars));
}

#[test]
fn denied_calendar_access_links_to_privacy_settings() {
    let mut rt = runtime("meetings-calendar-denied", true, (980.0, 2400.0));
    assert!(rt.find("calendar-connect").is_none());
    tap(&mut rt, "calendar-privacy");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Calendar(
            CalendarRequest::OpenPrivacySettings
        )]
    );
}

#[test]
fn commands_search_select_and_show_their_detail() {
    let mut rt = runtime("mac-commands", true, PANE_SIZE);
    assert!(texts(&rt).iter().any(|text| text == "Voice control"));
    tap(&mut rt, "command-github.pulls");
    assert_eq!(rt.state.commands.selected.as_deref(), Some("github.pulls"));
    let shown = texts(&rt);
    assert!(shown.iter().any(|text| text == "github.com"));
    assert!(shown.iter().any(|text| text == "pull requests\nopen pulls"));
    rt.state.commands.search = TextEdit::new("slack");
    refresh(&mut rt);
    assert!(rt.find("command-open.slack").is_some());
    assert!(rt.find("command-mode.sleep").is_none());
    rt.state.commands.search = TextEdit::new("nothing like this");
    refresh(&mut rt);
    assert!(
        texts(&rt)
            .iter()
            .any(|text| text == "No matching commands.")
    );
}

#[test]
fn commands_turn_on_from_their_card() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    tap(&mut rt, "nav-commands");
    assert!(rt.find("commands-enabled").is_some());
    tap(&mut rt, "commands-enabled");
    assert!(rt.state.settings.commands_enabled);
}

#[test]
fn enabling_commands_with_a_released_microphone_asks_first() {
    let mut rt = runtime("mac-commands", true, PANE_SIZE);
    tap(&mut rt, "commands-enabled");
    assert!(!rt.state.settings.commands_enabled);
    rt.state.settings.release_microphone_while_idle = true;
    refresh(&mut rt);
    tap(&mut rt, "commands-enabled");
    assert!(rt.state.commands.confirm_enable);
    assert!(!rt.state.settings.commands_enabled);
    tap(&mut rt, "confirm-enable-commands");
    assert!(rt.state.settings.commands_enabled);
    assert!(!rt.state.settings.release_microphone_while_idle);
}

#[test]
fn commands_copy_the_agent_prompt_and_open_the_config() {
    let mut rt = runtime("mac-commands", true, PANE_SIZE);
    tap(&mut rt, "commands-copy-prompt");
    assert!(
        rt.copy_request
            .as_deref()
            .is_some_and(|prompt| prompt.contains("hex.config.ts"))
    );
    tap(&mut rt, "commands-open-config");
    assert_eq!(asked(&rt), [PlatformRequest::OpenCommandsConfig]);
}

#[test]
fn meetings_select_search_and_copy_a_transcript() {
    let mut rt = runtime("meetings", true, PANE_SIZE);
    tap(&mut rt, "meeting-preview-design");
    assert_eq!(
        rt.state.meetings.selected.as_deref(),
        Some("preview-design")
    );
    tap(&mut rt, "meeting-copy-transcript");
    let copied = rt.copy_request.as_deref().unwrap().to_owned();
    assert!(copied.starts_with("**You** · 00:09\nOh nice."), "{copied}");
    rt.state.meetings.search = TextEdit::new("roblox");
    refresh(&mut rt);
    assert!(rt.find("meeting-preview-failed").is_none());
    assert!(
        rt.find("meeting-preview-notes").is_some(),
        "searches transcripts"
    );
}

#[test]
fn recording_starts_and_stops_from_the_header() {
    let mut rt = runtime("meetings", true, PANE_SIZE);
    tap(&mut rt, "stop-meeting");
    assert_eq!(asked(&rt), [PlatformRequest::Meeting(MeetingRequest::Stop)]);
    assert!(rt.find("stop-meeting").is_none(), "finalizing");

    let mut rt = runtime("meetings-empty", true, PANE_SIZE);
    assert!(texts(&rt).iter().any(|text| text == "No meetings yet."));
    tap(&mut rt, "start-meeting");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Meeting(MeetingRequest::Start)]
    );
}

#[test]
fn meetings_rename_from_the_title_field() {
    let mut rt = runtime("meetings", true, PANE_SIZE);
    tap(&mut rt, "meeting-rename");
    assert_eq!(rt.state.meetings.renaming.as_deref(), Some("preview-notes"));
    rt.state.meetings.rename = TextEdit::new("Arcade launch");
    refresh(&mut rt);
    tap(&mut rt, "meeting-save-title");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Meeting(MeetingRequest::Rename {
            id: "preview-notes".into(),
            title: "Arcade launch".into(),
        })]
    );
    assert_eq!(rt.state.meetings.meetings[1].title, "Arcade launch");
    assert!(rt.state.meetings.renaming.is_none());
}

#[test]
fn deleting_a_meeting_takes_the_menu_then_a_confirmation() {
    let mut rt = runtime("meetings", true, PANE_SIZE);
    tap(&mut rt, "meeting-more");
    assert_eq!(rt.state.open, Some(Menu::MeetingActions));
    // Copy summary, Reveal Markdown, Regenerate summary, a separator, Delete.
    tap(&mut rt, "meeting-actions-item4");
    assert_eq!(
        rt.state.meetings.delete_armed.as_deref(),
        Some("preview-notes")
    );
    assert!(asked(&rt).is_empty(), "the first step only arms");
    assert!(
        texts(&rt)
            .iter()
            .any(|text| text == "Trash recording & Markdown")
    );
    tap(&mut rt, "meeting-confirm-delete");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Meeting(MeetingRequest::Trash(
            "preview-notes".into()
        ))]
    );
    assert!(
        rt.state
            .meetings
            .meetings
            .iter()
            .all(|m| m.id != "preview-notes")
    );

    let mut rt = runtime("meetings-delete", true, PANE_SIZE);
    key(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.meetings.delete_armed.is_none(), "Escape cancels");

    let mut rt = runtime("meetings-delete", true, PANE_SIZE);
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Meeting(MeetingRequest::Trash(
            "preview-notes".into()
        ))],
        "Return confirms"
    );
}

#[test]
fn the_overflow_menu_copies_the_summary_and_asks_for_new_notes() {
    let mut rt = runtime("meetings-actions", true, PANE_SIZE);
    tap(&mut rt, "meeting-actions-item0");
    assert!(
        rt.copy_request
            .as_deref()
            .is_some_and(|summary| summary.starts_with("One login"))
    );
    assert_eq!(rt.state.open, Some(Menu::MeetingActions), "stays open");
    tap(&mut rt, "meeting-actions-item2");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Meeting(MeetingRequest::Summarize(
            "preview-notes".into()
        ))]
    );
}

#[test]
fn copy_section_copies_one_chapter() {
    let rt = runtime("meetings", true, PANE_SIZE);
    assert!(rt.find("chapters").is_none(), "no chapters, no section");

    let mut rt = runtime("meetings-chapters", true, PANE_SIZE);
    assert!(texts(&rt).iter().any(|text| text == "Chapters"));
    tap(&mut rt, "chapter1/copy-section");
    let copied = rt.copy_request.as_deref().unwrap().to_owned();
    assert!(
        copied.starts_with("## Gravity well\n\nA pulse and gravity well"),
        "{copied}"
    );
    assert!(copied.contains("Prototype it in WebGPU"), "{copied}");
    assert!(!copied.contains("Roblox") && !copied.contains("build screen"));
    assert!(texts(&rt).iter().any(|text| text == "Copied"));
}

#[test]
fn active_and_failed_meetings_show_their_state() {
    let rt = runtime("meetings-recording", true, PANE_SIZE);
    let shown = texts(&rt);
    assert!(shown.iter().any(|text| text == "Live transcript"));
    assert!(shown.iter().any(|text| text == "Listening…"));
    assert!(
        rt.find("meeting-rename").is_none(),
        "no rename while recording"
    );
    let rt = runtime("meetings-failed", true, PANE_SIZE);
    let shown = texts(&rt);
    assert!(shown.iter().any(|text| text == "Recording failed."));
    assert!(shown.iter().any(|text| text == "Recovered draft"));
}

#[test]
fn the_mac_tour_asks_each_permission_then_practices() {
    let mut rt = runtime("mac-onboarding-setup", true, PANE_SIZE);
    assert_eq!(rt.state.tour.as_ref().unwrap().flow.step(), Step::Setup);
    assert!(
        matches!(sent(&rt), [Request::ChooseModel { .. }]),
        "the model downloads behind the permissions"
    );
    tap(&mut rt, "setup-microphone/setup-grant-control/setup-grant");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Permission(
            PermissionAction::RequestMicrophone
        )]
    );
    // macOS grants everything and the model lands: setup moves on.
    use crate::onboarding::PermissionState::Ready;
    rt.state.platform.permissions.microphone = Ready;
    rt.state.platform.permissions.accessibility = Ready;
    rt.state.picker.installed = Some(vec![rt.state.settings.transcription.model]);
    refresh(&mut rt);
    std::thread::sleep(std::time::Duration::from_millis(750));
    settle(&mut rt);
    assert_eq!(rt.state.tour.as_ref().unwrap().flow.step(), Step::TryIt);
    assert!(rt.find("tour-skip").is_some(), "practice can be skipped");
    onboarding::input(&mut rt.state, Input::Dictation(Observation::Started));
    onboarding::input(&mut rt.state, Input::Dictation(Observation::Pasted));
    refresh(&mut rt);
    assert_eq!(
        rt.state.tour.as_ref().unwrap().flow.view().practice,
        Practice::Landed
    );
    tap(&mut rt, "tour-continue");
    assert_eq!(rt.state.tour.as_ref().unwrap().flow.step(), Step::Done);
    tap(&mut rt, "tour-continue");
    assert!(rt.state.tour.is_none());
}

#[test]
fn a_listener_that_will_not_start_relaunches_once_then_asks_to_reopen() {
    let mut rt = runtime("mac-onboarding-try", true, PANE_SIZE);
    onboarding::input(
        &mut rt.state,
        Input::ListenerFailed {
            relaunch_allowed: true,
        },
    );
    assert_eq!(asked(&rt), [PlatformRequest::Relaunch]);
    let mut rt = runtime("mac-onboarding-reopen", true, PANE_SIZE);
    tap(&mut rt, "tour-reopen");
    assert_eq!(asked(&rt), [PlatformRequest::Relaunch]);
}

#[test]
fn a_grant_waiting_for_a_restart_makes_setup_restart_hex() {
    let mut rt = runtime("mac-onboarding-restart", true, PANE_SIZE);
    assert_eq!(rt.state.tour.as_ref().unwrap().flow.step(), Step::Setup);
    tap(&mut rt, "tour-continue");
    assert_eq!(asked(&rt), [PlatformRequest::Relaunch]);
}

#[test]
fn a_refused_microphone_points_to_system_settings() {
    let mut rt = runtime("mac-onboarding-denied", true, PANE_SIZE);
    let shown = texts(&rt);
    assert!(
        shown
            .iter()
            .any(|text| text == "Turned off earlier. Switch Hex on in System Settings.")
    );
    tap(&mut rt, "setup-microphone/setup-grant-control/setup-grant");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::Permission(PermissionAction::ShowGuide(
            PermissionKind::Microphone
        ))]
    );
}

/// The macOS and Windows surfaces in both appearances.
#[test]
fn platform_goldens() {
    let _goldens = Goldens::collect();
    for dark in [true, false] {
        check_golden("meetings-calendar", dark, (980.0, 1800.0));
        for scene in [
            "mac-settings",
            "mac-input-source-models",
            "mac-commands",
            "meetings",
            "meetings-settings",
            "meetings-off",
            "meetings-recording",
            "mac-onboarding-setup",
            "mac-onboarding-landed",
            "shortcut-explainer",
            "shortcut-explainer-toggle",
        ] {
            check_golden(scene, dark, PANE_SIZE);
        }
    }
    check_golden("shortcut-explainer-chord", false, PANE_SIZE);
    check_golden("mac-settings-permissions", true, PANE_SIZE);
    check_golden("mac-settings-advanced", false, PANE_SIZE);
    check_golden("meetings-calendar-denied", true, PANE_SIZE);
    check_golden("mac-commands-unavailable", true, PANE_SIZE);
    check_golden("windows-settings", false, GOLDEN_SIZE);
    check_golden("mac-commands-confirm", false, PANE_SIZE);
    check_golden("meetings-actions", true, PANE_SIZE);
    check_golden("meetings-delete", false, PANE_SIZE);
    check_golden("meetings-rename", true, PANE_SIZE);
    check_golden("meetings-empty", false, PANE_SIZE);
    check_golden("mac-onboarding", true, PANE_SIZE);
    check_golden("mac-onboarding-progress", false, PANE_SIZE);
    check_golden("mac-onboarding-denied", true, PANE_SIZE);
    check_golden("mac-onboarding-download-failed", false, PANE_SIZE);
    check_golden("mac-onboarding-listening", true, PANE_SIZE);
    check_golden("mac-onboarding-reopen", false, PANE_SIZE);
    check_golden("mac-onboarding-done", true, PANE_SIZE);
    check_golden("mac-onboarding-mic-open", true, PANE_SIZE);
    check_golden("mac-onboarding-mic-silent", false, PANE_SIZE);
}

#[test]
fn voice_filter_saves_the_mode_and_applies_the_tuning_live() {
    use crate::voice_filter::VoiceFilterMode;
    let mut rt = runtime("mac-voice-filter", true, PANE_SIZE);
    tap(&mut rt, "fun-mode/segment1");
    assert_eq!(
        rt.state.settings.experimental.hex_microphone_mode,
        VoiceFilterMode::Muted
    );
    assert!(matches!(
        asked(&rt),
        [PlatformRequest::SetTuning(cfg)] if cfg.mode == VoiceFilterMode::Muted
    ));
    assert!(
        rt.find("fun-comfort-noise").is_some(),
        "Muted offers comfort noise"
    );
    tap(&mut rt, "fun-mode/segment3");
    assert!(
        rt.find("fun-slider-strength").is_none(),
        "Normal has no sliders"
    );
}

#[test]
fn voice_filter_sliders_set_the_filter_from_the_pointer() {
    let mut rt = runtime("mac-voice-filter", true, PANE_SIZE);
    let before = rt.state.platform.tuning.muffled_wall_hz;
    let (x, y) = rt.find("fun-slider-wall/track").unwrap();
    rt.pointer_down(x + 140.0, y);
    rt.pointer_up(x + 140.0, y);
    settle(&mut rt);
    let after = rt.state.platform.tuning.muffled_wall_hz;
    assert!(after > before && after >= 2900.0, "{before} -> {after}");
    assert!(matches!(
        asked(&rt).last(),
        Some(PlatformRequest::SetTuning(_))
    ));
    tap(&mut rt, "fun-reset-tuning");
    assert_eq!(rt.state.platform.tuning.muffled_wall_hz, before);
}

#[test]
fn try_it_records_and_plays_through_the_host() {
    use platform::DemoRequest;
    let mut rt = runtime("mac-voice-filter", true, PANE_SIZE);
    tap(&mut rt, "fun-demo-record");
    tap(&mut rt, "fun-play-call-hears");
    tap(&mut rt, "fun-demo-source/segment1");
    assert_eq!(
        asked(&rt),
        [
            PlatformRequest::HexMicrophoneDemo(DemoRequest::Record),
            PlatformRequest::HexMicrophoneDemo(DemoRequest::Play(
                crate::hex_microphone::DemoPlaybackTrack::CallHears
            )),
            PlatformRequest::HexMicrophoneDemo(DemoRequest::SelectSource(
                crate::hex_microphone::DemoSourceKind::Recording
            )),
        ]
    );
    let mut rt = runtime("mac-voice-filter-muted", true, PANE_SIZE);
    tap(&mut rt, "fun-play-call-hears");
    assert_eq!(
        asked(&rt),
        [PlatformRequest::HexMicrophoneDemo(
            DemoRequest::StopPlayback
        )]
    );
}

#[test]
fn developer_panes_show_only_in_developer_builds() {
    let rt = runtime("mac-settings", true, PANE_SIZE);
    assert!(rt.find("nav-hud-lab").is_none());
    assert!(rt.find("nav-activity").is_none());
    assert!(
        rt.find("nav-voice-filter").is_some(),
        "Hex Microphone is on"
    );
    assert!(rt.find("nav-experiments").is_some());
    let rt = runtime("mac-hud-lab", true, PANE_SIZE);
    assert!(rt.find("nav-hud-lab").is_some());
    assert!(rt.find("nav-activity").is_some());
}

#[test]
fn voice_filter_needs_its_experiment_or_hex_microphone() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    rt.state.settings.experimental.hex_microphone = false;
    rt.state.platform.hex_microphone = None;
    refresh(&mut rt);
    assert!(rt.find("nav-voice-filter").is_none());
    assert!(rt.find("nav-experiments").is_none(), "no empty heading");
    rt.state.platform.voice_filter = true;
    refresh(&mut rt);
    assert!(rt.find("nav-voice-filter").is_some());
}

#[test]
fn hud_lab_drives_the_indicator() {
    let mut rt = runtime("mac-hud-lab", true, PANE_SIZE);
    tap(&mut rt, "hud-style2");
    tap(&mut rt, "hud-demo/segment2");
    let Some(PlatformRequest::ConfigureHud(lab)) = asked(&rt).last().cloned() else {
        panic!("{:?}", asked(&rt));
    };
    assert_eq!(lab.tuning, hud_lab::style(2));
    assert_eq!(lab.demo, platform::HudDemo::Processing);
    assert_eq!(lab.queued, 2);
    tap(&mut rt, "copy-hud-preset");
    assert!(
        rt.copy_request
            .as_deref()
            .is_some_and(|preset| preset.starts_with("hud-v1 style=2 lines=3"))
    );
    assert!(rt.state.hud_copied);
}

#[test]
fn hud_lab_tunes_dot_scope_when_its_experiment_is_on() {
    let mut rt = runtime("mac-hud-lab", true, PANE_SIZE);
    assert!(rt.find("dot-columns").is_none(), "the styles without it");
    let mut rt2 = runtime("mac-hud-lab-dot-scope", true, PANE_SIZE);
    assert!(rt2.find("hud-styles").is_none());
    assert!(rt2.find("dot-glow").is_some());
    rt2.state.platform.hud.dot_scope = Some(crate::dot_scope::Tuning {
        glow: 2.0,
        ..Default::default()
    });
    refresh(&mut rt2);
    tap(&mut rt2, "hud-demo/segment1");
    let Some(PlatformRequest::ConfigureHud(lab)) = asked(&rt2).last().cloned() else {
        panic!("{:?}", asked(&rt2));
    };
    assert_eq!(lab.dot_scope.map(|t| t.glow), Some(2.0));
    tap(&mut rt2, "dot-scope-copy");
    assert!(
        rt2.copy_request
            .as_deref()
            .is_some_and(|json| json.contains("\"glow\": 2.0"))
    );
    tap(&mut rt2, "dot-scope-save");
    assert!(matches!(
        asked(&rt2).last(),
        Some(PlatformRequest::SaveDotScope(tuning)) if tuning.glow == 2.0
    ));
    assert!(rt2.state.hud_saved);
    tap(&mut rt2, "dot-scope-reset");
    assert_eq!(
        rt2.state.platform.hud.dot_scope,
        Some(crate::dot_scope::Tuning::default())
    );
    assert!(!rt2.state.hud_saved);
    tap(&mut rt, "hud-style0");
    assert_eq!(rt.state.platform.hud.dot_scope, None);
}

#[test]
fn hud_lab_picks_dot_scope_processing_placement_and_level() {
    use crate::dot_scope::{Placement, Processing};
    let mut rt = runtime("mac-hud-lab-dot-scope", true, PANE_SIZE);
    tap(&mut rt, "dot-processing/segments/segment2");
    tap(&mut rt, "dot-placement/segments/segment2");
    tap(&mut rt, "dot-auto-level");
    tap(&mut rt, "dot-pretend-notch");
    let Some(PlatformRequest::TuneHud(lab)) = asked(&rt).last().cloned() else {
        panic!("{:?}", asked(&rt));
    };
    let tuning = lab.dot_scope.unwrap();
    assert_eq!(tuning.processing, Processing::Pulse);
    assert_eq!(tuning.placement, Placement::Notch);
    assert!(!tuning.auto_level && tuning.pretend_notch);
    assert!(rt.find("dot-noise-floor").is_some());
}

#[test]
fn release_builds_show_hud_lab_only_for_dot_scope() {
    // The scene is a release build (no developer panes) with the experiment on.
    let mut rt = runtime("mac-hud-lab-dot-scope", true, PANE_SIZE);
    assert!(!rt.state.platform.developer);
    assert!(rt.find("nav-hud-lab").is_some());
    assert!(rt.find("nav-activity").is_none());
    let (x, y) = rt.find("dot-spacing/track").expect("a Dot scope slider");
    rt.pointer_down(x - 100.0, y);
    rt.pointer_up(x - 100.0, y);
    settle(&mut rt);
    assert!(
        matches!(asked(&rt).last(), Some(PlatformRequest::TuneHud(_))),
        "a slider tunes the HUD live without restarting its demo: {:?}",
        asked(&rt)
    );
    tap(&mut rt, "nav-general");
    assert!(matches!(
        asked(&rt).last(),
        Some(PlatformRequest::EndHudDemo)
    ));
    let mut rt = runtime("mac-hud-lab-dot-scope", true, PANE_SIZE);
    rt.state.platform.hud.dot_scope = None;
    refresh(&mut rt);
    assert!(rt.find("nav-hud-lab").is_none());
}

#[test]
fn hud_lab_controls_round_trip() {
    use hud_lab::Control;
    let mut tuning = platform::HudTuning::default();
    for (control, _, _) in Control::ALL {
        control.set(&mut tuning, 0.6);
        assert!((control.value(&tuning) - 0.6).abs() < 1e-4, "{control:?}");
    }
}

#[test]
fn activity_lists_events_and_shows_the_raw_one() {
    let mut rt = runtime("mac-activity", true, PANE_SIZE);
    assert!(texts(&rt).iter().any(|t| t == "Context changed to Slack"));
    tap(&mut rt, "activity-event4");
    assert_eq!(rt.state.activity.selected, Some(4));
    assert!(
        texts(&rt)
            .iter()
            .any(|t| t.contains("\"application\":\"Slack\""))
    );
}

#[test]
fn modifier_only_shortcuts_choose_their_side() {
    use crate::app_settings::ModifierSide;
    let mut rt = runtime("mac-settings-sides", true, (980.0, 2400.0));
    tap(&mut rt, "dictation-side/segment2");
    assert_eq!(
        rt.state.settings.dictation_hotkey.modifiers.option,
        Some(ModifierSide::Right)
    );
    // Key chords and Linux have no side choice.
    let rt = runtime("mac-settings", true, (980.0, 2400.0));
    assert!(rt.find("dictation-side").is_none());
    let rt = runtime("portal", true, GOLDEN_SIZE);
    assert!(rt.find("dictation-side").is_none());
}

#[test]
fn a_side_that_would_collide_is_refused() {
    use crate::app_settings::{HotkeyBinding, HotkeyModifiers, ModifierSide};
    let mut ui = fixture(DesktopShortcuts::Registered);
    let option = |side| HotkeyBinding {
        modifiers: HotkeyModifiers {
            option: Some(side),
            ..HotkeyModifiers::default()
        },
        key: None,
    };
    ui.settings.dictation_hotkey = option(ModifierSide::Left);
    ui.settings.paste_last_hotkey = Some(option(ModifierSide::Right));
    assert!(
        platform_settings::side_binding(
            &ui.settings,
            ShortcutTarget::Dictation,
            ModifierSide::Right
        )
        .is_none()
    );
    assert!(
        platform_settings::side_binding(
            &ui.settings,
            ShortcutTarget::Dictation,
            ModifierSide::Left
        )
        .is_some()
    );
}

#[test]
fn macos_27_names_accessibility_as_system_settings_does() {
    let rt = runtime("mac-settings-permissions", true, (980.0, 2400.0));
    assert!(
        texts(&rt)
            .iter()
            .any(|t| t == "Device Control and Data Access")
    );
}

#[test]
fn experiment_goldens() {
    let _goldens = Goldens::collect();
    for dark in [true, false] {
        check_golden("mac-voice-filter", dark, PANE_SIZE);
        check_golden("mac-hud-lab", dark, PANE_SIZE);
        check_golden("mac-hud-lab-dot-scope", dark, PANE_SIZE);
    }
    check_golden("mac-voice-filter-muted", true, PANE_SIZE);
    check_golden("mac-voice-filter-driver-missing", false, PANE_SIZE);
    check_golden("mac-activity", false, PANE_SIZE);
    check_golden("mac-settings-sides", true, PANE_SIZE);
    check_golden("mac-local-build", true, PANE_SIZE);
}

#[test]
fn double_tap_to_lock_folds_away_under_the_rows_below() {
    let mut rt = runtime("mac-settings", true, PANE_SIZE);
    let element = |rt: &Runtime<Ui>, key: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element {key}"))
    };
    let edge = |rt: &Runtime<Ui>| {
        let slot = element(rt, "double-tap-fold").rect;
        slot[1] + slot[3]
    };
    let separator = element(&rt, "transcription").rect[1] - edge(&rt);
    for segment in ["segment1", "segment0"] {
        let (x, y) = rt
            .find(&format!("shortcut-mode-control/{segment}"))
            .unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
            let bottom = edge(&rt);
            let below = element(&rt, "transcription").rect[1];
            assert!(
                (below - bottom - separator).abs() < 0.01,
                "the speech model row rides the slot's edge: {below} vs {bottom}"
            );
            if let Some(row) = rt.elements().into_iter().find(|e| {
                e.key.as_deref() == Some("double-tap") && e.opacity > 0.0 && e.visible[3] > 0.0
            }) {
                let shown = row.visible[1] + row.visible[3];
                assert!(
                    shown <= bottom + 0.25,
                    "the row shows {shown} past its slot's edge {bottom}"
                );
            }
        }
        settle(&mut rt);
    }
}

#[test]
fn ducking_shows_a_persisted_slider_only_on_macos_when_selected() {
    use crate::app_settings::RecordingAudioBehavior;
    let mut rt = runtime("mac-settings", true, (1100.0, 1000.0));
    assert!(rt.find("recording-duck-slider").is_none());
    tap(&mut rt, "while-dictating-control/segment2");
    assert_eq!(
        rt.state.settings.recording_audio_behavior,
        RecordingAudioBehavior::Duck
    );
    let before = rt.state.saved;
    let (x, y) = rt.find("recording-duck-slider/track").unwrap();
    rt.pointer_down(x + 60.0, y);
    assert_eq!(
        rt.state.saved, before,
        "dragging does not persist each movement"
    );
    rt.pointer_up(x + 60.0, y);
    settle(&mut rt);
    assert_eq!(rt.state.saved, before + 1);
    let volume = rt.state.settings.recording_duck_volume;
    assert!(volume > 0.2 && volume <= 1.0);
    tap(&mut rt, "while-dictating-control/segment3");
    assert!(rt.find("recording-duck-slider").is_none());
    tap(&mut rt, "while-dictating-control/segment2");
    assert_eq!(rt.state.settings.recording_duck_volume, volume);
    for scene in ["portal", "windows-settings"] {
        let mut rt = runtime(scene, true, (1100.0, 1000.0));
        if scene == "portal" {
            rt.state.platform.caps = platform::Capabilities::LINUX;
            refresh(&mut rt);
        }
        assert!(rt.find("while-dictating-control/segment3").is_none());
        assert!(rt.find("recording-duck-slider").is_none());
    }
}

#[test]
fn ducking_goldens() {
    let _goldens = Goldens::collect();
    for dark in [true, false] {
        check_golden("mac-settings-ducking", dark, PANE_SIZE);
        check_golden("mac-settings", dark, PANE_SIZE);
    }
    check_golden("mac-settings-permissions", true, PANE_SIZE);
    check_golden("mac-settings-advanced", false, PANE_SIZE);
    check_golden("mac-settings-sides", true, PANE_SIZE);
}
