//! Shapes built from outlines: the tools artists use for blades, handles,
//! pommels, bottles, pipes and rope.
//!
//! - [`loft`]: one cross-section shape, scaled and moved along Y at each
//!   station (a sword blade: lens-shaped section, tapering to a point).
//! - [`lathe`]: a side profile spun around Y (pommels, vases, wheels).
//! - [`extrude_outline`]: a flat outline given thickness (guards, axe heads, signs).
//! - [`sweep`]: a round tube along a path (grip wraps, cables, rope, rails).
//!
//! All results are closed, outward-facing meshes with smooth faces; the
//! renderer keeps edges sharper than 50 degrees crisp.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};
use serde::Deserialize;

use crate::poly::PolyMesh;

const EPS: f32 = 1e-6;

/// One station of a loft.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Station {
    /// Height of this cross-section.
    pub y: f32,
    /// Scale of the shape in X and Z. Zero (or close) collapses to a point.
    #[serde(default = "one2")]
    pub scale: Vec2,
    /// Shift of the cross-section centre in X and Z.
    #[serde(default)]
    pub offset: Vec2,
}

fn one2() -> Vec2 {
    Vec2::ONE
}

fn check_outline(shape: &[Vec2], what: &str) -> Result<(), String> {
    if shape.len() < 3 {
        return Err(format!("{what} needs at least 3 points"));
    }
    if shape.iter().any(|p| !p.is_finite()) {
        return Err(format!("{what} has non-finite points"));
    }
    if signed_area(shape).abs() < 1e-10 {
        return Err(format!("{what} has no area"));
    }
    Ok(())
}

fn signed_area(pts: &[Vec2]) -> f32 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].perp_dot(pts[(i + 1) % n]))
        .sum::<f32>()
        * 0.5
}

/// Make every face point outwards (positive volume).
fn orient(mut m: PolyMesh) -> PolyMesh {
    if m.volume() < 0.0 {
        for f in &mut m.faces {
            f.verts.reverse();
        }
    }
    m
}

/// Loft a closed cross-section (`shape`, points in the XZ plane) through
/// stations along Y. Ends are capped; a station with zero scale becomes a point.
pub fn loft(shape: &[Vec2], stations: &[Station]) -> Result<PolyMesh, String> {
    check_outline(shape, "loft shape")?;
    if stations.len() < 2 {
        return Err("loft needs at least 2 stations".into());
    }
    if stations.windows(2).any(|w| w[1].y <= w[0].y) {
        return Err("station heights must increase".into());
    }
    let n = shape.len();
    let mut m = PolyMesh::default();
    // Each ring is either n vertices or a single tip vertex.
    let rings: Vec<Vec<u32>> = stations
        .iter()
        .map(|s| {
            let at = |p: Vec2| {
                let q = p * s.scale + s.offset;
                Vec3::new(q.x, s.y, q.y)
            };
            if s.scale.max_element() < EPS {
                vec![m.add_vertex(at(Vec2::ZERO))]
            } else {
                shape.iter().map(|&p| m.add_vertex(at(p))).collect()
            }
        })
        .collect();
    for w in rings.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        match (a.len(), b.len()) {
            (1, 1) => return Err("two neighbouring stations are both points".into()),
            (1, _) => (0..n).for_each(|i| m.add_face(vec![a[0], b[(i + 1) % n], b[i]], true)),
            (_, 1) => (0..n).for_each(|i| m.add_face(vec![a[i], a[(i + 1) % n], b[0]], true)),
            _ => (0..n).for_each(|i| {
                let j = (i + 1) % n;
                m.add_face(vec![a[i], a[j], b[j], b[i]], true)
            }),
        }
    }
    for ring in [rings.first().unwrap(), rings.last().unwrap()] {
        if ring.len() > 1 {
            m.add_face(ring.clone(), false);
        }
    }
    Ok(finish(m))
}

/// Spin a side profile around the Y axis. `profile` points are (radius, y),
/// listed from one end to the other. Points on the axis (radius 0) close the
/// shape there; open ends get a flat cap.
pub fn lathe(profile: &[Vec2], segments: u32) -> Result<PolyMesh, String> {
    if profile.len() < 2 {
        return Err("lathe profile needs at least 2 points".into());
    }
    if profile.iter().any(|p| !p.is_finite() || p.x < 0.0) {
        return Err("lathe radii must be 0 or more".into());
    }
    let segments = segments.clamp(3, 256);
    let mut m = PolyMesh::default();
    let rings: Vec<Vec<u32>> = profile
        .iter()
        .map(|p| {
            if p.x < EPS {
                vec![m.add_vertex(Vec3::new(0.0, p.y, 0.0))]
            } else {
                (0..segments)
                    .map(|s| {
                        let a = s as f32 / segments as f32 * TAU;
                        m.add_vertex(Vec3::new(p.x * a.cos(), p.y, -p.x * a.sin()))
                    })
                    .collect()
            }
        })
        .collect();
    let n = segments as usize;
    for w in rings.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        match (a.len(), b.len()) {
            (1, 1) => return Err("two neighbouring profile points are both on the axis".into()),
            (1, _) => (0..n).for_each(|i| m.add_face(vec![a[0], b[i], b[(i + 1) % n]], true)),
            (_, 1) => (0..n).for_each(|i| m.add_face(vec![a[i], b[0], a[(i + 1) % n]], true)),
            _ => (0..n).for_each(|i| {
                let j = (i + 1) % n;
                m.add_face(vec![a[i], b[i], b[j], a[j]], true)
            }),
        }
    }
    for ring in [rings.first().unwrap(), rings.last().unwrap()] {
        if ring.len() > 1 {
            m.add_face(ring.clone(), false);
        }
    }
    Ok(finish(m))
}

/// Give a flat outline (points in the XY plane) a thickness along Z,
/// centred on z = 0. Concave outlines are fine.
pub fn extrude_outline(outline: &[Vec2], depth: f32) -> Result<PolyMesh, String> {
    check_outline(outline, "outline")?;
    if depth.is_nan() || depth <= 0.0 {
        return Err("depth must be positive".into());
    }
    let n = outline.len();
    let mut m = PolyMesh::default();
    let h = depth / 2.0;
    let front: Vec<u32> = outline
        .iter()
        .map(|p| m.add_vertex(Vec3::new(p.x, p.y, h)))
        .collect();
    let back: Vec<u32> = outline
        .iter()
        .map(|p| m.add_vertex(Vec3::new(p.x, p.y, -h)))
        .collect();
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_face(vec![front[i], back[i], back[j], front[j]], true);
    }
    m.add_face(front.clone(), false);
    m.add_face(back.iter().rev().copied().collect(), false);
    Ok(finish(m))
}

/// A round tube of `radius` following `path`. Closed paths (first point equal
/// to the last) join up; open ones get flat caps. `radii` optionally gives a
/// radius per path point.
pub fn sweep(
    path: &[Vec3],
    radius: f32,
    radii: Option<&[f32]>,
    segments: u32,
) -> Result<PolyMesh, String> {
    let mut path: Vec<Vec3> = path.to_vec();
    path.dedup_by(|a, b| a.distance(*b) < EPS);
    if path.len() < 2 {
        return Err("sweep path needs at least 2 distinct points".into());
    }
    if let Some(r) = radii {
        if r.len() != path.len() {
            return Err("`radii` needs one value per path point".into());
        }
    }
    let closed = path.len() > 3 && path[0].distance(*path.last().unwrap()) < 1e-4;
    if closed {
        path.pop();
    }
    let count = path.len();
    let segments = segments.clamp(3, 64) as usize;
    let tangent = |i: usize| -> Vec3 {
        let prev = if i > 0 {
            path[i - 1]
        } else if closed {
            path[count - 1]
        } else {
            path[i]
        };
        let next = if i + 1 < count {
            path[i + 1]
        } else if closed {
            path[0]
        } else {
            path[i]
        };
        (next - prev).normalize_or(Vec3::Y)
    };
    // Parallel-transport a frame along the path so the tube doesn't twist.
    let mut normal = tangent(0).any_orthonormal_vector();
    let mut m = PolyMesh::default();
    let mut rings = Vec::with_capacity(count);
    let mut prev_t = tangent(0);
    for i in 0..count {
        let t = tangent(i);
        let axis = prev_t.cross(t);
        if axis.length() > 1e-6 {
            let angle = prev_t.dot(t).clamp(-1.0, 1.0).acos();
            normal = glam::Quat::from_axis_angle(axis.normalize(), angle) * normal;
        }
        normal = (normal - t * normal.dot(t)).normalize_or(t.any_orthonormal_vector());
        let binormal = t.cross(normal);
        prev_t = t;
        let r = radii.map_or(radius, |r| r[i]).max(1e-5);
        let ring: Vec<u32> = (0..segments)
            .map(|s| {
                let a = s as f32 / segments as f32 * TAU;
                m.add_vertex(path[i] + (normal * a.cos() + binormal * a.sin()) * r)
            })
            .collect();
        rings.push(ring);
    }
    let spans = if closed { count } else { count - 1 };
    for k in 0..spans {
        let (a, b) = (&rings[k], &rings[(k + 1) % count]);
        for s in 0..segments {
            let j = (s + 1) % segments;
            m.add_face(vec![a[s], b[s], b[j], a[j]], true);
        }
    }
    if !closed {
        m.add_face(rings[0].clone(), false);
        m.add_face(rings[count - 1].clone(), false);
    }
    // Rings may self-overlap on tight closed loops; a pass of orient keeps
    // faces outward and validate() catches anything degenerate.
    let m = finish(m);
    if !closed && m.volume().abs() < 1e-12 {
        return Err("sweep produced no volume".into());
    }
    Ok(m)
}

/// Points along a helix around Y, for `sweep` (grip wraps, springs, threads).
pub fn helix(radius: f32, height: f32, turns: f32, points_per_turn: u32) -> Vec<Vec3> {
    let steps = ((turns * points_per_turn.max(6) as f32).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let a = t * turns * TAU;
            Vec3::new(radius * a.cos(), height * (t - 0.5), -radius * a.sin())
        })
        .collect()
}

fn finish(m: PolyMesh) -> PolyMesh {
    let mut m = orient(m);
    // Caps were added without regard to winding; orient flipped everything
    // together, so fix any cap that still disagrees with its neighbours.
    fix_winding(&mut m);
    m
}

/// Flip faces whose winding disagrees with an edge-neighbour, growing from
/// face 0, then make the whole mesh outward-facing.
fn fix_winding(m: &mut PolyMesh) {
    use std::collections::{HashMap, VecDeque};
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, f) in m.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            edge_faces.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let mut done = vec![false; m.faces.len()];
    for start in 0..m.faces.len() {
        if done[start] {
            continue;
        }
        done[start] = true;
        let mut queue = VecDeque::from([start]);
        while let Some(fi) = queue.pop_front() {
            let edges: Vec<(u32, u32)> = m.faces[fi].edges().collect();
            for (a, b) in edges {
                for &g in &edge_faces[&(a.min(b), a.max(b))] {
                    if done[g] {
                        continue;
                    }
                    // A consistent neighbour walks the shared edge as (b, a).
                    if m.faces[g].edges().any(|e| e == (a, b)) {
                        m.faces[g].verts.reverse();
                    }
                    done[g] = true;
                    queue.push_back(g);
                }
            }
        }
    }
    if m.volume() < 0.0 {
        for f in &mut m.faces {
            f.verts.reverse();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens(w: f32, t: f32) -> Vec<Vec2> {
        // Diamond-ish blade section with 8 points.
        (0..8)
            .map(|i| {
                let a = i as f32 / 8.0 * TAU;
                Vec2::new(w * a.cos(), t * a.sin())
            })
            .collect()
    }

    #[test]
    fn blade_loft_is_closed_and_tapers_to_a_point() {
        let st = [
            Station {
                y: 0.0,
                scale: Vec2::ONE,
                offset: Vec2::ZERO,
            },
            Station {
                y: 0.8,
                scale: Vec2::splat(0.8),
                offset: Vec2::ZERO,
            },
            Station {
                y: 1.0,
                scale: Vec2::ZERO,
                offset: Vec2::ZERO,
            },
        ];
        let m = loft(&lens(0.03, 0.006), &st).unwrap();
        assert!(m.is_closed_manifold());
        assert!(m.volume() > 0.0);
        let b = m.bounds().unwrap();
        assert!((b.max.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn concave_section_with_a_fuller() {
        // Lens with a groove on both faces.
        let shape: Vec<Vec2> = [
            (1.0, 0.0),
            (0.5, 0.18),
            (0.15, 0.2),
            (0.0, 0.12),
            (-0.15, 0.2),
            (-0.5, 0.18),
            (-1.0, 0.0),
            (-0.5, -0.18),
            (-0.15, -0.2),
            (0.0, -0.12),
            (0.15, -0.2),
            (0.5, -0.18),
        ]
        .map(|(x, y)| Vec2::new(x, y))
        .to_vec();
        let st = [
            Station {
                y: 0.0,
                scale: Vec2::ONE,
                offset: Vec2::ZERO,
            },
            Station {
                y: 1.0,
                scale: Vec2::ONE,
                offset: Vec2::ZERO,
            },
        ];
        let m = loft(&shape, &st).unwrap();
        assert!(m.is_closed_manifold());
        let expected = signed_area(&shape).abs();
        assert!(
            (m.volume() - expected).abs() < 1e-4,
            "{} vs {expected}",
            m.volume()
        );
        // Concave caps triangulate correctly, so the render mesh has the same volume.
        let r = m.to_mesh();
        r.validate().unwrap();
    }

    #[test]
    fn lathe_pommel_closes_on_the_axis() {
        let p = [
            (0.0, 0.0),
            (0.03, 0.01),
            (0.035, 0.03),
            (0.02, 0.05),
            (0.0, 0.055),
        ]
        .map(|(r, y)| Vec2::new(r, y));
        let m = lathe(&p, 24).unwrap();
        assert!(m.is_closed_manifold());
        assert!(m.volume() > 0.0);
        // Open profile gets caps.
        let cup = lathe(&[Vec2::new(0.5, 0.0), Vec2::new(0.5, 1.0)], 16).unwrap();
        assert!(cup.is_closed_manifold());
        let cyl = std::f32::consts::PI * 0.25;
        assert!((cup.volume() - cyl).abs() < 0.05);
    }

    #[test]
    fn extruded_outline_of_any_winding_points_outwards() {
        let l = [(0., 0.), (2., 0.), (2., 1.), (1., 1.), (1., 2.), (0., 2.)]
            .map(|(x, y)| Vec2::new(x, y));
        for o in [l.to_vec(), l.iter().rev().copied().collect()] {
            let m = extrude_outline(&o, 0.5).unwrap();
            assert!(m.is_closed_manifold());
            assert!((m.volume() - 1.5).abs() < 1e-5);
        }
    }

    #[test]
    fn sweep_open_and_closed_paths() {
        let line = [
            Vec3::ZERO,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
        ];
        let m = sweep(&line, 0.1, None, 12).unwrap();
        assert!(m.is_closed_manifold());
        assert!(m.volume() > 0.0);
        let ring: Vec<Vec3> = (0..=32)
            .map(|i| {
                let a = i as f32 / 32.0 * TAU;
                Vec3::new(a.cos(), 0.0, a.sin())
            })
            .collect();
        let torus = sweep(&ring, 0.1, None, 12).unwrap();
        assert!(torus.is_closed_manifold());
        let expected = TAU * 1.0 * std::f32::consts::PI * 0.01;
        assert!(
            (torus.volume() - expected).abs() / expected < 0.08,
            "{} vs {expected}",
            torus.volume()
        );
        let wrap = sweep(&helix(0.02, 0.15, 8.0, 16), 0.004, None, 8).unwrap();
        assert!(wrap.is_closed_manifold());
    }
}
