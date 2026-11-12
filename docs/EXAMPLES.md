# Example prompts

Paste these into your agent once MeshMind is connected (see
[AGENTS.md](AGENTS.md)). Good prompts name the object, its size, its main
parts and the look you want.

## Props

> Using MeshMind, build a sci-fi supply crate about 1 m wide with bevelled
> edges, recessed side panels and worn olive paint. Export it to crate.glb.

> Using MeshMind, model a rusty oil barrel (0.6 m wide, 0.9 m tall) with two
> raised metal bands and a filler cap on top. Export to barrel.glb.

> Using MeshMind, make a low wooden stool with four slightly splayed legs and a
> round seat, in a warm oak wood. Save the project as stool.meshmind.

> Using MeshMind, build a stone pedestal: square base, octagonal column, and a
> wider top slab, all with chipped edges. Check it under outdoor and night lighting.

## Iterating

> The handles look too thin. Make them twice as thick and add a small bevel.

> Make the paint more worn around the edges and change the colour to a faded red.

> Cut a round porthole window in the front panel and give it a rubber seal.

> Undo the last two changes and try the panels as a raised frame instead.

## Workflow tips

- Ask for a `render_preview` whenever you want to see the agent's view.
- Materials are baked from the shape, so ask for shape changes first and
  materials last. The agent re-applies materials after shape edits.
- Ask the agent to `save_project` before you end a session; `open_project`
  continues where you stopped.
- The exported `.glb` opens directly in Unity, Unreal, Godot and Blender with
  its textures.
