//! The whole surface arriving and leaving, and a line of it changing its
//! words. Closing a capsule is true the moment it happens: `kind` is `None`,
//! nothing in it takes input and no command can come from it. What it showed
//! stays drawn as it goes, and the window hides once it has gone.
//!
//! A surface arrives and leaves in place, about its saved corner. A line
//! whose words change lets the old words go before the new ones arrive, so
//! a worker that reports several titles within a few frames shows the last
//! one once, never a pile of them.

use super::stage::{ENTER, EXIT, Presence, REDUCED, Track};
use super::*;

/// How far the surface shrinks toward its corner, and softens, as it goes.
const SCALE: f32 = 0.96;
const BLUR: f32 = 3.0;
/// A line replaced in place softens this much.
const LINE_BLUR: f32 = 2.0;
/// Words this faint no longer show.
const GONE: f32 = 0.02;

#[derive(Default)]
pub(super) struct Surface {
    /// 0 hidden, 1 shown.
    track: Track,
    /// What the surface showed when it closed, drawn until it has gone.
    pub(super) ghost: Option<Kind>,
    /// When it closed: a leaving recording's time stops there.
    left: Option<Instant>,
    last: Option<f64>,
    /// The finished meeting's title and summary as they are drawn.
    pub(super) title: Roll<String>,
    pub(super) summary: Roll<(String, bool)>,
}

impl Surface {
    /// A surface already on screen, at rest.
    #[cfg(test)]
    pub(super) fn shown() -> Self {
        Self {
            track: Track::at(1.0),
            ..Default::default()
        }
    }

    pub(super) fn leave(&mut self, kind: Option<Kind>) {
        if let Some(kind) = kind {
            self.ghost = Some(kind);
            self.left = Some(Instant::now());
        }
    }

    pub(super) fn leaving(&self) -> bool {
        self.ghost.is_some()
    }

    /// The time a recording's clock reads: now, or when its surface closed.
    pub(super) fn clock(&self) -> Instant {
        self.left
            .filter(|_| self.leaving())
            .unwrap_or_else(Instant::now)
    }

    #[cfg(test)]
    pub(super) fn settle(&mut self, shown: bool) {
        self.track = Track::at(f32::from(shown));
        self.last = None;
        if shown {
            self.title.settle();
            self.summary.settle();
        } else {
            self.ghost = None;
            self.left = None;
        }
    }

    fn look(&self, reduced: bool) -> Presence {
        let p = self.track.x.clamp(0.0, 1.0);
        if reduced {
            return Presence {
                opacity: p,
                scale: 1.0,
                blur: 0.0,
            };
        }
        Presence {
            opacity: p,
            scale: SCALE + (1.0 - SCALE) * p,
            blur: BLUR * (1.0 - p),
        }
    }
}

/// One line whose words can change. The words drawn are the last ones that
/// arrived whole; newer words wait until those have gone.
#[derive(Default)]
pub(super) struct Roll<T> {
    shown: Option<T>,
    track: Track,
}

impl<T: PartialEq + Clone> Roll<T> {
    /// Steps toward showing `wanted`; true while the line moves.
    fn step(&mut self, wanted: Option<T>, reduced: bool, dt: f32) -> bool {
        let Some(wanted) = wanted else {
            self.shown = None;
            return false;
        };
        let Some(shown) = &self.shown else {
            // A line's first words arrive with the line.
            self.shown = Some(wanted);
            self.track = Track::at(1.0);
            return false;
        };
        let same = *shown == wanted;
        let spring = match (reduced, same) {
            (true, _) => REDUCED,
            (false, true) => ENTER,
            (false, false) => EXIT,
        };
        let moving = self.track.step(f32::from(same), spring, dt, 0.01);
        // The old words no longer show: the new ones start from nothing,
        // without waiting out the spring's tail over an empty line.
        if !same && self.track.x < GONE {
            self.shown = Some(wanted);
            self.track = Track::at(0.0);
            return true;
        }
        moving
    }

    #[cfg(test)]
    fn settle(&mut self) {
        self.shown = None;
    }

    /// The words to draw in place of `now`, and how strongly.
    pub(super) fn line(&self, now: T, reduced: bool) -> (T, f32, f32) {
        match &self.shown {
            Some(shown) => {
                let p = self.track.x.clamp(0.0, 1.0);
                let blur = if reduced { 0.0 } else { LINE_BLUR * (1.0 - p) };
                (shown.clone(), p, blur)
            }
            None => (now, 1.0, 0.0),
        }
    }
}

impl Capsule {
    /// What the surface draws: what it shows, or what it showed as it goes.
    pub(super) fn drawn(&self) -> Option<&Kind> {
        self.kind.as_ref().or(self.surface.ghost.as_ref())
    }

    /// Shows `kind` in this capsule's window, in place of whatever it showed
    /// or was still drawing as it left. A hidden surface arrives; one still
    /// on screen carries on from the strength it has.
    pub(super) fn show(
        &mut self,
        kind: Kind,
        presentation: offer_surface::Presentation,
        corner: MeetingIndicatorCorner,
        timer: Option<offer::After>,
        tx: &mut Tx,
    ) {
        self.kind = Some(kind);
        self.presentation = presentation;
        self.stage = Default::default();
        self.opened = None;
        self.more_open = false;
        self.set_offer_timer(timer);
        self.live = Live::default();
        self.waveform = None;
        self.hovered = false;
        self.expired = false;
        self.corner = corner;
        self.throw.stop();
        // Nothing in it mounts with an entrance of its own.
        tx.instant();
        tx.wake_after(Duration::ZERO);
    }

    /// Closes from an update: the surface starts to go in the next frame.
    pub(super) fn dismiss(&mut self, tx: &mut Tx) {
        self.close();
        tx.wake_after(Duration::ZERO);
    }

    /// Steps the surface's presence and its lines; true while they move.
    pub(super) fn step_surface(&mut self, now: f64, tx: &mut Tx) -> bool {
        let reduced = self.reduced.get();
        let shown = self.kind.is_some();
        if shown {
            self.surface.ghost = None;
            self.surface.left = None;
        }
        let (title, summary) = match self.drawn() {
            Some(Kind::Finalizing { .. }) => (Some(tr("Transcribing…").to_owned()), None),
            Some(Kind::Ready(manifest)) => (
                Some(meeting::display_title(manifest)),
                panel::ready_summary(manifest),
            ),
            _ => (None, None),
        };
        let s = &mut self.surface;
        if !shown && s.ghost.is_none() {
            s.track = Track::at(0.0);
            s.last = None;
            return false;
        }
        if shown
            && s.track == Track::at(0.0)
            && let Some(show) = &self.show
        {
            // Asked for from here too, so a hide this capsule requested
            // earlier can't outlast what it shows now.
            show();
        }
        let dt = s
            .last
            .map_or(1.0 / 60.0, |last| (now - last).clamp(0.0, 0.1) as f32);
        let spring = match (reduced, shown) {
            (true, _) => REDUCED,
            (false, true) => ENTER,
            (false, false) => EXIT,
        };
        // Below one part in a hundred nothing shows: the window can go.
        let mut moving = s.track.step(f32::from(shown), spring, dt, 0.01);
        moving |= s.title.step(title, reduced, dt);
        moving |= s.summary.step(summary, reduced, dt);
        if !shown && s.track == Track::at(0.0) {
            // Gone: nothing is drawn, and only now does the window hide.
            s.ghost = None;
            s.left = None;
            s.last = None;
            self.stage = Default::default();
            self.opened = None;
            if let Some(hide) = &self.hide {
                hide();
            }
            return true;
        }
        s.last = moving.then_some(now);
        if moving {
            tx.wake_after(Duration::ZERO);
        }
        moving
    }
}

/// The window's contents as the surface presents them: arriving, at rest,
/// or leaving. What is leaving takes no input.
pub(super) fn present(capsule: &Capsule, body: El) -> El {
    let leaving = capsule.surface.leaving();
    let look = capsule.surface.look(capsule.reduced.get());
    // Set at rest too, so the last step to rest lands and isn't eased again.
    let body = body
        .opacity(look.opacity)
        .opacity_motion(Anim::INSTANT)
        .blur(look.blur)
        .blur_motion(Anim::INSTANT);
    if leaving {
        body.inert().a11y_hidden()
    } else {
        body
    }
}

/// The surface itself, drawn about its saved corner as it arrives or
/// leaves: the anchored edges stay exactly where they rest.
pub(super) fn anchored(capsule: &Capsule, surface: El) -> El {
    let look = capsule.surface.look(capsule.reduced.get());
    surface
        .origin(
            if capsule.corner.is_right() { 1.0 } else { 0.0 },
            if capsule.corner.is_top() { 0.0 } else { 1.0 },
        )
        .scale(look.scale)
        .scale_motion(Anim::INSTANT)
}
