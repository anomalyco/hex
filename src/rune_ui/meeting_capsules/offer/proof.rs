// Fixtures supply only the inputs to detection. They do not supply titles,
// layout, buttons, springs or an alternate view. No microphone is opened.
fn provider_cases() -> Vec<(&'static str, MeetingCandidate)> {
    use crate::dictation::context::ContextSnapshot;
    use crate::meeting::detection::candidate_from_microphone;
    use crate::microphone_activity::ActiveMicrophoneApplication;
    const BRAVE: &str = "com.brave.Browser";
    [
        (
            "meet",
            BRAVE,
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Google Meet",
        ),
        ("zoom", "us.zoom.xos", "zoom.us", None, ""),
        ("teams", "com.microsoft.teams2", "Microsoft Teams", None, ""),
        ("facetime", "com.apple.FaceTime", "FaceTime", None, ""),
        (
            "banshee",
            BRAVE,
            "Brave Browser",
            Some("https://banshee.app/demo"),
            "",
        ),
        ("slack", "com.tinyspeck.slackmacgap", "Slack", None, ""),
        ("unknown", BRAVE, "Brave Browser", None, ""),
        ("whatsapp", "net.whatsapp.WhatsApp", "WhatsApp", None, ""),
        ("gather", "com.gather.GatherV2", "Gather", None, ""),
        ("discord", "com.hnc.Discord", "Discord", None, ""),
        (
            "signal",
            "org.whispersystems.signal-desktop",
            "Signal",
            None,
            "",
        ),
        ("telegram", "ru.keepcoder.Telegram", "Telegram", None, ""),
        ("wechat", "com.tencent.xinWeChat", "WeChat", None, ""),
        ("line", "jp.naver.line.mac", "LINE", None, ""),
        ("webex", "com.webex.meetingmanager", "Webex", None, ""),
        ("element", "im.riot.app", "Element", None, ""),
        ("phone", "com.apple.mobilephone", "Phone", None, ""),
        (
            "voice",
            BRAVE,
            "Brave Browser",
            Some("https://voice.google.com/"),
            "",
        ),
        (
            "messenger",
            BRAVE,
            "Brave Browser",
            Some("https://messenger.com/"),
            "",
        ),
        (
            "groupme",
            BRAVE,
            "Brave Browser",
            Some("https://web.groupme.com/"),
            "",
        ),
        (
            "teams-web",
            BRAVE,
            "Brave Browser",
            Some("https://teams.microsoft.com/"),
            "",
        ),
        (
            "named",
            BRAVE,
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Meet - Design review",
        ),
    ]
    .into_iter()
    .map(|(slug, bundle, name, url, title)| {
        let app = ActiveMicrophoneApplication {
            pid: 42,
            bundle_id: bundle.into(),
            name: name.into(),
        };
        let cx = ContextSnapshot {
            application: Some(name.into()),
            window_title: Some(title.into()),
            browser_url: url.map(|s| s.parse().unwrap()),
            ..Default::default()
        };
        (slug, candidate_from_microphone(&app, &cx).unwrap())
    })
    .collect()
}

#[test]
fn the_offer_keeps_its_corner_controls_and_clear_input_margin() {
    for lang in ["en", "fr"] {
        crate::desktop::i18n::apply(Some(lang));
        for reduced in [false, true] {
            for right in [false, true] {
                let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
                rt.state.corner = if right {
                    MeetingIndicatorCorner::BottomRight
                } else {
                    MeetingIndicatorCorner::TopLeft
                };
                rt.set_system_reduced_motion(reduced);
                rt.update(|_, tx| tx.instant());
                rt.frame(0.);
                let size = rt.state.presentation.window();
                let mut widths = Vec::new();
                for frame in 0..100 {
                    match frame {
                        10 => tap(&mut rt, "offer-more"),
                        60 => tap(&mut rt, "offer-cancel"),
                        _ => (),
                    }
                    advance_offer(&mut rt);
                    let els = rt.elements();
                    let get =
                        |key: &str| els.iter().find(|e| e.key.as_deref() == Some(key)).unwrap();
                    let [x, y, w, h] = get("pill").rect;
                    let near = |a: f32, b: f32| {
                        assert!((a - b).abs() < 0.03, "{lang}, frame {frame}: {a} vs {b}")
                    };
                    near(h, 44.);
                    near(if right { size.0 - x - w } else { x }, CAPSULE_INSET);
                    for [l, t, r, b] in rt.hit_regions() {
                        assert!(*l >= x - 0.03 && *r <= x + w + 0.03 && *t >= y && *b <= y + h);
                    }
                    assert!(!rt.hit_regions().is_empty());
                    if [0, 55, 99].contains(&frame) {
                        let end = get(if frame == 55 {
                            "always-record"
                        } else {
                            "record"
                        });
                        let [bx, by, bw, bh] = end.rect;
                        near(by - y, 8.);
                        near(x + w - bx - bw, 8.);
                        near(bh, 28.);
                        near(get("pill").radius - end.radius, 8.);
                        let [_, _, mw, mh] = get("provider-mark").rect;
                        near(mw, 24.);
                        near(mh, 24.);
                    }
                    widths.push(w);
                }
                assert!(widths[55] > widths[0]);
                for s in widths[..60].windows(2) {
                    assert!(s[1] >= s[0] - 0.02);
                }
                for s in widths[60..].windows(2) {
                    assert!(s[1] <= s[0] + 0.02);
                }
                if reduced {
                    assert!(widths.iter().all(|w| *w == widths[0] || *w == widths[55]));
                }
                assert!(commands.try_recv().is_err() && chosen.lock().unwrap().is_empty());
            }
        }
    }
}

#[test]
fn the_offers_buttons_take_the_pointer_while_its_body_grabs() {
    let (mut rt, _, _) = runtime(offer(slack_huddle()), false, true, PANEL);
    landed(&mut rt);
    for key in ["offer-more", "not-now", "record"] {
        let (x, y) = rt.find(key).unwrap();
        rt.pointer_move(x, y);
        rt.frame(1. / 60.);
        assert_eq!(rt.cursor(), Cursor::Pointer, "{key}");
    }
    // Between the name and its buttons the capsule itself drags.
    let (x, y) = rt.find("title").unwrap();
    rt.pointer_move(x, y);
    rt.frame(1. / 60.);
    assert_eq!(rt.cursor(), Cursor::Grab);
}

#[test]
fn the_offer_has_no_default_key_and_names_each_focus_target() {
    let key = |rt: &mut Runtime<Capsule>, key| {
        rt.key(rune::KeyEvent {
            key,
            mods: Default::default(),
        });
        rt.frame(0.);
    };
    for tabs in 1..=3 {
        let (mut rt, commands, chosen) = runtime(offer(slack_huddle()), false, true, PANEL);
        assert!(!rt.elements().iter().any(|e| e.focused));
        key(&mut rt, rune::Key::Enter);
        for (id, label) in [
            ("offer-more", "More"),
            ("not-now", "Not now"),
            ("record", "Record"),
        ]
        .into_iter()
        .take(tabs)
        {
            key(&mut rt, rune::Key::Tab);
            assert!(rt.elements().iter().any(|e| e.focus_visible
                && e.role == "button"
                && e.key.as_deref() == Some(id)
                && e.label.as_deref() == Some(label)));
        }
        assert!(commands.try_recv().is_err());
        key(&mut rt, rune::Key::Text(" ".into()));
        match tabs {
            1 => assert!(rt.state.more_open()),
            2 => assert!(matches!(
                commands.try_recv(),
                Ok(ControllerCommand::Dismiss)
            )),
            _ => assert!(matches!(
                commands.try_recv(),
                Ok(ControllerCommand::Record(_, Some(_)))
            )),
        }
        assert!(commands.try_recv().is_err() && chosen.lock().unwrap().is_empty());
    }
}

#[test]
fn detected_offer_sheet() {
    use crate::rune_ui::offscreen;
    use std::fmt::Write;
    let Some(dir) = std::env::var_os("HEX_DETECTED_OFFERS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
    let mut renderer = offscreen::renderer(&gpu.device);
    let mut report = String::new();
    for lang in ["en", "fr"] {
        crate::desktop::i18n::apply(Some(lang));
        for (slug, candidate) in provider_cases() {
            for dark in [true, false] {
                let (mut rt, commands, chosen) =
                    runtime(offer(candidate.clone()), false, dark, PANEL);
                let size = rt.state.presentation.window();
                rt.transparent = true;
                rt.set_viewport(size, 2.0);
                let native =
                    offer_surface::local_bundle(&candidate).and_then(offer_surface::native_icon);
                if lang == "en" && dark {
                    writeln!(
                        report,
                        "{slug}\t{}\t{}\t{}",
                        candidate.offer_title(),
                        if native.is_some() {
                            "installed app"
                        } else {
                            offer_surface::fallback_name(&candidate)
                        },
                        size.0
                    )
                    .unwrap();
                }
                rt.update(move |c, tx| {
                    c.presentation.icon = native;
                    tx.instant()
                });
                rt.frame(0.);
                landed(&mut rt);
                let w = (size.0 * 2.0) as u32;
                let h = (size.1 * 2.0) as u32;
                let target = offscreen::target(&gpu.device, w, h);
                let count = if slug == "slack" { 96 } else { 1 };
                let theme = if dark { "dark" } else { "light" };
                for frame in 0..count {
                    if frame == 18 {
                        tap(&mut rt, "offer-more");
                    } else if frame == 60 {
                        tap(&mut rt, "offer-cancel");
                    }
                    advance_offer(&mut rt);
                    let bytes = offscreen::paint(&gpu, &mut renderer, &target, &mut rt);
                    rune::render::write_png(
                        &dir.join(format!("{lang}-{theme}-{slug}-{frame:03}.png")),
                        w,
                        h,
                        &bytes,
                    )
                    .unwrap();
                }
                assert!(commands.try_recv().is_err() && chosen.lock().unwrap().is_empty());
            }
        }
    }
    std::fs::write(dir.join("providers.tsv"), report).unwrap();
}
// Replay Record (not More) with the worker's Started event 500 ms later.
#[test]
fn a_record_click_keeps_its_corner_until_the_worker_is_live() {
    use MeetingIndicatorCorner::*;
    for corner in [TopRight, BottomRight, TopLeft, BottomLeft] {
        for reduced in [false, true] {
            for started_frame in [1, 75] {
                let (mut rt, commands, _) = runtime(offer(slack_huddle()), false, false, PANEL);
                rt.update(move |c, tx| {
                    c.corner = corner;
                    tx.instant()
                });
                rt.set_system_reduced_motion(reduced);
                rt.frame(0.);
                let size = rt.state.presentation.window();
                let (x, y) = rt.find("record").unwrap();
                rt.pointer_move(x, y);
                tap(&mut rt, "record");
                let Ok(ControllerCommand::Record(_, Some(id))) = commands.try_recv() else {
                    panic!("no Record")
                };
                let mut host = offer::Host::Offered(id);
                assert_eq!(
                    host.route(&ControllerEvent::Starting(Some(id))),
                    offer::Route::Keep
                );
                assert_eq!(
                    host.route(&ControllerEvent::OfferWithdrawn(id)),
                    offer::Route::Ordinary
                );
                rt.update(|c, _| c.expire_offer());
                let mut last = (f32::MAX, 44.);
                for frame in 0..started_frame + 70 {
                    if frame == started_frame {
                        let event = started("fixture");
                        assert_eq!(host.route(&event), offer::Route::Inherit);
                        let mut model = PillModel::default();
                        model.apply(&event, Instant::now());
                        let r = model.pill(true).unwrap().clone();
                        rt.update(move |c, tx| c.start_recording(r, tx));
                    }
                    if [1, started_frame + 50].contains(&frame) {
                        rt.pointer_down(x, y);
                        rt.pointer_up(x, y);
                    }
                    advance_offer(&mut rt);
                    let elements = rt.elements();
                    let [x, y, w, h] = elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some("pill"))
                        .unwrap()
                        .rect;
                    let near = |x: f32, y: f32| (x - y).abs() < 0.03;
                    assert!(near(
                        if corner.is_right() { size.0 - x - w } else { x },
                        CAPSULE_INSET
                    ));
                    assert!(near(
                        if corner.is_top() { y } else { size.1 - y - h },
                        CAPSULE_INSET
                    ));
                    assert!(w <= last.0 + 0.02 && h <= last.1 + 0.02);
                    assert!(rt.hit_regions().iter().all(|[l, t, r, b]| *l >= x - 0.03
                        && *r <= x + w + 0.03
                        && *t >= y - 0.03
                        && *b <= y + h + 0.03));
                    if reduced {
                        assert!(near(h, 44.) || near(h, 36.));
                    }
                    if frame < started_frame {
                        assert!(
                            rt.state.waveform.is_none()
                                && rt.find("elapsed").is_none()
                                && rt.find("meeting-listening-field").is_none()
                        );
                        if frame == 60 {
                            assert!(texts(&rt).contains(&tr("Starting…").to_owned()));
                        }
                    }
                    assert!(rt.find("stop").is_none() && !rt.state.hovered());
                    assert!(commands.try_recv().is_err());
                    last = (w, h);
                }
                assert_eq!(last, (REST_WIDTH, REST_HEIGHT));
                assert!(rt.find("elapsed").is_some());
                rt.pointer_leave();
                rt.frame(0.);
                open_recording(&mut rt);
                assert!(rt.find("stop").is_some());
            }
        }
    }
}

#[test]
fn pending_offers_never_mask_a_failure_or_adopt_another_start() {
    use offer::{Host, Route};
    let id = OfferId::next();
    let other = OfferId::next();
    let failed = ControllerEvent::Failed {
        title: "Fixture".into(),
        error: "Permission denied".into(),
    };
    let mut host = Host::Offered(id);
    assert_eq!(
        host.route(&ControllerEvent::OfferWithdrawn(other)),
        Route::Ordinary
    );
    assert_eq!(
        host.route(&ControllerEvent::OfferWithdrawn(id)),
        Route::Close
    );
    assert_eq!(host.route(&ControllerEvent::Starting(None)), Route::Close);
    assert_eq!(
        host.route(&ControllerEvent::Starting(Some(other))),
        Route::Close
    );
    assert_eq!(
        host.route(&ControllerEvent::Starting(Some(id))),
        Route::Keep
    );
    assert_eq!(host.route(&failed), Route::Close);
    assert_eq!(host.route(&ControllerEvent::Transcribing), Route::Close);
    assert_eq!(Host::Panel.route(&started("manual")), Route::Ordinary);
    let (mut rt, rx, _) = runtime(offer(slack_huddle()), false, false, PANEL);
    drop(rx); // A rejected send is still an offer, not a pending recording.
    tap(&mut rt, "record");
    assert!(!rt.state.presentation.inherits_offer_window());
    assert!(rt.find("record").is_some() && rt.find("elapsed").is_none());
}

