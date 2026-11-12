use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::texture::Texture;

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
    /// 0 = opaque, 1 = clear like glass: light passes through, tinted by the
    /// base colour (glTF `KHR_materials_transmission`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transmission: f32,
    /// Index of refraction: 1.5 for glass, 1.33 water, 1.77 ruby, 2.42 diamond.
    #[serde(default = "default_ior")]
    pub ior: f32,
    /// Baked textures (shared, so undo snapshots stay cheap). When present,
    /// the factors above multiply them, as in glTF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub textures: Option<Arc<MaterialTextures>>,
    /// The smart material the textures were baked from, so they can be baked
    /// again (for example after detail baking or polygon reduction).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smart: Option<SmartSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartSource {
    pub material: crate::smart::SmartMaterial,
    pub resolution: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialTextures {
    pub base_color: Texture,
    /// R = ambient occlusion, G = roughness, B = metallic (glTF packs
    /// occlusion into the same texture's red channel).
    pub metallic_roughness: Texture,
    pub normal: Texture,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn default_ior() -> f32 {
    1.5
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
            transmission: 0.0,
            ior: 1.5,
            textures: None,
            smart: None,
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
        self.transmission = self.transmission.clamp(0.0, 1.0);
        self.ior = if self.ior.is_finite() {
            self.ior.clamp(1.0, 3.0)
        } else {
            1.5
        };
        self
    }
}
