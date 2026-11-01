use serde_json::Value;

use super::{
    CONTROL_VERSION, ControlReply, ControlResponse, EXIT_NO_SPEECH, EXIT_NOT_RUNNING, EXIT_REFUSED,
    Output, fail,
};
use crate::control::{ModelInfo, StatusReport, settings::SettingValue};
use crate::dictation::vocabulary::VocabularyEntry;

/// Prints `reply` and returns its exit code.
pub(crate) fn print(reply: ControlReply, output: Output) -> u8 {
    let code = exit_code(&reply);
    if output.json {
        let response = ControlResponse {
            version: CONTROL_VERSION,
            reply,
        };
        match serde_json::to_string_pretty(&response) {
            Ok(json) => println!("{json}"),
            Err(error) => return fail(&error.to_string()),
        }
        return code;
    }
    match reply {
        ControlReply::Done { message, .. } => println!("{message}"),
        ControlReply::Status(report) => print!("{}", human_status(&report)),
        ControlReply::Settings { settings } => print!("{}", human_settings(&settings)),
        ControlReply::Models { models } => print!("{}", human_models(&models)),
        ControlReply::Vocabulary { entries } => print!("{}", human_vocabulary(&entries)),
        ControlReply::ScrawlCapture(report) => print!("{}", human_scrawl_capture(&report)),
        ControlReply::ScrawlCaptures { captures } => print!("{}", human_captures(&captures)),
        ControlReply::ScrawlCaptureDetail(detail) => print!("{}", human_capture_detail(&detail)),
        ControlReply::ScrawlCaptureRoute { explanation } => print!("{explanation}"),
        ControlReply::Transcript { text } => println!("{text}"),
        ControlReply::Error { message, .. } => eprintln!("hex: {message}"),
    }
    code
}

pub(super) fn human_scrawl_capture(report: &crate::control::ScrawlCaptureReport) -> String {
    use crate::control::ScrawlCaptureState;
    let mut out = String::new();
    let bundle = |bundle: &Option<std::path::PathBuf>| {
        bundle
            .as_ref()
            .map_or_else(String::new, |bundle| bundle.display().to_string())
    };
    match (report.state, report.changed) {
        (ScrawlCaptureState::Recording, true) => {
            out.push_str(&format!("Capturing into {}\n", bundle(&report.bundle)));
        }
        (ScrawlCaptureState::Recording, false) => out.push_str(&format!(
            "Capturing into {} ({} ms)\n",
            bundle(&report.bundle),
            report.elapsed_ms.unwrap_or_default()
        )),
        (ScrawlCaptureState::Idle, true) => {
            out.push_str(&format!("Ended the capture {}\n", bundle(&report.bundle)));
        }
        (ScrawlCaptureState::Idle, false) => out.push_str("No capture is recording\n"),
    }
    for finishing in &report.finishing {
        out.push_str(&format!("Writing {}\n", finishing.display()));
    }
    if let Some(last) = &report.last {
        let words = last
            .words
            .map_or_else(String::new, |words| format!(", {words} words"));
        let gaps = last
            .gaps
            .filter(|gaps| *gaps > 0)
            .map_or_else(String::new, |gaps| format!(", {gaps} audio gaps"));
        let missing = last
            .missing_transcript
            .as_ref()
            .map_or_else(String::new, |why| format!(", no transcript ({why})"));
        out.push_str(&format!(
            "Last: {} {}{words}{gaps}{missing}\n",
            last.stage,
            last.bundle.display()
        ));
    }
    out
}

pub(super) fn human_captures(captures: &[crate::control::ScrawlCaptureSummary]) -> String {
    if captures.is_empty() {
        return "No captures\n".into();
    }
    captures
        .iter()
        .map(|capture| format!("{}\n", capture_line(capture)))
        .collect()
}

/// `1792933491953.scrawl  2026-10-26 14:02  0:04  window · Brave  12 words  → Hex`
pub(super) fn capture_line(capture: &crate::control::ScrawlCaptureSummary) -> String {
    let seconds = capture.duration_ms / 1_000;
    let mut line = format!(
        "{}  {}  {}:{:02}  {}",
        capture.id,
        local_minute(capture.created_at_ms),
        seconds / 60,
        seconds % 60,
        capture.mode
    );
    if let Some(app) = &capture.app {
        line.push_str(&format!(" · {app}"));
    }
    line.push_str(&format!("  {} words", capture.words));
    if let Some(sent) = &capture.sent_to {
        line.push_str(&format!("  → {}", sent.title));
    }
    if capture.queued {
        line.push_str(" (queued)");
    }
    line
}

pub(super) fn human_capture_detail(detail: &crate::control::ScrawlCaptureDetail) -> String {
    let mut out = format!(
        "{}\n{}\n",
        capture_line(&detail.summary),
        detail.summary.bundle.display()
    );
    if let Some(title) = &detail.window_title {
        out.push_str(&format!("Window: {title}\n"));
    }
    if let Some(sent) = &detail.summary.sent_to {
        out.push_str(&format!("Sent to: {} ({})\n", sent.title, sent.session));
    }
    out.push_str(&format!(
        "{} marks, {} keyframes\n",
        detail.marks, detail.keyframes
    ));
    for (label, path) in [
        ("Video", &detail.video),
        ("Audio", &detail.audio),
        ("Contact sheet", &detail.contact_sheet),
    ] {
        if let Some(path) = path {
            out.push_str(&format!("{label}: {}\n", path.display()));
        }
    }
    if let Some(note) = &detail.note {
        out.push_str(&format!("\nNote: {note}\n"));
    }
    match (&detail.missing_transcript, detail.transcript.trim()) {
        (Some(why), _) => out.push_str(&format!("\nNo transcript: {why}\n")),
        (None, "") => out.push_str("\nNo words\n"),
        (None, text) => out.push_str(&format!("\n{text}\n")),
    }
    out
}

/// `2026-10-26 14:02` in local time where Hex knows it.
pub(super) fn local_minute(ms: u64) -> String {
    let iso = crate::meeting::query::iso_local(ms);
    iso.get(..16)
        .map_or(iso.clone(), |minute| minute.replace('T', " "))
}

pub(super) fn exit_code(reply: &ControlReply) -> u8 {
    match reply {
        ControlReply::Error { code, .. } if code == "not-running" => EXIT_NOT_RUNNING,
        ControlReply::Error { code, .. } if code == "no-speech" => EXIT_NO_SPEECH,
        ControlReply::Error { .. } => EXIT_REFUSED,
        _ => 0,
    }
}

pub(super) fn human_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "none".into(),
        other => other.to_string(),
    }
}

pub(super) fn human_settings(settings: &[SettingValue]) -> String {
    if let [setting] = settings {
        return format!("{}\n", human_value(&setting.value));
    }
    let width = settings
        .iter()
        .map(|setting| setting.key.len())
        .max()
        .unwrap_or(0);
    settings
        .iter()
        .map(|setting| {
            let mut value = human_value(&setting.value);
            if value.len() > 60 {
                value = format!("{}…", value.chars().take(59).collect::<String>());
            }
            format!(
                "{:width$}  {value}{}\n",
                setting.key,
                if !setting.available {
                    "  (not on this OS)"
                } else if setting.writable {
                    ""
                } else {
                    "  (read-only)"
                }
            )
        })
        .collect()
}

pub(super) fn human_models(models: &[ModelInfo]) -> String {
    let width = models.iter().map(|model| model.id.len()).max().unwrap_or(0);
    let name_width = models
        .iter()
        .map(|model| model.name.len())
        .max()
        .unwrap_or(0);
    models
        .iter()
        .map(|model| {
            let languages = if model.languages.len() > 6 {
                format!("{} languages", model.languages.len())
            } else {
                model.languages.join(", ")
            };
            format!(
                "{} {:width$}  {:name_width$}  {:>8}  {:13}  {languages}{}\n",
                if model.selected { "*" } else { " " },
                model.id,
                model.name,
                model.size,
                if model.installed {
                    "installed"
                } else {
                    "not installed"
                },
                if model.source.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", model.source)
                },
            )
        })
        .collect()
}

pub(super) fn human_vocabulary(entries: &[VocabularyEntry]) -> String {
    if entries.is_empty() {
        return "The vocabulary is empty; `hex vocabulary add <term>` adds one\n".into();
    }
    entries
        .iter()
        .map(|entry| match entry.sounds_like.as_slice() {
            [] => format!("{}\n", entry.term),
            spellings => format!("{}  (sounds like {})\n", entry.term, spellings.join(", ")),
        })
        .collect()
}

pub(super) fn duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

pub(super) fn on(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

pub(super) fn human_status(report: &StatusReport) -> String {
    let memory = report
        .memory_footprint_bytes
        .map(|bytes| format!(", {} MB", bytes / 1_000_000))
        .unwrap_or_default();
    let capture = match report.capture.recording {
        Some(kind) => format!(
            "recording {}",
            serde_json::to_value(kind)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        ),
        None => serde_json::to_value(report.capture.state)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
    };
    let pending = match report.capture.pending_jobs {
        0 => String::new(),
        jobs => format!(" ({jobs} pending)"),
    };
    let meeting = match (&report.meeting.title, report.meeting.elapsed_seconds) {
        (Some(title), Some(elapsed)) => {
            format!(
                "{} “{title}” for {}",
                report.meeting.state,
                duration(elapsed)
            )
        }
        _ if report.features.meetings => report.meeting.state.clone(),
        _ => "off".into(),
    };
    let model = &report.model;
    let mut model_line = format!(
        "{} ({}){}",
        model.name,
        model.language,
        if model.installed {
            ""
        } else {
            ", not installed"
        }
    );
    if let Some(preparing) = &model.preparing {
        model_line.push_str(&format!(
            "; preparing {preparing} ({} MB)",
            model.downloaded_bytes / 1_000_000
        ));
    }
    if let Some(error) = &model.error {
        model_line.push_str(&format!("; error: {error}"));
    }
    let features = &report.features;
    let permissions = &report.permissions;
    let permission_line = if permissions.ready {
        "ready".to_string()
    } else {
        format!(
            "microphone {}, accessibility {}, model {}",
            permissions.microphone,
            permissions.accessibility,
            if permissions.transcription_model {
                "ready"
            } else {
                "missing"
            }
        )
    };
    let window = match (&report.window.pane, report.window.open) {
        (Some(pane), true) => format!("open ({pane})"),
        (None, true) => "open".into(),
        _ => "closed".into(),
    };
    format!(
        "Hex {} (pid {}{memory})\n\
         Listener     {}\n\
         Capture      {capture}{pending}\n\
         Meeting      {meeting}\n\
         Model        {model_line}\n\
         Features     meetings {}, release mic while idle {}, history {}\n\
         Permissions  {permission_line}\n\
         Window       {window}\n\
         Update       {}\n\
         Labs         {}\n",
        report.app_version,
        report.pid,
        if report.listener.running {
            format!("running: {}", report.listener.status)
        } else {
            format!("waiting: {}", report.listener.status)
        },
        on(features.meetings),
        on(features.release_microphone_while_idle),
        features.history_retention,
        report.update,
        match report.labs.as_slice() {
            [] => "none".to_string(),
            names => names.join(", "),
        },
    )
}
