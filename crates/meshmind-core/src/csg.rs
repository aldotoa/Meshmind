//! Boolean operations (union, difference, intersection) using Manifold
//! (<https://github.com/elalish/manifold>), the robust mesh boolean library
//! also used by OpenSCAD and Blender.
//!
//! Each face's index travels through Manifold as a vertex property, so the
//! result keeps track of which input face every triangle came from. Triangles
//! from the same face are merged back into one polygon where that polygon is
//! simple and convex, so later operations (inset, extrude) still see clean faces.

use std::collections::HashMap;

use glam::Vec3;
use manifold_csg::{Manifold, MeshGL64, MeshGL64Options};
use serde::{Deserialize, Serialize};

use crate::poly::{Face, PolyMesh};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp {
    /// Keep everything in either shape.
    Union,
    /// Cut the second shape out of the first.
    Difference,
    /// Keep only where the shapes overlap.
    Intersection,
}

/// Convert to Manifold. Every face gets its own vertices carrying the face's
/// index (offset by `face_base`) as a 4th property; merge pairs tell Manifold
/// which of them are really the same point.
fn to_manifold(mesh: &PolyMesh, face_base: usize) -> Result<Manifold, String> {
    let mut props: Vec<f64> = Vec::new();
    let mut tris: Vec<u64> = Vec::new();
    let mut merge_from: Vec<u64> = Vec::new();
    let mut merge_to: Vec<u64> = Vec::new();
    let mut first_copy: HashMap<u32, u64> = HashMap::new();
    let mut next: u64 = 0;
    for (fi, f) in mesh.faces.iter().enumerate() {
        let ids: Vec<u64> = f
            .verts
            .iter()
            .map(|&v| {
                let p = mesh.positions[v as usize];
                props.extend_from_slice(&[
                    p.x as f64,
                    p.y as f64,
                    p.z as f64,
                    (face_base + fi) as f64,
                ]);
                let id = next;
                next += 1;
                match first_copy.get(&v) {
                    Some(&canon) => {
                        merge_from.push(id);
                        merge_to.push(canon);
                    }
                    None => {
                        first_copy.insert(v, id);
                    }
                }
                id
            })
            .collect();
        for t in mesh.triangulate_face(f) {
            tris.extend_from_slice(&[ids[t[0]], ids[t[1]], ids[t[2]]]);
        }
    }
    let opts = MeshGL64Options::new().merge_vertices(&merge_from, &merge_to);
    let gl = MeshGL64::new_with_options(&props, 4, &tris, opts).map_err(|e| e.to_string())?;
    Manifold::from_meshgl64(&gl)
        .map_err(|_| "a boolean input is not a closed, watertight shape".to_string())
}

/// Quantized plane (normal x, y, z, distance) plus the smooth flag.
type PlaneKey = (i64, i64, i64, i64, bool);

/// Combine two meshes given in the same coordinate space.
pub fn boolean(a: &PolyMesh, b: &PolyMesh, op: BooleanOp) -> Result<PolyMesh, String> {
    let ma = to_manifold(a, 0)?;
    let mb = to_manifold(b, a.faces.len())?;
    let result = match op {
        BooleanOp::Union => ma.union(&mb),
        BooleanOp::Difference => ma.difference(&mb),
        BooleanOp::Intersection => ma.intersection(&mb),
    };
    result
        .status()
        .map_err(|e| format!("boolean failed: {e}"))?;
    let (props, n_props, tris) = result.to_mesh_f64();

    let smooth = |source: usize| -> bool {
        if source < a.faces.len() {
            a.faces[source].smooth
        } else {
            b.faces
                .get(source - a.faces.len())
                .is_some_and(|f| f.smooth)
        }
    };

    // Positions, welded across faces.
    let mut out = PolyMesh::default();
    let mut weld: HashMap<[u64; 3], u32> = HashMap::new();
    let n_verts = props.len() / n_props;
    let mut vid = Vec::with_capacity(n_verts);
    for i in 0..n_verts {
        let p = &props[i * n_props..i * n_props + 3];
        let key = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits()];
        let id = *weld
            .entry(key)
            .or_insert_with(|| out.add_vertex(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)));
        vid.push(id);
    }

    // Group triangles that lie in the same plane with the same shading, then
    // split each group into edge-connected pieces.
    let mut groups: HashMap<PlaneKey, Vec<[u32; 3]>> = HashMap::new();
    for t in tris.chunks_exact(3) {
        let source = props[t[0] as usize * n_props + 3].round() as usize;
        let tri = [vid[t[0] as usize], vid[t[1] as usize], vid[t[2] as usize]];
        if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
            continue;
        }
        let [pa, pb, pc] = tri.map(|i| out.positions[i as usize]);
        let n = (pb - pa).cross(pc - pa).normalize_or_zero();
        let q = |x: f32| (x * 1e4).round() as i64;
        groups
            .entry((q(n.x), q(n.y), q(n.z), q(n.dot(pa)), smooth(source)))
            .or_default()
            .push(tri);
    }
    let mut keys: Vec<_> = groups.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        for piece in connected_pieces(&groups[&key]) {
            match merge_group(&out, &piece) {
                Some(poly) => out.faces.push(Face::new(poly, key.4)),
                None => out
                    .faces
                    .extend(piece.iter().map(|t| Face::new(t.to_vec(), key.4))),
            }
        }
    }
    out.remove_unused_vertices();
    Ok(out)
}

/// Split triangles into groups connected through shared edges.
fn connected_pieces(tris: &[[u32; 3]]) -> Vec<Vec<[u32; 3]>> {
    let mut parent: Vec<usize> = (0..tris.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut by_edge: HashMap<(u32, u32), usize> = HashMap::new();
    for (ti, t) in tris.iter().enumerate() {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            if let Some(&other) = by_edge.get(&(a.min(b), a.max(b))) {
                let (x, y) = (find(&mut parent, ti), find(&mut parent, other));
                parent[x] = y;
            } else {
                by_edge.insert((a.min(b), a.max(b)), ti);
            }
        }
    }
    let mut pieces: HashMap<usize, Vec<[u32; 3]>> = HashMap::new();
    for (ti, t) in tris.iter().enumerate() {
        let root = find(&mut parent, ti);
        pieces.entry(root).or_default().push(*t);
    }
    let mut out: Vec<Vec<[u32; 3]>> = pieces.into_values().collect();
    out.sort_by_key(|p| p[0]);
    out
}

/// Merge a group of coplanar triangles into one polygon if their outline is a
/// single convex loop. Returns `None` otherwise (e.g. a face with a hole).
fn merge_group(mesh: &PolyMesh, tris: &[[u32; 3]]) -> Option<Vec<u32>> {
    if tris.len() == 1 {
        return Some(tris[0].to_vec());
    }
    let mut directed: HashMap<(u32, u32), usize> = HashMap::new();
    for t in tris {
        for i in 0..3 {
            *directed.entry((t[i], t[(i + 1) % 3])).or_default() += 1;
        }
    }
    // Boundary edges: no matching reverse edge inside the group.
    let mut next: HashMap<u32, u32> = HashMap::new();
    for &(a, b) in directed.keys() {
        if !directed.contains_key(&(b, a)) && next.insert(a, b).is_some() {
            return None; // vertex touched twice: not a simple loop
        }
    }
    let start = *next.keys().min()?;
    let mut lp = vec![start];
    let mut v = next[&start];
    while v != start {
        lp.push(v);
        v = *next.get(&v)?;
        if lp.len() > next.len() {
            return None;
        }
    }
    if lp.len() != next.len() {
        return None; // more than one loop
    }
    // Drop vertices lying on straight runs, then require convexity.
    let pts = |i: usize, lp: &[u32]| mesh.positions[lp[i % lp.len()] as usize];
    let normal = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            (b - a).cross(c - a)
        })
        .sum::<Vec3>()
        .normalize_or_zero();
    let n = lp.len();
    let mut kept = Vec::with_capacity(n);
    // Positive turn = convex corner.
    for i in 0..n {
        let (p, c, q) = (pts(i + n - 1, &lp), pts(i, &lp), pts(i + 1, &lp));
        let turn = (c - p).cross(q - c).dot(normal);
        let scale = (c - p).length() * (q - c).length();
        if turn < -1e-6 * scale {
            return None; // concave corner
        }
        // Collinear points are kept so neighbouring faces that share them stay joined.
        kept.push(lp[i]);
    }
    (kept.len() >= 3).then_some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, cylinder};
    use glam::Affine3A;

    fn shifted(m: &PolyMesh, by: Vec3) -> PolyMesh {
        m.transformed(&Affine3A::from_translation(by))
    }

    #[test]
    fn overlapping_cubes() {
        let a = cube(2.0);
        let b = shifted(&cube(2.0), Vec3::new(1.0, 0.0, 0.0));
        let u = boolean(&a, &b, BooleanOp::Union).unwrap();
        let d = boolean(&a, &b, BooleanOp::Difference).unwrap();
        let i = boolean(&a, &b, BooleanOp::Intersection).unwrap();
        assert!((u.volume() - 12.0).abs() < 1e-3, "union {}", u.volume());
        assert!((d.volume() - 4.0).abs() < 1e-3, "difference {}", d.volume());
        assert!(
            (i.volume() - 4.0).abs() < 1e-3,
            "intersection {}",
            i.volume()
        );
        for m in [&u, &d, &i] {
            assert!(m.is_closed_manifold());
        }
        // Clean faces: the union's flat sides merge back into single polygons.
        assert!(u.faces.len() <= 14, "union has {} faces", u.faces.len());
    }

    #[test]
    fn drill_a_hole() {
        let block = cube(2.0);
        let drill = cylinder(0.5, 3.0, 32);
        let result = boolean(&block, &drill, BooleanOp::Difference).unwrap();
        let expected = 8.0 - drill.volume() * (2.0 / 3.0);
        assert!(
            (result.volume() - expected).abs() < 1e-2,
            "{}",
            result.volume()
        );
        assert!(result.is_closed_manifold());
        // The hole's wall keeps the cylinder's smooth shading.
        assert!(result.faces.iter().any(|f| f.smooth));
    }

    #[test]
    fn coplanar_faces_do_not_leave_slivers() {
        // Same-height boxes sharing top and bottom planes: the classic BSP failure case.
        let a = cube(1.0);
        let b = shifted(&cube(1.0), Vec3::new(0.5, 0.0, 0.0));
        let u = boolean(&a, &b, BooleanOp::Union).unwrap();
        assert!(u.is_closed_manifold());
        assert!((u.volume() - 1.5).abs() < 1e-4);
        assert_eq!(u.faces.len(), 6, "a 1.5 x 1 x 1 box has 6 faces");
    }

    #[test]
    fn open_input_is_rejected() {
        let open = crate::primitives::plane(1.0);
        assert!(boolean(&cube(1.0), &open, BooleanOp::Union).is_err());
    }
}
