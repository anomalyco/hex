//! `fetch`, `node:child_process`, the trust gate, and fallback, through real
//! scripts: a local HTTP server and real programs, no network.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

/// A local HTTP server answering `count` requests, each after `delay`, with
/// `status` and a JSON `body`. Joining it returns each request's line and body.
fn serve(
    count: usize,
    delay: Duration,
    status: &'static str,
    body: &'static str,
) -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let mut seen = Vec::new();
        for stream in listener.incoming().take(count) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut content = vec![0; length];
            reader.read_exact(&mut content).unwrap();
            seen.push(format!(
                "{} {}",
                line.trim_end(),
                String::from_utf8(content).unwrap()
            ));
            thread::sleep(delay);
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nX-Test: yes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
        seen
    });
    (port, server)
}

#[test]
fn fetch_waits_for_approval_then_reaches_a_local_server() {
    let (port, server) = serve(2, Duration::ZERO, "201 Created", r#"{"response":"HELLO"}"#);
    let config = format!(
        r#"export default {{
  transformations: {{
    ollama: {{
      name: "Ollama",
      transform: async (text) => {{
        const response = await fetch("http://127.0.0.1:{port}/api/generate", {{
          method: "POST",
          headers: {{ "Content-Type": "application/json" }},
          body: JSON.stringify({{ prompt: text }}),
        }})
        const data = await response.json()
        return [response.ok, response.status, response.statusText, response.headers.get("x-test"),
          response.headers.has("X-Test"), response.bodyUsed, data.response].join(" ")
      }},
    }},
  }},
}}
"#
    );
    let mut harness = Harness::new(&config);
    let permissions = harness.worker.status.permissions.clone().unwrap();
    assert!(permissions.fetch && !permissions.exec);
    assert_eq!(permissions.hosts, ["127.0.0.1"]);
    let refused = harness.transform(&["ollama"], "hi").unwrap_err();
    assert!(refused.contains("has not allowed this config"), "{refused}");

    harness.allow();
    assert_eq!(
        harness.transform(&["ollama"], "hi").unwrap(),
        "true 201 Created yes true true HELLO"
    );

    // Any edit asks again.
    harness.write(&format!("{config}// edited\n"));
    assert!(
        harness
            .transform(&["ollama"], "hi")
            .unwrap_err()
            .contains("has not allowed")
    );
    harness.allow();
    assert!(harness.transform(&["ollama"], "again").is_ok());
    assert_eq!(
        server.join().unwrap(),
        [
            r#"POST /api/generate HTTP/1.1 {"prompt":"hi"}"#,
            r#"POST /api/generate HTTP/1.1 {"prompt":"again"}"#,
        ]
    );
}

#[test]
fn fetch_rejects_on_abort_and_on_network_errors() {
    let (port, server) = serve(1, Duration::from_millis(1500), "200 OK", "{}");
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let harness = Harness::new(&format!(
        r#"const attempt = async (url, init) => {{
  try {{ await fetch(url, init); return "fetched" }} catch (error) {{ return `${{error.name}}: ${{error.message.split(":")[0]}}` }}
}}
export default {{
  transformations: {{
    slow: {{ name: "Slow", transform: () => attempt("http://127.0.0.1:{port}/", {{ signal: AbortSignal.timeout(100) }}) }},
    closed: {{ name: "Closed", transform: () => attempt("http://127.0.0.1:{closed}/") }},
    body: {{ name: "Body", transform: () => attempt("http://127.0.0.1:{closed}/", {{ body: "x" }}) }},
  }},
}}
"#
    ));
    harness.allow();
    let started = Instant::now();
    assert_eq!(
        harness.transform(&["slow"], "").unwrap(),
        "TimeoutError: The operation timed out."
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        harness.transform(&["closed"], "").unwrap(),
        "TypeError: fetch failed"
    );
    assert_eq!(
        harness.transform(&["body"], "").unwrap(),
        "TypeError: Request with GET/HEAD method cannot have body."
    );
    server.join().unwrap();
}

#[cfg(unix)]
#[test]
fn exec_and_exec_file_follow_node() {
    let harness = Harness::new(
        r#"import { exec, execFile } from "node:child_process"
import { promisify } from "node:util"
const run = promisify(execFile)
const shell = promisify(exec)
export default {
  transformations: {
    upper: { name: "Upper", transform: async (text) => (await run("tr", ["a-z", "A-Z"], { input: text })).stdout },
    exit: {
      name: "Exit",
      transform: () => new Promise((resolve) => {
        exec("echo out; echo err >&2; exit 3", (error, stdout, stderr) =>
          resolve(`${error.code} ${error.killed} ${error.signal} ${stdout.trim()} ${stderr.trim()} ${error.message.startsWith("Command failed: ")}`))
      }),
    },
    env: { name: "Env", transform: async () => (await shell("echo \"$HEX_TEST:$CARGO_MANIFEST_DIR\"", { env: { HEX_TEST: "set" } })).stdout.trim() },
    missing: { name: "Missing", transform: async () => { try { await run("hex-no-such-program") } catch (error) { return `${error.code} ${error.syscall}` } } },
    slow: { name: "Slow", transform: async () => { try { await shell("sleep 5", { timeout: 100 }) } catch (error) { return `${error.killed} ${error.signal} ${error.code}` } } },
    big: { name: "Big", transform: async () => { try { await shell("yes | head -c 5000", { maxBuffer: 1000 }) } catch (error) { return `${error.code} ${error.stdout.length}` } } },
  },
}
"#,
    );
    let permissions = harness.worker.status.permissions.clone().unwrap();
    assert!(permissions.exec && !permissions.fetch);
    assert!(permissions.partial, "promisify hides the programs");
    assert!(
        harness
            .transform(&["upper"], "hi")
            .unwrap_err()
            .contains("has not allowed this config to run programs")
    );
    harness.allow();
    assert_eq!(harness.transform(&["upper"], "hello").unwrap(), "HELLO");
    assert_eq!(
        harness.transform(&["exit"], "").unwrap(),
        "3 false null out err true"
    );
    assert_eq!(harness.transform(&["env"], "").unwrap(), "set:");
    assert_eq!(
        harness.transform(&["missing"], "").unwrap(),
        "ENOENT spawn hex-no-such-program"
    );
    let started = Instant::now();
    assert_eq!(
        harness.transform(&["slow"], "").unwrap(),
        "true SIGTERM null"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        harness.transform(&["big"], "").unwrap(),
        "ERR_CHILD_PROCESS_STDIO_MAXBUFFER 1000"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn the_transformation_timeout_ends_the_whole_process_tree() {
    let harness = Harness::new(
        r#"import { exec } from "node:child_process"
export default {
  transformations: {
    hang: {
      name: "Hang",
      timeout: 300,
      transform: () => new Promise((resolve) => exec("sleep 30 & echo $! > child.pid; wait", () => resolve("done"))),
    },
  },
}
"#,
    );
    harness.allow();
    let started = Instant::now();
    assert_eq!(
        harness.transform(&["hang"], "").unwrap_err(),
        "timed out after 300 ms"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let pid = fs::read_to_string(harness.worker.workspace.join("child.pid")).unwrap();
    let stat = format!("/proc/{}/stat", pid.trim());
    let deadline = Instant::now() + Duration::from_secs(2);
    // Gone, or a zombie its new parent has yet to reap.
    while fs::read_to_string(&stat).is_ok_and(|stat| {
        !stat
            .rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z')
    }) {
        assert!(
            Instant::now() < deadline,
            "sleep outlived its transformation"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Dictation's processing stage over a worker running on its own thread,
/// as Hex runs it.
#[test]
fn a_throwing_or_slow_transformation_pastes_the_text_unchanged() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("hex-fallback-{unique}"));
    let workspace = root.join("hex");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        workspace.join("hex.config.ts"),
        r#"export default {
  transformations: {
    slow: { name: "Slow", timeout: 200, transform: () => new Promise((resolve) => setTimeout(resolve, 5000, "late")) },
    broken: { name: "Broken", transform: () => { throw new Error("no") } },
  },
}
"#,
    )
    .unwrap();
    let client = Arc::new(TransformationClient::default());
    let (requests, receiver) = mpsc::sync_channel(4);
    let stop = Arc::new(AtomicBool::new(false));
    let running = {
        let (client, requests, stop) = (client.clone(), requests.clone(), stop.clone());
        thread::Builder::new()
            .stack_size(script::THREAD_STACK_SIZE)
            .spawn(move || {
                Worker {
                    transformations: client,
                    requests,
                    workspace,
                    status_file: None,
                    trust_file: None,
                    generation: 0,
                    loaded: None,
                    status: StatusSnapshot::default(),
                }
                .run(&receiver, &stop);
            })
            .unwrap()
    };
    let ready = Instant::now() + Duration::from_secs(5);
    while client
        .active
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .is_none()
    {
        assert!(Instant::now() < ready, "the config never loaded");
        thread::sleep(Duration::from_millis(10));
    }
    for (id, why) in [("slow", "timed out after 200 ms"), ("broken", "Error: no")] {
        let mut processed = crate::dictation::processor::Processed {
            text: "as heard".into(),
            observation: None,
            transformations: vec![id.into()],
        };
        processed.apply_transformations(
            &client,
            &ContextSnapshot::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(processed.text, "as heard");
        let fallback = processed.observation.unwrap().fallback.unwrap();
        assert!(fallback.starts_with(why), "{fallback}");
    }
    stop.store(true, Ordering::Release);
    let _ = requests.try_send(Request::Reload);
    running.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}
