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

## Getting AAA results

MeshMind tells every agent how to work the moment it connects: plan the
parts and real-world size, build each part with the shape tool that fits its
silhouette (loft, lathe, extrude_outline, sweep), add trim, bevel, apply
smart materials last, then review its own renders and fix what it sees. You
don't need to explain any of that, but you get better models when your
prompt gives:

- **Size and parts**: "a one-handed longsword about 1 m long: lens-shaped
  blade with a fuller, curved crossguard, leather-wrapped grip, wheel pommel".
- **Materials and age**: "polished steel blade, brass guard, dark brown
  leather, lightly used".
- **Style**: realistic, stylized (hand-painted fantasy) or low-poly.

For characters and creatures, the agent starts from `create_character`
(human, orc, goblin, dwarf or troll) or `sculpt_body`, which give one smooth
body instead of a pile of primitives, then shapes it with sculpt brushes and
adds gear at the returned landmarks. Say what the character is, its build and
what it wears: "a hulking orc warrior, hunched, heavy underbite with tusks,
leather belt and loincloth, one iron shoulder pad".

If you have concept art, a sketch or a photo, give the agent the file path:
it loads it with `set_reference` and calls `compare_to_reference` after each
round, which overlays the model's outline on your image and lists where the
proportions are off. At the end it calls `make_game_ready` on sculpted
objects, which cuts them to a game triangle budget and bakes the detail into
normal and occlusion maps, with LODs. Say the budget if you have one: "a
hero character, under 25k triangles".

Three prompt templates package this up. In Claude Code they appear as
`/mcp__meshmind__model_asset` and so on:

| Prompt | Use it to |
|---|---|
| `model_asset` | Build a new asset from a description, with an optional `style` (realistic, stylized, low_poly) and output file. |
| `refine_asset` | Make a specific change to the current model. |
| `review_asset` | Have the agent critique the model like an art director and fix the three biggest problems. Run it once or twice at the end. |

The agent also has `check_asset`, which lists problems a reviewer would
spot (grey parts, stale materials, parts floating free, the model not on
the ground, default names, triangle count against game budgets), and
`render_preview` with `focus` (object ids) or `focus_point` and
`focus_radius` (any spot, such as a character's face) for close-ups.

## Tips

- Agents that can read images get much better results, because
  `render_preview` lets them see and fix their own work.
- If a result looks basic, ask "review it like an art director and fix
  what falls short" (or run `review_asset`) before asking for specific fixes.
- See [RECIPES.md](RECIPES.md) for worked examples (a longsword, an axe, a
  barrel, a potion bottle and an orc warrior), [EXAMPLES.md](EXAMPLES.md) for prompts that
  work well, and
  [TOOLS.md](TOOLS.md) for every tool the agent gets.

## Troubleshooting

- **"Could not listen on port 7878"**: another program (or a second MeshMind)
  uses the port. Close it, or start MeshMind with `MESHMIND_PORT=7879` and use
  that port in the agent's config.
- **The agent doesn't see the tools**: restart the agent after adding the
  server, and check that the desktop app is running when you use HTTP.
- **Nothing appears in the window**: with stdio, the agent runs its own
  headless MeshMind. Use the HTTP address to see its work live.
