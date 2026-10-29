//! First-run setup state. macOS gates dictation on three privacy
//! permissions; Windows and Linux have no such grants, so their permission
//! states are always ready and only the transcription model step applies.

use std::fs::{self, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2::msg_send;
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyClass, Bool};
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;

use crate::transcription_models::TranscriptionSelection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionState {
    Ready,
    NeedsRequest,
    NeedsSettings,
    /// Switched on in System Settings, but macOS applies it to Hex only
    /// after Hex restarts.
    NeedsRestart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionKind {
    Microphone,
    Accessibility,
    /// System Audio Recording, asked only when Meetings is on.
    SystemAudio,
    /// Screen Recording, asked only from the Scrawl pane or at a first
    /// capture, never in the tour.
    ScreenRecording,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionAction {
    /// macOS shows its own prompt the first time.
    RequestMicrophone,
    /// Everything else is turned on in System Settings, with the guide.
    ShowGuide(PermissionKind),
    /// Granted, waiting for Hex to restart.
    Restart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PermissionWarning {
    pub kind: PermissionKind,
    pub action: PermissionAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupStatus {
    pub microphone: PermissionState,
    pub accessibility: PermissionState,
    pub transcription_model: bool,
}

impl SetupStatus {
    pub fn ready(self) -> bool {
        self.microphone == PermissionState::Ready
            && self.accessibility == PermissionState::Ready
            && self.transcription_model
    }
}

pub fn permission_warnings(status: SetupStatus) -> Vec<PermissionWarning> {
    let mut warnings = Vec::with_capacity(2);
    match status.microphone {
        PermissionState::Ready => {}
        PermissionState::NeedsRequest => warnings.push(PermissionWarning {
            kind: PermissionKind::Microphone,
            action: PermissionAction::RequestMicrophone,
        }),
        PermissionState::NeedsSettings => warnings.push(PermissionWarning {
            kind: PermissionKind::Microphone,
            action: PermissionAction::ShowGuide(PermissionKind::Microphone),
        }),
        PermissionState::NeedsRestart => warnings.push(PermissionWarning {
            kind: PermissionKind::Microphone,
            action: PermissionAction::Restart,
        }),
    }
    match status.accessibility {
        PermissionState::Ready => {}
        PermissionState::NeedsRestart => warnings.push(PermissionWarning {
            kind: PermissionKind::Accessibility,
            action: PermissionAction::Restart,
        }),
        _ => warnings.push(PermissionWarning {
            kind: PermissionKind::Accessibility,
            action: PermissionAction::ShowGuide(PermissionKind::Accessibility),
        }),
    }
    warnings
}

/// The first-run tour, in order. `Setup` holds every permission on one
/// screen; `TryIt` is a practice dictation into a field in the window. The
/// speech model downloads in the background from the moment the tour opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Welcome,
    Setup,
    TryIt,
    Done,
}

impl Step {
    const TOUR: [Self; 4] = [Self::Welcome, Self::Setup, Self::TryIt, Self::Done];

    fn position(self) -> usize {
        Self::TOUR
            .iter()
            .position(|step| *step == self)
            .unwrap_or(0)
    }
}

/// One row of the setup screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupItem {
    Microphone,
    Accessibility,
    Model,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemStatus {
    Ready,
    /// Not asked yet.
    Todo,
    /// Asked; macOS's prompt or System Settings is up. The button reopens it.
    Waiting,
    /// Refused once: only System Settings can turn it on.
    Denied,
    /// Switched on; macOS applies it once Hex restarts.
    Restart,
    /// Downloading, with progress from zero through one.
    Downloading(f32),
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelProgress {
    pub installed: bool,
    pub downloading: Option<f32>,
    pub failed: bool,
}

/// What a practice dictation looks like from the event log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Observation {
    Started,
    Transcribing,
    Discarded,
    Cancelled,
    Pasted,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Practice {
    #[default]
    Waiting,
    Listening,
    Transcribing,
    /// Released before Hex kept any audio, or the clip was silent.
    TooShort,
    /// Transcribed, but no words came back.
    NothingHeard,
    Landed,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Continue,
    Back,
    /// "Skip" on the practice step.
    Skip,
    Grant(SetupItem),
    Status(SetupStatus, ModelProgress),
    Dictation(Observation),
    /// The shortcut listener could not start after setup became ready.
    ListenerFailed {
        relaunch_allowed: bool,
    },
    Reopen,
    /// "Restart Hex" once every permission is on and one waits for it.
    Restart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    None,
    RequestMicrophone,
    /// Open the Privacy pane with the guide beside it: drag Hex into the
    /// list, or (for a refused microphone) switch it on.
    ShowGuide(PermissionKind),
    DownloadModel,
    /// Every permission just turned on: move to the next step after a beat,
    /// so the last check is seen.
    AdvanceSoon,
    /// Quit and reopen Hex, resuming the tour: at the practice step, or
    /// at setup while setup is unfinished.
    Relaunch,
    /// Close the tour.
    Finish,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub item: SetupItem,
    pub status: ItemStatus,
    /// The first row still to do; its button carries the screen's accent.
    pub next: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub step: Step,
    /// Position among the dotted steps (every step but `Done`).
    pub index: usize,
    pub count: usize,
    pub rows: Vec<Row>,
    /// Every setup row is ready.
    pub ready: bool,
    /// Dictation can run: permissions and the model are ready.
    pub dictation_ready: bool,
    pub model: ItemStatus,
    /// The tour downloaded the model, so its progress is worth showing
    /// outside the setup rows.
    pub show_download: bool,
    pub practice: Practice,
    pub can_go_back: bool,
    /// The listener needs a restart Hex will not do by itself again.
    pub reopen: bool,
    /// Every permission is on, and at least one applies only after Hex
    /// restarts: setup's primary action restarts Hex.
    pub restart: bool,
}

#[derive(Clone, Debug)]
pub struct Flow {
    step: Step,
    status: SetupStatus,
    model: ModelProgress,
    permissions: bool,
    practice_step: bool,
    /// Microphone, then Accessibility.
    asked: [bool; 2],
    /// A refused microphone's System Settings guide is up.
    microphone_guide: bool,
    practice: Practice,
    /// The setup screen keeps its page dot once it has been needed.
    setup_needed: bool,
    download_requested: bool,
    advance_due: bool,
    reopen: bool,
    relaunching: bool,
}

impl Flow {
    /// `permissions` is false where the OS has no privacy grants; `practice`
    /// is false where this window cannot observe dictation.
    pub fn new(start: Step, status: SetupStatus, permissions: bool, practice: bool) -> Self {
        let mut flow = Self {
            step: Step::Welcome,
            status,
            model: ModelProgress {
                installed: status.transcription_model,
                ..ModelProgress::default()
            },
            permissions,
            practice_step: practice,
            asked: [false; 2],
            microphone_guide: false,
            practice: Practice::Waiting,
            setup_needed: false,
            download_requested: false,
            advance_due: false,
            reopen: false,
            relaunching: false,
        };
        flow.setup_needed = !flow.setup_ready();
        flow.step = if start == Step::Setup
            || start.position() > Step::Setup.position() && !flow.setup_ready()
        {
            Step::Setup
        } else {
            flow.forward_from(start)
        };
        flow
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn on(&mut self, input: Input) -> Effect {
        match input {
            Input::Continue => self.primary(),
            Input::Back => {
                self.step = self.back_from(self.step);
                Effect::None
            }
            Input::Skip if self.step == Step::TryIt => self.advance(),
            Input::Skip => Effect::None,
            Input::Grant(item) => self.grant(item),
            Input::Status(status, model) => self.update(status, model),
            Input::Dictation(observation) => {
                if self.step == Step::TryIt {
                    self.practice = next_practice(self.practice, observation);
                }
                Effect::None
            }
            Input::ListenerFailed { relaunch_allowed } => {
                if relaunch_allowed {
                    self.relaunch()
                } else {
                    self.reopen = true;
                    Effect::None
                }
            }
            Input::Reopen if self.reopen => self.relaunch(),
            Input::Reopen => Effect::None,
            Input::Restart if self.step == Step::Setup && self.restart_needed() => {
                self.relaunching = true;
                Effect::Relaunch
            }
            Input::Restart => Effect::None,
        }
    }

    pub fn view(&self) -> View {
        let dotted: Vec<Step> = Step::TOUR
            .into_iter()
            .filter(|step| *step != Step::Done && self.shows(*step))
            .collect();
        let rows = self.rows();
        View {
            step: self.step,
            index: dotted
                .iter()
                .position(|step| *step == self.step)
                .unwrap_or(dotted.len()),
            count: dotted.len(),
            rows,
            ready: self.setup_ready(),
            dictation_ready: self.status.ready(),
            model: self.model_status(),
            show_download: self.download_requested && !self.model_in_setup(),
            practice: self.practice,
            can_go_back: self.step != Step::Welcome
                && self.step != Step::Done
                && self.back_from(self.step) != self.step,
            reopen: self.reopen,
            restart: self.restart_needed(),
        }
    }

    fn restart_needed(&self) -> bool {
        let states = [self.status.microphone, self.status.accessibility];
        !self.relaunching
            && states.contains(&PermissionState::NeedsRestart)
            && states.iter().all(|state| {
                matches!(
                    state,
                    PermissionState::Ready | PermissionState::NeedsRestart
                )
            })
    }

    /// Where the practice step can wait for the model, the model is not a
    /// setup row: it downloads behind the permissions.
    fn model_in_setup(&self) -> bool {
        !self.permissions || !self.practice_step
    }

    fn permissions_ready(&self) -> bool {
        self.status.microphone == PermissionState::Ready
            && self.status.accessibility == PermissionState::Ready
    }

    fn setup_ready(&self) -> bool {
        self.permissions_ready() && (!self.model_in_setup() || self.status.transcription_model)
    }

    fn model_status(&self) -> ItemStatus {
        if self.status.transcription_model {
            ItemStatus::Ready
        } else if let Some(progress) = self.model.downloading {
            ItemStatus::Downloading(progress)
        } else if self.model.failed {
            ItemStatus::Failed
        } else if self.download_requested {
            ItemStatus::Downloading(0.0)
        } else {
            ItemStatus::Todo
        }
    }

    fn update(&mut self, status: SetupStatus, model: ModelProgress) -> Effect {
        let was_ready = self.setup_ready();
        self.status = status;
        self.model = model;
        if self.step == Step::Setup && !was_ready && self.setup_ready() {
            self.advance_due = true;
        }
        // The download starts the first time the tour sees no model, before
        // any permission is asked, so it runs while the person reads.
        if !status.transcription_model
            && model.downloading.is_none()
            && !model.failed
            && !self.download_requested
        {
            self.download_requested = true;
            return Effect::DownloadModel;
        }
        if std::mem::take(&mut self.advance_due) && self.step == Step::Setup {
            return Effect::AdvanceSoon;
        }
        Effect::None
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::with_capacity(4);
        if self.permissions {
            rows.push(Row {
                item: SetupItem::Microphone,
                status: match self.status.microphone {
                    PermissionState::Ready => ItemStatus::Ready,
                    PermissionState::NeedsSettings if self.microphone_guide => ItemStatus::Waiting,
                    PermissionState::NeedsSettings => ItemStatus::Denied,
                    PermissionState::NeedsRequest if self.asked[0] => ItemStatus::Waiting,
                    PermissionState::NeedsRequest => ItemStatus::Todo,
                    PermissionState::NeedsRestart => ItemStatus::Restart,
                },
                next: false,
            });
            rows.push(Row {
                item: SetupItem::Accessibility,
                status: match self.status.accessibility {
                    PermissionState::Ready => ItemStatus::Ready,
                    PermissionState::NeedsRestart => ItemStatus::Restart,
                    _ if self.asked[1] => ItemStatus::Waiting,
                    _ => ItemStatus::Todo,
                },
                next: false,
            });
        }
        if self.model_in_setup() {
            rows.push(Row {
                item: SetupItem::Model,
                status: self.model_status(),
                next: false,
            });
        }
        if let Some(row) = rows.iter_mut().find(|row| {
            !matches!(
                row.status,
                ItemStatus::Ready
                    | ItemStatus::Downloading(_)
                    | ItemStatus::Waiting
                    | ItemStatus::Restart
            )
        }) {
            row.next = true;
        }
        rows
    }

    fn shows(&self, step: Step) -> bool {
        match step {
            Step::TryIt => self.practice_step,
            Step::Setup => self.setup_needed || self.step == Step::Setup,
            _ => true,
        }
    }

    fn passes(&self, step: Step) -> bool {
        match step {
            Step::Setup => self.setup_ready(),
            Step::TryIt => !self.practice_step,
            _ => false,
        }
    }

    fn forward_from(&self, mut step: Step) -> Step {
        while self.passes(step) && step != Step::Done {
            step = Step::TOUR[step.position() + 1];
        }
        step
    }

    fn back_from(&self, step: Step) -> Step {
        let mut index = step.position();
        while index > 0 {
            index -= 1;
            if !self.passes(Step::TOUR[index]) {
                return Step::TOUR[index];
            }
        }
        step
    }

    fn advance(&mut self) -> Effect {
        if self.step == Step::Done {
            return Effect::Finish;
        }
        self.step = self.forward_from(Step::TOUR[self.step.position() + 1]);
        Effect::None
    }

    fn primary(&mut self) -> Effect {
        match self.step {
            Step::Setup if !self.setup_ready() => Effect::None,
            Step::TryIt if self.practice != Practice::Landed => Effect::None,
            _ => self.advance(),
        }
    }

    fn grant(&mut self, item: SetupItem) -> Effect {
        match item {
            // The model can be retried from any step: its progress follows
            // the whole tour.
            SetupItem::Model
                if !self.status.transcription_model && self.model.downloading.is_none() =>
            {
                self.model.failed = false;
                self.download_requested = true;
                Effect::DownloadModel
            }
            _ if self.step != Step::Setup => Effect::None,
            SetupItem::Microphone => {
                self.asked[0] = true;
                match self.status.microphone {
                    PermissionState::Ready | PermissionState::NeedsRestart => Effect::None,
                    PermissionState::NeedsRequest => Effect::RequestMicrophone,
                    PermissionState::NeedsSettings => {
                        self.microphone_guide = true;
                        Effect::ShowGuide(PermissionKind::Microphone)
                    }
                }
            }
            SetupItem::Accessibility if grantable(self.status.accessibility) => {
                self.asked[1] = true;
                Effect::ShowGuide(PermissionKind::Accessibility)
            }
            _ => Effect::None,
        }
    }

    /// Once per tour, and only after dictation is ready: before that, the
    /// listener has not been asked to start.
    fn relaunch(&mut self) -> Effect {
        if self.relaunching || !self.status.ready() {
            return Effect::None;
        }
        self.relaunching = true;
        self.reopen = false;
        Effect::Relaunch
    }
}

/// Not on yet, so System Settings still has something to switch on.
fn grantable(state: PermissionState) -> bool {
    !matches!(
        state,
        PermissionState::Ready | PermissionState::NeedsRestart
    )
}

fn next_practice(current: Practice, observation: Observation) -> Practice {
    match observation {
        Observation::Started => Practice::Listening,
        Observation::Transcribing => Practice::Transcribing,
        Observation::Discarded if current == Practice::Transcribing => Practice::NothingHeard,
        Observation::Discarded => Practice::TooShort,
        Observation::Cancelled if current == Practice::Landed => Practice::Landed,
        Observation::Cancelled => Practice::Waiting,
        Observation::Pasted => Practice::Landed,
        Observation::Failed => Practice::Failed,
    }
}

/// A relaunch the tour started at most this long ago keeps it from
/// relaunching again, so a grant that never reaches Hex cannot loop.
pub const RELAUNCH_GUARD_MS: u64 = 120_000;

static LISTENER_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called by the recognition worker when the shortcut listener cannot start.
pub fn note_listener_failure() {
    LISTENER_FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn take_listener_failure() -> bool {
    LISTENER_FAILED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// Records a relaunch and where the tour resumes after it.
pub fn record_relaunch(now_ms: u64) -> color_eyre::Result<()> {
    record_relaunch_at(&crate::app_paths::support_dir()?, now_ms)
}

pub fn relaunch_allowed(now_ms: u64) -> bool {
    crate::app_paths::support_dir()
        .map(|directory| relaunch_allowed_at(&directory, now_ms))
        .unwrap_or(false)
}

/// The step a relaunched Hex resumes at, consumed once.
pub fn take_resume() -> Option<Step> {
    let directory = crate::app_paths::support_dir().ok()?;
    take_resume_at(&directory, unix_ms())
}

pub(crate) fn record_relaunch_at(directory: &Path, now_ms: u64) -> color_eyre::Result<()> {
    fs::create_dir_all(directory)?;
    fs::write(directory.join("onboarding-relaunch"), now_ms.to_string())?;
    Ok(())
}

fn relaunched_at(directory: &Path) -> Option<u64> {
    fs::read_to_string(directory.join("onboarding-relaunch"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub(crate) fn relaunch_allowed_at(directory: &Path, now_ms: u64) -> bool {
    relaunched_at(directory).is_none_or(|at| now_ms.saturating_sub(at) >= RELAUNCH_GUARD_MS)
}

/// A relaunch older than the guard is stale (Hex was quit and reopened
/// later), so the tour does not resume from it.
pub(crate) fn take_resume_at(directory: &Path, now_ms: u64) -> Option<Step> {
    let at = relaunched_at(directory)?;
    let marker = directory.join("onboarding-resume-consumed");
    if marker.is_file() && relaunched_at_consumed(&marker) == Some(at) {
        return None;
    }
    let _ = fs::write(&marker, at.to_string());
    (now_ms.saturating_sub(at) < RELAUNCH_GUARD_MS).then_some(Step::TryIt)
}

fn relaunched_at_consumed(marker: &Path) -> Option<u64> {
    fs::read_to_string(marker).ok()?.trim().parse().ok()
}

/// Remembers the tour's step, so a tour quit part-way (macOS's own "Quit &
/// Reopen" after a grant, or ⌘Q) resumes there.
pub fn record_step(step: Step) {
    let result =
        crate::app_paths::support_dir().and_then(|directory| record_step_at(&directory, step));
    if let Err(error) = result {
        tracing::warn!(%error, "could not record the onboarding step");
    }
}

pub fn saved_step() -> Option<Step> {
    saved_step_at(&crate::app_paths::support_dir().ok()?)
}

pub(crate) fn record_step_at(directory: &Path, step: Step) -> color_eyre::Result<()> {
    let name = match step {
        Step::Welcome => "welcome",
        Step::Setup => "setup",
        Step::TryIt => "try-it",
        Step::Done => "done",
    };
    fs::create_dir_all(directory)?;
    fs::write(directory.join("onboarding-step"), name)?;
    Ok(())
}

pub(crate) fn saved_step_at(directory: &Path) -> Option<Step> {
    match fs::read_to_string(directory.join("onboarding-step"))
        .ok()?
        .trim()
    {
        "setup" => Some(Step::Setup),
        "try-it" => Some(Step::TryIt),
        "done" => Some(Step::Done),
        _ => None,
    }
}

pub fn completion_recorded() -> bool {
    let Ok(directory) = crate::app_paths::support_dir() else {
        return false;
    };
    completion_recorded_at(&directory)
}

pub fn record_completion() -> color_eyre::Result<()> {
    let directory = crate::app_paths::support_dir()?;
    record_completion_at(&directory)
}

pub(crate) fn completion_recorded_at(directory: &Path) -> bool {
    if directory.join("onboarding-complete").is_file() {
        return true;
    }
    if directory.join("onboarding-pending").is_file() || !directory.join("settings.json").is_file()
    {
        return false;
    }
    if let Err(error) = record_completion_at(directory) {
        tracing::warn!(%error, "could not migrate onboarding completion");
    }
    true
}

pub(crate) fn record_pending_at(directory: &Path) -> color_eyre::Result<()> {
    fs::create_dir_all(directory)?;
    write_owner_only_marker(&directory.join("onboarding-pending"), false)?;
    crate::transcription_models::sync_directory(directory)?;
    Ok(())
}

pub(crate) fn record_completion_at(directory: &Path) -> color_eyre::Result<()> {
    fs::create_dir_all(directory)?;
    let destination = directory.join("onboarding-complete");
    let temporary = directory.join(".onboarding-complete.tmp");
    write_owner_only_marker(&temporary, true)?;
    fs::rename(temporary, destination)?;
    crate::transcription_models::sync_directory(directory)?;
    Ok(())
}

fn write_owner_only_marker(path: &Path, truncate: bool) -> color_eyre::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(truncate).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.sync_all()?;
    Ok(())
}

pub fn status(selection: &TranscriptionSelection) -> SetupStatus {
    let transcription_model = crate::transcription_models::validate(selection)
        .is_ok_and(|model| crate::transcription_models::is_installed(model, &selection.language));
    status_with_transcription_model(transcription_model)
}

#[cfg(target_os = "macos")]
pub fn status_with_transcription_model(transcription_model: bool) -> SetupStatus {
    SetupStatus {
        microphone: microphone_state(),
        // Accessibility also grants the listen-event access Hex's shortcut
        // taps need, so Input Monitoring is never asked for.
        accessibility: accessibility_state(CGPreflightPostEventAccess(), AXIsProcessTrusted()),
        transcription_model,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status_with_transcription_model(transcription_model: bool) -> SetupStatus {
    SetupStatus {
        microphone: PermissionState::Ready,
        accessibility: PermissionState::Ready,
        transcription_model,
    }
}

#[cfg(target_os = "macos")]
pub fn request_microphone() {
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        tracing::error!("AVCaptureDevice is unavailable");
        return;
    };
    let handler = RcBlock::new(|granted: Bool| {
        tracing::info!(
            granted = granted.as_bool(),
            "microphone permission request finished"
        );
    });
    unsafe {
        let _: () = msg_send![
            class,
            requestAccessForMediaType: &*AVMediaTypeAudio,
            completionHandler: &*handler
        ];
    }
}

/// Reopens the installed app once this process exits. Returns false outside
/// an app bundle (a development binary), where the person reopens Hex.
/// The relaunch keeps this process's `HEX_*` environment.
#[cfg(target_os = "macos")]
pub fn spawn_relaunch() -> bool {
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    let Some(bundle) = executable
        .ancestors()
        .nth(3)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
    else {
        return false;
    };
    Command::new("/bin/sh")
        .args(relaunch_arguments(
            std::process::id(),
            bundle,
            std::env::vars_os(),
        ))
        .spawn()
        .inspect_err(|error| tracing::error!(%error, "could not schedule relaunch"))
        .is_ok()
}

/// `/bin/sh` arguments that wait for `pid` to exit, then open `bundle` with
/// the forwarded environment. Waiting keeps the relaunched copy from racing
/// the instance lock; `open` without -n leaves a single copy.
#[cfg(target_os = "macos")]
fn relaunch_arguments(
    pid: u32,
    bundle: &Path,
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<std::ffi::OsString> {
    let mut arguments: Vec<std::ffi::OsString> = vec![
        "-c".into(),
        "pid=$1; shift; while kill -0 \"$pid\" 2>/dev/null; do sleep 0.1; done; \
         exec /usr/bin/open \"$@\""
            .into(),
        "hex-relaunch".into(),
        pid.to_string().into(),
    ];
    arguments.extend(open_env_arguments(vars));
    arguments.push(bundle.into());
    arguments
}

/// `open --env` arguments for every `HEX_*` variable in `vars`
/// (HEX_RETAIN_DICTATION_AUDIO, HEX_ENABLE_LLM, HEX_EXPERIMENTS, …), sorted.
/// LaunchServices starts apps without the caller's environment, so a
/// relaunch or `hex start` passes these explicitly. Values go only to
/// `open`'s arguments; never log them.
#[cfg(target_os = "macos")]
pub fn open_env_arguments(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<std::ffi::OsString> {
    let mut forwarded = vars
        .into_iter()
        .filter(|(key, _)| key.to_str().is_some_and(|key| key.starts_with("HEX_")))
        .collect::<Vec<_>>();
    forwarded.sort();
    forwarded
        .into_iter()
        .flat_map(|(key, value)| {
            let mut pair = key;
            pair.push("=");
            pair.push(value);
            [std::ffi::OsString::from("--env"), pair]
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn microphone_state() -> PermissionState {
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        return PermissionState::NeedsRequest;
    };
    let status: isize =
        unsafe { msg_send![class, authorizationStatusForMediaType: &*AVMediaTypeAudio] };
    microphone_authorization_state(status)
}

/// Accessibility reaches a running Hex: `AXIsProcessTrusted` follows the
/// switch, and the listener's event taps, created only once setup is ready,
/// are allowed (macOS offers no "Quit & Reopen" for it, as it does for Input
/// Monitoring). Only `CGPreflightPostEventAccess` keeps its launch-time answer,
/// so it must not hold setup at a restart. If macOS still refuses the taps,
/// the listener-failure relaunch takes over.
#[cfg(any(target_os = "macos", test))]
const fn accessibility_state(post_event_preflight: bool, trusted: bool) -> PermissionState {
    if post_event_preflight || trusted {
        PermissionState::Ready
    } else {
        PermissionState::NeedsSettings
    }
}

#[cfg(any(target_os = "macos", test))]
const fn microphone_authorization_state(status: isize) -> PermissionState {
    match status {
        0 => PermissionState::NeedsRequest,
        3 => PermissionState::Ready,
        _ => PermissionState::NeedsSettings,
    }
}

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: *const NSString;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    safe fn CGPreflightPostEventAccess() -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    safe fn AXIsProcessTrusted() -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn relaunch_forwards_only_the_hex_environment() {
        let vars = [
            ("PATH", "/usr/bin"),
            ("HEX_RETAIN_DICTATION_AUDIO", "1"),
            ("HEX_ENABLE_LLM", "1"),
            ("HEX_EXPERIMENTS", "a=b,c"),
            ("HEXAGON", "no"),
        ]
        .map(|(key, value)| (key.into(), value.into()));
        let arguments = relaunch_arguments(42, Path::new("/Applications/Hex.app"), vars);
        let arguments = arguments
            .iter()
            .map(|argument| argument.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(arguments[0], "-c");
        assert!(arguments[1].ends_with("exec /usr/bin/open \"$@\""));
        assert_eq!(
            arguments[2..],
            [
                "hex-relaunch",
                "42",
                "--env",
                "HEX_ENABLE_LLM=1",
                "--env",
                "HEX_EXPERIMENTS=a=b,c",
                "--env",
                "HEX_RETAIN_DICTATION_AUDIO=1",
                "/Applications/Hex.app",
            ]
        );
    }

    #[test]
    fn missing_commands_model_does_not_block_dictation_setup() {
        let mut status = SetupStatus {
            microphone: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: true,
        };
        assert!(status.ready());
        status.transcription_model = false;
        assert!(!status.ready());
        status.transcription_model = true;
        status.microphone = PermissionState::NeedsRequest;
        assert!(!status.ready());
        status.microphone = PermissionState::Ready;
        status.accessibility = PermissionState::NeedsSettings;
        assert!(!status.ready());
    }

    #[test]
    fn denied_microphone_permission_requires_system_settings() {
        assert_eq!(
            microphone_authorization_state(0),
            PermissionState::NeedsRequest
        );
        assert_eq!(
            microphone_authorization_state(2),
            PermissionState::NeedsSettings
        );
        assert_eq!(microphone_authorization_state(3), PermissionState::Ready);
    }

    #[test]
    fn healthy_permissions_have_no_warning_projection() {
        assert!(permission_warnings(ready_status()).is_empty());
    }

    #[test]
    fn warning_projection_preserves_permission_order_and_actions() {
        let warnings = permission_warnings(SetupStatus {
            microphone: PermissionState::NeedsRequest,
            accessibility: PermissionState::NeedsSettings,
            ..ready_status()
        });
        assert_eq!(
            warnings,
            [
                PermissionWarning {
                    kind: PermissionKind::Microphone,
                    action: PermissionAction::RequestMicrophone,
                },
                PermissionWarning {
                    kind: PermissionKind::Accessibility,
                    action: PermissionAction::ShowGuide(PermissionKind::Accessibility),
                },
            ]
        );
    }

    #[test]
    fn denied_microphone_maps_to_settings_in_warning_projection() {
        let warning = permission_warnings(SetupStatus {
            microphone: PermissionState::NeedsSettings,
            ..ready_status()
        });
        assert_eq!(
            warning[0].action,
            PermissionAction::ShowGuide(PermissionKind::Microphone)
        );
    }

    #[test]
    fn existing_rust_settings_migrate_onboarding_completion() {
        let directory =
            std::env::temp_dir().join(format!("hex-onboarding-migration-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("settings.json"), b"{}").unwrap();

        assert!(completion_recorded_at(&directory));
        let marker = directory.join("onboarding-complete");
        assert!(marker.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(marker).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fresh_install_does_not_skip_onboarding() {
        let directory =
            std::env::temp_dir().join(format!("hex-onboarding-fresh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();

        assert!(!completion_recorded_at(&directory));
        assert!(!directory.join("onboarding-complete").exists());

        fs::remove_dir_all(directory).unwrap();
    }

    fn ready_status() -> SetupStatus {
        SetupStatus {
            microphone: PermissionState::Ready,
            accessibility: PermissionState::Ready,
            transcription_model: true,
        }
    }

    fn fresh_status() -> SetupStatus {
        SetupStatus {
            microphone: PermissionState::NeedsRequest,
            accessibility: PermissionState::NeedsSettings,
            transcription_model: false,
        }
    }

    fn installed() -> ModelProgress {
        ModelProgress {
            installed: true,
            ..ModelProgress::default()
        }
    }

    fn statuses(flow: &Flow) -> Vec<(SetupItem, ItemStatus, bool)> {
        flow.view()
            .rows
            .into_iter()
            .map(|row| (row.item, row.status, row.next))
            .collect()
    }

    fn downloading(progress: f32) -> ModelProgress {
        ModelProgress {
            downloading: Some(progress),
            ..ModelProgress::default()
        }
    }

    fn permissions_only() -> SetupStatus {
        SetupStatus {
            transcription_model: false,
            ..ready_status()
        }
    }

    #[test]
    fn tour_walks_welcome_setup_try_done() {
        let mut flow = Flow::new(Step::Welcome, fresh_status(), true, true);
        assert_eq!(flow.view().count, 3);
        assert!(!flow.view().can_go_back);
        assert_eq!(flow.on(Input::Continue), Effect::None);
        assert_eq!(flow.step(), Step::Setup);
        assert_eq!(flow.on(Input::Continue), Effect::None, "setup is not ready");
        assert_eq!(flow.step(), Step::Setup);
        flow.on(Input::Status(ready_status(), installed()));
        assert!(flow.view().ready);
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::TryIt);
        assert_eq!(flow.on(Input::Continue), Effect::None, "practice first");
        flow.on(Input::Dictation(Observation::Started));
        flow.on(Input::Dictation(Observation::Transcribing));
        flow.on(Input::Dictation(Observation::Pasted));
        assert_eq!(flow.view().practice, Practice::Landed);
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::Done);
        assert!(!flow.view().can_go_back);
        assert_eq!(flow.on(Input::Continue), Effect::Finish);
    }

    #[test]
    fn the_model_downloads_as_soon_as_the_tour_opens() {
        let mut flow = Flow::new(Step::Welcome, fresh_status(), true, true);
        assert_eq!(flow.view().model, ItemStatus::Todo);
        assert_eq!(
            flow.on(Input::Status(fresh_status(), ModelProgress::default())),
            Effect::DownloadModel,
            "on the welcome screen, before any permission"
        );
        assert_eq!(flow.step(), Step::Welcome);
        assert!(flow.view().show_download);
        assert_eq!(
            flow.view().model,
            ItemStatus::Downloading(0.0),
            "progress shows before the first byte"
        );
        assert_eq!(
            flow.on(Input::Status(fresh_status(), ModelProgress::default())),
            Effect::None,
            "asked once"
        );
        flow.on(Input::Status(fresh_status(), downloading(0.3)));
        assert_eq!(flow.view().model, ItemStatus::Downloading(0.3));

        let installed_flow = {
            let mut flow = Flow::new(Step::Welcome, ready_status(), true, true);
            assert_eq!(
                flow.on(Input::Status(ready_status(), installed())),
                Effect::None
            );
            flow
        };
        assert!(!installed_flow.view().show_download);
    }

    #[test]
    fn a_failed_download_retries_from_any_step() {
        let mut flow = Flow::new(Step::Welcome, fresh_status(), true, true);
        flow.on(Input::Status(fresh_status(), ModelProgress::default()));
        let failed = ModelProgress {
            failed: true,
            ..ModelProgress::default()
        };
        assert_eq!(flow.on(Input::Status(fresh_status(), failed)), Effect::None);
        assert_eq!(flow.view().model, ItemStatus::Failed);
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Model)),
            Effect::DownloadModel,
            "retry on the welcome screen"
        );
        flow.on(Input::Status(fresh_status(), downloading(0.1)));
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Model)),
            Effect::None,
            "a running download is not restarted"
        );
    }

    #[test]
    fn permissions_are_the_only_rows_and_the_model_waits_for_practice() {
        let mut flow = Flow::new(Step::Setup, fresh_status(), true, true);
        assert_eq!(
            statuses(&flow),
            [
                (SetupItem::Microphone, ItemStatus::Todo, true),
                (SetupItem::Accessibility, ItemStatus::Todo, false),
            ]
        );
        flow.on(Input::Status(permissions_only(), downloading(0.5)));
        assert!(flow.view().ready, "permissions alone finish setup");
        assert!(!flow.view().dictation_ready);
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::TryIt);
        assert_eq!(flow.view().model, ItemStatus::Downloading(0.5));
        flow.on(Input::Status(ready_status(), installed()));
        assert!(flow.view().dictation_ready);
    }

    #[test]
    fn rows_flip_live_and_the_next_row_carries_the_accent() {
        let mut flow = Flow::new(Step::Setup, fresh_status(), true, true);
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Microphone)),
            Effect::RequestMicrophone
        );
        assert_eq!(flow.view().rows[0].status, ItemStatus::Waiting);
        assert!(
            flow.view().rows[1].next,
            "a waiting row hands on the accent"
        );
        let mut status = fresh_status();
        status.microphone = PermissionState::Ready;
        flow.on(Input::Status(status, downloading(0.4)));
        assert_eq!(flow.view().rows[0].status, ItemStatus::Ready);
        assert!(flow.view().rows[1].next);
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Accessibility)),
            Effect::ShowGuide(PermissionKind::Accessibility)
        );
        assert_eq!(flow.view().rows[1].status, ItemStatus::Waiting);
        assert!(!flow.view().rows[1].next, "a waiting row has no accent");
    }

    #[test]
    fn the_last_grant_advances_after_a_beat() {
        let mut flow = Flow::new(Step::Setup, fresh_status(), true, true);
        flow.on(Input::Status(fresh_status(), downloading(0.2)));
        let mut status = permissions_only();
        status.accessibility = PermissionState::NeedsSettings;
        assert_eq!(
            flow.on(Input::Status(status, downloading(0.3))),
            Effect::None
        );
        assert_eq!(
            flow.on(Input::Status(permissions_only(), downloading(0.4))),
            Effect::AdvanceSoon
        );
        assert_eq!(
            flow.on(Input::Status(permissions_only(), downloading(0.5))),
            Effect::None,
            "once"
        );
        assert_eq!(
            flow.step(),
            Step::Setup,
            "the window moves on after the beat"
        );
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::TryIt);
    }

    #[test]
    fn a_refused_microphone_opens_its_guide_and_pulses() {
        let mut status = fresh_status();
        status.microphone = PermissionState::NeedsSettings;
        let mut flow = Flow::new(Step::Setup, status, true, true);
        assert_eq!(flow.view().rows[0].status, ItemStatus::Denied);
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Microphone)),
            Effect::ShowGuide(PermissionKind::Microphone)
        );
        assert_eq!(flow.view().rows[0].status, ItemStatus::Waiting);
    }

    #[test]
    fn platforms_without_permissions_show_only_the_model() {
        let status = SetupStatus {
            transcription_model: false,
            ..ready_status()
        };
        let mut flow = Flow::new(Step::Welcome, status, false, false);
        assert_eq!(
            statuses(&flow),
            [(SetupItem::Model, ItemStatus::Todo, true)]
        );
        assert_eq!(flow.view().count, 2, "no practice step");
        assert_eq!(
            flow.on(Input::Status(status, ModelProgress::default())),
            Effect::DownloadModel
        );
        assert!(
            !flow.view().show_download,
            "the setup row shows the progress"
        );
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::Setup);
        assert_eq!(flow.on(Input::Continue), Effect::None);
        assert_eq!(
            flow.on(Input::Status(ready_status(), installed())),
            Effect::AdvanceSoon
        );
    }

    #[test]
    fn without_practice_setup_continues_to_done() {
        let mut flow = Flow::new(Step::Setup, ready_status(), false, false);
        assert_eq!(flow.step(), Step::Setup);
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::Done);
        assert!(!flow.view().can_go_back);
    }

    #[test]
    fn granted_permissions_skip_setup() {
        let mut flow = Flow::new(Step::Welcome, permissions_only(), true, true);
        assert_eq!(flow.view().count, 2);
        flow.on(Input::Continue);
        assert_eq!(flow.step(), Step::TryIt);
    }

    #[test]
    fn practice_hints_distinguish_short_taps_from_silence() {
        let mut flow = Flow::new(Step::TryIt, ready_status(), true, true);
        assert_eq!(flow.step(), Step::TryIt);
        flow.on(Input::Dictation(Observation::Started));
        assert_eq!(flow.view().practice, Practice::Listening);
        flow.on(Input::Dictation(Observation::Discarded));
        assert_eq!(flow.view().practice, Practice::TooShort);
        flow.on(Input::Dictation(Observation::Started));
        flow.on(Input::Dictation(Observation::Transcribing));
        flow.on(Input::Dictation(Observation::Discarded));
        assert_eq!(flow.view().practice, Practice::NothingHeard);
        flow.on(Input::Dictation(Observation::Started));
        flow.on(Input::Dictation(Observation::Cancelled));
        assert_eq!(flow.view().practice, Practice::Waiting);
        flow.on(Input::Dictation(Observation::Failed));
        assert_eq!(flow.view().practice, Practice::Failed);
        assert_eq!(flow.on(Input::Skip), Effect::None);
        assert_eq!(flow.step(), Step::Done);
    }

    #[test]
    fn dictation_outside_the_practice_step_is_ignored() {
        let mut flow = Flow::new(Step::Welcome, ready_status(), true, true);
        flow.on(Input::Dictation(Observation::Pasted));
        assert_eq!(flow.view().practice, Practice::Waiting);
    }

    #[test]
    fn resuming_past_setup_waits_on_setup_until_ready() {
        let flow = Flow::new(Step::TryIt, fresh_status(), true, true);
        assert_eq!(flow.step(), Step::Setup);
        let mut flow = Flow::new(Step::TryIt, ready_status(), true, true);
        assert_eq!(flow.step(), Step::TryIt);
        flow.on(Input::Back);
        assert_eq!(flow.step(), Step::Welcome, "a ready setup is passed");
    }

    #[test]
    fn listener_failure_relaunches_once_then_offers_reopen() {
        let mut flow = Flow::new(Step::Setup, fresh_status(), true, true);
        assert_eq!(
            flow.on(Input::ListenerFailed {
                relaunch_allowed: true
            }),
            Effect::None,
            "nothing to relaunch for before setup is ready"
        );
        flow.on(Input::Status(ready_status(), installed()));
        assert_eq!(
            flow.on(Input::ListenerFailed {
                relaunch_allowed: true
            }),
            Effect::Relaunch
        );
        assert_eq!(
            flow.on(Input::ListenerFailed {
                relaunch_allowed: true
            }),
            Effect::None,
            "one relaunch per tour"
        );

        let mut guarded = Flow::new(Step::TryIt, ready_status(), true, true);
        assert_eq!(
            guarded.on(Input::ListenerFailed {
                relaunch_allowed: false
            }),
            Effect::None
        );
        assert!(guarded.view().reopen);
        assert_eq!(guarded.on(Input::Reopen), Effect::Relaunch);
        assert!(!guarded.view().reopen);
    }

    #[test]
    fn relaunch_guard_and_resume_marker() {
        let directory =
            std::env::temp_dir().join(format!("hex-onboarding-relaunch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();

        assert!(relaunch_allowed_at(&directory, 1_000));
        assert_eq!(take_resume_at(&directory, 1_000), None);
        record_relaunch_at(&directory, 1_000).unwrap();
        assert!(!relaunch_allowed_at(
            &directory,
            1_000 + RELAUNCH_GUARD_MS - 1
        ));
        assert!(relaunch_allowed_at(&directory, 1_000 + RELAUNCH_GUARD_MS));
        assert_eq!(take_resume_at(&directory, 3_000), Some(Step::TryIt));
        assert_eq!(take_resume_at(&directory, 3_000), None, "consumed once");

        record_relaunch_at(&directory, 10_000).unwrap();
        assert_eq!(
            take_resume_at(&directory, 10_000 + RELAUNCH_GUARD_MS),
            None,
            "a stale relaunch does not resume"
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_grant_the_process_cannot_use_yet_waits_for_a_restart() {
        let waiting = SetupStatus {
            accessibility: PermissionState::NeedsRestart,
            ..ready_status()
        };
        assert!(!waiting.ready(), "the listener waits for the restart");
        assert_eq!(
            permission_warnings(waiting),
            [PermissionWarning {
                kind: PermissionKind::Accessibility,
                action: PermissionAction::Restart,
            }]
        );
    }

    #[test]
    fn an_accessibility_grant_applies_without_a_restart() {
        assert_eq!(accessibility_state(true, true), PermissionState::Ready);
        assert_eq!(accessibility_state(true, false), PermissionState::Ready);
        assert_eq!(
            accessibility_state(false, true),
            PermissionState::Ready,
            "the launch-time preflight is stale; trust is live"
        );
        assert_eq!(
            accessibility_state(false, false),
            PermissionState::NeedsSettings
        );

        let mut flow = Flow::new(Step::Setup, fresh_status(), true, true);
        flow.on(Input::Status(
            SetupStatus {
                accessibility: PermissionState::NeedsSettings,
                ..ready_status()
            },
            downloading(0.5),
        ));
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Accessibility)),
            Effect::ShowGuide(PermissionKind::Accessibility)
        );
        assert_eq!(
            flow.on(Input::Status(
                SetupStatus {
                    accessibility: accessibility_state(false, true),
                    ..ready_status()
                },
                downloading(0.5),
            )),
            Effect::AdvanceSoon
        );
        assert!(!flow.view().restart);
        assert!(flow.view().ready);
    }

    #[test]
    fn setup_offers_a_restart_once_every_permission_is_on() {
        let mut flow = Flow::new(Step::Setup, fresh_status(), true, true);
        flow.on(Input::Status(
            SetupStatus {
                accessibility: PermissionState::NeedsRestart,
                ..fresh_status()
            },
            downloading(0.5),
        ));
        assert_eq!(
            statuses(&flow)[1],
            (SetupItem::Accessibility, ItemStatus::Restart, false)
        );
        assert!(!flow.view().restart, "the microphone is still off");
        assert_eq!(flow.on(Input::Restart), Effect::None);
        assert_eq!(
            flow.on(Input::Grant(SetupItem::Accessibility)),
            Effect::None,
            "nothing left to switch on"
        );

        flow.on(Input::Status(
            SetupStatus {
                accessibility: PermissionState::NeedsRestart,
                transcription_model: false,
                ..ready_status()
            },
            downloading(0.5),
        ));
        assert!(flow.view().restart);
        assert!(!flow.view().ready);
        assert_eq!(flow.on(Input::Continue), Effect::None);
        assert_eq!(flow.on(Input::Restart), Effect::Relaunch);
        assert!(!flow.view().restart, "one restart per tour");
        assert_eq!(flow.on(Input::Restart), Effect::None);
    }

    #[test]
    fn the_tour_step_is_saved_for_the_next_launch() {
        let directory =
            std::env::temp_dir().join(format!("hex-onboarding-step-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(saved_step_at(&directory), None);
        record_step_at(&directory, Step::Setup).unwrap();
        assert_eq!(saved_step_at(&directory), Some(Step::Setup));
        record_step_at(&directory, Step::TryIt).unwrap();
        assert_eq!(saved_step_at(&directory), Some(Step::TryIt));
        record_step_at(&directory, Step::Welcome).unwrap();
        assert_eq!(saved_step_at(&directory), None, "Welcome is the default");
        fs::remove_dir_all(directory).unwrap();
    }
}
