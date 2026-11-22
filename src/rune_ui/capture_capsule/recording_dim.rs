//! The recording dim: while a Labs › Screenshots recording of an area or a
//! window records, every display dims but what's recorded, outlined as the
//! picker outlined it, so it's plain what the movie holds. One click-through
//! overlay per display, never taking focus, and left out of the recording
//! unless `capture.show_overlays_in_recordings` asks for Hex's overlays.
//!
//! [`crate::screenshots`] decides when it shows and where the target is;
//! this draws it with the picker's own [`picker::dim`].

use std::time::Duration;

use rune::{Cx, El, OverlayHandle, OverlayInput, Tx, el};

use super::picker::{self, FADE, Kind};
use crate::capture_host::DIM_WINDOWS;
use crate::rune_ui::controls::Paint;

/// How long the fade out gets before the overlay hides.
const HIDE_AFTER_S: f64 = 0.35;

/// One display's dim.
pub(crate) struct DimState {
    /// The display it covers, a global frame in points.
    display: [f32; 4],
    kind: Kind,
    /// What's recorded, a global rect in points.
    target: [f32; 4],
    /// False while it fades out (and before it fades in).
    on: bool,
    /// Shown at its hidden pose; fades in next poll.
    opening: bool,
    off_since: Option<f64>,
    shown: bool,
    overlay: Option<OverlayHandle<DimState>>,
    /// A still draws it over the picker's plain desktop.
    #[cfg(test)]
    backdrop: bool,
}

impl DimState {
    fn new() -> Self {
        Self {
            display: [0.0, 0.0, 1440.0, 900.0],
            kind: Kind::Area,
            target: [0.0; 4],
            on: false,
            opening: false,
            off_since: None,
            shown: false,
            overlay: None,
            #[cfg(test)]
            backdrop: false,
        }
    }

    /// Fully shown on `display` around `target`, for a still.
    #[cfg(test)]
    pub(crate) fn still(display: [f32; 4], kind: Kind, target: [f32; 4]) -> Self {
        Self {
            display,
            kind,
            target,
            on: true,
            backdrop: true,
            ..Self::new()
        }
    }

    /// Opens on `display` around `target`: mounted unseen, fading in next
    /// poll. Already showing, it stays and moves there.
    fn open(&mut self, display: [f32; 4], kind: Kind, target: [f32; 4], tx: &mut Tx) {
        let moved = self.display != display;
        self.display = display;
        self.kind = kind;
        self.target = target;
        self.off_since = None;
        if let Some(overlay) = &self.overlay
            && (moved || !self.shown)
        {
            overlay.set_placement(Some(picker::placement(display)));
            overlay.resize((display[2], display[3]));
        }
        tx.instant();
        if !self.on {
            self.opening = true;
            tx.wake_after(Duration::ZERO);
        }
    }

    /// The recorded window moved: the hole follows at once, as the movie
    /// does.
    fn follow(&mut self, target: [f32; 4], tx: &mut Tx) {
        if self.target != target {
            self.target = target;
            tx.instant();
        }
    }

    /// Fades out, then hides.
    fn close(&mut self, tx: &mut Tx) {
        self.opening = false;
        if self.on {
            self.on = false;
            tx.animated();
        }
        tx.wake_after(Duration::ZERO);
    }
}

fn poll(s: &mut DimState, now: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if s.opening && s.shown {
        // The hidden pose is on screen: fade in from it.
        s.opening = false;
        s.on = true;
        tx.animated();
        changed = true;
    }
    let off = !s.on && !s.opening;
    let since = match (off, s.off_since) {
        (true, None) => *s.off_since.insert(now),
        (true, Some(since)) => since,
        (false, _) => {
            s.off_since = None;
            now
        }
    };
    let leaving = since + HIDE_AFTER_S - now;
    if off && leaving > 0.0 {
        tx.wake_after(Duration::from_secs_f64(leaving));
    }
    let shown = !off || leaving > 0.0;
    if s.shown != shown {
        s.shown = shown;
        if let Some(overlay) = &s.overlay {
            if shown {
                overlay.show();
            } else {
                overlay.hide();
            }
        }
        if shown && s.opening {
            tx.wake_after(Duration::ZERO);
        }
    }
    changed
}

pub(crate) fn view(s: &DimState, cx: &mut Cx<DimState>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let [dx, dy, w, h] = s.display;
    let [x, y, tw, th] = s.target;
    let root = el().key("recording-dim").size(w, h);
    #[cfg(test)]
    let root = if s.backdrop {
        root.child(picker::desk(p, s.display, &picker::tests::windows()))
    } else {
        root
    };
    root.child(
        el().key("layer")
            .trace_name("recording dim")
            .absolute(0.0, 0.0)
            .size(w, h)
            .opacity(if s.on { 1.0 } else { 0.0 })
            .opacity_motion(FADE)
            .child(picker::dim(p, s.kind, [x - dx, y - dy, tw, th], false)),
    )
}

/// Every display's dim, as [`crate::screenshots`] holds it.
#[derive(Clone)]
pub(crate) struct RecordingDim {
    overlays: Vec<OverlayHandle<DimState>>,
}

impl RecordingDim {
    /// Adds one overlay per display Hex dims, hidden, holding no window or
    /// GPU surface until it shows.
    pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> Self {
        use rune::{OverlayLevel, OverlayOptions, OverlayPlacement, OverlaySpec};
        let overlays = (0..DIM_WINDOWS)
            .map(|n| {
                let spec = OverlaySpec::new("Hex recording dim", (1440.0, 900.0), view)
                    .options(OverlayOptions {
                        placement: Some(OverlayPlacement::FullScreen),
                        display: picker::DISPLAY,
                        // The picker's level: over the menu bar, under the
                        // capsule and its stop button.
                        level: OverlayLevel::AboveStatus,
                        exclude_from_capture: !crate::capture_host::own_windows_recorded(),
                        input: OverlayInput::None,
                        prewarm: false,
                        ..Default::default()
                    })
                    .release_gpu_when_hidden()
                    .poll(poll)
                    .on_created(move |c| {
                        if let Some(window) = c.window_number {
                            crate::capture_host::exclude_window(
                                crate::capture_host::OwnWindow::Dim(n),
                                window,
                            );
                        }
                    });
                let overlay = app.overlay(spec, DimState::new());
                let handle = overlay.clone();
                overlay.update(move |s, _| s.overlay = Some(handle));
                overlay
            })
            .collect();
        Self { overlays }
    }

    /// Dims every display in `displays` (global frames in points) but
    /// `target`. Safe from any thread.
    pub(crate) fn show(&self, displays: &[[f32; 4]], kind: Kind, target: [f32; 4]) {
        self.show_in_recordings(crate::capture_host::own_windows_recorded());
        for (n, overlay) in self.overlays.iter().enumerate() {
            match displays.get(n).copied() {
                Some(display) => overlay.update(move |s, tx| s.open(display, kind, target, tx)),
                None => overlay.update(DimState::close),
            }
        }
    }

    /// Moves the hole to `target`, the recorded window's new place.
    pub(crate) fn follow(&self, target: [f32; 4]) {
        for overlay in &self.overlays {
            overlay.update(move |s, tx| s.follow(target, tx));
        }
    }

    /// Fades every display's dim out.
    pub(crate) fn hide(&self) {
        for overlay in &self.overlays {
            overlay.update(DimState::close);
        }
    }

    /// Lets screen recordings see the dim, or keeps it out.
    pub(crate) fn show_in_recordings(&self, shown: bool) {
        for overlay in &self.overlays {
            overlay.set_exclude_from_capture(!shown);
        }
    }
}
