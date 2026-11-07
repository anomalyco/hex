//! The editor's panel: its toolbar, the shot with its marks and crop, and
//! the Send to… list.

use std::time::Duration;

use rune::{
    Anim, Color, Cursor, Cx, Drag, El, LineCap, Path, Phase, Shape, Spring, SpringPair, TextStyle,
    Transition, Tx, canvas, el, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize};

use super::marks::{self, Mark, Tool};
use super::{Editor, Layout, Out, PAD, SendList, Sessions, Shot, clamp_panel, matching};
use crate::desktop::i18n::tr;
use crate::rune_ui::capture_capsule::ink::{ink_red, layer};
use crate::rune_ui::capture_capsule::surface::{self, CHIP_H, CHIP_PX, SHELL_RADIUS, medium};
use crate::rune_ui::controls::Paint;

/// The panel's corner, the capsule's and the thumbnail's.
const RADIUS: f32 = SHELL_RADIUS;
/// The shot's and the chips' corners, concentric with the panel's.
const INNER_RADIUS: f32 = RADIUS - PAD;
/// The open Send to… list.
const LIST_W: f32 = 320.0;
const ROW_H: f32 = 32.0;
/// The tool glyphs' stroke, in points, the capsule's marks' line.
const GLYPH_STROKE: f32 = 0.8;
const GLYPH: f32 = 16.0;
/// In on the HUD family's spring; out shorter, none. Reduce Motion fades.
const ENTER: Spring =
    crate::rune_ui::capture_capsule::tuning::spring(crate::rune_hud::spring::MORPH);
const LEAVE: Spring = Spring::new(0.22, 0.0);
const REDUCED: Spring =
    crate::rune_ui::capture_capsule::tuning::spring(crate::rune_hud::spring::REDUCED_FADE);
/// How small it grows from.
const ENTER_SCALE: f32 = 0.96;

/// The type of a text mark, `size` points.
pub(super) fn text_style(size: f32) -> TextStyle {
    TextStyle {
        size,
        weight: 700.0,
        line_height: size * 1.2,
        ..medium()
    }
}

/// `mark` drawn on a shot shown at `fit` points a pixel; `scale` is the
/// shot's pixels per point.
pub(super) fn mark_el(mark: &Mark, fit: f32, scale: f32) -> Option<El> {
    if let Mark::Text { at, text: typed } = mark {
        return Some(
            text(typed)
                .font(text_style(marks::TEXT_PT * scale * fit))
                .color(ink_red())
                .single_line()
                .absolute(at[0] * fit, at[1] * fit),
        );
    }
    let at = |p: [f32; 2]| [p[0] * fit, p[1] * fit];
    layer(
        mark.segments(scale)
            .into_iter()
            .map(|(a, b, width)| (at(a), at(b), width * fit)),
        ink_red(),
        1.0,
    )
}

pub(crate) fn view(s: &Editor, cx: &mut Cx<Editor>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let root = el()
        .key("screenshot-editor")
        .w_full()
        .h_full()
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT);
    let Some(shot) = s.shot.as_ref().filter(|_| s.on) else {
        return root;
    };
    let k = p.k;
    let layout = s.layout();
    let [x, y, w, h] = layout.panel;
    let mut panel = el()
        .key("panel")
        .absolute(x, y)
        .size(w, h)
        .radius(RADIUS)
        .fill(k.c.bg_base)
        .role("dialog")
        .label(tr("Screenshot"));
    for shadow in &k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let travel = if cx.is_reduced_motion() {
        Transition::opacity().springs(SpringPair::new(REDUCED, REDUCED))
    } else {
        Transition::opacity()
            .combined(Transition::scale_anchor(
                ENTER_SCALE,
                rune::Vec2::new(0.5, 0.5),
            ))
            .springs(SpringPair::new(ENTER, LEAVE))
    };
    let mut panel = panel
        .hit()
        .on_drag(|s: &mut Editor, d: &mut Drag, tx: &mut Tx| {
            match d.phase {
                Phase::Down => {
                    let [x, y, ..] = s.layout().panel;
                    s.grab = [x, y];
                    s.keys();
                }
                Phase::Move | Phase::Up => {
                    tx.instant();
                    let [.., w, h] = s.layout().panel;
                    s.pos = Some(clamp_panel(
                        [s.grab[0] + d.dx, s.grab[1] + d.dy],
                        [w, h],
                        s.screen,
                    ));
                }
            }
            tx.wake_after(Duration::ZERO);
        })
        .transition(travel)
        .child(
            el().absolute(0.0, 0.0)
                .size(w, h)
                .radius(RADIUS)
                .border(p.hair, k.c.border_weak),
        )
        .child(toolbar(p, s, w - 2.0 * PAD).absolute(PAD, PAD))
        .child(canvas_el(p, s, shot, layout));
    if let Some(list) = &s.send {
        panel = panel.child(send_list(p, list).absolute(w - PAD - LIST_W, PAD + CHIP_H + 4.0));
    }
    root.child(panel)
}

/// The shot, its marks, and the crop, taking the drags that draw.
fn canvas_el(p: Paint, s: &Editor, shot: &Shot, layout: Layout) -> El {
    let k = p.k;
    let [ix, iy, iw, ih] = layout.image;
    let fit = layout.fit;
    let scale = shot.scale;
    let cursor = if s.tool == Tool::Text {
        Cursor::Text
    } else {
        Cursor::Crosshair
    };
    let mut image = el()
        .key("shot")
        .absolute(ix, iy)
        .size(iw, ih)
        .radius(INNER_RADIUS)
        .fill(k.c.layer_02)
        .pixels_fit(shot.image.pixels(), rune::SurfaceFit::Fit)
        .clip_rounded(INNER_RADIUS)
        .instant_changes()
        .hit()
        .cursor(cursor)
        .on_drag(move |s: &mut Editor, d: &mut Drag, tx: &mut Tx| {
            if d.phase == Phase::Down {
                s.send = None;
                s.keys();
            }
            s.draw(d.phase, [d.x / fit, d.y / fit]);
            tx.wake_after(Duration::ZERO);
        });
    let marks = s
        .marks
        .iter()
        .enumerate()
        .map(|(i, m)| (format!("mark-{i}"), m));
    for (key, mark) in marks.chain(s.draft.iter().map(|m| ("draft".to_owned(), m))) {
        if let Some(mark) = mark_el(mark, fit, scale) {
            image = image.child(mark.key(key));
        }
    }
    if let Some((at, typed)) = &s.typing {
        let size = marks::TEXT_PT * scale * fit;
        let caret = el()
            .size((2.0 * fit * scale).max(1.5), size * 1.2)
            .fill(p.accent());
        image = image.child(
            row()
                .absolute(at[0] * fit, at[1] * fit)
                .child(
                    text(typed)
                        .font(text_style(size))
                        .color(ink_red())
                        .single_line(),
                )
                .child(caret),
        );
    }
    let crop = match s.cropping {
        Some((a, b)) => Some([
            a[0].min(b[0]),
            a[1].min(b[1]),
            (a[0] - b[0]).abs(),
            (a[1] - b[1]).abs(),
        ]),
        None => s.crop,
    };
    if let Some([cx, cy, cw, ch]) = crop {
        let [cx, cy, cw, ch] = [cx * fit, cy * fit, cw * fit, ch * fit];
        let shade = k.alpha(k.c.black, 0.55);
        for [x, y, w, h] in [
            [0.0, 0.0, iw, cy],
            [0.0, cy + ch, iw, ih - cy - ch],
            [0.0, cy, cx, ch],
            [cx + cw, cy, iw - cx - cw, ch],
        ] {
            if w > 0.0 && h > 0.0 {
                image = image.child(el().absolute(x, y).size(w, h).fill(shade));
            }
        }
        image = image.child(el().absolute(cx, cy).size(cw, ch).border(1.0, k.c.white));
    }
    image.child(
        el().absolute(0.0, 0.0)
            .size(iw, ih)
            .radius(INNER_RADIUS)
            .border(p.hair, k.alpha(k.c.text_base, 0.14)),
    )
}

/// The tools on the left; a note, Copy, Save, Send to…, and ✕ on the
/// right. Its empty middle moves the panel.
fn toolbar(p: Paint, s: &Editor, w: f32) -> El {
    let k = p.k;
    let mut bar = row().center_y().gap(2.0).size(w, CHIP_H);
    for tool in Tool::ALL {
        let lit = s.tool == tool;
        let ink = if lit { p.text() } else { p.muted() };
        let mut chip = chip(p)
            .w(CHIP_H)
            .center()
            .child(canvas(glyph(tool, ink)).size(GLYPH, GLYPH))
            .role("radio")
            .checked(lit)
            .label(tr(tool.name()))
            .help(tr(tool.name()))
            .on_press(move |s: &mut Editor| s.pick(tool));
        if lit {
            chip = chip.fill(k.c.pressed);
        }
        bar = bar.child(chip);
    }
    let note = s.note.as_ref().map(|(note, _)| note.as_str()).unwrap_or("");
    bar = bar.child(
        row()
            .grow(1.0)
            .h_full()
            .center_y()
            .justify_end()
            .px(8.0)
            .child(
                text(note)
                    .font(surface::small())
                    .color(p.muted())
                    .single_line()
                    .truncate(),
            ),
    );
    bar.child(push(
        k,
        Button::new(tr("Copy"))
            .variant(ButtonVariant::Contrast)
            .on_press_tx(|s: &mut Editor, tx| s.deliver(Out::Copy, tx)),
    ))
    .child(push(
        k,
        Button::new(tr("Save"))
            .variant(ButtonVariant::Ghost)
            .on_press_tx(|s: &mut Editor, tx| s.deliver(Out::Save, tx)),
    ))
    .child(push(
        k,
        Button::new(tr("Send to…"))
            .variant(ButtonVariant::Ghost)
            .expanded(s.send.is_some())
            .on_press(|s: &mut Editor| s.open_send()),
    ))
    .child(
        chip(p)
            .w(CHIP_H)
            .center()
            .child(rune::icon(k.glyph.close, GLYPH).color(p.muted()))
            .label(tr("Close"))
            .help(tr("Close"))
            .on_press_tx(|s: &mut Editor, tx| s.close(tx)),
    )
}

/// A toolbar chip: the capsule's chip height, its hover and press.
fn chip(p: Paint) -> El {
    row()
        .h(CHIP_H)
        .radius(INNER_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .role("button")
}

/// A labelled button the chips' height: the kit's, so Copy, the one main
/// action, is its contrast button.
fn push(k: &rune_kit::Kit, button: Button) -> El {
    button
        .control_size(ControlSize::Large)
        .build(k)
        .h(CHIP_H)
        .px(CHIP_PX)
        .radius(INNER_RADIUS)
}

/// The open Send to… list: what's typed, then the sessions it matches.
fn send_list(p: Paint, list: &SendList) -> El {
    let k = p.k;
    let mut body = rune::col()
        .key("send-list")
        .w(LIST_W)
        .padding(4.0)
        .radius(INNER_RADIUS + 4.0)
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .hit()
        .role("listbox")
        .label(tr("Send to…"));
    for shadow in &k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    let caret = el().size(1.5, 16.0).radius(0.75).fill(p.accent());
    let field = if list.query.is_empty() {
        row()
            .center_y()
            .gap(1.0)
            .child(caret)
            .child(text(tr("Search sessions")).font(medium()).color(p.faint()))
    } else {
        row()
            .center_y()
            .gap(1.0)
            .child(
                text(&list.query)
                    .font(medium())
                    .color(p.text())
                    .single_line(),
            )
            .child(caret)
    };
    body = body.child(
        row()
            .h(ROW_H)
            .px(CHIP_PX - 4.0)
            .center_y()
            .child(field)
            .role("textbox"),
    );
    let quiet = |message: &str| {
        row()
            .h(ROW_H)
            .px(CHIP_PX - 4.0)
            .center_y()
            .child(text(message).font(medium()).color(p.faint()))
    };
    let dests = match &list.sessions {
        Sessions::Loading => return body.child(quiet(tr("Looking for sessions…"))),
        Sessions::Failed => return body.child(quiet(tr("Couldn't reach OpenCode"))),
        Sessions::Ready(dests) => matching(dests, &list.query),
    };
    if dests.is_empty() {
        return body.child(quiet(tr("No sessions")));
    }
    for (i, dest) in dests.into_iter().enumerate() {
        let dest = dest.clone();
        let mut entry = row()
            .h(ROW_H)
            .px(CHIP_PX - 4.0)
            .gap(8.0)
            .center_y()
            .radius(INNER_RADIUS)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .role("option")
            .checked(i == list.selected)
            .label(dest.name.clone())
            .child(
                text(&dest.name)
                    .font(medium())
                    .color(p.text())
                    .single_line()
                    .truncate()
                    .grow(1.0),
            );
        if let Some(folder) = &dest.folder {
            entry = entry.child(
                text(folder)
                    .font(surface::small())
                    .color(p.faint())
                    .single_line()
                    .truncate()
                    .max_w(120.0),
            );
        }
        if i == list.selected {
            entry = entry.fill(k.c.hover);
        }
        body = body.child(
            entry.on_press_tx(move |s: &mut Editor, tx| s.deliver(Out::Send(dest.clone()), tx)),
        );
    }
    body
}

/// A tool's line glyph on a 24 × 24 grid.
fn glyph(tool: Tool, color: Color) -> Shape {
    let line = |a: [f32; 2], b: [f32; 2]| Path::new().move_to(a).line_to(b);
    let paths: Vec<Path> = match tool {
        // A pencil, tip at the bottom left, as Annotate's pen.
        Tool::Pen => vec![
            Path::new()
                .move_to([16.5, 3.5])
                .line_to([20.5, 7.5])
                .line_to([8.0, 20.0])
                .line_to([3.5, 20.5])
                .line_to([4.0, 16.0])
                .close(),
            line([13.5, 6.5], [17.5, 10.5]),
        ],
        Tool::Arrow => vec![
            line([5.0, 19.0], [19.0, 5.0]),
            Path::new()
                .move_to([10.0, 5.0])
                .line_to([19.0, 5.0])
                .line_to([19.0, 14.0]),
        ],
        Tool::Box => vec![
            Path::new()
                .move_to([4.0, 5.0])
                .line_to([20.0, 5.0])
                .line_to([20.0, 19.0])
                .line_to([4.0, 19.0])
                .close(),
        ],
        Tool::Text => vec![
            line([5.0, 5.0], [19.0, 5.0]),
            line([12.0, 5.0], [12.0, 20.0]),
        ],
        Tool::Crop => vec![
            Path::new()
                .move_to([7.0, 2.5])
                .line_to([7.0, 17.0])
                .line_to([21.5, 17.0]),
            Path::new()
                .move_to([2.5, 7.0])
                .line_to([17.0, 7.0])
                .line_to([17.0, 21.5]),
        ],
    };
    let width = GLYPH_STROKE * 24.0 / GLYPH;
    paths
        .iter()
        .fold(Shape::new().view_box(24.0, 24.0), |shape, path| {
            shape.stroke_path(path, width, LineCap::Round, color)
        })
}
