//! MCP (Model Context Protocol) server exposing MeshMind to AI agents.
//!
//! Transport-agnostic: feed it JSON-RPC messages and it returns replies.
//! `main.rs` wires it to stdio; the desktop app embeds the same [`Server`].

mod tools;

use meshmind_core::Scene;
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub struct Server {
    pub scene: Scene,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            scene: Scene::new(),
        }
    }

    /// Handle one line of JSON-RPC. Returns the serialized reply, or `None`
    /// for notifications, which get no reply.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle(&msg)?,
            Err(e) => error_reply(Value::Null, -32700, &format!("parse error: {e}")),
        };
        Some(reply.to_string())
    }

    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // Messages without an id are notifications (e.g. notifications/initialized).
        let id = id?;

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "meshmind", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "MeshMind is a 3D modelling engine. Build models by creating \
                    primitives and editing them, then assign PBR materials. Units are metres, Y is up."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::definitions() })),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match tools::call(&mut self.scene, name, &args) {
                    Some(outcome) => Ok(tool_result(outcome)),
                    None => Err((-32602, format!("unknown tool: {name}"))),
                }
            }
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_reply(id, code, &message),
        })
    }
}

/// Tool failures are reported inside the result (with `isError`) so the
/// agent can read the message and correct itself.
fn tool_result(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(data) => json!({
            "content": [{ "type": "text", "text": data.to_string() }],
            "structuredContent": data,
            "isError": false
        }),
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    }
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
