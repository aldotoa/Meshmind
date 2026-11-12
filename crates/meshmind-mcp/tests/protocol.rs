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

#[test]
fn smart_material_lighting_and_textured_export() {
    let mut s = Server::new();
    let id = create(&mut s, json!({ "shape": "cube" }));
    call(&mut s, "bevel", json!({ "id": id, "amount": 0.03 }));
    let r = call(
        &mut s,
        "apply_smart_material",
        json!({ "id": id, "preset": "painted_metal", "color": [0.1, 0.2, 0.05], "wear": 0.6, "resolution": 256 }),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["unwrapped"], true);
    let obj = s.scene.object(id as u32).unwrap();
    assert!(obj.mesh.has_uvs());
    assert!(s.scene.material(obj.material).unwrap().textures.is_some());

    let r = call(&mut s, "set_lighting", json!({ "preset": "sunset" }));
    assert_eq!(r["isError"], false, "{r}");
    let r = call(
        &mut s,
        "render_preview",
        json!({ "size": 128, "views": ["three_quarter"] }),
    );
    assert_eq!(r["isError"], false, "{r}");

    let path = std::env::temp_dir().join(format!("meshmind-tex-{}.glb", std::process::id()));
    call(
        &mut s,
        "export_glb",
        json!({ "path": path.to_str().unwrap() }),
    );
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        bytes.len() > 50_000,
        "textures should be embedded ({} bytes)",
        bytes.len()
    );

    // Editing the shape afterwards clears UVs and warns that textures are stale.
    let r = call(
        &mut s,
        "extrude",
        json!({ "id": id, "faces": { "facing": "+y" }, "distance": 0.1 }),
    );
    assert!(r["structuredContent"]["note"]
        .as_str()
        .unwrap()
        .contains("apply_smart_material"));
    assert!(!s.scene.object(id as u32).unwrap().mesh.has_uvs());

    let r = call(
        &mut s,
        "apply_smart_material",
        json!({ "id": id, "preset": "chocolate" }),
    );
    assert_eq!(r["isError"], true);
}

#[test]
fn inset_returns_inner_faces_for_a_real_recess() {
    let mut s = Server::new();
    let id = create(&mut s, json!({ "shape": "cube" }));
    let r = call(
        &mut s,
        "inset",
        json!({ "id": id, "faces": { "facing": "+z" }, "amount": 0.2 }),
    );
    let inner = r["structuredContent"]["inner_faces"].clone();
    assert_eq!(inner.as_array().unwrap().len(), 1);
    call(
        &mut s,
        "extrude",
        json!({ "id": id, "faces": inner, "distance": -0.1 }),
    );
    let mesh = &s.scene.object(id as u32).unwrap().mesh;
    // Only the inner 0.8 x 0.8 panel went in 0.1, so the volume drops by 0.064.
    assert!(
        (mesh.volume() - (1.0 - 0.064)).abs() < 1e-4,
        "{}",
        mesh.volume()
    );
}

#[test]
fn save_and_open_project() {
    let dir = std::env::temp_dir().join(format!("meshmind-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crate.meshmind");
    let path = path.to_str().unwrap();

    let mut s = Server::new();
    let id = call(
        &mut s,
        "create_primitive",
        json!({ "shape": "cube", "name": "Crate" }),
    )["structuredContent"]["id"]
        .as_u64()
        .unwrap();
    let r = call(
        &mut s,
        "apply_smart_material",
        json!({ "id": id, "preset": "wood", "resolution": 256 }),
    );
    assert_eq!(r["isError"], false, "{r}");
    let r = call(&mut s, "save_project", json!({ "path": path }));
    assert_eq!(r["isError"], false, "{r}");

    let mut fresh = Server::new();
    let r = call(&mut fresh, "open_project", json!({ "path": path }));
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["structuredContent"]["objects"][0]["name"], "Crate");
    assert_eq!(fresh.scene.objects(), s.scene.objects());
    let textured = fresh.scene.materials().any(|(_, m)| m.textures.is_some());
    assert!(textured, "baked textures are kept");
    // Opening is undoable.
    call(&mut fresh, "undo", json!({}));
    assert!(fresh.scene.objects().is_empty());

    let r = call(&mut s, "save_project", json!({ "path": "model.txt" }));
    assert_eq!(r["isError"], true);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn prompts_describe_the_workflow() {
    let mut s = Server::new();
    let init = rpc(&mut s, 1, "initialize", json!({}));
    assert!(init["result"]["capabilities"]["prompts"].is_object());
    let list = rpc(&mut s, 2, "prompts/list", json!({}));
    assert_eq!(list["result"]["prompts"][0]["name"], "model_asset");
    let got = rpc(
        &mut s,
        3,
        "prompts/get",
        json!({ "name": "model_asset", "arguments": { "description": "a wooden stool" } }),
    );
    let text = got["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("a wooden stool") && text.contains("export_glb"));
    let missing = rpc(&mut s, 4, "prompts/get", json!({ "name": "model_asset" }));
    assert_eq!(missing["error"]["code"], -32602);
}

#[test]
fn tool_reference_doc_is_up_to_date() {
    let committed = include_str!("../../../docs/TOOLS.md").replace("\r\n", "\n");
    assert!(
        committed == meshmind_mcp::tool_reference_markdown(),
        "docs/TOOLS.md is stale: run `cargo run -p meshmind-mcp --example tool_docs > docs/TOOLS.md`"
    );
}

#[test]
fn scaled_objects_are_edited_in_metres() {
    let mut s = Server::new();
    let id = call(&mut s, "create_primitive", json!({ "shape": "cube" }))["structuredContent"]
        ["id"]
        .as_u64()
        .unwrap();
    call(
        &mut s,
        "set_transform",
        json!({ "id": id, "translation": [0, 0.1, 0], "scale": [2, 0.2, 1] }),
    );
    let before = call(&mut s, "list_objects", json!({}))["structuredContent"]["objects"][0].clone();
    let r = call(
        &mut s,
        "bevel",
        json!({ "id": id, "amount": 0.02, "segments": 1 }),
    );
    assert_eq!(r["structuredContent"]["scale_applied"], true, "{r}");
    let after = call(&mut s, "list_objects", json!({}))["structuredContent"]["objects"][0].clone();
    // Same place and size in the world, now with scale 1.
    assert_eq!(before["size"], after["size"]);
    let o = &s.scene.objects()[0];
    assert_eq!(o.transform.scale, glam::Vec3::ONE);
    let b = o.mesh.bounds().unwrap();
    assert!((b.max.y - b.min.y - 0.2).abs() < 1e-5);
}

#[test]
fn hex_colours_are_srgb() {
    let mut s = Server::new();
    let id = call(&mut s, "create_primitive", json!({ "shape": "cube" }))["structuredContent"]
        ["id"]
        .as_u64()
        .unwrap();
    let r = call(
        &mut s,
        "set_material",
        json!({ "id": id, "base_color_hex": "#808080" }),
    );
    assert_eq!(r["isError"], false, "{r}");
    let (_, m) = s.scene.materials().last().unwrap();
    assert!(
        (m.base_color[0] - 0.2159).abs() < 1e-3,
        "{:?}",
        m.base_color
    );
    let r = call(
        &mut s,
        "set_material",
        json!({ "id": id, "base_color_hex": "grey" }),
    );
    assert_eq!(r["isError"], true);
}

#[test]
fn shape_tools_build_a_sword() {
    let mut s = Server::new();
    let diamond = json!([[1, 0], [0, 1], [-1, 0], [0, -1]]);
    let blade = call(
        &mut s,
        "loft",
        json!({ "name": "Blade", "shape": diamond, "stations": [
            { "y": 0, "scale": [0.03, 0.008] },
            { "y": 0.6, "scale": [0.02, 0.006] },
            { "y": 0.7, "scale": [0, 0] } ] }),
    );
    assert_eq!(blade["isError"], false, "{blade}");
    let size = &blade["structuredContent"]["size"];
    assert!((size[1].as_f64().unwrap() - 0.7).abs() < 1e-4);
    assert!((size[0].as_f64().unwrap() - 0.06).abs() < 1e-4);

    let pommel = call(
        &mut s,
        "lathe",
        json!({ "profile": [[0, 0], [0.03, 0.01], [0.03, 0.03], [0, 0.04]], "segments": 16 }),
    );
    assert_eq!(pommel["isError"], false, "{pommel}");
    let guard = call(
        &mut s,
        "extrude_outline",
        json!({ "outline": [[-0.1, 0], [0.1, 0], [0.1, 0.02], [-0.1, 0.02]], "depth": 0.02 }),
    );
    assert_eq!(guard["isError"], false, "{guard}");
    let wrap = call(
        &mut s,
        "sweep",
        json!({ "helix": { "radius": 0.015, "height": 0.12, "turns": 8 }, "radius": 0.003 }),
    );
    assert_eq!(wrap["isError"], false, "{wrap}");
    let ring = call(
        &mut s,
        "sweep",
        json!({ "path": [[0.1, 0, 0], [0, 0, 0.1], [-0.1, 0, 0], [0, 0, -0.1], [0.1, 0, 0]], "radius": 0.01 }),
    );
    assert_eq!(ring["isError"], false, "{ring}");

    let bad = call(
        &mut s,
        "loft",
        json!({ "shape": diamond, "stations": [{ "y": 0 }] }),
    );
    assert_eq!(bad["isError"], true);
    let bad = call(&mut s, "sweep", json!({ "radius": 0.01 }));
    assert_eq!(bad["isError"], true);

    // Close-up render of the small parts.
    let id = |v: &Value| v["structuredContent"]["id"].clone();
    let r = call(
        &mut s,
        "render_preview",
        json!({ "views": ["front"], "size": 128, "focus": [id(&pommel), id(&wrap)] }),
    );
    assert_eq!(r["isError"], false, "{r}");
    let r = call(&mut s, "render_preview", json!({ "focus": [999] }));
    assert_eq!(r["isError"], true);
}

#[test]
fn deform_slice_and_move_vertices() {
    let mut s = Server::new();
    let bar = call(
        &mut s,
        "create_primitive",
        json!({ "shape": "cube", "size": 0.1 }),
    )["structuredContent"]["id"]
        .clone();
    call(
        &mut s,
        "set_transform",
        json!({ "id": bar, "scale": [1, 10, 1] }),
    );
    let bent = call(
        &mut s,
        "deform",
        json!({ "id": bar, "mode": "bend", "amount": 90, "toward": "x" }),
    );
    assert_eq!(bent["isError"], false, "{bent}");
    let obj = call(&mut s, "get_object", json!({ "id": bar }))["structuredContent"].clone();
    // A 90 degree bend moves the top of the bar off to the side.
    assert!(obj["bounds_max"][0].as_f64().unwrap() > 0.3, "{obj}");

    let sliced = call(
        &mut s,
        "slice",
        json!({ "id": bar, "axis": "y", "cuts": 4 }),
    );
    assert_eq!(sliced["isError"], false, "{sliced}");
    let moved = call(
        &mut s,
        "move_vertices",
        json!({ "id": bar, "min": [-1, -1, -1], "max": [1, 0, 1], "translate": [0, -0.1, 0] }),
    );
    assert_eq!(moved["isError"], false, "{moved}");
    assert!(
        moved["structuredContent"]["moved_vertices"]
            .as_u64()
            .unwrap()
            > 0
    );
    let none = call(
        &mut s,
        "move_vertices",
        json!({ "id": bar, "min": [5, 5, 5], "max": [6, 6, 6], "translate": [0, 1, 0] }),
    );
    assert_eq!(none["isError"], true);
    let bad = call(
        &mut s,
        "deform",
        json!({ "id": bar, "mode": "melt", "amount": 1 }),
    );
    assert_eq!(bad["isError"], true);
}
