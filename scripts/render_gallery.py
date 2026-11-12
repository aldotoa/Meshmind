"""Build the README gallery by driving meshmind-mcp exactly as an agent would.

    cargo build --release -p meshmind-mcp
    python3 scripts/render_gallery.py [output-dir]

Each step is an ordinary MCP tool call over stdio, so this doubles as an
example of scripting MeshMind without an AI.
"""
import json, subprocess, sys, base64, os
BIN = os.environ.get("MESHMIND_MCP", os.path.join("target", "release", "meshmind-mcp"))
OUT = sys.argv[1] if len(sys.argv) > 1 else "gallery"
os.makedirs(OUT, exist_ok=True)
p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
n = 0
def rpc(method, params):
    global n; n += 1
    p.stdin.write(json.dumps({"jsonrpc":"2.0","id":n,"method":method,"params":params})+"\n"); p.stdin.flush()
    return json.loads(p.stdout.readline())
def call(tool_name, **args):
    r = rpc("tools/call", {"name": tool_name, "arguments": args})["result"]
    if r["isError"]: print("ERROR", tool_name, r["content"][0]["text"]); sys.exit(1)
    return r
def sc(tool_name, **a): return call(tool_name, **a).get("structuredContent", {})
def shot(fname, views=["three_quarter"], size=900, lighting="studio"):
    sc("set_lighting", preset=lighting)
    r = call("render_preview", views=views, size=size)
    img = next(c for c in r["content"] if c["type"] == "image")
    open(os.path.join(OUT, fname), "wb").write(base64.b64decode(img["data"]))
rpc("initialize", {})

def crate(x=0, z=0):
    c = sc("create_primitive", shape="cube", name="Supply crate")["id"]
    sc("bevel", id=c, amount=0.04, segments=3)
    for d in ["+x", "-x", "+z", "-z", "+y"]:
        inner = sc("inset", id=c, faces={"facing": d, "max_angle": 2}, amount=0.16)["inner_faces"]
        sc("extrude", id=c, faces=inner, distance=-0.03)
    sc("set_transform", id=c, translation=[x, 0.5, z])
    sc("apply_smart_material", id=c, preset="painted_metal", color_hex="#4b5a22", wear=0.5, dirt=0.5, resolution=1024)
def barrel(x=0, z=0):
    b = sc("create_primitive", shape="cylinder", name="Oil barrel", radius=0.3, height=0.9, segments=48)["id"]
    sc("bevel", id=b, amount=0.015, segments=2)
    for y in [0.28, -0.28]:
        band = sc("create_primitive", shape="torus", radius=0.3, tube_radius=0.022, segments=48, rings=8)["id"]
        sc("set_transform", id=band, translation=[0, y, 0])
        sc("boolean", target=b, tool=band, operation="union")
    cap = sc("create_primitive", shape="cylinder", radius=0.05, height=0.06, segments=16)["id"]
    sc("set_transform", id=cap, translation=[0.15, 0.47, 0.05])
    sc("boolean", target=b, tool=cap, operation="union")
    sc("set_transform", id=b, translation=[x, 0.45, z])
    sc("apply_smart_material", id=b, preset="rusty_metal", color_hex="#7a2418", wear=0.55, dirt=0.6, resolution=1024)
def stool(x=0, z=0):
    seat = sc("create_primitive", shape="cylinder", name="Stool seat", radius=0.22, height=0.05, segments=40)["id"]
    sc("bevel", id=seat, amount=0.012, segments=3)
    sc("set_transform", id=seat, translation=[x, 0.47, z])
    sc("apply_smart_material", id=seat, preset="wood", color_hex="#8a5a32", wear=0.2, resolution=1024)
    for i, (dx, dz) in enumerate([(1,1),(1,-1),(-1,1),(-1,-1)]):
        leg = sc("create_primitive", shape="cylinder", name=f"Stool leg {i+1}", radius=0.022, height=0.46, segments=16)["id"]
        sc("set_transform", id=leg, translation=[x+dx*0.13, 0.225, z+dz*0.13], rotation_degrees=[-dz*7, 0, dx*7])
        sc("apply_smart_material", id=leg, preset="wood", color_hex="#7a4e2b", wear=0.2, resolution=512)
def pedestal(x=0, z=0):
    base = sc("create_primitive", shape="cube", name="Pedestal base", size=1)["id"]
    sc("bevel", id=base, amount=0.03, segments=2)
    sc("set_transform", id=base, translation=[x, 0.08, z], scale=[0.6, 0.16, 0.6])
    col = sc("create_primitive", shape="cylinder", name="Pedestal column", radius=0.2, height=0.7, segments=8)["id"]
    sc("bevel", id=col, amount=0.02, segments=2)
    sc("set_transform", id=col, translation=[x, 0.51, z])
    top = sc("create_primitive", shape="cube", name="Pedestal top", size=1)["id"]
    sc("bevel", id=top, amount=0.03, segments=2)
    sc("set_transform", id=top, translation=[x, 0.92, z], scale=[0.55, 0.12, 0.55])
    for o in [base, col, top]:
        sc("apply_smart_material", id=o, preset="stone", color_hex="#6e6a64", wear=0.6, dirt=0.5, resolution=1024)

for name, fn, light in [("crate", crate, "studio"), ("barrel", barrel, "sunset"), ("stool", stool, "outdoor"), ("pedestal", pedestal, "studio")]:
    sc("clear_scene"); fn(); shot(f"{name}.png", lighting=light)
sc("clear_scene")
crate(0, 0); barrel(1.0, 0.4); stool(-0.55, 0.95); pedestal(-1.15, -0.35)
shot("hero.png", size=1100, lighting="studio")
print("ok", n, "messages")
