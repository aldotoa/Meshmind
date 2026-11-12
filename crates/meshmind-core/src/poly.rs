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
    /// Texture coordinates per corner, set by UV unwrapping. Empty when the
    /// face has none; geometry edits clear them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uvs: Vec<Vec2>,
}

impl Face {
    pub fn new(verts: Vec<u32>, smooth: bool) -> Self {
        Self {
            verts,
            smooth,
            uvs: Vec::new(),
        }
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

    pub fn has_uvs(&self) -> bool {
        !self.faces.is_empty() && self.faces.iter().all(|f| f.uvs.len() == f.verts.len())
    }

    pub fn clear_uvs(&mut self) {
        for f in &mut self.faces {
            f.uvs.clear();
        }
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

    /// Split a face into triangles, as corner indices into `face.verts`.
    /// Convex faces use a fan; concave ones (outlines with notches, blade
    /// cross-sections with a fuller) use ear clipping.
    pub fn triangulate_face(&self, face: &Face) -> Vec<[usize; 3]> {
        let n = face.verts.len();
        let fan = || (1..n - 1).map(|i| [0, i, i + 1]).collect::<Vec<_>>();
        if n <= 3 {
            return fan();
        }
        let normal = self.face_normal_area(face);
        if normal.length_squared() < 1e-20 {
            return fan();
        }
        // Project onto the face plane.
        let (u, v) = normal.normalize().any_orthonormal_pair();
        let pts: Vec<Vec2> = face
            .verts
            .iter()
            .map(|&i| {
                let p = self.positions[i as usize];
                Vec2::new(p.dot(u), p.dot(v))
            })
            .collect();
        // Orientation of (u, v) relative to the normal decides the sign of a convex turn.
        let sign = if u.cross(v).dot(normal) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let turn = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - b) * sign;
        if (0..n).all(|i| turn(pts[i], pts[(i + 1) % n], pts[(i + 2) % n]) >= -1e-12) {
            return fan();
        }
        ear_clip(&pts, sign).unwrap_or_else(fan)
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
                f.uvs.reverse();
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
    /// face normal. Smooth faces share area-weighted normals with smooth
    /// neighbours, except across edges sharper than [`AUTO_SMOOTH_DEGREES`],
    /// which stay crisp (a blade's cutting edge, the rim of a lathed pommel).
    pub fn to_mesh(&self) -> Mesh {
        let normals: Vec<Vec3> = self
            .faces
            .iter()
            .map(|f| self.face_normal_area(f))
            .collect();
        let unit: Vec<Vec3> = normals.iter().map(|n| n.normalize_or_zero()).collect();
        let mut vertex_faces: Vec<Vec<u32>> = vec![Vec::new(); self.positions.len()];
        for (fi, f) in self.faces.iter().enumerate() {
            if f.smooth {
                for &v in &f.verts {
                    vertex_faces[v as usize].push(fi as u32);
                }
            }
        }
        let cos_limit = AUTO_SMOOTH_DEGREES.to_radians().cos();
        let smooth_normal = |v: u32, fi: usize| -> Vec3 {
            let mut sum = Vec3::ZERO;
            for &g in &vertex_faces[v as usize] {
                if unit[g as usize].dot(unit[fi]) >= cos_limit {
                    sum += normals[g as usize];
                }
            }
            sum.normalize_or(unit[fi])
        };
        let mut mesh = Mesh::default();
        // Smooth corners share a vertex when position, normal and UV match.
        let mut shared: HashMap<(u32, [u32; 2], [i32; 3]), u32> = HashMap::new();
        for (fi, f) in self.faces.iter().enumerate() {
            let face_n = unit[fi];
            let has_uv = f.uvs.len() == f.verts.len();
            let mut idx = Vec::with_capacity(f.verts.len());
            for (k, &v) in f.verts.iter().enumerate() {
                let p = self.positions[v as usize];
                let vn = if f.smooth {
                    smooth_normal(v, fi)
                } else {
                    face_n
                };
                let uv = match (has_uv, f.smooth) {
                    (true, _) => f.uvs[k],
                    // Project by the shared vertex normal so smooth neighbours agree.
                    (false, true) => box_uv(p, vn),
                    (false, false) => box_uv(p, face_n),
                };
                let make = |mesh: &mut Mesh, normal: Vec3| {
                    mesh.positions.push(p);
                    mesh.normals.push(normal);
                    mesh.uvs.push(uv);
                    (mesh.positions.len() - 1) as u32
                };
                let i = if f.smooth {
                    let q = (vn * 4096.0).round().as_ivec3().to_array();
                    *shared
                        .entry((v, [uv.x.to_bits(), uv.y.to_bits()], q))
                        .or_insert_with(|| make(&mut mesh, vn))
                } else {
                    make(&mut mesh, face_n)
                };
                idx.push(i);
            }
            for t in self.triangulate_face(f) {
                mesh.indices
                    .extend_from_slice(&[idx[t[0]], idx[t[1]], idx[t[2]]]);
            }
        }
        mesh
    }
}

/// Smooth faces meeting at a sharper angle than this keep a hard edge.
pub const AUTO_SMOOTH_DEGREES: f32 = 50.0;

/// Ear clipping for a simple polygon in 2D. `sign` makes convex turns positive.
fn ear_clip(pts: &[Vec2], sign: f32) -> Option<Vec<[usize; 3]>> {
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    let mut out = Vec::with_capacity(pts.len() - 2);
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a) * sign;
    let mut guard = 0;
    while idx.len() > 3 {
        guard += 1;
        if guard > pts.len() * pts.len() {
            return None;
        }
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (a, b, c) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            if cross(pts[a], pts[b], pts[c]) <= 1e-12 {
                continue; // reflex or degenerate corner
            }
            let inside = idx.iter().any(|&k| {
                k != a
                    && k != b
                    && k != c
                    && cross(pts[a], pts[b], pts[k]) >= 0.0
                    && cross(pts[b], pts[c], pts[k]) >= 0.0
                    && cross(pts[c], pts[a], pts[k]) >= 0.0
            });
            if !inside {
                out.push([a, b, c]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            return None;
        }
    }
    out.push([idx[0], idx[1], idx[2]]);
    Some(out)
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
    fn concave_faces_triangulate_inside_their_outline() {
        // An L-shaped face: a fan from corner 0 would cover the notch.
        let mut m = PolyMesh::default();
        for (x, y) in [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.)] {
            m.add_vertex(Vec3::new(x, y, 0.0));
        }
        m.add_face((0..6).collect(), false);
        let tris = m.triangulate_face(&m.faces[0]);
        assert_eq!(tris.len(), 4);
        let area: f32 = tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i]);
                (b - a).cross(c - a).z * 0.5
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-5, "area {area}");
        assert!(tris.iter().all(|t| {
            let [a, b, c] = t.map(|i| m.positions[i]);
            (b - a).cross(c - a).z > 0.0
        }));
    }

    #[test]
    fn auto_smooth_keeps_sharp_edges_on_smooth_faces() {
        let mut c = cube(1.0);
        for f in &mut c.faces {
            f.smooth = true;
        }
        // 90 degree edges stay hard: every corner keeps its face normal.
        assert_eq!(c.to_mesh().vertex_count(), 24);
        let s = uv_sphere(1.0, 16, 8);
        assert_eq!(s.to_mesh().vertex_count(), s.positions.len());
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
