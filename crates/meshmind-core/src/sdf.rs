//! Clay-like modelling: blend simple volumes (ellipsoids, tapered capsules,
//! rounded boxes) into one continuous surface.
//!
//! Each part is a signed distance field. Parts are combined in order with a
//! smooth union (or a smooth subtraction for sockets and grooves), so where a
//! neck meets a shoulder the surface flows instead of creasing. The field is
//! then turned into a single watertight quad mesh with Surface Nets, its
//! vertices snapped onto the true surface and relaxed so the quads are even.
//!
//! This is how organic forms (characters, creatures, rocks, roots, food) are
//! blocked out: a body is a few dozen overlapping volumes, not a stack of
//! separate primitives.

use std::collections::HashMap;

use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::poly::PolyMesh;

/// A volume shape, in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum SdfShape {
    /// A sphere stretched to `radii` (x, y, z), turned by the part's rotation.
    Ellipsoid { center: Vec3, radii: Vec3 },
    /// A tube from `a` to `b` with rounded ends, radius `radius_a` at `a`
    /// tapering to `radius_b` at `b`. Limbs, fingers, necks, horns, tusks.
    Capsule {
        a: Vec3,
        b: Vec3,
        radius_a: f32,
        radius_b: f32,
    },
    /// A box of full `size` with its edges rounded by `rounding`.
    Box {
        center: Vec3,
        size: Vec3,
        #[serde(default)]
        rounding: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SdfOp {
    /// Add the volume, blending into what is already there.
    #[default]
    Add,
    /// Carve the volume away (eye sockets, mouths, grooves between muscles).
    Subtract,
}

/// One volume in a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SdfPart {
    /// Label such as "left upper arm", so the agent can find it again.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub shape: SdfShape,
    #[serde(default)]
    pub op: SdfOp,
    /// How far (metres) the part melts into the parts before it. 0 is a hard join.
    #[serde(default = "default_blend")]
    pub blend: f32,
    /// Also add a copy mirrored across x = 0 (the other arm, ear, eye socket).
    #[serde(default, skip_serializing_if = "is_false")]
    pub mirror: bool,
    /// Euler rotation in degrees (x, y, z), for ellipsoids and boxes.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: Vec3,
}

fn default_blend() -> f32 {
    0.03
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(v: &Vec3) -> bool {
    *v == Vec3::ZERO
}

impl SdfPart {
    pub fn add(name: &str, shape: SdfShape, blend: f32) -> Self {
        SdfPart {
            name: name.into(),
            shape,
            op: SdfOp::Add,
            blend,
            mirror: false,
            rotation: Vec3::ZERO,
        }
    }
    pub fn mirrored(mut self) -> Self {
        self.mirror = true;
        self
    }
    pub fn subtract(mut self) -> Self {
        self.op = SdfOp::Subtract;
        self
    }
    pub fn rotated(mut self, degrees: Vec3) -> Self {
        self.rotation = degrees;
        self
    }
}

pub fn ellipsoid(center: Vec3, radii: Vec3) -> SdfShape {
    SdfShape::Ellipsoid { center, radii }
}
pub fn capsule(a: Vec3, b: Vec3, radius_a: f32, radius_b: f32) -> SdfShape {
    SdfShape::Capsule {
        a,
        b,
        radius_a,
        radius_b,
    }
}
pub fn rounded_box(center: Vec3, size: Vec3, rounding: f32) -> SdfShape {
    SdfShape::Box {
        center,
        size,
        rounding,
    }
}

/// A part ready to evaluate: inverse rotation precomputed, bounds known.
struct Prepared {
    shape: SdfShape,
    inv_rot: Quat,
    subtract: bool,
    k: f32,
    mirror: bool,
    lo: Vec3,
    hi: Vec3,
}

fn prepare(p: &SdfPart) -> Result<Prepared, String> {
    let rot = Quat::from_euler(
        EulerRot::XYZ,
        p.rotation.x.to_radians(),
        p.rotation.y.to_radians(),
        p.rotation.z.to_radians(),
    );
    let label = if p.name.is_empty() { "a part" } else { &p.name };
    let finite = |v: Vec3| v.is_finite();
    let (center, reach) = match &p.shape {
        SdfShape::Ellipsoid { center, radii } => {
            if !finite(*center) || !finite(*radii) || radii.min_element() <= 0.0 {
                return Err(format!("{label}: ellipsoid radii must be positive numbers"));
            }
            (*center, Vec3::splat(radii.max_element()))
        }
        SdfShape::Capsule {
            a,
            b,
            radius_a,
            radius_b,
        } => {
            if !finite(*a) || !finite(*b) || !(*radius_a >= 0.0 && *radius_b >= 0.0) {
                return Err(format!(
                    "{label}: capsule needs points and radii of 0 or more"
                ));
            }
            if radius_a.max(*radius_b) <= 0.0 {
                return Err(format!("{label}: capsule needs a radius above 0"));
            }
            let r = radius_a.max(*radius_b);
            let lo = a.min(*b) - Vec3::splat(r);
            let hi = a.max(*b) + Vec3::splat(r);
            ((lo + hi) * 0.5, (hi - lo) * 0.5)
        }
        SdfShape::Box { center, size, .. } => {
            if !finite(*center) || !finite(*size) || size.min_element() <= 0.0 {
                return Err(format!("{label}: box size must be positive"));
            }
            (*center, Vec3::splat((*size * 0.5).length()))
        }
    };
    let k = if p.blend.is_finite() {
        p.blend.max(0.0)
    } else {
        0.0
    };
    let mut lo = center - reach;
    let mut hi = center + reach;
    if p.mirror {
        let mlo = Vec3::new(-hi.x, lo.y, lo.z);
        let mhi = Vec3::new(-lo.x, hi.y, hi.z);
        lo = lo.min(mlo);
        hi = hi.max(mhi);
    }
    Ok(Prepared {
        shape: p.shape.clone(),
        inv_rot: rot.inverse(),
        subtract: p.op == SdfOp::Subtract,
        k,
        mirror: p.mirror,
        lo,
        hi,
    })
}

fn sd_shape(shape: &SdfShape, inv_rot: Quat, p: Vec3) -> f32 {
    match shape {
        SdfShape::Ellipsoid { center, radii } => {
            let q = inv_rot * (p - *center);
            let k0 = (q / *radii).length();
            let k1 = (q / (*radii * *radii)).length();
            if k1 < 1e-12 {
                -radii.min_element()
            } else {
                k0 * (k0 - 1.0) / k1
            }
        }
        SdfShape::Capsule {
            a,
            b,
            radius_a,
            radius_b,
        } => round_cone(p, *a, *b, *radius_a, *radius_b),
        SdfShape::Box {
            center,
            size,
            rounding,
        } => {
            let half = *size * 0.5;
            let r = rounding.clamp(0.0, half.min_element());
            let q = (inv_rot * (p - *center)).abs() - (half - Vec3::splat(r));
            q.max(Vec3::ZERO).length() + q.max_element().min(0.0) - r
        }
    }
}

/// Exact distance to a cone with spherical caps (Inigo Quilez).
fn round_cone(p: Vec3, a: Vec3, b: Vec3, r1: f32, r2: f32) -> f32 {
    let ba = b - a;
    let l2 = ba.length_squared();
    let rr = r1 - r2;
    // Degenerate: one sphere swallows the other.
    if l2 < 1e-12 || rr * rr >= l2 {
        let (c, r) = if r1 >= r2 { (a, r1) } else { (b, r2) };
        return (p - c).length() - r;
    }
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = p - a;
    let y = pa.dot(ba);
    let z = y - l2;
    let x2 = (pa * l2 - ba * y).length_squared();
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        return (x2 + z2).sqrt() * il2 - r2;
    }
    if y.signum() * a2 * y2 < k {
        return (x2 + y2).sqrt() * il2 - r1;
    }
    ((x2 * a2 * il2).sqrt() + y * rr) * il2 - r1
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

fn box_dist(p: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    (lo - p).max(p - hi).max(Vec3::ZERO).length()
}

/// A compiled body that can be sampled anywhere.
pub struct Field {
    parts: Vec<Prepared>,
}

impl Field {
    pub fn new(parts: &[SdfPart]) -> Result<Field, String> {
        if parts.is_empty() {
            return Err("a body needs at least one part".into());
        }
        if parts.first().is_some_and(|p| p.op == SdfOp::Subtract) {
            return Err("the first part must add volume, not subtract it".into());
        }
        let parts = parts.iter().map(prepare).collect::<Result<Vec<_>, _>>()?;
        Ok(Field { parts })
    }

    /// Signed distance (approximately metres): negative inside.
    pub fn eval(&self, p: Vec3) -> f32 {
        let mut d = f32::INFINITY;
        for part in &self.parts {
            // Skip parts too far away to change the result. With a smooth
            // union, a part whose distance is at least `k` above `d` leaves
            // `d` untouched; the bounding box gives a lower bound for it.
            let lb = box_dist(p, part.lo, part.hi);
            if part.subtract {
                if lb >= part.k - d {
                    continue;
                }
            } else if d.is_finite() && lb >= d + part.k {
                continue;
            }
            let mut di = sd_shape(&part.shape, part.inv_rot, p);
            if part.mirror {
                let m = Vec3::new(-p.x, p.y, p.z);
                di = smin(di, sd_shape(&part.shape, part.inv_rot, m), part.k * 0.5);
            }
            d = if part.subtract {
                -smin(-d, di, part.k)
            } else if d.is_finite() {
                smin(d, di, part.k)
            } else {
                di
            };
        }
        d
    }

    fn gradient(&self, p: Vec3, h: f32) -> Vec3 {
        let dx = Vec3::new(h, 0.0, 0.0);
        let dy = Vec3::new(0.0, h, 0.0);
        let dz = Vec3::new(0.0, 0.0, h);
        Vec3::new(
            self.eval(p + dx) - self.eval(p - dx),
            self.eval(p + dy) - self.eval(p - dy),
            self.eval(p + dz) - self.eval(p - dz),
        ) / (2.0 * h)
    }

    /// Smallest radius among the parts that add volume (tapered tips aside).
    fn thinnest(&self) -> f32 {
        self.parts
            .iter()
            .filter(|p| !p.subtract)
            .map(|p| match &p.shape {
                SdfShape::Ellipsoid { radii, .. } => radii.min_element(),
                SdfShape::Capsule {
                    radius_a, radius_b, ..
                } => radius_a.min(*radius_b).max(radius_a.max(*radius_b) * 0.6),
                SdfShape::Box { size, .. } => size.min_element() * 0.5,
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// Box around everything the parts can add.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in self.parts.iter().filter(|p| !p.subtract) {
            lo = lo.min(p.lo - Vec3::splat(p.k));
            hi = hi.max(p.hi + Vec3::splat(p.k));
        }
        (lo, hi)
    }
}

/// How finely a body is meshed.
#[derive(Debug, Clone, Copy)]
pub struct MeshOptions {
    /// Grid cells along the body's longest side. More is finer and slower.
    pub resolution: u32,
    /// Relaxation passes that even out the quads.
    pub relax: u32,
    /// Raise the resolution where parts are thin (fingers), so they stay whole.
    pub fit_thin_parts: bool,
}

impl Default for MeshOptions {
    fn default() -> Self {
        MeshOptions {
            resolution: 160,
            relax: 3,
            fit_thin_parts: true,
        }
    }
}

/// Turn parts into one watertight, smooth-shaded quad mesh.
pub fn build(parts: &[SdfPart], opts: MeshOptions) -> Result<PolyMesh, String> {
    let field = Field::new(parts)?;
    let (lo, hi) = field.bounds();
    let extent = hi - lo;
    if !extent.is_finite() || extent.max_element() <= 0.0 {
        return Err("the body has no size".into());
    }
    if extent.max_element() > 1000.0 {
        return Err("the body is larger than 1 km; check the units (metres)".into());
    }
    // Thin parts need several cells across them, or the surface tears.
    let res = opts.resolution.clamp(16, 512) as f32;
    // Never more than 1.8x the requested resolution, so polygon counts stay sane.
    let thin = if opts.fit_thin_parts {
        field.thinnest() / 1.5
    } else {
        f32::INFINITY
    };
    let cell = (extent.max_element() / res).min(thin.max(extent.max_element() / (res * 1.8)));
    let n = ((extent / cell).ceil() + Vec3::splat(4.0)).as_uvec3();
    // Centre the grid on the body, so a symmetric body samples symmetrically.
    let origin = (lo + hi) * 0.5 - n.as_vec3() * cell * 0.5;
    let (nx, ny, nz) = (n.x as usize, n.y as usize, n.z as usize);
    let corners = (nx + 1) * (ny + 1) * (nz + 1);
    if corners > 40_000_000 {
        return Err("that resolution is too high for this body; use a lower one".into());
    }

    // Sample the field at every grid corner, one z-slab per thread.
    let mut values = vec![0f32; corners];
    let slab = (nx + 1) * (ny + 1);
    let threads = std::thread::available_parallelism()
        .map(|t| t.get())
        .unwrap_or(4)
        .min(nz + 1);
    let per = (nz + 1).div_ceil(threads);
    std::thread::scope(|s| {
        for (t, chunk) in values.chunks_mut(slab * per).enumerate() {
            let field = &field;
            s.spawn(move || {
                for (i, v) in chunk.iter_mut().enumerate() {
                    let k = t * per + i / slab;
                    let r = i % slab;
                    let (y, x) = (r / (nx + 1), r % (nx + 1));
                    let p = origin + Vec3::new(x as f32, y as f32, k as f32) * cell;
                    *v = field.eval(p);
                }
            });
        }
    });
    resolve_ambiguous_faces(&mut values, nx, ny, nz);
    let at = |x: usize, y: usize, z: usize| values[x + (nx + 1) * (y + (ny + 1) * z)];
    let corner_pos =
        |x: usize, y: usize, z: usize| origin + Vec3::new(x as f32, y as f32, z as f32) * cell;

    // One vertex per cell the surface passes through, at the average of the
    // points where it crosses the cell's edges.
    let mut mesh = PolyMesh::default();
    let mut cell_vert: HashMap<(usize, usize, usize), u32> = HashMap::new();
    type Corner = (usize, usize, usize);
    const EDGES: [(Corner, Corner); 12] = [
        ((0, 0, 0), (1, 0, 0)),
        ((0, 1, 0), (1, 1, 0)),
        ((0, 0, 1), (1, 0, 1)),
        ((0, 1, 1), (1, 1, 1)),
        ((0, 0, 0), (0, 1, 0)),
        ((1, 0, 0), (1, 1, 0)),
        ((0, 0, 1), (0, 1, 1)),
        ((1, 0, 1), (1, 1, 1)),
        ((0, 0, 0), (0, 0, 1)),
        ((1, 0, 0), (1, 0, 1)),
        ((0, 1, 0), (0, 1, 1)),
        ((1, 1, 0), (1, 1, 1)),
    ];
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                let mut sum = Vec3::ZERO;
                let mut count = 0;
                for (a, b) in EDGES {
                    let (pa, pb) = ((x + a.0, y + a.1, z + a.2), (x + b.0, y + b.1, z + b.2));
                    let (va, vb) = (at(pa.0, pa.1, pa.2), at(pb.0, pb.1, pb.2));
                    if (va < 0.0) != (vb < 0.0) {
                        let t = va / (va - vb);
                        sum += corner_pos(pa.0, pa.1, pa.2).lerp(corner_pos(pb.0, pb.1, pb.2), t);
                        count += 1;
                    }
                }
                if count > 0 {
                    let id = mesh.add_vertex(sum / count as f32);
                    cell_vert.insert((x, y, z), id);
                }
            }
        }
    }

    // One quad per grid edge the surface crosses, joining the four cells
    // around that edge, wound so it faces outward.
    let quad = |c: [(usize, usize, usize); 4], inside_first: bool| -> Option<Vec<u32>> {
        let ids: Option<Vec<u32>> = c.iter().map(|k| cell_vert.get(k).copied()).collect();
        let mut ids = ids?;
        if !inside_first {
            ids.reverse();
        }
        Some(ids)
    };
    let mut faces = Vec::new();
    for z in 1..nz {
        for y in 1..ny {
            for x in 0..nx {
                let (a, b) = (at(x, y, z), at(x + 1, y, z));
                if (a < 0.0) != (b < 0.0) {
                    faces.extend(quad(
                        [(x, y - 1, z - 1), (x, y, z - 1), (x, y, z), (x, y - 1, z)],
                        a < 0.0,
                    ));
                }
            }
        }
    }
    for z in 1..nz {
        for y in 0..ny {
            for x in 1..nx {
                let (a, b) = (at(x, y, z), at(x, y + 1, z));
                if (a < 0.0) != (b < 0.0) {
                    faces.extend(quad(
                        [(x - 1, y, z - 1), (x - 1, y, z), (x, y, z), (x, y, z - 1)],
                        a < 0.0,
                    ));
                }
            }
        }
    }
    for z in 0..nz {
        for y in 1..ny {
            for x in 1..nx {
                let (a, b) = (at(x, y, z), at(x, y, z + 1));
                if (a < 0.0) != (b < 0.0) {
                    faces.extend(quad(
                        [(x - 1, y - 1, z), (x, y - 1, z), (x, y, z), (x - 1, y, z)],
                        a < 0.0,
                    ));
                }
            }
        }
    }
    for f in faces {
        mesh.add_face(f, true);
    }
    if mesh.faces.is_empty() {
        return Err("the parts produced no surface (is everything subtracted away?)".into());
    }

    // Snap vertices onto the true surface, then relax them so the quads are
    // even, snapping again after each pass.
    let neighbours = vertex_neighbours(&mesh);
    let snap = |mesh: &mut PolyMesh| {
        let pos = std::mem::take(&mut mesh.positions);
        mesh.positions = parallel_map(&pos, |p| {
            let mut p = *p;
            for _ in 0..2 {
                let d = field.eval(p);
                let g = field.gradient(p, cell * 0.25);
                let gl = g.length_squared();
                if gl > 1e-8 {
                    let step = g * (d / gl);
                    // Never move more than a cell: guards against bad gradients.
                    p -= step.clamp_length_max(cell);
                }
            }
            p
        });
    };
    snap(&mut mesh);
    for _ in 0..opts.relax {
        let old = mesh.positions.clone();
        for (i, nb) in neighbours.iter().enumerate() {
            if nb.is_empty() {
                continue;
            }
            let avg = nb.iter().map(|&j| old[j as usize]).sum::<Vec3>() / nb.len() as f32;
            mesh.positions[i] = old[i].lerp(avg, 0.5);
        }
        snap(&mut mesh);
    }
    Ok(mesh)
}

/// A grid face whose corners alternate inside, outside, inside, outside has
/// two possible surfaces, and Surface Nets would join four quads along one
/// edge (a tear). Nudge the inside corner nearest the surface outside, which
/// picks the "separate" reading; repeat until no such face is left.
fn resolve_ambiguous_faces(values: &mut [f32], nx: usize, ny: usize, nz: usize) {
    let idx = |x: usize, y: usize, z: usize| x + (nx + 1) * (y + (ny + 1) * z);
    loop {
        let mut changed = false;
        for z in 0..=nz {
            for y in 0..=ny {
                for x in 0..=nx {
                    // The three faces with (x, y, z) as their lowest corner.
                    let faces: [Option<[usize; 4]>; 3] = [
                        (x < nx && y < ny).then(|| {
                            [
                                idx(x, y, z),
                                idx(x + 1, y, z),
                                idx(x + 1, y + 1, z),
                                idx(x, y + 1, z),
                            ]
                        }),
                        (y < ny && z < nz).then(|| {
                            [
                                idx(x, y, z),
                                idx(x, y + 1, z),
                                idx(x, y + 1, z + 1),
                                idx(x, y, z + 1),
                            ]
                        }),
                        (x < nx && z < nz).then(|| {
                            [
                                idx(x, y, z),
                                idx(x + 1, y, z),
                                idx(x + 1, y, z + 1),
                                idx(x, y, z + 1),
                            ]
                        }),
                    ];
                    for c in faces.into_iter().flatten() {
                        let s = c.map(|i| values[i] < 0.0);
                        if s[0] == s[2] && s[1] == s[3] && s[0] != s[1] {
                            // Only ever flip inside to outside, so this settles.
                            let k = *c
                                .iter()
                                .filter(|i| values[**i] < 0.0)
                                .max_by(|a, b| values[**a].total_cmp(&values[**b]))
                                .expect("two inside corners");
                            values[k] = 1e-7;
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn parallel_map(input: &[Vec3], f: impl Fn(&Vec3) -> Vec3 + Sync) -> Vec<Vec3> {
    let threads = std::thread::available_parallelism()
        .map(|t| t.get())
        .unwrap_or(4);
    let per = input.len().div_ceil(threads).max(1);
    let mut out = vec![Vec3::ZERO; input.len()];
    std::thread::scope(|s| {
        for (src, dst) in input.chunks(per).zip(out.chunks_mut(per)) {
            let f = &f;
            s.spawn(move || {
                for (a, b) in src.iter().zip(dst.iter_mut()) {
                    *b = f(a);
                }
            });
        }
    });
    out
}

/// Each vertex's edge-connected neighbours.
pub fn vertex_neighbours(mesh: &PolyMesh) -> Vec<Vec<u32>> {
    let mut nb: Vec<Vec<u32>> = vec![Vec::new(); mesh.positions.len()];
    for f in &mesh.faces {
        for (a, b) in f.edges() {
            if !nb[a as usize].contains(&b) {
                nb[a as usize].push(b);
            }
            if !nb[b as usize].contains(&a) {
                nb[b as usize].push(a);
            }
        }
    }
    nb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coarse() -> MeshOptions {
        MeshOptions {
            resolution: 40,
            relax: 2,
            fit_thin_parts: true,
        }
    }

    #[test]
    fn sphere_is_watertight_and_round() {
        let parts = [SdfPart::add(
            "ball",
            ellipsoid(Vec3::ZERO, Vec3::splat(0.5)),
            0.0,
        )];
        let m = build(&parts, coarse()).unwrap();
        assert!(m.is_closed_manifold(), "not watertight");
        let v = m.volume();
        let exact = 4.0 / 3.0 * std::f32::consts::PI * 0.125;
        assert!((v - exact).abs() / exact < 0.03, "volume {v} vs {exact}");
        for p in &m.positions {
            assert!(
                (p.length() - 0.5).abs() < 0.01,
                "vertex off the surface: {p}"
            );
        }
    }

    #[test]
    fn blending_fills_the_join() {
        let a = SdfPart::add(
            "a",
            ellipsoid(Vec3::new(-0.3, 0.0, 0.0), Vec3::splat(0.25)),
            0.0,
        );
        let b = |k| {
            SdfPart::add(
                "b",
                ellipsoid(Vec3::new(0.3, 0.0, 0.0), Vec3::splat(0.25)),
                k,
            )
        };
        let hard = Field::new(&[a.clone(), b(0.0)])
            .unwrap()
            .eval(Vec3::new(0.0, 0.2, 0.0));
        let soft = Field::new(&[a, b(0.2)])
            .unwrap()
            .eval(Vec3::new(0.0, 0.2, 0.0));
        assert!(soft < hard, "a blend should bulge into the gap");
    }

    #[test]
    fn subtract_and_mirror() {
        let body = SdfPart::add("body", rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.1), 0.0);
        let socket = SdfPart::add(
            "socket",
            ellipsoid(Vec3::new(0.3, 0.2, 0.5), Vec3::splat(0.12)),
            0.02,
        )
        .subtract()
        .mirrored();
        let f = Field::new(&[body, socket]).unwrap();
        assert!(f.eval(Vec3::new(0.3, 0.2, 0.48)) > 0.0, "socket carved");
        assert!(
            f.eval(Vec3::new(-0.3, 0.2, 0.48)) > 0.0,
            "mirrored socket carved"
        );
        assert!(f.eval(Vec3::ZERO) < 0.0);
    }

    #[test]
    fn capsule_limb_is_watertight() {
        let parts = [
            SdfPart::add(
                "torso",
                ellipsoid(Vec3::ZERO, Vec3::new(0.2, 0.3, 0.15)),
                0.0,
            ),
            SdfPart::add(
                "arm",
                capsule(
                    Vec3::new(0.15, 0.2, 0.0),
                    Vec3::new(0.5, -0.2, 0.0),
                    0.07,
                    0.04,
                ),
                0.05,
            )
            .mirrored(),
        ];
        let m = build(&parts, coarse()).unwrap();
        assert!(m.is_closed_manifold());
        assert!(m.volume() > 0.0, "faces must point outward");
        let b = m.bounds().unwrap();
        assert!(b.min.x < -0.5 && b.max.x > 0.5, "both arms present");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(build(&[], coarse()).is_err());
        let bad = SdfPart::add("x", ellipsoid(Vec3::ZERO, Vec3::new(0.0, 1.0, 1.0)), 0.0);
        assert!(build(&[bad], coarse()).is_err());
    }
}
