//! Labs: one flat sidebar item listing every Labs feature (`crate::labs`)
//! as a row with a toggle, each off by default, plus Send feedback and Run
//! setup again. A feature with its own pane (Meetings, Voice Filter,
//! Playground) joins the main sidebar while on and is hidden while off.

use rune::{El, Icon, Tx, row, text};

use super::controls::{self, CardRow, Paint, card, icon_row, pressable_row, row_switch};
use super::icons::{BOOK, GLOBE, HINTS, KEYBOARD, MOUSE, SPARKLES, TERMINAL, TIMER, WAVE};
use super::{NAV_ROW_GAP, NAV_ROW_HEIGHT, Pane, Ui, VOICE_FILTER_ICON};
use crate::desktop_i18n::{tr, tr_fill};
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

/// The Playground item while Labs › Playground is on: it opens it.
pub(super) fn playground_nav(ui: &Ui, p: Paint) -> Option<El> {
    ui.settings.labs.playground.then(|| {
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
        tr("Scene and story browser in the window"),
    ));
    rows.push(row(
        Feature::LiveText,
        &WAVE,
        tr("Live text"),
        tr("Show provisional words beside the HUD while dictating"),
    ));
    if caps.meetings {
        rows.push(row(
            Feature::LiveChapters,
            &HINTS,
            tr("Live chapters"),
            tr("Break meeting transcripts into chapters while recording"),
        ));
        rows.push(row(
            Feature::SubtitleExport,
            &HINTS,
            tr("Subtitle export"),
            tr("Export meeting transcripts as SRT or VTT subtitles with hex meetings show"),
        ));
    }
    rows.push(row(
        Feature::VocabularyBiasing,
        &BOOK,
        tr("Vocabulary biasing"),
        tr("Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way"),
    ));
    if caps.keyboard_model_switching {
        rows.push(row(
            Feature::KeyboardModelSwitching,
            &KEYBOARD,
            tr("Switch model with keyboard language"),
            tr("Changing input source selects its mapped model and language; manual choices still work"),
        ));
    }
    if caps.os == super::platform::Os::Macos {
        rows.push(row(
            Feature::MouseShortcuts,
            &MOUSE,
            tr("Mouse shortcuts"),
            tr("Record mouse buttons as shortcuts"),
        ));
        rows.push(row(
            Feature::PauseBrowser,
            &GLOBE,
            tr("Also pause the browser"),
            tr("While dictating with Pause media: YouTube and other tabs, resumed only if Hex paused them"),
        ));
        rows.push(row(
            Feature::KeepOnClipboard,
            &HINTS,
            tr("Keep dictation on clipboard"),
            tr("Leave the dictation on the clipboard after pasting instead of restoring what was there"),
        ));
        rows.push(row(
            Feature::CopyOnly,
            &HINTS,
            tr("Copy dictation only"),
            tr("Put the dictation on the clipboard instead of pasting it"),
        ));
        // macOS's listener only, so Linux renders of macOS scenes leave it out.
        if cfg!(target_os = "macos") {
            rows.push(row(
                Feature::SecondDictationShortcut,
                &KEYBOARD,
                tr("Second dictation shortcut"),
                tr("Another shortcut that dictates like the main one, such as a key on an external keyboard"),
            ));
            // Reads the text before the cursor through macOS Accessibility.
            rows.push(row(
                Feature::SmartSpacing,
                &HINTS,
                tr("Smart spacing"),
                tr("Add a space before a dictation pasted right after a word"),
            ));
        }
    }
    rows.push(row(
        Feature::UnloadIdle,
        &TIMER,
        tr("Unload model when idle"),
        tr("Frees the speech model's memory after a while unused; the next dictation reloads it"),
    ));
    rows.push(row(
        Feature::RestoreShortcutDefault,
        &KEYBOARD,
        tr("Restore shortcut default"),
        tr("Offer Restore default next to Paste last dictation's shortcut"),
    ));
    rows.push(row(
        Feature::SpokenFormatting,
        &HINTS,
        tr("Spoken formatting"),
        tr("Say \"new line\", \"new paragraph\", \"open quote\", or \"close quote\" to insert line breaks and quotation marks"),
    ));
    rows.push(row(
        Feature::SpokenSymbols,
        &HINTS,
        tr("Spoken symbols"),
        tr("Say \"comma\", \"colon\", \"hashtag\", \"underscore\", or \"slash\" to insert the symbol joined to its words"),
    ));
    rows.push(row(
        Feature::FillerWords,
        &HINTS,
        tr("Remove filler words"),
        tr("Remove filler words such as um and uh from dictations, without an AI model"),
    ));
    if caps.share_shortcut {
        rows.push(row(
            Feature::ShareShortcut,
            &KEYBOARD,
            tr("Share dictation shortcut with other apps"),
            tr("Lets apps such as Discord detect the same key"),
        ));
    }
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
        Feature::KeyboardModelSwitching => "lab-keyboard-model-switching",
        Feature::MouseShortcuts => "lab-mouse-shortcuts",
        Feature::PauseBrowser => "lab-pause-browser",
        Feature::ShareShortcut => "lab-share-shortcut",
        Feature::UnloadIdle => "lab-unload-idle",
        Feature::RestoreShortcutDefault => "lab-restore-shortcut-default",
        Feature::KeepOnClipboard => "lab-keep-on-clipboard",
        Feature::CopyOnly => "lab-copy-only",
        Feature::SpokenFormatting => "lab-spoken-formatting",
        Feature::FillerWords => "lab-filler-words",
        Feature::SubtitleExport => "lab-subtitle-export",
        Feature::SecondDictationShortcut => "lab-second-dictation-shortcut",
        Feature::SmartSpacing => "lab-smart-spacing",
        Feature::SpokenSymbols => "lab-spoken-symbols",
    }
}

fn flip(ui: &mut Ui, feature: Feature) {
    if feature == Feature::VoiceFilter && super::fun::driver_installed(ui) {
        return;
    }
    let on = !feature.on(&ui.settings);
    ui.update(|settings| feature.set(settings, on));
    match feature {
        Feature::KeyboardModelSwitching => ui.open = None,
        // Hex hears the browser through System Audio Recording.
        Feature::PauseBrowser if on && !ui.platform.system_audio => {
            ui.ask(super::platform::PlatformRequest::SystemAudio);
        }
        _ => {}
    }
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
    let mut cards: Vec<CardRow> = Vec::new();
    for row in rows(ui) {
        let unload_idle = row.feature == Feature::UnloadIdle && row.on;
        cards.push(feature_row(p, row));
        if unload_idle {
            cards.push(unload_after_row(ui, p));
        }
    }
    let rows = cards;
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

/// How long the model sits unused before it unloads, under its row while on.
fn unload_after_row(ui: &Ui, p: Paint) -> CardRow {
    use crate::app_settings::UNLOAD_IDLE_MINUTES;
    let labels: Vec<String> = UNLOAD_IDLE_MINUTES
        .iter()
        .map(|minutes| tr_fill("{} min", &minutes.to_string()))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let selected = UNLOAD_IDLE_MINUTES
        .iter()
        .position(|minutes| *minutes == ui.settings.labs.unload_idle_minutes)
        .unwrap_or(1);
    controls::setting_row(
        p,
        "lab-unload-idle-minutes",
        tr("Unload after"),
        None,
        controls::segmented(p, "unload-idle-minutes", &labels, selected, |ui, index| {
            let minutes = UNLOAD_IDLE_MINUTES[index];
            ui.update(|settings| settings.labs.unload_idle_minutes = minutes);
        }),
    )
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
