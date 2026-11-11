use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::mesh::Mesh;

/// Parametric shapes an agent can start from. All are centred on the origin.
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
}

impl Primitive {
    pub fn default_name(&self) -> &'static str {
        match self {
            Primitive::Cube { .. } => "Cube",
            Primitive::Plane { .. } => "Plane",
            Primitive::UvSphere { .. } => "Sphere",
            Primitive::Cylinder { .. } => "Cylinder",
        }
    }

    pub fn build(&self) -> Mesh {
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
        }
    }
}

/// Builder that appends flat quads (two triangles) with their own vertices.
#[derive(Default)]
struct Builder {
    mesh: Mesh,
}

impl Builder {
    fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.mesh.positions.push(p);
        self.mesh.normals.push(n);
        self.mesh.uvs.push(uv);
        (self.mesh.positions.len() - 1) as u32
    }

    /// Quad with corners in counter-clockwise order when seen from outside.
    fn quad(&mut self, corners: [Vec3; 4], n: Vec3) {
        let uvs = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];
        let i: Vec<u32> = corners
            .iter()
            .zip(uvs)
            .map(|(p, uv)| self.vertex(*p, n, uv))
            .collect();
        self.mesh
            .indices
            .extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }
}

pub fn cube(size: f32) -> Mesh {
    let h = size / 2.0;
    let mut b = Builder::default();
    // For each face: normal, and two tangent axes (u, v) with u × v = normal.
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    for (n, u, v) in faces {
        let c = n * h;
        b.quad(
            [
                c - u * h - v * h,
                c + u * h - v * h,
                c + u * h + v * h,
                c - u * h + v * h,
            ],
            n,
        );
    }
    b.mesh
}

pub fn plane(size: f32) -> Mesh {
    let h = size / 2.0;
    let mut b = Builder::default();
    b.quad(
        [
            Vec3::new(-h, 0.0, h),
            Vec3::new(h, 0.0, h),
            Vec3::new(h, 0.0, -h),
            Vec3::new(-h, 0.0, -h),
        ],
        Vec3::Y,
    );
    b.mesh
}

pub fn uv_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut m = Mesh::default();
    for r in 0..=rings {
        let v = r as f32 / rings as f32;
        let theta = v * PI;
        for s in 0..=segments {
            let u = s as f32 / segments as f32;
            let phi = u * TAU;
            let n = Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                -theta.sin() * phi.sin(),
            );
            m.positions.push(n * radius);
            m.normals.push(n);
            m.uvs.push(Vec2::new(u, 1.0 - v));
        }
    }
    let row = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * row + s;
            let b = a + row;
            if r != 0 {
                m.indices.extend_from_slice(&[a, b, a + 1]);
            }
            if r != rings - 1 {
                m.indices.extend_from_slice(&[a + 1, b, b + 1]);
            }
        }
    }
    m
}

pub fn cylinder(radius: f32, height: f32, segments: u32) -> Mesh {
    let h = height / 2.0;
    let mut b = Builder::default();
    let ring = |i: u32| {
        let a = i as f32 / segments as f32 * TAU;
        Vec3::new(a.cos(), 0.0, -a.sin())
    };
    // Side wall, smooth-shaded.
    for i in 0..=segments {
        let d = ring(i);
        let u = i as f32 / segments as f32;
        b.vertex(d * radius - Vec3::Y * h, d, Vec2::new(u, 0.0));
        b.vertex(d * radius + Vec3::Y * h, d, Vec2::new(u, 1.0));
    }
    for i in 0..segments {
        let (b0, t0, b1, t1) = (i * 2, i * 2 + 1, i * 2 + 2, i * 2 + 3);
        b.mesh.indices.extend_from_slice(&[b0, b1, t1, b0, t1, t0]);
    }
    // Caps as triangle fans.
    for (y, n) in [(h, Vec3::Y), (-h, Vec3::NEG_Y)] {
        let centre = b.vertex(Vec3::Y * y, n, Vec2::splat(0.5));
        let first = centre + 1;
        for i in 0..segments {
            let d = ring(i);
            b.vertex(
                d * radius + Vec3::Y * y,
                n,
                Vec2::new(0.5 + d.x * 0.5, 0.5 - d.z * 0.5),
            );
        }
        for i in 0..segments {
            let (p, q) = (first + i, first + (i + 1) % segments);
            if n.y > 0.0 {
                b.mesh.indices.extend_from_slice(&[centre, p, q]);
            } else {
                b.mesh.indices.extend_from_slice(&[centre, q, p]);
            }
        }
    }
    b.mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's winding should agree with its vertex normals (faces point outward).
    fn assert_outward(m: &Mesh) {
        for tri in m.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
            let face = (m.positions[b] - m.positions[a]).cross(m.positions[c] - m.positions[a]);
            if face.length_squared() < 1e-12 {
                continue;
            }
            let avg = m.normals[a] + m.normals[b] + m.normals[c];
            assert!(face.dot(avg) > 0.0, "triangle {tri:?} faces inward");
        }
    }

    #[test]
    fn cube_is_valid_and_sized() {
        let m = cube(2.0);
        m.validate().unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.bounds().unwrap().size(), Vec3::splat(2.0));
        assert_outward(&m);
    }

    #[test]
    fn sphere_is_valid() {
        let m = uv_sphere(1.0, 16, 8);
        m.validate().unwrap();
        assert!((m.bounds().unwrap().size() - Vec3::splat(2.0)).length() < 1e-4);
        assert_outward(&m);
    }

    #[test]
    fn cylinder_is_valid() {
        let m = cylinder(0.5, 2.0, 12);
        m.validate().unwrap();
        assert!((m.bounds().unwrap().size() - Vec3::new(1.0, 2.0, 1.0)).length() < 1e-4);
        assert_outward(&m);
    }

    #[test]
    fn plane_faces_up() {
        let m = plane(1.0);
        m.validate().unwrap();
        assert_outward(&m);
    }

    #[test]
    fn primitive_json_round_trip() {
        let p: Primitive = serde_json::from_str(r#"{"kind":"cube","size":1.5}"#).unwrap();
        assert_eq!(p, Primitive::Cube { size: 1.5 });
    }
}
