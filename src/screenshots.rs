//! Labs › Screenshots (macOS): ⌘⇧4 picks an area (Space for a window), as
//! CleanShot does, and the shot lands on the clipboard, in the folder, and
//! in a thumbnail in the corner.
//!
//! The shortcut is taken in Hex's session tap ([`key`]), never by the
//! listener: a press opens a second capture picker, Escape closes it and
//! Space switches it between an area and a window. What it picks is shot
//! by ScreenCaptureKit at the display's own pixels, with Hex's overlays and
//! the pointer left out, on its own thread.
//!
//! The record shortcut (unbound until set) opens the same picker to record
//! what it picks as an Annotate capture, and pressing it again stops; the
//! recording then lands on the same card ([`recording`]).

pub(crate) mod narration;
mod recording;
pub(crate) mod save;
pub(crate) mod system_shortcut;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::app_settings::RuntimeHotkey;
use crate::capture_host::{CaptureNeed, CaptureTarget};
use crate::history::captures::{self, CaptureDraft, CaptureKind, CaptureWord};
use crate::rune_ui::capture_capsule::flow::{Action, Mode};
use crate::rune_ui::capture_capsule::picker::{PickerOverlay, PickerRole};
use crate::rune_ui::screenshot_thumbnail::{CardKey, Thumbnail};

struct Overlays {
    picker: PickerOverlay,
    thumbnail: Thumbnail,
}

static OVERLAYS: OnceLock<Overlays> = OnceLock::new();
/// While the picker shows, the tap hands it Escape and Space.
static PICKING: AtomicBool = AtomicBool::new(false);
/// Whether it picks a window rather than an area.
static WINDOW_MODE: AtomicBool = AtomicBool::new(false);
/// Whether the open picker records what it picks rather than shooting it.
static RECORDING: AtomicBool = AtomicBool::new(false);
/// The picker's window, left out of the shot; 0 before it's made.
static PICKER_WINDOW: AtomicU32 = AtomicU32::new(0);

/// Adds the screenshot picker, thumbnail, and editor, hidden. They're made at
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
    crate::rune_ui::screenshot_thumbnail::editor::install(app);
    let _ = OVERLAYS.set(Overlays { picker, thumbnail });
    crate::capture_host::on_clip_written(recording::written);
}

/// Opens the picker on an area, unless one is open or Screen Recording is
/// missing (then Annotate's permission helper shows). Safe from any thread;
/// true when it opens.
pub(crate) fn begin() -> bool {
    open(false)
}

/// The record shortcut: stops the Screenshots recording under way, or opens
/// the picker to record what it picks. Nothing while an Annotate capture
/// records. Safe from any thread.
pub(crate) fn record() -> bool {
    match crate::capture_host::snapshot().recording {
        Some(recording) if recording.clip => {
            let stop = crate::capture_host::CaptureControl::Scrawl(
                crate::control::ScrawlCaptureAction::Stop,
            );
            if let Err(error) = crate::capture_host::send_to_listener(stop) {
                tracing::warn!(%error, "could not stop the recording");
                return false;
            }
            true
        }
        Some(_) => false,
        None => open(true),
    }
}

/// Opens the picker to shoot or, with `record`, to record. The picker's own
/// permission helper calls this again once its grant is made.
pub(crate) fn open(record: bool) -> bool {
    if !crate::labs::screenshots() || OVERLAYS.get().is_none() {
        return false;
    }
    if PICKING.swap(true, Ordering::AcqRel) {
        return false;
    }
    WINDOW_MODE.store(false, Ordering::Release);
    RECORDING.store(record, Ordering::Release);
    if let Some(overlays) = OVERLAYS.get() {
        overlays.picker.set_screen_label(if record {
            crate::desktop::i18n::tr("Full screen · Click to record")
        } else {
            crate::desktop::i18n::tr("Full screen · Click to capture")
        });
    }
    // Off the tap's thread: a fresh Screen Recording read may take a moment.
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot-open".into())
        .spawn(move || {
            if crate::capture_host::permission_state(CaptureNeed::ScreenRecording, true).is_some() {
                PICKING.store(false, Ordering::Release);
                tracing::info!("screenshot needs Screen Recording");
                // The helper returns here, to the picker, once it's granted:
                // never to Annotate's whole-screen recording.
                crate::capture_host::report_screenshot_permission(
                    CaptureNeed::ScreenRecording,
                    record,
                );
                return;
            }
            show(Some(Mode::Area));
        });
    if spawned.is_err() {
        PICKING.store(false, Ordering::Release);
    }
    spawned.is_ok()
}

/// Restore Last Capture: brings back the card closed last. Safe from any
/// thread.
pub(crate) fn restore_last() {
    if crate::labs::screenshots()
        && let Some(overlays) = OVERLAYS.get()
    {
        overlays.thumbnail.restore();
    }
}

/// Whether Restore Last Capture has a card to bring back.
pub(crate) fn can_restore() -> bool {
    crate::labs::screenshots() && crate::rune_ui::screenshot_thumbnail::restorable()
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
/// on its own thread, or the listener starts recording it.
fn picked(action: Action) {
    let Action::Start(target) = action else {
        return;
    };
    cancel();
    if RECORDING.swap(false, Ordering::AcqRel) {
        let clip = crate::capture_host::CaptureControl::Command(
            crate::capture_host::CaptureCommand::Clip(target),
        );
        if let Err(error) = crate::capture_host::send_to_listener(clip) {
            tracing::warn!(%error, "could not start a recording");
            crate::feedback::play(crate::feedback::Tone::Error);
        }
        return;
    }
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
    let shot = source(target)?;
    let image = capture_core::screenshot_with(&shot.source, &exclusions(), None, false)?;
    let scale = f64::from(image.width) / shot.points_wide.max(1.0);
    let png = save::encode(image.width, image.height, &image.rgba, scale)?;
    let folder = crate::app_settings::screenshot_settings().folder();
    let path = save::write(&folder, &png);
    if let Err(error) = &path {
        tracing::warn!(%error, folder = %folder.display(), "could not save the screenshot");
    }
    save::copy(&png, path.as_deref().ok())?;
    let path = path?;
    let (width, height, small) = shrink(image.width, image.height, &image.rgba, THUMB_PX);
    remember(
        Remembered {
            kind: CaptureKind::Shot,
            file: &path,
            size: [image.width, image.height],
            duration_ms: 0,
            timed: Vec::new(),
        },
        (width, height, &small),
    );
    if let Some(overlays) = OVERLAYS.get() {
        overlays.thumbnail.show(
            path.clone(),
            width,
            height,
            small,
            shot.screen,
            shot.capture,
        );
    }
    Ok(path)
}

/// A saved capture for Captures to list.
struct Remembered<'a> {
    kind: CaptureKind,
    file: &'a std::path::Path,
    size: [u32; 2],
    duration_ms: u64,
    timed: Vec<CaptureWord>,
}

/// Remembers a saved capture in Captures, with a thumbnail shrunk from
/// `rgba` (`width` × `height`).
fn remember(capture: Remembered, (width, height, rgba): (u32, u32, &[u8])) {
    let (w, h, small) = shrink(width, height, rgba, captures::THUMB_PX);
    let thumbnail_png = match save::encode(w, h, &small, 1.0) {
        Ok(png) => png,
        Err(error) => {
            tracing::warn!(%error, "could not make the capture's thumbnail");
            return;
        }
    };
    let words = capture
        .timed
        .iter()
        .map(|word| word.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    captures::record(CaptureDraft {
        kind: capture.kind,
        file: capture.file.to_owned(),
        size: capture.size,
        duration_ms: capture.duration_ms,
        words,
        timed: capture.timed,
        thumbnail_png,
    });
}

/// What was said over the shot saved at `file` in the editor, for
/// Captures' search. Off the caller's thread.
pub(crate) fn remember_words(file: std::path::PathBuf, words: String) {
    let spawned = std::thread::Builder::new()
        .name("hex-capture-words".into())
        .spawn(move || captures::set_words(&file, &words));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not keep the words said over the shot");
    }
}

/// The display under the pointer, in points, where a recording's card rests.
fn screen_under_pointer() -> (f32, f32) {
    use scrawl_capture::screens;
    let displays = screens::displays();
    screens::pointer()
        .and_then(|at| screens::index_at(&displays, at))
        .map(|i| (displays[i].frame[2] as f32, displays[i].frame[3] as f32))
        .unwrap_or(crate::rune_ui::screenshot_thumbnail::NOMINAL)
}

/// The thumbnail's long edge, in pixels.
const THUMB_PX: u32 = 480;

/// What ScreenCaptureKit shoots for `target`: its source, its width in
/// points, the display it lands on, and where on that display it was taken
/// from when it was an area or a window.
struct Shot {
    source: capture_core::Source,
    points_wide: f64,
    screen: (f32, f32),
    capture: Option<[f32; 4]>,
}

fn source(target: CaptureTarget) -> anyhow::Result<Shot> {
    use scrawl_capture::screens;
    let displays = screens::displays();
    let display_at = |at: [f64; 2]| {
        screens::index_at(&displays, at)
            .map(|i| &displays[i])
            .ok_or_else(|| anyhow::anyhow!("no display there"))
    };
    let screen = |display: &screens::Screen| (display.frame[2] as f32, display.frame[3] as f32);
    Ok(match target {
        CaptureTarget::Window { id } => {
            let bounds = scrawl_capture::windows::raw_windows()
                .into_iter()
                .find(|w| w.id == id)
                .map(|w| w.bounds);
            let center = bounds.map_or([0.0, 0.0], |[x, y, w, h]| [x + w * 0.5, y + h * 0.5]);
            let display = display_at(center)?;
            let [dx, dy, ..] = display.frame;
            Shot {
                source: capture_core::Source::Window(id),
                points_wide: bounds.map_or(0.0, |b| b[2]),
                screen: screen(display),
                capture: bounds
                    .map(|[x, y, w, h]| [(x - dx) as f32, (y - dy) as f32, w as f32, h as f32]),
            }
        }
        CaptureTarget::Pointer => {
            let at = screens::pointer().unwrap_or_default();
            let display = display_at(at)?;
            Shot {
                source: capture_core::Source::Display(display.id),
                points_wide: display.frame[2],
                screen: screen(display),
                capture: None,
            }
        }
        CaptureTarget::Phone => anyhow::bail!("a screenshot is never of an iPhone"),
        CaptureTarget::Display { at } => {
            let display = display_at(at)?;
            Shot {
                source: capture_core::Source::Display(display.id),
                points_wide: display.frame[2],
                screen: screen(display),
                capture: None,
            }
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
            Shot {
                source: capture_core::Source::Region {
                    display: display.id,
                    rect,
                },
                points_wide: w,
                screen: screen(display),
                capture: Some([(x - dx) as f32, (y - dy) as f32, w as f32, h as f32]),
            }
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
    /// The record shortcut: opens the picker to record, or stops.
    Record,
    Cancel,
    Toggle,
    /// A key for the card under the pointer.
    Card(CardKey),
}

/// The two shortcuts the tap matches: a screenshot, and a recording.
#[derive(Clone, Copy, Debug, Default)]
struct Shortcuts {
    shot: Option<RuntimeHotkey>,
    record: Option<RuntimeHotkey>,
}

const ESCAPE: u16 = 53;
const SPACE: u16 = 49;
const RETURN: u16 = 36;
const KEY_C: u16 = 8;
const KEY_S: u16 = 1;
const KEY_E: u16 = 14;
const KEY_W: u16 = 13;
/// Command, Shift, Control, and Option in a key's flags.
const COMMAND: u64 = 0x10_0000;
const MODIFIERS: u64 = 0x1E_0000;

/// What a key does to the card under the pointer, as CleanShot's overlay
/// takes them: ⌘C, ⌘S, ⌘E, ⌘W, and Escape and Return alone.
fn card_key(code: u16, flags: u64) -> Option<CardKey> {
    match (code, flags & MODIFIERS) {
        (KEY_C, COMMAND) => Some(CardKey::Copy),
        (KEY_S, COMMAND) => Some(CardKey::Reveal),
        (KEY_E, COMMAND) => Some(CardKey::Edit),
        (KEY_W, COMMAND) | (ESCAPE, 0) => Some(CardKey::Close),
        (RETURN, 0) => Some(CardKey::Keep),
        _ => None,
    }
}

/// The keys this took the press of, so their releases go too.
#[derive(Default)]
struct Taken(u128);

impl Taken {
    fn decide(
        &mut self,
        key: Key,
        shortcuts: Shortcuts,
        picking: bool,
        card: bool,
        suspended: bool,
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
        if suspended {
            return (Take::No, Effect::None);
        }
        let pressed = |hotkey: Option<RuntimeHotkey>| {
            hotkey.is_some_and(|h| h.matches_key_press(key.code, key.flags))
        };
        let (shot, record) = (pressed(shortcuts.shot), pressed(shortcuts.record));
        let effect = match key.code {
            _ if (shot || record) && picking => Effect::None,
            _ if shot => Effect::Begin,
            _ if record => Effect::Record,
            ESCAPE if picking => Effect::Cancel,
            SPACE if picking => Effect::Toggle,
            code if card && !picking => match card_key(code, key.flags) {
                Some(card) => Effect::Card(card),
                None => return (Take::No, Effect::None),
            },
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
    let shortcuts = Shortcuts {
        shot: crate::app_settings::screenshot_hotkey(),
        record: crate::app_settings::screenshot_record_hotkey(),
    };
    let (take, effect) = taken.decide(
        key,
        shortcuts,
        picking,
        crate::rune_ui::screenshot_thumbnail::hovered(),
        crate::app_settings::shortcuts_suspended(),
    );
    drop(taken);
    match effect {
        Effect::None => {}
        Effect::Begin => {
            begin();
        }
        Effect::Record => {
            record();
        }
        Effect::Cancel => cancel(),
        Effect::Toggle => toggle(),
        Effect::Card(card) => {
            if let Some(overlays) = OVERLAYS.get() {
                overlays.thumbnail.key(card);
            }
        }
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

    fn hotkey() -> Shortcuts {
        Shortcuts {
            shot: Some(HotkeyBinding::screenshot_default().runtime()),
            record: None,
        }
    }

    #[test]
    fn the_shortcut_opens_the_picker_and_its_release_stays_with_it() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), hotkey(), false, false, false),
            (Take::AfterListener, Effect::Begin)
        );
        assert_eq!(
            taken.decide(release(21), hotkey(), true, false, false),
            (Take::AfterListener, Effect::None)
        );
        assert_eq!(taken.0, 0);
    }

    #[test]
    fn a_held_shortcut_opens_one_picker() {
        let mut taken = Taken::default();
        let mut repeat = press(21, COMMAND_SHIFT);
        repeat.repeat = true;
        taken.decide(press(21, COMMAND_SHIFT), hotkey(), false, false, false);
        assert_eq!(
            taken.decide(repeat, hotkey(), true, false, false),
            (Take::AfterListener, Effect::None)
        );
    }

    #[test]
    fn escape_and_space_belong_to_the_open_picker_only() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(ESCAPE, 0), hotkey(), false, false, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(SPACE, 0), hotkey(), true, false, false),
            (Take::AfterListener, Effect::Toggle)
        );
        assert_eq!(
            taken.decide(press(ESCAPE, 0), hotkey(), true, false, false),
            (Take::Only, Effect::Cancel)
        );
        // Escape's release goes with it even once the picker closed.
        assert_eq!(
            taken.decide(release(ESCAPE), hotkey(), false, false, false),
            (Take::Only, Effect::None)
        );
    }

    #[test]
    fn other_keys_and_other_chords_pass() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(21, 0x10_0000), hotkey(), false, false, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(0, 0), hotkey(), true, false, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(
                press(21, COMMAND_SHIFT),
                Shortcuts::default(),
                false,
                false,
                false
            ),
            (Take::No, Effect::None)
        );
    }

    #[test]
    fn the_record_shortcut_records_and_never_while_the_picker_shows() {
        let mut taken = Taken::default();
        let shortcuts = Shortcuts {
            record: Some(HotkeyBinding::screenshot_default().runtime()),
            shot: None,
        };
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), shortcuts, false, false, false),
            (Take::AfterListener, Effect::Record)
        );
        taken.decide(release(21), shortcuts, false, false, false);
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), shortcuts, true, false, false),
            (Take::AfterListener, Effect::None)
        );
    }

    #[test]
    fn a_hovered_card_takes_its_keys_and_only_then() {
        let mut taken = Taken::default();
        let command = |code| press(code, COMMAND | 0x8);
        assert_eq!(
            taken.decide(command(KEY_C), hotkey(), false, true, false),
            (Take::AfterListener, Effect::Card(CardKey::Copy))
        );
        taken.decide(release(KEY_C), hotkey(), false, true, false);
        assert_eq!(
            taken.decide(command(KEY_W), hotkey(), false, true, false),
            (Take::AfterListener, Effect::Card(CardKey::Close))
        );
        assert_eq!(
            taken.decide(press(ESCAPE, 0), hotkey(), false, true, false),
            (Take::Only, Effect::Card(CardKey::Close))
        );
        assert_eq!(
            taken.decide(press(RETURN, 0), hotkey(), false, true, false),
            (Take::AfterListener, Effect::Card(CardKey::Keep))
        );
        // Off the cards, with another modifier, or while picking: not ours.
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(command(KEY_C), hotkey(), false, false, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(KEY_C, COMMAND_SHIFT), hotkey(), false, true, false),
            (Take::No, Effect::None)
        );
        assert_eq!(
            taken.decide(press(ESCAPE, 0), hotkey(), true, true, false),
            (Take::Only, Effect::Cancel)
        );
    }

    #[test]
    fn recording_a_shortcut_takes_nothing() {
        let mut taken = Taken::default();
        assert_eq!(
            taken.decide(press(21, COMMAND_SHIFT), hotkey(), false, false, true),
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
