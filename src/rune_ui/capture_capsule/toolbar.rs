//! Labs › Screenshots' recording toolbar (macOS), as macOS's own ⌘⇧5 bar:
//! what to record (an area, a window, or a whole display), which display,
//! the sound a recording keeps, its settings, and Record. It rests at the
//! bottom of the display under the pointer, never takes focus, and is left
//! out of every capture; Hex's session tap gives it Escape and Return. What
//! a choice does is the controller's ([`crate::screenshots`]).

use super::*;
use crate::app_settings::{
    RECORD_MICROPHONE_DEFAULT, RECORD_MICROPHONE_NONE, RecordKind, RecordingQuality,
};
use rune::{PopupItem, PopupMenu};

/// The window, in points: the island, its Record label in the longest
/// translation, and room for its shadow.
pub(crate) const SIZE: (f32, f32) = (480.0, 76.0);
/// How far above the display's bottom edge it rests, clear of the Dock.
pub(crate) const MARGIN: f32 = 64.0;

/// One display the toolbar can record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolbarDisplay {
    /// `CGDirectDisplayID`.
    pub(crate) id: u32,
    pub(crate) name: String,
    /// A point on it, in global points.
    pub(crate) at: [f64; 2],
}

/// A choice made on the toolbar.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ToolbarAction {
    Kind(RecordKind),
    Display(usize),
    Microphone(String),
    SystemAudio(bool),
    OneX(bool),
    Fps(u32),
    Quality(RecordingQuality),
    Record,
    Close,
}

/// What the toolbar shows: the choices last made, the displays, and the
/// microphones connected when it opened.
#[derive(Clone, Debug, Default)]
pub(crate) struct Toolbar {
    pub(crate) kind: RecordKind,
    pub(crate) displays: Vec<ToolbarDisplay>,
    pub(crate) display: usize,
    pub(crate) microphone: String,
    pub(crate) microphones: Vec<String>,
    pub(crate) system_audio: bool,
    pub(crate) one_x: bool,
    pub(crate) fps: u32,
    pub(crate) quality: RecordingQuality,
    /// The controller, told of every choice after the toolbar shows it.
    pub(crate) act: Option<fn(&Toolbar, ToolbarAction)>,
}

impl Toolbar {
    pub(crate) fn act(&mut self, action: ToolbarAction) {
        match &action {
            ToolbarAction::Kind(kind) => self.kind = *kind,
            ToolbarAction::Display(index) => self.display = *index,
            ToolbarAction::Microphone(name) => self.microphone.clone_from(name),
            ToolbarAction::SystemAudio(on) => self.system_audio = *on,
            ToolbarAction::OneX(on) => self.one_x = *on,
            ToolbarAction::Fps(fps) => self.fps = *fps,
            ToolbarAction::Quality(quality) => self.quality = *quality,
            ToolbarAction::Record | ToolbarAction::Close => {}
        }
        if let Some(act) = self.act {
            act(self, action);
        }
    }

    /// The display a full-screen recording takes.
    pub(crate) fn chosen_display(&self) -> Option<&ToolbarDisplay> {
        self.displays.get(self.display)
    }

    /// The microphone's line on its button: none, dictation's, or a name.
    fn microphone_label(&self) -> &str {
        match self.microphone.as_str() {
            RECORD_MICROPHONE_NONE => tr("No microphone"),
            RECORD_MICROPHONE_DEFAULT => tr("Microphone"),
            name => name,
        }
    }
}

/// Adds the toolbar, hidden.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> OverlayHandle<Toolbar> {
    let spec = rune::OverlaySpec {
        name: "Hex recording toolbar",
        size: SIZE,
        edge: rune::ScreenEdge::Bottom,
        margin: MARGIN,
        align: rune::Align::Center,
        inset: 0.0,
        interactive: true,
        view,
    }
    .options(rune::OverlayOptions {
        display: rune::DisplayTarget::UnderPointer,
        exclude_from_capture: true,
        input: rune::OverlayInput::Regions,
        ..Default::default()
    })
    .animate_reveal();
    app.overlay(spec, Toolbar::default())
}

/// Saves a toolbar choice off the main thread.
pub(crate) fn remember(
    what: &'static str,
    change: impl FnOnce(&mut crate::app_settings::ScreenshotSettings) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("hex-record-choice".into())
        .spawn(move || {
            if let Err(error) =
                crate::rune_ui::host::save_settings(|settings| change(&mut settings.screenshots))
            {
                tracing::warn!(%error, what, "could not save a recording choice");
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, what, "could not save a recording choice");
    }
}

pub(crate) fn view(s: &Toolbar, cx: &mut Cx<Toolbar>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let (w, h) = SIZE;
    row()
        .key("record-toolbar")
        .size(w, h)
        .center()
        .child(island(p, s))
}

fn island(p: Paint, s: &Toolbar) -> El {
    let k = p.k;
    let mut body = row()
        .key("island")
        .h(surface::ISLAND_H)
        .px(surface::ISLAND_INSET)
        .gap(2.0)
        .center_y()
        .radius(surface::SHELL_RADIUS)
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_weak);
    for shadow in &k.elevation.overlay {
        body = body.box_shadow(*shadow);
    }
    body.child(close(p))
        .child(kinds(p, s).mx(GROUP_GAP))
        .child(sound(p, s))
        .child(options(p))
        .child(record(p).ml(GROUP_GAP))
        .role("group")
        .label(tr("Record screen"))
        .hit()
}

/// Space between the toolbar's groups, which have no lines between them.
const GROUP_GAP: f32 = 6.0;
/// A segment of the kind control and the icon buttons, in points.
const SEGMENT: (f32, f32) = (32.0, 24.0);
const TRACK_PAD: f32 = 2.0;
const TRACK_RADIUS: f32 = 8.0;
const SEGMENT_RADIUS: f32 = TRACK_RADIUS - TRACK_PAD;
const CHEVRON: f32 = 10.0;

/// Area, Window, and Full screen as one icon-only segmented control: a
/// recessed track with the chosen segment raised, its name in the tooltip.
/// With more than one display, Full screen carries a chevron that picks
/// which, as does a second click on it.
fn kinds(p: Paint, s: &Toolbar) -> El {
    let c = &p.k.c;
    let mut track = row()
        .key("kinds")
        .h(SEGMENT.1 + 2.0 * TRACK_PAD)
        .padding(TRACK_PAD)
        .gap(0.0)
        .center_y()
        .radius(TRACK_RADIUS)
        .fill(c.layer_01)
        .box_shadow(rune::Shadow {
            spread: 0.5,
            color: c.border_base,
            ..Default::default()
        })
        .role("radiogroup")
        .label(tr("Record"));
    for kind in RecordKind::ALL {
        let on = s.kind == kind;
        let mut segment = row()
            .key(kind.id())
            .h(SEGMENT.1)
            .center_y()
            .radius(SEGMENT_RADIUS);
        segment = segment.child(kind_segment(p, s, kind, on));
        if on && kind == RecordKind::Screen && s.displays.len() > 1 {
            segment = segment.child(display_chevron(p, s));
        }
        if on {
            segment = segment.fill(c.bg_base).box_shadow(rune::Shadow {
                spread: 0.5,
                color: c.border_strong,
                ..Default::default()
            });
        }
        track = track.child(segment);
    }
    track
}

fn kind_segment(p: Paint, s: &Toolbar, kind: RecordKind, on: bool) -> El {
    let mode = match kind {
        RecordKind::Area => Mode::Area,
        RecordKind::Window => Mode::Window,
        RecordKind::Screen => Mode::Screen,
    };
    let fg = if on { p.text() } else { p.muted() };
    let name = tr(kind.name());
    let mut segment = row()
        .key("segment")
        .size(SEGMENT.0, SEGMENT.1)
        .center()
        .radius(SEGMENT_RADIUS)
        .child(mark(Id::Mode(mode), SEG_ICON, fg));
    if !on {
        segment = segment.hover(p.k.c.hover).press(p.k.c.pressed);
    }
    let many = kind == RecordKind::Screen && s.displays.len() > 1;
    segment
        .role("radio")
        .label(name)
        .help(name)
        .checked(on)
        .hit()
        .on_press_tx(move |s: &mut Toolbar, tx| {
            if many && s.kind == RecordKind::Screen {
                display_menu(s, tx);
            } else {
                s.act(ToolbarAction::Kind(kind));
            }
        })
}

/// Which display Full screen records, beside its segment.
fn display_chevron(p: Paint, s: &Toolbar) -> El {
    let name = s
        .chosen_display()
        .map(|display| display.name.clone())
        .unwrap_or_default();
    row()
        .key("display")
        .size(14.0, SEGMENT.1)
        .pr(4.0)
        .center()
        .radius(SEGMENT_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .child(chevron(p))
        .role("button")
        .label(name.clone())
        .help(name)
        .hit()
        .on_press_tx(|s: &mut Toolbar, tx| display_menu(s, tx))
}

fn chevron(p: Paint) -> El {
    rune::icon(&rune_theme_opencode::icons::CHEVRON_DOWN, CHEVRON).color(p.muted())
}

/// An icon button with a chevron: its menu holds the words.
fn menu_button(p: Paint, key: &'static str, name: String) -> El {
    row()
        .key(key)
        .h(BUTTON_H)
        .px(6.0)
        .gap(3.0)
        .center_y()
        .radius(view::ICON_BUTTON_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .role("button")
        .label(name.clone())
        .help(name)
        .hit()
}

/// What the recording hears: a microphone, crossed out when it keeps none,
/// and a waveform beside it when it keeps the computer's audio too. The
/// device's name lives in the menu and the tooltip.
fn sound(p: Paint, s: &Toolbar) -> El {
    let microphone = s.microphone != RECORD_MICROPHONE_NONE;
    let name = match (microphone, s.system_audio) {
        (true, true) => format!("{}, {}", s.microphone_label(), tr("Computer audio")),
        (true, false) => s.microphone_label().to_owned(),
        (false, true) => tr("Computer audio").to_owned(),
        (false, false) => tr("No microphone").to_owned(),
    };
    let fg = if microphone || s.system_audio {
        p.text()
    } else {
        p.muted()
    };
    let glyph = if microphone {
        &crate::rune_ui::icons::MIC
    } else {
        &crate::rune_ui::icons::MIC_OFF
    };
    let mut button = menu_button(p, "audio", name).child(rune::icon(glyph, 16.0).color(fg));
    if s.system_audio {
        button = button.child(rune::icon(&rune_theme_opencode::icons::WAVEFORM, 14.0).color(fg));
    }
    button
        .child(chevron(p))
        .on_press_tx(|s: &mut Toolbar, tx| audio_menu(s, tx))
}

fn options(p: Paint) -> El {
    menu_button(p, "options", tr("Options").to_owned())
        .child(rune::icon(&rune_theme_opencode::icons::SLIDERS, 16.0).color(p.text()))
        .child(chevron(p))
        .on_press_tx(|s: &mut Toolbar, tx| options_menu(s, tx))
}

fn close(p: Paint) -> El {
    button_box(p, BUTTON_H)
        .key("close")
        .child(close_glyph(p))
        .role("button")
        .label(tr("Close"))
        .help(tr("Close"))
        .hit()
        .on_press_tx(|s: &mut Toolbar, _| s.act(ToolbarAction::Close))
}

fn record(p: Paint) -> El {
    let c = &p.k.c;
    let name = tr("Record");
    const DOT: f32 = 7.0;
    row()
        .key("record")
        .h(BUTTON_H)
        .pl(10.0)
        .pr(12.0)
        .gap(6.0)
        .center_y()
        .radius(view::ICON_BUTTON_RADIUS)
        .fill(c.bg_contrast)
        .hover(c.contrast_hover)
        .press(c.contrast_pressed)
        .child(el().size(DOT, DOT).radius(DOT * 0.5).fill(p.danger()))
        .child(label(name, medium(), c.text_contrast))
        .role("button")
        .label(name)
        .hit()
        .on_press_tx(|s: &mut Toolbar, _| s.act(ToolbarAction::Record))
}

fn display_menu(s: &Toolbar, tx: &mut Tx) {
    let names = s
        .displays
        .iter()
        .enumerate()
        .map(|(index, display)| (index.to_string(), display.name.clone()));
    let menu = PopupMenu::picker(names, &s.display.to_string());
    tx.popup_menu(menu, "display", |s: &mut Toolbar, picked, _| {
        if let Some(index) = picked.and_then(|picked| picked.parse().ok()) {
            s.act(ToolbarAction::Display(index));
        }
    });
}

/// Menu ids for the microphone choices; a device's is `DEVICE` + its name.
const NONE: &str = "none";
const DICTATION: &str = "dictation";
const DEVICE: &str = "device:";
const COMPUTER: &str = "computer";

fn audio_menu(s: &Toolbar, tx: &mut Tx) {
    let chosen = s.microphone.as_str();
    let mut menu = PopupMenu::new()
        .item(PopupItem::new("microphone", tr("Microphone")).enabled(false))
        .item(PopupItem::new(NONE, tr("None")).checked(chosen == RECORD_MICROPHONE_NONE))
        .item(
            PopupItem::new(DICTATION, tr("Same as dictation"))
                .checked(chosen == RECORD_MICROPHONE_DEFAULT),
        );
    let mut names = s.microphones.clone();
    // A chosen microphone that's unplugged still shows, checked.
    if chosen != RECORD_MICROPHONE_NONE
        && chosen != RECORD_MICROPHONE_DEFAULT
        && !names.iter().any(|name| name == chosen)
    {
        names.push(chosen.to_owned());
    }
    for name in names {
        let checked = name == chosen;
        menu = menu.item(PopupItem::new(format!("{DEVICE}{name}"), name).checked(checked));
    }
    let menu = menu
        .separator()
        .item(PopupItem::new(COMPUTER, tr("Record computer audio")).checked(s.system_audio));
    tx.popup_menu(menu, "audio", |s: &mut Toolbar, picked, _| {
        let action = match picked {
            Some(COMPUTER) => ToolbarAction::SystemAudio(!s.system_audio),
            Some(NONE) => ToolbarAction::Microphone(RECORD_MICROPHONE_NONE.into()),
            Some(DICTATION) => ToolbarAction::Microphone(RECORD_MICROPHONE_DEFAULT.into()),
            Some(id) => match id.strip_prefix(DEVICE) {
                Some(name) => ToolbarAction::Microphone(name.to_owned()),
                None => return,
            },
            None => return,
        };
        s.act(action);
    });
}

fn options_menu(s: &Toolbar, tx: &mut Tx) {
    let mut menu = PopupMenu::new()
        .item(PopupItem::new("one-x", tr("Scale Retina to 1x")).checked(s.one_x))
        .separator()
        .item(PopupItem::new("fps", tr("Frame rate")).enabled(false));
    for fps in [30, 60] {
        let name = if fps == 30 {
            tr("30 fps")
        } else {
            tr("60 fps")
        };
        menu = menu.item(PopupItem::new(format!("fps:{fps}"), name).checked(s.fps == fps));
    }
    menu = menu
        .separator()
        .item(PopupItem::new("quality", tr("Quality")).enabled(false));
    for (index, quality) in RecordingQuality::ALL.into_iter().enumerate() {
        menu = menu.item(
            PopupItem::new(format!("quality:{index}"), tr(quality.name()))
                .checked(s.quality == quality),
        );
    }
    tx.popup_menu(menu, "options", |s: &mut Toolbar, picked, _| {
        let Some(picked) = picked else { return };
        let action = if picked == "one-x" {
            ToolbarAction::OneX(!s.one_x)
        } else if let Some(fps) = picked.strip_prefix("fps:").and_then(|f| f.parse().ok()) {
            ToolbarAction::Fps(fps)
        } else if let Some(quality) = picked
            .strip_prefix("quality:")
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| RecordingQuality::ALL.get(i))
        {
            ToolbarAction::Quality(*quality)
        } else {
            return;
        };
        s.act(action);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_shows_before_the_controller_hears_it() {
        use std::sync::Mutex;
        static HEARD: Mutex<Vec<(RecordKind, ToolbarAction)>> = Mutex::new(Vec::new());
        fn act(toolbar: &Toolbar, action: ToolbarAction) {
            HEARD.lock().unwrap().push((toolbar.kind, action));
        }
        let mut toolbar = Toolbar {
            act: Some(act),
            ..Toolbar::default()
        };
        toolbar.act(ToolbarAction::Kind(RecordKind::Screen));
        toolbar.act(ToolbarAction::Record);
        assert_eq!(
            *HEARD.lock().unwrap(),
            [
                (RecordKind::Screen, ToolbarAction::Kind(RecordKind::Screen)),
                (RecordKind::Screen, ToolbarAction::Record),
            ]
        );
    }

    #[test]
    fn the_sound_button_names_what_it_keeps() {
        let mut toolbar = Toolbar {
            microphone: RECORD_MICROPHONE_NONE.into(),
            ..Toolbar::default()
        };
        assert_eq!(toolbar.microphone_label(), "No microphone");
        toolbar.microphone = "Studio Display Microphone".into();
        assert_eq!(toolbar.microphone_label(), "Studio Display Microphone");
    }

    /// `HEX_TOOLBAR_RENDERS=<folder>` draws the toolbar offscreen, light and
    /// dark, with one display and with two. No golden checks it.
    #[test]
    #[ignore = "writes stills only when asked"]
    fn renders_the_toolbar() {
        let Some(folder) = std::env::var_os("HEX_TOOLBAR_RENDERS") else {
            return;
        };
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let Ok(gpu) = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None) else {
            eprintln!("skipping the toolbar stills: no GPU adapter");
            return;
        };
        let display = |id, name: &str| ToolbarDisplay {
            id,
            name: name.into(),
            at: [0.0, 0.0],
        };
        let one = Toolbar {
            kind: RecordKind::Area,
            displays: vec![display(1, "Built-in Retina Display")],
            microphone: RECORD_MICROPHONE_DEFAULT.into(),
            fps: 30,
            one_x: true,
            ..Toolbar::default()
        };
        let two = Toolbar {
            kind: RecordKind::Screen,
            displays: vec![
                display(1, "Built-in Retina Display"),
                display(2, "Studio Display"),
            ],
            display: 1,
            microphone: "MacBook Pro Microphone".into(),
            system_audio: true,
            ..one.clone()
        };
        let silent = Toolbar {
            kind: RecordKind::Window,
            microphone: RECORD_MICROPHONE_NONE.into(),
            ..one.clone()
        };
        for (name, toolbar) in [
            ("one-display", one),
            ("two-displays", two),
            ("silent", silent),
        ] {
            for dark in [false, true] {
                let theme = if dark { "dark" } else { "light" };
                let mut rt = crate::rune_ui::offscreen::runtime(
                    toolbar.clone(),
                    view,
                    SIZE,
                    rune::Env {
                        system_dark: dark,
                        scale: 2.0,
                    },
                    rune::text::TextSystem::new(
                        rune_theme_opencode::FONTS,
                        rune_theme_opencode::FAMILIES,
                    ),
                );
                let (w, h) = ((SIZE.0 * 2.0) as u32, (SIZE.1 * 2.0) as u32);
                let pixels = crate::rune_ui::offscreen::draw(&gpu, &mut rt, w, h);
                let path = folder.join(format!("toolbar-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &pixels).unwrap();
            }
        }
    }
}
