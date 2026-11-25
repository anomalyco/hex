//! The picker's fixture: recent rows that resolve to nothing, and drawn
//! captures (a page with a chart, a dark settings list) marked in Annotate's
//! red, so the scenes show a picture where the picker shows one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::view::{LEAD, PICTURE_W, Size};
use crate::history::paste_source;
use crate::paste::recent::{Detail, Entry, Id, Row};

/// Saturday, 21 November 2026, 12:00 UTC.
pub(super) const NOW: u64 = 1_795_262_400_000;
const MIN: u64 = 60_000;

/// The scenes' captures are drawn at the pixels the preview shows them at
/// (2×), a display's 16:10.
const SHOT: (u32, u32) = ((PICTURE_W * 2.0) as u32, (PICTURE_W * 2.0 / 1.6) as u32);

const NEWEST_WORDS: &str = "Let's move the launch review to Thursday. Dana needs one more day on the pricing numbers, and I'd rather walk through the final deck than a draft.\n\nSame room, same time. I'll update the invite after lunch.";

pub(super) fn rows() -> Vec<Entry> {
    let words = |n, ago: u64, text: &str, app: Option<&str>| Entry {
        row: Row {
            ready_at: NOW - ago * MIN,
            id: Id::Dictation(paste_source::Id::Retained(n)),
            cancelled: false,
            preview: text.chars().take(100).collect(),
        },
        detail: Detail {
            text: text.into(),
            application: app.map(str::to_owned),
            model: Some("Parakeet Unified English".into()),
            ..Detail::default()
        },
    };
    let capture = |ago: u64, text: &str, app: &str, seconds: u64, shot: &str| Entry {
        row: Row {
            ready_at: NOW - ago * MIN,
            id: Id::Capture(crate::capture_host::paste_source::Id::fixture(
                NOW - ago * MIN,
            )),
            cancelled: false,
            preview: text.into(),
        },
        detail: Detail {
            text: text.into(),
            application: Some(app.into()),
            duration_ms: seconds * 1_000,
            picture: Some(PathBuf::from(shot)),
            ..Detail::default()
        },
    };
    let mut cancelled = words(4, 14, "Actually, scratch that last part.", None);
    cancelled.row.cancelled = true;
    cancelled.detail.model = None;
    let mut raw = words(
        2,
        190,
        "Can you check whether the Windows build still fails on the ARM64 runner?",
        Some("Ghostty"),
    );
    raw.detail.raw = true;
    vec![
        capture(
            1,
            "Move the caption below the chart, and give the bars a little more room.",
            "Brave Browser",
            18,
            CHART,
        ),
        words(5, 8, NEWEST_WORDS, Some("Slack")),
        cancelled,
        words(
            3,
            52,
            "Thanks, that works. I'll send the notes after lunch.",
            Some("Messages"),
        ),
        raw,
        capture(
            312,
            "This row's switch sits a few points lower than the others.",
            "Hex",
            9,
            SETTINGS,
        ),
        words(
            1,
            1_480,
            "Remind me to renew the passport before the trip in March.",
            Some("Notes"),
        ),
        words(
            0,
            1_530,
            "Sounds good. Let's ship it on Monday and watch the numbers through the week.",
            Some("Slack"),
        ),
    ]
}

const CHART: &str = "chart";
const SETTINGS: &str = "settings";

/// A fixture capture's picture at `size`, drawn and decoded on first use.
pub(super) fn pixels(path: &Path, size: Size) -> Option<rune::Pixels> {
    static DECODED: OnceLock<Vec<rune::Image>> = OnceLock::new();
    let decoded = DECODED.get_or_init(|| {
        let cache = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        let lead = ((LEAD.0 * 2.0) as u32, (LEAD.1 * 2.0) as u32);
        [chart(), settings()]
            .into_iter()
            .flat_map(|shot| [shot.cover(lead.0, lead.1), shot])
            .map(|shot| {
                let image = cache.rgba(shot.w, shot.h, shot.rgba);
                let _ = image.wait();
                // The image reports ready a moment before its pixels hold it.
                while image.pixels().size() == (0, 0) {
                    std::thread::yield_now();
                }
                image
            })
            .collect()
    });
    let shot = [CHART, SETTINGS]
        .iter()
        .position(|name| Path::new(name) == path)?;
    let at = 2 * shot + usize::from(size == Size::Preview);
    decoded.get(at).map(|image| image.pixels().clone())
}

/// An RGBA picture being drawn.
struct Canvas {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

type Rgb = [u8; 3];

const INK: Rgb = [255, 59, 48];

impl Canvas {
    fn new(w: u32, h: u32, fill: Rgb) -> Self {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend([fill[0], fill[1], fill[2], 255]);
        }
        Self { w, h, rgba }
    }

    fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || alpha <= 0.0 {
            return;
        }
        let at = ((y as u32 * self.w + x as u32) * 4) as usize;
        let alpha = alpha.min(1.0);
        for (channel, value) in color.into_iter().enumerate() {
            let under = f32::from(self.rgba[at + channel]);
            self.rgba[at + channel] = (under + (f32::from(value) - under) * alpha).round() as u8;
        }
    }

    /// A rounded rectangle with antialiased edges.
    fn rect(&mut self, [x, y, w, h]: [f32; 4], radius: f32, color: Rgb) {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        for py in y.floor() as i32..=(y + h).ceil() as i32 {
            for px in x.floor() as i32..=(x + w).ceil() as i32 {
                let qx = (px as f32 + 0.5 - cx).abs() - (w / 2.0 - radius);
                let qy = (py as f32 + 0.5 - cy).abs() - (h / 2.0 - radius);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt()
                    + qx.max(qy).min(0.0)
                    - radius;
                self.blend(px, py, color, 0.5 - outside);
            }
        }
    }

    /// A round-capped stroke through `points`.
    fn stroke(&mut self, points: &[[f32; 2]], width: f32, color: Rgb) {
        let reach = width / 2.0 + 1.0;
        for pair in points.windows(2) {
            let ([ax, ay], [bx, by]) = (pair[0], pair[1]);
            let (vx, vy) = (bx - ax, by - ay);
            let length = (vx * vx + vy * vy).max(f32::EPSILON);
            for py in (ay.min(by) - reach).floor() as i32..=(ay.max(by) + reach).ceil() as i32 {
                for px in (ax.min(bx) - reach).floor() as i32..=(ax.max(bx) + reach).ceil() as i32 {
                    let (qx, qy) = (px as f32 + 0.5 - ax, py as f32 + 0.5 - ay);
                    let t = ((qx * vx + qy * vy) / length).clamp(0.0, 1.0);
                    let (dx, dy) = (qx - t * vx, qy - t * vy);
                    let distance = (dx * dx + dy * dy).sqrt();
                    self.blend(px, py, color, width / 2.0 + 0.5 - distance);
                }
            }
        }
    }

    /// A loop drawn by hand: an ellipse that wobbles and overshoots its
    /// start.
    fn lasso(&mut self, [cx, cy]: [f32; 2], [rx, ry]: [f32; 2], width: f32) {
        let points: Vec<[f32; 2]> = (0..=76)
            .map(|step| {
                let turn = step as f32 / 70.0;
                let angle = std::f32::consts::TAU * turn - 2.3;
                let wobble = 1.0 + 0.035 * (angle * 3.0).sin() + 0.04 * (turn - 0.5);
                [
                    cx + rx * wobble * angle.cos(),
                    cy + ry * wobble * angle.sin(),
                ]
            })
            .collect();
        self.stroke(&points, width, INK);
    }

    /// A curved arrow from `from` to `to`, bowed by `bow`.
    fn arrow(&mut self, from: [f32; 2], to: [f32; 2], bow: f32, width: f32) {
        let mid = [(from[0] + to[0]) / 2.0 + bow, (from[1] + to[1]) / 2.0];
        let at = |t: f32| {
            let u = 1.0 - t;
            [
                u * u * from[0] + 2.0 * u * t * mid[0] + t * t * to[0],
                u * u * from[1] + 2.0 * u * t * mid[1] + t * t * to[1],
            ]
        };
        let points: Vec<[f32; 2]> = (0..=40).map(|step| at(step as f32 / 40.0)).collect();
        self.stroke(&points, width, INK);
        let before = at(0.9);
        let heading = (to[1] - before[1]).atan2(to[0] - before[0]);
        let barb = |turn: f32| {
            let angle = heading + std::f32::consts::PI + turn;
            [to[0] + 26.0 * angle.cos(), to[1] + 26.0 * angle.sin()]
        };
        self.stroke(&[barb(0.5), to, barb(-0.5)], width, INK);
    }

    /// The middle of the picture at `w` × `h`, each pixel the mean of the
    /// pixels it covers.
    fn cover(&self, w: u32, h: u32) -> Self {
        let scale = (self.w as f32 / w as f32).min(self.h as f32 / h as f32);
        let (x0, y0) = (
            (self.w as f32 - w as f32 * scale) / 2.0,
            (self.h as f32 - h as f32 * scale) / 2.0,
        );
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (left, top) = (x0 + x as f32 * scale, y0 + y as f32 * scale);
                let mut sum = [0.0f32; 3];
                let mut count = 0.0;
                for sy in top as u32..((top + scale).ceil() as u32).min(self.h) {
                    for sx in left as u32..((left + scale).ceil() as u32).min(self.w) {
                        let at = ((sy * self.w + sx) * 4) as usize;
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += f32::from(self.rgba[at + channel]);
                        }
                        count += 1.0;
                    }
                }
                rgba.extend(sum.map(|total| (total / count).round() as u8));
                rgba.push(255);
            }
        }
        Self { w, h, rgba }
    }
}

/// A light page in a browser window: a heading, a caption circled in red,
/// a bar chart, and an arrow from the caption to under the chart.
fn chart() -> Canvas {
    let (w, h) = (SHOT.0 as f32, SHOT.1 as f32);
    let mut shot = Canvas::new(SHOT.0, SHOT.1, [246, 246, 244]);
    // The browser's bar: its lights, a tab, and the address field.
    shot.rect([0.0, 0.0, w, 52.0], 0.0, [232, 232, 229]);
    for (i, light) in [[255, 95, 87], [254, 188, 46], [40, 200, 64]]
        .into_iter()
        .enumerate()
    {
        shot.rect([20.0 + 22.0 * i as f32, 19.0, 14.0, 14.0], 7.0, light);
    }
    shot.rect([112.0, 12.0, w - 232.0, 28.0], 14.0, [246, 246, 244]);
    shot.rect([128.0, 22.0, 190.0, 8.0], 4.0, [196, 196, 192]);
    // The page.
    let left = 88.0;
    shot.rect([left, 80.0, 300.0, 20.0], 6.0, [38, 38, 36]);
    shot.rect([left, 128.0, 420.0, 9.0], 4.5, [168, 168, 164]);
    shot.rect([left, 146.0, 328.0, 9.0], 4.5, [168, 168, 164]);
    // The chart's card, its gridlines and bars.
    let card = [left, 196.0, w - 2.0 * left, 232.0];
    shot.rect(card, 12.0, [255, 255, 255]);
    for line in 0..4 {
        shot.rect(
            [
                card[0] + 24.0,
                card[1] + 36.0 + 48.0 * line as f32,
                card[2] - 48.0,
                1.5,
            ],
            0.0,
            [236, 236, 233],
        );
    }
    let bars = [0.42, 0.58, 0.5, 0.74, 0.66, 0.9, 0.82];
    let slot = (card[2] - 96.0) / bars.len() as f32;
    for (i, level) in bars.into_iter().enumerate() {
        let tall = 168.0 * level;
        let tint = if i == 5 {
            [59, 108, 246]
        } else {
            [164, 190, 250]
        };
        shot.rect(
            [
                card[0] + 48.0 + slot * i as f32 + slot * 0.2,
                card[1] + 204.0 - tall,
                slot * 0.6,
                tall,
            ],
            5.0,
            tint,
        );
    }
    shot.rect([left, 452.0, 250.0, 8.0], 4.0, [204, 204, 200]);
    // The marks: the caption circled, and where it should go.
    shot.lasso([left + 206.0, 142.0], [246.0, 27.0], 5.0);
    shot.arrow([left + 462.0, 150.0], [left + 300.0, h - 34.0], 250.0, 5.0);
    shot
}

/// A dark settings card of rows with switches, one boxed in red.
fn settings() -> Canvas {
    let w = SHOT.0 as f32;
    let mut shot = Canvas::new(SHOT.0, SHOT.1, [16, 16, 16]);
    shot.rect([0.0, 0.0, 190.0, SHOT.1 as f32], 0.0, [10, 10, 10]);
    for item in 0..5 {
        let lit = if item == 0 {
            [44, 44, 44]
        } else {
            [10, 10, 10]
        };
        shot.rect([14.0, 48.0 + 42.0 * item as f32, 162.0, 32.0], 8.0, lit);
        shot.rect(
            [28.0, 60.0 + 42.0 * item as f32, 84.0, 8.0],
            4.0,
            [120, 120, 118],
        );
    }
    let card = [222.0, 48.0, w - 254.0, 340.0];
    shot.rect(card, 14.0, [26, 26, 26]);
    for line in 0..5 {
        let y = card[1] + 68.0 * line as f32;
        if line > 0 {
            shot.rect([card[0] + 24.0, y, card[2] - 48.0, 1.5], 0.0, [40, 40, 40]);
        }
        shot.rect(
            [card[0] + 24.0, y + 24.0, 170.0 + 22.0 * line as f32, 9.0],
            4.5,
            [214, 214, 210],
        );
        shot.rect([card[0] + 24.0, y + 42.0, 250.0, 7.0], 3.5, [104, 104, 102]);
        // The fourth row's switch sits low: what the capture points out.
        let drop = if line == 3 { 7.0 } else { 0.0 };
        let on = line % 2 == 0;
        let track = [card[0] + card[2] - 78.0, y + 22.0 + drop, 50.0, 28.0];
        shot.rect(track, 14.0, if on { [59, 108, 246] } else { [70, 70, 70] });
        let knob = if on { track[0] + 25.0 } else { track[0] + 3.0 };
        shot.rect([knob, track[1] + 3.0, 22.0, 22.0], 11.0, [240, 240, 240]);
    }
    let mark = [
        card[0] + card[2] - 96.0,
        card[1] + 68.0 * 3.0 + 12.0,
        86.0,
        54.0,
    ];
    shot.stroke(
        &[
            [mark[0], mark[1]],
            [mark[0] + mark[2], mark[1] + 2.0],
            [mark[0] + mark[2] - 2.0, mark[1] + mark[3]],
            [mark[0] + 3.0, mark[1] + mark[3] - 2.0],
            [mark[0], mark[1]],
        ],
        5.0,
        INK,
    );
    shot.arrow(
        [card[0] + 330.0, card[1] + 316.0],
        [mark[0] - 14.0, mark[1] + 34.0],
        -60.0,
        5.0,
    );
    shot
}
