//! Permissions step for `"Prototypes/Onboarding/First run"`:
//!
//! - One `440 pt` column shared by Hex's setup card, System Settings, and the
//!   floating drag guide, with every app icon aligned on one vertical spine
//!   (`x = 26 pt`).
//! - Calm, zero-bounce permission state pills (`Allow` -> `Asking…` ->
//!   `✓ Allowed`) with no harsh blinking or glow.
//! - Escaping Accessibility panel: when Accessibility is requested, the helper
//!   panel inside Hex's card detaches, leaving a recessed outline socket
//!   behind, and floats down across the screen to dock underneath System
//!   Settings. Then the Hex row lifts cleanly out of that guide into an
//!   expanding slot in System Settings' Accessibility list.

use rune::{Anim, Color, El, Spring, TextStyle, Transition, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, glyphs};

use super::super::controls::{Paint, PushSize};
use super::flows::continue_button;
use super::hex_meter::hex_meter;
use super::landing::{LEAD, Motion, arrives, title_font};
use super::live::Live;
use super::stories::Onboard;
use crate::onboarding::{ItemStatus, SetupItem};

/// Total duration of the automated permissions step.
pub(crate) const PERMISSIONS_SECS: f64 = 7.2;

const COL_W: f32 = 440.0;
const STAGE_H: f32 = 424.0;
const ACTION_H: f32 = 32.0;

// Timeline beats (seconds since `permissions_from`).
const MIC_ASK: f64 = 0.85;
const MIC_PRESS: f64 = 1.75;
pub(crate) const MIC_GRANT: f64 = 2.10;
const AX_TRAY_OPEN: f64 = 2.25;
pub(crate) const AX_ESCAPE: f64 = 2.85;
const AX_DOCKED: f64 = 3.55;
const DRAG: (f64, f64) = (3.85, 4.75);
const SWITCH_ON: f64 = 4.90;
pub(crate) const AX_GRANT: f64 = 5.15;
const SETTINGS_CLOSE: f64 = 5.80;
const CONTINUE_AT: f64 = 6.00;

// Unified geometry inside the `440 pt` stage.
const ROW_H: f32 = 58.0;
const SOCKET_TRAY_H: f32 = 52.0;
const SOCKET_RECT: [f32; 4] = [12.0, 2.0 * ROW_H + 6.0, COL_W - 24.0, 38.0];
const SETTINGS_Y: f32 = 2.0 * ROW_H + SOCKET_TRAY_H + 14.0; // 182.0
const SETTINGS_H: f32 = 152.0;
const LIST_INSET: f32 = 16.0;
const LIST_Y: f32 = 54.0;
const LIST_W: f32 = COL_W - 2.0 * LIST_INSET; // 408.0
const LIST_ROW_H: f32 = 28.0;
const ROW_PX: f32 = 10.0; // App icons sit at `16.0 + 10.0 = 26.0` everywhere!
const GUIDE_Y: f32 = SETTINGS_Y + SETTINGS_H + 10.0; // 344.0
const GUIDE_H: f32 = 70.0;
const PROMPT_W: f32 = 276.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermState {
    Allow { primary: bool },
    Asking,
    Allowed,
}

pub(crate) fn state_at(e: f64, ask: f64, grant: f64, primary: bool) -> PermState {
    if e < ask {
        PermState::Allow { primary }
    } else if e < grant {
        PermState::Asking
    } else {
        PermState::Allowed
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn page(p: Paint, s: &Onboard) -> El {
    if let Some(live) = &s.live {
        return live_page(p, live, s.motion);
    }
    let e = s.permissions_from.map_or(0.0, |from| s.now - from);
    let reduced = s.reduced.get();
    let mic = state_at(e, MIC_ASK, MIC_GRANT, true);
    let ax = state_at(e, AX_ESCAPE, AX_GRANT, mic == PermState::Allowed);

    let head = heading(p, s.motion);

    col()
        .key("permissions")
        .gap(22.0)
        .items_center()
        .child(head)
        .child(s.motion.staggered(stage(p, e, mic, ax, reduced), 1.0, 8.0))
        .child(
            row()
                .key("action")
                .h(ACTION_H)
                .center()
                .child(if e >= CONTINUE_AT {
                    continue_button(p, s)
                } else {
                    el().key("none")
                }),
        )
}

fn heading(p: Paint, motion: Motion) -> El {
    motion.heading(
        col()
            .key("permissions-head")
            .gap(8.0)
            .items_center()
            .child(
                text("Let Hex hear you and type for you")
                    .key("title")
                    .font(title_font(p))
                    .color(p.text()),
            )
            .child(
                text("macOS asks once. You can change these any time in System Settings.")
                    .key("lead")
                    .font(LEAD)
                    .color(p.muted())
                    .align(rune::Align::Center),
            ),
    )
}

/// The real first run's permissions: macOS's own prompt and System Settings
/// with Hex's guide, and the rows follow what macOS reports.
fn live_page(p: Paint, live: &Live, motion: Motion) -> El {
    let k = p.k;
    let mic_ready = live.microphone == ItemStatus::Ready;
    let ax_desc = match live.accessibility {
        ItemStatus::Waiting => "Switch on Hex in System Settings",
        _ => "Notices the shortcut and pastes your words in any app",
    };
    let mic_desc = match live.microphone {
        ItemStatus::Denied => "Switch on Hex in System Settings",
        _ => "Hears you while you hold the shortcut",
    };
    let card = col()
        .key("setup-card")
        .w(COL_W)
        .radius(k.radius.lg)
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .child(perm_row(
            p,
            "mic-row",
            &glyphs::MIC,
            "Microphone",
            mic_desc,
            live_pill(p, "mic-pill", live.microphone, true, SetupItem::Microphone),
        ))
        .child(separator(p, "sep0"))
        .child(perm_row(
            p,
            "ax-row",
            &glyphs::CURSOR_TEXT,
            "Accessibility",
            ax_desc,
            live_pill(
                p,
                "ax-pill",
                live.accessibility,
                mic_ready,
                SetupItem::Accessibility,
            ),
        ));
    let action = if live.restart {
        Button::new("Restart Hex")
            .variant(ButtonVariant::Contrast)
            .on_press(super::live::restart_hex)
            .build_push(k)
            .key("restart")
            .default_button()
            .transition(arrives())
    } else {
        el().key("none")
    };
    col()
        .key("permissions")
        .gap(22.0)
        .items_center()
        .child(heading(p, motion))
        .child(motion.staggered(k.elevate(card, &k.elevation.raised), 1.0, 8.0))
        .child(row().key("action").h(ACTION_H).center().child(action))
}

fn live_pill(p: Paint, key: &str, status: ItemStatus, primary: bool, item: SetupItem) -> El {
    let k = p.k;
    let allow = |label: &str, primary: bool| {
        Button::new(label)
            .variant(if primary {
                ButtonVariant::Contrast
            } else {
                ButtonVariant::Neutral
            })
            .on_press(super::live::allow(item))
            .build_push(k)
            .key(format!("{key}-allow"))
            .transition(arrives())
    };
    match status {
        ItemStatus::Ready => perm_pill(p, key, PermState::Allowed),
        ItemStatus::Todo => allow("Allow", primary),
        ItemStatus::Denied => allow("Open System Settings", primary),
        // Pressing again reopens macOS's prompt or System Settings.
        ItemStatus::Waiting => allow("Open", false),
        ItemStatus::Restart => row()
            .key(format!("{key}-restart"))
            .h(28.0)
            .px(6.0)
            .center_y()
            .transition(arrives())
            .child(
                text("Needs a restart")
                    .key("lbl")
                    .font(k.ty.small_medium)
                    .color(p.muted()),
            ),
        ItemStatus::Downloading(_) | ItemStatus::Failed => el().key(format!("{key}-none")),
    }
}

fn stage(p: Paint, e: f64, mic: PermState, ax: PermState, reduced: bool) -> El {
    let tray_open = (AX_TRAY_OPEN..SETTINGS_CLOSE).contains(&e);
    let escaped = e >= AX_ESCAPE;
    let settings_open = (AX_ESCAPE..SETTINGS_CLOSE).contains(&e);

    let mut stage = el().key("permissions-stage").size(COL_W, STAGE_H);

    // 1. Hex's own permissions card at the top of the `440 pt` column.
    stage = stage.child(setup_card(p, mic, ax, tray_open, escaped));

    // 2. macOS Microphone permission alert (`MIC_ASK .. MIC_GRANT`).
    if (MIC_ASK..MIC_GRANT).contains(&e) {
        stage = stage.child(
            microphone_prompt(p, e >= MIC_PRESS)
                .absolute((COL_W - PROMPT_W) * 0.5, 2.0 * ROW_H + 18.0)
                .transition(arrives()),
        );
    }

    // 3. System Settings (`Privacy & Security › Accessibility`).
    if settings_open {
        stage = stage.child(
            settings_window(p, e, reduced)
                .absolute(0.0, SETTINGS_Y)
                .transition(arrives()),
        );
    }

    // 4. The Accessibility helper panel: sits inside Hex's card at `AX_TRAY_OPEN`,
    //    then escapes out of the card at `AX_ESCAPE` and floats down to dock
    //    underneath System Settings (`GUIDE_Y`), then launches the drag row!
    if tray_open {
        stage = stage.child(escaping_guide_and_drag(p, e, reduced));
    }

    stage
}

fn setup_card(p: Paint, mic: PermState, ax: PermState, tray_open: bool, escaped: bool) -> El {
    let k = p.k;
    let ax_desc = if ax == PermState::Asking {
        "Drag Hex into System Settings below"
    } else {
        "Notices the shortcut and pastes your words in any app"
    };
    let mut card = col()
        .key("setup-card")
        .absolute(0.0, 0.0)
        .w(COL_W)
        .radius(k.radius.lg)
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .motion(Spring::new(0.30, 0.0))
        .child(perm_row(
            p,
            "mic-row",
            &glyphs::MIC,
            "Microphone",
            "Hears you while you hold the shortcut",
            perm_pill(p, "mic-pill", mic),
        ))
        .child(separator(p, "sep0"))
        .child(perm_row(
            p,
            "ax-row",
            &glyphs::CURSOR_TEXT,
            "Accessibility",
            ax_desc,
            perm_pill(p, "ax-pill", ax),
        ));

    if tray_open {
        card = card.child(
            el().key("escape-socket-tray")
                .w(COL_W)
                .h(SOCKET_TRAY_H)
                .transition(Transition::opacity())
                .child(socket_outline(p, escaped)),
        );
    }

    k.elevate(card, &k.elevation.raised)
}

/// Recessed outline left behind inside Hex's card once the helper panel escapes.
fn socket_outline(p: Paint, escaped: bool) -> El {
    let k = p.k;
    row()
        .key("socket-outline")
        .absolute(12.0, 5.0)
        .size(COL_W - 24.0, 38.0)
        .px(12.0)
        .center_y()
        .justify_between()
        .radius(8.0)
        .fill(k.c.bg_deep.mix(k.c.bg_base, 0.45))
        .border(p.hair, k.c.border_base)
        .children(escaped.then(|| {
            text("Floating beside System Settings")
                .key("escaped-note")
                .font(k.ty.small)
                .color(p.faint())
                .transition(Transition::opacity())
        }))
}

fn separator(p: Paint, key: &str) -> El {
    el().key(key).h(p.hair).mx(16.0).fill(p.k.c.border_muted)
}

fn perm_row(
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    desc: &str,
    control: El,
) -> El {
    let k = p.k;
    row()
        .key(key)
        .h(ROW_H)
        .px(16.0)
        .gap(12.0)
        .center_y()
        .child(icon(glyph, 16.0).key("icon").color(k.c.icon_base))
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .gap(2.0)
                .child(
                    text(title)
                        .key("title")
                        .font(k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(text(desc).key("desc").font(k.ty.small).color(p.muted())),
        )
        .child(control)
}

/// Calm, zero-bounce permission control: no frantic blinking dot or spring bounce.
fn perm_pill(p: Paint, key: &str, state: PermState) -> El {
    let k = p.k;
    match state {
        PermState::Allow { primary } => Button::new("Allow")
            .variant(if primary {
                ButtonVariant::Contrast
            } else {
                ButtonVariant::Neutral
            })
            .on_press(|_: &mut Onboard| {})
            .build_push(k)
            .key(format!("{key}-allow"))
            .transition(arrives()),
        PermState::Asking => row()
            .key(format!("{key}-asking"))
            .h(28.0)
            .px(10.0)
            .gap(6.0)
            .center_y()
            .radius(k.radius.md)
            .fill(k.c.surface_raised)
            .border(p.hair, k.c.border_weak)
            .transition(arrives())
            .child(
                el().key("dot")
                    .size(6.0, 6.0)
                    .radius(3.0)
                    .fill(p.k.c.icon_muted),
            )
            .child(
                text("Open")
                    .key("lbl")
                    .font(k.ty.small_medium)
                    .color(p.muted()),
            ),
        PermState::Allowed => row()
            .key(format!("{key}-allowed"))
            .h(28.0)
            .px(6.0)
            .gap(5.0)
            .center_y()
            .transition(arrives())
            .child(
                icon(k.glyph.check_small, 16.0)
                    .key("check")
                    .color(p.success()),
            )
            .child(
                text("Allowed")
                    .key("lbl")
                    .font(k.ty.ui_medium)
                    .color(p.success()),
            ),
    }
}

fn microphone_prompt(p: Paint, pressed: bool) -> El {
    let k = p.k;
    let button = |key: &str, label: &str, variant: ButtonVariant| {
        Button::new(label)
            .variant(variant)
            .on_press(|_: &mut Onboard| {})
            .build_push(k)
            .key(key.to_string())
            .grow(1.0)
    };
    let badge = el()
        .key("app-badge")
        .size(40.0, 40.0)
        .radius(10.0)
        .continuous_corners()
        .center()
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .child(hex_meter(p, "icon", 22.0, 1.0, 1.0));
    let alert = col()
        .key("mic-prompt")
        .w(PROMPT_W)
        .padding(18.0)
        .gap(10.0)
        .items_center()
        .radius(18.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .child(badge)
        .child(
            text("“Hex” would like to access the microphone.")
                .key("title")
                .font(TextStyle {
                    weight: if k.dark { 560.0 } else { 600.0 },
                    ..k.ty.ui_medium
                })
                .color(p.text())
                .align(rune::Align::Center)
                .wrap()
                .w(PROMPT_W - 36.0),
        )
        .child(
            text("Hex listens only while you hold your shortcut.")
                .key("body")
                .font(k.ty.small)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .w(PROMPT_W - 36.0),
        )
        .child(
            row()
                .key("buttons")
                .w_full()
                .gap(8.0)
                .pt(4.0)
                .child(button("deny", "Don't Allow", ButtonVariant::Neutral))
                .child(
                    button("allow", "Allow", ButtonVariant::Contrast)
                        .scale(if pressed { 0.96 } else { 1.0 })
                        .opacity(if pressed { 0.84 } else { 1.0 })
                        .motion(Spring::new(0.12, 0.0)),
                ),
        );
    k.elevate(alert, &k.elevation.floating)
}

/// System Settings' Accessibility pane (`w = 440 pt`), with its app icons at
/// `x = 26 pt` (`LIST_INSET + ROW_PX`).
fn settings_window(p: Paint, e: f64, reduced: bool) -> El {
    let k = p.k;
    let dark = k.dark;
    let drag_u = drag_progress(e, reduced);
    let slot_open = drag_u.is_some_and(|u| u >= 0.42) || e >= DRAG.1;
    let landed = e >= DRAG.1;

    let dot = |key: &str, color: Color| {
        el().key(key.to_string())
            .size(9.0, 9.0)
            .radius(4.5)
            .fill(color)
    };
    let bar = row()
        .key("title-bar")
        .absolute(0.0, 0.0)
        .w(COL_W)
        .h(28.0)
        .px(14.0)
        .gap(6.0)
        .center_y()
        .child(dot("close", rune_theme_opencode::ember(dark, 9)))
        .child(dot(
            "minimize",
            rune_theme_opencode::scale("amber", dark, 9),
        ))
        .child(dot("zoom", rune_theme_opencode::scale("apple", dark, 9)))
        .child(
            text("Privacy & Security  ›  Accessibility")
                .key("title")
                .font(k.ty.small_medium)
                .color(p.muted())
                .pl(10.0),
        );

    let mut list = col()
        .key("list")
        .absolute(LIST_INSET, LIST_Y)
        .w(LIST_W)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_weak)
        .motion(Spring::new(0.26, 0.0))
        .child(settings_app_row(
            p,
            "raycast",
            "Raycast",
            squircle_icon(rune_theme_opencode::ember(dark, 9)),
            true,
        ))
        .child(
            el().key("lsep0")
                .h(p.hair)
                .mx(ROW_PX)
                .fill(k.c.border_muted),
        )
        .child(settings_app_row(
            p,
            "rectangle",
            "Rectangle",
            squircle_icon(rune_theme_opencode::scale("cobalt", dark, 9)),
            true,
        ));

    if slot_open {
        list = list.child(
            el().key("lsep1")
                .h(p.hair)
                .mx(ROW_PX)
                .fill(k.c.border_muted),
        );
        if landed {
            list = list.child(
                settings_app_row(p, "hex-landed", "Hex", hex_app_icon(p), e >= SWITCH_ON)
                    .transition(Transition::opacity()),
            );
        } else {
            // Subtle drop-target highlight waiting to catch the dragged Hex row.
            list = list.child(
                row()
                    .key("drop-slot")
                    .h(LIST_ROW_H)
                    .px(ROW_PX)
                    .center_y()
                    .fill(k.c.bg_deep.mix(k.c.surface_raised, 0.5))
                    .border(p.hair, k.c.border_focus),
            );
        }
    }

    let win = el()
        .key("settings-window")
        .size(COL_W, SETTINGS_H)
        .radius(12.0)
        .continuous_corners()
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak)
        .child(bar)
        .child(
            text("Allow the applications below to control your computer.")
                .key("explain")
                .font(k.ty.small)
                .color(p.muted())
                .absolute(LIST_INSET, 31.0),
        )
        .child(list);
    k.elevate(win, &k.elevation.floating)
}

/// The panel that sits inside Hex's card, detaches at `AX_ESCAPE` leaving an
/// outline behind, floats down underneath System Settings, and then launches
/// the `Hex` drag row straight up the `x = 26 pt` spine into the list.
fn escaping_guide_and_drag(p: Paint, e: f64, reduced: bool) -> El {
    let k = p.k;
    let esc_u = if e < AX_ESCAPE {
        0.0
    } else if reduced {
        1.0
    } else {
        smoothstep(((e - AX_ESCAPE) / (AX_DOCKED - AX_ESCAPE)) as f32)
    };

    let x = lerp(SOCKET_RECT[0], 0.0, esc_u);
    let y = lerp(SOCKET_RECT[1], GUIDE_Y, esc_u) - 8.0 * (esc_u * std::f32::consts::PI).sin();
    let w = lerp(SOCKET_RECT[2], COL_W, esc_u);
    let h = lerp(SOCKET_RECT[3], GUIDE_H, esc_u);
    let lift_scale = 1.0 + 0.025 * (esc_u * std::f32::consts::PI).sin();
    let tilt = -0.018 * (esc_u * std::f32::consts::PI).sin();

    let drag_u = drag_progress(e, reduced);
    let tile_taken = drag_u.is_some() || e >= DRAG.1;

    // Inside the panel: `tile_x` moves from `4` (at `x = 12`, putting the icon
    // at `12 + 4 + 10 = 26`) to `16` (at `x = 0`, putting the icon at `0 + 16 + 10 = 26`)!
    let tile_x = lerp(4.0, LIST_INSET, esc_u);
    let tile_y = lerp(4.0, 30.0, esc_u);
    let tile_w = LIST_W;

    let instruction = (esc_u > 0.35).then(|| {
        text("↑  Drag Hex into the Accessibility list above")
            .key("guide-instruction")
            .font(k.ty.small_medium)
            .color(p.text())
            .absolute(LIST_INSET, 8.0)
            .opacity(((esc_u - 0.35) / 0.45).clamp(0.0, 1.0))
            .opacity_motion(Anim::INSTANT)
    });

    // When `tile_taken`, ONLY the empty recessed slot remains — zero duplicate
    // ghost `"Hex"` text underneath the lifted chip!
    let inner_slot = if tile_taken {
        el().key("guide-tile-empty")
            .absolute(tile_x, tile_y)
            .size(tile_w, 30.0)
            .radius(7.0)
            .fill(k.c.bg_deep.mix(k.c.bg_base, 0.5))
            .border(p.hair, k.c.border_weak)
    } else {
        row()
            .key("guide-tile")
            .absolute(tile_x, tile_y)
            .size(tile_w, 30.0)
            .px(ROW_PX)
            .gap(8.0)
            .center_y()
            .radius(7.0)
            .fill(k.c.surface_raised)
            .border(p.hair, k.c.border_base)
            .offset_motion(Anim::INSTANT)
            .child(hex_app_icon(p))
            .child(
                text("Hex")
                    .key("name")
                    .font(k.ty.small_medium)
                    .color(p.text()),
            )
            .child(el().key("sp").grow(1.0))
            .children((esc_u < 0.25).then(|| {
                text("Accessibility helper")
                    .key("sub")
                    .font(k.ty.small)
                    .color(p.faint())
            }))
    };

    let mut panel = el()
        .key("escaping-panel")
        .absolute(x, y)
        .size(w, h)
        .radius(lerp(8.0, 12.0, esc_u))
        .continuous_corners()
        .fill(if esc_u < 0.05 {
            k.c.surface_raised
        } else {
            k.c.bg_base
        })
        .border(
            p.hair,
            if esc_u > 0.05 && esc_u < 0.98 {
                k.c.border_focus
            } else {
                k.c.border_weak
            },
        )
        .scale(lift_scale)
        .rotate(tilt)
        .offset_motion(Anim::INSTANT)
        .rotate_motion(Anim::INSTANT)
        .children(instruction)
        .child(inner_slot);

    if esc_u > 0.02 {
        panel = k.elevate(panel, &k.elevation.floating);
    }

    let mut layer = el()
        .key("escape-layer")
        .absolute(0.0, 0.0)
        .size(COL_W, STAGE_H)
        .instant_changes()
        .transition(Transition::opacity())
        .child(panel);

    if let Some(u) = drag_u {
        layer = layer.child(dragged_row(p, u));
    }

    layer
}

fn drag_progress(e: f64, reduced: bool) -> Option<f32> {
    if reduced || !(DRAG.0..DRAG.1).contains(&e) {
        return None;
    }
    Some(smoothstep(((e - DRAG.0) / (DRAG.1 - DRAG.0)) as f32))
}

/// The lifted `Hex` row travelling up the `x = 26 pt` icon spine from the
/// docked guide into Row 3 of System Settings' Accessibility list.
fn dragged_row(p: Paint, u: f32) -> El {
    let k = p.k;
    let from_y = GUIDE_Y + 30.0;
    let to_y = SETTINGS_Y + LIST_Y + 2.0 * LIST_ROW_H + 2.0 * p.hair;
    let y = lerp(from_y, to_y, u) - 8.0 * (u * std::f32::consts::PI).sin();
    let lift = 1.0 + 0.03 * (u * std::f32::consts::PI).sin();

    let chip = row()
        .key("drag-chip")
        .absolute(LIST_INSET, y)
        .size(LIST_W, LIST_ROW_H)
        .px(ROW_PX)
        .gap(8.0)
        .center_y()
        .radius(7.0)
        .fill(k.c.surface_raised)
        .border(p.hair, k.c.border_focus)
        .scale(lift)
        .offset_motion(Anim::INSTANT)
        .child(hex_app_icon(p))
        .child(
            text("Hex")
                .key("name")
                .font(k.ty.small_medium)
                .color(p.text()),
        );
    k.elevate(chip, &k.elevation.floating)
}

fn squircle_icon(color: Color) -> El {
    el().key("icon")
        .size(18.0, 18.0)
        .radius(4.5)
        .continuous_corners()
        .fill(color)
}

fn hex_app_icon(p: Paint) -> El {
    el().key("hex-app-icon")
        .size(18.0, 18.0)
        .radius(4.5)
        .continuous_corners()
        .center()
        .fill(p.k.c.bg_deep)
        .border(p.hair, p.k.c.border_base)
        .child(hex_meter(p, "hex", 12.0, 1.0, 1.0))
}

fn settings_app_row(p: Paint, key: &str, name: &str, icon: El, on: bool) -> El {
    let k = p.k;
    row()
        .key(key.to_string())
        .h(LIST_ROW_H)
        .px(ROW_PX)
        .gap(8.0)
        .center_y()
        .child(icon)
        .child(
            text(name)
                .key("name")
                .font(k.ty.small)
                .color(p.text())
                .grow(1.0),
        )
        .child(macos_switch(p, on))
}

fn macos_switch(p: Paint, on: bool) -> El {
    let k = p.k;
    let knob = el()
        .key("knob")
        .absolute(if on { 12.5 } else { 1.5 }, 1.5)
        .size(12.0, 12.0)
        .radius(6.0)
        .fill(k.c.white)
        .motion(Spring::new(0.22, 0.0));
    el().key("switch")
        .size(26.0, 15.0)
        .radius(7.5)
        .fill(if on { p.accent() } else { k.c.border_base })
        .child(knob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_permission_asks_then_is_granted_in_order() {
        assert_eq!(
            state_at(0.0, MIC_ASK, MIC_GRANT, true),
            PermState::Allow { primary: true }
        );
        assert_eq!(state_at(1.2, MIC_ASK, MIC_GRANT, true), PermState::Asking);
        assert_eq!(state_at(2.5, MIC_ASK, MIC_GRANT, true), PermState::Allowed);
        const { assert!(MIC_GRANT < AX_ESCAPE && AX_DOCKED < DRAG.0 && DRAG.1 < AX_GRANT) };
        const { assert!(AX_GRANT < CONTINUE_AT && CONTINUE_AT < PERMISSIONS_SECS) };
    }

    #[test]
    fn settings_list_and_docked_guide_share_one_icon_spine() {
        let settings_icon_x = LIST_INSET + ROW_PX;
        let docked_guide_icon_x = 0.0 + LIST_INSET + ROW_PX;
        assert_eq!(settings_icon_x, 26.0);
        assert_eq!(settings_icon_x, docked_guide_icon_x);
        const { assert!(GUIDE_Y + GUIDE_H <= STAGE_H) };
    }
}
