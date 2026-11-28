//! The keys the capsule takes from Hex's tap, and what the capture shortcut
//! does over a finished capsule that shows Paste.
use super::*;

impl Flow {
    /// Whether the capture shortcut presses Paste now (Labs › Shortcut
    /// pastes a capture): the lab is on, the capsule shows Paste, and no list
    /// is open over it. A suggestion hides Paste, so there the shortcut still
    /// opens the search.
    pub(super) fn shortcut_pastes_now(&self) -> bool {
        self.shortcut_pastes
            && self.paste_bundle().is_some()
            && !matches!(&self.phase, Phase::Routed(Routed { open: Some(_), .. }))
    }

    /// Whether the capture shortcut presses Paste while it shows; read from
    /// Labs › Shortcut pastes a capture on every poll.
    pub(crate) fn set_shortcut_pastes(&mut self, on: bool) {
        self.shortcut_pastes = on;
    }

    /// The keys the capsule takes from Hex's tap: the picker's, an open
    /// list's (Escape only in a list Kit opened or the picker), a compact
    /// finished capsule's shortcut to open session search, or, with Labs ›
    /// Shortcut pastes a capture, the shortcut that presses Paste.
    pub(crate) fn keys(&self) -> Option<crate::capture_host::CapsuleKeys> {
        use crate::capture_host::CapsuleKeys;
        if self.shortcut_pastes_now() {
            return Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false,
            });
        }
        match &self.phase {
            Phase::Hidden | Phase::Dismissed | Phase::Kept { .. } => None,
            Phase::Selecting(_) => Some(CapsuleKeys::Picker),
            // Nothing to list; ✕ dismisses, or it
            // leaves by itself.
            Phase::TakenBack { .. } => None,
            Phase::Routed(Routed {
                open: Some(open), ..
            }) => Some(if open.typing {
                CapsuleKeys::List
            } else {
                CapsuleKeys::Suggest {
                    cycle: false,
                    search: self.aimed,
                }
            }),
            Phase::Routed(routed) if routed.compact_searchable() => Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false,
            }),
            Phase::Queued { .. } => Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false,
            }),
            // While the `Always…` configurator is open, listen for clicks away
            // without taking keys from the app in front.
            Phase::Sent(Sent {
                always: Some(_), ..
            }) => Some(CapsuleKeys::Suggest {
                cycle: false,
                search: false,
            }),
            // Never Escape here: it goes to the app in front, and a send is
            // taken back only by clicking Undo.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_host::CapsuleKeys;

    const CLAIMED: Option<CapsuleKeys> = Some(CapsuleKeys::Suggest {
        cycle: false,
        search: false,
    });

    fn written(shortcut_pastes: bool, routing: bool) -> Flow {
        let mut flow = Flow::new();
        flow.set_shortcut_pastes(shortcut_pastes);
        flow.input(
            Input::Written {
                bundle: "shown.scrawl".into(),
                routing,
            },
            0.0,
        );
        flow
    }

    fn shows_paste(flow: &Flow) -> bool {
        matches!(flow.pres(0.2), Pres::Done(done) if done.paste)
    }

    fn suggested() -> Route {
        Route::Guess {
            suggested: vec![Dest::new("SEER", "same page")],
            recent: vec![Dest::new("HOME", "yesterday")],
            site: None,
        }
    }

    #[test]
    fn the_capture_shortcut_pastes_while_paste_shows() {
        let clipboard_only = |flow: &mut Flow| {
            flow.input(Input::Routed(Route::Nothing { recent: Vec::new() }), 0.1);
        };
        let with_recent = |flow: &mut Flow| {
            flow.input(
                Input::Routed(Route::Nothing {
                    recent: vec![Dest::new("HOME", "yesterday")],
                }),
                0.1,
            );
        };
        let unreachable = |flow: &mut Flow| {
            flow.input(
                Input::Routed(Route::Unreachable {
                    why: "OpenCode isn't running".into(),
                }),
                0.1,
            );
        };
        let no_routing = |_: &mut Flow| {};
        let cases: [(&dyn Fn(&mut Flow), bool); 4] = [
            (&no_routing, false),
            (&clipboard_only, true),
            (&with_recent, true),
            (&unreachable, true),
        ];
        for (answer, routing) in cases {
            let mut flow = written(true, routing);
            answer(&mut flow);
            assert!(shows_paste(&flow));
            assert_eq!(flow.keys(), CLAIMED, "the capsule takes the shortcut");
            assert_eq!(
                flow.input(Input::Record, 0.3),
                [Action::Paste("shown.scrawl".into())]
            );
            assert_eq!(flow.keys(), None, "the next press records");
            assert!(flow.input(Input::Record, 0.4).is_empty());
        }
    }

    #[test]
    fn the_capture_shortcut_still_searches_where_paste_is_hidden_or_the_lab_is_off() {
        // A suggestion hides Paste: the shortcut opens the search.
        let mut flow = written(true, true);
        flow.input(Input::Routed(suggested()), 0.1);
        assert!(flow.input(Input::Record, 0.2).is_empty());
        assert!(matches!(
            &flow.phase,
            Phase::Routed(Routed { open: Some(open), .. }) if open.typing
        ));

        // Off, the clipboard-only capsule lets the shortcut record as before.
        let mut off = written(false, true);
        off.input(Input::Routed(Route::Nothing { recent: Vec::new() }), 0.1);
        assert_eq!(off.keys(), None);
        assert!(off.input(Input::Record, 0.2).is_empty());
        assert!(shows_paste(&off));
    }
}
