//! Smart materials: believable surfaces baked into textures from the shape
//! itself. Paint wears off convex edges, dirt and rust collect in creases,
//! noise breaks up flat colour, and fine bumps go into a normal map.
//!
//! Output is a glTF-ready set: base colour (sRGB), metallic-roughness
//! (G = roughness, B = metallic) and a tangent-space normal map.

use std::collections::HashMap;

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::poly::PolyMesh;
use crate::texture::{linear_to_srgb, to_u8, Texture};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Paint over metal; worn edges show bare metal.
    PaintedMetal,
    /// Machined or cast metal with polished edges.
    BareMetal,
    /// Old metal with rust in creases and patches.
    RustyMetal,
    /// Moulded plastic with scuffed edges.
    Plastic,
    /// Wood with grain, lighter worn edges.
    Wood,
    /// Stone or concrete with chipped edges.
    Stone,
    /// Dark matte rubber.
    Rubber,
    /// Polished steel: mirror-like, brushed along the object's Y axis, with
    /// fine scratches. Blades, armour, tools, chrome.
    PolishedMetal,
    /// Gold or brass: polished, warm, tarnished in the creases.
    Gold,
    /// Leather: pebbled grain, darker in creases, lighter and shinier where worn.
    Leather,
    /// Bronze with green verdigris in the creases and polished high points.
    Bronze,
    /// Woven cloth: banners, sacks, upholstery, clothing.
    Fabric,
    /// Glazed ceramic: pottery, tiles. Chips show the clay underneath.
    Ceramic,
    /// Bone, ivory or horn: fibrous, porous, stained in the creases.
    Bone,
    /// Clear glass, see-through. The colour tints it.
    Glass,
    /// A see-through, deeply coloured gem (ruby by default).
    Gemstone,
    /// Skin or hide: mottled tone that warms toward red in creases, soft
    /// sheen, pores and fine wrinkles. Any colour: human, orc green, troll grey.
    Skin,
}

impl Preset {
    pub const ALL: [Preset; 17] = [
        Preset::PaintedMetal,
        Preset::BareMetal,
        Preset::RustyMetal,
        Preset::Plastic,
        Preset::Wood,
        Preset::Stone,
        Preset::Rubber,
        Preset::PolishedMetal,
        Preset::Gold,
        Preset::Leather,
        Preset::Bronze,
        Preset::Fabric,
        Preset::Ceramic,
        Preset::Bone,
        Preset::Glass,
        Preset::Gemstone,
        Preset::Skin,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::PaintedMetal => "painted_metal",
            Preset::BareMetal => "bare_metal",
            Preset::RustyMetal => "rusty_metal",
            Preset::Plastic => "plastic",
            Preset::Wood => "wood",
            Preset::Stone => "stone",
            Preset::Rubber => "rubber",
            Preset::PolishedMetal => "polished_metal",
            Preset::Gold => "gold",
            Preset::Leather => "leather",
            Preset::Bronze => "bronze",
            Preset::Fabric => "fabric",
            Preset::Ceramic => "ceramic",
            Preset::Bone => "bone",
            Preset::Glass => "glass",
            Preset::Gemstone => "gemstone",
            Preset::Skin => "skin",
        }
    }

    pub fn from_name(s: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.name() == s)
    }

    fn default_color(self) -> Vec3 {
        match self {
            Preset::PaintedMetal => Vec3::new(0.12, 0.16, 0.06),
            Preset::BareMetal => Vec3::new(0.56, 0.57, 0.58),
            Preset::RustyMetal => Vec3::new(0.35, 0.33, 0.31),
            Preset::Plastic => Vec3::new(0.6, 0.6, 0.6),
            Preset::Wood => Vec3::new(0.32, 0.17, 0.07),
            Preset::Stone => Vec3::new(0.2, 0.19, 0.17),
            Preset::Rubber => Vec3::new(0.025, 0.025, 0.025),
            Preset::PolishedMetal => Vec3::new(0.78, 0.79, 0.8),
            Preset::Gold => Vec3::new(1.0, 0.71, 0.29),
            Preset::Leather => Vec3::new(0.12, 0.055, 0.025),
            Preset::Bronze => Vec3::new(0.62, 0.36, 0.17),
            Preset::Fabric => Vec3::new(0.32, 0.05, 0.04),
            Preset::Ceramic => Vec3::new(0.7, 0.68, 0.62),
            Preset::Bone => Vec3::new(0.5, 0.41, 0.27),
            Preset::Glass => Vec3::new(0.95, 0.99, 0.97),
            Preset::Gemstone => Vec3::new(0.75, 0.02, 0.06),
            Preset::Skin => Vec3::new(0.56, 0.3, 0.2),
        }
    }

    /// How see-through the material is (glTF transmission).
    pub fn transmission(self) -> f32 {
        match self {
            Preset::Glass | Preset::Gemstone => 1.0,
            _ => 0.0,
        }
    }

    /// Index of refraction.
    pub fn ior(self) -> f32 {
        match self {
            Preset::Gemstone => 1.77,
            _ => 1.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartMaterial {
    pub preset: Preset,
    /// Main colour, linear RGB. `None` uses the preset's colour.
    pub color: Option<[f32; 3]>,
    /// Edge wear amount, 0 (new) to 1 (heavily worn).
    pub wear: f32,
    /// Dirt and grime amount, 0 (clean) to 1 (filthy).
    pub dirt: f32,
    /// Size of noise features in metres (blotches, grain spacing).
    pub scale: f32,
    pub seed: u32,
}

impl SmartMaterial {
    pub fn new(preset: Preset) -> SmartMaterial {
        SmartMaterial {
            preset,
            color: None,
            wear: 0.4,
            dirt: 0.3,
            scale: 0.15,
            seed: 1,
        }
    }
}

pub struct Baked {
    pub base_color: Texture,
    pub metallic_roughness: Texture,
    pub normal: Texture,
}

/// A surface sample before it is written to the textures.
#[derive(Clone, Copy)]
struct Surface {
    color: Vec3,
    metallic: f32,
    roughness: f32,
    height: f32,
}

struct Edge {
    a: Vec3,
    b: Vec3,
    /// 0 to 1: how much this edge counts.
    strength: f32,
    convex: bool,
}

/// Bake `mat` for `mesh` (which must have UVs) at `size` × `size` pixels.
pub fn bake(mesh: &PolyMesh, mat: &SmartMaterial, size: u32) -> Result<Baked, String> {
    if !mesh.has_uvs() {
        return Err("the mesh has no UVs; unwrap it first".into());
    }
    if !(16..=4096).contains(&size) {
        return Err("texture size must be from 16 to 4096".into());
    }
    let bounds = mesh.bounds().ok_or("empty mesh")?;
    let diag = bounds.size().length().max(1e-3);
    let edges = feature_edges(mesh, diag);
    let wear_w = diag * 0.012 * (0.5 + mat.wear);
    let cavity_w = diag * 0.03 * (0.5 + mat.dirt);
    let reach = wear_w.max(cavity_w) * 1.8;

    // Smooth vertex normals for smooth faces.
    let mut vn = vec![Vec3::ZERO; mesh.positions.len()];
    for f in mesh.faces.iter().filter(|f| f.smooth) {
        let n = mesh.face_normal_area(f);
        for &v in &f.verts {
            vn[v as usize] += n;
        }
    }

    let n = size as usize;
    let mut surf: Vec<Option<Surface>> = vec![None; n * n];
    for f in &mesh.faces {
        let face_n = mesh.face_normal(f);
        let pts: Vec<Vec3> = f
            .verts
            .iter()
            .map(|&v| mesh.positions[v as usize])
            .collect();
        let nrm: Vec<Vec3> = f
            .verts
            .iter()
            .map(|&v| {
                let s = vn[v as usize].normalize_or(face_n);
                let cos_limit = crate::poly::AUTO_SMOOTH_DEGREES.to_radians().cos();
                if f.smooth && s.dot(face_n) >= cos_limit {
                    s
                } else {
                    face_n
                }
            })
            .collect();
        // Edges close enough to matter for this face.
        let (lo, hi) = pts.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(l, h), p| (l.min(*p), h.max(*p)),
        );
        let (lo, hi) = (lo - Vec3::splat(reach), hi + Vec3::splat(reach));
        let near: Vec<&Edge> = edges
            .iter()
            .filter(|e| e.a.min(e.b).cmple(hi).all() && e.a.max(e.b).cmpge(lo).all())
            .collect();
        for tri in mesh.triangulate_face(f) {
            let uv = tri.map(|k| f.uvs[k] * size as f32);
            raster(uv, n, |x, y, w| {
                let p = pts[tri[0]] * w[0] + pts[tri[1]] * w[1] + pts[tri[2]] * w[2];
                let nn = (nrm[tri[0]] * w[0] + nrm[tri[1]] * w[1] + nrm[tri[2]] * w[2])
                    .normalize_or(face_n);
                let (mut convex_d, mut concave_d) = (f32::INFINITY, f32::INFINITY);
                for e in &near {
                    // Weaker edges act as if they were further away.
                    let d = seg_dist(p, e.a, e.b) / e.strength.max(1e-3);
                    if e.convex {
                        convex_d = convex_d.min(d);
                    } else {
                        concave_d = concave_d.min(d);
                    }
                }
                surf[y * n + x] = Some(shade(mat, p, nn, convex_d, concave_d, wear_w, cavity_w));
            });
        }
    }

    // Fill empty texels from their neighbours so seams do not show.
    dilate(&mut surf, n, 8);

    let mut base = Texture::new("base_color", size, size);
    let mut mr = Texture::new("metallic_roughness", size, size);
    let mut normal = Texture::new("normal", size, size);
    let fallback = Surface {
        color: Vec3::splat(0.5),
        metallic: 0.0,
        roughness: 0.5,
        height: 0.5,
    };
    let height_at = |x: usize, y: usize| {
        surf[y.min(n - 1) * n + x.min(n - 1)]
            .unwrap_or(fallback)
            .height
    };
    // Height is in texels of "depth"; this sets how strong the bumps look.
    let bump = 2.5;
    for y in 0..n {
        for x in 0..n {
            let s = surf[y * n + x].unwrap_or(fallback);
            let c = s.color.clamp(Vec3::ZERO, Vec3::ONE);
            base.set(
                x as u32,
                y as u32,
                [
                    to_u8(linear_to_srgb(c.x)),
                    to_u8(linear_to_srgb(c.y)),
                    to_u8(linear_to_srgb(c.z)),
                    255,
                ],
            );
            mr.set(
                x as u32,
                y as u32,
                [255, to_u8(s.roughness), to_u8(s.metallic), 255],
            );
            let dx = height_at(x + 1, y) - height_at(x.saturating_sub(1), y);
            let dy = height_at(x, y + 1) - height_at(x, y.saturating_sub(1));
            // glTF uses the OpenGL convention: green points to the top of the
            // image, i.e. towards smaller rows, so the y slope changes sign.
            let nm = Vec3::new(-dx * bump, dy * bump, 1.0).normalize();
            normal.set(
                x as u32,
                y as u32,
                [
                    to_u8(nm.x * 0.5 + 0.5),
                    to_u8(nm.y * 0.5 + 0.5),
                    to_u8(nm.z * 0.5 + 0.5),
                    255,
                ],
            );
        }
    }
    Ok(Baked {
        base_color: base,
        metallic_roughness: mr,
        normal,
    })
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn shade(
    m: &SmartMaterial,
    p: Vec3,
    n: Vec3,
    convex_d: f32,
    concave_d: f32,
    wear_w: f32,
    cavity_w: f32,
) -> Surface {
    let seed = m.seed as f32 * 17.13;
    let q = p / m.scale.max(1e-3) + Vec3::splat(seed);
    let big = fbm(q * 0.5, 4);
    let mid = fbm(q * 2.0 + 31.7, 4);
    let fine = fbm(q * 9.0 + 7.3, 3);

    // Edge wear: near convex edges, with a ragged chipped outline. The edge
    // factor fades with distance; high-frequency noise decides where paint
    // actually breaks, so the border is irregular rather than a clean stripe.
    let chip = fbm(q * 7.0 + 11.1, 4);
    let edge = 1.0 - (convex_d / (wear_w * 2.2)).min(1.0);
    let wear = smoothstep(
        0.52,
        0.6,
        edge * (0.35 + 0.9 * m.wear) + (chip - 0.5) * 0.9 + (mid - 0.5) * 0.4,
    );
    // Fine scratches: thin lines from stretched noise, only on the larger flat areas.
    let scratch_noise = fbm(Vec3::new(q.x * 60.0, q.y * 4.0, q.z * 60.0) + 3.1, 2);
    let scratches =
        smoothstep(0.02, 0.0, (scratch_noise - 0.5).abs()) * smoothstep(0.6, 0.8, mid) * m.wear;
    // Dirt: creases, upward faces (dust), and blotches.
    let cavity = 1.0 - smoothstep(0.0, cavity_w * (0.5 + big), concave_d);
    let dust = n.y.max(0.0) * smoothstep(0.45, 0.75, big) * 0.6;
    let dirt =
        ((cavity * 0.9 + dust + smoothstep(0.6, 0.85, mid) * 0.25) * m.dirt * 1.4).clamp(0.0, 1.0);

    let color = Vec3::from(
        m.color
            .unwrap_or_else(|| m.preset.default_color().to_array()),
    );
    let vary = 0.88 + 0.24 * big;
    let grime = Vec3::new(0.06, 0.05, 0.035);

    let mut s = match m.preset {
        Preset::PaintedMetal => {
            let metal = Vec3::new(0.55, 0.55, 0.56);
            let paint = color * vary;
            let mut s = Surface {
                color: paint,
                metallic: 0.0,
                roughness: 0.45 + 0.15 * mid,
                height: 0.6,
            };
            let bare = wear;
            s.roughness = (s.roughness + scratches * 0.3).min(1.0);
            s.color = s.color.lerp(metal * (0.8 + 0.3 * fine), bare);
            s.metallic = bare;
            s.roughness = s.roughness * (1.0 - bare) + 0.3 * bare;
            s.height -= bare * 0.4; // paint chips are recessed
            s
        }
        Preset::BareMetal => {
            let mut s = Surface {
                color: color * (0.92 + 0.12 * mid),
                metallic: 1.0,
                roughness: 0.32 + 0.18 * big,
                height: 0.5,
            };
            s.roughness = s.roughness * (1.0 - wear) + 0.12 * wear;
            s.color = s.color.lerp(color * 1.15, wear);
            s.roughness = (s.roughness + scratches * 0.25).min(1.0);
            s
        }
        Preset::RustyMetal => {
            let rust = Vec3::new(0.24, 0.075, 0.025).lerp(Vec3::new(0.38, 0.15, 0.05), fine);
            let mut s = Surface {
                color: color * vary,
                metallic: 1.0,
                roughness: 0.4 + 0.2 * mid,
                height: 0.5,
            };
            let rusty =
                (smoothstep(0.45, 0.7, big + cavity * 0.5) * (0.4 + m.dirt)).clamp(0.0, 1.0);
            s.color = s.color.lerp(rust, rusty);
            s.metallic = 1.0 - rusty;
            s.roughness = s.roughness * (1.0 - rusty) + 0.92 * rusty;
            s.height += rusty * 0.3 * fine;
            s.color = s.color.lerp(color * 1.25, wear * (1.0 - rusty));
            s
        }
        Preset::Plastic => {
            let mut s = Surface {
                color: color * (0.96 + 0.08 * big),
                metallic: 0.0,
                roughness: 0.42 + 0.1 * mid,
                height: 0.5,
            };
            s.color = s.color.lerp(s.color * 0.7 + Vec3::splat(0.25), wear * 0.6);
            s.roughness = s.roughness * (1.0 - wear) + 0.7 * wear;
            s.height += fine * 0.05;
            s
        }
        Preset::Wood => {
            // Grain runs along the object's longest axis (Y by default).
            let along = p.y / m.scale.max(1e-3);
            let ring = (((p.x * p.x + p.z * p.z).sqrt() / m.scale.max(1e-3) * 2.5
                + big * 3.0
                + along * 0.1)
                * std::f32::consts::TAU)
                .sin()
                * 0.5
                + 0.5;
            let streak = fbm(Vec3::new(q.x * 14.0, q.y * 0.3, q.z * 14.0), 4);
            let g = smoothstep(0.25, 0.95, ring * 0.35 + streak * 0.85);
            let mut s = Surface {
                color: color.lerp(color * 0.55, g),
                metallic: 0.0,
                roughness: 0.62 + 0.15 * g,
                height: 0.5 - g * 0.15,
            };
            s.color = s.color.lerp(color * 1.5 + Vec3::splat(0.05), wear * 0.7);
            s
        }
        Preset::Stone => {
            let mut s = Surface {
                color: color * (0.75 + 0.5 * big) * (0.92 + 0.16 * fine),
                metallic: 0.0,
                roughness: 0.82 + 0.12 * fine,
                height: 0.4 + 0.4 * fine + 0.2 * mid,
            };
            s.color = s.color.lerp(color * 1.35, wear);
            s.height -= wear * 0.5;
            s
        }
        Preset::Rubber => Surface {
            color: color * (0.9 + 0.2 * mid),
            metallic: 0.0,
            roughness: 0.85 + 0.1 * fine,
            height: 0.5 + fine * 0.08,
        },
        Preset::PolishedMetal | Preset::Gold => {
            // Brushing: long, thin streaks along Y, in absolute millimetres so
            // they read the same on any object size.
            let brush = fbm(Vec3::new(p.x * 900.0, p.y * 6.0, p.z * 900.0) + seed, 3);
            let streak = (brush - 0.5) * 2.0;
            // Sparse random scratches crossing the brushing.
            let sc = fbm(
                Vec3::new(q.x * 3.0 + q.y * 40.0, q.y * 3.0 - q.x * 40.0, q.z * 40.0),
                2,
            );
            let scratch = smoothstep(0.012, 0.0, (sc - 0.5).abs()) * smoothstep(0.55, 0.75, big);
            let base_rough = if m.preset == Preset::Gold { 0.16 } else { 0.1 };
            let mut s = Surface {
                color: color * (0.97 + 0.05 * mid),
                metallic: 1.0,
                roughness: base_rough + 0.06 * streak.abs() + 0.05 * big,
                height: 0.5 + streak * 0.015,
            };
            s.roughness = (s.roughness + scratch * (0.15 + 0.3 * m.wear)).min(1.0);
            s.height -= scratch * 0.08;
            if m.preset == Preset::Gold {
                // Tarnish darkens and dulls the creases.
                let tarnish = (cavity * (0.6 + 0.4 * mid)).clamp(0.0, 1.0);
                s.color = s.color.lerp(color * Vec3::new(0.35, 0.3, 0.22), tarnish);
                s.roughness = s.roughness * (1.0 - tarnish) + 0.5 * tarnish;
            }
            // Polished edges catch more light.
            s.roughness *= 1.0 - wear * 0.4;
            s
        }
        Preset::Leather => {
            // Pebble grain: cellular-looking bumps from sharpened noise.
            let grain = fbm(p * 900.0 + seed, 3);
            let pebble = smoothstep(0.35, 0.7, grain);
            let mut s = Surface {
                color: color * (0.8 + 0.35 * big) * (0.9 + 0.15 * pebble),
                metallic: 0.0,
                roughness: 0.62 + 0.12 * pebble,
                height: 0.45 + 0.15 * pebble,
            };
            // Creases: darker; worn high points: lighter and smoother (oiled).
            s.color *= 1.0 - cavity * 0.5;
            s.color = s
                .color
                .lerp(color * 1.6 + Vec3::new(0.015, 0.01, 0.005), wear * 0.5);
            s.roughness = s.roughness * (1.0 - wear) + 0.42 * wear;
            s
        }
        Preset::Bronze => {
            let patina_col = Vec3::new(0.09, 0.32, 0.24).lerp(Vec3::new(0.2, 0.42, 0.33), fine);
            let mut s = Surface {
                color: color * (0.9 + 0.15 * mid),
                metallic: 1.0,
                roughness: 0.3 + 0.15 * big,
                height: 0.5 + 0.02 * fine,
            };
            let patina = (smoothstep(
                0.42,
                0.68,
                mid * 0.6 + big * 0.3 + cavity * 0.9 + (fine - 0.5) * 0.35,
            ) * (0.5 + m.dirt))
                .clamp(0.0, 1.0);
            s.color = s.color.lerp(patina_col, patina);
            s.metallic = 1.0 - patina;
            s.roughness = s.roughness * (1.0 - patina) + 0.85 * patina;
            s.height += patina * 0.15 * fine;
            // Handled high points are polished bright.
            s.color = s.color.lerp(color * 1.2, wear * (1.0 - patina));
            s.roughness *= 1.0 - wear * 0.5;
            s
        }
        Preset::Fabric => {
            // A plain weave: threads ~4 mm apart (canvas, burlap; smaller `scale` is finer), projected along the face's main axis.
            let a = n.abs();
            let (u, v) = if a.y >= a.x && a.y >= a.z {
                (p.x, p.z)
            } else if a.x >= a.z {
                (p.z, p.y)
            } else {
                (p.x, p.y)
            };
            let pitch = 0.004 * (m.scale / 0.15).clamp(0.3, 4.0);
            let (u, v) = (u / pitch, v / pitch);
            let over = ((u.floor() + v.floor()) as i64).rem_euclid(2) == 0;
            let warp = (u.fract() * std::f32::consts::PI).sin();
            let weft = (v.fract() * std::f32::consts::PI).sin();
            let thread = if over { warp } else { weft };
            let fuzz = fbm(p * 1500.0 + seed, 2);
            let mut s = Surface {
                color: color * (0.7 + 0.3 * thread) * (0.92 + 0.16 * big) * (0.95 + 0.1 * fuzz),
                metallic: 0.0,
                roughness: 0.88 + 0.1 * fuzz,
                height: 0.46 + 0.08 * thread,
            };
            // Worn edges fray and fade.
            s.color = s.color.lerp(s.color * 0.6 + Vec3::splat(0.12), wear * 0.6);
            s.height -= wear * 0.15;
            s
        }
        Preset::Ceramic => {
            let clay = Vec3::new(0.42, 0.2, 0.11);
            // Fine crackle in the glaze.
            let crack_n = fbm(q * 6.0 + 5.5, 3);
            let crackle = smoothstep(0.012, 0.0, (crack_n - 0.5).abs());
            let mut s = Surface {
                color: color * (0.95 + 0.08 * big),
                metallic: 0.0,
                roughness: 0.08 + 0.05 * mid,
                height: 0.5 - crackle * 0.03,
            };
            s.color *= 1.0 - crackle * (0.15 + 0.4 * m.dirt);
            // Chips expose rough, unglazed clay.
            s.color = s.color.lerp(clay * (0.9 + 0.2 * fine), wear);
            s.roughness = s.roughness * (1.0 - wear) + 0.85 * wear;
            s.height -= wear * 0.4;
            s
        }
        Preset::Bone => {
            let fibre = fbm(Vec3::new(q.x * 40.0, q.y * 2.0, q.z * 40.0) + seed, 3);
            let pores = smoothstep(0.62, 0.72, fbm(p * 700.0 + seed, 2));
            let mut s = Surface {
                color: color * (0.85 + 0.25 * fibre) * (1.0 - pores * 0.25),
                metallic: 0.0,
                roughness: 0.5 + 0.2 * fibre + pores * 0.2,
                height: 0.5 + (fibre - 0.5) * 0.08 - pores * 0.12,
            };
            // Age stains the creases brown.
            let stain = (cavity * 0.8 + smoothstep(0.55, 0.8, big) * 0.4).min(1.0);
            s.color = s
                .color
                .lerp(color * Vec3::new(0.55, 0.42, 0.25), stain * 0.7);
            s.color = s.color.lerp(color * 1.15, wear * 0.5);
            s.roughness = s.roughness * (1.0 - wear) + 0.35 * wear;
            s
        }
        Preset::Skin => {
            // Pores and wrinkles at a fixed real-world size (millimetres).
            let pores = smoothstep(0.6, 0.74, fbm(p * 160.0 + seed, 2));
            let wrinkle = fbm(Vec3::new(p.x * 90.0, p.y * 380.0, p.z * 90.0) + seed, 2);
            let mottle = (big - 0.5) * 0.3 + (mid - 0.5) * 0.16 + (fine - 0.5) * 0.06;
            let base = color * (1.0 + mottle);
            // Blood shows through where skin is thin or folded: warmer, redder.
            let flush = (cavity * 0.7 + smoothstep(0.55, 0.85, mid) * 0.35).min(1.0);
            let warm = base * Vec3::new(1.18, 0.86, 0.8);
            Surface {
                color: base.lerp(warm, flush) * (1.0 - pores * 0.06),
                metallic: 0.0,
                roughness: 0.52 + 0.14 * fine + pores * 0.12 - cavity * 0.08,
                height: 0.5 - pores * 0.04 + (wrinkle - 0.5) * 0.04,
            }
        }
        Preset::Glass | Preset::Gemstone => {
            // Mostly clean and smooth; smudges and fine scratches show in the highlights.
            let smudge = smoothstep(0.55, 0.85, mid) * 0.12;
            let base = if m.preset == Preset::Gemstone {
                0.02
            } else {
                0.03
            };
            let tone = if m.preset == Preset::Gemstone {
                0.85 + 0.25 * big
            } else {
                1.0
            };
            Surface {
                color: color * tone,
                metallic: 0.0,
                roughness: (base + smudge + scratches * 0.3).min(1.0),
                height: 0.5,
            }
        }
    };
    // Dirt on top of everything.
    s.color = s.color.lerp(grime, dirt * 0.85);
    s.roughness = s.roughness * (1.0 - dirt) + 0.95 * dirt;
    s.metallic *= 1.0 - dirt;
    s
}

/// Edges that drive wear and dirt: hard edges, and the curved strips that
/// bevels create (tight curvature compared with the object's size).
fn feature_edges(mesh: &PolyMesh, diag: f32) -> Vec<Edge> {
    let normals: Vec<Vec3> = mesh.faces.iter().map(|f| mesh.face_normal(f)).collect();
    let centroids: Vec<Vec3> = mesh.faces.iter().map(|f| mesh.face_centroid(f)).collect();
    let areas: Vec<f32> = mesh
        .faces
        .iter()
        .map(|f| mesh.face_normal_area(f).length() / 2.0)
        .collect();
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, f) in mesh.faces.iter().enumerate() {
        for (a, b) in f.edges() {
            by_edge.entry((a.min(b), a.max(b))).or_default().push(fi);
        }
    }
    let mut out = Vec::new();
    for (&(a, b), faces) in &by_edge {
        if faces.len() != 2 {
            continue;
        }
        let (f, g) = (faces[0], faces[1]);
        let angle = normals[f].dot(normals[g]).clamp(-1.0, 1.0).acos();
        if angle < 0.05 {
            continue;
        }
        let (pa, pb) = (mesh.positions[a as usize], mesh.positions[b as usize]);
        let len = pa.distance(pb).max(1e-6);
        let width = (areas[f].min(areas[g]) / len).max(1e-6);
        // Curvature relative to object size: bevel strips score high, gently
        // curved surfaces (spheres, cylinder sides) score low.
        let curvature = angle / width * diag;
        // Gentle angles (tessellated curves: cylinder sides, spheres, the long
        // thin triangles booleans leave) never count, however narrow the faces.
        let strength = if angle > 0.7 {
            1.0
        } else {
            smoothstep(0.2, 0.42, angle) * smoothstep(15.0, 60.0, curvature)
        };
        if strength <= 0.0 {
            continue;
        }
        let convex = (centroids[g] - centroids[f]).dot(normals[f]) < 0.0;
        out.push(Edge {
            a: pa,
            b: pb,
            strength,
            convex,
        });
    }
    out
}

fn seg_dist(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Call `f(x, y, barycentric)` for every texel centre inside the triangle
/// (given in pixel coordinates), plus a one-texel border for conservative coverage.
fn raster(uv: [Vec2; 3], n: usize, mut f: impl FnMut(usize, usize, [f32; 3])) {
    let area = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]);
    if area.abs() < 1e-12 {
        return;
    }
    let lo = uv[0].min(uv[1]).min(uv[2]).floor().max(Vec2::ZERO);
    let hi = uv[0]
        .max(uv[1])
        .max(uv[2])
        .ceil()
        .min(Vec2::splat(n as f32 - 1.0));
    for y in lo.y as usize..=hi.y as usize {
        for x in lo.x as usize..=hi.x as usize {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let w0 = (uv[1] - p).perp_dot(uv[2] - p) / area;
            let w1 = (uv[2] - p).perp_dot(uv[0] - p) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 >= -1e-4 && w1 >= -1e-4 && w2 >= -1e-4 {
                f(x, y, [w0, w1, w2]);
            }
        }
    }
}

fn dilate(surf: &mut [Option<Surface>], n: usize, passes: usize) {
    for _ in 0..passes {
        let prev = surf.to_vec();
        for y in 0..n {
            for x in 0..n {
                if prev[y * n + x].is_some() {
                    continue;
                }
                let mut acc: Option<(Surface, f32)> = None;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= n as i32 || ny >= n as i32 {
                        continue;
                    }
                    if let Some(s) = prev[ny as usize * n + nx as usize] {
                        acc = Some(match acc {
                            None => (s, 1.0),
                            Some((a, k)) => (
                                Surface {
                                    color: a.color + s.color,
                                    metallic: a.metallic + s.metallic,
                                    roughness: a.roughness + s.roughness,
                                    height: a.height + s.height,
                                },
                                k + 1.0,
                            ),
                        });
                    }
                }
                if let Some((a, k)) = acc {
                    surf[y * n + x] = Some(Surface {
                        color: a.color / k,
                        metallic: a.metallic / k,
                        roughness: a.roughness / k,
                        height: a.height / k,
                    });
                }
            }
        }
    }
}

// ---------- Noise ----------

fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Smooth value noise in [0, 1].
fn noise(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash(x + dx, y + dy, z + dz);
    lerp(
        lerp(
            lerp(c(0, 0, 0), c(1, 0, 0), u.x),
            lerp(c(0, 1, 0), c(1, 1, 0), u.x),
            u.y,
        ),
        lerp(
            lerp(c(0, 0, 1), c(1, 0, 1), u.x),
            lerp(c(0, 1, 1), c(1, 1, 1), u.x),
            u.y,
        ),
        u.z,
    )
}

/// Fractal noise in roughly [0, 1].
pub(crate) fn fbm(p: Vec3, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for _ in 0..octaves {
        sum += noise(p * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bevel::bevel;
    use crate::primitives::cube;
    use crate::uv::{unwrap, UnwrapOptions};

    fn crate_mesh() -> PolyMesh {
        let mut m = bevel(&cube(1.0), 0.03, 30.0, 3).unwrap();
        unwrap(&mut m, &UnwrapOptions::default()).unwrap();
        m
    }

    #[test]
    fn needs_uvs() {
        assert!(bake(&cube(1.0), &SmartMaterial::new(Preset::Plastic), 64).is_err());
    }

    #[test]
    fn painted_metal_shows_metal_on_edges_only() {
        let m = crate_mesh();
        let mut mat = SmartMaterial::new(Preset::PaintedMetal);
        mat.wear = 0.8;
        mat.dirt = 0.0;
        let baked = bake(&m, &mat, 128).unwrap();
        // Average metallic over each face: face centres are paint, bevel strips are worn.
        let mut centre_metal = Vec::new();
        for f in m.faces.iter().take(6) {
            let c = f.uvs.iter().copied().sum::<Vec2>() / f.uvs.len() as f32;
            centre_metal.push(baked.metallic_roughness.sample(c).z);
        }
        // A scratch may cross a centre, but most centres are paint.
        let painted = centre_metal.iter().filter(|&&v| v < 0.3).count();
        assert!(
            painted >= 5,
            "face centres should be painted: {centre_metal:?}"
        );
        let metal_texels = baked
            .metallic_roughness
            .pixels
            .chunks(4)
            .filter(|p| p[2] > 128)
            .count();
        assert!(
            metal_texels > 100,
            "some edges should be worn to metal ({metal_texels})"
        );
    }

    #[test]
    fn every_preset_bakes() {
        let m = crate_mesh();
        for p in Preset::ALL {
            let b = bake(&m, &SmartMaterial::new(p), 32).unwrap();
            assert_eq!(b.base_color.pixels.len(), 32 * 32 * 4);
            assert_eq!(&b.normal.to_png()[1..4], b"PNG");
        }
    }

    #[test]
    fn noise_is_in_range() {
        for i in 0..1000 {
            let v = fbm(
                Vec3::new(i as f32 * 0.37, i as f32 * 0.11, -(i as f32) * 0.23),
                4,
            );
            assert!((0.0..=1.0).contains(&v));
        }
    }
}
