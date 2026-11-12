use glam::{Affine3A, Quat, Vec3};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::lighting::LightingPreset;
use crate::material::{Material, MaterialId};
use crate::poly::PolyMesh;

pub type ObjectId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub fn matrix(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: String,
    pub transform: Transform,
    pub mesh: PolyMesh,
    pub material: MaterialId,
    /// For objects made from blended volumes (sculpt_body, create_character):
    /// the parts, so the body can be adjusted and rebuilt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<crate::sdf::SdfPart>>,
    /// Detail baked from a denser mesh (normal map and ambient occlusion),
    /// laid out on this mesh's UVs. Cleared when the mesh changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<std::sync::Arc<crate::bake::DetailMaps>>,
    /// Lighter versions for distant views (LOD1, LOD2, ...), sharing this
    /// mesh's UVs and material. Exported as `<name>_LOD1` and so on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lods: Vec<PolyMesh>,
}

#[derive(Debug, Error, PartialEq)]
pub enum SceneError {
    #[error("no object with id {0}")]
    NoSuchObject(ObjectId),
    #[error("no material with id {0}")]
    NoSuchMaterial(MaterialId),
    #[error("invalid mesh: {0}")]
    InvalidMesh(String),
}

/// The document an agent edits. Ids are never reused within a scene.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    objects: Vec<Object>,
    materials: Vec<(MaterialId, Material)>,
    next_object_id: ObjectId,
    next_material_id: MaterialId,
    /// How the scene is lit in previews and the viewer.
    #[serde(default)]
    pub lighting: LightingPreset,
    /// Reference images (concept art) to compare the model against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<crate::reference::Reference>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            materials: vec![(0, Material::default())],
            next_object_id: 1,
            next_material_id: 1,
            lighting: LightingPreset::default(),
            references: Vec::new(),
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Id of the built-in default material.
    pub const DEFAULT_MATERIAL: MaterialId = 0;

    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    pub fn object(&self, id: ObjectId) -> Result<&Object, SceneError> {
        self.objects
            .iter()
            .find(|o| o.id == id)
            .ok_or(SceneError::NoSuchObject(id))
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Result<&mut Object, SceneError> {
        self.objects
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or(SceneError::NoSuchObject(id))
    }

    pub fn add_object(
        &mut self,
        name: impl Into<String>,
        mesh: PolyMesh,
    ) -> Result<ObjectId, SceneError> {
        mesh.validate().map_err(SceneError::InvalidMesh)?;
        let id = self.next_object_id;
        self.next_object_id += 1;
        self.objects.push(Object {
            id,
            name: name.into(),
            transform: Transform::default(),
            mesh,
            material: Self::DEFAULT_MATERIAL,
            body: None,
            detail: None,
            lods: Vec::new(),
        });
        Ok(id)
    }

    /// Replace an object's mesh, e.g. with the result of a modelling operation.
    pub fn set_mesh(&mut self, id: ObjectId, mesh: PolyMesh) -> Result<(), SceneError> {
        mesh.validate().map_err(SceneError::InvalidMesh)?;
        let obj = self.object_mut(id)?;
        if obj.mesh != mesh {
            // Baked detail and LODs belong to the old shape.
            obj.detail = None;
            obj.lods.clear();
        }
        obj.mesh = mesh;
        Ok(())
    }

    pub fn duplicate_object(&mut self, id: ObjectId) -> Result<ObjectId, SceneError> {
        let mut copy = self.object(id)?.clone();
        copy.id = self.next_object_id;
        copy.name = format!("{} copy", copy.name);
        self.next_object_id += 1;
        let new_id = copy.id;
        self.objects.push(copy);
        Ok(new_id)
    }

    pub fn remove_object(&mut self, id: ObjectId) -> Result<Object, SceneError> {
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or(SceneError::NoSuchObject(id))?;
        Ok(self.objects.remove(idx))
    }

    pub fn materials(&self) -> impl Iterator<Item = (MaterialId, &Material)> {
        self.materials.iter().map(|(id, m)| (*id, m))
    }

    pub fn material(&self, id: MaterialId) -> Result<&Material, SceneError> {
        self.materials
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, m)| m)
            .ok_or(SceneError::NoSuchMaterial(id))
    }

    pub fn add_material(&mut self, material: Material) -> MaterialId {
        let id = self.next_material_id;
        self.next_material_id += 1;
        self.materials.push((id, material.sanitized()));
        id
    }

    pub fn assign_material(
        &mut self,
        object: ObjectId,
        material: MaterialId,
    ) -> Result<(), SceneError> {
        self.material(material)?;
        self.object_mut(object)?.material = material;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::cube;

    #[test]
    fn add_find_remove() {
        let mut s = Scene::new();
        let a = s.add_object("A", cube(1.0)).unwrap();
        let b = s.add_object("B", cube(1.0)).unwrap();
        assert_ne!(a, b);
        assert_eq!(s.object(b).unwrap().name, "B");
        s.remove_object(a).unwrap();
        assert_eq!(s.object(a), Err(SceneError::NoSuchObject(a)));
        // Ids are not reused.
        let c = s.add_object("C", cube(1.0)).unwrap();
        assert!(c > b);
    }

    #[test]
    fn rejects_invalid_mesh() {
        let mut s = Scene::new();
        let mut bad = PolyMesh::default();
        bad.add_vertex(Vec3::ZERO);
        bad.add_face(vec![0, 1, 2], false);
        assert!(matches!(
            s.add_object("bad", bad),
            Err(SceneError::InvalidMesh(_))
        ));
    }

    #[test]
    fn materials_are_clamped_and_assignable() {
        let mut s = Scene::new();
        let o = s.add_object("A", cube(1.0)).unwrap();
        let m = s.add_material(Material {
            roughness: 3.0,
            ..Default::default()
        });
        assert_eq!(s.material(m).unwrap().roughness, 1.0);
        s.assign_material(o, m).unwrap();
        assert_eq!(s.object(o).unwrap().material, m);
        assert_eq!(
            s.assign_material(o, 99),
            Err(SceneError::NoSuchMaterial(99))
        );
    }
}
