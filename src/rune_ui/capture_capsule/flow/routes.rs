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
    format!("Sent to {}", short(to))
}

/// A session's name cut to the capsule's available width.
pub(super) fn short(name: &str) -> String {
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
    routed.route = route;
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

pub(super) fn countdown(routed: &Routed, now: f64) -> Option<String> {
    let left = 3.0 - (now - routed.at);
    (!routed.delivered && left > 0.0).then(|| format!("{} s", left.ceil() as u32))
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
        assert_eq!(done.why.as_deref(), Some("1 s"));
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
