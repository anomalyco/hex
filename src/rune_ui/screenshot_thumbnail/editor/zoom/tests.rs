//! The zoom between a card and the editor, frame by frame, with the stack
//! and the editor driven together as the two overlays are: every handoff
//! (card to editor, editor back to card) lands on the same pixels.

use std::path::PathBuf;
use std::sync::Arc;

use rune::Tx;

use super::super::super::tests::LENT;
use super::super::super::{Cards, HOME, Lent, Shot as CardShot, home, poll as cards_poll};
use super::super::{Editor, Shot, poll as editor_poll};
use super::*;

/// The display the scene plays on, in points.
const SCREEN: [f32; 2] = [960.0, 600.0];
/// Its pixels per point.
const SCALE: f32 = 2.0;
/// The area shot, on the display.
const AREA: [f32; 4] = [190.0, 120.0, 420.0, 280.0];

/// One frame of the scene: where the card and the editor's shot are.
#[derive(Clone, Copy, Debug)]
struct Row {
    t: f64,
    card: Option<[f32; 4]>,
    lift: f32,
    lent: Lent,
    /// The editor's flying shot, while it shows.
    flying: Option<[f32; 4]>,
    chrome: bool,
}

/// A desktop of \`w\` × \`h\` pixels: a dusk wallpaper and two windows, one
/// over the other.
fn backdrop(w: u32, h: u32) -> Vec<u8> {
    let s = w as f32 / SCREEN[0];
    let window = |rect: [f32; 4], x: f32, y: f32| {
        let [rx, ry, rw, rh] = rect.map(|v| v * s);
        x >= rx && x < rx + rw && y >= ry && y < ry + rh
    };
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for py in 0..h {
        for px in 0..w {
            let (x, y) = (px as f32, py as f32);
            let (u, v) = (x / w as f32, y / h as f32);
            let back = [210.0, 120.0, 520.0, 360.0];
            let front = [
                AREA[0] + 10.0,
                AREA[1] + 10.0,
                AREA[2] - 20.0,
                AREA[3] - 20.0,
            ];
            let pixel = if window(front, x, y) {
                let [fx, fy, fw, _] = front.map(|v| v * s);
                let (lx, ly) = (x - fx, y - fy);
                if ly < 28.0 * s {
                    let dot = |cx: f32, c: [u8; 3]| {
                        { ((lx - cx * s).powi(2) + (ly - 14.0 * s).powi(2)).sqrt() < 5.0 * s }
                            .then_some(c)
                    };
                    dot(16.0, [255, 95, 87])
                        .or(dot(32.0, [254, 188, 46]).filter(|_| true))
                        .or(dot(48.0, [40, 200, 64]))
                        .map_or([236, 236, 240, 255], |c| [c[0], c[1], c[2], 255])
                } else {
                    let line = ((ly - 28.0 * s) / (18.0 * s)) as u32;
                    let within = (ly - 28.0 * s) % (18.0 * s) > 6.0 * s;
                    let len = fw * (0.35 + 0.5 * (((line * 37) % 11) as f32 / 11.0));
                    if within && lx > 20.0 * s && lx < len && line < 11 {
                        if line % 4 == 0 {
                            [60, 110, 230, 255]
                        } else {
                            [190, 192, 200, 255]
                        }
                    } else {
                        [252, 252, 253, 255]
                    }
                }
            } else if window(back, x, y) {
                let [_, by, ..] = back.map(|v| v * s);
                if y - by < 28.0 * s {
                    [52, 54, 62, 255]
                } else {
                    [32, 34, 40, 255]
                }
            } else {
                let r = 40.0 + 120.0 * u + 30.0 * v;
                let g = 40.0 + 40.0 * v;
                let b = 110.0 + 90.0 * (1.0 - u) * (1.0 - v * 0.5);
                [r as u8, g as u8, b as u8, 255]
            };
            rgba.extend(pixel);
        }
    }
    rgba
}

/// The pixels of \`rect\` (points) out of a backdrop \`w\` pixels wide.
fn crop(rgba: &[u8], w: u32, rect: [f32; 4]) -> (u32, u32, Vec<u8>) {
    let s = w as f32 / SCREEN[0];
    let [x, y, cw, ch] = rect.map(|v| (v * s).round() as u32);
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y..y + ch {
        let at = ((row * w + x) * 4) as usize;
        out.extend_from_slice(&rgba[at..at + (cw * 4) as usize]);
    }
    (cw, ch, out)
}

/// `rgba` at half its size, as a card's small picture.
fn half(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (w, h) = (width / 2, height / 2);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let at = (((y * 2) * width + x * 2) * 4) as usize;
            out.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    (w, h, out)
}

fn image(w: u32, h: u32, rgba: Vec<u8>) -> rune::Image {
    let image = rune::ImageCache::new(Arc::new(|| {})).rgba(w, h, rgba);
    image.wait().unwrap();
    while image.pixels().size() == (0, 0) {
        std::thread::yield_now();
    }
    image
}

/// The area shot, as the stack and the editor each hold it.
#[derive(Clone)]
struct Area {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
}

impl Area {
    fn take() -> (Self, Vec<u8>) {
        let (w, h) = ((SCREEN[0] * SCALE) as u32, (SCREEN[1] * SCALE) as u32);
        let desk = backdrop(w, h);
        let (width, height, rgba) = crop(&desk, w, AREA);
        (
            Self {
                width,
                height,
                rgba: Arc::new(rgba),
            },
            desk,
        )
    }

    fn card(&self) -> (CardShot, rune::Image) {
        let (w, h, small) = half(self.width, self.height, &self.rgba);
        let shot = CardShot {
            path: PathBuf::from("/tmp/Hex Screenshot.png"),
            image: image(w, h, small),
            size: [self.width, self.height],
            recording: None,
        };
        (shot, image(self.width, self.height, (*self.rgba).clone()))
    }

    fn editor(&self) -> Shot {
        Shot {
            image: image(self.width, self.height, (*self.rgba).clone()),
            rgba: self.rgba.clone(),
            size: [self.width, self.height],
            scale: SCALE,
        }
    }
}

/// The two overlays, as plain states or each in its runtime.
trait Sides {
    fn cards(&mut self, f: impl FnOnce(&mut Cards));
    fn editor(&mut self, f: impl FnOnce(&mut Editor));
}

/// The two overlays' states, stepped as their polls step them.
struct Scene {
    cards: Cards,
    editor: Editor,
}

impl Sides for Scene {
    fn cards(&mut self, f: impl FnOnce(&mut Cards)) {
        f(&mut self.cards);
    }
    fn editor(&mut self, f: impl FnOnce(&mut Editor)) {
        f(&mut self.editor);
    }
}

fn new_cards() -> Cards {
    Cards {
        screen: (SCREEN[0], SCREEN[1]),
        ..Cards::default()
    }
}

fn new_editor() -> Editor {
    Editor {
        screen: SCREEN,
        on: false,
        ..Editor::default()
    }
}

impl Scene {
    fn new() -> Self {
        Self {
            cards: new_cards(),
            editor: new_editor(),
        }
    }
}

/// The stack and the editor at animation time `t`, driven as the app
/// drives them.
struct Driver<S: Sides> {
    sides: S,
    area: Arc<Area>,
    t: f64,
}

impl<S: Sides> Driver<S> {
    fn new(sides: S) -> Self {
        Self {
            sides,
            area: Arc::new(Area::take().0),
            t: 0.0,
        }
    }

    fn shoot(&mut self) -> u64 {
        let (shot, sharp) = self.area.card();
        let mut id = 0;
        self.sides
            .cards(|cards| id = cards.push(shot, Some(AREA), Some(sharp)));
        id
    }

    /// A click on card `id`: as [`Cards::open`], with the editor opened
    /// here rather than on its thread.
    fn open(&mut self, id: u64) {
        let mut from = None;
        self.sides.cards(|cards| from = cards.lend_out(id));
        let shot = self.area.editor();
        self.sides.editor(|editor| {
            editor.load(shot, SCREEN);
            editor.zoom = from.map(Zoom::new);
        });
    }

    fn close(&mut self) {
        self.sides.editor(|editor| editor.close(&mut Tx::default()));
    }

    fn discard(&mut self, id: u64) {
        self.sides
            .cards(|cards| cards.discard(id, &mut Tx::default()));
    }

    /// One frame `dt` seconds on: both polls, then what the editor lent
    /// or gave back reaching the stack.
    fn step(&mut self, dt: f64) -> Row {
        self.t += dt;
        let t = self.t;
        self.sides
            .cards(|cards| _ = cards_poll(cards, t, &mut Tx::default()));
        self.sides
            .editor(|editor| _ = editor_poll(editor, t, &mut Tx::default()));
        for (id, lent) in LENT.with(|lent| std::mem::take(&mut *lent.borrow_mut())) {
            self.sides
                .cards(|cards| cards.lend(id, lent, &mut Tx::default()));
        }
        self.row()
    }

    fn row(&mut self) -> Row {
        let t = self.t;
        let mut row = Row {
            t,
            card: None,
            lift: 0.0,
            lent: Lent::No,
            flying: None,
            chrome: false,
        };
        self.sides.cards(|cards| {
            if let Some(card) = cards.cards.first() {
                row.card = Some(card.flight.rect());
                row.lift = card.flight.lift();
                row.lent = card.lent;
            }
        });
        self.sides.editor(|editor| {
            let zoom = editor.zoom.as_ref();
            row.flying = zoom
                .filter(|zoom| zoom.flies(t) && editor.picture().is_some())
                .map(|zoom| zoom.flight.rect());
            row.chrome = match zoom {
                Some(zoom) => zoom.way == Way::In && zoom.arrived,
                None => editor.on,
            };
        });
        row
    }

    fn card(&mut self, id: u64) -> Option<([f32; 4], Lent)> {
        let mut found = None;
        self.sides.cards(|cards| {
            found = cards.get(id).map(|card| (card.flight.rect(), card.lent));
        });
        found
    }
}

/// When things happen in the scene, in seconds.
const SHOOT_AT: f64 = 0.3;
const OPEN_AT: f64 = 1.7;
const CLOSE_AT: f64 = 3.5;
const END_AT: f64 = 4.6;

/// The whole scene at `hz` frames a second: `each` sees every frame.
fn play<S: Sides>(
    driver: &mut Driver<S>,
    hz: u32,
    mut each: impl FnMut(&mut Driver<S>, usize, Row),
) {
    let dt = 1.0 / f64::from(hz);
    let frames = (END_AT * f64::from(hz)).round() as usize;
    let mut id = 0;
    for frame in 0..=frames {
        let t = frame as f64 * dt;
        let at = |when: f64| t < when + dt * 0.5 && t + dt * 0.5 >= when;
        if at(SHOOT_AT) {
            id = driver.shoot();
        }
        if at(OPEN_AT) {
            driver.open(id);
        }
        if at(CLOSE_AT) {
            driver.close();
        }
        let row = driver.step(if frame == 0 { 0.0 } else { dt });
        each(driver, frame, row);
    }
}

fn near(a: [f32; 4], b: [f32; 4], within: f32) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= within)
}

/// Settles a fresh driver's first card in its slot, then opens it.
fn opened() -> (Driver<Scene>, u64, [f32; 4]) {
    let mut driver = Driver::new(Scene::new());
    let id = driver.shoot();
    for _ in 0..120 {
        driver.step(1.0 / 60.0);
    }
    let (card, _) = driver.card(id).unwrap();
    driver.open(id);
    (driver, id, card)
}

#[test]
fn the_shot_never_jumps_between_the_card_and_the_editor() {
    let mut rows = Vec::new();
    play(&mut Driver::new(Scene::new()), 120, |_, _, row| {
        rows.push(row)
    });
    // The card's first frame is the area itself, flat on the display.
    let first = rows.iter().find(|row| row.card.is_some()).unwrap();
    assert_eq!(first.card, Some(AREA));
    assert_eq!(first.lift, 0.0);
    // Whatever shows the shot (the card, or the editor's flying copy), it
    // moves smoothly: no frame moves it more than a fast flight does.
    let shown = |row: &Row| match (row.lent, row.flying) {
        (_, Some(rect)) => Some(rect),
        (Lent::Away, None) => None,
        (_, None) => row.card,
    };
    for pair in rows.windows(2) {
        let (Some(a), Some(b)) = (shown(&pair[0]), shown(&pair[1])) else {
            continue;
        };
        let step = a
            .iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(
            step < 40.0,
            "a {step} point jump at {:.3}s: {a:?} to {b:?}",
            pair[1].t
        );
    }
    // The card goes only once the editor draws the shot exactly where it
    // was, and comes back only once the shot is exactly on it again.
    let away = rows.iter().position(|row| row.lent == Lent::Away).unwrap();
    let card = rows[away - 1].card.unwrap();
    let flying = rows[away].flying.unwrap();
    assert!(near(flying, card, 0.01), "{flying:?} vs {card:?}");
    let back = away
        + rows[away..]
            .iter()
            .position(|row| row.lent == Lent::No)
            .unwrap();
    let landed = rows[back]
        .flying
        .expect("the flying shot covers the card as it shows");
    assert!(near(landed, rows[back].card.unwrap(), 0.01));
    // Then the editor goes, a moment later.
    assert!(rows.last().unwrap().flying.is_none());
    assert_eq!(rows.last().unwrap().lent, Lent::No);
    // The panel came in only once the shot had landed in it.
    let chrome = rows.iter().position(|row| row.chrome).unwrap();
    assert!(chrome > away);
    assert!(HOME.lock().unwrap().is_none());
}

#[test]
fn a_close_mid_zoom_turns_straight_back() {
    let (mut driver, id, card) = opened();
    let mut last = None;
    for _ in 0..12 {
        last = driver.step(1.0 / 60.0).flying;
    }
    let before = last.unwrap();
    driver.close();
    let after = driver.step(1.0 / 60.0).flying.unwrap();
    assert!(near(before, after, 40.0), "{before:?} to {after:?}");
    let mut landed = false;
    for _ in 0..120 {
        let row = driver.step(1.0 / 60.0);
        landed |= row.lent == Lent::No && row.flying.is_some_and(|f| near(f, card, 0.01));
    }
    assert!(landed, "it landed back on its card");
    assert_eq!(driver.card(id).map(|(_, lent)| lent), Some(Lent::No));
    assert!(driver.sides.editor.zoom.is_none());
}

#[test]
fn a_shot_whose_card_is_gone_fades_where_it_is() {
    let (mut driver, id, _) = opened();
    for _ in 0..90 {
        driver.step(1.0 / 60.0);
    }
    driver.discard(id);
    driver.close();
    driver.step(1.0 / 60.0);
    assert!(
        driver
            .sides
            .editor
            .zoom
            .as_ref()
            .is_some_and(|zoom| zoom.gone)
    );
    assert_eq!(home(id), None);
}

#[test]
fn a_new_shot_mid_zoom_moves_the_card_the_shot_flies_back_to() {
    let (mut driver, id, _) = opened();
    for _ in 0..60 {
        driver.step(1.0 / 60.0);
    }
    driver.shoot();
    driver.step(1.0 / 60.0);
    let moved = driver.sides.cards.slots()[0].1;
    assert_eq!(
        home(id),
        Some(moved),
        "the card made room above the new one"
    );
    driver.close();
    let mut landed = None;
    for _ in 0..120 {
        let row = driver.step(1.0 / 60.0);
        if landed.is_none() && driver.card(id).is_some_and(|(_, lent)| lent == Lent::No) {
            landed = row.flying;
        }
    }
    let landed = landed.expect("the flying shot covers the card as it shows");
    assert!(near(landed, moved, 0.01), "{landed:?} vs {moved:?}");
}

/// The two overlays, each in its offscreen runtime.
struct Rendered {
    cards: rune::Runtime<Cards>,
    editor: rune::Runtime<Editor>,
}

impl Sides for Rendered {
    fn cards(&mut self, f: impl FnOnce(&mut Cards)) {
        self.cards.update(|cards, tx| {
            f(cards);
            tx.animated();
        });
    }
    fn editor(&mut self, f: impl FnOnce(&mut Editor)) {
        self.editor.update(|editor, tx| {
            f(editor);
            tx.animated();
        });
    }
}

/// `HEX_SCREENSHOT_MOTION=<folder>` writes the scene (an area shot flying
/// into its card, the card zooming up into the editor, and the editor
/// closing back into it) over a desktop, dark, at 2×, encoded by
/// `ffmpeg`: `<folder>/1x/clip.mp4` at 60 frames a second and
/// `<folder>/quarter/clip.mp4` at 240 played at 60, with a per-frame
/// `trace.csv` beside each and stills of the 1× one. Without the variable
/// or a GPU adapter it does nothing.
#[test]
fn zoom_frames() {
    let Some(folder) = std::env::var_os("HEX_SCREENSHOT_MOTION") else {
        return;
    };
    let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
        return;
    };
    for (name, hz) in [("1x", 60), ("quarter", 240)] {
        render(&gpu, &PathBuf::from(&folder).join(name), hz);
    }
}

fn render(gpu: &rune::render::Gpu, out: &std::path::Path, hz: u32) {
    use crate::rune_ui::offscreen;
    std::fs::create_dir_all(out).unwrap();
    let (w, h) = ((SCREEN[0] * SCALE) as u32, (SCREEN[1] * SCALE) as u32);
    let (_, desk) = Area::take();
    let env = rune::Env {
        system_dark: true,
        scale: SCALE,
    };
    let text =
        || rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let size = (SCREEN[0], SCREEN[1]);
    let mut rendered = Rendered {
        cards: offscreen::runtime(
            new_cards(),
            super::super::super::card::view,
            size,
            env,
            text(),
        ),
        editor: offscreen::runtime(new_editor(), super::super::view::view, size, env, text()),
    };
    rendered.cards.transparent = true;
    rendered.editor.transparent = true;
    let mut driver = Driver::new(rendered);
    let mut cards_paint = offscreen::renderer(&gpu.device);
    let mut editor_paint = offscreen::renderer(&gpu.device);
    let target = offscreen::target(&gpu.device, w, h);
    let mut trace = String::from(
        "frame,t,card_x,card_y,card_w,card_h,lift,lent,fly_x,fly_y,fly_w,fly_h,chrome\n",
    );
    let dt = 1.0 / f64::from(hz);
    // Encoded as it renders: a clip's raw frames would be gigabytes.
    let mut ffmpeg = std::process::Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
        ])
        .arg(format!("{w}x{h}"))
        .args([
            "-r", "60", "-i", "-", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16",
        ])
        .arg(out.join("clip.mp4"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("ffmpeg on the PATH");
    let mut stdin = ffmpeg.stdin.take().unwrap();
    // Stills at the moments worth a look, at 60 frames a second only.
    let stills = [0.32, 0.55, 1.1, 1.75, 1.95, 2.6, 3.65, 4.2];
    play(&mut driver, hz, |driver, frame, row| {
        let step = if frame == 0 { 0.0 } else { dt };
        let sides = &mut driver.sides;
        sides.cards.frame(step);
        sides.editor.frame(step);
        let cards = offscreen::paint(gpu, &mut cards_paint, &target, &mut sides.cards);
        let editor = offscreen::paint(gpu, &mut editor_paint, &target, &mut sides.editor);
        let mut pixels = desk.clone();
        over(&mut pixels, &cards);
        over(&mut pixels, &editor);
        use std::io::Write;
        stdin.write_all(&pixels).unwrap();
        if hz == 60 && stills.iter().any(|t| (row.t - t).abs() < dt * 0.5) {
            let name = format!("still-{:.2}s.png", row.t);
            rune::render::write_png(&out.join(name), w, h, &pixels).unwrap();
        }
        let rect = |r: Option<[f32; 4]>| {
            r.map_or(",,,".into(), |[x, y, w, h]| {
                format!("{x:.2},{y:.2},{w:.2},{h:.2}")
            })
        };
        trace.push_str(&format!(
            "{frame},{:.4},{},{:.3},{:?},{},{}\n",
            row.t,
            rect(row.card),
            row.lift,
            row.lent,
            rect(row.flying),
            row.chrome
        ));
    });
    drop(stdin);
    assert!(ffmpeg.wait().unwrap().success());
    std::fs::write(out.join("trace.csv"), trace).unwrap();
}

/// `top`, premultiplied, over `under`.
fn over(under: &mut [u8], top: &[u8]) {
    for (u, t) in under.chunks_exact_mut(4).zip(top.chunks_exact(4)) {
        let keep = 255 - u32::from(t[3]);
        for i in 0..3 {
            u[i] = (u32::from(t[i]) + (u32::from(u[i]) * keep + 127) / 255).min(255) as u8;
        }
    }
}
