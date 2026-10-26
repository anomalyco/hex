//! `hex mcp` speaks MCP over stdio against synthetic fixture meetings.
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

#[test]
fn mcp_server_initializes_lists_tools_and_answers_calls_over_stdio() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings");
    let support =
        std::env::temp_dir().join(format!("voice-control-mcp-support-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_voice-control"))
        .arg("mcp")
        .arg("--meetings-dir")
        .arg(&fixtures)
        .env("HEX_APPLICATION_SUPPORT_DIR", &support)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let mut send = move |message: Value| {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    };
    let receive = |id: u64| -> Value {
        loop {
            let line = receiver
                .recv_timeout(Duration::from_secs(20))
                .expect("MCP response");
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("stdout must carry only JSON-RPC: {error}: {line}"));
            if message["id"] == json!(id) {
                return message;
            }
        }
    };

    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "hex-test", "version": "1"}
        }}),
    );
    let initialized = receive(1);
    assert_eq!(initialized["result"]["serverInfo"]["name"], "hex-meetings");
    assert!(initialized["result"]["capabilities"]["tools"].is_object());
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
    let tools = receive(2);
    let tools = tools["result"]["tools"].as_array().unwrap();
    let mut names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "get_chapters",
            "get_meeting_notes",
            "get_transcript",
            "list_meetings",
            "search_transcripts"
        ]
    );
    for tool in tools {
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "{tool}");
        assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
    }
    let search = tools
        .iter()
        .find(|tool| tool["name"] == "search_transcripts")
        .unwrap();
    assert_eq!(search["inputSchema"]["required"], json!(["query"]));
    let transcript = tools
        .iter()
        .find(|tool| tool["name"] == "get_transcript")
        .unwrap();
    for bound in ["start", "end", "from", "to"] {
        assert!(
            transcript["inputSchema"]["properties"][bound].is_object(),
            "{transcript}"
        );
    }

    send(
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "search_transcripts",
            "arguments": {"query": "Dax", "limit": 3}
        }}),
    );
    let result = receive(3);
    let text = result["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1790000000000-1001"), "{text}");
    assert!(text.contains("**Dax**"), "{text}");

    send(
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
            "name": "get_transcript",
            "arguments": {"meeting_id": "1790000000000-1001", "start": 20, "end": "0:30"}
        }}),
    );
    let result = receive(4);
    let text = result["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Showing 00:16–00:31"), "{text}");

    send(
        json!({"jsonrpc": "2.0", "id": 41, "method": "tools/call", "params": {
            "name": "get_transcript",
            "arguments": {
                "meeting_id": "1790000000000-1001", "start": null, "end": null,
                "from": "0:20", "to": 30
            }
        }}),
    );
    let result = receive(41);
    let text = result["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Showing 00:16–00:31"), "{text}");
    assert!(!text.contains("write the spec"), "{text}");

    for (id, arguments, error) in [
        (
            42,
            json!({"meeting_id": "1790000000000-1001", "from": "0:30", "to": "0:20"}),
            "The range start 00:30 must be earlier than its end 00:20.",
        ),
        (
            43,
            json!({"meeting_id": "1790000000000-1001", "start": "5:00"}),
            "The range starts at 05:00, after the meeting's end at 00:40.",
        ),
        (
            44,
            json!({"meeting_id": "1790000000000-1001", "to": 0}),
            "The range start 00:00 must be earlier than its end 00:00.",
        ),
        (
            45,
            json!({"meeting_id": "1790000000000-1001", "start": "0:10", "from": "0:20"}),
            "Give either start or from, not both.",
        ),
    ] {
        send(
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
                "name": "get_transcript", "arguments": arguments
            }}),
        );
        let result = receive(id);
        assert_eq!(result["result"]["isError"], true, "{result}");
        let text = result["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(text, error);
    }

    send(
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
            "name": "get_meeting_notes",
            "arguments": {"meeting_id": "missing"}
        }}),
    );
    let result = receive(5);
    assert_eq!(result["result"]["isError"], true, "{result}");

    send(
        json!({"jsonrpc": "2.0", "id": 51, "method": "tools/call", "params": {
            "name": "get_chapters",
            "arguments": {"meeting_id": "1790000000000-1001"}
        }}),
    );
    let result = receive(51);
    let text = result["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("No chapters are stored for this meeting."),
        "{text}"
    );

    send(
        json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": {
            "uri": "hex-meeting://1790000000000-1001"
        }}),
    );
    let result = receive(6);
    let markdown = result["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(markdown.starts_with("# Weekly sync"), "{result}");

    drop(send);
    let status = child.wait().unwrap();
    assert!(status.success(), "{status}");
    assert!(
        !support.exists(),
        "hex mcp must not create Application Support state"
    );
}
