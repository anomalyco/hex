//! The Commands pane (macOS): the voice command catalog grouped by task,
//! search, a detail with the phrase's task, context, and aliases, the
//! personal `hex.config.ts` workspace, and the commands opt-in card with its
//! microphone confirmation and the command model's notice. The catalog and workspace come from the host as
//! [`PlatformState`](super::platform::PlatformState) data.

use rune::{El, TextEdit, Tx, col, row, text};
use rune_kit::TextInput;

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, Paint, button, card, setting_row, toggle_row};
use super::list_detail::{self as ld, Item};
use super::lists::error_panel;
use super::platform::{CatalogCommand, CommandModel, PlatformRequest, Workspace};
use crate::commands::CommandScope;
use crate::desktop_i18n::tr;

#[derive(Default)]
pub(super) struct CommandsPane {
    pub(super) search: TextEdit,
    pub(super) selected: Option<String>,
    /// Commands are off and the microphone is released: enabling them asks
    /// to keep the microphone ready first.
    pub(super) confirm_enable: bool,
    prompt_copied: bool,
}

fn matches(command: &CatalogCommand, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let scope = match &command.scope {
        CommandScope::Sleeping | CommandScope::Global => "global",
        CommandScope::Application(name) => name,
        CommandScope::Browser(host) => host,
    };
    command.phrase.to_lowercase().contains(query)
        || command.description.to_lowercase().contains(query)
        || command.task.to_lowercase().contains(query)
        || scope.to_lowercase().contains(query)
        || command
            .aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(query))
}

fn scope_label(scope: &CommandScope) -> String {
    match scope {
        CommandScope::Sleeping | CommandScope::Global => tr("Global").into(),
        CommandScope::Application(application) => application.clone(),
        CommandScope::Browser(host) => host.clone(),
    }
}

/// The task heading a command is listed under.
fn task_label(task: &str) -> &str {
    match task {
        "Voice control" => tr("Voice control"),
        "Dictation" => tr("Dictation"),
        "Meetings" => tr("Meetings"),
        "Open applications" => tr("Open applications"),
        "Open websites" => tr("Open websites"),
        "Browser navigation" => tr("Browser navigation"),
        "Keyboard" => tr("Keyboard"),
        "Application navigation" => tr("Application navigation"),
        "Other" => tr("Other"),
        other => other,
    }
}

pub(super) fn set_enabled(ui: &mut Ui, enabled: bool) {
    if enabled && ui.settings.release_microphone_while_idle {
        ui.commands.confirm_enable = true;
        return;
    }
    ui.commands.confirm_enable = false;
    let mut candidate = ui.drafted();
    match candidate.set_commands_enabled(enabled) {
        Ok(()) => {
            ui.commit(candidate);
        }
        Err(error) => ui.error = Some(format!("{error:#}")),
    }
}

fn keep_ready_and_enable(ui: &mut Ui) {
    let mut candidate = ui.drafted();
    candidate.keep_microphone_ready_and_enable_commands();
    if ui.commit(candidate) {
        ui.commands.confirm_enable = false;
    }
}

fn agent_prompt(workspace: &str) -> String {
    format!(
        "Help me configure my Hex custom voice commands.\n\n\
         Workspace: {workspace}\n\
         Config: {workspace}/hex.config.ts\n\n\
         Read AGENTS.md in the workspace before editing. Ask me what commands or \
         dictation phrases I want, update hex.config.ts, and run `hex commands check` \
         when finished. Hex runs the config itself: no npm packages, filesystem, or \
         network."
    )
}

/// The header actions (workspace, prompt), the opt-in card and notices, and
/// the list + detail body.
pub(super) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let (visible, selected) = visible(ui);
    let mut list = col().key("commands-list");
    for (group, task) in tasks(&visible).iter().enumerate() {
        list = list.child(ld::group(
            p,
            &format!("task{group}"),
            task_label(task),
            group == 0,
        ));
        for command in visible.iter().filter(|command| command.task == *task) {
            let id = command.id.clone();
            list = list.child(ld::item(
                p,
                Item {
                    key: format!("command-{}", command.id),
                    title: command.phrase.clone(),
                    meta: command.description.clone(),
                    meta_ink: None,
                    time: scope_label(&command.scope),
                    hover: Vec::new(),
                    selected: selected == Some(command.id.as_str()),
                },
                move |ui: &mut Ui| ui.commands.selected = Some(id.clone()),
            ));
        }
    }
    let column = ld::list_column(search(ui, p), "commands-scroll", list);
    let command = selected.and_then(|id| ui.platform.commands.iter().find(|c| c.id == id));
    let body = ld::body(p, "commands-body", column, detail(ui, p, command));
    (
        header_actions(ui, p),
        col()
            .key("commands-pane")
            .grow(1.0)
            .shrink()
            .child(ld::notices(notices(ui, p)))
            .child(body),
    )
}

fn search(ui: &Ui, p: Paint) -> El {
    TextInput::new(&ui.commands.search)
        .placeholder(tr("Search commands"))
        .width(None)
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.commands.search)
        .build(p.k)
        .key("commands-search")
}

/// The tasks of `visible`, in catalog order.
fn tasks<'a>(visible: &[&'a CatalogCommand]) -> Vec<&'a str> {
    let mut tasks: Vec<&str> = Vec::new();
    for command in visible {
        if !tasks.contains(&command.task.as_str()) {
            tasks.push(&command.task);
        }
    }
    tasks
}

/// The commands the search matches, and the selected one's id (the first
/// when none is).
fn visible(ui: &Ui) -> (Vec<&CatalogCommand>, Option<&str>) {
    let query = ui.commands.search.text.trim().to_lowercase();
    let visible: Vec<&CatalogCommand> = ui
        .platform
        .commands
        .iter()
        .filter(|command| matches(command, &query))
        .collect();
    let selected = ui
        .commands
        .selected
        .as_deref()
        .filter(|id| visible.iter().any(|command| command.id == *id))
        .or_else(|| visible.first().map(|command| command.id.as_str()));
    (visible, selected)
}

/// Above the list: the opt-in card, then any workspace or model notice.
fn notices(ui: &Ui, p: Paint) -> Vec<El> {
    let state = &ui.platform;
    let mut top = vec![enable_card(ui, p)];
    if let Some(error) = state
        .workspace_error
        .as_deref()
        .filter(|_| state.workspace == Workspace::Error)
    {
        top.push(error_panel(
            p,
            "commands-error",
            tr("Personal commands are not loading."),
            error,
        ));
    }
    if let Some((title, description)) = model_notice(ui) {
        top.push(model_notice_panel(ui, p, title, description));
    }
    top
}

/// The workspace and agent prompt actions.
fn header_actions(ui: &Ui, p: Paint) -> Vec<El> {
    let state = &ui.platform;
    let mut actions = Vec::new();
    actions.push(match state.workspace {
        Workspace::Missing => button(
            p,
            "commands-create-config",
            tr("Create config"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.ask(PlatformRequest::CreateCommandsConfig),
        ),
        Workspace::Creating => text(tr("Creating config…"))
            .key("commands-creating")
            .font(DESCRIPTION)
            .color(p.muted()),
        Workspace::Ready | Workspace::Error => button(
            p,
            "commands-open-config",
            tr("Edit config"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.ask(PlatformRequest::OpenCommandsConfig),
        ),
    });
    if matches!(state.workspace, Workspace::Ready | Workspace::Error) {
        actions.push(button(
            p,
            "commands-copy-prompt",
            if ui.commands.prompt_copied {
                tr("Copied")
            } else {
                tr("Copy agent prompt")
            },
            ButtonKind::Secondary,
            |ui: &mut Ui, tx: &mut Tx| {
                tx.copy(agent_prompt(&ui.platform.workspace_path));
                ui.commands.prompt_copied = true;
            },
        ));
    }
    actions
}

/// The opt-in, and the confirmation that keeps the released microphone
/// ready for it.
fn enable_card(ui: &Ui, p: Paint) -> El {
    let mut rows = vec![toggle_row(
        p,
        "commands-enabled",
        tr("Voice commands"),
        tr("Say a phrase to open apps and websites or press keys"),
        ui.settings.commands_enabled,
        |ui: &mut Ui| {
            let enabled = !ui.settings.commands_enabled;
            set_enabled(ui, enabled);
        },
    )];
    rows.extend(confirm_row(ui, p));
    card(p, "enable", rows)
}

/// Asks to keep the released microphone ready before commands turn on.
pub(super) fn confirm_row(ui: &Ui, p: Paint) -> Option<El> {
    ui.commands.confirm_enable.then(|| {
        setting_row(
            p,
            "confirm-enable",
            tr("Keep the microphone ready?"),
            Some(tr(
                "Commands listen all the time, so Hex stops releasing the microphone when idle",
            )),
            row()
                .key("actions")
                .gap(8.0)
                .child(button(
                    p,
                    "cancel-enable-commands",
                    tr("Cancel"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| ui.commands.confirm_enable = false,
                ))
                .child(
                    button(
                        p,
                        "confirm-enable-commands",
                        tr("Keep microphone ready & enable commands"),
                        ButtonKind::Primary,
                        |ui: &mut Ui, _| keep_ready_and_enable(ui),
                    )
                    .default_button(),
                ),
        )
        .fill(p.k.c.hover)
    })
}

/// The voice command model's notice while commands are on and it is not
/// ready. Hotkey dictation never waits for it.
fn model_notice(ui: &Ui) -> Option<(&'static str, &'static str)> {
    if !ui.platform.caps.commands || !ui.settings.commands_enabled {
        return None;
    }
    match ui.platform.command_model {
        CommandModel::Disabled | CommandModel::Ready => None,
        CommandModel::Loading => Some((
            tr("Preparing voice commands"),
            tr("The command model is loading. Hotkey dictation does not require it."),
        )),
        CommandModel::Failed => Some((
            tr("Voice commands are unavailable"),
            tr(
                "The command model could not load. Retry or turn off commands; hotkey dictation is unaffected.",
            ),
        )),
    }
}

/// The notice with Turn off commands, and Retry once the model failed.
fn model_notice_panel(ui: &Ui, p: Paint, title: &str, description: &str) -> El {
    let mut actions = row().key("actions").gap(8.0).center_y();
    if ui.platform.command_model == CommandModel::Failed {
        actions = actions.child(button(
            p,
            "retry-commands",
            tr("Retry"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.ask(PlatformRequest::RetryCommandModel),
        ));
    }
    actions = actions.child(button(
        p,
        "disable-commands",
        tr("Turn off commands"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| set_enabled(ui, false),
    ));
    card(
        p,
        "command-model-notice",
        vec![setting_row(
            p,
            "command-model",
            title,
            Some(description),
            actions,
        )],
    )
}

fn detail(ui: &Ui, p: Paint, command: Option<&CatalogCommand>) -> El {
    let Some(command) = command else {
        let searching = !ui.commands.search.text.trim().is_empty();
        return ld::empty_state(
            p,
            "command-detail",
            ld::SEARCH_ICON,
            if searching {
                tr("No matching commands.")
            } else {
                tr("No commands yet.")
            },
            None,
        );
    };
    let aliases = if command.aliases.is_empty() {
        tr("None").to_string()
    } else {
        command.aliases.join(", ")
    };
    let context = ui
        .platform
        .context
        .clone()
        .unwrap_or_else(|| tr("Unavailable").into());
    col()
        .key("command-detail")
        .gap(16.0)
        .child(ld::card_header(
            ld::card_title(p, &command.phrase),
            vec![ld::meta_text(p, &command.description)],
            Vec::new(),
        ))
        .child(ld::facts(
            p,
            vec![
                (tr("Context"), scope_label(&command.scope)),
                (tr("Aliases"), aliases),
                (tr("Current context"), context),
            ],
        ))
}
