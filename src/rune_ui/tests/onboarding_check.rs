//! The onboarding check (`scripts/check-onboarding.sh`, run before every
//! stable release): every onboarding scene against its approved golden in
//! both appearances, and a first run walked through the real tour in a
//! clean data folder. Like every golden, these render on Linux only: the
//! macOS and Windows scenes are the same window drawn from fixtures.

use super::*;
use crate::desktop::onboarding::Step;

/// Every onboarding state a person can see, on macOS and on Linux and
/// Windows. An intentional change refreshes these goldens in the same PR
/// (`HEX_UPDATE_GOLDENS=1`).
pub(super) const SCENES: &[&str] = &[
    // A fresh install: the welcome.
    "onboarding",
    "mac-onboarding",
    // Permissions (macOS): to do, refused, stale entry, waiting for a restart.
    "mac-onboarding-setup",
    "mac-onboarding-denied",
    "mac-onboarding-stale",
    "mac-onboarding-restart",
    // The model step: nothing found, so the recommended model downloads;
    // in progress and failed.
    "onboarding-setup",
    "mac-onboarding-progress",
    "mac-onboarding-download-failed",
    // Models already on this computer; the recommended one among them.
    "onboarding-model-found",
    "onboarding-model-found-recommended",
    // The practice dictation.
    "mac-onboarding-try",
    "mac-onboarding-listening",
    "mac-onboarding-landed",
    "mac-onboarding-mic",
    "mac-onboarding-mic-lid",
    "mac-onboarding-mic-open",
    "mac-onboarding-mic-silent",
    "mac-onboarding-reopen",
    // Done.
    "mac-onboarding-done",
];

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
/// Hugging Face cache): the real settings load, the real decision to open the
/// tour, the real download and verification of the model the tour picks, and
/// every step to Done. Downloads the recommended model, so only
/// scripts/check-onboarding.sh runs it.
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
    ui.tour = Some(
        super::super::onboarding::Tour::start(&ui.settings, &ui.platform)
            .expect("a new install opens onboarding"),
    );
    let mut rt = runtime_for(ui, true, PANE_SIZE);
    let step = |rt: &Runtime<Ui>| rt.state.tour.as_ref().map(|tour| tour.flow.step());
    assert_eq!(step(&rt), Some(Step::Welcome));

    // Setup asks the service for the model the tour picked.
    tap(&mut rt, "tour-continue");
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

    // Setup moves on once the model is ready, then the practice step and Done.
    settle(&mut rt);
    std::thread::sleep(std::time::Duration::from_millis(750));
    settle(&mut rt);
    if step(&rt) == Some(Step::TryIt) {
        tap(&mut rt, "tour-skip");
    }
    assert_eq!(step(&rt), Some(Step::Done));
    assert!(
        rt.state.tour.as_ref().unwrap().flow.view().dictation_ready,
        "Done with dictation ready"
    );
    tap(&mut rt, "tour-continue");
    assert!(rt.state.tour.is_none(), "Start using Hex closes onboarding");
    println!(
        "onboarding walkthrough: Welcome -> Setup -> {} downloaded ({} bytes) -> Done",
        definition.name,
        downloaded.load(std::sync::atomic::Ordering::Relaxed)
    );
}
