//! Runs the real `shine-mcp` binary and talks MCP to it over stdio.
#![cfg(feature = "gui")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn mcp_session_over_stdio() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shine-mcp"))
        .env("SHINE_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn shine-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut call = |message: Value| -> Option<Value> {
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
        message.get("id")?;
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        Some(serde_json::from_str(&line).expect("stdout carries only JSON-RPC"))
    };

    let init = call(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}))
    .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "shine");
    call(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let tools = call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 6);

    let sim = call(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "simulate_circuit", "arguments": {"circuit": {
        "blocks": [{"id": "l", "type": "laser"}, {"id": "y", "type": "y_splitter"}, {"id": "a", "type": "detector"}, {"id": "b", "type": "detector"}],
        "connections": [{"from": "l.out", "to": "y.in"}, {"from": "y.out1", "to": "a.in"}, {"from": "y.out2", "to": "b.in"}]}}}}))
    .unwrap();
    assert_eq!(sim["result"]["structuredContent"]["readings"]["a"], 0.5);

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
