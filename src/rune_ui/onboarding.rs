//! The first-run tour, driven by `onboarding::Flow`.
//!
//! What it shows follows [`Capabilities`](super::platform::Capabilities):
//! macOS asks for every privacy permission on one screen (with the System
//! Settings guide and a relaunch when macOS applies a grant only after a
//! restart) and ends with a practice dictation into the window's own field,
//! while the speech model downloads behind them. Linux and Windows have no
//! permissions, and this window cannot observe a practice dictation there,
//! so their steps are Welcome, the speech model download, and Done.
//!
//! The welcome art is the GPU hero ([`hero`](super::hero)); the done art is static.

use std::time::{Duration, Instant};

use rune::{Color, El, TextEdit, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Progress, TextInput, glyphs};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, KeySize, Paint, PushSize, TRAY_PADDING, button, card, icon_row,
    key_tray, keycap, keycaps, setting_row,
};
use super::lists::{PermissionControl, allowed, permission_control};
use super::platform::{Os, PlatformRequest, PlatformState};
use super::{Menu, Request};
use crate::app_settings::AppSettings;
use crate::desktop::i18n::{tr, tr_fill};
use crate::desktop::onboarding::{
    Effect, Flow, Input, ItemStatus, ModelProgress, Observation, PermissionState, Practice, Row,
    SetupItem, Step, View,
};
use crate::dictation::ShortcutMode;

const WIDTH: f32 = 520.0;
const ART_HEIGHT: f32 = 140.0;
/// Every step's heading: the one display size in the window.
const TITLE: TextStyle = TextStyle::new(26.0, 600.0).lh(32.0).tracking(-0.3);
/// The line under a heading.
const LEAD: TextStyle = TextStyle::new(15.0, 400.0).lh(22.0);
/// Back and the primary action hold equal slots, so the page dots stay
/// centred and nothing in the footer moves between steps.
const FOOTER_SLOT: f32 = 160.0;
/// How long the ready check shows before setup moves on.
const ADVANCE_BEAT: Duration = Duration::from_millis(700);

pub(super) struct Tour {
    pub(super) flow: Flow,
    advance_at: Option<Instant>,
    completed: bool,
    /// The practice dictation pastes here.
    practice: TextEdit,
    /// The welcome hero's settled frame, as Reduce Motion shows it (fixtures).
    pub(super) hero_still: bool,
    /// What the practice dictation's microphone hears.
    pub(super) mic: MicMeter,
}

/// The practice dictation's level, and whether Hex heard nothing from the
/// microphone it records from.
#[derive(Default)]
pub(super) struct MicMeter {
    /// 0 to 1, shown in `LEVEL_STEPS` steps.
    pub(super) level: f32,
    quiet_since: Option<Instant>,
    /// The microphone setting Hex heard nothing from. The warning stays
    /// until that microphone is heard or another is picked.
    pub(super) silent: Option<Option<String>>,
}

/// A peak this loud is sound; a switched-off microphone delivers zeros
/// and a quiet room stays well under it.
const HEARD_PEAK: f32 = 0.02;
/// Silence this long while recording means Hex hears nothing.
const QUIET_FOR: Duration = Duration::from_millis(1500);
const LEVEL_STEPS: f32 = 6.0;

impl MicMeter {
    /// Takes the latest `peak` from `microphone`; true when what shows changed.
    fn hear(
        &mut self,
        listening: bool,
        peak: f32,
        microphone: &Option<String>,
        now: Instant,
    ) -> bool {
        let before = (self.step(), self.silent.clone());
        if listening {
            if peak >= HEARD_PEAK {
                self.quiet_since = None;
                self.silent = None;
            } else if now.duration_since(*self.quiet_since.get_or_insert(now)) >= QUIET_FOR {
                self.silent = Some(microphone.clone());
            }
            self.level = follow_level(self.level, peak);
        } else {
            self.quiet_since = None;
            self.level = 0.0;
        }
        before != (self.step(), self.silent.clone())
    }

    fn step(&self) -> u8 {
        level_step(self.level)
    }
}

/// A meter's next level for the latest `peak`: it rises at once and falls
/// over a few polls.
pub(super) fn follow_level(level: f32, peak: f32) -> f32 {
    (peak * 3.0).clamp(0.0, 1.0).max(level - 0.12)
}

/// `level` in the steps a meter shows, so it redraws only when they change.
pub(super) fn level_step(level: f32) -> u8 {
    (level * LEVEL_STEPS).round() as u8
}

/// `level` rounded to the step a meter shows.
pub(super) fn shown_level(level: f32) -> f32 {
    f32::from(level_step(level)) / LEVEL_STEPS
}

impl Tour {
    fn new(flow: Flow, completed: bool) -> Self {
        Self {
            flow,
            advance_at: None,
            completed,
            practice: TextEdit::default(),
            hero_still: false,
            mic: MicMeter::default(),
        }
    }

    /// The tour from Welcome, as on a new Mac: Restart onboarding.
    pub(super) fn replay(settings: &AppSettings, platform: &PlatformState) -> Tour {
        let installed = super::picker::Picker::default().selected_installed(settings);
        Tour::new(
            Flow::new(
                Step::Welcome,
                platform.setup_status(installed),
                platform.caps.permissions,
                platform.caps.practice_dictation,
            ),
            false,
        )
    }

    /// The tour when this run still needs it.
    pub(super) fn start(settings: &AppSettings, platform: &PlatformState) -> Option<Tour> {
        let installed = super::picker::Picker::default().selected_installed(settings);
        let status = platform.setup_status(installed);
        let completed = crate::desktop::onboarding::completion_recorded();
        let resume = crate::desktop::onboarding::take_resume();
        let saved = (!completed)
            .then(crate::desktop::onboarding::saved_step)
            .flatten();
        (resume.is_some() || (!completed && (!status.ready() || saved.is_some()))).then(|| {
            Tour::new(
                Flow::new(
                    resume.or(saved).unwrap_or(Step::Welcome),
                    status,
                    platform.caps.permissions,
                    platform.caps.practice_dictation,
                ),
                completed,
            )
        })
    }
}

pub(super) fn progress(ui: &Ui) -> (bool, ModelProgress) {
    let installed = ui.picker.selected_installed(&ui.settings);
    let selected = crate::transcription::models::validate(&ui.settings.transcription).ok();
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    let downloading = selected.and_then(|model| {
        (transcription.and_then(|t| t.preparing) == Some(model.id)).then(|| {
            model.download_bytes().map_or(0.0, |bytes| {
                (transcription.map_or(0, |t| t.downloaded_bytes) as f32 / bytes as f32)
                    .clamp(0.0, 1.0)
            })
        })
    });
    (
        installed,
        ModelProgress {
            installed,
            downloading,
            failed: downloading.is_none() && transcription.is_some_and(|t| t.error.is_some()),
        },
    )
}

/// Feeds the flow fresh permission and model state; true when the tour
/// changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    // Onboarding looks for models already on this computer once, and holds
    // its own download until the scan answers; with one found, the person
    // chooses (`found_choice`), and nothing is picked for them.
    if ui.tour.is_some() {
        super::found::scan_for_setup(ui);
        let held = ui.found.scanning() || !ui.found_models.is_empty();
        if let Some(tour) = &mut ui.tour {
            tour.flow.hold_download(held);
        }
    }
    let (installed, model) = progress(ui);
    let status = ui.platform.setup_status(installed);
    let accessibility_stale = ui
        .platform
        .stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    let live = ui.live();
    let Some(tour) = &mut ui.tour else {
        return false;
    };
    let before = tour.flow.view();
    let step = before.step;
    let mut effect = tour.flow.on(Input::Status(status, model));
    tour.flow.set_accessibility_stale(accessibility_stale);
    if tour
        .advance_at
        .is_some_and(|at| at.elapsed() >= ADVANCE_BEAT)
    {
        tour.advance_at = None;
        if tour.flow.step() == Step::Setup {
            effect = tour.flow.on(Input::Continue);
        }
    }
    if status.ready() && !tour.completed {
        tour.completed = true;
        if live && let Err(error) = crate::desktop::onboarding::record_completion() {
            tracing::warn!(%error, "could not record onboarding completion");
        }
    }
    let refocused = focus_practice(tour, live);
    let practicing = tour.flow.step() == Step::TryIt;
    apply(ui, effect);
    let heard = ui.host.tour_inputs(practicing);
    let mut changed = refocused || !heard.is_empty();
    for heard in heard {
        input(ui, heard);
    }
    let peak = ui.host.practice_level();
    // Fixtures set the level they show.
    if let Some(tour) = ui.tour.as_mut().filter(|_| live) {
        let listening =
            tour.flow.step() == Step::TryIt && tour.flow.view().practice == Practice::Listening;
        changed |= tour
            .mic
            .hear(listening, peak, &ui.settings.microphone, Instant::now());
    }
    remember_step(ui, step);
    let after = ui.tour.as_ref().map(|tour| tour.flow.view());
    // Done waited for the model: the success chime sounds once it is ready.
    if ui.live()
        && before.step == Step::Done
        && !before.dictation_ready
        && after
            .as_ref()
            .is_some_and(|after| after.step == Step::Done && after.dictation_ready)
    {
        crate::feedback::play(crate::feedback::Tone::OnboardingSuccess);
    }
    changed || after.as_ref() != Some(&before)
}

pub(super) fn input(ui: &mut Ui, input: Input) {
    let live = ui.live();
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let step = tour.flow.step();
    let effect = tour.flow.on(input);
    focus_practice(tour, live);
    apply(ui, effect);
    remember_step(ui, step);
}

/// Saves a new step so a tour quit part-way resumes there, and sounds the
/// success chime on reaching the last one once dictation can run.
fn remember_step(ui: &Ui, before: Step) {
    if let Some(tour) = &ui.tour
        && tour.flow.step() != before
        && ui.live()
    {
        crate::desktop::onboarding::record_step(tour.flow.step());
        if tour.flow.step() == Step::Done && tour.flow.view().dictation_ready {
            crate::feedback::play(crate::feedback::Tone::OnboardingSuccess);
        }
    }
}

/// The practice dictation pastes into the focused field, so Try it keeps
/// the field focused until the dictation lands. Fixtures leave it alone.
/// True when it moved focus back to the field.
fn focus_practice(tour: &mut Tour, live: bool) -> bool {
    let view = tour.flow.view();
    let wanted = live && view.step == Step::TryIt && view.practice != Practice::Landed;
    let refocus = wanted && !tour.practice.focused;
    if refocus {
        tour.practice.focused = true;
    }
    refocus
}

fn apply(ui: &mut Ui, effect: Effect) {
    match effect {
        Effect::DownloadModel => {
            // The model onboarding's choice picked, else the recommended one.
            let selection = ui
                .onboarding_choice
                .take()
                .unwrap_or_else(|| ui.settings.transcription.clone());
            if crate::transcription::models::validate(&selection).is_ok() {
                ui.send(Request::ChooseModel {
                    model: selection.model,
                    language: selection.language,
                });
            }
        }
        Effect::AdvanceSoon => {
            if let Some(tour) = &mut ui.tour {
                tour.advance_at = Some(Instant::now());
            }
        }
        Effect::RequestMicrophone => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::RequestMicrophone,
        )),
        Effect::ShowGuide(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ShowGuide(kind),
        )),
        Effect::ResetPermission(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ResetAndPrompt(kind),
        )),
        Effect::Relaunch => ui.ask(PlatformRequest::Relaunch),
        Effect::Finish => ui.tour = None,
        Effect::None => {}
    }
}

/// Whether the tour shows the practice microphone, so the window keeps its
/// input list current.
pub(super) fn shows_microphone(ui: &Ui) -> bool {
    ui.tour
        .as_ref()
        .is_some_and(|tour| tour.flow.step() == Step::TryIt)
}

/// The tour over the whole window, while it shows. Its menus go in `layer`,
/// above it.
pub(super) fn overlay(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> Option<El> {
    let tour = ui.tour.as_ref()?;
    let view = tour.flow.view();
    let page = match view.step {
        Step::Welcome => welcome(ui, p),
        Step::Setup => setup(ui, p, &view),
        Step::TryIt => try_it(ui, p, tour, &view, layer),
        Step::Done => done(ui, p, &view),
    };
    let primary = match view.step {
        Step::Welcome => Some((tr("Get started"), true, Input::Continue)),
        Step::Setup if view.restart => Some((tr("Restart Hex"), true, Input::Restart)),
        Step::Setup => Some((tr("Continue"), view.ready, Input::Continue)),
        Step::TryIt if view.practice == Practice::Landed => {
            Some((tr("Continue"), true, Input::Continue))
        }
        Step::TryIt => None,
        Step::Done if !view.dictation_ready => Some((tr("Continue"), true, Input::Continue)),
        Step::Done => Some((tr("Start using Hex"), true, Input::Continue)),
    };
    // Until setup is ready the next setup row carries the accent, so
    // Continue waits, dimmed, in the accent's own shape.
    // Forward actions point onward; Restart and the last step don't.
    let onward = |label: &'static str, forward: bool| {
        let button = Button::new(label).variant(ButtonVariant::Contrast);
        if !forward {
            return button;
        }
        button.label_with(move |font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(text(label).key("label").font(font).color(ink))
                .child(
                    rune::icon(&glyphs::ARROW_RIGHT, 16.0)
                        .key("arrow")
                        .color(ink),
                )
        })
    };
    let forward = matches!(view.step, Step::Welcome | Step::TryIt)
        || (view.step == Step::Setup && !view.restart);
    let primary = match primary {
        Some((label, true, action)) => onward(label, forward)
            .on_press_tx(move |ui: &mut Ui, _| input(ui, action))
            .build_push(p.k)
            .key("tour-continue")
            .default_button(),
        Some((label, false, _)) => onward(label, forward)
            .disabled(true)
            .build_push(p.k)
            // Dark's light-grey accent still reads as live at Rune's 0.4.
            .opacity(if p.k.dark { 0.3 } else { 1.0 })
            .key("tour-continue"),
        None => button(
            p,
            "tour-skip",
            tr("Skip"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| input(ui, Input::Skip),
        ),
    };
    let back = view.can_go_back.then(|| {
        button(
            p,
            "tour-back",
            tr("Back"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| input(ui, Input::Back),
        )
    });
    let footer = row()
        .key("tour-footer")
        .w(WIDTH)
        .h(32.0)
        .center_y()
        .child(row().key("start").w(FOOTER_SLOT).children(back))
        .child(
            row()
                .key("middle")
                .grow(1.0)
                .w(0.0)
                .center()
                .children(step_dots(p, &view)),
        )
        .child(row().key("end").w(FOOTER_SLOT).justify_end().child(primary));
    // The download keeps one corner for the whole tour, clear of the
    // footer, where the traffic lights' row has room.
    let top = row()
        .key("tour-top")
        .w_full()
        .h(48.0)
        .px(20.0)
        .center_y()
        .justify_end()
        .children(
            (view.show_download && !waiting_for_model(&view) && !done_shows_progress(&view))
                .then(|| download(p, view.model)),
        );
    Some(
        col()
            .key("tour")
            .role("dialog")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(p.k.c.bg_deep)
            .on_press(|_: &mut Ui| {})
            .child(super::title_bar(top))
            .child(
                col()
                    .key("tour-page")
                    .grow(1.0)
                    .shrink()
                    .center()
                    .child(col().key("tour-column").w(WIDTH).child(page)),
            )
            .child(
                col()
                    .key("tour-bottom")
                    .pt(16.0)
                    .pb(28.0)
                    .center()
                    .child(footer),
            ),
    )
}

fn step_dots(p: Paint, view: &View) -> Option<El> {
    if view.step == Step::Done || view.count < 2 {
        return None;
    }
    let mut dots = row().key("step-dots").gap(6.0).center_y();
    for index in 0..view.count {
        let current = index == view.index;
        dots = dots.child(
            el().key(format!("dot{index}"))
                .size(if current { 16.0 } else { 6.0 }, 6.0)
                .radius(3.0)
                .fill(if current { p.text() } else { p.k.c.border_base }),
        );
    }
    Some(dots)
}

/// The model's progress in the tour's top corner, while it downloads
/// behind the other steps.
fn download(p: Paint, status: ItemStatus) -> El {
    let label = |key: &str, copy: &str| {
        text(copy)
            .key(key)
            .font(p.k.ty.small_medium)
            .color(p.muted())
    };
    match status {
        ItemStatus::Ready => allowed(p, "tour-download", tr("Speech model ready")),
        ItemStatus::Failed => row()
            .key("tour-download")
            .gap(8.0)
            .center_y()
            .child(label("label", tr("Download stopped")))
            .child(
                rune_kit::Button::new(tr("Retry"))
                    .on_press(|ui: &mut Ui| input(ui, Input::Grant(SetupItem::Model)))
                    .build_small_push(p.k)
                    .key("tour-download-retry"),
            ),
        ItemStatus::Downloading(fraction) => row()
            .key("tour-download")
            .gap(10.0)
            .center_y()
            .child(label("label", tr("Speech model")))
            .child(
                Progress::new(Some(fraction))
                    .build(p.k)
                    .key("progress")
                    .w(96.0),
            )
            .child(
                text(format!("{}%", (fraction * 100.0).round()))
                    .key("value")
                    .font(p.k.ty.small.features("'tnum' 1"))
                    .color(p.faint())
                    .w(30.0)
                    .align(rune::Align::End),
            ),
        _ => el().key("tour-download"),
    }
}

fn title(p: Paint, title: &str, body: Option<&str>) -> El {
    col()
        .key("title-block")
        .gap(10.0)
        .center()
        .child(text(title).key("title").font(TITLE).color(p.text()))
        .children(body.map(|body| {
            text(body)
                .key("body")
                .font(LEAD)
                .color(p.muted())
                .align(rune::Align::Center)
                .wrap()
                .w(440.0)
        }))
}

fn welcome(ui: &Ui, p: Paint) -> El {
    col()
        .key("welcome")
        .gap(36.0)
        .child(super::hero::stage())
        .child(title(
            p,
            tr("Welcome to Hex"),
            Some(if ui.platform.caps.os == Os::Macos {
                tr("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this Mac.")
            } else {
                tr("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this computer.")
            }),
        ))
}

fn setup(ui: &Ui, p: Paint, view: &View) -> El {
    let permissions = ui.platform.caps.permissions;
    let model_ready = view
        .rows
        .iter()
        .any(|row| row.item == SetupItem::Model && row.status == ItemStatus::Ready);
    // A compatible model on this computer: the person chooses between it
    // and the recommended download, which the model row would otherwise do.
    let choosing = super::found_choice::offered(ui, model_ready)
        && view.rows.iter().any(|row| {
            row.item == SetupItem::Model
                && matches!(row.status, ItemStatus::Todo | ItemStatus::Failed)
        });
    let rows: Vec<El> = view
        .rows
        .iter()
        .filter(|row| !(choosing && row.item == SetupItem::Model))
        .map(|row| setup_row(ui, p, *row))
        .collect();
    if choosing {
        let mut page = col().key("setup").gap(24.0).child(title(
            p,
            tr("Choose a speech model"),
            Some(tr("Hex found speech models already on this computer.")),
        ));
        if !rows.is_empty() {
            page = page.child(card(p, "setup-card", rows));
        }
        return page.child(super::found_choice::choice(ui, p));
    }
    let mut page = col()
        .key("setup")
        .gap(28.0)
        .child(if permissions {
            title(p, tr("Let Hex hear you and type for you"), None)
        } else if view
            .rows
            .iter()
            .any(|row| row.item == SetupItem::Microphone)
        {
            title(
                p,
                tr("Let Hex hear you"),
                Some(tr(
                    "Windows has the microphone turned off for desktop apps.",
                )),
            )
        } else {
            title(
                p,
                tr("Download a speech model"),
                Some(tr(
                    "Hex transcribes on this computer, so it needs a local model first.",
                )),
            )
        })
        .child(card(p, "setup-card", rows));
    if ui.platform.relaunch_failed {
        page = page.child(card(
            p,
            "relaunch-notice",
            vec![super::controls::note(
                "copy",
                tr(
                    "Quit and reopen Hex to finish setup. macOS applies some permissions only after a restart.",
                ),
                p.muted(),
            )],
        ));
    }
    page
}

fn setup_row(ui: &Ui, p: Paint, row: Row) -> El {
    let waiting = row.status == ItemStatus::Waiting;
    let model = crate::transcription::models::validate(&ui.settings.transcription).ok();
    let (key, glyph, title, description): (&str, &'static rune::Icon, &str, String) = match row.item
    {
        SetupItem::Microphone => (
            "setup-microphone",
            &glyphs::MIC,
            tr("Microphone"),
            match row.status {
                ItemStatus::Denied | ItemStatus::Waiting if ui.platform.caps.os == Os::Windows => {
                    tr("Turn on “Let desktop apps access your microphone” in Windows Settings.")
                        .into()
                }
                ItemStatus::Denied => {
                    tr("Turned off earlier. Switch Hex on in System Settings.").into()
                }
                ItemStatus::Waiting
                    if ui.platform.permissions.microphone == PermissionState::NeedsSettings =>
                {
                    tr("Switch Hex on in the list").into()
                }
                _ => tr("Hears you only while you hold the shortcut").into(),
            },
        ),
        SetupItem::Accessibility => (
            "setup-accessibility",
            &glyphs::CURSOR_TEXT,
            super::platform_settings::accessibility_name(ui.platform.accessibility_renamed),
            if row.status == ItemStatus::Restart {
                tr("Turns on when Hex restarts").into()
            } else if row.status == ItemStatus::Stale {
                tr("An older Hex entry can keep this off").into()
            } else if waiting {
                tr("Drag Hex into the list, or switch it on if it is there").into()
            } else {
                tr("Notices the shortcut and pastes your words in any app").into()
            },
        ),
        SetupItem::Model => (
            "setup-model",
            &glyphs::SPARKLE,
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
    let kind = if row.next {
        ButtonKind::Primary
    } else {
        ButtonKind::Secondary
    };
    let item = row.item;
    let grant = move |ui: &mut Ui| input(ui, Input::Grant(item));
    let control = if item == SetupItem::Model {
        match row.status {
            ItemStatus::Ready => allowed(p, "ready", tr("Ready")),
            ItemStatus::Downloading(fraction) => Progress::new(Some(fraction))
                .show_value()
                .build(p.k)
                .key("download")
                .w(160.0),
            ItemStatus::Failed => {
                button(p, "setup-grant", tr("Retry"), kind, move |ui, _| grant(ui))
            }
            _ => button(p, "setup-grant", tr("Download"), kind, move |ui, _| {
                grant(ui)
            }),
        }
    } else {
        permission_control(
            p,
            "setup-grant",
            match row.status {
                ItemStatus::Ready | ItemStatus::Restart => PermissionControl::Allowed,
                ItemStatus::Stale => PermissionControl::Stale,
                ItemStatus::Waiting => PermissionControl::Waiting,
                _ => PermissionControl::Allow,
            },
            kind,
            grant,
        )
    };
    icon_row(p, key, glyph, title, Some(&description), control)
}

/// Give it a try while dictation can't run yet: the speech model is still
/// downloading, or its download stopped. Permissions are ready by then.
fn waiting_for_model(view: &View) -> bool {
    view.step == Step::TryIt && !view.reopen && !view.dictation_ready
}

/// Done while the model still downloads: its progress shows in the ring,
/// not the corner. A stopped download keeps its Retry in the corner.
fn done_shows_progress(view: &View) -> bool {
    view.step == Step::Done
        && !view.dictation_ready
        && matches!(view.model, ItemStatus::Downloading(_))
}

fn holds(ui: &Ui) -> bool {
    ui.settings.shortcut_mode == ShortcutMode::Hold
}

fn try_it(ui: &Ui, p: Paint, tour: &Tour, view: &View, layer: &mut Vec<El>) -> El {
    let shortcut = ui.settings.dictation_hotkey.keycaps().join("");
    let field = TextInput::new(&tour.practice)
        .placeholder(tr("Your words will appear here"))
        .width(Some(WIDTH))
        .large()
        .on_edit(|ui: &mut Ui| {
            &mut ui
                .tour
                .as_mut()
                .expect("the practice field shows with the tour")
                .practice
        })
        .build(p.k)
        .key("practice-field");
    let (heading, body) = if waiting_for_model(view) {
        (
            tr("Almost ready"),
            tr("Hex starts listening once the speech model has downloaded.").to_string(),
        )
    } else if holds(ui) {
        (
            tr("Give it a try"),
            tr_fill("Hold {}, say a sentence, then let go.", &shortcut),
        )
    } else {
        (
            tr("Give it a try"),
            tr_fill("Press {}, say a sentence, then press it again.", &shortcut),
        )
    };
    col()
        .key("try-it")
        .gap(28.0)
        .child(title(p, heading, Some(&body)))
        .child(
            col().key("practice").gap(12.0).child(field).child(
                col()
                    .key("practice-status")
                    .gap(6.0)
                    .child(practice_hint(ui, p, view, &shortcut))
                    .child(microphone(ui, p, tour, layer)),
            ),
        )
}

/// One quiet line under the practice field: the microphone Hex records
/// from, as a small menu of every input, with its live level, and a short
/// warning when that microphone gives Hex nothing.
fn microphone(ui: &Ui, p: Paint, tour: &Tour, layer: &mut Vec<El>) -> El {
    let listed = ui.microphones.as_ref().ok();
    let off = listed.and_then(|devices| devices.off_with_lid_closed.as_deref());
    let saved =
        ui.settings.microphone.as_deref().filter(|saved| {
            off != Some(*saved) && super::settings::missing_microphone(ui).is_none()
        });
    let in_use = saved
        .or(listed.and_then(|devices| devices.system_default.as_deref()))
        .or(off);
    // With nothing else to fall back to, Hex records the closed lid's
    // switched-off microphone.
    let lid_closed = off.is_some() && in_use == off;
    let silent = tour.mic.silent.as_ref() == Some(&ui.settings.microphone);
    let warning = if lid_closed {
        Some(tr("The lid is closed, so the built-in mic is off."))
    } else if silent {
        Some(tr("No sound. Pick another mic?"))
    } else {
        None
    };
    let open = ui.open == Some(Menu::TourMicrophone);
    let name = in_use.unwrap_or(tr("System default"));
    let trigger = mic_trigger(p, name, tour.mic.level, warning.is_some(), open)
        .on_press(|ui: &mut Ui| super::settings::toggle_menu(ui, Menu::TourMicrophone));
    let mut menu = rune_kit::Menu::new("tour-mic")
        .trigger(trigger)
        .placement(rune_kit::Placement::BottomStart)
        .gutter(4.0)
        .min_width(super::controls::DEVICE_SELECT_WIDTH)
        .open(open)
        .on_close(|ui: &mut Ui| ui.open = None);
    for choice in super::settings::microphone_choices(ui) {
        let mut item = rune_kit::MenuItem::new(choice.label).checked(choice.selected);
        if let Some(detail) = choice.detail {
            item = item.shortcut(detail);
        }
        menu = menu.item(item.on_select(move |ui: &mut Ui| (choice.choose)(ui)));
    }
    row()
        .key("tour-mic")
        .h(24.0)
        .gap(6.0)
        .center_y()
        .child(menu.build(p.k, layer))
        .children(warning.map(|warning| mic_warning(p, warning)))
}

/// The microphone line's menu button: the live level and the name of the
/// microphone `name`, at rest and tinted while `warned`, as the
/// onboarding stories draw it too.
pub(super) fn mic_trigger(p: Paint, name: &str, level: f32, warned: bool, open: bool) -> El {
    let (level, tint) = if warned {
        (0.0, p.warning())
    } else {
        (level, p.faint())
    };
    let c = &p.k.c;
    let chevron = if open {
        p.k.glyph.chevron_up
    } else {
        p.k.glyph.chevron_down
    };
    let trigger = row()
        .key("tour-mic-field")
        .role("button")
        .label(tr_fill("Microphone, {}", name))
        .focusable(true)
        .focus_ring(1.0, 0.0, c.border_focus)
        .h(24.0)
        // The pill bleeds past the line's edge so the level sits on it.
        .ml(-6.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(p.k.radius.md)
        .child(mic_bars(p, level, tint))
        .child(
            text(name)
                .key("name")
                .font(DESCRIPTION)
                .color(if open { p.text() } else { p.muted() })
                .truncate()
                .shrink(),
        )
        .child(rune::icon(chevron, 12.0).key("chevron").color(c.icon_muted));
    if open {
        trigger.fill(c.hover)
    } else {
        trigger.hover(c.hover)
    }
}

/// The short warning beside the microphone when it gives Hex nothing.
pub(super) fn mic_warning(p: Paint, warning: &str) -> El {
    text(warning)
        .key("warning")
        .font(DESCRIPTION)
        .color(p.muted())
        .truncate()
        .shrink()
}

const MIC_LEVEL: f32 = 14.0;

/// Three short bars, as tall as Hex hears it.
pub(super) fn mic_bars(p: Paint, level: f32, tint: Color) -> El {
    const WEIGHTS: [f32; 3] = [0.6, 1.0, 0.75];
    let mut bars = row()
        .key("level")
        .size(MIC_LEVEL, MIC_LEVEL)
        .gap(2.0)
        .center();
    for (index, weight) in WEIGHTS.into_iter().enumerate() {
        // Uneven at rest too, so the bars never read as an ellipsis.
        let height = (2.0 + weight * 4.0 + level * weight * 8.0).round();
        bars = bars.child(
            el().key(format!("bar{index}"))
                .size(2.0, height)
                .radius(1.0)
                .fill(if level > 0.0 { p.text() } else { tint }),
        );
    }
    bars
}

fn practice_hint(ui: &Ui, p: Paint, view: &View, shortcut: &str) -> El {
    let hint = |copy: &str, color| {
        text(copy)
            .key("hint")
            .font(DESCRIPTION)
            .color(color)
            .wrap()
            .grow(1.0)
            .shrink()
    };
    // As tall as the keys' tray in every state, so the centred page never
    // jumps when the hint changes.
    let line = row()
        .key("practice-hint")
        .gap(8.0)
        .center_y()
        .min_h(KeySize::Small.height() + 2.0 * TRAY_PADDING);
    if view.reopen {
        return line
            .child(hint(
                tr("Hex needs to reopen before it can hear the shortcut."),
                p.muted(),
            ))
            .child(button(
                p,
                "tour-reopen",
                tr("Reopen Hex"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| input(ui, Input::Reopen),
            ));
    }
    // The heading and body say why the shortcut does nothing yet; the
    // download's progress, or its Retry, sits here instead of the corner.
    if waiting_for_model(view) {
        return line.child(download(p, view.model));
    }
    let (dot, copy, color) = match view.practice {
        Practice::Waiting => (
            false,
            if holds(ui) {
                tr("Hold to talk")
            } else {
                tr("Press to talk")
            }
            .into(),
            p.faint(),
        ),
        Practice::Listening => (
            true,
            if holds(ui) {
                tr_fill("Listening… let go of {} when you are done.", shortcut)
            } else {
                tr_fill("Listening… press {} again when you are done.", shortcut)
            },
            p.muted(),
        ),
        Practice::Transcribing => (false, tr("Transcribing…").into(), p.faint()),
        Practice::TooShort => (
            false,
            if holds(ui) {
                tr_fill(
                    "That was a quick tap. Keep holding {} while you speak.",
                    shortcut,
                )
            } else {
                tr("That was too short to hear. Speak a full sentence before you finish.").into()
            },
            p.muted(),
        ),
        Practice::NothingHeard => (
            false,
            tr("Hex did not catch any words. Speak a little closer, or pick another microphone below.").into(),
            p.muted(),
        ),
        Practice::Failed => (
            false,
            tr("Something went wrong while transcribing. Try once more.").into(),
            p.danger(),
        ),
        Practice::Landed => {
            return line.child(allowed(
                p,
                "landed",
                tr("That's it. Hex pastes wherever your cursor is, in any app."),
            ));
        }
    };
    // The shortcut's keys stay in one tray while it can be pressed, so a
    // release springs them back up: held down while it records.
    let down = matches!(view.practice, Practice::Listening) && holds(ui);
    let line = line.child(key_tray(
        p,
        "keys",
        KeySize::Small,
        ui.settings
            .dictation_hotkey
            .keycaps()
            .iter()
            .enumerate()
            .map(|(index, cap)| keycap(p, index, cap, KeySize::Small, down)),
    ));
    let line = if dot {
        line.child(super::controls::pulsing_dot(p.danger()))
    } else {
        line
    };
    line.child(hint(&copy, color))
}

fn done(ui: &Ui, p: Paint, view: &View) -> El {
    let ready = view.dictation_ready;
    let shortcut = ui.settings.dictation_hotkey.keycaps().join("+");
    let mut tips = Vec::new();
    if let Some(binding) = &ui.settings.paste_last_hotkey {
        tips.push(setting_row(
            p,
            "tip-paste-last",
            tr("Paste your last dictation"),
            Some(tr("Missed the text field? Paste it again anywhere.")),
            keycaps(p, "keys", &binding.keycaps()),
        ));
    }
    tips.push(setting_row(
        p,
        "tip-modes",
        tr("Modes"),
        Some(tr("Fix names, tone, and formatting per app.")),
        el().key("none"),
    ));
    let send_off = match ui.platform.caps.os {
        Os::Macos => tr_fill(
            "Hex waits in the menu bar. Use {} in any app to dictate.",
            &shortcut,
        ),
        Os::Windows => tr_fill(
            "Hex waits in the system tray. Use {} in any app to dictate.",
            &shortcut,
        ),
        Os::Linux => tr_fill(
            "Hex keeps running after you close this window. Use {} in any app to dictate.",
            &shortcut,
        ),
    };
    let c = &p.k.c;
    let badge = row().key("badge").size(72.0, 72.0).radius(36.0).center();
    // Until dictation can run, the badge stays neutral and shows how far
    // the speech model has downloaded.
    let badge = if ready {
        badge.fill(c.success.bg).child(
            rune::icon(p.k.glyph.check_small, 40.0)
                .key("check")
                .color(p.success()),
        )
    } else {
        let inner = match view.model {
            ItemStatus::Downloading(fraction) => text(format!("{}%", (fraction * 100.0).round()))
                .key("percent")
                .font(p.k.ty.ui_medium.features("'tnum' 1"))
                .color(p.muted()),
            _ => rune::icon(&glyphs::SPARKLE, 32.0)
                .key("model")
                .color(p.muted()),
        };
        badge.fill(c.bg_button_neutral).child(inner)
    };
    let art = row().key("art").h(ART_HEIGHT).w_full().center().child(
        row()
            .key("ring")
            .padding(16.0)
            .radius(52.0)
            .border(1.0, c.border_base.alpha(0.3))
            .child(badge),
    );
    let (heading, body) = if ready {
        (tr("You're all set"), send_off)
    } else {
        (
            tr("Almost ready"),
            tr("Hex starts listening once the speech model has downloaded.").to_string(),
        )
    };
    col()
        .key("done")
        .gap(24.0)
        .children((ui.platform.caps.os == Os::Macos).then_some(art))
        .child(title(p, heading, Some(&body)))
        .child(
            col()
                .key("more")
                .gap(super::controls::METRICS.label_gap)
                .child(
                    text(tr("When you want more"))
                        .key("label")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .ml(super::controls::METRICS.row_px),
                )
                .child(card(p, "tips-card", tips)),
        )
}

/// A first run at `step`, with the selected model not yet installed.
pub(super) fn fixture(ui: &mut Ui, step: Step, downloading: bool) {
    ui.picker.installed = Some(Vec::new());
    let status = ui.platform.setup_status(false);
    let mut flow = Flow::new(
        Step::Welcome,
        status,
        ui.platform.caps.permissions,
        ui.platform.caps.practice_dictation,
    );
    flow.hold_download(!ui.found_models.is_empty());
    if step != Step::Welcome {
        flow.on(Input::Continue);
    }
    ui.tour = Some(Tour::new(flow, false));
    if downloading {
        input(ui, Input::Grant(SetupItem::Model));
        set_downloaded(ui, 62);
        poll(ui);
    }
}

fn set_downloaded(ui: &mut Ui, percent: u64) {
    if let Some(view) = &mut ui.view {
        let model = ui.settings.transcription.model;
        let bytes = crate::transcription::models::definition(model)
            .download_bytes()
            .unwrap_or(0);
        view.transcription.preparing = Some(model);
        view.transcription.downloaded_bytes = bytes * percent / 100;
    }
}

/// The macOS tour's states, walked as a person would so the page dots and
/// the download match what they see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MacStep {
    Welcome,
    Setup,
    SetupProgress,
    SetupStale,
    SetupDenied,
    /// Every permission on, Accessibility waiting for a restart.
    SetupRestart,
    SetupDownloadFailed,
    TryIt,
    TryItListening,
    TryItLanded,
    TryItReopen,
    Done,
}

/// Walks a fresh macOS first run to `state`. `ui.platform` must carry
/// macOS capabilities.
pub(super) fn mac_fixture(ui: &mut Ui, state: MacStep) {
    use PermissionState::{NeedsRequest, NeedsRestart, NeedsSettings, Ready};
    let fresh = crate::desktop::onboarding::SetupStatus {
        microphone: NeedsRequest,
        accessibility: NeedsSettings,
        transcription_model: false,
    };
    let installed = matches!(
        state,
        MacStep::TryIt
            | MacStep::TryItListening
            | MacStep::TryItLanded
            | MacStep::TryItReopen
            | MacStep::Done
    );
    let permissions = match state {
        MacStep::Welcome | MacStep::Setup | MacStep::SetupDownloadFailed => fresh,
        MacStep::SetupProgress | MacStep::SetupStale => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            ..fresh
        },
        MacStep::SetupDenied => crate::desktop::onboarding::SetupStatus {
            microphone: NeedsSettings,
            ..fresh
        },
        MacStep::SetupRestart => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: NeedsRestart,
            ..fresh
        },
        _ => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: Ready,
            transcription_model: true,
        },
    };
    ui.platform.permissions = fresh;
    ui.picker.installed = Some(Vec::new());
    ui.tour = Some(Tour::new(
        Flow::new(Step::Welcome, fresh, true, true),
        false,
    ));
    // The tour starts the download as soon as it sees no model.
    poll(ui);
    if state != MacStep::Welcome {
        input(ui, Input::Continue);
    }
    if matches!(state, MacStep::SetupProgress | MacStep::SetupStale) {
        for item in [SetupItem::Microphone, SetupItem::Accessibility] {
            input(ui, Input::Grant(item));
        }
    }
    if state == MacStep::SetupStale {
        ui.platform
            .note_stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    }
    ui.platform.permissions = permissions;
    match state {
        MacStep::SetupDownloadFailed => {
            if let Some(view) = &mut ui.view {
                view.transcription.preparing = None;
                view.transcription.error = Some("The connection was reset.".into());
            }
        }
        _ if installed => {
            ui.picker.installed = Some(vec![ui.settings.transcription.model]);
            if let Some(view) = &mut ui.view {
                view.transcription.preparing = None;
            }
        }
        MacStep::Welcome => set_downloaded(ui, 8),
        _ => set_downloaded(ui, 31),
    }
    poll(ui);
    if installed {
        input(ui, Input::Continue);
    }
    if let Some(tour) = &mut ui.tour {
        tour.advance_at = None;
    }
    let observations: &[Observation] = match state {
        MacStep::TryItListening => &[Observation::Started],
        MacStep::TryItLanded => &[
            Observation::Started,
            Observation::Transcribing,
            Observation::Pasted,
        ],
        _ => &[],
    };
    for observation in observations {
        input(ui, Input::Dictation(*observation));
    }
    if state == MacStep::TryItLanded
        && let Some(tour) = &mut ui.tour
    {
        tour.practice = TextEdit::new("Let's move the standup to ten thirty tomorrow.");
    }
    if state == MacStep::TryItReopen {
        input(
            ui,
            Input::ListenerFailed {
                relaunch_allowed: false,
            },
        );
    }
    if state == MacStep::Done {
        input(ui, Input::Skip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_it_focuses_the_practice_field_until_the_dictation_lands() {
        let ready = crate::desktop::onboarding::SetupStatus {
            microphone: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: true,
        };
        let mut tour = Tour::new(Flow::new(Step::TryIt, ready, true, true), true);
        assert_eq!(tour.flow.step(), Step::TryIt);
        assert!(!focus_practice(&mut tour, false));
        assert!(!tour.practice.focused, "fixtures leave the field alone");
        assert!(focus_practice(&mut tour, true));
        assert!(tour.practice.focused);
        assert!(!focus_practice(&mut tour, true), "already focused");

        let mut tour = Tour::new(Flow::new(Step::TryIt, ready, true, true), true);
        for observation in [
            Observation::Started,
            Observation::Transcribing,
            Observation::Pasted,
        ] {
            tour.flow.on(Input::Dictation(observation));
        }
        assert_eq!(tour.flow.view().practice, Practice::Landed);
        assert!(!focus_practice(&mut tour, true));
        assert!(!tour.practice.focused, "a landed practice frees the field");
    }

    #[test]
    fn try_it_waits_for_the_model_until_it_is_ready() {
        let downloading = crate::desktop::onboarding::SetupStatus {
            microphone: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: false,
        };
        let mut flow = Flow::new(Step::TryIt, downloading, true, true);
        assert_eq!(flow.step(), Step::TryIt);
        assert!(waiting_for_model(&flow.view()));

        let ready = crate::desktop::onboarding::SetupStatus {
            transcription_model: true,
            ..downloading
        };
        flow.on(Input::Status(
            ready,
            ModelProgress {
                installed: true,
                ..ModelProgress::default()
            },
        ));
        assert!(!waiting_for_model(&flow.view()));
    }

    #[test]
    fn done_waits_for_the_model_after_a_skipped_practice() {
        let downloading = crate::desktop::onboarding::SetupStatus {
            microphone: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: false,
        };
        let progress = ModelProgress {
            downloading: Some(0.4),
            ..ModelProgress::default()
        };
        let mut flow = Flow::new(Step::TryIt, downloading, true, true);
        flow.on(Input::Status(downloading, progress));
        flow.on(Input::Skip);
        let view = flow.view();
        assert_eq!(view.step, Step::Done);
        assert!(!view.dictation_ready, "Done must not claim Hex is ready");
        assert!(done_shows_progress(&view));

        let ready = crate::desktop::onboarding::SetupStatus {
            transcription_model: true,
            ..downloading
        };
        flow.on(Input::Status(
            ready,
            ModelProgress {
                installed: true,
                ..ModelProgress::default()
            },
        ));
        let view = flow.view();
        assert!(view.dictation_ready);
        assert!(!done_shows_progress(&view));
    }

    #[test]
    fn mac_stale_fixture_marks_accessibility_stale_and_dispatches_reset() {
        let mut ui = crate::rune_ui::mac_fixture();
        mac_fixture(&mut ui, MacStep::SetupStale);
        let rows = ui.tour.as_ref().unwrap().flow.view().rows;
        assert!(
            rows.iter()
                .any(|row| row.item == SetupItem::Accessibility && row.status == ItemStatus::Stale)
        );
        input(&mut ui, Input::Grant(SetupItem::Accessibility));
        assert!(
            !ui.platform
                .stale(crate::desktop::onboarding::PermissionKind::Accessibility)
        );
        match &ui.host {
            crate::rune_ui::platform::PlatformHost::Fixture(sent) => {
                assert_eq!(
                    sent.last(),
                    Some(&PlatformRequest::Permission(
                        crate::desktop::onboarding::PermissionAction::ResetAndPrompt(
                            crate::desktop::onboarding::PermissionKind::Accessibility
                        )
                    ))
                );
            }
            _ => panic!("expected fixture host"),
        }
    }
}
