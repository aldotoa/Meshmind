# MeshMind

**3D modelling for the AI era.** Describe the model you want; your AI agent
builds it. MeshMind is an open-source modelling engine that AI agents drive
through [MCP](https://modelcontextprotocol.io), so you get game-ready assets
without learning a complex 3D tool, and without paying for another AI service.

> Status: early development (step 1 of 4). Not ready for production use yet.
> "MeshMind" is a working name.

## Why

- **Your agent does the clicking.** Every feature is a tool an AI can call.
- **Use the AI subscription you already have.** Works with Claude Code, Codex,
  Cursor, or any MCP client. MeshMind never calls an AI API on its own.
- **Game-ready output.** PBR materials and glTF export that open cleanly in
  Unity, Unreal, Godot, and Blender.
- **Windows, macOS, and Linux.**

## Try it

```sh
cargo install --path crates/meshmind-mcp
```

Then add it to your agent's MCP config, for example in Claude Code:

```sh
claude mcp add meshmind -- meshmind-mcp
```

Ask your agent: *"Using MeshMind, build a simple wooden table."*

### Desktop app

Requires Rust and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
for your platform.

```sh
cargo run -p meshmind-desktop
```

## Project layout

| Path | What it is |
|---|---|
| `crates/meshmind-core` | Engine: scene, meshes, PBR materials, primitives |
| `crates/meshmind-mcp` | MCP server exposing the engine as agent tools |
| `apps/desktop` | Tauri desktop app: live viewer and review UI |
| `docs/ARCHITECTURE.md` | Design and roadmap |

## Development

```sh
cargo test          # engine and MCP server tests
cargo clippy --workspace --all-targets
cargo fmt --all
```

## License

MIT. See [LICENSE](LICENSE).
