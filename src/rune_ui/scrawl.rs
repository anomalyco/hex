//! The Annotate pane (macOS, Linux, and Windows): Scrawl's captures inside
//! Hex. The opt-in with Screen Recording while it's missing and the capture
//! shortcut, collapsed How it works, what happens after an annotation (how
//! long it's kept and its folder), optional Agents setup, and an Advanced fold
//! for the transcriber, demos, and the replay Lab. Scrawl's own app keeps the
//! shortcut while it runs until "Use Hex for annotations" asks it to quit.

pub(super) mod routing_rules;

use rune::{El, col, row, text};
use scrawl_capture::retention::{Policy, Preset};

use super::controls::{
    ButtonKind, CardRow, Choice, METRICS, Paint, button, card, folder_label, folding, icon_row,
    icon_toggle_row, nested, note, page, section, section_with, segmented, select, setting_row,
    step_row,
};
use super::general::{advanced_open, advanced_toggle, disclosure_link, keys_button};
use super::icons::{CLOCK, FOLDER, KEYBOARD, WAVE};
use super::lists::permission_control;
use super::platform::{PlatformRequest, ScrawlRequest, ScrawlView};
use super::settings::cancel_capture;
use super::sidebar::SCRAWL_ICON;
use super::{Menu, Pane, ShortcutTarget, Ui};
use crate::agent_install::{self, Agent, AgentStatus, SkillState};
use crate::app_settings::{AppSettings, CaptureTranscriber};
use crate::desktop::i18n::{tr, tr_fill};
use crate::desktop::onboarding::{PermissionAction, PermissionKind};

/// The switch's second line: the whole gesture with the bound shortcut while
/// on and a shortcut is bound, else what Annotate does.
fn description(ui: &Ui, enabled: bool) -> String {
    if enabled && ui.records_shortcuts() {
        let separator = if cfg!(target_os = "macos") { "" } else { "+" };
        tr_fill(
            "Tap {}, talk and draw; tap again and it's copied",
            &ui.settings.capture.hotkey.keycaps().join(separator),
        )
    } else {
        tr("Record your screen, draw on it, and talk over it").into()
    }
}

/// Turns Annotate on or off; turning it on fails while its shortcut
/// collides with another, and leaves the shortcuts as they are.
fn set_enabled(settings: &mut AppSettings, enabled: bool) -> Result<(), &'static str> {
    if enabled {
        // Judged as it will be, not as it is: a shortcut the capture
        // shortcuts only reserve while Annotate is on (the Screenshots
        // record shortcut among them) must still count.
        let mut candidate = settings.clone();
        candidate.capture.enabled = true;
        if super::settings::conflicts(
            &candidate,
            ShortcutTarget::Capture,
            &candidate.capture.hotkey,
        ) {
            return Err(tr(
                "The capture shortcut is already in use. Change it below before turning captures on.",
            ));
        }
    }
    settings.capture.enabled = enabled;
    Ok(())
}

pub(super) fn toggle(ui: &mut Ui) {
    if crate::policy::Policy::current().locks(crate::policy::Lock::Annotate) {
        return;
    }
    cancel_capture(ui);
    let enabled = !ui.settings.capture.enabled;
    let mut candidate = ui.drafted();
    let error = match set_enabled(&mut candidate, enabled) {
        Ok(()) if ui.commit(candidate) => None,
        Ok(()) => ui.error.clone(),
        Err(error) => Some(error.to_string()),
    };
    if let Some(view) = ui.platform.scrawl.as_mut() {
        view.enable_error = error;
    }
}

fn transcriber_description(transcriber: CaptureTranscriber) -> &'static str {
    match transcriber {
        CaptureTranscriber::Parakeet => tr("Uses Hex's dictation model"),
        CaptureTranscriber::Apple => tr("Apple's speech recognizer"),
    }
}

/// What the Keep menu calls `policy`: one of Scrawl's presets, or the days
/// imported from Scrawl.
fn retention_label(policy: Policy) -> String {
    match Preset::ALL
        .into_iter()
        .find(|preset| preset.policy() == policy)
    {
        Some(Preset::Week) => tr("Video for 7 days").into(),
        Some(Preset::Month) => tr("Video for 30 days").into(),
        Some(Preset::Forever) => tr("Forever").into(),
        None => match policy.media_days {
            Some(days) => tr_fill("Video for {} days", &days.to_string()),
            None => tr("Forever").into(),
        },
    }
}

/// The non-obvious half of retention: the dropdown already names video days,
/// so the caption only notes how long the annotation itself lasts.
fn retention_description(policy: Policy) -> Option<String> {
    match (policy.media_days, policy.bundle_days) {
        (_, Some(days)) => Some(tr_fill("Annotations for {} days", &days.to_string())),
        (Some(_), None) => Some(tr("Annotations stay until you remove them").into()),
        (None, None) => None,
    }
}

/// The steps in order, each with its keys where it has any: one key taps
/// to start and stop and is held to draw; the capsule's pen draws without
/// it; Esc stops drawing.
fn how_it_works_steps(ui: &Ui, p: Paint) -> Vec<El> {
    let shortcut = ui.settings.capture.hotkey.keycaps();
    let keys = |key: &str, parts: &[String]| Some(super::controls::keycaps(p, key, parts));
    // Wayland tells Hex nothing of Shift outside its own windows.
    let draw_how = if cfg!(not(target_os = "linux")) {
        tr(
            "While it records, hold the shortcut and move the pointer; no click needed. Shift draws too",
        )
    } else {
        tr("While it records, hold the shortcut and move the pointer; no click needed")
    };
    vec![
        step_row(
            p,
            "how-start",
            1,
            tr("Start"),
            keys("how-start-keys", &shortcut),
            tr("Tap the shortcut to start recording; talk as you go"),
        ),
        step_row(
            p,
            "how-draw",
            2,
            tr("Draw"),
            keys("how-draw-keys", &shortcut),
            draw_how,
        ),
        step_row(
            p,
            "how-pen",
            3,
            tr("Pen"),
            keys("how-pen-keys", &["Esc".to_string()]),
            tr(
                "Or click the pen on the bar, then drag to draw; click it again or press Esc to stop",
            ),
        ),
        step_row(
            p,
            "how-stop",
            4,
            tr("Stop"),
            keys("how-stop-keys", &shortcut),
            tr("Tap the shortcut again: the annotation is copied, ready to paste"),
        ),
        step_row(
            p,
            "how-send",
            5,
            tr("Send"),
            None,
            tr("Paste it anywhere, or click Send to… on the capsule to pick a session"),
        ),
    ]
}

/// A fold's link, and under it, while open, its card.
fn fold(p: Paint, key: &str, link: El, rows: Option<Vec<El>>) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(link)
        .children(rows.map(|rows| card(p, "card", rows)))
}

/// How it works, closed until its link is pressed.
fn how_it_works(ui: &Ui, p: Paint, view: &ScrawlView) -> El {
    let link = disclosure_link(
        p,
        "how-toggle",
        tr("How it works"),
        view.how_it_works,
        |ui: &mut Ui| {
            if let Some(view) = ui.platform.scrawl.as_mut() {
                view.how_it_works = !view.how_it_works;
            }
        },
    );
    fold(
        p,
        "how",
        link,
        view.how_it_works.then(|| how_it_works_steps(ui, p)),
    )
}

fn scrawl_request(ui: &mut Ui, request: ScrawlRequest) {
    ui.ask(PlatformRequest::Scrawl(request));
}

/// While Scrawl's own app runs it keeps the shortcut; only this card's
/// button asks it to quit.
fn scrawl_running(p: Paint, view: &ScrawlView) -> El {
    let description = if view.quitting_scrawl {
        tr("Waiting for Scrawl to quit…")
    } else {
        tr("Hex leaves the Annotate shortcut to Scrawl until you switch")
    };
    card(
        p,
        "scrawl-running",
        vec![setting_row(
            p,
            "scrawl-running-row",
            tr("Scrawl is running"),
            Some(description),
            button(
                p,
                "use-hex-for-captures",
                tr("Use Hex for annotations"),
                ButtonKind::Primary,
                |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::QuitScrawl),
            ),
        )],
    )
}

/// The switch, Screen Recording while Annotate can't record yet, and the
/// shortcut (or desktop command on Linux without a shortcuts portal).
fn top_rows(ui: &Ui, p: Paint, view: &ScrawlView) -> Vec<CardRow> {
    let enabled = ui.settings.capture.enabled;
    let switch = if crate::policy::Policy::current().locks(crate::policy::Lock::Annotate) {
        super::controls::managed_row(p, "capture-enabled", Some(&SCRAWL_ICON), tr("Annotate"))
    } else {
        icon_toggle_row(
            p,
            "capture-enabled",
            &SCRAWL_ICON,
            tr("Annotate"),
            description(ui, enabled).as_str(),
            enabled,
            toggle,
        )
    };
    let mut rows = vec![
        switch.into(),
        folding(
            "capture-error-fold",
            view.enable_error.as_ref().map(|error| {
                note(
                    "capture-error",
                    &format!("{} {error}", tr("Could not update Annotate:")),
                    p.danger(),
                )
            }),
        ),
        folding("screen-recording-fold", screen_recording_row(ui, p, view)),
    ];
    // Without a shortcuts portal nothing registers the key: the row says
    // which command to bind in the desktop's own keyboard settings.
    if !ui.records_shortcuts() {
        if enabled {
            rows.push(
                note(
                    "no-portal",
                    tr(
                        "Your desktop has no shortcuts portal. Bind this command in its keyboard settings.",
                    ),
                    p.muted(),
                )
                .into(),
            );
            rows.push(
                super::settings::command_row(
                    p,
                    "capture-command",
                    tr("Annotate"),
                    tr("Start and stop an annotation"),
                    &["hex annotate toggle"],
                )
                .into(),
            );
        }
        return rows;
    }
    rows.push(
        icon_row(
            p,
            "capture-shortcut",
            &KEYBOARD,
            tr("Shortcut"),
            enabled.then(|| tr("Hold while recording to draw")),
            keys_button(ui, p, ShortcutTarget::Capture),
        )
        .into(),
    );
    rows
}

/// Screen Recording, only while Annotate is on and can't record the screen.
/// Linux asks through the desktop's screen-cast portal when a capture
/// starts, so it has none.
fn screen_recording_row(ui: &Ui, p: Paint, view: &ScrawlView) -> Option<El> {
    if !ui.settings.capture.enabled || !cfg!(target_os = "macos") {
        return None;
    }
    if view.screen_needs_restart {
        // macOS lets a running app use the grant only after it relaunches.
        return Some(nested(setting_row(
            p,
            "screen-recording",
            tr("Screen Recording"),
            Some(tr("Turns on when Hex restarts")),
            button(
                p,
                "screen-recording-restart",
                tr("Restart Hex"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.ask(PlatformRequest::Permission(PermissionAction::Restart)),
            ),
        )));
    }
    if view.screen_recording {
        return None;
    }
    let stale = ui.platform.stale(PermissionKind::ScreenRecording);
    Some(nested(setting_row(
        p,
        "screen-recording",
        tr("Screen Recording"),
        Some(if stale {
            tr("An older Hex entry can keep this off")
        } else {
            tr("Hex can't see your screen yet")
        }),
        permission_control(
            p,
            "screen-recording-allow",
            ui.platform
                .control_for(PermissionKind::ScreenRecording, false),
            ButtonKind::Secondary,
            move |ui: &mut Ui| {
                let action = if stale {
                    PermissionAction::ResetAndPrompt(PermissionKind::ScreenRecording)
                } else {
                    PermissionAction::ShowGuide(PermissionKind::ScreenRecording)
                };
                ui.ask(PlatformRequest::Permission(action))
            },
        ),
    )))
}

/// Keep, then the folder.
fn after_rows(ui: &Ui, p: Paint, view: &ScrawlView, layer: &mut Vec<El>) -> Vec<El> {
    let policy = ui.settings.capture.retention();
    let mut choices: Vec<Choice> = Preset::ALL
        .into_iter()
        .map(|preset| Choice {
            label: retention_label(preset.policy()),
            detail: None,
            selected: preset.policy() == policy,
            choose: Box::new(move |ui: &mut Ui| {
                ui.open = None;
                ui.update(|settings| settings.capture.set_retention(preset.policy()));
            }),
        })
        .collect();
    if !choices.iter().any(|choice| choice.selected) {
        choices.push(Choice {
            label: retention_label(policy),
            detail: None,
            selected: true,
            choose: Box::new(|ui: &mut Ui| ui.open = None),
        });
    }
    let kept = retention_description(policy);
    let folder = folder_label(&ui.settings.capture.captures_dir());
    let mut rows = vec![
        routing_rules::settings_row(ui, p),
        icon_row(
            p,
            "capture-retention",
            &CLOCK,
            tr("Keep"),
            kept.as_deref(),
            select(
                p,
                "capture-retention-menu",
                tr("Keep"),
                &retention_label(policy),
                ui.open == Some(Menu::CaptureRetention),
                choices,
                |ui: &mut Ui| {
                    ui.open =
                        (ui.open != Some(Menu::CaptureRetention)).then_some(Menu::CaptureRetention);
                },
                layer,
            ),
        ),
        icon_row(
            p,
            "capture-folder",
            &FOLDER,
            tr("Folder"),
            Some(folder.as_str()),
            row()
                .key("controls")
                .gap(8.0)
                .center_y()
                .child(button(
                    p,
                    "reveal-capture-folder",
                    tr("Reveal"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::RevealFolder),
                ))
                .child(button(
                    p,
                    "choose-capture-folder",
                    tr("Change…"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| scrawl_request(ui, ScrawlRequest::ChooseFolder),
                )),
        ),
    ];
    if let Some(error) = &view.folder_error {
        rows.push(note("capture-folder-error", error, p.danger()));
    }
    rows
}

/// The transcriber, showing Hex in screen recordings for demos, and the
/// replay Lab: settings most people never change. macOS only.
fn more_rows(ui: &Ui, p: Paint) -> Vec<El> {
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut rows = vec![transcriber_row(ui, p), recordings_row(ui, p)];
    #[cfg(target_os = "macos")]
    if crate::labs::Feature::AnnotateReplay.on_this_os()
        && crate::labs::Feature::AnnotateReplay.on(&ui.settings)
    {
        let mut control = row()
            .key("capture-replay-shortcut-control")
            .gap(4.0)
            .center_y()
            .child(keys_button(ui, p, ShortcutTarget::CaptureReplay));
        if ui.settings.capture.replay_hotkey.is_some() {
            control = control.child(super::controls::icon_button(
                p,
                "clear-capture-replay-shortcut",
                &rune_theme_opencode::icons::TRASH,
                tr("Clear shortcut"),
                |ui: &mut Ui| {
                    ui.update(|settings| settings.capture.replay_hotkey = None);
                },
            ));
        }
        use super::lists::{Tone, tone_tag};
        rows.push(super::controls::icon_row_tagged(
            p,
            "capture-replay-shortcut",
            &super::icons::REFRESH,
            tr("Replay last 15 s"),
            Some(tone_tag(p, "lab", tr("Lab"), Tone::Accent)),
            Some(tr("Press to save; hold to narrate")),
            control,
        ));
    }
    rows
}

/// Lets screen recordings, captures included, show Hex's overlays.
fn recordings_row(ui: &Ui, p: Paint) -> El {
    icon_toggle_row(
        p,
        "capture-show-in-recordings",
        &rune_kit::glyphs::EYE,
        tr("Show Hex in screen recordings"),
        tr("Include the capsule, ink, and HUD"),
        ui.settings.capture.show_overlays_in_recordings,
        |ui: &mut Ui| {
            ui.update(|settings| {
                settings.capture.show_overlays_in_recordings =
                    !settings.capture.show_overlays_in_recordings;
            });
        },
    )
}

fn transcriber_row(ui: &Ui, p: Paint) -> El {
    let transcriber = ui.settings.capture.transcriber;
    let selected = CaptureTranscriber::ALL
        .iter()
        .position(|choice| *choice == transcriber)
        .unwrap_or(0);
    icon_row(
        p,
        "capture-transcriber",
        &WAVE,
        tr("Transcriber"),
        Some(transcriber_description(transcriber)),
        segmented(
            p,
            "capture-transcriber-choice",
            &["Parakeet", "Apple"],
            selected,
            |ui: &mut Ui, index| {
                let transcriber = CaptureTranscriber::ALL[index];
                ui.update(|settings| settings.capture.transcriber = transcriber);
            },
        ),
    )
}

/// The command the copy row hands to people who manage skills themselves.
const SKILLS_COMMAND: &str = "npx skills add anomalyco/hex --skill hex-annotate";

/// The Annotate pane's Agents section (Labs › Install agent skill from
/// Annotate): each coding agent's skill folder and, on macOS, the hex link.
/// The folders are read when the pane opens and after an install, never
/// while drawing.
#[derive(Default)]
pub(super) struct Agents {
    paths: Option<agent_install::Paths>,
    statuses: Vec<AgentStatus>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    cli: Option<agent_install::cli::Status>,
    /// One per `statuses` row: checked means Install writes there.
    selected: Vec<bool>,
    error: Option<String>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    cli_error: Option<String>,
}

impl Agents {
    /// Reads every agent's folder and the hex link, and checks each found
    /// agent that cannot read this Hex's skill.
    pub(super) fn refresh(&mut self) {
        self.paths = agent_install::Paths::current();
        self.read();
    }

    fn read(&mut self) {
        if let Some(paths) = &self.paths {
            self.statuses = agent_install::status(paths);
        }
        #[cfg(target_os = "macos")]
        {
            self.cli = Some(agent_install::cli::status(
                &agent_install::cli::Environment::current(),
            ));
        }
        self.selected = self
            .statuses
            .iter()
            .map(|status| status.found && !status.linked && status.skill != SkillState::Current)
            .collect();
    }

    /// Whether Install has anything to write.
    fn any_selected(&self) -> bool {
        self.selected.iter().any(|selected| *selected)
    }

    fn selected_for(&self, agent: Agent) -> bool {
        self.statuses
            .iter()
            .position(|status| status.agent == agent)
            .and_then(|index| self.selected.get(index).copied())
            .unwrap_or(false)
    }

    fn toggle(&mut self, agent: Agent) {
        let Some(index) = self
            .statuses
            .iter()
            .position(|status| status.agent == agent && status.found && !status.linked)
        else {
            return;
        };
        if let Some(selected) = self.selected.get_mut(index) {
            *selected = !*selected;
        }
    }

    /// Writes the skill into every checked agent's folder, then reads them
    /// again so each row says Installed.
    fn install(&mut self) {
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let mut failure = None;
        for (index, status) in self.statuses.iter().enumerate() {
            if !self.selected.get(index).copied().unwrap_or(false) || status.linked {
                continue;
            }
            if let Err(error) = agent_install::install(&paths, status.agent) {
                failure.get_or_insert(error);
            }
        }
        self.read();
        self.error = failure;
    }

    #[cfg(target_os = "macos")]
    fn install_cli(&mut self) {
        use agent_install::cli::LinkError;
        self.cli_error = None;
        match agent_install::cli::install(&agent_install::cli::Environment::current()) {
            Ok(status) => self.cli = Some(status),
            Err(LinkError::Translocation) => {
                self.cli_error = Some(tr("Move Hex to Applications first").to_string());
            }
            Err(LinkError::Io(error)) => self.cli_error = Some(error),
        }
    }

    /// A scene's rows: one ready to install, one outdated, one current, one
    /// not found, and the link already installed.
    pub(super) fn fixture() -> Self {
        let home = std::path::PathBuf::from("/Users/you");
        Self {
            statuses: vec![
                AgentStatus {
                    agent: Agent::ClaudeCode,
                    found: true,
                    skill: SkillState::Missing,
                    path: home.join(".claude/skills/hex-annotate/SKILL.md"),
                    linked: false,
                },
                AgentStatus {
                    agent: Agent::Codex,
                    found: true,
                    skill: SkillState::Outdated,
                    path: home.join(".agents/skills/hex-annotate/SKILL.md"),
                    linked: false,
                },
                AgentStatus {
                    agent: Agent::OpenCode,
                    found: true,
                    skill: SkillState::Current,
                    path: home.join(".config/opencode/skills/hex-annotate/SKILL.md"),
                    linked: false,
                },
                AgentStatus {
                    agent: Agent::Pi,
                    found: false,
                    skill: SkillState::Missing,
                    path: home.join(".pi/agent/skills/hex-annotate/SKILL.md"),
                    linked: false,
                },
            ],
            selected: vec![true, true, false, false],
            cli: Some(agent_install::cli::Status {
                state: agent_install::cli::State::Installed,
                path: std::path::PathBuf::from("/opt/homebrew/bin/hex"),
                off_path: false,
            }),
            ..Self::default()
        }
    }
}

/// One agent's row: its name, whether it is found and has this Hex's skill,
/// and a switch that checks it for Install. A not-found agent, and a skill
/// someone else manages through a symlink, has no switch: it is read by
/// content and never written here.
fn agent_row(ui: &Ui, p: Paint, status: &AgentStatus) -> El {
    // A symlinked skill or folder is someone else's to update; the row says
    // so rather than offering a state Hex can't change.
    let label = if status.linked {
        tr("Managed elsewhere")
    } else {
        match (status.found, status.skill) {
            (false, _) => tr("Not found"),
            (true, SkillState::Missing) => tr("Not installed"),
            (true, SkillState::Current) => tr("Installed"),
            (true, SkillState::Outdated) => tr("Update available"),
        }
    };
    let key = format!("agent-{}", status.agent.key());
    if !status.found || status.linked {
        return setting_row(p, &key, status.agent.name(), Some(label), row());
    }
    let checked = ui.agents.selected_for(status.agent);
    let row = setting_row(
        p,
        &key,
        status.agent.name(),
        Some(label),
        super::controls::row_switch(p, checked),
    );
    let agent = status.agent;
    super::controls::pressable_row(row, checked)
        .on_press(move |ui: &mut Ui| ui.agents.toggle(agent))
}

/// The command people who manage skills themselves run, with its copy.
fn agent_command_row(p: Paint) -> El {
    let command = row()
        .key("command")
        .gap(8.0)
        .center_y()
        .child(
            text(SKILLS_COMMAND)
                .key("line")
                .font(p.k.ty.mono)
                .color(p.muted())
                .a11y_hidden(),
        )
        .child(button(
            p,
            "agents-skill-copy",
            tr("Copy"),
            ButtonKind::Secondary,
            |_, tx| tx.copy(SKILLS_COMMAND.to_owned()),
        ));
    setting_row(
        p,
        "agents-skill-command",
        tr("Install with the skills CLI"),
        Some(tr("For skills you manage yourself")),
        command,
    )
}

/// The macOS Command-line tool row, with its Install, and where the link is
/// when it landed in `~/.local/bin`, which may not be on PATH.
#[cfg(target_os = "macos")]
fn cli_rows(ui: &Ui, p: Paint) -> Vec<El> {
    use agent_install::cli::{State, Status};
    let status: Option<&Status> = ui.agents.cli.as_ref();
    let (label, installable) = match status {
        Some(Status {
            state: State::Installed,
            path,
            ..
        }) => (tr_fill("Installed at {}", &folder_label(path)), false),
        Some(Status {
            state: State::OwnedByAnother,
            ..
        }) => (tr("Owned by another program").to_string(), false),
        Some(_) | None => (tr("Not installed").to_string(), true),
    };
    let control = if installable {
        button(
            p,
            "agents-cli-install",
            tr("Install"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.agents.install_cli(),
        )
    } else {
        row()
    };
    let mut rows = vec![setting_row(
        p,
        "agents-cli",
        tr("Command-line tool"),
        Some(label.as_str()),
        control,
    )];
    if let Some(Status {
        path,
        off_path: true,
        ..
    }) = status
    {
        let folder = path.parent().unwrap_or(path);
        rows.push(note(
            "agents-cli-path",
            &tr_fill("{} isn't on your PATH", &folder_label(folder)),
            p.muted(),
        ));
    }
    if let Some(error) = &ui.agents.cli_error {
        rows.push(note(
            "agents-cli-error",
            &tr_fill("Couldn't link the command-line tool: {}", error),
            p.danger(),
        ));
    }
    rows
}

/// The Agents section: the intro, each agent's row, the copyable skills
/// command, and on macOS the Command-line tool.
fn agents_section(ui: &Ui, p: Paint) -> El {
    let mut rows = vec![note(
        "agents-intro",
        tr("Let coding agents read your captures with the hex-annotate skill"),
        p.muted(),
    )];
    for status in &ui.agents.statuses {
        rows.push(agent_row(ui, p, status));
    }
    if let Some(error) = &ui.agents.error {
        rows.push(note(
            "agents-error",
            &tr_fill("Couldn't install the skill: {}", error),
            p.danger(),
        ));
    }
    rows.push(agent_command_row(p));
    #[cfg(target_os = "macos")]
    rows.extend(cli_rows(ui, p));
    if !ui.settings.labs.annotate_agent_tools {
        rows.push(note(
            "agents-tools-off",
            tr("The skill's zoom, inspect, and frames commands need Labs › Agent tools for captures on"),
            p.muted(),
        ));
    }
    section_with(
        p,
        "agents",
        tr("Agents"),
        super::controls::button_enabled(
            p,
            "agents-install",
            tr("Install"),
            ButtonKind::Secondary,
            ui.agents.any_selected(),
            |ui: &mut Ui, _| ui.agents.install(),
        ),
        rows,
    )
}

pub(super) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let view = ui.platform.scrawl.clone().unwrap_or_default();
    let enabled = ui.settings.capture.enabled;
    let mut content = page("content").child(card(p, "enable", top_rows(ui, p, &view)));
    if !enabled {
        return super::scroll_body(p.k, "scroll", content);
    }
    if view.scrawl_running {
        content = content.child(scrawl_running(p, &view));
    }
    content = content.child(how_it_works(ui, p, &view)).child(section(
        p,
        "after",
        tr("After an annotation"),
        after_rows(ui, p, &view, layer),
    ));
    if ui.settings.labs.annotate_agent_install {
        content = content.child(agents_section(ui, p));
    }
    // Apple's speech recognizer, keeping Hex out of recordings, and replay
    // are macOS's; Linux and Windows transcribe with Hex's model.
    if cfg!(target_os = "macos") {
        let link = advanced_toggle(ui, p, Pane::Annotate);
        let open = advanced_open(ui, Pane::Annotate);
        content = content.child(fold(p, "advanced", link, open.then(|| more_rows(ui, p))));
    }
    super::scroll_body(p.k, "scroll", content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn captures_default_on_and_refuse_a_shortcut_in_use() {
        let settings = AppSettings::default();
        assert!(settings.capture.enabled);
        let mut taken = AppSettings::default();
        taken.capture.hotkey = taken.dictation_hotkey.clone();
        let before = serde_json::to_value(&taken).unwrap();
        assert!(
            set_enabled(&mut taken, true)
                .unwrap_err()
                .contains("already in use")
        );
        assert_eq!(serde_json::to_value(&taken).unwrap(), before);
        // Turning captures off always works.
        set_enabled(&mut taken, false).unwrap();
    }

    /// A record shortcut saved while Annotate was off holds its key: giving
    /// Annotate the same chord and turning it on is refused, since the
    /// Screenshots record shortcut would otherwise go dead and Annotate's
    /// whole-screen recording would take the chord.
    #[cfg(target_os = "macos")]
    #[test]
    fn annotate_will_not_take_a_saved_record_shortcut() {
        let record = crate::app_settings::HotkeyBinding {
            modifiers: crate::app_settings::HotkeyModifiers {
                command: Some(crate::app_settings::ModifierSide::Either),
                shift: Some(crate::app_settings::ModifierSide::Either),
                ..Default::default()
            },
            key: Some(crate::app_settings::HotkeyKey::named("r")),
        };
        let mut settings = AppSettings::default();
        settings.capture.enabled = false;
        settings.labs.screenshots = true;
        settings.screenshots.record_hotkey = Some(record.clone());
        settings.capture.hotkey = record;
        let before = serde_json::to_value(&settings).unwrap();
        assert!(
            set_enabled(&mut settings, true)
                .unwrap_err()
                .contains("already in use")
        );
        assert_eq!(serde_json::to_value(&settings).unwrap(), before);

        // With the Lab off, every Mac's saved record shortcut doesn't hold
        // the key, and turning the Lab on lets the record shortcut go.
        settings.labs.screenshots = false;
        set_enabled(&mut settings, true).unwrap();
        assert!(settings.capture.enabled);
        crate::labs::Feature::Screenshots.set(&mut settings, true);
        assert_eq!(settings.screenshots.record_hotkey, None);
        assert!(settings.capture.enabled);
    }

    /// The Scrawl pane drawn from `scene`, laid out headlessly.
    fn pane(scene: &str) -> rune::Runtime<Ui> {
        use crate::rune_ui::{SCENES, Service};
        let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        let (_, apply) = SCENES.iter().find(|(name, _)| *name == scene).unwrap();
        apply(&mut state);
        render(state, (980.0, 760.0))
    }

    fn render(state: Ui, size: (f32, f32)) -> rune::Runtime<Ui> {
        let env = rune::Env {
            system_dark: true,
            scale: 1.0,
        };
        let text =
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        let mut rt =
            crate::rune_ui::offscreen::runtime(state, crate::rune_ui::view, size, env, text);
        rt.settle_all();
        rt.frame(0.0);
        rt
    }

    /// A fresh folder under the temp directory, removed when the test ends,
    /// panic or not.
    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static SEQUENCE: AtomicU32 = AtomicU32::new(0);
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hex-scrawl-agents-{}-{name}-{sequence}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The Annotate pane with the Agents lab on and these agents' folders.
    fn agents_pane(home: &std::path::Path) -> rune::Runtime<Ui> {
        use crate::rune_ui::{Pane, Service};
        let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        state.settings.capture.enabled = true;
        state.settings.labs.annotate_agent_install = true;
        state.platform.scrawl = Some(ScrawlView::preview());
        state.pane = Pane::Annotate;
        state.agents.paths = Some(agent_install::Paths {
            home: home.to_path_buf(),
            ..agent_install::Paths::default()
        });
        state.agents.read();
        // Tall enough that every agent row and the CLI row are on screen.
        render(state, (980.0, 1720.0))
    }

    /// The text shown, without the recorder's invisible sizing faces.
    fn texts(rt: &rune::Runtime<Ui>) -> Vec<String> {
        rt.elements()
            .into_iter()
            .filter(|e| !e.path.contains("/faces/size"))
            .filter_map(|e| e.text)
            .collect()
    }

    fn tap(rt: &mut rune::Runtime<Ui>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
    }

    fn asked(rt: &rune::Runtime<Ui>) -> &[PlatformRequest] {
        match &rt.state.host {
            crate::rune_ui::platform::PlatformHost::Fixture(sent) => sent,
            _ => panic!("scenes record platform requests"),
        }
    }

    #[cfg(target_os = "macos")]
    const ALLOW: &str = "screen-recording-allow";

    #[cfg(target_os = "macos")]
    #[test]
    fn granted_screen_recording_hides_its_row() {
        let rt = pane("mac-scrawl");
        assert!(rt.find("screen-recording").is_none());
        assert!(rt.find(ALLOW).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn screen_recording_allow_opens_the_permission_guide() {
        let mut rt = pane("mac-scrawl-permission");
        tap(&mut rt, ALLOW);
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Permission(PermissionAction::ShowGuide(
                PermissionKind::ScreenRecording
            ))]
        );
        assert!(
            rt.find(&format!("{ALLOW}-control/mark/dot")).is_some(),
            "waits with a dot"
        );
        // The fixture grants on the second ask.
        tap(&mut rt, ALLOW);
        assert!(rt.state.platform.scrawl.as_ref().unwrap().screen_recording);
        rt.settle_all();
        rt.frame(0.0);
        assert!(rt.find(ALLOW).is_none(), "the row folds away once granted");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rare_settings_stay_folded_until_opened() {
        let mut rt = pane("mac-scrawl");
        assert!(rt.find("capture-transcriber").is_none());
        tap(&mut rt, "advanced-toggle");
        assert!(rt.find("capture-transcriber").is_some());
        assert!(rt.find("capture-show-in-recordings").is_some());
    }

    #[test]
    fn how_it_works_opens_from_its_link() {
        let mut rt = pane("mac-scrawl");
        assert!(rt.find("how-start").is_none());
        tap(&mut rt, "how-toggle");
        assert!(rt.find("how-start").is_some());
    }

    #[test]
    fn off_shows_only_the_top_card() {
        let rt = pane("mac-scrawl-off");
        assert!(rt.find("capture-enabled").is_some());
        assert!(rt.find("capture-shortcut").is_some());
        assert!(rt.find("how-toggle").is_none());
        assert!(rt.find("after").is_none());
        assert!(rt.find("advanced-toggle").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_grant_this_hex_cant_use_yet_offers_a_restart() {
        let mut rt = pane("mac-scrawl-restart");
        assert!(
            rt.find(ALLOW).is_none(),
            "no Allow over a switch already on"
        );
        tap(&mut rt, "screen-recording-restart");
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Permission(PermissionAction::Restart)]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn stale_screen_recording_offers_fix_it_and_resets_permission() {
        let mut rt = pane("mac-scrawl-permission");
        rt.state
            .platform
            .note_stale(PermissionKind::ScreenRecording);
        rt.invalidate();
        rt.settle_all();
        rt.frame(0.0);
        assert!(
            rt.find(&format!("{ALLOW}-control/mark/dot")).is_some(),
            "stale keeps the waiting dot"
        );
        tap(&mut rt, ALLOW);
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Permission(
                PermissionAction::ResetAndPrompt(PermissionKind::ScreenRecording)
            )]
        );
        assert!(!rt.state.platform.stale(PermissionKind::ScreenRecording));
    }

    #[test]
    fn opening_the_pane_reads_screen_recording_again() {
        let mut rt = pane("mac-scrawl");
        tap(&mut rt, "nav-general");
        tap(&mut rt, "nav-labs");
        tap(&mut rt, "nav-annotate");
        assert_eq!(
            asked(&rt),
            [PlatformRequest::Scrawl(ScrawlRequest::Refresh)]
        );
    }

    #[test]
    fn retention_names_presets_and_imported_days() {
        assert_eq!(retention_label(Preset::Week.policy()), "Video for 7 days");
        assert_eq!(retention_label(Preset::Forever.policy()), "Forever");
        assert_eq!(retention_description(Preset::Forever.policy()), None);
        let imported = Policy {
            media_days: Some(3),
            bundle_days: Some(30),
        };
        assert_eq!(retention_label(imported), "Video for 3 days");
        assert_eq!(
            retention_description(imported).as_deref(),
            Some("Annotations for 30 days")
        );
        let video_only = Policy {
            media_days: Some(3),
            bundle_days: None,
        };
        assert_eq!(
            retention_description(video_only).as_deref(),
            Some("Annotations stay until you remove them")
        );
    }

    /// The Agents section is exactly absent while its lab is off.
    #[test]
    fn the_agents_section_needs_its_lab() {
        let off = pane("mac-scrawl");
        assert!(off.find("agents").is_none());
        assert!(off.find("agents-install").is_none());
        assert!(
            !texts(&off)
                .iter()
                .any(|text| text
                    == "Let coding agents read your captures with the hex-annotate skill")
        );

        let on = pane("mac-scrawl-agents");
        assert!(on.find("agents").is_some());
        assert!(on.find("agents-install").is_some());
        assert!(on.find("agents-skill-copy").is_some());
        for agent in Agent::ALL {
            assert!(
                on.find(&format!("agent-{}", agent.key())).is_some(),
                "{} has no row",
                agent.name()
            );
        }
        let shown = texts(&on);
        for status in [
            "Not installed",
            "Update available",
            "Installed",
            "Not found",
        ] {
            assert!(shown.iter().any(|text| text == status), "{status}");
        }
        // The skill's agent commands are their own lab, named here while off.
        assert!(shown.iter().any(|text| text
            == "The skill's zoom, inspect, and frames commands need Labs › Agent tools for captures on"));
        #[cfg(target_os = "macos")]
        {
            assert!(on.find("agents-cli").is_some());
            assert!(
                shown
                    .iter()
                    .any(|text| text == "Installed at /opt/homebrew/bin/hex")
            );
        }
    }

    /// Install writes the skill into each checked, found agent's own folder
    /// and never creates one for an agent this computer doesn't have.
    #[test]
    fn installing_writes_each_checked_agents_skill() {
        let temp = TempDir::new("install");
        let home = temp.path();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let mut rt = agents_pane(home);
        assert_eq!(
            rt.state.agents.selected,
            vec![true, true, false, false],
            "found agents without the skill start checked"
        );
        tap(&mut rt, "agents-install");
        assert_eq!(
            std::fs::read(home.join(".claude/skills/hex-annotate/SKILL.md")).unwrap(),
            agent_install::SKILL.as_bytes()
        );
        assert_eq!(
            std::fs::read(home.join(".agents/skills/hex-annotate/SKILL.md")).unwrap(),
            agent_install::SKILL.as_bytes(),
            "Codex reads the shared copy"
        );
        assert!(
            std::fs::read(home.join(".pi/agent/skills/hex-annotate/SKILL.md")).is_err(),
            "Pi was not found, so nothing was written for it"
        );
        assert_eq!(
            rt.state.agents.statuses[0].skill,
            SkillState::Current,
            "the row says Installed now"
        );
        assert_eq!(rt.state.agents.selected, vec![false; 4]);
        // Nothing is checked now, so Install is disabled.
        let install = crate::rune_ui::accessibility_tests::node(&rt, "agents-install");
        assert!(
            install["states"]
                .as_array()
                .is_some_and(|states| states.iter().any(|state| state == "disabled")),
            "Install is disabled while nothing is checked: {install}"
        );
    }

    /// Clicking one agent's row checks or unchecks only that agent, and a
    /// not-found agent's row does nothing.
    #[test]
    fn an_agents_row_checks_only_that_agent() {
        let temp = TempDir::new("toggle");
        let home = temp.path();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".pi")).unwrap();
        let mut rt = agents_pane(home);
        assert_eq!(rt.state.agents.selected, vec![true, false, false, true]);
        tap(&mut rt, "agent-claude-code");
        assert_eq!(rt.state.agents.selected, vec![false, false, false, true]);
        tap(&mut rt, "agent-claude-code");
        assert_eq!(rt.state.agents.selected, vec![true, false, false, true]);
        // Pi is found, so its row toggles; OpenCode is not found, so its row
        // has no switch and tapping it does nothing.
        tap(&mut rt, "agent-pi");
        assert_eq!(rt.state.agents.selected, vec![true, false, false, false]);
        tap(&mut rt, "agent-opencode");
        assert_eq!(rt.state.agents.selected, vec![true, false, false, false]);
    }

    /// A skill someone else manages through a symlink is read by content but
    /// has no switch, so Install never writes through the link.
    #[cfg(unix)]
    #[test]
    fn a_linked_skill_has_no_switch() {
        let temp = TempDir::new("linked");
        let home = temp.path().join("home");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        std::fs::create_dir_all(outside.join("hex-annotate")).unwrap();
        std::fs::write(outside.join("hex-annotate/SKILL.md"), "an older skill").unwrap();
        std::os::unix::fs::symlink(
            outside.join("hex-annotate"),
            home.join(".claude/skills/hex-annotate"),
        )
        .unwrap();
        let mut rt = agents_pane(&home);
        assert!(rt.state.agents.statuses[0].linked);
        assert_eq!(rt.state.agents.statuses[0].skill, SkillState::Outdated);
        assert!(
            texts(&rt).iter().any(|text| text == "Managed elsewhere"),
            "the row says someone else manages it: {:?}",
            texts(&rt)
        );
        assert_eq!(
            rt.state.agents.selected,
            vec![false; 4],
            "a linked skill is never checked"
        );
        tap(&mut rt, "agent-claude-code");
        assert_eq!(rt.state.agents.selected, vec![false; 4]);
        tap(&mut rt, "agents-install");
        assert_eq!(
            std::fs::read_to_string(outside.join("hex-annotate/SKILL.md")).unwrap(),
            "an older skill",
            "nothing was written through the link"
        );
    }
}
