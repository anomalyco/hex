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
mod fun;
mod general;
mod hero;
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
mod models;
mod model_import;
mod modes;
mod onboarding;
mod onboarding_kit;
mod output;
mod picker;
mod platform;
mod platform_settings;
pub(crate) mod playground;
mod practice;
#[cfg(hex_capture)]
mod scrawl;
mod settings;
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
use settings::Capture;

/// How long typing pauses before a text field's edit is saved.
const DRAFT_DELAY: Duration = Duration::from_millis(250);
/// How often timed state (draft saves, "Saved" badges, the OpenCode retry,
/// the setup beat, permission polling) is checked while the window shows.
const WINDOW_TICK: Duration = Duration::from_millis(250);

static SETTINGS_ICON: Icon = Icon {
    name: "hex-settings",
    view: 24.0,
    body: include_str!("../assets/icons/settings.svg"),
};
static MODES_ICON: Icon = Icon {
    name: "hex-modes",
    view: 24.0,
    body: include_str!("../assets/icons/modes.svg"),
};
static OUTPUT_ICON: Icon = Icon {
    name: "hex-output",
    view: 24.0,
    body: include_str!("../assets/icons/output.svg"),
};
static HISTORY_ICON: Icon = Icon {
    name: "hex-history",
    view: 24.0,
    body: include_str!("../assets/icons/history.svg"),
};
static ACTIVITY_ICON: Icon = Icon {
    name: "hex-activity",
    view: 24.0,
    body: include_str!("../assets/icons/activity.svg"),
};
static HUD_LAB_ICON: Icon = Icon {
    name: "hex-hud-lab",
    view: 24.0,
    body: include_str!("../assets/icons/hud-lab.svg"),
};
static VOICE_FILTER_ICON: Icon = Icon {
    name: "hex-voice-filter",
    view: 24.0,
    body: include_str!("../assets/icons/voice-filter.svg"),
};
static MEETINGS_ICON: Icon = Icon {
    name: "hex-meetings",
    view: 24.0,
    body: include_str!("../assets/icons/meetings.svg"),
};
#[cfg(hex_capture)]
static SCRAWL_ICON: Icon = Icon {
    name: "hex-scrawl",
    view: 24.0,
    body: include_str!("../assets/icons/scrawl.svg"),
};

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
    let mut app = self::app(Some(host.clone())).args(Vec::new()).windowless();
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
    let app = build(connect, PlaygroundMode::Gated);
    #[cfg(target_os = "macos")]
    let app = app.on_open_url(on_open_url);
    app
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
    let app = build(
        Default::default(),
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

/// The window's compositor: the UI, then the welcome hero over the tour's
/// stage and the Sounds sheet's HUD over its stage.
#[derive(Default)]
pub(crate) struct WindowCompositor {
    hero: hero::HeroCompositor,
    sounds: sound_sets::Preview,
}

impl rune::compose::Compositor<Ui> for WindowCompositor {
    fn features(&self) -> wgpu::Features {
        // The Sounds sheet's HUD, and one device shared with the HUD overlay.
        self.hero.features() | crate::rune_hud::Painter::FEATURES
    }

    fn animating(&self) -> bool {
        self.hero.animating()
    }

    fn summary(&mut self, device: &wgpu::Device) -> Option<String> {
        self.hero.summary(device)
    }

    fn render(&mut self, f: &mut rune::compose::Frame<'_, Ui>) {
        self.hero.render(f);
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

fn build(connect: host::Connect, playground: PlaygroundMode) -> rune::App<Ui> {
    let spec = if playground == PlaygroundMode::None {
        spec()
    } else {
        playground::spec(spec())
    };
    let app = rune::App::new(spec)
        .init(move || Ui::load_with(connect))
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
        #[cfg(hex_capture)]
        HotkeySlot::Capture => ShortcutTarget::Capture,
        #[cfg(target_os = "macos")]
        HotkeySlot::SecondDictation => ShortcutTarget::SecondDictation,
        // Labs › Second dictation shortcut is macOS's; elsewhere a saved one
        // meets the dictation shortcut's rules.
        #[cfg(not(target_os = "macos"))]
        HotkeySlot::SecondDictation => ShortcutTarget::Dictation,
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
        init: Ui::load,
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
    /// The service is recording or saving a shortcut.
    editing: bool,
}

/// Every pane, in navigation order. Which ones show depends on the OS's
/// [`platform::Capabilities`] and opt-ins ([`Pane::shown`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Pane {
    /// Dictation, output, and application basics.
    #[default]
    General,
    /// What Hex does to every dictation and in some apps, read-only: the
    /// person's agent edits it.
    Output,
    /// The old per-app editor, kept for its scenes until it is removed.
    Modes,
    /// Recorded calls and their settings, or the opt-in (macOS beta).
    Meetings,
    History,
    /// Annotate (Scrawl's captures inside Hex): the switch, shortcuts,
    /// permission, transcriber, folder, retention, and Scrawl's own app.
    /// Dev builds, macOS and Linux only.
    #[cfg(hex_capture)]
    Annotate,
    /// The Hex Microphone's driver, tuning, and try-it demo (macOS, while
    /// its Labs toggle is on).
    VoiceFilter,
    /// The dictation indicator's tuning scene (Playground only).
    HudLab,
    /// Legacy event-log inspector scene (Playground only).
    Activity,
}

impl Pane {
    #[cfg(all(test, target_os = "linux"))]
    const ALL: &[Pane] = &[
        Pane::General,
        Pane::Output,
        Pane::Modes,
        Pane::Meetings,
        Pane::History,
        #[cfg(hex_capture)]
        Pane::Annotate,
        Pane::VoiceFilter,
        Pane::HudLab,
        Pane::Activity,
    ];

    fn shown(self, ui: &Ui) -> bool {
        let caps = ui.platform.caps;
        match self {
            Pane::Modes | Pane::HudLab | Pane::Activity => false,
            Pane::Meetings => caps.meetings && meetings::available(ui),
            // On from Labs, or kept by a driver apps already use.
            Pane::VoiceFilter => caps.fun && fun::running(ui),
            #[cfg(hex_capture)]
            Pane::Annotate => ui.platform.scrawl.is_some(),
            _ => true,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Pane::General => tr("General"),
            Pane::Output => tr("Output"),
            Pane::Modes => tr("Modes"),
            Pane::History => tr("History"),
            Pane::Meetings => tr("Meetings"),
            #[cfg(hex_capture)]
            Pane::Annotate => tr("Annotate"),
            Pane::VoiceFilter => tr("Voice Filter"),
            Pane::HudLab => "HUD Lab",
            Pane::Activity => tr("Activity"),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Pane::General => "nav-general",
            Pane::Output => "nav-output",
            Pane::Modes => "nav-modes",
            Pane::History => "nav-history",
            Pane::Meetings => "nav-meetings",
            #[cfg(hex_capture)]
            Pane::Annotate => "nav-annotate",
            Pane::VoiceFilter => "nav-voice-filter",
            Pane::HudLab => "nav-hud-lab",
            Pane::Activity => "nav-activity",
        }
    }

    fn icon(self) -> &'static Icon {
        match self {
            Pane::General => &SETTINGS_ICON,
            Pane::Output => &OUTPUT_ICON,
            Pane::Modes => &MODES_ICON,
            Pane::History => &HISTORY_ICON,
            Pane::Meetings => &MEETINGS_ICON,
            #[cfg(hex_capture)]
            Pane::Annotate => &SCRAWL_ICON,
            Pane::VoiceFilter => &VOICE_FILTER_ICON,
            Pane::HudLab => &HUD_LAB_ICON,
            Pane::Activity => &ACTIVITY_ICON,
        }
    }
}

/// The one anchored menu or popover that is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Microphone,
    /// An input source's model in keyboard model switching.
    InputSourceModel(usize),
    /// An input source's language in keyboard model switching.
    InputSourceLanguage(usize),
    /// The microphone under the tour's practice field.
    TourMicrophone,
    /// How long History keeps dictations.
    HistoryRetention,
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
    confirm_release: bool,
    mic_level: general::MicLevel,
    /// The panes whose Advanced fold is open.
    advanced: Vec<Pane>,
    /// The Labs page shows in place of a pane.
    labs_open: bool,
    /// The Labs row a link just opened, pulsing until it is cleared.
    lab_flash: Option<(crate::labs::Feature, Instant)>,
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
}

/// Which of the window's text editors hold unsaved typing.
#[derive(Clone, Copy, Debug)]
struct Drafts {
    modes: bool,
    vocabulary: bool,
}

impl Ui {
    fn load() -> Self {
        Self::load_with(host::Connect::default())
    }

    fn load_with(connect: host::Connect) -> Self {
        let (settings, error) = match AppSettings::load() {
            Ok(settings) => (settings, None),
            Err(error) => (AppSettings::default(), Some(format!("{error:#}"))),
        };
        crate::desktop::i18n::apply(settings.ui_language.as_deref());
        let store = host::history(&connect, settings.history_retention);
        let mut ui = Self::with(settings, Service::Pending(connect));
        ui.error = error;
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
            confirm_release: false,
            mic_level: general::MicLevel::default(),
            advanced: Vec::new(),
            labs_open: false,
            lab_flash: None,
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
            window: None,
            tick_at: 0.0,
        }
    }

    fn live(&self) -> bool {
        matches!(self.service, Service::Live(_))
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
/// sidebar's panes in order; otherwise Escape closes the shortcut explainer,
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
    changed |= labs::poll_flash(ui, now);
    // The OpenCode catalog is read only while the pane that uses it shows.
    changed |= ui.models.poll(
        ui.live()
            && ui.pane == Pane::Modes
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
    let login_unknown = ui.platform.login_item.is_none();
    changed |= settings::poll_microphones(ui);
    // General names the input in use; list again when the OS reports an
    // input change. Windows reports a default change only to an open
    // default stream, so there the list is also refreshed every few seconds.
    if (ui.pane == Pane::General || onboarding::shows_microphone(ui))
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
        _ if ui.labs_open => (Vec::new(), labs::page(ui, p)),
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
            (ui.pane == Pane::Meetings && !ui.labs_open)
                .then(|| meetings::view_switch(ui, p))
                .flatten(),
            actions,
        ))
        .child(body);
    let window = row()
        .key("window")
        .role("window")
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
    let mut tour_layer = Vec::new();
    if let Some(first_run) = onboarding_kit::live::overlay(ui, p, cx.is_reduced_motion()) {
        layer.push(first_run);
    } else if let Some(tour) = onboarding::overlay(ui, p, &mut tour_layer) {
        layer.push(tour);
        layer.extend(tour_layer);
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

/// macOS draws the traffic lights over the sidebar (Rune's window has a
/// transparent, full-size title bar), so the navigation starts below them.
const SIDEBAR_TOP: f32 = if cfg!(target_os = "macos") {
    40.0
} else {
    12.0
};

/// Sidebar rows follow OpenCode Desktop's settings navigation: 28 pt rows
/// with a 4 pt gap, so each row's highlight stays its own.
const NAV_ROW_HEIGHT: f32 = 28.0;
const NAV_ROW_GAP: f32 = 4.0;

/// On macOS the sidebar and the pane header stand in for the title bar the
/// window draws none of: pressing their background moves the window.
fn title_bar(element: El) -> El {
    if cfg!(target_os = "macos") {
        element.window_drag()
    } else {
        element
    }
}

fn sidebar(ui: &Ui, p: Paint) -> El {
    let mut nav = lists::source_list(col().key("nav"));
    for pane in labs::main_panes(ui) {
        nav = nav.child(nav_slot(
            p,
            pane,
            ui.pane == pane && !ui.labs_open,
            NAV_ROW_GAP / 2.0,
            NAV_ROW_GAP / 2.0,
        ));
    }
    nav = nav.children(labs::playground_nav(ui, p));
    nav = nav.child(labs::nav(ui, p));
    let action = match ui.view.as_ref().map(|view| &view.update_status) {
        Some(DesktopUpdateStatus::ReadyToRestart) => Some((
            tr("Restart to update").to_owned(),
            DesktopAction::RestartIntoUpdate,
        )),
        Some(DesktopUpdateStatus::Available) => {
            Some((tr("Download update").to_owned(), DesktopAction::OpenUpdate))
        }
        Some(DesktopUpdateStatus::SwitchAvailable { channel, version }) => Some((
            crate::updater::switch_label(channel, version),
            DesktopAction::ConfirmUpdateSwitch,
        )),
        _ => None,
    };
    // An available update is a state, not the screen's main action.
    let update = action.map(|(label, action)| {
        row()
            .key("sidebar-update-control")
            .gap(8.0)
            .pl(6.0)
            .center_y()
            .child(controls::status_dot(p.accent()))
            .child(controls::button(
                p,
                "sidebar-update",
                &label,
                controls::ButtonKind::Secondary,
                move |ui: &mut Ui, _| ui.send(Request::Desktop(action.clone())),
            ))
    });
    title_bar(col())
        .key("sidebar")
        .role("navigation")
        .w(220.0)
        .h_full()
        .pt(SIDEBAR_TOP - NAV_ROW_GAP / 2.0)
        .pb(12.0)
        .pl(10.0)
        .pr(6.0)
        .child(nav)
        .child(el().key("spacer").grow(1.0))
        .children(update)
        .children(host::QUIT_IN_WINDOW.then(|| quit_item(p)))
        .child(footer(ui, p))
}

/// The version and, under it, a manual check or what the updater is doing.
/// Both lines keep their place in every state, so a check never moves the
/// version, and the status line is one element whose words change in place
/// rather than two that cross-fade over each other. An available update is
/// the Update button above.
fn footer(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let line = |key: &str, words: &str, color| {
        row()
            .key(key)
            .h(FOOTER_LINE)
            .px(6.0)
            .center_y()
            .self_start()
            .radius(p.k.radius.sm)
            .child(
                text(words)
                    .key("label")
                    .font(p.k.ty.small)
                    .color(color)
                    .truncate(),
            )
    };
    let status = ui.view.as_ref().map(|view| &view.update_status);
    let status = match status {
        None
        | Some(
            DesktopUpdateStatus::Unavailable
            | DesktopUpdateStatus::Available
            | DesktopUpdateStatus::ReadyToRestart
            | DesktopUpdateStatus::SwitchAvailable { .. },
        ) => None,
        Some(DesktopUpdateStatus::Current | DesktopUpdateStatus::Failed) => Some(
            line("update-status", tr("Check for updates"), c.text_muted)
                .role("button")
                .label(tr("Check for updates"))
                .hover(c.hover)
                .press(c.pressed)
                .focusable(true)
                .focus_ring(2.0, -2.0, c.border_focus)
                .on_press(|ui: &mut Ui| ui.send(Request::Desktop(DesktopAction::CheckForUpdates))),
        ),
        // The same words, quieter, while it checks: the hourly check and a
        // pressed one read the same and never swap the line's words.
        Some(DesktopUpdateStatus::Checking) => Some(
            line("update-status", tr("Check for updates"), p.faint())
                .label(tr("Checking for updates…")),
        ),
        Some(DesktopUpdateStatus::Managed) => Some(line(
            "update-status",
            tr("Managed by your organization"),
            p.faint(),
        )),
        Some(DesktopUpdateStatus::Downloading) => {
            Some(line("update-status", tr("Downloading update"), p.faint()))
        }
        Some(DesktopUpdateStatus::SourceBuild) => Some(line(
            "update-status",
            if ui.build_preview.unwrap_or_else(build_kind) == BuildKind::Local {
                tr("Local build · updates off")
            } else {
                tr("Updated from your source checkout")
            },
            p.faint(),
        )),
        Some(DesktopUpdateStatus::PackageManager) => Some(line(
            "update-status",
            tr("Updated by your package manager"),
            p.faint(),
        )),
        Some(DesktopUpdateStatus::PackageUpdate { command }) => Some(
            text(command.clone())
                .key("update-command")
                .font(p.k.ty.small)
                .color(p.faint())
                .wrap()
                .px(6.0)
                .py(2.0),
        ),
    };
    col()
        .key("footer")
        .mt(8.0)
        .child(version_line(
            ui,
            p,
            line("version", &version_label(), p.faint()),
        ))
        .children(status)
}

/// A local or test build marks itself beside the version, quietly: an outline
/// pill with a dot, in muted theme tokens.
fn version_line(ui: &Ui, p: Paint, version: El) -> El {
    let badge = match ui.build_preview.unwrap_or_else(build_kind) {
        BuildKind::Release => return version,
        BuildKind::Local => tr("Local"),
        BuildKind::Test => tr("Test"),
    };
    let c = &p.k.c;
    row()
        .key("version-row")
        .gap(2.0)
        .center_y()
        .child(version.key("version"))
        .child(
            row()
                .key("build-badge")
                .h(16.0)
                .px(6.0)
                .gap(4.0)
                .center_y()
                .radius(8.0)
                .border(p.hair.max(0.5), c.border_muted)
                .child(
                    controls::status_dot(c.text_faint)
                        .size(5.0, 5.0)
                        .radius(2.5),
                )
                .child(
                    text(badge)
                        .key("label")
                        .font(p.k.ty.small)
                        .color(c.text_muted),
                ),
        )
}

/// What kind of build this is, for the footer's badge. The scenes preview
/// the others on a release build.
#[derive(Clone, Copy, PartialEq)]
enum BuildKind {
    Release,
    Local,
    Test,
}

fn build_kind() -> BuildKind {
    if crate::app_paths::is_local_build() {
        BuildKind::Local
    } else if option_env!("HEX_TEST_BUILD") == Some("1") {
        BuildKind::Test
    } else {
        BuildKind::Release
    }
}

/// The footer's lines: as tall as each other, so a check moves nothing.
const FOOTER_LINE: f32 = 20.0;

/// Test and local builds also name their commit: main moves past a release without
/// changing the version.
fn version_label() -> String {
    // Goldens render a fixed version: a local build's names its commit.
    let version = if cfg!(test) { "0.0.0" } else { crate::VERSION };
    match option_env!("HEX_COMMIT") {
        Some(commit) => {
            let kind = if crate::app_paths::is_local_build() {
                "local"
            } else {
                "test"
            };
            tr_fill("Version {}", &format!("{version} ({kind} {commit})"))
        }
        None => tr_fill("Version {}", version),
    }
}

/// A sidebar item inside its slot. Slots tile the item column, each
/// reaching halfway into the gaps around its item, so wherever the pointer
/// is in the column, the nearest item shows its hover, and a press there
/// selects it.
fn nav_slot(p: Paint, pane: Pane, on: bool, above: f32, below: f32) -> El {
    col()
        .key(format!("{}-slot", pane.key()))
        .pt(above)
        .pb(below)
        .hover_group()
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx| {
            select_on_down(ui, d, tx, pane)
        })
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
        .child(nav_item(p, pane, on))
}

/// Switch on pointer down, like a native sidebar; the press keeps keyboard
/// activation.
fn select_on_down(ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx, pane: Pane) {
    if d.phase == rune::Phase::Down {
        ui.select_pane(pane, tx);
    }
}

fn nav_item(p: Paint, pane: Pane, on: bool) -> El {
    let c = &p.k.c;
    let mut item = row()
        .key(pane.key())
        .role("button")
        .label(pane.title())
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .press(c.pressed)
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx| {
            select_on_down(ui, d, tx, pane)
        })
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
        // The slot's hover, drawn on the item.
        .child(
            el().key("hover")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .radius(p.k.radius.sm)
                .fill(c.hover)
                .group_opacity(0.0, 1.0),
        )
        .child(icon(pane.icon(), 16.0).key("icon").color(if on {
            c.icon_base
        } else {
            c.icon_muted
        }))
        .child(
            text(pane.title())
                .key("label")
                .font(p.k.ty.ui)
                .color(if on { c.text_base } else { c.text_muted })
                .truncate(),
        );
    if on {
        item = item.fill(c.layer_02);
    }
    lists::source_item(item, on)
}

/// Linux has no tray: the window's way to stop Hex. Closing the window
/// keeps Hex dictating.
fn quit_item(p: Paint) -> El {
    let c = &p.k.c;
    row()
        .key("quit")
        .role("button")
        .label(tr("Quit Hex"))
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .mt(NAV_ROW_GAP)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(|ui: &mut Ui| ui.quit())
        .focusable(true)
        .focus_ring(2.0, -2.0, c.border_focus)
        .child(
            text(tr("Quit Hex"))
                .key("label")
                .font(p.k.ty.ui)
                .color(c.text_muted)
                .truncate(),
        )
}

/// The title bar of the content panel: the title, a view switch that stays
/// beside it, and the pane's actions on the right.
fn pane_header(p: Paint, title: &str, switch: Option<El>, actions: Vec<El>) -> El {
    title_bar(row())
        .key("pane-header")
        .h(52.0)
        .px(METRICS.gutter)
        .gap(8.0)
        .center_y()
        .child(
            text(title)
                .key("pane-title")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .shrink()
                .truncate(),
        )
        .children(switch.map(|switch| switch.ml(8.0)))
        .child(el().key("header-spacer").grow(1.0))
        .children(actions)
}

/// How far a scroll body's top and bottom edges fade out.
const SCROLL_FADE: (f32, f32) = (4.0, 24.0);

/// A vertically scrolling body whose edges fade out. At rest its content
/// sits clear of the fades, so the first and last borders, rings, and fills
/// are never cut; content needing room beside the edges (bordered cards)
/// pads itself horizontally.
fn scroll_body(key: &str, content: El) -> El {
    faded_body(key, content, SCROLL_FADE)
}

/// A [`scroll_body`] with its own top and bottom fades.
fn faded_body(key: &str, content: El, (top, bottom): (f32, f32)) -> El {
    col()
        .key(key)
        .grow(1.0)
        .shrink()
        .clip_fade(0.0, top, 0.0, bottom)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(col().key("inset").pt(top).pb(bottom).child(content))
}

// ---------------------------------------------------------------------------
// Fixtures for snapshots and tests

/// A named starting state for `--scene`.
type Scene = (&'static str, fn(&mut Ui));

const SCENES: &[Scene] = &[
    // The running service's real state, for headless snapshots and scripts
    // (`--snap`): waits for its first snapshot instead of drawing
    // "Connecting…" at virtual time zero.
    ("live", |ui| {
        *ui = Ui::load();
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
        ui.confirm_release = true;
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
    // Japanese saved: the English-only models say they switch the language.
    ("picker-japanese", |ui| {
        *ui = portal_fixture();
        ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
        ui.settings.transcription.language = "ja".into();
        ui.picker.installed = Some(vec![TranscriptionModelId::WhisperLargeV3Turbo]);
        picker::fixture(ui, None);
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
    ("shortcut-explainer", |ui| {
        *ui = mac_fixture();
        ui.settings.double_tap_lock = true;
        explainer::fixture(ui, Some(explainer::SNAPSHOT_MS));
    }),
    ("shortcut-explainer-toggle", |ui| {
        *ui = mac_fixture();
        ui.settings.shortcut_mode = crate::dictation::ShortcutMode::Toggle;
        explainer::fixture(ui, Some(explainer::SNAPSHOT_MS));
    }),
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
    // A three-key chord: Control-Option-Space.
    ("shortcut-explainer-chord", |ui| {
        *ui = mac_fixture();
        ui.settings.double_tap_lock = true;
        let modifiers = &mut ui.settings.dictation_hotkey.modifiers;
        modifiers.control = modifiers.option;
        explainer::fixture(ui, Some(explainer::SNAPSHOT_MS));
    }),
    // Loops, for `--frames` clips.
    ("shortcut-explainer-motion", |ui| {
        *ui = mac_fixture();
        ui.settings.double_tap_lock = true;
        explainer::fixture(ui, None);
    }),
    ("onboarding", |ui| {
        *ui = portal_fixture();
        onboarding::fixture(ui, crate::desktop::onboarding::Step::Welcome, false);
    }),
    ("onboarding-setup", |ui| {
        *ui = portal_fixture();
        onboarding::fixture(ui, crate::desktop::onboarding::Step::Setup, true);
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
    ("mac-onboarding", |ui| {
        mac_tour(ui, onboarding::MacStep::Welcome)
    }),
    ("mac-onboarding-setup", |ui| {
        mac_tour(ui, onboarding::MacStep::Setup)
    }),
    ("mac-onboarding-progress", |ui| {
        mac_tour(ui, onboarding::MacStep::SetupProgress)
    }),
    ("mac-onboarding-stale", |ui| {
        mac_tour(ui, onboarding::MacStep::SetupStale)
    }),
    ("mac-onboarding-denied", |ui| {
        mac_tour(ui, onboarding::MacStep::SetupDenied)
    }),
    ("mac-onboarding-restart", |ui| {
        mac_tour(ui, onboarding::MacStep::SetupRestart)
    }),
    ("mac-onboarding-download-failed", |ui| {
        mac_tour(ui, onboarding::MacStep::SetupDownloadFailed)
    }),
    ("mac-onboarding-try", |ui| {
        mac_tour(ui, onboarding::MacStep::TryIt)
    }),
    ("mac-onboarding-listening", |ui| {
        mac_tour(ui, onboarding::MacStep::TryItListening)
    }),
    ("mac-onboarding-landed", |ui| {
        mac_tour(ui, onboarding::MacStep::TryItLanded)
    }),
    ("mac-onboarding-reopen", |ui| {
        mac_tour(ui, onboarding::MacStep::TryItReopen)
    }),
    ("mac-onboarding-done", |ui| {
        mac_tour(ui, onboarding::MacStep::Done)
    }),
    ("mac-onboarding-mic", |ui| mic_tour(ui, MicScene::Hearing)),
    ("mac-onboarding-mic-open", |ui| mic_tour(ui, MicScene::Open)),
    ("mac-onboarding-mic-silent", |ui| {
        mic_tour(ui, MicScene::Silent)
    }),
    ("mac-onboarding-mic-lid", |ui| {
        mic_tour(ui, MicScene::LidClosed)
    }),
    // The welcome hero animating (`--video`, or a live window), and settled as
    // Reduce Motion shows it.
    ("onboarding-hero", |ui| hero_tour(ui, false)),
    ("onboarding-hero-still", |ui| hero_tour(ui, true)),
];

fn hero_tour(ui: &mut Ui, still: bool) {
    mac_tour(ui, onboarding::MacStep::Welcome);
    if let Some(tour) = &mut ui.tour {
        tour.hero_still = still;
    }
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

fn mac_fixture() -> Ui {
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

fn mac_tour(ui: &mut Ui, step: onboarding::MacStep) {
    *ui = mac_fixture();
    onboarding::mac_fixture(ui, step);
}

/// What the tour's practice microphone shows in a scene.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MicScene {
    /// Recording, and Hex hears the person.
    Hearing,
    /// Its menu of inputs open.
    Open,
    /// Recording, and Hex hears nothing.
    Silent,
    /// The lid closed and no other input.
    LidClosed,
}

fn mic_tour(ui: &mut Ui, scene: MicScene) {
    let recording = matches!(scene, MicScene::Hearing | MicScene::Silent);
    mac_tour(
        ui,
        if recording {
            onboarding::MacStep::TryItListening
        } else {
            onboarding::MacStep::TryIt
        },
    );
    let built_in = "MacBook Pro Microphone".to_owned();
    ui.microphones = Ok(if scene == MicScene::LidClosed {
        crate::audio::InputDevices {
            names: vec![built_in.clone()],
            system_default: None,
            off_with_lid_closed: Some(built_in),
            default_is_off_with_lid_closed: true,
            ..Default::default()
        }
    } else {
        crate::audio::InputDevices {
            names: vec![
                "Kit Pods".into(),
                built_in.clone(),
                "Studio Display Microphone".into(),
            ],
            system_default: Some(built_in),
            ..Default::default()
        }
    });
    if scene == MicScene::Open {
        ui.open = Some(Menu::TourMicrophone);
    }
    let microphone = ui.settings.microphone.clone();
    if let Some(tour) = &mut ui.tour {
        match scene {
            MicScene::Hearing => tour.mic.level = 0.6,
            MicScene::Silent => tour.mic.silent = Some(microphone),
            _ => {}
        }
    }
}

fn portal_fixture() -> Ui {
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
fn fixture(shortcuts: DesktopShortcuts) -> Ui {
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
        editing: false,
    });
    ui
}

#[cfg(all(test, target_os = "linux"))]
mod tests;

#[cfg(test)]
mod offscreen;

#[cfg(test)]
mod keyboard_model_tests;

#[cfg(test)]
mod dmg_keycap;
