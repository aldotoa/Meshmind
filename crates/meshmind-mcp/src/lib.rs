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
the ground is y = 0, and objects are created centred on the origin. Aim for game-ready, AAA-looking assets. \
Work like a professional 3D artist:\n\
1. Plan: decide the real-world size in metres and list the parts (a sword: blade, guard, grip, wrap, ferrules, \
pommel). Real objects are made of many separate parts; one object per part.\n\
2. Block out: build each part with the shape tool that matches its silhouette, place it with set_transform, \
and check proportions with render_preview before adding detail. Boxes and plain cylinders look like \
placeholders, so use create_primitive only for boxy parts.\n\
   - loft: parts whose cross-section changes along their length (blades, horns, handles, legs, spires, \
     fins). A concave cross-section gives grooves such as a sword's fuller.\n\
   - lathe: round parts (pommels, knobs, bottles, vases, barrels, wheels, columns, lamp bases).\n\
   - extrude_outline: flat parts with a distinctive silhouette (axe heads, shields, brackets, gears).\n\
   - sweep: tubes along a path (rope, cables, pipes, rings, chains, cord or leather wrapped round a grip).\n\
   - deform (bend, twist, taper) and move_vertices: curve, twist or reshape a part after creating it.\n\
   - sculpt_body: organic forms (creatures, animals, tentacles, rocks, roots, food) as overlapping \
     ellipsoids, tapered capsules and rounded boxes melted into ONE smooth surface.\n\
CHARACTERS AND CREATURES are different from props: never assemble a body from separate spheres, cylinders \
and boxes (it looks like a stack of toys). Start every humanoid with create_character (species human, orc, \
goblin, dwarf or troll; options for height, bulk, gut, hunch, jaw, ears, feminine, pose). It returns one \
smooth body with anatomy, eyeballs and landmarks (eyes, mouth, lower canines, hand grips, waist_front/left/back, \
shoulder_top, forearm). Then: (a) adjust proportions with sculpt_body id + edit_parts by part name; (b) add \
character with sculpt: crease for frown lines, nose-to-mouth folds, muscle separations and wrinkles, inflate \
for cheekbones, muscles and veins, noise for rough hide (strokes mirror left/right and snap to the skin); \
(c) build tusks, horns and claws as small sculpt_body capsules tapering to radius 0, and clothing, armour, \
belts, bracers and weapons as separate props sized from the landmarks, overlapping the skin slightly so \
nothing floats; (d) apply_smart_material: skin on the body (orc #3d5226, human #c58c6a), bone on tusks, \
leather, fabric and metal on gear. Anatomy that sells a character: a neck (never a head sitting on the \
shoulders), shoulders sloping from the neck, elbows at the waist, wrists at the crotch, hands about the size \
of the face, feet as long as the forearm, knees halfway between hip and heel, and a clear silhouette \
(orcs: broad chest, narrow waist, hunched; goblins: big head, ears and nose, thin limbs). Check the face \
with render_preview focus_point = landmarks.head_center, focus_radius about 0.15 times the height.\n\
3. Detail: inset then extrude (using the returned inner_faces) for panels and recesses, boolean for holes \
and cut-outs, array and mirror for repeats. Small trim parts (rings, rivets, ferrules, straps, hinges) add \
a lot of realism.\n\
4. Finish shape: bevel hard edges (1 to 3% of the part's size, 2 to 3 segments) so they catch light. Shape \
first, materials last: any geometry edit clears UVs and baked textures.\n\
5. Surface: apply_smart_material on every part; it bakes edge wear, grime and a normal map from the shape. \
Presets: polished_metal (blades, armour, chrome), bare_metal (cast or machined parts), painted_metal, \
rusty_metal, gold (also brass), bronze (patina), skin (characters; give the skin colour), wood, leather (grips, straps), fabric (cloth, sacks, \
banners), stone (also concrete), ceramic (pottery, tiles), bone (also ivory, horn), plastic, rubber, glass \
and gemstone (see-through; color tints them). Pick colours with color_hex and keep wear and dirt believable: \
0.2 to 0.4 for used items, higher only for battered ones. Use set_material only for flat colours, glowing \
parts (emissive) or custom glass (transmission, ior).\n\
6. Review like an art director: render_preview from several views under at least two lighting presets, use \
`focus` for close-ups of small parts, and call check_asset. Fix floating or intersecting parts, wrong \
proportions, grey parts and stale materials, then look again.\n\
7. Deliver: export_glb for game engines, and save_project so the work can be continued later.\n\
Common mistakes: everything built from cubes and cylinders; one object for the whole model; parts hovering \
just off each other; the model not standing on y = 0; skipping bevels; wear or dirt at 1.0 on everything; \
editing shape after materials without re-applying them. Every change can be undone. Prompts model_asset, \
refine_asset and review_asset describe these workflows, and docs/RECIPES.md in the MeshMind repository shows \
worked examples (sword, axe, barrel, potion bottle, orc warrior).";

/// Prompt templates the user's agent can offer (MCP `prompts`).
fn prompts() -> Value {
    json!([
        {
            "name": "model_asset",
            "title": "Model a game asset",
            "description": "Build a game-ready 3D model from a description, with materials, and export it.",
            "arguments": [
                { "name": "description", "description": "What to build, e.g. 'a rusty oil barrel with two metal bands'.", "required": true },
                { "name": "output", "description": "Where to save the .glb (default: asset.glb).", "required": false },
                { "name": "style", "description": "realistic (default), stylized (hand-painted, chunky shapes) or low_poly.", "required": false }
            ]
        },
        {
            "name": "refine_asset",
            "title": "Refine the current model",
            "description": "Review the model in the scene and improve it step by step.",
            "arguments": [
                { "name": "feedback", "description": "What to change, e.g. 'make the handles thicker and the paint more worn'.", "required": true }
            ]
        },
        {
            "name": "review_asset",
            "title": "Review and polish the current model",
            "description": "Critique the model like an art director, then fix what falls short of AAA quality.",
            "arguments": [
                { "name": "focus", "description": "Optional area to look at hardest, e.g. 'the hilt' or 'materials'.", "required": false }
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
            let style = match arg("style").as_deref() {
                Some("stylized") => "Style: stylized, like a hand-painted fantasy game. Exaggerate proportions \
                    slightly (chunkier, bolder shapes), use bigger bevels, saturated colours and low dirt (0.1).",
                Some("low_poly") => "Style: low-poly. Use few segments (6 to 8 round, 3 to 4 stations per loft), \
                    flat shading where facets read well, no subdivision, and clean flat colours with light wear.",
                _ => "Style: realistic. Real-world proportions, small bevels, believable wear (0.2 to 0.4).",
            };
            format!(
                "Using the MeshMind tools, model this game asset: {description}\n{style}\n\n\
                 Work like a professional 3D artist:\n\
                 1. Decide the real-world size in metres and list the parts. Start with clear_scene.\n\
                 If it is a character or creature, start the body with create_character (humanoids) or \
                 sculpt_body (other creatures), shape it with sculpt_body edit_parts and sculpt brushes, \
                 and add gear as separate parts placed at the returned landmarks. Never build a body from \
                 separate primitives.\n\
                 2. Block out each part with the shape tool that fits its silhouette (loft for tapering \
                 parts, lathe for round parts, extrude_outline for flat silhouettes, sweep for tubes and \
                 wraps; create_primitive only for boxy parts) and place it with set_transform, giving \
                 every object a descriptive name. Call render_preview and fix proportions before adding detail.\n\
                 3. Add detail: inset + extrude for panels, boolean for holes, array/mirror for repeats, \
                 and small trim parts (rings, rivets, straps).\n\
                 4. Bevel hard edges slightly (2-3 segments) so they catch light.\n\
                 5. apply_smart_material on every part with a fitting preset and colour.\n\
                 6. Review: render_preview under at least two lighting presets, close-ups of small parts \
                 with `focus`, and check_asset. Fix everything it finds and look again.\n\
                 7. export_glb to {output} and save_project next to it (same name, .meshmind).\n\
                 Finish with a short summary of what you built, its size and triangle count."
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
        "review_asset" => {
            let focus = arg("focus")
                .map(|f| format!(" Look hardest at {f}."))
                .unwrap_or_default();
            format!(
                "Using the MeshMind tools, review the current model like an art director and bring it up                  to AAA quality.{focus}

                 1. Call list_objects, check_asset, and render_preview from the front, the side and                  three_quarter under studio and outdoor lighting, plus close-ups of small parts with `focus`.
                 2. Write a short critique: silhouette (is it recognisable at a glance?), proportions against                  the real object, detail density (are there enough separate parts and trim?), edges (bevelled?),                  materials (right preset, believable colour, wear and dirt?), and anything floating,                  intersecting or grey.
                 3. Fix the three most important problems, re-applying materials to any part you reshape.
                 4. Render again and compare. Use undo if a change made things worse.
                 Finish with what you changed and what you would improve next."
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
