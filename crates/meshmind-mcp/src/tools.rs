//! Tool definitions and handlers. Descriptions are written for a model to read.

use glam::{EulerRot, Quat, Vec3};
use std::sync::Arc;

use meshmind_core::bake::{self as detail_bake, BakeOptions};
use meshmind_core::character::{self, CharacterParams};
use meshmind_core::material::SmartSource;
use meshmind_core::sculpt::{self as brushes, Brush, Stroke};
use meshmind_core::sdf::{self, MeshOptions, SdfPart};
use meshmind_core::simplify::simplify;
use meshmind_core::smart::{self, Preset, SmartMaterial};
use meshmind_core::uv::{self, UnwrapOptions};
use meshmind_core::{
    csg, deform, export, ops, shapes, Axis, BooleanOp, LightingPreset, Material, MaterialTextures,
    Primitive, Scene, Selection,
};
use meshmind_render::{render_sheet_framed, View};
use serde_json::{json, Value};

/// What a tool returns: structured data, plus an optional PNG image.
pub struct ToolOutput {
    pub data: Value,
    pub image_png: Option<Vec<u8>>,
}

impl From<Value> for ToolOutput {
    fn from(data: Value) -> Self {
        ToolOutput {
            data,
            image_png: None,
        }
    }
}

/// Tools that change the scene (and so get an undo step).
pub fn is_mutating(name: &str) -> bool {
    !matches!(
        name,
        "list_objects"
            | "get_object"
            | "check_asset"
            | "render_preview"
            | "compare_to_reference"
            | "export_glb"
            | "save_project"
            | "undo"
            | "redo"
    )
}

fn vec3_schema(desc: &str) -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3, "description": desc })
}

fn faces_schema() -> Value {
    json!({
        "description": "Which faces to affect. \"all\"; or {\"facing\": \"+y\"} to pick faces pointing a \
            world direction (\"+x\", \"-x\", \"+y\", \"-y\", \"+z\", \"-z\" or [x, y, z]), with optional \
            \"max_angle\" in degrees (default 30); or a list of face indices from get_object.",
        "oneOf": [
            { "type": "string", "enum": ["all"] },
            { "type": "object", "properties": {
                "facing": { "oneOf": [{ "type": "string" }, { "type": "array", "items": { "type": "number" } }] },
                "max_angle": { "type": "number" } }, "required": ["facing"] },
            { "type": "array", "items": { "type": "integer" } }
        ]
    })
}

fn id_prop() -> Value {
    json!({ "type": "integer", "description": "Object id." })
}

fn part_schema() -> Value {
    json!({
        "type": "object",
        "description": "One volume. ellipsoid: center + radii [x, y, z]. capsule: a, b (end points) + radius_a, radius_b (tapers; 0 at b makes a point, e.g. horns, tusks, claws). box: center + size [x, y, z] + rounding. rotation: Euler degrees for ellipsoids and boxes.",
        "properties": {
            "name": { "type": "string", "description": "Label, e.g. \"left upper arm\" (needed to edit it later)." },
            "shape": { "type": "string", "enum": ["ellipsoid", "capsule", "box"] },
            "center": { "type": "array", "items": { "type": "number" } },
            "radii": { "type": "array", "items": { "type": "number" } },
            "a": { "type": "array", "items": { "type": "number" } },
            "b": { "type": "array", "items": { "type": "number" } },
            "radius_a": { "type": "number" },
            "radius_b": { "type": "number" },
            "size": { "type": "array", "items": { "type": "number" } },
            "rounding": { "type": "number" },
            "rotation": { "type": "array", "items": { "type": "number" } },
            "op": { "type": "string", "enum": ["add", "subtract"] },
            "blend": { "type": "number", "description": "Metres of smooth blending into earlier parts. Default 0.03." },
            "mirror": { "type": "boolean", "description": "Also add a copy mirrored across x = 0." }
        },
        "required": ["shape"]
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required }
    })
}

pub fn definitions() -> Value {
    json!([
        tool("create_primitive",
            "Add a new object from a basic shape, centred on the origin (Y is up, units are metres). \
             Returns its id. Most models start from a few primitives that are then edited and combined.",
            json!({
                "shape": { "type": "string", "enum": ["cube", "plane", "sphere", "cylinder", "cone", "torus"] },
                "name": { "type": "string", "description": "Display name, e.g. \"Table leg\"." },
                "size": { "type": "number", "description": "Cube/plane edge length. Default 1." },
                "radius": { "type": "number", "description": "Sphere, cylinder and cone radius; torus ring radius. Default 0.5." },
                "height": { "type": "number", "description": "Cylinder and cone height. Default 1." },
                "tube_radius": { "type": "number", "description": "Torus tube radius. Default 0.15." },
                "segments": { "type": "integer", "description": "Detail around the axis. Default 32." },
                "rings": { "type": "integer", "description": "Sphere rings, or torus tube segments. Default 16." }
            }), &["shape"]),
        tool("loft",
            "Create an object by sweeping one cross-section shape up the Y axis through stations, \
             each scaling and shifting it. The way to model blades, swords, knives, horns, tapered \
             handles, table legs and spires. Example sword blade: a lens or diamond `shape` about 1 \
             wide and 0.2 thick, stations [{y:0, scale:[0.025,0.03]}, {y:0.65, scale:[0.022,0.025]}, \
             {y:0.78, scale:[0.012,0.02]}, {y:0.82, scale:[0,0]}] for a blade 5 cm wide tapering to \
             a point. A concave shape (a groove in each face) makes a fuller. Edges sharper than 50 \
             degrees render crisp, so a pointed shape gives a sharp cutting edge.",
            json!({ "name": { "type": "string" },
                    "shape": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                        "description": "Closed outline as [x, z] points (3 or more), centred on 0. Scaled by each station." },
                    "stations": { "type": "array", "items": { "type": "object", "properties": {
                            "y": { "type": "number", "description": "Height in metres; must increase." },
                            "scale": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, z] scale of the shape. [0, 0] makes a point." },
                            "offset": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, z] shift of the section (curved blades, leaning shapes)." } },
                        "required": ["y"] }, "description": "2 or more cross-sections from bottom to top." } }),
            &["shape", "stations"]),
        tool("lathe",
            "Create a round object by spinning a side profile around the Y axis, like a potter's \
             wheel. The way to model pommels, knobs, bottles, vases, goblets, wheels, columns and \
             finials. Profile points are [radius, y] from bottom to top; radius 0 closes the shape \
             on the axis. Sharp corners in the profile stay crisp.",
            json!({ "name": { "type": "string" },
                    "profile": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                        "description": "[radius, y] points in metres, in order along the outline." },
                    "segments": { "type": "integer", "description": "Detail around the axis. Default 32; use 6 or 8 for faceted shapes." } }),
            &["profile"]),
        tool("extrude_outline",
            "Create a flat-sided object from a 2D outline (in the XY plane) with a thickness along Z. \
             The way to model crossguards, axe heads, shields, brackets, signs, gears and any \
             silhouette. Concave outlines are fine. Bevel it afterwards to soften the edges.",
            json!({ "name": { "type": "string" },
                    "outline": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                        "description": "Closed outline as [x, y] points in metres (3 or more)." },
                    "depth": { "type": "number", "description": "Thickness along Z in metres, centred on z = 0." } }),
            &["outline", "depth"]),
        tool("sweep",
            "Create a round tube along a path: rope, cables, pipes, rails, rings, chains, and leather \
             or wire wrapped around a grip (use `helix`). Give either `path` points or `helix`.",
            json!({ "name": { "type": "string" },
                    "path": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                        "description": "[x, y, z] points in metres. Repeat the first point at the end for a closed loop." },
                    "helix": { "type": "object", "properties": {
                            "radius": { "type": "number" }, "height": { "type": "number" },
                            "turns": { "type": "number" }, "points_per_turn": { "type": "integer" } },
                        "description": "A spiral around Y centred on the origin, e.g. a grip wrap: {radius: 0.017, height: 0.14, turns: 10}." },
                    "radius": { "type": "number", "description": "Tube radius in metres." },
                    "radii": { "type": "array", "items": { "type": "number" }, "description": "Optional radius per path point (tapering tubes)." },
                    "segments": { "type": "integer", "description": "Detail around the tube. Default 12." } }),
            &["radius"]),
        tool("deform",
            "Bend, twist or taper a whole object along an axis (curved blades and horns, twisted \
             bars and ropes, tapering legs and towers). The low end of the axis stays put. Edge \
             loops are added automatically so the curve is smooth.",
            json!({ "id": id_prop(),
                    "mode": { "type": "string", "enum": ["taper", "twist", "bend"] },
                    "axis": { "type": "string", "enum": ["x", "y", "z"], "description": "Along which local axis. Default y." },
                    "amount": { "type": "number", "description": "taper: scale at the far end (0 = point, 0.5 = half). twist and bend: degrees." },
                    "toward": { "type": "string", "enum": ["x", "y", "z"], "description": "bend: which axis the object curves towards (positive side; use a negative amount for the other way). Default x." },
                    "loops": { "type": "integer", "description": "Edge loops to add first. Default 24 for bend and twist, 0 for taper." } }),
            &["id", "mode", "amount"]),
        tool("slice",
            "Add evenly spaced edge loops across an object along an axis, so later edits (deform, \
             move_vertices) have vertices to work with.",
            json!({ "id": id_prop(),
                    "axis": { "type": "string", "enum": ["x", "y", "z"] },
                    "cuts": { "type": "integer", "minimum": 1, "maximum": 64 } }),
            &["id", "axis", "cuts"]),
        tool("move_vertices",
            "Move or scale the vertices of an object inside a world-space box, for shaping details: \
             pinch a tip, flare a guard's ends, push a dent, widen one end. `falloff` blends the \
             edit smoothly into the surrounding surface. Use slice first if the area has no vertices.",
            json!({ "id": id_prop(),
                    "min": vec3_schema("Box corner (world space, metres)."),
                    "max": vec3_schema("Opposite box corner."),
                    "translate": vec3_schema("Move by [x, y, z] metres."),
                    "scale": vec3_schema("Scale around the box centre, e.g. [1.5, 1, 1.5]."),
                    "falloff": { "type": "number", "description": "Width of the soft blend around the box in metres. Default 0." } }),
            &["id", "min", "max"]),
        tool("create_character",
            "Start a character from a ready-made body with believable anatomy: one smooth, \
             watertight surface (no visible joins) with head, face (brow, eye sockets, nose, lips, \
             ears), neck, muscles, arms, five-fingered hands, legs and feet, plus eyeballs as separate \
             objects. ALWAYS start humanoids and creatures here instead of stacking primitives. \
             Returns the body id, eye ids and `landmarks` (world points such as left_hand_grip, \
             waist, head_top, left_lower_canine) to place weapons, belts, armour, tusks and hair. \
             Then reshape with sculpt_body (edit_parts by name) and sculpt, and add clothing, armour \
             and props as separate objects. Faces +Z; the character's left is +X; feet on y = 0.",
            json!({ "species": { "type": "string", "enum": character::Species::ALL.map(|s| s.name()),
                        "description": "Sets proportions: human (7.5 heads tall), orc (broad, hunched, heavy jaw, pointed ears), goblin (small, big head, long ears and nose, pot belly), dwarf (short, wide, barrel chest), troll (huge, long arms, stooped). Default human." },
                    "name": { "type": "string" },
                    "height": { "type": "number", "description": "Standing height in metres. Defaults to the species' (human 1.8, orc 2.05, goblin 1.0, dwarf 1.35, troll 2.8)." },
                    "feminine": { "type": "boolean", "description": "Narrower shoulders and waist, wider hips, breasts, finer jaw." },
                    "bulk": { "type": "number", "minimum": 0, "maximum": 1, "description": "0 lean, 0.5 average, 1 massive. Defaults to the species'." },
                    "gut": { "type": "number", "minimum": 0, "maximum": 1, "description": "Belly size, 0 flat to 1 big." },
                    "hunch": { "type": "number", "minimum": 0, "maximum": 1, "description": "0 upright to 1 stooped." },
                    "jaw": { "type": "number", "minimum": 0, "maximum": 1, "description": "0 small jaw to 1 heavy jaw with underbite." },
                    "head_size": { "type": "number", "description": "Head size multiplier, 1 normal. Stylized characters often use 1.2 to 1.5." },
                    "arm_length": { "type": "number", "description": "Arm length multiplier, 1 normal." },
                    "leg_length": { "type": "number", "description": "Leg length multiplier, 1 normal." },
                    "ears": { "type": "string", "enum": ["round", "pointed", "long"] },
                    "pose": { "type": "string", "enum": ["relaxed", "a_pose"], "description": "relaxed (arms near the body, default) or a_pose (arms 45 degrees out, for rigging)." },
                    "detail": { "type": "string", "enum": ["low", "medium", "high"], "description": "Mesh density. Default medium." } }),
            &[]),
        tool("sculpt_body",
            "Model organic shapes as clay: overlapping volumes (ellipsoids, tapered capsules, rounded \
             boxes) melted together into ONE smooth, watertight surface. Use it for creatures, \
             monsters, animals, tentacles, rocks, roots, food and any organic form, never a pile of \
             separate primitives. `blend` (metres) is how much a part melts into the parts before it: \
             0.01 to 0.03 for tight joins, 0.05 to 0.15 for soft fleshy transitions. op \"subtract\" \
             carves (eye sockets, mouths, grooves). `mirror` adds a copy across x = 0 (build \
             symmetric things centred on x = 0, facing +Z). Pass `id` to rebuild an existing body: \
             `edit_parts` changes named parts (only the fields given), `add_parts` and \
             `remove_parts` add or drop parts. Rebuilding discards sculpt strokes, so settle the \
             big forms first.",
            json!({ "name": { "type": "string" },
                    "id": { "type": "integer", "description": "Rebuild this body (from sculpt_body or create_character) instead of creating a new object." },
                    "parts": { "type": "array", "items": part_schema(), "description": "The full list of parts, in order. Replaces the body's parts when `id` is given." },
                    "add_parts": { "type": "array", "items": part_schema(), "description": "Parts to append to the body named by `id`." },
                    "edit_parts": { "type": "array", "items": { "type": "object" },
                        "description": "Changes to existing parts, matched by `name`, e.g. [{\"name\": \"ear\", \"radii\": [0.02, 0.06, 0.2]}, {\"name\": \"belly\", \"center\": [0, 1.0, 0.2]}]. Only the fields given change." },
                    "remove_parts": { "type": "array", "items": { "type": "string" }, "description": "Names of parts to drop." },
                    "detail": { "type": "string", "enum": ["low", "medium", "high"], "description": "Mesh density. Default medium. Thin parts (fingers) automatically get finer detail." } }),
            &[]),
        tool("sculpt",
            "Sculpt a dense organic surface (from sculpt_body or create_character, or after \
             subdivide) with a brush along a stroke, like ZBrush. Every vertex within `radius` of \
             the stroke moves, fading smoothly to the edge. Mirrored across the object's x = 0 by \
             default. Brushes: inflate (push out by `strength` metres, negative pushes in: muscles, \
             cheekbones, knuckles, veins with a small radius), crease (groove `strength` metres deep: \
             wrinkles, eyelids, lips, muscle separations, scars), pinch (0 to 1: sharpen a ridge such \
             as a jaw line), smooth (0 to 1), flatten (0 to 1: facial planes, worn areas), grab \
             (move by `offset`: pull out an ear, nose or chin), noise (lumps `strength` metres high \
             and `scale` metres across: warts, rough hide, rock). Points are world coordinates on \
             the surface; use create_character landmarks or get_object bounds to find them.",
            json!({ "id": id_prop(),
                    "brush": { "type": "string", "enum": ["inflate", "crease", "pinch", "smooth", "flatten", "grab", "noise"] },
                    "points": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                        "description": "Stroke path as [x, y, z] world points (one point is a single dab). Omit to affect the whole object (inflate, smooth and noise only)." },
                    "radius": { "type": "number", "description": "Brush radius in metres." },
                    "strength": { "type": "number", "description": "Metres for inflate, crease and noise; 0 to 1 for pinch, smooth and flatten. Default 0.5 for those, 0.01 for the others." },
                    "offset": vec3_schema("grab only: how far to drag, [x, y, z] metres."),
                    "scale": { "type": "number", "description": "noise only: bump size in metres. Default radius / 4." },
                    "symmetric": { "type": "boolean", "description": "Also apply mirrored across x = 0. Default true." } }),
            &["id", "brush", "radius"]),
        tool("make_game_ready",
            "The last modelling step for a dense or sculpted object (characters, creatures, sculpted \
             props, anything over a few thousand triangles): reduce it to a game budget, bake every \
             sculpted crease and bump into a normal map plus ambient occlusion, re-apply its smart \
             material on top, and build LODs (lighter copies for distance, exported as _LOD1, _LOD2). \
             It looks almost the same up close at a fraction of the triangles. Do it after the shape \
             and materials are final; undo restores the dense version.",
            json!({ "id": id_prop(),
                    "triangles": { "type": "integer", "description": "Target triangle count. Default: a fifth of the current count, between 500 and 20000. Typical budgets: hero character 15000 to 40000, enemy 5000 to 15000, weapon 1500 to 5000, small prop 300 to 1500." },
                    "lods": { "type": "array", "items": { "type": "number" }, "description": "Fractions of `triangles` for each LOD. Default [0.5, 0.25]; [] for none." },
                    "resolution": { "type": "integer", "enum": [512, 1024, 2048], "description": "Texture size. Default 1024 (2048 for hero assets)." } }),
            &["id"]),
        tool("decimate",
            "Reduce an object's triangle count while keeping its shape (quadric edge collapse). \
             make_game_ready does this plus baking, so prefer it; use decimate for quick low-poly \
             versions. Clears UVs and baked textures.",
            json!({ "id": id_prop(),
                    "triangles": { "type": "integer", "description": "Target triangle count." },
                    "ratio": { "type": "number", "description": "Or a fraction of the current count, e.g. 0.25." } }),
            &["id"]),
        tool("bake_detail",
            "Bake the surface detail of a dense object (`source`) onto a lighter object (`id`) that \
             covers the same shape, as a normal map and ambient occlusion, then re-apply the light \
             object's material on top. For hand-made low-poly versions; make_game_ready does the \
             whole job in one step.",
            json!({ "id": id_prop(),
                    "source": { "type": "integer", "description": "The dense, detailed object." },
                    "resolution": { "type": "integer", "enum": [512, 1024, 2048] },
                    "distance": { "type": "number", "description": "How far apart the two surfaces may be, in metres. Default 2% of the object's size." } }),
            &["id", "source"]),
        tool("list_objects",
            "List all objects with id, name, transform, world-space size and bounding box, face count and material.",
            json!({}), &[]),
        tool("check_asset",
            "Check the scene for problems a reviewer would spot, and get a list of fixes: parts left \
             with the default grey material, materials made stale by later shape edits, parts \
             floating free of the rest or the model not standing on the ground, default names, and \
             a triangle count against typical game budgets. Call it before you call the asset done, \
             alongside render_preview.",
            json!({}), &[]),
        tool("get_object",
            "Details of one object, including each face's index, centre and normal (local space), \
             so you can pick faces by index for extrude/inset.",
            json!({ "id": id_prop(), "max_faces": { "type": "integer", "description": "Limit on faces listed. Default 100." },
                    "parts": { "type": "boolean", "description": "For bodies from sculpt_body or create_character: list every part in full (shape, position, size) instead of just names." } }),
            &["id"]),
        tool("set_transform",
            "Move, rotate or scale an object. Omitted fields keep their value. Rotation is Euler \
             degrees applied X, then Y, then Z.",
            json!({ "id": id_prop(), "translation": vec3_schema("Position [x, y, z]."),
                    "rotation_degrees": vec3_schema("Rotation [x, y, z] in degrees."),
                    "scale": vec3_schema("Scale [x, y, z].") }),
            &["id"]),
        tool("duplicate_object", "Copy an object, including its transform and material. Returns the new id.",
            json!({ "id": id_prop() }), &["id"]),
        tool("delete_object", "Remove an object.", json!({ "id": id_prop() }), &["id"]),
        tool("clear_scene", "Remove every object. Can be undone.", json!({}), &[]),
        tool("extrude",
            "Push the selected faces out along their normals as one region, adding side walls. \
             Negative distance pushes in, making a recess. Use for legs, handles, panels, greebles.",
            json!({ "id": id_prop(), "faces": faces_schema(),
                    "distance": { "type": "number", "description": "Metres." } }),
            &["id", "faces", "distance"]),
        tool("inset",
            "Shrink each selected face towards its centre, leaving a border ring. Returns \
             inner_faces: extrude exactly those (pass the list as `faces`) by a small negative \
             distance for recessed panels, or positive for raised ones. Selecting by direction again \
             would also catch the ring.",
            json!({ "id": id_prop(), "faces": faces_schema(),
                    "amount": { "type": "number", "description": "0 to 1: fraction of the way to the centre. Typical 0.1 to 0.3." } }),
            &["id", "faces", "amount"]),
        tool("bevel",
            "Round off the hard edges of a closed object (edges where faces meet at more than \
             `angle` degrees, default 30). Real objects never have perfectly sharp edges: a small \
             rounded bevel (1 to 3% of the object's size) is one of the biggest steps towards a \
             professional look. Curved surfaces like a cylinder's side are left alone. Use \
             segments 1 for a flat chamfer (machined or cut look), 3 to 5 for rounded edges.",
            json!({ "id": id_prop(), "amount": { "type": "number", "description": "Bevel width in metres." },
                    "segments": { "type": "integer", "minimum": 1, "maximum": 16, "description": "1 = flat chamfer, more = rounder. Default 3." },
                    "angle": { "type": "number", "description": "Only edges sharper than this many degrees. Default 30." } }),
            &["id", "amount"]),
        tool("subdivide",
            "Split every face into smaller faces. smooth=true (default) rounds the shape \
             (Catmull-Clark), good for organic forms and soft edges. Each level makes about 4x the faces.",
            json!({ "id": id_prop(), "levels": { "type": "integer", "minimum": 1, "maximum": 4, "description": "Default 1." },
                    "smooth": { "type": "boolean" } }),
            &["id"]),
        tool("boolean",
            "Combine two objects. \"difference\" cuts the tool object out of the target (holes, \
             notches), \"union\" merges them, \"intersection\" keeps the overlap. The result \
             replaces the target; the tool object is deleted unless keep_tool is true. Both should be closed shapes.",
            json!({ "target": { "type": "integer" }, "tool": { "type": "integer" },
                    "operation": { "type": "string", "enum": ["union", "difference", "intersection"] },
                    "keep_tool": { "type": "boolean" } }),
            &["target", "tool", "operation"]),
        tool("mirror",
            "Mirror an object across its local origin plane and join both halves. Faces lying on the \
             mirror plane are removed, so model one half, then mirror for perfect symmetry.",
            json!({ "id": id_prop(), "axis": { "type": "string", "enum": ["x", "y", "z"] } }),
            &["id", "axis"]),
        tool("array",
            "Repeat an object's mesh several times inside the same object, each copy shifted by \
             offset (metres). Good for rails, bolts, steps, fences.",
            json!({ "id": id_prop(), "count": { "type": "integer", "minimum": 1, "maximum": 256 },
                    "offset": vec3_schema("Shift between copies [x, y, z].") }),
            &["id", "count", "offset"]),
        tool("set_shading",
            "Smooth shading hides facets on curved surfaces; flat shading keeps hard edges.",
            json!({ "id": id_prop(), "smooth": { "type": "boolean" } }),
            &["id", "smooth"]),
        tool("set_material",
            "Create a PBR material and assign it to an object. Colors are linear RGB 0 to 1 (or use \
             base_color_hex for an sRGB hex colour). Use \
             metallic 1 for bare metals and 0 for everything else (paint, plastic, wood, stone). \
             Roughness 0 is mirror-like, 1 is fully matte.",
            json!({ "id": id_prop(), "name": { "type": "string" },
                    "base_color": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 4 },
                    "base_color_hex": { "type": "string", "description": "Colour as sRGB hex, e.g. \"#c0392b\", instead of `base_color`." },
                    "metallic": { "type": "number", "minimum": 0, "maximum": 1 },
                    "roughness": { "type": "number", "minimum": 0, "maximum": 1 },
                    "emissive": vec3_schema("Glow color, linear RGB. Values above 1 glow brighter."),
                    "transmission": { "type": "number", "minimum": 0, "maximum": 1,
                        "description": "1 = clear like glass or water, tinted by base_color; 0 = opaque (default). Use roughness near 0 for clear glass, 0.3+ for frosted." },
                    "ior": { "type": "number", "description": "Index of refraction for transmissive materials: 1.33 water, 1.5 glass (default), 1.77 ruby, 2.42 diamond." } }),
            &["id"]),
        tool("unwrap_uvs",
            "Lay the object's surface flat into texture space (UV unwrapping). apply_smart_material \
             does this automatically; call it yourself only to re-unwrap. Any geometry edit clears UVs.",
            json!({ "id": id_prop() }), &["id"]),
        tool("apply_smart_material",
            "Give an object a realistic, textured material in one step: the surface is unwrapped and \
             textures are generated from its shape. Paint chips off convex edges, dirt and rust \
             collect in creases, colour varies naturally and fine bumps go into a normal map. This is \
             the main way to get an AAA look; plain set_material looks like clean CG. Do it after the \
             shape is final (geometry edits make the textures outdated). Bevel hard edges first: wear \
             follows them.",
            json!({ "id": id_prop(),
                    "preset": { "type": "string", "enum": Preset::ALL.map(|p| p.name()),
                        "description": "painted_metal (paint over metal), bare_metal (cast or machined), rusty_metal, polished_metal (blades, armour, chrome: mirror-like, brushed along Y), gold (also brass via color), bronze (green verdigris patina in creases), leather (grips, straps, belts), fabric (woven cloth: banners, sacks, upholstery), ceramic (glazed pottery, tiles; chips show clay), bone (also ivory, horn), glass (clear and see-through; color tints it), gemstone (see-through, saturated: ruby by default), skin (characters and creatures: mottled, warmer in creases, pores; give the skin colour), plastic, wood, stone (also concrete), rubber." },
                    "color": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3,
                        "description": "Main colour, linear RGB 0 to 1 (paint colour for painted_metal). Defaults to the preset's." },
                    "color_hex": { "type": "string", "description": "Main colour as sRGB hex, e.g. \"#5a6b2e\", instead of `color`." },
                    "wear": { "type": "number", "minimum": 0, "maximum": 1, "description": "Edge wear: 0 new, 0.3 used, 0.7 battered. Default 0.4." },
                    "dirt": { "type": "number", "minimum": 0, "maximum": 1, "description": "Grime in creases and dust: 0 clean, 1 filthy. Default 0.3." },
                    "scale": { "type": "number", "description": "Size of patterns (blotches, wood grain) in metres. Default 0.15." },
                    "seed": { "type": "integer", "description": "Change for a different random pattern." },
                    "resolution": { "type": "integer", "enum": [256, 512, 1024, 2048], "description": "Texture size. Default 1024." } }),
            &["id", "preset"]),
        tool("set_lighting",
            "Choose the lighting for previews and the viewer: studio (neutral, best for judging), \
             outdoor (sun and sky), sunset (warm, dramatic), night (cool, dim). Check important \
             models under more than one.",
            json!({ "preset": { "type": "string", "enum": LightingPreset::ALL.map(|p| p.name()) } }),
            &["preset"]),
        tool("render_preview",
            "Render the scene and return an image so you can check your work. Use it after every \
             few edits and fix what looks wrong. Default views: front, right, top and three_quarter \
             in a 2x2 grid (left to right, top to bottom).",
            json!({ "views": { "type": "array", "items": { "type": "string",
                        "enum": ["front", "back", "left", "right", "top", "three_quarter"] } },
                    "size": { "type": "integer", "description": "Pixels per view, 128 to 1024. Default 384." },
                    "focus": { "type": "array", "items": { "type": "integer" },
                        "description": "Object ids to zoom in on (a close-up of small parts such as a hilt or a handle). The rest of the scene is still drawn." },
                    "focus_point": vec3_schema("Zoom in on this world point instead, e.g. a character's head_center or hand landmark."),
                    "focus_radius": { "type": "number", "description": "With focus_point: how much to show around it, in metres. Default 0.3." } }),
            &[]),
        tool("set_reference",
            "Load a reference image (concept art, a photo, a sketch: PNG or JPEG file) to model from. \
             Ask the user for one whenever they describe something specific, and work from it: AAA \
             models match their reference. `view` says which camera angle it shows. Loading another \
             with the same name replaces it.",
            json!({ "path": { "type": "string", "description": "PNG or JPEG file path." },
                    "view": { "type": "string", "enum": ["front", "back", "left", "right", "three_quarter"], "description": "The angle the image shows. Default front." },
                    "name": { "type": "string", "description": "Label, e.g. \"front sketch\". Defaults to the view." } }),
            &["path"]),
        tool("compare_to_reference",
            "Render the model from the reference's angle next to the reference, plus the model's \
             outline drawn over the reference (matched by height), and measure where the silhouette \
             is too wide or too narrow. Use it after blocking out and after every round of changes: \
             fix silhouette and proportions first, then missing parts, then detail and materials.",
            json!({ "name": { "type": "string", "description": "Which reference. Default: the latest." },
                    "size": { "type": "integer", "description": "Panel height in pixels, 192 to 768. Default 448." } }),
            &[]),
        tool("export_glb",
            "Save the whole scene as a .glb file (glTF 2.0) for Unity, Unreal, Godot, Blender or the web.",
            json!({ "path": { "type": "string", "description": "File path ending in .glb." } }),
            &["path"]),
        tool("save_project",
            "Save the editable scene (meshes, materials, baked textures, lighting) as a .meshmind \
             project file, so work can continue later with open_project. Save before ending a session.",
            json!({ "path": { "type": "string", "description": "File path ending in .meshmind." } }),
            &["path"]),
        tool("open_project",
            "Replace the current scene with a .meshmind project saved earlier. Can be undone.",
            json!({ "path": { "type": "string", "description": "File path ending in .meshmind." } }),
            &["path"]),
        tool("undo", "Undo the last change to the scene.", json!({}), &[]),
        tool("redo", "Redo the last undone change.", json!({}), &[]),
    ])
}

/// Run a tool. `None` means the tool name is unknown. Undo and redo are
/// handled by the server, which owns the history.
pub fn call(scene: &mut Scene, name: &str, args: &Value) -> Option<Result<ToolOutput, String>> {
    let r: Result<ToolOutput, String> = match name {
        "create_primitive" => create_primitive(scene, args).map(Into::into),
        "loft" | "lathe" | "extrude_outline" | "sweep" => {
            create_shape(scene, name, args).map(Into::into)
        }
        "deform" => edit(scene, args, |m, _, a| {
            let mode = a.get("mode").and_then(Value::as_str).unwrap_or_default();
            let axis = axis_arg(a, "axis")?.unwrap_or(Axis::Y);
            let amount = number(a, "amount")?;
            let how = match mode {
                "taper" => deform::Deform::Taper { end_scale: amount },
                "twist" => deform::Deform::Twist { degrees: amount },
                "bend" => deform::Deform::Bend {
                    degrees: amount,
                    toward: axis_arg(a, "toward")?.unwrap_or(if axis == Axis::X {
                        Axis::Y
                    } else {
                        Axis::X
                    }),
                },
                _ => return Err("`mode` must be taper, twist or bend".into()),
            };
            let default_loops = if mode == "taper" { 0 } else { 24 };
            let loops = a
                .get("loops")
                .and_then(Value::as_u64)
                .unwrap_or(default_loops)
                .min(128) as u32;
            let sliced = deform::slice(m, axis, loops);
            deform::deform(&sliced, axis, how)
        }),
        "slice" => edit(scene, args, |m, _, a| {
            let axis = axis_arg(a, "axis")?.ok_or("`axis` is required")?;
            let cuts = a
                .get("cuts")
                .and_then(Value::as_u64)
                .ok_or("`cuts` is required")?;
            Ok(deform::slice(m, axis, cuts.clamp(1, 64) as u32))
        }),
        "move_vertices" => move_vertices(scene, args).map(Into::into),
        "create_character" => create_character(scene, args).map(Into::into),
        "sculpt_body" => sculpt_body(scene, args).map(Into::into),
        "sculpt" => sculpt(scene, args).map(Into::into),
        "decimate" => decimate(scene, args).map(Into::into),
        "bake_detail" => bake_detail(scene, args).map(Into::into),
        "make_game_ready" => make_game_ready(scene, args).map(Into::into),
        "list_objects" => Ok(list_objects(scene).into()),
        "check_asset" => Ok(check_asset(scene).into()),
        "get_object" => get_object(scene, args).map(Into::into),
        "set_transform" => set_transform(scene, args).map(Into::into),
        "duplicate_object" => object_id(args, "id")
            .and_then(|id| scene.duplicate_object(id).map_err(|e| e.to_string()))
            .map(|id| json!({ "id": id }).into()),
        "delete_object" => object_id(args, "id")
            .and_then(|id| scene.remove_object(id).map_err(|e| e.to_string()))
            .map(|o| json!({ "deleted": o.id }).into()),
        "clear_scene" => {
            let ids: Vec<u32> = scene.objects().iter().map(|o| o.id).collect();
            for id in &ids {
                let _ = scene.remove_object(*id);
            }
            Ok(json!({ "deleted": ids.len() }).into())
        }
        "extrude" => selection_edit(scene, args, "extruded_faces", |m, sel, a| {
            ops::extrude(m, sel, number(a, "distance")?)
        }),
        "inset" => selection_edit(scene, args, "inner_faces", |m, sel, a| {
            ops::inset(m, sel, number(a, "amount")?)
        }),
        "bevel" => edit(scene, args, |m, _, a| {
            let angle = a.get("angle").and_then(Value::as_f64).unwrap_or(30.0) as f32;
            let segments = a.get("segments").and_then(Value::as_u64).unwrap_or(3) as u32;
            ops::bevel(m, positive(a, "amount", None)?, angle, segments)
        }),
        "subdivide" => edit(scene, args, |m, _, a| {
            let levels = a.get("levels").and_then(Value::as_u64).unwrap_or(1) as u32;
            let smooth = a.get("smooth").and_then(Value::as_bool).unwrap_or(true);
            let out = ops::subdivide(m, levels.max(1), smooth)?;
            if out.faces.len() > 500_000 {
                return Err(format!(
                    "that would make {} faces; use fewer levels",
                    out.faces.len()
                ));
            }
            Ok(out)
        }),
        "mirror" => edit(scene, args, |m, _, a| {
            let axis = match a.get("axis").and_then(Value::as_str) {
                Some("x") => Axis::X,
                Some("y") => Axis::Y,
                Some("z") => Axis::Z,
                _ => return Err("`axis` must be \"x\", \"y\" or \"z\"".into()),
            };
            Ok(ops::mirror(m, axis))
        }),
        "array" => edit(scene, args, |m, _, a| {
            let count = a
                .get("count")
                .and_then(Value::as_u64)
                .ok_or("`count` is required")? as u32;
            let offset = floats::<3>(a, "offset")?.ok_or("`offset` is required")?;
            ops::array(m, count, Vec3::from(offset))
        }),
        "set_shading" => edit(scene, args, |m, _, a| {
            let smooth = a
                .get("smooth")
                .and_then(Value::as_bool)
                .ok_or("`smooth` is required")?;
            let mut out = m.clone();
            for f in &mut out.faces {
                f.smooth = smooth;
            }
            Ok(out)
        }),
        "boolean" => boolean(scene, args).map(Into::into),
        "set_material" => set_material(scene, args).map(Into::into),
        "unwrap_uvs" => unwrap_uvs(scene, args).map(Into::into),
        "apply_smart_material" => apply_smart_material(scene, args).map(Into::into),
        "set_lighting" => {
            let name = args
                .get("preset")
                .and_then(Value::as_str)
                .unwrap_or_default();
            LightingPreset::from_name(name)
                .ok_or(format!("unknown lighting preset `{name}`"))
                .map(|p| {
                    scene.lighting = p;
                    json!({ "lighting": p.name() }).into()
                })
        }
        "render_preview" => render_preview(scene, args),
        "set_reference" => crate::reference::set_reference(scene, args).map(Into::into),
        "compare_to_reference" => crate::reference::compare_to_reference(scene, args),
        "export_glb" => export_glb(scene, args).map(Into::into),
        "save_project" => save_project(scene, args).map(Into::into),
        "open_project" => open_project(scene, args).map(Into::into),
        _ => return None,
    };
    Some(r)
}

fn number(args: &Value, key: &str) -> Result<f32, String> {
    args.get(key)
        .and_then(Value::as_f64)
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
        .ok_or(format!("`{key}` must be a number"))
}

fn positive(args: &Value, key: &str, default: Option<f32>) -> Result<f32, String> {
    match (args.get(key).filter(|v| !v.is_null()), default) {
        (None, Some(d)) => Ok(d),
        _ => {
            let n = number(args, key)?;
            if n > 0.0 {
                Ok(n)
            } else {
                Err(format!("`{key}` must be a positive number"))
            }
        }
    }
}

fn int(args: &Value, key: &str, default: u32) -> Result<u32, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| (3..=512).contains(n))
            .map(|n| n as u32)
            .ok_or(format!("`{key}` must be an integer from 3 to 512")),
    }
}

fn floats<const N: usize>(args: &Value, key: &str) -> Result<Option<[f32; N]>, String> {
    let Some(v) = args.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let arr = v
        .as_array()
        .ok_or(format!("`{key}` must be an array of {N} numbers"))?;
    if arr.len() != N {
        return Err(format!("`{key}` must have exactly {N} numbers"));
    }
    let mut out = [0.0; N];
    for (o, x) in out.iter_mut().zip(arr) {
        *o = x
            .as_f64()
            .filter(|f| f.is_finite())
            .ok_or(format!("`{key}` must contain only numbers"))? as f32;
    }
    Ok(Some(out))
}

fn object_id(args: &Value, key: &str) -> Result<u32, String> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .ok_or(format!("`{key}` (object id) is required"))
}

fn direction(v: &Value) -> Result<Vec3, String> {
    if let Some(s) = v.as_str() {
        return match s.to_ascii_lowercase().as_str() {
            "+x" | "x" | "right" => Ok(Vec3::X),
            "-x" | "left" => Ok(-Vec3::X),
            "+y" | "y" | "up" | "top" => Ok(Vec3::Y),
            "-y" | "down" | "bottom" => Ok(-Vec3::Y),
            "+z" | "z" | "front" => Ok(Vec3::Z),
            "-z" | "back" => Ok(-Vec3::Z),
            _ => Err(format!(
                "unknown direction `{s}`; use +x, -x, +y, -y, +z, -z or [x, y, z]"
            )),
        };
    }
    let arr = v
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or("`facing` must be a direction like \"+y\" or [x, y, z]")?;
    let d = Vec3::new(
        arr[0].as_f64().unwrap_or(0.0) as f32,
        arr[1].as_f64().unwrap_or(0.0) as f32,
        arr[2].as_f64().unwrap_or(0.0) as f32,
    );
    if d.length_squared() < 1e-12 {
        return Err("`facing` direction must not be zero".into());
    }
    Ok(d)
}

/// Parse a face selection. `world_to_local` turns a world direction into the
/// object's local normal space.
fn selection(args: &Value, world_to_local: glam::Mat3) -> Result<Selection, String> {
    let v = args.get("faces").ok_or("`faces` is required")?;
    if v.as_str() == Some("all") {
        return Ok(Selection::All);
    }
    if let Some(arr) = v.as_array() {
        let indices = arr
            .iter()
            .map(|i| {
                i.as_u64()
                    .map(|n| n as usize)
                    .ok_or("face indices must be integers")
            })
            .collect::<Result<_, _>>()?;
        return Ok(Selection::Faces { indices });
    }
    let facing = v
        .get("facing")
        .ok_or("`faces` must be \"all\", {\"facing\": ...} or a list of indices")?;
    let world = direction(facing)?;
    let max_angle_deg = v.get("max_angle").and_then(Value::as_f64).unwrap_or(30.0) as f32;
    Ok(Selection::Facing {
        direction: (world_to_local * world).normalize_or_zero(),
        max_angle_deg,
    })
}

/// Apply a mesh operation to the object named by `id`.
/// Like [`edit`] for operations on a face selection. Both extrude and inset
/// keep the selected faces' indices, so the result lists them under `key`
/// for the agent to chain the next step precisely (inset, then extrude the
/// inner faces only).
fn selection_edit(
    scene: &mut Scene,
    args: &Value,
    key: &str,
    op: impl FnOnce(
        &meshmind_core::PolyMesh,
        &Selection,
        &Value,
    ) -> Result<meshmind_core::PolyMesh, String>,
) -> Result<ToolOutput, String> {
    let id = object_id(args, "id")?;
    let scale_applied = apply_object_scale(scene, id)?;
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let world_to_local = glam::Mat3::from(obj.transform.matrix().matrix3).transpose();
    let sel = selection(args, world_to_local)?;
    let indices: Vec<usize> = sel
        .resolve(&obj.mesh)
        .iter()
        .enumerate()
        .filter(|(_, s)| **s)
        .map(|(i, _)| i)
        .collect();
    let mut out = edit(scene, args, |m, _, a| {
        op(
            m,
            &Selection::Faces {
                indices: indices.clone(),
            },
            a,
        )
    })?;
    out.data[key] = json!(indices);
    if scale_applied {
        out.data["scale_applied"] = json!(true);
    }
    Ok(out)
}

/// Bake an object's scale into its mesh (leaving rotation and position), so
/// widths, distances and texture detail are measured in real metres rather
/// than stretched with the object. Returns whether anything changed.
fn apply_object_scale(scene: &mut Scene, id: u32) -> Result<bool, String> {
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let scale = obj.transform.scale;
    if (scale - Vec3::ONE).abs().max_element() < 1e-6 {
        return Ok(false);
    }
    let mesh = obj.mesh.transformed(&glam::Affine3A::from_scale(scale));
    scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
    scene
        .object_mut(id)
        .map_err(|e| e.to_string())?
        .transform
        .scale = Vec3::ONE;
    Ok(true)
}

/// A colour from `<key>` (linear RGB 0 to 1) or `<key>_hex` ("#RRGGBB", sRGB
/// as in colour pickers and design tools).
fn color_arg<const N: usize>(args: &Value, key: &str) -> Result<Option<[f32; N]>, String> {
    let hex_key = format!("{key}_hex");
    if let Some(h) = args.get(&hex_key).and_then(Value::as_str) {
        let h = h.trim().trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("`{hex_key}` must look like \"#a1b2c3\""));
        }
        let mut out = [1.0; N];
        for (i, slot) in out.iter_mut().take(3).enumerate() {
            let v = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).expect("checked hex");
            *slot = meshmind_core::texture::srgb_to_linear(v as f32 / 255.0);
        }
        return Ok(Some(out));
    }
    floats::<N>(args, key)
}

fn edit(
    scene: &mut Scene,
    args: &Value,
    op: impl FnOnce(
        &meshmind_core::PolyMesh,
        glam::Mat3,
        &Value,
    ) -> Result<meshmind_core::PolyMesh, String>,
) -> Result<ToolOutput, String> {
    let id = object_id(args, "id")?;
    let scale_applied = apply_object_scale(scene, id)?;
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    // Normals transform by the inverse transpose, so world -> local uses the transpose.
    let world_to_local = glam::Mat3::from(obj.transform.matrix().matrix3).transpose();
    let mut mesh = op(&obj.mesh, world_to_local, args)?;
    // The shape changed, so any UV layout no longer fits.
    mesh.clear_uvs();
    let textured = scene
        .material(obj.material)
        .is_ok_and(|m| m.textures.is_some());
    let faces = mesh.faces.len();
    scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
    let mut out = json!({ "id": id, "faces": faces });
    if scale_applied {
        out["scale_applied"] = json!(true);
    }
    if textured {
        out["note"] = json!("This object's baked textures no longer match its shape. Call apply_smart_material again when the shape is final.");
    }
    Ok(out.into())
}

fn create_primitive(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let shape = args
        .get("shape")
        .and_then(Value::as_str)
        .ok_or("`shape` is required")?;
    let primitive = match shape {
        "cube" => Primitive::Cube {
            size: positive(args, "size", Some(1.0))?,
        },
        "plane" => Primitive::Plane {
            size: positive(args, "size", Some(1.0))?,
        },
        "sphere" => Primitive::UvSphere {
            radius: positive(args, "radius", Some(0.5))?,
            segments: int(args, "segments", 32)?,
            rings: int(args, "rings", 16)?,
        },
        "cylinder" => Primitive::Cylinder {
            radius: positive(args, "radius", Some(0.5))?,
            height: positive(args, "height", Some(1.0))?,
            segments: int(args, "segments", 32)?,
        },
        "cone" => Primitive::Cone {
            radius: positive(args, "radius", Some(0.5))?,
            height: positive(args, "height", Some(1.0))?,
            segments: int(args, "segments", 32)?,
        },
        "torus" => Primitive::Torus {
            major_radius: positive(args, "radius", Some(0.5))?,
            minor_radius: positive(args, "tube_radius", Some(0.15))?,
            segments: int(args, "segments", 32)?,
            tube_segments: int(args, "rings", 16)?,
        },
        other => {
            return Err(format!(
                "unknown shape `{other}`; use cube, plane, sphere, cylinder, cone or torus"
            ))
        }
    };
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(primitive.default_name());
    let id = scene
        .add_object(name, primitive.build())
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "name": name }))
}

fn axis_arg(args: &Value, key: &str) -> Result<Option<Axis>, String> {
    match args.get(key).and_then(Value::as_str) {
        None => Ok(None),
        Some("x") => Ok(Some(Axis::X)),
        Some("y") => Ok(Some(Axis::Y)),
        Some("z") => Ok(Some(Axis::Z)),
        Some(_) => Err(format!("`{key}` must be \"x\", \"y\" or \"z\"")),
    }
}

fn points2(args: &Value, key: &str) -> Result<Vec<glam::Vec2>, String> {
    let arr = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or(format!("`{key}` must be a list of [x, y] points"))?;
    arr.iter()
        .map(|p| {
            let a = p.as_array().filter(|a| a.len() == 2);
            let n = |i: usize| a.and_then(|a| a[i].as_f64()).map(|v| v as f32);
            match (n(0), n(1)) {
                (Some(x), Some(y)) => Ok(glam::Vec2::new(x, y)),
                _ => Err(format!("every point in `{key}` must be two numbers")),
            }
        })
        .collect()
}

fn create_shape(scene: &mut Scene, kind: &str, args: &Value) -> Result<Value, String> {
    let mesh = match kind {
        "loft" => {
            let shape = points2(args, "shape")?;
            let stations: Vec<shapes::Station> =
                serde_json::from_value(args.get("stations").cloned().unwrap_or(Value::Null))
                    .map_err(|e| format!("`stations`: {e}"))?;
            shapes::loft(&shape, &stations)?
        }
        "lathe" => {
            let segments = args.get("segments").and_then(Value::as_u64).unwrap_or(32) as u32;
            shapes::lathe(&points2(args, "profile")?, segments)?
        }
        "extrude_outline" => {
            shapes::extrude_outline(&points2(args, "outline")?, positive(args, "depth", None)?)?
        }
        _ => {
            let radius = positive(args, "radius", None)?;
            let segments = args.get("segments").and_then(Value::as_u64).unwrap_or(12) as u32;
            let path: Vec<Vec3> = if let Some(h) = args.get("helix").filter(|v| !v.is_null()) {
                shapes::helix(
                    positive(h, "radius", None)?,
                    number(h, "height")?,
                    positive(h, "turns", None)?,
                    h.get("points_per_turn")
                        .and_then(Value::as_u64)
                        .unwrap_or(24) as u32,
                )
            } else {
                let arr = args
                    .get("path")
                    .and_then(Value::as_array)
                    .ok_or("give `path` points or a `helix`")?;
                arr.iter()
                    .map(|p| {
                        let a: Option<Vec<f32>> = p.as_array().map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_f64().map(|v| v as f32))
                                .collect()
                        });
                        match a {
                            Some(a) if a.len() == 3 => Ok(Vec3::new(a[0], a[1], a[2])),
                            _ => Err("every `path` point must be [x, y, z]".to_string()),
                        }
                    })
                    .collect::<Result<_, _>>()?
            };
            let radii: Option<Vec<f32>> = args.get("radii").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_f64().map(|v| v as f32))
                    .collect()
            });
            shapes::sweep(&path, radius, radii.as_deref(), segments)?
        }
    };
    mesh.validate()?;
    let default_name = match kind {
        "loft" => "Loft",
        "lathe" => "Lathe",
        "extrude_outline" => "Outline",
        _ => "Tube",
    };
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(default_name);
    let faces = mesh.faces.len();
    let size = mesh.bounds().map(|b| b.size().to_array());
    let id = scene.add_object(name, mesh).map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "name": name, "faces": faces, "size": size }))
}

fn move_vertices(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    apply_object_scale(scene, id)?;
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let min = Vec3::from(floats::<3>(args, "min")?.ok_or("`min` is required")?);
    let max = Vec3::from(floats::<3>(args, "max")?.ok_or("`max` is required")?);
    let translate = floats::<3>(args, "translate")?
        .map(Vec3::from)
        .unwrap_or(Vec3::ZERO);
    let scale = floats::<3>(args, "scale")?
        .map(Vec3::from)
        .unwrap_or(Vec3::ONE);
    let falloff = args
        .get("falloff")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .max(0.0) as f32;
    let m = obj.transform.matrix();
    let world = obj.mesh.transformed(&m);
    let (moved_world, count) = deform::move_vertices(
        &world,
        min.min(max),
        min.max(max),
        translate,
        scale,
        falloff,
    )?;
    let local = moved_world.transformed(&m.inverse());
    let textured = scene
        .material(obj.material)
        .is_ok_and(|m| m.textures.is_some());
    scene.set_mesh(id, local).map_err(|e| e.to_string())?;
    let mut out = json!({ "id": id, "moved_vertices": count });
    if textured {
        out["note"] = json!("This object's baked textures no longer match its shape. Call apply_smart_material again when the shape is final.");
    }
    Ok(out)
}

fn detail_options(args: &Value) -> Result<MeshOptions, String> {
    let resolution = match args.get("detail").and_then(Value::as_str) {
        None | Some("medium") => 160,
        Some("low") => 96,
        Some("high") => 256,
        Some(other) => {
            return Err(format!(
                "`detail` must be low, medium or high, not `{other}`"
            ))
        }
    };
    Ok(MeshOptions {
        resolution,
        ..MeshOptions::default()
    })
}

/// Parse parts, accepting `radius` as shorthand for both capsule radii.
fn parse_parts(v: &Value, key: &str) -> Result<Vec<SdfPart>, String> {
    let arr = v
        .as_array()
        .ok_or(format!("`{key}` must be a list of parts"))?;
    arr.iter()
        .enumerate()
        .map(|(i, p)| {
            let mut p = p.clone();
            if let Some(o) = p.as_object_mut() {
                if let Some(r) = o.get("radius").cloned() {
                    o.entry("radius_a").or_insert(r.clone());
                    o.entry("radius_b").or_insert(r);
                    o.remove("radius");
                }
                if o.get("shape").and_then(Value::as_str) == Some("sphere") {
                    o.insert("shape".into(), json!("ellipsoid"));
                    if let Some(r) = o.get("radius_a").and_then(Value::as_f64) {
                        o.insert("radii".into(), json!([r, r, r]));
                        o.remove("radius_a");
                        o.remove("radius_b");
                    }
                }
            }
            serde_json::from_value::<SdfPart>(p).map_err(|e| format!("`{key}` part {i}: {e}"))
        })
        .collect()
}

fn part_names(parts: &[SdfPart]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for p in parts {
        if !p.name.is_empty() && !names.contains(&p.name) {
            names.push(p.name.clone());
        }
    }
    names
}

fn sculpt_body(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let opts = detail_options(args)?;
    let existing = args.get("id").and_then(Value::as_u64).map(|n| n as u32);
    let mut parts: Vec<SdfPart> = match existing {
        Some(id) => scene
            .object(id)
            .map_err(|e| e.to_string())?
            .body
            .clone()
            .unwrap_or_default(),
        None => Vec::new(),
    };
    if let Some(v) = args.get("parts") {
        parts = parse_parts(v, "parts")?;
    }
    if let Some(edits) = args.get("edit_parts").and_then(Value::as_array) {
        for e in edits {
            let name = e
                .get("name")
                .and_then(Value::as_str)
                .ok_or("every `edit_parts` entry needs the `name` of the part to change")?;
            let mut found = false;
            for p in parts.iter_mut().filter(|p| p.name == name) {
                let mut merged = serde_json::to_value(&*p).map_err(|e| e.to_string())?;
                for (k, v) in e.as_object().into_iter().flatten() {
                    merged[k] = v.clone();
                }
                *p = parse_parts(&json!([merged]), "edit_parts")?.remove(0);
                found = true;
            }
            if !found {
                return Err(format!(
                    "no part named `{name}`. Parts: {}",
                    part_names(&parts).join(", ")
                ));
            }
        }
    }
    if let Some(names) = args.get("remove_parts").and_then(Value::as_array) {
        for n in names.iter().filter_map(Value::as_str) {
            let before = parts.len();
            parts.retain(|p| p.name != n);
            if parts.len() == before {
                return Err(format!("no part named `{n}` to remove"));
            }
        }
    }
    if let Some(v) = args.get("add_parts") {
        parts.extend(parse_parts(v, "add_parts")?);
    }
    if parts.is_empty() {
        return Err("give `parts` (or `id` of a body plus changes)".into());
    }
    let mesh = sdf::build(&parts, opts)?;
    let faces = mesh.faces.len();
    let watertight = mesh.is_closed_manifold();
    let id = match existing {
        Some(id) => {
            scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
            id
        }
        None => {
            let name = args.get("name").and_then(Value::as_str).unwrap_or("Body");
            scene.add_object(name, mesh).map_err(|e| e.to_string())?
        }
    };
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        scene.object_mut(id).map_err(|e| e.to_string())?.name = name.into();
    }
    let obj = scene.object_mut(id).map_err(|e| e.to_string())?;
    obj.body = Some(parts.clone());
    let textured = scene
        .material(scene.object(id).map_err(|e| e.to_string())?.material)
        .is_ok_and(|m| m.textures.is_some());
    let mut out =
        json!({ "id": id, "faces": faces, "parts": parts.len(), "part_names": part_names(&parts) });
    if !watertight {
        out["note"] = json!(
            "Some very thin parts did not mesh cleanly. Make them thicker or use detail \"high\"."
        );
    }
    if textured {
        out["textures_note"] = json!(
            "Call apply_smart_material again: the baked textures no longer match the new shape."
        );
    }
    Ok(out)
}

fn create_character(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let opt_f = |k: &str| args.get(k).and_then(Value::as_f64).map(|v| v as f32);
    let species = match args.get("species").and_then(Value::as_str) {
        None => None,
        Some(s) => Some(character::Species::from_name(s).ok_or(format!("unknown species `{s}`"))?),
    };
    let ears = match args.get("ears").and_then(Value::as_str) {
        None => None,
        Some("round") => Some(character::Ears::Round),
        Some("pointed") => Some(character::Ears::Pointed),
        Some("long") => Some(character::Ears::Long),
        Some(o) => return Err(format!("`ears` must be round, pointed or long, not `{o}`")),
    };
    let pose = match args.get("pose").and_then(Value::as_str) {
        None => None,
        Some("relaxed") => Some(character::Pose::Relaxed),
        Some("a_pose") => Some(character::Pose::APose),
        Some(o) => return Err(format!("`pose` must be relaxed or a_pose, not `{o}`")),
    };
    let params = CharacterParams {
        species,
        height: opt_f("height"),
        feminine: args
            .get("feminine")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        bulk: opt_f("bulk"),
        gut: opt_f("gut"),
        head_size: opt_f("head_size"),
        arm_length: opt_f("arm_length"),
        leg_length: opt_f("leg_length"),
        hunch: opt_f("hunch"),
        jaw: opt_f("jaw"),
        ears,
        pose,
    };
    let c = character::build_character(&params)?;
    let mesh = sdf::build(&c.parts, detail_options(args)?)?;
    let faces = mesh.faces.len();
    let species_name = c.species.name();
    let title = {
        let mut t = species_name.to_string();
        t[..1].make_ascii_uppercase();
        t
    };
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or(format!("{title} body"));
    let body = scene.add_object(name, mesh).map_err(|e| e.to_string())?;
    scene.object_mut(body).map_err(|e| e.to_string())?.body = Some(c.parts.clone());

    // Eyeballs: separate objects so they get their own glossy material.
    let eye_mat = scene.add_material(Material {
        name: "Eye".into(),
        base_color: [0.02, 0.012, 0.008, 1.0],
        metallic: 0.0,
        roughness: 0.06,
        ..Material::default()
    });
    let mut eye_ids = Vec::new();
    for (side, key) in [("Left", "left_eye"), ("Right", "right_eye")] {
        let eye = Primitive::UvSphere {
            radius: c.eye_radius,
            segments: 24,
            rings: 12,
        }
        .build();
        let id = scene
            .add_object(format!("{side} eye"), eye)
            .map_err(|e| e.to_string())?;
        scene
            .object_mut(id)
            .map_err(|e| e.to_string())?
            .transform
            .translation = c.landmarks[key];
        scene
            .assign_material(id, eye_mat)
            .map_err(|e| e.to_string())?;
        eye_ids.push(id);
    }
    let round = |v: Vec3| v.to_array().map(|c| (c * 1000.0).round() / 1000.0);
    let landmarks: serde_json::Map<String, Value> = c
        .landmarks
        .iter()
        .map(|(k, v)| (k.clone(), json!(round(*v))))
        .collect();
    Ok(json!({
        "body": body,
        "eyes": eye_ids,
        "species": species_name,
        "height": c.height,
        "faces": faces,
        "landmarks": landmarks,
        "part_names": part_names(&c.parts),
        "suggested_skin_hex": c.species.skin_hex(),
        "next": "Render it. Adjust proportions with sculpt_body (id, edit_parts by name). Add character with sculpt (crease wrinkles, inflate muscles, noise for rough skin). Model clothing, armour, hair, tusks, horns and props as separate objects placed at the landmarks. Then apply_smart_material: skin on the body, and leather, fabric, metal and bone on the rest."
    }))
}

fn sculpt(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    apply_object_scale(scene, id)?;
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let to_local = obj.transform.matrix().inverse();
    let radius = positive(args, "radius", None)?;
    let name = args
        .get("brush")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let brush = match name {
        "inflate" => Brush::Inflate,
        "crease" => Brush::Crease,
        "pinch" => Brush::Pinch,
        "smooth" => Brush::Smooth,
        "flatten" => Brush::Flatten,
        "grab" => {
            let o = floats::<3>(args, "offset")?.ok_or("grab needs `offset`")?;
            Brush::Grab {
                offset: to_local.transform_vector3(Vec3::from(o)),
            }
        }
        "noise" => Brush::Noise {
            scale: positive(args, "scale", Some(radius / 4.0))?,
        },
        _ => {
            return Err(
                "`brush` must be inflate, crease, pinch, smooth, flatten, grab or noise".into(),
            )
        }
    };
    let default_strength = match brush {
        Brush::Pinch | Brush::Smooth | Brush::Flatten => 0.5,
        Brush::Grab { .. } => 1.0,
        _ => 0.01,
    };
    let strength = match args.get("strength").filter(|v| !v.is_null()) {
        None => default_strength,
        Some(_) => number(args, "strength")?,
    };
    let points: Vec<Vec3> = match args.get("points").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .ok_or("`points` must be a list of [x, y, z]")?
            .iter()
            .map(|p| {
                let a: Vec<f32> = p
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_f64().map(|x| x as f32))
                            .collect()
                    })
                    .unwrap_or_default();
                if a.len() == 3 {
                    Ok(to_local.transform_point3(Vec3::new(a[0], a[1], a[2])))
                } else {
                    Err("every point must be [x, y, z]".to_string())
                }
            })
            .collect::<Result<_, _>>()?,
    };
    let stroke = Stroke {
        points,
        radius,
        strength,
        symmetric: args
            .get("symmetric")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    };
    let has_body = obj.body.is_some();
    let textured = scene
        .material(obj.material)
        .is_ok_and(|m| m.textures.is_some());
    let (mut mesh, moved) = brushes::sculpt(&obj.mesh, brush, &stroke)?;
    mesh.clear_uvs();
    scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
    let mut out = json!({ "id": id, "moved_vertices": moved });
    if moved == 0 {
        out["note"] = json!("Nothing was within `radius` of the points. Check the coordinates against the object's bounds or the character landmarks, or use a larger radius.");
    } else if moved < 12 {
        out["note"] = json!("Only a few vertices moved: the surface is coarse here. Subdivide first or use a larger radius.");
    } else if has_body {
        out["note"] = json!("Rebuilding this body with sculpt_body will discard sculpt strokes.");
    }
    if textured {
        out["textures_note"] = json!("Call apply_smart_material again when the shape is final.");
    }
    Ok(out)
}

fn tri_count(m: &meshmind_core::PolyMesh) -> usize {
    m.faces
        .iter()
        .map(|f| f.verts.len().saturating_sub(2))
        .sum()
}

fn decimate(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    let before = tri_count(&scene.object(id).map_err(|e| e.to_string())?.mesh);
    let target = match (
        args.get("triangles").and_then(Value::as_u64),
        args.get("ratio").and_then(Value::as_f64),
    ) {
        (Some(t), _) => t as usize,
        (None, Some(r)) if r > 0.0 && r <= 1.0 => (before as f64 * r) as usize,
        _ => return Err("give `triangles` or a `ratio` between 0 and 1".into()),
    };
    let mut out = edit(scene, args, |m, _, _| simplify(m, target))?;
    let after = tri_count(&scene.object(id).map_err(|e| e.to_string())?.mesh);
    out.data["triangles_before"] = json!(before);
    out.data["triangles"] = json!(after);
    Ok(out.data)
}

/// Bake the object's material again, on top of its baked detail. Objects
/// without a smart material get flat-colour textures that carry the detail.
fn rebake_material(scene: &mut Scene, id: u32, resolution: u32) -> Result<(), String> {
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let mesh = obj.mesh.clone();
    let detail = obj.detail.clone();
    let name = obj.name.clone();
    let old = scene
        .material(obj.material)
        .map_err(|e| e.to_string())?
        .clone();
    let material = if let Some(src) = &old.smart {
        let baked = smart::bake_with_detail(
            &mesh,
            &src.material,
            src.resolution.max(resolution),
            detail.as_deref(),
        )?;
        Material {
            textures: Some(Arc::new(MaterialTextures {
                base_color: baked.base_color,
                metallic_roughness: baked.metallic_roughness,
                normal: baked.normal,
            })),
            smart: Some(SmartSource {
                material: src.material.clone(),
                resolution: src.resolution.max(resolution),
            }),
            ..old
        }
    } else {
        let Some(d) = detail else { return Ok(()) };
        let n = d.normal.width;
        let mut base = meshmind_core::texture::Texture::new("base_color", n, n);
        let mut mr = meshmind_core::texture::Texture::new("metallic_roughness", n, n);
        for y in 0..n {
            for x in 0..n {
                let ao = d.occlusion.get(x, y)[0];
                let shade = (140 + ao as u32 * 115 / 255) as u8;
                base.set(x, y, [shade, shade, shade, 255]);
                mr.set(x, y, [ao, 255, 255, 255]);
            }
        }
        Material {
            name: format!("{name} (baked)"),
            textures: Some(Arc::new(MaterialTextures {
                base_color: base,
                metallic_roughness: mr,
                normal: d.normal.clone(),
            })),
            ..old
        }
    };
    let m = scene.add_material(material);
    scene.assign_material(id, m).map_err(|e| e.to_string())
}

fn bake_detail(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    let source = object_id(args, "source")?;
    if id == source {
        return Err("`source` must be a different, denser object".into());
    }
    let resolution = args
        .get("resolution")
        .and_then(Value::as_u64)
        .unwrap_or(1024) as u32;
    if ![512, 1024, 2048].contains(&resolution) {
        return Err("`resolution` must be 512, 1024 or 2048".into());
    }
    let low = scene.object(id).map_err(|e| e.to_string())?;
    let src = scene.object(source).map_err(|e| e.to_string())?;
    // The source, moved into the light object's own space.
    let to_local = low.transform.matrix().inverse() * src.transform.matrix();
    let high = src.mesh.transformed(&to_local);
    let mut mesh = low.mesh.clone();
    if !mesh.has_uvs() {
        uv::unwrap(&mut mesh, &UnwrapOptions::default())?;
    }
    let maps = detail_bake::bake_detail(
        &mesh,
        &high,
        BakeOptions {
            size: resolution,
            max_distance: args
                .get("distance")
                .and_then(Value::as_f64)
                .map(|d| d as f32),
            ..Default::default()
        },
    )?;
    scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
    scene.object_mut(id).map_err(|e| e.to_string())?.detail = Some(Arc::new(maps));
    rebake_material(scene, id, resolution)?;
    Ok(
        json!({ "id": id, "source": source, "resolution": resolution,
        "note": "Detail baked. You can hide or delete the source object now." }),
    )
}

fn make_game_ready(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    apply_object_scale(scene, id)?;
    let resolution = args
        .get("resolution")
        .and_then(Value::as_u64)
        .unwrap_or(1024) as u32;
    if ![512, 1024, 2048].contains(&resolution) {
        return Err("`resolution` must be 512, 1024 or 2048".into());
    }
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let high = obj.mesh.clone();
    let before = tri_count(&high);
    let target = args
        .get("triangles")
        .and_then(Value::as_u64)
        .map(|t| t as usize)
        .unwrap_or((before / 5).clamp(500, 20_000));
    if target >= before {
        return Err(format!(
            "the object already has {before} triangles; give a smaller `triangles`"
        ));
    }
    let lod_ratios: Vec<f64> = match args.get("lods").and_then(Value::as_array) {
        Some(a) => a
            .iter()
            .filter_map(Value::as_f64)
            .filter(|r| *r > 0.0 && *r < 1.0)
            .collect(),
        None => vec![0.5, 0.25],
    };
    let mut low = simplify(&high, target)?;
    uv::unwrap(&mut low, &UnwrapOptions::default())?;
    let maps = detail_bake::bake_detail(
        &low,
        &high,
        BakeOptions {
            size: resolution,
            ..Default::default()
        },
    )?;
    let mut lods = Vec::new();
    for r in &lod_ratios {
        lods.push(simplify(&low, (target as f64 * r) as usize)?);
    }
    let lod_tris: Vec<usize> = lods.iter().map(tri_count).collect();
    let after = tri_count(&low);
    scene.set_mesh(id, low).map_err(|e| e.to_string())?;
    let o = scene.object_mut(id).map_err(|e| e.to_string())?;
    o.detail = Some(Arc::new(maps));
    o.lods = lods;
    rebake_material(scene, id, resolution)?;
    Ok(json!({
        "id": id,
        "triangles_before": before,
        "triangles": after,
        "lod_triangles": lod_tris,
        "resolution": resolution,
        "note": "Detail is now in the normal and occlusion maps. Render a close-up to check it, and export_glb: LODs export as _LOD1, _LOD2 children. Undo restores the dense mesh."
    }))
}

/// Problems a reviewer would spot, each with the fix.
fn check_asset(scene: &Scene) -> Value {
    let mut issues: Vec<Value> = Vec::new();
    let mut issue = |id: Option<u32>, kind: &str, message: String| {
        issues.push(json!({ "object": id, "kind": kind, "message": message }));
    };
    let objects = scene.objects();
    if objects.is_empty() {
        return json!({ "ok": false, "issues": [{ "object": null, "kind": "empty", "message": "The scene is empty." }], "triangles": 0 });
    }
    let mut boxes = Vec::new();
    let mut triangles = 0usize;
    for o in objects {
        let world = o.mesh.transformed(&o.transform.matrix());
        let tris = world.to_mesh().triangle_count();
        triangles += tris;
        if let Some(b) = world.bounds() {
            boxes.push((o.id, b));
        }
        if o.material == Scene::DEFAULT_MATERIAL {
            issue(Some(o.id), "no_material", format!(
                "`{}` still has the default grey material. Give it apply_smart_material (or set_material for glowing or flat-colour parts).",
                o.name
            ));
        } else if let Ok(m) = scene.material(o.material) {
            if m.textures.is_some() && !o.mesh.has_uvs() {
                issue(Some(o.id), "stale_material", format!(
                    "`{}` was reshaped after its material was baked, so its textures no longer fit. Call apply_smart_material again.",
                    o.name
                ));
            }
        }
        let default_names = [
            "Cube", "Plane", "Sphere", "Cylinder", "Cone", "Torus", "Loft", "Lathe", "Outline",
            "Tube", "Object",
        ];
        let base = o
            .name
            .trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ' || c == '.');
        if default_names.contains(&base) {
            issue(Some(o.id), "default_name", format!(
                "`{}` has a default name. Name it after what it is (\"Blade\", \"Lid hinge\") so the model is easy to work with in a game engine.",
                o.name
            ));
        }
        if tris > 50_000 {
            issue(Some(o.id), "heavy", format!(
                "`{}` has {tris} triangles. Lower `segments` or subdivision levels unless it is a hero asset.",
                o.name
            ));
        }
    }
    let (lo, hi) = boxes.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), (_, b)| (lo.min(b.min), hi.max(b.max)),
    );
    let size = (hi - lo).max_element().max(1e-6);
    let tol = size * 0.01;
    if lo.y > tol {
        issue(None, "floating", format!(
            "The model's lowest point is {:.3} m above the ground (y = 0). Move it down so it stands on the ground.",
            lo.y
        ));
    } else if lo.y < -tol {
        issue(None, "below_ground", format!(
            "The model reaches {:.3} m below the ground (y = 0). Move it up so it stands on the ground, unless that is intended.",
            -lo.y
        ));
    }
    if boxes.len() > 1 {
        for (i, (id, b)) in boxes.iter().enumerate() {
            let touches = boxes.iter().enumerate().any(|(j, (_, c))| {
                i != j
                    && b.min.x <= c.max.x + tol
                    && c.min.x <= b.max.x + tol
                    && b.min.y <= c.max.y + tol
                    && c.min.y <= b.max.y + tol
                    && b.min.z <= c.max.z + tol
                    && c.min.z <= b.max.z + tol
            });
            if !touches {
                let name = scene
                    .object(*id)
                    .map(|o| o.name.clone())
                    .unwrap_or_default();
                issue(Some(*id), "detached", format!(
                    "`{name}` doesn't touch any other part. If it belongs to the same asset, move it into place."
                ));
            }
        }
    }
    let budget = match triangles {
        0..=2_000 => "very light: fine for small props and mobile",
        2_001..=20_000 => "typical for a game prop",
        20_001..=100_000 => "heavy: fine for a hero asset, high for a background prop",
        _ => "very heavy for a game asset: reduce segments and subdivision",
    };
    json!({
        "ok": issues.is_empty(),
        "issues": issues,
        "objects": objects.len(),
        "triangles": triangles,
        "triangle_budget": budget,
        "size": (hi - lo).to_array(),
    })
}

fn object_summary(o: &meshmind_core::Object) -> Value {
    let t = &o.transform;
    let (x, y, z) = t.rotation.to_euler(EulerRot::XYZ);
    // World-space bounding box, so the agent can line objects up.
    let bounds = o.mesh.transformed(&t.matrix()).bounds();
    let size = bounds.map(|b| b.size().to_array());
    let (min, max) = (
        bounds.map(|b| b.min.to_array()),
        bounds.map(|b| b.max.to_array()),
    );
    json!({
        "id": o.id,
        "name": o.name,
        "translation": t.translation.to_array(),
        "rotation_degrees": [x.to_degrees(), y.to_degrees(), z.to_degrees()],
        "scale": t.scale.to_array(),
        "size": size,
        "bounds_min": min,
        "bounds_max": max,
        "faces": o.mesh.faces.len(),
        "material": o.material,
    })
}

fn list_objects(scene: &Scene) -> Value {
    json!({ "objects": scene.objects().iter().map(object_summary).collect::<Vec<_>>() })
}

fn get_object(scene: &Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    let o = scene.object(id).map_err(|e| e.to_string())?;
    let max = args.get("max_faces").and_then(Value::as_u64).unwrap_or(100) as usize;
    let round = |v: Vec3| v.to_array().map(|c| (c * 1000.0).round() / 1000.0);
    let faces: Vec<Value> = o
        .mesh
        .faces
        .iter()
        .take(max)
        .enumerate()
        .map(|(i, f)| json!({ "index": i, "center": round(o.mesh.face_centroid(f)), "normal": round(o.mesh.face_normal(f)), "corners": f.verts.len() }))
        .collect();
    let mut out = object_summary(o);
    out["vertices"] = json!(o.mesh.positions.len());
    out["closed"] = json!(o.mesh.is_closed_manifold());
    out["face_list"] = json!(faces);
    out["face_list_truncated"] = json!(o.mesh.faces.len() > max);
    let m = scene.material(o.material).map_err(|e| e.to_string())?;
    out["material_detail"] = serde_json::to_value(m).unwrap_or(Value::Null);
    if let Some(parts) = &o.body {
        if args.get("parts").and_then(Value::as_bool).unwrap_or(false) {
            out["body_parts"] = serde_json::to_value(parts).unwrap_or(Value::Null);
        } else {
            out["body_part_names"] = json!(part_names(parts));
        }
    }
    Ok(out)
}

fn set_transform(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    let translation = floats::<3>(args, "translation")?;
    let rotation = floats::<3>(args, "rotation_degrees")?;
    let scale = floats::<3>(args, "scale")?;
    let obj = scene.object_mut(id).map_err(|e| e.to_string())?;
    if let Some(t) = translation {
        obj.transform.translation = Vec3::from(t);
    }
    if let Some([x, y, z]) = rotation {
        obj.transform.rotation = Quat::from_euler(
            EulerRot::XYZ,
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        );
    }
    if let Some(s) = scale {
        if s.iter().any(|c| c.abs() < 1e-6) {
            return Err("scale components must not be zero".into());
        }
        obj.transform.scale = Vec3::from(s);
    }
    Ok(json!({ "id": id }))
}

fn boolean(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let target = object_id(args, "target")?;
    let tool = object_id(args, "tool")?;
    if target == tool {
        return Err("target and tool must be different objects".into());
    }
    let op = match args.get("operation").and_then(Value::as_str) {
        Some("union") => BooleanOp::Union,
        Some("difference") => BooleanOp::Difference,
        Some("intersection") => BooleanOp::Intersection,
        _ => return Err("`operation` must be union, difference or intersection".into()),
    };
    let keep_tool = args
        .get("keep_tool")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let t = scene.object(target).map_err(|e| e.to_string())?;
    let b = scene.object(tool).map_err(|e| e.to_string())?;
    // Work in the target's local space.
    let target_m = t.transform.matrix();
    let tool_in_target = target_m.inverse() * b.transform.matrix();
    let result = csg::boolean(&t.mesh, &b.mesh.transformed(&tool_in_target), op)?;
    if result.faces.is_empty() {
        return Err("the result is empty (do the objects overlap?)".into());
    }
    let faces = result.faces.len();
    scene.set_mesh(target, result).map_err(|e| e.to_string())?;
    if !keep_tool {
        scene.remove_object(tool).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "id": target, "faces": faces, "tool_deleted": !keep_tool }))
}

fn set_material(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    scene.object(id).map_err(|e| e.to_string())?;
    let mut m = Material::default();
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        m.name = name.into();
    }
    if args.get("base_color_hex").is_some() {
        if let Some(c) = color_arg::<3>(args, "base_color")? {
            m.base_color[..3].copy_from_slice(&c);
        }
    } else if let Some(c) = args.get("base_color").filter(|v| !v.is_null()) {
        let arr = c
            .as_array()
            .filter(|a| a.len() == 3 || a.len() == 4)
            .ok_or("`base_color` must be [r, g, b] or [r, g, b, a]")?;
        for (i, v) in arr.iter().enumerate() {
            m.base_color[i] = v.as_f64().ok_or("`base_color` must contain numbers")? as f32;
        }
    }
    if let Some(v) = args.get("metallic").and_then(Value::as_f64) {
        m.metallic = v as f32;
    }
    if let Some(v) = args.get("roughness").and_then(Value::as_f64) {
        m.roughness = v as f32;
    }
    if let Some(e) = floats::<3>(args, "emissive")? {
        m.emissive = e;
    }
    if let Some(v) = args.get("transmission").and_then(Value::as_f64) {
        m.transmission = v as f32;
    }
    if let Some(v) = args.get("ior").and_then(Value::as_f64) {
        m.ior = v as f32;
    }
    let material = scene.add_material(m.sanitized());
    scene
        .assign_material(id, material)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "id": id, "material": material }))
}

fn render_preview(scene: &Scene, args: &Value) -> Result<ToolOutput, String> {
    let views: Vec<View> = match args.get("views").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|v| {
                v.as_str()
                    .and_then(View::from_name)
                    .ok_or(format!("unknown view {v}"))
            })
            .collect::<Result<_, _>>()?,
        _ => vec![View::Front, View::Right, View::Top, View::ThreeQuarter],
    };
    let size = args
        .get("size")
        .and_then(Value::as_u64)
        .unwrap_or(384)
        .clamp(128, 1024) as u32;
    let focus: Option<Vec<u32>> = match args.get("focus").and_then(Value::as_array) {
        Some(ids) if !ids.is_empty() => Some(
            ids.iter()
                .map(|v| {
                    let id = v.as_u64().ok_or("`focus` must be a list of object ids")? as u32;
                    scene.object(id).map_err(|e| e.to_string())?;
                    Ok(id)
                })
                .collect::<Result<_, String>>()?,
        ),
        _ => None,
    };
    let region = match floats::<3>(args, "focus_point")? {
        Some(c) => Some((Vec3::from(c), positive(args, "focus_radius", Some(0.3))?)),
        None => None,
    };
    let img = render_sheet_framed(scene, &views, size, focus.as_deref(), region);
    Ok(ToolOutput {
        data: json!({ "views": views.iter().map(|v| v.name()).collect::<Vec<_>>(), "width": img.width, "height": img.height }),
        image_png: Some(img.to_png()),
    })
}

fn export_glb(scene: &Scene, args: &Value) -> Result<Value, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("`path` is required")?;
    if !path.to_ascii_lowercase().ends_with(".glb") {
        return Err("`path` must end in .glb".into());
    }
    let bytes = export::to_glb(scene);
    std::fs::write(path, &bytes).map_err(|e| format!("could not write {path}: {e}"))?;
    let abs = std::fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or(path.to_string());
    Ok(json!({ "path": abs, "bytes": bytes.len(), "objects": scene.objects().len() }))
}

fn project_path(args: &Value) -> Result<&str, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or("`path` is required")?;
    if !path.to_ascii_lowercase().ends_with(".meshmind") {
        return Err("`path` must end in .meshmind".into());
    }
    Ok(path)
}

fn save_project(scene: &Scene, args: &Value) -> Result<Value, String> {
    let path = project_path(args)?;
    let bytes = meshmind_core::project::to_bytes(scene);
    // Write to a temporary file first so a failed save never destroys the old one.
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("could not write {path}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("could not write {path}: {e}"))?;
    let abs = std::fs::canonicalize(path)
        .map(|p| p.display().to_string())
        .unwrap_or(path.to_string());
    Ok(json!({ "path": abs, "bytes": bytes.len(), "objects": scene.objects().len() }))
}

fn open_project(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let path = project_path(args)?;
    let bytes = std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))?;
    *scene = meshmind_core::project::from_bytes(&bytes)?;
    Ok(json!({ "path": path, "objects": list_objects(scene)["objects"].clone() }))
}

fn unwrap_uvs(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    apply_object_scale(scene, id)?;
    let mut mesh = scene.object(id).map_err(|e| e.to_string())?.mesh.clone();
    let stats = uv::unwrap(&mut mesh, &UnwrapOptions::default())?;
    scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
    Ok(
        json!({ "id": id, "charts": stats.charts, "coverage": (stats.coverage * 100.0).round() / 100.0 }),
    )
}

fn apply_smart_material(scene: &mut Scene, args: &Value) -> Result<Value, String> {
    let id = object_id(args, "id")?;
    let name = args
        .get("preset")
        .and_then(Value::as_str)
        .ok_or("`preset` is required")?;
    let preset = Preset::from_name(name).ok_or(format!(
        "unknown preset `{name}`; use one of {}",
        Preset::ALL.map(|p| p.name()).join(", ")
    ))?;
    let mut sm = SmartMaterial::new(preset);
    if let Some(c) = color_arg::<3>(args, "color")? {
        sm.color = Some(c.map(|v| v.clamp(0.0, 1.0)));
    }
    if let Some(v) = args.get("wear").and_then(Value::as_f64) {
        sm.wear = (v as f32).clamp(0.0, 1.0);
    }
    if let Some(v) = args.get("dirt").and_then(Value::as_f64) {
        sm.dirt = (v as f32).clamp(0.0, 1.0);
    }
    if let Some(v) = args.get("scale").and_then(Value::as_f64) {
        sm.scale = (v as f32).clamp(0.005, 10.0);
    }
    if let Some(v) = args.get("seed").and_then(Value::as_u64) {
        sm.seed = v as u32;
    }
    let resolution = args
        .get("resolution")
        .and_then(Value::as_u64)
        .unwrap_or(1024) as u32;
    if ![256, 512, 1024, 2048].contains(&resolution) {
        return Err("`resolution` must be 256, 512, 1024 or 2048".into());
    }

    let scale_applied = apply_object_scale(scene, id)?;
    let obj = scene.object(id).map_err(|e| e.to_string())?;
    let mut mesh = obj.mesh.clone();
    let mut unwrapped = false;
    if !mesh.has_uvs() {
        uv::unwrap(&mut mesh, &UnwrapOptions::default())?;
        unwrapped = true;
    }
    let baked = smart::bake_with_detail(&mesh, &sm, resolution, obj.detail.as_deref())?;
    let material_name = format!("{} ({})", obj.name, preset.name());
    scene.set_mesh(id, mesh).map_err(|e| e.to_string())?;
    let material = scene.add_material(Material {
        name: material_name,
        base_color: [1.0; 4],
        metallic: 1.0,
        roughness: 1.0,
        transmission: preset.transmission(),
        ior: preset.ior(),
        textures: Some(Arc::new(MaterialTextures {
            base_color: baked.base_color,
            metallic_roughness: baked.metallic_roughness,
            normal: baked.normal,
        })),
        smart: Some(SmartSource {
            material: sm.clone(),
            resolution,
        }),
        ..Default::default()
    });
    scene
        .assign_material(id, material)
        .map_err(|e| e.to_string())?;
    let mut out = json!({ "id": id, "material": material, "preset": preset.name(), "resolution": resolution, "unwrapped": unwrapped });
    if scale_applied {
        out["scale_applied"] = json!(true);
    }
    Ok(out)
}
