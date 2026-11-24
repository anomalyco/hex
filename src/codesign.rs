//! The `/usr/bin/codesign` check the Hex Microphone driver install makes
//! before it asks for administrator rights: the driver bundle must satisfy
//! `app/driver-requirement.txt`, a designated requirement that pins its
//! identifier, Anomaly's Developer ID anchor, and the team. The updater keeps
//! its own copy of the same `codesign` invocation.

use std::path::Path;
use std::process::{Command, Stdio};

/// Verifies `bundle` against `requirement` with
/// `codesign --verify --deep --strict -R`, so nested code and every resource
/// must match too. `Ok` means the signature satisfies the requirement; `Err`
/// carries codesign's own complaint, or why it could not run.
pub fn verify(bundle: &Path, requirement: &str) -> Result<(), String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={}", requirement.trim()))
        .arg(bundle)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not run codesign: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        return Err(format!("codesign did not accept it ({})", output.status));
    }
    Err(detail.to_owned())
}
