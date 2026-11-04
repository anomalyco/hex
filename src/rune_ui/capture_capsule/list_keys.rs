//! Prototype (t1339): the send-to list with its keys the same in every
//! state. Drawn only when the playground picks a [`ListKeys`] other than
//! `Today`; the app never does, so the shipping list is unchanged.
//!
//! Every direction keeps one footer under the suggestions and the search
//! alike. Holding Shift turns it into the exact rule a pick would save,
//! or why none can be made here, with the rule's scope (site, page, app,
//! window, …) cycling on Tab. The playground's [`Sample`] stands in for
//! what the router would have to say for that: the app in front, why it
//! can't key a rule, and the scopes a rule could match.

// Only the macOS playground sets the samples.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::RwLock;

use super::tuning::ListKeys;
use super::*;

/// What a rule can match, as the router would offer it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    Site,
    Page,
    App,
    Window,
    /// A terminal's working folder (proposed; the router can't read it).
    Folder,
    /// OpenCode's focused session (proposed).
    Session,
}

impl ScopeKind {
    fn label(self) -> &'static str {
        match self {
            ScopeKind::Site => "Site",
            ScopeKind::Page => "Page",
            ScopeKind::App => "App",
            ScopeKind::Window => "Window",
            ScopeKind::Folder => "Folder",
            ScopeKind::Session => "Session",
        }
    }

    fn badge(self) -> Badge {
        match self {
            ScopeKind::Site | ScopeKind::Page => Badge::Website,
            ScopeKind::Session => Badge::Chat,
            ScopeKind::App | ScopeKind::Window | ScopeKind::Folder => Badge::App,
        }
    }
}

/// The playground's stand-in for what the router would tell the list.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    /// The app in front, when a rule can't be keyed to it.
    pub(crate) app: Option<&'static str>,
    /// Why no rule can be made there, in a few words.
    pub(crate) why_not: &'static str,
    /// What a rule could match here, narrowest first.
    pub(crate) scopes: &'static [(ScopeKind, &'static str)],
    pub(crate) scope: usize,
    /// Shift is down (the flow forgets it when no rule can be made).
    pub(crate) shift: bool,
    /// The countdown's share left, 0…1 (C).
    pub(crate) left: f32,
    /// Jev is confident in the top suggestion, so C counts it down.
    pub(crate) jev: bool,
}

impl Sample {
    pub(crate) const NONE: Sample = Sample {
        app: None,
        why_not: "",
        scopes: &[],
        scope: 0,
        shift: false,
        left: 0.6,
        jev: true,
    };
}

static SAMPLE: RwLock<Sample> = RwLock::new(Sample::NONE);

pub(crate) fn sample() -> Sample {
    *SAMPLE.read().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn set_sample(sample: Sample) {
    *SAMPLE.write().unwrap_or_else(|e| e.into_inner()) = sample;
}

pub(crate) fn update(change: impl FnOnce(&mut Sample)) {
    change(&mut SAMPLE.write().unwrap_or_else(|e| e.into_inner()));
}

/// Whether the list draws a prototype instead of today's.
pub(super) fn active() -> bool {
    tuning::get().list_keys != ListKeys::Today
}

/// The rule a Shift pick would save, if one can be made: its kind and what
/// it matches.
fn rule(list: &List, s: &Sample) -> Option<(Badge, String)> {
    if let Some((kind, value)) = s.scopes.get(s.scope) {
        return Some((kind.badge(), (*value).to_owned()));
    }
    list.always.as_ref().map(|(site, _)| {
        let kind = if site.contains('.') && !site.contains(char::is_whitespace) {
            Badge::Website
        } else {
            Badge::App
        };
        (kind, site.clone())
    })
}

fn shift_down(list: &List, s: &Sample) -> bool {
    list.always.as_ref().is_some_and(|(_, shift)| *shift) || s.shift
}

/// The scope chips show while Shift is down and there's a choice.
fn scoping(list: &List, s: &Sample) -> bool {
    shift_down(list, s) && s.scopes.len() > 1
}

const SCOPE_H: f32 = 30.0;
const KEY_COL: f32 = 30.0;
const RING: f32 = 14.0;

fn footer_h(list: &List, s: &Sample) -> f32 {
    LIST_PAD + BAND_H + if scoping(list, s) { SCOPE_H } else { 0.0 }
}

pub(super) fn height(list: &List) -> f32 {
    let s = sample();
    let header = if headed(list) { HEADER_H } else { 0.0 };
    let rows = list
        .rows
        .len()
        .max(list.min_rows)
        .max(usize::from(unmatched(list))) as f32;
    LIST_PAD + header + rows * ROW_H + footer_h(list, &s)
}

fn numbered(keys: ListKeys) -> bool {
    matches!(keys, ListKeys::Numbers | ListKeys::Countdown)
}

pub(super) fn view(p: Paint, list: &List, x: f32, w: f32, reduced: bool) -> El {
    let k = p.k;
    let keys = tuning::get().list_keys;
    let s = sample();
    let h = height(list);
    let blur = if reduced { 0.0 } else { PRESENCE_BLUR };
    let scale = if reduced { 1.0 } else { 0.96 };
    let t = tuning::get();
    let in_place = Transition::blur(blur)
        .combined(Transition::scale(scale))
        .springs(SpringPair::new(t.content_in, t.content_out));
    let suggesting = list.suggesting();
    let shift = shift_down(list, &s);
    let can_rule = rule(list, &s).is_some();
    let saving = shift && can_rule;
    let counting = keys == ListKeys::Countdown && suggesting && !shift && s.jev;
    let mut body = col().absolute(0.0, 0.0).w(w).px(LIST_PAD).pt(LIST_PAD);
    if headed(list) {
        body = body.child(
            row()
                .key(format!("section-{}", list.title))
                .items_end()
                .h(HEADER_H)
                .px(ROW_PX)
                .pb(HEADER_PB)
                .child(label(list.title, k.ty.small_medium, p.faint()))
                .transition(in_place),
        );
    }
    let key_col = if numbered(keys) {
        KEY_COL + ROW_PX
    } else {
        0.0
    };
    for (n, dest) in list.rows.iter().enumerate() {
        let content = w - 2.0 * LIST_PAD - 4.0 * ROW_PX - key_col;
        let why_room = (content - measure(&dest.name, medium())).max(content * 0.4);
        let why = if saving && n == list.selected {
            "always"
        } else {
            dest.why.as_str()
        };
        let why = ellipsize(why, small(), why_room);
        let max = content - measure(&why, small());
        let lit = (suggesting && n == 0) || saving && n == list.selected;
        let mut entry = row()
            .key(format!("row-{}", dest.name))
            .trace_name("row")
            .center_y()
            .h(ROW_H)
            .px(ROW_PX)
            .gap(ROW_PX)
            .radius(ROW_RADIUS)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .child(label(
                &ellipsize(&dest.name, medium(), max),
                medium(),
                p.text(),
            ))
            .child(el().grow(1.0))
            .child(
                label(&why, small(), if lit { p.accent() } else { p.muted() })
                    .offset(0.0, (medium().size - small().size) * BASELINE_DROP),
            );
        if numbered(keys) {
            let slot = if counting && n == 0 {
                row()
                    .w(KEY_COL)
                    .justify_end()
                    .child(canvas(ring(p, s.left)).size(RING, RING))
            } else {
                let cap = if n < 5 {
                    format!("{}{}", if shift { "⌥⇧" } else { "⌥" }, n + 1)
                } else {
                    String::new()
                };
                // Shift without a rule: the keys still send, just don't
                // remember, so they stay as they were.
                let cap = if shift && !can_rule {
                    format!("⌥{}", n + 1)
                } else {
                    cap
                };
                row().w(KEY_COL).justify_end().child(
                    label(&cap, small(), if saving { p.accent() } else { p.faint() })
                        .offset(0.0, (medium().size - small().size) * BASELINE_DROP),
                )
            };
            entry = entry.child(slot.key("key"));
        }
        entry = entry
            .role("button")
            .label(dest.name.clone())
            .hit()
            .on_press_tx(press(Id::Row(n)))
            .transition(in_place);
        let chosen = if suggesting {
            counting && n == 0
        } else {
            n == list.selected
        };
        if chosen {
            entry = entry.fill(k.c.layer_02);
        }
        body = body.child(entry);
    }
    if unmatched(list) {
        body = body.child(
            row()
                .key("unmatched")
                .center_y()
                .h(ROW_H)
                .px(ROW_PX)
                .child(label(UNMATCHED, medium(), p.faint()))
                .transition(in_place),
        );
    }
    let footer = footer(p, list, &s, keys, w);
    let fh = footer_h(list, &s) - LIST_PAD;
    let band = el()
        .key("footer")
        .absolute(0.0, h - fh)
        .size(w, fh)
        .fill(k.c.layer_01)
        .child(
            el().key("rule")
                .absolute(0.0, 0.0)
                .size(w, p.hair)
                .fill(k.c.border_weak),
        )
        .child(
            col()
                .absolute(0.0, 0.0)
                .size(w, fh)
                .px(LIST_PAD + ROW_PX)
                .child(footer),
        );
    list_panel(p, body.child(band), None, x, w, h, blur)
}

/// The footer: the same three keys over the suggestions and the search,
/// or, Shift down, the rule a pick would save.
fn footer(p: Paint, list: &List, s: &Sample, keys: ListKeys, w: f32) -> El {
    let room = w - 2.0 * LIST_PAD - 2.0 * ROW_PX;
    let line = |child: El| {
        row()
            .key("line")
            .h(BAND_H)
            .center_y()
            .child(child.grow(1.0))
    };
    if shift_down(list, s) {
        let rule_line = line(rule_hint(p, list, s, room));
        return if scoping(list, s) {
            col()
                .child(
                    row()
                        .key("scopes")
                        .h(SCOPE_H)
                        .center_y()
                        .child(scopes(p, s)),
                )
                .child(rule_line)
        } else {
            col().child(rule_line)
        };
    }
    let suggesting = list.suggesting();
    let can_rule = rule(list, s).is_some();
    let n = list.rows.len().min(5);
    let range = if n > 1 {
        format!("1–{n}")
    } else {
        "1".into()
    };
    let shortcut = surface::shortcut_keycaps();
    let send: Option<(&str, Vec<String>)> = match (suggesting, keys) {
        _ if list.rows.is_empty() => None,
        (true, ListKeys::Countdown) if s.jev => Some(("Send now", vec!["⌥".into(), "↵".into()])),
        (true, ListKeys::Numbers | ListKeys::Countdown) => None,
        (true, _) => Some(("Send", vec!["⌥".into(), range])),
        (false, _) => Some(("Send", vec!["↵".into()])),
    };
    let always: Vec<String> = if suggesting {
        vec!["⇧".into()]
    } else {
        vec!["⇧".into(), "↵".into()]
    };
    let (right, right_keys) = if suggesting {
        ("Search", shortcut)
    } else {
        ("New capture", shortcut)
    };
    let mut used = 0.0;
    if let Some((copy, keys)) = &send {
        used += hint_width(copy, keys) + HINTS_GAP;
    }
    used += hint_width("Always", &always);
    let fits_right = used + HINTS_GAP + hint_width(right, &right_keys) <= room;
    let always_hint = hint(p, "Always", &always).key("always");
    line(
        row()
            .center_y()
            .gap(HINTS_GAP)
            .children(send.map(|(copy, keys)| hint(p, copy, &keys).key("send")))
            .child(if can_rule {
                always_hint
            } else {
                always_hint.opacity(0.4)
            })
            .child(el().grow(1.0))
            .children(fits_right.then(|| hint(p, right, &right_keys).key("right"))),
    )
}

/// Shift down: "Always [🌐 site] → [💬 SEER]", or why there's no rule.
fn rule_hint(p: Paint, list: &List, s: &Sample, room: f32) -> El {
    let Some((kind, value)) = rule(list, s) else {
        let mut hint = row().center_y().gap(BADGE_GAP).key("no-rule");
        match s.app {
            Some(app) => {
                hint = hint
                    .child(label("No rule for", small(), p.muted()))
                    .child(badge(p, Badge::App, app).key("app"))
                    .child(label(s.why_not, small(), p.faint()));
            }
            None => {
                hint = hint.child(label(
                    "No rule: no page or app to remember",
                    small(),
                    p.faint(),
                ));
            }
        }
        return hint.child(el().grow(1.0));
    };
    let to = list.selected_dest().map(|dest| dest.name.as_str());
    let tab = s.scopes.len() > 1;
    let tab_w = if tab {
        hint_width("Scope", &["⇥"]) + HINTS_GAP
    } else {
        0.0
    };
    let words = measure("Always", small()) + measure("→", small()) + 3.0 * BADGE_GAP;
    let badges = 2.0 * (2.0 * BADGE_PX + BADGE_ICON + BADGE_ICON_GAP);
    let left = (room - words - badges - tab_w).max(0.0);
    let to_w = to.map_or(0.0, |to| measure(to, small()));
    let value_w = measure(&value, small());
    let (value_room, to_room) = if value_w + to_w <= left {
        (value_w, to_w)
    } else {
        let to_room = to_w.min(left * 0.4);
        (left - to_room, to_room)
    };
    let mut hint = row()
        .center_y()
        .gap(BADGE_GAP)
        .key("saving")
        .child(label("Always", small(), p.muted()))
        .child(badge(p, kind, &ellipsize(&value, small(), value_room)).key("match"))
        .child(label("→", small(), p.muted()));
    hint = match to {
        Some(to) => hint.child(badge(p, Badge::Chat, &ellipsize(to, small(), to_room)).key("to")),
        None => hint.child(label("the row you pick", small(), p.muted())),
    };
    hint.child(el().grow(1.0))
        .children(tab.then(|| super::view::hint(p, "Scope", &["⇥"]).key("tab")))
}

/// The scopes a rule could match, the picked one filled.
fn scopes(p: Paint, s: &Sample) -> El {
    let k = p.k;
    let mut chips = row()
        .center_y()
        .gap(4.0)
        .child(label("Match", small(), p.faint()).mr(4.0));
    for (n, (kind, _)) in s.scopes.iter().enumerate() {
        let on = n == s.scope;
        let mut chip = row()
            .key(format!("scope-{}", kind.label()))
            .center()
            .h(BADGE_H)
            .px(8.0)
            .radius(BADGE_RADIUS)
            .child(label(
                kind.label(),
                small(),
                if on { p.text() } else { p.muted() },
            ));
        if on {
            chip = chip.fill(k.alpha(k.c.text_base, 0.09));
        }
        chips = chips.child(chip);
    }
    chips.child(el().grow(1.0))
}

/// The countdown's ring: what's left of it, drawn clockwise from the top,
/// over a faint track.
fn ring(p: Paint, left: f32) -> Shape {
    let arc = |from: f32, to: f32| {
        let steps = 48;
        let mut path = Path::new();
        for i in 0..=steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            let pt = [12.0 + 9.0 * a.cos(), 12.0 + 9.0 * a.sin()];
            path = if i == 0 {
                path.move_to(pt)
            } else {
                path.line_to(pt)
            };
        }
        path
    };
    let top = -FRAC_PI_2;
    let k = p.k;
    Shape::new()
        .view_box(24.0, 24.0)
        .stroke_path(
            &arc(0.0, 2.0 * PI),
            2.4,
            LineCap::Round,
            k.alpha(k.c.text_base, 0.12),
        )
        .stroke_path(
            &arc(top + 2.0 * PI * (1.0 - left), top + 2.0 * PI),
            2.4,
            LineCap::Round,
            p.accent(),
        )
}
