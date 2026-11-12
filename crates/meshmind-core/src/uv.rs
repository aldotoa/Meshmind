//! Automatic UV unwrapping, needed before textures can be baked.
//!
//! 1. Split the surface into charts: neighbouring faces join a chart while
//!    the edge between them is gentle and they face roughly the chart's way.
//!    Hard edges become seams, which suits hard-surface models.
//! 2. Flatten each chart by projecting it onto its average plane, rotated to
//!    its tightest bounding box.
//! 3. Pack all charts into the unit square at one shared texel density, with
//!    padding so texture filtering does not bleed between charts.

use std::collections::{HashMap, VecDeque};

use glam::{Vec2, Vec3};

use crate::poly::PolyMesh;

pub struct UnwrapOptions {
    /// Neighbouring faces whose normals differ by more than this start a new chart.
    pub max_edge_angle_deg: f32,
    /// Faces further than this from the chart's first face start a new chart.
    pub max_chart_angle_deg: f32,
    /// Gap between charts, as a fraction of the texture size.
    pub padding: f32,
}

impl Default for UnwrapOptions {
    fn default() -> Self {
        Self {
            max_edge_angle_deg: 40.0,
            max_chart_angle_deg: 45.0,
            padding: 0.01,
        }
    }
}

/// Statistics about an unwrap, reported back to the agent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnwrapStats {
    pub charts: usize,
    /// Fraction of the unit square covered by charts (higher is better).
    pub coverage: f32,
}

struct Chart {
    faces: Vec<usize>,
    /// Per face, per corner, flattened coordinates (in object units).
    uvs: Vec<Vec<Vec2>>,
    min: Vec2,
    size: Vec2,
}

pub fn unwrap(mesh: &mut PolyMesh, opts: &UnwrapOptions) -> Result<UnwrapStats, String> {
    if mesh.faces.is_empty() {
        return Err("the mesh has no faces".into());
    }
    let normals: Vec<Vec3> = mesh.faces.iter().map(|f| mesh.face_normal(f)).collect();
    let areas: Vec<f32> = mesh
        .faces
        .iter()
        .map(|f| mesh.face_normal_area(f).length() / 2.0)
        .collect();

    // Undirected edge -> faces.
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            edge_faces.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }

    // 1. Grow charts, largest faces first so charts start from big flat areas.
    let edge_cos = opts.max_edge_angle_deg.to_radians().cos();
    let chart_cos = opts.max_chart_angle_deg.to_radians().cos();
    let mut order: Vec<usize> = (0..mesh.faces.len()).collect();
    order.sort_by(|&a, &b| areas[b].total_cmp(&areas[a]));
    let mut chart_of = vec![usize::MAX; mesh.faces.len()];
    let mut charts_faces: Vec<Vec<usize>> = Vec::new();
    for &seed in &order {
        if chart_of[seed] != usize::MAX {
            continue;
        }
        let id = charts_faces.len();
        let seed_n = normals[seed];
        let mut faces = vec![seed];
        chart_of[seed] = id;
        let mut queue = VecDeque::from([seed]);
        while let Some(f) = queue.pop_front() {
            for (a, b) in mesh.faces[f].edges() {
                for &g in &edge_faces[&(a.min(b), a.max(b))] {
                    if chart_of[g] == usize::MAX
                        && normals[g].dot(normals[f]) >= edge_cos
                        && normals[g].dot(seed_n) >= chart_cos
                    {
                        chart_of[g] = id;
                        faces.push(g);
                        queue.push_back(g);
                    }
                }
            }
        }
        charts_faces.push(faces);
    }

    // 2. Flatten each chart.
    let mut charts: Vec<Chart> = charts_faces
        .into_iter()
        .map(|faces| {
            let n = faces
                .iter()
                .map(|&f| normals[f] * areas[f])
                .sum::<Vec3>()
                .normalize_or(normals[faces[0]]);
            let (t, mut b) = n.any_orthonormal_pair();
            if t.cross(b).dot(n) < 0.0 {
                b = -b; // keep charts unmirrored
            }
            let mut uvs: Vec<Vec<Vec2>> = faces
                .iter()
                .map(|&f| {
                    mesh.faces[f]
                        .verts
                        .iter()
                        .map(|&v| {
                            let p = mesh.positions[v as usize];
                            Vec2::new(p.dot(t), p.dot(b))
                        })
                        .collect()
                })
                .collect();
            // Rotate to the tightest box, trying the direction of each edge.
            let mut best = (f32::INFINITY, 0.0f32);
            for poly in uvs.iter().take(64) {
                for i in 0..poly.len() {
                    let d = poly[(i + 1) % poly.len()] - poly[i];
                    if d.length_squared() < 1e-12 {
                        continue;
                    }
                    let angle = -d.y.atan2(d.x);
                    let (min, max) = bounds(&uvs, angle);
                    let area = (max - min).x * (max - min).y;
                    if area < best.0 {
                        best = (area, angle);
                    }
                }
            }
            let rot = Vec2::from_angle(best.1);
            for poly in &mut uvs {
                for p in poly.iter_mut() {
                    *p = rot.rotate(*p);
                }
            }
            let (min, max) = bounds(&uvs, 0.0);
            // Put the longer side horizontal; shelf packing likes wide charts.
            let size = max - min;
            if size.y > size.x {
                for poly in &mut uvs {
                    for p in poly.iter_mut() {
                        *p = Vec2::new(p.y, -p.x);
                    }
                }
            }
            let (min, max) = bounds(&uvs, 0.0);
            Chart {
                faces,
                uvs,
                min,
                size: (max - min).max(Vec2::splat(1e-6)),
            }
        })
        .collect();

    // 3. Shelf packing. Find the smallest square side that fits.
    charts.sort_by(|a, b| b.size.y.total_cmp(&a.size.y));
    let total: f32 = charts.iter().map(|c| c.size.x * c.size.y).sum();
    let widest = charts.iter().map(|c| c.size.x).fold(0.0f32, f32::max);
    let mut side = total.sqrt().max(widest);
    let placements = loop {
        let pad = side * opts.padding;
        if let Some(p) = shelf_pack(&charts, side, pad) {
            break p;
        }
        side *= 1.05;
    };
    let pad = side * opts.padding;
    let scale = 1.0 / (side + pad);
    for (chart, offset) in charts.iter().zip(&placements) {
        for (&f, poly) in chart.faces.iter().zip(&chart.uvs) {
            mesh.faces[f].uvs = poly
                .iter()
                .map(|p| (*p - chart.min + *offset) * scale)
                .collect();
        }
    }
    let coverage = total * scale * scale;
    Ok(UnwrapStats {
        charts: charts.len(),
        coverage,
    })
}

fn bounds(uvs: &[Vec<Vec2>], angle: f32) -> (Vec2, Vec2) {
    let rot = Vec2::from_angle(angle);
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for p in uvs.iter().flatten() {
        let q = rot.rotate(*p);
        min = min.min(q);
        max = max.max(q);
    }
    (min, max)
}

/// Place charts left to right in rows. Returns each chart's lower-left offset.
fn shelf_pack(charts: &[Chart], side: f32, pad: f32) -> Option<Vec<Vec2>> {
    let mut out = Vec::with_capacity(charts.len());
    let (mut x, mut y, mut row_h) = (pad, pad, 0.0f32);
    for c in charts {
        if x + c.size.x + pad > side + pad && x > pad {
            x = pad;
            y += row_h + pad;
            row_h = 0.0;
        }
        if y + c.size.y > side + 1e-6 || x + c.size.x > side + 1e-6 {
            return None;
        }
        out.push(Vec2::new(x, y));
        x += c.size.x + pad;
        row_h = row_h.max(c.size.y);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, cylinder, uv_sphere};

    fn check(mut m: PolyMesh) -> UnwrapStats {
        let stats = unwrap(&mut m, &UnwrapOptions::default()).unwrap();
        assert!(m.has_uvs());
        for f in &m.faces {
            for uv in &f.uvs {
                assert!(
                    uv.cmpge(Vec2::ZERO).all() && uv.cmple(Vec2::ONE).all(),
                    "uv {uv} outside the unit square"
                );
            }
        }
        // Texel density is uniform: UV area / surface area is the same for every face.
        let ratios: Vec<f32> = m
            .faces
            .iter()
            .filter_map(|f| {
                let a3 = m.face_normal_area(f).length();
                let mut a2 = 0.0;
                for i in 0..f.uvs.len() {
                    let (p, q) = (f.uvs[i], f.uvs[(i + 1) % f.uvs.len()]);
                    a2 += p.perp_dot(q);
                }
                (a3 > 1e-6).then(|| a2.abs() / a3)
            })
            .collect();
        let (lo, hi) = ratios
            .iter()
            .fold((f32::INFINITY, 0.0f32), |(l, h), r| (l.min(*r), h.max(*r)));
        assert!(hi / lo < 1.6, "density varies {lo}..{hi}");
        // Faces keep their orientation in UV space (no mirrored charts).
        for f in &m.faces {
            let mut a2 = 0.0;
            for i in 0..f.uvs.len() {
                a2 += f.uvs[i].perp_dot(f.uvs[(i + 1) % f.uvs.len()]);
            }
            assert!(a2 >= -1e-9, "mirrored face");
        }
        stats
    }

    #[test]
    fn cube_gets_six_charts() {
        let s = check(cube(1.0));
        assert_eq!(s.charts, 6);
        assert!(s.coverage > 0.4, "coverage {}", s.coverage);
    }

    #[test]
    fn cylinder_and_sphere_unwrap() {
        check(cylinder(0.5, 2.0, 32));
        check(uv_sphere(1.0, 24, 12));
    }

    #[test]
    fn charts_do_not_overlap() {
        let mut m = cube(1.0);
        unwrap(&mut m, &UnwrapOptions::default()).unwrap();
        let boxes: Vec<(Vec2, Vec2)> = m
            .faces
            .iter()
            .map(|f| {
                f.uvs.iter().fold((Vec2::ONE, Vec2::ZERO), |(lo, hi), p| {
                    (lo.min(*p), hi.max(*p))
                })
            })
            .collect();
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                let (a, b) = (boxes[i], boxes[j]);
                let overlap = a.0.x < b.1.x - 1e-6
                    && b.0.x < a.1.x - 1e-6
                    && a.0.y < b.1.y - 1e-6
                    && b.0.y < a.1.y - 1e-6;
                assert!(!overlap, "faces {i} and {j} overlap");
            }
        }
    }
}
