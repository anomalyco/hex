//! The overlay dock: where Hex's floating surfaces sit when two of them want
//! the same edge of the same display.
//!
//! Every surface that rests on a screen edge (the meeting capsules, Annotate's
//! capsule and recording toolbar, the dictation HUD) claims the body it draws
//! there: its display, its edge, how it aligns along that edge, its size, and
//! its distance from the edge. A surface asks [`clearance`] how much farther
//! from the edge than its natural place it must sit to clear every surface that
//! outranks it on that edge of that display, [`GAP`] apart. Ranks are fixed:
//!
//! ```text
//! Meeting capsules     rank 0  anchored where the person put them; never move
//! Annotate capsule,    rank 1  clear the meeting capsules
//!   recording toolbar
//! Dictation HUD        rank 2  clears both
//! ```
//!
//! A surface only ever moves for one that outranks it, and claims where it
//! actually sits (natural place plus clearance), so placing settles in one pass
//! per rank and never oscillates. Each surface asks again as it places itself
//! (the HUD every frame on macOS, the others as they poll), so one that hides,
//! shrinks, or moves to another display lets the others fall back to their
//! natural places.
//!
//! Geometry is in each display's own points, measured from the edge a claim
//! names. Displays on either side of a claim are told apart by the OS's own
//! name for them ([`Display::Id`]: `NSScreenNumber` on macOS, the monitor's
//! device name on Windows); [`Display::Primary`] is a surface that only knows it
//! is on the primary display.

use std::sync::Mutex;

/// The room a surface leaves between itself and one it clears: the spatial
/// scale's 8 pt step (`docs/design-system.md`).
pub(crate) const GAP: f32 = 8.0;

/// A surface that floats on a screen edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Surface {
    /// The meeting recording capsule (the pill and the finished panel it grows
    /// into).
    MeetingPill,
    /// The meeting panel: an offer, a reminder, a call that ended.
    MeetingPanel,
    /// Annotate's capture capsule under the menu bar or in the notch.
    AnnotateCapsule,
    /// The screen recording toolbar (Labs › Screenshots), which Annotate
    /// offers on macOS only, so nothing else constructs it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    AnnotateToolbar,
    /// The dictation HUD.
    Dictation,
}

impl Surface {
    /// Lower ranks stay put; a surface clears every one ranked below it.
    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::MeetingPill | Self::MeetingPanel => 0,
            Self::AnnotateCapsule | Self::AnnotateToolbar => 1,
            Self::Dictation => 2,
        }
    }
}

/// The screen edge a surface rests on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Edge {
    Top,
    Bottom,
}

/// How a surface sits along its edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Align {
    /// `inset` points from the display's left.
    Start,
    /// Its middle `inset` points right of the display's middle.
    Center,
    /// `inset` points from the display's right.
    End,
}

/// The display a claim is on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Display {
    /// The primary display, by a surface that can't name it.
    Primary,
    /// The OS's own name for the display.
    Id(String),
}

/// The body a surface draws on its edge.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Claim {
    pub(crate) display: Display,
    /// The display's width in points, when the surface measured it. A centred
    /// surface and a cornered one can only be compared with it.
    pub(crate) display_width: Option<f32>,
    pub(crate) edge: Edge,
    pub(crate) align: Align,
    /// See [`Align`].
    pub(crate) inset: f32,
    /// Points from the edge to the body's near side.
    pub(crate) margin: f32,
    /// The body's width and height in points.
    pub(crate) size: (f32, f32),
}

impl Claim {
    /// The body's left and right, in points from the display's left on a
    /// display `width` wide.
    fn span(&self, width: f32) -> (f32, f32) {
        let w = self.size.0;
        let left = match self.align {
            Align::Start => self.inset,
            Align::Center => (width - w) / 2.0 + self.inset,
            Align::End => width - self.inset - w,
        };
        (left, left + w)
    }

    /// Whether this body and `other`'s share any of the edge, `GAP` included.
    /// Without either display's width, two bodies aligned alike compare by
    /// their insets, opposite sides never meet, and a centred body is taken to
    /// meet a cornered one: a needless move is better than an overlap.
    fn meets_along(&self, other: &Self) -> bool {
        let width = match (
            self.display_width.or(other.display_width),
            self.align,
            other.align,
        ) {
            (Some(width), _, _) => width,
            (None, a, b) if a == b => 0.0,
            (None, Align::Start, Align::End) | (None, Align::End, Align::Start) => return false,
            (None, _, _) => return true,
        };
        let (a0, a1) = self.span(width);
        let (b0, b1) = other.span(width);
        a0 < b1 + GAP && b0 < a1 + GAP
    }
}

/// Whether two claims' displays are one, `primary` naming the primary display
/// when some surface could. An unnamed primary display is taken to be any
/// display nobody could tell apart from it.
fn same_display(a: &Display, b: &Display, primary: Option<&str>) -> bool {
    match (a, b) {
        (Display::Primary, Display::Primary) => true,
        (Display::Id(a), Display::Id(b)) => a == b,
        (Display::Primary, Display::Id(id)) | (Display::Id(id), Display::Primary) => {
            primary.is_none_or(|primary| primary == id)
        }
    }
}

/// Every surface's claim, and the primary display's name once one is known.
#[derive(Debug, Default)]
pub(crate) struct Dock {
    claims: Vec<(Surface, Claim)>,
    primary: Option<String>,
}

impl Dock {
    pub(crate) const fn new() -> Self {
        Self {
            claims: Vec::new(),
            primary: None,
        }
    }

    /// Sets `surface`'s claim, `None` when it hides. True when it changed.
    pub(crate) fn claim(&mut self, surface: Surface, claim: Option<Claim>) -> bool {
        let at = self.claims.iter().position(|(held, _)| *held == surface);
        match (at, claim) {
            (Some(at), Some(claim)) if self.claims[at].1 == claim => false,
            (Some(at), Some(claim)) => {
                self.claims[at].1 = claim;
                true
            }
            (Some(at), None) => {
                self.claims.remove(at);
                true
            }
            (None, Some(claim)) => {
                self.claims.push((surface, claim));
                true
            }
            (None, None) => false,
        }
    }

    /// Names the primary display. True when it changed.
    pub(crate) fn set_primary(&mut self, primary: Option<String>) -> bool {
        if self.primary == primary {
            return false;
        }
        self.primary = primary;
        true
    }

    /// How many points farther from its edge than `natural` the body of
    /// `surface` must sit to clear every outranking surface on that edge of
    /// that display, [`GAP`] apart; 0 when nothing is in the way. A blocker
    /// it meets moves it to just past that blocker's far side, so it stacks
    /// past each in turn.
    pub(crate) fn clearance(&self, surface: Surface, natural: &Claim) -> f32 {
        let blockers: Vec<&Claim> = self
            .claims
            .iter()
            .filter(|(held, claim)| {
                held.rank() < surface.rank()
                    && claim.edge == natural.edge
                    && same_display(&claim.display, &natural.display, self.primary.as_deref())
                    && claim.meets_along(natural)
            })
            .map(|(_, claim)| claim)
            .collect();
        let height = natural.size.1;
        let mut margin = natural.margin;
        // Each pass moves past at least one more blocker, or settles.
        for _ in 0..=blockers.len() {
            let mut moved = false;
            for blocker in &blockers {
                let (near, far) = (blocker.margin, blocker.margin + blocker.size.1);
                if margin < far + GAP && near < margin + height + GAP {
                    margin = far + GAP;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        margin - natural.margin
    }

    /// The claim `surface` holds, if it shows.
    #[cfg(test)]
    pub(crate) fn held(&self, surface: Surface) -> Option<&Claim> {
        self.claims
            .iter()
            .find(|(held, _)| *held == surface)
            .map(|(_, claim)| claim)
    }
}

static DOCK: Mutex<Dock> = Mutex::new(Dock::new());

fn dock() -> std::sync::MutexGuard<'static, Dock> {
    DOCK.lock().unwrap_or_else(|error| error.into_inner())
}

/// Sets `surface`'s claim in this process's dock, `None` once it hides.
pub(crate) fn claim(surface: Surface, claim: Option<Claim>) {
    dock().claim(surface, claim);
}

/// Names the primary display, for claims that only know they are on it.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn set_primary(primary: Option<String>) {
    dock().set_primary(primary);
}

/// See [`Dock::clearance`], against this process's claims.
pub(crate) fn clearance(surface: Surface, natural: &Claim) -> f32 {
    dock().clearance(surface, natural)
}

/// The display under the pointer, and its width in points where it can be
/// read: what a surface Rune places `UnderPointer` is on. Main thread on
/// macOS; Linux can't name its displays, so it answers the primary display.
pub(crate) fn pointer_display() -> (Display, Option<f32>) {
    #[cfg(target_os = "macos")]
    {
        if let Some((screen, _, _, Some(id))) =
            objc2::MainThreadMarker::new().and_then(crate::rune_hud::pointer_screen)
        {
            return (Display::Id(id.to_string()), Some(screen.frame.width as f32));
        }
    }
    #[cfg(target_os = "windows")]
    {
        let monitors = scrawl_capture::win::monitors();
        let under = scrawl_capture::win::cursor_position().and_then(|(x, y)| {
            monitors.iter().find(|monitor| {
                let [left, top, width, height] = monitor.rect;
                x >= left && y >= top && x < left + width && y < top + height
            })
        });
        if let Some(monitor) = under {
            let scale = if monitor.scale > 0.0 {
                monitor.scale
            } else {
                1.0
            };
            return (
                Display::Id(monitor.device.clone()),
                Some((monitor.rect[2] / scale) as f32),
            );
        }
    }
    (Display::Primary, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: f32 = 1512.0;

    fn on(display: &str) -> Display {
        Display::Id(display.into())
    }

    /// The meeting pill in a top corner: 12 pt in, under the 28 pt menu bar
    /// allowance, 220 × 40.
    fn meeting(align: Align, edge: Edge) -> Claim {
        Claim {
            display: on("1"),
            display_width: Some(LAPTOP),
            edge,
            align,
            inset: 12.0,
            margin: 40.0,
            size: (220.0, 40.0),
        }
    }

    /// Annotate's capsule, centred under the menu bar.
    fn capsule(width: f32) -> Claim {
        Claim {
            display: on("1"),
            display_width: Some(LAPTOP),
            edge: Edge::Top,
            align: Align::Center,
            inset: 0.0,
            margin: 24.0,
            size: (width, 44.0),
        }
    }

    /// The dictation HUD's plate, centred.
    fn hud(edge: Edge) -> Claim {
        Claim {
            display: on("1"),
            display_width: None,
            edge,
            align: Align::Center,
            inset: 0.0,
            margin: 30.0,
            size: (120.0, 26.0),
        }
    }

    #[test]
    fn nothing_in_the_way_leaves_every_surface_where_it_is() {
        let dock = Dock::new();
        assert_eq!(dock.clearance(Surface::Dictation, &hud(Edge::Top)), 0.0);
        assert_eq!(
            dock.clearance(Surface::AnnotateCapsule, &capsule(300.0)),
            0.0
        );
    }

    /// The meeting pill in its top-right corner and a resting capsule in the
    /// middle never meet on a laptop: neither moves.
    #[test]
    fn a_corner_meeting_and_a_centred_capsule_apart_stay_put() {
        let mut dock = Dock::new();
        dock.claim(Surface::MeetingPill, Some(meeting(Align::End, Edge::Top)));
        assert_eq!(
            dock.clearance(Surface::AnnotateCapsule, &capsule(300.0)),
            0.0
        );
    }

    /// A capsule wide enough to reach the meeting's corner (an open list, a
    /// narrow display) drops below the meeting pill, 8 pt clear of it.
    #[test]
    fn annotate_clears_a_meeting_it_would_cover() {
        let mut dock = Dock::new();
        dock.claim(Surface::MeetingPill, Some(meeting(Align::End, Edge::Top)));
        let wide = capsule(1100.0);
        let clearance = dock.clearance(Surface::AnnotateCapsule, &wide);
        assert_eq!(wide.margin + clearance, 40.0 + 40.0 + GAP);
        // The meeting never moves for Annotate.
        let mut annotate = Dock::new();
        annotate.claim(Surface::AnnotateCapsule, Some(wide));
        assert_eq!(
            annotate.clearance(Surface::MeetingPill, &meeting(Align::End, Edge::Top)),
            0.0
        );
    }

    /// The HUD centred under the menu bar meets a meeting pill there only
    /// when one sits in the middle of the same edge.
    #[test]
    fn dictation_clears_a_meeting_on_its_edge() {
        let mut dock = Dock::new();
        dock.claim(Surface::MeetingPill, Some(meeting(Align::End, Edge::Top)));
        assert_eq!(dock.clearance(Surface::Dictation, &hud(Edge::Top)), 0.0);
        // A meeting panel that spans the middle (the finished summary on a
        // narrow display).
        let mut wide = meeting(Align::End, Edge::Top);
        wide.size.0 = 900.0;
        dock.claim(Surface::MeetingPanel, Some(wide));
        let natural = hud(Edge::Top);
        assert_eq!(
            natural.margin + dock.clearance(Surface::Dictation, &natural),
            40.0 + 40.0 + GAP
        );
    }

    /// All three on the top edge: the meeting stays, Annotate drops below it,
    /// and the HUD drops below where Annotate actually sits, so nothing
    /// overlaps.
    #[test]
    fn all_three_stack_meeting_then_annotate_then_dictation() {
        let mut dock = Dock::new();
        let mut panel = meeting(Align::Center, Edge::Top);
        panel.inset = 0.0;
        dock.claim(Surface::MeetingPanel, Some(panel));
        let natural = capsule(400.0);
        let shift = dock.clearance(Surface::AnnotateCapsule, &natural);
        let placed = Claim {
            margin: natural.margin + shift,
            ..natural
        };
        assert_eq!(placed.margin, 40.0 + 40.0 + GAP);
        dock.claim(Surface::AnnotateCapsule, Some(placed.clone()));
        let hud = hud(Edge::Top);
        let margin = hud.margin + dock.clearance(Surface::Dictation, &hud);
        assert_eq!(margin, placed.margin + placed.size.1 + GAP);
        // Nothing overlaps along the edge.
        assert!(margin >= placed.margin + placed.size.1 + GAP);
        assert!(placed.margin >= 40.0 + 40.0 + GAP);
    }

    /// The bottom edge stacks upward the same way, and never reads claims on
    /// the top edge.
    #[test]
    fn the_bottom_edge_stacks_on_its_own() {
        let mut dock = Dock::new();
        let toolbar = Claim {
            display: on("1"),
            display_width: None,
            edge: Edge::Bottom,
            align: Align::Center,
            inset: 0.0,
            margin: 40.0,
            size: (520.0, 80.0),
        };
        dock.claim(Surface::AnnotateToolbar, Some(toolbar));
        dock.claim(Surface::AnnotateCapsule, Some(capsule(400.0)));
        let natural = hud(Edge::Bottom);
        assert_eq!(
            natural.margin + dock.clearance(Surface::Dictation, &natural),
            40.0 + 80.0 + GAP
        );
        // A HUD that already sits clear below it (nearer the edge) stays.
        let mut under = natural.clone();
        under.margin = 4.0;
        under.size.1 = 26.0;
        assert_eq!(dock.clearance(Surface::Dictation, &under), 0.0);
        // A meeting pill in a bottom corner is clear of the centred toolbar.
        dock.claim(
            Surface::MeetingPill,
            Some(meeting(Align::Start, Edge::Bottom)),
        );
        assert_eq!(
            dock.clearance(
                Surface::AnnotateToolbar,
                &dock.held(Surface::AnnotateToolbar).unwrap().clone()
            ),
            0.0
        );
    }

    /// A surface on another display never moves for one here; the primary
    /// display named by one surface matches another that only knows it is on
    /// the primary.
    #[test]
    fn different_displays_never_offset() {
        let mut dock = Dock::new();
        dock.claim(Surface::AnnotateCapsule, Some(capsule(400.0)));
        let mut elsewhere = hud(Edge::Top);
        elsewhere.display = on("2");
        assert_eq!(dock.clearance(Surface::Dictation, &elsewhere), 0.0);
        let mut primary = hud(Edge::Top);
        primary.display = Display::Primary;
        // Nobody named the primary display yet: it may be this one.
        assert!(dock.clearance(Surface::Dictation, &primary) > 0.0);
        dock.set_primary(Some("2".into()));
        assert_eq!(dock.clearance(Surface::Dictation, &primary), 0.0);
        dock.set_primary(Some("1".into()));
        assert!(dock.clearance(Surface::Dictation, &primary) > 0.0);
    }

    /// Hiding a surface takes its claim away, so the ones it pushed fall back
    /// to their natural places; showing it again pushes them once more.
    #[test]
    fn hiding_and_showing_again_reflows() {
        let mut dock = Dock::new();
        let natural = hud(Edge::Top);
        assert!(dock.claim(Surface::AnnotateCapsule, Some(capsule(400.0))));
        assert!(
            !dock.claim(Surface::AnnotateCapsule, Some(capsule(400.0))),
            "unchanged"
        );
        assert!(dock.clearance(Surface::Dictation, &natural) > 0.0);
        assert!(dock.claim(Surface::AnnotateCapsule, None));
        assert_eq!(dock.clearance(Surface::Dictation, &natural), 0.0);
        assert!(!dock.claim(Surface::AnnotateCapsule, None), "already gone");
        // It grows (the routing list opens): the HUD moves farther.
        dock.claim(Surface::AnnotateCapsule, Some(capsule(400.0)));
        let before = dock.clearance(Surface::Dictation, &natural);
        let mut grown = capsule(400.0);
        grown.size.1 += 100.0;
        assert!(dock.claim(Surface::AnnotateCapsule, Some(grown)));
        assert_eq!(dock.clearance(Surface::Dictation, &natural), before + 100.0);
    }

    /// The notch: a capsule hung from it (margin 0, its walls past the top)
    /// and a flat display's capsule under the menu bar both push the HUD past
    /// their bottom; the HUD's natural place there decides nothing else.
    #[test]
    fn notched_and_flat_displays_clear_alike() {
        for (margin, height) in [(0.0, 76.0), (24.0, 44.0)] {
            let mut dock = Dock::new();
            dock.claim(
                Surface::AnnotateCapsule,
                Some(Claim {
                    margin,
                    size: (209.0, height),
                    ..capsule(209.0)
                }),
            );
            let natural = hud(Edge::Top);
            assert_eq!(
                natural.margin + dock.clearance(Surface::Dictation, &natural),
                margin + height + GAP
            );
        }
    }

    /// Without a measured width, alike alignments compare by inset, opposite
    /// corners never meet, and a centred body meets a cornered one.
    #[test]
    fn unmeasured_displays_compare_conservatively() {
        let unmeasured = |align, inset| Claim {
            display_width: None,
            align,
            inset,
            ..meeting(align, Edge::Top)
        };
        assert!(!unmeasured(Align::Start, 12.0).meets_along(&unmeasured(Align::End, 12.0)));
        assert!(unmeasured(Align::Start, 12.0).meets_along(&unmeasured(Align::Start, 100.0)));
        assert!(!unmeasured(Align::Start, 12.0).meets_along(&unmeasured(Align::Start, 400.0)));
        assert!(unmeasured(Align::Center, 0.0).meets_along(&unmeasured(Align::End, 12.0)));
    }

    /// Equal ranks never move each other: Annotate's capsule and toolbar sit
    /// on their own edges.
    #[test]
    fn equal_ranks_leave_each_other_alone() {
        let mut dock = Dock::new();
        dock.claim(Surface::AnnotateToolbar, Some(capsule(600.0)));
        assert_eq!(
            dock.clearance(Surface::AnnotateCapsule, &capsule(400.0)),
            0.0
        );
    }
}
