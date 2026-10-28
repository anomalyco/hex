//! The Activity developer pane: the newest dictation, command, and context
//! events from the host's event log (`PlatformState::activity_log`), the
//! listener's state, and a detail with the raw event. Debug builds only.

use std::path::PathBuf;

use rune::{El, col, row, text};

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, METRICS, Paint, button, status_dot};
use super::lists::{
    detail_header, detail_title, fact, message, placeholder, section_label, source_item,
    source_list, split,
};
use crate::desktop_activity::DesktopActivity;
use crate::desktop_i18n::{event_age, tr};
use crate::events::{EventReader, VoiceEvent, VoiceState};

const LIMIT: usize = 100;

#[derive(Default)]
pub(super) struct ActivityPane {
    reader: Option<(PathBuf, EventReader)>,
    events: Vec<VoiceEvent>,
    activity: DesktopActivity,
    pub(super) selected: Option<usize>,
}

impl ActivityPane {
    /// Rereads the log while the pane shows; true when anything changed.
    pub(super) fn poll(&mut self, log: Option<&PathBuf>, visible: bool) -> bool {
        if !visible {
            return false;
        }
        let Some(path) = log else {
            return false;
        };
        if self.reader.as_ref().is_none_or(|(open, _)| open != path) {
            self.reader = Some((path.clone(), EventReader::open(path.clone())));
        }
        self.reload()
    }

    fn reload(&mut self) -> bool {
        let Some((_, reader)) = &mut self.reader else {
            return false;
        };
        let selected = self
            .selected
            .and_then(|index| self.events.get(index))
            .cloned();
        let before = (
            self.events.len(),
            self.events.first().cloned(),
            self.activity.clone(),
        );
        self.activity.refresh(reader);
        if self.activity.error.is_none() {
            self.events = reader.recent(LIMIT);
        }
        self.selected =
            selected.and_then(|selected| self.events.iter().position(|e| *e == selected));
        before
            != (
                self.events.len(),
                self.events.first().cloned(),
                self.activity.clone(),
            )
    }

    pub(super) fn fixture() -> Self {
        use crate::events::{DictationPhase, DictationProcessing, TranscriptPhase};
        let now = crate::events::now_ms();
        let at = |seconds: u64| now - seconds * 1_000;
        let events = vec![
            VoiceEvent::Dictation {
                timestamp_ms: at(4),
                phase: DictationPhase::Pasted,
                text: "Let's move the standup to ten thirty tomorrow.".into(),
                processing: Some(DictationProcessing {
                    profile: "Chat".into(),
                    latency_ms: 910,
                    fallback: None,
                }),
                reason: None,
            },
            VoiceEvent::Dictation {
                timestamp_ms: at(6),
                phase: DictationPhase::Transcribing,
                text: String::new(),
                processing: None,
                reason: None,
            },
            VoiceEvent::Transcript {
                timestamp_ms: at(6),
                phase: TranscriptPhase::Completed,
                latency_ms: 182,
                text: "let's move the standup to ten thirty tomorrow".into(),
            },
            VoiceEvent::Dictation {
                timestamp_ms: at(9),
                phase: DictationPhase::Started,
                text: String::new(),
                processing: None,
                reason: None,
            },
            VoiceEvent::Context {
                timestamp_ms: at(12),
                application: Some("Slack".into()),
                browser_url: None,
            },
            VoiceEvent::State {
                timestamp_ms: at(60),
                state: VoiceState::Listening,
                device: "MacBook Pro Microphone".into(),
            },
            VoiceEvent::SessionStarted {
                timestamp_ms: at(61),
            },
        ];
        Self {
            reader: None,
            events,
            activity: DesktopActivity {
                session_started_at: Some(at(61)),
                state: Some(VoiceState::Listening),
                device: Some("MacBook Pro Microphone".into()),
                error: None,
                last_failure: None,
            },
            selected: Some(0),
        }
    }
}

pub(super) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let pane = &ui.activity;
    let mut actions = Vec::new();
    if let Some(state) = pane.activity.state_label() {
        let active = pane.activity.state != Some(VoiceState::Stopping);
        actions.push(
            row()
                .key("listener-status")
                .gap(8.0)
                .center_y()
                .child(status_dot(if active { p.success() } else { p.muted() }))
                .child(
                    text(tr(state))
                        .key("state")
                        .font(DESCRIPTION)
                        .color(p.text()),
                )
                .children(pane.activity.device.clone().map(|device| {
                    text(device)
                        .key("device")
                        .font(DESCRIPTION)
                        .color(p.muted())
                })),
        );
    }
    actions.push(button(
        p,
        "refresh-activity",
        tr("Refresh"),
        ButtonKind::Secondary,
        |ui: &mut Ui, _| {
            ui.activity.reload();
        },
    ));
    let mut list = col().key("activity-list");
    if let Some(error) = &pane.activity.error {
        list = list.child(message(
            p,
            "activity-error",
            "Activity could not be loaded.",
            Some(error),
        ));
    } else if pane.events.is_empty() {
        list = list.child(message(p, "activity-empty", "No activity yet.", None));
    }
    let c = &p.k.c;
    for (index, event) in pane.events.iter().enumerate() {
        let selected = pane.selected == Some(index);
        let boundary = matches!(event, VoiceEvent::SessionStarted { .. });
        let summary = crate::desktop_activity::event_summary(event);
        let mut item = row()
            .key(format!("activity-event{index}"))
            .role("button")
            .label(summary.clone())
            .px(10.0)
            .py(8.0)
            .gap(12.0)
            .radius(p.k.radius.md)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| ui.activity.selected = Some(index))
            .child(
                text(summary)
                    .key("summary")
                    .font(if boundary {
                        p.k.ty.small_medium
                    } else {
                        p.k.ty.ui
                    })
                    .color(if boundary { p.faint() } else { p.muted() })
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            )
            .child(
                text(event_age(event.timestamp_ms()))
                    .key("age")
                    .font(p.k.ty.small)
                    .color(p.faint()),
            );
        if selected {
            item = item.fill(c.layer_02);
        }
        list = list.child(source_item(item, selected));
    }
    let list = super::scroll_body("activity-scroll", source_list(list));
    let detail = match pane.selected.and_then(|index| pane.events.get(index)) {
        None => placeholder(p, "activity-detail", "Select an activity row."),
        Some(event) => {
            let raw = serde_json::to_string(event).unwrap_or_else(|error| error.to_string());
            col()
                .key("activity-detail")
                .grow(1.0)
                .shrink()
                .px(METRICS.gutter)
                .child(detail_header(
                    detail_title(p, &crate::desktop_activity::event_summary(event)),
                    Vec::new(),
                    Vec::new(),
                ))
                .child(super::scroll_body(
                    "activity-detail-scroll",
                    col()
                        .key("body")
                        .gap(16.0)
                        .pt(16.0)
                        .child(
                            col()
                                .key("facts")
                                .gap(8.0)
                                .child(fact(p, "event", "Event", event.kind()))
                                .child(fact(
                                    p,
                                    "timestamp",
                                    "Timestamp",
                                    &format!("{} ms", event.timestamp_ms()),
                                )),
                        )
                        .child(section_label(p, "raw-label", "Raw event"))
                        .child(
                            text(raw)
                                .key("raw")
                                .font(p.k.ty.mono)
                                .color(p.faint())
                                .wrap(),
                        ),
                ))
        }
    };
    (actions, split(p, "activity-body", list, detail))
}
