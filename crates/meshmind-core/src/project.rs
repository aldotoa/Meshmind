//! MeshMind project files (`.meshmind`): the whole scene, including baked
//! textures, as JSON. Unlike a .glb export, a project keeps the editable
//! n-gon meshes, so work can continue where it stopped.

use serde::{Deserialize, Serialize};

use crate::scene::Scene;

pub const FORMAT: &str = "meshmind";
pub const VERSION: u32 = 1;

#[derive(Serialize)]
struct Out<'a> {
    format: &'static str,
    version: u32,
    generator: &'static str,
    scene: &'a Scene,
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    format: String,
    #[serde(default)]
    version: u32,
}

#[derive(Deserialize)]
struct In {
    scene: Scene,
}

pub fn to_bytes(scene: &Scene) -> Vec<u8> {
    serde_json::to_vec(&Out {
        format: FORMAT,
        version: VERSION,
        generator: concat!("MeshMind ", env!("CARGO_PKG_VERSION")),
        scene,
    })
    .expect("scene serializes")
}

pub fn from_bytes(bytes: &[u8]) -> Result<Scene, String> {
    let bad = |e: serde_json::Error| format!("not a MeshMind project: {e}");
    let header: Header = serde_json::from_slice(bytes).map_err(bad)?;
    if header.format != FORMAT {
        return Err("not a MeshMind project".into());
    }
    if header.version > VERSION {
        return Err(format!(
            "this project was saved by a newer MeshMind (file version {}, supported {VERSION}); please update",
            header.version
        ));
    }
    let file: In = serde_json::from_slice(bytes).map_err(bad)?;
    for obj in file.scene.objects() {
        obj.mesh
            .validate()
            .map_err(|e| format!("object {:?} is damaged: {e}", obj.name))?;
    }
    Ok(file.scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::cube;
    use crate::smart::{bake, Preset, SmartMaterial};
    use crate::{uv, Material, MaterialTextures};
    use std::sync::Arc;

    #[test]
    fn round_trip_keeps_meshes_materials_and_textures() {
        let mut scene = Scene::new();
        let mut mesh = cube(1.0);
        uv::unwrap(&mut mesh, &Default::default()).unwrap();
        let baked = bake(&mesh, &SmartMaterial::new(Preset::PaintedMetal), 64).unwrap();
        let id = scene.add_object("Crate", mesh).unwrap();
        let m = scene.add_material(Material {
            name: "Paint".into(),
            textures: Some(Arc::new(MaterialTextures {
                base_color: baked.base_color,
                metallic_roughness: baked.metallic_roughness,
                normal: baked.normal,
            })),
            ..Default::default()
        });
        scene.assign_material(id, m).unwrap();
        scene.lighting = crate::LightingPreset::Sunset;

        let loaded = from_bytes(&to_bytes(&scene)).unwrap();
        assert_eq!(loaded.objects(), scene.objects());
        assert_eq!(loaded.lighting, scene.lighting);
        let (_, a) = scene.materials().find(|(i, _)| *i == m).unwrap();
        let (_, b) = loaded.materials().find(|(i, _)| *i == m).unwrap();
        assert_eq!(a, b, "textures survive the PNG round trip exactly");
        // New ids keep counting up after loading.
        let mut loaded = loaded;
        assert!(loaded.add_object("Next", cube(1.0)).unwrap() > id);
    }

    #[test]
    fn rejects_other_files() {
        assert!(from_bytes(b"{}").is_err());
        assert!(
            from_bytes(br#"{"format":"meshmind","version":99,"scene":{}}"#)
                .unwrap_err()
                .contains("newer")
        );
    }
}
