//! Boolean operations (union, difference, intersection) using BSP trees,
//! following the classic csg.js approach by Evan Wallace (MIT).
//!
//! Inputs should be closed meshes with outward-facing, convex faces.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::poly::PolyMesh;

const EPS: f32 = 1e-5;

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

#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vec3,
    w: f32,
}

impl Plane {
    fn from_points(a: Vec3, b: Vec3, c: Vec3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        let len = n.length();
        (len > 1e-12).then(|| {
            let normal = n / len;
            Plane {
                normal,
                w: normal.dot(a),
            }
        })
    }

    fn flip(&mut self) {
        self.normal = -self.normal;
        self.w = -self.w;
    }
}

#[derive(Debug, Clone)]
struct Polygon {
    verts: Vec<Vec3>,
    plane: Plane,
    smooth: bool,
}

impl Polygon {
    fn new(verts: Vec<Vec3>, smooth: bool) -> Option<Polygon> {
        // Find a non-degenerate triple to define the plane.
        let n = verts.len();
        for i in 1..n - 1 {
            if let Some(plane) = Plane::from_points(verts[0], verts[i], verts[i + 1]) {
                return Some(Polygon {
                    verts,
                    plane,
                    smooth,
                });
            }
        }
        None
    }

    fn flip(&mut self) {
        self.verts.reverse();
        self.plane.flip();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Split `poly` by `plane`, sending the pieces to the right lists.
fn split_polygon(
    plane: &Plane,
    poly: Polygon,
    coplanar_front: &mut Vec<Polygon>,
    coplanar_back: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let mut kind = 0u8;
    let types: Vec<u8> = poly
        .verts
        .iter()
        .map(|v| {
            let t = plane.normal.dot(*v) - plane.w;
            let ty = if t < -EPS {
                BACK
            } else if t > EPS {
                FRONT
            } else {
                COPLANAR
            };
            kind |= ty;
            ty
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.normal.dot(poly.plane.normal) > 0.0 {
                coplanar_front.push(poly)
            } else {
                coplanar_back.push(poly)
            }
        }
        FRONT => front.push(poly),
        BACK => back.push(poly),
        _ => {
            let (mut f, mut b) = (Vec::new(), Vec::new());
            let n = poly.verts.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.verts[i], poly.verts[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.normal.dot(vi)) / plane.normal.dot(vj - vi);
                    let v = vi.lerp(vj, t);
                    f.push(v);
                    b.push(v);
                }
            }
            if f.len() >= 3 {
                if let Some(p) = Polygon::new(f, poly.smooth) {
                    front.push(Polygon {
                        plane: poly.plane,
                        ..p
                    });
                }
            }
            if b.len() >= 3 {
                if let Some(p) = Polygon::new(b, poly.smooth) {
                    back.push(Polygon {
                        plane: poly.plane,
                        ..p
                    });
                }
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Polygon>,
}

impl Node {
    fn new(polygons: Vec<Polygon>) -> Node {
        let mut n = Node::default();
        n.build(polygons);
        n
    }

    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(pl) = &mut self.plane {
            pl.flip();
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Remove the parts of `polygons` that are inside this BSP tree.
    fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = self.plane else {
            return polygons;
        };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in polygons {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split_polygon(&plane, p, &mut cf, &mut cb, &mut front, &mut back);
            front.append(&mut cf);
            back.append(&mut cb);
        }
        let front = match &self.front {
            Some(f) => f.clip_polygons(front),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back),
            None => Vec::new(),
        };
        let mut out = front;
        out.extend(back);
        out
    }

    fn clip_to(&mut self, other: &Node) {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(f) = &mut self.front {
            f.clip_to(other);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other);
        }
    }

    fn all_polygons(&self) -> Vec<Polygon> {
        let mut out = self.polygons.clone();
        if let Some(f) = &self.front {
            out.extend(f.all_polygons());
        }
        if let Some(b) = &self.back {
            out.extend(b.all_polygons());
        }
        out
    }

    fn build(&mut self, polygons: Vec<Polygon>) {
        if polygons.is_empty() {
            return;
        }
        let plane = *self.plane.get_or_insert(polygons[0].plane);
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in polygons {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split_polygon(&plane, p, &mut cf, &mut cb, &mut front, &mut back);
            self.polygons.append(&mut cf);
            self.polygons.append(&mut cb);
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front);
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back);
        }
    }
}

fn to_polygons(mesh: &PolyMesh) -> Vec<Polygon> {
    mesh.faces
        .iter()
        .filter_map(|f| {
            Polygon::new(
                f.verts
                    .iter()
                    .map(|&v| mesh.positions[v as usize])
                    .collect(),
                f.smooth,
            )
        })
        .collect()
}

fn from_polygons(polygons: Vec<Polygon>) -> PolyMesh {
    let mut m = PolyMesh::default();
    for p in polygons {
        let verts = p.verts.iter().map(|v| m.add_vertex(*v)).collect();
        m.add_face(verts, p.smooth);
    }
    m.weld(EPS * 10.0);
    m
}

/// Combine two meshes given in the same coordinate space.
pub fn boolean(a: &PolyMesh, b: &PolyMesh, op: BooleanOp) -> PolyMesh {
    let (pa, pb) = (to_polygons(a), to_polygons(b));
    // BSP building recurses; give it a roomy stack for detailed meshes.
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            let mut a = Node::new(pa);
            let mut b = Node::new(pb);
            match op {
                BooleanOp::Union => {
                    a.clip_to(&b);
                    b.clip_to(&a);
                    b.invert();
                    b.clip_to(&a);
                    b.invert();
                    a.build(b.all_polygons());
                }
                BooleanOp::Difference => {
                    a.invert();
                    a.clip_to(&b);
                    b.clip_to(&a);
                    b.invert();
                    b.clip_to(&a);
                    b.invert();
                    a.build(b.all_polygons());
                    a.invert();
                }
                BooleanOp::Intersection => {
                    a.invert();
                    b.clip_to(&a);
                    b.invert();
                    a.clip_to(&b);
                    b.clip_to(&a);
                    a.build(b.all_polygons());
                    a.invert();
                }
            }
            from_polygons(a.all_polygons())
        })
        .expect("spawn boolean worker")
        .join()
        .expect("boolean worker panicked")
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
        let u = boolean(&a, &b, BooleanOp::Union).volume();
        let d = boolean(&a, &b, BooleanOp::Difference).volume();
        let i = boolean(&a, &b, BooleanOp::Intersection).volume();
        assert!((u - 12.0).abs() < 1e-3, "union {u}");
        assert!((d - 4.0).abs() < 1e-3, "difference {d}");
        assert!((i - 4.0).abs() < 1e-3, "intersection {i}");
    }

    #[test]
    fn drill_a_hole() {
        let block = cube(2.0);
        let drill = cylinder(0.5, 3.0, 32);
        let result = boolean(&block, &drill, BooleanOp::Difference);
        let expected = 8.0 - drill.volume() * (2.0 / 3.0);
        assert!(
            (result.volume() - expected).abs() < 1e-2,
            "{}",
            result.volume()
        );
    }
}
