//! The window as screen readers and automation read it: the accessibility
//! tree Rune builds from the panes (`docs/features/accessibility.md`).
//! Every control a person can reach has a name, switches say whether they
//! are on, choices say what they choose and their value, the sidebar says
//! which pane shows, and lists say how long they are.

use rune::accesskit::Action;
use rune::text::TextSystem;
use rune::{Env, Runtime};
use serde_json::Value;

use super::*;

const SIZE: (f32, f32) = (980.0, 760.0);

fn runtime(scene: &str) -> Runtime<Ui> {
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    let (_, apply) = SCENES
        .iter()
        .find(|(name, _)| *name == scene)
        .unwrap_or_else(|| panic!("no scene {scene}"));
    apply(&mut state);
    let env = Env {
        system_dark: true,
        scale: 2.0,
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = offscreen::runtime(state, view, SIZE, env, text);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    rt.settle_all();
    rt.frame(0.0);
    rt
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..90 {
        rt.frame(1.0 / 30.0);
    }
    rt.settle_all();
    rt.frame(0.0);
}

/// Every node of `tree` with the path of keys and roles above it.
fn nodes(tree: &Value) -> Vec<(String, &Value)> {
    fn walk<'a>(node: &'a Value, path: String, out: &mut Vec<(String, &'a Value)>) {
        let step = node["key"].as_str().map_or_else(
            || node["role"].as_str().unwrap_or("?").to_owned(),
            str::to_owned,
        );
        let path = if path.is_empty() {
            step
        } else {
            format!("{path}/{step}")
        };
        out.push((path.clone(), node));
        for child in node["children"].as_array().into_iter().flatten() {
            walk(child, path.clone(), out);
        }
    }
    let mut out = Vec::new();
    walk(tree, String::new(), &mut out);
    out
}

/// The node keyed `key` in `rt`'s accessibility tree.
pub(super) fn node(rt: &Runtime<Ui>, key: &str) -> Value {
    let tree = rt.accessibility_json();
    let all = nodes(&tree);
    all.iter()
        .find(|(_, node)| node["key"].as_str() == Some(key))
        .map(|(_, node)| (*node).clone())
        .unwrap_or_else(|| {
            let paths: Vec<&str> = all.iter().map(|(path, _)| path.as_str()).collect();
            panic!(
                "no accessible element keyed {key}; drawn at {:?}; the tree:\n{}",
                rt.find(key),
                paths.join("\n")
            )
        })
}

/// The accessible value of the element keyed `key`: a choice's chosen item.
pub(super) fn a11y_value(rt: &Runtime<Ui>, key: &str) -> Option<String> {
    node(rt, key)["value"].as_str().map(str::to_owned)
}

fn states(node: &Value) -> Vec<&str> {
    node["states"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn name(node: &Value) -> &str {
    node["name"].as_str().unwrap_or_default().trim()
}

/// Roles a person operates, so assistive technology must say what they do.
const CONTROLS: &[&str] = &[
    "button",
    "default-button",
    "check-box",
    "switch",
    "radio-button",
    "combo-box",
    "text-input",
    "search-input",
    "multiline-text-input",
    "slider",
    "link",
    "menu-item",
    "menu-item-check-box",
    "menu-item-radio",
    "list-box-option",
    "list-item",
    "tab",
];

/// A control with no name is a control a screen reader user can't use, and
/// one automation can't find by what it says.
#[test]
fn every_control_in_every_scene_has_a_name() {
    let mut unnamed = Vec::new();
    for (scene, _) in SCENES.iter().filter(|(name, _)| *name != "live") {
        let rt = runtime(scene);
        let tree = rt.accessibility_json();
        for (path, node) in nodes(&tree) {
            let role = node["role"].as_str().unwrap_or_default();
            let acts = node["actions"]
                .as_array()
                .is_some_and(|actions| actions.iter().any(|a| a == "click" || a == "focus"));
            if (acts || CONTROLS.contains(&role)) && name(node).is_empty() {
                unnamed.push(format!("{scene}: {path} ({role})"));
            }
        }
    }
    assert!(
        unnamed.is_empty(),
        "controls without a name:\n{}",
        unnamed.join("\n")
    );
}

/// The window's tree starts at the pane's title, a heading.
#[test]
fn the_pane_title_is_its_heading() {
    let rt = runtime("history");
    let title = node(&rt, "pane-title");
    assert_eq!(title["role"], "heading");
    assert_eq!(title["level"], 1);
    assert_eq!(name(&title), "History");
}

/// The sidebar is a list whose shown pane is selected, and choosing
/// another through accessibility switches to it, as a click does.
#[test]
fn the_sidebar_says_which_pane_shows_and_switches_through_accessibility() {
    let mut rt = runtime("portal");
    let general = node(&rt, "nav-general");
    assert_eq!(general["role"], "list-item");
    assert_eq!(name(&general), "General");
    assert!(states(&general).contains(&"selected"), "{general}");
    assert!(!states(&node(&rt, "nav-history")).contains(&"selected"));

    assert!(rt.accessibility_action("nav-history", Action::Click, None));
    settle(&mut rt);
    assert_eq!(rt.state.pane, Pane::History);
    assert!(states(&node(&rt, "nav-history")).contains(&"selected"));
    assert!(!states(&node(&rt, "nav-general")).contains(&"selected"));
    assert_eq!(name(&node(&rt, "pane-title")), "History");
}

/// History's list says how many entries it holds and each row its place,
/// its text, and what made it.
#[test]
fn history_lists_its_entries_with_their_place() {
    let rt = runtime("history");
    let list = node(&rt, "history-list");
    assert_eq!(list["role"], "list");
    assert_eq!(name(&list), "History");
    assert_eq!(list["setSize"], 3);
    let first = node(&rt, "history-entry3");
    assert_eq!(first["role"], "list-item");
    assert_eq!(first["position"], 1);
    assert_eq!(
        name(&first),
        "Let's move the standup to ten thirty tomorrow."
    );
    assert!(
        first["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Dictation · WezTerm")),
        "{first}"
    );
    assert!(states(&node(&rt, "history-entry2")).contains(&"selected"));
    assert_eq!(node(&rt, "history-entry1")["position"], 3);
}

/// A settings switch is the whole row: named by its title, described by its
/// note, and on or off; pressing it through accessibility flips it.
#[test]
fn settings_switches_say_whether_they_are_on() {
    let mut rt = runtime("portal");
    let release = rt.state.settings.release_microphone_while_idle;
    let row = node(&rt, "microphone-mode");
    assert_eq!(row["role"], "switch");
    assert_eq!(name(&row), "Turn off the mic when not dictating");
    assert!(row["description"].as_str().is_some_and(|d| !d.is_empty()));
    let on = |row: &Value| states(row).contains(&"checked");
    assert_eq!(on(&row), release);

    assert!(rt.accessibility_action("microphone-mode", Action::Click, None));
    settle(&mut rt);
    assert_eq!(rt.state.settings.release_microphone_while_idle, !release);
    assert_eq!(on(&node(&rt, "microphone-mode")), !release);
}

/// A choice reads as what it chooses, with the current choice as its value.
#[test]
fn choices_name_what_they_choose_and_read_their_value() {
    let rt = runtime("portal");
    let model = node(&rt, "transcription-control");
    assert_eq!(name(&model), "Speech model");
    assert_eq!(
        a11y_value(&rt, "transcription-control").as_deref(),
        Some("Parakeet Unified English")
    );
    let microphone = node(&rt, "microphone-control");
    assert_eq!(name(&microphone), "Microphone");
    assert!(microphone["value"].as_str().is_some_and(|v| !v.is_empty()));
}

/// The model picker is a dialog named by its title, the only part of the
/// window a screen reader reaches while it is open.
#[test]
fn the_model_picker_is_a_named_modal_dialog() {
    let mut rt = runtime("portal");
    assert!(rt.accessibility_action("transcription-control", Action::Click, None));
    settle(&mut rt);
    assert!(rt.state.picker.open);
    let picker = node(&rt, "transcription-picker");
    assert_eq!(picker["role"], "dialog");
    assert!(!name(&picker).is_empty());
    assert!(states(&picker).contains(&"modal"), "{picker}");
    let tree = rt.accessibility_json();
    assert!(
        !nodes(&tree)
            .iter()
            .any(|(_, node)| node["key"] == "nav-general"),
        "the window behind the picker leaves the tree"
    );
}

/// Closing Settings and opening it again keeps the window's runtime but
/// gives it a new accessibility adapter. With assistive technology on, the
/// new adapter's first update must hold the whole tree: a partial one
/// panicked AccessKit, which took the window down on reopen.
#[test]
fn settings_reopened_sends_its_whole_tree_to_the_new_adapter() {
    let mut rt = runtime("portal");
    let mut first = rune::AccessibilityLink::new();
    first.activate();
    let opened = first
        .service(&mut rt, "Hex")
        .expect("the first window's tree");
    assert!(opened.tree.is_some());
    assert!(rt.accessibility_action("transcription-control", Action::Click, None));
    settle(&mut rt);
    assert!(
        first
            .service(&mut rt, "Hex")
            .expect("the picker opened")
            .tree
            .is_none()
    );

    rt.update(on_close);
    settle(&mut rt);
    drop(first);

    rt.update(|ui, tx| show(ui, Destination::Settings, tx));
    settle(&mut rt);
    let mut reopened = rune::AccessibilityLink::new();
    if let Some(update) = reopened.service(&mut rt, "Hex") {
        assert!(
            update.tree.is_some(),
            "a partial tree before the client asked"
        );
    }
    reopened.activate();
    let update = reopened
        .service(&mut rt, "Hex")
        .expect("the reopened window's tree");
    assert!(update.tree.is_some());
    assert!(
        update
            .nodes
            .iter()
            .any(|(_, node)| node.author_id() == Some("transcription-control")),
        "the reopened window shows Settings"
    );
}
