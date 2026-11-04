//! The screenshot just taken, as a card in the bottom-right corner of the
//! display under the pointer, as macOS and CleanShot show theirs: a click
//! opens it in Preview, a drag carries the file out, and ✕ closes it. It
//! goes by itself a few seconds on, unless the pointer is on it.

use std::path::PathBuf;
use std::time::Duration;

use rune::{Align, Anim, Cursor, Cx, El, OverlayHandle, OverlaySpec, ScreenEdge, Spring, Tx, el};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::controls::Paint;
use crate::desktop::i18n::tr;

/// The overlay: room for the largest card and its ✕.
const SIZE: (f32, f32) = (CARD_MAX.0 + 2.0 * PAD, CARD_MAX.1 + 2.0 * PAD);
const CARD_MAX: (f32, f32) = (240.0, 168.0);
/// Around the card, for its shadow and the ✕ that overhangs its corner.
const PAD: f32 = 16.0;
const MARGIN: f32 = 8.0;
const RADIUS: f32 = 10.0;
/// How long it stays once the pointer isn't on it.
const LINGER: Duration = Duration::from_secs(5);
const FADE: Spring = Spring::new(0.22, 0.0);
/// The fade's length, before the overlay hides.
const FADE_S: f64 = 0.3;

static CLOSE: &rune::Icon = &rune_theme_opencode::icons::CLOSE;

/// The shot the card shows.
pub(crate) struct Shot {
    pub(crate) path: PathBuf,
    pub(crate) image: rune::Image,
    /// Its size in pixels.
    pub(crate) size: [u32; 2],
}

#[derive(Default)]
pub(crate) struct Card {
    shot: Option<Shot>,
    on: bool,
    hovered: bool,
    /// Animation seconds it last appeared, or the pointer last left it.
    since: Option<f64>,
    /// Animation seconds it began to fade.
    leaving: Option<f64>,
    now: f64,
    overlay: Option<OverlayHandle<Card>>,
}

impl Card {
    /// `shot` showing, as a still.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn still(shot: Shot, hovered: bool) -> Self {
        Self {
            shot: Some(shot),
            on: true,
            hovered,
            ..Self::default()
        }
    }

    fn close(&mut self, tx: &mut Tx) {
        if self.on {
            self.on = false;
            self.leaving = Some(self.now);
            tx.animated();
            tx.wake_after(Duration::from_secs_f64(FADE_S));
        }
    }

    fn open(&mut self, tx: &mut Tx) {
        #[cfg(target_os = "macos")]
        if let Some(shot) = &self.shot {
            let opened = std::process::Command::new("/usr/bin/open")
                .args(["-a", "Preview"])
                .arg(&shot.path)
                .spawn();
            if let Err(error) = opened {
                tracing::warn!(%error, "could not open the screenshot in Preview");
            }
        }
        self.close(tx);
    }

    /// The card's size: the shot fitted into [`CARD_MAX`].
    fn card_size(&self) -> (f32, f32) {
        let Some([w, h]) = self.shot.as_ref().map(|shot| shot.size) else {
            return CARD_MAX;
        };
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let fit = (CARD_MAX.0 / w).min(CARD_MAX.1 / h);
        ((w * fit).round().max(48.0), (h * fit).round().max(32.0))
    }
}

fn poll(s: &mut Card, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    if let Some(at) = s.leaving {
        if now - at >= FADE_S {
            s.leaving = None;
            s.shot = None;
            if let Some(overlay) = &s.overlay {
                overlay.hide();
            }
            return true;
        }
        tx.wake_after(Duration::from_secs_f64(at + FADE_S - now));
        return false;
    }
    if !s.on || s.hovered {
        return false;
    }
    let since = *s.since.get_or_insert(now);
    let left = since + LINGER.as_secs_f64() - now;
    if left <= 0.0 {
        s.close(tx);
        return true;
    }
    tx.wake_after(Duration::from_secs_f64(left));
    false
}

/// The thumbnail overlay, as the screenshot thread holds it.
#[derive(Clone)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct Thumbnail {
    overlay: OverlayHandle<Card>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Thumbnail {
    /// Adds the overlay, hidden, made the first time it shows.
    pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> Self {
        let spec = OverlaySpec {
            name: "Hex screenshot",
            size: SIZE,
            edge: ScreenEdge::Bottom,
            margin: MARGIN,
            align: Align::End,
            inset: MARGIN,
            interactive: true,
            view,
        }
        .options(rune::OverlayOptions {
            display: rune::DisplayTarget::UnderPointer,
            exclude_from_capture: true,
            input: rune::OverlayInput::Regions,
            ..Default::default()
        })
        .animate_reveal()
        .poll(poll);
        let overlay = app.overlay(spec, Card::default());
        let handle = overlay.clone();
        overlay.update(move |s, _| s.overlay = Some(handle));
        Self { overlay }
    }

    /// Shows the shot at `path`, from `rgba` (`width` × `height` pixels,
    /// already small), in place of any before it. Safe from any thread;
    /// waits for the decode.
    pub(crate) fn show(&self, path: PathBuf, width: u32, height: u32, rgba: Vec<u8>) {
        let images = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        let image = images.rgba(width, height, rgba);
        if image.wait().is_err() {
            return;
        }
        let handle = self.overlay.clone();
        self.overlay.update(move |s, tx| {
            s.shot = Some(Shot {
                path,
                image,
                size: [width, height],
            });
            s.on = true;
            s.hovered = false;
            s.since = None;
            s.leaving = None;
            handle.show();
            tx.animated();
            tx.wake_after(Duration::ZERO);
        });
    }
}

pub(crate) fn view(s: &Card, cx: &mut Cx<Card>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let k = p.k;
    let root = el()
        .key("screenshot-thumbnail")
        .size(SIZE.0, SIZE.1)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT);
    let Some(shot) = &s.shot else {
        return root;
    };
    let (w, h) = s.card_size();
    let (x, y) = (SIZE.0 - PAD - w, SIZE.1 - PAD - h);
    let mut card = el()
        .key("card")
        .absolute(PAD, PAD)
        .size(w, h)
        .radius(RADIUS)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.floating {
        card = card.box_shadow(*shadow);
    }
    let card = card
        .pixels_fit(shot.image.pixels(), rune::SurfaceFit::Fit)
        .cursor(Cursor::Pointer)
        .on_press_tx(|s: &mut Card, tx| s.open(tx))
        .drag_out(|s: &Card| {
            s.shot
                .as_ref()
                .map(|shot| rune::DragItem::new().file(shot.path.clone()))
        })
        .child(
            el().absolute(0.0, 0.0)
                .size(w, h)
                .radius(RADIUS)
                .border(p.hair, k.c.border_weak),
        );
    let close = IconButton::new(CLOSE)
        .variant(IconButtonVariant::Neutral)
        .control_size(ControlSize::Small)
        .label(tr("Close"))
        .on_press_tx(|s: &mut Card, tx| s.close(tx))
        .build(k)
        .key("close")
        .absolute(PAD - 10.0, PAD - 10.0)
        .opacity(if s.hovered { 1.0 } else { 0.0 })
        .opacity_motion(FADE);
    let group = el()
        .key("shot")
        .absolute(x - PAD, y - PAD)
        .size(w + 2.0 * PAD, h + 2.0 * PAD)
        .hit()
        .on_hover_change(|s: &mut Card, on, tx| {
            s.hovered = on;
            if !on {
                s.since = Some(s.now);
                tx.wake_after(Duration::ZERO);
            }
            tx.animated();
        })
        .opacity(if s.on { 1.0 } else { 0.0 })
        .opacity_motion(FADE)
        .child(card)
        .child(close);
    root.child(group)
}

#[cfg(test)]
mod tests {
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
        }
    }

    /// A desktop-ish shot: a light window on a blue backdrop.
    fn desktop(size: [u32; 2]) -> Shot {
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
        }
    }

    fn render(gpu: &rune::render::Gpu, card: Card, dark: bool) -> (u32, u32, Vec<u8>) {
        let mut rt = crate::rune_ui::offscreen::runtime(
            card,
            view,
            SIZE,
            rune::Env {
                system_dark: dark,
                scale: 2.0,
            },
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        rt.settle_all();
        rt.frame(0.0);
        let (w, h) = ((SIZE.0 * 2.0) as u32, (SIZE.1 * 2.0) as u32);
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

    /// The card at rest and under the pointer against
    /// `tests/golden/screenshot-thumbnail-*.png`; `HEX_UPDATE_GOLDENS=1`
    /// rewrites them and `HEX_GOLDEN_RENDERS` also writes them there.
    /// Without a GPU adapter it skips. Checked on Linux only, like every
    /// golden.
    #[test]
    #[cfg_attr(not(target_os = "linux"), ignore = "goldens are checked on Linux")]
    fn thumbnail_goldens() {
        let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
            eprintln!("skipping screenshot thumbnail golden: no GPU adapter");
            return;
        };
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        let mut misses = Vec::new();
        for (name, hovered) in [("rest", false), ("hover", true)] {
            for dark in [false, true] {
                let theme = if dark { "dark" } else { "light" };
                let name = format!("screenshot-thumbnail-{name}-{theme}");
                let card = Card::still(desktop([1440, 900]), hovered);
                let (w, h, pixels) = render(&gpu, card, dark);
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

    #[test]
    fn the_card_keeps_the_shots_shape() {
        let wide = Card::still(shot([1600, 400]), false);
        assert_eq!(wide.card_size(), (240.0, 60.0));
        let tall = Card::still(shot([300, 900]), false);
        assert_eq!(tall.card_size(), (56.0, 168.0));
    }

    #[test]
    fn it_goes_by_itself_unless_the_pointer_is_on_it() {
        let mut card = Card::still(shot([800, 600]), false);
        let mut tx = Tx::default();
        poll(&mut card, 0.0, &mut tx);
        poll(&mut card, 4.9, &mut tx);
        assert!(card.on);
        poll(&mut card, 5.0, &mut tx);
        assert!(!card.on);
        poll(&mut card, 5.01 + FADE_S, &mut tx);
        assert!(card.shot.is_none());

        let mut card = Card::still(shot([800, 600]), true);
        poll(&mut card, 0.0, &mut tx);
        poll(&mut card, 60.0, &mut tx);
        assert!(card.on);
    }
}
