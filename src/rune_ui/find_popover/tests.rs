use super::*;
use crate::rune_ui::accessibility_tests::runtime;
use rune::{Key, Mods, Runtime};

fn send(rt: &mut Runtime<Ui>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    rt.settle_all();
    rt.frame(1.0 / 60.0);
}

fn focus(rt: &Runtime<Ui>) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.focused)
        .map(|e| e.path)
}

#[test]
fn indexed_settings_focus_their_own_control_without_saving() {
    for scene in ["mac-settings", "windows-settings", "portal", "commands"] {
        let mut rt = runtime(scene);
        let before = rt.state.settings.clone();
        let writes = rt.state.saved;
        for entry in entries(&rt.state) {
            let Link::Setting(_, row, _) = entry.link else {
                continue;
            };
            rt.update(move |ui, tx| reveal(ui, tx, entry.link));
            rt.settle_all();
            rt.frame(0.0);
            let path = focus(&rt).unwrap_or_default();
            assert!(
                path.split('/').any(|part| part == row),
                "{scene}: {row} focused {path}"
            );
            assert!(rt.state.capture == settings::Capture::Idle);
            assert!(rt.state.settings == before);
            assert_eq!(rt.state.saved, writes);
        }
    }
}

#[test]
fn typing_yolo_and_return_only_reveals_its_off_switch() {
    let mut rt = runtime("mac-settings");
    let before = rt.state.settings.clone();
    send(&mut rt, Key::Command('k'));
    send(&mut rt, Key::Text("YOLO".into()));
    assert_eq!(results(&rt.state.find)[0].title, "YOLO mode");
    send(&mut rt, Key::Enter);
    assert!(rt.state.labs_open && !rt.state.find.open);
    assert!(focus(&rt).unwrap_or_default().ends_with("/lab-yolo-mode"));
    assert!(rt.state.settings == before);
    assert_eq!(rt.state.saved, 0);
}

#[test]
fn find_restores_focus_and_respects_local_search() {
    let mut rt = runtime("mac-settings");
    send(&mut rt, Key::Tab);
    let initial = focus(&rt);
    assert!(initial.is_some());
    send(&mut rt, Key::Command('f'));
    assert!(rt.state.find.open);
    send(&mut rt, Key::Escape);
    assert_eq!(focus(&rt), initial);
    rt.update(|ui, tx| labs::reveal(ui, &Default::default(), tx));
    rt.frame(0.0);
    send(&mut rt, Key::Command('f'));
    assert!(rt.state.lab_store.search.focused);
    send(&mut rt, Key::Command('k'));
    assert!(!rt.state.find.open);
}

#[test]
fn an_old_opt_in_is_ignored_without_changing_other_labs() {
    let settings: AppSettings = serde_json::from_value(
        serde_json::json!({"labs": {"settings_search":true,"appearance":true}}),
    )
    .unwrap();
    let labs = &serde_json::to_value(settings).unwrap()["labs"];
    assert_eq!(labs["appearance"], true);
    assert!(labs.get("settings_search").is_none());
}

#[test]
fn recorder_and_modal_keep_their_keys() {
    let mut recording = runtime("mac-settings-recording");
    send(&mut recording, Key::Command('k'));
    assert!(!recording.state.find.open);
    let mut dialog = runtime("mac-settings");
    let (x, y) = dialog.find("shortcut-mode-control").unwrap();
    dialog.pointer_down(x, y);
    dialog.pointer_up(x, y);
    dialog.frame(0.0);
    assert!(dialog.state.is_modal(Modal::Explainer));
    send(&mut dialog, Key::Command('k'));
    assert!(dialog.state.is_modal(Modal::Explainer) && !dialog.state.find.open);
}

#[test]
fn an_unavailable_shortcut_is_not_a_setting_hit() {
    let rt = runtime("mac-settings");
    let second = Link::Setting(Pane::General, "second-dictation-shortcut", false);
    assert!(!entries(&rt.state).iter().any(|e| e.link == second));
    // People can still find the lab that offers it, without enabling it.
    assert!(entries(&rt.state).iter().any(|e| e.link
        == Link::Lab(labs::Lab::Feature(
            crate::labs::Feature::SecondDictationShortcut
        ))));
}

#[cfg(hex_capture)]
#[test]
fn annotate_only_lists_when_the_store_has_its_card() {
    let mut rt = runtime("mac-scrawl");
    let lab = Link::Lab(labs::Lab::Annotate);
    assert!(entries(&rt.state).iter().any(|e| e.link == lab));
    let before = rt.state.settings.clone();
    rt.update(move |ui, tx| reveal(ui, tx, lab));
    rt.frame(0.0);
    assert!(focus(&rt).unwrap_or_default().ends_with("/lab-annotate"));
    assert!(rt.state.settings == before);
    rt.state.platform.scrawl = None;
    assert!(!entries(&rt.state).iter().any(|e| e.link == lab));
}
