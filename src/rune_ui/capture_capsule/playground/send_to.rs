//! Prototypes for t1057, "send to any session, with search": two ways the
//! capsule's list could reach every OpenCode session and remember a pick,
//! drawn from the capsule's own pieces on fake sessions. Nothing here runs
//! in the real capsule until one is picked.
//!
//! - A, type to search: the field takes typing from the start and searches
//!   every session; Shift (with a number, a click or ⏎) also remembers.
//! - B, remember switch: the same search, but remembering is a switch in
//!   the footer (Tab or a click), so a pick never needs Shift.
//!
//! Both play Kit's case: a capture made in OpenCode (Rune) while on RUNE
//! DESKTOP, remembered, and sent there by itself next time.

use rune::{Cx, El, OverlaySpec, Play, SpringPair, Story, Transition, canvas, el, row};

use super::super::surface::{
    CHIP_H, HEADER_H, ISLAND_H, LIST_PAD, LIST_W, LIST_Y, MEDIUM, PANEL_H, PANEL_W, ROW_H,
    SHELL_RADIUS, SMALL, TOP, ellipsize, measure,
};
use super::super::{
    BASELINE_DROP, Badge, Id, NUMBER, NUMBER_W, PRESENCE_BLUR, ROW_PX, ROW_RADIUS, badge,
    footer_box, footer_height, hint, icon, label, rim, tuning,
};
use crate::rune_ui::controls::Paint;

/// The app the capture was made in, which a remembered pick is for.
const APP: &str = "OpenCode (Rune)";

/// Every session, newest first: what typing searches.
const SESSIONS: &[(&str, &str, &str)] = &[
    ("RUNE DESKTOP", "seer/rune-desktop", "now"),
    ("SEER", "seer", "4 min"),
    ("RUNE", "rune", "12 min"),
    ("HOME", "home", "1 h"),
    ("WISP", "wisp", "2 h"),
    ("WISP DEV", "wisp/dev", "3 h"),
    ("ABATTOIR", "abattoir", "5 h"),
    ("HEX", "hex", "6 h"),
    ("DEADBOLT", "deadbolt", "yesterday"),
    ("INVOKE", "invoke", "2 d"),
    ("OPENCODE IOS", "opencode-ios", "3 d"),
    ("RUNE GOLDENS", "rune/goldens", "5 d"),
];

/// The guesses a capture in OpenCode (Rune) on RUNE DESKTOP opens with.
const SUGGESTED: &[(&str, &str)] = &[
    ("RUNE DESKTOP", "you're in it"),
    ("SEER", "same checkout"),
    ("RUNE", "12 min ago"),
    ("HOME", "an hour ago"),
];

/// Typing shows at most this many sessions.
const MAX_FOUND: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Flow {
    #[default]
    Search,
    Switch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    /// The list is open on the suggestions, or on what's typed.
    #[default]
    Pick,
    /// Sent and remembered.
    Sent,
    /// The next capture in the same app: the remembered pick sends itself.
    Next,
}

#[derive(Clone, Debug, Default)]
pub(super) struct SendTo {
    flow: Flow,
    step: Step,
    query: String,
    selected: usize,
    /// A: Shift is down. B: the switch is on.
    remember: bool,
}

/// `playground` with both flows as stories, each step a variant.
pub(super) fn add<S: 'static>(playground: rune::Playground<S>) -> rune::Playground<S> {
    playground
        .story(story(
            "Prototypes/Capture capsule/Send to A",
            Flow::Search,
            &[
                ("Suggestions", |_| {}),
                ("Hold Shift", |s| s.remember = true),
                ("Search", |s| s.query = "rune".into()),
                ("Search, Shift", |s| {
                    s.query = "rune".into();
                    s.remember = true;
                }),
                ("Sent", |s| s.step = Step::Sent),
                ("Next capture", |s| s.step = Step::Next),
            ],
        ))
        .story(story(
            "Prototypes/Capture capsule/Send to B",
            Flow::Switch,
            &[
                ("Suggestions", |_| {}),
                ("Switch on", |s| s.remember = true),
                ("Search", |s| s.query = "rune".into()),
                ("Sent", |s| s.step = Step::Sent),
                ("Next capture", |s| s.step = Step::Next),
            ],
        ))
}

type Variant = (&'static str, fn(&mut SendTo));

fn story(name: &'static str, flow: Flow, variants: &[Variant]) -> Story<SendTo> {
    let frame = OverlaySpec::new("Hex capture", (PANEL_W, PANEL_H), view);
    let mut story = Story::overlay(name, &frame, move || SendTo {
        flow,
        ..SendTo::default()
    })
    .trigger('p', "Play", play);
    for &(name, apply) in variants {
        story = story.variant(name, apply);
    }
    story
}

/// Kit's case end to end: the suggestions, a search, a remembered pick,
/// and the next capture sending itself.
fn play(s: &mut SendTo, play: &mut Play<'_, SendTo>) {
    let flow = s.flow;
    *s = SendTo {
        flow,
        ..SendTo::default()
    };
    let shift = flow == Flow::Search;
    if shift {
        play.after(1.0, |s, _| s.remember = true);
        play.after(1.9, |s, _| s.remember = false);
    }
    for (i, typed) in ["r", "ru", "run", "rune"].into_iter().enumerate() {
        play.after(2.5 + 0.14 * i as f64, move |s, _| s.query = typed.into());
    }
    play.after(3.6, |s, _| s.remember = true);
    play.after(4.6, |s, _| {
        s.step = Step::Sent;
        s.query.clear();
        if s.flow == Flow::Search {
            s.remember = false;
        }
    });
    play.after(6.6, |s, _| s.step = Step::Next);
}

fn view(s: &SendTo, cx: &mut Cx<SendTo>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let x = (PANEL_W - LIST_W) * 0.5;
    let mut stage = el()
        .key("send-to")
        .size(PANEL_W, PANEL_H)
        .child(island(p, s).absolute(x, TOP));
    if s.step == Step::Pick {
        stage = stage.child(list(p, s, x));
    }
    stage
}

/// The capsule: the check, then the field while picking, or what was sent.
fn island(p: Paint, s: &SendTo) -> El {
    let k = p.k;
    let check = canvas(icon(Id::Done, 16.0 * 2.7 / 24.0, p.success())).size(16.0, 16.0);
    let mut shell = row()
        .key("island")
        .size(LIST_W, ISLAND_H)
        .center_y()
        .gap(8.0)
        .px(12.0)
        .radius(SHELL_RADIUS)
        .fill(k.c.bg_base)
        .child(check);
    for shadow in &k.elevation.overlay {
        shell = shell.box_shadow(*shadow);
    }
    let fade = Transition::opacity().combined(Transition::blur(PRESENCE_BLUR));
    shell = match s.step {
        Step::Pick => shell
            .child(label("Copied", SMALL, p.muted()))
            .child(field(p, s).grow(1.0).transition(fade))
            .child(canvas(icon(Id::Close, 14.0 * 2.2 / 24.0, p.muted())).size(14.0, 14.0)),
        Step::Sent | Step::Next => {
            let why = if s.step == Step::Sent {
                "· remembered"
            } else {
                "· your pick for this app"
            };
            shell
                .child(
                    label("Sent to", MEDIUM, p.muted())
                        .key("sent")
                        .transition(fade),
                )
                .child(label("RUNE DESKTOP", MEDIUM, p.text()).key("to"))
                .child(label(why, SMALL, p.faint()).key(why).transition(fade))
                .child(el().grow(1.0))
                .child(label("Undo", SMALL, p.muted()).key("undo"))
        }
    };
    if k.dark {
        shell = shell.child(rim(k, LIST_W, ISLAND_H, SHELL_RADIUS));
    }
    shell
}

/// The field: what's typed and the caret, or what typing does.
fn field(p: Paint, s: &SendTo) -> El {
    let caret = el()
        .key("caret")
        .size(1.5, 15.0)
        .radius(0.75)
        .fill(p.accent());
    let inner = if s.query.is_empty() {
        row().gap(1.0).center_y().child(caret).child(label(
            "Search all sessions",
            MEDIUM,
            p.faint(),
        ))
    } else {
        row()
            .gap(1.0)
            .center_y()
            .child(label(&s.query, MEDIUM, p.text()))
            .child(caret)
    };
    row()
        .key("field")
        .h(CHIP_H - 4.0)
        .px(10.0)
        .center_y()
        .radius(8.0)
        .fill(p.k.c.layer_01)
        .child(inner)
}

/// One row: what to show, and whether it's numbered.
struct Line {
    name: &'static str,
    why: String,
    number: Option<usize>,
}

fn lines(s: &SendTo) -> (&'static str, Vec<Line>) {
    if s.query.is_empty() {
        let rows = SUGGESTED
            .iter()
            .enumerate()
            .map(|(i, (name, why))| Line {
                name,
                why: (*why).into(),
                number: Some(i + 1),
            })
            .collect();
        return ("Send to", rows);
    }
    let query = s.query.to_lowercase();
    let rows = SESSIONS
        .iter()
        .filter(|(name, dir, _)| name.to_lowercase().contains(&query) || dir.contains(&query))
        .take(MAX_FOUND)
        .map(|(name, dir, age)| Line {
            name,
            why: format!("{dir} · {age}"),
            number: None,
        })
        .collect();
    ("All sessions", rows)
}

fn list(p: Paint, s: &SendTo, x: f32) -> El {
    let k = p.k;
    let w = LIST_W;
    let (title, rows) = lines(s);
    let h = LIST_PAD + HEADER_H + rows.len() as f32 * ROW_H + footer_height();
    let t = tuning::get();
    let springs = SpringPair::new(t.content_in, t.content_out);
    let in_place = Transition::blur(PRESENCE_BLUR)
        .combined(Transition::scale(0.96))
        .springs(springs);
    let mut body = rune::col()
        .absolute(0.0, 0.0)
        .w(w)
        .px(LIST_PAD)
        .pt(LIST_PAD)
        .child(
            row()
                .key(format!("section-{title}"))
                .items_end()
                .h(HEADER_H)
                .px(ROW_PX)
                .pb(4.0)
                .child(label(title, k.ty.small_medium, p.faint()))
                .transition(in_place),
        );
    let content = w - 2.0 * LIST_PAD - 2.0 * ROW_PX - NUMBER_W - 3.0 * ROW_PX;
    for (n, line) in rows.iter().enumerate() {
        let marked = s.remember && n == s.selected;
        let why = if marked {
            if s.flow == Flow::Search {
                "remember".to_owned()
            } else {
                "always".to_owned()
            }
        } else {
            line.why.clone()
        };
        let why = ellipsize(
            &why,
            SMALL,
            (content - measure(line.name, MEDIUM)).max(content * 0.4),
        );
        let accent = marked || (n == 0 && s.query.is_empty() && !s.remember);
        let number = line.number.map(|i| i.to_string()).unwrap_or_default();
        let mut entry = row()
            .key(format!("row-{}", line.name))
            .center_y()
            .h(ROW_H)
            .px(ROW_PX)
            .gap(ROW_PX)
            .radius(ROW_RADIUS)
            .child(label(&number, NUMBER, p.faint()).w(NUMBER_W))
            .child(label(line.name, MEDIUM, p.text()))
            .child(el().grow(1.0))
            .child(
                label(&why, SMALL, if accent { p.accent() } else { p.muted() })
                    .offset(0.0, (MEDIUM.size - SMALL.size) * BASELINE_DROP),
            )
            .transition(in_place);
        if n == s.selected {
            entry = entry.fill(k.c.layer_02);
        }
        body = body.child(entry);
    }
    let hints = match s.flow {
        Flow::Search => search_footer(p, s),
        Flow::Switch => switch_footer(p, s),
    };
    let mut panel = el()
        .key("list")
        .absolute(x, LIST_Y)
        .size(w, h)
        .radius(SHELL_RADIUS)
        .fill(k.c.bg_base);
    for shadow in &k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    panel = panel.child(
        el().absolute(0.0, 0.0)
            .size(w, h)
            .radius(SHELL_RADIUS)
            .clip_rounded(SHELL_RADIUS)
            .child(body)
            .child(footer_box(p, hints, w, h)),
    );
    if k.dark {
        panel = panel.child(rim(k, w, h, SHELL_RADIUS));
    }
    panel.motion(t.island)
}

/// "Remember [▭ OpenCode (Rune)] → [💬 RUNE DESKTOP]": what a pick saves.
fn remembering(p: Paint, lead: &str, s: &SendTo) -> El {
    let to = lines(s)
        .1
        .get(s.selected)
        .map(|line| line.name)
        .unwrap_or("…");
    row()
        .key("remembering")
        .center_y()
        .gap(6.0)
        .child(label(lead, SMALL, p.muted()))
        .child(badge(p, Badge::App, APP))
        .child(label("→", SMALL, p.muted()))
        .child(badge(p, Badge::Chat, to))
        .child(el().grow(1.0))
}

/// A's footer, in words rather than keycaps: what typing does and what
/// Shift adds; Shift down, what it will remember.
fn search_footer(p: Paint, s: &SendTo) -> El {
    if s.remember {
        return remembering(p, "Remember", s);
    }
    if s.query.is_empty() {
        return row()
            .key("plain")
            .center_y()
            .child(label("Type to search all sessions", SMALL, p.muted()))
            .child(el().grow(1.0))
            .child(label("Hold ⇧ to remember", SMALL, p.faint()));
    }
    row()
        .key("keys")
        .center_y()
        .gap(16.0)
        .child(hint(p, "Send", "⏎"))
        .child(hint(p, "Send and remember", "⇧⏎"))
        .child(el().grow(1.0))
}

/// B's footer: a switch for remembering, on Tab or a click.
fn switch_footer(p: Paint, s: &SendTo) -> El {
    let k = p.k;
    let on = s.remember;
    let toggle = el()
        .key("switch")
        .size(24.0, 14.0)
        .radius(7.0)
        .fill(if on {
            k.c.text_accent
        } else {
            k.alpha(k.c.text_base, 0.16)
        })
        .child(
            el().key("knob")
                .absolute(if on { 12.0 } else { 2.0 }, 2.0)
                .size(10.0, 10.0)
                .radius(5.0)
                .fill(k.c.text_base)
                .motion(tuning::get().island),
        );
    let words = if on {
        remembering(p, "Always", s)
    } else {
        row()
            .center_y()
            .child(label(&format!("Always send {APP} here"), SMALL, p.muted()))
            .child(el().grow(1.0))
    };
    row()
        .center_y()
        .gap(8.0)
        .child(toggle)
        .child(words.grow(1.0))
        .children((!on).then(|| hint(p, "", "tab")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_searches_every_session_not_just_the_suggestions() {
        let s = SendTo {
            query: "dead".into(),
            ..SendTo::default()
        };
        let (_, rows) = lines(&s);
        assert_eq!(
            rows.iter().map(|l| l.name).collect::<Vec<_>>(),
            ["DEADBOLT"]
        );
        assert!(rows.iter().all(|l| l.number.is_none()), "digits type");
    }
}
