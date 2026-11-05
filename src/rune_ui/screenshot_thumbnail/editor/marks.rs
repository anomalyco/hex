//! What the editor draws on a shot, in the shot's own pixels, and how each
//! mark becomes strokes: the same strokes on screen and in the file.

use scrawl_capture::sketch::smooth_ink_points;

/// What a drag on the shot does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    #[default]
    Pen,
    Arrow,
    Box,
    Text,
    Crop,
}

impl Tool {
    pub(crate) const ALL: [Tool; 5] = [Tool::Pen, Tool::Arrow, Tool::Box, Tool::Text, Tool::Crop];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tool::Pen => "Pen",
            Tool::Arrow => "Arrow",
            Tool::Box => "Box",
            Tool::Text => "Text",
            Tool::Crop => "Crop",
        }
    }
}

/// One mark, in the shot's pixels.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mark {
    Pen(Vec<[f32; 2]>),
    Arrow {
        from: [f32; 2],
        to: [f32; 2],
    },
    Box {
        from: [f32; 2],
        to: [f32; 2],
    },
    /// `at` is the text's top-left.
    Text {
        at: [f32; 2],
        text: String,
    },
}

/// Line widths and the text's size, in points: a shot's pixels per point
/// scales them, so a mark looks the same on a Retina shot as on another.
pub(crate) const PEN_PT: f32 = 4.5;
pub(crate) const LINE_PT: f32 = 4.0;
pub(crate) const TEXT_PT: f32 = 22.0;
/// An arrow's head: its sides' angle from the shaft, and their length in
/// line widths, at most half the arrow.
const HEAD_ANGLE: f32 = 0.5;
const HEAD_LENGTH: f32 = 4.0;
/// An arrow or box shorter than this, in points, was a click.
const MIN_PT: f32 = 4.0;

/// A stroke: from, to, and its width, in the shot's pixels.
pub(crate) type Segment = ([f32; 2], [f32; 2], f32);

impl Mark {
    /// A mark the drag that made it ends with, or `None` when it was only a
    /// click. `scale` is the shot's pixels per point.
    pub(crate) fn finished(self, scale: f32) -> Option<Mark> {
        let far = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) >= MIN_PT * scale;
        match &self {
            Mark::Pen(points) if points.is_empty() => None,
            Mark::Arrow { from, to } if !far(*from, *to) => None,
            Mark::Box { from, to }
                if (from[0] - to[0]).abs() < MIN_PT * scale
                    || (from[1] - to[1]).abs() < MIN_PT * scale =>
            {
                None
            }
            Mark::Text { text, .. } if text.trim().is_empty() => None,
            _ => Some(self),
        }
    }

    /// Its strokes, in the shot's pixels; text has none.
    pub(crate) fn segments(&self, scale: f32) -> Vec<Segment> {
        let line = LINE_PT * scale;
        match self {
            Mark::Pen(points) => {
                let points: Vec<[f32; 2]> =
                    points.iter().map(|[x, y]| [x / scale, y / scale]).collect();
                let smoothed = smooth_ink_points(&points);
                let width = |p: f32| PEN_PT * scale * p.clamp(0.55, 1.15);
                let at = |[x, y]: [f32; 2]| [x * scale, y * scale];
                if let [(only, p)] = smoothed.as_slice() {
                    return vec![(at(*only), at(*only), width(*p))];
                }
                smoothed
                    .windows(2)
                    .map(|pair| {
                        let (a, pa) = pair[0];
                        let (b, pb) = pair[1];
                        (at(a), at(b), (width(pa) + width(pb)) * 0.5)
                    })
                    .collect()
            }
            Mark::Arrow { from, to } => {
                let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
                let length = dx.hypot(dy).max(f32::EPSILON);
                let head = (HEAD_LENGTH * line).min(length * 0.5);
                let (ux, uy) = (dx / length, dy / length);
                let side = |turn: f32| {
                    let (sin, cos) = turn.sin_cos();
                    let (bx, by) = (-ux * cos + uy * sin, -ux * sin - uy * cos);
                    [to[0] + bx * head, to[1] + by * head]
                };
                vec![
                    (*from, *to, line),
                    (*to, side(HEAD_ANGLE), line),
                    (*to, side(-HEAD_ANGLE), line),
                ]
            }
            Mark::Box { from, to } => {
                let [x0, y0, x1, y1] = [
                    from[0].min(to[0]),
                    from[1].min(to[1]),
                    from[0].max(to[0]),
                    from[1].max(to[1]),
                ];
                vec![
                    ([x0, y0], [x1, y0], line),
                    ([x1, y0], [x1, y1], line),
                    ([x1, y1], [x0, y1], line),
                    ([x0, y1], [x0, y0], line),
                ]
            }
            Mark::Text { .. } => Vec::new(),
        }
    }
}

/// A rectangle in the shot's pixels, `[x, y, w, h]`.
pub(crate) type Rect = [f32; 4];

/// The rectangle a crop drag from `a` to `b` makes, inside a shot `size`
/// pixels, on whole pixels; `None` when it's too small to mean a crop.
pub(crate) fn crop_rect(a: [f32; 2], b: [f32; 2], size: [u32; 2], scale: f32) -> Option<Rect> {
    let clamp = |v: f32, max: u32| v.clamp(0.0, max as f32).round();
    let (x0, x1) = (
        clamp(a[0].min(b[0]), size[0]),
        clamp(a[0].max(b[0]), size[0]),
    );
    let (y0, y1) = (
        clamp(a[1].min(b[1]), size[1]),
        clamp(a[1].max(b[1]), size[1]),
    );
    let min = MIN_PT * scale;
    (x1 - x0 >= min && y1 - y0 >= min).then_some([x0, y0, x1 - x0, y1 - y0])
}

/// `rect` as whole pixels inside a shot `size` pixels: `[x, y, w, h]`.
pub(crate) fn pixel_rect(rect: Option<Rect>, size: [u32; 2]) -> [u32; 4] {
    let Some([x, y, w, h]) = rect else {
        return [0, 0, size[0], size[1]];
    };
    let x = (x.max(0.0) as u32).min(size[0].saturating_sub(1));
    let y = (y.max(0.0) as u32).min(size[1].saturating_sub(1));
    let w = (w.max(1.0) as u32).min(size[0] - x);
    let h = (h.max(1.0) as u32).min(size[1] - y);
    [x, y, w, h]
}

/// `rgba` (`width` pixels wide) cut to `rect`.
pub(crate) fn crop(rgba: &[u8], width: u32, rect: [u32; 4]) -> Vec<u8> {
    let [x, y, w, h] = rect;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (w * 4) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_leaves_no_mark() {
        let at = [10.0, 10.0];
        assert!(
            Mark::Arrow {
                from: at,
                to: [12.0, 11.0]
            }
            .finished(2.0)
            .is_none()
        );
        assert!(
            Mark::Box {
                from: at,
                to: [60.0, 12.0]
            }
            .finished(2.0)
            .is_none()
        );
        assert!(
            Mark::Text {
                at,
                text: "  ".into()
            }
            .finished(2.0)
            .is_none()
        );
        assert!(Mark::Pen(vec![at]).finished(2.0).is_some());
        assert!(
            Mark::Box {
                from: at,
                to: [60.0, 60.0]
            }
            .finished(2.0)
            .is_some()
        );
    }

    #[test]
    fn an_arrows_head_points_back_along_its_shaft() {
        let arrow = Mark::Arrow {
            from: [0.0, 0.0],
            to: [200.0, 0.0],
        };
        let segments = arrow.segments(2.0);
        assert_eq!(segments.len(), 3);
        for (tip, end, width) in &segments[1..] {
            assert_eq!(*tip, [200.0, 0.0]);
            assert!(end[0] < 200.0, "{end:?}");
            assert_eq!(*width, LINE_PT * 2.0);
        }
        assert!(
            segments[1].1[1] * segments[2].1[1] < 0.0,
            "one side each way"
        );
    }

    #[test]
    fn a_box_is_the_same_whichever_way_it_was_dragged() {
        let a = Mark::Box {
            from: [10.0, 10.0],
            to: [50.0, 40.0],
        };
        let b = Mark::Box {
            from: [50.0, 40.0],
            to: [10.0, 10.0],
        };
        assert_eq!(a.segments(1.0), b.segments(1.0));
    }

    #[test]
    fn a_crop_stays_inside_the_shot_on_whole_pixels() {
        assert_eq!(
            crop_rect([-20.0, 30.4], [90.6, 500.0], [100, 200], 2.0),
            Some([0.0, 30.0, 91.0, 170.0])
        );
        assert_eq!(crop_rect([10.0, 10.0], [12.0, 80.0], [100, 200], 2.0), None);
        assert_eq!(pixel_rect(None, [100, 200]), [0, 0, 100, 200]);
        assert_eq!(
            pixel_rect(Some([90.0, 10.0, 40.0, 10.0]), [100, 200]),
            [90, 10, 10, 10]
        );
    }

    #[test]
    fn cropping_keeps_the_rows_inside() {
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|v| v as u8).collect();
        let out = crop(&rgba, 4, [1, 1, 2, 2]);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[..4], &rgba[20..24]);
        assert_eq!(&out[8..12], &rgba[36..40]);
    }
}
