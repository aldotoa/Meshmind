//! Sculpt brushes: push, pull, crease, pinch, smooth, flatten and roughen a
//! dense surface along a stroke, the way a digital sculptor works clay.
//!
//! A stroke is a list of points on (or near) the surface. Every vertex within
//! `radius` of the stroke line moves, fully at the line and fading smoothly to
//! nothing at the edge. Strokes can be mirrored across x = 0 so both sides of
//! a face or body change together.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::poly::PolyMesh;
use crate::sdf::vertex_neighbours;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "brush", rename_all = "snake_case")]
pub enum Brush {
    /// Push the surface out along its normal by `strength` metres (negative
    /// pushes in). Muscles, brow ridges, cheekbones, veins, knuckles.
    Inflate,
    /// Cut a groove `strength` metres deep, pulling its sides together.
    /// Wrinkles, lips, eyelids, nostrils, muscle separations, scars.
    Crease,
    /// Pull the surface toward the stroke line (`strength` 0 to 1). Sharpens
    /// ridges such as a jaw line or a brow edge.
    Pinch,
    /// Even out bumps (`strength` 0 to 1).
    Smooth,
    /// Press the area toward a flat plane (`strength` 0 to 1). Planes of the
    /// face, flat armour areas, worn stone.
    Flatten,
    /// Drag the area by `offset` metres. Pull out ears, a nose, a chin.
    Grab { offset: Vec3 },
    /// Lumpy surface noise `strength` metres high with bumps about `scale`
    /// metres across. Warts, rough hide, bark, rock.
    Noise { scale: f32 },
}

/// One stroke, in the object's own coordinates (metres).
#[derive(Debug, Clone)]
pub struct Stroke {
    /// The line the brush follows; each point snaps to the nearest point of
    /// the surface. One point is a single dab. Empty affects
    /// the whole object (inflate, smooth and noise only).
    pub points: Vec<Vec3>,
    pub radius: f32,
    pub strength: f32,
    /// Also apply the stroke mirrored across x = 0.
    pub symmetric: bool,
}

/// Closest point on a polyline, and the distance to it.
fn closest_on_line(p: Vec3, line: &[Vec3]) -> (Vec3, f32) {
    if line.len() == 1 {
        return (line[0], (p - line[0]).length());
    }
    let mut best = (line[0], f32::INFINITY);
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let t = if ab.length_squared() > 1e-12 {
            ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let c = a + ab * t;
        let d = (p - c).length();
        if d < best.1 {
            best = (c, d);
        }
    }
    best
}

/// Smooth bump: 1 at the centre, 0 at the edge, flat at both ends.
fn falloff(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let s = 1.0 - t * t;
    s * s
}

pub fn vertex_normals(mesh: &PolyMesh) -> Vec<Vec3> {
    let mut n = vec![Vec3::ZERO; mesh.positions.len()];
    for f in &mesh.faces {
        let fn_ = mesh.face_normal_area(f);
        for &v in &f.verts {
            n[v as usize] += fn_;
        }
    }
    n.iter().map(|v| v.normalize_or_zero()).collect()
}

/// Apply one brush stroke. Returns the new mesh and how many vertices moved.
pub fn sculpt(mesh: &PolyMesh, brush: Brush, stroke: &Stroke) -> Result<(PolyMesh, usize), String> {
    if !(stroke.radius > 0.0 && stroke.radius.is_finite()) {
        return Err("`radius` must be a positive number".into());
    }
    if !stroke.strength.is_finite() {
        return Err("`strength` must be a number".into());
    }
    if stroke.points.iter().any(|p| !p.is_finite()) {
        return Err("stroke points must be numbers".into());
    }
    if stroke.points.is_empty()
        && !matches!(brush, Brush::Inflate | Brush::Smooth | Brush::Noise { .. })
    {
        return Err("this brush needs `points` (where to apply it)".into());
    }
    if let Brush::Noise { scale } = brush {
        if scale.is_nan() || scale <= 0.0 {
            return Err("`scale` must be a positive number".into());
        }
    }

    // Snap stroke points onto the surface (the nearest vertex), so a point
    // given slightly inside or outside the skin still lands on it.
    let snapped: Vec<Vec3> = stroke
        .points
        .iter()
        .map(|&q| {
            mesh.positions
                .iter()
                .copied()
                .min_by(|a, b| {
                    (*a - q)
                        .length_squared()
                        .total_cmp(&(*b - q).length_squared())
                })
                .unwrap_or(q)
        })
        .collect();
    let stroke = &Stroke {
        points: snapped,
        ..stroke.clone()
    };
    let mirrored: Vec<Vec3> = stroke
        .points
        .iter()
        .map(|p| Vec3::new(-p.x, p.y, p.z))
        .collect();
    // Per vertex: weight, closest stroke point, and whether it is the mirror side.
    let influence = |p: Vec3| -> (f32, Vec3, bool) {
        if stroke.points.is_empty() {
            return (1.0, p, false);
        }
        let (c, d) = closest_on_line(p, &stroke.points);
        let mut best = (c, d, false);
        if stroke.symmetric {
            let (mc, md) = closest_on_line(p, &mirrored);
            if md < d {
                best = (mc, md, true);
            }
        }
        (falloff(best.1 / stroke.radius), best.0, best.2)
    };

    let normals = vertex_normals(mesh);
    let weights: Vec<(f32, Vec3, bool)> = mesh.positions.iter().map(|&p| influence(p)).collect();
    let moved = weights.iter().filter(|w| w.0 > 1e-4).count();
    let mut out = mesh.clone();
    let s = stroke.strength;

    match brush {
        Brush::Inflate => {
            for (i, p) in out.positions.iter_mut().enumerate() {
                *p += normals[i] * s * weights[i].0;
            }
        }
        Brush::Crease => {
            for (i, p) in out.positions.iter_mut().enumerate() {
                let (w, c, _) = weights[i];
                if w <= 0.0 {
                    continue;
                }
                let n = normals[i];
                let to_line = c - *p;
                let tangential = to_line - n * to_line.dot(n);
                // Narrow, deep centre; the sides lean in.
                *p += -n * s * w * w + tangential * 0.35 * w;
            }
        }
        Brush::Pinch => {
            let s = s.clamp(-1.0, 1.0);
            for (i, p) in out.positions.iter_mut().enumerate() {
                let (w, c, _) = weights[i];
                let n = normals[i];
                let to_line = c - *p;
                *p += (to_line - n * to_line.dot(n)) * s * w;
            }
        }
        Brush::Smooth => {
            let nb = vertex_neighbours(mesh);
            let passes = 1 + (s.clamp(0.0, 1.0) * 6.0).round() as usize;
            for _ in 0..passes {
                let old = out.positions.clone();
                for (i, p) in out.positions.iter_mut().enumerate() {
                    let w = weights[i].0;
                    if w <= 0.0 || nb[i].is_empty() {
                        continue;
                    }
                    let avg =
                        nb[i].iter().map(|&j| old[j as usize]).sum::<Vec3>() / nb[i].len() as f32;
                    *p = old[i].lerp(avg, 0.6 * w);
                }
            }
        }
        Brush::Flatten => {
            // One plane per side: weighted centre and average normal.
            let s = s.clamp(0.0, 1.0);
            for side in [false, true] {
                let (mut c, mut n, mut wsum) = (Vec3::ZERO, Vec3::ZERO, 0.0);
                for (i, &(w, _, m)) in weights.iter().enumerate() {
                    if m == side && w > 0.0 {
                        c += mesh.positions[i] * w;
                        n += normals[i] * w;
                        wsum += w;
                    }
                }
                if wsum <= 0.0 || n.length_squared() < 1e-12 {
                    continue;
                }
                let (c, n) = (c / wsum, n.normalize());
                for (i, p) in out.positions.iter_mut().enumerate() {
                    let (w, _, m) = weights[i];
                    if m == side && w > 0.0 {
                        *p -= n * (*p - c).dot(n) * s * w;
                    }
                }
            }
        }
        Brush::Grab { offset } => {
            if !offset.is_finite() {
                return Err("`offset` must be numbers".into());
            }
            for (i, p) in out.positions.iter_mut().enumerate() {
                let (w, _, m) = weights[i];
                let o = if m {
                    Vec3::new(-offset.x, offset.y, offset.z)
                } else {
                    offset
                };
                *p += o * w;
            }
        }
        Brush::Noise { scale } => {
            for (i, p) in out.positions.iter_mut().enumerate() {
                let w = weights[i].0;
                if w <= 0.0 {
                    continue;
                }
                let q = mesh.positions[i] / scale;
                // Mirror the pattern too, so symmetric strokes stay symmetric.
                let q = if stroke.symmetric {
                    Vec3::new(q.x.abs(), q.y, q.z)
                } else {
                    q
                };
                let bump = (crate::smart::fbm(q + Vec3::splat(17.3), 3) - 0.5) * 2.0;
                *p += normals[i] * bump * s * w;
            }
        }
    }
    Ok((out, moved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdf::{build, ellipsoid, MeshOptions, SdfPart};

    fn ball() -> PolyMesh {
        build(
            &[SdfPart::add(
                "ball",
                ellipsoid(Vec3::ZERO, Vec3::splat(0.5)),
                0.0,
            )],
            MeshOptions {
                resolution: 48,
                relax: 2,
                fit_thin_parts: true,
            },
        )
        .unwrap()
    }

    fn dab(p: Vec3, radius: f32, strength: f32) -> Stroke {
        Stroke {
            points: vec![p],
            radius,
            strength,
            symmetric: false,
        }
    }

    #[test]
    fn inflate_pushes_out_locally() {
        let m = ball();
        let (out, moved) = sculpt(
            &m,
            Brush::Inflate,
            &dab(Vec3::new(0.0, 0.5, 0.0), 0.2, 0.05),
        )
        .unwrap();
        assert!(moved > 0 && moved < m.positions.len() / 2);
        let top = out.bounds().unwrap().max.y;
        assert!((top - 0.55).abs() < 0.01, "top at {top}");
        assert!(
            (out.bounds().unwrap().min.y + 0.5).abs() < 0.01,
            "far side untouched"
        );
    }

    #[test]
    fn crease_cuts_in_and_symmetry_mirrors() {
        let m = ball();
        let stroke = Stroke {
            points: vec![Vec3::new(0.35, 0.0, 0.35)],
            radius: 0.1,
            strength: 0.03,
            symmetric: true,
        };
        let (out, _) = sculpt(&m, Brush::Crease, &stroke).unwrap();
        assert!(out.volume() < m.volume());
        let near = |x: f32| {
            out.positions
                .iter()
                .filter(|p| (p.x - x).abs() < 0.05 && p.y.abs() < 0.05 && p.z > 0.2)
                .map(|p| p.length())
                .fold(1.0f32, f32::min)
        };
        assert!(
            near(0.35) < 0.48 && near(-0.35) < 0.48,
            "both sides creased"
        );
    }

    #[test]
    fn grab_and_smooth() {
        let m = ball();
        let (out, _) = sculpt(
            &m,
            Brush::Grab {
                offset: Vec3::new(0.0, 0.0, 0.2),
            },
            &dab(Vec3::new(0.0, 0.0, 0.5), 0.15, 1.0),
        )
        .unwrap();
        assert!(out.bounds().unwrap().max.z > 0.65);
        let (smoothed, _) = sculpt(
            &out,
            Brush::Smooth,
            &dab(Vec3::new(0.0, 0.0, 0.6), 0.3, 1.0),
        )
        .unwrap();
        assert!(smoothed.bounds().unwrap().max.z < out.bounds().unwrap().max.z);
        assert!(sculpt(
            &m,
            Brush::Pinch,
            &Stroke {
                points: vec![],
                radius: 0.1,
                strength: 0.5,
                symmetric: false
            }
        )
        .is_err());
    }
}
