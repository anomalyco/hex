//! The hex-annotate skill in coding agents' own folders, and hex on PATH for
//! the agents that run it: the Annotate pane's Agents section (Labs › Install
//! agent skill from Annotate). Every path is given, so the pane's rows read
//! [status] and an install writes [SKILL] where it is asked, and a hex another
//! program owns is never touched.

use std::path::{Path, PathBuf};

/// The guide "hex annotate skill" prints, embedded from the repository.
pub(crate) use crate::control::cli::annotate_read::SKILL;

/// One coding agent that reads skills from a folder of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Agent {
    ClaudeCode,
    Codex,
    OpenCode,
    Pi,
}

impl Agent {
    /// Every agent the section lists, in its order.
    pub(crate) const ALL: [Self; 4] = [Self::ClaudeCode, Self::Codex, Self::OpenCode, Self::Pi];

    /// The name the row shows; a product name, never translated.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Pi => "Pi",
        }
    }

    /// The row's key in the window.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
        }
    }
}

/// Whether an agent's folder already holds this Hex's skill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SkillState {
    /// No SKILL.md in the agent's folder.
    Missing,
    /// The file is exactly the skill this Hex ships.
    Current,
    /// The file is there but differs: an older Hex wrote it, or someone
    /// edited it. Installing replaces it.
    Outdated,
}

/// One agent's folder and what it holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentStatus {
    pub(crate) agent: Agent,
    /// The agent's own config folder exists, so it is installed here.
    pub(crate) found: bool,
    pub(crate) skill: SkillState,
    /// The SKILL.md path, whether or not it exists.
    pub(crate) path: PathBuf,
}

/// The folders every agent reads, from the home and the environment's own
/// overrides.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Paths {
    home: PathBuf,
    codex_home: Option<PathBuf>,
    config_home: Option<PathBuf>,
}

impl Paths {
    pub(crate) fn new(
        home: PathBuf,
        codex_home: Option<PathBuf>,
        config_home: Option<PathBuf>,
    ) -> Self {
        Self {
            home,
            codex_home,
            config_home,
        }
    }

    /// This process's home, $CODEX_HOME, and $XDG_CONFIG_HOME; None when
    /// the OS reports no home folder.
    pub(crate) fn current() -> Option<Self> {
        Some(Self::new(
            crate::app_paths::home_dir()?,
            nonempty("CODEX_HOME"),
            nonempty("XDG_CONFIG_HOME"),
        ))
    }

    /// The folder that means the agent is installed: ~/.claude,
    /// $CODEX_HOME or ~/.codex, $XDG_CONFIG_HOME/opencode or
    /// ~/.config/opencode, and ~/.pi.
    pub(crate) fn folder(&self, agent: Agent) -> PathBuf {
        match agent {
            Agent::ClaudeCode => self.home.join(".claude"),
            Agent::Codex => self
                .codex_home
                .clone()
                .unwrap_or_else(|| self.home.join(".codex")),
            Agent::OpenCode => self
                .config_home
                .clone()
                .unwrap_or_else(|| self.home.join(".config"))
                .join("opencode"),
            Agent::Pi => self.home.join(".pi"),
        }
    }

    /// Where the agent's SKILL.md goes.
    pub(crate) fn skill_path(&self, agent: Agent) -> PathBuf {
        let folder = self.folder(agent);
        match agent {
            // Pi keeps each agent's skills under ~/.pi/agent.
            Agent::Pi => folder.join("agent/skills/hex-annotate/SKILL.md"),
            _ => folder.join("skills/hex-annotate/SKILL.md"),
        }
    }

    /// Whether the agent's own folder exists.
    pub(crate) fn found(&self, agent: Agent) -> bool {
        self.folder(agent).is_dir()
    }
}

fn nonempty(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Every agent's folder and skill as this computer has them.
pub(crate) fn status(paths: &Paths) -> Vec<AgentStatus> {
    Agent::ALL
        .into_iter()
        .map(|agent| {
            let path = paths.skill_path(agent);
            AgentStatus {
                agent,
                found: paths.found(agent),
                skill: read_state(&path),
                path,
            }
        })
        .collect()
}

fn read_state(path: &Path) -> SkillState {
    match std::fs::read(path) {
        Ok(bytes) if bytes == SKILL.as_bytes() => SkillState::Current,
        Ok(_) => SkillState::Outdated,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SkillState::Missing,
        // Unreadable (a directory, a permission bit): installing replaces it.
        Err(_) => SkillState::Outdated,
    }
}

/// Writes this Hex's skill into the agent's folder, creating its directories,
/// with a staging file renamed into place so a crash leaves the old file or
/// the complete new one. Nothing else in the folder is touched.
pub(crate) fn install(paths: &Paths, agent: Agent) -> Result<PathBuf, String> {
    let path = paths.skill_path(agent);
    let folder = path
        .parent()
        .ok_or_else(|| format!("{} has no folder", path.display()))?;
    std::fs::create_dir_all(folder).map_err(|error| format!("{}: {}", folder.display(), error))?;
    crate::durable::write(&path, SKILL.as_bytes())
        .map_err(|error| format!("{}: {}", path.display(), error))?;
    Ok(path)
}

/// Putting Hex's hex on PATH for agents: /opt/homebrew/bin, /usr/local/bin,
/// else ~/.local/bin. A hex that isn't Hex's own link is never replaced.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod cli {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    /// What a status reads: the home folder, this process's executable, the
    /// PATH variable, and the two system folders to try first.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct Environment {
        pub(crate) home: PathBuf,
        pub(crate) exe: PathBuf,
        pub(crate) path: Option<OsString>,
        /// /opt/homebrew/bin and /usr/local/bin, in the order tried.
        pub(crate) system_dirs: [PathBuf; 2],
    }

    impl Environment {
        pub(crate) fn current() -> Self {
            Self {
                home: crate::app_paths::home_dir().unwrap_or_default(),
                exe: std::env::current_exe().unwrap_or_default(),
                path: std::env::var_os("PATH"),
                system_dirs: [
                    PathBuf::from("/opt/homebrew/bin"),
                    PathBuf::from("/usr/local/bin"),
                ],
            }
        }

        fn local_bin(&self) -> PathBuf {
            self.home.join(".local/bin")
        }

        fn candidates(&self) -> [PathBuf; 3] {
            [
                self.system_dirs[0].clone(),
                self.system_dirs[1].clone(),
                self.local_bin(),
            ]
        }
    }

    /// What the Command-line tool row says.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum State {
        NotInstalled,
        Installed,
        /// A regular file, or a link to anything but Hex.
        OwnedByAnother,
    }

    /// The hex link and the folder it is (or would be) in.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct Status {
        pub(crate) state: State,
        /// The link path, folder/hex.
        pub(crate) path: PathBuf,
        /// The folder is ~/.local/bin and $PATH doesn't name it.
        pub(crate) off_path: bool,
    }

    /// The link as this computer has it: an existing hex in any candidate
    /// folder wins, else the folder an install would use is named.
    pub(crate) fn status(env: &Environment) -> Status {
        for folder in env.candidates() {
            let found = status_in(env, &folder);
            if found.state != State::NotInstalled {
                return found;
            }
        }
        status_in(env, &target(env))
    }

    /// Links hex into the first folder that takes it. A folder holding
    /// another program's hex is left alone, and the status returned says so.
    /// A system folder that exists but refuses the link (a root-owned
    /// /usr/local/bin) falls through to ~/.local/bin.
    pub(crate) fn install(env: &Environment) -> Result<Status, String> {
        for folder in &env.system_dirs {
            let current = status_in(env, folder);
            match current.state {
                State::Installed | State::OwnedByAnother => return Ok(current),
                State::NotInstalled if !folder.is_dir() => {}
                State::NotInstalled => match link(env, folder) {
                    Ok(()) => return Ok(status_in(env, folder)),
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
                    Err(error) => {
                        return Err(format!("{}: {}", folder.join("hex").display(), error));
                    }
                },
            }
        }
        let folder = env.local_bin();
        let current = status_in(env, &folder);
        if current.state == State::NotInstalled {
            std::fs::create_dir_all(&folder)
                .map_err(|error| format!("{}: {}", folder.display(), error))?;
            link(env, &folder)
                .map_err(|error| format!("{}: {}", folder.join("hex").display(), error))?;
        }
        Ok(status_in(env, &folder))
    }

    /// The folder an install starts with: the first system folder that exists
    /// and reads as writable, else ~/.local/bin.
    fn target(env: &Environment) -> PathBuf {
        env.system_dirs
            .iter()
            .find(|folder| writable(folder))
            .cloned()
            .unwrap_or_else(|| env.local_bin())
    }

    /// A cheap read: the folder exists and its own permission bits allow a
    /// write. A folder that passes but still refuses the link falls through
    /// at install.
    fn writable(folder: &Path) -> bool {
        std::fs::metadata(folder).is_ok_and(|meta| meta.is_dir() && !meta.permissions().readonly())
    }

    fn status_in(env: &Environment, folder: &Path) -> Status {
        let path = folder.join("hex");
        let state = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => match std::fs::read_link(&path) {
                Ok(target) if is_hex(&resolve(folder, &target), env) => State::Installed,
                _ => State::OwnedByAnother,
            },
            Ok(_) => State::OwnedByAnother,
            Err(_) => State::NotInstalled,
        };
        Status {
            state,
            path,
            off_path: folder == env.local_bin()
                && env
                    .path
                    .as_deref()
                    .is_some_and(|path| !on_path(path, folder)),
        }
    }

    fn resolve(folder: &Path, target: &Path) -> PathBuf {
        if target.is_absolute() {
            target.to_path_buf()
        } else {
            folder.join(target)
        }
    }

    /// This Hex's own executable, or any Hex.app's: a link to another build's
    /// app still is Hex.
    fn is_hex(target: &Path, env: &Environment) -> bool {
        target == env.exe || is_bundle_hex(target)
    }

    fn is_bundle_hex(target: &Path) -> bool {
        target.file_name().is_some_and(|name| name == "hex")
            && target
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "MacOS")
            && target
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Contents")
    }

    fn on_path(path: &std::ffi::OsStr, folder: &Path) -> bool {
        std::env::split_paths(path).any(|entry| entry == folder)
    }

    #[cfg(unix)]
    fn link(env: &Environment, folder: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(&env.exe, folder.join("hex"))
    }

    #[cfg(not(unix))]
    fn link(_: &Environment, folder: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("linking hex is not supported here: {}", folder.display()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh folder under the temp directory, named for this test.
    pub(super) fn temp(name: &str) -> PathBuf {
        static SEQUENCE: AtomicU32 = AtomicU32::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let folder = std::env::temp_dir().join(format!(
            "hex-agents-{}-{name}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn every_agent_is_found_by_its_own_folder_and_reads_its_skill() {
        let home = temp("agents");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".pi")).unwrap();
        let paths = Paths::new(home.clone(), None, None);
        let before = status(&paths);
        assert!(before[0].found, "Claude Code's folder exists");
        assert!(!before[1].found, "Codex's folder is absent");
        assert!(!before[2].found, "OpenCode's folder is absent");
        assert!(before[3].found, "Pi's folder exists");
        assert_eq!(before[0].skill, SkillState::Missing);
        assert_eq!(
            before[0].path,
            home.join(".claude/skills/hex-annotate/SKILL.md")
        );
        assert_eq!(
            before[3].path,
            home.join(".pi/agent/skills/hex-annotate/SKILL.md")
        );

        // A skill Hex wrote reads current; an edited one reads outdated.
        install(&paths, Agent::ClaudeCode).unwrap();
        let installed = status(&paths);
        assert_eq!(installed[0].skill, SkillState::Current);
        assert_eq!(std::fs::read(&installed[0].path).unwrap(), SKILL.as_bytes());
        std::fs::write(&installed[0].path, "an older skill").unwrap();
        assert_eq!(status(&paths)[0].skill, SkillState::Outdated);
    }

    #[test]
    fn codex_home_and_the_xdg_config_home_win_over_the_defaults() {
        let home = temp("overrides");
        let codex = temp("codex-home");
        let config = temp("config-home");
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::create_dir_all(config.join("opencode")).unwrap();
        let paths = Paths::new(home, Some(codex.clone()), Some(config.clone()));
        assert_eq!(
            paths.skill_path(Agent::Codex),
            codex.join("skills/hex-annotate/SKILL.md")
        );
        assert_eq!(
            paths.skill_path(Agent::OpenCode),
            config.join("opencode/skills/hex-annotate/SKILL.md")
        );
        let found: Vec<_> = status(&paths).into_iter().map(|s| s.found).collect();
        assert_eq!(found, [false, true, true, false]);
    }

    #[test]
    fn installing_creates_the_folders_writes_the_skill_and_leaves_other_files() {
        let home = temp("install");
        let paths = Paths::new(home.clone(), None, None);
        let folder = home.join(".codex/skills/hex-annotate");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("NOTES.md"), "mine").unwrap();

        let path = install(&paths, Agent::Codex).unwrap();
        assert_eq!(path, folder.join("SKILL.md"));
        assert_eq!(std::fs::read(&path).unwrap(), SKILL.as_bytes());
        assert_eq!(
            std::fs::read_to_string(folder.join("NOTES.md")).unwrap(),
            "mine"
        );
        // The staging file is gone.
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn installing_updates_an_outdated_skill() {
        let home = temp("outdated");
        let paths = Paths::new(home, None, None);
        install(&paths, Agent::OpenCode).unwrap();
        std::fs::write(paths.skill_path(Agent::OpenCode), "old").unwrap();
        assert_eq!(status(&paths)[2].skill, SkillState::Outdated);
        install(&paths, Agent::OpenCode).unwrap();
        assert_eq!(status(&paths)[2].skill, SkillState::Current);
    }

    #[cfg(unix)]
    mod cli_tests {
        use super::super::cli::{Environment, State, install, status};
        use std::path::{Path, PathBuf};

        fn environment(name: &str) -> (Environment, PathBuf, PathBuf) {
            let root = super::temp(name);
            let home = root.join("home");
            let homebrew = root.join("homebrew");
            let usr_local = root.join("usr-local");
            std::fs::create_dir_all(&home).unwrap();
            let env = Environment {
                home,
                exe: PathBuf::from("/Applications/Hex.app/Contents/MacOS/hex"),
                path: Some(std::ffi::OsString::from("/usr/bin:/bin")),
                system_dirs: [homebrew.clone(), usr_local.clone()],
            };
            (env, homebrew, usr_local)
        }

        fn link_to(target: &Path, folder: &Path) {
            std::fs::create_dir_all(folder).unwrap();
            std::os::unix::fs::symlink(target, folder.join("hex")).unwrap();
        }

        #[test]
        fn the_link_tries_homebrew_then_usr_local_then_the_local_bin() {
            let (env, homebrew, usr_local) = environment("choose");
            assert_eq!(status(&env).state, State::NotInstalled);
            assert_eq!(status(&env).path, env.home.join(".local/bin/hex"));

            std::fs::create_dir_all(&usr_local).unwrap();
            assert_eq!(
                status(&env).path,
                usr_local.join("hex"),
                "the first existing writable system folder"
            );
            std::fs::create_dir_all(&homebrew).unwrap();
            assert_eq!(status(&env).path, homebrew.join("hex"));
        }

        #[test]
        fn installing_links_this_hex_into_the_first_folder() {
            let (env, homebrew, _) = environment("install");
            std::fs::create_dir_all(&homebrew).unwrap();
            let installed = install(&env).unwrap();
            assert_eq!(installed.state, State::Installed);
            assert_eq!(installed.path, homebrew.join("hex"));
            assert_eq!(std::fs::read_link(homebrew.join("hex")).unwrap(), env.exe);
            assert_eq!(status(&env).state, State::Installed);
        }

        #[test]
        fn a_hex_another_program_owns_is_never_replaced() {
            let (env, homebrew, _) = environment("foreign");
            std::fs::create_dir_all(&homebrew).unwrap();
            std::fs::write(homebrew.join("hex"), "another program").unwrap();
            let installed = install(&env).unwrap();
            assert_eq!(installed.state, State::OwnedByAnother);
            assert_eq!(
                std::fs::read_to_string(homebrew.join("hex")).unwrap(),
                "another program"
            );

            // A link to something that isn't Hex is foreign too.
            let (env, homebrew, _) = environment("foreign-link");
            link_to(Path::new("/usr/bin/true"), &homebrew);
            assert_eq!(status(&env).state, State::OwnedByAnother);
            assert_eq!(install(&env).unwrap().state, State::OwnedByAnother);
        }

        #[test]
        fn a_link_to_another_hex_build_is_hex_s_own() {
            let (env, homebrew, _) = environment("other-build");
            let other = Path::new("/Applications/Hex 2.app/Contents/MacOS/hex");
            link_to(other, &homebrew);
            assert_eq!(status(&env).state, State::Installed);
            assert_eq!(install(&env).unwrap().state, State::Installed);
            assert_eq!(
                std::fs::read_link(homebrew.join("hex")).unwrap(),
                other,
                "another build's link is kept"
            );
        }

        #[test]
        fn a_read_only_system_folder_falls_through_to_the_local_bin() {
            let (env, homebrew, _) = environment("read-only");
            std::fs::create_dir_all(&homebrew).unwrap();
            let original = std::fs::metadata(&homebrew).unwrap().permissions();
            let mut readonly = original.clone();
            readonly.set_readonly(true);
            std::fs::set_permissions(&homebrew, readonly).unwrap();
            let installed = install(&env);
            std::fs::set_permissions(&homebrew, original).unwrap();
            let installed = installed.unwrap();
            assert_eq!(installed.state, State::Installed);
            assert_eq!(installed.path, env.home.join(".local/bin/hex"));
            assert!(installed.off_path, "~/.local/bin is not on this PATH");
        }

        #[test]
        fn the_local_bin_hint_needs_the_folder_off_path() {
            let (mut env, _, _) = environment("path");
            let local = env.home.join(".local/bin");
            std::fs::create_dir_all(&local).unwrap();
            link_to(
                Path::new("/Applications/Hex.app/Contents/MacOS/hex"),
                &local,
            );
            assert!(status(&env).off_path);
            env.path = Some(std::ffi::OsString::from(format!(
                "/usr/bin:{}",
                local.display()
            )));
            assert!(!status(&env).off_path, "the folder is on PATH");
            let installed = status(&env);
            assert_eq!(installed.state, State::Installed);
            assert_eq!(installed.path, local.join("hex"));
        }
    }
}
