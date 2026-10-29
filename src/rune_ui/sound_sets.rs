//! Sounds: a sheet from General's Sounds row where each sound set can be
//! heard in context. A stage at the top plays a short dictation (start, a
//! moment of speaking, stop, a moment of transcribing, done) while Hex's own
//! HUD, where this OS puts it, does what it does at each sound; its chips
//! play one sound at a time. Below it each set has its name, what it sounds
//! like and a play button; picking one is the setting, and plays it.
//!
//! The HUD runs on the window's poll clock while a sound plays and until it
//! leaves. Any playback stops the one before it, and closing the sheet stops
//! it.

mod hud;

use std::time::Duration;

use rune::{Cursor, El, Icon, Tx, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use self::hud::Hud;
pub(crate) use self::hud::Preview;
use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, Paint, button, modal_scrim_transition, modal_transition,
};
use super::lists::{list_row, source_list};
use super::platform::Os;
use crate::app_settings::IndicatorPosition;
use crate::desktop_i18n::tr;
use crate::feedback::{self, Audition, Cue, SoundSet};
use crate::rune_hud::{BOTTOM_OFFSET, TOP_OFFSET};

const MODAL_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;
/// The slice of screen the stage shows.
const SCREEN_HEIGHT: f32 = 84.0;
const MENU_BAR: f32 = 24.0;
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
    /// The HUD while something plays and until it leaves.
    hud: Option<Hud>,
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
    ui.sound_sets.hud = None;
}

/// Closes the sheet, stopping whatever it plays.
pub(super) fn close(ui: &mut Ui) {
    if ui.sound_sets.playing.take().is_some() && ui.live() {
        feedback::stop_audition();
    }
    ui.sound_sets.hud = None;
    ui.sound_sets.open = false;
}

/// Opens the sheet, held `ms` into `set`'s `what` when given.
pub(super) fn fixture(ui: &mut Ui, playing: Option<(SoundSet, Audition, f64)>) {
    open(ui);
    if let Some((set, what, ms)) = playing {
        let mut hud = Hud::new(ui.platform.hud.tuning, what);
        hud.advance_to((ms / 1000.0) as f32);
        ui.sound_sets.playing = Some(Playing { set, what });
        ui.sound_sets.hud = Some(hud);
        ui.sound_sets.elapsed_ms = ms;
        ui.sound_sets.held = true;
    }
}

fn play(ui: &mut Ui, set: SoundSet, what: Audition) {
    let hud = Hud::new(ui.platform.hud.tuning, what);
    let sheet = &mut ui.sound_sets;
    sheet.playing = Some(Playing { set, what });
    sheet.hud = Some(hud);
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
        ui.sound_sets.hud = None;
        if ui.live() {
            feedback::stop_audition();
        }
    } else {
        play(ui, set, Audition::Dictation);
    }
}

/// Saves `set` as the sound set and plays its dictation.
fn choose(ui: &mut Ui, set: SoundSet) {
    if ui.settings.sound_theme != set && !ui.update(|settings| settings.sound_theme = set) {
        return;
    }
    play(ui, set, Audition::Dictation);
}

/// Advances the HUD while something plays and until it leaves. True when
/// it moved.
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
    if let Some(hud) = &mut sheet.hud {
        hud.advance_to((sheet.elapsed_ms / 1000.0) as f32);
    }
    if sounding(playing.what, sheet.elapsed_ms).is_none()
        && sheet.hud.as_ref().is_none_or(Hud::settled)
    {
        sheet.playing = None;
        sheet.hud = None;
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
        .map_or(ui.settings.sound_theme, |playing| playing.set)
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
    let saved = ui.settings.sound_theme;
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
        // Nothing plays with Sound volume off; the HUD still shows.
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

/// The HUD over the staged set's sounds, one chip each.
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
        .child(screen(ui, p))
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

/// One sound of `set`, played alone. Inverted while its moment shows; faint
/// and inert when the set has no such sound.
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

/// A slice of the screen with Hex's HUD where this OS puts it: on a Mac its
/// plate 6 pt under the menu bar or 24 pt above the Dock, on Linux and
/// Windows its window 12 pt from the top or 24 pt from the bottom. Off shows
/// nothing. The window's compositor draws the HUD itself.
fn screen(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let mac = ui.platform.caps.os == Os::Macos;
    let radius = p.k.radius.md;
    let mut screen = el()
        .key(hud::SCREEN_KEY)
        .role("none")
        .w_full()
        .h(SCREEN_HEIGHT)
        .radius(radius)
        .clip_rounded(radius)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak);
    if mac {
        screen = screen.child(
            el().key("menu-bar")
                .role("none")
                .absolute(0.0, 0.0)
                .w_full()
                .h(MENU_BAR)
                .fill(c.layer_02),
        );
    }
    let position = ui.settings.indicator_position;
    let Some(hud) = &ui.sound_sets.hud else {
        return screen;
    };
    if position == IndicatorPosition::Hidden {
        return screen;
    }
    let (hud, height, inset) = hud.view();
    let y = match (position, mac) {
        (IndicatorPosition::Bottom, true) => SCREEN_HEIGHT - 24.0 - height + inset,
        (IndicatorPosition::Bottom, false) => SCREEN_HEIGHT - BOTTOM_OFFSET - height,
        (_, true) => MENU_BAR + crate::hud_placement::TOP_OFFSET as f32 - inset,
        (_, false) => TOP_OFFSET,
    };
    screen.child(hud.absolute(0.0, y))
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
