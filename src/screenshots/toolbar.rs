//! The recording toolbar's controller: opens it on the display under the
//! pointer with the choices last made, saves each change as it's made, and
//! starts what Record asks for.

use std::sync::OnceLock;

use rune::OverlayHandle;

use super::picker::ToolbarClose;
use crate::app_settings::RecordKind;
use crate::capture_host::{CaptureNeed, CaptureTarget};
use crate::rune_ui::capture_capsule::toolbar::{
    self, Toolbar, ToolbarAction, ToolbarDisplay, remember,
};

static OVERLAY: OnceLock<OverlayHandle<Toolbar>> = OnceLock::new();

pub(super) fn install<S: 'static>(app: &mut rune::App<S>) {
    let _ = OVERLAY.set(toolbar::install(app));
}

/// Whether the toolbar has passed permission, been populated, and shows on
/// screen. False while permission or devices are still loading.
pub(super) fn is_open() -> bool {
    super::front().toolbar_shown()
}

/// Opens the toolbar, unless a picker or toolbar is already open/loading or
/// Screen Recording is missing (then Annotate's permission helper shows, and
/// returns to the picker once it's granted). Safe from any thread; true when
/// opening starts.
pub(super) fn open() -> bool {
    let Some(overlay) = OVERLAY.get() else {
        return false;
    };
    let Some(id) = super::front().begin_toolbar() else {
        return false;
    };
    toolbar::forget_menu();
    // Off the tap's thread: the permission and the microphones take a moment.
    let spawned = std::thread::Builder::new()
        .name("hex-record-toolbar".into())
        .spawn(move || {
            if crate::capture_host::permission_state(CaptureNeed::ScreenRecording, true).is_some() {
                if super::front().toolbar_permission_denied(id) {
                    tracing::info!("recording needs Screen Recording");
                    crate::capture_host::report_screenshot_permission(
                        CaptureNeed::ScreenRecording,
                        true,
                    );
                }
                return;
            }
            let settings = crate::app_settings::screenshot_settings();
            let metered = settings.record_microphone.clone();
            let microphones = microphones();
            let (displays, under) = displays();
            let handle = overlay.clone();
            overlay.update(move |s, tx| {
                if !super::front().toolbar_show(id) {
                    return;
                }
                let names = display_names();
                let displays = displays
                    .into_iter()
                    .enumerate()
                    .map(|(index, (id, at))| ToolbarDisplay {
                        id,
                        name: names
                            .iter()
                            .find(|(number, _)| *number == id)
                            .map(|(_, name)| name.clone())
                            .unwrap_or_else(|| {
                                crate::desktop::i18n::tr_fill(
                                    "Display {}",
                                    &(index + 1).to_string(),
                                )
                            }),
                        at,
                    })
                    .collect::<Vec<_>>();
                let on = displays.get(under).map(|display| display.id);
                *s = Toolbar {
                    kind: settings.record_kind,
                    displays,
                    display: under,
                    microphone: settings.record_microphone.clone(),
                    microphones,
                    system_audio: settings.record_system_audio,
                    one_x: settings.scale_to_1x,
                    fps: settings.fps(),
                    quality: settings.quality,
                    level: None,
                    act: Some(act),
                };
                if let Some(id) = on {
                    handle.set_display(rune::DisplayTarget::Native(rune::NativeDisplayId::MacOS(
                        id,
                    )));
                }
                dock(true, on);
                handle.show();
                tx.animated();
                super::toolbar_level::start(handle.clone(), metered);
            });
        });
    if spawned.is_err() {
        super::front().toolbar_permission_denied(id);
    }
    spawned.is_ok()
}

/// Claims the toolbar's bar at the bottom of the display `on` names (the
/// primary when none) in the overlay dock while it shows, so a dictation HUD
/// at the bottom clears it; nothing once it closes.
fn dock(shown: bool, on: Option<u32>) {
    use crate::desktop::overlay_dock::{self, Align, Claim, Display, Edge, Surface};
    let claim = shown.then(|| Claim {
        display: on.map_or(Display::Primary, |id| Display::Id(id.to_string())),
        display_width: None,
        edge: Edge::Bottom,
        align: Align::Center,
        inset: 0.0,
        margin: toolbar::MARGIN,
        size: toolbar::SIZE,
    });
    overlay_dock::claim(Surface::AnnotateToolbar, claim);
}

/// Closes the toolbar (or cancels an in-flight open). Safe from any thread.
pub(super) fn close() {
    super::toolbar_level::stop();
    dock(false, None);
    if super::front().close_toolbar() == ToolbarClose::ClosedShown
        && let Some(overlay) = OVERLAY.get()
    {
        overlay.hide();
    }
}

/// Return: records what the toolbar shows once it is actually shown. Safe from
/// any thread.
pub(super) fn record() {
    if !is_open() {
        return;
    }
    if let Some(overlay) = OVERLAY.get() {
        overlay.update(|s, _| {
            if is_open() {
                s.act(ToolbarAction::Record);
            }
        });
    }
}

/// A choice on the toolbar, on the main thread.
fn act(toolbar: &Toolbar, action: ToolbarAction) {
    match action {
        ToolbarAction::Kind(kind) => {
            remember("kind", move |settings| settings.record_kind = kind);
        }
        ToolbarAction::Display(_) => {
            if let (Some(display), Some(overlay)) = (toolbar.chosen_display(), OVERLAY.get()) {
                overlay.set_display(rune::DisplayTarget::Native(rune::NativeDisplayId::MacOS(
                    display.id,
                )));
                dock(true, Some(display.id));
            }
        }
        ToolbarAction::Microphone(name) => {
            if let Some(overlay) = OVERLAY.get() {
                super::toolbar_level::start(overlay.clone(), name.clone());
            }
            remember("microphone", move |settings| {
                settings.record_microphone.clone_from(&name);
            });
        }
        ToolbarAction::SystemAudio(on) => {
            remember("computer audio", move |settings| {
                settings.record_system_audio = on;
            });
        }
        ToolbarAction::OneX(on) => {
            remember("scale", move |settings| settings.scale_to_1x = on);
        }
        ToolbarAction::Fps(fps) => {
            remember("frame rate", move |settings| settings.frame_rate = fps);
        }
        ToolbarAction::Quality(quality) => {
            remember("quality", move |settings| settings.quality = quality);
        }
        ToolbarAction::Close => close(),
        ToolbarAction::Record => {
            let at = toolbar.chosen_display().map(|display| display.at);
            close();
            start(toolbar.kind, at);
        }
    }
}

/// Starts a recording of `kind`: the picker for an area or a window, or at
/// once for the display at `at`.
fn start(kind: RecordKind, at: Option<[f64; 2]>) {
    match kind {
        RecordKind::Area => {
            super::open_as(true, false);
        }
        RecordKind::Window => {
            super::open_as(true, true);
        }
        RecordKind::Screen => {
            let at = at
                .or_else(scrawl_capture::screens::pointer)
                .unwrap_or_default();
            let clip = crate::capture_host::CaptureControl::Command(
                crate::capture_host::CaptureCommand::Clip(CaptureTarget::Display { at }),
            );
            if let Err(error) = crate::capture_host::send_to_listener(clip) {
                tracing::warn!(%error, "could not start a recording");
                crate::feedback::play(crate::feedback::Tone::Error);
            }
        }
    }
}

/// The input devices a recording can open, less Hex Microphone.
fn microphones() -> Vec<String> {
    match crate::audio::input_devices() {
        Ok(devices) => devices
            .names
            .into_iter()
            .filter(|name| !crate::audio::hex_microphone::is_hex_virtual_device_name(name))
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "could not list microphones for the recording toolbar");
            Vec::new()
        }
    }
}

/// Every display's id and centre, and which one the pointer is on.
fn displays() -> (Vec<(u32, [f64; 2])>, usize) {
    use scrawl_capture::screens;
    let displays = screens::displays();
    let under = screens::pointer()
        .and_then(|at| screens::index_at(&displays, at))
        .unwrap_or(0);
    let listed = displays
        .iter()
        .map(|display| {
            let [x, y, w, h] = display.frame;
            (display.id, [x + w * 0.5, y + h * 0.5])
        })
        .collect();
    (listed, under)
}

/// Every screen's `NSScreenNumber` and the name macOS gives it ("Studio
/// Display"). Main thread only; empty elsewhere.
fn display_names() -> Vec<(u32, String)> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{NSNumber, NSString};
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return Vec::new();
    };
    let key = NSString::from_str("NSScreenNumber");
    NSScreen::screens(mtm)
        .iter()
        .filter_map(|screen| {
            let description = screen.deviceDescription();
            let number = description.objectForKey(&key)?;
            let id = number.downcast_ref::<NSNumber>()?.unsignedIntValue();
            Some((id, screen.localizedName().to_string()))
        })
        .collect()
}
