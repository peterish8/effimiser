//! Drive the real binary over stdio, as a host process would.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn serves_newline_delimited_json_rpc_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), b"hello\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ctx-mcp"))
        .arg("--root")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let frames = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "not json",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ctx_snapshot","arguments":{"path":"note.txt"}}}"#,
    ];
    for f in frames {
        writeln!(stdin, "{f}").unwrap();
    }
    drop(stdin);

    let mut lines = Vec::new();
    let mut buf = String::new();
    while stdout.read_line(&mut buf).unwrap() > 0 {
        lines.push(serde_json::from_str::<serde_json::Value>(buf.trim()).unwrap());
        buf.clear();
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(lines.len(), 3, "the notification gets no response");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["error"]["code"], -32700);
    assert_eq!(lines[2]["result"]["structuredContent"]["byte_len"], 6);
}
