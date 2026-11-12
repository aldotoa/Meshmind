import * as THREE from "./vendor/three.module.min.js";
import { OrbitControls } from "./vendor/OrbitControls.js";

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
const pmrem = new THREE.PMREMGenerator(renderer);
scene.environment = pmrem.fromScene(studioEnvironment(), 0.02).texture;
scene.environmentIntensity = 0.8;

// A photo studio for reflections: a dark room with soft boxes (a big key up
// and to the left, a tall strip on the right, one overhead) and a bright
// band behind the camera. Polished metal and glass read as such because they
// reflect these shapes, the same way the agent's preview renderer lights them.
function studioEnvironment() {
  const env = new THREE.Scene();
  const room = new THREE.Mesh(
    new THREE.BoxGeometry(20, 12, 20),
    new THREE.MeshBasicMaterial({ color: 0x0c0c10, side: THREE.BackSide }),
  );
  room.position.y = 4;
  env.add(room);
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(20, 20), new THREE.MeshBasicMaterial({ color: 0x1c1c22 }));
  floor.rotation.x = -Math.PI / 2;
  floor.position.y = -1.99;
  env.add(floor);
  const panel = (w, h, brightness, pos) => {
    const m = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));
    m.material.color.setScalar(brightness);
    m.position.set(...pos);
    m.lookAt(0, 0, 0);
    env.add(m);
  };
  panel(4.5, 3, 7, [-5, 5, 5]); // key
  panel(0.8, 7, 5, [7, 1.5, 1]); // strip
  panel(7, 0.8, 4, [0, 8, -1]); // top
  panel(14, 1, 1.6, [0, 1, 9]); // band behind the camera
  panel(1.2, 4, 2, [-6, 1, -6]); // back rim
  // A soft bright band all round the horizon, like a studio's curved wall:
  // long flat surfaces such as blades reflect it as a gradient.
  const band = new THREE.Mesh(
    new THREE.CylinderGeometry(9, 9, 2.5, 48, 1, true),
    new THREE.MeshBasicMaterial({ side: THREE.BackSide }),
  );
  band.material.color.setScalar(0.9);
  env.add(band);
  return env;
}

// Background: a soft vertical gradient with a light vignette, from the preset.
const bgCanvas = document.createElement("canvas");
bgCanvas.width = 16;
bgCanvas.height = 256;
const background = new THREE.CanvasTexture(bgCanvas);
background.colorSpace = THREE.SRGBColorSpace;
scene.background = background;
function paintBackground(top, bottom) {
  const ctx = bgCanvas.getContext("2d");
  const g = ctx.createLinearGradient(0, 0, 0, bgCanvas.height);
  g.addColorStop(0, top.getStyle());
  g.addColorStop(0.55, top.clone().lerp(bottom, 0.5).getStyle());
  g.addColorStop(1, bottom.getStyle());
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, bgCanvas.width, bgCanvas.height);
  background.needsUpdate = true;
}
paintBackground(new THREE.Color(0.1, 0.105, 0.125), new THREE.Color(0.03, 0.03, 0.036));

const camera = new THREE.PerspectiveCamera(40, 1, 0.01, 1000);
camera.position.set(3, 2.4, 4);
const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.addEventListener("start", () => (userMovedCamera = true));

const sun = new THREE.DirectionalLight(0xffffff, 2.2);
sun.position.set(4, 8, 5);
sun.castShadow = true;
sun.shadow.mapSize.set(4096, 4096);
sun.shadow.bias = -0.0002;
sun.shadow.normalBias = 0.01;
sun.shadow.radius = 4;
scene.add(sun, sun.target);

const hemi = new THREE.HemisphereLight(0xffffff, 0x444444, 0.4);
scene.add(hemi);

// Apply a lighting preset from the engine (same values the preview renderer uses).
let currentLighting = null;
function applyLighting(lighting) {
  if (!lighting || lighting.name === currentLighting) return;
  currentLighting = lighting.name;
  const p = lighting.params;
  const col = (c) => new THREE.Color(c[0], c[1], c[2]);
  hemi.color = col(p.sky);
  hemi.groundColor = col(p.ground);
  hemi.intensity = p.ambient_intensity * 1.2;
  sun.color = col(p.key_color);
  sun.intensity = p.key_intensity * 1.2;
  const d = new THREE.Vector3(...p.key_direction).normalize();
  sunOffset.set(d.x * 8, Math.max(d.y, 0.15) * 10, d.z * 8);
  scene.environmentIntensity = 0.35 + p.ambient_intensity;
  paintBackground(col(p.background_top), col(p.background_bottom));
  renderer.toneMappingExposure = p.exposure;
  document.getElementById("lighting").value = lighting.name;
  fitStage();
}
const sunOffset = new THREE.Vector3(4, 8, 5);

const ground = new THREE.Mesh(new THREE.PlaneGeometry(40, 40), new THREE.ShadowMaterial({ opacity: 0.35 }));
ground.rotation.x = -Math.PI / 2;
ground.receiveShadow = true;
scene.add(ground);
const grid = new THREE.GridHelper(20, 40, 0x3a3a4a, 0x26262f);
grid.position.y = 0.0005;
grid.material.transparent = true;
grid.material.opacity = 0.45;
grid.material.depthWrite = false;
scene.add(grid);

const models = new THREE.Group();
scene.add(models);

// ---------- Contact shadow ----------
// A soft, dark patch where objects touch or come close to the floor, which is
// what makes them sit on it instead of floating. The scene is rendered from
// below as depth (nearer = darker), blurred, and laid on the floor. It only
// changes when the scene does, so it costs nothing per frame.
const CONTACT_RES = 512;
const contactTarget = new THREE.WebGLRenderTarget(CONTACT_RES, CONTACT_RES);
const contactBlurTarget = new THREE.WebGLRenderTarget(CONTACT_RES, CONTACT_RES);
contactTarget.texture.generateMipmaps = false;
contactBlurTarget.texture.generateMipmaps = false;
const contactPlane = new THREE.Mesh(
  new THREE.PlaneGeometry(1, 1).rotateX(Math.PI / 2),
  new THREE.MeshBasicMaterial({ map: contactTarget.texture, transparent: true, depthWrite: false }),
);
contactPlane.renderOrder = 1;
const contactCamera = new THREE.OrthographicCamera(-0.5, 0.5, 0.5, -0.5, 0, 1);
contactCamera.rotation.x = Math.PI / 2; // look up from the floor
const contactGroup = new THREE.Group();
contactGroup.add(contactPlane, contactCamera);
scene.add(contactGroup);
const contactDepth = new THREE.MeshDepthMaterial();
contactDepth.depthTest = false;
contactDepth.depthWrite = false;
contactDepth.onBeforeCompile = (shader) => {
  shader.fragmentShader = shader.fragmentShader.replace(
    "gl_FragColor = vec4( vec3( 1.0 - fragCoordZ ), opacity );",
    "gl_FragColor = vec4( vec3( 0.0 ), pow( 1.0 - fragCoordZ, 2.0 ) * 0.9 );",
  );
};
const blurMaterial = new THREE.ShaderMaterial({
  uniforms: { tDiffuse: { value: null }, step: { value: new THREE.Vector2() } },
  vertexShader: "varying vec2 vUv; void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }",
  fragmentShader: `uniform sampler2D tDiffuse; uniform vec2 step; varying vec2 vUv;
    void main() {
      vec4 sum = texture2D(tDiffuse, vUv) * 0.2270270270;
      sum += texture2D(tDiffuse, vUv + step * 1.3846153846) * 0.3162162162;
      sum += texture2D(tDiffuse, vUv - step * 1.3846153846) * 0.3162162162;
      sum += texture2D(tDiffuse, vUv + step * 3.2307692308) * 0.0702702703;
      sum += texture2D(tDiffuse, vUv - step * 3.2307692308) * 0.0702702703;
      gl_FragColor = sum;
    }`,
  depthTest: false,
  depthWrite: false,
});
const blurQuad = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), blurMaterial);
blurQuad.frustumCulled = false;
const blurScene = new THREE.Scene();
blurScene.add(blurQuad);
const blurCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);

function blurContact(amount) {
  for (const [from, to, dx, dy] of [
    [contactTarget, contactBlurTarget, amount, 0],
    [contactBlurTarget, contactTarget, 0, amount],
  ]) {
    blurMaterial.uniforms.tDiffuse.value = from.texture;
    blurMaterial.uniforms.step.value.set(dx / CONTACT_RES, dy / CONTACT_RES);
    renderer.setRenderTarget(to);
    renderer.render(blurScene, blurCamera);
  }
}

function updateContactShadow(box, floor) {
  const size = box.getSize(new THREE.Vector3());
  const span = Math.max(size.x, size.z, size.y * 0.5) * 1.6 + 0.05;
  const reach = Math.max(size.y * 0.35, span * 0.15);
  contactGroup.position.set((box.min.x + box.max.x) / 2, floor + 0.0008, (box.min.z + box.max.z) / 2);
  contactPlane.scale.set(span, -1, span);
  Object.assign(contactCamera, { left: -span / 2, right: span / 2, top: span / 2, bottom: -span / 2, near: 0, far: reach });
  contactCamera.updateProjectionMatrix();

  const hidden = [contactPlane, ground, grid];
  hidden.forEach((o) => (o.visible = false));
  const background = scene.background;
  scene.background = null;
  scene.overrideMaterial = contactDepth;
  const clear = renderer.getClearColor(new THREE.Color());
  const clearAlpha = renderer.getClearAlpha();
  renderer.setClearColor(0x000000, 0);
  renderer.setRenderTarget(contactTarget);
  renderer.clear();
  renderer.render(scene, contactCamera);
  scene.overrideMaterial = null;
  blurContact(2.5);
  blurContact(1.2);
  renderer.setRenderTarget(null);
  renderer.setClearColor(clear, clearAlpha);
  scene.background = background;
  hidden.forEach((o) => (o.visible = true));
}

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
// Baked textures, cached by material id (a material's textures never change).
const textureCache = new Map();
const loader = new THREE.TextureLoader();
function loadTex(url, srgb) {
  const t = loader.load(url);
  t.flipY = false; // glTF convention: v = 0 is the top row
  t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
  t.anisotropy = 8;
  return t;
}
async function texturesFor(materialId) {
  if (!textureCache.has(materialId)) {
    textureCache.set(materialId, invoke("material_textures", { materialId }).then((urls) =>
      urls && {
        map: loadTex(urls.base_color, true),
        roughnessMap: loadTex(urls.metallic_roughness, false),
        metalnessMap: loadTex(urls.metallic_roughness, false),
        normalMap: loadTex(urls.normal, false),
      }));
  }
  return textureCache.get(materialId);
}
async function buildObject(o) {
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(o.positions, 3));
  g.setAttribute("normal", new THREE.Float32BufferAttribute(o.normals, 3));
  g.setAttribute("uv", new THREE.Float32BufferAttribute(o.uvs, 2));
  g.setIndex(o.indices);
  const m = o.material ?? {};
  const [r, gr, b, a] = m.base_color ?? [0.8, 0.8, 0.8, 1];
  const params = {
    color: new THREE.Color(r, gr, b), // linear values, as stored by the engine
    metalness: m.metallic ?? 0,
    roughness: m.roughness ?? 0.5,
    emissive: new THREE.Color(...(m.emissive ?? [0, 0, 0])),
    transparent: a < 1,
    opacity: a,
    side: THREE.DoubleSide,
    wireframe,
  };
  // See-through materials (glass, gems) use three.js's physical transmission.
  const clear = (m.transmission ?? 0) > 0;
  const mat = clear
    ? new THREE.MeshPhysicalMaterial({ ...params, transmission: m.transmission, ior: m.ior ?? 1.5, thickness: 0.02 })
    : new THREE.MeshStandardMaterial(params);
  if (o.textured) {
    const tex = await texturesFor(o.material_id);
    if (tex) {
      Object.assign(mat, tex);
      // glTF normal maps have +Y up in UV space, the reverse of three.js's default.
      mat.normalScale = new THREE.Vector2(1, -1);
      mat.side = THREE.FrontSide;
      mat.needsUpdate = true;
    }
  }
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
}

// Keep the floor under the model and the shadow camera around it. The floor
// is y = 0, but anything that dips below (a primitive still centred on the
// origin, say) pushes it down, so the floor never cuts through an object and
// its shadow and grid never draw across the model.
function fitStage() {
  const box = new THREE.Box3().setFromObject(models);
  if (box.isEmpty()) {
    contactPlane.visible = false;
    ground.position.y = 0;
    grid.position.y = 0.0005;
    return;
  }
  const floor = Math.min(0, box.min.y);
  ground.position.y = floor;
  grid.position.y = floor + 0.0005;
  box.expandByPoint(new THREE.Vector3(box.min.x, floor, box.min.z));
  const sphere = box.getBoundingSphere(new THREE.Sphere());
  const s = Math.max(sphere.radius * 2, 2);
  const reach = sphere.radius * 3 + 5;
  Object.assign(sun.shadow.camera, { left: -s, right: s, top: s, bottom: -s, near: 0.1, far: reach * 2 });
  sun.target.position.copy(sphere.center);
  sun.position.copy(sphere.center).add(sunOffset.clone().normalize().multiplyScalar(reach));
  sun.shadow.camera.updateProjectionMatrix();
  updateContactShadow(box, floor);
  camera.near = Math.max(sphere.radius * 0.005, 0.001);
  camera.far = Math.max(sphere.radius * 60, 50);
  camera.updateProjectionMatrix();
}

let refreshing = false;
let pending = false;
async function refresh() {
  if (refreshing) { pending = true; return; }
  refreshing = true;
  try {
    const snap = await invoke("scene_snapshot");
    const built = await Promise.all(snap.objects.map(buildObject));
    for (const child of [...models.children]) {
      child.geometry.dispose();
      child.material.dispose(); // textures stay cached
      models.remove(child);
    }
    for (const m of built) models.add(m);
    applyLighting(snap.lighting);
    fitStage();
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
document.getElementById("lighting").onchange = (e) => tool("set_lighting", { preset: e.target.value });
document.getElementById("frame").onclick = () => { userMovedCamera = false; frameAll(); };
const wireBtn = document.getElementById("wire");
wireBtn.onclick = () => {
  wireframe = !wireframe;
  wireBtn.classList.toggle("on", wireframe);
  models.children.forEach((m) => (m.material.wireframe = wireframe));
};
const spinBtn = document.getElementById("spin");
spinBtn.onclick = () => {
  controls.autoRotate = !controls.autoRotate;
  controls.autoRotateSpeed = 1.5;
  spinBtn.classList.toggle("on", controls.autoRotate);
};

window.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
    e.preventDefault();
    tool(e.shiftKey ? "redo" : "undo");
  } else if (e.key === "f") frameAll();
  else if (e.key === "w") wireBtn.click();
  else if (e.key === "t") spinBtn.click();
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

document.getElementById("restored").hidden = !info.restored;
document.getElementById("new-scene").onclick = () => tool("clear_scene");

await listen("scene-changed", () => refresh());
refresh();
