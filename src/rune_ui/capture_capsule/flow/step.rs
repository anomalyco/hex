//! How the capsule moves on an input from the host or the keys.
use super::*;

impl Flow {
    pub(super) fn step(&mut self, input: Input, now: f64) -> Vec<Action> {
        let mut actions = Vec::new();
        if input == Input::NothingHeard {
            self.silent = true;
            return actions;
        }
        if let Input::Drawing(on) = input {
            self.drawing = on;
            return actions;
        }
        if input == Input::Stage(Stage::Recording) {
            self.silent = false;
            if !matches!(self.phase, Phase::Recording { .. }) {
                self.drawing = false;
            }
        }
        if matches!(input, Input::Stage(_)) {
            self.closed = false;
            self.waiting = None;
            self.settled = false;
            self.undoing = false;
            self.superseded = false;
        }
        if let Input::Undone(undone) = input {
            // The earlier send's, taken back before the pick went.
            if std::mem::take(&mut self.superseded) {
                return actions;
            }
            self.undoing = false;
            // Too late, it stays where it went.
            self.settled |= undone == Undone::TooLate;
        }
        // The send an Undo takes back, arriving late (it went, queued, or
        // failed), changes nothing: the list it reopened stays.
        if self.undoing
            && matches!(
                input,
                Input::Settled | Input::Delivered(_) | Input::Failed(Failure::Send(_), _)
            )
        {
            return actions;
        }
        // News of the earlier send under the pick shown says nothing of it.
        if self.superseded
            && let Phase::Sent(sent) = &self.phase
            && match &input {
                Input::Delivered(Delivered::Sent { to } | Delivered::Queued { to, .. })
                | Input::Failed(Failure::Send(to), _) => *to != sent.to,
                _ => false,
            }
        {
            return actions;
        }
        if matches!(input, Input::Settled | Input::Delivered(_)) {
            self.waiting = None;
            if input == Input::Settled {
                self.settled = true;
                self.hold_floor = self.hold_floor.max(now + tuning::get().brief_hold_s);
                // A rule's send went: it sounds Sent, once.
                if let Phase::Routed(routed) = &mut self.phase
                    && matches!(routed.route, Route::Rule { .. })
                    && !std::mem::replace(&mut routed.delivered, true)
                {
                    self.cues.push(Cue::Sent);
                }
                return actions;
            }
        }
        match input {
            // A capture finishing without its screen: its capsule says
            // where it went first.
            Input::NeedsPermission(need, asked_by)
                if matches!(
                    self.phase,
                    Phase::Processing
                        | Phase::Saved { .. }
                        | Phase::Routed(_)
                        | Phase::Sent(_)
                        | Phase::Queued { .. }
                ) =>
            {
                self.helper_next = Some((need, asked_by));
                return actions;
            }
            Input::NeedsPermission(need, asked_by) => {
                self.helper_next = None;
                // Pressed again while it's up: it stays as it stands, but
                // the newest ask is what its grant returns to.
                let grant = match &self.phase {
                    Phase::Permission {
                        need: shown, grant, ..
                    } if *shown == need => *grant,
                    _ => Grant::Off,
                };
                self.phase = Phase::Permission {
                    need,
                    grant,
                    at: now,
                    asked_by,
                };
                return actions;
            }
            Input::Permission(need, grant) => {
                if let Phase::Permission {
                    need: shown,
                    grant: was,
                    at,
                    ..
                } = &mut self.phase
                {
                    // Still off once Allow was pressed: it waits on System
                    // Settings.
                    let grant = if grant == Grant::Off && *was == Grant::Asked {
                        Grant::Asked
                    } else {
                        grant
                    };
                    if (*shown, *was) != (need, grant) {
                        *shown = need;
                        *was = grant;
                        *at = now;
                    }
                }
                return actions;
            }
            _ => {}
        }
        if let Input::Phone(phone) = input {
            self.phone = phone;
            if !phone && self.phase == Phase::Selecting(Mode::Iphone) {
                self.phase = Phase::Selecting(Mode::Screen);
            }
            return actions;
        }
        let switching = matches!(input, Input::Stage(_) | Input::Failed(..) | Input::Gone)
            && std::mem::take(&mut self.switching);
        let phase = std::mem::replace(&mut self.phase, Phase::Hidden);
        self.phase = match (phase, input) {
            (_, Input::Stage(stage)) => match stage {
                Stage::Recording => Phase::Recording {
                    chips_until: None,
                    discard_until: None,
                },
                Stage::Stopping | Stage::Transcribing | Stage::Rendering | Stage::Writing => {
                    Phase::Processing
                }
                Stage::Done => Phase::Saved { at: now },
                Stage::Cancelled => Phase::Hidden,
                Stage::Failed => Phase::Failed {
                    at: now,
                    what: Failure::Save,
                    why: None,
                },
            },
            (_, Input::Failed(what, why)) => Phase::Failed {
                at: now,
                what: match what {
                    Failure::Save if switching => Failure::Start,
                    what => what,
                },
                why: why.as_deref().and_then(plain_reason),
            },
            (Phase::Recording { discard_until, .. }, Input::Chips) => Phase::Recording {
                chips_until: Some(now + tuning::get().chips_open_s),
                discard_until,
            },
            (
                Phase::Recording {
                    chips_until,
                    discard_until,
                },
                Input::Worked,
            ) => Phase::Recording {
                chips_until: chips_until.map(|until| {
                    until
                        .min(until - tuning::get().chips_open_s + CHIPS_MIN_S)
                        .max(now)
                }),
                discard_until,
            },
            (Phase::Recording { .. }, Input::Gone) => Phase::Hidden,
            (Phase::Selecting(mode), Input::Record) => {
                actions.push(Action::Record(mode));
                Phase::Selecting(mode)
            }
            // Closed before the router answered (dismissed, or the answer
            // came after the hold): it stays closed, and `hex capture list`
            // and `hex capture send` still offer the capture.
            // A guess still waits for Send to….
            (Phase::Hidden, Input::Routed(route)) if self.closed => {
                if !self.settled {
                    self.waiting = waits(Routed {
                        route,
                        at: now,
                        open: None,
                        taken_back: false,
                        already_with: None,
                        taken_from: None,
                        delivered: false,
                    });
                }
                Phase::Hidden
            }
            (Phase::Hidden, Input::Delivered(_)) if self.closed => Phase::Hidden,
            // Closed with nothing picked: Send to… brings the list
            // back.
            (Phase::Hidden | Phase::Kept { .. }, Input::OpenList) if self.waiting.is_some() => {
                let routed = self.waiting.take().expect("checked");
                self.phase = Phase::Routed(Routed { at: now, ..routed });
                return self.step(Input::OpenList, now);
            }
            // A queued send went on a retry: Hex can't take it back.
            (_, Input::Delivered(delivered)) => match delivered {
                Delivered::Sent { to } => Phase::Sent(Sent {
                    undoable: false,
                    ..Sent::picked(&Dest::new(to, ""), false, now, None)
                }),
                Delivered::Queued { why, .. } => Phase::Queued { why, at: now },
            },
            (Phase::Routed(routed), Input::Refined(route)) if !routed.taken_back => {
                Phase::Routed(routes::refine(routed, route, now, self.suggest_on_route))
            }
            (_, Input::Routed(route)) => {
                let suggests = self.suggest_on_route
                    && !matches!(
                        route,
                        Route::Rule { .. }
                            | Route::ResolvingRule { .. }
                            | Route::Unreachable { .. }
                    )
                    && sections(&route) != (&[][..], &[][..]);
                Phase::Routed(Routed {
                    route,
                    at: now,
                    open: suggests.then(Open::suggesting),
                    taken_back: false,
                    already_with: None,
                    taken_from: None,
                    delivered: false,
                })
            }
            (Phase::Routed(routed), Input::OpenList)
                if matches!(routed.route, Route::Unreachable { .. }) =>
            {
                Phase::Routed(routed)
            }
            // The capture shortcut over the suggestions searches every
            // session; in the search it records anew, which the listener
            // does itself.
            (Phase::Routed(routed), Input::Record)
                if routed.open.as_ref().is_some_and(|open| !open.typing) =>
            {
                self.phase = Phase::Routed(routed);
                return self.step(Input::OpenList, now);
            }
            (Phase::Routed(mut routed), Input::OpenList) => {
                match &mut routed.open {
                    // The same rows: the highlight starts on the one the
                    // pointer is on.
                    Some(open) if !open.typing => {
                        open.typing = true;
                        open.back = true;
                        open.selected = open.pointed.unwrap_or(0);
                    }
                    Some(_) => {}
                    None => {
                        // A guess opens on itself; a rule on the next best place.
                        let selected = usize::from(matches!(routed.route, Route::Rule { .. }));
                        routed.open = Some(Open {
                            selected,
                            typing: true,
                            ..Open::suggesting()
                        });
                    }
                }
                clamp(&mut routed);
                Phase::Routed(routed)
            }
            (Phase::Routed(mut routed), input @ (Input::Type(_) | Input::Backspace)) => {
                // Typed over the suggestions: the search opens with it, and
                // Escape goes back to them.
                if let (Input::Type(_), Some(open)) = (&input, routed.open.as_mut())
                    && !open.typing
                {
                    open.typing = true;
                    open.back = true;
                }
                if let Some(open) = routed.open.as_mut().filter(|open| open.typing) {
                    match input {
                        Input::Type(typed) => open.query.insert(&typed),
                        _ => backspace(&mut open.query),
                    }
                    open.selected = 0;
                    open.pointed = None;
                }
                Phase::Routed(routed)
            }
            (Phase::Routed(mut routed), Input::Move(by)) => {
                // The arrows take over from the pointer, from where it was.
                if let Some(open) = &mut routed.open {
                    open.selected = (open.selected as i64 + i64::from(by)).max(0) as usize;
                    open.pointed = None;
                }
                clamp(&mut routed);
                Phase::Routed(routed)
            }
            (Phase::Routed(routed), Input::Send { always }) => {
                let selected = routed.open.as_ref().map(|open| open.selected);
                send(routed, selected, always, now, &mut actions)
            }
            (Phase::Routed(routed), Input::SendRow(row)) => {
                send(routed, Some(row), false, now, &mut actions)
            }
            (Phase::Routed(mut routed), Input::Cycle(by)) => {
                let rows = routed
                    .open
                    .as_ref()
                    .filter(|open| !open.typing)
                    .map_or(0, |open| list(&routed.route, open).rows().count());
                if let Some(open) = routed.open.as_mut().filter(|_| rows > 0) {
                    let rows = rows as i64;
                    open.selected =
                        (open.selected as i64 + i64::from(by)).rem_euclid(rows) as usize;
                    open.pointed = None;
                }
                Phase::Routed(routed)
            }
            (Phase::Routed(mut routed), Input::Shift(down)) => {
                if let Some(open) = &mut routed.open {
                    open.shift = down;
                }
                Phase::Routed(routed)
            }
            // A click away from a search opened over the suggestions goes
            // back to them, as Escape does.
            (Phase::Routed(mut routed), input @ (Input::ClickAway | Input::Escape))
                if routed
                    .open
                    .as_ref()
                    .is_some_and(|open| open.typing && open.back)
                    && (input == Input::Escape || !self.hovered) =>
            {
                if let Some(open) = &mut routed.open {
                    *open = Open {
                        shift: open.shift,
                        ..Open::suggesting()
                    };
                }
                Phase::Routed(routed)
            }
            // Taken back, folding it starts its hold: it leaves like any
            // finished capsule, and Send to… brings it back.
            (Phase::Routed(mut routed), Input::ClickAway) if !self.hovered => {
                if routed.taken_back {
                    routed.at = now;
                }
                if routed.open.take().is_some() {
                    self.fold_hold(now);
                }
                Phase::Routed(routed)
            }
            (phase, Input::ClickAway) => phase,
            // Taken back, ✕ (or Escape in its open list) is the dismissal:
            // the bundle is kept. Too late, it's where it went.
            (
                Phase::Routed(Routed {
                    taken_back: true,
                    already_with,
                    open,
                    ..
                }),
                input @ (Input::Escape | Input::Dismiss),
            ) if input == Input::Dismiss
                || open.as_ref().is_some_and(|open| open.typing && !open.back) =>
            {
                match already_with {
                    None => Phase::Kept { at: now },
                    Some(_) => Phase::Hidden,
                }
            }
            // Escape only reaches an open list; it folds it. It never takes
            // a send back: that is the Undo button's.
            (Phase::Routed(mut routed), Input::Escape)
                if routed.open.as_ref().is_some_and(|open| open.typing) =>
            {
                if routed.open.take().is_some() {
                    self.fold_hold(now);
                }
                Phase::Routed(routed)
            }
            (Phase::Routed(routed), Input::Undo) => {
                if let Route::ResolvingRule { name } = &routed.route {
                    actions.push(Action::Undo { to: name.clone() });
                    return {
                        self.phase = Phase::TakenBack { at: now };
                        actions
                    };
                }
                // The chip is only there while the window runs; a click that
                // lands as it ends still counts, and the router answers too
                // late if it was.
                match (rule_to(&routed.route).cloned(), !routed.taken_back) {
                    (Some(to), true) => {
                        actions.push(Action::Undo {
                            to: to.name.clone(),
                        });
                        taking_back(Some(routed.route), &to, now)
                    }
                    _ => Phase::Routed(routed),
                }
            }
            (Phase::Sent(sent), Input::Undo) if sent.undoable => {
                actions.push(Action::Undo {
                    to: sent.to.clone(),
                });
                let from = sent.from();
                taking_back(sent.back, &from, now)
            }
            // Nothing to list: dismissing says where the bundle is.
            (Phase::TakenBack { .. }, Input::Dismiss) => Phase::Kept { at: now },
            // Escape closes the picker, but never ends a recording: it may be
            // meant for the app being recorded.
            (Phase::Selecting(_), Input::Escape) => {
                actions.push(Action::Cancel);
                Phase::Hidden
            }
            // Pasted or dismissed: the take-back it asked for says nothing.
            (Phase::Hidden, Input::Undone(_)) => Phase::Hidden,
            // The list opened as the Undo was asked for. Taken back, it
            // stays as it is; too late, the session it went to has it.
            (Phase::Routed(mut routed), Input::Undone(undone)) if routed.taken_back => {
                if let (Undone::TooLate, None, Some(from)) =
                    (undone, &routed.already_with, routed.taken_from.clone())
                {
                    if let Route::Guess {
                        suggested, recent, ..
                    } = &mut routed.route
                    {
                        for dest in suggested.iter_mut().chain(recent.iter_mut()) {
                            if dest.session == from.session {
                                dest.why = tr("already sent").into();
                            }
                        }
                    }
                    routed.already_with = Some(from.name);
                }
                Phase::Routed(routed)
            }
            (Phase::TakenBack { at }, Input::Undone(_)) => Phase::TakenBack { at },
            // An Undo never ends the capture: the list comes back with the
            // same sessions, the one it went to last, until a pick or a
            // dismissal. Too late to take back, a pick sends a copy.
            (phase, Input::Undone(undone)) => {
                let (route, from) = match phase {
                    Phase::Routed(Routed {
                        route: route @ Route::Rule { .. },
                        ..
                    }) => {
                        let from = rule_to(&route).cloned();
                        (Some(route), from)
                    }
                    Phase::Sent(sent) => {
                        let from = sent.from();
                        (sent.back, Some(from))
                    }
                    other if undone == Undone::TooLate => {
                        self.phase = other;
                        return actions;
                    }
                    _ => (None, None),
                };
                let too_late = undone == Undone::TooLate;
                match (route, &from) {
                    (Some(route), Some(from)) => offered_again(route, from, too_late, now),
                    _ => None,
                }
                .map_or_else(
                    || match (too_late, from) {
                        (true, Some(from)) => Phase::TooLate {
                            to: from.name,
                            at: now,
                        },
                        (false, Some(from)) => Phase::Routed(lone_back(&from, now)),
                        _ => Phase::TakenBack { at: now },
                    },
                    Phase::Routed,
                )
            }
            // Escape is the app's everywhere else: it is never claimed, so
            // one that arrives is ignored.
            (phase, Input::Escape | Input::Undo) => phase,
            (_, Input::Dismiss) => Phase::Hidden,
            (phase, _) => phase,
        };
        actions
    }
}
