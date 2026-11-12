//! Headless software renderer for previews.
//!
//! The agent calls `render_preview` to look at its own work. This renderer runs
//! on the CPU, so it works on every machine and in CI with no GPU, and the same
//! scene always gives the same image.
//!
//! Studio-style shading: image-based reflections from a few soft boxes (what
//! makes metal read as metal), a shadow-mapped key light, and an invisible
//! floor that catches the shadow and a contact shadow so objects sit on the
//! ground.

use glam::{Mat4, Vec2, Vec3, Vec4, Vec4Swizzles};
use meshmind_core::texture::srgb_to_linear;
use meshmind_core::{LightingParams, Material, Scene};

/// A named camera direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Front,
    Back,
    Left,
    Right,
    Top,
    ThreeQuarter,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Front,
        View::Back,
        View::Left,
        View::Right,
        View::Top,
        View::ThreeQuarter,
    ];

    pub fn name(self) -> &'static str {
        match self {
            View::Front => "front",
            View::Back => "back",
            View::Left => "left",
            View::Right => "right",
            View::Top => "top",
            View::ThreeQuarter => "three_quarter",
        }
    }

    pub fn from_name(name: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.name() == name)
    }

    /// Direction from the target towards the camera, and the camera's up vector.
    fn eye_dir_up(self) -> (Vec3, Vec3) {
        match self {
            View::Front => (Vec3::Z, Vec3::Y),
            View::Back => (-Vec3::Z, Vec3::Y),
            View::Left => (-Vec3::X, Vec3::Y),
            View::Right => (Vec3::X, Vec3::Y),
            View::Top => (Vec3::Y, -Vec3::Z),
            View::ThreeQuarter => (Vec3::new(1.0, 0.75, 1.3).normalize(), Vec3::Y),
        }
    }
}

/// An RGBA8 image.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(&self.pixels).expect("png data");
        w.finish().expect("png finish");
        out
    }

    fn blit(&mut self, src: &Image, x0: u32, y0: u32) {
        for y in 0..src.height {
            let s = (y * src.width * 4) as usize;
            let d = (((y0 + y) * self.width + x0) * 4) as usize;
            self.pixels[d..d + (src.width * 4) as usize]
                .copy_from_slice(&src.pixels[s..s + (src.width * 4) as usize]);
        }
    }
}

struct Tri {
    world: [Vec3; 3],
    normal: [Vec3; 3],
    uv: [Vec2; 3],
    /// World-space tangent and bitangent from the UV layout, for normal maps.
    tangent: Vec3,
    bitangent: Vec3,
    material: usize,
}

/// Material index for the shadow-catching floor.
const FLOOR: usize = usize::MAX;

/// An orthographic depth map: the nearest depth seen along `dir` per texel.
struct DepthMap {
    res: usize,
    origin: Vec3,
    u: Vec3,
    v: Vec3,
    dir: Vec3,
    /// World units per texel.
    texel: f32,
    depth: Vec<f32>,
}

impl DepthMap {
    /// Looking along `dir`, covering a square of `half` around `origin`.
    fn build(tris: &[Tri], origin: Vec3, dir: Vec3, half: f32, res: usize) -> DepthMap {
        let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
        let u = dir.cross(up).normalize();
        let v = u.cross(dir).normalize();
        let texel = 2.0 * half / res as f32;
        let mut map = DepthMap {
            res,
            origin,
            u,
            v,
            dir,
            texel,
            depth: vec![f32::INFINITY; res * res],
        };
        for t in tris {
            let q = t.world.map(|p| map.project(p));
            let area =
                (q[1].x - q[0].x) * (q[2].y - q[0].y) - (q[2].x - q[0].x) * (q[1].y - q[0].y);
            if area.abs() < 1e-12 {
                continue;
            }
            let lo = q[0].min(q[1]).min(q[2]);
            let hi = q[0].max(q[1]).max(q[2]);
            let (x0, x1) = (
                lo.x.floor().max(0.0) as usize,
                (hi.x.ceil() as i64).clamp(0, res as i64 - 1) as usize,
            );
            let (y0, y1) = (
                lo.y.floor().max(0.0) as usize,
                (hi.y.ceil() as i64).clamp(0, res as i64 - 1) as usize,
            );
            if lo.x >= res as f32 || lo.y >= res as f32 || hi.x < 0.0 || hi.y < 0.0 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((q[1].x - px) * (q[2].y - py) - (q[2].x - px) * (q[1].y - py)) / area;
                    let w1 = ((q[2].x - px) * (q[0].y - py) - (q[0].x - px) * (q[2].y - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-3 || w1 < -1e-3 || w2 < -1e-3 {
                        continue;
                    }
                    let d = w0 * q[0].z + w1 * q[1].z + w2 * q[2].z;
                    let i = y * res + x;
                    if d < map.depth[i] {
                        map.depth[i] = d;
                    }
                }
            }
        }
        map
    }

    /// Texel x, texel y, depth along `dir`.
    fn project(&self, p: Vec3) -> Vec3 {
        let r = p - self.origin;
        let h = self.res as f32 / 2.0;
        Vec3::new(
            r.dot(self.u) / self.texel + h,
            r.dot(self.v) / self.texel + h,
            r.dot(self.dir),
        )
    }

    fn at(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= self.res as i64 || y >= self.res as i64 {
            return f32::INFINITY;
        }
        self.depth[y as usize * self.res + x as usize]
    }

    /// 0 = fully lit, 1 = fully shadowed, with a soft percentage-closer edge.
    fn shadow(&self, p: Vec3, bias: f32, soft: i64) -> f32 {
        let q = self.project(p);
        let (cx, cy) = (q.x.floor() as i64, q.y.floor() as i64);
        let mut hit = 0;
        let mut count = 0;
        for dy in -soft..=soft {
            for dx in -soft..=soft {
                count += 1;
                if self.at(cx + dx, cy + dy) < q.z - bias {
                    hit += 1;
                }
            }
        }
        hit as f32 / count as f32
    }
}

/// Render one view of the scene at `size` × `size` pixels.
pub fn render_view(scene: &Scene, view: View, size: u32) -> Image {
    render_view_focused(scene, view, size, None)
}

/// Like [`render_view`], but frames the camera on the objects in `focus` (a
/// close-up) while still drawing the whole scene.
pub fn render_view_focused(scene: &Scene, view: View, size: u32, focus: Option<&[u32]>) -> Image {
    const SS: u32 = 2; // supersampling factor for anti-aliasing
    let n = size * SS;

    let materials: Vec<Material> = scene.materials().map(|(_, m)| m.clone()).collect();
    let material_ids: Vec<u32> = scene.materials().map(|(id, _)| id).collect();

    let mut tris = Vec::new();
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    let (mut flo, mut fhi) = (lo, hi);
    for obj in scene.objects() {
        let focused = focus.is_some_and(|f| f.contains(&obj.id));
        let m = obj.transform.matrix();
        let nm = m.matrix3.inverse().transpose();
        let mesh = obj.mesh.to_mesh();
        let mat = material_ids
            .iter()
            .position(|&id| id == obj.material)
            .unwrap_or(0);
        for t in mesh.indices.chunks_exact(3) {
            let idx = [t[0] as usize, t[1] as usize, t[2] as usize];
            let world = idx.map(|i| m.transform_point3(mesh.positions[i]));
            let normal = idx.map(|i| (nm * mesh.normals[i]).normalize_or_zero());
            for p in world {
                lo = lo.min(p);
                hi = hi.max(p);
                if focused {
                    flo = flo.min(p);
                    fhi = fhi.max(p);
                }
            }
            let uv = idx.map(|i| mesh.uvs[i]);
            let (e1, e2) = (world[1] - world[0], world[2] - world[0]);
            let (d1, d2) = (uv[1] - uv[0], uv[2] - uv[0]);
            let det = d1.x * d2.y - d2.x * d1.y;
            let (tangent, bitangent) = if det.abs() > 1e-12 {
                ((e1 * d2.y - e2 * d1.y) / det, (e2 * d1.x - e1 * d2.x) / det)
            } else {
                (Vec3::ZERO, Vec3::ZERO)
            };
            tris.push(Tri {
                world,
                normal,
                uv,
                tangent,
                bitangent,
                material: mat,
            });
        }
    }

    let light = scene.lighting.params();
    let mut color = vec![Vec3::ZERO; (n * n) as usize];
    // Background: soft vertical gradient.
    for y in 0..n {
        let t = y as f32 / n as f32;
        let c = Vec3::from(light.background_top).lerp(Vec3::from(light.background_bottom), t);
        for x in 0..n {
            color[(y * n + x) as usize] = c;
        }
    }

    if !tris.is_empty() {
        let center = (lo + hi) / 2.0;
        let radius = ((hi - lo).length() / 2.0).max(1e-3);
        // The camera frames the focused objects, or everything.
        let (cam_center, cam_radius) = if flo.x.is_finite() {
            ((flo + fhi) / 2.0, ((fhi - flo).length() / 2.0).max(1e-3))
        } else {
            (center, radius)
        };
        let fov = 35f32.to_radians();
        let dist = cam_radius / (fov / 2.0).sin() * 1.05;
        let (dir, up) = view.eye_dir_up();
        let eye = cam_center + dir * dist;
        let view_m = Mat4::look_at_rh(eye, cam_center, up);
        let proj = Mat4::perspective_rh(
            fov,
            1.0,
            (dist - cam_radius * 1.5).max(dist * 0.01),
            dist + radius * 8.0,
        );
        let vp = proj * view_m;

        // Lights follow the camera so every view is readable.
        let cam_to_world = view_m.inverse();
        let mut key = cam_to_world
            .transform_vector3(Vec3::from(light.key_direction))
            .normalize();
        // Keep the key above the horizon so shadows fall on the floor.
        if key.y < 0.35 {
            key = Vec3::new(key.x, 0.0, key.z).normalize_or(Vec3::X) * 0.94 + Vec3::Y * 0.35;
            key = key.normalize();
        }
        let shadow_map = DepthMap::build(&tris, center, -key, radius * 1.05, 1024);
        let shadow_bias = radius * 0.012;
        // Lowest point of the object above each spot of the floor, for the contact shadow.
        let floor_y = lo.y;
        let contact_half = radius * 1.6;
        let contact = DepthMap::build(
            &tris,
            Vec3::new(center.x, floor_y, center.z),
            Vec3::Y,
            contact_half,
            192,
        );
        // The floor: a grid, so the part behind the camera can be skipped.
        let floor_half = radius * 6.0;
        let cells = 24;
        let cell = 2.0 * floor_half / cells as f32;
        for gz in 0..cells {
            for gx in 0..cells {
                let x0 = center.x - floor_half + gx as f32 * cell;
                let z0 = center.z - floor_half + gz as f32 * cell;
                let c = [
                    Vec3::new(x0, floor_y, z0),
                    Vec3::new(x0, floor_y, z0 + cell),
                    Vec3::new(x0 + cell, floor_y, z0 + cell),
                    Vec3::new(x0 + cell, floor_y, z0),
                ];
                for world in [[c[0], c[1], c[2]], [c[0], c[2], c[3]]] {
                    tris.push(Tri {
                        world,
                        normal: [Vec3::Y; 3],
                        uv: [Vec2::ZERO; 3],
                        tangent: Vec3::ZERO,
                        bitangent: Vec3::ZERO,
                        material: FLOOR,
                    });
                }
            }
        }
        let to_cam = |d: Vec3| view_m.transform_vector3(d);
        let fill = cam_to_world
            .transform_vector3(Vec3::new(0.7, 0.1, 0.4))
            .normalize();

        let mut depth = vec![f32::INFINITY; (n * n) as usize];
        for tri in &tris {
            let clip = tri.world.map(|p| vp * Vec4::new(p.x, p.y, p.z, 1.0));
            if clip.iter().any(|c| c.w <= 1e-6) {
                continue;
            }
            let ndc = clip.map(|c| c.xyz() / c.w);
            let sx = ndc.map(|p| (p.x * 0.5 + 0.5) * n as f32);
            let sy = ndc.map(|p| (1.0 - (p.y * 0.5 + 0.5)) * n as f32);
            let area = (sx[1] - sx[0]) * (sy[2] - sy[0]) - (sx[2] - sx[0]) * (sy[1] - sy[0]);
            if area.abs() < 1e-9 {
                continue;
            }
            let min_x = sx
                .iter()
                .cloned()
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.0) as u32;
            let max_x = (sx.iter().cloned().fold(f32::NEG_INFINITY, f32::max).ceil() as i64)
                .clamp(0, n as i64 - 1) as u32;
            let min_y = sy
                .iter()
                .cloned()
                .fold(f32::INFINITY, f32::min)
                .floor()
                .max(0.0) as u32;
            let max_y = (sy.iter().cloned().fold(f32::NEG_INFINITY, f32::max).ceil() as i64)
                .clamp(0, n as i64 - 1) as u32;
            let inv_w = clip.map(|c| 1.0 / c.w);
            let is_floor = tri.material == FLOOR;
            let mat = if is_floor {
                &materials[0]
            } else {
                &materials[tri.material]
            };
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((sx[1] - px) * (sy[2] - py) - (sx[2] - px) * (sy[1] - py)) / area;
                    let w1 = ((sx[2] - px) * (sy[0] - py) - (sx[0] - px) * (sy[2] - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    // A little slack closes hairline cracks between neighbouring triangles.
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let z = w0 * ndc[0].z + w1 * ndc[1].z + w2 * ndc[2].z;
                    let i = (y * n + x) as usize;
                    if z >= depth[i] {
                        continue;
                    }
                    depth[i] = z;
                    // Perspective-correct interpolation.
                    let (a, b, c) = (w0 * inv_w[0], w1 * inv_w[1], w2 * inv_w[2]);
                    let s = a + b + c;
                    let pos = (tri.world[0] * a + tri.world[1] * b + tri.world[2] * c) / s;
                    let mut nrm = ((tri.normal[0] * a + tri.normal[1] * b + tri.normal[2] * c) / s)
                        .normalize_or_zero();
                    if is_floor {
                        // Invisible floor: the background, darkened by shadows.
                        let from_center = Vec2::new(pos.x - center.x, pos.z - center.z).length();
                        let fade = 1.0 - smooth(radius * 1.2, radius * 4.5, from_center);
                        let shadow = shadow_map.shadow(pos, shadow_bias, 2);
                        let mut near = 0.0f32;
                        let q = contact.project(pos);
                        let k = 4;
                        let mut wsum = 0.0;
                        for dy in -k..=k {
                            for dx in -k..=k {
                                let w = 1.0 / (1.0 + (dx * dx + dy * dy) as f32);
                                wsum += w;
                                let h = contact.at(q.x as i64 + dx, q.y as i64 + dy);
                                if h.is_finite() {
                                    near += w * (-(h - floor_y).max(0.0) / (radius * 0.06)).exp();
                                }
                            }
                        }
                        let near = near / wsum;
                        let bg = color[i];
                        let dark = (shadow * 0.55 * (light.key_intensity / 2.0).min(1.0)
                            + near * 0.7)
                            .min(0.85);
                        color[i] = bg * (1.0 - dark * fade);
                        continue;
                    }
                    let uv = (tri.uv[0] * a + tri.uv[1] * b + tri.uv[2] * c) / s;
                    let to_eye = (eye - pos).normalize();
                    if nrm.dot(to_eye) < 0.0 {
                        nrm = -nrm; // show back faces of open meshes
                    }
                    let mut surface = Surface {
                        base: Vec3::new(mat.base_color[0], mat.base_color[1], mat.base_color[2]),
                        metallic: mat.metallic,
                        roughness: mat.roughness,
                    };
                    if let Some(tex) = &mat.textures {
                        let b = tex.base_color.sample(uv);
                        surface.base *= Vec3::new(
                            srgb_to_linear(b.x),
                            srgb_to_linear(b.y),
                            srgb_to_linear(b.z),
                        );
                        let mr = tex.metallic_roughness.sample(uv);
                        surface.roughness *= mr.y;
                        surface.metallic *= mr.z;
                        let tn = tex.normal.sample(uv).xyz() * 2.0 - Vec3::ONE;
                        // Gram-Schmidt the UV tangent frame against the shading normal.
                        let t = (tri.tangent - nrm * nrm.dot(tri.tangent)).normalize_or_zero();
                        if t != Vec3::ZERO {
                            let sign = if nrm.cross(t).dot(tri.bitangent) < 0.0 {
                                -1.0
                            } else {
                                1.0
                            };
                            // Green points to the top of the image, i.e. towards smaller v.
                            let bt = -nrm.cross(t) * sign;
                            nrm = (t * tn.x + bt * tn.y + nrm * tn.z).normalize_or(nrm);
                        }
                    }
                    let lit = 1.0 - shadow_map.shadow(pos, shadow_bias, 1);
                    let r = (2.0 * nrm.dot(to_eye) * nrm - to_eye).normalize_or(nrm);
                    let boxes = softboxes(to_cam(r), surface.roughness)
                        * Vec3::from(light.key_color)
                        * light.key_intensity
                        * 0.6;
                    color[i] = shade(&surface, &light, nrm, to_eye, key, fill, lit, boxes)
                        + Vec3::from(mat.emissive);
                }
            }
        }
    }

    // Downsample and convert to sRGB.
    let mut img = Image::new(size, size);
    for y in 0..size {
        for x in 0..size {
            let mut sum = Vec3::ZERO;
            for dy in 0..SS {
                for dx in 0..SS {
                    sum += color[((y * SS + dy) * n + x * SS + dx) as usize];
                }
            }
            let c = tone_map(sum / (SS * SS) as f32);
            let o = ((y * size + x) * 4) as usize;
            img.pixels[o..o + 4].copy_from_slice(&[
                to_srgb8(c.x),
                to_srgb8(c.y),
                to_srgb8(c.z),
                255,
            ]);
        }
    }
    img
}

struct Surface {
    base: Vec3,
    metallic: f32,
    roughness: f32,
}

/// Sky seen in direction `d`: used for ambient light and reflections.
fn environment(light: &LightingParams, d: Vec3) -> Vec3 {
    let sky = Vec3::from(light.sky);
    let ground = Vec3::from(light.ground);
    let horizon = (sky + ground) * 0.5 + Vec3::splat(0.08);
    let t = d.y.clamp(-1.0, 1.0);
    let c = if t > 0.0 {
        horizon.lerp(sky, t.powf(0.6))
    } else {
        horizon.lerp(ground, (-t).powf(0.4))
    };
    // A bright band around the horizon, like the curved wall of a photo
    // studio: long flat surfaces (blades, panels) reflect it as a gradient.
    let glow = (-(t / 0.25) * (t / 0.25)).exp() * 1.3;
    (c + Vec3::splat(glow) * horizon) * light.ambient_intensity * 2.2
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Studio soft boxes seen in camera-space direction `d` (x right, y up, z
/// towards the viewer), blurred by roughness. A large key box up and to the
/// left, a tall strip on the right and a strip overhead, so every view of a
/// metal surface shows crisp, readable highlights like a product photo.
fn softboxes(d: Vec3, rough: f32) -> Vec3 {
    let blur = 0.04 + rough * rough * 0.9;
    let damp = 1.0 / (1.0 + rough * rough * 6.0);
    let panel = |center: Vec3, half_w: f32, half_h: f32, strength: f32| {
        let c = center.normalize();
        let cos = d.dot(c);
        if cos <= 0.0 {
            return 0.0;
        }
        // Gnomonic projection onto the panel's plane.
        let right = Vec3::Y.cross(c).normalize_or(Vec3::X);
        let up = c.cross(right);
        let p = d / cos;
        let (x, y) = (p.dot(right).abs(), p.dot(up).abs());
        let mx = 1.0 - smooth(half_w - blur, half_w + blur, x);
        let my = 1.0 - smooth(half_h - blur, half_h + blur, y);
        mx * my * strength
    };
    let k = panel(Vec3::new(-0.6, 0.55, 0.6), 0.32, 0.22, 1.0)
        + panel(Vec3::new(0.85, 0.05, 0.25), 0.07, 0.6, 0.8)
        + panel(Vec3::new(0.0, 1.0, -0.15), 0.6, 0.06, 0.6)
        // A wide, low-hanging panel behind the camera: flat faces turned
        // towards the viewer show its edge as a light/dark split.
        + panel(Vec3::new(0.0, 0.42, 1.0), 1.4, 0.16, 0.3);
    Vec3::splat(k * damp)
}

#[allow(clippy::too_many_arguments)]
fn shade(
    m: &Surface,
    light: &LightingParams,
    n: Vec3,
    v: Vec3,
    key: Vec3,
    fill: Vec3,
    key_lit: f32,
    boxes: Vec3,
) -> Vec3 {
    let base = m.base;
    let metallic = m.metallic.clamp(0.0, 1.0);
    let rough = m.roughness.clamp(0.04, 1.0);
    let diffuse = base * (1.0 - metallic);
    let f0 = Vec3::splat(0.04).lerp(base, metallic);

    // Sky/ground hemisphere ambient.
    let sky = Vec3::from(light.sky);
    let ground = Vec3::from(light.ground);
    let ambient = ground.lerp(sky, n.y * 0.5 + 0.5) * light.ambient_intensity;

    let mut out = diffuse * ambient;
    let n_dot_v = n.dot(v).max(0.0);
    // Reflection of the environment, blurred towards the ambient average by roughness.
    let r = (2.0 * n.dot(v) * n - v).normalize_or(n);
    // Studio reflections are mostly dark with bright boxes, which is what
    // gives polished metal its contrast; rough surfaces average it out.
    let env = (environment(light, r) * (0.3 + 0.7 * rough)).lerp(ambient * 1.6, rough * rough);
    let fresnel_env = f0 + (Vec3::ONE - f0) * (1.0 - n_dot_v).powi(5) * (1.0 - rough);
    out += fresnel_env * (env + boxes);

    let shininess = (2.0 / (rough * rough * rough * rough) - 2.0).clamp(1.0, 2048.0);
    for (l, color, intensity) in [
        (
            key,
            Vec3::from(light.key_color),
            light.key_intensity * key_lit,
        ),
        (fill, Vec3::from(light.fill_color), light.fill_intensity),
    ] {
        let n_dot_l = n.dot(l).max(0.0);
        if n_dot_l <= 0.0 {
            continue;
        }
        let h = (l + v).normalize();
        let spec =
            n.dot(h).max(0.0).powf(shininess) * (shininess + 8.0) / (8.0 * std::f32::consts::PI);
        let fresnel = f0 + (Vec3::ONE - f0) * (1.0 - h.dot(v).max(0.0)).powi(5);
        out += (diffuse * 0.75 + fresnel * spec) * n_dot_l * intensity * color;
    }
    // Rim light to separate the silhouette from the background.
    out += Vec3::splat((1.0 - n_dot_v).powi(3) * 0.15);
    out * light.exposure
}

fn tone_map(c: Vec3) -> Vec3 {
    // Narkowicz ACES approximation.
    let (a, b, cc, d, e) = (2.51, 0.03, 2.43, 0.59, 0.14);
    ((c * (c * a + b)) / (c * (c * cc + d) + e)).clamp(Vec3::ZERO, Vec3::ONE)
}

fn to_srgb8(c: f32) -> u8 {
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Render several views side by side in a grid (2 columns).
pub fn render_sheet(scene: &Scene, views: &[View], size: u32) -> Image {
    render_sheet_focused(scene, views, size, None)
}

/// Like [`render_sheet`], framed on the objects in `focus`.
pub fn render_sheet_focused(
    scene: &Scene,
    views: &[View],
    size: u32,
    focus: Option<&[u32]>,
) -> Image {
    let cols = views.len().clamp(1, 2) as u32;
    let rows = (views.len() as u32).div_ceil(cols).max(1);
    let gap = 4;
    let mut sheet = Image::new(
        cols * size + (cols - 1) * gap,
        rows * size + (rows - 1) * gap,
    );
    for (i, &v) in views.iter().enumerate() {
        let img = render_view_focused(scene, v, size, focus);
        let (c, r) = (i as u32 % cols, i as u32 / cols);
        sheet.blit(&img, c * (size + gap), r * (size + gap));
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;
    use meshmind_core::primitives::{cube, uv_sphere};

    fn pixel(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * img.width + x) * 4) as usize;
        img.pixels[o..o + 4].try_into().unwrap()
    }

    #[test]
    fn renders_an_object_in_the_middle() {
        let mut scene = Scene::new();
        let id = scene.add_object("Ball", uv_sphere(1.0, 24, 12)).unwrap();
        let red = scene.add_material(Material {
            base_color: [0.8, 0.05, 0.05, 1.0],
            ..Default::default()
        });
        scene.assign_material(id, red).unwrap();
        let img = render_view(&scene, View::Front, 64);
        let [r, g, b, _] = pixel(&img, 32, 32);
        assert!(
            r > g + 40 && r > b + 40,
            "centre should be red, got {r} {g} {b}"
        );
        let corner = pixel(&img, 1, 1);
        assert!(
            corner[0].abs_diff(corner[2]) < 20,
            "corner should be grey background"
        );
    }

    #[test]
    fn sheet_and_png() {
        let mut scene = Scene::new();
        scene.add_object("Box", cube(1.0)).unwrap();
        let sheet = render_sheet(
            &scene,
            &[View::Front, View::Right, View::Top, View::ThreeQuarter],
            32,
        );
        assert_eq!((sheet.width, sheet.height), (68, 68));
        assert_eq!(&sheet.to_png()[1..4], b"PNG");
    }

    #[test]
    fn empty_scene_renders_background() {
        let img = render_view(&Scene::new(), View::Top, 16);
        assert_eq!(img.pixels.len(), 16 * 16 * 4);
    }
}
