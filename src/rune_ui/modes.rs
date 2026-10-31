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
    Align, Drag, El, Phase, ScrollSpec, Side, TextEdit, Tx, Waker, col, edit_access, el, row,
    spacer, text,
};
use rune_kit::{ControlSize, IconButton, IconButtonVariant, Switch, TextArea, TextInput};

use super::controls::{
    ButtonKind, DESCRIPTION, Highlight, METRICS, Paint, button, card, note, page, section,
    section_with, setting_row,
};
use super::lists::{LIST_COLUMN_PL, list_row, remove_button, source_list};
use super::models::{self, Target};
use super::{Menu, Ui};
use crate::app_settings::{AppSettings, DictationMode};
use crate::application_catalog::InstalledApplication;
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::file_dialog::{Choice, Dialog, Filter, Pending, Target as DialogTarget};
use crate::opencode_catalog::OpenCodeFeature;
use crate::personal_commands::{HostState, StatusSnapshot, StatusTransformation};
use crate::text_replacements::TextReplacement;

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

pub(super) fn mode(settings: &AppSettings, selection: Selection) -> &DictationMode {
    match selection {
        Selection::Default => &settings.dictation_processing.default_mode,
        Selection::Custom(index) => &settings.dictation_processing.modes[index],
    }
}

pub(super) fn mode_mut(settings: &mut AppSettings, selection: Selection) -> &mut DictationMode {
    match selection {
        Selection::Default => &mut settings.dictation_processing.default_mode,
        Selection::Custom(index) => &mut settings.dictation_processing.modes[index],
    }
}

/// The selected mode's text fields.
#[derive(Default)]
struct Editor {
    name: TextEdit,
    prompt: TextEdit,
    deadline: TextEdit,
    replacements: Vec<(TextEdit, TextEdit)>,
}

impl Editor {
    fn of(mode: &DictationMode) -> Self {
        Self {
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

enum Applications {
    NotLoaded,
    Loading(Receiver<Vec<InstalledApplication>>),
    Loaded(Vec<InstalledApplication>),
}

pub(super) struct ModesPane {
    pub(super) selected: Selection,
    editor: Editor,
    pub(super) delete_armed: bool,
    pub(super) application_picker_open: bool,
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
            delete_armed: false,
            application_picker_open: false,
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
        changed
    }

    /// The selected mode's drafts applied to `settings`.
    pub(super) fn project(&self, settings: &mut AppSettings) {
        let selection = self.selected;
        if let Selection::Custom(index) = selection
            && index >= settings.dictation_processing.modes.len()
        {
            return;
        }
        let editor = &self.editor;
        let mode = mode_mut(settings, selection);
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

    /// Rebuilds the drafts from settings another client saved.
    pub(super) fn reload(&mut self, settings: &AppSettings) {
        if let Selection::Custom(index) = self.selected
            && index >= settings.dictation_processing.modes.len()
        {
            self.selected = Selection::Default;
        }
        self.editor = Editor::of(mode(settings, self.selected));
    }

    pub(super) fn select(&mut self, selection: Selection) {
        self.selected = selection;
        self.delete_armed = false;
        self.application_picker_open = false;
        self.application_error = None;
        self.drag = None;
    }

    fn select_in(&mut self, settings: &AppSettings, selection: Selection) {
        self.select(selection);
        self.editor = Editor::of(mode(settings, selection));
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
    ui.update(|settings| change(mode_mut(settings, selection)))
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
    if !ui.modes.delete_armed {
        ui.modes.delete_armed = true;
        return;
    }
    // The drafts belong to the mode going away.
    ui.modes.editor = Editor::of(mode(&ui.settings, ui.modes.selected));
    if ui.update(|settings| {
        settings.dictation_processing.modes.remove(index);
    }) {
        ui.modes.select_in(&ui.settings, Selection::Default);
    }
}

fn add_replacement(ui: &mut Ui) {
    if update_mode(ui, |mode| {
        mode.replacements.push(TextReplacement::default())
    }) {
        let mut phrase = TextEdit::default();
        phrase.focused = true;
        ui.modes
            .editor
            .replacements
            .push((phrase, TextEdit::default()));
    }
}

fn remove_replacement(ui: &mut Ui, index: usize) {
    if index < ui.modes.editor.replacements.len() {
        ui.modes.editor.replacements.remove(index);
        ui.flush_drafts();
    }
}

fn add_application(ui: &mut Ui, name: String) {
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
    if !mode(&ui.settings, selection).applications.contains(&name)
        && update_mode(ui, |mode| {
            mode.applications.push(name);
            mode.applications.sort_by_key(|name| name.to_lowercase());
        })
    {
        ui.modes.application_search = TextEdit::default();
        ui.modes.application_highlight = Highlight::default();
        ui.modes.application_search.focused = true;
        ui.modes.application_error = None;
    }
}

/// Up and Down move the application search's highlight.
fn move_application_highlight(ui: &mut Ui, event: &rune::KeyEvent) -> bool {
    let query = ui.modes.application_query();
    let count = ui
        .modes
        .application_choices(&mode(&ui.settings, ui.modes.selected).applications)
        .len();
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
    ui.modes.chooser = Some(crate::file_dialog::open(application_dialog(), move || {
        if let Some(waker) = &waker {
            waker();
        }
    }));
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
            let committed = mode(&ui.settings, ui.modes.selected)
                .transformations
                .clone();
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
        .child(super::scroll_body("modes-scroll", source_list(list)));
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
    let current = mode(&ui.settings, selection);
    let mut content = page("mode-detail-content");
    content = content.child(basics(ui, p, selection, current));
    content = content.child(corrections(ui, p));
    content = content.child(processing(ui, p, selection, current, layer));
    content = content.child(transformations(ui, p, current, layer));
    if selection != Selection::Default {
        let action = if ui.modes.delete_armed {
            row()
                .key("delete-actions")
                .gap(8.0)
                .child(button(
                    p,
                    "cancel-delete-mode",
                    tr("Cancel"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.modes.delete_armed = false,
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
        .child(super::scroll_body("mode-detail-scroll", content))
}

fn basics(ui: &Ui, p: Paint, selection: Selection, current: &DictationMode) -> El {
    if selection == Selection::Default {
        return card(
            p,
            "basics-card",
            vec![setting_row(
                p,
                "global",
                tr("Global"),
                Some(tr("Used unless a more specific mode matches.")),
                el().key("none"),
            )],
        );
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
        let icon = ui.modes.named_icon(name);
        chips = chips.child(
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
                    text(name.clone())
                        .key("name")
                        .font(p.k.ty.small_medium)
                        .color(p.text()),
                )
                .child(remove_button(p, ControlSize::Small, move |ui: &mut Ui| {
                    let remove = remove.clone();
                    update_mode(ui, |mode| {
                        mode.applications
                            .retain(|application| *application != remove)
                    });
                })),
        );
    }
    chips = chips.child(
        IconButton::new(PLUS)
            .variant(IconButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .label(tr("Add application"))
            .on_press(|ui: &mut Ui| {
                ui.modes.application_picker_open = !ui.modes.application_picker_open;
                ui.modes.application_error = None;
                ui.modes.ensure_applications();
                ui.modes.application_search = TextEdit::default();
                ui.modes.application_search.focused = ui.modes.application_picker_open;
            })
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
    if let Some(error) = &ui.modes.application_error {
        rows.push(note("application-error", error, p.danger()));
    }
    if ui.modes.application_picker_open {
        rows.push(application_picker(ui, p, current));
    }
    card(p, "basics-card", rows)
}

fn application_picker(ui: &Ui, p: Paint, current: &DictationMode) -> El {
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
                let selected = &mode(&ui.settings, ui.modes.selected).applications;
                let query = ui.modes.application_query();
                let choices = ui.modes.application_choices(selected);
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
    let choices = ui.modes.application_choices(&current.applications);
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
    col()
        .key("application-picker")
        .ml(METRICS.row_px)
        .mr(METRICS.row_px)
        .mb(8.0)
        .padding(8.0)
        .gap(6.0)
        .radius(p.k.radius.md)
        .fill(c.layer_01)
        .child(search)
        .children(status.map(|status| {
            text(status)
                .key("status")
                .font(DESCRIPTION)
                .color(p.muted())
                .px(8.0)
                .py(6.0)
        }))
        .child(results)
        .child(
            row()
                .key("picker-actions")
                .justify_between()
                .child(choose_file_button(p))
                .child(spacer())
                .child(button(
                    p,
                    "close-application-picker",
                    tr("Done"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| {
                        ui.modes.application_picker_open = false;
                        ui.modes.application_search = TextEdit::default();
                    },
                )),
        )
}

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
    let enabled = current.post_processing.enabled;
    let can_toggle = enabled || ui.models.available();
    let toggle = Switch::new(enabled)
        .disabled(!can_toggle)
        .on_toggle(|ui: &mut Ui| {
            let enabled = !mode(&ui.settings, ui.modes.selected)
                .post_processing
                .enabled;
            update_mode(ui, |mode| mode.post_processing.enabled = enabled);
        })
        .build(p.k)
        .key("processing-toggle");
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

/// The mode's transformations, in the order they run; the + menu adds a
/// registered one it does not use yet.
fn transformations(ui: &Ui, p: Paint, current: &DictationMode, layer: &mut Vec<El>) -> El {
    let catalog = &ui.modes.status.transformations;
    let action = (!catalog.is_empty()).then(|| transformation_menu(ui, p, current, layer));
    let mut rows = Vec::new();
    let dragging = ui.modes.drag.as_ref();
    let order = dragging.map_or(&current.transformations, |drag| &drag.order);
    let pitch = TRANSFORMATION_ROW + p.hair;
    for (index, id) in order.iter().enumerate() {
        let name = catalog
            .iter()
            .find(|transformation| &transformation.id == id)
            .map_or_else(|| id.clone(), |transformation| transformation.name.clone());
        let held = dragging.is_some_and(|drag| &drag.id == id);
        let (dragged, remove) = (id.clone(), id.clone());
        let handle = row()
            .key("handle")
            .role("button")
            .label(tr("Reorder"))
            .w(20.0)
            .h_full()
            .center()
            .on_drag(move |ui: &mut Ui, d: &mut Drag, _: &mut Tx| {
                drag_transformation(ui, &dragged, index, pitch, d)
            })
            .child(rune::icon(&GRIP, 16.0).key("grip").color(p.muted()));
        let mut item = row()
            .key(format!("transformation{index}"))
            .h(TRANSFORMATION_ROW)
            .pl(METRICS.row_px - 6.0)
            .pr(METRICS.row_px)
            .gap(6.0)
            .center_y()
            .radius(p.k.radius.md)
            .child(handle)
            .child(
                text(name)
                    .key("name")
                    .font(p.k.ty.ui)
                    .color(p.text())
                    .grow(1.0)
                    .truncate(),
            )
            .child(remove_button(
                p,
                ControlSize::Regular,
                move |ui: &mut Ui| {
                    let id = remove.clone();
                    update_mode(ui, |mode| {
                        mode.transformations.retain(|candidate| *candidate != id)
                    });
                },
            ));
        if held {
            item = item.fill(p.k.c.layer_02);
        }
        rows.push(item);
    }
    if let Some(workspace) = workspace_status(ui, p) {
        rows.push(workspace);
    }
    if rows.is_empty() {
        rows.push(note(
            "no-transformations",
            tr("No transformations in this mode."),
            p.muted(),
        ));
    }
    let title = tr("Transformations");
    match action {
        Some(action) => section_with(p, "transformations", title, action, rows),
        None => section(p, "transformations", title, rows),
    }
}

const TRANSFORMATION_MENU: &str = "transformation-menu-trigger";

/// The + and its menu of the registered transformations the mode does not
/// use, each name with its description below; choosing one appends it.
/// The kit's menus draw one line per item, so this one is drawn here.
fn transformation_menu(ui: &Ui, p: Paint, current: &DictationMode, layer: &mut Vec<El>) -> El {
    let trigger = row().key(TRANSFORMATION_MENU).child(
        IconButton::new(PLUS)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Regular)
            .label(tr("Add transformation"))
            .on_press(|ui: &mut Ui| {
                ui.open = (ui.open != Some(Menu::Transformations)).then_some(Menu::Transformations);
            })
            .build(p.k)
            .key("add-transformation"),
    );
    if ui.open != Some(Menu::Transformations) {
        return trigger.mr(-4.0);
    }
    let c = &p.k.c;
    let close = |ui: &mut Ui| ui.open = None;
    let available = ui
        .modes
        .status
        .transformations
        .iter()
        .filter(|transformation| !current.transformations.contains(&transformation.id));
    let mut options = col().key("options").items_stretch().padding(4.0);
    let mut empty = true;
    for (index, transformation) in available.enumerate() {
        empty = false;
        let id = transformation.id.clone();
        options = options.child(
            col()
                .key(format!("transformation-option{index}"))
                .role("menuitem")
                .label(transformation.name.clone())
                .focusable(true)
                .focus_highlight(c.hover)
                .px(8.0)
                .py(6.0)
                .gap(2.0)
                .radius(4.0)
                .on_press(move |ui: &mut Ui| {
                    ui.open = None;
                    let id = id.clone();
                    update_mode(ui, |mode| mode.transformations.push(id));
                })
                .child(
                    text(transformation.name.clone())
                        .key("name")
                        .font(p.k.ty.ui_compact)
                        .color(c.text_base)
                        .truncate(),
                )
                .children(transformation.description.clone().map(|description| {
                    text(description)
                        .key("description")
                        .font(p.k.ty.small)
                        .color(c.text_weak)
                        .wrap()
                })),
        );
    }
    if empty {
        options = options.child(
            text(tr("Every registered transformation is already included."))
                .key("all-included")
                .font(p.k.ty.small)
                .color(c.text_weak)
                .px(8.0)
                .py(6.0)
                .wrap(),
        );
    }
    let mut menu = col()
        .key("transformation-menu")
        .role("menu")
        .focus_group()
        .focus_return(TRANSFORMATION_MENU)
        .modal(true, close)
        .w(320.0)
        .radius(6.0)
        .fill(c.layer_01)
        .child(
            col()
                .key("viewport")
                .shrink()
                .min_h(0.0)
                .max_h(400.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(options),
        )
        .anchored_fit(TRANSFORMATION_MENU, Side::Bottom, Align::End, 4.0);
    for shadow in &p.k.elevation.floating {
        menu = menu.box_shadow(*shadow);
    }
    layer.push(
        el().key("transformation-menu-scrim")
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .on_press(close),
    );
    layer.push(menu);
    trigger.mr(-4.0)
}

/// The config behind custom transformations: setup, loading, and load
/// failures, which clear when a saved fix reloads. Hidden once it is loaded.
fn workspace_status(ui: &Ui, p: Paint) -> Option<El> {
    if !ui.live() {
        return None;
    }
    let pane = &ui.modes;
    let (description, error, action) = if pane.workspace_receiver.is_some() {
        (tr("Creating config…"), None, None)
    } else if let Some(error) = pane.workspace_error.clone() {
        (
            tr("The config could not be created."),
            Some(error),
            Some(tr("Retry setup")),
        )
    } else if !pane.config_exists {
        (
            tr("Create a config file to define reusable transformations."),
            None,
            Some(tr("Set up")),
        )
    } else {
        match pane.status.host_state {
            HostState::Active if pane.status.last_reload_error.is_none() => return None,
            HostState::Starting => (tr("Loading custom transformations…"), None, None),
            _ => (
                tr("Custom transformations are unavailable."),
                pane.status.last_reload_error.clone(),
                None,
            ),
        }
    };
    let control = action.map_or_else(
        || el().key("none"),
        |label| {
            button(
                p,
                "workspace-action",
                label,
                ButtonKind::Secondary,
                |ui: &mut Ui, _| provision_workspace(ui),
            )
        },
    );
    let mut row = setting_row(
        p,
        "workspace",
        tr("Custom transformations"),
        Some(description),
        control,
    );
    if let Some(error) = error {
        row = col().key("workspace-status").child(row).child(note(
            "workspace-error",
            &error,
            p.danger(),
        ));
    }
    Some(row)
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
    ui.modes.select_in(&ui.settings, Selection::Custom(0));
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
    use super::reorder_transformation;

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
