//! Labs › Screenshots (macOS): ⌘⇧4 picks an area (Space for a window), as
//! CleanShot does, and the shot lands on the clipboard, in the folder, and
//! in a thumbnail in the corner.
//!
//! The shortcut is taken in Hex's session tap ([`key`]), never by the
//! listener: a press opens a second capture picker, Escape closes it and
//! Space switches it between an area and a window. What it picks is shot
//! by ScreenCaptureKit at the display's own pixels, with Hex's overlays and
//! the pointer left out, on its own thread.

mod save;
pub(crate) mod system_shortcut;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::app_settings::RuntimeHotkey;
use crate::capture_host::{CaptureNeed, CaptureTarget};
use crate::rune_ui::capture_capsule::flow::{Action, Mode};
use crate::rune_ui::capture_capsule::picker::{PickerOverlay, PickerRole};
use crate::rune_ui::screenshot_thumbnail::Thumbnail;

struct Overlays {
    picker: PickerOverlay,
    thumbnail: Thumbnail,
}

static OVERLAYS: OnceLock<Overlays> = OnceLock::new();
/// While the picker shows, the tap hands it Escape and Space.
static PICKING: AtomicBool = AtomicBool::new(false);
/// Whether it picks a window rather than an area.
static WINDOW_MODE: AtomicBool = AtomicBool::new(false);
/// The picker's window, left out of the shot; 0 before it's made.
static PICKER_WINDOW: AtomicU32 = AtomicU32::new(0);

/// Adds the screenshot picker and thumbnail, hidden. They're made at
/// launch only while the Lab is on then.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    let picker = PickerOverlay::install_as(
        app,
        picked,
        PickerRole {
            name: "Hex screenshot picker",
            screen_label: crate::desktop::i18n::tr("Full screen · Click to capture"),
            exclude_from_capture: true,
            prewarm: crate::labs::screenshots(),
            created: |window| PICKER_WINDOW.store(window, Ordering::Release),
        },
    );
    let thumbnail = Thumbnail::install(app);
    let _ = OVERLAYS.set(Overlays { picker, thumbnail });
}

/// Opens the picker on an area, unless one is open or Screen Recording is
/// missing (then Annotate's permission helper shows). Safe from any thread;
/// true when it opens.
pub(crate) fn begin() -> bool {
    if !crate::labs::screenshots() || OVERLAYS.get().is_none() {
        return false;
    }
    if PICKING.swap(true, Ordering::AcqRel) {
        return false;
    }
    WINDOW_MODE.store(false, Ordering::Release);
    // Off the tap's thread: a fresh Screen Recording read may take a moment.
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot-open".into())
        .spawn(|| {
            if crate::capture_host::permission_state(CaptureNeed::ScreenRecording, true).is_some() {
                PICKING.store(false, Ordering::Release);
                tracing::info!("screenshot needs Screen Recording");
                crate::capture_host::report_missing_permission(CaptureNeed::ScreenRecording);
                return;
            }
            show(Some(Mode::Area));
        });
    if spawned.is_err() {
        PICKING.store(false, Ordering::Release);
    }
    spawned.is_ok()
}

/// Closes the picker without a shot.
pub(crate) fn cancel() {
    PICKING.store(false, Ordering::Release);
    show(None);
}

/// Switches the open picker between an area and a window.
fn toggle() {
    let window = !WINDOW_MODE.fetch_xor(true, Ordering::AcqRel);
    show(Some(if window { Mode::Window } else { Mode::Area }));
}

pub(crate) fn picking() -> bool {
    PICKING.load(Ordering::Acquire)
}

fn show(mode: Option<Mode>) {
    if let Some(overlays) = OVERLAYS.get() {
        overlays.picker.set(mode);
    }
}

/// The picker's pick, on the main thread: it closes, and the shot is taken
/// on its own thread.
fn picked(action: Action) {
    let Action::Start(target) = action else {
        return;
    };
    cancel();
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot".into())
        .spawn(move || shoot(target));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start a screenshot");
    }
}

/// `hex screenshot`: the whole display under the pointer, at once.
pub(crate) fn shoot_display() -> anyhow::Result<std::path::PathBuf> {
    anyhow::ensure!(crate::labs::screenshots(), "Labs › Screenshots is off");
    let at = scrawl_capture::screens::pointer().unwrap_or_default();
    take(CaptureTarget::Display { at })
}

fn shoot(target: CaptureTarget) {
    match take(target) {
        Ok(path) => tracing::info!(path = %path.display(), "screenshot saved"),
        Err(error) => {
            tracing::warn!(%error, "screenshot failed");
            crate::feedback::play(crate::feedback::Tone::Error);
        }
    }
}

/// Shoots `target`, saves it, copies it, and shows its thumbnail.
fn take(target: CaptureTarget) -> anyhow::Result<std::path::PathBuf> {
    let (source, points_wide) = source(target)?;
    let image = capture_core::screenshot_with(&source, &exclusions(), None, false)?;
    let scale = f64::from(image.width) / points_wide.max(1.0);
    let png = save::encode(image.width, image.height, &image.rgba, scale)?;
    let folder = crate::app_settings::screenshot_settings().folder();
    let path = save::write(&folder, &png);
    if let Err(error) = &path {
        tracing::warn!(%error, folder = %folder.display(), "could not save the screenshot");
    }
    save::copy(&png, path.as_deref().ok())?;
    let path = path?;
    if let Some(overlays) = OVERLAYS.get() {
        let (width, height, small) = shrink(image.width, image.height, &image.rgba, THUMB_PX);
        overlays.thumbnail.show(path.clone(), width, height, small);
    }
    Ok(path)
}

/// The thumbnail's long edge, in pixels.
const THUMB_PX: u32 = 480;

/// What ScreenCaptureKit shoots for `target`, and its width in points.
fn source(target: CaptureTarget) -> anyhow::Result<(capture_core::Source, f64)> {
    use scrawl_capture::screens;
    let displays = screens::displays();
    let display_at = |at: [f64; 2]| {
        screens::index_at(&displays, at)
            .map(|i| &displays[i])
            .ok_or_else(|| anyhow::anyhow!("no display there"))
    };
    Ok(match target {
        CaptureTarget::Window { id } => {
            let wide = scrawl_capture::windows::raw_windows()
                .into_iter()
                .find(|w| w.id == id)
                .map_or(0.0, |w| w.bounds[2]);
            (capture_core::Source::Window(id), wide)
        }
        CaptureTarget::Pointer => {
            let at = screens::pointer().unwrap_or_default();
            let display = display_at(at)?;
            (capture_core::Source::Display(display.id), display.frame[2])
        }
        CaptureTarget::Phone => anyhow::bail!("a screenshot is never of an iPhone"),
        CaptureTarget::Display { at } => {
            let display = display_at(at)?;
            (capture_core::Source::Display(display.id), display.frame[2])
        }
        CaptureTarget::Area { rect: [x, y, w, h] } => {
            let display = display_at([x + w * 0.5, y + h * 0.5])?;
            let [dx, dy, ..] = display.frame;
            let rect = capture_core::Rect {
                x: x - dx,
                y: y - dy,
                width: w,
                height: h,
            };
            (
                capture_core::Source::Region {
                    display: display.id,
                    rect,
                },
                w,
            )
        }
    })
}

/// Hex's own overlays (every window of Hex's above the normal layer) and
/// the picker, left out of the shot. Hex's settings window stays in.
fn exclusions() -> capture_core::Exclusions {
    let pid = std::process::id() as i32;
    let mut windows: Vec<u32> = scrawl_capture::windows::raw_windows()
        .into_iter()
        .filter(|w| w.pid == pid && w.layer != 0)
        .map(|w| w.id)
        .collect();
    let picker = PICKER_WINDOW.load(Ordering::Acquire);
    if picker != 0 && !windows.contains(&picker) {
        windows.push(picker);
    }
    capture_core::Exclusions {
        windows,
        apps: Vec::new(),
    }
}

/// `rgba` shrunk by whole-pixel box averages until its long edge is at
/// most `long_edge`; unchanged when it already is.
fn shrink(width: u32, height: u32, rgba: &[u8], long_edge: u32) -> (u32, u32, Vec<u8>) {
    let step = width.max(height).div_ceil(long_edge.max(1)).max(1);
    let (w, h) = (width / step, height / step);
    if step == 1 || w == 0 || h == 0 {
        return (width, height, rgba.to_vec());
    }
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..step {
                let row = ((y * step + dy) * width + x * step) as usize * 4;
                for px in rgba[row..row + step as usize * 4].as_chunks::<4>().0 {
                    for (total, v) in sum.iter_mut().zip(px) {
                        *total += u32::from(*v);
                    }
                }
            }
            let n = step * step;
            out.extend(sum.map(|total| (total / n) as u8));
        }
    }
    (w, h, out)
}

/// What the session tap does with a key: lets it go, keeps it from the app
/// and the listener (Escape, so it cancels nothing else), or keeps it from
/// the app with the listener still seeing it, so a held modifier pressing
/// the shortcut stays a chord rather than a dictation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Take {
    No,
    Only,
    AfterListener,
}

/// What a key does here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    None,
    Begin,
    Cancel,
    Toggle,
}

const ESCAPE: u16 = 53;
const SPACE: u16 = 49;

/// The keys this took the press of, so their releases go too.
#[derive(Default)]
struct Taken(u128);

impl Taken {
    fn decide(
        &mut self,
        key: Key,
        hotkey: Option<RuntimeHotkey>,
        picking: bool,
        recording_shortcut: bool,
    ) -> (Take, Effect) {
        let Some(bit) = 1u128.checked_shl(u32::from(key.code)) else {
            return (Take::No, Effect::None);
        };
        let take = if key.code == ESCAPE {
            Take::Only
        } else {
            Take::AfterListener
        };
        if !key.down {
            let ours = self.0 & bit != 0;
            self.0 &= !bit;
            return (if ours { take } else { Take::No }, Effect::None);
        }
        if recording_shortcut {
            return (Take::No, Effect::None);
        }
        let shortcut = hotkey.is_some_and(|h| h.matches_key_press(key.code, key.flags));
        let effect = match key.code {
            _ if shortcut && !picking => Effect::Begin,
            _ if shortcut => Effect::None,
            ESCAPE if picking => Effect::Cancel,
            SPACE if picking => Effect::Toggle,
            _ => return (Take::No, Effect::None),
        };
        self.0 |= bit;
        let effect = if key.repeat { Effect::None } else { effect };
        (take, effect)
    }
}

/// A key event as the session tap reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Key {
    pub(crate) code: u16,
    pub(crate) down: bool,
    pub(crate) repeat: bool,
    pub(crate) flags: u64,
}

/// The session tap's key, on its thread. Does nothing while the Lab is
/// off and no key it took is still down.
pub(crate) fn key(key: Key) -> Take {
    static TAKEN: std::sync::Mutex<Taken> = std::sync::Mutex::new(Taken(0));
    let mut taken = TAKEN.lock().unwrap_or_else(|error| error.into_inner());
    let picking = picking();
    if taken.0 == 0 && !picking && !crate::labs::screenshots() {
        return Take::No;
    }
    let (take, effect) = taken.decide(
        key,
        crate::app_settings::screenshot_hotkey(),
        picking,
        crate::app_settings::hotkey_capture_active(),
    );
    drop(taken);
    match effect {
        Effect::None => {}
        Effect::Begin => {
            begin();
        }
        Effect::Cancel => cancel(),
        Effect::Toggle => toggle(),
    }
    take
}

/// Whether macOS's own screenshot shortcut takes Hex's now: it's matched
/// in the window server before any app's tap, so Hex never sees the press.
/// Read again at most every 2 s, since Settings draws it each frame.
pub(crate) fn system_takes_shortcut() -> bool {
    use std::time::{Duration, Instant};
    static LAST: std::sync::Mutex<Option<(Instant, Option<RuntimeHotkey>, bool)>> =
        std::sync::Mutex::new(None);
    let hotkey = crate::app_settings::screenshot_hotkey();
    let mut last = LAST.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((at, read_for, takes)) = *last
        && read_for == hotkey
        && at.elapsed() < Duration::from_secs(2)
    {
        return takes;
    }
    let takes = hotkey.is_some_and(system_shortcut::takes);
    *last = Some((Instant::now(), hotkey, takes));
    takes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::HotkeyBinding;

    const COMMAND_SHIFT: u64 = 0x12_0000;

    fn press(code: u16, flags: u64) -> Key {
        Key {
            code,
            down: true,
            repeat: false,
            flags,
        }
    }

    fn release(code: u16) -> Key {
        Key {
            code,
            down: false,
            repeat: false,
            flags: 0,
        }
    }

    fn hotkey() -> Option<RuntimeHotkey> {
        Some(HotkeyBinding::screenshot_default().runtime())
    }

    #[test]
    fn the_shortcut_opens_the_picker_and_its_release_stays_with_it() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), hotkey(), false, false),
            (Take::AfterListener, Effect::Begin)
        );
        assert_eq!(
            taken.decide(release(21), hotkey(), true, false),
            (Take::AfterListener, Effect::None)
        );
        assert_eq!(taken.0, 0);
    }

    #[test]
    fn a_held_shortcut_opens_one_picker() {
        let mut taken = Taken::default();
        let mut repeat = press(21, COMMAND_SHIFT);
        repeat.repeat = true;
        taken.decide(press(21, COMMAND_SHIFT), hotkey(), false, false);
        assert_eq!(
            taken.decide(repeat, hotkey(), true, false),
            (Take::AfterListener, Effect::None)
        );
    }

    #[test]
    fn escape_and_space_belong_to_the_open_picker_only() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(ESCAPE, 0), hotkey(), false, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(SPACE, 0), hotkey(), true, false),
            (Take::AfterListener, Effect::Toggle)
        );
        assert_eq!(
            taken.decide(press(ESCAPE, 0), hotkey(), true, false),
            (Take::Only, Effect::Cancel)
        );
        // Escape's release goes with it even once the picker closed.
        assert_eq!(
            taken.decide(release(ESCAPE), hotkey(), false, false),
            (Take::Only, Effect::None)
        );
    }

    #[test]
    fn other_keys_and_other_chords_pass() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(21, 0x10_0000), hotkey(), false, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(0, 0), hotkey(), true, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), None, false, false),
            (Take::No, Effect::None)
        );
    }

    #[test]
    fn recording_a_shortcut_takes_nothing() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), hotkey(), false, true),
            (Take::No, Effect::None)
        );
    }

    #[test]
    fn a_large_shot_shrinks_for_its_thumbnail() {
        let rgba = [10u8, 20, 30, 255].repeat(8 * 4);
        let (w, h, small) = shrink(8, 4, &rgba, 4);
        assert_eq!((w, h), (4, 2));
        assert_eq!(&small[..4], &[10, 20, 30, 255]);
        let (w, h, same) = shrink(8, 4, &rgba, 8);
        assert_eq!((w, h, same.len()), (8, 4, rgba.len()));
    }
}
