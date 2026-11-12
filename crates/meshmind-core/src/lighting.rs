//! Lighting presets shared by the preview renderer and the desktop viewer,
//! so the agent and the person see the model under the same light.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingPreset {
    /// Neutral, soft three-point light. Best for judging shape and colour.
    #[default]
    Studio,
    /// Bright sun and blue sky.
    Outdoor,
    /// Low warm sun, cool shadows.
    Sunset,
    /// Dim blue ambience with a cold key light.
    Night,
}

/// Light values, all linear RGB.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LightingParams {
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    pub ambient_intensity: f32,
    /// Direction the key light comes from, in camera space for previews
    /// (x right, y up, z towards the viewer).
    pub key_direction: [f32; 3],
    pub key_color: [f32; 3],
    pub key_intensity: f32,
    pub fill_color: [f32; 3],
    pub fill_intensity: f32,
    pub background_top: [f32; 3],
    pub background_bottom: [f32; 3],
    pub exposure: f32,
}

impl LightingPreset {
    pub const ALL: [LightingPreset; 4] = [
        LightingPreset::Studio,
        LightingPreset::Outdoor,
        LightingPreset::Sunset,
        LightingPreset::Night,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LightingPreset::Studio => "studio",
            LightingPreset::Outdoor => "outdoor",
            LightingPreset::Sunset => "sunset",
            LightingPreset::Night => "night",
        }
    }

    pub fn from_name(s: &str) -> Option<LightingPreset> {
        LightingPreset::ALL.into_iter().find(|p| p.name() == s)
    }

    pub fn params(self) -> LightingParams {
        match self {
            LightingPreset::Studio => LightingParams {
                sky: [0.55, 0.6, 0.7],
                ground: [0.2, 0.18, 0.16],
                ambient_intensity: 0.45,
                key_direction: [-0.5, 0.7, 0.6],
                key_color: [1.0, 0.97, 0.92],
                key_intensity: 2.0,
                fill_color: [0.8, 0.88, 1.0],
                fill_intensity: 0.5,
                background_top: [0.16, 0.17, 0.2],
                background_bottom: [0.05, 0.05, 0.06],
                exposure: 1.0,
            },
            LightingPreset::Outdoor => LightingParams {
                sky: [0.45, 0.62, 0.95],
                ground: [0.28, 0.25, 0.2],
                ambient_intensity: 0.6,
                key_direction: [-0.4, 0.85, 0.35],
                key_color: [1.0, 0.95, 0.85],
                key_intensity: 3.0,
                fill_color: [0.6, 0.75, 1.0],
                fill_intensity: 0.3,
                background_top: [0.32, 0.5, 0.82],
                background_bottom: [0.7, 0.78, 0.86],
                exposure: 0.9,
            },
            LightingPreset::Sunset => LightingParams {
                sky: [0.45, 0.4, 0.6],
                ground: [0.25, 0.15, 0.1],
                ambient_intensity: 0.4,
                key_direction: [-0.85, 0.25, 0.45],
                key_color: [1.0, 0.6, 0.3],
                key_intensity: 2.8,
                fill_color: [0.45, 0.55, 1.0],
                fill_intensity: 0.45,
                background_top: [0.18, 0.15, 0.32],
                background_bottom: [0.75, 0.38, 0.2],
                exposure: 1.0,
            },
            LightingPreset::Night => LightingParams {
                sky: [0.12, 0.16, 0.3],
                ground: [0.04, 0.04, 0.06],
                ambient_intensity: 0.35,
                key_direction: [0.5, 0.6, 0.6],
                key_color: [0.65, 0.78, 1.0],
                key_intensity: 1.4,
                fill_color: [1.0, 0.65, 0.35],
                fill_intensity: 0.35,
                background_top: [0.02, 0.025, 0.05],
                background_bottom: [0.05, 0.06, 0.1],
                exposure: 1.3,
            },
        }
    }
}
