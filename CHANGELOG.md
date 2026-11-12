# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- Viewer: the floor's shadow and grid no longer draw across objects that
  reach below y = 0; the floor now moves down to the lowest object.

## [0.1.0] - first public release

### Modelling
- Primitives: cube, plane, sphere, cylinder, cone, torus.
- Edits: extrude, inset, rounded multi-segment bevel, Catmull-Clark subdivide,
  mirror, array, smooth/flat shading.
- Watertight booleans (union, difference, intersection) using Manifold.
- Faces picked by direction, by index, or all at once.
- Undo and redo for every change.

### Look
- Automatic UV unwrapping.
- Smart materials baked from the shape (edge wear, grime, dust, colour
  variation, normal maps): painted metal, bare metal, rusty metal, plastic,
  wood, stone, rubber.
- Lighting presets: studio, outdoor, sunset, night.

### Agents
- MCP server over stdio and Streamable HTTP, with a built-in modelling guide
  and the `model_asset` and `refine_asset` prompts.
- `render_preview` returns a multi-view image so agents can check their work.
- Sizes are in metres even on scaled objects (the scale is applied to the
  mesh before an edit), and colours can be given as sRGB hex.

### Files
- glTF 2.0 (.glb) export with embedded PBR textures.
- `.meshmind` project files (`save_project`, `open_project`).

### Desktop app
- Live 3D view of the agent's work, lighting picker, undo/redo, wireframe.
- Automatic save and restore of the last session.
- Windows, macOS and Linux.
