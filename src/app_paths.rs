use std::path::PathBuf;

use color_eyre::eyre::{Result, eyre};

pub fn support_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HEX_APPLICATION_SUPPORT_DIR") {
        return Ok(path.into());
    }
    Ok(dirs::data_dir()
        .ok_or_else(|| eyre!("macOS application support directory is unavailable"))?
        .join("voice-control"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("logs"))
}
