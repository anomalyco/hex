//! The editor's panel: its toolbar, the shot with its marks and crop, and
//! the Send to… list.

use std::time::Duration;

use rune::{
    Anim, Cursor, Cx, Drag, El, Phase, PopupMenu, Spring, SpringPair, TextStyle, Transition, Tx,
    el, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant};

use super::clip::{Clip, Grip, duration_label, position_label, size_label};
use super::marks::{self, Mark, Tool};
use super::{
    Editor, Layout, Listen, Out, PAD, Pressed, SendList, Sessions, Shot, Talk, clamp_panel,
    matching, talk, zoom,
};
use crate::app_settings::ExportSize;
use crate::desktop::i18n::tr;
use crate::rune_ui::capture_capsule::ink::{ink_red, layer};
use crate::rune_ui::capture_capsule::surface::{self, CHIP_H, CHIP_PX, SHELL_RADIUS, medium};
use crate::rune_ui::controls::{PAUSE, PLAY, Paint, status_dot};
use crate::rune_ui::icons::{ARROW_TOOL, BOX_TOOL, CROP_TOOL, MIC, PEN, TEXT_TOOL};

/// The panel's corner, the capsule's and the thumbnail's.
const RADIUS: f32 = SHELL_RADIUS;
/// The shot's and the chips' corners, concentric with the panel's.
const INNER_RADIUS: f32 = RADIUS - PAD;
/// The open Send to… list.
const LIST_W: f32 = 320.0;
const ROW_H: f32 = 32.0;
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
    let Some(shot) = s.picture().filter(|_| s.on || s.zooming_out()) else {
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
        .label(if s.clip.is_some() {
            tr("Screen recording")
        } else {
            tr("Screenshot")
        });
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
    let mut panel = zoom::chrome(s, panel)
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
    if let Some(clip) = &s.clip {
        panel = panel
            .child(transport_el(p, clip, layout))
            .child(strip_el(p, clip, layout));
    }
    let mut panel = panel.child(words_el(p, s, layout));
    if let Some(list) = &s.send {
        panel = panel.child(send_list(p, list).absolute(w - PAD - LIST_W, PAD + CHIP_H + 4.0));
    }
    root.child(panel).children(zoom::flying(p, s))
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
    let image = el()
        .key("shot")
        .absolute(ix, iy)
        .size(iw, ih)
        .radius(INNER_RADIUS)
        .fill(k.c.layer_02)
        .pixels_fit(shot.image.pixels(), rune::SurfaceFit::Fit)
        .clip_rounded(INNER_RADIUS)
        .instant_changes();
    let mut image = zoom::panel_shot(s, image);
    if let Some(player) = s
        .clip
        .as_ref()
        .filter(|c| c.live)
        .and_then(|c| c.player.as_ref())
    {
        image = image.child(
            el().key("movie")
                .absolute(0.0, 0.0)
                .size(iw, ih)
                .radius(INNER_RADIUS)
                .video(player, rune::SurfaceFit::Fit),
        );
    }
    // A recording's frame takes no marks.
    if s.clip.is_none() {
        image =
            image
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
    }
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
/// right. Its empty middle moves the panel. A recording has the span kept
/// and the size it exports at, with that size's estimate, in place of the
/// tools; while a shrink runs, the button pressed shows how far it is.
fn toolbar(p: Paint, s: &Editor, w: f32) -> El {
    let k = p.k;
    let mut bar = row().center_y().gap(2.0).size(w, CHIP_H);
    if let Some(clip) = &s.clip {
        let [from, to] = clip.trim;
        bar = bar.child(export_size(k, s)).child(
            text(format!("{} – {}", position_label(from), duration_label(to)))
                .key("span")
                .font(medium())
                .color(p.muted())
                .single_line()
                .px(6.0),
        );
    }
    for tool in Tool::ALL.into_iter().filter(|_| s.clip.is_none()) {
        bar = bar.child(
            IconButton::new(icon(tool))
                .variant(IconButtonVariant::Ghost)
                .control_size(ControlSize::Large)
                .label(tr(tool.name()))
                .help(tr(tool.name()))
                .expanded(s.tool == tool)
                .on_press(move |s: &mut Editor| s.pick(tool))
                .build(k)
                .key(format!("tool-{}", tr(tool.name()))),
        );
    }
    let note = s.note.as_ref().map(|(note, _)| note.as_str()).unwrap_or("");
    let ink = if s.failed.is_some() {
        p.danger()
    } else {
        p.muted()
    };
    bar = bar.child(
        row()
            .grow(1.0)
            .h_full()
            .center_y()
            .justify_end()
            .px(4.0)
            .child(
                text(note)
                    .key("note")
                    .font(k.ty.ui)
                    .color(ink)
                    .single_line()
                    .truncate(),
            ),
    );
    if s.failed.is_some() {
        bar = bar.child(push(
            k,
            Button::new(tr("Retry"))
                .variant(ButtonVariant::Ghost)
                .on_press_tx(|s: &mut Editor, tx| s.retry(tx)),
        ));
    }
    // A recording's size is picked beside it, so its save says what it does.
    let save = if s.clip.is_some() {
        tr("Export")
    } else {
        tr("Save")
    };
    bar.child(push(
        k,
        Button::new(pressed_label(s, Pressed::Copy, tr("Copy")))
            .variant(ButtonVariant::Contrast)
            .on_press_tx(|s: &mut Editor, tx| s.deliver(Out::Copy, tx)),
    ))
    .child(push(
        k,
        Button::new(pressed_label(s, Pressed::Save, save))
            .variant(ButtonVariant::Ghost)
            .on_press_tx(|s: &mut Editor, tx| s.deliver(Out::Save, tx)),
    ))
    .child(push(
        k,
        Button::new(pressed_label(s, Pressed::Send, tr("Send to…")))
            .variant(ButtonVariant::Ghost)
            .expanded(s.send.is_some())
            .on_press(|s: &mut Editor| s.open_send()),
    ))
    .child(
        IconButton::new(k.glyph.close)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Large)
            .label(tr("Close"))
            .help(tr("Close"))
            .on_press_tx(|s: &mut Editor, tx| s.close(tx))
            .build(k)
            .key("close"),
    )
}

/// `label`, or how far the export pressed on it is.
fn pressed_label(s: &Editor, pressed: Pressed, label: &str) -> String {
    match s.exporting {
        Some((on, done)) if on == pressed => format!("{}%", (done * 100.0).floor()),
        _ => label.to_owned(),
    }
}

/// The size a recording exports at and about how large it comes out; a
/// press picks another, each with its own estimate.
fn export_size(k: &rune_kit::Kit, s: &Editor) -> El {
    let size = s.export_size;
    let label = |size: ExportSize| match s.estimate(size) {
        Some(bytes) if bytes > 0 => {
            format!("{} · {}", tr(size.name()), estimate_label(size, bytes))
        }
        _ => tr(size.name()).to_owned(),
    };
    let title = label(size);
    let options: Vec<(String, String)> = ExportSize::ALL
        .into_iter()
        .map(|size| (size.id().to_owned(), label(size)))
        .collect();
    let shown = title.clone();
    let button = Button::new(title)
        .variant(ButtonVariant::Outline)
        .help(tr("Export size"))
        .label_with(move |font, ink| {
            row()
                .center_y()
                .gap(4.0)
                .child(text(shown).font(font).color(ink).single_line())
                .child(rune::icon(&rune_theme_opencode::icons::CHEVRON_DOWN, 10.0).color(ink))
        })
        .on_press_tx(move |_: &mut Editor, tx| {
            let menu = PopupMenu::picker(options.clone(), size.id());
            tx.popup_menu(menu, "export-size", |s: &mut Editor, picked, tx| {
                if let Some(size) = ExportSize::ALL
                    .into_iter()
                    .find(|size| Some(size.id()) == picked)
                {
                    s.pick_export_size(size);
                    tx.animated();
                }
            });
        });
    push(k, button).key("export-size")
}

/// An estimate as the picker shows it: as recorded it's the file's share,
/// close to exact; shrunk, it's a guess.
fn estimate_label(size: ExportSize, bytes: u64) -> String {
    match size {
        ExportSize::Original => size_label(bytes),
        _ => format!("~{}", size_label(bytes)),
    }
}

/// Under the shot: the microphone, then what was said over the shot, or
/// how to say it. While a dictation goes, the microphone carries the
/// recording dot and the words end with where it's at.
fn words_el(p: Paint, s: &Editor, layout: Layout) -> El {
    let k = p.k;
    let [x, y, w, h] = layout.words;
    if s.clip.is_some() {
        let words = s.words();
        let words = if words.is_empty() {
            text(tr("Nothing was said")).color(p.faint())
        } else {
            text(words.into_owned()).color(p.text())
        };
        return row()
            .key("words")
            .absolute(x, y)
            .size(w, h)
            .py(super::WORDS_PY)
            .center_y()
            .child(
                words
                    .font(medium())
                    .max_lines(super::WORDS_LINES)
                    .grow(1.0)
                    .min_w(0.0),
            );
    }
    let recording = s.listen == Listen::Recording;
    let mic_label = if recording {
        tr("Finish talking")
    } else {
        tr("Talk over it")
    };
    let mut mic = IconButton::new(&MIC)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(mic_label)
        .help(mic_label)
        .expanded(recording)
        .on_press(|_: &mut Editor| talk(Talk::Toggle))
        .build(k)
        .key("mic");
    if recording {
        mic = mic.child(status_dot(p.danger()).absolute(19.0, 4.0));
    }
    let status = match s.listen {
        Listen::Idle => None,
        Listen::Recording => Some(tr("Listening…")),
        Listen::Transcribing => Some(surface::processing()),
    };
    let words = if !s.words.is_empty() {
        text(&s.words).color(p.text())
    } else if let Some(status) = status {
        text(status).color(p.muted())
    } else {
        text(tr("Hold your dictation shortcut to talk over it")).color(p.faint())
    };
    let mut line = row()
        .grow(1.0)
        .h_full()
        .py(super::WORDS_PY)
        .center_y()
        .gap(8.0)
        .child(
            words
                .font(medium())
                .max_lines(super::WORDS_LINES)
                .grow(1.0)
                .min_w(0.0),
        );
    if let Some(status) = status.filter(|_| !s.words.is_empty()) {
        line = line.child(
            text(status)
                .font(surface::small())
                .color(p.muted())
                .single_line(),
        );
    }
    let mut bar = row()
        .key("words")
        .absolute(x, y)
        .size(w, h)
        .gap(super::GAP)
        .child(mic.self_start())
        .child(line);
    if !s.words.is_empty() && s.listen == Listen::Idle {
        bar = bar.child(
            IconButton::new(k.glyph.close)
                .variant(IconButtonVariant::Ghost)
                .control_size(ControlSize::Large)
                .label(tr("Clear words"))
                .help(tr("Clear words"))
                .on_press(|s: &mut Editor| s.words.clear())
                .build(k)
                .key("clear-words")
                .self_start(),
        );
    }
    bar
}

/// A handle's width, at each end of the span kept.
const HANDLE_W: f32 = 12.0;
const PLAYHEAD_W: f32 = 3.0;
/// How near a handle, in points, a press takes it rather than the playhead.
const HANDLE_REACH: f32 = 16.0;

/// Play or Pause, and where the playhead is of the whole recording.
fn transport_el(p: Paint, clip: &Clip, layout: Layout) -> El {
    let k = p.k;
    let [x, y, w, h] = layout.transport;
    let name = if clip.playing {
        tr("Pause")
    } else {
        tr("Play")
    };
    let play = IconButton::new(if clip.playing { &PAUSE } else { &PLAY })
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .label(name)
        .help(name)
        .shortcut("Space")
        .on_press_tx(|s: &mut Editor, tx| {
            if let Some(clip) = &mut s.clip {
                clip.toggle_play();
            }
            tx.wake_after(Duration::ZERO);
        })
        .build(k)
        .key("play");
    row()
        .key("transport")
        .absolute(x, y)
        .size(w, h)
        .center_y()
        .gap(4.0)
        .child(play)
        .child(
            text(format!(
                "{} / {}",
                position_label(clip.at_ms),
                duration_label(clip.duration_ms)
            ))
            .key("time")
            .font(surface::small())
            .color(p.muted())
            .single_line(),
        )
}

/// A recording's filmstrip: its frames side by side, the span kept framed
/// between two handles with the rest dimmed, and the playhead. A press
/// near a handle drags it; anywhere else it moves the playhead. Either
/// shows the frame there above.
fn strip_el(p: Paint, clip: &Clip, layout: Layout) -> El {
    let k = p.k;
    let [x, y, w, h] = layout.strip;
    let duration = clip.duration_ms.max(1) as f32;
    let at = |ms: u64| (ms as f32 / duration * w).clamp(0.0, w).round();
    // Cells keep the movie's shape at any strip width, each showing the
    // frame nearest its middle, so a narrow editor still shows whole frames.
    let aspect = clip
        .frames
        .first()
        .map(|(_, shot)| shot.size[0] as f32 / shot.size[1].max(1) as f32)
        .unwrap_or(16.0 / 10.0);
    let cells = ((w / (h * aspect)).round() as usize).max(1);
    let cell = w / cells as f32;
    let mut strip = el()
        .key("strip")
        .absolute(x, y)
        .size(w, h)
        .radius(INNER_RADIUS)
        .fill(k.c.layer_02)
        .clip_rounded(INNER_RADIUS)
        .instant_changes();
    for i in 0..cells {
        let Some((_, frame)) = clip
            .frames
            .get((i * 2 + 1) * clip.frames.len() / (cells * 2))
        else {
            continue;
        };
        strip = strip.child(
            el().absolute((i as f32 * cell).floor(), 0.0)
                .size(cell.ceil() + 1.0, h)
                .pixels_fit(frame.image.pixels(), rune::SurfaceFit::Fill),
        );
    }
    let (a, b) = (at(clip.trim[0]), at(clip.trim[1]));
    let shade = k.alpha(k.c.bg_base, 0.72);
    if a > 0.0 {
        strip = strip.child(el().absolute(0.0, 0.0).size(a, h).fill(shade));
    }
    if b < w {
        strip = strip.child(el().absolute(b, 0.0).size(w - b, h).fill(shade));
    }
    let span_w = (b - a).max(2.0 * HANDLE_W);
    let a = a.min(w - span_w);
    strip = strip.child(
        el().key("span")
            .absolute(a, 0.0)
            .size(span_w, h)
            .radius(INNER_RADIUS)
            .border(2.0, p.text()),
    );
    for (key, left) in [("start", a), ("end", a + span_w - HANDLE_W)] {
        strip = strip.child(
            el().key(key)
                .absolute(left, 0.0)
                .size(HANDLE_W, h)
                .radius(INNER_RADIUS)
                .fill(p.text())
                .child(
                    el().absolute(HANDLE_W * 0.5 - 1.0, h * 0.5 - 7.0)
                        .size(2.0, 14.0)
                        .radius(1.0)
                        .fill(k.c.bg_base),
                ),
        );
    }
    let head = at(clip.at_ms).clamp(a + HANDLE_W, (a + span_w - HANDLE_W).max(a + HANDLE_W));
    // White in a dark halo, so it reads over any frame in either theme.
    strip = strip.child(
        el().key("playhead")
            .absolute(head - PLAYHEAD_W * 0.5 - 1.0, 0.0)
            .size(PLAYHEAD_W + 2.0, h)
            .radius(PLAYHEAD_W * 0.5 + 1.0)
            .fill(k.alpha(k.c.black, 0.5))
            .child(
                el().absolute(1.0, 1.0)
                    .size(PLAYHEAD_W, h - 2.0)
                    .radius(PLAYHEAD_W * 0.5)
                    .fill(k.c.white),
            ),
    );
    let reach = (HANDLE_REACH / w.max(1.0) * duration).round() as u64;
    strip
        .child(
            el().absolute(0.0, 0.0)
                .size(w, h)
                .radius(INNER_RADIUS)
                .border(p.hair, k.alpha(k.c.text_base, 0.14)),
        )
        .hit()
        .cursor(Cursor::ResizeColumn)
        .role("slider")
        .label(tr("Trim"))
        .on_drag(move |s: &mut Editor, d: &mut Drag, tx: &mut Tx| {
            s.send = None;
            if let Some(clip) = &mut s.clip {
                let ms = ((d.x / w).clamp(0.0, 1.0) * clip.duration_ms as f32).round() as u64;
                let grip = match (d.phase, clip.holding) {
                    (Phase::Down, _) | (_, None) => clip.grip(ms, reach),
                    (_, Some(grip)) => grip,
                };
                clip.holding = (d.phase != Phase::Up).then_some(grip);
                match grip {
                    Grip::Handle(handle) => clip.drag(handle, ms),
                    Grip::Playhead => clip.scrub(ms),
                }
            }
            tx.instant();
            tx.wake_after(Duration::ZERO);
        })
}

/// A toolbar button, the kit's own: Copy, the one main action, is its
/// contrast button.
fn push(k: &rune_kit::Kit, button: Button) -> El {
    button.control_size(ControlSize::Large).build(k)
}

/// A tool's icon, in Hex's line icons, the theme's own stroke.
fn icon(tool: Tool) -> &'static rune::Icon {
    match tool {
        Tool::Pen => &PEN,
        Tool::Arrow => &ARROW_TOOL,
        Tool::Box => &BOX_TOOL,
        Tool::Text => &TEXT_TOOL,
        Tool::Crop => &CROP_TOOL,
    }
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
