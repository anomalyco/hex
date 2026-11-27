//! The command side of `hex dev call-window`: it asks the running Hex for
//! each read and writes what comes back to the one file it was given.

use std::fs::{File, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::Serialize;

use super::{FilterSpec, Reading, Reply, Request, Target};

/// The command's long help: what it is for, where the read runs, and the two
/// things a person must know before running it.
pub const ABOUT: &str = "\
Reads another app's windows through macOS Accessibility and reports what is \
there: each element's role, title, description, value, DOM id and DOM classes. \
It exists to measure what a call app's tiles expose (names, mute state, a \
speaking mark) and what one read costs. Dev builds only.

The read runs in the running Hex, which holds the Accessibility grant; this \
command asks it over the local API and writes the file itself. Every window \
the app lists is read, in front or not.

It only reads: it never clicks, focuses, raises or types.

PRIVACY: the output holds other people's names and whatever text is on \
screen. It is written only to the path given to --out, readable by you alone \
(0600). Hex's logs get counts and timings, never the tree.

WEB CONTENT: a Chromium or Electron app (Slack, Teams, Chrome, Arc, Brave, \
Edge) builds its web accessibility tree only when an assistive app asks. This \
command never asks unless you pass --enable-web-accessibility, because that \
changes how the target app behaves until it quits: it keeps computing the \
tree, which costs it CPU and memory, and it may act as it does for a screen \
reader. Without the flag such an app shows its windows and little else.";

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Write one snapshot of every window's tree to a file, as JSON.
    #[command(long_about = format!("Write one snapshot of every window's tree to a file, as JSON.\n\n{ABOUT}"))]
    Dump {
        #[command(flatten)]
        target: TargetArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Repeat the read and write one JSON line per read, then a summary.
    #[command(long_about = format!(
        "Repeat the read at an interval and write one JSON line per read: the time, how long \
         the read took, how many elements it asked, whether a budget cut it short, and the \
         elements a filter matches. An element matches when any of --id-prefix, \
         --class-contains or --label matches it. Ends with a summary: reads, median and worst \
         elapsed milliseconds, median element count, and reads cut short. Ctrl-C ends it \
         without the summary; the lines written stay.\n\n{ABOUT}"
    ))]
    Watch {
        #[command(flatten)]
        target: TargetArgs,
        #[command(flatten)]
        read: ReadArgs,
        /// Milliseconds from one read's start to the next.
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(50..))]
        interval_ms: u64,
        /// Seconds to keep reading.
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
        duration: u64,
        /// Report elements whose DOM id starts with this.
        #[arg(long, value_name = "PREFIX")]
        id_prefix: Option<String>,
        /// Report elements with a DOM class containing this.
        #[arg(long, value_name = "TEXT")]
        class_contains: Option<String>,
        /// Report elements whose title, description or value matches this regular expression.
        #[arg(long, value_name = "REGEX")]
        label: Option<String>,
        /// Print "Speak now" this often, in seconds, and mark the moment in the file, to time
        /// how long a speaking mark takes to appear.
        #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
        cue_every: Option<u64>,
    },
}

#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct TargetArgs {
    /// The target app's bundle id, such as com.tinyspeck.slackmacgap.
    #[arg(long, value_name = "BUNDLE_ID")]
    app: Option<String>,
    /// The target app's process id.
    #[arg(long)]
    pid: Option<i32>,
}

impl TargetArgs {
    fn target(&self) -> Target {
        match (&self.app, self.pid) {
            (Some(bundle_id), _) => Target::BundleId(bundle_id.clone()),
            (None, pid) => Target::Pid(pid.unwrap_or_default()),
        }
    }
}

#[derive(clap::Args)]
struct ReadArgs {
    /// The file to write. Created or replaced, readable by you alone.
    #[arg(long, short, value_name = "FILE")]
    out: PathBuf,
    /// Ask a Chromium or Electron app to build its web accessibility tree. This changes how
    /// the target app behaves until it quits.
    #[arg(long)]
    enable_web_accessibility: bool,
    /// Elements one read may ask, at most 20000 [default: 20000 for dump, 4000 for watch].
    #[arg(long)]
    max_nodes: Option<usize>,
    /// Levels below a window one read may go, at most 200.
    #[arg(long, default_value_t = 80)]
    max_depth: usize,
    /// Milliseconds one read may take, at most 5000 [default: 5000 for dump, 1000 for watch].
    #[arg(long)]
    deadline_ms: Option<u64>,
}

/// The time a loop runs on, so tests need not wait.
trait Clock {
    /// Time since the command started.
    fn elapsed(&self) -> Duration;
    fn wait_until(&mut self, at: Duration);
    /// Unix time in milliseconds.
    fn wall_ms(&self) -> u64;
}

struct SystemClock(Instant);

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    fn wait_until(&mut self, at: Duration) {
        std::thread::sleep(at.saturating_sub(self.0.elapsed()));
    }

    fn wall_ms(&self) -> u64 {
        crate::desktop::events::now_ms()
    }
}

/// How long a web tree asked for gets to appear before a dump gives up.
const WEB_TREE_WAIT: Duration = Duration::from_millis(250);
const WEB_TREE_TRIES: usize = 12;

/// Runs the command, or refuses it as an unknown one in a stable build.
pub fn run(args: &Args, top_level: impl FnOnce() -> clap::Command) -> Result<()> {
    if let Some(refusal) = refusal(crate::dev_build::is_dev_build(), top_level) {
        refusal.exit();
    }
    let mut ask = |request: &Request| crate::local_api::accessibility_probe(request);
    let mut clock = SystemClock(Instant::now());
    match &args.command {
        Command::Dump { target, read } => {
            let request = request(target, read, None, true);
            print!("{}", dump(&request, &read.out, &mut ask, &mut clock)?);
        }
        Command::Watch {
            target,
            read,
            interval_ms,
            duration,
            id_prefix,
            class_contains,
            label,
            cue_every,
        } => {
            let filter = FilterSpec {
                id_prefix: id_prefix.clone(),
                class_contains: class_contains.clone(),
                label: label.clone(),
            };
            filter
                .compile()
                .map_err(|error| eyre!("--label: {error}"))?;
            let request = request(target, read, (!filter.is_empty()).then_some(filter), false);
            let plan = Plan {
                interval: Duration::from_millis(*interval_ms),
                duration: Duration::from_secs(*duration),
                cue_every: cue_every.map(Duration::from_secs),
            };
            let mut file = create_private(&read.out)?;
            let summary = watch(
                &request,
                plan,
                &mut file,
                &mut ask,
                &mut clock,
                &mut |cue| {
                    eprintln!("{cue}");
                },
            )?;
            print!("{summary}");
            if summary.without_web_content == summary.reads {
                print!("{}", no_web_content(&request, None));
            }
            println!("{}", wrote(&read.out));
        }
    }
    Ok(())
}

/// The usage error a stable build gives, the one clap gives any unknown
/// command.
fn refusal(dev_build: bool, top_level: impl FnOnce() -> clap::Command) -> Option<clap::Error> {
    (!super::offered(dev_build)).then(|| {
        top_level().error(
            clap::error::ErrorKind::InvalidSubcommand,
            "unrecognized subcommand 'call-window'",
        )
    })
}

fn request(
    target: &TargetArgs,
    read: &ReadArgs,
    filter: Option<FilterSpec>,
    whole_tree: bool,
) -> Request {
    Request {
        target: target.target(),
        enable_web_accessibility: read.enable_web_accessibility,
        max_nodes: read
            .max_nodes
            .unwrap_or(if whole_tree { 20_000 } else { 4_000 }),
        max_depth: read.max_depth,
        deadline_ms: read
            .deadline_ms
            .unwrap_or(if whole_tree { 5_000 } else { 1_000 }),
        filter,
        tree: whole_tree,
    }
}

/// Creates or empties `path` for its owner alone. A link is refused rather
/// than followed, and a file that was there keeps none of its wider mode.
fn create_private(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .wrap_err_with(|| format!("could not write {}", path.display()))?;
    file.set_permissions(Permissions::from_mode(0o600))
        .wrap_err_with(|| format!("could not make {} private", path.display()))?;
    Ok(file)
}

fn wrote(path: &Path) -> String {
    format!(
        "Wrote {} (readable by you alone; it holds other people's names and on-screen text)",
        path.display()
    )
}

fn reading(reply: Reply) -> Result<Reading> {
    match reply {
        Reply::Read(reading) => Ok(reading),
        Reply::Error { code, message } => bail!("{code}: {message}"),
    }
}

/// What to say when a read found no web content.
fn no_web_content(request: &Request, took_request: Option<bool>) -> String {
    if !request.enable_web_accessibility {
        return "No web content in the tree. A Chromium or Electron app (Slack, Teams, Chrome, \
                Arc, Brave, Edge) builds its web tree only when asked: pass \
                --enable-web-accessibility, which changes how that app behaves until it quits. \
                A native app has none to show.\n"
            .into();
    }
    let took = match took_request {
        Some(true) => "The app took the request. ",
        Some(false) => "The app refused the request. ",
        None => "",
    };
    format!(
        "Still no web content after asking for it. {took}A hidden or minimized window may build \
         none, and a native app has none to show.\n"
    )
}

#[derive(Serialize)]
struct Dump<'a> {
    time_ms: u64,
    #[serde(flatten)]
    reading: &'a Reading,
}

/// One snapshot, written whole to `out`. Returns what to print.
fn dump(
    request: &Request,
    out: &Path,
    ask: &mut dyn FnMut(&Request) -> Result<Reply>,
    clock: &mut dyn Clock,
) -> Result<String> {
    // The file first: a path that can't be written fails before the target
    // app is asked anything.
    let mut file = create_private(out)?;
    let mut time_ms = clock.wall_ms();
    let mut found = reading(ask(request)?)?;
    let took_request = found.web_accessibility_requested;
    // Chromium builds the tree a moment after it is asked. Only the first
    // read asks; the rest wait for it.
    let waiting = Request {
        enable_web_accessibility: false,
        ..request.clone()
    };
    for _ in 0..WEB_TREE_TRIES {
        if !request.enable_web_accessibility || found.web_nodes > 0 {
            break;
        }
        clock.wait_until(clock.elapsed() + WEB_TREE_WAIT);
        time_ms = clock.wall_ms();
        found = reading(ask(&waiting)?)?;
    }
    serde_json::to_writer_pretty(
        &mut file,
        &Dump {
            time_ms,
            reading: &found,
        },
    )?;
    file.write_all(b"\n")?;
    let mut report = format!(
        "Read pid {} in {:.1} ms: {} windows, {} elements ({} unreadable), {} in web content\n",
        found.pid, found.elapsed_ms, found.windows, found.nodes, found.unreadable, found.web_nodes
    );
    if !found.cut_short.is_empty() {
        let by = serde_json::to_string(&found.cut_short)?;
        report.push_str(&format!(
            "Cut short by {by}: the file holds part of the tree\n"
        ));
    }
    if found.web_nodes == 0 {
        report.push_str(&no_web_content(request, took_request));
    }
    report.push_str(&wrote(out));
    report.push('\n');
    Ok(report)
}

struct Plan {
    interval: Duration,
    duration: Duration,
    cue_every: Option<Duration>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Line<'a> {
    Read {
        time_ms: u64,
        #[serde(flatten)]
        reading: &'a Reading,
    },
    /// "Speak now" was printed at this moment.
    Cue { time_ms: u64 },
    /// A read that failed after the first one worked.
    Error {
        time_ms: u64,
        code: &'a str,
        message: &'a str,
    },
}

#[derive(Debug, Default, PartialEq)]
struct Summary {
    reads: usize,
    failed: usize,
    median_ms: f64,
    worst_ms: f64,
    median_nodes: usize,
    cut_short: usize,
    /// Reads with at least one match, when a filter was given.
    with_matches: Option<usize>,
    without_web_content: usize,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Reads: {} ({} failed)", self.reads, self.failed)?;
        writeln!(
            f,
            "Elapsed: median {:.1} ms, worst {:.1} ms",
            self.median_ms, self.worst_ms
        )?;
        writeln!(f, "Elements: median {}", self.median_nodes)?;
        writeln!(f, "Cut short: {}", self.cut_short)?;
        if let Some(with_matches) = self.with_matches {
            writeln!(f, "Reads with a match: {with_matches}")?;
        }
        Ok(())
    }
}

/// The middle of `values`, the upper one of an even count's two.
fn median<T: Copy + PartialOrd + Default>(mut values: Vec<T>) -> T {
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    values.get(values.len() / 2).copied().unwrap_or_default()
}

/// Reads on `plan`'s interval for its duration, one JSON line per read. The
/// first read's failure ends the watch; a later one is a line and a count.
fn watch(
    request: &Request,
    plan: Plan,
    out: &mut dyn Write,
    ask: &mut dyn FnMut(&Request) -> Result<Reply>,
    clock: &mut dyn Clock,
    cue: &mut dyn FnMut(&str),
) -> Result<Summary> {
    let mut summary = Summary {
        with_matches: request.filter.as_ref().map(|_| 0),
        ..Default::default()
    };
    let (mut elapsed, mut nodes) = (Vec::new(), Vec::new());
    let mut line = |line: &Line| -> Result<()> {
        serde_json::to_writer(&mut *out, line)?;
        out.write_all(b"\n")?;
        Ok(out.flush()?)
    };
    // Only the first read asks for the web tree; asking again changes nothing.
    let mut request = request.clone();
    let mut next_cue = plan.cue_every;
    let mut at = Duration::ZERO;
    while at < plan.duration {
        clock.wait_until(at);
        if let (Some(due), Some(every)) = (next_cue, plan.cue_every)
            && clock.elapsed() >= due
        {
            line(&Line::Cue {
                time_ms: clock.wall_ms(),
            })?;
            cue("Speak now");
            next_cue = Some(due + every);
        }
        let time_ms = clock.wall_ms();
        let first = summary.reads + summary.failed == 0;
        let failure = match ask(&request) {
            Ok(Reply::Read(reading)) => {
                line(&Line::Read {
                    time_ms,
                    reading: &reading,
                })?;
                summary.reads += 1;
                summary.cut_short += usize::from(!reading.cut_short.is_empty());
                summary.without_web_content += usize::from(reading.web_nodes == 0);
                if let (Some(with_matches), Some(matches)) =
                    (&mut summary.with_matches, &reading.matches)
                {
                    *with_matches += usize::from(!matches.is_empty());
                }
                elapsed.push(reading.elapsed_ms);
                nodes.push(reading.nodes);
                None
            }
            Ok(Reply::Error { code, message }) => Some((code, message)),
            Err(error) => Some(("transport".into(), format!("{error:#}"))),
        };
        if let Some((code, message)) = failure {
            if first {
                bail!("{code}: {message}");
            }
            line(&Line::Error {
                time_ms,
                code: &code,
                message: &message,
            })?;
            summary.failed += 1;
        }
        request.enable_web_accessibility = false;
        // The next tick still ahead: a slow read skips ticks, it never
        // queues a burst of reads behind it.
        let interval = plan.interval.as_nanos().max(1);
        let ticks = clock.elapsed().as_nanos() / interval + 1;
        at = (plan.interval * u32::try_from(ticks).unwrap_or(u32::MAX)).max(at + plan.interval);
    }
    summary.worst_ms = elapsed.iter().copied().fold(0.0, f64::max);
    summary.median_ms = median(elapsed);
    summary.median_nodes = median(nodes);
    Ok(summary)
}

#[cfg(test)]
mod tests;
