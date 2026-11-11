//! Export to glTF 2.0 binary (.glb), the format game engines import best.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::scene::Scene;

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// Serialize the whole scene as a .glb file. Each object becomes a node with
/// its transform, mesh and PBR material.
pub fn to_glb(scene: &Scene) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut meshes: Vec<Value> = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();

    let mut material_index: HashMap<u32, usize> = HashMap::new();
    let mut materials: Vec<Value> = Vec::new();
    for (id, m) in scene.materials() {
        material_index.insert(id, materials.len());
        materials.push(json!({
            "name": m.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": m.base_color,
                "metallicFactor": m.metallic,
                "roughnessFactor": m.roughness,
            },
            "emissiveFactor": m.emissive,
            "alphaMode": if m.base_color[3] < 1.0 { "BLEND" } else { "OPAQUE" },
        }));
    }

    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: u32| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        views.push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target }));
        bin.extend_from_slice(bytes);
        views.len() - 1
    };

    for obj in scene.objects() {
        let mesh = obj.mesh.to_mesh();
        if mesh.indices.is_empty() {
            continue;
        }
        let floats =
            |data: Vec<f32>| -> Vec<u8> { data.iter().flat_map(|f| f.to_le_bytes()).collect() };
        let pos = floats(mesh.positions.iter().flat_map(|p| p.to_array()).collect());
        let nrm = floats(mesh.normals.iter().flat_map(|p| p.to_array()).collect());
        let uv = floats(mesh.uvs.iter().flat_map(|p| p.to_array()).collect());
        let idx: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let bounds = mesh.bounds().expect("non-empty mesh");
        let count = mesh.positions.len();

        let base = accessors.len();
        let v = push_view(&mut bin, &pos, ARRAY_BUFFER);
        accessors.push(
            json!({ "bufferView": v, "componentType": FLOAT, "count": count, "type": "VEC3",
            "min": bounds.min.to_array(), "max": bounds.max.to_array() }),
        );
        let v = push_view(&mut bin, &nrm, ARRAY_BUFFER);
        accessors.push(
            json!({ "bufferView": v, "componentType": FLOAT, "count": count, "type": "VEC3" }),
        );
        let v = push_view(&mut bin, &uv, ARRAY_BUFFER);
        accessors.push(
            json!({ "bufferView": v, "componentType": FLOAT, "count": count, "type": "VEC2" }),
        );
        let v = push_view(&mut bin, &idx, ELEMENT_ARRAY_BUFFER);
        accessors.push(json!({ "bufferView": v, "componentType": UNSIGNED_INT, "count": mesh.indices.len(), "type": "SCALAR" }));

        meshes.push(json!({
            "name": obj.name,
            "primitives": [{
                "attributes": { "POSITION": base, "NORMAL": base + 1, "TEXCOORD_0": base + 2 },
                "indices": base + 3,
                "material": material_index.get(&obj.material).copied().unwrap_or(0),
            }]
        }));
        let t = &obj.transform;
        nodes.push(json!({
            "name": obj.name,
            "mesh": meshes.len() - 1,
            "translation": t.translation.to_array(),
            "rotation": t.rotation.to_array(),
            "scale": t.scale.to_array(),
        }));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let mut doc = json!({
        "asset": { "version": "2.0", "generator": concat!("MeshMind ", env!("CARGO_PKG_VERSION")) },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
    });
    if !bin.is_empty() {
        doc["buffers"] = json!([{ "byteLength": bin.len() }]);
    }

    let mut json_bytes = serde_json::to_vec(&doc).expect("serialize glTF JSON");
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut out = Vec::with_capacity(12 + 8 + json_bytes.len() + 8 + bin.len());
    let total = 12 + 8 + json_bytes.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::Material;
    use crate::primitives::{cube, uv_sphere};

    #[test]
    fn glb_loads_in_a_real_gltf_reader() {
        let mut scene = Scene::new();
        let a = scene.add_object("Box", cube(1.0)).unwrap();
        scene.add_object("Ball", uv_sphere(0.5, 16, 8)).unwrap();
        let red = scene.add_material(Material {
            name: "Red".into(),
            base_color: [1.0, 0.0, 0.0, 1.0],
            ..Default::default()
        });
        scene.assign_material(a, red).unwrap();

        let glb = to_glb(&scene);
        let (doc, buffers, _) = gltf::import_slice(&glb).expect("valid glb");
        assert_eq!(doc.nodes().count(), 2);
        let box_mesh = doc.meshes().next().unwrap();
        let prim = box_mesh.primitives().next().unwrap();
        assert_eq!(prim.material().name(), Some("Red"));
        let reader = prim.reader(|b| Some(&buffers[b.index()]));
        assert_eq!(reader.read_indices().unwrap().into_u32().count(), 36);
    }

    #[test]
    fn empty_scene_is_still_valid() {
        let glb = to_glb(&Scene::new());
        gltf::Gltf::from_slice(&glb).expect("valid glb");
    }
}
