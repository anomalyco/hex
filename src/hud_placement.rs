//! Where a HUD sits on its screen: under the menu bar (or above the Dock),
//! in the menu bar, or as a black island grown out of the notch. Pure
//! geometry over one screen's metrics, so the dictation HUD and the capture
//! capsule can place alike; `dictation_indicator` reads the metrics from
//! `NSScreen` and applies the result.

use crate::dot_scope::Placement;

/// The plate's gap under the menu bar.
const TOP_OFFSET: f64 = 12.0;
/// The plate's gap above the Dock or the screen's bottom edge.
const BOTTOM_OFFSET: f64 = 24.0;
/// The space the in-bar plate leaves above and below itself.
const BAR_INSET: f64 = 3.0;
/// The in-bar plate's gap from the notch it sits beside.
const NOTCH_GAP: f64 = 10.0;
/// The island's room around the plate below the notch.
pub const ISLAND_PAD: f64 = 7.0;
/// "Pretend notch" on a flat screen: a 14-inch MacBook Pro's notch width.
const PRETEND_NOTCH_WIDTH: f64 = 185.0;

/// A rectangle in AppKit screen coordinates, y up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn top(self) -> f64 {
        self.y + self.height
    }

    fn mid_x(self) -> f64 {
        self.x + self.width / 2.0
    }
}

/// One screen as `NSScreen` describes it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Screen {
    pub frame: Rect,
    pub visible: Rect,
    /// `safeAreaInsets.top`: the notch's height, 0 on a flat screen.
    pub safe_top: f64,
    /// The widths of `auxiliaryTopLeftArea` and `auxiliaryTopRightArea`,
    /// the menu bar either side of the notch.
    pub aux_left: f64,
    pub aux_right: f64,
    /// `NSStatusBar.systemStatusBar.thickness`.
    pub bar: f64,
}

/// The notch: its middle in screen x, and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notch {
    pub x: f64,
    pub width: f64,
    pub height: f64,
}

impl Screen {
    pub fn notch(&self) -> Option<Notch> {
        let width = self.frame.width - self.aux_left - self.aux_right;
        (self.safe_top > 0.0 && self.aux_left > 0.0 && self.aux_right > 0.0 && width > 0.0).then(
            || Notch {
                x: self.frame.x + self.aux_left + width / 2.0,
                width,
                height: self.safe_top,
            },
        )
    }

    /// This screen with a notch the menu bar's height in the middle, unless
    /// it has one: for trying the island on a flat display.
    pub fn pretending_notch(self) -> Self {
        if self.notch().is_some() {
            return self;
        }
        let side = (self.frame.width - PRETEND_NOTCH_WIDTH) / 2.0;
        Self {
            safe_top: self.bar_height(),
            aux_left: side,
            aux_right: side,
            ..self
        }
    }

    /// The menu bar's height, shown or hidden.
    pub fn bar_height(&self) -> f64 {
        let shown = self.frame.top() - self.visible.top();
        let bar = if self.bar > 0.0 { self.bar } else { 24.0 };
        self.safe_top.max(bar).max(shown.min(bar + 16.0))
    }
}

/// How a HUD's content sits in its window, in points from the window's
/// middle, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The plate's middle, or its left edge when `anchor_left`.
    pub center: [f32; 2],
    /// The plate grows rightward from `center` instead of from its middle.
    pub anchor_left: bool,
    /// Draws the plate this much smaller, to fit the menu bar.
    pub fit: f32,
    pub island: Option<Island>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            center: [0.0; 2],
            anchor_left: false,
            fit: 1.0,
            island: None,
        }
    }
}

/// A black island grown out of the notch: the notch's size and the screen's
/// top edge, from the window's middle, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Island {
    pub notch: [f32; 2],
    pub top: f32,
}

/// Where the window goes and how its content sits in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// The window's top-left corner, screen coordinates.
    pub top_left: (f64, f64),
    pub layout: Layout,
    /// Whether the window sits over the menu bar and needs a level above it.
    pub over_menu_bar: bool,
}

/// What is being placed: the window's size and the plate's widest size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Content {
    pub window: (f64, f64),
    pub plate: (f64, f64),
}

/// Places `content` on `screen`. `bottom` puts Below above the Dock; the
/// menu bar and the notch are always at the top. On a flat screen From the
/// notch sits in the menu bar; on a notched screen In the menu bar sits
/// beside the notch, where the bar has room.
pub fn place(placement: Placement, screen: &Screen, content: Content, bottom: bool) -> Placed {
    let (window_width, window_height) = content.window;
    let (_, plate_height) = content.plate;
    let screen_top = screen.frame.top();
    let notch = screen.notch();
    let placement = match (placement, notch) {
        (Placement::Notch, None) => Placement::MenuBar,
        (placement, _) => placement,
    };
    // A window centred on the plate, as the capsule always was.
    let centred = |x: f64, y: f64| Placed {
        top_left: (x - window_width / 2.0, y + window_height / 2.0),
        layout: Layout::default(),
        over_menu_bar: false,
    };
    // A window hanging from the screen's top edge around `x`, the plate's
    // middle `depth` below the top.
    let hanging = |x: f64, depth: f64, layout: Layout| Placed {
        top_left: (x - window_width / 2.0, screen_top),
        layout: Layout {
            center: [
                layout.center[0],
                (depth - window_height / 2.0) as f32,
            ],
            ..layout
        },
        over_menu_bar: true,
    };
    match placement {
        Placement::Below if bottom => centred(
            screen.frame.mid_x(),
            screen.visible.y + BOTTOM_OFFSET + plate_height / 2.0,
        ),
        Placement::Below => {
            // Under the notch where a fullscreen space hides the menu bar
            // and `visibleFrame` reaches the top of a notched screen.
            let plate_top = screen.visible.top().min(screen_top - screen.safe_top) - TOP_OFFSET;
            centred(screen.frame.mid_x(), plate_top - plate_height / 2.0)
        }
        Placement::MenuBar => {
            let bar = screen.bar_height();
            let fit = ((bar - 2.0 * BAR_INSET) / plate_height).min(1.0);
            let layout = Layout {
                fit: fit as f32,
                ..Layout::default()
            };
            match notch {
                Some(notch) => {
                    // Grows rightward from beside the notch; the window
                    // starts a little left of it for the plate's shadow.
                    let left = notch.x + notch.width / 2.0 + NOTCH_GAP;
                    let margin = 20.0_f64.min(window_width / 2.0);
                    hanging(
                        left - margin + window_width / 2.0,
                        bar / 2.0,
                        Layout {
                            center: [(margin - window_width / 2.0) as f32, 0.0],
                            anchor_left: true,
                            ..layout
                        },
                    )
                }
                None => hanging(screen.frame.mid_x(), bar / 2.0, layout),
            }
        }
        Placement::Notch => {
            let notch = notch.expect("a notch");
            hanging(
                notch.x,
                notch.height + ISLAND_PAD + plate_height / 2.0,
                Layout {
                    island: Some(Island {
                        notch: [notch.width as f32, notch.height as f32],
                        top: -(window_height / 2.0) as f32,
                    }),
                    ..Layout::default()
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: Content = Content {
        window: (320.0, 128.0),
        plate: (112.0, 26.0),
    };

    /// A 1512 × 982 point 14-inch MacBook Pro: a 32 pt notch 185 pt wide.
    fn notched() -> Screen {
        Screen {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                width: 1512.0,
                height: 982.0,
            },
            visible: Rect {
                x: 0.0,
                y: 70.0,
                width: 1512.0,
                height: 950.0,
            },
            safe_top: 32.0,
            aux_left: 663.5,
            aux_right: 663.5,
            bar: 24.0,
        }
    }

    /// A 2560 × 1440 flat display left of the main one, its menu bar 24 pt.
    fn flat() -> Screen {
        Screen {
            frame: Rect {
                x: -2560.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
            },
            visible: Rect {
                x: -2560.0,
                y: 0.0,
                width: 2560.0,
                height: 1416.0,
            },
            safe_top: 0.0,
            aux_left: 0.0,
            aux_right: 0.0,
            bar: 24.0,
        }
    }

    /// The plate's middle on screen, y up.
    fn plate(placed: &Placed) -> (f64, f64) {
        let (x, top) = placed.top_left;
        (
            x + CONTENT.window.0 / 2.0 + f64::from(placed.layout.center[0]),
            top - CONTENT.window.1 / 2.0 - f64::from(placed.layout.center[1]),
        )
    }

    #[test]
    fn below_hangs_12_points_under_the_menu_bar_or_above_the_dock() {
        let placed = place(Placement::Below, &flat(), CONTENT, false);
        assert_eq!(plate(&placed), (-1280.0, 1416.0 - 12.0 - 13.0));
        assert!(!placed.over_menu_bar);
        assert_eq!(placed.layout, Layout::default());
        let placed = place(Placement::Below, &notched(), CONTENT, true);
        assert_eq!(plate(&placed), (756.0, 70.0 + 24.0 + 13.0));
    }

    #[test]
    fn below_clears_the_notch_when_fullscreen_hides_the_menu_bar() {
        let fullscreen = Screen {
            visible: notched().frame,
            ..notched()
        };
        let placed = place(Placement::Below, &fullscreen, CONTENT, false);
        assert_eq!(plate(&placed).1, 982.0 - 32.0 - 12.0 - 13.0);
    }

    #[test]
    fn the_menu_bar_centres_a_fitted_plate_in_a_flat_bar() {
        let placed = place(Placement::MenuBar, &flat(), CONTENT, true);
        assert_eq!(plate(&placed), (-1280.0, 1440.0 - 12.0));
        assert!((placed.layout.fit - 18.0 / 26.0).abs() < 1e-6);
        assert!(placed.over_menu_bar && placed.layout.island.is_none());
        assert_eq!(placed.top_left.1, 1440.0, "hangs from the top edge");
    }

    #[test]
    fn the_menu_bar_sits_beside_a_notch() {
        let placed = place(Placement::MenuBar, &notched(), CONTENT, false);
        assert!(placed.layout.anchor_left);
        let (left, y) = plate(&placed);
        assert_eq!(left, 756.0 + 92.5 + 10.0);
        assert_eq!(y, 982.0 - 16.0);
        let (x, _) = placed.top_left;
        let widest = CONTENT.plate.0 * f64::from(placed.layout.fit);
        assert!(x < left && x + CONTENT.window.0 > left + widest + 10.0);
    }

    #[test]
    fn the_notch_grows_an_island_and_a_flat_screen_uses_the_menu_bar() {
        let placed = place(Placement::Notch, &notched(), CONTENT, true);
        let island = placed.layout.island.expect("an island");
        assert_eq!(island.notch, [185.0, 32.0]);
        assert_eq!(island.top, -64.0, "the window hangs from the top edge");
        assert_eq!(plate(&placed), (756.0, 982.0 - 32.0 - ISLAND_PAD - 13.0));
        assert_eq!(
            place(Placement::Notch, &flat(), CONTENT, false),
            place(Placement::MenuBar, &flat(), CONTENT, false)
        );
        let pretend = flat().pretending_notch();
        let notch = pretend.notch().expect("a pretend notch");
        assert_eq!((notch.x, notch.width, notch.height), (-1280.0, 185.0, 24.0));
        assert_eq!(notched().pretending_notch(), notched());
    }
}
