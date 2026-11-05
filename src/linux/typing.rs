//! Labs › Type dictations instead of pasting on Linux: XTest on X11, and
//! `wtype`'s virtual keyboard on Wayland. GNOME's RemoteDesktop portal can
//! only send keysyms, so a Wayland desktop without `wtype` has no typing
//! path and Labs doesn't list the row there.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask, Keysym};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use super::input::{KEY_PRESS, KEY_RELEASE, Keymap, ShortcutKeys, XK_SHIFT_L, connect_x11};
use super::paste::{MODIFIER_WAIT_LIMIT, run_helper, wait_for_modifiers, wtype_pastes};
use super::session::LinuxSession;
use crate::typing::Stroke;

const XK_RETURN: u32 = 0xff0d;
const XK_TAB: u32 = 0xff09;
/// How long clients get to read a remapped keycode before it is pressed.
/// X clients update their keymap from MappingNotify; a press sent in the
/// same instant can still be read through the old mapping.
const MAPPING_SETTLE: Duration = Duration::from_millis(25);
/// `wtype` types about a key every few milliseconds, plus its keymap upload;
/// generous enough for a long dictation while Escape still cancels through
/// `stop`.
const WTYPE_TIMEOUT: Duration = Duration::from_secs(60);

/// Whether this desktop can type a dictation: X11 everywhere, Wayland only
/// with `wtype` and the virtual keyboard protocol. Cached: the session and
/// the compositor's globals do not change while Hex runs.
pub(crate) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| match LinuxSession::detect() {
        LinuxSession::X11 => true,
        LinuxSession::Wayland => wtype_pastes(),
    })
}

/// Types `text` on X11 with XTest, mapping characters the layout can't type
/// onto free keycodes for the moment they are pressed.
pub(super) fn type_x11(text: &str, stop: &AtomicBool, keys: &ShortcutKeys) -> Result<()> {
    X11Typer::open()?.type_text(text, stop, keys)
}

/// Types `text` on Wayland through `wtype`, whose text comes on stdin and
/// never on its argv. A line break is one Shift-Return.
pub(super) fn type_wayland(text: &str, stop: &AtomicBool) -> Result<()> {
    if !wtype_pastes() {
        return Err(eyre!(
            "typing needs wtype and a compositor offering the virtual keyboard protocol; turn off Labs › Type dictations to paste instead"
        ));
    }
    for command in wtype_commands(text) {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("typing cancelled"));
        }
        let mut wtype = std::process::Command::new("wtype");
        // wtype reads stdin as multibyte text; Hex may run with no locale set
        // (a service started before the session), where UTF-8 would stop at
        // the first non-ASCII character.
        wtype.env("LC_ALL", "C.UTF-8");
        let input = match command {
            WtypeCommand::Text(text) => {
                wtype.arg("-");
                text.into_bytes()
            }
            WtypeCommand::LineBreak => {
                wtype.args(["-M", "shift", "-k", "Return", "-m", "shift"]);
                Vec::new()
            }
        };
        run_helper(wtype, &input, stop, WTYPE_TIMEOUT)
            .wrap_err("wtype could not type the dictation")?;
    }
    Ok(())
}

/// One `wtype` run: text goes on its stdin, never its argv.
#[derive(Debug, PartialEq, Eq)]
enum WtypeCommand {
    Text(String),
    LineBreak,
}

fn wtype_commands(text: &str) -> Vec<WtypeCommand> {
    crate::typing::strokes(text)
        .into_iter()
        .map(|stroke| match stroke {
            Stroke::Text(units) => WtypeCommand::Text(
                String::from_utf16(&units).expect("strokes keep whole characters"),
            ),
            Stroke::LineBreak => WtypeCommand::LineBreak,
        })
        .collect()
}

/// The layout's own keys and the keycodes free for remapping.
pub(super) struct KeyLayout {
    first_keycode: u8,
    symbols_per_keycode: usize,
    symbols: Vec<Keysym>,
    /// Keycodes no keysym uses, safe to map for one character at a time.
    free: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    keycode: u8,
    shift: bool,
}

/// One stretch of typing. Remapped characters in a row share a remap, so the
/// layout is changed as rarely as possible.
#[derive(Debug, PartialEq, Eq, Default)]
struct Remap {
    /// The keycode each distinct character's keysym is mapped to.
    assignments: Vec<(u8, Keysym)>,
    /// The keycodes to press, in typed order; a repeated character reuses
    /// its keycode.
    presses: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Press(Press),
    Remap(Remap),
}

impl KeyLayout {
    fn new(first_keycode: u8, symbols_per_keycode: usize, symbols: Vec<Keysym>) -> Self {
        let width = symbols_per_keycode.max(1);
        let free = symbols
            .chunks(width)
            .enumerate()
            .filter(|(_, row)| row.iter().all(|keysym| *keysym == 0))
            .map(|(index, _)| first_keycode + index as u8)
            .collect();
        Self {
            first_keycode,
            symbols_per_keycode: width,
            symbols,
            free,
        }
    }

    fn from_keymap(keymap: &Keymap) -> Self {
        let (first, symbols_per_keycode, symbols) = keymap.mapping();
        Self::new(first, symbols_per_keycode, symbols.to_vec())
    }

    /// The layout's own key for `keysym`: level 1 plainly, or level 2 with
    /// Shift (uppercase and the shifted symbols, when the layout has them).
    fn key(&self, keysym: Keysym) -> Option<Press> {
        let rows = || self.symbols.chunks(self.symbols_per_keycode).enumerate();
        if let Some((index, _)) = rows().find(|(_, row)| row.first() == Some(&keysym)) {
            return Some(Press {
                keycode: self.first_keycode + index as u8,
                shift: false,
            });
        }
        let (index, _) = rows().find(|(_, row)| row.get(1) == Some(&keysym))?;
        Some(Press {
            keycode: self.first_keycode + index as u8,
            shift: true,
        })
    }

    fn row(&self, keycode: u8) -> &[Keysym] {
        let start = usize::from(keycode - self.first_keycode) * self.symbols_per_keycode;
        &self.symbols[start..start + self.symbols_per_keycode]
    }

    fn free(&self) -> &[u8] {
        &self.free
    }
}

/// The keysym that types `character` on any layout.
fn keysym(character: char) -> Keysym {
    match character {
        '\t' => XK_TAB,
        character => {
            let code = u32::from(character);
            if (0x20..=0x7e).contains(&code) || (0xa0..=0xff).contains(&code) {
                code
            } else {
                0x0100_0000 | code
            }
        }
    }
}

/// The presses that type `text`, in order, with a line break as one
/// Shift-Return.
fn plan(text: &str, layout: &KeyLayout, return_key: u8) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut remap = Remap::default();
    let mut free = 0;
    for stroke in crate::typing::strokes(text) {
        match stroke {
            Stroke::Text(units) => {
                let text = String::from_utf16(&units).expect("strokes keep whole characters");
                for character in text.chars() {
                    let keysym = keysym(character);
                    if let Some(press) = layout.key(keysym) {
                        flush(&mut steps, &mut remap, &mut free);
                        steps.push(Step::Press(press));
                        continue;
                    }
                    if let Some(keycode) = remapped(&remap, keysym) {
                        remap.presses.push(keycode);
                        continue;
                    }
                    let Some(&keycode) = layout.free().get(free) else {
                        if remap.presses.is_empty() {
                            return Err(eyre!(
                                "this X11 keyboard has no free keycode to type {character:?}"
                            ));
                        }
                        // This stretch is full; the next one starts a new map.
                        flush(&mut steps, &mut remap, &mut free);
                        let keycode = *layout.free().first().ok_or_else(|| {
                            eyre!("this X11 keyboard has no free keycode to type {character:?}")
                        })?;
                        free = 1;
                        remap.assignments.push((keycode, keysym));
                        remap.presses.push(keycode);
                        continue;
                    };
                    free += 1;
                    remap.assignments.push((keycode, keysym));
                    remap.presses.push(keycode);
                }
            }
            Stroke::LineBreak => {
                flush(&mut steps, &mut remap, &mut free);
                steps.push(Step::Press(Press {
                    keycode: return_key,
                    shift: true,
                }));
            }
        }
    }
    flush(&mut steps, &mut remap, &mut free);
    Ok(steps)
}

fn remapped(remap: &Remap, keysym: Keysym) -> Option<u8> {
    remap
        .assignments
        .iter()
        .find(|(_, mapped)| *mapped == keysym)
        .map(|(keycode, _)| *keycode)
}

fn flush(steps: &mut Vec<Step>, remap: &mut Remap, free: &mut usize) {
    if remap.presses.is_empty() {
        return;
    }
    steps.push(Step::Remap(std::mem::take(remap)));
    *free = 0;
}

struct X11Typer {
    connection: RustConnection,
    root: u32,
    layout: KeyLayout,
    return_key: u8,
    shift_key: u8,
}

impl X11Typer {
    fn open() -> Result<Self> {
        let (connection, root, keymap) = connect_x11()?;
        Ok(Self {
            connection,
            root,
            layout: KeyLayout::from_keymap(&keymap),
            return_key: keymap.keycode(XK_RETURN)?,
            shift_key: keymap.keycode(XK_SHIFT_L)?,
        })
    }

    fn type_text(&mut self, text: &str, stop: &AtomicBool, keys: &ShortcutKeys) -> Result<()> {
        let steps = plan(text, &self.layout, self.return_key)?;
        self.wait_for_modifier_release(stop, keys)?;
        for step in steps {
            if stop.load(Ordering::Acquire) {
                return Err(eyre!("typing cancelled"));
            }
            match step {
                Step::Press(press) => self.press(press)?,
                Step::Remap(remap) => self.remap_and_type(&remap, stop)?,
            }
        }
        Ok(())
    }

    fn press(&self, Press { keycode, shift }: Press) -> Result<()> {
        if shift {
            self.fake_input(KEY_PRESS, self.shift_key)?;
        }
        self.fake_input(KEY_PRESS, keycode)?;
        self.fake_input(KEY_RELEASE, keycode)?;
        if shift {
            self.fake_input(KEY_RELEASE, self.shift_key)?;
        }
        self.connection.flush()?;
        Ok(())
    }

    fn fake_input(&self, type_: u8, keycode: u8) -> Result<()> {
        xtest::fake_input(&self.connection, type_, keycode, 0, self.root, 0, 0, 0)?.check()?;
        Ok(())
    }

    /// Maps each character onto a free keycode, types the run, then puts the
    /// previous keymap back even when a press fails or typing is cancelled.
    fn remap_and_type(&mut self, remap: &Remap, stop: &AtomicBool) -> Result<()> {
        let saved = remap
            .assignments
            .iter()
            .map(|(keycode, _)| (*keycode, self.layout.row(*keycode).to_vec()))
            .collect::<Vec<_>>();
        let typed = (|| {
            for (keycode, keysym) in &remap.assignments {
                let symbols = vec![*keysym; self.layout.symbols_per_keycode];
                self.write_mapping(*keycode, &symbols)?;
            }
            self.connection.flush()?;
            thread::sleep(MAPPING_SETTLE);
            for keycode in &remap.presses {
                if stop.load(Ordering::Acquire) {
                    return Err(eyre!("typing cancelled"));
                }
                self.press(Press {
                    keycode: *keycode,
                    shift: false,
                })?;
            }
            Ok(())
        })();
        let restored = self.restore(&saved);
        match (typed, restored) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error.wrap_err("could not put the X11 keymap back")),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn write_mapping(&self, keycode: u8, symbols: &[Keysym]) -> Result<()> {
        self.connection
            .change_keyboard_mapping(keycode, 1, self.layout.symbols_per_keycode as u8, symbols)?
            .check()?;
        Ok(())
    }

    fn restore(&self, saved: &[(u8, Vec<Keysym>)]) -> Result<()> {
        for (keycode, symbols) in saved {
            self.write_mapping(*keycode, symbols)?;
        }
        self.connection.flush()?;
        Ok(())
    }

    fn wait_for_modifier_release(&self, stop: &AtomicBool, keys: &ShortcutKeys) -> Result<()> {
        wait_for_modifiers(
            stop,
            MODIFIER_WAIT_LIMIT,
            || keys.any_held(),
            || {
                let mask = self.connection.query_pointer(self.root)?.reply()?.mask;
                let held = mask
                    & (KeyButMask::SHIFT
                        | KeyButMask::CONTROL
                        | KeyButMask::MOD1
                        | KeyButMask::MOD4);
                Ok(held != KeyButMask::default())
            },
        )
    }
}

/// A keyboard with `a`–`z` (uppercase on the second level), space on 34,
/// Return on 35, Shift on 36, and two free keycodes for remapping.
#[cfg(test)]
fn test_layout() -> KeyLayout {
    let mut symbols = vec![0; 29 * 4];
    for (index, character) in ('a'..='z').enumerate() {
        symbols[index * 4] = u32::from(character);
        symbols[index * 4 + 1] = u32::from(character.to_ascii_uppercase());
    }
    symbols[26 * 4] = 0x20;
    symbols[27 * 4] = XK_RETURN;
    symbols[28 * 4] = XK_SHIFT_L;
    let mut layout = KeyLayout::new(8, 4, symbols);
    layout.free = vec![200, 201];
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    const RETURN: u8 = 8 + 27;

    fn press(keycode: u8, shift: bool) -> Step {
        Step::Press(Press { keycode, shift })
    }

    #[test]
    fn typing_uses_the_layouts_keys_and_shift_for_capitals() {
        assert_eq!(
            plan("aB ", &test_layout(), RETURN).unwrap(),
            [
                press(8, false),
                // 'B' is the second level of the b key.
                press(9, true),
                press(8 + 26, false),
            ]
        );
    }

    #[test]
    fn typing_sends_each_line_break_as_one_shift_return() {
        assert_eq!(
            plan("a\nb\r\n", &test_layout(), RETURN).unwrap(),
            [
                press(8, false),
                press(RETURN, true),
                press(9, false),
                press(RETURN, true),
            ]
        );
    }

    #[test]
    fn characters_the_layout_lacks_are_remapped_and_repeated_ones_reuse_their_key() {
        let steps = plan("aéé", &test_layout(), RETURN).unwrap();
        let [Step::Press(_), Step::Remap(remap)] = &steps[..] else {
            panic!("expected a press then one remap: {steps:?}");
        };
        assert_eq!(remap.assignments, [(200, 0xe9)]);
        assert_eq!(remap.presses, [200, 200]);
    }

    #[test]
    fn a_remap_stops_at_the_free_keys_and_the_rest_gets_its_own() {
        let steps = plan("é漢😀", &test_layout(), RETURN).unwrap();
        let [Step::Remap(first), Step::Remap(second)] = &steps[..] else {
            panic!("expected two remap runs: {steps:?}");
        };
        assert_eq!(
            first.assignments,
            [(200, 0xe9), (201, 0x0100_0000 | 0x6f22)]
        );
        assert_eq!(first.presses, [200, 201]);
        assert_eq!(second.assignments, [(200, 0x0100_0000 | 0x1f600)]);
        assert_eq!(second.presses, [200]);
    }

    #[test]
    fn wtype_gets_text_on_stdin_and_line_breaks_as_shift_return() {
        assert_eq!(
            wtype_commands("one\ntwo\r\n"),
            [
                WtypeCommand::Text("one".into()),
                WtypeCommand::LineBreak,
                WtypeCommand::Text("two".into()),
                WtypeCommand::LineBreak,
            ]
        );
        assert_eq!(wtype_commands(""), []);
    }
}
