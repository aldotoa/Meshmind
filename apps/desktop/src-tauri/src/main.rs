// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use meshmind_mcp::{http, Server};
use serde_json::{json, Value};
use tauri::{Emitter, State};

/// One MCP server shared by the agent (over HTTP) and the viewer (over Tauri
/// commands), so both always see the same scene.
struct AppState {
    server: Arc<Mutex<Server>>,
    port: u16,
    http_error: Mutex<Option<String>>,
}

/// Forward one MCP JSON-RPC message from the UI to the embedded server.
#[tauri::command]
fn mcp(state: State<AppState>, app: tauri::AppHandle, message: Value) -> Value {
    let (reply, changed) = {
        let mut s = state.server.lock().expect("server lock poisoned");
        let before = s.revision;
        let reply = s.handle(&message).unwrap_or(Value::Null);
        (reply, (s.revision != before).then_some(s.revision))
    };
    if let Some(rev) = changed {
        let _ = app.emit("scene-changed", rev);
    }
    reply
}

/// The scene as triangle meshes, ready for the WebGL viewer.
#[tauri::command]
fn scene_snapshot(state: State<AppState>) -> Value {
    let s = state.server.lock().expect("server lock poisoned");
    let objects: Vec<Value> = s
        .scene
        .objects()
        .iter()
        .map(|o| {
            let mesh = o.mesh.to_mesh();
            let material = s.scene.material(o.material).ok();
            let t = &o.transform;
            json!({
                "id": o.id,
                "name": o.name,
                "faces": o.mesh.faces.len(),
                "positions": mesh.positions.iter().flat_map(|p| p.to_array()).collect::<Vec<f32>>(),
                "normals": mesh.normals.iter().flat_map(|p| p.to_array()).collect::<Vec<f32>>(),
                "uvs": mesh.uvs.iter().flat_map(|p| p.to_array()).collect::<Vec<f32>>(),
                "material_id": o.material,
                "textured": material.is_some_and(|m| m.textures.is_some()),
                "indices": mesh.indices,
                "translation": t.translation.to_array(),
                "rotation": t.rotation.to_array(),
                "scale": t.scale.to_array(),
                "material": material,
            })
        })
        .collect();
    let lighting = s.scene.lighting;
    json!({
        "revision": s.revision,
        "objects": objects,
        "lighting": { "name": lighting.name(), "params": lighting.params() },
    })
}

/// A material's baked textures as PNG data URLs. Textures never change for a
/// given material id, so the viewer caches them by id.
#[tauri::command]
fn material_textures(state: State<AppState>, material_id: u32) -> Value {
    use base64::Engine;
    let s = state.server.lock().expect("server lock poisoned");
    let Some(tex) = s
        .scene
        .material(material_id)
        .ok()
        .and_then(|m| m.textures.clone())
    else {
        return Value::Null;
    };
    drop(s); // PNG encoding is slow; don't hold the scene lock.
    let url = |t: &meshmind_core::texture::Texture| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(t.to_png())
        )
    };
    json!({
        "base_color": url(&tex.base_color),
        "metallic_roughness": url(&tex.metallic_roughness),
        "normal": url(&tex.normal),
    })
}

#[tauri::command]
fn connection_info(state: State<AppState>) -> Value {
    let url = format!("http://127.0.0.1:{}/mcp", state.port);
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "url": url,
        "claude_command": format!("claude mcp add --transport http meshmind {url}"),
        "error": state.http_error.lock().expect("lock").clone(),
    })
}

fn main() {
    let port = std::env::var("MESHMIND_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(http::DEFAULT_PORT);
    let server = Arc::new(Mutex::new(Server::new()));

    tauri::Builder::default()
        .manage(AppState {
            server: server.clone(),
            port,
            http_error: Mutex::new(None),
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let errors = handle.clone();
            std::thread::spawn(move || {
                let notify = handle.clone();
                if let Err(e) = http::serve(server, port, move |rev| {
                    let _ = notify.emit("scene-changed", rev);
                }) {
                    use tauri::Manager;
                    let msg = format!("Could not listen on port {port}: {e}");
                    eprintln!("{msg}");
                    *errors.state::<AppState>().http_error.lock().expect("lock") = Some(msg);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            mcp,
            scene_snapshot,
            material_textures,
            connection_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running MeshMind");
}
