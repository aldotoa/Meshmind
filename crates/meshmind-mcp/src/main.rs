//! Standalone MCP server.
//!
//! - stdio (default): `{ "mcpServers": { "meshmind": { "command": "meshmind-mcp" } } }`
//! - HTTP: `meshmind-mcp --http [port]`, then connect to `http://127.0.0.1:<port>/mcp`

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use meshmind_mcp::{http, Server};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("meshmind-mcp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "meshmind-mcp {}: MCP server that lets AI agents build 3D models\n\n\
                 USAGE:\n  meshmind-mcp                 serve MCP over stdio (for agents that start it)\n  \
                 meshmind-mcp --http [PORT]   serve MCP on http://127.0.0.1:PORT/mcp (default {})\n\n\
                 Docs: https://github.com/aldotoa/meshmind",
                env!("CARGO_PKG_VERSION"),
                http::DEFAULT_PORT
            );
            return Ok(());
        }
        Some("--http") | None => {}
        Some(other) => {
            eprintln!("unknown option {other:?}; try --help");
            std::process::exit(2);
        }
    }
    if args.first().map(String::as_str) == Some("--http") {
        let port = args
            .get(1)
            .and_then(|p| p.parse().ok())
            .unwrap_or(http::DEFAULT_PORT);
        eprintln!("MeshMind MCP server on http://127.0.0.1:{port}/mcp");
        return http::serve(Arc::new(Mutex::new(Server::new())), port, |_| {});
    }

    let mut server = Server::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
