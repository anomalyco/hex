use super::*;
use rune::text::TextSystem;
use rune::{Env, Runtime};

fn runtime(scene: &str) -> Runtime<Ui> {
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    SCENES.iter().find(|(name, _)| *name == scene).unwrap().1(&mut state);
    if scene == "portal" {
        state.platform.caps = platform::Capabilities::LINUX;
    }
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = crate::rune_ui::offscreen::runtime(
        state,
        view,
        (980.0, 1720.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
        text,
    );
    rt.settle_all();
    rt.frame(0.0);
    rt
}

fn tap(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.settle_all();
    rt.frame(0.0);
}

#[test]
fn keyboard_model_switching_is_a_labs_toggle_wherever_layouts_are_observable() {
    let mut rt = runtime("mac-settings");
    assert!(!rt.state.settings.keyboard_model_switching.enabled);
    assert!(
        rt.find("keyboard-model-switching").is_none(),
        "no General row"
    );
    tap(&mut rt, "nav-labs");
    tap(&mut rt, "lab-keyboard-model-switching");
    assert!(rt.state.settings.keyboard_model_switching.enabled);
    assert_eq!(rt.state.saved, 1);
    tap(&mut rt, "lab-keyboard-model-switching");
    assert!(!rt.state.settings.keyboard_model_switching.enabled);
    let mut rt = runtime("windows-settings");
    tap(&mut rt, "nav-labs");
    assert!(rt.find("lab-keyboard-model-switching").is_some());
    // Wayland (the portal scene) has no layout protocol.
    let mut rt = runtime("portal");
    rt.state.platform.caps.keyboard_model_switching = false;
    tap(&mut rt, "nav-labs");
    assert!(rt.find("lab-keyboard-model-switching").is_none());
}

#[test]
fn keyboard_model_switching_maps_enabled_sources_to_an_installed_model_and_language() {
    use crate::app_settings::KeyboardModelChoice;
    let mut rt = runtime("mac-settings");
    rt.state.platform.input_sources = vec![
        crate::transcription::keyboard_model_switching::InputSource {
            id: "com.apple.keylayout.Spanish".into(),
            name: "Spanish".into(),
            languages: vec!["es".into()],
        },
    ];
    rt.state.picker.installed = Some(vec![TranscriptionModelId::ParakeetV3]);
    rt.state.advanced = vec![Pane::General];
    rt.state.settings.keyboard_model_switching.enabled = true;
    rt.invalidate();
    rt.settle_all();
    rt.frame(0.0);
    assert!(rt.find("input-source-language0").is_none());
    tap(&mut rt, "input-source-model0-trigger");
    tap(&mut rt, "input-source-model0-item1");
    let mapping = |rt: &Runtime<Ui>| {
        rt.state
            .settings
            .keyboard_model_switching
            .mappings
            .get("com.apple.keylayout.Spanish")
            .cloned()
    };
    // A new mapping starts in the source's own language.
    assert_eq!(
        mapping(&rt),
        Some(KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: "es".into(),
        })
    );
    let german = crate::transcription::models::definition(TranscriptionModelId::ParakeetV3)
        .language_choices()
        .iter()
        .position(|code| *code == "de")
        .unwrap();
    tap(&mut rt, "input-source-language0-trigger");
    tap(&mut rt, &format!("input-source-language0-item{german}"));
    assert_eq!(mapping(&rt).unwrap().language, "de");
    tap(&mut rt, "input-source-model0-trigger");
    tap(&mut rt, "input-source-model0-item0");
    assert_eq!(mapping(&rt), None);
    assert!(rt.find("input-source-language0").is_none());
}

#[test]
fn a_one_language_model_shows_its_language_fixed() {
    let rt = runtime("mac-input-source-models");
    // ABC maps to Parakeet Unified English, which has only English.
    assert!(rt.find("input-source-language0").is_some());
    assert!(rt.find("input-source-language0-trigger").is_none());
    // Russian maps to Parakeet v3, which offers a choice.
    assert!(rt.find("input-source-language1-trigger").is_some());
    // Spanish doesn't switch, so it has no language.
    assert!(rt.find("input-source-language2").is_none());
}
