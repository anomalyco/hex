//! Sounds: a sheet from General's Sounds row. The volume every sound plays
//! at sits on top, and its left end turns the sounds off. Below it each set
//! has its name, what it sounds like and a play button, and a check marks
//! the chosen one; picking one is the setting, and plays it.
//!
//! Any playback stops the one before it, and closing the sheet stops it.

use std::time::Duration;

use rune::{El, Tx, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::controls::{ButtonKind, Paint, button, modal_sheet};
use super::lists::{inline_slider, list_row, source_list};
use super::settings::sound_volume;
use super::{Modal, Ui};
use crate::desktop::i18n::tr;
use crate::feedback::{self, Audition, SoundSet};

const MODAL_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;
/// A set row's text inset, bled into the panel's padding so the check and
/// the volume's label share the title's edge.
const ROW_BLEED: f32 = 10.0;
const SLIDER_WIDTH: f32 = 200.0;
/// The column the chosen set's check sits in.
const CHECK_WIDTH: f32 = 16.0;

#[derive(Default)]
pub(super) struct SoundSets {
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
    ui.open_modal(Modal::SoundSets);
    ui.sound_sets.playing = None;
    ui.sound_sets.volume_drag = None;
}

/// Closes the sheet, stopping whatever it plays.
pub(super) fn close(ui: &mut Ui) {
    ui.close_modal_if(Modal::SoundSets);
}

pub(super) fn on_close(ui: &mut Ui) {
    if ui.sound_sets.playing.take().is_some() && ui.live() {
        feedback::stop_audition();
    }
    ui.sound_sets.volume_drag = None;
}

/// Opens the sheet, held while `playing` plays when given.
pub(super) fn fixture(ui: &mut Ui, playing: Option<SoundSet>) {
    open(ui);
    ui.sound_sets.playing = playing;
    ui.sound_sets.held = true;
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

/// Saves `set` as the sound set and plays its dictation.
fn choose(ui: &mut Ui, set: SoundSet) {
    if ui.settings.sound_theme != set && !ui.update(|settings| settings.sound_theme = set) {
        return;
    }
    play(ui, set);
}

/// Follows the slider in steps of 5% and saves where it lets go. Zero turns
/// the sounds off and keeps the last volume, as
/// `hex settings set sound_effect_volume 0` does; any other volume plays the
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
    if !ui.is_modal(Modal::SoundSets) {
        return false;
    }
    let sheet = &mut ui.sound_sets;
    if sheet.held || sheet.playing.is_none() {
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

pub(super) fn modal(ui: &Ui, p: Paint) -> [El; 2] {
    let c = &p.k.c;
    let volume = ui
        .sound_sets
        .volume_drag
        .unwrap_or_else(|| sound_volume(&ui.settings));
    let volume_row = row()
        .key("volume")
        .gap(12.0)
        .center_y()
        .child(
            text(tr("Sound volume"))
                .key("label")
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .grow(1.0)
                .shrink()
                .truncate()
                // The slider reads its name.
                .a11y_hidden(),
        )
        .child(inline_slider(
            p,
            "sound-volume-slider",
            tr("Sound volume"),
            volume,
            if volume > 0.0 {
                format!("{:.0}%", volume * 100.0)
            } else {
                tr("Off").to_owned()
            },
            SLIDER_WIDTH,
            set_volume,
        ));
    let saved = ui.settings.sound_theme;
    let playing = ui.sound_sets.playing;
    let sets =
        source_list(col().key("sets").gap(2.0)).children(SoundSet::ALL.into_iter().map(|set| {
            list_row(
                p,
                &set_key(set),
                Some(check(p, set == saved)),
                tr(set.name()),
                vec![play_button(p, set, playing == Some(set))],
                tr(set.character()),
                set == saved,
                move |ui: &mut Ui| choose(ui, set),
            )
        }));
    let footer = row().key("footer").justify_end().child(
        button(
            p,
            "close-sound-sets",
            tr("Done"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| close(ui),
        )
        .default_button(),
    );
    let panel = col()
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
        .child(volume_row)
        .child(sets.mx(-ROW_BLEED))
        .child(footer);
    modal_sheet(p, "sound-sets", panel, close)
}

/// The chosen set's check, or the empty column that keeps every name on
/// one edge.
fn check(p: Paint, chosen: bool) -> El {
    el().key("check")
        .w(CHECK_WIDTH)
        .center()
        .a11y_hidden()
        .children(chosen.then(|| {
            rune::icon(&rune_theme_opencode::icons::CHECK_SMALL, CHECK_WIDTH)
                .key("mark")
                .color(p.text())
        }))
}

fn play_button(p: Paint, set: SoundSet, sounding: bool) -> El {
    let (icon, label) = if sounding {
        (&rune_theme_opencode::icons::STOP, tr("Stop"))
    } else {
        (&super::controls::PLAY, tr("Play"))
    };
    IconButton::new(icon)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Regular)
        .label(label)
        .on_press(move |ui: &mut Ui| toggle(ui, set))
        .build(p.k)
        .key("play")
}
