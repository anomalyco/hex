// A custom harness keeps initialization on the OS main thread and gives each
// concurrency scenario a fresh process-global keyboard-layout cache.
#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/keyboard.rs"]
mod keyboard;

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("keyboard layout regression: skipped (macOS only)");
}

#[cfg(target_os = "macos")]
fn main() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const CASES: [&str; 4] = ["cold-lookups", "cold-mixed", "warm-hits", "warm-misses"];
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--case") {
        // Native aborts should fail the child without creating crash reports or
        // terminating the parent test runner. _exit is async-signal-safe.
        unsafe extern "C" fn abort_child(signal: libc::c_int) {
            unsafe { libc::_exit(128 + signal) };
        }
        unsafe {
            libc::signal(
                libc::SIGABRT,
                abort_child as *const () as libc::sighandler_t,
            );
        }
        run_case(args.get(1).expect("missing child case"));
        return;
    }
    if args.iter().any(|arg| arg == "--list") {
        for case in CASES {
            println!("keyboard_layout::{case}: test");
        }
        return;
    }
    if args.iter().any(|arg| arg == "--ignored") {
        return;
    }
    for option in args.iter().filter(|arg| arg.starts_with('-')) {
        assert!(
            matches!(
                option.as_str(),
                "--nocapture" | "--exact" | "--quiet" | "-q"
            ) || option
                .strip_prefix("--test-threads=")
                .is_some_and(|value| value.parse::<usize>().is_ok_and(|value| value > 0)),
            "unsupported test option {option:?}"
        );
    }
    let mut filters = args.iter().filter(|arg| !arg.starts_with('-'));
    let filter = filters.next();
    assert!(filters.next().is_none(), "expected at most one test filter");
    let mut failures = Vec::new();
    let mut runs = 0;
    for case in CASES {
        let name = format!("keyboard_layout::{case}");
        if filter.is_some_and(|filter| {
            if args.iter().any(|arg| arg == "--exact") {
                &name != filter
            } else {
                !name.contains(filter)
            }
        }) {
            continue;
        }
        for attempt in 1..=3 {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--case", case])
                .env("RUST_BACKTRACE", "0")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            let timed_out = loop {
                if child.try_wait().unwrap().is_some() {
                    break false;
                }
                if Instant::now() >= deadline {
                    child.kill().unwrap();
                    break true;
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let output = child.wait_with_output().unwrap();
            runs += 1;
            if timed_out || !output.status.success() {
                failures.push(format!("{case} attempt {attempt}"));
                eprintln!(
                    "FAIL {case} attempt {attempt}: status={}, timed_out={timed_out}\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            } else {
                println!("PASS {case} attempt {attempt}");
            }
        }
    }
    assert!(
        failures.is_empty(),
        "keyboard layout regression: {}/{} child processes failed: {}",
        failures.len(),
        runs,
        failures.join(", ")
    );
    println!("keyboard layout regression: {runs} child processes passed");
}

#[cfg(target_os = "macos")]
fn run_case(case: &str) {
    use std::sync::{Arc, Barrier};

    const WORKERS: usize = 8;
    const ITERATIONS: usize = 1_000;
    let warm = case.starts_with("warm-");
    if warm {
        keyboard::initialize_layout().unwrap();
    }
    let barrier = Arc::new(Barrier::new(WORKERS));
    let workers = (0..WORKERS)
        .map(|worker| {
            let barrier = barrier.clone();
            let case = case.to_owned();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..ITERATIONS {
                    if case == "cold-mixed" && worker % 2 == 0 {
                        keyboard::initialize_layout().unwrap();
                    } else if case == "warm-misses" {
                        assert!(keyboard::key_code_for('\u{10ffff}').is_err());
                    } else {
                        keyboard::key_code_for(' ').unwrap();
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
    let queries = || keyboard::INPUT_SOURCE_QUERIES.load(std::sync::atomic::Ordering::Relaxed);
    if warm {
        assert_eq!(
            queries(),
            1,
            "warm lookups must not re-enter native layout APIs"
        );
    } else if case == "cold-lookups" {
        assert_eq!(
            queries(),
            WORKERS * ITERATIONS,
            "headless lookups remain live"
        );
    }
    keyboard::initialize_layout().unwrap();
    let snapshot_queries = queries();
    assert!(keyboard::key_code_for(' ').is_ok());
    assert!(keyboard::key_code_for('\u{10ffff}').is_err());
    assert_eq!(
        keyboard::key_code_for('v').ok(),
        keyboard::key_code_for('V').ok()
    );
    assert_eq!(
        queries(),
        snapshot_queries,
        "after snapshot publication even misses must stay out of native layout APIs"
    );
}
