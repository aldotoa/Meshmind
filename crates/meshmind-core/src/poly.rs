//! Editable polygon meshes.
//!
//! [`PolyMesh`] is the source of truth for every object: welded vertices and
//! n-gon faces, which is what modelling operations need. It is triangulated
//! into a render/export [`Mesh`] on demand.

use std::collections::HashMap;

use glam::{Affine3A, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::mesh::{Aabb, Mesh};

/// One polygon. Vertices are listed counter-clockwise when seen from outside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub verts: Vec<u32>,
    /// Smooth faces share normals with smooth neighbours; flat faces get a
    /// hard edge all around.
    pub smooth: bool,
}

impl Face {
    pub fn new(verts: Vec<u32>, smooth: bool) -> Self {
        Self { verts, smooth }
    }

    /// Edges as (from, to) pairs in winding order.
    pub fn edges(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let n = self.verts.len();
        (0..n).map(move |i| (self.verts[i], self.verts[(i + 1) % n]))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PolyMesh {
    pub positions: Vec<Vec3>,
    pub faces: Vec<Face>,
}

impl PolyMesh {
    pub fn add_vertex(&mut self, p: Vec3) -> u32 {
        self.positions.push(p);
        (self.positions.len() - 1) as u32
    }

    pub fn add_face(&mut self, verts: Vec<u32>, smooth: bool) {
        self.faces.push(Face::new(verts, smooth));
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Newell's method: robust for slightly non-planar polygons. Not normalized;
    /// its length is twice the polygon's area.
    pub fn face_normal_area(&self, face: &Face) -> Vec3 {
        let mut n = Vec3::ZERO;
        for (a, b) in face.edges() {
            let (p, q) = (self.positions[a as usize], self.positions[b as usize]);
            n += Vec3::new(
                (p.y - q.y) * (p.z + q.z),
                (p.z - q.z) * (p.x + q.x),
                (p.x - q.x) * (p.y + q.y),
            );
        }
        n
    }

    pub fn face_normal(&self, face: &Face) -> Vec3 {
        self.face_normal_area(face).normalize_or_zero()
    }

    pub fn face_centroid(&self, face: &Face) -> Vec3 {
        let sum: Vec3 = face.verts.iter().map(|&v| self.positions[v as usize]).sum();
        sum / face.verts.len() as f32
    }

    pub fn bounds(&self) -> Option<Aabb> {
        let first = *self.positions.first()?;
        let (min, max) = self
            .positions
            .iter()
            .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        Some(Aabb { min, max })
    }

    /// Signed volume (positive for a closed mesh with outward faces).
    pub fn volume(&self) -> f32 {
        let mut v = 0.0;
        for f in &self.faces {
            let p0 = self.positions[f.verts[0] as usize];
            for i in 1..f.verts.len() - 1 {
                let p1 = self.positions[f.verts[i] as usize];
                let p2 = self.positions[f.verts[i + 1] as usize];
                v += p0.dot(p1.cross(p2));
            }
        }
        v / 6.0
    }

    /// Map from directed edge (a, b) to the face that contains it.
    pub fn directed_edges(&self) -> HashMap<(u32, u32), usize> {
        let mut map = HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            for e in f.edges() {
                map.insert(e, fi);
            }
        }
        map
    }

    /// True when every edge is shared by exactly two faces with opposite winding.
    pub fn is_closed_manifold(&self) -> bool {
        let edges = self.directed_edges();
        let total: usize = self.faces.iter().map(|f| f.verts.len()).sum();
        total == edges.len() && edges.keys().all(|&(a, b)| edges.contains_key(&(b, a)))
    }

    pub fn validate(&self) -> Result<(), String> {
        let n = self.positions.len();
        for (i, f) in self.faces.iter().enumerate() {
            if f.verts.len() < 3 {
                return Err(format!("face {i} has fewer than 3 vertices"));
            }
            if let Some(bad) = f.verts.iter().find(|&&v| v as usize >= n) {
                return Err(format!(
                    "face {i} uses vertex {bad}, but there are only {n}"
                ));
            }
        }
        if self.positions.iter().any(|p| !p.is_finite()) {
            return Err("mesh has non-finite vertex positions".into());
        }
        Ok(())
    }

    pub fn transformed(&self, m: &Affine3A) -> PolyMesh {
        let mut out = self.clone();
        for p in &mut out.positions {
            *p = m.transform_point3(*p);
        }
        // A mirroring transform flips the winding.
        if m.matrix3.determinant() < 0.0 {
            for f in &mut out.faces {
                f.verts.reverse();
            }
        }
        out
    }

    /// Append another mesh's vertices and faces.
    pub fn append(&mut self, other: &PolyMesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        for f in &other.faces {
            self.add_face(f.verts.iter().map(|v| v + base).collect(), f.smooth);
        }
    }

    /// Merge vertices closer than `epsilon`, drop unused vertices and faces
    /// that collapse below three distinct corners.
    pub fn weld(&mut self, epsilon: f32) {
        let inv = 1.0 / epsilon.max(1e-9);
        let key = |p: Vec3| {
            (
                (p.x * inv).round() as i64,
                (p.y * inv).round() as i64,
                (p.z * inv).round() as i64,
            )
        };
        let mut grid: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut positions = Vec::new();
        let mut remap = Vec::with_capacity(self.positions.len());
        for &p in &self.positions {
            let id = *grid.entry(key(p)).or_insert_with(|| {
                positions.push(p);
                (positions.len() - 1) as u32
            });
            remap.push(id);
        }
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut verts: Vec<u32> = Vec::with_capacity(f.verts.len());
            for &v in &f.verts {
                let r = remap[v as usize];
                if verts.last() != Some(&r) {
                    verts.push(r);
                }
            }
            while verts.len() > 1 && verts.first() == verts.last() {
                verts.pop();
            }
            if verts.len() >= 3 {
                faces.push(Face::new(verts, f.smooth));
            }
        }
        self.positions = positions;
        self.faces = faces;
        self.remove_unused_vertices();
    }

    pub fn remove_unused_vertices(&mut self) {
        let mut used = vec![u32::MAX; self.positions.len()];
        let mut positions = Vec::new();
        for f in &mut self.faces {
            for v in &mut f.verts {
                if used[*v as usize] == u32::MAX {
                    used[*v as usize] = positions.len() as u32;
                    positions.push(self.positions[*v as usize]);
                }
                *v = used[*v as usize];
            }
        }
        self.positions = positions;
    }

    /// Triangulate into a render mesh. Flat faces get their own vertices and
    /// face normal; smooth faces share area-weighted normals per position.
    /// UVs are a simple box projection until proper unwrapping lands.
    pub fn to_mesh(&self) -> Mesh {
        let normals: Vec<Vec3> = self
            .faces
            .iter()
            .map(|f| self.face_normal_area(f))
            .collect();
        let mut smooth_normals = vec![Vec3::ZERO; self.positions.len()];
        for (f, n) in self.faces.iter().zip(&normals) {
            if f.smooth {
                for &v in &f.verts {
                    smooth_normals[v as usize] += *n;
                }
            }
        }
        let mut mesh = Mesh::default();
        let mut shared: HashMap<u32, u32> = HashMap::new();
        for (f, n) in self.faces.iter().zip(&normals) {
            let face_n = n.normalize_or_zero();
            let mut idx = Vec::with_capacity(f.verts.len());
            for &v in &f.verts {
                let p = self.positions[v as usize];
                let make = |mesh: &mut Mesh, normal: Vec3| {
                    mesh.positions.push(p);
                    mesh.normals.push(normal);
                    mesh.uvs.push(box_uv(p, face_n));
                    (mesh.positions.len() - 1) as u32
                };
                let i = if f.smooth {
                    *shared.entry(v).or_insert_with(|| {
                        make(&mut mesh, smooth_normals[v as usize].normalize_or_zero())
                    })
                } else {
                    make(&mut mesh, face_n)
                };
                idx.push(i);
            }
            for i in 1..idx.len() - 1 {
                mesh.indices
                    .extend_from_slice(&[idx[0], idx[i], idx[i + 1]]);
            }
        }
        mesh
    }
}

fn box_uv(p: Vec3, n: Vec3) -> Vec2 {
    let a = n.abs();
    if a.x >= a.y && a.x >= a.z {
        Vec2::new(p.z, p.y)
    } else if a.y >= a.z {
        Vec2::new(p.x, p.z)
    } else {
        Vec2::new(p.x, p.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, uv_sphere};

    #[test]
    fn cube_volume_and_topology() {
        let c = cube(2.0);
        assert!((c.volume() - 8.0).abs() < 1e-4);
        assert!(c.is_closed_manifold());
        let m = c.to_mesh();
        m.validate().unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.vertex_count(), 24, "flat faces get their own vertices");
    }

    #[test]
    fn smooth_sphere_shares_vertices() {
        let s = uv_sphere(1.0, 16, 8);
        assert!(s.is_closed_manifold());
        let m = s.to_mesh();
        assert_eq!(m.vertex_count(), s.positions.len());
    }

    #[test]
    fn mirror_transform_keeps_outward_winding() {
        let c = cube(1.0).transformed(&Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!(c.volume() > 0.0);
    }

    #[test]
    fn weld_merges_duplicates() {
        let mut a = cube(1.0);
        let b = cube(1.0);
        a.append(&b);
        a.weld(1e-5);
        assert_eq!(a.positions.len(), 8);
    }
}
