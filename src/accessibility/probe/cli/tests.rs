use std::cell::Cell;
use std::rc::Rc;

use clap::Parser;

use super::*;
use crate::accessibility::tree::{Cut, Match, Node};

#[derive(Parser)]
struct Harness {
    #[command(flatten)]
    arguments: Args,
}

fn parse(arguments: &[&str]) -> Result<Command, clap::Error> {
    Harness::try_parse_from(["call-window"].iter().chain(arguments))
        .map(|harness| harness.arguments.command)
}

/// Time that moves only when something waits or a read says it took time.
struct FakeClock(Rc<Cell<Duration>>);

impl Clock for FakeClock {
    fn elapsed(&self) -> Duration {
        self.0.get()
    }

    fn wait_until(&mut self, at: Duration) {
        self.0.set(self.0.get().max(at));
    }

    fn wall_ms(&self) -> u64 {
        1_800_000_000_000 + self.0.get().as_millis() as u64
    }
}

fn found(nodes: usize, elapsed_ms: f64, web_nodes: usize) -> Reading {
    Reading {
        pid: 4242,
        frontmost: Some(false),
        elapsed_ms,
        windows: 2,
        nodes,
        unreadable: 0,
        cut_short: Vec::new(),
        web_areas: usize::from(web_nodes > 0),
        web_nodes,
        web_accessibility_requested: None,
        matches: None,
        tree: None,
    }
}

fn pid_request() -> Request {
    Request {
        target: Target::Pid(4242),
        enable_web_accessibility: false,
        max_nodes: 4_000,
        max_depth: 80,
        deadline_ms: 1_000,
        filter: None,
        tree: false,
    }
}

fn temp_file(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("hex-call-window-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join("out.json")
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_target_and_a_file_are_required_and_the_flag_is_never_implied() {
    assert!(parse(&["dump", "--out", "tree.json"]).is_err());
    assert!(parse(&["dump", "--pid", "7"]).is_err());
    assert!(
        parse(&[
            "dump",
            "--pid",
            "7",
            "--app",
            "com.example.calls",
            "--out",
            "t"
        ])
        .is_err()
    );
    let Ok(Command::Dump { target, read }) =
        parse(&["dump", "--app", "com.example.calls", "--out", "tree.json"])
    else {
        panic!("a dump parses");
    };
    let asked = request(&target, &read, None, true);
    assert_eq!(asked.target, Target::BundleId("com.example.calls".into()));
    assert!(!asked.enable_web_accessibility);
    assert!(asked.tree);
    assert_eq!((asked.max_nodes, asked.deadline_ms), (20_000, 5_000));

    let Ok(Command::Watch {
        target,
        read,
        interval_ms,
        duration,
        id_prefix,
        class_contains,
        label,
        cue_every,
    }) = parse(&[
        "watch",
        "--pid",
        "7",
        "--out",
        "watch.jsonl",
        "--enable-web-accessibility",
        "--id-prefix",
        "huddle-grid-gridcell-",
        "--class-contains",
        "active_speaker",
        "--label",
        "profile|audio",
        "--cue-every",
        "10",
    ])
    else {
        panic!("a watch parses");
    };
    assert_eq!((interval_ms, duration, cue_every), (250, 60, Some(10)));
    assert_eq!(id_prefix.as_deref(), Some("huddle-grid-gridcell-"));
    assert_eq!(class_contains.as_deref(), Some("active_speaker"));
    assert_eq!(label.as_deref(), Some("profile|audio"));
    let asked = request(&target, &read, None, false);
    assert_eq!(asked.target, Target::Pid(7));
    assert!(asked.enable_web_accessibility);
    assert!(!asked.tree);
    assert_eq!((asked.max_nodes, asked.deadline_ms), (4_000, 1_000));
    assert!(parse(&["watch", "--pid", "7", "--out", "w", "--interval-ms", "10"]).is_err());
}

#[test]
fn a_stable_build_refuses_the_command_as_an_unknown_one() {
    let top_level = || clap::Command::new("hex");
    assert!(refusal(true, top_level).is_none());
    let refused = refusal(false, top_level).expect("a stable build refuses");
    assert_eq!(refused.kind(), clap::error::ErrorKind::InvalidSubcommand);
    assert_eq!(refused.exit_code(), 2);
}

#[test]
fn a_dump_writes_the_tree_to_a_file_only_its_owner_reads() {
    let out = temp_file("dump");
    // A file that was there, readable by everyone, is replaced and narrowed.
    std::fs::write(&out, "old").unwrap();
    std::fs::set_permissions(&out, Permissions::from_mode(0o644)).unwrap();
    let tree = vec![Node {
        role: Some("AXWindow".into()),
        title: Some("Huddle".into()),
        ..Default::default()
    }];
    let mut asked = Vec::new();
    let mut ask = |request: &Request| {
        asked.push(request.clone());
        Ok(Reply::Read(Reading {
            tree: Some(tree.clone()),
            cut_short: vec![Cut::Nodes],
            ..found(1, 12.25, 0)
        }))
    };
    let request = Request {
        tree: true,
        ..pid_request()
    };
    let report = dump(&request, &out, &mut ask, &mut FakeClock(Rc::default())).unwrap();
    assert_eq!(mode(&out), 0o600);
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written["time_ms"], 1_800_000_000_000_u64);
    assert_eq!(written["pid"], 4242);
    assert_eq!(written["nodes"], 1);
    assert_eq!(written["cut_short"], serde_json::json!(["nodes"]));
    assert_eq!(written["tree"][0]["title"], "Huddle");
    // Without the flag it asks once, says the tree has no web content, and
    // names the flag.
    assert_eq!(asked.len(), 1);
    assert!(
        report.contains("Read pid 4242 in 12.2 ms: 2 windows, 1 elements"),
        "{report}"
    );
    assert!(report.contains(r#"Cut short by ["nodes"]"#), "{report}");
    assert!(report.contains("--enable-web-accessibility"), "{report}");
    assert!(report.contains("until it quits"), "{report}");
    assert!(report.contains("readable by you alone"), "{report}");
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn a_dump_with_the_flag_asks_once_then_waits_for_the_web_tree() {
    let out = temp_file("dump-web");
    let mut asked = Vec::new();
    let mut ask = |request: &Request| {
        asked.push(request.enable_web_accessibility);
        Ok(Reply::Read(Reading {
            web_accessibility_requested: request.enable_web_accessibility.then_some(true),
            ..found(40, 3.0, if asked.len() < 3 { 0 } else { 25 })
        }))
    };
    let request = Request {
        enable_web_accessibility: true,
        tree: true,
        ..pid_request()
    };
    let time = Rc::new(Cell::new(Duration::ZERO));
    let report = dump(&request, &out, &mut ask, &mut FakeClock(time.clone())).unwrap();
    assert_eq!(asked, [true, false, false]);
    assert_eq!(time.get(), WEB_TREE_WAIT * 2);
    assert!(report.contains("25 in web content"), "{report}");
    assert!(!report.contains("--enable-web-accessibility"), "{report}");
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn a_link_is_refused_rather_than_followed() {
    let out = temp_file("link");
    let elsewhere = out.with_file_name("elsewhere.json");
    std::fs::write(&elsewhere, "kept").unwrap();
    std::os::unix::fs::symlink(&elsewhere, &out).unwrap();
    assert!(create_private(&out).is_err());
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "kept");
    std::fs::remove_dir_all(out.parent().unwrap()).unwrap();
}

#[test]
fn a_watch_writes_a_line_per_read_and_sums_them_up() {
    let time = Rc::new(Cell::new(Duration::ZERO));
    let reads = [
        (800, 10.0, false),
        (820, 30.0, true),
        (810, 20.0, true),
        (0, 0.0, false),
        (4_000, 900.0, true),
    ];
    let mut index = 0;
    let read_time = time.clone();
    let mut ask = |request: &Request| {
        let (nodes, elapsed_ms, speaking) = reads[index];
        // Only the first read asks the target to change.
        assert_eq!(request.enable_web_accessibility, index == 0);
        index += 1;
        read_time.set(read_time.get() + Duration::from_secs_f64(elapsed_ms / 1_000.0));
        if nodes == 0 {
            return Ok(Reply::Error {
                code: "target-not-found".into(),
                message: "no process has the pid 4242".into(),
            });
        }
        Ok(Reply::Read(Reading {
            cut_short: if nodes == 4_000 {
                vec![Cut::Nodes]
            } else {
                Vec::new()
            },
            matches: Some(if speaking {
                vec![Match {
                    path: "0/0/0/0/1/2".into(),
                    node: Node {
                        dom_classes: vec!["p-huddle_peer_tile__overlay--active_speaker".into()],
                        ..Default::default()
                    },
                }]
            } else {
                Vec::new()
            }),
            ..found(nodes, elapsed_ms, 700)
        }))
    };
    let request = Request {
        enable_web_accessibility: true,
        filter: Some(FilterSpec {
            class_contains: Some("active_speaker".into()),
            ..Default::default()
        }),
        ..pid_request()
    };
    let mut out = Vec::new();
    let mut cues = Vec::new();
    let summary = watch(
        &request,
        Plan {
            interval: Duration::from_millis(250),
            duration: Duration::from_millis(1_250),
            cue_every: Some(Duration::from_millis(500)),
        },
        &mut out,
        &mut ask,
        &mut FakeClock(time),
        &mut |cue| cues.push(cue.to_owned()),
    )
    .unwrap();
    assert_eq!(
        summary,
        Summary {
            reads: 4,
            failed: 1,
            median_ms: 30.0,
            worst_ms: 900.0,
            median_nodes: 820,
            cut_short: 1,
            with_matches: Some(3),
            without_web_content: 0,
        }
    );
    assert_eq!(
        summary.to_string(),
        "Reads: 4 (1 failed)\nElapsed: median 30.0 ms, worst 900.0 ms\nElements: median 820\n\
         Cut short: 1\nReads with a match: 3\n"
    );
    assert_eq!(cues, ["Speak now", "Speak now"]);
    let lines: Vec<serde_json::Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let kinds: Vec<&str> = lines
        .iter()
        .map(|line| line["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["read", "read", "cue", "read", "error", "cue", "read"]
    );
    // Reads start on the interval: 0, 250, 500, 750, 1000 ms.
    assert_eq!(lines[0]["time_ms"], 1_800_000_000_000_u64);
    assert_eq!(lines[1]["time_ms"], 1_800_000_000_250_u64);
    assert_eq!(lines[3]["time_ms"], 1_800_000_000_500_u64);
    assert_eq!(lines[1]["elapsed_ms"], 30.0);
    assert_eq!(lines[1]["nodes"], 820);
    assert_eq!(lines[1]["cut_short"], serde_json::json!([]));
    assert_eq!(lines[1]["frontmost"], false);
    assert_eq!(lines[1]["matches"][0]["path"], "0/0/0/0/1/2");
    assert_eq!(lines[4]["code"], "target-not-found");
    assert_eq!(lines[6]["cut_short"], serde_json::json!(["nodes"]));
}

#[test]
fn a_slow_read_skips_ticks_instead_of_queueing_reads() {
    let time = Rc::new(Cell::new(Duration::ZERO));
    let read_time = time.clone();
    let mut ask = |_: &Request| {
        read_time.set(read_time.get() + Duration::from_millis(600));
        Ok(Reply::Read(found(10, 600.0, 0)))
    };
    let mut out = Vec::new();
    let summary = watch(
        &pid_request(),
        Plan {
            interval: Duration::from_millis(250),
            duration: Duration::from_millis(2_000),
            cue_every: None,
        },
        &mut out,
        &mut ask,
        &mut FakeClock(time),
        &mut |_| {},
    )
    .unwrap();
    // Reads at 0, 750 and 1500 ms: each waits for the next tick ahead.
    assert_eq!(summary.reads, 3);
    assert_eq!(summary.with_matches, None);
    assert_eq!(summary.without_web_content, 3);
    assert!(!summary.to_string().contains("match"));
}

#[test]
fn a_watch_whose_first_read_is_refused_ends_with_the_reason() {
    let mut ask = |_: &Request| {
        Ok(Reply::Error {
            code: "not-trusted".into(),
            message: "the running Hex isn't allowed to use Accessibility".into(),
        })
    };
    let mut out = Vec::new();
    let error = watch(
        &pid_request(),
        Plan {
            interval: Duration::from_millis(250),
            duration: Duration::from_secs(1),
            cue_every: None,
        },
        &mut out,
        &mut ask,
        &mut FakeClock(Rc::default()),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().starts_with("not-trusted: "));
    assert!(out.is_empty());
}

#[test]
fn the_note_about_web_content_names_the_flag_only_when_it_was_not_passed() {
    let quiet = no_web_content(&pid_request(), None);
    assert!(quiet.contains("--enable-web-accessibility"));
    assert!(quiet.contains("until it quits"));
    let asked = Request {
        enable_web_accessibility: true,
        ..pid_request()
    };
    assert!(!no_web_content(&asked, Some(true)).contains("--enable-web-accessibility"));
    assert!(no_web_content(&asked, Some(false)).contains("refused"));
}

/// A stable build's help leaves the command out, and every build's long
/// help carries the warnings a person needs before running it.
#[test]
fn the_command_is_listed_in_dev_builds_only_and_its_help_warns() {
    use clap::CommandFactory;

    let call_window = |command: &clap::Command| {
        command
            .find_subcommand("dev")
            .and_then(|dev| dev.find_subcommand("call-window"))
            .cloned()
            .expect("dev call-window is defined")
    };
    let listed = call_window(&crate::Cli::command());
    assert!(!listed.is_hide_set());
    for subcommand in ["dump", "watch"] {
        let help = listed
            .find_subcommand(subcommand)
            .unwrap()
            .clone()
            .render_long_help()
            .to_string();
        assert!(help.contains("other people's names"), "{help}");
        assert!(help.contains("until it quits"), "{help}");
        assert!(
            help.contains("never clicks, focuses, raises or types"),
            "{help}"
        );
        assert!(help.contains("running Hex"), "{help}");
    }
    {
        let _stable = crate::dev_build::pretend_stable();
        assert!(call_window(&crate::Cli::command()).is_hide_set());
    }
    assert!(matches!(
        crate::Cli::try_parse_from([
            "hex",
            "dev",
            "call-window",
            "dump",
            "--pid",
            "7",
            "--out",
            "tree.json"
        ])
        .unwrap()
        .command,
        Some(crate::Command::Dev {
            command: crate::DevCommand::CallWindow(_)
        })
    ));
}
