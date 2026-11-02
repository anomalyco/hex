//! Fixtures for snapshots and tests: the named starting states `--scene`
//! opens.

use super::*;

/// A named starting state for `--scene`.
pub(super) type Scene = (&'static str, fn(&mut Ui));

pub(super) const SCENES: &[Scene] = &[
    // The running service's real state, for headless snapshots and scripts
    // (`--snap`): waits for its first snapshot instead of drawing
    // "Connecting…" at virtual time zero.
    ("live", |ui| {
        // Reads the saved settings and the running Hex; writes neither.
        let settings = AppSettings::read().unwrap_or_default();
        crate::desktop::i18n::apply(settings.ui_language.as_deref());
        *ui = Ui::with(settings, Service::Fixture(Vec::new()));
        let mut backend = Backend::connect(Default::default(), Arc::new(|| {}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while backend.state().is_none() && backend.error().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
            backend.refresh();
        }
        ui.service = Service::Live(Box::new(backend));
        poll(ui, 0.0, &mut Tx::default());
    }),
    ("portal", |ui| *ui = portal_fixture()),
    ("portal-mouse", |ui| {
        *ui = portal_fixture();
        ui.settings.dictation_hotkey = HotkeyBinding {
            modifiers: Default::default(),
            key: crate::app_settings::HotkeyKey::mouse(3),
        };
        if let Some(view) = &mut ui.view {
            view.listener.shortcuts =
                DesktopShortcuts::Portal(vec![crate::desktop::host::DesktopGrantedShortcut {
                    name: "Dictation".into(),
                    trigger: None,
                    unconfirmed: false,
                }]);
        }
    }),
    ("commands", |ui| *ui = fixture(DesktopShortcuts::Commands)),
    ("disabled", |ui| {
        *ui = portal_fixture();
        if let Some(view) = &mut ui.view {
            view.listener.running = false;
            view.listener.status = "Disabled".into();
        }
    }),
    ("capturing", |ui| {
        *ui = portal_fixture();
        ui.capture = Capture::Listening {
            target: ShortcutTarget::Dictation,
            message: None,
            held: Default::default(),
        };
    }),
    ("chinese", |ui| {
        *ui = portal_fixture();
        ui.settings.ui_language = Some("zh".into());
        crate::desktop::i18n::apply(Some("zh"));
    }),
    ("menus", |ui| {
        *ui = portal_fixture();
        ui.open = Some(Menu::Microphone);
        ui.microphones = Ok(crate::audio::InputDevices {
            names: vec!["Built-in Audio Analog Stereo".into(), "Shure MV7".into()],
            system_default: Some("Shure MV7".into()),
            default_is_hex_microphone: false,
            ..Default::default()
        });
    }),
    ("history", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture();
        ui.pane = Pane::History;
    }),
    ("history-empty", |ui| {
        *ui = portal_fixture();
        ui.history = history::HistoryPane::fixture_empty();
        ui.pane = Pane::History;
    }),
    ("output", |ui| {
        *ui = portal_fixture();
        output::fixture(ui);
        ui.pane = Pane::Output;
    }),
    ("output-open", |ui| {
        *ui = portal_fixture();
        output::fixture(ui);
        ui.output.expanded = Some(0);
        ui.output.prompt_copied = true;
        ui.pane = Pane::Output;
    }),
    ("output-trust", |ui| {
        *ui = portal_fixture();
        output::trust_fixture(ui);
    }),
    ("output-empty", |ui| {
        *ui = portal_fixture();
        ui.pane = Pane::Output;
    }),
    ("modes", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.pane = Pane::Modes;
    }),
    ("modes-opencode-not-running", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.models = models::Models::fixture_not_running();
        ui.settings.dictation_processing.modes[0]
            .post_processing
            .enabled = false;
        ui.pane = Pane::Modes;
    }),
    ("modes-global", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.modes.select(modes::Selection::Default);
        ui.pane = Pane::Modes;
    }),
    ("modes-picker", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.modes.application_picker_open = true;
        ui.pane = Pane::Modes;
    }),
    ("modes-transformation-menu", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
        ui.open = Some(Menu::Transformations);
        ui.pane = Pane::Modes;
    }),
    ("picker", |ui| {
        *ui = portal_fixture();
        picker::fixture(ui, None);
    }),
    ("picker-downloading", |ui| {
        *ui = portal_fixture();
        picker::fixture(ui, Some(TranscriptionModelId::CohereTranscribe));
    }),
    // Labs › Parakeet Ultra: its card's details, credit, and published
    // benchmarks (lists Ultra only where the saved settings turn it on).
    ("picker-parakeet-ultra", |ui| {
        *ui = portal_fixture();
        picker::parakeet_ultra_fixture(ui);
    }),
    // Every model, with the full list's Add model file….
    ("picker-custom-models", |ui| {
        *ui = portal_fixture();
        picker::all_fixture(ui);
    }),
    // Found on this computer: Parakeet v3's exact file and an untested Whisper.
    ("picker-found", |ui| {
        *ui = portal_fixture();
        ui.found.models = found::fixture();
        picker::all_fixture(ui);
    }),
    // Japanese saved: Japanese models compared, SenseVoice downloading.
    ("picker-japanese", |ui| {
        *ui = portal_fixture();
        picker::japanese_fixture(ui, Some(TranscriptionModelId::SenseVoiceSmall));
    }),
    // Every model: the English-only ones fold away and switch the language.
    ("picker-japanese-all", |ui| {
        *ui = portal_fixture();
        picker::japanese_fixture(ui, None);
        (ui.picker.view, ui.picker.others_open) = (picker::View::All, true);
    }),
    // Parakeet chosen over Japanese: Settings names the language change.
    ("language-fallback", |ui| {
        *ui = portal_fixture();
        ui.picker.fallback = Some(picker::LanguageFallback {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            from: "ja".into(),
        });
    }),
    // Whisper's languages open below the model.
    ("language-menu", |ui| {
        *ui = portal_fixture();
        ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
        ui.picker.installed = Some(vec![TranscriptionModelId::WhisperLargeV3Turbo]);
        ui.open = Some(Menu::Pick("transcription-language"));
    }),
    ("shortcut-mode", shortcut_mode::scene_modifier),
    ("shortcut-mode-key", shortcut_mode::scene_key),
    ("shortcut-mode-chord", shortcut_mode::scene_chord),
    ("shortcut-mode-motion", shortcut_mode::scene_motion),
    // The Shortcut row with a modifier-only shortcut, Left Option, beside
    // its side picker (macOS), in each face of the recorder.
    ("shortcut-row", |ui| shortcut_row(ui, Capture::Idle)),
    ("shortcut-row-right", |ui| {
        shortcut_row(ui, Capture::Idle);
        ui.settings.dictation_hotkey.modifiers.option = Some(ModifierSide::Right);
    }),
    // Two sided modifiers have no side picker, so their keycaps name the side.
    ("shortcut-row-sided-chord", |ui| {
        shortcut_row(ui, Capture::Idle);
        ui.settings.dictation_hotkey.modifiers.command = Some(ModifierSide::Right);
    }),
    ("shortcut-row-recording", |ui| {
        shortcut_row(ui, listening(None, HotkeyModifiers::default()));
    }),
    ("shortcut-row-held", |ui| {
        shortcut_row(ui, listening(None, HotkeyModifiers::option_command()));
    }),
    ("shortcut-row-rejected", |ui| {
        let message = crate::desktop::i18n::tr("Add a modifier");
        shortcut_row(ui, listening(Some(message), HotkeyModifiers::default()));
    }),
    ("shortcut-row-saved", |ui| {
        shortcut_row(
            ui,
            Capture::Saved {
                target: ShortcutTarget::Dictation,
                until: None,
            },
        );
    }),
    // The first run on Linux (Windows is the same window): setting up Hex
    // with nothing found, the model step with models found here, the demo,
    // waiting for the model, the practice game, and Hex is ready.
    ("onboarding", |ui| {
        first_run(ui, FirstRun::Fresh, Page::Download)
    }),
    ("onboarding-download-failed", |ui| {
        first_run(ui, FirstRun::Failed, Page::Download)
    }),
    // Continue without a model after it, then Settings with no model.
    ("onboarding-no-model", |ui| {
        first_run(ui, FirstRun::Failed, Page::Download);
        onboarding_kit::live::continue_without_model(ui);
    }),
    ("onboarding-model-found", found::listed),
    ("onboarding-model-found-downloading", found::loading),
    ("onboarding-model-found-recommended", found::present),
    ("onboarding-model-found-back", found::chosen),
    ("onboarding-demo", |ui| {
        first_run(ui, FirstRun::Downloading, Page::Demo)
    }),
    ("onboarding-waiting", |ui| {
        first_run(ui, FirstRun::Downloading, Page::Waiting)
    }),
    // GNOME and KDE on Wayland: why the desktop asks before the first paste.
    ("onboarding-paste", |ui| {
        first_run(ui, FirstRun::PasteAccess, Page::Permissions)
    }),
    ("onboarding-try", |ui| {
        first_run(ui, FirstRun::Ready, Page::Try)
    }),
    ("onboarding-ready", |ui| {
        first_run(ui, FirstRun::Ready, Page::Ready)
    }),
    // Windows with its microphone privacy switch off.
    ("windows-onboarding-microphone", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.platform.permissions.microphone =
            crate::desktop::onboarding::PermissionState::NeedsSettings;
        onboarding::fixture(ui, crate::desktop::onboarding::Step::Setup, true);
        onboarding_kit::live::fixture(ui, Page::Permissions);
    }),
    // macOS and Windows surfaces, from preview data (see `platform`).
    ("mac-settings", |ui| *ui = mac_fixture()),
    // The microphone row following a recorded voice (`--video`), and with
    // the microphone released while idle, when no audio arrives.
    ("mac-settings-mic-level", |ui| {
        *ui = mac_fixture();
        ui.mic_level = general::MicLevel::playing(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/vocabulary/audio/term-opencode-2.wav"
        )));
    }),
    ("mac-settings-mic-released", |ui| {
        *ui = mac_fixture();
        ui.settings.release_microphone_while_idle = true;
        ui.mic_level = general::MicLevel::closed();
    }),
    ("mac-settings-ducking", |ui| {
        *ui = mac_fixture();
        ui.settings.recording_audio_behavior = crate::app_settings::RecordingAudioBehavior::Duck;
    }),
    ("mac-input-source-models", |ui| {
        use crate::app_settings::KeyboardModelChoice;
        *ui = mac_fixture();
        ui.advanced = vec![Pane::General];
        ui.settings.keyboard_model_switching.enabled = true;
        ui.settings.keyboard_model_switching.mappings = [
            (
                "com.apple.keylayout.ABC",
                TranscriptionModelId::ParakeetUnifiedEnglish,
                "en",
            ),
            (
                "com.apple.keylayout.RussianWin",
                TranscriptionModelId::ParakeetV3,
                "ru",
            ),
        ]
        .into_iter()
        .map(|(id, model, language)| {
            (
                id.into(),
                KeyboardModelChoice {
                    model,
                    language: language.into(),
                },
            )
        })
        .collect();
        ui.platform.input_sources = [
            ("com.apple.keylayout.ABC", "ABC", "en"),
            ("com.apple.keylayout.RussianWin", "Russian - PC", "ru"),
            ("com.apple.keylayout.Spanish", "Spanish", "es"),
        ]
        .into_iter()
        .map(
            |(id, name, language)| crate::transcription::keyboard_model_switching::InputSource {
                id: id.into(),
                name: name.into(),
                languages: vec![language.into()],
            },
        )
        .collect();
        ui.picker.installed = Some(vec![
            TranscriptionModelId::ParakeetUnifiedEnglish,
            TranscriptionModelId::ParakeetV3,
        ]);
    }),
    // The sound sets sheet at rest, then Wood's dictation recording.
    ("sound-sets", |ui| {
        *ui = mac_fixture();
        sound_sets::fixture(ui, None);
    }),
    ("sound-sets-playing", |ui| {
        *ui = mac_fixture();
        sound_sets::fixture(
            ui,
            Some((
                crate::feedback::SoundSet::Wood,
                crate::feedback::Audition::Dictation,
                900.0,
            )),
        );
    }),
    ("mac-microphone-missing", |ui| {
        *ui = mac_fixture();
        ui.settings.microphone = Some("Shure MV7".into());
        ui.open = Some(Menu::Microphone);
    }),
    ("mac-microphone-lid-closed", |ui| {
        *ui = mac_fixture();
        if let Ok(devices) = &mut ui.microphones {
            devices.system_default = Some("Studio Display Microphone".into());
            devices.off_with_lid_closed = Some("MacBook Pro Microphone".into());
            devices.default_is_off_with_lid_closed = true;
        }
        ui.open = Some(Menu::Microphone);
    }),
    // System default passes over the Bluetooth default for the built-in mic.
    ("mac-microphone-bluetooth", |ui| {
        *ui = mac_fixture();
        if let Ok(devices) = &mut ui.microphones {
            devices.system_default = Some("MacBook Pro Microphone".into());
            devices.bluetooth_default_passed_over = Some("Kit Pods".into());
        }
        ui.advanced = vec![Pane::General];
    }),
    ("mac-settings-vocabulary", |ui| {
        *ui = mac_fixture();
        vocabulary::fixture(ui, true);
    }),
    ("mac-settings-vocabulary-empty", |ui| {
        *ui = mac_fixture();
        vocabulary::fixture(ui, false);
    }),
    // Cohere ignores the list: the row and the sheet say so.
    ("vocabulary-unused", |ui| {
        *ui = portal_fixture();
        ui.settings.transcription.model = TranscriptionModelId::CohereTranscribe;
        ui.picker.installed = Some(vec![TranscriptionModelId::CohereTranscribe]);
        vocabulary::fixture(ui, true);
    }),
    ("mac-settings-recording", |ui| {
        *ui = mac_fixture();
        let held = crate::app_settings::HotkeyModifiers {
            option: Some(crate::app_settings::ModifierSide::Either),
            command: Some(crate::app_settings::ModifierSide::Either),
            ..Default::default()
        };
        ui.capture = Capture::Listening {
            target: ShortcutTarget::Dictation,
            message: None,
            held,
        };
    }),
    ("mac-settings-permissions", |ui| {
        platform_settings::mac_permissions_fixture(ui, false)
    }),
    ("mac-settings-permissions-stale", |ui| {
        platform_settings::mac_permissions_fixture(ui, true)
    }),
    ("mac-settings-pause-browser", |ui| {
        *ui = mac_fixture();
        ui.settings.recording_audio_behavior =
            crate::app_settings::RecordingAudioBehavior::PauseMedia;
        ui.settings.pause_browser_media = true;
        ui.platform.system_audio = false;
    }),
    ("mac-settings-advanced", |ui| {
        *ui = mac_fixture();
        ui.advanced = vec![Pane::General];
    }),
    ("mac-settings-dev-channel", |ui| {
        *ui = mac_fixture();
        ui.settings.updates.channel = Some(crate::app_settings::UpdateChannel::Dev);
    }),
    // A local build: the badge, the quiet status line, no channel row.
    ("mac-local-build", |ui| {
        *ui = mac_fixture();
        ui.build_preview = Some(BuildKind::Local);
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::SourceBuild;
        }
    }),
    ("mac-test-build", |ui| {
        *ui = mac_fixture();
        ui.build_preview = Some(BuildKind::Test);
    }),
    ("mac-update-ready", |ui| {
        *ui = mac_fixture();
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::ReadyToRestart;
        }
    }),
    ("mac-update-checking", |ui| {
        *ui = mac_fixture();
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::Checking;
        }
    }),
    ("mac-update-downloading", |ui| {
        *ui = mac_fixture();
        if let Some(view) = &mut ui.view {
            view.update_status = DesktopUpdateStatus::Downloading;
        }
    }),
    ("windows-settings", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.paste_with_shift = false;
    }),
    ("meetings", |ui| {
        *ui = mac_fixture();
        ui.pane = Pane::Meetings;
    }),
    ("meetings-settings", |ui| {
        *ui = mac_fixture();
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    // System Audio not yet allowed: tap Allow to wait, again to grant.
    ("meetings-allow", |ui| {
        *ui = mac_fixture();
        ui.platform.system_audio = false;
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-calendar", |ui| {
        *ui = mac_fixture();
        ui.settings.meetings.calendar.connected = true;
        ui.settings.meetings.calendar.excluded_calendars = vec!["birthdays".into()];
        ui.platform.calendar = platform::CalendarView::preview();
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-calendar-denied", |ui| {
        *ui = mac_fixture();
        ui.platform.calendar.access = crate::calendar::CalendarAccess::Denied;
        ui.meetings.view = meetings::View::Settings;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-off", |ui| {
        *ui = mac_fixture();
        ui.settings.meetings.enabled = false;
        ui.pane = Pane::Meetings;
    }),
    ("meetings-recording", |ui| {
        *ui = mac_fixture();
        ui.meetings.select("preview-live".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-actions", |ui| {
        *ui = mac_fixture();
        ui.open = Some(Menu::MeetingActions);
        ui.pane = Pane::Meetings;
    }),
    ("meetings-delete", |ui| {
        *ui = mac_fixture();
        ui.meetings.delete_armed = Some("preview-notes".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-rename", |ui| {
        *ui = mac_fixture();
        ui.meetings.renaming = Some("preview-notes".into());
        ui.meetings.rename = TextEdit::new("Arcade launch plan");
        ui.pane = Pane::Meetings;
    }),
    ("meetings-failed", |ui| {
        *ui = mac_fixture();
        ui.meetings.select("preview-failed".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-chapters", |ui| {
        *ui = mac_fixture();
        ui.meetings = meetings::MeetingsPane::chapters_fixture();
        ui.pane = Pane::Meetings;
    }),
    ("meetings-empty", |ui| {
        *ui = mac_fixture();
        ui.meetings = meetings::MeetingsPane::default();
        ui.pane = Pane::Meetings;
    }),
    #[cfg(hex_capture)]
    ("mac-scrawl-off", |ui| scrawl_fixture(ui, false, |_| {})),
    #[cfg(hex_capture)]
    ("mac-scrawl", |ui| scrawl_fixture(ui, true, |_| {})),
    #[cfg(hex_capture)]
    ("mac-scrawl-running", |ui| {
        scrawl_fixture(ui, true, |view| view.scrawl_running = true)
    }),
    #[cfg(hex_capture)]
    ("mac-scrawl-permission", |ui| {
        scrawl_fixture(ui, true, |view| view.screen_recording = false)
    }),
    #[cfg(hex_capture)]
    ("mac-scrawl-restart", |ui| {
        scrawl_fixture(ui, true, |view| {
            view.screen_recording = false;
            view.screen_needs_restart = true;
        })
    }),
    ("mac-voice-filter", |ui| {
        *ui = mac_fixture();
        ui.pane = Pane::VoiceFilter;
    }),
    ("mac-voice-filter-muted", |ui| {
        *ui = mac_fixture();
        ui.platform.tuning.mode = crate::audio::voice_filter::VoiceFilterMode::Muted;
        ui.platform.demo = Some(fun::preview_demo(
            false,
            Some(crate::audio::hex_microphone::DemoPlaybackTrack::CallHears),
        ));
        ui.pane = Pane::VoiceFilter;
    }),
    ("mac-voice-filter-driver-missing", |ui| {
        *ui = mac_fixture();
        if let Some(driver) = &mut ui.platform.hex_microphone {
            driver.driver_bundle_installed = false;
            driver.driver_loaded = false;
            driver.installed_version = None;
            driver.active_apps.clear();
        }
        ui.platform.demo = Some(fun::preview_demo(true, None));
        ui.pane = Pane::VoiceFilter;
    }),
    ("voice-filter", |ui| {
        *ui = fixture(DesktopShortcuts::Commands);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::LINUX);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.experimental.hex_microphone = true;
        ui.pane = Pane::VoiceFilter;
    }),
    ("voice-filter-unavailable", |ui| {
        *ui = fixture(DesktopShortcuts::Commands);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::LINUX);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        if let Some(source) = &mut ui.platform.hex_microphone {
            source.driver_loaded = false;
            source.action_state = crate::audio::hex_microphone::DriverActionState::Failed(
                "pw-cat was not found; Hex Microphone needs PipeWire".into(),
            );
        }
        ui.settings.experimental.hex_microphone = true;
        ui.pane = Pane::VoiceFilter;
    }),
    ("mac-hud-lab", |ui| {
        *ui = mac_fixture();
        ui.platform.developer = true;
        ui.pane = Pane::HudLab;
    }),
    ("mac-labs", |ui| {
        *ui = mac_fixture();
        #[cfg(hex_capture)]
        {
            ui.platform.scrawl = Some(platform::ScrawlView::preview());
        }
        ui.labs_open = true;
    }),
    ("mac-labs-link", |ui| {
        *ui = mac_fixture();
        ui.labs_open = true;
        // Far ahead, so the pulse holds for the render.
        ui.lab_flash = Some((
            crate::labs::Feature::VocabularyBiasing,
            Instant::now() + Duration::from_secs(3600),
        ));
    }),
    ("mac-production", |ui| {
        *ui = mac_fixture();
        ui.settings.meetings.enabled = false;
    }),
    ("mac-activity", |ui| {
        *ui = mac_fixture();
        ui.platform.developer = true;
        ui.activity = activity::ActivityPane::fixture();
        ui.pane = Pane::Activity;
    }),
    // Toggle's cancel shortcut under the shortcut mode, on its default
    // (Escape), and on Wayland, where the desktop binds it.
    ("toggle-cancel", |ui| {
        *ui = mac_fixture();
        ui.settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle;
    }),
    ("portal-toggle-cancel", |ui| {
        *ui = portal_fixture();
        ui.settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle;
    }),
    ("mac-settings-sides", |ui| {
        *ui = mac_fixture();
        ui.advanced = vec![Pane::General];
        ui.settings.dictation_hotkey = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(crate::app_settings::ModifierSide::Left),
                ..HotkeyModifiers::default()
            },
            key: None,
        };
    }),
    // The first run on macOS: setting up Hex, each permission state, the
    // practice game, and Hex is ready.
    ("mac-onboarding", |ui| {
        mac_first_run(ui, onboarding::MacStep::Setup, Page::Download)
    }),
    ("mac-onboarding-permissions", |ui| {
        mac_first_run(ui, onboarding::MacStep::Setup, Page::Permissions)
    }),
    ("mac-onboarding-denied", |ui| {
        mac_first_run(ui, onboarding::MacStep::SetupDenied, Page::Permissions)
    }),
    ("mac-onboarding-stale", |ui| {
        mac_first_run(ui, onboarding::MacStep::SetupStale, Page::Permissions)
    }),
    ("mac-onboarding-restart", |ui| {
        mac_first_run(ui, onboarding::MacStep::SetupRestart, Page::Permissions)
    }),
    ("mac-onboarding-try", |ui| {
        mac_first_run(ui, onboarding::MacStep::TryIt, Page::Try)
    }),
    ("mac-onboarding-reopen", |ui| {
        mac_first_run(ui, onboarding::MacStep::TryItReopen, Page::Try)
    }),
    ("mac-onboarding-ready", |ui| {
        mac_first_run(ui, onboarding::MacStep::Done, Page::Ready)
    }),
];

/// The tour state a Linux first-run scene starts from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstRun {
    /// Nothing installed; the recommended model just started downloading.
    Fresh,
    /// The download is a third of the way.
    Downloading,
    /// The download stopped.
    Failed,
    /// The model is installed.
    Ready,
    /// The model is installed, and the desktop asks before Hex's first
    /// paste (GNOME and KDE on Wayland).
    PasteAccess,
}

fn first_run(ui: &mut Ui, state: FirstRun, page: Page) {
    *ui = portal_fixture();
    onboarding::fixture(ui, crate::desktop::onboarding::Step::Setup, true);
    match state {
        FirstRun::Fresh => onboarding::set_downloaded(ui, 8),
        FirstRun::Downloading => onboarding::set_downloaded(ui, 31),
        FirstRun::Failed => {
            if let Some(view) = &mut ui.view {
                view.transcription.preparing = None;
                view.transcription.error = Some("The connection was reset.".into());
            }
        }
        FirstRun::Ready | FirstRun::PasteAccess => {
            ui.picker.installed = Some(vec![ui.settings.transcription.model]);
            if let Some(view) = &mut ui.view {
                view.transcription.preparing = None;
            }
        }
    }
    if state == FirstRun::PasteAccess {
        ui.platform.paste = Some(paste_access::PasteAccess::Ask);
    }
    onboarding::poll(ui);
    onboarding_kit::live::fixture(ui, page);
}

fn mac_first_run(ui: &mut Ui, step: onboarding::MacStep, page: Page) {
    *ui = mac_fixture();
    onboarding::mac_fixture(ui, step);
    onboarding_kit::live::fixture(ui, page);
}

/// The Annotate pane with captures `enabled`, a 3-day video retention, and
/// `adjust` applied to what the host shows.
#[cfg(hex_capture)]
fn scrawl_fixture(ui: &mut Ui, enabled: bool, adjust: fn(&mut platform::ScrawlView)) {
    *ui = mac_fixture();
    ui.settings.capture.enabled = enabled;
    ui.settings.capture.media_days = Some(3);
    let mut view = platform::ScrawlView::preview();
    adjust(&mut view);
    ui.platform.scrawl = Some(view);
    ui.pane = Pane::Annotate;
}

pub(super) fn mac_fixture() -> Ui {
    let mut ui = fixture(DesktopShortcuts::Registered);
    ui.platform = platform::PlatformState::preview(platform::Capabilities::MACOS);
    ui.host = platform::PlatformHost::Fixture(Vec::new());
    ui.settings.dictation_hotkey = AppSettings::default().dictation_hotkey;
    ui.settings.paste_with_shift = false;
    ui.settings.meetings.enabled = true;
    ui.settings.meetings.providers = vec![
        crate::app_settings::MeetingProviderChoice {
            key: "com.tinyspeck.slackmacgap".into(),
            label: "Slack huddles".into(),
            policy: crate::app_settings::MeetingPolicy::AlwaysRecord,
        },
        crate::app_settings::MeetingProviderChoice {
            key: "problems.kitlangton.com".into(),
            label: "problems.kitlangton.com".into(),
            policy: crate::app_settings::MeetingPolicy::NeverOffer,
        },
    ];
    ui.settings.experimental.hex_microphone = true;
    ui.microphones = Ok(crate::audio::InputDevices {
        names: vec![
            "Kit Pods".into(),
            "MacBook Pro Microphone".into(),
            "Studio Display Microphone".into(),
        ],
        system_default: Some("Kit Pods".into()),
        default_is_hex_microphone: false,
        ..Default::default()
    });
    ui.meetings = meetings::MeetingsPane::fixture();
    ui
}

fn shortcut_row(ui: &mut Ui, capture: Capture) {
    *ui = mac_fixture();
    ui.settings.dictation_hotkey = HotkeyBinding {
        modifiers: HotkeyModifiers {
            option: Some(ModifierSide::Left),
            ..HotkeyModifiers::default()
        },
        key: None,
    };
    ui.capture = capture;
}

fn listening(message: Option<&'static str>, held: HotkeyModifiers) -> Capture {
    Capture::Listening {
        target: ShortcutTarget::Dictation,
        message,
        held,
    }
}

pub(super) fn portal_fixture() -> Ui {
    fixture(DesktopShortcuts::Portal(vec![
        DesktopGrantedShortcut {
            name: "Dictation".into(),
            trigger: Some("F12".into()),
            unconfirmed: false,
        },
        DesktopGrantedShortcut {
            name: "Paste last dictation".into(),
            trigger: Some("Ctrl+Alt+V".into()),
            unconfirmed: false,
        },
    ]))
}

/// Deterministic settings and service state matching a typical Wayland
/// desktop with the shortcuts portal.
pub(super) fn fixture(shortcuts: DesktopShortcuts) -> Ui {
    crate::desktop::i18n::apply(Some("en"));
    let settings = AppSettings {
        dictation_hotkey: HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: Some(HotkeyKey::named("f12")),
        },
        transcription: crate::transcription::models::TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        },
        paste_with_shift: true,
        ..AppSettings::default()
    };
    let mut ui = Ui::with(settings, Service::Fixture(Vec::new()));
    ui.models = models::Models::fixture();
    ui.picker.installed = Some(vec![TranscriptionModelId::ParakeetUnifiedEnglish]);
    ui.view = Some(ServiceView {
        listener: DesktopListenerSnapshot {
            running: true,
            status: "Enabled".into(),
            warning: None,
            shortcuts,
            shortcuts_configurable: false,
        },
        operation_error: None,
        transcription: DesktopTranscriptionSnapshot {
            downloaded_bytes: 0,
            error: None,
            preparation_stage: None,
            selection: ui.settings.transcription.clone(),
            preparing: None,
        },
        update_status: DesktopUpdateStatus::Current,
        update_notes: None,
        editing: false,
    });
    ui
}
