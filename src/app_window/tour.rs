//! The first-run tour: welcome, how Hex works, one setup screen with every
//! requirement, a practice dictation, and a short send-off. The flow itself
//! is `crate::onboarding::Flow`; this module only draws it and turns its
//! effects into actions.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Context, Div, Focusable, FontWeight, IntoElement, Window,
    div, ease_in_out, ease_out_quint, prelude::*, px, rgb,
};

use super::{AppWindow, platform};
use crate::design_tokens::*;
use crate::desktop_i18n::{tr, tr_fill};
use crate::desktop_ui::{
    ButtonKind, Icon, button, hero_keycaps, hotkey_keycaps, icon, icon_at, panel, progress_track,
    section_label, small_button, status_dot, step_dots,
};
use crate::events::{DictationPhase, VoiceEvent};
use crate::onboarding::{
    Effect, Input, ItemStatus, ModelProgress, Observation, Practice, Row, SetupItem, Step, View,
};

const PAGE_DURATION: Duration = Duration::from_millis(360);
const PAGE_SHIFT: f32 = 20.0;
const SWAP_DURATION: Duration = Duration::from_millis(220);
const WELCOME_LOOP: Duration = Duration::from_millis(2600);
const LISTENING_LOOP: Duration = Duration::from_millis(1200);

impl AppWindow {
    pub(super) fn tour_input(&mut self, input: Input, cx: &mut Context<Self>) {
        let before = self.tour.step();
        let effect = self.tour.on(input);
        let after = self.tour.step();
        if before != after {
            self.tour_forward = step_index(after) >= step_index(before);
            if after == Step::TryIt {
                self.practice_since = None;
                self.practice_seen = 0;
            }
        }
        self.apply_tour_effect(effect, cx);
        cx.notify();
    }

    fn apply_tour_effect(&mut self, effect: Effect, cx: &mut Context<Self>) {
        match effect {
            Effect::None => {}
            Effect::RequestMicrophone | Effect::OpenMicrophoneSettings | Effect::ShowGuide(_) => {
                if !self.preview {
                    self.platform_tour_permission(effect);
                }
                self.permission_refresh_at = std::time::Instant::now();
            }
            Effect::DownloadModel => {
                if let Ok(model) =
                    crate::transcription_models::validate(&self.settings.transcription)
                {
                    let language = self.settings.transcription.language.clone();
                    self.choose_transcription_model(model, language, cx);
                }
            }
            Effect::Relaunch => {
                if self.preview {
                    return;
                }
                if let Err(error) = crate::onboarding::record_relaunch(crate::onboarding::unix_ms())
                {
                    tracing::warn!(%error, "could not record the onboarding relaunch");
                }
                if self.platform_relaunch() {
                    tracing::info!("relaunching so the shortcut listener can start");
                    cx.quit();
                } else {
                    self.tour_relaunch_failed = true;
                }
            }
            Effect::Finish => {
                self.setup_visible = false;
            }
        }
    }

    /// Feeds the tour fresh permission, model, and listener state.
    pub(super) fn poll_tour(&mut self) -> bool {
        if !self.setup_visible || self.preview {
            return false;
        }
        let model = self.tour_model_progress();
        let before = self.tour.view();
        self.tour.on(Input::Status(self.setup_status, model));
        if crate::onboarding::take_listener_failure() {
            let relaunch_allowed =
                crate::onboarding::relaunch_allowed(crate::onboarding::unix_ms());
            let effect = self.tour.on(Input::ListenerFailed { relaunch_allowed });
            if effect == Effect::Relaunch {
                if let Err(error) = crate::onboarding::record_relaunch(crate::onboarding::unix_ms())
                {
                    tracing::warn!(%error, "could not record the onboarding relaunch");
                }
                if self.platform_relaunch() {
                    self.tour_quit_requested = true;
                } else {
                    self.tour_relaunch_failed = true;
                }
            }
        }
        let practiced = self.poll_practice();
        practiced || before != self.tour.view() || self.tour_quit_requested
    }

    /// The practice step reads dictation outcomes from the event log written
    /// by this process's listener.
    fn poll_practice(&mut self) -> bool {
        if self.tour.step() != Step::TryIt {
            return false;
        }
        if let Err(error) = self.event_reader.refresh() {
            tracing::warn!(%error, "could not read dictation events for practice");
            return false;
        }
        let since = *self
            .practice_since
            .get_or_insert_with(crate::events::now_ms);
        let observations: Vec<Observation> = self
            .event_reader
            .events()
            .iter()
            .filter_map(|event| match event {
                VoiceEvent::Dictation {
                    timestamp_ms,
                    phase,
                    ..
                } if *timestamp_ms >= since => observation(phase),
                _ => None,
            })
            .collect();
        let observations = observations
            .get(self.practice_seen..)
            .unwrap_or_default()
            .to_vec();
        self.practice_seen += observations.len();
        let changed = !observations.is_empty();
        for observation in observations {
            self.tour.on(Input::Dictation(observation));
        }
        changed
    }

    fn tour_model_progress(&self) -> ModelProgress {
        let selected = crate::transcription_models::validate(&self.settings.transcription).ok();
        let downloading = selected.and_then(|model| {
            (self.transcription_status.model == Some(model.id)).then(|| {
                model.download_bytes().map_or(0.0, |bytes| {
                    (self.transcription_status.downloaded_bytes as f32 / bytes as f32)
                        .clamp(0.0, 1.0)
                })
            })
        });
        ModelProgress {
            installed: self.setup_status.transcription_model,
            downloading,
            failed: downloading.is_none() && self.transcription_status.error.is_some(),
        }
    }

    pub(super) fn render_tour(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self.tour.view();
        let forward = if self.tour_forward { 1.0 } else { -1.0 };
        let reduce_motion = crate::onboarding::reduce_motion();
        let page = match view.step {
            Step::Welcome => self.render_welcome(reduce_motion),
            Step::HowItWorks => self.render_how_it_works(reduce_motion),
            Step::Setup => self.render_setup_step(&view, cx),
            Step::TryIt => self.render_try_it(&view, reduce_motion, window, cx),
            Step::Done => self.render_done(reduce_motion),
        };
        let page = div()
            .id(("tour-page", step_index(view.step)))
            .relative()
            .w(px(ONBOARDING_WIDTH))
            .flex()
            .flex_col()
            .items_center()
            .child(page)
            .with_animation(
                ("tour-page-motion", step_index(view.step)),
                Animation::new(PAGE_DURATION).with_easing(ease_out_quint()),
                move |page, delta| {
                    let page = page.opacity(delta);
                    if reduce_motion {
                        page
                    } else {
                        page.left(px(forward * PAGE_SHIFT * (1.0 - delta)))
                    }
                },
            );

        div()
            .id("onboarding")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(rgb(CANVAS))
            .flex()
            .flex_col()
            .items_center()
            .child(
                platform::window_drag_region(div().w_full().h(px(TITLEBAR_INSET)).flex_none())
                    .children(platform::caption(window)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(page),
            )
            .child(self.render_tour_footer(&view, cx))
            .into_any_element()
    }

    fn render_tour_footer(&mut self, view: &View, cx: &mut Context<Self>) -> Div {
        let primary: Option<(&'static str, bool)> = match view.step {
            Step::Welcome => Some((tr("Get started"), true)),
            Step::HowItWorks => Some((tr("Continue"), true)),
            Step::Setup => Some((tr("Continue"), view.ready)),
            Step::TryIt if view.practice == Practice::Landed => Some((tr("Continue"), true)),
            Step::TryIt => None,
            Step::Done => Some((tr("Start using Hex"), true)),
        };
        // Until setup is ready, the next setup row carries the screen's
        // accent, so Continue waits as a disabled neutral button.
        let trailing = match primary {
            Some((label, true)) => button(ButtonKind::Primary, label)
                .id("tour-continue")
                .flex_1()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.tour_input(Input::Continue, cx)))
                .into_any_element(),
            Some((label, false)) => button(ButtonKind::Secondary, label)
                .id("tour-continue")
                .flex_1()
                .justify_center()
                .opacity(DISABLED_OPACITY)
                .into_any_element(),
            None => button(ButtonKind::Secondary, tr("Skip"))
                .id("tour-skip")
                .flex_1()
                .justify_center()
                .on_click(cx.listener(|this, _, _, cx| this.tour_input(Input::Skip, cx)))
                .into_any_element(),
        };
        let back = view.can_go_back.then(|| {
            button(ButtonKind::Secondary, tr("Back"))
                .id("tour-back")
                .on_click(cx.listener(|this, _, _, cx| this.tour_input(Input::Back, cx)))
        });
        div()
            .w(px(ONBOARDING_WIDTH))
            .flex_none()
            .pb_6()
            .flex()
            .flex_col()
            .items_center()
            .gap_5()
            .child(
                div()
                    .min_w(px(ONBOARDING_PRIMARY_WIDTH))
                    .flex()
                    .child(trailing),
            )
            .child(
                div()
                    .w_full()
                    .h(px(CONTROL_HEIGHT))
                    .flex()
                    .items_center()
                    .child(div().flex_1().flex().children(back))
                    .when(view.step != Step::Done, |row| {
                        row.child(step_dots(view.index, view.count))
                    })
                    .child(div().flex_1()),
            )
    }

    fn shortcut_parts(&self) -> Vec<String> {
        self.settings.dictation_hotkey.keycaps()
    }

    fn shortcut_label(&self) -> String {
        self.shortcut_parts().join("")
    }

    fn holds(&self) -> bool {
        self.settings.shortcut_mode == crate::dictation::ShortcutMode::Hold
    }

    fn render_welcome(&self, reduce_motion: bool) -> AnyElement {
        let parts = self.shortcut_parts();
        let art = div()
            .relative()
            .h(px(ONBOARDING_ART_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .justify_center();
        let art = if reduce_motion {
            art.child(listening_art(parts, 0.5)).into_any_element()
        } else {
            art.with_animation(
                "tour-welcome-art",
                Animation::new(WELCOME_LOOP).repeat(),
                move |art, phase| art.child(listening_art(parts.clone(), phase)),
            )
            .into_any_element()
        };
        centered_page(
            art,
            tr("Welcome to Hex"),
            if platform::CAPABILITIES.permissions {
                tr("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this Mac.")
            } else {
                tr("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this computer.")
            }
            .into(),
        )
    }

    fn render_how_it_works(&self, reduce_motion: bool) -> AnyElement {
        let shortcut = self.shortcut_label();
        let parts = self.shortcut_parts();
        let (first, last) = if self.holds() {
            (
                tr_fill("Hold {} in any app where you can type.", &shortcut),
                tr_fill(
                    "Let go of {}. Hex pastes your words where your cursor is.",
                    &shortcut,
                ),
            )
        } else {
            (
                tr_fill("Press {} in any app where you can type.", &shortcut),
                tr_fill(
                    "Press {} again. Hex pastes your words where your cursor is.",
                    &shortcut,
                ),
            )
        };
        let waveform = if reduce_motion {
            waveform_art(0.3).into_any_element()
        } else {
            div()
                .with_animation(
                    "tour-how-waveform",
                    Animation::new(LISTENING_LOOP).repeat(),
                    |art, phase| art.child(waveform_art(phase)),
                )
                .into_any_element()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_6()
            .child(page_title(tr("How Hex works"), None))
            .child(
                panel()
                    .child(how_row(hotkey_keycaps(parts.clone(), 1.0), first, false))
                    .child(how_row(
                        waveform,
                        tr("Speak naturally. A small capsule at the bottom of the screen shows Hex is listening.").into(),
                        false,
                    ))
                    .child(how_row(pasted_art(), last, true)),
            )
            .child(
                div()
                    .text_footnote()
                    .text_color(rgb(TEXT_TERTIARY))
                    .text_center()
                    .child(tr("Press Esc while speaking to cancel. Nothing is pasted.")),
            )
            .into_any_element()
    }

    fn render_setup_step(&mut self, view: &View, cx: &mut Context<Self>) -> AnyElement {
        let permissions = platform::CAPABILITIES.permissions;
        let rows = view
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                self.render_setup_row(*row, index + 1 == view.rows.len(), cx)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_6()
            .child(page_title(
                if permissions {
                    tr("Let Hex hear you and type for you")
                } else {
                    tr("Download a speech model")
                },
                Some(
                    if permissions {
                        tr("Rows turn green as macOS confirms them. Any order works.")
                    } else {
                        tr("Hex transcribes on this computer, so it needs a local model first.")
                    }
                    .into(),
                ),
            ))
            .child(panel().children(rows))
            .when(self.tour_relaunch_failed, |page| {
                page.child(notice(tr(
                    "Quit and reopen Hex to finish setup. macOS applies some permissions only after a restart.",
                )))
            })
            .into_any_element()
    }

    fn render_setup_row(&mut self, row: Row, last: bool, cx: &mut Context<Self>) -> Div {
        let model = crate::transcription_models::validate(&self.settings.transcription).ok();
        let (glyph, title, description): (Icon, &'static str, String) = match row.item {
            SetupItem::Microphone => (
                Icon::Microphone,
                tr("Microphone"),
                match row.status {
                    ItemStatus::Denied => {
                        tr("Turned off earlier. Switch Hex on in System Settings.").into()
                    }
                    _ => tr("Hears you only while you hold the shortcut").into(),
                },
            ),
            SetupItem::InputMonitoring => (
                Icon::Keyboard,
                tr("Input Monitoring"),
                match row.status {
                    ItemStatus::Waiting => {
                        tr("Drag Hex into the list, or switch it on if it is there").into()
                    }
                    _ => tr("Notices the shortcut in every app").into(),
                },
            ),
            SetupItem::Accessibility => (
                Icon::Accessibility,
                tr("Accessibility"),
                match row.status {
                    ItemStatus::Waiting => {
                        tr("Drag Hex into the list, or switch it on if it is there").into()
                    }
                    _ => tr("Pastes your words into the app you are using").into(),
                },
            ),
            SetupItem::Model => (
                Icon::Waveform,
                tr("Speech model"),
                match (row.status, model) {
                    (ItemStatus::Failed, _) => {
                        tr("The download stopped. Check your connection and try again.").into()
                    }
                    (_, Some(model)) => format!("{} · {}", model.name, model.size_label()),
                    (_, None) => tr("Runs entirely on this computer").into(),
                },
            ),
        };
        let ready = row.status == ItemStatus::Ready;
        let badge = div()
            .size(px(ONBOARDING_BADGE_SIZE))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(if ready {
                SUCCESS_SURFACE
            } else {
                SURFACE_HOVER
            }))
            .child(icon(glyph, if ready { SUCCESS } else { TEXT_SECONDARY }));
        let trailing = self.render_setup_control(row, cx);
        div()
            .w_full()
            .min_h(px(SETTINGS_ROW_MIN_HEIGHT))
            .px_4()
            .py_3()
            .flex()
            .items_center()
            .gap_3()
            .when(!last, |row| row.border_b_1().border_color(rgb(LINE)))
            .child(badge)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_body()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_footnote()
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(description),
                    ),
            )
            .child(
                div()
                    .id((
                        "setup-control",
                        setup_item_index(row.item) * 8 + status_index(row.status),
                    ))
                    .flex_none()
                    .child(trailing)
                    .with_animation(
                        (
                            "setup-control-motion",
                            setup_item_index(row.item) * 8 + status_index(row.status),
                        ),
                        Animation::new(SWAP_DURATION).with_easing(ease_out_quint()),
                        |control, delta| control.opacity(delta),
                    ),
            )
    }

    fn render_setup_control(&mut self, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let item = row.item;
        let kind = if row.next {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        };
        let grant = move |label: &'static str, kind: ButtonKind, cx: &mut Context<Self>| {
            small_button(kind, label)
                .id(("setup-grant", setup_item_index(item)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tour_input(Input::Grant(item), cx);
                }))
                .into_any_element()
        };
        match row.status {
            ItemStatus::Ready => div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_footnote()
                .text_color(rgb(SUCCESS))
                .child(icon(Icon::Check, SUCCESS))
                .child(if item == SetupItem::Model {
                    tr("Ready")
                } else {
                    tr("Allowed")
                })
                .into_any_element(),
            ItemStatus::Todo if item == SetupItem::Model => div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    small_button(ButtonKind::Secondary, tr("Change"))
                        .id("setup-choose-model")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.transcription_picker_language =
                                Some(this.settings.transcription.language.clone());
                            this.clear_transcription_error();
                            cx.notify();
                        })),
                )
                .child(grant(tr("Download"), kind, cx))
                .into_any_element(),
            ItemStatus::Todo => grant(tr("Allow"), kind, cx),
            ItemStatus::Waiting => div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    status_dot(ACCENT).with_animation(
                        ("setup-waiting", setup_item_index(item)),
                        Animation::new(LISTENING_LOOP)
                            .repeat()
                            .with_easing(ease_in_out),
                        |dot, phase| dot.opacity(pulse(phase)),
                    ),
                )
                .child(grant(
                    if item == SetupItem::Microphone {
                        tr("Allow")
                    } else {
                        tr("Open settings")
                    },
                    ButtonKind::Secondary,
                    cx,
                ))
                .into_any_element(),
            ItemStatus::Denied => grant(tr("Open settings"), kind, cx),
            ItemStatus::Downloading(progress) => div()
                .w(px(ONBOARDING_PROGRESS_WIDTH))
                .flex()
                .items_center()
                .gap_2()
                .child(progress_track(div().w(gpui::relative(progress))))
                .child(
                    div()
                        .text_caption()
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(format!("{:.0}%", progress * 100.0)),
                )
                .into_any_element(),
            ItemStatus::Failed => grant(tr("Retry"), kind, cx),
        }
    }

    fn render_try_it(
        &mut self,
        view: &View,
        reduce_motion: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shortcut = self.shortcut_label();
        let field = self.practice_input.clone();
        if !self.preview && !field.read(cx).is_focused(window) && view.practice != Practice::Landed
        {
            field.read(cx).focus_handle(cx).focus(window);
        }
        let hint = self.practice_hint(view, reduce_motion, &shortcut, cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_6()
            .child(page_title(
                tr("Give it a try"),
                Some(if self.holds() {
                    tr_fill("Hold {}, say a sentence, then let go.", &shortcut)
                } else {
                    tr_fill("Press {}, say a sentence, then press it again.", &shortcut)
                }),
            ))
            .child(div().flex().flex_col().gap_3().child(field).child(hint))
            .into_any_element()
    }

    fn practice_hint(
        &mut self,
        view: &View,
        reduce_motion: bool,
        shortcut: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if view.reopen {
            return div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(hint_text(
                    tr("Hex needs to reopen before it can hear the shortcut."),
                    TEXT_SECONDARY,
                ))
                .child(
                    small_button(ButtonKind::Secondary, tr("Reopen Hex"))
                        .id("tour-reopen")
                        .on_click(cx.listener(|this, _, _, cx| this.tour_input(Input::Reopen, cx))),
                )
                .into_any_element();
        }
        let (dot, text, color): (Option<u32>, String, u32) = match view.practice {
            Practice::Waiting => {
                return div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(hotkey_keycaps(self.shortcut_parts(), 1.0))
                    .child(hint_text(
                        if self.holds() {
                            tr("Hold to talk")
                        } else {
                            tr("Press to talk")
                        },
                        TEXT_TERTIARY,
                    ))
                    .into_any_element();
            }
            Practice::Listening => (
                Some(DANGER),
                if self.holds() {
                    tr_fill("Listening… let go of {} when you are done.", shortcut)
                } else {
                    tr_fill("Listening… press {} again when you are done.", shortcut)
                },
                TEXT_SECONDARY,
            ),
            Practice::Transcribing => (None, tr("Transcribing…").into(), TEXT_TERTIARY),
            Practice::TooShort => (
                None,
                if self.holds() {
                    tr_fill(
                        "That was a quick tap. Keep holding {} while you speak.",
                        shortcut,
                    )
                } else {
                    tr("That was too short to hear. Speak a full sentence before you finish.")
                        .into()
                },
                TEXT_SECONDARY,
            ),
            Practice::NothingHeard => (
                None,
                tr("Hex did not catch any words. Speak a little closer, or pick another microphone in Settings.").into(),
                TEXT_SECONDARY,
            ),
            Practice::Failed => (
                None,
                tr("Something went wrong while transcribing. Try once more.").into(),
                DANGER,
            ),
            Practice::Landed => {
                return div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(Icon::Check, SUCCESS))
                    .child(hint_text(
                        tr("That's it. Hex pastes wherever your cursor is, in any app."),
                        SUCCESS,
                    ))
                    .into_any_element();
            }
        };
        let dot = dot.map(|color| {
            if reduce_motion {
                status_dot(color).into_any_element()
            } else {
                status_dot(color)
                    .with_animation(
                        "practice-listening-dot",
                        Animation::new(LISTENING_LOOP)
                            .repeat()
                            .with_easing(ease_in_out),
                        |dot, phase| dot.opacity(pulse(phase)),
                    )
                    .into_any_element()
            }
        });
        div()
            .id(("practice-hint", practice_index(view.practice)))
            .flex()
            .items_center()
            .gap_2()
            .children(dot)
            .child(hint_text(text, color))
            .with_animation(
                ("practice-hint-motion", practice_index(view.practice)),
                Animation::new(SWAP_DURATION).with_easing(ease_out_quint()),
                |hint, delta| hint.opacity(delta),
            )
            .into_any_element()
    }

    fn render_done(&self, reduce_motion: bool) -> AnyElement {
        let art = div()
            .size(px(HERO_KEYCAP_SIZE + 16.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(SUCCESS_SURFACE))
            .child(icon_at(Icon::Check, HERO_ICON_SIZE, SUCCESS));
        let art = div()
            .h(px(ONBOARDING_ART_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .child(if reduce_motion {
                art.into_any_element()
            } else {
                art.with_animation(
                    "tour-done-art",
                    Animation::new(PAGE_DURATION).with_easing(ease_out_quint()),
                    |art, delta| art.opacity(delta),
                )
                .into_any_element()
            });
        let mut tips: Vec<(Vec<String>, &'static str, &'static str)> = Vec::new();
        if platform::CAPABILITIES.paste_last
            && let Some(binding) = self.settings.paste_last_hotkey.as_ref()
        {
            tips.push((
                binding.keycaps(),
                tr("Paste your last dictation"),
                tr("Missed the text field? Paste it again anywhere."),
            ));
        }
        if platform::CAPABILITIES.voice_action {
            tips.push((
                self.settings.edit_hotkey.keycaps(),
                tr("Voice Action"),
                tr("Ask OpenCode to write or rewrite. Turn it on in Voice Action."),
            ));
        }
        if platform::CAPABILITIES.modes {
            tips.push((
                Vec::new(),
                tr("Modes"),
                tr("Fix names, tone, and formatting per app."),
            ));
        }
        let count = tips.len();
        let tips = tips
            .into_iter()
            .enumerate()
            .map(|(index, (keys, title, description))| {
                div()
                    .w_full()
                    .px_4()
                    .py_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(index + 1 < count, |row| {
                        row.border_b_1().border_color(rgb(LINE))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_body()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_footnote()
                                    .text_color(rgb(TEXT_TERTIARY))
                                    .child(description),
                            ),
                    )
                    .when(!keys.is_empty(), |row| row.child(hotkey_keycaps(keys, 1.0)))
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .child(art)
            .child(page_title(
                tr("You're all set"),
                Some(if cfg!(target_os = "windows") {
                    tr_fill(
                        "Hex waits in the system tray. Use {} in any app to dictate.",
                        &self.shortcut_label(),
                    )
                } else if cfg!(target_os = "linux") {
                    tr_fill(
                        "Hex keeps running after you close this window. Use {} in any app to dictate.",
                        &self.shortcut_label(),
                    )
                } else {
                    tr_fill(
                        "Hex waits in the menu bar. Use {} in any app to dictate.",
                        &self.shortcut_label(),
                    )
                }),
            ))
            .when(count > 0, |page| {
                page.child(
                    div()
                        .w_full()
                        .mt_6()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(section_label(tr("When you want more")).px_4())
                        .child(panel().children(tips)),
                )
            })
            .into_any_element()
    }
}

fn observation(phase: &DictationPhase) -> Option<Observation> {
    Some(match phase {
        DictationPhase::Started => Observation::Started,
        DictationPhase::Transcribing => Observation::Transcribing,
        DictationPhase::Discarded => Observation::Discarded,
        DictationPhase::Cancelled => Observation::Cancelled,
        DictationPhase::Pasted | DictationPhase::Edited => Observation::Pasted,
        DictationPhase::Failed(_) => Observation::Failed,
        _ => return None,
    })
}

fn step_index(step: Step) -> usize {
    match step {
        Step::Welcome => 0,
        Step::HowItWorks => 1,
        Step::Setup => 2,
        Step::TryIt => 3,
        Step::Done => 4,
    }
}

fn setup_item_index(item: SetupItem) -> usize {
    match item {
        SetupItem::Microphone => 0,
        SetupItem::InputMonitoring => 1,
        SetupItem::Accessibility => 2,
        SetupItem::Model => 3,
    }
}

fn status_index(status: ItemStatus) -> usize {
    match status {
        ItemStatus::Ready => 0,
        ItemStatus::Todo => 1,
        ItemStatus::Waiting => 2,
        ItemStatus::Denied => 3,
        ItemStatus::Downloading(_) => 4,
        ItemStatus::Failed => 5,
    }
}

fn practice_index(practice: Practice) -> usize {
    match practice {
        Practice::Waiting => 0,
        Practice::Listening => 1,
        Practice::Transcribing => 2,
        Practice::TooShort => 3,
        Practice::NothingHeard => 4,
        Practice::Landed => 5,
        Practice::Failed => 6,
    }
}

fn page_title(title: &'static str, body: Option<String>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .child(div().text_title().text_center().child(title))
        .when_some(body, |heading, body| {
            heading.child(
                div()
                    .max_w(px(ONBOARDING_BODY_WIDTH))
                    .text_body()
                    .text_color(rgb(TEXT_SECONDARY))
                    .text_center()
                    .child(body),
            )
        })
}

fn centered_page(art: AnyElement, title: &'static str, body: String) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap_6()
        .child(art)
        .child(page_title(title, Some(body)))
        .into_any_element()
}

fn how_row(art: impl IntoElement, text: String, last: bool) -> Div {
    div()
        .w_full()
        .px_4()
        .py_4()
        .flex()
        .items_center()
        .gap_4()
        .when(!last, |row| row.border_b_1().border_color(rgb(LINE)))
        .child(
            div()
                .w(px(ONBOARDING_ART_COLUMN))
                .flex_none()
                .flex()
                .justify_center()
                .child(art),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_body()
                .text_color(rgb(TEXT_SECONDARY))
                .child(text),
        )
}

fn hint_text(text: impl Into<gpui::SharedString>, color: u32) -> Div {
    div()
        .text_footnote()
        .text_color(rgb(color))
        .child(text.into())
}

fn notice(text: &'static str) -> Div {
    panel()
        .px_4()
        .py_3()
        .text_footnote()
        .text_color(rgb(TEXT_SECONDARY))
        .child(text)
}

/// The shortcut pressed and released on a loop, with rings of sound while
/// it is held. `phase` runs from zero through one.
fn listening_art(parts: Vec<String>, phase: f32) -> Div {
    let pressed = if phase < 0.12 {
        smooth(phase / 0.12)
    } else if phase < 0.7 {
        1.0
    } else if phase < 0.8 {
        1.0 - smooth((phase - 0.7) / 0.1)
    } else {
        0.0
    };
    let rings = (0..2).map(move |ring| {
        let start = 0.14 + ring as f32 * 0.22;
        let t = ((phase - start) / 0.5).clamp(0.0, 1.0);
        let size = HERO_KEYCAP_SIZE + 24.0 + t * 72.0;
        let opacity = if t <= 0.0 || t >= 1.0 {
            0.0
        } else {
            (1.0 - t) * 0.9
        };
        div()
            .absolute()
            .size(px(size))
            .rounded_full()
            .border_1()
            .border_color(rgb(ACCENT_BORDER))
            .opacity(opacity)
    });
    div()
        .relative()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .children(rings)
        .child(hero_keycaps(parts, pressed))
}

/// Five bars rising and falling like a voice.
fn waveform_art(phase: f32) -> Div {
    const LEVELS: [f32; 5] = [0.45, 0.8, 1.0, 0.7, 0.5];
    div()
        .h(px(CONTROL_HEIGHT_SMALL))
        .px_3()
        .flex()
        .items_center()
        .gap_1()
        .rounded_full()
        .border_1()
        .border_color(rgb(LINE_STRONG))
        .bg(rgb(SURFACE_HOVER))
        .children(LEVELS.iter().enumerate().map(move |(index, level)| {
            let wave = (std::f32::consts::TAU * (phase + index as f32 * 0.18)).sin() * 0.5 + 0.5;
            let height = 4.0 + 10.0 * level * (0.35 + 0.65 * wave);
            div()
                .w(px(2.0))
                .h(px(height))
                .rounded_full()
                .bg(rgb(TEXT_SECONDARY))
        }))
}

/// A line of text with the cursor after it, as if just pasted.
fn pasted_art() -> Div {
    div()
        .h(px(CONTROL_HEIGHT_SMALL))
        .px_2()
        .flex()
        .items_center()
        .gap_0p5()
        .rounded(RADIUS_SMALL)
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(CANVAS))
        .text_footnote()
        .text_color(rgb(TEXT_PRIMARY))
        .child(tr("Hello"))
        .child(div().w(px(1.0)).h(px(FOOTNOTE.line_height)).bg(rgb(ACCENT)))
}

/// Dim, bright, dim over one loop.
fn pulse(phase: f32) -> f32 {
    0.35 + 0.65 * (1.0 - (2.0 * phase - 1.0).abs())
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
