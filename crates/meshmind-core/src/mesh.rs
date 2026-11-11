use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// An indexed triangle mesh with per-vertex normals and UVs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    /// Triangle list: every three indices form one triangle.
    pub indices: Vec<u32>,
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Bounding box of all vertices, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<Aabb> {
        let first = *self.positions.first()?;
        let (min, max) = self
            .positions
            .iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        Some(Aabb { min, max })
    }

    /// Recompute smooth vertex normals from triangle faces (area-weighted).
    pub fn recompute_normals(&mut self) {
        let mut normals = vec![Vec3::ZERO; self.positions.len()];
        for tri in self.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            let n = (self.positions[b] - self.positions[a])
                .cross(self.positions[c] - self.positions[a]);
            normals[a] += n;
            normals[b] += n;
            normals[c] += n;
        }
        self.normals = normals.into_iter().map(|n| n.normalize_or_zero()).collect();
    }

    /// Check structural consistency. Returns a description of the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.positions.len();
        if self.indices.len() % 3 != 0 {
            return Err("index count is not a multiple of 3".into());
        }
        if !self.normals.is_empty() && self.normals.len() != n {
            return Err("normal count does not match vertex count".into());
        }
        if !self.uvs.is_empty() && self.uvs.len() != n {
            return Err("uv count does not match vertex count".into());
        }
        if let Some(bad) = self.indices.iter().find(|&&i| i as usize >= n) {
            return Err(format!("index {bad} is out of range for {n} vertices"));
        }
        Ok(())
    }
}
