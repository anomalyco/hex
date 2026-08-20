//! The shared HUD Lab pane: the macOS developer surface for live-tuning the
//! dictation HUD shader, ported to the Windows and Linux shells. The rows,
//! slider ranges, tuning defaults, and style-preset reset semantics are
//! copied from the macOS lab in `src/platform/macos/app_window.rs` and MUST
//! be kept in lockstep with it. Where the macOS lab restyles the real
//! floating HUD, this pane embeds a live CPU-shader preview at the mac HUD
//! geometry (112x64 logical at 2x backing scale) and forwards `Configure`
//! to the platform HUD through [`HudLabDelegate`]. Like the macOS lab, the
//! tuning is session-only: nothing persists, and "Copy preset" is the
//! export path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Bounds, Context, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    RenderImage, Window, canvas, div, img, prelude::*, px, rgb,
};

pub(crate) use crate::desktop::indicator_model::HudTuning;
use crate::desktop::indicator_model::{
    DictationIndicatorEvent, IndicatorModel, WINDOW_HEIGHT, WINDOW_WIDTH,
};
use crate::desktop::indicator_shader;
use crate::desktop_i18n::tr;
use crate::desktop_ui::{
    LINE, MUTED, PANEL_RADIUS, SURFACE, SURFACE_HOVER, SURFACE_SELECTED, TEXT, TEXT_SOFT,
    accent_color, compact_button, header_button, pane_content, pane_header_with_action,
    slider_track,
};

/// The mac HUD's backing scale; the preview renders at 224x128 like the
/// macOS Metal drawable and the Windows HUD window.
const PREVIEW_SCALE: f32 = 2.0;
/// Mac's `HUD_MAX_SPEED`: the top of the speed slider's range.
const HUD_MAX_SPEED: f32 = 24.0;
/// A render gap this long means the pane was left and re-entered, so the
/// demo replays from its entrance — the macOS lab likewise re-applies its
/// demo whenever its pane is shown.
const REPLAY_GAP: Duration = Duration::from_millis(400);
/// Cadence of the synthetic meter events that keep the recording demo's
/// waveform moving like live speech.
const METER_INTERVAL: Duration = Duration::from_millis(80);

/// The demo the preview plays, mirroring the macOS lab's state buttons.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudDemoState {
    Recording,
    Transcribing,
    Processing,
}

/// One tunable shader parameter, mirroring the macOS lab's control set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HudControl {
    LineCount,
    Curvature,
    Speed,
    Blur,
    Glow,
    Depth,
    LightAngle,
    Outline,
}

/// The slider rows in macOS order, with the macOS labels.
const SLIDERS: [(HudControl, &str); 8] = [
    (HudControl::LineCount, "Line count"),
    (HudControl::Curvature, "Curvature"),
    (HudControl::Speed, "Speed"),
    (HudControl::Blur, "Highlight blur"),
    (HudControl::Glow, "Line glow"),
    (HudControl::Depth, "Sphere depth"),
    (HudControl::LightAngle, "Light angle"),
    (HudControl::Outline, "Sphere outline"),
];

/// The style cards in macOS order: key, name, description.
const STYLES: [(&str, &str, &str); 3] = [
    (
        "A",
        "Moving light",
        "A line-free area light travels around the orb.",
    ),
    (
        "B",
        "Meridians",
        "Highlights wrap around the sphere instead of crossing it.",
    ),
    (
        "C",
        "Ribbons",
        "Curved bands retained for comparison and tuning.",
    ),
];

/// The full-tuning preset each style card resets to; a verbatim copy of the
/// macOS lab's `select_hud_style`.
fn style_preset(style: usize) -> HudTuning {
    match style {
        0 => HudTuning {
            style: 0.0,
            line_count: 1.0,
            curvature: 0.5,
            speed: 0.7,
            sharpness: 0.42,
            glow: 0.4,
            depth: 0.75,
            light_angle: 0.35,
            outline: 0.25,
        },
        1 => HudTuning {
            style: 1.0,
            line_count: 2.0,
            curvature: 0.47,
            speed: 17.08,
            sharpness: 0.29,
            glow: 0.77,
            depth: 0.65,
            light_angle: 0.35,
            outline: 1.0,
        },
        _ => HudTuning {
            style: 2.0,
            line_count: 3.0,
            curvature: 0.7,
            speed: 0.9,
            sharpness: 0.64,
            glow: 0.28,
            depth: 0.74,
            light_angle: 0.35,
            outline: 0.3,
        },
    }
}

/// A control's normalized slider position, the macOS value mapping.
fn control_fraction(tuning: &HudTuning, control: HudControl) -> f32 {
    match control {
        HudControl::LineCount => (tuning.line_count - 1.0) / 5.0,
        HudControl::Curvature => tuning.curvature,
        HudControl::Speed => tuning.speed / HUD_MAX_SPEED,
        HudControl::Blur => 1.0 - tuning.sharpness,
        HudControl::Glow => tuning.glow,
        HudControl::Depth => tuning.depth,
        HudControl::LightAngle => tuning.light_angle,
        HudControl::Outline => tuning.outline,
    }
    .clamp(0.0, 1.0)
}

/// The value label beside each slider, the macOS display formats.
fn control_display(tuning: &HudTuning, control: HudControl) -> String {
    match control {
        HudControl::LineCount => format!("{:.0}", tuning.line_count),
        HudControl::Speed => format!("{:.2}x", tuning.speed),
        HudControl::LightAngle => format!("{:.0}°", tuning.light_angle * 360.0),
        _ => format!("{:.2}", control_fraction(tuning, control)),
    }
}

/// The copyable preset line, byte-for-byte the macOS `hud_preset` format.
fn preset_line(tuning: &HudTuning) -> String {
    format!(
        "hud-v1 style={} lines={:.0} curvature={:.2} speed={:.2} blur={:.2} glow={:.2} depth={:.2} light={:.2} outline={:.2}",
        tuning.style as usize,
        tuning.line_count,
        tuning.curvature,
        tuning.speed,
        1.0 - tuning.sharpness,
        tuning.glow,
        tuning.depth,
        tuning.light_angle,
        tuning.outline,
    )
}

/// The lab's per-shell state: the working tuning, the demo selection, and
/// the embedded preview's indicator model. Owned by each shell's app entity
/// and rendered through [`render_hud_lab_pane`].
pub(crate) struct HudLabState {
    tuning: HudTuning,
    demo: HudDemoState,
    queued: usize,
    dragging: Option<HudControl>,
    copied: bool,
    preview: IndicatorModel,
    epoch: Instant,
    last_meter: Instant,
    last_frame: Option<Instant>,
    slider_bounds: [Option<Bounds<Pixels>>; SLIDERS.len()],
    /// The previous paint's preview texture, evicted from the sprite
    /// atlas on the next paint — without this every frame leaks a tile.
    last_render_image: Option<Arc<RenderImage>>,
}

impl HudLabState {
    pub(crate) fn new() -> Self {
        let mut preview = IndicatorModel::new();
        preview.set_backing_scale(PREVIEW_SCALE);
        let now = Instant::now();
        let mut state = Self {
            tuning: HudTuning::default(),
            // The macOS lab drives the real floating HUD and opens on its
            // transcribing demo; the embedded preview opens recording so
            // the pane greets its user with the live waveform capsule.
            demo: HudDemoState::Recording,
            queued: 1,
            dragging: None,
            copied: false,
            preview,
            epoch: now,
            last_meter: now,
            last_frame: None,
            slider_bounds: [None; SLIDERS.len()],
            last_render_image: None,
        };
        state.apply_demo();
        state
    }

    /// Replays the selected demo on the preview model; a copy of the macOS
    /// `apply_hud_lab` aimed at the embedded model instead of the real HUD.
    fn apply_demo(&mut self) {
        self.preview
            .handle(DictationIndicatorEvent::Configure(self.tuning));
        self.preview.handle(DictationIndicatorEvent::Reset);
        match self.demo {
            HudDemoState::Recording => {
                for job_id in 0..self.queued as u64 {
                    self.preview
                        .handle(DictationIndicatorEvent::Submitted { job_id });
                    self.preview
                        .handle(DictationIndicatorEvent::Transcribing { job_id });
                }
                self.preview.handle(DictationIndicatorEvent::Started);
                self.preview.handle(DictationIndicatorEvent::Meter {
                    average: 0.08,
                    peak: 0.42,
                });
            }
            HudDemoState::Transcribing | HudDemoState::Processing => {
                self.preview
                    .handle(DictationIndicatorEvent::Submitted { job_id: 0 });
                self.preview.handle(match self.demo {
                    HudDemoState::Transcribing => {
                        DictationIndicatorEvent::Transcribing { job_id: 0 }
                    }
                    HudDemoState::Processing => DictationIndicatorEvent::Processing { job_id: 0 },
                    HudDemoState::Recording => unreachable!(),
                });
                for job_id in 1..=self.queued as u64 {
                    self.preview
                        .handle(DictationIndicatorEvent::Submitted { job_id });
                    self.preview
                        .handle(DictationIndicatorEvent::Transcribing { job_id });
                }
            }
        }
    }

    /// Applies a slider position, the macOS `set_hud_control` mapping, and
    /// returns the tuning for delivery to the platform HUD.
    fn set_control(&mut self, control: HudControl, fraction: f32) -> HudTuning {
        let value = fraction.clamp(0.0, 1.0);
        match control {
            HudControl::LineCount => self.tuning.line_count = (1.0 + value * 5.0).round(),
            HudControl::Curvature => self.tuning.curvature = value,
            HudControl::Speed => self.tuning.speed = value * HUD_MAX_SPEED,
            HudControl::Blur => self.tuning.sharpness = 1.0 - value,
            HudControl::Glow => self.tuning.glow = value,
            HudControl::Depth => self.tuning.depth = value,
            HudControl::LightAngle => self.tuning.light_angle = value,
            HudControl::Outline => self.tuning.outline = value,
        }
        self.copied = false;
        self.preview
            .handle(DictationIndicatorEvent::Configure(self.tuning));
        self.tuning
    }

    /// Resets the full tuning to a style card's preset and replays the
    /// demo, the macOS `select_hud_style` reset behavior.
    fn select_style(&mut self, style: usize) -> HudTuning {
        self.tuning = style_preset(style);
        self.copied = false;
        self.apply_demo();
        self.tuning
    }

    fn set_demo(&mut self, demo: HudDemoState) {
        self.demo = demo;
        self.apply_demo();
    }

    fn set_queued(&mut self, queued: usize) {
        self.queued = queued;
        self.apply_demo();
    }

    /// Copies the preset line, mirroring the macOS header action.
    fn copy_preset(&mut self) -> bool {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return false;
        };
        let _ = clipboard.set_text(preset_line(&self.tuning));
        self.copied = true;
        true
    }

    /// Steps the preview one frame and rasterizes it: synthetic meter
    /// events keep the recording demo speaking, and a long gap since the
    /// last frame means the pane was re-entered, so the demo replays.
    fn preview_frame(&mut self) -> Arc<RenderImage> {
        let now = Instant::now();
        if self
            .last_frame
            .is_none_or(|last| now.duration_since(last) > REPLAY_GAP)
        {
            self.apply_demo();
        }
        self.last_frame = Some(now);
        if self.demo == HudDemoState::Recording
            && now.duration_since(self.last_meter) >= METER_INTERVAL
        {
            self.last_meter = now;
            // A synthetic voice: the macOS demo meter (0.08 average, 0.42
            // peak) wobbled by two incommensurate sines.
            let time = now.duration_since(self.epoch).as_secs_f32();
            self.preview.handle(DictationIndicatorEvent::Meter {
                average: 0.08 * (1.0 + 0.45 * (time * 2.3).sin() * (time * 1.1).cos()),
                peak: 0.42 * (1.0 + 0.3 * (time * 3.7).sin()),
            });
        }
        let frame = self.preview.frame(now);
        let width = (WINDOW_WIDTH * PREVIEW_SCALE) as usize;
        let height = (WINDOW_HEIGHT * PREVIEW_SCALE) as usize;
        let mut bytes = indicator_shader::render(&frame.uniforms, width, height);
        // gpui's raw frames carry BGRA in an RGBA container.
        for pixel in bytes.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let buffer = image::RgbaImage::from_raw(width as u32, height as u32, bytes)
            .expect("the shader buffer matches its dimensions");
        Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
    }
}

/// The lab's per-shell hooks: where its state lives and how the tuning
/// reaches the platform's real dictation HUD.
pub(crate) trait HudLabDelegate: Sized + 'static {
    fn hud_lab(&self) -> &HudLabState;
    fn hud_lab_mut(&mut self) -> &mut HudLabState;
    /// Deliver the tuning to the platform's dictation HUD so lab edits
    /// restyle the real capsule immediately, like the macOS lab does.
    fn configure_platform_hud(&mut self, tuning: HudTuning);
}

fn update_slider<D: HudLabDelegate>(
    this: &mut D,
    control: HudControl,
    pointer_x: Pixels,
    cx: &mut Context<D>,
) {
    let index = SLIDERS
        .iter()
        .position(|(candidate, _)| *candidate == control)
        .expect("every control has a slider row");
    let Some(bounds) = this.hud_lab().slider_bounds[index] else {
        return;
    };
    if bounds.size.width <= px(0.0) {
        return;
    }
    let fraction = ((pointer_x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
    let tuning = this.hud_lab_mut().set_control(control, fraction);
    this.configure_platform_hud(tuning);
    cx.notify();
}

pub(crate) fn render_hud_lab_pane<D: HudLabDelegate>(
    view: &mut D,
    window: &mut Window,
    cx: &mut Context<D>,
) -> AnyElement {
    // The preview is the pane's animation: step one shader frame per paint
    // and immediately schedule the next — the request_animation_frame
    // cadence the macOS springs animate with.
    if let Some(previous) = view.hud_lab_mut().last_render_image.take() {
        let _ = window.drop_image(previous);
    }
    let preview = view.hud_lab_mut().preview_frame();
    view.hud_lab_mut().last_render_image = Some(preview.clone());
    window.request_animation_frame();
    let state = view.hud_lab();
    let tuning = state.tuning;
    let demo = state.demo;
    let queued = state.queued;
    let copied = state.copied;

    let preview_panel = div()
        .w_full()
        .py_6()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(PANEL_RADIUS))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x0d0d0d))
        .child(
            img(preview)
                .w(px(WINDOW_WIDTH))
                .h(px(WINDOW_HEIGHT))
                .flex_none(),
        );

    let state_button = |demo_state: HudDemoState, label: &'static str, index: usize| {
        compact_button(tr(label))
            .id(("hud-lab-state", index))
            .when(demo == demo_state, |button| {
                button.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.hud_lab_mut().set_demo(demo_state);
                cx.notify();
            }))
    };
    let queue_buttons = (0..=3_usize).map(|count| {
        compact_button(count.to_string())
            .id(("hud-lab-queue", count))
            .when(queued == count, |button| {
                button.bg(rgb(SURFACE_SELECTED)).text_color(rgb(TEXT))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.hud_lab_mut().set_queued(count);
                cx.notify();
            }))
    });

    let style_cards = STYLES
        .into_iter()
        .enumerate()
        .map(|(index, (key, name, description))| {
            let selected = tuning.style as usize == index;
            div()
                .id(("hud-lab-style", index))
                .w(px(190.0))
                .p_3()
                .flex()
                .flex_col()
                .gap_1()
                .rounded_md()
                .border_1()
                .border_color(if selected {
                    rgb(accent_color())
                } else {
                    rgb(LINE)
                })
                .bg(rgb(if selected { SURFACE_SELECTED } else { SURFACE }))
                .hover(|card| card.bg(rgb(SURFACE_HOVER)))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(accent_color()))
                        .child(key),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(name)),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(MUTED))
                        .child(tr(description)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let tuning = this.hud_lab_mut().select_style(index);
                    this.configure_platform_hud(tuning);
                    cx.notify();
                }))
        });

    let slider = |control: HudControl, label: &'static str, index: usize| -> AnyElement {
        let value = control_fraction(&tuning, control);
        let display = control_display(&tuning, control);
        let bounds_entity = cx.entity();
        div()
            .w(px(360.0))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(12.0))
                    .child(tr(label))
                    .child(div().text_color(rgb(MUTED)).child(display)),
            )
            .child(
                div()
                    .id(("hud-lab-slider", index))
                    .relative()
                    .w_full()
                    .h(px(18.0))
                    .flex()
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.hud_lab_mut().dragging = Some(control);
                            update_slider(this, control, event.position.x, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if this.hud_lab().dragging == Some(control) {
                            if event.pressed_button == Some(MouseButton::Left) {
                                update_slider(this, control, event.position.x, cx);
                            } else {
                                this.hud_lab_mut().dragging = None;
                                cx.notify();
                            }
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.hud_lab_mut().dragging = None;
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.hud_lab_mut().dragging = None;
                            cx.notify();
                        }),
                    )
                    .child(slider_track(value))
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                bounds_entity.update(cx, |this, _| {
                                    this.hud_lab_mut().slider_bounds[index] = Some(bounds);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
            .into_any_element()
    };
    let sliders = SLIDERS
        .into_iter()
        .enumerate()
        .map(|(index, (control, label))| slider(control, label, index));

    let preset_box = div()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(0x0d0d0d))
        .text_size(px(10.0))
        .text_color(rgb(TEXT_SOFT))
        .child(preset_line(&tuning));
    #[cfg(target_os = "windows")]
    let preset_box = preset_box.font_family("Consolas");
    #[cfg(target_os = "linux")]
    let preset_box = preset_box.font_family("monospace");

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(pane_header_with_action(
            "HUD Lab",
            Some(
                header_button(if copied {
                    tr("Copied")
                } else {
                    tr("Copy preset")
                })
                .id("hud-lab-copy-preset")
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.hud_lab_mut().copy_preset() {
                        cx.notify();
                    }
                }))
                .into_any_element(),
            ),
        ))
        .child(
            pane_content()
                .id("hud-lab-scroll")
                .flex_1()
                .mx_auto()
                .overflow_y_scroll()
                .p_6()
                .gap_6()
                .child(preview_panel)
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children([
                            state_button(HudDemoState::Recording, "Recording", 0),
                            state_button(HudDemoState::Transcribing, "Transcribing", 1),
                            state_button(HudDemoState::Processing, "Processing", 2),
                        ])
                        .child(div().w_4())
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(MUTED))
                                .child(tr("Queued")),
                        )
                        .children(queue_buttons),
                )
                .child(div().flex().gap_3().children(style_cards))
                .child(div().flex().flex_col().gap_4().children(sliders))
                .child(preset_box),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_mapping_mirrors_the_macos_ranges() {
        let mut state = HudLabState::new();

        assert_eq!(state.set_control(HudControl::Speed, 1.0).speed, 24.0);
        assert_eq!(
            state.set_control(HudControl::LineCount, 0.0).line_count,
            1.0
        );
        assert_eq!(
            state.set_control(HudControl::LineCount, 1.0).line_count,
            6.0
        );
        let blurred = state.set_control(HudControl::Blur, 0.25);
        assert!((blurred.sharpness - 0.75).abs() < f32::EPSILON);
        // Out-of-track pointer positions clamp like the macOS lab.
        assert_eq!(state.set_control(HudControl::Glow, 1.7).glow, 1.0);
    }

    #[test]
    fn style_cards_reset_the_full_tuning_to_the_macos_presets() {
        let mut state = HudLabState::new();
        state.set_control(HudControl::Glow, 0.1);

        let reset = state.select_style(1);

        // Style B is the shipped default, so resetting to it restores
        // `HudTuning::default()` field for field.
        let default = HudTuning::default();
        assert_eq!(preset_line(&reset), preset_line(&default));
        assert_eq!(state.select_style(0).style, 0.0);
        assert_eq!(state.select_style(2).style, 2.0);
    }

    #[test]
    fn preset_line_matches_the_macos_export_format() {
        assert_eq!(
            preset_line(&HudTuning::default()),
            "hud-v1 style=1 lines=2 curvature=0.47 speed=17.08 blur=0.71 glow=0.77 \
             depth=0.65 light=0.35 outline=1.00"
        );
    }
}
