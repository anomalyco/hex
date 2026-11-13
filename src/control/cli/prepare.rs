//! A model downloaded and loaded through a local API's
//! `POST /models/{id}/prepare`: the running Hex's or an owned embedded
//! service's, with the download shown on a terminal's stderr.

use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::time::Duration;

use super::progress;
use crate::control::ControlReply;
use crate::local_api::Endpoint;
use crate::transcription::models::{TranscriptionSelection, definition};

/// The stream sends a keep-alive every 5 s; quieter than this is a stall.
const IDLE: Duration = Duration::from_secs(60);
const MAX_EVENT: usize = 64 * 1024;

/// Whether the person at the terminal agrees to download `selection`'s
/// model. Asked once, on stderr, only when stdin and stderr are both a
/// terminal and the reply isn't JSON; anything but Enter or yes declines.
pub(super) fn consent(selection: &TranscriptionSelection, json: bool) -> bool {
    if json || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return false;
    }
    let model = definition(selection.model);
    progress::clear();
    let mut stderr = std::io::stderr();
    let _ = write!(
        stderr,
        "Download {} ({})? [Y/n] ",
        model.name,
        model.size_label()
    );
    let _ = stderr.flush();
    let mut answer = String::new();
    match std::io::stdin().lock().read_line(&mut answer) {
        Ok(0) | Err(_) => {
            let _ = writeln!(stderr);
            false
        }
        Ok(_) => agrees(&answer),
    }
}

fn agrees(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    )
}

/// Downloads, verifies, and loads `selection`'s model in `endpoint`'s
/// service, until it reports `ok`. Nothing about the selected model changes.
pub(super) fn prepare(
    endpoint: &Endpoint,
    selection: &TranscriptionSelection,
) -> Result<(), ControlReply> {
    let model = definition(selection.model);
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("language", &selection.language)
        .finish();
    let path = format!("/models/{}/prepare?{query}", selection.model.as_str());
    let mut outcome = None;
    let answer = endpoint.events(&path, IDLE, MAX_EVENT, |event| {
        match event.get("type").and_then(|kind| kind.as_str()) {
            Some("downloading") => {
                let done = event["downloadedBytes"].as_u64().unwrap_or(0);
                let total = event["totalBytes"].as_u64().unwrap_or(0);
                progress::show(&if total > 0 {
                    format!(
                        "Downloading {}… {} of {}",
                        model.name,
                        progress::megabytes(done),
                        progress::megabytes(total)
                    )
                } else {
                    format!("Downloading {}…", model.name)
                });
                false
            }
            Some("verifying") => {
                progress::show(&format!("Checking {}…", model.name));
                false
            }
            Some("loading") => {
                progress::show(&format!("Loading {}…", model.name));
                false
            }
            Some("ok") => {
                outcome = Some(Ok(()));
                true
            }
            Some("error") => {
                let code = event["error"]["code"]
                    .as_str()
                    .unwrap_or("failed")
                    .to_owned();
                outcome = Some(Err(code));
                true
            }
            _ => false,
        }
    });
    progress::clear();
    let name = model.name;
    match answer {
        Ok((200, _)) => match outcome {
            Some(Ok(())) => Ok(()),
            Some(Err(code)) => Err(failed(name, &code)),
            None => Err(ControlReply::error(
                "failed",
                format!("Hex stopped preparing {name} before it finished; try again"),
            )),
        },
        Ok((status, body)) => {
            let code = crate::local_api::error_code(status, &body);
            Err(match code.as_str() {
                "prepare-busy" => ControlReply::error(
                    "busy",
                    "Hex is already preparing a model; try again once it finishes",
                ),
                "unknown-model" => ControlReply::error(
                    "unknown-model",
                    format!(
                        "Hex doesn't know `{}`; run `hex model list`",
                        selection.model.as_str()
                    ),
                ),
                "unsupported-model" => ControlReply::error(
                    "model-unavailable",
                    format!("{name} doesn't support `{}`", selection.language),
                ),
                _ => {
                    ControlReply::error("failed", format!("Hex refused to prepare {name} ({code})"))
                }
            })
        }
        Err(error) => Err(ControlReply::error("transport", format!("{error:#}"))),
    }
}

fn failed(name: &str, code: &str) -> ControlReply {
    let what = match code {
        "download-failed" => "couldn't download",
        "verification-failed" => "couldn't verify the download of",
        "load-failed" => "couldn't load",
        "cancelled" => "stopped preparing",
        _ => "couldn't prepare",
    };
    ControlReply::error("failed", format!("Hex {what} {name}; try again"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_or_yes_agrees_and_anything_else_declines() {
        for answer in ["\n", "y\n", "Y\n", "yes\n", " YES \r\n"] {
            assert!(agrees(answer), "{answer:?}");
        }
        for answer in ["n\n", "no\n", "nope\n", "q\n"] {
            assert!(!agrees(answer), "{answer:?}");
        }
    }

    #[test]
    fn json_never_asks() {
        let selection = TranscriptionSelection::default();
        assert!(!consent(&selection, true));
    }
}
