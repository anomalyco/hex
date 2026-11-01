//! The Windows window's host: the runtime thread `windows_rune` starts in
//! this process, which owns the listener, model preparation, and updates.
//! Shortcut capture stays in the window: it polls the keyboard
//! state, because a modifier-only chord such as Ctrl+Win never reaches the
//! window as a key event.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

use color_eyre::Result;

use super::settings::Chord;
use super::{HostState, ServiceView};
use crate::app_settings::HotkeyBinding;
use crate::desktop::i18n::tr;
use crate::windows::input::{ChordCapture, ChordPoll, WindowsHotkey};
use crate::windows::rune::RuntimeHandle;
pub(super) use crate::windows::rune::{Request, ShortcutTarget};

/// The window saves a recorded shortcut; the runtime restarts the listener.
pub(super) const HOST_SAVES_SHORTCUTS: bool = false;
pub(super) const RECORDS_KEY_EVENTS: bool = false;
/// Windows' Terminal paste (Ctrl+Shift+V) is a Labs feature, not a row in
/// General.
pub(super) const TERMINAL_PASTE: bool = false;
/// The window offers Quit Hex beside the tray's.
pub(super) const QUIT_IN_WINDOW: bool = true;

/// How often shortcut capture samples the keyboard.
const CHORD_POLL: Duration = Duration::from_millis(16);

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
        let runtime = crate::windows::rune::runtime();
        if let Some(runtime) = &runtime {
            runtime.subscribe(wake.clone());
        }
        Self {
            runtime,
            wake,
            chords: None,
        }
    }

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
            request => match &self.runtime {
                Some(runtime) => runtime.send(request),
                None => Err(color_eyre::eyre::eyre!("Hex's runtime is not running")),
            },
        }
    }

    /// Saves `settings.json`, then asks the runtime to reload it.
    pub(super) fn save(&mut self, settings: &crate::app_settings::AppSettings) -> Result<()> {
        settings.save()?;
        self.send(Request::ReloadSettings)
    }

    /// Asks the runtime to stop the listener and end the app, as the
    /// tray's Quit Hex does.
    pub(super) fn quit(&mut self) {
        if let Err(error) = self.send(Request::Quit) {
            tracing::warn!(%error, "could not quit Hex");
        }
    }

    /// Rune hides the window (`keep_running`) and the runtime keeps
    /// listening; without a tray Rune ends the app instead.
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
                operation_error: published.operation_error.clone(),
                transcription: published.transcription.clone(),
                update_status: published.update_status.clone(),
                update_notes: published.update_notes.clone(),
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

/// Samples the keyboard on its own thread while the window records a
/// shortcut, with the listener's hook suspended so the current binding
/// can be recorded again.
struct ChordWatch {
    results: Receiver<Chord>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ChordWatch {
    fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        crate::windows::input::set_capture_inhibited(true);
        let (sender, results) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::Builder::new()
            .name("hex-chord-capture".into())
            .spawn(move || {
                let mut capture = ChordCapture::default();
                // The modifiers last shown while the chord is in progress.
                let mut shown = capture.held();
                while !worker_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(CHORD_POLL);
                    // Keys typed into another application never rebind a
                    // shortcut.
                    if !crate::windows::input::app_is_foreground() {
                        continue;
                    }
                    let chord = match capture.poll() {
                        ChordPoll::Pending if capture.held() == shown => continue,
                        ChordPoll::Pending => {
                            shown = capture.held();
                            Chord::Held(HotkeyBinding::from(&shown).modifiers)
                        }
                        ChordPoll::Cancelled => Chord::Cancelled,
                        ChordPoll::Unsupported => {
                            shown = capture.held();
                            Chord::Captured(Err(tr("Unsupported key")))
                        }
                        ChordPoll::Captured(hotkey) => {
                            // A rejected chord: record the next one from
                            // scratch.
                            capture = ChordCapture::default();
                            shown = capture.held();
                            Chord::Captured(Ok(HotkeyBinding::from(&hotkey)))
                        }
                    };
                    let cancelled = matches!(chord, Chord::Cancelled);
                    if sender.send(chord).is_err() {
                        return;
                    }
                    wake();
                    if cancelled {
                        return;
                    }
                }
            });
        match worker {
            Ok(worker) => Ok(Self {
                results,
                stop,
                worker: Some(worker),
            }),
            Err(error) => {
                crate::windows::input::set_capture_inhibited(false);
                Err(error.into())
            }
        }
    }
}

impl Drop for ChordWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        crate::windows::input::set_capture_inhibited(false);
    }
}

/// A dictation shortcut needs a modifier, a function key, or a mouse button,
/// and Paste Last fires on a key press.
pub(super) fn validate_hotkey(
    target: ShortcutTarget,
    binding: &HotkeyBinding,
) -> Result<(), &'static str> {
    if target != ShortcutTarget::Dictation && binding.key.is_none() {
        return Err(tr("Add a key"));
    }
    WindowsHotkey::from(binding)
        .validate()
        .map_err(|_| tr("Add a modifier"))
}

/// The listener's retained history, shared with the window.
pub(super) fn history(
    _: &Connect,
    retention: crate::history::HistoryRetention,
) -> Option<crate::history::History> {
    match crate::windows::rune::runtime() {
        Some(runtime) => runtime.history(),
        None => crate::history::History::open_default(retention)
            .inspect_err(|error| tracing::warn!(%error, "could not open retained history"))
            .ok(),
    }
}

/// UI Automation reports the foreground application.
pub(super) fn focused_application_known() -> bool {
    true
}

/// The current-user Run key is the source of truth, never `settings.json`.
pub(super) fn launch_at_login() -> Option<Result<bool, String>> {
    Some(crate::windows::login_item::is_enabled().map_err(|error| format!("{error:#}")))
}

pub(super) fn set_launch_at_login(enabled: bool) -> Result<bool, String> {
    crate::windows::login_item::set_enabled(enabled).map_err(|error| format!("{error:#}"))?;
    crate::windows::login_item::is_enabled().map_err(|error| format!("{error:#}"))
}
