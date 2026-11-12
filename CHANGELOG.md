# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Shape tools that build parts the way artists do: `loft` (blades, handles,
  horns), `lathe` (round parts), `extrude_outline` (flat silhouettes) and
  `sweep` (tubes, rope, rings, helical grip wraps).
- `deform` (bend, twist, taper), `slice` (edge loops) and `move_vertices`
  (soft-falloff reshaping in a box).
- Smart materials `polished_metal` (brushed, scratched, mirror-like), `gold`
  (also brass and bronze, tarnished in creases) and `leather`.
- `render_preview` takes `focus` (object ids) for close-ups.
- See-through materials: `transmission` and `ior` on materials (`set_material`),
  rendered with refraction in the preview, shown in the viewer, and exported
  as glTF `KHR_materials_transmission` and `KHR_materials_ior`.
- Smart materials `glass`, `gemstone`, `bronze` (verdigris patina),
  `fabric` (woven), `ceramic` (glazed, crackled, chips show clay) and `bone`
  (also ivory and horn).
- `check_asset`: lists what a reviewer would flag (grey parts, stale
  materials, floating or detached parts, default names, triangle budget).
- `review_asset` prompt (art-director critique and fix pass) and a `style`
  argument for `model_asset` (realistic, stylized, low_poly).
- Worked recipes in docs/RECIPES.md and scripts/recipes.py: longsword, axe,
  barrel, potion bottle.

### Changed
- Desktop viewer: a studio environment with soft boxes and a horizon band
  for reflections (matching the agent's preview), soft contact shadows where
  objects meet the floor, a gradient background, sharper soft shadows, a
  quieter grid, glass via physical transmission, and a Turntable button (T).
- Preview renderer: studio soft boxes and a horizon band in reflections,
  shadow-mapped key light, floor shadow and contact shadow. The studio
  background is darker.
- Smooth shading keeps edges sharper than 50° crisp (auto-smooth), and
  concave polygons triangulate correctly.
- The agent guide now walks through planning parts, picking a shape tool by
  silhouette, every material preset, an art-director review, and common
  mistakes. docs/AGENTS.md explains how to prompt for the best results.

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
