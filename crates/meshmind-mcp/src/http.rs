//! Streamable HTTP transport (the MCP spec's HTTP transport, JSON responses only).
//!
//! Agents connect with e.g. `claude mcp add --transport http meshmind http://127.0.0.1:7878/mcp`.
//! Only binds to localhost: the server can write files, so it must not be
//! reachable from other machines.

use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Response, StatusCode};

use crate::Server;

pub const DEFAULT_PORT: u16 = 7878;

/// Serve `server` on `127.0.0.1:port` until the process exits. `on_change`
/// runs after any request that changed the scene.
pub fn serve(
    server: Arc<Mutex<Server>>,
    port: u16,
    on_change: impl Fn(u64) + Send + 'static,
) -> std::io::Result<()> {
    let http = tiny_http::Server::http(("127.0.0.1", port)).map_err(std::io::Error::other)?;
    let json_header = Header::from_bytes("Content-Type", "application/json").expect("valid header");
    for mut req in http.incoming_requests() {
        if req.url().split('?').next() != Some("/mcp") {
            let _ = req.respond(Response::empty(StatusCode(404)));
            continue;
        }
        // Reject cross-site requests from browsers (DNS rebinding protection).
        let bad_origin = req.headers().iter().any(|h| {
            h.field.equiv("Origin") && {
                let o = h.value.as_str();
                !(o.starts_with("http://127.0.0.1")
                    || o.starts_with("http://localhost")
                    || o.starts_with("tauri://"))
            }
        });
        if bad_origin {
            let _ = req.respond(Response::empty(StatusCode(403)));
            continue;
        }
        match req.method() {
            Method::Post => {
                let mut body = String::new();
                if req.as_reader().read_to_string(&mut body).is_err() {
                    let _ = req.respond(Response::empty(StatusCode(400)));
                    continue;
                }
                let (reply, changed) = {
                    let mut s = server.lock().expect("server lock poisoned");
                    let before = s.revision;
                    let reply = s.handle_line(&body);
                    (reply, (s.revision != before).then_some(s.revision))
                };
                if let Some(rev) = changed {
                    on_change(rev);
                }
                let _ = match reply {
                    Some(text) => {
                        req.respond(Response::from_string(text).with_header(json_header.clone()))
                    }
                    // Notifications get 202 Accepted with no body.
                    None => req.respond(Response::empty(StatusCode(202))),
                };
            }
            Method::Delete => {
                let _ = req.respond(Response::empty(StatusCode(200)));
            }
            // No server-initiated stream.
            _ => {
                let _ = req.respond(Response::empty(StatusCode(405)));
            }
        }
    }
    Ok(())
}
