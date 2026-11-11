# MeshMind architecture

MeshMind is an open-source 3D modelling tool built for AI agents. A person
describes what they want; their own AI agent (Claude Code, Codex, Cursor, or
any MCP client) does the modelling by calling MeshMind's tools. The person
watches, reviews, and steers.

> "MeshMind" is a working name and can be changed.

## Design principles

1. **The agent is the user.** Every capability is an MCP tool with a clear
   name, a JSON schema, and a description written for a model to read. The
   GUI is a viewer and review surface, not a toolbox of buttons.
2. **Bring your own AI.** MeshMind never calls an LLM API itself. Users spend
   the AI subscription they already pay for, through whatever agent they use.
3. **The agent can see its work.** A `render_preview` tool returns images
   from several angles, so the agent can critique and iterate, the same loop
   a human artist uses.
4. **High-level first, low-level available.** Tools like "add bevelled edges
   with wear" sit on top of primitive mesh operations, so agents get to AAA
   quality in few calls but can still drop down when needed.
5. **Deterministic and undoable.** Every tool call is a command on an undo
   stack. Same calls in, same model out.
6. **Cross-platform from day one.** Windows, macOS, and Linux are built and
   tested on every change.

## Components

```
 ┌───────────────────────┐        MCP (stdio or local HTTP)
 │ User's AI agent       │ ──────────────────────────────┐
 │ (Claude Code, Codex…) │                               │
 └───────────────────────┘                               ▼
                                         ┌──────────────────────────────┐
                                         │ meshmind-mcp                 │
                                         │  tool registry, JSON-RPC,    │
                                         │  schemas, validation         │
                                         └──────────────┬───────────────┘
                                                        │ commands
                                         ┌──────────────▼───────────────┐
                                         │ meshmind-core                │
                                         │  scene, mesh, materials,     │
                                         │  ops, undo, import/export    │
                                         └──────────────┬───────────────┘
                                                        │ scene snapshots
                                         ┌──────────────▼───────────────┐
                                         │ apps/desktop (Tauri)         │
                                         │  WebGPU viewer, history,     │
                                         │  review + approve UI         │
                                         └──────────────────────────────┘
```

| Crate / app | Role |
|---|---|
| `crates/meshmind-core` | Pure Rust engine with no UI: scene graph, meshes, PBR materials, mesh operations, undo stack, glTF/FBX/OBJ export. Fully unit-tested. |
| `crates/meshmind-mcp` | MCP server. Exposes core operations as tools. Runs standalone (`meshmind-mcp` binary over stdio) or embedded in the desktop app. |
| `apps/desktop` | Tauri v2 app. Embeds the MCP server (HTTP on `127.0.0.1:7878`), shows the live scene with three.js (PBR, soft shadows, environment lighting), and lets the person undo and redo. |
| `crates/meshmind-render` | CPU software renderer used by `render_preview`. Needs no GPU, so it works in CI and on any machine. A GPU path for texture baking may follow in step 3. |
| `plugins/*` *(step 3)* | Optional extras, such as local image-to-3D models (TRELLIS, Hunyuan3D) for a fast rough first shape. |

## How an agent connects

- **Headless:** the agent's MCP config runs `meshmind-mcp` over stdio. Good
  for CI, batch asset generation, and agents without a GUI.
- **With the app:** the desktop app hosts the same server on a localhost
  port, and the agent connects to that. The person sees each change live.

## Tool surface (planned)

| Group | Tools |
|---|---|
| Scene | `list_objects`, `get_object`, `delete_object`, `duplicate_object`, `set_transform`, `group`, `undo`, `redo` |
| Create | `create_primitive` (cube, sphere, cylinder, plane, torus, cone), `create_from_profile` (lathe/extrude a 2D outline) |
| Edit | `extrude`, `bevel`, `inset`, `boolean`, `subdivide`, `mirror`, `array`, `deform` |
| Look | `set_material`, `apply_smart_material` (wear, edge damage, dirt), `unwrap_uvs`, `bake_textures`, `set_lighting_preset` |
| Review | `render_preview` (multi-angle images), `scene_stats` (poly count, UV coverage, problems) |
| Output | `export` (glTF/GLB, FBX, OBJ), `save_project`, `load_project` |

## Data model

- **Scene**: a flat list of objects for now (parent/child links come with
  grouping). Each object has a stable id, a name, a transform
  (translation, rotation quaternion, scale), a mesh, and a material id.
- **PolyMesh** (source of truth): welded vertices and n-gon faces, each
  flagged smooth or flat. All modelling ops work on it and return a new
  mesh, which makes undo simple (scene snapshots).
- **Mesh**: triangles with normals and UVs, derived from the PolyMesh for
  rendering and export. UVs are a box projection until step 3's unwrapping.
- **Material**: metallic-roughness PBR, matching glTF 2.0, so exports open
  correctly in Unity, Unreal, Godot, and Blender.

## Known limitations (step 2)

- `bevel` rounds hard edges with 1 to 16 segments and stays watertight where
  hard and soft edges meet. Concave edges get the same arc profile, which is a
  close approximation rather than a true fillet.
- `boolean` (BSP, after csg.js) needs closed inputs and can produce thin
  slivers; results are welded but not retopologised.
- The software preview has no shadows or reflections; the desktop viewer does.

## Roadmap

1. **Architecture and skeleton** (done): workspace, core types,
   primitives, a working MCP server with basic scene tools, Tauri shell, CI.
2. **Core engine and tools** (done): mesh editing ops, booleans, undo stack,
   glTF export, headless preview rendering, live viewer, HTTP transport.
3. **AAA quality layer**: smart materials, UV unwrapping, baking, lighting,
   optional image-to-3D plugins.
4. **Polish and launch**: docs, example prompts, signed releases, demo.
