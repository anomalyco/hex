//! Voice Action on Windows: a held shortcut records an instruction, the
//! transcription plus the focused application's selected text go to the
//! local OpenCode service, and its reply is pasted at the cursor.
//!
//! This is a minimal blocking client for the same `opencode2 api` CLI the
//! macOS processor drives; the two should eventually share one client in
//! the common layer.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const GENERATION_DEADLINE: Duration = Duration::from_secs(45);
const PROTOCOL_PROMPT: &str = "You execute a one-off voice instruction. When selected text is provided, transform or use it as instructed. When no text is selected, generate the requested text. Return only the exact paste-ready result without an explanation, label, alternative, or Markdown fence.";

#[derive(Serialize)]
struct Request<'a> {
    prompt: &'a str,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Response {
    Success { data: ResponseData },
    Error { message: String },
}

#[derive(Deserialize)]
struct ResponseData {
    text: String,
}

pub fn opencode_installed() -> bool {
    opencode_executable().is_some()
}

/// The prompt shape matches the macOS processor so both platforms steer the
/// model identically; Windows has no browser-host detection yet.
pub fn voice_action_prompt(
    instruction: &str,
    application: Option<&str>,
    selected_text: Option<&str>,
) -> String {
    let application = application.unwrap_or("unknown");
    let selection = selected_text.map_or_else(
        || "No text was selected. Generate the requested paste-ready text.".into(),
        |text| format!("Selected text ({} UTF-8 bytes):\n{text}", text.len()),
    );
    format!(
        "{PROTOCOL_PROMPT}\n\nForeground application: {application}\nBrowser host: none\n\nDictated instruction ({} UTF-8 bytes):\n{instruction}\n\n{selection}",
        instruction.len()
    )
}

/// Ask OpenCode's default model to fulfil the prompt, bounded by `deadline`.
pub fn generate(prompt: &str, deadline: Duration) -> Result<String> {
    let executable =
        opencode_executable().ok_or_else(|| eyre!("OpenCode (opencode2) is not installed"))?;
    let workspace = crate::app_paths::opencode_workspace()?;
    std::fs::create_dir_all(&workspace).wrap_err("could not create the OpenCode workspace")?;
    let workspace = workspace
        .canonicalize()
        .wrap_err("could not resolve the OpenCode workspace")?;
    let directory = workspace
        .to_str()
        .ok_or_else(|| eyre!("the OpenCode workspace path is not valid UTF-8"))?;
    let directory = encode_uri_component(directory);
    let data = serde_json::to_string(&Request { prompt })?;
    let mut child = Command::new(&executable)
        .current_dir(&workspace)
        .args([
            "api",
            "--header",
            &format!("x-opencode-directory:{directory}"),
            "post",
            "/api/generate",
            "--data",
            &data,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .wrap_err("could not start opencode2")?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(eyre!(
                "OpenCode did not answer within {} seconds",
                deadline.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = child
        .wait_with_output()
        .wrap_err("could not read opencode2 output")?;
    if !status.success() {
        return Err(eyre!(
            "opencode2 exited with {status}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let response: Response = serde_json::from_slice(&output.stdout)
        .wrap_err("opencode2 returned an invalid generation response")?;
    match response {
        Response::Success { data } if data.text.trim().is_empty() => {
            Err(eyre!("OpenCode returned empty text"))
        }
        Response::Success { data } => Ok(data.text),
        Response::Error { message } => Err(eyre!("OpenCode generation failed: {message}")),
    }
}

fn opencode_executable() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("VOICE_CONTROL_OPENCODE_CLI") {
        return Some(PathBuf::from(configured));
    }
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        for candidate in [
            "opencode2.cmd",
            "opencode2.exe",
            "opencode2.bat",
            "opencode2",
        ] {
            let candidate = directory.join(candidate);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_carry_selection_and_instruction_sizes() {
        let prompt = voice_action_prompt("make it shorter", Some("code"), Some("hello"));
        assert!(prompt.contains("Foreground application: code"));
        assert!(prompt.contains("Dictated instruction (15 UTF-8 bytes)"));
        assert!(prompt.contains("Selected text (5 UTF-8 bytes):\nhello"));

        let prompt = voice_action_prompt("write a haiku", None, None);
        assert!(prompt.contains("Foreground application: unknown"));
        assert!(prompt.contains("No text was selected."));
    }

    #[test]
    fn uri_components_escape_reserved_characters() {
        assert_eq!(
            encode_uri_component("C:\\Users\\x y"),
            "C%3A%5CUsers%5Cx%20y"
        );
        assert_eq!(encode_uri_component("abc123"), "abc123");
    }
}
