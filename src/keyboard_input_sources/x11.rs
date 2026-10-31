//! XKB groups as input sources on X11. Wayland has no general protocol for
//! the active layout (each compositor keeps its own), so it has none. Hex
//! reads them only while keyboard model switching is on.

use x11rb::connection::Connection;
use x11rb::protocol::xkb::{self, ConnectionExt as _, NameDetail};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::keyboard_model_switching::InputSource;

/// The core keyboard (`XkbUseCoreKbd`).
const CORE_KEYBOARD: xkb::DeviceSpec = 0x0100;

/// X11 sessions only.
pub(crate) fn available() -> bool {
    !crate::linux_session::LinuxSession::detect().is_wayland()
}

pub(crate) fn current() -> Option<String> {
    Watcher::connect()?.current()
}

pub(crate) fn enabled() -> Vec<InputSource> {
    Watcher::connect()
        .and_then(|watcher| watcher.groups())
        .unwrap_or_default()
}

/// One XKB connection, polled by the host while switching is on; reports
/// the first group once, like the macOS observer, then only changes.
pub(crate) struct Watcher {
    connection: RustConnection,
    last: Option<u8>,
}

impl Watcher {
    pub(crate) fn connect() -> Option<Self> {
        if !available() {
            return None;
        }
        let (connection, _) = x11rb::connect(None)
            .inspect_err(|error| tracing::debug!(%error, "no X11 display for input sources"))
            .ok()?;
        connection.xkb_use_extension(1, 0).ok()?.reply().ok()?;
        Some(Self {
            connection,
            last: None,
        })
    }

    /// Asks only for the active group; its name is read when it changes.
    pub(crate) fn poll(&mut self) -> Option<Option<String>> {
        let group = self.group()?;
        if self.last == Some(group) {
            return None;
        }
        self.last = Some(group);
        Some(self.source(group))
    }

    fn current(&self) -> Option<String> {
        self.source(self.group()?)
    }

    fn group(&self) -> Option<u8> {
        let state = self
            .connection
            .xkb_get_state(CORE_KEYBOARD)
            .ok()?
            .reply()
            .ok()?;
        Some(u8::from(state.group))
    }

    fn source(&self, group: u8) -> Option<String> {
        self.groups()?
            .into_iter()
            .nth(usize::from(group))
            .map(|source| source.id)
    }

    /// Groups in XKB order, so a group index finds its source.
    fn groups(&self) -> Option<Vec<InputSource>> {
        let names = self
            .connection
            .xkb_get_names(CORE_KEYBOARD, NameDetail::GROUP_NAMES | NameDetail::SYMBOLS)
            .ok()?
            .reply()
            .ok()?
            .value_list;
        let layouts = names
            .symbols_name
            .and_then(|atom| self.atom(atom))
            .map(|symbols| layouts(&symbols))
            .unwrap_or_default();
        let sources = names
            .groups
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, atom)| {
                let layout = layouts.get(index).cloned().flatten();
                let name = self
                    .atom(atom)
                    .filter(|name| !name.is_empty())
                    .or_else(|| layout.clone())
                    .unwrap_or_else(|| format!("Group {}", index + 1));
                InputSource {
                    id: layout.clone().unwrap_or_else(|| name.clone()),
                    languages: layout.as_deref().map(language).into_iter().collect(),
                    name,
                }
            })
            .collect();
        Some(sources)
    }

    fn atom(&self, atom: u32) -> Option<String> {
        let reply = self.connection.get_atom_name(atom).ok()?.reply().ok()?;
        String::from_utf8(reply.name).ok()
    }
}

/// The layout of each group from the keymap's symbols name, as setxkbmap
/// writes it: `pc+us+ru:2+de(neo):3+inet(evdev)+group(alt_shift_toggle)`.
/// The first layout is group 1; later ones carry their group number.
fn layouts(symbols: &str) -> Vec<Option<String>> {
    let mut groups: Vec<Option<String>> = Vec::new();
    let mut first = true;
    for part in symbols.split('+').skip_while(|part| *part == "pc") {
        let (layout, group) = match part.rsplit_once(':') {
            Some((layout, group)) => match group.parse::<usize>() {
                Ok(group) if (1..=4).contains(&group) => (layout, group),
                _ => continue,
            },
            None if first => (part, 1),
            None => continue,
        };
        first = false;
        if groups.len() < group {
            groups.resize(group, None);
        }
        groups[group - 1] = Some(layout.to_owned());
    }
    groups
}

/// A layout's language: XKB names most layouts for a country, not a
/// language, so the common ones are mapped; others use their own code.
fn language(layout: &str) -> String {
    let code = layout.split('(').next().unwrap_or(layout);
    match code {
        "us" | "gb" | "au" | "ie" => "en",
        "br" => "pt",
        "ua" => "uk",
        "by" => "be",
        "kz" => "kk",
        "cz" => "cs",
        "dk" => "da",
        "se" => "sv",
        "ee" => "et",
        "gr" => "el",
        "il" => "he",
        "ir" => "fa",
        "jp" => "ja",
        "kr" => "ko",
        "cn" | "tw" => "zh",
        "vn" => "vi",
        "ge" => "ka",
        "am" => "hy",
        "rs" => "sr",
        "si" => "sl",
        "latam" => "es",
        "at" | "ch" => "de",
        "ara" => "ar",
        "in" => "hi",
        "pk" => "ur",
        other => other,
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_name_gives_each_groups_layout() {
        assert_eq!(
            layouts("pc+us+ru:2+de(neo):3+inet(evdev)+group(alt_shift_toggle)"),
            [Some("us".into()), Some("ru".into()), Some("de(neo)".into())]
        );
        assert_eq!(layouts("pc+gb+inet(evdev)"), [Some("gb".into())]);
        assert_eq!(layouts("pc+us+de:3"), [Some("us".into()), None, Some("de".into())]);
    }

    #[test]
    fn country_layouts_map_to_their_language() {
        assert_eq!(language("us"), "en");
        assert_eq!(language("ua(phonetic)"), "uk");
        assert_eq!(language("ru"), "ru");
    }
}
