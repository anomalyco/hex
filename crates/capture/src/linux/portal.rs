//! The desktop's screen-cast portal (`org.freedesktop.portal.ScreenCast`): one session per
//! recording, the desktop's own picker for what to record, and its PipeWire remote. A screen
//! keeps the portal's restore token, so the desktop asks once and remembers the choice; a
//! window is picked each time. Closing the session (dropping [`Cast`]) ends the stream.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use zbus::{Connection, MatchRule, MessageStream, OwnedObjectPath, OwnedVariant, Variant, message};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREEN_CAST: &str = "org.freedesktop.portal.ScreenCast";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The desktop entry the host installs (`hex.desktop`).
const APP_ID: &str = "hex";
/// How long the desktop's picker may wait for a choice.
const PICK_TIMEOUT: Duration = Duration::from_secs(120);
const SOURCE_MONITOR: u32 = 1;
const SOURCE_WINDOW: u32 = 2;
const CURSOR_EMBEDDED: u32 = 2;
/// Kept until the person revokes it.
const PERSIST_UNTIL_REVOKED: u32 = 2;

/// Where the screen's restore token lives; `None` (the default) keeps none.
static TOKEN_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Keeps the screen's restore token at `path`, so the picker shows once.
pub fn set_token_path(path: PathBuf) {
    *TOKEN_PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

fn token_path() -> Option<PathBuf> {
    TOKEN_PATH.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What the portal asks the person to pick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// A whole screen, remembered after the first pick.
    Screen,
    /// A window, picked each time.
    Window,
}

/// A started screen cast: its PipeWire remote and node, and the stream's size and position in
/// the desktop's logical coordinates when it reports them.
pub struct Cast {
    pub remote: Option<OwnedFd>,
    pub node: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
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
/// `stop` is set.
pub fn start(source: Source, stop: &AtomicBool) -> Result<Cast> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the screen-cast portal runtime")?;
    let token_file = (source == Source::Screen).then(token_path).flatten();
    let restore = token_file
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty());
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
                        if token_file.is_some() {
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
        runtime,
        connection,
        session,
    };
    // From here dropping `cast` closes the session.
    let (results, remote) = picked?;
    cast.remote = Some(remote);
    if let (Some(path), Some(token)) = (token_file, results.get("restore_token")) {
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
