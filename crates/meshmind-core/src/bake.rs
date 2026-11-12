//! Detail baking: carry the shape of a dense, sculpted mesh over to a light,
//! game-ready one as textures, the way every AAA game asset is made.
//!
//! For each texel of the light mesh's UV layout, a ray goes out and in along
//! the surface normal to find the dense surface. Its normal is stored in the
//! light mesh's tangent space (a normal map), so lighting shows every crease
//! and bump the light mesh no longer has. Ambient occlusion is gathered on
//! the dense surface too: how much of the sky each point sees, which darkens
//! creases and contact areas.

use glam::{Vec2, Vec3};

use crate::poly::PolyMesh;
use crate::texture::{to_u8, Texture};

/// Baked maps, same size and UV layout as the light mesh.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DetailMaps {
    /// Tangent-space normal map (glTF/OpenGL convention).
    pub normal: Texture,
    /// Ambient occlusion in the red channel (1 = fully open).
    pub occlusion: Texture,
}

#[derive(Debug, Clone, Copy)]
pub struct BakeOptions {
    pub size: u32,
    /// How far from the light surface to look for the dense one (metres).
    /// `None` uses 2% of the mesh's size.
    pub max_distance: Option<f32>,
    /// Rays per texel for ambient occlusion; 0 skips it.
    pub ao_rays: u32,
}

impl Default for BakeOptions {
    fn default() -> Self {
        BakeOptions {
            size: 1024,
            max_distance: None,
            ao_rays: 24,
        }
    }
}

/// Per-corner shading normals with the same 50 degree auto-smooth rule the
/// renderer and exporter use.
pub(crate) fn corner_normals(mesh: &PolyMesh) -> Vec<Vec<Vec3>> {
    let mut vn = vec![Vec3::ZERO; mesh.positions.len()];
    for f in mesh.faces.iter().filter(|f| f.smooth) {
        let n = mesh.face_normal_area(f);
        for &v in &f.verts {
            vn[v as usize] += n;
        }
    }
    let cos_limit = crate::poly::AUTO_SMOOTH_DEGREES.to_radians().cos();
    mesh.faces
        .iter()
        .map(|f| {
            let face_n = mesh.face_normal(f);
            f.verts
                .iter()
                .map(|&v| {
                    let s = vn[v as usize].normalize_or(face_n);
                    if f.smooth && s.dot(face_n) >= cos_limit {
                        s
                    } else {
                        face_n
                    }
                })
                .collect()
        })
        .collect()
}

struct Tri {
    p: [Vec3; 3],
    n: [Vec3; 3],
}

struct Node {
    lo: Vec3,
    hi: Vec3,
    /// Leaf: first triangle index and count. Inner: left child, 0.
    start: u32,
    count: u32,
    right: u32,
}

/// A bounding volume hierarchy over triangles, for fast ray casts.
struct Bvh {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
}

impl Bvh {
    fn new(mut tris: Vec<Tri>) -> Bvh {
        let mut nodes = Vec::new();
        let n = tris.len();
        Self::build(&mut tris, 0, n, &mut nodes);
        Bvh { tris, nodes }
    }

    fn build(tris: &mut [Tri], start: usize, end: usize, nodes: &mut Vec<Node>) -> u32 {
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for t in &tris[start..end] {
            for p in t.p {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let id = nodes.len() as u32;
        nodes.push(Node {
            lo,
            hi,
            start: start as u32,
            count: (end - start) as u32,
            right: 0,
        });
        if end - start <= 4 {
            return id;
        }
        let ext = hi - lo;
        let axis = if ext.x >= ext.y && ext.x >= ext.z {
            0
        } else if ext.y >= ext.z {
            1
        } else {
            2
        };
        let c = |t: &Tri| (t.p[0][axis] + t.p[1][axis] + t.p[2][axis]) / 3.0;
        let mid = (start + end) / 2;
        tris[start..end].select_nth_unstable_by(mid - start, |a, b| c(a).total_cmp(&c(b)));
        Self::build(tris, start, mid, nodes);
        let right = Self::build(tris, mid, end, nodes);
        let node = &mut nodes[id as usize];
        node.count = 0;
        node.right = right;
        id
    }

    /// Nearest hit with t in (0, t_max]: (t, triangle, barycentric u, v).
    fn cast(&self, o: Vec3, d: Vec3, t_max: f32) -> Option<(f32, usize, f32, f32)> {
        let inv = Vec3::ONE / d;
        let mut best: Option<(f32, usize, f32, f32)> = None;
        let mut limit = t_max;
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let node = &self.nodes[i as usize];
            let t0 = (node.lo - o) * inv;
            let t1 = (node.hi - o) * inv;
            let tmin = t0.min(t1).max_element().max(0.0);
            let tmax = t0.max(t1).min_element().min(limit);
            if tmin > tmax {
                continue;
            }
            if node.count > 0 {
                for k in node.start..node.start + node.count {
                    let t = &self.tris[k as usize];
                    if let Some((th, u, v)) = ray_tri(o, d, t.p) {
                        if th > 1e-6 && th <= limit {
                            limit = th;
                            best = Some((th, k as usize, u, v));
                        }
                    }
                }
            } else {
                stack.push(i + 1);
                stack.push(node.right);
            }
        }
        best
    }

    fn occluded(&self, o: Vec3, d: Vec3, t_max: f32) -> bool {
        self.cast(o, d, t_max).is_some()
    }
}

/// Möller-Trumbore; returns (t, u, v), either side.
fn ray_tri(o: Vec3, d: Vec3, p: [Vec3; 3]) -> Option<(f32, f32, f32)> {
    let e1 = p[1] - p[0];
    let e2 = p[2] - p[0];
    let h = d.cross(e2);
    let a = e1.dot(h);
    if a.abs() < 1e-12 {
        return None;
    }
    let f = 1.0 / a;
    let s = o - p[0];
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some((f * e2.dot(q), u, v))
}

/// A texel to bake: where it is on the light surface and its tangent frame.
struct Sample {
    x: u32,
    y: u32,
    p: Vec3,
    n: Vec3,
    t: Vec3,
    b: Vec3,
}

fn hash(x: u32, y: u32, i: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ i.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFFFF) as f32 / 16_777_216.0
}

/// Bake `high`'s surface detail onto `low`, which must have UVs. Both meshes
/// are in the same coordinate space.
pub fn bake_detail(
    low: &PolyMesh,
    high: &PolyMesh,
    opts: BakeOptions,
) -> Result<DetailMaps, String> {
    if !low.has_uvs() {
        return Err("the light mesh has no UVs; unwrap it first".into());
    }
    if !(16..=4096).contains(&opts.size) {
        return Err("texture size must be from 16 to 4096".into());
    }
    let bounds = high.bounds().ok_or("the detailed mesh is empty")?;
    let diag = bounds.size().length().max(1e-3);
    let reach = opts.max_distance.unwrap_or(diag * 0.02).max(1e-5);
    let ao_reach = diag * 0.06;

    let hn = corner_normals(high);
    let mut tris = Vec::new();
    for (f, n) in high.faces.iter().zip(&hn) {
        for t in high.triangulate_face(f) {
            tris.push(Tri {
                p: t.map(|k| high.positions[f.verts[k] as usize]),
                n: t.map(|k| n[k]),
            });
        }
    }
    let bvh = Bvh::new(tris);

    // Rasterise the light mesh into texel samples.
    let size = opts.size;
    let ln = corner_normals(low);
    let mut samples = Vec::new();
    for (f, n) in low.faces.iter().zip(&ln) {
        for tri in low.triangulate_face(f) {
            let p = tri.map(|k| low.positions[f.verts[k] as usize]);
            let nn = tri.map(|k| n[k]);
            let uv = tri.map(|k| f.uvs[k]);
            let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
            let (d1, d2) = (uv[1] - uv[0], uv[2] - uv[0]);
            let det = d1.x * d2.y - d2.x * d1.y;
            let (tu, tv) = if det.abs() > 1e-12 {
                ((e1 * d2.y - e2 * d1.y) / det, (e2 * d1.x - e1 * d2.x) / det)
            } else {
                (e1, e2)
            };
            raster(uv.map(|u| u * size as f32), size, |x, y, w| {
                let pos = p[0] * w[0] + p[1] * w[1] + p[2] * w[2];
                let n = (nn[0] * w[0] + nn[1] * w[1] + nn[2] * w[2]).normalize_or(Vec3::Y);
                // The same tangent frame the renderer and glTF viewers rebuild.
                let t = (tu - n * n.dot(tu)).normalize_or_zero();
                let sign = if n.cross(t).dot(tv) < 0.0 { -1.0 } else { 1.0 };
                let b = -n.cross(t) * sign;
                samples.push(Sample {
                    x,
                    y,
                    p: pos,
                    n,
                    t,
                    b,
                });
            });
        }
    }

    // Bake every sample, spread over all cores.
    let threads = std::thread::available_parallelism()
        .map(|t| t.get())
        .unwrap_or(4);
    let per = samples.len().div_ceil(threads).max(1);
    let results: Vec<Vec<(u32, u32, Vec3, f32)>> = std::thread::scope(|s| {
        let handles: Vec<_> = samples
            .chunks(per)
            .map(|chunk| {
                let bvh = &bvh;
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|sm| {
                            // Nearest dense surface along the normal, either side.
                            let out = bvh.cast(sm.p, sm.n, reach);
                            let inn = bvh.cast(sm.p, -sm.n, reach);
                            let hit = match (out, inn) {
                                (Some(a), Some(b)) => {
                                    Some(if a.0 <= b.0 { (a, 1.0) } else { (b, -1.0) })
                                }
                                (Some(a), None) => Some((a, 1.0)),
                                (None, Some(b)) => Some((b, -1.0)),
                                (None, None) => None,
                            };
                            let Some(((th, ti, u, v), side)) = hit else {
                                return (sm.x, sm.y, Vec3::Z, 1.0);
                            };
                            let tri = &bvh.tris[ti];
                            let mut nh = (tri.n[0] * (1.0 - u - v) + tri.n[1] * u + tri.n[2] * v)
                                .normalize_or(sm.n);
                            if nh.dot(sm.n) < 0.0 {
                                nh = sm.n; // hit the back of something else
                            }
                            let tn = Vec3::new(nh.dot(sm.t), nh.dot(sm.b), nh.dot(sm.n))
                                .normalize_or(Vec3::Z);
                            let mut ao = 1.0;
                            if opts.ao_rays > 0 {
                                let ph = sm.p + sm.n * th * side + nh * (diag * 1e-4);
                                let (ta, tb) = {
                                    let a = if nh.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                                    let t = nh.cross(a).normalize();
                                    (t, nh.cross(t))
                                };
                                let mut hits = 0;
                                for i in 0..opts.ao_rays {
                                    // Cosine-weighted directions.
                                    let r1 = hash(sm.x, sm.y, i * 2);
                                    let r2 = hash(sm.x, sm.y, i * 2 + 1);
                                    let phi = std::f32::consts::TAU * r1;
                                    let r = r2.sqrt();
                                    let d = (ta * (phi.cos() * r)
                                        + tb * (phi.sin() * r)
                                        + nh * (1.0 - r2).sqrt())
                                    .normalize();
                                    if bvh.occluded(ph, d, ao_reach) {
                                        hits += 1;
                                    }
                                }
                                ao = 1.0 - hits as f32 / opts.ao_rays as f32;
                            }
                            (sm.x, sm.y, tn, ao)
                        })
                        .collect()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("bake thread"))
            .collect()
    });

    let n = size as usize;
    let mut filled: Vec<Option<(Vec3, f32)>> = vec![None; n * n];
    for (x, y, tn, ao) in results.into_iter().flatten() {
        filled[y as usize * n + x as usize] = Some((tn, ao));
    }
    dilate(&mut filled, n, 8);
    let mut normal = Texture::new("detail_normal", size, size);
    let mut occlusion = Texture::new("occlusion", size, size);
    for y in 0..n {
        for x in 0..n {
            let (tn, ao) = filled[y * n + x].unwrap_or((Vec3::Z, 1.0));
            normal.set(
                x as u32,
                y as u32,
                [
                    to_u8(tn.x * 0.5 + 0.5),
                    to_u8(tn.y * 0.5 + 0.5),
                    to_u8(tn.z * 0.5 + 0.5),
                    255,
                ],
            );
            let a = to_u8(ao);
            occlusion.set(x as u32, y as u32, [a, a, a, 255]);
        }
    }
    Ok(DetailMaps { normal, occlusion })
}

/// Visit texel centres inside a UV triangle (pixels), with barycentrics.
/// Calls `f` for every texel whose centre is inside the triangle. A sliver
/// that covers no centre still gets the texel under its middle, so tiny
/// triangles never end up with no texture at all.
fn raster(uv: [Vec2; 3], size: u32, mut f: impl FnMut(u32, u32, [f32; 3])) {
    let lo = uv[0].min(uv[1]).min(uv[2]);
    let hi = uv[0].max(uv[1]).max(uv[2]);
    let area = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]);
    if area.abs() < 1e-12 {
        return;
    }
    let x0 = (lo.x.floor().max(0.0)) as u32;
    let y0 = (lo.y.floor().max(0.0)) as u32;
    let x1 = (hi.x.ceil() as i64).clamp(0, size as i64 - 1) as u32;
    let y1 = (hi.y.ceil() as i64).clamp(0, size as i64 - 1) as u32;
    let mut any = false;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = (uv[1] - p).perp_dot(uv[2] - p) / area;
            let w1 = (uv[2] - p).perp_dot(uv[0] - p) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 >= -1e-4 && w1 >= -1e-4 && w2 >= -1e-4 {
                f(x, y, [w0, w1, w2]);
                any = true;
            }
        }
    }
    if !any {
        let c = (uv[0] + uv[1] + uv[2]) / 3.0;
        if c.x >= 0.0 && c.y >= 0.0 && c.x < size as f32 && c.y < size as f32 {
            f(c.x as u32, c.y as u32, [1.0 / 3.0; 3]);
        }
    }
}

fn dilate<T: Copy>(px: &mut [Option<T>], n: usize, passes: usize) {
    for _ in 0..passes {
        let prev = px.to_vec();
        for y in 0..n {
            for x in 0..n {
                if prev[y * n + x].is_some() {
                    continue;
                }
                let around = [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)];
                for (dx, dy) in around {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < n && (ny as usize) < n {
                        if let Some(v) = prev[ny as usize * n + nx as usize] {
                            px[y * n + x] = Some(v);
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sculpt::{sculpt, Brush, Stroke};
    use crate::sdf::{build, ellipsoid, MeshOptions, SdfPart};

    #[test]
    fn a_crease_on_the_dense_mesh_shows_in_the_normal_map() {
        let ball = build(
            &[SdfPart::add(
                "b",
                ellipsoid(Vec3::ZERO, Vec3::splat(0.5)),
                0.0,
            )],
            MeshOptions {
                resolution: 64,
                ..Default::default()
            },
        )
        .unwrap();
        let (high, _) = sculpt(
            &ball,
            Brush::Crease,
            &Stroke {
                points: vec![Vec3::new(-0.3, 0.0, 0.4), Vec3::new(0.3, 0.0, 0.4)],
                radius: 0.05,
                strength: 0.03,
                symmetric: false,
            },
        )
        .unwrap();
        let mut low = crate::simplify::simplify(&high, 600).unwrap();
        crate::uv::unwrap(&mut low, &Default::default()).unwrap();
        let maps = bake_detail(
            &low,
            &high,
            BakeOptions {
                size: 128,
                ao_rays: 8,
                ..Default::default()
            },
        )
        .unwrap();
        // Away from flat (128, 128, 255) somewhere: the crease's slopes.
        let tilted = maps.normal.pixels.chunks(4).filter(|p| p[2] < 235).count();
        assert!(
            tilted > 20,
            "the crease should tilt normals ({tilted} texels)"
        );
        let dark = maps
            .occlusion
            .pixels
            .chunks(4)
            .filter(|p| p[0] < 200)
            .count();
        assert!(dark > 0, "the crease should be occluded somewhere");
    }
}
