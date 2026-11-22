//! When the recording dim shows ([`RecordingDim`]): once a recording of
//! the area or window the picker picked has started, until it stops or is
//! cancelled. A window recording follows the window, as ScreenCaptureKit's
//! window filter does, so the hole follows it too; an area stays put.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::capture_host::{CaptureSnapshot, CaptureTarget};
use crate::rune_ui::capture_capsule::picker::Kind;
use crate::rune_ui::capture_capsule::recording_dim::RecordingDim;

static DIM: OnceLock<RecordingDim> = OnceLock::new();
/// Bumped by each pick: an older pick's watcher stops.
static PICKS: AtomicU64 = AtomicU64::new(0);

/// How long a pick waits for its recording to start.
const START_WITHIN: Duration = Duration::from_secs(10);
/// How often a window recording's hole looks for the window: about a
/// frame, so it keeps up with a drag.
const FOLLOW_EVERY: Duration = Duration::from_millis(16);
/// How often an area recording looks for its stop between changes.
const LOOK_EVERY: Duration = Duration::from_millis(250);

pub(super) fn install<S: 'static>(app: &mut rune::App<S>) {
    let _ = DIM.set(RecordingDim::install(app));
}

/// What a pick records, as the dim needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Recorded {
    /// A global rect in points.
    Area([f32; 4]),
    Window(u32),
}

impl Recorded {
    /// What the dim outlines for `target`; nothing for a whole display,
    /// which the dim would only darken.
    fn of(target: CaptureTarget) -> Option<Self> {
        match target {
            CaptureTarget::Area { rect } => Some(Self::Area(rect.map(|v| v as f32))),
            CaptureTarget::Window { id } => Some(Self::Window(id)),
            _ => None,
        }
    }

    fn kind(self) -> Kind {
        match self {
            Self::Area(_) => Kind::Area,
            Self::Window(_) => Kind::Window,
        }
    }

    /// Where it is now, in global points; `None` once a window is gone.
    fn rect(self) -> Option<[f32; 4]> {
        match self {
            Self::Area(rect) => Some(rect),
            Self::Window(id) => {
                scrawl_capture::windows::window_bounds(id).map(|bounds| bounds.map(|v| v as f32))
            }
        }
    }
}

/// What the snapshot says of the recording a pick started.
#[derive(Debug, PartialEq)]
enum Seen {
    /// Not recording the screen yet.
    Waiting,
    Recording(PathBuf),
    /// It stopped, was cancelled, or another capture records instead.
    Over,
}

/// `snapshot` for a pick whose recording is `bundle`, once it started.
fn seen(snapshot: &CaptureSnapshot, bundle: Option<&Path>) -> Seen {
    match (&snapshot.recording, bundle) {
        (Some(recording), Some(bundle)) if recording.bundle == bundle => {
            Seen::Recording(recording.bundle.clone())
        }
        (_, Some(_)) => Seen::Over,
        (Some(recording), None) if !recording.clip => Seen::Over,
        (Some(recording), None) if recording.screen => Seen::Recording(recording.bundle.clone()),
        (_, None) => Seen::Waiting,
    }
}

/// The picker picked `target` to record: the dim shows once its recording
/// has started, on its own thread. Safe from any thread.
pub(super) fn expect(target: CaptureTarget) {
    let pick = PICKS.fetch_add(1, Ordering::AcqRel) + 1;
    let (Some(dim), Some(recorded)) = (DIM.get().cloned(), Recorded::of(target)) else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hex-recording-dim".into())
        .spawn(move || watch(&dim, recorded, pick));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not watch the recording for its dim");
    }
}

fn current(pick: u64) -> bool {
    PICKS.load(Ordering::Acquire) == pick
}

fn watch(dim: &RecordingDim, recorded: Recorded, pick: u64) {
    let changes = crate::capture_host::watch();
    let deadline = Instant::now() + START_WITHIN;
    let bundle = loop {
        if !current(pick) {
            return;
        }
        match seen(&crate::capture_host::snapshot(), None) {
            Seen::Recording(bundle) => break bundle,
            Seen::Over => return,
            Seen::Waiting => {}
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        changes.wait(left.min(LOOK_EVERY));
    };
    let Some(mut rect) = recorded.rect() else {
        return;
    };
    dim.show(&displays(), recorded.kind(), rect);
    let mut shown_in_recordings = crate::capture_host::own_windows_recorded();
    let every = match recorded {
        Recorded::Area(_) => LOOK_EVERY,
        Recorded::Window(_) => FOLLOW_EVERY,
    };
    loop {
        if !current(pick) {
            return;
        }
        if seen(&crate::capture_host::snapshot(), Some(&bundle)) == Seen::Over {
            dim.hide();
            return;
        }
        if let Some(now) = recorded.rect()
            && now != rect
        {
            rect = now;
            dim.follow(rect);
        }
        let recorded_now = crate::capture_host::own_windows_recorded();
        if recorded_now != shown_in_recordings {
            shown_in_recordings = recorded_now;
            dim.show_in_recordings(recorded_now);
        }
        changes.wait(every);
    }
}

/// Every display's global frame, in points.
fn displays() -> Vec<[f32; 4]> {
    scrawl_capture::screens::displays()
        .iter()
        .map(|display| display.frame.map(|v| v as f32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::CaptureInstant;
    use crate::capture_host::{RecordingCapture, TargetKind};

    fn recording(bundle: &str, clip: bool, screen: bool) -> CaptureSnapshot {
        CaptureSnapshot {
            recording: Some(RecordingCapture {
                bundle: PathBuf::from(bundle),
                since: CaptureInstant::from_nanos(0),
                screen,
                target: TargetKind::Area,
                worked: false,
                clip,
            }),
            ..CaptureSnapshot::default()
        }
    }

    #[test]
    fn the_dim_waits_for_the_screen_and_ends_with_its_recording() {
        let idle = CaptureSnapshot::default();
        assert_eq!(seen(&idle, None), Seen::Waiting, "not started yet");
        assert_eq!(
            seen(&recording("a", true, false), None),
            Seen::Waiting,
            "starting, the screen not yet recorded"
        );
        assert_eq!(
            seen(&recording("a", true, true), None),
            Seen::Recording(PathBuf::from("a"))
        );
        assert_eq!(
            seen(&recording("a", false, true), None),
            Seen::Over,
            "an Annotate capture is never dimmed"
        );
        let a = Path::new("a");
        assert_eq!(
            seen(&recording("a", true, true), Some(a)),
            Seen::Recording(PathBuf::from("a"))
        );
        assert_eq!(seen(&idle, Some(a)), Seen::Over, "stopped or cancelled");
        assert_eq!(
            seen(&recording("b", true, true), Some(a)),
            Seen::Over,
            "another recording"
        );
    }

    #[test]
    fn only_an_area_or_a_window_is_dimmed() {
        assert_eq!(
            Recorded::of(CaptureTarget::Area {
                rect: [10.0, 20.0, 300.0, 200.0]
            }),
            Some(Recorded::Area([10.0, 20.0, 300.0, 200.0]))
        );
        assert_eq!(
            Recorded::of(CaptureTarget::Window { id: 7 }),
            Some(Recorded::Window(7))
        );
        assert_eq!(
            Recorded::of(CaptureTarget::Display { at: [0.0, 0.0] }),
            None,
            "a full-screen recording keeps the screen as it is"
        );
    }
}
