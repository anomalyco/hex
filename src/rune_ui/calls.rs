//! Calls: Meetings › Settings' Calls row and the sheet it opens. The row
//! names the answer every call gets and stacks the icons of the apps and
//! websites with their own; the sheet chooses that answer and lists every
//! app and website Hex has seen a call in, plus any with a saved answer,
//! newest first, each with its own menu.
//!
//! The meeting watcher writes the seen list (`meeting::seen_calls`); the
//! window only reads it, every few seconds while Meetings › Settings shows.

use std::sync::mpsc::{Receiver, sync_channel};

use super::controls::{
    ButtonKind, CardRow, METRICS, ModalWidth, Paint, button, card, icon_row, modal_panel,
    modal_sheet, note, setting_row, toggle_row,
};
use super::{Menu, Modal, Pane, Ui, icons};
use crate::app_settings::{MeetingPolicy, MeetingProviderChoice};
use crate::calendar::CallProvider;
use crate::calendar::date::{LocalDate, local_date};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::meeting::seen_calls::{self, SeenCall};
use rune::{El, col, el, row, text};

const ICON: f32 = 20.0;
/// Minimum width of the answer column so last-seen dates align while leaving
/// a clean gap beside the dropdown.
const ANSWER_MIN_WIDTH: f32 = 116.0;
/// How dim an answer that follows the default reads.
const FOLLOWING: f32 = 0.55;
/// The Calls row's icons before the rest become "+N".
const STACKED: usize = 4;
const STACK_ICON: f32 = 18.0;
/// Seconds between reads of the seen list while it shows.
const RELOAD_EVERY: f64 = 5.0;

#[derive(Default)]
pub(super) struct Calls {
    /// The apps and websites Hex has seen a call in, newest first.
    seen: Vec<SeenCall>,
    loader: Option<Receiver<Vec<SeenCall>>>,
    /// The poll clock at the last read.
    read_at: Option<f64>,
    /// A fixed now for snapshots, in milliseconds since the Unix epoch.
    now_ms: Option<i64>,
}

/// One row of the sheet's table.
struct CallRow {
    key: String,
    label: String,
    /// The app's bundle ID (macOS) or executable, for its icon.
    application: Option<String>,
    web: bool,
    last_seen_ms: Option<i64>,
}

impl CallRow {
    /// The key a service's mark is found by: the call's own, else its name
    /// (an app's bundle ID may not name its service).
    fn glyph_key(&self) -> &str {
        if CallProvider::from_meeting_app(&self.key).is_some() {
            &self.key
        } else {
            &self.label
        }
    }
}

/// Every app and website seen, newest first, then those with a saved
/// answer Hex hasn't seen a call in since it began keeping the list.
fn rows(ui: &Ui) -> Vec<CallRow> {
    let mut rows: Vec<CallRow> = ui
        .calls
        .seen
        .iter()
        .map(|seen| CallRow {
            key: seen.key.clone(),
            label: seen.label.clone(),
            application: seen.application.clone(),
            web: seen.application.is_none(),
            last_seen_ms: Some(seen.last_seen_ms),
        })
        .collect();
    for choice in &ui.settings.meetings.providers {
        if !rows.iter().any(|row| row.key == choice.key) {
            rows.push(CallRow {
                key: choice.key.clone(),
                label: choice.label.clone(),
                // A saved app's key is its bundle ID; a website's is its
                // host, which it is also named by.
                application: Some(choice.key.clone()),
                web: choice.label == choice.key && choice.key.contains('.'),
                last_seen_ms: None,
            });
        }
    }
    rows
}

/// The glyph for a call without an app icon: its service's mark, a globe
/// for any other website, else a call.
pub(crate) fn glyph(web: bool, key: &str) -> &'static rune::Icon {
    match CallProvider::from_meeting_app(key) {
        Some(provider) => super::platform_settings::provider_icon(provider),
        None if web => &icons::GLOBE,
        None => super::platform_settings::call_icon(),
    }
}

/// The app's icon when the catalog has it, else its glyph.
fn row_icon(ui: &Ui, p: Paint, call: &CallRow, size: f32) -> El {
    let pixels = call
        .application
        .as_deref()
        .and_then(|id| ui.modes.app_icon(id));
    match pixels {
        Some(pixels) => super::modes::application_icon("icon", &pixels, size),
        None => el().key("icon").size(size, size).center().child(
            rune::icon(glyph(call.web, call.glyph_key()), size - 4.0)
                .key("glyph")
                .color(p.muted()),
        ),
    }
    .a11y_hidden()
}

/// Opens the sheet, alone: it ends a shortcut recording and closes the
/// other sheets and any menu, and reads the seen list at once.
pub(super) fn open(ui: &mut Ui) {
    ui.open_modal(Modal::Calls);
    ui.calls.read_at = None;
}

pub(super) fn close(ui: &mut Ui) {
    ui.close_modal_if(Modal::Calls);
}

pub(super) fn on_close(ui: &mut Ui) {
    if matches!(ui.open, Some(Menu::Call(_))) {
        ui.open = None;
    }
}

/// Shows `seen` at a fixed `now_ms`, the sheet open when `open`.
pub(super) fn fixture(ui: &mut Ui, seen: Vec<SeenCall>, now_ms: i64, open: bool) {
    ui.calls.seen = seen;
    ui.calls.now_ms = Some(now_ms);
    if open {
        self::open(ui);
    }
}

/// Reads the seen list off the UI thread while Meetings › Settings or the
/// sheet shows, and the app catalog for their icons. True when the list
/// changed.
pub(super) fn poll(ui: &mut Ui, clock: f64, visible: bool) -> bool {
    let mut changed = false;
    if let Some(loader) = &ui.calls.loader
        && let Ok(seen) = loader.try_recv()
    {
        ui.calls.loader = None;
        if seen != ui.calls.seen {
            ui.calls.seen = seen;
            changed = true;
        }
    }
    let showing = ui.is_modal(Modal::Calls)
        || (ui.pane == Pane::Meetings
            && ui.meetings.view == super::meetings::View::Settings
            && ui.settings.meetings.enabled);
    if !(showing && visible && ui.live()) {
        return changed;
    }
    ui.modes.ensure_applications();
    if ui.calls.loader.is_none() && ui.calls.read_at.is_none_or(|at| clock - at >= RELOAD_EVERY) {
        ui.calls.read_at = Some(clock);
        let (sender, receiver) = sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(seen_calls::load());
        });
        ui.calls.loader = Some(receiver);
    }
    changed
}

fn last_seen(ms: i64, now_ms: i64) -> String {
    match (local_date(ms), local_date(now_ms)) {
        (Some(date), Some(today)) => day_label(date, today),
        _ => String::new(),
    }
}

/// A day a call was seen: Today, Yesterday, its weekday within a week, else
/// its date.
fn day_label(date: LocalDate, today: LocalDate) -> String {
    match today.days() - date.days() {
        ..=0 => tr("Today").into(),
        1 => tr("Yesterday").into(),
        2..=6 => crate::calendar_menu::weekday_name(date.weekday()).into(),
        _ => {
            let months = tr("Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec");
            let month = months
                .split_whitespace()
                .nth((date.month - 1) as usize)
                .unwrap_or("");
            let fields = [
                ("month", month),
                ("day", &date.day.to_string()),
                ("year", &date.year.to_string()),
            ];
            if today.year == date.year {
                tr_named("{month} {day}", &fields)
            } else {
                tr_named("{month} {day}, {year}", &fields)
            }
        }
    }
}

/// Meetings › Settings' Calls row: the default's name, the icons of the
/// apps and websites that answer otherwise, and a chevron. Pressed
/// anywhere, it opens the sheet.
pub(super) fn settings_row(ui: &Ui, p: Paint) -> El {
    let meetings = &ui.settings.meetings;
    let default = tr(meetings.default_policy.label());
    let own: Vec<CallRow> = rows(ui)
        .into_iter()
        .filter(|call| meetings.has_own_answer(&call.key))
        .collect();
    let description = match own.as_slice() {
        [] => default.to_owned(),
        [call] => format!(
            "{default} · {}",
            tr_named(
                "{name}: {answer}",
                &[
                    ("name", tr(&call.label)),
                    ("answer", tr(meetings.policy_for(&call.key).label())),
                ],
            )
        ),
        own => format!(
            "{default} · {}",
            tr_fill(
                "{} apps and websites answer differently",
                &own.len().to_string()
            )
        ),
    };
    let c = &p.k.c;
    let ring = STACK_ICON + 3.0;
    let stack = row()
        .key("stack")
        .center_y()
        .a11y_hidden()
        .children(own.iter().take(STACKED).enumerate().map(|(index, call)| {
            el().key(format!("own{index}"))
                .size(ring, ring)
                .ml(if index == 0 { 0.0 } else { -6.0 })
                .radius(ring / 2.0)
                .fill(c.bg_base)
                .center()
                .child(row_icon(ui, p, call, STACK_ICON))
        }))
        .children((own.len() > STACKED).then(|| {
            text(format!("+{}", own.len() - STACKED))
                .key("more")
                .font(p.k.ty.small)
                .color(p.muted())
                .ml(4.0)
        }));
    let control = row()
        .key("calls-controls")
        .gap(6.0)
        .center_y()
        .child(stack)
        .child(super::controls::value_button(
            p,
            "calls-control",
            tr("Calls"),
            default,
            &rune_theme_opencode::icons::CHEVRON_RIGHT,
            open,
        ));
    icon_row(
        p,
        "calls",
        &icons::CALL,
        tr("Calls"),
        Some(&description),
        control,
    )
    .on_press(open)
}

fn set_default(ui: &mut Ui, policy: MeetingPolicy) {
    if ui.settings.meetings.default_policy != policy {
        ui.update(|settings| settings.meetings.set_default_policy(policy));
    }
}

/// The default as one row with the same quiet menu as the apps below, and
/// Stop when the call ends as its sibling row, never under a radio, while
/// anything records on its own.
fn default_rows(ui: &Ui, p: Paint, menus: &mut Vec<El>) -> Vec<CardRow> {
    let meetings = &ui.settings.meetings;
    let default = meetings.default_policy;
    let records = default == MeetingPolicy::AlwaysRecord
        || meetings
            .providers
            .iter()
            .any(|choice| choice.policy == MeetingPolicy::AlwaysRecord);
    let select = super::general::quiet_select(
        ui,
        p,
        Menu::Pick("calls-default"),
        "calls-default-control".to_owned(),
        tr("When a call starts"),
        MeetingPolicy::ALL
            .iter()
            .map(|policy| tr(policy.label()).to_owned())
            .collect(),
        MeetingPolicy::ALL
            .iter()
            .position(|known| *known == default)
            .unwrap_or(0),
        |ui, index| set_default(ui, MeetingPolicy::ALL[index]),
        menus,
    );
    let mut rows: Vec<CardRow> =
        vec![setting_row(p, "calls-default", tr("When a call starts"), None, select).into()];
    if records {
        let grace = crate::meeting::detection::AUTO_STOP_GRACE.as_secs();
        let after = tr_fill(
            "{} seconds after the call app leaves the microphone",
            &grace.to_string(),
        );
        rows.push(
            toggle_row(
                p,
                "meeting-stop-when-call-ends",
                tr("Stop when the call ends"),
                after.as_str(),
                meetings.stop_when_call_ends,
                |ui: &mut Ui| {
                    let on = !ui.settings.meetings.stop_when_call_ends;
                    ui.update(|settings| settings.meetings.stop_when_call_ends = on);
                },
            )
            .into(),
        );
    }
    rows
}

/// One app or website: its icon, name, when Hex last saw a call in it, and
/// its answer, dimmed while it follows the default.
fn call_row(
    ui: &Ui,
    p: Paint,
    index: usize,
    call: &CallRow,
    now_ms: i64,
    menus: &mut Vec<El>,
) -> El {
    let meetings = &ui.settings.meetings;
    let name = tr(&call.label).to_owned();
    let policy = meetings.policy_for(&call.key);
    let following = !meetings.has_own_answer(&call.key);
    let (key, label) = (call.key.clone(), call.label.clone());
    let select = super::general::quiet_select(
        ui,
        p,
        Menu::Call(index),
        format!("call{index}-policy-control"),
        &name,
        MeetingPolicy::ALL
            .iter()
            .map(|policy| tr(policy.label()).to_owned())
            .collect(),
        MeetingPolicy::ALL
            .iter()
            .position(|known| *known == policy)
            .unwrap_or(0),
        move |ui, index| {
            let choice = MeetingProviderChoice {
                key: key.clone(),
                label: label.clone(),
                policy: MeetingPolicy::ALL[index],
            };
            ui.update(|settings| {
                crate::app_settings::set_meeting_provider(&mut settings.meetings, choice);
            });
        },
        menus,
    );
    let when = call
        .last_seen_ms
        .map(|ms| last_seen(ms, now_ms))
        .unwrap_or_default();
    row()
        .key(format!("call{index}"))
        .role("group")
        .label(name.clone())
        .min_h(44.0)
        .px(METRICS.row_px)
        .gap(12.0)
        .center_y()
        .child(row_icon(ui, p, call, ICON))
        .child(
            text(name)
                .key("name")
                .font(p.k.ty.ui)
                .color(p.text())
                .grow(1.0)
                .shrink()
                .truncate()
                .a11y_hidden(),
        )
        .children((!when.is_empty()).then(|| {
            text(when)
                .key("when-text")
                .font(p.k.ty.small)
                .color(p.faint())
                .mr(4.0)
        }))
        .child(
            row()
                .key("answer")
                .min_w(ANSWER_MIN_WIDTH)
                .justify_end()
                .opacity(if following { FOLLOWING } else { 1.0 })
                .child(select),
        )
}

/// The sheet's scrim and panel, then the menus its rows opened.
pub(super) fn modal(ui: &Ui, p: Paint) -> Vec<El> {
    let mut menus = Vec::new();
    let now_ms = ui
        .calls
        .now_ms
        .unwrap_or_else(|| crate::history::now_ms() as i64);
    let table: Vec<El> = rows(ui)
        .iter()
        .enumerate()
        .map(|(index, call)| call_row(ui, p, index, call, now_ms, &mut menus))
        .collect();
    let section = |key: &str, title: &str, body: El| {
        col()
            .key(key)
            .gap(METRICS.label_gap)
            .child(p.section_label(title).mx(METRICS.row_px))
            .child(body)
    };
    let list = if table.is_empty() {
        note(
            "calls-empty",
            tr("Apps and websites show up here after Hex sees a call in them."),
            p.muted(),
        )
    } else {
        section("seen", tr("Apps and websites"), card(p, "card", table))
    };
    let panel = modal_panel(
        p,
        "calls-sheet",
        tr("Calls"),
        ModalWidth::Medium,
        vec![card(p, "default", default_rows(ui, p, &mut menus)), list],
        vec![
            button(
                p,
                "close-calls",
                tr("Done"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| close(ui),
            )
            .default_button(),
        ],
    );
    let mut layers = Vec::from(modal_sheet(p, "calls", panel, close));
    // A row's menu opens above the sheet.
    layers.extend(menus);
    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_reads_as_today_yesterday_its_weekday_or_its_date() {
        // Saturday, 21 November 2026.
        let today = LocalDate::from_ymd(20261121);
        let label = |ymd| day_label(LocalDate::from_ymd(ymd), today);
        assert_eq!(label(20261121), "Today");
        assert_eq!(label(20261120), "Yesterday");
        assert_eq!(label(20261119), "Thu");
        assert_eq!(label(20261115), "Sun");
        assert_eq!(label(20261114), "Nov 14");
        assert_eq!(label(20251231), "Dec 31, 2025");
    }
}
