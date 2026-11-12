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
the ground is y = 0, and objects are created centred on the origin. A good workflow:\n\
1. Block out: build the big shapes from primitives and place them with set_transform. Check proportions \
with render_preview before adding detail.\n\
2. Detail: inset then extrude (using the returned inner_faces) for panels and recesses, boolean for holes \
and cut-outs, array and mirror for repeated parts.\n\
3. Finish shape: bevel hard edges (small amounts, 2 to 3 segments) so they catch light. Do this before \
materials, because any geometry edit clears UVs and baked textures.\n\
4. Surface: apply_smart_material per object (edge wear, grime and normal maps are baked from the shape). \
Use set_material only for flat colours or emissive parts.\n\
5. Review: render_preview from several views and lighting presets; fix anything that looks wrong.\n\
6. Deliver: export_glb for game engines, and save_project so the work can be continued later.\n\
Every change can be undone. Prompts named model_asset and refine_asset describe this workflow in detail.";

/// Prompt templates the user's agent can offer (MCP `prompts`).
fn prompts() -> Value {
    json!([
        {
            "name": "model_asset",
            "title": "Model a game asset",
            "description": "Build a game-ready 3D model from a description, with materials, and export it.",
            "arguments": [
                { "name": "description", "description": "What to build, e.g. 'a rusty oil barrel with two metal bands'.", "required": true },
                { "name": "output", "description": "Where to save the .glb (default: asset.glb).", "required": false }
            ]
        },
        {
            "name": "refine_asset",
            "title": "Refine the current model",
            "description": "Review the model in the scene and improve it step by step.",
            "arguments": [
                { "name": "feedback", "description": "What to change, e.g. 'make the handles thicker and the paint more worn'.", "required": true }
            ]
        }
    ])
}

fn get_prompt(params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arg = |k: &str| {
        params
            .get("arguments")
            .and_then(|a| a.get(k))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let text = match name {
        "model_asset" => {
            let description = arg("description")
                .ok_or((-32602, "argument `description` is required".to_string()))?;
            let output = arg("output").unwrap_or_else(|| "asset.glb".into());
            format!(
                "Using the MeshMind tools, model this game asset: {description}\n\n\
                 Work like a professional 3D artist:\n\
                 1. Decide the real-world size in metres and the main parts. Start with clear_scene.\n\
                 2. Block out each part with create_primitive and set_transform, giving every object a \
                 descriptive name. Call render_preview and fix proportions before adding detail.\n\
                 3. Add detail: inset + extrude for panels, boolean for holes, array/mirror for repeats.\n\
                 4. Bevel hard edges slightly (2-3 segments) so they catch light.\n\
                 5. apply_smart_material on each part with a fitting preset and colour.\n\
                 6. render_preview under at least two lighting presets, then fix anything that looks off.\n\
                 7. export_glb to {output} and save_project next to it (same name, .meshmind).\n\
                 Finish with a short summary of what you built."
            )
        }
        "refine_asset" => {
            let feedback =
                arg("feedback").ok_or((-32602, "argument `feedback` is required".to_string()))?;
            format!(
                "Using the MeshMind tools, improve the current model: {feedback}\n\n\
                 First call list_objects and render_preview to see what exists. Make the changes with \
                 the smallest edits that work, and re-apply smart materials to any object whose shape \
                 you changed. Check the result with render_preview, and use undo if a step made things worse."
            )
        }
        _ => return Err((-32602, format!("unknown prompt: {name}"))),
    };
    Ok(json!({
        "description": prompts().as_array().unwrap().iter()
            .find(|p| p["name"] == name).map(|p| p["description"].clone()).unwrap_or_default(),
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }]
    }))
}

/// Markdown reference of every tool, generated from the definitions agents
/// see. `docs/TOOLS.md` is this output; a test keeps the two in sync.
pub fn tool_reference_markdown() -> String {
    let mut out = String::from(
        "# Tool reference\n\n<!-- Generated by `cargo run -p meshmind-mcp --example tool_docs > docs/TOOLS.md`. Do not edit by hand. -->\n\n\
         These are the tools MeshMind gives your AI agent. You normally don't call them yourself; \
         the agent reads these descriptions and picks the right ones.\n",
    );
    for t in tools::definitions().as_array().expect("tool list") {
        out.push_str(&format!(
            "\n## `{}`\n\n{}\n",
            t["name"].as_str().unwrap_or_default(),
            t["description"].as_str().unwrap_or_default()
        ));
        let schema = &t["inputSchema"];
        let required: Vec<&str> = schema["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let Some(props) = schema["properties"].as_object().filter(|p| !p.is_empty()) else {
            continue;
        };
        out.push_str("\n| Argument | Type | Description |\n|---|---|---|\n");
        for (name, p) in props {
            let mut ty = p["type"].as_str().unwrap_or("any").to_string();
            if let Some(values) = p["enum"].as_array() {
                let v: Vec<String> = values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|v| format!("`{v}`"))
                    .collect();
                ty = format!("one of {}", v.join(", "));
            } else if ty == "array" {
                if let Some(it) = p["items"]["type"].as_str() {
                    ty = format!("array of {it}");
                }
            }
            let desc = p["description"]
                .as_str()
                .unwrap_or_default()
                .replace('|', "\\|");
            let req = if required.contains(&name.as_str()) {
                " (required)"
            } else {
                ""
            };
            out.push_str(&format!("| `{name}`{req} | {ty} | {desc} |\n"));
        }
    }
    out
}

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
                "capabilities": {
                    "tools": { "listChanged": false },
                    "prompts": { "listChanged": false },
                },
                "serverInfo": { "name": "meshmind", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "prompts/list" => Ok(json!({ "prompts": prompts() })),
            "prompts/get" => get_prompt(&params),
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
