// No native panel, App or recorder. Drive the actual fixture through both
// pointer handlers and composite its transparent window against a flat mat.
#[test]
fn offer_navigation_preview() {
    use crate::rune_ui::offscreen;
    use std::fmt::Write;
    let Some(folder) = std::env::var_os("HEX_OFFER_PREVIEW") else {
        return;
    };
    let dir = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
    let mut trace = String::from("corner,frame,key,x,y,w,h\n");
    for corner in [
        MeetingIndicatorCorner::TopRight,
        MeetingIndicatorCorner::TopLeft,
        MeetingIndicatorCorner::BottomRight,
        MeetingIndicatorCorner::BottomLeft,
    ] {
        let variant = std::env::var("HEX_OFFER_VARIANT").unwrap_or_default();
        let prototype = variant != "before";
        if prototype {
            let widths = [false, true].map(|open| {
                let (probe, _, _) =
                    runtime_with(offer(slack_huddle()), open, true, MENU, measure_page);
                probe
                    .elements()
                    .iter()
                    .find(|el| el.key.as_deref() == Some("page"))
                    .unwrap()
                    .rect[2]
            });
            POSE.set((widths, 0.0));
        }
        let reduced = variant == "reduced";
        let kit = Paint::new(true, 2.0).k;
        let spring = if reduced {
            kit.motion.overlay
        } else {
            kit.motion.disclosure
        };
        let mut velocity = 0.0;
        let mut size = if prototype { MENU } else { PANEL };
        let (mut rt, commands, chosen) = runtime_with(
            offer(slack_huddle()),
            false,
            true,
            size,
            if prototype { fitted_offer } else { view },
        );
        rt.transparent = true;
        rt.update(|c, tx| {
            c.corner = corner;
            tx.instant();
        });
        let (resize, sizes) = mpsc::channel();
        if !prototype {
            rt.state.resize = Some(Arc::new(move |size| resize.send(size).unwrap()));
        }
        let mut renderer = offscreen::renderer(&gpu.device);
        rt.settle_all();
        rt.frame(0.0);
        for frame in 0..138 {
            let click = match frame {
                24 => Some("offer-more"),
                84 => Some("offer-cancel"),
                _ => None,
            };
            if let Some(key) = click {
                let (x, y) = rt.find(key).unwrap();
                rt.pointer_move(x, y);
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
                rt.frame(0.0);
                if let Ok(requested) = sizes.try_recv() {
                    size = requested;
                    rt.set_viewport(size, 2.0);
                }
                assert_eq!(rt.state.more_open, frame == 24);
            }
            if prototype {
                let (widths, value) = POSE.get();
                let (value, v) = spring.sample(
                    value,
                    velocity,
                    if rt.state.more_open { 1.0 } else { 0.0 },
                    1.0 / 60.0,
                );
                velocity = v;
                POSE.set((widths, value.clamp(0.0, 1.0)));
                rt.update(|_, tx| tx.instant());
            }
            rt.frame(1.0 / 60.0);
            let ox = if corner.is_right() {
                580.0 - size.0
            } else {
                20.0
            };
            let oy = if corner.is_top() {
                12.0
            } else {
                100.0 - size.1
            };
            for el in rt.elements() {
                if let Some(key) = el.key.as_deref().or(el.text.as_deref()) {
                    let [x, y, w, h] = el.rect;
                    writeln!(
                        trace,
                        "{corner:?},{frame},{key},{},{},{w},{h}",
                        ox + x,
                        oy + y
                    )
                    .unwrap();
                }
            }
            if corner == MeetingIndicatorCorner::TopRight {
                let w = (2.0 * size.0) as u32;
                let h = (2.0 * size.1) as u32;
                let target = offscreen::target(&gpu.device, w, h);
                let rgba = offscreen::paint(&gpu, &mut renderer, &target, &mut rt);
                let mut mat = vec![36u8; 1200 * 192 * 4];
                for p in mat.chunks_exact_mut(4) {
                    p[3] = 255;
                }
                for y in 0..h as usize {
                    for (x, p) in rgba[y * w as usize * 4..(y + 1) * w as usize * 4]
                        .chunks_exact(4)
                        .enumerate()
                    {
                        let i = 4 * ((y + (oy * 2.0) as usize) * 1200 + x + (ox * 2.0) as usize);
                        for k in 0..3 {
                            mat[i + k] =
                                (p[k] as u32 + (255 - p[3]) as u32 * 36 / 255).min(255) as u8;
                        }
                    }
                }
                rune::render::write_png(&dir.join(format!("{frame:03}.png")), 1200, 192, &mat)
                    .unwrap();
            }
        }
        assert!(commands.try_recv().is_err() && chosen.lock().unwrap().is_empty());
    }
    std::fs::write(dir.join("trace.csv"), trace).unwrap();
}

thread_local! { static POSE: Cell<([f32;2], f32)> = const {Cell::new(([0.;2],0.))}; }

fn measure_page(c: &Capsule, cx: &mut Cx<Capsule>) -> El {
    col().w_full().items_start().child(offer_page(
        c,
        Paint::new(cx.system_dark(), cx.env().scale),
        c.more_open,
    ))
}

fn offer_page(c: &Capsule, p: Paint, rules: bool) -> El {
    let Some(Kind::Offer(id, candidate)) = &c.kind else {
        return row();
    };
    let (dot, title, subtitle, actions) = offer::actions(p, *id, candidate, rules);
    let label = col()
        .items_start()
        .child(
            text(title.clone())
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .children((!subtitle.is_empty()).then(|| {
            text(subtitle)
                .key("subtitle")
                .font(p.k.ty.small)
                .color(p.faint())
        }));
    row()
        .key("page")
        .h(56.0)
        .pl(16.0)
        .pr(ACTIONS_INSET)
        .center_y()
        .gap(12.0)
        .child(status_dot(dot))
        .child(label)
        .child(row().key("actions").gap(8.0).children(actions))
}

// The shape and page have independent origins. A page is mounted at its
// measured final size and corner, never laid out to the moving shell width.
fn fitted_offer(c: &Capsule, cx: &mut Cx<Capsule>) -> El {
    let reduced = std::env::var("HEX_OFFER_VARIANT").as_deref() == Ok("reduced");
    cx.reduced_motion(reduced);
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let ([compact, wide], t) = POSE.get();
    let split = if reduced { 0.5 } else { 0.69 };
    let rules = t > split;
    let alpha = if rules {
        (t - split) / (1.0 - split)
    } else {
        (split - t) / split
    };
    let alpha = (alpha * 1.45).min(1.0);
    let fraction = if reduced {
        if rules { 1.0 } else { 0.0 }
    } else {
        (t / split).min(1.0)
    };
    let width = compact + fraction * (wide - compact);
    let x = |w| {
        if c.corner.is_right() {
            MENU.0 - CAPSULE_INSET - w
        } else {
            CAPSULE_INSET
        }
    };
    let body = row()
        .key("pill")
        .absolute(x(width), CAPSULE_INSET)
        .w(width)
        .h(56.0)
        .max_w(MEETING_MENU_WIDTH - 2.0 * CAPSULE_INSET)
        .radius(panel_radius(p))
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .hit();
    let w = if rules { wide } else { compact };
    let page = offer_page(c, p, rules)
        .absolute(x(w), CAPSULE_INSET)
        .w(w)
        .opacity(alpha);
    let page = if rules != c.more_open || alpha < 0.2 {
        page.inert().a11y_hidden()
    } else {
        page
    };
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .child(body)
        .child(page)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
}
