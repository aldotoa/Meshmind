//! Standalone MCP server over stdio. Add it to your agent's MCP config:
//!
//! ```json
//! { "mcpServers": { "meshmind": { "command": "meshmind-mcp" } } }
//! ```

use std::io::{self, BufRead, Write};

use meshmind_mcp::Server;

fn main() -> io::Result<()> {
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
