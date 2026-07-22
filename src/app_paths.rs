use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};

pub fn support_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HEX_APPLICATION_SUPPORT_DIR") {
        return Ok(path.into());
    }
    Ok(dirs::data_dir()
        .ok_or_else(|| eyre!("application data directory is unavailable"))?
        .join("voice-control"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("logs"))
}

pub fn local_api_discovery_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("local-api.json"))
}

#[cfg(target_os = "macos")]
pub fn personal_commands_status_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("personal-commands.json"))
}

#[cfg(target_os = "macos")]
pub fn personal_commands_workspace() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| eyre!("home directory is unavailable"))?
        .join(".config/hex"))
}

#[cfg(target_os = "macos")]
pub fn personal_commands_host() -> Result<PathBuf> {
    let workspace_host =
        personal_commands_workspace()?.join("node_modules/@hex/commands/dist/bin.js");
    if workspace_host.is_file() {
        return Ok(workspace_host);
    }
    Err(eyre!(
        "personal command SDK is not installed; run `hex commands init`"
    ))
}

#[cfg(target_os = "macos")]
pub fn personal_commands_sdk() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    if let Some(contents) = executable.parent().and_then(|path| path.parent()) {
        let bundled = contents.join("Resources/commands-sdk");
        if bundled.join("dist/bin.js").is_file() {
            return Ok(bundled);
        }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sdk/commands");
    if source.join("dist/bin.js").is_file() {
        Ok(source)
    } else {
        Err(eyre!("personal command SDK resources were not found"))
    }
}
