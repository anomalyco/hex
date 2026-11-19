//! A requester's icon, wherever Hex names one: in the permission window and
//! the Connected apps pane. A native app's icon draws as itself (it carries
//! its own shape); a website's favicon sits on the neutral tile, as a browser
//! shows a tab's; without one, the tile shows a glyph for an app or a site.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use rune::{El, Icon, el};
use rune_theme_opencode::icons::MONITOR;

use super::super::controls::Paint;
use super::super::icons::{GLOBE, tile};
use crate::local_api::{ConsentIcon, Recipient};

/// How many decoded icons are kept before the memo starts over.
const MAX_ICONS: usize = 64;

/// Decodes requester icons off the view's thread and remembers them.
#[derive(Default)]
pub(crate) struct Icons {
    cache: Option<rune::ImageCache>,
    images: RefCell<HashMap<(usize, u32), rune::Image>>,
}

impl Icons {
    /// The window's cache, whose worker wakes it when an icon is ready.
    pub(crate) fn set_cache(&mut self, cache: rune::ImageCache) {
        self.cache = Some(cache);
    }

    /// `icon`'s pixels once decoded. Without a window's cache (a fixture, a
    /// snapshot), it decodes at once and waits, which a small icon allows.
    fn pixels(&self, icon: &ConsentIcon) -> Option<rune::Pixels> {
        let key = (icon.pixels().as_ptr() as usize, icon.size());
        let mut images = self.images.borrow_mut();
        if !images.contains_key(&key) {
            if images.len() >= MAX_ICONS {
                images.clear();
            }
            let image = match &self.cache {
                Some(cache) => cache.rgba(icon.size(), icon.size(), icon.pixels().to_vec()),
                None => {
                    let cache = rune::ImageCache::new(Arc::new(|| {}));
                    let image = cache.rgba(icon.size(), icon.size(), icon.pixels().to_vec());
                    let _ = image.wait();
                    while image.error().is_none() && image.pixels().size() == (0, 0) {
                        std::thread::yield_now();
                    }
                    image
                }
            };
            images.insert(key, image);
        }
        let image = &images[&key];
        (image.is_ready() && image.error().is_none()).then(|| image.pixels().clone())
    }
}

/// `recipient`'s mark, `size` points square: its icon, else a glyph.
pub(in crate::rune_ui) fn mark(p: Paint, icons: &Icons, recipient: &Recipient, size: f32) -> El {
    let glyph: &'static Icon = match recipient {
        Recipient::Native { .. } => &MONITOR,
        Recipient::Web { .. } => &GLOBE,
    };
    let Some(pixels) = recipient.icon().and_then(|icon| icons.pixels(icon)) else {
        return tile(p, glyph, size);
    };
    match recipient {
        Recipient::Native { .. } => el()
            .key("tile")
            .size(size, size)
            .pixels(&pixels)
            .a11y_hidden(),
        Recipient::Web { .. } => {
            // The neutral tile `tile` draws, with the favicon for its glyph.
            let favicon = (size * 0.56).round();
            el().key("tile")
                .size(size, size)
                .center()
                .radius(if size > 40.0 {
                    p.k.radius.lg
                } else {
                    p.k.radius.md
                })
                .fill(p.k.c.layer_02)
                .border(p.hair, p.k.c.border_weak)
                .a11y_hidden()
                .child(
                    el().key("favicon")
                        .size(favicon, favicon)
                        .clip_rounded(p.k.radius.xs)
                        .pixels(&pixels),
                )
        }
    }
}

/// A drawn app icon and favicon for scenes and snapshots, so they never read
/// this computer's apps: an amber notes app, and a blue page mark.
pub(crate) mod fixtures {
    use super::ConsentIcon;

    pub(crate) fn app() -> ConsentIcon {
        draw(128, |x, y| {
            // A macOS-style squircle, 10% inset, with a warm top-down gradient
            // and a white page with three ruled lines.
            let (u, v) = (x * 2.0 - 1.0, y * 2.0 - 1.0);
            let inside = (u.abs() / 0.8).powi(5) + (v.abs() / 0.8).powi(5) <= 1.0;
            if !inside {
                return [0, 0, 0, 0];
            }
            let page = (0.30..0.70).contains(&x) && (0.24..0.78).contains(&y);
            if page {
                let ruled = [0.40, 0.51, 0.62]
                    .iter()
                    .any(|line| (y - line).abs() < 0.018 && (0.36..0.64).contains(&x));
                return if ruled {
                    [214, 160, 74, 255]
                } else {
                    [255, 252, 245, 255]
                };
            }
            let t = y;
            [
                (250.0 - 18.0 * t) as u8,
                (190.0 - 70.0 * t) as u8,
                (60.0 - 30.0 * t) as u8,
                255,
            ]
        })
    }

    pub(crate) fn favicon() -> ConsentIcon {
        draw(32, |x, y| {
            let (u, v) = (x * 2.0 - 1.0, y * 2.0 - 1.0);
            if (u.abs() / 0.94).powi(4) + (v.abs() / 0.94).powi(4) > 1.0 {
                return [0, 0, 0, 0];
            }
            // A white "N" made of three strokes on blue.
            let stroke = 0.11;
            let left = (x - 0.32).abs() < stroke / 2.0;
            let right = (x - 0.68).abs() < stroke / 2.0;
            let diagonal = ((x - 0.32) / 0.36 - (y - 0.26) / 0.48).abs() < 0.16;
            let band = (0.26..0.74).contains(&y);
            if band && (left || right || (diagonal && (0.32..0.68).contains(&x))) {
                [255, 255, 255, 255]
            } else {
                [37, 99, 235, 255]
            }
        })
    }

    /// `side` pixels square of `colour(x, y)` at 4×4 samples per pixel,
    /// straight alpha.
    fn draw(side: u32, colour: impl Fn(f32, f32) -> [u8; 4]) -> ConsentIcon {
        let mut pixels = Vec::with_capacity((side * side * 4) as usize);
        for py in 0..side {
            for px in 0..side {
                let mut sum = [0u32; 4];
                for sy in 0..4 {
                    for sx in 0..4 {
                        let x = (px as f32 + (sx as f32 + 0.5) / 4.0) / side as f32;
                        let y = (py as f32 + (sy as f32 + 0.5) / 4.0) / side as f32;
                        let c = colour(x, y);
                        let a = u32::from(c[3]);
                        for i in 0..3 {
                            sum[i] += u32::from(c[i]) * a;
                        }
                        sum[3] += a;
                    }
                }
                let alpha = sum[3] / 16;
                let straight = |i: usize| sum[i].checked_div(sum[3]).unwrap_or(0) as u8;
                pixels.extend([straight(0), straight(1), straight(2), alpha as u8]);
            }
        }
        ConsentIcon::new(side, pixels.into()).expect("a drawn icon fits")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_fixture_icons_decode() {
        let icons = Icons::default();
        for icon in [fixtures::app(), fixtures::favicon()] {
            assert!(icons.pixels(&icon).is_some(), "{}", icon.size());
        }
    }
}
