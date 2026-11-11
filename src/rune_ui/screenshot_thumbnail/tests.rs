use std::path::PathBuf;

use rune::Tx;

use super::*;

fn shot(size: [u32; 2]) -> Shot {
    let images = rune::ImageCache::new(std::sync::Arc::new(|| {}));
    let image = images.rgba(
        size[0],
        size[1],
        vec![128; (size[0] * size[1] * 4) as usize],
    );
    image.wait().unwrap();
    Shot {
        path: PathBuf::from("/tmp/Hex Screenshot.png"),
        image,
        size,
        recording: None,
    }
}

/// A desktop-ish shot: a light window on a backdrop of `hue` (0 blue, 1
/// green, 2 violet).
fn desktop(size: [u32; 2], hue: u8) -> Shot {
    let [w, h] = size;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let window = x > w / 6 && x < w * 5 / 6 && y > h / 5 && y < h * 4 / 5;
            let bar = window && y < h / 5 + h / 14;
            let (a, b) = ((60 * x / w) as u8, (50 * y / h) as u8);
            rgba.extend(match (window, bar) {
                (true, true) => [222, 222, 226, 255],
                (true, false) => [250, 250, 250, 255],
                _ => match hue {
                    1 => [40 + a, 150 + b, 110, 255],
                    2 => [120 + a, 80 + b, 190, 255],
                    _ => [40 + a, 90 + b, 180, 255],
                },
            });
        }
    }
    let images = rune::ImageCache::new(std::sync::Arc::new(|| {}));
    let image = images.rgba(w, h, rgba);
    image.wait().unwrap();
    while image.pixels().size() == (0, 0) {
        std::thread::yield_now();
    }
    Shot {
        path: PathBuf::from("/tmp/Hex Screenshot.png"),
        image,
        size,
        recording: None,
    }
}

fn recording(mut shot: Shot) -> Shot {
    shot.recording = Some(Recording {
        duration_ms: 12_400,
        words: Vec::new(),
    });
    shot
}

/// Three cards on a display tall enough for them: an older window shot, a
/// recording, and the newest shot in the corner.
fn stack() -> Cards {
    Cards::still(
        vec![
            desktop([1200, 900], 1),
            recording(desktop([1440, 900], 2)),
            desktop([1440, 900], 0),
        ],
        STACK_SCREEN,
        None,
    )
}

/// The display the stack's golden rests on.
const STACK_SCREEN: (f32, f32) = (420.0, 560.0);

fn render(gpu: &rune::render::Gpu, cards: Cards, dark: bool) -> (u32, u32, Vec<u8>) {
    let screen = cards.screen;
    let mut rt = crate::rune_ui::offscreen::runtime(
        cards,
        card::view,
        screen,
        rune::Env {
            system_dark: dark,
            scale: 2.0,
        },
        rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
    rt.settle_all();
    rt.frame(0.0);
    let (w, h) = ((screen.0 * 2.0) as u32, (screen.1 * 2.0) as u32);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (w, h),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, rt.clear_color());
    target.copy(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
    (w, h, target.read(&gpu.device))
}

/// A card at rest, under the pointer, holding a recording's length, and
/// three stacked, against `tests/golden/screenshot-thumbnail-*.png`;
/// `HEX_UPDATE_GOLDENS=1` rewrites them and `HEX_GOLDEN_RENDERS` also
/// writes them there. Without a GPU adapter it skips. Checked on Linux
/// only, like every golden.
#[test]
#[cfg_attr(not(target_os = "linux"), ignore = "goldens are checked on Linux")]
fn thumbnail_goldens() {
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        eprintln!("skipping screenshot thumbnail golden: no GPU adapter");
        return;
    };
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut misses = Vec::new();
    for name in ["rest", "hover", "recording", "stack"] {
        for dark in [false, true] {
            let theme = if dark { "dark" } else { "light" };
            let name_theme = format!("screenshot-thumbnail-{name}-{theme}");
            let cards = match name {
                "rest" => Cards::one(desktop([1440, 900], 0), false),
                "hover" => Cards::one(desktop([1440, 900], 0), true),
                "recording" => Cards::one(recording(desktop([1440, 900], 0)), false),
                _ => stack(),
            };
            let (w, h, pixels) = render(&gpu, cards, dark);
            let name = name_theme;
            if let Some(renders) = std::env::var_os("HEX_GOLDEN_RENDERS") {
                let renders = std::path::PathBuf::from(renders);
                std::fs::create_dir_all(&renders).unwrap();
                rune::render::write_png(&renders.join(format!("{name}.png")), w, h, &pixels)
                    .unwrap();
            }
            let path = folder.join(format!("{name}.png"));
            if std::env::var_os("HEX_UPDATE_GOLDENS").is_some() {
                rune::render::write_png(&path, w, h, &pixels).unwrap();
                continue;
            }
            let actual = std::env::temp_dir().join(format!("rune-settings-{name}-actual.png"));
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
                rune::render::write_png(&actual, w, h, &pixels).unwrap();
                misses.push(format!("{name}: differs; actual at {}", actual.display()));
            }
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

/// `HEX_SCREENSHOT_FRAMES=<folder>` writes, at 60 fps and dark, a card
/// growing in from a window shot's rectangle and leaving (`thumbnail/`),
/// and three captures landing one after another, the older cards making
/// room, then the middle one closing (`stack/`). Without the variable or a
/// GPU adapter it does nothing.
#[test]
fn thumbnail_frames() {
    let Some(folder) = std::env::var_os("HEX_SCREENSHOT_FRAMES") else {
        return;
    };
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    // A window shot from the middle of the display: the card grows in from
    // that rectangle.
    frames(
        &gpu,
        &folder.join("thumbnail"),
        PREVIEW,
        150,
        |index, rt| match index {
            12 => rt.update(|s: &mut Cards, tx| {
                s.push(desktop([1440, 900], 0), Some([120.0, 90.0, 260.0, 170.0]));
                tx.animated();
            }),
            110 => rt.update(|s: &mut Cards, tx| {
                let id = s.cards[0].id;
                s.close(id, tx);
            }),
            _ => {}
        },
    );
    frames(
        &gpu,
        &folder.join("stack"),
        STACK_SCREEN,
        240,
        |index, rt| match index {
            10 => rt.update(|s: &mut Cards, tx| {
                s.push(desktop([1200, 900], 1), Some([40.0, 60.0, 200.0, 150.0]));
                tx.animated();
            }),
            60 => rt.update(|s: &mut Cards, tx| {
                s.push(recording(desktop([1440, 900], 2)), None);
                tx.animated();
            }),
            110 => rt.update(|s: &mut Cards, tx| {
                s.push(desktop([1440, 900], 0), Some([60.0, 200.0, 300.0, 190.0]));
                tx.animated();
            }),
            190 => rt.update(|s: &mut Cards, tx| {
                let id = s.cards[1].id;
                s.close(id, tx);
            }),
            _ => {}
        },
    );
}

/// `count` frames of an empty stack on a `screen`-sized display, after
/// `step` changes it at each frame's index, to `out`.
fn frames(
    gpu: &rune::render::Gpu,
    out: &std::path::Path,
    screen: (f32, f32),
    count: usize,
    step: impl Fn(usize, &mut rune::Runtime<Cards>),
) {
    std::fs::create_dir_all(out).unwrap();
    let cards = Cards {
        screen,
        ..Cards::default()
    };
    let mut rt = crate::rune_ui::offscreen::runtime(
        cards,
        card::view,
        screen,
        rune::Env {
            system_dark: true,
            scale: 2.0,
        },
        rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
    rt.frame(0.0);
    let (w, h) = ((screen.0 * 2.0) as u32, (screen.1 * 2.0) as u32);
    let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
    let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
    for index in 0..=count {
        step(index, &mut rt);
        rt.frame(1.0 / 60.0);
        let pixels = crate::rune_ui::offscreen::paint(gpu, &mut renderer, &target, &mut rt);
        rune::render::write_png(&out.join(format!("{index:04}.png")), w, h, &pixels).unwrap();
    }
}

#[test]
fn its_corners_nest_in_the_displays_and_match_the_capsule() {
    assert_eq!(RADIUS, SCREEN_RADIUS - INSET);
    assert_eq!(RADIUS, super::super::capture_capsule::surface::SHELL_RADIUS);
    assert_eq!(CLOSE_RADIUS + CLOSE_INSET, RADIUS);
}

#[test]
fn the_card_keeps_the_shots_shape() {
    assert_eq!(card_size(&shot([1600, 400])), (240.0, 60.0));
    assert_eq!(card_size(&shot([300, 900])), (56.0, 168.0));
}

#[test]
fn a_card_stays_until_its_closed() {
    let mut cards = Cards::one(shot([800, 600]), false);
    let mut tx = Tx::default();
    poll(&mut cards, 0.0, &mut tx);
    poll(&mut cards, 3_600.0, &mut tx);
    assert_eq!(cards.cards.len(), 1);
    let id = cards.cards[0].id;
    cards.close(id, &mut tx);
    assert!(cards.cards.is_empty());
}

#[test]
fn the_newest_rests_in_the_corner_and_the_older_ones_above_it() {
    let mut cards = Cards::still(Vec::new(), (800.0, 600.0), None);
    let old = cards.push(shot([1440, 900]), None);
    let new = cards.push(shot([1440, 900]), None);
    let slots = cards.slots();
    let [x, y, w, h] = slots[1].1;
    assert_eq!(slots[1].0, new);
    assert_eq!((x + w, y + h), (800.0 - INSET, 600.0 - INSET));
    assert_eq!(slots[0].0, old);
    assert_eq!(slots[0].1[1] + slots[0].1[3], y - GAP);

    // Closing the newest brings the older one down into the corner.
    cards.close(new, &mut Tx::default());
    assert_eq!(cards.slots()[0].1, [x, y, w, h]);
}

#[test]
fn cards_of_any_shape_share_the_corners_right_edge() {
    let cards = stack();
    let slots = cards.slots();
    let right = STACK_SCREEN.0 - INSET;
    for (_, [x, _, w, _]) in &slots {
        assert_eq!(x + w, right);
    }
    // Each keeps its shot's shape inside the same largest size: a 4:3 shot
    // is narrower than a 16:10 one.
    assert_eq!(slots[0].1[2..], [224.0, 168.0]);
    assert_eq!(slots[2].1[2..], [240.0, 150.0]);
}

#[test]
fn the_oldest_cards_close_once_the_stack_reaches_the_top() {
    let mut cards = Cards::still(Vec::new(), (800.0, 400.0), None);
    let ids: Vec<u64> = (0..4)
        .map(|_| cards.push(shot([1440, 900]), None))
        .collect();
    let kept: Vec<u64> = cards.cards.iter().map(|card| card.id).collect();
    assert_eq!(kept, ids[2..]);
}

#[test]
fn a_key_goes_to_the_card_under_the_pointer_only() {
    let mut cards = Cards::still(
        vec![shot([800, 600]), shot([800, 600])],
        (800.0, 600.0),
        None,
    );
    let mut tx = Tx::default();
    let (older, newer) = (cards.cards[0].id, cards.cards[1].id);
    cards.hover(Some(older));
    assert!(hovered());
    let hovered_id = cards.hovered.unwrap();
    cards.act(hovered_id, CardKey::Close, &mut tx);
    assert_eq!(cards.cards.len(), 1);
    assert_eq!(cards.cards[0].id, newer);
    // The card under the pointer went, so the keys stop.
    assert!(cards.hovered.is_none() && !hovered());
}

#[test]
fn restore_brings_back_the_card_closed_last_but_never_a_trashed_one() {
    let file = std::env::temp_dir().join(format!("hex-card-{}.png", std::process::id()));
    std::fs::write(&file, b"png").unwrap();
    let on_disk = || {
        let mut shot = shot([800, 600]);
        shot.path = file.clone();
        shot
    };
    let mut cards = Cards::still(vec![on_disk(), on_disk()], (800.0, 600.0), None);
    let mut tx = Tx::default();
    let (first, second) = (cards.cards[0].id, cards.cards[1].id);
    assert!(!cards.restore());
    cards.close(first, &mut tx);
    cards.close(second, &mut tx);
    assert!(restorable());
    assert!(cards.restore());
    assert_eq!(cards.cards.len(), 1);
    assert!(cards.restore());
    assert_eq!(cards.cards.len(), 2);
    assert!(!cards.restore() && !restorable());

    // A trashed card is gone for good, and one whose file went is skipped.
    let id = cards.cards[1].id;
    cards.discard(id, &mut tx);
    assert!(!restorable());
    let id = cards.cards[0].id;
    cards.close(id, &mut tx);
    std::fs::remove_file(&file).unwrap();
    assert!(!cards.restore());
}
