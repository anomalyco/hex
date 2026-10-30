//! Input from a host's own event tap.
//!
//! Scrawl's app reads the mouse, keys and modifiers itself (a key tap, a global mouse monitor and
//! an 8 ms poller). A host that runs the engine with `SharedResources::none()`, such as Hex,
//! reports what its own tap sees instead, and these pure rules turn that into the
//! [`InputEvent`]s Scrawl's taps and poller would have sent: `⇧` alone draws ink (a `⇧`-click is
//! part of the drawing), and only `⌘`/`⌃` chords are logged, never ordinary typing.
//! Ink thresholds and clicks on the host's own windows are then decided by
//! [`SharedCaptureTelemetry::handle_input`](crate::capture::SharedCaptureTelemetry::handle_input)
//! with [`is_real_ink`](crate::capture::is_real_ink)
//! and [`on_own_window`](crate::capture::on_own_window).

use crate::capture::{InputEvent, pure_shift_state};

/// The modifiers held with an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

impl Modifiers {
    /// `⇧` without `⌘`, `⌃` or `⌥`.
    pub const fn shift_only(self) -> bool {
        self.shift && !self.others()
    }

    const fn others(self) -> bool {
        self.command || self.control || self.option
    }
}

/// One event from the host's tap, in global top-left points.
#[derive(Clone, Debug, PartialEq)]
pub enum HostInput {
    Button {
        down: bool,
        right: bool,
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    /// The pointer moved, with or without a button held.
    Moved {
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    /// The modifiers changed with the pointer at `x`, `y`.
    ModifiersChanged {
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    /// A fresh key press, never an autorepeat: its name on the user's layout ([`key_name`]).
    Key {
        name: Option<String>,
        modifiers: Modifiers,
    },
}

/// Turns one host's events into engine events, remembering between them whether `⇧` is left
/// over from a chord (such as the capture shortcut `⌥⇧S`), which must not start ink.
#[derive(Debug, Default)]
pub struct HostedInput {
    await_shift_release: bool,
}

impl HostedInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// For a capture that starts with `⇧` already held, as when a chord such as `⌥⇧S` started
    /// it: that `⇧` must come up once before it draws ink. A host passes the live session state,
    /// so a capture started without `⇧` held inks from its first stroke.
    pub fn with_shift_held(held: bool) -> Self {
        Self {
            await_shift_release: held,
        }
    }

    /// The engine event for `input` at `t_ms` on the capture's clock, if it makes one.
    pub fn translate(&mut self, t_ms: u64, input: HostInput) -> Option<InputEvent> {
        match input {
            HostInput::Button {
                down,
                right,
                x,
                y,
                modifiers,
            } => {
                let shift_only = pure_shift_state(
                    modifiers.shift,
                    modifiers.others(),
                    &mut self.await_shift_release,
                );
                Some(if down {
                    InputEvent::ButtonDown {
                        t_ms,
                        x,
                        y,
                        right,
                        shift_only,
                    }
                } else {
                    InputEvent::ButtonUp {
                        t_ms,
                        x,
                        y,
                        right,
                        shift_only,
                    }
                })
            }
            HostInput::Moved { x, y, modifiers }
            | HostInput::ModifiersChanged { x, y, modifiers } => Some(InputEvent::Move {
                t_ms,
                x,
                y,
                shift_only: pure_shift_state(
                    modifiers.shift,
                    modifiers.others(),
                    &mut self.await_shift_release,
                ),
            }),
            HostInput::Scroll { x, y, dx, dy } => {
                (dx != 0.0 || dy != 0.0).then_some(InputEvent::Scroll { t_ms, x, y, dx, dy })
            }
            HostInput::Key { name, modifiers } => key_input(t_ms, name.as_deref(), modifiers),
        }
    }
}

/// `⌘`/`⌃` chords are logged by name (`⌃⌥⇧⌘` order, as Scrawl
/// writes them). Keys without `⌘` or `⌃` are typing and never leave the host.
pub fn key_input(t_ms: u64, name: Option<&str>, modifiers: Modifiers) -> Option<InputEvent> {
    let Modifiers {
        command,
        control,
        option,
        shift,
    } = modifiers;
    if !command && !control {
        return None;
    }
    let mut shortcut = String::new();
    for (held, symbol) in [(control, '⌃'), (option, '⌥'), (shift, '⇧'), (command, '⌘')] {
        if held {
            shortcut.push(symbol);
        }
    }
    shortcut.push_str(name?);
    Some(InputEvent::KeyChord { t_ms, shortcut })
}

/// The name of a macOS virtual key as the user's layout labels it: keys that type no character
/// are named the same on every layout; the rest come from `layout_char` (what `⌘` + the key
/// types), else from the ANSI QWERTY positions.
pub fn key_name(keycode: u16, layout_char: impl Fn(u16) -> Option<char>) -> Option<String> {
    let fixed = match keycode {
        36 | 76 => "↩",
        48 => "⇥",
        49 => "Space",
        51 => "⌫",
        53 => "Esc",
        117 => "⌦",
        123 => "←",
        124 => "→",
        125 => "↓",
        126 => "↑",
        115 => "Home",
        119 => "End",
        116 => "PgUp",
        121 => "PgDn",
        122 => "F1",
        120 => "F2",
        99 => "F3",
        118 => "F4",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        100 => "F8",
        101 => "F9",
        109 => "F10",
        103 => "F11",
        111 => "F12",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_string());
    }
    layout_char(keycode)
        .or_else(|| ansi_key_char(keycode))
        .map(|c| c.to_uppercase().collect())
}

fn ansi_key_char(keycode: u16) -> Option<char> {
    const ANSI: &str = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.";
    ANSI.chars()
        .nth(usize::from(keycode))
        .filter(|&c| c != '\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: Modifiers = Modifiers {
        command: false,
        control: false,
        option: false,
        shift: true,
    };
    const OPTION_SHIFT: Modifiers = Modifiers {
        command: false,
        control: false,
        option: true,
        shift: true,
    };
    const COMMAND: Modifiers = Modifiers {
        command: true,
        control: false,
        option: false,
        shift: false,
    };

    fn moved(input: &mut HostedInput, modifiers: Modifiers) -> bool {
        match input.translate(
            1,
            HostInput::Moved {
                x: 0.0,
                y: 0.0,
                modifiers,
            },
        ) {
            Some(InputEvent::Move { shift_only, .. }) => shift_only,
            other => panic!("not a move: {other:?}"),
        }
    }

    #[test]
    fn shift_alone_inks_but_shift_left_from_a_chord_does_not() {
        let mut input = HostedInput::new();
        assert!(moved(&mut input, SHIFT));
        assert!(!moved(&mut input, Modifiers::default()));
        // ⌥⇧S, then ⌥ comes up first: the ⇧ still held is the chord's, not ink.
        assert!(!moved(&mut input, OPTION_SHIFT));
        assert!(!moved(&mut input, SHIFT));
        assert!(!moved(&mut input, Modifiers::default()));
        assert!(moved(&mut input, SHIFT));
    }

    #[test]
    fn a_capture_started_by_its_shortcut_waits_for_shift_to_come_up() {
        // The first event a host sees may already be ⌥ coming up with ⇧ still held.
        let mut input = HostedInput::with_shift_held(true);
        assert!(!moved(&mut input, SHIFT));
        assert!(!moved(&mut input, Modifiers::default()));
        assert!(moved(&mut input, SHIFT));
    }

    #[test]
    fn a_capture_started_without_shift_held_inks_from_the_first_stroke() {
        let mut input = HostedInput::with_shift_held(false);
        assert!(moved(&mut input, SHIFT));
    }

    #[test]
    fn a_click_with_the_chords_shift_still_held_is_no_pin() {
        let mut input = HostedInput::with_shift_held(true);
        let click = |input: &mut HostedInput, down, modifiers| match input.translate(
            1,
            HostInput::Button {
                down,
                right: false,
                x: 0.0,
                y: 0.0,
                modifiers,
            },
        ) {
            Some(
                InputEvent::ButtonDown { shift_only, .. } | InputEvent::ButtonUp { shift_only, .. },
            ) => shift_only,
            other => panic!("{other:?}"),
        };
        assert!(!click(&mut input, true, SHIFT));
        assert!(!click(&mut input, false, SHIFT));
        // ⇧ comes up and goes down again: now it draws.
        assert!(!click(&mut input, true, Modifiers::default()));
        assert!(click(&mut input, true, SHIFT));
    }

    #[test]
    fn modifier_changes_move_the_ink_like_pointer_moves() {
        let mut input = HostedInput::new();
        let event = input.translate(
            7,
            HostInput::ModifiersChanged {
                x: 3.0,
                y: 4.0,
                modifiers: SHIFT,
            },
        );
        assert_eq!(
            event,
            Some(InputEvent::Move {
                t_ms: 7,
                x: 3.0,
                y: 4.0,
                shift_only: true
            })
        );
    }

    #[test]
    fn buttons_carry_whether_shift_was_held_alone() {
        let mut input = HostedInput::new();
        let down = input.translate(
            5,
            HostInput::Button {
                down: true,
                right: false,
                x: 1.0,
                y: 2.0,
                modifiers: SHIFT,
            },
        );
        assert_eq!(
            down,
            Some(InputEvent::ButtonDown {
                t_ms: 5,
                x: 1.0,
                y: 2.0,
                right: false,
                shift_only: true
            })
        );
        let up = input.translate(
            6,
            HostInput::Button {
                down: false,
                right: true,
                x: 1.0,
                y: 2.0,
                modifiers: OPTION_SHIFT,
            },
        );
        assert_eq!(
            up,
            Some(InputEvent::ButtonUp {
                t_ms: 6,
                x: 1.0,
                y: 2.0,
                right: true,
                shift_only: false
            })
        );
    }

    #[test]
    fn empty_scrolls_are_dropped() {
        let mut input = HostedInput::new();
        let scroll = |dx, dy| HostInput::Scroll {
            x: 0.0,
            y: 0.0,
            dx,
            dy,
        };
        assert_eq!(input.translate(1, scroll(0.0, 0.0)), None);
        assert!(input.translate(1, scroll(0.0, -3.0)).is_some());
    }

    #[test]
    fn only_modifier_chords_are_logged() {
        assert_eq!(
            key_input(9, Some("Z"), COMMAND),
            Some(InputEvent::KeyChord {
                t_ms: 9,
                shortcut: "⌘Z".into()
            })
        );
        assert_eq!(
            key_input(9, Some("B"), COMMAND),
            Some(InputEvent::KeyChord {
                t_ms: 9,
                shortcut: "⌘B".into()
            })
        );
        let all = Modifiers {
            command: true,
            control: true,
            option: true,
            shift: true,
        };
        assert_eq!(
            key_input(9, Some("Z"), all),
            Some(InputEvent::KeyChord {
                t_ms: 9,
                shortcut: "⌃⌥⇧⌘Z".into()
            })
        );
        // Typing, with or without ⇧ or ⌥, never leaves the host.
        assert_eq!(key_input(9, Some("A"), Modifiers::default()), None);
        assert_eq!(key_input(9, Some("A"), OPTION_SHIFT), None);
        assert_eq!(key_input(9, None, COMMAND), None);
    }

    #[test]
    fn shortcut_keys_are_named_by_the_layout_not_their_qwerty_position() {
        // Dvorak puts B where QWERTY has N (45) and T where QWERTY has K (40).
        let dvorak = |code: u16| match code {
            45 => Some('b'),
            40 => Some('t'),
            13 => Some(','),
            _ => None,
        };
        assert_eq!(key_name(45, dvorak).as_deref(), Some("B"));
        assert_eq!(key_name(40, dvorak).as_deref(), Some("T"));
        assert_eq!(key_name(13, dvorak).as_deref(), Some(","));
        // Keys that type nothing keep fixed names whatever the layout says.
        let odd = |_| Some('x');
        assert_eq!(key_name(36, odd).as_deref(), Some("↩"));
        assert_eq!(key_name(123, odd).as_deref(), Some("←"));
        assert_eq!(key_name(122, odd).as_deref(), Some("F1"));
        assert_eq!(key_name(53, odd).as_deref(), Some("Esc"));
    }

    #[test]
    fn shortcut_keys_fall_back_to_ansi_positions_without_a_layout() {
        let none = |_| None;
        assert_eq!(key_name(45, none).as_deref(), Some("N"));
        assert_eq!(key_name(17, none).as_deref(), Some("T"));
        assert_eq!(key_name(0, none).as_deref(), Some("A"));
        assert_eq!(key_name(47, none).as_deref(), Some("."));
        assert_eq!(key_name(29, none).as_deref(), Some("0"));
        assert_eq!(key_name(10, none), None);
        assert_eq!(key_name(200, none), None);
    }
}
