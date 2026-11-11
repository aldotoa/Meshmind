// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use meshmind_mcp::Server;
use serde_json::{json, Value};
use tauri::State;

/// The app embeds the same MCP server agents talk to, so the viewer and the
/// agent always see one scene. Exposing it on a local port comes in step 2.
struct AppState(Mutex<Server>);

/// Forward one MCP JSON-RPC message from the UI to the embedded server.
#[tauri::command]
fn mcp(state: State<AppState>, message: Value) -> Value {
    let mut server = state.0.lock().expect("server lock poisoned");
    server.handle(&message).unwrap_or(Value::Null)
}

#[tauri::command]
fn engine_info() -> Value {
    json!({ "name": "MeshMind", "version": env!("CARGO_PKG_VERSION") })
}

fn main() {
    tauri::Builder::default()
        .manage(AppState(Mutex::new(Server::new())))
        .invoke_handler(tauri::generate_handler![mcp, engine_info])
        .run(tauri::generate_context!())
        .expect("error while running MeshMind");
}
