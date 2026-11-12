# Contributing to MeshMind

Thanks for helping! Bug reports, ideas, docs fixes and code are all welcome.

## Ground rules

- MeshMind is driven by AI agents, so **every feature is a tool**. A good tool
  has a clear description, validates its arguments, returns a helpful error
  the agent can act on, and can be undone.
- MeshMind never calls an AI API itself. Users bring their own agent.
- Keep the engine (`meshmind-core`) free of UI and I/O so it stays testable.

## Getting started

You need Rust (see `rust-toolchain.toml`), CMake and a C++ compiler (for
Manifold), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
to build the desktop app.

```sh
cargo test                                   # engine and MCP server
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run --release -p meshmind-desktop      # the app
cargo run -p meshmind-render --example smart # renders all material presets to smart.png
```

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit.

## Adding or changing a tool

1. Implement the operation in `meshmind-core` with unit tests.
2. Expose it in `crates/meshmind-mcp/src/tools.rs`: definition, argument
   parsing and the call. Mark read-only tools in `is_mutating`.
3. Add a protocol test in `crates/meshmind-mcp/tests/protocol.rs`.
4. Regenerate the reference:
   `cargo run -p meshmind-mcp --example tool_docs > docs/TOOLS.md`
5. Try it with a real agent and look at `render_preview` output.

## Pull requests

- One topic per pull request, with tests.
- CI must pass on Windows, macOS and Linux.
- Add a line to `CHANGELOG.md` under "Unreleased".

By contributing you agree that your work is released under the MIT license.
Please follow our [Code of Conduct](CODE_OF_CONDUCT.md).
