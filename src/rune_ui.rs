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
mod commands;
mod controls;
mod fun;
mod history;
#[cfg_attr(target_os = "linux", path = "rune_ui/linux_host.rs")]
#[cfg_attr(target_os = "windows", path = "rune_ui/windows_host.rs")]
#[cfg_attr(target_os = "macos", path = "rune_ui/macos_host.rs")]
mod host;
mod hud_lab;
mod lists;
#[cfg(target_os = "macos")]
pub(crate) mod meeting_capsules;
mod meeting_outline;
mod meetings;
mod models;
mod modes;
mod onboarding;
mod picker;
mod platform;
mod platform_settings;
mod settings;
mod voice_action;

use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::{AppSpec, Cx, El, Icon, ScrollSpec, TextEdit, Tx, Waker, col, el, icon, row, text};

use crate::app_settings::{AppSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers};
use crate::desktop_host::{
    DesktopAction, DesktopGrantedShortcut, DesktopListenerSnapshot, DesktopShortcuts,
    DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::desktop_i18n::{tr, tr_fill};
use crate::transcription_models::TranscriptionModelId;
use controls::Paint;
use host::{Backend, Request, ShortcutTarget};
use settings::Capture;

/// How long typing pauses before a text field's edit is saved.
const DRAFT_DELAY: Duration = Duration::from_millis(250);
/// How often timed state (draft saves, "Saved" badges, the OpenCode retry,
/// the setup beat, permission polling) is checked while the window shows.
const WINDOW_TICK: Duration = Duration::from_millis(250);

static SETTINGS_ICON: Icon = Icon {
    name: "hex-settings",
    view: 16.0,
    body: include_str!("../assets/icons/settings.svg"),
};
static MODES_ICON: Icon = Icon {
    name: "hex-modes",
    view: 16.0,
    body: include_str!("../assets/icons/modes.svg"),
};
static VOICE_ACTION_ICON: Icon = Icon {
    name: "hex-voice-action",
    view: 16.0,
    body: include_str!("../assets/icons/voice-action.svg"),
};
static HISTORY_ICON: Icon = Icon {
    name: "hex-history",
    view: 16.0,
    body: include_str!("../assets/icons/history.svg"),
};
static COMMANDS_ICON: Icon = Icon {
    name: "hex-commands",
    view: 16.0,
    body: include_str!("../assets/icons/commands.svg"),
};
static ACTIVITY_ICON: Icon = Icon {
    name: "hex-activity",
    view: 16.0,
    body: include_str!("../assets/icons/activity.svg"),
};
static HUD_LAB_ICON: Icon = Icon {
    name: "hex-hud-lab",
    view: 16.0,
    body: include_str!("../assets/icons/hud-lab.svg"),
};
static MEETINGS_ICON: Icon = Icon {
    name: "hex-meetings",
    view: 16.0,
    body: include_str!("../assets/icons/meetings.svg"),
};

/// Runs the Linux window's app on the main thread until Hex quits: the
/// window side of `linux_app`'s seam. It starts without a window; each
/// `Activate` opens or focuses it with the launcher's activation token, and
/// closing it keeps the app, its state, and the host running.
#[cfg(target_os = "linux")]
pub(crate) fn run_app(
    host: crate::linux_app::HostHandle,
    requests: std::sync::mpsc::Receiver<crate::linux_service::Control>,
    first: Option<crate::linux_service::Activation>,
) {
    use crate::linux_service::{Activation, Control};
    rune::set_app_id("hex");
    let mut app = self::app(Some(host.clone()))
        .args(Vec::new())
        .windowless()
        .window_options(fixed_window());
    crate::linux_rune_hud::install(&mut app);
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
    std::thread::Builder::new()
        .name("hex-window-requests".into())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                match request {
                    Control::Activate(activation) => show(activation),
                    Control::Quit => {
                        quit.quit();
                        break;
                    }
                }
            }
        })
        .expect("could not start the window request thread");
    app.run();
}

/// The window as a Rune app whose lifetime the caller chooses, connected to
/// its host through `connect`.
pub(crate) fn app(connect: host::Connect) -> rune::App<Ui> {
    let app = rune::App::new(spec())
        .init(move || Ui::load_with(connect))
        .on_close(on_close);
    let window = app.handle();
    window.update({
        let window = window.clone();
        move |ui, _| ui.window = Some(window)
    });
    app
}

/// Closing the window saves pending text edits and stops a shortcut
/// recording, as leaving a pane does. Hex keeps running.
fn on_close(ui: &mut Ui, _: &mut Tx) {
    ui.flush_drafts();
    settings::cancel_capture(ui);
    ui.open = None;
    picker::close(ui);
    if let Service::Live(backend) = &mut ui.service {
        backend.closed();
    }
    #[cfg(target_os = "macos")]
    crate::macos_rune::window_closed();
}

/// Where the menu bar, a meeting, or the local API sends the window.
#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Settings,
    /// Settings with the transcription picker open.
    Models,
    /// The Meetings pane, with a meeting selected when given.
    Meetings(Option<String>),
    Modes,
    Commands,
    VoiceAction,
    History,
}

/// Shows `destination` in the window (the caller opens it).
#[cfg(target_os = "macos")]
pub(crate) fn show(ui: &mut Ui, destination: Destination, tx: &mut Tx) {
    let pane = match &destination {
        Destination::Settings | Destination::Models => Pane::Settings,
        Destination::Meetings(_) => Pane::Meetings,
        Destination::Modes => Pane::Modes,
        Destination::Commands => Pane::Commands,
        Destination::VoiceAction => Pane::VoiceAction,
        Destination::History => Pane::History,
    };
    if pane.shown(ui) {
        ui.select_pane(pane, tx);
    }
    match destination {
        Destination::Models => {
            let language = ui.settings.transcription.language.clone();
            picker::open(ui, language);
        }
        Destination::Meetings(Some(id)) => ui.meetings.select(id),
        _ => {}
    }
}

/// A capsule's "Never offer" or "Always record" choice, kept in the window's
/// live settings so its next save doesn't drop it.
#[cfg(target_os = "macos")]
pub(crate) fn meeting_provider_chosen(
    ui: &mut Ui,
    choice: crate::app_settings::MeetingProviderChoice,
) {
    crate::app_settings::set_meeting_provider(&mut ui.settings.meetings, choice);
}

/// A meeting recording could not start or finish; the Meetings pane says why.
#[cfg(target_os = "macos")]
pub(crate) fn meeting_failed(ui: &mut Ui, error: String) {
    ui.meetings.start_pending = false;
    ui.meetings.stop_pending = false;
    ui.meetings.runtime_error = Some(error);
}

/// `hex app -- --snap out.png --scene …`: a headless render of a fixture
/// scene, or `live` for the running Hex's `hex status`.
#[cfg(target_os = "linux")]
pub(crate) fn render() {
    rune::run(spec());
}

/// The window's logical size, which every pane and the picker fit.
const WINDOW_SIZE: (f32, f32) = (980.0, 760.0);

/// Linux opens the window at one fixed size the user cannot change. Tiling
/// compositors (sway, Hyprland, niri) and X11 window managers such as i3
/// float a toplevel whose minimum and maximum size match, and Wayland has
/// no other way to ask for floating.
#[cfg(target_os = "linux")]
fn fixed_window() -> rune::WindowOptions {
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
    #[default]
    Settings,
    Modes,
    /// The voice command catalog (macOS).
    Commands,
    VoiceAction,
    History,
    /// The Hex Microphone's tuning and try-it demo (macOS, behind its
    /// experiment).
    Fun,
    /// The dictation indicator's tuning (macOS debug builds).
    HudLab,
    /// Recorded calls (macOS beta, behind the Meetings opt-in).
    Meetings,
    /// The event log (debug builds).
    Activity,
}

impl Pane {
    const ALL: [Pane; 9] = [
        Pane::Settings,
        Pane::Modes,
        Pane::Commands,
        Pane::VoiceAction,
        Pane::History,
        Pane::Fun,
        Pane::HudLab,
        Pane::Meetings,
        Pane::Activity,
    ];

    fn shown(self, ui: &Ui) -> bool {
        let caps = ui.platform.caps;
        match self {
            Pane::Commands => caps.commands,
            Pane::Meetings => caps.meetings && ui.settings.meetings.available(),
            Pane::Fun => caps.fun && ui.settings.experimental.hex_microphone_available(),
            Pane::HudLab => caps.hud_lab && ui.platform.developer,
            Pane::Activity => ui.platform.developer,
            _ => true,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Pane::Settings => tr("Settings"),
            Pane::Modes => tr("Modes"),
            Pane::Commands => tr("Commands"),
            Pane::VoiceAction => tr("Voice Action"),
            Pane::History => tr("History"),
            Pane::Meetings => tr("Meetings"),
            Pane::Fun => tr("Fun"),
            Pane::HudLab => "HUD Lab",
            Pane::Activity => tr("Activity"),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Pane::Settings => "nav-settings",
            Pane::Modes => "nav-modes",
            Pane::Commands => "nav-commands",
            Pane::VoiceAction => "nav-voice-action",
            Pane::History => "nav-history",
            Pane::Meetings => "nav-meetings",
            Pane::Fun => "nav-fun",
            Pane::HudLab => "nav-hud-lab",
            Pane::Activity => "nav-activity",
        }
    }

    fn icon(self) -> &'static Icon {
        match self {
            Pane::Settings => &SETTINGS_ICON,
            Pane::Modes => &MODES_ICON,
            Pane::Commands => &COMMANDS_ICON,
            Pane::VoiceAction => &VOICE_ACTION_ICON,
            Pane::History => &HISTORY_ICON,
            Pane::Meetings => &MEETINGS_ICON,
            Pane::Fun | Pane::HudLab => &HUD_LAB_ICON,
            Pane::Activity => &ACTIVITY_ICON,
        }
    }
}

/// The one anchored menu or popover that is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Microphone,
    Language,
    HistoryRetention,
    /// The OpenCode model search for Voice Action or the selected mode.
    Model,
    /// The OpenCode thinking variant.
    Variant,
    /// The selected meeting's overflow menu.
    MeetingActions,
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
    // Settings
    microphones: Result<Vec<String>, String>,
    confirm_release: bool,
    capture: Capture,
    hints: TextEdit,
    /// The model the service was last seen preparing, and since when.
    preparing: Option<(TranscriptionModelId, Instant)>,
    /// When a text field last changed without being saved.
    draft_since: Option<Instant>,
    models: models::Models,
    voice_action_error: Option<String>,
    history: history::HistoryPane,
    modes: modes::ModesPane,
    picker: picker::Picker,
    tour: Option<onboarding::Tour>,
    /// The OS's capabilities and the OS-only state its panes show.
    platform: platform::PlatformState,
    host: platform::PlatformHost,
    commands: commands::CommandsPane,
    meetings: meetings::MeetingsPane,
    fun: fun::FunPane,
    activity: activity::ActivityPane,
    /// HUD Lab's preset was copied since it last changed.
    hud_copied: bool,
    /// Settings saves and Quit Hex a fixture recorded.
    saved: usize,
    quit_requested: bool,
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
        crate::desktop_i18n::apply(settings.ui_language.as_deref());
        let store = host::history(&connect, settings.history_retention);
        let mut ui = Self::with(settings, Service::Pending(connect));
        ui.error = error;
        ui.history = history::HistoryPane::shared(store);
        ui.models = models::Models::load();
        ui.modes.load_personal_commands();
        ui.platform.developer = crate::DEVELOPER_FEATURES_ENABLED;
        ui.tour = onboarding::Tour::start(&ui.settings, &ui.platform);
        ui.platform.set_login_item(host::launch_at_login());
        ui
    }

    fn with(settings: AppSettings, service: Service) -> Self {
        let hints = TextEdit::new(settings.transcription.recognition_hints.clone());
        let modes = modes::ModesPane::new(&settings);
        Self {
            settings,
            service,
            view: None,
            error: None,
            connection_error: None,
            service_settings: None,
            pane: Pane::Settings,
            open: None,
            microphones: Ok(Vec::new()),
            confirm_release: false,
            capture: Capture::Idle,
            hints,
            preparing: None,
            draft_since: None,
            models: models::Models::default(),
            voice_action_error: None,
            history: history::HistoryPane::default(),
            modes,
            picker: picker::Picker::default(),
            tour: None,
            platform: platform::PlatformState::new(platform::Capabilities::current()),
            host: platform::PlatformHost::None,
            commands: commands::CommandsPane::default(),
            meetings: meetings::MeetingsPane::default(),
            fun: fun::FunPane::default(),
            activity: activity::ActivityPane::default(),
            hud_copied: false,
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
        true
    }

    /// The settings with every text field's current text applied.
    fn drafted(&self) -> AppSettings {
        let mut candidate = self.settings.clone();
        if crate::transcription_models::definition(candidate.transcription.model)
            .supports_recognition_hints
        {
            candidate.transcription.recognition_hints = self.hints.text.clone();
        }
        self.modes.project(&mut candidate);
        candidate
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
                crate::onboarding::PermissionAction::RequestMicrophone,
            ) => self
                .platform
                .note_asked(crate::onboarding::PermissionKind::Microphone),
            platform::PlatformRequest::Permission(
                crate::onboarding::PermissionAction::ShowGuide(kind),
            ) => self.platform.note_asked(*kind),
            platform::PlatformRequest::SystemAudio => self
                .platform
                .note_asked(crate::onboarding::PermissionKind::SystemAudio),
            platform::PlatformRequest::SetLaunchAtLogin(enabled) => match &self.host {
                // Windows writes its Run key from the window's own process.
                platform::PlatformHost::None => {
                    let result = host::set_launch_at_login(*enabled);
                    self.platform.set_login_item(Some(result));
                    return;
                }
                // macOS stays the source of truth; previews flip at once.
                platform::PlatformHost::Fixture(_) => {
                    self.platform.set_login_item(Some(Ok(*enabled)));
                }
                platform::PlatformHost::Native(_) => {}
            },
            _ => {}
        }
        let is_relaunch = request == platform::PlatformRequest::Relaunch;
        if let Err(error) = self.host.send(request) {
            if is_relaunch {
                self.platform.relaunch_failed = true;
            } else {
                self.error = Some(error);
            }
        }
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
    /// edit this window just made.
    fn adopt(&mut self, service: AppSettings) -> bool {
        if self.service_settings.as_ref() == Some(&service) {
            return false;
        }
        let changed = service != self.settings && self.draft_since.is_none();
        if changed {
            if service.transcription.recognition_hints
                != self.settings.transcription.recognition_hints
            {
                self.hints = TextEdit::new(service.transcription.recognition_hints.clone());
            }
            if service.dictation_processing != self.settings.dictation_processing {
                self.modes.reload(&service);
            }
            self.settings = service.clone();
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

    /// Switches panes at once: the new pane replaces the old without a fade.
    fn select_pane(&mut self, pane: Pane, tx: &mut Tx) {
        if self.pane == pane {
            return;
        }
        tx.instant();
        self.flush_drafts();
        settings::cancel_capture(self);
        self.open = None;
        self.pane = pane;
        match pane {
            Pane::History => self.history.reload(),
            Pane::HudLab => self.ask(platform::PlatformRequest::ConfigureHud(self.platform.hud)),
            _ => {}
        }
    }
}

/// Shortcut recording takes every key; otherwise Escape closes the picker
/// or the open menu.
fn on_key(ui: &mut Ui, event: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if settings::on_key(ui, event, tx) {
        return true;
    }
    if event.key != rune::Key::Escape {
        return false;
    }
    if ui.picker.language.is_some() {
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

fn setup(ui: &mut Ui, waker: Waker) {
    ui.modes.waker = Some(waker.clone());
    if let Service::Pending(connect) = &mut ui.service {
        let connect = std::mem::take(connect);
        #[cfg(target_os = "macos")]
        {
            ui.host = platform::PlatformHost::Native(host::native_host());
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
    changed |= ui.models.poll(ui.live());
    changed |= ui.history.poll(ui.pane == Pane::History);
    changed |= ui.modes.poll(ui.live());
    changed |= modes::poll_chooser(ui);
    changed |= ui.host.poll(&mut ui.platform);
    if ui.pane == Pane::Meetings
        && let Some(listed) = ui.host.meetings()
    {
        ui.meetings.adopt(listed);
        changed = true;
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
    changed | settings::advance_capture(ui)
}

// ---------------------------------------------------------------------------
// View

fn view(ui: &Ui, cx: &mut Cx<Ui>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    // Menus and popovers mount above the page (rune-kit overlays).
    let mut layer = Vec::new();
    let (actions, body) = match ui.pane {
        Pane::Settings => (Vec::new(), settings::pane(ui, p, &mut layer)),
        Pane::Modes => modes::pane(ui, p, &mut layer),
        Pane::Commands => commands::pane(ui, p),
        Pane::VoiceAction => (Vec::new(), voice_action::pane(ui, p, &mut layer)),
        Pane::History => history::pane(ui, p, &mut layer),
        Pane::Meetings => meetings::pane(ui, p, &mut layer),
        Pane::Fun => (Vec::new(), fun::pane(ui, p)),
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
        .child(pane_header(p, ui.pane.title(), actions))
        .child(body);
    let window = row()
        .key("window")
        .role("window")
        .fill(p.k.c.bg_deep)
        .child(sidebar(ui, p))
        .child(panel);
    if let Some(modal) = picker::modal(ui, p) {
        layer.push(modal);
    }
    if let Some(tour) = onboarding::overlay(ui, p) {
        layer.push(tour);
    }
    rune_kit::root(window, layer)
}

/// macOS draws the traffic lights over the sidebar (Rune's window has a
/// transparent, full-size title bar), so the navigation starts below them.
const SIDEBAR_TOP: f32 = if cfg!(target_os = "macos") {
    40.0
} else {
    12.0
};

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
    let mut nav = col().key("nav").gap(1.0);
    for pane in Pane::ALL.into_iter().filter(|pane| pane.shown(ui)) {
        nav = nav.child(nav_item(p, pane, ui.pane == pane));
    }
    let action = match ui.view.as_ref().map(|view| view.update_status) {
        Some(DesktopUpdateStatus::ReadyToRestart) => Some(DesktopAction::RestartIntoUpdate),
        Some(DesktopUpdateStatus::Available) => Some(DesktopAction::OpenUpdate),
        _ => None,
    };
    let update = action.map(|action| {
        controls::button(
            p,
            "sidebar-update",
            tr("Update"),
            controls::ButtonKind::Secondary,
            move |ui: &mut Ui, _| ui.send(Request::Desktop(action.clone())),
        )
    });
    title_bar(col())
        .key("sidebar")
        .role("navigation")
        .w(220.0)
        .h_full()
        .pt(SIDEBAR_TOP)
        .pb(12.0)
        .pl(10.0)
        .pr(6.0)
        .child(nav)
        .child(el().key("spacer").grow(1.0))
        .children(update)
        .children(host::QUIT_IN_WINDOW.then(|| quit_item(p)))
        .child(
            text(tr_fill("Version {}", env!("CARGO_PKG_VERSION")))
                .key("version")
                .font(p.k.ty.small)
                .color(p.faint())
                .pl(6.0)
                .pt(8.0),
        )
}

fn nav_item(p: Paint, pane: Pane, on: bool) -> El {
    let c = &p.k.c;
    let mut item = row()
        .key(pane.key())
        .role("button")
        .label(pane.title())
        .h(28.0)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.md)
        .hover(c.hover)
        .press(c.pressed)
        // Switch on pointer down, like a native sidebar; the press keeps
        // keyboard activation.
        .on_drag(move |ui: &mut Ui, d: &mut rune::Drag, tx: &mut Tx| {
            if d.phase == rune::Phase::Down {
                ui.select_pane(pane, tx);
            }
        })
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
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
    item
}

/// Linux has no tray: the window's way to stop Hex. Closing the window
/// keeps Hex dictating.
fn quit_item(p: Paint) -> El {
    let c = &p.k.c;
    row()
        .key("quit")
        .role("button")
        .label(tr("Quit Hex"))
        .h(28.0)
        .px(6.0)
        .mt(4.0)
        .center_y()
        .radius(p.k.radius.md)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(|ui: &mut Ui| ui.quit())
        .child(
            text(tr("Quit Hex"))
                .key("label")
                .font(p.k.ty.ui)
                .color(c.text_muted)
                .truncate(),
        )
}

/// The title bar of the content panel, with the pane's actions on the right.
fn pane_header(p: Paint, title: &str, actions: Vec<El>) -> El {
    title_bar(row())
        .key("pane-header")
        .h(52.0)
        .px(24.0)
        .gap(8.0)
        .center_y()
        .child(
            text(title)
                .key("pane-title")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .grow(1.0)
                .truncate(),
        )
        .children(actions)
}

/// How far a scroll body's top and bottom edges fade out.
const SCROLL_FADE: (f32, f32) = (4.0, 24.0);

/// A vertically scrolling body whose edges fade out. At rest its content
/// sits clear of the fades, so the first and last borders, rings, and fills
/// are never cut; content needing room beside the edges (bordered cards)
/// pads itself horizontally.
fn scroll_body(key: &str, content: El) -> El {
    let (top, bottom) = SCROLL_FADE;
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
        };
    }),
    ("chinese", |ui| {
        *ui = portal_fixture();
        ui.settings.ui_language = Some("zh".into());
        crate::desktop_i18n::apply(Some("zh"));
    }),
    ("menus", |ui| {
        *ui = portal_fixture();
        ui.open = Some(Menu::Microphone);
        ui.microphones = Ok(vec![
            "Built-in Audio Analog Stereo".into(),
            "Shure MV7".into(),
        ]);
        ui.confirm_release = true;
    }),
    ("voice-action", |ui| {
        *ui = portal_fixture();
        ui.settings.voice_action.enabled = true;
        ui.pane = Pane::VoiceAction;
    }),
    ("voice-action-off", |ui| {
        *ui = portal_fixture();
        ui.pane = Pane::VoiceAction;
    }),
    ("voice-action-unavailable", |ui| {
        *ui = portal_fixture();
        ui.settings.voice_action.enabled = true;
        ui.models = models::Models::fixture_missing();
        ui.pane = Pane::VoiceAction;
    }),
    ("voice-action-models", |ui| {
        *ui = portal_fixture();
        ui.settings.voice_action.enabled = true;
        ui.pane = Pane::VoiceAction;
        ui.open = Some(Menu::Model);
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
    ("modes", |ui| {
        *ui = portal_fixture();
        modes::fixture(ui);
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
        ui.modes.transformation_picker_open = true;
        ui.modes.application_picker_open = true;
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
    ("onboarding", |ui| {
        *ui = portal_fixture();
        onboarding::fixture(ui, crate::onboarding::Step::Welcome, false);
    }),
    ("onboarding-setup", |ui| {
        *ui = portal_fixture();
        onboarding::fixture(ui, crate::onboarding::Step::Setup, true);
    }),
    // macOS and Windows surfaces, from preview data (see `platform`).
    ("mac-settings", |ui| *ui = mac_fixture()),
    ("mac-settings-permissions", |ui| {
        *ui = mac_fixture();
        use crate::onboarding::PermissionState;
        ui.platform.permissions.input_monitoring = PermissionState::NeedsSettings;
        ui.platform.permissions.accessibility = PermissionState::NeedsSettings;
        ui.platform
            .permissions_asked
            .push(crate::onboarding::PermissionKind::Accessibility);
        ui.settings.commands_enabled = true;
        ui.platform.command_model = platform::CommandModel::Failed;
        ui.platform.login_item = Some(crate::login_item::LoginItemStatus::RequiresApproval);
        ui.platform.accessibility_renamed = true;
    }),
    ("windows-settings", |ui| {
        *ui = fixture(DesktopShortcuts::Registered);
        ui.platform = platform::PlatformState::preview(platform::Capabilities::WINDOWS);
        ui.host = platform::PlatformHost::Fixture(Vec::new());
        ui.settings.paste_with_shift = false;
    }),
    ("mac-commands", |ui| {
        *ui = mac_fixture();
        ui.settings.commands_enabled = true;
        ui.pane = Pane::Commands;
    }),
    ("mac-commands-confirm", |ui| {
        *ui = mac_fixture();
        ui.settings.release_microphone_while_idle = true;
        ui.commands.confirm_enable = true;
        ui.platform.workspace = platform::Workspace::Missing;
        ui.commands.search = TextEdit::new("zzz");
        ui.pane = Pane::Commands;
    }),
    ("meetings", |ui| {
        *ui = mac_fixture();
        ui.pane = Pane::Meetings;
    }),
    ("meetings-recording", |ui| {
        *ui = mac_fixture();
        ui.meetings.select("preview-live".into());
        ui.pane = Pane::Meetings;
    }),
    ("meetings-notes-open", |ui| {
        *ui = mac_fixture();
        ui.meetings.select("preview-live".into());
        ui.meetings.open_note = ui
            .meetings
            .meetings
            .first()
            .and_then(|meeting| meeting.outline.as_ref())
            .and_then(|outline| outline.sections.get(1))
            .and_then(|section| section.notes.first())
            .map(|note| note.id());
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
    ("meetings-empty", |ui| {
        *ui = mac_fixture();
        ui.meetings = meetings::MeetingsPane::default();
        ui.pane = Pane::Meetings;
    }),
    ("mac-fun", |ui| {
        *ui = mac_fixture();
        ui.pane = Pane::Fun;
    }),
    ("mac-fun-muted", |ui| {
        *ui = mac_fixture();
        ui.platform.tuning.mode = crate::voice_filter::VoiceFilterMode::Muted;
        ui.platform.demo = Some(fun::preview_demo(
            false,
            Some(crate::hex_microphone::DemoPlaybackTrack::CallHears),
        ));
        ui.pane = Pane::Fun;
    }),
    ("mac-fun-driver-missing", |ui| {
        *ui = mac_fixture();
        if let Some(driver) = &mut ui.platform.hex_microphone {
            driver.driver_bundle_installed = false;
            driver.driver_loaded = false;
            driver.installed_version = None;
            driver.active_apps.clear();
        }
        ui.platform.demo = Some(fun::preview_demo(true, None));
        ui.pane = Pane::Fun;
    }),
    ("mac-hud-lab", |ui| {
        *ui = mac_fixture();
        ui.platform.developer = true;
        ui.platform.hud.queued = 2;
        ui.pane = Pane::HudLab;
    }),
    ("mac-activity", |ui| {
        *ui = mac_fixture();
        ui.platform.developer = true;
        ui.activity = activity::ActivityPane::fixture();
        ui.pane = Pane::Activity;
    }),
    ("mac-settings-sides", |ui| {
        *ui = mac_fixture();
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
    ("mac-onboarding-denied", |ui| {
        mac_tour(ui, onboarding::MacStep::SetupDenied)
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
];

/// The macOS window with every permission granted, meetings on, the Hex
/// Microphone experiment on, and sample meetings and commands.
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
    ui.meetings = meetings::MeetingsPane::fixture();
    ui
}

fn mac_tour(ui: &mut Ui, step: onboarding::MacStep) {
    *ui = mac_fixture();
    onboarding::mac_fixture(ui, step);
}

fn portal_fixture() -> Ui {
    fixture(DesktopShortcuts::Portal(vec![
        DesktopGrantedShortcut {
            name: "Dictation".into(),
            trigger: Some("F12".into()),
        },
        DesktopGrantedShortcut {
            name: "Paste last dictation".into(),
            trigger: Some("Ctrl+Alt+V".into()),
        },
    ]))
}

/// Deterministic settings and service state matching a typical Wayland
/// desktop with the shortcuts portal.
fn fixture(shortcuts: DesktopShortcuts) -> Ui {
    crate::desktop_i18n::apply(Some("en"));
    let settings = AppSettings {
        dictation_hotkey: HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: Some(HotkeyKey::named("f12")),
        },
        transcription: crate::transcription_models::TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
            recognition_hints: String::new(),
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
