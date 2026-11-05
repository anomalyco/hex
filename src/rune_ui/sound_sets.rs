//! Sounds: a sheet from General's Sounds row. The volume every sound plays
//! at sits on top, and dragging it to the left turns the sounds off. Below
//! it each set has its name, what it sounds like and a play button, and a
//! check marks the chosen one; picking one is the setting, and plays it.
//!
//! Any playback stops the one before it, and closing the sheet stops it.

use std::time::Duration;

use rune::{Cursor, El, Icon, Tx, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::Ui;
use super::controls::{
    ButtonKind, METRICS, Paint, button, card, icon_row, modal_scrim_transition, modal_transition,
    section,
};
use super::icons::BELL;
use super::lists::{select_at_once, slider_track, source_item, source_list};
use crate::desktop::i18n::{tr, tr_fill};
use crate::feedback::{self, Audition, SoundSet};

const MODAL_WIDTH: f32 = 520.0;
const PADDING: f32 = 18.0;
const SLIDER_WIDTH: f32 = 160.0;
/// The column the chosen set's check sits in.
const CHECK_WIDTH: f32 = 16.0;

static PLAY: Icon = Icon {
    name: "hex-play",
    view: 20.0,
    body: r#"<path d="M6.5 5.1v9.8c0 .5.5.8.9.5l7.6-4.9c.4-.2.4-.8 0-1L7.4 4.6c-.4-.3-.9 0-.9.5Z" fill="currentColor"/>"#,
};

#[derive(Default)]
pub(super) struct SoundSets {
    pub(super) open: bool,
    /// The set whose dictation plays.
    playing: Option<SoundSet>,
    /// The poll clock when it started.
    start: Option<f64>,
    /// A fixed moment for snapshots.
    held: bool,
    /// The volume while the slider is dragged, saved when it lets go.
    volume_drag: Option<f32>,
}

impl SoundSets {
    /// What plays now.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn playing(&self) -> Option<SoundSet> {
        self.playing
    }
}

/// Opens the sheet over General, alone: it ends a shortcut recording and
/// closes the model picker, the shortcut explainer and any menu.
pub(super) fn open(ui: &mut Ui) {
    super::settings::cancel_capture(ui);
    super::picker::close(ui);
    super::explainer::close(ui);
    super::vocabulary::close(ui);
    ui.open = None;
    ui.sound_sets.open = true;
    ui.sound_sets.playing = None;
    ui.sound_sets.volume_drag = None;
}

/// Closes the sheet, stopping whatever it plays.
pub(super) fn close(ui: &mut Ui) {
    if ui.sound_sets.playing.take().is_some() && ui.live() {
        feedback::stop_audition();
    }
    ui.sound_sets.open = false;
}

/// Opens the sheet, held while `playing` plays when given.
pub(super) fn fixture(ui: &mut Ui, playing: Option<SoundSet>) {
    open(ui);
    ui.sound_sets.playing = playing;
    ui.sound_sets.held = true;
}

/// Stops `set` when it plays, else plays its dictation.
fn toggle(ui: &mut Ui, set: SoundSet) {
    if ui.sound_sets.playing == Some(set) {
        ui.sound_sets.playing = None;
        if ui.live() {
            feedback::stop_audition();
        }
    } else {
        play(ui, set);
    }
}

fn play(ui: &mut Ui, set: SoundSet) {
    let sheet = &mut ui.sound_sets;
    sheet.playing = Some(set);
    sheet.start = None;
    sheet.held = false;
    if ui.live() {
        feedback::audition(set, Audition::Dictation);
    }
}

/// Saves `set` as the sound set and plays its dictation.
fn choose(ui: &mut Ui, set: SoundSet) {
    if ui.settings.sound_theme != set && !ui.update(|settings| settings.sound_theme = set) {
        return;
    }
    play(ui, set);
}

/// The volume the slider shows: zero while the sounds are off.
fn shown_volume(ui: &Ui) -> f32 {
    ui.sound_sets
        .volume_drag
        .unwrap_or(if ui.settings.sound_effects {
            ui.settings.sound_effect_volume
        } else {
            0.0
        })
}

/// Follows the slider, in steps of 5%, and saves where it lets go: zero
/// turns the sounds off and keeps the last volume. A saved volume plays the
/// start sound at it.
fn set_volume(ui: &mut Ui, value: f32, done: bool) {
    let volume = (value * 20.0).round() / 20.0;
    ui.sound_sets.volume_drag = (!done).then_some(volume);
    if !done {
        return;
    }
    let saved = ui.update(|settings| {
        settings.sound_effects = volume > 0.0;
        if volume > 0.0 {
            settings.sound_effect_volume = volume;
        }
    });
    if saved && volume > 0.0 && ui.live() {
        feedback::play(feedback::Tone::DictationStart);
    }
}

/// Ends the play button's Stop once the dictation has played. True when it
/// ended.
pub(super) fn poll(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    let sheet = &mut ui.sound_sets;
    if !sheet.open || sheet.held || sheet.playing.is_none() {
        return false;
    }
    let start = *sheet.start.get_or_insert(clock);
    let left = Audition::Dictation.length().as_secs_f64() - (clock - start);
    if left <= 0.0 {
        sheet.playing = None;
        return true;
    }
    tx.wake_after(Duration::from_secs_f64(left));
    false
}

/// A stable key for `set`'s row.
fn set_key(set: SoundSet) -> String {
    format!("set-{set:?}").to_lowercase()
}

pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.sound_sets.open {
        return None;
    }
    let c = &p.k.c;
    let volume = shown_volume(ui);
    let percent = format!("{:.0}%", volume * 100.0);
    let line = if volume > 0.0 {
        tr_fill("{} · drag to the left to turn sounds off", &percent)
    } else {
        tr("Off · drag to the right to turn sounds on").to_string()
    };
    let volume_row = icon_row(
        p,
        "sound-volume",
        &BELL,
        tr("Volume"),
        Some(&line),
        slider_track(
            p,
            "sound-volume-slider",
            tr("Sound volume"),
            volume,
            percent,
            SLIDER_WIDTH,
            set_volume,
        ),
    );
    let sets = SoundSet::ALL
        .into_iter()
        .map(|set| set_row(ui, p, set))
        .collect::<Vec<_>>();
    let footer = row()
        .key("footer")
        .child(el().key("spacer").grow(1.0))
        .child(
            button(
                p,
                "close-sound-sets",
                tr("Done"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| close(ui),
            )
            .default_button(),
        );
    let mut panel = col()
        .key("sound-sets")
        .role("dialog")
        .label(tr("Sounds"))
        .w(MODAL_WIDTH)
        .pt(PADDING)
        .px(PADDING)
        .pb(14.0)
        .gap(14.0)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(
            text(tr("Sounds"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(card(p, "volume", vec![volume_row]))
        .child(source_list(section(p, "sets", tr("Sound set"), sets)))
        .child(footer);
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let layer = |key: &str| {
        el().key(key)
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
    };
    Some([
        layer("sound-sets-scrim")
            .fill(c.scrim)
            .on_press(close)
            .transition(modal_scrim_transition(p)),
        layer("sound-sets-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the row's button that opened it.
            .child(panel.modal(true, close)),
    ])
}

/// One set: a check when it is the chosen one, its name and what it sounds
/// like, and its play button. Pressing the row chooses it.
fn set_row(ui: &Ui, p: Paint, set: SoundSet) -> El {
    let c = &p.k.c;
    let chosen = ui.settings.sound_theme == set;
    let title = tr(set.name());
    let meta = tr(set.character());
    let mut item = row()
        .key(set_key(set))
        .role("listitem")
        .label(title)
        .description(meta)
        .px(METRICS.row_px)
        .py(10.0)
        .gap(10.0)
        .center_y()
        .hover(c.hover)
        .cursor(Cursor::Pointer)
        .child(
            el().key("check")
                .w(CHECK_WIDTH)
                .center()
                .children(chosen.then(|| {
                    rune::icon(&rune_theme_opencode::icons::CHECK, 16.0)
                        .key("mark")
                        .color(c.icon_accent)
                }))
                .a11y_hidden(),
        )
        .child(
            col()
                .key("copy")
                .gap(2.0)
                .grow(1.0)
                .shrink()
                .child(
                    text(title)
                        .key("title")
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .truncate(),
                )
                .child(
                    text(meta)
                        .key("meta")
                        .font(p.k.ty.small)
                        .color(p.muted())
                        .truncate(),
                )
                .a11y_hidden(),
        )
        .child(play_button(p, set, ui.sound_sets.playing == Some(set)));
    if chosen {
        item = item.fill(c.layer_01);
    }
    source_item(
        select_at_once(item, chosen, move |ui: &mut Ui| choose(ui, set)),
        chosen,
    )
}

fn play_button(p: Paint, set: SoundSet, sounding: bool) -> El {
    let (icon, label) = if sounding {
        (&rune_theme_opencode::icons::STOP, tr("Stop"))
    } else {
        (&PLAY, tr("Play"))
    };
    IconButton::new(icon)
        .variant(IconButtonVariant::Neutral)
        .control_size(ControlSize::Regular)
        .label(label)
        .on_press(move |ui: &mut Ui| toggle(ui, set))
        .build(p.k)
        .key("play")
        .radius(999.0)
}
