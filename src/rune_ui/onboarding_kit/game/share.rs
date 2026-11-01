//! The score card that rises after the certificate and after Done: what
//! the run destroyed, the best score, and the transcript of what was said.

use super::*;

/// How the share card stacks up: its transcript's height (0 for none),
/// whether that scrolls, whether it's signed, and its whole height.
struct ShareLayout {
    said: f32,
    scrolls: bool,
    signed: bool,
    h: f32,
}

fn share_layout(p: Paint, g: &Play) -> ShareLayout {
    let width = SHARE_W - 2.0 * SHARE_PAD;
    let all = g.log.iter().map(|c| said_height(p, c, width)).sum::<f32>()
        + SAID_GAP * g.log.len().saturating_sub(1) as f32;
    let said = all.min(SAID_MAX);
    let signed = g.certificate().is_some_and(|c| c.answered);
    let rows = [
        Some(SHARE_ROWS[0]),
        Some(SHARE_ROWS[1]),
        Some(SHARE_ROWS[2]),
        Some(SHARE_ROWS[3]),
        Some(SHARE_ROWS[4]),
        (said > 0.0).then_some(said),
        signed.then_some(SHARE_ROWS[5]),
        Some(SHARE_ROWS[6]),
    ];
    let shown = rows.iter().flatten();
    let h = 2.0 * SHARE_PAD + shown.clone().sum::<f32>() + SHARE_GAP * (shown.count() - 1) as f32;
    ShareLayout {
        said,
        scrolls: all > SAID_MAX,
        signed,
        h,
    }
}

fn said_lines(c: &Cleared) -> (String, String) {
    (
        format!("{}: {}", c.notice.from, c.notice.body),
        format!("You: {}", c.reply),
    )
}

fn said_height(p: Paint, c: &Cleared, width: f32) -> f32 {
    let (them, you) = said_lines(c);
    let (_, a) = rune::text::measure(&them, &[], p.k.ty.small, Some(width));
    let (_, b) = rune::text::measure(&you, &[], p.k.ty.ui, Some(width));
    a + 2.0 + b
}

/// Every message cleared and what the player said back, oldest first.
fn transcript(p: Paint, g: &Play, layout: &ShareLayout) -> El {
    let k = p.k;
    let width = SHARE_W - 2.0 * SHARE_PAD;
    let lines = col()
        .key("lines")
        .w(width)
        .gap(SAID_GAP)
        .children(g.log.iter().enumerate().map(|(i, c)| {
            let (them, you) = said_lines(c);
            col()
                .key(format!("said-{i}"))
                .w(width)
                .gap(2.0)
                .child(
                    text(them)
                        .key("them")
                        .font(k.ty.small)
                        .color(p.muted())
                        .wrap(),
                )
                .child(text(you).key("you").font(k.ty.ui).color(p.text()).wrap())
        }));
    let said = col().key("transcript").w(width).h(layout.said);
    if layout.scrolls {
        said.clip_fade(0.0, 0.0, 0.0, 18.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(col().key("inset").pb(18.0).child(lines))
    } else {
        said.child(lines)
    }
}

/// The score's second line: the best so far, or a new one.
fn best_copy(score: Score) -> String {
    let Score { destroyed, best } = score;
    if destroyed > best && best > 0 {
        format!("New high score · best was {best}")
    } else if destroyed > best {
        "New high score".to_string()
    } else if destroyed == best {
        "Ties your best".to_string()
    } else {
        format!("Your best is {best}")
    }
}

/// The shareable moment: the certificate's blast becomes a score card. Its
/// surface goes opaque at once, then its content shows; Keep playing folds
/// the surface and the number into the score pill.
pub(super) fn share<S: 'static>(
    p: Paint,
    g: &Play,
    t: f64,
    reduced: bool,
    look: &Look<'_, S>,
) -> Option<El> {
    let age = (t - g.share?) as f32;
    let fold = fold(g, t);
    let score = g.score?;
    if age < 0.0 || fold >= 1.0 {
        return None;
    }
    let destroyed = score.destroyed;
    let layout = share_layout(p, g);
    let share_h = layout.h;
    let k = p.k;
    let w = SHARE_W;
    let [cx, cy] = g.share_from;
    let x = (age / 0.6).clamp(0.0, 1.0);
    let rise = if reduced {
        1.0
    } else {
        1.0 - (1.0 - x).powi(4)
    };
    // easeOutBack: a touch past full size, then settles.
    let settle = if reduced {
        1.0
    } else {
        let c = 1.4;
        1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
    };
    let home = [(STAGE_W - w) * 0.5, ((STAGE_H - share_h) * 0.5).max(16.0)];
    let from = [cx - (home[0] + w * 0.5), cy - (home[1] + share_h * 0.5)];
    let surface_in = smooth(0.0, 0.11, age);
    let content_in = smooth(0.11, 0.23, age);
    let pill = pill_in(fold, reduced);
    // The fold: everything but the number goes in its first 100 ms, the
    // surface and number become the pill, and the pill takes over at the end.
    let (morph, rest) = if reduced {
        (0.0, 1.0 - pill)
    } else {
        (ease_in_out(fold), 1.0 - smooth(0.0, 0.1 / FOLD, fold))
    };
    // Still warm from the blast, cooling.
    let warm = (-age / 0.45).exp();
    let (words, keys, secs) = g.stats();
    // The number counts up, each tick landing with a small kick.
    let count = if reduced {
        destroyed as f32
    } else {
        smooth(0.15, 0.75, age) * destroyed as f32
    };
    let shown = count.ceil() as usize;
    let since_tick = if reduced || shown == 0 {
        1.0
    } else {
        // When the count passed `shown - 1`: smoothstep's inverse.
        let y = (shown as f32 - 1.0) / destroyed as f32;
        let x = 0.5 - ((1.0 - 2.0 * y).asin() / 3.0).sin();
        age - lerp(0.15, 0.75, x)
    };
    let kick = if reduced {
        0.0
    } else {
        (-since_tick.max(0.0) * 9.0).exp()
    };
    let ember = ember_tone(p, 0.7);
    let stat = |key: &str, value: String, label: &str| {
        col()
            .key(key.to_string())
            .gap(1.0)
            .grow(1.0)
            .child(
                text(value)
                    .key("v")
                    .font(TextStyle::new(17.0, 620.0).lh(22.0).features("'tnum' 1"))
                    .color(p.text()),
            )
            .child(
                text(label.to_string())
                    .key("l")
                    .font(k.ty.small)
                    .color(p.muted()),
            )
    };
    let rule = |key: &str| {
        el().key(key.to_string())
            .size(w - 2.0 * SHARE_PAD, SHARE_ROWS[2])
            .fill(k.c.border_weak)
    };
    let big = TextStyle::new(BIG_SIZE, 720.0)
        .lh(BIG_LINE)
        .tracking(-2.0)
        .features("'tnum' 1");
    let number = |key: &str, color: Color| {
        el().key(key.to_string())
            .scale(1.0 + 0.1 * kick)
            .origin(0.5, 0.8)
            .scale_motion(Anim::INSTANT)
            .child(
                text(shown.to_string())
                    .key("glow")
                    .font(big)
                    .color(ember.alpha((0.45 + 0.35 * kick) * warm))
                    .absolute(0.0, 0.0)
                    .blur(12.0),
            )
            .child(text(shown.to_string()).key("n").font(big).color(color))
    };
    let folding = fold > 0.0 && !reduced;
    let content = col()
        .key("content")
        .absolute(0.0, 0.0)
        .w(w)
        .padding(SHARE_PAD)
        .gap(SHARE_GAP)
        .opacity(content_in * rest)
        .opacity_motion(Anim::INSTANT)
        .child(
            row()
                .key("top")
                .h(SHARE_ROWS[0])
                .center_y()
                .child(
                    text("Hex")
                        .key("brand")
                        .font(k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(el().key("space").grow(1.0))
                .child(
                    text("Onboarding complete")
                        .key("when")
                        .font(k.ty.small)
                        .color(p.muted()),
                ),
        )
        .child(
            row()
                .key("score")
                .h(SHARE_ROWS[1])
                .gap(16.0)
                .items_end()
                .child(
                    number("number", ember_tone(p, 0.85 + 0.15 * kick))
                        .opacity(if folding { 0.0 } else { 1.0 })
                        .opacity_motion(Anim::INSTANT),
                )
                .child(
                    col()
                        .key("what")
                        .pb(14.0)
                        .gap(2.0)
                        .child(
                            text("messages destroyed")
                                .key("a")
                                .font(LEAD)
                                .color(p.text()),
                        )
                        .child({
                            let high = score.destroyed > score.best;
                            text(best_copy(score))
                                .key("b")
                                .font(if high { k.ty.small_medium } else { k.ty.small })
                                .color(if high { ember } else { p.muted() })
                        }),
                ),
        )
        .child(rule("rule-1"))
        .child(
            row()
                .key("stats")
                .h(SHARE_ROWS[3])
                .gap(12.0)
                .child(stat("words", words.to_string(), "words said"))
                .child(stat("keys", keys.to_string(), "keys typed"))
                .child(stat("time", format!("{secs:.1} s"), "start to finish")),
        )
        .child(rule("rule-2"))
        .children((layout.said > 0.0).then(|| transcript(p, g, &layout)))
        .children(g.certificate().filter(|_| layout.signed).map(|cert| {
            row()
                .key("signed")
                .h(SHARE_ROWS[5])
                .gap(10.0)
                .items_end()
                .child(
                    text("Signed")
                        .key("s")
                        .font(k.ty.small)
                        .color(p.muted())
                        .pb(6.0),
                )
                .child(
                    col()
                        .key("sig")
                        .gap(2.0)
                        .child(
                            text(cert.text.clone())
                                .key("name")
                                .font(SIGN.font.lh(28.0))
                                .color(p.text()),
                        )
                        .child(
                            el().key("line")
                                .size(200.0, p.hair.max(1.0))
                                .fill(k.c.border_base),
                        ),
                )
        }))
        .child(
            row()
                .key("actions")
                .h(SHARE_ROWS[6])
                .gap(8.0)
                .center_y()
                .child(el().key("space").grow(1.0))
                .child(
                    Button::new("Keep playing")
                        .variant(ButtonVariant::Ghost)
                        .on_press(act(&look.keep))
                        .build_push(k)
                        .key("keep"),
                )
                .child(
                    Button::new("Continue")
                        .variant(ButtonVariant::Contrast)
                        .on_press(act(&look.done))
                        .build_push(k)
                        .key("continue"),
                ),
        );
    // The surface, from the card's rect to the pill's.
    let pill_rect = [
        SCORE_AT[0] - home[0],
        SCORE_AT[1] - home[1],
        PILL[0],
        PILL[1],
    ];
    let card_rect = [0.0, 0.0, w, share_h];
    let [sx, sy, sw, sh]: [f32; 4] =
        std::array::from_fn(|n| lerp(card_rect[n], pill_rect[n], morph));
    let fill = if k.dark {
        k.c.surface_raised
    } else {
        k.c.bg_base.mix(k.c.surface_raised, morph)
    };
    let surface = el()
        .key("surface")
        .absolute(sx, sy)
        .size(sw, sh)
        .radius(lerp(20.0, PILL[1] * 0.5, morph))
        .continuous_corners()
        .fill(fill)
        .radial_gradient(
            [0.14, 0.28],
            [240.0, 160.0],
            [(0.0, ember.alpha(0.12 * warm)), (1.0, ember.alpha(0.0))],
        )
        .border(p.hair, k.c.border_weak);
    let surface = k.elevate(surface, &k.elevation.floating);
    let surface = if warm > 0.02 {
        surface.shadow(0.0, 96.0, ember.alpha(0.16 * warm))
    } else {
        surface
    }
    .opacity(surface_in * (1.0 - pill))
    .opacity_motion(Anim::INSTANT);
    // The number flies to the pill's, shrinking to its size.
    let tiny = 14.0 / BIG_SIZE;
    let num_from = [SHARE_PAD, SHARE_PAD + SHARE_ROWS[0] + SHARE_GAP];
    let num_to = [
        SCORE_AT[0] + 12.0 - home[0],
        SCORE_AT[1] + (PILL[1] - BIG_LINE * tiny) * 0.5 - home[1],
    ];
    let flying_number = folding.then(|| {
        number("number-fold", ember_tone(p, lerp(0.85, 0.7, morph)))
            .absolute(
                lerp(num_from[0], num_to[0], morph),
                lerp(num_from[1], num_to[1], morph),
            )
            .scale(lerp(1.0, tiny, morph))
            .origin(0.0, 0.0)
            .opacity(1.0 - pill)
            .opacity_motion(Anim::INSTANT)
    });
    Some(
        el().key("share")
            .absolute(home[0], home[1])
            .size(w, share_h)
            .offset(from[0] * (1.0 - rise), from[1] * (1.0 - rise))
            .scale(if reduced {
                1.0
            } else {
                lerp(0.86, 1.0, settle)
            })
            .offset_motion(Anim::INSTANT)
            .scale_motion(Anim::INSTANT)
            .child(surface)
            .child(content)
            .children(flying_number),
    )
}
