//! The desktop's screen-cast portal (`org.freedesktop.portal.ScreenCast`): one session per
//! recording, the desktop's own picker for what to record, and its PipeWire remote. A screen
//! keeps the portal's restore token, so the desktop asks once and remembers the choice; a
//! window is picked each time. Closing the session (dropping [`Cast`]) ends the stream.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use zbus::{Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant, message};

use crate::capture::{OutputBounds, PortalToken};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREEN_CAST: &str = "org.freedesktop.portal.ScreenCast";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Hex's app id, named by the desktop entry the host installs (`anomaly.hex.desktop`).
const APP_ID: &str = "anomaly.hex";
/// How long the desktop's picker may wait for a choice.
const PICK_TIMEOUT: Duration = Duration::from_secs(120);
const SOURCE_MONITOR: u32 = 1;
const SOURCE_WINDOW: u32 = 2;
const CURSOR_EMBEDDED: u32 = 2;
/// Kept until the person revokes it.
const PERSIST_UNTIL_REVOKED: u32 = 2;

/// What the portal asks the person to pick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// A whole screen, remembered after the first pick.
    Screen,
    /// A window, picked each time.
    Window,
}

/// A started screen cast: its PipeWire remote and node, the stream's size and position in the
/// desktop's logical coordinates when it reports them, and the output it records when the
/// desktop names one (a monitor pick).
pub struct Cast {
    pub remote: Option<OwnedFd>,
    pub node: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
    /// The picked monitor's connector name, such as `DP-3` (Hyprland's portal, 1.4.0+, sets
    /// it for monitor picks). Window picks carry none.
    pub mapping_id: Option<String>,
    runtime: tokio::runtime::Runtime,
    connection: Connection,
    session: OwnedObjectPath,
}

impl Drop for Cast {
    fn drop(&mut self) {
        let (connection, session) = (self.connection.clone(), self.session.clone());
        self.runtime.block_on(async move {
            let _ = tokio::time::timeout(
                CALL_TIMEOUT,
                connection.call_method(Some(DESKTOP), &session, Some(SESSION), "Close", &()),
            )
            .await;
        });
    }
}

type Results = HashMap<String, OwnedVariant>;

struct Portal {
    connection: Connection,
    responses: MessageStream,
    sender: String,
    next: u32,
}

impl Portal {
    async fn connect() -> Result<Self> {
        let connection = tokio::time::timeout(CALL_TIMEOUT, Connection::session())
            .await
            .context("the session bus did not answer")?
            .context("no session bus")?;
        let rule = MatchRule::builder()
            .msg_type(message::Type::Signal)
            .interface(REQUEST)
            .member("Response")
            .build()?;
        let responses = MessageStream::for_match_rule(rule, &connection, None).await?;
        // A host app has no app id of its own; registering one (Hex's
        // desktop entry) is what lets the portal keep its screen choice.
        // Older portals lack the registry, and then ask every time.
        let _ = tokio::time::timeout(
            CALL_TIMEOUT,
            connection.call_method(
                Some(DESKTOP),
                DESKTOP_PATH,
                Some("org.freedesktop.host.portal.Registry"),
                "Register",
                &(APP_ID, HashMap::<&str, Variant>::new()),
            ),
        )
        .await;
        let sender = connection
            .unique_name()
            .ok_or_else(|| anyhow!("the session bus gave Hex no name"))?
            .trim_start_matches(':')
            .replace('.', "_");
        Ok(Self {
            connection,
            responses,
            sender,
            next: 0,
        })
    }

    fn token(&mut self) -> String {
        self.next += 1;
        format!("hex{}_{}", std::process::id(), self.next)
    }

    /// Calls a ScreenCast method whose answer comes as a `Request.Response`, and waits for it.
    async fn request<B>(
        &mut self,
        method: &str,
        body: impl FnOnce(String) -> B,
        timeout: Duration,
        stop: &AtomicBool,
    ) -> Result<Results>
    where
        B: serde::Serialize + zbus::DynamicType,
    {
        let token = self.token();
        let path = format!("{DESKTOP_PATH}/request/{}/{token}", self.sender);
        tokio::time::timeout(
            CALL_TIMEOUT,
            self.connection.call_method(
                Some(DESKTOP),
                DESKTOP_PATH,
                Some(SCREEN_CAST),
                method,
                &body(token),
            ),
        )
        .await
        .with_context(|| format!("the screen-cast portal did not answer {method}"))?
        .with_context(|| format!("the screen-cast portal refused {method}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            if stop.load(Ordering::Acquire) {
                let _ = self
                    .connection
                    .call_method(Some(DESKTOP), path.as_str(), Some(REQUEST), "Close", &())
                    .await;
                bail!("the capture stopped before the screen was picked");
            }
            if Instant::now() >= deadline {
                bail!("nothing was picked to record");
            }
            let Ok(next) =
                tokio::time::timeout(Duration::from_millis(100), self.responses.next()).await
            else {
                continue;
            };
            let message = next.ok_or_else(|| anyhow!("the session bus closed"))??;
            if message.header().path().map(|p| p.as_str()) != Some(path.as_str()) {
                continue;
            }
            let (code, results): (u32, Results) = message.body().deserialize()?;
            return match code {
                0 => Ok(results),
                1 => bail!("the screen pick was cancelled"),
                _ => bail!("the screen-cast portal failed {method}"),
            };
        }
    }
}

fn variant<'a>(value: impl Into<Variant<'a>>) -> Variant<'a> {
    value.into()
}

/// Asks the desktop for `source` and starts casting it. Blocks while the picker shows, until
/// `stop` is set. `tokens` says where the screen's remembered choice lives: one file for every
/// screen, or one per output for the output under the pointer.
pub fn start(source: Source, stop: &AtomicBool, tokens: &PortalToken) -> Result<Cast> {
    let read = (source == Source::Screen)
        .then(|| token_file(tokens))
        .flatten();
    let restore = read
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty());
    match start_with(source, stop, tokens, read.clone(), restore.clone()) {
        // A revoked or stale choice: forget it and ask again.
        Err(error) if restore.is_some() && !stop.load(Ordering::Acquire) => {
            crate::log::scrawl_log(format!(
                "the remembered screen was refused ({error:#}); asking again"
            ));
            if let Some(path) = &read {
                let _ = std::fs::remove_file(path);
            }
            start_with(source, stop, tokens, read, None)
        }
        result => result,
    }
}

/// The file a screen's remembered choice is asked from: the one file, or the
/// pointer's output's file. `None` keeps none.
fn token_file(tokens: &PortalToken) -> Option<PathBuf> {
    match tokens {
        PortalToken::None => None,
        PortalToken::File(path) => Some(path.clone()),
        PortalToken::PerOutput { dir, pointer, .. } => Some(dir.join(token_file_name(pointer))),
    }
}

/// The file a screen's new token goes in: the one file, or the file of the
/// output the desktop actually recorded (its `mapping_id`, else the output
/// whose logical geometry holds the stream's position, else the pointer's).
fn token_target(source: Source, tokens: &PortalToken, cast: &Cast) -> Option<PathBuf> {
    if source != Source::Screen {
        return None;
    }
    match tokens {
        PortalToken::None => None,
        PortalToken::File(path) => Some(path.clone()),
        PortalToken::PerOutput {
            dir,
            pointer,
            outputs,
        } => {
            let name = recorded_output(
                cast.mapping_id.as_deref(),
                cast.position,
                cast.size,
                outputs,
            )
            .unwrap_or_else(|| pointer.clone());
            Some(dir.join(token_file_name(&name)))
        }
    }
}

/// The output the stream came from, as the desktop named it or as its logical
/// geometry matches the stream's position and size.
fn recorded_output(
    mapping_id: Option<&str>,
    position: Option<(i32, i32)>,
    size: Option<(i32, i32)>,
    outputs: &[OutputBounds],
) -> Option<String> {
    if let Some(name) = mapping_id.filter(|name| !name.is_empty()) {
        return Some(name.to_owned());
    }
    let position = position?;
    let exact = outputs
        .iter()
        .find(|output| output.position == position && size.is_none_or(|size| output.size == size));
    let containing = exact.or_else(|| {
        outputs.iter().find(|output| {
            let (x, y) = output.position;
            position.0 >= x
                && position.0 < x + output.size.0
                && position.1 >= y
                && position.1 < y + output.size.1
        })
    });
    containing.map(|output| output.name.clone())
}

/// `name` as a file name: every character but `A-Za-z0-9_-` becomes `_`.
fn token_file_name(name: &str) -> String {
    let mut file = String::from("screencast-token-");
    for character in name.chars() {
        file.push(
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            },
        );
    }
    if file.len() == "screencast-token-".len() {
        file.push_str("output");
    }
    file
}

fn start_with(
    source: Source,
    stop: &AtomicBool,
    tokens: &PortalToken,
    token_file: Option<PathBuf>,
    restore: Option<String>,
) -> Result<Cast> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the screen-cast portal runtime")?;
    let started = runtime.block_on(async {
        let mut portal = Portal::connect().await?;
        let session_token = portal.token();
        let created = portal
            .request(
                "CreateSession",
                |token| {
                    (HashMap::from([
                        ("handle_token", variant(token)),
                        ("session_handle_token", variant(session_token)),
                    ]),)
                },
                CALL_TIMEOUT,
                stop,
            )
            .await?;
        let session: String = created
            .get("session_handle")
            .ok_or_else(|| anyhow!("the portal made no session"))?
            .try_clone()?
            .try_into()?;
        let session = OwnedObjectPath::try_from(session)?;
        let picked = async {
            portal
                .request(
                    "SelectSources",
                    |token| {
                        let mut options = HashMap::from([
                            ("handle_token", variant(token)),
                            (
                                "types",
                                variant(match source {
                                    Source::Screen => SOURCE_MONITOR,
                                    Source::Window => SOURCE_WINDOW,
                                }),
                            ),
                            ("multiple", variant(false)),
                            ("cursor_mode", variant(CURSOR_EMBEDDED)),
                        ]);
                        if source == Source::Screen && token_file.is_some() {
                            options.insert("persist_mode", variant(PERSIST_UNTIL_REVOKED));
                        }
                        if let Some(restore) = restore.clone() {
                            options.insert("restore_token", variant(restore));
                        }
                        (&session, options)
                    },
                    CALL_TIMEOUT,
                    stop,
                )
                .await?;
            let results = portal
                .request(
                    "Start",
                    |token| {
                        (
                            &session,
                            "",
                            HashMap::from([("handle_token", variant(token))]),
                        )
                    },
                    PICK_TIMEOUT,
                    stop,
                )
                .await?;
            let remote: zbus::OwnedFd = tokio::time::timeout(
                CALL_TIMEOUT,
                portal.connection.call_method(
                    Some(DESKTOP),
                    DESKTOP_PATH,
                    Some(SCREEN_CAST),
                    "OpenPipeWireRemote",
                    &(&session, HashMap::<&str, Variant>::new()),
                ),
            )
            .await
            .context("the portal did not open PipeWire")??
            .body()
            .deserialize()?;
            anyhow::Ok((results, OwnedFd::from(remote)))
        }
        .await;
        anyhow::Ok((portal.connection, session, picked))
    })?;
    let (connection, session, picked) = started;
    let mut cast = Cast {
        remote: None,
        node: 0,
        size: None,
        position: None,
        mapping_id: None,
        runtime,
        connection,
        session,
    };
    // From here dropping `cast` closes the session.
    let (results, remote) = picked?;
    cast.remote = Some(remote);
    let streams: Vec<(u32, Results)> = results
        .get("streams")
        .ok_or_else(|| anyhow!("the portal started no stream"))?
        .try_clone()?
        .try_into()?;
    let (node, props) = streams
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("the portal started no stream"))?;
    let pair =
        |key: &str| -> Option<(i32, i32)> { props.get(key)?.try_clone().ok()?.try_into().ok() };
    cast.node = node;
    cast.size = pair("size");
    cast.position = pair("position");
    // A string beside `size` and `position`; absent or another type for a window.
    cast.mapping_id = props
        .get("mapping_id")
        .and_then(|value| value.try_clone().ok()?.try_into().ok());
    // Saved for the output the desktop recorded, once it has said which.
    if let (Some(path), Some(token)) = (
        token_target(source, tokens, &cast),
        results.get("restore_token"),
    ) {
        let token: Result<String, _> = token
            .try_clone()
            .map_err(anyhow::Error::from)
            .and_then(|t| Ok(t.try_into()?));
        if let Ok(token) = token
            && let Err(error) = write_private(&path, &token)
        {
            crate::log::scrawl_log(format!("could not keep the screen choice: {error:#}"));
        }
    }
    Ok(cast)
}

fn write_private(path: &std::path::Path, token: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OutputBounds, recorded_output, token_file_name};

    fn output(name: &str, position: (i32, i32), size: (i32, i32)) -> OutputBounds {
        OutputBounds {
            name: name.to_owned(),
            position,
            size,
        }
    }

    #[test]
    fn the_desktop_s_own_name_wins() {
        let outputs = [output("DP-3", (0, 0), (2560, 1440))];
        assert_eq!(
            recorded_output(Some("HDMI-A-1"), Some((0, 0)), Some((2560, 1440)), &outputs),
            Some("HDMI-A-1".to_owned())
        );
        assert_eq!(
            recorded_output(Some(""), Some((0, 0)), None, &outputs),
            Some("DP-3".to_owned())
        );
    }

    #[test]
    fn the_stream_position_and_size_name_an_output() {
        let outputs = [
            output("DP-3", (0, 0), (2560, 1440)),
            output("HDMI-A-1", (2560, 0), (1920, 1080)),
        ];
        assert_eq!(
            recorded_output(None, Some((2560, 0)), Some((1920, 1080)), &outputs),
            Some("HDMI-A-1".to_owned())
        );
        // A stream that reports its corner falls on the output holding it.
        assert_eq!(
            recorded_output(None, Some((3000, 200)), Some((1280, 720)), &outputs),
            Some("HDMI-A-1".to_owned())
        );
    }

    #[test]
    fn an_unplaced_stream_names_nothing() {
        let outputs = [output("DP-3", (0, 0), (2560, 1440))];
        assert_eq!(
            recorded_output(None, None, Some((2560, 1440)), &outputs),
            None
        );
        assert_eq!(
            recorded_output(None, Some((5000, 5000)), None, &outputs),
            None
        );
    }

    #[test]
    fn a_token_file_keeps_the_output_readable() {
        assert_eq!(token_file_name("DP-3"), "screencast-token-DP-3".to_owned());
        assert_eq!(
            token_file_name("HDMI/A-1"),
            "screencast-token-HDMI_A-1".to_owned()
        );
        assert_eq!(token_file_name("a b"), "screencast-token-a_b".to_owned());
        assert_eq!(token_file_name("../x"), "screencast-token-___x".to_owned());
        assert_eq!(token_file_name(""), "screencast-token-output".to_owned());
        assert_eq!(token_file_name("日本"), "screencast-token-__".to_owned());
    }
}
