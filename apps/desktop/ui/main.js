import * as THREE from "./vendor/three.module.min.js";
import { OrbitControls } from "./vendor/OrbitControls.js";
import { RoomEnvironment } from "./vendor/RoomEnvironment.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// ---------- Renderer ----------
const canvas = document.getElementById("canvas");
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
renderer.setPixelRatio(window.devicePixelRatio);
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x15151c);
const pmrem = new THREE.PMREMGenerator(renderer);
scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
scene.environmentIntensity = 0.8;

const camera = new THREE.PerspectiveCamera(40, 1, 0.01, 1000);
camera.position.set(3, 2.4, 4);
const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.addEventListener("start", () => (userMovedCamera = true));

const sun = new THREE.DirectionalLight(0xffffff, 2.2);
sun.position.set(4, 8, 5);
sun.castShadow = true;
sun.shadow.mapSize.set(2048, 2048);
sun.shadow.bias = -0.0005;
scene.add(sun, sun.target);

const ground = new THREE.Mesh(new THREE.PlaneGeometry(40, 40), new THREE.ShadowMaterial({ opacity: 0.35 }));
ground.rotation.x = -Math.PI / 2;
ground.receiveShadow = true;
scene.add(ground);
const grid = new THREE.GridHelper(20, 40, 0x3a3a4a, 0x26262f);
grid.position.y = 0.0005;
scene.add(grid);

const models = new THREE.Group();
scene.add(models);

let wireframe = false;
let selectedId = null;
// Keep the whole model in view until the person moves the camera themselves.
let userMovedCamera = false;
let lastCount = 0;

function resize() {
  const { clientWidth: w, clientHeight: h } = canvas.parentElement;
  renderer.setSize(w, h, false);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
}
new ResizeObserver(resize).observe(canvas.parentElement);
resize();

renderer.setAnimationLoop(() => {
  controls.update();
  renderer.render(scene, camera);
});

// ---------- Scene sync ----------
function buildObject(o) {
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(o.positions, 3));
  g.setAttribute("normal", new THREE.Float32BufferAttribute(o.normals, 3));
  g.setIndex(o.indices);
  const m = o.material ?? {};
  const [r, gr, b, a] = m.base_color ?? [0.8, 0.8, 0.8, 1];
  const mat = new THREE.MeshStandardMaterial({
    color: new THREE.Color(r, gr, b), // linear values, as stored by the engine
    metalness: m.metallic ?? 0,
    roughness: m.roughness ?? 0.5,
    emissive: new THREE.Color(...(m.emissive ?? [0, 0, 0])),
    transparent: a < 1,
    opacity: a,
    side: THREE.DoubleSide,
    wireframe,
  });
  if (o.id === selectedId) mat.emissive.add(new THREE.Color(0.06, 0.04, 0.15));
  const mesh = new THREE.Mesh(g, mat);
  mesh.castShadow = mesh.receiveShadow = true;
  mesh.position.fromArray(o.translation);
  mesh.quaternion.fromArray(o.rotation);
  mesh.scale.fromArray(o.scale);
  mesh.userData.id = o.id;
  return mesh;
}

function frameAll() {
  const box = new THREE.Box3().setFromObject(models);
  if (box.isEmpty()) return;
  const sphere = box.getBoundingSphere(new THREE.Sphere());
  const dist = sphere.radius / Math.sin(THREE.MathUtils.degToRad(camera.fov / 2)) * 1.1;
  const dir = camera.position.clone().sub(controls.target).normalize();
  controls.target.copy(sphere.center);
  camera.position.copy(sphere.center).addScaledVector(dir, dist);
  // Keep shadows covering the model.
  const s = Math.max(sphere.radius * 2, 2);
  Object.assign(sun.shadow.camera, { left: -s, right: s, top: s, bottom: -s, near: 0.1, far: 50 });
  sun.target.position.copy(sphere.center);
  sun.position.copy(sphere.center).add(new THREE.Vector3(4, 8, 5));
  sun.shadow.camera.updateProjectionMatrix();
}

let refreshing = false;
let pending = false;
async function refresh() {
  if (refreshing) { pending = true; return; }
  refreshing = true;
  try {
    const snap = await invoke("scene_snapshot");
    for (const child of [...models.children]) {
      child.geometry.dispose();
      child.material.dispose();
      models.remove(child);
    }
    for (const o of snap.objects) models.add(buildObject(o));
    renderList(snap.objects);
    const faces = snap.objects.reduce((n, o) => n + o.faces, 0);
    document.getElementById("stats").textContent =
      snap.objects.length ? `${snap.objects.length} objects · ${faces.toLocaleString()} faces` : "";
    document.getElementById("welcome").hidden = snap.objects.length > 0;
    if (snap.objects.length && (!userMovedCamera || lastCount === 0)) frameAll();
    if (!snap.objects.length) userMovedCamera = false;
    lastCount = snap.objects.length;
  } catch (e) {
    showError(e);
  } finally {
    refreshing = false;
    if (pending) { pending = false; refresh(); }
  }
}

function renderList(objects) {
  const list = document.getElementById("objects");
  list.innerHTML = "";
  if (!objects.length) {
    list.innerHTML = '<li class="muted">Empty. Waiting for your agent…</li>';
    return;
  }
  for (const o of objects) {
    const li = document.createElement("li");
    li.className = o.id === selectedId ? "selected" : "";
    const name = document.createElement("span");
    name.textContent = o.name;
    const info = document.createElement("span");
    info.className = "muted";
    info.textContent = `#${o.id} · ${o.faces} faces`;
    li.append(name, info);
    li.onclick = () => { selectedId = selectedId === o.id ? null : o.id; refresh(); };
    list.appendChild(li);
  }
}

function showError(e) {
  const el = document.getElementById("error");
  el.textContent = String(e);
  el.hidden = false;
}

// ---------- Controls ----------
let rpcId = 1;
async function tool(name, args = {}) {
  const reply = await invoke("mcp", { message: { jsonrpc: "2.0", id: rpcId++, method: "tools/call", params: { name, arguments: args } } });
  if (reply.error) throw new Error(reply.error.message);
  return reply.result;
}

document.getElementById("undo").onclick = () => tool("undo");
document.getElementById("redo").onclick = () => tool("redo");
document.getElementById("frame").onclick = () => { userMovedCamera = false; frameAll(); };
const wireBtn = document.getElementById("wire");
wireBtn.onclick = () => {
  wireframe = !wireframe;
  wireBtn.classList.toggle("on", wireframe);
  models.children.forEach((m) => (m.material.wireframe = wireframe));
};
window.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
    e.preventDefault();
    tool(e.shiftKey ? "redo" : "undo");
  } else if (e.key === "f") frameAll();
  else if (e.key === "w") wireBtn.click();
});

// ---------- Startup ----------
const info = await invoke("connection_info");
document.getElementById("version").textContent = `v${info.version}`;
const endpoint = document.getElementById("endpoint");
endpoint.textContent = info.url;
document.getElementById("cmd").textContent = info.claude_command;
if (info.error) {
  showError(info.error);
} else {
  endpoint.classList.add("live");
}
document.getElementById("copy-cmd").onclick = async () => {
  await navigator.clipboard.writeText(info.claude_command);
  const b = document.getElementById("copy-cmd");
  b.textContent = "Copied";
  setTimeout(() => (b.textContent = "Copy Claude Code command"), 1500);
};

await listen("scene-changed", () => refresh());
refresh();
