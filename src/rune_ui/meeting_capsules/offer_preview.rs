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
    let mut trace = String::from("corner,frame,key,x,y,w,h,radius\n");
    for corner in [
        MeetingIndicatorCorner::TopRight,
        MeetingIndicatorCorner::TopLeft,
        MeetingIndicatorCorner::BottomRight,
        MeetingIndicatorCorner::BottomLeft,
    ] {
        let variant = std::env::var("HEX_OFFER_VARIANT").unwrap_or_default();
        let dark = !variant.contains("light");
        let scale = if variant.ends_with("-1x") { 1.0 } else { 2.0 };
        let frame_study = variant.starts_with("quiet-frame");
        if variant.ends_with("-fr") {
            crate::desktop::i18n::apply(Some("fr"));
        }
        REFINED.set(frame_study);
        QUIET.set(variant.starts_with("quiet"));
        DISMISSAL.set(match variant.as_str() {
            "quiet-label" => Some(Dismissal::Label),
            "quiet-cross" => Some(Dismissal::Cross),
            v if v.starts_with("quiet-frame") || v.starts_with("quiet-label-") => {
                Some(Dismissal::Label)
            }
            _ => None,
        });
        let prototype = variant != "before";
        let candidate = if variant == "quiet-unknown" {
            bare_browser()
        } else {
            slack_huddle()
        };
        if prototype {
            let widths = measure_offer(&candidate, dark, scale);
            POSE.set((widths, 0.0));
        }
        let reduced = variant.ends_with("reduced");
        let kit = Paint::new(dark, 2.0).k;
        let spring = if reduced {
            kit.motion.overlay
        } else {
            kit.motion.disclosure
        };
        let mut velocity = 0.0;
        let mut size = if prototype { preview_viewport() } else { PANEL };
        let mat_width = MENU.0.max(size.0) + 40.0;
        let (mut rt, commands, chosen) = runtime_with(
            offer(candidate),
            false,
            dark,
            size,
            if prototype { fitted_offer } else { view },
        );
        rt.transparent = true;
        rt.set_viewport(size, scale);
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
        for frame in 0..=138 {
            if frame == 138 || (frame == 81 && frame_study) {
                rt.pointer_move(0.0, 0.0);
                let (target, name) = if frame == 81 {
                    ("always-record", tr("Always record"))
                } else if frame_study {
                    ("record", tr("Record"))
                } else {
                    ("offer-more", tr("More"))
                };
                let focused = |rt: &Runtime<Capsule>| {
                    rt.elements().iter().any(|e| {
                        e.key.as_deref() == Some(target)
                            && e.focus_visible
                            && e.role == "button"
                            && e.label.as_deref() == Some(name)
                    })
                };
                for _ in 0..5 {
                    rt.key(rune::KeyEvent {
                        key: rune::Key::Tab,
                        mods: Default::default(),
                    });
                    rt.frame(0.0);
                    if focused(&rt) {
                        break;
                    }
                }
                assert!(focused(&rt), "Tab must reach {name}");
            }
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
                    rt.set_viewport(size, scale);
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
                mat_width - 20.0 - size.0
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
                        "{corner:?},{frame},{key},{},{},{w},{h},{}",
                        ox + x,
                        oy + y,
                        el.radius
                    )
                    .unwrap();
                }
            }
            if corner == MeetingIndicatorCorner::TopRight {
                let w = (scale * size.0) as u32;
                let h = (scale * size.1) as u32;
                let target = offscreen::target(&gpu.device, w, h);
                let rgba = offscreen::paint(&gpu, &mut renderer, &target, &mut rt);
                let mw = (mat_width * scale) as usize;
                let mh = 96 * scale as usize;
                let mut mat = vec![36u8; mw * mh * 4];
                for p in mat.chunks_exact_mut(4) {
                    p[3] = 255;
                }
                for y in 0..h as usize {
                    for (x, p) in rgba[y * w as usize * 4..(y + 1) * w as usize * 4]
                        .chunks_exact(4)
                        .enumerate()
                    {
                        let i = 4 * ((y + (oy * scale) as usize) * mw + x + (ox * scale) as usize);
                        for k in 0..3 {
                            mat[i + k] =
                                (p[k] as u32 + (255 - p[3]) as u32 * 36 / 255).min(255) as u8;
                        }
                    }
                }
                rune::render::write_png(
                    &dir.join(format!("{frame:03}.png")),
                    mw as u32,
                    mh as u32,
                    &mat,
                )
                .unwrap();
            }
        }
        assert!(commands.try_recv().is_err() && chosen.lock().unwrap().is_empty());
    }
    std::fs::write(dir.join("trace.csv"), trace).unwrap();
}

thread_local! { static POSE: Cell<([f32;2], f32)> = const {Cell::new(([0.;2],0.))}; }
thread_local! { static QUIET: Cell<bool> = const {Cell::new(false)}; }
thread_local! { static REFINED: Cell<bool> = const {Cell::new(false)}; }
// Both end controls use the same push metrics. Read the real compact face in
// the width pass, not a second copy of the kit's Large height and radius.
thread_local! { static FACE: Cell<(f32,f32)> = const {Cell::new((0.0,0.0))}; }

fn measure_offer(candidate: &MeetingCandidate, dark: bool, scale: f32) -> [f32; 2] {
    [false, true].map(|open| {
        let (mut probe, _, _) =
            runtime_with(offer(candidate.clone()), open, dark, MENU, measure_page);
        probe.set_viewport(MENU, scale);
        probe.frame(0.0);
        probe.settle_all();
        probe.frame(0.0);
        let els = probe.elements();
        if !open {
            let button = els
                .iter()
                .find(|e| e.key.as_deref() == Some("record"))
                .unwrap();
            FACE.set((button.rect[3], button.radius));
        }
        els.iter()
            .find(|e| e.key.as_deref() == Some("page"))
            .unwrap()
            .rect[2]
    })
}

fn shell_height() -> f32 {
    if REFINED.get() {
        FACE.get().0 + 2.0 * ACTIONS_INSET
    } else {
        56.0
    }
}

fn preview_viewport() -> (f32, f32) {
    let ([a, b], _) = POSE.get();
    if REFINED.get() {
        (MENU.0.max((a.max(b) + 2.0 * CAPSULE_INSET).ceil()), MENU.1)
    } else {
        MENU
    }
}

#[test]
fn fitted_offer_right_corners_follow_the_real_face() {
    REFINED.set(true);
    QUIET.set(true);
    DISMISSAL.set(Some(Dismissal::Label));
    let near = |a: f32, b: f32| assert!((a - b).abs() < 0.025, "{a} vs {b}");
    for lang in ["en", "fr"] {
        crate::desktop::i18n::apply(Some(lang)); // thread-local in tests
        for scale in [1.0, 2.0] {
            let mut c = slack_huddle();
            c.named = true;
            c.title = "Product review with the support team".into();
            let widths = measure_offer(&c, true, scale);
            for open in [false, true] {
                POSE.set((widths, if open { 1.0 } else { 0.0 }));
                let size = preview_viewport();
                let (mut rt, _, _) = runtime_with(offer(c.clone()), open, true, size, fitted_offer);
                rt.set_viewport(size, scale);
                rt.frame(0.0);
                rt.settle_all();
                rt.frame(0.0);
                let els = rt.elements();
                let get = |key: &str| els.iter().find(|e| e.key.as_deref() == Some(key)).unwrap();
                let shell = get("pill");
                let face = get(if open { "always-record" } else { "record" });
                let [x, y, w, h] = shell.rect;
                let [bx, by, bw, bh] = face.rect;
                assert!(
                    (x + w - bx - bw - ACTIONS_INSET).abs() < 0.025,
                    "{lang} {scale}x open={open} widths={widths:?}, shell {:?}, control {:?}",
                    shell.rect,
                    face.rect
                );
                assert!(bh >= 28.0);
                near(by - y, ACTIONS_INSET);
                near(y + h - by - bh, ACTIONS_INSET);
                near(x + w - bx - bw, ACTIONS_INSET);
                near(shell.radius - face.radius, ACTIONS_INSET);
                for key in if open {
                    ["offer-cancel", "never-offer", "always-record"]
                } else {
                    ["offer-more", "not-now", "record"]
                } {
                    near(get(key).radius, face.radius);
                }
                for e in &els {
                    if e.text.is_some() {
                        let [u, v, ew, eh] = e.rect;
                        assert!(
                            u >= x && u + ew <= x + w + 0.025 && v >= y && v + eh <= y + h + 0.025,
                            "{:?} outside shell",
                            e.text
                        );
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dismissal {
    Label,
    Cross,
}
thread_local! { static DISMISSAL: Cell<Option<Dismissal>> = const {Cell::new(None)}; }

// Same scoped commands as the original; only the faces change in this study.
fn quiet_face(p: Paint, key: &str, label: &str, primary: bool) -> El {
    if let Some(choice) = DISMISSAL.get() {
        let secondary_icon = |glyph, name: &str| {
            IconButton::new(glyph)
                .variant(IconButtonVariant::Neutral)
                .control_size(ControlSize::Large)
                .help(name)
                .build(p.k)
                .w(34.0)
                .key(key)
        };
        match key {
            "offer-more" => {
                return secondary_icon(&crate::rune_ui::meetings::MORE_ICON, tr("More"));
            }
            "not-now" if choice == Dismissal::Cross => return secondary_icon(CLOSE, label),
            "record" => {
                return Button::new(label)
                    .variant(ButtonVariant::Submit)
                    .build_push(p.k)
                    .key(key);
            }
            _ => {}
        }
    }
    if key == "offer-more" {
        return IconButton::new(&crate::rune_ui::meetings::MORE_ICON)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Large)
            .label(tr("More"))
            .build(p.k)
            .w(34.0)
            .key(key);
    }
    button_face(p, key, label, primary)
}

#[test]
fn offer_dismissal_faces_keep_named_keyboard_actions() {
    QUIET.set(true);
    for style in [Dismissal::Label, Dismissal::Cross] {
        DISMISSAL.set(Some(style));
        let widths = [false, true].map(|open| {
            let (rt, _, _) = runtime_with(offer(slack_huddle()), open, true, MENU, measure_page);
            rt.elements()
                .iter()
                .find(|e| e.key.as_deref() == Some("page"))
                .unwrap()
                .rect[2]
        });
        for (active, tabs) in [("offer-more", 1), ("not-now", 2), ("record", 3)] {
            POSE.set((widths, 0.0));
            let (mut rt, commands, chosen) =
                runtime_with(offer(slack_huddle()), false, true, MENU, fitted_offer);
            let (arm, armed) = mpsc::channel();
            rt.state.set_offer_timer(Some(Box::new(move |delay, fire| {
                arm.send((delay, fire)).unwrap()
            })));
            let (delay, _) = armed.try_recv().unwrap();
            assert_eq!(delay, OFFER_TIMEOUT);
            let key = |rt: &mut Runtime<Capsule>, key| {
                rt.key(rune::KeyEvent {
                    key,
                    mods: Default::default(),
                });
                rt.frame(0.0);
            };
            assert!(!rt.elements().iter().any(|e| e.focused));
            key(&mut rt, rune::Key::Enter);
            for (name, id) in [
                (tr("More"), "offer-more"),
                (tr("Not now"), "not-now"),
                (tr("Record"), "record"),
            ]
            .into_iter()
            .take(tabs)
            {
                key(&mut rt, rune::Key::Tab);
                assert!(rt.elements().iter().any(|e| e.focus_visible
                    && e.key.as_deref() == Some(id)
                    && e.role == "button"
                    && e.label.as_deref() == Some(name)
                    && e.rect[3] == 28.0));
            }
            assert!(commands.try_recv().is_err());
            key(&mut rt, rune::Key::Text(" ".into()));
            match active {
                "offer-more" => assert!(rt.state.more_open),
                "not-now" => assert!(matches!(
                    commands.try_recv(),
                    Ok(ControllerCommand::Dismiss)
                )),
                _ => assert!(matches!(
                    commands.try_recv(),
                    Ok(ControllerCommand::Record(_, Some(_)))
                )),
            }
            assert!(
                armed.try_recv().is_err()
                    && commands.try_recv().is_err()
                    && chosen.lock().unwrap().is_empty()
            );
        }
    }
}

fn offer_service(candidate: &MeetingCandidate) -> Option<crate::calendar::CallProvider> {
    // An unknown browser's app name, bundle id and tab title are not a call provider.
    let identity = match candidate.source {
        MeetingSource::BrowserCall => candidate.site.as_deref()?,
        source => source.label(),
    };
    crate::calendar::CallProvider::from_meeting_app(identity)
}

fn provider_mark(p: Paint, candidate: &MeetingCandidate) -> El {
    use crate::calendar::CallProvider;
    let service = offer_service(candidate);
    let mark = if service == Some(CallProvider::Slack) {
        // A small fixed rendition from Slack.app's local app icon, not an
        // app lookup, URL or installed-app dependency in the replay.
        static ICON: std::sync::OnceLock<rune::Pixels> = std::sync::OnceLock::new();
        let pixels = ICON.get_or_init(|| {
            let bytes = include_bytes!("../../../tests/fixtures/offer-slack-icon.png");
            let mut png = png::Decoder::new(std::io::Cursor::new(bytes))
                .read_info()
                .unwrap();
            let mut buf = vec![0; png.output_buffer_size().unwrap()];
            let info = png.next_frame(&mut buf).unwrap();
            assert_eq!(info.color_type, png::ColorType::Rgba);
            let pixels = rune::Pixels::new();
            pixels.set(info.width, info.height, buf);
            pixels
        });
        rune::el().size(24.0, 24.0).pixels(pixels)
    } else {
        let icon = service
            .map(crate::rune_ui::platform_settings::provider_icon)
            .unwrap_or(&crate::rune_ui::icons::CALL);
        row()
            .size(24.0, 24.0)
            .center()
            .child(rune::icon(icon, 18.0).color(p.muted()))
    };
    mark.key("provider-mark").a11y_hidden()
}

#[test]
fn an_offer_mark_uses_the_detected_service_not_the_browser_or_title() {
    use crate::calendar::CallProvider;
    assert_eq!(offer_service(&slack_huddle()), Some(CallProvider::Slack));
    let mut c = bare_browser();
    c.title = "Slack huddle".into();
    assert_eq!(offer_service(&c), None);
    c.site = Some("meet.google.com".into());
    assert_eq!(offer_service(&c), Some(CallProvider::GoogleMeet));
    c.site = Some("app.slack.com.example.org".into());
    assert_eq!(offer_service(&c), None);
}

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
    let quiet = QUIET.get();
    let (dot, title, subtitle, actions) = offer::actions_with(
        p,
        *id,
        candidate,
        rules,
        if quiet { quiet_face } else { button_face },
    );
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
    let mark = if quiet {
        provider_mark(p, candidate)
    } else {
        status_dot(dot)
    };
    let actions = row()
        .key("actions")
        .center_y()
        .gap(ACTIONS_INSET)
        .children(actions);
    if REFINED.get() {
        // The 24 pt Slack asset has ~2.5 pt of clear canvas on each side.
        // Treat mark + name as one object, separated from the controls.
        return row()
            .key("page")
            .h(shell_height())
            .pl(10.0)
            .pr(ACTIONS_INSET)
            .center_y()
            .gap(16.0)
            .child(
                row()
                    .key("identity")
                    .center_y()
                    .gap(5.0)
                    .child(mark)
                    .child(label),
            )
            .child(actions);
    }
    row()
        .key("page")
        .h(56.0)
        .pl(16.0)
        .pr(ACTIONS_INSET)
        .center_y()
        .gap(12.0)
        .child(mark)
        .child(label)
        .child(actions)
}

// The shape and page have independent origins. A page is mounted at its
// measured final size and corner, never laid out to the moving shell width.
fn fitted_offer(c: &Capsule, cx: &mut Cx<Capsule>) -> El {
    let reduced = std::env::var("HEX_OFFER_VARIANT").is_ok_and(|s| s.ends_with("reduced"));
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
    } else if wide < compact {
        // With no Always record action, the rules can be the smaller page.
        // Let the wider page fade before shrinking its shell.
        ((t - split) / (1.0 - split)).max(0.0)
    } else {
        (t / split).min(1.0)
    };
    let width = compact + fraction * (wide - compact);
    let x = |w| {
        if c.corner.is_right() {
            preview_viewport().0 - CAPSULE_INSET - w
        } else {
            CAPSULE_INSET
        }
    };
    let height = shell_height();
    let y = if c.corner.is_top() {
        CAPSULE_INSET
    } else {
        MENU.1 - CAPSULE_INSET - height
    };
    let body = row()
        .key("pill")
        .absolute(x(width), y)
        .w(width)
        .h(height)
        .max_w(preview_viewport().0 - 2.0 * CAPSULE_INSET)
        .radius(if REFINED.get() {
            FACE.get().1 + ACTIONS_INSET
        } else {
            panel_radius(p)
        })
        .fill(p.k.c.bg_base)
        .border(p.hair, p.k.c.border_base)
        .hit();
    let w = if rules { wide } else { compact };
    let page = offer_page(c, p, rules)
        .absolute(x(w), y)
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
