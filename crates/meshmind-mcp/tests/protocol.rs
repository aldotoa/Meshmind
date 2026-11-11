use meshmind_mcp::Server;
use serde_json::{json, Value};

fn rpc(server: &mut Server, id: u64, method: &str, params: Value) -> Value {
    server
        .handle(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
        .unwrap()
}

fn call(server: &mut Server, name: &str, args: Value) -> Value {
    rpc(
        server,
        1,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    )["result"]
        .clone()
}

#[test]
fn initialize_and_list_tools() {
    let mut s = Server::new();
    let init = rpc(&mut s, 1, "initialize", json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "meshmind");
    assert!(s
        .handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .is_none());
    let tools = rpc(&mut s, 2, "tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"create_primitive"));
}

#[test]
fn build_a_simple_model() {
    let mut s = Server::new();
    let table = call(
        &mut s,
        "create_primitive",
        json!({ "shape": "cube", "name": "Tabletop" }),
    );
    let id = table["structuredContent"]["id"].as_u64().unwrap();
    let r = call(
        &mut s,
        "set_transform",
        json!({ "id": id, "translation": [0, 0.75, 0], "scale": [1.2, 0.05, 0.8] }),
    );
    assert_eq!(r["isError"], false);
    let r = call(
        &mut s,
        "set_material",
        json!({ "id": id, "base_color": [0.4, 0.25, 0.1], "roughness": 0.6 }),
    );
    assert_eq!(r["isError"], false);

    let list = call(&mut s, "list_objects", json!({}));
    let obj = &list["structuredContent"]["objects"][0];
    assert_eq!(obj["name"], "Tabletop");
    assert_eq!(obj["translation"][1].as_f64().unwrap() as f32, 0.75);
    assert_ne!(obj["material"], 0);
}

#[test]
fn bad_arguments_are_reported_to_the_agent() {
    let mut s = Server::new();
    let r = call(&mut s, "create_primitive", json!({ "shape": "teapot" }));
    assert_eq!(r["isError"], true);
    let r = call(
        &mut s,
        "create_primitive",
        json!({ "shape": "cube", "size": -1 }),
    );
    assert_eq!(r["isError"], true);
    let r = call(&mut s, "delete_object", json!({ "id": 42 }));
    assert_eq!(r["isError"], true);
}

#[test]
fn unknown_method_and_bad_json() {
    let mut s = Server::new();
    assert_eq!(rpc(&mut s, 1, "nope", json!({}))["error"]["code"], -32601);
    let reply: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(reply["error"]["code"], -32700);
}

fn create(server: &mut Server, args: Value) -> u64 {
    let r = call(server, "create_primitive", args);
    assert_eq!(r["isError"], false, "{r}");
    r["structuredContent"]["id"].as_u64().unwrap()
}

#[test]
fn modelling_tools_and_undo() {
    let mut s = Server::new();
    let id = create(&mut s, json!({ "shape": "cube" }));
    let r = call(
        &mut s,
        "inset",
        json!({ "id": id, "faces": { "facing": "+y" }, "amount": 0.2 }),
    );
    assert_eq!(r["isError"], false, "{r}");
    let r = call(
        &mut s,
        "extrude",
        json!({ "id": id, "faces": [2], "distance": 0.5 }),
    );
    assert_eq!(r["isError"], false, "{r}");
    let r = call(&mut s, "bevel", json!({ "id": id, "amount": 0.02 }));
    assert_eq!(r["isError"], false, "{r}");
    let faces_after = r["structuredContent"]["faces"].as_u64().unwrap();
    assert!(faces_after > 6);

    // Undo the bevel, then redo it.
    call(&mut s, "undo", json!({}));
    let obj = call(&mut s, "get_object", json!({ "id": id }));
    assert!(obj["structuredContent"]["faces"].as_u64().unwrap() < faces_after);
    call(&mut s, "redo", json!({}));
    let obj = call(&mut s, "get_object", json!({ "id": id }));
    assert_eq!(
        obj["structuredContent"]["faces"].as_u64().unwrap(),
        faces_after
    );
    assert_eq!(obj["structuredContent"]["closed"], true);
}

#[test]
fn failed_tool_leaves_scene_and_history_untouched() {
    let mut s = Server::new();
    let id = create(&mut s, json!({ "shape": "cube" }));
    let rev = s.revision;
    let r = call(
        &mut s,
        "extrude",
        json!({ "id": id, "faces": { "facing": [0, 0, 0] }, "distance": 1 }),
    );
    assert_eq!(r["isError"], true);
    assert_eq!(s.revision, rev);
    call(&mut s, "undo", json!({}));
    assert!(
        s.scene.objects().is_empty(),
        "undo should remove the cube, not a failed edit"
    );
}

#[test]
fn facing_selection_uses_world_directions() {
    let mut s = Server::new();
    let id = create(&mut s, json!({ "shape": "cube" }));
    // Turn the cube so its local +X face points up in the world.
    call(
        &mut s,
        "set_transform",
        json!({ "id": id, "rotation_degrees": [0, 0, 90] }),
    );
    call(
        &mut s,
        "extrude",
        json!({ "id": id, "faces": { "facing": "+y", "max_angle": 5 }, "distance": 1 }),
    );
    let size =
        call(&mut s, "list_objects", json!({}))["structuredContent"]["objects"][0]["size"].clone();
    assert!(
        (size[1].as_f64().unwrap() - 2.0).abs() < 1e-4,
        "world height should be 2, got {size}"
    );
}

#[test]
fn boolean_cuts_a_hole() {
    let mut s = Server::new();
    let block = create(&mut s, json!({ "shape": "cube", "size": 2 }));
    let drill = create(
        &mut s,
        json!({ "shape": "cylinder", "radius": 0.4, "height": 3 }),
    );
    let r = call(
        &mut s,
        "boolean",
        json!({ "target": block, "tool": drill, "operation": "difference" }),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(s.scene.objects().len(), 1);
    assert!(s.scene.object(block as u32).unwrap().mesh.volume() < 7.5);
}

#[test]
fn render_preview_returns_png() {
    let mut s = Server::new();
    create(&mut s, json!({ "shape": "torus" }));
    let r = call(&mut s, "render_preview", json!({ "size": 128 }));
    assert_eq!(r["isError"], false, "{r}");
    let image = &r["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert!(image["data"].as_str().unwrap().starts_with("iVBOR"));
}

#[test]
fn export_glb_writes_a_file() {
    let mut s = Server::new();
    create(&mut s, json!({ "shape": "sphere" }));
    let path = std::env::temp_dir().join(format!("meshmind-test-{}.glb", std::process::id()));
    let r = call(
        &mut s,
        "export_glb",
        json!({ "path": path.to_str().unwrap() }),
    );
    assert_eq!(r["isError"], false, "{r}");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[0..4], b"glTF");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn http_transport() {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    let server = Arc::new(Mutex::new(Server::new()));
    // Find a free port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let seen = changes.clone();
    let srv = server.clone();
    std::thread::spawn(move || {
        meshmind_mcp::http::serve(srv, port, move |rev| seen.lock().unwrap().push(rev))
    });

    let post = |body: &str| -> String {
        for _ in 0..50 {
            if let Ok(mut c) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                write!(c, "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                let mut out = String::new();
                c.read_to_string(&mut out).unwrap();
                return out;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("server did not start");
    };
    let reply = post(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_primitive","arguments":{"shape":"cube"}}}"#,
    );
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("\"isError\":false"));
    let reply = post(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    assert!(reply.starts_with("HTTP/1.1 202"), "{reply}");
    assert_eq!(*changes.lock().unwrap(), vec![1]);
    assert_eq!(server.lock().unwrap().scene.objects().len(), 1);
}
