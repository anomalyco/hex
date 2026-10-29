//! Labs: the opt-in features and the experiments, folded under one sidebar
//! row. A feature turned on joins the main sidebar; the experiments' panes
//! list under Labs.

use rune::{El, Icon, Tx, col, row, text};

use super::controls::{self, CardRow, Paint, card, icon_row, pressable_row, row_switch};
use super::{NAV_ROW_GAP, NAV_ROW_HEIGHT, Pane, Ui};
use crate::desktop_i18n::tr;

pub(super) static LABS_ICON: Icon = Icon {
    name: "hex-labs",
    view: 16.0,
    body: r#"<path d="M6.25 2.5h3.5M6.75 2.5v4L3.2 12.3a.8.8 0 0 0 .7 1.2h8.2a.8.8 0 0 0 .7-1.2L9.25 6.5v-4M4.8 10h6.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// One opt-in feature: off, it shows only on the Labs page; on, its pane
/// joins the sidebar.
struct Lab {
    key: &'static str,
    title: &'static str,
    about: &'static str,
    on: bool,
    flip: fn(&mut Ui),
    pane: Pane,
}

fn labs(ui: &Ui) -> Vec<Lab> {
    let settings = &ui.settings;
    let mut labs = vec![
        Lab {
            key: "lab-commands",
            title: tr("Commands"),
            about: tr("Open apps and press keys by voice"),
            on: settings.commands_enabled,
            flip: |ui| {
                let on = !ui.settings.commands_enabled;
                super::commands::set_enabled(ui, on);
            },
            pane: Pane::Commands,
        },
        Lab {
            key: "lab-meetings",
            title: tr("Meetings"),
            about: tr("Record calls and keep transcripts"),
            on: settings.meetings.enabled,
            flip: |ui| {
                let on = !ui.settings.meetings.enabled;
                ui.update(|settings| settings.meetings.enabled = on);
            },
            pane: Pane::Meetings,
        },
    ];
    labs.retain(|lab| lab.pane.shown(ui));
    labs
}

/// An opt-in feature's pane is in the main sidebar: it's on.
fn turned_on(ui: &Ui, pane: Pane) -> bool {
    match pane {
        Pane::Commands => ui.settings.commands_enabled,
        Pane::Meetings => super::meetings::available(ui),
        _ => false,
    }
}

/// The main sidebar's panes, in order: General, History, then the opt-in
/// features turned on.
pub(super) fn main_panes(ui: &Ui) -> Vec<Pane> {
    let mut panes = vec![Pane::General, Pane::History];
    panes.extend(
        [Pane::Commands, Pane::Meetings]
            .into_iter()
            .filter(|pane| pane.shown(ui) && turned_on(ui, *pane)),
    );
    panes
}

/// The experiments' panes this install has, Modes among them: listed under
/// Labs, never in the main sidebar.
pub(super) fn experiments(ui: &Ui) -> Vec<Pane> {
    Pane::ALL
        .iter()
        .copied()
        .filter(|pane| pane.shown(ui) && (pane.experimental() || *pane == Pane::Modes))
        .collect()
}

/// Every pane the sidebar can open, in its order, for Command-1 through 9.
pub(super) fn order(ui: &Ui) -> Vec<Pane> {
    let mut panes = main_panes(ui);
    panes.extend(experiments(ui));
    panes
}

/// The Labs row, and, while Labs or one of its panes is open, the
/// experiments' panes indented under it.
pub(super) fn nav(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let inside = experiments(ui);
    let expanded = ui.labs_open || inside.contains(&ui.pane);
    let on = ui.labs_open;
    let mut item = row()
        .key("nav-labs")
        .role("button")
        .label(tr("Labs"))
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            if on {
                ui.select_pane(Pane::General, tx);
            } else {
                open(ui, tx);
            }
        })
        .child(rune::icon(&LABS_ICON, 16.0).key("icon").color(if on {
            c.icon_base
        } else {
            c.icon_muted
        }))
        .child(text(tr("Labs")).key("label").font(p.k.ty.ui).color(if on {
            c.text_base
        } else {
            c.text_muted
        }))
        .child(row().key("spacer").grow(1.0))
        .children((!inside.is_empty()).then(|| {
            rune::icon(
                if expanded {
                    p.k.glyph.triangle_down
                } else {
                    p.k.glyph.triangle_right
                },
                14.0,
            )
            .key("arrow")
            .color(c.icon_weaker)
        }));
    if on {
        item = item.fill(c.layer_02);
    }
    let mut group = col()
        .key("labs-group")
        .pt(NAV_ROW_GAP / 2.0)
        .pb(NAV_ROW_GAP / 2.0)
        .child(item);
    if expanded {
        for pane in inside {
            group = group.child(
                super::nav_slot(
                    p,
                    pane,
                    ui.pane == pane && !ui.labs_open,
                    NAV_ROW_GAP / 2.0,
                    NAV_ROW_GAP / 2.0,
                )
                .pl(14.0),
            );
        }
    }
    group
}

fn open(ui: &mut Ui, tx: &mut Tx) {
    ui.select_pane(Pane::General, tx);
    ui.labs_open = true;
    tx.settle();
}

/// The Labs page: the opt-in features with their switches, then the
/// experiments turned on with `hex experiments`.
pub(super) fn page(ui: &Ui, p: Paint) -> El {
    let mut rows: Vec<CardRow> = Vec::new();
    for lab in labs(ui) {
        let pane = lab.pane;
        rows.push(lab_row(p, &lab));
        if pane == Pane::Commands {
            rows.extend(super::commands::confirm_row(ui, p).map(CardRow::from));
        }
    }
    let mut content = controls::page("content").child(
        text(tr("Turn a feature on and it joins the sidebar."))
            .key("lede")
            .font(p.k.ty.ui)
            .color(p.muted())
            .wrap()
            .mx(controls::METRICS.row_px),
    );
    if !rows.is_empty() {
        content = content.child(card(p, "labs", rows));
    }
    let inside = experiments(ui);
    if !inside.is_empty() {
        let rows: Vec<CardRow> = inside.into_iter().map(|pane| pane_row(p, pane)).collect();
        content = content.child(controls::section(p, "experiments", tr("Experiments"), rows));
    }
    super::scroll_body("scroll", content)
}

/// A feature's row: pressing it flips the feature; once on, a chevron opens
/// its pane.
fn lab_row(p: Paint, lab: &Lab) -> CardRow {
    let c = &p.k.c;
    let pane = lab.pane;
    let open = lab.on.then(|| {
        rune_kit::IconButton::new(&rune_theme_opencode::icons::CHEVRON_RIGHT)
            .variant(rune_kit::IconButtonVariant::Ghost)
            .control_size(rune_kit::ControlSize::Small)
            .label(tr("Open"))
            .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
            .build(p.k)
            .key("open")
    });
    let trailing = row()
        .key("trailing")
        .gap(8.0)
        .center_y()
        .child(row_switch(p, lab.on))
        .children(open);
    let flip = lab.flip;
    pressable_row(icon_row(
        p,
        lab.key,
        pane.icon(),
        lab.title,
        Some(lab.about),
        trailing,
    ))
    .hover(c.hover)
    .on_press(move |ui: &mut Ui| flip(ui))
    .into()
}

/// An experiment's row, which opens its pane.
fn pane_row(p: Paint, pane: Pane) -> CardRow {
    let c = &p.k.c;
    let chevron = rune::icon(&rune_theme_opencode::icons::CHEVRON_RIGHT, 16.0)
        .key("chevron")
        .color(c.icon_muted);
    icon_row(
        p,
        &format!("lab-{}", pane.key()),
        pane.icon(),
        pane.title(),
        None,
        chevron,
    )
    .role("button")
    .hover(c.hover)
    .press(c.pressed)
    .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
    .into()
}
