//! In-place routing updates preserve a search, its selection and user actions.
use super::*;

fn pending_name(route: &Route) -> Option<String> {
    match route {
        Route::Rule { to, .. } => Some(to.name.clone()),
        Route::ResolvingRule { name } => Some(name.clone()),
        _ => None,
    }
}

pub(super) fn undoable(phase: &Phase, now: f64) -> Option<Action> {
    if let Phase::Routed(r) = phase
        && !r.delivered
        && !r.taken_back
        && let Some(to) = pending_name(&r.route)
    {
        return Some(Action::Undo { to });
    }
    let (to, at) = match phase {
        Phase::Routed(Routed {
            route: Route::Rule { to, .. },
            at,
            ..
        }) => (&to.name, *at),
        Phase::Sent(Sent {
            to,
            at,
            undoable: true,
            ..
        }) => (to, *at),
        _ => return None,
    };
    (now - at < undo_s()).then(|| Action::Undo { to: to.clone() })
}

pub(super) fn sent_to(to: &str) -> String {
    tr_fill("Sent to {}", &short(to))
}

/// A session's name cut to the capsule's available width. A raw `ses…` id
/// (a host that named a session by id) reads "session": never the id.
pub(super) fn short(name: &str) -> String {
    if name.trim().starts_with("ses_") {
        return tr("session").to_owned();
    }
    if name.chars().count() <= ROUTE_NAME_MAX {
        return name.to_owned();
    }
    let cut: String = name.chars().take(ROUTE_NAME_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

pub(super) fn refine(mut routed: Routed, route: Route, _now: f64, suggest: bool) -> Routed {
    let resolving = matches!(routed.route, Route::ResolvingRule { .. });
    let selected = routed.open.as_ref().and_then(|open| {
        list(&routed.route, open)
            .rows
            .get(open.selected)
            .map(|d| d.session.clone())
    });
    routed.route = keep_rows(&routed.route, route);
    if resolving && suggest && matches!(routed.route, Route::Guess { .. }) {
        routed.open = Some(Open::suggesting());
    }
    if let Some(open) = &mut routed.open {
        let rows = list(&routed.route, open).rows;
        open.selected = selected
            .and_then(|id| rows.iter().position(|d| d.session == id))
            .unwrap_or(0);
    }
    routed
}

/// `next` with every session `shown` listed kept in its place, since the
/// pointer may be on it: a row takes its new name and reason (its old
/// reason when the new one is blank), no row leaves, and sessions new to
/// the list come last, a rule's own session too. The router keeps its
/// rows in place as well; this holds whatever answer reaches the capsule.
fn keep_rows(shown: &Route, next: Route) -> Route {
    let (above, below) = sections(shown);
    let (suggested, recent) = sections(&next);
    let fresh: Vec<&Dest> = suggested.iter().chain(recent).collect();
    if above.is_empty() && below.is_empty() || fresh.is_empty() {
        return next;
    }
    let was = |dest: &Dest| {
        above
            .iter()
            .chain(below)
            .any(|old| old.session == dest.session)
    };
    let latest = |old: &Dest| match fresh.iter().find(|new| new.session == old.session) {
        Some(new) if new.why.is_empty() => Dest {
            why: old.why.clone(),
            ..(*new).clone()
        },
        Some(new) => (*new).clone(),
        None => old.clone(),
    };
    let suggested: Vec<Dest> = above.iter().map(latest).collect();
    let mut recent: Vec<Dest> = below.iter().map(latest).collect();
    let mut added = Vec::new();
    for new in fresh {
        if !was(new) && !added.iter().any(|d: &Dest| d.session == new.session) {
            added.push(new.clone());
        }
    }
    recent.extend(added);
    match next {
        Route::Rule { to, site, .. } => Route::Rule {
            to,
            suggested,
            recent,
            site,
        },
        Route::Guess { site, .. } => Route::Guess {
            suggested,
            recent,
            site,
        },
        Route::NoAnswer { .. } => Route::NoAnswer {
            recent: suggested.into_iter().chain(recent).collect(),
        },
        _ => Route::Nothing {
            recent: suggested.into_iter().chain(recent).collect(),
        },
    }
}

pub(super) fn sections(route: &Route) -> (&[Dest], &[Dest]) {
    match route {
        Route::Rule {
            suggested, recent, ..
        }
        | Route::Guess {
            suggested, recent, ..
        } => (suggested, recent),
        Route::Nothing { recent } | Route::NoAnswer { recent } => (&[], recent),
        Route::Unreachable { .. } | Route::ResolvingRule { .. } => (&[], &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guess() -> Route {
        Route::Guess {
            suggested: vec![
                Dest::new("Board", "").session("ses_board"),
                Dest::new("Home", "").session("ses_home"),
            ],
            recent: Vec::new(),
            site: None,
        }
    }

    #[test]
    fn refinements_preserve_the_search_and_selected_session() {
        let mut flow = Flow::new();
        flow.input(Input::Routed(guess()), 0.0);
        flow.input(Input::OpenList, 0.1);
        flow.input(Input::Type("Home".into()), 0.2);
        let before = flow.pres(0.2);
        let mut route = guess();
        if let Route::Guess { suggested, .. } = &mut route {
            suggested.reverse();
        }
        flow.input(Input::Refined(route), 2.0);
        assert_eq!(flow.pres(2.0), before);
    }

    /// Kit: "the suggested places still rearrange themselves". Nothing
    /// that happens while the list shows moves a row.
    #[test]
    fn no_shown_row_ever_moves() {
        let rows = |flow: &Flow| -> Vec<(String, String)> {
            let Pres::Done(done) = flow.pres(5.0) else {
                panic!("missing capsule");
            };
            let (_, list) = done.open.expect("the list shows");
            list.rows()
                .map(|dest| (dest.session.clone(), dest.why.clone()))
                .collect()
        };
        let sessions = |flow: &Flow| -> Vec<String> {
            rows(flow).into_iter().map(|(session, _)| session).collect()
        };
        let guess_of = |suggested: Vec<Dest>, recent: Vec<Dest>| Route::Guess {
            suggested,
            recent,
            site: Some("example.com".into()),
        };
        let mut flow = Flow::new();
        flow.input(
            Input::Routed(guess_of(
                vec![Dest::new("Board", "same page").session("ses_board")],
                vec![
                    Dest::new("Home", "recent").session("ses_home"),
                    Dest::new("Hex", "recent").session("ses_hex"),
                ],
            )),
            0.0,
        );
        let shown = sessions(&flow);
        assert_eq!(shown, ["ses_board", "ses_home", "ses_hex"]);
        flow.point(1, true);
        flow.input(Input::Shift(true), 0.2);
        flow.input(Input::Shift(false), 0.3);
        assert_eq!(sessions(&flow), shown, "pointing and Shift");
        // Jev's pick replaces the answer: Hex on top, the recent ones
        // re-sorted by their latest activity, and one more session.
        flow.input(
            Input::Refined(guess_of(
                vec![Dest::new("Hex", "mentioned").session("ses_hex")],
                vec![
                    Dest::new("Rune", "recent").session("ses_rune"),
                    Dest::new("Board", "recent").session("ses_board"),
                    Dest::new("Home", "").session("ses_home"),
                ],
            )),
            0.4,
        );
        assert_eq!(
            rows(&flow),
            [
                ("ses_board".into(), "recent".into()),
                ("ses_home".into(), "recent".into()),
                ("ses_hex".into(), "mentioned".into()),
                ("ses_rune".into(), "recent".into()),
            ],
            "rows keep their place and take their new reason; a new one comes last"
        );
        let shown = sessions(&flow);
        flow.input(Input::OpenList, 0.5);
        assert_eq!(sessions(&flow), shown, "the search opens on the same rows");
        flow.input(Input::Escape, 0.6);
        assert_eq!(sessions(&flow), shown, "and goes back to them");
        flow.input(
            Input::Refined(guess_of(
                vec![Dest::new("Rune", "mentioned").session("ses_rune")],
                Vec::new(),
            )),
            0.7,
        );
        assert_eq!(sessions(&flow), shown, "a row never leaves");
    }

    #[test]
    fn a_late_answer_cannot_replace_a_pick_or_undo() {
        for undo in [false, true] {
            let mut flow = Flow::new();
            flow.input(Input::Routed(guess()), 0.0);
            flow.input(Input::SendRow(0), 0.1);
            if undo {
                flow.input(Input::Undo, 0.2);
            }
            let before = flow.pres(0.3);
            flow.input(Input::Refined(guess()), 0.3);
            assert_eq!(flow.pres(0.3), before);
        }
    }

    #[test]
    fn resolving_a_folder_does_not_restart_its_countdown_and_can_be_cancelled() {
        let mut flow = Flow::new();
        flow.input(
            Input::Routed(Route::ResolvingRule {
                name: "board".into(),
            }),
            0.0,
        );
        assert_eq!(
            flow.undoable(10.0),
            Some(Action::Undo { to: "board".into() })
        );
        flow.input(
            Input::Refined(Route::Rule {
                to: Dest::new("Board", "").session("ses_board"),
                suggested: Vec::new(),
                recent: Vec::new(),
                site: None,
            }),
            2.0,
        );
        let Pres::Done(done) = flow.pres(2.0) else {
            panic!("missing capsule");
        };
        assert_eq!(done.countdown, Some(undo_s()));
        let mut unresolved = Flow::new();
        unresolved.input(
            Input::Routed(Route::ResolvingRule {
                name: "board".into(),
            }),
            0.0,
        );
        assert_eq!(
            unresolved.input(Input::Undo, 0.1),
            vec![Action::Undo { to: "board".into() }]
        );
        let before = unresolved.pres(0.1);
        unresolved.input(Input::Refined(guess()), 0.2);
        assert_eq!(unresolved.pres(0.2), before);
    }
}
