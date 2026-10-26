//! The Linux recording HUD: a focus-free "Recording"/"Transcribing" pill as
//! a Rune overlay in the Hex process. Rune makes it a layer-shell surface on
//! Wayland (overlay layer, no keyboard, empty input region) and an
//! override-redirect window with an empty input shape on X11, so it never
//! takes focus or a click.
//!
//! It only observes: the listener reports its capture and pending output
//! through `LinuxIndicator`, which never waits on the HUD. The listener can
//! start before the window's app exists, so reports are kept here and the
//! overlay shows the latest one once `install` adds it.

use std::sync::{Mutex, OnceLock};

use rune::{Availability, Cx, El, OverlayHandle, OverlaySpec, ScreenEdge, row, text};

use crate::app_settings::IndicatorPosition;

/// Distance between the HUD and the screen edge it sits on.
const EDGE_MARGIN: f32 = 16.0;
/// Wide enough for the longest translated label in the pill.
const SIZE: (f32, f32) = (280.0, 44.0);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum IndicatorState {
    #[default]
    Hidden,
    Recording,
    Processing,
}

impl IndicatorState {
    fn for_capture(recording: bool, pending: usize) -> Self {
        if recording {
            Self::Recording
        } else if pending > 0 {
            Self::Processing
        } else {
            Self::Hidden
        }
    }
}

/// A listener's handle on the HUD; dropping it hides what it reported.
pub(crate) struct LinuxIndicator {
    generation: Option<u64>,
}

impl LinuxIndicator {
    pub(crate) fn new(position: IndicatorPosition) -> Self {
        let generation = (position != IndicatorPosition::Hidden).then(|| acquire(position));
        Self { generation }
    }

    pub(crate) fn update(&self, recording: bool, pending: usize) {
        if let Some(generation) = self.generation {
            set(generation, IndicatorState::for_capture(recording, pending));
        }
    }
}

impl Drop for LinuxIndicator {
    fn drop(&mut self) {
        self.update(false, 0);
    }
}

#[derive(Default)]
struct Hud {
    state: IndicatorState,
}

/// The newest listener's report; an older listener's reports stop counting.
struct Slot {
    generation: u64,
    state: IndicatorState,
    position: IndicatorPosition,
    /// What the overlay was last asked to show.
    shown: Option<(IndicatorState, IndicatorPosition)>,
}

impl Slot {
    const fn new() -> Self {
        Self {
            generation: 0,
            state: IndicatorState::Hidden,
            position: IndicatorPosition::Top,
            shown: None,
        }
    }
}

static SLOT: Mutex<Slot> = Mutex::new(Slot::new());
static OVERLAY: OnceLock<OverlayHandle<Hud>> = OnceLock::new();

/// Adds the HUD overlay to the window's app and shows the latest report.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    let overlay = app.overlay(
        OverlaySpec {
            name: "hex-hud",
            size: SIZE,
            edge: ScreenEdge::Top,
            margin: EDGE_MARGIN,
            align: rune::Align::Center,
            inset: 0.0,
            interactive: false,
            view,
        },
        Hud::default(),
    );
    if OVERLAY.set(overlay).is_ok() {
        apply(&mut lock());
    }
}

fn lock() -> std::sync::MutexGuard<'static, Slot> {
    SLOT.lock().unwrap_or_else(|error| error.into_inner())
}

fn acquire(position: IndicatorPosition) -> u64 {
    let mut slot = lock();
    slot.position = position;
    slot.generation += 1;
    slot.state = IndicatorState::Hidden;
    slot.generation
}

fn set(generation: u64, state: IndicatorState) {
    let mut slot = lock();
    if slot.generation == generation {
        slot.state = state;
        apply(&mut slot);
    }
}

fn apply(slot: &mut Slot) {
    let Some(overlay) = OVERLAY.get() else {
        return;
    };
    let desired = (slot.state, slot.position);
    if slot.shown == Some(desired) {
        return;
    }
    let was_hidden = slot
        .shown
        .is_none_or(|(state, _)| state == IndicatorState::Hidden);
    slot.shown = Some(desired);
    let (state, position) = desired;
    if state == IndicatorState::Hidden || position == IndicatorPosition::Hidden {
        overlay.hide();
        return;
    }
    overlay.update(move |hud, tx| {
        hud.state = state;
        tx.instant();
    });
    if was_hidden {
        overlay.set_edge(match position {
            IndicatorPosition::Bottom => ScreenEdge::Bottom,
            _ => ScreenEdge::Top,
        });
    }
    overlay.show();
}

/// Why the HUD can't show, once Rune has tried.
pub(crate) fn limitation() -> Option<String> {
    let overlay = OVERLAY.get()?;
    (overlay.availability() == Availability::Unavailable).then(|| {
        "Recording HUD unavailable: this compositor does not support Wayland layer-shell. Dictation still works.".into()
    })
}

fn view(hud: &Hud, _: &mut Cx<Hud>) -> El {
    let label = crate::desktop_i18n::tr(match hud.state {
        IndicatorState::Processing => "Transcribing",
        _ => "Recording",
    });
    // A dark pill in either appearance: #171717 on a #292929 hairline, #eeeeee 600 13px.
    row().key("hud").w_full().h_full().center().child(
        row()
            .key("pill")
            .h(36.0)
            .px(18.0)
            .center()
            .radius(18.0)
            .fill(srgb(0x171717))
            .border(1.0, srgb(0x292929))
            .child(
                text(label)
                    .key("label")
                    .font(rune::TextStyle::new(13.0, 600.0))
                    .color(srgb(0xeeeeee)),
            ),
    )
}

fn srgb(hex: u32) -> rune::Color {
    let channel = |shift: u32| rune::color::srgb_decode(((hex >> shift) & 0xff) as f32 / 255.0);
    rune::Color {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.0,
    }
}
