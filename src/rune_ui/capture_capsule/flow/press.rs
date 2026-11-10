//! What a click on the capsule's controls does.
use super::*;

impl Flow {
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
        self.closing(&before);
        self.left_waiting(before, &actions);
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
                    self.switching = true;
                    vec![Action::Switch(mode)]
                }
                Mode::Area | Mode::Window => {
                    self.phase = Phase::Selecting(mode);
                    vec![Action::Discard]
                }
            },
            (Phase::Recording { .. }, Id::Finish) => vec![Action::Finish],
            (Phase::Recording { .. }, Id::Pen) => vec![Action::Pen],
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
            // Pasting here takes back a send still waiting out its Undo, as
            // Scrawl's Paste does.
            (_, Id::Paste) => {
                let mut actions: Vec<Action> = self.undoable(now).into_iter().collect();
                actions.push(Action::Paste);
                self.phase = Phase::Hidden;
                actions
            }
            (_, Id::Inspect) => {
                self.phase = Phase::Hidden;
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
            (_, Id::Ask) => self.input(Input::OpenList, now),
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
