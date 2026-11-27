//! `hex dev call-window`: a dev-build probe that reads another app's windows
//! through Accessibility and reports what is there, so that what a call
//! app's tiles expose (names, mute state, a speaking mark) and what a read
//! costs are measured before Hex relies on either.
//!
//! The read runs in the running Hex, not in the `hex` command: macOS grants
//! Accessibility to the app, and a command started from a terminal is judged
//! by that terminal's grant instead. The command asks over the local API
//! (`POST /dev/accessibility-probe`) and writes what comes back itself.
//!
//! It only reads: it never clicks, focuses, raises or types. The one change
//! it can make to the target is the web accessibility request, made only
//! when the request asks for it. What it reads holds other people's names
//! and on-screen text, so Hex logs counts and timings and nothing else.
//!
//! macOS only: Windows and Linux Hex have no Accessibility client (UI
//! Automation, AT-SPI) to read another app with yet.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::tree::{self, Budgets, Cut, Filter, Match, Node};
use super::{foreign_application, live};

pub(crate) mod cli;

/// The most one read may be asked for, whatever the request says: a read
/// holds one of the local API's four workers until it ends.
const NODE_CEILING: usize = 20_000;
pub(crate) const DEPTH_CEILING: usize = 200;
const DEADLINE_CEILING: Duration = Duration::from_secs(5);

/// Whether this build offers the probe: dev builds only. A stable build
/// neither lists the command, accepts it, nor answers its route.
pub(crate) fn offered(dev_build: bool) -> bool {
    dev_build
}

/// The app whose windows are read.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Target {
    Pid(i32),
    BundleId(String),
}

/// A filter as it travels: [`Filter`] with its pattern still text.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FilterSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) id_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) class_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) label: Option<String>,
}

impl FilterSpec {
    pub(crate) fn is_empty(&self) -> bool {
        self.id_prefix.is_none() && self.class_contains.is_none() && self.label.is_none()
    }

    pub(crate) fn compile(&self) -> Result<Filter, regex::Error> {
        Ok(Filter {
            id_prefix: self.id_prefix.clone(),
            class_contains: self.class_contains.clone(),
            label: self.label.as_deref().map(regex::Regex::new).transpose()?,
        })
    }
}

/// One read, as the command asks for it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Request {
    pub(crate) target: Target,
    /// Ask a Chromium or Electron app for its web tree first. This changes
    /// the target until it quits, so it is never implied.
    #[serde(default)]
    pub(crate) enable_web_accessibility: bool,
    pub(crate) max_nodes: usize,
    pub(crate) max_depth: usize,
    pub(crate) deadline_ms: u64,
    /// Report the elements this matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) filter: Option<FilterSpec>,
    /// Send the whole tree back.
    #[serde(default)]
    pub(crate) tree: bool,
}

impl Request {
    fn budgets(&self) -> Budgets {
        Budgets {
            max_nodes: self.max_nodes.min(NODE_CEILING),
            max_depth: self.max_depth.min(DEPTH_CEILING),
            deadline: Duration::from_millis(self.deadline_ms).min(DEADLINE_CEILING),
        }
    }
}

/// What one read found.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Reading {
    pub(crate) pid: i32,
    /// Whether the target was the app in front, when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) frontmost: Option<bool>,
    /// The read alone, in Hex: windows listed, tree walked.
    pub(crate) elapsed_ms: f64,
    pub(crate) windows: usize,
    /// Elements asked, answered or not.
    pub(crate) nodes: usize,
    /// Elements the app did not answer for.
    pub(crate) unreadable: usize,
    /// The budgets that cut the read short; empty for a whole tree.
    pub(crate) cut_short: Vec<Cut>,
    /// `AXWebArea` elements, and elements below one.
    pub(crate) web_areas: usize,
    pub(crate) web_nodes: usize,
    /// Whether the target took the web accessibility request, when one was
    /// made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) web_accessibility_requested: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) matches: Option<Vec<Match>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tree: Option<Vec<Node>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(crate) enum Reply {
    Read(Reading),
    Error { code: String, message: String },
}

impl Reply {
    fn error(code: &str, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Parses a probe reply whose tree may nest deeper than `serde_json`'s
    /// default 128-container recursion limit (`DEPTH_CEILING` = 200 nests two
    /// JSON containers per tree level).
    pub(crate) fn from_json_slice(body: &[u8]) -> serde_json::Result<Self> {
        let mut deserializer = serde_json::Deserializer::from_slice(body);
        deserializer.disable_recursion_limit();
        let reply = Self::deserialize(&mut deserializer)?;
        deserializer.end()?;
        Ok(reply)
    }
}

/// Carries out one request in this process, which must be the running Hex.
pub(crate) fn answer(request: &Request) -> Reply {
    // One read at a time, so probes never hold more than one API worker.
    static READING: Mutex<()> = Mutex::new(());
    let Ok(_reading) = READING.try_lock() else {
        return Reply::error("busy", "another call-window read is still running");
    };
    let filter = match request.filter.as_ref().map(FilterSpec::compile).transpose() {
        Ok(filter) => filter,
        Err(error) => return Reply::error("invalid-filter", format!("--label: {error}")),
    };
    let pid = match &request.target {
        Target::Pid(pid) => *pid,
        Target::BundleId(bundle_id) => {
            match crate::microphone_activity::running_application_by_bundle_id(bundle_id) {
                Some((pid, _)) => pid,
                None => {
                    return Reply::error(
                        "target-not-found",
                        format!("no running app has the bundle id {bundle_id}"),
                    );
                }
            }
        }
    };
    if pid == std::process::id() as i32 {
        return Reply::error("own-process", "Hex never reads its own window's tree");
    }
    // SAFETY: signal 0 sends nothing; it only asks whether the process is there.
    if pid <= 0 || (unsafe { libc::kill(pid, 0) } != 0 && last_error_is_no_process()) {
        return Reply::error("target-not-found", format!("no process has the pid {pid}"));
    }
    if !live::trusted() {
        return Reply::error(
            "not-trusted",
            "the running Hex isn't allowed to use Accessibility \
             (System Settings › Privacy & Security › Accessibility)",
        );
    }
    let (Some(application), Some(attributes)) = (foreign_application(pid), live::Attributes::new())
    else {
        return Reply::error("failed", format!("pid {pid} gave no Accessibility element"));
    };
    let web_accessibility_requested = request
        .enable_web_accessibility
        .then(|| live::request_web_accessibility(&application));
    let started = Instant::now();
    let windows = live::windows(&application, &attributes);
    let snapshot = tree::snapshot(windows, request.budgets());
    let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let web = tree::web_content(&snapshot.windows);
    let matches = filter.map(|filter| tree::matches(&snapshot.windows, &filter));
    // Counts and timings only: the tree holds other people's names.
    tracing::info!(
        pid,
        elapsed_ms,
        nodes = snapshot.nodes,
        unreadable = snapshot.unreadable,
        windows = snapshot.windows.len(),
        web_nodes = web.nodes,
        cut_short = !snapshot.cut_short.is_empty(),
        matches = matches.as_ref().map(Vec::len),
        web_accessibility_requested,
        "call-window probe read"
    );
    Reply::Read(Reading {
        pid,
        frontmost: live::frontmost(&application),
        elapsed_ms,
        windows: snapshot.windows.len(),
        nodes: snapshot.nodes,
        unreadable: snapshot.unreadable,
        cut_short: snapshot.cut_short,
        web_areas: web.areas,
        web_nodes: web.nodes,
        web_accessibility_requested,
        matches,
        tree: request.tree.then_some(snapshot.windows),
    })
}

fn last_error_is_no_process() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(target: Target) -> Request {
        Request {
            target,
            enable_web_accessibility: false,
            max_nodes: 100,
            max_depth: 10,
            deadline_ms: 100,
            filter: None,
            tree: false,
        }
    }

    fn code(reply: Reply) -> String {
        match reply {
            Reply::Error { code, .. } => code,
            Reply::Read(reading) => panic!("expected a refusal, read {} nodes", reading.nodes),
        }
    }

    #[test]
    fn only_dev_builds_offer_the_probe() {
        assert!(offered(true));
        assert!(!offered(false));
    }

    /// Each refusal comes before anything is asked of another app, so these
    /// run without the Accessibility grant and touch no real app.
    #[test]
    fn a_request_it_cannot_carry_out_is_refused_with_its_code() {
        assert_eq!(
            code(answer(&request(Target::Pid(i32::MAX)))),
            "target-not-found"
        );
        assert_eq!(code(answer(&request(Target::Pid(0)))), "target-not-found");
        assert_eq!(
            code(answer(&request(Target::BundleId(
                "invalid.hex.call-window-probe.no-such-app".into()
            )))),
            "target-not-found"
        );
        assert_eq!(
            code(answer(&request(Target::Pid(std::process::id() as i32)))),
            "own-process"
        );
        assert_eq!(
            code(answer(&Request {
                filter: Some(FilterSpec {
                    label: Some("(".into()),
                    ..Default::default()
                }),
                ..request(Target::Pid(i32::MAX))
            })),
            "invalid-filter"
        );
    }

    #[test]
    fn a_request_never_reads_past_the_ceilings() {
        let budgets = Request {
            max_nodes: usize::MAX,
            max_depth: usize::MAX,
            deadline_ms: u64::MAX,
            ..request(Target::Pid(1))
        }
        .budgets();
        assert_eq!(
            budgets,
            Budgets {
                max_nodes: NODE_CEILING,
                max_depth: DEPTH_CEILING,
                deadline: DEADLINE_CEILING,
            }
        );
    }

    #[test]
    fn the_wire_form_is_plain_json() {
        let asked = Request {
            enable_web_accessibility: true,
            filter: Some(FilterSpec {
                id_prefix: Some("huddle-grid-gridcell-".into()),
                ..Default::default()
            }),
            ..request(Target::BundleId("com.example.calls".into()))
        };
        let json = serde_json::to_string(&asked).unwrap();
        assert_eq!(
            json,
            r#"{"target":{"bundle_id":"com.example.calls"},"enable_web_accessibility":true,"max_nodes":100,"max_depth":10,"deadline_ms":100,"filter":{"id_prefix":"huddle-grid-gridcell-"},"tree":false}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), asked);
        assert_eq!(
            serde_json::to_string(&Reply::error("not-trusted", "no")).unwrap(),
            r#"{"status":"error","code":"not-trusted","message":"no"}"#
        );
        // A request that says nothing about web accessibility never asks for it.
        let quiet: Request = serde_json::from_str(
            r#"{"target":{"pid":7},"max_nodes":1,"max_depth":1,"deadline_ms":1}"#,
        )
        .unwrap();
        assert!(!quiet.enable_web_accessibility);
    }
}
