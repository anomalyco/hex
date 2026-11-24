//! The inline `Always…` rule configurator hanging right under the `Sent to …`
//! capsule: lets the user scope the rule (subpath vs whole site, terminal
//! working folder, or non-general app) and choose whether it targets this
//! specific session or the project's newest session, then save it or jump
//! straight to Hex's `Auto-send rules` sheet.

use rune::{El, Icon, SpringPair, Transition, col, el, row, text};
use rune_theme_opencode::icons as oc;

use super::flow::AlwaysConfig;
use super::surface::{Id, LIST_Y, PANEL_W, SHELL_RADIUS, ellipsize, measure, medium, small};
use super::view::{BUTTON_H, ICON_BUTTON_RADIUS, press, rim};
use super::{CapsuleState, LIST_DROP, PRESENCE_BLUR, Part, tuning};
use crate::capture_host::RuleMatch;
use crate::desktop::i18n::tr;
use crate::rune_ui::controls::Paint;
use crate::rune_ui::icons;

pub(crate) const POP_W: f32 = 320.0;
const POP_PAD: f32 = 6.0;
const POP_RADIUS: f32 = SHELL_RADIUS;
const OPTION_H: f32 = 44.0;
const HEADER_H: f32 = 26.0;
const FOOTER_H: f32 = 44.0;

fn scope_count(rule_match: &RuleMatch) -> usize {
    match rule_match {
        RuleMatch::Site {
            subpath: Some(_), ..
        } => 2,
        RuleMatch::Site { subpath: None, .. }
        | RuleMatch::Terminal { .. }
        | RuleMatch::App { .. } => 1,
    }
}

fn to_count(config: &AlwaysConfig) -> usize {
    if config.project_dir.is_some() { 2 } else { 1 }
}

/// Height of the inline `Always…` configurator panel in points.
pub(crate) fn always_height(config: &AlwaysConfig) -> f32 {
    let options = (scope_count(&config.rule_match) + to_count(config)) as f32;
    2.0 * POP_PAD + 2.0 * HEADER_H + 4.0 + options * OPTION_H + FOOTER_H
}

fn header(p: Paint, key: &str, copy: &str) -> El {
    row()
        .key(key.to_owned())
        .h(HEADER_H)
        .px(10.0)
        .center_y()
        .child(text(copy).font(small()).color(p.muted()))
}

fn option(
    p: Paint,
    key: &str,
    id: Id,
    glyph: &'static Icon,
    name: &str,
    detail: &str,
    on: bool,
) -> El {
    let c = &p.k.c;
    let max_w = POP_W - 2.0 * POP_PAD - 20.0 - 16.0 - 10.0 - 24.0;
    let name_fit = ellipsize(name, medium(), max_w.max(60.0));
    let detail_fit = ellipsize(detail, small(), max_w.max(60.0));
    let base = row().key(key.to_owned());
    let base = if on {
        base.fill(c.layer_02)
    } else {
        base.hover(c.hover)
    };
    base.h(OPTION_H)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .radius(POP_RADIUS - POP_PAD)
        .press(c.pressed)
        .role("button")
        .label(format!("{name} · {detail}"))
        .checked(on)
        .hit()
        .on_press_tx(press(id))
        .child(rune::icon(glyph, 16.0).color(if on { p.text() } else { p.muted() }))
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .gap(1.0)
                .child(text(name_fit).font(medium()).color(p.text()))
                .child(text(detail_fit).font(small()).color(p.muted())),
        )
        .children(on.then(|| {
            rune::icon(p.k.glyph.check_small, 16.0)
                .key("check")
                .color(p.text())
        }))
}

/// Renders the inline `Always…` rule configurator right under the capsule.
pub(super) fn always_view(p: Paint, config: &AlwaysConfig, reduced: bool) -> El {
    let k = p.k;
    let c = &k.c;
    let mut body = col()
        .key("body")
        .absolute(0.0, 0.0)
        .w(POP_W)
        .px(POP_PAD)
        .py(POP_PAD)
        .child(header(p, "from", tr("When I annotate")));

    match &config.rule_match {
        RuleMatch::Site {
            subpath: Some(subpath),
            host,
            ..
        } => {
            body = body
                .child(option(
                    p,
                    "scope-narrow",
                    Id::AlwaysScopeNarrow,
                    &icons::GLOBE,
                    subpath,
                    tr("This page and pages under it"),
                    config.use_subpath,
                ))
                .child(option(
                    p,
                    "scope-wide",
                    Id::AlwaysScopeWide,
                    &icons::GLOBE,
                    host,
                    tr("The whole site"),
                    !config.use_subpath,
                ));
        }
        RuleMatch::Site {
            subpath: None,
            host,
            ..
        } => {
            body = body.child(option(
                p,
                "scope-wide",
                Id::AlwaysScopeWide,
                &icons::GLOBE,
                host,
                tr("The whole site"),
                true,
            ));
        }
        RuleMatch::Terminal { display, .. } => {
            body = body.child(option(
                p,
                "scope-narrow",
                Id::AlwaysScopeNarrow,
                &icons::FOLDER,
                display,
                tr("Terminal folder, and folders in it"),
                true,
            ));
        }
        RuleMatch::App { name, .. } => {
            body = body.child(option(
                p,
                "scope-narrow",
                Id::AlwaysScopeNarrow,
                &oc::MONITOR,
                name,
                tr("Any window of this app"),
                true,
            ));
        }
    }

    body = body
        .child(el().h(4.0))
        .child(header(p, "to", tr("Send to")))
        .child(option(
            p,
            "to-session",
            Id::AlwaysToSession,
            &oc::SPEECH_BUBBLE,
            &config.session_name,
            tr("This session, always"),
            config.to_session || config.project_dir.is_none(),
        ));
    if let Some((_, folder_name)) = &config.project_dir {
        body = body.child(option(
            p,
            "to-project",
            Id::AlwaysToProject,
            &oc::FOLDER,
            folder_name,
            tr("The project's newest session"),
            !config.to_session,
        ));
    }

    let h = always_height(config);
    let body_h = h - FOOTER_H;
    let save_label = tr("Save rule");
    let save_w = measure(save_label, medium()) + 24.0;
    let footer = row()
        .key("footer")
        .absolute(0.0, body_h)
        .size(POP_W, FOOTER_H)
        .px(POP_PAD + 2.0)
        .center_y()
        .child(
            row()
                .key("all")
                .h(BUTTON_H)
                .px(8.0)
                .center_y()
                .radius(ICON_BUTTON_RADIUS)
                .hover(c.hover)
                .press(c.pressed)
                .role("button")
                .label(tr("All rules…"))
                .hit()
                .on_press_tx(press(Id::OpenRules))
                .child(text(tr("All rules…")).font(medium()).color(p.muted())),
        )
        .child(el().grow(1.0))
        .child(
            row()
                .key("save")
                .size(save_w, BUTTON_H)
                .center()
                .radius(ICON_BUTTON_RADIUS)
                .fill(c.bg_contrast)
                .hover(c.contrast_hover)
                .press(c.contrast_pressed)
                .role("button")
                .label(save_label)
                .hit()
                .on_press_tx(press(Id::AlwaysSave))
                .child(text(save_label).font(medium()).color(c.text_contrast)),
        );
    let line = el()
        .key("footer-line")
        .absolute(0.0, body_h)
        .size(POP_W, p.hair)
        .fill(c.border_weak);
    let x = ((PANEL_W - POP_W) * 0.5).round();
    let blur = if reduced { 0.0 } else { PRESENCE_BLUR };
    let mut panel = el()
        .key("always-popover")
        .trace_name("always-popover")
        .absolute(x, LIST_Y)
        .size(POP_W, h)
        .radius(POP_RADIUS)
        .fill(c.bg_base);
    for shadow in &k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    panel = panel.child(body).child(line).child(footer);
    if k.dark {
        panel = panel.child(rim(k, POP_W, h, POP_RADIUS));
    }
    panel
        .motion(tuning::get().island)
        .role("dialog")
        .label(tr("Auto-send rules"))
        .hit()
        .on_hover_change(|s: &mut CapsuleState, on, tx| s.hover(Part::List, on, tx))
        .on_pointer_move_local(|s: &mut CapsuleState, _, tx| s.pointer_moved(tx))
        .transition(
            Transition::offset(0.0, -LIST_DROP)
                .combined(Transition::opacity())
                .combined(Transition::blur(blur))
                .springs(SpringPair::new(
                    tuning::get().content_in,
                    tuning::get().content_out,
                )),
        )
}
