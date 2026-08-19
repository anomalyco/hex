//! The shared Activity pane: the live listening session at a glance —
//! state, capture device, session age, the most recent transcripts, and
//! where the raw observations live. Read-only, so both port shells
//! render it from the snapshot without a delegate.

use gpui::{AnyElement, div, prelude::*, px, rgb};

use crate::desktop_host::DesktopSnapshot;
use crate::desktop_i18n::{tr, tr_fill};
use crate::desktop_ui::{
    CRITICAL, FAINT, MUTED, SUCCESS, TEXT, TEXT_SOFT, empty_message, event_age, pane_content,
    pane_header, settings_panel, settings_row, settings_section_label,
};

pub(crate) fn render_activity_pane(snapshot: &DesktopSnapshot) -> AnyElement {
    let running = snapshot
        .listener
        .as_ref()
        .is_some_and(|listener| listener.running);
    let state = snapshot.listener.as_ref().map_or_else(
        || tr("Ready").to_string(),
        |listener| tr(&listener.status).to_string(),
    );
    let device = snapshot
        .activity
        .device
        .clone()
        .unwrap_or_else(|| tr("Automatic microphone").into());
    let session_age = snapshot.activity.session_started_at.map(event_age);
    let transcripts = snapshot.activity.transcripts.clone();

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(pane_header("Activity"))
        .child(
            div()
                .id("activity-scroll")
                .flex_1()
                .overflow_y_scroll()
                .px_8()
                .pt_1()
                .pb_7()
                .child(
                    div().w_full().flex().justify_center().child(
                        pane_content()
                            .child(div().pt(px(20.0)).child(settings_section_label("Session")))
                            .child(
                                settings_panel()
                                    .child(
                                        div()
                                            .w_full()
                                            .min_h(px(56.0))
                                            .px_4()
                                            .py_3()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .child(
                                                div().size(px(10.0)).flex_none().rounded_full().bg(
                                                    if running { rgb(SUCCESS) } else { rgb(FAINT) },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .min_w(px(0.0))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.0))
                                                    .child(
                                                        div()
                                                            .text_size(px(14.0))
                                                            .text_color(rgb(TEXT))
                                                            .child(state),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.0))
                                                            .text_color(rgb(MUTED))
                                                            .truncate()
                                                            .child(match &session_age {
                                                                Some(age) => tr_fill(
                                                                    "Session started {}",
                                                                    age,
                                                                ),
                                                                None => {
                                                                    tr("No session recorded yet")
                                                                        .to_string()
                                                                }
                                                            }),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        settings_row(
                                            "Microphone",
                                            "The device the active session captures from",
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(rgb(MUTED))
                                                .child(device),
                                        )
                                        .border_b_0(),
                                    )
                                    .when_some(
                                        snapshot.activity.last_failure.clone(),
                                        |panel, failure| {
                                            panel.child(
                                                div()
                                                    .w_full()
                                                    .px_4()
                                                    .pb_3()
                                                    .text_size(px(12.0))
                                                    .text_color(rgb(CRITICAL))
                                                    .child(tr_fill(
                                                        "Last dictation failed: {}",
                                                        &failure,
                                                    )),
                                            )
                                        },
                                    ),
                            )
                            .child(settings_section_label("Recent transcripts"))
                            .child(if transcripts.is_empty() {
                                settings_panel()
                                    .child(empty_message(tr(
                                        "Nothing transcribed this session yet.",
                                    )))
                                    .into_any_element()
                            } else {
                                settings_panel()
                                    .children(transcripts.iter().enumerate().map(
                                        |(index, transcript)| {
                                            let last = index + 1 == transcripts.len();
                                            div()
                                                .w_full()
                                                .px_4()
                                                .py_3()
                                                .when(!last, |row| {
                                                    row.border_b_1().border_color(rgb(
                                                        crate::desktop_ui::DIVIDER,
                                                    ))
                                                })
                                                .text_size(px(12.0))
                                                .line_height(px(19.0))
                                                .text_color(rgb(TEXT_SOFT))
                                                .child(transcript.clone())
                                        },
                                    ))
                                    .into_any_element()
                            })
                            .child(settings_section_label("Observations"))
                            .child(
                                settings_panel().child(
                                    settings_row(
                                        "Event log",
                                        "Newest-first session observations on this disk",
                                        div()
                                            .max_w(px(320.0))
                                            .text_size(px(11.0))
                                            .text_color(rgb(FAINT))
                                            .truncate()
                                            .child(snapshot.observations_path.clone()),
                                    )
                                    .border_b_0(),
                                ),
                            ),
                    ),
                ),
        )
        .into_any_element()
}
