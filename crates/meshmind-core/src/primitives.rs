use std::f32::consts::{PI, TAU};

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::poly::PolyMesh;

/// Parametric shapes an agent can start from. All are centred on the origin, Y up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Primitive {
    Cube {
        size: f32,
    },
    Plane {
        size: f32,
    },
    UvSphere {
        radius: f32,
        segments: u32,
        rings: u32,
    },
    Cylinder {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Cone {
        radius: f32,
        height: f32,
        segments: u32,
    },
    Torus {
        major_radius: f32,
        minor_radius: f32,
        segments: u32,
        tube_segments: u32,
    },
}

impl Primitive {
    pub fn default_name(&self) -> &'static str {
        match self {
            Primitive::Cube { .. } => "Cube",
            Primitive::Plane { .. } => "Plane",
            Primitive::UvSphere { .. } => "Sphere",
            Primitive::Cylinder { .. } => "Cylinder",
            Primitive::Cone { .. } => "Cone",
            Primitive::Torus { .. } => "Torus",
        }
    }

    pub fn build(&self) -> PolyMesh {
        match *self {
            Primitive::Cube { size } => cube(size),
            Primitive::Plane { size } => plane(size),
            Primitive::UvSphere {
                radius,
                segments,
                rings,
            } => uv_sphere(radius, segments.max(3), rings.max(2)),
            Primitive::Cylinder {
                radius,
                height,
                segments,
            } => cylinder(radius, height, segments.max(3)),
            Primitive::Cone {
                radius,
                height,
                segments,
            } => cone(radius, height, segments.max(3)),
            Primitive::Torus {
                major_radius,
                minor_radius,
                segments,
                tube_segments,
            } => torus(
                major_radius,
                minor_radius,
                segments.max(3),
                tube_segments.max(3),
            ),
        }
    }
}

pub fn cube(size: f32) -> PolyMesh {
    let h = size / 2.0;
    let mut m = PolyMesh::default();
    for i in 0..8u32 {
        let s = |bit: u32| if i & bit != 0 { h } else { -h };
        m.add_vertex(Vec3::new(s(1), s(2), s(4)));
    }
    for f in [
        [1, 3, 7, 5],
        [0, 4, 6, 2],
        [2, 6, 7, 3],
        [0, 1, 5, 4],
        [4, 5, 7, 6],
        [0, 2, 3, 1],
    ] {
        m.add_face(f.to_vec(), false);
    }
    m
}

pub fn plane(size: f32) -> PolyMesh {
    let h = size / 2.0;
    let mut m = PolyMesh::default();
    for (x, z) in [(-h, h), (h, h), (h, -h), (-h, -h)] {
        m.add_vertex(Vec3::new(x, 0.0, z));
    }
    m.add_face(vec![0, 1, 2, 3], false);
    m
}

/// Unit direction around the Y axis; angle grows counter-clockwise seen from above.
fn around_y(i: u32, segments: u32) -> Vec3 {
    let a = i as f32 / segments as f32 * TAU;
    Vec3::new(a.cos(), 0.0, -a.sin())
}

pub fn uv_sphere(radius: f32, segments: u32, rings: u32) -> PolyMesh {
    let mut m = PolyMesh::default();
    let top = m.add_vertex(Vec3::Y * radius);
    let ring_start = |r: u32| 1 + (r - 1) * segments;
    for r in 1..rings {
        let theta = r as f32 / rings as f32 * PI;
        for s in 0..segments {
            let d = around_y(s, segments) * theta.sin() + Vec3::Y * theta.cos();
            m.add_vertex(d * radius);
        }
    }
    let bottom = m.add_vertex(-Vec3::Y * radius);
    let at = |r: u32, s: u32| ring_start(r) + s % segments;
    for s in 0..segments {
        m.add_face(vec![top, at(1, s), at(1, s + 1)], true);
    }
    for r in 1..rings - 1 {
        for s in 0..segments {
            m.add_face(
                vec![at(r, s), at(r + 1, s), at(r + 1, s + 1), at(r, s + 1)],
                true,
            );
        }
    }
    for s in 0..segments {
        m.add_face(vec![bottom, at(rings - 1, s + 1), at(rings - 1, s)], true);
    }
    m
}

pub fn cylinder(radius: f32, height: f32, segments: u32) -> PolyMesh {
    let h = height / 2.0;
    let mut m = PolyMesh::default();
    for i in 0..segments {
        m.add_vertex(around_y(i, segments) * radius - Vec3::Y * h);
    }
    for i in 0..segments {
        m.add_vertex(around_y(i, segments) * radius + Vec3::Y * h);
    }
    for i in 0..segments {
        let j = (i + 1) % segments;
        m.add_face(vec![i, j, segments + j, segments + i], true);
    }
    m.add_face((segments..2 * segments).collect(), false);
    m.add_face((0..segments).rev().collect(), false);
    m
}

pub fn cone(radius: f32, height: f32, segments: u32) -> PolyMesh {
    let h = height / 2.0;
    let mut m = PolyMesh::default();
    for i in 0..segments {
        m.add_vertex(around_y(i, segments) * radius - Vec3::Y * h);
    }
    let apex = m.add_vertex(Vec3::Y * h);
    for i in 0..segments {
        m.add_face(vec![i, (i + 1) % segments, apex], true);
    }
    m.add_face((0..segments).rev().collect(), false);
    m
}

pub fn torus(major: f32, minor: f32, segments: u32, tube: u32) -> PolyMesh {
    let mut m = PolyMesh::default();
    for i in 0..segments {
        let d = around_y(i, segments);
        for j in 0..tube {
            let b = j as f32 / tube as f32 * TAU;
            m.add_vertex(d * (major + minor * b.cos()) + Vec3::Y * minor * b.sin());
        }
    }
    let at = |i: u32, j: u32| (i % segments) * tube + j % tube;
    for i in 0..segments {
        for j in 0..tube {
            m.add_face(
                vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)],
                true,
            );
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(p: Primitive, expected_volume: f32) {
        let m = p.build();
        m.validate().unwrap();
        assert!(m.is_closed_manifold(), "{p:?} is not closed");
        let v = m.volume();
        assert!(
            (v - expected_volume).abs() / expected_volume < 0.05,
            "{p:?}: volume {v}, expected {expected_volume}"
        );
        m.to_mesh().validate().unwrap();
    }

    #[test]
    fn closed_primitives_have_outward_faces_and_right_volume() {
        check(Primitive::Cube { size: 2.0 }, 8.0);
        check(
            Primitive::UvSphere {
                radius: 1.0,
                segments: 64,
                rings: 32,
            },
            4.0 / 3.0 * PI,
        );
        check(
            Primitive::Cylinder {
                radius: 1.0,
                height: 2.0,
                segments: 64,
            },
            2.0 * PI,
        );
        check(
            Primitive::Cone {
                radius: 1.0,
                height: 3.0,
                segments: 64,
            },
            PI,
        );
        check(
            Primitive::Torus {
                major_radius: 2.0,
                minor_radius: 0.5,
                segments: 64,
                tube_segments: 32,
            },
            2.0 * PI * PI * 2.0 * 0.25,
        );
    }

    #[test]
    fn plane_faces_up() {
        let m = plane(1.0);
        assert!(m.face_normal(&m.faces[0]).abs_diff_eq(Vec3::Y, 1e-6));
    }

    #[test]
    fn primitive_json_round_trip() {
        let p: Primitive = serde_json::from_str(r#"{"kind":"cube","size":1.5}"#).unwrap();
        assert_eq!(p, Primitive::Cube { size: 1.5 });
    }
}
