//! The finished capsule's lines, and the routing helpers behind them.
use super::*;

/// The permission helper as the finished capsule draws it: what's needed
/// and why in one line, and the one thing to do about it.
pub(super) fn permission_done(need: CaptureNeed, grant: Grant) -> Done {
    Done {
        no_frame: true,
        ..permission_lines(need, grant)
    }
}

pub(super) fn permission_lines(need: CaptureNeed, grant: Grant) -> Done {
    let name = tr(need.name());
    let why = match need {
        CaptureNeed::ScreenRecording => tr("to see your screen"),
        CaptureNeed::Microphone => tr("to hear you"),
        CaptureNeed::Accessibility => tr("to follow your clicks"),
    };
    match grant {
        Grant::Off => Done {
            why: Some(why.into()),
            ask: Some(tr("Allow").into()),
            ..Done::plain(Lead::Warning, tr_fill("Hex needs {}", name))
        },
        Grant::Asked => Done {
            why: Some(tr("turn Hex on in System Settings").into()),
            ask: Some(tr("Allow").into()),
            ..Done::plain(Lead::Warning, tr_fill("Hex needs {}", name))
        },
        Grant::Restart => Done {
            why: Some(tr("turns on when Hex restarts").into()),
            ask: Some(tr("Restart Hex").into()),
            ..Done::plain(Lead::Warning, tr_fill("{} is on", name))
        },
        Grant::Ready => Done {
            ask: Some(tr("Capture").into()),
            ask_keys: true,
            ..Done::plain(Lead::Check, tr_fill("{} is on", name))
        },
    }
}

/// A closed list that still waits for a pick: a guess, or one offered
/// again after an Undo; a rule's was sent, as was one too late to take
/// back, and an unreachable one lists nothing.
pub(super) fn waits(routed: Routed) -> Option<Routed> {
    let (suggested, recent) = sections(&routed.route);
    (!matches!(routed.route, Route::Rule { .. })
        && routed.already_with.is_none()
        && !(suggested.is_empty() && recent.is_empty()))
    .then_some(Routed {
        open: None,
        ..routed
    })
}

pub(super) fn hold_until(phase: &Phase) -> Option<f64> {
    match phase {
        Phase::Routed(r)
            if !r.delivered
                && !r.taken_back
                && matches!(r.route, Route::Rule { .. } | Route::ResolvingRule { .. }) =>
        {
            None
        }
        Phase::Saved { at } => Some(at + tuning::get().done_hold_s),
        Phase::Queued { at, .. } | Phase::TooLate { at, .. } => {
            Some(at + tuning::get().brief_hold_s)
        }
        // A send leaves once its Undo window ends (a send with none, and
        // a queued one, after the brief hold).
        Phase::Sent(sent) if sent.undoable => Some(sent.at + undo_s()),
        Phase::Sent(sent) => Some(sent.at + tuning::get().brief_hold_s),
        // The suggestions leave like any finished capsule, and so does a
        // take-back once folded (from when it folded); a search, or a
        // take-back's open list, stays.
        Phase::Routed(Routed {
            route: Route::Rule { .. },
            at,
            open: None,
            taken_back: false,
            ..
        }) => Some(at + undo_s()),
        // Too late to take back, folded: it's where it went, so it leaves
        // as the other settled sends do.
        Phase::Routed(Routed {
            at,
            open: None,
            already_with: Some(_),
            ..
        }) => Some(at + tuning::get().brief_hold_s),
        Phase::Routed(Routed {
            at,
            open,
            taken_back,
            ..
        }) if open.as_ref().is_none_or(|open| !open.typing && !taken_back) => {
            // The menu-bar flow's hint is brief: the capture waits in the
            // menu bar.
            let hint = open.is_some() && tuning::get().flow == CapsuleFlow::MenuBar;
            Some(
                at + if hint {
                    MENU_HINT_S
                } else {
                    tuning::get().done_hold_s
                },
            )
        }
        Phase::Kept { at } | Phase::TakenBack { at } | Phase::Failed { at, .. } => {
            Some(at + tuning::get().brief_hold_s)
        }
        Phase::Permission {
            grant: Grant::Ready,
            at,
            ..
        } => Some(at + tuning::get().done_hold_s),
        // Waiting on Kit: it stays a good while, and ✕ closes it.
        Phase::Permission { at, .. } => Some(at + PERMISSION_HOLD_S),
        _ => None,
    }
}

/// Backspace in the search: deletes the selection, else the character
/// before the caret. Keys come through Hex's tap, not Rune's key events.
pub(super) fn backspace(edit: &mut TextEdit) {
    let (start, end) = edit.range();
    let back = edit.text[..start].chars().last().map_or(0, char::len_utf8);
    if start == end {
        edit.set_selection(start - back, start);
    }
    edit.insert("");
}

/// The list for `route`: its suggestions, then the newest sessions, and
/// searching, every session with each word of the query in its title,
/// folder, or id, those with every word in the title first.
pub(super) fn list(route: &Route, open: &Open) -> List {
    let (suggested, recent) = sections(route);
    let site = match route {
        Route::Guess { site, .. } | Route::Rule { site, .. } => site.clone(),
        _ => None,
    };
    let always = site.map(|site| (site, open.shift));
    let every = || suggested.iter().chain(recent);
    let mut list = List {
        rows: matching(every(), &open.query.text),
        title: if open.typing {
            tr("All sessions")
        } else {
            tr("Send to")
        },
        // Searching keeps the suggestions' height, so the footer is all
        // that comes in.
        min_rows: if open.typing {
            every().take(SUGGEST_ROWS).count()
        } else {
            0
        },
        selected: open.selected,
        footer: if open.typing {
            Footer::Search {
                remember: always.is_some(),
            }
        } else {
            Footer::Suggesting
        },
        always,
    };
    // Shift down, the highlighted row is where the rule would send it.
    if list.always.as_ref().is_some_and(|(_, shift)| *shift)
        && let Some(row) = list.rows.get_mut(list.selected)
    {
        row.why = tr("always").into();
    }
    list
}

/// The first [`SUGGEST_ROWS`] of `dests` that match `query`, in order but
/// title matches first. A row names its folder beside its reason when its
/// title alone doesn't say why it matched, or reads like another row's.
fn matching<'a>(dests: impl Iterator<Item = &'a Dest>, query: &str) -> Vec<Dest> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<(bool, &Dest)> = dests
        .filter_map(|dest| {
            let title = dest.name.to_lowercase();
            let folder = dest.folder.as_deref().unwrap_or_default().to_lowercase();
            let id = dest.session.to_lowercase();
            let in_title = words.iter().all(|word| title.contains(word));
            let found = in_title
                || words.iter().all(|word| {
                    [&title, &folder, &id]
                        .iter()
                        .any(|text| text.contains(word))
                });
            found.then_some((in_title, dest))
        })
        .collect();
    found.sort_by_key(|(in_title, _)| !in_title);
    found.truncate(SUGGEST_ROWS);
    let alike = |dest: &Dest| {
        found
            .iter()
            .filter(|(_, other)| other.name.to_lowercase() == dest.name.to_lowercase())
            .count()
            > 1
    };
    found
        .iter()
        .map(|&(in_title, dest)| match &dest.folder {
            Some(folder) if !in_title || alike(dest) => Dest {
                why: if dest.why.is_empty() {
                    folder.clone()
                } else {
                    format!("{folder} · {}", dest.why)
                },
                ..dest.clone()
            },
            _ => dest.clone(),
        })
        .collect()
}

pub(super) fn clamp(routed: &mut Routed) {
    let Some(open) = &routed.open else { return };
    let count = list(&routed.route, open).rows().count();
    if let Some(open) = &mut routed.open {
        open.selected = open.selected.min(count.saturating_sub(1));
    }
}

pub(super) fn send(
    routed: Routed,
    row: Option<usize>,
    always: bool,
    now: f64,
    actions: &mut Vec<Action>,
) -> Phase {
    let Some(open) = &routed.open else {
        return Phase::Routed(routed);
    };
    let list = list(&routed.route, open);
    let Some(dest) = row.and_then(|row| list.rows().nth(row)) else {
        return Phase::Routed(routed);
    };
    // The rule's own session already has it: the list just closes.
    if !(always || open.shift)
        && rule_to(&routed.route).is_some_and(|to| to.session == dest.session)
    {
        return Phase::Routed(Routed {
            open: None,
            ..routed
        });
    }
    let site = match (&routed.route, always || open.shift) {
        (Route::Guess { site, .. } | Route::Rule { site, .. }, true) => site.clone(),
        _ => None,
    };
    actions.push(Action::Send {
        to: dest.session.clone(),
        always: site.clone(),
    });
    Phase::Sent(Sent::picked(
        dest,
        site.is_some(),
        now,
        Some(routed.route.clone()),
    ))
}

/// Where a rule sent it.
pub(super) fn rule_to(route: &Route) -> Option<&Dest> {
    match route {
        Route::Rule { to, .. } => Some(to),
        _ => None,
    }
}

/// The open list again after an Undo of the send to `from`: the same
/// sessions as a guess, `from` once, last of all. Taken back, nothing is
/// sent; too late, `from` keeps it. `None` when there's nothing to list.
pub(super) fn offered_again(route: Route, from: &Dest, too_late: bool, now: f64) -> Option<Routed> {
    let (mut suggested, mut recent, site) = match route {
        Route::Rule {
            suggested,
            recent,
            site,
            ..
        }
        | Route::Guess {
            suggested,
            recent,
            site,
        } => (suggested, recent, site),
        Route::Nothing { recent } | Route::NoAnswer { recent } => (Vec::new(), recent, None),
        Route::Unreachable { .. } | Route::ResolvingRule { .. } => return None,
    };
    // A rule's session leads its suggestions and may be among the recent
    // ones too: it is listed once, at the end of the whole list.
    let mut last = None;
    for rows in [&mut suggested, &mut recent] {
        rows.retain(|dest| {
            let taken = dest.session == from.session;
            if taken && last.is_none() {
                last = Some(dest.clone());
            }
            !taken
        });
    }
    if let Some(dest) = last {
        let rows = if recent.is_empty() {
            &mut suggested
        } else {
            &mut recent
        };
        rows.push(Dest {
            why: if too_late {
                tr("already sent")
            } else {
                tr("taken back")
            }
            .into(),
            ..dest
        });
    }
    if suggested.is_empty() && recent.is_empty() {
        return None;
    }
    Some(Routed {
        route: Route::Guess {
            suggested,
            recent,
            site,
        },
        at: now,
        open: Some(Open::suggesting()),
        taken_back: true,
        already_with: too_late.then(|| from.name.clone()),
        taken_from: Some(from.clone()),
        delivered: false,
    })
}

/// What the capsule shows the moment an Undo of the send to `from` is
/// asked for: the list again, never gone, whatever the router answers.
pub(super) fn taking_back(route: Option<Route>, from: &Dest, now: f64) -> Phase {
    Phase::Routed(
        route
            .and_then(|route| offered_again(route, from, false, now))
            .unwrap_or_else(|| lone_back(from, now)),
    )
}

/// Taken back with no other session to offer: the list still comes back,
/// with `from` alone, so Search can still send it somewhere.
pub(super) fn lone_back(from: &Dest, now: f64) -> Routed {
    Routed {
        route: Route::Nothing {
            recent: vec![Dest {
                why: tr("taken back").into(),
                ..from.clone()
            }],
        },
        at: now,
        open: Some(Open::suggesting()),
        taken_back: true,
        already_with: None,
        taken_from: Some(from.clone()),
        delivered: false,
    }
}

pub(super) fn routed_done(routed: &Routed, now: f64) -> Done {
    let undo = now - routed.at < undo_s();
    let countdown = undo.then(|| routed.at + undo_s());
    let base = match &routed.route {
        // Sent nowhere, it waits for a pick: a click on Send to… reopens the list.
        _ if routed.taken_back => Done {
            // Too late, it's where it went: nothing to offer.
            ask: routed.already_with.is_none().then(|| tr("Send to…").into()),
            ..Done::plain(
                Lead::Check,
                match &routed.already_with {
                    Some(to) => already_sent_to(to),
                    None => tr("Taken back").into(),
                },
            )
        },
        Route::Rule { to, .. } => Done {
            undo: undo || !routed.delivered,
            countdown,
            ..Done::plain(
                Lead::Check,
                if routed.delivered {
                    sent_to(&to.name)
                } else {
                    tr_fill("Sending to {}", &short(&to.name))
                },
            )
        },
        Route::ResolvingRule { name } => Done {
            undo: true,
            countdown,
            ..Done::plain(Lead::Check, tr_fill("Sending to {}", &short(name)))
        },
        // The suggestions are the ask (closed, Send to… opens them).
        Route::Guess { .. } => Done {
            ask: Some(tr("Send to…").into()),
            ..Done::plain(Lead::Check, "")
        },
        Route::Nothing { .. } => Done {
            ask: Some(tr("Send to…").into()),
            ..Done::plain(Lead::Check, "")
        },
        Route::NoAnswer { .. } => Done {
            why: Some(tr("no suggestions").into()),
            ask: Some(tr("Send to…").into()),
            ..Done::plain(Lead::Check, copied())
        },
        // Still copied, so Paste and Inspect stay; there's nothing to list.
        Route::Unreachable { why } => match plain_reason(why) {
            Some(why) if why == not_running() => Done::plain(Lead::Warning, not_running()),
            why => Done {
                why,
                ..Done::plain(Lead::Warning, tr("Couldn't reach OpenCode"))
            },
        },
    };
    match &routed.open {
        // The suggestions hang under a finished capsule that keeps Paste
        // and Inspect, and its note.
        Some(open) if !open.typing => offered(base, list(&routed.route, open)),
        // Searching, the field takes the chips' place.
        Some(open) => Done {
            why: None,
            undo: false,
            countdown: None,
            ask: None,
            open: Some((*open.query.clone(), list(&routed.route, open))),
            ..base
        },
        None => Done {
            hand_off: true,
            ..base
        },
    }
}

/// The suggestions as the flow in use offers them (`capture.capsule_flow`).
pub(super) fn offered(base: Done, list: List) -> Done {
    let names = || {
        list.rows()
            .map(|dest| (short(&dest.name), dest.why.clone()))
            .collect()
    };
    match tuning::get().flow {
        // Just Search and its shortcut beside "Copied": a click on a row
        // sends, and the capture is already on the clipboard to paste.
        CapsuleFlow::List => Done {
            text: if base.text.is_empty() {
                copied().into()
            } else {
                base.text.clone()
            },
            ask: Some(tr("Search").into()),
            ask_keys: true,
            open: Some((TextEdit::default(), list)),
            hand_off: false,
            ..base
        },
        // One guess at a time on one quiet line; ⏎ sends it, ⇥ moves on.
        CapsuleFlow::Toast => Done {
            text: tr_fill(
                "Send to {}",
                &list
                    .selected_dest()
                    .map(|dest| short(&dest.name))
                    .unwrap_or_default(),
            ),
            why: Some("⏎".into()),
            ask: None,
            cycle: list.rows().count() > 1,
            sends: true,
            ..base
        },
        // The frame alone, then the guesses as chips.
        CapsuleFlow::MenuBar => Done {
            text: String::new(),
            why: None,
            ask: None,
            picks: names(),
            ..base
        },
        CapsuleFlow::Cursor | CapsuleFlow::Notch => Done {
            text: tr("Send to").into(),
            why: None,
            ask: None,
            picks: names(),
            ..base
        },
    }
}
