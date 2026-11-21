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

/// The window, in points: the island with two displays and long
/// translations, and room for its shadow.
pub(crate) const SIZE: (f32, f32) = (1000.0, 76.0);
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
    body = body.child(close(p)).child(divider(p));
    for kind in RecordKind::ALL {
        body = body.child(kind_chip(p, kind, s.kind == kind));
    }
    body = body.child(divider(p));
    if s.displays.len() > 1
        && let Some(display) = s.chosen_display()
    {
        body = body.child(menu_chip(
            p,
            "display",
            &rune_theme_opencode::icons::MONITOR,
            &display.name,
            true,
            display_menu,
        ));
    }
    body.child(sound_chip(p, s))
        .child(menu_chip(
            p,
            "options",
            &rune_theme_opencode::icons::SLIDERS,
            tr("Options"),
            true,
            options_menu,
        ))
        .child(divider(p))
        .child(record(p))
        .role("group")
        .label(tr("Record screen"))
        .hit()
}

fn divider(p: Paint) -> El {
    el().size(surface::DIVIDER_W, 16.0)
        .mx(4.0)
        .fill(p.k.c.border_base)
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

fn chip(p: Paint, key: &'static str) -> El {
    row()
        .key(key)
        .h(CHIP_H)
        .px(CHIP_PX)
        .gap(SEG_ICON_GAP)
        .center_y()
        .radius(surface::CHIP_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
}

fn kind_chip(p: Paint, kind: RecordKind, on: bool) -> El {
    let mode = match kind {
        RecordKind::Area => Mode::Area,
        RecordKind::Window => Mode::Window,
        RecordKind::Screen => Mode::Screen,
    };
    let fg = if on { p.text() } else { p.muted() };
    let name = tr(kind.name());
    let chip = chip(p, kind.id())
        .child(mark(Id::Mode(mode), SEG_ICON, fg))
        .child(label(name, medium(), fg));
    let chip = if on { chip.fill(p.k.c.layer_02) } else { chip };
    chip.role("radio")
        .label(name)
        .checked(on)
        .hit()
        .on_press_tx(move |s: &mut Toolbar, _| s.act(ToolbarAction::Kind(kind)))
}

fn menu_chip(
    p: Paint,
    key: &'static str,
    glyph: &'static rune::Icon,
    text: &str,
    lit: bool,
    open: fn(&Toolbar, &mut Tx),
) -> El {
    let fg = if lit { p.text() } else { p.muted() };
    chip(p, key)
        .child(rune::icon(glyph, 16.0).color(fg))
        .child(label(&ellipsize_name(text), medium(), fg))
        .child(rune::icon(&rune_theme_opencode::icons::CHEVRON_DOWN, 12.0).color(p.muted()))
        .role("button")
        .label(text.to_owned())
        .hit()
        .on_press_tx(move |s: &mut Toolbar, tx| open(s, tx))
}

/// What the recording hears: the microphone's name with its glyph, and a
/// waveform when it keeps the computer's audio too.
fn sound_chip(p: Paint, s: &Toolbar) -> El {
    let microphone = s.microphone != RECORD_MICROPHONE_NONE;
    let (text, name) = if microphone {
        let mut name = s.microphone_label().to_owned();
        if s.system_audio {
            name = format!("{name}, {}", tr("Computer audio"));
        }
        (s.microphone_label(), name)
    } else if s.system_audio {
        (tr("Computer audio"), tr("Computer audio").to_owned())
    } else {
        (tr("No microphone"), tr("No microphone").to_owned())
    };
    let fg = if microphone || s.system_audio {
        p.text()
    } else {
        p.muted()
    };
    let mut chip = chip(p, "audio");
    if microphone || !s.system_audio {
        chip = chip.child(rune::icon(&rune_kit::glyphs::MIC, 16.0).color(fg));
    }
    chip = chip.child(label(&ellipsize_name(text), medium(), fg));
    if s.system_audio {
        chip = chip.child(rune::icon(&rune_theme_opencode::icons::WAVEFORM, 16.0).color(fg));
    }
    chip.child(rune::icon(&rune_theme_opencode::icons::CHEVRON_DOWN, 12.0).color(p.muted()))
        .role("button")
        .label(name)
        .hit()
        .on_press_tx(|s: &mut Toolbar, tx| audio_menu(s, tx))
}

/// A long device or display name, cut so the island keeps its width.
fn ellipsize_name(text: &str) -> String {
    const MOST: usize = 24;
    if text.chars().count() <= MOST {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MOST - 1).collect();
    cut.push('…');
    cut
}

fn record(p: Paint) -> El {
    let c = &p.k.c;
    let name = tr("Record");
    row()
        .key("record")
        .h(CHIP_H)
        .px(CHIP_PX)
        .center_y()
        .radius(surface::CHIP_RADIUS)
        .fill(c.bg_contrast)
        .hover(c.contrast_hover)
        .press(c.contrast_pressed)
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
        assert_eq!(ellipsize_name(&"x".repeat(40)).chars().count(), 24);
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
                rt.settle_all();
                rt.frame(0.0);
                let (w, h) = ((SIZE.0 * 2.0) as u32, (SIZE.1 * 2.0) as u32);
                let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
                let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
                renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
                renderer.prepare(
                    &gpu.device,
                    &gpu.queue,
                    &mut rt.text.atlas,
                    &rt.instances,
                    (w, h),
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                renderer.draw(&mut encoder, &target.view, rt.clear_color());
                target.copy(&mut encoder);
                gpu.queue.submit([encoder.finish()]);
                let pixels = target.read(&gpu.device);
                let path = folder.join(format!("toolbar-{name}-{theme}.png"));
                rune::render::write_png(&path, w, h, &pixels).unwrap();
            }
        }
    }
}
