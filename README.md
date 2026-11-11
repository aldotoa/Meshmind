# MeshMind

**3D modelling for the AI era.** Describe the model you want; your AI agent
builds it. MeshMind is an open-source modelling engine that AI agents drive
through [MCP](https://modelcontextprotocol.io), so you get game-ready assets
without learning a complex 3D tool, and without paying for another AI service.

> Status: early development (step 2 of 4). Not ready for production use yet.
> "MeshMind" is a working name.

## Why

- **Your agent does the clicking.** Every feature is a tool an AI can call.
- **Use the AI subscription you already have.** Works with Claude Code, Codex,
  Cursor, or any MCP client. MeshMind never calls an AI API on its own.
- **Game-ready output.** PBR materials and glTF export that open cleanly in
  Unity, Unreal, Godot, and Blender.
- **Windows, macOS, and Linux.**

## Try it

### With the desktop app (recommended)

Requires Rust and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
for your platform.

```sh
cargo run --release -p meshmind-desktop
```

The app shows the model live and serves MCP on `http://127.0.0.1:7878/mcp`.
Connect your agent once, for example in Claude Code:

```sh
claude mcp add --transport http meshmind http://127.0.0.1:7878/mcp
```

Then ask: *"Using MeshMind, build a sci-fi supply crate with bevelled edges,
recessed side panels and a worn olive paint, then export it to crate.glb."*

### Headless (no window)

```sh
cargo install --path crates/meshmind-mcp
claude mcp add meshmind -- meshmind-mcp          # stdio
meshmind-mcp --http 7878                         # or HTTP
```

The agent can still see its work through the `render_preview` tool.

## Tools your agent gets

| Group | Tools |
|---|---|
| Create | `create_primitive` (cube, plane, sphere, cylinder, cone, torus) |
| Edit | `extrude`, `inset`, `bevel`, `subdivide`, `boolean` (union, difference, intersection), `mirror`, `array`, `set_shading` |
| Scene | `list_objects`, `get_object`, `set_transform`, `duplicate_object`, `delete_object`, `clear_scene`, `undo`, `redo` |
| Look | `set_material` (PBR metallic-roughness) |
| Review | `render_preview` returns a multi-angle image the agent can look at |
| Output | `export_glb` (glTF 2.0, opens in Unity, Unreal, Godot, Blender) |

Faces can be picked by world direction (`{"facing": "+y"}`), all at once, or by index.

## Project layout

| Path | What it is |
|---|---|
| `crates/meshmind-core` | Engine: scene, polygon meshes, modelling ops, booleans, PBR materials, glTF export |
| `crates/meshmind-mcp` | MCP server exposing the engine as agent tools (stdio and HTTP) |
| `crates/meshmind-render` | CPU preview renderer used by `render_preview` |
| `apps/desktop` | Tauri desktop app: live viewer and review UI |
| `docs/ARCHITECTURE.md` | Design and roadmap |

## Development

```sh
cargo test          # engine and MCP server tests
cargo clippy --workspace --all-targets
cargo fmt --all
```

## License

MIT. See [LICENSE](LICENSE). The desktop app bundles [three.js](https://threejs.org) (MIT).
