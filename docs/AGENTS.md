# Connecting your AI agent

MeshMind does not call any AI service itself. Your own agent (Claude Code,
Codex, Cursor, and others) drives it through the
[Model Context Protocol](https://modelcontextprotocol.io), so modelling runs on
the AI subscription you already pay for.

There are two ways to run MeshMind:

| | Desktop app | Headless `meshmind-mcp` |
|---|---|---|
| You see the model live | Yes | No (the agent still sees previews) |
| How the agent connects | HTTP: `http://127.0.0.1:7878/mcp` | stdio (the agent starts it) or HTTP |
| Work is saved automatically | Yes | Ask the agent to `save_project` |

Start the desktop app first when you use HTTP. The address is shown at the top
of the window, with a button that copies the Claude Code command. Change the
port with the `MESHMIND_PORT` environment variable.

## Claude Code

```sh
# Desktop app (live view)
claude mcp add --transport http meshmind http://127.0.0.1:7878/mcp

# Headless
claude mcp add meshmind -- meshmind-mcp
```

## Claude Desktop

Edit `claude_desktop_config.json` (Settings, Developer, Edit Config):

```json
{
  "mcpServers": {
    "meshmind": { "command": "/full/path/to/meshmind-mcp" }
  }
}
```

To use the desktop app's live view instead, bridge to its HTTP address with
[`mcp-remote`](https://www.npmjs.com/package/mcp-remote) (needs Node.js):

```json
{
  "mcpServers": {
    "meshmind": { "command": "npx", "args": ["-y", "mcp-remote", "http://127.0.0.1:7878/mcp"] }
  }
}
```

## Codex CLI

```sh
codex mcp add meshmind -- meshmind-mcp
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.meshmind]
command = "meshmind-mcp"
```

## Cursor

In `~/.cursor/mcp.json` (all projects) or `.cursor/mcp.json` (one project):

```json
{
  "mcpServers": {
    "meshmind": { "url": "http://127.0.0.1:7878/mcp" }
  }
}
```

## VS Code (Copilot agent mode)

In `.vscode/mcp.json`:

```json
{
  "servers": {
    "meshmind": { "type": "http", "url": "http://127.0.0.1:7878/mcp" }
  }
}
```

## Gemini CLI

In `~/.gemini/settings.json`:

```json
{
  "mcpServers": {
    "meshmind": { "httpUrl": "http://127.0.0.1:7878/mcp" }
  }
}
```

## Any other MCP client

- stdio: run `meshmind-mcp`.
- Streamable HTTP: `http://127.0.0.1:7878/mcp` (desktop app, or
  `meshmind-mcp --http 7878`).

## Tips

- Agents that can read images get much better results, because
  `render_preview` lets them see and fix their own work.
- MeshMind offers two prompt templates, `model_asset` and `refine_asset`.
  In Claude Code they appear as `/mcp__meshmind__model_asset`.
- See [EXAMPLES.md](EXAMPLES.md) for prompts that work well, and
  [TOOLS.md](TOOLS.md) for every tool the agent gets.

## Troubleshooting

- **"Could not listen on port 7878"**: another program (or a second MeshMind)
  uses the port. Close it, or start MeshMind with `MESHMIND_PORT=7879` and use
  that port in the agent's config.
- **The agent doesn't see the tools**: restart the agent after adding the
  server, and check that the desktop app is running when you use HTTP.
- **Nothing appears in the window**: with stdio, the agent runs its own
  headless MeshMind. Use the HTTP address to see its work live.
