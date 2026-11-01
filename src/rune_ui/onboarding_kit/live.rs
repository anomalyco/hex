//! Hex's first run: the onboarding story, the one onboarding on every OS.
//! A new install (or an upgrade whose setup isn't ready) opens it at
//! launch, and Run setup again and `hex onboarding reset` replay it. The
//! window's tour ([`onboarding`](super::super::onboarding)) is the state
//! machine underneath: it starts the model download, asks for the
//! permissions, relaunches when macOS needs it, watches the practice
//! dictation, and records completion. This module shows the story's pages
//! over the window and feeds them that real state each frame.
//!
//! When the Hugging Face cache holds a compatible model, the download page
//! becomes the model step (`found_choice`): one list, Hex's recommended
//! model first and selected, then the models found. Picking ours downloads
//! it; picking a found one uses it in place, and the story moves on without
//! a download. Back on the next pages returns to the list. Nothing found
//! leaves the download page as it is, with the recommended model
//! downloading by itself.

use crate::desktop::i18n::tr;
use std::sync::OnceLock;
use std::time::Instant;

use rune::{El, TextEdit, Transition, Tx, text};

use super::super::controls::Paint;
use super::super::found_choice::{self, ChoiceRow};
use super::super::platform::Os;
use super::super::{Request, Ui, onboarding};
use super::fixture::{Language, Speech};
use super::pill::set_other_label;
use super::stories::{FRAME, Flow, Onboard, Step, advance_to};
use crate::desktop::onboarding::{
    Input, ItemStatus, Practice, SetupItem, SetupStatus, Step as TourStep,
};
use crate::transcription::models::{
    self as transcription_models, TranscriptionModelId, TranscriptionSelection,
};

/// How long both permission checks show before the page moves on.
const GRANTED_BEAT: f64 = 0.7;
/// How often the first run looks at the model on disk and the permissions.
const CHECK_EVERY: f64 = 0.25;

/// What the real first run reports to the story's pages.
pub(crate) struct Live {
    /// Hex's own dictation records.
    pub(crate) listening: bool,
    /// The model download stopped with an error.
    pub(crate) failed: bool,
    /// Why, in one line, such as a proxy or certificate (#894).
    pub(crate) failure: Option<String>,
    /// A model file chosen with Use a model file… is being added.
    pub(crate) using_file: bool,
    pub(crate) microphone: ItemStatus,
    pub(crate) accessibility: ItemStatus,
    /// Every permission is on and one applies only after Hex restarts.
    pub(crate) restart: bool,
    /// macOS asks for Microphone and Accessibility; Windows only shows the
    /// Microphone row while its privacy switch is off.
    pub(crate) accessibility_row: bool,
    /// macOS 27 calls Accessibility "Device Control and Data Access".
    pub(crate) accessibility_renamed: bool,
    /// The shortcut listener failed after setup and Hex won't relaunch by
    /// itself again: Reopen Hex.
    pub(crate) reopen: bool,
    /// macOS needed a restart and Hex couldn't relaunch itself.
    pub(crate) relaunch_failed: bool,
    /// The model step: the models found on this computer, while it shows.
    pub(crate) models: Vec<ChoiceRow>,
    /// Where the model step stands.
    pub(crate) model_step: ModelStep,
    /// The pick is under way: ours downloading, or a found model ready.
    pick_started: bool,
    /// The scan for models on this computer hasn't answered yet.
    scanning: bool,
    pub(crate) practice: Practice,
    /// The model and language this run downloads.
    choice: TranscriptionSelection,
    granted_at: Option<f64>,
    checked: Option<Checked>,
    /// Dictation streams send their live words to the practice field.
    _live_words: crate::dictation::live_text::Demand,
}

/// The model step, shown only when the scan found a compatible model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ModelStep {
    /// Nothing found (or not looked yet): the download page as always.
    #[default]
    None,
    /// The list shows and holds the story until Continue.
    Choosing,
    /// Continued from the list; Back returns to it.
    Chosen,
}

impl Live {
    /// The story waits on the download page for the model step.
    pub(crate) fn holds(&self) -> bool {
        self.scanning
            || match self.model_step {
                ModelStep::None => false,
                ModelStep::Choosing => true,
                ModelStep::Chosen => !self.pick_started,
            }
    }
}

/// The last look at the disk and macOS.
#[derive(Clone, Copy)]
struct Checked {
    at: f64,
    installed: bool,
    status: SetupStatus,
}

impl Live {
    pub(crate) fn granted_for(&self, now: f64) -> bool {
        self.granted_at.is_some_and(|at| now - at >= GRANTED_BEAT)
    }
}

/// Where a fixture's clock starts.
const FIXTURE_START: f64 = 100.0;

/// Seconds on the first run's own clock. Fixtures keep the story's own
/// time, so a scene shows one settled moment.
fn clock(ui: &Ui) -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    if ui.is_fixture() {
        return ui.first_run.as_ref().map_or(FIXTURE_START, |s| s.now);
    }
    START.get_or_init(Instant::now).elapsed().as_secs_f64() + 1.0
}

fn english(selection: &TranscriptionSelection) -> bool {
    selection.model == TranscriptionModelId::ParakeetUnifiedEnglish
}

/// The selection the language capsule switches to.
fn other_selection(current: &TranscriptionSelection) -> TranscriptionSelection {
    if english(current) {
        let v3 = transcription_models::definition(TranscriptionModelId::ParakeetV3);
        let preferred = crate::desktop::i18n::preferred_language();
        let language = if preferred != "en" && v3.supports_language(&preferred) {
            preferred
        } else {
            "fr".to_owned()
        };
        TranscriptionSelection {
            model: TranscriptionModelId::ParakeetV3,
            language,
        }
    } else {
        TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        }
    }
}

fn installed(ui: &Ui, selection: &TranscriptionSelection) -> bool {
    transcription_models::validate(selection)
        .is_ok_and(|model| ui.picker.installed(model, &selection.language))
}

/// Opens the first run over the tour, at the tour's step: at launch when
/// this run needs onboarding, and from Run setup again.
pub(in crate::rune_ui) fn open(ui: &mut Ui) {
    let Some(tour_step) = ui.tour.as_ref().map(|tour| tour.flow.step()) else {
        return;
    };
    if ui.first_run.is_some() {
        return;
    }
    let choice = ui.settings.transcription.clone();
    let wants = if english(&choice) {
        Language::English
    } else {
        Language::Other
    };
    let other = if english(&choice) {
        other_selection(&choice)
    } else {
        choice.clone()
    };
    set_other_label(transcription_models::language_name(&other.language));
    let had_model = installed(ui, &choice);
    let status = ui.platform.setup_status(had_model);
    let granted = status.microphone == crate::desktop::onboarding::PermissionState::Ready
        && status.accessibility == crate::desktop::onboarding::PermissionState::Ready;

    let mut s = Onboard::new(Flow::FirstRun);
    s.speech = Speech::AfterDownload;
    s.wants = wants;
    s.caps = ui.settings.dictation_hotkey.keycaps();
    s.had_model = had_model;
    s.granted = granted;
    s.home = match ui.platform.caps.os {
        Os::Macos => super::landing::Home::MenuBar,
        Os::Windows => super::landing::Home::Tray,
        Os::Linux => super::landing::Home::Launcher,
    };
    s.live = Some(Live {
        listening: false,
        failed: false,
        failure: None,
        using_file: false,
        microphone: ItemStatus::Todo,
        accessibility: ItemStatus::Todo,
        restart: false,
        accessibility_row: ui.platform.caps.permissions,
        accessibility_renamed: ui.platform.accessibility_renamed,
        reopen: false,
        relaunch_failed: false,
        models: Vec::new(),
        model_step: ModelStep::None,
        pick_started: false,
        scanning: false,
        practice: Practice::Waiting,
        choice,
        granted_at: None,
        checked: None,
        _live_words: crate::dictation::live_text::demand(),
    });
    let now = clock(ui);
    s.now = now;
    match tour_step {
        TourStep::Welcome => s.begin_live(now),
        TourStep::Setup => s.jump_to_permissions(),
        TourStep::TryIt if had_model => s.jump_to_try(),
        TourStep::TryIt => s.jump_to_waiting(),
        TourStep::Done => s.jump_to_ready(),
    }
    ui.first_run = Some(s);
    // The tour waits at setup, where it asks for permissions, while the
    // story plays; with everything granted it goes on to the practice.
    if tour_step == TourStep::Welcome {
        onboarding::input(ui, Input::Continue);
    }
}

/// A page of the first run, for scenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::rune_ui) enum Page {
    /// `"Setting up Hex"`, or the model step when models were found.
    Download,
    /// The demo, finished: `"Twice as fast"` and Continue.
    Demo,
    Permissions,
    /// `"Almost ready"`: the model still downloads.
    Waiting,
    /// `"Your turn"`: the practice game.
    Try,
    Ready,
}

/// Scenes: the first run over the tour a fixture walked, played on its own
/// clock to `page` as a person would see it, then held there.
pub(in crate::rune_ui) fn fixture(ui: &mut Ui, page: Page) {
    ui.first_run = None;
    open(ui);
    if let Some(s) = ui.first_run.as_mut() {
        // From the start, as a new install plays it, whatever the tour's step.
        s.now = FIXTURE_START;
        s.step = Step::Pick;
        s.begin_live(FIXTURE_START);
    }
    fixture_resume(ui, page);
}

/// Scenes: plays a fixture's first run on from where it is to `page`.
pub(in crate::rune_ui) fn fixture_resume(ui: &mut Ui, page: Page) {
    // One poll fills in what the tour reports.
    let mut tx = Tx::default();
    poll(ui, &mut tx);
    let Some(s) = ui.first_run.as_mut() else {
        return;
    };
    let reached = |s: &Onboard| match page {
        Page::Download => matches!(s.step, Step::Pick | Step::Started),
        Page::Demo => s.step == Step::Done,
        Page::Permissions => s.step == Step::Permissions,
        Page::Waiting => s.step == Step::Waiting,
        Page::Try => s.step == Step::Try,
        Page::Ready => s.step == Step::Ready,
    };
    if page == Page::Ready {
        s.jump_to_ready();
    }
    for _ in 0..64 {
        if reached(s) {
            break;
        }
        let Some(next) = super::stories::first_run_next(s, s.now) else {
            break;
        };
        s.now += next;
    }
    if page == Page::Try {
        super::game::step(s, s.now);
    }
    // Everything that entered has settled; nothing more is due.
    s.now += 10.0;
    s.auto_due_at = None;
    poll(ui, &mut tx);
}

/// "Run setup again" and `hex onboarding reset`: forgets that onboarding
/// finished and opens it now. Settings, history, models, and permissions
/// stay as they are, so the run finds them ready and passes their steps.
pub(crate) fn restart(ui: &mut Ui, tx: &mut Tx) -> Result<(), String> {
    if ui.live() {
        crate::desktop::onboarding::reset().map_err(|error| format!("{error:#}"))?;
    }
    ui.select_pane(super::super::Pane::General, tx);
    ui.first_run = None;
    ui.tour = Some(onboarding::Tour::replay(&ui.settings, &ui.platform));
    open(ui);
    tx.settle();
    if ui.first_run.is_none() {
        return Err("Hex could not open onboarding".into());
    }
    Ok(())
}

/// Feeds the first run fresh state and moves it on; true while it shows.
pub(in crate::rune_ui) fn poll(ui: &mut Ui, tx: &mut Tx) -> bool {
    if ui.first_run.is_none() {
        return false;
    }
    let now = clock(ui);
    let view = ui.tour.as_ref().map(|tour| tour.flow.view());
    let tour_model = view.as_ref().map_or(ItemStatus::Todo, |view| view.model);
    let offered = found_choice::offered(ui, tour_model);
    let models = if offered {
        found_choice::rows(ui, tour_model)
    } else {
        Vec::new()
    };
    let started = found_choice::started(ui, tour_model);
    let scanning = ui.found.scanning();
    let caps = ui.settings.dictation_hotkey.keycaps();
    let Some(choice) = ui
        .first_run
        .as_ref()
        .and_then(|s| s.live.as_ref())
        .map(|live| live.choice.clone())
    else {
        return false;
    };
    let model = transcription_models::validate(&choice).ok();
    let (preparing, downloaded_bytes, errored) =
        ui.view.as_ref().map_or((false, 0, false), |view| {
            let t = &view.transcription;
            (
                model.is_some_and(|m| t.preparing == Some(m.id)),
                t.downloaded_bytes,
                t.error.is_some(),
            )
        });
    let failure = super::super::model_import::download_failure(ui);
    let using_file = super::super::model_import::using_file(ui);
    // The disk and macOS checks run a few times a second, not every frame.
    let checked = ui
        .first_run
        .as_ref()
        .and_then(|s| s.live.as_ref())
        .and_then(|live| live.checked)
        // A fixture's clock stands still, so it looks every time.
        .filter(|checked| !ui.is_fixture() && now - checked.at < CHECK_EVERY);
    let checked = match checked {
        Some(checked) => checked,
        None => {
            let installed = installed(ui, &choice);
            Checked {
                at: now,
                installed,
                status: ui.platform.setup_status(installed),
            }
        }
    };
    // A model picked from the list is the one Hex uses, whatever the
    // language capsule last chose.
    let (choice_installed, status) = (
        checked.installed || tour_model == ItemStatus::Ready,
        checked.status,
    );
    // Setup that is already ready never moves the tour past it by itself,
    // and the tour watches the practice dictation only once it has.
    if status.ready()
        && ui
            .tour
            .as_ref()
            .is_some_and(|tour| tour.flow.step() == TourStep::Setup)
    {
        onboarding::input(ui, Input::Continue);
    }
    let asks_permissions = ui.platform.caps.permissions;
    let accessibility_renamed = ui.platform.accessibility_renamed;
    let relaunch_failed = ui.platform.relaunch_failed;
    let live_window = !ui.is_fixture();
    let Some(s) = ui.first_run.as_mut() else {
        return false;
    };
    let Some(live) = s.live.as_mut() else {
        return false;
    };
    live.checked = Some(checked);
    // Linux and Windows ask for no permissions: both rows are ready.
    if !asks_permissions {
        live.microphone = ItemStatus::Ready;
        live.accessibility = ItemStatus::Ready;
    }

    // Permissions, as the tour's setup rows show them.
    if let Some(view) = &view {
        for row in &view.rows {
            match row.item {
                SetupItem::Microphone => live.microphone = row.status,
                SetupItem::Accessibility => live.accessibility = row.status,
                _ => {}
            }
        }
        live.restart = view.restart;
        live.accessibility_renamed = accessibility_renamed;
        live.relaunch_failed = relaunch_failed;
        live.reopen = view.reopen;
        live.practice = view.practice;
    }

    // The model step: shown on the download page once the scan found
    // something, and held there until Continue.
    live.models = models;
    live.pick_started = started;
    live.scanning = scanning;
    let downloading_page = matches!(s.step, Step::Pick | Step::Started);
    match live.model_step {
        ModelStep::None if offered && downloading_page => live.model_step = ModelStep::Choosing,
        ModelStep::Choosing if !offered => live.model_step = ModelStep::None,
        // A pick that failed asks again.
        ModelStep::Chosen if downloading_page && tour_model == ItemStatus::Failed => {
            live.model_step = ModelStep::Choosing;
        }
        _ => {}
    }
    if live.model_step == ModelStep::Chosen && downloading_page {
        // A model already on this computer needs no download pill.
        s.had_model = tour_model == ItemStatus::Ready;
    }
    let granted = status.microphone == crate::desktop::onboarding::PermissionState::Ready
        && status.accessibility == crate::desktop::onboarding::PermissionState::Ready;
    if granted {
        live.granted_at.get_or_insert(now);
    } else {
        live.granted_at = None;
    }
    live.listening = live.practice == Practice::Listening;

    // The model download.
    live.failed = !choice_installed && !preparing && (errored || failure.is_some());
    live.failure = failure;
    live.using_file = using_file;
    s.granted = granted;
    s.caps = caps;
    if let Some(download) = s.download.as_mut() {
        if let Some(total) = model.and_then(|m| m.download_bytes()) {
            download.total = total;
        }
        if choice_installed {
            download.downloaded = download.total;
        } else if preparing {
            download.downloaded = downloaded_bytes.min(download.total.saturating_sub(1));
        }
    }

    // Hex's state arrives between frames, so the first run keeps polling;
    // a fixture shows one moment and stays still.
    advance_to(s, now, tx);
    if live_window {
        tx.wake_after(FRAME);
    }
    true
}

/// The first run over the whole window, while it shows.
pub(in crate::rune_ui) fn overlay(ui: &Ui, p: Paint, reduced: bool) -> Option<El> {
    let s = ui.first_run.as_ref()?;
    s.reduced.set(reduced);
    Some(
        super::flows::view(p, s)
            .key("first-run")
            .role("dialog")
            .absolute(0.0, 0.0)
            .on_press(|_: &mut Ui| {}),
    )
}

/// A story handler run on the window's first run.
pub(super) fn on_ui(f: fn(&mut Onboard)) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| {
        if let Some(s) = ui.first_run.as_mut() {
            f(s);
        }
    }
}

pub(super) fn press_continue(ui: &mut Ui) {
    if let Some(s) = ui.first_run.as_mut() {
        s.move_on();
    }
}

/// "Start using Hex": closes the first run and the tour under it.
pub(super) fn finish(ui: &mut Ui) {
    let installed = ui.picker.selected_installed(&ui.settings);
    if ui.live()
        && ui.platform.setup_status(installed).ready()
        && !crate::desktop::onboarding::completion_recorded()
        && let Err(error) = crate::desktop::onboarding::record_completion()
    {
        tracing::warn!(%error, "could not record onboarding completion");
    }
    ui.first_run = None;
    ui.tour = None;
}

fn choose(ui: &mut Ui, selection: &TranscriptionSelection) {
    if transcription_models::validate(selection).is_ok() {
        ui.send(Request::ChooseModel {
            model: selection.model,
            language: selection.language.clone(),
        });
    }
}

pub(super) fn cycle_language(ui: &mut Ui) {
    let Some(s) = ui.first_run.as_mut() else {
        return;
    };
    s.cycle_language();
    let Some(live) = s.live.as_mut() else {
        return;
    };
    live.choice = other_selection(&live.choice);
    live.failed = false;
    live.checked = None;
    let choice = live.choice.clone();
    choose(ui, &choice);
}

/// Picks row `index` of the model step's list and starts it.
pub(super) fn pick_model(index: usize) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| found_choice::pick(ui, index)
}

/// Continue on the model step: starts the selected row if none was picked,
/// and the story moves on once the pick is under way.
pub(in crate::rune_ui) fn continue_model(ui: &mut Ui) {
    let model = ui
        .tour
        .as_ref()
        .map_or(ItemStatus::Todo, |tour| tour.flow.view().model);
    if found_choice::pending(ui, model) {
        found_choice::start_selected(ui);
    }
    if let Some(live) = ui.first_run.as_mut().and_then(|s| s.live.as_mut()) {
        live.model_step = ModelStep::Chosen;
    }
    if let Some(s) = ui.first_run.as_mut() {
        s.move_on();
    }
}

/// Back to the model step, to change the pick.
pub(super) fn back_to_models(ui: &mut Ui) {
    if let Some(s) = ui.first_run.as_mut() {
        s.back_to_download();
        if let Some(live) = s.live.as_mut() {
            live.model_step = ModelStep::Choosing;
        }
    }
}

/// "Reopen Hex" once the listener failed and Hex won't relaunch itself.
pub(super) fn reopen(ui: &mut Ui) {
    onboarding::input(ui, Input::Reopen);
}

/// "Use a model file…" when the download is blocked: a GGUF chosen here is
/// imported and selected, and the first run moves on once it is ready.
pub(super) fn use_file(ui: &mut Ui) {
    super::super::model_import::use_file(ui);
}

pub(super) fn retry_download(ui: &mut Ui) {
    let Some(live) = ui.first_run.as_mut().and_then(|s| s.live.as_mut()) else {
        return;
    };
    live.failed = false;
    let choice = live.choice.clone();
    choose(ui, &choice);
}

/// Allow on a permission row.
pub(super) fn allow(item: SetupItem) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| onboarding::input(ui, Input::Grant(item))
}

/// "Restart Hex" once macOS applies a grant only after a restart.
pub(super) fn restart_hex(ui: &mut Ui) {
    onboarding::input(ui, Input::Restart);
}

pub(super) fn practice(ui: &mut Ui) -> &mut TextEdit {
    match ui.first_run.as_mut() {
        Some(s) => &mut s.practice,
        // The field shows only with the first run.
        None => Box::leak(Box::default()),
    }
}

pub(super) fn practice_changed(ui: &mut Ui, tx: &mut Tx) {
    if let Some(s) = ui.first_run.as_mut() {
        s.on_practice_change(tx);
    }
}

/// What the practice dictation's last try needs said.
pub(super) fn hint_copy(live: &Live) -> Option<&'static str> {
    Some(match live.practice {
        Practice::TooShort => tr("Hold the shortcut a little longer while you speak."),
        Practice::NothingHeard => tr("Hex didn't hear any words. Try again a little louder."),
        Practice::Failed => tr("That didn't work. Try again."),
        Practice::Transcribing => tr("Typing what you said…"),
        _ => return None,
    })
}

/// [`hint_copy`] under the practice field.
pub(super) fn hint(p: Paint, live: &Live) -> Option<El> {
    let copy = hint_copy(live)?;
    Some(
        text(copy)
            .key("practice-hint")
            .font(p.k.ty.small)
            .color(p.muted())
            .transition(Transition::opacity()),
    )
}
