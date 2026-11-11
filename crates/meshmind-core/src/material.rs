use serde::{Deserialize, Serialize};

pub type MaterialId = u32;

/// Metallic-roughness PBR material, matching the glTF 2.0 model so exports
/// look the same in Unity, Unreal, Godot and Blender.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Linear RGBA base color.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear RGB emissive color.
    pub emissive: [f32; 3],
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
        }
    }
}

impl Material {
    /// Clamp all factors into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        for c in &mut self.base_color {
            *c = c.clamp(0.0, 1.0);
        }
        for c in &mut self.emissive {
            *c = c.max(0.0);
        }
        self.metallic = self.metallic.clamp(0.0, 1.0);
        self.roughness = self.roughness.clamp(0.0, 1.0);
        self
    }
}
