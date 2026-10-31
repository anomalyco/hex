//! Microphone users on Linux: PipeWire's running capture streams, watched
//! through one long-running `pw-dump --monitor`. Streams that capture an
//! output's monitor (`stream.capture.sink`), such as Hex's own meeting
//! system audio, are not microphone users. Only stream metadata is read;
//! no audio is captured.

use std::collections::HashMap;
use std::io::BufReader;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde_json::Value;

use super::{ActiveMicrophoneApplication, deduplicated};

const INPUT_STREAM: &str = "Stream/Input/Audio";

#[derive(Clone, Debug, Default, PartialEq)]
struct Node {
    input: bool,
    running: bool,
    client: Option<u64>,
    process: Process,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Process {
    pid: Option<i32>,
    binary: Option<String>,
    name: Option<String>,
}

impl Process {
    fn from_props(props: &Value) -> Self {
        let text = |key: &str| props.get(key).and_then(Value::as_str).map(str::to_owned);
        Self {
            pid: props.get("application.process.id").and_then(|pid| {
                pid.as_i64()
                    .or_else(|| pid.as_str().and_then(|pid| pid.parse().ok()))
                    .and_then(|pid| i32::try_from(pid).ok())
            }),
            binary: text("application.process.binary"),
            name: text("application.name"),
        }
    }

    /// Node properties, completed by the client's where the node lacks them.
    fn or(&self, other: &Process) -> Process {
        Process {
            pid: self.pid.or(other.pid),
            binary: self.binary.clone().or_else(|| other.binary.clone()),
            name: self.name.clone().or_else(|| other.name.clone()),
        }
    }
}

/// The PipeWire graph as far as microphone use goes.
#[derive(Default)]
struct Graph {
    nodes: HashMap<u64, Node>,
    clients: HashMap<u64, Process>,
}

impl Graph {
    /// Applies one object of `pw-dump` output: a node or client added or
    /// changed, or any object removed (`"info": null`).
    fn apply(&mut self, object: &Value) {
        let Some(id) = object.get("id").and_then(Value::as_u64) else {
            return;
        };
        let info = object.get("info").filter(|info| !info.is_null());
        let Some(info) = info else {
            self.nodes.remove(&id);
            self.clients.remove(&id);
            return;
        };
        let props = info.get("props").cloned().unwrap_or(Value::Null);
        match object.get("type").and_then(Value::as_str) {
            Some("PipeWire:Interface:Node") => {
                let class = props.get("media.class").and_then(Value::as_str);
                let monitor = props.get("stream.capture.sink").is_some_and(|sink| {
                    sink.as_bool() == Some(true) || sink.as_str() == Some("true")
                });
                self.nodes.insert(
                    id,
                    Node {
                        input: class == Some(INPUT_STREAM) && !monitor,
                        running: info.get("state").and_then(Value::as_str) == Some("running"),
                        client: props.get("client.id").and_then(|client| {
                            client
                                .as_u64()
                                .or_else(|| client.as_str().and_then(|client| client.parse().ok()))
                        }),
                        process: Process::from_props(&props),
                    },
                );
            }
            Some("PipeWire:Interface:Client") => {
                self.clients.insert(id, Process::from_props(&props));
            }
            _ => {}
        }
    }

    fn microphone_users(&self, own_pid: i32) -> Vec<ActiveMicrophoneApplication> {
        let applications = self
            .nodes
            .values()
            .filter(|node| node.input && node.running)
            .filter_map(|node| {
                let client = node
                    .client
                    .and_then(|client| self.clients.get(&client))
                    .cloned()
                    .unwrap_or_default();
                let process = node.process.or(&client);
                let pid = process.pid?;
                let binary = process.binary?;
                (pid != own_pid).then(|| ActiveMicrophoneApplication {
                    pid,
                    name: process.name.unwrap_or_else(|| binary.clone()),
                    bundle_id: binary,
                })
            })
            .collect();
        deduplicated(applications)
    }
}

struct Monitor {
    child: Child,
    graph: Arc<Mutex<Graph>>,
}

impl Monitor {
    fn start() -> Result<Self> {
        let mut command = Command::new("pw-dump");
        command
            .arg("--monitor")
            .arg("--no-colors")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: `prctl` is async-signal-safe; it ends the monitor with the
        // thread that started it, the meeting controller.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .wrap_err("could not watch PipeWire (is pw-dump installed?)")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("pw-dump has no output"))?;
        let graph = Arc::new(Mutex::new(Graph::default()));
        let shared = graph.clone();
        thread::Builder::new()
            .name("microphone-activity".into())
            .spawn(move || {
                let updates = serde_json::Deserializer::from_reader(BufReader::new(stdout))
                    .into_iter::<Vec<Value>>();
                for update in updates {
                    let Ok(objects) = update else {
                        return;
                    };
                    let mut graph = shared.lock().unwrap_or_else(|error| error.into_inner());
                    for object in &objects {
                        graph.apply(object);
                    }
                }
            })
            .wrap_err("could not start the PipeWire watcher")?;
        Ok(Self { child, graph })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// How long a monitor that would not start waits before the next try.
const RETRY: Duration = Duration::from_secs(60);

/// The applications whose PipeWire capture streams are running now. Without
/// PipeWire this fails once, then reports none until the next try.
pub fn active_microphone_applications() -> Result<Vec<ActiveMicrophoneApplication>> {
    static MONITOR: Mutex<Option<std::result::Result<Monitor, Instant>>> = Mutex::new(None);
    let mut monitor = MONITOR.lock().unwrap_or_else(|error| error.into_inner());
    let current = match monitor.as_mut() {
        Some(Ok(running)) => running.alive(),
        Some(Err(failed_at)) => failed_at.elapsed() < RETRY,
        None => false,
    };
    if !current {
        match Monitor::start() {
            Ok(started) => *monitor = Some(Ok(started)),
            Err(error) => {
                *monitor = Some(Err(Instant::now()));
                return Err(error);
            }
        }
    }
    match monitor.as_ref() {
        Some(Ok(running)) => Ok(running
            .graph
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .microphone_users(std::process::id() as i32)),
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: u64, state: &str, props: Value) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Node", "info": {"state": state, "props": props}})
    }

    #[test]
    fn running_capture_streams_are_microphone_users() {
        let mut graph = Graph::default();
        for object in [
            node(
                1,
                "running",
                json!({"media.class": "Stream/Input/Audio", "application.process.binary": "zoom",
                       "application.process.id": 41, "application.name": "ZOOM VoiceEngine"}),
            ),
            // A monitor capture (meeting system audio, level meters).
            node(
                2,
                "running",
                json!({"media.class": "Stream/Input/Audio", "stream.capture.sink": true,
                       "application.process.binary": "pavucontrol", "application.process.id": 42}),
            ),
            // Idle, and playback.
            node(
                3,
                "suspended",
                json!({"media.class": "Stream/Input/Audio", "application.process.binary": "chrome",
                       "application.process.id": 43}),
            ),
            node(
                4,
                "running",
                json!({"media.class": "Stream/Output/Audio", "application.process.binary": "spotify",
                       "application.process.id": 44}),
            ),
            // Hex itself.
            node(
                5,
                "running",
                json!({"media.class": "Stream/Input/Audio", "application.process.binary": "hex",
                       "application.process.id": 7}),
            ),
            // Properties only on the client.
            json!({"id": 9, "type": "PipeWire:Interface:Client",
                   "info": {"props": {"application.process.binary": "firefox", "application.process.id": "45"}}}),
            node(
                6,
                "running",
                json!({"media.class": "Stream/Input/Audio", "client.id": 9}),
            ),
        ] {
            graph.apply(&object);
        }
        let users = graph.microphone_users(7);
        let binaries = users
            .iter()
            .map(|user| user.bundle_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(binaries, ["firefox", "zoom"]);
        assert_eq!(users[1].name, "ZOOM VoiceEngine");
        assert_eq!(users[0].pid, 45);

        graph.apply(&json!({"id": 1, "info": null}));
        let users = graph.microphone_users(7);
        assert_eq!(users.len(), 1);
    }
}
