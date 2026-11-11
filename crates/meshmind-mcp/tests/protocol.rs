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
