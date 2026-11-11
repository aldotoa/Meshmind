//! Bevel: round or chamfer the hard edges of a closed mesh.
//!
//! How it works:
//! 1. Every face keeps its plane but shrinks away from its hard edges, so each
//!    face corner sits exactly `amount` from every hard edge next to it.
//! 2. Each hard edge becomes a strip of `segments` faces following a circular
//!    arc between the two shrunken faces (one flat face when `segments` is 1).
//! 3. Each vertex touching a hard edge gets a patch closing the hole between
//!    the strips: a rounded fan at true corners, a polygon elsewhere.
//! 4. Where a hard edge meets soft edges, shrunken corners can land part way
//!    along a soft edge. Those points are inserted into the neighbouring faces
//!    so the mesh stays watertight (no T-junctions).

use std::collections::HashMap;

use glam::Vec3;

use crate::poly::PolyMesh;

/// Bevel hard edges (faces meeting at more than `angle_deg`). `amount` is the
/// bevel width; `segments` 1 gives a flat chamfer, 2 or more a rounded edge.
pub fn bevel(
    mesh: &PolyMesh,
    amount: f32,
    angle_deg: f32,
    segments: u32,
) -> Result<PolyMesh, String> {
    if !mesh.is_closed_manifold() {
        return Err("bevel needs a closed mesh (every edge shared by two faces)".into());
    }
    if !(1..=16).contains(&segments) {
        return Err("bevel segments must be from 1 to 16".into());
    }
    let edges = mesh.directed_edges();
    let normals: Vec<Vec3> = mesh.faces.iter().map(|f| mesh.face_normal(f)).collect();
    let cos_limit = angle_deg.to_radians().cos();
    let sharp = |a: u32, b: u32| -> bool {
        normals[edges[&(a, b)]].dot(normals[edges[&(b, a)]]) < cos_limit
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
    if amount <= 0.0 || !amount.is_finite() {
        return Err("bevel amount must be positive".into());
    }
    let r = amount.min(min_edge * 0.45);
    let rounded = segments > 1;

    let mut out = PolyMesh::default();

    // 1. Shrunken faces. corner[(face, vertex)] = new vertex id.
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
            let angle = u_prev
                .angle_between(u_next)
                .clamp(0.2, std::f32::consts::PI - 0.2);
            let offset = match (sharp(pv, v), sharp(v, nv)) {
                (true, true) => (u_prev + u_next).normalize_or_zero() * (r / (angle / 2.0).sin()),
                (false, true) => u_prev * (r / angle.sin()),
                (true, false) => u_next * (r / angle.sin()),
                (false, false) => Vec3::ZERO,
            };
            let id = out.add_vertex(p + offset);
            corner.insert((fi, v), id);
            new_face.push(id);
        }
        out.add_face(new_face, f.smooth);
    }

    // 2. Arcs. For hard edge (between faces f < g) at vertex v, the arc runs
    // from f's corner to g's corner, including both ends.
    let mut arcs: HashMap<(u32, usize, usize), Vec<u32>> = HashMap::new();
    let mut arc_centers: HashMap<u32, Vec<Vec3>> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            let g = edges[&(b, a)];
            if fi >= g || !sharp(a, b) {
                continue;
            }
            for v in [a, b] {
                let (pa, pb) = (
                    out.positions[corner[&(fi, v)] as usize],
                    out.positions[corner[&(g, v)] as usize],
                );
                let (na, nb) = (normals[fi], normals[g]);
                let (ca, cb) = (pa - na * r, pb - nb * r);
                arc_centers.entry(v).or_default().push((ca + cb) / 2.0);
                let mut ids = vec![corner[&(fi, v)]];
                for s in 1..segments {
                    let t = s as f32 / segments as f32;
                    let dir = na.lerp(nb, t).normalize_or_zero();
                    ids.push(out.add_vertex(ca.lerp(cb, t) + dir * r));
                }
                ids.push(corner[&(g, v)]);
                arcs.insert((v, fi, g), ids);
            }
            let (at_a, at_b) = (&arcs[&(a, fi, g)], &arcs[&(b, fi, g)]);
            for k in 0..segments as usize {
                out.add_face(vec![at_b[k], at_a[k], at_a[k + 1], at_b[k + 1]], rounded);
            }
        }
    }
    // Arc at v between faces f and g, ordered from f's side to g's side.
    let arc = |v: u32, f: usize, g: usize| -> Vec<u32> {
        if f < g {
            arcs[&(v, f, g)].clone()
        } else {
            arcs[&(v, g, f)].iter().rev().copied().collect()
        }
    };

    // 3. Vertex patches.
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
        let sharp_count = out_edges.iter().filter(|&&(_, b)| sharp(v, b)).count();
        if sharp_count == 0 {
            continue;
        }
        // Faces around v, with the far end of the edge leading to the next face.
        let start = out_edges[0].0;
        let mut ring = vec![(start, out_edges[0].1)];
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
            let next_b = out_edges
                .iter()
                .find(|(face, _)| *face == f)
                .expect("manifold")
                .1;
            ring.push((f, next_b));
            if ring.len() > out_edges.len() {
                return Err("bevel found a non-manifold vertex".into());
            }
        }
        // ring[i] -> ring[i+1] crosses edge (v, ring[i].1). Walk it backwards
        // so the patch winds opposite to the strips.
        let n = ring.len();
        let mut boundary: Vec<u32> = Vec::new();
        for i in (0..n).rev() {
            let (x, _) = ring[(i + 1) % n];
            let (y, b) = ring[i];
            // Edge between y and x at v is (v, b).
            if sharp(v, b) {
                let pts = arc(v, x, y);
                boundary.extend_from_slice(&pts[..pts.len() - 1]);
            } else {
                boundary.push(corner[&(x, v)]);
            }
        }
        let pts: Vec<Vec3> = boundary
            .iter()
            .map(|&i| out.positions[i as usize])
            .collect();
        let area = newell(&pts).length();
        if area < r * r * 1e-3 {
            continue; // flat crease: the strips already meet
        }
        if rounded && sharp_count >= 3 {
            let centers = &arc_centers[&v];
            let c = centers.iter().copied().sum::<Vec3>() / centers.len() as f32;
            let p = mesh.positions[v as usize];
            let tip = out.add_vertex(c + (p - c).normalize_or_zero() * r);
            for i in 0..boundary.len() {
                out.add_face(
                    vec![tip, boundary[i], boundary[(i + 1) % boundary.len()]],
                    true,
                );
            }
        } else {
            out.add_face(boundary, rounded);
        }
    }

    // 4. Close T-junctions along soft edges that touch hard ones.
    let mut groups: Vec<Vec<u32>> = Vec::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            let g = edges[&(b, a)];
            if fi < g && !sharp(a, b) {
                let mut ids = vec![
                    corner[&(fi, a)],
                    corner[&(g, a)],
                    corner[&(fi, b)],
                    corner[&(g, b)],
                ];
                ids.sort_unstable();
                ids.dedup();
                if ids.len() > 2 {
                    groups.push(ids);
                }
            }
        }
    }
    insert_collinear(&mut out, &groups, r * 1e-3);

    // Corners that did not move coincide; merge them and drop collapsed faces.
    out.weld(r * 1e-3);
    let positions = out.positions.clone();
    out.faces.retain(|f| {
        let pts: Vec<Vec3> = f.verts.iter().map(|&i| positions[i as usize]).collect();
        newell(&pts).length() > r * r * 1e-6
    });
    out.remove_unused_vertices();
    Ok(out)
}

fn newell(pts: &[Vec3]) -> Vec3 {
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        n += Vec3::new(
            (p.y - q.y) * (p.z + q.z),
            (p.z - q.z) * (p.x + q.x),
            (p.x - q.x) * (p.y + q.y),
        );
    }
    n
}

/// For each group of vertices lying on one line, insert into every face edge
/// joining two of them the group's other vertices that lie strictly between.
fn insert_collinear(mesh: &mut PolyMesh, groups: &[Vec<u32>], eps: f32) {
    let mut member: HashMap<u32, Vec<usize>> = HashMap::new();
    for (gi, g) in groups.iter().enumerate() {
        for &v in g {
            member.entry(v).or_default().push(gi);
        }
    }
    for fi in 0..mesh.faces.len() {
        let verts = mesh.faces[fi].verts.clone();
        let n = verts.len();
        let mut new_verts = Vec::with_capacity(n);
        for i in 0..n {
            let (p, q) = (verts[i], verts[(i + 1) % n]);
            new_verts.push(p);
            let (Some(gp), Some(gq)) = (member.get(&p), member.get(&q)) else {
                continue;
            };
            let (pp, pq) = (mesh.positions[p as usize], mesh.positions[q as usize]);
            let len = pp.distance(pq);
            if len <= eps {
                continue;
            }
            let dir = (pq - pp) / len;
            let mut between: Vec<(f32, u32)> = Vec::new();
            for gi in gp.iter().filter(|g| gq.contains(g)) {
                for &w in &groups[*gi] {
                    if w == p || w == q || between.iter().any(|&(_, x)| x == w) {
                        continue;
                    }
                    let pw = mesh.positions[w as usize];
                    let t = (pw - pp).dot(dir);
                    let off_line = (pw - pp - dir * t).length();
                    if t > eps && t < len - eps && off_line < eps {
                        between.push((t, w));
                    }
                }
            }
            between.sort_by(|a, b| a.0.total_cmp(&b.0));
            new_verts.extend(between.into_iter().map(|(_, w)| w));
        }
        mesh.faces[fi].verts = new_verts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{extrude, subdivide, Selection};
    use crate::primitives::{cube, cylinder, uv_sphere};

    #[test]
    fn chamfer_cube() {
        let m = bevel(&cube(1.0), 0.1, 30.0, 1).unwrap();
        assert!(m.is_closed_manifold());
        assert_eq!(m.faces.len(), 6 + 12 + 8);
        let v = m.volume();
        assert!(v < 1.0 && v > 0.9, "volume {v}");
        assert!(
            (m.bounds().unwrap().size() - Vec3::ONE).length() < 1e-5,
            "bevel keeps outer size"
        );
    }

    #[test]
    fn rounded_cube_corners_lie_on_a_sphere() {
        let r = 0.2;
        let m = bevel(&cube(2.0), r, 30.0, 4).unwrap();
        assert!(m.is_closed_manifold());
        assert!((m.bounds().unwrap().size() - Vec3::splat(2.0)).length() < 1e-4);
        // Exact volume of a box with rounded edges and corners.
        let (a, pi) = (2.0 - 2.0 * r, std::f32::consts::PI);
        let exact = a * a * a + 6.0 * a * a * r + 3.0 * pi * r * r * a + 4.0 / 3.0 * pi * r * r * r;
        let v = m.volume();
        assert!(
            v < exact + 1e-3 && v > exact * 0.995,
            "volume {v}, exact {exact}"
        );
        // Every corner-patch vertex is at distance r from its corner's sphere centre.
        let centre = Vec3::splat(1.0 - r);
        for p in m
            .positions
            .iter()
            .filter(|p| p.cmpgt(Vec3::splat(1.0 - r + 1e-4)).all())
        {
            assert!(
                ((*p - centre).length() - r).abs() < 0.02 * r,
                "{p} is off the sphere"
            );
        }
    }

    #[test]
    fn rounded_cylinder_rims() {
        let c = cylinder(0.5, 1.0, 32);
        let m = bevel(&c, 0.05, 30.0, 3).unwrap();
        assert!(m.is_closed_manifold());
        // Sides, caps, 3-segment strips on both rims, plus a thin patch per rim vertex.
        assert_eq!(m.faces.len(), 32 + 2 + 2 * 32 * 3 + 64);
        assert!(m.volume() < c.volume());
    }

    #[test]
    fn sphere_has_no_hard_edges() {
        assert!(bevel(&uv_sphere(1.0, 16, 8), 0.05, 30.0, 2).is_err());
    }

    /// A vertex where one hard edge meets several soft ones used to leave a
    /// T-junction. Build one: split a cube's top into 4 and raise one quarter.
    fn stepped_block() -> PolyMesh {
        let m = subdivide(&cube(2.0), 1, false).unwrap();
        let quarter = m
            .faces
            .iter()
            .position(|f| {
                let c = m.face_centroid(f);
                m.face_normal(f).y > 0.9 && c.x > 0.0 && c.z > 0.0
            })
            .unwrap();
        extrude(
            &m,
            &Selection::Faces {
                indices: vec![quarter],
            },
            0.5,
        )
        .unwrap()
    }

    #[test]
    fn mixed_hard_and_soft_edges_stay_watertight() {
        let base = stepped_block();
        assert!(base.is_closed_manifold());
        for segments in [1, 3] {
            let m = bevel(&base, 0.1, 30.0, segments).unwrap();
            assert!(
                m.is_closed_manifold(),
                "segments {segments}: not watertight"
            );
            let (v0, v) = (base.volume(), m.volume());
            assert!(v < v0 && v > v0 * 0.9, "segments {segments}: {v0} -> {v}");
        }
    }

    #[test]
    fn bevel_twice() {
        let once = bevel(&cube(1.0), 0.1, 30.0, 1).unwrap();
        let twice = bevel(&once, 0.02, 30.0, 2).unwrap();
        assert!(twice.is_closed_manifold());
    }
}
