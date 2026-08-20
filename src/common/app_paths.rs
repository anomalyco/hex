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

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn opencode_workspace() -> Result<PathBuf> {
    Ok(support_dir()?.join("opencode"))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn local_api_discovery_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("local-api.json"))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn personal_commands_status_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("personal-commands.json"))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn personal_commands_workspace() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| eyre!("home directory is unavailable"))?
        .join(".config/hex"))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn personal_commands_host() -> Result<PathBuf> {
    let package = personal_commands_workspace()?.join("node_modules/@hex/commands");
    for relative in ["dist/bin.js", "src/bin.ts"] {
        let workspace_host = package.join(relative);
        if workspace_host.is_file() {
            return Ok(workspace_host);
        }
    }
    Err(eyre!(
        "personal command SDK is not installed; initialize the personal command workspace"
    ))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn personal_commands_sdk() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    if let Some(contents) = executable.parent().and_then(|path| path.parent()) {
        let bundled = contents.join("Resources/commands-sdk");
        if bundled.join("dist/bin.js").is_file() {
            return Ok(bundled);
        }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sdk/commands");
    if source.join("dist/bin.js").is_file() || source.join("src/bin.ts").is_file() {
        Ok(source)
    } else {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            materialize_embedded_personal_commands_sdk()
        }
        #[cfg(target_os = "macos")]
        {
            Err(eyre!("personal command SDK resources were not found"))
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn materialize_embedded_personal_commands_sdk() -> Result<PathBuf> {
    materialize_embedded_personal_commands_sdk_at(&support_dir()?.join("commands-sdk"))
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn materialize_embedded_personal_commands_sdk_at(root: &std::path::Path) -> Result<PathBuf> {
    use std::fmt::Write as _;
    use std::fs;

    use sha2::{Digest, Sha256};

    const FILES: &[(&str, &[u8])] = &[
        (
            "package.json",
            include_bytes!("../../sdk/commands/package.json"),
        ),
        (
            "src/bin.ts",
            include_bytes!("../../sdk/commands/src/bin.ts"),
        ),
        (
            "src/effect.ts",
            include_bytes!("../../sdk/commands/src/effect.ts"),
        ),
        (
            "src/host.ts",
            include_bytes!("../../sdk/commands/src/host.ts"),
        ),
        (
            "src/index.ts",
            include_bytes!("../../sdk/commands/src/index.ts"),
        ),
        (
            "src/model.ts",
            include_bytes!("../../sdk/commands/src/model.ts"),
        ),
        (
            "src/protocol.ts",
            include_bytes!("../../sdk/commands/src/protocol.ts"),
        ),
        (
            "workspace-template/AGENTS.md",
            include_bytes!("../../sdk/commands/workspace-template/AGENTS.md"),
        ),
        (
            "workspace-template/hex.config.ts",
            include_bytes!("../../sdk/commands/workspace-template/hex.config.ts"),
        ),
        (
            "workspace-template/package.json",
            include_bytes!("../../sdk/commands/workspace-template/package.json"),
        ),
        (
            "workspace-template/tsconfig.json",
            include_bytes!("../../sdk/commands/workspace-template/tsconfig.json"),
        ),
        (
            "workspace-template/.agents/skills/personal-commands/SKILL.md",
            include_bytes!(
                "../../sdk/commands/workspace-template/.agents/skills/personal-commands/SKILL.md"
            ),
        ),
    ];

    let mut hasher = Sha256::new();
    for (path, bytes) in FILES {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
    }
    let mut fingerprint = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to a string cannot fail");
    }
    let root = root.join(&fingerprint[..16]);
    for (relative, bytes) in FILES {
        let destination = root.join(relative);
        if fs::read(&destination).is_ok_and(|existing| existing == *bytes) {
            continue;
        }
        let parent = destination
            .parent()
            .ok_or_else(|| eyre!("embedded SDK path has no parent"))?;
        fs::create_dir_all(parent)?;
        let temporary = destination.with_extension(format!("{}.partial", std::process::id()));
        fs::write(&temporary, bytes)?;
        crate::transcription_models::atomic_replace(&temporary, &destination)?;
    }
    Ok(root)
}

#[cfg(all(test, any(target_os = "linux", target_os = "windows")))]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn embedded_personal_commands_sdk_materializes_and_repairs_resources() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("hex-embedded-sdk-{unique}"));

        let sdk = materialize_embedded_personal_commands_sdk_at(&root).unwrap();
        assert!(sdk.join("src/bin.ts").is_file());
        assert!(sdk.join("src/index.ts").is_file());
        assert!(sdk.join("workspace-template/hex.config.ts").is_file());
        assert!(
            sdk.join("workspace-template/.agents/skills/personal-commands/SKILL.md")
                .is_file()
        );

        fs::write(sdk.join("src/bin.ts"), "stale").unwrap();
        assert_eq!(
            materialize_embedded_personal_commands_sdk_at(&root).unwrap(),
            sdk
        );
        assert_eq!(
            fs::read(sdk.join("src/bin.ts")).unwrap(),
            include_bytes!("../../sdk/commands/src/bin.ts")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
