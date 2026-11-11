const { invoke } = window.__TAURI__.core;
let nextId = 1;

async function mcp(method, params = {}) {
  const reply = await invoke("mcp", { message: { jsonrpc: "2.0", id: nextId++, method, params } });
  if (reply.error) throw new Error(reply.error.message);
  return reply.result;
}

async function callTool(name, args = {}) {
  const result = await mcp("tools/call", { name, arguments: args });
  if (result.isError) throw new Error(result.content[0].text);
  return result.structuredContent;
}

async function refresh() {
  const { objects } = await callTool("list_objects");
  const list = document.getElementById("objects");
  list.innerHTML = "";
  if (objects.length === 0) list.innerHTML = '<li class="muted">Empty</li>';
  for (const o of objects) {
    const li = document.createElement("li");
    li.textContent = `${o.name} · ${o.triangles} tris`;
    list.appendChild(li);
  }
}

document.getElementById("demo").addEventListener("click", async () => {
  await callTool("create_primitive", { shape: "cube" });
  await refresh();
});

invoke("engine_info").then((info) => {
  document.getElementById("version").textContent = `v${info.version}`;
});
mcp("initialize").then(refresh);
