//! Labs: the opt-in features (Meetings beta on macOS, plus Annotate in dev
//! builds) and the experiments in dev, local, and test builds, folded under one
//! sidebar row. Production builds (`!dev_build`) expose only Meetings (beta)
//! on macOS and hide Annotate and every experiment.

use rune::{El, Icon, Tx, col, row, text};

use super::controls::{self, CardRow, Paint, card, icon_row, pressable_row, row_switch};
use super::icons::{HINTS, SPARKLES, WAVE};
use super::{NAV_ROW_GAP, NAV_ROW_HEIGHT, Pane, Ui, VOICE_FILTER_ICON};
use crate::desktop_i18n::tr;
use crate::experiments::Experiment;

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

/// Whether the sidebar shows Labs: dev builds always have experiments;
/// production builds show Labs only on OSes with a release opt-in (Meetings on
/// macOS).
pub(super) fn shown(ui: &Ui) -> bool {
    ui.platform.dev_build || !labs(ui).is_empty()
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

/// The experiments' panes this install has: listed under Labs, never in the
/// main sidebar. Empty in production builds.
pub(super) fn experiments(ui: &Ui) -> Vec<Pane> {
    if !ui.platform.dev_build {
        return Vec::new();
    }
    Pane::ALL
        .iter()
        .copied()
        .filter(|pane| pane.shown(ui) && pane.experimental())
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

/// The Labs page: the opt-in features with their switches, then interactive
/// toggles for every experiment available on this OS.
pub(super) fn page(ui: &Ui, p: Paint) -> El {
    let rows: Vec<CardRow> = labs(ui).iter().map(|lab| lab_row(p, lab)).collect();
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
    let exp_rows = experiment_toggles(ui)
        .into_iter()
        .map(|toggle| experiment_row(p, toggle))
        .collect::<Vec<_>>();
    if !exp_rows.is_empty() {
        content = content.child(controls::section(
            p,
            "experiments",
            tr("Experiments"),
            exp_rows,
        ));
    }
    if super::onboarding_kit::live::restart_unavailable(ui).is_none() {
        content = content.child(controls::section(
            p,
            "onboarding",
            "Onboarding",
            vec![restart_onboarding_row(p)],
        ));
    }
    super::scroll_body("scroll", content)
}

/// Replays the first run on this install, for trying it again (dev, test,
/// and local builds).
fn restart_onboarding_row(p: Paint) -> CardRow {
    let restart = rune_kit::Button::new("Restart")
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
        "Restart onboarding",
        Some("Play the first run again. Settings, models, and permissions stay"),
        restart,
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

struct ExperimentToggle {
    key: &'static str,
    experiment: Experiment,
    icon: &'static Icon,
    title: &'static str,
    about: &'static str,
    on: bool,
    pane: Option<Pane>,
}

fn experiment_on(ui: &Ui, experiment: Experiment) -> bool {
    match experiment {
        Experiment::VoiceFilter => ui.settings.experimental.voice_filter_enabled(
            ui.platform.voice_filter,
            ui.platform
                .hex_microphone
                .as_ref()
                .is_some_and(|status| status.driver_bundle_installed),
        ),
        Experiment::Playground => ui.platform.playground,
        other => ui.platform.experiments[other.index()],
    }
}

fn experiment_toggles(ui: &Ui) -> Vec<ExperimentToggle> {
    if !ui.platform.dev_build {
        return Vec::new();
    }
    let caps = ui.platform.caps;
    let mut items = Vec::new();
    if caps.fun {
        items.push(ExperimentToggle {
            key: "exp-voice-filter",
            experiment: Experiment::VoiceFilter,
            icon: &VOICE_FILTER_ICON,
            title: tr("Voice Filter"),
            about: tr("Hex Microphone virtual input and real-time voice filters"),
            on: experiment_on(ui, Experiment::VoiceFilter),
            pane: Some(Pane::VoiceFilter),
        });
    }
    items.push(ExperimentToggle {
        key: "exp-live-text",
        experiment: Experiment::LiveText,
        icon: &WAVE,
        title: tr("Live text"),
        about: tr("Show provisional words beside the HUD while dictating (applies on restart)"),
        on: experiment_on(ui, Experiment::LiveText),
        pane: None,
    });
    if caps.meetings {
        items.push(ExperimentToggle {
            key: "exp-live-chapters",
            experiment: Experiment::LiveChapters,
            icon: &HINTS,
            title: tr("Live chapters"),
            about: tr("Break meeting transcripts into chapters while recording"),
            on: experiment_on(ui, Experiment::LiveChapters),
            pane: None,
        });
    }
    items.push(ExperimentToggle {
        key: "exp-playground",
        experiment: Experiment::Playground,
        icon: &SPARKLES,
        title: "Playground",
        about: tr("Scene and story browser in the window (applies on restart)"),
        on: experiment_on(ui, Experiment::Playground),
        pane: None,
    });
    items
}

fn flip_experiment(ui: &mut Ui, experiment: Experiment) {
    let on = !experiment_on(ui, experiment);
    if ui.live()
        && let Err(error) = crate::experiments::set_enabled(experiment, on)
    {
        ui.error = Some(format!("{error:#}"));
        return;
    }
    ui.platform.experiments[experiment.index()] = on;
    match experiment {
        Experiment::VoiceFilter => {
            ui.platform.voice_filter = on;
            if !on && ui.settings.experimental.hex_microphone {
                ui.update(|settings| settings.experimental.hex_microphone = false);
            } else if ui.live() {
                ui.settings.refresh_runtime();
            }
        }
        Experiment::Playground => {
            ui.platform.playground = on;
        }
        Experiment::LiveChapters | Experiment::LiveText | Experiment::NewOnboarding => {}
    }
}

fn experiment_row(p: Paint, toggle: ExperimentToggle) -> CardRow {
    let c = &p.k.c;
    let open = toggle
        .pane
        .filter(|pane| toggle.on && *pane != Pane::General)
        .map(|pane| open_pane_button(p, pane));
    let trailing = row()
        .key("trailing")
        .gap(8.0)
        .center_y()
        .children(open)
        .child(row_switch(p, toggle.on));
    let experiment = toggle.experiment;
    pressable_row(icon_row(
        p,
        toggle.key,
        toggle.icon,
        toggle.title,
        Some(toggle.about),
        trailing,
    ))
    .hover(c.hover)
    .on_press(move |ui: &mut Ui| flip_experiment(ui, experiment))
    .into()
}
