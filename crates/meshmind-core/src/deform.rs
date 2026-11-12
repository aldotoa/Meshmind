//! Whole-object deformers (taper, bend, twist), slicing to add edge loops,
//! and moving a region of vertices.

use std::collections::HashMap;

use glam::{Quat, Vec3};

use crate::ops::Axis;
use crate::poly::{Face, PolyMesh};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Deform {
    /// Scale the cross-section linearly from 1 at the low end of `axis` to
    /// `end_scale` at the high end.
    Taper { end_scale: f32 },
    /// Rotate cross-sections around `axis`, from 0 at the low end to `degrees`.
    Twist { degrees: f32 },
    /// Curve the object so it bends by `degrees` along `axis`, towards `toward`.
    Bend { degrees: f32, toward: Axis },
}

fn component(v: Vec3, a: Axis) -> f32 {
    v.dot(a.unit())
}

/// Insert `cuts` evenly spaced edge loops across `axis`, splitting every face
/// they cross. Gives deformers (and later edits) the vertices they need.
pub fn slice(mesh: &PolyMesh, axis: Axis, cuts: u32) -> PolyMesh {
    let Some(b) = mesh.bounds() else {
        return mesh.clone();
    };
    let (lo, hi) = (component(b.min, axis), component(b.max, axis));
    let mut out = mesh.clone();
    for i in 1..=cuts {
        let c = lo + (hi - lo) * i as f32 / (cuts + 1) as f32;
        out = slice_at(&out, axis, c);
    }
    out
}

/// Split every face crossing the plane `axis = c`.
pub fn slice_at(mesh: &PolyMesh, axis: Axis, c: f32) -> PolyMesh {
    let eps = 1e-6;
    let mut out = PolyMesh {
        positions: mesh.positions.clone(),
        faces: Vec::with_capacity(mesh.faces.len()),
    };
    let mut cut_vertex: HashMap<(u32, u32), u32> = HashMap::new();
    let d = |v: u32| component(mesh.positions[v as usize], axis) - c;
    for f in &mesh.faces {
        let ds: Vec<f32> = f.verts.iter().map(|&v| d(v)).collect();
        let above = ds.iter().any(|&x| x > eps);
        let below = ds.iter().any(|&x| x < -eps);
        if !(above && below) {
            out.faces.push(Face::new(f.verts.clone(), f.smooth));
            continue;
        }
        let (mut pos, mut neg) = (Vec::new(), Vec::new());
        let n = f.verts.len();
        for i in 0..n {
            let (a, b) = (f.verts[i], f.verts[(i + 1) % n]);
            let (da, db) = (ds[i], ds[(i + 1) % n]);
            if da >= -eps {
                pos.push(a);
            }
            if da <= eps {
                neg.push(a);
            }
            if (da > eps && db < -eps) || (da < -eps && db > eps) {
                let key = (a.min(b), a.max(b));
                let v = *cut_vertex.entry(key).or_insert_with(|| {
                    let (pa, pb) = (mesh.positions[a as usize], mesh.positions[b as usize]);
                    let t = da / (da - db);
                    out.positions.push(pa.lerp(pb, t));
                    (out.positions.len() - 1) as u32
                });
                pos.push(v);
                neg.push(v);
            }
        }
        for part in [pos, neg] {
            if part.len() >= 3 {
                out.faces.push(Face::new(part, f.smooth));
            }
        }
    }
    out
}

/// Apply a deformer along `axis`. The low end of the object stays put.
pub fn deform(mesh: &PolyMesh, axis: Axis, how: Deform) -> Result<PolyMesh, String> {
    let b = mesh.bounds().ok_or("the object is empty")?;
    let (lo, hi) = (component(b.min, axis), component(b.max, axis));
    let len = hi - lo;
    if len < 1e-6 {
        return Err("the object has no length along that axis".into());
    }
    let a = axis.unit();
    let centre = (b.min + b.max) * 0.5;
    let mut out = mesh.clone();
    match how {
        Deform::Taper { end_scale } => {
            if end_scale.is_nan() || end_scale < 0.0 {
                return Err("`amount` must be 0 or more".into());
            }
            for p in &mut out.positions {
                let t = (component(*p, axis) - lo) / len;
                let s = 1.0 + (end_scale - 1.0) * t;
                let along = a * component(*p - centre, axis);
                let across = (*p - centre) - along;
                *p = centre + along + across * s;
            }
        }
        Deform::Twist { degrees } => {
            for p in &mut out.positions {
                let t = (component(*p, axis) - lo) / len;
                let q = Quat::from_axis_angle(a, (degrees * t).to_radians());
                *p = centre + q * (*p - centre);
            }
        }
        Deform::Bend { degrees, toward } => {
            if toward == axis {
                return Err("bend direction must differ from the axis".into());
            }
            let theta = degrees.to_radians();
            if theta.abs() < 1e-5 {
                return Ok(out);
            }
            let dir = toward.unit();
            let r = len / theta;
            for p in &mut out.positions {
                let s = component(*p, axis) - lo;
                let off = component(*p - centre, toward);
                let ang = theta * s / len;
                let rr = r - off;
                let new_s = rr * ang.sin();
                let new_off = r - rr * ang.cos();
                *p += a * (lo + new_s - component(*p, axis))
                    + dir * (component(centre, toward) + new_off - component(*p, toward));
            }
        }
    }
    out.clear_uvs();
    out.validate()?;
    Ok(out)
}

/// Move (and optionally scale, around the region's centre) every vertex
/// inside the box `min..max`, with a soft falloff band of `falloff` around it.
pub fn move_vertices(
    mesh: &PolyMesh,
    min: Vec3,
    max: Vec3,
    translate: Vec3,
    scale: Vec3,
    falloff: f32,
) -> Result<(PolyMesh, usize), String> {
    let centre = (min + max) * 0.5;
    let mut out = mesh.clone();
    let mut moved = 0;
    for p in &mut out.positions {
        // Distance outside the box (0 inside).
        let outside = (min - *p).max(*p - max).max(Vec3::ZERO).length();
        let w = if outside <= 0.0 {
            1.0
        } else if falloff > 0.0 && outside < falloff {
            let t = 1.0 - outside / falloff;
            t * t * (3.0 - 2.0 * t)
        } else {
            0.0
        };
        if w > 0.0 {
            let target = centre + (*p - centre) * scale + translate;
            *p = p.lerp(target, w);
            moved += 1;
        }
    }
    if moved == 0 {
        return Err("no vertices inside that region; check it with get_object's bounds".into());
    }
    out.clear_uvs();
    out.validate()?;
    Ok((out, moved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, cylinder};

    #[test]
    fn slicing_keeps_the_mesh_closed() {
        let m = slice(&cube(1.0), Axis::Y, 3);
        assert!(m.is_closed_manifold());
        assert_eq!(m.faces.len(), 2 + 4 * 4);
        assert!((m.volume() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn taper_to_a_point_halves_volume_of_a_prism() {
        let m = deform(&cube(1.0), Axis::Y, Deform::Taper { end_scale: 0.0 }).unwrap();
        assert!((m.volume() - 1.0 / 3.0).abs() < 1e-4, "{}", m.volume());
    }

    #[test]
    fn twist_and_bend_keep_volume_roughly() {
        let c = slice(&cylinder(0.1, 1.0, 16), Axis::Y, 20);
        let v = c.volume();
        let t = deform(&c, Axis::Y, Deform::Twist { degrees: 90.0 }).unwrap();
        assert!((t.volume() - v).abs() / v < 0.02);
        let b = deform(
            &c,
            Axis::Y,
            Deform::Bend {
                degrees: 90.0,
                toward: Axis::X,
            },
        )
        .unwrap();
        assert!(b.is_closed_manifold());
        assert!((b.volume() - v).abs() / v < 0.05, "{} vs {v}", b.volume());
        // The top end now points sideways: the object reaches far along +X.
        assert!(b.bounds().unwrap().max.x > 0.4);
    }

    #[test]
    fn move_a_region() {
        let m = slice(&cube(1.0), Axis::Y, 1);
        let (out, n) = move_vertices(
            &m,
            Vec3::new(-1.0, 0.4, -1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::Y * 0.5,
            Vec3::ONE,
            0.0,
        )
        .unwrap();
        assert_eq!(n, 4);
        assert!((out.bounds().unwrap().max.y - 1.0).abs() < 1e-6);
        assert!(move_vertices(
            &m,
            Vec3::splat(5.0),
            Vec3::splat(6.0),
            Vec3::X,
            Vec3::ONE,
            0.0
        )
        .is_err());
    }
}
