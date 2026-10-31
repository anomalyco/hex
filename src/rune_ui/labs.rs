//! Labs: the one switch for every unfinished feature (`crate::labs`), the
//! opt-in features (Meetings beta on macOS, plus Annotate in dev builds), and
//! the Labs features' panes, folded under one sidebar row.

use rune::{El, Icon, Tx, col, row, text};

use super::controls::{self, CardRow, Paint, card, icon_row, pressable_row, row_switch};
use super::icons::{HINTS, SPARKLES, WAVE};
use super::{NAV_ROW_GAP, NAV_ROW_HEIGHT, Pane, Ui, VOICE_FILTER_ICON};
use crate::desktop_i18n::tr;

pub(super) static LABS_ICON: Icon = Icon {
    name: "hex-labs",
    view: 16.0,
    body: r#"<path d="M6.25 2.5h3.5M6.75 2.5v4L3.2 12.3a.8.8 0 0 0 .7 1.2h8.2a.8.8 0 0 0 .7-1.2L9.25 6.5v-4M4.8 10h6.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// One opt-in feature: off, it shows on the Labs page; on, its pane shows in
/// the sidebar.
struct Lab {
    key: &'static str,
    title: &'static str,
    about: &'static str,
    on: bool,
    open_when_off: bool,
    flip: fn(&mut Ui),
    pane: Pane,
}

fn labs(ui: &Ui) -> Vec<Lab> {
    let settings = &ui.settings;
    let mut labs = vec![Lab {
        key: "lab-meetings",
        title: tr("Meetings (beta)"),
        about: tr("Record calls and keep transcripts"),
        on: settings.meetings.enabled,
        open_when_off: false,
        flip: |ui| {
            let on = !ui.settings.meetings.enabled;
            ui.update(|settings| settings.meetings.enabled = on);
        },
        pane: Pane::Meetings,
    }];
    #[cfg(hex_capture)]
    if ui.platform.dev_build && ui.platform.scrawl.is_some() {
        labs.push(Lab {
            key: "lab-annotate",
            title: tr("Annotate"),
            about: tr("Record your screen, draw on it, and talk over it"),
            on: settings.capture.enabled,
            open_when_off: true,
            flip: super::scrawl::toggle,
            pane: Pane::Annotate,
        });
    }
    labs.retain(|lab| lab.pane.shown(ui));
    labs
}

/// An opt-in feature's pane is in the main sidebar: it's on.
fn turned_on(ui: &Ui, pane: Pane) -> bool {
    match pane {
        Pane::Meetings => super::meetings::available(ui),
        _ => false,
    }
}

/// The main sidebar's panes, in order: General, Output, History, then the
/// opt-in features turned on.
pub(super) fn main_panes(ui: &Ui) -> Vec<Pane> {
    let mut panes = vec![Pane::General, Pane::Output, Pane::History];
    panes.extend(
        [Pane::Meetings]
            .into_iter()
            .filter(|pane| pane.shown(ui) && turned_on(ui, *pane)),
    );
    panes
}

/// The Labs features' panes this install has: listed under Labs, never in
/// the main sidebar.
pub(super) fn lab_panes(ui: &Ui) -> Vec<Pane> {
    Pane::ALL
        .iter()
        .copied()
        .filter(|pane| pane.shown(ui) && pane.experimental())
        .collect()
}

/// Every pane the sidebar can open, in its order, for Command-1 through 9.
pub(super) fn order(ui: &Ui) -> Vec<Pane> {
    let mut panes = main_panes(ui);
    panes.extend(lab_panes(ui));
    panes
}

/// The Labs row, and, while Labs or one of its panes is open, the
/// Labs features' panes indented under it.
pub(super) fn nav(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let inside = lab_panes(ui);
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

/// The Labs page: the Labs switch, the opt-in features with their
/// switches, then, while Labs is on, what it shows (Live text and Live
/// chapters with their own switches, off by default) and Run setup again.
pub(super) fn page(ui: &Ui, p: Paint) -> El {
    let mut content =
        controls::page("content").child(card(p, "labs-switch", vec![labs_switch_row(ui, p)]));
    let rows: Vec<CardRow> = labs(ui).iter().map(|lab| lab_row(p, lab)).collect();
    if !rows.is_empty() {
        content = content
            .child(
                text(tr("Turn a feature on and it joins the sidebar."))
                    .key("lede")
                    .font(p.k.ty.ui)
                    .color(p.muted())
                    .wrap()
                    .mx(controls::METRICS.row_px),
            )
            .child(card(p, "labs", rows));
    }
    if ui.settings.labs.enabled {
        let mut included = features(ui)
            .into_iter()
            .map(|feature| feature_row(p, feature))
            .collect::<Vec<_>>();
        if super::onboarding_kit::live::restart_unavailable(ui).is_none() {
            included.push(run_setup_row(p));
        }
        content = content.child(controls::section(
            p,
            "experiments",
            tr("Experiments"),
            included,
        ));
    }
    super::scroll_body("scroll", content)
}

/// The one Labs switch, with a link to send feedback.
fn labs_switch_row(ui: &Ui, p: Paint) -> CardRow {
    let c = &p.k.c;
    let feedback = rune_kit::Button::new(tr("Send feedback"))
        .variant(rune_kit::ButtonVariant::Ghost)
        .control_size(rune_kit::ControlSize::Small)
        .on_press(|ui: &mut Ui| {
            if ui.live()
                && let Err(error) = crate::opencode_catalog::open_url(crate::labs::FEEDBACK_URL)
            {
                ui.error = Some(format!("{error:#}"));
            }
        })
        .build(p.k)
        .key("send-feedback");
    let trailing = row()
        .key("trailing")
        .gap(8.0)
        .center_y()
        .child(feedback)
        .child(row_switch(p, ui.settings.labs.enabled));
    pressable_row(icon_row(
        p,
        "labs-enabled",
        &LABS_ICON,
        tr("Labs"),
        Some(tr("Try unfinished features. They may change or break.")),
        trailing,
    ))
    .hover(c.hover)
    .on_press(flip_labs)
    .into()
}

fn flip_labs(ui: &mut Ui) {
    let on = !ui.settings.labs.enabled;
    // An installed Hex Microphone driver keeps the Voice Filter on.
    let keep_voice_filter = super::fun::driver_installed(ui);
    ui.update(|settings| {
        settings.labs.enabled = on;
        if !on && !keep_voice_filter {
            settings.experimental.hex_microphone = false;
        }
    });
}

/// Runs the first run again (`onboarding_kit::live::restart`).
fn run_setup_row(p: Paint) -> CardRow {
    let run = rune_kit::Button::new(tr("Run"))
        .variant(rune_kit::ButtonVariant::Ghost)
        .control_size(rune_kit::ControlSize::Small)
        .on_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            if let Err(error) = super::onboarding_kit::live::restart(ui, tx) {
                ui.error = Some(error);
            }
        })
        .build(p.k)
        .key("restart-onboarding");
    icon_row(
        p,
        "lab-restart-onboarding",
        &SPARKLES,
        tr("Run setup again"),
        Some(tr(
            "Play the first run again. Settings, models, and permissions stay",
        )),
        run,
    )
    .into()
}

fn open_pane_button(p: Paint, pane: Pane) -> El {
    rune_kit::Button::new(tr("Open"))
        .variant(rune_kit::ButtonVariant::Ghost)
        .control_size(rune_kit::ControlSize::Small)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
        .build(p.k)
        .key("open")
}

/// A feature's row: pressing it flips the feature; once on, Open jumps to
/// its pane.
fn lab_row(p: Paint, lab: &Lab) -> CardRow {
    let c = &p.k.c;
    let pane = lab.pane;
    let open = (lab.on || lab.open_when_off).then(|| open_pane_button(p, pane));
    let trailing = row()
        .key("trailing")
        .gap(8.0)
        .center_y()
        .children(open)
        .child(row_switch(p, lab.on));
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

/// One feature Labs turns on, listed while Labs is on.
struct Feature {
    key: &'static str,
    icon: &'static Icon,
    title: &'static str,
    about: &'static str,
    pane: Option<Pane>,
    /// The feature's own switch, off by default, for one with side effects
    /// and no pane of its own: whether it is on, and what flips it.
    switch: Option<FeatureSwitch>,
}

/// Whether a feature's own switch is on, and what flips it.
type FeatureSwitch = (bool, fn(&mut Ui));

fn features(ui: &Ui) -> Vec<Feature> {
    let caps = ui.platform.caps;
    let mut items = Vec::new();
    if caps.fun {
        items.push(Feature {
            key: "exp-voice-filter",
            icon: &VOICE_FILTER_ICON,
            title: tr("Voice Filter"),
            about: tr("Hex Microphone virtual input and real-time voice filters"),
            pane: Some(Pane::VoiceFilter).filter(|pane| pane.shown(ui)),
            switch: None,
        });
    }
    items.push(Feature {
        key: "exp-live-text",
        icon: &WAVE,
        title: tr("Live text"),
        about: tr("Show provisional words beside the HUD while dictating (applies on restart)"),
        pane: None,
        switch: Some((ui.settings.labs.live_text, |ui| {
            let on = !ui.settings.labs.live_text;
            ui.update(|settings| settings.labs.live_text = on);
        })),
    });
    if caps.meetings {
        items.push(Feature {
            key: "exp-live-chapters",
            icon: &HINTS,
            title: tr("Live chapters"),
            about: tr("Break meeting transcripts into chapters while recording"),
            pane: None,
            switch: Some((ui.settings.labs.live_chapters, |ui| {
                let on = !ui.settings.labs.live_chapters;
                ui.update(|settings| settings.labs.live_chapters = on);
            })),
        });
    }
    items.push(Feature {
        key: "exp-playground",
        icon: &SPARKLES,
        title: "Playground",
        about: tr("Scene and story browser in the window (applies on restart)"),
        pane: None,
        switch: None,
    });
    items
}

fn feature_row(p: Paint, feature: Feature) -> CardRow {
    let open = feature.pane.map(|pane| open_pane_button(p, pane));
    let trailing = row()
        .key("trailing")
        .gap(8.0)
        .center_y()
        .children(open)
        .children(feature.switch.map(|(on, _)| row_switch(p, on)));
    let row = icon_row(
        p,
        feature.key,
        feature.icon,
        feature.title,
        Some(feature.about),
        trailing,
    );
    match feature.switch {
        Some((_, flip)) => pressable_row(row)
            .hover(p.k.c.hover)
            .on_press(move |ui: &mut Ui| flip(ui))
            .into(),
        None => row.into(),
    }
}
