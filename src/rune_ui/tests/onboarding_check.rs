//! The onboarding check (`scripts/check-onboarding.sh`, run before every
//! stable release): every onboarding scene against its approved golden in
//! both appearances, and a first run walked through the real onboarding in
//! a clean data folder. Like every golden, these render on Linux only: the
//! macOS and Windows scenes are the same window drawn from fixtures.

use super::super::onboarding_kit::live::Page;
use super::*;
use crate::desktop::onboarding::Step;

/// Every onboarding state a person can see, on macOS and on Linux and
/// Windows. An intentional change refreshes these goldens in the same PR
/// (`HEX_UPDATE_GOLDENS=1`).
pub(super) const SCENES: &[&str] = &[
    // A fresh install: setting up Hex, the recommended model downloading.
    "onboarding",
    "mac-onboarding",
    // The download stopped.
    "onboarding-download-failed",
    // The model step: models on this computer; the recommended one among
    // them; ours picked and downloading; Back to the list from the next page.
    "onboarding-model-found",
    "onboarding-model-found-recommended",
    "onboarding-model-found-downloading",
    "onboarding-model-found-back",
    // The demo while the model downloads.
    "onboarding-demo",
    // Permissions (macOS): to do, refused, stale entry, waiting for a
    // restart; Windows' microphone privacy switch.
    "mac-onboarding-permissions",
    "mac-onboarding-denied",
    "mac-onboarding-stale",
    "mac-onboarding-restart",
    "windows-onboarding-microphone",
    // GNOME and KDE on Wayland: the desktop's paste permission, explained.
    "onboarding-paste",
    // Almost ready: the model still downloads, and a sketch from "Watch
    // another one" while it does.
    "onboarding-waiting",
    "onboarding-sketch",
    // Your turn: the practice game, and the shortcut that didn't start.
    "onboarding-try",
    "mac-onboarding-try",
    "mac-onboarding-reopen",
    // Hex is ready.
    "onboarding-ready",
    "mac-onboarding-ready",
];

/// Skip on every page: it shows on each scene but Hex is ready, ends
/// onboarding at once, and leaves the model downloading (the recommended
/// one, also when the model step held it).
#[test]
fn onboarding_skip() {
    for scene in SCENES {
        let rt = runtime(scene, true, PANE_SIZE);
        let last = scene.ends_with("-ready");
        assert_eq!(rt.find("skip").is_some(), !last, "{scene}: Skip");
    }
    for scene in [
        "onboarding",
        "onboarding-model-found",
        "mac-onboarding-permissions",
        "onboarding-try",
    ] {
        let mut rt = runtime(scene, true, PANE_SIZE);
        tap(&mut rt, "skip");
        assert!(rt.state.first_run.is_none(), "{scene}: onboarding closed");
        assert!(rt.state.tour.is_none(), "{scene}: and its tour");
        let installed = rt.state.picker.selected_installed(&rt.state.settings);
        assert!(
            installed
                || sent(&rt)
                    .iter()
                    .any(|request| matches!(request, Request::ChooseModel { .. })),
            "{scene}: the model keeps downloading, sent {:?}",
            sent(&rt)
        );
    }
}

#[test]
fn onboarding_goldens() {
    let _goldens = Goldens::collect();
    for scene in SCENES {
        for dark in [true, false] {
            check_golden(scene, dark, PANE_SIZE);
        }
    }
}

/// A first run on Linux or Windows, in the clean data folder
/// `HEX_APPLICATION_SUPPORT_DIR` names (no settings, no models, an empty
/// Hugging Face cache): the real settings load, the real decision to open
/// onboarding, the real download and verification of the model it picks,
/// and every page to Hex is ready. Downloads the recommended model, so only
/// scripts/check-onboarding.sh runs it. The window here is a fixture, not
/// connected to a service: the test does the service's download itself,
/// plays the story on its own clock, and nothing writes the completion
/// marker.
#[test]
#[ignore = "downloads a speech model; run by scripts/check-onboarding.sh"]
fn onboarding_walkthrough() {
    let support = crate::app_paths::support_dir().unwrap();
    assert!(
        std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_some(),
        "set HEX_APPLICATION_SUPPORT_DIR to a clean folder"
    );
    assert!(
        std::fs::read_dir(&support).map_or(true, |mut entries| entries.next().is_none()),
        "{} must start empty",
        support.display()
    );
    assert!(
        crate::transcription::found_models::scan().is_empty(),
        "the Hugging Face cache must be empty"
    );

    // A new install: default settings from an empty folder, nothing installed.
    let settings = AppSettings::load().unwrap();
    let mut ui = portal_fixture();
    ui.platform.caps = platform::Capabilities::LINUX;
    ui.settings = settings;
    let installed = |ui: &Ui| {
        let selection = &ui.settings.transcription;
        crate::transcription::models::is_installed(
            crate::transcription::models::definition(selection.model),
            &selection.language,
        )
    };
    assert!(!installed(&ui), "no model is installed yet");
    ui.picker.installed = Some(Vec::new());
    // As the window opens at launch.
    ui.tour = Some(
        super::super::onboarding::Tour::start(&ui.settings, &ui.platform)
            .expect("a new install opens onboarding"),
    );
    super::super::onboarding_kit::live::open(&mut ui);
    assert!(ui.first_run.is_some(), "the first run shows");
    let mut rt = runtime_for(ui, true, PANE_SIZE);
    let step = |rt: &Runtime<Ui>| rt.state.tour.as_ref().map(|tour| tour.flow.step());
    let shown = texts(&rt);
    assert!(shown.contains(&"Setting up Hex".to_owned()), "{shown:?}");
    assert!(
        !shown.contains(&"Choose a speech model".to_owned()),
        "nothing found, so no model step"
    );

    // Setting up Hex asks the service for the recommended model at once.
    assert_eq!(step(&rt), Some(Step::Setup));
    let (model, language) = match sent(&rt) {
        [Request::ChooseModel { model, language }] => (*model, language.clone()),
        other => panic!("setup should choose a model, sent {other:?}"),
    };
    assert_eq!(model, rt.state.settings.transcription.model);

    // The service's work, for real: download and verify into the clean folder.
    let definition = crate::transcription::models::definition(model);
    let downloaded = std::sync::atomic::AtomicU64::new(0);
    let path = crate::transcription::models::download_with_progress(
        definition,
        &std::sync::atomic::AtomicBool::new(false),
        &downloaded,
    )
    .unwrap_or_else(|error| panic!("downloading {} failed: {error:#}", definition.name));
    assert!(
        path.starts_with(&support),
        "{} is outside the data folder",
        path.display()
    );
    rt.state.settings.transcription = TranscriptionSelection { model, language };
    assert!(installed(&rt.state), "{} is installed", definition.name);
    assert!(
        crate::transcription::models::is_verified(definition),
        "{} is verified against its pin",
        definition.name
    );
    rt.state.picker.installed = Some(vec![model]);

    // The story plays on: the demo, then the practice game ("With Hex")
    // once the model is ready, then Hex is ready.
    settle(&mut rt);
    rt.update(|ui, _| super::super::onboarding_kit::live::fixture_resume(ui, Page::Try));
    settle(&mut rt);
    assert_eq!(step(&rt), Some(Step::TryIt));
    assert!(texts(&rt).contains(&"With Hex".to_owned()));
    assert!(rt.find("skip").is_some(), "Skip shows on the practice too");
    tap(&mut rt, "done");
    assert!(texts(&rt).contains(&"Hex is ready".to_owned()));
    tap(&mut rt, "start-using-hex");
    assert!(rt.state.tour.is_none(), "Start using Hex closes onboarding");
    assert!(rt.state.first_run.is_none());
    println!(
        "onboarding walkthrough: Setting up Hex -> {} downloaded ({} bytes) -> Your turn -> Hex is ready",
        definition.name,
        downloaded.load(std::sync::atomic::Ordering::Relaxed)
    );
}
