//! MCP (Model Context Protocol) server exposing MeshMind to AI agents.
//!
//! Transport-agnostic core: feed it JSON-RPC messages and it returns replies.
//! [`http`] serves it over Streamable HTTP; `main.rs` also offers stdio.

pub mod http;
mod tools;

use base64::Engine;
use meshmind_core::Scene;
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_UNDO: usize = 100;

const INSTRUCTIONS: &str = "MeshMind is a 3D modelling engine you drive with tools. Units are metres, Y is up, \
the ground is y = 0. Workflow: block out the shape with primitives, refine with extrude/inset/bevel/boolean/\
subdivide, assign PBR materials, then call render_preview and fix anything that looks wrong. Small bevels on \
hard edges and believable materials make models look professional. Every change can be undone.";

pub struct Server {
    pub scene: Scene,
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    /// Increases on every scene change, so viewers know when to refresh.
    pub revision: u64,
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
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
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
        // Messages without an id are notifications (e.g. notifications/initialized).
        let id = msg.get("id").cloned()?;
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "meshmind", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
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
                match self.call_tool(name, &args) {
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

    fn call_tool(&mut self, name: &str, args: &Value) -> Option<Result<tools::ToolOutput, String>> {
        match name {
            "undo" => return Some(self.step_history(true)),
            "redo" => return Some(self.step_history(false)),
            _ => {}
        }
        if !tools::is_mutating(name) {
            return tools::call(&mut self.scene, name, args);
        }
        let before = self.scene.clone();
        let outcome = tools::call(&mut self.scene, name, args)?;
        match outcome {
            Ok(_) => {
                self.undo.push(before);
                if self.undo.len() > MAX_UNDO {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.revision += 1;
            }
            // Leave the scene exactly as it was if a tool fails half way.
            Err(_) => self.scene = before,
        }
        Some(outcome)
    }

    fn step_history(&mut self, undo: bool) -> Result<tools::ToolOutput, String> {
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let scene = from.pop().ok_or(if undo {
            "nothing to undo"
        } else {
            "nothing to redo"
        })?;
        to.push(std::mem::replace(&mut self.scene, scene));
        self.revision += 1;
        Ok(json!({ "undo_steps": self.undo.len(), "redo_steps": self.redo.len() }).into())
    }
}

/// Tool failures are reported inside the result (with `isError`) so the
/// agent can read the message and correct itself.
fn tool_result(outcome: Result<tools::ToolOutput, String>) -> Value {
    match outcome {
        Ok(out) => {
            let mut content = vec![json!({ "type": "text", "text": out.data.to_string() })];
            if let Some(png) = out.image_png {
                let data = base64::engine::general_purpose::STANDARD.encode(png);
                content.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
            }
            json!({ "content": content, "structuredContent": out.data, "isError": false })
        }
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    }
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
