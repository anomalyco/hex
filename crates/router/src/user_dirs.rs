//! The user's folders, as the process's environment names them.
//!
//! On Windows `dirs` asks the shell for the signed-in user's known folders and
//! ignores `USERPROFILE`, `LOCALAPPDATA`, and `APPDATA`. A copy launched with
//! those pointed elsewhere (an isolated benchmark of its host) would then read
//! and write the real user's folders. Here the environment comes first, as it
//! already does on Linux (`HOME`, `XDG_*`) and macOS (`HOME`); for a signed-in
//! user they name the same folders. Documents and Downloads have no variable:
//! they follow a profile folder only when `USERPROFILE` points away from the
//! user's own.

use std::path::PathBuf;

#[cfg(windows)]
fn env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// `USERPROFILE` on Windows, else `dirs::home_dir` (`HOME`).
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(profile) = env("USERPROFILE") {
        return Some(profile);
    }
    dirs::home_dir()
}

/// `LOCALAPPDATA` on Windows, else `dirs::data_local_dir`.
pub fn data_local_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(local) = env("LOCALAPPDATA") {
        return Some(local);
    }
    dirs::data_local_dir()
}

/// `APPDATA` on Windows, else `dirs::data_dir`.
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(roaming) = env("APPDATA") {
        return Some(roaming);
    }
    dirs::data_dir()
}

/// The profile folder `USERPROFILE` names when it is not the signed-in
/// user's own, so known folders under it are the copy's.
#[cfg(windows)]
fn redirected_profile() -> Option<PathBuf> {
    let profile = env("USERPROFILE")?;
    let own = dirs::home_dir()?;
    let same = |a: &PathBuf, b: &PathBuf| {
        a.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
    };
    (!same(&profile, &own)).then_some(profile)
}

/// `dirs::document_dir`, or `Documents` in a redirected Windows profile.
pub fn document_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(profile) = redirected_profile() {
        return Some(profile.join("Documents"));
    }
    dirs::document_dir()
}

/// `dirs::download_dir`, or `Downloads` in a redirected Windows profile.
pub fn download_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(profile) = redirected_profile() {
        return Some(profile.join("Downloads"));
    }
    dirs::download_dir()
}
