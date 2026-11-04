//! More on `"Setting up Hex"` than a title and a percentage: a bigger HUD,
//! what comes next while the model downloads, or both. Prototypes, each a
//! story variant, until one is picked; the first run shows today's page.

use rune::{El, TextStyle, col, el, icon, row, text};
use rune_kit::glyphs;

use super::super::controls::Paint;
use super::stories::Onboard;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum FirstScreen {
    #[default]
    Today,
    BigHud,
    UpNext,
    Both,
}

const TODAY_HUD: f32 = 1.24;
const BIG_HUD: f32 = 2.2;
const CARD_W: f32 = 196.0;

/// How big the HUD above `"Setting up Hex"` draws.
pub(super) fn hud_scale(s: &Onboard) -> f32 {
    match s.first_screen {
        FirstScreen::BigHud | FirstScreen::Both => BIG_HUD,
        FirstScreen::Today | FirstScreen::UpNext => TODAY_HUD,
    }
}

/// The three steps after the download, quiet under its progress.
pub(super) fn up_next(p: Paint, s: &Onboard) -> Option<El> {
    if !matches!(s.first_screen, FirstScreen::UpNext | FirstScreen::Both) {
        return None;
    }
    let steps = [
        (&glyphs::CURSOR_TEXT, "Watch Hex work", "A quick demo"),
        (&glyphs::MIC, "Allow the microphone", "One macOS prompt"),
        (&glyphs::KEYBOARD, "Try it yourself", "Hold a key and talk"),
    ];
    let cards = steps
        .into_iter()
        .enumerate()
        .map(|(i, (glyph, title, line))| {
            s.motion
                .staggered(card(p, i, glyph, title, line), 3.0 + i as f32, 8.0)
        });
    Some(
        col()
            .key("up-next")
            .gap(10.0)
            .items_center()
            .pt(18.0)
            .child(
                s.motion.staggered(
                    text("While it downloads")
                        .key("up-next-label")
                        .font(TextStyle::new(12.0, 520.0).lh(16.0))
                        .color(p.muted()),
                    2.5,
                    6.0,
                ),
            )
            .child(row().key("up-next-cards").gap(12.0).children(cards)),
    )
}

fn card(p: Paint, i: usize, glyph: &'static rune::Icon, title: &str, line: &str) -> El {
    let k = p.k;
    row()
        .key(format!("next-{i}"))
        .w(CARD_W)
        .gap(10.0)
        .padding(12.0)
        .center_y()
        .radius(12.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(
            el().key("glyph")
                .size(30.0, 30.0)
                .radius(9.0)
                .fill(k.c.bg_base)
                .border(p.hair, k.c.border_weak)
                .center()
                .child(icon(glyph, 15.0).key("icon").color(p.text())),
        )
        .child(
            col()
                .key("words")
                .gap(1.0)
                .shrink()
                .child(
                    text(title.to_string())
                        .key("title")
                        .font(TextStyle::new(13.0, 560.0).lh(18.0))
                        .color(p.text()),
                )
                .child(
                    text(line.to_string())
                        .key("line")
                        .font(TextStyle::new(12.0, 420.0).lh(16.0))
                        .color(p.muted()),
                ),
        )
}
