//! Real send pipeline against a disposable HTTP service and a 60 ms fake SSH hop.
use super::*;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
#[ignore = "paired remote transport benchmark, one warmup and seven measured sends"]
fn paired_remote_delivery() {
    let root = std::env::temp_dir().join(format!("hex-remote-bench-{}", std::process::id()));
    fs::create_dir_all(root.join("opencode")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    fs::write(
        root.join("opencode/service.json"),
        serde_json::json!({"url":format!("http://{address}")}).to_string(),
    )
    .unwrap();
    executable(
        &root.join("ssh"),
        r#"#!/bin/sh
sleep 0.06
case "$1" in
  *'tar -C'*) cat >/dev/null; echo copied >> "$HOME/copies" ;;
  *) exec /bin/sh -c "$1" ;;
esac
"#,
    );
    executable(
        &root.join("bin/opencode"),
        r#"#!/usr/bin/env python3
import http.client,json,os,sys,urllib.parse
service=json.load(open(os.path.join(os.environ['HOME'],'opencode/service.json')))
url=urllib.parse.urlsplit(service['url'])
c=http.client.HTTPConnection(url.hostname,url.port,timeout=5)
body=sys.argv[5] if len(sys.argv)>5 else None
c.request(sys.argv[2].upper(),sys.argv[3],body,{'Content-Type':'application/json'})
r=c.getresponse()
sys.stdout.buffer.write(r.read())
"#,
    );
    let bundle = root.join("fixture.scrawl");
    fs::create_dir(&bundle).unwrap();
    fs::write(
        bundle.join("scrawl.json"),
        serde_json::json!({
            "version":1,"id":"fixture","createdAtMs":0,"durationMs":1000,
        "target":{"mode":"display","displayId":1,"frame":{"x":0,"y":0,"width":100,"height":100,"scale":1}},
            "transcript":{"source":"fixture","text":"fixture","segments":[]},
            "strokes":[],"marks":[],"events":[],"keyframes":[],"contactSheet":""
        })
        .to_string(),
    )
    .unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..32 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
            }
            let header = String::from_utf8(bytes).unwrap();
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .map(|s| s.parse::<usize>().unwrap())
                .unwrap_or(0);
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            let body = if header.starts_with("POST ") {
                let prompt: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert!(prompt.to_string().contains("fixture"));
                r#"{"data":{"id":"msg_fixture"}}"#
            } else {
                assert!(header.starts_with("GET /api/session/active "));
                r#"{"data":{}}"#
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    for before in [true, false] {
        let mut values = Vec::new();
        let _guard = crate::env::enter(Arc::new(crate::env::Env {
            remote_fixture: Some(root.clone()),
            config: crate::RouterConfig {
                agent_host: Some("fixture".into()),
                agent_opencode: before.then(|| "opencode".into()),
                ..Default::default()
            },
            ..Default::default()
        }));
        for sample in 0..8 {
            let start = Instant::now();
            let sent = send_bundle(&bundle, "ses_fixture").unwrap();
            assert_eq!(sent.message_id.as_deref(), Some("msg_fixture"));
            if sample > 0 {
                values.push(start.elapsed().as_millis());
            }
        }
        values.sort();
        eprintln!(
            "METRIC remote_{}_send_ms={} range={}-{}",
            if before { "before" } else { "after" },
            values[3],
            values[0],
            values[6]
        );
    }
    server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read_to_string(root.join("copies"))
        .unwrap_or_default()
        .lines()
        .count()
        < 16
    {
        assert!(
            Instant::now() < deadline,
            "background transfers didn't finish"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    }
    fs::remove_dir_all(root).unwrap();
}
