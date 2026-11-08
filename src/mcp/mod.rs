//! Model Context Protocol server: lets AI assistants design and simulate
//! photonic circuits with Shine.
//!
//! Speaks JSON-RPC 2.0 over stdio, one message per line (MCP stdio
//! transport). Run it with `shine-mcp`; logs go to stderr so stdout carries
//! only protocol messages.

pub mod spec;
pub mod tools;

use std::io::{BufRead, Write};

use serde_json::{Map, Value, json};

/// Newest first; the first is offered when the client asks for an unknown version.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "Shine simulates photonic circuits exactly in the frequency domain. \
Start with list_blocks to see block types, ports and parameters. Describe a circuit as blocks plus \
connections between \"block.port\" names and call simulate_circuit (needs a laser) or sweep_wavelength \
(drives an open input). Use decompose_matrix to program MZI meshes, save_project to hand a circuit to \
a person in the Shine editor, and simulate_project to evaluate projects they saved.";

#[derive(Default)]
pub struct Server {
    pub initialized: bool,
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

impl Server {
    /// Handle one line of input; returns the line to send back, if any.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        let response = match serde_json::from_str::<Value>(line) {
            Ok(message) => self.handle(message),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
        };
        response.map(|r| r.to_string())
    }

    /// Handle one JSON-RPC message; notifications get no response.
    pub fn handle(&mut self, message: Value) -> Option<Value> {
        let Some(object) = message.as_object() else {
            return Some(error(
                Value::Null,
                INVALID_REQUEST,
                "expected a JSON-RPC object (batches are not supported)",
            ));
        };
        let id = object.get("id").cloned();
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response to something we never sent, or garbage.
            return id.map(|id| error(id, INVALID_REQUEST, "missing method"));
        };
        let empty = Map::new();
        let params = object
            .get("params")
            .and_then(Value::as_object)
            .unwrap_or(&empty);

        let Some(id) = id else {
            if method == "notifications/initialized" {
                self.initialized = true;
            }
            tracing::debug!("notification {method}");
            return None;
        };
        tracing::debug!("request {method}");
        Some(match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|v| **v == requested)
                    .unwrap_or(&PROTOCOL_VERSIONS[0]);
                result(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {"name": "shine", "title": "Shine photonics", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": INSTRUCTIONS,
                    }),
                )
            }
            "ping" => result(id, json!({})),
            "tools/list" => {
                let tools: Vec<Value> = tools::tools()
                    .iter()
                    .map(|t| json!({"name": t.name, "title": t.title, "description": t.description, "inputSchema": (t.input_schema)()}))
                    .collect();
                result(id, json!({"tools": tools}))
            }
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(error(id, INVALID_PARAMS, "tools/call needs a tool name"));
                };
                let Some(tool) = tools::tools().into_iter().find(|t| t.name == name) else {
                    return Some(error(id, INVALID_PARAMS, format!("unknown tool: {name}")));
                };
                let arguments = params
                    .get("arguments")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let start = web_time::Instant::now();
                let outcome = (tool.call)(&arguments);
                tracing::info!(
                    "tool {name} {} in {:?}",
                    if outcome.is_ok() {
                        "succeeded"
                    } else {
                        "failed"
                    },
                    start.elapsed()
                );
                // Tool failures are results the model can read and correct, not protocol errors.
                match outcome {
                    Ok(value) => result(
                        id,
                        json!({
                            "content": [{"type": "text", "text": serde_json::to_string_pretty(&value).expect("serialisable")}],
                            "structuredContent": value,
                            "isError": false,
                        }),
                    ),
                    Err(message) => result(
                        id,
                        json!({"content": [{"type": "text", "text": message}], "isError": true}),
                    ),
                }
            }
            other => error(id, METHOD_NOT_FOUND, format!("method not found: {other}")),
        })
    }
}

/// Serve requests from `input` until it closes, writing responses to `output`.
pub fn serve(input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut server = Server::default();
    for line in input.lines() {
        if let Some(response) = server.handle_line(&line?) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(server: &mut Server, method: &str, params: Value) -> Value {
        server
            .handle(json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params}))
            .unwrap()
    }

    #[test]
    fn initialize_negotiates_the_protocol_version() {
        let mut s = Server::default();
        let r = request(
            &mut s,
            "initialize",
            json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
        );
        assert_eq!(r["id"], 7);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "shine");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert!(
            r["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("list_blocks")
        );
        let r = request(
            &mut s,
            "initialize",
            json!({"protocolVersion": "1999-01-01"}),
        );
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);

        assert!(!s.initialized);
        assert_eq!(
            s.handle(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
            None
        );
        assert!(s.initialized);
    }

    #[test]
    fn lists_and_calls_tools() {
        let mut s = Server::default();
        let r = request(&mut s, "tools/list", json!({}));
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "list_blocks",
                "simulate_circuit",
                "sweep_wavelength",
                "decompose_matrix",
                "simulate_project",
                "save_project"
            ]
        );
        assert!(r["result"]["tools"][1]["inputSchema"]["properties"]["circuit"].is_object());

        let r = request(
            &mut s,
            "tools/call",
            json!({"name": "list_blocks", "arguments": {}}),
        );
        assert_eq!(r["result"]["isError"], false);
        assert!(r["result"]["structuredContent"]["blocks"].is_array());
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            serde_json::from_str::<Value>(text).is_ok(),
            "text content is the same JSON"
        );

        // Tool failures come back as readable results.
        let r = request(
            &mut s,
            "tools/call",
            json!({"name": "simulate_circuit", "arguments": {}}),
        );
        assert_eq!(r["result"]["isError"], true);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("circuit")
        );
        // Missing arguments are treated as empty.
        let r = request(&mut s, "tools/call", json!({"name": "list_blocks"}));
        assert_eq!(r["result"]["isError"], false);
    }

    #[test]
    fn protocol_errors() {
        let mut s = Server::default();
        assert_eq!(
            request(&mut s, "tools/call", json!({"name": "nope"}))["error"]["code"],
            INVALID_PARAMS
        );
        assert_eq!(
            request(&mut s, "tools/call", json!({}))["error"]["code"],
            INVALID_PARAMS
        );
        assert_eq!(
            request(&mut s, "resources/list", json!({}))["error"]["code"],
            METHOD_NOT_FOUND
        );
        assert_eq!(request(&mut s, "ping", json!({}))["result"], json!({}));
        assert_eq!(
            s.handle(json!([1, 2])).unwrap()["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(
            s.handle(json!({"id": 3})).unwrap()["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(
            s.handle(json!({"result": {}})),
            None,
            "stray responses are ignored"
        );
        assert_eq!(
            s.handle(json!({"method": "notifications/cancelled", "params": {}})),
            None
        );
        let parse = s.handle_line("{not json").unwrap();
        assert!(
            parse.contains("-32700") && parse.contains("\"id\":null"),
            "{parse}"
        );
        assert_eq!(s.handle_line("   "), None);
    }

    #[test]
    fn serves_a_whole_session_over_streams() {
        let input = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": "two", "method": "tools/call", "params": {"name": "decompose_matrix", "arguments": {"matrix": [[0, 1], [1, 0]], "kind": "unitary"}}}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output).unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "no reply to the notification");
        assert_eq!(lines[1]["id"], "two");
        assert_eq!(lines[1]["result"]["structuredContent"]["mzi_count"], 1);
    }
}
