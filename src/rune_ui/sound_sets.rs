//! Sounds: a sheet from General's Sounds row where each sound set can be
//! heard in context. A stage at the top plays a short dictation (start, a
//! moment of speaking, stop, a moment of transcribing, done) while a small
//! capsule shows each moment as its sound plays, and its chips play one
//! sound at a time. Below it each set has its name, what it sounds like and
//! a play button; picking one is the setting, and plays it.
//!
//! The capsule runs on the window's poll clock while a sound plays. Any
//! playback stops the one before it, and closing the sheet stops it.

use std::cell::Cell;
use std::time::Duration;

use rune::{Cursor, Cx, El, Icon, Transition, Tx, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, modal_scrim_transition, modal_transition, status_dot,
};
use super::lists::{list_row, source_list};
use crate::desktop_i18n::tr;
use crate::feedback::{self, Audition, Cue, SoundSet};

const MODAL_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;
const CAPSULE_WIDTH: f32 = 92.0;
const CAPSULE_HEIGHT: f32 = 28.0;
const FRAME: Duration = Duration::from_micros(16_667);

static PLAY: Icon = Icon {
    name: "hex-play",
    view: 20.0,
    body: r#"<path d="M6.5 5.1v9.8c0 .5.5.8.9.5l7.6-4.9c.4-.2.4-.8 0-1L7.4 4.6c-.4-.3-.9 0-.9.5Z" fill="currentColor"/>"#,
};

#[derive(Default)]
pub(super) struct SoundSets {
    pub(super) open: bool,
    playing: Option<Playing>,
    /// The poll clock when the playing sounds' frames started.
    start: Option<f64>,
    elapsed_ms: f64,
    /// The poll clock when the next frame's wake is due.
    next_frame: f64,
    /// A fixed moment for snapshots.
    held: bool,
    /// Reduce Motion, as the last view saw it: the capsule's level holds
    /// still, while its moments still follow the sounds.
    reduced: Cell<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Playing {
    set: SoundSet,
    what: Audition,
}

impl SoundSets {
    /// What plays now.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn playing(&self) -> Option<(SoundSet, Audition)> {
        self.playing.map(|playing| (playing.set, playing.what))
    }

    /// The cue whose moment shows now, while something plays.
    fn sounding(&self) -> Option<Cue> {
        let playing = self.playing?;
        sounding(playing.what, self.elapsed_ms)
    }
}

/// The cue whose moment shows `ms` into `what`, until it ends.
fn sounding(what: Audition, ms: f64) -> Option<Cue> {
    if ms >= what.length().as_secs_f64() * 1000.0 {
        return None;
    }
    what.cues()
        .iter()
        .rev()
        .find(|(_, at)| at.as_secs_f64() * 1000.0 <= ms)
        .map(|&(cue, _)| cue)
}

/// Opens the sheet over General, alone: it ends a shortcut recording and
/// closes the model picker, the shortcut explainer and any menu.
pub(super) fn open(ui: &mut Ui) {
    super::settings::cancel_capture(ui);
    super::picker::close(ui);
    super::explainer::close(ui);
    ui.open = None;
    ui.sound_sets.open = true;
    ui.sound_sets.playing = None;
}

/// Closes the sheet, stopping whatever it plays.
pub(super) fn close(ui: &mut Ui) {
    if ui.sound_sets.playing.take().is_some() && ui.live() {
        feedback::stop_audition();
    }
    ui.sound_sets.open = false;
}

/// Opens the sheet, held `ms` into `set`'s `what` when given.
pub(super) fn fixture(ui: &mut Ui, playing: Option<(SoundSet, Audition, f64)>) {
    open(ui);
    if let Some((set, what, ms)) = playing {
        ui.sound_sets.playing = Some(Playing { set, what });
        ui.sound_sets.elapsed_ms = ms;
        ui.sound_sets.held = true;
    }
}

fn play(ui: &mut Ui, set: SoundSet, what: Audition) {
    let sheet = &mut ui.sound_sets;
    sheet.playing = Some(Playing { set, what });
    sheet.start = None;
    sheet.elapsed_ms = 0.0;
    sheet.next_frame = 0.0;
    sheet.held = false;
    if ui.live() {
        feedback::audition(set, what);
    }
}

/// Stops `set` when it plays, else plays its dictation.
fn toggle(ui: &mut Ui, set: SoundSet) {
    if ui
        .sound_sets
        .playing
        .is_some_and(|playing| playing.set == set)
    {
        ui.sound_sets.playing = None;
        if ui.live() {
            feedback::stop_audition();
        }
    } else {
        play(ui, set, Audition::Dictation);
    }
}

/// Saves `set` as the sound set and plays its dictation.
fn choose(ui: &mut Ui, set: SoundSet) {
    if ui.settings.sound_set != set && !ui.update(|settings| settings.sound_set = set) {
        return;
    }
    play(ui, set, Audition::Dictation);
}

/// Advances the capsule while something plays. True when it moved.
pub(super) fn poll(ui: &mut Ui, clock: f64, visible: bool, tx: &mut Tx) -> bool {
    let sheet = &mut ui.sound_sets;
    let Some(playing) = sheet.playing else {
        return false;
    };
    if !sheet.open || sheet.held {
        return false;
    }
    let start = *sheet.start.get_or_insert(clock);
    sheet.elapsed_ms = (clock - start) * 1000.0;
    if sounding(playing.what, sheet.elapsed_ms).is_none() {
        sheet.playing = None;
        return true;
    }
    if visible && clock >= sheet.next_frame {
        sheet.next_frame = clock + FRAME.as_secs_f64();
        tx.wake_after(FRAME);
    }
    true
}

/// The set the stage plays: the one playing, else the saved one.
fn staged(ui: &Ui) -> SoundSet {
    ui.sound_sets
        .playing
        .map_or(ui.settings.sound_set, |playing| playing.set)
}

/// A stable key for `set`'s row.
fn set_key(set: SoundSet) -> String {
    format!("set-{set:?}").to_lowercase()
}

pub(super) fn modal(ui: &Ui, cx: &Cx<Ui>, p: Paint) -> Option<[El; 2]> {
    // Kept while closed too, so the first frame after opening already knows.
    ui.sound_sets.reduced.set(cx.system_reduced_motion());
    if !ui.sound_sets.open {
        return None;
    }
    let c = &p.k.c;
    let saved = ui.settings.sound_set;
    let playing = ui.sound_sets.playing;
    let sets =
        source_list(col().key("sets").gap(2.0)).children(SoundSet::ALL.into_iter().map(|set| {
            let sounding = playing.is_some_and(|playing| playing.set == set);
            list_row(
                p,
                &set_key(set),
                tr(set.name()),
                vec![play_button(p, set, sounding)],
                tr(set.character()),
                set == saved,
                move |ui: &mut Ui| choose(ui, set),
            )
        }));
    let mut footer = row().key("footer").gap(12.0).center_y();
    footer = footer.child(
        // Nothing plays with Sound volume off; the capsule still shows.
        text(if ui.settings.sound_effects {
            ""
        } else {
            tr("Sound volume is off")
        })
        .key("muted")
        .font(DESCRIPTION)
        .color(p.muted())
        .grow(1.0)
        .shrink(),
    );
    footer = footer.child(
        button(
            p,
            "close-sound-sets",
            tr("Close"),
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
        .child(stage(ui, p))
        .child(sets.mx(-6.0))
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

fn play_button(p: Paint, set: SoundSet, sounding: bool) -> El {
    let (icon, label) = if sounding {
        (&rune_theme_opencode::icons::STOP, tr("Stop"))
    } else {
        (&PLAY, tr("Play"))
    };
    IconButton::new(icon)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Regular)
        .label(label)
        .on_press(move |ui: &mut Ui| toggle(ui, set))
        .build(p.k)
        .key("play")
}

/// The capsule over the staged set's sounds, one chip each.
fn stage(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let set = staged(ui);
    let sounding = ui.sound_sets.sounding();
    let chips = Cue::ALL
        .into_iter()
        .filter(|&cue| cue != Cue::Error || ui.platform.caps.error_sound)
        .map(|cue| chip(p, set, cue, sounding == Some(cue)));
    col()
        .key("stage")
        .padding(12.0)
        .gap(12.0)
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .child(
            row()
                .key("screen")
                .h(CAPSULE_HEIGHT + 16.0)
                .center()
                .child(capsule(ui, p, sounding)),
        )
        .child(
            row()
                .key("cues")
                .gap(6.0)
                .center_y()
                .child(
                    text(tr(set.name()))
                        .key("set")
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .children(chips),
        )
}

/// One sound of `set`, played alone. Inverted while its moment shows, as
/// the capsule is; faint and inert when the set has no such sound.
fn chip(p: Paint, set: SoundSet, cue: Cue, lit: bool) -> El {
    let c = &p.k.c;
    let label = tr(cue.name());
    let has = set.has(cue);
    let ink = if lit {
        c.text_inverse
    } else if has {
        p.text()
    } else {
        p.faint()
    };
    let mut chip = row()
        .key(cue.name())
        .role("button")
        .label(label)
        .h(24.0)
        .px(9.0)
        .center()
        .radius(p.k.radius.sm)
        .border(p.hair, if lit { c.bg_inverse } else { c.border_weak })
        .child(
            text(label)
                .key("label")
                .font(p.k.ty.small_medium)
                .color(ink),
        );
    if lit {
        chip = chip.fill(c.bg_inverse);
    } else if has {
        chip = chip.hover(c.hover).press(c.pressed);
    }
    if has {
        chip = chip
            .focusable(true)
            .focus_ring(2.0, 2.0, c.border_focus)
            .cursor(Cursor::Pointer)
            .on_press(move |ui: &mut Ui| play(ui, set, Audition::Cue(cue)));
    }
    chip
}

/// A small stand-in for the dictation HUD: an empty slot at rest, then the
/// moment the sound marks.
fn capsule(ui: &Ui, p: Paint, sounding: Option<Cue>) -> El {
    let c = &p.k.c;
    let pill = row()
        .key("capsule")
        .role("none")
        .w(CAPSULE_WIDTH)
        .h(CAPSULE_HEIGHT)
        .radius(CAPSULE_HEIGHT / 2.0)
        .center()
        .gap(7.0);
    let Some(cue) = sounding else {
        return pill.border(p.hair, c.border_weak);
    };
    let seconds = ui.sound_sets.elapsed_ms / 1000.0;
    let still = ui.sound_sets.reduced.get();
    let fades = || Transition::opacity();
    let moment = match cue {
        Cue::Start => row()
            .key("recording")
            .gap(7.0)
            .center_y()
            .child(status_dot(p.danger()))
            .child(levels(p, seconds, still)),
        Cue::Stop => thinking(p, seconds, still),
        Cue::Done => glyph(p, "done", p.k.glyph.check_small, c.text_inverse),
        Cue::Cancel => glyph(p, "cancelled", p.k.glyph.close, c.text_inverse),
        Cue::Error => glyph(
            p,
            "failed",
            &rune_theme_opencode::icons::WARNING,
            p.danger(),
        ),
    };
    pill.fill(c.bg_inverse).child(moment.transition(fades()))
}

fn glyph(_: Paint, key: &str, icon: &'static Icon, color: rune::Color) -> El {
    row()
        .key(key)
        .center()
        .child(rune::icon(icon, 16.0).key("icon").color(color))
}

/// Five bars moving as a voice would, still under Reduce Motion.
fn levels(p: Paint, seconds: f64, still: bool) -> El {
    const STILL: [f64; 5] = [0.4, 0.8, 0.55, 0.9, 0.5];
    row()
        .key("levels")
        .gap(2.5)
        .center_y()
        .children((0..5).map(|bar| {
            let level = if still {
                STILL[bar]
            } else {
                let phase = bar as f64;
                ((seconds * 7.3 + phase * 1.9).sin() * 0.6
                    + (seconds * 13.1 + phase * 0.7).sin() * 0.4)
                    .abs()
            };
            el().key(format!("bar{bar}"))
                .role("none")
                .w(3.0)
                .h(4.0 + 10.0 * level as f32)
                .radius(1.5)
                .fill(p.k.c.text_inverse)
        }))
}

/// Three dots lighting in turn while Hex transcribes, held under Reduce
/// Motion.
fn thinking(p: Paint, seconds: f64, still: bool) -> El {
    row()
        .key("transcribing")
        .gap(4.0)
        .center_y()
        .children((0..3).map(|dot| {
            let lit = if still {
                1.0
            } else {
                let wave = ((seconds * 6.0 - dot as f64 * 0.9).sin() + 1.0) / 2.0;
                0.35 + 0.65 * wave as f32
            };
            let ink = p.k.c.text_inverse;
            el().key(format!("dot{dot}"))
                .role("none")
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(ink.alpha(ink.a * lit))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_moment_shows_from_its_sound_to_the_next() {
        let at = |ms| sounding(Audition::Dictation, ms);
        assert_eq!(at(0.0), Some(Cue::Start));
        assert_eq!(at(1_399.0), Some(Cue::Start));
        assert_eq!(at(1_400.0), Some(Cue::Stop));
        assert_eq!(at(2_100.0), Some(Cue::Done));
        assert_eq!(at(2_899.0), Some(Cue::Done));
        assert_eq!(at(2_900.0), None);
        for cue in Cue::ALL {
            let alone = Audition::Cue(cue);
            assert_eq!(sounding(alone, 0.0), Some(cue));
            assert_eq!(sounding(alone, alone.length().as_secs_f64() * 1000.0), None);
        }
    }
}
