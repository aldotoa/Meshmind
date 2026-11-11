//! Modelling operations on [`PolyMesh`]. Each takes a mesh and returns a new one,
//! so callers can keep the old version for undo.

use std::collections::HashMap;

use glam::{Affine3A, Vec3};
use serde::{Deserialize, Serialize};

use crate::poly::{Face, PolyMesh};

/// Which faces an operation applies to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Selection {
    All,
    /// Faces whose normal is within `max_angle_deg` of `direction`.
    Facing {
        direction: Vec3,
        max_angle_deg: f32,
    },
    /// Explicit face indices.
    Faces {
        indices: Vec<usize>,
    },
}

impl Selection {
    pub fn resolve(&self, mesh: &PolyMesh) -> Vec<bool> {
        match self {
            Selection::All => vec![true; mesh.faces.len()],
            Selection::Facing {
                direction,
                max_angle_deg,
            } => {
                let dir = direction.normalize_or_zero();
                let cos = max_angle_deg.to_radians().cos() - 1e-5;
                mesh.faces
                    .iter()
                    .map(|f| mesh.face_normal(f).dot(dir) >= cos)
                    .collect()
            }
            Selection::Faces { indices } => {
                let mut sel = vec![false; mesh.faces.len()];
                for &i in indices {
                    if let Some(s) = sel.get_mut(i) {
                        *s = true;
                    }
                }
                sel
            }
        }
    }
}

fn require_any(sel: &[bool]) -> Result<(), String> {
    if sel.iter().any(|&s| s) {
        Ok(())
    } else {
        Err("the selection matched no faces".into())
    }
}

/// Push the selected faces out along their normals by `distance` as one
/// connected region, adding side walls around the region's border.
/// A negative distance pushes inward (for recesses).
pub fn extrude(mesh: &PolyMesh, selection: &Selection, distance: f32) -> Result<PolyMesh, String> {
    let sel = selection.resolve(mesh);
    require_any(&sel)?;
    let mut out = mesh.clone();
    let edges = mesh.directed_edges();

    // Average normal of the selected faces around each vertex.
    let mut dirs: HashMap<u32, Vec3> = HashMap::new();
    for (f, _) in mesh.faces.iter().zip(&sel).filter(|(_, s)| **s) {
        let n = mesh.face_normal(f);
        for &v in &f.verts {
            *dirs.entry(v).or_default() += n;
        }
    }
    let mut copy: HashMap<u32, u32> = HashMap::new();
    let mut verts: Vec<u32> = dirs.keys().copied().collect();
    verts.sort_unstable();
    for v in verts {
        let d = dirs[&v].normalize_or_zero();
        copy.insert(v, out.add_vertex(mesh.positions[v as usize] + d * distance));
    }

    for (fi, f) in mesh.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        for (a, b) in f.edges() {
            let across_selected = edges.get(&(b, a)).is_some_and(|&g| sel[g]);
            if !across_selected {
                out.add_face(vec![a, b, copy[&b], copy[&a]], false);
            }
        }
        out.faces[fi].verts = f.verts.iter().map(|v| copy[v]).collect();
    }
    Ok(out)
}

/// Shrink each selected face towards its centre by `amount` (0 to 1), joining
/// it to its old border with a ring of new faces. Pair with `extrude` for
/// panels, buttons and recesses.
pub fn inset(mesh: &PolyMesh, selection: &Selection, amount: f32) -> Result<PolyMesh, String> {
    if !(0.0..1.0).contains(&amount) {
        return Err("inset amount must be between 0 and 1".into());
    }
    let sel = selection.resolve(mesh);
    require_any(&sel)?;
    let mut out = mesh.clone();
    for (fi, f) in mesh.faces.iter().enumerate() {
        if !sel[fi] {
            continue;
        }
        let c = mesh.face_centroid(f);
        let inner: Vec<u32> = f
            .verts
            .iter()
            .map(|&v| {
                let p = mesh.positions[v as usize];
                out.add_vertex(p + (c - p) * amount)
            })
            .collect();
        let n = f.verts.len();
        for i in 0..n {
            let j = (i + 1) % n;
            out.add_face(vec![f.verts[i], f.verts[j], inner[j], inner[i]], f.smooth);
        }
        out.faces[fi] = Face::new(inner, f.smooth);
    }
    Ok(out)
}

/// Chamfer the hard edges of a closed mesh: edges where the faces meet at more
/// than `angle_deg` degrees. `amount` is the distance from the original edge.
/// Hard-surface models look far more real with small bevels; follow with a
/// smooth `subdivide` for rounded edges.
pub fn bevel(mesh: &PolyMesh, amount: f32, angle_deg: f32) -> Result<PolyMesh, String> {
    if !mesh.is_closed_manifold() {
        return Err("bevel needs a closed mesh (every edge shared by two faces)".into());
    }
    let edges = mesh.directed_edges();
    let normals: Vec<Vec3> = mesh.faces.iter().map(|f| mesh.face_normal(f)).collect();
    let cos_limit = angle_deg.to_radians().cos();
    let sharp = |a: u32, b: u32| -> bool {
        let (f, g) = (edges[&(a, b)], edges[&(b, a)]);
        normals[f].dot(normals[g]) < cos_limit
    };

    let min_edge = mesh
        .faces
        .iter()
        .flat_map(|f| f.edges())
        .filter(|&(a, b)| sharp(a, b))
        .map(|(a, b)| mesh.positions[a as usize].distance(mesh.positions[b as usize]))
        .fold(f32::INFINITY, f32::min);
    if !min_edge.is_finite() {
        return Err(format!(
            "no edges sharper than {angle_deg} degrees to bevel"
        ));
    }
    let amount = amount.min(min_edge * 0.45);
    if amount <= 0.0 {
        return Err("bevel amount must be positive".into());
    }

    let mut out = PolyMesh::default();
    // corner[(face, vertex)] = new vertex id. Each face's corner moves away
    // from its sharp edges, staying in the face's plane.
    let mut corner: HashMap<(usize, u32), u32> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        let n = f.verts.len();
        let mut new_face = Vec::with_capacity(n);
        for k in 0..n {
            let v = f.verts[k];
            let (pv, nv) = (f.verts[(k + n - 1) % n], f.verts[(k + 1) % n]);
            let p = mesh.positions[v as usize];
            let u_prev = (mesh.positions[pv as usize] - p).normalize_or_zero();
            let u_next = (mesh.positions[nv as usize] - p).normalize_or_zero();
            let corner_angle = u_prev
                .angle_between(u_next)
                .clamp(0.2, std::f32::consts::PI - 0.2);
            // Place the corner exactly `amount` away from each sharp edge.
            let offset = match (sharp(pv, v), sharp(v, nv)) {
                (true, true) => {
                    (u_prev + u_next).normalize_or_zero() * (amount / (corner_angle / 2.0).sin())
                }
                (false, true) => u_prev * (amount / corner_angle.sin()),
                (true, false) => u_next * (amount / corner_angle.sin()),
                (false, false) => Vec3::ZERO,
            };
            let id = out.add_vertex(p + offset);
            corner.insert((fi, v), id);
            new_face.push(id);
        }
        out.add_face(new_face, f.smooth);
    }

    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            let g = edges[&(b, a)];
            // Each sharp undirected edge once.
            if fi < g && sharp(a, b) {
                out.add_face(
                    vec![
                        corner[&(fi, b)],
                        corner[&(fi, a)],
                        corner[&(g, a)],
                        corner[&(g, b)],
                    ],
                    false,
                );
            }
        }
    }

    // One polygon per original vertex, closing the gap between its corners.
    let mut outgoing: HashMap<u32, Vec<(usize, u32)>> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            outgoing.entry(a).or_default().push((fi, b));
        }
    }
    let mut vertex_ids: Vec<u32> = outgoing.keys().copied().collect();
    vertex_ids.sort_unstable();
    for v in vertex_ids {
        let out_edges = &outgoing[&v];
        if !out_edges.iter().any(|&(_, b)| sharp(v, b)) {
            continue;
        }
        let start = out_edges[0].0;
        let mut ring = vec![start];
        let mut f = start;
        loop {
            let (_, b) = *out_edges
                .iter()
                .find(|(face, _)| *face == f)
                .expect("manifold");
            f = edges[&(b, v)];
            if f == start {
                break;
            }
            ring.push(f);
            if ring.len() > out_edges.len() {
                return Err("bevel found a non-manifold vertex".into());
            }
        }
        ring.reverse();
        out.add_face(ring.iter().map(|&f| corner[&(f, v)]).collect(), false);
    }
    // Corners that did not move coincide; merge them and drop collapsed faces.
    out.weld(amount * 1e-3);
    Ok(out)
}
/// Catmull-Clark subdivision (`smooth = true`) rounds the shape and makes it
/// look organic. Linear subdivision (`smooth = false`) only adds detail
/// without changing the shape. Each level multiplies the face count by about 4.
pub fn subdivide(mesh: &PolyMesh, levels: u32, smooth: bool) -> Result<PolyMesh, String> {
    if levels > 4 {
        return Err("at most 4 subdivision levels at once".into());
    }
    let mut m = mesh.clone();
    for _ in 0..levels {
        m = subdivide_once(&m, smooth);
    }
    Ok(m)
}

fn subdivide_once(mesh: &PolyMesh, smooth: bool) -> PolyMesh {
    let key = |a: u32, b: u32| (a.min(b), a.max(b));
    let face_points: Vec<Vec3> = mesh.faces.iter().map(|f| mesh.face_centroid(f)).collect();
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            edge_faces.entry(key(a, b)).or_default().push(fi);
        }
    }

    let mut out = PolyMesh {
        positions: mesh.positions.clone(),
        faces: Vec::new(),
    };

    // Edge points.
    let mut edge_keys: Vec<(u32, u32)> = edge_faces.keys().copied().collect();
    edge_keys.sort_unstable();
    let mut edge_point: HashMap<(u32, u32), u32> = HashMap::new();
    for k in edge_keys {
        let faces = &edge_faces[&k];
        let (a, b) = (mesh.positions[k.0 as usize], mesh.positions[k.1 as usize]);
        let p = if smooth && faces.len() == 2 {
            (a + b + face_points[faces[0]] + face_points[faces[1]]) / 4.0
        } else {
            (a + b) / 2.0
        };
        edge_point.insert(k, out.add_vertex(p));
    }

    // Move original vertices.
    if smooth {
        let n = mesh.positions.len();
        let mut face_sum = vec![Vec3::ZERO; n];
        let mut face_count = vec![0u32; n];
        let mut edge_sum = vec![Vec3::ZERO; n];
        let mut edge_count = vec![0u32; n];
        let mut boundary = vec![false; n];
        for (fi, f) in mesh.faces.iter().enumerate() {
            for &v in &f.verts {
                face_sum[v as usize] += face_points[fi];
                face_count[v as usize] += 1;
            }
        }
        for (&(a, b), faces) in &edge_faces {
            let mid = (mesh.positions[a as usize] + mesh.positions[b as usize]) / 2.0;
            for v in [a, b] {
                edge_sum[v as usize] += mid;
                edge_count[v as usize] += 1;
                if faces.len() != 2 {
                    boundary[v as usize] = true;
                }
            }
        }
        for v in 0..n {
            if boundary[v] || face_count[v] == 0 {
                continue;
            }
            let k = face_count[v] as f32;
            let f = face_sum[v] / k;
            let r = edge_sum[v] / edge_count[v] as f32;
            out.positions[v] = (f + 2.0 * r + (k - 3.0) * mesh.positions[v]) / k;
        }
    }

    for (fi, f) in mesh.faces.iter().enumerate() {
        let c = out.add_vertex(face_points[fi]);
        let n = f.verts.len();
        for i in 0..n {
            let v = f.verts[i];
            let next = f.verts[(i + 1) % n];
            let prev = f.verts[(i + n - 1) % n];
            out.add_face(
                vec![v, edge_point[&key(v, next)], c, edge_point[&key(prev, v)]],
                f.smooth || smooth,
            );
        }
    }
    out
}

/// Axis for mirroring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn unit(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }
}

/// Mirror the mesh across the plane through the origin perpendicular to
/// `axis` and join the halves. Faces lying exactly on the mirror plane are
/// removed so a half-model becomes one closed shape. Model one side, mirror
/// for symmetry.
pub fn mirror(mesh: &PolyMesh, axis: Axis) -> PolyMesh {
    let ax = axis.unit();
    let scale = Vec3::ONE - 2.0 * ax;
    let mut out = mesh.clone();
    out.append(&mesh.transformed(&Affine3A::from_scale(scale)));
    let eps = 1e-5;
    let positions = out.positions.clone();
    out.faces.retain(|f| {
        !f.verts
            .iter()
            .all(|&v| positions[v as usize].dot(ax).abs() < eps)
    });
    out.weld(eps);
    out
}

/// Repeat the mesh `count` times, each copy moved by `offset` from the last.
pub fn array(mesh: &PolyMesh, count: u32, offset: Vec3) -> Result<PolyMesh, String> {
    if !(1..=256).contains(&count) {
        return Err("array count must be from 1 to 256".into());
    }
    let mut out = mesh.clone();
    for i in 1..count {
        out.append(&mesh.transformed(&Affine3A::from_translation(offset * i as f32)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, cylinder, uv_sphere};

    fn up() -> Selection {
        Selection::Facing {
            direction: Vec3::Y,
            max_angle_deg: 10.0,
        }
    }

    #[test]
    fn extrude_top_of_cube() {
        let m = extrude(&cube(1.0), &up(), 0.5).unwrap();
        assert!(m.is_closed_manifold());
        assert!((m.volume() - 1.5).abs() < 1e-4);
        assert!((m.bounds().unwrap().max.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn extrude_inward_makes_recess() {
        let m = inset(&cube(1.0), &up(), 0.5).unwrap();
        let top = Selection::Faces { indices: vec![2] }; // inset keeps the face's index
        let m = extrude(&m, &top, -0.2).unwrap();
        assert!(m.is_closed_manifold());
        assert!((m.volume() - (1.0 - 0.25 * 0.2)).abs() < 1e-4);
    }

    #[test]
    fn empty_selection_is_an_error() {
        let sel = Selection::Faces { indices: vec![] };
        assert!(extrude(&cube(1.0), &sel, 1.0).is_err());
    }

    #[test]
    fn bevel_cube() {
        let m = bevel(&cube(1.0), 0.1, 30.0).unwrap();
        assert!(m.is_closed_manifold());
        assert_eq!(m.faces.len(), 6 + 12 + 8);
        let v = m.volume();
        assert!(v < 1.0 && v > 0.9, "volume {v}");
        let b = m.bounds().unwrap();
        assert!(
            (b.size() - Vec3::ONE).length() < 1e-5,
            "bevel keeps outer size"
        );
    }

    #[test]
    fn bevel_cylinder() {
        let c = cylinder(0.5, 1.0, 32);
        let m = bevel(&c, 0.05, 30.0).unwrap();
        assert!(m.is_closed_manifold());
        let (v0, v1) = (c.volume(), m.volume());
        assert!(v1 < v0 && v1 > v0 * 0.95, "{v0} -> {v1}");
        // Only the two rims are bevelled: 32 side faces + 2 caps + 64 rim chamfers.
        assert_eq!(m.faces.len(), 32 + 2 + 64);
        assert!(
            bevel(&crate::primitives::uv_sphere(1.0, 16, 8), 0.05, 30.0).is_err(),
            "a sphere has no hard edges"
        );
    }

    #[test]
    fn subdivide_cube_rounds_it() {
        let m = subdivide(&cube(2.0), 2, true).unwrap();
        assert!(m.is_closed_manifold());
        assert_eq!(m.faces.len(), 6 * 16);
        let v = m.volume();
        assert!(v < 8.0 && v > 2.0, "volume {v}");
        let linear = subdivide(&cube(2.0), 1, false).unwrap();
        assert!((linear.volume() - 8.0).abs() < 1e-4);
    }

    #[test]
    fn mirror_half_cube_makes_whole_box() {
        let half = cube(1.0).transformed(&Affine3A::from_translation(Vec3::new(0.5, 0.0, 0.0)));
        let m = mirror(&half, Axis::X);
        assert!(m.is_closed_manifold());
        assert!((m.volume() - 2.0).abs() < 1e-4);
        assert_eq!(m.positions.len(), 12);
    }

    #[test]
    fn array_copies() {
        let m = array(&uv_sphere(0.5, 8, 4), 3, Vec3::X * 2.0).unwrap();
        assert!((m.bounds().unwrap().size().x - 5.0).abs() < 1e-5);
    }
}
