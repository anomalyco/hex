//! The capsule's view: the shell, its item, and their glyphs.
use super::*;

/// The sound for a cue, from the chosen sound set.
pub(super) fn tone(cue: Cue) -> crate::feedback::Tone {
    use crate::feedback::Tone;
    match cue {
        Cue::Start => Tone::CaptureStart,
        Cue::Stop => Tone::CaptureStop,
        Cue::Sent => Tone::CaptureSent,
        Cue::TakenBack => Tone::CaptureUndo,
        Cue::Discarded => Tone::Cancel,
        Cue::Failed => Tone::Error,
    }
}

pub(super) fn view(s: &CapsuleState, cx: &mut Cx<CapsuleState>) -> El {
    #[cfg(target_os = "macos")]
    if s.playground.is_some() {
        return playground::view(s, cx);
    }
    s.reduced.set(cx.is_reduced_motion());
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let capsule = capsule(p, &s.surface, &s.live, cx);
    if s.backdrop {
        let mut backdrop = el().size(PANEL_W, PANEL_H).fill(p.k.c.layer_03);
        // Stills and clips have no pointer: the cursor's card draws one
        // where the app would find it.
        if s.surface.anchor == surface::Anchor::Cursor {
            backdrop = backdrop.child(pointer(p));
        }
        if tuning::get().flow == tuning::CapsuleFlow::MenuBar {
            backdrop = backdrop.child(menu_item(p));
        }
        return backdrop.child(capsule);
    }
    capsule
}

/// Hex's menu-bar item where the menu-bar flow folds the capsule, for
/// stills and clips, which have no menu bar.
pub(super) fn menu_item(p: Paint) -> El {
    let [x, y, w, h] = surface::MENU_FOLD;
    el().key("menu-item")
        .absolute(x, y)
        .size(w, h)
        .radius(h * 0.5)
        .border(1.2, p.muted())
        .child(
            el().absolute(w * 0.5 - 2.5, h * 0.5 - 2.5)
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(p.muted()),
        )
}

/// The arrow pointer just above and left of the cursor's card.
pub(super) fn pointer(p: Paint) -> El {
    let arrow = Path::new()
        .move_to([1.0, 1.0])
        .line_to([1.0, 16.0])
        .line_to([5.0, 12.5])
        .line_to([7.6, 18.2])
        .line_to([10.2, 17.0])
        .line_to([7.7, 11.6])
        .line_to([12.6, 11.2])
        .close();
    let shape = Shape::new()
        .view_box(14.0, 20.0)
        .fill_path(&arrow, p.k.c.text_base)
        .stroke_path(&arrow, 1.2, LineCap::Round, p.k.c.bg_base);
    canvas(shape)
        .key("pointer")
        .absolute(
            surface::CURSOR_X - surface::POINTER_OFFSET[0],
            surface::CURSOR_Y - surface::POINTER_OFFSET[1],
        )
        .size(14.0, 20.0)
}

/// How content comes and goes, all of it together over the walls' move:
/// a blur, a scale toward the capsule's middle, so the pieces shrink and
/// grow as one, and a fade. Reduce Motion keeps the fade alone.
pub(super) fn presence(surface: &Surface, rect: [f32; 4], reduced: bool) -> Transition {
    let t = tuning::get();
    let springs = SpringPair::new(t.content_in, t.content_out);
    if reduced {
        return Transition::opacity().springs(springs);
    }
    let [sx, sy, sw, sh] = surface.shell;
    let [x, y, w, h] = rect;
    let middle = rune::Vec2::new(
        (sx + sw * 0.5 - x) / w.max(1.0),
        (sy + sh * 0.5 - y) / h.max(1.0),
    );
    Transition::scale_anchor(t.content_scale, middle)
        .combined(Transition::blur(t.content_blur))
        .springs(springs)
}

/// The capsule in its `PANEL_W` × `PANEL_H` panel. Its live chips and
/// buttons and the list are the only `.hit()` boxes: a click anywhere else
/// on it reaches the app beneath, and hover still comes from the pointer.
pub(super) fn capsule(
    p: Paint,
    surface: &Surface,
    live: &Animated,
    cx: &mut Cx<CapsuleState>,
) -> El {
    // The notch is black in either appearance, so what's on it is dark's.
    let p = if surface.anchor == surface::Anchor::Notch {
        Paint::new(true, 1.0 / p.hair)
    } else {
        p
    };
    let reduced = cx.is_reduced_motion() || surface.reduced;
    if surface.reduced {
        cx.reduced_motion(true);
    }
    let mut root = el()
        .key("panel")
        .size(PANEL_W, PANEL_H)
        .child(shell(p, surface, &live.meter));
    // Everything inside is masked by the capsule as it moves: a box on the
    // shell's rect and spring clips to its rounded shape, and the box
    // inside it moves back by the same amount on the same spring, so the
    // content stays where it's laid out while the walls pass over it.
    let [x, y, w, h] = surface.shell;
    let mut inner = el()
        .key("inner")
        .absolute(-x, -y)
        .size(PANEL_W, PANEL_H)
        .motion(surface.shell_motion);
    if let Some(highlight) = &surface.highlight {
        inner = inner.child(
            placed(el(), highlight)
                .trace_name("highlight")
                .radius(surface::CHIP_RADIUS)
                .fill(p.k.c.layer_02)
                .box_shadow(rune::Shadow {
                    spread: p.hair,
                    color: p.k.c.border_base,
                    ..Default::default()
                })
                .transition(presence(surface, highlight.rect, reduced)),
        );
    }
    // Content entering over content that leaves crossfades additively,
    // without a midpoint dip. The highlight is a shared background, so it
    // stays outside; nothing inside overlaps at rest.
    let mut content = el()
        .key("content")
        .absolute(0.0, 0.0)
        .size(PANEL_W, PANEL_H)
        .dissolve();
    for shown in &surface.items {
        let transition = presence(surface, shown.rect, reduced);
        content = content.child(item(p, surface, live, shown).transition(transition));
    }
    let clip = el()
        .key("clip")
        .absolute(x, y)
        .size(w, h)
        .motion(surface.shell_motion)
        .clip_rounded(corner(h))
        .on_hover_change(|s: &mut CapsuleState, on, tx| s.hover(Part::Pill, on, tx))
        .on_pointer_move_local(|s: &mut CapsuleState, _, tx| s.pointer_moved(tx));
    root = root.child(clip.child(inner.child(content)));
    if let Pres::Done(Done {
        open: Some((_, list)),
        ..
    }) = &surface.pres
    {
        root = root.child(list_view(p, list, x, w, reduced));
    }
    root
}

pub(super) fn placed(e: El, shown: &Shown) -> El {
    let [x, y, w, h] = shown.rect;
    e.key(shown.key.clone())
        .absolute(x, y)
        .size(w, h)
        .motion(shown.motion)
}

/// The capsule's edge in dark: a hairline lit from above, which the list
/// panel wears too.
pub(super) fn rim(k: &rune_kit::Kit, w: f32, h: f32, radius: f32) -> El {
    el().key("rim")
        .absolute(0.0, 0.0)
        .size(w, h)
        .radius(radius)
        .border_gradient(0.5, k.light(0.12), k.light(0.03))
}

pub(super) fn shell(p: Paint, surface: &Surface, meter: &Meter) -> El {
    let k = p.k;
    let [x, y, w, h] = surface.shell;
    let mut body = el()
        .key("shell")
        .trace_name("shell")
        .absolute(x, y)
        .size(w, h)
        .radius(corner(h));
    // At the notch it's the notch: black, with no edge or shadow, and
    // tucked into the notch rather than gone when hidden. Under Reduce
    // Motion it never travels, so it fades at rest instead.
    if surface.anchor == surface::Anchor::Notch {
        body = body.fill(k.c.black).motion(surface.shell_motion);
        if surface.reduced {
            body = body
                .opacity(if surface.shell_on { 1.0 } else { 0.0 })
                .opacity_motion(surface.shell_fade);
        }
        return body.role("group").label(tr("Capture"));
    }
    body = body.fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    // The glow inside the edge while the shape morphs, stronger at the top.
    body = body.child(
        el().key("glow")
            .absolute(0.0, 0.0)
            .size(w, h)
            .radius(corner(h))
            .box_shadow(rune::Shadow {
                y: 1.5,
                blur: 8.0,
                color: k.alpha(k.c.text_base, if k.dark { 0.10 } else { 0.06 }),
                inset: true,
                ..Default::default()
            })
            .live_opacity(&meter.glow)
            .motion(surface.shell_motion),
    );
    if k.dark {
        // Dark, a hairline lit from above, as Scrawl's capsule has.
        body = body.child(rim(k, w, h, corner(h)).motion(surface.shell_motion));
    }
    body.opacity(if surface.shell_on { 1.0 } else { 0.0 })
        .motion(surface.shell_motion)
        .opacity_motion(surface.shell_fade)
        .role("group")
        .label(tr("Capture"))
}

pub(super) fn label(content: &str, style: rune::TextStyle, color: Color) -> El {
    text(content).font(style).color(color)
}

pub(super) fn press(id: Id) -> impl Fn(&mut CapsuleState, &mut Tx) + 'static {
    move |s: &mut CapsuleState, tx: &mut Tx| s.click(id, tx)
}

pub(super) fn item(p: Paint, surface: &Surface, live: &Animated, shown: &Shown) -> El {
    let c = &p.k.c;
    let id = shown.id;
    let base = placed(row().center(), shown).trace_name(id.trace_key());
    match id {
        Id::Mode(mode) => {
            let fg = if shown.active { p.text() } else { p.muted() };
            let mut chip = base
                .radius(surface::CHIP_RADIUS)
                .gap(SEG_ICON_GAP)
                .child(mark(id, SEG_ICON, fg))
                .child(label(mode.label(), medium(), fg))
                .press(c.pressed);
            if !shown.active {
                chip = chip.hover(c.hover);
            }
            chip.role("button")
                .label(mode.label())
                .hit()
                .on_press_tx(press(id))
        }
        // The recording's toolbar: the pen, lit with the accent while
        // drawing. Later tools join it, before the divider.
        Id::Pen => {
            let lit = shown.active;
            let ink = if lit { c.bg_base } else { p.muted() };
            let button = icon_button(p, mark(id, 16.0, ink));
            let button = if lit { button.fill(p.accent()) } else { button };
            base.child(
                button
                    .role("button")
                    .label(tr("Draw"))
                    .checked(lit)
                    .help(tr("Draw"))
                    .hit()
                    .on_press_tx(press(id)),
            )
        }
        Id::Divider => base.child(el().size(surface::DIVIDER_W, 16.0).fill(c.border_base)),
        // Stop: a filled square, Finish's icon.
        Id::Finish => {
            let side = 10.0;
            let square = Shape::new()
                .view_box(side, side)
                .fill_path(&rounded(0.0, 0.0, side, side, 2.0), p.text());
            base.child(
                icon_button(p, canvas(square).size(side, side))
                    .role("button")
                    .label(tr("Stop"))
                    .help(tr("Stop"))
                    .hit()
                    .on_press_tx(press(id)),
            )
        }
        // Cancel: ✕, which asks before discarding.
        Id::Cancel => base.child(
            icon_button(p, close_glyph(p))
                .role("button")
                .label(tr("Discard"))
                .help(tr("Discard"))
                .hit()
                .on_press_tx(press(id)),
        ),
        Id::Record => {
            let name = tr("Record");
            base.radius(surface::CHIP_RADIUS)
                .fill(c.bg_contrast)
                .hover(c.contrast_hover)
                .press(c.contrast_pressed)
                .child(label(name, medium(), c.text_contrast))
                .role("button")
                .label(name)
                .hit()
                .on_press_tx(press(id))
        }
        Id::Keep => {
            let name = surface::keep_label();
            base.radius(surface::CHIP_RADIUS)
                .hover(c.hover)
                .press(c.pressed)
                .child(label(name, medium(), p.muted()))
                .role("button")
                .label(name)
                .hit()
                .on_press_tx(press(id))
        }
        // Destructive, so the danger ink on the neutral chip, never the
        // accent.
        Id::Discard => base
            .radius(surface::CHIP_RADIUS)
            .fill(c.layer_02)
            .hover(c.hover)
            .press(c.pressed)
            .child(label(surface::discard_label(), medium(), p.danger()))
            .role("button")
            .label(surface::discard_label())
            .hit()
            .on_press_tx(press(id)),
        Id::Close => base.child(
            row()
                .center()
                .size(28.0, 28.0)
                .radius(ICON_BUTTON_RADIUS)
                .hover(c.hover)
                .press(c.pressed)
                .child(close_glyph(p))
                .role("button")
                .label(tr("Close"))
                .hit()
                .on_press_tx(press(id)),
        ),
        Id::Lead => placed(el(), shown)
            .trace_name("lead")
            .child(dot(p, &live.meter))
            .child(timer(p, surface, live)),
        Id::Processing => placed(el(), shown)
            .trace_name("processing")
            .child(spinner(p, &live.meter))
            .child(processing_label(p, &live.meter)),
        Id::Done => {
            let lead = match &surface.pres {
                Pres::Done(done) => done.lead,
                _ => Lead::Check,
            };
            let ink = match tuning::get().top_line {
                tuning::TopLine::Quiet => p.muted(),
                tuning::TopLine::Icons => p.text(),
            };
            let mut done = placed(el(), shown).trace_name("done").child(glyph(
                p,
                lead,
                live.thumb.as_ref().map(rune::Image::pixels),
            ));
            if !shown.ident.is_empty() {
                done = done.child(
                    row()
                        .center_y()
                        .absolute(done_text_x(lead), 0.0)
                        .h(CHIP_H)
                        .child(label(&shown.ident, done_style(), ink)),
                );
            }
            let sends = matches!(&surface.pres, Pres::Done(done) if done.sends);
            if sends {
                return done
                    .radius(surface::CHIP_RADIUS)
                    .hover(c.hover)
                    .press(c.pressed)
                    .role("button")
                    .label(shown.ident.clone())
                    .hit()
                    .on_press_tx(press(id));
            }
            done.role("text").label(if shown.ident.is_empty() {
                tr("Copied").to_owned()
            } else {
                shown.ident.clone()
            })
        }
        Id::Next => keyed(p, base, &["⇥"], tr("Next"), medium()).on_press_tx(press(id)),
        // A chip beside the frame in the menu-bar flow.
        Id::Pick(_) if tuning::get().flow == tuning::CapsuleFlow::MenuBar => {
            keyed(p, base, &[], shown.text(), medium()).on_press_tx(press(id))
        }
        Id::Pick(row) => pick_row(p, base, shown, row),
        Id::CardHints => base
            .justify_start()
            .px(ROW_PX)
            .child(hint(p, tr("Search"), &surface::shortcut_keycaps()))
            .role("text")
            .label(tr_fill("Search {}", shown.text())),
        Id::Why => placed(row().center_y(), shown)
            .trace_name("why")
            .child(quiet(p, shown.text(), done_style())),
        // The quiet way out of a send that just went.
        // Click only: Escape meant for another app was taking sends back.
        Id::Undo => {
            let [_, _, w, h] = shown.rect;
            let ground = if surface.anchor == surface::Anchor::Notch {
                c.black
            } else {
                c.bg_base
            };
            undo_ring::chip(p, base, [w, h], ground, live.ring.as_ref()).on_press_tx(press(id))
        }
        // Search, with the capture shortcut that opens it as Settings draws it.
        Id::Ask
            if matches!(
                &surface.pres,
                Pres::Done(Done { ask: Some(ask), ask_keys: true, .. }) if *ask == shown.ident
            ) =>
        {
            base.justify_start()
                .pl(CHIP_PX)
                .pr(surface::keycaps_inset())
                .gap(surface::KEYCAPS_GAP)
                .radius(surface::CHIP_RADIUS)
                .hover(c.hover)
                .press(c.pressed)
                .child(label(&shown.ident, medium(), p.muted()))
                .child(el().grow(1.0))
                .child(super::super::controls::keycaps(
                    p,
                    "ask-keys",
                    &surface::shortcut_keycaps(),
                ))
                .role("button")
                .label(shown.ident.clone())
                .hit()
                .on_press_tx(press(id))
        }
        Id::Ask => keyed(p, base, &[], &shown.ident, medium()).on_press_tx(press(id)),
        Id::Paste | Id::Inspect => {
            let (name, help) = if id == Id::Paste {
                (tr("Paste"), tr("Paste the capture"))
            } else {
                (tr("Inspect"), tr("Open the capture"))
            };
            base.child(
                row()
                    .center()
                    .size(28.0, 28.0)
                    .radius(ICON_BUTTON_RADIUS)
                    .hover(c.hover)
                    .press(c.pressed)
                    .child(mark(id, 16.0, p.muted()))
                    .role("button")
                    .label(name)
                    .help(help)
                    .hit()
                    .on_press_tx(press(id)),
            )
        }
        Id::Field => {
            let empty = rune::TextEdit::default();
            let typed = match &surface.pres {
                Pres::Done(Done {
                    open: Some((typed, _)),
                    ..
                }) => typed,
                _ => &empty,
            };
            field(p, base, typed)
        }
        // Rows live in the list, never in the capsule.
        Id::Row(_) => base,
    }
}

/// A guess in the cursor's or the notch's card, as the list's rows read:
/// its name and its reason, the best one's in the accent.
pub(super) fn pick_row(p: Paint, base: El, shown: &Shown, row: usize) -> El {
    let c = &p.k.c;
    let room = shown.rect[2] - 4.0 * ROW_PX - measure(shown.text(), medium());
    let note = ellipsize(shown.note(), small(), room.max(24.0));
    base.justify_start()
        .px(ROW_PX)
        .gap(ROW_PX)
        .radius(ROW_RADIUS + 2.0)
        .hover(c.hover)
        .press(c.pressed)
        .child(label(shown.text(), medium(), p.text()))
        .child(el().grow(1.0))
        .child(
            label(
                &note,
                small(),
                if row == 0 { p.accent() } else { p.muted() },
            )
            .offset(0.0, (medium().size - small().size) * BASELINE_DROP),
        )
        .role("button")
        .label(shown.text().to_owned())
        .hit()
        .on_press_tx(press(Id::Pick(row)))
}

/// A chip's label, then its shortcut, if any, as one quiet word.
pub(super) fn keyed(p: Paint, base: El, keys: &[&str], name: &str, style: rune::TextStyle) -> El {
    let mut chip = base
        .radius(surface::CHIP_RADIUS)
        .gap(HINT_GAP)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .child(label(name, style, p.muted()));
    if !keys.is_empty() {
        chip = chip.child(quiet(p, &chord(keys), style));
    }
    chip.role("button").label(name.to_owned()).hit()
}

/// Small faint text (a shortcut, a note) beside words in `beside`, on
/// their baseline: centring a smaller line in the same row would lift it.
pub(super) fn quiet(p: Paint, content: &str, beside: rune::TextStyle) -> El {
    label(content, small(), p.faint()).offset(0.0, (beside.size - small().size) * BASELINE_DROP)
}

/// A centred line's baseline sits this many ems below its middle: half
/// the theme face's ascender less its descender (Inter: 0.969, 0.242).
pub(super) const BASELINE_DROP: f32 = 0.364;

/// What a hint does in the muted ink, then its keys as Settings draws a
/// shortcut: keycaps in one small tray ("Send [⏎]").
pub(super) fn hint<S: AsRef<str>>(p: Paint, copy: &str, keys: &[S]) -> El {
    let mut hint = row().center_y().gap(HINT_GAP);
    if !copy.is_empty() {
        hint = hint.child(label(copy, small(), p.muted()));
    }
    if !keys.is_empty() {
        hint = hint.child(super::super::controls::keycaps(p, "keys", &key_parts(keys)));
    }
    hint
}

/// A [`hint`]'s width.
pub(super) fn hint_width<S: AsRef<str>>(copy: &str, keys: &[S]) -> f32 {
    let gap = if copy.is_empty() || keys.is_empty() {
        0.0
    } else {
        HINT_GAP
    };
    let tray = if keys.is_empty() {
        0.0
    } else {
        super::super::controls::keycaps_width(&key_parts(keys), measure)
    };
    measure(copy, small()) + gap + tray
}

pub(super) fn key_parts<S: AsRef<str>>(keys: &[S]) -> Vec<String> {
    keys.iter().map(|key| key.as_ref().to_owned()).collect()
}

/// The open capsule's field: the kit's search field over the typed
/// text. The capsule never takes key focus, so the field always reads as
/// focused and Hex's tap feeds it ([`flow::Input::Type`]).
pub(super) fn field(p: Paint, base: El, typed: &rune::TextEdit) -> El {
    let mut edit = typed.clone();
    edit.focused = true;
    let input = TextInput::new(&edit)
        .placeholder(tr("Send to…"))
        .label(tr("Send to"))
        .leading_icon(p.k.glyph.search)
        .width(None)
        .height(CHIP_H)
        .build(p.k);
    base.trace_name("field").child(input.grow(1.0))
}

/// The finished capsule's check, or its warning: its ink spans 12…20,
/// where the recording dot's does.
pub(super) fn glyph(p: Paint, lead: Lead, thumb: Option<&rune::Pixels>) -> El {
    let glyph = match (lead, thumb) {
        (Lead::Thumbnail, Some(pixels)) => return thumbnail(p, pixels),
        (Lead::Check | Lead::Thumbnail, _) => {
            rune::icon(p.k.glyph.check_small, CHECK).color(p.success())
        }
        (Lead::Warning, _) => mark(Id::Why, CHECK, p.k.c.warning.fg),
    };
    glyph.absolute(8.0, (CHIP_H - CHECK) * 0.5)
}

/// The kit's ✕, at the size Hex's icon buttons draw it.
pub(super) fn close_glyph(p: Paint) -> El {
    rune::icon(p.k.glyph.close, 16.0).color(p.muted())
}

/// The capsule's own line marks (the modes, the pen, Paste, Inspect, the
/// warning, the badges) all draw at this stroke in points, the line of the
/// kit's ✕ beside them, whatever their size.
pub(super) const MARK_STROKE: f32 = 0.8;

/// [`MARK_STROKE`] on the 24 × 24 grid of a mark drawn `size` points.
pub(super) fn mark_stroke(size: f32) -> f32 {
    MARK_STROKE * 24.0 / size
}

/// One of the capsule's own line marks, `size` points square.
pub(super) fn mark(id: Id, size: f32, color: Color) -> El {
    canvas(icon(id, mark_stroke(size), color)).size(size, size)
}

pub(super) const CHECK: f32 = 16.0;
/// A 28 pt icon button's corners, concentric with the shell's.
pub(super) const ICON_BUTTON_RADIUS: f32 = surface::CHIP_RADIUS - 2.0;
pub(super) const THUMB_RADIUS: f32 = 4.0;

/// The capture's first frame, cropped to fill a small rounded card, with a
/// hairline over its edge so a white page still reads on the capsule.
pub(super) fn thumbnail(p: Paint, pixels: &rune::Pixels) -> El {
    el().absolute(THUMB_X, (CHIP_H - THUMB_H) * 0.5)
        .size(THUMB_W, THUMB_H)
        .radius(THUMB_RADIUS)
        .fill(p.k.c.layer_02)
        .pixels_fit(pixels, rune::SurfaceFit::Fill)
        .child(
            el().absolute(0.0, 0.0)
                .size(THUMB_W, THUMB_H)
                .radius(THUMB_RADIUS)
                .border(p.hair, p.k.alpha(p.k.c.text_base, 0.14)),
        )
}

/// The timer in tabular digits, bound to its `LiveText` in a box as wide
/// as the clock.
pub(super) fn timer(p: Paint, surface: &Surface, live: &Animated) -> El {
    row().center_y().absolute(LEAD_TEXT_X, 0.0).h(CHIP_H).child(
        label(&surface.clock, clock_style(), p.text())
            .live_text(&live.clock)
            .w(clock_width(&surface.clock)),
    )
}

/// "Processing…", each glyph at its place in the shaped label and lit by
/// the shimmer.
pub(super) fn processing_label(p: Paint, meter: &Meter) -> El {
    let mut glyphs = el()
        .absolute(PROCESSING_TEXT_X, 0.0)
        .h(CHIP_H)
        .w(measure(processing(), medium()))
        .role("text")
        .label(processing());
    for ((c, x, w), live) in processing_glyphs().into_iter().zip(&meter.shimmer) {
        glyphs = glyphs.child(
            row()
                .center_y()
                .absolute(x, 0.0)
                .h(CHIP_H)
                .w(w)
                .child(label(c.encode_utf8(&mut [0; 4]), medium(), p.text()))
                .live_opacity(live),
        );
    }
    glyphs
}

/// The recording dot: a breathing core with a glow, and a ripple ring
/// every 1.6 s.
pub(super) fn dot(p: Paint, meter: &Meter) -> El {
    let red = p.danger();
    let ring = 20.0;
    let core = 8.0;
    el().absolute(0.0, 0.0)
        .size(CHIP_H, CHIP_H)
        .child(
            el().absolute((CHIP_H - ring) * 0.5, (CHIP_H - ring) * 0.5)
                .size(ring, ring)
                .radius(ring * 0.5)
                .fill(p.k.alpha(red, 0.5))
                .live_scale(&meter.ring_scale)
                .live_opacity(&meter.ring_alpha),
        )
        .child(
            el().absolute((CHIP_H - core) * 0.5, (CHIP_H - core) * 0.5)
                .size(core, core)
                .radius(core * 0.5)
                .fill(red)
                .shadow(0.0, 7.0, red.alpha(0.55))
                .live_opacity(&meter.core),
        )
}

/// Eight dots around a 6.2 pt circle with a bright head going round.
pub(super) fn spinner(p: Paint, meter: &Meter) -> El {
    let mut dots = el().absolute(0.0, 0.0).size(CHIP_H, CHIP_H);
    let d = 2.7;
    for (k, live) in meter.spin.iter().enumerate() {
        let angle = k as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
        let (x, y) = (
            CHIP_H * 0.5 + 6.2 * angle.cos(),
            CHIP_H * 0.5 + 6.2 * angle.sin(),
        );
        dots = dots.child(
            el().absolute(x - d * 0.5, y - d * 0.5)
                .size(d, d)
                .radius(d * 0.5)
                .fill(p.text())
                .live_opacity(live),
        );
    }
    dots
}
