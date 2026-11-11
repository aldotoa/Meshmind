//! Headless software renderer for previews.
//!
//! The agent calls `render_preview` to look at its own work. This renderer runs
//! on the CPU, so it works on every machine and in CI with no GPU, and the same
//! scene always gives the same image.

use glam::{Mat4, Vec3, Vec4, Vec4Swizzles};
use meshmind_core::{Material, Scene};

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
    material: usize,
}

/// Render one view of the scene at `size` × `size` pixels.
pub fn render_view(scene: &Scene, view: View, size: u32) -> Image {
    const SS: u32 = 2; // supersampling factor for anti-aliasing
    let n = size * SS;

    let materials: Vec<Material> = scene.materials().map(|(_, m)| m.clone()).collect();
    let material_ids: Vec<u32> = scene.materials().map(|(id, _)| id).collect();

    let mut tris = Vec::new();
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for obj in scene.objects() {
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
            }
            tris.push(Tri {
                world,
                normal,
                material: mat,
            });
        }
    }

    let mut color = vec![Vec3::ZERO; (n * n) as usize];
    // Background: soft vertical gradient.
    for y in 0..n {
        let t = y as f32 / n as f32;
        let c = Vec3::new(0.16, 0.17, 0.2).lerp(Vec3::new(0.05, 0.05, 0.06), t);
        for x in 0..n {
            color[(y * n + x) as usize] = c;
        }
    }

    if !tris.is_empty() {
        let center = (lo + hi) / 2.0;
        let radius = ((hi - lo).length() / 2.0).max(1e-3);
        let fov = 35f32.to_radians();
        let dist = radius / (fov / 2.0).sin() * 1.05;
        let (dir, up) = view.eye_dir_up();
        let eye = center + dir * dist;
        let view_m = Mat4::look_at_rh(eye, center, up);
        let proj = Mat4::perspective_rh(
            fov,
            1.0,
            (dist - radius * 1.5).max(dist * 0.01),
            dist + radius * 1.5,
        );
        let vp = proj * view_m;

        // Lights follow the camera so every view is readable.
        let cam_to_world = view_m.inverse();
        let key = cam_to_world
            .transform_vector3(Vec3::new(-0.5, 0.7, 0.6))
            .normalize();
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
            let mat = &materials[tri.material];
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
                    let to_eye = (eye - pos).normalize();
                    if nrm.dot(to_eye) < 0.0 {
                        nrm = -nrm; // show back faces of open meshes
                    }
                    color[i] = shade(mat, nrm, to_eye, key, fill);
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

fn shade(m: &Material, n: Vec3, v: Vec3, key: Vec3, fill: Vec3) -> Vec3 {
    let base = Vec3::new(m.base_color[0], m.base_color[1], m.base_color[2]);
    let metallic = m.metallic.clamp(0.0, 1.0);
    let rough = m.roughness.clamp(0.04, 1.0);
    let diffuse = base * (1.0 - metallic);
    let f0 = Vec3::splat(0.04).lerp(base, metallic);

    // Sky/ground hemisphere ambient.
    let sky = Vec3::new(0.55, 0.6, 0.7);
    let ground = Vec3::new(0.2, 0.18, 0.16);
    let ambient = ground.lerp(sky, n.y * 0.5 + 0.5) * 0.45;

    let mut out = diffuse * ambient;
    let n_dot_v = n.dot(v).max(0.0);
    let fresnel_env = f0 + (Vec3::ONE - f0) * (1.0 - n_dot_v).powi(5) * (1.0 - rough);
    out += fresnel_env * ambient * 2.2;

    let shininess = (2.0 / (rough * rough * rough * rough) - 2.0).clamp(1.0, 2048.0);
    for (l, intensity) in [(key, 2.0f32), (fill, 0.5)] {
        let n_dot_l = n.dot(l).max(0.0);
        if n_dot_l <= 0.0 {
            continue;
        }
        let h = (l + v).normalize();
        let spec =
            n.dot(h).max(0.0).powf(shininess) * (shininess + 8.0) / (8.0 * std::f32::consts::PI);
        let fresnel = f0 + (Vec3::ONE - f0) * (1.0 - h.dot(v).max(0.0)).powi(5);
        out += (diffuse * 0.75 + fresnel * spec) * n_dot_l * intensity;
    }
    // Rim light to separate the silhouette from the background.
    out += Vec3::splat((1.0 - n_dot_v).powi(3) * 0.15);
    out + Vec3::from(m.emissive)
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
    let cols = views.len().clamp(1, 2) as u32;
    let rows = (views.len() as u32).div_ceil(cols).max(1);
    let gap = 4;
    let mut sheet = Image::new(
        cols * size + (cols - 1) * gap,
        rows * size + (rows - 1) * gap,
    );
    for (i, &v) in views.iter().enumerate() {
        let img = render_view(scene, v, size);
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
