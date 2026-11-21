//! The hex-annotate skill in coding agents' own folders, and hex on PATH for
//! the agents that run it: the Annotate pane's Agents section (Labs › Install
//! agent skill from Annotate). Every path is given, so the pane's rows read
//! [status] and an install writes [SKILL] where it is asked, and a hex another
//! program owns is never touched.
//!
//! Codex and Pi share ~/.agents/skills, where npx skills writes too, so a
//! copy there is read by content and never written twice. A symlinked
//! SKILL.md or hex-annotate folder is managed elsewhere: its content is read,
//! and nothing is written through the link.

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
    /// edited it. Installing replaces it, unless it is a symlink.
    Outdated,
}

/// One agent's folder and what it holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentStatus {
    pub(crate) agent: Agent,
    /// The agent's own config folder exists, so it is installed here.
    pub(crate) found: bool,
    pub(crate) skill: SkillState,
    /// The SKILL.md the row reads and an install writes.
    pub(crate) path: PathBuf,
    /// The file or its hex-annotate folder is a symlink, so the skill is
    /// managed elsewhere: read by content, never written through.
    pub(crate) linked: bool,
}

/// The folders every agent reads, from the home and the environment's own
/// overrides: $CODEX_HOME, $XDG_CONFIG_HOME, $CLAUDE_CONFIG_DIR, and
/// $PI_CODING_AGENT_DIR.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Paths {
    pub(crate) home: PathBuf,
    pub(crate) codex_home: Option<PathBuf>,
    pub(crate) config_home: Option<PathBuf>,
    pub(crate) claude_home: Option<PathBuf>,
    pub(crate) pi_agent: Option<PathBuf>,
}

impl Paths {
    /// This process's home and the four overrides; None when the OS reports
    /// no home folder.
    pub(crate) fn current() -> Option<Self> {
        Some(Self {
            home: crate::app_paths::home_dir()?,
            codex_home: nonempty("CODEX_HOME"),
            config_home: nonempty("XDG_CONFIG_HOME"),
            claude_home: nonempty("CLAUDE_CONFIG_DIR"),
            pi_agent: nonempty("PI_CODING_AGENT_DIR"),
        })
    }

    fn claude_folder(&self) -> PathBuf {
        self.claude_home
            .clone()
            .unwrap_or_else(|| self.home.join(".claude"))
    }

    fn codex_folder(&self) -> PathBuf {
        self.codex_home
            .clone()
            .unwrap_or_else(|| self.home.join(".codex"))
    }

    fn opencode_folder(&self) -> PathBuf {
        self.config_home
            .clone()
            .unwrap_or_else(|| self.home.join(".config"))
            .join("opencode")
    }

    fn pi_folder(&self) -> PathBuf {
        self.pi_agent
            .clone()
            .unwrap_or_else(|| self.home.join(".pi/agent"))
    }

    fn pi_root(&self) -> PathBuf {
        self.home.join(".pi")
    }

    /// ~/.agents: where Codex and Pi read skills, and where npx skills
    /// writes them for either.
    pub(crate) fn shared_folder(&self) -> PathBuf {
        self.home.join(".agents")
    }

    /// ~/.agents/skills/hex-annotate/SKILL.md.
    pub(crate) fn shared_skill(&self) -> PathBuf {
        self.shared_folder().join("skills/hex-annotate/SKILL.md")
    }

    /// The folder that means the agent is installed: ~/.claude (or
    /// $CLAUDE_CONFIG_DIR), ~/.codex (or $CODEX_HOME) or the shared
    /// ~/.agents, $XDG_CONFIG_HOME/opencode or ~/.config/opencode, and
    /// ~/.pi (or $PI_CODING_AGENT_DIR).
    pub(crate) fn found(&self, agent: Agent) -> bool {
        match agent {
            Agent::ClaudeCode => self.claude_folder().is_dir(),
            Agent::Codex => self.codex_folder().is_dir() || self.shared_folder().is_dir(),
            Agent::OpenCode => self.opencode_folder().is_dir(),
            Agent::Pi if self.pi_agent.is_some() => self.pi_folder().is_dir(),
            Agent::Pi => self.pi_root().is_dir(),
        }
    }

    /// The SKILL.md the agent reads and an install writes. Codex reads the
    /// shared copy; Pi reads the shared copy when npx skills already wrote
    /// one, else its own.
    pub(crate) fn skill_path(&self, agent: Agent) -> PathBuf {
        match agent {
            Agent::ClaudeCode => self.claude_folder().join("skills/hex-annotate/SKILL.md"),
            Agent::Codex => self.shared_skill(),
            Agent::OpenCode => self.opencode_folder().join("skills/hex-annotate/SKILL.md"),
            Agent::Pi if self.shared_skill().exists() => self.shared_skill(),
            Agent::Pi => self.pi_folder().join("skills/hex-annotate/SKILL.md"),
        }
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
                linked: linked(&path),
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

/// Whether the skill file or its hex-annotate folder is a symlink: someone
/// else manages it, so it is read but never written through.
fn linked(path: &Path) -> bool {
    is_symlink(path) || path.parent().is_some_and(is_symlink)
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Writes this Hex's skill into the agent's folder, creating its directories,
/// with a staging file renamed into place so a crash leaves the old file or
/// the complete new one. Nothing else in the folder is touched. A symlinked
/// skill or folder is left where it is.
pub(crate) fn install(paths: &Paths, agent: Agent) -> Result<PathBuf, String> {
    let path = paths.skill_path(agent);
    if linked(&path) {
        return Ok(path);
    }
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

    /// Why an install didn't happen.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) enum LinkError {
        /// macOS runs Hex from a temporary App Translocation folder; a link
        /// to it would stop working when that folder goes.
        Translocation,
        Io(String),
    }

    /// The link as this computer has it: an existing hex in any candidate
    /// folder wins, else the folder an install would use is named. A link
    /// whose target is gone is Hex's own only when it names a Hex app; one
    /// to anything else stays another program's.
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
    /// /usr/local/bin) falls through to ~/.local/bin. A dangling link to a
    /// Hex that was deleted is replaced; one to anything else is not.
    pub(crate) fn install(env: &Environment) -> Result<Status, LinkError> {
        if env.exe.to_string_lossy().contains("/AppTranslocation/") {
            return Err(LinkError::Translocation);
        }
        for folder in &env.system_dirs {
            let current = status_in(env, folder);
            match current.state {
                State::Installed | State::OwnedByAnother => return Ok(current),
                State::NotInstalled if !folder.is_dir() => {}
                State::NotInstalled => match link(env, folder) {
                    Ok(()) => return Ok(status_in(env, folder)),
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
                    Err(error) => {
                        return Err(LinkError::Io(format!(
                            "{}: {}",
                            folder.join("hex").display(),
                            error
                        )));
                    }
                },
            }
        }
        let folder = env.local_bin();
        let current = status_in(env, &folder);
        if current.state == State::NotInstalled {
            std::fs::create_dir_all(&folder)
                .map_err(|error| LinkError::Io(format!("{}: {}", folder.display(), error)))?;
            link(env, &folder).map_err(|error| {
                LinkError::Io(format!("{}: {}", folder.join("hex").display(), error))
            })?;
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
                Ok(target) => {
                    let target = resolve(folder, &target);
                    if !is_hex(&target, env) {
                        State::OwnedByAnother
                    } else if target.exists() {
                        State::Installed
                    } else {
                        // A link to a Hex that is gone is ours to replace.
                        State::NotInstalled
                    }
                }
                Err(_) => State::OwnedByAnother,
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
        let path = folder.join("hex");
        // A dangling link to a Hex that is gone is ours to replace.
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            std::fs::remove_file(&path)?;
        }
        std::os::unix::fs::symlink(&env.exe, path)
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

    /// A fresh folder under the temp directory, removed with everything in it
    /// when the test ends, panic or not.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQUENCE: AtomicU32 = AtomicU32::new(0);
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hex-agents-{}-{tag}-{sequence}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn every_agent_is_found_by_its_own_folder_and_reads_its_skill() {
        let temp = TempDir::new("agents");
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".agents")).unwrap();
        std::fs::create_dir_all(home.join(".pi")).unwrap();
        let paths = Paths {
            home: home.clone(),
            ..Paths::default()
        };
        let before = status(&paths);
        assert!(before[0].found, "Claude Code's folder exists");
        assert!(before[1].found, "the shared agents folder exists");
        assert!(!before[2].found, "OpenCode's folder is absent");
        assert!(before[3].found, "Pi's folder exists");
        assert_eq!(before[0].skill, SkillState::Missing);
        assert_eq!(
            before[0].path,
            home.join(".claude/skills/hex-annotate/SKILL.md")
        );
        assert_eq!(
            before[1].path,
            home.join(".agents/skills/hex-annotate/SKILL.md"),
            "Codex reads the shared copy"
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
    fn the_environment_s_overrides_choose_each_agent_s_folder() {
        let temp = TempDir::new("overrides");
        let home = temp.path().join("home");
        let codex = temp.path().join("codex-home");
        let config = temp.path().join("config-home");
        let claude = temp.path().join("claude-home");
        let pi = temp.path().join("pi-home");
        for folder in [&codex, &config, &claude, &pi] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::create_dir_all(config.join("opencode")).unwrap();
        let paths = Paths {
            home,
            codex_home: Some(codex.clone()),
            config_home: Some(config.clone()),
            claude_home: Some(claude.clone()),
            pi_agent: Some(pi.clone()),
        };
        assert_eq!(
            paths.skill_path(Agent::ClaudeCode),
            claude.join("skills/hex-annotate/SKILL.md")
        );
        assert_eq!(
            paths.skill_path(Agent::Codex),
            paths.shared_skill(),
            "Codex always reads the shared copy"
        );
        assert_eq!(
            paths.skill_path(Agent::OpenCode),
            config.join("opencode/skills/hex-annotate/SKILL.md")
        );
        assert_eq!(
            paths.skill_path(Agent::Pi),
            pi.join("skills/hex-annotate/SKILL.md")
        );
        let found: Vec<_> = status(&paths).into_iter().map(|s| s.found).collect();
        assert_eq!(found, [true, true, true, true]);
    }

    #[test]
    fn a_shared_skill_counts_once_for_codex_and_pi() {
        let temp = TempDir::new("shared");
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(home.join(".pi")).unwrap();
        let paths = Paths {
            home: home.clone(),
            ..Paths::default()
        };
        // npx skills wrote the shared copy: both agents read it and neither
        // is checked for a second copy.
        let shared = paths.shared_skill();
        write_file(&shared, SKILL);
        let statuses = status(&paths);
        assert_eq!(statuses[1].skill, SkillState::Current);
        assert_eq!(statuses[1].path, shared);
        assert_eq!(statuses[3].skill, SkillState::Current);
        assert_eq!(statuses[3].path, shared);

        // An outdated shared copy is the one Pi's install updates.
        std::fs::write(&shared, "old").unwrap();
        assert_eq!(status(&paths)[3].skill, SkillState::Outdated);
        install(&paths, Agent::Pi).unwrap();
        assert_eq!(std::fs::read(&shared).unwrap(), SKILL.as_bytes());
        assert!(
            !home.join(".pi/agent/skills/hex-annotate/SKILL.md").exists(),
            "no second copy is written"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_skill_or_folder_is_managed_elsewhere() {
        let temp = TempDir::new("linked");
        let home = temp.path().join("home");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
        write_file(&outside.join("hex-annotate/SKILL.md"), "an older skill");
        std::os::unix::fs::symlink(
            outside.join("hex-annotate"),
            home.join(".claude/skills/hex-annotate"),
        )
        .unwrap();
        let paths = Paths {
            home: home.clone(),
            ..Paths::default()
        };
        let statuses = status(&paths);
        assert!(statuses[0].linked, "the folder is a symlink");
        assert_eq!(statuses[0].skill, SkillState::Outdated);
        install(&paths, Agent::ClaudeCode).unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.join("hex-annotate/SKILL.md")).unwrap(),
            "an older skill",
            "nothing was written through the link"
        );
    }

    #[test]
    fn installing_creates_the_folders_writes_the_skill_and_leaves_other_files() {
        let temp = TempDir::new("install");
        let home = temp.path().join("home");
        let paths = Paths {
            home: home.clone(),
            ..Paths::default()
        };
        let folder = home.join(".agents/skills/hex-annotate");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("NOTES.md"), "mine").unwrap();

        let path = install(&paths, Agent::Codex).unwrap();
        assert_eq!(path, folder.join("SKILL.md"));
        assert_eq!(std::fs::read(&path).unwrap(), SKILL.as_bytes());
        assert_eq!(
            std::fs::read_to_string(folder.join("NOTES.md")).unwrap(),
            "mine"
        );
        // The staging file is gone; only the skill and the notes remain.
        let names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn installing_updates_an_outdated_skill() {
        let temp = TempDir::new("outdated");
        let home = temp.path().join("home");
        let paths = Paths {
            home: home.clone(),
            ..Paths::default()
        };
        install(&paths, Agent::OpenCode).unwrap();
        std::fs::write(paths.skill_path(Agent::OpenCode), "old").unwrap();
        assert_eq!(status(&paths)[2].skill, SkillState::Outdated);
        install(&paths, Agent::OpenCode).unwrap();
        assert_eq!(status(&paths)[2].skill, SkillState::Current);
    }

    #[cfg(unix)]
    mod cli_tests {
        use super::super::cli::{Environment, LinkError, State, install, status};
        use super::TempDir;
        use std::path::{Path, PathBuf};

        fn environment(root: &TempDir, name: &str) -> (Environment, PathBuf, PathBuf) {
            let base = root.path().join(name);
            let (home, homebrew, usr_local) = (
                base.join("home"),
                base.join("homebrew"),
                base.join("usr-local"),
            );
            std::fs::create_dir_all(&home).unwrap();
            // A real file, as the running app's executable is.
            let exe = base.join("Applications/Hex.app/Contents/MacOS/hex");
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, "hex").unwrap();
            let env = Environment {
                home,
                exe,
                path: Some(std::ffi::OsString::from("/usr/bin:/bin")),
                system_dirs: [homebrew.clone(), usr_local.clone()],
            };
            (env, homebrew, usr_local)
        }

        fn link_to(target: &Path, folder: &Path) {
            std::fs::create_dir_all(folder).unwrap();
            std::os::unix::fs::symlink(target, folder.join("hex")).unwrap();
        }

        /// A real file a link can point at, so it isn't dangling.
        fn fake_hex(root: &TempDir, name: &str) -> PathBuf {
            let path = root.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "hex").unwrap();
            path
        }

        #[test]
        fn the_link_tries_homebrew_then_usr_local_then_the_local_bin() {
            let root = TempDir::new("choose");
            let (env, homebrew, usr_local) = environment(&root, "env");
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
            let root = TempDir::new("install");
            let (env, homebrew, _) = environment(&root, "env");
            std::fs::create_dir_all(&homebrew).unwrap();
            let installed = install(&env).unwrap();
            assert_eq!(installed.state, State::Installed);
            assert_eq!(installed.path, homebrew.join("hex"));
            assert_eq!(std::fs::read_link(homebrew.join("hex")).unwrap(), env.exe);
            assert_eq!(status(&env).state, State::Installed);
        }

        #[test]
        fn a_hex_another_program_owns_is_never_replaced() {
            let root = TempDir::new("foreign");
            let (env, homebrew, _) = environment(&root, "file");
            std::fs::create_dir_all(&homebrew).unwrap();
            std::fs::write(homebrew.join("hex"), "another program").unwrap();
            let installed = install(&env).unwrap();
            assert_eq!(installed.state, State::OwnedByAnother);
            assert_eq!(
                std::fs::read_to_string(homebrew.join("hex")).unwrap(),
                "another program"
            );

            // A link to something that isn't Hex is foreign too.
            let (env, homebrew, _) = environment(&root, "link");
            let other = fake_hex(&root, "other-bin/hex");
            link_to(&other, &homebrew);
            assert_eq!(status(&env).state, State::OwnedByAnother);
            assert_eq!(install(&env).unwrap().state, State::OwnedByAnother);
        }

        #[test]
        fn a_link_to_another_hex_build_is_hex_s_own() {
            let root = TempDir::new("other-build");
            let (env, homebrew, _) = environment(&root, "env");
            let other = fake_hex(&root, "Hex 2.app/Contents/MacOS/hex");
            link_to(&other, &homebrew);
            assert_eq!(status(&env).state, State::Installed);
            assert_eq!(install(&env).unwrap().state, State::Installed);
            assert_eq!(
                std::fs::read_link(homebrew.join("hex")).unwrap(),
                other,
                "another build's link is kept"
            );
        }

        #[test]
        fn a_dangling_link_to_a_hex_app_is_ours_to_replace() {
            let root = TempDir::new("dangling-hex");
            let (env, homebrew, _) = environment(&root, "env");
            link_to(
                Path::new("/Applications/Gone.app/Contents/MacOS/hex"),
                &homebrew,
            );
            assert_eq!(status(&env).state, State::NotInstalled);
            let installed = install(&env).unwrap();
            assert_eq!(installed.state, State::Installed);
            assert_eq!(std::fs::read_link(homebrew.join("hex")).unwrap(), env.exe);
        }

        #[test]
        fn a_dangling_link_elsewhere_stays_foreign() {
            let root = TempDir::new("dangling-other");
            let (env, homebrew, _) = environment(&root, "env");
            let gone = root.path().join("gone/hex");
            link_to(&gone, &homebrew);
            assert_eq!(status(&env).state, State::OwnedByAnother);
            assert_eq!(install(&env).unwrap().state, State::OwnedByAnother);
            assert_eq!(
                std::fs::read_link(homebrew.join("hex")).unwrap(),
                gone,
                "the foreign link was left alone"
            );
        }

        #[test]
        fn a_translocated_hex_is_never_linked() {
            let root = TempDir::new("translocation");
            let (mut env, homebrew, _) = environment(&root, "env");
            env.exe = PathBuf::from(
                "/private/var/folders/x/AppTranslocation/ABC/d/Hex.app/Contents/MacOS/hex",
            );
            std::fs::create_dir_all(&homebrew).unwrap();
            assert_eq!(install(&env), Err(LinkError::Translocation));
            assert!(
                !homebrew.join("hex").exists(),
                "nothing was linked to a temporary folder"
            );
        }

        #[test]
        fn a_read_only_system_folder_falls_through_to_the_local_bin() {
            let root = TempDir::new("read-only");
            let (env, homebrew, _) = environment(&root, "env");
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
            let root = TempDir::new("path");
            let (mut env, _, _) = environment(&root, "env");
            let local = env.home.join(".local/bin");
            std::fs::create_dir_all(&local).unwrap();
            let hex = fake_hex(&root, "Hex.app/Contents/MacOS/hex");
            link_to(&hex, &local);
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
