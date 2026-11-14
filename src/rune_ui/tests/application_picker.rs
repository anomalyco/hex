//! The application picker Modes and Labs › YOLO mode share: a popover over
//! the cards below its +, closed by a pick, a click outside, or Escape.

use super::*;

fn open(rt: &Runtime<Ui>) -> bool {
    rt.state.modes.application_picker_open
}

/// Opens, then closes it each way, from the + keyed `trigger`; `first` is
/// the first app it offers.
fn dismisses_every_way(
    mut rt: Runtime<Ui>,
    trigger: &str,
    first: &str,
    chosen: impl Fn(&Ui) -> Vec<String>,
) {
    tap(&mut rt, trigger);
    assert!(open(&rt), "{trigger} opens the picker");
    key(&mut rt, Key::Escape, Mods::default());
    settle(&mut rt);
    assert!(!open(&rt), "Escape closes it");

    tap(&mut rt, trigger);
    let picker = element(&rt, "application-picker").rect;
    let (x, y) = (picker[0] + picker[2] + 40.0, picker[1] + picker[3] / 2.0);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(!open(&rt), "a click outside closes it");
    assert!(
        chosen(&rt.state)
            .iter()
            .all(|name| name != "Visual Studio Code")
    );

    tap(&mut rt, trigger);
    tap(&mut rt, "available0");
    assert!(!open(&rt), "picking an app closes it");
    assert!(chosen(&rt.state).iter().any(|name| name == first));
    assert!(rt.find("application-picker").is_none());
}

#[test]
fn the_modes_picker_closes_on_a_pick_a_click_outside_or_escape() {
    dismisses_every_way(
        runtime("modes", true, PANE_SIZE),
        "open-application-picker",
        // Chat already has Firefox and Slack.
        "Visual Studio Code",
        |ui| {
            ui.settings.dictation_processing.modes[0]
                .applications
                .clone()
        },
    );
}

#[test]
fn the_yolo_mode_picker_closes_on_a_pick_a_click_outside_or_escape() {
    let mut rt = runtime("mac-labs-yolo-mode-picker", true, PANE_SIZE);
    rt.state.modes.application_picker_open = false;
    rt.invalidate();
    settle(&mut rt);
    dismisses_every_way(rt, "open-yolo-mode-picker", "Firefox", |ui| {
        ui.settings.labs.yolo_mode_apps.clone()
    });
}

#[test]
fn the_yolo_mode_picker_floats_below_its_button_past_the_card() {
    let rt = runtime("mac-labs-yolo-mode-picker", true, PANE_SIZE);
    let button = element(&rt, "open-yolo-mode-picker").rect;
    let picker = element(&rt, "application-picker").rect;
    let card = element(&rt, "lab-card-yolo-mode").rect;
    assert!(picker[1] >= button[1] + button[3], "it opens below the +");
    assert!(
        picker[1] + picker[3] > card[1] + card[3],
        "it reaches past the card instead of growing it: {picker:?} in {card:?}"
    );
}
