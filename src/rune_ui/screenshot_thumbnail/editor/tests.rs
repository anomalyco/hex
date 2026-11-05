use super::*;

pub(super) fn shot(size: [u32; 2], scale: f32) -> Shot {
    let [w, h] = size;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let window = x > w / 6 && x < w * 5 / 6 && y > h / 5 && y < h * 4 / 5;
            let bar = window && y < h / 5 + h / 14;
            rgba.extend(match (window, bar) {
                (true, true) => [222, 222, 226, 255],
                (true, false) => [250, 250, 250, 255],
                _ => [40 + (60 * x / w) as u8, 90 + (50 * y / h) as u8, 180, 255],
            });
        }
    }
    let rgba = Arc::new(rgba);
    let image = rune::ImageCache::new(Arc::new(|| {})).rgba(w, h, (*rgba).clone());
    image.wait().unwrap();
    while image.pixels().size() == (0, 0) {
        std::thread::yield_now();
    }
    Shot {
        image,
        rgba,
        size,
        scale,
    }
}

#[test]
fn the_shot_fits_the_display_and_never_grows_past_its_points() {
    let editor = Editor::still(shot([1440, 900], 2.0), [1470.0, 956.0]);
    let layout = editor.layout();
    assert_eq!(layout.fit, 0.5, "720 × 450 points fit");
    assert_eq!(layout.image[2..], [720.0, 450.0]);
    let [x, y, w, h] = layout.panel;
    assert_eq!(w, 736.0);
    assert_eq!(h, PAD + CHIP_H + GAP + 450.0 + PAD);
    assert_eq!(
        [x, y],
        [
            ((1470.0 - w) / 2.0f32).round(),
            ((956.0 - h) / 2.0f32).round()
        ]
    );

    let big = Editor::still(shot([4000, 3000], 2.0), [1470.0, 956.0]);
    let [.., w, h] = big.layout().panel;
    assert!(
        w <= 1470.0 - 2.0 * MARGIN && h <= 956.0 - 2.0 * MARGIN,
        "{w} × {h}"
    );

    let small = Editor::still(shot([200, 100], 2.0), [1470.0, 956.0]);
    let layout = small.layout();
    assert_eq!(layout.panel[2], MIN_W, "the toolbar always fits");
    assert_eq!(
        layout.image[0],
        ((MIN_W - 100.0) / 2.0).round(),
        "the shot centres"
    );
}

#[test]
fn a_moved_panel_stays_on_the_display() {
    assert_eq!(
        clamp_panel([-40.0, 900.0], [600.0, 400.0], [1470.0, 956.0]),
        [0.0, 556.0]
    );
    assert_eq!(
        clamp_panel([10.0, 10.0], [2000.0, 400.0], [1470.0, 956.0])[0],
        0.0
    );
}

#[test]
fn drags_on_the_shot_draw_each_tool() {
    let mut editor = Editor::still(shot([800, 600], 2.0), [1470.0, 956.0]);
    editor.draw(Phase::Down, [10.0, 10.0]);
    editor.draw(Phase::Move, [40.0, 30.0]);
    editor.draw(Phase::Up, [40.0, 30.0]);
    assert!(matches!(editor.marks.as_slice(), [Mark::Pen(points)] if points.len() == 2));

    editor.pick(Tool::Arrow);
    editor.draw(Phase::Down, [100.0, 100.0]);
    editor.draw(Phase::Move, [102.0, 101.0]);
    editor.draw(Phase::Up, [102.0, 101.0]);
    assert_eq!(editor.marks.len(), 1, "a click draws no arrow");
    editor.draw(Phase::Down, [100.0, 100.0]);
    editor.draw(Phase::Move, [900.0, 300.0]);
    editor.draw(Phase::Up, [900.0, 300.0]);
    assert_eq!(
        editor.marks.last(),
        Some(&Mark::Arrow {
            from: [100.0, 100.0],
            to: [800.0, 300.0]
        }),
        "a drag past the shot's edge stops at it"
    );

    editor.pick(Tool::Crop);
    editor.draw(Phase::Down, [50.0, 60.0]);
    editor.draw(Phase::Move, [450.0, 360.0]);
    assert!(editor.cropping.is_some());
    editor.draw(Phase::Up, [450.0, 360.0]);
    assert_eq!(editor.crop, Some([50.0, 60.0, 400.0, 300.0]));
    editor.draw(Phase::Down, [70.0, 70.0]);
    editor.draw(Phase::Up, [70.0, 70.0]);
    assert_eq!(editor.crop, None, "a click clears the crop");
}

#[test]
fn ink_lands_under_the_pointer_in_the_frame_it_is_drawn() {
    let mut editor = Editor::still(shot([1440, 900], 2.0), [960.0, 640.0]);
    editor.tool = Tool::Box;
    let mut rt = crate::rune_ui::offscreen::runtime(
        editor,
        view::view,
        (960.0, 640.0),
        rune::Env {
            system_dark: true,
            scale: 2.0,
        },
        rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
    rt.frame(0.0);
    let draft = |rt: &rune::Runtime<Editor>| {
        rt.elements()
            .iter()
            .find(|e| e.key.as_deref() == Some("draft"))
            .map(|e| e.rect)
    };
    for (phase, at) in [
        (Phase::Down, [300.0, 260.0]),
        (Phase::Move, [500.0, 400.0]),
        (Phase::Move, [760.0, 520.0]),
    ] {
        rt.update(move |s: &mut Editor, tx| {
            s.draw(phase, at);
            tx.animated();
        });
        rt.frame(1.0 / 60.0);
        let drawn = draft(&rt).expect("the box being drawn");
        rt.settle_all();
        rt.frame(0.0);
        assert_eq!(
            Some(drawn),
            draft(&rt),
            "no spring between {at:?} and the ink"
        );
    }
}

#[test]
fn text_is_typed_where_it_was_clicked_and_escape_ends_it() {
    let mut editor = Editor::still(shot([800, 600], 2.0), [1470.0, 956.0]);
    let mut tx = Tx::default();
    editor.pick(Tool::Text);
    editor.draw(Phase::Down, [100.0, 200.0]);
    editor.draw(Phase::Up, [100.0, 200.0]);
    for typed in ["H", "i", "!"] {
        assert!(on_key(&mut editor, &key(Key::Text(typed.into())), &mut tx));
    }
    on_key(&mut editor, &key(Key::Backspace), &mut tx);
    assert_eq!(editor.typing.as_ref().map(|(_, t)| t.as_str()), Some("Hi"));
    on_key(&mut editor, &key(Key::Escape), &mut tx);
    assert!(editor.on, "the first Escape ends the text");
    assert!(matches!(editor.marks.as_slice(), [Mark::Text { text, .. }] if text == "Hi"));
    on_key(&mut editor, &key(Key::Escape), &mut tx);
    assert!(!editor.on, "the next closes the editor");
}

#[test]
fn undo_takes_back_the_last_mark_then_the_crop() {
    let mut editor = Editor::still(shot([800, 600], 2.0), [1470.0, 956.0]);
    let mut tx = Tx::default();
    editor.crop = Some([0.0, 0.0, 100.0, 100.0]);
    editor.marks.push(Mark::Box {
        from: [0.0, 0.0],
        to: [50.0, 50.0],
    });
    let undo = KeyEvent {
        key: Key::Command('z'),
        mods: rune::Mods {
            cmd: true,
            ..Default::default()
        },
    };
    on_key(&mut editor, &undo, &mut tx);
    assert!(editor.marks.is_empty());
    assert!(editor.crop.is_some());
    on_key(&mut editor, &undo, &mut tx);
    assert!(editor.crop.is_none());
}

#[test]
fn the_send_list_searches_titles_first() {
    let dest = |name: &str, folder: &str, session: &str| Dest {
        session: session.into(),
        name: name.into(),
        folder: Some(folder.into()),
    };
    let dests = vec![
        dest("Fix the picker", "hex", "ses_a"),
        dest("Hex editor", "rune", "ses_b"),
        dest("Seer cards", "seer", "ses_c"),
    ];
    let names = |query| -> Vec<String> {
        matching(&dests, query)
            .into_iter()
            .map(|d| d.name.clone())
            .collect()
    };
    assert_eq!(names("hex"), ["Hex editor", "Fix the picker"]);
    assert_eq!(names("ses_c"), ["Seer cards"]);
    assert_eq!(names("").len(), 3);
}

#[test]
fn an_export_without_marks_is_the_crop_of_the_shot() {
    let shot = shot([64, 48], 2.0);
    let export = Export {
        image: shot.image.clone(),
        rgba: shot.rgba.clone(),
        size: shot.size,
        scale: 2.0,
        marks: Vec::new(),
        crop: Some([8.0, 4.0, 32.0, 20.0]),
    };
    let png = export.png().unwrap();
    let path = std::env::temp_dir().join("hex-editor-export-test.png");
    std::fs::write(&path, &png).unwrap();
    let back = export::decode(&path).unwrap();
    assert_eq!(back.size, [32, 20]);
    assert_eq!(
        &back.rgba[..4],
        &shot.rgba[(4 * 64 + 8) * 4..(4 * 64 + 8) * 4 + 4]
    );
    #[cfg(target_os = "macos")]
    assert_eq!(back.scale, 2.0, "it keeps the shot's pixels per point");
}

/// The marks are drawn over the shot at its pixels, and the rest of the
/// shot comes through as it was. Without a GPU adapter it skips.
#[test]
fn an_export_draws_the_marks_over_the_shot() {
    if rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).is_err() {
        eprintln!("skipping the export render: no GPU adapter");
        return;
    }
    let shot = shot([400, 300], 2.0);
    let export = Export {
        image: shot.image.clone(),
        rgba: shot.rgba.clone(),
        size: shot.size,
        scale: 2.0,
        marks: vec![Mark::Box {
            from: [100.0, 100.0],
            to: [300.0, 200.0],
        }],
        crop: Some([50.0, 50.0, 300.0, 200.0]),
    };
    let rgba = export
        .render(marks::pixel_rect(export.crop, export.size))
        .unwrap();
    assert_eq!(rgba.len(), 300 * 200 * 4);
    let at = |x: usize, y: usize| &rgba[(y * 300 + x) * 4..(y * 300 + x) * 4 + 4];
    let shot_at = |x: usize, y: usize| &shot.rgba[(y * 400 + x) * 4..(y * 400 + x) * 4 + 4];
    // The box's top edge, at crop (150, 50) = shot (200, 100).
    let red = at(150, 50);
    assert!(red[0] > 180 && red[1] < 90, "{red:?}");
    for (x, y) in [(5, 5), (290, 190), (150, 100)] {
        let (a, b) = (at(x, y), shot_at(x + 50, y + 50));
        assert!(
            a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 2),
            "({x}, {y}): {a:?} vs {b:?}"
        );
    }
}

fn key(key: Key) -> KeyEvent {
    KeyEvent {
        key,
        mods: rune::Mods::default(),
    }
}

/// The editor over a 960 × 640 display: a 1440 × 900 shot at 2× with an
/// arrow, a box, text, and a crop, then with the Send to… list open,
/// against `tests/golden/screenshot-editor-*.png`. `HEX_UPDATE_GOLDENS=1`
/// rewrites them and `HEX_GOLDEN_RENDERS` also writes them there. Without a
/// GPU adapter it skips. Checked on Linux only, like every golden.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "goldens are checked on Linux")]
fn editor_goldens() {
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping screenshot editor golden: no GPU adapter");
        return;
    };
    const SCREEN: [f32; 2] = [960.0, 640.0];
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut misses = Vec::new();
    for (name, send) in [("marks", false), ("send", true)] {
        for dark in [false, true] {
            let theme = if dark { "dark" } else { "light" };
            let name = format!("screenshot-editor-{name}-{theme}");
            let mut editor = Editor::still(shot([1440, 900], 2.0), SCREEN);
            editor.tool = Tool::Arrow;
            editor.marks = vec![
                Mark::Box {
                    from: [300.0, 260.0],
                    to: [760.0, 520.0],
                },
                Mark::Arrow {
                    from: [1080.0, 770.0],
                    to: [790.0, 540.0],
                },
                Mark::Text {
                    at: [1096.0, 750.0],
                    text: "Too tight".into(),
                },
            ];
            editor.crop = Some([160.0, 120.0, 1180.0, 720.0]);
            if send {
                editor.send = Some(SendList {
                    query: "hex".into(),
                    sessions: Sessions::Ready(vec![
                        Dest {
                            session: "ses_a".into(),
                            name: "Hex editor".into(),
                            folder: Some("hex".into()),
                        },
                        Dest {
                            session: "ses_b".into(),
                            name: "Fix the picker crosshair".into(),
                            folder: Some("hex".into()),
                        },
                    ]),
                    selected: 0,
                });
            }
            let mut rt = crate::rune_ui::offscreen::runtime(
                editor,
                view::view,
                (SCREEN[0], SCREEN[1]),
                rune::Env {
                    system_dark: dark,
                    scale: 2.0,
                },
                rune::text::TextSystem::new(
                    rune_theme_opencode::FONTS,
                    rune_theme_opencode::FAMILIES,
                ),
            );
            let (w, h) = ((SCREEN[0] * 2.0) as u32, (SCREEN[1] * 2.0) as u32);
            let pixels = crate::rune_ui::offscreen::draw(&gpu, &mut rt, w, h);
            if let Some(renders) = std::env::var_os("HEX_GOLDEN_RENDERS") {
                let renders = PathBuf::from(renders);
                std::fs::create_dir_all(&renders).unwrap();
                rune::render::write_png(&renders.join(format!("{name}.png")), w, h, &pixels)
                    .unwrap();
            }
            let path = folder.join(format!("{name}.png"));
            if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
                rune::render::write_png(&path, w, h, &pixels).unwrap();
                continue;
            }
            let golden = path.exists().then(|| {
                let file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
                let mut reader = png::Decoder::new(file).read_info().unwrap();
                let mut golden = vec![0; reader.output_buffer_size().unwrap()];
                let info = reader.next_frame(&mut golden).unwrap();
                ((info.width, info.height), golden)
            });
            let differing = match &golden {
                Some((size, golden)) if *size == (w, h) => pixels
                    .chunks(4)
                    .zip(golden.chunks(4))
                    .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 12))
                    .count(),
                _ => usize::MAX,
            };
            if differing > (w * h) as usize / 500 {
                let actual = std::env::temp_dir().join(format!("rune-settings-{name}-actual.png"));
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!("{name}: differs; actual at {}", actual.display()));
            }
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

/// `HEX_SCREENSHOT_FRAMES=<folder>` writes the editor opening over a 960 ×
/// 640 display, a box and an arrow drawn, then closing, at 60 fps, dark, to
/// `<folder>/editor/`. Without the variable or a GPU adapter it does
/// nothing.
#[test]
fn editor_frames() {
    let Some(folder) = std::env::var_os("HEX_SCREENSHOT_FRAMES") else {
        return;
    };
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        return;
    };
    const SCREEN: [f32; 2] = [960.0, 640.0];
    let out = PathBuf::from(folder).join("editor");
    std::fs::create_dir_all(&out).unwrap();
    let mut editor = Editor::still(shot([1440, 900], 2.0), SCREEN);
    editor.on = false;
    editor.tool = Tool::Box;
    let mut rt = crate::rune_ui::offscreen::runtime(
        editor,
        view::view,
        (SCREEN[0], SCREEN[1]),
        rune::Env {
            system_dark: true,
            scale: 2.0,
        },
        rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
    rt.frame(0.0);
    let (w, h) = ((SCREEN[0] * 2.0) as u32, (SCREEN[1] * 2.0) as u32);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let lerp = |a: [f32; 2], b: [f32; 2], t: f32| {
        let t = ease(t.clamp(0.0, 1.0));
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    };
    let drag = |rt: &mut rune::Runtime<Editor>, phase: Phase, at: [f32; 2]| {
        rt.update(move |s: &mut Editor, tx| {
            s.draw(phase, at);
            tx.animated();
        });
    };
    let (box_from, box_to) = ([300.0, 260.0], [760.0, 520.0]);
    let (arrow_from, arrow_to) = ([1100.0, 770.0], [790.0, 540.0]);
    for index in 0..=220usize {
        match index {
            10 => rt.update(|s: &mut Editor, tx| {
                s.on = true;
                tx.animated();
            }),
            50 => drag(&mut rt, Phase::Down, box_from),
            51..=80 => drag(
                &mut rt,
                Phase::Move,
                lerp(box_from, box_to, (index - 50) as f32 / 30.0),
            ),
            81 => drag(&mut rt, Phase::Up, box_to),
            100 => rt.update(|s: &mut Editor, tx| {
                s.pick(Tool::Arrow);
                tx.animated();
            }),
            115 => drag(&mut rt, Phase::Down, arrow_from),
            116..=140 => drag(
                &mut rt,
                Phase::Move,
                lerp(arrow_from, arrow_to, (index - 115) as f32 / 25.0),
            ),
            141 => drag(&mut rt, Phase::Up, arrow_to),
            190 => rt.update(|s: &mut Editor, tx| s.close(tx)),
            _ => {}
        }
        rt.frame(1.0 / 60.0);
        let pixels = crate::rune_ui::offscreen::paint(&gpu, &mut renderer, &target, &mut rt);
        rune::render::write_png(&out.join(format!("{index:04}.png")), w, h, &pixels).unwrap();
    }
}
