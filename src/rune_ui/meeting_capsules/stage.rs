//! The recording host's one surface. Its outline and the presence of what is
//! drawn in it are tracks stepped here: a change of state gives a track a new
//! target and the track bends toward it from where it is, at the speed it
//! has. Nothing restarts, and nothing waits for a track: the controller's
//! state is true the moment it arrives and the surface catches up.
//!
//! Content is laid out at its final size, anchored to the saved corner; only
//! the outline moves over it. While an edge moves, what it crosses fades
//! under it in proportion to the edge's speed, so nothing is cut hard in
//! flight and nothing is dimmed at rest.

use super::*;

/// The outline: the body opens and closes on these, and the width leads.
const OPEN: Spring = Spring::new(0.42, 0.0);
const CLOSE: Spring = Spring::new(0.38, 0.0);
/// The width's share of the body's duration (0.37 beside 0.45).
const WIDTH_LEAD: f32 = 0.82;
/// What arrives, and what leaves (sooner).
pub(super) const ENTER: Spring = Spring::new(0.3, 0.0);
pub(super) const EXIT: Spring = Spring::new(0.25, 0.0);
/// Reduce Motion: no travel, a short fade.
pub(super) const REDUCED: Spring = Spring::new(0.18, 0.0);
/// Details follow the main change, most important first.
pub(super) const STAGGER: f64 = 0.04;
/// An edge at full speed fades this much of what it crosses.
const VEIL: f32 = 24.0;
/// Seconds of an edge's travel its fade covers.
const VEIL_REACH: f32 = 0.035;
/// The fade follows the edge's speed over this long, so it never pops on.
const VEIL_EASE: f32 = 0.05;
/// How far an arriving or leaving layer, or the whole surface, shrinks
/// and softens.
const LAYER_SCALE: f32 = 0.96;
const LAYER_BLUR: f32 = 3.0;

/// The spring a presence arrives or leaves on: it leaves sooner, and under
/// Reduce Motion the one flat spring stands in for both.
pub(super) fn spring(reduced: bool, arrived: bool) -> Spring {
    match (reduced, arrived) {
        (true, _) => REDUCED,
        (false, true) => ENTER,
        (false, false) => EXIT,
    }
}

/// Sizes as drawn land on the 1/64-point grid, so a moving outline and the
/// layout it uncovers agree at rest.
pub(super) fn grid(v: f32) -> f32 {
    (v * 64.0).round() / 64.0
}

/// How long a track in full flight takes to believe a new target. A retarget
/// changes where the spring pulls, not how hard it pulls this frame, so the
/// edge's speed bends round instead of dropping in one frame.
const RETARGET: f32 = 0.05;
/// A track carrying less than this share of the speed its spring would
/// reach on the new move has no momentum worth bending: it just leaves.
const MOMENTUM: f32 = 0.25;
/// The pull moves within a frame, so a frame is stepped in this many parts.
const SUBSTEPS: usize = 4;

/// One sprung value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Track {
    pub(super) x: f32,
    pub(super) v: f32,
    /// Where the spring pulls now: the target, once the track believes it.
    pull: f32,
    to: f32,
    /// How long the pull takes to reach `to`, set when the target changed.
    ease: f32,
}

impl Track {
    pub(super) const fn at(x: f32) -> Self {
        Self {
            x,
            v: 0.0,
            pull: x,
            to: x,
            ease: 0.0,
        }
    }

    /// Whether it is already on its way to `to`, or resting there.
    pub(super) fn heads_for(&self, to: f32) -> bool {
        self.to == to
    }

    /// Bends toward `to` from where it is; true while it still moves.
    /// `rest` is the distance that no longer shows.
    pub(super) fn step(&mut self, to: f32, spring: Spring, dt: f32, rest: f32) -> bool {
        if self.x == to && self.v == 0.0 {
            *self = Self::at(to);
            return false;
        }
        if to != self.to {
            // From rest a track leaves as its spring does. In flight the
            // pull travels to the new target, as slowly as the track's
            // momentum is large, so its acceleration has no step in it.
            let reach = std::f32::consts::TAU / spring.duration * (to - self.x).abs();
            let carried = (self.v.abs() / (MOMENTUM * reach).max(f32::EPSILON)).min(1.0);
            self.to = to;
            self.ease = RETARGET * carried;
        }
        let h = dt / SUBSTEPS as f32;
        for _ in 0..SUBSTEPS {
            self.pull = if self.ease < 1e-4 {
                to
            } else {
                to + (self.pull - to) * (-h / self.ease).exp()
            };
            (self.x, self.v) = spring.sample(self.x, self.v, self.pull, h);
        }
        if (self.x - to).abs() < rest && self.v.abs() < rest * 20.0 && (self.pull - to).abs() < rest
        {
            *self = Self::at(to);
        }
        true
    }
}

/// Where the surface is heading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Outline {
    pub(super) w: f32,
    pub(super) h: f32,
    pub(super) radius: f32,
    /// 0 follows the host's appearance; 1 is the recording's dark surface.
    pub(super) dark: f32,
}

/// What can be drawn in the surface. Each has a presence from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Layer {
    /// The detected call and its actions.
    Offer,
    /// "Starting…", once a start has been slow enough to need saying.
    Pending,
    /// The listening field and timer.
    Meter,
    /// Stop and the two copies, in order of importance.
    Action(usize),
}

pub(super) const ACTIONS: usize = 3;
const LAYERS: usize = 3 + ACTIONS;

impl Layer {
    fn index(self) -> usize {
        match self {
            Layer::Offer => 0,
            Layer::Pending => 1,
            Layer::Meter => 2,
            Layer::Action(row) => 3 + row.min(ACTIONS - 1),
        }
    }
}

/// The state a frame steps toward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Target {
    pub(super) outline: Outline,
    pub(super) offer: bool,
    /// Since when "Starting…" has been wanted.
    pub(super) pending: Option<f64>,
    pub(super) meter: bool,
    /// Since when the actions have been wanted.
    pub(super) actions: Option<f64>,
    /// Stop is one of them. Once capture has ended it leaves and the copies stay.
    pub(super) stop: bool,
    /// How wide the listening field is wanted: its resting width, or the
    /// open header's room.
    pub(super) field: f32,
}

impl Target {
    fn wants(&self, layer: Layer, now: f64) -> bool {
        match layer {
            Layer::Offer => self.offer,
            Layer::Pending => self.pending.is_some_and(|since| now >= since),
            Layer::Meter => self.meter,
            Layer::Action(row) => {
                (row > 0 || self.stop)
                    && self
                        .actions
                        .is_some_and(|since| now >= since + row as f64 * STAGGER)
            }
        }
    }
}

/// A layer as it is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Presence {
    pub(super) opacity: f32,
    pub(super) scale: f32,
    pub(super) blur: f32,
}

impl Presence {
    /// Opacity alone: under Reduce Motion nothing travels or softens.
    pub(super) fn flat(opacity: f32) -> Self {
        Self {
            opacity,
            scale: 1.0,
            blur: 0.0,
        }
    }

    /// Shrinks toward its corner and softens as it leaves: how the whole
    /// surface and its soft layers arrive and go.
    pub(super) fn soft(opacity: f32) -> Self {
        Self {
            opacity,
            scale: LAYER_SCALE + (1.0 - LAYER_SCALE) * opacity,
            blur: LAYER_BLUR * (1.0 - opacity),
        }
    }

    pub(super) fn shows(self) -> bool {
        self.opacity > 0.0
    }

    /// Enough of it shows to act on.
    pub(super) fn takes_input(self) -> bool {
        self.opacity >= 0.5
    }
}

/// What one step of the stage found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stepped {
    /// Something drawn moved.
    Moved,
    /// Nothing drawn moved, but a layer waits its turn: the clock runs on.
    Waiting,
    /// At rest.
    Still,
}

#[derive(Default)]
pub(super) struct Stage {
    /// Width, height, radius, dark. `None` until something shows: a surface
    /// appears at rest, it doesn't animate its first frame.
    outline: Option<[Track; 4]>,
    /// The listening field's width, on the outline's width spring: the
    /// field's free end and the wall it follows are one motion.
    field: Track,
    layers: [Track; LAYERS],
    last: Option<f64>,
    /// When the outline last followed another motion.
    followed: Option<f64>,
    /// The outline itself is in flight, not only a layer.
    travelling: bool,
    /// When the pointer opened the recording, for its rows' order of arrival.
    pub(super) opened: Option<f64>,
    veil: (f32, f32),
    reduced: bool,
}

impl Stage {
    pub(super) fn outline(&self) -> Option<Outline> {
        self.outline.map(|[w, h, radius, dark]| Outline {
            w: w.x,
            h: h.x,
            radius: radius.x,
            dark: dark.x,
        })
    }

    /// How wide the listening field has opened.
    pub(super) fn field(&self) -> f32 {
        self.field.x
    }

    /// Points per second: how fast the width and height are changing.
    #[cfg(test)]
    pub(super) fn velocity(&self) -> (f32, f32) {
        self.outline.map_or((0.0, 0.0), |[w, h, ..]| (w.v, h.v))
    }

    #[cfg(test)]
    pub(super) fn resting(&self) -> bool {
        self.last.is_none()
    }

    /// The outline is on its way somewhere.
    pub(super) fn travelling(&self) -> bool {
        self.travelling
    }

    /// How much of what the moving vertical and horizontal edges cross fades
    /// under them: nothing at rest, more the faster they travel.
    pub(super) fn veil(&self) -> (f32, f32) {
        self.veil
    }

    fn ease_veil(&mut self, dt: f32) {
        let Some([w, h, ..]) = self.outline else {
            return;
        };
        let reach = |track: Track, size: f32| {
            (track.v.abs() * VEIL_REACH)
                .min(VEIL)
                .min(size * 0.5)
                .max(0.0)
        };
        let ease = 1.0 - (-dt / VEIL_EASE).exp();
        let toward = |from: f32, to: f32| {
            let next = from + (to - from) * ease;
            if to == 0.0 && next < 0.05 { 0.0 } else { next }
        };
        self.veil = if self.reduced {
            (0.0, 0.0)
        } else {
            (
                toward(self.veil.0, reach(w, w.x)),
                toward(self.veil.1, reach(h, h.x)),
            )
        };
    }

    pub(super) fn presence(&self, layer: Layer) -> Presence {
        let p = self.layers[layer.index()].x.clamp(0.0, 1.0);
        // The meter and the actions stay where they are: they only fade.
        if matches!(layer, Layer::Offer | Layer::Pending) && !self.reduced {
            Presence::soft(p)
        } else {
            Presence::flat(p)
        }
    }

    /// The surface as `target` describes it, at rest.
    pub(super) fn land(&mut self, target: &Target, now: f64) {
        let o = target.outline;
        self.outline = Some([o.w, o.h, o.radius, o.dark].map(Track::at));
        self.field = Track::at(target.field);
        for layer in Self::layers() {
            self.layers[layer.index()] = Track::at(f32::from(target.wants(layer, now)));
        }
        self.last = None;
        self.travelling = false;
        self.veil = (0.0, 0.0);
    }

    /// The outline follows something else's motion (the offer's own pages):
    /// it takes that place and remembers the speed it arrived with, so a
    /// change of state mid-way carries on from there.
    pub(super) fn follow(&mut self, target: &Target, now: f64) {
        let o = target.outline;
        let to = [o.w, o.h, o.radius, o.dark];
        let from = self.outline;
        let dt = self.followed.replace(now).map(|last| (now - last) as f32);
        self.land(target, now);
        if let (Some(from), Some(dt), Some(tracks)) = (
            from,
            dt.filter(|dt| *dt > 0.0 && *dt < 0.1),
            &mut self.outline,
        ) {
            for ((track, from), to) in tracks.iter_mut().zip(from).zip(to) {
                track.v = (to - from.x) / dt;
            }
        }
    }

    /// Steps every track toward `target`; what the step found.
    pub(super) fn step(&mut self, target: &Target, reduced: bool, now: f64) -> Stepped {
        self.reduced = reduced;
        self.followed = None;
        let Some(tracks) = &mut self.outline else {
            self.land(target, now);
            return Stepped::Moved;
        };
        let dt = self
            .last
            .map_or(1.0 / 60.0, |last| (now - last).clamp(0.0, 0.1) as f32);
        let o = target.outline;
        let mut moving = false;
        if reduced {
            let to = [o.w, o.h, o.radius, o.dark];
            for (track, to) in tracks.iter_mut().zip(to) {
                moving |= *track != Track::at(to);
                *track = Track::at(to);
            }
            moving |= self.field != Track::at(target.field);
            self.field = Track::at(target.field);
        } else {
            let body = if o.w * o.h >= tracks[0].x * tracks[1].x {
                OPEN
            } else {
                CLOSE
            };
            let width = body.with_duration(body.duration * WIDTH_LEAD);
            moving |= tracks[0].step(o.w, width, dt, 0.01);
            moving |= self.field.step(target.field, width, dt, 0.01);
            moving |= tracks[1].step(o.h, body, dt, 0.01);
            moving |= tracks[2].step(o.radius, body, dt, 0.01);
            moving |= tracks[3].step(o.dark, body, dt, 0.002);
        }
        self.travelling = moving;
        self.ease_veil(dt);
        moving |= self.veil != (0.0, 0.0);
        let mut waiting = false;
        for layer in Self::layers() {
            let wants = target.wants(layer, now);
            // A layer whose turn hasn't come keeps the clock running: it
            // wants to show, only not yet.
            waiting |= !wants && target.wants(layer, f64::INFINITY);
            let track = &mut self.layers[layer.index()];
            let spring = match (reduced, wants) {
                (true, true) => REDUCED,
                // The outline has already landed: what leaves would only be
                // cut by it, so it goes at once and the arrival fades.
                (true, false) => {
                    moving |= *track != Track::at(0.0);
                    *track = Track::at(0.0);
                    continue;
                }
                (false, true) => ENTER,
                (false, false) => EXIT,
            };
            moving |= track.step(f32::from(wants), spring, dt, 0.002);
        }
        self.last = (moving || waiting).then_some(now);
        if moving {
            Stepped::Moved
        } else if waiting {
            Stepped::Waiting
        } else {
            Stepped::Still
        }
    }

    fn layers() -> impl Iterator<Item = Layer> {
        [Layer::Offer, Layer::Pending, Layer::Meter]
            .into_iter()
            .chain((0..ACTIONS).map(Layer::Action))
    }
}

impl Capsule {
    /// Where the surface is heading for what the capsule shows now, and
    /// whether its outline only follows the offer's own pages.
    pub(super) fn stage_target(&mut self, now: f64) -> Option<(Target, bool)> {
        let pill = |w: f32| Outline {
            w,
            h: REST_HEIGHT,
            radius: REST_HEIGHT / 2.0,
            dark: 1.0,
        };
        let panel = |h: f32| Outline {
            w: PANEL_WIDTH,
            h,
            radius: PANEL_RADIUS,
            dark: 1.0,
        };
        let none = Target {
            outline: pill(REST_WIDTH),
            offer: false,
            pending: None,
            meter: false,
            actions: None,
            stop: true,
            field: waveform::WIDTH,
        };
        // A surface that is leaving says nothing it hadn't already said.
        let may_say = self.kind.is_some() || self.stage.presence(Layer::Pending).shows();
        let target = match self
            .kind
            .as_ref()
            .or(self.surface.ghost.as_ref().map(|(kind, _)| kind))
        {
            Some(Kind::Offer(..)) if self.presentation.departing() => Target {
                pending: may_say.then(|| self.presentation.begun(now) + PENDING_LABEL_AFTER),
                ..none
            },
            Some(Kind::Offer(..)) => {
                let target = Target {
                    outline: self.presentation.outline(self.reduced.get()),
                    offer: true,
                    ..none
                };
                return Some((target, true));
            }
            Some(Kind::Recording(shown)) => {
                let open = self.open();
                if !open {
                    self.stage.opened = None;
                }
                Target {
                    outline: if open {
                        panel(panel::open_height(shown))
                    } else {
                        pill(panel::rest_width(shown))
                    },
                    meter: true,
                    actions: open.then(|| *self.stage.opened.get_or_insert(now)),
                    field: panel::field_wanted(self),
                    ..none
                }
            }
            // Capture has ended: Stop leaves, and the surface finishes its
            // way to the open panel the finished meeting's layout grows from.
            Some(Kind::Finalizing { .. } | Kind::Ready(_)) => Target {
                outline: panel(panel::open_height_without_notice()),
                meter: true,
                actions: Some(self.stage.opened.unwrap_or(f64::MIN)),
                stop: false,
                field: panel::field_wanted(self),
                ..none
            },
            _ => return None,
        };
        Some((target, false))
    }

    /// Steps the surface toward what the capsule shows; true while it moves.
    pub(super) fn step_stage(&mut self, now: f64, tx: &mut Tx) -> bool {
        let reduced = self.reduced.get();
        let surface = self.step_surface(now, tx);
        let paging = matches!(self.drawn(), Some(Kind::Offer(..)))
            && !self.presentation.departing()
            && self.presentation.step(self.more_open, reduced, now);
        let Some((target, follow)) = self.stage_target(now) else {
            self.stage = Stage::default();
            return surface;
        };
        let stepped = if follow {
            self.stage.follow(&target, now);
            if paging {
                tx.instant();
            }
            if paging {
                Stepped::Moved
            } else {
                Stepped::Still
            }
        } else {
            self.stage.step(&target, reduced, now)
        };
        if matches!(self.kind, Some(Kind::Recording(_))) {
            self.presentation.arrived(&self.stage);
        }
        if stepped != Stepped::Still {
            tx.wake_after(Duration::ZERO);
        }
        surface || (stepped != Stepped::Still && (follow || stepped == Stepped::Moved))
    }

    /// The surface as it rests for what the capsule shows now.
    #[cfg(test)]
    pub(super) fn settle_stage(&mut self, now: f64) {
        if self.open() {
            self.stage.opened = Some(now - 10.0);
        }
        self.presentation.backdate(now - 10.0);
        match self.stage_target(now) {
            Some((target, _)) => self.stage.land(&target, now),
            None => self.stage = Stage::default(),
        }
        if matches!(self.kind, Some(Kind::Recording(_))) {
            self.presentation.arrived(&self.stage);
        }
        self.surface.settle(self.kind.is_some());
    }

    /// A handler changed what the capsule shows: the surface starts toward
    /// it in this frame, not at the next poll.
    pub(super) fn stir(&mut self, tx: &mut Tx) {
        let now = tx.clock();
        self.step_stage(now, tx);
        tx.wake_after(Duration::ZERO);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(w: f32, h: f32) -> Target {
        Target {
            outline: Outline {
                w,
                h,
                radius: 13.0,
                dark: 1.0,
            },
            offer: false,
            pending: None,
            meter: true,
            actions: None,
            stop: true,
            field: 78.0,
        }
    }

    /// The pill at rest, and the open panel it is stepped between.
    fn resting() -> Target {
        target(148.0, 36.0)
    }

    fn opened() -> Target {
        target(252.0, 227.0)
    }

    #[test]
    fn a_surface_appears_at_rest_and_moves_only_when_its_target_does() {
        let mut stage = Stage::default();
        assert_eq!(stage.step(&resting(), false, 0.0), Stepped::Moved);
        assert_eq!(stage.outline().unwrap().w, 148.0);
        assert_eq!(stage.presence(Layer::Meter).opacity, 1.0);
        assert_eq!(stage.step(&resting(), false, 1.0 / 60.0), Stepped::Still);
        assert!(stage.resting());
        assert_eq!(stage.veil(), (0.0, 0.0));
    }

    /// The original discontinuity: a reversal restarted from rest, so the
    /// edge stopped dead for a frame and the width and height moved as one.
    #[test]
    fn a_reversal_keeps_its_speed_and_the_width_leads() {
        let mut stage = Stage::default();
        let (rest, open) = (resting(), opened());
        stage.step(&rest, false, 0.0);
        let mut now = 0.0;
        let mut frame = |stage: &mut Stage, to: &Target| {
            now += 1.0 / 60.0;
            stage.step(to, false, now);
            (stage.outline().unwrap(), stage.velocity())
        };
        let mut last = (148.0, 0.0);
        for _ in 0..6 {
            let (o, (vw, vh)) = frame(&mut stage, &open);
            // Width leads: it has covered more of its way than the height.
            assert!((o.w - 148.0) / 104.0 > (o.h - 36.0) / 191.0);
            assert!(vw > 0.0 && vh > 0.0);
            last = (o.w, vw);
        }
        // Reverse mid-flight: the edge carries on from the same place before
        // it turns round. Restarting from rest would stop it dead here.
        let (o, _) = frame(&mut stage, &rest);
        assert!(
            o.w > last.0 && o.w - last.0 < last.1 / 60.0,
            "{} after {} at {} pt/s",
            o.w,
            last.0,
            last.1
        );
        let mut widths = vec![o.w];
        for _ in 0..90 {
            widths.push(frame(&mut stage, &rest).0.w);
        }
        let peak = widths.iter().cloned().fold(0.0, f32::max);
        let turn = widths.iter().position(|w| *w == peak).unwrap();
        assert!(widths[turn..].windows(2).all(|w| w[1] <= w[0]));
        assert_eq!(*widths.last().unwrap(), 148.0);
        assert!(stage.resting());
    }

    /// The first candidate handed a reversed track straight to its spring:
    /// an edge opening at 650 pt/s was closing a frame later. Its speed is
    /// continuous now and so is its acceleration.
    #[test]
    fn a_retarget_in_flight_bends_the_speed_and_a_launch_is_unchanged() {
        let spring = Spring::new(0.42, 0.0);
        let dt = 1.0 / 60.0;
        let mut track = Track::at(36.0);
        // From rest the track is its spring, to the last bit.
        track.step(219.0, spring, dt, 0.01);
        let launch = track.v;
        let (x, v) = spring.sample(36.0, 0.0, 219.0, dt);
        assert!((track.x - x).abs() < 0.01 && (track.v - v).abs() < 0.5);
        for _ in 0..7 {
            track.step(219.0, spring, dt, 0.01);
        }
        let mut speeds = vec![track.v];
        for _ in 0..40 {
            track.step(36.0, spring, dt, 0.01);
            speeds.push(track.v);
        }
        assert!(speeds[0] > 600.0, "{}", speeds[0]);
        // The frame after the reversal it still travels outward.
        assert!(speeds[1] > 0.0 && speeds[1] < speeds[0], "{speeds:?}");
        // No frame changes its speed by more than a launch from rest does.
        let steepest = speeds
            .windows(2)
            .map(|v| (v[1] - v[0]).abs())
            .fold(0.0, f32::max);
        assert!(steepest < launch, "{steepest} against a launch of {launch}");
        // It turns once and closes.
        let turn = speeds.iter().position(|v| *v < 0.0).unwrap();
        assert!(speeds[turn..].iter().all(|v| *v <= 0.0));
        for _ in 0..90 {
            track.step(36.0, spring, dt, 0.01);
        }
        assert_eq!(track, Track::at(36.0));

        // A track that has all but stopped has no momentum to bend: it
        // leaves for a new target as it would from rest.
        let mut slow = Track::at(36.0);
        for _ in 0..20 {
            slow.step(40.0, spring, dt, 0.0001);
        }
        assert!(slow.v.abs() < 5.0 && slow != Track::at(40.0), "{slow:?}");
        let before = slow;
        slow.step(219.0, spring, dt, 0.01);
        let (x, _) = spring.sample(before.x, before.v, 219.0, dt);
        assert!((slow.x - x).abs() < 0.05, "{} against {x}", slow.x);
    }

    #[test]
    fn a_moving_edge_fades_what_it_crosses_and_a_resting_one_cuts_nothing() {
        let mut stage = Stage::default();
        stage.step(&resting(), false, 0.0);
        let open = opened();
        let mut peak = (0.0f32, 0.0f32);
        let mut last = (0.0, 0.0);
        for frame in 1..=90 {
            stage.step(&open, false, frame as f64 / 60.0);
            let veil = stage.veil();
            // The fade never pops: it grows and shrinks with the edge's speed.
            assert!((veil.0 - last.0).abs() < 6.0 && (veil.1 - last.1).abs() < 6.0);
            peak = (peak.0.max(veil.0), peak.1.max(veil.1));
            last = veil;
        }
        assert!(peak.0 > 12.0 && peak.1 > 12.0 && peak.0 <= VEIL && peak.1 <= VEIL);
        assert_eq!(stage.veil(), (0.0, 0.0));
    }

    #[test]
    fn actions_arrive_in_order_and_leave_together() {
        let mut stage = Stage::default();
        stage.step(&resting(), false, 0.0);
        let open = Target {
            actions: Some(0.0),
            ..opened()
        };
        for frame in 1..=6 {
            stage.step(&open, false, frame as f64 / 60.0);
        }
        let shown: Vec<f32> = (0..ACTIONS)
            .map(|row| stage.presence(Layer::Action(row)).opacity)
            .collect();
        assert!(shown.windows(2).all(|p| p[0] > p[1]), "{shown:?}");
        assert!(stage.presence(Layer::Action(0)).blur == 0.0);
        let closed = resting();
        for frame in 7..=60 {
            stage.step(&closed, false, frame as f64 / 60.0);
        }
        assert!((0..ACTIONS).all(|row| !stage.presence(Layer::Action(row)).shows()));
    }

    #[test]
    fn a_fast_start_never_shows_the_pending_label() {
        let mut stage = Stage::default();
        let offer = Target {
            offer: true,
            meter: false,
            ..target(416.0, 44.0)
        };
        stage.step(&offer, false, 0.0);
        let pending = Target {
            pending: Some(0.18),
            meter: false,
            ..resting()
        };
        for frame in 1..=3 {
            assert_eq!(
                stage.step(&pending, false, frame as f64 / 60.0),
                Stepped::Moved
            );
            assert!(!stage.presence(Layer::Pending).shows());
        }
        let live = resting();
        for frame in 4..=90 {
            stage.step(&live, false, frame as f64 / 60.0);
            assert!(!stage.presence(Layer::Pending).shows());
        }
        assert_eq!(stage.presence(Layer::Meter).opacity, 1.0);
        assert!(stage.resting());
    }

    #[test]
    fn reduced_motion_lands_the_outline_and_keeps_a_short_fade() {
        let mut stage = Stage::default();
        stage.step(&resting(), true, 0.0);
        let open = Target {
            actions: Some(0.0),
            ..opened()
        };
        assert_eq!(stage.step(&open, true, 1.0 / 60.0), Stepped::Moved);
        assert_eq!(stage.outline().unwrap().w, 252.0);
        assert_eq!(stage.veil(), (0.0, 0.0));
        let fading = stage.presence(Layer::Action(0));
        assert!(fading.opacity > 0.0 && fading.opacity < 1.0 && fading.scale == 1.0);
        for frame in 2..=40 {
            stage.step(&open, true, frame as f64 / 60.0);
        }
        assert_eq!(stage.presence(Layer::Action(ACTIONS - 1)).opacity, 1.0);
        // Leaving, nothing lingers to be cut by an outline that has landed.
        stage.step(&resting(), true, 41.0 / 60.0);
        assert!((0..ACTIONS).all(|row| !stage.presence(Layer::Action(row)).shows()));
        assert_eq!(stage.outline().unwrap().w, 148.0);
    }
}
