//! A Linux requester's name and icon from the desktop entry its verified
//! identity points to: `<app id>.desktop` for a Flatpak,
//! `<snap>_<app>.desktop` for a Snap, and for an executable the entry whose
//! `TryExec` or `Exec` program resolves to that same file. The entry's
//! `Name` is the name and its `Icon` (hicolor theme or pixmaps, PNG or SVG)
//! the icon. No entry, no name or icon.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::super::display::{self, Shown};
use crate::local_api::consent::{SandboxKind, Verification};

/// The most desktop entries one lookup reads, and the deepest folder.
const ENTRIES: usize = 4096;
const DEPTH: usize = 3;

pub(super) fn shown(verification: &Verification) -> Shown {
    let dirs = application_dirs();
    let entry = match verification {
        Verification::Sandbox {
            kind: SandboxKind::Flatpak,
            app_id,
        } => named(&dirs, &format!("{app_id}.desktop")),
        Verification::Sandbox {
            kind: SandboxKind::Snap,
            app_id,
        } => app_id
            .split_once('.')
            .and_then(|(snap, app)| named(&dirs, &format!("{snap}_{app}.desktop"))),
        Verification::Executable { path } => running(&dirs, path, &search_path()),
        _ => None,
    };
    let Some((path, text)) = entry else {
        return Shown::default();
    };
    Shown {
        name: value(&text, "Name").and_then(crate::local_api::consent::shown_name),
        icon: display::platform_icon(&path),
    }
}

/// Every `applications` folder, most important first: `XDG_DATA_HOME`,
/// `XDG_DATA_DIRS`, then Flatpak's and snapd's exports.
fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let flatpak_user = dirs::home_dir().map(|home| home.join(".local/share/flatpak/exports/share"));
    let mut dirs: Vec<PathBuf> = home
        .into_iter()
        .chain(
            system
                .split(':')
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute()),
        )
        .chain(flatpak_user)
        .chain(["/var/lib/flatpak/exports/share", "/var/lib/snapd/desktop"].map(PathBuf::from))
        .map(|dir| dir.join("applications"))
        .collect();
    let mut seen = HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    dirs
}

/// The first `file_name` entry, by folder importance.
fn named(dirs: &[PathBuf], file_name: &str) -> Option<(PathBuf, String)> {
    dirs.iter().find_map(|dir| {
        let path = dir.join(file_name);
        let text = read_entry(&path)?;
        application(&text).then_some((path, text))
    })
}

/// The entry whose program is `executable`. An entry hidden by a more
/// important one of the same desktop file id never counts.
fn running(dirs: &[PathBuf], executable: &Path, path: &[PathBuf]) -> Option<(PathBuf, String)> {
    let target = std::fs::canonicalize(executable).ok()?;
    let mut ids = HashSet::new();
    let mut budget = ENTRIES;
    for dir in dirs {
        let mut found = Vec::new();
        collect(dir, dir, DEPTH, &mut budget, &mut found);
        found.sort();
        for (id, file) in found {
            if !ids.insert(id) {
                continue;
            }
            let Some(text) = read_entry(&file) else {
                continue;
            };
            if application(&text) && runs(&text, &target, path) {
                return Some((file, text));
            }
        }
    }
    None
}

/// `.desktop` files under `dir`, with their desktop file ids
/// (`sub/app.desktop` is `sub-app.desktop`).
fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    budget: &mut usize,
    found: &mut Vec<(String, PathBuf)>,
) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for item in read.flatten() {
        if *budget == 0 {
            return;
        }
        let path = item.path();
        let Ok(kind) = item.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 1 {
                collect(root, &path, depth - 1, budget, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "desktop") {
            *budget -= 1;
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let id = relative.to_string_lossy().replace('/', "-");
            found.push((id, path));
        }
    }
}

fn read_entry(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(64 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

/// The `[Desktop Entry]` group's unlocalized `key`.
fn value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let mut group = "";
    for line in entry.lines().map(str::trim) {
        if line.starts_with('[') {
            group = line;
            continue;
        }
        if group != "[Desktop Entry]" {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim()).filter(|value| !value.is_empty());
        }
    }
    None
}

/// A shown application entry (`Type=Application`, not `Hidden`).
fn application(entry: &str) -> bool {
    value(entry, "Type") == Some("Application") && value(entry, "Hidden") != Some("true")
}

/// Whether the entry's `TryExec`, or its `Exec` program, is `target`.
fn runs(entry: &str, target: &Path, path: &[PathBuf]) -> bool {
    let try_exec = value(entry, "TryExec").map(unescape);
    let program = value(entry, "Exec").and_then(exec_program);
    [try_exec, program]
        .into_iter()
        .flatten()
        .filter_map(|program| resolve(&program, path))
        .any(|resolved| resolved == target)
}

/// The program of an `Exec` value: its first argument, quoted or not.
fn exec_program(exec: &str) -> Option<String> {
    let exec = unescape(exec);
    let exec = exec.trim_start();
    if let Some(quoted) = exec.strip_prefix('"') {
        let mut program = String::new();
        let mut chars = quoted.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(program),
                '\\' => program.push(chars.next()?),
                c => program.push(c),
            }
        }
        return None;
    }
    exec.split_whitespace().next().map(str::to_owned)
}

/// A desktop entry string value's escapes (`\s`, `\n`, `\t`, `\r`, `\\`).
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `program` as a file: absolute as written, or the first match in `path`.
fn resolve(program: &str, path: &[PathBuf]) -> Option<PathBuf> {
    let program = Path::new(program);
    if program.is_absolute() {
        return std::fs::canonicalize(program).ok();
    }
    if program.components().count() != 1 {
        return None;
    }
    path.iter()
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
        .and_then(|found| std::fs::canonicalize(found).ok())
}

fn search_path() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|dir| dir.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hex-desktop-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn program(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        fs::canonicalize(path).unwrap()
    }

    #[test]
    fn an_exec_program_is_its_first_argument() {
        assert_eq!(exec_program("notes %U").as_deref(), Some("notes"));
        assert_eq!(
            exec_program(r#""/opt/My Notes/notes" --new-window %F"#).as_deref(),
            Some("/opt/My Notes/notes")
        );
        assert_eq!(
            exec_program(r"/opt/My\sNotes/notes").as_deref(),
            Some("/opt/My")
        );
        assert_eq!(exec_program(r#""unterminated"#), None);
    }

    #[test]
    fn an_executable_is_matched_by_its_entrys_program() {
        let root = folder("match");
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let notes = program(&bin, "notes");
        let other = program(&bin, "other");
        let user = root.join("user/applications");
        let system = root.join("system/applications");
        fs::create_dir_all(&user).unwrap();
        fs::create_dir_all(system.join("vendor")).unwrap();
        // By PATH name, in a vendor subfolder.
        fs::write(
            system.join("vendor/notes.desktop"),
            "[Desktop Entry]\nType=Application\nName=Field Notes\nExec=notes %U\nIcon=notes\n",
        )
        .unwrap();
        // Another program's entry, and an entry in another group, never match.
        fs::write(
            system.join("other.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Other\nExec={}\n[Desktop Action x]\nExec={}\n",
                other.display(),
                notes.display()
            ),
        )
        .unwrap();
        let dirs = [user.clone(), system.clone()];
        let path = [bin.clone()];
        let (file, text) = running(&dirs, &notes, &path).unwrap();
        assert_eq!(file, system.join("vendor/notes.desktop"));
        assert_eq!(value(&text, "Name"), Some("Field Notes"));

        // A user entry of the same desktop file id hides the system one.
        fs::write(
            user.join("vendor-notes.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nHidden=true\nExec=notes\n",
        )
        .unwrap();
        assert!(running(&dirs, &notes, &path).is_none());

        // TryExec names the program too.
        fs::write(
            user.join("launcher.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Launcher\nTryExec={}\nExec=sh -c notes\n",
                notes.display()
            ),
        )
        .unwrap();
        let (_, text) = running(&dirs, &notes, &path).unwrap();
        assert_eq!(value(&text, "Name"), Some("Launcher"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_sandbox_entry_is_found_by_its_file_name() {
        let root = folder("named");
        let apps = root.join("applications");
        fs::create_dir_all(&apps).unwrap();
        fs::write(
            apps.join("org.example.Notes.desktop"),
            "[Desktop Entry]\nType=Application\nName=Notes\n",
        )
        .unwrap();
        fs::write(
            apps.join("org.example.Link.desktop"),
            "[Desktop Entry]\nType=Link\n",
        )
        .unwrap();
        let dirs = [apps];
        assert!(named(&dirs, "org.example.Notes.desktop").is_some());
        assert!(named(&dirs, "org.example.Link.desktop").is_none());
        assert!(named(&dirs, "org.example.Missing.desktop").is_none());
        let _ = fs::remove_dir_all(root);
    }
}
