//! The Paste picker overlay (`labs.paste_picker`, macOS): a focus-free,
//! non-activating Rune overlay listing the newest pasted dictations at the
//! active text caret (falling back to the pointer), plus deterministic `--snap`
//! scenes (`paste-picker`, `paste-picker-multiselect`).

#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

use rune::{Anim, Color, Cx, El, OverlayPlacement, ScrollSpec, col, el, row, text};

use super::Ui;
use super::controls::Paint;
use crate::desktop::i18n::{event_age_at, tr, tr_fill};
use crate::paste_picker::{Anchor, PickerItem, PickerSnapshot};

const CARD_W: f32 = 368.0;
const PAD_X: f32 = 16.0;
const PAD_TOP: f32 = 10.0;
const PAD_BOTTOM: f32 = 22.0;
pub(crate) const OVERLAY_SIZE: (f32, f32) = (CARD_W + 2.0 * PAD_X, 472.0);
const MAX_LIST_H: f32 = OVERLAY_SIZE.1 - PAD_TOP - PAD_BOTTOM - 32.0;
const CARET_GAP: f64 = 6.0;
const SCREEN_MARGIN: f64 = 8.0;

#[derive(Default)]
pub(crate) struct OverlayState {
    snapshot: PickerSnapshot,
    flipped: bool,
}

/// Visible screen frame in Quartz global coordinates (points from the primary
/// screen's top-left, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScreenBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlacedOverlay {
    pub placement: OverlayPlacement,
    pub flipped: bool,
    pub hit_rect: [f64; 4],
}

/// Computes the overlay placement for `anchor` within `screen`, preferring to
/// hang below the caret line and flipping cleanly above it when near the bottom
/// edge (such as bottom-anchored chat composers).
pub(crate) fn place_overlay(
    anchor: Anchor,
    item_count: usize,
    screen: ScreenBounds,
) -> PlacedOverlay {
    let card_w = f64::from(CARD_W);
    let est_h = f64::from(estimated_card_height(item_count));
    let min_x = screen.x + SCREEN_MARGIN;
    let max_x = (screen.x + screen.width - SCREEN_MARGIN - card_w).max(min_x);
    let card_x = anchor.x.clamp(min_x, max_x);

    let below_top = anchor.y + anchor.height + CARET_GAP;
    let above_bottom = anchor.y - CARET_GAP;
    let fits_below = below_top + est_h <= screen.y + screen.height - SCREEN_MARGIN;
    let fits_above = above_bottom - est_h >= screen.y + SCREEN_MARGIN;
    let flipped = !fits_below && fits_above;

    let overlay_h = f64::from(OVERLAY_SIZE.1);
    let (overlay_y, hit_y) = if flipped {
        let bottom = above_bottom.clamp(
            screen.y + SCREEN_MARGIN + est_h,
            screen.y + screen.height - SCREEN_MARGIN,
        );
        (bottom - overlay_h + f64::from(PAD_BOTTOM), bottom - est_h)
    } else {
        let min_y = screen.y + SCREEN_MARGIN;
        let max_y = (screen.y + screen.height - SCREEN_MARGIN - est_h).max(min_y);
        let top = below_top.clamp(min_y, max_y);
        (top - f64::from(PAD_TOP), top)
    };

    PlacedOverlay {
        placement: OverlayPlacement::Rect {
            x: (card_x - f64::from(PAD_X)) as f32,
            y: overlay_y as f32,
            width: OVERLAY_SIZE.0,
            height: OVERLAY_SIZE.1,
        },
        flipped,
        hit_rect: [card_x, hit_y, card_w, est_h],
    }
}

fn estimated_card_height(item_count: usize) -> f32 {
    if item_count == 0 {
        return 76.0;
    }
    let rows_h = (item_count as f32 * 48.0).min(MAX_LIST_H);
    rows_h + 38.0
}

/// Installs the non-activating Paste picker overlay on macOS. Nothing is
/// prewarmed or allocated on GPU until Labs › Paste picker opens it.
#[cfg(target_os = "macos")]
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use rune::{
        DisplayTarget, OverlayInput, OverlayLevel, OverlayOptions, OverlaySpec, ScreenEdge,
    };
    let spec = OverlaySpec {
        name: "Hex paste picker",
        size: OVERLAY_SIZE,
        // Placed for real at the caret each time it opens.
        edge: ScreenEdge::Top,
        margin: 0.0,
        align: rune::Align::Start,
        inset: 0.0,
        interactive: true,
        view: overlay_view,
    }
    .options(OverlayOptions {
        display: DisplayTarget::Primary,
        level: OverlayLevel::AboveOverlays,
        input: OverlayInput::Regions,
        exclude_from_capture: true,
        prewarm: false,
        ..Default::default()
    });
    let overlay = app.overlay(spec, OverlayState::default());
    let shown = Arc::new(AtomicBool::new(false));
    crate::paste_picker::watch(move |snapshot| {
        if !snapshot.open {
            if shown.swap(false, Ordering::AcqRel) {
                let handle = overlay.clone();
                overlay.update(move |state, tx| {
                    state.snapshot = PickerSnapshot::default();
                    state.flipped = false;
                    tx.instant();
                    crate::paste_picker::set_overlay_rect(None);
                    handle.hide();
                });
            }
            return;
        }
        let appearing = !shown.swap(true, Ordering::AcqRel);
        let next = snapshot.clone();
        let handle = overlay.clone();
        overlay.update(move |state, tx| {
            let anchor = next.anchor.unwrap_or(Anchor {
                x: 240.0,
                y: 240.0,
                width: 0.0,
                height: 16.0,
            });
            let placed = place_overlay(anchor, next.items.len(), screen_for_anchor(anchor));
            state.flipped = placed.flipped;
            state.snapshot = next;
            tx.instant();
            crate::paste_picker::set_overlay_rect(Some(placed.hit_rect));
            handle.set_display(DisplayTarget::Primary);
            handle.set_placement(Some(placed.placement));
            if appearing {
                handle.show();
            }
        });
    });
}

#[cfg(target_os = "macos")]
fn screen_for_anchor(anchor: Anchor) -> ScreenBounds {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let fallback = ScreenBounds {
        x: 0.0,
        y: 24.0,
        width: 1512.0,
        height: 958.0,
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return fallback;
    };
    let screens = NSScreen::screens(mtm);
    let Some(primary) = screens.iter().next() else {
        return fallback;
    };
    let primary_top = primary.frame().origin.y + primary.frame().size.height;
    let to_quartz = |rect: objc2_foundation::NSRect| ScreenBounds {
        x: rect.origin.x,
        y: primary_top - (rect.origin.y + rect.size.height),
        width: rect.size.width,
        height: rect.size.height,
    };
    let matched = screens
        .iter()
        .find(|screen| {
            let q = to_quartz(screen.frame());
            anchor.x >= q.x
                && anchor.x < q.x + q.width
                && anchor.y >= q.y
                && anchor.y < q.y + q.height
        })
        .or_else(|| NSScreen::mainScreen(mtm))
        .unwrap_or(primary);
    to_quartz(matched.visibleFrame())
}

#[cfg(target_os = "macos")]
fn shift_held_now() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
    }
    const COMBINED_SESSION_STATE: u32 = 0;
    // SAFETY: queries global modifier flags without side effects.
    (unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) })
        & crate::app_settings::SHIFT_KEY_MASK
        != 0
}

#[cfg(not(target_os = "macos"))]
fn shift_held_now() -> bool {
    false
}

fn overlay_view(state: &OverlayState, cx: &mut Cx<OverlayState>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let mut root = col()
        .key("paste-picker-overlay")
        .w_full()
        .h_full()
        .px(PAD_X)
        .pt(PAD_TOP)
        .pb(PAD_BOTTOM)
        .hover_motion_within(Anim::INSTANT, Anim::INSTANT);
    if !state.snapshot.open {
        return root;
    }
    if state.flipped {
        root = root.child(el().key("flip-spacer").grow(1.0));
    }
    root.child(card_view::<OverlayState>(
        &state.snapshot,
        p,
        |index| crate::paste_picker::click_row(index, shift_held_now()),
        crate::paste_picker::commit_current,
    ))
}

/// Renders the picker card itself, shared by the live non-activating overlay,
/// `--snap` fixture scenes, and offscreen unit tests.
pub(super) fn card_view<S: 'static>(
    snapshot: &PickerSnapshot,
    p: Paint,
    on_row: impl Fn(usize) + Copy + 'static,
    on_commit: impl Fn() + Copy + 'static,
) -> El {
    let c = &p.k.c;
    let body = if snapshot.items.is_empty() {
        col().key("picker-empty").px(14.0).py(18.0).center().child(
            text(tr("No recent dictations"))
                .key("empty-label")
                .font(p.k.ty.ui)
                .color(p.muted()),
        )
    } else {
        let multi = !snapshot.selected.is_empty();
        let mut rows = col().key("picker-rows").padding(4.0).gap(2.0);
        for (index, item) in snapshot.items.iter().enumerate() {
            rows = rows.child(item_row::<S>(snapshot, index, item, multi, p, on_row));
        }
        col()
            .key("picker-viewport")
            .shrink()
            .min_h(0.0)
            .max_h(MAX_LIST_H)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows)
    };

    let footer = footer_bar::<S>(snapshot, p, on_commit);

    let card = col()
        .key("paste-picker-card")
        .w(CARD_W)
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .border(p.hair, c.border_base)
        .clip_soft(0.0)
        .hit()
        .child(body)
        .child(
            el().key("footer-rule")
                .h(p.hair)
                .w_full()
                .fill(c.border_weak),
        )
        .child(footer);
    p.k.elevate(card, &p.k.elevation.floating)
}

fn item_row<S: 'static>(
    snapshot: &PickerSnapshot,
    index: usize,
    item: &PickerItem,
    multi: bool,
    p: Paint,
    on_row: impl Fn(usize) + Copy + 'static,
) -> El {
    let c = &p.k.c;
    let order = snapshot.selection_order(index);
    let chosen = snapshot.is_chosen(index);
    let focused = snapshot.cursor == index;
    let transparent = Color {
        a: 0.0,
        ..c.border_focus
    };
    let border_color = if multi && focused {
        c.border_focus
    } else {
        transparent
    };

    let badge = multi.then(|| match order {
        Some(number) => row()
            .key("order-badge")
            .w(18.0)
            .h(18.0)
            .mt(1.0)
            .center()
            .radius(9.0)
            .fill(p.accent())
            .child(
                text(number.to_string())
                    .key("order-label")
                    .font(p.k.ty.small_medium)
                    .color(c.text_inverse),
            ),
        None => el()
            .key("order-slot")
            .w(18.0)
            .h(18.0)
            .mt(1.0)
            .radius(9.0)
            .border(p.hair, c.border_base),
    });

    let preview = preview_text(&item.text);
    let age = event_age_at(item.timestamp_ms, snapshot.now_ms);

    let mut r = row()
        .key(format!("picker-row-{index}"))
        .role("button")
        .focusable(false)
        .px(10.0)
        .py(7.0)
        .gap(10.0)
        .radius(p.k.radius.md)
        .border(p.hair, border_color)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(move |_: &mut S| on_row(index))
        .children(badge)
        .child(
            col()
                .key("preview-col")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(
                    text(preview)
                        .key("preview")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .wrap()
                        .max_lines(2),
                ),
        )
        .child(row().key("age-col").pt(1.0).child(
            text(age).key("age").font(p.k.ty.small).color(if chosen {
                p.muted()
            } else {
                p.faint()
            }),
        ));
    if chosen {
        r = r.fill(c.layer_02);
    }
    r
}

fn footer_bar<S: 'static>(
    snapshot: &PickerSnapshot,
    p: Paint,
    on_commit: impl Fn() + Copy + 'static,
) -> El {
    let c = &p.k.c;
    let left = if !snapshot.selected.is_empty() {
        let label = tr_fill("Paste {} in order", &snapshot.selected.len().to_string());
        row()
            .key("footer-commit")
            .role("button")
            .focusable(false)
            .px(6.0)
            .py(2.0)
            .ml(-6.0)
            .radius(p.k.radius.sm)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |_: &mut S| on_commit())
            .child(
                text(label)
                    .key("footer-left")
                    .font(p.k.ty.small_medium)
                    .color(p.accent()),
            )
    } else {
        row().key("footer-hint").child(
            text(tr("V / ↑↓ move · ⇧ select"))
                .key("footer-left")
                .font(p.k.ty.small)
                .color(p.faint()),
        )
    };

    row()
        .key("picker-footer")
        .h(28.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .child(left)
        .child(el().key("footer-spacer").grow(1.0))
        .child(
            text(tr("⏎ paste · Esc"))
                .key("footer-right")
                .font(p.k.ty.small)
                .color(p.faint()),
        )
}

/// Collapses embedded newlines into single spaces so multi-paragraph dictations
/// use both lines of the row preview cleanly without altering the pasted text.
fn preview_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for word in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Full-window `--snap` / Playground stage showing a focused editor field with
/// its text caret and the Paste picker anchored right below the caret.
pub(super) fn scene_overlay(ui: &Ui, p: Paint) -> Option<El> {
    let snapshot = ui.paste_picker_preview.as_ref()?;
    let c = &p.k.c;
    let prompt_prefix = "Rebased onto origin/dev and ran targeted checks. ";
    let composer = col()
        .key("scene-composer")
        .w(620.0)
        .px(18.0)
        .py(14.0)
        .gap(6.0)
        .radius(p.k.radius.lg)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(
            row()
                .key("composer-line")
                .center_y()
                .child(
                    text(prompt_prefix)
                        .key("composer-text")
                        .font(p.k.ty.body)
                        .color(p.text()),
                )
                .child(
                    el().key("composer-caret")
                        .w(2.0)
                        .h(18.0)
                        .radius(1.0)
                        .fill(p.accent()),
                ),
        );

    let card = card_view::<Ui>(snapshot, p, |_| {}, || {});

    Some(
        col()
            .key("paste-picker-scene")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .fill(c.bg_deep)
            .pt(96.0)
            .pl(180.0)
            .gap(6.0)
            .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
            .child(composer)
            .child(row().key("picker-anchor-offset").pl(352.0).child(card)),
    )
}

const FIXTURE_NOW_MS: u64 = 1_793_400_000_000;

fn fixture_items() -> Vec<PickerItem> {
    [
        (
            8,
            14_000,
            "Open a draft PR against dev once the targeted check passes, and leave the installed app untouched.",
        ),
        (
            7,
            95_000,
            "Place the list at the active text caret via Accessibility, falling back to the pointer when an app exposes no range.",
        ),
        (
            6,
            240_000,
            "Swallow navigation keys in the session tap so V, arrows, Return, and Escape never leak into the focused editor.",
        ),
        (
            5,
            540_000,
            "Join multi-selected dictations in selection order with the same sentence spacing as consecutive pastes.",
        ),
        (
            4,
            1_320_000,
            "Keep the Labs switch off by default so tapping Paste last dictation behaves identically until opted in.",
        ),
        (
            3,
            3_900_000,
            "Check light and dark renders with headless --snap scenes.",
        ),
    ]
    .into_iter()
    .map(|(id, age_ms, text)| PickerItem {
        id,
        text: text.into(),
        timestamp_ms: FIXTURE_NOW_MS - age_ms,
    })
    .collect()
}

pub(super) fn fixture_single(ui: &mut Ui) {
    ui.paste_picker_preview = Some(PickerSnapshot {
        open: true,
        items: fixture_items(),
        cursor: 1,
        selected: Vec::new(),
        now_ms: FIXTURE_NOW_MS,
        anchor: None,
    });
}

pub(super) fn fixture_multiselect(ui: &mut Ui) {
    ui.paste_picker_preview = Some(PickerSnapshot {
        open: true,
        items: fixture_items(),
        cursor: 0,
        selected: vec![3, 1, 0],
        now_ms: FIXTURE_NOW_MS,
        anchor: None,
    });
}

#[cfg(test)]
mod tests {
    use rune::Runtime;
    use rune::text::TextSystem;

    use super::*;

    fn runtime(snapshot: PickerSnapshot, flipped: bool, dark: bool) -> Runtime<OverlayState> {
        let state = OverlayState { snapshot, flipped };
        let mut rt = crate::rune_ui::offscreen::runtime(
            state,
            overlay_view,
            OVERLAY_SIZE,
            rune::Env {
                system_dark: dark,
                scale: 2.0,
            },
            TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        rt.settle_all();
        rt.frame(0.0);
        rt
    }

    fn rect(rt: &Runtime<OverlayState>, key: &str) -> Option<[f32; 4]> {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .map(|e| e.rect)
    }

    #[test]
    fn card_renders_two_line_previews_and_multiselect_badges_within_overlay() {
        let snapshot = PickerSnapshot {
            open: true,
            items: fixture_items(),
            cursor: 0,
            selected: vec![2, 0],
            now_ms: FIXTURE_NOW_MS,
            anchor: None,
        };
        for dark in [false, true] {
            let rt = runtime(snapshot.clone(), false, dark);
            let [x, y, w, h] = rect(&rt, "paste-picker-card").expect("card renders");
            assert_eq!(w, CARD_W);
            assert!(x >= 0.0 && x + w <= OVERLAY_SIZE.0);
            assert!(
                y >= 0.0 && y + h <= OVERLAY_SIZE.1,
                "card fits overlay: {y} + {h}"
            );
            assert!(rect(&rt, "picker-row-0").is_some());
            assert!(rect(&rt, "picker-row-5").is_some());
            assert!(rect(&rt, "footer-commit").is_some());
        }
    }

    #[test]
    fn empty_state_renders_cleanly_and_flipped_card_anchors_to_bottom() {
        let empty = PickerSnapshot {
            open: true,
            items: Vec::new(),
            cursor: 0,
            selected: Vec::new(),
            now_ms: FIXTURE_NOW_MS,
            anchor: None,
        };
        let rt = runtime(empty, true, true);
        let [_, y, _, h] = rect(&rt, "paste-picker-card").expect("empty card renders");
        assert!(rect(&rt, "empty-label").is_some());
        assert!(
            (y + h - (OVERLAY_SIZE.1 - PAD_BOTTOM)).abs() < 1.5,
            "flipped card hugs bottom edge above caret: y={y} h={h}"
        );
    }

    #[test]
    fn placement_hangs_below_caret_and_flips_above_near_bottom_edge() {
        let screen = ScreenBounds {
            x: 0.0,
            y: 25.0,
            width: 1512.0,
            height: 957.0,
        };
        let mid = place_overlay(
            Anchor {
                x: 400.0,
                y: 300.0,
                width: 1.0,
                height: 18.0,
            },
            6,
            screen,
        );
        assert!(!mid.flipped);
        assert_eq!(
            mid.placement,
            OverlayPlacement::Rect {
                x: 400.0 - PAD_X,
                y: (300.0 + 18.0 + CARET_GAP as f32) - PAD_TOP,
                width: OVERLAY_SIZE.0,
                height: OVERLAY_SIZE.1,
            }
        );

        let bottom_chat = place_overlay(
            Anchor {
                x: 1450.0,
                y: 920.0,
                width: 1.0,
                height: 18.0,
            },
            6,
            screen,
        );
        assert!(
            bottom_chat.flipped,
            "flips above bottom-anchored chat input"
        );
        if let OverlayPlacement::Rect { x, .. } = bottom_chat.placement {
            assert!(
                x + PAD_X + CARD_W <= (screen.width - SCREEN_MARGIN) as f32 + 0.5,
                "clamped inside right screen edge: x={x}"
            );
        } else {
            panic!("expected Rect placement");
        }
    }
}
