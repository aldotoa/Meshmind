use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::material::{Material, MaterialId};
use crate::mesh::Mesh;

pub type ObjectId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
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
    pub mesh: Mesh,
    pub material: MaterialId,
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
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            materials: vec![(0, Material::default())],
            next_object_id: 1,
            next_material_id: 1,
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
        mesh: Mesh,
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
        });
        Ok(id)
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
        let bad = Mesh {
            positions: vec![Vec3::ZERO],
            indices: vec![0, 1, 2],
            ..Default::default()
        };
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
