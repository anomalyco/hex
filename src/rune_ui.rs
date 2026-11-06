//! Hex's window on every OS, on Rune.
//!
//! The window talks to a host in the same process: on Linux the dictation
//! host beside it (`linux_host`); on Windows and macOS the runtime thread
//! `windows_rune` or `macos_rune` starts (`windows_host`, `macos_host`).
//! Either way it saves `settings.json` through the host, which adopts it,
//! and sends the same requests for listening, shortcuts, models, and
//! updates. `platform::Capabilities` decides which panes and rows each OS
//! shows. Everything is drawn with `rune-kit`'s components in OpenCode's
//! design language (`rune-theme-opencode`); see `docs/design-system.md`.

mod activity;
#[cfg(hex_capture)]
pub(crate) mod capture_capsule;
mod controls;
#[cfg(target_os = "macos")]
pub(crate) mod deep_link;
mod explainer;
mod found;
mod found_choice;
mod fun;
mod general;
mod history;
#[cfg_attr(target_os = "linux", path = "rune_ui/linux_host.rs")]
#[cfg_attr(target_os = "windows", path = "rune_ui/windows_host.rs")]
#[cfg_attr(target_os = "macos", path = "rune_ui/macos_host.rs")]
mod host;
mod hud_lab;
#[cfg(target_os = "macos")]
mod hud_stories;
mod icons;
mod labs;
mod list_detail;
mod lists;
mod live_notes;
pub(crate) mod live_text;
pub(crate) mod meeting_capsules;
mod meetings;
mod meetings_host;
mod model_details;
mod model_import;
mod models;
mod modes;
mod onboarding;
mod onboarding_kit;
mod output;
mod paste_access;
mod picker;
mod platform;
mod platform_settings;
pub(crate) mod playground;
mod practice;
mod scenes;
#[cfg(hex_capture)]
mod scrawl;
#[cfg(hex_capture)]
pub(crate) mod screenshot_thumbnail;
mod settings;
mod shortcut_mode;
mod sidebar;
mod sidebar_update;
mod sound_sets;
mod spark;
#[cfg(target_os = "macos")]
pub(crate) mod story_link;
mod vocabulary;

use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::{
    Anim, AppSpec, Cx, El, Icon, ScrollSpec, TextEdit, Tx, Waker, col, el, icon, row, text,
};

use crate::app_settings::{AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide};
use crate::desktop::host::{
    DesktopAction, DesktopGrantedShortcut, DesktopListenerSnapshot, DesktopShortcuts,
    DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::desktop::i18n::{tr, tr_fill};
use crate::transcription::models::TranscriptionModelId;
use controls::{METRICS, Paint};
use host::{Backend, Request, ShortcutTarget};
use onboarding_kit::live::Page;
use scenes::{SCENES, Scene, fixture, mac_fixture, portal_fixture};
use settings::Capture;
use sidebar::{BuildKind, Pane, pane_header, sidebar};

/// How long typing pauses before a text field's edit is saved.
const DRAFT_DELAY: Duration = Duration::from_millis(250);
/// How often timed state (draft saves, "Saved" badges, the OpenCode retry,
/// the setup beat, permission polling) is checked while the window shows.
const WINDOW_TICK: Duration = Duration::from_millis(250);

/// Runs the Linux window's app on the main thread until Hex quits: the
/// window side of `linux_app`'s seam. It starts without a window; each
/// `Activate` opens or focuses it with the launcher's activation token, and
/// closing it keeps the app, its state, and the host running.
#[cfg(target_os = "linux")]
pub(crate) fn run_app(
    host: crate::linux::app::HostHandle,
    requests: std::sync::mpsc::Receiver<crate::linux::service::Control>,
    first: Option<crate::linux::service::Activation>,
    shutdown: &'static std::sync::atomic::AtomicBool,
) {
    use crate::linux::service::{Activation, Control};
    rune::set_app_id(crate::linux::portal::APP_ID);
    let mut app = self::app(Some(host.clone()))
        .args(window_args())
        .windowless();
    // Tiling window managers float a window that maps at a fixed size.
    // Desktops that float every window need no such map, and KWin on X11
    // keeps a fixed-size window from fitting a small screen, its title bar
    // out of reach (#736).
    if !crate::linux::session::floats_windows() {
        app = app.float_on_open();
    }
    crate::linux::rune_hud::install(&mut app);
    live_text::install(&mut app, crate::rune_hud::live_text_place);
    // The capture capsule, picker, and ink.
    let _capture_overlays =
        crate::rune_ui::capture_capsule::feed::CaptureOverlays::install(&mut app);
    let meetings = crate::meeting::runtime::Meetings::install(&mut app, shutdown)
        .inspect_err(|error| tracing::error!(%error, "meetings could not start"))
        .ok();
    let handle = app.handle();
    host.set_window_visible({
        let handle = handle.clone();
        Arc::new(move || handle.is_visible())
    });
    let show = move |activation: Activation| match activation.token.or(activation.startup_id) {
        Some(token) => handle.activate(token),
        None => handle.open(),
    };
    if let Some(activation) = first {
        show(activation);
    }
    let quit = app.handle();
    let window = app.handle();
    std::thread::Builder::new()
        .name("hex-window-requests".into())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                match request {
                    Control::Activate(activation) => show(activation),
                    Control::Show(pane) => {
                        show(Activation::default());
                        window.update(move |ui, tx| self::show(ui, destination(pane), tx));
                    }
                    Control::Hide => window.close(),
                    Control::OnboardingReset(answer) => {
                        show(Activation::default());
                        window.update(move |ui, tx| {
                            let _ = answer.send(restart_onboarding(ui, tx));
                        });
                    }
                    Control::Quit => {
                        self::host::close_before_quit(&window);
                        quit.quit();
                        break;
                    }
                }
            }
        })
        .expect("could not start the window request thread");
    app.run();
    // A recording stops, keeping its audio for the next launch.
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    if let Some(meetings) = meetings {
        meetings.join();
    }
}

/// The window as a Rune app whose lifetime the caller chooses, connected to
/// its host through `connect`, with the playground that Labs › Playground
/// opens.
pub(crate) fn app(connect: host::Connect) -> rune::App<Ui> {
    onboarding_kit::set_live_window(true);
    let app = build(move || Ui::load_with(connect), PlaygroundMode::Gated);
    #[cfg(target_os = "macos")]
    let app = app.on_open_url(on_open_url);
    app
}

/// Rune's flags for the window on Linux and Windows: scroll bars show
/// whenever there's more to scroll, as those platforms do, so a pointer
/// without a wheel can reach them (#1209). macOS follows its own "Show scroll
/// bars" setting.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn window_args() -> Vec<String> {
    vec!["--scroll-bars".into(), "always".into()]
}

/// Beside a setting an organization locked (`crate::policy`).
fn managed_note() -> &'static str {
    tr("Managed by your organization")
}

/// A `hex://` link the system opened with Hex: a story link opens the window
/// on that playground story; anything else is ignored.
#[cfg(target_os = "macos")]
fn on_open_url(ui: &mut Ui, link: &str, tx: &mut Tx) {
    if let Some(target) = deep_link::parse(link, story_link::scheme()) {
        match target {
            Ok(target) => deep_link::open(ui, target, tx),
            Err(why) => tracing::warn!(%why, "ignored a link opened with Hex"),
        }
        crate::macos_rune::bring_window_forward();
        return;
    }
    let link = match story_link::parse(link, story_link::scheme()) {
        Ok(link) => link,
        Err(why) => {
            tracing::warn!(%why, "ignored a link opened with Hex");
            return;
        }
    };
    if let Err(why) = story_link::open(&link, tx) {
        tracing::warn!(story = %link.story, %why, "a story link opened only the window");
    }
    crate::macos_rune::bring_window_forward();
}

/// `hex app -- --snap …`: a headless render of a fixture scene. With the
/// Labs on, `--playground [story]` renders the playground.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn render_app(rune_args: Vec<String>) -> rune::App<Ui> {
    let playground = rune_args.iter().any(|arg| arg == "--playground");
    let headless = rune_args
        .iter()
        .any(|arg| arg == "--snap" || arg == "--video" || arg == "--list-scenes");
    onboarding_kit::set_live_window(playground && !headless);
    // Fixtures only: a render never reads, migrates, or saves the user's
    // settings, History, or anything else in the data folder.
    let app = build(
        Ui::fixture_base,
        if playground && playground::enabled() {
            PlaygroundMode::Attached
        } else {
            PlaygroundMode::None
        },
    )
    .args(rune_args);
    if playground && !headless {
        app.handle().update_if(|_, _| {
            crate::feedback::set_enabled(true);
            crate::feedback::set_volume(0.65);
            let _ = crate::feedback::preload();
            false
        });
    }
    app
}

/// The window's compositor: the UI (through the first run's GPU stage or
/// the practice game's strike and burn while they show), then the Sounds
/// sheet's HUD over its stage.
#[derive(Default)]
pub(crate) struct WindowCompositor {
    onboarding: Option<onboarding_kit::gpu_stage::OnboardingGpu>,
    game: Option<onboarding_kit::game_fx::GameFx>,
    sounds: sound_sets::Preview,
    /// The device whose backend `process.log` names.
    logged: Option<rune::compose::DeviceKey>,
}

impl rune::compose::Compositor<Ui> for WindowCompositor {
    fn features(&self) -> wgpu::Features {
        // The Sounds sheet's HUD, and one device shared with the HUD overlay.
        crate::rune_hud::Painter::FEATURES
    }

    fn render(&mut self, f: &mut rune::compose::Frame<'_, Ui>) {
        if self.logged != Some(f.device_key()) {
            self.logged = Some(f.device_key());
            tracing::info!(gpu = %rune::describe_gpu(f.device), "window draws");
        }
        if f.rt
            .marks
            .iter()
            .any(|&m| onboarding_kit::game_fx::is_game_mark(m))
        {
            self.game
                .get_or_insert_with(|| onboarding_kit::game_fx::GameFx::new(f.device))
                .render(f);
        } else if self.onboarding.is_some()
            || f.rt
                .marks
                .iter()
                .any(|&m| onboarding_kit::gpu_stage::is_onboard_mark(m))
        {
            if self
                .onboarding
                .as_ref()
                .is_none_or(|g| !g.matches(f.format))
            {
                self.onboarding = Some(onboarding_kit::gpu_stage::OnboardingGpu::new(
                    f.device, f.format,
                ));
            }
            self.onboarding.as_mut().expect("created above").render(f);
        } else {
            f.draw_ui();
        }
        self.sounds.render(f);
    }
}

/// Whether and how the window has the playground.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaygroundMode {
    None,
    /// With Rune's own ⇧⌘D (headless `--playground` renders).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Attached,
    /// Opened only while Labs › Playground is on (the window).
    Gated,
}

fn build(init: impl FnOnce() -> Ui + 'static, playground: PlaygroundMode) -> rune::App<Ui> {
    let spec = if playground == PlaygroundMode::None {
        spec()
    } else {
        playground::spec(spec())
    };
    let app = rune::App::new(spec)
        .init(init)
        .on_close(on_close)
        .compositor(WindowCompositor::default());
    // Linux records shortcuts from the window's keys, so a modifier pressed
    // alone reaches the recorder, which says Linux needs a key with it.
    #[cfg(target_os = "linux")]
    let app = app.modifier_keys();
    let app = match playground {
        PlaygroundMode::None => app,
        PlaygroundMode::Attached => playground::attach(app),
        PlaygroundMode::Gated => playground::attach_gated(app),
    };
    let window = app.handle();
    // Nothing shown reads the handle, so this asks for no frame. The first
    // frame's poll ran without it, so the window's tick starts from here.
    window.update_if({
        let window = window.clone();
        move |ui, tx| {
            ui.window = Some(window);
            tx.wake_after(WINDOW_TICK);
            false
        }
    });
    app
}

/// Closing the window saves pending text edits and stops a shortcut
/// recording, as leaving a pane does. Hex keeps running.
fn on_close(ui: &mut Ui, _: &mut Tx) {
    ui.flush_drafts();
    settings::cancel_capture(ui);
    ui.open = None;
    models::close(ui);
    picker::close(ui);
    explainer::close(ui);
    sound_sets::close(ui);
    vocabulary::close(ui);
    // HUD Lab's demo would otherwise stay on screen; reopening starts at
    // General, and coming back to HUD Lab starts the demo again.
    if ui.pane == Pane::HudLab {
        ui.ask(platform::PlatformRequest::EndHudDemo);
        ui.pane = Pane::General;
    }
    if let Service::Live(backend) = &mut ui.service {
        backend.closed();
    }
    #[cfg(target_os = "macos")]
    crate::macos_rune::window_closed();
}

/// Where the menu bar, a meeting, or a `hex window show` sends the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Settings,
    /// Settings with the transcription picker open.
    Models,
    /// The Meetings pane, with a meeting selected when given.
    Meetings(Option<String>),
    Modes,
    History,
}

/// Where `hex window show` opens the window. While Meetings is off, its
/// pane shows the opt-in.
#[cfg(not(target_os = "macos"))]
pub(crate) fn destination(pane: Option<crate::control::Pane>) -> Destination {
    use crate::control::Pane as Named;
    match pane {
        None | Some(Named::Settings) => Destination::Settings,
        Some(Named::Models) => Destination::Models,
        Some(Named::Modes) => Destination::Modes,
        Some(Named::History) => Destination::History,
        Some(Named::Meetings) => Destination::Meetings(None),
    }
}

/// Shows `destination` in the window (the caller opens it).
pub(crate) fn show(ui: &mut Ui, destination: Destination, tx: &mut Tx) {
    let pane = match &destination {
        Destination::Settings | Destination::Models => Pane::General,
        Destination::Meetings(_) => Pane::Meetings,
        Destination::Modes => Pane::Output,
        Destination::History => Pane::History,
    };
    picker::close(ui);
    explainer::close(ui);
    sound_sets::close(ui);
    vocabulary::close(ui);
    if pane.shown(ui) {
        ui.select_pane(pane, tx);
    }
    match destination {
        Destination::Models => picker::open(ui),
        Destination::Meetings(Some(id)) => {
            ui.meetings.view = meetings::View::Recordings;
            ui.meetings.select(id);
        }
        _ => {}
    }
}

/// What this OS's Hex can do, by capability name, for `hex status` and the
/// settings `hex settings` can change here.
pub(crate) fn capabilities() -> std::collections::BTreeMap<String, bool> {
    platform::Capabilities::current()
        .fields()
        .into_iter()
        .map(|(name, on)| (name.to_owned(), on))
        .collect()
}

/// Whether this OS's Hex has the capability `name` ([`capabilities`]).
pub(crate) fn capability(name: &str) -> bool {
    platform::Capabilities::current()
        .fields()
        .into_iter()
        .any(|(field, on)| field == name && on)
}

/// `hex onboarding reset`: replays the first run in the window now; the
/// window also shows why it couldn't.
pub(crate) fn restart_onboarding(ui: &mut Ui, tx: &mut Tx) -> Result<(), String> {
    onboarding_kit::live::restart(ui, tx).inspect_err(|why| ui.error = Some(why.clone()))
}

/// The pane the window shows, by its `hex window show` name: General is
/// still `settings` there.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn current_pane(ui: &Ui) -> &'static str {
    match ui.pane {
        Pane::General => "settings",
        pane => pane.key().trim_start_matches("nav-"),
    }
}

/// The OS's rules for a shortcut, the ones Settings applies when recording
/// one; shared with `hex settings set`.
pub(crate) fn shortcut_rule(
    slot: crate::app_settings::HotkeySlot,
    binding: &HotkeyBinding,
) -> Result<(), &'static str> {
    use crate::app_settings::HotkeySlot;
    let target = match slot {
        HotkeySlot::Dictation => ShortcutTarget::Dictation,
        HotkeySlot::PasteLast => ShortcutTarget::PasteLast,
        HotkeySlot::RawDictation => ShortcutTarget::RawDictation,
        HotkeySlot::Cancel => ShortcutTarget::Cancel,
        #[cfg(hex_capture)]
        HotkeySlot::Capture => ShortcutTarget::Capture,
        #[cfg(all(hex_capture, target_os = "macos"))]
        HotkeySlot::CaptureReplay => ShortcutTarget::CaptureReplay,
        #[cfg(all(hex_capture, not(target_os = "macos")))]
        HotkeySlot::CaptureReplay => ShortcutTarget::Capture,
        #[cfg(target_os = "macos")]
        HotkeySlot::SecondDictation => ShortcutTarget::SecondDictation,
        #[cfg(target_os = "macos")]
        HotkeySlot::Screenshot => ShortcutTarget::Screenshot,
        // Labs › Second dictation shortcut and Screenshots are macOS's;
        // elsewhere a saved one meets the dictation shortcut's rules.
        #[cfg(not(target_os = "macos"))]
        HotkeySlot::SecondDictation | HotkeySlot::Screenshot => ShortcutTarget::Dictation,
    };
    host::validate_hotkey(target, binding)
}

/// The key `name` names on the active keyboard layout ("v", "space", "f5").
#[cfg(target_os = "macos")]
pub(crate) fn hotkey_key_named(name: &str) -> Result<HotkeyKey, &'static str> {
    host::hotkey_key_named(name)
}

/// The key `name` names: Linux and Windows bind keys by name, and the
/// shortcut's rule ([`shortcut_rule`]) checks it can register.
#[cfg(not(target_os = "macos"))]
pub(crate) fn hotkey_key_named(name: &str) -> Result<HotkeyKey, &'static str> {
    Ok(HotkeyKey::named(name))
}

/// The OS's shortcut rules for `hex settings set`.
pub(crate) fn shortcut_rules() -> crate::control::settings::ShortcutRules {
    crate::control::settings::ShortcutRules {
        key_named: |name| hotkey_key_named(name).map_err(str::to_owned),
        rule: |slot, binding| shortcut_rule(slot, binding).map_err(str::to_owned),
    }
}

/// A capsule's "Never offer" or "Always record" choice, kept in the window's
/// live settings so its next save doesn't drop it.
pub(crate) fn meeting_provider_chosen(
    ui: &mut Ui,
    choice: crate::app_settings::MeetingProviderChoice,
) {
    crate::app_settings::set_meeting_provider(&mut ui.settings.meetings, choice);
}

/// The corner the recording capsule was dragged to, kept in the window's live
/// settings so its next save doesn't move it back.
pub(crate) fn meeting_corner_moved(
    ui: &mut Ui,
    corner: crate::app_settings::MeetingIndicatorCorner,
) {
    ui.settings.meetings.recording_indicator_corner = corner;
}

/// An error from outside the window, such as settings that could not be read
/// at launch, shown where save errors show.
#[cfg(target_os = "macos")]
pub(crate) fn show_error(ui: &mut Ui, error: String) {
    ui.error = Some(error);
}

/// The menu bar asked for an update check: the footer says how it went.
#[cfg(target_os = "macos")]
pub(crate) fn update_check_asked(ui: &mut Ui) {
    ui.update_asked = true;
}

/// A meeting recording could not start or finish; the Meetings pane says why.
pub(crate) fn meeting_failed(ui: &mut Ui, error: String) {
    ui.meetings.start_pending = false;
    ui.meetings.stop_pending = false;
    ui.meetings.runtime_error = Some(error);
}

/// `hex app -- --snap out.png --scene …`: a headless render of a fixture
/// scene, or `live` for the running Hex's `hex status`.
#[cfg(target_os = "linux")]
pub(crate) fn render() {
    let with_playground =
        std::env::args().any(|arg| arg == "--playground") && playground::enabled();
    if !with_playground {
        rune::run_with(spec(), WindowCompositor::default());
        return;
    }
    let app = rune::App::new(playground::spec(spec())).compositor(WindowCompositor::default());
    playground::attach(app).run();
}

/// The window's logical size, which every pane and the picker fit.
const WINDOW_SIZE: (f32, f32) = (980.0, 760.0);

/// One fixed size the user cannot change, which macOS tiling window managers
/// (AeroSpace, yabai) float: the first-run tour opens centred this way. With
/// the playground, it resizes, so the stage beside its columns has room.
#[cfg(target_os = "macos")]
pub(crate) fn fixed_window() -> rune::WindowOptions {
    if playground::enabled() {
        return rune::WindowOptions::default();
    }
    rune::WindowOptions {
        min_size: Some(WINDOW_SIZE),
        max_size: Some(WINDOW_SIZE),
        resizable: false,
        ..Default::default()
    }
}

fn spec() -> AppSpec<Ui> {
    AppSpec {
        title: "Hex",
        size: WINDOW_SIZE,
        // Narrower windows crowd the sidebar and rows.
        min_size: Some((860.0, 560.0)),
        init: Ui::fixture_base,
        view,
        scenes: SCENES,
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
    }
}

/// Where the window sends its requests.
enum Service {
    /// Not connected yet: `setup` connects, with what the window's host handed
    /// over, unless a scene chose a fixture.
    Pending(host::Connect),
    Live(Box<Backend>),
    /// Snapshots and tests: requests are recorded, never sent.
    Fixture(Vec<Request>),
}

/// The host's latest state.
struct HostState {
    view: ServiceView,
    settings: AppSettings,
}

/// What the window shows of the service, from its latest snapshot.
#[derive(Clone, PartialEq)]
struct ServiceView {
    listener: DesktopListenerSnapshot,
    operation_error: Option<String>,
    transcription: DesktopTranscriptionSnapshot,
    update_status: DesktopUpdateStatus,
    update_notes: Option<crate::desktop::host::UpdateNotes>,
    /// Why this Hex can't update itself, beside Download update (Windows).
    update_blocked: Option<&'static str>,
    /// The service is recording or saving a shortcut.
    editing: bool,
}

/// The one anchored menu or popover that is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Microphone,
    /// An input source's model in keyboard model switching.
    InputSourceModel(usize),
    /// An input source's language in keyboard model switching.
    InputSourceLanguage(usize),
    /// How long History keeps dictations.
    HistoryRetention,
    /// The installed models History's Transcribe again offers.
    HistoryAgain,
    /// The language of Transcribe again's chosen model, for that run.
    HistoryAgainLanguage,
    /// The OpenCode thinking variant.
    Variant,
    /// The selected meeting's overflow menu.
    MeetingActions,
    /// The calendars meeting reminders read.
    Calendars,
    /// The transformations a mode can add (the Modes pane).
    Transformations,
    /// How long annotations are kept (the Annotate pane).
    #[cfg(hex_capture)]
    CaptureRetention,
    /// A settings row's quiet select, by the row's key.
    Pick(&'static str),
    /// A detected call provider's policy, by its row.
    Call(usize),
}

pub(crate) struct Ui {
    settings: AppSettings,
    service: Service,
    /// This window's app, to know whether it shows (`app` sets it).
    window: Option<rune::AppHandle<Ui>>,
    /// The poll clock when the next timed check is due.
    tick_at: f64,
    view: Option<ServiceView>,
    /// A save or policy change that failed here.
    error: Option<String>,
    /// Why the window cannot reach the host.
    connection_error: Option<String>,
    /// The settings the host last reported, to adopt only its changes.
    service_settings: Option<AppSettings>,
    pane: Pane,
    open: Option<Menu>,
    // General
    microphones: Result<crate::audio::InputDevices, String>,
    /// The [`crate::audio::input_route_generation`] `microphones` was listed at.
    microphones_route: Option<u64>,
    /// The poll clock `microphones` was last listed at.
    microphones_listed_at: f64,
    /// When Windows and Linux last listed keyboard input sources; macOS's
    /// native host lists them.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    input_sources_listed_at: f64,
    /// A listing in flight. Listing reads CoreAudio or COM endpoint
    /// properties, which can block, so it runs off the UI thread.
    microphones_listing:
        Option<std::sync::mpsc::Receiver<Result<crate::audio::InputDevices, String>>>,
    mic_level: general::MicLevel,
    /// General's search, while Labs › Settings search is on.
    general_search: TextEdit,
    /// The panes whose Advanced fold is open.
    advanced: Vec<Pane>,
    /// The Labs page shows in place of a pane.
    labs_open: bool,
    /// The Labs store's search, tab, and chip.
    lab_store: labs::Store,
    /// A scene previewing the badge of a build kind this binary is not.
    build_preview: Option<BuildKind>,
    capture: Capture,
    vocabulary: vocabulary::VocabularyEditor,
    /// The model the service was last seen preparing, and since when.
    preparing: Option<(TranscriptionModelId, Instant)>,
    /// When a text field last changed without being saved.
    draft_since: Option<Instant>,
    duck_volume_drag: Option<f32>,
    models: models::Models,
    history: history::HistoryPane,
    modes: modes::ModesPane,
    output: output::OutputPane,
    picker: picker::Picker,
    /// Speech models already on this computer, and choosing one.
    found: found::FoundState,
    explainer: explainer::Explainer,
    sound_sets: sound_sets::SoundSets,
    tour: Option<onboarding::Tour>,
    /// The new first run over the tour (Labs).
    first_run: Option<onboarding_kit::Onboard>,
    /// The OS's capabilities and the OS-only state its panes show.
    platform: platform::PlatformState,
    host: platform::PlatformHost,
    meetings: meetings::MeetingsPane,
    fun: fun::FunPane,
    activity: activity::ActivityPane,
    /// HUD Lab's preset was copied since it last changed.
    hud_copied: bool,
    /// HUD Lab saved Dot scope's tuning, until the next change.
    hud_saved: bool,
    /// Settings saves and Quit Hex a fixture recorded.
    saved: usize,
    quit_requested: bool,
    /// The person asked for an update check (the footer or the menu bar),
    /// so the footer says how it went.
    update_asked: bool,
}

/// Which of the window's text editors hold unsaved typing.
#[derive(Clone, Copy, Debug)]
struct Drafts {
    modes: bool,
    vocabulary: bool,
}

impl Ui {
    /// The state a headless render starts from before its scene: default
    /// settings and a fixture service, built without touching the data
    /// folder. The goldens start from it too.
    fn fixture_base() -> Self {
        Self::with(AppSettings::default(), Service::Fixture(Vec::new()))
    }

    fn load_with(connect: host::Connect) -> Self {
        let (settings, error) = match AppSettings::load() {
            Ok(settings) => (settings, None),
            Err(error) => (AppSettings::default(), Some(format!("{error:#}"))),
        };
        crate::desktop::i18n::apply(settings.ui_language.as_deref());
        let store = host::history(&connect, settings.history_retention);
        #[cfg(target_os = "linux")]
        let paste = connect.is_some().then(paste_access::detect).flatten();
        #[cfg(target_os = "linux")]
        let clipboard_unrestorable =
            connect.is_some() && crate::linux::clipboard::wayland_without_data_control();
        let mut ui = Self::with(settings, Service::Pending(connect));
        ui.error = error;
        #[cfg(target_os = "linux")]
        {
            ui.platform.paste = paste;
            ui.platform.clipboard_unrestorable = clipboard_unrestorable;
        }
        // A mouse-button shortcut went back to its default: say so once.
        if ui.settings.mouse_shortcut_notice {
            ui.settings.mouse_shortcut_notice = false;
            if let Err(error) = ui.settings.save() {
                tracing::warn!(%error, "could not clear the mouse shortcut note");
            }
            ui.error.get_or_insert_with(|| {
                tr("Mouse shortcuts are in Labs now, so your mouse-button shortcut went back to the default. Turn on Labs › Mouse shortcuts to use one again.").to_owned()
            });
        }
        ui.history = history::HistoryPane::shared(store);
        ui.output = output::OutputPane::shuffled();
        ui.modes.load_personal_commands();
        ui.platform.developer =
            crate::dev_build::is_dev_build() && crate::DEVELOPER_FEATURES_ENABLED;
        ui.platform.hud.tuning = crate::rune_hud::dot_scope::lab();
        ui.tour = onboarding::Tour::start(&ui.settings, &ui.platform);
        onboarding_kit::live::open(&mut ui);
        ui.platform.set_login_item(host::launch_at_login());
        ui
    }

    fn with(settings: AppSettings, service: Service) -> Self {
        let modes = modes::ModesPane::new(&settings);
        let vocabulary = vocabulary::VocabularyEditor::new(&settings.vocabulary);
        Self {
            settings,
            service,
            view: None,
            error: None,
            connection_error: None,
            service_settings: None,
            pane: Pane::General,
            open: None,
            microphones: Ok(Default::default()),
            microphones_route: None,
            microphones_listed_at: 0.0,
            input_sources_listed_at: f64::NEG_INFINITY,
            microphones_listing: None,
            mic_level: general::MicLevel::default(),
            general_search: TextEdit::default(),
            advanced: Vec::new(),
            labs_open: false,
            lab_store: labs::Store::default(),
            build_preview: None,
            capture: Capture::Idle,
            vocabulary,
            preparing: None,
            draft_since: None,
            duck_volume_drag: None,
            models: models::Models::default(),
            history: history::HistoryPane::default(),
            modes,
            output: output::OutputPane::default(),
            picker: picker::Picker::default(),
            found: found::FoundState::default(),
            explainer: explainer::Explainer::default(),
            sound_sets: sound_sets::SoundSets::default(),
            tour: None,
            first_run: None,
            platform: platform::PlatformState::new(platform::Capabilities::current()),
            host: platform::PlatformHost::Local(Default::default()),
            meetings: meetings::MeetingsPane::default(),
            fun: fun::FunPane::default(),
            activity: activity::ActivityPane::default(),
            hud_copied: false,
            hud_saved: false,
            saved: 0,
            quit_requested: false,
            update_asked: false,
            window: None,
            tick_at: 0.0,
        }
    }

    fn live(&self) -> bool {
        matches!(self.service, Service::Live(_))
    }

    /// A scene or test: nothing here runs on real time.
    fn is_fixture(&self) -> bool {
        matches!(self.service, Service::Fixture(_))
    }

    fn send(&mut self, request: Request) {
        match &mut self.service {
            Service::Live(client) => {
                if let Err(error) = client.send(request) {
                    self.error = Some(format!("{error:#}"));
                }
            }
            Service::Fixture(sent) => sent.push(request),
            Service::Pending(_) => {}
        }
    }

    /// Saves `settings.json` through the host, which adopts the change. A
    /// failed save leaves the shown settings alone.
    fn update(&mut self, change: impl FnOnce(&mut AppSettings)) -> bool {
        let mut candidate = self.drafted();
        change(&mut candidate);
        self.commit(candidate)
    }

    fn commit(&mut self, candidate: AppSettings) -> bool {
        match &mut self.service {
            Service::Live(backend) => {
                if let Err(error) = backend.save(&candidate) {
                    self.error = Some(format!("{error:#}"));
                    return false;
                }
            }
            Service::Fixture(_) => self.saved += 1,
            Service::Pending(_) => {}
        }
        self.error = None;
        self.settings = candidate;
        self.draft_since = None;
        self.settings_changed();
        true
    }

    /// Meetings turned off forgets its view: turning it back on lists the
    /// recordings.
    fn settings_changed(&mut self) {
        if !meetings::available(self) {
            self.meetings.view = meetings::View::Recordings;
        }
    }

    /// The settings with every text field's current text applied.
    fn drafted(&self) -> AppSettings {
        let mut candidate = self.settings.clone();
        self.modes.project(&mut candidate);
        candidate.vocabulary = self.vocabulary.project();
        candidate
    }

    /// Which editors hold typing the saved settings don't have yet.
    fn drafts(&self) -> Drafts {
        let settings = &self.settings;
        let mut modes = settings.clone();
        self.modes.project(&mut modes);
        Drafts {
            modes: modes.dictation_processing != settings.dictation_processing,
            vocabulary: self.vocabulary.project() != settings.vocabulary,
        }
    }

    /// Saves text-field edits once typing pauses for [`DRAFT_DELAY`].
    fn save_drafts(&mut self, now: Instant) -> bool {
        let candidate = self.drafted();
        if candidate == self.settings {
            self.draft_since = None;
            return false;
        }
        let since = *self.draft_since.get_or_insert(now);
        if now.duration_since(since) < DRAFT_DELAY {
            return false;
        }
        self.commit(candidate)
    }

    /// Saves pending text-field edits now (leaving a pane or a mode).
    fn flush_drafts(&mut self) {
        let candidate = self.drafted();
        if candidate != self.settings {
            self.commit(candidate);
        }
    }

    /// Asks the OS host for something only it can do.
    fn ask(&mut self, request: platform::PlatformRequest) {
        match &request {
            platform::PlatformRequest::Permission(
                crate::desktop::onboarding::PermissionAction::RequestMicrophone,
            ) => self
                .platform
                .note_asked(crate::desktop::onboarding::PermissionKind::Microphone),
            platform::PlatformRequest::Permission(
                crate::desktop::onboarding::PermissionAction::ShowGuide(kind),
            ) => {
                // Previews grant Screen Recording on the second ask, as for
                // system audio.
                #[cfg(hex_capture)]
                if *kind == crate::desktop::onboarding::PermissionKind::ScreenRecording
                    && self.host.is_fixture()
                    && self.platform.asked(*kind)
                    && let Some(view) = self.platform.scrawl.as_mut()
                {
                    view.screen_recording = true;
                }
                self.platform.clear_stale(*kind);
                self.platform.note_asked(*kind);
            }
            platform::PlatformRequest::Permission(
                crate::desktop::onboarding::PermissionAction::ResetAndPrompt(kind),
            ) => {
                self.platform.clear_stale(*kind);
                self.platform.note_asked(*kind);
            }
            platform::PlatformRequest::SystemAudio => {
                let kind = crate::desktop::onboarding::PermissionKind::SystemAudio;
                // Previews grant on the second ask, so a scene can play
                // Allow, waiting, and Allowed in turn.
                if self.host.is_fixture() && self.platform.asked(kind) {
                    self.platform.system_audio = true;
                }
                self.platform.clear_stale(kind);
                self.platform.note_asked(kind);
            }
            #[cfg(hex_capture)]
            platform::PlatformRequest::Scrawl(platform::ScrawlRequest::QuitScrawl) => {
                let fixture = self.host.is_fixture();
                if let Some(view) = self.platform.scrawl.as_mut() {
                    view.quitting_scrawl = true;
                    if fixture {
                        view.scrawl_running = false;
                    }
                }
            }
            platform::PlatformRequest::SetLaunchAtLogin(enabled) => match &self.host {
                // Windows (its Run key) and Linux (XDG autostart) write
                // the login item from the window's own process.
                platform::PlatformHost::Local(_) => {
                    let result = host::set_launch_at_login(*enabled);
                    self.platform.set_login_item(Some(result));
                    return;
                }
                // Previews flip at once.
                platform::PlatformHost::Fixture(_) => {
                    self.platform.set_login_item(Some(Ok(*enabled)));
                }
                // The switch moves now; macOS's answer settles it.
                platform::PlatformHost::Native(_) => {
                    self.platform.request_login_item(*enabled);
                    if let Err(message) = self.host.send(request) {
                        let failure = crate::desktop::login_item::LoginItemFailure {
                            status: None,
                            message,
                        };
                        self.platform.login_item_answered(Err(failure), true);
                    }
                    return;
                }
            },
            _ => {}
        }
        #[cfg(not(target_os = "macos"))]
        if let platform::PlatformRequest::Meeting(meeting) = &request
            && matches!(self.host, platform::PlatformHost::Local(_))
        {
            if let Err(error) = self.send_meeting(meeting.clone()) {
                self.error = Some(error);
            }
            return;
        }
        let is_relaunch = request == platform::PlatformRequest::Relaunch;
        match self.host.send(request) {
            // The frame draws right after its actions, before the next poll,
            // so read the host's answer now (the poll the frame runs anyway,
            // its refresh moved to now by `send`) rather than show stale
            // state for a tick.
            Ok(()) => {
                self.host.poll(&mut self.platform);
            }
            Err(_) if is_relaunch => self.platform.relaunch_failed = true,
            Err(error) => self.error = Some(error),
        }
    }

    /// Sends a meeting request to whoever carries it out: on Linux and
    /// Windows the meetings this window keeps itself, whose host ignores
    /// meeting requests; elsewhere the OS host.
    fn send_meeting(&mut self, request: platform::MeetingRequest) -> Result<(), String> {
        #[cfg(not(target_os = "macos"))]
        if matches!(self.host, platform::PlatformHost::Local(_)) {
            let waker = self.modes.waker.clone();
            let wake = move || {
                if let Some(waker) = &waker {
                    waker();
                }
            };
            return self.meetings.local.send(request, wake);
        }
        self.host.send(platform::PlatformRequest::Meeting(request))
    }

    fn running(&self) -> bool {
        self.view.as_ref().is_some_and(|view| view.listener.running)
    }

    fn shortcuts(&self) -> Option<&DesktopShortcuts> {
        self.view.as_ref().map(|view| &view.listener.shortcuts)
    }

    /// Without a shortcuts portal the desktop's keybindings own every
    /// shortcut, so the window has none to record.
    fn records_shortcuts(&self) -> bool {
        self.shortcuts() != Some(&DesktopShortcuts::Commands)
    }

    /// Adopts settings the host saved, unless they are the stale copy of an
    /// edit this window just made. An editor with typing not yet saved
    /// keeps it; every other editor and setting is adopted beneath, so the
    /// draft's save never writes back what another writer (`hex settings
    /// set`, a capture's saved rule) changed meanwhile.
    fn adopt(&mut self, service: AppSettings) -> bool {
        if self.service_settings.as_ref() == Some(&service) {
            return false;
        }
        let changed = service != self.settings;
        if changed {
            let drafts = self.drafts();
            if !drafts.modes && service.dictation_processing != self.settings.dictation_processing {
                self.modes.reload(&service);
            }
            if !drafts.vocabulary && service.vocabulary != self.settings.vocabulary {
                self.vocabulary.reload(&service.vocabulary);
            }
            self.settings = service.clone();
            self.settings_changed();
        }
        self.service_settings = Some(service);
        changed
    }

    /// Saves pending edits, then lets the host finish its work and exit.
    fn quit(&mut self) {
        self.flush_drafts();
        settings::cancel_capture(self);
        match &mut self.service {
            Service::Live(backend) => backend.quit(),
            Service::Fixture(_) => self.quit_requested = true,
            Service::Pending(_) => {}
        }
    }

    /// Switches panes at once: the new pane replaces the old without a fade,
    /// already at rest (a grant's morph doesn't replay).
    fn select_pane(&mut self, pane: Pane, tx: &mut Tx) {
        let labs = std::mem::take(&mut self.labs_open);
        if self.pane == pane && !labs {
            return;
        }
        tx.settle();
        self.flush_drafts();
        settings::cancel_capture(self);
        self.open = None;
        models::close(self);
        picker::close(self);
        explainer::close(self);
        sound_sets::close(self);
        vocabulary::close(self);
        self.output.prompt_copied = false;
        if self.pane == Pane::HudLab {
            self.ask(platform::PlatformRequest::EndHudDemo);
        }
        self.pane = pane;
        match pane {
            Pane::History => self.history.reload(),
            Pane::HudLab => self.ask(platform::PlatformRequest::ConfigureHud(self.platform.hud)),
            #[cfg(hex_capture)]
            Pane::Annotate => self.ask(platform::PlatformRequest::Scrawl(
                platform::ScrawlRequest::Refresh,
            )),
            _ => {}
        }
    }
}

/// Shortcut recording takes every key; Command-1 through 9 open the
/// sidebar's panes in order; otherwise Escape closes Shortcut mode,
/// the sound sets sheet, the picker, or the open menu.
fn on_key(ui: &mut Ui, event: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if settings::on_key(ui, event, tx) {
        return true;
    }
    // Labs › Playground: ⇧⌘D (Ctrl+Shift+D off macOS) opens the playground.
    if event.key == rune::Key::Command('d') && event.mods.shift && playground::enabled() {
        tx.toggle_playground();
        return true;
    }
    // Ctrl+Q quits Hex on Linux, as Quit Hex in the sidebar does (#808).
    // macOS's menu bar owns ⌘Q; Windows quits from its tray.
    #[cfg(target_os = "linux")]
    if event.key == rune::Key::Command('q') && !event.mods.shift && !event.mods.alt {
        ui.quit();
        return true;
    }
    // ⌘F (Ctrl+F off macOS) searches Labs or General.
    if general::focus_search_key(ui, event) {
        return true;
    }
    if let rune::Key::Command(digit @ '1'..='9') = event.key
        && !event.mods.alt
        && !ui.picker.open
        && !ui.explainer.open
        && !ui.sound_sets.open
        && !ui.vocabulary.open
        && ui.tour.is_none()
    {
        let index = digit as usize - '1' as usize;
        let pane = labs::order(ui).get(index).copied();
        if let Some(pane) = pane {
            ui.select_pane(pane, tx);
        }
        return pane.is_some();
    }
    if event.key != rune::Key::Escape {
        return false;
    }
    if ui.explainer.open {
        explainer::close(ui);
        true
    } else if ui.sound_sets.open {
        sound_sets::close(ui);
        true
    } else if ui.vocabulary.open {
        vocabulary::close(ui);
        true
    } else if ui.picker.open {
        picker::close(ui);
        true
    } else if ui.open.is_some() {
        ui.open = None;
        true
    } else if ui.meetings.renaming.is_some() || ui.meetings.delete_armed.is_some() {
        ui.meetings.renaming = None;
        ui.meetings.delete_armed = None;
        true
    } else {
        false
    }
}

/// Linux and Windows: the listing and a chosen transcript folder, from the
/// meetings the window keeps itself.
#[cfg(not(target_os = "macos"))]
fn local_meetings(ui: &mut Ui) -> bool {
    let mut changed = false;
    if ui.pane == Pane::Meetings
        && meetings::available(ui)
        && let Some(listed) = ui.meetings.local.meetings()
    {
        ui.meetings.adopt(listed);
        changed = true;
    }
    match ui.meetings.local.chosen_folder() {
        Some(Ok(folder)) => {
            ui.platform.meeting_folder_error = None;
            ui.update(|settings| settings.meetings.output_folder = Some(folder));
            changed = true;
        }
        Some(Err(error)) => {
            ui.platform.meeting_folder_error = Some(error);
            changed = true;
        }
        None => {}
    }
    changed
}

fn setup(ui: &mut Ui, waker: Waker) {
    ui.modes.waker = Some(waker.clone());
    ui.modes.icon_cache = Some(rune::ImageCache::new(waker.clone()));
    if let Service::Pending(connect) = &mut ui.service {
        let connect = std::mem::take(connect);
        #[cfg(target_os = "macos")]
        {
            let host_wake: Arc<dyn Fn() + Send + Sync> = {
                let waker = waker.clone();
                Arc::new(move || waker())
            };
            ui.host = platform::PlatformHost::Native(host::native_host(host_wake));
        }
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker());
        ui.service = Service::Live(Box::new(Backend::connect(connect, wake)));
    }
}

/// Runs before every frame: adopts service updates, follows shortcut
/// capture, model preparation, the OpenCode catalog, history, and setup, and
/// saves paused text edits. True when anything shown changed.
fn poll(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    if clock >= ui.tick_at && ui.window.as_ref().is_some_and(rune::AppHandle::is_visible) {
        ui.tick_at = clock + WINDOW_TICK.as_secs_f64();
        tx.wake_after(WINDOW_TICK);
    }
    let now = Instant::now();
    let mut changed = ui.save_drafts(now);
    changed |= paste_access::poll(ui);
    // The OpenCode catalog is read only while a pane that uses it shows:
    // Modes, or Meetings › Settings while OpenCode writes the summaries.
    changed |= ui.models.poll(
        ui.live()
            && (ui.pane == Pane::Modes || platform_settings::summary_model_shown(ui))
            && ui.window.as_ref().is_some_and(rune::AppHandle::is_visible),
    );
    changed |= ui.history.poll(ui.pane == Pane::History);
    if ui.pane == Pane::Output && !ui.labs_open {
        changed |= output::poll(ui, clock, tx);
    }
    // Mode rows show their applications' icons, which the catalog locates.
    if matches!(ui.pane, Pane::Modes | Pane::Output)
        && ui
            .settings
            .dictation_processing
            .modes
            .iter()
            .any(|mode| !mode.applications.is_empty())
    {
        ui.modes.ensure_applications();
    }
    changed |= ui.modes.poll(ui.live());
    changed |= modes::poll_chooser(ui);
    changed |= model_import::poll(ui);
    changed |= found::poll(ui);
    let login_unknown = ui.platform.login_item.is_none();
    changed |= settings::poll_microphones(ui);
    // General names the input in use; list again when the OS reports an
    // input change. Windows reports a default change only to an open
    // default stream, so there the list is also refreshed every few seconds.
    if ui.pane == Pane::General
        && ui.live()
        && ui.window.as_ref().is_some_and(rune::AppHandle::is_visible)
        && (ui.microphones_route != Some(crate::audio::input_route_generation())
            || (cfg!(target_os = "windows") && clock >= ui.microphones_listed_at + 3.0))
    {
        settings::refresh_microphones(ui);
        ui.microphones_listed_at = clock;
    }
    platform_settings::update_shown_permissions(ui);
    changed |= ui.host.poll(&mut ui.platform);
    #[cfg(not(target_os = "macos"))]
    {
        changed |= settings::poll_input_sources(ui, clock);
    }
    // macOS reports the login item moments after launch; its switch replaces
    // "Checking…" at once instead of fading in and reflowing the row.
    if login_unknown && ui.platform.login_item.is_some() {
        tx.instant();
    }
    if ui.pane == Pane::Meetings
        && meetings::available(ui)
        && let Some(listed) = ui.host.meetings()
    {
        ui.meetings.adopt(listed);
        changed = true;
    }
    #[cfg(not(target_os = "macos"))]
    if matches!(ui.host, platform::PlatformHost::Local(_)) {
        changed |= local_meetings(ui);
    }
    changed |= ui
        .activity
        .poll(ui.platform.activity_log.as_ref(), ui.pane == Pane::Activity);
    if let Service::Live(backend) = &mut ui.service {
        backend.refresh();
        let error = backend.error();
        let (view, settings) = match backend.state() {
            Some(state) => (Some(state.view), Some(state.settings)),
            None => (None, None),
        };
        if error != ui.connection_error {
            ui.connection_error = error;
            changed = true;
        }
        if view != ui.view {
            ui.view = view;
            changed = true;
        }
        if let Some(settings) = settings {
            changed |= ui.adopt(settings);
            // A pane the adopted settings hide (the Hex Microphone turned
            // off from `hex settings`) leaves for General.
            if !ui.pane.shown(ui) {
                ui.pane = Pane::General;
                ui.open = None;
                changed = true;
            }
        }
    }
    let preparing = ui
        .view
        .as_ref()
        .and_then(|view| view.transcription.preparing);
    if preparing != ui.preparing.map(|(model, _)| model) {
        ui.preparing = preparing.map(|model| (model, now));
        changed = true;
    }
    changed |= onboarding::poll(ui);
    changed |= onboarding_kit::live::poll(ui, tx);
    changed |= general::poll_level(ui, clock, tx);
    let visible = ui.window.as_ref().is_none_or(rune::AppHandle::is_visible);
    changed |= explainer::poll(ui, clock, visible, tx);
    changed |= sound_sets::poll(ui, clock, visible, tx);
    changed | settings::advance_capture(ui, clock, tx)
}

// ---------------------------------------------------------------------------
// View

fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    // The native title bar takes the window's background, so it follows a
    // system theme switch along with the rest of the window.
    #[cfg(target_os = "windows")]
    crate::windows::rune::match_title_bar(p.k.c.bg_deep);
    // Menus and popovers mount above the page (rune-kit overlays).
    let mut layer = Vec::new();
    let (actions, body) = match ui.pane {
        _ if ui.labs_open => (vec![labs::tabs(ui, p)], labs::page(ui, p)),
        Pane::General => (Vec::new(), general::pane(ui, p, &mut layer)),
        Pane::Output => (Vec::new(), output::pane(ui, p, cx.is_reduced_motion())),
        Pane::Modes => modes::pane(ui, p, &mut layer),
        Pane::History => history::pane(ui, p, &mut layer),
        Pane::Meetings => meetings::pane(ui, p, &mut layer),
        #[cfg(hex_capture)]
        Pane::Annotate => (Vec::new(), scrawl::pane(ui, p, &mut layer)),
        Pane::VoiceFilter => (Vec::new(), fun::pane(ui, p)),
        Pane::HudLab => hud_lab::pane(ui, p),
        Pane::Activity => activity::pane(ui, p),
    };
    let panel = col()
        .key("panel")
        .role("main")
        .grow(1.0)
        .shrink()
        .mt(8.0)
        .mr(8.0)
        .mb(8.0)
        .radius(p.k.radius.lg)
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_weak)
        // Children sit inside the border, so their fills never paint over it.
        .padding(p.hair)
        .clip_soft(0.0)
        .child(pane_header(
            p,
            if ui.labs_open {
                tr("Labs")
            } else {
                ui.pane.title()
            },
            if ui.labs_open {
                Some(labs::search_field(ui, p))
            } else {
                (ui.pane == Pane::Meetings)
                    .then(|| meetings::view_switch(ui, p))
                    .flatten()
            },
            actions,
        ))
        .child(body);
    // The OS window is the accessibility tree's root; this box adds nothing
    // to it.
    let window = row()
        .key("window")
        .role("none")
        .fill(p.k.c.bg_deep)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
        .child(sidebar(ui, p))
        .child(panel);
    if let Some(modal) = picker::modal(ui, p) {
        layer.extend(modal);
    }
    if let Some(modal) = explainer::modal(ui, cx, p) {
        layer.extend(modal);
    }
    if let Some(modal) = sound_sets::modal(ui, p) {
        layer.extend(modal);
    }
    if let Some(modal) = vocabulary::modal(ui, p) {
        layer.extend(modal);
    }
    if let Some(first_run) = onboarding_kit::live::overlay(ui, p, cx.is_reduced_motion()) {
        layer.push(first_run);
    }
    rune_kit::root(window, layer).on_window_focus(window_focused)
}

fn window_focused(ui: &mut Ui, focused: bool, tx: &mut Tx) {
    explainer::window_focused(ui, focused, tx);
    if focused {
        ui.send(Request::Desktop(DesktopAction::WindowFocused));
    }
    // Screen Recording is often switched on in System Settings, so the
    // Annotate pane reads it again as soon as Hex comes back.
    #[cfg(hex_capture)]
    if focused && ui.pane == Pane::Annotate && ui.platform.scrawl.is_some() {
        ui.ask(platform::PlatformRequest::Scrawl(
            platform::ScrawlRequest::Refresh,
        ));
    }
}

/// How far a scroll body's top and bottom edges fade out.
const SCROLL_FADE: (f32, f32) = (4.0, 24.0);

/// A vertically scrolling body whose edges fade out. At rest its content
/// sits clear of the fades, so the first and last borders, rings, and fills
/// are never cut; content needing room beside the edges (bordered cards)
/// pads itself horizontally.
fn scroll_body(k: &rune_kit::Kit, key: &str, content: El) -> El {
    faded_body(k, key, content, SCROLL_FADE)
}

/// A [`scroll_body`] with its own top and bottom fades, and a scroll bar to
/// drag while it overflows (#1209).
fn faded_body(k: &rune_kit::Kit, key: &str, content: El, fades: (f32, f32)) -> El {
    list_detail::faded_body_sticky(k, key, content, fades, false)
}

#[cfg(all(test, target_os = "linux"))]
mod tests;

#[cfg(any(test, hex_capture))]
mod offscreen;

#[cfg(test)]
mod accessibility_tests;

#[cfg(test)]
mod snap_isolation;

#[cfg(test)]
mod keyboard_model_tests;

#[cfg(test)]
mod dmg_keycap;
