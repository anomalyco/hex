//! Labs: one flat sidebar item listing every Labs feature (`crate::labs`)
//! as a row with a toggle, each off by default, plus Send feedback and Run
//! setup again. A feature with its own pane (Meetings, Voice Filter,
//! Playground) joins the main sidebar while on and is hidden while off.

use rune::{El, Icon, Tx, row, text};

use super::controls::{self, CardRow, Paint, card, icon_row, pressable_row, row_switch};
use super::icons::{BOOK, HINTS, SPARKLES, TERMINAL, WAVE};
use super::{NAV_ROW_GAP, NAV_ROW_HEIGHT, Pane, Ui, VOICE_FILTER_ICON};
use crate::desktop_i18n::tr;
use crate::labs::Feature;

pub(super) static LABS_ICON: Icon = Icon {
    name: "hex-labs",
    view: 16.0,
    body: r#"<path d="M6.25 2.5h3.5M6.75 2.5v4L3.2 12.3a.8.8 0 0 0 .7 1.2h8.2a.8.8 0 0 0 .7-1.2L9.25 6.5v-4M4.8 10h6.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// The main sidebar's panes, in order: General, Output, History, then the
/// features turned on (Meetings and Voice Filter from Labs, Annotate in dev
/// builds).
pub(super) fn main_panes(ui: &Ui) -> Vec<Pane> {
    let mut panes = vec![Pane::General, Pane::Output, Pane::History];
    #[cfg(hex_capture)]
    let optional = [Pane::Meetings, Pane::Annotate, Pane::VoiceFilter];
    #[cfg(not(hex_capture))]
    let optional = [Pane::Meetings, Pane::VoiceFilter];
    panes.extend(optional.into_iter().filter(|pane| pane.shown(ui)));
    panes
}

/// Every pane the sidebar can open, in its order, for Command-1 through 9.
pub(super) fn order(ui: &Ui) -> Vec<Pane> {
    main_panes(ui)
}

/// A sidebar item that isn't a pane: Labs, or the playground.
fn nav_row(p: Paint, key: &'static str, icon: &'static Icon, label: &'static str, on: bool) -> El {
    let c = &p.k.c;
    let mut item = row()
        .key(key)
        .role("button")
        .label(label)
        .h(NAV_ROW_HEIGHT)
        .px(6.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .child(rune::icon(icon, 16.0).key("icon").color(if on {
            c.icon_base
        } else {
            c.icon_muted
        }))
        .child(text(label).key("label").font(p.k.ty.ui).color(if on {
            c.text_base
        } else {
            c.text_muted
        }));
    if on {
        item = item.fill(c.layer_02);
    }
    row()
        .key(format!("{key}-slot"))
        .pt(NAV_ROW_GAP / 2.0)
        .pb(NAV_ROW_GAP / 2.0)
        .child(item.grow(1.0))
}

/// The Playground item while the playground is on: it opens the playground.
pub(super) fn playground_nav(p: Paint) -> Option<El> {
    super::playground::attached().then(|| {
        nav_row(p, "nav-playground", &SPARKLES, "Playground", false)
            .on_press_tx(|_: &mut Ui, tx: &mut Tx| tx.open_playground())
    })
}

/// The Labs item: opens the Labs page, or goes back to General.
pub(super) fn nav(ui: &Ui, p: Paint) -> El {
    let on = ui.labs_open;
    nav_row(p, "nav-labs", &LABS_ICON, tr("Labs"), on).on_press_tx(
        move |ui: &mut Ui, tx: &mut Tx| {
            if on {
                ui.select_pane(Pane::General, tx);
            } else {
                open(ui, tx);
            }
        },
    )
}

fn open(ui: &mut Ui, tx: &mut Tx) {
    ui.select_pane(Pane::General, tx);
    ui.labs_open = true;
    tx.settle();
}

/// One Labs row.
struct Row {
    feature: Feature,
    icon: &'static Icon,
    title: &'static str,
    about: &'static str,
    on: bool,
    /// Shown on and can't be turned off (an installed Hex Microphone driver).
    locked: bool,
}

fn rows(ui: &Ui) -> Vec<Row> {
    let caps = ui.platform.caps;
    let settings = &ui.settings;
    let row = |feature: Feature, icon, title, about| Row {
        feature,
        icon,
        title,
        about,
        on: feature.on(settings),
        locked: false,
    };
    let mut rows = Vec::new();
    if caps.meetings {
        rows.push(row(
            Feature::Meetings,
            Pane::Meetings.icon(),
            tr("Meetings (beta)"),
            tr("Record calls and keep transcripts"),
        ));
    }
    if caps.fun {
        let locked = super::fun::driver_installed(ui);
        rows.push(Row {
            on: super::fun::running(ui),
            locked,
            about: if locked {
                tr("On while the Hex Microphone driver is installed. Uninstall the driver in Voice Filter to turn it off.")
            } else {
                tr("Hex Microphone virtual input and real-time voice filters")
            },
            ..row(Feature::VoiceFilter, &VOICE_FILTER_ICON, tr("Voice Filter"), "")
        });
    }
    rows.push(row(
        Feature::Playground,
        &SPARKLES,
        "Playground",
        tr("Scene and story browser in the window (applies on restart)"),
    ));
    rows.push(row(
        Feature::LiveText,
        &WAVE,
        tr("Live text"),
        tr("Show provisional words beside the HUD while dictating (applies on restart)"),
    ));
    if caps.meetings {
        rows.push(row(
            Feature::LiveChapters,
            &HINTS,
            tr("Live chapters"),
            tr("Break meeting transcripts into chapters while recording"),
        ));
    }
    rows.push(row(
        Feature::VocabularyBiasing,
        &BOOK,
        tr("Vocabulary biasing"),
        tr("Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way"),
    ));
    if caps.os == super::platform::Os::Windows {
        rows.push(row(
            Feature::TerminalPaste,
            &TERMINAL,
            tr("Terminal paste"),
            tr("Paste with Ctrl+Shift+V so terminals accept it"),
        ));
    }
    rows
}

fn row_key(feature: Feature) -> &'static str {
    match feature {
        Feature::Meetings => "lab-meetings",
        Feature::VoiceFilter => "lab-voice-filter",
        Feature::Playground => "lab-playground",
        Feature::LiveText => "lab-live-text",
        Feature::LiveChapters => "lab-live-chapters",
        Feature::VocabularyBiasing => "lab-vocabulary-biasing",
        Feature::TerminalPaste => "lab-terminal-paste",
    }
}

fn flip(ui: &mut Ui, feature: Feature) {
    if feature == Feature::VoiceFilter && super::fun::driver_installed(ui) {
        return;
    }
    let on = !feature.on(&ui.settings);
    ui.update(|settings| feature.set(settings, on));
}

/// The Labs page: what Labs is with Send feedback, every feature's row,
/// then Run setup again.
pub(super) fn page(ui: &Ui, p: Paint) -> El {
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
    let lede = row()
        .key("lede-row")
        .gap(8.0)
        .center_y()
        .mx(controls::METRICS.row_px)
        .child(
            text(tr("Try unfinished features. They may change or break."))
                .key("lede")
                .font(p.k.ty.ui)
                .color(p.muted())
                .wrap()
                .grow(1.0),
        )
        .child(feedback);
    let rows: Vec<CardRow> = rows(ui)
        .into_iter()
        .map(|row| feature_row(p, row))
        .collect();
    let content = controls::page("content")
        .child(lede)
        .child(card(p, "labs", rows))
        .child(card(p, "labs-setup", vec![run_setup_row(p)]));
    super::scroll_body("scroll", content)
}

fn feature_row(p: Paint, row: Row) -> CardRow {
    let switch = if row.locked {
        rune_kit::Switch::new(true)
            .disabled(true)
            .build(p.k)
            .key("switch")
            .focusable(false)
    } else {
        row_switch(p, row.on)
    };
    let item = icon_row(
        p,
        row_key(row.feature),
        row.icon,
        row.title,
        Some(row.about),
        switch,
    );
    if row.locked {
        return item.into();
    }
    let feature = row.feature;
    pressable_row(item)
        .hover(p.k.c.hover)
        .on_press(move |ui: &mut Ui| flip(ui, feature))
        .into()
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
