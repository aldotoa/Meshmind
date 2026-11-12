//! Polygon reduction by quadric error edge collapse (Garland and Heckbert).
//!
//! Each vertex carries a quadric: the summed squared distance to the planes
//! of its original faces. Collapsing the edge whose merged quadric costs
//! least keeps the shape where it matters (silhouettes, creases) and spends
//! fewer triangles where the surface is flat or gently curved.
//!
//! Collapses that would tear the surface (non-manifold links), flip a face
//! or squash one to a sliver are refused. With UVs, vertices are split along
//! UV seams and seam edges are kept, so the texture layout stays valid; a
//! collapse then moves a vertex onto its neighbour (keeping its UV) instead
//! of to the optimal point.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{DMat3, DVec3, Vec2, Vec3};

use crate::poly::{Face, PolyMesh};

#[derive(Clone, Copy, Default)]
struct Quadric {
    // Symmetric 4x4 matrix, upper triangle: a2 ab ac ad b2 bc bd c2 cd d2.
    m: [f64; 10],
}

impl Quadric {
    fn plane(n: DVec3, d: f64, w: f64) -> Quadric {
        let (a, b, c) = (n.x, n.y, n.z);
        Quadric {
            m: [
                a * a * w,
                a * b * w,
                a * c * w,
                a * d * w,
                b * b * w,
                b * c * w,
                b * d * w,
                c * c * w,
                c * d * w,
                d * d * w,
            ],
        }
    }
    fn add(&self, o: &Quadric) -> Quadric {
        let mut m = self.m;
        for (a, b) in m.iter_mut().zip(o.m) {
            *a += b;
        }
        Quadric { m }
    }
    fn error(&self, p: DVec3) -> f64 {
        let m = &self.m;
        let (x, y, z) = (p.x, p.y, p.z);
        m[0] * x * x
            + 2.0 * m[1] * x * y
            + 2.0 * m[2] * x * z
            + 2.0 * m[3] * x
            + m[4] * y * y
            + 2.0 * m[5] * y * z
            + 2.0 * m[6] * y
            + m[7] * z * z
            + 2.0 * m[8] * z
            + m[9]
    }
    /// The point with the least error, if the system is well conditioned.
    fn optimum(&self) -> Option<DVec3> {
        let m = &self.m;
        let a = DMat3::from_cols(
            DVec3::new(m[0], m[1], m[2]),
            DVec3::new(m[1], m[4], m[5]),
            DVec3::new(m[2], m[5], m[7]),
        );
        let det = a.determinant();
        if det.abs() < 1e-12 {
            return None;
        }
        Some(a.inverse() * -DVec3::new(m[3], m[6], m[8]))
    }
}

struct Candidate {
    cost: f64,
    a: u32,
    b: u32,
    stamp: (u32, u32),
    target: DVec3,
}
impl PartialEq for Candidate {
    fn eq(&self, o: &Self) -> bool {
        self.cost == o.cost
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Candidate {
    fn cmp(&self, o: &Self) -> Ordering {
        // Min-heap on cost.
        o.cost.total_cmp(&self.cost)
    }
}

/// Reduce `mesh` to about `target_triangles`. Returns a triangle mesh; faces
/// keep their smooth flag, and UVs when the input has them.
pub fn simplify(mesh: &PolyMesh, target_triangles: usize) -> Result<PolyMesh, String> {
    if mesh.faces.is_empty() {
        return Err("the mesh has no faces".into());
    }
    let with_uv = mesh.has_uvs();

    // Triangles over the welded vertices; UVs travel per corner, so a UV
    // seam is just an edge whose two triangles disagree about its UVs.
    let mut pos: Vec<DVec3> = mesh.positions.iter().map(|p| p.as_dvec3()).collect();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut tri_uv: Vec<[Vec2; 3]> = Vec::new();
    let mut tri_smooth: Vec<bool> = Vec::new();
    for f in &mesh.faces {
        for t in mesh.triangulate_face(f) {
            let ids = t.map(|k| f.verts[k]);
            if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
                tris.push(ids);
                tri_uv.push(if with_uv {
                    t.map(|k| f.uvs[k])
                } else {
                    [Vec2::ZERO; 3]
                });
                tri_smooth.push(f.smooth);
            }
        }
    }
    let nv = pos.len();
    let target = target_triangles.max(4);
    if tris.len() <= target {
        return Ok(to_poly(
            &pos,
            &tris,
            &tri_uv,
            &tri_smooth,
            &vec![true; tris.len()],
            with_uv,
        ));
    }

    // Vertex -> triangles, and open-boundary vertices (kept in place).
    let mut vt: Vec<Vec<u32>> = vec![Vec::new(); nv];
    for (i, t) in tris.iter().enumerate() {
        for &v in t {
            vt[v as usize].push(i as u32);
        }
    }
    let mut edge_count: HashMap<(u32, u32), u32> = HashMap::new();
    for t in &tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *edge_count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut locked = vec![false; nv];
    for (&(a, b), &c) in &edge_count {
        if c != 2 {
            locked[a as usize] = true;
            locked[b as usize] = true;
        }
    }

    // Quadrics from area-weighted face planes.
    let mut q = vec![Quadric::default(); nv];
    for t in &tris {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let n = (b - a).cross(c - a);
        let area = n.length();
        if area < 1e-18 {
            continue;
        }
        let n = n / area;
        let pq = Quadric::plane(n, -n.dot(a), area);
        for &v in t {
            q[v as usize] = q[v as usize].add(&pq);
        }
    }

    let mut alive = vec![true; tris.len()];
    let mut alive_count = tris.len();
    let mut stamp = vec![0u32; nv];
    let mut heap = BinaryHeap::new();
    let candidate = |a: u32,
                     b: u32,
                     q: &[Quadric],
                     pos: &[DVec3],
                     locked: &[bool],
                     stamp: &[u32]|
     -> Option<Candidate> {
        let (la, lb) = (locked[a as usize], locked[b as usize]);
        if la && lb {
            return None;
        }
        // Collapse b into a: keep the locked one in place.
        let (a, b) = if lb { (b, a) } else { (a, b) };
        let qq = q[a as usize].add(&q[b as usize]);
        let (pa, pb) = (pos[a as usize], pos[b as usize]);
        let target = if locked[a as usize] || with_uv {
            // Keep a's position (and UV).
            let (ea, eb) = (qq.error(pa), qq.error(pb));
            if !locked[a as usize] && eb < ea {
                return Some(Candidate {
                    cost: eb,
                    a: b,
                    b: a,
                    stamp: (stamp[b as usize], stamp[a as usize]),
                    target: pb,
                });
            }
            pa
        } else {
            qq.optimum()
                .filter(|o| (*o - (pa + pb) * 0.5).length() < (pa - pb).length() * 2.0)
                .unwrap_or((pa + pb) * 0.5)
        };
        Some(Candidate {
            cost: qq.error(target).max(0.0),
            a,
            b,
            stamp: (stamp[a as usize], stamp[b as usize]),
            target,
        })
    };
    for &(a, b) in edge_count.keys() {
        if let Some(c) = candidate(a, b, &q, &pos, &locked, &stamp) {
            heap.push(c);
        }
    }

    let neighbours = |v: u32, vt: &[Vec<u32>], alive: &[bool], tris: &[[u32; 3]]| -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for &t in &vt[v as usize] {
            if !alive[t as usize] {
                continue;
            }
            for &w in &tris[t as usize] {
                if w != v && !out.contains(&w) {
                    out.push(w);
                }
            }
        }
        out
    };

    while alive_count > target {
        let Some(c) = heap.pop() else { break };
        let (a, b) = (c.a, c.b);
        if stamp[a as usize] != c.stamp.0 || stamp[b as usize] != c.stamp.1 {
            continue;
        }
        // Still an edge?
        let shared: Vec<u32> = vt[a as usize]
            .iter()
            .copied()
            .filter(|&t| alive[t as usize] && tris[t as usize].contains(&b))
            .collect();
        if shared.len() != 2 {
            continue;
        }
        // UVs: every distinct UV that b has must appear in one of the two
        // removed triangles, which says what a's UV is on that side. A seam
        // vertex can then slide along its seam but never across it.
        let mut uv_map: Vec<(Vec2, Vec2)> = Vec::new();
        if with_uv {
            let corner_uv = |t: u32, v: u32| {
                let k = tris[t as usize]
                    .iter()
                    .position(|&w| w == v)
                    .expect("corner");
                tri_uv[t as usize][k]
            };
            for &t in &shared {
                uv_map.push((corner_uv(t, b), corner_uv(t, a)));
            }
            let all_mapped = vt[b as usize]
                .iter()
                .filter(|&&t| alive[t as usize])
                .all(|&t| {
                    let ub = corner_uv(t, b);
                    uv_map.iter().any(|(from, _)| *from == ub)
                });
            if !all_mapped {
                continue;
            }
        }
        // Link condition: a and b share exactly the two opposite vertices.
        let na = neighbours(a, &vt, &alive, &tris);
        let nb = neighbours(b, &vt, &alive, &tris);
        let common = na.iter().filter(|v| nb.contains(v)).count();
        if common != 2 {
            continue;
        }
        // No flipped or squashed faces around b (and a) after the move.
        let p_new = c.target;
        let mut ok = true;
        for &v in &[a, b] {
            for &t in &vt[v as usize] {
                if !alive[t as usize] || shared.contains(&t) {
                    continue;
                }
                let tri = tris[t as usize];
                let p = |w: u32| {
                    if w == a || w == b {
                        p_new
                    } else {
                        pos[w as usize]
                    }
                };
                let old = (pos[tri[1] as usize] - pos[tri[0] as usize])
                    .cross(pos[tri[2] as usize] - pos[tri[0] as usize]);
                let new = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                let (ol, nl) = (old.length(), new.length());
                if nl < 1e-14 || old.dot(new) < 0.3 * ol * nl {
                    ok = false;
                    break;
                }
                // Avoid slivers: compare area with the longest edge squared.
                let e = [p(tri[0]), p(tri[1]), p(tri[2])];
                let longest = (e[0] - e[1])
                    .length_squared()
                    .max((e[1] - e[2]).length_squared())
                    .max((e[2] - e[0]).length_squared());
                if nl < longest * 0.02 {
                    ok = false;
                    break;
                }
            }
            if !ok {
                break;
            }
        }
        if !ok {
            continue;
        }
        // Collapse b into a.
        pos[a as usize] = p_new;
        q[a as usize] = q[a as usize].add(&q[b as usize]);
        for &t in &shared {
            alive[t as usize] = false;
            alive_count -= 1;
        }
        let moved: Vec<u32> = vt[b as usize].clone();
        for t in moved {
            if !alive[t as usize] {
                continue;
            }
            for k in 0..3 {
                if tris[t as usize][k] == b {
                    tris[t as usize][k] = a;
                    if with_uv {
                        let ub = tri_uv[t as usize][k];
                        if let Some((_, ua)) = uv_map.iter().find(|(from, _)| *from == ub) {
                            tri_uv[t as usize][k] = *ua;
                        }
                    }
                }
            }
            vt[a as usize].push(t);
        }
        vt[a as usize].retain(|&t| alive[t as usize]);
        vt[b as usize].clear();
        stamp[a as usize] += 1;
        stamp[b as usize] += 1;
        for w in neighbours(a, &vt, &alive, &tris) {
            stamp[w as usize] += 1;
        }
        for w in neighbours(a, &vt, &alive, &tris) {
            if let Some(c) = candidate(a, w, &q, &pos, &locked, &stamp) {
                heap.push(c);
            }
            for x in neighbours(w, &vt, &alive, &tris) {
                if x != a {
                    if let Some(c) = candidate(w, x, &q, &pos, &locked, &stamp) {
                        heap.push(c);
                    }
                }
            }
        }
    }
    Ok(to_poly(&pos, &tris, &tri_uv, &tri_smooth, &alive, with_uv))
}

fn to_poly(
    pos: &[DVec3],
    tris: &[[u32; 3]],
    uvs: &[[Vec2; 3]],
    smooth: &[bool],
    alive: &[bool],
    with_uv: bool,
) -> PolyMesh {
    let mut out = PolyMesh::default();
    let mut remap = vec![u32::MAX; pos.len()];
    for (i, t) in tris.iter().enumerate() {
        if !alive[i] {
            continue;
        }
        for &v in t {
            if remap[v as usize] == u32::MAX {
                let p = pos[v as usize];
                remap[v as usize] = out.add_vertex(Vec3::new(p.x as f32, p.y as f32, p.z as f32));
            }
        }
        let mut f = Face::new(t.map(|v| remap[v as usize]).to_vec(), smooth[i]);
        if with_uv {
            f.uvs = uvs[i].to_vec();
        }
        out.faces.push(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdf::{build, ellipsoid, MeshOptions, SdfPart};

    #[test]
    fn halves_a_sphere_and_keeps_it_round_and_closed() {
        let m = build(
            &[SdfPart::add(
                "b",
                ellipsoid(Vec3::ZERO, Vec3::splat(0.5)),
                0.0,
            )],
            MeshOptions {
                resolution: 48,
                ..Default::default()
            },
        )
        .unwrap();
        let tris: usize = m.faces.iter().map(|f| f.verts.len() - 2).sum();
        let out = simplify(&m, tris / 10).unwrap();
        assert!(
            out.faces.len() <= tris / 10 + 2,
            "{} faces",
            out.faces.len()
        );
        assert!(out.is_closed_manifold(), "still watertight");
        for p in &out.positions {
            assert!((p.length() - 0.5).abs() < 0.02, "vertex drifted: {p}");
        }
        assert!((out.volume() - m.volume()).abs() / m.volume() < 0.05);
    }

    #[test]
    fn keeps_uv_seams() {
        let mut m = crate::primitives::uv_sphere(0.5, 32, 16);
        crate::uv::unwrap(&mut m, &Default::default()).unwrap();
        let out = simplify(&m, 200).unwrap();
        assert!(out.has_uvs());
        assert!(out.faces.len() < m.faces.len() * 2);
        assert!(out.is_closed_manifold());
    }
}
