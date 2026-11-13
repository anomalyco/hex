//! The captured terminal: its focused pane's folder and title (Ghostty's AppleScript), and what
//! runs in front in it (the process tree under Ghostty, read through libproc with no new
//! permission), so a capture of an OpenCode TUI names its session.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::smart::Candidate;
use super::GHOSTTY;
use crate::util;

/// A terminal's focused pane, as far as it can be told.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pane {
    /// The folder its shell reported (`working directory`).
    pub cwd: Option<PathBuf>,
    pub title: String,
    /// What runs in front in it, when it's clear which pane's process that is.
    pub foreground: Option<Foreground>,
}

/// The process in front on a pane's terminal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Foreground {
    /// Its executable's file name.
    pub name: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl Pane {
    /// The folder the pane works in: its foreground process's, else the shell's.
    pub fn folder(&self) -> Option<&Path> {
        self.foreground
            .as_ref()
            .and_then(|f| f.cwd.as_deref())
            .or(self.cwd.as_deref())
    }

    /// Whether OpenCode's TUI runs in it.
    pub fn runs_opencode(&self) -> bool {
        self.foreground
            .as_ref()
            .is_some_and(Foreground::is_opencode)
            || !matches!(tui_title(&self.title), TuiTitle::Other)
    }

    /// The session named on OpenCode's command line (`-s`/`--session`). It can be stale: the
    /// TUI may have moved to another session since; the title follows it.
    pub fn argv_session(&self) -> Option<&str> {
        let foreground = self.foreground.as_ref().filter(|f| f.is_opencode())?;
        let mut args = foreground.args.iter();
        while let Some(arg) = args.next() {
            let id = match arg.as_str() {
                "-s" | "--session" => args.next().map(String::as_str),
                other => other.strip_prefix("--session="),
            };
            if let Some(id) = id.filter(|id| id.starts_with("ses_") && !id.contains('/')) {
                return Some(id);
            }
        }
        None
    }
}

impl Foreground {
    pub fn is_opencode(&self) -> bool {
        let named = |text: &str| {
            Path::new(text)
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("opencode"))
        };
        // Run by a runtime (`bun …/opencode`), the script names it.
        named(&self.name) || self.args.get(1).is_some_and(|arg| named(arg))
    }
}

/// What OpenCode's TUI puts in the terminal's title: `OC | <session title>` (cut to 40
/// characters, the last of them `…`), or `OpenCode` on its home screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuiTitle<'a> {
    Session(&'a str),
    Home,
    Other,
}

pub fn tui_title(title: &str) -> TuiTitle<'_> {
    let title = title.trim();
    if title == "OpenCode" {
        return TuiTitle::Home;
    }
    match title.strip_prefix("OC | ").map(str::trim) {
        Some(session) if !session.is_empty() => TuiTitle::Session(session),
        _ => TuiTitle::Other,
    }
}

/// Which session the TUI's title names.
#[derive(Debug, PartialEq, Eq)]
pub enum Titled<'a> {
    One(&'a Candidate),
    /// More than one session fits and nothing tells them apart.
    Several,
    None,
}

/// The session whose title the TUI shows as `shown`. When several fit, the one working in
/// `folder`, then the one OpenCode's command line `opened`; if that still leaves more than one,
/// `Several`, never a guess.
pub fn session_titled<'a>(
    shown: &str,
    candidates: &'a [Candidate],
    folder: Option<&Path>,
    opened: Option<&str>,
) -> Titled<'a> {
    let cut = shown.strip_suffix('…');
    let mut fits: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| {
            let title = c.title.trim();
            match cut {
                Some(start) => title.chars().count() > 40 && title.starts_with(start),
                None => title == shown,
            }
        })
        .collect();
    let tests: [&dyn Fn(&Candidate) -> bool; 2] = [
        &|c| folder.is_some() && c.directory.as_deref() == folder,
        &|c| opened == Some(c.id.as_str()),
    ];
    for test in tests {
        if fits.len() <= 1 {
            break;
        }
        let kept: Vec<&Candidate> = fits.iter().copied().filter(|c| test(c)).collect();
        if !kept.is_empty() {
            fits = kept;
        }
    }
    match fits.as_slice() {
        [] => Titled::None,
        [one] => Titled::One(one),
        _ => Titled::Several,
    }
}

/// Ghostty's focused pane (AppleScript, Ghostty 1.3+), with what runs in front in it. `pid` is
/// the captured app's process, when known. Other terminals give nothing yet.
pub fn terminal_pane(bundle_id: &str, pid: Option<i32>) -> Option<Pane> {
    if !bundle_id.eq_ignore_ascii_case(GHOSTTY) {
        return None;
    }
    let (cwd, title) = applescript_pane().unwrap_or_default();
    let pane = Pane {
        foreground: choose(surfaces(ghostty(pid)), cwd.as_deref(), &title),
        cwd,
        title,
    };
    (pane.cwd.is_some() || pane.foreground.is_some()).then_some(pane)
}

/// The focused pane's folder and title through Ghostty's AppleScript.
fn applescript_pane() -> Option<(Option<PathBuf>, String)> {
    let script = format!(
        "tell application id \"{GHOSTTY}\"\nif (count of windows) is 0 then return \"\"\nset t to focused terminal of selected tab of front window\nreturn (working directory of t) & linefeed & (name of t)\nend tell"
    );
    let output = super::run(Command::new("/usr/bin/osascript").args(["-e", &script]))?;
    let (dir, title) = output
        .trim_end()
        .split_once('\n')
        .unwrap_or((output.trim(), ""));
    let dir = PathBuf::from(dir.trim());
    let dir = (dir.is_absolute() && dir != Path::new("/")).then_some(dir);
    Some((dir, title.trim().to_owned()))
}

/// Ghostty's process: `pid` when it is Ghostty, else the one Ghostty running.
fn ghostty(pid: Option<i32>) -> Option<i32> {
    let is_ghostty = |pid: i32| {
        util::process_executable(pid)
            .is_some_and(|exe| exe.ends_with("Ghostty.app/Contents/MacOS/ghostty"))
    };
    if let Some(pid) = pid.filter(|&pid| is_ghostty(pid)) {
        return Some(pid);
    }
    let mut found = util::all_processes()
        .into_iter()
        .filter(|&pid| is_ghostty(pid));
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// What runs in front on each of Ghostty's terminals. Ghostty starts each one through `login`,
/// which runs as root and can't be read, so a terminal's device and front process group come
/// from the first process under it that can.
fn surfaces(ghostty: Option<i32>) -> Vec<Foreground> {
    let Some(ghostty) = ghostty else {
        return Vec::new();
    };
    util::child_processes(ghostty)
        .into_iter()
        .filter_map(|child| {
            let mut level = vec![child];
            for _ in 0..3 {
                if let Some(front) = level
                    .iter()
                    .filter_map(|&pid| util::process_info(pid))
                    .find_map(|info| info.foreground)
                {
                    return foreground(front);
                }
                level = level.into_iter().flat_map(util::child_processes).collect();
            }
            None
        })
        .collect()
}

/// The front process group's leader: its name, arguments and folder.
fn foreground(pgid: i32) -> Option<Foreground> {
    let name = util::process_executable(pgid)?
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some(Foreground {
        name,
        args: util::process_args(pgid).unwrap_or_default(),
        cwd: util::process_cwd(pgid),
    })
}

/// The focused pane's front process among every terminal's: the only one, else the one that
/// fits the pane's folder and title. `None` when that leaves more than one.
pub fn choose(surfaces: Vec<Foreground>, cwd: Option<&Path>, title: &str) -> Option<Foreground> {
    let mut left = surfaces;
    let tui = !matches!(tui_title(title), TuiTitle::Other);
    let tests: [&dyn Fn(&Foreground) -> bool; 3] = [
        &|f| !tui || f.is_opencode(),
        &|f| !title.is_empty() && f.name == title.trim(),
        &|f| cwd.is_some() && f.cwd.as_deref() == cwd,
    ];
    for test in tests {
        if left.len() <= 1 {
            break;
        }
        let kept: Vec<Foreground> = left.iter().filter(|f| test(f)).cloned().collect();
        if !kept.is_empty() {
            left = kept;
        }
    }
    (left.len() == 1).then(|| left.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn front(name: &str, args: &[&str], cwd: &str) -> Foreground {
        Foreground {
            name: name.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: Some(PathBuf::from(cwd)),
        }
    }

    fn session(id: &str, title: &str, dir: &str, updated_ms: u64) -> Candidate {
        Candidate {
            id: id.into(),
            title: title.into(),
            directory: Some(PathBuf::from(dir)),
            repo: None,
            updated_ms,
            viewed_ms: None,
        }
    }

    #[test]
    fn the_tui_title_names_a_session_or_its_home() {
        assert_eq!(
            tui_title("OC | Fix the HUD"),
            TuiTitle::Session("Fix the HUD")
        );
        assert_eq!(tui_title("OpenCode"), TuiTitle::Home);
        assert_eq!(tui_title("OC | "), TuiTitle::Other);
        assert_eq!(tui_title("~/code/hex"), TuiTitle::Other);
    }

    #[test]
    fn a_cut_title_matches_the_long_one_it_was_cut_from() {
        let long = "Make the capture capsule remember terminal folders";
        let shown: String = long.chars().take(37).collect::<String>() + "…";
        let sessions = [
            session("ses_long", long, "/code/hex", 1),
            session("ses_short", &long[..37], "/code/hex", 2),
            session("ses_same", "Fix the HUD", "/code/other", 9),
            session("ses_here", "Fix the HUD", "/code/hex", 3),
            session("ses_twin_a", "Twins", "/code/hex", 4),
            session("ses_twin_b", "Twins", "/code/hex", 5),
        ];
        let id = |shown: &str, folder: Option<&str>, opened: Option<&str>| match session_titled(
            shown,
            &sessions,
            folder.map(Path::new),
            opened,
        ) {
            Titled::One(found) => Some(found.id.as_str()),
            Titled::Several => Some("several"),
            Titled::None => None,
        };
        assert_eq!(id(&shown, None, None), Some("ses_long"));
        // Two sessions with one title: the one in the terminal's folder.
        assert_eq!(id("Fix the HUD", Some("/code/hex"), None), Some("ses_here"));
        // Nothing to tell them apart: no guess, not even the newest.
        assert_eq!(id("Fix the HUD", None, None), Some("several"));
        assert_eq!(id("Twins", Some("/code/hex"), None), Some("several"));
        // Two in one checkout with one title: the one OpenCode was opened on.
        assert_eq!(
            id("Twins", Some("/code/hex"), Some("ses_twin_a")),
            Some("ses_twin_a")
        );
        assert_eq!(id("Nothing like it", None, None), None);
    }

    #[test]
    fn opencodes_command_line_may_name_its_session() {
        let pane = |args: &[&str]| Pane {
            foreground: Some(front("opencode-cli", args, "/code/hex")),
            ..Pane::default()
        };
        let id = |args: &[&str]| pane(args).argv_session().map(str::to_owned);
        assert_eq!(
            id(&[
                "opencode-cli",
                "--server",
                "http://127.0.0.1:4096",
                "-s",
                "ses_a"
            ])
            .as_deref(),
            Some("ses_a")
        );
        assert_eq!(
            id(&["opencode", "--session=ses_b"]).as_deref(),
            Some("ses_b")
        );
        assert_eq!(id(&["opencode", "-s", "../ses_c"]), None);
        assert_eq!(id(&["opencode"]), None);
        assert!(pane(&["opencode"]).runs_opencode());
        // Another program's `-s` names nothing.
        let vim = Pane {
            foreground: Some(front("nvim", &["nvim", "-s", "ses_x"], "/code/hex")),
            ..Pane::default()
        };
        assert_eq!(vim.argv_session(), None);
        assert!(!vim.runs_opencode());
        // Run by a runtime, the script names it.
        let script = "/usr/local/lib/node_modules/opencode-ai/bin/opencode";
        assert!(front("node", &["node", script], "/").is_opencode());
        assert!(!front("node", &["node", "/x/server.js"], "/").is_opencode());
    }

    #[test]
    fn the_focused_pane_is_the_one_that_fits_its_folder_and_title() {
        let tui = front("opencode-cli", &["opencode-cli"], "/code/hex");
        let other_tui = front("opencode-cli", &["opencode-cli"], "/code/seer");
        let shell = front("zsh", &["-zsh"], "/code/hex");
        let vim = front("nvim", &["nvim"], "/code/seer");
        // One terminal needs no telling.
        assert_eq!(choose(vec![shell.clone()], None, ""), Some(shell.clone()));
        // An OpenCode title picks the TUI; two TUIs need the folder.
        assert_eq!(
            choose(
                vec![shell.clone(), tui.clone()],
                Some(Path::new("/code/hex")),
                "OC | Fix"
            ),
            Some(tui.clone())
        );
        assert_eq!(
            choose(
                vec![tui.clone(), other_tui.clone(), vim.clone()],
                Some(Path::new("/code/seer")),
                "OpenCode"
            ),
            Some(other_tui.clone())
        );
        // A title naming the program picks it.
        assert_eq!(
            choose(vec![shell.clone(), vim.clone()], None, "nvim"),
            Some(vim.clone())
        );
        // Two in one folder with nothing else to go on: neither.
        assert_eq!(
            choose(
                vec![shell.clone(), tui.clone()],
                Some(Path::new("/code/hex")),
                "~/code/hex"
            ),
            None
        );
        assert_eq!(choose(vec![shell, tui], None, ""), None);
        assert_eq!(choose(Vec::new(), None, ""), None);
    }
}
