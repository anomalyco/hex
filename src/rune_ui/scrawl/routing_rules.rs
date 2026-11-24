//! Annotate's Auto-send rules: the Annotate pane's row in `After an annotation`
//! (stacked icons + rule count) and the `Auto-send rules` modal sheet for
//! managing site/subpath, terminal-folder, and app rules plus the conditional
//! `Auto-send with Jev` toggle.

use std::path::Path;
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::Duration;

use rune::{El, TextEdit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, TextInput};
use rune_theme_opencode::icons as oc;
use scrawl_capture::RuleKind;
use scrawl_capture::routing::policy::{self, AutoSend};

use crate::capture_host::routing::rule_match::{display_site, shorten_home, site_is_subpath};
use crate::capture_host::{RouteDest, jev_available, recent_sessions};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::controls::{
    ButtonKind, CardRow, Choice, METRICS, Paint, button, card, folder_label, icon_row, modal_sheet,
    note, section_with, segmented, select, setting_row, toggle_row, value_button, value_font,
};
use crate::rune_ui::{Menu, Modal, Pane, Ui, icons};

const MODAL_WIDTH: f32 = 560.0;
const PADDING: f32 = 18.0;
const ICON: f32 = 20.0;
const STACKED: usize = 4;
const STACK_ICON: f32 = 18.0;
const FIELD_WIDTH: f32 = 240.0;
const RELOAD_EVERY: f64 = 10.0;
const ADD_APP_MENU: &str = "routing-add-app";
const ADD_TARGET_MENU: &str = "routing-add-target";

/// Loaded snapshot from the background OpenCode / Jev probe.
struct ProbeResult {
    jev_available: bool,
    sessions: Vec<RouteDest>,
}

/// Runtime UI state for the `Auto-send rules` modal sheet.
#[derive(Default)]
pub(crate) struct Routing {
    /// Whether Jev has an active credential available; controls whether the
    /// top `Auto-send with Jev` toggle card is shown.
    jev_available: bool,
    /// OpenCode sessions loaded off the UI thread while the sheet is open.
    sessions: Vec<RouteDest>,
    loader: Option<Receiver<ProbeResult>>,
    read_at: Option<f64>,
    /// Whether the sheet currently displays the `New rule` form.
    adding: bool,
    /// 0 = Website, 1 = Terminal folder, 2 = App.
    kind: usize,
    /// Website prefix, terminal folder, or fallback app bundle ID.
    prefix: TextEdit,
    /// Chosen app bundle ID from the installed-applications picker.
    selected_app: Option<String>,
    /// 0 = Specific session, 1 = Project folder.
    to_kind: usize,
    /// Chosen session ID (`ses_…`) when `to_kind == 0`.
    selected_session: Option<String>,
    /// Chosen project folder path when `to_kind == 1`.
    selected_folder: Option<String>,
    /// Manual target input (custom folder path, or session ID when offline).
    custom_target: TextEdit,
    /// Whether the user chose "Other folder…" in the project-folder picker.
    use_custom_folder: bool,
    /// Validation error shown under the `New rule` form.
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuleEntry {
    kind: RuleKind,
    /// Exact map key persisted in `RouterConfig`.
    key: String,
    /// Display string shown on the row.
    display: String,
    /// True for a website rule covering a subpath rather than a whole host.
    subpath: bool,
    /// Persisted target (`ses_…` or project folder).
    target: String,
}

impl RuleEntry {
    fn caption(&self) -> &'static str {
        match self.kind {
            RuleKind::Site if self.subpath => tr("Subpath"),
            RuleKind::Site => tr("Website"),
            RuleKind::Terminal => tr("Terminal folder"),
            RuleKind::App => tr("App"),
        }
    }
}

/// Collects every saved rule from `settings.capture.routing` in stable order:
/// websites (subpaths & hosts), terminal folders, then apps.
fn entries(ui: &Ui) -> Vec<RuleEntry> {
    let config = ui.settings.capture.routing.config();
    let mut out =
        Vec::with_capacity(config.sites.len() + config.terminals.len() + config.apps.len());
    for (key, target) in &config.sites {
        out.push(RuleEntry {
            kind: RuleKind::Site,
            key: key.clone(),
            display: display_site(key),
            subpath: site_is_subpath(key),
            target: target.clone(),
        });
    }
    for (key, target) in &config.terminals {
        out.push(RuleEntry {
            kind: RuleKind::Terminal,
            key: key.clone(),
            display: folder_label(Path::new(key)),
            subpath: false,
            target: target.clone(),
        });
    }
    for (key, target) in &config.apps {
        let display = ui
            .modes
            .app_name(key)
            .map(str::to_owned)
            .unwrap_or_else(|| key.clone());
        out.push(RuleEntry {
            kind: RuleKind::App,
            key: key.clone(),
            display,
            subpath: false,
            target: target.clone(),
        });
    }
    out
}

/// Unique project folders known from loaded OpenCode sessions, as
/// `(full_path_or_tilde, short_display_name)`.
fn project_folders(ui: &Ui) -> Vec<(String, String)> {
    let mut folders: Vec<(String, String)> = Vec::new();
    for dest in &ui.routing.sessions {
        let Some(dir) = &dest.directory else {
            continue;
        };
        let key = shorten_home(dir);
        if folders.iter().any(|(existing, _)| existing == &key) {
            continue;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| key.clone());
        folders.push((key, name));
    }
    folders
}

/// Installed applications that may have a `routing.apps` rule (macOS bundle
/// IDs excluding general-purpose apps).
fn eligible_apps(ui: &Ui) -> Vec<(String, String)> {
    let mut apps: Vec<(String, String)> = ui
        .modes
        .loaded_applications()
        .iter()
        .filter_map(|app| {
            let id = app.bundle_id.as_deref()?.trim();
            if id.is_empty() || policy::is_general_app(id) {
                return None;
            }
            Some((id.to_owned(), app.name.clone()))
        })
        .collect();
    apps.sort_by_key(|a| a.1.to_lowercase());
    apps.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    apps
}

/// Whether Jev auto-send is currently enabled in persisted settings.
fn jev_auto_send_on(ui: &Ui) -> bool {
    let config = ui.settings.capture.routing.config();
    config.auto_send == AutoSend::Confident && config.jev_enabled()
}

/// Toggles `Auto-send with Jev` between `Confident` (+ `jev_routing` +
/// `smart_routing`) and `Rules`.
fn toggle_jev(ui: &mut Ui) {
    let enable = !jev_auto_send_on(ui);
    ui.update(|settings| {
        settings.capture.routing.update(|config| {
            if enable {
                config.auto_send = AutoSend::Confident;
                config.jev_routing = true;
                config.smart_routing = true;
            } else {
                config.auto_send = AutoSend::Rules;
            }
        });
    });
}

/// Opens the `Auto-send rules` modal sheet and kicks off a fresh background
/// probe of Jev availability and OpenCode sessions.
pub(crate) fn open(ui: &mut Ui) {
    ui.open_modal(Modal::RoutingRules);
    ui.routing.adding = false;
    ui.routing.error = None;
    ui.routing.read_at = None;
}

pub(crate) fn close(ui: &mut Ui) {
    ui.close_modal_if(Modal::RoutingRules);
}

pub(crate) fn on_close(ui: &mut Ui) {
    ui.routing.adding = false;
    ui.routing.error = None;
    if is_routing_menu(ui.open) {
        ui.open = None;
    }
}

pub(crate) fn is_routing_menu(menu: Option<Menu>) -> bool {
    matches!(
        menu,
        Some(Menu::RoutingRule(_))
            | Some(Menu::Pick(ADD_APP_MENU))
            | Some(Menu::Pick(ADD_TARGET_MENU))
    )
}

/// Polls Jev availability and OpenCode sessions off the UI thread ONLY while
/// the `Auto-send rules` modal sheet is open. Loads the app catalog when
/// Annotate shows app rules so stacked icons resolve.
pub(crate) fn poll(ui: &mut Ui, clock: f64, visible: bool) -> bool {
    let mut changed = false;
    if let Some(loader) = &ui.routing.loader
        && let Ok(result) = loader.try_recv()
    {
        ui.routing.loader = None;
        if ui.routing.jev_available != result.jev_available
            || ui.routing.sessions != result.sessions
        {
            ui.routing.jev_available = result.jev_available;
            ui.routing.sessions = result.sessions;
            changed = true;
        }
    }
    if !(visible && ui.live()) {
        return changed;
    }
    let modal_open = ui.is_modal(Modal::RoutingRules);
    let annotate_has_apps = ui.pane == Pane::Annotate
        && ui.settings.capture.enabled
        && !ui.settings.capture.routing.config().apps.is_empty();
    if modal_open || annotate_has_apps {
        ui.modes.ensure_applications();
    }
    if !modal_open {
        return changed;
    }
    if ui.routing.loader.is_none()
        && ui
            .routing
            .read_at
            .is_none_or(|at| clock - at >= RELOAD_EVERY)
    {
        ui.routing.read_at = Some(clock);
        let (sender, receiver) = sync_channel(1);
        std::thread::spawn(move || {
            let jev = jev_available(Duration::from_millis(500));
            let sessions = recent_sessions(Duration::from_secs(2)).unwrap_or_default();
            let _ = sender.send(ProbeResult {
                jev_available: jev,
                sessions,
            });
        });
        ui.routing.loader = Some(receiver);
    }
    changed
}

/// Populates fixture state with generic stand-ins for `mac-routing-rules`
/// and `mac-routing-add` scenes.
pub(crate) fn fixture(ui: &mut Ui, open_sheet: bool, adding: bool) {
    crate::rune_ui::modes::fixture_call_catalog(ui);
    ui.settings.capture.routing.update(|config| {
        config.auto_send = AutoSend::Confident;
        config.jev_routing = true;
        config.smart_routing = true;
        config.sites.clear();
        config
            .sites
            .insert("demo.example.com/docs/".into(), "ses_acme".into());
        config
            .sites
            .insert("status.example.com".into(), "ses_seer".into());
        config.terminals.clear();
        config
            .terminals
            .insert("~/code/acme-web".into(), "~/code/acme-web".into());
        config.apps.clear();
        config
            .apps
            .insert("com.tinyspeck.slackmacgap".into(), "ses_home".into());
    });
    ui.routing = Routing {
        jev_available: true,
        sessions: vec![
            RouteDest {
                session: "ses_acme".into(),
                name: "ACME".into(),
                why: "2m ago".into(),
                directory: Some("/Users/you/code/acme-web".into()),
            },
            RouteDest {
                session: "ses_seer".into(),
                name: "SEER".into(),
                why: "12m ago".into(),
                directory: Some("/Users/you/code/acme-api".into()),
            },
            RouteDest {
                session: "ses_home".into(),
                name: "HOME".into(),
                why: "1h ago".into(),
                directory: Some("/Users/you/code/home".into()),
            },
        ],
        adding,
        kind: 0,
        prefix: TextEdit::new("demo.example.com/docs/".to_owned()),
        selected_session: Some("ses_acme".into()),
        selected_folder: Some("~/code/acme-web".into()),
        ..Routing::default()
    };
    if open_sheet {
        self::open(ui);
        ui.routing.adding = adding;
        ui.routing.prefix = TextEdit::new("demo.example.com/docs/".to_owned());
        ui.routing.selected_session = Some("ses_acme".into());
    }
}

/// A rule's icon: its app's icon when available, else its kind's glyph.
fn rule_icon(ui: &Ui, p: Paint, entry: &RuleEntry, size: f32) -> El {
    if entry.kind == RuleKind::App
        && let Some(pixels) = ui.modes.app_icon(&entry.key)
    {
        return crate::rune_ui::modes::application_icon("icon", &pixels, size).a11y_hidden();
    }
    let glyph = match entry.kind {
        RuleKind::Site => &icons::GLOBE,
        RuleKind::Terminal => &oc::CONSOLE,
        RuleKind::App => &oc::MONITOR,
    };
    el().key("icon")
        .size(size, size)
        .center()
        .child(rune::icon(glyph, size - 4.0).key("glyph").color(p.muted()))
        .a11y_hidden()
}

/// A stacked icon for the `After an annotation` settings row.
fn stack_icon(ui: &Ui, p: Paint, entry: &RuleEntry) -> El {
    if entry.kind == RuleKind::App && ui.modes.app_icon(&entry.key).is_some() {
        return rule_icon(ui, p, entry, STACK_ICON);
    }
    let glyph = match entry.kind {
        RuleKind::Site => &icons::GLOBE,
        RuleKind::Terminal => &oc::CONSOLE,
        RuleKind::App => &oc::MONITOR,
    };
    el().key("tile")
        .size(STACK_ICON, STACK_ICON)
        .radius(STACK_ICON / 2.0)
        .fill(p.k.c.layer_02)
        .center()
        .child(rune::icon(glyph, 12.0).key("glyph").color(p.muted()))
        .a11y_hidden()
}

/// The Annotate pane's row in `After an annotation`: stacked icons of active
/// rule types + count (`N rules ›` or `No rules ›`), opening `Modal::RoutingRules`.
pub(crate) fn settings_row(ui: &Ui, p: Paint) -> El {
    let rules = entries(ui);
    let c = &p.k.c;
    let ring = STACK_ICON + 3.0;
    // Deduplicate non-app kinds so multiple websites stack as one globe and
    // multiple terminals stack as one console icon, while distinct apps each
    // keep their own icon.
    let mut shown: Vec<&RuleEntry> = Vec::new();
    for entry in &rules {
        let same = shown.iter().any(|seen| match (seen.kind, entry.kind) {
            (RuleKind::Site, RuleKind::Site) => true,
            (RuleKind::Terminal, RuleKind::Terminal) => true,
            (RuleKind::App, RuleKind::App) => seen.key == entry.key,
            _ => false,
        });
        if !same {
            shown.push(entry);
        }
    }
    let stack = row()
        .key("stack")
        .center_y()
        .a11y_hidden()
        .children(
            shown
                .iter()
                .take(STACKED)
                .enumerate()
                .map(|(index, entry)| {
                    el().key(format!("rule{index}"))
                        .size(ring, ring)
                        .ml(if index == 0 { 0.0 } else { -6.0 })
                        .radius(ring / 2.0)
                        .fill(c.bg_base)
                        .center()
                        .child(stack_icon(ui, p, entry))
                }),
        )
        .children((shown.len() > STACKED).then(|| {
            text(format!("+{}", shown.len() - STACKED))
                .key("more")
                .font(p.k.ty.small)
                .color(p.muted())
                .ml(4.0)
        }));
    let count = match rules.len() {
        0 => tr("No rules").to_owned(),
        1 => tr("1 rule").to_owned(),
        n => tr_fill("{} rules", &n.to_string()),
    };
    let description = if jev_auto_send_on(ui) {
        tr("Saved rules and Jev")
    } else {
        tr("Send matching annotations automatically")
    };
    let control = row()
        .key("routing-controls")
        .gap(6.0)
        .center_y()
        .child(stack)
        .child(value_button(
            p,
            "routing-control",
            tr("Auto-send rules"),
            &count,
            &oc::CHEVRON_RIGHT,
            open,
        ));
    icon_row(
        p,
        "auto-send",
        &icons::ZAP,
        tr("Auto-send rules"),
        Some(description),
        control,
    )
    .on_press(open)
}

/// Resolves a rule target (`ses_…` or project folder) to its glyph and label.
fn target_display(ui: &Ui, target: &str) -> (&'static rune::Icon, String) {
    if target.starts_with("ses_") {
        let label = ui
            .routing
            .sessions
            .iter()
            .find(|dest| dest.session == target)
            .map(|dest| dest.name.clone())
            .unwrap_or_else(|| tr("Session").to_owned());
        (&oc::SPEECH_BUBBLE, label)
    } else {
        let name = Path::new(target)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| folder_label(Path::new(target)));
        (&oc::FOLDER, name)
    }
}

/// Updates an existing rule's target in persisted `capture.routing`.
fn update_rule_target(ui: &mut Ui, kind: RuleKind, key: String, target: String) {
    ui.open = None;
    ui.update(|settings| {
        settings.capture.routing.update(|config| {
            let map = match kind {
                RuleKind::Site => &mut config.sites,
                RuleKind::Terminal => &mut config.terminals,
                RuleKind::App => &mut config.apps,
            };
            map.insert(key, target);
        });
    });
}

/// Deletes a rule from persisted `capture.routing`.
fn remove_rule(ui: &mut Ui, kind: RuleKind, key: String) {
    ui.open = None;
    ui.update(|settings| {
        settings.capture.routing.update(|config| {
            let map = match kind {
                RuleKind::Site => &mut config.sites,
                RuleKind::Terminal => &mut config.terminals,
                RuleKind::App => &mut config.apps,
            };
            map.remove(&key);
        });
    });
}

/// Builds the destination menu choices for an existing rule row: loaded
/// OpenCode sessions, then known project folders, keeping the current target
/// present even if OpenCode is offline.
fn destination_choices(ui: &Ui, entry: &RuleEntry) -> Vec<Choice> {
    let mut choices = Vec::new();
    let mut seen_current = false;
    for dest in &ui.routing.sessions {
        let selected = dest.session == entry.target;
        seen_current |= selected;
        let kind = entry.kind;
        let key = entry.key.clone();
        let target = dest.session.clone();
        let detail = dest
            .directory
            .as_deref()
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned());
        choices.push(Choice {
            label: dest.name.clone(),
            detail,
            selected,
            choose: Box::new(move |ui: &mut Ui| {
                update_rule_target(ui, kind, key.clone(), target.clone());
            }),
        });
    }
    for (folder, name) in project_folders(ui) {
        let selected = folder == entry.target;
        seen_current |= selected;
        let kind = entry.kind;
        let key = entry.key.clone();
        let target = folder.clone();
        choices.push(Choice {
            label: name,
            detail: Some(tr("Project folder").to_owned()),
            selected,
            choose: Box::new(move |ui: &mut Ui| {
                update_rule_target(ui, kind, key.clone(), target.clone());
            }),
        });
    }
    if !seen_current {
        let (_, label) = target_display(ui, &entry.target);
        let detail = if entry.target.starts_with("ses_") {
            Some(tr("Specific session").to_owned())
        } else {
            Some(tr("Project folder").to_owned())
        };
        choices.insert(
            0,
            Choice {
                label,
                detail,
                selected: true,
                choose: Box::new(|ui: &mut Ui| ui.open = None),
            },
        );
    }
    choices
}

/// One saved rule's row: icon, match pattern + kind caption, destination
/// picker button, and `✕` remove button.
fn rule_row(ui: &Ui, p: Paint, index: usize, entry: &RuleEntry, menus: &mut Vec<El>) -> El {
    let c = &p.k.c;
    let menu = Menu::RoutingRule(index);
    let open_menu = ui.open == Some(menu);
    let (glyph, dest_name) = target_display(ui, &entry.target);
    let anchor_key = format!("rule{index}-to");
    let trigger = row()
        .key(anchor_key.clone())
        .role("button")
        .label(tr("Send to"))
        .a11y_value(dest_name.clone())
        .focusable(true)
        .h(26.0)
        .pl(8.0)
        .pr(5.0)
        .gap(6.0)
        .center_y()
        .radius(p.k.radius.sm)
        .fill(c.layer_02)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(move |ui: &mut Ui| crate::rune_ui::settings::toggle_menu(ui, menu))
        .child(rune::icon(glyph, 14.0).key("kind").color(c.icon_muted))
        .child(
            text(dest_name)
                .key("name")
                .font(value_font(p))
                .color(c.text_base),
        )
        .child(
            rune::icon(&oc::CHEVRON_DOWN, 14.0)
                .key("chevron")
                .color(c.icon_muted),
        );
    let mut menu_builder = rune_kit::Menu::new(&anchor_key)
        .trigger(trigger)
        .placement(rune_kit::Placement::BottomEnd)
        .gutter(4.0)
        .min_width(210.0)
        .open(open_menu)
        .on_close(|ui: &mut Ui| ui.open = None);
    for choice in destination_choices(ui, entry) {
        let choose = choice.choose;
        let mut item = rune_kit::MenuItem::new(choice.label).checked(choice.selected);
        if let Some(detail) = choice.detail {
            item = item.description(detail);
        }
        menu_builder = menu_builder.item(item.on_select(move |ui: &mut Ui| choose(ui)));
    }
    let picker = menu_builder.build(p.k, menus);
    let rm_kind = entry.kind;
    let rm_key = entry.key.clone();
    row()
        .key(format!("rule{index}"))
        .role("group")
        .label(entry.display.clone())
        .min_h(52.0)
        .pl(METRICS.row_px)
        .pr(8.0)
        .gap(12.0)
        .center_y()
        .child(rule_icon(ui, p, entry, ICON))
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .gap(1.0)
                .child(
                    text(entry.display.clone())
                        .key("match")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .truncate(),
                )
                .child(
                    text(entry.caption())
                        .key("kind")
                        .font(p.k.ty.small)
                        .color(p.muted()),
                ),
        )
        .child(picker)
        .child(
            IconButton::new(&oc::CLOSE_SMALL)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label(tr("Remove rule"))
                .on_press(move |ui: &mut Ui| remove_rule(ui, rm_kind, rm_key.clone()))
                .build(p.k)
                .key("remove"),
        )
}

fn titled(key: &str, p: Paint, title: &str, body: El) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(p.section_label(title).mx(METRICS.row_px))
        .child(body)
}

/// Prepares the `New rule` form with sensible defaults from loaded sessions
/// and apps.
fn begin_add(ui: &mut Ui) {
    ui.open = None;
    ui.routing.adding = true;
    ui.routing.kind = 0;
    ui.routing.prefix = TextEdit::default();
    ui.routing.selected_app = eligible_apps(ui).first().map(|(id, _)| id.clone());
    ui.routing.to_kind = if ui.routing.sessions.is_empty() { 1 } else { 0 };
    ui.routing.selected_session = ui.routing.sessions.first().map(|d| d.session.clone());
    let folders = project_folders(ui);
    ui.routing.selected_folder = folders.first().map(|(f, _)| f.clone());
    ui.routing.use_custom_folder = folders.is_empty();
    ui.routing.custom_target = TextEdit::default();
    ui.routing.error = None;
}

/// Validates and persists the `New rule` form into `settings.capture.routing`.
fn save_new_rule(ui: &mut Ui) {
    let kind = match ui.routing.kind {
        0 => RuleKind::Site,
        1 => RuleKind::Terminal,
        _ => RuleKind::App,
    };
    let key = match kind {
        RuleKind::App => ui
            .routing
            .selected_app
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| ui.routing.prefix.text.trim().to_owned()),
        RuleKind::Site | RuleKind::Terminal => ui.routing.prefix.text.trim().to_owned(),
    };
    let target = if ui.routing.to_kind == 0 {
        ui.routing
            .selected_session
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| ui.routing.custom_target.text.trim().to_owned())
    } else if ui.routing.use_custom_folder {
        ui.routing.custom_target.text.trim().to_owned()
    } else {
        ui.routing
            .selected_folder
            .clone()
            .unwrap_or_else(|| ui.routing.custom_target.text.trim().to_owned())
    };
    let current = ui.settings.capture.routing.config().clone();
    match current.with_rule(kind, &key, &target) {
        Ok(next) => {
            ui.open = None;
            ui.routing.error = None;
            ui.routing.adding = false;
            ui.update(|settings| {
                settings.capture.routing = next.into();
            });
        }
        Err(error) => {
            ui.routing.error = Some(error.to_string());
        }
    }
}

fn rules_view(ui: &Ui, p: Paint, menus: &mut Vec<El>) -> Vec<El> {
    let mut sections = Vec::new();
    // Show the Jev auto-send toggle ONLY when Jev is available.
    if ui.routing.jev_available {
        sections.push(card(
            p,
            "jev-card",
            vec![toggle_row(
                p,
                "auto-send-jev",
                tr("Auto-send with Jev"),
                tr("When no rule matches, send if Jev is sure; every send waits 3 seconds with Undo"),
                jev_auto_send_on(ui),
                toggle_jev,
            )],
        ));
    }
    let add = Button::new(tr("Add rule"))
        .icon(&oc::PLUS_SMALL)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(begin_add)
        .build(p.k)
        .key("add-rule")
        .mr(-8.0);
    let saved = entries(ui);
    let rows: Vec<CardRow> = if saved.is_empty() {
        vec![
            note(
                "rules-empty",
                tr("Choose Always… after a send, or add a rule above."),
                p.muted(),
            )
            .into(),
        ]
    } else {
        saved
            .iter()
            .enumerate()
            .map(|(index, entry)| rule_row(ui, p, index, entry, menus).into())
            .collect()
    };
    sections.push(section_with(p, "rules", tr("Rules"), add, rows));
    if !ui.routing.jev_available {
        sections.push(
            note(
                "undo-hold",
                tr("Each send waits 3 seconds first, so you can Undo it."),
                p.muted(),
            )
            .mt(-4.0),
        );
    }
    sections
}

fn editor_view(ui: &Ui, p: Paint, menus: &mut Vec<El>) -> Vec<El> {
    let routing = &ui.routing;
    let kind_labels = [tr("Website"), tr("Terminal folder"), tr("App")];
    let apps = eligible_apps(ui);
    let match_row = match routing.kind {
        0 => setting_row(
            p,
            "rule-match",
            tr("Address"),
            Some(tr("This page and pages under it, or a whole website")),
            TextInput::new(&routing.prefix)
                .placeholder("example.com/path/")
                .width(Some(FIELD_WIDTH))
                .on_edit(|ui: &mut Ui| &mut ui.routing.prefix)
                .build(p.k)
                .key("rule-prefix"),
        ),
        1 => setting_row(
            p,
            "rule-match",
            tr("Folder"),
            Some(tr("A terminal working in this folder or below it")),
            TextInput::new(&routing.prefix)
                .placeholder("~/code/project")
                .width(Some(FIELD_WIDTH))
                .on_edit(|ui: &mut Ui| &mut ui.routing.prefix)
                .build(p.k)
                .key("rule-prefix"),
        ),
        _ => {
            let control = if !apps.is_empty() {
                let menu = Menu::Pick(ADD_APP_MENU);
                let current_label = routing
                    .selected_app
                    .as_deref()
                    .and_then(|id| apps.iter().find(|(a, _)| a == id))
                    .map(|(_, name)| name.as_str())
                    .unwrap_or_else(|| tr("Choose an app"));
                let choices = apps
                    .iter()
                    .map(|(id, name)| {
                        let id_owned = id.clone();
                        Choice {
                            label: name.clone(),
                            detail: None,
                            selected: routing.selected_app.as_deref() == Some(id.as_str()),
                            choose: Box::new(move |ui: &mut Ui| {
                                ui.routing.selected_app = Some(id_owned.clone());
                                ui.open = None;
                            }),
                        }
                    })
                    .collect();
                select(
                    p,
                    "rule-app-picker",
                    tr("App"),
                    current_label,
                    ui.open == Some(menu),
                    choices,
                    move |ui: &mut Ui| crate::rune_ui::settings::toggle_menu(ui, menu),
                    menus,
                )
            } else {
                TextInput::new(&routing.prefix)
                    .placeholder("com.example.app")
                    .width(Some(FIELD_WIDTH))
                    .on_edit(|ui: &mut Ui| &mut ui.routing.prefix)
                    .build(p.k)
                    .key("rule-prefix")
            };
            setting_row(
                p,
                "rule-match",
                tr("App"),
                Some(tr("Any window of this app")),
                control,
            )
        }
    };
    let matching = vec![
        setting_row(
            p,
            "rule-kind",
            tr("Kind"),
            None,
            segmented(
                p,
                "rule-kind-control",
                &kind_labels,
                routing.kind,
                |ui, index| {
                    ui.routing.kind = index;
                    ui.routing.error = None;
                    ui.open = None;
                    if index == 2 && ui.routing.selected_app.is_none() {
                        ui.routing.selected_app =
                            eligible_apps(ui).first().map(|(id, _)| id.clone());
                    }
                },
            ),
        ),
        match_row,
    ];
    let target_menu = Menu::Pick(ADD_TARGET_MENU);
    let folders = project_folders(ui);
    let (target_title, target_hint, target_control) = if routing.to_kind == 0 {
        let control = if !routing.sessions.is_empty() {
            let current_label = routing
                .selected_session
                .as_deref()
                .and_then(|id| routing.sessions.iter().find(|d| d.session == id))
                .map(|d| d.name.as_str())
                .unwrap_or_else(|| tr("Choose a session"));
            let choices = routing
                .sessions
                .iter()
                .map(|dest| {
                    let id = dest.session.clone();
                    let detail = dest
                        .directory
                        .as_deref()
                        .and_then(|d| d.file_name())
                        .map(|n| n.to_string_lossy().into_owned());
                    Choice {
                        label: dest.name.clone(),
                        detail,
                        selected: routing.selected_session.as_deref() == Some(id.as_str()),
                        choose: Box::new(move |ui: &mut Ui| {
                            ui.routing.selected_session = Some(id.clone());
                            ui.open = None;
                        }),
                    }
                })
                .collect();
            select(
                p,
                "rule-target",
                tr("Destination"),
                current_label,
                ui.open == Some(target_menu),
                choices,
                move |ui: &mut Ui| crate::rune_ui::settings::toggle_menu(ui, target_menu),
                menus,
            )
        } else {
            TextInput::new(&routing.custom_target)
                .placeholder("ses_…")
                .width(Some(FIELD_WIDTH))
                .on_edit(|ui: &mut Ui| &mut ui.routing.custom_target)
                .build(p.k)
                .key("rule-target-input")
        };
        (
            tr("Session"),
            tr("Even after newer sessions start"),
            control,
        )
    } else {
        let control = if !folders.is_empty() && !routing.use_custom_folder {
            let current_label = routing
                .selected_folder
                .as_deref()
                .and_then(|f| folders.iter().find(|(path, _)| path == f))
                .map(|(_, name)| name.as_str())
                .unwrap_or_else(|| tr("Choose a project"));
            let mut choices: Vec<Choice> = folders
                .iter()
                .map(|(path, name)| {
                    let path_owned = path.clone();
                    Choice {
                        label: name.clone(),
                        detail: Some(path.clone()),
                        selected: routing.selected_folder.as_deref() == Some(path.as_str()),
                        choose: Box::new(move |ui: &mut Ui| {
                            ui.routing.selected_folder = Some(path_owned.clone());
                            ui.routing.use_custom_folder = false;
                            ui.open = None;
                        }),
                    }
                })
                .collect();
            choices.push(Choice {
                label: tr("Other folder…").to_owned(),
                detail: None,
                selected: false,
                choose: Box::new(|ui: &mut Ui| {
                    ui.routing.use_custom_folder = true;
                    ui.open = None;
                }),
            });
            select(
                p,
                "rule-target",
                tr("Destination"),
                current_label,
                ui.open == Some(target_menu),
                choices,
                move |ui: &mut Ui| crate::rune_ui::settings::toggle_menu(ui, target_menu),
                menus,
            )
        } else {
            TextInput::new(&routing.custom_target)
                .placeholder("~/code/project")
                .width(Some(FIELD_WIDTH))
                .on_edit(|ui: &mut Ui| &mut ui.routing.custom_target)
                .build(p.k)
                .key("rule-target-input")
        };
        (tr("Project"), tr("Its newest session"), control)
    };
    let sending = vec![
        setting_row(
            p,
            "rule-to-kind",
            tr("Send to"),
            None,
            segmented(
                p,
                "rule-to-kind-control",
                &[tr("Specific session"), tr("Project folder")],
                routing.to_kind,
                |ui, index| {
                    ui.routing.to_kind = index;
                    ui.routing.use_custom_folder = index == 1 && project_folders(ui).is_empty();
                    ui.routing.error = None;
                    ui.open = None;
                },
            ),
        ),
        setting_row(
            p,
            "rule-target-row",
            target_title,
            Some(target_hint),
            target_control,
        ),
    ];
    let mut body = vec![
        titled("match", p, tr("Match"), card(p, "card", matching)),
        titled(
            "destination",
            p,
            tr("Destination"),
            card(p, "card", sending),
        ),
    ];
    if let Some(error) = &routing.error {
        body.push(note("rule-error", error, p.danger()).mt(-4.0));
    }
    body
}

/// The sheet's scrim and panel via `modal_sheet`, plus any open dropdown menu.
pub(crate) fn modal(ui: &Ui, p: Paint) -> Vec<El> {
    let c = &p.k.c;
    let mut menus = Vec::new();
    let adding = ui.routing.adding;
    let (title, body) = if adding {
        (tr("New rule"), editor_view(ui, p, &mut menus))
    } else {
        (tr("Auto-send rules"), rules_view(ui, p, &mut menus))
    };
    let footer = if adding {
        row()
            .key("footer")
            .justify_end()
            .gap(8.0)
            .child(button(
                p,
                "cancel-rule",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    ui.routing.adding = false;
                    ui.routing.error = None;
                    ui.open = None;
                },
            ))
            .child(
                button(
                    p,
                    "save-rule",
                    tr("Save rule"),
                    ButtonKind::Primary,
                    |ui: &mut Ui, _| save_new_rule(ui),
                )
                .default_button(),
            )
    } else {
        row().key("footer").justify_end().child(
            button(
                p,
                "close-routing",
                tr("Done"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| close(ui),
            )
            .default_button(),
        )
    };
    let panel = col()
        .key("routing-sheet")
        .role("dialog")
        .label(title)
        .w(MODAL_WIDTH)
        .pt(PADDING)
        .px(PADDING)
        .pb(14.0)
        .gap(16.0)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        .on_press(|_: &mut Ui| {})
        .child(
            text(title)
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .children(body)
        .child(footer);
    let mut layers = Vec::from(modal_sheet(p, "routing", panel, close));
    layers.extend(menus);
    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rune_ui::scenes::mac_fixture;

    #[test]
    fn jev_toggle_is_conditional_and_switches_between_confident_and_rules() {
        let mut ui = mac_fixture();
        ui.routing.jev_available = false;
        let p = Paint::new(true, 2.0);
        let mut menus = Vec::new();
        let sections = rules_view(&ui, p, &mut menus);
        assert_eq!(sections.len(), 2, "rules + undo hold note without Jev card");

        ui.routing.jev_available = true;
        let sections = rules_view(&ui, p, &mut menus);
        assert_eq!(sections.len(), 2, "Jev card + rules without redundant note");

        assert!(!jev_auto_send_on(&ui));
        toggle_jev(&mut ui);
        let cfg = ui.settings.capture.routing.config();
        assert_eq!(cfg.auto_send, AutoSend::Confident);
        assert!(cfg.jev_routing && cfg.smart_routing);

        toggle_jev(&mut ui);
        assert_eq!(
            ui.settings.capture.routing.config().auto_send,
            AutoSend::Rules
        );
        assert!(!jev_auto_send_on(&ui));
    }

    #[test]
    fn adding_updating_and_removing_site_terminal_and_app_rules_persists() {
        let mut ui = mac_fixture();
        // Add a website subpath rule.
        begin_add(&mut ui);
        ui.routing.kind = 0;
        ui.routing.prefix = TextEdit::new("demo.example.com/docs/");
        ui.routing.to_kind = 0;
        ui.routing.selected_session = Some("ses_acme".into());
        save_new_rule(&mut ui);
        assert!(!ui.routing.adding);
        assert_eq!(
            ui.settings
                .capture
                .routing
                .config()
                .sites
                .get("demo.example.com/docs/")
                .map(String::as_str),
            Some("ses_acme")
        );

        // Add a terminal folder rule targeting a project folder.
        begin_add(&mut ui);
        ui.routing.kind = 1;
        ui.routing.prefix = TextEdit::new("~/code/acme-web");
        ui.routing.to_kind = 1;
        ui.routing.use_custom_folder = true;
        ui.routing.custom_target = TextEdit::new("~/code/acme-web");
        save_new_rule(&mut ui);
        assert_eq!(
            ui.settings
                .capture
                .routing
                .config()
                .terminals
                .get("~/code/acme-web")
                .map(String::as_str),
            Some("~/code/acme-web")
        );

        // General-purpose app is rejected with a validation error.
        begin_add(&mut ui);
        ui.routing.kind = 2;
        ui.routing.selected_app = Some("com.apple.Safari".into());
        ui.routing.to_kind = 0;
        ui.routing.selected_session = Some("ses_acme".into());
        save_new_rule(&mut ui);
        assert!(ui.routing.adding);
        assert!(ui.routing.error.is_some());

        // Non-general app succeeds.
        ui.routing.selected_app = Some("anomaly.hex".into());
        save_new_rule(&mut ui);
        assert!(!ui.routing.adding);
        assert_eq!(
            ui.settings
                .capture
                .routing
                .config()
                .apps
                .get("anomaly.hex")
                .map(String::as_str),
            Some("ses_acme")
        );

        // Update destination and then remove rules.
        update_rule_target(
            &mut ui,
            RuleKind::Site,
            "demo.example.com/docs/".into(),
            "ses_seer".into(),
        );
        assert_eq!(
            ui.settings
                .capture
                .routing
                .config()
                .sites
                .get("demo.example.com/docs/")
                .map(String::as_str),
            Some("ses_seer")
        );
        remove_rule(&mut ui, RuleKind::Site, "demo.example.com/docs/".into());
        assert!(
            !ui.settings
                .capture
                .routing
                .config()
                .sites
                .contains_key("demo.example.com/docs/")
        );
        remove_rule(&mut ui, RuleKind::Terminal, "~/code/acme-web".into());
        assert!(ui.settings.capture.routing.config().terminals.is_empty());
        remove_rule(&mut ui, RuleKind::App, "anomaly.hex".into());
        assert!(ui.settings.capture.routing.config().apps.is_empty());
    }
}
