//! The recording toolbar's controller: opens it on the display under the
//! pointer with the choices last made, saves each change as it's made, and
//! starts what Record asks for.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use rune::OverlayHandle;

use crate::app_settings::RecordKind;
use crate::capture_host::{CaptureNeed, CaptureTarget};
use crate::rune_ui::capture_capsule::toolbar::{
    self, Toolbar, ToolbarAction, ToolbarDisplay, remember,
};

static OVERLAY: OnceLock<OverlayHandle<Toolbar>> = OnceLock::new();
static OPEN: AtomicBool = AtomicBool::new(false);

pub(super) fn install<S: 'static>(app: &mut rune::App<S>) {
    let _ = OVERLAY.set(toolbar::install(app));
}

/// Whether the toolbar shows, so the session tap gives it Escape and Return.
pub(super) fn is_open() -> bool {
    OPEN.load(Ordering::Acquire)
}

/// Opens the toolbar, unless it's open or Screen Recording is missing (then
/// Annotate's permission helper shows, and returns to the picker once it's
/// granted). Safe from any thread; true when it opens.
pub(super) fn open() -> bool {
    let Some(overlay) = OVERLAY.get() else {
        return false;
    };
    if OPEN.swap(true, Ordering::AcqRel) {
        return false;
    }
    // Off the tap's thread: the permission and the microphones take a moment.
    let spawned = std::thread::Builder::new()
        .name("hex-record-toolbar".into())
        .spawn(move || {
            if crate::capture_host::permission_state(CaptureNeed::ScreenRecording, true).is_some() {
                OPEN.store(false, Ordering::Release);
                tracing::info!("recording needs Screen Recording");
                crate::capture_host::report_screenshot_permission(
                    CaptureNeed::ScreenRecording,
                    true,
                );
                return;
            }
            let settings = crate::app_settings::screenshot_settings();
            let microphones = microphones();
            let (displays, under) = displays();
            let handle = overlay.clone();
            overlay.update(move |s, tx| {
                if !is_open() {
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
                    act: Some(act),
                };
                if let Some(id) = on {
                    handle.set_display(rune::DisplayTarget::Native(rune::NativeDisplayId::MacOS(
                        id,
                    )));
                }
                handle.show();
                tx.animated();
            });
        });
    if spawned.is_err() {
        OPEN.store(false, Ordering::Release);
    }
    spawned.is_ok()
}

/// Closes the toolbar. Safe from any thread.
pub(super) fn close() {
    if OPEN.swap(false, Ordering::AcqRel)
        && let Some(overlay) = OVERLAY.get()
    {
        overlay.hide();
    }
}

/// Return: records what the toolbar shows. Safe from any thread.
pub(super) fn record() {
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
            }
        }
        ToolbarAction::Microphone(name) => {
            remember("microphone", move |settings| {
                settings.record_microphone = name
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
