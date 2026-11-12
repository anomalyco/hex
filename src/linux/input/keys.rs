//! The keys a Linux shortcut can bind: one table for X11's grabs, the
//! shortcuts portal's triggers, and GNOME's accelerators, so every path
//! accepts the same keys Settings records (#846).

/// A key Settings records, as X names it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Keysym {
    /// The xkb keysym name the portal and GNOME's accelerators read.
    pub(crate) name: String,
    /// The keysym X11 grabs.
    pub(crate) value: u32,
}

/// Named keys, by every lowercase name Settings or `hex settings` records.
const NAMED: &[(&[&str], &str, u32)] = &[
    (&["space", " "], "space", 0x20),
    (&["enter", "return"], "Return", 0xff0d),
    (&["tab"], "Tab", 0xff09),
    (&["backspace"], "BackSpace", 0xff08),
    (&["escape"], "Escape", 0xff1b),
    (&["delete"], "Delete", 0xffff),
    (&["insert"], "Insert", 0xff63),
    (&["home"], "Home", 0xff50),
    (&["end"], "End", 0xff57),
    (&["pageup"], "Page_Up", 0xff55),
    (&["pagedown"], "Page_Down", 0xff56),
    (&["left"], "Left", 0xff51),
    (&["up"], "Up", 0xff52),
    (&["right"], "Right", 0xff53),
    (&["down"], "Down", 0xff54),
    (&["print"], "Print", 0xff61),
    // Labs › Special keys alone; settings hold these only while it's on.
    (&["menu"], "Menu", 0xff67),
    (&["pause"], "Pause", 0xff13),
    (
        &["scroll lock", "scrolllock", "scroll_lock"],
        "Scroll_Lock",
        0xff14,
    ),
    // The ISO key left of 1; alone only with Labs › Section key alone.
    (&["§", "section"], "section", 0xa7),
];

/// ASCII punctuation, whose keysym is its character, by its xkb name.
const PUNCTUATION: &[(char, &str)] = &[
    ('!', "exclam"),
    ('"', "quotedbl"),
    ('#', "numbersign"),
    ('$', "dollar"),
    ('%', "percent"),
    ('&', "ampersand"),
    ('\'', "apostrophe"),
    ('(', "parenleft"),
    (')', "parenright"),
    ('*', "asterisk"),
    ('+', "plus"),
    (',', "comma"),
    ('-', "minus"),
    ('.', "period"),
    ('/', "slash"),
    (':', "colon"),
    (';', "semicolon"),
    ('<', "less"),
    ('=', "equal"),
    ('>', "greater"),
    ('?', "question"),
    ('@', "at"),
    ('[', "bracketleft"),
    ('\\', "backslash"),
    (']', "bracketright"),
    ('^', "asciicircum"),
    ('_', "underscore"),
    ('`', "grave"),
    ('{', "braceleft"),
    ('|', "bar"),
    ('}', "braceright"),
    ('~', "asciitilde"),
];

/// The keysym of `key`, a key name as Settings records it (`a`, `f5`,
/// `pagedown`, `-` or `minus`); `None` for a key no Linux path can bind.
pub(crate) fn keysym(key: &str) -> Option<Keysym> {
    let key = key.to_ascii_lowercase();
    let mut characters = key.chars();
    let only = characters.next().filter(|_| characters.next().is_none());
    if let Some(&(_, name, value)) = NAMED.iter().find(|(names, ..)| names.contains(&&*key)) {
        return Some(Keysym {
            name: name.into(),
            value,
        });
    }
    if let Some(&(character, name)) = PUNCTUATION
        .iter()
        .find(|&&(character, name)| only == Some(character) || key == name)
    {
        return Some(Keysym {
            name: name.into(),
            value: u32::from(character),
        });
    }
    if let Some(character) = only.filter(char::is_ascii_alphanumeric) {
        return Some(Keysym {
            name: key,
            value: u32::from(character),
        });
    }
    let number = key
        .strip_prefix('f')?
        .parse::<u32>()
        .ok()
        .filter(|number| (1..=24).contains(number))?;
    Some(Keysym {
        name: format!("F{number}"),
        value: 0xffbd + number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both(key: &str) -> Option<(String, u32)> {
        keysym(key).map(|keysym| (keysym.name, keysym.value))
    }

    /// Every key the window's recorder records resolves for X11 and the
    /// portal alike, so a saved shortcut never stops the X11 listener.
    #[test]
    fn every_recorded_key_has_a_name_and_a_keysym() {
        for (key, name, value) in [
            ("space", "space", 0x20),
            ("enter", "Return", 0xff0d),
            ("tab", "Tab", 0xff09),
            ("backspace", "BackSpace", 0xff08),
            ("delete", "Delete", 0xffff),
            ("left", "Left", 0xff51),
            ("right", "Right", 0xff53),
            ("up", "Up", 0xff52),
            ("down", "Down", 0xff54),
            ("home", "Home", 0xff50),
            ("end", "End", 0xff57),
            ("pageup", "Page_Up", 0xff55),
            ("pagedown", "Page_Down", 0xff56),
            ("insert", "Insert", 0xff63),
            ("print", "Print", 0xff61),
            ("menu", "Menu", 0xff67),
            ("pause", "Pause", 0xff13),
            ("scroll lock", "Scroll_Lock", 0xff14),
            ("§", "section", 0xa7),
            ("v", "v", 0x76),
            ("7", "7", 0x37),
            ("-", "minus", 0x2d),
            ("minus", "minus", 0x2d),
            ("!", "exclam", 0x21),
            ("f1", "F1", 0xffbe),
            ("f24", "F24", 0xffd5),
        ] {
            assert_eq!(both(key), Some((name.into(), value)), "{key}");
        }
        for key in ["", "f0", "f25", "é", "hyper", "mouse4"] {
            assert_eq!(both(key), None, "{key}");
        }
    }
}
