//! What a click on the capsule's controls does.
use super::*;

impl Flow {
    pub(super) fn paste_bundle(&self) -> Option<&std::path::PathBuf> {
        if self.uncopied {
            return None;
        }
        let (bundle, routing) = self.written.as_ref()?;
        match &self.phase {
            Phase::Saved { .. } if !routing => Some(bundle),
            Phase::Routed(r)
                if !r.sending
                    && !r.delivered
                    && !r.taken_back
                    && !matches!(r.route, Route::Rule { .. } | Route::ResolvingRule { .. })
                    && !(r.open.is_none()
                        && matches!(&r.route, Route::Guess { suggested, .. } if !suggested.is_empty()))
                    && r.open.as_ref().is_none_or(|open| !open.typing) =>
            {
                Some(bundle)
            }
            _ => None,
        }
    }

    /// What a click on `id` does: the action for the host, after moving
    /// the capsule as the click shows.
    pub(crate) fn click(&mut self, id: super::super::surface::Id, now: f64) -> Vec<Action> {
        let before = self.phase.clone();
        let cued = self.cues.len();
        let actions = self.press(id, now);
        // A click that became an input cued already.
        if self.cues.len() == cued {
            self.cue(&before, &actions);
        }
        self.track_undo(&actions, now);
        let mut actions = actions;
        actions.extend(self.hold_countdown(now));
        actions
    }

    pub(super) fn press(&mut self, id: super::super::surface::Id, now: f64) -> Vec<Action> {
        use super::super::surface::Id;
        match (&self.phase, id) {
            (Phase::Selecting(current), Id::Mode(mode)) => {
                if *current == mode && mode.shows_record() {
                    return vec![Action::Record(mode)];
                }
                self.phase = Phase::Selecting(mode);
                Vec::new()
            }
            (Phase::Selecting(mode), Id::Record) => vec![Action::Record(*mode)],
            // A chip restarts rather than retargets (Scrawl's
            // `switch_recording`): Screen is what's recording, so it only
            // folds the chips; iPhone starts the phone in its place; Window
            // and Area drop it and become the picker.
            (Phase::Recording { .. }, Id::Mode(mode)) => match mode {
                Mode::Screen => {
                    self.phase = Phase::Recording {
                        chips_until: None,
                        discard_until: None,
                    };
                    Vec::new()
                }
                Mode::Iphone => {
                    self.phase = Phase::Recording {
                        chips_until: None,
                        discard_until: None,
                    };
                    vec![Action::Switch(mode)]
                }
                Mode::Area | Mode::Window => {
                    self.phase = Phase::Selecting(mode);
                    vec![Action::Discard]
                }
            },
            (Phase::Recording { .. }, Id::Finish) => vec![Action::Finish],
            (Phase::Recording { .. }, Id::Pen) => vec![Action::Pen],
            // The source button opens the target chips on demand.
            (Phase::Recording { discard_until, .. }, Id::Source) => {
                self.phase = Phase::Recording {
                    chips_until: Some(now + tuning::get().chips_open_s),
                    discard_until: *discard_until,
                };
                Vec::new()
            }
            // Discarding a recording takes a second click: ✕ asks first.
            (Phase::Recording { .. }, Id::Cancel | Id::Close) => {
                self.phase = Phase::Recording {
                    chips_until: None,
                    discard_until: Some(now + DISCARD_ASK_S),
                };
                Vec::new()
            }
            (Phase::Recording { .. }, Id::Keep) => {
                self.phase = Phase::Recording {
                    chips_until: None,
                    discard_until: None,
                };
                Vec::new()
            }
            (
                Phase::Recording {
                    discard_until: Some(_),
                    ..
                },
                Id::Discard,
            )
            | (Phase::Selecting(_), Id::Cancel | Id::Close) => {
                self.phase = Phase::Hidden;
                vec![Action::Cancel]
            }
            (_, Id::Paste) => {
                let Some(bundle) = self.paste_bundle().cloned() else {
                    return Vec::new();
                };
                self.phase = Phase::Dismissed;
                vec![Action::Paste(bundle)]
            }
            (_, Id::Inspect) => {
                self.phase = Phase::Dismissed;
                vec![Action::Inspect]
            }
            (_, Id::Undo) => self.input(Input::Undo, now),
            (
                Phase::Permission {
                    need,
                    grant,
                    asked_by,
                    ..
                },
                Id::Ask,
            ) => {
                let need = *need;
                let asked_by = *asked_by;
                match *grant {
                    Grant::Off | Grant::Asked => {
                        self.phase = Phase::Permission {
                            need,
                            grant: Grant::Asked,
                            at: now,
                            asked_by,
                        };
                        vec![Action::Allow(need)]
                    }
                    Grant::Restart => vec![Action::Restart],
                    Grant::Ready => {
                        self.phase = Phase::Hidden;
                        match asked_by {
                            // Annotate's own helper records the screen, as
                            // it always has.
                            AskedBy::Capture => vec![Action::Record(Mode::Screen)],
                            // The Screenshots picker was what the shortcut
                            // asked for: open it again, never Annotate's
                            // whole-screen recording.
                            AskedBy::Screenshots { record } => {
                                vec![Action::OpenScreenshots { record }]
                            }
                        }
                    }
                }
            }
            (_, Id::Ask)
                if self.uncopied
                    && matches!(
                        &self.phase,
                        Phase::Saved { .. }
                            | Phase::Routed(Routed {
                                route: Route::Unreachable { .. },
                                ..
                            })
                    ) =>
            {
                let Some((bundle, _)) = &self.written else {
                    return Vec::new();
                };
                vec![Action::Copy(bundle.clone())]
            }
            (Phase::Routed(_), Id::QuickSend) => {
                let Phase::Routed(routed) = std::mem::replace(&mut self.phase, Phase::Hidden)
                else {
                    unreachable!()
                };
                let mut actions = Vec::new();
                let rule_match = self.rule_match.clone();
                self.phase = quick_send(routed, now, rule_match, &mut actions);
                actions
            }
            (Phase::Sent(_), Id::Always) => {
                let mut closed = false;
                if let Phase::Sent(sent) = &mut self.phase {
                    if sent.always.is_some() {
                        sent.always = None;
                        closed = true;
                    } else if let Some(rule_match) = sent.rule_match.clone() {
                        sent.always = Some(AlwaysConfig::new(
                            rule_match,
                            sent.session.clone(),
                            sent.to.clone(),
                            sent.directory.as_deref(),
                        ));
                    }
                }
                if closed {
                    self.fold_hold(now);
                }
                Vec::new()
            }
            (Phase::Sent(_), Id::AlwaysScopeNarrow) => {
                if let Phase::Sent(sent) = &mut self.phase
                    && let Some(always) = &mut sent.always
                {
                    always.use_subpath = true;
                }
                Vec::new()
            }
            (Phase::Sent(_), Id::AlwaysScopeWide) => {
                if let Phase::Sent(sent) = &mut self.phase
                    && let Some(always) = &mut sent.always
                {
                    always.use_subpath = false;
                }
                Vec::new()
            }
            (Phase::Sent(_), Id::AlwaysToSession) => {
                if let Phase::Sent(sent) = &mut self.phase
                    && let Some(always) = &mut sent.always
                {
                    always.to_session = true;
                }
                Vec::new()
            }
            (Phase::Sent(_), Id::AlwaysToProject) => {
                if let Phase::Sent(sent) = &mut self.phase
                    && let Some(always) = &mut sent.always
                    && always.project_dir.is_some()
                {
                    always.to_session = false;
                }
                Vec::new()
            }
            (Phase::Sent(_), Id::AlwaysSave) => {
                if let Phase::Sent(sent) = &mut self.phase
                    && let Some(always) = sent.always.take()
                {
                    let (kind, key, target) = always.save_spec();
                    sent.rule_saved = true;
                    sent.saved_from_popover = true;
                    sent.at = now;
                    return vec![Action::SaveRule { kind, key, target }];
                }
                Vec::new()
            }
            (_, Id::OpenRules) => {
                self.phase = Phase::Dismissed;
                vec![Action::OpenRules]
            }
            (_, Id::Ask | Id::Search | Id::More) => self.input(Input::OpenList, now),
            (_, Id::Row(row) | Id::Pick(row)) => self.input(Input::SendRow(row), now),
            (_, Id::Next) => self.input(Input::Cycle(1), now),
            // The toast's line is its one guess: a click sends it.
            (
                Phase::Routed(Routed {
                    open: Some(open), ..
                }),
                Id::Done,
            ) if !open.typing && tuning::get().flow == CapsuleFlow::Toast => {
                self.input(Input::Send { always: false }, now)
            }
            // ✕ beside "Sent to X" just closes, a rule's as a pick's.
            (_, Id::Close) => self.input(Input::Dismiss, now),
            _ => Vec::new(),
        }
    }
}
