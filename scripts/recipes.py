"""Worked modelling recipes, built by driving meshmind-mcp exactly as an agent would.

    cargo build --release -p meshmind-mcp
    python3 scripts/recipes.py [output-dir] [sword|axe|barrel|bottle ...]

docs/RECIPES.md explains each step. Every call here is an ordinary MCP tool
call over stdio, so an agent can follow the same steps.
"""
import base64, json, math, os, subprocess, sys

BIN = os.environ.get("MESHMIND_MCP", os.path.join("target", "release", "meshmind-mcp"))
OUT = sys.argv[1] if len(sys.argv) > 1 else "recipes"
ONLY = sys.argv[2:]


class MM:
    """A tiny MCP client: m("tool", **args) returns the tool's structured result."""

    def __init__(self):
        self.p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.n = 0
        self.rpc("initialize", {})

    def rpc(self, method, params):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}) + "\n")
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def call(self, tool_name, **args):
        r = self.rpc("tools/call", {"name": tool_name, "arguments": args})["result"]
        if r["isError"]:
            sys.exit(f"{tool_name} failed: {r['content'][0]['text']}")
        return r

    def __call__(self, tool_name, **args):
        return self.call(tool_name, **args).get("structuredContent", {})

    def shot(self, path, **args):
        r = self.call("render_preview", **args)
        img = next(c for c in r["content"] if c["type"] == "image")
        open(path, "wb").write(base64.b64decode(img["data"]))


def circle(r, y, n=32):
    return [[r * math.cos(2 * math.pi * i / n), y, r * math.sin(2 * math.pi * i / n)] for i in range(n)] + [[r, y, 0]]


def sword():
    # Layout along Y (metres): pommel 0-0.052, grip 0.058-0.218, guard ~0.242, blade 0.25-1.0.

    # Blade: a lens-shaped section with a fuller groove, lofted to a point.
    half = [(1, 0), (0.6, 0.36), (0.3, 0.5), (0.16, 0.42), (0.06, 0.3), (0, 0.28)]
    top = half + [(-x, z) for x, z in reversed(half[:-1])]
    shape = top + [(x, -z) for x, z in reversed(top[1:-1])]
    blade = m("loft", name="Blade", shape=shape, stations=[
        {"y": 0.0, "scale": [0.027, 0.008]},
        {"y": 0.02, "scale": [0.026, 0.008]},
        {"y": 0.5, "scale": [0.022, 0.0068]},
        {"y": 0.62, "scale": [0.0185, 0.0062]},
        {"y": 0.69, "scale": [0.013, 0.0055]},
        {"y": 0.73, "scale": [0.006, 0.0045]},
        {"y": 0.75, "scale": [0, 0]},
    ])["id"]
    m("set_transform", id=blade, translation=[0, 0.25, 0])

    # Crossguard: an octagonal bar lofted from tip to tip, thick at the centre,
    # slimmer along the arms, flaring into upswept ends. Built along Y, laid flat.
    octa = [(1, 0.4), (0.4, 1), (-0.4, 1), (-1, 0.4), (-1, -0.4), (-0.4, -1), (0.4, -1), (1, -0.4)]
    arm = lambda y, sx, sz, up: {"y": y, "scale": [sx, sz], "offset": [up, 0]}
    st = [arm(-0.118, 0.0, 0.0, 0.012), arm(-0.116, 0.007, 0.007, 0.011), arm(-0.108, 0.0095, 0.009, 0.0075),
          arm(-0.09, 0.0075, 0.0075, 0.003), arm(-0.045, 0.0068, 0.0072, 0.0), arm(-0.022, 0.009, 0.0105, 0.0),
          arm(-0.012, 0.0125, 0.0125, 0.0)]
    st = st + [dict(s_, y=-s_["y"]) for s_ in reversed(st)]
    st[len(st)//2 - 1]["y"] = -0.012; st[len(st)//2]["y"] = 0.012
    guard = m("loft", name="Guard", shape=octa, stations=st)["id"]
    m("set_transform", id=guard, translation=[0, 0.24, 0], rotation_degrees=[0, 0, 90])

    # Grip core, then a leather cord wrapped around it.
    core = m("lathe", name="Grip core", profile=[[0, 0], [0.0135, 0], [0.0155, 0.08], [0.0135, 0.16], [0, 0.16]], segments=24)["id"]
    m("set_transform", id=core, translation=[0, 0.058, 0])
    wrap = m("sweep", name="Grip wrap", helix={"radius": 0.0155, "height": 0.15, "turns": 22, "points_per_turn": 20},
             radius=0.0036, segments=8)["id"]
    m("set_transform", id=wrap, translation=[0, 0.138, 0])

    # Ferrules at both ends of the grip.
    ring = [[0, 0], [0.0175, 0], [0.019, 0.003], [0.0175, 0.008], [0, 0.008]]
    f1 = m("lathe", name="Ferrule low", profile=ring, segments=32)["id"]
    m("set_transform", id=f1, translation=[0, 0.052, 0])
    f2 = m("lathe", name="Ferrule high", profile=ring, segments=32)["id"]
    m("set_transform", id=f2, translation=[0, 0.218, 0])

    # Pommel: a wheel, lathed then turned to face forward, with a raised boss.
    pommel = m("lathe", name="Pommel", segments=48, profile=[
        [0, -0.0125], [0.011, -0.0125], [0.0125, -0.0105], [0.0135, -0.0085], [0.024, -0.0085], [0.029, -0.0055],
        [0.0305, 0], [0.029, 0.0055], [0.024, 0.0085], [0.0135, 0.0085], [0.0125, 0.0105], [0.011, 0.0125], [0, 0.0125]])["id"]
    m("set_transform", id=pommel, translation=[0, 0.021, 0], rotation_degrees=[90, 0, 0])

    m("apply_smart_material", id=blade, preset="polished_metal", wear=0.25, dirt=0.05, resolution=2048)
    m("apply_smart_material", id=guard, preset="polished_metal", color_hex="#b8b8bc", wear=0.4, dirt=0.25)
    m("apply_smart_material", id=pommel, preset="polished_metal", color_hex="#b8b8bc", wear=0.4, dirt=0.25)
    for f in [f1, f2]:
        m("apply_smart_material", id=f, preset="gold", wear=0.4, dirt=0.3)
    m("apply_smart_material", id=core, preset="leather", color_hex="#1e120b", wear=0.1, dirt=0.3)
    m("apply_smart_material", id=wrap, preset="leather", color_hex="#2e1c12", wear=0.3, dirt=0.25)
    return [guard, core, wrap, pommel, f1, f2]


def axe():
    oval = [[math.cos(a)*1, math.sin(a)*0.75] for a in [2*math.pi*i/12 for i in range(12)]]
    handle = m("loft", name="Handle", shape=oval, stations=[
        {"y": 0, "scale": [0.019, 0.019]}, {"y": 0.04, "scale": [0.017, 0.017]},
        {"y": 0.45, "scale": [0.015, 0.015]}, {"y": 0.62, "scale": [0.018, 0.018]}, {"y": 0.66, "scale": [0.016, 0.016]}])["id"]
    m("deform", id=handle, mode="bend", axis="y", amount=-12, toward="x")
    head = m("extrude_outline", name="Head", depth=0.022, outline=[
        [-0.03, -0.035], [0.03, -0.03], [0.06, -0.045], [0.12, -0.085], [0.135, -0.06], [0.14, 0.0],
        [0.135, 0.06], [0.12, 0.085], [0.06, 0.045], [0.03, 0.03], [-0.03, 0.035], [-0.055, 0.02], [-0.055, -0.02]])["id"]
    m("bevel", id=head, amount=0.003, segments=2)
    m("set_transform", id=head, translation=[0.0, 0.6, 0])
    wrap = m("sweep", name="Wrap", helix={"radius": 0.018, "height": 0.12, "turns": 14}, radius=0.003, segments=6)["id"]
    m("set_transform", id=wrap, translation=[0, 0.09, 0])
    m("apply_smart_material", id=handle, preset="wood", color_hex="#6b4226", wear=0.3)
    m("apply_smart_material", id=head, preset="polished_metal", color_hex="#a9abb0", wear=0.6, dirt=0.4)
    m("apply_smart_material", id=wrap, preset="leather", color_hex="#2e1c12", wear=0.3)
def barrel():
    body = m("lathe", name="Barrel", segments=20, profile=[
        [0, 0], [0.24, 0], [0.25, 0.01], [0.28, 0.2], [0.29, 0.35], [0.28, 0.5], [0.25, 0.69], [0.24, 0.7], [0, 0.7]])["id"]
    m("apply_smart_material", id=body, preset="wood", color_hex="#7a5230", wear=0.4, dirt=0.4, scale=0.3)
    for i, (y, r) in enumerate([(0.08, 0.258), (0.24, 0.287), (0.46, 0.287), (0.62, 0.258)]):
        band = m("lathe", name=f"Band {i+1}", segments=40, profile=[[0, 0], [r, 0], [r + 0.004, 0.004], [r + 0.004, 0.026], [r, 0.03], [0, 0.03]])["id"]
        m("set_transform", id=band, translation=[0, y - 0.015, 0])
        m("apply_smart_material", id=band, preset="rusty_metal", wear=0.5, dirt=0.5)
def bottle():
    glass = m("lathe", name="Bottle", segments=40, profile=[
        [0, 0], [0.04, 0], [0.048, 0.008], [0.05, 0.04], [0.045, 0.075], [0.02, 0.1], [0.014, 0.115], [0.014, 0.15], [0.017, 0.153], [0.017, 0.16], [0, 0.16]])["id"]
    m("apply_smart_material", id=glass, preset="glass", color_hex="#9fd8b0")
    cork = m("lathe", name="Cork", segments=24, profile=[[0, 0], [0.012, 0], [0.0135, 0.025], [0, 0.025]])["id"]
    m("set_transform", id=cork, translation=[0, 0.152, 0])
    m("apply_smart_material", id=cork, preset="wood", color_hex="#b08a5a", wear=0.2, scale=0.02)
    cord = m("sweep", name="Cord", path=circle(0.0155, 0, 24), radius=0.0025, segments=8)["id"]
    m("set_transform", id=cord, translation=[0, 0.125, 0])
    m("apply_smart_material", id=cord, preset="leather", color_hex="#5a3a22", wear=0.3)


def orc():
    # 1. Start from a body with real anatomy: one smooth surface, not a pile of parts.
    c = m("create_character", species="orc", name="Orc")
    body, L = c["body"], c["landmarks"]
    P = lambda k, dx=0, dy=0, dz=0: [L[k][0] + dx, L[k][1] + dy, L[k][2] + dz]

    # 2. Adjust proportions by part name: heavier forearms, a wider underbite.
    m("sculpt_body", id=body, edit_parts=[
        {"name": "forearm muscle", "radii": [0.075, 0.13, 0.068]},
        {"name": "lower lip", "radius_a": 0.016, "radius_b": 0.016},
    ])

    # 3. Sculpt character into the surface. Strokes are mirrored to the other side,
    #    and points snap onto the skin.
    eye, mouth, chest, waist = L["left_eye"], L["mouth"], L["chest_front"], L["waist_front"]
    m("sculpt", id=body, brush="crease", radius=0.012, strength=0.006,          # frown lines
      points=[[0.012, eye[1] + 0.035, eye[2] + 0.03], [0.02, eye[1] + 0.005, eye[2] + 0.03]])
    m("sculpt", id=body, brush="crease", radius=0.014, strength=0.005,          # nose-to-mouth folds
      points=[[0.05, eye[1] - 0.06, eye[2] + 0.02], [0.07, mouth[1] - 0.01, mouth[2] - 0.02]])
    m("sculpt", id=body, brush="inflate", radius=0.03, strength=0.008,          # cheekbones
      points=[[0.085, eye[1] - 0.035, eye[2]]])
    m("sculpt", id=body, brush="crease", radius=0.045, strength=0.008,          # under the chest muscles
      points=[[0.03, chest[1] - 0.09, chest[2]], [0.16, chest[1] - 0.08, chest[2] - 0.03], [0.24, chest[1] - 0.02, chest[2] - 0.08]])
    m("sculpt", id=body, brush="crease", radius=0.02, strength=0.008, symmetric=False,  # centre line of the abs
      points=[[0, chest[1] - 0.1, chest[2]], [0, waist[1] + 0.05, waist[2]]])
    for dy in (0.03, 0.12):                                                       # abdominal bands
        m("sculpt", id=body, brush="crease", radius=0.018, strength=0.005,
          points=[[0.01, waist[1] + dy, waist[2]], [0.09, waist[1] + dy + 0.01, waist[2] - 0.02]])
    m("sculpt", id=body, brush="inflate", radius=0.012, strength=0.004,         # a vein down the forearm
      points=[P("left_elbow", 0.03, 0, 0.05), P("left_forearm_mid", 0.04, 0, 0.04), P("left_wrist", 0.03, 0, 0.03)])
    m("sculpt", id=body, brush="noise", radius=1, strength=0.003, scale=0.03)    # lumpy hide all over
    m("apply_smart_material", id=body, preset="skin", color_hex="#3d5226", dirt=0.35, scale=0.12)

    # 4. Tusks: curved, tapering capsules from the lower canines, outward and up.
    t = L["left_lower_canine"]
    tusks = m("sculpt_body", name="Tusks", detail="high", parts=[
        {"name": "tusk", "shape": "capsule", "mirror": True, "blend": 0.004, "a": [t[0] - 0.005, t[1] - 0.012, t[2] - 0.018],
         "b": [t[0] + 0.012, t[1] + 0.035, t[2] + 0.006], "radius_a": 0.011, "radius_b": 0.008},
        {"name": "tusk tip", "shape": "capsule", "mirror": True, "blend": 0.004, "a": [t[0] + 0.012, t[1] + 0.035, t[2] + 0.006],
         "b": [t[0] + 0.03, t[1] + 0.065, t[2] - 0.006], "radius_a": 0.008, "radius_b": 0.0015},
    ])["id"]
    m("apply_smart_material", id=tusks, preset="bone", color_hex="#d8c8a0", dirt=0.5, scale=0.02)

    # 5. Gear sized from the landmarks: a belt and buckle, a loincloth, bracers, a pauldron.
    wl, wf, wb = L["waist_left"], L["waist_front"], L["waist_back"]
    rx, rz, zc = wl[0] + 0.012, (wf[2] - wb[2]) / 2 + 0.012, (wf[2] + wb[2]) / 2
    ring = [[math.cos(a) * rx, math.sin(a) * rz] for a in [i * 2 * math.pi / 40 for i in range(40)]]
    belt = m("loft", name="Belt", shape=ring, stations=[{"y": 0, "offset": [0, zc]}, {"y": 0.07, "offset": [0, zc]}])["id"]
    m("set_transform", id=belt, translation=[0, wl[1] - 0.035, 0])
    m("bevel", id=belt, amount=0.006)
    m("apply_smart_material", id=belt, preset="leather", color_hex="#4a2e1a", wear=0.5, dirt=0.4)
    buckle = m("extrude_outline", name="Buckle", depth=0.015, outline=[[-0.045, -0.04], [0.045, -0.04], [0.05, 0.04], [-0.05, 0.04]])["id"]
    m("set_transform", id=buckle, translation=[0, wl[1], wf[2] + 0.012])
    m("bevel", id=buckle, amount=0.004)
    m("apply_smart_material", id=buckle, preset="rusty_metal", wear=0.6, dirt=0.7, scale=0.03)
    cloth = m("extrude_outline", name="Loincloth", depth=0.006, outline=[[-0.12, 0], [0.12, 0], [0.15, -0.4], [0.03, -0.44], [-0.14, -0.41]])["id"]
    m("deform", id=cloth, mode="bend", amount=-18, axis="y", toward="z")
    m("bevel", id=cloth, amount=0.0025)
    m("set_transform", id=cloth, translation=[0, wl[1] - 0.02, wf[2] + 0.004])
    m("apply_smart_material", id=cloth, preset="fabric", color_hex="#5a2a1a", wear=0.6, dirt=0.6, scale=0.08)
    for side in (1, -1):
        e, w = L["left_elbow"], L["left_wrist"]
        r = L["left_forearm_outer"][0] - L["left_forearm_mid"][0] + 0.01
        d = [w[i] - e[i] for i in range(3)]
        length = math.sqrt(sum(x * x for x in d))
        bracer = m("lathe", name="Bracer", segments=24, profile=[[0, 0], [r * 0.95, 0], [r, 0.01], [r * 1.12, length * 0.5], [r * 1.18, length * 0.62], [r * 1.12, length * 0.64], [0, length * 0.64]])["id"]
        tilt = math.degrees(math.atan2(d[0], -d[1]))
        lean = math.degrees(math.atan2(d[2], -d[1]))
        m("set_transform", id=bracer, translation=[side * w[0], w[1] + 0.02, w[2] + 0.005], rotation_degrees=[-lean, 0, side * tilt])
        m("apply_smart_material", id=bracer, preset="leather", color_hex="#3a2416", wear=0.6, dirt=0.4)
    st = L["left_shoulder_top"]
    pad = m("lathe", name="Pauldron", segments=32, profile=[[0, 0.11], [0.06, 0.105], [0.11, 0.08], [0.15, 0.03], [0.165, -0.01], [0.17, -0.02], [0, -0.02]])["id"]
    m("set_transform", id=pad, translation=[st[0] + 0.01, st[1] - 0.07, st[2]], rotation_degrees=[0, 0, -22])
    m("bevel", id=pad, amount=0.005)
    m("apply_smart_material", id=pad, preset="rusty_metal", wear=0.6, dirt=0.8, scale=0.05)
    return {"focus_point": L["head_center"], "focus_radius": 0.28}


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name in ONLY or ["sword", "axe", "barrel", "bottle"]:
        m = MM()
        m("set_lighting", preset="studio")
        detail = globals()[name]()
        m.shot(os.path.join(OUT, f"{name}.png"), views=["three_quarter"], size=640)
        if detail:
            frame = detail if isinstance(detail, dict) else {"focus": detail}
            m.shot(os.path.join(OUT, f"{name}-detail.png"), views=["front", "three_quarter"], size=480, **frame)
        print(f"{name}: {m.n - 1} tool calls")
