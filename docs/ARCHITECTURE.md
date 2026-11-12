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
| `crates/meshmind-render` | CPU software renderer used by `render_preview`, with textures, normal maps, environment reflections and the lighting presets. Needs no GPU, so it works in CI and on any machine. |
| `plugins/*` *(step 3)* | Optional extras, such as local image-to-3D models (TRELLIS, Hunyuan3D) for a fast rough first shape. |

## How an agent connects

- **Headless:** the agent's MCP config runs `meshmind-mcp` over stdio. Good
  for CI, batch asset generation, and agents without a GUI.
- **With the app:** the desktop app hosts the same server on a localhost
  port, and the agent connects to that. The person sees each change live.

## Tool surface

The tools that exist today are listed in [TOOLS.md](TOOLS.md), which is
generated from the definitions agents see. Planned additions: `group`,
`scene_stats` (poly count, UV coverage, problems), and FBX/OBJ export.

Project files (`.meshmind`) are JSON holding the editable scene, with baked
textures as base64 PNG. The desktop app autosaves to its data folder shortly
after every change and restores the last session at startup.

## Data model

- **Scene**: a flat list of objects for now (parent/child links come with
  grouping). Each object has a stable id, a name, a transform
  (translation, rotation quaternion, scale), a mesh, and a material id.
- **PolyMesh** (source of truth): welded vertices and n-gon faces, each
  flagged smooth or flat. All modelling ops work on it and return a new
  mesh, which makes undo simple (scene snapshots).
- **Mesh**: triangles with normals and UVs, derived from the PolyMesh for
  rendering and export. Faces carry unwrapped UVs when they have them, and a
  box projection otherwise.
- **Material**: metallic-roughness PBR, matching glTF 2.0, so exports open
  correctly in Unity, Unreal, Godot, and Blender.

## How the AAA look is produced (step 3)

1. **Booleans** go through Manifold, so results are always watertight. Each
   input face's index rides along as a vertex property, and coplanar
   triangles from the result are merged back into clean polygons.
2. **UV unwrapping** splits the surface into charts at hard edges, flattens
   each onto its plane, and shelf-packs them at a uniform texel density.
3. **Smart materials** are baked per texel from the shape: distance to
   convex feature edges drives chipped edge wear, distance to concave edges
   drives grime, upward faces collect dust, and 3D noise adds variation,
   grain and rust. Output is glTF's base colour, metallic-roughness and an
   OpenGL-convention normal map. Bevel strips count as edges, so bevel
   before applying materials.
4. **Lighting presets** are defined once in core and used by both the
   preview renderer and the desktop viewer.
5. **Shape tools** (`shapes.rs`, `deform.rs`) build parts the way artists
   do: lofting a cross-section through stations, lathing a profile,
   extruding an outline, sweeping a tube along a path (parallel-transport
   frames, so helices don't twist). `deform` slices the mesh into edge loops
   first so bends and twists stay smooth.
6. **Auto-smooth.** Smooth faces only share a normal across edges under 50°,
   so a lofted blade has smooth faces and crisp cutting edges without the
   agent having to manage shading. Concave n-gons are ear-clipped.
7. **The preview renderer** lights scenes like a product photo: soft boxes
   and a bright horizon band in the reflections (what makes polished metal
   read as metal), a shadow-mapped key light, and an invisible floor that
   catches the shadow plus a contact shadow.
8. **Organic modelling** (`sdf.rs`, `sculpt.rs`, `character.rs`). Bodies are
   signed distance fields: ellipsoids, tapered capsules and rounded boxes
   combined in order with a smooth minimum (or smooth subtraction), so parts
   flow into each other. Surface Nets turns the field into one quad mesh
   (ambiguous grid faces are resolved first, so it stays watertight), then
   vertices are snapped onto the true surface and relaxed. Grids are
   centred on the body so mirrored parts sample identically, parts too far
   away to matter are skipped per sample, and sampling runs on all cores.
   The part list is stored on the object (`Object::body`) so a body can be
   edited by part name and rebuilt. Sculpt brushes move vertices by a
   smooth falloff around a stroke polyline, optionally mirrored in the
   object's x. `create_character` writes a part list from species
   proportions measured in head heights, and finds skin landmarks by
   marching rays through the field.

## Known limitations

- `bevel` arcs on concave edges approximate a true fillet.
- UV charts are planar projections (up to 45° from the chart's normal), so
  curved parts get more seams than a hand unwrap would.
- Any geometry edit clears UVs; the agent must re-apply smart materials
  once the shape is final (the tool result says so).
- Glass in the preview is approximate: one refraction offset per surface,
  no caustics or total internal reflection.
- Image-to-3D plugins are not built yet.
- Sculpted bodies are unrigged, and rebuilding a body from its parts
  discards sculpt strokes.
- `make_game_ready` re-unwraps the reduced mesh with the planar unwrapper,
  so organic shapes get many small UV islands.
- `compare_to_reference` separates the subject from the background by
  colour, so busy backgrounds and cast shadows in the reference skew the
  width measurements.

## Roadmap

1. **Architecture and skeleton** (done): workspace, core types,
   primitives, a working MCP server with basic scene tools, Tauri shell, CI.
2. **Core engine and tools** (done): mesh editing ops, booleans, undo stack,
   glTF export, headless preview rendering, live viewer, HTTP transport.
3. **AAA quality layer** (done, except image-to-3D plugins): Manifold
   booleans, UV unwrapping, smart materials baked to PBR textures, lighting
   presets, textured glTF export.
4. **Polish and launch** (done): project files and autosave, agent guide and
   prompts, documentation, release pipeline with optional code signing, demo
   gallery.

5. **Characters** (in progress): blended-volume bodies, sculpt brushes,
   character starters with landmarks, skin material, polygon reduction with
   normal and occlusion baking and LODs (`make_game_ready`), and reference
   image comparison.

Next ideas: rigging, image-to-3D plugins (bring a concept image, get a
starting mesh), collision meshes for game engines.
