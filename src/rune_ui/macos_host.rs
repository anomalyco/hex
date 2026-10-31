//! The macOS window's host: the runtime thread `macos_rune` starts in this
//! process, which owns model preparation and publishes the listener, model,
//! update, and settings state, plus the OS-only work the panes ask for
//! (permissions, the login item, Hex Microphone, meetings).
//!
//! Shortcut capture stays in the window: an AppKit local event
//! monitor records the chord, because a modifier-only binding such as Option
//! or Fn never reaches Rune as a key event. The listener's event tap is
//! suspended meanwhile so the current binding can be recorded again.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use block2::RcBlock;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};

use super::meetings::Meeting;
use super::meetings_host::MeetingFeed;
use super::platform::{
    CalendarRequest, CalendarView, HudDemo, HudLab, MeetingRequest, NativeHost, NotesAvailability,
    PlatformRequest, PlatformState, UpcomingMeeting, poll_voice_filter, voice_filter_request,
};
use super::practice::PracticeFeed;
use super::settings::Chord;
use super::{HostState, ServiceView};
use crate::app_settings::{
    CalendarSettings, HotkeyBinding, HotkeyKey, HotkeyModifiers, modifiers_from_flags,
};
use crate::calendar::{AccessAnswer, CalendarAccess, CalendarSnapshot};
use crate::desktop::host::DesktopAction;
use crate::desktop::i18n::tr;
use crate::login_item::{LoginItemFailure, LoginItemRequest, LoginItemWorker};
use crate::macos_rune::RuntimeHandle;
pub(super) use crate::macos_rune::{Request, ShortcutTarget};
use crate::onboarding::{Input, PermissionAction, PermissionKind, PermissionState};

/// The window saves a recorded shortcut; the listener follows `settings.json`.
pub(super) const HOST_SAVES_SHORTCUTS: bool = false;
pub(super) const RECORDS_KEY_EVENTS: bool = false;
pub(super) const TERMINAL_PASTE: bool = false;
/// Quit Hex lives in the menu bar and the app menu (⌘Q).
pub(super) const QUIT_IN_WINDOW: bool = false;

/// How often the native host re-reads permissions and driver state.
const PLATFORM_REFRESH: Duration = Duration::from_secs(1);
/// How often the Calendar card rereads EventKit when nothing changed, so
/// started meetings leave the list.
const CALENDAR_REFRESH: Duration = Duration::from_secs(30);
const ESCAPE_KEY_CODE: u16 = 53;

/// The runtime is found through its own module; nothing is handed over.
#[derive(Default)]
pub(crate) struct Connect {
    _nothing: (),
}

pub(super) struct Backend {
    runtime: Option<RuntimeHandle>,
    wake: Arc<dyn Fn() + Send + Sync>,
    chords: Option<ChordWatch>,
}

impl Backend {
    pub(super) fn connect(_: Connect, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let runtime = crate::macos_rune::runtime();
        if let Some(runtime) = &runtime {
            runtime.subscribe(wake.clone());
        }
        Self {
            runtime,
            wake,
            chords: None,
        }
    }

    /// Runs on the main thread (Rune's handlers), where the event monitor
    /// lives.
    pub(super) fn send(&mut self, request: Request) -> Result<()> {
        match request {
            Request::CaptureShortcutFor(_) => {
                self.chords = Some(ChordWatch::start(self.wake.clone())?);
                Ok(())
            }
            Request::CancelShortcut => {
                self.chords = None;
                Ok(())
            }
            Request::Desktop(
                action @ (DesktopAction::CheckForUpdates
                | DesktopAction::OpenUpdate
                | DesktopAction::RestartIntoUpdate
                | DesktopAction::ConfirmUpdateSwitch
                | DesktopAction::WindowFocused),
            ) => crate::macos_rune::update_action(&action),
            request => match &self.runtime {
                Some(runtime) => runtime.send(request),
                None => Err(eyre!("Hex's runtime is not running")),
            },
        }
    }

    /// Saves `settings.json`, then asks the runtime to reload it. The
    /// listener follows the saved file itself.
    pub(super) fn save(&mut self, settings: &crate::app_settings::AppSettings) -> Result<()> {
        settings.save()?;
        self.send(Request::ReloadSettings)
    }

    pub(super) fn quit(&mut self) {
        crate::macos_rune::quit();
    }

    /// Rune hides the window (`keep_running`); the window's own close hook
    /// restores the Dock policy (`macos_rune::window_closed`).
    pub(super) fn closed(&mut self) {}

    /// The runtime publishes into shared memory; nothing to pull.
    pub(super) fn refresh(&mut self) {}

    pub(super) fn error(&self) -> Option<String> {
        self.runtime
            .is_none()
            .then(|| "Hex's runtime is not running".into())
    }

    pub(super) fn state(&self) -> Option<HostState> {
        let published = self.runtime.as_ref()?.published()?;
        Some(HostState {
            view: ServiceView {
                listener: published.listener.clone(),
                operation_error: None,
                transcription: published.transcription.clone(),
                update_status: published.update_status.clone(),
                editing: false,
            },
            settings: published.settings.clone(),
        })
    }

    pub(super) fn take_chord(&mut self) -> Option<Chord> {
        let chord = self.chords.as_ref()?.results.try_recv().ok();
        if matches!(chord, Some(Chord::Cancelled)) {
            self.chords = None;
        }
        chord
    }
}

/// Records chords from the window's keyboard events while Settings listens,
/// with the listener's event tap suspended. Removing the monitor ends it.
struct ChordWatch {
    results: Receiver<Chord>,
    monitor: Retained<AnyObject>,
}

impl ChordWatch {
    fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        crate::app_settings::set_hotkey_capture_active(true);
        let (sender, results) = mpsc::channel();
        let capture = RefCell::new(ChordCapture::default());
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the handler's duration.
            let event_ref = unsafe { event.as_ref() };
            let flags = event_ref.modifierFlags().bits() as u64;
            let chord = match event_ref.r#type() {
                NSEventType::KeyDown => {
                    let characters = event_ref
                        .charactersIgnoringModifiers()
                        .map(|characters| characters.to_string());
                    capture.borrow_mut().key_down(
                        event_ref.keyCode(),
                        event_ref.isARepeat(),
                        characters.as_deref(),
                        flags,
                    )
                }
                NSEventType::FlagsChanged => capture.borrow_mut().flags_changed(flags),
                // Mouse buttons record only with Labs › Mouse shortcuts on;
                // otherwise the click reaches the window as usual.
                NSEventType::LeftMouseDown
                | NSEventType::RightMouseDown
                | NSEventType::OtherMouseDown
                    if crate::labs::mouse_shortcuts() =>
                {
                    capture
                        .borrow_mut()
                        .mouse_down(event_ref.buttonNumber(), flags)
                }
                // Key releases still reach the window so its key state
                // stays balanced.
                _ => return event.as_ptr(),
            };
            if let Some(chord) = chord
                && sender.send(chord).is_ok()
            {
                wake();
            }
            std::ptr::null_mut()
        });
        // SAFETY: the block lives as long as AppKit retains it; the monitor
        // is removed on drop, on the main thread that installed it.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown
                    | NSEventMask::FlagsChanged
                    | NSEventMask::LeftMouseDown
                    | NSEventMask::RightMouseDown
                    | NSEventMask::OtherMouseDown,
                &block,
            )
        };
        match monitor {
            Some(monitor) => Ok(Self { results, monitor }),
            None => {
                crate::app_settings::set_hotkey_capture_active(false);
                Err(eyre!("could not watch the keyboard to record a shortcut"))
            }
        }
    }
}

impl Drop for ChordWatch {
    fn drop(&mut self) {
        // SAFETY: the monitor came from addLocalMonitor on this thread.
        unsafe { NSEvent::removeMonitor(&self.monitor) };
        crate::app_settings::set_hotkey_capture_active(false);
    }
}

/// Turns key and modifier events into a chord: a key with its held
/// modifiers, or the most modifier keys held at once when they are all
/// released without a key, with the sides `recorded_sides` keeps.
#[derive(Default)]
struct ChordCapture {
    peak: HotkeyModifiers,
    /// A key went down with these modifiers; their release records nothing.
    keyed: bool,
}

impl ChordCapture {
    fn mouse_down(&mut self, button: isize, flags: u64) -> Option<Chord> {
        let key = u16::try_from(button).ok().and_then(HotkeyKey::mouse)?;
        let mut modifiers = modifiers_from_flags(flags).without_side_constraints();
        modifiers.function = false;
        self.keyed = !modifiers.is_empty();
        self.peak = HotkeyModifiers::default();
        Some(Chord::Captured(Ok(HotkeyBinding {
            modifiers,
            key: Some(key),
        })))
    }

    fn key_down(
        &mut self,
        code: u16,
        repeat: bool,
        characters: Option<&str>,
        flags: u64,
    ) -> Option<Chord> {
        if repeat {
            return None;
        }
        if code == ESCAPE_KEY_CODE {
            return Some(Chord::Cancelled);
        }
        let mut modifiers = modifiers_from_flags(flags).without_side_constraints();
        // Arrow and function keys report Fn; a chord never binds it.
        modifiers.function = false;
        // Only modifiers held with the key are spent by it.
        self.keyed = !modifiers.is_empty();
        self.peak = HotkeyModifiers::default();
        let key = match hotkey_key(code, characters) {
            Ok(key) => key,
            Err(message) => return Some(Chord::Captured(Err(message))),
        };
        if modifiers.is_empty() && !is_function_key(&key.label) {
            return Some(Chord::Captured(Err(tr("Add a modifier"))));
        }
        Some(Chord::Captured(Ok(HotkeyBinding {
            modifiers,
            key: Some(key),
        })))
    }

    /// While modifiers are down, reports the ones their release would record.
    fn flags_changed(&mut self, flags: u64) -> Option<Chord> {
        let current = modifiers_from_flags(flags).recorded_sides();
        if !current.is_empty() {
            if self.keyed {
                return None;
            }
            if current.count() >= self.peak.count() {
                self.peak = current;
            }
            return Some(Chord::Held(self.peak));
        }
        let keyed = std::mem::take(&mut self.keyed);
        let peak = std::mem::take(&mut self.peak);
        if keyed || peak.is_empty() {
            return None;
        }
        Some(Chord::Captured(Ok(HotkeyBinding {
            modifiers: peak,
            key: None,
        })))
    }
}

fn is_function_key(label: &str) -> bool {
    label
        .strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=20).contains(&number))
}

/// The key a virtual key code records, and its label.
fn hotkey_key(code: u16, characters: Option<&str>) -> Result<HotkeyKey, &'static str> {
    let special = match code {
        49 => Some("Space"),
        48 => Some("Tab"),
        36 => Some("Return"),
        51 => Some(tr("Delete")),
        126 => Some("Up"),
        125 => Some("Down"),
        123 => Some(tr("Left")),
        124 => Some(tr("Right")),
        116 => Some("Page Up"),
        121 => Some("Page Down"),
        115 => Some("Home"),
        119 => Some("End"),
        117 => Some("Forward Delete"),
        114 => Some("Help"),
        122 => Some("F1"),
        120 => Some("F2"),
        99 => Some("F3"),
        118 => Some("F4"),
        96 => Some("F5"),
        97 => Some("F6"),
        98 => Some("F7"),
        100 => Some("F8"),
        101 => Some("F9"),
        109 => Some("F10"),
        103 => Some("F11"),
        111 => Some("F12"),
        105 => Some("F13"),
        107 => Some("F14"),
        113 => Some("F15"),
        106 => Some("F16"),
        64 => Some("F17"),
        79 => Some("F18"),
        80 => Some("F19"),
        90 => Some("F20"),
        _ => None,
    };
    if let Some(label) = special {
        return Ok(HotkeyKey {
            code,
            label: label.into(),
        });
    }
    let mut chars = characters.unwrap_or_default().chars();
    match (chars.next(), chars.next()) {
        (Some(character), None) if !character.is_control() => Ok(HotkeyKey {
            code,
            label: character.to_uppercase().collect(),
        }),
        _ => Err(tr("Unsupported key")),
    }
}

/// The key a typed name records, with the label recording it would show.
pub(super) fn hotkey_key_named(name: &str) -> Result<HotkeyKey, &'static str> {
    let normalized: String = name
        .chars()
        .filter(|character| !matches!(character, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect();
    if let Some(number) = normalized.strip_prefix("mouse") {
        return number
            .parse::<u16>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(HotkeyKey::mouse)
            .ok_or(tr("Unsupported key"));
    }
    let code = match normalized.as_str() {
        "space" => Some(49),
        "tab" => Some(48),
        "return" | "enter" => Some(36),
        "delete" | "backspace" => Some(51),
        "up" => Some(126),
        "down" => Some(125),
        "left" => Some(123),
        "right" => Some(124),
        "pageup" => Some(116),
        "pagedown" => Some(121),
        "home" => Some(115),
        "end" => Some(119),
        "forwarddelete" => Some(117),
        "help" => Some(114),
        function => function
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .and_then(|number| {
                const CODES: [u16; 20] = [
                    122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64,
                    79, 80, 90,
                ];
                CODES.get(usize::from(number).checked_sub(1)?).copied()
            }),
    };
    if let Some(code) = code {
        return hotkey_key(code, None);
    }
    let mut characters = name.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => {
            let character = character.to_lowercase().next().unwrap_or(character);
            let code =
                crate::keyboard::key_code_for(character).map_err(|_| tr("Unsupported key"))?;
            hotkey_key(code, Some(&character.to_string()))
        }
        _ => Err(tr("Unsupported key")),
    }
}

/// macOS accepts every binding the capture can produce, except that Paste
/// Last fires only on a key press (`suppression::paste_action`), and the
/// capture shortcut, pressed once to start and again to finish, and the
/// send-to list's need a key.
pub(super) fn validate_hotkey(
    target: ShortcutTarget,
    binding: &HotkeyBinding,
) -> Result<(), &'static str> {
    if target == ShortcutTarget::PasteLast && binding.key.is_none() {
        return Err(tr("Add a key"));
    }
    #[cfg(hex_capture)]
    if target == ShortcutTarget::Capture && binding.key.is_none() {
        return Err(tr("Add a key"));
    }
    Ok(())
}

/// The listener's retained history, shared with the window.
pub(super) fn history(
    _: &Connect,
    retention: crate::history::HistoryRetention,
) -> Option<crate::history::History> {
    match crate::macos_rune::runtime() {
        Some(runtime) => runtime.history(),
        None => crate::history::History::open_default(retention)
            .inspect_err(|error| tracing::warn!(%error, "could not open retained history"))
            .ok(),
    }
}

/// The listener knows the frontmost application.
pub(super) fn focused_application_known() -> bool {
    true
}

/// The native host reports the login item (`PlatformState::login_item`).
pub(super) fn launch_at_login() -> Option<Result<bool, String>> {
    None
}

pub(super) fn set_launch_at_login(_: bool) -> Result<bool, String> {
    Err("Launch at login goes through the macOS host".into())
}

pub(super) fn native_host() -> Box<dyn NativeHost> {
    Box::new(MacHost::new())
}

/// Performs the panes' OS-only requests and refreshes their state once a
/// second (permission checks and the driver status are not free per frame).
struct MacHost {
    login: Option<LoginItemWorker>,
    login_checking: bool,
    folder_error: Option<String>,
    refresh_at: Instant,
    notes: Option<NotesAvailability>,
    meetings: MeetingFeed,
    practice: Option<PracticeFeed>,
    /// The meetings folder panel, while it is open.
    folder_choice: Option<crate::file_dialog::Pending>,
    calendar: CalendarFeed,
    #[cfg(hex_capture)]
    scrawl: ScrawlHost,
}

impl MacHost {
    fn new() -> Self {
        let login = LoginItemWorker::new()
            .inspect_err(|error| tracing::warn!(%error, "login item worker is unavailable"))
            .ok();
        let mut host = Self {
            login,
            login_checking: false,
            folder_error: None,
            refresh_at: Instant::now(),
            notes: None,
            meetings: MeetingFeed::default(),
            practice: None,
            folder_choice: None,
            calendar: CalendarFeed::default(),
            #[cfg(hex_capture)]
            scrawl: ScrawlHost::default(),
        };
        let _ = host.request_login(LoginItemRequest::Status);
        host
    }

    fn request_login(&mut self, request: LoginItemRequest) -> Result<(), String> {
        let login = self
            .login
            .as_mut()
            .ok_or("Launch at login is unavailable")?;
        login.request(request).inspect_err(|error| {
            tracing::warn!(%error, "could not ask for the login item");
        })?;
        self.login_checking = true;
        Ok(())
    }

    fn runtime(&self) -> Result<RuntimeHandle, String> {
        crate::macos_rune::runtime().ok_or_else(|| "Hex's runtime is not running".into())
    }

    fn meeting(&mut self, request: MeetingRequest) -> Result<(), String> {
        match request {
            MeetingRequest::Start => self
                .runtime()?
                .meeting(crate::meeting::MeetingRequest::Start { title: None }),
            MeetingRequest::Stop => self
                .runtime()?
                .meeting(crate::meeting::MeetingRequest::Stop),
            MeetingRequest::Summarize(id) => self
                .runtime()?
                .meeting(crate::meeting::MeetingRequest::Summarize(id)),
            MeetingRequest::Rename { id, title } => crate::meeting::rename(&id, &title)
                .map(|_| ())
                .map_err(|error| format!("{error:#}")),
            MeetingRequest::Trash(id) => {
                crate::meeting::trash(&id).map_err(|error| format!("{error:#}"))
            }
            MeetingRequest::Reveal(id) => {
                let root = crate::meeting::root().map_err(|error| format!("{error:#}"))?;
                open_path(&root.join(id));
                Ok(())
            }
            MeetingRequest::RevealFolder => {
                let folder = crate::app_settings::meeting_output_folder();
                match std::fs::create_dir_all(&folder) {
                    Ok(()) => {
                        crate::meeting::reveal(&folder);
                        self.folder_error = None;
                    }
                    Err(error) => self.folder_error = Some(error.to_string()),
                }
                Ok(())
            }
            MeetingRequest::ChooseFolder => {
                if self.folder_choice.is_none() {
                    self.folder_choice = Some(choose_folder());
                }
                Ok(())
            }
        }
    }
}

impl NativeHost for MacHost {
    fn send(&mut self, request: PlatformRequest) -> Result<(), String> {
        match request {
            PlatformRequest::Permission(PermissionAction::RequestMicrophone) => {
                crate::onboarding::request_microphone();
            }
            PlatformRequest::Permission(PermissionAction::ShowGuide(kind)) => {
                // Screen Recording may have been granted since the last
                // refresh; the refresh below then shows Allowed, or Restart.
                if kind != PermissionKind::ScreenRecording
                    || crate::capture_host::screen_access()
                        == crate::capture_host::ScreenAccess::Off
                {
                    crate::permission_guide::show(kind);
                }
            }
            PlatformRequest::Permission(PermissionAction::Restart) => {
                if !crate::onboarding::spawn_relaunch() {
                    return Err("could not restart Hex".into());
                }
                crate::macos_rune::quit();
            }
            PlatformRequest::SystemAudio => {
                // macOS prompts once; after that the switch is in System
                // Settings, with the guide beside it.
                if !crate::meeting::request_system_audio_access() {
                    crate::permission_guide::show(PermissionKind::SystemAudio);
                }
            }
            PlatformRequest::SetLaunchAtLogin(enabled) => {
                self.request_login(LoginItemRequest::SetEnabled(enabled))?;
            }
            PlatformRequest::OpenLoginItemSettings => {
                let _ = self.request_login(LoginItemRequest::OpenSettings);
            }
            PlatformRequest::InstallHexMicrophone
            | PlatformRequest::UninstallHexMicrophone
            | PlatformRequest::SetTuning(_)
            | PlatformRequest::HexMicrophoneDemo(_) => voice_filter_request(request),
            PlatformRequest::Meeting(request) => self.meeting(request)?,
            PlatformRequest::Calendar(CalendarRequest::Connect) => self.calendar.connect(),
            PlatformRequest::Calendar(CalendarRequest::OpenPrivacySettings) => {
                self.calendar.opening_privacy_settings();
                crate::calendar::open_privacy_settings();
            }
            PlatformRequest::Relaunch => {
                if let Err(error) = crate::onboarding::record_relaunch(crate::onboarding::unix_ms())
                {
                    tracing::warn!(%error, "could not record the onboarding relaunch");
                }
                if !crate::onboarding::spawn_relaunch() {
                    return Err("could not relaunch Hex".into());
                }
                crate::macos_rune::quit();
            }
            PlatformRequest::ConfigureHud(lab) => configure_hud(&self.runtime()?, lab),
            PlatformRequest::TuneHud(lab) => tune_hud(&self.runtime()?, lab),
            PlatformRequest::EndHudDemo => self
                .runtime()?
                .indicator(crate::dictation::indicator::DictationIndicatorEvent::Reset),
            PlatformRequest::SaveDotScope(tuning) => {
                tuning.save().map_err(|error| format!("{error:#}"))?;
            }
            #[cfg(hex_capture)]
            PlatformRequest::Scrawl(request) => self.scrawl.send(request),
        }
        // Show the result of an action at once, not on the next refresh.
        self.refresh_at = Instant::now();
        Ok(())
    }

    fn poll(&mut self, state: &mut PlatformState) -> bool {
        let mut changed = false;
        // The playground's HUD stories change the tuning HUD Lab shows.
        let shared = crate::rune_hud::dot_scope::lab();
        if shared != state.hud.tuning {
            state.hud.tuning = shared;
            changed = true;
        }
        if let Some(login) = &mut self.login {
            match login.poll() {
                Ok(Some(answer)) => {
                    // A later click may already be running behind this answer.
                    let settled = login.idle();
                    self.login_checking = !settled;
                    state.login_item_answered(answer, settled);
                    changed = true;
                }
                Ok(None) => {}
                Err(message) => {
                    self.login = None;
                    self.login_checking = false;
                    let failure = LoginItemFailure {
                        status: None,
                        message,
                    };
                    state.login_item_answered(Err(failure), true);
                    changed = true;
                }
            }
        }
        if let Some(choice) = self.folder_choice.as_ref().and_then(|p| p.poll()) {
            self.folder_choice = None;
            match choice {
                Ok(Some(folder)) => self.folder_error = save_meetings_folder(folder).err(),
                Ok(None) => {}
                Err(error) => self.folder_error = Some(error),
            }
            changed = true;
        }
        let refresh = Instant::now() >= self.refresh_at;
        changed |= poll_voice_filter(state, refresh);
        #[cfg(hex_capture)]
        {
            changed |= self.scrawl.poll(state, Instant::now() >= self.refresh_at);
        }
        if Instant::now() < self.refresh_at {
            return changed;
        }
        self.refresh_at = Instant::now() + PLATFORM_REFRESH;
        // Input sources are read only while keyboard model switching is on.
        let switching = crate::macos_rune::runtime()
            .and_then(|runtime| runtime.published())
            .is_some_and(|published| published.settings.keyboard_model_switching.enabled);
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            changed |= update(
                &mut state.input_sources,
                if switching {
                    crate::keyboard_input_sources::enabled(mtm)
                } else {
                    Vec::new()
                },
            );
        }
        changed |= self.calendar.poll(&mut state.calendar);
        changed |= update(
            &mut state.accessibility_renamed,
            crate::permission_guide::list_name(PermissionKind::Accessibility) != "Accessibility",
        );
        changed |= update(
            &mut state.activity_log,
            crate::app_paths::logs_dir()
                .ok()
                .map(|logs| logs.join("live.ndjson")),
        );
        let permissions = crate::onboarding::status_with_transcription_model(
            state.permissions.transcription_model,
        );
        let system_audio = crate::meeting::system_audio_ready();
        changed |= update(&mut state.permissions, permissions);
        changed |= update(&mut state.system_audio, system_audio);
        let asked_len = state.permissions_asked.len();
        state.permissions_asked.retain(|kind| match kind {
            PermissionKind::Microphone => permissions.microphone != PermissionState::Ready,
            PermissionKind::Accessibility => permissions.accessibility != PermissionState::Ready,
            PermissionKind::SystemAudio => !system_audio,
            PermissionKind::ScreenRecording => {
                crate::capture_host::screen_access() == crate::capture_host::ScreenAccess::Off
            }
        });
        changed |= state.permissions_asked.len() != asked_len;
        changed |= update(&mut state.login_item_checking, self.login_checking);
        changed |= update(
            &mut state.notes,
            *self.notes.get_or_insert_with(notes_availability),
        );
        changed |= update(&mut state.meeting_folder_error, self.folder_error.clone());
        changed
    }

    fn meetings(&mut self) -> Option<Result<Vec<Meeting>, String>> {
        self.meetings.poll()
    }

    fn tour_inputs(&mut self, practicing: bool) -> Vec<Input> {
        let mut inputs = Vec::new();
        if crate::onboarding::take_listener_failure() {
            inputs.push(Input::ListenerFailed {
                relaunch_allowed: crate::onboarding::relaunch_allowed(crate::onboarding::unix_ms()),
            });
        }
        if !practicing {
            self.practice = None;
            return inputs;
        }
        if self.practice.is_none() {
            self.practice =
                crate::macos_rune::runtime().map(|runtime| PracticeFeed::new(runtime.event_path()));
        }
        if let Some(practice) = &mut self.practice {
            inputs.extend(practice.observations().into_iter().map(Input::Dictation));
        }
        inputs
    }
}

/// The Calendar card's state. EventKit is touched only on the reader thread,
/// never the window's. Connect asks macOS once, on the person's click.
#[derive(Default)]
struct CalendarFeed {
    connect: Option<Receiver<AccessAnswer>>,
    reader: Option<CalendarReader>,
    connect_error: Option<String>,
    /// macOS's last answer in this process. EventKit's status can keep
    /// saying "not determined" after it until Hex relaunches; once the reader
    /// sees a decision, that is current and this is forgotten.
    answered: Option<CalendarAccess>,
    /// The person asked Hex to connect (Connect, or Open settings from the
    /// blocked card) and it is not saved yet, so full access granted in System
    /// Settings afterwards connects. A grant Hex did not start never does.
    asked: bool,
}

struct CalendarReader {
    snapshots: Receiver<CalendarSnapshot>,
    /// Dropping this stops the reader thread.
    _stop: mpsc::SyncSender<()>,
}

/// The Calendar card's reader: rereads when the settings, macOS access, or
/// (once connected) the EventKit store change, and every
/// [`CALENDAR_REFRESH`] so started meetings leave the list.
fn calendar_reader(snapshots: mpsc::SyncSender<CalendarSnapshot>, stop: Receiver<()>) {
    let mut read_for: Option<(CalendarSettings, CalendarAccess, u64)> = None;
    let mut read_at: Option<Instant> = None;
    let mut logged: Option<(CalendarAccess, bool, usize, Option<String>)> = None;
    let mut wait = Duration::ZERO;
    loop {
        if let Err(mpsc::RecvTimeoutError::Disconnected) = stop.recv_timeout(wait) {
            return;
        }
        wait = PLATFORM_REFRESH;
        if !crate::app_settings::meetings_enabled() {
            read_for = None;
            continue;
        }
        let settings = crate::app_settings::calendar_settings();
        let changes = if settings.connected {
            crate::calendar::change_count()
        } else {
            0
        };
        let key = (settings.clone(), crate::calendar::access(), changes);
        if read_for.as_ref() == Some(&key)
            && read_at.is_some_and(|at| at.elapsed() < CALENDAR_REFRESH)
        {
            continue;
        }
        let snapshot = crate::calendar::snapshot(&settings, crate::calendar::wall_clock_ms());
        let summary = (
            snapshot.access,
            settings.connected,
            snapshot.calendars.len(),
            snapshot.error.clone(),
        );
        if logged.as_ref() != Some(&summary) {
            tracing::info!(
                access = ?snapshot.access,
                connected = settings.connected,
                calendars = snapshot.calendars.len(),
                upcoming = snapshot.upcoming.len(),
                error = snapshot.error.as_deref().unwrap_or(""),
                "calendar: read"
            );
            logged = Some(summary);
        }
        read_for = Some(key);
        read_at = Some(Instant::now());
        // The window dropped its feed: stop.
        if snapshots.send(snapshot).is_err() {
            return;
        }
    }
}

impl CalendarFeed {
    fn connect(&mut self) {
        if self.connect.is_some() {
            tracing::info!("calendar: connect clicked while macOS is still asking");
            return;
        }
        tracing::info!("calendar: connect clicked; asking macOS for full access");
        self.asked = true;
        let (sender, receiver) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("hex-calendar-connect".into())
            .spawn(move || {
                let _ = sender.send(crate::calendar::request_access());
            });
        match spawned {
            Ok(_) => {
                self.connect = Some(receiver);
                self.connect_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "calendar: could not start the access request");
                self.connect_error = Some(error.to_string());
            }
        }
    }

    /// Open settings on the blocked card: switching Hex on there connects.
    fn opening_privacy_settings(&mut self) {
        tracing::info!("calendar: opening Privacy & Security › Calendars");
        self.asked = true;
    }

    /// Applies macOS's answer: a grant marks the calendar connected (through
    /// `save`) and shows full access; anything else shows what macOS left.
    fn answer(
        &mut self,
        answer: &AccessAnswer,
        view: &mut CalendarView,
        save: impl FnOnce() -> Result<(), String>,
    ) {
        let access = answer.access();
        tracing::info!(
            granted = answer.granted,
            status = ?answer.status,
            error = answer.error.as_deref().unwrap_or(""),
            ?access,
            "calendar: macOS answered"
        );
        self.connect_error = answer.error.clone();
        if access != CalendarAccess::NotDetermined {
            self.answered = Some(access);
        }
        if access == CalendarAccess::FullAccess {
            self.save_connected(save);
        }
        view.access = access;
        view.error = self.connect_error.clone();
    }

    fn save_connected(&mut self, save: impl FnOnce() -> Result<(), String>) {
        match save() {
            Ok(()) => {
                tracing::info!("calendar: saved connected; settings reloading");
                self.asked = false;
            }
            Err(error) => {
                tracing::warn!(%error, "calendar: could not save connected");
                self.connect_error = Some(error);
            }
        }
    }

    /// Takes a reader snapshot: forgets the answer once EventKit reports a
    /// decision, and connects when full access arrives after the person asked
    /// Hex to connect (refused the prompt, then switched Hex on in System
    /// Settings).
    fn read(
        &mut self,
        snapshot: CalendarSnapshot,
        connected: bool,
        view: &CalendarView,
        now_ms: i64,
        save: impl FnOnce() -> Result<(), String>,
    ) -> CalendarView {
        if snapshot.access != CalendarAccess::NotDetermined {
            self.answered = None;
        }
        if self.asked && !connected && snapshot.access == CalendarAccess::FullAccess {
            tracing::info!("calendar: full access granted after connect; connecting");
            self.connect_error = None;
            self.save_connected(save);
        }
        self.shown(snapshot, view, now_ms)
    }

    /// A reader snapshot as the card shows it.
    fn shown(&self, snapshot: CalendarSnapshot, view: &CalendarView, now_ms: i64) -> CalendarView {
        let access = match (snapshot.access, self.answered) {
            (CalendarAccess::NotDetermined, Some(answered)) => answered,
            (access, _) => access,
        };
        CalendarView {
            access,
            calendars: snapshot.calendars,
            upcoming: snapshot
                .upcoming
                .iter()
                .map(|meeting| upcoming_meeting(meeting, now_ms))
                .collect(),
            connecting: view.connecting,
            error: snapshot.error.or_else(|| self.connect_error.clone()),
        }
    }

    fn poll(&mut self, view: &mut CalendarView) -> bool {
        let mut changed = false;
        if let Some(receiver) = &self.connect
            && let Ok(answer) = receiver.try_recv()
        {
            self.connect = None;
            self.answer(&answer, view, save_calendar_connected);
            changed = true;
        }
        changed |= update(&mut view.connecting, self.connect.is_some());
        if self.reader.is_none() && crate::app_settings::meetings_enabled() {
            let (sender, snapshots) = mpsc::sync_channel(1);
            let (stop, stopped) = mpsc::sync_channel(1);
            let spawned = std::thread::Builder::new()
                .name("hex-calendar-read".into())
                .spawn(move || calendar_reader(sender, stopped));
            match spawned {
                Ok(_) => {
                    self.reader = Some(CalendarReader {
                        snapshots,
                        _stop: stop,
                    });
                }
                Err(error) => tracing::warn!(%error, "calendar: could not start the reader"),
            }
        }
        if let Some(reader) = &self.reader
            && let Ok(snapshot) = reader.snapshots.try_recv()
        {
            let connected = crate::app_settings::calendar_settings().connected;
            let shown = self.read(
                snapshot,
                connected,
                view,
                crate::calendar::wall_clock_ms(),
                save_calendar_connected,
            );
            changed |= update(view, shown);
        }
        changed
    }
}

/// One upcoming meeting as the Calendar card lists it: "Today · 2:30 PM".
fn upcoming_meeting(meeting: &crate::calendar::CalendarMeeting, now_ms: i64) -> UpcomingMeeting {
    let start = meeting.start_ms.max(0) as u64;
    let now = now_ms.max(0) as u64;
    let day = if crate::meeting::day_label(start, now) == "Today" {
        tr("Today").to_string()
    } else if crate::meeting::day_label(start, now + 86_400_000) == "Today" {
        tr("Tomorrow").to_string()
    } else {
        crate::meeting::day_label(start, now)
    };
    UpcomingMeeting {
        title: meeting.display_title(),
        when: format!("{day} · {}", crate::meeting::clock(start)),
        provider: meeting.link.as_ref().map(|link| link.provider),
    }
}

/// Marks the calendar connected after macOS granted access; the runtime
/// publishes it to the window and starts reminders.
fn save_calendar_connected() -> Result<(), String> {
    let mut settings =
        crate::app_settings::AppSettings::load().map_err(|error| format!("{error:#}"))?;
    settings.meetings.calendar.connected = true;
    settings.save().map_err(|error| format!("{error:#}"))?;
    match crate::macos_rune::runtime() {
        Some(runtime) => runtime
            .send(Request::ReloadSettings)
            .map_err(|error| format!("{error:#}"))?,
        None => tracing::warn!("calendar: no runtime to reload settings"),
    }
    Ok(())
}

/// Opens the folder panel beside the window, starting at the current
/// meetings folder.
fn choose_folder() -> crate::file_dialog::Pending {
    use crate::file_dialog::{Dialog, Target};
    crate::file_dialog::open(
        Dialog {
            title: tr("Transcript folder").into(),
            target: Target::Folder,
            directory: Some(crate::app_settings::meeting_output_folder()),
        },
        || crate::macos_rune::on_main_thread(|| {}),
    )
}

/// The Scrawl pane's OS work: Scrawl's own app and the captures folder.
/// Its Screen Recording Allow opens the permission guide.
#[cfg(hex_capture)]
#[derive(Default)]
struct ScrawlHost {
    folder_choice: Option<crate::file_dialog::Pending>,
    folder_error: Option<String>,
}

#[cfg(hex_capture)]
impl ScrawlHost {
    fn send(&mut self, request: super::platform::ScrawlRequest) {
        use super::platform::ScrawlRequest;
        match request {
            // `send` moves the next refresh to now.
            ScrawlRequest::Refresh => {}
            ScrawlRequest::QuitScrawl => crate::capture_host::quit_scrawl(),
            ScrawlRequest::RevealFolder => {
                let folder = crate::app_settings::capture_settings().captures_dir();
                match std::fs::create_dir_all(&folder) {
                    Ok(()) => {
                        crate::meeting::reveal(&folder);
                        self.folder_error = None;
                    }
                    Err(error) => self.folder_error = Some(error.to_string()),
                }
            }
            ScrawlRequest::ChooseFolder => {
                if self.folder_choice.is_none() {
                    use crate::file_dialog::{Dialog, Target};
                    self.folder_choice = Some(crate::file_dialog::open(
                        Dialog {
                            title: tr("Annotations folder").into(),
                            target: Target::Folder,
                            directory: Some(crate::app_settings::capture_settings().captures_dir()),
                        },
                        || crate::macos_rune::on_main_thread(|| {}),
                    ));
                }
            }
        }
    }

    /// The pane shows while the `capture` experiment is on; `refresh` also
    /// rereads the permission and whether Scrawl runs.
    fn poll(&mut self, state: &mut PlatformState, refresh: bool) -> bool {
        let mut changed = state.scrawl.is_none();
        let view = state.scrawl.get_or_insert_default();
        if let Some(choice) = self.folder_choice.as_ref().and_then(|p| p.poll()) {
            self.folder_choice = None;
            match choice {
                Ok(Some(folder)) => {
                    self.folder_error = save_settings(|settings| {
                        settings.capture.folder = Some(folder);
                    })
                    .err();
                }
                Ok(None) => {}
                Err(error) => self.folder_error = Some(error),
            }
            changed = true;
        }
        changed |= update(&mut view.folder_error, self.folder_error.clone());
        if !(refresh || changed) {
            return changed;
        }
        let screen = crate::capture_host::screen_access();
        let screen_recording = screen == crate::capture_host::ScreenAccess::Allowed;
        let needs_restart = screen == crate::capture_host::ScreenAccess::NeedsRestart;
        changed |= update(&mut view.screen_needs_restart, needs_restart);
        let running = crate::capture_host::scrawl_running();
        changed |= update(&mut view.screen_recording, screen_recording);
        changed |= update(&mut view.scrawl_running, running);
        if !running {
            changed |= update(&mut view.quitting_scrawl, false);
        }
        changed
    }
}

/// Saves one change to the settings; the runtime publishes it to the
/// window.
#[cfg(hex_capture)]
fn save_settings(change: impl FnOnce(&mut crate::app_settings::AppSettings)) -> Result<(), String> {
    let mut settings =
        crate::app_settings::AppSettings::load().map_err(|error| format!("{error:#}"))?;
    change(&mut settings);
    settings.save().map_err(|error| format!("{error:#}"))?;
    if let Some(runtime) = crate::macos_rune::runtime() {
        runtime
            .send(Request::ReloadSettings)
            .map_err(|error| format!("{error:#}"))?;
    }
    Ok(())
}

/// Saves the chosen meetings folder; the runtime publishes it to the window.
fn save_meetings_folder(folder: PathBuf) -> Result<(), String> {
    let mut settings =
        crate::app_settings::AppSettings::load().map_err(|error| format!("{error:#}"))?;
    settings.meetings.output_folder = Some(folder);
    settings.save().map_err(|error| format!("{error:#}"))?;
    if let Some(runtime) = crate::macos_rune::runtime() {
        runtime
            .send(Request::ReloadSettings)
            .map_err(|error| format!("{error:#}"))?;
    }
    Ok(())
}

/// Shows HUD Lab's tuning and demonstrated state on the dictation HUD.
fn configure_hud(runtime: &RuntimeHandle, lab: HudLab) {
    use crate::dictation::indicator::DictationIndicatorEvent as Event;
    tune_hud(runtime, lab);
    runtime.indicator(Event::Reset);
    match lab.demo {
        HudDemo::Recording => {
            runtime.indicator(Event::Started);
            runtime.indicator(Event::DemoVoice);
        }
        HudDemo::Transcribing | HudDemo::Processing => {
            runtime.indicator(Event::Submitted { job_id: 0 });
            runtime.indicator(if lab.demo == HudDemo::Transcribing {
                Event::Transcribing { job_id: 0 }
            } else {
                Event::Processing { job_id: 0 }
            });
        }
    }
}

/// Applies HUD Lab's tuning to whatever the HUD shows.
fn tune_hud(runtime: &RuntimeHandle, lab: HudLab) {
    use crate::dictation::indicator::DictationIndicatorEvent as Event;
    runtime.indicator(Event::ConfigureDotScope(lab.tuning));
}

fn update<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

fn notes_availability() -> NotesAvailability {
    use crate::apple_intelligence::Availability;
    match crate::apple_intelligence::availability() {
        Availability::Available => NotesAvailability::Available,
        Availability::DeviceNotEligible => NotesAvailability::DeviceNotEligible,
        Availability::AppleIntelligenceNotEnabled => NotesAvailability::NotEnabled,
        Availability::ModelNotReady => NotesAvailability::ModelNotReady,
        Availability::UnsupportedSystem => NotesAvailability::UnsupportedSystem,
        Availability::Unknown => NotesAvailability::Unknown,
    }
}

fn open_path(path: &std::path::Path) {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
    {
        tracing::error!(%error, path = %path.display(), "could not open path");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::ModifierSide;

    #[test]
    fn paste_last_needs_a_key() {
        let modifiers_only = HotkeyBinding {
            modifiers: crate::app_settings::HotkeyModifiers::option_command(),
            key: None,
        };
        assert_eq!(
            validate_hotkey(ShortcutTarget::PasteLast, &modifiers_only),
            Err(tr("Add a key"))
        );
        assert_eq!(
            validate_hotkey(ShortcutTarget::Dictation, &modifiers_only),
            Ok(())
        );
        let chord = HotkeyBinding::paste_last_default();
        assert_eq!(validate_hotkey(ShortcutTarget::PasteLast, &chord), Ok(()));
    }

    const LEFT_OPTION: u64 = 0x0008_0020;
    const RIGHT_OPTION: u64 = 0x0008_0040;
    const LEFT_COMMAND: u64 = 0x0010_0008;
    const FUNCTION: u64 = 0x0080_0000;

    fn captured(chord: Option<Chord>) -> Result<HotkeyBinding, &'static str> {
        match chord {
            Some(Chord::Captured(result)) => result,
            Some(Chord::Held(_)) => panic!("still held"),
            Some(Chord::Cancelled) => panic!("cancelled"),
            None => panic!("nothing recorded"),
        }
    }

    fn held(chord: Option<Chord>) -> Vec<String> {
        match chord {
            Some(Chord::Held(modifiers)) => HotkeyBinding {
                modifiers,
                key: None,
            }
            .keycaps(),
            _ => panic!("not holding"),
        }
    }

    #[test]
    fn mouse_buttons_record_with_or_without_modifiers() {
        for button in 0..32 {
            for flags in [0, LEFT_OPTION, RIGHT_OPTION | LEFT_COMMAND] {
                let mut capture = ChordCapture::default();
                capture.flags_changed(flags);
                let binding = captured(capture.mouse_down(button, flags)).unwrap();
                assert_eq!(binding.key, HotkeyKey::mouse(button as u16));
                assert_eq!(
                    binding.modifiers,
                    modifiers_from_flags(flags).without_side_constraints()
                );
                assert!(capture.flags_changed(0).is_none());
                let text = crate::control::settings::format_hotkey(&binding);
                let parsed = crate::control::settings::parse_hotkey(&text, |name| {
                    hotkey_key_named(name).map_err(str::to_owned)
                });
                // A bare left click is recorded, then refused (issue 411).
                if binding.is_bare_primary_click() {
                    assert!(parsed.is_err());
                } else {
                    assert_eq!(parsed.unwrap(), binding);
                }
            }
        }
        let mut capture = ChordCapture::default();
        assert!(capture.mouse_down(-1, 0).is_none());
        assert!(capture.mouse_down(32, 0).is_none());
        assert!(matches!(
            capture.key_down(ESCAPE_KEY_CODE, false, None, 0),
            Some(Chord::Cancelled)
        ));
    }

    #[test]
    fn mouse_bindings_persist_and_conflict_only_with_the_same_trigger() {
        let binding = captured(ChordCapture::default().mouse_down(3, 0)).unwrap();
        let restored: HotkeyBinding =
            serde_json::from_str(&serde_json::to_string(&binding).unwrap()).unwrap();
        assert_eq!(restored, binding);
        assert_eq!(restored.keycaps(), ["Mouse 4"]);
        assert!(binding.overlaps(&restored));
        assert!(!binding.overlaps(&captured(ChordCapture::default().mouse_down(4, 0)).unwrap()));
        assert!(!binding.overlaps(&HotkeyBinding::default()));
        for name in ["mouse0", "mouse33", "mouse"] {
            assert!(hotkey_key_named(name).is_err());
        }
        assert_eq!(
            hotkey_key_named("mouse-1").unwrap(),
            HotkeyKey::mouse(0).unwrap()
        );
    }

    #[test]
    fn releasing_modifiers_records_the_most_held_with_their_side() {
        let mut capture = ChordCapture::default();
        assert_eq!(held(capture.flags_changed(LEFT_OPTION)), ["L⌥"]);
        assert_eq!(
            held(capture.flags_changed(LEFT_OPTION | LEFT_COMMAND)),
            ["⌥", "⌘"],
            "two modifiers record from either side"
        );
        assert_eq!(
            held(capture.flags_changed(LEFT_OPTION)),
            ["⌥", "⌘"],
            "letting one go still shows what releasing records"
        );
        let binding = captured(capture.flags_changed(0)).unwrap();
        assert_eq!(binding.key, None);
        assert_eq!(binding.modifiers.option, Some(ModifierSide::Either));
        assert_eq!(binding.modifiers.command, Some(ModifierSide::Either));
        let binding = {
            assert_eq!(held(capture.flags_changed(RIGHT_OPTION)), ["R⌥"]);
            captured(capture.flags_changed(0)).unwrap()
        };
        assert_eq!(binding.modifiers.option, Some(ModifierSide::Right));
        assert_eq!(held(capture.flags_changed(FUNCTION)), ["fn"]);
        assert!(
            captured(capture.flags_changed(0))
                .unwrap()
                .modifiers
                .function
        );
    }

    #[test]
    fn both_options_record_as_one_chord_of_two_keys() {
        let mut capture = ChordCapture::default();
        assert_eq!(held(capture.flags_changed(RIGHT_OPTION)), ["R⌥"]);
        assert_eq!(
            held(capture.flags_changed(LEFT_OPTION | RIGHT_OPTION)),
            ["L⌥", "R⌥"]
        );
        assert_eq!(
            held(capture.flags_changed(LEFT_OPTION)),
            ["L⌥", "R⌥"],
            "the peak stays"
        );
        let binding = captured(capture.flags_changed(0)).unwrap();
        assert_eq!(binding.modifiers.option, Some(ModifierSide::Both));
        assert_eq!(binding.modifiers.count(), 2);
        assert_eq!(binding.standalone_side(), None, "no side picker");
        assert_eq!(
            crate::control::settings::format_hotkey(&binding),
            "left-option+right-option"
        );
        assert!(!binding.overlaps(&HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Right),
                ..Default::default()
            },
            key: None,
        }));
        assert!(
            binding.overlaps(&HotkeyBinding::default()),
            "⌥ takes both too"
        );
        assert_eq!(
            held(capture.flags_changed(LEFT_OPTION | RIGHT_OPTION | LEFT_COMMAND)),
            ["L⌥", "R⌥", "⌘"],
            "an extra key shows while held"
        );
    }

    #[test]
    fn a_key_records_its_modifiers_on_either_side_and_their_release_nothing() {
        let mut capture = ChordCapture::default();
        assert_eq!(held(capture.flags_changed(LEFT_OPTION)), ["L⌥"]);
        let binding = captured(capture.key_down(9, false, Some("v"), LEFT_OPTION)).unwrap();
        assert_eq!(binding.modifiers.option, Some(ModifierSide::Either));
        assert_eq!(
            binding.key,
            Some(HotkeyKey {
                code: 9,
                label: "V".into()
            })
        );
        assert!(capture.flags_changed(0).is_none());
        assert!(capture.key_down(9, true, Some("v"), LEFT_OPTION).is_none());
    }

    #[test]
    fn keys_without_modifiers_need_one_except_function_keys() {
        let mut capture = ChordCapture::default();
        assert_eq!(
            captured(capture.key_down(0, false, Some("a"), 0)),
            Err(tr("Add a modifier"))
        );
        assert_eq!(
            held(capture.flags_changed(LEFT_OPTION)),
            ["L⌥"],
            "a rejected bare key spends no modifiers"
        );
        assert!(captured(capture.flags_changed(0)).is_ok());
        let f12 = captured(capture.key_down(111, false, None, FUNCTION)).unwrap();
        assert!(f12.modifiers.is_empty());
        assert_eq!(f12.key.unwrap().label, "F12");
        assert!(matches!(
            capture.key_down(ESCAPE_KEY_CODE, false, None, 0),
            Some(Chord::Cancelled)
        ));
    }

    fn calendar_answer(granted: bool, status: CalendarAccess) -> AccessAnswer {
        AccessAnswer {
            granted,
            status,
            error: None,
        }
    }

    #[test]
    fn a_granted_calendar_is_saved_connected_and_stays_connected() {
        let mut feed = CalendarFeed::default();
        let mut view = CalendarView::default();
        let mut saved = false;
        feed.answer(
            &calendar_answer(true, CalendarAccess::NotDetermined),
            &mut view,
            || {
                saved = true;
                Ok(())
            },
        );
        assert!(saved);
        assert_eq!(view.access, CalendarAccess::FullAccess);
        assert_eq!(view.error, None);
        // A snapshot read before the answer must not undo it.
        let stale = CalendarSnapshot::default();
        assert_eq!(
            feed.shown(stale, &view, 0).access,
            CalendarAccess::FullAccess
        );
    }

    #[test]
    fn a_failed_save_shows_its_error() {
        let mut feed = CalendarFeed::default();
        let mut view = CalendarView::default();
        feed.answer(
            &calendar_answer(true, CalendarAccess::FullAccess),
            &mut view,
            || Err("disk full".into()),
        );
        assert_eq!(view.error.as_deref(), Some("disk full"));
        let shown = feed.shown(CalendarSnapshot::default(), &view, 0);
        assert_eq!(shown.error.as_deref(), Some("disk full"));
    }

    #[test]
    fn a_denied_calendar_is_not_saved_and_stays_blocked() {
        let mut feed = CalendarFeed::default();
        let mut view = CalendarView::default();
        feed.answer(
            &calendar_answer(false, CalendarAccess::NotDetermined),
            &mut view,
            || panic!("a denial must not save"),
        );
        assert_eq!(view.access, CalendarAccess::Denied);
        assert!(view.access.blocked());
        let shown = feed.shown(CalendarSnapshot::default(), &view, 0);
        assert_eq!(shown.access, CalendarAccess::Denied);
        // System Settings granting it later is what the reader reports.
        let granted = CalendarSnapshot {
            access: CalendarAccess::FullAccess,
            ..CalendarSnapshot::default()
        };
        assert_eq!(
            feed.shown(granted, &view, 0).access,
            CalendarAccess::FullAccess
        );
    }

    fn full_access() -> CalendarSnapshot {
        CalendarSnapshot {
            access: CalendarAccess::FullAccess,
            ..CalendarSnapshot::default()
        }
    }

    #[test]
    fn granting_in_system_settings_after_a_refusal_connects() {
        let mut feed = CalendarFeed::default();
        let mut view = CalendarView::default();
        feed.asked = true; // Connect was clicked.
        feed.answer(
            &calendar_answer(false, CalendarAccess::NotDetermined),
            &mut view,
            || panic!("a denial must not save"),
        );
        let denied = CalendarSnapshot {
            access: CalendarAccess::Denied,
            ..CalendarSnapshot::default()
        };
        feed.read(denied, false, &view, 0, || panic!("still denied"));
        feed.opening_privacy_settings();
        let mut saved = 0;
        let shown = feed.read(full_access(), false, &view, 0, || {
            saved += 1;
            Ok(())
        });
        assert_eq!(saved, 1);
        assert_eq!(shown.access, CalendarAccess::FullAccess);
        // Saved once: the next read before the reload does not save again.
        feed.read(full_access(), false, &view, 0, || panic!("saved twice"));
    }

    #[test]
    fn a_grant_hex_did_not_ask_for_never_connects() {
        let mut feed = CalendarFeed::default();
        let shown = feed.read(full_access(), false, &CalendarView::default(), 0, || {
            panic!("connecting needs the person's click")
        });
        assert_eq!(shown.access, CalendarAccess::FullAccess);
    }

    #[test]
    fn a_decision_from_eventkit_replaces_the_remembered_answer() {
        let mut feed = CalendarFeed::default();
        let mut view = CalendarView::default();
        feed.answer(
            &calendar_answer(true, CalendarAccess::NotDetermined),
            &mut view,
            || Ok(()),
        );
        feed.read(full_access(), true, &view, 0, || panic!("connected"));
        // `tccutil reset` puts it back to not determined: Connect again.
        let reset = feed.read(CalendarSnapshot::default(), true, &view, 0, || {
            panic!("not granted")
        });
        assert_eq!(reset.access, CalendarAccess::NotDetermined);
    }

    #[test]
    fn a_failed_request_keeps_connect_and_shows_the_error() {
        let mut feed = CalendarFeed::default();
        let mut view = CalendarView::default();
        let answer = AccessAnswer {
            granted: false,
            status: CalendarAccess::NotDetermined,
            error: Some("missing usage description".into()),
        };
        feed.answer(&answer, &mut view, || panic!("a failure must not save"));
        assert_eq!(view.access, CalendarAccess::NotDetermined);
        assert_eq!(view.error.as_deref(), Some("missing usage description"));
        let shown = feed.shown(CalendarSnapshot::default(), &view, 0);
        assert_eq!(shown.access, CalendarAccess::NotDetermined);
    }
}
