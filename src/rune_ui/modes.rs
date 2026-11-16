//! The Modes pane: the Global mode and custom modes chosen by the frontmost
//! application, each with corrections, optional OpenCode rewriting, and
//! transformations, built in or from the user's config. Text fields are drafts saved when
//! typing pauses; every other edit saves at once and asks the service to
//! reload.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use rune::{
    Align, Drag, El, Phase, ScrollSpec, Side, TextEdit, Tx, Waker, col, edit_access, el, row, text,
};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, Switch, TextArea, TextInput};

use super::controls::{
    ButtonKind, DESCRIPTION, Highlight, METRICS, Paint, button, card, note, page, section,
    section_with, setting_row,
};
use super::lists::{LIST_COLUMN_PL, list_row, remove_button, source_list};
use super::models::{self, Target};
use super::{Menu, Ui, speech_model};
use crate::app_settings::{AppSettings, DictationMode};
use crate::application_catalog::InstalledApplication;
use crate::desktop::file_dialog::{Choice, Dialog, Filter, Pending, Target as DialogTarget};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::desktop::opencode_catalog::OpenCodeFeature;
use crate::dictation::text_replacements::TextReplacement;
use crate::personal_commands::{HostState, StatusSnapshot, StatusTransformation};
use transformations::transformations;

mod transformations;

const LIST_WIDTH: f32 = 240.0;
const FIELD_WIDTH: f32 = 280.0;
/// How often the transformation host's status file is reread.
const STATUS_INTERVAL: Duration = Duration::from_secs(1);

static PLUS: &rune::Icon = &rune_theme_opencode::icons::PLUS;
static GRIP: rune::Icon = rune::Icon {
    name: "hex-grip",
    view: 16.0,
    body: r#"<path d="M4 5.5H12M4 8H12M4 10.5H12" stroke="currentColor"/>"#,
};
/// A transformation row's height; dragging moves one place per row.
const TRANSFORMATION_ROW: f32 = 36.0;
/// Application icons draw at 24 points in the picker and 16 beside names,
/// from one raster per application.
const ICON_SIZE: f32 = 24.0;
/// Past this many icons (each about 12 KB with its mips), the held ones are
/// dropped and those still shown load again.
const MAX_ICONS: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Selection {
    Default,
    Custom(usize),
}

pub(super) fn mode(settings: &AppSettings, selection: Selection) -> Option<&DictationMode> {
    match selection {
        Selection::Default => Some(&settings.dictation_processing.default_mode),
        Selection::Custom(index) => settings.dictation_processing.modes.get(index),
    }
}

pub(super) fn mode_mut(
    settings: &mut AppSettings,
    selection: Selection,
) -> Option<&mut DictationMode> {
    match selection {
        Selection::Default => Some(&mut settings.dictation_processing.default_mode),
        Selection::Custom(index) => settings.dictation_processing.modes.get_mut(index),
    }
}

/// The selected mode's text fields.
#[derive(Default)]
struct Editor {
    base: DictationMode,
    name: TextEdit,
    prompt: TextEdit,
    deadline: TextEdit,
    replacements: Vec<(TextEdit, TextEdit)>,
}

impl Editor {
    fn rebase(&mut self, current: &DictationMode, keep_drafts: bool) {
        let old = Self::of(&self.base);
        let next = Self::of(current);
        for (field, before, after) in [
            (&mut self.name, old.name, next.name),
            (&mut self.prompt, old.prompt, next.prompt),
            (&mut self.deadline, old.deadline, next.deadline),
        ] {
            if (!keep_drafts || field.text == before.text) && field.text != after.text {
                *field = after;
            }
        }
        let values = |rows: &[(TextEdit, TextEdit)]| {
            rows.iter()
                .map(|(a, b)| (a.text.clone(), b.text.clone()))
                .collect::<Vec<_>>()
        };
        if (!keep_drafts || values(&self.replacements) == values(&old.replacements))
            && values(&self.replacements) != values(&next.replacements)
        {
            self.replacements = next.replacements;
        }
        self.base = current.clone();
    }

    fn of(mode: &DictationMode) -> Self {
        Self {
            base: mode.clone(),
            name: TextEdit::new(mode.name.clone()),
            prompt: TextEdit::new(mode.post_processing.prompt.clone()),
            deadline: TextEdit::new(mode.post_processing.deadline_seconds.to_string()),
            replacements: mode
                .replacements
                .iter()
                .map(|r| {
                    (
                        TextEdit::new(r.matched_phrase.clone()),
                        TextEdit::new(r.output.clone()),
                    )
                })
                .collect(),
        }
    }
}

/// A transformation being dragged: the order it started from, the order
/// shown while dragging, and where it started.
struct TransformationDrag {
    id: String,
    start: usize,
    committed: Vec<String>,
    order: Vec<String>,
}

pub(super) type ChosenApplication = Result<Option<InstalledApplication>, String>;

/// Where the application picker adds what it picks: the selected mode, or
/// Labs › YOLO mode's Only in list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PickerTarget {
    #[default]
    Mode,
    YoloMode,
}

enum Applications {
    NotLoaded,
    Loading(Receiver<Vec<InstalledApplication>>),
    Loaded(Vec<InstalledApplication>),
}

pub(super) struct ModesPane {
    pub(super) selected: Selection,
    editor: Editor,
    pub(super) delete_armed: Option<String>,
    pub(super) application_picker_open: bool,
    pub(super) picker_target: PickerTarget,
    application_search: TextEdit,
    application_highlight: Highlight,
    pub(super) application_error: Option<String>,
    applications: Applications,
    /// The native file dialog, while it is open.
    chooser: Option<Pending>,
    drag: Option<TransformationDrag>,
    pub(super) waker: Option<Waker>,
    /// Rasterizes application icons off the UI thread; made in `setup`.
    pub(super) icon_cache: Option<rune::ImageCache>,
    /// Icons by catalog path, requested as the view first shows them. The
    /// cache shares only handles kept alive here.
    icons: RefCell<HashMap<PathBuf, rune::Image>>,
    icons_ready: usize,
    /// Whether the listener can tell which app is frontmost (X11, not Wayland).
    focused_app_known: bool,
    pub(super) status: StatusSnapshot,
    /// The config hash the person allowed to use `fetch` and programs.
    pub(super) approved: Option<String>,
    status_read: Option<Instant>,
    workspace: PathBuf,
    config_exists: bool,
    workspace_receiver: Option<Receiver<Result<PathBuf, String>>>,
    workspace_error: Option<String>,
}

impl ModesPane {
    pub(super) fn new(settings: &AppSettings) -> Self {
        let mut status = StatusSnapshot::default();
        crate::personal_commands::include_builtin_transformations(&mut status);
        Self {
            selected: Selection::Default,
            editor: Editor::of(&settings.dictation_processing.default_mode),
            delete_armed: None,
            application_picker_open: false,
            picker_target: PickerTarget::Mode,
            application_search: TextEdit::default(),
            application_highlight: Highlight::default(),
            application_error: None,
            applications: Applications::NotLoaded,
            chooser: None,
            drag: None,
            waker: None,
            icon_cache: None,
            icons: RefCell::default(),
            icons_ready: 0,
            focused_app_known: true,
            status,
            approved: None,
            status_read: None,
            workspace: PathBuf::new(),
            config_exists: true,
            workspace_receiver: None,
            workspace_error: None,
        }
    }

    pub(super) fn load_personal_commands(&mut self) {
        self.focused_app_known = super::host::focused_application_known();
        self.workspace = crate::app_paths::personal_commands_workspace()
            .unwrap_or_else(|_| PathBuf::from("~/.config/hex"));
        self.read_status();
    }

    fn read_status(&mut self) -> bool {
        self.status_read = Some(Instant::now());
        let exists = crate::personal_commands::workspace_configured();
        let mut changed = exists != self.config_exists;
        self.config_exists = exists;
        let mut status =
            crate::personal_commands::load_status().unwrap_or_else(|error| StatusSnapshot {
                host_state: HostState::Unavailable,
                last_reload_error: Some(error.to_string()),
                ..StatusSnapshot::default()
            });
        crate::personal_commands::include_builtin_transformations(&mut status);
        if status != self.status {
            self.status = status;
            changed = true;
        }
        let approved = crate::personal_commands::approved_config();
        if approved != self.approved {
            self.approved = approved;
            changed = true;
        }
        changed
    }

    /// The selected mode's drafts applied to `settings`.
    pub(super) fn project(&self, settings: &mut AppSettings) {
        let selection = self.selected;
        let Some(mode) = mode_mut(settings, selection) else {
            return;
        };
        let editor = &self.editor;
        if selection != Selection::Default {
            mode.name = editor.name.text.trim().to_string();
        }
        mode.replacements = editor
            .replacements
            .iter()
            .map(|(phrase, output)| TextReplacement {
                matched_phrase: phrase.text.clone(),
                output: output.text.clone(),
            })
            .collect();
        mode.post_processing.prompt = editor.prompt.text.trim().to_string();
        mode.post_processing.deadline_seconds = editor
            .deadline
            .text
            .trim()
            .parse::<u64>()
            .unwrap_or(mode.post_processing.deadline_seconds)
            .max(1);
    }

    /// Saved names identify modes across external reordering. A removed,
    /// renamed, or ambiguous name drops its draft rather than editing a sibling.
    pub(super) fn adopt(&mut self, settings: &AppSettings, dirty: bool) {
        if self.selected != Selection::Default {
            let mut matches = settings
                .dictation_processing
                .modes
                .iter()
                .enumerate()
                .filter(|(_, mode)| mode.name == self.editor.base.name);
            let found = matches.next();
            if let Some((index, _)) = found
                && matches.next().is_none()
            {
                self.selected = Selection::Custom(index);
            } else {
                self.select_in(settings, Selection::Default);
                return;
            }
        }
        if let Some(current) = mode(settings, self.selected) {
            self.editor.rebase(current, dirty);
        }
    }

    pub(super) fn committed(&mut self, settings: &AppSettings) {
        self.editor.base.name = self.editor.name.text.trim().to_owned();
        self.adopt(settings, false);
    }

    pub(super) fn select(&mut self, selection: Selection) {
        self.selected = selection;
        self.delete_armed = None;
        self.application_picker_open = false;
        self.application_error = None;
        self.drag = None;
    }

    fn select_in(&mut self, settings: &AppSettings, selection: Selection) {
        self.select(selection);
        if let Some(current) = mode(settings, selection) {
            self.editor = Editor::of(current);
        } else {
            self.select(Selection::Default);
            self.editor = Editor::of(&settings.dictation_processing.default_mode);
        }
    }

    fn application_query(&self) -> String {
        self.application_search.text.trim().to_lowercase()
    }

    pub(super) fn poll(&mut self, live: bool) -> bool {
        let mut changed = false;
        if let Applications::Loading(receiver) = &self.applications
            && let Ok(applications) = receiver.try_recv()
        {
            self.applications = Applications::Loaded(applications);
            changed = true;
        }
        let ready = self
            .icons
            .get_mut()
            .values()
            .filter(|icon| icon.is_ready())
            .count();
        if ready != self.icons_ready {
            self.icons_ready = ready;
            changed = true;
        }
        if let Some(receiver) = &self.workspace_receiver
            && let Ok(result) = receiver.try_recv()
        {
            self.workspace_receiver = None;
            match result {
                Ok(workspace) => {
                    self.workspace = workspace;
                    self.workspace_error = None;
                }
                Err(error) => self.workspace_error = Some(error),
            }
            changed = true;
        }
        if live
            && self
                .status_read
                .is_none_or(|at| at.elapsed() >= STATUS_INTERVAL)
        {
            changed |= self.read_status();
        }
        changed
    }

    /// The decoded icon for a catalog path, requesting it when first shown.
    /// None while it loads, when it has none, or without a cache.
    fn icon(&self, path: &Path) -> Option<rune::Pixels> {
        let mut icons = self.icons.borrow_mut();
        if !icons.contains_key(path) {
            let cache = self.icon_cache.as_ref()?;
            if icons.len() >= MAX_ICONS {
                icons.clear();
            }
            icons.insert(path.to_path_buf(), cache.app_icon(path, ICON_SIZE));
        }
        let icon = &icons[path];
        (icon.is_ready() && icon.error().is_none()).then(|| icon.pixels().clone())
    }

    /// The icon for an application a mode names, once the catalog lists it.
    pub(super) fn named_icon(&self, name: &str) -> Option<rune::Pixels> {
        let Applications::Loaded(applications) = &self.applications else {
            return None;
        };
        let application = applications.iter().find(|a| a.name == name)?;
        self.icon(&application.path)
    }

    pub(super) fn ensure_applications(&mut self) {
        if matches!(self.applications, Applications::NotLoaded) {
            let (sender, receiver) = sync_channel(1);
            thread::spawn(move || {
                let _ = sender.send(crate::application_catalog::discover());
            });
            self.applications = Applications::Loading(receiver);
        }
    }

    fn application_choices(&self, selected: &[String]) -> Vec<&InstalledApplication> {
        let Applications::Loaded(applications) = &self.applications else {
            return Vec::new();
        };
        let query = self.application_query();
        applications
            .iter()
            .filter(|application| {
                !selected.contains(&application.name)
                    && (query.is_empty()
                        || application.name.to_lowercase().contains(&query)
                        || application
                            .bundle_id
                            .as_deref()
                            .is_some_and(|bundle| bundle.to_lowercase().contains(&query)))
            })
            .take(10)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Edits

fn select_mode(ui: &mut Ui, selection: Selection) {
    if ui.modes.selected == selection {
        return;
    }
    ui.flush_drafts();
    ui.open = None;
    ui.modes.select_in(&ui.settings, selection);
}

/// Saves one change to the selected mode.
fn update_mode(ui: &mut Ui, change: impl FnOnce(&mut DictationMode)) -> bool {
    let selection = ui.modes.selected;
    if mode(&ui.settings, selection).is_none() {
        return false;
    }
    ui.update(|settings| {
        if let Some(mode) = mode_mut(settings, selection) {
            change(mode);
        }
    })
}

fn add_mode(ui: &mut Ui) {
    ui.flush_drafts();
    let mut new = ui.settings.dictation_processing.default_mode.clone();
    new.name = tr_fill(
        "Mode {}",
        &(ui.settings.dictation_processing.modes.len() + 1).to_string(),
    );
    new.applications.clear();
    new.browser_hosts.clear();
    let index = ui.settings.dictation_processing.modes.len();
    if ui.update(|settings| settings.dictation_processing.modes.push(new)) {
        ui.modes.select_in(&ui.settings, Selection::Custom(index));
    }
}

fn delete_mode(ui: &mut Ui) {
    let Selection::Custom(index) = ui.modes.selected else {
        return;
    };
    let Some(current) = mode(&ui.settings, ui.modes.selected) else {
        return;
    };
    if ui.modes.delete_armed.as_deref() != Some(&current.name) {
        ui.modes.delete_armed = Some(current.name.clone());
        return;
    }
    // The drafts belong to the mode going away.
    ui.modes.editor = Editor::of(current);
    if ui.update(|settings| {
        settings.dictation_processing.modes.remove(index);
    }) {
        ui.modes.select_in(&ui.settings, Selection::Default);
    }
}

fn add_replacement(ui: &mut Ui) {
    if update_mode(ui, |mode| {
        mode.replacements.push(TextReplacement::default())
    }) && let Some((phrase, _)) = ui.modes.editor.replacements.last_mut()
    {
        phrase.focused = true;
    }
}

fn remove_replacement(ui: &mut Ui, index: usize) {
    if index < ui.modes.editor.replacements.len() {
        ui.modes.editor.replacements.remove(index);
        ui.flush_drafts();
    }
}

/// The applications the picker's target already has.
fn picked(ui: &Ui) -> Vec<String> {
    match ui.modes.picker_target {
        PickerTarget::Mode => mode(&ui.settings, ui.modes.selected)
            .map(|mode| mode.applications.clone())
            .unwrap_or_default(),
        PickerTarget::YoloMode => ui.settings.labs.yolo_mode_apps.clone(),
    }
}

/// Opens or closes the application picker for `target`; opening it for
/// another target moves it there.
pub(super) fn toggle_picker(ui: &mut Ui, target: PickerTarget) {
    let modes = &mut ui.modes;
    modes.application_picker_open =
        !(modes.application_picker_open && modes.picker_target == target);
    modes.picker_target = target;
    modes.application_error = None;
    modes.ensure_applications();
    modes.application_search = TextEdit::default();
    modes.application_search.focused = modes.application_picker_open;
}

fn add_application(ui: &mut Ui, name: String) {
    if ui.modes.picker_target == PickerTarget::YoloMode {
        if !ui.settings.labs.yolo_mode_apps.contains(&name) {
            ui.update(|settings| {
                let apps = &mut settings.labs.yolo_mode_apps;
                apps.push(name);
                apps.sort_by_key(|name| name.to_lowercase());
            });
        }
        close_picker(ui);
        return;
    }
    let selection = ui.modes.selected;
    let conflict = ui
        .settings
        .dictation_processing
        .modes
        .iter()
        .enumerate()
        .find(|(index, mode)| {
            Selection::Custom(*index) != selection && mode.applications.contains(&name)
        })
        .map(|(_, mode)| mode.name.clone());
    if let Some(conflict) = conflict {
        ui.modes.application_error = Some(tr_named(
            "{name} already activates {conflict}. Remove it there before reassigning it.",
            &[("name", &name), ("conflict", &conflict)],
        ));
        return;
    }
    if mode(&ui.settings, selection).is_some_and(|mode| !mode.applications.contains(&name))
        && update_mode(ui, |mode| {
            mode.applications.push(name);
            mode.applications.sort_by_key(|name| name.to_lowercase());
        })
    {
        ui.modes.application_error = None;
        close_picker(ui);
    }
}

/// Closes the application picker, clearing its search.
pub(super) fn close_picker(ui: &mut Ui) {
    let modes = &mut ui.modes;
    modes.application_picker_open = false;
    modes.application_search = TextEdit::default();
    modes.application_highlight = Highlight::default();
}

/// Up and Down move the application search's highlight.
fn move_application_highlight(ui: &mut Ui, event: &rune::KeyEvent) -> bool {
    let selected = picked(ui);
    let query = ui.modes.application_query();
    let count = ui.modes.application_choices(&selected).len();
    ui.modes
        .application_highlight
        .key(&event.key, &query, count)
}

/// Opens the native file dialog for an application the catalog does not
/// list: a macOS bundle, a Windows executable, or a Linux `.desktop` entry
/// or executable.
fn choose_application(ui: &mut Ui) {
    if !ui.live() || ui.modes.chooser.is_some() {
        return;
    }
    let waker = ui.modes.waker.clone();
    ui.modes.chooser = Some(crate::desktop::file_dialog::open(
        application_dialog(),
        move || {
            if let Some(waker) = &waker {
                waker();
            }
        },
    ));
}

fn application_dialog() -> Dialog {
    let applications = |extensions| Filter {
        label: tr("Applications").into(),
        extensions,
    };
    let any_file = || Filter {
        label: tr("All files").into(),
        extensions: &["*"],
    };
    let (filters, directory) = if cfg!(target_os = "macos") {
        (vec![applications(&["app"])], Some("/Applications".into()))
    } else if cfg!(target_os = "windows") {
        (vec![applications(&["exe"]), any_file()], None)
    } else {
        (vec![applications(&["desktop"]), any_file()], None)
    };
    Dialog {
        title: tr("Add Application").into(),
        target: DialogTarget::File(filters),
        directory,
    }
}

/// Adds what the file dialog returned: the application joins
/// the catalog and the selected mode.
pub(super) fn poll_chooser(ui: &mut Ui) -> bool {
    let Some(choice) = ui.modes.chooser.as_ref().and_then(Pending::poll) else {
        return false;
    };
    ui.modes.chooser = None;
    chosen_application(ui, application_from(choice));
    true
}

fn application_from(choice: Choice) -> ChosenApplication {
    choice.and_then(|path| {
        path.map(|path| {
            crate::application_catalog::metadata(&path).map_err(|error| format!("{error:#}"))
        })
        .transpose()
    })
}

pub(super) fn chosen_application(ui: &mut Ui, result: ChosenApplication) {
    match result {
        Ok(Some(application)) => {
            let name = application.name.clone();
            match &mut ui.modes.applications {
                Applications::Loaded(applications) => {
                    crate::application_catalog::insert(applications, application);
                }
                applications => *applications = Applications::Loaded(vec![application]),
            }
            ui.modes.application_error = None;
            add_application(ui, name);
        }
        Ok(None) => {}
        Err(error) => ui.modes.application_error = Some(error),
    }
}

/// Drags a transformation by its handle. Rows are keyed by position, so the
/// pressed slot stays put while the rows reorder under it; the new order is
/// saved once, on release. A cancelled drag drops the pending order.
fn drag_transformation(ui: &mut Ui, id: &str, start: usize, pitch: f32, d: &Drag) {
    match d.phase {
        Phase::Down => {
            let Some(current) = mode(&ui.settings, ui.modes.selected) else {
                return;
            };
            let committed = current.transformations.clone();
            ui.modes.drag = Some(TransformationDrag {
                id: id.to_string(),
                start,
                order: committed.clone(),
                committed,
            });
        }
        Phase::Move => {
            let Some(drag) = &mut ui.modes.drag else {
                return;
            };
            let moved = ((d.y - d.start_y) / pitch).round() as isize;
            let last = drag.committed.len().saturating_sub(1) as isize;
            let target = (drag.start as isize + moved).clamp(0, last) as usize;
            let mut order = drag.committed.clone();
            reorder_transformation(&mut order, &drag.id, target);
            drag.order = order;
        }
        Phase::Up => {
            let Some(drag) = ui.modes.drag.take() else {
                return;
            };
            if !d.cancelled() && drag.order != drag.committed {
                update_mode(ui, |mode| mode.transformations = drag.order);
            }
        }
    }
}

/// Moves `id` to `target_index`; false when it is missing or already there.
fn reorder_transformation(values: &mut Vec<String>, id: &str, target_index: usize) -> bool {
    let Some(from) = values.iter().position(|candidate| candidate == id) else {
        return false;
    };
    if from == target_index {
        return false;
    }
    let value = values.remove(from);
    values.insert(target_index.min(values.len()), value);
    true
}

fn provision_workspace(ui: &mut Ui) {
    if !ui.live() || ui.modes.workspace_receiver.is_some() {
        return;
    }
    let (sender, receiver) = sync_channel(1);
    thread::spawn(move || {
        let result =
            crate::personal_commands::initialize_workspace().map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    ui.modes.workspace_receiver = Some(receiver);
}

// ---------------------------------------------------------------------------
// View

pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    let add = IconButton::new(PLUS)
        .variant(IconButtonVariant::Neutral)
        .control_size(ControlSize::Regular)
        .label(tr("Add mode"))
        .on_press(add_mode)
        .build(p.k)
        .key("add-mode");
    let c = &p.k.c;
    let mut list = col().key("modes-list");
    list = list.child(mode_row(
        p,
        "mode-global",
        tr("Global"),
        Vec::new(),
        tr("Fallback for everything"),
        ui.modes.selected == Selection::Default,
        Selection::Default,
    ));
    for (index, mode) in ui.settings.dictation_processing.modes.iter().enumerate() {
        let summary = if mode.applications.is_empty() {
            tr("No activation rules").to_string()
        } else {
            mode.applications.join(", ")
        };
        // The first few applications' icons trail the name.
        let icons = mode
            .applications
            .iter()
            .filter_map(|name| ui.modes.named_icon(name))
            .take(3)
            .enumerate()
            .map(|(index, icon)| application_icon(format!("icon{index}"), &icon, 16.0))
            .collect();
        list = list.child(mode_row(
            p,
            &format!("mode{index}"),
            &mode.name,
            icons,
            &summary,
            ui.modes.selected == Selection::Custom(index),
            Selection::Custom(index),
        ));
    }
    let list = col()
        .key("modes-column")
        .w(LIST_WIDTH)
        .h_full()
        .pl(LIST_COLUMN_PL)
        .pr(8.0)
        .child(super::scroll_body(p.k, "modes-scroll", source_list(list)));
    let body = row()
        .key("modes-body")
        .grow(1.0)
        .shrink()
        .child(list)
        .child(
            el().key("modes-rule")
                .w(p.hair)
                .h_full()
                .fill(c.border_weak),
        )
        .child(detail(ui, p, layer));
    (vec![add], body)
}

fn mode_row(
    p: Paint,
    key: &str,
    name: &str,
    icons: Vec<El>,
    summary: &str,
    selected: bool,
    selection: Selection,
) -> El {
    list_row(p, key, name, icons, summary, selected, move |ui| {
        select_mode(ui, selection)
    })
}

fn detail(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let selection = ui.modes.selected;
    let Some(current) = mode(&ui.settings, selection) else {
        return el();
    };
    let mut content = page("mode-detail-content");
    content = content.child(basics(ui, p, selection, current, layer));
    content = content.child(corrections(ui, p));
    content = content.child(processing(ui, p, selection, current, layer));
    content = content.child(transformations(ui, p, current, layer));
    if selection != Selection::Default {
        let action = if ui.modes.delete_armed.as_deref() == Some(&current.name) {
            row()
                .key("delete-actions")
                .gap(8.0)
                .child(button(
                    p,
                    "cancel-delete-mode",
                    tr("Cancel"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.modes.delete_armed = None,
                ))
                .child(button(
                    p,
                    "confirm-delete-mode",
                    tr("Confirm delete"),
                    ButtonKind::Danger,
                    |ui: &mut Ui, _| delete_mode(ui),
                ))
        } else {
            button(
                p,
                "delete-mode",
                tr("Delete mode"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| delete_mode(ui),
            )
        };
        content = content.child(row().key("delete-row").justify_end().child(action));
    }
    col()
        .key("mode-detail")
        .grow(1.0)
        .shrink()
        .child(super::scroll_body(p.k, "mode-detail-scroll", content))
}

fn basics(
    ui: &Ui,
    p: Paint,
    selection: Selection,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> El {
    if selection == Selection::Default {
        let mut rows = vec![setting_row(
            p,
            "global",
            tr("Global"),
            Some(tr("Used unless a more specific mode matches.")),
            el().key("none"),
        )];
        if let Some(speech) = speech_row(ui, p, selection, current, layer) {
            rows.push(speech);
        }
        return card(p, "basics-card", rows);
    }
    let name = TextInput::new(&ui.modes.editor.name)
        .placeholder(tr("Mode name"))
        .width(Some(FIELD_WIDTH))
        .on_edit(|ui: &mut Ui| &mut ui.modes.editor.name)
        .build(p.k)
        .key("mode-name");
    let mut chips = row()
        .key("applications")
        .gap(4.0)
        .center_y()
        .flex_wrap()
        .max_w(FIELD_WIDTH);
    for (index, name) in current.applications.iter().enumerate() {
        let remove = name.clone();
        chips = chips.child(application_chip(ui, p, index, name, move |ui: &mut Ui| {
            let remove = remove.clone();
            update_mode(ui, |mode| {
                mode.applications
                    .retain(|application| *application != remove)
            });
        }));
    }
    chips = chips.child(
        IconButton::new(PLUS)
            .variant(IconButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .label(tr("Add application"))
            .on_press(|ui: &mut Ui| toggle_picker(ui, PickerTarget::Mode))
            .build(p.k)
            .key("open-application-picker"),
    );
    let mut rows = vec![
        setting_row(p, "name", tr("Name"), None, name),
        setting_row(
            p,
            "applications-row",
            tr("Applications"),
            Some(if ui.modes.focused_app_known {
                tr("Switch when an app is frontmost")
            } else {
                tr("Wayland doesn't report the frontmost app; these apply under X11")
            }),
            chips,
        ),
    ];
    if let Some(speech) = speech_row(ui, p, selection, current, layer) {
        rows.push(speech);
    }
    let picking = ui.modes.application_picker_open && ui.modes.picker_target == PickerTarget::Mode;
    if let Some(error) = ui.modes.application_error.as_ref().filter(|_| !picking) {
        rows.push(note("application-error", error, p.danger()));
    }
    if picking {
        application_picker(
            ui,
            p,
            &current.applications,
            "open-application-picker",
            layer,
        );
    }
    card(p, "basics-card", rows)
}

/// The selected mode's speech model and language, while Labs › Switch model
/// with app is on; None leaves the Modes pane exactly as it was.
fn speech_row(
    ui: &Ui,
    p: Paint,
    selection: Selection,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> Option<El> {
    if !speech_model::enabled(ui) {
        return None;
    }
    let suffix = match selection {
        Selection::Default => "global".to_string(),
        Selection::Custom(index) => index.to_string(),
    };
    let key = format!("mode-speech-{suffix}");
    let model_key = format!("mode-speech-model-{suffix}");
    let language_key = format!("mode-speech-language-{suffix}");
    Some(speech_model::setting(
        ui,
        p,
        speech_model::SpeechRow {
            key: &key,
            model_key: &model_key,
            language_key: &language_key,
            title: tr("Speech model"),
            description: None,
            choice: current.speech.clone(),
            model_menu: Menu::ModeSpeechModel(selection),
            language_menu: Menu::ModeSpeechLanguage(selection),
        },
        // A new choice starts on the model's last-used language, or the
        // saved one when that model has no recent choice.
        |_, fallback| fallback.to_owned(),
        move |ui, choice| {
            update_mode(ui, |mode| mode.speech = choice);
        },
        layer,
    ))
}

/// One chosen application: its icon once the catalog lists it, its name,
/// and a remove button.
pub(super) fn application_chip(
    ui: &Ui,
    p: Paint,
    index: usize,
    name: &str,
    remove: impl Fn(&mut Ui) + 'static,
) -> El {
    let icon = ui.modes.named_icon(name);
    row()
        .key(format!("application{index}"))
        .h(24.0)
        .pl(if icon.is_some() { 4.0 } else { 8.0 })
        .gap(if icon.is_some() { 4.0 } else { 2.0 })
        .center_y()
        .radius(p.k.radius.sm)
        .fill(p.k.c.layer_02)
        .children(icon.map(|icon| application_icon("icon", &icon, 16.0)))
        .child(
            text(name.to_owned())
                .key("name")
                .font(p.k.ty.small_medium)
                .color(p.text()),
        )
        .child(remove_button(p, ControlSize::Small, remove))
}

/// The catalog search, results, and Choose a file…, floating under the
/// element keyed `anchor` (the + that opened it), adding to the picker's
/// target; `selected` is what the target already has. Picking an app, a
/// click outside, or Escape closes it.
pub(super) fn application_picker(
    ui: &Ui,
    p: Paint,
    selected: &[String],
    anchor: &'static str,
    layer: &mut Vec<El>,
) {
    let c = &p.k.c;
    let search = TextInput::new(&ui.modes.application_search)
        .placeholder(tr("Search applications"))
        .leading_icon(p.k.glyph.search)
        .width(None)
        .on_edit(|ui: &mut Ui| &mut ui.modes.application_search)
        .on_key(|ui: &mut Ui, event: &rune::KeyEvent, _: &mut Tx| {
            move_application_highlight(ui, event)
        })
        .on_submit(|ui: &mut Ui, _: &mut Tx| {
            let picked = {
                let selected = picked(ui);
                let query = ui.modes.application_query();
                let choices = ui.modes.application_choices(&selected);
                ui.modes
                    .application_highlight
                    .index(&query, choices.len())
                    .map(|index| choices[index].name.clone())
            };
            if let Some(name) = picked {
                add_application(ui, name);
            }
        })
        .build(p.k)
        .key("application-search");
    let choices = ui.modes.application_choices(selected);
    let query = ui.modes.application_query();
    let highlighted = ui.modes.application_highlight.index(&query, choices.len());
    let status = match &ui.modes.applications {
        Applications::NotLoaded | Applications::Loading(_) => {
            Some(tr("Finding applications…").to_string())
        }
        Applications::Loaded(all) if all.is_empty() => {
            Some(tr("No applications found.").to_string())
        }
        Applications::Loaded(_) if choices.is_empty() && !query.is_empty() => {
            Some(tr_fill("No applications match \"{}\".", &query))
        }
        Applications::Loaded(_) => None,
    };
    let mut results = col().key("application-results");
    for (index, application) in choices.into_iter().enumerate() {
        let name = application.name.clone();
        let location = application
            .path
            .parent()
            .map_or_else(String::new, |path| path.display().to_string());
        let fill = if highlighted == Some(index) {
            c.hover
        } else {
            rune::Color::TRANSPARENT
        };
        results = results.child(
            row()
                .key(format!("available{index}"))
                .fill(fill)
                .role("button")
                .label(application.name.clone())
                .px(8.0)
                .py(6.0)
                .gap(10.0)
                .center_y()
                .radius(p.k.radius.sm)
                .hover(c.hover)
                .press(c.pressed)
                .on_press(move |ui: &mut Ui| add_application(ui, name.clone()))
                .child(match ui.modes.icon(&application.path) {
                    Some(icon) => application_icon("icon", &icon, ICON_SIZE),
                    None => monogram(p, &application.name),
                })
                .child(
                    col()
                        .key("copy")
                        .grow(1.0)
                        .shrink()
                        .child(
                            text(application.name.clone())
                                .key("name")
                                .font(p.k.ty.ui_medium)
                                .color(p.text())
                                .truncate(),
                        )
                        .child(
                            text(location)
                                .key("location")
                                .font(p.k.ty.small)
                                .color(p.muted())
                                .truncate(),
                        ),
                ),
        );
    }
    let mut picker = col()
        .key("application-picker")
        .role("dialog")
        .label(tr("Add application"))
        .focus_return(anchor)
        .modal(true, close_picker)
        .w(PICKER_WIDTH)
        .shrink()
        .min_h(0.0)
        .padding(8.0)
        .gap(6.0)
        .radius(p.k.radius.md)
        .fill(c.layer_01)
        .child(search)
        .children(ui.modes.application_error.as_ref().map(|error| {
            text(error.clone())
                .key("application-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .px(8.0)
                .wrap()
        }))
        .children(status.map(|status| {
            text(status)
                .key("status")
                .font(DESCRIPTION)
                .color(p.muted())
                .px(8.0)
                .py(6.0)
        }))
        .child(
            col()
                .key("viewport")
                .shrink()
                .min_h(0.0)
                .max_h(PICKER_RESULTS_HEIGHT)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(results),
        )
        .child(row().key("picker-actions").child(choose_file_button(p)))
        .anchored_fit(anchor, Side::Bottom, Align::Start, 4.0);
    for shadow in &p.k.elevation.floating {
        picker = picker.box_shadow(*shadow);
    }
    layer.push(
        el().key("application-picker-scrim")
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .on_press(close_picker),
    );
    layer.push(picker);
}

const PICKER_WIDTH: f32 = 340.0;
const PICKER_RESULTS_HEIGHT: f32 = 320.0;

/// "Choose a file…" opens the native file dialog.
fn choose_file_button(p: Paint) -> El {
    button(
        p,
        "choose-application-file",
        tr("Choose a file…"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| choose_application(ui),
    )
}

pub(super) fn application_icon(key: impl Into<String>, icon: &rune::Pixels, size: f32) -> El {
    el().key(key)
        .size(size, size)
        .pixels_fit(icon, rune::SurfaceFit::Fit)
}

/// An app's initial on a tinted square, while its icon loads or when it has
/// none.
pub(super) fn monogram(p: Paint, name: &str) -> El {
    let letter = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or("?".into(), |c| c.to_uppercase().to_string());
    row()
        .key("monogram")
        .size(24.0, 24.0)
        .center()
        .radius(p.k.radius.sm)
        .fill(p.k.c.layer_03)
        .child(
            text(letter)
                .key("letter")
                .font(p.k.ty.small_medium)
                .color(p.text()),
        )
}

fn corrections(ui: &Ui, p: Paint) -> El {
    let add = IconButton::new(PLUS)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Regular)
        .label(tr("Add correction"))
        .on_press(add_replacement)
        .build(p.k)
        .key("add-correction");
    let mut rows = Vec::new();
    if ui.modes.editor.replacements.is_empty() {
        rows.push(note(
            "no-corrections",
            tr("No corrections in this mode."),
            p.muted(),
        ));
    }
    for (index, (phrase, output)) in ui.modes.editor.replacements.iter().enumerate() {
        rows.push(
            row()
                .key(format!("correction{index}"))
                .px(METRICS.row_px)
                .py(6.0)
                .gap(8.0)
                .center_y()
                .child(
                    TextInput::new(phrase)
                        .placeholder(tr("Spoken phrase (e.g. open code)"))
                        .width(None)
                        .on_edit(edit_access(move |ui: &mut Ui| {
                            &mut ui.modes.editor.replacements[index].0
                        }))
                        .build(p.k)
                        .key("phrase")
                        .grow(1.0)
                        .shrink(),
                )
                .child(text("→").key("arrow").font(p.k.ty.ui).color(p.faint()))
                .child(
                    TextInput::new(output)
                        .placeholder(tr("Replacement (e.g. OpenCode)"))
                        .width(None)
                        .on_edit(edit_access(move |ui: &mut Ui| {
                            &mut ui.modes.editor.replacements[index].1
                        }))
                        .build(p.k)
                        .key("output")
                        .grow(1.0)
                        .shrink(),
                )
                .child(remove_button(
                    p,
                    ControlSize::Regular,
                    move |ui: &mut Ui| remove_replacement(ui, index),
                )),
        );
    }
    section_with(p, "corrections", tr("Corrections"), add.mr(-4.0), rows)
}

fn processing(
    ui: &Ui,
    p: Paint,
    selection: Selection,
    current: &DictationMode,
    layer: &mut Vec<El>,
) -> El {
    if super::platform_settings::opencode_managed() {
        let row = super::controls::managed_row(
            p,
            "processing-enabled",
            None,
            tr("OpenCode transformation"),
        );
        return section(p, "processing", tr("Processing"), vec![row]);
    }
    let enabled = current.post_processing.enabled;
    let can_toggle = enabled || ui.models.available();
    let toggle = Switch::new(enabled)
        .disabled(!can_toggle)
        .on_toggle(|ui: &mut Ui| {
            let Some(current) = mode(&ui.settings, ui.modes.selected) else {
                return;
            };
            let enabled = !current.post_processing.enabled;
            update_mode(ui, |mode| mode.post_processing.enabled = enabled);
        })
        .build(p.k)
        .key("processing-toggle")
        .label(tr("OpenCode transformation"));
    let reason = (!can_toggle)
        .then(|| models::unavailable_reason(ui, OpenCodeFeature::Transformation))
        .flatten();
    let mut rows = vec![setting_row(
        p,
        "processing-enabled",
        tr("OpenCode transformation"),
        reason,
        toggle,
    )];
    if enabled {
        rows.extend(models::rows(ui, p, Target::Mode(selection), layer));
        rows.push(setting_row(
            p,
            "deadline",
            tr("Deadline"),
            Some(tr("Seconds")),
            TextInput::new(&ui.modes.editor.deadline)
                .label(tr("Deadline"))
                .placeholder("30")
                .width(Some(96.0))
                .on_edit(|ui: &mut Ui| &mut ui.modes.editor.deadline)
                .build(p.k)
                .key("deadline-field"),
        ));
        rows.push(
            col()
                .key("instructions")
                .px(METRICS.row_px)
                .py(8.0)
                .gap(6.0)
                .child(
                    text(tr("Instructions"))
                        .key("title")
                        .font(p.k.ty.ui)
                        .color(p.text()),
                )
                .child(
                    text(tr(
                        "System instructions for rewriting dictation in this mode",
                    ))
                    .key("description")
                    .font(DESCRIPTION)
                    .color(p.muted()),
                )
                .child(
                    TextArea::new(&ui.modes.editor.prompt)
                        .placeholder(tr("Describe how OpenCode should rewrite your dictation…"))
                        .rows(4)
                        .width(None)
                        .on_edit(|ui: &mut Ui| &mut ui.modes.editor.prompt)
                        .build(p.k)
                        .key("prompt"),
                ),
        );
    }
    if let Some(notice) = models::unavailable(ui, p, OpenCodeFeature::Transformation) {
        rows.push(notice);
    }
    section(p, "processing", tr("Processing"), rows)
}

// ---------------------------------------------------------------------------
// Fixture

/// Representative modes: a chat mode with corrections, rewriting, and
/// transformations, and an empty one.
pub(super) fn fixture(ui: &mut Ui) {
    let mut chat = ui.settings.dictation_processing.default_mode.clone();
    chat.name = "Chat".into();
    chat.applications = vec!["Firefox".into(), "Slack".into()];
    chat.replacements = vec![
        TextReplacement {
            matched_phrase: "open code".into(),
            output: "OpenCode".into(),
        },
        TextReplacement {
            matched_phrase: "hex app".into(),
            output: "Hex".into(),
        },
    ];
    chat.transformations = vec!["smart-punctuation".into()];
    chat.post_processing.enabled = true;
    chat.post_processing.model = Some("anthropic/claude-sonnet-4-5".into());
    chat.post_processing.variant = Some("high".into());
    let mut code = ui.settings.dictation_processing.default_mode.clone();
    code.name = "Terminal".into();
    code.applications = vec!["WezTerm".into()];
    ui.settings.dictation_processing.modes = vec![chat, code];
    ui.modes.status.transformations = vec![
        StatusTransformation {
            id: "smart-punctuation".into(),
            name: "Smart punctuation".into(),
            description: Some("Curly quotes, dashes, and ellipses".into()),
        },
        StatusTransformation {
            id: "normalize-links".into(),
            name: "Normalize links".into(),
            description: None,
        },
    ];
    fixture_catalog(ui);
    ui.modes.select_in(&ui.settings, Selection::Custom(0));
}

/// The fixture's installed applications and their stand-in icons.
pub(super) fn fixture_catalog(ui: &mut Ui) {
    let app = |name: &str, path: &str| InstalledApplication {
        name: name.into(),
        bundle_id: None,
        path: PathBuf::from(path),
    };
    ui.modes.applications = Applications::Loaded(vec![
        app("Firefox", "/usr/share/applications/firefox.desktop"),
        app("Slack", "/usr/share/applications/slack.desktop"),
        app("Visual Studio Code", "/usr/share/applications/code.desktop"),
        app(
            "WezTerm",
            "/usr/share/applications/org.wezfurlong.wezterm.desktop",
        ),
        app("Zed", "/usr/share/applications/dev.zed.Zed.desktop"),
    ]);
    // Stand-in icons, so renders never depend on the host's installed apps.
    // Zed has none and keeps its monogram.
    let cache = ui
        .modes
        .icon_cache
        .get_or_insert_with(|| rune::ImageCache::new(std::sync::Arc::new(|| {})));
    let icons = [
        ("firefox", [255, 113, 57]),
        ("slack", [74, 21, 75]),
        ("code", [0, 122, 204]),
        ("org.wezfurlong.wezterm", [52, 58, 64]),
    ]
    .map(|(id, rgb)| {
        let icon = cache.rgba(FIXTURE_ICON, FIXTURE_ICON, fixture_icon(rgb));
        let _ = icon.wait();
        (
            PathBuf::from(format!("/usr/share/applications/{id}.desktop")),
            icon,
        )
    });
    ui.modes.icons.get_mut().extend(icons);
}

const FIXTURE_ICON: u32 = 48;

/// A rounded square in `rgb` with a lighter disc, as straight-alpha RGBA.
fn fixture_icon(rgb: [u8; 3]) -> Vec<u8> {
    let centre = FIXTURE_ICON as f32 / 2.0;
    let (half, corner) = (centre - 2.0, 10.0);
    let mut pixels = Vec::with_capacity((FIXTURE_ICON * FIXTURE_ICON * 4) as usize);
    for y in 0..FIXTURE_ICON {
        for x in 0..FIXTURE_ICON {
            let (dx, dy) = (x as f32 + 0.5 - centre, y as f32 + 0.5 - centre);
            let (qx, qy) = (dx.abs() - half + corner, dy.abs() - half + corner);
            let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - corner;
            let alpha = (0.5 - outside).clamp(0.0, 1.0);
            let disc = (0.5 - (dx.hypot(dy) - 9.0)).clamp(0.0, 1.0) * 0.55;
            let [r, g, b] =
                rgb.map(|v| (f32::from(v) + (255.0 - f32::from(v)) * disc).round() as u8);
            pixels.extend_from_slice(&[r, g, b, (alpha * 255.0).round() as u8]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AppSettings {
        let mut settings = AppSettings::default();
        settings.dictation_processing.modes = ["First", "Second"]
            .map(|name| DictationMode {
                name: name.into(),
                ..Default::default()
            })
            .to_vec();
        settings
    }

    #[test]
    fn dirty_name_keeps_only_that_field_when_other_fields_change() {
        let mut settings = settings();
        let mut pane = ModesPane::new(&settings);
        pane.select_in(&settings, Selection::Custom(1));
        pane.editor.name.text = "Draft".into();
        settings.dictation_processing.modes[1]
            .post_processing
            .prompt = "From CLI".into();
        settings.dictation_processing.modes[1]
            .post_processing
            .deadline_seconds = 17;
        pane.adopt(&settings, true);
        pane.project(&mut settings);
        let saved = &settings.dictation_processing.modes[1];
        assert_eq!(saved.name, "Draft");
        assert_eq!(saved.post_processing.prompt, "From CLI");
        assert_eq!(saved.post_processing.deadline_seconds, 17);
    }

    #[test]
    fn adding_a_correction_focuses_the_one_row_the_save_adopted() {
        let mut ui = Ui::with(settings(), super::super::Service::Fixture(Vec::new()));
        ui.modes.select_in(&ui.settings, Selection::Custom(1));
        let before = ui.modes.editor.replacements.len();
        add_replacement(&mut ui);
        assert_eq!(ui.modes.editor.replacements.len(), before + 1);
        let phrase = &mut ui.modes.editor.replacements.last_mut().unwrap().0;
        assert!(phrase.focused);
        phrase.text = "example".into();
        // Adding again also commits the first row's pending text.
        add_replacement(&mut ui);
        assert_eq!(ui.modes.editor.replacements.len(), before + 2);
        assert!(ui.modes.editor.replacements.last().unwrap().0.focused);
        ui.flush_drafts();
        assert_eq!(
            ui.settings.dictation_processing.modes[1].replacements[before].matched_phrase,
            "example"
        );
    }

    #[test]
    fn older_revisions_cannot_undo_a_save_and_newer_reverts_are_adopted() {
        let mut settings = settings();
        settings.source.revision = 1;
        let mut ui = Ui::with(settings.clone(), super::super::Service::Fixture(Vec::new()));
        ui.service_settings = Some(settings.clone());
        ui.settings.sound_effect_volume = 0.37;
        ui.settings.source.revision = 3;
        assert!(!ui.adopt(settings.clone()));
        assert_eq!(ui.settings.sound_effect_volume, 0.37);
        settings.source.revision = 4;
        assert!(ui.adopt(settings.clone()));
        assert_eq!(
            ui.settings.sound_effect_volume,
            settings.sound_effect_volume
        );
    }

    #[test]
    fn removed_mode_discards_its_dirty_editor_and_confirmation() {
        let mut settings = settings();
        let mut pane = ModesPane::new(&settings);
        pane.select_in(&settings, Selection::Custom(1));
        pane.editor.name.text = "Draft".into();
        pane.delete_armed = Some("Second".into());
        settings.dictation_processing.modes.pop();
        pane.adopt(&settings, true);
        assert_eq!(pane.selected, Selection::Default);
        assert!(pane.delete_armed.is_none());
        pane.project(&mut settings);
        assert_eq!(settings.dictation_processing.modes[0].name, "First");
        assert!(mode(&settings, Selection::Custom(1)).is_none());
        assert!(mode_mut(&mut settings, Selection::Custom(1)).is_none());
    }

    #[test]
    fn reordered_modes_keep_drafts_and_confirmation_on_the_saved_name() {
        let mut settings = settings();
        let mut pane = ModesPane::new(&settings);
        pane.select_in(&settings, Selection::Custom(1));
        pane.editor.name.text = "Draft".into();
        pane.delete_armed = Some("Second".into());
        settings.dictation_processing.modes.swap(0, 1);
        pane.adopt(&settings, true);
        assert_eq!(pane.selected, Selection::Custom(0));
        assert_eq!(pane.delete_armed.as_deref(), Some("Second"));
        pane.project(&mut settings);
        assert_eq!(settings.dictation_processing.modes[0].name, "Draft");
        assert_eq!(settings.dictation_processing.modes[1].name, "First");
    }

    #[test]
    fn ambiguous_names_never_retarget_a_draft() {
        let mut settings = settings();
        let mut pane = ModesPane::new(&settings);
        pane.select_in(&settings, Selection::Custom(1));
        settings.dictation_processing.modes[0].name = "Second".into();
        pane.adopt(&settings, true);
        assert_eq!(pane.selected, Selection::Default);
    }

    #[test]
    fn transformations_reorder_in_both_directions() {
        let mut transformations = vec!["one".to_string(), "two".into(), "three".into()];
        assert!(reorder_transformation(&mut transformations, "one", 2));
        assert_eq!(transformations, ["two", "three", "one"]);
        assert!(reorder_transformation(&mut transformations, "one", 0));
        assert_eq!(transformations, ["one", "two", "three"]);
    }

    #[test]
    fn transformation_reorder_ignores_same_position_and_missing_ids() {
        let mut transformations = vec!["one".to_string(), "two".into()];
        assert!(!reorder_transformation(&mut transformations, "one", 0));
        assert!(!reorder_transformation(&mut transformations, "missing", 1));
        assert_eq!(transformations, ["one", "two"]);
    }
}
